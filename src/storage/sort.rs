//! External merge sort (TS-26 fast-load step 2, TS-18 step 4; task #17).
//!
//! A [`Sorter`] takes `(key, value)` records in any order and returns them
//! in byte-wise key order. Its memory is one reservation from the memory
//! accountant, made when it starts, as a sort buffer (TS-08); every buffer
//! it allocates is sized from that reservation, so the sort never goes over
//! the cap.
//!
//! - **Runs.** Records are copied into an arena with an index of their
//!   offsets. When either is full, the index is sorted (keys compared in
//!   place, no allocation) and the records are written in that order to a
//!   temporary file through the storage trait (TS-03), the run.
//! - **Merge.** If everything fit in memory, the arena is returned in index
//!   order and no file is written. Otherwise runs are combined by a k-way
//!   merge on a binary heap. Each open run needs one read buffer; if the
//!   reservation cannot hold a buffer for every run, groups of runs are
//!   first merged into longer runs, as PostgreSQL's polyphase merge does
//!   when its tape count is limited (R-PG `src/backend/utils/sort/tuplesort.c:11-15,
//!   "Knuth's Algorithm D"`; `tuplesort_merge_order`, `tuplesort.c`). SQLite's
//!   `vdbesort.c` likewise writes sorted "packed memory arrays" to a
//!   temporary file and merges them with a fixed fan-in
//!   (`SORTER_MAX_MERGE_COUNT` = 16, R-SQLITE `src/vdbesort.c:310`) in
//!   several passes when needed.
//! - **Files.** Temporary files are named `<prefix>-<n>` in the caller's
//!   directory and are removed when the sort ends, whether it succeeds or
//!   fails (the [`Drop`] of [`Sorter`] and [`Sorted`]). They are never
//!   synced: like PostgreSQL's logical tapes (`logtape.c`), they hold no
//!   state a crash must keep. A file of the same name left by a sort that a
//!   crash interrupted is replaced. The sort of a database
//!   ([`Engine::sorter`]) keeps its files in the database location under
//!   the fixed prefix [`SORT_TEMP`], and open removes every such file, as
//!   PostgreSQL removes temporary files at startup (`RemovePgTempFiles`,
//!   R-PG `src/backend/postmaster/postmaster.c:1332`).
//!
//! A run record is a 2-byte key length, a 4-byte value length, the key and
//! the value, integers little-endian. Records of equal keys are all kept, in
//! no particular order; the bulk build rejects them.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use super::btree::MAX_KEY;
use super::engine::Engine;
use super::error::{Error, Result};
use super::memory::Reservation;
use super::pool::Pool;
use super::{Storage, StorageFile};

/// Prefix of the temporary files of a database's sorts:
/// `sort-temp-<sort>-<run>` in the database location.
pub(crate) const SORT_TEMP: &str = "sort-temp-";

/// Whether `name` is a temporary file of a database's sort.
pub(crate) fn is_sort_temp(name: &OsStr) -> bool {
    name.to_str().is_some_and(|n| n.starts_with(SORT_TEMP))
}

/// Bytes of a record header.
const HEAD: usize = 6;
/// Smallest reservation a sort accepts.
pub(crate) const MIN_MEMORY: u64 = 64 << 10;
/// Smallest read buffer of one run during a merge.
const MIN_READ: usize = 4 << 10;
/// Bytes per index entry.
const INDEX_ENTRY: usize = std::mem::size_of::<usize>();
/// Memory one heap entry may hold besides its read buffer: the key copy
/// (at most 1 024 bytes, TS-05) and the entry itself.
const HEAP_ENTRY: usize = 1024 + 64;

fn put_record(out: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(value);
}

/// The key and value of the record at `at` in `b`, and its length.
fn record(b: &[u8], at: usize) -> (&[u8], &[u8], usize) {
    let k = usize::from(u16::from_le_bytes([b[at], b[at + 1]]));
    let v = u32::from_le_bytes(b[at + 2..at + 6].try_into().expect("4 bytes")) as usize;
    let key = &b[at + HEAD..at + HEAD + k];
    let value = &b[at + HEAD + k..at + HEAD + k + v];
    (key, value, HEAD + k + v)
}

/// The length of the record whose header starts `b` (at least [`HEAD`]
/// bytes).
fn record_len(b: &[u8]) -> usize {
    HEAD + usize::from(u16::from_le_bytes([b[0], b[1]]))
        + u32::from_le_bytes(b[2..6].try_into().expect("4 bytes")) as usize
}

/// A run written to a temporary file.
struct Run {
    path: PathBuf,
    len: u64,
}

/// The temporary files of one sort, removed when dropped.
struct Files<'a> {
    storage: &'a dyn Storage,
    dir: PathBuf,
    prefix: String,
    next: u64,
    live: Vec<PathBuf>,
}

impl Files<'_> {
    fn create(&mut self) -> Result<(PathBuf, Box<dyn StorageFile>)> {
        let path = self.dir.join(format!("{}-{}", self.prefix, self.next));
        self.next += 1;
        let file = match self.storage.create(&path) {
            // Left by a sort a crash interrupted: it holds no state.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                self.storage.remove(&path)?;
                self.storage.create(&path)?
            }
            r => r?,
        };
        self.live.push(path.clone());
        Ok((path, file))
    }

    fn remove(&mut self, path: &Path) -> Result<()> {
        self.live.retain(|p| p != path);
        self.storage.remove(path)?;
        Ok(())
    }
}

impl Drop for Files<'_> {
    fn drop(&mut self) {
        for p in self.live.drain(..) {
            // Nothing to report from a drop: a file left behind holds no
            // database state.
            let _ = self.storage.remove(&p);
        }
    }
}

/// Writes records to a run file through a buffer of fixed capacity.
struct RunWriter<'b> {
    file: Box<dyn StorageFile>,
    buf: &'b mut Vec<u8>,
    at: u64,
}

impl RunWriter<'_> {
    fn add(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if self.buf.len() + HEAD + key.len() + value.len() > self.buf.capacity() {
            self.flush()?;
        }
        if HEAD + key.len() + value.len() > self.buf.capacity() {
            // A record longer than the buffer goes out on its own.
            let mut one = Vec::with_capacity(HEAD + key.len() + value.len());
            put_record(&mut one, key, value);
            self.file.write_at(&one, self.at)?;
            self.at += one.len() as u64;
            return Ok(());
        }
        put_record(self.buf, key, value);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if !self.buf.is_empty() {
            self.file.write_at(self.buf, self.at)?;
            self.at += self.buf.len() as u64;
            self.buf.clear();
        }
        Ok(())
    }
}

/// An external merge sort in progress.
pub(crate) struct Sorter<'a> {
    files: Files<'a>,
    /// The accountant's bytes for every buffer below.
    budget: Reservation,
    arena: Vec<u8>,
    index: Vec<usize>,
    out: Vec<u8>,
    runs: Vec<Run>,
    /// Longest record seen.
    longest: usize,
    records: u64,
}

impl Engine {
    /// Starts a sort of this database, reserving `memory` bytes as a sort
    /// buffer (TS-08). Its temporary files are `sort-temp-<sort>-<run>` in
    /// the database location, with a number `<sort>` unique in this open;
    /// open removes any left by a crash.
    ///
    /// # Errors
    ///
    /// As [`Sorter::new`].
    pub(crate) fn sorter(&self, memory: u64) -> Result<Sorter<'_>> {
        let core = self.core();
        let n = core
            .sorts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Sorter::new(
            &*core.storage,
            &core.location,
            &format!("{SORT_TEMP}{n}"),
            &core.pool,
            memory,
        )
    }
}

impl<'a> Sorter<'a> {
    /// Starts a sort whose temporary files are `<prefix>-<n>` in `dir`,
    /// reserving `memory` bytes from `pool`'s accountant as a sort buffer
    /// (TS-08). No other sort may use `prefix` in `dir` at the same time: a
    /// file of the same name is taken to be a leftover of a crash and
    /// replaced.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `memory` is below [`MIN_MEMORY`];
    /// [`Error::MemoryLimit`] if the reservation does not fit the cap.
    pub(crate) fn new(
        storage: &'a dyn Storage,
        dir: &Path,
        prefix: &str,
        pool: &Pool,
        memory: u64,
    ) -> Result<Sorter<'a>> {
        if memory < MIN_MEMORY {
            return Err(Error::InvalidArgument("sort memory below 64 KiB"));
        }
        let budget = pool.reserve(memory)?;
        let m = usize::try_from(memory).map_err(|_| Error::InvalidArgument("sort memory"))?;
        // An eighth for the run writer's buffer, the rest for the arena and
        // its index, sized for records of 32 bytes on average; smaller
        // records end a run when the index is full.
        let out = (m / 8).min(1 << 20);
        let rest = m - out;
        let index = rest / 4 / INDEX_ENTRY;
        let arena = rest - index * INDEX_ENTRY;
        Ok(Sorter {
            files: Files {
                storage,
                dir: dir.to_path_buf(),
                prefix: prefix.to_owned(),
                next: 0,
                live: Vec::new(),
            },
            budget,
            arena: Vec::with_capacity(arena),
            index: Vec::with_capacity(index),
            out: Vec::with_capacity(out),
            runs: Vec::new(),
            longest: 0,
            records: 0,
        })
    }

    /// Bytes reserved for the sort.
    pub(crate) fn memory(&self) -> u64 {
        self.budget.bytes()
    }

    /// Bytes the sort's buffers hold now: never above [`Sorter::memory`].
    pub(crate) fn allocated(&self) -> usize {
        self.arena.capacity() + self.index.capacity() * INDEX_ENTRY + self.out.capacity()
    }

    /// Temporary files written so far.
    pub(crate) fn runs(&self) -> usize {
        self.runs.len()
    }

    /// Adds one record.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key longer than 1 024 bytes (TS-05)
    /// or a value longer than 4 GiB; [`Error::MemoryLimit`] for a record
    /// that does not fit the arena; [`Error::Io`] if a run cannot be
    /// written.
    pub(crate) fn push(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if key.len() > MAX_KEY || u32::try_from(value.len()).is_err() {
            return Err(Error::InvalidArgument("sort record too long"));
        }
        let len = HEAD + key.len() + value.len();
        if len > self.arena.capacity() {
            return Err(Error::MemoryLimit {
                requested: len as u64,
            });
        }
        if self.arena.len() + len > self.arena.capacity()
            || self.index.len() == self.index.capacity()
        {
            self.spill()?;
        }
        self.index.push(self.arena.len());
        put_record(&mut self.arena, key, value);
        self.longest = self.longest.max(len);
        self.records += 1;
        Ok(())
    }

    /// Sorts the index of the arena by key.
    fn sort_index(&mut self) {
        let arena = &self.arena;
        self.index
            .sort_unstable_by(|&a, &b| record(arena, a).0.cmp(record(arena, b).0));
    }

    /// Writes the arena, sorted, as a new run.
    fn spill(&mut self) -> Result<()> {
        if self.index.is_empty() {
            return Ok(());
        }
        self.sort_index();
        let (path, file) = self.files.create()?;
        let mut w = RunWriter {
            file,
            buf: &mut self.out,
            at: 0,
        };
        for &at in &self.index {
            let (k, v, _) = record(&self.arena, at);
            w.add(k, v)?;
        }
        w.flush()?;
        self.runs.push(Run { path, len: w.at });
        self.arena.clear();
        self.index.clear();
        Ok(())
    }

    /// Ends the input and returns the records in key order.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a run cannot be written or read;
    /// [`Error::MemoryLimit`] if the reservation cannot hold two read
    /// buffers for the longest record.
    pub(crate) fn finish(mut self) -> Result<Sorted<'a>> {
        if self.runs.is_empty() {
            self.sort_index();
            return Ok(Sorted {
                inner: Inner::Memory { at: 0 },
                files: self.files,
                arena: self.arena,
                index: self.index,
                _budget: self.budget,
                records: self.records,
            });
        }
        self.spill()?;
        // The arena and index are no longer needed: their bytes become read
        // buffers.
        let memory = self.allocated();
        self.arena = Vec::new();
        self.index = Vec::new();
        let read = self.longest.max(MIN_READ);
        let fan_in = memory / (read + HEAP_ENTRY);
        if fan_in < 2 {
            return Err(Error::MemoryLimit {
                requested: (2 * (read + HEAP_ENTRY)) as u64,
            });
        }
        // Earlier passes keep the output buffer, so they open fewer runs.
        while self.runs.len() > fan_in {
            let left = memory - self.out.capacity();
            let group = (left / (read + HEAP_ENTRY)).min(self.runs.len());
            if group < 2 {
                return Err(Error::MemoryLimit {
                    requested: (2 * (read + HEAP_ENTRY)) as u64,
                });
            }
            let inputs: Vec<Run> = self.runs.drain(..group).collect();
            let buf = left / group - HEAP_ENTRY;
            let mut m = Merge::open(&self.files, &inputs, buf)?;
            let (path, file) = self.files.create()?;
            let mut w = RunWriter {
                file,
                buf: &mut self.out,
                at: 0,
            };
            while let Some((k, v)) = m.next()? {
                w.add(k, v)?;
            }
            w.flush()?;
            drop(m);
            for r in &inputs {
                self.files.remove(&r.path)?;
            }
            self.runs.push(Run { path, len: w.at });
        }
        self.out = Vec::new();
        let buf = memory / self.runs.len() - HEAP_ENTRY;
        let merge = Merge::open(&self.files, &self.runs, buf)?;
        Ok(Sorted {
            inner: Inner::Merge(merge),
            files: self.files,
            arena: Vec::new(),
            index: Vec::new(),
            _budget: self.budget,
            records: self.records,
        })
    }
}

/// A run being read through a buffer.
struct Reader {
    file: Box<dyn StorageFile>,
    len: u64,
    /// File offset of `buf[0]`.
    base: u64,
    buf: Vec<u8>,
    /// Offset in `buf` of the current record.
    at: usize,
}

impl Reader {
    /// Makes the record at the current position whole in the buffer.
    /// Returns `false` at the end of the run.
    fn fill(&mut self) -> Result<bool> {
        let have = self.buf.len() - self.at;
        if have >= HEAD && have >= record_len(&self.buf[self.at..]) {
            return Ok(true);
        }
        let file_at = self.base + self.at as u64;
        if file_at >= self.len {
            return Ok(false);
        }
        // Move the partial record to the front and read after it.
        self.buf.copy_within(self.at.., 0);
        self.buf.truncate(have);
        self.base = file_at;
        self.at = 0;
        let want = (self.buf.capacity() - have).min((self.len - file_at) as usize - have);
        let old = self.buf.len();
        self.buf.resize(old + want, 0);
        let n = self
            .file
            .read_at(&mut self.buf[old..], file_at + old as u64)?;
        if n < want {
            return Err(Error::Io(std::io::Error::other(
                "sort run file shorter than written",
            )));
        }
        if self.buf.len() < HEAD || self.buf.len() < record_len(&self.buf) {
            return Err(Error::Io(std::io::Error::other(
                "sort run file ends inside a record",
            )));
        }
        Ok(true)
    }

    fn current(&self) -> (&[u8], &[u8]) {
        let (k, v, _) = record(&self.buf, self.at);
        (k, v)
    }

    fn advance(&mut self) {
        let (_, _, n) = record(&self.buf, self.at);
        self.at += n;
    }
}

/// A heap entry: a copy of a run's current key, smallest first.
struct Head {
    key: Vec<u8>,
    run: usize,
}

impl PartialEq for Head {
    fn eq(&self, o: &Head) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}

impl Eq for Head {}

impl PartialOrd for Head {
    fn partial_cmp(&self, o: &Head) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Head {
    /// Reversed, so that `BinaryHeap`, a max-heap, pops the smallest key;
    /// ties go to the lower run.
    fn cmp(&self, o: &Head) -> Ordering {
        o.key.cmp(&self.key).then(o.run.cmp(&self.run))
    }
}

/// A k-way merge of runs.
struct Merge {
    readers: Vec<Reader>,
    heap: BinaryHeap<Head>,
    /// The run whose record was returned last; it advances on the next call.
    last: Option<usize>,
    spare: Vec<Vec<u8>>,
}

impl Merge {
    fn open(files: &Files<'_>, runs: &[Run], buf: usize) -> Result<Merge> {
        let mut m = Merge {
            readers: Vec::with_capacity(runs.len()),
            heap: BinaryHeap::with_capacity(runs.len()),
            last: None,
            spare: Vec::new(),
        };
        for (i, r) in runs.iter().enumerate() {
            let mut reader = Reader {
                file: files.storage.open(&r.path)?,
                len: r.len,
                base: 0,
                buf: Vec::with_capacity(buf),
                at: 0,
            };
            if reader.fill()? {
                let mut key = Vec::with_capacity(1024);
                key.extend_from_slice(reader.current().0);
                m.heap.push(Head { key, run: i });
            }
            m.readers.push(reader);
        }
        Ok(m)
    }

    fn next(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        if let Some(run) = self.last.take() {
            let r = &mut self.readers[run];
            r.advance();
            if r.fill()? {
                let mut key = self.spare.pop().unwrap_or_default();
                key.clear();
                key.extend_from_slice(r.current().0);
                self.heap.push(Head { key, run });
            }
        }
        let Some(head) = self.heap.pop() else {
            return Ok(None);
        };
        let run = head.run;
        self.spare.push(head.key);
        self.last = Some(run);
        Ok(Some(self.readers[run].current()))
    }
}

enum Inner {
    Memory { at: usize },
    Merge(Merge),
}

/// The records of a finished sort, in key order. Its temporary files are
/// removed when it is dropped.
pub(crate) struct Sorted<'a> {
    inner: Inner,
    files: Files<'a>,
    arena: Vec<u8>,
    index: Vec<usize>,
    _budget: Reservation,
    records: u64,
}

impl Sorted<'_> {
    /// Records sorted.
    pub(crate) fn len(&self) -> u64 {
        self.records
    }

    /// The next record, or `None` after the last.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a run cannot be read.
    pub(crate) fn next_record(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        match &mut self.inner {
            Inner::Memory { at } => {
                let Some(&off) = self.index.get(*at) else {
                    return Ok(None);
                };
                *at += 1;
                let (k, v, _) = record(&self.arena, off);
                Ok(Some((k, v)))
            }
            Inner::Merge(m) => m.next(),
        }
    }

    /// Temporary files still present.
    pub(crate) fn files(&self) -> usize {
        self.files.live.len()
    }
}

#[cfg(test)]
mod tests;
