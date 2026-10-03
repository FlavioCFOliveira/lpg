//! Recovery at open: TS-13 steps 4, 5 and 6. Step 8 (the open checkpoint)
//! runs in [`super::engine`]; step 7 (the old-version store) arrives with
//! MVCC in Sprint 2.
//!
//! - **Step 4.** The segments of the generation are checked against the
//!   redo point R: the segment holding R exists unless the log is empty
//!   after a clean shutdown; segments from there on form one chain; and the
//!   last frame position of the newest segment holds no valid frame, since
//!   the writer creates the next segment before it writes that frame
//!   (TS-09). A missing segment is corruption (FR-REC-008), as InnoDB
//!   refuses a broken chain of redo files (R-INNO
//!   `log/log0files_finder.cc:121-163, 350-355`).
//! - **Replay.** From R, each valid group's page images are written to the
//!   data file, sealed, extending the file first where a page lies beyond
//!   its end (TS-05). Pages are written straight to the file, before any
//!   page is read through the buffer pool, so a torn page is overwritten
//!   before anything reads it (TS-10).
//! - **Step 5.** Where the next group at X is not valid, the rest of the
//!   WAL is searched for a later valid group whose first LSN is above X and
//!   whose salt is at least the last applied group's. If one exists the WAL
//!   is corrupt; otherwise the group at X is a torn tail and the WAL ends at
//!   X.
//! - **Step 6.** The counters are the maximum of `CONTROL` and the applied
//!   commit frames.

use std::collections::BTreeMap;
use std::path::Path;

use super::Storage;
use super::StorageFile;
use super::control::Control;
use super::datafile::{Datafile, MAX_PAGES};
use super::error::{Error, Result};
use super::page::{self, PAGE_SIZE};
use super::wal::{
    self, Counters, FRAME_BYTES, FRAME_HEADER, FRAME_SIZE, KIND_COMMIT, KIND_PAGE, SEGMENT_FRAMES,
    SEGMENT_SPAN,
};

/// What recovery found and did.
pub(crate) struct Recovered {
    /// The WAL end: where the next group starts.
    pub(crate) wal_end: u64,
    /// The counters (step 6).
    pub(crate) counters: Counters,
    /// Whether the log was empty: no segment holds the redo point.
    pub(crate) empty_log: bool,
    /// Groups replayed.
    pub(crate) groups: u64,
    /// Page images written to the data file.
    pub(crate) pages: u64,
    /// WAL bytes replayed: from the redo point to the WAL end.
    pub(crate) bytes: u64,
    /// The segment files of the generation, by first LSN.
    pub(crate) segments: BTreeMap<u64, Box<dyn StorageFile>>,
}

/// A valid group read from the WAL.
struct Group {
    salt: u64,
    counters: Counters,
    /// LSN after its commit frame.
    end: u64,
}

/// Reads frames by LSN from the chain of segments.
struct Reader<'a> {
    segments: &'a BTreeMap<u64, Box<dyn StorageFile>>,
    /// End of the frame stream: the end of the newest segment.
    end: u64,
}

impl Reader<'_> {
    /// Reads the frame at `lsn` into `buf` and returns its header if it is
    /// valid there.
    fn frame(&self, lsn: u64, buf: &mut [u8]) -> Result<Option<wal::FrameHead>> {
        let Some((&first, file)) = self.segments.range(..=lsn).next_back() else {
            return Ok(None);
        };
        if lsn + FRAME_BYTES > first + SEGMENT_SPAN {
            return Ok(None);
        }
        let n = file.read_at(buf, lsn - first)?;
        if n < FRAME_SIZE {
            return Ok(None);
        }
        Ok(wal::decode_frame(buf, lsn))
    }

    /// The valid group starting at `x` whose salt is at least `min_salt`,
    /// with its page frames left in `frames`, or `None` (TS-09 Valid group).
    fn group(&self, x: u64, min_salt: u64, frames: &mut Vec<u8>) -> Result<Option<Group>> {
        frames.clear();
        let mut buf = vec![0u8; FRAME_SIZE];
        let mut lsn = x;
        let mut salts = Vec::new();
        loop {
            if lsn + FRAME_BYTES > self.end {
                return Ok(None);
            }
            let Some(h) = self.frame(lsn, &mut buf)? else {
                return Ok(None);
            };
            match h.kind {
                KIND_PAGE => {
                    frames.extend_from_slice(&buf);
                    salts.push(h.salt);
                    lsn += FRAME_BYTES;
                }
                KIND_COMMIT => {
                    let r = wal::commit_record(&buf);
                    if r.first_lsn != x || h.salt < min_salt || salts.iter().any(|&s| s != h.salt) {
                        return Ok(None);
                    }
                    return Ok(Some(Group {
                        salt: h.salt,
                        counters: r.counters,
                        end: lsn + FRAME_BYTES,
                    }));
                }
                _ => return Ok(None),
            }
        }
    }

    /// Step 5: whether a valid group with first LSN above `x` and salt at
    /// least `min_salt` exists anywhere after `x`.
    fn later_valid_group(&self, x: u64, min_salt: u64) -> Result<Option<u64>> {
        let mut buf = vec![0u8; FRAME_SIZE];
        let mut frames = Vec::new();
        let mut q = x + FRAME_BYTES;
        while q + FRAME_BYTES <= self.end {
            if let Some(h) = self.frame(q, &mut buf)?
                && h.kind == KIND_COMMIT
                && h.salt >= min_salt
            {
                let first = wal::commit_record(&buf).first_lsn;
                if first > x && first <= q && self.group(first, min_salt, &mut frames)?.is_some() {
                    return Ok(Some(first));
                }
            }
            q += FRAME_BYTES;
        }
        Ok(None)
    }
}

/// Runs TS-13 steps 4–6 for generation `generation` at `location`, from
/// `control`'s redo point, writing replayed pages into `data`, whose header
/// has not been read yet.
///
/// # Errors
///
/// [`Error::Corruption`] for a missing segment (step 4) or a corrupt WAL
/// (step 5); ENOSPC if the data file cannot be extended; [`Error::Io`].
pub(crate) fn recover(
    storage: &dyn Storage,
    location: &Path,
    generation: u64,
    control: &Control,
    data: &Datafile,
) -> Result<Recovered> {
    let r = control.redo_point;
    let mut segments: BTreeMap<u64, Box<dyn StorageFile>> = BTreeMap::new();
    for name in storage.list(location)? {
        if let Some((g, lsn)) = wal::parse_segment_name(&name)
            && g == generation
        {
            segments.insert(lsn, storage.open(&location.join(&name))?);
        }
    }
    let base = Counters {
        commit_ts: control.commit_ts,
        next_element_id: control.next_element_id,
        next_catalog_id: control.next_catalog_id,
        next_token_id: control.next_token_id,
    };
    // Step 4: the segment holding R.
    let holders: Vec<u64> = segments
        .keys()
        .copied()
        .filter(|&f| f <= r && r < f + SEGMENT_SPAN)
        .collect();
    let holder = match holders[..] {
        [] => {
            if control.clean_shutdown && segments.keys().all(|&f| f <= r) {
                return Ok(Recovered {
                    wal_end: r,
                    counters: base,
                    empty_log: true,
                    groups: 0,
                    pages: 0,
                    bytes: 0,
                    segments,
                });
            }
            return Err(Error::Corruption(format!(
                "no WAL segment of generation {generation} holds the redo point {r:#x}"
            )));
        }
        [h] => h,
        _ => {
            return Err(Error::Corruption(format!(
                "WAL segments {holders:x?} overlap at the redo point {r:#x}"
            )));
        }
    };
    let mut expected = holder;
    for &f in segments.range(holder..).map(|(f, _)| f) {
        if f != expected {
            return Err(Error::Corruption(format!(
                "WAL segment {} is missing",
                wal::segment_name(generation, expected)
            )));
        }
        expected = f + SEGMENT_SPAN;
    }
    let newest = expected - SEGMENT_SPAN;
    let reader = Reader {
        segments: &segments,
        end: expected,
    };
    let mut buf = vec![0u8; FRAME_SIZE];
    let last = newest + (SEGMENT_FRAMES - 1) * FRAME_BYTES;
    if reader.frame(last, &mut buf)?.is_some() {
        return Err(Error::Corruption(format!(
            "WAL segment {} is missing: the last frame of the newest segment is valid",
            wal::segment_name(generation, expected)
        )));
    }
    // Replay.
    let mut x = r;
    let mut counters = base;
    let mut last_salt = None;
    let mut groups = 0;
    let mut pages = 0;
    let mut frames = Vec::new();
    let mut image = vec![0u8; PAGE_SIZE];
    while let Some(g) = reader.group(x, last_salt.unwrap_or(0), &mut frames)? {
        for f in frames.as_chunks::<FRAME_SIZE>().0 {
            let page_no = u64::from_le_bytes(f[24..32].try_into().expect("8 bytes"));
            // A valid frame can still name a page no data file can hold:
            // damage that its CRC-32C does not cover, or a frame written
            // by something else. SQLite rejects a frame with an impossible
            // page number before using it (R-SQLITE `src/wal.c:1053-1056`,
            // page 0); lpg logs page 0, so the bound is the file's size
            // limit.
            if page_no >= MAX_PAGES {
                return Err(Error::Corruption(format!(
                    "WAL frame of group {x:#x} names page {page_no}, beyond the largest data file"
                )));
            }
            image.copy_from_slice(&f[FRAME_HEADER..]);
            page::seal(&mut image, page_no);
            // `used` is 0 before the header is read, so this reserves
            // through `page_no` (TS-05 extension, TS-13 step 4).
            data.ensure(page_no + 1)?;
            data.write_page(page_no, &image)?;
            pages += 1;
        }
        counters = counters.max(g.counters);
        last_salt = Some(g.salt);
        groups += 1;
        x = g.end;
    }
    // Step 5.
    if let Some(first) = reader.later_valid_group(x, last_salt.unwrap_or(0))? {
        return Err(Error::Corruption(format!(
            "WAL corrupt: the group at LSN {x:#x} is not valid, but a later group at \
             LSN {first:#x} is"
        )));
    }
    Ok(Recovered {
        wal_end: x,
        counters,
        empty_log: false,
        groups,
        pages,
        bytes: x - r,
        segments,
    })
}

#[cfg(test)]
mod tests;
