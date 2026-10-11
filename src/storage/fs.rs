//! Default storage on the local filesystem (TS-03).
//!
//! Positional I/O uses [`std::os::unix::fs::FileExt`]; `available_space` uses
//! `fstatvfs` and `reserve` uses `fallocate` through `rustix`, since `std`
//! offers neither. `try_lock` uses [`File::try_lock`] (`flock`, TS-27).

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;

use rustix::fs::{FallocateFlags, fallocate, fstatvfs};

use super::{Storage, StorageFile, StorageLock};

/// [`Storage`] on the local filesystem. Paths are used as given.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsStorage;

/// A file opened by [`FsStorage`].
#[derive(Debug)]
struct FsFile(File);

/// A lock taken by [`FsStorage::try_lock`]; closing the file releases it.
#[derive(Debug)]
struct FsLock {
    _file: File,
}

impl StorageLock for FsLock {}

impl Storage for FsStorage {
    fn open(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Box::new(FsFile(file)))
    }

    fn create(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        Ok(Box::new(FsFile(file)))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    /// `renameat2(RENAME_NOREPLACE)` (Linux 3.15; rustix
    /// `renameat_with`); on a file system that does not support the flag
    /// (`EINVAL`), `link(2)`, which fails with `EEXIST` if `to` exists, then
    /// `unlink(2)` of `from`.
    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()> {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
            Ok(()) => Ok(()),
            Err(rustix::io::Errno::INVAL) => {
                fs::hard_link(from, to)?;
                fs::remove_file(from)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<OsString>> {
        let mut names = fs::read_dir(dir)?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        File::open(dir)?.sync_all()
    }

    fn try_lock(&self, path: &Path) -> io::Result<Box<dyn StorageLock>> {
        let file = File::open(path)?;
        file.try_lock()?;
        Ok(Box::new(FsLock { _file: file }))
    }

    fn available_space(&self, dir: &Path) -> io::Result<u64> {
        let stat = fstatvfs(File::open(dir)?)?;
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }
}

impl StorageFile for FsFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let mut done = 0;
        while done < buf.len() {
            match self.0.read_at(&mut buf[done..], offset + done as u64) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        self.0.write_all_at(buf, offset)
    }

    fn sync_data(&self) -> io::Result<()> {
        self.0.sync_data()
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }

    fn len(&self) -> io::Result<u64> {
        Ok(self.0.metadata()?.len())
    }

    fn reserve(&self, offset: u64, len: u64) -> io::Result<()> {
        // Mode 0 allocates the range and extends the file size if needed,
        // like `posix_fallocate`.
        Ok(fallocate(&self.0, FallocateFlags::empty(), offset, len)?)
    }
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::path::PathBuf;

    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};

    use super::*;

    const PAGE: usize = 8192;

    /// Directory of the laboratory runs below: `LPG_TEST_DIR` if set, else
    /// the system temporary directory. The other tests use
    /// [`crate::test_support::graph_dir`].
    fn test_dir() -> tempfile::TempDir {
        match std::env::var_os("LPG_TEST_DIR") {
            Some(dir) => tempfile::tempdir_in(dir),
            None => tempfile::tempdir(),
        }
        .expect("create test directory")
    }

    #[test]
    fn file_round_trip() {
        let dir = crate::test_support::graph_dir();
        let path = dir.path().join("f");
        let storage: Box<dyn Storage> = Box::new(FsStorage);
        let file = storage.create(&path).unwrap();
        file.write_at(b"hello", 3).unwrap();
        file.sync_data().unwrap();
        assert_eq!(file.len().unwrap(), 8);
        let mut buf = [0xffu8; 16];
        assert_eq!(file.read_at(&mut buf, 0).unwrap(), 8);
        assert_eq!(&buf[..8], b"\0\0\0hello");
        assert_eq!(file.read_at(&mut buf, 100).unwrap(), 0);
        file.set_len(4).unwrap();
        assert_eq!(file.len().unwrap(), 4);
        file.reserve(0, 1 << 20).unwrap();
        assert_eq!(file.len().unwrap(), 1 << 20);
        drop(file);
        let file = storage.open(&path).unwrap();
        assert_eq!(file.read_at(&mut buf[..4], 0).unwrap(), 4);
        assert_eq!(&buf[..4], b"\0\0\0h");
    }

    #[test]
    fn create_and_open_errors() {
        let dir = crate::test_support::graph_dir();
        let path = dir.path().join("f");
        assert_eq!(
            FsStorage.open(&path).err().unwrap().kind(),
            io::ErrorKind::NotFound
        );
        FsStorage.create(&path).unwrap();
        assert_eq!(
            FsStorage.create(&path).err().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn namespace_operations() {
        let dir = crate::test_support::graph_dir();
        let d = dir.path();
        FsStorage.create(&d.join("b")).unwrap();
        FsStorage.create(&d.join("a")).unwrap();
        FsStorage.sync_dir(d).unwrap();
        assert_eq!(FsStorage.list(d).unwrap(), ["a", "b"]);
        FsStorage.rename(&d.join("a"), &d.join("c")).unwrap();
        FsStorage.remove(&d.join("b")).unwrap();
        FsStorage.sync_dir(d).unwrap();
        assert_eq!(FsStorage.list(d).unwrap(), ["c"]);
        assert!(FsStorage.available_space(d).unwrap() > 0);
    }

    #[test]
    fn try_lock_is_exclusive_within_the_process() {
        let dir = crate::test_support::graph_dir();
        let path = dir.path().join("LOCK");
        FsStorage.create(&path).unwrap();
        let lock = FsStorage.try_lock(&path).unwrap();
        assert_eq!(
            FsStorage.try_lock(&path).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lock);
        FsStorage.try_lock(&path).unwrap();
    }

    /// Fixed run for `strace -c -f`: exactly 10 `reserve`, 10 000 8 KiB
    /// `write_at`, 100 `sync_data` and 10 000 8 KiB `read_at`.
    #[test]
    #[ignore = "laboratory run; see docs/evidence/lab-sprint-1/report.md"]
    fn lab_fixed_syscall_run() {
        const PAGES: u64 = 10_000;
        let dir = test_dir();
        let storage: Box<dyn Storage> = Box::new(FsStorage);
        let file = storage.create(&dir.path().join("data")).unwrap();
        let bytes = PAGES * PAGE as u64;
        for i in 0..10 {
            file.reserve(i * bytes / 10, bytes / 10).unwrap();
        }
        let mut buf = vec![0u8; PAGE];
        for p in 0..PAGES {
            buf[..8].copy_from_slice(&p.to_le_bytes());
            file.write_at(&buf, p * PAGE as u64).unwrap();
            if p % 100 == 99 {
                file.sync_data().unwrap();
            }
        }
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..PAGES {
            let p = rng.random_range(0..PAGES);
            assert_eq!(file.read_at(&mut buf, p * PAGE as u64).unwrap(), PAGE);
            assert_eq!(buf[..8], p.to_le_bytes());
        }
    }

    /// Cached 8 KiB reads for `perf stat`. `LPG_PERF_MODE` is `dyn` (through
    /// `Box<dyn StorageFile>`), `direct` (`FileExt::read_at` on a `File`) or
    /// `setup` (no reads, to subtract); `LPG_PERF_N` is the number of reads.
    #[test]
    #[ignore = "laboratory run; see docs/evidence/lab-sprint-1/report.md"]
    fn lab_cached_read_at() {
        const PAGES: u64 = 8192; // 64 MiB, resident in the page cache.
        let mode = std::env::var("LPG_PERF_MODE").unwrap_or_else(|_| "setup".into());
        let n: usize = std::env::var("LPG_PERF_N").map_or(1_000_000, |v| v.parse().unwrap());
        let dir = test_dir();
        let path: PathBuf = dir.path().join("data");
        let file = FsStorage.create(&path).unwrap();
        let buf = vec![7u8; PAGE];
        for p in 0..PAGES {
            file.write_at(&buf, p * PAGE as u64).unwrap();
        }
        let mut rng = StdRng::seed_from_u64(1);
        let offsets: Vec<u64> = (0..n)
            .map(|_| rng.random_range(0..PAGES) * PAGE as u64)
            .collect();
        let mut buf = vec![0u8; PAGE];
        let mut sum = 0usize;
        match mode.as_str() {
            "dyn" => {
                let file: Box<dyn StorageFile> = black_box(file);
                for &off in &offsets {
                    sum += file.read_at(black_box(&mut buf), off).unwrap();
                }
            }
            "direct" => {
                let raw = File::open(&path).unwrap();
                for &off in &offsets {
                    sum += raw.read_at(black_box(&mut buf), off).unwrap();
                }
            }
            "setup" => sum = n * PAGE,
            other => panic!("unknown LPG_PERF_MODE {other}"),
        }
        assert_eq!(black_box(sum), n * PAGE);
    }
}
