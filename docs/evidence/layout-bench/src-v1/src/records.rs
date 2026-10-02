//! Layout D: record chains in the style of the Neo4j record format, with the
//! research-02 "R2" indirection (logical id -> slot map) so that identifiers
//! are never reused while slots may be.
//!
//! Node record (24 B): logical id u64, first u32 (rel or group slot), degree u32, dense u8.
//! Relationship record (52 B): src slot, dst slot, type, src_prev, src_next,
//!   dst_prev, dst_next (u32 each), edge id u64, created u64, deleted u64.
//!   Each relationship is in two doubly linked chains, one per endpoint.
//! Group record (20 B): type, first_out, first_in, next group, owner (u32 each).
//! A node with more than DENSE relationships is converted to per-type groups
//! with separate OUT and IN chains (Neo4j default threshold 50).
//! Maps: node id -> slot and edge id -> slot, fixed-width B+trees (12 B entries).
//! Chains hold slots, so traversal does not use the maps; the start node and
//! each returned neighbour's logical id need the map / node record.

use crate::btree::{FixedTree, HDR};
use crate::pool::{NONE, PAGE, Pool};
use crate::util::{rd32, rd64, wr32, wr64};

pub const DENSE: u32 = 50;
const NR: usize = 24;
const NPP: usize = (PAGE - HDR) / NR;
const RR: usize = 52;
const RPP: usize = (PAGE - HDR) / RR;
const GR: usize = 20;
const GPP: usize = (PAGE - HDR) / GR;
pub const ME: usize = 12;

pub struct Recs {
    node_pids: Vec<u32>,
    rel_pids: Vec<u32>,
    grp_pids: Vec<u32>,
    nrel: u32,
    ngrp: u32,
    pub nmap: FixedTree,
    pub emap: FixedTree,
    /// Writes to already existing records (pointer and counter updates).
    pub rewrites: u64,
    pub densified: u64,
}

fn map_entry(id: u64, slot: u32) -> [u8; ME] {
    let mut e = [0u8; ME];
    e[..8].copy_from_slice(&id.to_be_bytes());
    wr32(&mut e, 8, slot);
    e
}

struct Rel {
    src: u32,
    dst: u32,
    ty: u32,
    snext: u32,
    dnext: u32,
    eid: u64,
    cts: u64,
    dts: u64,
}

impl Recs {
    /// Creates `n` node records (slot = creation order) and the node map.
    pub fn new(pool: &mut Pool, n: u64) -> Recs {
        let np = (n as usize).div_ceil(NPP);
        let node_pids: Vec<u32> = (0..np).map(|_| pool.alloc()).collect();
        for s in 0..n as usize {
            let (pid, o) = (node_pids[s / NPP], HDR + (s % NPP) * NR);
            pool.write(pid, |p| {
                wr64(p, o, s as u64);
                wr32(p, o + 8, NONE);
                wr32(p, o + 12, 0);
                p[o + 16] = 0;
            });
        }
        let mut it = (0..n).map(|id| map_entry(id, id as u32).to_vec());
        let nmap = FixedTree::bulk(pool, ME, 8, &mut it, 1.0);
        let emap = FixedTree::new(pool, ME, 8);
        Recs { node_pids, rel_pids: Vec::new(), grp_pids: Vec::new(), nrel: 0, ngrp: 0, nmap, emap, rewrites: 0, densified: 0 }
    }

    pub fn pages(&self) -> (u64, u64, u64) {
        (self.node_pids.len() as u64, self.rel_pids.len() as u64, self.grp_pids.len() as u64)
    }

    #[inline(always)]
    fn nloc(&self, s: u32) -> (u32, usize) {
        let s = s as usize;
        (self.node_pids[s / NPP], HDR + (s % NPP) * NR)
    }
    #[inline(always)]
    fn rloc(&self, s: u32) -> (u32, usize) {
        let s = s as usize;
        (self.rel_pids[s / RPP], HDR + (s % RPP) * RR)
    }
    #[inline(always)]
    fn gloc(&self, s: u32) -> (u32, usize) {
        let s = s as usize;
        (self.grp_pids[s / GPP], HDR + (s % GPP) * GR)
    }

    #[inline(always)]
    fn read_rel(&self, pool: &mut Pool, r: u32) -> Rel {
        let (pid, o) = self.rloc(r);
        pool.read(pid, |p| Rel {
            src: rd32(p, o),
            dst: rd32(p, o + 4),
            ty: rd32(p, o + 8),
            snext: rd32(p, o + 16),
            dnext: rd32(p, o + 24),
            eid: rd64(p, o + 28),
            cts: rd64(p, o + 36),
            dts: rd64(p, o + 44),
        })
    }
    /// Pointer field offset: side = true for the source side.
    #[inline(always)]
    fn ptr_off(src_side: bool, next: bool) -> usize {
        12 + if src_side { 0 } else { 8 } + if next { 4 } else { 0 }
    }
    fn set_ptr(&mut self, pool: &mut Pool, r: u32, src_side: bool, next: bool, v: u32) {
        let (pid, o) = self.rloc(r);
        let off = o + Self::ptr_off(src_side, next);
        pool.write(pid, |p| wr32(p, off, v));
    }
    fn side_of(&self, pool: &mut Pool, r: u32, x: u32) -> bool {
        let (pid, o) = self.rloc(r);
        pool.read(pid, |p| rd32(p, o) == x)
    }
    fn node(&self, pool: &mut Pool, x: u32) -> (u32, u32, u8) {
        let (pid, o) = self.nloc(x);
        pool.read(pid, |p| (rd32(p, o + 8), rd32(p, o + 12), p[o + 16]))
    }
    fn set_node(&mut self, pool: &mut Pool, x: u32, first: u32, deg: u32, dense: u8) {
        let (pid, o) = self.nloc(x);
        pool.write(pid, |p| {
            wr32(p, o + 8, first);
            wr32(p, o + 12, deg);
            p[o + 16] = dense;
        });
    }
    /// Logical id of node slot `x` (read from its record).
    #[inline(always)]
    pub fn lid(&self, pool: &mut Pool, x: u32) -> u64 {
        let (pid, o) = self.nloc(x);
        pool.read(pid, |p| rd64(p, o))
    }
    pub fn slot_of(&mut self, pool: &mut Pool, id: u64) -> u32 {
        let mut out = [0u8; ME];
        if self.nmap.get(pool, &id.to_be_bytes(), &mut out) { rd32(&out, 8) } else { NONE }
    }

    fn new_group(&mut self, pool: &mut Pool, ty: u32, owner: u32, next: u32) -> u32 {
        let g = self.ngrp;
        self.ngrp += 1;
        if g as usize / GPP >= self.grp_pids.len() {
            self.grp_pids.push(pool.alloc());
        }
        let (pid, o) = self.gloc(g);
        pool.write(pid, |p| {
            wr32(p, o, ty);
            wr32(p, o + 4, NONE);
            wr32(p, o + 8, NONE);
            wr32(p, o + 12, next);
            wr32(p, o + 16, owner);
        });
        g
    }
    fn group(&self, pool: &mut Pool, g: u32) -> (u32, u32, u32, u32) {
        let (pid, o) = self.gloc(g);
        pool.read(pid, |p| (rd32(p, o), rd32(p, o + 4), rd32(p, o + 8), rd32(p, o + 12)))
    }
    fn set_group_head(&mut self, pool: &mut Pool, g: u32, out: bool, v: u32) {
        let (pid, o) = self.gloc(g);
        let off = o + if out { 4 } else { 8 };
        pool.write(pid, |p| wr32(p, off, v));
    }

    /// Links relationship `r` into node `x`'s chain; `src_side` = x is the source.
    fn link(&mut self, pool: &mut Pool, x: u32, r: u32, src_side: bool, ty: u32) {
        let (first, deg, dense) = self.node(pool, x);
        if dense == 0 {
            self.set_ptr(pool, r, src_side, true, first);
            if first != NONE {
                let s = self.side_of(pool, first, x);
                self.set_ptr(pool, first, s, false, r);
                self.rewrites += 1;
            }
            self.set_node(pool, x, r, deg + 1, 0);
            self.rewrites += 1;
            if deg + 1 > DENSE {
                self.densify(pool, x);
            }
        } else {
            // Find or create the group for `ty`.
            let mut g = first;
            let mut found = NONE;
            while g != NONE {
                let (gty, _, _, nx) = self.group(pool, g);
                if gty == ty {
                    found = g;
                    break;
                }
                g = nx;
            }
            let mut head_first = first;
            if found == NONE {
                found = self.new_group(pool, ty, x, first);
                head_first = found;
            }
            let (_, fo, fi, _) = self.group(pool, found);
            let head = if src_side { fo } else { fi };
            self.set_ptr(pool, r, src_side, true, head);
            if head != NONE {
                let s = self.side_of(pool, head, x);
                self.set_ptr(pool, head, s, false, r);
                self.rewrites += 1;
            }
            self.set_group_head(pool, found, src_side, r);
            self.rewrites += 1;
            self.set_node(pool, x, head_first, deg + 1, 1);
            self.rewrites += 1;
        }
    }

    /// Converts node `x` from one chain to per-type groups with OUT/IN chains.
    fn densify(&mut self, pool: &mut Pool, x: u32) {
        self.densified += 1;
        let (first, deg, _) = self.node(pool, x);
        let mut rels = Vec::new();
        let mut cur = first;
        while cur != NONE {
            let r = self.read_rel(pool, cur);
            let s = r.src == x;
            rels.push((cur, s, r.ty));
            cur = if s { r.snext } else { r.dnext };
        }
        let mut groups: Vec<(u32, u32)> = Vec::new(); // (type, slot)
        let mut gfirst = NONE;
        for &(_, _, ty) in &rels {
            if !groups.iter().any(|g| g.0 == ty) {
                let g = self.new_group(pool, ty, x, gfirst);
                gfirst = g;
                groups.push((ty, g));
            }
        }
        for &(r, s, ty) in rels.iter().rev() {
            let g = groups.iter().find(|g| g.0 == ty).map(|g| g.1).unwrap_or(NONE);
            let (_, fo, fi, _) = self.group(pool, g);
            let head = if s { fo } else { fi };
            self.set_ptr(pool, r, s, false, NONE);
            self.set_ptr(pool, r, s, true, head);
            if head != NONE {
                let hs = self.side_of(pool, head, x);
                self.set_ptr(pool, head, hs, false, r);
            }
            self.set_group_head(pool, g, s, r);
            self.rewrites += 2;
        }
        self.set_node(pool, x, gfirst, deg, 1);
    }

    pub fn insert(&mut self, pool: &mut Pool, src: u64, dst: u64, ty: u32, eid: u64, cts: u64) {
        let r = self.insert_raw(pool, src, dst, ty, eid, cts);
        self.emap.insert(pool, &map_entry(eid, r));
    }

    /// Bulk variant: builds the edge map bottom-up from (edge id, slot) pairs sorted by id.
    pub fn finish_emap(&mut self, pool: &mut Pool, pairs: &[(u64, u32)]) {
        let mut it = pairs.iter().map(|&(e, r)| map_entry(e, r).to_vec());
        self.emap = FixedTree::bulk(pool, ME, 8, &mut it, 1.0);
    }

    /// Inserts without touching the edge map; returns the relationship slot.
    pub fn insert_raw(&mut self, pool: &mut Pool, src: u64, dst: u64, ty: u32, eid: u64, cts: u64) -> u32 {
        let s = self.slot_of(pool, src);
        let d = self.slot_of(pool, dst);
        let r = self.nrel;
        self.nrel += 1;
        if r as usize / RPP >= self.rel_pids.len() {
            self.rel_pids.push(pool.alloc());
        }
        let (pid, o) = self.rloc(r);
        pool.write(pid, |p| {
            wr32(p, o, s);
            wr32(p, o + 4, d);
            wr32(p, o + 8, ty);
            for k in 0..4 {
                wr32(p, o + 12 + 4 * k, NONE);
            }
            wr64(p, o + 28, eid);
            wr64(p, o + 36, cts);
            wr64(p, o + 44, u64::MAX);
        });
        self.link(pool, s, r, true, ty);
        self.link(pool, d, r, false, ty);
        r
    }

    pub fn delete(&mut self, pool: &mut Pool, eid: u64, ts: u64) -> bool {
        let mut out = [0u8; ME];
        if !self.emap.get(pool, &eid.to_be_bytes(), &mut out) {
            return false;
        }
        let r = rd32(&out, 8);
        let (pid, o) = self.rloc(r);
        pool.write(pid, |p| wr64(p, o + 44, ts));
        true
    }

    #[inline(always)]
    fn walk(&self, pool: &mut Pool, mut cur: u32, x: u32, want_out: bool, ty: Option<u32>, snap: u64, f: &mut impl FnMut(u64, u32)) {
        while cur != NONE {
            let r = self.read_rel(pool, cur);
            let s = r.src == x;
            if s == want_out && ty.is_none_or(|t| t == r.ty) && crate::adj::visible(r.cts, r.dts, snap) {
                f(r.eid, if s { r.dst } else { r.src });
            }
            cur = if s { r.snext } else { r.dnext };
        }
    }

    /// Expands node slot `x`; `f(edge id, neighbour slot)`.
    #[inline(always)]
    pub fn expand_slot(&self, pool: &mut Pool, x: u32, out: bool, ty: Option<u32>, snap: u64, mut f: impl FnMut(u64, u32)) {
        let (first, _, dense) = self.node(pool, x);
        if dense == 0 {
            self.walk(pool, first, x, out, ty, snap, &mut f);
        } else {
            let mut g = first;
            while g != NONE {
                let (gty, fo, fi, nx) = self.group(pool, g);
                if ty.is_none_or(|t| t == gty) {
                    self.walk(pool, if out { fo } else { fi }, x, out, ty, snap, &mut f);
                }
                g = nx;
            }
        }
    }
}
