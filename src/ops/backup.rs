//! Online physical backup (TS-26 backup steps 1-5; task #18).
//!
//! 1. A checkpoint runs and its redo point becomes `start`; WAL recycling
//!    is held at `start` before the checkpoint recycles anything
//!    ([`checkpoint::run_held`]), so every segment that holds an LSN at or
//!    above `start` stays until the hold is released.
//! 2. `CONTROL`, read under its latch, and every byte of `data-<g>` are
//!    copied to the target through the storage trait. Commits go on: a page
//!    written during the copy may be copied old, new or torn, but it was
//!    changed after `start` through a logged commit, so its full image is in
//!    the WAL copied next (TS-26).
//! 3. `stop` is the durable LSN, and the data file's length at `stop` is
//!    recorded. The WAL from `start` to `stop` is copied: one artefact file
//!    per segment that holds an LSN in `[start, stop]`, named as the
//!    segment and holding that segment's bytes of the range. Every commit
//!    durable at `stop` is therefore in the backup, and no later one.
//! 4. The hold is released.
//! 5. Every artefact file is synced, then the target; `MANIFEST` is written
//!    last, as `MANIFEST.tmp`, synced, renamed and the target synced again
//!    (the completion marker).
//!
//! A failed read of the database (its `CONTROL`, data file or WAL) is a
//! failed read like any other: it sets the unusable latch (TS-12). A failed
//! write to the target fails only the backup. A backup and a bulk build
//! exclude each other for their whole run
//! ([`crate::storage::engine::Core::backup_or_load`]): the pages a bulk
//! build writes bypass the WAL, so the backup's WAL copy could not cover
//! them. A backup requested while a build runs fails with
//! [`Error::LoadRunning`] before it writes anything (FR-OPS-001); a build
//! requested while a backup runs waits until the backup ends (TS-26).
//!
//! This is PostgreSQL's base backup: a checkpoint, a copy of the files
//! while the server runs, and the WAL from the backup's start to its stop
//! (R-PG `src/backend/access/transam/xlog.c`, `do_pg_backup_start` and
//! `do_pg_backup_stop`), with a manifest of the WAL range and a CRC-32C per
//! file (`src/backend/backup/basebackup.c:657-660, 729`), written durably
//! after the data (`src/bin/pg_basebackup/pg_basebackup.c:2298-2321`).
//! RocksDB's `Checkpoint` likewise copies the WAL into a flat target
//! directory (R-ROCKS `utilities/checkpoint/checkpoint_impl.cc:472-484`).
//!
//! **`MANIFEST`** layout, integers little-endian:
//!
//! | Size | Field |
//! |---|---|
//! | 8 | magic `LPGMANI\0` |
//! | 4 | layout version of this file, 1 |
//! | 4 | database format version |
//! | 16 | database UUID |
//! | 8 | generation `<g>` |
//! | 4 | page size |
//! | 8 | `start` |
//! | 8 | `stop` |
//! | 8 | length of `data-<g>` at `stop` |
//! | 4 | number of files |
//! | per file | name length (2), name, length (8), CRC-32C (4) |
//! | 4 | CRC-32C of every byte before it |

use std::path::Path;
use std::time::{Duration, Instant};

use super::{COPY_BUFFER, copy_range};
use crate::storage::engine::{CONTROL, Engine, data_name};
use crate::storage::error::{Error, Result};
use crate::storage::wal::{SEGMENT_SPAN, segment_name};
use crate::storage::{Storage, checkpoint};

/// Name of the completion marker.
pub(crate) const MANIFEST: &str = "MANIFEST";
/// Name `MANIFEST` is written under before its rename.
pub(crate) const MANIFEST_TMP: &str = "MANIFEST.tmp";

const MAGIC: [u8; 8] = *b"LPGMANI\0";
const LAYOUT: u32 = 1;

/// One artefact file listed in `MANIFEST`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileEntry {
    /// File name in the artefact.
    pub(crate) name: String,
    /// Length in bytes.
    pub(crate) len: u64,
    /// CRC-32C of its bytes.
    pub(crate) crc: u32,
}

/// The content of `MANIFEST`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Manifest {
    /// Database format version.
    pub(crate) format_version: u32,
    /// Database UUID.
    pub(crate) uuid: [u8; 16],
    /// Generation `<g>`.
    pub(crate) generation: u64,
    /// Page size.
    pub(crate) page_size: u32,
    /// Redo point of the backup's checkpoint.
    pub(crate) start: u64,
    /// Durable LSN when the WAL copy began.
    pub(crate) stop: u64,
    /// Length of `data-<g>` at `stop`.
    pub(crate) data_len: u64,
    /// Every other file, `CONTROL` and `data-<g>` first, then the segments
    /// in LSN order.
    pub(crate) files: Vec<FileEntry>,
}

impl Manifest {
    /// The bytes of `MANIFEST`.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC);
        b.extend_from_slice(&LAYOUT.to_le_bytes());
        b.extend_from_slice(&self.format_version.to_le_bytes());
        b.extend_from_slice(&self.uuid);
        b.extend_from_slice(&self.generation.to_le_bytes());
        b.extend_from_slice(&self.page_size.to_le_bytes());
        b.extend_from_slice(&self.start.to_le_bytes());
        b.extend_from_slice(&self.stop.to_le_bytes());
        b.extend_from_slice(&self.data_len.to_le_bytes());
        b.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        for f in &self.files {
            b.extend_from_slice(&(f.name.len() as u16).to_le_bytes());
            b.extend_from_slice(f.name.as_bytes());
            b.extend_from_slice(&f.len.to_le_bytes());
            b.extend_from_slice(&f.crc.to_le_bytes());
        }
        let crc = crc32c::crc32c(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    /// Decodes `MANIFEST`, or `None` if its checksum, magic or layout is
    /// wrong.
    pub(crate) fn decode(b: &[u8]) -> Option<Manifest> {
        let body = b.get(..b.len().checked_sub(4)?)?;
        let crc = u32::from_le_bytes(b[body.len()..].try_into().ok()?);
        if crc != crc32c::crc32c(body) {
            return None;
        }
        let mut r = Reader { b: body, at: 0 };
        if r.take(8)? != MAGIC || r.u32()? != LAYOUT {
            return None;
        }
        let mut m = Manifest {
            format_version: r.u32()?,
            uuid: r.take(16)?.try_into().ok()?,
            generation: r.u64()?,
            page_size: r.u32()?,
            start: r.u64()?,
            stop: r.u64()?,
            data_len: r.u64()?,
            files: Vec::new(),
        };
        for _ in 0..r.u32()? {
            let n = usize::from(u16::from_le_bytes(r.take(2)?.try_into().ok()?));
            let name = String::from_utf8(r.take(n)?.to_vec()).ok()?;
            m.files.push(FileEntry {
                name,
                len: r.u64()?,
                crc: r.u32()?,
            });
        }
        (r.at == body.len()).then_some(m)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

/// Figures of one backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupStats {
    /// The manifest written.
    pub(crate) manifest: Manifest,
    /// Bytes of WAL copied: `stop - start`.
    pub(crate) wal_bytes: u64,
    /// Time from the start of step 1 to the release of the hold.
    pub(crate) held: Duration,
}

/// Releases the recycling hold when dropped (step 4), also on failure.
struct Hold<'a> {
    engine: &'a Engine,
    at: u64,
}

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        self.engine.wal().release(self.at);
    }
}

impl Engine {
    /// Backs the open database up into `target`, an existing, empty
    /// directory of the database's storage (TS-26; module documentation).
    /// Commits continue meanwhile; a bulk build requested meanwhile waits
    /// for the backup.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `target` holds any entry, and
    /// [`Error::LoadRunning`] if a bulk build runs, both without waiting and
    /// before anything is written; [`Error::Unusable`]; errors of the checkpoint,
    /// which set the unusable latch; [`Error::Io`] for a failed read of the
    /// database, which sets the unusable latch (TS-12), or for a failed
    /// write to the target, which leaves the database usable. Either way the
    /// artefact has no `MANIFEST`.
    pub(crate) fn backup(&self, target: &Path) -> Result<BackupStats> {
        let core = self.core();
        core.health.check()?;
        let s: &dyn Storage = &*core.storage;
        if !s.list(target)?.is_empty() {
            return Err(Error::InvalidArgument("the backup target is not empty"));
        }
        // FR-OPS-001: fail, do not wait, while a bulk build holds the latch.
        let Some(_load) = core.try_backup_or_load() else {
            return Err(Error::LoadRunning);
        };
        let began = Instant::now();
        // A failed read of the database sets the unusable latch (TS-12).
        let read_failed = |e: Error| core.health.fail(e);
        // Step 1.
        let start = checkpoint::run_held(core)?.redo;
        let hold = Hold {
            engine: self,
            at: start,
        };
        #[cfg(test)]
        crate::storage::wal::TestPoints::run(&core.wal.points.backup_started);
        let mut buf = vec![0u8; COPY_BUFFER];
        let mut files = Vec::new();
        let mut out = Vec::new();
        // Step 2.
        let (control, raw) = {
            let c = core.control();
            (*c.current(), c.raw().map_err(read_failed)?)
        };
        let f = s.create(&target.join(CONTROL))?;
        f.write_at(&raw, 0)?;
        files.push(super::backup::FileEntry {
            name: CONTROL.to_owned(),
            len: raw.len() as u64,
            crc: crc32c::crc32c(&raw),
        });
        out.push(f);
        let g = control.generation;
        let data = data_name(g);
        let src = s
            .open(&core.location.join(&data))
            .map_err(|e| read_failed(e.into()))?;
        let len = src.len().map_err(|e| read_failed(e.into()))?;
        let f = s.create(&target.join(&data))?;
        let crc = copy_range(&*src, 0, &*f, 0, len, &mut buf, 0, &read_failed)?;
        files.push(FileEntry {
            name: data.clone(),
            len,
            crc,
        });
        out.push(f);
        // Step 3.
        let stop = core.wal.durable_lsn();
        let data_len = core.data.reserved_pages() * crate::storage::page::PAGE_BYTES;
        for first in core.wal.segment_lsns() {
            if first > stop || first + SEGMENT_SPAN <= start {
                continue;
            }
            let (from, to) = (start.max(first), stop.min(first + SEGMENT_SPAN));
            let name = segment_name(g, first);
            let seg = s
                .open(&core.wal.segment_path(first))
                .map_err(|e| read_failed(e.into()))?;
            let f = s.create(&target.join(&name))?;
            let crc = copy_range(
                &*seg,
                from - first,
                &*f,
                0,
                to - from,
                &mut buf,
                0,
                &read_failed,
            )?;
            files.push(FileEntry {
                name,
                len: to - from,
                crc,
            });
            out.push(f);
        }
        // Step 4.
        drop(hold);
        let held = began.elapsed();
        // Step 5.
        for f in &out {
            f.sync_data()?;
        }
        s.sync_dir(target)?;
        let manifest = Manifest {
            format_version: control.format_version,
            uuid: control.uuid,
            generation: g,
            page_size: control.page_size,
            start,
            stop,
            data_len: data_len.max(len),
            files,
        };
        let bytes = manifest.encode();
        let tmp = target.join(MANIFEST_TMP);
        let f = s.create(&tmp)?;
        f.write_at(&bytes, 0)?;
        f.sync_data()?;
        drop(f);
        s.rename(&tmp, &target.join(MANIFEST))?;
        s.sync_dir(target)?;
        Ok(BackupStats {
            manifest,
            wal_bytes: stop - start,
            held,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
