//! The commit protocol (TS-16), Sprint 1 steps: 2, 3, 6, 7, 8 and 9.
//! Without the transaction layer, a commit's input is the whole batch
//! (TS-16 Batch); steps 1, 4, 5, 10 and 11 arrive with MVCC and locks.
//!
//! 2. **Reserve file space** outside the latch: the reserve of unused pages
//!    is made to cover the TS-05 estimate. ENOSPC fails the commit before
//!    it takes the latch; reads continue (FR-REC-007).
//! 3. **Commit latch**, and the next commit timestamp. A commit that fails
//!    under the latch never stores its timestamp, so the next commit draws
//!    the same value.
//! 6. **Apply** the batch ([`crate::storage::btree::Store::apply_batch`]),
//!    undone on any failure.
//! 7. **Join** the open WAL group, then release the latch.
//! 8. **Wait** until the group is durable (FR-TX-002). A failed write or
//!    sync of the group fails the commit and makes the database unusable
//!    (FR-REC-006).
//! 9. **Publish**: the WAL writer publishes each group's timestamps once the
//!    group is durable, and groups become durable in order, so timestamps
//!    are published in order.

use std::time::Instant;

use crate::obs::names::transactions::{COMMIT_LATENCY, COMMITTED, FAILED_COMMITS};
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::Engine;
use crate::storage::error::Result;
use crate::storage::wal::Counters;

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
        let core = self.core();
        let r = self.commit_steps(batch, new_trees, ids);
        match &r {
            Ok(_) => {
                core.sink.counter(COMMITTED, 1);
                core.sink
                    .histogram(COMMIT_LATENCY, started.elapsed().as_nanos() as u64);
                core.checkpoints.note_commit();
            }
            Err(_) => core.sink.counter(FAILED_COMMITS, 1),
        }
        r
    }

    fn commit_steps(&self, batch: &[Write<'_>], new_trees: usize, ids: Ids) -> Result<Committed> {
        let core = self.core();
        core.health.check()?;
        let store = core.store();
        // Step 2.
        #[cfg(test)]
        let reserve = !core.skip_reserve.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(test))]
        let reserve = true;
        if reserve {
            core.data.ensure(store.estimate(batch, new_trees)?)?;
        }
        // Step 3.
        let mut st = store.lock();
        let commit_ts = st.log_ref().counters.commit_ts + 1;
        // Step 6.
        let trees = store.apply_batch(&mut st, Some(&core.wal), batch, new_trees)?;
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
        drop(st);
        // Step 8.
        core.wal.wait_durable(seq)?;
        // Step 9.
        debug_assert!(core.wal.published() >= commit_ts);
        Ok(Committed { commit_ts, trees })
    }
}

#[cfg(test)]
pub(crate) mod tests;
