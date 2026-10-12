//! The spanning trees family (rmp #68): ALG-147 minimum spanning tree,
//! ALG-149 k-spanning tree, ALG-151 number of spanning trees, ALG-153
//! Steiner tree, ALG-154 prize-collecting Steiner tree. 153 is parallel;
//! the others run on one worker. (The flows of the same inventory family
//! are in [`super::flow`].)
//!
//! * **147** is Prim's algorithm from `sourceNode` over the undirected
//!   projection, as GDS's `spanningtree/Prim.java` (studied): the tree
//!   spans the source's component; `objective: "maximum"` maximises.
//!   petgraph 0.8.3's `min_spanning_tree` and `min_spanning_tree_prim`
//!   need `IntoNodeReferences`, which the projection does not implement,
//!   and span every component, so they are not called; the heap is the
//!   one of [`super::kernels::dijkstra`] (`MinScored`, cost then node
//!   index). Rows: every tree node with its parent and the weight of the
//!   edge to it; the source is its own parent with weight 0, and the write
//!   mode creates no relationship for it (GDS: 33 relationships for the
//!   34 karate nodes, measured).
//! * **149** (write mode only) grows the tree of 147 as GDS's
//!   `kspanningtree/KSpanningTree.java` describes it (studied; written
//!   here): nodes are taken from a priority queue in order of the weight
//!   of their tree edge, starting at the source; once the tree holds `k`
//!   nodes, a node enters only by replacing the worst leaf (the root
//!   counts as a leaf, with its child's weight, once a removal has left it
//!   one child), and only if its own edge is better. Both queues are
//!   [`super::kernels::IndexedHeap`]s fed in edge order by target. A node
//!   whose tree parent has left the tree is dropped, so the result stays
//!   one tree.
//!   The write mode sets `writeProperty` to the root's element identifier
//!   on the tree's nodes and to each other node's own identifier, as GDS
//!   writes its internal identifiers (measured).
//! * **151** is the matrix-tree theorem of graphalgs @ `1f8a412ef4af`
//!   `src/spec/st.rs:79` (MIT, licence text in [`super::paths`]): the
//!   determinant of the Laplacian without the root's row and column
//!   counts the spanning trees (undirected) or the spanning trees oriented
//!   to the root, out-degree Laplacian (directed). graphalgs casts an
//!   `f64` determinant to an integer, which is not exact (karate: numpy
//!   gives 5090996323019105.0 for 5090996323019136, measured). lpg
//!   computes it exactly: modulo two primes near 2^61 by Gaussian
//!   elimination, joined by the Chinese remainder theorem into the residue
//!   modulo `M` ≈ 2^123. The determinant of this M-matrix is at most the
//!   product `B` of its diagonal (Hadamard–Fischer inequality); when `B <
//!   M` the residue is the count. Otherwise a residue above `i64::MAX`
//!   proves a count above it (a smaller count would equal its residue),
//!   and a smaller residue is the count unless nalgebra's `f64`
//!   determinant reaches 2^100. A count above the `INTEGER` range fails
//!   with 22003 (`algorithm-entries.md`).
//! * **153** (parallel) is GDS's rooted, directed heuristic
//!   (`steiner/ShortestPathsSteinerAlgorithm.java`,
//!   `SteinerBasedDeltaStepping.java`, `SimpleRerouter.java`, studied;
//!   written here; TS-25 item C): the tree starts as the source; while a
//!   target is unreached, the shortest paths from the tree (its nodes at
//!   distance 0) are computed, the nearest target joins with its path,
//!   and the path's nodes join the tree at distance 0. GDS computes the
//!   distances by delta-stepping; lpg by synchronous Bellman-Ford rounds:
//!   each round, by blocks, a node takes the smallest `(distance, parent,
//!   edge)` of its own and of `distance(u) + weight` over its incoming
//!   edges, reading the last round only, until no node changes; ties go
//!   to the smaller node index. `delta` sets GDS's bucket width and has no
//!   effect here. With `applyRerouting`, each tree node `s` in index order
//!   offers each out-edge `s → t`, by target index, to a tree node `t`
//!   other than the root:
//!   when the edge is lighter than `t`'s edge to its parent and `s` is not
//!   below `t`, `t` takes `s` as parent; branches that then lead to no
//!   target are cut (GDS's `SimpleRerouter`, which GDS uses on a natural
//!   projection without an inverse index).
//! * **154** is the Goemans-Williamson growth of GDS's
//!   `pricesteiner/GrowthPhase.java` with its strong pruning
//!   (`StrongPruning.java`; studied; written here; the method is that of
//!   Hegde, Indyk and Schmidt's `pcst_fast`, MIT, read at `25ab31a`):
//!   every node is an active cluster with its prize as budget; moats grow
//!   at rate 1 on active clusters; an edge whose two moats' sums reach its
//!   weight merges its clusters into an active one, a cluster whose moats
//!   reach its budget becomes inactive, until one active cluster is left;
//!   an edge of negative weight merges its ends first. The merge edges
//!   among that cluster's nodes form a tree; leaves are peeled in index
//!   order, each subtree kept under its parent when its value exceeds the
//!   edge's weight, and the subtree of the best value is the result.
//!   The next event is found by scanning every edge (`O(n m)`), where GDS
//!   and `pcst_fast` keep pairing heaps; equal times go to the cluster
//!   event, then to the smaller cluster or edge index. Modified: the best
//!   node is detached from its parent before the other roots are pruned
//!   (GDS's code prunes the root above it, studied, `StrongPruning.java`
//!   `pruneUnnecessarySubTrees`). Rows: every result node but the root.

use super::config::{Def, Key, REL_WRITE, Ty, WRITE_PROPERTY};
use super::kernels::{self, IndexedHeap, NONE, id_value, node_index};
use super::projection::{Ix, Orientation, Projection, with_projection};
use super::{Entry, Output, Run, WriteKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

const WEIGHT: Key = Key::new("relationshipWeightProperty", Ty::Str, Def::Absent);
const OBJECTIVE: Key = Key::new("objective", Ty::Str, Def::Str("minimum"));
const TREE_ROWS: &[&str] = &["nodeId", "parentId", "weight"];
const TREE_WRITE_KEYS: &[Key] = &[
    REL_WRITE[0],
    Key::new(WRITE_PROPERTY, Ty::Str, Def::Required),
];
const TREE_WRITE: WriteKind = WriteKind::RelsProp {
    src: 1,
    dst: 0,
    value: 2,
};

/// ALG-147: `gds.spanningTree.stream`, `.write`.
pub(crate) const SPANNING_TREE: Entry = Entry {
    algs: &[147],
    stream: "gds.spanningTree.stream",
    write: Some("gds.spanningTree.write"),
    keys: &[
        Key::new("sourceNode", Ty::Node, Def::Required),
        OBJECTIVE,
        WEIGHT,
    ],
    undirected: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: TREE_ROWS,
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "effectiveNodeCount",
        "relationshipsWritten",
        "totalWeight",
        "configuration",
    ],
    run: run_spanning_tree,
    write_keys: TREE_WRITE_KEYS,
    write_kind: TREE_WRITE,
    ..Entry::BASE
};

/// ALG-149: `gds.kSpanningTree.write` (no stream mode).
pub(crate) const K_SPANNING_TREE: Entry = Entry {
    algs: &[149],
    stream: "",
    write: Some("gds.kSpanningTree.write"),
    keys: &[
        Key::new("sourceNode", Ty::Node, Def::Required),
        Key::new("k", Ty::Int { min: 1 }, Def::Required),
        OBJECTIVE,
        WEIGHT,
    ],
    undirected: true,
    edge_props: &["relationshipWeightProperty"],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "postProcessingMillis",
        "effectiveNodeCount",
        "configuration",
    ],
    run: run_k_spanning_tree,
    ..Entry::BASE
};

/// ALG-151: `gds.countSpanningTrees.stream`.
pub(crate) const COUNT_SPANNING_TREES: Entry = Entry {
    algs: &[151],
    stream: "gds.countSpanningTrees.stream",
    keys: &[Key::new("rootNode", Ty::Node, Def::Absent)],
    stream_columns: &["count"],
    run: run_count_spanning_trees,
    ..Entry::BASE
};

/// ALG-153: `gds.steinerTree.stream`, `.write`.
pub(crate) const STEINER_TREE: Entry = Entry {
    algs: &[153],
    stream: "gds.steinerTree.stream",
    write: Some("gds.steinerTree.write"),
    keys: &[
        Key::new("sourceNode", Ty::Node, Def::Required),
        Key::new("targetNodes", Ty::Nodes, Def::EmptyList),
        Key::new(
            "delta",
            Ty::Float {
                min: 0.0,
                max: f64::INFINITY,
                max_open: false,
            },
            Def::Float(2.0),
        ),
        Key::new("applyRerouting", Ty::Bool, Def::Bool(false)),
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: TREE_ROWS,
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "effectiveNodeCount",
        "effectiveTargetNodesCount",
        "totalWeight",
        "relationshipsWritten",
        "configuration",
    ],
    run: run_steiner,
    write_keys: TREE_WRITE_KEYS,
    write_kind: TREE_WRITE,
    ..Entry::BASE
};

/// ALG-154: `gds.prizeSteinerTree.stream`, `.write`.
pub(crate) const PRIZE_STEINER_TREE: Entry = Entry {
    algs: &[154],
    stream: "gds.prizeSteinerTree.stream",
    write: Some("gds.prizeSteinerTree.write"),
    keys: &[Key::new("prizeProperty", Ty::Str, Def::Required), WEIGHT],
    undirected: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["prizeProperty"],
    stream_columns: TREE_ROWS,
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "effectiveNodeCount",
        "totalWeight",
        "sumOfPrizes",
        "relationshipsWritten",
        "configuration",
    ],
    run: run_prize_steiner,
    write_keys: TREE_WRITE_KEYS,
    write_kind: TREE_WRITE,
    ..Entry::BASE
};

/// A tree over the projection's nodes: each tree node's parent (itself
/// for the root, [`NONE`] outside) and the weight of the edge to it.
pub(crate) struct Tree {
    pub(crate) parent: Vec<usize>,
    pub(crate) cost: Vec<f64>,
    /// The nodes in the order they joined.
    pub(crate) order: Vec<usize>,
}

/// Whether the run maximises.
fn maximise(r: &Run<'_, '_>) -> bool {
    r.cfg
        .str("objective")
        .is_some_and(|o| o.eq_ignore_ascii_case("maximum"))
}

/// The edges of `v` in GDS's adjacency order: by target, then edge.
fn sorted_out<I: Ix>(p: &Projection<I>, v: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = p.out(v).map(|(t, slot)| (t.index(), slot)).collect();
    out.sort_unstable();
    out
}

/// Prim's algorithm from `source` (module documentation), on an
/// [`IndexedHeap`] with decrease-key over the edges by target.
pub(crate) fn prim<I: Ix>(p: &Projection<I>, source: usize, weighted: bool, max: bool) -> Tree {
    let n = p.node_count();
    let mut t = Tree {
        parent: vec![NONE; n],
        cost: vec![0.0; n],
        order: Vec::new(),
    };
    let mut via = vec![NONE; n];
    let mut done = vec![false; n];
    let mut heap = IndexedHeap::new(n);
    via[source] = source;
    heap.set(source, 0.0);
    while let Some(u) = heap.pop() {
        done[u] = true;
        t.parent[u] = via[u];
        let key = heap.cost(u);
        t.cost[u] = if u == source {
            0.0
        } else if max {
            -key
        } else {
            key
        };
        t.order.push(u);
        for (w, slot) in sorted_out(p, u) {
            if done[w] {
                continue;
            }
            let c = kernels::cost(p, weighted, slot);
            let key = if max { -c } else { c };
            if !heap.contains(w) || key < heap.cost(w) {
                heap.set(w, key);
                via[w] = u;
            }
        }
    }
    t
}

/// The rows of `tree`: each node with its parent and edge weight, in
/// index order; the root's row only in the stream mode.
fn tree_rows<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    tree: &Tree,
    with_root: bool,
) -> Result<(Vec<Vec<Value>>, f64, usize), Error> {
    let mut rows = Vec::new();
    let mut total = 0.0;
    let mut count = 0;
    for v in 0..p.node_count() {
        let pa = tree.parent[v];
        if pa == NONE {
            continue;
        }
        count += 1;
        if pa == v {
            if with_root {
                r.emit(
                    &mut rows,
                    vec![id_value(p, v), id_value(p, v), Value::Float(0.0)],
                )?;
            }
            continue;
        }
        total += tree.cost[v];
        r.emit(
            &mut rows,
            vec![id_value(p, v), id_value(p, pa), Value::Float(tree.cost[v])],
        )?;
    }
    Ok((rows, total, count))
}

fn run_spanning_tree(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => spanning_tree(p, r))
}

fn spanning_tree<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let source = node_index(p, r.cfg.get("sourceNode"), "sourceNode")?;
    let n = p.node_count();
    r.mem
        .grow(n * (4 * size_of::<usize>() + 2 * size_of::<f64>()) + p.edge_ids.len() * 32)?;
    let tree = prim(
        p,
        source,
        r.cfg.has("relationshipWeightProperty"),
        maximise(r),
    );
    let with_root = r.mode == super::AlgoMode::Stream;
    let (rows, total, count) = tree_rows(p, r, &tree, with_root)?;
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("effectiveNodeCount", Value::Int(count as i64)),
            ("totalWeight", Value::Float(total)),
        ],
    })
}

fn run_k_spanning_tree(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => k_spanning_tree(p, r))
}

/// The nodes of ALG-149's tree and its root (module documentation).
pub(crate) fn k_tree(mst: &Tree, source: usize, k: usize, max: bool) -> (Vec<bool>, usize) {
    let n = mst.parent.len();
    let mut included = vec![false; n];
    let mut root = source;
    if mst.order.len() <= k {
        for v in &mst.order {
            included[*v] = true;
        }
        return (included, root);
    }
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for v in &mst.order {
        if mst.parent[*v] != *v {
            children[mst.parent[*v]].push(*v);
        }
    }
    for c in &mut children {
        c.sort_unstable();
    }
    // Better edges first: the heap pops the smallest key.
    let key = |c: f64| if max { -c } else { c };
    let mut kids = vec![0usize; n];
    let mut heap = IndexedHeap::new(n);
    heap.set(source, 0.0);
    let mut count = 0;
    // The root is a leaf only once a removal has left it one child.
    let mut root_leaf = false;
    while let Some(v) = heap.pop() {
        let c = heap.cost(v);
        let pa = mst.parent[v];
        if v != source && !included[pa] {
            continue;
        }
        if count >= k {
            // The worst leaf: the largest key, then the smallest index.
            let mut worst: Option<(f64, usize)> = None;
            for u in 0..n {
                if !included[u] {
                    continue;
                }
                let leaf_cost = if u == root {
                    if !root_leaf || kids[u] != 1 {
                        continue;
                    }
                    let child = children[u]
                        .iter()
                        .find(|x| included[**x])
                        .copied()
                        .expect("one child");
                    key(mst.cost[child])
                } else if kids[u] == 0 {
                    key(mst.cost[u])
                } else {
                    continue;
                };
                if worst.is_none_or(|(w, _)| leaf_cost > w) {
                    worst = Some((leaf_cost, u));
                }
            }
            let Some((leaf_cost, leaf)) = worst else {
                continue;
            };
            if pa == leaf || c >= leaf_cost {
                continue;
            }
            included[leaf] = false;
            count -= 1;
            if leaf == root {
                root = children[leaf]
                    .iter()
                    .find(|x| included[**x])
                    .copied()
                    .expect("one child");
                root_leaf = kids[root] == 1;
            } else {
                let pl = mst.parent[leaf];
                kids[pl] -= 1;
                if pl == root && kids[pl] == 1 {
                    root_leaf = true;
                }
            }
        }
        included[v] = true;
        count += 1;
        if v != root {
            kids[pa] += 1;
            if pa == root {
                root_leaf = false;
            }
        }
        for w in &children[v] {
            if !heap.contains(*w) {
                heap.set(*w, key(mst.cost[*w]));
            }
        }
    }
    (included, root)
}

fn k_spanning_tree<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let source = node_index(p, r.cfg.get("sourceNode"), "sourceNode")?;
    let k = usize::try_from(r.cfg.int("k")).unwrap_or(usize::MAX);
    let n = p.node_count();
    r.mem
        .grow(n * (9 * size_of::<usize>() + 3 * size_of::<f64>()) + p.edge_ids.len() * 32)?;
    let max = maximise(r);
    let mst = prim(p, source, r.cfg.has("relationshipWeightProperty"), max);
    let (included, root) = k_tree(&mst, source, k, max);
    let mut rows = Vec::new();
    for (v, inside) in included.iter().enumerate() {
        let value = if *inside { root } else { v };
        r.emit(&mut rows, vec![id_value(p, v), id_value(p, value)])?;
    }
    let count = included.iter().filter(|x| **x).count();
    Ok(Output::Rows {
        rows,
        extra: vec![("effectiveNodeCount", Value::Int(count as i64))],
    })
}

/// The two primes of ALG-151's residues: 2^61 − 1 and 2^62 − 57.
const PRIMES: [u64; 2] = [2_305_843_009_213_693_951, 4_611_686_018_427_387_847];

fn pow_mod(mut b: u64, mut e: u64, m: u64) -> u64 {
    let mut acc = 1u64;
    b %= m;
    while e > 0 {
        if e & 1 == 1 {
            acc = ((u128::from(acc) * u128::from(b)) % u128::from(m)) as u64;
        }
        b = ((u128::from(b) * u128::from(b)) % u128::from(m)) as u64;
        e >>= 1;
    }
    acc
}

/// The determinant of the integer matrix `a` (`k × k`, row major)
/// modulo the prime `m`, by Gaussian elimination.
fn det_mod(a: &[i64], k: usize, m: u64) -> u64 {
    let mm = u128::from(m);
    let mut x: Vec<u64> = a.iter().map(|v| v.rem_euclid(m as i64) as u64).collect();
    let mut det = 1u64;
    for c in 0..k {
        let Some(piv) = (c..k).find(|r| x[r * k + c] != 0) else {
            return 0;
        };
        if piv != c {
            for j in 0..k {
                x.swap(piv * k + j, c * k + j);
            }
            det = (m - det) % m;
        }
        let d = x[c * k + c];
        det = ((u128::from(det) * u128::from(d)) % mm) as u64;
        let inv = pow_mod(d, m - 2, m);
        for row in c + 1..k {
            let f = x[row * k + c];
            if f == 0 {
                continue;
            }
            let f = ((u128::from(f) * u128::from(inv)) % mm) as u64;
            for j in c..k {
                let sub = ((u128::from(f) * u128::from(x[c * k + j])) % mm) as u64;
                x[row * k + j] = (x[row * k + j] + m - sub) % m;
            }
        }
    }
    det
}

/// The count of ALG-151 from the reduced Laplacian `a` (module
/// documentation): `Ok(None)` when it exceeds `i64::MAX`.
pub(crate) fn exact_det(a: &[i64], k: usize) -> Option<i64> {
    let r1 = det_mod(a, k, PRIMES[0]);
    let r2 = det_mod(a, k, PRIMES[1]);
    let (p1, p2) = (u128::from(PRIMES[0]), u128::from(PRIMES[1]));
    // x = r1 + p1 · ((r2 − r1) · p1⁻¹ mod p2)
    let inv = u128::from(pow_mod(PRIMES[0] % PRIMES[1], PRIMES[1] - 2, PRIMES[1]));
    let diff = (u128::from(r2) + p2 - u128::from(r1) % p2) % p2;
    let t = (diff * inv) % p2;
    let residue = u128::from(r1) + p1 * t;
    let modulus = p1 * p2;
    let mut bound = 1u128;
    for i in 0..k {
        let d = u128::try_from(a[i * k + i].max(0)).unwrap_or(0);
        bound = bound.saturating_mul(d);
    }
    let exact = bound < modulus || {
        let m = nalgebra::DMatrix::from_fn(k, k, |i, j| a[i * k + j] as f64);
        m.determinant().abs() < 2f64.powi(100)
    };
    if residue > i64::MAX as u128 || !exact {
        return None;
    }
    Some(residue as i64)
}

fn run_count_spanning_trees(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => count_spanning_trees(p, r))
}

fn count_spanning_trees<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let root = if r.cfg.has("rootNode") {
        node_index(p, r.cfg.get("rootNode"), "rootNode")?
    } else if p.orientation == Orientation::Undirected {
        0
    } else {
        return Err(Error::new(
            codes::C22N11,
            "invalid argument: the configuration needs the key rootNode unless orientation is UNDIRECTED",
        ));
    };
    let count = if n == 0 {
        0
    } else {
        let k = n - 1;
        r.mem
            .grow(k.saturating_mul(k).saturating_mul(3 * size_of::<i64>()) + n * 8)?;
        let pos = |v: usize| if v < root { v } else { v - 1 };
        let mut a = vec![0i64; k * k];
        for v in 0..n {
            if v == root {
                continue;
            }
            for (w, _) in p.out(v) {
                let w = w.index();
                if w == v {
                    continue;
                }
                a[pos(v) * k + pos(v)] += 1;
                if w != root {
                    a[pos(v) * k + pos(w)] -= 1;
                }
            }
        }
        r.mem.check_now()?;
        exact_det(&a, k).ok_or_else(|| {
            Error::new(
                codes::C22003,
                "numeric value out of range: the number of spanning trees exceeds the INTEGER range",
            )
        })?
    };
    let mut rows = Vec::new();
    r.emit(&mut rows, vec![Value::Int(count)])?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_steiner(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => steiner(p, r))
}

/// One node's state in [`steiner_tree`]'s rounds: distance from the
/// tree, parent and edge.
type Dist = (f64, usize, usize);

fn better(a: Dist, b: Dist) -> bool {
    a.0.total_cmp(&b.0)
        .then(a.1.cmp(&b.1))
        .then(a.2.cmp(&b.2))
        .is_lt()
}

/// ALG-153's tree (module documentation) and its number of rounds.
pub(crate) fn steiner_tree<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    source: usize,
    targets: &[usize],
    weighted: bool,
    reroute: bool,
) -> Result<(Tree, usize), Error> {
    let n = p.node_count();
    let mut tree = Tree {
        parent: vec![NONE; n],
        cost: vec![0.0; n],
        order: vec![source],
    };
    tree.parent[source] = source;
    let mut state: Vec<Dist> = r.mem.vec(n, (f64::INFINITY, NONE, NONE))?;
    let mut next: Vec<Dist> = r.mem.vec(n, (f64::INFINITY, NONE, NONE))?;
    state[source] = (0.0, NONE, NONE);
    let mut is_target = vec![false; n];
    for t in targets {
        is_target[*t] = true;
    }
    let mut unreached: Vec<usize> = targets.to_vec();
    unreached.sort_unstable();
    unreached.dedup();
    unreached.retain(|t| *t != source);
    let mut rounds = 0;
    while !unreached.is_empty() {
        // Rounds until no distance changes.
        for _ in 0..=n {
            rounds += 1;
            let (prev, in_tree) = (&state, &tree.parent);
            let changed = r.sched.fill(&mut next, |_, range, out| {
                let mut changed = false;
                for (o, v) in out.iter_mut().zip(range) {
                    let mut best = prev[v];
                    if in_tree[v] == NONE {
                        for (u, slot) in p.inc(v) {
                            let u = u.index();
                            if !prev[u].0.is_finite() {
                                continue;
                            }
                            let d = prev[u].0 + kernels::cost(p, weighted, slot);
                            let cand = (d, u, slot);
                            if better(cand, best) {
                                best = cand;
                            }
                        }
                    }
                    changed |= best != prev[v];
                    *o = best;
                }
                changed
            })?;
            std::mem::swap(&mut state, &mut next);
            if !changed.contains(&true) {
                break;
            }
            r.mem.check_now()?;
        }
        let Some(&t) = unreached
            .iter()
            .filter(|t| state[**t].0.is_finite())
            .min_by(|a, b| state[**a].0.total_cmp(&state[**b].0).then(a.cmp(b)))
        else {
            break;
        };
        unreached.retain(|x| *x != t);
        // The path from the tree to `t` joins the tree.
        let mut v = t;
        let mut path = Vec::new();
        while tree.parent[v] == NONE && path.len() <= n {
            let (_, u, slot) = state[v];
            path.push(v);
            tree.parent[v] = u;
            tree.cost[v] = kernels::cost(p, weighted, slot);
            v = u;
        }
        for v in path.into_iter().rev() {
            tree.order.push(v);
            state[v] = (0.0, NONE, NONE);
        }
    }
    if reroute {
        reroute_tree(p, &mut tree, source, &is_target, weighted);
    }
    Ok((tree, rounds))
}

/// GDS's simple rerouting, then the cut of branches without a target
/// (module documentation).
fn reroute_tree<I: Ix>(
    p: &Projection<I>,
    tree: &mut Tree,
    source: usize,
    is_target: &[bool],
    weighted: bool,
) {
    let n = p.node_count();
    let below = |tree: &Tree, s: usize, t: usize| {
        // Whether `s` lies in `t`'s subtree.
        let mut v = s;
        for _ in 0..=n {
            if v == t {
                return true;
            }
            if tree.parent[v] == v || tree.parent[v] == NONE {
                return false;
            }
            v = tree.parent[v];
        }
        true
    };
    let mut changed = false;
    for s in 0..n {
        if tree.parent[s] == NONE {
            continue;
        }
        // GDS's adjacency order: by target, then edge.
        let mut out: Vec<(usize, usize)> = p.out(s).map(|(t, slot)| (t.index(), slot)).collect();
        out.sort_unstable();
        for (t, slot) in out {
            let pa = tree.parent[t];
            if pa == NONE || pa == t {
                continue;
            }
            let w = kernels::cost(p, weighted, slot);
            if w < tree.cost[t] && !below(tree, s, t) {
                tree.parent[t] = s;
                tree.cost[t] = w;
                changed = true;
            }
        }
    }
    if changed {
        let mut keep = vec![false; n];
        keep[source] = true;
        for (t, target) in is_target.iter().enumerate() {
            if !target || tree.parent[t] == NONE {
                continue;
            }
            let mut v = t;
            while !keep[v] {
                keep[v] = true;
                v = tree.parent[v];
            }
        }
        for (v, k) in keep.iter().enumerate() {
            if !k {
                tree.parent[v] = NONE;
            }
        }
        tree.order.retain(|v| keep[*v]);
    }
}

fn steiner<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let source = node_index(p, r.cfg.get("sourceNode"), "sourceNode")?;
    let mut targets = Vec::new();
    for id in r.cfg.nodes("targetNodes") {
        targets.push(node_index(p, &Value::Node(id), "targetNodes")?);
    }
    let weighted = r.cfg.has("relationshipWeightProperty");
    let reroute = r.cfg.bool("applyRerouting");
    let n = p.node_count();
    r.mem
        .grow(n * (4 * size_of::<usize>() + size_of::<f64>()))?;
    let (tree, _) = steiner_tree(p, r, source, &targets, weighted, reroute)?;
    let reached = targets
        .iter()
        .filter(|t| **t != source && tree.parent[**t] != NONE)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let with_root = r.mode == super::AlgoMode::Stream;
    let (rows, total, count) = tree_rows(p, r, &tree, with_root)?;
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("effectiveNodeCount", Value::Int(count as i64)),
            ("effectiveTargetNodesCount", Value::Int(reached as i64)),
            ("totalWeight", Value::Float(total)),
        ],
    })
}

fn run_prize_steiner(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => prize_steiner(p, r))
}

/// A cluster of [`gw_growth`].
#[derive(Debug, Clone, Copy)]
struct Cluster {
    parent: usize,
    active: bool,
    /// When the cluster last became active or inactive.
    since: f64,
    /// The moat it had when it became inactive.
    moat: f64,
    /// Its budget: the prizes inside less the moats inside.
    left: f64,
    children: (usize, usize),
}

impl Cluster {
    fn moat_at(&self, t: f64) -> f64 {
        if self.active {
            t - self.since
        } else {
            self.moat
        }
    }
}

/// ALG-154's growth phase (module documentation): the nodes of the last
/// active cluster and the merge edges, as `(u, v, weight)`.
pub(crate) fn gw_growth(prizes: &[f64], edges: &[(usize, usize, f64)]) -> (Vec<bool>, Vec<usize>) {
    let n = prizes.len();
    let mut cl: Vec<Cluster> = prizes
        .iter()
        .map(|p| Cluster {
            parent: NONE,
            active: true,
            since: 0.0,
            moat: 0.0,
            left: if p.is_nan() { 0.0 } else { *p },
            children: (NONE, NONE),
        })
        .collect();
    let mut active = n;
    let mut tree = Vec::new();
    let top = |cl: &[Cluster], mut v: usize| {
        while cl[v].parent != NONE {
            v = cl[v].parent;
        }
        v
    };
    let sum = |cl: &[Cluster], mut v: usize, t: f64| {
        let mut s = cl[v].moat_at(t);
        while cl[v].parent != NONE {
            v = cl[v].parent;
            s += cl[v].moat_at(t);
        }
        s
    };
    let deactivate = |cl: &mut Vec<Cluster>, c: usize, t: f64| {
        if cl[c].active {
            cl[c].moat = cl[c].moat_at(t);
            cl[c].active = false;
            cl[c].since = t;
        }
    };
    let merge = |cl: &mut Vec<Cluster>, a: usize, b: usize, t: f64| {
        let left = cl[a].left + cl[b].left - cl[a].moat_at(t) - cl[b].moat_at(t);
        let new = cl.len();
        cl.push(Cluster {
            parent: NONE,
            active: true,
            since: t,
            moat: 0.0,
            left,
            children: (a, b),
        });
        for c in [a, b] {
            if cl[c].active {
                cl[c].moat = cl[c].moat_at(t);
                cl[c].active = false;
                cl[c].since = t;
            }
            cl[c].parent = new;
        }
        new
    };
    // Negative edges merge first (GDS's `handleNegativeEdge`).
    for (i, (u, v, w)) in edges.iter().enumerate() {
        if *w < 0.0 {
            let (a, b) = (top(&cl, *u), top(&cl, *v));
            if a != b {
                let before = usize::from(cl[a].active) + usize::from(cl[b].active);
                let new = merge(&mut cl, a, b, 0.0);
                cl[new].left -= w;
                active = active + 1 - before;
                tree.push(i);
            }
        }
    }
    let mut now = 0.0f64;
    while active > 1 {
        let mut cluster_event: Option<(f64, usize)> = None;
        for (c, x) in cl.iter().enumerate() {
            if x.active {
                let t = x.since + x.left;
                if cluster_event.is_none_or(|(b, _)| t < b) {
                    cluster_event = Some((t, c));
                }
            }
        }
        let mut edge_event: Option<(f64, usize, usize, usize)> = None;
        for (i, (u, v, w)) in edges.iter().enumerate() {
            if *w < 0.0 {
                continue;
            }
            let (a, b) = (top(&cl, *u), top(&cl, *v));
            if a == b || !(cl[a].active || cl[b].active) {
                continue;
            }
            let rest = w - sum(&cl, *u, now) - sum(&cl, *v, now);
            let dt = if cl[a].active && cl[b].active {
                rest / 2.0
            } else {
                rest
            };
            let t = now + dt.max(0.0);
            if edge_event.is_none_or(|(b, ..)| t < b) {
                edge_event = Some((t, i, a, b));
            }
        }
        match (cluster_event, edge_event) {
            (Some((tc, c)), e) if e.is_none_or(|(te, ..)| tc <= te) => {
                now = now.max(tc);
                deactivate(&mut cl, c, now);
                active -= 1;
            }
            (_, Some((te, i, a, b))) => {
                now = now.max(te);
                let before = usize::from(cl[a].active) + usize::from(cl[b].active);
                merge(&mut cl, a, b, now);
                active = active + 1 - before;
                tree.push(i);
            }
            _ => break,
        }
    }
    let mut members = vec![false; n];
    if let Some(last) = (0..cl.len()).find(|c| cl[*c].active && cl[*c].parent == NONE) {
        let mut stack = vec![last];
        while let Some(c) = stack.pop() {
            if c < n {
                members[c] = true;
            } else {
                stack.push(cl[c].children.0);
                stack.push(cl[c].children.1);
            }
        }
    }
    (members, tree)
}

/// ALG-154's strong pruning (module documentation) over the merge edges
/// among `members`: each node's parent (itself for the root, [`NONE`]
/// when pruned) and edge weight.
pub(crate) fn strong_pruning(
    prizes: &[f64],
    edges: &[(usize, usize, f64)],
    members: &[bool],
    tree_edges: &[usize],
) -> Tree {
    let n = prizes.len();
    let prize = |v: usize| if prizes[v].is_nan() { 0.0 } else { prizes[v] };
    let mut out = Tree {
        parent: vec![NONE; n],
        cost: vec![0.0; n],
        order: Vec::new(),
    };
    let count = members.iter().filter(|x| **x).count();
    if count == 0 {
        return out;
    }
    if count == 1 {
        let v = members.iter().position(|x| *x).expect("one member");
        out.parent[v] = v;
        out.order.push(v);
        return out;
    }
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut degree = vec![0usize; n];
    for i in tree_edges {
        let (u, v, w) = edges[*i];
        if members[u] && members[v] {
            adj[u].push((v, w));
            adj[v].push((u, w));
            degree[u] += 1;
            degree[v] += 1;
        }
    }
    for a in &mut adj {
        a.sort_by_key(|x| x.0);
    }
    let mut dp = vec![0.0f64; n];
    let mut queue: Vec<usize> = (0..n).filter(|v| degree[*v] == 1).collect();
    let mut at = 0;
    let mut best: Option<usize> = None;
    let mut parent = vec![NONE; n];
    while at < queue.len() {
        let leaf = queue[at];
        at += 1;
        dp[leaf] += prize(leaf);
        degree[leaf] = 0;
        parent[leaf] = leaf;
        if best.is_none_or(|b| dp[b] < dp[leaf]) {
            best = Some(leaf);
        }
        let Some(&(pa, w)) = adj[leaf].iter().find(|(x, _)| degree[*x] > 0) else {
            continue;
        };
        if w < dp[leaf] {
            dp[pa] += dp[leaf] - w;
            parent[leaf] = pa;
            out.cost[leaf] = w;
        }
        degree[pa] -= 1;
        if degree[pa] == 1 {
            queue.push(pa);
        }
    }
    let best = best.expect("a leaf");
    parent[best] = best;
    // Keep `best`'s subtree: the nodes whose parent chain reaches it.
    let mut keep = vec![None; n];
    keep[best] = Some(true);
    for v in 0..n {
        if parent[v] == NONE || keep[v].is_some() {
            continue;
        }
        let mut chain = vec![v];
        let mut x = v;
        let verdict = loop {
            if let Some(k) = keep[x] {
                break k;
            }
            if parent[x] == x {
                break false;
            }
            x = parent[x];
            chain.push(x);
        };
        for c in chain {
            keep[c].get_or_insert(verdict);
        }
    }
    for v in 0..n {
        if keep[v] == Some(true) {
            out.parent[v] = parent[v];
            out.order.push(v);
        } else {
            out.cost[v] = 0.0;
        }
    }
    out
}

fn prize_steiner<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let weighted = r.cfg.has("relationshipWeightProperty");
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    r.mem
        .grow(n * (12 * size_of::<usize>() + 4 * size_of::<f64>()) + p.edge_ids.len() * 64)?;
    for u in 0..n {
        for slot in p.fwd.row[u]..p.fwd.row[u + 1] {
            let v = p.fwd.col[slot].index();
            if u != v {
                edges.push((u.max(v), u.min(v), kernels::cost(p, weighted, slot)));
            }
        }
    }
    // GDS's edge order: by the larger end, then adjacency order.
    edges.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let prizes = &p.node_props[0];
    let (members, tree_edges) = gw_growth(prizes, &edges);
    r.mem.check_now()?;
    let tree = strong_pruning(prizes, &edges, &members, &tree_edges);
    let (rows, total, count) = tree_rows(p, r, &tree, false)?;
    let sum: f64 = (0..n)
        .filter(|v| tree.parent[*v] != NONE)
        .map(|v| if prizes[v].is_nan() { 0.0 } else { prizes[v] })
        .sum();
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("effectiveNodeCount", Value::Int(count as i64)),
            ("totalWeight", Value::Float(total)),
            ("sumOfPrizes", Value::Float(sum)),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree from `(node, parent, weight)` triples; the root is its own
    /// parent.
    fn tree(n: usize, edges: &[(usize, usize, f64)]) -> Tree {
        let mut t = Tree {
            parent: vec![NONE; n],
            cost: vec![0.0; n],
            order: Vec::new(),
        };
        for (v, p, w) in edges {
            t.parent[*v] = *p;
            t.cost[*v] = *w;
            t.order.push(*v);
        }
        t
    }

    /// ALG-149's growth replaces the worst leaf by a cheaper node, and
    /// the root once a removal has left it one child.
    #[test]
    fn k_tree_trims_leaves_and_the_root() {
        // 0 -5- 1, 0 -5- 2 -1- 4 -1- 5.
        let mst = tree(
            6,
            &[
                (0, 0, 0.0),
                (1, 0, 5.0),
                (2, 0, 5.0),
                (4, 2, 1.0),
                (5, 4, 1.0),
            ],
        );
        let (inc, root) = k_tree(&mst, 0, 3, false);
        let set: Vec<usize> = (0..6).filter(|v| inc[*v]).collect();
        assert_eq!((set, root), (vec![2, 4, 5], 2));
        // Without node 5 the root stays: leaf 1 gives way to node 4.
        let mst = tree(5, &[(0, 0, 0.0), (1, 0, 5.0), (2, 0, 5.0), (4, 2, 1.0)]);
        let (inc, root) = k_tree(&mst, 0, 3, false);
        assert_eq!(
            ((0..5).filter(|v| inc[*v]).collect::<Vec<_>>(), root),
            (vec![0, 2, 4], 0)
        );
        // A component no larger than k is the whole tree.
        let (inc, _) = k_tree(&mst, 0, 9, false);
        assert_eq!(inc.iter().filter(|x| **x).count(), 4);
    }

    /// ALG-151's exact determinant: a row swap, a bound above the
    /// residues' modulus, and counts above the INTEGER range.
    #[test]
    fn exact_determinants() {
        assert_eq!(det_mod(&[0, 1, 1, 0], 2, PRIMES[0]), PRIMES[0] - 1);
        assert_eq!(det_mod(&[0, 0, 0, 1], 2, PRIMES[0]), 0);
        // det = 2a − 1 = i64::MAX for a = 2^62; the diagonal's product
        // 2^124 exceeds the modulus, so nalgebra's estimate decides.
        let a = 1i64 << 62;
        assert_eq!(exact_det(&[a, -(a - 1), -(a - 1), a], 2), Some(i64::MAX));
        // K_40: 40^38 spanning trees.
        let n = 39;
        let mut m = vec![-1i64; n * n];
        for i in 0..n {
            m[i * n + i] = 39;
        }
        assert_eq!(exact_det(&m, n), None);
        // K_17: Cayley's 17^15, above 2^53.
        let n = 16;
        let mut m = vec![-1i64; n * n];
        for i in 0..n {
            m[i * n + i] = 16;
        }
        assert_eq!(exact_det(&m, n), Some(17i64.pow(15)));
    }

    /// ALG-154's growth merges a negative edge's ends first.
    #[test]
    fn negative_edges_merge_first() {
        let (members, tree) = gw_growth(&[1.0, 1.0, 0.0], &[(1, 0, -2.0), (2, 1, 5.0)]);
        assert_eq!(tree[0], 0, "the negative edge first");
        assert!(members[0] && members[1] && !members[2]);
        let t = strong_pruning(
            &[1.0, 1.0, 0.0],
            &[(1, 0, -2.0), (2, 1, 5.0)],
            &members,
            &tree,
        );
        assert_eq!(t.order.len(), 2);
    }
}
