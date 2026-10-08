//! Bottom-up bulk build of a B+tree from sorted entries (TS-06 Fill,
//! TS-18 step 4, TS-26 fast-load steps 3, 5 and 6; task #17).
//!
//! 1. **Pages.** Under the commit latch, the entries are written in order
//!    into leaves filled to 90 % of their usable bytes (TS-06), then the
//!    inner levels are built bottom-up, each page also filled to 90 %. Pages
//!    come only from the reserve of unused pages beyond the page count
//!    ("newly reserved pages", TS-26), in allocation order, and are written
//!    straight to the data file, sealed, with page LSN 0: they bypass the
//!    pool and the WAL, as PostgreSQL's `nbtsort.c` writes a new index
//!    (R-PG `src/backend/access/nbtree/nbtsort.c:4-6, 44-56`).
//!    As in `_bt_buildadd` (`nbtsort.c`), one page per level is open at a
//!    time: when a page is full, the next page's number is taken first, so
//!    the full page is written with its right link and high key, and its
//!    downlink is added to the level above. Memory is therefore one page
//!    per level, whatever the tree's size.
//! 2. **`sync_data`** of the data file (TS-26 step 5).
//! 3. **Install.** One page-level commit ([`Engine::commit_latched`]) adds
//!    the root to the directory and writes the next tree identifier and the
//!    page count, which now covers the built pages, to page 0 (TS-26 step
//!    6). Until it is durable, page 0 on disk keeps the earlier page count,
//!    so after a crash the built pages are in the reserve again and no tree
//!    refers to them. InnoDB's bulk loader likewise builds an index's pages
//!    bottom-up, page by page, before the index becomes visible (R-INNO
//!    `storage/innobase/btr/btr0bulk.cc`, `PageBulk::insert` and
//!    `BtrBulk::finish`).
//!
//! The commit latch is held from the first page to the install, so no
//! commit can take a reserved page the build uses or write a page count
//! that covers its pages before the install. Commits wait for the build.
//! TS-18 requires that writes never wait for an index build (Sprint 2); how
//! a build shares the reserve with concurrent commits is not specified
//! (reported as a gap of task #17).
//!
//! A page the build takes may still have a frame in the pool, left by an
//! undone batch (TS-16 step 6 undo, item 3): the build first waits at the
//! reuse fence, as every batch does, and then writes the page with that
//! frame's exclusive latch held, replacing the frame with the new image,
//! clean ([`super::Pool::write_new_page`]). No eviction or checkpoint can
//! write the frame's older image over the new page meanwhile.
//!
//! A bulk build and a backup exclude each other
//! ([`crate::storage::engine::Core::backup_or_load`]): the built pages
//! bypass the WAL, so a backup could not restore them. A build requested
//! while a backup runs waits until the backup ends (TS-26); a backup
//! requested while a build runs fails (FR-OPS-001). The build takes that
//! latch before the commit latch and holds it until the install.

use std::time::Instant;

use super::node::{
    self, Encoder, Entries, MAX_INLINE, MAX_KEY, NONE, OVERFLOW_CAP, Pivots, Place,
    RESTART_INTERVAL, RIGHTMOST_FILL, USABLE, Value,
};
use super::{CommitLatch, Store, Tree, WriteState};
use crate::storage::engine::Engine;
use crate::storage::error::{Error, Result};
use crate::storage::page::{self, PAGE_SIZE};
use crate::tx::commit::Ids;

/// Bytes of the usable page area a built page may fill: 90 % (TS-06).
pub(crate) const FILL_BYTES: usize = (USABLE as f64 * RIGHTMOST_FILL) as usize;

/// A source of entries in strictly increasing key order.
pub(crate) trait SortedEntries {
    /// The next entry, or `None` after the last.
    ///
    /// # Errors
    ///
    /// Errors of the source.
    fn next_entry(&mut self) -> Result<Option<(&[u8], &[u8])>>;
}

impl SortedEntries for crate::storage::sort::Sorted<'_> {
    fn next_entry(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.next_record()
    }
}

/// One entry copied out of the source, its value stored.
struct Held {
    key: Vec<u8>,
    inline: Vec<u8>,
    overflow: Option<(u64, u64)>,
}

impl Held {
    fn value(&self) -> Value<'_> {
        match self.overflow {
            Some((len, first)) => Value::Overflow { len, first },
            None => Value::Inline(&self.inline),
        }
    }
}

/// One inner level in progress: the open page and its downlinks.
struct Level {
    pv: Pivots,
    /// The open page's number; meaningful once `pv` has a left-most child.
    page: u64,
    /// The key that precedes the open page in its parent; `None` for the
    /// first page of the level.
    sep: Option<Vec<u8>>,
    /// Body bytes of the open page's keys.
    size: usize,
    /// Pages of this level written so far.
    written: u64,
    open: bool,
}

/// The pages of a build in progress.
struct Builder<'s, 'l> {
    store: &'s Store<'s>,
    st: &'l mut WriteState,
    id: u64,
    buf: Vec<u8>,
    enc: Encoder,
    levels: Vec<Level>,
    pages: u64,
    entries: u64,
}

impl Builder<'_, '_> {
    /// Takes the next reserved page, extending the file if the reserve is
    /// used up (TS-05), after the reuse fence of an undone batch.
    fn allocate(&mut self) -> Result<u64> {
        let data = self.store.data;
        let p = match data.allocate() {
            Err(e) if e.is_no_space() => {
                data.ensure(1)?;
                data.allocate()?
            }
            r => r?,
        };
        if let Some((batch, fenced)) = self.st.fence.as_ref()
            && fenced.contains(&p)
        {
            let batch = *batch;
            let mut spins = 0u32;
            while self.store.trees.readers.horizon() < batch {
                spins += 1;
                if spins < 64 {
                    crate::sync::thread::yield_now();
                } else {
                    crate::sync::thread::pause(std::time::Duration::from_micros(50));
                }
            }
            self.st.fence = None;
        }
        self.pages += 1;
        Ok(p)
    }

    /// Seals and writes the page image in `buf` at `p`, and replaces a
    /// frame of `p` left in the pool ([`super::Pool::write_new_page`]).
    fn write(&mut self, p: u64) -> Result<()> {
        page::set_lsn(&mut self.buf, 0);
        page::seal(&mut self.buf, p);
        self.store.pool.write_new_page(p, &self.buf)
    }

    /// Writes `value` to a new overflow chain and returns its first page.
    fn overflow(&mut self, value: &[u8]) -> Result<u64> {
        let n = value.len().div_ceil(OVERFLOW_CAP);
        let first = self.allocate()?;
        let mut p = first;
        for (i, chunk) in value.chunks(OVERFLOW_CAP).enumerate() {
            let next = if i + 1 < n { self.allocate()? } else { NONE };
            self.buf.fill(0);
            node::write_overflow(&mut self.buf, self.id, next, chunk);
            self.write(p)?;
            p = next;
        }
        Ok(first)
    }

    /// Copies `(key, value)` out of the source, writing an overflow chain
    /// for a value longer than 2 048 bytes (TS-05).
    fn hold(&mut self, key: &[u8], value: &[u8]) -> Result<Held> {
        if key.len() > MAX_KEY {
            return Err(Error::InvalidArgument("key longer than 1 024 bytes"));
        }
        let overflow = if value.len() > MAX_INLINE {
            Some((value.len() as u64, self.overflow(value)?))
        } else {
            None
        };
        Ok(Held {
            key: key.to_vec(),
            inline: if overflow.is_none() {
                value.to_vec()
            } else {
                Vec::new()
            },
            overflow,
        })
    }

    /// Writes the leaf `page` from `es`, with high key `hk` and right link
    /// `right`, and adds it to level 1 after `sep`, unless it is the only
    /// leaf, which is then the root.
    fn write_leaf(
        &mut self,
        page: u64,
        es: &Entries,
        sep: Option<Vec<u8>>,
        hk: Option<&[u8]>,
        right: u64,
    ) -> Result<()> {
        self.buf.fill(0);
        if es.len() == 0 {
            node::init_empty_leaf(&mut self.buf, self.id, node::Keys::Prefix);
        } else {
            let at = Place {
                tree: self.id,
                level: 0,
                high_key: hk,
                right,
            };
            if !node::write_leaf(&mut self.buf, at, es, 0..es.len(), &mut self.enc) {
                return Err(Error::Corruption(format!(
                    "bulk build: leaf {page} does not fit"
                )));
            }
        }
        self.write(page)?;
        if sep.is_none() && hk.is_none() {
            return Ok(());
        }
        self.add_child(1, sep, page)
    }

    /// Adds the downlink `child`, preceded by key `sep`, to the open page of
    /// inner level `k`, closing that page first if it is full.
    fn add_child(&mut self, k: usize, sep: Option<Vec<u8>>, child: u64) -> Result<()> {
        if self.levels.len() < k {
            self.levels.push(Level {
                pv: Pivots::default(),
                page: 0,
                sep: None,
                size: 0,
                written: 0,
                open: false,
            });
        }
        let i = k - 1;
        if !self.levels[i].open {
            let page = self.allocate()?;
            let l = &mut self.levels[i];
            l.pv.reset(child);
            l.page = page;
            l.sep = sep;
            l.size = 0;
            l.open = true;
            return Ok(());
        }
        let sep = sep.expect("only the first child of a level has no separator");
        // A key takes a slot, its length, the key and the child (node.rs
        // inner layout). The page keeps room for a high key of the largest
        // size, which it gets when the next key closes it.
        let cell = 2 + 2 + sep.len() + 8;
        let l = &self.levels[i];
        if l.size + cell <= FILL_BYTES && l.size + cell + MAX_KEY <= USABLE {
            let l = &mut self.levels[i];
            l.pv.push(&sep, child);
            l.size += cell;
            return Ok(());
        }
        // Close the open page: `sep` becomes its high key and moves up.
        let next = self.allocate()?;
        self.write_inner(i, Some(&sep), next)?;
        let l = &mut self.levels[i];
        let (page, up) = (l.page, l.sep.take());
        l.pv.reset(child);
        l.page = next;
        l.sep = Some(sep);
        l.size = 0;
        l.written += 1;
        self.add_child(k + 1, up, page)
    }

    /// Writes the open page of level index `i` with high key `hk` and right
    /// link `right`.
    fn write_inner(&mut self, i: usize, hk: Option<&[u8]>, right: u64) -> Result<()> {
        let l = &self.levels[i];
        let at = Place {
            tree: self.id,
            level: (i + 1) as u8,
            high_key: hk,
            right,
        };
        self.buf.fill(0);
        let leftmost = l.pv.child(0);
        let page = l.page;
        if !node::write_inner(&mut self.buf, at, leftmost, &l.pv, 0..l.pv.len()) {
            return Err(Error::Corruption(format!(
                "bulk build: inner page {page} does not fit"
            )));
        }
        self.write(page)
    }

    /// Closes every inner level, right-most pages last, and returns the
    /// root: the only page of the highest level, or `leaf` if the tree has
    /// one leaf.
    fn finish(&mut self, leaf: u64) -> Result<u64> {
        let mut k = 0;
        if self.levels.is_empty() {
            return Ok(leaf);
        }
        loop {
            let top = k + 1 == self.levels.len();
            if top && self.levels[k].written == 0 {
                self.write_inner(k, None, NONE)?;
                return Ok(self.levels[k].page);
            }
            self.write_inner(k, None, NONE)?;
            let l = &mut self.levels[k];
            l.written += 1;
            l.open = false;
            let (page, up) = (l.page, l.sep.take());
            self.add_child(k + 2, up, page)?;
            k += 1;
        }
    }
}

/// Builds tree `id` from `source` into reserved pages under the commit
/// latch `st` and returns its root and the number of entries.
fn build_pages(
    store: &Store<'_>,
    st: &mut WriteState,
    id: u64,
    source: &mut dyn SortedEntries,
) -> Result<(u64, u64)> {
    let mut b = Builder {
        store,
        st,
        id,
        buf: vec![0u8; PAGE_SIZE],
        enc: Encoder::default(),
        levels: Vec::new(),
        pages: 0,
        entries: 0,
    };
    let mut es = Entries::default();
    // Body bytes of `es`: entries and restart array.
    let mut size = 0usize;
    let mut page = b.allocate()?;
    let mut sep: Option<Vec<u8>> = None;
    let mut next = match source.next_entry()? {
        Some((k, v)) => Some(b.hold(k, v)?),
        None => None,
    };
    while let Some(e) = next.take() {
        // One entry of lookahead: the high key this leaf gets if `e` is its
        // last entry is the separator of `e` and the entry after it.
        next = match source.next_entry()? {
            Some((k, v)) => {
                if k <= e.key.as_slice() {
                    return Err(Error::InvalidArgument(
                        "bulk build input is not in strictly increasing key order",
                    ));
                }
                Some(b.hold(k, v)?)
            }
            None => None,
        };
        let hk_len = next
            .as_ref()
            .map_or(0, |n| node::separator(&e.key, &n.key).len());
        es.push(&e.key, e.value());
        let i = es.len() - 1;
        let restart = i % RESTART_INTERVAL == 0;
        let cost = es.encoded_len(i, restart) + if restart { 2 } else { 0 };
        if i > 0 && size + cost + hk_len > FILL_BYTES {
            // The leaf is full without `e`: its high key is the separator
            // of its last entry and `e`, whose fit was checked when that
            // entry was added.
            es.remove(i);
            let hk = node::separator(es.key(i - 1), &e.key).to_vec();
            let right = b.allocate()?;
            b.write_leaf(page, &es, sep.take(), Some(&hk), right)?;
            page = right;
            sep = Some(hk);
            es.clear();
            es.push(&e.key, e.value());
            size = es.encoded_len(0, true) + 2;
        } else {
            size += cost;
        }
        b.entries += 1;
    }
    b.write_leaf(page, &es, sep, None, NONE)?;
    let root = b.finish(page)?;
    Ok((root, b.entries))
}

/// Pages of a bulk build written and synced, not yet installed. Dropping it
/// without [`Built::install`] releases the commit latch and gives the pages
/// back to the reserve, as a crash before the install does.
pub(crate) struct Built<'e> {
    engine: &'e Engine,
    st: Option<CommitLatch<'e>>,
    /// The backup-or-load latch, held until the install ends.
    _load: crate::sync::MutexGuard<'e, ()>,
    id: u64,
    root: u64,
    /// The page count before the build.
    before: u64,
    /// Entries in the tree.
    pub(crate) entries: u64,
    /// Pages the build wrote.
    pub(crate) pages: u64,
}

impl Built<'_> {
    /// Installs the tree with one page-level commit (TS-26 step 6) and
    /// returns it once that commit is durable.
    ///
    /// # Errors
    ///
    /// As [`Engine::commit`]; on failure the pages return to the reserve.
    pub(crate) fn install(mut self) -> Result<Tree> {
        let started = Instant::now();
        let st = self.st.take().expect("held until install");
        let (id, root, before) = (self.id, self.root, self.before);
        let engine = self.engine;
        let core = engine.core();
        let r = engine.commit_latched(st, Ids::default(), |store, st, wal, _ts| {
            let r = core
                .data
                .ensure(store.estimate(&[], 1)?)
                .and_then(|()| store.install_tree(st, wal, id, root));
            match r {
                Ok(t) => Ok(vec![t]),
                Err(e) => {
                    // The undo restored the count after the build.
                    store.data.rollback_page_count(before);
                    Err(e)
                }
            }
        });
        engine.note_commit_result(started, &r);
        Ok(r?.trees[0])
    }
}

impl Drop for Built<'_> {
    fn drop(&mut self) {
        if self.st.is_some() {
            self.engine.core().data.rollback_page_count(self.before);
        }
    }
}

impl Engine {
    /// Builds a new tree from `source`, whose keys are strictly increasing,
    /// and installs it (TS-26 fast-load steps 3, 5 and 6; module
    /// documentation).
    ///
    /// # Errors
    ///
    /// As [`Engine::bulk_build_pages`] and [`Built::install`].
    pub(crate) fn bulk_build(&self, source: &mut dyn SortedEntries) -> Result<Tree> {
        self.bulk_build_pages(source)?.install()
    }

    /// Steps 1 and 2 of a bulk build: the pages, written and synced, under
    /// the backup-or-load latch and the commit latch, which the result holds
    /// until it is installed or dropped. Waits while a backup runs.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for keys out of order or longer than
    /// 1 024 bytes; ENOSPC; errors of the source; [`Error::Io`] for a
    /// failed write or sync of the data file, which sets the unusable
    /// latch. On any failure the pages return to the reserve.
    pub(crate) fn bulk_build_pages(&self, source: &mut dyn SortedEntries) -> Result<Built<'_>> {
        let core = self.core();
        core.health.check()?;
        let load = core.backup_or_load();
        let store = core.store();
        let mut st = store.lock();
        let before = core.data.page_count();
        let id = store.draw_tree_id();
        let r = build_pages(&store, &mut st, id, source).and_then(|(root, entries)| {
            core.data.sync()?;
            Ok((root, entries))
        });
        match r {
            Ok((root, entries)) => Ok(Built {
                engine: self,
                st: Some(st),
                _load: load,
                id,
                root,
                before,
                entries,
                pages: core.data.page_count() - before,
            }),
            Err(e) => {
                core.data.rollback_page_count(before);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests;
