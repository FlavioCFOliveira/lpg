//! Format migration from N−1 to N with the generation switch (TS-28 steps
//! 1-6; task #19), on a closed location.
//!
//! 1. Take `LOCK`; if the database is open, fail.
//! 2. If `CONTROL` does not show a clean shutdown, fail, naming the crate
//!    version that wrote the database, with which it must be opened and
//!    closed first.
//! 3. Remove the files of other generations, left by an interrupted
//!    migration; then, if `available_space` is below the size of `data-<g>`
//!    plus its WAL segments, fail with nothing else changed.
//! 4. Write `data-<g+1>` in format N: every page below the page count is
//!    read, checked with the old format's rule, converted and written with
//!    its page LSN unchanged; the file is reserved to the old file's
//!    length. `sync_data`, then `sync_dir`.
//! 5. Write the next `CONTROL` slot with generation `<g+1>`, version N and
//!    the same redo point, clean-shutdown flag set: this one slot write is
//!    the switch.
//! 6. Remove the generation-`<g>` files, then `sync_dir`.
//!
//! A crash before step 5 leaves the old database, whose next migration
//! first removes the files of other generations, as open does (TS-05); a
//! crash after it leaves the new one, whose next open removes `<g>`.
//! PostgreSQL's `pg_upgrade` refuses a source that was not shut down
//! cleanly (R-PG `src/bin/pg_upgrade/controldata.c:160-164`) and keeps page
//! LSNs by copying relation files (`src/bin/pg_upgrade/relfilenumber.c:531,
//! 590-611`). Neo4j migrates only a stopped database and has no downgrade
//! (TS-28, sources S8 to S10).
//!
//! **Format steps.** Each supported migration is one [`Step`]: the format it
//! reads, the page check of that format, and the page conversion. This crate
//! writes the first format version, 1, so it has no step; tests define the
//! test-only format 0 and its step ([`Formats`]).

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use super::COPY_BUFFER;
use crate::obs::MetricsSink;
use crate::storage::control::{Control, ControlSlots};
use crate::storage::datafile::Header;
use crate::storage::engine::{CONTROL, FORMAT_VERSION, LOCK, data_name, file_set_generation};
use crate::storage::error::{Error, Health, Result};
use crate::storage::page::{PAGE_BYTES, PAGE_SIZE};
use crate::storage::wal::{TEMP_SEGMENT, parse_segment_name};
use crate::storage::{Storage, StorageFile};

/// A sink for the one metric `CONTROL` reads count: migration runs on a
/// closed location, which has no metrics sink.
struct NullSink;

impl MetricsSink for NullSink {
    fn counter(&self, _: &'static str, _: u64) {}
    fn gauge(&self, _: &'static str, _: u64) {}
    fn histogram(&self, _: &'static str, _: u64) {}
}

/// One migration from format `from` to `from + 1`.
#[derive(Clone, Copy)]
pub(crate) struct Step {
    /// The format read.
    pub(crate) from: u32,
    /// Whether a page image of format `from` passes that format's check at
    /// page number `page_no`.
    pub(crate) verify: fn(page: &[u8], page_no: u64) -> bool,
    /// Converts a verified page image to format `from + 1` in place,
    /// keeping its page LSN; the format version field of page 0 is set by
    /// the caller.
    pub(crate) convert: fn(page: &mut [u8], page_no: u64),
}

/// The formats a build of the crate knows.
#[derive(Clone, Copy)]
pub(crate) struct Formats {
    /// The format this build writes (N).
    pub(crate) current: u32,
    /// The steps it can run.
    pub(crate) steps: &'static [Step],
    /// For each format version, the crate version that writes it.
    pub(crate) crate_versions: &'static [(u32, &'static str)],
}

impl Formats {
    fn crate_version(&self, format: u32) -> Option<&'static str> {
        self.crate_versions
            .iter()
            .find(|&&(f, _)| f == format)
            .map(|&(_, v)| v)
    }
}

/// The formats of this crate: version 1, written by this crate version.
pub(crate) const FORMATS: Formats = Formats {
    current: FORMAT_VERSION,
    steps: &[],
    crate_versions: &[(FORMAT_VERSION, env!("CARGO_PKG_VERSION"))],
};

/// What a migration did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Migrated {
    /// Format version before.
    pub(crate) from: u32,
    /// Generation written.
    pub(crate) generation: u64,
    /// Bytes of the old data file and WAL: what step 3 required.
    pub(crate) required: u64,
    /// Bytes `available_space` reported.
    pub(crate) available: u64,
    /// Pages converted.
    pub(crate) pages: u64,
    /// Wall time.
    pub(crate) duration: Duration,
}

/// Migrates the closed database at `location` to this crate's format
/// (module documentation).
///
/// # Errors
///
/// As [`migrate_with`].
pub(crate) fn migrate(location: &Path, storage: &dyn Storage) -> Result<Option<Migrated>> {
    migrate_with(location, storage, &FORMATS)
}

/// Migrates the closed database at `location` from format `formats.current
/// − 1` to `formats.current`. Returns `None`, changing nothing, if it is
/// already in the current format.
///
/// # Errors
///
/// [`Error::NoDatabase`] without `CONTROL`; [`Error::LockMissing`] without
/// `LOCK` (TS-27); [`Error::Locked`] if the database is open;
/// [`Error::NewerFormat`] for a newer format (FR-OPS-013);
/// [`Error::MigrationPath`] for a format two or more versions older
/// (FR-OPS-012); [`Error::NotCleanlyClosed`]; ENOSPC when
/// `available_space` is below what step 4 writes, with nothing changed but
/// the removal of files an interrupted migration left;
/// [`Error::Corruption`] for a page that fails the old format's check;
/// [`Error::Io`]. Every failure before step 5 leaves the old database.
pub(crate) fn migrate_with(
    location: &Path,
    storage: &dyn Storage,
    formats: &Formats,
) -> Result<Option<Migrated>> {
    let began = Instant::now();
    let s = storage;
    let control_file = match s.open(&location.join(CONTROL)) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Error::NoDatabase),
        Err(e) => return Err(e.into()),
    };
    // Step 1.
    let _lock = s
        .try_lock(&location.join(LOCK))
        .map_err(|e| match e.kind() {
            io::ErrorKind::WouldBlock => Error::Locked,
            io::ErrorKind::NotFound => Error::LockMissing,
            _ => Error::Io(e),
        })?;
    let health = std::sync::Arc::new(Health::default());
    let mut control = ControlSlots::open(control_file, health, &NullSink)?;
    let c = *control.current();
    let found = c.format_version;
    if found == formats.current {
        return Ok(None);
    }
    if found > formats.current {
        return Err(Error::NewerFormat {
            found,
            supported: formats.current,
        });
    }
    let Some(step) = formats.steps.iter().find(|st| st.from == found).copied() else {
        return Err(Error::MigrationPath {
            found,
            crate_version: formats.crate_version(found + 1),
        });
    };
    // Step 2.
    if !c.clean_shutdown {
        return Err(Error::NotCleanlyClosed {
            crate_version: formats.crate_version(found).unwrap_or("that wrote it"),
        });
    }
    // Step 3. Files left by an interrupted migration, or of any other
    // generation, are removed first, so that the space they hold counts as
    // available (TS-28 step 3).
    let g = c.generation;
    let names = s.list(location)?;
    let mut removed = false;
    for name in &names {
        if file_set_generation(name).is_some_and(|h| h != g) || name == TEMP_SEGMENT {
            s.remove(&location.join(name))?;
            removed = true;
        }
    }
    if removed {
        s.sync_dir(location)?;
    }
    let old_data = location.join(data_name(g));
    let old = s.open(&old_data).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::Corruption(format!("{} is missing", old_data.display())),
        _ => Error::Io(e),
    })?;
    let old_len = old.len()?;
    let mut required = old_len;
    for name in &names {
        if parse_segment_name(name).is_some_and(|(sg, _)| sg == g) {
            required += s.open(&location.join(name))?.len()?;
        }
    }
    let available = s.available_space(location)?;
    if available < required {
        return Err(Error::Io(io::ErrorKind::StorageFull.into()));
    }
    // Step 4.
    let next = g + 1;
    let new_data = location.join(data_name(next));
    let pages = write_converted(s, &*old, old_len, &new_data, &step, formats.current)?;
    s.sync_dir(location)?;
    // Step 5.
    control.write(Control {
        format_version: formats.current,
        generation: next,
        clean_shutdown: true,
        ..c
    })?;
    // Step 6.
    drop(old);
    for name in s.list(location)? {
        if file_set_generation(&name) == Some(g) {
            s.remove(&location.join(&name))?;
        }
    }
    s.sync_dir(location)?;
    Ok(Some(Migrated {
        from: found,
        generation: next,
        required,
        available,
        pages,
        duration: began.elapsed(),
    }))
}

/// Step 4: writes `path` from `old`, converting every page below the page
/// count with `step`, and syncs it. Returns the pages converted.
fn write_converted(
    s: &dyn Storage,
    old: &dyn StorageFile,
    old_len: u64,
    path: &Path,
    step: &Step,
    current: u32,
) -> Result<u64> {
    let mut page0 = vec![0u8; PAGE_SIZE];
    if old.read_at(&mut page0, 0)? < PAGE_SIZE || !(step.verify)(&page0, 0) {
        return Err(Error::Corruption("page 0 fails its checksum".into()));
    }
    let header = Header::decode(&page0)?;
    if header.format_version != step.from || header.page_count * PAGE_BYTES > old_len {
        return Err(Error::Corruption(format!(
            "data file header (format {}, {} pages) does not match CONTROL",
            header.format_version, header.page_count
        )));
    }
    let new = s.create(path)?;
    new.reserve(0, old_len)?;
    let mut buf = vec![0u8; COPY_BUFFER];
    let per = (COPY_BUFFER / PAGE_SIZE) as u64;
    let mut p = 0;
    while p < header.page_count {
        let n = per.min(header.page_count - p);
        let bytes = (n * PAGE_BYTES) as usize;
        if old.read_at(&mut buf[..bytes], p * PAGE_BYTES)? < bytes {
            return Err(Error::Corruption(
                "data file shorter than its page count".into(),
            ));
        }
        for (i, page) in buf[..bytes]
            .as_chunks_mut::<PAGE_SIZE>()
            .0
            .iter_mut()
            .enumerate()
        {
            let page_no = p + i as u64;
            if !(step.verify)(page, page_no) {
                return Err(Error::Corruption(format!(
                    "page {page_no} fails the checksum of format {}",
                    step.from
                )));
            }
            (step.convert)(page, page_no);
            if page_no == 0 {
                // `encode` keeps the page LSN and seals the page.
                let mut h = Header::decode(page)?;
                h.format_version = current;
                h.encode(page);
            } else {
                crate::storage::page::seal(page, page_no);
            }
        }
        new.write_at(&buf[..bytes], p * PAGE_BYTES)?;
        p += n;
    }
    new.sync_data()?;
    Ok(header.page_count)
}

#[cfg(test)]
pub(crate) mod tests;
