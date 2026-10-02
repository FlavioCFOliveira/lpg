//! Layouts A and B: one clustered adjacency B+tree keyed by
//! (node u64, dir u8, type u32, neighbour u64, edge id u64) = 29-byte key,
//! two entries per edge (OUT at the source, IN at the target), each carrying
//! created_ts and deleted_ts. Plus an edge table (edge id -> src, dst, type),
//! a fixed-width B+tree, used to locate the adjacency entries on delete.
//!
//! A: fixed 45-byte leaf entries (29 B key + 8 B created + 8 B deleted).
//! B: compressed leaves: per-entry flag byte, node delta, (type,dir) elided
//!    when equal to the previous entry, neighbour delta within a run, varint
//!    edge id, created_ts elided when frozen (0), deleted_ts elided when live.
//!    Restart entries (flag bit 4, encoded without delta; one per group of up to
//!    2*RI entries, RI at build) allow a binary search inside the leaf.
//!    An insert or stamp update re-encodes only the group that holds the key.

use crate::btree::{self, FixedTree, HDR, LEAF, Path, cnt, descend, init_page, next_of, propagate, set_cnt, set_next};
use crate::pool::{NONE, PAGE, Pool};
use crate::util::{get_var, put_var, rd16, rd64, rd64be, wr16, wr64};

pub const LIVE: u64 = u64::MAX;
pub const KLEN: usize = 29;
pub const AE: usize = 45;
pub const OUT: u8 = 0;
pub const IN: u8 = 1;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Adj {
    pub node: u64,
    pub dir: u8,
    pub ty: u32,
    pub nbr: u64,
    pub eid: u64,
    pub cts: u64,
    pub dts: u64,
}
const ZERO: Adj = Adj { node: 0, dir: 0, ty: 0, nbr: 0, eid: 0, cts: 0, dts: 0 };

#[inline(always)]
pub fn key_bytes(node: u64, dir: u8, ty: u32, nbr: u64, eid: u64) -> [u8; KLEN] {
    let mut k = [0u8; KLEN];
    k[..8].copy_from_slice(&node.to_be_bytes());
    k[8] = dir;
    k[9..13].copy_from_slice(&ty.to_be_bytes());
    k[13..21].copy_from_slice(&nbr.to_be_bytes());
    k[21..29].copy_from_slice(&eid.to_be_bytes());
    k
}
pub fn enc_a(a: &Adj) -> [u8; AE] {
    let mut e = [0u8; AE];
    e[..KLEN].copy_from_slice(&key_bytes(a.node, a.dir, a.ty, a.nbr, a.eid));
    wr64(&mut e, 29, a.cts);
    wr64(&mut e, 37, a.dts);
    e
}

/// Edge table entry: key eid (8 B BE) + src 8 + dst 8 + type 4 = 28 B.
pub const EE: usize = 28;
pub fn enc_edge(eid: u64, src: u64, dst: u64, ty: u32) -> [u8; EE] {
    let mut e = [0u8; EE];
    e[..8].copy_from_slice(&eid.to_be_bytes());
    wr64(&mut e, 8, src);
    wr64(&mut e, 16, dst);
    e[24..28].copy_from_slice(&ty.to_le_bytes());
    e
}
pub fn edge_lookup(et: &mut FixedTree, pool: &mut Pool, eid: u64) -> Option<(u64, u64, u32)> {
    let mut out = [0u8; EE];
    if !et.get(pool, &eid.to_be_bytes(), &mut out) {
        return None;
    }
    Some((rd64(&out, 8), rd64(&out, 16), u32::from_le_bytes([out[24], out[25], out[26], out[27]])))
}

#[inline(always)]
pub fn visible(cts: u64, dts: u64, snap: u64) -> bool {
    cts <= snap && snap < dts
}

// ---------------------------------------------------------------- A

pub struct AdjA {
    pub t: FixedTree,
    pub et: FixedTree,
}

impl AdjA {
    pub fn new(pool: &mut Pool) -> AdjA {
        AdjA { t: FixedTree::new(pool, AE, KLEN), et: FixedTree::new(pool, EE, 8) }
    }
    pub fn insert(&mut self, pool: &mut Pool, src: u64, dst: u64, ty: u32, eid: u64, cts: u64) {
        self.et.insert(pool, &enc_edge(eid, src, dst, ty));
        self.t.insert(pool, &enc_a(&Adj { node: src, dir: OUT, ty, nbr: dst, eid, cts, dts: LIVE }));
        self.t.insert(pool, &enc_a(&Adj { node: dst, dir: IN, ty, nbr: src, eid, cts, dts: LIVE }));
    }
    pub fn delete(&mut self, pool: &mut Pool, eid: u64, ts: u64) -> bool {
        let Some((s, d, ty)) = edge_lookup(&mut self.et, pool, eid) else { return false };
        let a = self.t.update(pool, &key_bytes(s, OUT, ty, d, eid), |e| wr64(e, 37, ts));
        let b = self.t.update(pool, &key_bytes(d, IN, ty, s, eid), |e| wr64(e, 37, ts));
        a && b
    }
    #[inline(always)]
    pub fn expand(&mut self, pool: &mut Pool, node: u64, dir: u8, ty: Option<u32>, snap: u64, mut f: impl FnMut(u64, u64)) {
        let key = key_bytes(node, dir, ty.unwrap_or(0), 0, 0);
        let plen = if ty.is_some() { 13 } else { 9 };
        self.t.scan_from(pool, &key, |e| {
            if e[..plen] != key[..plen] {
                return false;
            }
            if visible(rd64(e, 29), rd64(e, 37), snap) {
                f(rd64be(e, 21), rd64be(e, 13));
            }
            true
        });
    }
    /// Full scan in key order (projection source).
    pub fn scan_all(&mut self, pool: &mut Pool, snap: u64, mut f: impl FnMut(u64, u8, u32, u64, u64)) {
        let key = [0u8; KLEN];
        self.t.scan_from(pool, &key, |e| {
            if visible(rd64(e, 29), rd64(e, 37), snap) {
                f(rd64be(e, 0), e[8], crate::util::rd32be(e, 9), rd64be(e, 13), rd64be(e, 21));
            }
            true
        });
    }
}

// ---------------------------------------------------------------- B

pub const RI: usize = 16;
const RESTART: u8 = 16;

#[inline(always)]
fn enc_entry(out: &mut Vec<u8>, e: &Adj, prev: &Adj, restart: bool) {
    let same_node = !restart && e.node == prev.node;
    let same_dt = !restart && e.ty == prev.ty && e.dir == prev.dir;
    let frozen = e.cts == 0;
    let live = e.dts == LIVE;
    let fl = (same_node as u8) | (same_dt as u8) << 1 | (frozen as u8) << 2 | (live as u8) << 3 | if restart { RESTART } else { 0 };
    out.push(fl);
    if !same_node {
        put_var(out, e.node - if restart { 0 } else { prev.node });
    }
    if !same_dt {
        put_var(out, ((e.ty as u64) << 1) | e.dir as u64);
    }
    if same_node && same_dt {
        put_var(out, e.nbr - prev.nbr);
    } else {
        put_var(out, e.nbr);
    }
    put_var(out, e.eid);
    if !frozen {
        put_var(out, e.cts);
    }
    if !live {
        put_var(out, e.dts);
    }
}

/// Decodes one entry; a restart entry (flag bit 4) ignores `prev`.
#[inline(always)]
fn dec_entry(p: &[u8], off: &mut usize, prev: &Adj) -> Adj {
    let fl = p[*off];
    *off += 1;
    let prev = if fl & RESTART != 0 { &ZERO } else { prev };
    let node = if fl & 1 != 0 { prev.node } else { prev.node + get_var(p, off) };
    let (ty, dir) = if fl & 2 != 0 {
        (prev.ty, prev.dir)
    } else {
        let v = get_var(p, off);
        ((v >> 1) as u32, (v & 1) as u8)
    };
    let nbr = if fl & 3 == 3 { prev.nbr + get_var(p, off) } else { get_var(p, off) };
    let eid = get_var(p, off);
    let cts = if fl & 4 != 0 { 0 } else { get_var(p, off) };
    let dts = if fl & 8 != 0 { LIVE } else { get_var(p, off) };
    Adj { node, dir, ty, nbr, eid, cts, dts }
}

/// Encodes `es` as one or more restart groups of <= `ri` entries, appending to `enc`;
/// pushes each group's page offset (`base` + position) to `rs`.
fn encode_groups(es: &[Adj], enc: &mut Vec<u8>, rs: &mut Vec<u16>, base: usize, ri: usize) {
    let mut prev = ZERO;
    for (i, e) in es.iter().enumerate() {
        let restart = i % ri == 0;
        if restart {
            rs.push((base + enc.len()) as u16);
        }
        enc_entry(enc, e, &prev, restart);
        prev = *e;
    }
}
fn encode_leaf(es: &[Adj], enc: &mut Vec<u8>, rs: &mut Vec<u16>) {
    enc.clear();
    rs.clear();
    encode_groups(es, enc, rs, HDR, RI);
}
fn write_rs(p: &mut [u8], rs: &[u16]) {
    wr16(p, 10, rs.len() as u16);
    for (i, r) in rs.iter().enumerate() {
        wr16(p, PAGE - 2 * (i + 1), *r);
    }
}
fn write_leaf(p: &mut [u8], enc: &[u8], rs: &[u16], n: usize, next: u32) {
    init_page(p, LEAF);
    set_cnt(p, n);
    set_next(p, next);
    wr16(p, 8, enc.len() as u16);
    p[HDR..HDR + enc.len()].copy_from_slice(enc);
    write_rs(p, rs);
}
fn decode_range(p: &[u8], from: usize, to: usize, out: &mut Vec<Adj>) {
    out.clear();
    let mut off = from;
    let mut prev = ZERO;
    while off < to {
        let e = dec_entry(p, &mut off, &prev);
        out.push(e);
        prev = e;
    }
}
fn decode_leaf(p: &[u8], out: &mut Vec<Adj>) {
    decode_range(p, HDR, HDR + rd16(p, 8) as usize, out);
}
#[inline(always)]
fn restart_off(p: &[u8], i: usize) -> usize {
    rd16(p, PAGE - 2 * (i + 1)) as usize
}
#[inline(always)]
fn kt(a: &Adj) -> (u64, u8, u32, u64, u64) {
    (a.node, a.dir, a.ty, a.nbr, a.eid)
}
/// Index of the last restart group whose first key <= `t` (0 if none).
#[inline(always)]
fn find_group(p: &[u8], t: (u64, u8, u32, u64, u64)) -> usize {
    let nrs = rd16(p, 10) as usize;
    let (mut lo, mut hi) = (0usize, nrs);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let mut o = restart_off(p, mid);
        if kt(&dec_entry(p, &mut o, &ZERO)) <= t {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo.saturating_sub(1)
}

#[derive(Clone, Copy)]
enum Op {
    Insert(Adj),
    SetDts(u64),
}

pub struct CompTree {
    pub root: u32,
    pub height: u32,
    pub pages: u64,
    pub leaves: u64,
    pub entries: u64,
    path: Path,
    scratch: Vec<Adj>,
    enc: Vec<u8>,
    rs: Vec<u16>,
}

enum Res {
    Done,
    NotFound,
    Overflow,
}

impl CompTree {
    pub fn new(pool: &mut Pool) -> CompTree {
        let root = pool.alloc();
        pool.write(root, |p| write_leaf(p, &[], &[], 0, NONE));
        CompTree { root, height: 1, pages: 1, leaves: 1, entries: 0, path: Path::new(), scratch: Vec::new(), enc: Vec::new(), rs: Vec::new() }
    }

    /// Group-local modification: decode the one restart group holding the key,
    /// modify, re-encode it (splitting it past 2*RI entries), shift the tail.
    fn modify_in_page(p: &mut [u8], key: &Adj, op: Op, g: &mut Vec<Adj>, enc: &mut Vec<u8>, rs: &mut Vec<u16>) -> Res {
        let n = cnt(p);
        let nrs = rd16(p, 10) as usize;
        let data_end = HDR + rd16(p, 8) as usize;
        let t = kt(key);
        let (gi, gs, ge) = if nrs == 0 {
            (0, HDR, HDR)
        } else {
            let gi = find_group(p, t);
            let ge = if gi + 1 < nrs { restart_off(p, gi + 1) } else { data_end };
            (gi, restart_off(p, gi), ge)
        };
        decode_range(p, gs, ge, g);
        let pos = g.partition_point(|x| kt(x) < t);
        match op {
            Op::Insert(e) => g.insert(pos, e),
            Op::SetDts(ts) => {
                if pos >= g.len() || kt(&g[pos]) != t {
                    return Res::NotFound;
                }
                g[pos].dts = ts;
            }
        }
        enc.clear();
        rs.clear();
        let ri = if g.len() > 2 * RI { g.len().div_ceil(2) } else { g.len() };
        encode_groups(g, enc, rs, gs, ri);
        let old_len = ge - gs;
        let new_end = data_end - old_len + enc.len();
        let new_nrs = if nrs == 0 { rs.len() } else { nrs - 1 + rs.len() };
        if new_end + 2 * new_nrs > PAGE {
            return Res::Overflow;
        }
        let delta = enc.len() as isize - old_len as isize;
        // Collect restart offsets: before gi unchanged, gi replaced by rs, after gi shifted.
        let mut all: Vec<u16> = Vec::with_capacity(nrs + 2);
        for i in 0..gi.min(nrs) {
            all.push(restart_off(p, i) as u16);
        }
        all.extend_from_slice(rs);
        for i in (gi + 1)..nrs {
            all.push((restart_off(p, i) as isize + delta) as u16);
        }
        p.copy_within(ge..data_end, (ge as isize + delta) as usize);
        p[gs..gs + enc.len()].copy_from_slice(enc);
        wr16(p, 8, (new_end - HDR) as u16);
        write_rs(p, &all);
        if let Op::Insert(_) = op {
            set_cnt(p, n + 1);
        }
        Res::Done
    }

    fn modify(&mut self, pool: &mut Pool, key: &Adj, op: Op) -> bool {
        let kb = key_bytes(key.node, key.dir, key.ty, key.nbr, key.eid);
        let leaf = descend(pool, self.root, self.height, &kb, KLEN, &mut self.path);
        let (scratch, enc, rs) = (&mut self.scratch, &mut self.enc, &mut self.rs);
        let r = pool.write(leaf, |p| Self::modify_in_page(p, key, op, scratch, enc, rs));
        match r {
            Res::Done => return true,
            Res::NotFound => return false,
            Res::Overflow => {}
        }
        // Leaf split: decode all, apply, split in two halves (or rightmost append).
        let (nxt, rightmost) = pool.read(leaf, |p| {
            decode_leaf(p, scratch);
            (next_of(p), next_of(p) == NONE)
        });
        let t = kt(key);
        let pos = scratch.partition_point(|x| kt(x) < t);
        let append = match op {
            Op::Insert(e) => {
                scratch.insert(pos, e);
                rightmost && pos + 1 == scratch.len()
            }
            Op::SetDts(ts) => {
                scratch[pos].dts = ts;
                false
            }
        };
        let tot = scratch.len();
        let split = if append { tot - 1 } else { tot / 2 };
        let newp = pool.alloc();
        self.pages += 1;
        self.leaves += 1;
        encode_leaf(&scratch[..split], enc, rs);
        pool.write(leaf, |p| write_leaf(p, enc, rs, split, newp));
        encode_leaf(&scratch[split..], enc, rs);
        pool.write(newp, |p| write_leaf(p, enc, rs, tot - split, nxt));
        let s = &scratch[split];
        let sep = key_bytes(s.node, s.dir, s.ty, s.nbr, s.eid);
        propagate(pool, &mut self.root, &mut self.height, &self.path, &sep, newp, KLEN, &mut self.pages);
        true
    }

    pub fn insert(&mut self, pool: &mut Pool, e: Adj) {
        self.entries += 1;
        self.modify(pool, &e, Op::Insert(e));
    }
    pub fn set_dts(&mut self, pool: &mut Pool, key: &Adj, ts: u64) -> bool {
        self.modify(pool, key, Op::SetDts(ts))
    }

    /// Visits entries >= `from` until `f` returns false.
    #[inline(always)]
    pub fn scan_from(&mut self, pool: &mut Pool, from: &Adj, mut f: impl FnMut(&Adj) -> bool) {
        let kb = key_bytes(from.node, from.dir, from.ty, from.nbr, from.eid);
        let mut pid = descend(pool, self.root, self.height, &kb, KLEN, &mut self.path);
        let target = kt(from);
        let mut first = true;
        loop {
            let (go, nx) = pool.read(pid, |p| {
                let end = HDR + rd16(p, 8) as usize;
                let mut off = if first && rd16(p, 10) > 0 { restart_off(p, find_group(p, target)) } else { HDR };
                let mut prev = ZERO;
                while off < end {
                    let e = dec_entry(p, &mut off, &prev);
                    prev = e;
                    if first && kt(&e) < target {
                        continue;
                    }
                    if !f(&e) {
                        return (false, NONE);
                    }
                }
                (true, next_of(p))
            });
            if !go || nx == NONE {
                return;
            }
            pid = nx;
            first = false;
        }
    }

    /// Bottom-up build from sorted entries; leaves filled to `fill` of the page.
    /// Streams: each leaf is written as soon as it is full (leaf pages are
    /// allocated in key order, as before).
    pub fn bulk(pool: &mut Pool, sorted: &mut dyn Iterator<Item = Adj>, fill: f64) -> CompTree {
        let mut t = CompTree { root: NONE, height: 1, pages: 0, leaves: 0, entries: 0, path: Path::new(), scratch: Vec::new(), enc: Vec::new(), rs: Vec::new() };
        let limit = (PAGE as f64 * fill) as usize;
        let mut level: Vec<(Vec<u8>, u32)> = Vec::new();
        let mut cur: Vec<Adj> = Vec::new();
        let mut cur_pid = NONE;
        let mut enc = Vec::new();
        let (mut enc2, mut rs2) = (Vec::new(), Vec::new());
        let mut nrs = 0usize;
        let mut close = |pool: &mut Pool, body: &[Adj], pid: u32, nx: u32, level: &mut Vec<(Vec<u8>, u32)>| {
            encode_leaf(body, &mut enc2, &mut rs2);
            pool.write(pid, |p| write_leaf(p, &enc2, &rs2, body.len(), nx));
            let f = body[0];
            level.push((key_bytes(f.node, f.dir, f.ty, f.nbr, f.eid).to_vec(), pid));
        };
        for e in sorted {
            if cur_pid == NONE {
                cur_pid = pool.alloc();
                t.pages += 1;
                t.leaves += 1;
            }
            let i = cur.len();
            let restart = i % RI == 0;
            let before = enc.len();
            let prev = if i == 0 { ZERO } else { cur[i - 1] };
            enc_entry(&mut enc, &e, &prev, restart);
            if HDR + enc.len() + 2 * (nrs + restart as usize) > limit && i > 0 {
                enc.truncate(before);
                let nx = pool.alloc();
                t.pages += 1;
                t.leaves += 1;
                close(pool, &cur, cur_pid, nx, &mut level);
                cur_pid = nx;
                cur.clear();
                enc.clear();
                enc_entry(&mut enc, &e, &ZERO, true);
                nrs = 1;
            } else {
                nrs += restart as usize;
            }
            cur.push(e);
            t.entries += 1;
        }
        if cur.is_empty() {
            return CompTree::new(pool);
        }
        close(pool, &cur, cur_pid, NONE, &mut level);
        let (root, h) = btree::build_inner(pool, level, KLEN, fill, &mut t.pages);
        t.root = root;
        t.height = h;
        t
    }

    /// Payload bytes used in leaves (encoded entries + restart arrays + headers).
    pub fn used_bytes(&self, pool: &mut Pool) -> u64 {
        let mut pid = descend(pool, self.root, self.height, &[0u8; KLEN], KLEN, &mut Path::new());
        let mut tot = 0u64;
        while pid != NONE {
            let (u, nx) = pool.read(pid, |p| ((HDR + rd16(p, 8) as usize + 2 * rd16(p, 10) as usize) as u64, next_of(p)));
            tot += u;
            pid = nx;
        }
        tot
    }
}

pub struct AdjB {
    pub t: CompTree,
    pub et: FixedTree,
}

impl AdjB {
    pub fn new(pool: &mut Pool) -> AdjB {
        AdjB { t: CompTree::new(pool), et: FixedTree::new(pool, EE, 8) }
    }
    pub fn insert(&mut self, pool: &mut Pool, src: u64, dst: u64, ty: u32, eid: u64, cts: u64) {
        self.et.insert(pool, &enc_edge(eid, src, dst, ty));
        self.t.insert(pool, Adj { node: src, dir: OUT, ty, nbr: dst, eid, cts, dts: LIVE });
        self.t.insert(pool, Adj { node: dst, dir: IN, ty, nbr: src, eid, cts, dts: LIVE });
    }
    pub fn delete(&mut self, pool: &mut Pool, eid: u64, ts: u64) -> bool {
        let Some((s, d, ty)) = edge_lookup(&mut self.et, pool, eid) else { return false };
        let a = self.t.set_dts(pool, &Adj { node: s, dir: OUT, ty, nbr: d, eid, ..ZERO }, ts);
        let b = self.t.set_dts(pool, &Adj { node: d, dir: IN, ty, nbr: s, eid, ..ZERO }, ts);
        a && b
    }
    #[inline(always)]
    pub fn expand(&mut self, pool: &mut Pool, node: u64, dir: u8, ty: Option<u32>, snap: u64, mut f: impl FnMut(u64, u64)) {
        let from = Adj { node, dir, ty: ty.unwrap_or(0), ..ZERO };
        self.t.scan_from(pool, &from, |e| {
            if e.node != node || e.dir != dir {
                return false;
            }
            if let Some(t) = ty {
                if e.ty != t {
                    return false;
                }
            }
            if visible(e.cts, e.dts, snap) {
                f(e.eid, e.nbr);
            }
            true
        });
    }
    pub fn scan_all(&mut self, pool: &mut Pool, snap: u64, mut f: impl FnMut(u64, u8, u32, u64, u64)) {
        self.t.scan_from(pool, &ZERO, |e| {
            if visible(e.cts, e.dts, snap) {
                f(e.node, e.dir, e.ty, e.nbr, e.eid);
            }
            true
        });
    }
}
