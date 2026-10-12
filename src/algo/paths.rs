//! The shortest and weighted paths family (rmp #64): ALG-013 unweighted
//! shortest paths, ALG-014 Dijkstra, ALG-015 bidirectional Dijkstra,
//! ALG-018 A*, ALG-019 Bellman-Ford, ALG-020 negative-cycle detection,
//! ALG-021 SPFA, ALG-023 delta-stepping, ALG-024 Johnson, ALG-025
//! Floyd-Warshall, ALG-026 Seidel, ALG-027 all-pairs shortest distances,
//! ALG-028 all shortest paths between two nodes, ALG-029 Yen's k shortest
//! paths, ALG-030 length of the k-th shortest path, ALG-031 all simple
//! paths, ALG-032 longest path in a DAG.
//!
//! * **Costs.** `relationshipWeightProperty` names the edge property each
//!   edge costs; without it every edge costs 1.0, as GDS's unweighted
//!   graphs. Entries that need non-negative costs (014, 015, 018, 023,
//!   027, 028, 029, 030) fail with 22N11 on a negative or NaN cost; a
//!   negative cycle fails 021, 024 and 025 with 22N11. A path's edge
//!   between two nodes is the cheapest one ([`kernels::best_slot`]).
//! * **Rows.** Path entries return PathRows ([`kernels::path_row`]); a
//!   path from a node to itself is one row of cost 0, as GDS's and
//!   NetworkX's results hold it. Rows are in source, then target, index
//!   order, except where noted.
//! * **Calls.** 009-style single-worker entries call petgraph 0.8.3 where
//!   it has the function: `astar` (018), `find_negative_cycle` (020),
//!   `spfa` (021), `floyd_warshall` (025), `k_shortest_path` (030).
//! * **Parallel entries** (TS-25 Parallelism): 019 runs synchronous
//!   Bellman-Ford rounds, each node pulling over its incoming edges what
//!   the previous round settled, so a round's result is the same at any
//!   `concurrency` (GDS's `paths/bellmanford/BellmanFord.java` relaxes a
//!   shared frontier instead; studied); 023 is delta-stepping with each
//!   bucket's light and heavy relaxations computed by blocks and applied
//!   in block order, the method of LAGraph @ `7543de1f3103`
//!   `src/algorithm/LAGr_SingleSourceShortestPath.c` (BSD-2-Clause,
//!   method only, no code); 024 and 027 run one Dijkstra (or
//!   breadth-first search) per source, sources split into blocks, as
//!   petgraph's `parallel_johnson` maps sources in parallel (below); 029
//!   computes the spur paths of a round by blocks of spur nodes; 032
//!   computes each topological level's distances by blocks
//!   ([`kernels::levels`]).
//!
//! **Origins.**
//!
//! * [`johnson`]: petgraph 0.8.3 `src/algo/johnson.rs` (`parallel_johnson`,
//!   `johnson_reweight`, MIT OR Apache-2.0, used under MIT; licence text
//!   in [`super::projection`]): reweight by the distances from a virtual
//!   source that reaches every node at cost 0, run Dijkstra from every
//!   source on the reweighted costs, undo the reweighting. Modified: the
//!   reweighting runs the synchronous Bellman-Ford rounds of 019, and
//!   rayon's `par_iter` is replaced by blocks.
//! * [`bidirectional`]: petgraph 0.8.3 `src/algo/dijkstra.rs`
//!   (`bidirectional_dijkstra`; MIT, as above): two heaps, a forward
//!   search over outgoing and a backward search over incoming edges, and
//!   the stop rule `d(u) + d(v) ≥ best`. Modified: each side keeps a
//!   predecessor per node and the best meeting edge, so the path is
//!   returned with its cost.
//! * [`yens`]: pathfinding @ `cd4600b36ec0` `src/directed/yen.rs` (`yen`,
//!   `prefix_costs`; Apache-2.0/MIT, used under MIT; copyright and
//!   licence below): the candidate heap ordered by cost then length, the
//!   spur loop from each route's spur index, the removed edges and root
//!   nodes, the equal-cost pops. Modified: paths are sequences of edges,
//!   so parallel edges give distinct paths, as GDS's Yen's keeps them
//!   (GDS `paths/yens/Yens.java`, studied); the spur paths of a round are
//!   computed by blocks and merged in spur order.
//! * [`seidel`]: graphalgs @ `1f8a412ef4af` `src/shortest_path/seidel.rs`
//!   (`seidel`, `apd`; MIT, "Copyright (c) 2021 PrototypeRailGun",
//!   licence below): the APD recursion on 0/1 adjacency matrices.
//!   Modified: nalgebra 0.35 instead of 0.33, no `unsafe` uninitialised
//!   matrices, and one run per connected component, since APD only ends
//!   on a connected graph.
//! * [`all_simple`]: MAGE @ `c72636014a8b`
//!   `cpp/algo_module/algorithm/algo.cpp` (`PathFinder`,
//!   `UpdateRelationshipDirection`, `DFS`; Apache-2.0, see
//!   `LICENSE-APACHE`; Memgraph Ltd., no copyright line in the file),
//!   ported with an explicit stack instead of recursion.
//!
//! pathfinding's licence (`Cargo.toml` `license = "Apache-2.0/MIT"`,
//! author Samuel Tardieu; the repository holds no licence file, so the
//! MIT text is quoted from the MIT licence):
//!
//! > Copyright (c) Samuel Tardieu
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining
//! > a copy of this software and associated documentation files (the
//! > "Software"), to deal in the Software without restriction, including
//! > without limitation the rights to use, copy, modify, merge, publish,
//! > distribute, sublicense, and/or sell copies of the Software, and to
//! > permit persons to whom the Software is furnished to do so, subject to
//! > the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be
//! > included in all copies or substantial portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
//! > EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! > MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
//! > NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
//! > BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
//! > ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
//! > CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! > SOFTWARE.
//!
//! graphalgs's licence (`LICENSE` @ `1f8a412ef4af`): the MIT licence
//! above, with "Copyright (c) 2021 PrototypeRailGun".

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use nalgebra::DMatrix;
use petgraph::visit::EdgeRef;

use super::config::{Def, Key, PATH_WRITE as PATH_WRITE_KEYS, Ty};
use super::kernels::{self, MinScored, NONE, Tree, id_value, node_index, path_row};
use super::projection::{Ix, Projection, with_projection};
use super::traversal::{key_node, key_nodes};
use super::{
    Arg, ArgDefault, Entry, NODE_WRITE_COLUMNS, Output, PATH_ROWS, PATH_WRITE, Run, WriteKind,
};
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

const WEIGHT: Key = Key::new("relationshipWeightProperty", Ty::Str, Def::Absent);

const DISTANCE_COLUMNS: &[&str] = &["sourceNodeId", "targetNodeId", "distance"];

/// The shape of a path entry with a write mode.
const fn path_entry(
    algs: &'static [u16],
    stream: &'static str,
    write: &'static str,
    keys: &'static [Key],
    run: fn(&mut Run<'_, '_>) -> Result<Output, Error>,
) -> Entry {
    Entry {
        algs,
        stream,
        write: Some(write),
        keys,
        edge_props: &["relationshipWeightProperty"],
        stream_columns: PATH_ROWS,
        write_columns: PATH_WRITE,
        run,
        write_keys: PATH_WRITE_KEYS,
        write_kind: WriteKind::PathRels { cycle: None },
        ..Entry::BASE
    }
}

/// The shape of an all-pairs distance entry.
const fn distance_entry(
    algs: &'static [u16],
    stream: &'static str,
    parallel: bool,
    run: fn(&mut Run<'_, '_>) -> Result<Output, Error>,
) -> Entry {
    Entry {
        algs,
        stream,
        keys: &[WEIGHT],
        parallel,
        edge_props: &["relationshipWeightProperty"],
        stream_columns: DISTANCE_COLUMNS,
        run,
        ..Entry::BASE
    }
}

/// ALG-013: `gds.unweightedShortestPaths.stream` and `.write`.
pub(crate) const UNWEIGHTED: Entry = Entry {
    edge_props: &[],
    ..path_entry(
        &[13],
        "gds.unweightedShortestPaths.stream",
        "gds.unweightedShortestPaths.write",
        &[
            Key::new("sourceNode", Ty::Node, Def::Absent),
            Key::new("targetNode", Ty::Node, Def::Absent),
        ],
        run_unweighted,
    )
};

/// ALG-014: `gds.shortestPath.dijkstra.stream` and `.write`.
pub(crate) const DIJKSTRA: Entry = path_entry(
    &[14],
    "gds.shortestPath.dijkstra.stream",
    "gds.shortestPath.dijkstra.write",
    &[
        Key::new("sourceNode", Ty::Node, Def::Required),
        Key::new("targetNode", Ty::Node, Def::Absent),
        Key::new("targetNodes", Ty::Nodes, Def::EmptyList),
        WEIGHT,
    ],
    run_dijkstra,
);

/// ALG-014: `gds.allShortestPaths.dijkstra.stream` and `.write`.
pub(crate) const DIJKSTRA_ALL: Entry = path_entry(
    &[14],
    "gds.allShortestPaths.dijkstra.stream",
    "gds.allShortestPaths.dijkstra.write",
    &[Key::new("sourceNode", Ty::Node, Def::Required), WEIGHT],
    run_dijkstra_all,
);

const PAIR_KEYS: &[Key] = &[
    Key::new("sourceNode", Ty::Node, Def::Required),
    Key::new("targetNode", Ty::Node, Def::Required),
    WEIGHT,
];

/// ALG-015: `gds.bidirectionalDijkstra.stream` and `.write`.
pub(crate) const BIDIRECTIONAL: Entry = path_entry(
    &[15],
    "gds.bidirectionalDijkstra.stream",
    "gds.bidirectionalDijkstra.write",
    PAIR_KEYS,
    run_bidirectional,
);

/// ALG-018: `gds.shortestPath.astar.stream` and `.write`.
pub(crate) const ASTAR: Entry = Entry {
    node_props: &["latitudeProperty", "longitudeProperty"],
    ..path_entry(
        &[18],
        "gds.shortestPath.astar.stream",
        "gds.shortestPath.astar.write",
        &[
            Key::new("sourceNode", Ty::Node, Def::Required),
            Key::new("targetNode", Ty::Node, Def::Required),
            Key::new("latitudeProperty", Ty::Str, Def::Required),
            Key::new("longitudeProperty", Ty::Str, Def::Required),
            WEIGHT,
        ],
        run_astar,
    )
};

/// ALG-019: `gds.bellmanFord.stream` and `.write`.
pub(crate) const BELLMAN_FORD: Entry = Entry {
    algs: &[19],
    stream: "gds.bellmanFord.stream",
    write: Some("gds.bellmanFord.write"),
    keys: &[Key::new("sourceNode", Ty::Node, Def::Required), WEIGHT],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &[
        "index",
        "sourceNode",
        "targetNode",
        "totalCost",
        "nodeIds",
        "costs",
        "route",
        "isNegativeCycle",
    ],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "postProcessingMillis",
        "relationshipsWritten",
        "containsNegativeCycle",
        "configuration",
    ],
    run: run_bellman_ford,
    write_keys: &[
        Key::new("writeRelationshipType", Ty::Str, Def::Required),
        Key::new("writeNodeIds", Ty::Bool, Def::Bool(false)),
        Key::new("writeCosts", Ty::Bool, Def::Bool(false)),
        Key::new("writeNegativeCycles", Ty::Bool, Def::Bool(false)),
    ],
    write_kind: WriteKind::PathRels { cycle: Some(7) },
    ..Entry::BASE
};

/// ALG-020: `gds.findNegativeCycle.stream`.
pub(crate) const NEGATIVE_CYCLE: Entry = Entry {
    algs: &[20],
    stream: "gds.findNegativeCycle.stream",
    keys: &[Key::new("sourceNode", Ty::Node, Def::Required), WEIGHT],
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &["nodeIds", "path"],
    run: run_negative_cycle,
    ..Entry::BASE
};

/// ALG-021: `gds.spfa.stream` and `.write`.
pub(crate) const SPFA: Entry = path_entry(
    &[21],
    "gds.spfa.stream",
    "gds.spfa.write",
    &[Key::new("sourceNode", Ty::Node, Def::Required), WEIGHT],
    run_spfa,
);

/// ALG-023: `gds.allShortestPaths.delta.stream` and `.write`.
pub(crate) const DELTA: Entry = Entry {
    parallel: true,
    ..path_entry(
        &[23],
        "gds.allShortestPaths.delta.stream",
        "gds.allShortestPaths.delta.write",
        &[
            Key::new("sourceNode", Ty::Node, Def::Required),
            Key::new(
                "delta",
                Ty::Float {
                    min: f64::MIN_POSITIVE,
                    max: f64::INFINITY,
                    max_open: true,
                },
                Def::Float(2.0),
            ),
            WEIGHT,
        ],
        run_delta,
    )
};

/// ALG-024: `gds.johnson.stream`.
pub(crate) const JOHNSON: Entry = distance_entry(&[24], "gds.johnson.stream", true, run_johnson);

/// ALG-025: `gds.floydWarshall.stream`.
pub(crate) const FLOYD_WARSHALL: Entry =
    distance_entry(&[25], "gds.floydWarshall.stream", false, run_floyd);

/// ALG-026: `gds.seidel.stream`.
pub(crate) const SEIDEL: Entry = Entry {
    keys: &[],
    edge_props: &[],
    undirected: true,
    ..distance_entry(&[26], "gds.seidel.stream", false, run_seidel)
};

/// ALG-027: `gds.allShortestPaths.stream`.
pub(crate) const ALL_PAIRS: Entry =
    distance_entry(&[27], "gds.allShortestPaths.stream", true, run_all_pairs);

/// ALG-028: `gds.allShortestPathsBetweenNodes.stream` and `.write`.
pub(crate) const ALL_SHORTEST_BETWEEN: Entry = path_entry(
    &[28],
    "gds.allShortestPathsBetweenNodes.stream",
    "gds.allShortestPathsBetweenNodes.write",
    PAIR_KEYS,
    run_all_shortest_between,
);

/// ALG-029: `gds.shortestPath.yens.stream` and `.write`.
pub(crate) const YENS: Entry = Entry {
    parallel: true,
    ..path_entry(
        &[29],
        "gds.shortestPath.yens.stream",
        "gds.shortestPath.yens.write",
        &[
            Key::new("sourceNode", Ty::Node, Def::Required),
            Key::new("targetNode", Ty::Node, Def::Required),
            Key::new("k", Ty::Int { min: 1 }, Def::Required),
            WEIGHT,
        ],
        run_yens,
    )
};

/// ALG-030: `gds.kShortestPath.stream` and `.write`.
pub(crate) const K_SHORTEST: Entry = Entry {
    algs: &[30],
    stream: "gds.kShortestPath.stream",
    write: Some("gds.kShortestPath.write"),
    keys: &[
        Key::new("sourceNode", Ty::Node, Def::Required),
        Key::new("k", Ty::Int { min: 1 }, Def::Required),
        Key::new("targetNode", Ty::Node, Def::Absent),
        WEIGHT,
    ],
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &["nodeId", "cost"],
    write_columns: NODE_WRITE_COLUMNS,
    run: run_k_shortest,
    write_kind: WriteKind::NodeProp { id: 0, value: 1 },
    ..Entry::BASE
};

/// ALG-031: `algo.all_simple_paths`.
pub(crate) const ALL_SIMPLE_PATHS: Entry = Entry {
    algs: &[31],
    stream: "algo.all_simple_paths",
    stream_columns: &["path"],
    run: run_all_simple,
    args: &[
        Arg::new("start_node", ArgDefault::Required),
        Arg::new("end_node", ArgDefault::Required),
        Arg::new("relationship_types", ArgDefault::Required),
        Arg::new("max_length", ArgDefault::Required),
    ],
    edge_types: true,
    ..Entry::BASE
};

/// ALG-032: `gds.dag.longestPath.stream`.
pub(crate) const LONGEST_PATH: Entry = Entry {
    algs: &[32],
    stream: "gds.dag.longestPath.stream",
    keys: &[WEIGHT],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: PATH_ROWS,
    run: run_longest_path,
    ..Entry::BASE
};

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// Whether the run is weighted.
fn weighted(r: &Run<'_, '_>) -> bool {
    r.cfg.str("relationshipWeightProperty").is_some()
}

/// Fails with 22N11 when an edge of the orientation costs a negative
/// amount or NaN.
fn non_negative<I: Ix>(p: &Projection<I>, weighted: bool) -> Result<(), Error> {
    if weighted && p.edge_props[0].iter().any(|w| w.is_nan() || *w < 0.0) {
        return Err(invalid(
            "the relationship weights must be numbers of at least 0",
        ));
    }
    Ok(())
}

/// Fails with 22N11 when an edge costs NaN.
fn numeric<I: Ix>(p: &Projection<I>, weighted: bool) -> Result<(), Error> {
    if weighted && p.edge_props[0].iter().any(|w| w.is_nan()) {
        return Err(invalid("a relationship weight is not a number"));
    }
    Ok(())
}

fn negative_cycle() -> Error {
    invalid("the projection holds a negative cycle")
}

fn rows_output(rows: Vec<Vec<Value>>) -> Output {
    Output::Rows {
        rows,
        extra: Vec::new(),
    }
}

/// Emits the path row of `tree` to `target`, if reached.
fn emit_tree_path<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    rows: &mut Vec<Vec<Value>>,
    tree: &Tree,
    target: usize,
) -> Result<(), Error> {
    if let Some((nodes, slots)) = tree.walk(target) {
        let costs = tree.costs(&nodes);
        let row = path_row(p, rows.len(), &nodes, &slots, &costs);
        r.emit(rows, row)?;
    }
    Ok(())
}

fn run_unweighted(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => unweighted(p, r))
}

/// A breadth-first tree from `source` over outgoing (`forward`) or
/// incoming edges, each edge costing 1.
fn bfs_tree<I: Ix>(p: &Projection<I>, source: usize, forward: bool) -> Tree {
    let mut t = Tree::new(p.node_count());
    t.dist[source] = 0.0;
    let mut queue = std::collections::VecDeque::from([source]);
    while let Some(v) = queue.pop_front() {
        let adj = if forward { p.out(v) } else { p.inc(v) };
        for (w, slot) in adj {
            let w = w.index();
            if t.dist[w].is_infinite() {
                trace!("visit", "{w} from {v}");
                t.dist[w] = t.dist[v] + 1.0;
                t.pred[w] = v;
                t.slot[w] = slot;
                queue.push_back(w);
            }
        }
    }
    t
}

fn unweighted<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let source = match r.cfg.get("sourceNode") {
        Value::Null => None,
        v => Some(node_index(p, v, "sourceNode")?),
    };
    let target = match r.cfg.get("targetNode") {
        Value::Null => None,
        v => Some(node_index(p, v, "targetNode")?),
    };
    r.mem.grow(kernels::dijkstra_bytes(n, 0))?;
    let mut rows = Vec::new();
    match (source, target) {
        (Some(s), Some(t)) => {
            let tree = bfs_tree(p, s, true);
            emit_tree_path(p, r, &mut rows, &tree, t)?;
        }
        (Some(s), None) => {
            let tree = bfs_tree(p, s, true);
            for t in 0..n {
                emit_tree_path(p, r, &mut rows, &tree, t)?;
            }
        }
        (None, Some(t)) => {
            // Paths to `t`: the tree of incoming edges from `t`, read
            // backwards.
            let tree = bfs_tree(p, t, false);
            for s in 0..n {
                if let Some((mut nodes, mut slots)) = tree.walk(s) {
                    nodes.reverse();
                    slots.reverse();
                    let costs: Vec<f64> = (0..nodes.len()).map(|i| i as f64).collect();
                    let row = path_row(p, rows.len(), &nodes, &slots, &costs);
                    r.emit(&mut rows, row)?;
                }
            }
        }
        (None, None) => {
            for s in 0..n {
                r.mem.check_now()?;
                let tree = bfs_tree(p, s, true);
                for t in 0..n {
                    emit_tree_path(p, r, &mut rows, &tree, t)?;
                }
            }
        }
    }
    Ok(rows_output(rows))
}

fn run_dijkstra(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => dijkstra_pairs(p, r))
}

fn dijkstra_pairs<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let mut targets = key_nodes(p, r, "targetNodes")?;
    if let Value::Node(_) = r.cfg.get("targetNode") {
        targets.insert(0, key_node(p, r, "targetNode")?);
    }
    if targets.is_empty() {
        return Err(invalid(
            "the configuration needs the key targetNode or targetNodes",
        ));
    }
    let mut seen = BTreeSet::new();
    targets.retain(|t| seen.insert(*t));
    r.mem
        .grow(kernels::dijkstra_bytes(p.node_count(), p.edge_ids.len()))?;
    let cost = |slot| kernels::cost(p, w, slot);
    let tree = kernels::dijkstra(p, s, &cost, None);
    let mut rows = Vec::new();
    for t in targets {
        emit_tree_path(p, r, &mut rows, &tree, t)?;
    }
    Ok(rows_output(rows))
}

fn run_dijkstra_all(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => dijkstra_all(p, r))
}

fn dijkstra_all<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    r.mem
        .grow(kernels::dijkstra_bytes(p.node_count(), p.edge_ids.len()))?;
    let cost = |slot| kernels::cost(p, w, slot);
    let tree = kernels::dijkstra(p, s, &cost, None);
    let mut rows = Vec::new();
    for t in 0..p.node_count() {
        emit_tree_path(p, r, &mut rows, &tree, t)?;
    }
    Ok(rows_output(rows))
}

fn run_bidirectional(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => bidirectional_entry(p, r))
}

fn bidirectional_entry<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let t = key_node(p, r, "targetNode")?;
    r.mem
        .grow(2 * kernels::dijkstra_bytes(p.node_count(), p.edge_ids.len()))?;
    let mut rows = Vec::new();
    if let Some((nodes, slots, _)) = bidirectional(p, w, s, t) {
        let mut costs = vec![0.0];
        for slot in &slots {
            let c = costs[costs.len() - 1] + kernels::cost(p, w, *slot);
            costs.push(c);
        }
        let row = path_row(p, 0, &nodes, &slots, &costs);
        r.emit(&mut rows, row)?;
    }
    Ok(rows_output(rows))
}

/// Bidirectional Dijkstra from `s` to `t` (module documentation): the
/// path's nodes, edges and cost.
fn bidirectional<I: Ix>(
    p: &Projection<I>,
    w: bool,
    s: usize,
    t: usize,
) -> Option<(Vec<usize>, Vec<usize>, f64)> {
    if s == t {
        return Some((vec![s], Vec::new(), 0.0));
    }
    let n = p.node_count();
    let (mut fwd, mut bwd) = (Tree::new(n), Tree::new(n));
    let (mut fvis, mut bvis) = (vec![false; n], vec![false; n]);
    fwd.dist[s] = 0.0;
    bwd.dist[t] = 0.0;
    let (mut fh, mut bh) = (BinaryHeap::new(), BinaryHeap::new());
    fh.push(MinScored(0.0, s));
    bh.push(MinScored(0.0, t));
    // The best meeting: cost, forward node, backward node, joining edge.
    let mut best: Option<(f64, usize, usize, usize)> = None;
    while let (Some(MinScored(_, u)), Some(MinScored(_, v))) = (fh.pop(), bh.pop()) {
        fvis[u] = true;
        bvis[v] = true;
        let (du, dv) = (fwd.dist[u], bwd.dist[v]);
        trace!("pop", "{u} {du:?} {v} {dv:?}");
        for (x, slot) in p.out(u) {
            let x = x.index();
            let c = kernels::cost(p, w, slot);
            if !fvis[x] && du + c < fwd.dist[x] {
                fwd.dist[x] = du + c;
                fwd.pred[x] = u;
                fwd.slot[x] = slot;
                fh.push(MinScored(du + c, x));
            }
            if bvis[x] {
                let total = du + c + bwd.dist[x];
                if best.is_none_or(|b| total < b.0) {
                    trace!("best", "{total:?} {u} {x}");
                    best = Some((total, u, x, slot));
                }
            }
        }
        for (x, slot) in p.inc(v) {
            let x = x.index();
            let c = kernels::cost(p, w, slot);
            if !bvis[x] && dv + c < bwd.dist[x] {
                bwd.dist[x] = dv + c;
                bwd.pred[x] = v;
                bwd.slot[x] = slot;
                bh.push(MinScored(dv + c, x));
            }
            if fvis[x] {
                let total = dv + c + fwd.dist[x];
                if best.is_none_or(|b| total < b.0) {
                    trace!("best", "{total:?} {x} {v}");
                    best = Some((total, x, v, slot));
                }
            }
        }
        if let Some(b) = best
            && du + dv >= b.0
        {
            trace!("stop", "{:?} >= {:?}", du + dv, b.0);
            let (cost, a, z, slot) = b;
            let (mut nodes, mut slots) = fwd.walk(a)?;
            slots.push(slot);
            let (mut back_nodes, mut back_slots) = bwd.walk(z)?;
            back_nodes.reverse();
            back_slots.reverse();
            nodes.extend(back_nodes);
            slots.extend(back_slots);
            return Some((nodes, slots, cost));
        }
    }
    // A heap ran out before the stop rule held: the best meeting found
    // is the shortest path (petgraph returns no path here).
    trace!("fallback", "{best:?}");
    let (cost, a, z, slot) = best?;
    let (mut nodes, mut slots) = fwd.walk(a)?;
    slots.push(slot);
    let (mut back_nodes, mut back_slots) = bwd.walk(z)?;
    back_nodes.reverse();
    back_slots.reverse();
    nodes.extend(back_nodes);
    slots.extend(back_slots);
    Some((nodes, slots, cost))
}

fn run_astar(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => astar(p, r))
}

/// GDS's A* heuristic: the haversine distance in nautical miles (GDS
/// `paths/astar/AStar.java` `HaversineHeuristic.distance`, studied:
/// `KM_TO_NM = 0.539957`, earth radius 6371 km).
pub(crate) fn haversine_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const KM_TO_NM: f64 = 0.539957;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + (d_lon / 2.0).sin().powi(2) * lat1.to_radians().cos() * lat2.to_radians().cos();
    6371.0 * KM_TO_NM * 2.0 * a.sqrt().asin()
}

fn astar<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let t = key_node(p, r, "targetNode")?;
    let (lat, lon) = (&p.node_props[0], &p.node_props[1]);
    if lat.iter().chain(lon.iter()).any(|x| x.is_nan()) {
        return Err(invalid(
            "every node needs a numeric latitude and longitude property",
        ));
    }
    r.mem
        .grow(4 * kernels::dijkstra_bytes(p.node_count(), p.edge_ids.len()))?;
    let found = petgraph::algo::astar(
        p,
        I::new(s),
        |v| v.index() == t,
        |e| kernels::cost(p, w, e.id()),
        |v| haversine_nm(lat[v.index()], lon[v.index()], lat[t], lon[t]),
    );
    let mut rows = Vec::new();
    trace!(
        "petgraph",
        "{:?}",
        found
            .as_ref()
            .map(|(c, path)| (*c, path.iter().map(|v| v.index()).collect::<Vec<_>>()))
    );
    if let Some((_, path)) = found {
        let nodes: Vec<usize> = path.iter().map(|v| v.index()).collect();
        let (slots, costs) = kernels::path_edges(p, w, &nodes);
        let row = path_row(p, 0, &nodes, &slots, &costs);
        r.emit(&mut rows, row)?;
    }
    Ok(rows_output(rows))
}

/// Synchronous Bellman-Ford rounds (module documentation) from the nodes
/// `sources` start at 0 (every node when `sources` is empty, Johnson's
/// virtual source). Returns the tree and the nodes still changing in
/// round `n`, which lie on or after a negative cycle (empty if none).
fn bellman_ford_rounds<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    w: bool,
    sources: &[usize],
) -> Result<(Tree, Vec<usize>), Error> {
    let n = p.node_count();
    r.mem.grow(2 * n * 32)?;
    let mut cur: Vec<(f64, usize, usize, bool)> = vec![(f64::INFINITY, NONE, NONE, false); n];
    if sources.is_empty() {
        cur.fill((0.0, NONE, NONE, true));
    } else {
        for s in sources {
            cur[*s] = (0.0, NONE, NONE, true);
        }
    }
    let mut next = cur.clone();
    let mut changing = Vec::new();
    for round in 1..=n {
        let prev = &cur;
        let changed = r.sched.fill(&mut next, |_, range, out| {
            let mut any = false;
            for (o, v) in out.iter_mut().zip(range) {
                let (mut best, mut pred, mut slot, _) = prev[v];
                let mut moved = false;
                for (u, s) in p.inc(v) {
                    let u = u.index();
                    let pu = prev[u];
                    if !pu.3 || pu.0.is_infinite() {
                        continue;
                    }
                    let c = pu.0 + kernels::cost(p, w, s);
                    if c < best {
                        best = c;
                        pred = u;
                        slot = s;
                        moved = true;
                    }
                }
                *o = (best, pred, slot, moved);
                any |= moved;
            }
            any
        })?;
        std::mem::swap(&mut cur, &mut next);
        trace!(
            "round",
            "{round} {:?}",
            cur.iter().map(|c| c.0).collect::<Vec<_>>()
        );
        #[cfg(test)]
        if let Some(key) = &r.sched.probe {
            rounds::record(key, round);
        }
        if !changed.iter().any(|c| *c) {
            break;
        }
        if round == n {
            changing = (0..n).filter(|v| cur[*v].3).collect();
            trace!("stop", "changing {changing:?}");
        }
    }
    let mut t = Tree::new(n);
    for (v, c) in cur.iter().enumerate() {
        t.dist[v] = c.0;
        t.pred[v] = c.1;
        t.slot[v] = c.2;
    }
    Ok((t, changing))
}

/// The rounds the last Bellman-Ford run of a database took (the
/// laboratory record of rmp #64), keyed as the worker-count probe.
#[cfg(test)]
pub(crate) mod rounds {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, PoisonError};

    static ROUNDS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

    pub(crate) fn record(key: &str, round: usize) {
        ROUNDS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_string(), round);
    }

    pub(crate) fn take(key: &str) -> Option<usize> {
        ROUNDS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key)
    }
}

/// The negative cycles the predecessors of `changing` lead to, each once,
/// starting at its smallest node: nodes (closed) and edges.
fn negative_cycles<I: Ix>(
    p: &Projection<I>,
    w: bool,
    t: &Tree,
    changing: &[usize],
) -> Vec<(Vec<usize>, Vec<usize>)> {
    let n = p.node_count();
    let mut found: BTreeSet<Vec<usize>> = BTreeSet::new();
    let mut out = Vec::new();
    for &start in changing {
        let mut v = start;
        for _ in 0..n {
            if t.pred[v] == NONE {
                break;
            }
            v = t.pred[v];
        }
        if t.pred[v] == NONE {
            continue;
        }
        // Collect the cycle through v backwards.
        let mut back = vec![v];
        let mut u = t.pred[v];
        while u != v && back.len() <= n {
            back.push(u);
            u = t.pred[u];
        }
        if u != v {
            continue;
        }
        back.reverse();
        // Rotate so the smallest node comes first.
        let at = (0..back.len()).min_by_key(|i| back[*i]).expect("a node");
        back.rotate_left(at);
        if !found.insert(back.clone()) {
            continue;
        }
        let mut nodes = back.clone();
        nodes.push(back[0]);
        let slots: Vec<usize> = nodes[1..].iter().map(|x| t.slot[*x]).collect();
        let weight: f64 = slots.iter().map(|s| kernels::cost(p, w, *s)).sum();
        if weight < 0.0 {
            out.push((nodes, slots));
        }
    }
    out
}

fn run_bellman_ford(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => bellman_ford(p, r))
}

fn bellman_ford<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    numeric(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let (tree, changing) = bellman_ford_rounds(p, r, w, &[s])?;
    let mut rows = Vec::new();
    let cycles = if changing.is_empty() {
        Vec::new()
    } else {
        negative_cycles(p, w, &tree, &changing)
    };
    let contains = !changing.is_empty();
    if contains {
        for (nodes, slots) in cycles {
            let mut costs = vec![0.0];
            for slot in &slots {
                let c = costs[costs.len() - 1] + kernels::cost(p, w, *slot);
                costs.push(c);
            }
            let mut row = path_row(p, rows.len(), &nodes, &slots, &costs);
            row.push(Value::Bool(true));
            r.emit(&mut rows, row)?;
        }
    } else {
        for t in 0..p.node_count() {
            if let Some((nodes, slots)) = tree.walk(t) {
                let costs = tree.costs(&nodes);
                let mut row = path_row(p, rows.len(), &nodes, &slots, &costs);
                row.push(Value::Bool(false));
                r.emit(&mut rows, row)?;
            }
        }
    }
    Ok(Output::Rows {
        rows,
        extra: vec![("containsNegativeCycle", Value::Bool(contains))],
    })
}

fn run_negative_cycle(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => find_negative_cycle(p, r))
}

fn find_negative_cycle<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    numeric(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    r.mem.grow(4 * p.node_count() * 16)?;
    let mut rows = Vec::new();
    let found = petgraph::algo::find_negative_cycle(p, I::new(s));
    trace!(
        "petgraph",
        "{:?}",
        found
            .as_ref()
            .map(|c| c.iter().map(|v| v.index()).collect::<Vec<_>>())
    );
    if let Some(cycle) = found {
        let mut nodes: Vec<usize> = cycle.iter().map(|v| v.index()).collect();
        if let Some(first) = nodes.first().copied() {
            nodes.push(first);
        }
        let (slots, _) = kernels::path_edges(p, w, &nodes);
        let ids: Vec<Value> = nodes.iter().map(|v| id_value(p, *v)).collect();
        let path = kernels::path_value(p, &nodes, &slots);
        r.emit(&mut rows, vec![Value::List(ids.into()), path])?;
    }
    Ok(rows_output(rows))
}

fn run_spfa(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => spfa(p, r))
}

fn spfa<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    numeric(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let n = p.node_count();
    r.mem.grow(4 * n * 16)?;
    let paths = petgraph::algo::spfa(p, I::new(s), |e| kernels::cost(p, w, e.id()))
        .map_err(|_| negative_cycle())?;
    trace!(
        "petgraph",
        "{:?} {:?}",
        paths.distances,
        paths
            .predecessors
            .iter()
            .map(|x| x.map(|v| v.index()))
            .collect::<Vec<_>>()
    );
    let mut t = Tree::new(n);
    for v in 0..n {
        if let Some(u) = paths.predecessors[v] {
            t.pred[v] = u.index();
            t.slot[v] = kernels::best_slot(p, w, u.index(), v).map_or(NONE, |b| b.0);
            t.dist[v] = paths.distances[v];
        }
    }
    t.dist[s] = 0.0;
    t.pred[s] = NONE;
    let mut rows = Vec::new();
    for v in 0..n {
        emit_tree_path(p, r, &mut rows, &t, v)?;
    }
    Ok(rows_output(rows))
}

fn run_delta(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => delta(p, r))
}

/// Delta-stepping from `s` (module documentation).
fn delta<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let delta = r.cfg.float("delta");
    let n = p.node_count();
    r.mem.grow(n * 64 + p.edge_ids.len() * 32)?;
    let mut t = Tree::new(n);
    t.dist[s] = 0.0;
    let bucket = |d: f64| (d / delta).floor() as u64;
    let mut buckets: BTreeMap<u64, Vec<usize>> = BTreeMap::from([(0, vec![s])]);
    // A relaxation request: target, new distance, predecessor, edge.
    type Request = (usize, f64, usize, usize);
    let requests = |frontier: &[usize], dist: &[f64], light: bool| {
        r.sched.map(|_, range| {
            let lo = frontier.partition_point(|v| *v < range.start);
            let hi = frontier.partition_point(|v| *v < range.end);
            let mut out: Vec<Request> = Vec::new();
            for &v in &frontier[lo..hi] {
                for (x, slot) in p.out(v) {
                    let c = kernels::cost(p, w, slot);
                    if (c <= delta) == light {
                        out.push((x.index(), dist[v] + c, v, slot));
                    }
                }
            }
            out
        })
    };
    while let Some((&i, _)) = buckets.iter().next() {
        r.mem.check_now()?;
        let mut settled: Vec<usize> = Vec::new();
        while let Some(mut frontier) = buckets.remove(&i) {
            frontier.sort_unstable();
            frontier.dedup();
            frontier.retain(|v| bucket(t.dist[*v]) == i);
            if frontier.is_empty() {
                break;
            }
            trace!("light", "{i} {frontier:?}");
            settled.extend_from_slice(&frontier);
            let reqs = requests(&frontier, &t.dist, true)?;
            for (x, d, v, slot) in reqs.into_iter().flatten() {
                if d < t.dist[x] {
                    trace!("relax", "{x} {d:?} {v}");
                    t.dist[x] = d;
                    t.pred[x] = v;
                    t.slot[x] = slot;
                    buckets.entry(bucket(d)).or_default().push(x);
                }
            }
        }
        settled.sort_unstable();
        settled.dedup();
        trace!("heavy", "{i} {settled:?}");
        let reqs = requests(&settled, &t.dist, false)?;
        for (x, d, v, slot) in reqs.into_iter().flatten() {
            if d < t.dist[x] {
                trace!("relax", "{x} {d:?} {v}");
                t.dist[x] = d;
                t.pred[x] = v;
                t.slot[x] = slot;
                buckets.entry(bucket(d)).or_default().push(x);
            }
        }
        // Drop entries of buckets below the next one: none remain, since
        // every request lands at its own distance's bucket, at least i.
        buckets.retain(|k, _| *k > i);
    }
    let mut rows = Vec::new();
    for v in 0..n {
        emit_tree_path(p, r, &mut rows, &t, v)?;
    }
    Ok(rows_output(rows))
}

/// Emits `(source, target, distance)` rows from per-source distance
/// arrays, reachable targets only.
fn emit_distances<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    rows: &mut Vec<Vec<Value>>,
    s: usize,
    dist: &[f64],
) -> Result<(), Error> {
    for (t, d) in dist.iter().enumerate() {
        if d.is_finite() {
            r.emit(rows, vec![id_value(p, s), id_value(p, t), Value::Float(*d)])?;
        }
    }
    Ok(())
}

fn run_johnson(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => johnson(p, r))
}

/// Johnson's all-pairs shortest paths (module documentation).
fn johnson<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    numeric(p, w)?;
    let n = p.node_count();
    let (h, changing) = bellman_ford_rounds(p, r, w, &[])?;
    if !changing.is_empty() {
        return Err(negative_cycle());
    }
    let h = h.dist;
    trace!("potential", "{h:?}");
    r.mem.grow(
        n.saturating_mul(n).saturating_mul(size_of::<f64>())
            + r.sched_workers() * kernels::dijkstra_bytes(n, p.edge_ids.len()),
    )?;
    let blocks = r.sched.map(|_, range| {
        range
            .map(|s| {
                let d = reweighted_dijkstra(p, s, &h, w);
                trace!("source", "{s} {d:?}");
                (s, d)
            })
            .collect::<Vec<_>>()
    })?;
    let mut rows = Vec::new();
    for (s, dist) in blocks.into_iter().flatten() {
        emit_distances(p, r, &mut rows, s, &dist)?;
    }
    Ok(rows_output(rows))
}

/// Dijkstra from `s` on Johnson's reweighted costs `c(u, x) + h(u) −
/// h(x)` for the step from `u` to `x`, returning the true distances.
fn reweighted_dijkstra<I: Ix>(p: &Projection<I>, s: usize, h: &[f64], w: bool) -> Vec<f64> {
    let n = p.node_count();
    let mut dist = vec![f64::INFINITY; n];
    let mut visited = vec![false; n];
    let mut heap = BinaryHeap::new();
    dist[s] = 0.0;
    heap.push(MinScored(0.0, s));
    while let Some(MinScored(d, u)) = heap.pop() {
        if visited[u] {
            continue;
        }
        visited[u] = true;
        for (x, slot) in p.out(u) {
            let x = x.index();
            if visited[x] {
                continue;
            }
            let next = d + (kernels::cost(p, w, slot) + h[u] - h[x]);
            if next < dist[x] {
                dist[x] = next;
                heap.push(MinScored(next, x));
            }
        }
    }
    for (t, d) in dist.iter_mut().enumerate() {
        if d.is_finite() {
            *d = *d + h[t] - h[s];
        }
    }
    dist
}

impl Run<'_, '_> {
    /// The number of workers the run's scheduler starts.
    pub(crate) fn sched_workers(&self) -> usize {
        self.sched.worker_count()
    }
}

fn run_floyd(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => floyd(p, r))
}

fn floyd<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    numeric(p, w)?;
    let n = p.node_count();
    // A negative self-loop is a negative cycle, which petgraph's
    // `floyd_warshall` misses: it sets each node's distance to itself to
    // 0 after loading the edges (`floyd_warshall.rs:284-288`; rmp #71).
    if (0..n).any(|v| {
        p.out(v)
            .any(|(x, slot)| x.index() == v && kernels::cost(p, w, slot) < 0.0)
    }) {
        return Err(negative_cycle());
    }
    // petgraph's matrix and its map of every pair.
    r.mem.grow(n.saturating_mul(n).saturating_mul(8 + 48))?;
    let d = petgraph::algo::floyd_warshall(p, |e| kernels::cost(p, w, e.id()))
        .map_err(|_| negative_cycle())?;
    trace!("petgraph", "{} pairs", d.len());
    let mut pairs: Vec<((usize, usize), f64)> = d
        .into_iter()
        .filter(|(_, x)| *x < f64::MAX)
        .map(|((a, b), x)| ((a.index(), b.index()), x))
        .collect();
    pairs.sort_unstable_by_key(|x| x.0);
    let mut rows = Vec::new();
    for ((s, t), x) in pairs {
        r.emit(
            &mut rows,
            vec![id_value(p, s), id_value(p, t), Value::Float(x)],
        )?;
    }
    Ok(rows_output(rows))
}

fn run_seidel(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => seidel(p, r))
}

/// Seidel's all-pairs shortest path lengths per connected component
/// (module documentation).
fn seidel<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let mut comp = r.mem.vec(n, NONE)?;
    let mut components: Vec<Vec<usize>> = Vec::new();
    for s in 0..n {
        if comp[s] != NONE {
            continue;
        }
        let c = components.len();
        comp[s] = c;
        let mut members = vec![s];
        let mut at = 0;
        while at < members.len() {
            let v = members[at];
            at += 1;
            for (x, _) in p.out(v) {
                if comp[x.index()] == NONE {
                    comp[x.index()] = c;
                    members.push(x.index());
                }
            }
        }
        members.sort_unstable();
        components.push(members);
    }
    let mut dist: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    for members in &components {
        let k = members.len();
        r.mem.grow(k.saturating_mul(k).saturating_mul(8 * 8 + 48))?;
        r.mem.check_now()?;
        let local = |v: usize| members.binary_search(&v).expect("a member");
        let mut a = DMatrix::<u64>::zeros(k, k);
        for (i, v) in members.iter().enumerate() {
            for (x, _) in p.out(*v) {
                let j = local(x.index());
                if i != j {
                    a[(i, j)] = 1;
                    a[(j, i)] = 1;
                }
            }
        }
        trace!("component", "{members:?}");
        let d = apd(a);
        for (i, v) in members.iter().enumerate() {
            for (j, x) in members.iter().enumerate() {
                dist.insert((*v, *x), d[(i, j)]);
            }
        }
    }
    let mut rows = Vec::new();
    for ((s, t), d) in dist {
        r.emit(
            &mut rows,
            vec![id_value(p, s), id_value(p, t), Value::Float(d as f64)],
        )?;
    }
    Ok(rows_output(rows))
}

/// graphalgs's `apd` (module documentation): the distance matrix of the
/// connected graph with 0/1 adjacency matrix `a`.
#[allow(non_snake_case, reason = "the names of Seidel's paper and graphalgs")]
fn apd(A: DMatrix<u64>) -> DMatrix<u64> {
    let n = A.nrows();
    if (0..n).all(|i| (0..n).all(|j| i == j || A[(i, j)] != 0)) {
        trace!("apd", "complete");
        return A;
    }
    let Z = &A * &A;
    let mut B = DMatrix::<u64>::zeros(n, n);
    for i in 0..n {
        for j in 0..n {
            if i != j && (A[(i, j)] == 1 || Z[(i, j)] > 0) {
                B[(i, j)] = 1;
            }
        }
    }
    let T = apd(B);
    let X = &T * &A;
    let degree: Vec<u64> = A.row_iter().map(|row| row.sum()).collect();
    let mut D = DMatrix::<u64>::zeros(n, n);
    for i in 0..n {
        for j in 0..n {
            D[(i, j)] = if X[(i, j)] >= T[(i, j)] * degree[j] {
                2 * T[(i, j)]
            } else {
                2 * T[(i, j)] - 1
            };
        }
    }
    trace!(
        "apd",
        "{:?}",
        (0..n)
            .map(|i| (0..n).map(|j| D[(i, j)]).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
    D
}

fn run_all_pairs(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => all_pairs(p, r))
}

/// All-pairs shortest distances: one search per source, sources by
/// blocks (module documentation).
fn all_pairs<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let n = p.node_count();
    r.mem.grow(
        n.saturating_mul(n).saturating_mul(size_of::<f64>())
            + r.sched_workers() * kernels::dijkstra_bytes(n, p.edge_ids.len()),
    )?;
    let blocks = r.sched.map(|_, range| {
        range
            .map(|s| {
                let tree = if w {
                    let cost = |slot| kernels::cost(p, w, slot);
                    kernels::dijkstra(p, s, &cost, None)
                } else {
                    bfs_tree(p, s, true)
                };
                trace!("source", "{s} {:?}", tree.dist);
                (s, tree.dist)
            })
            .collect::<Vec<_>>()
    })?;
    let mut rows = Vec::new();
    for (s, mut dist) in blocks.into_iter().flatten() {
        if !w {
            // GDS's unweighted form (multi-source BFS) has no row from a
            // node to itself; its weighted form has (measured, rmp #64).
            dist[s] = f64::INFINITY;
        }
        emit_distances(p, r, &mut rows, s, &dist)?;
    }
    Ok(rows_output(rows))
}

fn run_all_shortest_between(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => all_shortest_between(p, r))
}

/// Every shortest path from the source to the target: Dijkstra's
/// distances, then the predecessors `u` of each node `v` with `d(u) +
/// c(u, v) = d(v)`, the cheapest edge of each pair, as NetworkX's
/// `all_shortest_paths` builds them (`shortest_paths/generic.py`,
/// `_build_paths_from_predecessors`; BSD-3-Clause, method only); paths
/// are enumerated from the target back, with an explicit stack.
fn all_shortest_between<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let t = key_node(p, r, "targetNode")?;
    let n = p.node_count();
    r.mem
        .grow(2 * kernels::dijkstra_bytes(n, p.edge_ids.len()) + p.edge_ids.len() * 16)?;
    let cost = |slot| kernels::cost(p, w, slot);
    let tree = kernels::dijkstra(p, s, &cost, None);
    let mut rows = Vec::new();
    if tree.dist[t].is_infinite() {
        return Ok(rows_output(rows));
    }
    // The predecessors of each node on a shortest path: (node, edge).
    let preds = |v: usize| -> Vec<(usize, usize)> {
        let mut best: BTreeMap<usize, (usize, f64)> = BTreeMap::new();
        for (u, slot) in p.inc(v) {
            let u = u.index();
            if u == v || tree.dist[u].is_infinite() {
                continue;
            }
            let c = cost(slot);
            let e = best.entry(u).or_insert((slot, c));
            if c < e.1 {
                *e = (slot, c);
            }
        }
        let out: Vec<(usize, usize)> = best
            .into_iter()
            .filter(|(u, (_, c))| tree.dist[*u] + c == tree.dist[v])
            .map(|(u, (slot, _))| (u, slot))
            .collect();
        trace!(
            "preds",
            "{v} {:?}",
            out.iter().map(|x| x.0).collect::<Vec<_>>()
        );
        out
    };
    // Depth-first from the target: a stack of (node, its predecessors,
    // the next one).
    let mut nodes = vec![t];
    let mut slots: Vec<usize> = Vec::new();
    let mut stack = vec![(preds(t), 0usize)];
    loop {
        let v = *nodes.last().expect("a node");
        if v == s {
            let mut ns = nodes.clone();
            let mut ss = slots.clone();
            ns.reverse();
            ss.reverse();
            trace!("path", "{ns:?}");
            let costs = tree.costs(&ns);
            let row = path_row(p, rows.len(), &ns, &ss, &costs);
            r.emit(&mut rows, row)?;
            stack.pop();
            nodes.pop();
            slots.pop();
        }
        let Some((ps, at)) = stack.last_mut() else {
            break;
        };
        if let Some(&(u, slot)) = ps.get(*at) {
            *at += 1;
            nodes.push(u);
            slots.push(slot);
            let next = if u == s { Vec::new() } else { preds(u) };
            stack.push((next, 0));
            if u != s && stack.len() % 1024 == 0 {
                r.mem.check_now()?;
            }
        } else {
            stack.pop();
            nodes.pop();
            slots.pop();
            if stack.is_empty() {
                break;
            }
        }
    }
    Ok(rows_output(rows))
}

fn run_yens(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => yens_entry(p, r))
}

/// A route of [`yens`]: its edges, nodes, cost and spur index.
#[derive(Debug, Clone)]
struct Route {
    nodes: Vec<usize>,
    slots: Vec<usize>,
    cost: f64,
    spur: usize,
}

impl Route {
    fn key(&self) -> (f64, usize, &[usize]) {
        (self.cost, self.nodes.len(), &self.slots)
    }
}

impl PartialEq for Route {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Route {}

impl PartialOrd for Route {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Route {
    /// pathfinding's order (cost, then length), then the edges, so that
    /// equal candidates pop in one order.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let (a, b) = (self.key(), other.key());
        a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2))
    }
}

fn yens_entry<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let t = key_node(p, r, "targetNode")?;
    let k = usize::try_from(r.cfg.int("k")).unwrap_or(usize::MAX);
    let routes = yens(p, r, w, s, t, k)?;
    let mut rows = Vec::new();
    for route in routes {
        let mut costs = vec![0.0];
        for slot in &route.slots {
            let c = costs[costs.len() - 1] + kernels::cost(p, w, *slot);
            costs.push(c);
        }
        let row = path_row(p, rows.len(), &route.nodes, &route.slots, &costs);
        r.emit(&mut rows, row)?;
    }
    Ok(rows_output(rows))
}

/// Yen's `k` shortest paths from `s` to `t` (module documentation).
fn yens<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    w: bool,
    s: usize,
    t: usize,
    k: usize,
) -> Result<Vec<Route>, Error> {
    let n = p.node_count();
    let m = p.edge_ids.len();
    r.mem
        .grow((r.sched_workers() + 1) * (kernels::dijkstra_bytes(n, m) + n + m))?;
    let cost = |slot| kernels::cost(p, w, slot);
    let first = kernels::dijkstra(p, s, &cost, Some(t));
    let Some((nodes, slots)) = first.walk(t) else {
        return Ok(Vec::new());
    };
    let mut routes = vec![Route {
        cost: first.dist[t],
        nodes,
        slots,
        spur: 0,
    }];
    let mut visited: BTreeSet<Vec<usize>> = BTreeSet::new();
    let mut candidates: BinaryHeap<std::cmp::Reverse<Route>> = BinaryHeap::new();
    for ki in 0..k.saturating_sub(1) {
        if routes.len() <= ki || routes.len() == k {
            break;
        }
        r.mem.check_now()?;
        let previous = routes[ki].clone();
        let mut root_costs = vec![0.0];
        for slot in &previous.slots {
            let c = root_costs[root_costs.len() - 1] + cost(*slot);
            root_costs.push(c);
        }
        // The spur positions of this round, by node index for the blocks.
        let mut spurs: Vec<(usize, usize)> = (previous.spur..previous.nodes.len() - 1)
            .map(|i| (previous.nodes[i], i))
            .collect();
        spurs.sort_unstable();
        let found = &routes;
        let parts = r.sched.map(|_, range| {
            let lo = spurs.partition_point(|(v, _)| *v < range.start);
            let hi = spurs.partition_point(|(v, _)| *v < range.end);
            let mut out = Vec::new();
            for &(spur_node, i) in &spurs[lo..hi] {
                let root_nodes = &previous.nodes[..i];
                let root_slots = &previous.slots[..i];
                let mut removed_edges = vec![false; m];
                for route in found {
                    if route.slots.len() > i
                        && route.nodes[..i] == *root_nodes
                        && route.slots[..i] == *root_slots
                        && route.nodes[i] == spur_node
                    {
                        removed_edges[route.slots[i]] = true;
                    }
                }
                let mut removed_nodes = vec![false; n];
                for v in root_nodes {
                    removed_nodes[*v] = true;
                }
                let tree = filtered_dijkstra(p, w, spur_node, t, &removed_nodes, &removed_edges);
                if let Some((spur_nodes, spur_slots)) = tree.walk(t) {
                    let mut nodes = root_nodes.to_vec();
                    nodes.extend(spur_nodes);
                    let mut slots = root_slots.to_vec();
                    slots.extend(spur_slots);
                    out.push((
                        i,
                        Route {
                            cost: root_costs[i] + tree.dist[t],
                            nodes,
                            slots,
                            spur: i,
                        },
                    ));
                }
            }
            out
        })?;
        let mut round: Vec<(usize, Route)> = parts.into_iter().flatten().collect();
        round.sort_by_key(|(i, _)| *i);
        trace!(
            "round",
            "{ki} spurs {}..{}",
            previous.spur,
            previous.nodes.len() - 1
        );
        for (_, route) in round {
            if visited.insert(route.slots.clone()) {
                trace!(
                    "candidate",
                    "{:?} {:?} {}", route.cost, route.nodes, route.spur
                );
                candidates.push(std::cmp::Reverse(route));
            }
        }
        if let Some(std::cmp::Reverse(route)) = candidates.pop() {
            let c = route.cost;
            trace!("accept", "{c:?} {:?}", route.nodes);
            routes.push(route);
            while routes.len() < k {
                match candidates.peek() {
                    Some(std::cmp::Reverse(next)) if next.cost == c => {
                        let std::cmp::Reverse(next) = candidates.pop().expect("peeked");
                        trace!("accept", "{c:?} {:?}", next.nodes);
                        routes.push(next);
                    }
                    _ => break,
                }
            }
        }
    }
    routes.sort();
    routes.truncate(k);
    Ok(routes)
}

/// Dijkstra from `s` to `t` without the removed nodes and edges.
fn filtered_dijkstra<I: Ix>(
    p: &Projection<I>,
    w: bool,
    s: usize,
    t: usize,
    removed_nodes: &[bool],
    removed_edges: &[bool],
) -> Tree {
    let cost = |slot: usize| {
        if removed_edges[slot] {
            f64::INFINITY
        } else {
            kernels::cost(p, w, slot)
        }
    };
    let n = p.node_count();
    let mut tree = Tree::new(n);
    let mut visited = removed_nodes.to_vec();
    let mut heap = BinaryHeap::new();
    tree.dist[s] = 0.0;
    heap.push(MinScored(0.0, s));
    while let Some(MinScored(d, u)) = heap.pop() {
        if visited[u] {
            continue;
        }
        if u == t {
            break;
        }
        visited[u] = true;
        for (x, slot) in p.out(u) {
            let x = x.index();
            if visited[x] {
                continue;
            }
            let next = d + cost(slot);
            if next < tree.dist[x] {
                tree.dist[x] = next;
                tree.pred[x] = u;
                tree.slot[x] = slot;
                heap.push(MinScored(next, x));
            }
        }
    }
    tree
}

fn run_k_shortest(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => k_shortest(p, r))
}

fn k_shortest<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    non_negative(p, w)?;
    let s = key_node(p, r, "sourceNode")?;
    let goal = match r.cfg.get("targetNode") {
        Value::Null => None,
        v => Some(node_index(p, v, "targetNode")?),
    };
    let k = usize::try_from(r.cfg.int("k")).unwrap_or(usize::MAX);
    let n = p.node_count();
    // petgraph's counters, map and heap.
    r.mem
        .grow(n.saturating_mul(64) + n.saturating_mul(k).saturating_mul(32))?;
    let d = petgraph::algo::k_shortest_path(p, I::new(s), goal.map(I::new), k, |e| {
        kernels::cost(p, w, e.id())
    });
    let mut pairs: Vec<(usize, f64)> = d.into_iter().map(|(v, c)| (v.index(), c)).collect();
    pairs.sort_unstable_by_key(|x| x.0);
    trace!("petgraph", "{pairs:?}");
    let mut rows = Vec::new();
    for (v, c) in pairs {
        r.emit(&mut rows, vec![id_value(p, v), Value::Float(c)])?;
    }
    Ok(rows_output(rows))
}

fn run_all_simple(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => all_simple(p, r))
}

/// A direction of MAGE's `PathFinder` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelDir {
    None,
    Any,
    Incoming,
    Outgoing,
    Both,
}

/// MAGE's `algo.all_simple_paths` (module documentation).
fn all_simple<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let start = node_index(p, &r.args[0], "start_node")?;
    let end = node_index(p, &r.args[1], "end_node")?;
    let names: Vec<String> = match &r.args[2] {
        Value::List(l) => l
            .iter()
            .map(|x| match x {
                Value::String(s) => Ok(s.to_string()),
                _ => Err(Error::new(
                    codes::C22G03,
                    "invalid value type: relationship_types must be a list of strings",
                )),
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(Error::new(
                codes::C22G03,
                "invalid value type: relationship_types must be a list of strings",
            ));
        }
    };
    let Value::Int(max_length) = r.args[3] else {
        return Err(Error::new(
            codes::C22G03,
            "invalid value type: max_length must be an integer",
        ));
    };
    // `UpdateRelationshipDirection`.
    let (mut any_in, mut any_out, mut all_in, mut all_out) = (false, false, false, false);
    let mut dirs: Vec<(Option<u64>, RelDir)> = Vec::new();
    if names.is_empty() {
        any_in = true;
        any_out = true;
    } else {
        let (mut in_rel, mut out_rel) = (false, false);
        for t in &names {
            let starts = t.starts_with('<');
            let ends = t.ends_with('>');
            if t.len() == 1 {
                if starts {
                    any_in = true;
                    in_rel = true;
                } else if ends {
                    any_out = true;
                    out_rel = true;
                } else {
                    dirs.push((r.token(TokenKind::EdgeType, t), RelDir::Any));
                    in_rel = true;
                    out_rel = true;
                }
                continue;
            }
            let (name, dir) = match (starts, ends) {
                (true, true) => (&t[1..t.len() - 1], RelDir::Both),
                (true, false) => (&t[1..], RelDir::Incoming),
                (false, true) => (&t[..t.len() - 1], RelDir::Outgoing),
                (false, false) => (&t[..], RelDir::Any),
            };
            match dir {
                RelDir::Incoming => in_rel = true,
                RelDir::Outgoing => out_rel = true,
                _ => {
                    in_rel = true;
                    out_rel = true;
                }
            }
            dirs.push((r.token(TokenKind::EdgeType, name), dir));
        }
        if !in_rel {
            all_out = true;
        } else if !out_rel {
            all_in = true;
        }
    }
    let direction = |ty: u64| {
        dirs.iter()
            .rev()
            .find(|(t, _)| *t == Some(ty))
            .map_or(RelDir::None, |(_, d)| *d)
    };
    // The edges `DFS` follows from `v`: incoming then outgoing.
    let follow = |v: usize, visited: &[bool]| -> Vec<(usize, usize)> {
        let mut seen: Vec<usize> = Vec::new();
        let mut out = Vec::new();
        let sides: &[(bool, bool)] = &[(false, !all_out), (true, !all_in)];
        for &(outgoing, active) in sides {
            if !active {
                continue;
            }
            let always = if outgoing { any_out } else { any_in };
            let adj: Vec<(usize, usize)> = if outgoing {
                p.out(v).map(|(x, s)| (x.index(), s)).collect()
            } else {
                p.inc(v).map(|(x, s)| (x.index(), s)).collect()
            };
            for (x, slot) in adj {
                if visited[x] {
                    continue;
                }
                let wanted = direction(p.edge_types[slot]);
                let current = if outgoing {
                    RelDir::Outgoing
                } else {
                    RelDir::Incoming
                };
                if always || wanted == RelDir::Any || wanted == current {
                    out.push((x, slot));
                } else if wanted == RelDir::Both {
                    if outgoing && seen.contains(&x) {
                        out.push((x, slot));
                    } else if !outgoing {
                        seen.push(x);
                    }
                }
            }
        }
        out
    };
    let n = p.node_count();
    let mut visited = r.mem.vec(n, false)?;
    r.mem.grow(4 * n * size_of::<usize>())?;
    let mut rows = Vec::new();
    let mut nodes = vec![start];
    let mut slots: Vec<usize> = Vec::new();
    let mut frames: Vec<(Vec<(usize, usize)>, usize)> = Vec::new();
    loop {
        // Enter the last node of the path (MAGE `DFS`).
        let v = *nodes.last().expect("a node");
        trace!(
            "enter",
            "{:?}",
            nodes
                .iter()
                .zip(slots.iter().map(|x| Some(p.edge_id(*x))).chain([None]))
                .flat_map(|(n, e)| [Some(*n as u64), e])
                .flatten()
                .collect::<Vec<_>>()
        );
        let next = if v == end {
            trace!("emit", "");
            let path = kernels::path_value(p, &nodes, &slots);
            r.emit(&mut rows, vec![path])?;
            Vec::new()
        } else if slots.len() as i64 == max_length {
            Vec::new()
        } else {
            visited[v] = true;
            r.mem.check_now()?;
            follow(v, &visited)
        };
        frames.push((next, 0));
        loop {
            let Some((edges, at)) = frames.last_mut() else {
                return Ok(rows_output(rows));
            };
            if let Some(&(x, slot)) = edges.get(*at) {
                *at += 1;
                nodes.push(x);
                slots.push(slot);
                break;
            }
            frames.pop();
            let left = nodes.pop().expect("a node");
            visited[left] = false;
            if slots.pop().is_none() {
                return Ok(rows_output(rows));
            }
        }
    }
}

fn run_longest_path(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => longest_path(p, r))
}

/// The longest path to every node of the DAG part of the projection
/// (module documentation): level by level, each node takes the largest
/// `d(u) + c(u, v)` over its incoming edges (the first of equal ones).
/// A node in no level (on or after a cycle) that a levelled node points
/// to also gets the largest such distance over its levelled
/// predecessors: GDS relaxes every edge out of a processed node,
/// whatever its target (`DagLongestPath.java` `longestPathTraverse`;
/// measured on `zipf60`, rmp #64).
fn longest_path<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let w = weighted(r);
    numeric(p, w)?;
    let levels = kernels::levels(p, r.sched, r.mem)?;
    let n = p.node_count();
    r.mem.grow(n * 32)?;
    let mut t = Tree::new(n);
    for level in &levels {
        let dist = &t.dist;
        let parts = r.sched.map(|_, range| {
            let lo = level.partition_point(|v| *v < range.start);
            let hi = level.partition_point(|v| *v < range.end);
            level[lo..hi]
                .iter()
                .map(|&v| {
                    let mut best = (0.0, NONE, NONE);
                    for (u, slot) in p.inc(v) {
                        let c = dist[u.index()] + kernels::cost(p, w, slot);
                        if best.1 == NONE || c > best.0 {
                            best = (c, u.index(), slot);
                        }
                    }
                    (v, best)
                })
                .collect::<Vec<_>>()
        })?;
        for (v, (d, u, slot)) in parts.into_iter().flatten() {
            trace!(
                "settle",
                "{v} {d:?} {}",
                if u == NONE { -1 } else { u as i64 }
            );
            t.dist[v] = d;
            t.pred[v] = u;
            t.slot[v] = slot;
        }
    }
    let mut in_dag = vec![false; n];
    for v in levels.iter().flatten() {
        in_dag[*v] = true;
    }
    let mut reached = in_dag.clone();
    for v in (0..n).filter(|v| !in_dag[*v]) {
        let mut best = (0.0, NONE, NONE);
        for (u, slot) in p.inc(v) {
            if in_dag[u.index()] {
                let c = t.dist[u.index()] + kernels::cost(p, w, slot);
                if best.1 == NONE || c > best.0 {
                    best = (c, u.index(), slot);
                }
            }
        }
        if best.1 != NONE {
            (t.dist[v], t.pred[v], t.slot[v]) = best;
            reached[v] = true;
        }
    }
    let mut rows = Vec::new();
    for v in (0..n).filter(|v| reached[*v]) {
        emit_tree_path(p, r, &mut rows, &t, v)?;
    }
    Ok(rows_output(rows))
}
