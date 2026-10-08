use crate::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::{Arc, Mutex};
use std::collections::BTreeMap;

use super::*;
use crate::obs::recording::RecordingSink;
use crate::ops::restore::restore;
use crate::storage::FsStorage;
use crate::storage::btree::{Tree, Write};
use crate::storage::wal::{SEGMENT_SPAN, parse_segment_name};
use crate::test_support::{no_flush, snapshot, test_dirs, wait_until};
use crate::tx::commit::Ids;

const CAP: u64 = 32 << 20;

fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

pub(crate) type Model = BTreeMap<Vec<u8>, Vec<u8>>;

pub(crate) fn entries(e: &Engine, tree: Tree) -> Model {
    let mut out = Model::new();
    e.store()
        .scan_prefix(tree, b"", |k, v| {
            out.insert(k.to_vec(), v.to_vec());
            true
        })
        .unwrap();
    out
}

fn put<'a>(tree: Tree, k: &'a [u8], v: &'a [u8]) -> Write<'a> {
    Write {
        tree,
        key: k,
        value: Some(v),
        stamp: 0,
    }
}

/// One commit of the concurrent workload: its timestamp, its keys and the
/// element identifier high-water mark it raised.
struct Done {
    ts: u64,
    keys: Vec<Vec<u8>>,
    next_element_id: u64,
}

/// Sets its flag when dropped.
struct StopOnDrop<'a>(&'a AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// rmp #18: a backup taken while four threads commit restores to the state
/// after exactly one commit `k`; that state holds every commit that
/// returned before the backup started; the restored database passes the
/// integrity check and has the counters of the source at `k`; and after
/// the source is removed, the restored database still opens with the data.
#[test]
fn a_backup_under_four_committers_restores_to_one_commit_point() {
    let dir = test_dirs().remove(0);
    let (src, target, restored) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let e = Engine::create(
        src.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let done = Mutex::new(Vec::<Done>::new());
    let stop = AtomicBool::new(false);
    let before_backup = AtomicU64::new(0);
    let stats = std::thread::scope(|s| {
        for t in 0..4u64 {
            let (e, done, stop) = (&e, &done, &stop);
            s.spawn(move || {
                let mut i = 0u64;
                while !stop.load(Ordering::Acquire) {
                    // Two keys per commit, several leaves apart.
                    let keys = vec![
                        format!("a{t}-{i:06}").into_bytes(),
                        format!("z{t}-{i:06}").into_bytes(),
                    ];
                    let id = t * 1_000_000 + i;
                    let batch: Vec<Write<'_>> =
                        keys.iter().map(|k| put(tree, k, &[7; 300])).collect();
                    let c = e
                        .commit(
                            &batch,
                            Ids {
                                next_element_id: id,
                                ..Ids::default()
                            },
                        )
                        .unwrap();
                    done.lock().unwrap().push(Done {
                        ts: c.commit_ts,
                        keys,
                        next_element_id: id,
                    });
                    i += 1;
                }
            });
        }
        // Ends the committers when the body returns or panics, so a failed
        // wait below fails the test instead of hanging the scope's join.
        let _stop = StopOnDrop(&stop);
        // Let some commits return, then back up while the rest go on.
        wait_until("200 commits return", || done.lock().unwrap().len() >= 200);
        let max_before = done.lock().unwrap().iter().map(|d| d.ts).max().unwrap();
        before_backup.store(max_before, Ordering::Release);
        let stats = e.backup(target.path()).unwrap();
        // Commits returned after the backup ended, too.
        let n = done.lock().unwrap().len();
        wait_until("100 commits return after the backup", || {
            done.lock().unwrap().len() >= n + 100
        });
        stats
    });
    assert!(stats.manifest.stop > stats.manifest.start);
    let done = done.into_inner().unwrap();
    let r = restore(target.path(), restored.path(), no_flush(), CAP, sink()).unwrap();
    let got = entries(&r, tree);
    // The commit point k: the highest timestamp whose keys are present.
    let ts_of: BTreeMap<&[u8], u64> = done
        .iter()
        .flat_map(|d| d.keys.iter().map(move |k| (k.as_slice(), d.ts)))
        .collect();
    let k = got.keys().map(|key| ts_of[key.as_slice()]).max().unwrap();
    let expected: Model = done
        .iter()
        .filter(|d| d.ts <= k)
        .flat_map(|d| d.keys.iter().map(|key| (key.clone(), vec![7; 300])))
        .collect();
    assert!(
        got == expected,
        "the restored state is not the state after commit {k}"
    );
    assert!(
        k >= before_backup.load(Ordering::Acquire),
        "a commit that returned before the backup started is missing"
    );
    let check = r.store().check().unwrap();
    assert!(check.is_ok(), "{:?}", check.problems);
    let counters = r.store().lock().log_ref().counters;
    assert_eq!(counters.commit_ts, k);
    let max_id = done
        .iter()
        .filter(|d| d.ts <= k)
        .map(|d| d.next_element_id)
        .max()
        .unwrap();
    assert_eq!(counters.next_element_id, max_id);
    e.close().unwrap();
    drop(src);
    r.close().unwrap();
    let r = Engine::open(restored.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&r, tree) == expected);
}

/// TS-26: a backup into a target that holds any entry fails before
/// writing anything.
#[test]
fn the_backup_target_must_be_empty() {
    let dir = test_dirs().remove(0);
    let (src, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let e = Engine::create(
        src.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    FsStorage.create(&target.path().join("x")).unwrap();
    let before = snapshot(target.path());
    assert!(matches!(
        e.backup(target.path()),
        Err(Error::InvalidArgument(_))
    ));
    assert!(snapshot(target.path()) == before);
}

/// TS-26 step 1: while a backup holds recycling at its start, checkpoints
/// keep every segment from there on; once it is released, they recycle
/// again.
#[test]
fn the_backup_hold_keeps_segments_from_its_start() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(
        dir.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    // 2 400 keys of 300 bytes: about 96 leaves.
    let keys: Vec<Vec<u8>> = (0..2_400u32).map(|i| i.to_be_bytes().to_vec()).collect();
    let load: Vec<Write<'_>> = keys.iter().map(|k| put(tree, k, &[0; 300])).collect();
    e.commit(&load, Ids::default()).unwrap();
    let start = checkpoint::run_held(e.core()).unwrap().redo;
    // 25 commits of about 96 one-leaf pages: more than one segment of WAL.
    for round in 0..25u8 {
        let v = [round; 300];
        let batch: Vec<Write<'_>> = keys.iter().step_by(20).map(|k| put(tree, k, &v)).collect();
        e.commit(&batch, Ids::default()).unwrap();
    }
    assert!(e.wal().durable_lsn() >= start + SEGMENT_SPAN);
    let held = e.checkpoint().unwrap();
    assert_eq!(
        held.recycled + held.removed,
        0,
        "a held segment was recycled"
    );
    let holder = || {
        FsStorage
            .list(dir.path())
            .unwrap()
            .iter()
            .filter_map(|n| parse_segment_name(n))
            .any(|(_, f)| f <= start && start < f + SEGMENT_SPAN)
    };
    assert!(holder());
    e.wal().release(start);
    let freed = e.checkpoint().unwrap();
    assert!(freed.recycled + freed.removed > 0);
    assert!(!holder());
}

/// `MANIFEST` round-trips, and any changed byte or a truncation is
/// refused.
#[test]
fn the_manifest_round_trips_and_any_change_is_refused() {
    let m = Manifest {
        format_version: 1,
        uuid: [3; 16],
        generation: 2,
        page_size: 8192,
        start: 10,
        stop: 20,
        data_len: 1 << 26,
        files: vec![
            FileEntry {
                name: "CONTROL".into(),
                len: 8192,
                crc: 5,
            },
            FileEntry {
                name: "wal-2-000000000000000a".into(),
                len: 10,
                crc: 6,
            },
        ],
    };
    let b = m.encode();
    assert_eq!(Manifest::decode(&b), Some(m));
    for i in 0..b.len() {
        let mut c = b.clone();
        c[i] ^= 0x04;
        assert_eq!(Manifest::decode(&c), None, "byte {i}");
    }
    assert_eq!(Manifest::decode(&b[..b.len() - 1]), None);
    assert_eq!(Manifest::decode(&[]), None);
}

/// Entries already in key order, for a bulk build.
struct Sorted(
    std::vec::IntoIter<(Vec<u8>, Vec<u8>)>,
    Option<(Vec<u8>, Vec<u8>)>,
);

impl crate::storage::btree::bulk::SortedEntries for Sorted {
    fn next_entry(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        self.1 = self.0.next();
        Ok(self.1.as_ref().map(|(k, v)| (k.as_slice(), v.as_slice())))
    }
}

/// Audit finding 9 (2026-10-03), TS-26 "a load requested while a backup
/// runs waits": the pages of a bulk build bypass the WAL, so a backup and a
/// build exclude each other. A build started while a backup is paused at
/// its pause point `backup_started` waits for it (the latch counts the
/// wait) and finishes only once the backup has; the backup restores to the
/// state before the build, and the source holds the built tree.
#[test]
fn audit_a_bulk_build_waits_for_a_running_backup() {
    use crate::sync::mpsc;
    use std::time::Duration;
    let dir = test_dirs().remove(0);
    let (src, target, restored) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let e = Engine::create(
        src.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    e.commit(&[put(tree, b"a", b"1")], Ids::default()).unwrap();
    let before = entries(&e, tree);
    let (started_tx, started) = mpsc::channel();
    let (go_tx, go) = mpsc::channel::<()>();
    {
        let (started_tx, go) = (Mutex::new(started_tx), Mutex::new(go));
        *e.wal().points.backup_started.lock().unwrap() = Some(Box::new(move || {
            started_tx.lock().unwrap().send(()).unwrap();
            // An error means the test is unwinding: go on.
            let _ = go.lock().unwrap().recv();
        }));
    }
    let built = AtomicBool::new(false);
    let entries_of_build: Vec<(Vec<u8>, Vec<u8>)> = (0..2_000u32)
        .map(|i| (i.to_be_bytes().to_vec(), vec![7; 40]))
        .collect();
    /// Releases the paused backup when dropped, also when an assertion
    /// below fails: otherwise the scope would wait for it forever.
    struct Release(mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let new_tree = std::thread::scope(|s| {
        let release = Release(go_tx);
        let backup = s.spawn(|| e.backup(target.path()).unwrap());
        started.recv().unwrap();
        let build = s.spawn(|| {
            let t = e
                .bulk_build(&mut Sorted(entries_of_build.clone().into_iter(), None))
                .unwrap();
            built.store(true, Ordering::SeqCst);
            t
        });
        // The build reaches the latch and waits there.
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while e.core().load_waits.load(Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the build did not wait for the running backup"
            );
            std::thread::yield_now();
        }
        assert!(
            !built.load(Ordering::SeqCst),
            "the build ran during the backup"
        );
        drop(release);
        backup.join().unwrap();
        build.join().unwrap()
    });
    *e.wal().points.backup_started.lock().unwrap() = None;
    let want: Model = entries_of_build.into_iter().collect();
    assert!(entries(&e, new_tree) == want);
    e.close().unwrap();
    let r = restore(target.path(), restored.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&r, tree) == before);
    assert!(r.store().tree(new_tree.id()).unwrap().is_none());
    let c = r.store().check().unwrap();
    assert!(c.is_ok(), "{:?}", c.problems);
}

/// FR-OPS-001, FR-IMP-002: a backup requested while a bulk build runs fails
/// with [`Error::LoadRunning`] at once, without waiting for the build. The
/// build is held between its page writes and its install, with the
/// backup-or-load latch held ([`crate::storage::btree::bulk::Built`]). The
/// failed backup leaves the target empty and the database usable and
/// unchanged; a backup after the build ends succeeds and restores both
/// trees.
#[test]
fn a_backup_requested_during_a_bulk_build_fails_at_once() {
    use crate::sync::mpsc;
    use std::time::Duration;
    let dir = test_dirs().remove(0);
    let (src, target, restored) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let e = Engine::create(
        src.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    e.commit(&[put(tree, b"a", b"1")], Ids::default()).unwrap();
    let before = entries(&e, tree);
    let entries_of_build: Vec<(Vec<u8>, Vec<u8>)> = (0..2_000u32)
        .map(|i| (i.to_be_bytes().to_vec(), vec![7; 40]))
        .collect();
    let new_tree = std::thread::scope(|s| {
        // Declared in the scope's closure, so that a failed assertion drops
        // it, releasing the latch, before the scope joins the backup.
        let built = e
            .bulk_build_pages(&mut Sorted(entries_of_build.clone().into_iter(), None))
            .unwrap();
        let (tx, rx) = mpsc::channel();
        let (e, to) = (&e, target.path());
        s.spawn(move || {
            let _ = tx.send(e.backup(to));
        });
        // Returns at once when the backup fails; the bound only stops a
        // backup that waits from hanging the test.
        let r = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the backup waited for the running bulk build");
        assert!(matches!(r, Err(Error::LoadRunning)), "{r:?}");
        assert_eq!(e.core().load_waits.load(Ordering::SeqCst), 0);
        assert!(FsStorage.list(target.path()).unwrap().is_empty());
        assert!(!e.core().health.is_unusable());
        built.install().unwrap()
    });
    // The database is unchanged by the failed backup and usable.
    assert!(entries(&e, tree) == before);
    let want: Model = entries_of_build.into_iter().collect();
    assert!(entries(&e, new_tree) == want);
    e.commit(&[put(tree, b"b", b"2")], Ids::default()).unwrap();
    let after = entries(&e, tree);
    // Once the build has ended, a backup succeeds and restores.
    e.backup(target.path()).unwrap();
    e.close().unwrap();
    let r = restore(target.path(), restored.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&r, tree) == after);
    assert!(entries(&r, new_tree) == want);
    let c = r.store().check().unwrap();
    assert!(c.is_ok(), "{:?}", c.problems);
}
