//! Restore of a backup artefact (TS-26 restore steps 1-6; task #18).
//!
//! 1. Fail if the target holds `CONTROL`: it is a database (FR-OPS-009).
//! 2. Read `MANIFEST`; absent or failing its CRC-32C, the backup is
//!    incomplete. Its format version is checked as open checks it (TS-28).
//! 3. Check the length and CRC-32C of every artefact file against
//!    `MANIFEST` before anything is written (FR-REC-003).
//! 4. Create and lock `LOCK`; fail again if the target holds `CONTROL`,
//!    which a create may have written since step 1; write `data-<g>`, reserved to its length at
//!    `stop`, and one 16 MiB zero-filled segment per artefact segment with
//!    the copied bytes at their offsets, and [`BACKUP_END`] holding `stop`;
//!    `sync_data` each, then `sync_dir`.
//! 5. Write `CONTROL` last: one valid slot, the artefact's authoritative
//!    slot with its redo point set to `start` and the clean-shutdown flag
//!    cleared, and the other slot zeroed. It is written whole as
//!    `CONTROL.tmp`, synced, renamed to `CONTROL` and the target synced
//!    (PostgreSQL's `durable_rename`, R-PG `src/common/file_utils.c:483`),
//!    so a failure or a crash in this step leaves no `CONTROL` and the
//!    target restorable again.
//! 6. Open the database: recovery replays the WAL from `start` to its end,
//!    which must reach `stop`, or the open fails as corruption before its
//!    checkpoint writes `CONTROL` (PostgreSQL: "WAL ends before end of
//!    online backup", R-PG `src/backend/access/transam/xlog.c:6598`). Step
//!    4 also writes `stop` to [`BACKUP_END`], so every later open of the
//!    target refuses it too, until an open recovers past `stop`, which then
//!    removes the file (PostgreSQL's `backup_label`).
//!
//! The artefact is read defensively: `MANIFEST` and the artefact's `CONTROL`
//! are length-checked before they are read into memory, and every segment
//! range must lie inside one segment.
//!
//! A location is a database exactly when `CONTROL` exists (TS-05), so a
//! restore interrupted before step 5 leaves no database. A later restore
//! overwrites what it left (TS-26): files of the same name are emptied and
//! rewritten, and database files the artefact does not hold (a `data-<h>`,
//! a `wal-<h>-<lsn>` or `wal-temp` left by an interrupted restore of
//! another artefact) are removed first, so that no stale segment can join
//! the restored WAL. PostgreSQL likewise starts recovery of a restored base
//! backup from the start location in `backup_label`, not from the copied
//! `pg_control` (R-PG `src/backend/access/transam/xlogrecovery.c:543-557`).

use std::path::Path;
use std::sync::Arc;

use super::backup::{MANIFEST, Manifest};
use super::{COPY_BUFFER, copy_range, create_or_truncate};
use crate::obs::MetricsSink;
use crate::storage::control::{self, ControlSlots, SLOT_SIZE};
use crate::storage::engine::{
    BACKUP_END, CONTROL, Engine, FORMAT_VERSION, LOCK, data_name, encode_backup_end,
    file_set_generation,
};
use crate::storage::error::{Error, Result};
use crate::storage::page::PAGE_SIZE;
use crate::storage::wal::{SEGMENT_SIZE, TEMP_SEGMENT, parse_segment_name, segment_name};
use crate::storage::{Storage, StorageFile};

/// Longest `MANIFEST` restore reads: one segment's size, room for some
/// 400 000 file entries. A longer one is not a `MANIFEST` this crate wrote.
pub(crate) const MANIFEST_MAX: u64 = SEGMENT_SIZE;

/// Reads a whole artefact file of at most `max` bytes; `None` if it is
/// longer, before anything is allocated for it.
fn read_all(storage: &dyn Storage, path: &Path, max: u64) -> Result<Option<Vec<u8>>> {
    let f = storage.open(path)?;
    let len = f.len()?;
    if len > max {
        return Ok(None);
    }
    let mut b = vec![0u8; len as usize];
    let n = f.read_at(&mut b, 0)?;
    b.truncate(n);
    Ok(Some(b))
}

/// The length and CRC-32C of a file, read through `buf`.
fn len_and_crc(f: &dyn StorageFile, buf: &mut [u8]) -> Result<(u64, u32)> {
    let len = f.len()?;
    let mut crc = 0;
    let mut at = 0;
    while at < len {
        let n = (len - at).min(buf.len() as u64) as usize;
        let got = f.read_at(&mut buf[..n], at)?;
        if got == 0 {
            break;
        }
        crc = crc32c::crc32c_append(crc, &buf[..got]);
        at += got as u64;
    }
    Ok((at, crc))
}

/// Restores the artefact in `artefact` into `target`, an existing
/// directory that holds no database, both on `storage`, and opens the
/// result with `memory_cap` and `sink` (TS-26; module documentation).
///
/// # Errors
///
/// [`Error::DatabaseExists`] if `target` holds `CONTROL`, with nothing
/// written; [`Error::IncompleteBackup`] if `MANIFEST` is absent or damaged;
/// [`Error::MigrationRequired`] or [`Error::NewerFormat`] for another format
/// version; [`Error::Corruption`] if an artefact file differs from
/// `MANIFEST`; [`Error::Locked`] if another handle holds the target's
/// `LOCK`; [`Error::Io`], with no `CONTROL` written before step 5 ends;
/// [`Error::Corruption`] if recovery ends before `stop`; errors of
/// [`Engine::open`].
pub(crate) fn restore(
    artefact: &Path,
    target: &Path,
    storage: Arc<dyn Storage>,
    memory_cap: u64,
    sink: Arc<dyn MetricsSink>,
) -> Result<Engine> {
    let s = &*storage;
    // Step 1.
    let existing = s.list(target)?;
    if existing.iter().any(|n| n == CONTROL) {
        return Err(Error::DatabaseExists);
    }
    // Step 2.
    let names = s.list(artefact)?;
    if !names.iter().any(|n| n == MANIFEST) {
        return Err(Error::IncompleteBackup("MANIFEST is missing".into()));
    }
    let bytes = read_all(s, &artefact.join(MANIFEST), MANIFEST_MAX)?.ok_or_else(|| {
        Error::IncompleteBackup("MANIFEST is longer than any backup writes".into())
    })?;
    let m = Manifest::decode(&bytes)
        .ok_or_else(|| Error::IncompleteBackup("MANIFEST fails its checksum".into()))?;
    if m.format_version < FORMAT_VERSION {
        return Err(Error::MigrationRequired {
            found: m.format_version,
            required: FORMAT_VERSION,
        });
    }
    if m.format_version > FORMAT_VERSION {
        return Err(Error::NewerFormat {
            found: m.format_version,
            supported: FORMAT_VERSION,
        });
    }
    if m.page_size as usize != PAGE_SIZE {
        return Err(Error::Corruption(format!(
            "backup page size {} differs from {PAGE_SIZE}",
            m.page_size
        )));
    }
    // Step 3.
    let data = data_name(m.generation);
    let mut segments = Vec::new();
    let mut buf = vec![0u8; COPY_BUFFER];
    for f in &m.files {
        let path = artefact.join(&f.name);
        let file = s
            .open(&path)
            .map_err(|e| Error::Corruption(format!("backup file {} is missing: {e}", f.name)))?;
        if len_and_crc(&*file, &mut buf)? != (f.len, f.crc) {
            return Err(Error::Corruption(format!(
                "backup file {} differs from MANIFEST",
                f.name
            )));
        }
        if let Some((g, lsn)) = parse_segment_name(f.name.as_ref()) {
            // Step 4 writes the bytes at this offset of a segment.
            let offset = m.start.max(lsn) - lsn;
            if g != m.generation || offset.checked_add(f.len).is_none_or(|e| e > SEGMENT_SIZE) {
                return Err(Error::Corruption(format!(
                    "backup file {} does not belong to the backup",
                    f.name
                )));
            }
            segments.push((lsn, f.len));
        }
    }
    let control_raw = read_all(s, &artefact.join(CONTROL), 2 * SLOT_SIZE as u64)?
        .ok_or_else(|| Error::Corruption("the backup CONTROL is too long".into()))?;
    let slots = [0, 1].map(|i| {
        control_raw
            .get(i * SLOT_SIZE..(i + 1) * SLOT_SIZE)
            .and_then(control::decode)
    });
    let (control, sequence) = match slots {
        [Some(a), Some(b)] => {
            if b.1 > a.1 {
                b
            } else {
                a
            }
        }
        [Some(a), None] | [None, Some(a)] => a,
        [None, None] => {
            return Err(Error::Corruption(
                "both CONTROL slots of the backup fail their checksum".into(),
            ));
        }
    };
    if control.uuid != m.uuid
        || control.generation != m.generation
        || !m.files.iter().any(|f| f.name == data)
        || segments.is_empty()
    {
        return Err(Error::Corruption(
            "backup CONTROL, data file or WAL differ from MANIFEST".into(),
        ));
    }
    // Step 4.
    let lock_path = target.join(LOCK);
    drop(create_or_truncate(s, &lock_path)?);
    let lock = s.try_lock(&lock_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::WouldBlock => Error::Locked,
        _ => Error::Io(e),
    })?;
    // A database created between step 1 and the lock: step 5 would replace
    // its `CONTROL` (FR-OPS-009), so check again under the lock, as
    // `Engine::create` does.
    let existing = s.list(target)?;
    if existing.iter().any(|n| n == CONTROL) {
        return Err(Error::DatabaseExists);
    }
    let keep: Vec<String> = m.files.iter().map(|f| f.name.clone()).collect();
    for name in existing {
        let stale = name == TEMP_SEGMENT || file_set_generation(&name).is_some();
        if stale && !keep.iter().any(|k| name == k.as_str()) {
            s.remove(&target.join(&name))?;
        }
    }
    let mut written = Vec::new();
    let src = s.open(&artefact.join(&data))?;
    let f = create_or_truncate(s, &target.join(&data))?;
    f.reserve(0, m.data_len)?;
    copy_range(&*src, 0, &*f, 0, src.len()?, &mut buf, 0, &|e| e)?;
    written.push(f);
    for &(lsn, len) in &segments {
        let name = segment_name(m.generation, lsn);
        let f = create_or_truncate(s, &target.join(&name))?;
        buf.fill(0);
        let mut at = 0;
        while at < SEGMENT_SIZE {
            let n = (SEGMENT_SIZE - at).min(buf.len() as u64) as usize;
            f.write_at(&buf[..n], at)?;
            at += n as u64;
        }
        let offset = m.start.max(lsn) - lsn;
        let src = s.open(&artefact.join(&name))?;
        copy_range(&*src, 0, &*f, offset, len, &mut buf, 0, &|e| e)?;
        written.push(f);
    }
    // The backup's end, which every open enforces until one has recovered
    // past it (step 6).
    let end = create_or_truncate(s, &target.join(BACKUP_END))?;
    end.write_at(&encode_backup_end(m.stop), 0)?;
    written.push(end);
    for f in &written {
        f.sync_data()?;
    }
    drop(written);
    s.sync_dir(target)?;
    // Step 5.
    let restored = control::Control {
        redo_point: m.start,
        clean_shutdown: false,
        ..control
    };
    ControlSlots::create_single(s, target, &restored, sequence)?;
    // Step 6. The lock is released for open, which takes it again.
    drop(lock);
    Engine::open(target, storage, memory_cap, sink)
}

#[cfg(test)]
mod tests;
