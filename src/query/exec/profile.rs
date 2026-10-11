// Licensed under the Apache License, Version 2.0 (see `LICENSE-APACHE` at
// the repository root).
//
// NOTICE of the source project:
//   Grafeo
//   Copyright 2025-2026 S.T. Grond
//   This product includes software developed at
//   https://github.com/GrafeoDB/grafeo
//   Licensed under the Apache License, Version 2.0.
//
//! PROFILE (rmp #45; TS-20 "EXPLAIN and PROFILE"; FR-OBS-006): a
//! decorator around each operator that measures it.
//!
//! Copied from Grafeo @ `3c23dbe` (<https://github.com/GrafeoDB/grafeo>,
//! Apache-2.0), `crates/grafeo-core/src/execution/profile.rs`:
//! `ProfileStats` (rows out, time, calls) and `ProfiledOperator` (time
//! each `next`, count the rows of each returned chunk, delegate `reset`).
//! Adapted: `parking_lot` is removed; the statistics live in the
//! statement's [`Profiler`], indexed by the operator's position in the
//! plan, instead of an `Arc<Mutex<_>>` per operator; the counters of
//! TS-20 are added: batches, B+tree entries read (Neo4j's "DB Hits"),
//! buffer hits and misses (Neo4j's page cache hits and misses), peak
//! memory, each exclusive of the operator's inputs; and the number of
//! operators running at once (NFR-PERF-007).

use std::time::Instant;

use super::{Batch, Cx, Operator};
use crate::error::Error;
use crate::query::plan::Description;
use crate::storage::counters;

/// The measured statistics of one operator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProfileStats {
    /// Rows produced.
    pub(crate) rows: u64,
    /// Batches produced.
    pub(crate) batches: u64,
    /// Calls of `next_batch`.
    pub(crate) calls: u64,
    /// Time in the operator, its inputs excluded, in nanoseconds.
    pub(crate) time_ns: u64,
    /// B+tree entries its own reads returned (Neo4j's "DB Hits").
    pub(crate) entries_read: u64,
    /// The same reads counted by the transaction layer: the independent
    /// check of `entries_read` (FR-OBS-006 acceptance).
    pub(crate) tx_entries: u64,
    /// Buffer pool hits of its own reads.
    pub(crate) hits: u64,
    /// Buffer pool misses of its own reads.
    pub(crate) misses: u64,
    /// The most memory it reserved from the memory accountant at once
    /// (Neo4j's "Memory (Bytes)").
    pub(crate) peak_memory: u64,
}

/// Counters read before and after a call.
#[derive(Debug, Clone, Copy, Default)]
struct Totals {
    entries: u64,
    tx_entries: u64,
    hits: u64,
    misses: u64,
    time_ns: u64,
}

impl Totals {
    fn sub(self, o: Totals) -> Totals {
        Totals {
            entries: self.entries.saturating_sub(o.entries),
            tx_entries: self.tx_entries.saturating_sub(o.tx_entries),
            hits: self.hits.saturating_sub(o.hits),
            misses: self.misses.saturating_sub(o.misses),
            time_ns: self.time_ns.saturating_sub(o.time_ns),
        }
    }

    fn add(&mut self, o: Totals) {
        self.entries += o.entries;
        self.tx_entries += o.tx_entries;
        self.hits += o.hits;
        self.misses += o.misses;
        self.time_ns += o.time_ns;
    }
}

/// The statistics of a profiled statement.
#[derive(Debug, Default)]
pub(crate) struct Profiler {
    stats: Vec<ProfileStats>,
    /// The inclusive totals of the children of each running operator.
    stack: Vec<Totals>,
    running: usize,
    max_running: usize,
}

impl Profiler {
    /// A new operator's index.
    pub(crate) fn register(&mut self) -> usize {
        self.stats.push(ProfileStats::default());
        self.stats.len() - 1
    }

    /// The highest number of operators running at once: an operator runs
    /// from the start of its `next_batch` to its end, except while it waits
    /// in a call of one of its inputs.
    pub(crate) fn max_running(&self) -> usize {
        self.max_running
    }

    /// Adds the statistics to a plan description, in the order the plan's
    /// operators were built (pre-order).
    pub(crate) fn annotate(&self, d: &mut Description) {
        fn walk(d: &mut Description, stats: &[ProfileStats], at: &mut usize) {
            d.profile = stats.get(*at).copied();
            *at += 1;
            for c in &mut d.children {
                walk(c, stats, at);
            }
        }
        let mut at = 0;
        walk(d, &self.stats, &mut at);
    }
}

/// The decorator of one operator.
pub(crate) struct Profiled {
    inner: Box<dyn Operator>,
    id: usize,
}

impl Profiled {
    /// Wraps `inner` as operator `id`.
    pub(crate) fn new(inner: Box<dyn Operator>, id: usize) -> Profiled {
        Profiled { inner, id }
    }
}

fn now(cx: &Cx<'_, '_>, t0: Instant) -> Totals {
    let c = counters::current();
    Totals {
        entries: c.entries,
        tx_entries: cx.tx.entries_read,
        hits: c.hits,
        misses: c.misses,
        time_ns: u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX),
    }
}

impl Operator for Profiled {
    fn next_batch(&mut self, cx: &mut Cx<'_, '_>) -> Result<Option<Batch>, Error> {
        let t0 = Instant::now();
        {
            let p = cx
                .st
                .profile
                .as_mut()
                .expect("a profiled plan has a profiler");
            if !p.stack.is_empty() {
                // The caller waits in this call.
                p.running -= 1;
            }
            p.running += 1;
            p.max_running = p.max_running.max(p.running);
            p.stack.push(Totals::default());
        }
        let before = now(cx, t0);
        let r = self.inner.next_batch(cx);
        let after = now(cx, t0);
        let held = self.inner.held();
        let p = cx
            .st
            .profile
            .as_mut()
            .expect("a profiled plan has a profiler");
        let children = p.stack.pop().unwrap_or_default();
        let inclusive = after.sub(before);
        let own = inclusive.sub(children);
        if let Some(parent) = p.stack.last_mut() {
            parent.add(inclusive);
        }
        p.running -= 1;
        if !p.stack.is_empty() {
            p.running += 1;
        }
        let s = &mut p.stats[self.id];
        s.calls += 1;
        s.time_ns += own.time_ns;
        s.entries_read += own.entries;
        s.tx_entries += own.tx_entries;
        s.hits += own.hits;
        s.misses += own.misses;
        s.peak_memory = s.peak_memory.max(held);
        if let Ok(Some(b)) = &r {
            s.rows += b.len() as u64;
            s.batches += 1;
        }
        r
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn held(&self) -> u64 {
        self.inner.held()
    }
}
