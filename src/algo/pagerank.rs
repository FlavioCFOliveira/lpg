//! ALG-042 PageRank and ALG-043 personalised PageRank (`sourceNodes`):
//! the pilot entry of the runtime (rmp #63).
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
}

fn run(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => page_rank(p, r))
}

fn page_rank<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
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
    let params = Params {
        damping: cfg.float("dampingFactor"),
        tolerance: cfg.float("tolerance"),
        max_iterations: usize::try_from(cfg.int("maxIterations")).unwrap_or(usize::MAX),
        weighted: cfg.str("relationshipWeightProperty").is_some(),
        sources,
    };
    let n = p.node_count();
    let mut degree = r.mem.vec(n, 0.0f64)?;
    let mut prev = r.mem.vec(n, NodeState::default())?;
    let mut next = r.mem.vec(n, NodeState::default())?;
    let (ran, converged) = supersteps(p, r.sched, &params, &mut degree, &mut prev, &mut next)?;
    let mut values = r.mem.vec(n, 0.0f64)?;
    for (x, s) in values.iter_mut().zip(&prev) {
        *x = s.rank;
    }
    super::scale::Scaler::of(cfg.get("scaler")).apply(&mut values);
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
                        let message = su.delta / degree[u.index()];
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
