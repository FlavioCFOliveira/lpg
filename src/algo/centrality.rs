//! The centrality family (rmp #65): ALG-034 degree, ALG-035 betweenness,
//! ALG-038 closeness, ALG-039 harmonic, ALG-041 Katz, ALG-045 HITS and
//! ALG-059 influence maximisation (CELF). ALG-040, ALG-043 and ALG-044 run
//! on the power-iteration kernel of [`super::pagerank`].
//!
//! **Semantics.** GDS-named entries follow GDS 2026.09.0, the oracle of
//! `algorithm-entries.md` (GDS @ `417d7ecf973d`, GPL: studied, not
//! copied); ALG-041 follows MAGE.
//!
//! * **034** (`degree/DegreeCentrality.java`): the number of a node's
//!   outgoing edges in the run's orientation, or the sum of their positive
//!   weights when `relationshipWeightProperty` is set. `orientation` is the
//!   common key, which GDS's degree also takes.
//! * **035** (`betweenness/BetweennessCentrality.java`,
//!   `UnweightedForwardTraverser.java`, `WeightedForwardTraverser.java`):
//!   Brandes's accumulation from every source (or a sample), over
//!   outgoing edges; a parallel edge is one more shortest path; each
//!   dependency is halved in the `UNDIRECTED` orientation, as GDS halves
//!   it on an undirected graph.
//! * **038, 039** (`closeness/ClosenessCentrality.java`,
//!   `harmonic/HarmonicCentrality.java`): GDS's multi-source BFS adds the
//!   distance from every source *to* a node, so lpg runs one BFS per node
//!   over its incoming edges. Closeness is `reached / farness` (0 when
//!   farness is 0), and Wasserman-Faust multiplies it by `reached / (n −
//!   1)`; harmonic is `Σ 1/d / (n − 1)`, summed by depth.
//! * **041** (MAGE `katz_centrality_module/algorithm/katz.cpp`
//!   `KatzCentralityLoop`, `Converged`; Apache-2.0, ported): `ω₀ = 1`,
//!   `ωₖ(v) = Σ ωₖ₋₁(u)` over the edges `u → v`, `cₖ = cₖ₋₁ + αᵏ ωₖ`;
//!   the bounds `lr = cₖ`, `ur = cₖ + αᵏ⁺¹ ωₖ γ` with `γ = Δ / (1 − α²Δ)`
//!   for the largest out-degree `Δ`; the loop stops when, with the nodes
//!   sorted by centrality (descending, then by index), each node's `ur −
//!   ε` is below the previous node's `lr`. MAGE's pruning loop starts at
//!   `k = |V|` and never runs, so it is left out.
//! * **045** (`hits/HitsComputation.java`, `HitsState.java`): Pregel
//!   supersteps cycling INIT (auth = in-degree), NORMALIZE_AUTHS,
//!   CALCULATE_HUBS, NORMALIZE_HUBS, CALCULATE_AUTHS, …, `4 ×
//!   hitsIterations` supersteps in all; each normalisation divides by the
//!   L2 norm of the step before. It never converges: `ranIterations` is
//!   `4 × hitsIterations`, `didConverge` false. Edge weights are not used
//!   (GDS's computation does not weight its messages).
//! * **059** (`influenceMaximization/CELF.java`, `ICLazyForwardMC.java`):
//!   CELF's lazy-forward greedy selection under the independent cascade
//!   model, `monteCarloSimulations` cascades per estimate. A node's
//!   `spread` is its marginal gain when it was selected.
//!
//! **Parallel forms** (TS-25 Parallelism):
//!
//! * 034, 038, 039: one value per node, computed by node blocks.
//! * 035: sources in blocks; each block adds its sources' dependencies
//!   into a partial array, and the partial arrays are added into the
//!   result in block order ([`Sched::fold`]).
//! * 045: each superstep by blocks, the norm summed in block order.
//! * 059: the singleton spreads by node blocks. Cascade `j` of every
//!   estimate uses the live-edge form of the independent cascade model:
//!   edge slot `e` is live in cascade `j` when the 64-bit word at position
//!   `2e` of `ChaCha8Rng::seed_from_u64(randomSeed)` with stream `j` falls
//!   below `propagationProbability` (`set_word_pos`). Every estimate of
//!   every candidate reads the same cascades whatever the block or the
//!   order of the traversal, so the result does not depend on
//!   `concurrency`. GDS also partitions its simulations by index
//!   (`ICLazyForwardTask.java`, studied).
//! * 035 sampling (`samplingSize` below the node count): block *b* draws
//!   one factor in `[1, Δ]` per node from its block generator; a node is
//!   selected when the factor is at most its degree (GDS
//!   `RandomDegreeSelectionStrategy.java`, studied); the first
//!   `samplingSize` selected nodes in index order are the sources, topped
//!   up with unselected nodes in index order.
//!
//! **Origins.** Brandes's accumulation follows rustworkx-core @
//! `25398ab47f66` `rustworkx-core/src/centrality.rs`
//! (`betweenness_centrality`, `shortest_path_for_centrality`,
//! `_accumulate_vertices`; <https://github.com/Qiskit/rustworkx>,
//! Apache-2.0, see `LICENSE-APACHE`; the rustworkx contributors, the file
//! carries no copyright line), modified: predecessor lists with one entry
//! per edge, GDS's halving and the block scheme. The Katz loop is a port
//! of MAGE @ `c72636014a8b` (<https://github.com/memgraph/mage>,
//! Apache-2.0, see `LICENSE-APACHE`; Memgraph Ltd., no copyright line in
//! the file), modified: dense arrays,
//! the convergence test without its unreachable pruning. Both files were
//! modified (Apache-2.0 §4(b)).

use std::collections::BinaryHeap;

use rand::{Rng, RngExt};

use super::config::{Def, Key, Ty};
use super::kernels::MinScored;
use super::projection::{Ix, Projection, with_projection};
use super::sched::{Sched, rng};
use super::{Arg, ArgDefault, Entry, Output, Run, SCORE, WriteKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// The write columns of a centrality (`algorithm-entries.md`
/// CentralityWrite).
pub(crate) const CENTRALITY_WRITE: &[&str] = &[
    "nodePropertiesWritten",
    "preProcessingMillis",
    "computeMillis",
    "postProcessingMillis",
    "writeMillis",
    "centralityDistribution",
    "configuration",
];

const WEIGHT: Key = Key::new("relationshipWeightProperty", Ty::Str, Def::Absent);

/// The values of GDS's Pregel `partitioning` key (`Partitioning.parse`),
/// which has no effect in lpg.
pub(crate) const PARTITIONINGS: &[&str] = &["RANGE", "DEGREE", "AUTO"];

/// ALG-034: `gds.degree.stream`, `gds.degree.write`.
pub(crate) const DEGREE: Entry = Entry {
    algs: &[34],
    stream: "gds.degree.stream",
    write: Some("gds.degree.write"),
    keys: &[WEIGHT],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: SCORE,
    write_columns: CENTRALITY_WRITE,
    run: run_degree,
    ..Entry::BASE
};

/// ALG-035: `gds.betweenness.stream`, `gds.betweenness.write`.
pub(crate) const BETWEENNESS: Entry = Entry {
    algs: &[35],
    stream: "gds.betweenness.stream",
    write: Some("gds.betweenness.write"),
    keys: &[
        Key::new("samplingSize", Ty::Int { min: 0 }, Def::Absent),
        Key::new("samplingSeed", Ty::Int { min: i64::MIN }, Def::Int(0)),
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: SCORE,
    write_columns: CENTRALITY_WRITE,
    run: run_betweenness,
    ..Entry::BASE
};

/// ALG-038: `gds.closeness.stream`, `gds.closeness.write`.
pub(crate) const CLOSENESS: Entry = Entry {
    algs: &[38],
    stream: "gds.closeness.stream",
    write: Some("gds.closeness.write"),
    keys: &[Key::new("useWassermanFaust", Ty::Bool, Def::Bool(false))],
    parallel: true,
    stream_columns: SCORE,
    write_columns: CENTRALITY_WRITE,
    run: run_closeness,
    ..Entry::BASE
};

/// ALG-039: `gds.closeness.harmonic.stream`, `gds.closeness.harmonic.write`.
pub(crate) const HARMONIC: Entry = Entry {
    algs: &[39],
    stream: "gds.closeness.harmonic.stream",
    write: Some("gds.closeness.harmonic.write"),
    parallel: true,
    stream_columns: SCORE,
    write_columns: CENTRALITY_WRITE,
    run: run_harmonic,
    ..Entry::BASE
};

/// ALG-041: `katz_centrality.get` (MAGE; no write mode).
pub(crate) const KATZ: Entry = Entry {
    algs: &[41],
    stream: "katz_centrality.get",
    stream_columns: &["node", "rank"],
    run: run_katz,
    args: &[
        Arg::new("alpha", ArgDefault::Float(0.2)),
        Arg::new("epsilon", ArgDefault::Float(0.01)),
    ],
    ..Entry::BASE
};

/// ALG-045: `gds.hits.stream`, `gds.hits.write`.
pub(crate) const HITS: Entry = Entry {
    algs: &[45],
    stream: "gds.hits.stream",
    write: Some("gds.hits.write"),
    keys: &[
        Key::new("hitsIterations", Ty::Int { min: 1 }, Def::Int(20)),
        Key::new("authProperty", Ty::Str, Def::Str("auth")),
        Key::new("hubProperty", Ty::Str, Def::Str("hub")),
        WEIGHT,
        Key::new("partitioning", Ty::Choice(PARTITIONINGS), Def::Str("AUTO")),
        Key::new("mutateProperty", Ty::Str, Def::Absent),
    ],
    parallel: true,
    stream_columns: &["nodeId", "values"],
    write_columns: &[
        "ranIterations",
        "didConverge",
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "configuration",
    ],
    run: run_hits,
    write_keys: &[],
    write_kind: WriteKind::NodeMap { id: 0, map: 1 },
    ..Entry::BASE
};

/// ALG-059: `gds.influenceMaximization.celf.stream`, `.write`.
pub(crate) const CELF: Entry = Entry {
    algs: &[59],
    stream: "gds.influenceMaximization.celf.stream",
    write: Some("gds.influenceMaximization.celf.write"),
    keys: &[
        Key::new("seedSetSize", Ty::Int { min: 1 }, Def::Required),
        Key::new("monteCarloSimulations", Ty::Int { min: 1 }, Def::Int(100)),
        Key::new(
            "propagationProbability",
            // GDS `InfluenceMaximizationBaseConfig`: [0.01, 1].
            Ty::Float {
                min: 0.01,
                max: 1.0,
                max_open: false,
            },
            Def::Float(0.1),
        ),
    ],
    parallel: true,
    randomised: true,
    stream_columns: &["nodeId", "spread"],
    write_columns: &[
        "writeMillis",
        "nodePropertiesWritten",
        "computeMillis",
        "totalSpread",
        "nodeCount",
        "configuration",
    ],
    run: run_celf,
    ..Entry::BASE
};

/// One float per node.
fn floats(values: Vec<f64>) -> Output {
    Output::NodeFloats {
        values,
        ran_iterations: None,
        did_converge: None,
    }
}

fn run_degree(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => degree(p, r))
}

fn degree<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let weighted = r.cfg.str("relationshipWeightProperty").is_some();
    let mut values = r.mem.vec(p.node_count(), 0.0f64)?;
    r.sched.fill(&mut values, |_, range, out| {
        for (x, v) in out.iter_mut().zip(range) {
            *x = if weighted {
                p.out(v)
                    .map(|(_, slot)| p.edge_prop(0, slot))
                    .filter(|w| *w > 0.0)
                    .sum()
            } else {
                p.out_degree(v) as f64
            };
            trace!("degree", "{v} {x:?}");
        }
    })?;
    Ok(floats(values))
}

fn run_betweenness(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => betweenness(p, r))
}

/// The sources of a betweenness run (module documentation).
fn sources<I: Ix>(p: &Projection<I>, sched: &Sched, size: Option<usize>, seed: u64) -> Vec<usize> {
    let n = p.node_count();
    let Some(size) = size.filter(|s| *s < n) else {
        return (0..n).collect();
    };
    let max = (0..n).map(|v| p.out_degree(v)).max().unwrap_or(0);
    let mut picked = vec![false; n];
    if max > 0 {
        for b in 0..n.div_ceil(super::sched::block_size(n)) {
            let mut g = rng(seed, b);
            for v in sched.range(b) {
                let factor = g.random_range(1..=max);
                picked[v] = factor <= p.out_degree(v);
            }
        }
    }
    let mut out: Vec<usize> = (0..n).filter(|v| picked[*v]).take(size).collect();
    if out.len() < size {
        let more: Vec<usize> = (0..n)
            .filter(|v| !picked[*v])
            .take(size - out.len())
            .collect();
        out.extend(more);
        out.sort_unstable();
    }
    out
}

/// The state of Brandes's accumulation from one source; `touched` lists
/// the nodes the last source reached, whose state is reset first.
struct Brandes {
    sigma: Vec<f64>,
    delta: Vec<f64>,
    dist: Vec<f64>,
    settled: Vec<bool>,
    /// Predecessors on shortest paths, one entry per edge.
    preds: Vec<Vec<usize>>,
    stack: Vec<usize>,
    touched: Vec<usize>,
}

impl Brandes {
    fn new(n: usize) -> Brandes {
        Brandes {
            sigma: vec![0.0; n],
            delta: vec![0.0; n],
            dist: vec![f64::INFINITY; n],
            settled: vec![false; n],
            preds: vec![Vec::new(); n],
            stack: Vec::with_capacity(n),
            touched: Vec::new(),
        }
    }

    /// Marks `v` reached.
    fn touch(&mut self, v: usize) {
        if self.dist[v].is_infinite() {
            self.touched.push(v);
        }
    }

    /// Adds the dependencies of source `s` to `acc`, divided by
    /// `divisor` (module documentation).
    fn add<I: Ix>(
        &mut self,
        p: &Projection<I>,
        s: usize,
        weighted: bool,
        divisor: f64,
        acc: &mut [f64],
    ) {
        for v in std::mem::take(&mut self.touched) {
            self.sigma[v] = 0.0;
            self.delta[v] = 0.0;
            self.dist[v] = f64::INFINITY;
            self.settled[v] = false;
            self.preds[v].clear();
        }
        self.stack.clear();
        self.touch(s);
        self.sigma[s] = 1.0;
        self.dist[s] = 0.0;
        if weighted {
            let mut heap = BinaryHeap::new();
            heap.push(MinScored(0.0, s));
            while let Some(MinScored(d, u)) = heap.pop() {
                if self.settled[u] || d > self.dist[u] {
                    continue;
                }
                self.settled[u] = true;
                self.stack.push(u);
                for (w, slot) in p.out(u) {
                    let w = w.index();
                    if self.settled[w] {
                        continue;
                    }
                    let next = d + p.edge_prop(0, slot);
                    if next < self.dist[w] {
                        self.touch(w);
                        self.dist[w] = next;
                        self.sigma[w] = self.sigma[u];
                        self.preds[w].clear();
                        self.preds[w].push(u);
                        heap.push(MinScored(next, w));
                    } else if next == self.dist[w] {
                        self.sigma[w] += self.sigma[u];
                        self.preds[w].push(u);
                    }
                }
            }
        } else {
            let mut head = 0;
            self.stack.push(s);
            while head < self.stack.len() {
                let u = self.stack[head];
                head += 1;
                let du = self.dist[u];
                for (w, _) in p.out(u) {
                    let w = w.index();
                    if self.dist[w].is_infinite() {
                        self.touch(w);
                        self.dist[w] = du + 1.0;
                        self.stack.push(w);
                    }
                    if self.dist[w] == du + 1.0 {
                        self.sigma[w] += self.sigma[u];
                        self.preds[w].push(u);
                    }
                }
            }
        }
        for i in (0..self.stack.len()).rev() {
            let w = self.stack[i];
            let coeff = (1.0 + self.delta[w]) / self.sigma[w];
            for k in 0..self.preds[w].len() {
                let v = self.preds[w][k];
                self.delta[v] += self.sigma[v] * coeff;
            }
            if w != s {
                acc[w] += self.delta[w] / divisor;
            }
        }
        trace!(
            "source",
            "{s} order={:?} sigma={:?} delta={:?}", self.stack, self.sigma, self.delta
        );
    }
}

fn betweenness<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let weighted = r.cfg.str("relationshipWeightProperty").is_some();
    let size = match r.cfg.get("samplingSize") {
        Value::Int(i) => Some(usize::try_from(*i).unwrap_or(usize::MAX)),
        _ => None,
    };
    let src = sources(p, r.sched, size, r.cfg.int("samplingSeed").cast_unsigned());
    trace!("sources", "{src:?}");
    let divisor = if p.orientation == super::projection::Orientation::Undirected {
        2.0
    } else {
        1.0
    };
    // Per worker: a partial array and Brandes's state (predecessors at
    // most one per edge).
    let m = p.edge_ids.len() * if divisor == 2.0 { 2 } else { 1 };
    r.mem.grow(
        r.sched.worker_count() * (n * (5 * size_of::<f64>() + size_of::<Vec<usize>>()) + m * 8)
            + src.len() * size_of::<usize>(),
    )?;
    let mut acc = r.mem.vec(n, 0.0f64)?;
    let src = &src;
    r.sched.fold(
        &mut acc,
        |_, range| {
            let lo = src.partition_point(|v| *v < range.start);
            let hi = src.partition_point(|v| *v < range.end);
            if lo == hi {
                return None;
            }
            let mut part = vec![0.0; n];
            let mut b = Brandes::new(n);
            for s in &src[lo..hi] {
                b.add(p, *s, weighted, divisor, &mut part);
            }
            Some(part)
        },
        |acc, part| {
            if let Some(part) = part {
                for (a, x) in acc.iter_mut().zip(part) {
                    *a += x;
                }
            }
        },
    )?;
    trace!("score", "{acc:?}");
    Ok(floats(acc))
}

/// The number of nodes at each distance 1, 2, … from `v` over incoming
/// edges (module documentation), with `dist` all `u32::MAX` on entry
/// and on return.
fn reverse_depths<I: Ix>(
    p: &Projection<I>,
    v: usize,
    dist: &mut [u32],
    queue: &mut Vec<usize>,
) -> Vec<u64> {
    let mut counts = Vec::new();
    queue.clear();
    queue.push(v);
    dist[v] = 0;
    let mut head = 0;
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        let d = dist[u];
        for (w, _) in p.inc(u) {
            let w = w.index();
            if dist[w] == u32::MAX {
                dist[w] = d + 1;
                if counts.len() <= d as usize {
                    counts.push(0);
                }
                counts[d as usize] += 1;
                queue.push(w);
            }
        }
    }
    for u in queue.iter() {
        dist[*u] = u32::MAX;
    }
    counts
}

/// One value per node from the depth counts of its reverse BFS.
fn by_depths<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    value: impl Fn(&[u64]) -> f64 + Sync,
) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem
        .grow(r.sched.worker_count() * n * (size_of::<u32>() + 2 * size_of::<usize>()))?;
    let mut values = r.mem.vec(n, 0.0f64)?;
    r.sched.fill(&mut values, |_, range, out| {
        let mut dist = vec![u32::MAX; n];
        let mut queue = Vec::new();
        for (x, v) in out.iter_mut().zip(range) {
            let counts = reverse_depths(p, v, &mut dist, &mut queue);
            *x = value(&counts);
            trace!("depths", "{v} {counts:?} {x:?}");
        }
    })?;
    Ok(floats(values))
}

fn run_closeness(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => {
        let n = p.node_count() as f64;
        let wf = r.cfg.bool("useWassermanFaust");
        by_depths(p, r, |counts| {
            let reached: u64 = counts.iter().sum();
            let farness: u64 = counts.iter().enumerate().map(|(d, c)| (d as u64 + 1) * c).sum();
            if farness == 0 {
                0.0
            } else if wf {
                (reached as f64 / farness as f64) * (reached as f64 / (n - 1.0))
            } else {
                reached as f64 / farness as f64
            }
        })
    })
}

fn run_harmonic(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => {
        let n = p.node_count() as f64;
        by_depths(p, r, |counts| {
            let mut sum = 0.0;
            for (d, c) in counts.iter().enumerate() {
                sum += *c as f64 * (1.0 / (d as f64 + 1.0));
            }
            sum / (n - 1.0)
        })
    })
}

fn run_katz(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => katz(p, r))
}

/// A float positional argument.
fn float_arg(v: &Value, name: &str) -> Result<f64, Error> {
    match v {
        Value::Float(f) => Ok(*f),
        Value::Int(i) => Ok(*i as f64),
        _ => Err(Error::new(
            codes::C22G03,
            format!("invalid value type: the argument {name} must be a number"),
        )),
    }
}

fn katz<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let alpha = float_arg(&r.args[0], "alpha")?;
    let epsilon = float_arg(&r.args[1], "epsilon")?;
    let n = p.node_count();
    let mut c = r.mem.vec(n, 0.0f64)?;
    let mut omega = r.mem.vec(n, 1.0f64)?;
    let mut next = r.mem.vec(n, 0.0f64)?;
    let mut ur = r.mem.vec(n, 0.0f64)?;
    let mut order: Vec<usize> = r.mem.vec(n, 0usize)?;
    if !p.edge_ids.is_empty() {
        let max = (0..n).map(|v| p.out_degree(v)).max().unwrap_or(0) as f64;
        let gamma = max / (1.0 - alpha * alpha * max);
        let mut k: i32 = 0;
        loop {
            k += 1;
            r.mem.check_now()?;
            for v in 0..n {
                next[v] = p.inc(v).map(|(u, _)| omega[u.index()]).sum();
                c[v] += alpha.powi(k) * next[v];
                ur[v] = c[v] + alpha.powi(k + 1) * next[v] * gamma;
            }
            std::mem::swap(&mut omega, &mut next);
            for (i, o) in order.iter_mut().enumerate() {
                *o = i;
            }
            order.sort_by(|a, b| c[*b].total_cmp(&c[*a]).then(a.cmp(b)));
            trace!("round", "{k} c={c:?} ur={ur:?} order={order:?}");
            if order.windows(2).all(|w| ur[w[1]] - epsilon < c[w[0]]) {
                trace!("stop", "{k}");
                break;
            }
        }
    }
    let mut rows = Vec::new();
    for (id, x) in p.ids.iter().zip(&c) {
        r.emit(&mut rows, vec![Value::Node(*id), Value::Float(*x)])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_hits(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => hits(p, r))
}

/// The L2 norm of `xs`, summed by blocks in block order.
fn l2(sched: &Sched, xs: &mut [f64]) -> Result<f64, Error> {
    let parts = sched.fill(xs, |_, _, out| out.iter().map(|x| x * x).sum::<f64>())?;
    Ok(parts.iter().sum::<f64>().sqrt())
}

fn hits<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    for key in ["authProperty", "hubProperty"] {
        // GDS `HitsConfig.validateAuthProperty`, `validateHubProperty`.
        if r.cfg
            .str(key)
            .is_some_and(|s| s.chars().any(char::is_whitespace))
        {
            return Err(Error::new(
                codes::C22N11,
                format!("invalid argument: {key} must not contain whitespace"),
            ));
        }
    }
    if p.orientation == super::projection::Orientation::Undirected {
        // GDS 2026.09.0 refuses an undirected projection (measured, rmp
        // #65: "The Hits algorithm requires relationship projections to
        // be directed").
        return Err(Error::new(
            codes::C22N11,
            "invalid argument: HITS requires a directed orientation",
        ));
    }
    let n = p.node_count();
    let rounds = usize::try_from(r.cfg.int("hitsIterations")).unwrap_or(usize::MAX);
    let mut auth = r.mem.vec(n, 0.0f64)?;
    let mut hub = r.mem.vec(n, 0.0f64)?;
    let sched = r.sched;
    // INIT.
    sched.fill(&mut auth, |_, range, out| {
        for (x, v) in out.iter_mut().zip(range) {
            *x = p.inc(v).count() as f64;
        }
    })?;
    let mut norm = l2(sched, &mut auth)?;
    trace!("step", "0 auth={auth:?} hub={hub:?} norm={norm:?}");
    let steps = rounds.saturating_mul(4);
    for step in 1..steps {
        r.mem.check_now()?;
        match step % 4 {
            // NORMALIZE_AUTHS, NORMALIZE_HUBS.
            1 | 3 => {
                let xs = if step % 4 == 1 { &mut auth } else { &mut hub };
                sched.fill(xs, |_, _, out| {
                    for x in out.iter_mut() {
                        *x /= norm;
                    }
                })?;
            }
            // CALCULATE_HUBS: the auths of the out-neighbours.
            2 => {
                let a = &auth;
                sched.fill(&mut hub, |_, range, out| {
                    for (x, v) in out.iter_mut().zip(range) {
                        *x = p.out(v).map(|(w, _)| a[w.index()]).sum();
                    }
                })?;
                norm = l2(sched, &mut hub)?;
            }
            // CALCULATE_AUTHS: the hubs of the in-neighbours.
            _ => {
                let h = &hub;
                sched.fill(&mut auth, |_, range, out| {
                    for (x, v) in out.iter_mut().zip(range) {
                        *x = p.inc(v).map(|(u, _)| h[u.index()]).sum();
                    }
                })?;
                norm = l2(sched, &mut auth)?;
            }
        }
        trace!("step", "{step} auth={auth:?} hub={hub:?} norm={norm:?}");
    }
    let (a, h) = (
        std::sync::Arc::<str>::from(r.cfg.str("authProperty").unwrap_or("auth")),
        std::sync::Arc::<str>::from(r.cfg.str("hubProperty").unwrap_or("hub")),
    );
    let mut rows = Vec::new();
    for v in 0..n {
        let values = Value::Record(
            vec![
                (a.clone(), Value::Float(auth[v])),
                (h.clone(), Value::Float(hub[v])),
            ]
            .into(),
        );
        r.emit(&mut rows, vec![Value::Int(p.ids[v].cast_signed()), values])?;
    }
    Ok(Output::Rows {
        rows,
        extra: vec![
            (
                "ranIterations",
                Value::Int(i64::try_from(steps).unwrap_or(i64::MAX)),
            ),
            ("didConverge", Value::Bool(false)),
        ],
    })
}

fn run_celf(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => celf(p, r))
}

/// Whether edge slot `slot` is live in cascade `j` (module
/// documentation).
fn live(seed: u64, j: u64, slot: usize, prob: f64) -> bool {
    let mut g = rng(seed, 0);
    g.set_stream(j);
    g.set_word_pos(2 * slot as u128);
    let x = (g.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
    x < prob
}

/// The cascades of one estimate: the seed set, the propagation
/// probability, the generator seed and the number of cascades.
struct Cascades {
    prob: f64,
    seed: u64,
    count: u64,
}

impl Cascades {
    /// The mean number of nodes the cascades from `set` activate, the set
    /// included; `seen` is all false on entry and on return.
    fn spread<I: Ix>(&self, p: &Projection<I>, set: &[usize], seen: &mut [bool]) -> f64 {
        let mut total = 0u64;
        let mut queue = Vec::new();
        for j in 0..self.count {
            queue.clear();
            for s in set {
                if !seen[*s] {
                    seen[*s] = true;
                    queue.push(*s);
                }
            }
            let mut head = 0;
            while head < queue.len() {
                let u = queue[head];
                head += 1;
                for (w, slot) in p.out(u) {
                    let w = w.index();
                    if !seen[w] && live(self.seed, j, slot, self.prob) {
                        seen[w] = true;
                        queue.push(w);
                    }
                }
            }
            total += queue.len() as u64;
            for u in &queue {
                seen[*u] = false;
            }
        }
        total as f64 / self.count as f64
    }
}

fn celf<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let k = usize::try_from(r.cfg.int("seedSetSize"))
        .unwrap_or(usize::MAX)
        .min(n);
    let cascades = Cascades {
        prob: r.cfg.float("propagationProbability"),
        seed: r.cfg.seed(),
        count: r.cfg.int("monteCarloSimulations").cast_unsigned(),
    };
    r.mem
        .grow(r.sched.worker_count() * n * (1 + size_of::<usize>()) + n * size_of::<usize>())?;
    let mut single = r.mem.vec(n, 0.0f64)?;
    r.sched.fill(&mut single, |_, range, out| {
        let mut seen = vec![false; n];
        for (x, v) in out.iter_mut().zip(range) {
            *x = cascades.spread(p, &[v], &mut seen);
        }
    })?;
    // CELF: a max-heap of (gain, round of the gain, node), ties to the
    // lowest node index.
    #[derive(PartialEq)]
    struct Item(f64, usize, usize);
    impl Eq for Item {}
    impl PartialOrd for Item {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Item {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            self.0.total_cmp(&o.0).then(o.2.cmp(&self.2))
        }
    }
    trace!("single", "{single:?}");
    let mut heap: BinaryHeap<Item> = (0..n).map(|v| Item(single[v], 0, v)).collect();
    let mut set: Vec<usize> = Vec::with_capacity(k);
    let mut rows = Vec::new();
    let mut total = 0.0;
    let mut seen = vec![false; n];
    while set.len() < k {
        let Some(Item(gain, round, v)) = heap.pop() else {
            break;
        };
        trace!("pop", "{v} {gain:?} {round}");
        if round == set.len() {
            trace!("select", "{v} {gain:?}");
            set.push(v);
            total += gain;
            r.emit(
                &mut rows,
                vec![Value::Int(p.ids[v].cast_signed()), Value::Float(gain)],
            )?;
            continue;
        }
        r.mem.check_now()?;
        set.push(v);
        let with = cascades.spread(p, &set, &mut seen);
        set.pop();
        trace!("gain", "{v} {:?}", with - total);
        heap.push(Item(with - total, set.len(), v));
    }
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("totalSpread", Value::Float(total)),
            (
                "nodeCount",
                Value::Int(i64::try_from(n).unwrap_or(i64::MAX)),
            ),
        ],
    })
}
