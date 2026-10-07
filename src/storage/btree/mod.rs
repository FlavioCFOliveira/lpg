//! The generic B+tree (TS-05, TS-06): every structure keeps its data in
//! trees of this one kind.
//!
//! - **Pages.** Slotted inner pages and compressed leaves ([`node`]);
//!   values above 2 048 bytes in overflow chains.
//! - **Directory.** Tree 0, rooted at page 1, maps each tree identifier to
//!   its root page. A root never moves: a root split moves the root's
//!   entries into two new pages and leaves the root in place (SQLite
//!   `balance_deeper`, R-SQLITE `src/btree.c`), so a [`Tree`] handle stays
//!   valid for the tree's life.
//! - **Readers** (Lehman and Yao). Every page has a high key and a right
//!   link. A reader holds one page latch at a time and moves right when its
//!   key is at or above a page's high key or the page is marked deleted. A
//!   prefix scan copies the matching entries of one leaf under its latch,
//!   records the right link, releases the latch and continues there
//!   (PostgreSQL, R-PG `src/backend/access/nbtree/README:17-29, 88-110`).
//! - **Writer.** One at a time, under the write latch ([`Trees`]), which is
//!   the commit latch (TS-16). A split fills the new right page, links it,
//!   and only then latches the parent, holding the child latch until it has
//!   the parent latch (TS-06). Deletes never merge: an empty page is freed
//!   only as PostgreSQL frees one (TS-05 Underflow).
//! - **Page changes.** Every change goes through [`PageWriter`]. Before the
//!   first change of an existing page in a batch, it copies the page into
//!   memory reserved from the accountant, and, when the batch is logged,
//!   gives the page its frame position in the open WAL group and sets its
//!   page LSN in the same latched write (TS-09, TS-16 step 6). A failed
//!   batch is undone from the copies ([`write`](mod@write)).
//!
//! Keys are byte strings compared byte-wise, at most 1 024 bytes (TS-05).

pub(crate) mod bulk;
pub(crate) mod node;
mod write;

#[cfg(test)]
pub(crate) mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, MutexGuard};

use node::{Encoder, Entries, Found, NONE, Pivots, Value};

use super::datafile::{Datafile, Header};
use super::error::{Error, Health, Result};
use super::freelist::{FreeList, Readers, Registered};
use super::memory::Reservation;
use super::page::{self, PageType};
use super::pool::{PinnedPage, Pool};
use super::wal::{Counters, OpenGroup};

pub(crate) use node::MAX_KEY;

/// A tree: its identifier and its root page, which never moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Tree {
    id: u64,
    root: u64,
}

impl Tree {
    /// A handle for tree `id` rooted at `root`, as the directory lists it.
    pub(crate) fn new(id: u64, root: u64) -> Tree {
        Tree { id, root }
    }

    /// The tree identifier.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The root page.
    pub(crate) fn root(&self) -> u64 {
        self.root
    }
}

/// The tree directory: tree 0 at page 1 (TS-05).
pub(crate) const DIRECTORY: Tree = Tree { id: 0, root: 1 };

/// One write of a batch (TS-16: `(tree, key, put value | delete)`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Write<'a> {
    /// The tree written.
    pub(crate) tree: Tree,
    /// The key, at most [`MAX_KEY`] bytes.
    pub(crate) key: &'a [u8],
    /// The value to put, or `None` to delete.
    pub(crate) value: Option<&'a [u8]>,
}

/// How the undo of a batch restores one page it changed.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Saved {
    /// From copy number `.0`, which holds the page and its dirty state as
    /// they were in the pool before the batch.
    Copy(usize),
    /// A page the batch took that is free before and after the batch,
    /// whether or not it was in the pool. The tree never reads it again;
    /// only a reader that followed a link the batch wrote can reach it, so
    /// the undo keeps the image the batch wrote ([`PinnedPage::undo_keep`]).
    Free,
}

/// The undo state of one batch (TS-16 step 6): the pages it changed, in
/// first-change order, and the copy of each one that was in the pool.
#[derive(Default)]
pub(crate) struct Undo {
    /// How each changed page is restored.
    touched: HashMap<u64, Saved>,
    /// Changed pages in first-change order.
    order: Vec<u64>,
    /// Page copies, one page each, with the page's dirty state before the
    /// batch.
    copies: Vec<(Box<[u8]>, bool)>,
    /// The accountant's bytes for `copies`.
    reserved: Option<Reservation>,
    /// Pages the batch took from the free list or the reserve.
    taken: Vec<u64>,
    /// Pages the batch freed: live again after an undo.
    freed: HashSet<u64>,
}

impl Undo {
    fn clear(&mut self) {
        self.touched.clear();
        self.order.clear();
        self.copies.clear();
        self.reserved = None;
        self.taken.clear();
        self.freed.clear();
    }
}

/// Gives access to the pages of a write batch and records each change: the
/// only path by which tree, free-list and header pages change.
pub(crate) struct PageWriter<'p> {
    pool: &'p Pool,
    data: &'p Datafile,
    undo: &'p mut Undo,
    header: bool,
    /// The open WAL group when the batch is logged.
    group: Option<&'p mut OpenGroup>,
}

impl<'p> PageWriter<'p> {
    /// Pins page `p` for reading; records nothing.
    pub(crate) fn fetch(&self, p: u64) -> Result<PinnedPage<'p>> {
        self.pool.fetch(p)
    }

    /// Reserves a page from the accountant for one more copy.
    fn reserve_copy(&mut self) -> Result<()> {
        let reservation = self.pool.reserve_page_copy()?;
        match &mut self.undo.reserved {
            Some(r) => r.absorb(reservation),
            None => self.undo.reserved = Some(reservation),
        }
        Ok(())
    }

    /// The page LSN of `p` in a logged batch: its frame position in the
    /// open group plus one frame (TS-09); 0 in an unlogged batch (tests).
    fn page_lsn(&mut self, p: u64) -> u64 {
        self.group.as_deref_mut().map_or(0, |g| g.page_lsn(p))
    }

    /// Records that page `p` changes in this batch. Before its first
    /// change, the page and its dirty state are copied into memory reserved
    /// from the accountant and, in a logged batch, the page is given its
    /// page LSN in the open group, in one write-latched step, so that
    /// eviction cannot write it before its group is durable (TS-09).
    ///
    /// # Errors
    ///
    /// [`Error::MemoryLimit`] if the copy does not fit the cap; errors of
    /// [`Pool::fetch`].
    pub(crate) fn mark(&mut self, p: u64) -> Result<()> {
        if self.undo.touched.contains_key(&p) {
            return Ok(());
        }
        self.reserve_copy()?;
        let pin = self.pool.fetch(p)?;
        let (mut g, was_dirty) = pin.write_noting_dirty()?;
        self.undo.copies.push((Box::from(&g[..]), was_dirty));
        self.undo
            .touched
            .insert(p, Saved::Copy(self.undo.copies.len() - 1));
        self.undo.order.push(p);
        if self.group.is_some() {
            let lsn = self.page_lsn(p);
            page::set_lsn(&mut g, lsn);
        }
        Ok(())
    }

    /// Records and pins page `p`, which the caller is about to change.
    pub(crate) fn change(&mut self, p: u64) -> Result<PinnedPage<'p>> {
        self.mark(p)?;
        self.pool.fetch(p)
    }

    /// Records and pins page `p`, newly taken, without reading it: its frame
    /// holds zeros, apart from its page LSN in a logged batch, and the
    /// caller initialises it.
    ///
    /// A taken page that this batch freed is copied first, with its dirty
    /// state, and read from disk if it left the pool: the undo makes it
    /// live again, so it must get its image back. LMDB likewise reuses
    /// within a transaction only pages it holds a dirty copy of ("loose"
    /// pages, R-LMDB `libraries/liblmdb/mdb.c:2406-2433`). The free list
    /// copies an emptied trunk before it is taken, for the same reason
    /// ([`super::freelist::FreeList::allocate`]).
    ///
    /// Every other taken page is free before and after the batch, in the
    /// pool or not, and needs no copy: the tree never reads it again, and a
    /// reader that followed a link the batch wrote needs the image the batch
    /// wrote, not an older one, which may belong to another tree. The undo
    /// keeps that image ([`PinnedPage::undo_keep`]), with a page LSN at
    /// which every commit that freed the page is durable, so an older image
    /// in a group not yet durable is never overwritten on disk early
    /// (TS-09). LMDB's abort likewise discards the dirty copies a
    /// transaction made of pages it took (R-LMDB
    /// `libraries/liblmdb/mdb.c:3740`, `mdb_dlist_free`).
    ///
    /// # Errors
    ///
    /// [`Error::MemoryLimit`] if a copy does not fit the cap; errors of
    /// [`Pool::create`].
    pub(crate) fn create(&mut self, p: u64) -> Result<PinnedPage<'p>> {
        if !self.undo.touched.contains_key(&p) {
            let saved = if self.undo.freed.contains(&p) {
                let (pin, dirty) = match self.pool.resident(p) {
                    Some(found) => found,
                    None => (self.pool.fetch(p)?, false),
                };
                self.reserve_copy()?;
                let g = pin.read()?;
                self.undo.copies.push((Box::from(&g[..]), dirty));
                Saved::Copy(self.undo.copies.len() - 1)
            } else {
                Saved::Free
            };
            self.undo.touched.insert(p, saved);
            self.undo.order.push(p);
        }
        let lsn = self.page_lsn(p);
        self.pool.create(p, lsn)
    }

    /// Records that a field of page 0 changes; it is written when the
    /// batch ends.
    pub(crate) fn header_changed(&mut self) {
        self.header = true;
    }

    /// Takes the next reserved, unused page of the data file, raising the
    /// page count in page 0. If the reserve is used up, the file is
    /// extended here, under the commit latch (TS-16 step 6 Overrun).
    ///
    /// # Errors
    ///
    /// ENOSPC if the extension does not fit; the batch is then undone.
    pub(crate) fn allocate_reserved(&mut self) -> Result<u64> {
        let p = match self.data.allocate() {
            Err(e) if e.is_no_space() => {
                self.data.ensure(1)?;
                self.data.allocate()?
            }
            r => r?,
        };
        self.header = true;
        Ok(p)
    }
}

/// Scratch buffers of the writer, kept between batches so that a write
/// allocates nothing once they have grown.
#[derive(Default)]
struct Scratch {
    es: Entries,
    enc: Encoder,
    pv: Pivots,
    pv2: Pivots,
    path: Vec<Step>,
}

/// One page of a root-to-leaf path: the page and its child position in the
/// page above (0 for the root).
#[derive(Debug, Clone, Copy)]
struct Step {
    page: u64,
    idx: usize,
}

/// The WAL state kept under the commit latch: the open group and the
/// counters of the last commit applied.
pub(crate) struct LogState {
    /// The open WAL group (TS-09).
    pub(crate) group: OpenGroup,
    /// Counters after the last commit that joined a group.
    pub(crate) counters: Counters,
}

/// State changed only by the writer, under the write latch.
pub(crate) struct WriteState {
    free: FreeList,
    next_tree_id: u64,
    undo: Undo,
    scratch: Scratch,
    log: LogState,
    /// After an undo: the batch number it took and the pages the undone
    /// batch had taken. A batch that takes one of them first waits until
    /// every tree read operation that started before the undo has ended
    /// (TS-16 step 6 undo, item 3).
    fence: Option<(u64, HashSet<u64>)>,
}

/// The tree state of one database.
pub(crate) struct Trees {
    write: Mutex<WriteState>,
    readers: Readers,
    /// Number of the last write batch whose apply step has finished.
    applied: AtomicU64,
    /// Tests only: raised each time a batch starts waiting at the reuse
    /// fence, so a test can see the wait without timing it.
    #[cfg(test)]
    pub(crate) fence_waits: AtomicU64,
}

impl Trees {
    /// Tree state for a database whose page 0 holds `header`; `free` is
    /// its loaded free list and `log` its WAL state after recovery.
    pub(crate) fn new(header: &Header, free: FreeList, log: LogState) -> Trees {
        Trees {
            write: Mutex::new(WriteState {
                free,
                next_tree_id: header.next_tree_id,
                undo: Undo::default(),
                scratch: Scratch::default(),
                log,
                fence: None,
            }),
            readers: Readers::default(),
            applied: AtomicU64::new(0),
            #[cfg(test)]
            fence_waits: AtomicU64::new(0),
        }
    }

    /// Takes the write latch. A latch poisoned by a panic sets the unusable
    /// latch of `health`, and so does a panic while the returned guard is
    /// held (see [`CommitLatch`]).
    fn lock<'a>(&'a self, health: &'a Health) -> CommitLatch<'a> {
        let guard = self.write.lock().unwrap_or_else(|poisoned| {
            let _ = health.fail(Error::Unusable);
            poisoned.into_inner()
        });
        CommitLatch {
            guard,
            health,
            panicking: std::thread::panicking(),
        }
    }
}

/// The held commit latch (TS-16 steps 3-7).
///
/// A panic while it is held can leave pages, the open WAL group or the free
/// list half changed, with no undo. Dropping the guard while the thread
/// unwinds from such a panic therefore sets the unusable latch before the
/// latch is released: no later commit, and no WAL group close
/// (`Core::close_group`), can then use that state. This is PostgreSQL's
/// critical-section rule, where any error is a PANIC because "the shared
/// buffers will contain unlogged changes" (R-PG
/// `src/backend/access/transam/README:441-446`). A guard taken while the
/// thread was already unwinding does not count that panic, as std's poison
/// flag (`library/std/src/sync/poison.rs`, `Flag::done`).
pub(crate) struct CommitLatch<'a> {
    guard: MutexGuard<'a, WriteState>,
    health: &'a Health,
    /// Whether the thread was already unwinding when the latch was taken.
    panicking: bool,
}

impl Deref for CommitLatch<'_> {
    type Target = WriteState;

    fn deref(&self) -> &WriteState {
        &self.guard
    }
}

impl DerefMut for CommitLatch<'_> {
    fn deref_mut(&mut self) -> &mut WriteState {
        &mut self.guard
    }
}

impl Drop for CommitLatch<'_> {
    fn drop(&mut self) {
        // Runs before the field `guard` is dropped, so the latch is set
        // before the mutex is released.
        if !self.panicking && std::thread::panicking() {
            let _ = self.health.fail(Error::Unusable);
        }
    }
}

/// The trees of one open database: the pool they live in and their state.
#[derive(Clone, Copy)]
pub(crate) struct Store<'e> {
    pub(crate) pool: &'e Pool,
    pub(crate) data: &'e Datafile,
    pub(crate) trees: &'e Trees,
    pub(crate) health: &'e Health,
}

thread_local! {
    /// Key reconstruction buffer of point reads.
    static KEY_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(node::MAX_KEY));
}

impl<'e> Store<'e> {
    /// A format error found in page `p` while open: corruption, which
    /// makes the database unusable (TS-12).
    fn corrupt(&self, p: u64, what: &str) -> Error {
        self.health
            .fail(Error::Corruption(format!("page {p}: {what}")))
    }

    /// Descends `tree` to the leaf whose key range holds `key`, moving
    /// right past concurrent splits and deleted pages, and calls `f` with
    /// that leaf under its shared latch.
    fn at_leaf<R>(
        &self,
        tree: Tree,
        key: &[u8],
        f: impl FnOnce(&[u8], u64) -> Result<R>,
    ) -> Result<R> {
        let mut page_no = tree.root;
        // A level or right-link chain longer than the file is a cycle.
        let mut steps = 0u64;
        loop {
            steps += 1;
            if steps > self.data.page_count() + 1 {
                return Err(self.corrupt(page_no, "descent does not end"));
            }
            let pin = self.pool.fetch(page_no)?;
            let g = pin.read()?;
            if page::tree(&g) != tree.id {
                return Err(self.corrupt(page_no, "page of another tree"));
            }
            if node::is_deleted(&g) || node::beyond_high_key(&g, key) {
                let r = node::right(&g);
                if r == NONE {
                    return Err(self.corrupt(page_no, "right-most page is deleted or bounded"));
                }
                page_no = r;
                continue;
            }
            match page::page_type(&g) {
                Some(PageType::Inner) => {
                    page_no = node::inner_child(&g, key)
                        .ok_or_else(|| self.corrupt(page_no, "malformed inner page"))?
                        .1;
                }
                Some(PageType::Leaf) => return f(&g, page_no),
                _ => return Err(self.corrupt(page_no, "not a tree page")),
            }
        }
    }

    /// Looks `key` up in `tree`. On a hit, the value replaces the contents
    /// of `out` and the result is `true`.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] for a malformed page, which sets the unusable
    /// latch; errors of [`Pool::fetch`].
    pub(crate) fn get(&self, tree: Tree, key: &[u8], out: &mut Vec<u8>) -> Result<bool> {
        if key.len() > MAX_KEY {
            return Ok(false);
        }
        let _reg = self.reader();
        let hit = KEY_BUF.with(|buf| {
            let mut buf = buf.borrow_mut();
            self.at_leaf(tree, key, |p, page_no| {
                match node::leaf_get(p, key, &mut buf)
                    .ok_or_else(|| self.corrupt(page_no, "malformed leaf"))?
                {
                    Found::Miss => Ok(None),
                    Found::Hit(Value::Inline(v)) => {
                        out.clear();
                        out.extend_from_slice(v);
                        Ok(Some(None))
                    }
                    Found::Hit(Value::Overflow { len, first }) => Ok(Some(Some((len, first)))),
                }
            })
        })?;
        match hit {
            None => Ok(false),
            Some(None) => Ok(true),
            Some(Some((len, first))) => {
                out.clear();
                self.read_overflow(tree, len, first, out)?;
                Ok(true)
            }
        }
    }

    /// Appends the `len` bytes of the overflow chain starting at `first` to
    /// `out`, holding one page latch at a time.
    fn read_overflow(&self, tree: Tree, len: u64, first: u64, out: &mut Vec<u8>) -> Result<()> {
        let start = out.len();
        let mut p = first;
        while ((out.len() - start) as u64) < len {
            if p == NONE {
                return Err(self.corrupt(first, "overflow chain ends early"));
            }
            let pin = self.pool.fetch(p)?;
            let g = pin.read()?;
            if page::page_type(&g) != Some(PageType::Overflow) || page::tree(&g) != tree.id {
                return Err(self.corrupt(p, "not an overflow page of this tree"));
            }
            let (next, bytes) =
                node::overflow(&g).ok_or_else(|| self.corrupt(p, "malformed overflow page"))?;
            out.extend_from_slice(bytes);
            p = next;
        }
        if (out.len() - start) as u64 != len {
            return Err(self.corrupt(first, "overflow chain longer than its value"));
        }
        Ok(())
    }

    /// Calls `f(key, value)` for every entry of `tree` whose key starts
    /// with `prefix`, in key order, until `f` returns `false`.
    ///
    /// The entries of one leaf are copied under its latch; `f` runs with no
    /// latch held. A key present and unchanged for the whole scan is passed
    /// exactly once (TS-06).
    ///
    /// The scan keeps its position in the key space: the highest high key of
    /// the leaves it has passed. On each later leaf it takes only keys at or
    /// above that position. A freed page gives its key space to its right
    /// sibling (TS-05 Underflow), so once the scan has left a leaf, that
    /// leaf and its neighbours can be freed and their ranges joined to a
    /// page the scan reaches later, which may then hold keys inserted behind
    /// the scan. Without the position the scan would return them out of
    /// order. PostgreSQL needs no such check because its MVCC snapshot hides
    /// those keys (R-PG `src/backend/access/nbtree/README:95-98`); here no
    /// snapshot exists until MVCC is built (TS-14).
    ///
    /// # Errors
    ///
    /// As [`Store::get`].
    pub(crate) fn scan_prefix(
        &self,
        tree: Tree,
        prefix: &[u8],
        mut f: impl FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<()> {
        let _reg = self.reader();
        let mut copied = Copied::default();
        let mut key_buf = Vec::new();
        let mut value = Vec::new();
        // The scan's position: every key below it has been passed.
        let mut from = prefix.to_vec();
        // The first leaf is found by descent; later ones by right links.
        let mut next = self.at_leaf(tree, prefix, |p, page_no| {
            copied
                .take(p, &mut from, prefix, &mut key_buf)
                .ok_or_else(|| self.corrupt(page_no, "malformed leaf"))
        })?;
        loop {
            for i in 0..copied.len() {
                let (k, v) = copied.get(i);
                let v = match v {
                    Value::Inline(v) => v,
                    Value::Overflow { len, first } => {
                        value.clear();
                        self.read_overflow(tree, len, first, &mut value)?;
                        &value[..]
                    }
                };
                if !f(k, v) {
                    return Ok(());
                }
            }
            let Some(page_no) = next else {
                return Ok(());
            };
            let pin = self.pool.fetch(page_no)?;
            let g = pin.read()?;
            if page::page_type(&g) != Some(PageType::Leaf) || page::tree(&g) != tree.id {
                return Err(self.corrupt(
                    page_no,
                    "right link to a page that is not a leaf of this tree",
                ));
            }
            next = copied
                .take(&g, &mut from, prefix, &mut key_buf)
                .ok_or_else(|| self.corrupt(page_no, "malformed leaf"))?;
        }
    }

    /// Finds `tree id` in the directory.
    ///
    /// # Errors
    ///
    /// As [`Store::get`]; [`Error::Corruption`] for a malformed entry.
    pub(crate) fn tree(&self, id: u64) -> Result<Option<Tree>> {
        let mut v = Vec::with_capacity(8);
        if id == 0 {
            return Ok(Some(DIRECTORY));
        }
        if !self.get(DIRECTORY, &id.to_be_bytes(), &mut v)? {
            return Ok(None);
        }
        let root = u64::from_le_bytes(
            v.as_slice()
                .try_into()
                .map_err(|_| self.corrupt(DIRECTORY.root, "malformed directory entry"))?,
        );
        Ok(Some(Tree { id, root }))
    }

    /// Registers a tree read operation that starts now (TS-05 Reuse); it
    /// ends when the result is dropped.
    pub(crate) fn reader(&self) -> Registered<'e> {
        self.trees.readers.enter(&self.trees.applied)
    }

    /// Takes the write latch: the single writer (TS-16 commit latch).
    pub(crate) fn lock(&self) -> CommitLatch<'e> {
        self.trees.lock(self.health)
    }

    /// The lowest value registered by a running reader, `u64::MAX` if none.
    pub(crate) fn horizon(&self) -> u64 {
        self.trees.readers.horizon()
    }
}

impl WriteState {
    /// The free list.
    pub(crate) fn free_list(&self) -> &FreeList {
        &self.free
    }

    /// The WAL state.
    pub(crate) fn log(&mut self) -> &mut LogState {
        &mut self.log
    }

    /// The WAL state, read-only.
    pub(crate) fn log_ref(&self) -> &LogState {
        &self.log
    }

    /// The next tree identifier.
    pub(crate) fn next_tree_id(&self) -> u64 {
        self.next_tree_id
    }
}

/// Entries of one leaf copied under its latch by a scan.
#[derive(Default)]
struct Copied {
    bytes: Vec<u8>,
    items: Vec<(std::ops::Range<usize>, CopiedValue)>,
}

enum CopiedValue {
    Inline(std::ops::Range<usize>),
    Overflow { len: u64, first: u64 },
}

impl Copied {
    fn len(&self) -> usize {
        self.items.len()
    }

    fn get(&self, i: usize) -> (&[u8], Value<'_>) {
        let (k, v) = &self.items[i];
        let v = match v {
            CopiedValue::Inline(r) => Value::Inline(&self.bytes[r.clone()]),
            CopiedValue::Overflow { len, first } => Value::Overflow {
                len: *len,
                first: *first,
            },
        };
        (&self.bytes[k.clone()], v)
    }

    /// Replaces the contents with the entries of leaf `p` that start with
    /// `prefix` and are at or above the scan position `from`, raises `from`
    /// to the leaf's high key, and returns the leaf to continue at: its
    /// right link, unless no later key can start with `prefix`. `None` on a
    /// malformed leaf.
    fn take(
        &mut self,
        p: &[u8],
        from: &mut Vec<u8>,
        prefix: &[u8],
        key_buf: &mut Vec<u8>,
    ) -> Option<Option<u64>> {
        self.bytes.clear();
        self.items.clear();
        let mut past = false;
        if !node::is_deleted(p) {
            node::leaf_for_each_from(p, from, key_buf, |k, v| {
                if !k.starts_with(prefix) {
                    past = true;
                    return false;
                }
                let ks = self.bytes.len();
                self.bytes.extend_from_slice(k);
                let key = ks..self.bytes.len();
                let value = match v {
                    Value::Inline(v) => {
                        let vs = self.bytes.len();
                        self.bytes.extend_from_slice(v);
                        CopiedValue::Inline(vs..self.bytes.len())
                    }
                    Value::Overflow { len, first } => CopiedValue::Overflow { len, first },
                };
                self.items.push((key, value));
                true
            })?;
        }
        let right = node::right(p);
        let hk = node::high_key(p);
        // Every key of this leaf's range is passed. A high key below the
        // position belongs to a page whose range grew behind the scan.
        if let Some(hk) = hk
            && hk > from.as_slice()
        {
            from.clear();
            from.extend_from_slice(hk);
        }
        // A high key above every key with the prefix ends the scan.
        let done =
            past || right == NONE || hk.is_some_and(|hk| !hk.starts_with(prefix) && hk > prefix);
        Some((!done).then_some(right))
    }
}
