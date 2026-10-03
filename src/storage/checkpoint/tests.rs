use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use super::*;
use crate::obs::names::{cache, checkpointing, store_size, transaction_log, transactions};
use crate::obs::recording::RecordingSink;
use crate::storage::control::ControlSlots;
use crate::storage::engine::{CONTROL, Engine};
use crate::storage::fault::Crash;
use crate::storage::wal::{FRAME_BYTES, SEGMENT_SIZE, SEGMENT_SPAN};
use crate::storage::{FsStorage, Storage};
use crate::test_support::{test_dirs, thread_names};
use crate::tx::commit::Ids;
use crate::tx::commit::tests::{Model, assert_state, fault, put, sink};

/// Memory cap of these tests: 8 MiB.
const CAP: u64 = 8 << 20;

/// The storage of an engine: [`fault`], whose sync skips the device; the
/// tests read and change the files through `FsStorage` between opens.
fn fs() -> Arc<dyn Storage> {
    fault()
}

fn read_control(dir: &std::path::Path) -> (Control, u64) {
    let file = FsStorage.open(&dir.join(CONTROL)).unwrap();
    let c = ControlSlots::open(file, Arc::default(), &RecordingSink::default()).unwrap();
    (*c.current(), c.sequence())
}

fn wait_for(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// TS-11: after a clean close the authoritative `CONTROL` slot records a
/// clean shutdown with the redo point at the WAL end; the next open's
/// checkpoint (TS-13 step 8) clears the flag, and its slot's sequence
/// number is the open's salt (TS-09).
#[test]
fn close_records_a_clean_shutdown_and_the_next_open_clears_it() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = Engine::create(d, fs(), CAP, sink()).unwrap();
        let (c, _) = read_control(d);
        assert!(
            !c.clean_shutdown,
            "create's open checkpoint clears the flag"
        );
        let tree = e.create_trees(1).unwrap()[0];
        e.commit(&[put(tree, b"a", b"1")], Ids::default()).unwrap();
        let end = e.wal().durable_lsn();
        e.close().unwrap();
        let (c, seq) = read_control(d);
        assert!(c.clean_shutdown);
        assert_eq!(c.redo_point, end);
        assert_eq!(c.commit_ts, 2);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        let (c2, seq2) = read_control(d);
        assert!(!c2.clean_shutdown);
        assert_eq!(seq2, seq + 1);
        assert_eq!(e.wal().salt(), seq2);
        assert_eq!(
            e.recovered().groups,
            0,
            "nothing to replay after a clean close"
        );
        let mut out = Vec::new();
        assert!(e.store().get(tree, b"a", &mut out).unwrap());
        // A drop leaves the flag clear.
        drop(e);
        assert!(!read_control(d).0.clean_shutdown);
    }
}

/// TS-11 trigger: 100 000 commits run exactly one checkpoint (the time
/// trigger is set beyond the test).
///
/// The storage skips the device sync ([`fault`]): with it the run time is
/// the number of WAL groups times the `sync_data` latency. Measured on
/// 2026-10-02 with real syncs: 1 824 groups, 3.7 s alone but 48 s beside
/// the rest of the suite.
#[test]
fn a_hundred_thousand_commits_trigger_exactly_one_checkpoint() {
    const THREADS: usize = 64;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let s = Arc::new(RecordingSink::default());
    let tuning = Tuning {
        checkpoint_interval: Duration::from_secs(3600),
        checkpoint_commits: 100_000,
    };
    let e = Engine::create_tuned(d, fault(), CAP, s.clone(), tuning).unwrap();
    let before = s.snapshot();
    assert_eq!(
        before.counter(checkpointing::EVENTS),
        1,
        "the open checkpoint"
    );
    std::thread::scope(|sc| {
        for _ in 0..THREADS {
            sc.spawn(|| {
                for _ in 0..100_000 / THREADS {
                    e.commit(&[], Ids::default()).unwrap();
                }
            });
        }
    });
    let rest = 100_000 - THREADS * (100_000 / THREADS);
    for _ in 0..rest {
        e.commit(&[], Ids::default()).unwrap();
    }
    assert_eq!(
        s.snapshot().since(&before).counter(transactions::COMMITTED),
        100_000
    );
    wait_for("the checkpoint", Duration::from_secs(60), || {
        s.snapshot().since(&before).counter(checkpointing::EVENTS) >= 1
    });
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        s.snapshot().since(&before).counter(checkpointing::EVENTS),
        1
    );
    // One commit short of the next trigger: still one.
    for _ in 0..10 {
        e.commit(&[], Ids::default()).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        s.snapshot().since(&before).counter(checkpointing::EVENTS),
        1
    );
}

/// TS-11 trigger: the periodic checkpoint runs on its interval.
#[test]
fn the_interval_triggers_checkpoints() {
    let dir = test_dirs().remove(0);
    let s = Arc::new(RecordingSink::default());
    let tuning = Tuning {
        checkpoint_interval: Duration::from_millis(100),
        checkpoint_commits: 100_000,
    };
    let e = Engine::create_tuned(dir.path(), fs(), CAP, s.clone(), tuning).unwrap();
    wait_for("three checkpoints", Duration::from_secs(10), || {
        s.snapshot().counter(checkpointing::EVENTS) >= 4
    });
    e.check().unwrap();
}

/// TS-11 step 5: segments below the redo point are renamed to follow the
/// newest one, or removed once enough segments lie ahead; their old frames
/// are never replayed (their LSNs differ from their new positions), and the
/// WAL stays bounded.
#[test]
fn checkpoints_recycle_segments_and_their_old_frames_are_never_replayed() {
    for dir in test_dirs() {
        let d = dir.path();
        let s = Arc::new(RecordingSink::default());
        // The device sync adds nothing here ([`fault`]); it made this test
        // wait 5.5 s of its 6.6 s alone (2026-10-03).
        let storage = fault();
        let e = Engine::create(d, storage.clone(), CAP, s.clone()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        let v = vec![0x42u8; 1 << 20];
        let mut i = 0;
        let mut commit_mib = |e: &Engine, model: &mut Model, upto: u64| {
            while e.wal().durable_lsn() < upto {
                let k = format!("k-{i:05}");
                e.commit(&[put(tree, k.as_bytes(), &v)], Ids::default())
                    .unwrap();
                model.insert(k.into_bytes(), v.clone());
                i += 1;
            }
        };
        commit_mib(&e, &mut model, 2 * SEGMENT_SPAN + SEGMENT_SPAN / 2);
        let st = e.checkpoint().unwrap();
        assert_eq!(st.recycled + st.removed, 2, "{st:?}");
        assert!(st.recycled >= 1);
        let lsns = e.wal().segment_lsns();
        assert!(lsns[0] >= 2 * SEGMENT_SPAN, "{lsns:x?}");
        for w in lsns.windows(2) {
            assert_eq!(w[1], w[0] + SEGMENT_SPAN, "one chain: {lsns:x?}");
        }
        // Write into the recycled segments, then recycle again.
        for round in 0..3 {
            commit_mib(&e, &mut model, st.redo + (round + 2) * SEGMENT_SPAN);
            e.checkpoint().unwrap();
            let size = s.snapshot().gauge(transaction_log::SIZE).unwrap();
            assert!(size <= 5 * SEGMENT_SIZE, "WAL size {size}");
        }
        let m = s.snapshot();
        assert!(m.counter(checkpointing::SEGMENTS_RECYCLED) >= 3);
        // Leave old frames in a recycled segment ahead of the WAL end, then
        // reopen after a drop.
        e.checkpoint().unwrap();
        let ahead = e.wal().segment_lsns();
        assert!(*ahead.last().unwrap() > e.wal().durable_lsn() + FRAME_BYTES);
        drop(e);
        let e = Engine::open(d, storage.clone(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
        e.close().unwrap();
        let e = Engine::open(d, storage.clone(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
    }
}

/// TS-11 fuzzy checkpoint: checkpoints that run while 8 threads commit
/// (with eviction) lose nothing after a crash that drops every unsynced
/// write.
#[test]
fn fuzzy_checkpoints_under_concurrent_commits_lose_nothing() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let done = AtomicBool::new(false);
    let mut checkpoints = 0;
    std::thread::scope(|sc| {
        let mut hs = Vec::new();
        for t in 0..8u32 {
            let e = &e;
            hs.push(sc.spawn(move || {
                let v = vec![t as u8; 300];
                for i in 0..120u32 {
                    let ks: Vec<String> = (0..40).map(|j| format!("{t}-{i:04}-{j:02}")).collect();
                    let batch: Vec<_> = ks.iter().map(|k| put(tree, k.as_bytes(), &v)).collect();
                    e.commit(&batch, Ids::default()).unwrap();
                }
            }));
        }
        while !hs.iter().all(|h| h.is_finished()) {
            let st = e.checkpoint().unwrap();
            checkpoints += 1;
            let _ = st;
        }
        done.store(true, AtomicOrdering::Release);
    });
    assert!(checkpoints >= 2, "{checkpoints} checkpoints");
    let mut model = Model::new();
    for t in 0..8u32 {
        for i in 0..120u32 {
            for j in 0..40 {
                model.insert(
                    format!("{t}-{i:04}-{j:02}").into_bytes(),
                    vec![t as u8; 300],
                );
            }
        }
    }
    assert_state(&e, tree, &model);
    drop(e);
    fault.crash(Crash::DropAll).unwrap();
    let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
    assert_state(&e, tree, &model);
}

/// FR-OBS-001: the kernel reports at least one metric in each category it
/// owns: transactions, transaction log, checkpointing, cache, store size.
#[test]
fn every_kernel_metric_category_reports() {
    let dir = test_dirs().remove(0);
    let s = Arc::new(RecordingSink::default());
    let e = Engine::create(dir.path(), fs(), CAP, s.clone()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    e.commit(&[put(tree, b"a", b"1")], Ids::default()).unwrap();
    let mut out = Vec::new();
    e.store().get(tree, b"a", &mut out).unwrap();
    e.checkpoint().unwrap();
    e.pool().publish_metrics();
    let m = s.snapshot();
    assert_eq!(m.counter(transactions::COMMITTED), 2);
    assert!(!m.samples(transactions::COMMIT_LATENCY).is_empty());
    assert!(m.counter(transaction_log::GROUPS) >= 2);
    assert!(m.counter(transaction_log::FLUSHES) >= 2);
    assert_eq!(
        m.counter(transaction_log::APPENDED_BYTES),
        m.counter(transaction_log::FRAMES) * FRAME_BYTES
    );
    assert_eq!(m.gauge(transaction_log::SIZE), Some(SEGMENT_SIZE));
    assert_eq!(m.counter(checkpointing::EVENTS), 2);
    assert!(m.counter(checkpointing::PAGES_WRITTEN) >= 3);
    assert!(m.gauge(cache::ACCOUNTED_BYTES).is_some());
    assert!(m.counter(cache::HITS) > 0);
    assert!(m.gauge(store_size::DATABASE).is_some());
}

/// TS-02: each open database runs the threads `lpg-wal` and
/// `lpg-checkpoint`. A spawned thread sets its own name when it starts
/// (`std` `thread/lifecycle.rs`), so the test waits for the names.
#[test]
fn the_wal_writer_and_checkpointer_threads_are_named() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(dir.path(), fs(), CAP, sink()).unwrap();
    wait_for("the thread names", Duration::from_secs(10), || {
        let names = thread_names();
        names.iter().any(|n| n == "lpg-wal") && names.iter().any(|n| n == "lpg-checkpoint")
    });
    e.close().unwrap();
}

/// TS-11 step 5, audit m2: recycling makes each rename and each removal of
/// a segment durable (`sync_dir`) before the next one, as PostgreSQL's
/// `durable_rename` and `durable_unlink`. `FaultStorage` keeps or loses all
/// namespace changes of a directory at once, so it cannot show a crash that
/// keeps a later rename without an earlier one; the call order is checked
/// instead, then a crash and a reopen.
#[test]
fn recycling_makes_each_rename_and_removal_durable_before_the_next() {
    use crate::storage::fault::Call;
    use crate::storage::wal::parse_segment_name;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    let v = vec![0x42u8; 1 << 20];
    let mut i = 0;
    while e.wal().durable_lsn() < 2 * SEGMENT_SPAN + SEGMENT_SPAN / 2 {
        let k = format!("k-{i:05}");
        e.commit(&[put(tree, k.as_bytes(), &v)], Ids::default())
            .unwrap();
        model.insert(k.into_bytes(), v.clone());
        i += 1;
    }
    let from = fault.calls().len();
    let st = e.checkpoint().unwrap();
    assert_eq!(st.recycled + st.removed, 2, "{st:?}");
    let is_segment = |p: &std::path::Path| p.file_name().and_then(parse_segment_name).is_some();
    let calls = &fault.calls()[from..];
    let mut changes = 0;
    for (n, c) in calls.iter().enumerate() {
        let changed = match c {
            Call::Rename(a, b) => is_segment(a) && is_segment(b),
            Call::Remove(p) => is_segment(p),
            _ => false,
        };
        if changed {
            changes += 1;
            assert!(
                matches!(calls.get(n + 1), Some(Call::SyncDir(_))),
                "{c:?} is not followed by sync_dir: {:?}",
                calls.get(n + 1)
            );
        }
    }
    assert_eq!(changes, 2);
    drop(e);
    fault.crash(Crash::DropAll).unwrap();
    let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
    assert_state(&e, tree, &model);
}
