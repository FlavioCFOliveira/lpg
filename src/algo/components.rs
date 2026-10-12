//! The components and connectivity family (rmp #67): ALG-083 weakly
//! connected components, ALG-084 connected components, ALG-085 strongly
//! connected components, ALG-086 condensation, ALG-089 biconnected
//! components, ALG-090 articulation points, ALG-091 bridges, ALG-093
//! k-core decomposition. 083 and 093 are parallel; the others run on one
//! worker.
//!
//! * **083** (parallel): minimum-label rounds. Each round reads the last
//!   round's labels only: a node takes the smallest label of itself and
//!   its neighbours over both directions of every edge, then the label of
//!   that label (pointer jumping). Labels never exceed the node's index
//!   and are always indexes of the same component, so the rounds end with
//!   every node labelled by the smallest index of its component, at every
//!   `concurrency`. References: Kùzu @ `89f0263cc7a1`
//!   `extension/algo/src/function/weakly_connected_components.cpp` and
//!   NetworKit @ `5f03e17cec09` `components/ParallelConnectedComponents.cpp`
//!   propagate the minimum label by rounds; LAGraph @ `7543de1f3103`
//!   `LG_CC_FastSV6.c` adds the pointer jumping (FastSV); GDS @
//!   `417d7ecf973d` `wcc/` links a concurrent union-find with CAS (GPL,
//!   studied only), whose result depends on thread timing only through
//!   the identifiers. lpg keeps the rounds (MIT and BSD methods, written
//!   here) because a round reads a fixed array. GDS's rules are kept:
//!   with `relationshipWeightProperty` only edges heavier than `threshold`
//!   join nodes, an edge without the property counts as heavier
//!   (`Wcc.java` `defaultWeight` = `threshold + 1`); with `seedProperty`
//!   a component's identifier is the smallest non-negative seed of its
//!   nodes, and a component without one takes the next identifier above
//!   the largest seed (`HugeAtomicDisjointSetStruct.java` `setIdOf`,
//!   `union` by minimum). Without seeds the identifier is the smallest
//!   node index of the component; `consecutiveIds` numbers the
//!   components 0, 1, … in the order of their smallest node.
//! * **084** calls petgraph 0.8.3's `UnionFind` over every edge of the
//!   undirected projection (`unionfind.rs`; petgraph's
//!   `connected_components`, `mod.rs:136`, returns only the count); the
//!   identifier is the smallest node index of the component.
//! * **085** calls petgraph's `tarjan_scc` on the projection
//!   (`scc/tarjan_scc.rs:267`); identifiers as 084.
//! * **086** follows petgraph's `condensation` with `make_acyclic`
//!   (`mod.rs:477`, MIT, licence text in [`super::projection`]): the
//!   components of `tarjan_scc`, one edge between two components however
//!   many edges join them, no self-loop. Modified: components numbered in
//!   the order of their smallest node; the result is the rows, not a
//!   graph.
//! * **089** is Hopcroft and Tarjan's edge-stack depth-first search on
//!   the simple undirected graph ([`super::kernels::Simple`]), as
//!   rustworkx-core @ `25398ab47f66` `connectivity/biconnected.rs` and
//!   NetworkX `components/biconnected.py` compute it; written here. One
//!   row per edge: MAGE's columns `bcc_id`, `node_from`, `node_to`
//!   (`biconnected_components_module.cpp`), parallel edges once.
//! * **090, 091** run GDS's low-link depth-first search
//!   (`articulationpoints/ArticulationPoints.java`, `bridges/Bridges.java`,
//!   studied): a node's first edge back to its parent is the tree edge,
//!   so a parallel edge to the parent is a back edge and the pair is no
//!   bridge. petgraph's `bridges` skips every edge to the parent
//!   (`bridges.rs:83`) and reports both parallel edges as bridges, so it
//!   is not called. `resultingComponents` holds the `count`, `max` and
//!   `min` of the sizes of the pieces the node's component falls into
//!   without it, and `remainingSizes` the sizes of the two sides of a
//!   bridge, `from`'s first (both measured on GDS 2026.09.0, rmp #67:
//!   karate node 0 gives 3 pieces of 27 to 1 nodes, bridge 0–11 `[33,
//!   1]`). The write mode of 090 sets 1 on articulation points and 0 on
//!   the other nodes, as GDS does (measured).
//! * **093** (parallel): synchronous peeling. A round removes every node
//!   whose remaining degree is at most the level `k`, found by blocks,
//!   and lowers its neighbours' degrees by the number of edges to removed
//!   nodes (integers, so the merge order does not matter); when no node
//!   is left at level `k` the level rises to the smallest remaining
//!   degree. A node's core value is the level at which it is removed.
//!   Degrees are those of the undirected projection, parallel edges
//!   counted and a self-loop twice, as GDS's (`kcore/KCoreDecomposition
//!   .java`, measured on zipf60 and uniform60, rmp #67). GDS removes a
//!   level's cascade inside one parallel phase with atomic counters;
//!   rustworkx-core `connectivity/core_number.rs` and NetworKit
//!   `centrality/CoreDecomposition.cpp` peel in bucket order; the rounds
//!   here give the same core values at every `concurrency`.

use std::collections::BTreeMap;
use std::sync::Arc;

use petgraph::unionfind::UnionFind;

use super::config::{Def, Key, NODE_WRITE, Ty};
use super::kernels::{self, id_value};
use super::projection::{Ix, Projection, with_projection};
use super::{Entry, NODE_WRITE_COLUMNS, Output, Run, WriteKind};
use crate::error::Error;
use crate::types::Value;

const WEIGHT: Key = Key::new("relationshipWeightProperty", Ty::Str, Def::Absent);
const CONSECUTIVE: Key = Key::new("consecutiveIds", Ty::Bool, Def::Bool(false));

/// GDS's ComponentWrite columns (`algorithm-entries.md`).
const COMPONENT_WRITE: &[&str] = &[
    "componentCount",
    "componentDistribution",
    "preProcessingMillis",
    "computeMillis",
    "postProcessingMillis",
    "writeMillis",
    "nodePropertiesWritten",
    "configuration",
];

const COMPONENT_ROWS: &[&str] = &["nodeId", "componentId"];

/// ALG-083: `gds.wcc.stream`, `.write`.
pub(crate) const WCC: Entry = Entry {
    algs: &[83],
    stream: "gds.wcc.stream",
    write: Some("gds.wcc.write"),
    keys: &[
        Key::new("seedProperty", Ty::Str, Def::Absent),
        Key::new(
            "threshold",
            Ty::Float {
                min: f64::NEG_INFINITY,
                max: f64::INFINITY,
                max_open: false,
            },
            Def::Float(0.0),
        ),
        CONSECUTIVE,
        Key::new("forceSeedOptimization", Ty::Bool, Def::Bool(false)),
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["seedProperty"],
    stream_columns: COMPONENT_ROWS,
    write_columns: COMPONENT_WRITE,
    run: run_wcc,
    ..Entry::BASE
};

/// ALG-084: `gds.connectedComponents.stream`, `.write`.
pub(crate) const CONNECTED: Entry = Entry {
    algs: &[84],
    stream: "gds.connectedComponents.stream",
    write: Some("gds.connectedComponents.write"),
    undirected: true,
    stream_columns: COMPONENT_ROWS,
    write_columns: NODE_WRITE_COLUMNS,
    run: run_connected,
    ..Entry::BASE
};

/// ALG-085: `gds.scc.stream`, `.write`.
pub(crate) const SCC: Entry = Entry {
    algs: &[85],
    stream: "gds.scc.stream",
    write: Some("gds.scc.write"),
    keys: &[CONSECUTIVE],
    stream_columns: COMPONENT_ROWS,
    write_columns: COMPONENT_WRITE,
    run: run_scc,
    ..Entry::BASE
};

/// ALG-086: `gds.condensation.stream`, `.write`.
pub(crate) const CONDENSATION: Entry = Entry {
    algs: &[86],
    stream: "gds.condensation.stream",
    write: Some("gds.condensation.write"),
    stream_columns: &["componentId", "nodeIds", "successorComponentIds"],
    write_columns: NODE_WRITE_COLUMNS,
    run: run_condensation,
    write_keys: NODE_WRITE,
    write_kind: WriteKind::NodeList { ids: 1, value: 0 },
    ..Entry::BASE
};

/// ALG-089: `biconnected_components.get` (MAGE; no write mode).
pub(crate) const BICONNECTED: Entry = Entry {
    algs: &[89],
    stream: "biconnected_components.get",
    undirected: true,
    stream_columns: &["bcc_id", "node_from", "node_to"],
    run: run_biconnected,
    ..Entry::BASE
};

/// ALG-090: `gds.articulationPoints.stream`, `.write`.
pub(crate) const ARTICULATION_POINTS: Entry = Entry {
    algs: &[90],
    stream: "gds.articulationPoints.stream",
    write: Some("gds.articulationPoints.write"),
    undirected: true,
    stream_columns: &["nodeId", "resultingComponents"],
    write_columns: &[
        "articulationPointCount",
        "nodePropertiesWritten",
        "writeMillis",
        "computeMillis",
        "configuration",
    ],
    run: run_articulation_points,
    ..Entry::BASE
};

/// ALG-091: `gds.bridges.stream`.
pub(crate) const BRIDGES: Entry = Entry {
    algs: &[91],
    stream: "gds.bridges.stream",
    undirected: true,
    stream_columns: &["from", "to", "remainingSizes"],
    run: run_bridges,
    ..Entry::BASE
};

/// ALG-093: `gds.kcore.stream`, `.write`.
pub(crate) const KCORE: Entry = Entry {
    algs: &[93],
    stream: "gds.kcore.stream",
    write: Some("gds.kcore.write"),
    parallel: true,
    undirected: true,
    stream_columns: &["nodeId", "coreValue"],
    write_columns: &[
        "nodePropertiesWritten",
        "degeneracy",
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "writeMillis",
        "configuration",
    ],
    run: run_kcore,
    ..Entry::BASE
};

/// `componentDistribution`: the component sizes' `min`, `max`, `mean`
/// and nearest-rank percentiles, with the keys GDS reports (measured).
fn component_distribution(ids: &[i64]) -> Value {
    let mut counts: BTreeMap<i64, i64> = BTreeMap::new();
    for c in ids {
        *counts.entry(*c).or_default() += 1;
    }
    let mut xs: Vec<i64> = counts.into_values().collect();
    if xs.is_empty() {
        return Value::Record(Vec::new().into());
    }
    xs.sort_unstable();
    let at = |q: f64| {
        let rank = (q / 100.0 * xs.len() as f64).ceil() as usize;
        Value::Int(xs[rank.clamp(1, xs.len()) - 1])
    };
    let f = |k: &str, v: Value| (Arc::<str>::from(k), v);
    let mut fields = vec![
        f("min", Value::Int(xs[0])),
        f("max", Value::Int(xs[xs.len() - 1])),
        f(
            "mean",
            Value::Float(xs.iter().sum::<i64>() as f64 / xs.len() as f64),
        ),
    ];
    for (k, q) in [
        ("p1", 1.0),
        ("p5", 5.0),
        ("p10", 10.0),
        ("p25", 25.0),
        ("p50", 50.0),
        ("p75", 75.0),
        ("p90", 90.0),
        ("p95", 95.0),
        ("p99", 99.0),
        ("p999", 99.9),
    ] {
        fields.push(f(k, at(q)));
    }
    Value::Record(fields.into())
}

/// Identifiers numbered 0, 1, … in the order of their first node.
fn consecutive(ids: &[i64]) -> Vec<i64> {
    let mut map: BTreeMap<i64, i64> = BTreeMap::new();
    ids.iter()
        .map(|c| {
            let next = map.len() as i64;
            *map.entry(*c).or_insert(next)
        })
        .collect()
}

/// The `(nodeId, componentId)` rows of `ids` and the ComponentWrite
/// summary values.
fn component_rows<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    ids: &[i64],
) -> Result<Output, Error> {
    let ids = if r.cfg.has("consecutiveIds") && r.cfg.bool("consecutiveIds") {
        consecutive(ids)
    } else {
        ids.to_vec()
    };
    let mut rows = Vec::new();
    for (v, c) in ids.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), Value::Int(*c)])?;
    }
    let mut distinct = ids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("componentCount", Value::Int(distinct.len() as i64)),
            ("componentDistribution", component_distribution(&ids)),
        ],
    })
}

fn run_wcc(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => wcc(p, r))
}

/// The labels of ALG-083's rounds (module documentation) and the number
/// of rounds.
pub(crate) fn wcc_labels<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    threshold: Option<f64>,
) -> Result<(Vec<usize>, usize), Error> {
    let n = p.node_count();
    let mut labels = r.mem.vec(n, 0usize)?;
    for (v, l) in labels.iter_mut().enumerate() {
        *l = v;
    }
    let mut next = r.mem.vec(n, 0usize)?;
    let keep = |slot: usize| match threshold {
        None => true,
        Some(t) => {
            let w = p.edge_prop(0, slot);
            w.is_nan() || w > t
        }
    };
    let mut rounds = 0;
    loop {
        rounds += 1;
        let prev = &labels;
        let changed = r.sched.fill(&mut next, |_, range, out| {
            let mut changed = false;
            for (o, v) in out.iter_mut().zip(range) {
                let mut m = prev[v];
                for i in p.fwd.row[v]..p.fwd.row[v + 1] {
                    if keep(i) {
                        m = m.min(prev[p.fwd.col[i].index()]);
                    }
                }
                for j in p.rev.row[v]..p.rev.row[v + 1] {
                    if keep(p.rev_slot[j]) {
                        m = m.min(prev[p.rev.col[j].index()]);
                    }
                }
                changed |= m != prev[v];
                *o = m;
            }
            changed
        })?;
        let mid = &next;
        let jumped = r.sched.fill(&mut labels, |_, range, out| {
            let mut changed = false;
            for (o, v) in out.iter_mut().zip(range) {
                let j = mid[mid[v]];
                changed |= j != mid[v];
                *o = j;
            }
            changed
        })?;
        trace!(
            "round",
            "{rounds} min {:?} jump {:?}",
            &next[..],
            &labels[..]
        );
        if !changed.contains(&true) && !jumped.contains(&true) {
            break;
        }
        r.mem.check_now()?;
    }
    Ok((labels, rounds))
}

fn wcc<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let weighted = r.cfg.has("relationshipWeightProperty");
    let threshold = weighted.then(|| r.cfg.float("threshold"));
    let (labels, rounds) = wcc_labels(p, r, threshold)?;
    #[cfg(test)]
    if let Some(k) = &r.sched.probe {
        super::sched::probe::record_rounds(k, "wcc", rounds);
    }
    let _ = rounds;
    let ids: Vec<i64> = if r.cfg.has("seedProperty") {
        let seed = |v: usize| {
            let x = p.node_props[0][v];
            (x.is_finite() && x >= 0.0).then_some(x as i64)
        };
        let max = (0..p.node_count()).filter_map(seed).max().unwrap_or(-1);
        let mut by_root: BTreeMap<usize, Option<i64>> = BTreeMap::new();
        for (v, l) in labels.iter().enumerate() {
            let e = by_root.entry(*l).or_insert(None);
            if let Some(s) = seed(v) {
                *e = Some(e.map_or(s, |c| c.min(s)));
            }
        }
        let mut fresh = max;
        let ids: BTreeMap<usize, i64> = by_root
            .into_iter()
            .map(|(root, s)| {
                (
                    root,
                    s.unwrap_or_else(|| {
                        fresh += 1;
                        fresh
                    }),
                )
            })
            .collect();
        labels.iter().map(|l| ids[l]).collect()
    } else {
        labels.iter().map(|l| *l as i64).collect()
    };
    component_rows(p, r, &ids)
}

fn run_connected(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => connected(p, r))
}

fn connected<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem.grow(3 * n * size_of::<usize>())?;
    let mut uf = UnionFind::<usize>::new(n);
    for v in 0..n {
        for (w, _) in p.out(v) {
            let _merged = uf.union(v, w.index());
            trace!("union", "{v} {} {_merged}", w.index());
        }
    }
    let rep = uf.into_labeling();
    trace!("label", "{rep:?}");
    let mut smallest = vec![usize::MAX; n];
    for (v, c) in rep.iter().enumerate() {
        smallest[*c] = smallest[*c].min(v);
    }
    let ids: Vec<i64> = rep.iter().map(|c| smallest[*c] as i64).collect();
    let mut rows = Vec::new();
    for (v, c) in ids.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), Value::Int(*c)])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// The strongly connected components of the projection, each labelled
/// by its smallest node index, and the components themselves.
fn scc_labels<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Vec<Vec<usize>>, Error> {
    let n = p.node_count();
    r.mem.grow(6 * n * size_of::<usize>())?;
    let mut comps: Vec<Vec<usize>> = petgraph::algo::tarjan_scc(p)
        .into_iter()
        .map(|c| {
            let mut c: Vec<usize> = c.into_iter().map(|v| v.index()).collect();
            trace!("scc", "{c:?}");
            c.sort_unstable();
            c
        })
        .collect();
    comps.sort_unstable_by_key(|c| c[0]);
    Ok(comps)
}

fn run_scc(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => scc(p, r))
}

fn scc<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let comps = scc_labels(p, r)?;
    let mut ids = vec![0i64; p.node_count()];
    for c in &comps {
        for v in c {
            ids[*v] = c[0] as i64;
        }
    }
    component_rows(p, r, &ids)
}

fn run_condensation(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => condensation(p, r))
}

fn condensation<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let comps = scc_labels(p, r)?;
    let n = p.node_count();
    let mut of = vec![0usize; n];
    for (i, c) in comps.iter().enumerate() {
        for v in c {
            of[*v] = i;
        }
    }
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); comps.len()];
    for v in 0..n {
        for (w, _) in p.out(v) {
            let (a, b) = (of[v], of[w.index()]);
            if a != b {
                succ[a].push(b);
            }
        }
    }
    let mut rows = Vec::new();
    for (i, c) in comps.iter().enumerate() {
        let s = &mut succ[i];
        s.sort_unstable();
        s.dedup();
        trace!("succ", "{i} {s:?}");
        let row = vec![
            Value::Int(i as i64),
            Value::List(c.iter().map(|v| id_value(p, *v)).collect::<Vec<_>>().into()),
            Value::List(
                s.iter()
                    .map(|x| Value::Int(*x as i64))
                    .collect::<Vec<_>>()
                    .into(),
            ),
        ];
        r.emit(&mut rows, row)?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_biconnected(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => biconnected(p, r))
}

/// The edge sets of the biconnected components of the simple graph `s`,
/// in the order the search completes them (module documentation).
pub(crate) fn biconnected_edges(s: &kernels::Simple) -> Vec<Vec<(usize, usize)>> {
    const NONE: usize = usize::MAX;
    let n = s.len();
    let mut disc = vec![NONE; n];
    let mut low = vec![0usize; n];
    let mut parent = vec![NONE; n];
    let mut time = 0;
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut out = Vec::new();
    for root in 0..n {
        if disc[root] != NONE {
            continue;
        }
        disc[root] = time;
        low[root] = time;
        time += 1;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&w) = s.of(v).get(*next) {
                *next += 1;
                if w == parent[v] {
                    continue;
                }
                if disc[w] == NONE {
                    parent[w] = v;
                    disc[w] = time;
                    low[w] = time;
                    time += 1;
                    trace!("tree", "{v} {w}");
                    edges.push((v, w));
                    stack.push((w, 0));
                } else if disc[w] < disc[v] {
                    low[v] = low[v].min(disc[w]);
                    trace!("back", "{v} {w}");
                    edges.push((v, w));
                }
            } else {
                stack.pop();
                let p = parent[v];
                if p != NONE {
                    low[p] = low[p].min(low[v]);
                    if low[v] >= disc[p] {
                        let mut comp = Vec::new();
                        while let Some(e) = edges.pop() {
                            comp.push(e);
                            if e == (p, v) {
                                break;
                            }
                        }
                        comp.reverse();
                        trace!("comp", "{comp:?}");
                        out.push(comp);
                    }
                }
            }
        }
    }
    out
}

fn biconnected<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let s = kernels::simple(p, r.mem)?;
    let n = p.node_count();
    r.mem.grow(4 * n * size_of::<usize>() + s.col.len() * 16)?;
    let comps = biconnected_edges(&s);
    let mut rows = Vec::new();
    for (i, comp) in comps.iter().enumerate() {
        for (a, b) in comp {
            r.emit(
                &mut rows,
                vec![
                    Value::Int(i as i64),
                    Value::Node(p.ids[*a]),
                    Value::Node(p.ids[*b]),
                ],
            )?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// GDS's low-link depth-first search over the undirected projection
/// (module documentation): entry time, low value, parent, subtree size
/// and the root of each node's search tree.
pub(crate) struct LowLink {
    pub(crate) tin: Vec<usize>,
    pub(crate) low: Vec<usize>,
    pub(crate) parent: Vec<usize>,
    pub(crate) size: Vec<usize>,
    pub(crate) root: Vec<usize>,
}

/// [`LowLink`] of `p`: the first edge from a node back to its parent is
/// the tree edge; every other edge to a visited node is a back edge.
pub(crate) fn low_link<I: Ix>(p: &Projection<I>) -> LowLink {
    const NONE: usize = usize::MAX;
    let n = p.node_count();
    let mut ll = LowLink {
        tin: vec![NONE; n],
        low: vec![NONE; n],
        parent: vec![NONE; n],
        size: vec![1; n],
        root: vec![NONE; n],
    };
    let adj: Vec<Vec<usize>> = (0..n)
        .map(|v| {
            let mut t: Vec<usize> = p.out(v).map(|(w, _)| w.index()).collect();
            t.sort_unstable();
            t
        })
        .collect();
    let mut time = 0;
    for start in 0..n {
        if ll.tin[start] != NONE {
            continue;
        }
        ll.tin[start] = time;
        ll.low[start] = time;
        ll.root[start] = start;
        time += 1;
        // (node, next neighbour position, parent edge skipped)
        let mut stack: Vec<(usize, usize, bool)> = vec![(start, 0, false)];
        while let Some(&mut (v, ref mut next, ref mut skipped)) = stack.last_mut() {
            if let Some(&w) = adj[v].get(*next) {
                *next += 1;
                if w == ll.parent[v] && !*skipped {
                    *skipped = true;
                    trace!("skip", "{v} {w}");
                    continue;
                }
                if ll.tin[w] == NONE {
                    ll.parent[w] = v;
                    ll.tin[w] = time;
                    ll.low[w] = time;
                    ll.root[w] = start;
                    time += 1;
                    trace!("tree", "{v} {w}");
                    stack.push((w, 0, false));
                } else {
                    ll.low[v] = ll.low[v].min(ll.tin[w]);
                    trace!("back", "{v} {w}");
                }
            } else {
                stack.pop();
                trace!("done", "{v} low {} size {}", ll.low[v], ll.size[v]);
                let pa = ll.parent[v];
                if pa != NONE {
                    ll.low[pa] = ll.low[pa].min(ll.low[v]);
                    ll.size[pa] += ll.size[v];
                }
            }
        }
    }
    ll
}

fn run_articulation_points(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => articulation_points(p, r))
}

fn articulation_points<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem
        .grow(6 * n * size_of::<usize>() + p.edge_ids.len() * 2 * size_of::<usize>())?;
    let ll = low_link(p);
    // The pieces each node's removal leaves.
    let mut pieces: Vec<Vec<usize>> = vec![Vec::new(); n];
    for c in 0..n {
        let v = ll.parent[c];
        if v == usize::MAX {
            continue;
        }
        if ll.root[v] == v || ll.low[c] >= ll.tin[v] {
            pieces[v].push(ll.size[c]);
        }
    }
    let mut rows = Vec::new();
    let mut count = 0i64;
    for (v, piece) in pieces.iter_mut().enumerate() {
        let is_root = ll.root[v] == v;
        let mut sizes = std::mem::take(piece);
        let ap = if is_root {
            sizes.len() > 1
        } else {
            !sizes.is_empty()
        };
        if !ap {
            if r.mode == super::AlgoMode::Write {
                r.emit(&mut rows, vec![id_value(p, v), Value::Int(0)])?;
            }
            continue;
        }
        count += 1;
        if !is_root {
            let rest = ll.size[ll.root[v]] - 1 - sizes.iter().sum::<usize>();
            if rest > 0 {
                sizes.push(rest);
            }
        }
        let value = if r.mode == super::AlgoMode::Write {
            Value::Int(1)
        } else {
            let f = |k: &str, x: usize| (Arc::<str>::from(k), Value::Int(x as i64));
            Value::Record(
                vec![
                    f("count", sizes.len()),
                    f("max", sizes.iter().copied().max().unwrap_or(0)),
                    f("min", sizes.iter().copied().min().unwrap_or(0)),
                ]
                .into(),
            )
        };
        r.emit(&mut rows, vec![id_value(p, v), value])?;
    }
    Ok(Output::Rows {
        rows,
        extra: vec![("articulationPointCount", Value::Int(count))],
    })
}

fn run_bridges(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => bridges(p, r))
}

fn bridges<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem
        .grow(6 * n * size_of::<usize>() + p.edge_ids.len() * 2 * size_of::<usize>())?;
    let ll = low_link(p);
    let mut rows = Vec::new();
    for c in 0..n {
        let v = ll.parent[c];
        if v == usize::MAX || ll.low[c] <= ll.tin[v] {
            continue;
        }
        let total = ll.size[ll.root[c]];
        let sizes = vec![
            Value::Int((total - ll.size[c]) as i64),
            Value::Int(ll.size[c] as i64),
        ];
        r.emit(
            &mut rows,
            vec![id_value(p, v), id_value(p, c), Value::List(sizes.into())],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_kcore(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => kcore(p, r))
}

/// ALG-093's core values (module documentation) and its number of
/// peeling rounds.
pub(crate) fn core_values<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
) -> Result<(Vec<usize>, usize), Error> {
    const NONE: usize = usize::MAX;
    let n = p.node_count();
    let mut degree = r.mem.vec(n, 0usize)?;
    r.sched.fill(&mut degree, |_, range, out| {
        for (d, v) in out.iter_mut().zip(range) {
            *d = p.out_degree(v);
        }
    })?;
    let mut core = r.mem.vec(n, NONE)?;
    r.mem.grow(2 * n * size_of::<usize>())?;
    let mut remaining = n;
    let mut k = 0usize;
    let mut rounds = 0;
    while remaining > 0 {
        let (deg, cr) = (&degree, &core);
        let removed: Vec<usize> = r
            .sched
            .map(|_, range| {
                range
                    .filter(|v| cr[*v] == NONE && deg[*v] <= k)
                    .collect::<Vec<usize>>()
            })?
            .into_iter()
            .flatten()
            .collect();
        if removed.is_empty() {
            k = r
                .sched
                .map(|_, range| {
                    range
                        .filter(|v| cr[*v] == NONE)
                        .map(|v| deg[v])
                        .min()
                        .unwrap_or(usize::MAX)
                })?
                .into_iter()
                .min()
                .unwrap_or(k);
            trace!("level", "{k}");
            continue;
        }
        rounds += 1;
        trace!("peel", "{k} {removed:?}");
        for v in &removed {
            core[*v] = k;
        }
        remaining -= removed.len();
        let cr = &core;
        let lowered = r.sched.map(|_, range| {
            let lo = removed.partition_point(|v| *v < range.start);
            let hi = removed.partition_point(|v| *v < range.end);
            let mut out = Vec::new();
            for v in &removed[lo..hi] {
                for (w, _) in p.out(*v) {
                    let w = w.index();
                    if cr[w] == NONE {
                        out.push(w);
                    }
                }
            }
            out
        })?;
        for w in lowered.into_iter().flatten() {
            degree[w] -= 1;
        }
        r.mem.check_now()?;
    }
    Ok((core, rounds))
}

fn kcore<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let (core, rounds) = core_values(p, r)?;
    #[cfg(test)]
    if let Some(k) = &r.sched.probe {
        super::sched::probe::record_rounds(k, "kcore", rounds);
    }
    let _ = rounds;
    let mut rows = Vec::new();
    for (v, c) in core.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), Value::Int(*c as i64)])?;
    }
    let degeneracy = core.iter().copied().max().unwrap_or(0);
    Ok(Output::Rows {
        rows,
        extra: vec![("degeneracy", Value::Int(degeneracy as i64))],
    })
}
