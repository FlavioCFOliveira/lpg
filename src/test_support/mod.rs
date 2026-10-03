//! Helpers shared by the crate's tests.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::storage::{FsStorage, Storage, StorageFile, StorageLock};

pub(crate) mod child;

/// The names of this process's threads, from `/proc/self/task/*/comm`.
pub(crate) fn thread_names() -> Vec<String> {
    use rustix::fs::{Mode, OFlags};
    let fd = rustix::fs::open(
        "/proc/self/task",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .expect("open /proc/self/task");
    let dir = rustix::fs::Dir::read_from(&fd).expect("read /proc/self/task");
    let mut names = Vec::new();
    for entry in dir {
        let entry = entry.expect("directory entry");
        let tid = entry.file_name().to_string_lossy().into_owned();
        if tid.starts_with('.') {
            continue;
        }
        let path = Path::new("/proc/self/task").join(&tid).join("comm");
        // A thread may end between the listing and the read.
        if let Ok(fd) = rustix::fs::open(&path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty()) {
            let mut buf = [0u8; 64];
            if let Ok(n) = rustix::io::read(&fd, &mut buf[..]) {
                names.push(String::from_utf8_lossy(&buf[..n]).trim_end().to_owned());
            }
        }
    }
    names
}

/// Directories a test runs in: one under `LPG_LAB_EXT4` and one under
/// `LPG_LAB_XFS` when set, else one under the system temporary directory.
pub(crate) fn test_dirs() -> Vec<tempfile::TempDir> {
    let dirs: Vec<_> = ["LPG_LAB_EXT4", "LPG_LAB_XFS"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|d| tempfile::tempdir_in(d).expect("create test directory"))
        .collect();
    if dirs.is_empty() {
        vec![tempfile::tempdir().expect("create test directory")]
    } else {
        dirs
    }
}

/// Every file of `dir` with its full content, read through [`FsStorage`],
/// sorted by name. Comparing two snapshots compares every byte, which is
/// stronger than comparing digests.
pub(crate) fn snapshot(dir: &Path) -> Vec<(OsString, Vec<u8>)> {
    FsStorage
        .list(dir)
        .expect("list directory")
        .into_iter()
        .map(|name| {
            let file = FsStorage.open(&dir.join(&name)).expect("open file");
            let mut bytes = vec![0; file.len().expect("file length") as usize];
            let n = file.read_at(&mut bytes, 0).expect("read file");
            assert_eq!(n, bytes.len());
            (name, bytes)
        })
        .collect()
}

/// [`FsStorage`] without the device flush: `sync_data` and `sync_dir` do
/// nothing; every other call, writes included, goes to the file system
/// unchanged. For tests that never crash the operating system: what they
/// read back comes from the OS page cache whether or not it was flushed, so
/// the flush proves nothing there and only adds device latency (measured
/// 2026-10-03: 40 % iowait during the full suite). Tests of real-device
/// behaviour use [`FsStorage`]; crash tests use
/// [`crate::storage::fault::FaultStorage`], which models durability itself.
pub(crate) struct NoFlush;

/// The storage of tests that need no device flush: [`NoFlush`].
pub(crate) fn no_flush() -> Arc<dyn Storage> {
    Arc::new(NoFlush)
}

struct NoFlushFile(Box<dyn StorageFile>);

impl Storage for NoFlush {
    fn open(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        Ok(Box::new(NoFlushFile(FsStorage.open(path)?)))
    }

    fn create(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        Ok(Box::new(NoFlushFile(FsStorage.create(path)?)))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        FsStorage.remove(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        FsStorage.rename(from, to)
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<OsString>> {
        FsStorage.list(dir)
    }

    fn sync_dir(&self, _dir: &Path) -> io::Result<()> {
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> io::Result<Box<dyn StorageLock>> {
        FsStorage.try_lock(path)
    }

    fn available_space(&self, dir: &Path) -> io::Result<u64> {
        FsStorage.available_space(dir)
    }
}

impl StorageFile for NoFlushFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        self.0.read_at(buf, offset)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        self.0.write_at(buf, offset)
    }

    fn sync_data(&self) -> io::Result<()> {
        Ok(())
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }

    fn len(&self) -> io::Result<u64> {
        self.0.len()
    }

    fn reserve(&self, offset: u64, len: u64) -> io::Result<()> {
        self.0.reserve(offset, len)
    }
}
