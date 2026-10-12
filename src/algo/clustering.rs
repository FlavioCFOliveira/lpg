//! The clustering entries of the community family (rmp #66): ALG-080
//! k-means, ALG-081 HDBSCAN, ALG-076 spectral clustering and ALG-077
//! balanced-cut clustering. All four are parity entries (TS-25): verified
//! by determinism and a quality measure.
//!
//! * **K-means kernel** ([`kmeans`], shared by 080, 076 and 077): Lloyd's
//!   iterations. Each iteration assigns every point to its nearest
//!   centroid (squared Euclidean distance, the lower centroid on a tie) by
//!   blocks, each block returning its centroid sums and counts, which are
//!   added in block order; a centroid without points keeps its place. The
//!   run stops when the share of points that changed centroid is below
//!   `deltaThreshold` (GDS), or when no centroid moved by more than a
//!   tolerance (cuGraph's `kmean_tolerance`), or after the iteration
//!   limit. The best of the restarts by inertia (sum of squared distances)
//!   wins, the first on a tie.
//! * **Initialisation** (the generator of stream 0, the run's sequential
//!   part): `UNIFORM` draws `k` distinct points; `KMEANS++` draws the
//!   first point uniformly and each next one with probability
//!   proportional to its squared distance to the nearest chosen centroid
//!   (linfa-clustering `k_means/init.rs` `weighted_k_means_plusplus`; GDS
//!   `kmeans/KmeansPlusPlusSampler.java`, studied); `seedCentroids` gives
//!   the centroids.
//! * **080** (GDS `kmeans/Kmeans.java`, studied): `nodeProperty` is a
//!   number or a list of numbers per node; `distanceFromCentroid` is the
//!   Euclidean distance; `silhouette` is `(b − a) / max(a, b)` with `a`
//!   the mean distance to the node's own cluster and `b` the least mean
//!   distance to another, when `computeSilhouette` is true, else −1 as
//!   GDS streams it (measured, rmp #66).
//! * **081 HDBSCAN** (Campello, Moulavi and Sander, 2013; the hdbscan
//!   crate, below): the core distance of a point is the distance to its
//!   `samples`-th nearest point, itself included, computed by blocks on
//!   the projected vectors, all pairs, so no k-d tree is needed; Prim's
//!   minimum spanning tree of the mutual-reachability distances, the
//!   single-linkage tree, its condensation by `minClusterSize`, and the
//!   clusters of greatest stability (excess of mass). Noise is labelled
//!   −1. `leafSize` tunes GDS's k-d tree and changes nothing here.
//! * **076, 077** (MAGE's cuGraph procedures; cuGraph @ `c9406a29ba00`
//!   `cpp/src/community/legacy/spectral_clustering.cu:36,110`, Apache-2.0,
//!   method only: a spectral embedding of the graph, then k-means): the
//!   graph is the run's undirected [`Net`], an edge without
//!   `weight_property` weighing 1. 076 embeds each node by the
//!   eigenvectors 2 to `num_eigenvectors + 1` of the normalised Laplacian
//!   `I − D^{-1/2} A D^{-1/2}`, each row scaled to unit length (Ng, Jordan
//!   and Weiss, 2001); 077 by those of the Laplacian `D − A`, the
//!   relaxation of the ratio cut (von Luxburg, "A tutorial on spectral
//!   clustering", 2007). Eigenvectors come from nalgebra's dense
//!   `SymmetricEigen` (TS-25), sorted by eigenvalue, the lower index on a
//!   tie; it iterates to machine precision, so `ev_tolerance` and
//!   `ev_max_iter` change nothing. K-means then runs with KMEANS++, 10
//!   restarts (cuVS `n_init` 10), `kmean_max_iter` iterations and
//!   `kmean_tolerance` as the centroid tolerance.
//!
//! **Origins.** The k-means++ initialisation and Lloyd's step are ported
//! from linfa-clustering @ `3b411a4c985c`
//! (<https://github.com/rust-ml/linfa>, MIT OR Apache-2.0, used under
//! MIT; "Copyright (c) 2015" in its `LICENSE-MIT`),
//! `algorithms/linfa-clustering/src/k_means/{algorithm.rs, init.rs}`,
//! modified: flat arrays, block sums, lpg's generator. HDBSCAN is copied
//! from hdbscan @ `82ac6aa9a378` (<https://github.com/tom-whitehead/hdbscan>,
//! MIT OR Apache-2.0, used under MIT; Copyright (c) 2025 hdbscan
//! Developers), `src/hdbscan.rs` (`make_single_linkage_tree`,
//! `condense_tree`, `extract_winning_clusters`, `label_data` and their
//! helpers), `src/min_spanning_tree.rs` (Prim), `src/union_find.rs`,
//! `src/core_distances/serial.rs` (brute force), modified: `f64` only,
//! flat arrays, no epsilon or single-cluster options, core distances by
//! blocks. Both licences (MIT):
//!
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

use std::collections::VecDeque;

use rand::RngExt;
use rand::rngs::ChaCha8Rng;

use super::community::Net;
use super::config::{Def, Key, Ty};
use super::projection::{Ix, Projection, with_projection};
use super::sched::{Sched, rng};
use super::{Arg, ArgDefault, Entry, Output, Run};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// How a k-means run stops (module documentation).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Stop {
    /// The share of points that changed centroid is below this.
    Changed(f64),
    /// No centroid moved by more than this.
    Shift(f64),
}

/// How k-means starts (module documentation).
#[derive(Debug, Clone)]
pub(crate) enum Init {
    Uniform,
    PlusPlus,
    Given(Vec<f64>),
}

/// The result of a k-means run.
#[derive(Debug, Clone)]
pub(crate) struct Clusters {
    pub(crate) assign: Vec<usize>,
    /// `k × d`, row-major.
    pub(crate) centroids: Vec<f64>,
    pub(crate) inertia: f64,
}

fn sq(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// The initial centroids (module documentation).
fn init(x: &[f64], d: usize, k: usize, how: &Init, g: &mut ChaCha8Rng) -> Vec<f64> {
    let n = x.len() / d;
    let point = |i: usize| &x[i * d..(i + 1) * d];
    match how {
        Init::Given(c) => c.clone(),
        Init::Uniform => {
            let mut pool: Vec<usize> = (0..n).collect();
            let mut out = Vec::with_capacity(k * d);
            for i in 0..k {
                let j = g.random_range(i..n);
                pool.swap(i, j);
                out.extend_from_slice(point(pool[i]));
            }
            out
        }
        Init::PlusPlus => {
            let mut out = Vec::with_capacity(k * d);
            out.extend_from_slice(point(g.random_range(0..n)));
            let mut dist: Vec<f64> = (0..n).map(|i| sq(point(i), &out[0..d])).collect();
            for c in 1..k {
                let total: f64 = dist.iter().sum();
                let pick = if total > 0.0 {
                    let r = g.random::<f64>() * total;
                    let mut acc = 0.0;
                    let mut pick = n - 1;
                    for (i, x) in dist.iter().enumerate() {
                        acc += x;
                        if acc > r {
                            pick = i;
                            break;
                        }
                    }
                    pick
                } else {
                    0
                };
                out.extend_from_slice(point(pick));
                let added = &out[c * d..(c + 1) * d];
                for (i, x) in dist.iter_mut().enumerate() {
                    *x = x.min(sq(point(i), added));
                }
            }
            out
        }
    }
}

/// K-means on the `n × d` points `x` (module documentation).
#[allow(clippy::too_many_arguments, reason = "the parameters of one run")]
pub(crate) fn kmeans(
    x: &[f64],
    d: usize,
    k: usize,
    max_iterations: usize,
    stop: Stop,
    how: &Init,
    restarts: usize,
    g: &mut ChaCha8Rng,
    sched: &Sched,
) -> Result<Clusters, Error> {
    let n = x.len() / d;
    let mut best: Option<Clusters> = None;
    for _ in 0..restarts.max(1) {
        let mut c = init(x, d, k, how, g);
        let mut assign = vec![usize::MAX; n];
        for _ in 0..max_iterations {
            let cref = &c;
            let blocks = sched.fill(&mut assign, |_, range, out| {
                let mut sums = vec![0.0; k * d];
                let mut counts = vec![0usize; k];
                let mut changed = 0usize;
                for (a, i) in out.iter_mut().zip(range) {
                    let p = &x[i * d..(i + 1) * d];
                    let mut bestc = 0;
                    let mut bestd = f64::INFINITY;
                    for j in 0..k {
                        let dd = sq(p, &cref[j * d..(j + 1) * d]);
                        if dd < bestd {
                            bestd = dd;
                            bestc = j;
                        }
                    }
                    if *a != bestc {
                        changed += 1;
                        *a = bestc;
                    }
                    counts[bestc] += 1;
                    for (s, v) in sums[bestc * d..(bestc + 1) * d].iter_mut().zip(p) {
                        *s += v;
                    }
                }
                (sums, counts, changed)
            })?;
            let mut sums = vec![0.0; k * d];
            let mut counts = vec![0usize; k];
            let mut changed = 0;
            for (s, cnt, ch) in blocks {
                for (a, b) in sums.iter_mut().zip(s) {
                    *a += b;
                }
                for (a, b) in counts.iter_mut().zip(cnt) {
                    *a += b;
                }
                changed += ch;
            }
            let mut shift = 0.0f64;
            for j in 0..k {
                if counts[j] == 0 {
                    continue;
                }
                for t in 0..d {
                    let v = sums[j * d + t] / counts[j] as f64;
                    shift = shift.max((v - c[j * d + t]).abs());
                    c[j * d + t] = v;
                }
            }
            let done = match stop {
                Stop::Changed(delta) => (changed as f64) / (n as f64) < delta,
                Stop::Shift(tol) => shift <= tol,
            };
            if done {
                break;
            }
        }
        // Final assignment to the final centroids.
        let mut inertia = 0.0;
        for i in 0..n {
            let p = &x[i * d..(i + 1) * d];
            let mut bestc = 0;
            let mut bestd = f64::INFINITY;
            for j in 0..k {
                let dd = sq(p, &c[j * d..(j + 1) * d]);
                if dd < bestd {
                    bestd = dd;
                    bestc = j;
                }
            }
            assign[i] = bestc;
            inertia += bestd;
        }
        if best.as_ref().is_none_or(|b| inertia < b.inertia) {
            best = Some(Clusters {
                assign,
                centroids: c,
                inertia,
            });
        }
    }
    Ok(best.expect("at least one restart"))
}

/// The points of vector property `0`: `n × d`, row-major.
fn points<I: Ix>(p: &Projection<I>, key: &str) -> Result<(Vec<f64>, usize), Error> {
    let vs = &p.vectors[0];
    let n = p.node_count();
    let d = (0..n).map(|v| vs.of(v).len()).max().unwrap_or(0);
    if d == 0 || (0..n).any(|v| vs.of(v).len() != d || vs.of(v).iter().any(|x| x.is_nan())) {
        return Err(invalid(&format!(
            "every node needs the property {key}, numbers of one dimension"
        )));
    }
    Ok((vs.val.clone(), d))
}

/// ALG-080: `gds.kmeans.stream`, `gds.kmeans.write`.
pub(crate) const KMEANS: Entry = Entry {
    algs: &[80],
    stream: "gds.kmeans.stream",
    write: Some("gds.kmeans.write"),
    keys: &[
        Key::new("nodeProperty", Ty::Str, Def::Required),
        Key::new("k", Ty::Int { min: 1 }, Def::Int(10)),
        Key::new("maxIterations", Ty::Int { min: 1 }, Def::Int(10)),
        Key::new(
            "deltaThreshold",
            Ty::Float {
                min: 0.0,
                max: 1.0,
                max_open: false,
            },
            Def::Float(0.05),
        ),
        Key::new("numberOfRestarts", Ty::Int { min: 1 }, Def::Int(1)),
        Key::new(
            "initialSampler",
            Ty::Choice(&["UNIFORM", "KMEANS++"]),
            Def::Str("UNIFORM"),
        ),
        Key::new("seedCentroids", Ty::Lists, Def::EmptyList),
        Key::new("computeSilhouette", Ty::Bool, Def::Bool(false)),
    ],
    parallel: true,
    randomised: true,
    vector_props: &["nodeProperty"],
    stream_columns: &[
        "nodeId",
        "communityId",
        "distanceFromCentroid",
        "silhouette",
    ],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "communityDistribution",
        "centroids",
        "averageDistanceToCentroid",
        "averageSilhouette",
        "configuration",
    ],
    run: run_kmeans,
    ..Entry::BASE
};

fn run_kmeans(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => kmeans_entry(p, r))
}

fn kmeans_entry<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let (x, d) = points(p, r.cfg.str("nodeProperty").unwrap_or_default())?;
    r.mem
        .grow(x.len() * 8 + n * 32 + r.sched.worker_count() * 1024)?;
    let given: Vec<Vec<f64>> = match r.cfg.get("seedCentroids") {
        Value::List(l) => l
            .iter()
            .map(|c| match c {
                Value::List(xs) => xs
                    .iter()
                    .map(|v| match v {
                        Value::Int(i) => Ok(*i as f64),
                        Value::Float(f) => Ok(*f),
                        _ => Err(invalid("seedCentroids must be lists of numbers")),
                    })
                    .collect::<Result<Vec<f64>, _>>(),
                _ => Err(invalid("seedCentroids must be lists of numbers")),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(invalid("seedCentroids must be a list of lists of numbers")),
    };
    let mut k = usize::try_from(r.cfg.int("k")).unwrap_or(usize::MAX);
    let how = if !given.is_empty() {
        if given.iter().any(|c| c.len() != d) {
            return Err(invalid(
                "each seed centroid needs the dimension of nodeProperty",
            ));
        }
        k = given.len();
        Init::Given(given.concat())
    } else if r.cfg.str("initialSampler") == Some("KMEANS++") {
        Init::PlusPlus
    } else {
        Init::Uniform
    };
    if k > n {
        return Err(invalid("k must be at most the number of nodes"));
    }
    let mut g = rng(r.cfg.seed(), 0);
    let res = kmeans(
        &x,
        d,
        k,
        usize::try_from(r.cfg.int("maxIterations")).unwrap_or(usize::MAX),
        Stop::Changed(r.cfg.float("deltaThreshold")),
        &how,
        usize::try_from(r.cfg.int("numberOfRestarts")).unwrap_or(1),
        &mut g,
        r.sched,
    )?;
    let point = |i: usize| &x[i * d..(i + 1) * d];
    let silhouette: Option<Vec<f64>> = r.cfg.bool("computeSilhouette").then(|| {
        (0..n)
            .map(|i| {
                let mut sum = vec![0.0; k];
                let mut cnt = vec![0usize; k];
                for j in 0..n {
                    if j != i {
                        sum[res.assign[j]] += sq(point(i), point(j)).sqrt();
                        cnt[res.assign[j]] += 1;
                    }
                }
                let own = res.assign[i];
                if cnt[own] == 0 {
                    return 0.0;
                }
                let a = sum[own] / cnt[own] as f64;
                let b = (0..k)
                    .filter(|c| *c != own && cnt[*c] > 0)
                    .map(|c| sum[c] / cnt[c] as f64)
                    .fold(f64::INFINITY, f64::min);
                if b.is_infinite() {
                    0.0
                } else {
                    (b - a) / a.max(b)
                }
            })
            .collect()
    });
    let mut rows = Vec::new();
    let mut total_d = 0.0;
    for i in 0..n {
        let c = res.assign[i];
        let dist = sq(point(i), &res.centroids[c * d..(c + 1) * d]).sqrt();
        total_d += dist;
        let s = silhouette
            .as_ref()
            .map_or(Value::Float(-1.0), |s| Value::Float(s[i]));
        r.emit(
            &mut rows,
            vec![
                Value::Int(p.ids[i].cast_signed()),
                Value::Int(c as i64),
                Value::Float(dist),
                s,
            ],
        )?;
    }
    let centroids = Value::List(
        res.centroids
            .chunks(d)
            .map(|c| {
                Value::List(
                    c.iter()
                        .map(|x| Value::Float(*x))
                        .collect::<Vec<_>>()
                        .into(),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    );
    let avg_s = silhouette.map_or(0.0, |s| s.iter().sum::<f64>() / n as f64);
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("communityDistribution", size_distribution(&res.assign)),
            ("centroids", centroids),
            (
                "averageDistanceToCentroid",
                Value::Float(total_d / n as f64),
            ),
            ("averageSilhouette", Value::Float(avg_s)),
            ("inertia", Value::Float(res.inertia)),
        ],
    })
}

/// `communityDistribution` of an assignment.
fn size_distribution(assign: &[usize]) -> Value {
    let mut counts = std::collections::BTreeMap::<usize, f64>::new();
    for c in assign {
        *counts.entry(*c).or_default() += 1.0;
    }
    let xs: Vec<f64> = counts.into_values().collect();
    super::distribution(&xs)
}

/// ALG-081: `gds.hdbscan.stream`, `gds.hdbscan.write`.
pub(crate) const HDBSCAN: Entry = Entry {
    algs: &[81],
    stream: "gds.hdbscan.stream",
    write: Some("gds.hdbscan.write"),
    keys: &[
        Key::new("nodeProperty", Ty::Str, Def::Required),
        Key::new("minClusterSize", Ty::Int { min: 2 }, Def::Int(5)),
        Key::new("samples", Ty::Int { min: 1 }, Def::Int(10)),
        Key::new("leafSize", Ty::Int { min: 1 }, Def::Int(1)),
    ],
    parallel: true,
    vector_props: &["nodeProperty"],
    stream_columns: &["nodeId", "label"],
    write_columns: &[
        "nodeCount",
        "numberOfClusters",
        "numberOfNoisePoints",
        "writeMillis",
        "nodePropertiesWritten",
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "configuration",
    ],
    run: run_hdbscan,
    ..Entry::BASE
};

fn run_hdbscan(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => hdbscan_entry(p, r))
}

fn hdbscan_entry<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let (x, d) = points(p, r.cfg.str("nodeProperty").unwrap_or_default())?;
    let samples = usize::try_from(r.cfg.int("samples")).unwrap_or(usize::MAX);
    let min_size = usize::try_from(r.cfg.int("minClusterSize")).unwrap_or(usize::MAX);
    if samples > n {
        return Err(invalid("samples must be at most the number of nodes"));
    }
    r.mem
        .grow(x.len() * 8 + n * 80 + r.sched.worker_count() * n * 8)?;
    // Core distances by blocks (module documentation).
    let mut core = vec![0.0f64; n];
    let xr = &x;
    r.sched.fill(&mut core, |_, range, out| {
        let mut row = vec![0.0f64; n];
        for (c, i) in out.iter_mut().zip(range) {
            for (j, dj) in row.iter_mut().enumerate() {
                *dj = sq(&xr[i * d..(i + 1) * d], &xr[j * d..(j + 1) * d]).sqrt();
            }
            row.sort_by(f64::total_cmp);
            *c = row[samples - 1];
        }
    })?;
    let labels = hdbscan(&x, d, &core, min_size);
    let mut rows = Vec::new();
    for (id, label) in p.ids.iter().zip(&labels) {
        r.emit(
            &mut rows,
            vec![Value::Int(id.cast_signed()), Value::Int(*label)],
        )?;
    }
    let clusters = labels
        .iter()
        .filter(|l| **l >= 0)
        .max()
        .map_or(0, |m| m + 1);
    let noise = labels.iter().filter(|l| **l < 0).count() as i64;
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("nodeCount", Value::Int(n as i64)),
            ("numberOfClusters", Value::Int(clusters)),
            ("numberOfNoisePoints", Value::Int(noise)),
        ],
    })
}

/// An edge of the single-linkage tree.
struct Link {
    left: usize,
    right: usize,
    distance: f64,
    size: usize,
}

/// A node of the condensed tree.
struct Condensed {
    node: usize,
    parent: usize,
    lambda: f64,
    size: usize,
}

/// hdbscan's union-find over `2n − 1` labels.
struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
    next: usize,
}

impl UnionFind {
    fn new(n: usize) -> UnionFind {
        let len = 2 * n - 1;
        UnionFind {
            parent: vec![len; len],
            size: (0..len).map(|i| usize::from(i < n)).collect(),
            next: n,
        }
    }

    fn union(&mut self, m: usize, n: usize) {
        self.parent[m] = self.next;
        self.parent[n] = self.next;
        self.size[self.next] = self.size[m] + self.size[n];
        self.next += 1;
    }

    fn find(&mut self, mut n: usize) -> usize {
        let len = self.parent.len();
        let mut p = n;
        while self.parent[n] != len {
            n = self.parent[n];
        }
        while p != n && self.parent[p] != n {
            let next = self.parent[p];
            self.parent[p] = n;
            p = next;
        }
        n
    }
}

/// The HDBSCAN labels of the points (module documentation): −1 for
/// noise, clusters numbered from 0 in the order of their condensed-tree
/// identifiers.
fn hdbscan(x: &[f64], d: usize, core: &[f64], min_size: usize) -> Vec<i64> {
    let n = core.len();
    if n < 2 {
        return vec![-1; n];
    }
    let point = |i: usize| &x[i * d..(i + 1) * d];
    let mrd = |a: usize, b: usize| core[a].max(core[b]).max(sq(point(a), point(b)).sqrt());
    // Prim.
    let mut in_tree = vec![false; n];
    let mut dist = vec![f64::INFINITY; n];
    let mut mst: Vec<(usize, usize, f64)> = Vec::with_capacity(n - 1);
    let mut left = 0;
    let mut right = 0;
    for _ in 1..n {
        in_tree[left] = true;
        let mut cur = f64::INFINITY;
        for i in 0..n {
            if in_tree[i] {
                continue;
            }
            let m = mrd(left, i);
            if m < dist[i] {
                dist[i] = m;
            }
            if dist[i] < cur {
                right = i;
                cur = dist[i];
            }
        }
        mst.push((left, right, cur));
        left = right;
    }
    mst.sort_by(|a, b| a.2.total_cmp(&b.2));
    // Single linkage.
    let mut uf = UnionFind::new(n);
    let mut slt: Vec<Link> = Vec::with_capacity(n - 1);
    for (a, b, distance) in mst {
        let (l, r) = (uf.find(a), uf.find(b));
        let size = uf.size[l] + uf.size[r];
        slt.push(Link {
            left: l,
            right: r,
            distance,
            size,
        });
        uf.union(l, r);
    }
    // Condensation.
    let children = |root: usize, slt: &[Link]| -> Vec<usize> {
        let mut q = VecDeque::from([root]);
        let mut out = Vec::new();
        while let Some(c) = q.pop_front() {
            out.push(c);
            if c >= n {
                q.push_back(slt[c - n].left);
                q.push_back(slt[c - n].right);
            }
        }
        out
    };
    let size_of = |c: usize, slt: &[Link]| if c < n { 1 } else { slt[c - n].size };
    let top = (n - 1) * 2;
    let order = children(top, &slt);
    let mut new_ids = vec![0usize; top + 1];
    new_ids[top] = n;
    let mut next_parent = n + 1;
    let mut visited = vec![false; top + 1];
    let mut tree: Vec<Condensed> = Vec::new();
    for c in order {
        if visited[c] || c < n {
            continue;
        }
        let link = &slt[c - n];
        let lambda = if link.distance > 0.0 {
            1.0 / link.distance
        } else {
            f64::INFINITY
        };
        let (l, r) = (link.left, link.right);
        let (ls, rs) = (size_of(l, &slt), size_of(r, &slt));
        let add_children =
            |child: usize, parent: usize, tree: &mut Vec<Condensed>, visited: &mut Vec<bool>| {
                for x in children(child, &slt) {
                    if x < n {
                        tree.push(Condensed {
                            node: x,
                            parent,
                            lambda,
                            size: 1,
                        });
                    }
                    visited[x] = true;
                }
            };
        match (ls >= min_size, rs >= min_size) {
            (true, true) => {
                for (child, size) in [(l, ls), (r, rs)] {
                    new_ids[child] = next_parent;
                    next_parent += 1;
                    tree.push(Condensed {
                        node: new_ids[child],
                        parent: new_ids[c],
                        lambda,
                        size,
                    });
                }
            }
            (false, false) => {
                let id = new_ids[c];
                add_children(l, id, &mut tree, &mut visited);
                add_children(r, id, &mut tree, &mut visited);
            }
            (false, true) => {
                new_ids[r] = new_ids[c];
                add_children(l, new_ids[c], &mut tree, &mut visited);
            }
            (true, false) => {
                new_ids[l] = new_ids[c];
                add_children(r, new_ids[c], &mut tree, &mut visited);
            }
        }
    }
    // Excess-of-mass selection.
    let lower = n + 1;
    let upper = lower + tree.len().saturating_sub(n);
    let birth =
        |c: usize, tree: &[Condensed]| tree.iter().find(|t| t.node == c).map_or(0.0, |t| t.lambda);
    let mut stability: Vec<f64> = (lower..upper)
        .map(|c| {
            let b = birth(c, &tree);
            tree.iter()
                .filter(|t| t.parent == c)
                .map(|t| (t.lambda - b) * t.size as f64)
                .sum()
        })
        .collect();
    let mut selected = vec![false; upper - lower];
    let descendants = |root: usize, tree: &[Condensed]| -> Vec<usize> {
        let mut q = VecDeque::from([root]);
        let mut out = Vec::new();
        while let Some(c) = q.pop_front() {
            for t in tree {
                if t.node >= n && t.parent == c {
                    out.push(t.node);
                    q.push_back(t.node);
                }
            }
        }
        out
    };
    for c in (lower..upper).rev() {
        let child: f64 = tree
            .iter()
            .filter(|t| t.parent == c && t.node >= n)
            .map(|t| stability[t.node - lower])
            .sum();
        if stability[c - lower] > child {
            selected[c - lower] = true;
            for x in descendants(c, &tree) {
                selected[x - lower] = false;
            }
        } else {
            stability[c - lower] = child;
        }
    }
    // Labels.
    let mut labels = vec![-1i64; n];
    let winners: Vec<usize> = (lower..upper).filter(|c| selected[c - lower]).collect();
    for (label, c) in winners.iter().enumerate() {
        let mut q = VecDeque::from([*c]);
        while let Some(cur) = q.pop_front() {
            for t in &tree {
                if t.parent != cur {
                    continue;
                }
                if t.node >= n {
                    q.push_back(t.node);
                } else {
                    labels[t.node] = label as i64;
                }
            }
        }
    }
    labels
}

/// The positional parameters of ALG-076 and ALG-077 (MAGE).
const SPECTRAL_ARGS: &[Arg] = &[
    Arg::new("num_clusters", ArgDefault::Required),
    Arg::new("num_eigenvectors", ArgDefault::Int(2)),
    Arg::new("ev_tolerance", ArgDefault::Float(0.00001)),
    Arg::new("ev_max_iter", ArgDefault::Int(100)),
    Arg::new("kmean_tolerance", ArgDefault::Float(0.00001)),
    Arg::new("kmean_max_iter", ArgDefault::Int(20)),
    Arg::new("weight_property", ArgDefault::Str("weight")),
];

/// ALG-076: `cugraph.spectral_clustering.get` (MAGE; no write mode).
pub(crate) const SPECTRAL: Entry = Entry {
    algs: &[76],
    stream: "cugraph.spectral_clustering.get",
    randomised: true,
    undirected: true,
    stream_columns: &["node", "cluster"],
    run: run_spectral,
    args: SPECTRAL_ARGS,
    edge_args: &[6],
    ..Entry::BASE
};

/// ALG-077: `cugraph.balanced_cut_clustering.get` (MAGE; no write mode).
pub(crate) const BALANCED_CUT: Entry = Entry {
    algs: &[77],
    stream: "cugraph.balanced_cut_clustering.get",
    run: run_balanced_cut,
    args: &[
        SPECTRAL_ARGS[0],
        SPECTRAL_ARGS[1],
        SPECTRAL_ARGS[2],
        SPECTRAL_ARGS[3],
        SPECTRAL_ARGS[4],
        Arg::new("kmean_max_iter", ArgDefault::Int(100)),
        SPECTRAL_ARGS[6],
    ],
    ..SPECTRAL
};

fn run_spectral(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => spectral(p, r, true))
}

fn run_balanced_cut(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => spectral(p, r, false))
}

/// An integer positional argument.
fn int_arg(v: &Value, name: &str) -> Result<i64, Error> {
    match v {
        Value::Int(i) => Ok(*i),
        _ => Err(Error::new(
            codes::C22G03,
            format!("invalid value type: the argument {name} must be an integer"),
        )),
    }
}

/// A float positional argument.
fn float_arg(v: &Value, name: &str) -> Result<f64, Error> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        _ => Err(Error::new(
            codes::C22G03,
            format!("invalid value type: the argument {name} must be a number"),
        )),
    }
}

/// The weighted net of ALG-076 and 077: an edge without the property
/// weighs 1.
fn spectral_net<I: Ix>(p: &Projection<I>) -> Net {
    if p.edge_props.is_empty() {
        return Net::of(p, false);
    }
    let mut q = p.clone();
    for w in &mut q.edge_props[0] {
        if w.is_nan() {
            *w = 1.0;
        }
    }
    Net::of(&q, true)
}

fn spectral<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    normalised: bool,
) -> Result<Output, Error> {
    let n = p.node_count();
    let k = int_arg(&r.args[0], "num_clusters")?;
    let e = int_arg(&r.args[1], "num_eigenvectors")?;
    let _ = float_arg(&r.args[2], "ev_tolerance")?;
    let _ = int_arg(&r.args[3], "ev_max_iter")?;
    let tol = float_arg(&r.args[4], "kmean_tolerance")?;
    let iters = int_arg(&r.args[5], "kmean_max_iter")?;
    if k < 2 || k as usize >= n.max(1) {
        return Err(invalid(
            "num_clusters must be more than 1 and less than the number of nodes",
        ));
    }
    if e < 1 || e > k || e as usize >= n {
        return Err(invalid(
            "num_eigenvectors must be at least 1, at most num_clusters and below the number of nodes",
        ));
    }
    let (k, e) = (k as usize, e as usize);
    let net = spectral_net(p);
    r.mem.grow(3 * n * n * 8 + n * e * 8)?;
    let mut m = nalgebra::DMatrix::<f64>::zeros(n, n);
    let deg = &net.k;
    for u in 0..n {
        for v in 0..n {
            let a = net.weight(u, v);
            let l = if u == v { deg[u] - a } else { -a };
            m[(u, v)] = if normalised {
                if deg[u] > 0.0 && deg[v] > 0.0 {
                    l / (deg[u] * deg[v]).sqrt()
                } else if u == v {
                    1.0
                } else {
                    0.0
                }
            } else {
                l
            };
        }
    }
    r.mem.check_now()?;
    let eig = nalgebra::SymmetricEigen::new(m);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        eig.eigenvalues[*a]
            .total_cmp(&eig.eigenvalues[*b])
            .then(a.cmp(b))
    });
    let mut x = vec![0.0f64; n * e];
    for i in 0..n {
        for (t, col) in order[1..=e].iter().enumerate() {
            x[i * e + t] = eig.eigenvectors[(i, *col)];
        }
        if normalised {
            let norm = x[i * e..(i + 1) * e]
                .iter()
                .map(|v| v * v)
                .sum::<f64>()
                .sqrt();
            if norm > 0.0 {
                for v in &mut x[i * e..(i + 1) * e] {
                    *v /= norm;
                }
            }
        }
    }
    let mut g = rng(r.cfg.seed(), 0);
    let res = kmeans(
        &x,
        e,
        k,
        usize::try_from(iters).unwrap_or(0).max(1),
        Stop::Shift(tol),
        &Init::PlusPlus,
        10,
        &mut g,
        r.sched,
    )?;
    let mut rows = Vec::new();
    for i in 0..n {
        r.emit(
            &mut rows,
            vec![Value::Node(p.ids[i]), Value::Int(res.assign[i] as i64)],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}
