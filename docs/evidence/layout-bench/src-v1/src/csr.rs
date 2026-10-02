//! Layout E: in-memory CSR projection of one snapshot (read-only baseline).
//! Structure of arrays per direction: offsets (u64, n+1), neighbour (u32),
//! edge id (u64), type (u8). Visibility is resolved when the projection is built.

pub struct Csr {
    pub n: usize,
    pub off: [Vec<u64>; 2],
    pub nbr: [Vec<u32>; 2],
    pub eid: [Vec<u64>; 2],
    pub ty: [Vec<u8>; 2],
}

impl Csr {
    pub fn builder(n: usize) -> Csr {
        Csr { n, off: [vec![0; n + 1], vec![0; n + 1]], nbr: [Vec::new(), Vec::new()], eid: [Vec::new(), Vec::new()], ty: [Vec::new(), Vec::new()] }
    }
    /// Entries of one direction must arrive grouped by ascending node.
    #[inline(always)]
    pub fn push(&mut self, node: u64, dir: u8, ty: u32, nbr: u64, eid: u64) {
        let d = dir as usize;
        self.nbr[d].push(nbr as u32);
        self.eid[d].push(eid);
        self.ty[d].push(ty as u8);
        self.off[d][node as usize + 1] = self.nbr[d].len() as u64;
    }
    pub fn finish(&mut self) {
        for d in 0..2 {
            for i in 1..=self.n {
                if self.off[d][i] < self.off[d][i - 1] {
                    self.off[d][i] = self.off[d][i - 1];
                }
            }
        }
    }
    pub fn bytes(&self) -> u64 {
        let mut b = 0u64;
        for d in 0..2 {
            b += self.off[d].len() as u64 * 8 + self.nbr[d].len() as u64 * 4 + self.eid[d].len() as u64 * 8 + self.ty[d].len() as u64;
        }
        b
    }
    pub fn edges(&self) -> u64 {
        self.nbr[0].len() as u64
    }
    #[inline(always)]
    pub fn expand(&self, node: u64, dir: u8, ty: Option<u32>, mut f: impl FnMut(u64, u64)) {
        let d = dir as usize;
        let (a, b) = (self.off[d][node as usize] as usize, self.off[d][node as usize + 1] as usize);
        let (nb, ei, tt) = (&self.nbr[d][a..b], &self.eid[d][a..b], &self.ty[d][a..b]);
        for i in 0..nb.len() {
            if let Some(t) = ty {
                if tt[i] as u32 != t {
                    continue;
                }
            }
            f(ei[i], nb[i] as u64);
        }
    }
    /// Order-independent digest of the whole projection (for cross-checks).
    pub fn digest(&self) -> u64 {
        let mut h = 0u64;
        for d in 0..2 {
            for v in 0..self.n {
                let (a, b) = (self.off[d][v] as usize, self.off[d][v + 1] as usize);
                for i in a..b {
                    h = h.wrapping_add(crate::util::mix((v as u64) << 33 ^ (d as u64) << 32 ^ self.eid[d][i] ^ (self.nbr[d][i] as u64) << 40 ^ self.ty[d][i] as u64));
                }
            }
        }
        h
    }
}
