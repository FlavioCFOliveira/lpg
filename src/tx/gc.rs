//! Garbage collection (TS-14) on the thread `lpg-gc` (TS-02).
//!
//! A pass runs after commits (they notify the thread) and every
//! `gc_interval` (30 seconds by default, [`crate::storage::engine::Tuning`]),
//! under the commit latch, never on an unusable database. With the horizon
//! H, the oldest snapshot ([`super::mvcc::horizon`]), it:
//! - frees the old versions replaced at or below H ([`super::versions`]);
//! - frees the write-key sets of commits at or below the oldest snapshot of
//!   any running serializable transaction ([`super::conflict`]);
//! - removes the entries whose `deleted_ts` is at or below H, which no
//!   snapshot sees any more, and frees the pages of the trees dropped at or
//!   below H, as one internal batch that joins the open WAL group without a
//!   commit timestamp and does not wait for it (TS-09 Members); leaves the
//!   batch rewrites drop their frozen `created_ts` (TS-06).
//!
//! Memgraph collects garbage the same way, from the oldest active start
//! timestamp, every 30 s by default (R-MG `src/storage/v2/inmemory/storage.cpp:3044,
//! 3115`; `src/flags/general.cpp:105`). PostgreSQL logs pruning as its own
//! WAL record outside any commit (R-PG `src/backend/access/heap/pruneheap.c:1354-1359`).
//!
//! **Work queues.** Commits queue their delete stamps and dropped trees.
//! A pass that fails after taking its work from the queues, in a read or
//! in its batch, puts that work back, so a later pass does it.
//! The queues are in memory: a clean close runs a last pass with no
//! transaction left, so it empties them (H is then the highest published
//! timestamp). After a crash, or a GC batch lost with its group, the
//! thread's first pass finds the remaining entries again by scanning every
//! tree (TS-09 Members: "garbage collection then finds the same entries
//! again"). The scan runs while commits queue their own work, so a pass
//! does each queued entry and tree once. Until the scan succeeds, a clean
//! close leaves the clean-shutdown flag clear (`Mvcc::unseeded`), so the
//! next open scans again.

use std::time::Duration;

use super::mvcc;
use crate::storage::btree::{CommitLatch, DIRECTORY, Store, Tree, decode_directory_entry};
use crate::storage::engine::Core;
use crate::storage::error::Result;

/// What one pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pass {
    /// The horizon used.
    pub(crate) horizon: u64,
    /// Old versions freed.
    pub(crate) versions: usize,
    /// Committed write-key sets freed.
    pub(crate) write_keys: usize,
    /// Delete-stamped entries removed.
    pub(crate) removed: usize,
    /// Dropped trees whose pages were freed.
    pub(crate) trees: usize,
    /// The WAL group the internal batch joined, if one ran.
    pub(crate) group: Option<u64>,
}

/// Runs one garbage-collection pass (module documentation).
///
/// # Errors
///
/// [`crate::storage::error::Error::Unusable`]; storage errors of the reads
/// that find what is left to do, among them
/// [`crate::storage::error::Error::MemoryLimit`], and errors of the
/// internal batch, which is undone. After any of them the pass's work
/// stays queued for the next pass.
pub(crate) fn pass(core: &Core) -> Result<Pass> {
    core.health.check()?;
    let store = core.store();
    let mvcc = &core.mvcc;
    let published = core.wal.progress.published_ts();
    // The batch's pages: one free-list trunk per 1 019 freed pages and the
    // splits of a few deletes; the reserve is not a bound (TS-05), an
    // overrun extends the file under the latch.
    let queued = {
        let q = mvcc.gc();
        q.deleted.len() + q.dropped.len()
    };
    if queued > 0 {
        core.data.ensure(4 + queued as u64 / 512)?;
    }
    let mut st = store.lock();
    core.health.check()?;
    let horizon = mvcc::horizon(&mvcc.snapshots, published);
    let mut out = Pass {
        horizon,
        versions: mvcc.versions.collect(horizon),
        write_keys: mvcc
            .committed
            .collect(mvcc::horizon(&mvcc.serializable, published)),
        ..Pass::default()
    };
    // The work the horizon allows.
    let (deleted, dropped) = {
        let mut q = mvcc.gc();
        let mut deleted = Vec::new();
        q.deleted.retain(|(ts, t, k)| {
            let ready = *ts <= horizon;
            if ready {
                deleted.push((*t, k.clone()));
            }
            !ready
        });
        let mut dropped = Vec::new();
        q.dropped.retain(|(ts, t)| {
            // A tree an index build reads stays until the build ends
            // (`crate::index::build`).
            let ready = *ts <= horizon && !core.builds.uses(t.id());
            if ready {
                dropped.push(*t);
            }
            !ready
        });
        // Entries of a tree being freed go with it.
        q.deleted.retain(|(_, t, _)| !dropped.contains(t));
        deleted.retain(|(t, _)| !dropped.contains(t));
        // The seed scan of an unclean open runs while commits queue their
        // work, so an entry or a tree can be queued twice: each is done
        // once.
        deleted.sort_unstable_by(|a, b| (a.0.id(), &a.1).cmp(&(b.0.id(), &b.1)));
        deleted.dedup_by(|a, b| a.0.id() == b.0.id() && a.1 == b.1);
        dropped.sort_unstable_by_key(|t| t.id());
        dropped.dedup_by_key(|t| t.id());
        (deleted, dropped)
    };
    if deleted.is_empty() && dropped.is_empty() {
        return Ok(out);
    }
    // Every error from here on, a read of the pool ([`Error::MemoryLimit`]
    // when every frame is pinned or dirty) or the batch, which is undone,
    // puts the work back in the queues for the next pass.
    match remove(core, &store, &mut st, horizon, &deleted, &dropped) {
        Ok(None) => Ok(out),
        Ok(Some((removed, trees, seq))) => {
            out.removed = removed;
            out.trees = trees;
            out.group = Some(seq);
            Ok(out)
        }
        Err(e) => {
            let mut q = mvcc.gc();
            for (t, k) in deleted {
                q.deleted.push_front((horizon, t, k));
            }
            for t in dropped {
                q.dropped.push_front((horizon, t));
            }
            Err(e)
        }
    }
}

/// The internal batch of a pass: removes each entry of `deleted` that
/// still carries a delete stamp at or below `horizon` (a later commit may
/// have put the key again) and frees each tree of `dropped` still dropped
/// at or below it. Returns the entries removed, the trees freed and the
/// WAL group the batch joined, or `None` if nothing was left to do.
///
/// # Errors
///
/// Storage errors; errors of the batch, which is undone.
fn remove(
    core: &Core,
    store: &Store<'_>,
    st: &mut CommitLatch<'_>,
    horizon: u64,
    deleted: &[(Tree, Box<[u8]>)],
    dropped: &[Tree],
) -> Result<Option<(usize, usize, u64)>> {
    #[cfg(test)]
    if core
        .mvcc
        .gc_fail_read
        .swap(false, crate::sync::atomic::Ordering::SeqCst)
    {
        return Err(crate::storage::error::Error::MemoryLimit { requested: 0 });
    }
    let mut v = Vec::new();
    let mut removable: Vec<(Tree, &[u8])> = Vec::new();
    for (t, k) in deleted {
        if let Some(stamps) = store.get_stamped(*t, k, &mut v)?
            && stamps.deleted != 0
            && stamps.deleted <= horizon
        {
            removable.push((*t, k));
        }
    }
    let mut frees = Vec::new();
    for t in dropped {
        if let Some((tree, d)) = store.tree_status(t.id())?
            && tree.root() == t.root()
            && d != 0
            && d <= horizon
        {
            frees.push(tree);
        }
    }
    if removable.is_empty() && frees.is_empty() {
        return Ok(None);
    }
    st.set_freeze_horizon(horizon);
    let r = store.apply_internal(st, Some(&core.wal), &removable, &frees);
    st.set_freeze_horizon(0);
    r?;
    let log = st.log();
    log.group.join_batch();
    let seq = log.group.seq();
    core.wal.note_member(seq);
    Ok(Some((removable.len(), frees.len(), seq)))
}

/// Queues the work a crash may have left: every delete-stamped entry and
/// every dropped tree, found by scanning the directory and every tree it
/// lists. Runs once, at the start of the thread, after an open that
/// recovered from a crash.
///
/// # Errors
///
/// Storage errors.
pub(crate) fn seed(core: &Core) -> Result<()> {
    let store = core.store();
    let mut trees = Vec::new();
    store.scan_prefix(DIRECTORY, b"", |k, v| {
        if let (Ok(id), Some((root, dropped))) = (<[u8; 8]>::try_from(k), decode_directory_entry(v))
        {
            trees.push((Tree::new(u64::from_be_bytes(id), root), dropped));
        }
        true
    })?;
    let mut deleted = Vec::new();
    let mut dropped = Vec::new();
    for (t, d) in trees {
        if d != 0 {
            dropped.push((d, t));
            continue;
        }
        store.scan_prefix_stamped(t, b"", |k, _, st| {
            if st.deleted != 0 {
                deleted.push((st.deleted, t, Box::<[u8]>::from(k)));
            }
            true
        })?;
    }
    let mut q = core.mvcc.gc();
    q.deleted.extend(deleted);
    q.dropped.extend(dropped);
    q.notified = true;
    Ok(())
}

/// Tests only: locations whose seed scan fails with
/// [`crate::storage::error::Error::MemoryLimit`], a fail point (TS-30 R10).
/// Keyed by location, so tests in one process do not interfere.
#[cfg(all(test, not(any(loom, shuttle))))]
pub(crate) static FAIL_SEED: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

/// The loop of thread `lpg-gc`: after an unclean open, [`seed`]; then a
/// pass after each notifying commit and every `interval`, until the
/// garbage collector is stopped. Once the database is unusable, it only
/// waits for the stop.
pub(crate) fn run_thread(core: &Core, interval: Duration, seed_first: bool) {
    if seed_first {
        #[cfg(all(test, not(any(loom, shuttle))))]
        let fail = FAIL_SEED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&core.location);
        #[cfg(not(all(test, not(any(loom, shuttle)))))]
        let fail = false;
        // A failed scan leaves `unseeded` set: its entries, invisible to
        // every snapshot, wait for the scan of the next open, which the
        // clean-shutdown flag left clear by `close` makes happen.
        if !fail && seed(core).is_ok() {
            core.mvcc
                .unseeded
                .store(false, crate::sync::atomic::Ordering::SeqCst);
        }
    }
    while core.mvcc.wait_gc(interval) {
        if core.health.is_unusable() {
            continue;
        }
        // Held until the pass ends: a test that pauses the collector waits
        // for a pass in flight (`Mvcc::gc_paused`).
        #[cfg(test)]
        let paused = core
            .mvcc
            .gc_paused
            .lock()
            .unwrap_or_else(crate::sync::PoisonError::into_inner);
        #[cfg(test)]
        if *paused {
            continue;
        }
        let _ = pass(core);
    }
}
