//! The MVCC state shared by the transactions of one database (TS-14): the
//! snapshot registries, the old-version store, the committed write-key
//! sets, the lock table and the work of garbage collection.
//!
//! **Snapshot registry** (TS-14). A slot registry with the protocol of the
//! page-reuse registry ([`Readers`]). Every atomic operation, and the store
//! that publishes a `commit_ts` ([`crate::storage::wal::Progress`]), is
//! `SeqCst`:
//! - **Begin** ([`begin`]): load the highest published `commit_ts` p, store
//!   p in a free slot with a compare-and-swap, issue a fence, then load the
//!   highest published `commit_ts` again. That second value is the
//!   snapshot; it is at least p. The slot is freed when the transaction
//!   ends.
//! - **Horizon** ([`horizon`]): load the highest published `commit_ts` P,
//!   issue a fence, then read every slot. The horizon is the minimum of P
//!   and the registered values.
//! - If the horizon misses a slot, its fence precedes the transaction's
//!   fence, so the transaction's second load returns at least P: no
//!   snapshot is ever below a horizon computed while it was registering
//!   (store buffering with fences on both sides). A `loom` model checks it.
//!
//! Serializable transactions also register in a second registry, whose
//! horizon bounds the committed write-key sets (TS-14 Garbage collection:
//! "older than the oldest snapshot of any active serializable
//! transaction").

use std::collections::VecDeque;

use super::conflict::Committed;
use super::lock::LockTable;
use super::versions::Versions;
use crate::storage::btree::Tree;
use crate::storage::freelist::{Readers, Registered};
use crate::sync::atomic::{AtomicU64, Ordering};
use crate::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// Registers a snapshot in `registry` against the published commit
/// timestamp `published` (TS-14 Begin). Returns the registration, which
/// ends the snapshot when dropped, and the snapshot.
pub(crate) fn begin<'a>(registry: &'a Readers, published: &AtomicU64) -> (Registered<'a>, u64) {
    // `Readers::enter` loads `published`, stores it with a SeqCst
    // compare-and-swap and issues a SeqCst fence.
    let reg = registry.enter(published);
    let snapshot = published.load(Ordering::SeqCst);
    (reg, snapshot)
}

/// The horizon of `registry` (TS-14 Horizon): no running or starting
/// snapshot is below it.
pub(crate) fn horizon(registry: &Readers, published: &AtomicU64) -> u64 {
    let p = published.load(Ordering::SeqCst);
    // `Readers::horizon` issues a SeqCst fence, then reads every slot.
    p.min(registry.horizon())
}

/// Work queued for garbage collection, and the thread's wake-up.
#[derive(Debug, Default)]
pub(crate) struct GcQueue {
    /// Delete-stamped entries: `(deleted_ts, tree, key)`, in commit order.
    pub(crate) deleted: VecDeque<(u64, Tree, Box<[u8]>)>,
    /// Dropped trees: `(drop_ts, tree)`, in commit order.
    pub(crate) dropped: VecDeque<(u64, Tree)>,
    /// A commit ended since the last pass.
    pub(crate) notified: bool,
    /// The thread must stop.
    pub(crate) stop: bool,
}

/// The MVCC state of one database.
#[derive(Default)]
pub(crate) struct Mvcc {
    /// Snapshots of every running transaction.
    pub(crate) snapshots: Readers,
    /// Snapshots of running serializable transactions.
    pub(crate) serializable: Readers,
    /// The old-version store.
    pub(crate) versions: Versions,
    /// Committed write-key sets.
    pub(crate) committed: Committed,
    /// The lock table.
    pub(crate) locks: LockTable,
    /// Garbage-collection work and wake-up.
    gc: Mutex<GcQueue>,
    gc_cv: Condvar,
    /// The next transaction identifier.
    next_tx: AtomicU64,
    /// The open recovered from a crash and the seed scan of garbage
    /// collection has not succeeded yet ([`super::gc::seed`]): a clean
    /// close must not set the clean-shutdown flag, so the next open scans
    /// again.
    pub(crate) unseeded: crate::sync::atomic::AtomicBool,
    /// Tests only: the thread `lpg-gc` skips its passes, so a test runs
    /// them itself ([`super::gc::pass`]) at chosen points.
    #[cfg(test)]
    pub(crate) gc_paused: crate::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for Mvcc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mvcc")
            .field("versions", &self.versions)
            .field("committed", &self.committed)
            .finish_non_exhaustive()
    }
}

impl Mvcc {
    /// A new transaction identifier.
    pub(crate) fn next_tx(&self) -> u64 {
        self.next_tx.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The garbage-collection queue, locked.
    pub(crate) fn gc(&self) -> MutexGuard<'_, GcQueue> {
        // Queue updates are single pushes and pops.
        self.gc.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wakes the garbage collector after a commit (TS-14: it runs after
    /// commits).
    pub(crate) fn notify_gc(&self) {
        self.gc().notified = true;
        self.gc_cv.notify_one();
    }

    /// Asks the garbage collector to stop.
    pub(crate) fn stop_gc(&self) {
        self.gc().stop = true;
        self.gc_cv.notify_all();
    }

    /// Waits until a commit notifies, `interval` passes or a stop; returns
    /// `false` on a stop.
    pub(crate) fn wait_gc(&self, interval: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + interval;
        let mut q = self.gc();
        loop {
            if q.stop {
                return false;
            }
            if q.notified {
                q.notified = false;
                return true;
            }
            let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return true;
            };
            if left.is_zero() {
                return true;
            }
            q = self
                .gc_cv
                .wait_timeout(q, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests;
