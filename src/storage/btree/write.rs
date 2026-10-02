//! The writer: applies a batch of puts and deletes (TS-16 step 6 without
//! the WAL), splitting and freeing pages (TS-05, TS-06).

use std::sync::atomic::Ordering;

use super::node::{self, Applied, Change, MAX_INLINE, MAX_KEY, NONE, OVERFLOW_CAP, Place, Value};
use super::{DIRECTORY, PageWriter, Scratch, Step, Store, Tree, Write, WriteState};
use crate::storage::datafile::Header;
use crate::storage::error::{Error, Result};
use crate::storage::freelist::{FreeList, Readers};
use crate::storage::page::{self, PageType};
use crate::storage::pool::PinnedPage;

/// Pages a batch may need, by the TS-05 bound: 2 × (⌈write-set bytes /
/// 4 KiB⌉ + trees written) + the sum of the heights of the trees written.
fn bound(bytes: u64, trees: u64, heights: u64) -> u64 {
    2 * (bytes.div_ceil(4096) + trees) + heights
}

impl Store<'_> {
    /// Height of `tree`: the root's level + 1.
    fn height(&self, tree: Tree) -> Result<u64> {
        let pin = self.pool.fetch(tree.root)?;
        let g = pin.read()?;
        Ok(u64::from(node::level(&g)) + 1)
    }

    /// Applies `batch` in order, as one write batch (TS-16 step 6).
    ///
    /// Before any page changes: every key is checked against the 1 024-byte
    /// limit (TS-05), and the reserve of unused pages is made to cover the
    /// TS-05 bound. A failure after the first change makes the database
    /// unusable, since nothing undoes a half-applied batch.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key over 1 024 bytes, with nothing
    /// changed; ENOSPC from the reservation, with nothing changed and the
    /// database usable (FR-REC-007); any later error, which sets the
    /// unusable latch.
    pub(crate) fn apply(&self, batch: &[Write<'_>]) -> Result<()> {
        self.health.check()?;
        if batch.iter().any(|w| w.key.len() > MAX_KEY) {
            return Err(Error::InvalidArgument("key longer than 1 024 bytes"));
        }
        let mut trees: Vec<Tree> = batch.iter().map(|w| w.tree).collect();
        trees.sort_unstable_by_key(|t| t.id);
        trees.dedup();
        let bytes: u64 = batch
            .iter()
            .map(|w| (w.key.len() + w.value.map_or(0, <[u8]>::len)) as u64)
            .sum();
        let mut heights = 0;
        for &t in &trees {
            heights += self.height(t)?;
        }
        self.data
            .ensure(bound(bytes, trees.len() as u64, heights))?;
        let mut st = self.lock();
        self.run(&mut st, |a| {
            for w in batch {
                match w.value {
                    Some(v) => a.put(w.tree, w.key, v)?,
                    None => a.delete(w.tree, w.key)?,
                }
            }
            Ok(())
        })
    }

    /// Creates an empty tree with the next identifier and records its root
    /// in the directory (TS-05).
    ///
    /// # Errors
    ///
    /// As [`Store::apply`].
    pub(crate) fn create_tree(&self) -> Result<Tree> {
        self.health.check()?;
        let heights = self.height(DIRECTORY)?;
        self.data.ensure(bound(16, 2, heights + 1))?;
        let mut st = self.lock();
        let mut tree = None;
        self.run(&mut st, |a| {
            let id = *a.next_tree_id;
            let root = a.allocate()?;
            {
                let pin = a.pw.create(root)?;
                node::init_empty_leaf(&mut pin.write()?, id);
            }
            a.put(DIRECTORY, &id.to_be_bytes(), &root.to_le_bytes())?;
            *a.next_tree_id = id + 1;
            a.pw.header_changed();
            tree = Some(Tree { id, root });
            Ok(())
        })?;
        Ok(tree.expect("set by the batch"))
    }

    /// Runs one write batch under the write latch `st`.
    fn run(
        &self,
        st: &mut WriteState,
        body: impl FnOnce(&mut Apply<'_>) -> Result<()>,
    ) -> Result<()> {
        let batch = self.trees.applied.load(Ordering::Relaxed) + 1;
        let WriteState {
            free,
            next_tree_id,
            changed,
            scratch,
        } = st;
        changed.clear();
        let mut a = Apply {
            pw: PageWriter {
                pool: self.pool,
                data: self.data,
                changed,
                header: false,
            },
            free,
            next_tree_id,
            s: scratch,
            readers: &self.trees.readers,
            batch,
            store: *self,
        };
        let r = body(&mut a).and_then(|()| a.finish());
        let touched = !a.pw.changed.is_empty();
        match r {
            Ok(()) => {
                // Readers that start from now on see this batch applied.
                self.trees.applied.store(batch, Ordering::SeqCst);
                Ok(())
            }
            Err(e) if touched => Err(self.health.fail(e)),
            Err(e) => Err(e),
        }
    }
}

/// One write batch in progress.
pub(super) struct Apply<'a> {
    pub(super) pw: PageWriter<'a>,
    free: &'a mut FreeList,
    next_tree_id: &'a mut u64,
    s: &'a mut Scratch,
    readers: &'a Readers,
    /// This batch's number: the tag of the pages it frees.
    batch: u64,
    store: Store<'a>,
}

impl<'a> Apply<'a> {
    fn corrupt(&self, p: u64, what: &str) -> Error {
        Error::Corruption(format!("page {p}: {what}"))
    }

    /// Writes the fields of page 0 if this batch changed them.
    fn finish(&mut self) -> Result<()> {
        if self.pw.header {
            let pin = self.pw.change(0)?;
            let mut g = pin.write()?;
            let mut h = Header::decode(&g)?;
            h.free_list_head = self.free.head();
            h.next_tree_id = *self.next_tree_id;
            h.page_count = self.store.data.page_count();
            h.write_fields(&mut g);
        }
        Ok(())
    }

    /// Takes a page for this batch (TS-05: free pages first).
    pub(super) fn allocate(&mut self) -> Result<u64> {
        let horizon = self.readers.horizon();
        self.free.allocate(horizon, &mut self.pw)
    }

    /// Frees page `p`, which this batch has unlinked.
    fn free_page(&mut self, p: u64) -> Result<()> {
        let horizon = self.readers.horizon();
        self.free.free(p, self.batch, horizon, &mut self.pw)
    }

    /// Writes `value` to a new overflow chain and returns its first page.
    fn write_overflow(&mut self, tree: Tree, value: &[u8]) -> Result<u64> {
        let pages: Vec<u64> = (0..value.len().div_ceil(OVERFLOW_CAP))
            .map(|_| self.allocate())
            .collect::<Result<_>>()?;
        for (i, chunk) in value.chunks(OVERFLOW_CAP).enumerate() {
            let next = pages.get(i + 1).copied().unwrap_or(NONE);
            let pin = self.pw.create(pages[i])?;
            node::write_overflow(&mut pin.write()?, tree.id, next, chunk);
        }
        Ok(pages[0])
    }

    /// Frees the overflow chain starting at `first`.
    fn free_overflow(&mut self, first: u64) -> Result<()> {
        let mut p = first;
        while p != NONE {
            let next = {
                let pin = self.pw.fetch(p)?;
                let g = pin.read()?;
                if page::page_type(&g) != Some(PageType::Overflow) {
                    return Err(self.corrupt(p, "not an overflow page"));
                }
                node::overflow(&g)
                    .ok_or_else(|| self.corrupt(p, "malformed overflow page"))?
                    .0
            };
            self.free_page(p)?;
            p = next;
        }
        Ok(())
    }

    /// Descends `tree` to the leaf for `key`, recording the path in
    /// `self.s.path`. The writer is alone, so no page on the way can have
    /// split or been deleted since its parent was read.
    fn descend(&mut self, tree: Tree, key: &[u8]) -> Result<()> {
        self.s.path.clear();
        let mut step = Step {
            page: tree.root,
            idx: 0,
        };
        loop {
            self.s.path.push(step);
            if self.s.path.len() > 64 {
                return Err(self.corrupt(tree.root, "tree deeper than 64 levels"));
            }
            let pin = self.pw.fetch(step.page)?;
            let g = pin.read()?;
            if page::tree(&g) != tree.id || node::is_deleted(&g) || node::beyond_high_key(&g, key) {
                return Err(self.corrupt(step.page, "page out of place on the writer's path"));
            }
            match page::page_type(&g) {
                Some(PageType::Leaf) => return Ok(()),
                Some(PageType::Inner) => {
                    let (idx, child) = node::inner_child(&g, key)
                        .ok_or_else(|| self.corrupt(step.page, "malformed inner page"))?;
                    step = Step { page: child, idx };
                }
                _ => return Err(self.corrupt(step.page, "not a tree page")),
            }
        }
    }

    /// Puts `value` under `key` in `tree`.
    pub(super) fn put(&mut self, tree: Tree, key: &[u8], value: &[u8]) -> Result<()> {
        let first = if value.len() > MAX_INLINE {
            Some(self.write_overflow(tree, value)?)
        } else {
            None
        };
        let stored = match first {
            Some(first) => Value::Overflow {
                len: value.len() as u64,
                first,
            },
            None => Value::Inline(value),
        };
        self.descend(tree, key)?;
        let leaf = self.s.path.last().expect("path holds the leaf").page;
        let applied = {
            let pin = self.pw.change(leaf)?;
            let mut g = pin.write()?;
            node::leaf_apply(&mut g, key, Change::Put(stored), &mut self.s.enc)
                .ok_or_else(|| self.corrupt(leaf, "malformed leaf"))?
        };
        let freed = match applied {
            Applied::Done(freed) => freed,
            Applied::Full => self.split_apply(tree, key, Change::Put(stored))?,
            Applied::NotFound => return Err(self.corrupt(leaf, "a put found nothing to apply")),
        };
        if let Some(f) = freed {
            self.free_overflow(f)?;
        }
        Ok(())
    }

    /// Removes `key` from `tree`, if present, and frees its leaf if that
    /// leaves it empty and TS-05 allows it.
    pub(super) fn delete(&mut self, tree: Tree, key: &[u8]) -> Result<()> {
        self.descend(tree, key)?;
        let leaf = self.s.path.last().expect("path holds the leaf").page;
        let (applied, empty) = {
            let pin = self.pw.change(leaf)?;
            let mut g = pin.write()?;
            let r = node::leaf_apply(&mut g, key, Change::Delete, &mut self.s.enc)
                .ok_or_else(|| self.corrupt(leaf, "malformed leaf"))?;
            (r, node::count(&g) == 0)
        };
        let freed = match applied {
            Applied::Done(freed) => freed,
            Applied::NotFound => return Ok(()),
            // Restart points moved and the leaf grew: split it (see
            // `node::leaf_decode_with`).
            Applied::Full => self.split_apply(tree, key, Change::Delete)?,
        };
        if let Some(f) = freed {
            self.free_overflow(f)?;
        }
        if empty {
            self.free_empty_leaf(tree)?;
        }
        Ok(())
    }

    /// Splits the leaf at the end of `self.s.path`, which cannot take
    /// `change` for `key`, and inserts the separator above. Returns the
    /// overflow chain of a replaced or removed value.
    fn split_apply(&mut self, tree: Tree, key: &[u8], change: Change<'_>) -> Result<Option<u64>> {
        let path = std::mem::take(&mut self.s.path);
        let r = self.split_apply_on(tree, &path, key, change);
        self.s.path = path;
        r
    }

    fn split_apply_on(
        &mut self,
        tree: Tree,
        path: &[Step],
        key: &[u8],
        change: Change<'_>,
    ) -> Result<Option<u64>> {
        // Pin the whole path first (TS-16 step 6), so the latches below are
        // taken on pages already in the pool.
        let pins: Vec<PinnedPage<'a>> = path
            .iter()
            .map(|s| self.pw.fetch(s.page))
            .collect::<Result<_>>()?;
        let k = path.len() - 1;
        let leaf = path[k].page;
        self.pw.mark(leaf);
        let mut g = pins[k].write()?;
        let freed = node::leaf_decode_with(&g, key, change, &mut self.s.es)
            .ok_or_else(|| self.corrupt(leaf, "malformed leaf"))?;
        let hk = node::high_key(&g).map(<[u8]>::to_vec);
        let right = node::right(&g);
        let n = self.s.es.len();
        let s = node::leaf_split_point(&self.s.es, right == NONE, hk.as_ref().map_or(0, Vec::len))
            .ok_or_else(|| self.corrupt(leaf, "no split point fits"))?;
        let sep = node::separator(self.s.es.key(s - 1), self.s.es.key(s)).to_vec();
        let place = |high_key, right| Place {
            tree: tree.id,
            level: 0,
            high_key,
            right,
        };
        if k == 0 {
            // Root leaf: its entries move to two new pages; the root stays.
            let (a, b) = (self.allocate()?, self.allocate()?);
            self.fill_leaf(a, place(Some(&sep), b), 0..s)?;
            self.fill_leaf(b, place(None, NONE), s..n)?;
            self.s.pv.reset(a);
            self.s.pv.push(&sep, b);
            let root = Place {
                level: 1,
                ..place(None, NONE)
            };
            let ok = node::write_inner(&mut g, root, a, &self.s.pv, 0..1);
            debug_assert!(ok);
            return Ok(freed);
        }
        // Fill the new right page and link it, then rewrite the left page.
        let r = self.allocate()?;
        self.fill_leaf(r, place(hk.as_deref(), right), s..n)?;
        let ok = node::write_leaf(
            &mut g,
            place(Some(&sep), r),
            &self.s.es,
            0..s,
            &mut self.s.enc,
        );
        debug_assert!(ok, "split point checked to fit");
        self.insert_pivot(tree, path, &pins, k - 1, g, sep, r)?;
        Ok(freed)
    }

    /// Writes entries `range` of the scratch entries to the new leaf `p`.
    fn fill_leaf(&mut self, p: u64, at: Place<'_>, range: std::ops::Range<usize>) -> Result<()> {
        let pin = self.pw.create(p)?;
        let mut g = pin.write()?;
        let ok = node::write_leaf(&mut g, at, &self.s.es, range, &mut self.s.enc);
        debug_assert!(ok, "split point checked to fit");
        Ok(())
    }

    /// Inserts `(sep, new_child)` after the child at `path[k + 1]` in the
    /// inner page `path[k]`, splitting upwards as needed. `child` is the
    /// exclusive latch of the page that split; it is released only once the
    /// parent's latch is held (TS-06).
    #[expect(
        clippy::too_many_arguments,
        reason = "the split state: path, its pins, level, held latch and pivot"
    )]
    fn insert_pivot<'p>(
        &mut self,
        tree: Tree,
        path: &[Step],
        pins: &'p [PinnedPage<'a>],
        mut k: usize,
        child: std::sync::RwLockWriteGuard<'p, Box<[u8]>>,
        mut sep: Vec<u8>,
        mut new_child: u64,
    ) -> Result<()> {
        let mut child = child;
        loop {
            let parent = path[k].page;
            let pos = path[k + 1].idx;
            self.pw.mark(parent);
            let mut pg = pins[k].write()?;
            drop(child);
            node::inner_decode(&pg, &mut self.s.pv)
                .ok_or_else(|| self.corrupt(parent, "malformed inner page"))?;
            self.s.pv.insert_after_child(pos, &sep, new_child);
            let level = node::level(&pg);
            let hk = node::high_key(&pg).map(<[u8]>::to_vec);
            let right = node::right(&pg);
            let place = |high_key, right| Place {
                tree: tree.id,
                level,
                high_key,
                right,
            };
            let n = self.s.pv.len();
            let leftmost = self.s.pv.child(0);
            if node::write_inner(
                &mut pg,
                place(hk.as_deref(), right),
                leftmost,
                &self.s.pv,
                0..n,
            ) {
                return Ok(());
            }
            let m = node::inner_split_point(&self.s.pv, hk.as_ref().map_or(0, Vec::len))
                .ok_or_else(|| self.corrupt(parent, "no inner split point fits"))?;
            let up = self.s.pv.key(m).to_vec();
            let right_leftmost = self.s.pv.child(m + 1);
            if k == 0 {
                // Root: both halves move to new pages; the root stays.
                let (a, b) = (self.allocate()?, self.allocate()?);
                self.fill_inner(a, place(Some(&up), b), leftmost, 0..m)?;
                self.fill_inner(b, place(None, NONE), right_leftmost, m + 1..n)?;
                self.s.pv2.reset(a);
                self.s.pv2.push(&up, b);
                let root = Place {
                    level: level + 1,
                    ..place(None, NONE)
                };
                let ok = node::write_inner(&mut pg, root, a, &self.s.pv2, 0..1);
                debug_assert!(ok);
                return Ok(());
            }
            let r = self.allocate()?;
            self.fill_inner(r, place(hk.as_deref(), right), right_leftmost, m + 1..n)?;
            let ok = node::write_inner(&mut pg, place(Some(&up), r), leftmost, &self.s.pv, 0..m);
            debug_assert!(ok, "split point checked to fit");
            child = pg;
            sep = up;
            new_child = r;
            k -= 1;
        }
    }

    /// Writes keys `range` of the scratch pivots to the new inner page `p`.
    fn fill_inner(
        &mut self,
        p: u64,
        at: Place<'_>,
        leftmost: u64,
        range: std::ops::Range<usize>,
    ) -> Result<()> {
        let pin = self.pw.create(p)?;
        let mut g = pin.write()?;
        let ok = node::write_inner(&mut g, at, leftmost, &self.s.pv, range);
        debug_assert!(ok, "split point checked to fit");
        Ok(())
    }

    /// The left sibling of the page at `path[j]`: the right-most page at
    /// its level of the subtree just left of it, or `None` if it is the
    /// left-most page of its level.
    fn left_sibling(&self, path: &[Step], j: usize) -> Result<Option<u64>> {
        let Some(a) = (1..=j).rev().find(|&i| path[i].idx > 0) else {
            return Ok(None);
        };
        // Child `idx - 1` of `path[a - 1]`, then right-most children down.
        let mut p = {
            let pin = self.pw.fetch(path[a - 1].page)?;
            let g = pin.read()?;
            let idx = path[a].idx - 1;
            if idx == 0 {
                node::leftmost(&g)
            } else {
                let mut child = None;
                let mut i = 0;
                node::inner_entries(&g, |_, c| {
                    i += 1;
                    if i == idx {
                        child = Some(c);
                    }
                })
                .map_err(|e| self.corrupt(path[a - 1].page, &e))?;
                child.ok_or_else(|| self.corrupt(path[a - 1].page, "child index out of range"))?
            }
        };
        for _ in a..j {
            let pin = self.pw.fetch(p)?;
            let g = pin.read()?;
            let mut last = node::leftmost(&g);
            node::inner_entries(&g, |_, c| last = c).map_err(|e| self.corrupt(p, &e))?;
            p = last;
        }
        Ok(Some(p))
    }

    /// Frees the empty leaf at the end of `self.s.path`, with the chain of
    /// inner pages above it left without children (TS-05 Underflow): the
    /// top of that chain is removed from its parent, which gives its key
    /// range to its right sibling; each page is unlinked from its left
    /// sibling, keeps its right link and is marked deleted. As in
    /// PostgreSQL, the last child of a parent is kept unless it is the only
    /// one, so neither the root nor the right-most page of a level is ever
    /// freed (R-PG `src/backend/access/nbtree/README`, "Deleting entire
    /// pages").
    fn free_empty_leaf(&mut self, tree: Tree) -> Result<()> {
        let path = std::mem::take(&mut self.s.path);
        let r = self.free_empty_leaf_on(tree, &path);
        self.s.path = path;
        r
    }

    fn free_empty_leaf_on(&mut self, tree: Tree, path: &[Step]) -> Result<()> {
        if path.len() < 2 {
            return Ok(());
        }
        // The top of the chain to free.
        let mut t = path.len() - 1;
        loop {
            let keys = {
                let pin = self.pw.fetch(path[t - 1].page)?;
                let g = pin.read()?;
                node::count(&g)
            };
            if keys == 0 {
                if t == 1 {
                    return Ok(()); // the root keeps its only child
                }
                t -= 1;
                continue;
            }
            if path[t].idx == keys {
                return Ok(()); // the last child of its parent stays
            }
            break;
        }
        let lefts: Vec<Option<u64>> = (t..path.len())
            .map(|j| self.left_sibling(path, j))
            .collect::<Result<_>>()?;
        // Remove the top from its parent: its range passes to the right.
        let parent = path[t - 1].page;
        {
            let pin = self.pw.change(parent)?;
            let mut g = pin.write()?;
            node::inner_decode(&g, &mut self.s.pv)
                .ok_or_else(|| self.corrupt(parent, "malformed inner page"))?;
            self.s.pv.remove_child_into_right(path[t].idx);
            let (level, right) = (node::level(&g), node::right(&g));
            let hk = node::high_key(&g).map(<[u8]>::to_vec);
            let n = self.s.pv.len();
            let leftmost = self.s.pv.child(0);
            let at = Place {
                tree: tree.id,
                level,
                high_key: hk.as_deref(),
                right,
            };
            let ok = node::write_inner(&mut g, at, leftmost, &self.s.pv, 0..n);
            debug_assert!(ok, "removing a key always fits");
        }
        for (j, left) in (t..path.len()).zip(lefts) {
            let p = path[j].page;
            let right = {
                let pin = self.pw.change(p)?;
                let mut g = pin.write()?;
                node::mark_deleted(&mut g);
                node::right(&g)
            };
            if let Some(l) = left {
                let pin = self.pw.change(l)?;
                let mut g = pin.write()?;
                if node::right(&g) != p {
                    return Err(self.corrupt(l, "left sibling does not link to the freed page"));
                }
                node::set_right(&mut g, right);
            }
            self.free_page(p)?;
        }
        Ok(())
    }
}
