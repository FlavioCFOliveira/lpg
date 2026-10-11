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

    #[test]
    fn memory_cap_for_cache_shares() {
        assert_eq!(memory_cap(131_072, 100), 131_072 * PAGE_SIZE);
        assert_eq!(memory_cap(131_072, 10), 13_108 * PAGE_SIZE);
        assert_eq!(memory_cap(131_072, 1), 1_311 * PAGE_SIZE);
        assert_eq!(memory_cap(10, 1), PAGE_SIZE);
    }
}
