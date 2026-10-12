//! The similarity family (rmp #65): ALG-105 node similarity and ALG-106
//! K-nearest neighbours, each with its filtered procedures. The GDS
//! functions ALG-107 and ALG-112 to ALG-117 are in [`super::functions`].
//!
//! **ALG-105** follows GDS 2026.09.0, its oracle (GDS @ `417d7ecf973d`
//! `similarity/nodesim/NodeSimilarity.java`, `VectorComputer.java`,
//! `algo-params/.../nodesim/*SimilarityComputer.java`,
//! `core/.../similarity/OverlapSimilarity.java`, `NodeSimilarityBaseConfig`;
//! GPL: studied, not copied):
//!
//! * A node's vector is the sorted, distinct targets of its outgoing edges
//!   in the run's orientation, without itself, with the weight of the
//!   parallel edge of lowest element identifier when weighted; its
//!   degree, for `degreeCutoff` and `upperDegreeCutoff`, is its length
//!   (GDS `NodeSimilarity.java` `DegreeComputer`).
//! * Jaccard `|A ∩ B| / |A ∪ B|` (weighted: `Σ min / Σ max`), overlap
//!   `|A ∩ B| / min(|A|, |B|)` (weighted: `Σ min / min(Σ A, Σ B)`), cosine
//!   `|A ∩ B| / √(|A| |B|)` (weighted: the cosine of the weight vectors).
//!   A similarity below `similarityCutoff` is dropped.
//! * Each source keeps its `topK` most similar targets, or its `bottomK`
//!   least similar when `bottomK` is given (both given: 22N11); then
//!   `topN` (or `bottomN`) keeps the most (least) similar pairs overall
//!   when it is not 0. Ties go to the lower target index, then the lower
//!   source index. `useComponents` compares only nodes of one weakly
//!   connected component. The filtered procedures restrict sources and
//!   targets to a label or a list of nodes.
//! * Rows come by source in index order, each source's targets from the
//!   most to the least similar (topN: the pairs in that order).
//!
//! **ALG-106** is a parity entry (TS-25): NN-descent as MAGE writes it
//! (MAGE @ `c72636014a8b` `cpp/knn_module/algorithms/knn.hpp`
//! `CalculateKNN`, `UpdateNN`; Apache-2.0, ported), with GDS's keys and
//! metrics (`similarity/knn/metrics/*`, studied): a float property is
//! compared by `1 / (1 + |a − b|)`, a list by `COSINE` (`(cos + 1) / 2`),
//! `EUCLIDEAN` (`1 / (1 + d)`), `PEARSON` (`(r + 1) / 2`), `JACCARD` or
//! `OVERLAP` of its values; several properties by the mean of their
//! similarities. Changed from MAGE, which shares one generator between
//! OpenMP threads and keeps its reverse lists across iterations, so that
//! every run is deterministic:
//!
//! * Each round splits every node's list into old and sampled new
//!   neighbours with its block generator, builds the reverse lists in node
//!   order, then joins by blocks: each block returns its proposals
//!   `(node, candidate, similarity)` in generation order.
//! * The proposals are applied to the neighbour lists in block order, then
//!   generation order (the candidate merge order); a list keeps the `topK`
//!   best by similarity, ties to the lower index.
//! * `randomJoins` random candidates per node and round, and the initial
//!   samples (`UNIFORM`, or `RANDOMWALK`: the distinct nodes of a random
//!   walk from the node, topped up uniformly), are drawn from the block
//!   generator. `perturbationRate` is the probability that a proposal of
//!   the same similarity as a full list's worst replaces it (GDS
//!   `NeighborList.add`:107-115; `knn-specific-algo-config.adoc:21`),
//!   drawn from the generator of stream `blocks`, applied in the same
//!   order.
//! * The run stops when a round makes fewer than `deltaThreshold × topK ×
//!   n` updates, or after `maxIterations` rounds.
//!
//! **Origin.** The NN-descent loop is a port of MAGE @ `c72636014a8b`
//! (<https://github.com/memgraph/mage>, Apache-2.0, see `LICENSE-APACHE`;
//! Memgraph Ltd., no copyright line in the file),
//! `cpp/knn_module/algorithms/knn.hpp`, modified
//! as above (Apache-2.0 §4(b)).

use std::sync::{Mutex, PoisonError};

use rand::RngExt;

use super::config::{Def, Key, REL_WRITE, Ty};
use super::projection::{Ix, Orientation, Projection, with_projection};
use super::sched::rng;
use super::{Entry, Output, Run, WriteKind};
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// The stream columns of a similarity.
const SIMILARITY_ROWS: &[&str] = &["node1", "node2", "similarity"];

/// The keys of a similarity write mode.
const SIMILARITY_WRITE_KEYS: &[Key] = &[
    REL_WRITE[0],
    Key::new(super::config::WRITE_PROPERTY, Ty::Str, Def::Required),
];

const UNIT: Ty = Ty::Float {
    min: 0.0,
    max: 1.0,
    max_open: false,
};

/// The keys of ALG-105.
const NODE_SIMILARITY_KEYS: &[Key] = &[
    Key::new("topK", Ty::Int { min: 1 }, Def::Int(10)),
    Key::new("bottomK", Ty::Int { min: 1 }, Def::Int(10)),
    Key::new("topN", Ty::Int { min: 0 }, Def::Int(0)),
    Key::new("bottomN", Ty::Int { min: 0 }, Def::Int(0)),
    Key::new(
        "similarityCutoff",
        // GDS `NodeSimilarityBaseConfig.similarityCutoff`: [0, 1].
        UNIT,
        Def::Float(1.0e-42),
    ),
    Key::new("degreeCutoff", Ty::Int { min: 1 }, Def::Int(1)),
    Key::new(
        "upperDegreeCutoff",
        Ty::Int { min: 1 },
        Def::Int(2_147_483_647),
    ),
    Key::new(
        "similarityMetric",
        Ty::Choice(&["JACCARD", "OVERLAP", "COSINE"]),
        Def::Str("JACCARD"),
    ),
    Key::new("useComponents", Ty::Bool, Def::Bool(false)),
    Key::new("relationshipWeightProperty", Ty::Str, Def::Absent),
];

const FILTER_KEYS: [Key; 2] = [
    Key::new("sourceNodeFilter", Ty::NodeFilter, Def::Absent),
    Key::new("targetNodeFilter", Ty::NodeFilter, Def::Absent),
];

const NODE_SIMILARITY_WRITE: &[&str] = &[
    "preProcessingMillis",
    "computeMillis",
    "writeMillis",
    "postProcessingMillis",
    "nodesCompared",
    "relationshipsWritten",
    "similarityDistribution",
    "configuration",
];

/// ALG-105: `gds.nodeSimilarity.stream`, `.write`.
pub(crate) const NODE_SIMILARITY: Entry = Entry {
    algs: &[105],
    stream: "gds.nodeSimilarity.stream",
    write: Some("gds.nodeSimilarity.write"),
    keys: NODE_SIMILARITY_KEYS,
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: SIMILARITY_ROWS,
    write_columns: NODE_SIMILARITY_WRITE,
    run: run_node_similarity,
    write_keys: SIMILARITY_WRITE_KEYS,
    write_kind: WriteKind::RelsProp {
        src: 0,
        dst: 1,
        value: 2,
    },
    ..Entry::BASE
};

/// The keys of the filtered ALG-105.
const NODE_SIMILARITY_FILTERED_KEYS: &[Key] = &{
    let mut k = [FILTER_KEYS[0]; 12];
    let mut i = 0;
    while i < 10 {
        k[i] = NODE_SIMILARITY_KEYS[i];
        i += 1;
    }
    k[10] = FILTER_KEYS[0];
    k[11] = FILTER_KEYS[1];
    k
};

/// ALG-105: `gds.nodeSimilarity.filtered.stream`, `.write`.
pub(crate) const NODE_SIMILARITY_FILTERED: Entry = Entry {
    stream: "gds.nodeSimilarity.filtered.stream",
    write: Some("gds.nodeSimilarity.filtered.write"),
    keys: NODE_SIMILARITY_FILTERED_KEYS,
    node_labels: true,
    ..NODE_SIMILARITY
};

/// The keys of ALG-106.
const KNN_KEYS: &[Key] = &[
    Key::new("nodeProperties", Ty::Props, Def::Required),
    Key::new("topK", Ty::Int { min: 1 }, Def::Int(10)),
    Key::new(
        "sampleRate",
        Ty::Float {
            min: 0.0,
            max: 1.0,
            max_open: false,
        },
        Def::Float(0.5),
    ),
    Key::new("deltaThreshold", UNIT, Def::Float(0.001)),
    Key::new("maxIterations", Ty::Int { min: 1 }, Def::Int(100)),
    Key::new("randomJoins", Ty::Int { min: 0 }, Def::Int(10)),
    Key::new(
        "initialSampler",
        Ty::Choice(&["UNIFORM", "RANDOMWALK"]),
        Def::Str("UNIFORM"),
    ),
    Key::new("perturbationRate", UNIT, Def::Float(0.0)),
    Key::new("similarityCutoff", UNIT, Def::Float(0.0)),
];

const KNN_WRITE: &[&str] = &[
    "preProcessingMillis",
    "computeMillis",
    "writeMillis",
    "postProcessingMillis",
    "nodesCompared",
    "relationshipsWritten",
    "didConverge",
    "ranIterations",
    "nodePairsConsidered",
    "similarityDistribution",
    "configuration",
];

/// ALG-106: `gds.knn.stream`, `.write`.
pub(crate) const KNN: Entry = Entry {
    algs: &[106],
    stream: "gds.knn.stream",
    write: Some("gds.knn.write"),
    keys: KNN_KEYS,
    parallel: true,
    randomised: true,
    vector_props: &["nodeProperties"],
    stream_columns: SIMILARITY_ROWS,
    write_columns: KNN_WRITE,
    run: run_knn,
    write_keys: SIMILARITY_WRITE_KEYS,
    write_kind: WriteKind::RelsProp {
        src: 0,
        dst: 1,
        value: 2,
    },
    ..Entry::BASE
};

/// The keys of the filtered ALG-106.
const KNN_FILTERED_KEYS: &[Key] = &{
    let mut k = [FILTER_KEYS[0]; 12];
    let mut i = 0;
    while i < 9 {
        k[i] = KNN_KEYS[i];
        i += 1;
    }
    k[9] = FILTER_KEYS[0];
    k[10] = FILTER_KEYS[1];
    k[11] = Key::new("seedTargetNodes", Ty::Bool, Def::Bool(false));
    k
};

/// ALG-106: `gds.knn.filtered.stream`, `.write`.
pub(crate) const KNN_FILTERED: Entry = Entry {
    stream: "gds.knn.filtered.stream",
    write: Some("gds.knn.filtered.write"),
    keys: KNN_FILTERED_KEYS,
    node_labels: true,
    ..KNN
};

/// The nodes a filter key admits, by dense index; `None` for no filter.
fn filter<I: Ix>(p: &Projection<I>, r: &Run<'_, '_>, key: &str) -> Option<Vec<bool>> {
    if !r.cfg.has(key) {
        return None;
    }
    let n = p.node_count();
    let mut ok = vec![false; n];
    match r.cfg.get(key) {
        Value::String(label) => {
            if let Some(t) = r.token(TokenKind::Label, label) {
                for (v, x) in ok.iter_mut().enumerate() {
                    *x = p.labels_of(v).contains(&t);
                }
            }
        }
        _ => {
            for id in r.cfg.nodes(key) {
                if let Some(v) = p.dense(id) {
                    ok[v] = true;
                }
            }
        }
    }
    Some(ok)
}

/// The similarity distribution of a write mode's summary.
fn distribution(rows: &[Vec<Value>]) -> Value {
    let xs: Vec<f64> = rows
        .iter()
        .filter_map(|r| match r[2] {
            Value::Float(x) => Some(x),
            _ => None,
        })
        .collect();
    super::distribution(&xs)
}

/// A similarity metric of ALG-105.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    Jaccard,
    Overlap,
    Cosine,
}

/// The similarity of two sorted vectors, with their weights when
/// weighted (module documentation).
fn similarity(m: Metric, a: &[usize], wa: &[f64], b: &[usize], wb: &[f64], weighted: bool) -> f64 {
    let (mut i, mut j) = (0, 0);
    if !weighted {
        let mut inter = 0usize;
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Equal => {
                    inter += 1;
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        let inter = inter as f64;
        return match m {
            Metric::Jaccard => {
                let union = a.len() as f64 + b.len() as f64 - inter;
                if union == 0.0 { 0.0 } else { inter / union }
            }
            Metric::Overlap => inter / (a.len().min(b.len()) as f64),
            Metric::Cosine => inter / ((a.len() as f64).sqrt() * (b.len() as f64).sqrt()),
        };
    }
    // Weighted: (min, max) for Jaccard, (top, Σ a, Σ b) for overlap,
    // (dot, Σ a², Σ b²) for cosine.
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let add_a = |w: f64, y: &mut f64| *y += if m == Metric::Cosine { w * w } else { w };
    let add_b = |w: f64, y: &mut f64, z: &mut f64| match m {
        Metric::Jaccard => *y += w,
        Metric::Overlap => *z += w,
        Metric::Cosine => *z += w * w,
    };
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                let (w1, w2) = (wa[i], wb[j]);
                match m {
                    Metric::Jaccard => {
                        x += w1.min(w2);
                        y += w1.max(w2);
                    }
                    Metric::Overlap => {
                        x += w1.min(w2);
                        y += w1;
                        z += w2;
                    }
                    Metric::Cosine => {
                        x += w1 * w2;
                        y += w1 * w1;
                        z += w2 * w2;
                    }
                }
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                add_a(wa[i], &mut y);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                add_b(wb[j], &mut y, &mut z);
                j += 1;
            }
        }
    }
    for w in &wa[i..] {
        add_a(*w, &mut y);
    }
    for w in &wb[j..] {
        add_b(*w, &mut y, &mut z);
    }
    match m {
        Metric::Jaccard => x / y,
        Metric::Overlap => x / y.min(z),
        Metric::Cosine => x / (y * z).sqrt(),
    }
}

/// The components of the undirected projection (union-find), by node.
fn components<I: Ix>(p: &Projection<I>) -> Vec<usize> {
    let n = p.node_count();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    for v in 0..n {
        for (w, _) in p.out(v).chain(p.inc(v)) {
            let (a, b) = (find(&mut parent, v), find(&mut parent, w.index()));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    (0..n).map(|v| find(&mut parent, v)).collect()
}

/// A scored pair, ordered best first: by similarity (descending when
/// `top`), then by index.
fn better(top: bool, a: (f64, usize), b: (f64, usize)) -> bool {
    let c = if top {
        b.0.total_cmp(&a.0)
    } else {
        a.0.total_cmp(&b.0)
    };
    c.then(a.1.cmp(&b.1)) == std::cmp::Ordering::Less
}

/// Keeps the `k` best of `list` (module documentation), sorted best first.
fn keep_best(list: &mut Vec<(f64, usize)>, k: usize, top: bool) {
    list.sort_by(|a, b| {
        if better(top, *a, *b) {
            std::cmp::Ordering::Less
        } else if better(top, *b, *a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    list.truncate(k);
}

fn run_node_similarity(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => node_similarity(p, r))
}

fn node_similarity<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let cfg = r.cfg;
    if cfg.given("topK") && cfg.given("bottomK") {
        return Err(invalid("topK and bottomK cannot be combined"));
    }
    if cfg.given("topN") && cfg.given("bottomN") && cfg.int("bottomN") != 0 && cfg.int("topN") != 0
    {
        return Err(invalid("topN and bottomN cannot be combined"));
    }
    if cfg.int("upperDegreeCutoff") < cfg.int("degreeCutoff") {
        return Err(invalid(
            "the value of upperDegreeCutoff cannot be smaller than degreeCutoff",
        ));
    }
    let n = p.node_count();
    let weighted = cfg.str("relationshipWeightProperty").is_some();
    let metric = match cfg.str("similarityMetric") {
        Some("OVERLAP") => Metric::Overlap,
        Some("COSINE") => Metric::Cosine,
        _ => Metric::Jaccard,
    };
    let top_k = !cfg.given("bottomK");
    let k = usize::try_from(cfg.int(if top_k { "topK" } else { "bottomK" })).unwrap_or(usize::MAX);
    let bottom_n = cfg.given("bottomN") && cfg.int("bottomN") != 0;
    let top_n = !bottom_n;
    let nn = usize::try_from(cfg.int(if bottom_n { "bottomN" } else { "topN" })).unwrap_or(0);
    let cutoff = cfg.float("similarityCutoff");
    let (lo, hi) = (cfg.int("degreeCutoff"), cfg.int("upperDegreeCutoff"));
    let has_filters = cfg.has("sourceNodeFilter") || cfg.has("targetNodeFilter");
    let sources_ok = if has_filters {
        filter(p, r, "sourceNodeFilter")
    } else {
        None
    };
    let targets_ok = if has_filters {
        filter(p, r, "targetNodeFilter")
    } else {
        None
    };
    // The vectors: sorted distinct targets and their weights.
    let m = p.edge_ids.len()
        * if p.orientation == Orientation::Undirected {
            2
        } else {
            1
        };
    r.mem
        .grow(n * (3 * size_of::<usize>() + 2) + m * (size_of::<usize>() + size_of::<f64>()))?;
    let mut row = Vec::with_capacity(n + 1);
    let mut col: Vec<usize> = Vec::new();
    let mut wts: Vec<f64> = Vec::new();
    let mut admitted = vec![false; n];
    row.push(0);
    for (v, ok) in admitted.iter_mut().enumerate() {
        let mut es: Vec<(usize, usize)> = p
            .out(v)
            .map(|(w, slot)| (w.index(), slot))
            .filter(|(w, _)| *w != v)
            .collect();
        // The first parallel edge is the one of lowest element
        // identifier, as GDS keeps the first in relationship order
        // (measured, rmp #65: cosine of zipf60 nodes 0 and 25).
        es.sort_unstable_by_key(|(w, slot)| (*w, p.edge_id(*slot)));
        es.dedup_by_key(|(w, _)| *w);
        let degree = es.len() as i64;
        if degree >= lo && degree <= hi {
            *ok = true;
            for (w, slot) in es {
                col.push(w);
                wts.push(if weighted { p.edge_prop(0, slot) } else { 1.0 });
            }
        }
        row.push(col.len());
    }
    let comp = if cfg.bool("useComponents") {
        Some(components(p))
    } else {
        None
    };
    let is_source = |v: usize| admitted[v] && sources_ok.as_ref().is_none_or(|f| f[v]);
    let is_target = |v: usize| admitted[v] && targets_ok.as_ref().is_none_or(|f| f[v]);
    let targets: Vec<usize> = (0..n).filter(|v| is_target(*v)).collect();
    r.mem.grow(r.sched.worker_count() * targets.len() * 16)?;
    let (row, col, wts, targets) = (&row, &col, &wts, &targets);
    let per_block = r.sched.map(|_, range| {
        let mut out: Vec<(usize, Vec<(f64, usize)>)> = Vec::new();
        for s in range.filter(|s| is_source(*s)) {
            let (a, wa) = (&col[row[s]..row[s + 1]], &wts[row[s]..row[s + 1]]);
            let mut list = Vec::new();
            for &t in targets {
                if t == s || comp.as_ref().is_some_and(|c| c[s] != c[t]) {
                    continue;
                }
                let (b, wb) = (&col[row[t]..row[t + 1]], &wts[row[t]..row[t + 1]]);
                let x = similarity(metric, a, wa, b, wb, weighted);
                if x >= cutoff {
                    list.push((x, t));
                }
            }
            keep_best(&mut list, k, top_k);
            trace!("source", "{s} {list:?}");
            out.push((s, list));
        }
        out
    })?;
    let compared = per_block.iter().map(Vec::len).sum::<usize>();
    let mut pairs: Vec<(f64, usize, usize)> = per_block
        .into_iter()
        .flatten()
        .flat_map(|(s, list)| list.into_iter().map(move |(x, t)| (x, s, t)))
        .collect();
    if nn > 0 {
        pairs.sort_by(|a, b| {
            let c = if top_n {
                b.0.total_cmp(&a.0)
            } else {
                a.0.total_cmp(&b.0)
            };
            c.then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
        });
        pairs.truncate(nn);
    }
    let mut rows = Vec::new();
    for (x, s, t) in pairs {
        r.emit(
            &mut rows,
            vec![
                Value::Int(p.ids[s].cast_signed()),
                Value::Int(p.ids[t].cast_signed()),
                Value::Float(x),
            ],
        )?;
    }
    let dist = distribution(&rows);
    Ok(Output::Rows {
        rows,
        extra: vec![
            (
                "nodesCompared",
                Value::Int(i64::try_from(compared).unwrap_or(i64::MAX)),
            ),
            ("similarityDistribution", dist),
        ],
    })
}

/// A KNN similarity of one property (module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnnMetric {
    Scalar,
    Cosine,
    Euclidean,
    Pearson,
    Jaccard,
    Overlap,
}

/// The similarity of two values of one property.
fn knn_similarity(m: KnnMetric, a: &[f64], b: &[f64]) -> f64 {
    use super::functions::{cosine, euclidean_distance};
    match m {
        KnnMetric::Scalar => 1.0 / (1.0 + (a[0] - b[0]).abs()),
        KnnMetric::Cosine => f64::midpoint(cosine(a, b), 1.0),
        KnnMetric::Euclidean => 1.0 / (1.0 + euclidean_distance(a, b)),
        KnnMetric::Pearson => f64::midpoint(pearson_raw(a, b), 1.0),
        KnnMetric::Jaccard | KnnMetric::Overlap => {
            let mut sa: Vec<u64> = a.iter().map(|x| x.to_bits()).collect();
            let mut sb: Vec<u64> = b.iter().map(|x| x.to_bits()).collect();
            sa.sort_unstable();
            sa.dedup();
            sb.sort_unstable();
            sb.dedup();
            let inter = sa.iter().filter(|x| sb.binary_search(x).is_ok()).count() as f64;
            if m == KnnMetric::Jaccard {
                let union = sa.len() as f64 + sb.len() as f64 - inter;
                if union == 0.0 { 0.0 } else { inter / union }
            } else {
                let den = sa.len().min(sb.len()) as f64;
                if den == 0.0 { 0.0 } else { inter / den }
            }
        }
    }
}

/// Pearson's r without GDS's 0 for undefined (KNN keeps NaN, GDS
/// `Pearson.doubleMetric`).
fn pearson_raw(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut dot, mut xx, mut yy) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (x - ma, y - mb);
        dot += dx * dy;
        xx += dx * dx;
        yy += dy * dy;
    }
    dot / (xx * yy).sqrt()
}

/// The properties and metrics of a KNN run.
struct Props<'p> {
    vectors: Vec<(&'p super::projection::Vectors, KnnMetric)>,
}

impl Props<'_> {
    fn similarity(&self, u: usize, v: usize) -> f64 {
        let mut sum = 0.0;
        for (vs, m) in &self.vectors {
            sum += knn_similarity(*m, vs.of(u), vs.of(v));
        }
        sum / self.vectors.len() as f64
    }
}

/// One neighbour of a KNN list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Nb {
    id: usize,
    sim: f64,
    new: bool,
}

/// Inserts `(id, sim)` into the sorted list `l` of at most `k` entries
/// (best first, ties to the lower index) unless `id` is in it or it is
/// no better than the worst of a full list; `force` replaces the worst
/// with a candidate of equal similarity (GDS `NeighborList.add`:107-115,
/// `perturbationRate`); returns whether the list changed.
fn update(l: &mut Vec<Nb>, k: usize, id: usize, sim: f64, force: bool) -> bool {
    if l.iter().any(|x| x.id == id) {
        return false;
    }
    let pos = l
        .iter()
        .position(|x| sim > x.sim || (sim == x.sim && id < x.id))
        .unwrap_or(l.len());
    if pos >= k {
        if !force || l.last().is_none_or(|w| w.sim != sim) {
            return false;
        }
        l.pop();
        l.push(Nb { id, sim, new: true });
        return true;
    }
    l.insert(pos, Nb { id, sim, new: true });
    l.truncate(k);
    true
}

/// `count` distinct members of `from` drawn uniformly (a partial
/// Fisher-Yates shuffle), in drawing order.
fn sample(g: &mut rand::rngs::ChaCha8Rng, from: &[usize], count: usize) -> Vec<usize> {
    let mut pool = from.to_vec();
    let count = count.min(pool.len());
    for i in 0..count {
        let j = g.random_range(i..pool.len());
        pool.swap(i, j);
    }
    pool.truncate(count);
    pool
}

fn run_knn(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => knn(p, r))
}

fn knn<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let cfg = r.cfg;
    let n = p.node_count();
    let names = cfg.props("nodeProperties");
    let mut vectors = Vec::new();
    for (i, (name, metric)) in names.iter().enumerate() {
        let vs = &p.vectors[i];
        let m = match metric.as_deref() {
            None if vs.scalar => KnnMetric::Scalar,
            None | Some("COSINE") => KnnMetric::Cosine,
            Some("EUCLIDEAN") => KnnMetric::Euclidean,
            Some("PEARSON") => KnnMetric::Pearson,
            Some("JACCARD") => KnnMetric::Jaccard,
            Some("OVERLAP") => KnnMetric::Overlap,
            Some(other) => {
                return Err(invalid(&format!(
                    "unknown similarity metric {other} for the property {name}"
                )));
            }
        };
        let len = (0..n).map(|v| vs.of(v).len()).max().unwrap_or(0);
        if (0..n).any(|v| vs.of(v).is_empty() || vs.of(v).len() != len) {
            return Err(invalid(&format!(
                "every node needs the property {name}, with values of one length"
            )));
        }
        vectors.push((vs, m));
    }
    let props = Props { vectors };
    let k = usize::try_from(cfg.int("topK")).unwrap_or(usize::MAX);
    let sources_ok = filter(p, r, "sourceNodeFilter");
    let targets_ok = filter(p, r, "targetNodeFilter");
    let is_target = |v: usize| targets_ok.as_ref().is_none_or(|f| f[v]);
    let targets: Vec<usize> = (0..n).filter(|v| is_target(*v)).collect();
    let seed = cfg.seed();
    let rate = cfg.float("sampleRate");
    if rate <= 0.0 {
        // GDS `KnnBaseConfig.sampleRate`: (0, 1].
        return Err(invalid("sampleRate must be above 0"));
    }
    let rho = ((rate * k as f64) as usize).max(1);
    let joins = usize::try_from(cfg.int("randomJoins")).unwrap_or(0);
    let walk = cfg.str("initialSampler") == Some("RANDOMWALK");
    let perturbation = cfg.float("perturbationRate");
    let max_iterations = cfg.int("maxIterations").cast_unsigned();
    let delta = cfg.float("deltaThreshold");
    let sched = r.sched;
    r.mem.grow(
        n * k * size_of::<Nb>() * 3 + n * 4 * k * size_of::<usize>() + sched.worker_count() * n * 8,
    )?;
    // Initial samples.
    let targets_ref = &targets;
    // One generator per block, kept across the rounds (TS-25 Randomness).
    let blocks = n.div_ceil(super::sched::block_size(n));
    let gens: Vec<Mutex<rand::rngs::ChaCha8Rng>> =
        (0..blocks).map(|b| Mutex::new(rng(seed, b))).collect();
    let gen_of = |b: usize| gens[b].lock().unwrap_or_else(PoisonError::into_inner);
    let mut lists: Vec<Vec<Nb>> = vec![Vec::new(); n];
    sched.fill(&mut lists, |b, range, out| {
        let mut g = gen_of(b);
        for (l, v) in out.iter_mut().zip(range) {
            let mut picked: Vec<usize> = Vec::new();
            if walk {
                let mut at = v;
                for _ in 0..(4 * k) {
                    let next: Vec<usize> =
                        p.out(at).chain(p.inc(at)).map(|(w, _)| w.index()).collect();
                    if next.is_empty() || picked.len() >= k {
                        break;
                    }
                    at = next[g.random_range(0..next.len())];
                    if at != v && is_target(at) && !picked.contains(&at) {
                        picked.push(at);
                    }
                }
            }
            let rest: Vec<usize> = targets_ref
                .iter()
                .copied()
                .filter(|t| *t != v && !picked.contains(t))
                .collect();
            let more = sample(&mut g, &rest, k.saturating_sub(picked.len()));
            picked.extend(more);
            trace!("init", "{v} {picked:?}");
            for t in picked {
                update(l, k, t, props.similarity(v, t), false);
            }
        }
    })?;
    let mut ran = 0u64;
    let mut converged = false;
    let mut considered = 0u64;
    let mut perturb = rng(seed, blocks);
    while ran < max_iterations {
        r.mem.check_now()?;
        ran += 1;
        // Old and sampled new neighbours, by blocks.
        let mut olds: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut news: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut split: Vec<(Vec<usize>, Vec<usize>, Vec<Nb>)> = lists
            .iter()
            .map(|l| (Vec::new(), Vec::new(), l.clone()))
            .collect();
        sched.fill(&mut split, |b, range, out| {
            let mut g = gen_of(b);
            for (slot, _v) in out.iter_mut().zip(range) {
                let (old, new, l) = slot;
                let fresh: Vec<usize> = (0..l.len()).filter(|i| l[*i].new).collect();
                for x in l.iter().filter(|x| !x.new) {
                    old.push(x.id);
                }
                for i in sample(&mut g, &fresh, rho) {
                    new.push(l[i].id);
                    l[i].new = false;
                }
                trace!("split", "{_v} old={old:?} new={new:?}");
            }
        })?;
        for (v, (old, new, l)) in split.into_iter().enumerate() {
            lists[v] = l;
            olds[v] = old;
            news[v] = new;
        }
        let mut old_rev: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut new_rev: Vec<Vec<usize>> = vec![Vec::new(); n];
        for v in 0..n {
            for u in &olds[v] {
                old_rev[*u].push(v);
            }
            for u in &news[v] {
                new_rev[*u].push(v);
            }
        }
        let (olds, news, old_rev, new_rev) = (&olds, &news, &old_rev, &new_rev);
        let proposals = sched.map(|b, range| {
            let mut g = gen_of(b);
            let mut out: Vec<(usize, usize, f64)> = Vec::new();
            let mut pairs = 0u64;
            for v in range {
                let mut old = olds[v].clone();
                for u in sample(&mut g, &old_rev[v], rho) {
                    if !old.contains(&u) {
                        old.push(u);
                    }
                }
                let mut new = news[v].clone();
                for u in sample(&mut g, &new_rev[v], rho) {
                    if !new.contains(&u) {
                        new.push(u);
                    }
                }
                trace!("join", "{v} old={old:?} new={new:?}");
                let mut propose = |a: usize, b: usize, out: &mut Vec<(usize, usize, f64)>| {
                    pairs += 1;
                    let s = props.similarity(a, b);
                    if is_target(b) {
                        out.push((a, b, s));
                    }
                    if is_target(a) {
                        out.push((b, a, s));
                    }
                };
                for i in 0..new.len() {
                    for j in (i + 1)..new.len() {
                        propose(new[i], new[j], &mut out);
                    }
                    for o in &old {
                        if *o != new[i] {
                            propose(new[i], *o, &mut out);
                        }
                    }
                }
                for _ in 0..joins {
                    if targets_ref.is_empty() {
                        break;
                    }
                    let u = targets_ref[g.random_range(0..targets_ref.len())];
                    trace!("random", "{v} {u}");
                    if u != v {
                        pairs += 1;
                        out.push((v, u, props.similarity(v, u)));
                    }
                }
            }
            (out, pairs)
        })?;
        let mut updates = 0u64;
        for (block, pairs) in proposals {
            considered += pairs;
            for (a, b, s) in block {
                if a == b {
                    continue;
                }
                let force = perturbation > 0.0 && perturb.random::<f64>() < perturbation;
                if update(&mut lists[a], k, b, s, force) {
                    trace!("update", "{a} {b} {s:?}");
                    updates += 1;
                }
            }
        }
        trace!(
            "round",
            "{ran} {updates} {:?}",
            lists
                .iter()
                .map(|l| l.iter().map(|x| x.id).collect::<Vec<usize>>())
                .collect::<Vec<_>>()
        );
        if (updates as f64) < delta * k as f64 * n as f64 {
            converged = true;
            break;
        }
    }
    let cutoff = cfg.float("similarityCutoff");
    let mut rows = Vec::new();
    let mut compared = 0u64;
    for v in 0..n {
        if sources_ok.as_ref().is_some_and(|f| !f[v]) {
            continue;
        }
        compared += 1;
        for nb in &lists[v] {
            if nb.sim >= cutoff {
                r.emit(
                    &mut rows,
                    vec![
                        Value::Int(p.ids[v].cast_signed()),
                        Value::Int(p.ids[nb.id].cast_signed()),
                        Value::Float(nb.sim),
                    ],
                )?;
            }
        }
    }
    let dist = distribution(&rows);
    let int = |x: u64| Value::Int(i64::try_from(x).unwrap_or(i64::MAX));
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("nodesCompared", int(compared)),
            ("didConverge", Value::Bool(converged)),
            ("ranIterations", int(ran)),
            ("nodePairsConsidered", int(considered)),
            ("similarityDistribution", dist),
        ],
    })
}
