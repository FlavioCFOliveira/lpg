//! Deterministic workload generators, derived from M-LAYOUT's
//! `layout-bench/src/generator.rs`.
//!
//! Node ranks are drawn from a Zipf distribution (P(rank r) ∝ 1 / r^α) or
//! uniformly, then mapped through a seeded random permutation so that node
//! id and degree are uncorrelated. Keys have the shape of the adjacency-tree
//! key `(node_id, direction, edge_type, neighbour_id, edge_id)` (TS-06).

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// Page size of the data file (TS-05).
pub(crate) const PAGE_SIZE: u64 = 8192;

/// Zipf exponent of M-LAYOUT's power-law graphs.
pub(crate) const ZIPF_ALPHA: f64 = 0.8;

/// Edge types, as in M-LAYOUT.
const EDGE_TYPES: u32 = 4;

/// Adjacency key: `(node_id, direction, edge_type, neighbour_id, edge_id)`;
/// direction 0 is OUT and 1 is IN.
pub(crate) type AdjKey = (u64, u8, u32, u64, u64);

/// Draws node ids from `0..n`.
pub(crate) struct Sampler {
    cdf: Option<Vec<f64>>,
    perm: Vec<u64>,
    rng: StdRng,
}

impl Sampler {
    /// Zipf-distributed nodes with exponent `alpha`.
    pub(crate) fn zipf(n: u64, alpha: f64, seed: u64) -> Sampler {
        let mut sum = 0.0;
        let cdf = (1..=n)
            .map(|r| {
                sum += 1.0 / (r as f64).powf(alpha);
                sum
            })
            .collect();
        Sampler::with_cdf(n, Some(cdf), seed)
    }

    /// Uniformly distributed nodes.
    pub(crate) fn uniform(n: u64, seed: u64) -> Sampler {
        Sampler::with_cdf(n, None, seed)
    }

    fn with_cdf(n: u64, cdf: Option<Vec<f64>>, seed: u64) -> Sampler {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut perm: Vec<u64> = (0..n).collect();
        for i in (1..perm.len()).rev() {
            perm.swap(i, rng.random_range(0..=i));
        }
        Sampler { cdf, perm, rng }
    }

    /// Next node id.
    pub(crate) fn node(&mut self) -> u64 {
        let rank = match &self.cdf {
            None => self.rng.random_range(0..self.perm.len()),
            Some(c) => {
                let u = self.rng.random::<f64>() * c[c.len() - 1];
                c.partition_point(|&x| x < u).min(c.len() - 1)
            }
        };
        self.perm[rank]
    }

    /// Adjacency key of edge `edge_id`: endpoints from this sampler, no
    /// self-loop, a uniform type and a uniform direction.
    pub(crate) fn adj_key(&mut self, edge_id: u64) -> AdjKey {
        let node = self.node();
        let mut neighbour = self.node();
        while neighbour == node {
            neighbour = self.node();
        }
        let direction = self.rng.random_range(0..2u8);
        let edge_type = self.rng.random_range(0..EDGE_TYPES);
        (node, direction, edge_type, neighbour, edge_id)
    }
}

/// A node of a generated property graph: labels and properties.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GenNode {
    pub(crate) labels: Vec<String>,
    pub(crate) props: Vec<(String, crate::types::Value)>,
}

/// An edge of a generated property graph: endpoints by node position,
/// type and properties.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GenEdge {
    pub(crate) src: usize,
    pub(crate) dst: usize,
    pub(crate) ty: String,
    pub(crate) props: Vec<(String, crate::types::Value)>,
}

/// Labels of generated graphs: plain, and holding the `:`, `;` and `\`
/// that CSV and GraphML escape (rmp #54).
pub(crate) const LABELS: [&str; 4] = ["A", "B", "C:D", "E;F\\"];
/// Edge types of generated graphs.
pub(crate) const EDGE_TYPES_NAMED: [&str; 3] = ["R", "S:T", "U;V"];

/// A property graph of `nodes` nodes and `edges` edges from `seed` (rmp
/// #54): endpoints from a uniform [`Sampler`] (self-loops allowed), each
/// node with a random subset of [`LABELS`] and each edge a type of
/// [`EDGE_TYPES_NAMED`]; property `p<k>` holds values of kind `k`
/// ([`crate::test_support::values::property_value`], every kind of IV011)
/// on about half the elements, and, with `mixed`, property `mixed` holds
/// values of a kind drawn per element. Property `uid` (nodes) and `eid`
/// (edges) number the elements, so two graphs compare element by element.
pub(crate) fn property_graph(
    seed: u64,
    nodes: usize,
    edges: usize,
    mixed: bool,
) -> (Vec<GenNode>, Vec<GenEdge>) {
    use crate::test_support::values::{PROPERTY_KINDS, mixed_property_value, property_value};
    use crate::types::Value;
    let mut rng = StdRng::seed_from_u64(seed);
    let props = |rng: &mut StdRng, id: (&str, usize)| {
        let mut p: Vec<(String, Value)> = vec![(id.0.to_owned(), Value::Int(id.1 as i64))];
        for k in 0..PROPERTY_KINDS {
            if rng.random_bool(0.5) {
                p.push((format!("p{k}"), property_value(rng, k)));
            }
        }
        if mixed && rng.random_bool(0.7) {
            p.push(("mixed".to_owned(), mixed_property_value(rng)));
        }
        p
    };
    let ns: Vec<GenNode> = (0..nodes)
        .map(|i| GenNode {
            labels: LABELS
                .iter()
                .filter(|_| rng.random_bool(0.4))
                .map(|l| (*l).to_owned())
                .collect(),
            props: props(&mut rng, ("uid", i)),
        })
        .collect();
    let mut sampler = Sampler::uniform(nodes.max(1) as u64, seed ^ 0x9e37);
    let es = (0..if nodes == 0 { 0 } else { edges })
        .map(|i| GenEdge {
            src: sampler.node() as usize,
            dst: sampler.node() as usize,
            ty: EDGE_TYPES_NAMED[rng.random_range(0..EDGE_TYPES_NAMED.len())].to_owned(),
            props: props(&mut rng, ("eid", i)),
        })
        .collect();
    (ns, es)
}

/// A graph of the Sprint 5 laboratory (rmp #61): nodes with labels, a
/// `uid` (their position) and optionally a `vec` vector; edges by node
/// position with a type and optional `weight`, `capacity` and `cost`.
/// `directed` says how the oracles read it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OracleGraph {
    pub(crate) nodes: Vec<GenNode>,
    pub(crate) edges: Vec<GenEdge>,
    pub(crate) directed: bool,
}

/// The portable generator of the laboratory graphs: `ChaCha8Rng`, whose
/// output rand documents as fixed, unlike `StdRng`'s (TS-25 Randomness), so
/// the committed fixtures can be regenerated byte for byte.
fn chacha(seed: u64) -> rand::rngs::ChaCha8Rng {
    rand::rngs::ChaCha8Rng::seed_from_u64(seed)
}

/// Nodes `0..n` with labels from `A` and `B` (each with probability
/// 1/2), `uid` and a three-element `vec` in `[0, 1)`.
fn lab_nodes(rng: &mut rand::rngs::ChaCha8Rng, n: usize) -> Vec<GenNode> {
    use crate::types::Value;
    (0..n)
        .map(|i| {
            let labels = ["A", "B"]
                .iter()
                .filter(|_| rng.random_bool(0.5))
                .map(|l| (*l).to_owned())
                .collect();
            let vec: Vec<Value> = (0..3).map(|_| Value::Float(rng.random::<f64>())).collect();
            GenNode {
                labels,
                props: vec![
                    ("uid".to_owned(), Value::Int(i as i64)),
                    ("vec".to_owned(), Value::List(vec.into())),
                ],
            }
        })
        .collect()
}

/// An edge `src → dst` of type `R` or `S` with a `weight` in `(0, 1]`.
fn lab_edge(rng: &mut rand::rngs::ChaCha8Rng, src: usize, dst: usize) -> GenEdge {
    GenEdge {
        src,
        dst,
        ty: if rng.random_bool(0.5) { "R" } else { "S" }.to_owned(),
        props: vec![(
            "weight".to_owned(),
            crate::types::Value::Float(1.0 - rng.random::<f64>()),
        )],
    }
}

/// A graph of `n` nodes and `m` edges whose endpoints are drawn from a
/// Zipf distribution with exponent [`ZIPF_ALPHA`] (`zipf`) or uniformly,
/// through a seeded permutation, as [`Sampler`] draws them; self-loops and
/// parallel edges are allowed.
pub(crate) fn power_law(seed: u64, n: usize, m: usize, zipf: bool, directed: bool) -> OracleGraph {
    let mut rng = chacha(seed);
    let nodes = lab_nodes(&mut rng, n);
    let mut perm: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        perm.swap(i, rng.random_range(0..=i));
    }
    let cdf: Vec<f64> = (1..=n)
        .scan(0.0, |sum, r| {
            *sum += 1.0 / (r as f64).powf(ZIPF_ALPHA);
            Some(*sum)
        })
        .collect();
    let pick = |rng: &mut rand::rngs::ChaCha8Rng| {
        let rank = if zipf {
            let u = rng.random::<f64>() * cdf[n - 1];
            cdf.partition_point(|&x| x < u).min(n - 1)
        } else {
            rng.random_range(0..n)
        };
        perm[rank]
    };
    let edges = (0..if n == 0 { 0 } else { m })
        .map(|_| {
            let (s, d) = (pick(&mut rng), pick(&mut rng));
            lab_edge(&mut rng, s, d)
        })
        .collect();
    OracleGraph {
        nodes,
        edges,
        directed,
    }
}

/// A directed acyclic graph: `m` edges, each from the earlier to the later
/// of two distinct nodes in a seeded topological order.
pub(crate) fn dag(seed: u64, n: usize, m: usize) -> OracleGraph {
    let mut rng = chacha(seed);
    let nodes = lab_nodes(&mut rng, n);
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        order.swap(i, rng.random_range(0..=i));
    }
    let edges = (0..if n < 2 { 0 } else { m })
        .map(|_| {
            let a = rng.random_range(0..n - 1);
            let b = rng.random_range(a + 1..n);
            lab_edge(&mut rng, order[a], order[b])
        })
        .collect();
    OracleGraph {
        nodes,
        edges,
        directed: true,
    }
}

/// A bipartite graph: nodes `0..left` labelled `L`, the next `right`
/// labelled `R`, and `m` edges from an `L` node to an `R` node.
pub(crate) fn bipartite(seed: u64, left: usize, right: usize, m: usize) -> OracleGraph {
    let mut rng = chacha(seed);
    let mut nodes = lab_nodes(&mut rng, left + right);
    for (i, node) in nodes.iter_mut().enumerate() {
        node.labels = vec![if i < left { "L" } else { "R" }.to_owned()];
    }
    let edges = (0..if left == 0 || right == 0 { 0 } else { m })
        .map(|_| {
            let a = rng.random_range(0..left);
            let b = left + rng.random_range(0..right);
            lab_edge(&mut rng, a, b)
        })
        .collect();
    OracleGraph {
        nodes,
        edges,
        directed: false,
    }
}

/// A flow network: the edges of [`dag`], with node 0 first and node
/// `n − 1` last in the order, and each edge an integer `capacity` in
/// `1..=10` and `cost` in `1..=5`.
pub(crate) fn flow_network(seed: u64, n: usize, m: usize) -> OracleGraph {
    use crate::types::Value;
    let mut g = dag(seed, n, m);
    let mut rng = chacha(seed ^ 0xf10);
    for e in &mut g.edges {
        e.props.push((
            "capacity".to_owned(),
            Value::Float(f64::from(rng.random_range(1..=10u32))),
        ));
        e.props.push((
            "cost".to_owned(),
            Value::Float(f64::from(rng.random_range(1..=5u32))),
        ));
    }
    g
}

/// A graph of [`power_law`]'s uniform kind whose `weight`s are shifted by
/// node potentials `h` in `[0, 2)`: `w + h(src) − h(dst)`, so edges may
/// cost less than 0 while every cycle keeps its positive cost (rmp #64).
/// With `negative_cycle`, nodes 0 and 1 are also joined both ways at cost
/// −1, a negative cycle.
pub(crate) fn potential(seed: u64, n: usize, m: usize, negative_cycle: bool) -> OracleGraph {
    use crate::types::Value;
    let mut g = power_law(seed, n, m, false, true);
    let mut rng = chacha(seed ^ 0x9e3779b9);
    let h: Vec<f64> = (0..n).map(|_| 2.0 * rng.random::<f64>()).collect();
    for e in &mut g.edges {
        for (k, v) in &mut e.props {
            if k == "weight"
                && let Value::Float(w) = v
            {
                *w += h[e.src] - h[e.dst];
            }
        }
    }
    if negative_cycle && n >= 2 {
        for (src, dst) in [(0, 1), (1, 0)] {
            g.edges.push(GenEdge {
                src,
                dst,
                ty: "R".to_owned(),
                props: vec![("weight".to_owned(), Value::Float(-1.0))],
            });
        }
    }
    g
}

/// A geographic graph (rmp #64, #69): `n` nodes with `lat` in `[38, 39)`
/// and `lng` in `[−9.5, −8.5)`, the first `special` labelled `S` and the
/// others `G`; `m` edges between distinct uniform nodes whose `weight` is
/// the haversine distance in nautical miles times a factor in `[1, 1.5)`,
/// so the haversine distance never exceeds a path's cost (A*'s heuristic
/// is admissible).
pub(crate) fn geo(seed: u64, n: usize, m: usize, special: usize) -> OracleGraph {
    use crate::types::Value;
    let mut rng = chacha(seed);
    let coords: Vec<(f64, f64)> = (0..n)
        .map(|_| (38.0 + rng.random::<f64>(), -9.5 + rng.random::<f64>()))
        .collect();
    let nodes = coords
        .iter()
        .enumerate()
        .map(|(i, (lat, lng))| GenNode {
            labels: vec![if i < special { "S" } else { "G" }.to_owned()],
            props: vec![
                ("uid".to_owned(), Value::Int(i as i64)),
                ("lat".to_owned(), Value::Float(*lat)),
                ("lng".to_owned(), Value::Float(*lng)),
            ],
        })
        .collect();
    let mut edges = Vec::new();
    while n >= 2 && edges.len() < m {
        let (a, b) = (rng.random_range(0..n), rng.random_range(0..n));
        if a == b {
            continue;
        }
        let (la, ga) = coords[a];
        let (lb, gb) = coords[b];
        let d = crate::algo::haversine_nm(la, ga, lb, gb) * (1.0 + 0.5 * rng.random::<f64>());
        edges.push(GenEdge {
            src: a,
            dst: b,
            ty: "R".to_owned(),
            props: vec![("weight".to_owned(), Value::Float(d))],
        });
    }
    OracleGraph {
        nodes,
        edges,
        directed: true,
    }
}

/// Two tournaments on `n` nodes each (rmp #69): nodes `0..n` labelled `T`
/// with each pair joined in a seeded direction (type `T`), and nodes
/// `n..2n` labelled `U` joined from the earlier to the later node of a
/// seeded order (type `U`), a transitive tournament.
pub(crate) fn tournaments(seed: u64, n: usize) -> OracleGraph {
    use crate::types::Value;
    let mut rng = chacha(seed);
    let nodes = (0..2 * n)
        .map(|i| GenNode {
            labels: vec![if i < n { "T" } else { "U" }.to_owned()],
            props: vec![("uid".to_owned(), Value::Int(i as i64))],
        })
        .collect();
    let edge = |src, dst, ty: &str| GenEdge {
        src,
        dst,
        ty: ty.to_owned(),
        props: Vec::new(),
    };
    let mut edges = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            edges.push(if rng.random_bool(0.5) {
                edge(i, j, "T")
            } else {
                edge(j, i, "T")
            });
        }
    }
    let mut order: Vec<usize> = (n..2 * n).collect();
    for i in (1..n).rev() {
        order.swap(i, rng.random_range(0..=i));
    }
    for i in 0..n {
        for j in i + 1..n {
            edges.push(edge(order[i], order[j], "U"));
        }
    }
    OracleGraph {
        nodes,
        edges,
        directed: true,
    }
}

/// The union of two directed graphs given as 0/1 adjacency matrices in
/// petgraph's test format (rows of space-separated digits): the first
/// labelled `P` with edges of type `P`, the second labelled `Q` with
/// edges of type `Q` (rmp #69, the isomorphism fixtures).
pub(crate) fn matrices(a: &str, b: &str) -> OracleGraph {
    use crate::types::Value;
    let parse = |text: &str| -> Vec<Vec<bool>> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split_whitespace().map(|x| x == "1").collect())
            .collect()
    };
    let (ma, mb) = (parse(a), parse(b));
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for (label, m, base) in [("P", &ma, 0), ("Q", &mb, ma.len())] {
        for i in 0..m.len() {
            nodes.push(GenNode {
                labels: vec![label.to_owned()],
                props: vec![("uid".to_owned(), Value::Int((base + i) as i64))],
            });
        }
        for (i, row) in m.iter().enumerate() {
            for (j, x) in row.iter().enumerate() {
                if *x {
                    edges.push(GenEdge {
                        src: base + i,
                        dst: base + j,
                        ty: label.to_owned(),
                        props: Vec::new(),
                    });
                }
            }
        }
    }
    OracleGraph {
        nodes,
        edges,
        directed: true,
    }
}

/// Zachary's karate club: 34 members, labelled `MrHi` or `Officer` by
/// faction, and 78 undirected edges whose `weight` is the strength of the
/// relation.
///
/// Copied from rustworkx @ `25398ab47f66`
/// (<https://github.com/Qiskit/rustworkx>, Apache-2.0, see
/// `LICENSE-APACHE`; author Matthew Treinish and the rustworkx
/// contributors, the file carries no copyright line),
/// `rustworkx-core/src/generators/karate_club.rs` (the faction list and the
/// adjacency list with strengths); modified: built as an [`OracleGraph`]
/// instead of a petgraph graph.
pub(crate) fn karate_club() -> OracleGraph {
    use crate::types::Value;
    const MR_HI: [usize; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 16, 17, 19, 21];
    let adjacency: [&[(usize, u32)]; 34] = [
        &[],
        &[(0, 4)],
        &[(0, 5), (1, 6)],
        &[(0, 3), (1, 3), (2, 3)],
        &[(0, 3)],
        &[(0, 3)],
        &[(0, 3), (4, 2), (5, 5)],
        &[(0, 2), (1, 4), (2, 4), (3, 3)],
        &[(0, 2), (2, 5)],
        &[(2, 1)],
        &[(0, 2), (4, 3), (5, 3)],
        &[(0, 3)],
        &[(0, 1), (3, 3)],
        &[(0, 3), (1, 5), (2, 3), (3, 3)],
        &[],
        &[],
        &[(5, 3), (6, 3)],
        &[(0, 2), (1, 1)],
        &[],
        &[(0, 2), (1, 2)],
        &[],
        &[(0, 2), (1, 2)],
        &[],
        &[],
        &[],
        &[(23, 5), (24, 2)],
        &[],
        &[(2, 2), (23, 4), (24, 3)],
        &[(2, 2)],
        &[(23, 3), (26, 4)],
        &[(1, 2), (8, 3)],
        &[(0, 2), (24, 2), (25, 7), (28, 2)],
        &[
            (2, 2),
            (8, 3),
            (14, 3),
            (15, 3),
            (18, 1),
            (20, 3),
            (22, 2),
            (23, 5),
            (29, 4),
            (30, 3),
            (31, 4),
        ],
        &[
            (8, 4),
            (9, 2),
            (13, 3),
            (14, 2),
            (15, 4),
            (18, 2),
            (19, 1),
            (20, 1),
            (23, 4),
            (26, 2),
            (27, 4),
            (28, 2),
            (29, 2),
            (30, 3),
            (31, 4),
            (32, 5),
            (22, 3),
        ],
    ];
    let nodes = (0..34)
        .map(|i| GenNode {
            labels: vec![
                if MR_HI.contains(&i) {
                    "MrHi"
                } else {
                    "Officer"
                }
                .to_owned(),
            ],
            props: vec![("uid".to_owned(), Value::Int(i as i64))],
        })
        .collect();
    let mut edges = Vec::new();
    for (row, neighbours) in adjacency.iter().enumerate() {
        for (neighbour, strength) in neighbours.iter() {
            edges.push(GenEdge {
                src: *neighbour,
                dst: row,
                ty: "KNOWS".to_owned(),
                props: vec![("weight".to_owned(), Value::Float(f64::from(*strength)))],
            });
        }
    }
    OracleGraph {
        nodes,
        edges,
        directed: false,
    }
}

/// The Petersen graph, the generalised Petersen graph G(5, 2): 10 nodes
/// and 15 undirected edges.
///
/// Copied from rustworkx @ `25398ab47f66` (Apache-2.0, see
/// `LICENSE-APACHE`; author Matthew Treinish and the rustworkx
/// contributors, the file carries no copyright line),
/// `rustworkx-core/src/generators/petersen_graph.rs` (`petersen_graph`:
/// star edges `i → i + k`, polygon edges `i → i + 1`, spokes); modified:
/// fixed at n = 5, k = 2, built as an [`OracleGraph`].
pub(crate) fn petersen() -> OracleGraph {
    use crate::types::Value;
    let (n, k) = (5, 2);
    let nodes = (0..2 * n)
        .map(|i| GenNode {
            labels: vec!["P".to_owned()],
            props: vec![("uid".to_owned(), Value::Int(i as i64))],
        })
        .collect();
    let edge = |src, dst| GenEdge {
        src,
        dst,
        ty: "E".to_owned(),
        props: Vec::new(),
    };
    let mut edges = Vec::new();
    edges.extend((0..n).map(|i| edge(i, (i + k) % n)));
    edges.extend((0..n).map(|i| edge(n + i, n + (i + 1) % n)));
    edges.extend((0..n).map(|i| edge(n + i, i)));
    OracleGraph {
        nodes,
        edges,
        directed: false,
    }
}

/// The edge property columns of the oracle graph file, in this order.
const EDGE_COLUMNS: [&str; 3] = ["weight", "capacity", "cost"];

/// Writes `g` as the oracle graph file: `nodes.csv` and `edges.csv` in
/// `dir`, in the Neo4j import layout that `nx_oracle.py`, `gds_oracle.sh`
/// and lpg's CSV import read (`id:ID` is the node's position, `uid:long`
/// the same number as a property; floats in Rust's shortest round-trip
/// form).
pub(crate) fn write_oracle(dir: &std::path::Path, g: &OracleGraph) {
    use crate::types::Value;
    use std::fmt::Write as _;
    let float = |v: &Value| match v {
        Value::Float(x) => format!("{x:?}"),
        Value::Int(i) => format!("{i}"),
        other => panic!("not a number: {other:?}"),
    };
    let with_vec = g
        .nodes
        .iter()
        .any(|n| n.props.iter().any(|(k, _)| k == "vec"));
    // Other float node properties (`lat`, `lng`), in first-seen order.
    let mut extra: Vec<&str> = Vec::new();
    for n in &g.nodes {
        for (k, v) in &n.props {
            if matches!(v, Value::Float(_)) && !extra.contains(&k.as_str()) {
                extra.push(k);
            }
        }
    }
    let mut nodes = String::from("id:ID,:LABEL,uid:long");
    if with_vec {
        nodes.push_str(",vec:double[]");
    }
    for k in &extra {
        let _ = write!(nodes, ",{k}:double");
    }
    nodes.push('\n');
    for (i, n) in g.nodes.iter().enumerate() {
        let _ = write!(nodes, "{i},{},{i}", n.labels.join(";"));
        if with_vec {
            let v = n.props.iter().find(|(k, _)| k == "vec").map(|(_, v)| v);
            let cell = match v {
                Some(Value::List(xs)) => xs.iter().map(float).collect::<Vec<_>>().join(";"),
                _ => String::new(),
            };
            let _ = write!(nodes, ",{cell}");
        }
        for k in &extra {
            let cell = n
                .props
                .iter()
                .find(|(p, _)| p == k)
                .map_or_else(String::new, |(_, v)| float(v));
            let _ = write!(nodes, ",{cell}");
        }
        nodes.push('\n');
    }
    let columns: Vec<&str> = EDGE_COLUMNS
        .into_iter()
        .filter(|c| g.edges.iter().any(|e| e.props.iter().any(|(k, _)| k == c)))
        .collect();
    let mut edges = String::from(":START_ID,:END_ID,:TYPE");
    for c in &columns {
        let _ = write!(edges, ",{c}:double");
    }
    edges.push('\n');
    for e in &g.edges {
        let _ = write!(edges, "{},{},{}", e.src, e.dst, e.ty);
        for c in &columns {
            let cell = e
                .props
                .iter()
                .find(|(k, _)| k == c)
                .map_or_else(String::new, |(_, v)| float(v));
            let _ = write!(edges, ",{cell}");
        }
        edges.push('\n');
    }
    super::write_text(&dir.join("nodes.csv"), &nodes);
    super::write_text(&dir.join("edges.csv"), &edges);
}

/// Loads the oracle graph file in `dir` into the default graph of `s`'s
/// database by lpg's own CSV import (rmp #59), the fast initial load; the
/// import report goes to a temporary directory outside `/tmp/lpgram`.
/// Returns the element identifier of each node by its `uid`.
pub(crate) async fn load_oracle(s: &crate::Session, dir: &std::path::Path) -> Vec<u64> {
    use crate::ops::bulk::{ImportMode, ImportRequest, InputFile};
    use crate::types::Value;
    let report = tempfile::tempdir().expect("a report directory");
    let mut req = ImportRequest::csv(
        "/public/lpg",
        dir,
        vec![InputFile::new("nodes.csv")],
        vec![InputFile::new("edges.csv")],
        "report.jsonl",
    );
    req.mode = ImportMode::FastLoad;
    req.report_location = report.path().to_path_buf();
    s.import(req).await.expect("import the oracle graph");
    let mut stream = s
        .execute("MATCH (n) RETURN n, n.uid AS uid", crate::Parameters::new())
        .await
        .expect("read the uids");
    let mut ids = Vec::new();
    while let Some(row) = stream.next().await.expect("a row") {
        let (Value::Node(id), Value::Int(uid)) = (&row[0], &row[1]) else {
            panic!("a node and its uid: {row:?}");
        };
        let uid = usize::try_from(*uid).expect("a uid");
        if ids.len() <= uid {
            ids.resize(uid + 1, 0);
        }
        ids[uid] = *id;
    }
    ids
}

/// Memory cap that lets the buffer pool hold `percent` % of `data_pages`
/// (at least one page). Only the parameter: the pool arrives in task #13.
pub(crate) fn memory_cap(data_pages: u64, percent: u64) -> u64 {
    (data_pages * percent).div_ceil(100).max(1) * PAGE_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samplers_are_deterministic_and_in_range() {
        let mut a = Sampler::zipf(1000, ZIPF_ALPHA, 1);
        let mut b = Sampler::zipf(1000, ZIPF_ALPHA, 1);
        for e in 0..1000 {
            let k = a.adj_key(e);
            assert_eq!(k, b.adj_key(e));
            assert!(k.0 < 1000 && k.3 < 1000 && k.0 != k.3);
            assert!(k.1 < 2 && k.2 < EDGE_TYPES);
        }
        let mut u = Sampler::uniform(1000, 1);
        assert!((0..1000).all(|_| u.node() < 1000));
    }

    #[test]
    fn zipf_is_skewed_and_uniform_is_not() {
        let top_share = |s: &mut Sampler| {
            let mut hits = vec![0u32; 10_000];
            for _ in 0..100_000 {
                hits[s.node() as usize] += 1;
            }
            hits.sort_unstable_by(|x, y| y.cmp(x));
            f64::from(hits[..100].iter().sum::<u32>()) / 100_000.0
        };
        // Top 1 % of nodes: for α = 0.8 and n = 10 000 the expected share is
        // H(100, 0.8) / H(10 000, 0.8) ≈ 0.28; uniform gives about 0.01.
        let zipf = top_share(&mut Sampler::zipf(10_000, ZIPF_ALPHA, 2));
        let uniform = top_share(&mut Sampler::uniform(10_000, 2));
        assert!(zipf > 0.2 && zipf < 0.4, "zipf top-1% share {zipf}");
        assert!(uniform < 0.03, "uniform top-1% share {uniform}");
    }

    /// The property graph generator is deterministic and covers every
    /// property kind and both label escapes (rmp #54).
    #[test]
    fn property_graphs_are_deterministic_and_cover_every_kind() {
        let a = property_graph(7, 60, 80, true);
        let b = property_graph(7, 60, 80, true);
        assert_eq!(a.0.len(), 60);
        assert_eq!(a.1.len(), 80);
        let same = |x: &[(String, crate::types::Value)], y: &[(String, crate::types::Value)]| {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|(p, q)| p.0 == q.0 && crate::types::lexical::bit_eq(&p.1, &q.1))
        };
        assert!(
            a.0.iter()
                .zip(&b.0)
                .all(|(x, y)| x.labels == y.labels && same(&x.props, &y.props))
        );
        assert!(
            a.1.iter()
                .zip(&b.1)
                .all(|(x, y)| (x.src, x.dst, &x.ty) == (y.src, y.dst, &y.ty))
        );
        let keys: std::collections::BTreeSet<&str> =
            a.0.iter()
                .flat_map(|n| n.props.iter().map(|(k, _)| k.as_str()))
                .collect();
        for k in 0..crate::test_support::values::PROPERTY_KINDS {
            assert!(keys.contains(format!("p{k}").as_str()), "p{k}");
        }
        assert!(keys.contains("mixed"));
        assert!(a.0.iter().any(|n| n.labels.iter().any(|l| l.contains(':'))));
    }

    /// The fixtures of the algorithm tests (rmp #61, #63): the karate club
    /// and a seeded Zipf graph, under `src/algo/testdata`.
    pub(crate) fn fixtures() -> Vec<(&'static str, OracleGraph)> {
        vec![
            ("karate", karate_club()),
            ("zipf300", power_law(5, 300, 1500, true, true)),
            // rmp #64 and #69.
            ("zipf60", power_law(21, 60, 180, true, true)),
            ("uniform60", power_law(22, 60, 180, false, true)),
            ("dag40", dag(23, 40, 100)),
            ("negw30", potential(24, 30, 90, false)),
            ("negcyc30", potential(24, 30, 90, true)),
            ("geo30", geo(25, 30, 120, 9)),
            ("cyc12", power_law(26, 12, 24, false, true)),
            ("petersen", petersen()),
            ("bip20", bipartite(27, 8, 12, 30)),
            ("tour7", tournaments(28, 7)),
        ]
    }

    /// Writes the isomorphism fixture `iso100` from petgraph 0.8.3's test
    /// graphs `tests/res/graph_100n_100e.txt` and `graph_100n_100e_iso.txt`
    /// (MIT OR Apache-2.0, used under MIT; licence text in
    /// `src/algo/projection.rs`), read from the local cargo registry.
    #[test]
    #[ignore = "writes a committed fixture from the cargo registry"]
    fn write_iso_fixture() {
        let home = std::env::var("CARGO_HOME")
            .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap()));
        let src = std::fs::read_dir(format!("{home}/registry/src"))
            .unwrap()
            .map(|d| d.unwrap().path().join("petgraph-0.8.3/tests/res"))
            .find(|p| p.exists())
            .expect("petgraph 0.8.3 sources");
        let a = std::fs::read_to_string(src.join("graph_100n_100e.txt")).unwrap();
        let b = std::fs::read_to_string(src.join("graph_100n_100e_iso.txt")).unwrap();
        let dir = fixture_dir("iso100");
        std::fs::create_dir_all(&dir).unwrap();
        write_oracle(&dir, &matrices(&a, &b));
    }

    use crate::test_support::algo::fixture_dir;

    /// Rewrites the committed fixtures from the generators; run once after
    /// a generator change, then commit the files and the oracle outputs
    /// made from them.
    #[test]
    #[ignore = "writes the committed fixtures"]
    fn write_fixtures() {
        for (name, g) in fixtures() {
            let dir = fixture_dir(name);
            std::fs::create_dir_all(&dir).unwrap();
            write_oracle(&dir, &g);
        }
    }

    /// rmp #61: two runs with one seed write byte-identical graph files,
    /// and the committed fixtures are those files; each generated DAG is
    /// acyclic, each bipartite graph two-colourable, each flow network has
    /// non-negative capacities.
    #[test]
    fn laboratory_graphs_are_reproducible_and_well_formed() {
        use crate::types::Value;
        use petgraph::graph::DiGraph;
        let files = |g: &OracleGraph| {
            let d = tempfile::tempdir().unwrap();
            write_oracle(d.path(), g);
            crate::test_support::snapshot(d.path())
        };
        for seed in [1, 2] {
            for g in [
                power_law(seed, 50, 200, true, true),
                power_law(seed, 50, 200, false, false),
                dag(seed, 40, 120),
                bipartite(seed, 10, 15, 60),
                flow_network(seed, 30, 90),
                karate_club(),
                petersen(),
            ] {
                assert_eq!(files(&g), files(&g.clone()));
            }
            assert_eq!(
                files(&power_law(seed, 50, 200, true, true)),
                files(&power_law(seed, 50, 200, true, true))
            );
            let graph = |g: &OracleGraph| {
                let mut p = DiGraph::<(), ()>::new();
                let ns: Vec<_> = (0..g.nodes.len()).map(|_| p.add_node(())).collect();
                for e in &g.edges {
                    p.add_edge(ns[e.src], ns[e.dst], ());
                }
                p
            };
            let d = dag(seed, 40, 120);
            assert!(petgraph::algo::toposort(&graph(&d), None).is_ok());
            let b = bipartite(seed, 10, 15, 60);
            let ub = graph(&b).into_edge_type::<petgraph::Undirected>();
            assert!(petgraph::algo::is_bipartite_undirected(&ub, 0.into()) || b.edges.is_empty());
            assert!(b.edges.iter().all(|e| e.src < 10 && e.dst >= 10));
            let f = flow_network(seed, 30, 90);
            assert!(f.edges.iter().all(|e| {
                e.props
                    .iter()
                    .any(|(k, v)| k == "capacity" && matches!(v, Value::Float(c) if *c >= 0.0))
            }));
        }
        assert_ne!(
            files(&power_law(1, 50, 200, true, true)),
            files(&power_law(2, 50, 200, true, true))
        );
        assert_eq!(karate_club().edges.len(), 78);
        assert_eq!(petersen().edges.len(), 15);
        for (name, g) in fixtures() {
            let committed = crate::test_support::snapshot(&fixture_dir(name));
            let committed: Vec<_> = committed
                .into_iter()
                .filter(|(n, _)| n == "nodes.csv" || n == "edges.csv")
                .collect();
            assert_eq!(committed, files(&g), "fixture {name}");
        }
    }

    #[test]
    fn memory_cap_for_cache_shares() {
        assert_eq!(memory_cap(131_072, 100), 131_072 * PAGE_SIZE);
        assert_eq!(memory_cap(131_072, 10), 13_108 * PAGE_SIZE);
        assert_eq!(memory_cap(131_072, 1), 1_311 * PAGE_SIZE);
        assert_eq!(memory_cap(10, 1), PAGE_SIZE);
    }
}
