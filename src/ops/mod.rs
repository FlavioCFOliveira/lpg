//! Operations: backup, restore, import, export and format migration.
//!
//! Sprint 1 provides the crate-internal entry points of online physical
//! backup, restore and format migration (modules `backup`, `restore` and
//! `migrate`); the public calls come with the API (Sprint 3).

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with the API tasks")
)]
pub(crate) mod backup;
pub mod bulk;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with the API tasks")
)]
pub(crate) mod migrate;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with the API tasks")
)]
pub(crate) mod restore;

use std::io;
use std::path::Path;

use crate::storage::error::{Error, Result};
use crate::storage::{Storage, StorageFile};

/// Bytes of the one copy buffer of backup, restore and migration (TS-08:
/// outside the cap; LMDB's `MDB_WBUF`, R-LMDB `libraries/liblmdb/mdb.c:11140`).
pub(crate) const COPY_BUFFER: usize = 1 << 20;

/// Creates `path`, or opens it and empties it if a file of that name is
/// left by an interrupted run of the same operation (TS-26: "a new restore
/// overwrites the files it left").
pub(crate) fn create_or_truncate(
    storage: &dyn Storage,
    path: &Path,
) -> Result<Box<dyn StorageFile>> {
    match storage.create(path) {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let f = storage.open(path)?;
            f.set_len(0)?;
            Ok(f)
        }
        Err(e) => Err(e.into()),
    }
}

/// Copies `len` bytes from `from` at `src` to `to` at `dst` through `buf`,
/// and returns the CRC-32C of the bytes, continuing `crc`. A failed or
/// short read of `from` is passed through `read_failed`, so that a backup
/// can treat it as a failed read of the database (TS-12); a failed write
/// to `to` is returned as it is.
#[expect(
    clippy::too_many_arguments,
    reason = "a positional copy between two files with its error policy"
)]
pub(crate) fn copy_range(
    from: &dyn StorageFile,
    src: u64,
    to: &dyn StorageFile,
    dst: u64,
    len: u64,
    buf: &mut [u8],
    mut crc: u32,
    read_failed: &dyn Fn(Error) -> Error,
) -> Result<u32> {
    let mut done = 0;
    while done < len {
        let n = (len - done).min(buf.len() as u64) as usize;
        let got = from
            .read_at(&mut buf[..n], src + done)
            .map_err(|e| read_failed(e.into()))?;
        if got < n {
            return Err(read_failed(
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "source file shorter than expected",
                )
                .into(),
            ));
        }
        to.write_at(&buf[..n], dst + done)?;
        crc = crc32c::crc32c_append(crc, &buf[..n]);
        done += n as u64;
    }
    Ok(crc)
}
