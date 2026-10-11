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
//! - **Trees it creates** (`Transaction::create_tree`) get their
//!   identifier at once, drawn as TS-07 draws element identifiers, and a
//!   root page only when the commit applies them (TS-16 step 6): a graph
//!   created in a transaction and the data written into it commit or roll
//!   back together (FR-CAT-004). PostgreSQL likewise assigns a new
//!   relation's file number from its OID counter, outside transaction
//!   control, when the relation is created (`GetNewRelFileNumber`, R-PG @
//!   `1f22453` `src/backend/catalog/catalog.c:584`, called at
//!   `src/backend/catalog/heap.c:1285`); it creates the file at once and
//!   deletes it if the transaction aborts (`RelationCreateStorage`,
//!   `src/backend/catalog/storage.c:122, 168`), while lpg creates the tree
//!   at commit, so an abort leaves nothing.

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

use crate::storage::btree::{DIRECTORY, Keys, MAX_KEY, NewTree, Tree};
use crate::storage::engine::Engine;
use crate::storage::error::{Error, Result};
use crate::storage::freelist::Detached;
use crate::storage::memory::Reservation;
use crate::sync::Arc;
use crate::sync::atomic::{AtomicU64, Ordering};

/// The isolation level of a transaction (FR-TX-003, FR-TX-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
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
pub(crate) struct Transaction {
    /// A handle of the database: the transaction owns what it uses, so
    /// it can move into `spawn_blocking` (TS-02 "Owned transactions").
    engine: Engine,
    id: TxId,
    pub(crate) isolation: Isolation,
    pub(crate) snapshot: u64,
    /// The snapshot registrations in `mvcc.snapshots` and, if
    /// serializable, `mvcc.serializable`; [`Transaction::end`] withdraws
    /// them.
    regs: Option<(Detached, Option<Detached>)>,
    /// The private write set, by `(tree, key)`.
    pub(crate) writes: BTreeMap<(u64, Vec<u8>), Pending>,
    /// Trees dropped by this transaction.
    pub(crate) drops: Vec<Tree>,
    /// Trees created by this transaction, created when it commits.
    pub(crate) created: Vec<NewTree>,
    /// The graphs this transaction wrote, for the statistics its commit
    /// applies (TS-17 Statistics).
    pub(crate) graphs: Vec<crate::storage::graph::Graph>,
    /// The graphs this transaction dropped, by catalog object identifier.
    pub(crate) dropped_graphs: Vec<u64>,
    /// The read set (serializable transactions).
    pub(crate) reads: ReadSet,
    /// The ONLINE switch of an index build this transaction commits
    /// (TS-18 step 6, [`crate::index`]).
    pub(crate) online: Option<crate::index::Online>,
    /// Statistics this transaction's commit adds: the counts of an index
    /// it makes ONLINE or creates built (TS-17).
    pub(crate) schema_stats: crate::catalog::stats::Delta,
    /// The memory of the write set, before-images, write keys and lock
    /// entries (TS-14).
    pub(crate) charge: Option<Reservation>,
    trees: HashMap<u64, TreeUse>,
    state: State,
    /// B+tree entries this transaction's reads received (PROFILE's
    /// independent check, rmp #45).
    pub(crate) entries_read: u64,
    /// The transaction's memory limit in bytes (FR-API-007, rmp #49), `None`
    /// for none: what it holds from the accountant, its write set charge
    /// and its statements' operator reservations together.
    pub(crate) memory_limit: Option<u64>,
    /// Bytes its statements' blocking operators hold (`query::exec::Held`),
    /// counted against [`Transaction::memory_limit`].
    pub(crate) operator_bytes: Arc<AtomicU64>,
}

impl std::fmt::Debug for Transaction {
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
    pub(crate) fn begin(&self, isolation: Isolation) -> Result<Transaction> {
        let core = self.core();
        core.health.check()?;
        let mvcc = &core.mvcc;
        let published = core.wal.progress.published_ts();
        let (reg, snapshot) = mvcc::begin_detached(&mvcc.snapshots, published);
        // A serializable transaction also registers in the second registry
        // before its snapshot is read: the load below follows both
        // registrations' fences, so both registered values are at most the
        // snapshot.
        let (ser, snapshot) = if isolation == Isolation::Serializable {
            let (r, s) = mvcc::begin_detached(&mvcc.serializable, published);
            (Some(r), s)
        } else {
            (None, snapshot)
        };
        Ok(Transaction {
            engine: Engine::handle(self.core_arc()),
            id: mvcc.next_tx(),
            isolation,
            snapshot,
            regs: Some((reg, ser)),
            writes: BTreeMap::new(),
            drops: Vec::new(),
            created: Vec::new(),
            graphs: Vec::new(),
            dropped_graphs: Vec::new(),
            reads: ReadSet::default(),
            online: None,
            schema_stats: crate::catalog::stats::Delta::default(),
            charge: Some(
                core.accountant
                    .try_reserve(0)
                    .expect("zero bytes always fit"),
            ),
            trees: HashMap::new(),
            state: State::Active,
            entries_read: 0,
            memory_limit: None,
            operator_bytes: Arc::new(AtomicU64::new(0)),
        })
    }
}

impl Transaction {
    /// The transaction's snapshot: the highest commit timestamp it reads.
    pub(crate) fn snapshot(&self) -> u64 {
        self.snapshot
    }

    /// The database the transaction runs on.
    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Aborts the transaction because of `e` and returns `e`, for the
    /// layers built on this API (catalog, graph) when a rule of TS-15 fails.
    pub(crate) fn abort_with(&mut self, e: Error) -> Error {
        self.fail(e)
    }

    /// Fails with [`Error::Aborted`] once the transaction is aborted, and
    /// with [`Error::Unusable`] on an unusable database.
    ///
    /// # Errors
    ///
    /// As above.
    pub(crate) fn check_active(&self) -> Result<()> {
        self.active()
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
        self.created.clear();
        self.graphs.clear();
        self.dropped_graphs.clear();
        self.reads = ReadSet::default();
        self.online = None;
        self.schema_stats = crate::catalog::stats::Delta::default();
        self.charge = None;
        if let Some((all, ser)) = self.regs.take() {
            core.mvcc.snapshots.leave(all);
            if let Some(s) = ser {
                core.mvcc.serializable.leave(s);
            }
        }
    }

    /// The bytes the transaction holds: its charge and its operators'
    /// reservations (FR-API-007 memory limit).
    pub(crate) fn memory_held(&self) -> u64 {
        self.charge.as_ref().map_or(0, Reservation::bytes)
            + self.operator_bytes.load(Ordering::Relaxed)
    }

    /// Fails with [`Error::TransactionMemoryLimit`] if `bytes` more would
    /// make the transaction hold more than its memory limit. The caller
    /// fails the statement, which aborts the transaction.
    ///
    /// # Errors
    ///
    /// As above.
    pub(crate) fn check_memory(&self, bytes: u64) -> Result<()> {
        match self.memory_limit {
            Some(limit) if self.memory_held().saturating_add(bytes) > limit => {
                Err(Error::TransactionMemoryLimit { limit })
            }
            _ => Ok(()),
        }
    }

    /// Reserves `bytes` for this transaction through the buffer pool,
    /// which evicts before it refuses (TS-14, TS-08); a refusal, or a
    /// reservation beyond the transaction's memory limit, aborts it.
    fn reserve(&mut self, bytes: u64) -> Result<()> {
        if let Err(e) = self.check_memory(bytes) {
            return Err(self.fail(e));
        }
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
        if tree.is_pending() {
            // Created by this transaction, so visible to no other one.
            return if self.created.iter().any(|t| t.id == tree.id()) {
                Ok(())
            } else {
                Err(Error::TreeDropped(tree.id()))
            };
        }
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
        if status.is_some() && tree.id() != DIRECTORY.id() {
            // The directory entry read (PROFILE's independent count).
            self.entries_read += 1;
        }
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

    /// Creates a tree, whose leaves store keys as `keys`, when the
    /// transaction commits: the returned handle is [`Tree::pending`] until
    /// then, and only this transaction can read or write it (module
    /// documentation).
    ///
    /// # Errors
    ///
    /// [`Error::Aborted`]; [`Error::Unusable`].
    pub(crate) fn create_tree(&mut self, keys: Keys) -> Result<Tree> {
        self.active()?;
        let id = self.engine.store().draw_tree_id();
        self.created.push(NewTree { id, keys });
        Ok(Tree::pending(id))
    }

    /// The handle of tree `id` as this transaction sees it: a tree it
    /// creates, else the directory's entry.
    ///
    /// # Errors
    ///
    /// [`Error::TreeDropped`] if the directory does not list it; storage
    /// errors.
    pub(crate) fn tree(&mut self, id: u64) -> Result<Tree> {
        if self.created.iter().any(|t| t.id == id) {
            return Ok(Tree::pending(id));
        }
        match self.engine.store().tree(id) {
            Ok(Some(t)) => {
                if id != DIRECTORY.id() {
                    self.entries_read += 1;
                }
                Ok(t)
            }
            Ok(None) => Err(Error::TreeDropped(id)),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// Takes `res` in `mode`, reserving the lock entry (TS-14, TS-08), for
    /// the layers built on this API.
    ///
    /// # Errors
    ///
    /// [`Error::Deadlock`] and [`Error::MemoryLimit`], which abort;
    /// [`Error::Aborted`].
    pub(crate) fn lock_resource(&mut self, res: Resource, mode: Mode) -> Result<()> {
        self.active()?;
        self.lock(res, mode)
    }

    /// Locks `key` of `tree` in `mode` and applies the write-write rule to
    /// its entry (TS-15): an entry created or delete-stamped after the
    /// snapshot fails the transaction with [`Error::Serialization`]. The
    /// catalog takes X on a name it creates or drops and S on the name of
    /// the parent it creates in ([`crate::catalog`]).
    ///
    /// # Errors
    ///
    /// As [`Transaction::put`].
    pub(crate) fn lock_key(&mut self, tree: Tree, key: &[u8], mode: Mode) -> Result<()> {
        self.active()?;
        self.use_tree(tree, mode == Mode::X)?;
        if tree.is_pending() {
            return Ok(());
        }
        self.lock(
            Resource::Key {
                tree: tree.id(),
                key: key.into(),
            },
            mode,
        )?;
        self.unchanged(tree, key).map(drop)
    }

    /// The write-write rule for `key` of `tree`, whose lock is held: fails
    /// the transaction with [`Error::Serialization`] if the committed entry
    /// was created or delete-stamped after the snapshot. Returns the
    /// committed entry's value length, if any.
    fn unchanged(&mut self, tree: Tree, key: &[u8]) -> Result<Option<usize>> {
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
        Ok(stamps.map(|_| v.len()))
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

    /// The committed version of `key` in `tree` that the snapshot sees,
    /// without this transaction's writes and without recording a read.
    pub(crate) fn read_committed(&mut self, tree: Tree, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if tree.is_pending() {
            return Ok(None);
        }
        let store = self.engine.store();
        let mut v = Vec::new();
        let stamps = match store.get_stamped(tree, key, &mut v) {
            Ok(s) => s,
            Err(e) => return Err(self.fail(e)),
        };
        let Some(stamps) = stamps else {
            return Ok(None);
        };
        self.entries_read += 1;
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

    /// [`Transaction::get`] that records no read (TS-15 Read sets), for a
    /// read whose result a lock the transaction holds keeps valid until it
    /// ends: an edge's endpoint, under its existence lock, which a label or
    /// property change of the endpoint must not fail (TS-15 Write-write
    /// rule).
    ///
    /// # Errors
    ///
    /// As [`Transaction::get`].
    pub(crate) fn get_locked(&mut self, tree: Tree, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.active()?;
        self.use_tree(tree, false)?;
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
        f: impl FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<()> {
        let iv = Interval::prefix(prefix);
        self.scan_range(tree, &iv.lo, iv.hi.as_deref(), f)
    }

    /// [`Transaction::scan_prefix`] over the keys `k` with `lo <= k` and,
    /// if `hi` is given, `k < hi`: the read of the interval `[lo, hi)`
    /// (TS-15 Read sets), an index range among them (TS-18).
    ///
    /// # Errors
    ///
    /// As [`Transaction::get`].
    pub(crate) fn scan_range(
        &mut self,
        tree: Tree,
        lo: &[u8],
        hi: Option<&[u8]>,
        f: impl FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<()> {
        self.scan_chunk(tree, lo, hi, usize::MAX, f).map(drop)
    }

    /// [`Transaction::scan_range`] that stops after `limit` committed
    /// entries: the read of `[lo, k]`, `k` the last committed key it
    /// returned, with this transaction's writes in that interval merged.
    /// Returns the key to resume from (the smallest key above `k`), or
    /// `None` when the range is done. A query scan reads in chunks this
    /// way, so it holds and reads only what one batch needs (rmp #45,
    /// FR-API-010).
    ///
    /// # Errors
    ///
    /// As [`Transaction::get`].
    pub(crate) fn scan_chunk(
        &mut self,
        tree: Tree,
        lo: &[u8],
        hi: Option<&[u8]>,
        limit: usize,
        mut f: impl FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<Option<Vec<u8>>> {
        self.active()?;
        self.use_tree(tree, false)?;
        // Committed entries visible at the snapshot, copied first: the scan
        // calls back without page latches, and the old-version store is
        // read for entries newer than the snapshot.
        let mut committed: Vec<(Vec<u8>, Option<Vec<u8>>)> = Vec::new();
        let core = self.engine.core();
        let s = self.snapshot;
        let mut delivered = 0u64;
        let r = if tree.is_pending() {
            Ok(())
        } else {
            self.engine
                .store()
                .scan_range_stamped(tree, lo, hi, |k, v, st| {
                    delivered += 1;
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
                    committed.len() < limit
                })
        };
        self.entries_read += delivered;
        if let Err(e) = r {
            return Err(self.fail(e));
        }
        // The interval read: up to the last committed key returned when the
        // chunk is full, else the whole range.
        let resume = if committed.len() >= limit {
            committed.last().map(|(k, _)| {
                let mut r = k.clone();
                r.push(0);
                r
            })
        } else {
            None
        };
        let read_hi: Option<Vec<u8>> = match &resume {
            Some(r) => Some(r.clone()),
            None => hi.map(<[u8]>::to_vec),
        };
        self.record(
            tree,
            Interval {
                lo: lo.to_vec(),
                hi: read_hi.clone(),
            },
        )?;
        // Merge the write set's entries of the interval over them.
        let own: Vec<(&Vec<u8>, &Pending)> = self
            .writes
            .range((tree.id(), lo.to_vec())..)
            .take_while(|((t, k), _)| {
                *t == tree.id() && read_hi.as_deref().is_none_or(|h| k.as_slice() < h)
            })
            .map(|((_, k), p)| (k, p))
            .collect();
        let (mut i, mut j) = (0, 0);
        loop {
            let next = match (committed.get(i), own.get(j)) {
                (None, None) => return Ok(resume),
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
                return Ok(resume);
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
        if first && tree.is_pending() {
            // A tree this transaction creates: no other transaction can
            // see it, and its entries have no before-image.
            self.reserve(WriteKeys::charge_of(key))?;
        } else if first {
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
            let committed = self.unchanged(tree, key)?;
            // The before-image this write will move into the old-version
            // store, and its write key (TS-14).
            let before = committed.map_or(0, |len| version_bytes(key, len));
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
        if tree.is_pending() {
            // Created by this transaction: it is never created.
            if !self.created.iter().any(|t| t.id == tree.id()) {
                return Err(Error::TreeDropped(tree.id()));
            }
            self.created.retain(|t| t.id != tree.id());
            let keys: Vec<(u64, Vec<u8>)> = self
                .writes
                .range((tree.id(), Vec::new())..)
                .take_while(|((t, _), _)| *t == tree.id())
                .map(|(k, _)| k.clone())
                .collect();
            for k in keys {
                let old = self.writes.remove(&k);
                self.release_pending(old);
            }
            return Ok(());
        }
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
        let engine = Engine::handle(self.engine.core_arc());
        let r = engine.commit_transaction(&mut self);
        // Step 10: the locks are released after publication.
        self.end();
        self.state = State::Aborted;
        engine.core().mvcc.notify_gc();
        r
    }

    /// Commits the transaction, as [`Transaction::commit`], with the tree
    /// that `built` holds installed by the same commit: a constraint and its
    /// owned index commit together (TS-18 Constraints). Then, or on failure,
    /// the build's pages that the tree does not use are released
    /// ([`crate::storage::btree::bulk`]).
    ///
    /// # Errors
    ///
    /// As [`Transaction::commit`].
    pub(crate) fn commit_installing(
        mut self,
        built: crate::storage::btree::bulk::Built<'_>,
    ) -> Result<Option<u64>> {
        self.active()?;
        let engine = Engine::handle(self.engine.core_arc());
        let r = engine.commit_transaction_installing(&mut self, Some(&built));
        drop(built);
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

impl Drop for Transaction {
    fn drop(&mut self) {
        if self.state == State::Active {
            self.end();
        }
    }
}
