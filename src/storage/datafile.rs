//! The paged data file `data-<g>` and its header page (TS-05).
//!
//! Page 0 is the header page. After the page header ([`page::HEADER_LEN`]
//! bytes) its body holds, integers little-endian:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 24 | 8 | magic `LPGDATA\0` |
//! | 32 | 4 | format version |
//! | 36 | 4 | zero |
//! | 40 | 8 | first trunk page of the free list (0: none) |
//! | 48 | 8 | next tree identifier |
//! | 56 | 8 | page count: pages `0..page_count` are in use or free-listed |
//!
//! The page count is not among the fields TS-05 lists. Without it, the
//! pages that are reserved but unused could not be told from used ones
//! after a reopen. SQLite keeps the same field ("size of the database in
//! pages", R-SQLITE `src/btreeInt.h`, file header offset 28).
//!
//! **Growth.** The file grows only by [`Datafile::ensure`], in extents of
//! 64 MiB or 1 % of the file, whichever is larger, each reserved with
//! [`StorageFile::reserve`] before any page in it is used. Pages are then
//! handed out by [`Datafile::allocate`]. Writes go only to reserved pages, so
//! a full device fails `ensure`, never a write or a read (FR-REC-007).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::error::{Error, Health, Result};
use super::page::{self, PAGE_BYTES, PAGE_SIZE, PageType};
use super::{Storage, StorageFile, btree};
use crate::obs::MetricsSink;
use crate::obs::names::store_size::{DATABASE, RESERVED_UNUSED};

const MAGIC: [u8; 8] = *b"LPGDATA\0";

/// Minimum extent: 64 MiB of pages (TS-05).
pub(crate) const MIN_EXTENT_PAGES: u64 = (64 << 20) / PAGE_BYTES;

/// Pages a data file can hold: every byte of the last one lies at an
/// offset positional I/O can address. `pread` and `pwrite` take an `off_t`,
/// a signed 64-bit offset on Linux, so the file ends at `i64::MAX` at most.
pub(crate) const MAX_PAGES: u64 = i64::MAX as u64 / PAGE_BYTES;

/// Pages of the next extent for a file of `pages` pages: 64 MiB, or 1 % of
/// the file if that is larger.
#[must_use]
pub(crate) fn extent_pages(pages: u64) -> u64 {
    MIN_EXTENT_PAGES.max(pages.div_ceil(100))
}

/// The fields of the header page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    /// On-disk format version; must equal `CONTROL`'s (TS-05).
    pub(crate) format_version: u32,
    /// First trunk page of the persisted free list; 0 if empty.
    pub(crate) free_list_head: u64,
    /// Next tree identifier (TS-05).
    pub(crate) next_tree_id: u64,
    /// Pages in use or on the free list.
    pub(crate) page_count: u64,
}

impl Header {
    /// Writes the header page image into `buf`, sealed.
    pub(crate) fn encode(&self, buf: &mut [u8]) {
        page::init(buf, PageType::Header, 0);
        buf[24..32].copy_from_slice(&MAGIC);
        buf[32..36].copy_from_slice(&self.format_version.to_le_bytes());
        buf[40..48].copy_from_slice(&self.free_list_head.to_le_bytes());
        buf[48..56].copy_from_slice(&self.next_tree_id.to_le_bytes());
        buf[56..64].copy_from_slice(&self.page_count.to_le_bytes());
        page::seal(buf, 0);
    }

    /// Writes the free-list head, next tree identifier and page count into
    /// the header page image `buf`, leaving its page header and the other
    /// fields alone. The image is sealed when written.
    pub(crate) fn write_fields(&self, buf: &mut [u8]) {
        buf[40..48].copy_from_slice(&self.free_list_head.to_le_bytes());
        buf[48..56].copy_from_slice(&self.next_tree_id.to_le_bytes());
        buf[56..64].copy_from_slice(&self.page_count.to_le_bytes());
    }

    /// Reads a verified header page image.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] if the page is not a header page.
    pub(crate) fn decode(buf: &[u8]) -> Result<Header> {
        let u64_at = |at: usize| u64::from_le_bytes(buf[at..at + 8].try_into().expect("8 bytes"));
        if page::page_type(buf) != Some(PageType::Header) || buf[24..32] != MAGIC {
            return Err(Error::Corruption(
                "page 0 of the data file is not a header page".into(),
            ));
        }
        Ok(Header {
            format_version: u32::from_le_bytes(buf[32..36].try_into().expect("4 bytes")),
            free_list_head: u64_at(40),
            next_tree_id: u64_at(48),
            page_count: u64_at(56),
        })
    }
}

/// An open data file.
pub(crate) struct Datafile {
    file: Box<dyn StorageFile>,
    /// Pages reserved: the file length in pages.
    reserved: AtomicU64,
    /// Pages in use: `0..used` (the header's page count).
    used: AtomicU64,
    /// Serialises growth.
    grow: Mutex<()>,
    health: Arc<Health>,
    sink: Arc<dyn MetricsSink>,
}

impl Datafile {
    /// Creates `path` with its first extent reserved, page 0 and page 1
    /// written, and syncs it. Page 1 is the empty root leaf of the tree
    /// directory (TS-05); without it, the first read of page 1 would fail
    /// its checksum.
    ///
    /// # Errors
    ///
    /// [`Error::Io`]; ENOSPC if the first extent does not fit.
    pub(crate) fn create(
        storage: &dyn Storage,
        path: &Path,
        format_version: u32,
        health: Arc<Health>,
        sink: Arc<dyn MetricsSink>,
    ) -> Result<Datafile> {
        let data = Datafile {
            file: storage.create(path)?,
            reserved: AtomicU64::new(0),
            used: AtomicU64::new(0),
            grow: Mutex::new(()),
            health,
            sink,
        };
        data.ensure(2)?;
        let header_page = data.allocate()?;
        let directory_root = data.allocate()?;
        debug_assert_eq!((header_page, directory_root), (0, 1));
        let mut buf = vec![0u8; PAGE_SIZE];
        Header {
            format_version,
            free_list_head: 0,
            next_tree_id: 1,
            page_count: 2,
        }
        .encode(&mut buf);
        data.write_page(0, &buf)?;
        btree::node::init_empty_leaf(&mut buf, 0);
        page::seal(&mut buf, 1);
        data.write_page(1, &buf)?;
        data.sync()?;
        Ok(data)
    }

    /// Opens an existing data file and reads its header page. The caller
    /// checks the header's format version against `CONTROL`.
    ///
    /// # Errors
    ///
    /// As [`Datafile::attach`] and [`Datafile::load_header`].
    pub(crate) fn open(
        file: Box<dyn StorageFile>,
        health: Arc<Health>,
        sink: Arc<dyn MetricsSink>,
    ) -> Result<(Datafile, Header)> {
        let data = Datafile::attach(file, health, sink)?;
        let header = data.load_header()?;
        Ok((data, header))
    }

    /// Opens an existing data file without reading any page: recovery
    /// writes pages into it before its header is read (TS-10, TS-13).
    /// Until [`Datafile::load_header`], no page counts as in use.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] if the file holds no page; [`Error::Io`].
    pub(crate) fn attach(
        file: Box<dyn StorageFile>,
        health: Arc<Health>,
        sink: Arc<dyn MetricsSink>,
    ) -> Result<Datafile> {
        let reserved = file.len()? / PAGE_BYTES;
        if reserved == 0 {
            return Err(Error::Corruption("the data file holds no page".into()));
        }
        Ok(Datafile {
            file,
            reserved: AtomicU64::new(reserved),
            used: AtomicU64::new(0),
            grow: Mutex::new(()),
            health,
            sink,
        })
    }

    /// Reads the header page and takes its page count as the pages in use.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] if page 0 fails its checksum or is not a header
    /// page, or if the page count exceeds the file; [`Error::Io`].
    pub(crate) fn load_header(&self) -> Result<Header> {
        let reserved = self.reserved_pages();
        let mut buf = vec![0u8; PAGE_SIZE];
        self.read_page(0, &mut buf)?;
        let header = Header::decode(&buf)?;
        if header.page_count == 0 || header.page_count > reserved {
            return Err(Error::Corruption(format!(
                "header page count {} outside the file's {reserved} pages",
                header.page_count
            )));
        }
        self.used.store(header.page_count, Ordering::Release);
        self.report();
        Ok(header)
    }

    /// Pages in use or on the free list: the header's page count, kept
    /// here as pages come into use and written to page 0 by each batch.
    #[must_use]
    pub(crate) fn page_count(&self) -> u64 {
        self.used.load(Ordering::Acquire)
    }

    /// Pages reserved, that is the file length in pages.
    #[must_use]
    pub(crate) fn reserved_pages(&self) -> u64 {
        self.reserved.load(Ordering::Acquire)
    }

    /// Pages reserved and not yet in use.
    #[must_use]
    pub(crate) fn unused_pages(&self) -> u64 {
        // `used` never exceeds `reserved`, and `reserved` never decreases.
        let used = self.used.load(Ordering::Acquire);
        self.reserved_pages() - used
    }

    /// Sets the page count back to `pages`, the value before a batch that
    /// is being undone: the pages it took from the reserve return to it
    /// (TS-16 step 6). Called under the commit latch, the only place pages
    /// are taken.
    pub(crate) fn rollback_page_count(&self, pages: u64) {
        debug_assert!(pages <= self.page_count());
        self.used.store(pages, Ordering::Release);
        self.report();
    }

    /// Makes sure at least `pages` reserved pages beyond those in use exist
    /// at the time of the call, reserving whole extents as needed with one
    /// `reserve` call (TS-05).
    ///
    /// # Errors
    ///
    /// ENOSPC ([`std::io::ErrorKind::StorageFull`]) if the storage is full;
    /// nothing changes and the database stays usable (FR-REC-007).
    /// [`std::io::ErrorKind::FileTooLarge`] if the file would exceed
    /// [`MAX_PAGES`]; nothing changes. Any other I/O error sets the unusable
    /// latch.
    pub(crate) fn ensure(&self, pages: u64) -> Result<()> {
        self.health.check()?;
        let _grow = self.grow.lock().unwrap_or_else(PoisonError::into_inner);
        let reserved = self.reserved_pages();
        let needed = self
            .used
            .load(Ordering::Acquire)
            .checked_add(pages)
            .filter(|&n| n <= MAX_PAGES)
            .ok_or_else(|| Error::Io(std::io::ErrorKind::FileTooLarge.into()))?;
        let mut target = reserved;
        while target < needed {
            target = (target + extent_pages(target)).min(MAX_PAGES);
        }
        if target == reserved {
            return Ok(());
        }
        let len = (target - reserved) * PAGE_BYTES;
        if let Err(e) = self.file.reserve(reserved * PAGE_BYTES, len) {
            let e = Error::from(e);
            return Err(if e.is_no_space() {
                e
            } else {
                self.health.fail(e)
            });
        }
        self.reserved.store(target, Ordering::Release);
        self.report();
        Ok(())
    }

    /// Takes the next reserved, unused page and returns its number.
    ///
    /// # Errors
    ///
    /// ENOSPC if no reserved page is left; call [`Datafile::ensure`] first.
    pub(crate) fn allocate(&self) -> Result<u64> {
        self.health.check()?;
        let reserved = self.reserved_pages();
        let mut used = self.used.load(Ordering::Acquire);
        let page = loop {
            if used >= reserved {
                return Err(Error::Io(std::io::ErrorKind::StorageFull.into()));
            }
            match self.used.compare_exchange_weak(
                used,
                used + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(page) => break page,
                Err(now) => used = now,
            }
        };
        self.report();
        Ok(page)
    }

    /// Reads page `page_no` into `buf` and verifies its checksum (TS-12).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a page outside the file. A failed or
    /// short read is [`Error::Io`] or [`Error::Corruption`], and a checksum
    /// failure is [`Error::Corruption`]; all three set the unusable latch.
    pub(crate) fn read_page(&self, page_no: u64, buf: &mut [u8]) -> Result<()> {
        if page_no >= self.reserved_pages() {
            return Err(Error::InvalidArgument("page outside the data file"));
        }
        match self.file.read_at(buf, page_no * PAGE_BYTES) {
            Err(e) => Err(self.health.fail(e.into())),
            Ok(n) if n < PAGE_SIZE => Err(self.health.fail(Error::Corruption(format!(
                "data file ends inside page {page_no}"
            )))),
            Ok(_) if !page::verify(buf, page_no) => Err(self.health.fail(Error::Corruption(
                format!("page {page_no} fails its checksum"),
            ))),
            Ok(_) => Ok(()),
        }
    }

    /// Reads page `page_no` into `buf` and returns whether its checksum
    /// holds, without setting the unusable latch on a mismatch: for the
    /// integrity check, which reports every bad page.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a page outside the file; a failed or
    /// short read, which sets the unusable latch.
    pub(crate) fn read_page_unverified(&self, page_no: u64, buf: &mut [u8]) -> Result<bool> {
        if page_no >= self.reserved_pages() {
            return Err(Error::InvalidArgument("page outside the data file"));
        }
        match self.file.read_at(buf, page_no * PAGE_BYTES) {
            Err(e) => Err(self.health.fail(e.into())),
            Ok(n) if n < PAGE_SIZE => Err(self.health.fail(Error::Corruption(format!(
                "data file ends inside page {page_no}"
            )))),
            Ok(_) => Ok(page::verify(buf, page_no)),
        }
    }

    /// Writes the sealed page image `buf` at page `page_no`. Only reserved
    /// pages are written (TS-05).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a page outside the reserved space;
    /// [`Error::Io`], which sets the unusable latch.
    pub(crate) fn write_page(&self, page_no: u64, buf: &[u8]) -> Result<()> {
        if page_no >= self.reserved_pages() {
            return Err(Error::InvalidArgument(
                "page outside the reserved data file",
            ));
        }
        self.file
            .write_at(buf, page_no * PAGE_BYTES)
            .map_err(|e| self.health.fail(e.into()))
    }

    /// `sync_data` on the file.
    ///
    /// # Errors
    ///
    /// [`Error::Io`], which sets the unusable latch.
    pub(crate) fn sync(&self) -> Result<()> {
        self.file
            .sync_data()
            .map_err(|e| self.health.fail(e.into()))
    }

    /// Reports the store-size gauges.
    fn report(&self) {
        self.sink
            .gauge(DATABASE, self.reserved_pages() * PAGE_BYTES);
        self.sink
            .gauge(RESERVED_UNUSED, self.unused_pages() * PAGE_BYTES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::recording::RecordingSink;
    use crate::storage::FsStorage;
    use crate::storage::fault::{Call, FaultStorage};

    fn create(storage: &dyn Storage, path: &Path) -> (Datafile, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::default());
        let data = Datafile::create(storage, path, 1, Arc::default(), sink.clone()).unwrap();
        (data, sink)
    }

    #[test]
    fn extent_rule() {
        assert_eq!(MIN_EXTENT_PAGES, 8192);
        assert_eq!(extent_pages(0), 8192);
        assert_eq!(extent_pages(819_200), 8192); // 6.25 GiB: 1 % = 64 MiB
        assert_eq!(extent_pages(819_201), 8193);
        assert_eq!(extent_pages(13_107_200), 131_072); // 100 GiB: 1 GiB
    }

    #[test]
    fn create_writes_header_and_reserves_first_extent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data-1");
        let (data, sink) = create(&crate::test_support::NoFlush, &path);
        assert_eq!(data.reserved_pages(), MIN_EXTENT_PAGES);
        assert_eq!(data.unused_pages(), MIN_EXTENT_PAGES - 2);
        let m = sink.snapshot();
        assert_eq!(m.gauge(DATABASE), Some(64 << 20));
        assert_eq!(m.gauge(RESERVED_UNUSED), Some((64 << 20) - 2 * PAGE_BYTES));
        drop(data);
        let (data, header) = Datafile::open(
            crate::test_support::NoFlush.open(&path).unwrap(),
            Arc::default(),
            sink,
        )
        .unwrap();
        assert_eq!(
            header,
            Header {
                format_version: 1,
                free_list_head: 0,
                next_tree_id: 1,
                page_count: 2
            }
        );
        assert_eq!(data.unused_pages(), MIN_EXTENT_PAGES - 2);
        // Page 1, the directory root, passes its checksum (#13 finding).
        let mut buf = vec![0u8; PAGE_SIZE];
        data.read_page(1, &mut buf).unwrap();
        assert_eq!(page::page_type(&buf), Some(PageType::Leaf));
    }

    #[test]
    fn ensure_grows_by_whole_extents_in_one_reserve() {
        let dir = tempfile::tempdir().unwrap();
        let s = FaultStorage::new(Box::new(FsStorage));
        let path = dir.path().join("data-1");
        let (data, _) = create(&s, &path);
        let before = s.calls().len();
        data.ensure(MIN_EXTENT_PAGES - 2).unwrap(); // already reserved
        assert_eq!(s.calls().len(), before);
        data.ensure(2 * MIN_EXTENT_PAGES).unwrap(); // needs two more extents
        let reserves: Vec<_> = s.calls()[before..]
            .iter()
            .filter(|c| matches!(c, Call::Reserve { .. }))
            .cloned()
            .collect();
        assert_eq!(
            reserves,
            [Call::Reserve {
                path: path.clone(),
                offset: MIN_EXTENT_PAGES * PAGE_BYTES,
                len: 2 * MIN_EXTENT_PAGES * PAGE_BYTES,
            }]
        );
        assert_eq!(data.reserved_pages(), 3 * MIN_EXTENT_PAGES);
        for expected in 2..5 {
            assert_eq!(data.allocate().unwrap(), expected);
        }
        assert_eq!(data.unused_pages(), 3 * MIN_EXTENT_PAGES - 5);
    }

    #[test]
    fn ensure_enospc_has_no_side_effects_and_reads_continue() {
        let dir = tempfile::tempdir().unwrap();
        let s = FaultStorage::new(Box::new(FsStorage));
        let path = dir.path().join("data-1");
        let (data, sink) = create(&s, &path);
        s.set_space(0);
        let before = sink.snapshot();
        let err = data.ensure(MIN_EXTENT_PAGES).unwrap_err();
        assert!(err.is_no_space(), "{err}");
        assert_eq!(data.reserved_pages(), MIN_EXTENT_PAGES);
        assert_eq!(s.open(&path).unwrap().len().unwrap(), 64 << 20);
        assert_eq!(sink.snapshot().since(&before).to_string(), "-");
        // Pages already reserved can still be allocated, written and read.
        let p = data.allocate().unwrap();
        let mut buf = vec![0u8; PAGE_SIZE];
        page::init(&mut buf, PageType::Leaf, 3);
        page::seal(&mut buf, p);
        data.write_page(p, &buf).unwrap();
        let mut back = vec![0u8; PAGE_SIZE];
        data.read_page(p, &mut back).unwrap();
        assert_eq!(back, buf);
        data.read_page(0, &mut back).unwrap();
    }

    #[test]
    fn allocate_without_reserve_is_enospc() {
        let dir = tempfile::tempdir().unwrap();
        let (data, _) = create(&crate::test_support::NoFlush, &dir.path().join("data-1"));
        for _ in 2..MIN_EXTENT_PAGES {
            data.allocate().unwrap();
        }
        assert!(data.allocate().unwrap_err().is_no_space());
        assert_eq!(data.unused_pages(), 0);
    }

    #[test]
    fn writes_outside_reserved_space_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (data, _) = create(&crate::test_support::NoFlush, &dir.path().join("data-1"));
        let buf = vec![0u8; PAGE_SIZE];
        assert!(matches!(
            data.write_page(MIN_EXTENT_PAGES, &buf),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn checksum_and_io_failures_set_the_latch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data-1");
        let (data, _) = create(&crate::test_support::NoFlush, &path);
        let mut buf = vec![0u8; PAGE_SIZE];
        // Page 5 is reserved but was never written: zeros fail the checksum.
        assert!(matches!(
            data.read_page(5, &mut buf),
            Err(Error::Corruption(_))
        ));
        assert!(data.health.is_unusable());
        assert!(matches!(data.ensure(1), Err(Error::Unusable)));

        let s = FaultStorage::new(Box::new(FsStorage));
        let (data, _) = create(&s, &dir.path().join("data-2"));
        s.fail_sync_data(2);
        assert!(matches!(data.sync(), Err(Error::Io(_))));
        assert!(data.health.is_unusable());
    }

    /// TS-12 page number binding (#23): a valid image of page A written at
    /// page B's offset, as a misdirected write would, fails the read of B
    /// with corruption and sets the unusable latch.
    #[test]
    fn a_valid_image_of_another_page_fails_the_read_as_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let (data, _) = create(&crate::test_support::NoFlush, &dir.path().join("data-1"));
        let (a, b) = (data.allocate().unwrap(), data.allocate().unwrap());
        let mut image_a = vec![0u8; PAGE_SIZE];
        page::init(&mut image_a, PageType::Leaf, 7);
        page::seal(&mut image_a, a);
        data.write_page(a, &image_a).unwrap();
        let mut back = vec![0u8; PAGE_SIZE];
        data.read_page(a, &mut back).unwrap();
        assert_eq!(back, image_a);
        data.write_page(b, &image_a).unwrap();
        let r = data.read_page(b, &mut back);
        assert!(matches!(r, Err(Error::Corruption(_))), "{r:?}");
        assert!(data.health.is_unusable());
        assert!(matches!(data.ensure(1), Err(Error::Unusable)));
    }

    #[test]
    fn open_rejects_bad_page_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data-1");
        let (data, _) = create(&crate::test_support::NoFlush, &path);
        let mut buf = vec![0u8; PAGE_SIZE];
        Header {
            format_version: 1,
            free_list_head: 0,
            next_tree_id: 1,
            page_count: MIN_EXTENT_PAGES + 1,
        }
        .encode(&mut buf);
        data.write_page(0, &buf).unwrap();
        data.sync().unwrap();
        drop(data);
        let r = Datafile::open(
            FsStorage.open(&path).unwrap(),
            Arc::default(),
            Arc::new(RecordingSink::default()),
        );
        assert!(matches!(r, Err(Error::Corruption(_))));
    }
}
