//! The buffer pool (TS-08).
//!
//! - **Frames.** Each frame holds one page in a `Box<[u8]>` behind a
//!   [`std::sync::RwLock`] latch, with an atomic pin count, an atomic usage
//!   count and an atomic dirty flag. The frame table has one slot per page
//!   the memory cap could hold; a slot gets its page buffer only when the
//!   accountant grants the bytes.
//! - **Page table.** [`SHARDS`] shards, each a `RwLock<HashMap>` from page
//!   number to frame. A frame is pinned only while its shard is latched, and
//!   eviction detaches a frame under the exclusive shard latch after seeing
//!   that only the evictor holds a pin. So no reader can reach a frame that
//!   eviction is taking (PostgreSQL `BufferAlloc`: lookup and pin under the
//!   partition lock, R-PG `src/backend/storage/buffer/bufmgr.c`).
//! - **Eviction.** A clock sweep over the frames (PostgreSQL
//!   `StrategyGetBuffer`, R-PG `src/backend/storage/buffer/freelist.c:236-310`):
//!   a pinned frame is skipped; a frame with a usage count above 0 has it
//!   decremented; the first unpinned frame at 0 is the victim. Usage counts
//!   stop at [`MAX_USAGE`], PostgreSQL's `BM_MAX_USAGE_COUNT`. A miss takes
//!   the victim whether clean or dirty; a reservation for another user
//!   first sweeps for clean frames only, then also takes dirty ones (TS-08
//!   order).
//! - **WAL before data.** A dirty frame is written only when its page LSN is
//!   at most the durable LSN, and only into reserved space (TS-05). Pinned
//!   frames are never evicted, so the pages a commit pins (TS-16 step 6)
//!   stay in the pool. Under NO-STEAL a page changed by a commit carries
//!   that commit's LSN, which is not durable until its WAL group is synced.
//! - **I/O.** Positional reads and writes through the storage trait, using
//!   the OS cache. Every read verifies the page checksum; a failure fails
//!   the read and sets the database's unusable latch (FR-REC-005).
//! - **Memory.** Page buffers are charged to the memory accountant, and so
//!   are the frame table and page-table estimate, charged once at creation.
//!   A floor of pages, by default 1 % of the cap, is charged at creation and
//!   never given back, so the pool can always run (TS-08).
//!
//! Hit and miss counts are kept per shard and sent to the metrics sink by
//! [`Pool::publish_metrics`], so a hit makes no sink call. Evictions and
//! dirty writes are sent as they happen.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{
    Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError,
};

use super::datafile::Datafile;
use super::error::{Error, Health, Result};
use super::memory::{MemoryAccountant, Reservation};
use super::page::{self, PAGE_BYTES, PAGE_SIZE};
use crate::obs::MetricsSink;
use crate::obs::names::cache::{ACCOUNTED_BYTES, EVICTIONS, FLUSHES, HITS, PAGE_FAULTS};

/// Page-table shards; PostgreSQL uses 128 buffer-mapping partitions
/// (`NUM_BUFFER_PARTITIONS`, R-PG `src/include/storage/lwlock.h`).
pub(crate) const SHARDS: usize = 128;

/// Highest usage count, as PostgreSQL's `BM_MAX_USAGE_COUNT` (R-PG
/// `src/include/storage/buf_internals.h:144`).
pub(crate) const MAX_USAGE: u32 = 5;

/// Default pool floor, in thousandths of the memory cap (TS-08: 1 %).
pub(crate) const DEFAULT_FLOOR_PERMILLE: u64 = 10;

/// Page number of a frame that holds no page.
const NO_PAGE: u64 = u64::MAX;

/// Pin count of a slot without a page buffer. The sweep claims a frame
/// with a compare-and-swap from 0 to 1, which can never succeed on this
/// value, so an empty slot has exactly one owner: the thread that took it
/// from the list of empty slots.
const EMPTY: u32 = u32::MAX;

/// Fibonacci hashing of page numbers: one multiplication. Page numbers are
/// dense integers, so this spreads them evenly (Knuth, TAOCP vol. 3, §6.4).
#[derive(Default)]
struct PageHasher(u64);

impl Hasher for PageHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("the page table hashes only u64 keys")
    }

    fn write_u64(&mut self, page: u64) {
        self.0 = page.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type PageMap = HashMap<u64, u32, BuildHasherDefault<PageHasher>>;

struct Frame {
    latch: RwLock<Box<[u8]>>,
    /// The page held, or [`NO_PAGE`]. Changed only by a thread that holds
    /// the only pin, under the page's shard latch.
    page: AtomicU64,
    /// Pins, or [`EMPTY`] for a slot without a buffer.
    pins: AtomicU32,
    usage: AtomicU32,
    /// Set by a writer under the write latch; cleared by eviction under the
    /// read latch after the page is written.
    dirty: AtomicBool,
}

impl Frame {
    fn empty() -> Frame {
        Frame {
            latch: RwLock::new(Box::default()),
            page: AtomicU64::new(NO_PAGE),
            pins: AtomicU32::new(EMPTY),
            usage: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
        }
    }

    fn unpin(&self) {
        // Release: our reads and writes of the page happen before an
        // evictor that sees the count drop (Acquire in `detach`).
        self.pins.fetch_sub(1, Ordering::Release);
    }
}

/// One page-table shard, on its own cache lines.
#[repr(align(128))]
struct Shard {
    map: RwLock<PageMap>,
    hits: AtomicU64,
    misses: AtomicU64,
}

/// Frames with a page buffer, and the slots without one.
struct Growth {
    buffers: u64,
    empty: Vec<u32>,
}

/// The buffer pool of one database.
pub(crate) struct Pool {
    frames: Box<[Frame]>,
    shards: Box<[Shard]>,
    hand: AtomicUsize,
    growth: Mutex<Growth>,
    floor_frames: u64,
    /// Bytes charged at creation: frame table, page-table estimate, floor.
    fixed_bytes: u64,
    accountant: Arc<MemoryAccountant>,
    data: Arc<Datafile>,
    health: Arc<Health>,
    durable_lsn: Arc<AtomicU64>,
    sink: Arc<dyn MetricsSink>,
    /// Hits and misses already sent to the sink.
    published: Mutex<(u64, u64)>,
}

/// A pinned page. While it exists the frame is not evicted. Dropping it
/// unpins the frame.
pub(crate) struct PinnedPage<'a> {
    pool: &'a Pool,
    frame: u32,
}

/// How a memory cap divides into pool slots and fixed charges.
pub(crate) struct Sizing {
    slots: u32,
    floor_frames: u64,
    per_shard: usize,
    /// Bytes charged at creation: frame table, page-table estimate, floor.
    pub(crate) fixed_bytes: u64,
}

impl Sizing {
    /// Sizes a pool for `cap` bytes with a floor of `floor_permille`
    /// thousandths of the cap, at least one page.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if the cap cannot hold the frame table and
    /// the floor, or exceeds 32 TiB.
    pub(crate) fn new(cap: u64, floor_permille: u64) -> Result<Sizing> {
        let slots = u32::try_from(cap / PAGE_BYTES)
            .map_err(|_| Error::InvalidArgument("memory_cap exceeds 32 TiB"))?;
        let floor_frames = (cap / 1000 * floor_permille / PAGE_BYTES).max(1);
        // Twice the mean occupancy per shard: a shard whose occupancy
        // fluctuates above its capacity rehashes, an allocation on the miss
        // path. Measured (lab::pool_fixed, 10 % cache, 30 000 misses under
        // heaptrack): 80 allocations with 1×, 0 with 2×.
        let per_shard = (slots as usize).div_ceil(SHARDS) * 2;
        // std's HashMap is hashbrown: per bucket one (u64, u32) entry, padded
        // to 16 bytes, and one control byte; buckets are a power of two at
        // most 7/8 full. This is an estimate of the allocation.
        let buckets = (per_shard * 8).div_ceil(7).next_power_of_two() as u64;
        let table_bytes = SHARDS as u64 * (buckets * 17 + 16);
        // Each slot: its frame, and its index in the list of empty slots.
        let frame_bytes = u64::from(slots) * (std::mem::size_of::<Frame>() + 4) as u64;
        let fixed_bytes = frame_bytes + table_bytes + floor_frames * PAGE_BYTES;
        if slots == 0 || fixed_bytes > cap {
            return Err(Error::InvalidArgument(
                "memory_cap is too small for the buffer pool",
            ));
        }
        Ok(Sizing {
            slots,
            floor_frames,
            per_shard,
            fixed_bytes,
        })
    }

    /// Bytes charged at creation other than the floor's page buffers: the
    /// frame table and the page-table estimate.
    pub(crate) fn overhead_bytes(&self) -> u64 {
        self.fixed_bytes - self.floor_frames * PAGE_BYTES
    }
}

impl Pool {
    /// A pool for `data`, charging `accountant`. `floor_permille` is the
    /// floor in thousandths of the cap, at least one page.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if the cap cannot hold the frame table and
    /// the floor, or if the accountant cannot grant them.
    pub(crate) fn new(
        data: Arc<Datafile>,
        accountant: Arc<MemoryAccountant>,
        health: Arc<Health>,
        durable_lsn: Arc<AtomicU64>,
        sink: Arc<dyn MetricsSink>,
        floor_permille: u64,
    ) -> Result<Pool> {
        let sizing = Sizing::new(accountant.cap(), floor_permille)?;
        if !accountant.try_charge(sizing.fixed_bytes) {
            return Err(Error::InvalidArgument(
                "memory_cap is too small for the buffer pool",
            ));
        }
        let Sizing {
            slots,
            floor_frames,
            per_shard,
            fixed_bytes,
        } = sizing;
        let shards: Box<[Shard]> = (0..SHARDS)
            .map(|_| Shard {
                map: RwLock::new(PageMap::with_capacity_and_hasher(
                    per_shard,
                    BuildHasherDefault::default(),
                )),
                hits: AtomicU64::new(0),
                misses: AtomicU64::new(0),
            })
            .collect();
        let frames: Box<[Frame]> = (0..slots).map(|_| Frame::empty()).collect();
        let pool = Pool {
            frames,
            shards,
            hand: AtomicUsize::new(0),
            growth: Mutex::new(Growth {
                buffers: 0,
                empty: (0..slots).rev().collect(),
            }),
            floor_frames,
            fixed_bytes,
            accountant,
            data,
            health,
            durable_lsn,
            sink,
            published: Mutex::new((0, 0)),
        };
        pool.sink.gauge(ACCOUNTED_BYTES, pool.accountant.used());
        Ok(pool)
    }

    /// Pages the pool keeps whatever other users reserve.
    pub(crate) fn floor_frames(&self) -> u64 {
        self.floor_frames
    }

    /// Frames that hold a page buffer now.
    pub(crate) fn buffer_frames(&self) -> u64 {
        self.lock_growth().buffers
    }

    fn lock_growth(&self) -> MutexGuard<'_, Growth> {
        self.growth.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn shard(&self, page_no: u64) -> &Shard {
        let h = page_no.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        &self.shards[(h >> 57) as usize % SHARDS]
    }

    /// A latch poisoned by a panic: the page may be half-changed, so the
    /// database becomes unusable.
    fn poisoned(&self) -> Error {
        self.health.fail(Error::Unusable)
    }

    /// Pins page `page_no`, reading it from the data file on a miss.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] after an earlier failure; [`Error::InvalidArgument`]
    /// for a page outside the data file; [`Error::Corruption`] or
    /// [`Error::Io`] if the read fails, which sets the unusable latch;
    /// [`Error::MemoryLimit`] if every frame is pinned or dirty with a page
    /// LSN that is not durable.
    pub(crate) fn fetch(&self, page_no: u64) -> Result<PinnedPage<'_>> {
        self.health.check()?;
        if page_no >= self.data.reserved_pages() {
            return Err(Error::InvalidArgument("page outside the data file"));
        }
        let shard = self.shard(page_no);
        if let Some(frame) = self.pin_mapped(&read_map(shard), page_no) {
            shard.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(PinnedPage { pool: self, frame });
        }
        self.load(shard, page_no)
    }

    /// Pins the frame that holds `page_no` in `map`, if any, and counts a
    /// use. `map` is borrowed from a held shard latch.
    fn pin_mapped(&self, map: &PageMap, page_no: u64) -> Option<u32> {
        let &frame = map.get(&page_no)?;
        let f = &self.frames[frame as usize];
        f.pins.fetch_add(1, Ordering::Acquire);
        let usage = f.usage.load(Ordering::Relaxed);
        if usage < MAX_USAGE {
            // A lost update between racing readers only undercounts a use.
            f.usage.store(usage + 1, Ordering::Relaxed);
        }
        Some(frame)
    }

    /// Miss path: takes a frame, maps it to `page_no`, and reads the page
    /// under the frame's write latch, so hits on the same page wait for the
    /// read and then see its result.
    fn load(&self, shard: &Shard, page_no: u64) -> Result<PinnedPage<'_>> {
        let frame = self.take_frame()?;
        let f = &self.frames[frame as usize];
        // Uncontended: the frame is unmapped and pinned only by us.
        let mut buf = f.latch.write().map_err(|_| self.poisoned())?;
        {
            let mut map = write_map(shard);
            if let Some(other) = self.pin_mapped(&map, page_no) {
                // Another thread loaded the page meanwhile; our frame stays
                // free for the sweep.
                drop(map);
                drop(buf);
                f.unpin();
                shard.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(PinnedPage {
                    pool: self,
                    frame: other,
                });
            }
            map.insert(page_no, frame);
            f.page.store(page_no, Ordering::Relaxed);
            f.usage.store(1, Ordering::Relaxed);
        }
        shard.misses.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = self.data.read_page(page_no, &mut buf) {
            // `read_page` has set the unusable latch; threads waiting on this
            // frame see it once they get the latch.
            write_map(shard).remove(&page_no);
            f.page.store(NO_PAGE, Ordering::Relaxed);
            f.usage.store(0, Ordering::Relaxed);
            drop(buf);
            f.unpin();
            return Err(e);
        }
        drop(buf);
        Ok(PinnedPage { pool: self, frame })
    }

    /// Pins page `page_no`, newly allocated, without reading it: its frame
    /// holds zeros except page LSN `lsn`, and is dirty; the caller
    /// initialises it. A page that is never written has no valid image on
    /// disk (#13), and a free page being reused has a stale one, so neither
    /// is read.
    ///
    /// The zeros and the page LSN are written under one exclusive latch, so
    /// no checkpoint or eviction can see the frame dirty with page LSN 0
    /// and write it before the group that will hold its image is durable
    /// (TS-09; the page LSN is set "in the same latched write that changes
    /// the page"). A resident frame of a page freed by a commit whose group
    /// is not yet durable would otherwise be overwritten on disk.
    ///
    /// # Errors
    ///
    /// As [`Pool::fetch`], except that no read can fail.
    pub(crate) fn create(&self, page_no: u64, lsn: u64) -> Result<PinnedPage<'_>> {
        self.health.check()?;
        if page_no >= self.data.reserved_pages() {
            return Err(Error::InvalidArgument("page outside the data file"));
        }
        let shard = self.shard(page_no);
        let mapped = self.pin_mapped(&read_map(shard), page_no);
        let pinned = match mapped {
            Some(frame) => PinnedPage { pool: self, frame },
            None => {
                let frame = self.take_frame()?;
                let f = &self.frames[frame as usize];
                let mut map = write_map(shard);
                if let Some(other) = self.pin_mapped(&map, page_no) {
                    drop(map);
                    f.unpin();
                    PinnedPage {
                        pool: self,
                        frame: other,
                    }
                } else {
                    map.insert(page_no, frame);
                    f.page.store(page_no, Ordering::Relaxed);
                    f.usage.store(1, Ordering::Relaxed);
                    PinnedPage { pool: self, frame }
                }
            }
        };
        {
            let mut g = pinned.write()?;
            g.fill(0);
            page::set_lsn(&mut g, lsn);
        }
        Ok(pinned)
    }

    /// Pins page `page_no` only if the pool holds it, and says whether it
    /// is dirty. Reads nothing (for the integrity check and the undo of a
    /// failed batch).
    pub(crate) fn resident(&self, page_no: u64) -> Option<(PinnedPage<'_>, bool)> {
        let frame = self.pin_mapped(&read_map(self.shard(page_no)), page_no)?;
        let dirty = self.frames[frame as usize].dirty.load(Ordering::Acquire);
        Some((PinnedPage { pool: self, frame }, dirty))
    }

    /// Writes every dirty frame whose page LSN is durable, waiting for each
    /// frame's latch, and returns the number written. It does not sync the
    /// data file.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`]; an I/O error, which sets the unusable latch.
    pub(crate) fn write_dirty(&self) -> Result<u64> {
        let mut skipped = Vec::new();
        self.checkpoint_pass(None, &mut skipped)
    }

    /// The write loop of a checkpoint (TS-11 step 2). Writes each dirty
    /// frame whose page LSN is durable, waiting for each frame's latch, and
    /// returns the number written. Each dirty frame whose page LSN is not
    /// durable is left in `skipped` as `(page, page LSN)`. With `only`,
    /// just those pages are visited; a page no longer in the pool was
    /// written by eviction.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`]; an I/O error, which sets the unusable latch.
    pub(crate) fn checkpoint_pass(
        &self,
        only: Option<&[(u64, u64)]>,
        skipped: &mut Vec<(u64, u64)>,
    ) -> Result<u64> {
        self.health.check()?;
        skipped.clear();
        let mut written = 0;
        let mut visit = |page_no: u64| -> Result<()> {
            // Pin through the page table, so the frame cannot be evicted
            // or remapped while it is written.
            let Some(frame) = self.pin_mapped(&read_map(self.shard(page_no)), page_no) else {
                return Ok(());
            };
            let pinned = PinnedPage { pool: self, frame };
            let f = pinned.frame();
            let buf = f.latch.read().map_err(|_| self.poisoned())?;
            if !f.dirty.load(Ordering::Acquire) {
                return Ok(());
            }
            let lsn = page::lsn(&buf);
            if lsn > self.durable_lsn.load(Ordering::Acquire) {
                skipped.push((page_no, lsn));
                return Ok(());
            }
            let mut copy = [0u8; PAGE_SIZE];
            copy.copy_from_slice(&buf);
            page::seal(&mut copy, page_no);
            self.data.write_page(page_no, &copy)?;
            f.dirty.store(false, Ordering::Release);
            self.sink.counter(FLUSHES, 1);
            written += 1;
            Ok(())
        };
        match only {
            Some(pages) => {
                for &(p, _) in pages {
                    visit(p)?;
                }
            }
            None => {
                for f in &self.frames[..] {
                    let page_no = f.page.load(Ordering::Relaxed);
                    if page_no != NO_PAGE && f.dirty.load(Ordering::Acquire) {
                        visit(page_no)?;
                    }
                }
            }
        }
        Ok(written)
    }

    /// Returns a frame pinned once by the caller, unmapped, with a buffer:
    /// a new buffer if the accountant allows, else the clock's victim,
    /// clean or dirty with a durable page LSN, as in PostgreSQL's sweep.
    /// (Clean frames first applies only to reservations, [`Pool::reserve`]:
    /// on the miss path it would keep dirty frames until a checkpoint.)
    fn take_frame(&self) -> Result<u32> {
        if let Some(frame) = self.grow() {
            return Ok(frame);
        }
        if let Some(frame) = self.sweep(true)? {
            return Ok(frame);
        }
        Err(Error::MemoryLimit {
            requested: PAGE_BYTES,
        })
    }

    /// Gives an empty slot a buffer if the floor or the accountant covers
    /// it.
    fn grow(&self) -> Option<u32> {
        let mut g = self.lock_growth();
        let &frame = g.empty.last()?;
        if g.buffers >= self.floor_frames && !self.accountant.try_charge(PAGE_BYTES) {
            return None;
        }
        g.empty.pop();
        g.buffers += 1;
        drop(g);
        let f = &self.frames[frame as usize];
        // The slot is ours alone: unmapped, and its pin count is `EMPTY`.
        *f.latch.write().unwrap_or_else(PoisonError::into_inner) =
            vec![0u8; PAGE_SIZE].into_boxed_slice();
        f.pins.store(1, Ordering::Release);
        Some(frame)
    }

    /// One clock sweep. Returns a victim pinned by the caller and detached
    /// from the page table, or `None` once a full turn of the clock finds no
    /// candidate.
    fn sweep(&self, allow_dirty: bool) -> Result<Option<u32>> {
        let n = self.frames.len();
        // Frames passed without a change of state; a usage decrement resets
        // it, as in PostgreSQL's `trycounter`.
        let mut idle = 0;
        while idle < n {
            let i = self.hand.fetch_add(1, Ordering::Relaxed) % n;
            let f = &self.frames[i];
            // A pinned frame, or an empty slot (`EMPTY`).
            if f.pins.load(Ordering::Relaxed) != 0 {
                idle += 1;
                continue;
            }
            let usage = f.usage.load(Ordering::Relaxed);
            if usage > 0 {
                f.usage.store(usage - 1, Ordering::Relaxed);
                idle = 0;
                continue;
            }
            if (!allow_dirty && f.dirty.load(Ordering::Acquire))
                || f.pins
                    .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_err()
            {
                idle += 1;
                continue;
            }
            match self.detach(f) {
                Ok(true) => return Ok(Some(i as u32)),
                Ok(false) => {
                    f.unpin();
                    idle += 1;
                }
                Err(e) => {
                    f.unpin();
                    return Err(e);
                }
            }
        }
        Ok(None)
    }

    /// Writes the pinned frame `f` if it is dirty and its page LSN is
    /// durable, then removes it from the page table if nobody else pinned or
    /// dirtied it meanwhile. Returns whether the frame is now free.
    fn detach(&self, f: &Frame) -> Result<bool> {
        if f.dirty.load(Ordering::Acquire) && !self.flush(f)? {
            return Ok(false);
        }
        let page_no = f.page.load(Ordering::Relaxed);
        if page_no == NO_PAGE {
            return Ok(true);
        }
        let mut map = write_map(self.shard(page_no));
        // Pins are taken only under the shard latch, which we hold: if ours
        // is the only one, no reader can reach the frame any more.
        if f.pins.load(Ordering::Acquire) != 1 || f.dirty.load(Ordering::Acquire) {
            return Ok(false);
        }
        map.remove(&page_no);
        f.page.store(NO_PAGE, Ordering::Relaxed);
        drop(map);
        self.sink.counter(EVICTIONS, 1);
        Ok(true)
    }

    /// Writes a dirty frame if its page LSN is durable (TS-08). Returns
    /// whether the frame is clean afterwards.
    fn flush(&self, f: &Frame) -> Result<bool> {
        // A conditional latch, as PostgreSQL's `GetVictimBuffer`
        // (`BufferLockConditional`, R-PG `bufmgr.c`): a frame latched by a
        // writer is not a good victim, and waiting for it buys nothing.
        let buf = match f.latch.try_read() {
            Ok(buf) => buf,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => return Err(self.poisoned()),
        };
        if !f.dirty.load(Ordering::Acquire) {
            return Ok(true);
        }
        // Acquire pairs with the WAL writer's Release store of the durable
        // LSN after its `sync_data`.
        if page::lsn(&buf) > self.durable_lsn.load(Ordering::Acquire) {
            return Ok(false);
        }
        // Seal a copy, as PostgreSQL's `PageSetChecksumCopy`: readers keep
        // the frame under the shared latch while it is written.
        let page_no = f.page.load(Ordering::Relaxed);
        let mut copy = [0u8; PAGE_SIZE];
        copy.copy_from_slice(&buf);
        page::seal(&mut copy, page_no);
        self.data.write_page(page_no, &copy)?;
        f.dirty.store(false, Ordering::Release);
        self.sink.counter(FLUSHES, 1);
        Ok(true)
    }

    /// Reserves `bytes` for another user of the memory cap (TS-08). When
    /// the accountant refuses, the pool frees frames, clean ones first, then
    /// dirty ones whose page LSN is durable, down to its floor.
    ///
    /// # Errors
    ///
    /// [`Error::MemoryLimit`] if the bytes do not fit even then;
    /// [`Error::Unusable`]; an I/O error while writing a victim.
    pub(crate) fn reserve(&self, bytes: u64) -> Result<Reservation> {
        self.health.check()?;
        loop {
            if let Some(r) = self.accountant.try_reserve(bytes) {
                return Ok(r);
            }
            if !self.shrink_one()? {
                return Err(Error::MemoryLimit { requested: bytes });
            }
        }
    }

    /// Frees one frame's buffer if the pool is above its floor and a victim
    /// exists. Returns whether it did.
    fn shrink_one(&self) -> Result<bool> {
        if self.lock_growth().buffers <= self.floor_frames {
            return Ok(false);
        }
        let mut victim = None;
        for allow_dirty in [false, true] {
            victim = self.sweep(allow_dirty)?;
            if victim.is_some() {
                break;
            }
        }
        let Some(frame) = victim else {
            return Ok(false);
        };
        let f = &self.frames[frame as usize];
        {
            let mut g = self.lock_growth();
            if g.buffers <= self.floor_frames {
                drop(g);
                f.unpin(); // stays a free frame with a buffer
                return Ok(false);
            }
            g.buffers -= 1;
        }
        *f.latch.write().unwrap_or_else(PoisonError::into_inner) = Box::default();
        f.usage.store(0, Ordering::Relaxed);
        // From our single pin straight to `EMPTY`: no sweep can claim it.
        f.pins.store(EMPTY, Ordering::Release);
        self.lock_growth().empty.push(frame);
        self.accountant.release(PAGE_BYTES);
        Ok(true)
    }

    /// Sends the hits and misses counted since the last call, and the
    /// accounted bytes, to the metrics sink.
    pub(crate) fn publish_metrics(&self) {
        let (hits, misses) = self.shards.iter().fold((0, 0), |(h, m), s| {
            (
                h + s.hits.load(Ordering::Relaxed),
                m + s.misses.load(Ordering::Relaxed),
            )
        });
        let mut published = self
            .published
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.sink.counter(HITS, hits - published.0);
        self.sink.counter(PAGE_FAULTS, misses - published.1);
        *published = (hits, misses);
        drop(published);
        self.sink.gauge(ACCOUNTED_BYTES, self.accountant.used());
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let buffers = self.lock_growth().buffers;
        let charged = buffers.saturating_sub(self.floor_frames) * PAGE_BYTES;
        self.accountant.release(self.fixed_bytes + charged);
    }
}

fn read_map(shard: &Shard) -> RwLockReadGuard<'_, PageMap> {
    // A panic cannot leave the map half-updated: its operations are single
    // calls that either complete or abort the process on allocation failure.
    shard.map.read().unwrap_or_else(PoisonError::into_inner)
}

fn write_map(shard: &Shard) -> RwLockWriteGuard<'_, PageMap> {
    shard.map.write().unwrap_or_else(PoisonError::into_inner)
}

impl PinnedPage<'_> {
    fn frame(&self) -> &Frame {
        &self.pool.frames[self.frame as usize]
    }

    /// Takes the shared latch.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the database became unusable, for example
    /// because the read that loaded this page failed.
    pub(crate) fn read(&self) -> Result<RwLockReadGuard<'_, Box<[u8]>>> {
        let guard = self
            .frame()
            .latch
            .read()
            .map_err(|_| self.pool.poisoned())?;
        self.pool.health.check()?;
        Ok(guard)
    }

    /// Takes the exclusive latch and marks the page dirty. The writer sets
    /// the page LSN of its change; the checksum is set when the page is
    /// written.
    ///
    /// # Errors
    ///
    /// As [`PinnedPage::read`].
    pub(crate) fn write(&self) -> Result<RwLockWriteGuard<'_, Box<[u8]>>> {
        let guard = self
            .frame()
            .latch
            .write()
            .map_err(|_| self.pool.poisoned())?;
        self.pool.health.check()?;
        self.frame().dirty.store(true, Ordering::Release);
        Ok(guard)
    }
}

impl PinnedPage<'_> {
    /// The exclusive latch, without the unusable check: the undo of a
    /// failed batch restores its pages before it reports the failure
    /// (TS-16 step 6), also when the failure set the unusable latch.
    fn undo_latch(&self) -> Result<RwLockWriteGuard<'_, Box<[u8]>>> {
        self.frame().latch.write().map_err(|_| self.pool.poisoned())
    }

    /// Takes the exclusive latch and marks the page dirty, as
    /// [`PinnedPage::write`], and returns whether the page was dirty before,
    /// so that an undo can restore that state.
    ///
    /// # Errors
    ///
    /// As [`PinnedPage::write`].
    pub(crate) fn write_noting_dirty(&self) -> Result<(RwLockWriteGuard<'_, Box<[u8]>>, bool)> {
        let guard = self.undo_latch()?;
        self.pool.health.check()?;
        let was = self.frame().dirty.swap(true, Ordering::AcqRel);
        Ok((guard, was))
    }

    /// Undo of a failed batch: writes `image` back into the frame, with its
    /// dirty state `dirty` from before the batch. A frame that was clean
    /// holds the image on disk again, so it becomes clean. Works on an
    /// unusable database: nothing writes a page of an unusable database.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the latch is poisoned.
    pub(crate) fn undo_restore(&self, image: &[u8], dirty: bool) -> Result<()> {
        let mut g = self.undo_latch()?;
        g.copy_from_slice(image);
        self.frame().dirty.store(dirty, Ordering::Release);
        Ok(())
    }

    /// Undo of a failed batch for a page it took that was free before it,
    /// in the pool or not, and that is free again after the undo (TS-16 step 6 undo,
    /// item 3). A reader may have followed a link the batch wrote to this
    /// page, so the page must stay readable with the image the batch left in
    /// the frame until the reuse fence is lifted. The image on disk cannot
    /// serve: it is a reserve page never written, which fails its checksum,
    /// or an older image of a free page.
    ///
    /// The image is therefore made to reach the data file before the frame
    /// can be freed, under the WAL-before-data rule (TS-09) for `lsn`, the
    /// durable LSN at which every commit that freed the page is durable
    /// ([`super::wal::OpenGroup::joined_durable_lsn`]):
    ///
    /// - if `lsn` is durable, the image is written now and the frame becomes
    ///   clean, its page LSN unchanged;
    /// - otherwise the frame becomes dirty with page LSN `lsn`, so eviction
    ///   and checkpoints write it once that commit is durable. The page LSN
    ///   it had before the batch is on disk and so at most the durable LSN:
    ///   `lsn` is not below it.
    ///
    /// On an unusable database nothing is written (TS-12) and the frame
    /// becomes clean; no read can reach it.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the latch is poisoned; [`Error::Io`] if the
    /// write fails, which sets the unusable latch.
    pub(crate) fn undo_keep(&self, lsn: u64) -> Result<()> {
        let mut g = self.undo_latch()?;
        let f = self.frame();
        if self.pool.health.is_unusable() {
            f.dirty.store(false, Ordering::Release);
            return Ok(());
        }
        if lsn <= self.pool.durable_lsn.load(Ordering::Acquire) {
            let page_no = f.page.load(Ordering::Relaxed);
            let mut copy = [0u8; PAGE_SIZE];
            copy.copy_from_slice(&g);
            page::seal(&mut copy, page_no);
            self.pool.data.write_page(page_no, &copy)?;
            f.dirty.store(false, Ordering::Release);
            self.pool.sink.counter(FLUSHES, 1);
        } else {
            page::set_lsn(&mut g, lsn);
            f.dirty.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// The shared latch without the unusable check, for the capture of a
    /// batch's final images (TS-09): the pages are pinned and their images
    /// are in memory, so no failure of another component can make them
    /// unreadable.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the latch is poisoned.
    pub(crate) fn read_for_capture(&self) -> Result<RwLockReadGuard<'_, Box<[u8]>>> {
        self.frame().latch.read().map_err(|_| self.pool.poisoned())
    }
}

impl Drop for PinnedPage<'_> {
    fn drop(&mut self) {
        self.frame().unpin();
    }
}

#[cfg(test)]
mod tests;
