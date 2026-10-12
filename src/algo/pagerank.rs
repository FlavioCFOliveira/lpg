//! ALG-042 PageRank and ALG-043 personalised PageRank (`sourceNodes`):
//! the pilot entry of the runtime (rmp #63); ALG-044 ArticleRank and
//! ALG-040 eigenvector centrality on the same power-iteration kernel
//! (rmp #65).
//!
//! * **ArticleRank** is GDS's PageRank with one change: a sender divides
//!   its delta by `degree + averageDegree` instead of `degree`, where
//!   `averageDegree` is the projection's edge count over its node count
//!   (GDS `pagerank/ArticleRankComputation.java`, `DegreeFunctions.java`
//!   `averageDegree`; studied).
//! * **Eigenvector centrality** (GDS `pagerank/EigenvectorComputation.java`,
//!   studied): every node starts at `1/n` (`1/|sourceNodes|` on the
//!   source nodes, 0 elsewhere). In superstep *k* each node computes
//!   `next = rank + Σ messages` (the sum of what its in-neighbours sent in
//!   superstep *k*−1, times the edge weight when weighted), sends `next /
//!   d` to its out-neighbours, where `d` is 1, or the sum of its positive
//!   out-weights when weighted; then every `next` is divided by the L2
//!   norm of all of them and becomes the rank. The run has converged
//!   after superstep *k* ≥ 1 when no rank moved by more than `tolerance`;
//!   `ranIterations` is then *k*. The `scaler` applies at the end, except
//!   `L2NORM`, which the ranks already are. The norm is summed over blocks
//!   in block order.
//!
//! **Semantics: GDS 2026.09.0's**, which the verification rule compares
//! against (`algorithm-entries.md`, rule F): GDS's PageRank is a Pregel
//! computation with synchronous supersteps, studied in GDS @
//! `417d7ecf973d` (GPL: studied, not copied):
//! `algo/.../pagerank/PageRankComputation.java`, `DegreeFunctions.java`,
//! `pregel/.../Pregel.java` (`run`), `ComputeStep.java`
//! (`computeBatch`), `ReducingMessenger.java`, `context/ComputeContext.java`
//! (`sendToNeighborsWeighted`), `degree/DegreeCentrality.java`.
//!
//! * Superstep 0: each node's rank is `1 − dampingFactor` (only the
//!   `sourceNodes`, if any, else 0); each node with out-degree > 0 sends
//!   `rank / degree` along each outgoing edge, times the edge's weight
//!   when `relationshipWeightProperty` is set.
//! * Superstep k ≥ 1: a node computes if it received a message (a sum of
//!   messages other than 0) or did not halt; `delta = dampingFactor ×
//!   sum`, `rank += delta`; if `delta > tolerance` it sends `delta /
//!   degree` (times the weight) and stays active, else it halts.
//! * The degree is the number of outgoing edges, or the sum of their
//!   positive weights when weighted.
//! * After superstep k the run has converged when no node sent and every
//!   node halted; `ranIterations` is then k, else `maxIterations`
//!   supersteps ran and `ranIterations` is `maxIterations`.
//! * The `scaler` applies after the last superstep ([`super::scale`]).
//!
//! **Block scheme.** GDS pushes messages and sums them in an atomic array,
//! in thread order. lpg pulls instead: in round k each node sums, over its
//! incoming edges in projection order, what the senders of round k−1
//! sent, so each node's sum, and the run, are the same at every
//! `concurrency` (TS-25 Determinism). Rounds alternate two state arrays
//! ([`super::sched::Sched::fill`]).
//!
//! **Origin.** The per-node pull of `parallel_page_rank` (petgraph 0.8.3,
//! `src/algo/page_rank.rs:133-187`, MIT OR Apache-2.0, used under MIT;
//! licence text in [`super::projection`]): each node's new rank is
//! computed alone from the previous ranks of the nodes that point to it,
//! then a convergence test over the round. Changed: petgraph's
//! normalised formula, which visits every node pair per node, is replaced
//! by GDS's delta formula over the incoming edges, and rayon by the block
//! scheduler.

use super::config::{Def, Key, Ty};
use super::projection::{Ix, Projection, with_projection};
use super::sched::Sched;
use super::{Entry, Output, RANK_WRITE, Run, SCORE};
use crate::error::Error;
use crate::gqlstatus::codes;

/// The keys of ALG-040 and ALG-042 to ALG-044 (`algorithm-entries.md`
/// Signatures), as GDS 2026.09.0 reports them.
const KEYS: &[Key] = &[
    Key::new("maxIterations", Ty::Int { min: 1 }, Def::Int(20)),
    Key::new(
        "tolerance",
        Ty::Float {
            min: 0.0,
            max: f64::INFINITY,
            max_open: false,
        },
        Def::Float(1e-7),
    ),
    Key::new("sourceNodes", Ty::Nodes, Def::EmptyList),
    Key::new("scaler", Ty::Scaler, Def::Str("NONE")),
    Key::new("relationshipWeightProperty", Ty::Str, Def::Absent),
    Key::new(
        "dampingFactor",
        Ty::Float {
            min: 0.0,
            max: 1.0,
            max_open: true,
        },
        Def::Float(0.85),
    ),
];

/// ALG-044: `gds.articleRank.stream` and `gds.articleRank.write`.
pub(crate) const ARTICLE_RANK: Entry = Entry {
    algs: &[44],
    stream: "gds.articleRank.stream",
    write: Some("gds.articleRank.write"),
    run: run_article_rank,
    ..ENTRY
};

/// ALG-040: `gds.eigenvector.stream` and `gds.eigenvector.write`: the
/// keys of PageRank without `dampingFactor`.
pub(crate) const EIGENVECTOR: Entry = Entry {
    algs: &[40],
    stream: "gds.eigenvector.stream",
    write: Some("gds.eigenvector.write"),
    keys: KEYS.split_at(5).0,
    run: run_eigenvector,
    ..ENTRY
};

/// ALG-042 and ALG-043: `gds.pageRank.stream` and `gds.pageRank.write`.
pub(crate) const ENTRY: Entry = Entry {
    algs: &[42, 43],
    stream: "gds.pageRank.stream",
    write: Some("gds.pageRank.write"),
    keys: KEYS,
    parallel: true,
    randomised: false,
    undirected: false,
    edge_props: &["relationshipWeightProperty"],
    node_props: &[],
    stream_columns: SCORE,
    write_columns: RANK_WRITE,
    run,
    ..Entry::BASE
};

/// One node's state after a superstep.
#[derive(Debug, Clone, Copy, Default)]
struct NodeState {
    rank: f64,
    /// The rank change it sent, if `sent`.
    delta: f64,
    sent: bool,
    halted: bool,
}

/// The parameters of one run.
struct Params {
    damping: f64,
    tolerance: f64,
    max_iterations: usize,
    weighted: bool,
    /// Dense indexes of `sourceNodes`, sorted.
    sources: Vec<usize>,
    /// Added to a sender's degree: 0, or ArticleRank's average degree.
    add: f64,
}

/// The power-iteration variant of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    PageRank,
    ArticleRank,
    Eigenvector,
}

fn run(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => page_rank(p, r, Variant::PageRank))
}

fn run_article_rank(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => page_rank(p, r, Variant::ArticleRank))
}

fn run_eigenvector(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => page_rank(p, r, Variant::Eigenvector))
}

fn page_rank<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    variant: Variant,
) -> Result<Output, Error> {
    let cfg = r.cfg;
    let mut sources = Vec::new();
    for id in cfg.nodes("sourceNodes") {
        sources.push(p.dense(id).ok_or_else(|| {
            Error::new(
                codes::C22N11,
                "invalid argument: a node of sourceNodes is not in the projection",
            )
        })?);
    }
    sources.sort_unstable();
    sources.dedup();
    let n = p.node_count();
    let add = if variant == Variant::ArticleRank && n > 0 {
        // GDS `DegreeFunctions.averageDegree`: the unweighted degree sum
        // over the node count.
        (0..n).map(|v| p.out_degree(v)).sum::<usize>() as f64 / n as f64
    } else {
        0.0
    };
    let params = Params {
        damping: if variant == Variant::Eigenvector {
            0.0
        } else {
            cfg.float("dampingFactor")
        },
        tolerance: cfg.float("tolerance"),
        max_iterations: usize::try_from(cfg.int("maxIterations")).unwrap_or(usize::MAX),
        weighted: cfg.str("relationshipWeightProperty").is_some(),
        sources,
        add,
    };
    let mut degree = r.mem.vec(n, 0.0f64)?;
    let mut prev = r.mem.vec(n, NodeState::default())?;
    let mut next = r.mem.vec(n, NodeState::default())?;
    let (ran, converged) = if variant == Variant::Eigenvector {
        eigenvector(p, r.sched, &params, &mut degree, &mut prev, &mut next)?
    } else {
        supersteps(p, r.sched, &params, &mut degree, &mut prev, &mut next)?
    };
    let mut values = r.mem.vec(n, 0.0f64)?;
    for (x, s) in values.iter_mut().zip(&prev) {
        *x = s.rank;
    }
    let scaler = super::scale::Scaler::of(cfg.get("scaler"));
    if !(variant == Variant::Eigenvector && scaler == super::scale::Scaler::L2Norm) {
        scaler.apply(&mut values);
    }
    Ok(Output::NodeFloats {
        values,
        ran_iterations: Some(ran as u64),
        did_converge: Some(converged),
    })
}

/// Runs the supersteps (module documentation); the final state is in
/// `prev`. Returns `ranIterations` and whether the run converged.
fn supersteps<I: Ix>(
    p: &Projection<I>,
    sched: &Sched,
    params: &Params,
    degree: &mut [f64],
    prev: &mut Vec<NodeState>,
    next: &mut Vec<NodeState>,
) -> Result<(usize, bool), Error> {
    let weighted = params.weighted;
    degrees(p, sched, weighted, degree)?;
    let degree = &*degree;
    if degree.is_empty() {
        // GDS: with no node, superstep 0 already has every node halted
        // and no message (`Pregel.java` `run`), so `ranIterations` is 0.
        return Ok((0, true));
    }
    let alpha = 1.0 - params.damping;
    // Superstep 0.
    sched.fill(prev, |_, range, out| {
        for (s, v) in out.iter_mut().zip(range) {
            let rank = if params.sources.is_empty() || params.sources.binary_search(&v).is_ok() {
                alpha
            } else {
                0.0
            };
            *s = NodeState {
                rank,
                delta: rank,
                sent: degree[v] > 0.0,
                halted: false,
            };
        }
    })?;
    for iteration in 1..params.max_iterations {
        let old = &**prev;
        let summary = sched.fill(next, |_, range, out| {
            let (mut sent, mut halted) = (false, true);
            for (s, v) in out.iter_mut().zip(range) {
                let mut sum = 0.0;
                for (u, slot) in p.inc(v) {
                    let su = &old[u.index()];
                    if su.sent {
                        let message = su.delta / (degree[u.index()] + params.add);
                        sum += if weighted {
                            message * p.edge_prop(0, slot)
                        } else {
                            message
                        };
                    }
                }
                let me = old[v];
                *s = if sum != 0.0 || !me.halted {
                    let delta = params.damping * sum;
                    let rank = me.rank + delta;
                    if delta > params.tolerance {
                        NodeState {
                            rank,
                            delta,
                            sent: degree[v] > 0.0,
                            halted: false,
                        }
                    } else {
                        NodeState {
                            rank,
                            delta,
                            sent: false,
                            halted: true,
                        }
                    }
                } else {
                    NodeState { sent: false, ..me }
                };
                sent |= s.sent;
                halted &= s.halted;
            }
            (sent, halted)
        })?;
        std::mem::swap(prev, next);
        if summary.iter().all(|(sent, halted)| !sent && *halted) {
            return Ok((iteration, true));
        }
    }
    Ok((params.max_iterations, false))
}

/// The degree array of a run: the out-degree, or the sum of the positive
/// out-weights when weighted (GDS `DegreeCentrality` with `NATURAL`).
fn degrees<I: Ix>(
    p: &Projection<I>,
    sched: &Sched,
    weighted: bool,
    degree: &mut [f64],
) -> Result<(), Error> {
    sched.fill(degree, |_, range, out| {
        for (d, v) in out.iter_mut().zip(range) {
            *d = if weighted {
                p.out(v)
                    .map(|(_, slot)| p.edge_prop(0, slot))
                    .filter(|w| *w > 0.0)
                    .sum()
            } else {
                p.out_degree(v) as f64
            };
        }
    })?;
    Ok(())
}

/// The supersteps of eigenvector centrality (module documentation); the
/// final ranks are in `prev`. `delta` holds a node's unnormalised value,
/// which its messages carry.
fn eigenvector<I: Ix>(
    p: &Projection<I>,
    sched: &Sched,
    params: &Params,
    degree: &mut [f64],
    prev: &mut Vec<NodeState>,
    next: &mut Vec<NodeState>,
) -> Result<(usize, bool), Error> {
    let weighted = params.weighted;
    degrees(p, sched, weighted, degree)?;
    let degree = &*degree;
    let n = degree.len();
    if n == 0 {
        return Ok((0, true));
    }
    let initial = if params.sources.is_empty() {
        1.0 / n as f64
    } else {
        1.0 / params.sources.len() as f64
    };
    // Superstep 0: `next` is the initial value; the ranks start there.
    sched.fill(prev, |_, range, out| {
        for (s, v) in out.iter_mut().zip(range) {
            let x = if params.sources.is_empty() || params.sources.binary_search(&v).is_ok() {
                initial
            } else {
                0.0
            };
            *s = NodeState {
                rank: x,
                delta: x,
                sent: true,
                halted: false,
            };
        }
    })?;
    normalise(sched, prev, None, params.tolerance)?;
    for iteration in 1..params.max_iterations {
        let old = &**prev;
        sched.fill(next, |_, range, out| {
            for (s, v) in out.iter_mut().zip(range) {
                let mut sum = old[v].rank;
                for (u, slot) in p.inc(v) {
                    let u = u.index();
                    let d = if weighted { degree[u] } else { 1.0 };
                    let message = old[u].delta / d;
                    sum += if weighted {
                        message * p.edge_prop(0, slot)
                    } else {
                        message
                    };
                }
                *s = NodeState {
                    rank: sum,
                    delta: sum,
                    sent: true,
                    halted: false,
                };
            }
        })?;
        let converged = normalise(sched, next, Some(old), params.tolerance)?;
        std::mem::swap(prev, next);
        if converged {
            return Ok((iteration, true));
        }
    }
    Ok((params.max_iterations, false))
}

/// Divides every `delta` of `state` by their L2 norm into `rank` (GDS's
/// `L2Norm` scaler: 0 below a norm of `1e-15`); returns whether no rank
/// moved by more than `tolerance` from `old`.
fn normalise(
    sched: &Sched,
    state: &mut [NodeState],
    old: Option<&[NodeState]>,
    tolerance: f64,
) -> Result<bool, Error> {
    let parts = sched.fill(state, |_, _, out| {
        out.iter().map(|s| s.delta * s.delta).sum::<f64>()
    })?;
    let norm = parts.iter().sum::<f64>().sqrt();
    let moved = sched.fill(state, |_, range, out| {
        let mut moved = false;
        for (s, v) in out.iter_mut().zip(range) {
            s.rank = if norm < 1e-15 { 0.0 } else { s.delta / norm };
            if let Some(old) = old {
                moved |= (s.rank - old[v].rank).abs() > tolerance;
            }
        }
        moved
    })?;
    Ok(old.is_some() && !moved.iter().any(|m| *m))
}
