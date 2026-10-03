//! The write-ahead log (TS-09): frames, segments, the open group and the
//! writer thread `lpg-wal`.
//!
//! **Frames.** Every frame is [`FRAME_SIZE`] bytes: a [`FRAME_HEADER`]-byte
//! header and an 8 KiB payload. Integers are little-endian.
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 4 | CRC-32C of bytes 4..[`FRAME_SIZE`] |
//! | 4 | 1 | kind: 1 page image, 2 commit |
//! | 5 | 3 | zero |
//! | 8 | 8 | LSN: the frame's position in the frame stream |
//! | 16 | 8 | salt of the open that wrote the frame |
//! | 24 | 8 | page number (page frame); 0 (commit frame) |
//! | 32 | 8192 | page image (page frame); commit record (commit frame) |
//!
//! The commit record, at offset 32: the LSN of the group's first frame,
//! the group's first and last commit timestamps, then the next element,
//! catalog-object and token identifiers (8 bytes each); the rest is zero.
//!
//! A frame is valid when its CRC-32C matches, its kind is known and its LSN
//! equals its position. A group is valid when its commit frame is valid,
//! records the group's start as its first LSN, every frame from the start
//! is valid and carries the commit frame's salt, and the salt is at least
//! the salt of the group before it (TS-09).
//!
//! **Segments.** Files `wal-<g>-<lsn>` of [`SEGMENT_SIZE`] bytes, each
//! holding [`SEGMENT_FRAMES`] frames; the bytes after the last frame are not
//! part of the stream. A segment exists before any group writes into it or
//! ends in it, and it is made durable (`sync_data`, then `sync_dir`) before
//! it is used (TS-09 creation order).
//!
//! **Group commit.** A committer, under the commit latch, gives each page it
//! changes the next frame position of the open group (page LSN = position +
//! one frame), and at the end of its apply copies the page's final image
//! into the group. The writer thread takes the commit latch only to swap
//! the open group for an empty one, then fills the frame headers, writes
//! the group with one `write_at` per segment it touches, and calls
//! `sync_data` on each. Only then does it advance the durable LSN and wake
//! the committers of the group. Group k+1 is therefore written only after
//! the `sync_data` of group k has returned. While the writer syncs one
//! group, later committers join the next: the batching of PostgreSQL's
//! `XLogFlush` (R-PG `src/backend/access/transam/xlog.c:2893-2899`) and of
//! InnoDB's log writer and flusher threads (R-INNO `log/log0write.cc`,
//! "Background redo log threads").

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use super::error::{Error, Health, Result};
use super::page::PAGE_SIZE;
use super::{Storage, StorageFile};
use crate::obs::MetricsSink;
use crate::obs::names::transaction_log::{
    APPEND_BATCH_SIZE, APPENDED_BYTES, FLUSHES, FRAMES, GROUPS, SEGMENTS_CREATED, SIZE,
};

/// Bytes of a frame header.
pub(crate) const FRAME_HEADER: usize = 32;
/// Bytes of one frame: header and one page.
pub(crate) const FRAME_SIZE: usize = FRAME_HEADER + PAGE_SIZE;
/// [`FRAME_SIZE`] as `u64`, for LSN arithmetic.
pub(crate) const FRAME_BYTES: u64 = FRAME_SIZE as u64;
/// Bytes of a segment file (TS-09).
pub(crate) const SEGMENT_SIZE: u64 = 16 << 20;
/// Frames in one segment: ⌊16 MiB / frame size⌋.
pub(crate) const SEGMENT_FRAMES: u64 = SEGMENT_SIZE / FRAME_BYTES;
/// LSNs one segment holds.
pub(crate) const SEGMENT_SPAN: u64 = SEGMENT_FRAMES * FRAME_BYTES;

/// Frame kind of a page image.
pub(crate) const KIND_PAGE: u8 = 1;
/// Frame kind of a commit frame.
pub(crate) const KIND_COMMIT: u8 = 2;

/// Bytes of the zero buffer a new segment is written from.
const ZERO_CHUNK: usize = 1 << 20;

/// Name of a segment being created, before it is renamed to its segment
/// name ([`Wal::create_segment`]). Not a segment name.
pub(crate) const TEMP_SEGMENT: &str = "wal-temp";

/// One frame of zeros.
static ZERO_FRAME: [u8; FRAME_SIZE] = [0; FRAME_SIZE];

/// Grows `frames` with zeroed frame slots to `len` bytes, a multiple of
/// [`FRAME_SIZE`]. Copies from [`ZERO_FRAME`] rather than `Vec::resize`,
/// which without optimisation (the test profile) fills byte by byte and
/// was 19 % of a recovery test's CPU time (`perf`, 2026-10-02).
fn grow_frames(frames: &mut Vec<u8>, len: usize) {
    while frames.len() < len {
        frames.extend_from_slice(&ZERO_FRAME);
    }
}

/// The counters a commit frame and `CONTROL` record (TS-05, TS-09): the
/// highest commit timestamp and the next element, catalog-object and token
/// identifiers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counters {
    /// Highest commit timestamp.
    pub(crate) commit_ts: u64,
    /// Next element identifier.
    pub(crate) next_element_id: u64,
    /// Next catalog object identifier.
    pub(crate) next_catalog_id: u64,
    /// Next token identifier.
    pub(crate) next_token_id: u64,
}

impl Counters {
    /// Each counter as the larger of `self` and `o` (TS-13 step 6).
    #[must_use]
    pub(crate) fn max(self, o: Counters) -> Counters {
        Counters {
            commit_ts: self.commit_ts.max(o.commit_ts),
            next_element_id: self.next_element_id.max(o.next_element_id),
            next_catalog_id: self.next_catalog_id.max(o.next_catalog_id),
            next_token_id: self.next_token_id.max(o.next_token_id),
        }
    }
}

/// The name of segment `lsn` of generation `g`: `wal-<g>-<lsn>`, the LSN as
/// 16 lowercase hexadecimal digits (TS-09).
pub(crate) fn segment_name(g: u64, lsn: u64) -> String {
    format!("wal-{g}-{lsn:016x}")
}

/// The generation and first LSN of a segment name, or `None` if `name` is
/// not exactly a segment name.
pub(crate) fn parse_segment_name(name: &OsStr) -> Option<(u64, u64)> {
    let (g, lsn) = name.to_str()?.strip_prefix("wal-")?.split_once('-')?;
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    if lsn.len() != 16 || !lsn.bytes().all(hex) {
        return None;
    }
    let g_value: u64 = g.parse().ok()?;
    if g_value.to_string() != g {
        return None;
    }
    Some((g_value, u64::from_str_radix(lsn, 16).ok()?))
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8-byte field"))
}

fn put_u64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// The commit record of a commit frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommitRecord {
    /// LSN of the group's first frame.
    pub(crate) first_lsn: u64,
    /// Lowest commit timestamp in the group.
    pub(crate) first_ts: u64,
    /// Counters after the group's last commit; `commit_ts` is the group's
    /// highest commit timestamp.
    pub(crate) counters: Counters,
}

/// The decoded header of a valid frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameHead {
    /// [`KIND_PAGE`] or [`KIND_COMMIT`].
    pub(crate) kind: u8,
    /// Salt of the open that wrote it.
    pub(crate) salt: u64,
    /// Page number of a page frame.
    pub(crate) page: u64,
}

/// Writes the header and checksum of frame `f` (exactly [`FRAME_SIZE`]
/// bytes), whose payload is already in place.
pub(crate) fn seal_frame(f: &mut [u8], kind: u8, lsn: u64, salt: u64, page: u64) {
    debug_assert_eq!(f.len(), FRAME_SIZE);
    f[4] = kind;
    f[5..8].fill(0);
    put_u64(f, 8, lsn);
    put_u64(f, 16, salt);
    put_u64(f, 24, page);
    let crc = crc32c::crc32c(&f[4..]);
    f[0..4].copy_from_slice(&crc.to_le_bytes());
}

/// Writes a commit record into the payload of frame `f`.
pub(crate) fn put_commit(f: &mut [u8], r: &CommitRecord) {
    let p = &mut f[FRAME_HEADER..];
    p.fill(0);
    put_u64(p, 0, r.first_lsn);
    put_u64(p, 8, r.first_ts);
    put_u64(p, 16, r.counters.commit_ts);
    put_u64(p, 24, r.counters.next_element_id);
    put_u64(p, 32, r.counters.next_catalog_id);
    put_u64(p, 40, r.counters.next_token_id);
}

/// The commit record of a commit frame `f`.
pub(crate) fn commit_record(f: &[u8]) -> CommitRecord {
    let p = &f[FRAME_HEADER..];
    CommitRecord {
        first_lsn: u64_at(p, 0),
        first_ts: u64_at(p, 8),
        counters: Counters {
            commit_ts: u64_at(p, 16),
            next_element_id: u64_at(p, 24),
            next_catalog_id: u64_at(p, 32),
            next_token_id: u64_at(p, 40),
        },
    }
}

/// The header of frame `f` read at position `lsn`, or `None` if the frame
/// is not valid there: a short read, a checksum mismatch, an unknown kind,
/// or an LSN other than `lsn` (TS-09).
pub(crate) fn decode_frame(f: &[u8], lsn: u64) -> Option<FrameHead> {
    if f.len() != FRAME_SIZE {
        return None;
    }
    let crc = u32::from_le_bytes(f[0..4].try_into().expect("4-byte field"));
    if crc != crc32c::crc32c(&f[4..]) || u64_at(f, 8) != lsn {
        return None;
    }
    let kind = f[4];
    if kind != KIND_PAGE && kind != KIND_COMMIT {
        return None;
    }
    Some(FrameHead {
        kind,
        salt: u64_at(f, 16),
        page: u64_at(f, 24),
    })
}

/// The open group: the pages committers have changed since the writer last
/// closed a group, each with its frame position and, once its commit has
/// joined, its final image. Changed only under the commit latch.
pub(crate) struct OpenGroup {
    /// Number of this group: 1 for the first group of an open.
    seq: u64,
    /// LSN of the group's first frame.
    start: u64,
    /// Page of each frame position, in position order.
    pages: Vec<u64>,
    /// Frame index of each page in `pages`.
    index: HashMap<u64, u32>,
    /// One frame-sized slot per page; the payload holds the page image.
    frames: Vec<u8>,
    /// Commits that have joined.
    commits: u64,
    /// Lowest commit timestamp that joined.
    first_ts: u64,
    /// Counters after the last commit that joined.
    counters: Counters,
}

impl OpenGroup {
    /// An empty group numbered `seq`, starting at `start`, reusing the
    /// allocation of `frames`.
    pub(crate) fn new(seq: u64, start: u64, mut frames: Vec<u8>, counters: Counters) -> OpenGroup {
        frames.clear();
        OpenGroup {
            seq,
            start,
            pages: Vec::new(),
            index: HashMap::new(),
            frames,
            commits: 0,
            first_ts: 0,
            counters,
        }
    }

    /// The group number.
    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    /// The page LSN of `page` in this group: its frame position plus one
    /// frame. The page takes the next position on its first change in the
    /// group and keeps it afterwards (TS-09).
    pub(crate) fn page_lsn(&mut self, page: u64) -> u64 {
        let i = match self.index.get(&page) {
            Some(&i) => i,
            None => {
                let i = u32::try_from(self.pages.len()).expect("fewer than 2^32 pages per group");
                self.pages.push(page);
                self.index.insert(page, i);
                i
            }
        };
        self.start + (u64::from(i) + 1) * FRAME_BYTES
    }

    /// The number of positions taken, for [`OpenGroup::rollback`].
    pub(crate) fn mark(&self) -> usize {
        self.pages.len()
    }

    /// Gives back every position taken since `mark` (TS-16 step 6 undo).
    pub(crate) fn rollback(&mut self, mark: usize) {
        for p in self.pages.drain(mark..) {
            self.index.remove(&p);
        }
        self.frames.truncate(self.pages.len() * FRAME_SIZE);
    }

    /// The lowest durable LSN at which every commit that has joined this
    /// group or an earlier one is durable: this group's end if a commit has
    /// joined it, else its start. Positions only grow within a group and
    /// groups become durable in order, so once the durable LSN reaches this
    /// value, no joined commit can still be lost (TS-16 step 6 undo).
    pub(crate) fn joined_durable_lsn(&self) -> u64 {
        if self.has_commits() {
            self.end()
        } else {
            self.start
        }
    }

    /// The WAL end after this group if it were closed now: its page frames
    /// and its commit frame.
    pub(crate) fn end(&self) -> u64 {
        self.start + (self.pages.len() as u64 + 1) * FRAME_BYTES
    }

    /// Copies `image`, the final image of `page` in the joining commit, into
    /// the page's frame slot (TS-09 Capture).
    pub(crate) fn capture(&mut self, page: u64, image: &[u8]) {
        let i = self.index[&page] as usize;
        grow_frames(&mut self.frames, self.pages.len() * FRAME_SIZE);
        let at = i * FRAME_SIZE + FRAME_HEADER;
        self.frames[at..at + PAGE_SIZE].copy_from_slice(image);
    }

    /// Records that a commit with timestamp `commit_ts` joined, leaving the
    /// counters at `counters`.
    pub(crate) fn join(&mut self, commit_ts: u64, counters: Counters) {
        if self.commits == 0 {
            self.first_ts = commit_ts;
        }
        self.commits += 1;
        self.counters = counters;
    }

    /// Whether any commit has joined.
    pub(crate) fn has_commits(&self) -> bool {
        self.commits > 0
    }

    /// Closes the group: the next group, numbered one higher, starts at this
    /// group's end with the allocation of `spare`.
    pub(crate) fn close(&mut self, spare: Vec<u8>) -> ClosedGroup {
        let next = OpenGroup::new(self.seq + 1, self.end(), spare, self.counters);
        let g = std::mem::replace(self, next);
        ClosedGroup {
            seq: g.seq,
            start: g.start,
            pages: g.pages,
            frames: g.frames,
            commits: g.commits,
            first_ts: g.first_ts,
            counters: g.counters,
        }
    }
}

/// A group the writer has taken from the commit latch.
pub(crate) struct ClosedGroup {
    seq: u64,
    start: u64,
    pages: Vec<u64>,
    frames: Vec<u8>,
    commits: u64,
    first_ts: u64,
    counters: Counters,
}

impl ClosedGroup {
    /// The WAL end after this group.
    fn end(&self) -> u64 {
        self.start + (self.pages.len() as u64 + 1) * FRAME_BYTES
    }

    /// Fills every frame header and the commit frame, with `salt`.
    fn seal(&mut self, salt: u64) {
        let n = self.pages.len();
        grow_frames(&mut self.frames, (n + 1) * FRAME_SIZE);
        self.frames.truncate((n + 1) * FRAME_SIZE);
        for (i, f) in self
            .frames
            .as_chunks_mut::<FRAME_SIZE>()
            .0
            .iter_mut()
            .enumerate()
        {
            let lsn = self.start + i as u64 * FRAME_BYTES;
            if i < n {
                debug_assert_eq!(
                    super::page::lsn(&f[FRAME_HEADER..]),
                    lsn + FRAME_BYTES,
                    "page image carries its frame's page LSN"
                );
                seal_frame(f, KIND_PAGE, lsn, salt, self.pages[i]);
            } else {
                put_commit(
                    f,
                    &CommitRecord {
                        first_lsn: self.start,
                        first_ts: self.first_ts,
                        counters: self.counters,
                    },
                );
                seal_frame(f, KIND_COMMIT, lsn, salt, 0);
            }
        }
    }
}

/// The segment files, by first LSN.
type SegmentMap = BTreeMap<u64, Arc<dyn StorageFile>>;

/// Progress shared by the writer, committers and the checkpointer.
#[derive(Default)]
struct Flow {
    /// Highest group number a committer has joined.
    joined: u64,
    /// Highest group number that is durable.
    durable: u64,
    /// Counters of the last durable group.
    durable_counters: Counters,
    /// Set when the writer fails; every waiter then fails.
    failed: Option<(io::ErrorKind, String)>,
    /// Set by [`Wal::stop`].
    stop: bool,
    /// Incremented by every undone commit (TS-16 step 6).
    undo_epoch: u64,
}

/// Figures of one checkpoint's step 5.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recycled {
    /// Segments renamed to follow the newest one.
    pub(crate) renamed: u64,
    /// Segments removed.
    pub(crate) removed: u64,
}

/// The message waiters fail with when the writer thread panics.
pub(crate) const WRITER_PANICKED: &str = "the WAL writer panicked";

/// Fails the waiters of the WAL if the writer thread unwinds.
struct FailWaitersOnPanic<'a>(&'a Wal);

impl Drop for FailWaitersOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let _ = self.0.health.fail(Error::Unusable);
            self.0
                .fail_waiters(io::ErrorKind::Other, WRITER_PANICKED.into());
        }
    }
}

/// The WAL of one open database.
pub(crate) struct Wal {
    location: PathBuf,
    storage: Arc<dyn Storage>,
    generation: u64,
    /// Salt of this open (TS-09); set before the writer starts.
    salt: AtomicU64,
    segments: Mutex<SegmentMap>,
    health: Arc<Health>,
    sink: Arc<dyn MetricsSink>,
    /// The durable LSN, read by eviction (TS-08).
    durable_lsn: Arc<AtomicU64>,
    /// Highest published commit timestamp (TS-16 step 9).
    published: AtomicU64,
    flow: Mutex<Flow>,
    /// The writer waits here for a joined group or a stop.
    writer_cv: Condvar,
    /// Committers and the checkpointer wait here for durability.
    durable_cv: Condvar,
    /// Tests only: pause points (TS-30 R10).
    #[cfg(test)]
    pub(crate) points: TestPoints,
}

/// A test hook run at a pause point.
#[cfg(test)]
pub(crate) type Hook = Box<dyn Fn() + Send + Sync>;

/// Test-only pause points: a test installs a hook that runs at a named
/// point, so it can order its own steps against the writer and the
/// committers without timing them (TS-30 R10). Turso consults an injected
/// hook at named state-machine boundaries the same way, only in test
/// builds (R-TURSO `core/mvcc/yield_points.rs:1-40`).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestPoints {
    /// After a segment is created and its name is durable.
    pub(crate) segment_created: Mutex<Option<Hook>>,
    /// In the writer, after its health check and before it takes the
    /// commit latch to close a group.
    pub(crate) before_close: Mutex<Option<Hook>>,
}

#[cfg(test)]
impl TestPoints {
    fn run(point: &Mutex<Option<Hook>>) {
        if let Some(hook) = point
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            hook();
        }
    }
}

impl Wal {
    /// The WAL of generation `generation` in `location`, with the segment
    /// files recovery found, the durable LSN at `wal_end` and the counters
    /// of the last durable state.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of an open database the WAL shares"
    )]
    pub(crate) fn new(
        location: &Path,
        storage: Arc<dyn Storage>,
        generation: u64,
        segments: BTreeMap<u64, Box<dyn StorageFile>>,
        durable_lsn: Arc<AtomicU64>,
        counters: Counters,
        health: Arc<Health>,
        sink: Arc<dyn MetricsSink>,
    ) -> Wal {
        let segments = segments
            .into_iter()
            .map(|(lsn, f)| (lsn, Arc::from(f)))
            .collect();
        let wal = Wal {
            location: location.to_path_buf(),
            storage,
            generation,
            salt: AtomicU64::new(0),
            segments: Mutex::new(segments),
            health,
            sink,
            durable_lsn,
            published: AtomicU64::new(counters.commit_ts),
            flow: Mutex::new(Flow {
                durable_counters: counters,
                ..Flow::default()
            }),
            writer_cv: Condvar::new(),
            durable_cv: Condvar::new(),
            #[cfg(test)]
            points: TestPoints::default(),
        };
        wal.report_size();
        wal
    }

    fn flow(&self) -> MutexGuard<'_, Flow> {
        // Flow updates are single assignments; a panic cannot leave them
        // half done.
        self.flow.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn segments(&self) -> MutexGuard<'_, SegmentMap> {
        self.segments.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the salt of this open: the sequence number of the `CONTROL`
    /// slot written by open's checkpoint (TS-09).
    pub(crate) fn set_salt(&self, salt: u64) {
        self.salt.store(salt, Ordering::Release);
    }

    /// The salt of this open.
    pub(crate) fn salt(&self) -> u64 {
        self.salt.load(Ordering::Acquire)
    }

    /// The durable LSN.
    pub(crate) fn durable_lsn(&self) -> u64 {
        self.durable_lsn.load(Ordering::Acquire)
    }

    /// The durable LSN and the counters of the last durable group, read
    /// together (TS-11 step 1: the redo point and what a checkpoint saves).
    pub(crate) fn durable_point(&self) -> (u64, Counters) {
        let f = self.flow();
        (self.durable_lsn(), f.durable_counters)
    }

    /// The highest published commit timestamp (TS-16 step 9).
    pub(crate) fn published(&self) -> u64 {
        self.published.load(Ordering::Acquire)
    }

    /// The first LSN of each segment, in order.
    pub(crate) fn segment_lsns(&self) -> Vec<u64> {
        self.segments().keys().copied().collect()
    }

    fn path(&self, lsn: u64) -> PathBuf {
        self.location.join(segment_name(self.generation, lsn))
    }

    fn report_size(&self) {
        let n = self.segments().len() as u64;
        self.sink.gauge(SIZE, n * SEGMENT_SIZE);
    }

    /// Creates segment `lsn` (TS-09): the zeros are written to
    /// [`TEMP_SEGMENT`] and synced, which is then renamed to the segment's
    /// name and made durable with `sync_dir`. A crash at any point leaves
    /// either no file with the segment's name or a complete, synced one, so
    /// open never finds a partial segment under a segment name; open
    /// removes a leftover [`TEMP_SEGMENT`]. PostgreSQL creates a segment the
    /// same way: it fills and fsyncs `xlogtemp.<pid>`, installs it with
    /// `durable_rename` (R-PG `src/backend/access/transam/xlog.c:3343-3459,
    /// 3675-3720`), and removes leftover temporary segments at recovery
    /// (`RemoveTempXlogFiles`, `xlog.c:3940-3965`).
    ///
    /// On ENOSPC the temporary file is removed and the error returned with
    /// the database usable (FR-REC-007); any other failure sets the unusable
    /// latch. Called with the segment list locked, so one segment is
    /// created at a time.
    fn create_segment(&self, lsn: u64) -> Result<Box<dyn StorageFile>> {
        let temp = self.location.join(TEMP_SEGMENT);
        let path = self.path(lsn);
        let file = match self.storage.create(&temp) {
            Ok(f) => f,
            Err(e) => return Err(self.io_failure(e)),
        };
        let zeros = vec![0u8; ZERO_CHUNK];
        let mut at = 0;
        let mut r = Ok(());
        while at < SEGMENT_SIZE && r.is_ok() {
            r = file.write_at(&zeros, at);
            at += ZERO_CHUNK as u64;
        }
        let r = r.and_then(|()| file.sync_data());
        if let Err(e) = r {
            drop(file);
            if e.kind() == io::ErrorKind::StorageFull {
                // Leave no partial file for the next attempt to trip on.
                if let Err(e) = self
                    .storage
                    .remove(&temp)
                    .and_then(|()| self.storage.sync_dir(&self.location))
                {
                    return Err(self.health.fail(e.into()));
                }
            }
            return Err(self.io_failure(e));
        }
        if let Err(e) = self
            .storage
            .rename(&temp, &path)
            .and_then(|()| self.storage.sync_dir(&self.location))
        {
            return Err(self.health.fail(e.into()));
        }
        self.sink.counter(SEGMENTS_CREATED, 1);
        #[cfg(test)]
        TestPoints::run(&self.points.segment_created);
        Ok(file)
    }

    /// ENOSPC passes through; any other I/O error sets the unusable latch.
    fn io_failure(&self, e: io::Error) -> Error {
        let e = Error::from(e);
        if e.is_no_space() {
            e
        } else {
            self.health.fail(e)
        }
    }

    /// Makes sure the segment that holds LSN `end` exists, creating the
    /// missing segments after the newest one (TS-09 creation order). Called
    /// by a committer under the commit latch, before its commit joins, so
    /// that ENOSPC fails only that commit (FR-REC-007).
    ///
    /// # Errors
    ///
    /// ENOSPC, with the database usable; [`Error::Io`], which sets the
    /// unusable latch.
    pub(crate) fn ensure_through(&self, end: u64) -> Result<()> {
        let mut segs = self.segments();
        loop {
            let (&newest, _) = segs
                .last_key_value()
                .expect("open creates the first segment");
            if newest + SEGMENT_SPAN > end {
                return Ok(());
            }
            let lsn = newest + SEGMENT_SPAN;
            let file = self.create_segment(lsn)?;
            segs.insert(lsn, Arc::from(file));
            drop(segs);
            self.report_size();
            segs = self.segments();
        }
    }

    /// Creates segment `lsn` if no segment exists yet (TS-13 step 8, empty
    /// log).
    ///
    /// # Errors
    ///
    /// As [`Wal::ensure_through`].
    pub(crate) fn create_first(&self, lsn: u64) -> Result<()> {
        let mut segs = self.segments();
        if segs
            .range(..=lsn)
            .next_back()
            .is_some_and(|(&f, _)| f + SEGMENT_SPAN > lsn)
        {
            return Ok(());
        }
        let file = self.create_segment(lsn)?;
        segs.insert(lsn, Arc::from(file));
        drop(segs);
        self.report_size();
        Ok(())
    }

    /// Writes the frames of `g` (sealed) and syncs every segment they touch.
    fn write_group(&self, g: &ClosedGroup) -> io::Result<u64> {
        let segs: Vec<(u64, Arc<dyn StorageFile>)> = {
            let map = self.segments();
            let first = map
                .range(..=g.start)
                .next_back()
                .map(|(&k, _)| k)
                .unwrap_or(g.start);
            map.range(first..g.end())
                .map(|(&k, f)| (k, Arc::clone(f)))
                .collect()
        };
        let mut lsn = g.start;
        let end = g.end();
        let mut touched = Vec::new();
        for (first, file) in &segs {
            let limit = first + SEGMENT_SPAN;
            if lsn >= limit || lsn >= end {
                continue;
            }
            let stop = limit.min(end);
            let from = (lsn - g.start) as usize;
            let to = (stop - g.start) as usize;
            file.write_at(&g.frames[from..to], lsn - first)?;
            touched.push(file);
            lsn = stop;
        }
        if lsn != end {
            return Err(io::Error::other(format!(
                "no WAL segment holds LSN {lsn:#x}; the creation order was broken"
            )));
        }
        for file in &touched {
            file.sync_data()?;
        }
        Ok(touched.len() as u64)
    }

    /// The writer loop of thread `lpg-wal` (TS-09 Writer). `close` takes the
    /// commit latch and closes the open group, reusing the given buffer.
    /// Returns when [`Wal::stop`] is called and every joined group is
    /// durable, or after a failure.
    pub(crate) fn run_writer(&self, mut close: impl FnMut(Vec<u8>) -> Option<ClosedGroup>) {
        // A panic of this thread fails every waiter instead of leaving it
        // blocked: no group can become durable any more.
        let _guard = FailWaitersOnPanic(self);
        let mut spare = Vec::new();
        loop {
            {
                let mut f = self.flow();
                loop {
                    if f.failed.is_some() {
                        return;
                    }
                    if f.joined > f.durable {
                        break;
                    }
                    if f.stop {
                        return;
                    }
                    f = self
                        .writer_cv
                        .wait(f)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
            if self.health.is_unusable() {
                self.fail_waiters(io::ErrorKind::Other, "the database is unusable".into());
                return;
            }
            #[cfg(test)]
            TestPoints::run(&self.points.before_close);
            let Some(mut g) = close(std::mem::take(&mut spare)) else {
                // `close` refuses a group once the database is unusable
                // (TS-12): its commits fail instead of becoming durable.
                if self.health.is_unusable() {
                    self.fail_waiters(io::ErrorKind::Other, "the database is unusable".into());
                    return;
                }
                continue;
            };
            g.seal(self.salt());
            match self.write_group(&g) {
                Ok(syncs) => {
                    let end = g.end();
                    // The group's metrics are sent before its commits are
                    // woken, so a commit that has returned is counted
                    // (FR-OBS-001); the flow latch below orders them before
                    // every waiter's return.
                    let frames = g.pages.len() as u64 + 1;
                    self.sink.counter(GROUPS, 1);
                    self.sink.counter(FRAMES, frames);
                    self.sink.counter(APPENDED_BYTES, frames * FRAME_BYTES);
                    self.sink.counter(FLUSHES, syncs);
                    self.sink.histogram(APPEND_BATCH_SIZE, g.commits);
                    {
                        // Under the flow latch, so that a checkpoint reads the
                        // durable LSN and its counters together.
                        let mut f = self.flow();
                        // Release pairs with the Acquire loads of eviction and
                        // of the checkpoint (TS-08 WAL before data).
                        self.durable_lsn.store(end, Ordering::Release);
                        // Groups become durable in order, so timestamps are
                        // published in order (TS-16 step 9).
                        self.published
                            .fetch_max(g.counters.commit_ts, Ordering::AcqRel);
                        f.durable = g.seq;
                        f.durable_counters = g.counters;
                    }
                    self.durable_cv.notify_all();
                    spare = g.frames;
                    self.precreate(end);
                }
                Err(e) => {
                    let msg = e.to_string();
                    let kind = e.kind();
                    let _ = self.health.fail(Error::Io(e));
                    self.fail_waiters(kind, msg);
                    return;
                }
            }
        }
    }

    /// Creates the segment after the newest one once the WAL end is past
    /// three quarters of the newest, so that committers rarely create one
    /// (PostgreSQL `PreallocXlogFiles`, R-PG
    /// `src/backend/access/transam/xlog.c:3802-3824`). A failure here fails
    /// no commit: ENOSPC is left for the committer that needs the segment;
    /// any other error sets the unusable latch.
    fn precreate(&self, end: u64) {
        let mut segs = self.segments();
        let Some((&newest, _)) = segs.last_key_value() else {
            return;
        };
        if end < newest + SEGMENT_SPAN / 4 * 3 {
            return;
        }
        let lsn = newest + SEGMENT_SPAN;
        if let Ok(file) = self.create_segment(lsn) {
            segs.insert(lsn, Arc::from(file));
            drop(segs);
            self.report_size();
        }
    }

    /// Makes every current and later waiter of [`Wal::wait_durable`] fail
    /// with `kind` and `msg`.
    fn fail_waiters(&self, kind: io::ErrorKind, msg: String) {
        self.flow().failed = Some((kind, msg));
        self.durable_cv.notify_all();
    }

    /// Step 8 of TS-16: wakes the writer for group `seq` and waits until it
    /// is durable.
    ///
    /// # Errors
    ///
    /// The writer's failure, as [`Error::Io`] (the unusable latch is set).
    pub(crate) fn wait_durable(&self, seq: u64) -> Result<()> {
        let mut f = self.flow();
        if f.joined < seq {
            f.joined = seq;
            self.writer_cv.notify_one();
        }
        loop {
            if f.durable >= seq {
                return Ok(());
            }
            if let Some((kind, msg)) = &f.failed {
                return Err(Error::Io(io::Error::new(*kind, msg.clone())));
            }
            f = self
                .durable_cv
                .wait(f)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The number of commits undone so far.
    pub(crate) fn undo_epoch(&self) -> u64 {
        self.flow().undo_epoch
    }

    /// Records an undone commit and wakes the waiters of
    /// [`Wal::wait_lsn`].
    pub(crate) fn note_undo(&self) {
        self.flow().undo_epoch += 1;
        self.durable_cv.notify_all();
    }

    /// Returns `false` as soon as a commit has been undone since `epoch`
    /// was read, even if `lsn` is durable: an undone commit gives its frame
    /// positions back, so `lsn` may never become durable, and it puts pages
    /// back to older images that the caller must visit again. Otherwise
    /// waits until the durable LSN reaches `lsn` and returns `true`.
    ///
    /// # Errors
    ///
    /// The writer's failure, or [`Error::Unusable`].
    pub(crate) fn wait_lsn(&self, lsn: u64, epoch: u64) -> Result<bool> {
        let mut f = self.flow();
        loop {
            // The epoch first: after an undo, a page seen before it may hold
            // an older image again, whatever the durable LSN now is, and the
            // caller must visit it again.
            if f.undo_epoch != epoch {
                return Ok(false);
            }
            if self.durable_lsn() >= lsn {
                return Ok(true);
            }
            if let Some((kind, msg)) = &f.failed {
                return Err(Error::Io(io::Error::new(*kind, msg.clone())));
            }
            self.health.check()?;
            // The writer flushes whatever has joined; a commit still in its
            // apply step joins or is undone, and both wake this wait.
            f = self
                .durable_cv
                .wait_timeout(f, std::time::Duration::from_millis(100))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Asks the writer to stop once every joined group is durable.
    pub(crate) fn stop(&self) {
        self.flow().stop = true;
        self.writer_cv.notify_all();
    }

    /// Step 5 of TS-11: each segment that holds only LSNs below `redo` is
    /// renamed to follow the newest segment, or removed when the segments
    /// after the one holding `wal_end` already hold at least `used` LSNs,
    /// the WAL used since the previous checkpoint. Each rename or removal is
    /// made durable with its own `sync_dir` before the next one, and before
    /// a renamed segment can be written.
    ///
    /// # Errors
    ///
    /// [`Error::Io`], which sets the unusable latch.
    pub(crate) fn recycle(&self, redo: u64, wal_end: u64, used: u64) -> Result<Recycled> {
        let mut segs = self.segments();
        let old: Vec<u64> = segs
            .keys()
            .copied()
            .filter(|&f| f + SEGMENT_SPAN <= redo)
            .collect();
        if old.is_empty() {
            return Ok(Recycled::default());
        }
        let mut out = Recycled::default();
        let holder = segs
            .range(..=wal_end)
            .next_back()
            .map_or(wal_end, |(&f, _)| f);
        let mut newest = *segs.keys().next_back().expect("segments exist");
        for lsn in old {
            let file = segs.remove(&lsn).expect("listed segment");
            let ahead = newest - holder;
            // Each rename or removal is made durable before the next, as
            // PostgreSQL's `durable_rename` and `durable_unlink` do per
            // segment (R-PG `src/backend/access/transam/xlog.c:3714,
            // 4184`; `src/common/file_utils.c:483`). With one `sync_dir`
            // for several renames, a crash could keep a later rename
            // without an earlier one; open would then find a gap in the
            // chain of segments and refuse the database (TS-13 step 4).
            let r = if ahead >= used {
                drop(file);
                out.removed += 1;
                self.storage.remove(&self.path(lsn))
            } else {
                let to = newest + SEGMENT_SPAN;
                newest = to;
                out.renamed += 1;
                segs.insert(to, file);
                self.storage.rename(&self.path(lsn), &self.path(to))
            };
            r.and_then(|()| self.storage.sync_dir(&self.location))
                .map_err(|e| self.health.fail(e.into()))?;
        }
        drop(segs);
        self.report_size();
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests;
