// Portions ported from Kuzu (<https://github.com/kuzudb/kuzu>, commit
// `89f0263`): MIT License, Copyright (c) 2022-2025 Kùzu Inc.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! Join order of one connected pattern component (rmp #44; [TS-20]
//! Planner): which node to start from and in which order to expand its
//! edges.
//!
//! * **Plans** are left-deep: a start node (a scan, a seek, or a node an
//!   earlier statement bound), then one edge at a time, each touching a
//!   node already covered (an `Expand`, or an `ExpandInto` when both
//!   endpoints are covered). lpg has no join operator between two
//!   subplans, so bushy plans are not enumerated.
//! * **Cardinality** of a set `S` of edges with node set `N(S)` is
//!   `Π n(v) · Π p(e)`: `n(v)` the node's estimate after its own filters
//!   (1 for a bound node), `p(e)` the probability that an edge of its
//!   type joins two given nodes (the estimate model of
//!   [`super::Estimates`]). It does not depend on the order.
//! * **Cost** of a plan is the sum of the cardinalities of its prefixes,
//!   the start included (TS-20: "the sum of intermediate cardinalities").
//! * **Exact** dynamic programming over connected edge subsets up to
//!   [`MAX_EXACT`] edges: `best(S) = min over e of best(S − e) + card(S)`,
//!   with `S − e` connected and touching `e`. Above, a greedy extension:
//!   the cheapest first edge, then repeatedly the adjacent edge with the
//!   smallest resulting cardinality. Kuzu plans levels up to 7
//!   relationships exactly and the larger ones approximately
//!   (`MAX_LEVEL_TO_PLAN_EXACTLY = 7`,
//!   `src/include/planner/subplans_table.h:11`;
//!   `src/planner/plan/plan_join_order.cpp:136-160, 174-176`): the
//!   threshold and the fallback are ported from there. Grafeo @ `3c23dbe`
//!   `crates/grafeo-engine/src/query/optimizer/join_order.rs` (DPccp over
//!   bushy join trees) was studied and not copied: its plans need a join
//!   operator between subplans, which lpg does not have (Simplicity).
//!
//! [TS-20]: ../../../../docs/technical-spec/query-engine.md

use std::collections::HashMap;

use crate::sync::atomic::{AtomicU64, Ordering};

/// The largest number of edges planned exactly.
pub(crate) const MAX_EXACT: usize = 7;

/// How many components the greedy path planned (tests count it).
pub(crate) static GREEDY_PLANS: AtomicU64 = AtomicU64::new(0);

/// A connected component to order: node estimates and edges.
#[derive(Debug, Clone)]
pub(crate) struct Graph {
    /// `n(v)` of each node.
    pub(crate) nodes: Vec<f64>,
    /// Whether each node is bound by the input (a start without a scan).
    pub(crate) bound: Vec<bool>,
    /// The edges: endpoints and `p(e)`.
    pub(crate) edges: Vec<(usize, usize, f64)>,
}

/// One step of a plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Step {
    /// The edge.
    pub(crate) edge: usize,
    /// The covered endpoint it is expanded from.
    pub(crate) from: usize,
    /// Whether the other endpoint is covered too (`ExpandInto`).
    pub(crate) into: bool,
}

/// An ordered plan and its cost.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Order {
    /// The start node.
    pub(crate) start: usize,
    /// The expansions.
    pub(crate) steps: Vec<Step>,
    /// The cost.
    pub(crate) cost: f64,
}

impl Graph {
    /// The node set of an edge set.
    fn nodes_of(&self, set: u64) -> u64 {
        let mut m = 0u64;
        for (i, &(a, b, _)) in self.edges.iter().enumerate() {
            if set & (1 << i) != 0 {
                m |= 1 << a | 1 << b;
            }
        }
        m
    }

    /// The starts a plan may take: the bound nodes if there are any (a
    /// scan would be a product with the input), else every node.
    fn starts(&self) -> Vec<usize> {
        let b: Vec<usize> = (0..self.nodes.len()).filter(|&v| self.bound[v]).collect();
        if b.is_empty() {
            (0..self.nodes.len()).collect()
        } else {
            b
        }
    }

    /// The cost of the start `v`.
    fn start_cost(&self, v: usize) -> f64 {
        if self.bound[v] { 1.0 } else { self.nodes[v] }
    }

    /// The cost of a given order (no feasibility check beyond the steps'
    /// own), for the exhaustive enumeration of the tests and for the
    /// greedy path.
    pub(crate) fn cost_of(&self, start: usize, edges: &[usize]) -> Option<f64> {
        let mut covered = 1u64 << start;
        let mut set = 0u64;
        let mut cost = self.start_cost(start);
        for &e in edges {
            let (a, b, _) = self.edges[e];
            if covered & (1 << a | 1 << b) == 0 {
                return None;
            }
            covered |= 1 << a | 1 << b;
            set |= 1 << e;
            cost += self.card_from(start, set);
        }
        Some(cost)
    }

    /// `card(S)` of a plan started at `start`: the start node counts with
    /// its own estimate even when no edge covers it yet.
    fn card_from(&self, start: usize, set: u64) -> f64 {
        let nodes = self.nodes_of(set) | 1 << start;
        let mut c = 1.0;
        for (v, n) in self.nodes.iter().enumerate() {
            if nodes & (1 << v) != 0 {
                c *= if self.bound[v] { 1.0 } else { *n };
            }
        }
        for (i, &(_, _, p)) in self.edges.iter().enumerate() {
            if set & (1 << i) != 0 {
                c *= p;
            }
        }
        c
    }

    /// The steps of an edge order from `start`.
    fn steps(&self, start: usize, edges: &[usize]) -> Vec<Step> {
        let mut covered = 1u64 << start;
        edges
            .iter()
            .map(|&e| {
                let (a, b, _) = self.edges[e];
                let from = if covered & (1 << a) != 0 { a } else { b };
                let other = if from == a { b } else { a };
                let into = covered & (1 << other) != 0;
                covered |= 1 << a | 1 << b;
                Step {
                    edge: e,
                    from,
                    into,
                }
            })
            .collect()
    }
}

/// The cheapest order of a connected component (module documentation).
pub(crate) fn order(g: &Graph) -> Order {
    let k = g.edges.len();
    if k == 0 {
        let start = g
            .starts()
            .into_iter()
            .min_by(|&a, &b| g.start_cost(a).total_cmp(&g.start_cost(b)))
            .unwrap_or(0);
        return Order {
            start,
            steps: Vec::new(),
            cost: g.start_cost(start),
        };
    }
    if k > MAX_EXACT {
        GREEDY_PLANS.fetch_add(1, Ordering::Relaxed);
        return greedy(g);
    }
    exact(g)
}

/// Exact dynamic programming over (start, connected edge subset).
fn exact(g: &Graph) -> Order {
    let k = g.edges.len();
    let mut best_order: Option<Order> = None;
    for start in g.starts() {
        // best[set] = (cost, last edge, previous set) for plans from start.
        let mut best: HashMap<u64, (f64, usize, u64)> = HashMap::new();
        let full = (1u64 << k) - 1;
        let start_cost = g.start_cost(start);
        // Subsets in order of size, so every predecessor is done first.
        let mut sets: Vec<u64> = (1..=full).collect();
        sets.sort_by_key(|s| s.count_ones());
        for set in sets {
            let mut found: Option<(f64, usize, u64)> = None;
            for e in 0..k {
                if set & (1 << e) == 0 {
                    continue;
                }
                let prev = set & !(1 << e);
                let prev_cost = if prev == 0 {
                    start_cost
                } else {
                    match best.get(&prev) {
                        Some(&(c, _, _)) => c,
                        None => continue,
                    }
                };
                let covered = g.nodes_of(prev) | 1 << start;
                let (a, b, _) = g.edges[e];
                if covered & (1 << a | 1 << b) == 0 {
                    continue;
                }
                let c = prev_cost + g.card_from(start, set);
                if found.is_none_or(|(fc, _, _)| c < fc) {
                    found = Some((c, e, prev));
                }
            }
            if let Some(f) = found {
                best.insert(set, f);
            }
        }
        if let Some(&(cost, _, _)) = best.get(&full) {
            let mut edges = Vec::new();
            let mut set = full;
            while set != 0 {
                let (_, e, prev) = best[&set];
                edges.push(e);
                set = prev;
            }
            edges.reverse();
            if best_order.as_ref().is_none_or(|o| cost < o.cost) {
                best_order = Some(Order {
                    start,
                    steps: g.steps(start, &edges),
                    cost,
                });
            }
        }
    }
    best_order.expect("a connected component has a plan")
}

/// Greedy extension (Kuzu's approximate levels).
fn greedy(g: &Graph) -> Order {
    let k = g.edges.len();
    let mut best: Option<Order> = None;
    for start in g.starts() {
        let mut covered = 1u64 << start;
        let mut set = 0u64;
        let mut edges = Vec::new();
        while edges.len() < k {
            let next = (0..k)
                .filter(|&e| set & (1 << e) == 0)
                .filter(|&e| {
                    let (a, b, _) = g.edges[e];
                    covered & (1 << a | 1 << b) != 0
                })
                .min_by(|&x, &y| {
                    g.card_from(start, set | 1 << x)
                        .total_cmp(&g.card_from(start, set | 1 << y))
                });
            let Some(e) = next else { break };
            let (a, b, _) = g.edges[e];
            covered |= 1 << a | 1 << b;
            set |= 1 << e;
            edges.push(e);
        }
        if let Some(cost) = g.cost_of(start, &edges)
            && edges.len() == k
            && best.as_ref().is_none_or(|o| cost < o.cost)
        {
            best = Some(Order {
                start,
                steps: g.steps(start, &edges),
                cost,
            });
        }
    }
    best.expect("a connected component has a plan")
}
