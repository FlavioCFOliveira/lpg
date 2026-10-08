use crate::sync::Arc;

use super::*;
use crate::obs::recording::RecordingSink;
use crate::ops::backup::MANIFEST;
use crate::ops::backup::tests::{Model, entries};
use crate::storage::FsStorage;
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::CONTROL_TMP;
use crate::test_support::{no_flush, snapshot, test_dirs};
use crate::tx::commit::Ids;

const CAP: u64 = 32 << 20;

fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

/// A database in `dir` with one tree of 300 entries, closed, and a backup
/// of it in `artefact`. Returns the tree and its entries.
fn backed_up(dir: &Path, artefact: &Path) -> (Tree, Model) {
    let e = Engine::create(dir, no_flush(), CAP, crate::test_support::OWNER, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    for i in 0..300u32 {
        let k = i.to_be_bytes();
        e.commit(
            &[Write {
                tree,
                key: &k,
                value: Some(&[5; 100]),
                stamp: 0,
            }],
            Ids::default(),
        )
        .unwrap();
    }
    e.backup(artefact).unwrap();
    let m = entries(&e, tree);
    e.close().unwrap();
    (tree, m)
}

/// rmp #18, FR-OPS-009: a restore into a location that holds a database
/// fails, and every byte of that database's files is unchanged.
#[test]
fn a_restore_into_a_database_fails_and_changes_nothing() {
    let dir = test_dirs().remove(0);
    let (src, artefact, other) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    backed_up(src.path(), artefact.path());
    let e = Engine::create(
        other.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    e.close().unwrap();
    let before = snapshot(other.path());
    let r = restore(artefact.path(), other.path(), no_flush(), CAP, sink());
    assert!(matches!(r, Err(Error::DatabaseExists)), "{:?}", r.err());
    assert!(snapshot(other.path()) == before);
}

/// TS-26 steps 2 and 3: an artefact without `MANIFEST`, with a damaged
/// `MANIFEST` or with a damaged file is refused before the target is
/// written.
#[test]
fn an_incomplete_or_damaged_artefact_is_refused_before_any_write() {
    let dir = test_dirs().remove(0);
    let (src, artefact, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    backed_up(src.path(), artefact.path());
    let flip = |name: &str, at: u64| {
        let f = FsStorage.open(&artefact.path().join(name)).unwrap();
        let mut b = [0u8; 1];
        f.read_at(&mut b, at).unwrap();
        f.write_at(&[b[0] ^ 1], at).unwrap();
    };
    let data = data_name(1);
    flip(&data, 9000);
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(matches!(r, Err(Error::Corruption(_))), "{:?}", r.err());
    flip(&data, 9000);
    flip(MANIFEST, 20);
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(
        matches!(r, Err(Error::IncompleteBackup(_))),
        "{:?}",
        r.err()
    );
    flip(MANIFEST, 20);
    FsStorage.remove(&artefact.path().join(MANIFEST)).unwrap();
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(
        matches!(r, Err(Error::IncompleteBackup(_))),
        "{:?}",
        r.err()
    );
    assert!(FsStorage.list(target.path()).unwrap().is_empty());
}

/// TS-26: a restore interrupted before `CONTROL` leaves files, which the
/// next restore overwrites, stale segments included; the result is the
/// backed-up database.
#[test]
fn a_restore_overwrites_what_an_interrupted_restore_left() {
    let dir = test_dirs().remove(0);
    let (src, artefact, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let (tree, m) = backed_up(src.path(), artefact.path());
    for name in [LOCK, "data-1", "wal-1-00000000f0000000", "wal-temp"] {
        let f = FsStorage.create(&target.path().join(name)).unwrap();
        f.write_at(&[9; 5000], 0).unwrap();
    }
    let e = restore(artefact.path(), target.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&e, tree) == m);
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    let names = FsStorage.list(target.path()).unwrap();
    assert!(!names.iter().any(|n| n == "wal-1-00000000f0000000"));
}

/// The artefact holds no `LOCK`, so it cannot be opened as a database
/// (TS-26, TS-27).
#[test]
fn an_artefact_does_not_open_as_a_database() {
    let dir = test_dirs().remove(0);
    let (src, artefact) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    backed_up(src.path(), artefact.path());
    let r = Engine::open(artefact.path(), no_flush(), CAP, sink());
    assert!(matches!(r, Err(Error::LockMissing)), "{:?}", r.err());
}

/// Audit (2026-10-03), TS-26 restore step 5 and "a restore interrupted
/// before step 5 leaves no `CONTROL` ... a new restore overwrites the files
/// it left": an ENOSPC on the write of `CONTROL` (step 5) returns an error
/// but leaves an empty `CONTROL` in the target, with no crash involved. The
/// target is then neither the old state (no database, restorable) nor the
/// new one: a second restore is refused with `DatabaseExists`, and open
/// fails on a `CONTROL` without a valid slot. The same state follows a
/// power loss between the creation of `CONTROL` and its `sync_data` on a
/// file system that keeps the directory entry but not the data (ext4
/// delayed allocation). Fails until step 5 writes `CONTROL` under a
/// temporary name and renames it, as `MANIFEST` is written.
#[test]
fn audit_a_failed_control_write_in_step_5_leaves_a_restorable_target() {
    use crate::storage::fault::{Call, Fault, FaultStorage, Kind};
    let dir = test_dirs().remove(0);
    let (src, artefact, dry, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let (tree, m) = backed_up(src.path(), artefact.path());
    // Dry run: the position of the write of `CONTROL` among the restore's
    // `write_at` calls.
    let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    f.skip_device_sync();
    drop(restore(artefact.path(), dry.path(), f.clone(), CAP, sink()).unwrap());
    // The first write of `CONTROL`, under its temporary name since the fix.
    let control = [dry.path().join(CONTROL), dry.path().join(CONTROL_TMP)];
    let n = f
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::WriteAt { .. }))
        .position(|c| matches!(c, Call::WriteAt { path, .. } if control.contains(path)))
        .expect("restore writes CONTROL") as u64
        + 1;
    // Only the call log is needed from the dry run.
    drop(dry);
    // The real run: that write fails with ENOSPC.
    let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    f.skip_device_sync();
    f.fail_nth(Kind::WriteAt, n, Fault::NoSpace);
    let r = restore(artefact.path(), target.path(), f.clone(), CAP, sink());
    assert!(r.is_err(), "precondition: the CONTROL write failed");
    // The space is back: the retry must succeed, or the target must open
    // as the restored database.
    let again = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    let opened = Engine::open(target.path(), no_flush(), CAP, sink());
    assert!(
        again.is_ok() || opened.is_ok(),
        "the target is stuck: retry {:?}, open {:?}",
        again.err(),
        opened.err()
    );
    let e = again.or(opened).unwrap();
    assert!(entries(&e, tree) == m);
}

/// Rewrites the artefact's `MANIFEST` as `f` changes it, with a correct
/// checksum: an artefact whose files all match their entries but whose
/// content is not what a backup writes.
fn patch_manifest(artefact: &Path, f: impl FnOnce(&mut Manifest)) {
    let path = artefact.join(MANIFEST);
    let file = FsStorage.open(&path).unwrap();
    let mut b = vec![0u8; file.len().unwrap() as usize];
    file.read_at(&mut b, 0).unwrap();
    let mut m = Manifest::decode(&b).unwrap();
    f(&mut m);
    drop(file);
    FsStorage.remove(&path).unwrap();
    FsStorage
        .create(&path)
        .unwrap()
        .write_at(&m.encode(), 0)
        .unwrap();
}

/// Audit finding 11 (2026-10-03), TS-26 restore step 6: recovery must reach
/// the backup's `stop`, or the restored database is refused as corruption
/// (PostgreSQL: "WAL ends before end of online backup", R-PG
/// `src/backend/access/transam/xlog.c:6598`). The refusal holds for every
/// later open of the target too, not only for the one the restore runs; the
/// genuine artefact still restores and reopens. PostgreSQL keeps the
/// requirement in `backup_label` until recovery has passed the backup's
/// end (R-PG `src/backend/access/transam/xlogrecovery.c`, `read_backup_label`).
#[test]
fn audit_a_wal_ending_before_stop_is_refused_by_the_restore_and_every_later_open() {
    let dir = test_dirs().remove(0);
    let (src, artefact, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    let (tree, m) = backed_up(src.path(), artefact.path());
    let mut good = Vec::new();
    {
        let f = FsStorage.open(&artefact.path().join(MANIFEST)).unwrap();
        good.resize(f.len().unwrap() as usize, 0);
        f.read_at(&mut good, 0).unwrap();
    }
    // `stop` one frame beyond the copied WAL.
    patch_manifest(artefact.path(), |m| {
        m.stop += crate::storage::wal::FRAME_BYTES
    });
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(matches!(r, Err(Error::Corruption(_))), "{:?}", r.err());
    for _ in 0..2 {
        let r = Engine::open(target.path(), no_flush(), CAP, sink());
        assert!(
            matches!(r, Err(Error::Corruption(_))),
            "a later open accepted a WAL that ends before the backup's stop: {:?}",
            r.err().map(|e| e.to_string())
        );
    }
    // The genuine artefact, restored into a fresh target, opens. The
    // refused target goes first, to keep the test's files small.
    drop(target);
    let fixed = tempfile::tempdir_in(dir.path()).unwrap();
    FsStorage.remove(&artefact.path().join(MANIFEST)).unwrap();
    FsStorage
        .create(&artefact.path().join(MANIFEST))
        .unwrap()
        .write_at(&good, 0)
        .unwrap();
    let e = restore(artefact.path(), fixed.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&e, tree) == m);
    e.close().unwrap();
    let e = Engine::open(fixed.path(), no_flush(), CAP, sink()).unwrap();
    assert!(entries(&e, tree) == m);
}

/// Audit finding 11: a segment entry whose bytes would not fit inside one
/// segment at their offset is refused before anything is written, and so
/// is a `MANIFEST` longer than any backup writes, before it is read into
/// memory.
#[test]
fn audit_an_out_of_bounds_segment_or_an_oversized_manifest_is_refused_before_any_write() {
    let dir = test_dirs().remove(0);
    let (src, artefact, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    backed_up(src.path(), artefact.path());
    // `start` moved past the end of the first segment: that segment's
    // copied bytes would be written beyond it.
    patch_manifest(artefact.path(), |m| {
        let first = m
            .files
            .iter()
            .filter_map(|f| parse_segment_name(f.name.as_ref()))
            .map(|(_, lsn)| lsn)
            .min()
            .expect("a segment");
        m.start = first + SEGMENT_SIZE + 1;
    });
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(
        matches!(&r, Err(Error::Corruption(w)) if w.contains("does not belong")),
        "{:?}",
        r.err()
    );
    assert!(FsStorage.list(target.path()).unwrap().is_empty());
    // A `MANIFEST` one byte longer than the longest one restore reads.
    let path = artefact.path().join(MANIFEST);
    FsStorage
        .open(&path)
        .unwrap()
        .set_len(MANIFEST_MAX + 1)
        .unwrap();
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(
        matches!(&r, Err(Error::IncompleteBackup(w)) if w.contains("longer")),
        "{:?}",
        r.err()
    );
    assert!(FsStorage.list(target.path()).unwrap().is_empty());
}

/// Re-audit (2026-10-07), TS-05 "a later create overwrites what it left"
/// and TS-26 "a restore interrupted before step 5 leaves no database": a
/// restore that fails at its `CONTROL` (ENOSPC creating `CONTROL.tmp`)
/// leaves `data-<g>`, the artefact's WAL segments and `BACKUP_END`, and no
/// `CONTROL`. `Engine::create` on that location must remove the segments
/// of the other database too; kept, the new database's recovery from redo
/// point 0 reads them and the location can no longer be opened.
#[test]
fn audit_a_create_after_an_interrupted_restore_ignores_its_segments() {
    create_after_an_interrupted_restore(true);
}

/// As above, with a backup whose `start` is LSN 0 (a backup taken before
/// the first commit, while commits run): the other database's frames then
/// begin at the new database's redo point and, kept, would be replayed
/// into it.
#[test]
fn audit_a_create_after_an_interrupted_restore_from_lsn_0_replays_nothing() {
    create_after_an_interrupted_restore(false);
}

fn create_after_an_interrupted_restore(tree_before_backup: bool) {
    use crate::storage::fault::{Call, Fault, FaultStorage, Kind};
    let dir = test_dirs().remove(0);
    let (src, artefact, dry, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    // A backup with commits between `start` and `stop`: the commits run
    // while the backup is paused after its checkpoint.
    {
        use crate::sync::Mutex;
        let e = Engine::create(
            src.path(),
            no_flush(),
            CAP,
            crate::test_support::OWNER,
            sink(),
        )
        .unwrap();
        let early = tree_before_backup.then(|| e.create_trees(1).unwrap()[0]);
        let (tx, rx) = crate::sync::mpsc::channel::<()>();
        let (go_tx, go) = crate::sync::mpsc::channel::<()>();
        let (tx, go) = (Mutex::new(tx), Mutex::new(go));
        *e.wal().points.backup_started.lock().unwrap() = Some(Box::new(move || {
            tx.lock().unwrap().send(()).unwrap();
            let _ = go.lock().unwrap().recv();
        }));
        std::thread::scope(|s| {
            let b = s.spawn(|| e.backup(artefact.path()).unwrap());
            rx.recv().unwrap();
            let tree = early.unwrap_or_else(|| e.create_trees(1).unwrap()[0]);
            for i in 0..20u32 {
                let k = i.to_be_bytes();
                e.commit(
                    &[Write {
                        tree,
                        key: &k,
                        value: Some(&[5; 100]),
                        stamp: 0,
                    }],
                    Ids::default(),
                )
                .unwrap();
            }
            go_tx.send(()).unwrap();
            let st = b.join().unwrap();
            assert!(
                st.manifest.stop > st.manifest.start,
                "precondition: WAL in the backup"
            );
            assert_eq!(
                st.manifest.start == 0,
                !tree_before_backup,
                "precondition: start"
            );
        });
        *e.wal().points.backup_started.lock().unwrap() = None;
        e.close().unwrap();
    }
    let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    f.skip_device_sync();
    drop(restore(artefact.path(), dry.path(), f.clone(), CAP, sink()).unwrap());
    let tmp = dry.path().join(CONTROL_TMP);
    let n = f
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::Create(_)))
        .position(|c| matches!(c, Call::Create(p) if *p == tmp))
        .expect("restore creates CONTROL.tmp") as u64
        + 1;
    drop(dry);
    let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    f.skip_device_sync();
    f.fail_nth(Kind::Create, n, Fault::NoSpace);
    assert!(restore(artefact.path(), target.path(), f.clone(), CAP, sink()).is_err());
    drop(f);
    let left = FsStorage.list(target.path()).unwrap();
    assert!(
        !left.iter().any(|n| n == CONTROL),
        "precondition: no CONTROL"
    );
    assert!(
        left.iter().any(|n| parse_segment_name(n).is_some()),
        "precondition: segments left: {left:?}"
    );
    let e = Engine::create(
        target.path(),
        no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    );
    let e = e.unwrap_or_else(|e| {
        let open = Engine::open(target.path(), no_flush(), CAP, sink()).err();
        let again = Engine::create(
            target.path(),
            no_flush(),
            CAP,
            crate::test_support::OWNER,
            sink(),
        )
        .err();
        panic!("create failed: {e}; then open: {open:?}; then create: {again:?}")
    });
    // The first tree after those create makes (TS-17 Creation).
    assert!(
        e.store()
            .tree(crate::catalog::CREATED_TREES + 1)
            .unwrap()
            .is_none(),
        "the new database holds a tree"
    );
    assert_eq!(
        e.recovered().groups,
        0,
        "the new database replayed WAL groups"
    );
}

/// Re-audit (2026-10-07), FR-OPS-009 / FR-DB-003: restore checks for
/// `CONTROL` at step 1, before it takes `LOCK` (step 4), and step 5
/// renames `CONTROL.tmp` over any `CONTROL`. A database created in that
/// window (here by a storage whose `try_lock` first creates and closes
/// one) must make the restore fail with `DatabaseExists`, as
/// `Engine::create` does by checking again under the lock.
#[test]
fn audit_a_restore_never_replaces_a_database_created_after_its_first_check() {
    use crate::sync::atomic::{AtomicBool, Ordering};
    use std::ffi::OsString;
    struct CreateOnLock {
        target: std::path::PathBuf,
        done: AtomicBool,
    }
    impl Storage for CreateOnLock {
        fn open(&self, p: &Path) -> std::io::Result<Box<dyn StorageFile>> {
            FsStorage.open(p)
        }
        fn create(&self, p: &Path) -> std::io::Result<Box<dyn StorageFile>> {
            FsStorage.create(p)
        }
        fn remove(&self, p: &Path) -> std::io::Result<()> {
            FsStorage.remove(p)
        }
        fn rename(&self, a: &Path, b: &Path) -> std::io::Result<()> {
            FsStorage.rename(a, b)
        }
        fn list(&self, d: &Path) -> std::io::Result<Vec<OsString>> {
            FsStorage.list(d)
        }
        fn sync_dir(&self, _: &Path) -> std::io::Result<()> {
            Ok(())
        }
        fn try_lock(&self, p: &Path) -> std::io::Result<Box<dyn crate::storage::StorageLock>> {
            if p.parent() == Some(self.target.as_path()) && !self.done.swap(true, Ordering::SeqCst)
            {
                let e = Engine::create(
                    &self.target,
                    no_flush(),
                    CAP,
                    crate::test_support::OWNER,
                    sink(),
                )
                .unwrap();
                let t = e.create_trees(1).unwrap()[0];
                e.commit(
                    &[Write {
                        tree: t,
                        key: b"created",
                        value: Some(b"1"),
                        stamp: 0,
                    }],
                    Ids::default(),
                )
                .unwrap();
                e.close().unwrap();
            }
            FsStorage.try_lock(p)
        }
        fn available_space(&self, d: &Path) -> std::io::Result<u64> {
            FsStorage.available_space(d)
        }
    }
    let dir = test_dirs().remove(0);
    let (src, artefact, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    backed_up(src.path(), artefact.path());
    let s = Arc::new(CreateOnLock {
        target: target.path().to_path_buf(),
        done: AtomicBool::new(false),
    });
    let r = restore(artefact.path(), target.path(), s.clone(), CAP, sink());
    assert!(s.done.load(Ordering::SeqCst), "precondition: the hook ran");
    assert!(
        matches!(r, Err(Error::DatabaseExists)),
        "the restore replaced a database created after its step-1 check: {:?}",
        r.as_ref().err()
    );
}

/// Final audit (2026-10-07), TS-26 restore step 4 vs `LOCK`: a database
/// created between step 1 and the lock whose handle is still open. Restore
/// has already created or emptied `LOCK` (same inode, a file this crate
/// never writes) and must then fail with `Locked`, leave the open handle
/// working, and leave a database every later open accepts, holding its data.
#[test]
fn audit_a_restore_meeting_a_database_created_and_still_open_leaves_it_intact() {
    use crate::sync::Mutex;
    use crate::sync::atomic::{AtomicBool, Ordering};
    use std::ffi::OsString;
    struct CreateOnLock {
        target: std::path::PathBuf,
        done: AtomicBool,
        held: Mutex<Option<(Engine, Tree)>>,
    }
    impl Storage for CreateOnLock {
        fn open(&self, p: &Path) -> std::io::Result<Box<dyn StorageFile>> {
            FsStorage.open(p)
        }
        fn create(&self, p: &Path) -> std::io::Result<Box<dyn StorageFile>> {
            FsStorage.create(p)
        }
        fn remove(&self, p: &Path) -> std::io::Result<()> {
            FsStorage.remove(p)
        }
        fn rename(&self, a: &Path, b: &Path) -> std::io::Result<()> {
            FsStorage.rename(a, b)
        }
        fn list(&self, d: &Path) -> std::io::Result<Vec<OsString>> {
            FsStorage.list(d)
        }
        fn sync_dir(&self, _: &Path) -> std::io::Result<()> {
            Ok(())
        }
        fn try_lock(&self, p: &Path) -> std::io::Result<Box<dyn crate::storage::StorageLock>> {
            if p.parent() == Some(self.target.as_path()) && !self.done.swap(true, Ordering::SeqCst)
            {
                let e = Engine::create(
                    &self.target,
                    no_flush(),
                    CAP,
                    crate::test_support::OWNER,
                    sink(),
                )
                .unwrap();
                let t = e.create_trees(1).unwrap()[0];
                e.commit(
                    &[Write {
                        tree: t,
                        key: b"created",
                        value: Some(b"1"),
                        stamp: 0,
                    }],
                    Ids::default(),
                )
                .unwrap();
                *self.held.lock().unwrap() = Some((e, t));
            }
            FsStorage.try_lock(p)
        }
        fn available_space(&self, d: &Path) -> std::io::Result<u64> {
            FsStorage.available_space(d)
        }
    }
    use std::os::unix::fs::MetadataExt;
    let dir = test_dirs().remove(0);
    let (src, artefact, target) = (
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
        tempfile::tempdir_in(dir.path()).unwrap(),
    );
    backed_up(src.path(), artefact.path());
    let s = Arc::new(CreateOnLock {
        target: target.path().to_path_buf(),
        done: AtomicBool::new(false),
        held: Mutex::new(None),
    });
    let r = restore(artefact.path(), target.path(), s.clone(), CAP, sink());
    assert!(s.done.load(Ordering::SeqCst), "precondition: the hook ran");
    assert!(matches!(r, Err(Error::Locked)), "{:?}", r.as_ref().err());
    let lock = target.path().join(LOCK);
    let ino = std::fs::metadata(&lock).unwrap().ino();
    assert_eq!(std::fs::metadata(&lock).unwrap().len(), 0);
    let (e, t) = s.held.lock().unwrap().take().unwrap();
    // The handle still holds the lock on the same `LOCK` inode.
    assert!(matches!(
        Engine::open(target.path(), no_flush(), CAP, sink()),
        Err(Error::Locked)
    ));
    e.commit(
        &[Write {
            tree: t,
            key: b"after",
            value: Some(b"2"),
            stamp: 0,
        }],
        Ids::default(),
    )
    .unwrap();
    e.close().unwrap();
    // A plain restore now refuses at step 1; the database opens with both keys.
    let r = restore(artefact.path(), target.path(), no_flush(), CAP, sink());
    assert!(
        matches!(r, Err(Error::DatabaseExists)),
        "{:?}",
        r.as_ref().err()
    );
    assert_eq!(std::fs::metadata(&lock).unwrap().ino(), ino);
    let e = Engine::open(target.path(), no_flush(), CAP, sink()).unwrap();
    let got = entries(&e, t);
    assert_eq!(got.len(), 2, "{got:?}");
    let c = e.store().check().unwrap();
    assert!(c.is_ok(), "{:?}", c.problems);
    e.close().unwrap();
}
