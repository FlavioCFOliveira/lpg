//! Tests of the transaction layer: MVCC (TS-14, rmp #27), isolation and
//! locks (TS-15, #28, with the Hermitage scenarios and Elle histories),
//! garbage collection (#29).

use std::time::Duration;

use super::*;
use crate::obs::recording::RecordingSink;
use crate::storage::btree::Stamps;
use crate::storage::engine::Tuning;
use crate::sync::Arc;
use crate::test_support::{graph_dir, no_flush};

#[cfg(not(any(loom, shuttle)))]
mod elle;
#[cfg(not(any(loom, shuttle)))]
mod gc;
#[cfg(not(any(loom, shuttle)))]
mod hermitage;
#[cfg(not(any(loom, shuttle)))]
mod isolation;
#[cfg(not(any(loom, shuttle)))]
mod mvcc;
#[cfg(all(shuttle, not(loom)))]
mod shuttle;

/// Memory cap of these tests: 32 MiB.
pub(crate) const CAP: u64 = 32 << 20;

/// Triggers that never fire during a test: no periodic checkpoint and no
/// periodic garbage collection (commits still wake the collector).
pub(crate) fn tuning() -> Tuning {
    Tuning {
        checkpoint_interval: Duration::from_secs(3600),
        checkpoint_commits: u64::MAX,
        gc_interval: Duration::from_secs(3600),
    }
}

/// A new database on the RAM-backed test directory (`NoFlush`: no test
/// here crashes the operating system).
pub(crate) fn db() -> (tempfile::TempDir, Engine) {
    db_with_cap(CAP)
}

pub(crate) fn db_with_cap(cap: u64) -> (tempfile::TempDir, Engine) {
    let d = graph_dir();
    let e = Engine::create_tuned(
        d.path(),
        no_flush(),
        cap,
        crate::test_support::OWNER,
        Arc::new(RecordingSink::default()),
        tuning(),
    )
    .unwrap();
    (d, e)
}

/// Reopens the database at `dir`.
pub(crate) fn reopen(dir: &std::path::Path) -> Engine {
    Engine::open_tuned(
        dir,
        no_flush(),
        CAP,
        Arc::new(RecordingSink::default()),
        tuning(),
    )
    .unwrap()
}

/// One new tree.
pub(crate) fn tree(e: &Engine) -> Tree {
    e.create_trees(1).unwrap()[0]
}

/// Commits the puts `kv` in one serializable transaction.
pub(crate) fn commit_puts(e: &Engine, t: Tree, kv: &[(&[u8], &[u8])]) -> u64 {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    for (k, v) in kv {
        tx.put(t, k, v).unwrap();
    }
    tx.commit()
        .unwrap()
        .expect("a write commit draws a timestamp")
}

/// The value of `k` in a new transaction.
pub(crate) fn read(e: &Engine, t: Tree, k: &[u8]) -> Option<Vec<u8>> {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let v = tx.get(t, k).unwrap();
    tx.commit().unwrap();
    v
}

/// Every entry of `t` that `tx` sees.
pub(crate) fn all(tx: &mut Transaction<'_>, t: Tree) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    tx.scan_prefix(t, b"", |k, v| {
        out.push((k.to_vec(), v.to_vec()));
        true
    })
    .unwrap();
    out
}

/// Every physical entry of `t` with its stamps.
pub(crate) fn raw(e: &Engine, t: Tree) -> Vec<(Vec<u8>, Vec<u8>, Stamps)> {
    let mut out = Vec::new();
    e.store()
        .scan_prefix_stamped(t, b"", |k, v, s| {
            out.push((k.to_vec(), v.to_vec(), s));
            true
        })
        .unwrap();
    out
}

/// Waits, without timing, until `n` lock requests wait; panics after
/// [`crate::test_support::WAIT_BOUND`].
pub(crate) fn until_waiting(e: &Engine, n: usize) {
    crate::test_support::wait_until("lock requests wait", || e.core().mvcc.locks.waiting() >= n);
}

/// Stops the garbage collector's own passes: the test runs them. Returns
/// once no pass of the collector's thread runs, and none starts until the
/// database is dropped.
pub(crate) fn pause_gc(e: &Engine) {
    *e.core()
        .mvcc
        .gc_paused
        .lock()
        .unwrap_or_else(crate::sync::PoisonError::into_inner) = true;
}

pub(crate) fn kv(pairs: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    pairs
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect()
}
