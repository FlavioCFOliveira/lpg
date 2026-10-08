//! The commit protocol (TS-16).
//!
//! A transaction's commit ([`Engine::commit_transaction`]) runs every step:
//!
//! 1. **Read-only** (it wrote nothing): end the snapshot and return;
//!    nothing is validated, logged or published (FR-TX-010).
//! 2. **Reserve file space** outside the latch: the reserve of unused pages
//!    is made to cover the TS-05 estimate. ENOSPC fails the transaction
//!    before it takes the latch; reads continue (FR-REC-007).
//! 3. **Commit latch**, and the next commit timestamp. A commit that fails
//!    under the latch never stores its timestamp, so the next commit draws
//!    the same value and the published timestamps have no gap.
//! 4. **Serializable only:** validate the read set against the write keys
//!    of every commit after the snapshot (TS-15); an overlap fails with
//!    [`Error::Serialization`] (GQLSTATUS 25N11).
//! 5. **Register the write-key set** (TS-15 Write keys).
//! 6. **Apply** the batch built from the write set, every entry stamped
//!    with `commit_ts` (TS-06 Stamps). Before any page changes, the
//!    before-image of each replaced entry moves into the old-version store,
//!    into memory reserved when the write entered the write set (TS-14), so
//!    a reader that finds the page changed finds the version. A failure is
//!    undone ([`crate::storage::btree::Store::apply_batch`]), and the undo
//!    also removes the versions and withdraws the write keys.
//! 7. **Join** the open WAL group, then release the latch.
//! 8. **Wait** until the group is durable (FR-TX-002).
//! 9. **Publish**: the WAL writer publishes each group's timestamps once the
//!    group is durable, in group order ([`crate::storage::wal::Progress`]).
//! 10. **Release the locks** (TS-15), then wake garbage collection.
//!
//! Snapshot-isolation transactions run every step except 4.
//!
//! Without the transaction layer, a commit's input is the whole batch, and
//! it runs steps 2, 3 and 6-9 only, with no before-images (TS-16 Batch):
//! [`Engine::commit`], used by the storage tests and the bulk build.

use std::time::Instant;

use super::conflict::WriteKeys;
use super::versions::version_bytes;
use super::{Isolation, Transaction};
use crate::obs::names::transactions::{COMMIT_LATENCY, COMMITTED, FAILED_COMMITS};
use crate::storage::btree::{
    CommitLatch, DIRECTORY, Store, Tree, Write, WriteState, dropped_directory_entry,
};
use crate::storage::engine::Engine;
use crate::storage::error::{Error, Result};
use crate::storage::memory::Reservation;
use crate::storage::wal::{Counters, Wal};

/// High-water marks of the identifier counters a commit raises (TS-05):
/// each is the next value to hand out after the commit. A counter is never
/// lowered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ids {
    /// Next element identifier.
    pub(crate) next_element_id: u64,
    /// Next catalog object identifier.
    pub(crate) next_catalog_id: u64,
    /// Next token identifier.
    pub(crate) next_token_id: u64,
}

/// The outcome of a successful commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Committed {
    /// The commit timestamp.
    pub(crate) commit_ts: u64,
    /// Trees the commit created, in order.
    pub(crate) trees: Vec<Tree>,
}

impl Engine {
    /// Commits `batch` as one unit (TS-16 steps 2, 3, 6–9) and raises the
    /// identifier counters to `ids`. Returns once the commit's WAL group is
    /// on stable storage.
    ///
    /// # Errors
    ///
    /// [`crate::storage::error::Error::InvalidArgument`] for a key over
    /// 1 024 bytes; ENOSPC or
    /// [`crate::storage::error::Error::MemoryLimit`], with nothing applied
    /// and the database usable; any other error, with the database
    /// unusable: a failed apply (undone), or a failed write or sync of the
    /// commit's group.
    pub(crate) fn commit(&self, batch: &[Write<'_>], ids: Ids) -> Result<Committed> {
        self.commit_with(batch, 0, ids)
    }

    /// Commits the creation of `n` empty trees, as [`Engine::commit`].
    ///
    /// # Errors
    ///
    /// As [`Engine::commit`].
    pub(crate) fn create_trees(&self, n: usize) -> Result<Vec<Tree>> {
        Ok(self.commit_with(&[], n, Ids::default())?.trees)
    }

    /// Commits a batch that first creates `new_trees` trees, then applies
    /// `batch`.
    ///
    /// # Errors
    ///
    /// As [`Engine::commit`].
    pub(crate) fn commit_with(
        &self,
        batch: &[Write<'_>],
        new_trees: usize,
        ids: Ids,
    ) -> Result<Committed> {
        let started = Instant::now();
        let r = self.commit_steps(batch, new_trees, ids);
        self.note_commit_result(started, &r);
        r
    }

    fn commit_steps(&self, batch: &[Write<'_>], new_trees: usize, ids: Ids) -> Result<Committed> {
        let core = self.core();
        core.health.check()?;
        let store = core.store();
        // Step 2.
        self.reserve_space(&store, batch, new_trees)?;
        // Step 3.
        let st = store.lock();
        self.commit_latched(st, ids, |store, st, wal, _| {
            store.apply_batch(st, Some(wal), batch, new_trees)
        })
    }

    /// TS-16 step 2: makes the reserve of unused pages cover the TS-05
    /// estimate of `batch` (skipped by tests that need an overrun).
    fn reserve_space(
        &self,
        store: &Store<'_>,
        batch: &[Write<'_>],
        new_trees: usize,
    ) -> Result<()> {
        let core = self.core();
        #[cfg(test)]
        let reserve = !core
            .skip_reserve
            .load(crate::sync::atomic::Ordering::Relaxed);
        #[cfg(not(test))]
        let reserve = true;
        if reserve {
            core.data.ensure(store.estimate(batch, new_trees)?)?;
        }
        Ok(())
    }

    /// TS-16 steps 3 and 6-9 with the commit latch `st` already held:
    /// draws `commit_ts`, runs `apply` with it (steps 4 to 6, which must
    /// undo themselves on failure), joins the open group, releases the
    /// latch and waits until the group is durable. The bulk build calls it
    /// after building its pages under the latch (`storage::btree::bulk`).
    ///
    /// # Errors
    ///
    /// Errors of `apply`; the WAL writer's failure (step 8).
    pub(crate) fn commit_latched(
        &self,
        mut st: CommitLatch<'_>,
        ids: Ids,
        apply: impl FnOnce(&Store<'_>, &mut WriteState, &Wal, u64) -> Result<Vec<Tree>>,
    ) -> Result<Committed> {
        let core = self.core();
        let store = core.store();
        // Step 3.
        let commit_ts = st.log_ref().counters.commit_ts + 1;
        // Steps 4 to 6.
        let trees = apply(&store, &mut st, &core.wal, commit_ts)?;
        #[cfg(test)]
        crate::storage::wal::TestPoints::run(&core.wal.points.after_apply);
        // Step 7.
        let log = st.log();
        log.counters = Counters {
            commit_ts,
            next_element_id: log.counters.next_element_id.max(ids.next_element_id),
            next_catalog_id: log.counters.next_catalog_id.max(ids.next_catalog_id),
            next_token_id: log.counters.next_token_id.max(ids.next_token_id),
        };
        let counters = log.counters;
        log.group.join(commit_ts, counters);
        let seq = log.group.seq();
        // Counted before the latch is released, so the writer's gather
        // window sees this commit in the group it closes.
        core.wal.note_join(seq);
        drop(st);
        // Step 8.
        core.wal.wait_durable(seq)?;
        // Step 9.
        debug_assert!(core.wal.published() >= commit_ts);
        Ok(Committed { commit_ts, trees })
    }

    /// Records the metrics of a finished commit attempt that began at
    /// `started`.
    pub(crate) fn note_commit_result<T>(&self, started: Instant, r: &Result<T>) {
        let core = self.core();
        match r {
            Ok(_) => {
                core.sink.counter(COMMITTED, 1);
                core.sink
                    .histogram(COMMIT_LATENCY, started.elapsed().as_nanos() as u64);
                core.checkpoints.note_commit();
            }
            Err(_) => core.sink.counter(FAILED_COMMITS, 1),
        }
    }

    /// Commits transaction `tx` (TS-16 steps 1 to 9; step 10, the release
    /// of its locks, is the caller's, after this returns). Returns its
    /// commit timestamp, or `None` for a read-only transaction.
    ///
    /// # Errors
    ///
    /// [`Error::Serialization`] (step 4); ENOSPC or [`Error::MemoryLimit`],
    /// with nothing applied and the database usable; any other error with
    /// the database unusable, as [`Engine::commit`].
    pub(crate) fn commit_transaction(&self, tx: &mut Transaction<'_>) -> Result<Option<u64>> {
        // Step 1.
        if tx.writes.is_empty() && tx.drops.is_empty() {
            return Ok(None);
        }
        let started = Instant::now();
        let mut charge = tx
            .charge
            .take()
            .expect("an active transaction holds its charge");
        let r = self.commit_transaction_steps(tx, &mut charge);
        tx.charge = Some(charge);
        self.note_commit_result(started, &r);
        r.map(Some)
    }

    fn commit_transaction_steps(
        &self,
        tx: &Transaction<'_>,
        charge: &mut Reservation,
    ) -> Result<u64> {
        let core = self.core();
        core.health.check()?;
        let store = core.store();
        let dir_keys: Vec<[u8; 8]> = tx.drops.iter().map(|t| t.id().to_be_bytes()).collect();
        // Step 2, with placeholder stamps and drop entries: the estimate
        // counts key and value bytes only, and is not a bound (TS-05).
        let placeholder = vec![[0u8; 16]; tx.drops.len()];
        self.reserve_space(&store, &batch_of(tx, 1, &dir_keys, &placeholder), 0)?;
        // Step 3.
        let st = store.lock();
        let mvcc = &core.mvcc;
        let committed = self.commit_latched(st, Ids::default(), |store, st, wal, ts| {
            // Step 4.
            if tx.isolation == Isolation::Serializable {
                mvcc.committed.validate(tx.snapshot, &tx.reads)?;
            }
            // Step 5.
            let mut keys: Vec<(u64, Box<[u8]>)> = tx
                .writes
                .keys()
                .map(|(t, k)| (*t, k.as_slice().into()))
                .collect();
            keys.extend(dir_keys.iter().map(|k| (DIRECTORY.id(), k[..].into())));
            let key_bytes: u64 = keys.iter().map(|(_, k)| WriteKeys::charge_of(k)).sum();
            mvcc.committed
                .register(ts, WriteKeys::new(keys), charge.split(key_bytes));
            // Step 6.
            let r = self.apply_transaction(store, st, wal, tx, charge, ts, &dir_keys);
            if r.is_err() {
                mvcc.committed.withdraw(ts);
            }
            r.map(|()| Vec::new())
        })?;
        Ok(committed.commit_ts)
    }

    /// TS-16 step 6 of transaction `tx` at `ts`, under the latch: moves the
    /// before-images into the old-version store, with bytes from `charge`
    /// reserved when the writes entered the write set; applies the stamped
    /// batch; queues the delete stamps and dropped trees for garbage
    /// collection. Undone on failure.
    #[expect(
        clippy::too_many_arguments,
        reason = "the latched state and the transaction's batch"
    )]
    fn apply_transaction(
        &self,
        store: &Store<'_>,
        st: &mut WriteState,
        wal: &Wal,
        tx: &Transaction<'_>,
        charge: &mut Reservation,
        ts: u64,
        dir_keys: &[[u8; 8]],
    ) -> Result<()> {
        let core = self.core();
        let mvcc = &core.mvcc;
        // The before-images, before any page changes.
        let mut pushed: Vec<(u64, &[u8])> = Vec::new();
        let mut value = Vec::new();
        let mut r = Ok(());
        for ((tree, key), p) in &tx.writes {
            if p.value.is_none() {
                // A delete is a stamp: the page keeps the value, which older
                // snapshots read there.
                continue;
            }
            let stamps = match store.get_stamped(p.tree, key, &mut value) {
                Ok(Some(stamps)) => stamps,
                // A creation: its before-image is "absent".
                Ok(None) => continue,
                Err(e) => {
                    r = Err(e);
                    break;
                }
            };
            let bytes = version_bytes(key, value.len());
            let mut c = charge.split(bytes);
            if c.bytes() < bytes {
                // Reserved at write time for the entry the snapshot saw; the
                // locks keep it unchanged, so this is not expected, but a
                // shortfall is reserved, never ignored.
                let more = bytes - c.bytes();
                match core.accountant.try_reserve(more) {
                    Some(extra) => c.absorb(extra),
                    None => {
                        r = Err(Error::MemoryLimit { requested: more });
                        break;
                    }
                }
            }
            mvcc.versions
                .push(*tree, key, std::mem::take(&mut value), stamps, ts, c);
            pushed.push((*tree, key));
        }
        let dir_values: Vec<[u8; 16]> = tx
            .drops
            .iter()
            .map(|t| dropped_directory_entry(t.root(), ts))
            .collect();
        let r = r.and_then(|()| {
            // Leaves this batch rewrites drop the stamps every snapshot
            // sees (TS-14).
            let horizon = super::mvcc::horizon(&mvcc.snapshots, core.wal.progress.published_ts());
            st.set_freeze_horizon(horizon);
            let batch = batch_of(tx, ts, dir_keys, &dir_values);
            let r = store.apply_batch(st, Some(wal), &batch, 0).map(drop);
            st.set_freeze_horizon(0);
            r
        });
        match r {
            Ok(()) => {
                let mut q = mvcc.gc();
                for ((_, key), p) in &tx.writes {
                    if p.value.is_none() {
                        q.deleted.push_back((ts, p.tree, key.as_slice().into()));
                    }
                }
                for t in &tx.drops {
                    q.dropped.push_back((ts, *t));
                }
                Ok(())
            }
            Err(e) => {
                for (tree, key) in pushed.iter().rev() {
                    mvcc.versions.undo(*tree, key, ts);
                }
                Err(e)
            }
        }
    }
}

/// The batch of transaction `tx` committing at `ts` (TS-16 Batch): every
/// pending write stamped with `ts`, then the directory entries `dir_values`
/// under `dir_keys` of the trees it drops.
fn batch_of<'a>(
    tx: &'a Transaction<'_>,
    ts: u64,
    dir_keys: &'a [[u8; 8]],
    dir_values: &'a [[u8; 16]],
) -> Vec<Write<'a>> {
    let mut batch: Vec<Write<'a>> = tx
        .writes
        .iter()
        .map(|((_, key), p)| Write {
            tree: p.tree,
            key,
            value: p.value.as_deref(),
            stamp: ts,
        })
        .collect();
    for (k, v) in dir_keys.iter().zip(dir_values) {
        batch.push(Write {
            tree: DIRECTORY,
            key: k,
            value: Some(v),
            stamp: 0,
        });
    }
    batch
}

#[cfg(test)]
pub(crate) mod tests;
