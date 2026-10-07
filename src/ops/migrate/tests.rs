use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::obs::recording::RecordingSink;
use crate::ops::backup::tests::entries;
use crate::storage::btree::{Tree, Write};
use crate::storage::control::{SLOT_SIZE, decode, encode};
use crate::storage::engine::Engine;
use crate::storage::fault::{Call, FaultStorage};
use crate::storage::page;
use crate::storage::{FsStorage, Storage};
use crate::test_support::{no_flush, snapshot, test_dirs};
use crate::tx::commit::Ids;

const CAP: u64 = 32 << 20;

/// The test-only format 0 (TS-28, rmp #19): format 1 less the page number
/// binding of task #23 (TS-12), the smallest page-format difference. Its
/// page checksum is the CRC-32C of bytes 4 to 8 191 alone.
pub(crate) fn verify0(p: &[u8], _page_no: u64) -> bool {
    u32::from_le_bytes(p[0..4].try_into().unwrap()) == crc32c::crc32c(&p[4..])
}

/// Format 0 to 1 needs no change of the page body: the caller seals each
/// page with the format-1 checksum.
pub(crate) fn convert0(_p: &mut [u8], _page_no: u64) {}

/// The formats of a test build: format 0 and its step to format 1.
pub(crate) const TEST_FORMATS: Formats = Formats {
    current: FORMAT_VERSION,
    steps: &[Step {
        from: 0,
        verify: verify0,
        convert: convert0,
    }],
    crate_versions: &[
        (0, "0.0.9-test"),
        (FORMAT_VERSION, env!("CARGO_PKG_VERSION")),
    ],
};

/// Rewrites the cleanly closed format-1 database at `dir` in format 0:
/// every page below the page count gets the format-0 checksum, page 0 and
/// both `CONTROL` slots format version 0. Each page must pass its format-1
/// checksum first, so the database was intact.
pub(crate) fn downgrade_to_format_0(dir: &Path) {
    let control = FsStorage.open(&dir.join(CONTROL)).unwrap();
    let mut both = vec![0u8; 2 * SLOT_SIZE];
    control.read_at(&mut both, 0).unwrap();
    let g = (0..2)
        .filter_map(|i| decode(&both[i * SLOT_SIZE..(i + 1) * SLOT_SIZE]))
        .max_by_key(|&(_, seq)| seq)
        .unwrap()
        .0
        .generation;
    let data = FsStorage.open(&dir.join(data_name(g))).unwrap();
    let mut p0 = vec![0u8; PAGE_SIZE];
    data.read_at(&mut p0, 0).unwrap();
    let pages = Header::decode(&p0).unwrap().page_count;
    let mut buf = vec![0u8; PAGE_SIZE];
    for p in 0..pages {
        data.read_at(&mut buf, p * PAGE_BYTES).unwrap();
        assert!(page::verify(&buf, p), "page {p} is not intact");
        if p == 0 {
            buf[32..36].copy_from_slice(&0u32.to_le_bytes());
        }
        let crc = crc32c::crc32c(&buf[4..]);
        buf[0..4].copy_from_slice(&crc.to_le_bytes());
        data.write_at(&buf, p * PAGE_BYTES).unwrap();
    }
    data.sync_data().unwrap();
    for i in 0..2 {
        let slot = &mut both[i * SLOT_SIZE..(i + 1) * SLOT_SIZE];
        if let Some((mut c, seq)) = decode(slot) {
            c.format_version = 0;
            slot.copy_from_slice(&encode(&c, seq));
        }
    }
    control.write_at(&both, 0).unwrap();
    control.sync_data().unwrap();
}

fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

/// A cleanly closed format-1 database of one tree with 2 000 entries over
/// several pages, its tree, entries and authoritative `CONTROL` record.
pub(crate) fn closed_database(
    dir: &Path,
    storage: Arc<dyn Storage>,
) -> (Tree, BTreeMap<Vec<u8>, Vec<u8>>, Control) {
    let e = Engine::create(dir, storage, CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let keys: Vec<[u8; 4]> = (0..2_000u32).map(u32::to_be_bytes).collect();
    for (n, chunk) in keys.chunks(400).enumerate() {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(&[3; 200]),
            })
            .collect();
        e.commit(
            &batch,
            Ids {
                next_element_id: 10 * n as u64 + 7,
                next_catalog_id: 3,
                next_token_id: 4,
            },
        )
        .unwrap();
    }
    let m = entries(&e, tree);
    e.close().unwrap();
    let c = read_control(dir);
    (tree, m, c)
}

fn read_control(dir: &Path) -> Control {
    let f = FsStorage.open(&dir.join(CONTROL)).unwrap();
    *ControlSlots::open(f, Arc::default(), &NullSink)
        .unwrap()
        .current()
}

/// rmp #19: a cleanly closed N-1 database migrates and then opens at the
/// same location in format N, with the same counters and tree contents;
/// the old generation's files are gone.
#[test]
fn a_cleanly_closed_older_database_migrates_and_opens_unchanged() {
    let dir = test_dirs().remove(0);
    let (tree, m, before) = closed_database(dir.path(), no_flush());
    downgrade_to_format_0(dir.path());
    let r = Engine::open(dir.path(), no_flush(), CAP, sink());
    assert!(
        matches!(r, Err(Error::MigrationRequired { found: 0, .. })),
        "{:?}",
        r.err()
    );
    let done = migrate_with(dir.path(), &FsStorage, &TEST_FORMATS)
        .unwrap()
        .unwrap();
    assert_eq!((done.from, done.generation), (0, 2));
    let after = read_control(dir.path());
    assert_eq!(
        after,
        Control {
            format_version: FORMAT_VERSION,
            generation: 2,
            ..before
        }
    );
    let names = FsStorage.list(dir.path()).unwrap();
    assert!(!names.iter().any(|n| n == "data-1"), "{names:?}");
    let e = Engine::open(dir.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&e, tree) == m);
    let c = e.store().lock().log_ref().counters;
    assert_eq!(
        (
            c.commit_ts,
            c.next_element_id,
            c.next_catalog_id,
            c.next_token_id
        ),
        (
            before.commit_ts,
            before.next_element_id,
            before.next_catalog_id,
            before.next_token_id
        )
    );
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    e.close().unwrap();
    // Already current: nothing to do, also for this crate's own formats.
    assert!(
        migrate_with(dir.path(), &FsStorage, &TEST_FORMATS)
            .unwrap()
            .is_none()
    );
    assert!(migrate(dir.path(), &FsStorage).unwrap().is_none());
}

/// rmp #19, TS-28 step 3: with free space below what the migration writes,
/// it fails before any write, and every byte of every file is unchanged.
#[test]
fn too_little_space_fails_before_any_write() {
    let dir = test_dirs().remove(0);
    closed_database(dir.path(), no_flush());
    downgrade_to_format_0(dir.path());
    let before = snapshot(dir.path());
    let fault = FaultStorage::new(Box::new(FsStorage));
    fault.set_space(64 << 20);
    let r = migrate_with(dir.path(), &fault, &TEST_FORMATS);
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{:?}", r.err());
    let writes = fault.calls().into_iter().filter(|c| {
        matches!(
            c,
            Call::WriteAt { .. }
                | Call::Create(_)
                | Call::Remove(_)
                | Call::Rename(..)
                | Call::Reserve { .. }
        )
    });
    assert_eq!(writes.count(), 0);
    assert!(snapshot(dir.path()) == before);
}

/// rmp #19, TS-28 step 1: an open database is not migrated.
#[test]
fn an_open_database_is_not_migrated() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(dir.path(), no_flush(), CAP, sink()).unwrap();
    let r = migrate_with(dir.path(), &FsStorage, &TEST_FORMATS);
    assert!(matches!(r, Err(Error::Locked)), "{:?}", r.err());
    drop(e);
}

/// rmp #19, TS-28 step 2: a database without a clean shutdown is not
/// migrated, and the message names the crate version that wrote it.
#[test]
fn a_database_without_a_clean_shutdown_names_the_previous_crate_version() {
    let dir = test_dirs().remove(0);
    closed_database(dir.path(), no_flush());
    downgrade_to_format_0(dir.path());
    crate::storage::control::tests::patch_slot(&dir.path().join(CONTROL), 0, |c| {
        c.clean_shutdown = false;
    });
    crate::storage::control::tests::patch_slot(&dir.path().join(CONTROL), 1, |c| {
        c.clean_shutdown = false;
    });
    let before = snapshot(dir.path());
    let r = migrate_with(dir.path(), &FsStorage, &TEST_FORMATS);
    let e = r.expect_err("not migrated");
    assert!(matches!(e, Error::NotCleanlyClosed { .. }), "{e:?}");
    assert!(e.to_string().contains("0.0.9-test"), "{e}");
    assert!(snapshot(dir.path()) == before);
}

/// rmp #19, FR-OPS-012: a format two versions older names the intermediate
/// crate version; a newer format is refused (FR-OPS-013). The formats of a
/// crate whose format is 2 are simulated: it migrates from 1 only.
#[test]
fn a_format_two_versions_older_names_the_intermediate_crate_version() {
    const FUTURE: Formats = Formats {
        current: 2,
        steps: &[Step {
            from: 1,
            verify: page::verify,
            convert: convert0,
        }],
        crate_versions: &[(0, "0.0.9-test"), (1, "0.1.0"), (2, "0.2.0")],
    };
    let dir = test_dirs().remove(0);
    closed_database(dir.path(), no_flush());
    downgrade_to_format_0(dir.path());
    let before = snapshot(dir.path());
    let e = migrate_with(dir.path(), &FsStorage, &FUTURE).expect_err("refused");
    assert!(
        matches!(
            e,
            Error::MigrationPath {
                found: 0,
                crate_version: Some("0.1.0")
            }
        ),
        "{e:?}"
    );
    assert!(e.to_string().contains("0.1.0"), "{e}");
    let past = Formats {
        current: 0,
        steps: &[],
        crate_versions: &[],
    };
    let dir2 = test_dirs().remove(0);
    closed_database(dir2.path(), no_flush());
    let r = migrate_with(dir2.path(), &FsStorage, &past);
    assert!(
        matches!(
            r,
            Err(Error::NewerFormat {
                found: 1,
                supported: 0
            })
        ),
        "{:?}",
        r.err()
    );
    assert!(snapshot(dir.path()) == before);
}

/// [`crate::test_support::NoFlush`] on a device of `cap` bytes:
/// `available_space` is `cap` less the lengths of the files in the
/// directory, so removing a file gives its space back, as on a file system.
struct Device {
    cap: u64,
}

impl Device {
    fn used(dir: &Path) -> u64 {
        FsStorage
            .list(dir)
            .unwrap()
            .iter()
            .map(|n| FsStorage.open(&dir.join(n)).unwrap().len().unwrap())
            .sum()
    }
}

impl Storage for Device {
    fn open(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        crate::test_support::NoFlush.open(path)
    }
    fn create(&self, path: &Path) -> io::Result<Box<dyn StorageFile>> {
        crate::test_support::NoFlush.create(path)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        crate::test_support::NoFlush.remove(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        crate::test_support::NoFlush.rename(from, to)
    }
    fn list(&self, dir: &Path) -> io::Result<Vec<std::ffi::OsString>> {
        crate::test_support::NoFlush.list(dir)
    }
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        crate::test_support::NoFlush.sync_dir(dir)
    }
    fn try_lock(&self, path: &Path) -> io::Result<Box<dyn crate::storage::StorageLock>> {
        crate::test_support::NoFlush.try_lock(path)
    }
    fn available_space(&self, dir: &Path) -> io::Result<u64> {
        Ok(self.cap.saturating_sub(Device::used(dir)))
    }
}

/// Audit finding 10 (2026-10-03), TS-28 step 3: the files an interrupted
/// migration left (here a full-size `data-2`) are removed before the space
/// check, so their space counts as available. On a device that holds the
/// database, the leftover and exactly what the migration writes, but not
/// the leftover and that, the migration succeeds; checked before the
/// removal, it would fail with ENOSPC.
#[test]
fn audit_the_leftovers_of_an_interrupted_migration_are_removed_before_the_space_check() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let (tree, m, _) = closed_database(d, no_flush());
    downgrade_to_format_0(d);
    let without = Device::used(d);
    let data_len = FsStorage
        .open(&d.join(data_name(1)))
        .unwrap()
        .len()
        .unwrap();
    let wal: u64 = FsStorage
        .list(d)
        .unwrap()
        .iter()
        .filter(|n| parse_segment_name(n).is_some())
        .map(|n| FsStorage.open(&d.join(n)).unwrap().len().unwrap())
        .sum();
    // What an interrupted migration leaves: `data-2` at full length
    // (sparse, so it takes no space on the drive).
    FsStorage
        .create(&d.join(data_name(2)))
        .unwrap()
        .set_len(data_len)
        .unwrap();
    let device = Device {
        cap: without + data_len + wal,
    };
    assert!(
        device.available_space(d).unwrap() < data_len + wal,
        "precondition: with the leftover counted, the space check fails"
    );
    migrate_with(d, &device, &TEST_FORMATS)
        .unwrap()
        .expect("migrated");
    let e = Engine::open(d, no_flush(), CAP, sink()).unwrap();
    assert!(entries(&e, tree) == m);
}
