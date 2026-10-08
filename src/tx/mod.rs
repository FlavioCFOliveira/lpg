//! Transactions (TS-14, TS-15, TS-16).
//!
//! A `Transaction` reads one committed snapshot and keeps its writes in a
//! private write set until it commits (FR-TX-001, FR-TX-006). This is the
//! crate-internal API over trees: `Engine::begin`, `Transaction::get`,
//! `Transaction::scan_prefix`, `Transaction::put`,
//! `Transaction::delete`, `Transaction::drop_tree`,
//! `Transaction::commit` and `Transaction::abort`. The public API is
//! Sprint 3's.
//!
//! - **Snapshot.** At begin, the highest published commit timestamp,
//!   registered with the protocol of `mvcc` (TS-14 Snapshot registry).
//! - **Reads** merge the write set over the snapshot. A page entry is
//!   visible when its `created_ts` is at or below the snapshot and its
//!   `deleted_ts` is absent or above it; an entry created after the
//!   snapshot is replaced by the newest older version in the old-version
//!   store (`versions`), if any (TS-14).
//! - **Writes** take an X lock on the key and an S lock on its tree (TS-15
//!   Lock table), waiting while another transaction holds a conflicting
//!   lock (FR-TX-005). Once the lock is held, an entry changed after the
//!   snapshot fails the write with `Error::Serialization` (TS-15
//!   Write-write rule). Each write reserves its bytes, its before-image,
//!   its write key and its lock entries from the memory accountant (TS-14);
//!   a refused reservation fails the operation with
//!   `Error::MemoryLimit` (FR-LIM-005, FR-LIM-007).
//! - **Failures** of an operation with `Error::Serialization`,
//!   `Error::Deadlock` or `Error::MemoryLimit` abort the transaction:
//!   its locks are released at once, and every later operation fails with
//!   `Error::Aborted`. Nothing is retried (BR-TX-001).
//! - **Serializable** transactions (the default, FR-TX-003) record their
//!   reads as intervals (`conflict`) and are validated at commit; under
//!   **snapshot isolation** (FR-TX-004) they are not.
//! - A transaction dropped without commit or abort is aborted.

pub(crate) mod commit;
pub(crate) mod conflict;
pub(crate) mod gc;
pub(crate) mod lock;
pub(crate) mod mvcc;
pub(crate) mod versions;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::{BTreeMap, HashMap};

use conflict::{Interval, ReadSet, WriteKeys};
use lock::{Granted, Mode, Resource, TxId};
use versions::{Older, version_bytes, visible};

use crate::storage::btree::{DIRECTORY, MAX_KEY, Tree};
use crate::storage::engine::Engine;
use crate::storage::error::{Error, Result};
use crate::storage::freelist::Registered;
use crate::storage::memory::Reservation;

/// The isolation level of a transaction (FR-TX-003, FR-TX-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Isolation {
    /// Serializable: the default.
    #[default]
    Serializable,
    /// Snapshot isolation: no read-set validation.
    Snapshot,
}

/// Bytes a pending write is charged with beyond its key and value.
const WRITE_OVERHEAD: u64 = 64;

/// A pending write of the write set.
#[derive(Debug)]
pub(crate) struct Pending {
    /// The tree written.
    pub(crate) tree: Tree,
    /// The value to put, or `None` for a delete stamp.
    pub(crate) value: Option<Vec<u8>>,
}

/// What a transaction knows of a tree it used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeUse {
    /// Readable at the snapshot.
    Read,
    /// Locked S for writing.
    Write,
}

/// The state of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Active,
    Aborted,
}

/// A transaction (TS-14).
pub(crate) struct Transaction<'e> {
    engine: &'e Engine,
    id: TxId,
    pub(crate) isolation: Isolation,
    pub(crate) snapshot: u64,
    /// The snapshot registrations; dropping them ends the snapshot.
    regs: Option<(Registered<'e>, Option<Registered<'e>>)>,
    /// The private write set, by `(tree, key)`.
    pub(crate) writes: BTreeMap<(u64, Vec<u8>), Pending>,
    /// Trees dropped by this transaction.
    pub(crate) drops: Vec<Tree>,
    /// The read set (serializable transactions).
    pub(crate) reads: ReadSet,
    /// The memory of the write set, before-images, write keys and lock
    /// entries (TS-14).
    pub(crate) charge: Option<Reservation>,
    trees: HashMap<u64, TreeUse>,
    state: State,
}

impl std::fmt::Debug for Transaction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("id", &self.id)
            .field("isolation", &self.isolation)
            .field("snapshot", &self.snapshot)
            .field("writes", &self.writes.len())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Begins a transaction at `isolation` (TS-14 Begin).
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`].
    pub(crate) fn begin(&self, isolation: Isolation) -> Result<Transaction<'_>> {
        let core = self.core();
        core.health.check()?;
        let mvcc = &core.mvcc;
        let published = core.wal.progress.published_ts();
        let (reg, snapshot) = mvcc::begin(&mvcc.snapshots, published);
        // A serializable transaction also registers in the second registry
        // before its snapshot is read: the load below follows both
        // registrations' fences, so both registered values are at most the
        // snapshot.
        let (ser, snapshot) = if isolation == Isolation::Serializable {
            let (r, s) = mvcc::begin(&mvcc.serializable, published);
            (Some(r), s)
        } else {
            (None, snapshot)
        };
        Ok(Transaction {
            engine: self,
            id: mvcc.next_tx(),
            isolation,
            snapshot,
            regs: Some((reg, ser)),
            writes: BTreeMap::new(),
            drops: Vec::new(),
            reads: ReadSet::default(),
            charge: Some(
                core.accountant
                    .try_reserve(0)
                    .expect("zero bytes always fit"),
            ),
            trees: HashMap::new(),
            state: State::Active,
        })
    }
}

impl<'e> Transaction<'e> {
    /// The transaction's snapshot: the highest commit timestamp it reads.
    pub(crate) fn snapshot(&self) -> u64 {
        self.snapshot
    }

    fn active(&self) -> Result<()> {
        if self.state == State::Aborted {
            return Err(Error::Aborted);
        }
        self.engine.core().health.check()
    }

    /// Aborts the transaction because of `e` and returns `e`: its locks are
    /// released at once and its writes discarded.
    fn fail(&mut self, e: Error) -> Error {
        self.end();
        self.state = State::Aborted;
        e
    }

    /// Ends the transaction: releases its locks, write set, memory and
    /// snapshot.
    fn end(&mut self) {
        let core = self.engine.core();
        core.mvcc.locks.release_all(self.id);
        self.writes.clear();
        self.drops.clear();
        self.reads = ReadSet::default();
        self.charge = None;
        self.regs = None;
    }

    /// Reserves `bytes` for this transaction through the buffer pool,
    /// which evicts before it refuses (TS-14, TS-08); a refusal aborts it.
    fn reserve(&mut self, bytes: u64) -> Result<()> {
        match self.engine.core().pool.reserve(bytes) {
            Ok(r) => {
                self.charge
                    .as_mut()
                    .expect("an active transaction holds its charge")
                    .absorb(r);
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// Records a read of `iv` in `tree` (serializable only, TS-15).
    fn record(&mut self, tree: Tree, iv: Interval) -> Result<()> {
        if self.isolation != Isolation::Serializable {
            return Ok(());
        }
        let pool = &self.engine.core().pool;
        match self.reads.add(|b| pool.reserve(b), tree.id(), iv) {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// Checks that `tree` exists at the snapshot. For a write, first locks
    /// it S (TS-15: every writer of a tree), then applies the write-write
    /// rule to its drop. A serializable transaction records, on its first
    /// use of the tree, a read of the tree's directory entry, the key a
    /// drop writes (TS-15): a drop committed after its snapshot then fails
    /// its validation, as a write to a key it read.
    fn use_tree(&mut self, tree: Tree, write: bool) -> Result<()> {
        let known = self.trees.get(&tree.id()).copied();
        if known == Some(TreeUse::Write) || (known == Some(TreeUse::Read) && !write) {
            return Ok(());
        }
        if known.is_none() {
            self.record(DIRECTORY, Interval::point(&tree.id().to_be_bytes()))?;
        }
        if write {
            self.lock(Resource::Tree(tree.id()), Mode::S)?;
        }
        let status = match self.engine.store().tree_status(tree.id()) {
            Ok(s) => s,
            Err(e) => return Err(self.fail(e)),
        };
        match status {
            Some((t, dropped)) if t.root() == tree.root() => {
                if dropped != 0 && dropped <= self.snapshot {
                    return Err(Error::TreeDropped(tree.id()));
                }
                if dropped != 0 && write {
                    // Dropped after the snapshot: the write-write rule.
                    return Err(self.fail(Error::Serialization));
                }
            }
            _ => return Err(Error::TreeDropped(tree.id())),
        }
        self.trees.insert(
            tree.id(),
            if write { TreeUse::Write } else { TreeUse::Read },
        );
        Ok(())
    }

    /// Takes `res` in `mode`, reserving the lock entry (TS-14, TS-08).
    fn lock(&mut self, res: Resource, mode: Mode) -> Result<()> {
        let core = self.engine.core();
        match core.mvcc.locks.lock(self.id, &res, mode) {
            // An upgrade changes the mode of an entry already charged.
            Ok(Granted::Already | Granted::Upgraded) => Ok(()),
            Ok(Granted::Now | Granted::AfterWait) => self.reserve(res.charge()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// The committed version of `key` in `tree` that the snapshot sees.
    fn read_committed(&mut self, tree: Tree, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let store = self.engine.store();
        let mut v = Vec::new();
        let stamps = match store.get_stamped(tree, key, &mut v) {
            Ok(s) => s,
            Err(e) => return Err(self.fail(e)),
        };
        let Some(stamps) = stamps else {
            return Ok(None);
        };
        Ok(match visible(stamps, self.snapshot) {
            Some(true) => Some(v),
            Some(false) => None,
            None => match self
                .engine
                .core()
                .mvcc
                .versions
                .find(tree.id(), key, self.snapshot)
            {
                Older::Value(v) => Some(v),
                Older::Absent => None,
            },
        })
    }

    /// The value of `key` in `tree` at the snapshot, with this
    /// transaction's own writes applied.
    ///
    /// # Errors
    ///
    /// [`Error::Aborted`]; [`Error::TreeDropped`]; [`Error::MemoryLimit`]
    /// for the read set, which aborts; storage errors.
    pub(crate) fn get(&mut self, tree: Tree, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.active()?;
        self.use_tree(tree, false)?;
        self.record(tree, Interval::point(key))?;
        if let Some(p) = self.writes.get(&(tree.id(), key.to_vec())) {
            return Ok(p.value.clone());
        }
        self.read_committed(tree, key)
    }

    /// Calls `f(key, value)` for every entry of `tree` whose key starts with
    /// `prefix`, in key order, as the snapshot sees it with this
    /// transaction's writes applied, until `f` returns `false`.
    ///
    /// # Errors
    ///
    /// As [`Transaction::get`].
    pub(crate) fn scan_prefix(
        &mut self,
        tree: Tree,
        prefix: &[u8],
        mut f: impl FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<()> {
        self.active()?;
        self.use_tree(tree, false)?;
        self.record(tree, Interval::prefix(prefix))?;
        // Committed entries visible at the snapshot, copied first: the scan
        // calls back without page latches, and the old-version store is
        // read for entries newer than the snapshot.
        let mut committed: Vec<(Vec<u8>, Option<Vec<u8>>)> = Vec::new();
        let core = self.engine.core();
        let s = self.snapshot;
        let r = self
            .engine
            .store()
            .scan_prefix_stamped(tree, prefix, |k, v, st| {
                let value = match visible(st, s) {
                    Some(true) => Some(v.to_vec()),
                    Some(false) => None,
                    None => match core.mvcc.versions.find(tree.id(), k, s) {
                        Older::Value(v) => Some(v),
                        Older::Absent => None,
                    },
                };
                if let Some(v) = value {
                    committed.push((k.to_vec(), Some(v)));
                }
                true
            });
        if let Err(e) = r {
            return Err(self.fail(e));
        }
        // Merge the write set's entries of the prefix over them.
        let lo = (tree.id(), prefix.to_vec());
        let own: Vec<(&Vec<u8>, &Pending)> = self
            .writes
            .range(lo..)
            .take_while(|((t, k), _)| *t == tree.id() && k.starts_with(prefix))
            .map(|((_, k), p)| (k, p))
            .collect();
        let (mut i, mut j) = (0, 0);
        loop {
            let next = match (committed.get(i), own.get(j)) {
                (None, None) => return Ok(()),
                (Some((ck, cv)), None) => {
                    i += 1;
                    (ck.as_slice(), cv.as_deref())
                }
                (None, Some((ok, op))) => {
                    j += 1;
                    (ok.as_slice(), op.value.as_deref())
                }
                (Some((ck, cv)), Some((ok, op))) => {
                    if ck < *ok {
                        i += 1;
                        (ck.as_slice(), cv.as_deref())
                    } else {
                        if ck == *ok {
                            i += 1;
                        }
                        j += 1;
                        (ok.as_slice(), op.value.as_deref())
                    }
                }
            };
            if let (k, Some(v)) = next
                && !f(k, v)
            {
                return Ok(());
            }
        }
    }

    /// Prepares a write of `key` in `tree`: locks the tree S and the key X,
    /// applies the write-write rule and reserves the write's memory, the
    /// first time this transaction writes the key. Returns whether the key
    /// has a committed version at the snapshot.
    fn prepare_write(&mut self, tree: Tree, key: &[u8], value_len: usize) -> Result<bool> {
        self.active()?;
        if key.len() > MAX_KEY {
            return Err(Error::InvalidArgument("key longer than 1 024 bytes"));
        }
        self.use_tree(tree, true)?;
        let first = !self.writes.contains_key(&(tree.id(), key.to_vec()));
        if first {
            self.lock(
                Resource::Key {
                    tree: tree.id(),
                    key: key.into(),
                },
                Mode::X,
            )?;
            // The write-write rule (TS-15): with the lock held, no other
            // transaction can change the entry; one that changed it after
            // the snapshot has committed and published.
            let mut v = Vec::new();
            let stamps = match self.engine.store().get_stamped(tree, key, &mut v) {
                Ok(s) => s,
                Err(e) => return Err(self.fail(e)),
            };
            if let Some(st) = stamps
                && (st.created > self.snapshot || st.deleted > self.snapshot)
            {
                return Err(self.fail(Error::Serialization));
            }
            // The before-image this write will move into the old-version
            // store, and its write key (TS-14).
            let before = stamps.map_or(0, |_| version_bytes(key, v.len()));
            self.reserve(before + WriteKeys::charge_of(key))?;
        }
        self.reserve(key.len() as u64 + value_len as u64 + WRITE_OVERHEAD)?;
        // The committed version the snapshot sees: the value a delete
        // removes, and whether a put over a pending delete creates.
        Ok(self.read_committed(tree, key)?.is_some())
    }

    /// Puts `value` under `key` in `tree`, in the write set.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key over 1 024 bytes, which does
    /// not abort; [`Error::Serialization`], [`Error::Deadlock`],
    /// [`Error::MemoryLimit`], which abort; [`Error::TreeDropped`];
    /// [`Error::Aborted`]; storage errors.
    pub(crate) fn put(&mut self, tree: Tree, key: &[u8], value: &[u8]) -> Result<()> {
        self.prepare_write(tree, key, value.len())?;
        let old = self.writes.insert(
            (tree.id(), key.to_vec()),
            Pending {
                tree,
                value: Some(value.to_vec()),
            },
        );
        self.release_pending(old);
        Ok(())
    }

    /// Returns the bytes of a pending write that a later write of the same
    /// key replaced.
    fn release_pending(&mut self, old: Option<Pending>) {
        if let (Some(old), Some(c)) = (old, self.charge.as_mut()) {
            let len = old.value.map_or(0, |v| v.len());
            c.shrink(len as u64);
        }
    }

    /// Deletes `key` from `tree`, in the write set. Returns whether the key
    /// was present. Deleting a key absent at the snapshot and not written
    /// by this transaction changes nothing and takes no lock; it is a read
    /// of its absence.
    ///
    /// # Errors
    ///
    /// As [`Transaction::put`].
    pub(crate) fn delete(&mut self, tree: Tree, key: &[u8]) -> Result<bool> {
        self.active()?;
        self.use_tree(tree, false)?;
        self.record(tree, Interval::point(key))?;
        let own = self
            .writes
            .get(&(tree.id(), key.to_vec()))
            .map(|p| p.value.is_some());
        if own.is_none() && self.read_committed(tree, key)?.is_none() {
            return Ok(false);
        }
        let was = own.unwrap_or(true);
        let committed = self.prepare_write(tree, key, 0)?;
        let old = if committed {
            self.writes
                .insert((tree.id(), key.to_vec()), Pending { tree, value: None })
        } else {
            // Created by this transaction: the delete undoes the creation.
            self.writes.remove(&(tree.id(), key.to_vec()))
        };
        self.release_pending(old);
        Ok(was)
    }

    /// Drops `tree`: at commit, its directory entry records the drop, and
    /// garbage collection frees its pages once no snapshot below the drop
    /// remains (TS-14). Takes an X lock on the tree, which waits for its
    /// writers.
    ///
    /// # Errors
    ///
    /// As [`Transaction::put`].
    pub(crate) fn drop_tree(&mut self, tree: Tree) -> Result<()> {
        self.active()?;
        self.lock(Resource::Tree(tree.id()), Mode::X)?;
        self.use_tree(tree, true)?;
        let key = tree.id().to_be_bytes();
        self.reserve(WriteKeys::charge_of(&key) + WRITE_OVERHEAD + 16)?;
        if !self.drops.contains(&tree) {
            self.drops.push(tree);
        }
        Ok(())
    }

    /// Commits the transaction (TS-16) and ends it. Returns its commit
    /// timestamp, or `None` if it wrote nothing (read-only, FR-TX-010).
    ///
    /// # Errors
    ///
    /// [`Error::Serialization`] if validation fails (FR-TX-009);
    /// [`Error::Aborted`]; the errors of
    /// [`Engine::commit_transaction`]. The transaction is ended in every
    /// case; its writes are not visible unless it returned `Ok`.
    pub(crate) fn commit(mut self) -> Result<Option<u64>> {
        self.active()?;
        let engine = self.engine;
        let r = engine.commit_transaction(&mut self);
        // Step 10: the locks are released after publication.
        self.end();
        self.state = State::Aborted;
        engine.core().mvcc.notify_gc();
        r
    }

    /// Aborts the transaction: nothing it wrote becomes visible.
    pub(crate) fn abort(mut self) {
        self.end();
        self.state = State::Aborted;
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if self.state == State::Active {
            self.end();
        }
    }
}
