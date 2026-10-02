//! Deterministic synthetic graphs.
//!
//! Endpoints are drawn from a Zipf-like distribution over node ranks
//! (P(rank r) ~ 1 / r^alpha), mapped through a random permutation so that
//! node id and degree are uncorrelated. alpha = 0 gives uniform endpoints.
//! Self-loops are rejected (layout D does not implement loop chains).
//! Edge types are uniform over 4 types. Edge id = generation index.

use crate::util::Rng;

pub const NTYPES: u64 = 4;

pub struct Sampler {
    pub n: u64,
    cdf: Option<Vec<f64>>,
    perm: Vec<u32>,
    pub rng: Rng,
}

impl Sampler {
    pub fn new(n: u64, alpha: f64, seed: u64) -> Sampler {
        let mut rng = Rng(seed);
        let mut perm: Vec<u32> = (0..n as u32).collect();
        for i in (1..perm.len()).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            perm.swap(i, j);
        }
        let cdf = if alpha > 0.0 {
            let mut c = Vec::with_capacity(n as usize);
            let mut s = 0.0;
            for i in 0..n {
                s += 1.0 / ((i + 1) as f64).powf(alpha);
                c.push(s);
            }
            Some(c)
        } else {
            None
        };
        Sampler { n, cdf, perm, rng }
    }
    #[inline]
    pub fn node(&mut self) -> u32 {
        let r = match &self.cdf {
            None => self.rng.below(self.n) as usize,
            Some(c) => {
                let u = self.rng.f64() * c[c.len() - 1];
                c.partition_point(|&x| x < u).min(c.len() - 1)
            }
        };
        self.perm[r]
    }
    pub fn edge(&mut self) -> (u32, u32, u8) {
        let s = self.node();
        let mut d = self.node();
        while d == s {
            d = self.node();
        }
        (s, d, self.rng.below(NTYPES) as u8)
    }
}

pub struct Graph {
    pub src: Vec<u32>,
    pub dst: Vec<u32>,
    pub ty: Vec<u8>,
}


pub fn generate(s: &mut Sampler, m: u64) -> Graph {
    let mut g = Graph { src: Vec::with_capacity(m as usize), dst: Vec::with_capacity(m as usize), ty: Vec::with_capacity(m as usize) };
    for _ in 0..m {
        let (a, b, t) = s.edge();
        g.src.push(a);
        g.dst.push(b);
        g.ty.push(t);
    }
    g
}
