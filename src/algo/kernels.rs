//! Kernels shared by the traversal, path, cycle and structure families
//! (rmp #64, #69): node arguments, the GDS-order breadth-first and
//! depth-first traversals, Dijkstra with a predecessor tree, topological
//! levels, path rows and the edge between two nodes.
//!
//! * **Neighbour order.** GDS stores each adjacency list sorted by target
//!   (its compressed lists are delta-encoded), so its traversals visit
//!   neighbours in target order. lpg's projection holds a node's edges in
//!   adjacency-tree order, by edge type and then neighbour
//!   ([`super::projection`]), so the GDS-order kernels sort a node's
//!   distinct targets first ([`targets`]).
//! * **Breadth-first search** ([`bfs`], parallel). GDS's BFS is level
//!   synchronous: each level is expanded by its tasks in parallel, then
//!   merged so that the visit order is the sequential FIFO order: a new
//!   node takes the position of its earliest predecessor in the level,
//!   then its rank in that predecessor's sorted targets (GDS @
//!   `417d7ecf973d` `paths/traverse/BFS.java` `compute`, `BFSTask.java`
//!   `relaxNode`, `syncNextChunk`; GPL, studied only). Kùzu expands a
//!   frontier the same way, a morsel of nodes per task with a level
//!   barrier (Kùzu @ `89f0263cc7a1` `src/function/gds/gds_frontier.cpp`,
//!   `frontier_morsel.cpp`; MIT, studied). lpg expands each level by
//!   blocks of node indexes ([`super::sched`]): block *b* expands the
//!   level's nodes whose index falls in it and returns `(position, rank,
//!   target)` candidates; the merge sorts the candidates and keeps each
//!   target's first, so the order is GDS's FIFO order at every
//!   `concurrency`. Rejected: Kùzu's dense frontier switch (a bitmap pass
//!   over every node per level), which only changes the cost.
//! * **Depth-first search** ([`dfs`], one worker): GDS's stack form,
//!   which marks a node visited when it is pushed and pushes a node's
//!   targets in target order (`DFS.java` `compute`, studied).
//! * **Dijkstra** ([`dijkstra`]): the binary-heap form with a predecessor
//!   per node, as below.
//! * **Topological levels** ([`levels`], parallel): Kahn's algorithm by
//!   levels. Level 0 holds the nodes without incoming edges; level *k* + 1
//!   the nodes whose last incoming edge comes from level *k*. Each round
//!   is computed by blocks (the targets of a block's level nodes) and
//!   merged in block order; the in-degree counts are integers, so the
//!   result does not depend on the order. A node's level is the length of
//!   the longest path that ends at it. GDS's topological sort and longest
//!   path also count down in-degrees from the sources
//!   (`dag/topologicalsort/TopologicalSort.java`,
//!   `dag/longestPath/DagLongestPath.java`, studied); they fork a task per
//!   ready node, which lpg replaces by rounds.
//!
//! **Origin of [`dijkstra`].** rustworkx-core @ `25398ab47f66`
//! (<https://github.com/Qiskit/rustworkx>, Apache-2.0, see
//! `LICENSE-APACHE`; the rustworkx contributors, the file carries no
//! copyright line), `rustworkx-core/src/shortest_path/dijkstra.rs`
//! (`dijkstra`, itself forked from petgraph): the heap of `MinScored`
//! entries, the visited map and the relaxation loop. Modified: a
//! predecessor node and edge per node instead of a cloned path per node,
//! dense arrays instead of a map, `f64` costs ordered by `total_cmp` with
//! the node index as tie-break.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::mem::Mem;
use super::projection::{Ix, Projection};
use super::sched::Sched;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// No node, no edge.
pub(crate) const NONE: usize = usize::MAX;

/// The dense index of the node `v` names (a node or an element
/// identifier), or 22N11 naming `what` if the projection does not hold it.
///
/// # Errors
///
/// As above; 22G03 if `v` is not a node or an integer.
pub(crate) fn node_index<I: Ix>(p: &Projection<I>, v: &Value, what: &str) -> Result<usize, Error> {
    let id = match v {
        Value::Node(id) => *id,
        Value::Int(i) if *i >= 0 => i.cast_unsigned(),
        Value::Int(_) | Value::Null => {
            return Err(Error::new(
                codes::C22N11,
                format!("invalid argument: {what} is not a node"),
            ));
        }
        _ => {
            return Err(Error::new(
                codes::C22G03,
                format!("invalid value type: {what} must be a node"),
            ));
        }
    };
    p.dense(id).ok_or_else(|| {
        Error::new(
            codes::C22N11,
            format!("invalid argument: {what} is not in the projection"),
        )
    })
}

/// The distinct targets of `v`'s outgoing edges, sorted by index (GDS's
/// adjacency order; module documentation).
pub(crate) fn targets<I: Ix>(p: &Projection<I>, v: usize) -> Vec<usize> {
    let mut t: Vec<usize> = p.out(v).map(|(w, _)| w.index()).collect();
    t.sort_unstable();
    t.dedup();
    t
}

/// The cost of the edge at forward position `slot`: its first projected
/// edge property, or 1.0 when `weighted` is false.
pub(crate) fn cost<I: Ix>(p: &Projection<I>, weighted: bool, slot: usize) -> f64 {
    if weighted { p.edge_prop(0, slot) } else { 1.0 }
}

/// The cheapest edge from `u` to `v` in the run's orientation (the first
/// of equal ones), with its cost.
pub(crate) fn best_slot<I: Ix>(
    p: &Projection<I>,
    weighted: bool,
    u: usize,
    v: usize,
) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (w, slot) in p.out(u) {
        if w.index() == v {
            let c = cost(p, weighted, slot);
            if best.is_none_or(|(_, b)| c < b) {
                best = Some((slot, c));
            }
        }
    }
    best
}

/// The edges joining consecutive nodes of `nodes`, each the cheapest
/// ([`best_slot`]), and the cumulative costs from the first node.
pub(crate) fn path_edges<I: Ix>(
    p: &Projection<I>,
    weighted: bool,
    nodes: &[usize],
) -> (Vec<usize>, Vec<f64>) {
    let mut slots = Vec::with_capacity(nodes.len().saturating_sub(1));
    let mut costs = Vec::with_capacity(nodes.len());
    let mut total = 0.0;
    costs.push(total);
    for w in nodes.windows(2) {
        let (slot, c) = best_slot(p, weighted, w[0], w[1]).unwrap_or((NONE, f64::NAN));
        slots.push(slot);
        total += c;
        costs.push(total);
    }
    (slots, costs)
}

/// A node as a GDS column holds it: its element identifier.
pub(crate) fn id_value<I: Ix>(p: &Projection<I>, v: usize) -> Value {
    Value::Int(p.ids[v].cast_signed())
}

/// The path value of `nodes` joined by the edges at `slots`; null if an
/// edge is missing.
pub(crate) fn path_value<I: Ix>(p: &Projection<I>, nodes: &[usize], slots: &[usize]) -> Value {
    if nodes.is_empty() || slots.contains(&NONE) {
        return Value::Null;
    }
    let mut out = Vec::with_capacity(nodes.len() * 2 - 1);
    out.push(p.ids[nodes[0]]);
    for (slot, v) in slots.iter().zip(&nodes[1..]) {
        out.push(p.edge_id(*slot));
        out.push(p.ids[*v]);
    }
    Value::Path(out.into())
}

/// A PathRows row (`algorithm-entries.md`): `index`, `sourceNode`,
/// `targetNode`, `totalCost`, `nodeIds`, `costs`, `path`.
pub(crate) fn path_row<I: Ix>(
    p: &Projection<I>,
    index: usize,
    nodes: &[usize],
    slots: &[usize],
    costs: &[f64],
) -> Vec<Value> {
    vec![
        Value::Int(index as i64),
        id_value(p, nodes[0]),
        id_value(p, nodes[nodes.len() - 1]),
        Value::Float(costs.last().copied().unwrap_or(0.0)),
        Value::List(
            nodes
                .iter()
                .map(|v| id_value(p, *v))
                .collect::<Vec<_>>()
                .into(),
        ),
        Value::List(
            costs
                .iter()
                .map(|c| Value::Float(*c))
                .collect::<Vec<_>>()
                .into(),
        ),
        path_value(p, nodes, slots),
    ]
}

/// A heap entry of [`dijkstra`]: the smallest score first, then the
/// smallest node index.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MinScored(pub(crate) f64, pub(crate) usize);

impl PartialEq for MinScored {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for MinScored {}

impl PartialOrd for MinScored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MinScored {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

/// A shortest-path tree: each node's distance (infinite when not reached)
/// and its predecessor node and edge ([`NONE`] for the source and for
/// nodes not reached).
#[derive(Debug, Clone)]
pub(crate) struct Tree {
    pub(crate) dist: Vec<f64>,
    pub(crate) pred: Vec<usize>,
    pub(crate) slot: Vec<usize>,
}

impl Tree {
    /// A tree of `n` nodes where nothing is reached.
    pub(crate) fn new(n: usize) -> Tree {
        Tree {
            dist: vec![f64::INFINITY; n],
            pred: vec![NONE; n],
            slot: vec![NONE; n],
        }
    }

    /// The nodes and edges from the tree's root to `target`, if reached.
    pub(crate) fn walk(&self, target: usize) -> Option<(Vec<usize>, Vec<usize>)> {
        if self.dist[target].is_infinite() {
            return None;
        }
        let (mut nodes, mut slots) = (vec![target], Vec::new());
        let mut v = target;
        while self.pred[v] != NONE {
            slots.push(self.slot[v]);
            v = self.pred[v];
            nodes.push(v);
            if nodes.len() > self.dist.len() {
                // A predecessor cycle: no path (a negative cycle).
                return None;
            }
        }
        nodes.reverse();
        slots.reverse();
        Some((nodes, slots))
    }

    /// The distances along `nodes`.
    pub(crate) fn costs(&self, nodes: &[usize]) -> Vec<f64> {
        nodes.iter().map(|v| self.dist[*v]).collect()
    }
}

/// The bytes [`dijkstra`] holds for `n` nodes and `m` edges: the tree,
/// the visited flags and a heap of at most `m + 1` entries.
pub(crate) fn dijkstra_bytes(n: usize, m: usize) -> usize {
    n * (8 + 8 + 8 + 1) + (m + 1) * size_of::<MinScored>()
}

/// Dijkstra's algorithm from `source` over the run's orientation, with
/// costs `cost(slot)` (module documentation); stops once `stop` is
/// settled. Edges to a node already settled are skipped, as in the
/// reference.
pub(crate) fn dijkstra<I: Ix>(
    p: &Projection<I>,
    source: usize,
    cost: &dyn Fn(usize) -> f64,
    stop: Option<usize>,
) -> Tree {
    let n = p.node_count();
    let mut t = Tree::new(n);
    let mut visited = vec![false; n];
    let mut heap = BinaryHeap::new();
    t.dist[source] = 0.0;
    heap.push(MinScored(0.0, source));
    while let Some(MinScored(d, u)) = heap.pop() {
        if visited[u] {
            continue;
        }
        trace!("pop", "{u} {d:?}");
        if stop == Some(u) {
            break;
        }
        for (w, slot) in p.out(u) {
            let w = w.index();
            if visited[w] {
                continue;
            }
            let next = d + cost(slot);
            if next < t.dist[w] {
                trace!("relax", "{w} {next:?} {u} {}", p.edge_id(slot));
                t.dist[w] = next;
                t.pred[w] = u;
                t.slot[w] = slot;
                heap.push(MinScored(next, w));
            }
        }
        visited[u] = true;
    }
    t
}

/// GDS's breadth-first order from `source` (module documentation): the
/// nodes in visit order, up to and including the first node of
/// `targets` that a processed level holds, over at most `max_depth`
/// levels (`None`: no limit).
///
/// # Errors
///
/// As [`Mem::grow`]; the scheduler's errors.
pub(crate) fn bfs<I: Ix>(
    p: &Projection<I>,
    sched: &Sched,
    mem: &mut Mem<'_>,
    source: usize,
    max_depth: Option<u64>,
    targets_of: &[usize],
) -> Result<Vec<usize>, Error> {
    let n = p.node_count();
    let mut visited = mem.vec(n, false)?;
    // The level's (node, position) pairs by node, and the candidates.
    mem.grow(
        n * size_of::<usize>()
            + n * size_of::<(usize, usize)>()
            + p.edge_ids.len() * size_of::<(usize, usize, usize)>(),
    )?;
    let mut order = Vec::with_capacity(n);
    visited[source] = true;
    order.push(source);
    let mut start = 0;
    let mut depth = 0u64;
    loop {
        if max_depth == Some(depth) {
            trace!("stop", "maxDepth {depth}");
            break;
        }
        let level = &order[start..];
        trace!("level", "{depth} {level:?}");
        if let Some(pos) = level.iter().position(|v| targets_of.contains(v)) {
            trace!("stop", "target {}", level[pos]);
            order.truncate(start + pos + 1);
            break;
        }
        let mut by_node: Vec<(usize, usize)> =
            level.iter().enumerate().map(|(pos, v)| (*v, pos)).collect();
        by_node.sort_unstable();
        let visited_ref = &visited;
        let parts = sched.map(|_, range| {
            let lo = by_node.partition_point(|(v, _)| *v < range.start);
            let hi = by_node.partition_point(|(v, _)| *v < range.end);
            let mut out = Vec::new();
            for (v, pos) in &by_node[lo..hi] {
                for (rank, w) in targets(p, *v).into_iter().enumerate() {
                    if !visited_ref[w] {
                        out.push((*pos, rank, w));
                    }
                }
                trace!("expand", "{v} at {pos}");
            }
            out
        })?;
        let mut cand: Vec<(usize, usize, usize)> = parts.into_iter().flatten().collect();
        cand.sort_unstable();
        let next = order.len();
        for (_, _, w) in cand {
            if !visited[w] {
                visited[w] = true;
                order.push(w);
            }
        }
        trace!("append", "{:?}", &order[next..]);
        if order.len() == next {
            trace!("stop", "empty");
            break;
        }
        start = next;
        depth += 1;
    }
    Ok(order)
}

/// GDS's depth-first order from `source` (module documentation), up to
/// and including the first node of `targets_of` popped, expanding nodes
/// at depth below `max_depth` (`None`: no limit).
///
/// # Errors
///
/// As [`Mem::grow`].
pub(crate) fn dfs<I: Ix>(
    p: &Projection<I>,
    mem: &mut Mem<'_>,
    source: usize,
    max_depth: Option<u64>,
    targets_of: &[usize],
) -> Result<Vec<usize>, Error> {
    let n = p.node_count();
    let mut visited = mem.vec(n, false)?;
    mem.grow(2 * n * size_of::<(usize, u64)>())?;
    let mut stack: Vec<(usize, u64)> = Vec::with_capacity(n);
    let mut order = Vec::with_capacity(n);
    visited[source] = true;
    stack.push((source, 0));
    while let Some((v, depth)) = stack.pop() {
        trace!("pop", "{v} {depth}");
        order.push(v);
        if targets_of.contains(&v) {
            trace!("stop", "target {v}");
            break;
        }
        if max_depth.is_none_or(|m| depth < m) {
            for w in targets(p, v) {
                if !visited[w] {
                    trace!("push", "{w} {}", depth + 1);
                    visited[w] = true;
                    stack.push((w, depth + 1));
                }
            }
        }
    }
    Ok(order)
}

/// Kahn's topological levels over the run's orientation (module
/// documentation). Nodes on a cycle, or reachable from one, are in no
/// level.
///
/// # Errors
///
/// As [`Mem::grow`]; the scheduler's errors.
pub(crate) fn levels<I: Ix>(
    p: &Projection<I>,
    sched: &Sched,
    mem: &mut Mem<'_>,
) -> Result<Vec<Vec<usize>>, Error> {
    let n = p.node_count();
    let mut indeg = mem.vec(n, 0usize)?;
    sched.fill(&mut indeg, |_, range, out| {
        for (d, v) in out.iter_mut().zip(range) {
            *d = p.inc(v).count();
        }
    })?;
    mem.grow(n * size_of::<usize>() + p.edge_ids.len() * size_of::<usize>())?;
    let mut level: Vec<usize> = (0..n).filter(|v| indeg[*v] == 0).collect();
    let mut out = Vec::new();
    while !level.is_empty() {
        trace!("level", "{} {level:?}", out.len());
        let parts = sched.map(|_, range| {
            let lo = level.partition_point(|v| *v < range.start);
            let hi = level.partition_point(|v| *v < range.end);
            level[lo..hi]
                .iter()
                .flat_map(|v| p.out(*v).map(|(w, _)| w.index()))
                .collect::<Vec<usize>>()
        })?;
        let mut next = Vec::new();
        for w in parts.into_iter().flatten() {
            indeg[w] -= 1;
            if indeg[w] == 0 {
                next.push(w);
            }
        }
        next.sort_unstable();
        out.push(std::mem::replace(&mut level, next));
    }
    Ok(out)
}

/// The simple graph of a projection (rmp #67, #68): the distinct other
/// ends of each node's edges in the run's orientation, sorted by index,
/// the node itself left out. Under `UNDIRECTED` it is the simple
/// undirected graph that triangle, clique, matching and colouring
/// entries read; parallel edges count once and self-loops not at all.
#[derive(Debug, Clone, Default)]
pub(crate) struct Simple {
    pub(crate) row: Vec<usize>,
    pub(crate) col: Vec<usize>,
}

impl Simple {
    /// The neighbours of `v`, ascending.
    pub(crate) fn of(&self, v: usize) -> &[usize] {
        &self.col[self.row[v]..self.row[v + 1]]
    }

    /// The number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.row.len() - 1
    }

    /// Whether `a` and `b` are adjacent.
    pub(crate) fn adjacent(&self, a: usize, b: usize) -> bool {
        self.of(a).binary_search(&b).is_ok()
    }

    /// The bytes [`simple`] reserves for `n` nodes and `m` entries.
    pub(crate) fn bytes(n: usize, m: usize) -> usize {
        (n + 1 + m) * size_of::<usize>()
    }
}

/// The simple graph of `p` ([`Simple`]) in the run's orientation,
/// reserved from `mem`.
///
/// # Errors
///
/// As [`Mem::grow`].
pub(crate) fn simple<I: Ix>(p: &Projection<I>, mem: &mut Mem<'_>) -> Result<Simple, Error> {
    simple_of(p, mem, false)
}

/// The simple undirected graph of `p` whatever the run's orientation:
/// both directions of every edge (K-1 colouring, rmp #68).
///
/// # Errors
///
/// As [`Mem::grow`].
pub(crate) fn simple_undirected<I: Ix>(
    p: &Projection<I>,
    mem: &mut Mem<'_>,
) -> Result<Simple, Error> {
    simple_of(p, mem, true)
}

fn simple_of<I: Ix>(p: &Projection<I>, mem: &mut Mem<'_>, both: bool) -> Result<Simple, Error> {
    let n = p.node_count();
    let m: usize = if both {
        2 * p.edge_ids.len()
    } else {
        (0..n).map(|v| p.out_degree(v)).sum()
    };
    mem.grow(Simple::bytes(n, m))?;
    let mut row = Vec::with_capacity(n + 1);
    let mut col = Vec::with_capacity(m);
    row.push(0);
    for v in 0..n {
        let start = col.len();
        if both {
            col.extend(
                p.fwd.col[p.fwd.row[v]..p.fwd.row[v + 1]]
                    .iter()
                    .chain(&p.rev.col[p.rev.row[v]..p.rev.row[v + 1]])
                    .map(|w| w.index())
                    .filter(|w| *w != v),
            );
        } else {
            col.extend(p.out(v).map(|(w, _)| w.index()).filter(|w| *w != v));
        }
        col[start..].sort_unstable();
        let mut keep = start;
        for i in start..col.len() {
            if i == start || col[i] != col[keep - 1] {
                col[keep] = col[i];
                keep += 1;
            }
        }
        col.truncate(keep);
        row.push(col.len());
    }
    Ok(Simple { row, col })
}

/// The number of common elements of two ascending slices.
pub(crate) fn intersect_count(a: &[usize], b: &[usize]) -> usize {
    let (mut i, mut j, mut c) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                c += 1;
                i += 1;
                j += 1;
            }
        }
    }
    c
}

/// An indexed binary min-heap over elements `0..n` with a cost each
/// (rmp #68): the textbook array heap rooted at position 1, with
/// decrease- and increase-key. Ties are left where the sift operations,
/// which move an element only past a strictly larger cost, leave them,
/// so equal costs come out in an order fixed by the operations' sequence.
#[derive(Debug, Clone)]
pub(crate) struct IndexedHeap {
    heap: Vec<usize>,
    at: Vec<usize>,
    cost: Vec<f64>,
}

impl IndexedHeap {
    /// An empty heap over `n` elements.
    pub(crate) fn new(n: usize) -> IndexedHeap {
        IndexedHeap {
            heap: vec![0],
            at: vec![0; n],
            cost: vec![0.0; n],
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.heap.len() == 1
    }

    /// Whether `e` is in the heap.
    pub(crate) fn contains(&self, e: usize) -> bool {
        self.at[e] > 0
    }

    /// The last cost given to `e`.
    pub(crate) fn cost(&self, e: usize) -> f64 {
        self.cost[e]
    }

    fn less(&self, a: usize, b: usize) -> bool {
        self.cost[a] < self.cost[b]
    }

    fn place(&mut self, pos: usize, e: usize) {
        self.heap[pos] = e;
        self.at[e] = pos;
    }

    fn up(&mut self, from: usize) -> bool {
        let e = self.heap[from];
        let mut i = from;
        while i > 1 && self.less(e, self.heap[i / 2]) {
            let p = self.heap[i / 2];
            self.place(i, p);
            i /= 2;
        }
        self.place(i, e);
        i != from
    }

    fn down(&mut self, from: usize) {
        let size = self.heap.len() - 1;
        let e = self.heap[from];
        let mut i = from;
        loop {
            let mut j = 2 * i;
            if j > size {
                break;
            }
            if j < size && self.less(self.heap[j + 1], self.heap[j]) {
                j += 1;
            }
            if !self.less(self.heap[j], e) {
                break;
            }
            let c = self.heap[j];
            self.place(i, c);
            i = j;
        }
        self.place(i, e);
    }

    /// Adds `e` with `cost`, or sets its cost if it is in the heap.
    pub(crate) fn set(&mut self, e: usize, cost: f64) {
        self.cost[e] = cost;
        if self.contains(e) {
            let pos = self.at[e];
            if !self.up(pos) {
                self.down(pos);
            }
        } else {
            self.heap.push(e);
            let last = self.heap.len() - 1;
            self.at[e] = last;
            self.up(last);
        }
    }

    /// Removes and returns the element of the smallest cost.
    pub(crate) fn pop(&mut self) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let top = self.heap[1];
        let last = self.heap.pop().expect("an element");
        self.at[top] = 0;
        if self.heap.len() > 1 {
            self.place(1, last);
            self.down(1);
        }
        Some(top)
    }
}
