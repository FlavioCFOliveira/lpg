//! Storage abstraction (TS-03).
//!
//! Every file the crate touches goes through [`Storage`] and [`StorageFile`]
//! (FR-STO-002). The application may supply its own implementation; the crate
//! provides [`FsStorage`], which uses the local filesystem.
//!
//! Both traits are synchronous and dyn-compatible: the engine holds them as
//! `Box<dyn Storage>` and `Box<dyn StorageFile>`. Errors are [`std::io::Error`];
//! an implementation reports a full device with
//! [`std::io::ErrorKind::StorageFull`] (`ENOSPC`) and a held lock with
//! [`std::io::ErrorKind::WouldBlock`].

use std::ffi::OsString;
use std::io;
use std::path::Path;

mod fs;

// The kernel modules below have no caller outside tests until the B+tree,
// WAL and API tasks build on them. `allow` rather than `expect`: Rust 1.89
// and 1.99 report dead code in these modules differently.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #15-#16")
)]
pub(crate) mod btree;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #15-#16")
)]
pub(crate) mod check;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod control;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod datafile;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod engine;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod error;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #15-#16")
)]
pub(crate) mod freelist;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod memory;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod page;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with #14-#16")
)]
pub(crate) mod pool;

#[cfg(test)]
pub(crate) mod fault;

pub use fs::FsStorage;

/// Store-level operations: naming, directories, locking and space.
///
/// Paths are passed as given by the caller; an implementation may interpret
/// them relative to its own root.
pub trait Storage: Send + Sync {
    /// Opens an existing file for reading and writing.
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::NotFound`] if the file does not exist, or
    /// with any other I/O error of the implementation.
    fn open(&self, path: &Path) -> io::Result<Box<dyn StorageFile>>;

    /// Creates a new, empty file for reading and writing.
    ///
    /// The new name is durable only after [`Storage::sync_dir`] on its
    /// directory.
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::AlreadyExists`] if the file exists, with
    /// [`io::ErrorKind::StorageFull`] if no space is left, or with any other
    /// I/O error.
    fn create(&self, path: &Path) -> io::Result<Box<dyn StorageFile>>;

    /// Removes a file. The removal is durable only after
    /// [`Storage::sync_dir`] on its directory.
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::NotFound`] if the file does not exist, or
    /// with any other I/O error.
    fn remove(&self, path: &Path) -> io::Result<()>;

    /// Renames `from` to `to`, replacing `to` if it exists. The rename is
    /// durable only after [`Storage::sync_dir`] on the directory of `to`.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Returns the names of the entries of a directory, sorted.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn list(&self, dir: &Path) -> io::Result<Vec<OsString>>;

    /// Makes the creations, removals and renames in a directory durable.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;

    /// Takes an exclusive lock on an existing file (TS-27). The lock is held
    /// until the returned guard is dropped.
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::WouldBlock`] if the lock is held, by this
    /// or another process, or with any other I/O error.
    fn try_lock(&self, path: &Path) -> io::Result<Box<dyn StorageLock>>;

    /// Returns the bytes available to an unprivileged writer on the
    /// filesystem that holds `dir`.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn available_space(&self, dir: &Path) -> io::Result<u64>;
}

/// File-level operations. Offsets and lengths are in bytes.
#[expect(
    clippy::len_without_is_empty,
    reason = "TS-03 defines `len`; no caller needs `is_empty` on a file"
)]
pub trait StorageFile: Send + Sync {
    /// Reads into `buf` from `offset`. Returns the number of bytes read,
    /// which is less than `buf.len()` only when the end of the file is
    /// reached.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize>;

    /// Writes all of `buf` at `offset`. The data is durable only after
    /// [`StorageFile::sync_data`].
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::StorageFull`] if no space is left, or with
    /// any other I/O error. After an error, any part of `buf` may have been
    /// written.
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()>;

    /// Makes the data written so far, and the file length, durable
    /// (`fdatasync`).
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation. After an error, the
    /// durability of earlier writes is unknown.
    fn sync_data(&self) -> io::Result<()>;

    /// Sets the file length, truncating or extending it with zeros.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn set_len(&self, len: u64) -> io::Result<()>;

    /// Returns the file length.
    ///
    /// # Errors
    ///
    /// Fails with any I/O error of the implementation.
    fn len(&self) -> io::Result<u64>;

    /// Allocates the byte range `offset..offset + len`, extending the file if
    /// needed, so that later writes in that range cannot fail for lack of
    /// space (`fallocate`, TS-05).
    ///
    /// # Errors
    ///
    /// Fails with [`io::ErrorKind::StorageFull`] if no space is left, or with
    /// any other I/O error.
    fn reserve(&self, offset: u64, len: u64) -> io::Result<()>;
}

/// A lock taken by [`Storage::try_lock`]; dropping it releases the lock.
pub trait StorageLock: Send + Sync {}
