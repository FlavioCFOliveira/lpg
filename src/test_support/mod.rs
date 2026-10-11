//! Helpers shared by the crate's tests.

use crate::sync::Arc;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::storage::{FsStorage, Storage, StorageFile, StorageLock};

pub(crate) mod child;
pub(crate) mod history;
pub(crate) mod values;

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

/// The seed of a seeded fault test (TS-30 R5): `LPG_SEED` if set, else a
/// random one. It is printed, so a failing test shows the seed that
/// reproduces it (the test harness shows the output of failed tests).
/// Turso takes its test seed from `SEED` or the clock the same way (R-TURSO
/// `tests/integration/common.rs:535-547`).
pub(crate) fn seed() -> u64 {
    let seed = std::env::var("LPG_SEED").map_or_else(
        |_| rand::random(),
        |v| v.parse().expect("LPG_SEED is a u64"),
    );
    eprintln!("LPG_SEED={seed}");
    seed
}

/// The owner principal of the databases the tests create (FR-DB-005).
pub(crate) const OWNER: &str = "owner";

/// The bound of [`wait_until`]: far above the few milliseconds the waited
/// background work takes, so only a regression reaches it.
pub(crate) const WAIT_BOUND: std::time::Duration = std::time::Duration::from_secs(30);

/// Yields until `done` returns `true`; panics naming `what` once
/// [`WAIT_BOUND`] has passed, so a regression in background work (the
/// garbage collector) fails its test instead of hanging the run.
pub(crate) fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + WAIT_BOUND;
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "not reached within {WAIT_BOUND:?}: {what}"
        );
        std::thread::yield_now();
    }
}

/// Base directory of the tests' graph data files (CLAUDE.md, "Graph data
/// files in `/tmp/lpgram`"). `/tmp` is a RAM-backed `tmpfs` on the
/// development machine: it only accelerates the file I/O of tests and is
/// never a production scenario.
pub(crate) const GRAPH_DIR: &str = "/tmp/lpgram";

/// A new directory for one test's graph data files under [`GRAPH_DIR`],
/// removed with its content when the returned guard is dropped. Every test
/// that creates graph data files gets its directory here, unless it needs
/// persistent storage or a named file system ([`test_dirs`]).
///
/// # Panics
///
/// If the directory cannot be created.
pub(crate) fn graph_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(graph_base()).expect("create a test directory in /tmp/lpgram")
}

/// [`GRAPH_DIR`], created if missing, for a caller that makes its own
/// per-run subdirectory in it and removes it ([`tempfile::tempdir_in`]).
///
/// # Panics
///
/// If [`GRAPH_DIR`] cannot be created.
pub(crate) fn graph_base() -> &'static Path {
    std::fs::create_dir_all(GRAPH_DIR).expect("create /tmp/lpgram");
    Path::new(GRAPH_DIR)
}

/// Directories a test runs in: one under `LPG_LAB_EXT4` and one under
/// `LPG_LAB_XFS` when set, which name persistent ext4 and XFS directories
/// (NFR-PLAT-004); else one under [`GRAPH_DIR`] ([`graph_dir`]).
pub(crate) fn test_dirs() -> Vec<tempfile::TempDir> {
    let dirs: Vec<_> = ["LPG_LAB_EXT4", "LPG_LAB_XFS"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|d| tempfile::tempdir_in(d).expect("create test directory"))
        .collect();
    if dirs.is_empty() {
        vec![graph_dir()]
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

struct NoFlushFile(Box<dyn StorageFile>, PathBuf);

/// [`StorageFile::reserve`] as the test storages ([`NoFlush`] and
/// [`crate::storage::fault::FaultStorage`]) pass it to the file system.
/// Under [`GRAPH_DIR`] it only extends the file length, as `set_len` does,
/// which reads back the same zeros: a `tmpfs` `fallocate` takes real memory
/// (the 64 MiB first extent of every test database, TS-05), and a `tmpfs`
/// says nothing about the space guarantees of a real disk anyway
/// (CLAUDE.md). Those guarantees are tested on the model of `FaultStorage`
/// (its space budget) and on [`FsStorage`] itself, which always calls
/// `fallocate`. Elsewhere (`LPG_LAB_EXT4`, `LPG_LAB_XFS`) the call is
/// passed unchanged.
pub(crate) fn reserve(
    file: &dyn StorageFile,
    path: &Path,
    offset: u64,
    len: u64,
) -> io::Result<()> {
    if !path.starts_with(GRAPH_DIR) {
        return file.reserve(offset, len);
    }
    let end = offset
        .checked_add(len)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    if file.len()? < end {
        file.set_len(end)?;
    }
    Ok(())
}

impl Storage for NoFlush {
    fn open(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        Ok(Box::new(NoFlushFile(
            FsStorage.open(path)?,
            path.to_path_buf(),
        )))
    }

    fn create(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        Ok(Box::new(NoFlushFile(
            FsStorage.create(path)?,
            path.to_path_buf(),
        )))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        FsStorage.remove(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        FsStorage.rename(from, to)
    }

    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()> {
        FsStorage.rename_noreplace(from, to)
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
        reserve(&*self.0, &self.1, offset, len)
    }
}

/// A pause inside the next logged write batch of `e`
/// ([`crate::storage::wal::TestPoints`] `batch_body`): with its pages
/// changed and the commit latch held, the batch signals `reached` and
/// waits on `fail`; `true` fails it with ENOSPC, so it is undone (TS-16
/// step 6). A dropped `fail` lets it go on, so a failing test does not
/// hang.
pub(crate) struct MidBatch {
    pub(crate) reached: std::sync::mpsc::Receiver<()>,
    pub(crate) fail: std::sync::mpsc::Sender<bool>,
}

/// Arms [`MidBatch`] on the next logged write batch of `e`.
pub(crate) fn mid_batch(e: &crate::storage::engine::Engine) -> MidBatch {
    let (rt, rr) = std::sync::mpsc::channel();
    let (ft, fr) = std::sync::mpsc::channel::<bool>();
    let fr = std::sync::Mutex::new(fr);
    let armed = std::sync::atomic::AtomicBool::new(true);
    *e.core()
        .wal
        .points
        .batch_body
        .lock()
        .unwrap_or_else(crate::sync::PoisonError::into_inner) = Some(Box::new(move |_| {
        if !armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        let _ = rt.send(());
        if fr
            .lock()
            .unwrap_or_else(crate::sync::PoisonError::into_inner)
            .recv()
            .unwrap_or(false)
        {
            return Err(crate::storage::error::Error::Io(io::Error::from(
                io::ErrorKind::StorageFull,
            )));
        }
        Ok(())
    }));
    MidBatch {
        reached: rr,
        fail: ft,
    }
}

impl MidBatch {
    /// Waits until the batch is paused.
    pub(crate) fn wait(&self) {
        self.reached
            .recv_timeout(WAIT_BOUND)
            .expect("the commit reaches its batch");
    }
}
