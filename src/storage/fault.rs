//! Fault-injecting storage for durability tests (TS-30).
//!
//! [`FaultStorage`] wraps any [`Storage`] and treats it as the disk:
//!
//! - Writes, `set_len` and `reserve` since a file's last `sync_data` are kept
//!   in memory; reads see them. `sync_data` applies them to the wrapped file
//!   and syncs it.
//! - `create`, `rename` and `remove` act on the wrapped storage at once but
//!   stay undoable until `sync_dir` on their directory. A replaced or
//!   removed file is kept under a hidden backup name until then.
//! - [`FaultStorage::crash`] drops all unsynced state, or keeps a seeded
//!   random subset of it, or tears unsynced writes at 512-byte boundaries;
//!   then it undoes every unsynced namespace change. Handles opened before
//!   the crash fail afterwards.
//! - A chosen `sync_data` or `write_at` call fails with `EIO`. A failed
//!   `sync_data` leaves the writes unsynced.
//! - With a space budget set, `write_at` and `reserve` that would grow a file
//!   beyond it, and `create` with no space left, fail with `ENOSPC`;
//!   `available_space` reports what is left. The budget counts growth only:
//!   shrinking or removing files returns nothing.
//! - Every call is logged in order.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use rustix::io::Errno;

use super::{Storage, StorageFile, StorageLock};

/// Sector size at which [`Crash::Tear`] splits writes.
const SECTOR: u64 = 512;

/// What a simulated crash keeps of the unsynced writes. Unsynced namespace
/// changes are always lost.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Crash {
    /// Keeps nothing.
    DropAll,
    /// Keeps each unsynced operation with probability 1/2, in order.
    KeepRandom { seed: u64 },
    /// Keeps each 512-byte sector of each unsynced write with probability
    /// 1/2; drops unsynced `set_len` and `reserve`.
    Tear { seed: u64 },
}

/// One logged call. File calls carry the file's current path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    Open(PathBuf),
    Create(PathBuf),
    Remove(PathBuf),
    Rename(PathBuf, PathBuf),
    List(PathBuf),
    SyncDir(PathBuf),
    TryLock(PathBuf),
    AvailableSpace(PathBuf),
    ReadAt {
        path: PathBuf,
        offset: u64,
        len: u64,
    },
    WriteAt {
        path: PathBuf,
        offset: u64,
        len: u64,
    },
    SyncData(PathBuf),
    SetLen(PathBuf, u64),
    Len(PathBuf),
    Reserve {
        path: PathBuf,
        offset: u64,
        len: u64,
    },
}

/// An unsynced file operation.
#[derive(Debug)]
enum Op {
    Write(u64, Vec<u8>),
    SetLen(u64),
    Reserve(u64, u64),
}

/// An unsynced namespace change, with what undoes it.
#[derive(Debug)]
enum Pending {
    Created(PathBuf),
    Renamed {
        from: PathBuf,
        to: PathBuf,
        backup: Option<PathBuf>,
    },
    Removed {
        path: PathBuf,
        backup: PathBuf,
    },
}

impl Pending {
    /// Directory whose `sync_dir` makes the change durable.
    fn dir(&self) -> Option<&Path> {
        match self {
            Pending::Created(p)
            | Pending::Renamed { to: p, .. }
            | Pending::Removed { path: p, .. } => p.parent(),
        }
    }
}

struct FileState {
    inner: Arc<dyn StorageFile>,
    path: PathBuf,
    ops: Vec<Op>,
    /// Length as seen through the unsynced operations.
    len: u64,
    /// Index of the unsynced writes: start offset to the positions of the
    /// writes in `ops` that start there. With [`FileState::max_write`] it
    /// finds the writes a read overlaps without scanning every unsynced
    /// operation, which made reads quadratic in the unsynced writes.
    writes: BTreeMap<u64, Vec<usize>>,
    /// Length of the longest unsynced write.
    max_write: u64,
    /// Whether an unsynced `set_len` is pending; reads then scan `ops`.
    set_len: bool,
}

impl FileState {
    /// Records an unsynced operation.
    fn push(&mut self, op: Op) {
        match op {
            Op::Write(o, ref d) => {
                let n = d.len() as u64;
                self.len = self.len.max(o + n);
                self.max_write = self.max_write.max(n);
                self.writes.entry(o).or_default().push(self.ops.len());
            }
            Op::SetLen(l) => {
                self.len = l;
                self.set_len = true;
            }
            Op::Reserve(o, l) => self.len = self.len.max(o + l),
        }
        self.ops.push(op);
    }

    /// Forgets the unsynced operations once they are applied.
    fn clear(&mut self) {
        self.ops.clear();
        self.writes.clear();
        self.max_write = 0;
        self.set_len = false;
    }

    /// Copies the unsynced data into `buf`, which holds the wrapped file's
    /// bytes at `offset`, in operation order.
    fn overlay(&self, buf: &mut [u8], offset: u64) {
        let end = offset + buf.len() as u64;
        let mut copy = |op: &Op| match *op {
            Op::Write(o, ref d) => {
                let (start, stop) = (o.max(offset), (o + d.len() as u64).min(end));
                if start < stop {
                    buf[(start - offset) as usize..(stop - offset) as usize]
                        .copy_from_slice(&d[(start - o) as usize..(stop - o) as usize]);
                }
            }
            Op::SetLen(l) => {
                if l < end {
                    buf[(l.max(offset) - offset) as usize..].fill(0);
                }
            }
            Op::Reserve(..) => {}
        };
        if self.set_len {
            self.ops.iter().for_each(copy);
            return;
        }
        // Writes that overlap [offset, end) start in this range.
        let from = offset.saturating_sub(self.max_write.saturating_sub(1));
        let mut hits: Vec<usize> = self
            .writes
            .range(from..end)
            .flat_map(|(_, i)| i.iter().copied())
            .collect();
        hits.sort_unstable();
        for i in hits {
            copy(&self.ops[i]);
        }
    }

    fn apply(&self, op: &Op) -> io::Result<()> {
        match *op {
            Op::Write(o, ref d) => self.inner.write_at(d, o),
            Op::SetLen(l) => self.inner.set_len(l),
            Op::Reserve(o, l) => self.inner.reserve(o, l),
        }
    }
}

#[derive(Default)]
struct State {
    generation: u64,
    files: HashMap<u64, FileState>,
    names: HashMap<PathBuf, u64>,
    next_key: u64,
    pending: Vec<Pending>,
    next_backup: u64,
    log: Vec<Call>,
    sync_data_calls: u64,
    write_calls: u64,
    fail_sync_data: Option<u64>,
    fail_write: Option<u64>,
    space: Option<u64>,
    /// Set by [`FaultStorage::skip_device_sync`].
    skip_device_sync: bool,
}

impl State {
    fn add_file(&mut self, inner: Box<dyn StorageFile>, path: &Path) -> io::Result<u64> {
        let key = self.next_key;
        self.next_key += 1;
        let state = FileState {
            len: inner.len()?,
            inner: Arc::from(inner),
            path: path.to_path_buf(),
            ops: Vec::new(),
            writes: BTreeMap::new(),
            max_write: 0,
            set_len: false,
        };
        self.files.insert(key, state);
        self.names.insert(path.to_path_buf(), key);
        Ok(key)
    }

    fn backup_name(&mut self, path: &Path) -> PathBuf {
        self.next_backup += 1;
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".fault-backup-{}", self.next_backup));
        path.with_file_name(name)
    }

    fn is_backup(&self, dir: &Path, name: &OsStr) -> bool {
        self.pending.iter().any(|p| match p {
            Pending::Renamed {
                backup: Some(b), ..
            }
            | Pending::Removed { backup: b, .. } => {
                b.parent() == Some(dir) && b.file_name() == Some(name)
            }
            _ => false,
        })
    }

    /// Takes `growth` bytes from the space budget, if one is set.
    fn take_space(&mut self, growth: u64) -> io::Result<()> {
        match self.space {
            Some(left) if growth > left => Err(Errno::NOSPC.into()),
            Some(left) => {
                self.space = Some(left - growth);
                Ok(())
            }
            None => Ok(()),
        }
    }
}

/// Storage wrapper that injects crashes, I/O errors and a full device.
pub(crate) struct FaultStorage {
    inner: Box<dyn Storage>,
    state: Arc<Mutex<State>>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .expect("fault storage state poisoned by a panicking test")
}

impl FaultStorage {
    pub(crate) fn new(inner: Box<dyn Storage>) -> Self {
        FaultStorage {
            inner,
            state: Arc::default(),
        }
    }

    /// Simulates a crash: applies what `mode` keeps of the unsynced file
    /// operations to the wrapped storage, then undoes every unsynced
    /// namespace change. Files opened before the crash become unusable.
    pub(crate) fn crash(&self, mode: Crash) -> io::Result<()> {
        let mut st = lock(&self.state);
        let mut keys: Vec<u64> = st.files.keys().copied().collect();
        keys.sort_unstable();
        let mut rng = match mode {
            Crash::DropAll => None,
            Crash::KeepRandom { seed } | Crash::Tear { seed } => Some(StdRng::seed_from_u64(seed)),
        };
        for key in keys {
            let file = &st.files[&key];
            let Some(rng) = rng.as_mut() else { break };
            for op in &file.ops {
                match (mode, op) {
                    (Crash::KeepRandom { .. }, _) => {
                        if rng.random_bool(0.5) {
                            file.apply(op)?;
                        }
                    }
                    (Crash::Tear { .. }, Op::Write(offset, data)) => {
                        let end = offset + data.len() as u64;
                        let mut start = *offset;
                        while start < end {
                            let stop = ((start / SECTOR + 1) * SECTOR).min(end);
                            if rng.random_bool(0.5) {
                                let d = &data[(start - offset) as usize..(stop - offset) as usize];
                                file.inner.write_at(d, start)?;
                            }
                            start = stop;
                        }
                    }
                    _ => {}
                }
            }
        }
        while let Some(change) = st.pending.pop() {
            match change {
                Pending::Created(path) => self.inner.remove(&path)?,
                Pending::Renamed { from, to, backup } => {
                    self.inner.rename(&to, &from)?;
                    if let Some(backup) = backup {
                        self.inner.rename(&backup, &to)?;
                    }
                }
                Pending::Removed { path, backup } => self.inner.rename(&backup, &path)?,
            }
        }
        st.files.clear();
        st.names.clear();
        st.generation += 1;
        Ok(())
    }

    /// Makes the `n`-th `sync_data` call (1-based, counted since creation)
    /// fail with `EIO`.
    pub(crate) fn fail_sync_data(&self, n: u64) {
        lock(&self.state).fail_sync_data = Some(n);
    }

    /// Makes the `n`-th `write_at` call (1-based, counted since creation)
    /// fail with `EIO`.
    pub(crate) fn fail_write(&self, n: u64) {
        lock(&self.state).fail_write = Some(n);
    }

    /// Makes `sync_data` and `sync_dir` complete without asking the
    /// wrapped storage to sync: writes still reach the wrapped files and
    /// namespace changes still become durable for [`FaultStorage::crash`],
    /// but nothing waits for the device. For tests that count operations
    /// and do not crash the operating system, whose run time would
    /// otherwise be the device's flush latency.
    pub(crate) fn skip_device_sync(&self) {
        lock(&self.state).skip_device_sync = true;
    }

    /// Sets the bytes that writes and reservations may still add.
    pub(crate) fn set_space(&self, bytes: u64) {
        lock(&self.state).space = Some(bytes);
    }

    /// Returns the calls made so far, in order.
    pub(crate) fn calls(&self) -> Vec<Call> {
        lock(&self.state).log.clone()
    }

    fn handle(&self, key: u64, generation: u64) -> Box<dyn StorageFile> {
        Box::new(FaultFile {
            key,
            generation,
            state: Arc::clone(&self.state),
        })
    }
}

impl Storage for FaultStorage {
    fn open(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        let mut st = lock(&self.state);
        st.log.push(Call::Open(path.to_path_buf()));
        let key = match st.names.get(path) {
            Some(&key) => key,
            None => {
                let inner = self.inner.open(path)?;
                st.add_file(inner, path)?
            }
        };
        Ok(self.handle(key, st.generation))
    }

    fn create(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        let mut st = lock(&self.state);
        st.log.push(Call::Create(path.to_path_buf()));
        if st.space == Some(0) {
            return Err(Errno::NOSPC.into());
        }
        let inner = self.inner.create(path)?;
        st.pending.push(Pending::Created(path.to_path_buf()));
        let key = st.add_file(inner, path)?;
        Ok(self.handle(key, st.generation))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log.push(Call::Remove(path.to_path_buf()));
        let backup = st.backup_name(path);
        self.inner.rename(path, &backup)?;
        st.names.remove(path);
        st.pending.push(Pending::Removed {
            path: path.to_path_buf(),
            backup,
        });
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log
            .push(Call::Rename(from.to_path_buf(), to.to_path_buf()));
        let backup = st.backup_name(to);
        let backup = match self.inner.rename(to, &backup) {
            Ok(()) => Some(backup),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if let Err(e) = self.inner.rename(from, to) {
            if let Some(b) = &backup {
                self.inner.rename(b, to)?;
            }
            return Err(e);
        }
        st.names.remove(to);
        if let Some(key) = st.names.remove(from) {
            st.names.insert(to.to_path_buf(), key);
            if let Some(file) = st.files.get_mut(&key) {
                file.path = to.to_path_buf();
            }
        }
        st.pending.push(Pending::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            backup,
        });
        Ok(())
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<OsString>> {
        let mut st = lock(&self.state);
        st.log.push(Call::List(dir.to_path_buf()));
        let mut names = self.inner.list(dir)?;
        names.retain(|n| !st.is_backup(dir, n));
        Ok(names)
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log.push(Call::SyncDir(dir.to_path_buf()));
        if !st.skip_device_sync {
            self.inner.sync_dir(dir)?;
        }
        let (done, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut st.pending)
            .into_iter()
            .partition(|p| p.dir() == Some(dir));
        st.pending = kept;
        for change in done {
            match change {
                Pending::Renamed {
                    backup: Some(b), ..
                }
                | Pending::Removed { backup: b, .. } => self.inner.remove(&b)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> io::Result<Box<dyn StorageLock>> {
        lock(&self.state)
            .log
            .push(Call::TryLock(path.to_path_buf()));
        self.inner.try_lock(path)
    }

    fn available_space(&self, dir: &Path) -> io::Result<u64> {
        let mut st = lock(&self.state);
        st.log.push(Call::AvailableSpace(dir.to_path_buf()));
        let real = self.inner.available_space(dir)?;
        Ok(st.space.map_or(real, |left| left.min(real)))
    }
}

/// A file handle of [`FaultStorage`].
struct FaultFile {
    key: u64,
    generation: u64,
    state: Arc<Mutex<State>>,
}

impl FaultFile {
    /// Locks the state and checks that no crash happened since the open.
    fn state(&self) -> io::Result<MutexGuard<'_, State>> {
        let st = lock(&self.state);
        if st.generation != self.generation {
            return Err(io::Error::other(
                "file handle opened before a simulated crash",
            ));
        }
        Ok(st)
    }
}

impl StorageFile for FaultFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let mut st = self.state()?;
        let file = &st.files[&self.key];
        let path = file.path.clone();
        let n = file.inner.read_at(buf, offset)?;
        buf[n..].fill(0);
        file.overlay(buf, offset);
        let len = file.len;
        st.log.push(Call::ReadAt {
            path,
            offset,
            len: buf.len() as u64,
        });
        Ok(len.saturating_sub(offset).min(buf.len() as u64) as usize)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        let mut st = self.state()?;
        let path = st.files[&self.key].path.clone();
        st.log.push(Call::WriteAt {
            path,
            offset,
            len: buf.len() as u64,
        });
        st.write_calls += 1;
        if st.fail_write == Some(st.write_calls) {
            return Err(Errno::IO.into());
        }
        let growth = (offset + buf.len() as u64).saturating_sub(st.files[&self.key].len);
        st.take_space(growth)?;
        let file = st.files.get_mut(&self.key).expect("open file has state");
        file.push(Op::Write(offset, buf.to_vec()));
        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        let mut st = self.state()?;
        let path = st.files[&self.key].path.clone();
        st.log.push(Call::SyncData(path));
        st.sync_data_calls += 1;
        if st.fail_sync_data == Some(st.sync_data_calls) {
            return Err(Errno::IO.into());
        }
        let skip = st.skip_device_sync;
        let file = st.files.get_mut(&self.key).expect("open file has state");
        for op in &file.ops {
            file.apply(op)?;
        }
        file.clear();
        if skip {
            return Ok(());
        }
        file.inner.sync_data()
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        let mut st = self.state()?;
        let file = st.files.get_mut(&self.key).expect("open file has state");
        file.push(Op::SetLen(len));
        let path = file.path.clone();
        st.log.push(Call::SetLen(path, len));
        Ok(())
    }

    fn len(&self) -> io::Result<u64> {
        let mut st = self.state()?;
        let file = &st.files[&self.key];
        let (path, len) = (file.path.clone(), file.len);
        st.log.push(Call::Len(path));
        Ok(len)
    }

    fn reserve(&self, offset: u64, len: u64) -> io::Result<()> {
        let mut st = self.state()?;
        let path = st.files[&self.key].path.clone();
        st.log.push(Call::Reserve { path, offset, len });
        let growth = (offset + len).saturating_sub(st.files[&self.key].len);
        st.take_space(growth)?;
        let file = st.files.get_mut(&self.key).expect("open file has state");
        file.push(Op::Reserve(offset, len));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FsStorage;

    /// Directories the suite runs in: `LPG_LAB_EXT4` and `LPG_LAB_XFS` when
    /// set, else the system temporary directory.
    fn test_dirs() -> Vec<tempfile::TempDir> {
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

    fn fault() -> FaultStorage {
        FaultStorage::new(Box::new(FsStorage))
    }

    /// Reads a whole file through the wrapped storage, as after a restart.
    fn durable(path: &Path) -> io::Result<Vec<u8>> {
        let file = FsStorage.open(path)?;
        let mut buf = vec![0; file.len()? as usize];
        file.read_at(&mut buf, 0)?;
        Ok(buf)
    }

    /// Creates `dir/f` holding `base`, durably.
    fn durable_file(storage: &dyn Storage, dir: &Path, base: &[u8]) -> Box<dyn StorageFile> {
        let file = storage.create(&dir.join("f")).unwrap();
        file.write_at(base, 0).unwrap();
        file.sync_data().unwrap();
        storage.sync_dir(dir).unwrap();
        file
    }

    #[test]
    fn same_contract_through_both_impls() {
        for dir in test_dirs() {
            let impls: [(&str, Box<dyn Storage>); 2] =
                [("fs", Box::new(FsStorage)), ("fault", Box::new(fault()))];
            for (name, storage) in impls {
                let sub = tempfile::tempdir_in(dir.path()).unwrap();
                let d = sub.path();
                let file = storage.create(&d.join("a")).unwrap();
                file.write_at(&[1; 1000], 0).unwrap();
                file.set_len(600).unwrap();
                file.write_at(&[2; 10], 700).unwrap();
                file.reserve(0, 4096).unwrap();
                assert_eq!(file.len().unwrap(), 4096, "{name}");
                let mut buf = vec![9; 5000];
                assert_eq!(file.read_at(&mut buf, 0).unwrap(), 4096, "{name}");
                assert!(buf[..600].iter().all(|&b| b == 1), "{name}");
                assert!(buf[600..700].iter().all(|&b| b == 0), "{name}");
                assert!(buf[700..710].iter().all(|&b| b == 2), "{name}");
                assert!(buf[710..4096].iter().all(|&b| b == 0), "{name}");
                file.sync_data().unwrap();
                storage.rename(&d.join("a"), &d.join("b")).unwrap();
                storage.create(&d.join("c")).unwrap();
                storage.remove(&d.join("c")).unwrap();
                storage.sync_dir(d).unwrap();
                assert_eq!(storage.list(d).unwrap(), ["b"], "{name}");
                assert_eq!(storage.open(&d.join("b")).unwrap().len().unwrap(), 4096);
                let lock_path = d.join("LOCK");
                storage.create(&lock_path).unwrap();
                let held = storage.try_lock(&lock_path).unwrap();
                let again = storage.try_lock(&lock_path).err().unwrap();
                assert_eq!(again.kind(), io::ErrorKind::WouldBlock, "{name}");
                drop(held);
                assert!(storage.available_space(d).unwrap() > 0);
            }
        }
    }

    #[test]
    fn crash_drops_unsynced_writes() {
        for dir in test_dirs() {
            let s = fault();
            let file = durable_file(&s, dir.path(), &[1; 1024]);
            file.write_at(&[2; 1024], 0).unwrap();
            file.write_at(&[3; 512], 1024).unwrap();
            let mut buf = [0; 1536];
            assert_eq!(file.read_at(&mut buf, 0).unwrap(), 1536);
            assert_eq!(buf[0], 2);
            assert_eq!(buf[1535], 3);
            s.crash(Crash::DropAll).unwrap();
            assert_eq!(durable(&dir.path().join("f")).unwrap(), [1; 1024]);
            let err = file.read_at(&mut buf, 0).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::Other);
        }
    }

    #[test]
    fn crash_keeps_a_seeded_subset_of_unsynced_writes() {
        for dir in test_dirs() {
            let s = fault();
            let file = durable_file(&s, dir.path(), &[0; 64]);
            for i in 0..64u8 {
                file.write_at(&[i + 1], u64::from(i)).unwrap();
            }
            s.crash(Crash::KeepRandom { seed: 7 }).unwrap();
            let after = durable(&dir.path().join("f")).unwrap();
            let kept = after.iter().filter(|&&b| b != 0).count();
            assert!(0 < kept && kept < 64, "kept {kept} of 64");
            for (i, &b) in after.iter().enumerate() {
                assert!(b == 0 || b as usize == i + 1, "byte {i} is {b}");
            }
            // The same seed keeps the same subset.
            let file = s.open(&dir.path().join("f")).unwrap();
            file.write_at(&[0; 64], 0).unwrap();
            file.sync_data().unwrap();
            for i in 0..64u8 {
                file.write_at(&[i + 1], u64::from(i)).unwrap();
            }
            s.crash(Crash::KeepRandom { seed: 7 }).unwrap();
            assert_eq!(durable(&dir.path().join("f")).unwrap(), after);
        }
    }

    #[test]
    fn crash_tears_writes_at_sector_boundaries() {
        for dir in test_dirs() {
            let s = fault();
            let file = durable_file(&s, dir.path(), &[0; 8192]);
            // An unaligned write: sectors are cut at absolute 512-byte
            // boundaries, so the first and last pieces are partial.
            file.write_at(&[0xAB; 8000], 100).unwrap();
            s.crash(Crash::Tear { seed: 3 }).unwrap();
            let after = durable(&dir.path().join("f")).unwrap();
            assert_eq!(after.len(), 8192);
            assert!(after[..100].iter().all(|&b| b == 0));
            assert!(after[8100..].iter().all(|&b| b == 0));
            let mut kept = 0;
            for sector in 0..16 {
                let lo = (sector * 512).max(100);
                let hi = ((sector + 1) * 512).min(8100);
                let piece = &after[lo..hi];
                let first = piece[0];
                assert!(first == 0 || first == 0xAB);
                assert!(
                    piece.iter().all(|&b| b == first),
                    "sector {sector} torn inside"
                );
                kept += usize::from(first == 0xAB);
            }
            assert!(0 < kept && kept < 16, "kept {kept} of 16 sectors");
        }
    }

    #[test]
    fn crash_loses_unsynced_rename_create_and_remove() {
        for dir in test_dirs() {
            let d = dir.path();
            let s = fault();
            durable_file(&s, d, b"old");
            let g = s.create(&d.join("g")).unwrap();
            g.write_at(b"new", 0).unwrap();
            g.sync_data().unwrap();
            // Replace f by g, then create and remove other files, unsynced.
            s.rename(&d.join("g"), &d.join("f")).unwrap();
            s.create(&d.join("h")).unwrap();
            assert_eq!(s.list(d).unwrap(), ["f", "h"]);
            s.remove(&d.join("h")).unwrap();
            assert_eq!(s.list(d).unwrap(), ["f"]);
            s.crash(Crash::DropAll).unwrap();
            assert_eq!(FsStorage.list(d).unwrap(), ["f"]);
            assert_eq!(durable(&d.join("f")).unwrap(), b"old");
            // Once the directory is synced, the rename survives.
            let g = s.create(&d.join("g")).unwrap();
            g.write_at(b"new", 0).unwrap();
            g.sync_data().unwrap();
            s.sync_dir(d).unwrap();
            s.rename(&d.join("g"), &d.join("f")).unwrap();
            s.sync_dir(d).unwrap();
            s.crash(Crash::DropAll).unwrap();
            assert_eq!(FsStorage.list(d).unwrap(), ["f"]);
            assert_eq!(durable(&d.join("f")).unwrap(), b"new");
        }
    }

    #[test]
    fn failed_sync_data_leaves_writes_unsynced() {
        for dir in test_dirs() {
            let s = fault();
            let file = durable_file(&s, dir.path(), b"aaaa"); // sync_data call 1
            s.fail_sync_data(2);
            file.write_at(b"bb", 0).unwrap();
            let err = file.sync_data().unwrap_err();
            assert_eq!(err.raw_os_error(), Some(Errno::IO.raw_os_error()));
            let mut buf = [0; 4];
            file.read_at(&mut buf, 0).unwrap();
            assert_eq!(&buf, b"bbaa");
            s.crash(Crash::DropAll).unwrap();
            assert_eq!(durable(&dir.path().join("f")).unwrap(), b"aaaa");
        }
    }

    #[test]
    fn failed_write_is_not_applied() {
        for dir in test_dirs() {
            let s = fault();
            let file = durable_file(&s, dir.path(), b"aaaa"); // write call 1
            s.fail_write(2);
            let err = file.write_at(b"bb", 0).unwrap_err();
            assert_eq!(err.raw_os_error(), Some(Errno::IO.raw_os_error()));
            file.write_at(b"cc", 2).unwrap();
            file.sync_data().unwrap();
            assert_eq!(durable(&dir.path().join("f")).unwrap(), b"aacc");
        }
    }

    #[test]
    fn space_budget_returns_enospc() {
        for dir in test_dirs() {
            let d = dir.path();
            let s = fault();
            let file = durable_file(&s, d, &[0; 100]);
            s.set_space(1000);
            assert_eq!(s.available_space(d).unwrap(), 1000);
            file.reserve(0, 600).unwrap(); // grows by 500
            assert_eq!(s.available_space(d).unwrap(), 500);
            // Writes inside the file use no space.
            file.write_at(&[1; 600], 0).unwrap();
            let full = file.reserve(600, 501).unwrap_err();
            assert_eq!(full.kind(), io::ErrorKind::StorageFull);
            let full = file.write_at(&[1; 501], 600).unwrap_err();
            assert_eq!(full.kind(), io::ErrorKind::StorageFull);
            file.write_at(&[1; 500], 600).unwrap();
            assert_eq!(s.available_space(d).unwrap(), 0);
            let full = s.create(&d.join("g")).err().unwrap();
            assert_eq!(full.kind(), io::ErrorKind::StorageFull);
            assert_eq!(file.len().unwrap(), 1100);
        }
    }

    #[test]
    fn calls_are_logged_in_order() {
        for dir in test_dirs() {
            let d = dir.path();
            let (f, g) = (d.join("f"), d.join("g"));
            let s = fault();
            let file = s.create(&f).unwrap();
            file.reserve(0, 8192).unwrap();
            file.write_at(&[1; 8192], 0).unwrap();
            file.sync_data().unwrap();
            file.read_at(&mut [0; 4096], 4096).unwrap();
            file.set_len(10).unwrap();
            file.len().unwrap();
            s.sync_dir(d).unwrap();
            s.rename(&f, &g).unwrap();
            file.sync_data().unwrap();
            s.list(d).unwrap();
            s.open(&g).unwrap();
            s.try_lock(&g).unwrap();
            s.available_space(d).unwrap();
            s.remove(&g).unwrap();
            let expected = vec![
                Call::Create(f.clone()),
                Call::Reserve {
                    path: f.clone(),
                    offset: 0,
                    len: 8192,
                },
                Call::WriteAt {
                    path: f.clone(),
                    offset: 0,
                    len: 8192,
                },
                Call::SyncData(f.clone()),
                Call::ReadAt {
                    path: f.clone(),
                    offset: 4096,
                    len: 4096,
                },
                Call::SetLen(f.clone(), 10),
                Call::Len(f.clone()),
                Call::SyncDir(d.to_path_buf()),
                Call::Rename(f.clone(), g.clone()),
                Call::SyncData(g.clone()),
                Call::List(d.to_path_buf()),
                Call::Open(g.clone()),
                Call::TryLock(g.clone()),
                Call::AvailableSpace(d.to_path_buf()),
                Call::Remove(g.clone()),
            ];
            assert_eq!(s.calls(), expected);
        }
    }
}
