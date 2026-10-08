use crate::sync::Arc;

use super::*;
use crate::obs::names::checkpointing::CONTROL_SLOT_INVALID;
use crate::obs::names::store_size::{DATABASE, RESERVED_UNUSED};
use crate::obs::recording::RecordingSink;
use crate::storage::FsStorage;
use crate::storage::control::tests::{corrupt_slot, patch_slot};
use crate::storage::datafile::{Header, MIN_EXTENT_PAGES};
use crate::storage::fault::FaultStorage;
use crate::storage::page::PAGE_BYTES;
use crate::test_support::{child, snapshot, test_dirs};

/// Memory cap of these tests: 16 MiB.
const CAP: u64 = 16 << 20;

fn fs() -> Arc<dyn Storage> {
    crate::test_support::no_flush()
}

fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

fn create(dir: &Path) -> Engine {
    Engine::create(dir, fs(), CAP, crate::test_support::OWNER, sink()).unwrap()
}

fn open(dir: &Path) -> Result<Engine> {
    Engine::open(dir, fs(), CAP, sink())
}

fn names(dir: &Path) -> Vec<String> {
    FsStorage
        .list(dir)
        .unwrap()
        .into_iter()
        .map(|n| n.into_string().unwrap())
        .collect()
}

/// Rewrites the header page of `data-1` with `f` applied, sealed.
fn patch_header(dir: &Path, f: impl FnOnce(&mut Header)) {
    let file = FsStorage.open(&dir.join(data_name(1))).unwrap();
    let mut buf = vec![0u8; PAGE_SIZE];
    file.read_at(&mut buf, 0).unwrap();
    let mut h = Header::decode(&buf).unwrap();
    f(&mut h);
    h.encode(&mut buf);
    file.write_at(&buf, 0).unwrap();
    file.sync_data().unwrap();
}

/// Reads page 0 through the pool and checks it is the header page.
fn assert_header_page(e: &Engine) {
    let page = e.pool().fetch(0).unwrap();
    let h = Header::decode(&page.read().unwrap()).unwrap();
    assert_eq!(h.format_version, FORMAT_VERSION);
}

#[test]
fn create_then_open_round_trip() {
    for dir in test_dirs() {
        let d = dir.path();
        let s = sink();
        let e = Engine::create(d, fs(), CAP, crate::test_support::OWNER, s.clone()).unwrap();
        assert_eq!(
            names(d),
            ["CONTROL", "LOCK", "data-1", "wal-1-0000000000000000"]
        );
        let c = *e.control().lock().unwrap().current();
        assert_eq!((c.format_version, c.page_size, c.generation), (1, 8192, 1));
        assert_eq!(c.uuid[6] >> 4, 4, "version 4 UUID");
        assert_eq!(e.data().reserved_pages(), MIN_EXTENT_PAGES);
        let m = s.snapshot();
        assert_eq!(m.gauge(DATABASE), Some(64 << 20));
        // Page 0, the directory and the trees create writes (TS-17).
        let used = 2 + crate::catalog::CREATED_TREES;
        assert_eq!(
            m.gauge(RESERVED_UNUSED),
            Some((64 << 20) - used * PAGE_BYTES)
        );
        drop(e);
        let e = open(d).unwrap();
        assert_eq!(*e.control().lock().unwrap().current(), c);
        assert_eq!(
            e.data().unused_pages(),
            MIN_EXTENT_PAGES - 2 - crate::catalog::CREATED_TREES
        );
        assert_header_page(&e);
    }
}

#[test]
fn create_over_an_existing_database_fails_and_changes_nothing() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = create(d);
        let before = snapshot(d);
        // While the first handle is open, and after it is closed.
        assert!(matches!(
            Engine::create(d, fs(), CAP, crate::test_support::OWNER, sink()),
            Err(Error::DatabaseExists)
        ));
        drop(e);
        assert!(matches!(
            Engine::create(d, fs(), CAP, crate::test_support::OWNER, sink()),
            Err(Error::DatabaseExists)
        ));
        assert_eq!(snapshot(d), before, "every file keeps every byte");
    }
}

#[test]
fn create_replaces_what_an_interrupted_create_left() {
    for dir in test_dirs() {
        let d = dir.path();
        // An interrupted create: LOCK and a data file, no CONTROL.
        FsStorage.create(&d.join(LOCK)).unwrap();
        FsStorage
            .create(&d.join("data-1"))
            .unwrap()
            .write_at(b"junk", 0)
            .unwrap();
        assert!(matches!(open(d), Err(Error::NoDatabase)));
        let e = create(d);
        assert_eq!(
            names(d),
            ["CONTROL", "LOCK", "data-1", "wal-1-0000000000000000"]
        );
        assert_header_page(&e);
    }
}

#[test]
fn open_without_a_database_fails_and_creates_nothing() {
    for dir in test_dirs() {
        let d = dir.path();
        assert!(matches!(open(d), Err(Error::NoDatabase)));
        assert!(names(d).is_empty(), "the directory stays empty");
        let missing = d.join("missing");
        assert!(matches!(open(&missing), Err(Error::NoDatabase)));
        assert!(names(d).is_empty());
    }
}

#[test]
fn memory_cap_too_small_is_refused_before_any_file() {
    for dir in test_dirs() {
        let d = dir.path();
        for cap in [0, PAGE_BYTES] {
            assert!(matches!(
                Engine::create(d, fs(), cap, crate::test_support::OWNER, sink()),
                Err(Error::InvalidArgument(_))
            ));
        }
        assert!(names(d).is_empty());
        drop(create(d));
        let before = snapshot(d);
        assert!(matches!(
            Engine::open(d, fs(), 0, sink()),
            Err(Error::InvalidArgument(_))
        ));
        assert_eq!(snapshot(d), before);
    }
}

#[test]
fn second_open_in_the_same_process_fails_and_the_first_keeps_working() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = create(d);
        assert!(matches!(open(d), Err(Error::Locked)));
        assert!(matches!(open(d), Err(Error::Locked)));
        e.check().unwrap();
        assert_header_page(&e);
        e.data().ensure(MIN_EXTENT_PAGES).unwrap();
        drop(e);
        open(d).unwrap();
    }
}

/// FR-DB-006: an open from another process is refused while this process
/// holds the database, and succeeds once it is closed.
#[test]
fn second_open_from_a_child_process_fails() {
    const DIR: &str = "LPG_TEST_DB_DIR";
    if let Some(role) = child::role() {
        let d = std::env::var_os(DIR).expect("database directory");
        let r = open(Path::new(&d));
        match role.as_str() {
            "expect-locked" => assert!(matches!(r, Err(Error::Locked)), "{:?}", r.err()),
            "expect-open" => drop(r.expect("open after the parent closed")),
            other => panic!("unknown role {other}"),
        }
        return;
    }
    let test = child::test_path(module_path!(), "second_open_from_a_child_process_fails");
    for dir in test_dirs() {
        let d = dir.path();
        let e = create(d);
        let vars = [(DIR, d.as_os_str())];
        let out = child::run(&test, "expect-locked", &vars);
        assert!(out.status.success(), "{out:?}");
        // The first handle keeps working.
        assert_header_page(&e);
        drop(e);
        let out = child::run(&test, "expect-open", &vars);
        assert!(out.status.success(), "{out:?}");
    }
}

#[test]
fn both_control_slots_invalid_is_corruption() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        corrupt_slot(&d.join(CONTROL), 0);
        corrupt_slot(&d.join(CONTROL), 1);
        assert!(matches!(open(d), Err(Error::Corruption(_))));
    }
}

#[test]
fn one_invalid_control_slot_opens_from_the_other_and_is_counted() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        corrupt_slot(&d.join(CONTROL), 1);
        let s = sink();
        let e = Engine::open(d, fs(), CAP, s.clone()).unwrap();
        assert_eq!(s.snapshot().counter(CONTROL_SLOT_INVALID), 1);
        // The next CONTROL write repairs the slot.
        let c = *e.control().lock().unwrap().current();
        e.control().lock().unwrap().write(c).unwrap();
        drop(e);
        let s = sink();
        Engine::open(d, fs(), CAP, s.clone()).unwrap();
        assert_eq!(s.snapshot().counter(CONTROL_SLOT_INVALID), 0);
    }
}

#[test]
fn page_zero_version_differing_from_control_is_corruption() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        patch_header(d, |h| h.format_version = 2);
        let err = open(d).err().unwrap();
        assert!(matches!(err, Error::Corruption(_)), "{err}");
        assert!(err.to_string().contains("format version 2"), "{err}");
    }
}

#[test]
fn page_zero_checksum_failure_is_corruption() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        let file = FsStorage.open(&d.join(data_name(1))).unwrap();
        file.write_at(&[0xFF], 100).unwrap();
        file.sync_data().unwrap();
        assert!(matches!(open(d), Err(Error::Corruption(_))));
    }
}

#[test]
fn newer_format_fails_and_changes_nothing() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        // Leave a file of another generation, which a successful open would
        // remove.
        FsStorage.create(&d.join("data-9")).unwrap();
        for slot in [0, 1] {
            patch_slot(&d.join(CONTROL), slot, |c| {
                c.format_version = FORMAT_VERSION + 1
            });
        }
        let before = snapshot(d);
        let err = open(d).err().unwrap();
        assert!(
            matches!(
                err,
                Error::NewerFormat {
                    found: 2,
                    supported: 1
                }
            ),
            "{err}"
        );
        assert_eq!(snapshot(d), before, "every file keeps every byte");
    }
}

#[test]
fn older_format_requires_migration() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        for slot in [0, 1] {
            patch_slot(&d.join(CONTROL), slot, |c| {
                c.format_version = FORMAT_VERSION - 1
            });
        }
        let before = snapshot(d);
        let err = open(d).err().unwrap();
        assert!(
            matches!(
                err,
                Error::MigrationRequired {
                    found: 0,
                    required: 1
                }
            ),
            "{err}"
        );
        assert!(err.to_string().starts_with("migration required"), "{err}");
        assert_eq!(snapshot(d), before);
    }
}

/// TS-05: open removes `data-<h>` and every `wal-<h>-<lsn>` of every other
/// generation, and keeps the current generation's files and every name that
/// does not match those patterns exactly.
#[test]
fn open_removes_other_generations_data_and_wal_segment_files() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        let removed = [
            "data-2",
            "data-10",
            "wal-2-0000000000000000",
            "wal-2-00000000deadbeef",
            "wal-10-ffffffffffffffff",
        ];
        let kept = [
            // The current generation: the segment after wal-1-0 (each
            // segment holds 0xffff00 LSNs, TS-09), empty.
            "wal-1-0000000000ffff00",
            // Not a file-set name.
            "data-",
            "data-x",
            "data-01",
            "data-+2",
            "wal-1-xyz",
            "wal-2-",
            "wal-2-0000000000000000.tmp",
            "wal-2-000000000000000",
            "wal-2-00000000000000000",
            "wal-2-00000000DEADBEEF",
            "wal-02-0000000000000000",
            "wal--0000000000000000",
            "wal-2",
        ];
        for name in removed.iter().chain(&kept) {
            FsStorage.create(&d.join(name)).unwrap();
        }
        open(d).unwrap();
        let mut expected: Vec<&str> =
            ["CONTROL", "LOCK", "data-1", "wal-1-0000000000000000"].into();
        expected.extend(kept);
        expected.sort_unstable();
        assert_eq!(names(d), expected);
    }
}

/// FR-REC-007: with the device full, extending the data file fails with
/// ENOSPC and leaves the database usable for reads.
#[test]
fn enospc_on_ensure_keeps_reads_working() {
    for dir in test_dirs() {
        let d = dir.path();
        let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
        let e = Engine::create(d, fault.clone(), CAP, crate::test_support::OWNER, sink()).unwrap();
        fault.set_space(0);
        let err = e.data().ensure(MIN_EXTENT_PAGES).err().unwrap();
        assert!(err.is_no_space(), "{err}");
        assert_eq!(e.data().reserved_pages(), MIN_EXTENT_PAGES);
        e.check().unwrap();
        // A miss: page 0 has not been through the pool yet.
        let page = e.pool().fetch(0).unwrap();
        assert_eq!(page.read().unwrap()[24..32], *b"LPGDATA\0");
    }
}

/// TS-27, UC-DB-002 A6: with `CONTROL` present and `LOCK` missing, open
/// fails with [`Error::LockMissing`] and creates nothing.
#[test]
fn open_with_lock_missing_fails_and_creates_nothing() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        FsStorage.remove(&d.join(LOCK)).unwrap();
        let before = snapshot(d);
        let err = open(d).err().unwrap();
        assert!(matches!(err, Error::LockMissing), "{err}");
        assert!(err.to_string().contains("LOCK is missing"), "{err}");
        assert_eq!(snapshot(d), before, "every file keeps every byte");
        assert_eq!(names(d), ["CONTROL", "data-1", "wal-1-0000000000000000"]);
    }
}

/// TS-27: the case behind the rule. Handle A is open and `LOCK` is
/// deleted externally; a second open, in this process or in another, fails
/// with [`Error::LockMissing`] instead of opening a second handle, and A
/// keeps working.
#[test]
fn lock_deleted_under_an_open_handle_refuses_a_second_open() {
    const DIR: &str = "LPG_TEST_DB_DIR";
    if let Some(role) = child::role() {
        assert_eq!(role, "expect-lock-missing");
        let d = std::env::var_os(DIR).expect("database directory");
        let r = open(Path::new(&d));
        assert!(matches!(r, Err(Error::LockMissing)), "{:?}", r.err());
        return;
    }
    let test = child::test_path(
        module_path!(),
        "lock_deleted_under_an_open_handle_refuses_a_second_open",
    );
    for dir in test_dirs() {
        let d = dir.path();
        let a = create(d);
        FsStorage.remove(&d.join(LOCK)).unwrap();
        let before = snapshot(d);
        let r = open(d);
        assert!(matches!(r, Err(Error::LockMissing)), "{:?}", r.err());
        let out = child::run(&test, "expect-lock-missing", &[(DIR, d.as_os_str())]);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(snapshot(d), before, "every file keeps every byte");
        assert_eq!(names(d), ["CONTROL", "data-1", "wal-1-0000000000000000"]);
        a.check().unwrap();
        assert_header_page(&a);
    }
}

/// TS-05, FR-REC-003: with `CONTROL` present and `data-<g>` missing, open
/// fails as corruption and creates nothing.
#[test]
fn open_with_data_file_missing_is_corruption_and_creates_nothing() {
    for dir in test_dirs() {
        let d = dir.path();
        drop(create(d));
        FsStorage.remove(&d.join(data_name(1))).unwrap();
        let before = snapshot(d);
        let err = open(d).err().unwrap();
        assert!(matches!(err, Error::Corruption(_)), "{err}");
        assert!(err.to_string().contains("data-1 is missing"), "{err}");
        assert_eq!(snapshot(d), before, "every file keeps every byte");
        assert_eq!(names(d), ["CONTROL", "LOCK", "wal-1-0000000000000000"]);
    }
}

/// Audit finding 1 (2026-10-03), at create: `CONTROL` is written whole as
/// `CONTROL.tmp`, synced, renamed and the directory synced, so a failure
/// or a crash anywhere in that step leaves either no `CONTROL`, and the
/// location can be created again, or a whole one, and it opens. Each of
/// the step's calls fails once (ENOSPC on the creation and the write, EIO
/// on the others),
/// and the storage stops after each of them, before a power loss in each
/// crash mode. Before the fix, `CONTROL` was created in place, and an
/// ENOSPC on its write left an empty `CONTROL`: create refused the
/// location and open failed on it.
#[test]
fn audit_a_failed_or_crashed_control_write_at_create_leaves_a_usable_location() {
    use crate::storage::fault::{Call, Crash, Fault, Kind};
    let is_control = |p: &Path| {
        p.file_name()
            .is_some_and(|n| n == CONTROL || n == CONTROL_TMP)
    };
    let fault = || {
        let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
        f.skip_device_sync();
        f
    };
    let dir = crate::test_support::graph_dir();
    // Dry run: the calls of the `CONTROL` step.
    let dry = tempfile::tempdir_in(dir.path()).unwrap();
    let f = fault();
    drop(
        Engine::create(
            dry.path(),
            f.clone(),
            CAP,
            crate::test_support::OWNER,
            sink(),
        )
        .unwrap(),
    );
    let calls = f.calls();
    let kind = |c: &Call| match c {
        Call::WriteAt { .. } => Some(Kind::WriteAt),
        Call::SyncData(_) => Some(Kind::SyncData),
        Call::SyncDir(_) => Some(Kind::SyncDir),
        Call::Rename(..) => Some(Kind::Rename),
        Call::Create(_) => Some(Kind::Create),
        Call::Remove(_) => Some(Kind::Remove),
        _ => None,
    };
    let first = calls
        .iter()
        .position(|c| matches!(c, Call::Create(path) if is_control(path)))
        .expect("create creates CONTROL");
    // The step ends with the `sync_dir` after the write of `CONTROL`.
    let last = first
        + calls[first..]
            .iter()
            .position(|c| matches!(c, Call::SyncDir(_)))
            .expect("create syncs the directory after CONTROL");
    let points_before = calls[..first].iter().filter(|c| kind(c).is_some()).count() as u64;
    let mut runs = 0;
    for (i, c) in calls.iter().enumerate().take(last + 1).skip(first) {
        let Some(k) = kind(c) else { continue };
        let nth = calls[..=i].iter().filter(|x| kind(x) == Some(k)).count() as u64;
        let fail = if k == Kind::WriteAt {
            Fault::NoSpace
        } else {
            Fault::Eio
        };
        let point = points_before
            + calls[first..=i]
                .iter()
                .filter(|c| kind(c).is_some())
                .count() as u64;
        let arms = [
            ("fail", Crash::DropAll, true),
            ("crash drop-all", Crash::DropAll, false),
            ("crash keep-random", Crash::KeepRandom { seed: nth }, false),
            ("crash tear", Crash::Tear { seed: nth }, false),
        ];
        for (what, mode, fails) in arms {
            let what = format!("{what} at {c:?}");
            let sub = tempfile::tempdir_in(dir.path()).unwrap();
            let d = sub.path();
            let f = fault();
            if fails {
                f.fail_nth(k, nth, fail);
            } else {
                f.crash_after(point);
            }
            let r = Engine::create(d, f.clone(), CAP, crate::test_support::OWNER, sink());
            if fails {
                assert!(r.is_err(), "{what}: precondition: create failed");
            }
            drop(r);
            f.crash(mode).unwrap();
            // The location is now a whole database or none.
            let e = match Engine::open(d, fs(), CAP, sink()) {
                Ok(e) => e,
                Err(Error::NoDatabase) => {
                    Engine::create(d, fs(), CAP, crate::test_support::OWNER, sink())
                        .unwrap_or_else(|e| panic!("{what}: create again: {e}"))
                }
                Err(e) => panic!("{what}: the location is stuck: {e}"),
            };
            e.create_trees(1).unwrap();
            e.check().unwrap();
            runs += 1;
        }
    }
    assert!(runs >= 20, "the CONTROL step has 5 calls, each with 4 arms");
}
