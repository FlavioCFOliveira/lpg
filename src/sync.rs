//! The synchronisation facade (TS-30 Concurrency, R11).
//!
//! Every synchronisation primitive of the crate is imported from here, never
//! from `std::sync` directly. A normal build re-exports `std`. Under
//! `RUSTFLAGS="--cfg loom"` the primitives come from `loom`, which explores
//! every interleaving of a small model; under `RUSTFLAGS="--cfg shuttle"`
//! they come from `shuttle`, which runs randomised schedules of larger tests.
//! The crate compiles under each `cfg`; only the tests named `loom_*` and
//! `shuttle_*` are meant to run there.
//!
//! **Always from `std`**, whatever the `cfg`:
//! - `Arc`. `loom`'s `Arc` cannot be coerced to `Arc<dyn Trait>` (unsized
//!   coercion of a foreign type needs the unstable `CoerceUnsized`), and the
//!   crate holds `Arc<dyn Storage>` and `Arc<dyn MetricsSink>`; `shuttle`
//!   re-exports `std`'s `Arc` itself (`shuttle-std` 0.1.2
//!   `src/sync/mod.rs:25`). `loom`'s `Arc` only adds leak checks, which no
//!   model here needs.
//! - `PoisonError`, `TryLockError`: both checkers use `std`'s
//!   (`loom` 0.7.2 `src/sync/mod.rs:20`, `shuttle-std` 0.1.2
//!   `src/sync/mod.rs:22`).
//! - `Barrier` and `mpsc`: used only by tests that run threads of `std`,
//!   never inside a model.
//!
//! **Origin.** The `cfg`-switched re-export module follows Turso's
//! `core/sync.rs` (R-TURSO @ `8cc059b`, MIT licence, "Copyright 2024 the
//! Turso authors"; <https://github.com/tursodatabase/turso>), the starting
//! point approved as R11. Its `shuttle` adapter wraps each lock to return
//! guards without `LockResult` (the `parking_lot` shape Turso uses), and its
//! `lock_arc` transmutes a guard's lifetime with `unsafe`
//! (`core/sync.rs:46-58`). Neither is taken: lpg keeps `std`'s API, poisoning
//! included (the commit latch relies on it, `storage::btree::CommitLatch`),
//! and the crate forbids `unsafe`. What remains of the starting point is the
//! module's shape. Turso's licence notice:
//!
//! > Permission is hereby granted, free of charge, to any person obtaining a
//! > copy of this software and associated documentation files (the
//! > "Software"), to deal in the Software without restriction, including
//! > without limitation the rights to use, copy, modify, merge, publish,
//! > distribute, sublicense, and/or sell copies of the Software, and to
//! > permit persons to whom the Software is furnished to do so, subject to
//! > the following conditions: The above copyright notice and this permission
//! > notice shall be included in all copies or substantial portions of the
//! > Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
//! > KIND, EXPRESS OR IMPLIED.

pub(crate) use std::sync::{Arc, PoisonError, TryLockError};
#[cfg(test)]
pub(crate) use std::sync::{Barrier, mpsc};

#[cfg(not(any(loom, shuttle)))]
pub(crate) use std::sync::{Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(loom)]
pub(crate) use loom::sync::{
    Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

#[cfg(all(shuttle, not(loom)))]
pub(crate) use shuttle::sync::{
    Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

/// Atomics and fences.
pub(crate) mod atomic {
    #[cfg(not(any(loom, shuttle)))]
    pub(crate) use std::sync::atomic::{
        AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence,
    };

    #[cfg(loom)]
    pub(crate) use loom::sync::atomic::{
        AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence,
    };

    #[cfg(all(shuttle, not(loom)))]
    pub(crate) use shuttle::sync::atomic::{
        AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence,
    };
}

/// Threads: spawning, yielding and the panic state.
pub(crate) mod thread {
    #[cfg(not(any(loom, shuttle)))]
    pub(crate) use std::thread::{Builder, JoinHandle, panicking, yield_now};

    #[cfg(loom)]
    pub(crate) use loom::thread::{Builder, JoinHandle, panicking, yield_now};

    #[cfg(all(shuttle, not(loom)))]
    pub(crate) use shuttle::thread::{Builder, JoinHandle, panicking, yield_now};

    /// Waits about `d` without holding anything, in a loop that polls
    /// another thread's progress. Under a model checker it only yields:
    /// neither checker models time, and a real sleep would stop the one
    /// thread they run at a time.
    pub(crate) fn pause(d: std::time::Duration) {
        #[cfg(not(any(loom, shuttle)))]
        std::thread::sleep(d);
        #[cfg(any(loom, shuttle))]
        {
            let _ = d;
            yield_now();
        }
    }
}

#[cfg(not(any(loom, shuttle)))]
pub(crate) use std::thread_local;

#[cfg(loom)]
pub(crate) use loom::thread_local;

#[cfg(all(shuttle, not(loom)))]
pub(crate) use shuttle::thread_local;
