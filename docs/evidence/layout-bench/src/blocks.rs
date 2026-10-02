//! Layout C: per-node adjacency blocks (append-only logs per node and
//! direction, with stamps per entry), with overflow to chained full pages.
//!
//! Node directory: 32 B per node (two 16 B direction slots), addressed by node
//! id (255 nodes per page). A slot holds (page u32, aux u32, len u32, class u8):
//!   class 0..8: a block of CAPS[class] entries at byte offset `aux` in `page`;
//!   class 9 (CHAIN): `page` is the head of a chain of full pages, `aux` the tail.
//! Blocks of one class are carved from slab pages of that class. A full block
//! is relocated to a block of the next class (copy O(degree)); the old block is
//! put on a free list. Past the largest class the list moves to chained pages.
//! Entry (36 B): type u32, neighbour u64, edge id u64, created u64, deleted u64.
//! Entries are in append order, so a type filter scans the whole list.

use crate::adj::{EE, LIVE, OUT, edge_lookup, enc_edge, visible};
use crate::btree::{FixedTree, HDR, cnt, next_of, set_cnt, set_next};
use crate::pool::{NONE, PAGE, Pool};
use crate::util::{rd32, rd64, wr32, wr64};

pub const CE: usize = 36;
pub const CAPS: [usize; 9] = [1, 2, 4, 8, 16, 32, 64, 113, 226];
pub const CHAIN: u8 = 9;
const CPC: usize = (PAGE - HDR) / CE; // 226 entries per chain page
const DS: usize = 32;
const DPP: usize = (PAGE - HDR) / DS; // 255 nodes per directory page

#[inline(always)]
fn enc(ty: u32, nbr: u64, eid: u64, cts: u64, dts: u64) -> [u8; CE] {
    let mut e = [0u8; CE];
    wr32(&mut e, 0, ty);
    wr64(&mut e, 4, nbr);
    wr64(&mut e, 12, eid);
    wr64(&mut e, 20, cts);
    wr64(&mut e, 28, dts);
    e
}

pub struct Blocks {
    dir_pids: Vec<u32>,
    open: [(u32, usize); 9],
    free: [Vec<(u32, u32)>; 9],
    pub et: FixedTree,
    pub pages_dir: u64,
    pub pages_blk: u64,
    pub pages_chain: u64,
    pub relocations: u64,
    pub copied_entries: u64,
    tmp: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Slot {
    pid: u32,
    aux: u32,
    len: u32,
    class: u8,
}

impl Blocks {
    pub fn new(pool: &mut Pool, n: u64) -> Blocks {
        let nd = (n as usize).div_ceil(DPP);
        let dir_pids: Vec<u32> = (0..nd).map(|_| pool.alloc()).collect();
        let et = FixedTree::new(pool, EE, 8);
        Blocks {
            dir_pids,
            open: [(NONE, 0); 9],
            free: Default::default(),
            et,
            pages_dir: nd as u64,
            pages_blk: 0,
            pages_chain: 0,
            relocations: 0,
            copied_entries: 0,
            tmp: Vec::new(),
        }
    }

    #[inline(always)]
    fn dloc(&self, node: u64, dir: u8) -> (u32, usize) {
        let n = node as usize;
        (self.dir_pids[n / DPP], HDR + (n % DPP) * DS + dir as usize * 16)
    }
    #[inline(always)]
    fn rslot(p: &[u8], o: usize) -> Slot {
        Slot { pid: rd32(p, o), aux: rd32(p, o + 4), len: rd32(p, o + 8), class: p[o + 12] }
    }
    fn wslot(pool: &mut Pool, dp: u32, o: usize, s: Slot) {
        pool.write(dp, |p| {
            wr32(p, o, s.pid);
            wr32(p, o + 4, s.aux);
            wr32(p, o + 8, s.len);
            p[o + 12] = s.class;
        });
    }

    fn balloc(&mut self, pool: &mut Pool, c: usize) -> (u32, u32) {
        if let Some(x) = self.free[c].pop() {
            return x;
        }
        let per = (PAGE - HDR) / (CE * CAPS[c]);
        if self.open[c].0 == NONE || self.open[c].1 == per {
            self.open[c] = (pool.alloc(), 0);
            self.pages_blk += 1;
        }
        let off = HDR + self.open[c].1 * CE * CAPS[c];
        self.open[c].1 += 1;
        (self.open[c].0, off as u32)
    }

    fn append(&mut self, pool: &mut Pool, node: u64, dir: u8, e: &[u8; CE]) {
        let (dp, o) = self.dloc(node, dir);
        let mut s = pool.read(dp, |p| Self::rslot(p, o));
        if s.len == 0 {
            let (pid, off) = self.balloc(pool, 0);
            pool.write(pid, |p| p[off as usize..off as usize + CE].copy_from_slice(e));
            s = Slot { pid, aux: off, len: 1, class: 0 };
        } else if s.class < CHAIN {
            let c = s.class as usize;
            let len = s.len as usize;
            if len < CAPS[c] {
                let at = s.aux as usize + len * CE;
                pool.write(s.pid, |p| p[at..at + CE].copy_from_slice(e));
                s.len += 1;
            } else {
                self.relocations += 1;
                self.copied_entries += len as u64;
                let tmp = &mut self.tmp;
                tmp.clear();
                let from = s.aux as usize;
                pool.read(s.pid, |p| tmp.extend_from_slice(&p[from..from + len * CE]));
                self.free[c].push((s.pid, s.aux));
                if c + 1 < CHAIN as usize {
                    let (pid, off) = self.balloc(pool, c + 1);
                    let off_u = off as usize;
                    let tmp = &self.tmp;
                    pool.write(pid, |p| {
                        p[off_u..off_u + len * CE].copy_from_slice(tmp);
                        p[off_u + len * CE..off_u + (len + 1) * CE].copy_from_slice(e);
                    });
                    s = Slot { pid, aux: off, len: s.len + 1, class: (c + 1) as u8 };
                } else {
                    let head = pool.alloc();
                    let tail = pool.alloc();
                    self.pages_chain += 2;
                    let tmp = &self.tmp;
                    pool.write(head, |p| {
                        p[HDR..HDR + len * CE].copy_from_slice(tmp);
                        set_cnt(p, len);
                        set_next(p, tail);
                    });
                    pool.write(tail, |p| {
                        p[HDR..HDR + CE].copy_from_slice(e);
                        set_cnt(p, 1);
                        set_next(p, NONE);
                    });
                    s = Slot { pid: head, aux: tail, len: s.len + 1, class: CHAIN };
                }
            }
        } else {
            let tail = s.aux;
            let n = pool.read(tail, cnt);
            if n < CPC {
                pool.write(tail, |p| {
                    p[HDR + n * CE..HDR + (n + 1) * CE].copy_from_slice(e);
                    set_cnt(p, n + 1);
                });
            } else {
                let nt = pool.alloc();
                self.pages_chain += 1;
                pool.write(nt, |p| {
                    p[HDR..HDR + CE].copy_from_slice(e);
                    set_cnt(p, 1);
                    set_next(p, NONE);
                });
                pool.write(tail, |p| set_next(p, nt));
                s.aux = nt;
            }
            s.len += 1;
        }
        Self::wslot(pool, dp, o, s);
    }

    pub fn insert(&mut self, pool: &mut Pool, src: u64, dst: u64, ty: u32, eid: u64, cts: u64) {
        self.et.insert(pool, &enc_edge(eid, src, dst, ty));
        self.append(pool, src, 0, &enc(ty, dst, eid, cts, LIVE));
        self.append(pool, dst, 1, &enc(ty, src, eid, cts, LIVE));
    }

    /// Scans the list of (node, dir); `f(entry bytes)` returns false to stop.
    #[inline(always)]
    fn scan(&self, pool: &mut Pool, node: u64, dir: u8, mut f: impl FnMut(&[u8]) -> bool) {
        let (dp, o) = self.dloc(node, dir);
        let s = pool.read(dp, |p| Self::rslot(p, o));
        if s.len == 0 {
            return;
        }
        if s.class < CHAIN {
            let a = s.aux as usize;
            let len = s.len as usize;
            pool.read(s.pid, |p| {
                for i in 0..len {
                    if !f(&p[a + i * CE..a + (i + 1) * CE]) {
                        return;
                    }
                }
            });
        } else {
            let mut pid = s.pid;
            while pid != NONE {
                let (go, nx) = pool.read(pid, |p| {
                    let n = cnt(p);
                    for i in 0..n {
                        if !f(&p[HDR + i * CE..HDR + (i + 1) * CE]) {
                            return (false, NONE);
                        }
                    }
                    (true, next_of(p))
                });
                if !go {
                    return;
                }
                pid = nx;
            }
        }
    }

    #[inline(always)]
    pub fn expand(&mut self, pool: &mut Pool, node: u64, dir: u8, ty: Option<u32>, snap: u64, mut f: impl FnMut(u64, u64)) {
        self.scan(pool, node, dir, |e| {
            if let Some(t) = ty {
                if rd32(e, 0) != t {
                    return true;
                }
            }
            if visible(rd64(e, 20), rd64(e, 28), snap) {
                f(rd64(e, 12), rd64(e, 4));
            }
            true
        });
    }

    /// Finds the entry with `eid` in the list and sets its deleted stamp.
    fn stamp(&mut self, pool: &mut Pool, node: u64, dir: u8, eid: u64, ts: u64) -> bool {
        let (dp, o) = self.dloc(node, dir);
        let s = pool.read(dp, |p| Self::rslot(p, o));
        if s.len == 0 {
            return false;
        }
        let find = |p: &[u8], base: usize, n: usize| (0..n).find(|&i| rd64(p, base + i * CE + 12) == eid);
        if s.class < CHAIN {
            let a = s.aux as usize;
            let len = s.len as usize;
            if let Some(i) = pool.read(s.pid, |p| find(p, a, len)) {
                pool.write(s.pid, |p| wr64(p, a + i * CE + 28, ts));
                return true;
            }
            false
        } else {
            let mut pid = s.pid;
            while pid != NONE {
                let (hit, nx) = pool.read(pid, |p| (find(p, HDR, cnt(p)), next_of(p)));
                if let Some(i) = hit {
                    pool.write(pid, |p| wr64(p, HDR + i * CE + 28, ts));
                    return true;
                }
                pid = nx;
            }
            false
        }
    }

    pub fn delete(&mut self, pool: &mut Pool, eid: u64, ts: u64) -> bool {
        let Some((s, d, _)) = edge_lookup(&mut self.et, pool, eid) else { return false };
        let a = self.stamp(pool, s, OUT, eid, ts);
        let b = self.stamp(pool, d, 1, eid, ts);
        a && b
    }

    /// Bulk build: `lists(node, dir)` yields each list's entries in order.
    pub fn bulk(pool: &mut Pool, n: u64, out: &[(u32, Vec<[u8; CE]>)], inn: &[(u32, Vec<[u8; CE]>)]) -> Blocks {
        let mut b = Blocks::new(pool, n);
        for (dir, lists) in [(0u8, out), (1u8, inn)] {
            for (node, es) in lists {
                if es.is_empty() {
                    continue;
                }
                let len = es.len();
                let (dp, o) = b.dloc(*node as u64, dir);
                let s = if let Some(c) = CAPS.iter().position(|&c| c >= len) {
                    let (pid, off) = b.balloc(pool, c);
                    let a = off as usize;
                    pool.write(pid, |p| {
                        for (i, e) in es.iter().enumerate() {
                            p[a + i * CE..a + (i + 1) * CE].copy_from_slice(e);
                        }
                    });
                    Slot { pid, aux: off, len: len as u32, class: c as u8 }
                } else {
                    let np = len.div_ceil(CPC);
                    let pids: Vec<u32> = (0..np).map(|_| pool.alloc()).collect();
                    b.pages_chain += np as u64;
                    for (k, ch) in es.chunks(CPC).enumerate() {
                        let nx = if k + 1 < np { pids[k + 1] } else { NONE };
                        pool.write(pids[k], |p| {
                            for (i, e) in ch.iter().enumerate() {
                                p[HDR + i * CE..HDR + (i + 1) * CE].copy_from_slice(e);
                            }
                            set_cnt(p, ch.len());
                            set_next(p, nx);
                        });
                    }
                    Slot { pid: pids[0], aux: pids[np - 1], len: len as u32, class: CHAIN }
                };
                Self::wslot(pool, dp, o, s);
            }
        }
        b
    }

    pub fn entry(ty: u32, nbr: u64, eid: u64, cts: u64) -> [u8; CE] {
        enc(ty, nbr, eid, cts, LIVE)
    }

    /// Node-ordered full scan of both directions (projection source).
    pub fn scan_node(&mut self, pool: &mut Pool, node: u64, dir: u8, snap: u64, mut f: impl FnMut(u32, u64, u64)) {
        self.scan(pool, node, dir, |e| {
            if visible(rd64(e, 20), rd64(e, 28), snap) {
                f(rd32(e, 0), rd64(e, 4), rd64(e, 12));
            }
            true
        });
    }

    /// Bytes of live payload in blocks (len * CE summed) for fill reporting.
    pub fn payload(&self, pool: &mut Pool, n: u64) -> (u64, u64) {
        let (mut used, mut reserved) = (0u64, 0u64);
        for node in 0..n {
            for dir in 0..2u8 {
                let (dp, o) = self.dloc(node, dir);
                let s = pool.read(dp, |p| Self::rslot(p, o));
                used += s.len as u64 * CE as u64;
                if s.len > 0 && s.class < CHAIN {
                    reserved += (CAPS[s.class as usize] * CE) as u64;
                }
            }
        }
        (used, reserved)
    }
}
