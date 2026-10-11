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
//!   random subset of it, or tears unsynced writes at 512-byte boundaries.
//!   Of the unsynced namespace changes it keeps none ([`Crash::DropAll`]) or
//!   a seeded prefix, in the order they were made (the other modes), and
//!   undoes the rest: a journaling file system commits metadata changes in
//!   order, so a crash can keep an unsynced `create` or `rename` but not a
//!   later one without an earlier one (basis not verified here against the
//!   ext4 or XFS source; it is the ordered-metadata model). Handles opened
//!   before the crash fail afterwards.
//! - The `n`-th call of a kind ([`Kind`]) fails with `EIO` or `ENOSPC`
//!   ([`FaultStorage::fail_nth`]), without effect. A failed `sync_data`
//!   behaves as Linux after a write-back error (TS-30 R8): the file's
//!   unsynced writes stay readable but stop being pending, so a later
//!   `sync_data` succeeds without making them durable and a crash loses
//!   them. PostgreSQL documents this: the data may be "entirely forgotten"
//!   and a retry "may be reported as successful" (R-PG
//!   `doc/src/sgml/config.sgml:12258-12264`).
//! - After the `k`-th crash-point call (`write_at`, `sync_data`,
//!   `sync_dir`, `rename`, `create`, `remove`), every call fails with `EIO`
//!   until [`FaultStorage::crash`]: the process "stops" right after that
//!   call (TS-30 R1, [`FaultStorage::crash_after`]). SQLite's crash VFS and
//!   Turso's simulator crash at a chosen sync the same way (R-SQLITE
//!   `src/test6.c`; R-TURSO `testing/simulator`).
//! - **Silent faults** (TS-30 R9). A misdirected write
//!   ([`FaultStorage::misdirect_nth`]) reaches the device at another offset:
//!   reads in this process still see it at its own offset, as from the
//!   operating-system cache, but what becomes durable is the write at the
//!   wrong offset. A lost write ([`FaultStorage::lose_nth`]) is acknowledged
//!   and stays readable in this process, but no `sync_data` makes it
//!   durable and a crash drops it.
//! - With a space budget set, `write_at` and `reserve` that would grow a file
//!   beyond it, and `create` with no space left, fail with `ENOSPC`;
//!   `available_space` reports what is left. The budget counts growth only:
//!   shrinking or removing files returns nothing.
//! - Every call is logged in order.

use crate::sync::{Arc, Mutex, MutexGuard};
use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use rustix::io::Errno;

use super::{Storage, StorageFile, StorageLock};

/// Sector size at which [`Crash::Tear`] splits writes.
const SECTOR: u64 = 512;

/// What a simulated crash keeps of the unsynced writes and of the
/// unsynced namespace changes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Crash {
    /// Keeps nothing.
    DropAll,
    /// Keeps each unsynced operation with probability 1/2, in order, and a
    /// seeded prefix of the unsynced namespace changes.
    KeepRandom { seed: u64 },
    /// Keeps each 512-byte sector of each unsynced write with probability
    /// 1/2; drops unsynced `set_len` and `reserve`; keeps a seeded prefix of
    /// the unsynced namespace changes.
    Tear { seed: u64 },
}

/// A kind of call that [`FaultStorage::fail_nth`] can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Kind {
    ReadAt,
    WriteAt,
    SyncData,
    SyncDir,
    Create,
    Rename,
    Remove,
    Reserve,
}

impl Kind {
    /// Every kind.
    pub(crate) const ALL: [Kind; 8] = [
        Kind::ReadAt,
        Kind::WriteAt,
        Kind::SyncData,
        Kind::SyncDir,
        Kind::Create,
        Kind::Rename,
        Kind::Remove,
        Kind::Reserve,
    ];

    /// Whether a call of this kind is a crash point (TS-30 R1, rmp #20).
    fn is_crash_point(self) -> bool {
        !matches!(self, Kind::ReadAt | Kind::Reserve)
    }
}

/// The error an injected failure returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// `EIO`.
    Eio,
    /// `ENOSPC`.
    NoSpace,
}

impl Fault {
    fn error(self) -> io::Error {
        match self {
            Fault::Eio => Errno::IO.into(),
            Fault::NoSpace => Errno::NOSPC.into(),
        }
    }
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
    /// The device side of a misdirected write: applied by `sync_data`, but
    /// never seen by reads of this process.
    Hidden(u64, Vec<u8>),
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

/// Applies `op` to `buf`, which holds the bytes at `offset`.
fn copy_op(op: &Op, buf: &mut [u8], offset: u64) {
    let end = offset + buf.len() as u64;
    match *op {
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
        Op::Reserve(..) | Op::Hidden(..) => {}
    }
}

struct FileState {
    inner: Arc<dyn StorageFile>,
    path: PathBuf,
    /// Writes a failed `sync_data` dropped from `ops`: still readable,
    /// never applied, lost at a crash (R8).
    ghost: Vec<Op>,
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
                self.patch_ghosts(o, d);
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
            Op::Hidden(..) => {}
        }
        self.ops.push(op);
    }

    /// Copies a newer write at `o` into the ghost writes it overlaps, so
    /// that reads, which overlay the ghosts on the file, see the newest
    /// bytes. The ghosts still never become durable.
    fn patch_ghosts(&mut self, o: u64, d: &[u8]) {
        let end = o + d.len() as u64;
        for g in &mut self.ghost {
            if let Op::Write(go, gd) = g {
                let gend = *go + gd.len() as u64;
                let (start, stop) = (o.max(*go), end.min(gend));
                if start < stop {
                    gd[(start - *go) as usize..(stop - *go) as usize]
                        .copy_from_slice(&d[(start - o) as usize..(stop - o) as usize]);
                }
            }
        }
    }

    /// Forgets the unsynced operations once they are applied.
    fn clear(&mut self) {
        self.ops.clear();
        self.writes.clear();
        self.max_write = 0;
        self.set_len = false;
    }

    /// Copies the unsynced data into `buf`, which holds the wrapped file's
    /// bytes at `offset`: the pending operations in order, then the writes
    /// that never become durable (a failed sync's, R8; lost or misdirected
    /// ones, R9). Those hold the newest bytes of their range, since every
    /// later write is copied into them ([`FileState::patch_ghosts`]).
    fn overlay(&self, buf: &mut [u8], offset: u64) {
        if self.set_len {
            for op in &self.ops {
                copy_op(op, buf, offset);
            }
        } else {
            // Writes that overlap [offset, end) start in this range.
            let end = offset + buf.len() as u64;
            let from = offset.saturating_sub(self.max_write.saturating_sub(1));
            let mut hits: Vec<usize> = self
                .writes
                .range(from..end)
                .flat_map(|(_, i)| i.iter().copied())
                .collect();
            hits.sort_unstable();
            for i in hits {
                copy_op(&self.ops[i], buf, offset);
            }
        }
        for op in &self.ghost {
            copy_op(op, buf, offset);
        }
    }

    fn apply(&self, op: &Op) -> io::Result<()> {
        match *op {
            Op::Write(o, ref d) => self.inner.write_at(d, o),
            Op::SetLen(l) => self.inner.set_len(l),
            Op::Reserve(o, l) => crate::test_support::reserve(&*self.inner, &self.path, o, l),
            Op::Hidden(o, ref d) => self.inner.write_at(d, o),
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
    /// Calls of each kind so far.
    counts: HashMap<Kind, u64>,
    /// `(kind, n, fault)`: the `n`-th call of `kind` fails with `fault`.
    fail: Vec<(Kind, u64, Fault)>,
    /// `(n, delta)`: the `n`-th `write_at` reaches the device `delta` bytes
    /// further on (R9).
    misdirect: Vec<(u64, u64)>,
    /// The `n`-th `write_at` calls that are lost (R9).
    lose: Vec<u64>,
    /// Crash-point calls so far.
    points: u64,
    /// After this many crash-point calls, every call fails.
    crash_after: Option<u64>,
    /// Set once the crash point is passed.
    dead: bool,
    space: Option<u64>,
    /// Set by [`FaultStorage::skip_device_sync`].
    skip_device_sync: bool,
    /// Bytes each successful `read_at` and `write_at` moved, by kind and
    /// file name ([`FaultStorage::bytes`]; rmp #54).
    bytes: HashMap<(Kind, OsString), u64>,
}

impl State {
    /// Counts a call of `kind` and returns its injected failure, if any:
    /// `EIO` once the crash point is passed, else a [`FaultStorage::fail_nth`]
    /// fault. A crash-point call that passes is counted towards
    /// [`FaultStorage::crash_after`].
    fn call(&mut self, kind: Kind) -> io::Result<()> {
        if self.dead {
            return Err(Errno::IO.into());
        }
        let n = self.counts.entry(kind).or_insert(0);
        *n += 1;
        let n = *n;
        if let Some(&(_, _, fault)) = self.fail.iter().find(|&&(k, m, _)| k == kind && m == n) {
            return Err(fault.error());
        }
        Ok(())
    }

    /// Records that a crash-point call of `kind` took effect.
    fn passed(&mut self, kind: Kind) {
        if kind.is_crash_point() {
            self.points += 1;
            if self.crash_after == Some(self.points) {
                self.dead = true;
            }
        }
    }

    fn add_file(&mut self, inner: Box<dyn StorageFile>, path: &Path) -> io::Result<u64> {
        let key = self.next_key;
        self.next_key += 1;
        let state = FileState {
            len: inner.len()?,
            inner: Arc::from(inner),
            path: path.to_path_buf(),
            ghost: Vec::new(),
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
        // A seeded prefix of the namespace changes survives; the rest is
        // undone, newest first.
        let kept = rng
            .as_mut()
            .map_or(0, |rng| rng.random_range(0..=st.pending.len()));
        let undone = st.pending.split_off(kept);
        for change in undone.into_iter().rev() {
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
        for change in std::mem::take(&mut st.pending) {
            match change {
                Pending::Renamed {
                    backup: Some(b), ..
                }
                | Pending::Removed { backup: b, .. } => self.inner.remove(&b)?,
                _ => {}
            }
        }
        st.files.clear();
        st.names.clear();
        st.generation += 1;
        st.dead = false;
        st.crash_after = None;
        Ok(())
    }

    /// Makes the `n`-th `sync_data` call (1-based, counted since creation)
    /// fail with `EIO`.
    pub(crate) fn fail_sync_data(&self, n: u64) {
        self.fail_nth(Kind::SyncData, n, Fault::Eio);
    }

    /// Makes the `n`-th `write_at` call (1-based, counted since creation)
    /// fail with `EIO`.
    pub(crate) fn fail_write(&self, n: u64) {
        self.fail_nth(Kind::WriteAt, n, Fault::Eio);
    }

    /// Makes the `n`-th call of `kind` (1-based, counted since creation)
    /// fail with `fault`, without effect (TS-30 R2).
    pub(crate) fn fail_nth(&self, kind: Kind, n: u64, fault: Fault) {
        lock(&self.state).fail.push((kind, n, fault));
    }

    /// Makes the `n`-th `write_at` call (1-based, counted since creation)
    /// a misdirected write: it succeeds, reads in this process see it at
    /// its offset, but the device receives it `delta` bytes further on,
    /// where `sync_data` makes it durable (TS-30 R9).
    pub(crate) fn misdirect_nth(&self, n: u64, delta: u64) {
        lock(&self.state).misdirect.push((n, delta));
    }

    /// Makes the `n`-th `write_at` call (1-based, counted since creation) a
    /// lost write: it succeeds and stays readable in this process, but no
    /// `sync_data` makes it durable and a crash drops it (TS-30 R9).
    pub(crate) fn lose_nth(&self, n: u64) {
        lock(&self.state).lose.push(n);
    }

    /// Bytes the successful calls of `kind` (`ReadAt` or `WriteAt`) moved
    /// so far to or from files whose name starts with `prefix`: the
    /// temporary-file bytes of a sort, for example (rmp #54). A read counts
    /// the bytes it returned.
    pub(crate) fn bytes(&self, kind: Kind, prefix: &str) -> u64 {
        lock(&self.state)
            .bytes
            .iter()
            .filter(|((k, name), _)| *k == kind && name.to_string_lossy().starts_with(prefix))
            .map(|(_, n)| n)
            .sum()
    }

    /// Calls of `kind` made so far.
    pub(crate) fn count(&self, kind: Kind) -> u64 {
        lock(&self.state).counts.get(&kind).copied().unwrap_or(0)
    }

    /// Crash-point calls made so far.
    pub(crate) fn points(&self) -> u64 {
        lock(&self.state).points
    }

    /// After `k` more crash-point calls, every call fails with `EIO` until
    /// [`FaultStorage::crash`] (TS-30 R1). With `k` = 0 the next call
    /// already fails.
    pub(crate) fn crash_after(&self, k: u64) {
        let mut st = lock(&self.state);
        st.crash_after = Some(st.points + k);
        st.dead = k == 0;
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
        if st.dead {
            return Err(Errno::IO.into());
        }
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
        st.call(Kind::Create)?;
        if st.space == Some(0) {
            return Err(Errno::NOSPC.into());
        }
        let inner = self.inner.create(path)?;
        st.pending.push(Pending::Created(path.to_path_buf()));
        let key = st.add_file(inner, path)?;
        st.passed(Kind::Create);
        Ok(self.handle(key, st.generation))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log.push(Call::Remove(path.to_path_buf()));
        st.call(Kind::Remove)?;
        let backup = st.backup_name(path);
        self.inner.rename(path, &backup)?;
        st.names.remove(path);
        st.pending.push(Pending::Removed {
            path: path.to_path_buf(),
            backup,
        });
        st.passed(Kind::Remove);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log
            .push(Call::Rename(from.to_path_buf(), to.to_path_buf()));
        st.call(Kind::Rename)?;
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
        st.passed(Kind::Rename);
        Ok(())
    }

    /// As [`FaultStorage::rename`], through the wrapped storage's
    /// no-replace rename; nothing is replaced, so nothing is backed up.
    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log
            .push(Call::Rename(from.to_path_buf(), to.to_path_buf()));
        st.call(Kind::Rename)?;
        self.inner.rename_noreplace(from, to)?;
        if let Some(key) = st.names.remove(from) {
            st.names.insert(to.to_path_buf(), key);
            if let Some(file) = st.files.get_mut(&key) {
                file.path = to.to_path_buf();
            }
        }
        st.pending.push(Pending::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            backup: None,
        });
        st.passed(Kind::Rename);
        Ok(())
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<OsString>> {
        let mut st = lock(&self.state);
        st.log.push(Call::List(dir.to_path_buf()));
        if st.dead {
            return Err(Errno::IO.into());
        }
        let mut names = self.inner.list(dir)?;
        names.retain(|n| !st.is_backup(dir, n));
        Ok(names)
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        let mut st = lock(&self.state);
        st.log.push(Call::SyncDir(dir.to_path_buf()));
        st.call(Kind::SyncDir)?;
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
        st.passed(Kind::SyncDir);
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> io::Result<Box<dyn StorageLock>> {
        let mut st = lock(&self.state);
        st.log.push(Call::TryLock(path.to_path_buf()));
        if st.dead {
            return Err(Errno::IO.into());
        }
        self.inner.try_lock(path)
    }

    fn available_space(&self, dir: &Path) -> io::Result<u64> {
        let mut st = lock(&self.state);
        st.log.push(Call::AvailableSpace(dir.to_path_buf()));
        if st.dead {
            return Err(Errno::IO.into());
        }
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
        if st.dead {
            return Err(Errno::IO.into());
        }
        Ok(st)
    }
}

impl StorageFile for FaultFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let mut st = self.state()?;
        st.call(Kind::ReadAt)?;
        let file = &st.files[&self.key];
        let path = file.path.clone();
        let n = file.inner.read_at(buf, offset)?;
        buf[n..].fill(0);
        file.overlay(buf, offset);
        let len = file.len;
        let got = len.saturating_sub(offset).min(buf.len() as u64);
        let name = path.file_name().unwrap_or_default().to_os_string();
        *st.bytes.entry((Kind::ReadAt, name)).or_insert(0) += got;
        st.log.push(Call::ReadAt {
            path,
            offset,
            len: buf.len() as u64,
        });
        Ok(got as usize)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        let mut st = self.state()?;
        let path = st.files[&self.key].path.clone();
        st.log.push(Call::WriteAt {
            path,
            offset,
            len: buf.len() as u64,
        });
        st.call(Kind::WriteAt)?;
        let growth = (offset + buf.len() as u64).saturating_sub(st.files[&self.key].len);
        st.take_space(growth)?;
        let n = st.counts[&Kind::WriteAt];
        let lost = st.lose.contains(&n);
        let misdirected = st.misdirect.iter().find(|&&(m, _)| m == n).map(|&(_, d)| d);
        let file = st.files.get_mut(&self.key).expect("open file has state");
        if lost || misdirected.is_some() {
            // Readable here, never durable (R9).
            file.patch_ghosts(offset, buf);
            file.ghost.push(Op::Write(offset, buf.to_vec()));
            file.len = file.len.max(offset + buf.len() as u64);
            if let Some(delta) = misdirected {
                file.push(Op::Hidden(offset + delta, buf.to_vec()));
            }
        } else {
            file.push(Op::Write(offset, buf.to_vec()));
        }
        let name = file.path.file_name().unwrap_or_default().to_os_string();
        *st.bytes.entry((Kind::WriteAt, name)).or_insert(0) += buf.len() as u64;
        st.passed(Kind::WriteAt);
        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        let mut st = self.state()?;
        let path = st.files[&self.key].path.clone();
        st.log.push(Call::SyncData(path));
        if let Err(e) = st.call(Kind::SyncData) {
            if !st.dead {
                // R8: the unsynced writes stay readable but are no longer
                // pending; nothing makes them durable any more.
                let file = st.files.get_mut(&self.key).expect("open file has state");
                let ops = std::mem::take(&mut file.ops);
                file.ghost.extend(ops);
                file.clear();
            }
            return Err(e);
        }
        let skip = st.skip_device_sync;
        let file = st.files.get_mut(&self.key).expect("open file has state");
        for op in &file.ops {
            file.apply(op)?;
        }
        file.clear();
        if !skip {
            file.inner.sync_data()?;
        }
        st.passed(Kind::SyncData);
        Ok(())
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
        st.call(Kind::Reserve)?;
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
    use crate::test_support::test_dirs;

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
            let sd = crate::test_support::seed();
            s.crash(Crash::KeepRandom { seed: sd }).unwrap();
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
            s.crash(Crash::KeepRandom { seed: sd }).unwrap();
            assert_eq!(durable(&dir.path().join("f")).unwrap(), after);
        }
    }

    #[test]
    fn crash_tears_writes_at_sector_boundaries() {
        // 128 sectors, so that no seed keeps all or none of them in
        // practice (probability 2^-127): the seed rotates (TS-30 R5).
        const LEN: usize = 128 * 512;
        for dir in test_dirs() {
            let s = fault();
            let file = durable_file(&s, dir.path(), &[0; LEN]);
            // An unaligned write: sectors are cut at absolute 512-byte
            // boundaries, so the first and last pieces are partial.
            file.write_at(&[0xAB; LEN - 200], 100).unwrap();
            s.crash(Crash::Tear {
                seed: crate::test_support::seed(),
            })
            .unwrap();
            let after = durable(&dir.path().join("f")).unwrap();
            assert_eq!(after.len(), LEN);
            assert!(after[..100].iter().all(|&b| b == 0));
            assert!(after[LEN - 100..].iter().all(|&b| b == 0));
            let mut kept = 0;
            for sector in 0..LEN / 512 {
                let lo = (sector * 512).max(100);
                let hi = ((sector + 1) * 512).min(LEN - 100);
                let piece = &after[lo..hi];
                let first = piece[0];
                assert!(first == 0 || first == 0xAB);
                assert!(
                    piece.iter().all(|&b| b == first),
                    "sector {sector} torn inside"
                );
                kept += usize::from(first == 0xAB);
            }
            assert!(0 < kept && kept < LEN / 512, "kept {kept} of 128 sectors");
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

    /// Audit finding 7 (2026-10-03): the crash modes other than
    /// [`Crash::DropAll`] keep a seeded prefix of the unsynced namespace
    /// changes, in order, so a crash can keep an unsynced `create` or
    /// `rename` as well as lose it, but never keeps a later change without
    /// an earlier one. Over 64 seeds, an unsynced `create` of `h` followed
    /// by an unsynced `rename` of `g` over `f` gives all three prefixes.
    #[test]
    fn crash_keeps_or_loses_an_unsynced_create_and_rename_in_order() {
        let dir = crate::test_support::graph_dir();
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..64u64 {
            let sub = tempfile::tempdir_in(dir.path()).unwrap();
            let d = sub.path();
            let s = fault();
            durable_file(&s, d, b"old");
            let g = s.create(&d.join("g")).unwrap();
            g.write_at(b"new", 0).unwrap();
            g.sync_data().unwrap();
            drop(g);
            s.sync_dir(d).unwrap();
            s.create(&d.join("h")).unwrap();
            s.rename(&d.join("g"), &d.join("f")).unwrap();
            let mode = if seed % 2 == 0 {
                Crash::KeepRandom { seed }
            } else {
                Crash::Tear { seed }
            };
            s.crash(mode).unwrap();
            let names = FsStorage.list(d).unwrap();
            let kept = match (
                names.iter().any(|n| n == "h"),
                names.iter().any(|n| n == "g"),
            ) {
                (false, true) => 0, // nothing kept
                (true, true) => 1,  // the create only
                (true, false) => 2, // the create and the rename
                (false, false) => panic!("seed {seed}: the rename was kept without the create"),
            };
            let f = durable(&d.join("f")).unwrap();
            assert_eq!(
                f,
                if kept == 2 { &b"new"[..] } else { &b"old"[..] },
                "seed {seed}"
            );
            seen.insert(kept);
        }
        assert_eq!(seen.len(), 3, "prefixes seen: {seen:?}");
    }

    /// TS-30 R8: after a failed `sync_data` the unsynced writes stay
    /// readable but are forgotten: a later `sync_data` succeeds without
    /// making them durable, and a crash loses them, as Linux does after a
    /// write-back error.
    #[test]
    fn failed_sync_data_forgets_the_unsynced_writes() {
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
            file.write_at(b"c", 3).unwrap();
            file.sync_data().unwrap();
            file.read_at(&mut buf, 0).unwrap();
            assert_eq!(&buf, b"bbac");
            assert_eq!(durable(&dir.path().join("f")).unwrap(), b"aaac");
            s.crash(Crash::DropAll).unwrap();
            assert_eq!(durable(&dir.path().join("f")).unwrap(), b"aaac");
        }
    }

    /// TS-30 R2: the `n`-th call of each kind fails with the chosen error
    /// and has no effect; the calls around it work.
    #[test]
    fn the_nth_call_of_each_kind_fails_without_effect() {
        for dir in test_dirs() {
            let d = dir.path();
            for fault_kind in [Fault::Eio, Fault::NoSpace] {
                let s = fault();
                let want = |e: io::Error| match fault_kind {
                    Fault::Eio => assert_eq!(e.raw_os_error(), Some(Errno::IO.raw_os_error())),
                    Fault::NoSpace => assert_eq!(e.kind(), io::ErrorKind::StorageFull),
                };
                let n = |k: Kind| s.count(k) + 1;
                s.fail_nth(Kind::Create, n(Kind::Create), fault_kind);
                want(s.create(&d.join("x")).err().unwrap());
                let f = s.create(&d.join("x")).unwrap();
                s.fail_nth(Kind::WriteAt, n(Kind::WriteAt), fault_kind);
                want(f.write_at(b"zz", 0).unwrap_err());
                f.write_at(b"ok", 0).unwrap();
                s.fail_nth(Kind::Reserve, n(Kind::Reserve), fault_kind);
                want(f.reserve(0, 100).unwrap_err());
                assert_eq!(f.len().unwrap(), 2);
                s.fail_nth(Kind::ReadAt, n(Kind::ReadAt), fault_kind);
                want(f.read_at(&mut [0; 2], 0).unwrap_err());
                s.fail_nth(Kind::SyncDir, n(Kind::SyncDir), fault_kind);
                want(s.sync_dir(d).unwrap_err());
                f.sync_data().unwrap();
                s.sync_dir(d).unwrap();
                s.fail_nth(Kind::Rename, n(Kind::Rename), fault_kind);
                want(s.rename(&d.join("x"), &d.join("y")).unwrap_err());
                s.fail_nth(Kind::Remove, n(Kind::Remove), fault_kind);
                want(s.remove(&d.join("x")).unwrap_err());
                assert_eq!(s.list(d).unwrap(), ["x"]);
                s.remove(&d.join("x")).unwrap();
                s.sync_dir(d).unwrap();
            }
        }
    }

    /// TS-30 R1: after the `k`-th crash-point call every call fails, until
    /// the crash; the crash keeps what was synced before the crash point.
    #[test]
    fn after_the_crash_point_every_call_fails_until_the_crash() {
        for dir in test_dirs() {
            let d = dir.path();
            let s = fault();
            let file = durable_file(&s, d, b"aaaa");
            let before = s.points();
            s.crash_after(2);
            file.write_at(b"b", 0).unwrap(); // point 1
            file.sync_data().unwrap(); // point 2: the last that runs
            assert_eq!(s.points(), before + 2);
            assert!(file.write_at(b"c", 1).is_err());
            assert!(file.read_at(&mut [0; 1], 0).is_err());
            assert!(s.list(d).is_err() && s.create(&d.join("g")).is_err());
            s.crash(Crash::DropAll).unwrap();
            assert_eq!(durable(&d.join("f")).unwrap(), b"baaa");
            let f = s.open(&d.join("f")).unwrap();
            f.write_at(b"d", 1).unwrap();
            f.sync_data().unwrap();
            assert_eq!(durable(&d.join("f")).unwrap(), b"bdaa");
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
