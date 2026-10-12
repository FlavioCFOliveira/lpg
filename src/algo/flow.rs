//! The flows of the spanning trees, flows and cuts family (rmp #68):
//! ALG-155 maximum flow (parallel) and ALG-157 minimum-cost flow (one
//! worker). Both run from every node of `sourceNodes` to every node of
//! `targetNodes` over the edges in the run's orientation, each edge's
//! capacity its `capacityProperty` (0 without the key, so the flow is 0,
//! as GDS 2026.09.0 gives, measured; NaN and negative capacities count as
//! 0). A super source feeds each source through an edge whose capacity is
//! the source's total outgoing capacity, and each target feeds a super
//! sink the same way, which the flow cannot exceed. Rows: one per edge
//! that carries flow, `(source, target, flow)`, parallel edges separately
//! (GDS's rows are per relationship, measured on flow30).
//!
//! * **155** (parallel) is a synchronous push-relabel written here (the
//!   reuse map names no permissive parallel source; GDS's parallel
//!   push-relabel is documented but not in its open repository).
//!   Goldberg and Tarjan's generic algorithm, as petgraph 0.8.3
//!   `maximum_flow/dinics.rs`, NetworKit @ `5f03e17cec09`
//!   `flow/Dinic.cpp` and pathfinding @ `cd4600b36ec0`
//!   `directed/edmonds_karp.rs` also guarantee the maximum value, runs in
//!   rounds of two phases, each by blocks of nodes reading the state the
//!   phase started from: in the push phase every active node (positive
//!   excess, neither super node) pushes along its admissible residual arcs
//!   (`h(u) = h(v) + 1`) in arc order; the two arcs of an edge are never
//!   both admissible, so no arc is pushed from both ends, and the pushes
//!   are applied in block order. In the relabel phase every active node
//!   without an admissible arc takes one more than the lowest height of
//!   its residual arcs' heads; heights only grow, so concurrent relabels
//!   keep the labelling valid. The rounds end when no node is active; the
//!   result is the same at every `concurrency`.
//! * **157** ports NetworKit's `flow/SuccessiveShortestPath.cpp` (MIT,
//!   licence text below): Bellman-Ford potentials, then Dijkstra on
//!   reduced costs over the residual graph from the super source,
//!   potentials raised by the distances, and the bottleneck pushed along
//!   the path to the super sink, until it is unreachable: a maximum flow
//!   of minimum cost. Modified: one super source and sink instead of node
//!   supplies; ties in the heap by node index. `alpha`, the scaling factor
//!   of GDS's cost-scaling algorithm, has no effect. A negative-cost
//!   cycle fails with 22N11.
//!
//! NetworKit's licence (`License.txt`):
//!
//! > Copyright (c) 2013-2022 Henning Meyerhenke, NetworKit Contributors
//! > Copyright (c) 2013-2017 Karlsruhe Institute of Technology
//! > Copyright (c) 2017-2018 University of Cologne
//! > Copyright (c) 2018-2022 Humboldt-Universität zu Berlin
//! >
//! > MIT License (<http://opensource.org/licenses/MIT>)
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

use std::collections::BinaryHeap;

use super::config::{Def, Key, REL_WRITE, Ty, WRITE_PROPERTY};
use super::kernels::{MinScored, NONE, id_value, node_index};
use super::projection::{Ix, Projection, with_projection};
use super::{Entry, Output, Run, WriteKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

const FLOW_KEYS: &[Key] = &[
    Key::new("sourceNodes", Ty::Nodes, Def::Required),
    Key::new("targetNodes", Ty::Nodes, Def::Required),
    Key::new("capacityProperty", Ty::Str, Def::Absent),
];
const FLOW_WRITE_KEYS: &[Key] = &[
    REL_WRITE[0],
    Key::new(WRITE_PROPERTY, Ty::Str, Def::Required),
];

/// ALG-155: `gds.maxFlow.stream`, `.write`.
pub(crate) const MAX_FLOW: Entry = Entry {
    algs: &[155],
    stream: "gds.maxFlow.stream",
    write: Some("gds.maxFlow.write"),
    keys: FLOW_KEYS,
    parallel: true,
    edge_props: &["capacityProperty"],
    stream_columns: &["source", "target", "flow"],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "relationshipsWritten",
        "totalFlow",
        "configuration",
    ],
    run: run_max_flow,
    write_keys: FLOW_WRITE_KEYS,
    write_kind: WriteKind::RelsProp {
        src: 0,
        dst: 1,
        value: 2,
    },
    ..Entry::BASE
};

/// ALG-157: `gds.maxFlow.minCost.stream`, `.write`.
pub(crate) const MIN_COST_FLOW: Entry = Entry {
    algs: &[157],
    stream: "gds.maxFlow.minCost.stream",
    write: Some("gds.maxFlow.minCost.write"),
    keys: &[
        FLOW_KEYS[0],
        FLOW_KEYS[1],
        FLOW_KEYS[2],
        Key::new("costProperty", Ty::Str, Def::Absent),
        Key::new(
            "alpha",
            Ty::Float {
                min: 1.0,
                max: f64::INFINITY,
                max_open: false,
            },
            Def::Float(6.0),
        ),
    ],
    edge_props: &["capacityProperty", "costProperty"],
    stream_columns: &["source", "target", "flow"],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "relationshipsWritten",
        "totalFlow",
        "totalCost",
        "configuration",
    ],
    run: run_min_cost_flow,
    write_keys: FLOW_WRITE_KEYS,
    write_kind: WriteKind::RelsProp {
        src: 0,
        dst: 1,
        value: 2,
    },
    ..Entry::BASE
};

/// The residual network of a run: nodes `0..n`, the super source `n`
/// and sink `n + 1`; arc `2e` runs along edge `e`, arc `2e + 1` against
/// it. Edges `0..m` are the projection's forward positions.
pub(crate) struct Network {
    pub(crate) nodes: usize,
    pub(crate) m: usize,
    pub(crate) head: Vec<usize>,
    pub(crate) res: Vec<f64>,
    pub(crate) cost: Vec<f64>,
    /// The arcs leaving each node: `arcs[row[v]..row[v + 1]]`.
    pub(crate) row: Vec<usize>,
    pub(crate) arcs: Vec<usize>,
}

impl Network {
    pub(crate) fn tail(&self, a: usize) -> usize {
        self.head[a ^ 1]
    }

    pub(crate) fn of(&self, v: usize) -> &[usize] {
        &self.arcs[self.row[v]..self.row[v + 1]]
    }

    /// The flow on edge `e`.
    pub(crate) fn flow(&self, e: usize) -> f64 {
        self.res[2 * e + 1]
    }

    /// The bytes a network of `n` nodes and `m` edges holds.
    pub(crate) fn bytes(n: usize, m: usize) -> usize {
        let arcs = 2 * (m + 2 * n);
        arcs * (2 * size_of::<usize>() + 2 * size_of::<f64>()) + (n + 3) * size_of::<usize>()
    }
}

/// A capacity as the flows read it: NaN and negative values are 0.
fn capacity(x: f64) -> f64 {
    if x > 0.0 { x } else { 0.0 }
}

/// The residual network of `p` (module documentation).
pub(crate) fn network<I: Ix>(
    p: &Projection<I>,
    sources: &[usize],
    targets: &[usize],
    cap: Option<usize>,
    cost: Option<usize>,
) -> Network {
    let n = p.node_count();
    let (s, t) = (n, n + 1);
    let mut head = Vec::new();
    let mut res = Vec::new();
    let mut costs = Vec::new();
    let mut out_cap = vec![0.0f64; n];
    let mut in_cap = vec![0.0f64; n];
    let mut tails = Vec::new();
    for (u, oc) in out_cap.iter_mut().enumerate() {
        for (v, slot) in p.out(u) {
            let v = v.index();
            let c = if u == v {
                0.0
            } else {
                cap.map_or(0.0, |i| capacity(p.edge_prop(i, slot)))
            };
            let k = cost.map_or(0.0, |i| p.edge_prop(i, slot));
            head.push(v);
            head.push(u);
            res.push(c);
            res.push(0.0);
            costs.push(k);
            costs.push(-k);
            tails.push(u);
            tails.push(v);
            *oc += c;
            in_cap[v] += c;
        }
    }
    let m = head.len() / 2;
    let mut sources = sources.to_vec();
    sources.sort_unstable();
    sources.dedup();
    let mut targets = targets.to_vec();
    targets.sort_unstable();
    targets.dedup();
    for x in &sources {
        head.extend([*x, s]);
        res.extend([out_cap[*x], 0.0]);
        costs.extend([0.0, 0.0]);
        tails.extend([s, *x]);
    }
    for x in &targets {
        head.extend([t, *x]);
        res.extend([in_cap[*x], 0.0]);
        costs.extend([0.0, 0.0]);
        tails.extend([*x, t]);
    }
    let nodes = n + 2;
    let mut row = vec![0usize; nodes + 1];
    for u in &tails {
        row[u + 1] += 1;
    }
    for v in 0..nodes {
        row[v + 1] += row[v];
    }
    let mut fill = row.clone();
    let mut arcs = vec![0usize; tails.len()];
    for (a, u) in tails.iter().enumerate() {
        arcs[fill[*u]] = a;
        fill[*u] += 1;
    }
    Network {
        nodes,
        m,
        head,
        res,
        cost: costs,
        row,
        arcs,
    }
}

/// The source and target node indexes of a run.
fn ends<I: Ix>(p: &Projection<I>, r: &Run<'_, '_>) -> Result<(Vec<usize>, Vec<usize>), Error> {
    let mut sources = Vec::new();
    for id in r.cfg.nodes("sourceNodes") {
        sources.push(node_index(p, &Value::Node(id), "sourceNodes")?);
    }
    let mut targets = Vec::new();
    for id in r.cfg.nodes("targetNodes") {
        targets.push(node_index(p, &Value::Node(id), "targetNodes")?);
    }
    Ok((sources, targets))
}

/// The rows of a network's flows (module documentation): one per edge
/// with flow, in edge order.
fn flow_rows<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    net: &Network,
) -> Result<Vec<Vec<Value>>, Error> {
    let mut rows = Vec::new();
    for e in 0..net.m {
        let f = net.flow(e);
        if f > 0.0 {
            let (u, v) = (net.head[2 * e + 1], net.head[2 * e]);
            r.emit(
                &mut rows,
                vec![id_value(p, u), id_value(p, v), Value::Float(f)],
            )?;
        }
    }
    Ok(rows)
}

fn run_max_flow(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => max_flow(p, r))
}

/// ALG-155's push-relabel (module documentation); returns the number of
/// rounds.
pub(crate) fn push_relabel(net: &mut Network, r: &mut Run<'_, '_>) -> Result<usize, Error> {
    let total = net.nodes;
    let n = total - 2;
    let s = n;
    let mut height = vec![0usize; total];
    let mut excess = vec![0.0f64; total];
    height[s] = total;
    for i in net.row[s]..net.row[s + 1] {
        let a = net.arcs[i];
        let d = net.res[a];
        if d > 0.0 {
            net.res[a] -= d;
            net.res[a ^ 1] += d;
            excess[net.head[a]] += d;
            excess[s] -= d;
        }
    }
    let mut next_height = vec![0usize; n];
    let mut rounds = 0;
    loop {
        let active = |v: usize, ex: &[f64]| v < n && ex[v] > 0.0;
        if !(0..n).any(|v| active(v, &excess)) {
            break;
        }
        rounds += 1;
        // Push phase.
        let (net_ref, h, ex) = (&*net, &height, &excess);
        let pushes = r.sched.map(|_, range| {
            let mut out: Vec<(usize, f64)> = Vec::new();
            for u in range {
                if !active(u, ex) {
                    continue;
                }
                let mut left = ex[u];
                for a in net_ref.of(u) {
                    let a = *a;
                    if net_ref.res[a] > 0.0 && h[u] == h[net_ref.head[a]] + 1 {
                        let d = left.min(net_ref.res[a]);
                        out.push((a, d));
                        left -= d;
                        if left == 0.0 {
                            break;
                        }
                    }
                }
            }
            out
        })?;
        for (a, d) in pushes.into_iter().flatten() {
            net.res[a] -= d;
            net.res[a ^ 1] += d;
            excess[net.tail(a)] -= d;
            excess[net.head[a]] += d;
        }
        // Relabel phase.
        let (net_ref, h, ex) = (&*net, &height, &excess);
        r.sched.fill(&mut next_height, |_, range, out| {
            for (o, u) in out.iter_mut().zip(range) {
                *o = h[u];
                if !active(u, ex) {
                    continue;
                }
                let arcs = net_ref.of(u);
                if arcs
                    .iter()
                    .any(|a| net_ref.res[*a] > 0.0 && h[u] == h[net_ref.head[*a]] + 1)
                {
                    continue;
                }
                if let Some(low) = arcs
                    .iter()
                    .filter(|a| net_ref.res[**a] > 0.0)
                    .map(|a| h[net_ref.head[*a]])
                    .min()
                {
                    *o = low + 1;
                }
            }
        })?;
        height[..n].copy_from_slice(&next_height);
        r.mem.check_now()?;
    }
    Ok(rounds)
}

fn max_flow<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let (sources, targets) = ends(p, r)?;
    let n = p.node_count();
    r.mem.grow(
        Network::bytes(n, p.edge_ids.len()) + (n + 2) * (2 * size_of::<usize>() + size_of::<f64>()),
    )?;
    let cap = r.cfg.has("capacityProperty").then_some(0);
    let mut net = network(p, &sources, &targets, cap, None);
    push_relabel(&mut net, r)?;
    let rows = flow_rows(p, r, &net)?;
    // The flow into the sink is the flow out of the targets.
    let into_sink: f64 = net
        .arcs
        .iter()
        .filter(|a| **a % 2 == 0 && net.head[**a] == n + 1)
        .map(|a| net.res[a ^ 1])
        .sum();
    Ok(Output::Rows {
        rows,
        extra: vec![("totalFlow", Value::Float(into_sink))],
    })
}

fn run_min_cost_flow(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => min_cost_flow(p, r))
}

/// NetworKit's successive shortest paths (module documentation).
pub(crate) fn successive_shortest_paths(
    net: &mut Network,
    r: &mut Run<'_, '_>,
) -> Result<(), Error> {
    let total = net.nodes;
    let (s, t) = (total - 2, total - 1);
    // Bellman-Ford potentials over the arcs with capacity.
    let mut pot = vec![0.0f64; total];
    for i in 0..=total {
        let mut updated = false;
        for a in 0..net.head.len() {
            if net.res[a] <= 0.0 {
                continue;
            }
            let (u, v) = (net.tail(a), net.head[a]);
            if pot[u] + net.cost[a] < pot[v] {
                pot[v] = pot[u] + net.cost[a];
                updated = true;
            }
        }
        if !updated {
            break;
        }
        if i == total {
            return Err(Error::new(
                codes::C22N11,
                "invalid argument: the projection holds a negative-cost cycle",
            ));
        }
    }
    let mut dist = vec![f64::INFINITY; total];
    let mut via = vec![NONE; total];
    let mut done = vec![false; total];
    loop {
        r.mem.check_now()?;
        dist.fill(f64::INFINITY);
        via.fill(NONE);
        done.fill(false);
        dist[s] = 0.0;
        let mut heap = BinaryHeap::new();
        heap.push(MinScored(0.0, s));
        while let Some(MinScored(d, u)) = heap.pop() {
            if done[u] {
                continue;
            }
            done[u] = true;
            for a in net.of(u) {
                let a = *a;
                if net.res[a] <= 0.0 {
                    continue;
                }
                let v = net.head[a];
                let reduced = net.cost[a] + pot[u] - pot[v];
                let nd = d + reduced;
                if nd < dist[v] {
                    dist[v] = nd;
                    via[v] = a;
                    heap.push(MinScored(nd, v));
                }
            }
        }
        if dist[t].is_infinite() {
            break;
        }
        for v in 0..total {
            if dist[v].is_finite() {
                pot[v] += dist[v];
            }
        }
        let mut bottleneck = f64::INFINITY;
        let mut v = t;
        while v != s {
            let a = via[v];
            bottleneck = bottleneck.min(net.res[a]);
            v = net.tail(a);
        }
        let mut v = t;
        while v != s {
            let a = via[v];
            net.res[a] -= bottleneck;
            net.res[a ^ 1] += bottleneck;
            v = net.tail(a);
        }
    }
    Ok(())
}

fn min_cost_flow<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let (sources, targets) = ends(p, r)?;
    let n = p.node_count();
    r.mem.grow(
        Network::bytes(n, p.edge_ids.len())
            + (n + 2) * (2 * size_of::<usize>() + 3 * size_of::<f64>()),
    )?;
    let has_cap = r.cfg.has("capacityProperty");
    let cap = has_cap.then_some(0);
    let cost = r.cfg.has("costProperty").then_some(usize::from(has_cap));
    let mut net = network(p, &sources, &targets, cap, cost);
    successive_shortest_paths(&mut net, r)?;
    let rows = flow_rows(p, r, &net)?;
    let mut total_flow = 0.0;
    let mut total_cost = 0.0;
    for e in 0..net.m {
        let f = net.flow(e);
        if f > 0.0 {
            total_cost += f * net.cost[2 * e];
        }
    }
    for a in &net.arcs {
        if *a % 2 == 0 && net.head[*a] == n + 1 {
            total_flow += net.res[a ^ 1];
        }
    }
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("totalFlow", Value::Float(total_flow)),
            ("totalCost", Value::Float(total_cost)),
        ],
    })
}
