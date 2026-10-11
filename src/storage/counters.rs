//! Per-thread read counters (rmp #45; FR-OBS-006, TS-20 PROFILE): B+tree
//! entries returned to callers and buffer pool hits and misses, counted
//! on the thread that reads. A statement runs on one thread at a time
//! (NFR-PERF-007), so PROFILE attributes the counts between two reads of
//! [`current`] to the operator that ran in between, as Neo4j's page
//! cursor tracers count per thread of execution.

use std::cell::Cell;

use crate::sync::thread_local;

/// The counts of the current thread since it started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// B+tree entries returned by point reads and scans.
    pub(crate) entries: u64,
    /// Buffer pool hits.
    pub(crate) hits: u64,
    /// Buffer pool misses.
    pub(crate) misses: u64,
}

thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts { entries: 0, hits: 0, misses: 0 }) };
}

/// The current thread's counts.
pub(crate) fn current() -> Counts {
    COUNTS.with(Cell::get)
}

fn bump(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|c| {
        let mut v = c.get();
        f(&mut v);
        c.set(v);
    });
}

/// Counts one B+tree entry returned.
pub(crate) fn entry() {
    bump(|c| c.entries += 1);
}

/// Counts one buffer pool hit.
pub(crate) fn hit() {
    bump(|c| c.hits += 1);
}

/// Counts one buffer pool miss.
pub(crate) fn miss() {
    bump(|c| c.misses += 1);
}
