//! Free space (TS-05): the persisted free list, the in-memory free-page
//! set and the reader registry that decides when a freed page may be
//! reused.
//!
//! **Persisted list.** A chain of trunk pages; page 0 holds the first
//! (TS-05). Each trunk holds free page numbers and the next trunk, as
//! SQLite's free-list trunks (R-SQLITE `src/btreeInt.h:206-215`). Trunk
//! layout after the page header, integers little-endian:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 24 | 8 | next trunk (0: last) |
//! | 32 | 4 | entries in this trunk |
//! | 36 | 4 | entries already taken (non-zero only in the first trunk) |
//! | 40 | 8 × [`TRUNK_CAP`] | free page numbers |
//!
//! The list is a queue: a freed page is appended to the last trunk, and
//! allocation takes from the first. SQLite instead pushes and pops at the
//! first trunk, and turns a freed page into a new trunk at once
//! (`freePage2`, `src/btree.c`). Here a freed page may still be in a
//! reader's hands, so it may not be written until the reuse rule allows
//! it; and since pages are freed in commit order, the pages the rule allows
//! are always a prefix of the queue, so allocation needs no search.
//!
//! **Reuse** (TS-05). A page freed by the write batch numbered `c` enters
//! the persisted list in that batch, and may be reused once every tree
//! read operation registered in [`Readers`] has registered a value of at
//! least `c`. With MVCC the value becomes the snapshot (Sprint 2). At open
//! no reader exists, so every listed page is reusable. A trunk page whose
//! entries are all taken is reused at once: readers never hold trunk pages.
//!
//! **In-memory set.** The listed pages in queue order, with the batch that
//! freed each, run-length encoded. Outside the memory cap (TS-08).
//!
//! **Journal.** Each change to the in-memory state is journalled from
//! [`FreeList::begin`] on, so that [`FreeList::rollback`] can restore it
//! when a batch is undone (TS-16 step 6); the trunk pages themselves are
//! restored from their page copies.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering, fence};

use super::btree::PageWriter;
use super::error::{Error, Result};
use super::page::{self, PAGE_SIZE, PageType};
use super::pool::Pool;

const NEXT: usize = page::HEADER_LEN;
const COUNT: usize = NEXT + 8;
const FIRST: usize = COUNT + 4;
const ENTRIES: usize = FIRST + 4;
/// Page numbers one trunk holds.
pub(crate) const TRUNK_CAP: usize = (PAGE_SIZE - ENTRIES) / 8;

fn u32_at(p: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(p[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_at(p: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(p[at..at + 8].try_into().expect("8 bytes"))
}

/// The fields of a trunk page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrunkView {
    /// Next trunk, 0 for the last.
    pub(crate) next: u64,
    /// Entries already taken.
    pub(crate) first: u32,
    /// Free pages listed and not yet taken.
    pub(crate) pages: Vec<u64>,
}

/// Decodes a trunk page, or describes why it is malformed.
pub(crate) fn decode_trunk(p: &[u8]) -> std::result::Result<TrunkView, String> {
    if page::page_type(p) != Some(PageType::FreeListTrunk) {
        return Err("not a free-list trunk page".into());
    }
    let count = u32_at(p, COUNT) as usize;
    let first = u32_at(p, FIRST);
    if count > TRUNK_CAP || first as usize > count {
        return Err(format!("trunk counts {first}/{count} out of range"));
    }
    let pages = (first as usize..count)
        .map(|i| u64_at(p, ENTRIES + 8 * i))
        .collect();
    Ok(TrunkView {
        next: u64_at(p, NEXT),
        first,
        pages,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Trunk {
    page: u64,
    count: u32,
}

/// One journalled change of the in-memory state.
#[derive(Debug, Clone, Copy)]
enum Op {
    /// The first trunk, emptied, was taken; `first` was its value.
    PopTrunk { trunk: Trunk, first: u32 },
    /// The oldest listed page was taken; it carried `tag`.
    PopPage { page: u64, tag: u64 },
    /// The last trunk gained an entry.
    GrowBack,
    /// A trunk was appended; `first` was its value before.
    PushTrunk { first: u32 },
    /// A page was appended to the list.
    PushPage,
}

/// The free list of one database: the persisted chain's shape and the
/// in-memory set. Changed only by the single writer.
#[derive(Debug, Default)]
pub(crate) struct FreeList {
    trunks: VecDeque<Trunk>,
    /// Entries of the first trunk already taken.
    first: u32,
    /// Listed free pages, in queue order.
    pages: VecDeque<u64>,
    /// `(batch that freed them, run length)` over `pages`.
    tags: VecDeque<(u64, u64)>,
    /// Changes since [`FreeList::begin`].
    journal: Vec<Op>,
}

/// The in-memory state of a free list, for comparisons in tests.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreeState {
    trunks: Vec<(u64, u32)>,
    first: u32,
    pages: Vec<u64>,
    tags: Vec<(u64, u64)>,
}

impl FreeList {
    /// Reads the persisted list starting at trunk `head` (TS-05). Every
    /// listed page is reusable: no reader exists at open.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] for a malformed trunk, a page number at or
    /// above `page_count`, or a cycle; errors of [`Pool::fetch`].
    pub(crate) fn load(pool: &Pool, head: u64, page_count: u64) -> Result<FreeList> {
        let mut list = FreeList::default();
        let mut trunk = head;
        while trunk != 0 {
            if trunk >= page_count || list.trunks.len() as u64 >= page_count {
                return Err(Error::Corruption(format!(
                    "free-list trunk {trunk} out of range or chain cycles"
                )));
            }
            let pin = pool.fetch(trunk)?;
            let g = pin.read()?;
            let t = decode_trunk(&g)
                .map_err(|e| Error::Corruption(format!("free-list trunk {trunk}: {e}")))?;
            if !list.trunks.is_empty() && t.first != 0 {
                return Err(Error::Corruption(format!(
                    "free-list trunk {trunk} is not the first but has taken entries"
                )));
            }
            if let Some(&bad) = t.pages.iter().find(|&&p| p == 0 || p >= page_count) {
                return Err(Error::Corruption(format!(
                    "free-list trunk {trunk} lists page {bad} outside the file"
                )));
            }
            if list.trunks.is_empty() {
                list.first = t.first;
            }
            list.trunks.push_back(Trunk {
                page: trunk,
                count: t.first + t.pages.len() as u32,
            });
            list.push_pages(&t.pages, 0);
            trunk = t.next;
        }
        Ok(list)
    }

    /// Starts a new journal: the changes of one batch.
    pub(crate) fn begin(&mut self) {
        self.journal.clear();
    }

    /// Undoes every change since [`FreeList::begin`], newest first.
    pub(crate) fn rollback(&mut self) {
        while let Some(op) = self.journal.pop() {
            match op {
                Op::PopTrunk { trunk, first } => {
                    self.trunks.push_front(trunk);
                    self.first = first;
                }
                Op::PopPage { page, tag } => {
                    self.pages.push_front(page);
                    self.first -= 1;
                    match self.tags.front_mut() {
                        Some((t, n)) if *t == tag => *n += 1,
                        _ => self.tags.push_front((tag, 1)),
                    }
                }
                Op::GrowBack => {
                    self.trunks.back_mut().expect("journalled trunk").count -= 1;
                }
                Op::PushTrunk { first } => {
                    self.trunks.pop_back();
                    self.first = first;
                }
                Op::PushPage => {
                    self.pages.pop_back();
                    let (_, n) = self.tags.back_mut().expect("journalled page has a tag");
                    *n -= 1;
                    if *n == 0 {
                        self.tags.pop_back();
                    }
                }
            }
        }
    }

    /// The in-memory state, for comparisons in tests.
    #[cfg(test)]
    pub(crate) fn state(&self) -> FreeState {
        FreeState {
            trunks: self.trunks.iter().map(|t| (t.page, t.count)).collect(),
            first: self.first,
            pages: self.pages.iter().copied().collect(),
            tags: self.tags.iter().copied().collect(),
        }
    }

    fn push_pages(&mut self, pages: &[u64], tag: u64) {
        if pages.is_empty() {
            return;
        }
        self.pages.extend(pages);
        match self.tags.back_mut() {
            Some((t, n)) if *t == tag => *n += pages.len() as u64,
            _ => self.tags.push_back((tag, pages.len() as u64)),
        }
    }

    /// The first trunk page, for page 0; 0 if the list is empty.
    pub(crate) fn head(&self) -> u64 {
        self.trunks.front().map_or(0, |t| t.page)
    }

    /// Trunk pages in chain order.
    pub(crate) fn trunk_pages(&self) -> impl Iterator<Item = u64> + '_ {
        self.trunks.iter().map(|t| t.page)
    }

    /// Listed free pages in queue order.
    pub(crate) fn pages(&self) -> impl Iterator<Item = u64> + '_ {
        self.pages.iter().copied()
    }

    /// Listed pages that the reuse rule allows now, given `horizon`, the
    /// lowest value a reader registered (`u64::MAX` if none).
    pub(crate) fn reusable(&self, horizon: u64) -> u64 {
        self.tags
            .iter()
            .take_while(|(t, _)| *t <= horizon)
            .map(|(_, n)| n)
            .sum()
    }

    /// Takes a page: an emptied first trunk, else the oldest listed page if
    /// the reuse rule allows it, else the next reserved page of the data
    /// file. The caller initialises the page.
    ///
    /// # Errors
    ///
    /// ENOSPC if no page is free and no reserved page is left; page errors.
    pub(crate) fn allocate(&mut self, horizon: u64, pw: &mut PageWriter<'_>) -> Result<u64> {
        if let Some(&t) = self.trunks.front()
            && self.first == t.count
        {
            // The trunk is part of the list until this batch ends, and an
            // undo puts it back: it is copied first, also when it is not in
            // the pool (TS-16 step 6 undo).
            pw.mark(t.page)?;
            self.journal.push(Op::PopTrunk {
                trunk: t,
                first: self.first,
            });
            self.trunks.pop_front();
            self.first = 0;
            pw.header_changed();
            return Ok(t.page);
        }
        if let Some((tag, n)) = self.tags.front_mut()
            && *tag <= horizon
        {
            let tag = *tag;
            *n -= 1;
            if *n == 0 {
                self.tags.pop_front();
            }
            let page = self.pages.pop_front().expect("tags cover pages");
            self.journal.push(Op::PopPage { page, tag });
            self.first += 1;
            let head = self.trunks.front().expect("a listed page has a trunk").page;
            let pin = pw.change(head)?;
            pin.write()?[FIRST..FIRST + 4].copy_from_slice(&self.first.to_le_bytes());
            return Ok(page);
        }
        pw.allocate_reserved()
    }

    /// Appends `page`, freed by batch `tag`, to the list. A new trunk, if
    /// one is needed, is allocated with `horizon`.
    ///
    /// # Errors
    ///
    /// As [`FreeList::allocate`].
    pub(crate) fn free(
        &mut self,
        page: u64,
        tag: u64,
        horizon: u64,
        pw: &mut PageWriter<'_>,
    ) -> Result<()> {
        if let Some(t) = self.trunks.back_mut()
            && (t.count as usize) < TRUNK_CAP
        {
            let pin = pw.change(t.page)?;
            {
                let mut g = pin.write()?;
                let at = ENTRIES + 8 * t.count as usize;
                g[at..at + 8].copy_from_slice(&page.to_le_bytes());
                t.count += 1;
                g[COUNT..COUNT + 4].copy_from_slice(&t.count.to_le_bytes());
            }
            self.journal.push(Op::GrowBack);
            self.push_pages(&[page], tag);
            self.journal.push(Op::PushPage);
            return Ok(());
        }
        let trunk = self.allocate(horizon, pw)?;
        let pin = pw.create(trunk)?;
        {
            let mut g = pin.write()?;
            page::init(&mut g, PageType::FreeListTrunk, 0);
            g[COUNT..COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
            g[ENTRIES..ENTRIES + 8].copy_from_slice(&page.to_le_bytes());
        }
        match self.trunks.back() {
            Some(last) => {
                let pin = pw.change(last.page)?;
                pin.write()?[NEXT..NEXT + 8].copy_from_slice(&trunk.to_le_bytes());
            }
            None => pw.header_changed(),
        }
        self.journal.push(Op::PushTrunk { first: self.first });
        if self.trunks.is_empty() {
            self.first = 0;
        }
        self.trunks.push_back(Trunk {
            page: trunk,
            count: 1,
        });
        self.push_pages(&[page], tag);
        self.journal.push(Op::PushPage);
        Ok(())
    }
}

/// Registry of running tree read operations (TS-05 Reuse).
///
/// Each read operation registers, when it starts, the number of the last
/// write batch whose apply step has finished, and withdraws it when it
/// ends. Slots are atomics, each on its own cache line; a reader takes a
/// free slot with a compare-and-swap, starting from a per-thread hint, so
/// registering costs one uncontended atomic operation.
pub(crate) struct Readers {
    slots: Box<[Slot]>,
}

#[repr(align(64))]
struct Slot(AtomicU64);

/// Value of a free slot.
const FREE: u64 = u64::MAX;

/// Slots in the registry; more concurrent readers wait for a slot.
const SLOTS: usize = 256;

thread_local! {
    static HINT: Cell<usize> = const { Cell::new(usize::MAX) };
}

static NEXT_HINT: AtomicUsize = AtomicUsize::new(0);

impl Default for Readers {
    fn default() -> Self {
        Readers {
            slots: (0..SLOTS).map(|_| Slot(AtomicU64::new(FREE))).collect(),
        }
    }
}

/// A registered read operation; dropping it withdraws the registration.
pub(crate) struct Registered<'a> {
    slot: &'a AtomicU64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        // Release: the reader's page reads happen before the writer sees
        // the slot free.
        self.slot.store(FREE, Ordering::Release);
    }
}

impl Readers {
    /// Registers a read operation that starts now, with the value of
    /// `applied`, the number of the last batch whose apply has finished.
    pub(crate) fn enter(&self, applied: &AtomicU64) -> Registered<'_> {
        let mut i = HINT.with(|h| {
            if h.get() == usize::MAX {
                h.set(NEXT_HINT.fetch_add(1, Ordering::Relaxed) % SLOTS);
            }
            h.get()
        });
        let value = applied.load(Ordering::SeqCst);
        loop {
            let slot = &self.slots[i].0;
            if slot
                .compare_exchange(FREE, value, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                // With the fence in `horizon`, a writer that misses this
                // registration has finished its unlink before the reader's
                // first page latch, so the reader cannot reach the page
                // (store-buffering with SeqCst fences on both sides).
                fence(Ordering::SeqCst);
                HINT.with(|h| h.set(i));
                return Registered { slot };
            }
            i = (i + 1) % SLOTS;
            if i == 0 {
                std::thread::yield_now();
            }
        }
    }

    /// The lowest registered value, or `u64::MAX` if no reader is
    /// registered. The writer calls it after its page changes.
    pub(crate) fn horizon(&self) -> u64 {
        fence(Ordering::SeqCst);
        self.slots
            .iter()
            .map(|s| s.0.load(Ordering::SeqCst))
            .min()
            .unwrap_or(FREE)
    }
}

#[cfg(test)]
mod tests;
