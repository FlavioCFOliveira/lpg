//! The fault-injection crash suite (TS-30 Durability, rmp #20).
//!
//! One harness drives every Sprint 1 write path ([`Workload`]): commits
//! (with one checkpoint and one segment created ahead of use), a
//! checkpoint that recycles a segment, the checkpoint at the end of
//! recovery, backup, restore, migration, and a bulk load (an external sort
//! that spills runs, then a bulk build and its install). Each runs on
//! [`FaultStorage`] over the file system, with a model of the commits that
//! returned.
//!
//! - **Crash-point sweep** (R1). A dry run counts the workload's crash
//!   points: its `write_at`, `sync_data`, `sync_dir`, `rename`, `create`
//!   and `remove` calls. Then, for every `k`, the workload runs again and
//!   the storage stops after its `k`-th call ([`FaultStorage::crash_after`]);
//!   the crash keeps nothing of the unsynced state, a seeded subset of it,
//!   or tears it at 512 bytes; the last two modes also keep a seeded prefix
//!   of the unsynced creations, renames and removals ([`Crash`]).
//! - **Failure sweep** (R2, R8). The `n`-th call of a kind fails with `EIO`
//!   or `ENOSPC` ([`FaultStorage::fail_nth`]): every `sync_data` and every
//!   kind with `EIO` in the commits workload; every reservation, creation
//!   and write with `ENOSPC` in every workload. A failed `sync_data` drops
//!   the file's unsynced writes from what can become durable (R8). In the
//!   commits workload, the `n`-th page-copy reservation of TS-16 step 6
//!   also fails, for every `n`, so that the undo of a batch that has
//!   already changed pages runs ([`Arm::FailCopy`]).
//! - **Oracle.** ENOSPC or a refused memory reservation fails only the
//!   commit that needed it, which leaves the database usable, readable and
//!   equal to the model of the commits that returned; any other failure
//!   makes it unusable. After a reopen, the database equals the model after
//!   a prefix of the attempted commits that includes every commit that
//!   returned, and passes the integrity check (TS-12). Every batch also
//!   inserts its own marker key first, so a failed batch that came back is
//!   seen even though later batches rewrite the same keys. For backup, restore
//!   and migration, each location opens as the old or the new database and
//!   matches that database's model; a backup without a valid `MANIFEST`
//!   is refused by restore. A target a restore left without `CONTROL`
//!   then takes, on alternate runs, a second restore or a create, whose
//!   database is empty and passes the integrity check.
//!
//! Corruption cases, misdirected and lost writes (R9) and the checkpoint
//! call order (R3) are separate tests below. Seeds rotate unless `LPG_SEED` is set (R5).
//!
//! The method follows SQLite's crash and fault-injection tests: a VFS that
//! simulates a power loss after a chosen sync (R-SQLITE `src/test6.c`), and
//! a loop that fails the `N`-th operation for `N` = 1, 2, … until no fault
//! fires (R-LIBSQL `libsql-sqlite3/test/malloc_common.tcl:41-50,
//! 326-385`).

use crate::sync::Arc;
use crate::sync::atomic::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::suite_dirs;
use crate::obs::recording::RecordingSink;
use crate::ops::backup::{MANIFEST, MANIFEST_TMP};
use crate::ops::migrate::migrate_with;
use crate::ops::migrate::tests::{TEST_FORMATS, downgrade_to_format_0};
use crate::ops::restore::restore;
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::{CONTROL, Engine, Tuning, data_name};
use crate::storage::error::Error;
use crate::storage::fault::{Call, Crash, Fault, FaultStorage, Kind};
use crate::storage::sort::{MIN_MEMORY, is_sort_temp};
use crate::storage::wal::parse_segment_name;
use crate::storage::{FsStorage, Storage};
use crate::test_support::seed;
use crate::tx::Isolation;
use crate::tx::commit::Ids;

const CAP: u64 = 32 << 20;
/// Keys loaded before every workload: about 24 leaves of 400-byte values.
const KEYS: u32 = 400;
/// Keys between two keys of one batch, so that a batch changes 25 leaves.
const STRIDE: usize = 16;

type Model = BTreeMap<Vec<u8>, Vec<u8>>;

fn key(i: u32) -> Vec<u8> {
    format!("k{i:05}").into_bytes()
}

/// No periodic checkpoint: the workloads run their own.
fn tuning() -> Tuning {
    Tuning {
        checkpoint_interval: Duration::from_secs(3600),
        checkpoint_commits: u64::MAX,
        ..Tuning::default()
    }
}

/// Batch `c` of a workload: first its own marker key `m<c>`, so that a
/// failed batch that came back is visible even where later batches rewrite
/// its other keys; then the values of every `STRIDE`-th key become `c`;
/// every seventh batch also deletes one key.
fn batch(c: u32) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
    let mut b: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![(
        format!("m{c:05}").into_bytes(),
        Some(c.to_le_bytes().to_vec()),
    )];
    b.extend(
        (0..KEYS)
            .step_by(STRIDE)
            .map(|i| (key(i), Some(vec![c as u8; 400]))),
    );
    if c.is_multiple_of(7) {
        b.push((key(c % KEYS + 1), None));
    }
    b
}

fn apply(m: &mut Model, b: &[(Vec<u8>, Option<Vec<u8>>)]) {
    for (k, v) in b {
        match v {
            Some(v) => m.insert(k.clone(), v.clone()),
            None => m.remove(k),
        };
    }
}

fn commit(e: &Engine, tree: Tree, b: &[(Vec<u8>, Option<Vec<u8>>)]) -> Result<(), Error> {
    let writes: Vec<Write<'_>> = b
        .iter()
        .map(|(k, v)| Write {
            tree,
            key: k,
            value: v.as_deref(),
            stamp: 0,
        })
        .collect();
    e.commit(&writes, Ids::default()).map(drop)
}

fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

fn entries(e: &Engine, tree: Tree) -> Model {
    let mut out = Model::new();
    e.store()
        .scan_prefix(tree, b"", |k, v| {
            out.insert(k.to_vec(), v.to_vec());
            true
        })
        .unwrap();
    out
}

fn assert_intact(e: &Engine, what: &str) {
    // The trees the suite creates outside the catalog: the workload's,
    // then the side tree or the bulk-built tree ([`BULK_TREE`]).
    let raw = [crate::catalog::CREATED_TREES + 1, BULK_TREE];
    let p = e.check_integrity_except(&raw).unwrap();
    assert!(p.is_empty(), "{what}: {p:?}");
    // TS-17: after any open, the statistics are exact.
    let stats = e.core().catalog.stats().counts().clone();
    assert!(
        stats == crate::storage::graph::recount(e.core()).unwrap(),
        "{what}: the statistics differ from a recount"
    );
}

/// The Sprint 1 write paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Workload {
    Commits,
    Checkpoint,
    RecoveryCheckpoint,
    Backup,
    Restore,
    Migration,
    /// An external sort of unsorted records in the database location, whose
    /// runs spill to files, then a bulk build from it and its install.
    BulkLoad,
    /// Sprint 2 paths (audit Round A, rmp #35): serializable and
    /// snapshot-isolation commits with delete stamps, a tree drop, and
    /// garbage-collection groups ([`tx_batch`]); a graph created with its
    /// nodes and edges, then dropped, node and edge property updates, a
    /// node deletion, and a checkpoint's statistics batch (rmp #31, #32;
    /// [`tx_graph`]); two online index builds (rmp #33; [`tx_indexes`]).
    Transactions,
}

/// What a run arms before the workload.
#[derive(Debug, Clone, Copy)]
enum Arm {
    Nothing,
    CrashAfter(u64, Crash),
    Fail(Kind, u64, Fault),
    /// The `n`-th page-copy reservation of the workload fails with
    /// [`Error::MemoryLimit`] (TS-16 step 6).
    FailCopy(u64),
}

/// Records of the bulk-load workload: unsorted keys, small values and
/// every 400th value long enough for an overflow chain.
fn bulk_records() -> Vec<(Vec<u8>, Vec<u8>)> {
    const N: u32 = 3_000;
    (0..N)
        .map(|i| {
            let j = (i * 1_831) % N; // a permutation: 1 831 is prime to N
            let len = if j % 400 == 7 { 3_000 } else { 80 };
            (format!("b{j:05}").into_bytes(), vec![(j % 251) as u8; len])
        })
        .collect()
}

/// The tree of a bulk load: the next tree after the workload's own, which
/// follows the trees create makes (TS-17 Creation).
const BULK_TREE: u64 = crate::catalog::CREATED_TREES + 2;

/// The directories and storage of one run.
struct Run {
    db: tempfile::TempDir,
    target: tempfile::TempDir,
    restored: tempfile::TempDir,
    fault: Arc<FaultStorage>,
}

/// State carried from the setup to the oracle.
struct State {
    engine: Option<Engine>,
    tree: Tree,
    /// The model after the setup.
    base: Model,
    /// Batches the workload attempts, in order, after the setup.
    attempted: Vec<u32>,
    /// How many of `attempted` returned.
    acked: usize,
    /// Whether the bulk load returned.
    loaded: bool,
    /// [`Workload::Transactions`]: the tree it drops, and its entries.
    side: Option<(Tree, Model)>,
    /// [`Workload::Transactions`]: the rows of the default graph.
    rows: Option<Rows>,
    /// [`Workload::Transactions`]: whether both index creations returned.
    indexed: bool,
}

impl Run {
    fn new(base: &Path) -> Run {
        let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
        // FaultStorage models durability itself; the device flush adds only
        // its latency (DECISION 77 on rmp task #15).
        fault.skip_device_sync();
        Run {
            db: tempfile::tempdir_in(base).unwrap(),
            target: tempfile::tempdir_in(base).unwrap(),
            restored: tempfile::tempdir_in(base).unwrap(),
            fault,
        }
    }

    fn storage(&self) -> Arc<dyn Storage> {
        self.fault.clone()
    }

    fn create(&self) -> (Engine, Tree, Model) {
        let e = Engine::create_tuned(
            self.db.path(),
            self.storage(),
            CAP,
            crate::test_support::OWNER,
            sink(),
            tuning(),
        )
        .unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let load: Vec<(Vec<u8>, Option<Vec<u8>>)> =
            (0..KEYS).map(|i| (key(i), Some(vec![0; 400]))).collect();
        for half in load.chunks(load.len() / 2) {
            commit(&e, tree, half).unwrap();
        }
        let m = entries(&e, tree);
        (e, tree, m)
    }

    fn open(&self, dir: &Path) -> Result<Engine, Error> {
        Engine::open_tuned(dir, self.storage(), CAP, sink(), tuning())
    }
}

/// Prepares `w` without any fault armed.
fn setup(w: Workload, r: &Run) -> State {
    let (e, tree, mut base) = r.create();
    let mut s = State {
        engine: None,
        tree,
        base: Model::new(),
        attempted: Vec::new(),
        acked: 0,
        loaded: false,
        side: None,
        rows: None,
        indexed: false,
    };
    let pre = match w {
        Workload::Commits => 0,
        // Enough WAL to fill the first segment, with about 25 dirty pages.
        Workload::Checkpoint => 100,
        _ => 5,
    };
    for c in 1..=pre {
        let b = batch(c);
        commit(&e, tree, &b).unwrap();
        apply(&mut base, &b);
    }
    // A segment created ahead of use is created now, not during the run.
    e.wal().precreate();
    s.base = base;
    match w {
        Workload::Commits | Workload::Checkpoint | Workload::Backup | Workload::BulkLoad => {
            s.engine = Some(e);
        }
        Workload::Transactions => {
            // The workload runs the collector's passes itself, so that its
            // calls are the same in every run.
            crate::tx::tests::pause_gc(&e);
            let side = e.create_trees(1).unwrap()[0];
            e.token(crate::catalog::TokenKind::Label, "L").unwrap();
            e.token(crate::catalog::TokenKind::EdgeType, "T").unwrap();
            e.token(crate::catalog::TokenKind::PropertyKey, "P")
                .unwrap();
            s.rows = Some(create_rows(&e));
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let mut m = Model::new();
            for i in 0..200u32 {
                let v = vec![i as u8; if i % 50 == 0 { 3_000 } else { 100 }];
                tx.put(side, &key(i), &v).unwrap();
                m.insert(key(i), v);
            }
            tx.commit().unwrap();
            e.wal().precreate();
            s.side = Some((side, m));
            s.engine = Some(e);
        }
        Workload::RecoveryCheckpoint => drop(e),
        Workload::Restore => {
            // Batches commit while the backup is paused after its
            // checkpoint, so that the backup's WAL holds groups after
            // `start` that a restore replays, and that a stale copy of it
            // would offer to a later database.
            use crate::sync::Mutex;
            use crate::sync::mpsc::channel;
            let (tx, rx) = channel::<()>();
            let (go_tx, go) = channel::<()>();
            let (tx, go) = (Mutex::new(tx), Mutex::new(go));
            *e.wal().points.backup_started.lock().unwrap() = Some(Box::new(move || {
                tx.lock().unwrap().send(()).unwrap();
                let _ = go.lock().unwrap().recv();
            }));
            std::thread::scope(|sc| {
                let b = sc.spawn(|| e.backup(r.target.path()).unwrap());
                rx.recv().unwrap();
                for c in pre + 1..=pre + 3 {
                    let b = batch(c);
                    commit(&e, tree, &b).unwrap();
                    apply(&mut s.base, &b);
                }
                go_tx.send(()).unwrap();
                let st = b.join().unwrap();
                assert!(
                    st.manifest.stop > st.manifest.start,
                    "precondition: commits in the backup's WAL"
                );
            });
            *e.wal().points.backup_started.lock().unwrap() = None;
            e.close().unwrap();
        }
        Workload::Migration => {
            e.close().unwrap();
            downgrade_to_format_0(r.db.path());
        }
    }
    s
}

/// Runs the write path of `w` with `arm` set. Checks the failure oracle as
/// it goes: a reservation ENOSPC leaves the database usable and readable;
/// any other failure leaves it unusable. ENOSPC from a write that is not a
/// reservation (a WAL group write) is "any other failure" (TS-12).
fn run(w: Workload, r: &Run, s: &mut State, arm: Arm) {
    let armed = !matches!(arm, Arm::Nothing);
    let reservation = matches!(
        arm,
        Arm::Fail(Kind::Reserve | Kind::Create, _, Fault::NoSpace)
    );
    match w {
        Workload::Commits => {
            let e = s.engine.as_ref().unwrap();
            for c in 1..=60 {
                if c == 10 && e.checkpoint().is_err() {
                    assert!(
                        e.check().is_err(),
                        "a failed checkpoint left the database usable"
                    );
                    return;
                }
                let b = batch(c);
                s.attempted.push(c);
                match commit(e, s.tree, &b) {
                    Ok(()) => s.acked = s.attempted.len(),
                    Err(err)
                        if (err.is_no_space() && (reservation || e.check().is_ok()))
                            || matches!(err, Error::MemoryLimit { .. }) =>
                    {
                        // Undone, and the database stays usable (FR-REC-007)
                        // and equal to the commits that returned.
                        s.attempted.pop();
                        e.check().unwrap();
                        let mut m = s.base.clone();
                        for &c in &s.attempted {
                            apply(&mut m, &batch(c));
                        }
                        assert!(
                            entries(e, s.tree) == m,
                            "the failed batch {c} ({err}) left a trace"
                        );
                    }
                    Err(err) => {
                        assert!(armed, "unexpected failure: {err}");
                        assert!(e.check().is_err(), "{err} left the database usable");
                        assert!(
                            commit(e, s.tree, &batch(99)).is_err(),
                            "a later commit succeeded"
                        );
                        return;
                    }
                }
            }
        }
        Workload::Transactions => run_transactions(s, arm),
        Workload::Checkpoint => {
            let e = s.engine.as_ref().unwrap();
            if e.checkpoint().is_err() {
                assert!(
                    e.check().is_err(),
                    "a failed checkpoint left the database usable"
                );
            }
        }
        Workload::RecoveryCheckpoint => {
            s.engine = r.open(r.db.path()).ok();
        }
        Workload::Backup => {
            let e = s.engine.as_ref().unwrap();
            if let Err(err) = e.backup(r.target.path()) {
                assert!(armed, "unexpected failure: {err}");
                if matches!(arm, Arm::Fail(Kind::ReadAt, ..)) {
                    // A failed read of the database (TS-12).
                    assert!(e.check().is_err(), "a failed read left the database usable");
                }
            }
        }
        Workload::BulkLoad => {
            let e = s.engine.as_ref().unwrap();
            let load = || -> Result<(), Error> {
                let mut sorter = e.sorter(MIN_MEMORY)?;
                for (k, v) in bulk_records() {
                    sorter.push(&k, &v)?;
                }
                let mut sorted = sorter.finish()?;
                assert!(sorted.files() > 0, "precondition: the sort spills runs");
                let t = e.bulk_build(&mut sorted)?;
                assert_eq!(t.id(), BULK_TREE);
                Ok(())
            };
            match load() {
                Ok(()) => s.loaded = true,
                Err(err) => assert!(armed, "unexpected failure: {err}"),
            }
        }
        Workload::Restore => {
            s.engine = restore(r.target.path(), r.restored.path(), r.storage(), CAP, sink()).ok();
        }
        Workload::Migration => {
            if let Err(err) = migrate_with(r.db.path(), &*r.fault, &TEST_FORMATS) {
                assert!(armed, "unexpected failure: {err}");
            }
        }
    }
}

/// The state of `dir` equals the model after a prefix of the attempted
/// batches that includes every acknowledged one, and passes the integrity
/// check.
fn assert_prefix(e: &Engine, s: &State, what: &str) {
    let got = entries(e, s.tree);
    let mut m = s.base.clone();
    let mut ok = got == m && s.acked == 0;
    for (i, &c) in s.attempted.iter().enumerate() {
        apply(&mut m, &batch(c));
        if got == m && i + 1 >= s.acked {
            ok = true;
        }
    }
    assert!(
        ok,
        "{what}: the state is not a prefix with every returned commit"
    );
    assert_intact(e, what);
}

/// The oracle after the crash or failure (module documentation).
fn verify(w: Workload, r: &Run, s: &State, arm: Arm, what: &str) {
    let has = |dir: &Path, name: &str| FsStorage.list(dir).unwrap().iter().any(|n| n == name);
    match w {
        Workload::Transactions => {
            let e = super::open_held(r.db.path(), || r.open(r.db.path()))
                .unwrap_or_else(|e| panic!("{what}: reopen: {e}"));
            super::assert_no_stamp_above(&e, what);
            assert_tx_prefix(&e, s, what);
            // Each index is ONLINE, so the integrity check compares it with
            // the data; an index creation that returned is kept.
            let indexes = super::finish_builds(&e, what);
            assert!(
                !s.indexed || indexes.len() == 2,
                "{what}: a returned index creation is lost"
            );
            assert_intact(&e, what);
            // The open recovered: the collector's seed scan finds the
            // delete stamps and the dropped tree again, and its pass
            // removes them; the result passes the integrity check.
            let (side, _) = s.side.as_ref().unwrap();
            crate::test_support::wait_until(&format!("{what}: the dropped tree is freed"), || {
                e.store()
                    .tree_status(side.id())
                    .unwrap()
                    .is_none_or(|(_, d)| d == 0)
            });
            super::assert_stamps_collected(&e, what);
            assert_intact(&e, what);
        }
        Workload::Commits
        | Workload::Checkpoint
        | Workload::RecoveryCheckpoint
        | Workload::Backup => {
            let e = r
                .open(r.db.path())
                .unwrap_or_else(|e| panic!("{what}: reopen: {e}"));
            assert_prefix(&e, s, what);
            drop(e);
            if w == Workload::Backup {
                let back = restore(r.target.path(), r.restored.path(), r.storage(), CAP, sink());
                if has(r.target.path(), MANIFEST) {
                    let e = back.unwrap_or_else(|e| panic!("{what}: restore: {e}"));
                    assert!(entries(&e, s.tree) == s.base, "{what}: restored state");
                    assert_intact(&e, what);
                } else {
                    assert!(
                        matches!(back, Err(Error::IncompleteBackup(_))),
                        "{what}: an incomplete backup was restored: {:?}",
                        back.err()
                    );
                }
            }
        }
        Workload::BulkLoad => {
            let e = r
                .open(r.db.path())
                .unwrap_or_else(|e| panic!("{what}: reopen: {e}"));
            assert_prefix(&e, s, what);
            let names = FsStorage.list(r.db.path()).unwrap();
            assert!(
                !names.iter().any(|n| is_sort_temp(n)),
                "{what}: open left sort files: {names:?}"
            );
            match e.store().tree(BULK_TREE).unwrap() {
                Some(t) => {
                    let want: Model = bulk_records().into_iter().collect();
                    assert!(
                        entries(&e, t) == want,
                        "{what}: the bulk-built tree differs"
                    );
                }
                None => assert!(!s.loaded, "{what}: a returned bulk load is lost"),
            }
            // The open released the pages of a build that did not finish:
            // no build record is left, and they are free again (integrity
            // check in `assert_prefix`); a new load works.
            assert!(
                e.store().build_records().unwrap().is_empty(),
                "{what}: a build record survives the open"
            );
            let mut sorter = e.sorter(MIN_MEMORY).unwrap();
            sorter.push(b"x", b"1").unwrap();
            drop(sorter.finish().unwrap());
        }
        Workload::Restore => {
            if has(r.restored.path(), CONTROL) {
                let e = r
                    .open(r.restored.path())
                    .unwrap_or_else(|e| panic!("{what}: open restored: {e}"));
                assert!(entries(&e, s.tree) == s.base, "{what}: restored state");
                assert_intact(&e, what);
            } else {
                assert!(matches!(r.open(r.restored.path()), Err(Error::NoDatabase)));
                // A location without `CONTROL` holds no database (TS-05):
                // on alternate runs a create there, on the others the next
                // restore, overwrites what this restore left.
                let n = match arm {
                    Arm::CrashAfter(n, _) | Arm::Fail(_, n, _) | Arm::FailCopy(n) => n,
                    Arm::Nothing => 0,
                };
                if n % 2 == 1 {
                    let e = Engine::create_tuned(
                        r.restored.path(),
                        r.storage(),
                        CAP,
                        crate::test_support::OWNER,
                        sink(),
                        tuning(),
                    )
                    .unwrap_or_else(|e| panic!("{what}: create: {e}"));
                    assert_eq!(e.recovered().groups, 0, "{what}: create replayed WAL");
                    assert!(
                        e.store().tree(s.tree.id()).unwrap().is_none(),
                        "{what}: the created database holds a tree"
                    );
                    assert_intact(&e, what);
                } else {
                    let e = restore(r.target.path(), r.restored.path(), r.storage(), CAP, sink())
                        .unwrap_or_else(|e| panic!("{what}: second restore: {e}"));
                    assert!(entries(&e, s.tree) == s.base, "{what}: restored state");
                }
            }
        }
        Workload::Migration => {
            let e = match r.open(r.db.path()) {
                Ok(e) => e,
                Err(Error::MigrationRequired { found: 0, .. }) => {
                    migrate_with(r.db.path(), &*r.fault, &TEST_FORMATS)
                        .unwrap_or_else(|e| panic!("{what}: second migration: {e}"));
                    r.open(r.db.path()).unwrap()
                }
                Err(e) => panic!("{what}: open after migration: {e}"),
            };
            assert!(entries(&e, s.tree) == s.base, "{what}: migrated state");
            assert_intact(&e, what);
        }
    }
}

/// One run of `w` in a new directory under `base`, with `arm`. Returns the
/// crash points the run's workload passed.
fn once(w: Workload, base: &Path, arm: Arm) -> u64 {
    let r = Run::new(base);
    let mut s = setup(w, &r);
    let p0 = r.fault.points();
    match arm {
        Arm::Nothing => {}
        Arm::CrashAfter(k, _) => r.fault.crash_after(k),
        Arm::Fail(kind, n, f) => r.fault.fail_nth(kind, r.fault.count(kind) + n, f),
        Arm::FailCopy(n) => {
            let pool = s.engine.as_ref().expect("an open engine").pool();
            pool.fail_copy
                .store(pool.copy_calls.load(Ordering::SeqCst) + n, Ordering::SeqCst);
        }
    }
    run(w, &r, &mut s, arm);
    let points = r.fault.points() - p0;
    drop(s.engine.take());
    let mode = match arm {
        Arm::CrashAfter(_, m) => m,
        _ => Crash::DropAll,
    };
    r.fault.crash(mode).unwrap();
    verify(w, &r, &s, arm, &format!("{w:?} {arm:?}"));
    points
}

/// What a dry run of `w` does: its crash-point calls, its calls of each
/// kind, and its page-copy reservations.
struct Dry {
    points: u64,
    counts: BTreeMap<Kind, u64>,
    copies: u64,
}

fn dry_counts(w: Workload, base: &Path) -> Dry {
    let r = Run::new(base);
    let mut s = setup(w, &r);
    let p0 = r.fault.points();
    let before: Vec<u64> = Kind::ALL.iter().map(|&k| r.fault.count(k)).collect();
    let copies = |s: &State| {
        s.engine
            .as_ref()
            .map_or(0, |e| e.pool().copy_calls.load(Ordering::SeqCst))
    };
    let c0 = copies(&s);
    run(w, &r, &mut s, Arm::Nothing);
    let counts = Kind::ALL
        .iter()
        .zip(before)
        .map(|(&k, b)| (k, r.fault.count(k) - b))
        .collect();
    Dry {
        points: r.fault.points() - p0,
        counts,
        copies: copies(&s) - c0,
    }
}

/// The values of `1..=n` a run visits: all of them in a full run, else at
/// most three, spread over the range.
fn sample(n: u64, full: bool) -> Vec<u64> {
    if full || n <= 3 {
        (1..=n).collect()
    } else {
        vec![1, n.div_ceil(2), n]
    }
}

/// The crash-point and failure sweeps of `w` in every suite directory;
/// with `copies`, also the page-copy failure sweep.
fn sweep(w: Workload, failures: &[(Kind, Fault)], copies: bool) {
    let (dirs, full) = suite_dirs();
    for (fs, base) in dirs {
        let started = Instant::now();
        let Dry {
            points,
            counts,
            copies: copy_calls,
        } = dry_counts(w, &base);
        assert!(points > 0, "{w:?}: the workload made no crash-point call");
        let ks = sample(points, full);
        let sd = seed();
        let modes = [
            ("drop-all", Crash::DropAll),
            ("keep-random", Crash::KeepRandom { seed: sd }),
            ("tear-512", Crash::Tear { seed: sd ^ 0x5eed }),
        ];
        let mut lines = Vec::new();
        for (name, mode) in modes {
            for &k in &ks {
                once(w, &base, Arm::CrashAfter(k, mode));
            }
            lines.push(format!("{name} {}", ks.len()));
        }
        let mut failed = 0;
        for &(kind, fault) in failures {
            let n = counts[&kind];
            for i in sample(n, full) {
                once(w, &base, Arm::Fail(kind, i, fault));
                failed += 1;
            }
        }
        let mut copy_runs = 0;
        if copies {
            assert!(copy_calls > 0, "{w:?}: the workload made no page copy");
            for i in sample(copy_calls, full) {
                once(w, &base, Arm::FailCopy(i));
                copy_runs += 1;
            }
        }
        println!(
            "SUITE fault fs={fs} workload={w:?} crash_points={points} runs: {} failure_runs={failed} \
             copy_failure_runs={copy_runs} seconds={:.1}{}",
            lines.join(", "),
            started.elapsed().as_secs_f64(),
            if full { "" } else { " (sample)" }
        );
    }
}

/// Transaction batches of [`Workload::Transactions`].
const TX_BATCHES: u32 = 12;
/// The batch that drops the side tree.
const TX_DROP: u32 = 4;
/// The batch that also creates graph `g` with [`GRAPH_NODES`] nodes in a
/// chain of edges, and one node in the default graph (rmp #31, #32).
const TX_GRAPH: u32 = 8;
/// The batch that also drops graph `g`.
const TX_GRAPH_DROP: u32 = 11;
/// The nodes of graph `g`.
const GRAPH_NODES: usize = 40;
/// The batch after which the workload creates a range index on the rows'
/// nodes and one on their edges, and waits for both builds (rmp #33).
const TX_INDEX: u32 = 2;
/// The batch that deletes the last row's node with its edge (detach).
const TX_DELETE: u32 = 10;
/// The row nodes of the default graph.
const ROWS: usize = 8;

/// The rows of [`Workload::Transactions`] in the default graph, created by
/// the setup: [`ROWS`] nodes of label `L` with property `P` = 0, each
/// joined to the next by an edge of type `T` with `P` = 0.
struct Rows {
    nodes: Vec<u64>,
    edges: Vec<u64>,
}

/// The tokens of [`Workload::Transactions`]: label `L`, edge type `T`,
/// property key `P`, all created by the setup.
fn tx_tokens(e: &Engine) -> (u64, u64, u64) {
    use crate::catalog::TokenKind;
    let id = |kind, name| e.token_id(kind, name).expect("the setup's token");
    (
        id(TokenKind::Label, "L"),
        id(TokenKind::EdgeType, "T"),
        id(TokenKind::PropertyKey, "P"),
    )
}

fn create_rows(e: &Engine) -> Rows {
    use crate::types::Value;
    let (l, t, p) = tx_tokens(e);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let g = tx.graph(crate::catalog::DEFAULT_GRAPH_ID).unwrap();
    let nodes: Vec<u64> = (0..ROWS)
        .map(|_| tx.create_node(&g, &[l], &[(p, Value::Int(0))]).unwrap())
        .collect();
    let edges = nodes
        .windows(2)
        .map(|w| {
            tx.create_edge(&g, w[0], w[1], t, &[(p, Value::Int(0))])
                .unwrap()
        })
        .collect();
    tx.commit().unwrap();
    Rows { nodes, edges }
}

/// The isolation level of batch `c`: every third batch runs under
/// snapshot isolation, the others serializable.
fn tx_isolation(c: u32) -> Isolation {
    if c.is_multiple_of(3) {
        Isolation::Snapshot
    } else {
        Isolation::Serializable
    }
}

/// The row node and row edge whose property `P` batch `c` sets to `c`;
/// never the ones [`TX_DELETE`] deletes.
fn row_targets(rows: &Rows, c: u32) -> (u64, u64) {
    (
        rows.nodes[c as usize % (ROWS - 1)],
        rows.edges[c as usize % (ROWS - 2)],
    )
}

/// The graph changes of [`tx_batch`] `c` in `tx`, with the label, edge
/// type and property key `tokens`.
fn tx_graph(
    tx: &mut crate::tx::Transaction,
    c: u32,
    tokens: (u64, u64, u64),
    rows: &Rows,
) -> Result<(), Error> {
    use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID};
    use crate::types::Value;
    if c == TX_GRAPH {
        let id = tx.create_graph(DEFAULT_SCHEMA_ID, "g", crate::test_support::OWNER, None)?;
        let g = tx.graph(id)?;
        let mut prev = None;
        for _ in 0..GRAPH_NODES {
            let n = tx.create_node(&g, &[tokens.0], &[])?;
            if let Some(p) = prev {
                tx.create_edge(&g, p, n, tokens.1, &[])?;
            }
            prev = Some(n);
        }
        let d = tx.graph(DEFAULT_GRAPH_ID)?;
        tx.create_node(&d, &[tokens.0], &[])?;
    }
    if c == TX_GRAPH_DROP {
        tx.drop_object(DEFAULT_SCHEMA_ID, "g")?;
    }
    let d = tx.graph(DEFAULT_GRAPH_ID)?;
    let (node, edge) = row_targets(rows, c);
    tx.set_property(&d, node, tokens.2, Value::Int(c.into()))?;
    tx.set_edge(&d, edge, &[(tokens.2, Value::Int(c.into()))])?;
    if c == TX_DELETE {
        assert!(tx.delete_node(&d, rows.nodes[ROWS - 1], true)?);
    }
    Ok(())
}

/// What snapshots see of the graphs: graph `g`'s nodes and edges, if it
/// exists; the default graph's node count; and property `P` of its row
/// nodes and row edges, by identifier.
#[derive(Debug, PartialEq, Eq)]
struct Graphs {
    g: Option<(usize, usize)>,
    default_nodes: usize,
    nodes: BTreeMap<u64, i64>,
    edges: BTreeMap<u64, i64>,
}

/// What the graph changes of the batches `done` leave ([`Graphs`]).
fn graph_model(rows: &Rows, done: &[u32]) -> Graphs {
    let created = done.contains(&TX_GRAPH);
    let dropped = done.contains(&TX_GRAPH_DROP);
    let deleted = done.contains(&TX_DELETE);
    let mut nodes: BTreeMap<u64, i64> = rows.nodes.iter().map(|&n| (n, 0)).collect();
    let mut edges: BTreeMap<u64, i64> = rows.edges.iter().map(|&n| (n, 0)).collect();
    for &c in done {
        let (n, e) = row_targets(rows, c);
        nodes.insert(n, c.into());
        edges.insert(e, c.into());
    }
    if deleted {
        nodes.remove(&rows.nodes[ROWS - 1]);
        edges.remove(&rows.edges[ROWS - 2]);
    }
    Graphs {
        g: (created && !dropped).then_some((GRAPH_NODES, GRAPH_NODES - 1)),
        default_nodes: ROWS + usize::from(created) - usize::from(deleted),
        nodes,
        edges,
    }
}

/// What a snapshot sees of the graph changes ([`graph_model`]).
fn graph_visible(e: &Engine) -> Graphs {
    use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID};
    use crate::storage::graph::{decode_edge, element_of};
    use crate::types::Value;
    let (_, _, p) = tx_tokens(e);
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let count = |tx: &mut crate::tx::Transaction, t: Tree| {
        let mut n = 0;
        tx.scan_prefix(t, b"", |_, _| {
            n += 1;
            true
        })
        .unwrap();
        n
    };
    let g = match tx.lookup(DEFAULT_SCHEMA_ID, "g").unwrap() {
        Some(o) => {
            let g = tx.graph(o.id).unwrap();
            Some((count(&mut tx, g.nodes), count(&mut tx, g.edges)))
        }
        None => None,
    };
    let d = tx.graph(DEFAULT_GRAPH_ID).unwrap();
    let prop = |props: &[(u64, Value)]| {
        props.iter().find(|x| x.0 == p).map(|x| match x.1 {
            Value::Int(v) => v,
            ref v => panic!("property P is {v:?}"),
        })
    };
    let (mut default_nodes, mut nodes, mut edges) = (0, BTreeMap::new(), BTreeMap::new());
    tx.scan_prefix(d.nodes, b"", |k, v| {
        default_nodes += 1;
        let (_, props) = crate::types::record::decode(v).expect("a node record");
        if let Some(x) = prop(&props) {
            nodes.insert(element_of(k).unwrap(), x);
        }
        true
    })
    .unwrap();
    tx.scan_prefix(d.edges, b"", |k, v| {
        if let Some(x) = prop(&decode_edge(v).expect("an edge record").props) {
            edges.insert(element_of(k).unwrap(), x);
        }
        true
    })
    .unwrap();
    tx.abort();
    Graphs {
        g,
        default_nodes,
        nodes,
        edges,
    }
}

/// Transaction batch `c` (1-based) on the workload's tree: its marker key
/// `t<c>`, five puts and one delete, which leaves a delete stamp; batch
/// [`TX_DROP`] also drops the side tree.
fn tx_batch(c: u32) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
    let mut b: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![(
        format!("t{c:05}").into_bytes(),
        Some(c.to_le_bytes().to_vec()),
    )];
    b.extend((0..5).map(|j| (key((c * 37 + j * 80) % KEYS), Some(vec![c as u8; 300]))));
    b.push((key((c * 53 + 11) % KEYS), None));
    b
}

/// Commits [`tx_batch`] `c` and its graph changes ([`tx_graph`]) as one
/// transaction at [`tx_isolation`]`(c)`.
fn tx_commit(e: &Engine, tree: Tree, side: Tree, rows: &Rows, c: u32) -> Result<(), Error> {
    let mut tx = e.begin(tx_isolation(c))?;
    tx_graph(&mut tx, c, tx_tokens(e), rows)?;
    for (k, v) in tx_batch(c) {
        match v {
            Some(v) => tx.put(tree, &k, &v)?,
            None => {
                tx.delete(tree, &k)?;
            }
        }
    }
    if c == TX_DROP {
        tx.drop_tree(side)?;
    }
    tx.commit().map(drop)
}

/// Creates a range index on property `P` of the nodes of label `L` and
/// one on the edges of type `T` in the default graph, and waits for each
/// build (TS-18 Online build).
fn tx_indexes(e: &Engine) -> Result<(), Error> {
    use crate::index::{Entity, SchemaKind};
    let (l, t, p) = tx_tokens(e);
    for (entity, token) in [(Entity::Node, l), (Entity::Edge, t)] {
        let id = e.create_index(
            crate::catalog::DEFAULT_GRAPH_ID,
            SchemaKind::Range,
            entity,
            token,
            &[p],
        )?;
        e.wait_index(id)?;
    }
    Ok(())
}

/// What a snapshot sees of `tree`; `None` if it is dropped.
fn visible(e: &Engine, tree: Tree) -> Option<Model> {
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let mut out = Model::new();
    match tx.scan_prefix(tree, b"", |k, v| {
        out.insert(k.to_vec(), v.to_vec());
        true
    }) {
        Ok(()) => Some(out),
        Err(Error::TreeDropped(_)) => None,
        Err(e) => panic!("visible: {e}"),
    }
}

/// The model of the trees after the batches `done`: the workload's tree,
/// and the side tree's entries or `None` once dropped.
fn tx_model(s: &State, done: &[u32]) -> (Model, Option<Model>) {
    let mut m = s.base.clone();
    for &c in done {
        apply(&mut m, &tx_batch(c));
    }
    let side = s.side.as_ref().unwrap();
    let dropped = done.contains(&TX_DROP);
    (m, (!dropped).then(|| side.1.clone()))
}

/// What snapshots see equals the model after a prefix of the attempted
/// batches that includes every acknowledged one.
fn assert_tx_prefix(e: &Engine, s: &State, what: &str) {
    let got = (
        visible(e, s.tree).expect("the workload's tree"),
        visible(e, s.side.as_ref().unwrap().0),
    );
    let graphs = graph_visible(e);
    let rows = s.rows.as_ref().unwrap();
    let ok = (s.acked..=s.attempted.len()).any(|i| {
        got == tx_model(s, &s.attempted[..i]) && graphs == graph_model(rows, &s.attempted[..i])
    });
    assert!(
        ok,
        "{what}: the visible state is not a prefix with every returned commit"
    );
}

/// [`Workload::Transactions`]: [`TX_BATCHES`] transactions, the index
/// builds after [`TX_INDEX`], and a garbage-collection pass made durable
/// after every sixth batch. The oracle of [`run`]: a usable failure (ENOSPC
/// of a reservation, a refused memory reservation) fails only that
/// transaction, build or pass, and leaves what snapshots see equal to the
/// returned commits; any other failure makes the database unusable.
fn run_transactions(s: &mut State, arm: Arm) {
    let armed = !matches!(arm, Arm::Nothing);
    let reservation = matches!(
        arm,
        Arm::Fail(Kind::Reserve | Kind::Create, _, Fault::NoSpace)
    );
    let e = s.engine.as_ref().unwrap();
    let side = s.side.as_ref().unwrap().0;
    let rows = s.rows.as_ref().unwrap();
    let usable = |err: &Error| {
        (err.is_no_space() && (reservation || e.check().is_ok()))
            || matches!(err, Error::MemoryLimit { .. })
    };
    for c in 1..=TX_BATCHES {
        s.attempted.push(c);
        match tx_commit(e, s.tree, side, rows, c) {
            Ok(()) => s.acked = s.attempted.len(),
            Err(err) if usable(&err) => {
                s.attempted.pop();
                e.check().unwrap();
                let want = tx_model(s, &s.attempted);
                let got = (visible(e, s.tree).unwrap(), visible(e, side));
                assert!(
                    got == want && graph_visible(e) == graph_model(rows, &s.attempted),
                    "the failed transaction {c} ({err}) left a trace"
                );
            }
            Err(err) => {
                assert!(armed, "unexpected failure: {err}");
                assert!(e.check().is_err(), "{err} left the database usable");
                return;
            }
        }
        if c == TX_INDEX {
            match tx_indexes(e) {
                Ok(()) => s.indexed = true,
                // The index stays POPULATING, or was not created.
                Err(err) if usable(&err) => e.check().unwrap(),
                Err(err) => {
                    assert!(armed, "unexpected failure: {err}");
                    assert!(e.check().is_err(), "{err} left the database usable");
                    return;
                }
            }
        }
        if c == TX_GRAPH + 1 {
            // A checkpoint whose statistics batch holds graph `g`'s counts
            // (TS-11).
            if let Err(err) = e.checkpoint() {
                assert!(armed, "unexpected failure: {err}");
                assert!(
                    e.check().is_err(),
                    "a failed checkpoint left the database usable"
                );
                return;
            }
        }
        if c % 6 == 0 {
            let r = crate::tx::gc::pass(e.core())
                .and_then(|p| p.group.map_or(Ok(()), |g| e.wal().wait_durable(g)));
            match r {
                Ok(()) => {}
                // Undone; the work stays queued.
                Err(err) if usable(&err) => e.check().unwrap(),
                Err(err) => {
                    assert!(armed, "unexpected failure: {err}");
                    assert!(e.check().is_err(), "{err} left the database usable");
                    return;
                }
            }
        }
    }
}

const NOSPACE: [(Kind, Fault); 3] = [
    (Kind::Reserve, Fault::NoSpace),
    (Kind::Create, Fault::NoSpace),
    (Kind::WriteAt, Fault::NoSpace),
];

#[test]
fn commits() {
    let mut f: Vec<(Kind, Fault)> = Kind::ALL.iter().map(|&k| (k, Fault::Eio)).collect();
    f.extend(NOSPACE);
    sweep(Workload::Commits, &f, true);
}

/// Audit Round A: the Sprint 2 transaction paths under every crash point
/// and failure.
#[test]
fn transactions() {
    let mut f: Vec<(Kind, Fault)> = Kind::ALL.iter().map(|&k| (k, Fault::Eio)).collect();
    f.extend(NOSPACE);
    sweep(Workload::Transactions, &f, true);
}

#[test]
fn bulk_load() {
    let mut f = NOSPACE.to_vec();
    f.extend([(Kind::SyncData, Fault::Eio), (Kind::WriteAt, Fault::Eio)]);
    sweep(Workload::BulkLoad, &f, false);
}

#[test]
fn checkpoint() {
    sweep(
        Workload::Checkpoint,
        &[(Kind::SyncData, Fault::Eio), (Kind::WriteAt, Fault::Eio)],
        false,
    );
}

#[test]
fn recovery_checkpoint() {
    sweep(
        Workload::RecoveryCheckpoint,
        &[
            (Kind::SyncData, Fault::Eio),
            (Kind::Reserve, Fault::NoSpace),
            (Kind::Create, Fault::NoSpace),
        ],
        false,
    );
}

#[test]
fn backup() {
    let mut f = NOSPACE.to_vec();
    f.push((Kind::ReadAt, Fault::Eio));
    sweep(Workload::Backup, &f, false);
}

#[test]
fn restore_() {
    sweep(Workload::Restore, &NOSPACE, false);
}

#[test]
fn migration() {
    let mut f = NOSPACE.to_vec();
    f.push((Kind::SyncData, Fault::Eio));
    sweep(Workload::Migration, &f, false);
}

/// Audit finding 4 (2026-10-03): the oracle sees an undone batch that came
/// back although a later batch rewrote its keys. Here batches 1, 2 and 3
/// all apply, while the history says batch 2 was undone and never
/// attempted again: the oracle must refuse that state, and accept it for
/// the true history. Without the marker key of each batch, batch 3
/// rewrites every key batch 2 wrote and the two histories are identical.
#[test]
fn the_oracle_sees_an_undone_batch_that_came_back() {
    let r = Run::new(crate::test_support::graph_base());
    let (e, tree, base) = r.create();
    for c in 1..=3 {
        commit(&e, tree, &batch(c)).unwrap();
    }
    let history = |attempted: Vec<u32>| State {
        engine: None,
        tree,
        base: base.clone(),
        acked: attempted.len(),
        attempted,
        loaded: false,
        side: None,
        rows: None,
        indexed: false,
    };
    let undone = history(vec![1, 3]);
    let seen = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_prefix(&e, &undone, "batch 2 came back");
    }))
    .is_err();
    assert!(
        seen,
        "the oracle accepted a state holding the undone batch 2"
    );
    assert_prefix(&e, &history(vec![1, 2, 3]), "true history");
}

/// R3: a checkpoint syncs the data file before it writes `CONTROL`, and
/// writes and syncs `CONTROL` before it renames or removes any segment,
/// each rename or removal followed by its own `sync_dir` (TS-11; Turso
/// fixed two checkpoint-order bugs of this kind, R-TURSO `CHANGELOG.md:1977,
/// 2006`).
#[test]
fn checkpoint_call_order() {
    let base = crate::test_support::graph_base();
    let r = Run::new(base);
    let s = setup(Workload::Checkpoint, &r);
    let e = s.engine.as_ref().unwrap();
    let from = r.fault.calls().len();
    let stats = e.checkpoint().unwrap();
    assert!(
        stats.recycled + stats.removed > 0,
        "precondition: a segment is recycled"
    );
    let calls = r.fault.calls()[from..].to_vec();
    let data = r.db.path().join(data_name(1));
    let control = r.db.path().join(CONTROL);
    let pos = |f: &dyn Fn(&Call) -> bool| calls.iter().position(f);
    let data_sync = pos(&|c| matches!(c, Call::SyncData(p) if *p == data)).unwrap();
    let control_write =
        pos(&|c| matches!(c, Call::WriteAt { path, .. } if *path == control)).unwrap();
    let control_sync = pos(&|c| matches!(c, Call::SyncData(p) if *p == control)).unwrap();
    let is_seg = |p: &Path| p.file_name().and_then(parse_segment_name).is_some();
    let first_recycle = pos(&|c| {
        matches!(c, Call::Rename(a, _) if is_seg(a)) || matches!(c, Call::Remove(p) if is_seg(p))
    })
    .unwrap();
    assert!(
        data_sync < control_write && control_write < control_sync && control_sync < first_recycle
    );
    for (i, c) in calls.iter().enumerate().skip(first_recycle) {
        if matches!(c, Call::Rename(..) | Call::Remove(_)) {
            assert!(
                matches!(calls.get(i + 1), Some(Call::SyncDir(_))),
                "a recycle step without its own sync_dir: {:?}",
                &calls[i..]
            );
        }
    }
    let late_data_write = calls[control_write..]
        .iter()
        .any(|c| matches!(c, Call::WriteAt { path, .. } if *path == data));
    assert!(!late_data_write, "a data page written after CONTROL");
}

/// Builds a database with `n` acknowledged commits in `dir` and closes it
/// without a checkpoint (drop), so its WAL holds them.
fn unclosed(dir: &Path, n: u32) -> (Tree, Model) {
    let e = Engine::create_tuned(
        dir,
        crate::test_support::no_flush(),
        CAP,
        crate::test_support::OWNER,
        sink(),
        tuning(),
    )
    .unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut m = Model::new();
    for c in 1..=n {
        let b = batch(c);
        commit(&e, tree, &b).unwrap();
        apply(&mut m, &b);
    }
    drop(e);
    (tree, m)
}

/// Flips bit 3 of the byte at `at` in `path`.
fn flip(path: &Path, at: u64) {
    let f = FsStorage.open(path).unwrap();
    let mut b = [0u8; 1];
    f.read_at(&mut b, at).unwrap();
    f.write_at(&[b[0] ^ 8], at).unwrap();
}

fn segment(dir: &Path) -> PathBuf {
    let name = FsStorage
        .list(dir)
        .unwrap()
        .into_iter()
        .find(|n| parse_segment_name(n).is_some())
        .unwrap();
    dir.join(name)
}

fn open(dir: &Path) -> Result<Engine, Error> {
    Engine::open_tuned(dir, crate::test_support::no_flush(), CAP, sink(), tuning())
}

/// The groups of the WAL segment `path`, as `(first LSN, end LSN)`, from
/// LSN 0 to the first frame that is not valid.
fn groups(path: &Path) -> Vec<(u64, u64)> {
    use crate::storage::wal::{FRAME_BYTES, FRAME_SIZE, KIND_COMMIT, commit_record, decode_frame};
    let f = FsStorage.open(path).unwrap();
    let mut out = Vec::new();
    let mut buf = vec![0u8; FRAME_SIZE];
    let mut lsn = 0;
    loop {
        if f.read_at(&mut buf, lsn).unwrap() < FRAME_SIZE {
            return out;
        }
        let Some(h) = decode_frame(&buf, lsn) else {
            return out;
        };
        if h.kind == KIND_COMMIT {
            out.push((commit_record(&buf).first_lsn, lsn + FRAME_BYTES));
        }
        lsn += FRAME_BYTES;
    }
}

/// rmp #20 corruption cases: a flipped bit in a WAL frame that a later
/// valid commit frame follows, in both `CONTROL` slots, or in the format
/// version of page 0 fails the open as corruption; a flip in one `CONTROL`
/// slot opens from the other (TS-05); a flip inside the last synced group
/// discards that group as a torn tail (TS-13, user decision 2026-10-02).
#[test]
fn corruption_fails_the_open_or_is_handled_as_recorded() {
    use crate::storage::wal::FRAME_HEADER;
    let base = crate::test_support::graph_base();
    let payload = |lsn: u64| lsn + FRAME_HEADER as u64 + 100;
    type Damage<'a> = &'a dyn Fn(&Path);
    let cases: [(&str, Damage<'_>); 3] = [
        ("WAL frame before a valid group", &|d| {
            let seg = segment(d);
            let g = groups(&seg);
            assert_eq!(g.len(), 5, "create_trees and four batches");
            flip(&seg, payload(g[2].0));
        }),
        ("both CONTROL slots", &|d| {
            flip(&d.join(CONTROL), 60);
            flip(&d.join(CONTROL), 4096 + 60);
        }),
        ("format version of page 0", &|d| {
            flip(&d.join(data_name(1)), 32)
        }),
    ];
    for (what, damage) in cases {
        let dir = tempfile::tempdir_in(base).unwrap();
        unclosed(dir.path(), 4);
        if what.starts_with("format") {
            // Page 0 is on disk after a clean close.
            open(dir.path()).unwrap().close().unwrap();
        }
        damage(dir.path());
        let r = open(dir.path());
        assert!(
            matches!(r, Err(Error::Corruption(_))),
            "{what}: {:?}",
            r.err()
        );
    }
    // One CONTROL slot: opens from the other.
    let dir = tempfile::tempdir_in(base).unwrap();
    let (tree, m) = unclosed(dir.path(), 4);
    flip(&dir.path().join(CONTROL), 60);
    let e = open(dir.path()).unwrap();
    assert!(entries(&e, tree) == m);
    drop(e);
    // The last synced group: discarded as a torn tail.
    let dir = tempfile::tempdir_in(base).unwrap();
    let (tree, _) = unclosed(dir.path(), 4);
    let mut m3 = Model::new();
    for c in 1..=3 {
        apply(&mut m3, &batch(c));
    }
    let seg = segment(dir.path());
    let last = *groups(&seg).last().unwrap();
    flip(&seg, payload(last.0));
    let e = open(dir.path()).unwrap();
    assert!(
        entries(&e, tree) == m3,
        "the damaged last group was not discarded"
    );
    assert_intact(&e, "torn tail");
}

/// rmp #20 corruption case: a flipped bit in a data page read while the
/// database is open fails that read, and every later operation fails until
/// the database is reopened (FR-REC-005).
#[test]
fn a_corrupt_data_page_read_makes_the_database_unusable_until_reopened() {
    let base = crate::test_support::graph_base();
    let dir = tempfile::tempdir_in(base).unwrap();
    let (tree, m) = unclosed(dir.path(), 2);
    open(dir.path()).unwrap().close().unwrap();
    // A small cap keeps only some pages in the pool: open, find a leaf,
    // close, damage it, open with an empty pool and read it.
    let e = open(dir.path()).unwrap();
    let leaf = {
        let mut p = tree.root();
        loop {
            let pin = e.pool().fetch(p).unwrap();
            let g = pin.read().unwrap();
            if crate::storage::page::page_type(&g) == Some(crate::storage::page::PageType::Leaf) {
                break p;
            }
            p = crate::storage::btree::node::leftmost(&g);
        }
    };
    e.close().unwrap();
    flip(
        &dir.path().join(data_name(1)),
        leaf * crate::storage::page::PAGE_BYTES + 3000,
    );
    let e = open(dir.path()).unwrap();
    let r = e.store().scan_prefix(tree, b"", |_, _| true);
    assert!(matches!(r, Err(Error::Corruption(_))), "{r:?}");
    assert!(matches!(e.check(), Err(Error::Unusable)));
    assert!(matches!(
        e.store().get(tree, &key(1), &mut Vec::new()),
        Err(Error::Unusable)
    ));
    assert!(commit(&e, tree, &batch(9)).is_err());
    drop(e);
    // Repaired by rewriting the page: the reopened database works. (Media
    // failure is repaired by restoring a backup, FR-REC-004.)
    flip(
        &dir.path().join(data_name(1)),
        leaf * crate::storage::page::PAGE_BYTES + 3000,
    );
    let e = open(dir.path()).unwrap();
    assert!(entries(&e, tree) == m);
}

/// The incomplete-artefact rule of the backup sweep: a backup that stopped
/// after `MANIFEST.tmp` but before its rename is refused.
#[test]
fn a_backup_with_only_manifest_tmp_is_refused() {
    let base = crate::test_support::graph_base();
    let r = Run::new(base);
    let s = setup(Workload::Backup, &r);
    s.engine.as_ref().unwrap().backup(r.target.path()).unwrap();
    FsStorage
        .rename(
            &r.target.path().join(MANIFEST),
            &r.target.path().join(MANIFEST_TMP),
        )
        .unwrap();
    let back = restore(r.target.path(), r.restored.path(), r.storage(), CAP, sink());
    assert!(
        matches!(back, Err(Error::IncompleteBackup(_))),
        "{:?}",
        back.err()
    );
}
