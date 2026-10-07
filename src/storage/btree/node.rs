//! Layouts of B+tree pages (TS-05, TS-06): leaves, inner pages and overflow
//! pages.
//!
//! Every tree page starts with the page header ([`page::HEADER_LEN`] bytes)
//! and then these fields, integers little-endian:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 24 | 1 | flags: bit 0 = deleted (TS-05 Underflow) |
//! | 25 | 1 | level: 0 for a leaf, child level + 1 for an inner page |
//! | 26 | 2 | count: entries of a leaf, keys of an inner page |
//! | 28 | 2 | restart points of a leaf; 0 in an inner page |
//! | 30 | 2 | end of the entry stream of a leaf; 0 in an inner page |
//! | 32 | 8 | right sibling (0: none, the right-most page of its level) |
//! | 40 | 2 | high-key length ([`NO_HIGH_KEY`]: none, the upper bound is +∞) |
//! | 42 | 8 | inner page: left-most child; leaf: 0 |
//! | 50 | … | high key, then the page body |
//!
//! **Leaf** (TS-06 compressed leaves, the LevelDB block format, R-LEVELDB
//! `table/block_builder.cc:5-27`). After the high key comes a stream of
//! entries in key order. Entry `i` is a restart point when `i % 16 == 0`:
//! it stores its whole key. Every other entry stores the length of the
//! prefix it shares with the previous key and only the rest. Each entry is
//! `shared` varint, `unshared` varint, `tag` varint, the `unshared` key
//! bytes, then the value: `tag = value_len << 1` for an inline value, whose
//! bytes follow; `tag = value_len << 1 | 1` for a value in an overflow
//! chain, followed by the first overflow page as 8 bytes. The offsets of the
//! restart points are `u16`s at the end of the page, restart `i` at
//! `PAGE_SIZE - 2 * (i + 1)`, so adding a restart point never moves the
//! others.
//!
//! **Inner page** (slotted). After the high key comes one `u16` slot per
//! key, in key order, each the offset of a cell; cells are packed from the
//! end of the page downwards. A cell is the key length (`u16`), the key and
//! the child page (`u64`). With keys `k1 < … < kn` and children
//! `c0 (left-most), c1, …, cn`, child `ci` holds the keys in `[ki, ki+1)`,
//! with `k0` and `kn+1` the page's own bounds.
//!
//! **Overflow page.** After the page header: the next page of the chain
//! (`u64`, 0 at the end) and the bytes held (`u32`), then the bytes.
//!
//! Decoders return `None` on malformed bytes, so a page that passes its
//! checksum but breaks the format is reported as corruption, never a panic.

use std::cmp::Ordering;
use std::ops::Range;

use crate::storage::page::{self, HEADER_LEN, PAGE_SIZE, PageType};

/// Upper bound of a key, in bytes (TS-05).
pub(crate) const MAX_KEY: usize = 1024;
/// Longest value kept inline; a longer one goes to an overflow chain
/// (TS-05: above 2 048 bytes).
pub(crate) const MAX_INLINE: usize = 2048;
/// Entries per restart group (TS-06; LevelDB `block_restart_interval`).
pub(crate) const RESTART_INTERVAL: usize = 16;
/// The "no page" link value. Page 0 is the header page, never a tree page.
pub(crate) const NONE: u64 = 0;
/// High-key length meaning "no high key" (+∞).
const NO_HIGH_KEY: u16 = u16::MAX;
const DELETED: u8 = 1;

const FLAGS: usize = HEADER_LEN;
const LEVEL: usize = 25;
const COUNT: usize = 26;
const RESTARTS: usize = 28;
const DATA_END: usize = 30;
const RIGHT: usize = 32;
const HK_LEN: usize = 40;
const LEFTMOST: usize = 42;
/// First byte after the fixed fields: the high key starts here.
pub(crate) const BODY: usize = 50;

/// Bytes of a page that hold fixed fields, high key excluded.
pub(crate) const USABLE: usize = PAGE_SIZE - BODY;

/// Overflow-page fields.
const OVF_NEXT: usize = HEADER_LEN;
const OVF_LEN: usize = HEADER_LEN + 8;
const OVF_DATA: usize = HEADER_LEN + 12;
/// Value bytes one overflow page holds.
pub(crate) const OVERFLOW_CAP: usize = PAGE_SIZE - OVF_DATA;

fn u16_at(p: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([p[at], p[at + 1]])
}

fn put_u16(p: &mut [u8], at: usize, v: u16) {
    p[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn u64_at(p: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(p[at..at + 8].try_into().expect("8 bytes"))
}

fn put_u64(p: &mut [u8], at: usize, v: u64) {
    p[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Varints (LEB128, LevelDB `util/coding.cc`).

/// Appends `v` as a varint.
pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Bytes of `v` as a varint.
pub(crate) fn varint_len(v: u64) -> usize {
    (64 - (v | 1).leading_zeros() as usize).div_ceil(7)
}

/// Reads a varint at `*at`, advancing it. `None` if it runs past `end` or
/// exceeds 64 bits.
fn get_varint(p: &[u8], at: &mut usize, end: usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        if *at >= end || shift > 63 {
            return None;
        }
        let b = p[*at];
        *at += 1;
        v |= u64::from(b & 0x7F) << shift;
        if b < 0x80 {
            return Some(v);
        }
        shift += 7;
    }
}

// ---------------------------------------------------------------------------
// Fields common to leaves and inner pages.

/// Whether the page is marked deleted (TS-05 Underflow).
pub(crate) fn is_deleted(p: &[u8]) -> bool {
    p[FLAGS] & DELETED != 0
}

/// Level: 0 for a leaf.
pub(crate) fn level(p: &[u8]) -> u8 {
    p[LEVEL]
}

/// Entries of a leaf, or keys of an inner page.
pub(crate) fn count(p: &[u8]) -> usize {
    usize::from(u16_at(p, COUNT))
}

/// The right sibling, or [`NONE`].
pub(crate) fn right(p: &[u8]) -> u64 {
    u64_at(p, RIGHT)
}

/// Sets the right sibling.
pub(crate) fn set_right(p: &mut [u8], r: u64) {
    put_u64(p, RIGHT, r);
}

/// The high key, or `None` for +∞. `None` also for a malformed length;
/// [`body_start`] reports that case.
pub(crate) fn high_key(p: &[u8]) -> Option<&[u8]> {
    let n = u16_at(p, HK_LEN);
    if n == NO_HIGH_KEY || usize::from(n) > MAX_KEY {
        return None;
    }
    Some(&p[BODY..BODY + usize::from(n)])
}

/// Offset where the body starts, after the high key, or `None` if the
/// stored length is malformed.
pub(crate) fn body_start(p: &[u8]) -> Option<usize> {
    let n = u16_at(p, HK_LEN);
    if n == NO_HIGH_KEY {
        Some(BODY)
    } else if usize::from(n) <= MAX_KEY {
        Some(BODY + usize::from(n))
    } else {
        None
    }
}

/// Whether `key` lies beyond the page's high key, so a reader must move
/// right (Lehman and Yao).
pub(crate) fn beyond_high_key(p: &[u8], key: &[u8]) -> bool {
    high_key(p).is_some_and(|hk| key >= hk)
}

/// Marks the page deleted and empties it, keeping its right link and high
/// key, so a reader that reaches it moves right (TS-05 Underflow).
pub(crate) fn mark_deleted(p: &mut [u8]) {
    p[FLAGS] |= DELETED;
    put_u16(p, COUNT, 0);
    put_u16(p, RESTARTS, 0);
    let start = body_start(p).unwrap_or(BODY);
    put_u16(p, DATA_END, start as u16);
}

/// Writes the fixed fields and high key of a tree page after `page::init`.
fn init_tree_page(
    p: &mut [u8],
    page_type: PageType,
    tree: u64,
    level: u8,
    hk: Option<&[u8]>,
    right: u64,
) -> usize {
    page::init(p, page_type, tree);
    p[LEVEL] = level;
    put_u64(p, RIGHT, right);
    match hk {
        None => {
            put_u16(p, HK_LEN, NO_HIGH_KEY);
            BODY
        }
        Some(k) => {
            debug_assert!(k.len() <= MAX_KEY);
            put_u16(p, HK_LEN, k.len() as u16);
            p[BODY..BODY + k.len()].copy_from_slice(k);
            BODY + k.len()
        }
    }
}

/// Writes an empty root leaf of `tree`: no high key, no sibling.
pub(crate) fn init_empty_leaf(p: &mut [u8], tree: u64) {
    let start = init_tree_page(p, PageType::Leaf, tree, 0, None, NONE);
    put_u16(p, DATA_END, start as u16);
}

// ---------------------------------------------------------------------------
// Values.

/// A value as stored in a leaf entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Value<'a> {
    /// Bytes stored in the entry.
    Inline(&'a [u8]),
    /// A value of `len` bytes in the overflow chain starting at `first`.
    Overflow {
        /// Value length in bytes.
        len: u64,
        /// First overflow page.
        first: u64,
    },
}

impl Value<'_> {
    /// Bytes the value takes in an entry: the tag and the inline bytes or
    /// the page number.
    fn encoded_len(&self) -> usize {
        match *self {
            Value::Inline(v) => varint_len((v.len() as u64) << 1) + v.len(),
            Value::Overflow { len, .. } => varint_len((len << 1) | 1) + 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Leaf reading.

/// Header of a leaf, read once per operation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeafView {
    start: usize,
    end: usize,
    count: usize,
    restarts: usize,
}

/// Validates the layout fields of a leaf.
pub(crate) fn leaf_view(p: &[u8]) -> Option<LeafView> {
    let start = body_start(p)?;
    let count = count(p);
    let restarts = usize::from(u16_at(p, RESTARTS));
    let end = usize::from(u16_at(p, DATA_END));
    if restarts != count.div_ceil(RESTART_INTERVAL) || end < start || end + 2 * restarts > PAGE_SIZE
    {
        return None;
    }
    Some(LeafView {
        start,
        end,
        count,
        restarts,
    })
}

impl LeafView {
    /// Entries in the leaf.
    pub(crate) fn count(&self) -> usize {
        self.count
    }

    fn restart(&self, p: &[u8], i: usize) -> Option<usize> {
        let at = usize::from(u16_at(p, PAGE_SIZE - 2 * (i + 1)));
        (at >= self.start && at < self.end).then_some(at)
    }

    /// Bytes used by the leaf body: entries and restart array.
    pub(crate) fn used_bytes(&self) -> usize {
        self.end - self.start + 2 * self.restarts
    }
}

/// One decoded entry header.
struct RawEntry {
    shared: usize,
    key: Range<usize>,
    value: RawValue,
    next: usize,
}

#[derive(Clone, Copy)]
enum RawValue {
    Inline(usize, usize),
    Overflow { len: u64, first: u64 },
}

/// Decodes the entry at `at`, with the limit `end`.
fn raw_entry(p: &[u8], at: usize, end: usize) -> Option<RawEntry> {
    let mut o = at;
    // LevelDB's fast path: all three varints in one byte each.
    let (shared, unshared, tag) = if o + 3 <= end && (p[o] | p[o + 1] | p[o + 2]) < 0x80 {
        o += 3;
        (
            usize::from(p[o - 3]),
            usize::from(p[o - 2]),
            u64::from(p[o - 1]),
        )
    } else {
        let s = get_varint(p, &mut o, end)?;
        let u = get_varint(p, &mut o, end)?;
        let t = get_varint(p, &mut o, end)?;
        (usize::try_from(s).ok()?, usize::try_from(u).ok()?, t)
    };
    if shared + unshared > MAX_KEY || unshared > end - o {
        return None;
    }
    let key = o..o + unshared;
    o += unshared;
    let value = if tag & 1 == 0 {
        let len = usize::try_from(tag >> 1).ok()?;
        if len > MAX_INLINE || len > end - o {
            return None;
        }
        o += len;
        RawValue::Inline(o - len, len)
    } else {
        if end - o < 8 {
            return None;
        }
        let first = u64_at(p, o);
        o += 8;
        RawValue::Overflow {
            len: tag >> 1,
            first,
        }
    };
    Some(RawEntry {
        shared,
        key,
        value,
        next: o,
    })
}

fn value_of(p: &[u8], v: RawValue) -> Value<'_> {
    match v {
        RawValue::Inline(at, len) => Value::Inline(&p[at..at + len]),
        RawValue::Overflow { len, first } => Value::Overflow { len, first },
    }
}

/// Index of the restart group that may hold `key`: the last restart point
/// whose key is at most `key`, or 0. Binary search over the restart array
/// comparing the full keys of restart entries in place, as LevelDB's
/// `Block::Iter::Seek` (R-LEVELDB `table/block.cc:164-210`).
fn find_group(p: &[u8], v: &LeafView, key: &[u8]) -> Option<usize> {
    let (mut lo, mut hi) = (0, v.restarts);
    // Invariant: groups < lo start at or below `key`; groups >= hi above it.
    while lo < hi {
        let mid = (lo + hi) / 2;
        let at = v.restart(p, mid)?;
        let e = raw_entry(p, at, v.end)?;
        if e.shared != 0 {
            return None;
        }
        if &p[e.key] <= key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Some(lo.saturating_sub(1))
}

/// Result of a leaf search.
pub(crate) enum Found<'a> {
    /// The key is present with this value.
    Hit(Value<'a>),
    /// The key is absent.
    Miss,
}

/// Looks `key` up in a leaf. `buf` is scratch for key reconstruction.
pub(crate) fn leaf_get<'a>(p: &'a [u8], key: &[u8], buf: &mut Vec<u8>) -> Option<Found<'a>> {
    let v = leaf_view(p)?;
    if v.count == 0 {
        return Some(Found::Miss);
    }
    let g = find_group(p, &v, key)?;
    let mut at = v.restart(p, g)?;
    buf.clear();
    let last = (g + 1) * RESTART_INTERVAL;
    for _ in g * RESTART_INTERVAL..last.min(v.count) {
        let e = raw_entry(p, at, v.end)?;
        if e.shared > buf.len() {
            return None;
        }
        buf.truncate(e.shared);
        buf.extend_from_slice(&p[e.key]);
        match buf.as_slice().cmp(key) {
            Ordering::Less => at = e.next,
            Ordering::Equal => return Some(Found::Hit(value_of(p, e.value))),
            Ordering::Greater => return Some(Found::Miss),
        }
    }
    Some(Found::Miss)
}

/// Calls `f` with each entry of the leaf whose key is at least `from`, in
/// order, until `f` returns `false`. Returns `None` on a malformed leaf.
pub(crate) fn leaf_for_each_from<'a>(
    p: &'a [u8],
    from: &[u8],
    buf: &mut Vec<u8>,
    mut f: impl FnMut(&[u8], Value<'a>) -> bool,
) -> Option<()> {
    let v = leaf_view(p)?;
    if v.count == 0 {
        return Some(());
    }
    let g = find_group(p, &v, from)?;
    let mut at = v.restart(p, g)?;
    buf.clear();
    for _ in g * RESTART_INTERVAL..v.count {
        let e = raw_entry(p, at, v.end)?;
        if e.shared > buf.len() {
            return None;
        }
        buf.truncate(e.shared);
        buf.extend_from_slice(&p[e.key]);
        at = e.next;
        if buf.as_slice() >= from && !f(buf, value_of(p, e.value)) {
            return Some(());
        }
    }
    Some(())
}

// ---------------------------------------------------------------------------
// Decoded entries, for rewriting leaves.

/// Decoded leaf entries: keys and inline values in one arena, so decoding
/// allocates nothing once the buffers have grown.
#[derive(Debug, Default)]
pub(crate) struct Entries {
    bytes: Vec<u8>,
    items: Vec<Item>,
    /// Scratch for key reconstruction while decoding.
    key: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Item {
    key: Range<u32>,
    value: ItemValue,
    /// Length of the prefix shared with the previous entry, when known
    /// from decoding ([`UNKNOWN`] otherwise), so re-encoding an unchanged
    /// neighbour needs no comparison.
    shared: u16,
}

/// [`Item::shared`] value of an entry whose shared prefix is not known.
const UNKNOWN: u16 = u16::MAX;

#[derive(Debug, Clone)]
enum ItemValue {
    Inline(Range<u32>),
    Overflow { len: u64, first: u64 },
}

impl Entries {
    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.items.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    fn store(&mut self, b: &[u8]) -> Range<u32> {
        let at = self.bytes.len() as u32;
        self.bytes.extend_from_slice(b);
        at..self.bytes.len() as u32
    }

    fn item(&mut self, key: &[u8], value: Value<'_>) -> Item {
        let key = self.store(key);
        let value = match value {
            Value::Inline(v) => ItemValue::Inline(self.store(v)),
            Value::Overflow { len, first } => ItemValue::Overflow { len, first },
        };
        Item {
            key,
            value,
            shared: UNKNOWN,
        }
    }

    /// Appends an entry.
    pub(crate) fn push(&mut self, key: &[u8], value: Value<'_>) {
        let it = self.item(key, value);
        self.items.push(it);
    }

    /// Appends an entry that shares `shared` bytes with the previous one.
    fn push_shared(&mut self, key: &[u8], value: Value<'_>, shared: usize) {
        let mut it = self.item(key, value);
        it.shared = shared as u16;
        self.items.push(it);
    }

    /// Inserts an entry at position `i`.
    pub(crate) fn insert(&mut self, i: usize, key: &[u8], value: Value<'_>) {
        let it = self.item(key, value);
        self.items.insert(i, it);
        self.forget_shared(i + 1);
    }

    /// Entry `i` follows a different entry now.
    fn forget_shared(&mut self, i: usize) {
        if let Some(it) = self.items.get_mut(i) {
            it.shared = UNKNOWN;
        }
    }

    /// Length of the prefix entry `i` shares with entry `i - 1`.
    fn shared(&self, i: usize) -> usize {
        match self.items[i].shared {
            UNKNOWN => common_prefix(self.key(i - 1), self.key(i)),
            n => usize::from(n),
        }
    }

    /// Replaces the value of entry `i`.
    pub(crate) fn set_value(&mut self, i: usize, value: Value<'_>) {
        let it = self.item(&[], value);
        self.items[i].value = it.value;
    }

    /// Removes entry `i`.
    pub(crate) fn remove(&mut self, i: usize) {
        self.items.remove(i);
        self.forget_shared(i);
    }

    /// Key of entry `i`.
    pub(crate) fn key(&self, i: usize) -> &[u8] {
        let r = &self.items[i].key;
        &self.bytes[r.start as usize..r.end as usize]
    }

    /// Value of entry `i`.
    pub(crate) fn value(&self, i: usize) -> Value<'_> {
        match &self.items[i].value {
            ItemValue::Inline(r) => Value::Inline(&self.bytes[r.start as usize..r.end as usize]),
            ItemValue::Overflow { len, first } => Value::Overflow {
                len: *len,
                first: *first,
            },
        }
    }

    /// First position whose key is not below `key`, and whether it equals
    /// `key`.
    pub(crate) fn search(&self, key: &[u8]) -> (usize, bool) {
        let i = self
            .items
            .partition_point(|it| &self.bytes[it.key.start as usize..it.key.end as usize] < key);
        (i, i < self.items.len() && self.key(i) == key)
    }

    /// Bytes entry `i` takes when encoded after entry `i - 1`, as a
    /// restart point if `restart`.
    pub(crate) fn encoded_len(&self, i: usize, restart: bool) -> usize {
        let key = self.key(i);
        let shared = if restart { 0 } else { self.shared(i) };
        varint_len(shared as u64)
            + varint_len((key.len() - shared) as u64)
            + (key.len() - shared)
            + self.value(i).encoded_len()
    }

    /// Encoded body bytes (entries and restart array) of entries
    /// `range` laid out as one leaf whose first entry is a restart point.
    pub(crate) fn body_len(&self, range: Range<usize>) -> usize {
        let n = range.len();
        let mut bytes = 2 * n.div_ceil(RESTART_INTERVAL);
        for (k, i) in range.enumerate() {
            bytes += self.encoded_len(i, k % RESTART_INTERVAL == 0);
        }
        bytes
    }
}

/// Length of the common prefix of `a` and `b`.
///
/// Compares eight bytes at a time; the first differing byte of a
/// mismatching word is found from the XOR's trailing zeros (bytes are
/// loaded little-endian). Measured: the byte-wise iterator took 36 % of
/// `put` time (raw/t14-perf-put-before.folded).
pub(crate) fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 8 <= n {
        let x = u64::from_le_bytes(a[i..i + 8].try_into().expect("8 bytes"));
        let y = u64::from_le_bytes(b[i..i + 8].try_into().expect("8 bytes"));
        if x != y {
            return i + ((x ^ y).trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Decodes entries `first_group * 16 ..` of a leaf into `out`, which is
/// cleared first.
pub(crate) fn leaf_decode(p: &[u8], first_group: usize, out: &mut Entries) -> Option<()> {
    out.clear();
    let v = leaf_view(p)?;
    if v.count == 0 {
        return Some(());
    }
    let mut at = v.restart(p, first_group)?;
    let mut key = std::mem::take(&mut out.key);
    key.clear();
    let mut ok = true;
    for _ in first_group * RESTART_INTERVAL..v.count {
        let Some(e) = raw_entry(p, at, v.end).filter(|e| e.shared <= key.len()) else {
            ok = false;
            break;
        };
        key.truncate(e.shared);
        key.extend_from_slice(&p[e.key]);
        // A restart point stores no shared length: it stays unknown.
        if out.len().is_multiple_of(RESTART_INTERVAL) {
            out.push(&key, value_of(p, e.value));
        } else {
            out.push_shared(&key, value_of(p, e.value), e.shared);
        }
        at = e.next;
    }
    out.key = key;
    (ok && at == v.end).then_some(())
}

/// Scratch buffers for encoding a leaf.
#[derive(Debug, Default)]
pub(crate) struct Encoder {
    bytes: Vec<u8>,
    offsets: Vec<u16>,
    /// Keys reconstructed by [`leaf_apply`]: the current entry's, the next
    /// one's, and the key a re-encoded entry is delta-encoded against.
    cur: Vec<u8>,
    next: Vec<u8>,
    prev: Vec<u8>,
}

/// Appends an entry with key `key` and value `value`, sharing `shared` bytes
/// with the previous key.
fn encode_parts(out: &mut Vec<u8>, key: &[u8], shared: usize, value: Value<'_>) {
    put_varint(out, shared as u64);
    put_varint(out, (key.len() - shared) as u64);
    match value {
        Value::Inline(v) => {
            put_varint(out, (v.len() as u64) << 1);
            out.extend_from_slice(&key[shared..]);
            out.extend_from_slice(v);
        }
        Value::Overflow { len, first } => {
            put_varint(out, (len << 1) | 1);
            out.extend_from_slice(&key[shared..]);
            out.extend_from_slice(&first.to_le_bytes());
        }
    }
}

/// Appends entry `i` of `es` to `out`.
fn encode_entry(out: &mut Vec<u8>, es: &Entries, i: usize, restart: bool) {
    let shared = if restart { 0 } else { es.shared(i) };
    encode_parts(out, es.key(i), shared, es.value(i));
}

/// Writes `bytes` as the leaf's entry stream from offset `base` to its end,
/// and `offsets` as restart points `first_restart..`, for a leaf of `count`
/// entries. Returns `false`, leaving the page unchanged, if they do not fit.
fn commit_tail(
    p: &mut [u8],
    base: usize,
    first_restart: usize,
    bytes: &[u8],
    offsets: &[u16],
    count: usize,
) -> bool {
    let restarts = count.div_ceil(RESTART_INTERVAL);
    debug_assert_eq!(first_restart + offsets.len(), restarts);
    let end = base + bytes.len();
    if end + 2 * restarts > PAGE_SIZE {
        return false;
    }
    p[base..end].copy_from_slice(bytes);
    for (k, off) in offsets.iter().enumerate() {
        put_u16(p, PAGE_SIZE - 2 * (first_restart + k + 1), *off);
    }
    // Zero the free gap, so stale bytes never reach disk.
    p[end..PAGE_SIZE - 2 * restarts].fill(0);
    put_u16(p, COUNT, count as u16);
    put_u16(p, RESTARTS, restarts as u16);
    put_u16(p, DATA_END, end as u16);
    true
}

/// Writes entries `range` of `es` as the leaf entries starting with restart
/// group `first_group`, keeping groups before it. `enc` is scratch. Returns
/// `false`, leaving the page unchanged, if they do not fit.
fn write_entries(
    p: &mut [u8],
    first_group: usize,
    es: &Entries,
    range: Range<usize>,
    enc: &mut Encoder,
) -> bool {
    let start = body_start(p).expect("valid high key");
    let base = if first_group == 0 {
        start
    } else {
        usize::from(u16_at(p, PAGE_SIZE - 2 * (first_group + 1)))
    };
    let Encoder { bytes, offsets, .. } = enc;
    bytes.clear();
    offsets.clear();
    for (k, i) in range.clone().enumerate() {
        let restart = k % RESTART_INTERVAL == 0;
        if restart {
            offsets.push((base + bytes.len()) as u16);
        }
        encode_entry(bytes, es, i, restart);
    }
    let count = first_group * RESTART_INTERVAL + range.len();
    commit_tail(p, base, first_group, bytes, offsets, count)
}

/// Where a tree page sits: its tree, level, high key and right sibling.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Place<'a> {
    /// Tree identifier.
    pub(crate) tree: u64,
    /// Level: 0 for a leaf.
    pub(crate) level: u8,
    /// High key, `None` for +∞.
    pub(crate) high_key: Option<&'a [u8]>,
    /// Right sibling, or [`NONE`].
    pub(crate) right: u64,
}

/// Writes a whole leaf from `es[range]`. Returns `false` if it does not fit.
pub(crate) fn write_leaf(
    p: &mut [u8],
    at: Place<'_>,
    es: &Entries,
    range: Range<usize>,
    enc: &mut Encoder,
) -> bool {
    debug_assert_eq!(at.level, 0);
    init_tree_page(p, PageType::Leaf, at.tree, 0, at.high_key, at.right);
    write_entries(p, 0, es, range, enc)
}

/// A change to one key of a leaf.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Change<'a> {
    /// Insert or replace the value.
    Put(Value<'a>),
    /// Remove the key.
    Delete,
}

/// Outcome of [`leaf_apply`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Applied {
    /// The leaf was changed. Holds the replaced or removed value's overflow
    /// chain, if any, which the caller frees.
    Done(Option<u64>),
    /// A delete found no such key; the leaf is unchanged.
    NotFound,
    /// The change does not fit; the leaf is unchanged.
    Full,
}

/// The overflow chain a value holds, if any.
fn chain_of(v: Value<'_>) -> Option<u64> {
    match v {
        Value::Overflow { first, .. } => Some(first),
        Value::Inline(_) => None,
    }
}

/// Applies `change` for `key` to a leaf in place.
///
/// An insert or delete shifts every later entry by one position, so every
/// later restart point moves by one entry (TS-06 keeps every 16th entry a
/// restart point). Only the entries whose encoding changes are re-encoded:
/// the changed entry, the entry after it (its predecessor changed), and at
/// each later group boundary the entry that becomes a restart point (it now
/// stores its whole key) and the one that stops being one (it now stores a
/// delta). Every other entry keeps its predecessor and its restart status,
/// so its bytes stay valid and are copied in runs. The page bytes equal
/// those of decoding and re-encoding the tail (test
/// `apply_matches_the_full_reencode`).
///
/// PERF: the full decode and re-encode took 85 % of `put`
/// (raw/t14-perf-put.folded); see the report, `## #14`, "Put optimisation".
pub(crate) fn leaf_apply(
    p: &mut [u8],
    key: &[u8],
    change: Change<'_>,
    enc: &mut Encoder,
) -> Option<Applied> {
    let v = leaf_view(p)?;
    let (g, base) = if v.count == 0 {
        (0, v.start)
    } else {
        let g = find_group(p, &v, key)?;
        (g, v.restart(p, g)?)
    };
    let Encoder {
        bytes,
        offsets,
        cur,
        next,
        prev,
    } = enc;
    bytes.clear();
    offsets.clear();
    cur.clear();
    // Position `i` of `key`: the first entry of group `g` on not below it.
    // `cur` ends as the key of entry `i - 1` (empty if `i` starts group
    // `g`), `next` as the key of entry `i`.
    let (mut k, mut at) = (g * RESTART_INTERVAL, base);
    let mut found = None;
    while k < v.count {
        let e = raw_entry(p, at, v.end)?;
        if e.shared > cur.len() {
            return None;
        }
        next.clear();
        next.extend_from_slice(&cur[..e.shared]);
        next.extend_from_slice(&p[e.key.clone()]);
        if next.as_slice() >= key {
            found = Some(e);
            break;
        }
        std::mem::swap(cur, next);
        at = e.next;
        k += 1;
    }
    let (i, at_i) = (k, at);
    let exists = found.is_some() && next.as_slice() == key;
    let restart = |j: usize| j.is_multiple_of(RESTART_INTERVAL);
    // Old entry `k` from `k` on becomes entry `k + 1` (insert), `k - 1`
    // (delete) or stays `k`. `run` starts the old bytes not yet copied;
    // `pred_changed`: the next walked entry follows a different entry now,
    // whose key `prev` holds.
    let (up, down);
    let mut run;
    let mut pred_changed;
    let freed = match (change, found) {
        (Change::Put(value), Some(e)) if exists => {
            if restart(i) {
                offsets.push(at_i as u16);
            }
            let shared = if restart(i) { 0 } else { e.shared };
            encode_parts(bytes, next, shared, value);
            std::mem::swap(cur, next);
            (up, down, pred_changed) = (false, false, false);
            (k, at, run) = (i + 1, e.next, e.next);
            chain_of(value_of(p, e.value))
        }
        (Change::Put(value), _) => {
            if restart(i) {
                offsets.push(at_i as u16);
            }
            let shared = if restart(i) {
                0
            } else {
                common_prefix(cur, key)
            };
            encode_parts(bytes, key, shared, value);
            prev.clear();
            prev.extend_from_slice(key);
            (up, down, pred_changed) = (true, false, true);
            run = at_i;
            None
        }
        (Change::Delete, Some(e)) if exists => {
            prev.clear();
            prev.extend_from_slice(cur);
            std::mem::swap(cur, next);
            (up, down, pred_changed) = (false, true, true);
            (k, at, run) = (i + 1, e.next, e.next);
            chain_of(value_of(p, e.value))
        }
        (Change::Delete, _) => return Some(Applied::NotFound),
    };
    // `cur` holds the key of old entry `k - 1`.
    while k < v.count {
        let e = raw_entry(p, at, v.end)?;
        if e.shared > cur.len() {
            return None;
        }
        let nk = if up {
            k + 1
        } else if down {
            k - 1
        } else {
            k
        };
        if restart(nk) {
            offsets.push((at_i + bytes.len() + (at - run)) as u16);
        }
        if restart(nk) == restart(k) && (restart(nk) || !pred_changed) {
            // Same predecessor (or none) and restart status: same bytes.
            cur.truncate(e.shared);
            cur.extend_from_slice(&p[e.key]);
        } else {
            bytes.extend_from_slice(&p[run..at]);
            if !restart(nk) && !pred_changed {
                prev.clear();
                prev.extend_from_slice(cur);
            }
            cur.truncate(e.shared);
            cur.extend_from_slice(&p[e.key.clone()]);
            let shared = if restart(nk) {
                0
            } else {
                common_prefix(prev, cur)
            };
            encode_parts(bytes, cur, shared, value_of(p, e.value));
            run = e.next;
        }
        pred_changed = false;
        at = e.next;
        k += 1;
    }
    if at != v.end {
        return None;
    }
    bytes.extend_from_slice(&p[run..v.end]);
    let count = if up {
        v.count + 1
    } else if down {
        v.count - 1
    } else {
        v.count
    };
    let first_restart = i.div_ceil(RESTART_INTERVAL);
    Some(
        if commit_tail(p, at_i, first_restart, bytes, offsets, count) {
            Applied::Done(freed)
        } else {
            Applied::Full
        },
    )
}

/// [`leaf_apply`] by decoding and re-encoding the tail of the leaf from the
/// key's restart group: the reference its bytes are tested against.
#[cfg(test)]
fn leaf_apply_reencode(
    p: &mut [u8],
    key: &[u8],
    change: Change<'_>,
    es: &mut Entries,
    enc: &mut Encoder,
) -> Option<Applied> {
    let v = leaf_view(p)?;
    let g = if v.count == 0 {
        0
    } else {
        find_group(p, &v, key)?
    };
    leaf_decode(p, g, es)?;
    let (i, exists) = es.search(key);
    let old = |es: &Entries| match es.value(i) {
        Value::Overflow { first, .. } => Some(first),
        Value::Inline(_) => None,
    };
    let freed = match change {
        Change::Put(value) => {
            if exists {
                let o = old(es);
                es.set_value(i, value);
                o
            } else {
                es.insert(i, key, value);
                None
            }
        }
        Change::Delete => {
            if !exists {
                return Some(Applied::NotFound);
            }
            let o = old(es);
            es.remove(i);
            o
        }
    };
    let n = es.len();
    Some(if write_entries(p, g, es, 0..n, enc) {
        Applied::Done(freed)
    } else {
        Applied::Full
    })
}

/// Decodes the whole leaf into `es` and applies `change` there, for a
/// split. Returns the replaced or removed value's overflow chain, if any.
///
/// A delete can need a split too: removing an entry moves every later
/// restart point by one entry, and the entry that becomes a restart point
/// stores its whole key, so the leaf can grow.
pub(crate) fn leaf_decode_with(
    p: &[u8],
    key: &[u8],
    change: Change<'_>,
    es: &mut Entries,
) -> Option<Option<u64>> {
    leaf_decode(p, 0, es)?;
    let (i, exists) = es.search(key);
    let old = |es: &Entries| match es.value(i) {
        Value::Overflow { first, .. } => Some(first),
        Value::Inline(_) => None,
    };
    Some(match change {
        Change::Put(value) if exists => {
            let o = old(es);
            es.set_value(i, value);
            o
        }
        Change::Put(value) => {
            es.insert(i, key, value);
            None
        }
        Change::Delete => {
            if !exists {
                return None;
            }
            let o = old(es);
            es.remove(i);
            o
        }
    })
}

/// The shortest separator `s` with `left < s <= right`: the prefix of
/// `right` one byte past the common prefix (suffix truncation: PostgreSQL
/// `_bt_truncate`, R-PG `src/backend/access/nbtree/nbtutils.c`; LevelDB
/// `FindShortestSeparator`).
pub(crate) fn separator<'a>(left: &[u8], right: &'a [u8]) -> &'a [u8] {
    debug_assert!(left < right);
    &right[..(common_prefix(left, right) + 1).min(right.len())]
}

/// Fraction of a leaf the left page keeps when the right-most leaf splits
/// (TS-06: 90 %; PostgreSQL `BTREE_DEFAULT_FILLFACTOR`).
pub(crate) const RIGHTMOST_FILL: f64 = 0.90;

/// Chooses where a leaf of entries `es` splits: the left page takes
/// `0..s`. `rightmost`: the leaf is the right-most of its level, so the
/// left page is left 90 % full (TS-06). Otherwise the bytes are balanced.
/// Both pages fit, with the high keys they get. Returns `None` if no split
/// point fits.
pub(crate) fn leaf_split_point(es: &Entries, rightmost: bool, old_hk_len: usize) -> Option<usize> {
    let n = es.len();
    if n < 2 {
        return None;
    }
    // Encoded sizes as one stream from entry 0: exact for every left part.
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0usize);
    for i in 0..n {
        let b = es.encoded_len(i, i % RESTART_INTERVAL == 0);
        prefix.push(prefix[i] + b);
    }
    let left_bytes = |s: usize| {
        prefix[s] + 2 * s.div_ceil(RESTART_INTERVAL) + separator(es.key(s - 1), es.key(s)).len()
    };
    let right_fits = |s: usize| es.body_len(s..n) + old_hk_len <= USABLE;
    let fits = |s: usize| left_bytes(s) <= USABLE && right_fits(s);
    let total = prefix[n] + 2 * n.div_ceil(RESTART_INTERVAL);
    let target = if rightmost {
        (USABLE as f64 * RIGHTMOST_FILL) as usize
    } else {
        total / 2
    };
    // The last split point whose left page stays within the target.
    let mut s0 = (1..n).rev().find(|&s| left_bytes(s) <= target).unwrap_or(1);
    // Balanced split: take one more entry if that is closer to half.
    if !rightmost
        && s0 + 1 < n
        && left_bytes(s0 + 1).abs_diff(target) < left_bytes(s0).abs_diff(target)
    {
        s0 += 1;
    }
    // The nearest split point that fits, s0 first.
    let candidates = (0..n).flat_map(|d| [s0.checked_sub(d), s0.checked_add(d + 1)]);
    candidates
        .flatten()
        .filter(|&s| s >= 1 && s < n)
        .take(2 * n)
        .find(|&s| fits(s))
}

// ---------------------------------------------------------------------------
// Inner pages.

/// Decoded keys and children of an inner page.
#[derive(Debug, Default)]
pub(crate) struct Pivots {
    bytes: Vec<u8>,
    keys: Vec<Range<u32>>,
    /// Children: `children[0]` is the left-most, `children[i + 1]` follows
    /// key `i`.
    children: Vec<u64>,
}

impl Pivots {
    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.keys.clear();
        self.children.clear();
    }

    /// Keys.
    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }

    /// Key `i`.
    pub(crate) fn key(&self, i: usize) -> &[u8] {
        let r = &self.keys[i];
        &self.bytes[r.start as usize..r.end as usize]
    }

    /// Child `i` (0 is the left-most).
    pub(crate) fn child(&self, i: usize) -> u64 {
        self.children[i]
    }

    /// Children.
    pub(crate) fn children(&self) -> &[u64] {
        &self.children
    }

    /// Starts with only a left-most child.
    pub(crate) fn reset(&mut self, leftmost: u64) {
        self.clear();
        self.children.push(leftmost);
    }

    /// Appends a key and the child after it.
    pub(crate) fn push(&mut self, key: &[u8], child: u64) {
        let at = self.bytes.len() as u32;
        self.bytes.extend_from_slice(key);
        self.keys.push(at..self.bytes.len() as u32);
        self.children.push(child);
    }

    /// Inserts key `key` and child `child` after child `i`.
    pub(crate) fn insert_after_child(&mut self, i: usize, key: &[u8], child: u64) {
        let at = self.bytes.len() as u32;
        self.bytes.extend_from_slice(key);
        self.keys.insert(i, at..self.bytes.len() as u32);
        self.children.insert(i + 1, child);
    }

    /// Removes child `i` and gives its key range to child `i + 1`: the key
    /// that starts child `i + 1` is removed, and child `i + 1` moves into
    /// position `i` (PostgreSQL page deletion, R-PG
    /// `src/backend/access/nbtree/README` "Deleting entire pages").
    pub(crate) fn remove_child_into_right(&mut self, i: usize) {
        self.keys.remove(i);
        self.children.remove(i);
    }

    /// Bytes of the body for keys `range` (slots and cells).
    fn body_len(&self, range: Range<usize>) -> usize {
        range.map(|i| 2 + 2 + self.key(i).len() + 8).sum()
    }
}

/// Decodes an inner page into `out`.
pub(crate) fn inner_decode(p: &[u8], out: &mut Pivots) -> Option<()> {
    out.reset(u64_at(p, LEFTMOST));
    let start = body_start(p)?;
    let n = count(p);
    if start + 2 * n > PAGE_SIZE {
        return None;
    }
    for i in 0..n {
        let (k, child) = inner_cell(p, start, i)?;
        out.push(k, child);
    }
    Some(())
}

/// Key and child of slot `i`.
fn inner_cell(p: &[u8], start: usize, i: usize) -> Option<(&[u8], u64)> {
    let at = usize::from(u16_at(p, start + 2 * i));
    if at + 2 > PAGE_SIZE {
        return None;
    }
    let len = usize::from(u16_at(p, at));
    if len > MAX_KEY || at + 2 + len + 8 > PAGE_SIZE {
        return None;
    }
    Some((&p[at + 2..at + 2 + len], u64_at(p, at + 2 + len)))
}

/// The child of an inner page to follow for `key`, and its position
/// (0 = left-most): the child after the last key at most `key`.
pub(crate) fn inner_child(p: &[u8], key: &[u8]) -> Option<(usize, u64)> {
    let start = body_start(p)?;
    let n = count(p);
    if start + 2 * n > PAGE_SIZE {
        return None;
    }
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let (k, _) = inner_cell(p, start, mid)?;
        if k <= key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        Some((0, u64_at(p, LEFTMOST)))
    } else {
        Some((lo, inner_cell(p, start, lo - 1)?.1))
    }
}

/// Writes an inner page from keys `range` of `pv`, with left-most child
/// `leftmost`. Returns `false` if it does not fit.
pub(crate) fn write_inner(
    p: &mut [u8],
    at: Place<'_>,
    leftmost: u64,
    pv: &Pivots,
    range: Range<usize>,
) -> bool {
    let start = BODY + at.high_key.map_or(0, <[u8]>::len);
    if start + pv.body_len(range.clone()) > PAGE_SIZE {
        return false;
    }
    init_tree_page(p, PageType::Inner, at.tree, at.level, at.high_key, at.right);
    put_u64(p, LEFTMOST, leftmost);
    put_u16(p, COUNT, range.len() as u16);
    let mut heap = PAGE_SIZE;
    for (slot, i) in range.enumerate() {
        let k = pv.key(i);
        heap -= 2 + k.len() + 8;
        put_u16(p, heap, k.len() as u16);
        p[heap + 2..heap + 2 + k.len()].copy_from_slice(k);
        put_u64(p, heap + 2 + k.len(), pv.child(i + 1));
        put_u16(p, start + 2 * slot, heap as u16);
    }
    true
}

/// Chooses the key of an overfull inner page that moves up: the left page
/// keeps keys `0..m`, the right page gets child `m + 1` as left-most and
/// keys `m + 1..`. Balances bytes; both pages fit. `None` if no key fits.
pub(crate) fn inner_split_point(pv: &Pivots, old_hk_len: usize) -> Option<usize> {
    let n = pv.len();
    if n < 3 {
        return None;
    }
    let total = pv.body_len(0..n);
    let fits = |m: usize| {
        BODY + pv.key(m).len() + pv.body_len(0..m) <= PAGE_SIZE
            && BODY + old_hk_len + pv.body_len(m + 1..n) <= PAGE_SIZE
    };
    let mut acc = 0;
    let mut m0 = 1;
    for i in 0..n {
        acc += pv.body_len(i..i + 1);
        if acc * 2 >= total {
            m0 = i.clamp(1, n - 2);
            break;
        }
    }
    (0..n)
        .flat_map(|d| [m0.checked_sub(d), m0.checked_add(d + 1)])
        .flatten()
        .filter(|&m| m >= 1 && m + 1 < n)
        .take(2 * n)
        .find(|&m| fits(m))
}

/// The left-most child of an inner page.
pub(crate) fn leftmost(p: &[u8]) -> u64 {
    u64_at(p, LEFTMOST)
}

// ---------------------------------------------------------------------------
// Overflow pages.

/// Writes an overflow page holding `data`, linked to `next`.
pub(crate) fn write_overflow(p: &mut [u8], tree: u64, next: u64, data: &[u8]) {
    debug_assert!(data.len() <= OVERFLOW_CAP);
    page::init(p, PageType::Overflow, tree);
    put_u64(p, OVF_NEXT, next);
    p[OVF_LEN..OVF_LEN + 4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    p[OVF_DATA..OVF_DATA + data.len()].copy_from_slice(data);
}

/// The next page and the bytes of an overflow page, or `None` if malformed.
pub(crate) fn overflow(p: &[u8]) -> Option<(u64, &[u8])> {
    let len = u32::from_le_bytes(p[OVF_LEN..OVF_LEN + 4].try_into().expect("4 bytes")) as usize;
    if len > OVERFLOW_CAP {
        return None;
    }
    Some((u64_at(p, OVF_NEXT), &p[OVF_DATA..OVF_DATA + len]))
}

// ---------------------------------------------------------------------------
// Whole-leaf iteration, for the integrity check and tests.

/// One entry of a leaf, as [`leaf_entries`] reports it.
pub(crate) struct LeafEntry<'a> {
    /// Whether the entry is stored as a restart point.
    pub(crate) restart: bool,
    /// The full key.
    pub(crate) key: &'a [u8],
    /// The value.
    pub(crate) value: Value<'a>,
}

/// Calls `f` with every entry of a leaf in stored order, checking the
/// encoding: restart points exactly at every 16th entry with their offsets
/// in the restart array, and the stream ending at the stored end. Returns a
/// description of the first format error.
pub(crate) fn leaf_entries(
    p: &[u8],
    mut f: impl FnMut(usize, LeafEntry<'_>),
) -> Result<LeafView, String> {
    let v = leaf_view(p).ok_or("malformed leaf header")?;
    let mut at = v.start;
    let mut key = Vec::new();
    for i in 0..v.count {
        let restart = i % RESTART_INTERVAL == 0;
        if restart && v.restart(p, i / RESTART_INTERVAL) != Some(at) {
            return Err(format!("restart point {} is not at entry {i}", i / 16));
        }
        let e = raw_entry(p, at, v.end).ok_or_else(|| format!("entry {i} is malformed"))?;
        if (restart && e.shared != 0) || e.shared > key.len() {
            return Err(format!("entry {i} has a bad shared length {}", e.shared));
        }
        key.truncate(e.shared);
        key.extend_from_slice(&p[e.key.clone()]);
        f(
            i,
            LeafEntry {
                restart,
                key: &key,
                value: value_of(p, e.value),
            },
        );
        at = e.next;
    }
    if at != v.end {
        return Err(format!("entry stream ends at {at}, not {}", v.end));
    }
    Ok(v)
}

/// Calls `f` with every key and child of an inner page, in slot order.
pub(crate) fn inner_entries(p: &[u8], mut f: impl FnMut(&[u8], u64)) -> Result<(), String> {
    let start = body_start(p).ok_or("malformed high-key length")?;
    let n = count(p);
    if start + 2 * n > PAGE_SIZE {
        return Err(format!("{n} slots do not fit"));
    }
    for i in 0..n {
        let (k, c) = inner_cell(p, start, i).ok_or_else(|| format!("cell {i} is malformed"))?;
        f(k, c);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_with(keys: &[&[u8]]) -> Vec<u8> {
        let mut es = Entries::default();
        for k in keys {
            es.push(k, Value::Inline(b"v"));
        }
        let mut p = vec![0u8; PAGE_SIZE];
        let mut enc = Encoder::default();
        let at = Place {
            tree: 1,
            level: 0,
            high_key: None,
            right: NONE,
        };
        assert!(write_leaf(&mut p, at, &es, 0..es.len(), &mut enc));
        p
    }

    #[test]
    fn varints_round_trip() {
        for v in [
            0,
            1,
            127,
            128,
            300,
            16_383,
            16_384,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let mut out = Vec::new();
            put_varint(&mut out, v);
            assert_eq!(out.len(), varint_len(v), "{v}");
            let mut at = 0;
            assert_eq!(get_varint(&out, &mut at, out.len()), Some(v));
            assert_eq!(at, out.len());
            let mut at = 0;
            assert_eq!(get_varint(&out, &mut at, out.len() - 1), None);
        }
    }

    #[test]
    fn restart_points_hold_full_keys_every_16_entries() {
        let keys: Vec<Vec<u8>> = (0..40u32)
            .map(|i| format!("key-{i:04}").into_bytes())
            .collect();
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
        let p = leaf_with(&refs);
        let mut seen = Vec::new();
        let v = leaf_entries(&p, |i, e| {
            assert_eq!(e.restart, i % 16 == 0);
            seen.push(e.key.to_vec());
        })
        .unwrap();
        assert_eq!(seen, keys);
        assert_eq!(v.count(), 40);
        // Shared prefixes: "key-00" for most entries, so a delta entry is
        // much shorter than a restart point.
        assert!(v.used_bytes() < 40 * 8);
        let mut buf = Vec::new();
        for k in &keys {
            assert!(matches!(
                leaf_get(&p, k, &mut buf),
                Some(Found::Hit(Value::Inline(b"v")))
            ));
        }
        assert!(matches!(leaf_get(&p, b"key-", &mut buf), Some(Found::Miss)));
        assert!(matches!(
            leaf_get(&p, b"key-0039x", &mut buf),
            Some(Found::Miss)
        ));
        assert!(matches!(leaf_get(&p, b"zzz", &mut buf), Some(Found::Miss)));
    }

    #[test]
    fn apply_keeps_every_16th_entry_a_restart_point() {
        let mut p = leaf_with(&[]);
        let mut enc = Encoder::default();
        // Inserts in descending order shift every later entry each time.
        for i in (0..100u32).rev() {
            let k = format!("{i:05}");
            let r = leaf_apply(
                &mut p,
                k.as_bytes(),
                Change::Put(Value::Inline(&[])),
                &mut enc,
            );
            assert_eq!(r, Some(Applied::Done(None)));
        }
        for i in (0..100u32).step_by(3) {
            let k = format!("{i:05}");
            let r = leaf_apply(&mut p, k.as_bytes(), Change::Delete, &mut enc);
            assert_eq!(r, Some(Applied::Done(None)));
        }
        let r = leaf_apply(&mut p, b"00000", Change::Delete, &mut enc);
        assert_eq!(r, Some(Applied::NotFound));
        let mut keys = Vec::new();
        leaf_entries(&p, |i, e| {
            assert_eq!(e.restart, i % 16 == 0);
            keys.push(String::from_utf8(e.key.to_vec()).unwrap());
        })
        .unwrap();
        let expected: Vec<String> = (0..100u32)
            .filter(|i| i % 3 != 0)
            .map(|i| format!("{i:05}"))
            .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn full_leaf_is_left_unchanged() {
        let mut p = leaf_with(&[]);
        let mut enc = Encoder::default();
        let big = vec![7u8; MAX_INLINE];
        let mut i = 0u32;
        loop {
            let k = i.to_be_bytes();
            let before = p.clone();
            match leaf_apply(&mut p, &k, Change::Put(Value::Inline(&big)), &mut enc).unwrap() {
                Applied::Done(_) => i += 1,
                Applied::Full => {
                    assert_eq!(p, before);
                    break;
                }
                Applied::NotFound => unreachable!(),
            }
        }
        assert_eq!(i, 3);
    }

    #[test]
    fn common_prefix_matches_the_bytewise_definition() {
        let base: Vec<u8> = (0..40u8).collect();
        for len_a in [0, 1, 7, 8, 9, 16, 29, 40] {
            for len_b in [0, 5, 8, 17, 29, 40] {
                for flip in 0..40 {
                    let a = &base[..len_a];
                    let mut b = base[..len_b].to_vec();
                    if flip < len_b {
                        b[flip] ^= 0x80;
                    }
                    let expected = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
                    assert_eq!(common_prefix(a, &b), expected, "{len_a} {len_b} {flip}");
                }
            }
        }
    }

    #[test]
    fn separator_is_shortest_between() {
        assert_eq!(separator(b"abc", b"abd"), b"abd");
        assert_eq!(separator(b"ab", b"abzz"), b"abz");
        assert_eq!(separator(b"a", b"b123"), b"b");
        assert_eq!(separator(b"", b"x"), b"x");
    }

    #[test]
    fn inner_pages_route_keys() {
        let mut pv = Pivots::default();
        pv.reset(10);
        pv.push(b"g", 11);
        pv.push(b"p", 12);
        let mut p = vec![0u8; PAGE_SIZE];
        let at = Place {
            tree: 3,
            level: 1,
            high_key: Some(b"z"),
            right: 99,
        };
        assert!(write_inner(&mut p, at, 10, &pv, 0..2));
        assert_eq!(inner_child(&p, b"a"), Some((0, 10)));
        assert_eq!(inner_child(&p, b"g"), Some((1, 11)));
        assert_eq!(inner_child(&p, b"o"), Some((1, 11)));
        assert_eq!(inner_child(&p, b"p"), Some((2, 12)));
        assert_eq!(high_key(&p), Some(&b"z"[..]));
        assert!(beyond_high_key(&p, b"z"));
        assert!(!beyond_high_key(&p, b"y"));
        assert_eq!((right(&p), level(&p), leftmost(&p)), (99, 1, 10));
        let mut back = Pivots::default();
        inner_decode(&p, &mut back).unwrap();
        assert_eq!(back.children(), [10, 11, 12]);
        back.remove_child_into_right(1);
        assert_eq!(back.children(), [10, 12]);
        assert_eq!(back.key(0), b"g");
    }

    /// Applies `change` with [`leaf_apply`] and with the full re-encode to
    /// copies of `p`; asserts equal outcomes and byte-identical pages, and
    /// returns them.
    fn apply_both(p: &[u8], key: &[u8], change: Change<'_>) -> (Option<Applied>, Vec<u8>) {
        let (mut a, mut b) = (p.to_vec(), p.to_vec());
        let ra = leaf_apply(&mut a, key, change, &mut Encoder::default());
        let rb = leaf_apply_reencode(
            &mut b,
            key,
            change,
            &mut Entries::default(),
            &mut Encoder::default(),
        );
        assert_eq!(ra, rb, "outcome for {key:?} {change:?}");
        assert!(a == b, "page bytes differ for {key:?} {change:?}");
        if !matches!(ra, Some(Applied::Done(_))) {
            assert!(a == p, "page changed without a change applied");
        }
        (ra, a)
    }

    fn used(p: &[u8]) -> usize {
        leaf_view(p).unwrap().used_bytes()
    }

    /// The copying [`leaf_apply`] writes the same bytes as decoding and
    /// re-encoding the tail, on random leaves: short keys over a small
    /// alphabet and keys with a 150-byte common prefix (two-byte shared
    /// varints), inline values of up to 300 bytes (two-byte tags) and
    /// overflow values, high key or none; random inserts, replacements,
    /// deletes and deletes of absent keys, 1 000 per leaf, until leaves are
    /// full.
    #[test]
    fn apply_matches_the_full_reencode() {
        use rand::rngs::StdRng;
        use rand::{RngExt, SeedableRng};
        let mut rng = StdRng::seed_from_u64(14);
        let (mut grow_deletes, mut full_puts, mut shifted, mut ops) = (0, 0, 0, 0);
        for trial in 0..120 {
            let long = trial % 2 == 1;
            let hk: Option<&[u8]> = if trial % 3 == 0 { Some(b"zz") } else { None };
            let mut p = vec![0u8; PAGE_SIZE];
            let at = Place {
                tree: 1,
                level: 0,
                high_key: hk,
                right: NONE,
            };
            assert!(write_leaf(
                &mut p,
                at,
                &Entries::default(),
                0..0,
                &mut Encoder::default()
            ));
            let mut model = std::collections::BTreeSet::<Vec<u8>>::new();
            let vbuf = vec![9u8; 300];
            for _ in 0..1000 {
                let mut key = if long && rng.random_range(0..4) > 0 {
                    vec![b'p'; 150]
                } else {
                    Vec::new()
                };
                for _ in 0..rng.random_range(1..10) {
                    key.push(b"abc"[rng.random_range(0..3)]);
                }
                let value = match rng.random_range(0..20) {
                    0 => Value::Overflow {
                        len: rng.random_range(3000..1 << 40),
                        first: rng.random(),
                    },
                    1 | 2 => Value::Inline(&vbuf[..rng.random_range(64..300)]),
                    _ => Value::Inline(&vbuf[..rng.random_range(0..20)]),
                };
                let delete = !model.is_empty() && rng.random_range(0..4) == 0;
                if delete && rng.random_range(0..4) > 0 {
                    // An existing key, picked by position.
                    let n = rng.random_range(0..model.len());
                    key = model.iter().nth(n).unwrap().clone();
                }
                let change = if delete {
                    Change::Delete
                } else {
                    Change::Put(value)
                };
                let before = used(&p);
                let pos = model.range::<Vec<u8>, _>(..&key).count();
                let (r, after) = apply_both(&p, &key, change);
                ops += 1;
                match (r, delete) {
                    (Some(Applied::Done(_)), true) => {
                        model.remove(&key);
                        if used(&after) > before {
                            grow_deletes += 1;
                        }
                        shifted += usize::from(model.len() - pos >= RESTART_INTERVAL);
                    }
                    (Some(Applied::Done(_)), false) => {
                        if model.insert(key) {
                            shifted += usize::from(model.len() - pos > RESTART_INTERVAL);
                        }
                    }
                    (Some(Applied::Full), false) => full_puts += 1,
                    _ => {}
                }
                p = after;
            }
            let mut keys = Vec::new();
            leaf_entries(&p, |i, e| {
                assert_eq!(e.restart, i % RESTART_INTERVAL == 0);
                keys.push(e.key.to_vec());
            })
            .unwrap();
            assert_eq!(keys, model.into_iter().collect::<Vec<_>>());
        }
        // Non-vacuity: the cases the copying path must get right occurred.
        assert!(grow_deletes > 50, "{grow_deletes} deletes grew a leaf");
        assert!(full_puts > 50, "{full_puts} puts did not fit");
        assert!(
            shifted > ops / 4,
            "{shifted} of {ops} changes moved a boundary"
        );
    }

    /// A delete that makes entries with long shared prefixes restart points
    /// grows the leaf, and with enough later groups no longer fits: both
    /// paths agree byte for byte, and the full leaf is left unchanged.
    #[test]
    fn deletes_that_grow_the_leaf_match_the_full_reencode() {
        // Group m: 16 keys `m`, 600 × 'x', two digits. The restart key
        // shares nothing with its predecessor and 602 bytes with its
        // successor, so each boundary that moves adds about 600 bytes.
        let keys: Vec<Vec<u8>> = (0..10u8)
            .flat_map(|m| {
                (0..16u8).map(move |j| {
                    let mut k = vec![b'a' + m];
                    k.extend_from_slice(&[b'x'; 600]);
                    k.extend_from_slice(format!("{j:02}").as_bytes());
                    k
                })
            })
            .collect();
        let refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
        let p = leaf_with(&refs);
        // One later boundary: the leaf grows and still fits.
        let k = &keys[16 * 8 + 1];
        let (r, after) = apply_both(&p, k, Change::Delete);
        assert_eq!(r, Some(Applied::Done(None)));
        assert!(
            used(&after) > used(&p) + 500,
            "{} {}",
            used(&after),
            used(&p)
        );
        // Nine later boundaries: it does not fit.
        let (r, _) = apply_both(&p, &keys[1], Change::Delete);
        assert_eq!(r, Some(Applied::Full));
    }
}
