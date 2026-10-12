//! The isomorphism and graph recognition family and the derived
//! structures family (rmp #69): ALG-189 graph isomorphism, ALG-190
//! subgraph isomorphism, ALG-194 bipartiteness, ALG-203 tournament tests,
//! ALG-204 degree-sequence realisability, ALG-205 transitive closure,
//! ALG-206 transitive reduction. All run on one worker.
//!
//! * **189, 190** call petgraph 0.8.3's VF2 (`is_isomorphic_matching`,
//!   `is_isomorphic_subgraph_matching`) on petgraph `Graph`s built from the
//!   two projections (`nodeLabels`/`relationshipTypes` and
//!   `otherNodeLabels`/`otherRelationshipTypes`): petgraph's VF2 expects
//!   no parallel edges (`isomorphism.rs:793`), so each node pair holds one
//!   edge weighted by its number of parallel edges, and edges match when
//!   their counts are equal (`algorithm-entries.md` Signatures). The
//!   projection's own wrapper lacks `GetAdjacencyMatrix`, which VF2 needs.
//!   190 asks whether the second projection is isomorphic to a
//!   node-induced subgraph of the first (`isomorphism.rs:870-894`).
//! * **194** copies rustworkx-core @ `25398ab47f66`
//!   `rustworkx-core/src/coloring.rs` (`two_color`; Apache-2.0, see
//!   `LICENSE-APACHE`; the rustworkx contributors, no copyright line in
//!   the file): each component from its first node, colour 1, a stack,
//!   neighbours over both directions, a conflict ends the run. Modified:
//!   dense arrays instead of a map; the colour is the side.
//! * **203, 204** copy graphalgs @ `1f8a412ef4af` `src/tournament.rs`
//!   (`is_tournament`, `is_tournament_transitive`) and `src/spec/mod.rs`
//!   (`is_degree_sequence_graphlike`; MIT, "Copyright (c) 2021
//!   PrototypeRailGun", licence text in [`super::paths`]). Modified: the
//!   empty graph is a tournament (graphalgs computes `n − 1` and
//!   underflows; NetworkX's `is_tournament` holds it true); `transitive`
//!   is reported only for a tournament, false otherwise.
//! * **205, 206** call petgraph's `dag_transitive_reduction_closure` on
//!   the toposorted adjacency list that petgraph's
//!   `dag_to_toposorted_adjacency_list` (`tred.rs:71`, MIT, licence text
//!   in [`super::projection`]) builds, copied here with parallel edges
//!   kept once: petgraph's closure adds a parallel edge's target twice.
//!   The order is Kahn's levels ([`super::kernels::levels`]); a node in no
//!   level lies on or after a cycle, and the run fails with 22N11
//!   (`algorithm-entries.md`). 206's kept edge of a pair is its first in
//!   forward order.

use petgraph::Direction;
use petgraph::adj::List;
use petgraph::graph::{DiGraph, NodeIndex, UnGraph};
use petgraph::visit::IntoNeighbors;

use super::config::{Def, Key, NODE_WRITE, REL_WRITE, Ty};
use super::kernels::{self, id_value};
use super::projection::{Any, Ix, Orientation, Projection, with_projection};
use super::{EDGE_PROP_WRITE_COLUMNS, Entry, Output, REL_WRITE_COLUMNS, Run, WriteKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

const ISO_KEYS: &[Key] = &[
    Key::new("otherNodeLabels", Ty::Names, Def::Required),
    Key::new("otherRelationshipTypes", Ty::Names, Def::Required),
];

/// ALG-189: `gds.isIsomorphic.stream`.
pub(crate) const IS_ISOMORPHIC: Entry = Entry {
    algs: &[189],
    stream: "gds.isIsomorphic.stream",
    keys: ISO_KEYS,
    stream_columns: &["isomorphic"],
    run: run_isomorphic,
    second: true,
    ..Entry::BASE
};

/// ALG-190: `gds.isIsomorphicSubgraph.stream`.
pub(crate) const IS_ISOMORPHIC_SUBGRAPH: Entry = Entry {
    algs: &[190],
    stream: "gds.isIsomorphicSubgraph.stream",
    keys: ISO_KEYS,
    stream_columns: &["isomorphic"],
    run: run_isomorphic_subgraph,
    second: true,
    ..Entry::BASE
};

/// ALG-194: `gds.isBipartiteUndirected.stream` and `.write`.
pub(crate) const BIPARTITE: Entry = Entry {
    algs: &[194],
    stream: "gds.isBipartiteUndirected.stream",
    write: Some("gds.isBipartiteUndirected.write"),
    undirected: true,
    stream_columns: &["nodeId", "side"],
    write_columns: &["bipartite", "nodePropertiesWritten", "configuration"],
    run: run_bipartite,
    write_keys: NODE_WRITE,
    write_kind: WriteKind::NodeProp { id: 0, value: 1 },
    ..Entry::BASE
};

/// ALG-203: `gds.isTournament.stream`.
pub(crate) const TOURNAMENT: Entry = Entry {
    algs: &[203],
    stream: "gds.isTournament.stream",
    stream_columns: &["tournament", "transitive"],
    run: run_tournament,
    ..Entry::BASE
};

/// ALG-204: `gds.isDegreeSequenceGraphlike.stream`.
pub(crate) const GRAPHLIKE: Entry = Entry {
    algs: &[204],
    stream: "gds.isDegreeSequenceGraphlike.stream",
    keys: &[Key::new("degreeSequence", Ty::Ints, Def::Required)],
    stream_columns: &["graphlike"],
    run: run_graphlike,
    ..Entry::BASE
};

/// ALG-205: `gds.transitiveClosure.stream` and `.write`.
pub(crate) const CLOSURE: Entry = Entry {
    algs: &[205],
    stream: "gds.transitiveClosure.stream",
    write: Some("gds.transitiveClosure.write"),
    stream_columns: &["sourceNode", "targetNode"],
    write_columns: REL_WRITE_COLUMNS,
    run: run_closure,
    write_keys: REL_WRITE,
    write_kind: WriteKind::Rels { src: 0, dst: 1 },
    ..Entry::BASE
};

/// ALG-206: `gds.transitiveReduction.stream` and `.write`.
pub(crate) const REDUCTION: Entry = Entry {
    algs: &[206],
    stream: "gds.transitiveReduction.stream",
    write: Some("gds.transitiveReduction.write"),
    stream_columns: &["sourceNode", "targetNode", "relationshipId"],
    write_columns: EDGE_PROP_WRITE_COLUMNS,
    run: run_reduction,
    write_kind: WriteKind::EdgeFlag { edge: 2 },
    ..Entry::BASE
};

fn one_row(r: &mut Run<'_, '_>, row: Vec<Value>) -> Result<Output, Error> {
    let mut rows = Vec::new();
    r.emit(&mut rows, row)?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// The edges of a projection as one weighted edge per node pair, the
/// weight the number of parallel edges (module documentation), over the
/// stored direction; under `UNDIRECTED` each pair is unordered.
fn counted<I: Ix>(p: &Projection<I>) -> Vec<(usize, usize, u32)> {
    let undirected = p.orientation == Orientation::Undirected;
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for u in 0..p.node_count() {
        for slot in p.fwd.row[u]..p.fwd.row[u + 1] {
            let v = p.fwd.col[slot].index();
            pairs.push(if undirected {
                (u.min(v), u.max(v))
            } else {
                (u, v)
            });
        }
    }
    pairs.sort_unstable();
    let mut out: Vec<(usize, usize, u32)> = Vec::new();
    for (a, b) in pairs {
        match out.last_mut() {
            Some(last) if (last.0, last.1) == (a, b) => last.2 += 1,
            _ => out.push((a, b, 1)),
        }
    }
    out
}

/// The answer of VF2 for the run's two projections; `subgraph` asks
/// whether the second is isomorphic to an induced subgraph of the first.
fn vf2(r: &mut Run<'_, '_>, subgraph: bool) -> Result<Output, Error> {
    let other = r.other.expect("a second projection");
    let (a, na, undirected) = with_projection!(r.graph, p => (counted(p), p.node_count(), p.orientation == Orientation::Undirected));
    let (b, nb) = match other {
        Any::Small(p) => (counted(p), p.node_count()),
        Any::Large(p) => (counted(p), p.node_count()),
    };
    trace!("counted", "{undirected} {a:?} {b:?}");
    r.mem
        .grow((na + nb) * 64 + (a.len() + b.len()) * 48 + (na * na + nb * nb) / 8 + 64)?;
    let em = |x: &u32, y: &u32| x == y;
    let nm = |_: &(), _: &()| true;
    let iso = if undirected {
        let (ga, gb) = (ungraph(na, &a), ungraph(nb, &b));
        if subgraph {
            petgraph::algo::is_isomorphic_subgraph_matching(&gb, &ga, nm, em)
        } else {
            petgraph::algo::is_isomorphic_matching(&ga, &gb, nm, em)
        }
    } else {
        let (ga, gb) = (digraph(na, &a), digraph(nb, &b));
        if subgraph {
            petgraph::algo::is_isomorphic_subgraph_matching(&gb, &ga, nm, em)
        } else {
            petgraph::algo::is_isomorphic_matching(&ga, &gb, nm, em)
        }
    };
    one_row(r, vec![Value::Bool(iso)])
}

fn digraph(n: usize, edges: &[(usize, usize, u32)]) -> DiGraph<(), u32> {
    let mut g = DiGraph::with_capacity(n, edges.len());
    for _ in 0..n {
        g.add_node(());
    }
    for (a, b, c) in edges {
        g.add_edge(NodeIndex::new(*a), NodeIndex::new(*b), *c);
    }
    g
}

fn ungraph(n: usize, edges: &[(usize, usize, u32)]) -> UnGraph<(), u32> {
    let mut g = UnGraph::with_capacity(n, edges.len());
    for _ in 0..n {
        g.add_node(());
    }
    for (a, b, c) in edges {
        g.add_edge(NodeIndex::new(*a), NodeIndex::new(*b), *c);
    }
    g
}

fn run_isomorphic(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    vf2(r, false)
}

fn run_isomorphic_subgraph(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    vf2(r, true)
}

fn run_bipartite(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => bipartite(p, r))
}

/// rustworkx's `two_color` (module documentation): the side of each
/// node, or `None` when the projection is not bipartite.
fn two_color<I: Ix>(p: &Projection<I>) -> Option<Vec<u8>> {
    let n = p.node_count();
    let mut colors: Vec<Option<u8>> = vec![None; n];
    for node in 0..n {
        if colors[node].is_some() {
            continue;
        }
        let mut queue = vec![node];
        trace!("colour", "{node} 1");
        colors[node] = Some(1);
        while let Some(v) = queue.pop() {
            let v_color = colors[v].expect("coloured");
            let color = 1 - v_color;
            for (w, _) in p.out(v).chain(p.inc(v)) {
                let w = w.index();
                match colors[w] {
                    Some(c) if c == v_color => {
                        trace!("conflict", "{v} {w}");
                        return None;
                    }
                    Some(_) => {}
                    None => {
                        trace!("colour", "{w} {color}");
                        colors[w] = Some(color);
                        queue.push(w);
                    }
                }
            }
        }
    }
    Some(colors.into_iter().map(|c| c.expect("coloured")).collect())
}

fn bipartite<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    r.mem.grow(3 * p.node_count() * size_of::<usize>())?;
    let sides = two_color(p);
    let mut rows = Vec::new();
    if let Some(sides) = &sides {
        for (v, side) in sides.iter().enumerate() {
            r.emit(
                &mut rows,
                vec![id_value(p, v), Value::Int(i64::from(*side))],
            )?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: vec![("bipartite", Value::Bool(sides.is_some()))],
    })
}

fn run_tournament(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => tournament(p, r))
}

fn tournament<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let edges: Vec<(usize, usize)> = (0..n)
        .flat_map(|u| p.out(u).map(move |(v, _)| (u, v.index())))
        .collect();
    r.mem.grow(edges.len() * 48 + n * 8)?;
    // graphalgs `is_tournament`.
    let mut set = std::collections::HashSet::new();
    let mut is_tournament = 2 * edges.len() == n * n.saturating_sub(1);
    if is_tournament {
        for &(s, t) in &edges {
            if s == t || set.contains(&(t, s)) || set.contains(&(s, t)) {
                is_tournament = false;
                break;
            }
            set.insert((s, t));
        }
    }
    // graphalgs `is_tournament_transitive`: no three nodes x → y → z → x.
    let mut transitive = is_tournament;
    if is_tournament {
        'outer: for x in 0..n {
            for y in 0..n {
                if x == y || !set.contains(&(x, y)) {
                    continue;
                }
                for z in 0..n {
                    if z != y && z != x && set.contains(&(y, z)) && set.contains(&(z, x)) {
                        transitive = false;
                        break 'outer;
                    }
                }
            }
            if x % 64 == 0 {
                r.mem.check_now()?;
            }
        }
    }
    one_row(r, vec![Value::Bool(is_tournament), Value::Bool(transitive)])
}

fn run_graphlike(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let degrees: Vec<usize> = r
        .cfg
        .ints("degreeSequence")
        .into_iter()
        .map(|d| usize::try_from(d).unwrap_or(usize::MAX))
        .collect();
    r.mem.grow(degrees.len() * 16)?;
    let ok = graphlike(&degrees);
    one_row(r, vec![Value::Bool(ok)])
}

/// graphalgs `is_degree_sequence_graphlike` (module documentation).
pub(crate) fn graphlike(degrees: &[usize]) -> bool {
    use std::collections::BinaryHeap;
    if degrees.iter().fold(0usize, |a, d| a.wrapping_add(*d)) % 2 == 1 {
        return false;
    }
    let mut heap: BinaryHeap<usize> = degrees.iter().copied().filter(|d| *d > 0).collect();
    while let Some(d) = heap.pop() {
        trace!("pop", "{d} {}", heap.len());
        if d > heap.len() {
            return false;
        }
        let mut temp = Vec::new();
        for _ in 0..d {
            match heap.pop() {
                Some(x) => temp.push(x - 1),
                None => return false,
            }
        }
        for x in temp {
            if x != 0 {
                heap.push(x);
            }
        }
    }
    true
}

fn run_closure(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => closure_reduction(p, r, true))
}

fn run_reduction(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => closure_reduction(p, r, false))
}

/// The transitive closure (`closure`) or reduction of the projection
/// (module documentation).
fn closure_reduction<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    closure: bool,
) -> Result<Output, Error> {
    let n = p.node_count();
    let order: Vec<usize> = kernels::levels(p, r.sched, r.mem)?
        .into_iter()
        .flatten()
        .collect();
    if order.len() != n {
        return Err(Error::new(
            codes::C22N11,
            "invalid argument: the projection holds a cycle",
        ));
    }
    r.mem
        .grow(n.saturating_mul(n).saturating_mul(8) / 4 + p.edge_ids.len() * 16)?;
    // petgraph's `dag_to_toposorted_adjacency_list`, each predecessor once.
    let mut list: List<(), usize> = List::with_capacity(n);
    let mut position = vec![0usize; n];
    for (ix, &v) in order.iter().enumerate() {
        position[v] = ix;
        let mut preds: Vec<usize> = p
            .neighbors_directed_dense(v, Direction::Incoming)
            .map(|u| position[u])
            .collect();
        preds.sort_unstable();
        preds.dedup();
        let new = list.add_node();
        debug_assert_eq!(new, ix);
        for pre in preds {
            list.add_edge(pre, ix, ());
        }
    }
    trace!("order", "{order:?}");
    let (tred, tclos) = petgraph::algo::tred::dag_transitive_reduction_closure(&list);
    let picked = if closure { &tclos } else { &tred };
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        for j in picked.neighbors(i) {
            pairs.push((order[i], order[j]));
        }
        if i % 256 == 0 {
            r.mem.check_now()?;
        }
    }
    pairs.sort_unstable();
    let mut rows = Vec::new();
    for (u, v) in pairs {
        let mut row = vec![id_value(p, u), id_value(p, v)];
        if !closure {
            let slot = p
                .out(u)
                .filter(|(w, _)| w.index() == v)
                .map(|(_, s)| s)
                .min()
                .expect("a reduction edge is an edge");
            row.push(Value::Int(p.edge_id(slot).cast_signed()));
        }
        r.emit(&mut rows, row)?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

impl<I: Ix> Projection<I> {
    /// The other ends of `v`'s edges in direction `d` of the run's
    /// orientation, as dense indexes.
    fn neighbors_directed_dense(&self, v: usize, d: Direction) -> impl Iterator<Item = usize> + '_ {
        let adj = match d {
            Direction::Outgoing => self.out(v),
            Direction::Incoming => self.inc(v),
        };
        adj.map(|(w, _)| w.index())
    }
}
