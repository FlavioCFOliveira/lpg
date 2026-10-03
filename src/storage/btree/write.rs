//! The writer: applies a batch of puts and deletes (TS-16 step 6),
//! splitting and freeing pages (TS-05, TS-06), and undoes a batch that
//! fails.
//!
//! **Undo** (TS-16 step 6, user decision 2026-10-02, option A). Pages are
//! changed in place. Before its first change in a batch, each existing page
//! is copied ([`PageWriter::mark`]); a page the batch takes needs no copy
//! when it is free before and after the batch. A taken page that the undo
//! makes live again is copied before it is taken:
//! a page this batch freed ([`PageWriter::create`]) and an emptied
//! free-list trunk ([`crate::storage::freelist::FreeList::allocate`]).
//! If anything fails, the copies are written back, newest first change
//! first, and the in-memory state the batch changed is restored: the free
//! list (from its journal), the next tree identifier, the data file's page
//! count and the open group's frame positions. The pages the batch took
//! return to the free list or the reserve; the next batch that takes one of
//! them first waits for every tree read operation that started before the
//! undo. This is SQLite's rollback journal kept in memory (R-SQLITE
//! `src/pager.c`, `pagerAddPageToRollbackJournal` before a page's first
//! change, `pager_playback_one_page` on rollback) and the effect of LMDB's
//! abort, which discards changed copies (R-LMDB `libraries/liblmdb/mdb.c`,
//! `mdb_page_touch`, `mdb_txn_abort`).
//!
//! ENOSPC and memory-limit failures leave the database usable
//! (FR-REC-007, FR-LIM-005, FR-LIM-007); any other failure is undone the
//! same way and also sets the unusable latch (FR-REC-005, FR-REC-006).

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use super::node::{self, Applied, Change, MAX_INLINE, MAX_KEY, NONE, OVERFLOW_CAP, Place, Value};
use super::{
    DIRECTORY, LogState, PageWriter, Saved, Scratch, Step, Store, Tree, Undo, Write, WriteState,
};
use crate::storage::datafile::Header;
use crate::storage::error::{Error, Result};
use crate::storage::freelist::{FreeList, Readers};
use crate::storage::page::{self, PageType};
use crate::storage::pool::PinnedPage;
use crate::storage::wal::Wal;

/// Page numbers one free-list trunk holds (TS-05: 8 KiB less a 40-byte
/// header, 8 bytes each).
const TRUNK_ENTRIES: u64 = 1019;

impl Store<'_> {
    /// Height of `tree`: the root's level + 1.
    fn height(&self, tree: Tree) -> Result<u64> {
        let pin = self.pool.fetch(tree.root)?;
        let g = pin.read()?;
        Ok(u64::from(node::level(&g)) + 1)
    }

    /// The TS-05 estimate E of the pages a batch takes, for a batch that
    /// first creates `new_trees` trees and then applies `batch`:
    ///
    /// E = 2 × (⌈B / 4 KiB⌉ + T) + Σ h + Σ over trees deleted from (h + 1)
    /// + ⌈d / 1 019⌉,
    ///
    /// with each new tree counted as one 16-byte directory entry and one
    /// root page. E is not a bound (TS-05); the apply meets an overrun.
    ///
    /// # Errors
    ///
    /// Errors of [`super::super::pool::Pool::fetch`] while reading tree
    /// heights.
    pub(crate) fn estimate(&self, batch: &[Write<'_>], new_trees: usize) -> Result<u64> {
        let mut trees: Vec<Tree> = batch.iter().map(|w| w.tree).collect();
        if new_trees > 0 {
            trees.push(DIRECTORY);
        }
        trees.sort_unstable_by_key(|t| t.id);
        trees.dedup();
        let mut deleted: Vec<Tree> = batch
            .iter()
            .filter(|w| w.value.is_none())
            .map(|w| w.tree)
            .collect();
        deleted.sort_unstable_by_key(|t| t.id);
        deleted.dedup();
        let bytes: u64 = batch
            .iter()
            .map(|w| (w.key.len() + w.value.map_or(0, <[u8]>::len)) as u64)
            .sum::<u64>()
            + 16 * new_trees as u64;
        let deletes = batch.iter().filter(|w| w.value.is_none()).count() as u64;
        let mut e = 2 * (bytes.div_ceil(4096) + trees.len() as u64) + new_trees as u64;
        for &t in &trees {
            e += self.height(t)?;
        }
        for &t in &deleted {
            e += self.height(t)? + 1;
        }
        Ok(e + deletes.div_ceil(TRUNK_ENTRIES))
    }

    /// Applies `batch` in order, as one write batch, without the WAL: the
    /// tree tests and the laboratory. Commits go through
    /// [`crate::tx::commit`].
    ///
    /// Before any page changes: every key is checked against the 1 024-byte
    /// limit (TS-05), and the reserve of unused pages is made to cover the
    /// estimate. A failure after the first change is undone.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key over 1 024 bytes, with nothing
    /// changed; ENOSPC or [`Error::MemoryLimit`], undone, with the database
    /// usable (FR-REC-007); any other error, undone, which also sets the
    /// unusable latch.
    #[cfg(test)]
    pub(crate) fn apply(&self, batch: &[Write<'_>]) -> Result<()> {
        self.health.check()?;
        self.data.ensure(self.estimate(batch, 0)?)?;
        let mut st = self.lock();
        self.apply_batch(&mut st, None, batch, 0).map(drop)
    }

    /// Creates an empty tree with the next identifier and records its root
    /// in the directory (TS-05), without the WAL (tests).
    ///
    /// # Errors
    ///
    /// As [`Store::apply`].
    #[cfg(test)]
    pub(crate) fn create_tree(&self) -> Result<Tree> {
        self.health.check()?;
        self.data.ensure(self.estimate(&[], 1)?)?;
        let mut st = self.lock();
        let trees = self.apply_batch(&mut st, None, &[], 1)?;
        Ok(trees[0])
    }

    /// TS-16 step 6 under the commit latch `st`: creates `new_trees` empty
    /// trees, then applies `batch`, as one batch. With `wal`, every changed
    /// page takes its frame position in the open group, the segments the
    /// group will reach are made to exist, and the final page images join
    /// the group. Any failure is undone. Returns the trees created.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key over 1 024 bytes, with nothing
    /// changed; ENOSPC or [`Error::MemoryLimit`], undone, with the database
    /// usable; any other error, undone, with the unusable latch set.
    pub(crate) fn apply_batch(
        &self,
        st: &mut WriteState,
        wal: Option<&Wal>,
        batch: &[Write<'_>],
        new_trees: usize,
    ) -> Result<Vec<Tree>> {
        self.health.check()?;
        if batch.iter().any(|w| w.key.len() > MAX_KEY) {
            return Err(Error::InvalidArgument("key longer than 1 024 bytes"));
        }
        let mut created = Vec::with_capacity(new_trees);
        self.run(st, wal, |a| {
            for _ in 0..new_trees {
                created.push(a.create_tree()?);
            }
            for w in batch {
                match w.value {
                    Some(v) => a.put(w.tree, w.key, v)?,
                    None => a.delete(w.tree, w.key)?,
                }
            }
            Ok(())
        })?;
        Ok(created)
    }

    /// Runs one write batch under the write latch `st`, undoing it on
    /// failure.
    fn run(
        &self,
        st: &mut WriteState,
        wal: Option<&Wal>,
        body: impl FnOnce(&mut Apply<'_>) -> Result<()>,
    ) -> Result<()> {
        let batch = self.trees.applied.load(Ordering::Relaxed) + 1;
        let saved_tree_id = st.next_tree_id;
        let saved_page_count = self.data.page_count();
        let group_mark = st.log.group.mark();
        // Once durable, every commit that freed a page this batch can take
        // is durable (TS-16 step 6 undo, item 3).
        let freed_durable = if wal.is_some() {
            st.log.group.joined_durable_lsn()
        } else {
            0
        };
        st.free.begin();
        st.undo.clear();
        let WriteState {
            free,
            next_tree_id,
            undo,
            scratch,
            log,
            fence,
        } = st;
        let r = {
            let mut a = Apply {
                pw: PageWriter {
                    pool: self.pool,
                    data: self.data,
                    undo,
                    header: false,
                    group: wal.map(|_| &mut log.group),
                },
                free,
                next_tree_id,
                s: scratch,
                readers: &self.trees.readers,
                batch,
                store: *self,
                fence,
            };
            body(&mut a).and_then(|()| a.finish())
        };
        // The segments this group will reach exist before the commit joins
        // it, so that ENOSPC there fails only this commit (TS-09). Then the
        // latch is checked again: a failure elsewhere while this batch ran
        // (another thread's checksum or I/O error) fails it (TS-12), before
        // any image enters the group.
        let r = r
            .and_then(|()| wal.map_or(Ok(()), |w| w.ensure_through(log.group.end())))
            .and_then(|()| self.health.check())
            .and_then(|()| {
                if wal.is_some() {
                    self.capture(undo, log)
                } else {
                    Ok(())
                }
            });
        match r {
            Ok(()) => {
                undo.clear();
                // Readers that start from now on see this batch applied.
                self.trees.applied.store(batch, Ordering::SeqCst);
                Ok(())
            }
            Err(e) => {
                // Every error exit undoes the whole step before the latch is
                // released, so the open group never keeps a position of this
                // batch: the writer, which closes the group under the same
                // latch, can only see committed images. PostgreSQL gets the
                // same guarantee from its critical section, where any error
                // is a PANIC because "the shared buffers will contain
                // unlogged changes" (R-PG `src/backend/access/transam/README:441-446`).
                //
                // Pages first: an I/O or checksum failure also sets the
                // unusable latch, but only after the pages are restored
                // (TS-16 step 6). The caller sees the failure that caused
                // the undo, not the latch, as SQLite's `pager_error` returns
                // the original code after entering its error state (R-SQLITE
                // `src/pager.c:1996-2010`). A page that cannot be restored
                // leaves the database unusable.
                let restored = self.undo(undo, wal.is_some(), freed_durable);
                let e = if restored.is_ok()
                    && (e.is_no_space() || matches!(e, Error::MemoryLimit { .. }))
                {
                    e
                } else {
                    self.health.fail(e)
                };
                free.rollback();
                *next_tree_id = saved_tree_id;
                self.data.rollback_page_count(saved_page_count);
                log.group.rollback(group_mark);
                extend_fence(fence, batch, &undo.taken);
                undo.clear();
                // The fence's batch number: readers registered from now on
                // started after the undo.
                self.trees.applied.store(batch, Ordering::SeqCst);
                if let Some(w) = wal {
                    w.note_undo();
                }
                Err(e)
            }
        }
    }

    /// Copies the final image of every page the batch changed into the
    /// open group (TS-09 Capture). The pages are dirty with a page LSN above
    /// the durable LSN, so they are still in the pool.
    ///
    /// All pages are pinned and latched before the first image is copied,
    /// so a failure leaves every frame slot of the group unchanged: a slot
    /// of a page that an earlier commit of the same group changed keeps that
    /// commit's image.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] if a changed page is no longer in the pool;
    /// [`Error::Unusable`] for a poisoned latch.
    fn capture(&self, undo: &Undo, log: &mut LogState) -> Result<()> {
        let pins = undo
            .order
            .iter()
            .map(|&p| {
                self.pool.resident(p).map(|(pin, _)| pin).ok_or_else(|| {
                    Error::Corruption(format!("page {p} changed by the batch left the pool"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let guards = pins
            .iter()
            .map(|pin| pin.read_for_capture())
            .collect::<Result<Vec<_>>>()?;
        for (&p, g) in undo.order.iter().zip(&guards) {
            log.group.capture(p, g);
        }
        Ok(())
    }

    /// Puts every changed page back, newest first change first: a copied
    /// page gets its image and dirty state from before the batch; a taken
    /// page that is free before and after the batch keeps the image the
    /// batch wrote, which
    /// reaches the data file once every commit that freed the page is
    /// durable, `freed_durable` (see [`PageWriter::create`] and
    /// [`crate::storage::pool::PinnedPage::undo_keep`]). No page LSN is
    /// lowered below its value before the batch.
    ///
    /// A changed page of a logged batch is still in the pool: its page LSN
    /// is above the durable LSN, so eviction cannot take it. It is restored
    /// there even when the failure being undone has already set the
    /// unusable latch (a failed read of another page). A page of an
    /// unlogged batch (tests) may have been evicted and is read back.
    ///
    /// # Errors
    ///
    /// The first page that cannot be restored; the restore continues with
    /// the other pages, and the caller sets the unusable latch.
    fn undo(&self, undo: &Undo, logged: bool, freed_durable: u64) -> Result<()> {
        let mut first = Ok(());
        for &p in undo.order.iter().rev() {
            let restore = || -> Result<()> {
                match undo.touched[&p] {
                    Saved::Copy(i) => {
                        let pin = match self.pool.resident(p) {
                            Some((pin, _)) => pin,
                            None if logged => {
                                return Err(Error::Corruption(format!(
                                    "page {p} changed by the batch left the pool"
                                )));
                            }
                            None => self.pool.fetch(p)?,
                        };
                        let (image, dirty) = &undo.copies[i];
                        pin.undo_restore(image, *dirty)
                    }
                    // Not in the pool: an unlogged batch's page, which
                    // eviction wrote with the batch's image (page LSN 0).
                    Saved::Free => match self.pool.resident(p) {
                        Some((pin, _)) => pin.undo_keep(freed_durable),
                        None => Ok(()),
                    },
                }
            };
            if let Err(e) = restore()
                && first.is_ok()
            {
                first = Err(e);
            }
        }
        first
    }
}

/// After an undo, the pages the undone batch took join the reuse fence
/// (TS-16 step 6 undo, item 3). An earlier fence still in force is kept:
/// its pages stay fenced, and the fence waits for readers that started
/// before the later undo, the higher batch number, which covers both.
pub(crate) fn extend_fence(fence: &mut Option<(u64, HashSet<u64>)>, batch: u64, taken: &[u64]) {
    if taken.is_empty() {
        return;
    }
    match fence {
        Some((b, pages)) => {
            *b = (*b).max(batch);
            pages.extend(taken.iter().copied());
        }
        None => *fence = Some((batch, taken.iter().copied().collect())),
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
    fence: &'a mut Option<(u64, HashSet<u64>)>,
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

    /// Creates an empty tree with the next identifier and records its root
    /// in the directory (TS-05).
    fn create_tree(&mut self) -> Result<Tree> {
        let id = *self.next_tree_id;
        let root = self.allocate()?;
        {
            let pin = self.pw.create(root)?;
            node::init_empty_leaf(&mut pin.write()?, id);
        }
        self.put(DIRECTORY, &id.to_be_bytes(), &root.to_le_bytes())?;
        *self.next_tree_id = id + 1;
        self.pw.header_changed();
        Ok(Tree { id, root })
    }

    /// Takes a page for this batch (TS-05: free pages first). A page that
    /// an undone batch had taken is used only once every tree read
    /// operation that started before the undo has ended.
    pub(super) fn allocate(&mut self) -> Result<u64> {
        let horizon = self.readers.horizon();
        let p = self.free.allocate(horizon, &mut self.pw)?;
        if let Some((batch, pages)) = self.fence.as_ref()
            && pages.contains(&p)
        {
            let batch = *batch;
            let mut spins = 0u32;
            #[cfg(test)]
            if self.readers.horizon() < batch {
                self.store.trees.fence_waits.fetch_add(1, Ordering::SeqCst);
            }
            while self.readers.horizon() < batch {
                spins += 1;
                if spins < 64 {
                    std::thread::yield_now();
                } else {
                    std::thread::sleep(std::time::Duration::from_micros(50));
                }
            }
            *self.fence = None;
        }
        self.pw.undo.taken.push(p);
        Ok(p)
    }

    /// Frees page `p`, which this batch has unlinked.
    fn free_page(&mut self, p: u64) -> Result<()> {
        let horizon = self.readers.horizon();
        self.pw.undo.freed.insert(p);
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
        self.pw.mark(leaf)?;
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
            self.pw.mark(parent)?;
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
