//! B+tree over pool pages.
//!
//! Page header (HDR = 32 bytes, reserved for checksum/LSN in a real engine):
//!   [0] kind (1 = leaf, 2 = inner), [2..4] count, [4..8] next leaf.
//! Inner page: children u32 at HDR + 4*i (i = 0..=count), then keys of `klen`
//! bytes. Keys compare by memcmp (big-endian encodings), and a key equal to a
//! separator lives in the right child.
//!
//! `FixedTree` stores fixed-width entries (key prefix + value) sorted in the
//! leaf. Splits are 50/50, except an append at the end of the rightmost leaf,
//! which leaves the old leaf full (PostgreSQL-style rightmost split).

use crate::pool::{NONE, PAGE, Pool};
use crate::util::{rd16, rd32, wr16, wr32};

pub const HDR: usize = 32;
pub const LEAF: u8 = 1;
pub const INNER: u8 = 2;
pub const MAXH: usize = 12;

#[inline(always)]
pub fn cnt(p: &[u8]) -> usize {
    rd16(p, 2) as usize
}
#[inline(always)]
pub fn set_cnt(p: &mut [u8], n: usize) {
    wr16(p, 2, n as u16);
}
#[inline(always)]
pub fn next_of(p: &[u8]) -> u32 {
    rd32(p, 4)
}
#[inline(always)]
pub fn set_next(p: &mut [u8], v: u32) {
    wr32(p, 4, v);
}
pub fn init_page(p: &mut [u8], kind: u8) {
    p[..HDR].fill(0);
    p[0] = kind;
    set_next(p, NONE);
}

/// Key comparison. With the `fastcmp` feature the first 8 bytes (the big-endian
/// node or edge id) are compared as one u64 before falling back to memcmp.
#[inline(always)]
pub fn kcmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    #[cfg(feature = "fastcmp")]
    {
        let (x, y) = (crate::util::rd64be(a, 0), crate::util::rd64be(b, 0));
        if x != y {
            return x.cmp(&y);
        }
    }
    a.cmp(b)
}

#[inline(always)]
pub fn maxk(klen: usize) -> usize {
    (PAGE - HDR - 4) / (klen + 4)
}
#[inline(always)]
fn koff(klen: usize) -> usize {
    HDR + 4 * (maxk(klen) + 1)
}
#[inline(always)]
fn child(p: &[u8], i: usize) -> u32 {
    rd32(p, HDR + 4 * i)
}
#[inline(always)]
fn ikey(p: &[u8], i: usize, klen: usize) -> &[u8] {
    let o = koff(klen) + i * klen;
    &p[o..o + klen]
}

/// Child index to follow for `key`: number of separators <= key.
#[inline(always)]
pub fn inner_find(p: &[u8], key: &[u8], klen: usize) -> (usize, u32) {
    let n = cnt(p);
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if kcmp(ikey(p, mid, klen), key) != std::cmp::Ordering::Greater {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    (lo, child(p, lo))
}

fn inner_insert_at(p: &mut [u8], klen: usize, idx: usize, sep: &[u8], right: u32) {
    let n = cnt(p);
    let ko = koff(klen);
    p.copy_within(ko + idx * klen..ko + n * klen, ko + (idx + 1) * klen);
    p[ko + idx * klen..ko + (idx + 1) * klen].copy_from_slice(sep);
    p.copy_within(HDR + 4 * (idx + 1)..HDR + 4 * (n + 1), HDR + 4 * (idx + 2));
    wr32(p, HDR + 4 * (idx + 1), right);
    set_cnt(p, n + 1);
}

fn write_inner(p: &mut [u8], klen: usize, keys: &[u8], ch: &[u32]) {
    init_page(p, INNER);
    let n = keys.len() / klen;
    let ko = koff(klen);
    p[ko..ko + keys.len()].copy_from_slice(keys);
    for (i, c) in ch.iter().enumerate() {
        wr32(p, HDR + 4 * i, *c);
    }
    set_cnt(p, n);
}

/// Descent path: (inner page, child index taken).
pub struct Path {
    pub v: [(u32, usize); MAXH],
    pub n: usize,
}
impl Path {
    pub fn new() -> Path {
        Path { v: [(0, 0); MAXH], n: 0 }
    }
}

/// Descends from `root` to the leaf for `key`, recording the path.
#[inline(always)]
pub fn descend(pool: &mut Pool, root: u32, height: u32, key: &[u8], klen: usize, path: &mut Path) -> u32 {
    let mut pid = root;
    path.n = 0;
    for _ in 1..height {
        let (idx, c) = pool.read(pid, |p| inner_find(p, key, klen));
        path.v[path.n] = (pid, idx);
        path.n += 1;
        pid = c;
    }
    pid
}

/// Inserts separator `sep` with right sibling `right` into the parents on `path`.
pub fn propagate(pool: &mut Pool, root: &mut u32, height: &mut u32, path: &Path, sep: &[u8], right: u32, klen: usize, pages: &mut u64) {
    let mut sep = sep.to_vec();
    let mut right = right;
    for k in (0..path.n).rev() {
        let (pid, idx) = path.v[k];
        let full = pool.read(pid, |p| cnt(p) >= maxk(klen));
        if !full {
            pool.write(pid, |p| inner_insert_at(p, klen, idx, &sep, right));
            return;
        }
        let (keys, ch) = pool.read(pid, |p| {
            let n = cnt(p);
            let mut keys = Vec::with_capacity((n + 1) * klen);
            let mut ch = Vec::with_capacity(n + 2);
            for i in 0..n {
                if i == idx {
                    keys.extend_from_slice(&sep);
                }
                keys.extend_from_slice(ikey(p, i, klen));
            }
            if idx == n {
                keys.extend_from_slice(&sep);
            }
            for i in 0..=n {
                ch.push(child(p, i));
                if i == idx {
                    ch.push(right);
                }
            }
            (keys, ch)
        });
        let tot = keys.len() / klen;
        let mid = tot / 2;
        let newp = pool.alloc();
        *pages += 1;
        pool.write(pid, |p| write_inner(p, klen, &keys[..mid * klen], &ch[..=mid]));
        pool.write(newp, |p| write_inner(p, klen, &keys[(mid + 1) * klen..], &ch[mid + 1..]));
        sep = keys[mid * klen..(mid + 1) * klen].to_vec();
        right = newp;
    }
    let nr = pool.alloc();
    *pages += 1;
    let old = *root;
    pool.write(nr, |p| write_inner(p, klen, &sep, &[old, right]));
    *root = nr;
    *height += 1;
}

/// Builds the inner levels over `level` = (first key, page) of a finished lower level.
pub fn build_inner(pool: &mut Pool, mut level: Vec<(Vec<u8>, u32)>, klen: usize, fill: f64, pages: &mut u64) -> (u32, u32) {
    let mut height = 1;
    let per = (((maxk(klen) + 1) as f64 * fill) as usize).max(2);
    while level.len() > 1 {
        let mut up = Vec::new();
        for grp in level.chunks(per) {
            let pid = pool.alloc();
            *pages += 1;
            let mut keys = Vec::with_capacity(grp.len() * klen);
            for (k, _) in &grp[1..] {
                keys.extend_from_slice(k);
            }
            let ch: Vec<u32> = grp.iter().map(|x| x.1).collect();
            pool.write(pid, |p| write_inner(p, klen, &keys, &ch));
            up.push((grp[0].0.clone(), pid));
        }
        level = up;
        height += 1;
    }
    (level[0].1, height)
}

pub struct FixedTree {
    pub root: u32,
    pub height: u32,
    pub elen: usize,
    pub klen: usize,
    pub cap: usize,
    pub pages: u64,
    pub leaves: u64,
    pub entries: u64,
    path: Path,
    tmp: Vec<u8>,
}

#[inline(always)]
fn leaf_lb(p: &[u8], key: &[u8], elen: usize, klen: usize) -> usize {
    let n = cnt(p);
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let o = HDR + mid * elen;
        if kcmp(&p[o..o + klen], key) == std::cmp::Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

impl FixedTree {
    pub fn new(pool: &mut Pool, elen: usize, klen: usize) -> FixedTree {
        let root = pool.alloc();
        pool.write(root, |p| init_page(p, LEAF));
        FixedTree {
            root,
            height: 1,
            elen,
            klen,
            cap: (PAGE - HDR) / elen,
            pages: 1,
            leaves: 1,
            entries: 0,
            path: Path::new(),
            tmp: Vec::new(),
        }
    }

    pub fn insert(&mut self, pool: &mut Pool, e: &[u8]) {
        let (elen, klen, cap) = (self.elen, self.klen, self.cap);
        let leaf = descend(pool, self.root, self.height, &e[..klen], klen, &mut self.path);
        self.entries += 1;
        let r = pool.write(leaf, |p| {
            let n = cnt(p);
            let pos = leaf_lb(p, &e[..klen], elen, klen);
            if n < cap {
                let o = HDR + pos * elen;
                p.copy_within(o..HDR + n * elen, o + elen);
                p[o..o + elen].copy_from_slice(e);
                set_cnt(p, n + 1);
                None
            } else {
                Some((pos, n, next_of(p)))
            }
        });
        let Some((pos, n, nxt)) = r else { return };
        // Split.
        let tmp = &mut self.tmp;
        tmp.clear();
        pool.read(leaf, |p| {
            tmp.extend_from_slice(&p[HDR..HDR + pos * elen]);
            tmp.extend_from_slice(e);
            tmp.extend_from_slice(&p[HDR + pos * elen..HDR + n * elen]);
        });
        let split = if nxt == NONE && pos == n { n } else { (n + 1) / 2 };
        let newp = pool.alloc();
        self.pages += 1;
        self.leaves += 1;
        let tot = n + 1;
        pool.write(leaf, |p| {
            init_page(p, LEAF);
            p[HDR..HDR + split * elen].copy_from_slice(&tmp[..split * elen]);
            set_cnt(p, split);
            set_next(p, newp);
        });
        pool.write(newp, |p| {
            init_page(p, LEAF);
            p[HDR..HDR + (tot - split) * elen].copy_from_slice(&tmp[split * elen..]);
            set_cnt(p, tot - split);
            set_next(p, nxt);
        });
        let sep = tmp[split * elen..split * elen + klen].to_vec();
        propagate(pool, &mut self.root, &mut self.height, &self.path, &sep, newp, klen, &mut self.pages);
    }

    /// Visits entries >= key in order until `f` returns false.
    #[inline(always)]
    pub fn scan_from(&mut self, pool: &mut Pool, key: &[u8], mut f: impl FnMut(&[u8]) -> bool) {
        let (elen, klen) = (self.elen, self.klen);
        let mut pid = descend(pool, self.root, self.height, key, klen, &mut self.path);
        let mut first = true;
        loop {
            let (go, nx) = pool.read(pid, |p| {
                let n = cnt(p);
                let start = if first { leaf_lb(p, key, elen, klen) } else { 0 };
                for i in start..n {
                    let o = HDR + i * elen;
                    if !f(&p[o..o + elen]) {
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

    /// Copies the entry with exactly `key` into `out`; returns false if absent.
    pub fn get(&mut self, pool: &mut Pool, key: &[u8], out: &mut [u8]) -> bool {
        let mut found = false;
        let klen = self.klen;
        self.scan_from(pool, key, |e| {
            if &e[..klen] == key {
                out[..e.len()].copy_from_slice(e);
                found = true;
            }
            false
        });
        found
    }

    /// Modifies the entry with exactly `key` in place (same width).
    pub fn update(&mut self, pool: &mut Pool, key: &[u8], g: impl FnOnce(&mut [u8])) -> bool {
        let (elen, klen) = (self.elen, self.klen);
        let leaf = descend(pool, self.root, self.height, key, klen, &mut self.path);
        let pos = pool.read(leaf, |p| {
            let pos = leaf_lb(p, key, elen, klen);
            if pos < cnt(p) && &p[HDR + pos * elen..HDR + pos * elen + klen] == key { Some(pos) } else { None }
        });
        let Some(pos) = pos else { return false };
        pool.write(leaf, |p| g(&mut p[HDR + pos * elen..HDR + (pos + 1) * elen]));
        true
    }

    /// Bottom-up build from sorted entries (concatenated, `elen` bytes each).
    pub fn bulk(pool: &mut Pool, elen: usize, klen: usize, sorted: &mut dyn Iterator<Item = Vec<u8>>, fill: f64) -> FixedTree {
        let mut t = FixedTree {
            root: NONE,
            height: 1,
            elen,
            klen,
            cap: (PAGE - HDR) / elen,
            pages: 0,
            leaves: 0,
            entries: 0,
            path: Path::new(),
            tmp: Vec::new(),
        };
        let per = ((t.cap as f64 * fill) as usize).max(1);
        let mut level: Vec<(Vec<u8>, u32)> = Vec::new();
        let mut page = vec![0u8; PAGE];
        let mut n = 0usize;
        let mut cur = NONE;
        let flush = |pool: &mut Pool, cur: u32, page: &mut Vec<u8>, n: usize, nx: u32| {
            page[0] = LEAF;
            set_cnt(page, n);
            set_next(page, nx);
            pool.write(cur, |p| p.copy_from_slice(page));
        };
        for e in sorted {
            if cur == NONE || n == per {
                let nx = pool.alloc();
                t.pages += 1;
                t.leaves += 1;
                if cur != NONE {
                    flush(pool, cur, &mut page, n, nx);
                }
                page.fill(0);
                cur = nx;
                n = 0;
                level.push((e[..klen].to_vec(), nx));
            }
            page[HDR + n * elen..HDR + (n + 1) * elen].copy_from_slice(&e);
            n += 1;
            t.entries += 1;
        }
        if cur == NONE {
            return FixedTree::new(pool, elen, klen);
        }
        flush(pool, cur, &mut page, n, NONE);
        let (root, h) = build_inner(pool, level, klen, fill, &mut t.pages);
        t.root = root;
        t.height = h;
        t
    }
}
