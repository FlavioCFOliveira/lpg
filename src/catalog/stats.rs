//! Statistics (TS-17 Statistics): node counts per label, edge counts per
//! `(label, edge type, direction)` with the label on one side, per graph,
//! and entry and distinct-key counts per ONLINE index ([`crate::index`]).
//!
//! - **In memory**, changed by the committer when it applies a commit
//!   (TS-16 step 6; [`super::Effects`]), under the commit latch. A label
//!   added to or removed from a node changes that label's edge counts by
//!   every adjacency entry of the node that the commit neither creates nor
//!   deletes; edges the commit creates or deletes count with the node's
//!   labels after or before it ([`crate::storage::graph::stats_delta`];
//!   Neo4j `TransactionCountingStateVisitor`, R-NEO
//!   `community/storage-engine-util/.../TransactionCountingStateVisitor.java:126-143`).
//! - **Saved** by each checkpoint before its step 1 ([`save`]): one
//!   internal batch, built under the commit latch, writes the counts that
//!   changed since the last save to the statistics tree, with the last
//!   commit timestamp applied; the checkpoint waits until its group is
//!   durable, so its images lie below the redo point (TS-11). The tree is
//!   unstamped (TS-06).
//! - **Loaded** at open ([`load`]); when recovery applied a commit above
//!   the saved timestamp, open recounts them exactly before it returns
//!   ([`crate::storage::graph::recount`]; Neo4j rebuilds an untrusted counts
//!   store by a full count before transactions run, R-NEO
//!   `community/counts-store/.../GBPTreeGenericCountsStore.java:195-201,
//!   252-267`).
//!
//! **Statistics tree** (tree [`super::STATS_TREE`]). Key `N`, graph,
//! label → count; key `E`, graph, label, edge type, direction → count;
//! key `I`, graph, index, distinct flag (1 byte) → count of the index's
//! entries (flag 0) or distinct keys (flag 1); key `T` → the last commit
//! timestamp the counts include. Integers in keys
//! are 8 bytes big-endian (the direction 1 byte), counts and the timestamp
//! 8 bytes little-endian.

use std::collections::BTreeMap;

use super::STATS_TREE;
use crate::storage::btree::Write;
use crate::storage::engine::Core;
use crate::storage::error::{Error, Result};

const NODES: u8 = b'N';
const EDGES: u8 = b'E';
const INDEX: u8 = b'I';
const TS_KEY: &[u8] = b"T";

/// One statistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StatKey {
    /// The nodes of `graph` with `label`.
    Nodes {
        /// The graph's catalog object identifier.
        graph: u64,
        /// The label token.
        label: u64,
    },
    /// The edges of `graph` of type `ty` whose endpoint on side `dir`
    /// (0: the source, OUT; 1: the target, IN) has `label`.
    Edges {
        /// The graph's catalog object identifier.
        graph: u64,
        /// The label token.
        label: u64,
        /// The edge-type token.
        ty: u64,
        /// The side of the labelled endpoint.
        dir: u8,
    },
    /// The entries of index `index` of `graph`, or, with `distinct`, its
    /// distinct keys (TS-17; [`crate::index::prefix_of`]).
    Index {
        /// The graph's catalog object identifier.
        graph: u64,
        /// The index's catalog object identifier.
        index: u64,
        /// Distinct keys rather than entries.
        distinct: bool,
    },
}

impl StatKey {
    /// The graph the statistic counts in.
    pub(crate) fn graph(&self) -> u64 {
        match *self {
            StatKey::Nodes { graph, .. }
            | StatKey::Edges { graph, .. }
            | StatKey::Index { graph, .. } => graph,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut k = Vec::with_capacity(26);
        match *self {
            StatKey::Nodes { graph, label } => {
                k.push(NODES);
                k.extend_from_slice(&graph.to_be_bytes());
                k.extend_from_slice(&label.to_be_bytes());
            }
            StatKey::Edges {
                graph,
                label,
                ty,
                dir,
            } => {
                k.push(EDGES);
                k.extend_from_slice(&graph.to_be_bytes());
                k.extend_from_slice(&label.to_be_bytes());
                k.extend_from_slice(&ty.to_be_bytes());
                k.push(dir);
            }
            StatKey::Index {
                graph,
                index,
                distinct,
            } => {
                k.push(INDEX);
                k.extend_from_slice(&graph.to_be_bytes());
                k.extend_from_slice(&index.to_be_bytes());
                k.push(u8::from(distinct));
            }
        }
        k
    }

    fn decode(k: &[u8]) -> Option<StatKey> {
        let be = |at: usize| Some(u64::from_be_bytes(k.get(at..at + 8)?.try_into().ok()?));
        match (k.first(), k.len()) {
            (Some(&NODES), 17) => Some(StatKey::Nodes {
                graph: be(1)?,
                label: be(9)?,
            }),
            (Some(&EDGES), 26) => Some(StatKey::Edges {
                graph: be(1)?,
                label: be(9)?,
                ty: be(17)?,
                dir: k[25],
            }),
            (Some(&INDEX), 18) if k[17] <= 1 => Some(StatKey::Index {
                graph: be(1)?,
                index: be(9)?,
                distinct: k[17] == 1,
            }),
            _ => None,
        }
    }
}

/// Changes of statistics, by key.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Delta(BTreeMap<StatKey, i64>);

impl Delta {
    /// Adds `d` to statistic `k`.
    pub(crate) fn add(&mut self, k: StatKey, d: i64) {
        let e = self.0.entry(k).or_insert(0);
        *e += d;
        if *e == 0 {
            self.0.remove(&k);
        }
    }

    /// Adds every change of `other`.
    pub(crate) fn merge(&mut self, other: &Delta) {
        for (k, d) in &other.0 {
            self.add(*k, *d);
        }
    }
}

/// The statistics of an open database.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    /// The counts of the commits applied, without zeros.
    counts: BTreeMap<StatKey, u64>,
    /// What the statistics tree holds.
    saved: BTreeMap<StatKey, u64>,
    /// The commit timestamp the statistics tree holds.
    saved_ts: u64,
}

impl Stats {
    /// Every non-zero statistic.
    pub(crate) fn counts(&self) -> &BTreeMap<StatKey, u64> {
        &self.counts
    }

    /// The commit timestamp the statistics tree holds.
    pub(crate) fn saved_ts(&self) -> u64 {
        self.saved_ts
    }

    /// Applies the changes of a commit. It runs under the commit latch and
    /// never panics: a count that would go below zero is wrong, so it is
    /// clamped to zero, and the integrity check reports the drift by
    /// comparing the statistics with a recount
    /// ([`crate::storage::engine::Engine::check_graphs`]; Neo4j's
    /// consistency checker reports an inconsistent count the same way,
    /// R-NEO `community/record-storage-engine/.../consistency/checker/CountsState.java:217-229`).
    pub(crate) fn apply(&mut self, d: &Delta) {
        for (k, v) in &d.0 {
            let old = self.counts.get(k).copied().unwrap_or(0);
            match old.checked_add_signed(*v).unwrap_or(0) {
                0 => {
                    self.counts.remove(k);
                }
                n => {
                    self.counts.insert(*k, n);
                }
            }
        }
    }

    /// Removes every statistic of `graph`, which a commit dropped.
    pub(crate) fn remove_graph(&mut self, graph: u64) {
        self.counts.retain(|k, _| k.graph() != graph);
    }

    /// Removes the statistics of index `index`, which a commit dropped.
    pub(crate) fn remove_index(&mut self, index: u64) {
        self.counts
            .retain(|k, _| !matches!(k, StatKey::Index { index: i, .. } if *i == index));
    }

    /// Replaces the counts with a recount's.
    pub(crate) fn replace(&mut self, counts: BTreeMap<StatKey, u64>) {
        self.counts = counts;
    }

    /// Records that the statistics tree now holds the counts, at commit
    /// timestamp `ts`.
    pub(crate) fn mark_saved(&mut self, ts: u64) {
        self.saved = self.counts.clone();
        self.saved_ts = ts;
    }

    /// The changes a save at commit timestamp `ts` writes: the counts that
    /// differ from the tree's, the removed ones as deletes, and the
    /// timestamp. Empty when nothing differs.
    pub(crate) fn changes(&self, ts: u64) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        let mut out = Vec::new();
        for (k, v) in &self.counts {
            if self.saved.get(k) != Some(v) {
                out.push((k.encode(), Some(v.to_le_bytes().to_vec())));
            }
        }
        for k in self.saved.keys() {
            if !self.counts.contains_key(k) {
                out.push((k.encode(), None));
            }
        }
        if !out.is_empty() || ts != self.saved_ts {
            out.push((TS_KEY.to_vec(), Some(ts.to_le_bytes().to_vec())));
        }
        out.sort();
        out
    }
}

/// Loads the statistics tree (TS-13 step 7).
///
/// # Errors
///
/// [`Error::Corruption`] for a malformed entry or a missing tree; storage
/// errors.
pub(crate) fn load(core: &Core) -> Result<Stats> {
    let store = core.store();
    let tree = store.tree(STATS_TREE)?.ok_or_else(|| {
        core.health
            .fail(Error::Corruption("the statistics tree is missing".into()))
    })?;
    let mut st = Stats::default();
    let mut bad = false;
    store.scan_prefix(tree, b"", |k, v| {
        let value = <[u8; 8]>::try_from(v).ok().map(u64::from_le_bytes);
        match (k == TS_KEY, StatKey::decode(k), value) {
            (true, _, Some(ts)) => st.saved_ts = ts,
            (false, Some(key), Some(n)) if n > 0 => {
                st.saved.insert(key, n);
            }
            _ => bad = true,
        }
        true
    })?;
    if bad {
        return Err(core
            .health
            .fail(Error::Corruption("malformed statistics entry".into())));
    }
    st.counts = st.saved.clone();
    Ok(st)
}

/// Saves the statistics (TS-11 Statistics first): under the commit latch,
/// the changes since the last save, with the last commit timestamp
/// applied, as one internal batch that joins the open WAL group without a
/// timestamp (TS-09 Members). Returns the group to wait for, or `None` if
/// nothing changed.
///
/// # Errors
///
/// ENOSPC or [`Error::MemoryLimit`], undone, with the database usable;
/// any other error, undone, with the database unusable.
pub(crate) fn save(core: &Core) -> Result<Option<u64>> {
    core.health.check()?;
    let store = core.store();
    let tree = store.tree(STATS_TREE)?.ok_or_else(|| {
        core.health
            .fail(Error::Corruption("the statistics tree is missing".into()))
    })?;
    // The batch's pages: a few splits and trunks; the reserve is not a
    // bound (TS-05), an overrun extends the file under the latch.
    let n = core.catalog.stats().counts.len() as u64;
    core.data.ensure(4 + n / 128)?;
    let mut st = store.lock();
    core.health.check()?;
    let ts = st.log_ref().counters.commit_ts;
    let mut stats = core.catalog.stats();
    let changes = stats.changes(ts);
    if changes.is_empty() {
        return Ok(None);
    }
    let batch: Vec<Write<'_>> = changes
        .iter()
        .map(|(k, v)| Write {
            tree,
            key: k,
            value: v.as_deref(),
            stamp: 0,
        })
        .collect();
    store.apply_batch(&mut st, Some(&core.wal), &batch, 0)?;
    stats.mark_saved(ts);
    drop(stats);
    let log = st.log();
    log.group.join_batch();
    let seq = log.group.seq();
    core.wal.note_member(seq);
    Ok(Some(seq))
}
