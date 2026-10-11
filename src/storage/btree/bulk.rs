//! Bottom-up bulk build of a B+tree from sorted entries (TS-06 Fill,
//! TS-18 step 4, TS-26 fast-load steps 3, 5 and 6; tasks #17, #33, #34).
//!
//! 1. **Pages.** The entries are written in order into leaves filled to
//!    90 % of their usable bytes (TS-06), then the inner levels are built
//!    bottom-up, each page also filled to 90 %. The pages are written
//!    straight to the data file, sealed, with page LSN 0, with no latch
//!    held: they bypass the pool and the WAL, as PostgreSQL's `nbtsort.c`
//!    writes a new index (R-PG `src/backend/access/nbtree/nbtsort.c:4-6,
//!    44-56`). As in `_bt_buildadd` (`nbtsort.c`), one page per level is
//!    open at a time: when a page is full, the next page's number is taken
//!    first, so the full page is written with its right link and high key,
//!    and its downlink is added to the level above. Memory is therefore one
//!    page per level, whatever the tree's size.
//! 2. **`sync_data`** of the data file (TS-26 step 5).
//! 3. **Install.** One page-level commit ([`Engine::commit_latched`]) adds
//!    the root to the directory, or copies it into the root of the tree it
//!    replaces ([`Built::install_replacing`]), and leaves in the build's
//!    record only the pages the tree does not use. InnoDB's bulk loader
//!    likewise builds an index's pages bottom-up, page by page, before the
//!    index becomes visible (R-INNO `storage/innobase/btr/btr0load.cc`,
//!    `Page_load`).
//!
//! **Page ownership.** A build takes its pages from the reserve in runs of
//! consecutive pages. Each run is one short page-level commit
//! ([`Store::take_build_run`]): under the commit latch it raises the page
//! count past the run, after the reuse fence of an undone batch (TS-16 step
//! 6 undo, item 3), and records the run in the build's **record**, a
//! directory entry whose key is the tree identifier, 8 bytes big-endian,
//! followed by the byte 1, and whose value is the build's ranges of pages,
//! each its first page and its length, 8 bytes little-endian each. The
//! pages of a run are then written with no latch held: no commit can take
//! them, and a commit's undo, which restores the page count under the
//! latch, cannot give them back to the reserve. Commits therefore wait for
//! a build only during these short commits, whose work does not depend on
//! the tree's size. A run has 64 pages, or an eighth of the pages the build
//! has taken if that is more: a build of `n` pages takes O(log n) runs and
//! leaves at most an eighth of its pages, plus 63, unused. This is
//! InnoDB's split: the bulk loader allocates each page in a mini-
//! transaction of its own that is redo-logged, while the page contents are
//! not (R-INNO `storage/innobase/btr/btr0load.cc:310-345`,
//! `Page_load::init`).
//!
//! **Release** ([`Engine::release_build`]). The pages of a record that no
//! tree uses are given back in short page-level commits: a range that ends
//! at the page count returns to the reserve by lowering the page count;
//! otherwise up to [`RELEASE_PAGES`] pages at its end get a valid image (an
//! empty leaf), written and synced, and then enter the free list, so every
//! page below the page count keeps a valid image (TS-28 migration reads
//! them all). The record shrinks with each commit and is removed with the
//! last. A release runs after an install, for the unused pages; when a
//! build fails or its result is dropped, for all of them; at open, for
//! every record left by a crash (a crash at any point leaves the pages of
//! a build owned by its record, never leaked); and at close. InnoDB drops
//! an index whose creation a crash interrupted when it recovers (R-INNO
//! `storage/innobase/dict/dict0load.cc:1883-1887`), and frees a segment
//! step by step, one mini-transaction each (`fsp/fsp0fsp.cc:3594-3599`,
//! `fseg_free_step`).
//!
//! A page the build takes may still have a frame in the pool, left by an
//! undone batch (TS-16 step 6 undo, item 3): the run's commit first waits
//! at the reuse fence, as every batch does, and the build then writes the
//! page with that frame's exclusive latch held, replacing the frame with
//! the new image, clean ([`super::Pool::write_new_page`]). No eviction or
//! checkpoint can write the frame's older image over the new page
//! meanwhile.
//!
//! **Backups.** The built pages, and the images a release writes, bypass
//! the WAL, so a backup's WAL copy cannot restore them
//! ([`crate::ops::backup`]).
//!
//! - The bulk build of a fast initial load ([`Engine::bulk_build_pages`])
//!   and a backup exclude each other for their whole run
//!   ([`crate::storage::engine::Core::backup_or_load`]): a load requested
//!   while a backup runs waits until the backup ends (TS-26); a backup
//!   requested while a load runs fails (FR-OPS-001). The load takes that
//!   latch before its first run and holds it until it is installed or
//!   dropped.
//! - Every other build (an index or a constraint's owned index,
//!   [`Engine::bulk_build_pages_as`]) runs during a backup. Its install
//!   commit and each release step, which writes its images, syncs and
//!   commits, hold the backup-or-install latch shared
//!   ([`crate::storage::engine::Core::backup_or_install`]), which a backup
//!   holds exclusively for its whole run: neither commit falls in a
//!   backup's WAL range, so a restored database holds either the installed
//!   tree, its pages copied after their sync, or the build record, which
//!   its open releases. The runs' commits are logged like any other.

use std::time::Instant;

use super::node::{
    self, Encoder, Entries, MAX_INLINE, MAX_KEY, NONE, OVERFLOW_CAP, Pivots, Place,
    RESTART_INTERVAL, RIGHTMOST_FILL, USABLE, Value,
};
use super::{DIRECTORY, Keys, Store, Tree, Write};
use crate::storage::engine::Engine;
use crate::storage::error::{Error, Result};
use crate::storage::freelist::TRUNK_CAP;
use crate::storage::page::{self, PAGE_SIZE};
use crate::tx::commit::Ids;

/// Bytes of the usable page area a built page may fill: 90 % (TS-06).
pub(crate) const FILL_BYTES: usize = (USABLE as f64 * RIGHTMOST_FILL) as usize;

/// Pages of a build's first run, and its smallest run.
pub(crate) const MIN_RUN: u64 = 64;

/// Pages a release commit frees at most: one free-list trunk's worth.
pub(crate) const RELEASE_PAGES: u64 = TRUNK_CAP as u64;

/// The ranges of pages of a build record: first page and length of each.
pub(crate) type Ranges = Vec<(u64, u64)>;

/// The byte after the tree identifier in the key of a build record.
const RECORD_TAG: u8 = 1;

/// The directory key of the build record of tree `owner`.
pub(crate) fn build_key(owner: u64) -> [u8; 9] {
    let mut k = [RECORD_TAG; 9];
    k[..8].copy_from_slice(&owner.to_be_bytes());
    k
}

/// The tree whose build record has directory key `key`; `None` for any
/// other key.
pub(crate) fn decode_build_key(key: &[u8]) -> Option<u64> {
    match key {
        [id @ .., RECORD_TAG] if id.len() == 8 => {
            Some(u64::from_be_bytes(id.try_into().expect("8 bytes")))
        }
        _ => None,
    }
}

/// The value of a build record holding `ranges`.
pub(crate) fn encode_ranges(ranges: &[(u64, u64)]) -> Vec<u8> {
    let mut v = Vec::with_capacity(16 * ranges.len());
    for &(first, len) in ranges {
        v.extend_from_slice(&first.to_le_bytes());
        v.extend_from_slice(&len.to_le_bytes());
    }
    v
}

/// The ranges of a build record's value; `None` if it is malformed.
pub(crate) fn decode_ranges(v: &[u8]) -> Option<Vec<(u64, u64)>> {
    if v.is_empty() || !v.len().is_multiple_of(16) {
        return None;
    }
    let u = |b: &[u8]| u64::from_le_bytes(b.try_into().expect("8 bytes"));
    let ranges: Vec<(u64, u64)> = v.chunks(16).map(|c| (u(&c[..8]), u(&c[8..]))).collect();
    ranges
        .iter()
        .all(|&(f, l)| l > 0 && f > 0 && f.checked_add(l).is_some())
        .then_some(ranges)
}

/// The information the install commit of a bulk build needs: the tree,
/// its built root, and the build's pages the tree does not use, which its
/// record keeps until they are released.
#[derive(Debug, Clone)]
pub(crate) struct Install {
    /// The tree identifier.
    pub(crate) id: u64,
    /// The built root.
    pub(crate) root: u64,
    /// The unused pages, as ranges.
    pub(crate) tail: Vec<(u64, u64)>,
}

/// The pages a build owns: its record's ranges and the run in use. Dropping
/// it releases whatever the record still holds ([`Engine::release_build`]).
struct Lease<'e> {
    engine: &'e Engine,
    owner: u64,
    /// The record's ranges, as committed.
    ranges: Vec<(u64, u64)>,
    /// The next unused page of the current run, and the run's end.
    next: u64,
    end: u64,
    /// Pages handed out so far.
    used: u64,
}

impl<'e> Lease<'e> {
    fn new(engine: &'e Engine, owner: u64) -> Lease<'e> {
        Lease {
            engine,
            owner,
            ranges: Vec::new(),
            next: 0,
            end: 0,
            used: 0,
        }
    }

    /// The next page the build owns, taking a new run when the current one
    /// is used up.
    fn allocate(&mut self) -> Result<u64> {
        if self.next == self.end {
            self.take_run()?;
        }
        let p = self.next;
        self.next += 1;
        self.used += 1;
        Ok(p)
    }

    /// Takes a run of reserved pages with one page-level commit (module
    /// documentation). The reserve is made to cover it before the commit
    /// latch is taken; an overrun extends the file under it.
    fn take_run(&mut self) -> Result<()> {
        let n = MIN_RUN.max(self.used / 8);
        let engine = self.engine;
        let core = engine.core();
        let store = core.store();
        let key = build_key(self.owner);
        let value = vec![0u8; 16 * (self.ranges.len() + 1)];
        let w = Write {
            tree: DIRECTORY,
            key: &key,
            value: Some(&value),
            stamp: 0,
        };
        core.data.ensure(n + store.estimate(&[w], 0)?)?;
        let started = Instant::now();
        let mut ranges = self.ranges.clone();
        let mut first = 0;
        let r = engine.commit_latched(store.lock(), Ids::default(), |store, st, wal, _| {
            first = store.take_build_run(st, wal, self.owner, n, &mut ranges)?;
            Ok(Vec::new())
        });
        engine.note_commit_result(started, &r);
        r?;
        self.ranges = ranges;
        self.next = first;
        self.end = first + n;
        Ok(())
    }

    /// The pages of the record that the build has not handed out: the rest
    /// of the current run.
    fn tail(&self) -> Vec<(u64, u64)> {
        if self.next < self.end {
            vec![(self.next, self.end - self.next)]
        } else {
            Vec::new()
        }
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        // A failure leaves the record, whose pages the next open releases.
        let _ = self.engine.release_build(self.owner);
    }
}

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
struct Builder<'s, 'l, 'e> {
    store: &'s Store<'s>,
    lease: &'l mut Lease<'e>,
    id: u64,
    /// How the leaves store their keys.
    keys: Keys,
    buf: Vec<u8>,
    enc: Encoder,
    levels: Vec<Level>,
    pages: u64,
    entries: u64,
}

impl Builder<'_, '_, '_> {
    /// Takes the next page the build owns ([`Lease::allocate`]).
    fn allocate(&mut self) -> Result<u64> {
        self.pages += 1;
        self.lease.allocate()
    }

    /// Seals and writes the page image in `buf` at `p`, with no latch held,
    /// and replaces a frame of `p` left in the pool
    /// ([`super::Pool::write_new_page`]).
    fn write(&mut self, p: u64) -> Result<()> {
        page::set_lsn(&mut self.buf, 0);
        page::seal(&mut self.buf, p);
        self.store.pool.write_new_page(p, &self.buf)?;
        #[cfg(test)]
        crate::storage::wal::TestPoints::run(&self.lease.engine.core().wal.points.bulk_page);
        Ok(())
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
            node::init_empty_leaf(&mut self.buf, self.id, self.keys);
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

/// Builds tree `id` from `source` into pages of `lease`, its leaves storing
/// keys as `keys`, and returns its root, the number of entries and the
/// number of pages.
fn build_pages(
    store: &Store<'_>,
    lease: &mut Lease<'_>,
    id: u64,
    keys: Keys,
    source: &mut dyn SortedEntries,
) -> Result<(u64, u64, u64)> {
    let mut b = Builder {
        store,
        lease,
        id,
        keys,
        buf: vec![0u8; PAGE_SIZE],
        enc: Encoder::default(),
        levels: Vec::new(),
        pages: 0,
        entries: 0,
    };
    let mut es = Entries::default();
    es.set_keys(keys);
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
    Ok((root, b.entries, b.pages))
}

/// Pages of a bulk build written and synced, not yet installed. Dropping it
/// without [`Built::install`] releases its pages, as the open after a crash
/// before the install does.
pub(crate) struct Built<'e> {
    /// Dropped first, while the backup-or-load latch is still held.
    lease: Lease<'e>,
    /// The backup-or-load latch of a fast initial load, held until the
    /// build is installed or dropped; `None` for an online build.
    _load: Option<crate::sync::MutexGuard<'e, ()>>,
    id: u64,
    root: u64,
    /// Entries in the tree.
    pub(crate) entries: u64,
    /// Pages of the tree.
    pub(crate) pages: u64,
}

impl Built<'_> {
    /// What the install commit needs ([`Install`]).
    pub(crate) fn install_args(&self) -> Install {
        Install {
            id: self.id,
            root: self.root,
            tail: self.lease.tail(),
        }
    }

    /// Installs the tree with one page-level commit (TS-26 step 6), then
    /// releases the build's unused pages, and returns the tree once the
    /// install is durable.
    ///
    /// # Errors
    ///
    /// As [`Engine::commit`]; on failure the build's pages are released.
    pub(crate) fn install(self) -> Result<Tree> {
        Ok(self
            .install_with(None, || true)?
            .expect("an unconditional install installs"))
    }

    /// [`Built::install`] into `old`, the existing tree of the same
    /// identifier, in one page-level commit, if `still` holds under the
    /// commit latch; returns `None`, and releases the build's pages,
    /// otherwise. `old` keeps its root page, which gets the built root's
    /// contents; its other pages and the built root's page are freed
    /// (TS-05: a root never moves). An index's tree when its online build
    /// ends (TS-18 step 4, [`crate::index`]); nothing reads `old`
    /// meanwhile.
    ///
    /// # Errors
    ///
    /// As [`Built::install`].
    pub(crate) fn install_replacing(
        self,
        old: Tree,
        still: impl FnOnce() -> bool,
    ) -> Result<Option<Tree>> {
        debug_assert_eq!(old.id(), self.id);
        self.install_with(Some(old), still)
    }

    fn install_with(self, old: Option<Tree>, still: impl FnOnce() -> bool) -> Result<Option<Tree>> {
        let started = Instant::now();
        let engine = self.lease.engine;
        let core = engine.core();
        let store = core.store();
        let install = self.install_args();
        core.data.ensure(store.estimate(&[], 1)?)?;
        let mut gone = false;
        // Not in a backup's WAL range (module documentation); released
        // before the lease's release steps take it again.
        let installs = core.install_step();
        let r = engine.commit_latched(store.lock(), Ids::default(), |store, st, wal, _ts| {
            if !still() {
                gone = true;
                return Ok(Vec::new());
            }
            Ok(vec![store.install_tree(st, wal, &install, old)?])
        });
        drop(installs);
        if !gone {
            engine.note_commit_result(started, &r);
        }
        Ok(r?.trees.first().copied())
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

    /// Steps 1 and 2 of the bulk build of a fast initial load, a tree with
    /// a new identifier: the pages, written and synced, under the
    /// backup-or-load latch, which the result holds until it is installed
    /// or dropped. Waits while a backup runs.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for keys out of order or longer than
    /// 1 024 bytes; ENOSPC; errors of the source; [`Error::Io`] for a
    /// failed write or sync of the data file, which sets the unusable
    /// latch. On any failure the build's pages are released.
    pub(crate) fn bulk_build_pages(&self, source: &mut dyn SortedEntries) -> Result<Built<'_>> {
        let load = self.core().backup_or_load();
        let id = self.store().draw_tree_id();
        self.bulk_build_pages_with(source, id, Keys::Prefix, Some(load))
    }

    /// Steps 1 and 2 of the online bulk build of tree `id`, an index's
    /// tree that [`Built::install_replacing`] replaces or a constraint's
    /// new owned-index tree: as [`Engine::bulk_build_pages`], without the
    /// backup-or-load latch, so a backup runs meanwhile (module
    /// documentation).
    ///
    /// # Errors
    ///
    /// As [`Engine::bulk_build_pages`].
    pub(crate) fn bulk_build_pages_as(
        &self,
        source: &mut dyn SortedEntries,
        id: u64,
    ) -> Result<Built<'_>> {
        self.bulk_build_pages_with(source, id, Keys::Prefix, None)
    }

    /// Steps 1 and 2 of the bulk build of tree `id`, an existing tree of a
    /// graph or an index that a fast initial load fills
    /// ([`Built::install_args`], installed by the load's commit with
    /// [`crate::tx::Transaction::commit_installing_all`]), its leaves
    /// storing keys as `keys` (field-delta leaves for an adjacency tree,
    /// TS-06). The caller holds the backup-or-load latch for the whole load
    /// ([`crate::storage::engine::Core::backup_or_load`]), so the result
    /// holds none.
    ///
    /// # Errors
    ///
    /// As [`Engine::bulk_build_pages`].
    pub(crate) fn bulk_build_pages_into(
        &self,
        source: &mut dyn SortedEntries,
        id: u64,
        keys: Keys,
    ) -> Result<Built<'_>> {
        self.bulk_build_pages_with(source, id, keys, None)
    }

    fn bulk_build_pages_with<'e>(
        &'e self,
        source: &mut dyn SortedEntries,
        id: u64,
        keys: Keys,
        load: Option<crate::sync::MutexGuard<'e, ()>>,
    ) -> Result<Built<'e>> {
        let core = self.core();
        core.health.check()?;
        let store = core.store();
        let mut lease = Lease::new(self, id);
        let (root, entries, pages) = build_pages(&store, &mut lease, id, keys, source)?;
        core.data.sync()?;
        Ok(Built {
            lease,
            _load: load,
            id,
            root,
            entries,
            pages,
        })
    }

    /// Releases the pages of the build record of tree `owner`, if there is
    /// one, in page-level commits of at most [`RELEASE_PAGES`] freed pages
    /// each, each step waiting while a backup runs (module documentation).
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] for a malformed record; errors of the page
    /// writes, the sync and the commits. The record then keeps the pages
    /// not yet released.
    pub(crate) fn release_build(&self, owner: u64) -> Result<()> {
        let core = self.core();
        let store = core.store();
        let key = build_key(owner);
        let mut v = Vec::new();
        let mut buf = vec![0u8; PAGE_SIZE];
        loop {
            // Not in a backup's WAL range (module documentation): the
            // images written below and the commit that frees their pages.
            let _installs = core.install_step();
            if !store.get(DIRECTORY, &key, &mut v)? {
                return Ok(());
            }
            let ranges = decode_ranges(&v)
                .ok_or_else(|| Error::Corruption(format!("build record of tree {owner}")))?;
            let &(f, l) = ranges.last().expect("a decoded record holds a range");
            // Only the latch decides whether the range ends at the page
            // count; this guess saves the writes when it does.
            let freed = if f + l == core.data.page_count() {
                None
            } else {
                let k = l.min(RELEASE_PAGES);
                for p in f + l - k..f + l {
                    buf.fill(0);
                    node::init_empty_leaf(&mut buf, owner, Keys::Prefix);
                    page::set_lsn(&mut buf, 0);
                    page::seal(&mut buf, p);
                    store.pool.write_new_page(p, &buf)?;
                }
                core.data.sync()?;
                Some(k)
            };
            let w = Write {
                tree: DIRECTORY,
                key: &key,
                value: Some(&v),
                stamp: 0,
            };
            // The freed pages fill at most one new free-list trunk.
            core.data.ensure(store.estimate(&[w], 0)? + 1)?;
            let started = Instant::now();
            let r = self.commit_latched(store.lock(), Ids::default(), |store, st, wal, _| {
                store.release_build_step(st, wal, owner, &ranges, freed)?;
                Ok(Vec::new())
            });
            self.note_commit_result(started, &r);
            r?;
        }
    }

    /// Releases the pages of every build record (at open and close;
    /// [`Engine::release_build`]).
    ///
    /// # Errors
    ///
    /// As [`Engine::release_build`].
    pub(crate) fn release_builds(&self) -> Result<()> {
        for (owner, _) in self.store().build_records()? {
            self.release_build(owner)?;
        }
        Ok(())
    }
}

impl Store<'_> {
    /// Every build record of the directory: the tree and its ranges.
    ///
    /// # Errors
    ///
    /// As [`Store::scan_prefix`]; [`Error::Corruption`] for a malformed
    /// record.
    pub(crate) fn build_records(&self) -> Result<Vec<(u64, Ranges)>> {
        let mut out = Vec::new();
        let mut bad = false;
        self.scan_prefix(DIRECTORY, b"", |k, v| {
            if let Some(owner) = decode_build_key(k) {
                match decode_ranges(v) {
                    Some(r) => out.push((owner, r)),
                    None => bad = true,
                }
            }
            !bad
        })?;
        if bad {
            return Err(Error::Corruption("malformed build record".into()));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
