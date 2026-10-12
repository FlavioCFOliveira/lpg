//! The in-memory projection of a run ([TS-25] Projection; rmp #63).
//!
//! * **Snapshot.** A projection reads the caller's transaction: its node
//!   tree, then (for edge properties) its edge tree, then its adjacency
//!   tree, each in ordered chunks of [`SCAN_CHUNK`] entries through
//!   [`crate::tx::Transaction::scan_chunk`], so every interval it reads joins the
//!   read set like a query scan's (TS-15) and only one chunk is copied at
//!   a time.
//! * **Dense indexes.** Nodes that have one of `nodeLabels` (all with
//!   `"*"`) get indexes `0..n` in element-identifier order; [`Projection`]
//!   is generic over the index type, `u32` below 2^32 nodes and `usize`
//!   above ([`Any`]). A label or edge type that no element has selects
//!   nothing.
//! * **Arrays.** A forward and a reverse compressed sparse row array, in
//!   petgraph's `Csr` layout (`row` offsets, `column` targets; petgraph
//!   0.8.3 `src/csr.rs`, below), filled in the one ordered adjacency scan:
//!   the tree holds `(node, OUT, type, neighbour, edge)` and `(node, IN,
//!   …)` entries per edge (`src/storage/graph.rs` `adj_key`), so a node's
//!   outgoing row and then its incoming row arrive in node order. Every
//!   edge is one entry, so parallel edges are kept, as GDS's default
//!   projection keeps them. Forward position `i` is the edge's index for
//!   petgraph's `EdgeIndexable`; `rev_slot` maps each reverse entry to it.
//! * **Properties.** Each edge property an entry's key names is an `f64`
//!   array by forward position, each node property one by node; an absent
//!   or non-numeric value is NaN, as GDS projects it.
//! * **Orientation** (TS-25): `NATURAL` reads the forward array as the
//!   outgoing edges, `REVERSE` the reverse array, `UNDIRECTED` both, so
//!   each edge is seen from both ends and a self-loop twice.
//! * **petgraph.** `&Projection` implements petgraph's visit traits,
//!   `IntoNeighborsDirected` and `IntoEdgesDirected` included, so its
//!   functions run on it unchanged; edge weights are the first edge
//!   property, or 1.0 without one.
//!
//! **Origin.** The `row`/`column` layout and the shape of the visit-trait
//! implementations follow petgraph 0.8.3 (crates.io; repository commit
//! `162903562ce5`), `src/csr.rs` (`Csr`, `Neighbors`, `EdgeReference`,
//! the `IntoNeighbors`, `IntoEdges`, `NodeIndexable`, `Visitable`
//! impls), modified: two arrays, parallel edges, orientation, the
//! directed traits, no weights per node. petgraph's licence (`LICENSE-MIT`;
//! petgraph is MIT OR Apache-2.0, used here under MIT):
//!
//! > Copyright (c) 2015
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
//! [TS-25]: ../../docs/technical-spec/algorithms.md

use std::collections::BTreeSet;

use petgraph::Direction;
use petgraph::graph::IndexType;
use petgraph::visit::{
    Data, EdgeCount, EdgeIndexable, EdgeRef, GraphBase, GraphProp, IntoEdgeReferences, IntoEdges,
    IntoEdgesDirected, IntoNeighbors, IntoNeighborsDirected, IntoNodeIdentifiers,
    NodeCompactIndexable, NodeCount, NodeIndexable, VisitMap, Visitable,
};

use super::mem::Mem;
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::storage::graph::{Graph, IN, OUT, decode_adj, decode_edge, element_of};
use crate::types::Value;
use crate::types::record;

/// The index type of a projection: `u32` or `usize`.
pub(crate) trait Ix: IndexType + Send + Sync {}

impl<T: IndexType + Send + Sync> Ix for T {}

/// Entries copied per scan call.
pub(crate) const SCAN_CHUNK: usize = 4096;

/// Which arrays a run reads as outgoing and incoming edges (module
/// documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Orientation {
    Natural,
    Reverse,
    Undirected,
}

impl Orientation {
    /// The orientation a parameter record names.
    pub(crate) fn of(name: Option<&str>) -> Orientation {
        match name {
            Some("REVERSE") => Orientation::Reverse,
            Some("UNDIRECTED") => Orientation::Undirected,
            _ => Orientation::Natural,
        }
    }
}

/// What a run projects.
#[derive(Debug, Clone, Default)]
pub(crate) struct Spec {
    /// Labels a node needs one of; `None` for every node.
    pub(crate) labels: Option<Vec<String>>,
    /// Edge types; `None` for every type.
    pub(crate) types: Option<Vec<String>>,
    /// Edge properties loaded as arrays, in this order.
    pub(crate) edge_props: Vec<String>,
    /// Node properties loaded as arrays, in this order.
    pub(crate) node_props: Vec<String>,
}

/// One compressed sparse row array: the targets of node `v` are
/// `col[row[v]..row[v + 1]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Csr<I> {
    pub(crate) row: Vec<usize>,
    pub(crate) col: Vec<I>,
}

/// A projection (module documentation).
#[derive(Debug, Clone)]
pub(crate) struct Projection<I> {
    /// The element identifier of each dense index, ascending.
    pub(crate) ids: Vec<u64>,
    pub(crate) fwd: Csr<I>,
    pub(crate) rev: Csr<I>,
    /// The element identifier of the edge at each forward position.
    pub(crate) edge_ids: Vec<u64>,
    /// The forward position of the edge at each reverse position.
    pub(crate) rev_slot: Vec<usize>,
    /// Edge property arrays by forward position ([`Spec::edge_props`]).
    pub(crate) edge_props: Vec<Vec<f64>>,
    /// Node property arrays by dense index ([`Spec::node_props`]).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the entries that read node properties arrive with #64 to #69"
        )
    )]
    pub(crate) node_props: Vec<Vec<f64>>,
    pub(crate) orientation: Orientation,
}

/// A projection of either index type (module documentation).
#[derive(Debug, Clone)]
pub(crate) enum Any {
    Small(Projection<u32>),
    Large(Projection<usize>),
}

/// Runs `$body` with `$p` bound to the projection of either index type.
macro_rules! with_projection {
    ($any:expr, $p:ident => $body:expr) => {
        match $any {
            $crate::algo::projection::Any::Small($p) => $body,
            $crate::algo::projection::Any::Large($p) => $body,
        }
    };
}
pub(crate) use with_projection;

/// A property value as a projection holds it: NaN when absent or not a
/// number.
fn number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Int(i)) => *i as f64,
        Some(Value::Float(f)) => *f,
        _ => f64::NAN,
    }
}

fn corrupt(what: &str) -> Error {
    crate::query::expr::internal(what)
}

/// Every entry of `tree` (`g`'s node, edge or adjacency tree), one chunk
/// at a time, through `f`; the statement's status is checked per chunk.
fn scan(
    mem: &mut Mem<'_>,
    tree: crate::storage::btree::Tree,
    mut f: impl FnMut(&mut Mem<'_>, &[u8], &[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut lo: Vec<u8> = Vec::new();
    let mut chunk: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(SCAN_CHUNK);
    loop {
        mem.check_now()?;
        chunk.clear();
        let next = mem.tx.scan_chunk(tree, &lo, None, SCAN_CHUNK, |k, v| {
            chunk.push((k.to_vec(), v.to_vec()));
            true
        })?;
        for (k, v) in &chunk {
            f(mem, k, v)?;
        }
        match next {
            Some(k) => lo = k,
            None => return Ok(()),
        }
    }
}

/// The tokens of `names` of `kind`; `None` for `None`. Names no element
/// has are left out (module documentation).
fn tokens(mem: &Mem<'_>, kind: TokenKind, names: Option<&Vec<String>>) -> Option<BTreeSet<u64>> {
    names.map(|ns| {
        ns.iter()
            .filter_map(|n| mem.tx.engine().token_id(kind, n))
            .collect()
    })
}

/// Builds the projection of graph `g` that `spec` describes (module
/// documentation), reserving every array from `mem` before it grows.
///
/// # Errors
///
/// 51N72 or 51N73 when the memory refuses; 25N14 once the statement is
/// stopped; storage errors.
pub(crate) fn build(
    mem: &mut Mem<'_>,
    g: &Graph,
    spec: &Spec,
    orientation: Orientation,
) -> Result<Any, Error> {
    let labels = tokens(mem, TokenKind::Label, spec.labels.as_ref());
    let types = tokens(mem, TokenKind::EdgeType, spec.types.as_ref());
    let node_keys: Vec<Option<u64>> = spec
        .node_props
        .iter()
        .map(|p| mem.tx.engine().token_id(TokenKind::PropertyKey, p))
        .collect();
    let edge_keys: Vec<Option<u64>> = spec
        .edge_props
        .iter()
        .map(|p| mem.tx.engine().token_id(TokenKind::PropertyKey, p))
        .collect();

    // Nodes, in identifier order.
    let mut ids: Vec<u64> = Vec::new();
    let mut node_props: Vec<Vec<f64>> = vec![Vec::new(); node_keys.len()];
    let decode = labels.is_some() || !node_keys.is_empty();
    scan(mem, g.nodes, |mem, k, v| {
        let id = element_of(k).ok_or_else(|| corrupt("a malformed node key"))?;
        if decode {
            let (ls, ps) = record::decode(v).ok_or_else(|| corrupt("a malformed node record"))?;
            if let Some(want) = &labels
                && !ls.iter().any(|l| want.contains(l))
            {
                return Ok(());
            }
            for (arr, key) in node_props.iter_mut().zip(&node_keys) {
                let x =
                    number(key.and_then(|key| ps.iter().find(|(p, _)| *p == key).map(|(_, x)| x)));
                mem.push(arr, x)?;
            }
        }
        mem.push(&mut ids, id)
    })?;

    // Edge properties, by edge identifier.
    let mut edge_values: Vec<(u64, Vec<f64>)> = Vec::new();
    if !edge_keys.is_empty() {
        scan(mem, g.edges, |mem, k, v| {
            let id = element_of(k).ok_or_else(|| corrupt("a malformed edge key"))?;
            let e = decode_edge(v).ok_or_else(|| corrupt("a malformed edge record"))?;
            mem.grow(edge_keys.len() * size_of::<f64>())?;
            let xs = edge_keys
                .iter()
                .map(|key| {
                    number(
                        key.and_then(|key| e.props.iter().find(|(p, _)| *p == key).map(|(_, x)| x)),
                    )
                })
                .collect();
            mem.push(&mut edge_values, (id, xs))
        })?;
    }

    // The adjacency tree: forward rows from OUT entries, reverse rows from
    // IN entries.
    let n = ids.len();
    let dense = |id: u64| ids.binary_search(&id).ok();
    let mut fwd_row: Vec<usize> = Vec::new();
    let mut fwd_col: Vec<usize> = Vec::new();
    let mut edge_ids: Vec<u64> = Vec::new();
    let mut rev_row: Vec<usize> = Vec::new();
    let mut rev_col: Vec<usize> = Vec::new();
    let mut rev_edge: Vec<u64> = Vec::new();
    scan(mem, g.adj, |mem, k, _| {
        let a = decode_adj(k).ok_or_else(|| corrupt("a malformed adjacency key"))?;
        if types.as_ref().is_some_and(|t| !t.contains(&a.ty)) {
            return Ok(());
        }
        let (Some(u), Some(w)) = (dense(a.node), dense(a.nbr)) else {
            return Ok(());
        };
        let (row, col) = if a.dir == OUT {
            (&mut fwd_row, &mut fwd_col)
        } else {
            debug_assert_eq!(a.dir, IN);
            (&mut rev_row, &mut rev_col)
        };
        while row.len() <= u {
            let at = col.len();
            mem.push(row, at)?;
        }
        mem.push(col, w)?;
        if a.dir == OUT {
            mem.push(&mut edge_ids, a.edge)
        } else {
            mem.push(&mut rev_edge, a.edge)
        }
    })?;
    for (row, len) in [(&mut fwd_row, fwd_col.len()), (&mut rev_row, rev_col.len())] {
        while row.len() <= n {
            mem.push(row, len)?;
        }
    }
    if fwd_col.len() != rev_col.len() {
        return Err(corrupt("an edge without both adjacency entries"));
    }

    // The forward position of each reverse entry, through the edge
    // identifiers.
    let m = edge_ids.len();
    mem.grow(m * size_of::<(u64, usize)>())?;
    let mut by_id: Vec<(u64, usize)> = Vec::with_capacity(m);
    by_id.extend(edge_ids.iter().enumerate().map(|(i, e)| (*e, i)));
    by_id.sort_unstable();
    let mut rev_slot = mem.vec(m, 0usize)?;
    for (slot, e) in rev_slot.iter_mut().zip(&rev_edge) {
        *slot = by_id
            .binary_search_by_key(e, |(id, _)| *id)
            .map(|i| by_id[i].1)
            .map_err(|_| corrupt("an edge without both adjacency entries"))?;
    }
    drop(by_id);
    drop(rev_edge);
    let mut edge_props = Vec::with_capacity(edge_keys.len());
    for p in 0..edge_keys.len() {
        let mut arr = mem.vec(m, f64::NAN)?;
        for (x, e) in arr.iter_mut().zip(&edge_ids) {
            if let Ok(i) = edge_values.binary_search_by_key(e, |(id, _)| *id) {
                *x = edge_values[i].1[p];
            }
        }
        edge_props.push(arr);
    }
    drop(edge_values);

    let parts = Parts {
        ids,
        fwd_row,
        fwd_col,
        rev_row,
        rev_col,
        edge_ids,
        rev_slot,
        edge_props,
        node_props,
        orientation,
    };
    if u32::try_from(n).is_ok() {
        parts.into_projection(mem).map(Any::Small)
    } else {
        parts.into_projection(mem).map(Any::Large)
    }
}

/// The arrays of a projection before their index type is chosen.
struct Parts {
    ids: Vec<u64>,
    fwd_row: Vec<usize>,
    fwd_col: Vec<usize>,
    rev_row: Vec<usize>,
    rev_col: Vec<usize>,
    edge_ids: Vec<u64>,
    rev_slot: Vec<usize>,
    edge_props: Vec<Vec<f64>>,
    node_props: Vec<Vec<f64>>,
    orientation: Orientation,
}

impl Parts {
    /// The projection with index type `I`: the targets are narrowed, the
    /// wider arrays dropped as each is converted.
    fn into_projection<I: IndexType>(self, mem: &mut Mem<'_>) -> Result<Projection<I>, Error> {
        let mut narrow = |col: Vec<usize>| -> Result<Vec<I>, Error> {
            mem.grow(col.len() * size_of::<I>())?;
            Ok(col.into_iter().map(I::new).collect())
        };
        let fwd_col = narrow(self.fwd_col)?;
        let rev_col = narrow(self.rev_col)?;
        Ok(Projection {
            ids: self.ids,
            fwd: Csr {
                row: self.fwd_row,
                col: fwd_col,
            },
            rev: Csr {
                row: self.rev_row,
                col: rev_col,
            },
            edge_ids: self.edge_ids,
            rev_slot: self.rev_slot,
            edge_props: self.edge_props,
            node_props: self.node_props,
            orientation: self.orientation,
        })
    }
}

/// One side of a node's edges: targets and their forward positions.
#[derive(Debug, Clone)]
struct Side<'a, I> {
    col: &'a [I],
    /// Forward position of `col[0]`, or the map of reverse positions.
    base: usize,
    slots: Option<&'a [usize]>,
    at: usize,
}

impl<I: IndexType> Iterator for Side<'_, I> {
    type Item = (I, usize);

    fn next(&mut self) -> Option<(I, usize)> {
        let t = *self.col.get(self.at)?;
        let slot = match self.slots {
            None => self.base + self.at,
            Some(s) => s[self.base + self.at],
        };
        self.at += 1;
        Some((t, slot))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.col.len() - self.at;
        (left, Some(left))
    }
}

/// The edges of a node in one direction of the run's orientation: each
/// other endpoint with the edge's forward position.
#[derive(Debug, Clone)]
pub(crate) struct Adj<'a, I> {
    first: Side<'a, I>,
    second: Option<Side<'a, I>>,
}

impl<I: IndexType> Iterator for Adj<'_, I> {
    type Item = (I, usize);

    fn next(&mut self) -> Option<(I, usize)> {
        self.first
            .next()
            .or_else(|| self.second.as_mut().and_then(Iterator::next))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.first.size_hint().0 + self.second.as_ref().map_or(0, |s| s.size_hint().0);
        (n, Some(n))
    }
}

impl<I: IndexType> Projection<I> {
    /// The number of nodes.
    pub(crate) fn node_count(&self) -> usize {
        self.ids.len()
    }

    /// The dense index of node element `id`, if projected.
    pub(crate) fn dense(&self, id: u64) -> Option<usize> {
        self.ids.binary_search(&id).ok()
    }

    fn forward(&self, v: usize) -> Side<'_, I> {
        let r = self.fwd.row[v]..self.fwd.row[v + 1];
        Side {
            col: &self.fwd.col[r.clone()],
            base: r.start,
            slots: None,
            at: 0,
        }
    }

    fn reverse(&self, v: usize) -> Side<'_, I> {
        let r = self.rev.row[v]..self.rev.row[v + 1];
        Side {
            col: &self.rev.col[r.clone()],
            base: r.start,
            slots: Some(&self.rev_slot),
            at: 0,
        }
    }

    /// The outgoing edges of `v` in the run's orientation.
    pub(crate) fn out(&self, v: usize) -> Adj<'_, I> {
        match self.orientation {
            Orientation::Natural => Adj {
                first: self.forward(v),
                second: None,
            },
            Orientation::Reverse => Adj {
                first: self.reverse(v),
                second: None,
            },
            Orientation::Undirected => Adj {
                first: self.forward(v),
                second: Some(self.reverse(v)),
            },
        }
    }

    /// The incoming edges of `v` in the run's orientation.
    pub(crate) fn inc(&self, v: usize) -> Adj<'_, I> {
        match self.orientation {
            Orientation::Natural => Adj {
                first: self.reverse(v),
                second: None,
            },
            Orientation::Reverse => Adj {
                first: self.forward(v),
                second: None,
            },
            Orientation::Undirected => Adj {
                first: self.forward(v),
                second: Some(self.reverse(v)),
            },
        }
    }

    /// The number of outgoing edges of `v` in the run's orientation.
    pub(crate) fn out_degree(&self, v: usize) -> usize {
        self.out(v).size_hint().0
    }

    /// Edge property `p` of the edge at forward position `slot`.
    pub(crate) fn edge_prop(&self, p: usize, slot: usize) -> f64 {
        self.edge_props[p][slot]
    }

    /// The bytes the projection's arrays hold.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        let edges = self.edge_ids.len();
        self.ids.capacity() * 8
            + (self.fwd.row.capacity() + self.rev.row.capacity()) * size_of::<usize>()
            + (self.fwd.col.capacity() + self.rev.col.capacity()) * size_of::<I>()
            + self.edge_ids.capacity() * 8
            + self.rev_slot.capacity() * size_of::<usize>()
            + self.edge_props.len() * edges * 8
            + self
                .node_props
                .iter()
                .map(|a| a.capacity() * 8)
                .sum::<usize>()
    }
}

/// The weight petgraph sees on an edge without a weight array.
static ONE: f64 = 1.0;

/// A visit map: one flag per node.
#[derive(Debug, Clone)]
pub(crate) struct Visited(Vec<bool>);

impl<I: IndexType> VisitMap<I> for Visited {
    fn visit(&mut self, a: I) -> bool {
        !std::mem::replace(&mut self.0[a.index()], true)
    }

    fn is_visited(&self, a: &I) -> bool {
        self.0[a.index()]
    }

    fn unvisit(&mut self, a: I) -> bool {
        std::mem::replace(&mut self.0[a.index()], false)
    }
}

/// An edge as petgraph sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Edge<'a, I> {
    source: I,
    target: I,
    id: usize,
    weight: &'a f64,
}

impl<I: IndexType> EdgeRef for Edge<'_, I> {
    type NodeId = I;
    type EdgeId = usize;
    type Weight = f64;

    fn source(&self) -> I {
        self.source
    }

    fn target(&self) -> I {
        self.target
    }

    fn weight(&self) -> &f64 {
        self.weight
    }

    fn id(&self) -> usize {
        self.id
    }
}

impl<I: IndexType> Projection<I> {
    fn weight_ref(&self, slot: usize) -> &f64 {
        self.edge_props.first().map_or(&ONE, |w| &w[slot])
    }
}

impl<I: IndexType> GraphBase for Projection<I> {
    type NodeId = I;
    type EdgeId = usize;
}

impl<I: IndexType> GraphProp for Projection<I> {
    type EdgeType = petgraph::Directed;
}

impl<I: IndexType> Data for Projection<I> {
    type NodeWeight = ();
    type EdgeWeight = f64;
}

impl<I: IndexType> NodeCount for Projection<I> {
    fn node_count(&self) -> usize {
        self.ids.len()
    }
}

impl<I: IndexType> EdgeCount for Projection<I> {
    fn edge_count(&self) -> usize {
        self.edge_ids.len()
    }
}

impl<I: IndexType> NodeIndexable for Projection<I> {
    fn node_bound(&self) -> usize {
        self.ids.len()
    }

    fn to_index(&self, a: I) -> usize {
        a.index()
    }

    fn from_index(&self, i: usize) -> I {
        I::new(i)
    }
}

impl<I: IndexType> NodeCompactIndexable for Projection<I> {}

impl<I: IndexType> EdgeIndexable for Projection<I> {
    fn edge_bound(&self) -> usize {
        self.edge_ids.len()
    }

    fn to_index(&self, a: usize) -> usize {
        a
    }

    fn from_index(&self, i: usize) -> usize {
        i
    }
}

impl<I: IndexType> Visitable for Projection<I> {
    type Map = Visited;

    fn visit_map(&self) -> Visited {
        Visited(vec![false; self.ids.len()])
    }

    fn reset_map(&self, map: &mut Visited) {
        map.0.clear();
        map.0.resize(self.ids.len(), false);
    }
}

/// The neighbours of a node in one direction.
pub(crate) type Neighbors<'a, I> = std::iter::Map<Adj<'a, I>, fn((I, usize)) -> I>;

fn first<I>(e: (I, usize)) -> I {
    e.0
}

impl<'a, I: IndexType> IntoNeighbors for &'a Projection<I> {
    type Neighbors = Neighbors<'a, I>;

    fn neighbors(self, a: I) -> Neighbors<'a, I> {
        self.out(a.index()).map(first as fn((I, usize)) -> I)
    }
}

impl<'a, I: IndexType> IntoNeighborsDirected for &'a Projection<I> {
    type NeighborsDirected = Neighbors<'a, I>;

    fn neighbors_directed(self, a: I, d: Direction) -> Neighbors<'a, I> {
        let adj = match d {
            Direction::Outgoing => self.out(a.index()),
            Direction::Incoming => self.inc(a.index()),
        };
        adj.map(first as fn((I, usize)) -> I)
    }
}

/// The edges of a node in one direction, as petgraph sees them.
#[derive(Debug, Clone)]
pub(crate) struct Edges<'a, I> {
    p: &'a Projection<I>,
    node: I,
    dir: Direction,
    adj: Adj<'a, I>,
}

impl<'a, I: IndexType> Iterator for Edges<'a, I> {
    type Item = Edge<'a, I>;

    fn next(&mut self) -> Option<Edge<'a, I>> {
        let (other, slot) = self.adj.next()?;
        let (source, target) = match self.dir {
            Direction::Outgoing => (self.node, other),
            Direction::Incoming => (other, self.node),
        };
        Some(Edge {
            source,
            target,
            id: slot,
            weight: self.p.weight_ref(slot),
        })
    }
}

impl<'a, I: IndexType> IntoEdges for &'a Projection<I> {
    type Edges = Edges<'a, I>;

    fn edges(self, a: I) -> Edges<'a, I> {
        self.edges_directed(a, Direction::Outgoing)
    }
}

impl<'a, I: IndexType> IntoEdgesDirected for &'a Projection<I> {
    type EdgesDirected = Edges<'a, I>;

    fn edges_directed(self, a: I, dir: Direction) -> Edges<'a, I> {
        let adj = match dir {
            Direction::Outgoing => self.out(a.index()),
            Direction::Incoming => self.inc(a.index()),
        };
        Edges {
            p: self,
            node: a,
            dir,
            adj,
        }
    }
}

/// Every edge of the orientation, node by node.
#[derive(Debug, Clone)]
pub(crate) struct EdgeReferences<'a, I> {
    p: &'a Projection<I>,
    next: usize,
    cur: Option<Edges<'a, I>>,
}

impl<'a, I: IndexType> Iterator for EdgeReferences<'a, I> {
    type Item = Edge<'a, I>;

    fn next(&mut self) -> Option<Edge<'a, I>> {
        loop {
            if let Some(e) = self.cur.as_mut().and_then(Iterator::next) {
                return Some(e);
            }
            if self.next >= self.p.ids.len() {
                return None;
            }
            self.cur = Some(self.p.edges(I::new(self.next)));
            self.next += 1;
        }
    }
}

impl<'a, I: IndexType> IntoEdgeReferences for &'a Projection<I> {
    type EdgeRef = Edge<'a, I>;
    type EdgeReferences = EdgeReferences<'a, I>;

    fn edge_references(self) -> EdgeReferences<'a, I> {
        EdgeReferences {
            p: self,
            next: 0,
            cur: None,
        }
    }
}

/// The node indexes `0..n`.
pub(crate) type NodeIdentifiers<I> = std::iter::Map<std::ops::Range<usize>, fn(usize) -> I>;

impl<I: IndexType> IntoNodeIdentifiers for &Projection<I> {
    type NodeIdentifiers = NodeIdentifiers<I>;

    fn node_identifiers(self) -> NodeIdentifiers<I> {
        (0..self.ids.len()).map(I::new as fn(usize) -> I)
    }
}
