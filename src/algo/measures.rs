//! The structural measures family (rmp #67): ALG-120 triangle count and
//! listing, ALG-121 local clustering coefficient, ALG-123 eccentricity,
//! ALG-128 girth, ALG-143 Laplacian matrix. 120 and 121 are parallel;
//! the others run on one worker.
//!
//! * **120, 121** (parallel) count over the simple undirected graph
//!   ([`super::kernels::simple`]): node `v`'s triangles are the edges
//!   among its neighbours, half the sum over its neighbours `w` of
//!   `|N(v) ∩ N(w)|`, the intersection of two sorted rows. Each block
//!   computes its own nodes' counts, so no count is shared between
//!   blocks; `gds.triangles` lists each triangle `(a, b, c)`, `a < b < c`,
//!   from its smallest node, blocks concatenated in order. graphina @
//!   `212717ec4749` `parallel/triangles.rs` (rayon) and LAGraph
//!   `LAGr_TriangleCount.c` count by the same row intersections (MIT and
//!   BSD methods, written here); GDS's `triangle/IntersectingTriangleCount
//!   .java` (studied) intersects sorted adjacency lists too. GDS's rules,
//!   measured on GDS 2026.09.0 (rmp #67): counts equal the simple graph's
//!   (parallel edges once, self-loops never); a node whose projected
//!   degree (parallel edges counted, a self-loop twice) exceeds
//!   `maxDegree` gets −1 and is in no triangle; the clustering
//!   coefficient is `2t / (d (d − 1))` with `d` the number of distinct
//!   neighbours, the node itself included when it has a self-loop
//!   (`LocalClusteringCoefficient.java`
//!   `degreeWithoutParallelRelationships`), 0 without triangles, NaN for
//!   an excluded node or a NaN `triangleCountProperty`. The average is
//!   the mean over every node, added in node order.
//! * **123** copies graphalgs @ `1f8a412ef4af` `src/metrics.rs`
//!   (`eccentricity`, `diameter`, `radius`, `weighted_eccentricity`; MIT,
//!   "Copyright (c) 2021 PrototypeRailGun", licence text in
//!   [`super::paths`]): a breadth-first search per node, or petgraph's
//!   `bellman_ford` with `relationshipWeightProperty`; the eccentricity is
//!   the largest distance, infinite when a node is unreachable; the
//!   diameter the largest and the radius the smallest eccentricity.
//!   Modified: `f64` instead of `f32`; a negative cycle fails with 22N11.
//! * **128**: graphalgs's `girth` (`metrics.rs:534`) is not copied: its
//!   directed form is a depth-first search that misses shorter cycles,
//!   and its undirected form does not mark the start node. lpg runs
//!   NetworkX's method (`cycles.py` `girth`): a breadth-first search from
//!   every node; on the undirected projection each edge between two nodes
//!   that are not tree parent and child closes a cycle of at most
//!   `depth(u) + depth(v) + 1` edges, and the smallest over all starts is
//!   the girth. Parallel edges count once, as in NetworkX's simple graphs
//!   and graphalgs's undirected form; a self-loop is a cycle of one edge.
//!   On a directed projection the cycles are directed: from each start
//!   `s`, an edge `u → s` closes one of `depth(u) + 1` edges.
//! * **143** copies graphalgs `src/spec/laplacian.rs` (`laplacian_matrix`,
//!   nalgebra `DMatrix`): each edge `u → v`, `u ≠ v`, adds its weight (1
//!   without `relationshipWeightProperty`) to `L[u][u]` and subtracts it
//!   from `L[u][v]`. Modified: the projection lists an undirected edge
//!   from both ends, so the directed branch builds the symmetric matrix;
//!   the matrix's `n²` floats are reserved first, so a projection whose
//!   matrix exceeds the memory cap fails with 51N72 or 51N73.

use std::collections::VecDeque;

use nalgebra::DMatrix;

use super::config::{Def, Key, Ty};
use super::kernels::{self, Simple, id_value};
use super::projection::{Ix, Orientation, Projection, with_projection};
use super::{Entry, Output, Run};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

const WEIGHT: Key = Key::new("relationshipWeightProperty", Ty::Str, Def::Absent);

const TRIANGLE_KEYS: &[Key] = &[
    Key::new("maxDegree", Ty::Int { min: 0 }, Def::Int(i64::MAX)),
    Key::new("labelFilter", Ty::Names, Def::EmptyList),
];

/// ALG-120: `gds.triangleCount.stream`, `.write`.
pub(crate) const TRIANGLE_COUNT: Entry = Entry {
    algs: &[120],
    stream: "gds.triangleCount.stream",
    write: Some("gds.triangleCount.write"),
    keys: TRIANGLE_KEYS,
    parallel: true,
    undirected: true,
    stream_columns: &["nodeId", "triangleCount"],
    write_columns: &[
        "globalTriangleCount",
        "nodeCount",
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "configuration",
        "postProcessingMillis",
    ],
    run: run_triangle_count,
    ..Entry::BASE
};

/// ALG-120: `gds.triangles` (stream only).
pub(crate) const TRIANGLES: Entry = Entry {
    algs: &[120],
    stream: "gds.triangles",
    keys: TRIANGLE_KEYS,
    parallel: true,
    undirected: true,
    stream_columns: &["nodeA", "nodeB", "nodeC"],
    run: run_triangles,
    ..Entry::BASE
};

/// ALG-121: `gds.localClusteringCoefficient.stream`, `.write`.
pub(crate) const LOCAL_CLUSTERING: Entry = Entry {
    algs: &[121],
    stream: "gds.localClusteringCoefficient.stream",
    write: Some("gds.localClusteringCoefficient.write"),
    keys: &[
        Key::new("triangleCountProperty", Ty::Str, Def::Absent),
        Key::new("forceSeedOptimization", Ty::Bool, Def::Bool(false)),
    ],
    parallel: true,
    undirected: true,
    node_props: &["triangleCountProperty"],
    stream_columns: &["nodeId", "localClusteringCoefficient"],
    write_columns: &[
        "averageClusteringCoefficient",
        "nodeCount",
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "configuration",
        "postProcessingMillis",
    ],
    run: run_local_clustering,
    ..Entry::BASE
};

/// ALG-123: `gds.eccentricity.stream`, `.write`.
pub(crate) const ECCENTRICITY: Entry = Entry {
    algs: &[123],
    stream: "gds.eccentricity.stream",
    write: Some("gds.eccentricity.write"),
    keys: &[WEIGHT],
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &["nodeId", "eccentricity"],
    write_columns: &[
        "nodePropertiesWritten",
        "diameter",
        "radius",
        "configuration",
    ],
    run: run_eccentricity,
    ..Entry::BASE
};

/// ALG-128: `gds.girth.stream`.
pub(crate) const GIRTH: Entry = Entry {
    algs: &[128],
    stream: "gds.girth.stream",
    stream_columns: &["girth"],
    run: run_girth,
    ..Entry::BASE
};

/// ALG-143: `gds.laplacianMatrix.stream`.
pub(crate) const LAPLACIAN: Entry = Entry {
    algs: &[143],
    stream: "gds.laplacianMatrix.stream",
    keys: &[WEIGHT],
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &["nodeId", "values"],
    run: run_laplacian,
    ..Entry::BASE
};

/// The nodes whose projected degree exceeds `maxDegree`, and fails on a
/// `labelFilter` (not supported: its GDS semantics are unverified).
fn excluded<I: Ix>(p: &Projection<I>, r: &Run<'_, '_>) -> Result<Vec<bool>, Error> {
    if !r.cfg.names("labelFilter").is_empty() {
        return Err(Error::new(
            codes::C22N11,
            "invalid argument: labelFilter is not supported",
        ));
    }
    let max = usize::try_from(r.cfg.int("maxDegree")).unwrap_or(usize::MAX);
    Ok((0..p.node_count()).map(|v| p.out_degree(v) > max).collect())
}

/// The triangles of each node over `s`, without the excluded nodes; −1
/// for an excluded node (module documentation).
fn triangle_counts(s: &Simple, ex: &[bool], r: &mut Run<'_, '_>) -> Result<Vec<i64>, Error> {
    let n = s.len();
    let mut t = r.mem.vec(n, 0i64)?;
    let any = ex.contains(&true);
    r.sched.fill(&mut t, |_, range, out| {
        for (o, v) in out.iter_mut().zip(range) {
            if ex[v] {
                *o = -1;
                continue;
            }
            let nv = s.of(v);
            let mut twice = 0usize;
            for w in nv {
                if ex[*w] {
                    continue;
                }
                twice += if any {
                    common(nv, s.of(*w), ex)
                } else {
                    kernels::intersect_count(nv, s.of(*w))
                };
            }
            *o = (twice / 2) as i64;
        }
    })?;
    Ok(t)
}

/// The common elements of two ascending slices that `ex` keeps.
fn common(a: &[usize], b: &[usize], ex: &[bool]) -> usize {
    let (mut i, mut j, mut c) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                c += usize::from(!ex[a[i]]);
                i += 1;
                j += 1;
            }
        }
    }
    c
}

fn run_triangle_count(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => triangle_count(p, r))
}

fn triangle_count<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let ex = excluded(p, r)?;
    let s = kernels::simple(p, r.mem)?;
    let t = triangle_counts(&s, &ex, r)?;
    let mut rows = Vec::new();
    for (v, c) in t.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), Value::Int(*c)])?;
    }
    let global = t.iter().filter(|c| **c > 0).sum::<i64>() / 3;
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("globalTriangleCount", Value::Int(global)),
            ("nodeCount", Value::Int(p.node_count() as i64)),
        ],
    })
}

fn run_triangles(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => triangles(p, r))
}

fn triangles<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let ex = excluded(p, r)?;
    let s = kernels::simple(p, r.mem)?;
    let parts = r.sched.map(|_, range| {
        let mut out = Vec::new();
        for a in range {
            if ex[a] {
                continue;
            }
            let na = s.of(a);
            for (i, b) in na.iter().enumerate() {
                if *b <= a || ex[*b] {
                    continue;
                }
                let nb = s.of(*b);
                for c in &na[i + 1..] {
                    if !ex[*c] && nb.binary_search(c).is_ok() {
                        out.push((a, *b, *c));
                    }
                }
            }
        }
        out
    })?;
    let mut rows = Vec::new();
    for (a, b, c) in parts.into_iter().flatten() {
        r.emit(
            &mut rows,
            vec![id_value(p, a), id_value(p, b), id_value(p, c)],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_local_clustering(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => local_clustering(p, r))
}

fn local_clustering<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let s = kernels::simple(p, r.mem)?;
    let t: Vec<f64> = if r.cfg.has("triangleCountProperty") {
        p.node_props[0].clone()
    } else {
        let ex = vec![false; n];
        triangle_counts(&s, &ex, r)?
            .into_iter()
            .map(|c| c as f64)
            .collect()
    };
    let mut lcc = r.mem.vec(n, 0.0f64)?;
    r.sched.fill(&mut lcc, |_, range, out| {
        for (o, v) in out.iter_mut().zip(range) {
            let tv = t[v];
            let looped = p.out(v).any(|(w, _)| w.index() == v);
            let d = (s.of(v).len() + usize::from(looped)) as f64;
            *o = if tv.is_nan() || tv == -1.0 {
                f64::NAN
            } else if tv == 0.0 {
                0.0
            } else {
                tv * 2.0 / (d * (d - 1.0))
            };
        }
    })?;
    let mut rows = Vec::new();
    for (v, x) in lcc.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), Value::Float(*x)])?;
    }
    let average = if n == 0 {
        0.0
    } else {
        lcc.iter().sum::<f64>() / n as f64
    };
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("averageClusteringCoefficient", Value::Float(average)),
            ("nodeCount", Value::Int(n as i64)),
        ],
    })
}

fn run_eccentricity(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => eccentricity(p, r))
}

/// graphalgs's breadth-first distances from `s`: infinity where `s` does
/// not reach.
fn bfs_distances<I: Ix>(p: &Projection<I>, s: usize) -> Vec<f64> {
    let mut dist = vec![f64::INFINITY; p.node_count()];
    let mut queue = VecDeque::new();
    dist[s] = 0.0;
    queue.push_back(s);
    while let Some(v) = queue.pop_front() {
        for (w, _) in p.out(v) {
            let w = w.index();
            if dist[w].is_infinite() {
                dist[w] = dist[v] + 1.0;
                queue.push_back(w);
            }
        }
    }
    dist
}

fn eccentricity<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let weighted = r.cfg.has("relationshipWeightProperty");
    r.mem.grow(4 * n * size_of::<f64>())?;
    let mut ecc = Vec::with_capacity(n);
    for v in 0..n {
        if v % 64 == 0 {
            r.mem.check_now()?;
        }
        let dist = if weighted {
            petgraph::algo::bellman_ford(p, I::new(v))
                .map_err(|_| {
                    Error::new(
                        codes::C22N11,
                        "invalid argument: the projection holds a negative cycle",
                    )
                })?
                .distances
        } else {
            bfs_distances(p, v)
        };
        ecc.push(dist.into_iter().fold(0.0f64, f64::max));
    }
    let mut rows = Vec::new();
    for (v, e) in ecc.iter().enumerate() {
        r.emit(&mut rows, vec![id_value(p, v), Value::Float(*e)])?;
    }
    let diameter = ecc.iter().copied().fold(0.0f64, f64::max);
    let radius = ecc.iter().copied().fold(f64::INFINITY, f64::min);
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("diameter", Value::Float(diameter)),
            ("radius", Value::Float(if n == 0 { 0.0 } else { radius })),
        ],
    })
}

fn run_girth(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => girth(p, r))
}

/// The length of the shortest cycle (module documentation), infinite
/// without one.
pub(crate) fn girth_of<I: Ix>(p: &Projection<I>) -> f64 {
    const NONE: usize = usize::MAX;
    let n = p.node_count();
    let undirected = p.orientation == Orientation::Undirected;
    let mut best = usize::MAX;
    let mut depth = vec![NONE; n];
    let mut tree = vec![NONE; n];
    let mut queue = VecDeque::new();
    for s in 0..n {
        depth.fill(NONE);
        tree.fill(NONE);
        depth[s] = 0;
        queue.clear();
        queue.push_back(s);
        while let Some(v) = queue.pop_front() {
            // An edge at `v` closes no cycle shorter than `2 depth(v)`
            // edges (undirected: its other end is at least one level
            // up) or `depth(v) + 1` (directed).
            let floor = if undirected {
                2 * depth[v]
            } else {
                depth[v] + 1
            };
            if floor >= best {
                break;
            }
            for (w, _) in p.out(v) {
                let w = w.index();
                if undirected {
                    // Tree edges, parallel copies included, close no cycle.
                    if w != v && (tree[v] == w || tree[w] == v) {
                        continue;
                    }
                    if depth[w] == NONE {
                        depth[w] = depth[v] + 1;
                        tree[w] = v;
                        queue.push_back(w);
                    } else {
                        best = best.min(depth[v] + depth[w] + 1);
                    }
                } else if w == s {
                    best = best.min(depth[v] + 1);
                } else if depth[w] == NONE {
                    depth[w] = depth[v] + 1;
                    queue.push_back(w);
                }
            }
        }
        if best == 1 {
            break;
        }
    }
    if best == usize::MAX {
        f64::INFINITY
    } else {
        best as f64
    }
}

fn girth<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    r.mem.grow(4 * p.node_count() * size_of::<usize>())?;
    let g = girth_of(p);
    let mut rows = Vec::new();
    r.emit(&mut rows, vec![Value::Float(g)])?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_laplacian(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => laplacian(p, r))
}

/// graphalgs's `laplacian_matrix` over the projection (module
/// documentation).
pub(crate) fn laplacian_of<I: Ix>(p: &Projection<I>, weighted: bool) -> DMatrix<f64> {
    let n = p.node_count();
    let mut kirch = DMatrix::from_element(n, n, 0.0f64);
    let mut diagonal = vec![0.0f64; n];
    for i in 0..n {
        for (j, slot) in p.out(i) {
            let j = j.index();
            if i == j {
                continue;
            }
            let cost = kernels::cost(p, weighted, slot);
            kirch[(i, j)] -= cost;
            diagonal[i] += cost;
        }
    }
    for (i, d) in diagonal.into_iter().enumerate() {
        kirch[(i, i)] = d;
    }
    kirch
}

fn laplacian<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem
        .grow(n.saturating_mul(n).saturating_mul(size_of::<f64>()) + n * size_of::<f64>())?;
    let weighted = r.cfg.has("relationshipWeightProperty");
    let m = laplacian_of(p, weighted);
    let mut rows = Vec::new();
    for i in 0..n {
        if i % 64 == 0 {
            r.mem.check_now()?;
        }
        let values: Vec<Value> = m.row(i).iter().map(|x| Value::Float(*x)).collect();
        r.emit(&mut rows, vec![id_value(p, i), Value::List(values.into())])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}
