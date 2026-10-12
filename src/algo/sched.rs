//! The block scheduler of parallel entries ([TS-25] Parallelism and
//! Randomness; rmp #63).
//!
//! * **Blocks.** Node indexes `0..n` are cut into blocks of
//!   `max(1, min(4096, ceil(n / 64)))` indexes ([`block_size`]): the size
//!   depends on `n` only, never on `concurrency`.
//! * **Workers.** A parallel entry runs on `min(concurrency, blocks)`
//!   `std::thread::scope` threads, any other entry on one; the thread of
//!   the calling statement only waits. Workers take blocks in index order
//!   from one atomic counter, as Kùzu's morsel dispatcher hands out node
//!   ranges (`src/function/gds/frontier_morsel.cpp`,
//!   `getNextRangeMorsel`, studied), and each block's result is stored at
//!   its block index, so the merge reads them in block order whatever the
//!   completion order: a floating-point fold over blocks adds in the same
//!   order at every `concurrency`.
//! * **Rounds.** An iterative entry keeps two state arrays and runs each
//!   round with [`Sched::fill`], reading the previous round's array and
//!   writing the next one, so round *k* reads only round *k*−1.
//! * **Randomness.** Block *b* draws from `ChaCha8Rng::seed_from_u64(seed)`
//!   after `set_stream(b)` ([`rng`]).
//! * **Cancellation.** Before each block a worker runs the statement's
//!   status check; the first failure stops every worker at its next block
//!   and becomes the run's error.
//!
//! Rejected (TS-25): a work-stealing pool (completion order would decide
//! the merge order) and one block per worker (results would depend on
//! `concurrency`); rayon, whose `reduce` and `sum` leave the order open.
//!
//! [TS-25]: ../../docs/technical-spec/algorithms.md

use std::ops::Range;

use rand::SeedableRng;
use rand::rngs::ChaCha8Rng;

use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::exec::Check;
// `std` directly, not `crate::sync`: no model checker runs the
// scheduler, whose threads are `std::thread::scope` threads.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// The largest block.
pub(crate) const MAX_BLOCK: usize = 4096;

/// The block size for `n` nodes: `max(1, min(4096, ceil(n / 64)))`.
#[must_use]
pub(crate) fn block_size(n: usize) -> usize {
    n.div_ceil(64).clamp(1, MAX_BLOCK)
}

/// The generator of block `block` for `seed` (module documentation).
#[must_use]
pub(crate) fn rng(seed: u64, block: usize) -> ChaCha8Rng {
    let mut r = ChaCha8Rng::seed_from_u64(seed);
    r.set_stream(block as u64);
    r
}

/// The scheduler of one run (module documentation).
#[derive(Clone)]
pub(crate) struct Sched {
    n: usize,
    size: usize,
    blocks: usize,
    workers: usize,
    concurrency: usize,
    parallel: bool,
    check: Option<Check>,
    /// The key the worker-count probe records under ([`probe`]).
    #[cfg(test)]
    pub(crate) probe: Option<String>,
}

impl std::fmt::Debug for Sched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sched")
            .field("n", &self.n)
            .field("size", &self.size)
            .field("blocks", &self.blocks)
            .field("workers", &self.workers)
            .finish_non_exhaustive()
    }
}

impl Sched {
    /// The scheduler of a run over `n` nodes, with at most `concurrency`
    /// workers if `parallel`, else one.
    #[must_use]
    pub(crate) fn new(n: usize, concurrency: usize, parallel: bool, check: Option<Check>) -> Sched {
        let size = block_size(n);
        let blocks = n.div_ceil(size);
        let workers = if parallel {
            concurrency.min(blocks).max(1)
        } else {
            1
        };
        Sched {
            n,
            size,
            blocks,
            workers,
            concurrency,
            parallel,
            check,
            #[cfg(test)]
            probe: None,
        }
    }

    /// The scheduler of the same run over `n` items instead (an
    /// aggregated graph of a community entry): the blocks of `n`, the
    /// same `concurrency`, status check and probe key.
    #[must_use]
    pub(crate) fn resized(&self, n: usize) -> Sched {
        Sched {
            #[cfg(test)]
            probe: self.probe.clone(),
            ..Sched::new(n, self.concurrency, self.parallel, self.check.clone())
        }
    }

    /// The number of blocks.
    #[cfg(test)]
    pub(crate) fn blocks(&self) -> usize {
        self.blocks
    }

    /// The number of workers a call starts.
    #[cfg(test)]
    pub(crate) fn workers(&self) -> usize {
        self.workers
    }

    /// The number of workers a call starts, for the memory a run
    /// reserves per worker.
    pub(crate) fn worker_count(&self) -> usize {
        self.workers
    }

    /// The node indexes of block `b`.
    pub(crate) fn range(&self, b: usize) -> Range<usize> {
        let start = b * self.size;
        start..(start + self.size).min(self.n)
    }

    /// Runs `f` on every block and returns its results in block order.
    ///
    /// # Errors
    ///
    /// The status check's error (25N14); 50N00 if a thread cannot start.
    pub(crate) fn map<R: Send>(
        &self,
        f: impl Fn(usize, Range<usize>) -> R + Sync,
    ) -> Result<Vec<R>, Error> {
        let results: Vec<Mutex<Option<R>>> = (0..self.blocks).map(|_| Mutex::new(None)).collect();
        self.run(0..self.blocks, |b| {
            let r = f(b, self.range(b));
            *results[b].lock().unwrap_or_else(PoisonError::into_inner) = Some(r);
        })?;
        Ok(results
            .into_iter()
            .map(|m| {
                m.into_inner()
                    .unwrap_or_else(PoisonError::into_inner)
                    .expect("every block ran")
            })
            .collect())
    }

    /// Runs `f` on every block with the block's part of `out` (`out` has
    /// one element per node) and returns its results in block order.
    ///
    /// # Errors
    ///
    /// As [`Sched::map`].
    pub(crate) fn fill<T: Send, R: Send>(
        &self,
        out: &mut [T],
        f: impl Fn(usize, Range<usize>, &mut [T]) -> R + Sync,
    ) -> Result<Vec<R>, Error> {
        debug_assert_eq!(out.len(), self.n);
        let parts: Vec<Mutex<Option<&mut [T]>>> = out
            .chunks_mut(self.size)
            .map(|c| Mutex::new(Some(c)))
            .collect();
        self.map(|b, r| {
            let part = parts[b]
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .expect("each block once");
            f(b, r, part)
        })
    }

    /// Runs `f` on every block and folds each block's result into `acc`
    /// with `merge`, in block order. Blocks run in waves of as many
    /// blocks as the run has workers, so at most that many results are
    /// held at once (per-source partial sums of a centrality); the merge
    /// order is the block order whatever the wave size, so the result
    /// does not depend on `concurrency`.
    ///
    /// # Errors
    ///
    /// As [`Sched::map`].
    pub(crate) fn fold<R: Send, A>(
        &self,
        acc: &mut A,
        f: impl Fn(usize, Range<usize>) -> R + Sync,
        mut merge: impl FnMut(&mut A, R),
    ) -> Result<(), Error> {
        let mut start = 0;
        while start < self.blocks {
            let end = (start + self.workers).min(self.blocks);
            let results: Vec<Mutex<Option<R>>> = (start..end).map(|_| Mutex::new(None)).collect();
            self.run(start..end, |b| {
                let r = f(b, self.range(b));
                *results[b - start]
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(r);
            })?;
            for m in results {
                merge(
                    acc,
                    m.into_inner()
                        .unwrap_or_else(PoisonError::into_inner)
                        .expect("every block ran"),
                );
            }
            start = end;
        }
        Ok(())
    }

    /// Runs block body `f` for every block of `blocks` on the run's
    /// workers (module documentation).
    fn run(&self, blocks: Range<usize>, f: impl Fn(usize) + Sync) -> Result<(), Error> {
        let first = blocks.start;
        let last = blocks.end;
        let next = AtomicUsize::new(first);
        let stop = AtomicBool::new(false);
        let failure: Mutex<Option<Error>> = Mutex::new(None);
        let fail = |e: Error| {
            stop.store(true, Ordering::Relaxed);
            failure
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert(e);
        };
        let body = |_worker: usize| {
            loop {
                // Relaxed suffices: the counter only hands out distinct
                // block indexes; the scope's join orders the results.
                let b = next.fetch_add(1, Ordering::Relaxed);
                if b >= last || stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(c) = &self.check
                    && let Err(e) = c(0)
                {
                    fail(e);
                    return;
                }
                f(b);
            }
        };
        #[cfg(test)]
        let key = self.probe.as_deref();
        #[cfg(not(test))]
        let key = None;
        let k = self.workers.min(last - first).max(1);
        if let Err(e) = workers(k, key, body) {
            fail(e);
        }
        match failure.into_inner().unwrap_or_else(PoisonError::into_inner) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Starts `k` scoped worker threads, each running `body` with its worker
/// number, and waits for them. Under test, each thread records itself
/// with the worker-count probe under `key`.
///
/// # Errors
///
/// 50N00 if a thread cannot start; the threads already started finish.
pub(crate) fn workers(
    k: usize,
    key: Option<&str>,
    body: impl Fn(usize) + Sync,
) -> Result<(), Error> {
    #[cfg(test)]
    let seen = probe::Scope::default();
    #[cfg(not(test))]
    let _ = key;
    std::thread::scope(|s| {
        for w in 0..k {
            let body = &body;
            #[cfg(test)]
            let seen = &seen;
            std::thread::Builder::new()
                .name(format!("lpg-algo-{w}"))
                .spawn_scoped(s, move || {
                    #[cfg(test)]
                    seen.enter();
                    body(w);
                })
                .map_err(|_| {
                    Error::new(
                        codes::C50N00,
                        "internal error: an algorithm worker thread could not start",
                    )
                })?;
        }
        Ok::<(), Error>(())
    })?;
    #[cfg(test)]
    if let Some(key) = key {
        probe::record(key, seen.count());
    }
    Ok(())
}

/// The worker-count probe (rmp #61, FR-ALG-006): each worker thread
/// records its thread identifier when it starts; a call's count is the
/// number of distinct threads that ran, and a key keeps the largest count
/// of its calls. Keys are chosen by the caller (the database location for
/// a run), so tests running at the same time do not mix.
#[cfg(test)]
pub(crate) mod probe {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{Mutex, PoisonError};
    use std::thread::ThreadId;

    static COUNTS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

    /// The threads seen by one call.
    #[derive(Default)]
    pub(crate) struct Scope(Mutex<HashSet<ThreadId>>);

    impl Scope {
        pub(crate) fn enter(&self) {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(std::thread::current().id());
        }

        pub(crate) fn count(&self) -> usize {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
        }
    }

    pub(crate) fn record(key: &str, count: usize) {
        let mut m = COUNTS.lock().unwrap_or_else(PoisonError::into_inner);
        let c = m.entry(key.to_string()).or_insert(0);
        *c = (*c).max(count);
    }

    /// The largest count recorded under `key` since the last call, which
    /// clears it.
    pub(crate) fn take(key: &str) -> Option<usize> {
        COUNTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key)
    }

    /// The rounds of the iterative entries (rmp #67), by key.
    static ROUNDS: Mutex<BTreeMap<String, Vec<(&'static str, usize)>>> =
        Mutex::new(BTreeMap::new());

    /// Records that a run under `key` took `rounds` rounds of `what`.
    pub(crate) fn record_rounds(key: &str, what: &'static str, rounds: usize) {
        ROUNDS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.to_string())
            .or_default()
            .push((what, rounds));
    }

    /// The rounds recorded under `key` since the last call, which clears
    /// them.
    pub(crate) fn take_rounds(key: &str) -> Vec<(&'static str, usize)> {
        ROUNDS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_sizes() {
        assert_eq!(block_size(0), 1);
        assert_eq!(block_size(1), 1);
        assert_eq!(block_size(64), 1);
        assert_eq!(block_size(65), 2);
        assert_eq!(block_size(64 * 4096), 4096);
        assert_eq!(block_size(usize::MAX / 2), 4096);
        let s = Sched::new(130, 4, true, None);
        assert_eq!((s.blocks(), s.workers()), (44, 4));
        assert_eq!(s.range(43), 129..130);
        assert_eq!(Sched::new(1, 16, true, None).workers(), 1);
        assert_eq!(Sched::new(2, 16, true, None).workers(), 2);
        assert_eq!(Sched::new(500, 16, false, None).workers(), 1);
        assert_eq!(Sched::new(0, 4, true, None).blocks(), 0);
    }

    /// Results come back in block order however the workers interleave;
    /// `fill` hands each block its own part of the output.
    #[test]
    fn block_order_and_fill() {
        for c in [1, 3, 16] {
            let s = Sched::new(1000, c, true, None);
            let got = s.map(|b, r| (b, r.start)).unwrap();
            assert!(
                got.iter()
                    .enumerate()
                    .all(|(i, (b, st))| i == *b && *st == i * 16)
            );
            let mut out = vec![0usize; 1000];
            let lens = s
                .fill(&mut out, |_, r, part| {
                    for (x, v) in part.iter_mut().zip(r.clone()) {
                        *x = v * 2;
                    }
                    part.len()
                })
                .unwrap();
            assert_eq!(lens.iter().sum::<usize>(), 1000);
            assert!(out.iter().enumerate().all(|(i, x)| *x == 2 * i));
        }
    }

    /// `fold` merges the block results in block order, whatever the
    /// number of workers (and so of waves); `resized` keeps the run's
    /// `concurrency`.
    #[test]
    fn fold_in_block_order() {
        let sum = |c: usize| {
            let s = Sched::new(1000, c, true, None);
            let mut acc = (0.0f64, Vec::new());
            s.fold(
                &mut acc,
                |b, r| (b, r.map(|v| 1.0 / (v as f64 + 1.0)).sum::<f64>()),
                |acc, (b, x)| {
                    acc.0 += x;
                    acc.1.push(b);
                },
            )
            .unwrap();
            acc
        };
        let one = sum(1);
        assert_eq!(one.1, (0..63).collect::<Vec<_>>());
        for c in [2, 5, 16] {
            let other = sum(c);
            assert_eq!(other.0.to_bits(), one.0.to_bits());
            assert_eq!(other.1, one.1);
        }
        let s = Sched::new(1000, 3, true, None).resized(10);
        assert_eq!((s.blocks(), s.workers()), (10, 3));
        assert_eq!(Sched::new(1000, 3, false, None).resized(10).workers(), 1);
    }

    /// The first failed check stops the run with its error.
    #[test]
    fn a_failed_check_stops_the_run() {
        let calls = AtomicUsize::new(0);
        let check: Check =
            std::sync::Arc::new(|_| Err(Error::new(codes::C25N14, "transaction terminated")));
        let s = Sched::new(1000, 4, true, Some(check));
        let e = s
            .map(|_, _| calls.fetch_add(1, Ordering::Relaxed))
            .unwrap_err();
        assert_eq!(e.code(), "25N14");
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    /// Block generators are reproducible and distinct per block.
    #[test]
    fn block_generators() {
        use rand::Rng;
        let a: u64 = rng(7, 3).next_u64();
        assert_eq!(a, rng(7, 3).next_u64());
        assert_ne!(a, rng(7, 4).next_u64());
        assert_ne!(a, rng(8, 3).next_u64());
    }
}
