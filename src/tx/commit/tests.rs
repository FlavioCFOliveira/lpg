use crate::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::{Arc, Mutex};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use super::*;
use crate::obs::recording::RecordingSink;
use crate::storage::FsStorage;
use crate::storage::btree::Tree;
use crate::storage::datafile::Header;
use crate::storage::error::Error;
use crate::storage::fault::{Call, FaultStorage};
use crate::storage::freelist::FreeState;
use crate::storage::wal::tests::DurableScan;
use crate::storage::wal::{FRAME_SIZE, SEGMENT_SPAN, parse_segment_name};
use crate::test_support::test_dirs;

/// Memory cap of these tests: 32 MiB.
const CAP: u64 = 32 << 20;

pub(crate) type Model = BTreeMap<Vec<u8>, Vec<u8>>;

/// A `FaultStorage` over the file system. It models durability itself:
/// what `sync_data` and `sync_dir` make durable survives its `crash`, and
/// nothing else. No test stops the operating system, so the device flush
/// behind each sync adds nothing to what the tests show; it is skipped,
/// because with it the suite's run time is the device's flush latency under
/// the suite's own writes (measured 2026-10-03: the slowest test 48-62 s
/// over three runs with it, see the task log).
pub(crate) fn fault() -> Arc<FaultStorage> {
    let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    f.skip_device_sync();
    f
}

pub(crate) fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

pub(crate) fn put<'a>(tree: Tree, k: &'a [u8], v: &'a [u8]) -> Write<'a> {
    Write {
        tree,
        key: k,
        value: Some(v),
        stamp: 0,
    }
}

pub(crate) fn del(tree: Tree, k: &[u8]) -> Write<'_> {
    Write {
        tree,
        key: k,
        value: None,
        stamp: 0,
    }
}

/// Every entry of `tree`.
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

/// The integrity check passes and `tree` holds exactly `model`.
pub(crate) fn assert_state(e: &Engine, tree: Tree, model: &Model) {
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    assert!(entries(e, tree) == *model, "tree differs from the model");
}

/// The state a failed commit must leave unchanged (TS-16 step 6 undo).
#[derive(Debug, PartialEq, Eq)]
struct State {
    free: FreeState,
    next_tree_id: u64,
    page_count: u64,
    header: Header,
    counters: Counters,
    group_end: u64,
    entries: Model,
}

fn state(e: &Engine, tree: Tree) -> State {
    let header = {
        let pin = e.pool().fetch(0).unwrap();
        Header::decode(&pin.read().unwrap()).unwrap()
    };
    let st = e.store().lock();
    let s = State {
        free: st.free_list().state(),
        next_tree_id: st.next_tree_id(),
        page_count: e.data().page_count(),
        header,
        counters: st.log_ref().counters,
        group_end: st.log_ref().group.end(),
        entries: Model::new(),
    };
    drop(st);
    State {
        entries: entries(e, tree),
        ..s
    }
}

fn key(t: usize, i: usize, j: usize) -> Vec<u8> {
    format!("k{t:02}-{i:05}-{j}").into_bytes()
}

/// In the `FaultStorage` call log, a WAL group write (a `write_at` whose
/// length is a whole number of frames, unlike the 1 MiB zero writes of a
/// new segment) starts only when every segment an earlier group wrote has
/// been synced, unless it continues the same group in the next segment.
fn assert_group_order(calls: &[Call]) -> u64 {
    let is_wal = |p: &Path| p.file_name().and_then(parse_segment_name).is_some();
    let mut unsynced: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut last_end: Option<u64> = None;
    let mut groups = 0;
    for c in calls {
        match c {
            Call::WriteAt { path, offset, len }
                if is_wal(path) && *len % FRAME_SIZE as u64 == 0 =>
            {
                let continuation = *offset == 0 && last_end == Some(SEGMENT_SPAN);
                assert!(
                    unsynced.is_empty() || continuation,
                    "a group was written at {path:?}+{offset} before the previous one was synced"
                );
                if !continuation {
                    groups += 1;
                }
                unsynced.insert(path.clone());
                last_end = Some(offset + len);
            }
            Call::SyncData(path) if is_wal(path) => {
                unsynced.remove(path);
            }
            _ => {}
        }
    }
    groups
}

/// FR-TX-002, TS-09: every commit returns only after a `sync_data` that
/// covers its commit frame (its group is in the wrapped storage, which
/// `FaultStorage` writes only in `sync_data`), alone and with 16 threads;
/// group k+1 is never written before group k's `sync_data` returned; and
/// commit timestamps have no gap.
#[test]
fn every_commit_returns_after_a_sync_covering_its_commit_frame() {
    for dir in test_dirs() {
        let d = dir.path();
        let fault = fault();
        let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let scan = Mutex::new(DurableScan::new(d, 1, 0));
        let mut stamps = vec![1];
        for i in 0..20 {
            let k = key(99, i, 0);
            let c = e.commit(&[put(tree, &k, b"v")], Ids::default()).unwrap();
            assert!(
                scan.lock().unwrap().holds(c.commit_ts),
                "commit {} not durable",
                c.commit_ts
            );
            stamps.push(c.commit_ts);
        }
        let all = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for t in 0..16 {
                let (e, scan, all) = (&e, &scan, &all);
                s.spawn(move || {
                    for i in 0..25 {
                        let k = key(t, i, 0);
                        let c = e.commit(&[put(tree, &k, b"v")], Ids::default()).unwrap();
                        assert!(
                            scan.lock().unwrap().holds(c.commit_ts),
                            "commit {} returned before its group was durable",
                            c.commit_ts
                        );
                        all.lock().unwrap().push(c.commit_ts);
                    }
                });
            }
        });
        stamps.extend(all.into_inner().unwrap());
        stamps.sort_unstable();
        assert_eq!(
            stamps,
            (1..=421).collect::<Vec<u64>>(),
            "timestamps 1..=421, no gap"
        );
        let groups = assert_group_order(&fault.calls());
        let scan = scan.into_inner().unwrap();
        assert_eq!(groups, scan.groups.len() as u64);
        assert!(
            groups < 421,
            "16 committers shared no group: {groups} groups for 421 commits"
        );
    }
}

/// FR-REC-006: a failed `sync_data` of the WAL fails the commit, and every
/// later operation fails until the database is reopened; after a crash the
/// failed commit is absent and the earlier ones present.
#[test]
fn failed_wal_sync_fails_the_commit_and_every_later_operation_until_reopen() {
    for dir in test_dirs() {
        let d = dir.path();
        let fault = fault();
        let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        for i in 0..5 {
            let k = key(0, i, 0);
            e.commit(&[put(tree, &k, b"v")], Ids::default()).unwrap();
            model.insert(k, b"v".to_vec());
        }
        let syncs = fault
            .calls()
            .iter()
            .filter(|c| matches!(c, Call::SyncData(_)))
            .count() as u64;
        fault.fail_sync_data(syncs + 1);
        let r = e.commit(&[put(tree, b"lost", b"v")], Ids::default());
        assert!(matches!(r, Err(Error::Io(_))), "{r:?}");
        let r = e.commit(&[put(tree, b"later", b"v")], Ids::default());
        assert!(matches!(r, Err(Error::Unusable)), "{r:?}");
        assert!(matches!(e.check(), Err(Error::Unusable)));
        let mut out = Vec::new();
        assert!(matches!(
            e.store().get(tree, &key(0, 0, 0), &mut out),
            Err(Error::Unusable)
        ));
        assert!(matches!(e.checkpoint(), Err(Error::Unusable)));
        drop(e);
        fault.crash(crate::storage::fault::Crash::DropAll).unwrap();
        let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
    }
}

/// FR-REC-007, TS-09: ENOSPC while creating a WAL segment fails only the
/// commit that needed it; a concurrent reader keeps reading, the state is
/// the one before that commit, and once space is back the next commit
/// succeeds.
#[test]
fn enospc_creating_a_segment_fails_that_commit_while_a_concurrent_read_succeeds() {
    for dir in test_dirs() {
        let d = dir.path();
        let fault = fault();
        let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        e.commit(&[put(tree, b"stable", b"v")], Ids::default())
            .unwrap();
        let mut model = Model::from([(b"stable".to_vec(), b"v".to_vec())]);
        fault.set_space(0);
        let stop = AtomicBool::new(false);
        let reads = AtomicU64::new(0);
        let value = vec![7u8; 1 << 20];
        let err = std::thread::scope(|s| {
            s.spawn(|| {
                let mut out = Vec::new();
                while !stop.load(Ordering::Acquire) {
                    assert!(e.store().get(tree, b"stable", &mut out).unwrap());
                    reads.fetch_add(1, Ordering::Relaxed);
                }
            });
            let mut i = 0;
            let err = loop {
                let k = key(0, i, 0);
                match e.commit(&[put(tree, &k, &value)], Ids::default()) {
                    Ok(_) => {
                        model.insert(k, value.clone());
                    }
                    Err(err) => break err,
                }
                i += 1;
                assert!(i < 100, "no segment was ever needed");
            };
            // The reader keeps going after the failure.
            let seen = reads.load(Ordering::Relaxed);
            while reads.load(Ordering::Relaxed) < seen + 100 {
                std::thread::yield_now();
            }
            stop.store(true, Ordering::Release);
            err
        });
        assert!(err.is_no_space(), "{err}");
        assert!(reads.load(Ordering::Relaxed) > 0);
        e.check().unwrap();
        assert_state(&e, tree, &model);
        assert_eq!(e.wal().segment_lsns(), [0], "no segment was created");
        fault.set_space(u64::MAX);
        let k = key(1, 0, 0);
        e.commit(&[put(tree, &k, &value)], Ids::default()).unwrap();
        model.insert(k, value.clone());
        assert_state(&e, tree, &model);
        drop(e);
        let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
    }
}

/// 16 threads committing disjoint batches leave every batch applied exactly
/// once, before and after a reopen.
#[test]
fn sixteen_threads_committing_disjoint_batches_apply_each_exactly_once() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = Engine::create(d, fault(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        std::thread::scope(|s| {
            for t in 0..16 {
                let e = &e;
                s.spawn(move || {
                    for i in 0..40 {
                        let ks: Vec<Vec<u8>> = (0..4).map(|j| key(t, i, j)).collect();
                        let counter = format!("counter-{t:02}").into_bytes();
                        let n = (i as u64).to_le_bytes();
                        let mut batch: Vec<Write<'_>> =
                            ks.iter().map(|k| put(tree, k, k)).collect();
                        batch.push(put(tree, &counter, &n));
                        // A delete of an absent key changes nothing.
                        batch.push(del(tree, b"never-present"));
                        e.commit(&batch, Ids::default()).unwrap();
                    }
                });
            }
        });
        let mut model = Model::new();
        for t in 0..16 {
            for i in 0..40 {
                for j in 0..4 {
                    let k = key(t, i, j);
                    model.insert(k.clone(), k);
                }
            }
            model.insert(
                format!("counter-{t:02}").into_bytes(),
                39u64.to_le_bytes().to_vec(),
            );
        }
        assert_state(&e, tree, &model);
        assert_eq!(e.store().lock().log_ref().counters.commit_ts, 1 + 16 * 40);
        drop(e);
        let e = Engine::open(d, fault(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
    }
}

/// Fills the data file's reserve with committed values until fewer than
/// `left` unused pages remain. Step 2 is skipped while filling: it keeps the
/// reserve at or above each commit's estimate (about 2 050 pages for a
/// 4 MiB value), so with it the reserve would never drop below `left`. Each
/// value takes about half of the unused pages above `left`, so no fill
/// commit overruns the reserve.
pub(crate) fn use_up_reserve(e: &Engine, tree: Tree, model: &mut Model, left: u64) {
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let mut i = 0;
    loop {
        let unused = e.data().unused_pages();
        if unused < left {
            break;
        }
        let n = ((unused - left + 1) * 4096).min(4 << 20) as usize;
        let value = vec![3u8; n];
        let k = format!("fill-{i}").into_bytes();
        e.commit(&[put(tree, &k, &value)], Ids::default()).unwrap();
        model.insert(k, value);
        i += 1;
    }
    e.core().skip_reserve.store(false, Ordering::Relaxed);
}

/// Option A (TS-16 step 6, user decision 2026-10-02): an overrun of the
/// reserve meets ENOSPC under the commit latch; the batch, which had
/// already deleted entries, freed overflow pages, taken free pages and
/// changed leaves, is undone: the tree, the free list, page 0, the page
/// count and the counters equal the pre-commit state, the database stays
/// usable and the next commit gets the timestamp the failed one drew.
#[test]
fn enospc_in_the_apply_step_is_undone_and_the_database_stays_usable() {
    for dir in test_dirs() {
        let d = dir.path();
        let fault = fault();
        let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        let medium = vec![5u8; 100_000];
        for i in 0..10 {
            let k = format!("ovf-{i}").into_bytes();
            e.commit(&[put(tree, &k, &medium)], Ids::default()).unwrap();
            model.insert(k, medium.clone());
        }
        use_up_reserve(&e, tree, &mut model, 600);
        // Free pages, so that the failing batch also takes free pages.
        let gone: Vec<Vec<u8>> = (0..4).map(|i| format!("ovf-{i}").into_bytes()).collect();
        let batch: Vec<Write<'_>> = gone.iter().map(|k| del(tree, k)).collect();
        e.commit(&batch, Ids::default()).unwrap();
        for k in &gone {
            model.remove(k);
        }
        assert_state(&e, tree, &model);
        assert!(e.store().lock().free_list().pages().count() > 40);
        let before = state(&e, tree);
        fault.set_space(0);
        e.core().skip_reserve.store(true, Ordering::Relaxed);
        let big = vec![9u8; 8 << 20];
        let r = e.commit(
            &[
                del(tree, b"ovf-5"),
                put(tree, b"a", b"1"),
                put(tree, b"ovf-6", b"short now"),
                put(tree, b"big", &big),
            ],
            Ids {
                next_element_id: 77,
                ..Ids::default()
            },
        );
        assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
        e.check().unwrap();
        assert_eq!(state(&e, tree), before, "the failed batch left a trace");
        assert_state(&e, tree, &model);
        e.core().skip_reserve.store(false, Ordering::Relaxed);
        fault.set_space(u64::MAX);
        let c = e
            .commit(&[put(tree, b"after", b"1")], Ids::default())
            .unwrap();
        assert_eq!(c.commit_ts, before.counters.commit_ts + 1);
        model.insert(b"after".to_vec(), b"1".to_vec());
        assert_state(&e, tree, &model);
        drop(e);
        let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
    }
}

/// TS-16 step 6 Overrun: when the reserve runs out during the apply and
/// space is available, the committer extends the file under the latch and
/// the commit succeeds.
#[test]
fn an_overrun_extends_the_file_under_the_latch() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = Engine::create(d, fault(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        use_up_reserve(&e, tree, &mut model, 600);
        let reserved = e.data().reserved_pages();
        e.core().skip_reserve.store(true, Ordering::Relaxed);
        let big = vec![9u8; 8 << 20];
        e.commit(&[put(tree, b"big", &big)], Ids::default())
            .unwrap();
        model.insert(b"big".to_vec(), big);
        assert!(e.data().reserved_pages() > reserved, "the file grew");
        assert_state(&e, tree, &model);
    }
}

/// Option A: a refused memory reservation in the apply step (a page copy
/// or a cache slot) undoes the batch and leaves the database usable; the
/// next commit succeeds once memory is free.
#[test]
fn a_refused_memory_reservation_in_the_apply_step_is_undone() {
    for dir in test_dirs() {
        let d = dir.path();
        let cap = 4 << 20;
        let e = Engine::create(d, fault(), cap, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        // About 60 leaves.
        for i in 0..20 {
            let batch_keys: Vec<Vec<u8>> = (0..200).map(|j| key(0, i, j)).collect();
            let v = [1u8; 100];
            let batch: Vec<Write<'_>> = batch_keys.iter().map(|k| put(tree, k, &v)).collect();
            e.commit(&batch, Ids::default()).unwrap();
            for k in batch_keys {
                model.insert(k, v.to_vec());
            }
        }
        let before = state(&e, tree);
        // Hold every byte the pool can give up, then free six pages.
        let mut hog = Vec::new();
        while let Ok(r) = e.pool().reserve(8192) {
            hog.push(r);
        }
        hog.truncate(hog.len() - 6);
        let v = [2u8; 100];
        let ks: Vec<Vec<u8>> = (0..20).map(|i| key(0, i, 100)).collect();
        let batch: Vec<Write<'_>> = ks.iter().map(|k| put(tree, k, &v)).collect();
        let r = e.commit(&batch, Ids::default());
        assert!(matches!(r, Err(Error::MemoryLimit { .. })), "{r:?}");
        e.check().unwrap();
        drop(hog);
        assert_eq!(state(&e, tree), before);
        assert_state(&e, tree, &model);
        let c = e.commit(&batch, Ids::default()).unwrap();
        assert_eq!(c.commit_ts, before.counters.commit_ts + 1);
        for k in ks {
            model.insert(k, v.to_vec());
        }
        assert_state(&e, tree, &model);
    }
}

/// TS-16 step 6 undo, item 3: a page an undone batch had taken is reused
/// only after every tree read operation that started before the undo has
/// ended.
#[test]
fn a_page_taken_by_an_undone_batch_waits_for_earlier_readers() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    e.commit(&[put(tree, b"a", b"1")], Ids::default()).unwrap();
    let mut model = Model::from([(b"a".to_vec(), b"1".to_vec())]);
    use_up_reserve(&e, tree, &mut model, 600);
    let store = e.store();
    let reader = store.reader();
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let big = vec![9u8; 8 << 20];
    let r = e.commit(&[put(tree, b"big", &big)], Ids::default());
    assert!(r.is_err_and(|e| e.is_no_space()));
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    let done = AtomicBool::new(false);
    let waits = &e.core().trees.fence_waits;
    let before = waits.load(Ordering::SeqCst);
    std::thread::scope(|s| {
        let h = s.spawn(|| {
            let v = vec![1u8; 100_000];
            e.commit(&[put(tree, b"next", &v)], Ids::default()).unwrap();
            done.store(true, Ordering::Release);
        });
        // The commit either waits at the fence, as it must, or finishes
        // without waiting, which reuses a page under the reader.
        while waits.load(Ordering::SeqCst) == before {
            assert!(
                !done.load(Ordering::Acquire),
                "a page the undone batch took was reused under a reader"
            );
            std::thread::yield_now();
        }
        assert!(!done.load(Ordering::Acquire));
        drop(reader);
        h.join().unwrap();
    });
    assert!(done.load(Ordering::Acquire));
    e.check().unwrap();
}

/// Counters raised by commits are kept by the next open from the commit
/// frames (TS-13 step 6); a failed commit raises none.
#[test]
fn commits_raise_counters_and_failed_commits_do_not() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let e = Engine::create(d, fault(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let ids = Ids {
        next_element_id: 10,
        next_catalog_id: 20,
        next_token_id: 30,
    };
    e.commit(&[put(tree, b"a", b"1")], ids).unwrap();
    e.commit(&[put(tree, b"b", b"1")], Ids::default()).unwrap();
    let long = vec![0u8; 1025];
    let r = e.commit(
        &[put(tree, &long, b"1")],
        Ids {
            next_element_id: 99,
            ..ids
        },
    );
    assert!(matches!(r, Err(Error::InvalidArgument(_))));
    let c = e.store().lock().log_ref().counters;
    assert_eq!(
        c,
        Counters {
            commit_ts: 3,
            next_element_id: 10,
            next_catalog_id: 20,
            next_token_id: 30
        }
    );
}

// ---------------------------------------------------------------------------
// AUDIT tests (storage-engine-auditor, 2026-10-02). Each one demonstrates a
// defect reported in the audit of tasks #15 and #16; it fails until the
// defect is fixed.

/// Joins a batch to the open group under the commit latch `st`, as TS-16
/// step 7 does, without step 8: the group stays open and is not durable,
/// as it is while the committer has not yet been served by the writer.
pub(crate) fn audit_join(st: &mut crate::storage::btree::WriteState) -> u64 {
    let log = st.log();
    let ts = log.counters.commit_ts + 1;
    log.counters.commit_ts = ts;
    let counters = log.counters;
    log.group.join(ts, counters);
    log.group.seq()
}

/// AUDIT A (WAL-before-data across the TS-16 step-6 undo). Commit c frees
/// the overflow pages of "big"; its group is joined but not yet durable.
/// Batch d takes those pages from the free list, overruns the reserve,
/// meets ENOSPC and is undone. The undo gives each taken page page LSN 0
/// (`write.rs` `undo`, `None if logged => set_lsn(0)`), so eviction or a
/// checkpoint pass may write d's content over a page whose last committed
/// image is not in the WAL after the redo point. A crash before c's group
/// is durable (c never returned) then corrupts the committed value "big".
#[test]
fn audit_undo_of_a_page_freed_by_an_undurable_commit_keeps_wal_before_data() {
    use crate::storage::fault::Crash;
    use crate::storage::page;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    let medium = vec![5u8; 100_000];
    e.commit(&[put(tree, b"big", &medium)], Ids::default())
        .unwrap();
    model.insert(b"big".to_vec(), medium.clone());
    use_up_reserve(&e, tree, &mut model, 600);
    e.checkpoint().unwrap();
    // c: delete "big", join, no step 8.
    let freed: Vec<u64> = {
        let store = e.store();
        let mut st = store.lock();
        let before: BTreeSet<u64> = st.free_list().pages().collect();
        store
            .apply_batch(&mut st, Some(e.wal()), &[del(tree, b"big")], 0)
            .unwrap();
        audit_join(&mut st);
        st.free_list()
            .pages()
            .filter(|p| !before.contains(p))
            .collect()
    };
    assert!(!freed.is_empty(), "c freed the overflow pages of big");
    // d: takes and writes the freed pages, overruns the reserve, ENOSPC,
    // undone.
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    // d1 writes the freed pages (they are taken and created); d2 overruns.
    let d1 = vec![8u8; 100_000];
    let huge = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"d1", &d1), put(tree, b"d2", &huge)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    let durable = e.wal().durable_lsn();
    let mut exposed = Vec::new();
    for &p in &freed {
        if let Some((pin, dirty)) = e.pool().resident(p) {
            let lsn = page::lsn(&pin.read().unwrap());
            if dirty && lsn <= durable {
                exposed.push((p, lsn));
            }
        }
    }
    // Eviction writes every such page (the same rule as this pass), and
    // FR-REC-002 allows the unsynced write to reach the disk before the
    // crash; `sync` picks that outcome deterministically.
    e.pool().write_dirty().unwrap();
    e.data().sync().unwrap();
    drop(e);
    fault.crash(Crash::DropAll).unwrap();
    let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
    let mut out = Vec::new();
    let got = e.store().get(tree, b"big", &mut out);
    let intact = matches!(got, Ok(true)) && out == medium;
    assert!(
        exposed.is_empty() && intact,
        "pages freed by the undurable commit c were writable to data after d's undo: \
         {exposed:?}; after the crash, get(big) = {got:?}, value intact = {intact}"
    );
}

/// AUDIT B (a failed capture leaves its frame positions in the open
/// group). Commit A has joined the open group and woken the writer, which
/// passed its health check and waits for the commit latch to close the
/// group. Batch B holds the latch; while B's `ensure_through` creates the
/// next segment, another thread sets the unusable latch (a reader's
/// checksum failure, a checkpoint I/O error). B's `capture` then fails at
/// `pool.fetch`, and `run` returns without rolling back B's positions
/// (`write.rs` `self.capture(undo, log)?`). The writer closes and writes a
/// group holding B's positions without images. Debug build: the writer
/// panics on the `seal` debug assertion and A waits forever. Release build:
/// the group, with zero-filled frames for B's pages (page 0 among them), is
/// synced, A returns Ok, and recovery replays the zero pages.
#[test]
fn audit_a_failed_capture_never_reaches_the_wal() {
    use crate::storage::wal::segment_name;
    use crate::sync::mpsc;
    let dir = test_dirs().remove(0);
    let d = dir.path().to_path_buf();
    let e =
        Arc::new(Engine::create(&d, crate::test_support::no_flush(), 256 << 20, sink()).unwrap());
    let tree = e.create_trees(1).unwrap()[0];
    let next = d.join(segment_name(1, SEGMENT_SPAN));
    assert!(!next.exists());
    let store = e.store();
    let mut st = store.lock();
    store
        .apply_batch(&mut st, Some(e.wal()), &[put(tree, b"a", b"1")], 0)
        .unwrap();
    let seq = audit_join(&mut st);
    // Pause points (TS-30 R10): the writer reports that it has passed its
    // health check and is about to take the commit latch, which this test
    // holds; the creation of segment 2 by B sets the unusable latch, as a
    // failure on another thread would.
    let (passed, writer_passed) = mpsc::channel();
    *e.wal().points.before_close.lock().unwrap() = Some(Box::new(move || {
        let _ = passed.send(());
    }));
    let (tx, rx) = mpsc::channel();
    {
        let e = Arc::clone(&e);
        std::thread::spawn(move || {
            let _ = tx.send(e.wal().wait_durable(seq).map_err(|x| x.to_string()));
        });
    }
    writer_passed.recv().unwrap();
    let fired = Arc::new(AtomicBool::new(false));
    {
        let (health, fired) = (Arc::clone(&e.core().health), Arc::clone(&fired));
        *e.wal().points.segment_created.lock().unwrap() = Some(Box::new(move || {
            let _ = health.fail(Error::Unusable);
            fired.store(true, Ordering::Release);
        }));
    }
    // About 2 090 overflow pages: the group crosses into segment 2.
    let big = vec![7u8; 17_000_000];
    let r = store.apply_batch(&mut st, Some(e.wal()), &[put(tree, b"b", &big)], 0);
    let fired = fired.load(Ordering::Acquire);
    assert!(next.exists(), "precondition: B created segment 2");
    drop(st);
    assert!(
        fired && r.is_err(),
        "precondition: unusable set during B's segment creation (fired = {fired}, B = {:?})",
        r.err()
    );
    let a = rx.recv_timeout(Duration::from_secs(10));
    assert!(
        a.is_ok(),
        "commit A never returned: the writer died (debug assertion in seal) \
         without failing its waiters"
    );
    // A debug build's writer panics on the `seal` debug assertion when it
    // closes a group holding B's positions without images, and A then
    // fails with the panic: that is the defect, not a refusal.
    if let Ok(Err(m)) = &a {
        assert!(
            !m.contains(crate::storage::wal::WRITER_PANICKED),
            "the writer closed a group holding B's positions: {m}"
        );
    }
    if let Ok(Ok(())) = a {
        // A was acknowledged: it must survive a reopen.
        let e = Arc::into_inner(e).expect("no other handle");
        drop(e);
        let e = Engine::open(&d, crate::test_support::no_flush(), 256 << 20, sink());
        let e = e.expect("reopen after an acknowledged commit");
        let mut out = Vec::new();
        assert!(e.store().get(tree, b"a", &mut out).unwrap());
        assert_eq!(out, b"1");
    }
}

/// AUDIT C (checkpoint step 2 can skip a page whose change was undone).
/// The checkpoint reads the undo epoch, then skips each dirty slot whose
/// page LSN is not durable, then calls `wait_lsn`; its comment says that
/// "an undo after a page was seen ends the wait", so that the skipped
/// pages are visited again. `wait_lsn` tests the durable LSN before the
/// epoch: once a later commit has made the target LSN durable, it returns
/// `true` even though a commit was undone, and `checkpoint::run` breaks
/// without visiting the skipped pages again. A skipped page whose change
/// was undone is back to an older page LSN at or below the redo point,
/// dirty and unwritten, while `CONTROL` records that redo point.
#[test]
fn audit_wait_lsn_reports_an_undo_even_when_the_lsn_is_already_durable() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    e.commit(&[put(tree, b"a", b"1")], Ids::default()).unwrap();
    use_up_reserve(&e, tree, &mut model, 600);
    // A checkpoint reads the epoch, then sees B's page at this LSN.
    let epoch = e.wal().undo_epoch();
    let target = e.wal().durable_lsn() + crate::storage::wal::FRAME_BYTES;
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let huge = vec![9u8; 8 << 20];
    let r = e.commit(&[put(tree, b"b", &huge)], Ids::default());
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    e.commit(&[put(tree, b"c", b"1")], Ids::default()).unwrap();
    assert!(e.wal().durable_lsn() >= target);
    assert_ne!(e.wal().undo_epoch(), epoch, "B was undone");
    assert!(
        !e.wal().wait_lsn(target, epoch).unwrap(),
        "wait_lsn returned true after an undo: checkpoint::run breaks without \
         visiting the skipped pages again"
    );
}

/// Audit M2 (TS-09): `Pool::create` gives a page its zeros and its page
/// LSN in one latched write, so a checkpoint pass never finds the frame
/// dirty with page LSN 0. Here the frame was dirty with a durable page
/// LSN before; after `create` with a page LSN beyond the durable LSN the
/// pass skips it with that page LSN and writes nothing. The interval
/// between two latch acquisitions that the audit found no longer exists,
/// so no interleaving test applies; this test pins the result.
#[test]
fn create_sets_the_page_lsn_in_the_write_that_zeroes_the_frame() {
    use crate::storage::page;
    let dir = test_dirs().remove(0);
    let e = Engine::create(dir.path(), fault(), CAP, sink()).unwrap();
    let durable = e.wal().durable_lsn();
    let q = e.data().page_count();
    {
        let pin = e.pool().create(q, 0).unwrap();
        pin.write().unwrap()[100] = 1;
    }
    let lsn = durable + 5 * crate::storage::wal::FRAME_BYTES;
    let pin = e.pool().create(q, lsn).unwrap();
    let g = pin.read().unwrap();
    assert_eq!(page::lsn(&g), lsn);
    assert!(g[page::HEADER_LEN..].iter().all(|&b| b == 0));
    drop(g);
    let mut skipped = Vec::new();
    let written = e.pool().checkpoint_pass(None, &mut skipped).unwrap();
    assert!(skipped.contains(&(q, lsn)), "{skipped:?}");
    let _ = written;
    drop(pin);
}

/// TS-16 step 6 undo of a taken page that was not in the pool: no commit
/// that freed it is pending, so `undo_keep` writes the batch's image to
/// `data` at once and leaves the frame clean; its page LSN is not lowered.
#[test]
fn an_undone_batch_leaves_the_frames_of_pages_it_took_from_disk_clean() {
    use crate::storage::page;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    use_up_reserve(&e, tree, &mut model, 600);
    let first = e.data().page_count();
    let reserved = e.data().reserved_pages();
    assert!((first..reserved).all(|p| e.pool().resident(p).is_none()));
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    // The first value fits the reserve, so its pages are taken and
    // written; the second overruns it and meets ENOSPC.
    let fits = vec![8u8; 2 << 20];
    let huge = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"a", &fits), put(tree, b"b", &huge)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    let durable = e.wal().durable_lsn();
    let mut taken = 0;
    for p in first..reserved {
        if let Some((pin, dirty)) = e.pool().resident(p) {
            taken += 1;
            assert!(!dirty, "page {p} is dirty after the undo");
            assert!(
                page::lsn(&pin.read().unwrap()) > durable,
                "page {p}: page LSN lowered"
            );
        }
    }
    assert!(taken > 0, "the batch took pages from the reserve");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    e.check().unwrap();
    assert_state(&e, tree, &model);
}

/// AUDIT D (re-audit 2026-10-03; TS-16 step 6 undo, item 3). A reader may
/// have read a link that an undone batch wrote, and fetch the linked page
/// after the undo: readers hold one page latch at a time
/// (`btree/mod.rs`, `get`). The reuse fence keeps such a page from being
/// reused, but the undo leaves the frame of a taken page that was not in
/// the pool clean (`PinnedPage::undo_discard`), so the pool may evict it
/// (TS-08: clean frames first). The page has no valid image on disk: a
/// reserve page was never written. The reader's fetch then fails its
/// checksum and sets the unusable latch, although the failure being undone
/// was ENOSPC, which must leave the database usable (FR-REC-007).
#[test]
fn audit_a_page_taken_by_an_undone_batch_stays_readable_after_eviction() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    use_up_reserve(&e, tree, &mut model, 600);
    let first = e.data().page_count();
    let reserved = e.data().reserved_pages();
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let fits = vec![8u8; 2 << 20];
    let huge = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"a", &fits), put(tree, b"b", &huge)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    let taken: Vec<u64> = (first..reserved)
        .filter(|&p| e.pool().resident(p).is_some())
        .collect();
    assert!(!taken.is_empty(), "the batch took pages from the reserve");
    // Memory pressure: other users of the cap make the pool free frames,
    // clean ones first.
    let mut held = Vec::new();
    while let Ok(r) = e.pool().reserve(1 << 20) {
        held.push(r);
    }
    let evicted: Vec<u64> = taken
        .iter()
        .copied()
        .filter(|&p| e.pool().resident(p).is_none())
        .collect();
    drop(held);
    let p = *evicted
        .first()
        .expect("precondition: a taken page was evicted");
    // The reader that followed the undone batch's link to `p`.
    let fetched = e.pool().fetch(p).map(drop);
    let health = e.check();
    assert!(
        health.is_ok(),
        "an ENOSPC undo left the database unusable once a reader fetched page {p} \
         ({} of {} taken pages evicted): fetch = {fetched:?}, check = {health:?}",
        evicted.len(),
        taken.len()
    );
}

/// D2 (TS-16 steps 3-7): a panic under the commit latch makes the database
/// unusable before the latch is released. The panicking thread had taken a
/// frame position in the open group without capturing its image, and had
/// recorded a join; the WAL writer must not close that group, so the
/// waiter fails, no WAL is written, and every later operation fails.
/// Without the latch, a release build writes the group with the slot's
/// empty image (measured 2026-10-03); a debug build stops earlier, at the
/// `debug_assert` of `ClosedGroup::seal`.
#[test]
fn a_panic_under_the_commit_latch_makes_the_database_unusable() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(dir.path(), fault(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let durable = e.wal().durable_lsn();
    let seq = AtomicU64::new(0);
    let joined = std::thread::scope(|s| {
        s.spawn(|| {
            let mut st = e.store().lock();
            let log = st.log();
            log.group.page_lsn(tree.root());
            let ts = log.counters.commit_ts + 1;
            let counters = log.counters;
            log.group.join(ts, counters);
            seq.store(log.group.seq(), Ordering::Relaxed);
            panic!("a committer panics under the commit latch");
        })
        .join()
    });
    assert!(joined.is_err());
    let seq = seq.load(Ordering::Relaxed);
    assert!(e.check().is_err_and(|e| matches!(e, Error::Unusable)));
    let r = e.wal().wait_durable(seq);
    assert!(r.is_err(), "the writer closed the group: {r:?}");
    assert_eq!(
        e.wal().durable_lsn(),
        durable,
        "WAL written after the panic"
    );
    let r = e.commit(&[put(tree, b"a", b"1")], Ids::default());
    assert!(matches!(r, Err(Error::Unusable)), "{r:?}");
}

/// Evicts pages down to the pool's floor, as memory pressure from other
/// users of the cap does (TS-08: clean frames first).
fn evict_all(e: &Engine) {
    let mut held = Vec::new();
    while let Ok(r) = e.pool().reserve(8192) {
        held.push(r);
    }
}

/// D1, free-list variant (TS-16 step 6 undo, item 3): the undone batch
/// takes free-list pages that are not in the pool. After the undo and an
/// eviction, each such page reads back with the image the batch left in
/// its frame, not with the older image of the free page on disk.
#[test]
fn a_free_list_page_taken_by_an_undone_batch_reads_back_its_undo_image() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    let medium = vec![5u8; 100_000];
    for i in 0..4 {
        let k = format!("ovf-{i}").into_bytes();
        e.commit(&[put(tree, &k, &medium)], Ids::default()).unwrap();
        model.insert(k, medium.clone());
    }
    use_up_reserve(&e, tree, &mut model, 600);
    let gone: Vec<Vec<u8>> = (0..4).map(|i| format!("ovf-{i}").into_bytes()).collect();
    let batch: Vec<Write<'_>> = gone.iter().map(|k| del(tree, k)).collect();
    e.commit(&batch, Ids::default()).unwrap();
    for k in &gone {
        model.remove(k);
    }
    let free: Vec<u64> = e.store().lock().free_list().pages().collect();
    evict_all(&e);
    let cold: Vec<u64> = free
        .into_iter()
        .filter(|&p| e.pool().resident(p).is_none())
        .collect();
    assert!(!cold.is_empty(), "precondition: free pages left the pool");
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    // The first value takes the free pages and fits; the second overruns
    // the reserve. A value's pages are all taken before any is written.
    let fits = vec![8u8; 1 << 20];
    let big = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"fits", &fits), put(tree, b"big", &big)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    let taken: Vec<(u64, Vec<u8>)> = cold
        .iter()
        .filter_map(|&p| {
            let (pin, _) = e.pool().resident(p)?;
            let image = pin.read().unwrap().to_vec();
            Some((p, image))
        })
        .collect();
    assert!(
        !taken.is_empty(),
        "the batch took free pages not in the pool"
    );
    evict_all(&e);
    let mut evicted = 0;
    for (p, image) in &taken {
        if e.pool().resident(*p).is_some() {
            continue;
        }
        evicted += 1;
        let pin = e.pool().fetch(*p).unwrap();
        // Bytes 0..4 hold the checksum, set only on the copy written.
        assert!(
            pin.read().unwrap()[4..] == image[4..],
            "page {p} read back an image other than the undo's"
        );
    }
    assert!(evicted > 0, "precondition: a taken page was evicted");
    assert_state(&e, tree, &model);
}

/// The free list copies an emptied trunk before a batch takes it, also
/// when the trunk is not in the pool: the undo of that batch puts the
/// trunk back into the list, so its image must come back too. Otherwise
/// its frame keeps the batch's bytes, which the integrity check, the next
/// change to the list, or the next open would read as the trunk.
#[test]
fn an_undone_batch_that_took_an_emptied_trunk_leaves_the_free_list_intact() {
    use crate::storage::btree::node::OVERFLOW_CAP;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    use_up_reserve(&e, tree, &mut model, 600);
    let medium = vec![5u8; 100_000];
    let ks: Vec<Vec<u8>> = (0..2).map(|i| format!("ovf-{i}").into_bytes()).collect();
    for k in &ks {
        e.commit(&[put(tree, k, &medium)], Ids::default()).unwrap();
    }
    let batch: Vec<Write<'_>> = ks.iter().map(|k| del(tree, k)).collect();
    e.commit(&batch, Ids::default()).unwrap();
    let (trunk, listed) = {
        let st = e.store().lock();
        let trunks: Vec<u64> = st.free_list().trunk_pages().collect();
        assert_eq!(trunks.len(), 1, "precondition: one trunk");
        (trunks[0], st.free_list().pages().count())
    };
    // Take every listed page, which empties the trunk but leaves it in
    // the list.
    let exact = vec![6u8; listed * OVERFLOW_CAP];
    e.commit(&[put(tree, b"exact", &exact)], Ids::default())
        .unwrap();
    model.insert(b"exact".to_vec(), exact);
    {
        let st = e.store().lock();
        assert_eq!(st.free_list().pages().count(), 0);
        assert_eq!(st.free_list().trunk_pages().collect::<Vec<_>>(), [trunk]);
    }
    evict_all(&e);
    // The frames left at the pool's floor are replaced by misses: reading
    // the tree turns the clock over them. Which frames remain at the floor
    // depends on the order of earlier evictions (task #15 changed it for
    // page copies), so the trunk may be one of them.
    for _ in 0..2 {
        e.store().scan_prefix(tree, b"", |_, _| true).unwrap();
    }
    assert!(
        e.pool().resident(trunk).is_none(),
        "precondition: the trunk left the pool"
    );
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    // The first value takes the trunk and fits; the second overruns the
    // reserve. A value's pages are all taken before any is written.
    let fits = vec![8u8; 1 << 20];
    let big = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"fits", &fits), put(tree, b"big", &big)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    assert_eq!(
        e.store()
            .lock()
            .free_list()
            .trunk_pages()
            .collect::<Vec<_>>(),
        [trunk]
    );
    assert_state(&e, tree, &model);
    e.commit(&[put(tree, b"after", b"1")], Ids::default())
        .unwrap();
    model.insert(b"after".to_vec(), b"1".to_vec());
    drop(e);
    let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
    assert_state(&e, tree, &model);
}

/// A batch that frees a page and later takes it again copies it first,
/// also when it is no longer in the pool: the undo makes the page live
/// again, so it must get its image back. Here the batch frees the overflow
/// chain of `k1`; under memory pressure the chain's pages leave the pool,
/// the next value takes them, and the batch fails with a memory limit. The
/// committed value of `k1` must read back intact, before and after reopen.
/// Without the copy it reads back with the failed batch's bytes (measured
/// 2026-10-03 for every headroom from 1 to 23 pages).
#[test]
fn an_undone_batch_that_took_a_page_it_freed_restores_that_page() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), 4 << 20, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let old = vec![5u8; 100_000];
    e.commit(&[put(tree, b"k1", &old)], Ids::default()).unwrap();
    let model = Model::from([(b"k1".to_vec(), old)]);
    let mut hog = Vec::new();
    while let Ok(r) = e.pool().reserve(8192) {
        hog.push(r);
    }
    hog.truncate(hog.len() - 4);
    let v2 = vec![7u8; 100_000];
    let r = e.commit(
        &[put(tree, b"k1", b"small"), put(tree, b"k2", &v2)],
        Ids::default(),
    );
    assert!(matches!(r, Err(Error::MemoryLimit { .. })), "{r:?}");
    drop(hog);
    assert_state(&e, tree, &model);
    drop(e);
    let e = Engine::open(d, fault.clone(), 4 << 20, sink()).unwrap();
    assert_state(&e, tree, &model);
}

/// D1, deferred write (TS-16 step 6 undo, item 3): commit c frees pages
/// and has joined the open group, which is not durable yet; batch d takes
/// them and is undone. The pages keep d's images, dirty with the page LSN
/// at which c is durable, so nothing writes them before c is durable.
/// Once it is, eviction writes them, and each reads back with d's image.
#[test]
fn pages_freed_by_an_undurable_commit_keep_the_undo_image_once_it_is_durable() {
    use crate::storage::page;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    let medium = vec![5u8; 100_000];
    e.commit(&[put(tree, b"big", &medium)], Ids::default())
        .unwrap();
    use_up_reserve(&e, tree, &mut model, 600);
    let (freed, seq) = {
        let store = e.store();
        let mut st = store.lock();
        let before: BTreeSet<u64> = st.free_list().pages().collect();
        store
            .apply_batch(&mut st, Some(e.wal()), &[del(tree, b"big")], 0)
            .unwrap();
        let seq = audit_join(&mut st);
        let freed: Vec<u64> = st
            .free_list()
            .pages()
            .filter(|p| !before.contains(p))
            .collect();
        (freed, seq)
    };
    // c read the pages it freed; they are clean and leave the pool.
    evict_all(&e);
    let cold: Vec<u64> = freed
        .into_iter()
        .filter(|&p| e.pool().resident(p).is_none())
        .collect();
    assert!(
        !cold.is_empty(),
        "precondition: freed pages not in the pool"
    );
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let d1 = vec![8u8; 100_000];
    let huge = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"d1", &d1), put(tree, b"d2", &huge)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    let durable = e.wal().durable_lsn();
    let mut taken = Vec::new();
    for &p in &cold {
        if let Some((pin, dirty)) = e.pool().resident(p) {
            let image = pin.read().unwrap().to_vec();
            assert!(dirty && page::lsn(&image) > durable, "page {p}");
            taken.push((p, image));
        }
    }
    assert!(!taken.is_empty(), "d took pages c freed");
    // A checkpoint pass writes none of them while c is not durable...
    e.pool().write_dirty().unwrap();
    for (p, _) in &taken {
        assert!(
            e.pool().resident(*p).is_some_and(|(_, dirty)| dirty),
            "page {p}"
        );
    }
    // ...and every one of them once it is.
    e.wal().wait_durable(seq).unwrap();
    e.pool().write_dirty().unwrap();
    // With the pool held at its floor, reading the tree recycles its
    // frames.
    {
        let mut held = Vec::new();
        while let Ok(r) = e.pool().reserve(8192) {
            held.push(r);
        }
        entries(&e, tree);
    }
    let mut evicted = 0;
    for (p, image) in &taken {
        if let Some((_, dirty)) = e.pool().resident(*p) {
            assert!(!dirty, "page {p} not written once c is durable");
            continue;
        }
        evicted += 1;
        let pin = e.pool().fetch(*p).unwrap();
        assert!(
            pin.read().unwrap()[4..] == image[4..],
            "page {p} read back an image other than the undo's"
        );
    }
    assert!(evicted > 0, "precondition: a taken page was evicted");
    assert_state(&e, tree, &model);
}

/// [`crate::test_support::NoFlush`] whose `sync_data` takes `delay`, as a
/// device flush does: the writer's gather window is half the last group's
/// write and sync (task #15).
struct SlowSync(Duration);

struct SlowFile(Box<dyn crate::storage::StorageFile>, Duration);

impl crate::storage::Storage for SlowSync {
    fn open(&self, p: &Path) -> std::io::Result<Box<dyn crate::storage::StorageFile>> {
        Ok(Box::new(SlowFile(
            crate::test_support::NoFlush.open(p)?,
            self.0,
        )))
    }
    fn create(&self, p: &Path) -> std::io::Result<Box<dyn crate::storage::StorageFile>> {
        Ok(Box::new(SlowFile(
            crate::test_support::NoFlush.create(p)?,
            self.0,
        )))
    }
    fn remove(&self, p: &Path) -> std::io::Result<()> {
        crate::test_support::NoFlush.remove(p)
    }
    fn rename(&self, a: &Path, b: &Path) -> std::io::Result<()> {
        crate::test_support::NoFlush.rename(a, b)
    }
    fn list(&self, d: &Path) -> std::io::Result<Vec<std::ffi::OsString>> {
        crate::test_support::NoFlush.list(d)
    }
    fn sync_dir(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn try_lock(&self, p: &Path) -> std::io::Result<Box<dyn crate::storage::StorageLock>> {
        crate::test_support::NoFlush.try_lock(p)
    }
    fn available_space(&self, d: &Path) -> std::io::Result<u64> {
        crate::test_support::NoFlush.available_space(d)
    }
}

impl crate::storage::StorageFile for SlowFile {
    fn read_at(&self, b: &mut [u8], o: u64) -> std::io::Result<usize> {
        self.0.read_at(b, o)
    }
    fn write_at(&self, b: &[u8], o: u64) -> std::io::Result<()> {
        self.0.write_at(b, o)
    }
    fn sync_data(&self) -> std::io::Result<()> {
        std::thread::sleep(self.1);
        Ok(())
    }
    fn set_len(&self, l: u64) -> std::io::Result<()> {
        self.0.set_len(l)
    }
    fn len(&self) -> std::io::Result<u64> {
        self.0.len()
    }
    fn reserve(&self, o: u64, l: u64) -> std::io::Result<()> {
        self.0.reserve(o, l)
    }
}

/// Commits `n` one-key batches from each of `threads` threads into a new
/// database on [`SlowSync`] and returns the engine, the sink and the
/// commits per WAL group.
fn closed_loop(dir: &Path, threads: usize, n: usize) -> (Engine, Arc<RecordingSink>, f64) {
    use crate::obs::names::transaction_log::GROUPS;
    let sink = sink();
    let storage: Arc<dyn crate::storage::Storage> = Arc::new(SlowSync(Duration::from_millis(2)));
    let e = Engine::create(dir, storage, CAP, sink.clone()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let before = sink.snapshot();
    std::thread::scope(|s| {
        for t in 0..threads {
            let e = &e;
            s.spawn(move || {
                for i in 0..n {
                    let k = key(t, i, 0);
                    e.commit(&[put(tree, &k, b"v")], Ids::default()).unwrap();
                }
            });
        }
    });
    let groups = sink.snapshot().since(&before).counter(GROUPS);
    let per_group = (threads * n) as f64 / groups as f64;
    (e, sink, per_group)
}

/// Task #15 gather window: committers woken by one group rejoin before the
/// writer closes the next, so four committers in a closed loop share one
/// flush instead of waiting for two (laboratory report, #15 "Open point
/// 1": 2.00 commits per group before, 3.98 with a window). Fails with the
/// window removed (2.0 commits per group).
#[test]
fn four_closed_loop_committers_share_one_flush() {
    let dir = test_dirs().remove(0);
    let (e, _, per_group) = closed_loop(dir.path(), 4, 25);
    assert!(
        e.wal().points.gathers.load(Ordering::Relaxed) > 0,
        "the writer never gathered"
    );
    assert!(per_group >= 2.5, "{per_group:.2} commits per group");
}

/// Task #15: a lone committer never waits in a gather window, so its
/// latency stays one flush. Fails if the window opens whatever the number
/// of recent committers.
#[test]
fn a_lone_committer_never_waits_in_a_gather_window() {
    let dir = test_dirs().remove(0);
    let (e, _, per_group) = closed_loop(dir.path(), 1, 20);
    assert_eq!(e.wal().points.gathers.load(Ordering::Relaxed), 0);
    assert!((per_group - 1.0).abs() < f64::EPSILON);
}

/// Task #15, D4: a segment created ahead of use is filled outside the
/// segment map and off the WAL writer thread, so commits that do not need
/// it become durable while it is being created. Before the fix the writer
/// created it while holding the map, and these commits waited 18-40 ms per
/// creation (laboratory report, #15 "Open point 4"); with the creation
/// held at its pause point they never return, and the test fails.
#[test]
fn commits_become_durable_while_a_segment_is_created_ahead_of_use() {
    use crate::sync::mpsc;
    let dir = test_dirs().remove(0);
    let e = Arc::new(Engine::create(dir.path(), fault(), CAP, sink()).unwrap());
    let tree = e.create_trees(1).unwrap()[0];
    let value = [1u8; 400];
    let keys: Vec<Vec<u8>> = (0..2_000u32).map(|i| i.to_be_bytes().to_vec()).collect();
    for chunk in keys.chunks(500) {
        let batch: Vec<Write<'_>> = chunk.iter().map(|k| put(tree, k, &value)).collect();
        e.commit(&batch, Ids::default()).unwrap();
    }
    // The creation ahead of use stops at its pause point until released.
    // Only the checkpointer thread's creation stops: a committer that had to
    // create the segment itself would block under the commit latch.
    let (entered_tx, entered) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), crate::sync::Condvar::new()));
    /// Releases the paused creation when dropped, also when an assertion
    /// below fails: otherwise the engine's drop would wait for the paused
    /// checkpointer forever and the test would hang instead of failing.
    struct Release(Arc<(Mutex<bool>, crate::sync::Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            let (m, cv) = &*self.0;
            *m.lock()
                .unwrap_or_else(crate::sync::PoisonError::into_inner) = true;
            cv.notify_all();
        }
    }
    {
        let release = Arc::clone(&release);
        let entered_tx = Mutex::new(entered_tx);
        *e.wal().points.segment_created.lock().unwrap() = Some(Box::new(move || {
            if std::thread::current().name() != Some("lpg-checkpoint") {
                return;
            }
            let _ = entered_tx.lock().unwrap().send(());
            let (m, cv) = &*release;
            let mut go = m.lock().unwrap();
            while !*go {
                go = cv.wait(go).unwrap();
            }
        }));
    }
    // Declared after `e`, so it is dropped, and the creation released,
    // before `e` is.
    let released = Release(Arc::clone(&release));
    // 100-page commits (one key per leaf) until the WAL end passes three
    // quarters of the first segment, where the writer asks the
    // checkpointer for the next one; then wait for that creation to start.
    let stride: Vec<&Vec<u8>> = keys.iter().step_by(20).collect();
    let mut fill = 2u8;
    while e.wal().durable_lsn() < SEGMENT_SPAN / 4 * 3 {
        let v = [fill; 400];
        let batch: Vec<Write<'_>> = stride.iter().map(|k| put(tree, k, &v)).collect();
        e.commit(&batch, Ids::default()).unwrap();
        fill = fill.wrapping_add(1);
    }
    entered
        .recv_timeout(Duration::from_secs(20))
        .expect("the checkpointer starts creating the next segment");
    assert!(e.wal().durable_lsn() < SEGMENT_SPAN, "precondition");
    let (done_tx, done) = mpsc::channel();
    {
        let e = Arc::clone(&e);
        std::thread::spawn(move || {
            for i in 0..3u8 {
                let r = e.commit(&[put(tree, &keys[0], &[i; 400])], Ids::default());
                let _ = done_tx.send(r.map(|_| ()).map_err(|x| x.to_string()));
            }
        });
    }
    let r = (0..3).map(|_| done.recv_timeout(Duration::from_secs(10)));
    let results: Vec<_> = r.collect();
    drop(released);
    for r in results {
        assert!(
            matches!(r, Ok(Ok(()))),
            "a commit waited for the segment creation: {r:?}"
        );
    }
}

/// TS-30 R10 pause points `after_apply` and `checkpoint_revisit`: a page a
/// commit changed before it joined is skipped by a checkpoint, which waits
/// for the commit's group and writes the page on its revisit (TS-11 step
/// 2). The interleaving is set by the points, without sleeps.
#[test]
fn a_checkpoint_skips_a_page_changed_before_its_commit_joins_and_writes_it_on_the_revisit() {
    use crate::sync::mpsc;
    let dir = test_dirs().remove(0);
    let e = Arc::new(Engine::create(dir.path(), fault(), CAP, sink()).unwrap());
    let tree = e.create_trees(1).unwrap()[0];
    e.commit(&[put(tree, b"k", b"0")], Ids::default()).unwrap();
    e.checkpoint().unwrap();
    let (revisit_tx, revisit) = mpsc::channel::<()>();
    let revisit_tx = Mutex::new(revisit_tx);
    *e.wal().points.checkpoint_revisit.lock().unwrap() = Some(Box::new(move || {
        let _ = revisit_tx.lock().unwrap().send(());
    }));
    let checkpoint = Arc::new(Mutex::new(None));
    {
        let (e2, checkpoint) = (Arc::clone(&e), Arc::clone(&checkpoint));
        let revisit = Mutex::new(revisit);
        *e.wal().points.after_apply.lock().unwrap() = Some(Box::new(move || {
            // Under the commit latch, the page is changed and not durable.
            let e3 = Arc::clone(&e2);
            let h = std::thread::spawn(move || e3.checkpoint());
            revisit.lock().unwrap().recv().unwrap();
            *checkpoint.lock().unwrap() = Some(h);
        }));
    }
    e.commit(&[put(tree, b"k", b"1")], Ids::default()).unwrap();
    *e.wal().points.after_apply.lock().unwrap() = None;
    let h = checkpoint
        .lock()
        .unwrap()
        .take()
        .expect("the checkpoint ran");
    let stats = h.join().unwrap().unwrap();
    assert!(stats.pages_waited >= 1, "{stats:?}");
    assert!(stats.pages_written >= 1, "{stats:?}");
    let mut skipped = Vec::new();
    assert_eq!(e.pool().checkpoint_pass(None, &mut skipped).unwrap(), 0);
    assert!(skipped.is_empty(), "every page was written: {skipped:?}");
}

/// Audit (2026-10-03), known item of rmp #20 confirmed: ENOSPC on the WAL
/// group write sets the unusable latch, yet the commit returns
/// `StorageFull`, which `Engine::commit` documents as "nothing applied and
/// the database usable" (FR-REC-007). A caller that retries on ENOSPC, as
/// the contract invites, meets a database that refuses everything until
/// reopened. Fails until a commit whose group failed returns an error that
/// is not ENOSPC (for example `Error::Unusable` or an `Io` of another
/// kind).
#[test]
fn audit_enospc_on_the_group_write_is_not_reported_as_a_usable_enospc() {
    use crate::storage::fault::{Fault, Kind};
    let dir = test_dirs().remove(0);
    let f = fault();
    let e = Engine::create(dir.path(), f.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    e.commit(&[put(tree, b"k", b"0")], Ids::default()).unwrap();
    {
        let f = f.clone();
        // After the apply, under the commit latch: the next write is the
        // writer's group write (the pool writes nothing at this size).
        *e.wal().points.after_apply.lock().unwrap() = Some(Box::new(move || {
            f.fail_nth(Kind::WriteAt, f.count(Kind::WriteAt) + 1, Fault::NoSpace);
        }));
    }
    let r = e.commit(&[put(tree, b"k", b"1")], Ids::default());
    *e.wal().points.after_apply.lock().unwrap() = None;
    let err = r.expect_err("the group write failed");
    assert!(e.check().is_err(), "precondition: the database is unusable");
    assert!(
        !err.is_no_space(),
        "the commit reports ENOSPC ({err}) while the database is unusable"
    );
}
