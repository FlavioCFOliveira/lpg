//! The matching, covering, cliques and colouring family (rmp #68):
//! ALG-161 maximum and maximal matching, ALG-163 bipartite matching,
//! ALG-169 maximal cliques, ALG-171 clique counting, ALG-173 greedy
//! colouring, ALG-174 DSatur colouring, ALG-176 K-1 colouring, ALG-177
//! greedy feedback arc set, ALG-178 set cover. 171 and 176 are parallel;
//! the others run on one worker. The undirected entries read the simple
//! graph ([`super::kernels::Simple`]): parallel edges once, no
//! self-loops.
//!
//! * **161** calls petgraph 0.8.3's `maximum_matching` (Gabow's
//!   algorithm, maximum cardinality) or, with `greedy`, `greedy_matching`
//!   (`matching.rs:373`, `:211`) on a petgraph `UnGraph` of the simple
//!   graph; a self-loop would let a node match itself. Rows: each matched
//!   pair, the smaller node first, in node order.
//! * **163** ports MAGE @ `c72636014a8b`
//!   `cpp/bipartite_matching_module/algorithm/bipartite_matching.cpp`
//!   (Apache-2.0, see `LICENSE-APACHE`; Memgraph Ltd., no copyright line
//!   in the file): a two-colouring by breadth-first search (a self-loop or
//!   an odd cycle gives 0, as MAGE returns), then Kuhn's augmenting paths
//!   from each node of colour 0. Modified: the depth-first augmenting
//!   search runs on an explicit stack; sides are the colours, not MAGE's
//!   renumbered edge ends.
//! * **169** copies petgraph's `maximal_cliques` (`maximal_cliques.rs`,
//!   Bron-Kerbosch with the pivot of most neighbours; MIT, licence text in
//!   [`super::projection`]). Modified: sorted vectors instead of hash
//!   sets, so the order of the cliques is fixed (the pivot is the first
//!   node of most neighbours); each clique is a row charged as it is
//!   found (TS-25 exponential outputs); no clique for the empty
//!   projection, as NetworkX's `find_cliques` (petgraph returns the empty
//!   clique; rmp #71).
//! * **171** (parallel; no permissive source, written here) counts, for
//!   each node `v`, the cliques of `k − 1` nodes among its neighbours, by
//!   enumerating the cliques of increasing node index; each block counts
//!   its own nodes, so no count is shared. `counts[i]` is the number of
//!   cliques of `i + 3` nodes holding the node, trailing zeros left out,
//!   and `globalCount[i]` the number of cliques of `i + 3` nodes, as GDS
//!   2026.09.0 reports them (measured on karate: `[45, 11, 2]`). GDS fails
//!   on a projection with self-loops or parallel edges (measured:
//!   `ArrayIndexOutOfBoundsException`, `NegativeArraySizeException`); lpg
//!   counts the cliques of its simple graph.
//! * **173** ports NetworkX's `greedy_color` strategies
//!   (`algorithms/coloring/greedy_coloring.py`, BSD-3-Clause, "Copyright
//!   (c) 2004-2024, NetworkX Developers", licence text below): nodes in the
//!   strategy's order each take the smallest colour no coloured neighbour
//!   has. Modified: where NetworkX takes an arbitrary element of a set,
//!   lpg takes the smallest index; `random_sequential` shuffles with the
//!   run's generator (TS-25 Randomness), so its order differs from
//!   NetworkX's. Under MAGE's name and columns; a `parameters` key other
//!   than `strategy` (MAGE's metaheuristic keys) fails with 22N11 (TS-25
//!   item B); `edge_property` has no effect.
//! * **174** calls petgraph's `dsatur_coloring` (`coloring.rs:63`) on the
//!   undirected projection.
//! * **176** (parallel; written here): GDS's `k1coloring/` (studied)
//!   colours speculatively and repairs conflicts, so its result depends
//!   on thread timing. lpg colours by Jones and Plassmann's rounds with
//!   the largest-degree-first priority (degree in the simple graph, then
//!   the smaller index): in a round, by blocks reading the last round,
//!   each uncoloured node all of whose higher-priority neighbours are
//!   coloured takes the smallest colour none of its coloured neighbours
//!   has; two such nodes are never adjacent, so no conflict arises, and
//!   the colouring equals the sequential greedy colouring in priority
//!   order at every `concurrency`. Both directions of every edge count.
//!   `ranIterations` is 1 (one colouring pass, no repair) and
//!   `didConverge` true; `maxIterations` and `batchSize` have no effect;
//!   `minCommunitySize` keeps the rows of the colours with at least that
//!   many nodes. References: LAGraph @ `7543de1f3103`
//!   `LAGraph_coloring_independent_set.c` (BSD-2-Clause, method studied)
//!   colours independent sets by rounds. Measured on the fixtures (rmp
//!   #68): lpg uses 5, 5, 4 and 7 colours on karate, zipf60, uniform60 and
//!   zipf300, GDS at `concurrency` 1 uses 6, 6, 5 to 6 and 11.
//! * **177** calls petgraph's `greedy_feedback_arc_set` (Eades, Lin and
//!   Smyth; `feedback_arc_set.rs:73`) on a petgraph `DiGraph` of the
//!   projection's edges in the run's orientation, in forward order; the
//!   projection does not satisfy the function's `GraphIndex` bound, so it
//!   is not called on the projection directly.
//! * **178** is the greedy set cover: MAGE's "greedy" solver
//!   (`python/mage/constraint_programming/solver.py:119-141`) takes the
//!   sets in a random order until the elements are covered, so its result
//!   is not reproducible; lpg takes, while an element is uncovered, the
//!   set covering the most uncovered elements (ties: first listed), the
//!   classic greedy algorithm. `element_vertexes[i]` belongs to
//!   `set_vertexes[i]`, as in MAGE (`python/set_cover.py`).
//!
//! NetworkX's licence (`LICENSE.txt`, BSD-3-Clause):
//!
//! > Copyright (c) 2004-2024, NetworkX Developers
//! > Aric Hagberg <hagberg@lanl.gov>
//! > Dan Schult <dschult@colgate.edu>
//! > Pieter Swart <swart@lanl.gov>
//! > All rights reserved.
//! >
//! > Redistribution and use in source and binary forms, with or without
//! > modification, are permitted provided that the following conditions
//! > are met:
//! >
//! > * Redistributions of source code must retain the above copyright
//! >   notice, this list of conditions and the following disclaimer.
//! > * Redistributions in binary form must reproduce the above copyright
//! >   notice, this list of conditions and the following disclaimer in the
//! >   documentation and/or other materials provided with the
//! >   distribution.
//! > * Neither the name of the NetworkX Developers nor the names of its
//! >   contributors may be used to endorse or promote products derived
//! >   from this software without specific prior written permission.
//! >
//! > THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
//! > "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
//! > LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
//! > A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
//! > OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//! > SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
//! > LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
//! > DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
//! > THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
//! > (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
//! > OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use petgraph::graph::{DiGraph, NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use rand::RngExt;

use super::config::{Def, Key, NODE_WRITE, REL_WRITE, Ty};
use super::kernels::{self, Simple, id_value};
use super::projection::{Ix, Projection, with_projection};
use super::sched::rng;
use super::{
    Arg, ArgDefault, EDGE_PROP_WRITE_COLUMNS, Entry, Output, REL_WRITE_COLUMNS, Run, WriteKind,
};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// ALG-161: `gds.maximumMatching.stream`, `.write`.
pub(crate) const MAXIMUM_MATCHING: Entry = Entry {
    algs: &[161],
    stream: "gds.maximumMatching.stream",
    write: Some("gds.maximumMatching.write"),
    keys: &[Key::new("greedy", Ty::Bool, Def::Bool(false))],
    undirected: true,
    stream_columns: &["node1", "node2"],
    write_columns: REL_WRITE_COLUMNS,
    run: run_maximum_matching,
    write_keys: REL_WRITE,
    write_kind: WriteKind::Rels { src: 0, dst: 1 },
    ..Entry::BASE
};

/// ALG-163: `bipartite_matching.max` (MAGE; no write mode).
pub(crate) const BIPARTITE_MATCHING: Entry = Entry {
    algs: &[163],
    stream: "bipartite_matching.max",
    undirected: true,
    stream_columns: &["maximum_bipartite_matching"],
    run: run_bipartite_matching,
    ..Entry::BASE
};

/// ALG-169: `gds.maximalCliques.stream`.
pub(crate) const MAXIMAL_CLIQUES: Entry = Entry {
    algs: &[169],
    stream: "gds.maximalCliques.stream",
    undirected: true,
    stream_columns: &["cliqueId", "nodeIds"],
    run: run_maximal_cliques,
    ..Entry::BASE
};

/// ALG-171: `gds.cliqueCounting.stream`, `.write`.
pub(crate) const CLIQUE_COUNTING: Entry = Entry {
    algs: &[171],
    stream: "gds.cliqueCounting.stream",
    write: Some("gds.cliqueCounting.write"),
    parallel: true,
    undirected: true,
    stream_columns: &["nodeId", "counts"],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "globalCount",
        "configuration",
    ],
    run: run_clique_counting,
    ..Entry::BASE
};

/// ALG-173: `graph_coloring.color_graph` (MAGE name; no write mode).
pub(crate) const GREEDY_COLORING: Entry = Entry {
    algs: &[173],
    stream: "graph_coloring.color_graph",
    randomised: true,
    undirected: true,
    stream_columns: &["node", "color"],
    run: run_greedy_coloring,
    args: &[
        Arg::new("parameters", ArgDefault::Null),
        Arg::new("edge_property", ArgDefault::Str("weight")),
    ],
    ..Entry::BASE
};

/// ALG-174: `gds.dsaturColoring.stream`, `.write`.
pub(crate) const DSATUR: Entry = Entry {
    algs: &[174],
    stream: "gds.dsaturColoring.stream",
    write: Some("gds.dsaturColoring.write"),
    undirected: true,
    stream_columns: &["nodeId", "color"],
    write_columns: &["nodePropertiesWritten", "colorCount", "configuration"],
    run: run_dsatur,
    ..Entry::BASE
};

/// ALG-176: `gds.k1coloring.stream`, `.write`.
pub(crate) const K1_COLORING: Entry = Entry {
    algs: &[176],
    stream: "gds.k1coloring.stream",
    write: Some("gds.k1coloring.write"),
    keys: &[
        Key::new("maxIterations", Ty::Int { min: 1 }, Def::Int(10)),
        Key::new("minCommunitySize", Ty::Int { min: 0 }, Def::Absent),
        Key::new("batchSize", Ty::Int { min: 1 }, Def::Int(10_000)),
    ],
    parallel: true,
    stream_columns: &["nodeId", "color"],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "nodeCount",
        "colorCount",
        "ranIterations",
        "didConverge",
        "configuration",
    ],
    run: run_k1_coloring,
    ..Entry::BASE
};

/// ALG-177: `gds.greedyFeedbackArcSet.stream`, `.write`.
pub(crate) const FEEDBACK_ARC_SET: Entry = Entry {
    algs: &[177],
    stream: "gds.greedyFeedbackArcSet.stream",
    write: Some("gds.greedyFeedbackArcSet.write"),
    stream_columns: &["sourceNode", "targetNode", "relationshipId"],
    write_columns: EDGE_PROP_WRITE_COLUMNS,
    run: run_feedback_arc_set,
    write_keys: NODE_WRITE,
    write_kind: WriteKind::EdgeFlag { edge: 2 },
    ..Entry::BASE
};

/// ALG-178: `set_cover.greedy` (MAGE; no write mode).
pub(crate) const SET_COVER: Entry = Entry {
    algs: &[178],
    stream: "set_cover.greedy",
    stream_columns: &["containing_set"],
    run: run_set_cover,
    args: &[
        Arg::new("element_vertexes", ArgDefault::Required),
        Arg::new("set_vertexes", ArgDefault::Required),
    ],
    ..Entry::BASE
};

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// A petgraph `UnGraph` of the simple graph `s`.
fn ungraph(s: &Simple) -> UnGraph<(), ()> {
    let n = s.len();
    let mut g = UnGraph::with_capacity(n, s.col.len() / 2);
    for _ in 0..n {
        g.add_node(());
    }
    for v in 0..n {
        for w in s.of(v) {
            if v < *w {
                g.add_edge(NodeIndex::new(v), NodeIndex::new(*w), ());
            }
        }
    }
    g
}

/// The colour count of a colouring.
fn color_count(colors: &[usize]) -> usize {
    colors.iter().collect::<BTreeSet<_>>().len()
}

fn run_maximum_matching(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => maximum_matching(p, r))
}

fn maximum_matching<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let s = kernels::simple(p, r.mem)?;
    let n = p.node_count();
    r.mem.grow(n * 64 + s.col.len() * 48)?;
    let g = ungraph(&s);
    trace!(
        "graph",
        "{:?}",
        g.edge_references()
            .map(|e| (e.source().index(), e.target().index()))
            .collect::<Vec<_>>()
    );
    let m = if r.cfg.bool("greedy") {
        petgraph::algo::greedy_matching(&g)
    } else {
        petgraph::algo::maximum_matching(&g)
    };
    trace!(
        "mate",
        "{:?}",
        (0..n)
            .map(|v| m.mate(NodeIndex::new(v)).map(|w| w.index()))
            .collect::<Vec<_>>()
    );
    let mut rows = Vec::new();
    for v in 0..n {
        if let Some(w) = m.mate(NodeIndex::new(v))
            && v < w.index()
        {
            r.emit(&mut rows, vec![id_value(p, v), id_value(p, w.index())])?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_bipartite_matching(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => bipartite_matching(p, r))
}

/// MAGE's maximum bipartite matching size (module documentation).
pub(crate) fn bipartite_matching_size(s: &Simple, looped: bool) -> usize {
    const NONE: usize = usize::MAX;
    let n = s.len();
    if looped {
        return 0;
    }
    let mut color = vec![NONE; n];
    for start in 0..n {
        if color[start] != NONE {
            continue;
        }
        color[start] = 1;
        let mut queue = VecDeque::from([start]);
        while let Some(v) = queue.pop_front() {
            for w in s.of(v) {
                if color[*w] == color[v] {
                    return 0;
                }
                if color[*w] == NONE {
                    color[*w] = 1 - color[v];
                    queue.push_back(*w);
                }
            }
        }
    }
    trace!("colour", "{color:?}");
    // Kuhn's augmenting paths from each node of colour 0.
    let mut matched = vec![NONE; n];
    let mut size = 0;
    let mut visited = vec![false; n];
    for u in (0..n).filter(|v| color[*v] == 0) {
        visited.fill(false);
        visited[u] = true;
        // (left node, next neighbour position); `via[x]` the right node
        // through which left node `x` was reached.
        let mut stack: Vec<(usize, usize)> = vec![(u, 0)];
        let mut via: Vec<usize> = Vec::new();
        let mut found = None;
        while let Some(&mut (x, ref mut next)) = stack.last_mut() {
            let Some(&y) = s.of(x).get(*next) else {
                stack.pop();
                via.pop();
                continue;
            };
            *next += 1;
            if matched[y] == NONE {
                found = Some(y);
                break;
            }
            let z = matched[y];
            if !visited[z] {
                visited[z] = true;
                trace!("visit", "{u} {x} {y} {z}");
                via.push(y);
                stack.push((z, 0));
            }
        }
        if let Some(mut y) = found {
            // Flip the path: each left node on the stack takes the right
            // node after it.
            for i in (0..stack.len()).rev() {
                matched[y] = stack[i].0;
                if i > 0 {
                    y = via[i - 1];
                }
            }
            size += 1;
        }
        trace!("search", "{u} {found:?}");
    }
    size
}

fn bipartite_matching<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let s = kernels::simple(p, r.mem)?;
    let n = p.node_count();
    r.mem.grow(n * 6 * size_of::<usize>())?;
    let looped = (0..n).any(|v| p.out(v).any(|(w, _)| w.index() == v));
    let size = bipartite_matching_size(&s, looped);
    let mut rows = Vec::new();
    r.emit(&mut rows, vec![Value::Int(size as i64)])?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_maximal_cliques(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => maximal_cliques(p, r))
}

/// petgraph's Bron-Kerbosch with a pivot (module documentation): calls
/// `found` with each maximal clique, ascending.
fn bron_kerbosch(
    s: &Simple,
    clique: &mut Vec<usize>,
    mut p: Vec<usize>,
    mut x: Vec<usize>,
    found: &mut dyn FnMut(&[usize]) -> Result<(), Error>,
) -> Result<(), Error> {
    if p.is_empty() {
        if x.is_empty() {
            let mut c = clique.clone();
            c.sort_unstable();
            trace!("clique", "{c:?}");
            found(&c)?;
        }
        return Ok(());
    }
    // The pivot: the node of `p` with the most neighbours, the first one.
    let mut u = p[0];
    for v in &p {
        if s.of(*v).len() > s.of(u).len() {
            u = *v;
        }
    }
    trace!("pivot", "{clique:?} {p:?} {x:?} {u}");
    let mut todo: Vec<usize> = p
        .iter()
        .copied()
        .filter(|v| *v == u || !s.adjacent(u, *v))
        .collect();
    while let Some(v) = todo.pop() {
        p.retain(|w| *w != v);
        let nv = s.of(v);
        let next_p: Vec<usize> = p
            .iter()
            .copied()
            .filter(|w| nv.binary_search(w).is_ok())
            .collect();
        let next_x: Vec<usize> = x
            .iter()
            .copied()
            .filter(|w| nv.binary_search(w).is_ok())
            .collect();
        clique.push(v);
        bron_kerbosch(s, clique, next_p, next_x, found)?;
        clique.pop();
        let at = x.partition_point(|w| *w < v);
        x.insert(at, v);
    }
    Ok(())
}

fn maximal_cliques<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let s = kernels::simple(p, r.mem)?;
    let n = p.node_count();
    // The search state: at most `n` levels of two node lists.
    r.mem
        .grow(2 * n * n.min(1024) * size_of::<usize>() + n * size_of::<usize>())?;
    let mut rows = Vec::new();
    let mut count = 0i64;
    let mut clique = Vec::new();
    // NetworkX's `find_cliques` yields no clique for the empty graph
    // (`clique.py:250-251`); petgraph's search would return the empty one.
    if n > 0 {
        let rows = &mut rows;
        let count = &mut count;
        let r = &mut *r;
        bron_kerbosch(
            &s,
            &mut clique,
            (0..n).collect(),
            Vec::new(),
            &mut |c: &[usize]| {
                if *count % 256 == 0 {
                    r.mem.check_now()?;
                }
                let ids: Vec<Value> = c.iter().map(|v| id_value(p, *v)).collect();
                r.emit(rows, vec![Value::Int(*count), Value::List(ids.into())])?;
                *count += 1;
                Ok(())
            },
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_clique_counting(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => clique_counting(p, r))
}

/// Adds to `counts[k]` each clique of `size + k + 1` nodes extending the
/// clique of `size` nodes whose common neighbours are `cand`, by nodes
/// of increasing index.
fn extend_cliques(s: &Simple, cand: &[usize], size: usize, counts: &mut Vec<u64>) {
    for (i, c) in cand.iter().enumerate() {
        let k = size + 1;
        if k >= 2 {
            if counts.len() < k - 1 {
                counts.resize(k - 1, 0);
            }
            counts[k - 2] += 1;
        }
        let nc = s.of(*c);
        let next: Vec<usize> = cand[i + 1..]
            .iter()
            .copied()
            .filter(|w| nc.binary_search(w).is_ok())
            .collect();
        if !next.is_empty() {
            extend_cliques(s, &next, k, counts);
        }
    }
}

/// The clique counts of each node (module documentation).
pub(crate) fn clique_counts(s: &Simple, r: &mut Run<'_, '_>) -> Result<Vec<Vec<u64>>, Error> {
    let n = s.len();
    let mut out: Vec<Vec<u64>> = r.mem.vec(n, Vec::new())?;
    r.sched.fill(&mut out, |_, range, part| {
        for (o, v) in part.iter_mut().zip(range) {
            // Cliques of `k − 1` nodes among the neighbours: `k − 1 ≥ 2`.
            let mut counts = Vec::new();
            extend_cliques(s, s.of(v), 0, &mut counts);
            trace!("cq", "{v} {counts:?}");
            *o = counts;
        }
    })?;
    Ok(out)
}

fn clique_counting<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let s = kernels::simple(p, r.mem)?;
    let counts = clique_counts(&s, r)?;
    let mut global: Vec<u64> = Vec::new();
    for c in &counts {
        if global.len() < c.len() {
            global.resize(c.len(), 0);
        }
        for (g, x) in global.iter_mut().zip(c) {
            *g += x;
        }
    }
    for (i, g) in global.iter_mut().enumerate() {
        *g /= (i + 3) as u64;
    }
    let ints = |c: &[u64]| {
        Value::List(
            c.iter()
                .map(|x| Value::Int(i64::try_from(*x).unwrap_or(i64::MAX)))
                .collect::<Vec<_>>()
                .into(),
        )
    };
    let mut rows = Vec::new();
    for (v, c) in counts.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), ints(c)])?;
    }
    Ok(Output::Rows {
        rows,
        extra: vec![("globalCount", ints(&global))],
    })
}

/// The greedy colouring of `s` in `order`.
pub(crate) fn greedy_colors(s: &Simple, order: &[usize]) -> Vec<usize> {
    const NONE: usize = usize::MAX;
    let mut colors = vec![NONE; s.len()];
    let mut used: Vec<bool> = Vec::new();
    for v in order {
        used.clear();
        used.resize(s.of(*v).len() + 1, false);
        for w in s.of(*v) {
            if colors[*w] < used.len() {
                used[colors[*w]] = true;
            }
        }
        colors[*v] = used.iter().position(|u| !u).unwrap_or(used.len());
        trace!("colour", "{v} {}", colors[*v]);
    }
    colors
}

/// The node order of NetworkX strategy `name` (module documentation).
pub(crate) fn strategy_order(s: &Simple, name: &str, seed: u64) -> Option<Vec<usize>> {
    let n = s.len();
    let degree = |v: usize| s.of(v).len();
    Some(match name {
        "largest_first" => {
            let mut o: Vec<usize> = (0..n).collect();
            o.sort_by_key(|v| (std::cmp::Reverse(degree(*v)), *v));
            o
        }
        "random_sequential" => {
            let mut o: Vec<usize> = (0..n).collect();
            let mut g = rng(seed, 0);
            for i in (1..n).rev() {
                o.swap(i, g.random_range(0..=i));
            }
            o
        }
        "smallest_last" => {
            let mut deg: Vec<usize> = (0..n).map(degree).collect();
            let mut buckets: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
            for (v, d) in deg.iter().enumerate() {
                buckets.entry(*d).or_default().insert(v);
            }
            let mut removed = vec![false; n];
            let mut out = VecDeque::new();
            for _ in 0..n {
                let (&d, set) = buckets.iter_mut().next().expect("a node");
                let u = set.pop_first().expect("a node");
                if set.is_empty() {
                    buckets.remove(&d);
                }
                out.push_front(u);
                removed[u] = true;
                for w in s.of(u) {
                    if removed[*w] {
                        continue;
                    }
                    let dw = deg[*w];
                    let set = buckets.get_mut(&dw).expect("a bucket");
                    set.remove(w);
                    if set.is_empty() {
                        buckets.remove(&dw);
                    }
                    deg[*w] -= 1;
                    buckets.entry(dw - 1).or_default().insert(*w);
                }
            }
            out.into()
        }
        "independent_set" => {
            let mut remaining: BTreeSet<usize> = (0..n).collect();
            let mut out = Vec::with_capacity(n);
            while !remaining.is_empty() {
                // A maximal independent set of the remaining nodes, each
                // time the node of fewest remaining neighbours.
                let mut left = remaining.clone();
                let mut set = Vec::new();
                while !left.is_empty() {
                    let v = *left
                        .iter()
                        .min_by_key(|v| {
                            (s.of(**v).iter().filter(|w| left.contains(w)).count(), **v)
                        })
                        .expect("a node");
                    set.push(v);
                    left.remove(&v);
                    for w in s.of(v) {
                        left.remove(w);
                    }
                }
                set.sort_unstable();
                for v in &set {
                    remaining.remove(v);
                }
                out.extend(set);
            }
            out
        }
        "connected_sequential_bfs" | "connected_sequential" | "connected_sequential_dfs" => {
            let dfs = name == "connected_sequential_dfs";
            let mut seen = vec![false; n];
            let mut out = Vec::with_capacity(n);
            for source in 0..n {
                if seen[source] {
                    continue;
                }
                seen[source] = true;
                out.push(source);
                if dfs {
                    // NetworkX's `dfs_edges`: each node's neighbours in
                    // order, a node yielded when first reached.
                    let mut stack: Vec<(usize, usize)> = vec![(source, 0)];
                    while let Some(&mut (v, ref mut next)) = stack.last_mut() {
                        match s.of(v).get(*next) {
                            Some(&w) => {
                                *next += 1;
                                if !seen[w] {
                                    seen[w] = true;
                                    out.push(w);
                                    stack.push((w, 0));
                                }
                            }
                            None => {
                                stack.pop();
                            }
                        }
                    }
                } else {
                    let mut queue = VecDeque::from([source]);
                    while let Some(v) = queue.pop_front() {
                        for w in s.of(v) {
                            if !seen[*w] {
                                seen[*w] = true;
                                out.push(*w);
                                queue.push_back(*w);
                            }
                        }
                    }
                }
            }
            out
        }
        "saturation_largest_first" | "DSATUR" => {
            const NONE: usize = usize::MAX;
            let mut colors = vec![NONE; n];
            let mut distinct: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                // The uncoloured node of the largest (saturation, degree),
                // the first one.
                let mut best: Option<usize> = None;
                for v in 0..n {
                    if colors[v] != NONE {
                        continue;
                    }
                    let key = (distinct[v].len(), degree(v));
                    if best.is_none_or(|b| key > (distinct[b].len(), degree(b))) {
                        best = Some(v);
                    }
                }
                let v = best.expect("a node");
                let mut c = 0;
                while s.of(v).iter().any(|w| colors[*w] == c) {
                    c += 1;
                }
                colors[v] = c;
                out.push(v);
                for w in s.of(v) {
                    distinct[*w].insert(c);
                }
            }
            out
        }
        _ => return None,
    })
}

fn run_greedy_coloring(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => greedy_coloring(p, r))
}

fn greedy_coloring<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let mut strategy = "largest_first".to_string();
    match &r.args[0] {
        Value::Null => {}
        Value::Record(fields) => {
            for (k, v) in fields.iter() {
                match (&**k, v) {
                    ("strategy", Value::String(s)) => strategy = s.to_string(),
                    ("strategy", _) => {
                        return Err(Error::new(
                            codes::C22G03,
                            "invalid value type: the parameter strategy must be a string",
                        ));
                    }
                    (other, _) => {
                        return Err(invalid(&format!(
                            "the parameter {other} is not supported: graph_coloring.color_graph is greedy colouring"
                        )));
                    }
                }
            }
        }
        _ => {
            return Err(Error::new(
                codes::C22G03,
                "invalid value type: the argument parameters must be a record",
            ));
        }
    }
    if !matches!(r.args[1], Value::String(_) | Value::Null) {
        return Err(Error::new(
            codes::C22G03,
            "invalid value type: the argument edge_property must be a string",
        ));
    }
    let s = kernels::simple(p, r.mem)?;
    let n = p.node_count();
    r.mem.grow(n * 16 * size_of::<usize>())?;
    let order = strategy_order(&s, &strategy, r.cfg.seed())
        .ok_or_else(|| invalid(&format!("unknown strategy: {strategy}")))?;
    trace!("order", "{order:?}");
    let colors = greedy_colors(&s, &order);
    let mut rows = Vec::new();
    for (v, c) in colors.iter().enumerate() {
        r.emit(
            &mut rows,
            vec![Value::Node(p.ids[v]), Value::Int(*c as i64)],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_dsatur(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => dsatur(p, r))
}

fn dsatur<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem
        .grow(n * 96 + p.edge_ids.len() * 2 * size_of::<(f64, usize)>())?;
    let (colored, count) = petgraph::algo::dsatur_coloring(p);
    trace!(
        "dsatur",
        "{:?} {count}",
        (0..n)
            .map(|v| colored.get(&I::new(v)).copied())
            .collect::<Vec<_>>()
    );
    let mut rows = Vec::new();
    for v in 0..n {
        let c = colored.get(&I::new(v)).copied().unwrap_or(0);
        r.emit(&mut rows, vec![id_value(p, v), Value::Int(c as i64)])?;
    }
    let count = if n == 0 { 0 } else { count };
    Ok(Output::Rows {
        rows,
        extra: vec![("colorCount", Value::Int(count as i64))],
    })
}

fn run_k1_coloring(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => k1_coloring(p, r))
}

/// ALG-176's Jones-Plassmann rounds (module documentation): the colours
/// and the number of rounds.
pub(crate) fn jones_plassmann(
    s: &Simple,
    r: &mut Run<'_, '_>,
) -> Result<(Vec<usize>, usize), Error> {
    const NONE: usize = usize::MAX;
    let n = s.len();
    let first = |a: usize, b: usize| {
        let (da, db) = (s.of(a).len(), s.of(b).len());
        da > db || (da == db && a < b)
    };
    let mut colors = r.mem.vec(n, NONE)?;
    let mut next = r.mem.vec(n, NONE)?;
    let mut rounds = 0;
    let mut left = n;
    while left > 0 {
        rounds += 1;
        let prev = &colors;
        let done = r.sched.fill(&mut next, |_, range, out| {
            let mut done = 0usize;
            let mut used: Vec<bool> = Vec::new();
            for (o, v) in out.iter_mut().zip(range) {
                *o = prev[v];
                if prev[v] != NONE {
                    continue;
                }
                let nv = s.of(v);
                if nv.iter().any(|w| prev[*w] == NONE && first(*w, v)) {
                    continue;
                }
                used.clear();
                used.resize(nv.len() + 1, false);
                for w in nv {
                    if prev[*w] < used.len() {
                        used[prev[*w]] = true;
                    }
                }
                *o = used.iter().position(|u| !u).unwrap_or(used.len());
                done += 1;
            }
            done
        })?;
        std::mem::swap(&mut colors, &mut next);
        trace!(
            "round",
            "{rounds} {:?}",
            (0..n)
                .filter(|v| colors[*v] != NONE && next[*v] == NONE)
                .map(|v| (v, colors[v]))
                .collect::<Vec<_>>()
        );
        left -= done.iter().sum::<usize>();
        r.mem.check_now()?;
    }
    Ok((colors, rounds))
}

fn k1_coloring<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let s = kernels::simple_undirected(p, r.mem)?;
    let n = p.node_count();
    let (colors, _) = jones_plassmann(&s, r)?;
    let mut size: BTreeMap<usize, usize> = BTreeMap::new();
    for c in &colors {
        *size.entry(*c).or_default() += 1;
    }
    let min = match r.cfg.get("minCommunitySize") {
        Value::Int(m) => usize::try_from(*m).unwrap_or(0),
        _ => 0,
    };
    let mut rows = Vec::new();
    for (v, c) in colors.iter().enumerate() {
        if size[c] >= min {
            r.emit(&mut rows, vec![id_value(p, v), Value::Int(*c as i64)])?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("nodeCount", Value::Int(n as i64)),
            ("colorCount", Value::Int(color_count(&colors) as i64)),
            ("ranIterations", Value::Int(i64::from(n > 0))),
            ("didConverge", Value::Bool(true)),
        ],
    })
}

fn run_feedback_arc_set(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => feedback_arc_set(p, r))
}

fn feedback_arc_set<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let m: usize = (0..n).map(|v| p.out_degree(v)).sum();
    r.mem.grow(n * 128 + m * 96)?;
    let mut g: DiGraph<(), usize> = DiGraph::with_capacity(n, m);
    for _ in 0..n {
        g.add_node(());
    }
    for u in 0..n {
        for (v, slot) in p.out(u) {
            g.add_edge(NodeIndex::new(u), NodeIndex::new(v.index()), slot);
        }
    }
    let mut rows = Vec::new();
    trace!(
        "graph",
        "{:?}",
        g.edge_references()
            .map(|e| (e.source().index(), e.target().index()))
            .collect::<Vec<_>>()
    );
    let arcs: Vec<(usize, usize, usize)> = petgraph::algo::greedy_feedback_arc_set(&g)
        .map(|e| (e.source().index(), e.target().index(), *e.weight()))
        .collect();
    trace!(
        "arcs",
        "{:?}",
        arcs.iter().map(|(u, v, _)| (*u, *v)).collect::<Vec<_>>()
    );
    for (u, v, slot) in arcs {
        r.emit(
            &mut rows,
            vec![
                id_value(p, u),
                id_value(p, v),
                Value::Int(p.edge_id(slot).cast_signed()),
            ],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_set_cover(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let ids = |v: &Value, what: &str| -> Result<Vec<u64>, Error> {
        match v {
            Value::List(l) => l
                .iter()
                .map(|x| match x {
                    Value::Node(id) => Ok(*id),
                    _ => Err(Error::new(
                        codes::C22G03,
                        format!("invalid value type: {what} must be a list of nodes"),
                    )),
                })
                .collect(),
            _ => Err(Error::new(
                codes::C22G03,
                format!("invalid value type: {what} must be a list of nodes"),
            )),
        }
    };
    let elements = ids(&r.args[0], "element_vertexes")?;
    let sets = ids(&r.args[1], "set_vertexes")?;
    let pairs = elements.len().min(sets.len());
    r.mem.grow(pairs * 64)?;
    let cover = greedy_cover(&elements[..pairs], &sets[..pairs]);
    let mut rows = Vec::new();
    for s in cover {
        r.emit(&mut rows, vec![Value::Node(s)])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// The classic greedy set cover (module documentation): the sets taken,
/// in order.
pub(crate) fn greedy_cover(elements: &[u64], sets: &[u64]) -> Vec<u64> {
    let mut order: Vec<u64> = Vec::new();
    let mut members: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for (e, s) in elements.iter().zip(sets) {
        if !members.contains_key(s) {
            order.push(*s);
        }
        members.entry(*s).or_default().insert(*e);
    }
    let mut uncovered: BTreeSet<u64> = elements.iter().copied().collect();
    let mut out = Vec::new();
    while !uncovered.is_empty() {
        let mut best: Option<(usize, u64)> = None;
        for s in &order {
            let gain = members[s].intersection(&uncovered).count();
            if gain > 0 && best.is_none_or(|(g, _)| gain > g) {
                best = Some((gain, *s));
            }
        }
        let Some((_gain, s)) = best else {
            break;
        };
        trace!("pick", "{s} {_gain}");
        for e in &members[&s] {
            uncovered.remove(e);
        }
        out.push(s);
    }
    out
}
