//! Layouts F and G: the CSR projection of one snapshot (layout E) stored in
//! pool pages, so it can be measured under a constrained cache like A-D.
//! Read-only: no stamps, no in-place insert or delete.
//!
//! Offsets: u64 per (direction, node), 1024 per page (same width as E).
//! F (fixed): 13-byte entries (neighbour u32, type u8, edge id u64), 630 per
//!   page so that no entry straddles a page; offsets are entry indexes.
//! G (compressed): per (node, direction) segment, entries in (type, neighbour,
//!   edge id) order, each encoded as varint(((nbr_val << 1) | type_changed) + 1),
//!   [type u8 if type_changed], varint(edge id). nbr_val is the delta to the
//!   previous neighbour inside a type run, else the absolute neighbour. The first
//!   byte of an entry is never 0, so an entry that would straddle a page is moved
//!   to the next page and the gap is zero-filled; offsets are byte positions.

use crate::csr::Csr;
use crate::pool::{PAGE, Pool};
use crate::util::{get_var, put_var, rd32, rd64};

const OPP: usize = PAGE / 8;
const FE: usize = 13;
const FPP: usize = PAGE / FE;
const MAXE: usize = 1 + 6 + 10; // longest G entry

pub struct PCsr {
    pub comp: bool,
    off_base: [u32; 2],
    ent_base: [u32; 2],
    pub off_pages: u64,
    pub ent_pages: u64,
    pub ent_bytes: u64,
}

fn write_pages(pool: &mut Pool, bytes: &[u8]) -> (u32, u64) {
    let np = bytes.len().div_ceil(PAGE).max(1);
    let mut first = 0;
    for k in 0..np {
        let pid = pool.alloc();
        if k == 0 {
            first = pid;
        }
        let a = (k * PAGE).min(bytes.len());
        let b = ((k + 1) * PAGE).min(bytes.len());
        pool.write(pid, |p| p[..b - a].copy_from_slice(&bytes[a..b]));
    }
    (first, np as u64)
}

impl PCsr {
    pub fn build(pool: &mut Pool, c: &Csr, comp: bool) -> PCsr {
        let mut r = PCsr { comp, off_base: [0; 2], ent_base: [0; 2], off_pages: 0, ent_pages: 0, ent_bytes: 0 };
        for d in 0..2 {
            let mut offs: Vec<u64> = Vec::with_capacity(c.n + 1);
            let mut st: Vec<u8> = Vec::new();
            if comp {
                let mut idx: Vec<usize> = Vec::new();
                offs.push(0);
                for v in 0..c.n {
                    let (a, b) = (c.off[d][v] as usize, c.off[d][v + 1] as usize);
                    idx.clear();
                    idx.extend(a..b);
                    idx.sort_unstable_by_key(|&i| (c.ty[d][i], c.nbr[d][i], c.eid[d][i]));
                    let (mut pty, mut pnb) = (u32::MAX, 0u64);
                    for &i in &idx {
                        let (ty, nb, e) = (c.ty[d][i] as u32, c.nbr[d][i] as u64, c.eid[d][i]);
                        let tc = ty != pty;
                        let val = if tc { nb } else { nb - pnb };
                        let mut ent = Vec::with_capacity(MAXE);
                        put_var(&mut ent, ((val << 1) | tc as u64) + 1);
                        if tc {
                            ent.push(ty as u8);
                        }
                        put_var(&mut ent, e);
                        let room = PAGE - st.len() % PAGE;
                        if ent.len() > room {
                            st.resize(st.len() + room, 0);
                        }
                        st.extend_from_slice(&ent);
                        pty = ty;
                        pnb = nb;
                    }
                    offs.push(st.len() as u64);
                }
            } else {
                for v in 0..=c.n {
                    offs.push(c.off[d][v]);
                }
                let m = c.nbr[d].len();
                st.resize(m.div_ceil(FPP) * PAGE, 0);
                for i in 0..m {
                    let o = (i / FPP) * PAGE + (i % FPP) * FE;
                    st[o..o + 4].copy_from_slice(&c.nbr[d][i].to_le_bytes());
                    st[o + 4] = c.ty[d][i];
                    st[o + 5..o + 13].copy_from_slice(&c.eid[d][i].to_le_bytes());
                }
            }
            let ob: Vec<u8> = offs.iter().flat_map(|x| x.to_le_bytes()).collect();
            let (p0, n0) = write_pages(pool, &ob);
            let (p1, n1) = write_pages(pool, &st);
            r.off_base[d] = p0;
            r.ent_base[d] = p1;
            r.off_pages += n0;
            r.ent_pages += n1;
            r.ent_bytes += st.len() as u64;
        }
        r
    }

    #[inline(always)]
    fn offs(&self, pool: &mut Pool, d: usize, node: u64) -> (usize, usize) {
        let i = node as usize;
        let pid = self.off_base[d] + (i / OPP) as u32;
        let o = (i % OPP) * 8;
        if i % OPP != OPP - 1 {
            pool.read(pid, |p| (rd64(p, o) as usize, rd64(p, o + 8) as usize))
        } else {
            (pool.read(pid, |p| rd64(p, o)) as usize, pool.read(pid + 1, |p| rd64(p, 0)) as usize)
        }
    }

    #[inline(always)]
    pub fn expand(&self, pool: &mut Pool, node: u64, dir: u8, ty: Option<u32>, mut f: impl FnMut(u64, u64)) {
        let d = dir as usize;
        let (a, b) = self.offs(pool, d, node);
        if !self.comp {
            let mut i = a;
            while i < b {
                let pg = i / FPP;
                let end = b.min((pg + 1) * FPP);
                pool.read(self.ent_base[d] + pg as u32, |p| {
                    for k in i..end {
                        let o = (k % FPP) * FE;
                        if let Some(t) = ty {
                            if p[o + 4] as u32 != t {
                                continue;
                            }
                        }
                        f(rd64(p, o + 5), rd32(p, o) as u64);
                    }
                });
                i = end;
            }
        } else {
            let (mut pty, mut pnb) = (0u32, 0u64);
            let mut x = a;
            while x < b {
                let pg = x / PAGE;
                let lim = b.min((pg + 1) * PAGE) - pg * PAGE;
                pool.read(self.ent_base[d] + pg as u32, |p| {
                    let mut o = x % PAGE;
                    while o < lim {
                        if p[o] == 0 {
                            break; // zero fill: the next entry starts on the next page
                        }
                        let h = get_var(p, &mut o) - 1;
                        let nb = if h & 1 != 0 {
                            pty = p[o] as u32;
                            o += 1;
                            h >> 1
                        } else {
                            pnb + (h >> 1)
                        };
                        pnb = nb;
                        let e = get_var(p, &mut o);
                        if ty.is_none_or(|t| t == pty) {
                            f(e, nb);
                        }
                    }
                });
                x = (pg + 1) * PAGE;
            }
        }
    }

    pub fn pages(&self) -> u64 {
        self.off_pages + self.ent_pages
    }
}
