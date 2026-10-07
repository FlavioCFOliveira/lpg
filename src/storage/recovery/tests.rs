use std::path::Path;
use std::sync::Arc;

use crate::storage::btree::Tree;
use crate::storage::control::tests::patch_slot;
use crate::storage::engine::{CONTROL, Engine};
use crate::storage::error::Error;
use crate::storage::fault::{Call, Crash};
use crate::storage::page::{self, PAGE_BYTES, PAGE_SIZE};
use crate::storage::wal::tests::DurableScan;
use crate::storage::wal::{
    self, CommitRecord, Counters, FRAME_BYTES, FRAME_HEADER, FRAME_SIZE, KIND_COMMIT, KIND_PAGE,
    SEGMENT_SPAN, segment_name,
};
use crate::storage::{FsStorage, Storage};
use crate::test_support::{seed, test_dirs};
use crate::tx::commit::Ids;
use crate::tx::commit::tests::{Model, assert_state, del, fault, put, sink};

/// Memory cap of these tests: 8 MiB, so that eviction writes pages.
const CAP: u64 = 8 << 20;

/// The storage of an engine: [`fault`], whose sync skips the device; the
/// tests read and change the files through `FsStorage` between opens.
fn fs() -> Arc<dyn Storage> {
    fault()
}

fn commit_kv(e: &Engine, tree: Tree, model: &mut Model, k: &[u8], v: &[u8]) {
    e.commit(&[put(tree, k, v)], Ids::default()).unwrap();
    model.insert(k.to_vec(), v.to_vec());
}

/// Commits 1 MiB values until the durable LSN passes `lsn`.
fn fill_wal_to(e: &Engine, tree: Tree, model: &mut Model, lsn: u64) {
    let v = vec![0x42u8; 1 << 20];
    let mut i = 0;
    while e.wal().durable_lsn() < lsn {
        commit_kv(e, tree, model, format!("fill-{i:04}").as_bytes(), &v);
        i += 1;
    }
}

fn names(dir: &Path) -> Vec<String> {
    FsStorage
        .list(dir)
        .unwrap()
        .into_iter()
        .map(|n| n.into_string().unwrap())
        .collect()
}

/// Flips one byte of the frame at `lsn` in the WAL of generation 1.
fn corrupt_frame(dir: &Path, lsn: u64, at: usize) {
    let segs = wal::tests::durable_segments(dir, 1);
    let (&first, _) = segs.range(..=lsn).next_back().unwrap();
    let f = FsStorage.open(&dir.join(segment_name(1, first))).unwrap();
    let off = lsn - first + at as u64;
    let mut b = [0u8; 1];
    f.read_at(&mut b, off).unwrap();
    f.write_at(&[b[0] ^ 0x01], off).unwrap();
    f.sync_data().unwrap();
}

/// FR-DB-009, FR-REC-001, FR-REC-002, TS-10: after a drop without close
/// and a crash (`mode`) that keeps none, a random subset, or torn sectors
/// of the unsynced writes (pages that eviction wrote, unsynced), the next
/// open holds every commit that returned, and nothing else. One test per
/// crash mode, so that they run in parallel.
///
/// 300 commits of 25 scattered 2 000-byte values (15 MB, about twice the
/// cap) make eviction write pages. Measured on 2026-10-02: 100 values of
/// 600 bytes per commit wrote 473 MiB per crash mode (`time` %O), each
/// group carrying about one frame per value, and the suite then spent most
/// of its time waiting for `sync_data`.
fn drop_without_close_then_crash(n: u8, mode: Crash) {
    const KEYS: u32 = 25;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    let v = vec![n; 2000];
    for i in 0..300u32 {
        let ks: Vec<String> = (0..KEYS)
            .map(|j| {
                format!(
                    "key-{:08}",
                    (i * KEYS + j).wrapping_mul(2_654_435_761) % 1_000_000
                )
            })
            .collect();
        let batch: Vec<_> = ks.iter().map(|k| put(tree, k.as_bytes(), &v)).collect();
        e.commit(&batch, Ids::default()).unwrap();
        for k in ks {
            model.insert(k.into_bytes(), v.clone());
        }
        if i == 150 {
            e.checkpoint().unwrap();
        }
    }
    for i in 0..30u32 {
        let ks: Vec<String> = (0..10u32)
            .map(|j| {
                format!(
                    "key-{:08}",
                    (i * KEYS + j).wrapping_mul(2_654_435_761) % 1_000_000
                )
            })
            .collect();
        let batch: Vec<_> = ks.iter().map(|k| del(tree, k.as_bytes())).collect();
        e.commit(&batch, Ids::default()).unwrap();
        for k in ks {
            model.remove(k.as_bytes());
        }
    }
    let data_writes = fault
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::WriteAt { path, .. } if path.ends_with("data-1")))
        .count();
    assert!(data_writes > 100, "eviction wrote pages: {data_writes}");
    drop(e);
    fault.crash(mode).unwrap();
    let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
    assert!(e.recovered().groups > 0);
    assert_state(&e, tree, &model);
}

#[test]
fn drop_without_close_then_a_crash_that_keeps_nothing_keeps_every_returned_commit() {
    drop_without_close_then_crash(0, Crash::DropAll);
}

#[test]
fn drop_without_close_then_a_crash_that_keeps_some_writes_keeps_every_returned_commit() {
    drop_without_close_then_crash(1, Crash::KeepRandom { seed: seed() });
}

#[test]
fn drop_without_close_then_a_crash_that_tears_writes_keeps_every_returned_commit() {
    let sd = seed();
    drop_without_close_then_crash(2, Crash::Tear { seed: sd });
    drop_without_close_then_crash(
        3,
        Crash::Tear {
            seed: sd.wrapping_add(1),
        },
    );
}

/// TS-13 step 5: a group whose `sync_data` never returned, torn by a crash
/// at 512-byte boundaries, is a torn tail: the next open ends the WAL
/// before it, and its commit is absent.
#[test]
fn a_torn_last_group_is_discarded() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    drop(e);
    let mut model = Model::new();
    let mut partial = 0;
    for seed in 0..8u64 {
        let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
        commit_kv(&e, tree, &mut model, format!("ok-{seed}").as_bytes(), b"1");
        let x = e.wal().durable_lsn();
        // The storage stops right after the group's write: its sync never
        // returns, and the crash below tears what the write left unsynced.
        // (A failed sync would not do: it forgets the unsynced writes, as
        // Linux does, TS-30 R8.)
        fault.crash_after(1);
        // The durable bytes where the torn group will lie: its 40 kB value
        // takes at least five overflow pages, so at least six frames.
        let span = 6 * FRAME_SIZE;
        let region = |segs: &std::collections::BTreeMap<u64, Vec<u8>>| {
            let (&first, b) = segs.range(..=x).next_back().unwrap();
            let off = (x - first) as usize;
            b[off..off + span].to_vec()
        };
        let before = region(&wal::tests::durable_segments(d, 1));
        let v = vec![seed as u8; 40_000];
        let r = e.commit(
            &[put(tree, format!("torn-{seed}").as_bytes(), &v)],
            Ids::default(),
        );
        assert!(r.is_err());
        drop(e);
        fault.crash(Crash::Tear { seed }).unwrap();
        // The tear kept some sectors of the group: the test is not vacuous.
        // (A whole 8 224-byte frame spans 17 sectors, so a tear rarely keeps
        // a complete frame; any kept sector is enough for the check.)
        if region(&wal::tests::durable_segments(d, 1)) != before {
            partial += 1;
        }
        let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
        assert_eq!(
            e.wal().durable_lsn(),
            x,
            "the WAL ends where the torn group began"
        );
        assert_state(&e, tree, &model);
    }
    assert!(
        partial > 0,
        "no seed left a sector of the torn group on disk"
    );
}

/// TS-13 step 5, FR-REC-003, FR-REC-008: a corrupt frame followed by a
/// later valid group fails the open as corruption; the same damage in the
/// newest group is a torn tail.
#[test]
fn a_corrupt_frame_followed_by_a_valid_group_fails_the_open() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = Engine::create(d, fs(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        for i in 0..5 {
            commit_kv(&e, tree, &mut model, format!("k{i}").as_bytes(), b"v");
        }
        drop(e);
        let mut scan = DurableScan::new(d, 1, 0);
        scan.advance();
        let groups = scan.groups.clone();
        assert_eq!(groups.len(), 6);
        // A page frame of the third group.
        corrupt_frame(d, groups[2].start, FRAME_HEADER + 100);
        let err = Engine::open(d, fs(), CAP, sink()).err().unwrap();
        assert!(matches!(err, Error::Corruption(_)), "{err}");
        assert!(err.to_string().contains("WAL corrupt"), "{err}");
        // Repair it; damage the commit frame of the newest group instead.
        corrupt_frame(d, groups[2].start, FRAME_HEADER + 100);
        let last = groups.last().unwrap();
        let commit = last.start + last.pages.len() as u64 * FRAME_BYTES;
        corrupt_frame(d, commit, 40);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        model.remove(b"k4".as_slice());
        assert_state(&e, tree, &model);
    }
}

/// Writes a synthetic group at `start`: one page frame for page `page`
/// holding `image`, then a commit frame, both with `salt`.
fn write_group(dir: &Path, start: u64, salt: u64, page: u64, image: &[u8]) {
    let mut f = vec![0u8; 2 * FRAME_SIZE];
    f[FRAME_HEADER..FRAME_SIZE].copy_from_slice(image);
    page::set_lsn(&mut f[FRAME_HEADER..FRAME_SIZE], start + FRAME_BYTES);
    wal::seal_frame(&mut f[..FRAME_SIZE], KIND_PAGE, start, salt, page);
    wal::put_commit(
        &mut f[FRAME_SIZE..],
        &CommitRecord {
            first_lsn: start,
            first_ts: 1000,
            counters: Counters {
                commit_ts: 1000,
                ..Counters::default()
            },
        },
    );
    wal::seal_frame(
        &mut f[FRAME_SIZE..],
        KIND_COMMIT,
        start + FRAME_BYTES,
        salt,
        0,
    );
    let segs = wal::tests::durable_segments(dir, 1);
    let (&first, _) = segs.range(..=start).next_back().unwrap();
    assert!(start + 2 * FRAME_BYTES <= first + SEGMENT_SPAN);
    let file = FsStorage.open(&dir.join(segment_name(1, first))).unwrap();
    file.write_at(&f, start - first).unwrap();
    file.sync_data().unwrap();
}

/// TS-09 Valid group: frames left at positions a later open reuses are
/// never replayed. (a) After a torn tail is discarded, the new groups
/// overwrite its start; its remaining frames, valid where they lie, fail
/// the first-LSN rule. (b) A complete group with a lower salt right at the
/// WAL end fails the salt rule; the same group with the current salt is
/// replayed, so the salt alone rejects it.
#[test]
fn leftover_frames_at_reused_positions_are_never_replayed() {
    for dir in test_dirs() {
        let d = dir.path();
        let e = Engine::create(d, fs(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        commit_kv(&e, tree, &mut model, b"g1", b"v");
        let x = e.wal().durable_lsn();
        let v = vec![1u8; 60_000];
        e.commit(&[put(tree, b"g2", &v)], Ids::default()).unwrap();
        drop(e);
        let mut scan = DurableScan::new(d, 1, x);
        scan.advance();
        let g2 = scan.groups[0].clone();
        assert!(g2.pages.len() >= 8);
        // (a) Damage G2's first frame: a torn tail at X.
        corrupt_frame(d, x, FRAME_HEADER + 10);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert_eq!(e.wal().durable_lsn(), x);
        assert_state(&e, tree, &model);
        commit_kv(&e, tree, &mut model, b"g3", b"v");
        let x3 = e.wal().durable_lsn();
        let salt3 = e.wal().salt();
        drop(e);
        // G2's frames beyond G3 are still valid where they lie, older salt.
        let segs = wal::tests::durable_segments(d, 1);
        let left = wal::decode_frame(wal::tests::frame_at(&segs, x3).unwrap(), x3)
            .expect("a leftover frame of G2 lies at the new WAL end");
        assert!(left.salt < salt3);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert_eq!(e.wal().durable_lsn(), x3);
        assert_state(&e, tree, &model);
        // (b) A group applied in this open, then a complete group with a
        // lower salt at the WAL end.
        commit_kv(&e, tree, &mut model, b"g4", b"v");
        let x4 = e.wal().durable_lsn();
        let salt4 = e.wal().salt();
        let unused = e.data().page_count();
        drop(e);
        let mut image = vec![0u8; PAGE_SIZE];
        page::init(&mut image, page::PageType::Overflow, 999);
        write_group(d, x4, salt4 - 1, unused, &image);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert_eq!(
            e.wal().durable_lsn(),
            x4,
            "the lower-salt group was replayed"
        );
        assert_state(&e, tree, &model);
        let mut back = vec![0u8; PAGE_SIZE];
        let read = e.data().read_page_unverified(unused, &mut back).unwrap();
        assert!(!read || page::tree(&back) != 999);
        commit_kv(&e, tree, &mut model, b"g5", b"v");
        let x5 = e.wal().durable_lsn();
        let salt5 = e.wal().salt();
        drop(e);
        write_group(d, x5, salt5, unused, &image);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert_eq!(
            e.wal().durable_lsn(),
            x5 + 2 * FRAME_BYTES,
            "the same group with the current salt is replayed"
        );
        e.data().read_page(unused, &mut back).unwrap();
        assert_eq!(page::tree(&back), 999);
        assert_state(&e, tree, &model);
    }
}

/// TS-13 step 4: each way a missing segment is detected fails the open as
/// corruption, while an empty log after a clean close opens and creates
/// `wal-<g>-<R>`.
#[test]
fn a_missing_segment_at_or_after_the_redo_point_fails_the_open() {
    for dir in test_dirs() {
        let d = dir.path();
        let seg = |lsn: u64| d.join(segment_name(1, lsn));
        let expect_missing = |what: &str| {
            let err = Engine::open(d, fs(), CAP, sink()).err().unwrap();
            assert!(matches!(err, Error::Corruption(_)), "{what}: {err}");
            assert!(
                err.to_string().contains("missing") || err.to_string().contains("holds"),
                "{what}: {err}"
            );
        };
        let e = Engine::create(d, fs(), CAP, sink()).unwrap();
        let tree = e.create_trees(1).unwrap()[0];
        let mut model = Model::new();
        // WAL end past three quarters of segment 1: segments 0, 1, 2.
        fill_wal_to(
            &e,
            tree,
            &mut model,
            SEGMENT_SPAN + SEGMENT_SPAN * 3 / 4 + FRAME_BYTES,
        );
        // The next segment is created ahead of use once the WAL end passes
        // three quarters of the newest. The checkpointer thread does it
        // when the writer asks (task #15, D4); run it here, so the state
        // does not depend on that thread's timing.
        commit_kv(&e, tree, &mut model, b"one-more", b"1");
        e.wal().precreate();
        assert_eq!(e.wal().segment_lsns(), [0, SEGMENT_SPAN, 2 * SEGMENT_SPAN]);
        drop(e);
        let saved: Vec<(u64, Vec<u8>)> = wal::tests::durable_segments(d, 1).into_iter().collect();
        let restore = || {
            for (lsn, bytes) in &saved {
                let _ = FsStorage.remove(&seg(*lsn));
                let f = FsStorage.create(&seg(*lsn)).unwrap();
                f.write_at(bytes, 0).unwrap();
                f.sync_data().unwrap();
            }
        };
        // A gap: segment 1 removed.
        FsStorage.remove(&seg(SEGMENT_SPAN)).unwrap();
        expect_missing("gap");
        // The newest segment's last frame is valid: segments 1 and 2 removed.
        FsStorage.remove(&seg(2 * SEGMENT_SPAN)).unwrap();
        expect_missing("next after a full segment");
        restore();
        // No segment holds R = 0, after a drop (flag clear).
        FsStorage.remove(&seg(0)).unwrap();
        expect_missing("redo point after a crash");
        restore();
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
        let r = e.wal().durable_lsn();
        e.close().unwrap();
        // After a clean close, R is the WAL end, in segment 1.
        let holder = SEGMENT_SPAN;
        assert!(holder <= r && r < 2 * SEGMENT_SPAN);
        // Flag set, R's segment removed, a later one present.
        FsStorage.remove(&seg(holder)).unwrap();
        expect_missing("flag set, a segment after R");
        // Flag set and no segment from R on: the log is empty, nothing is
        // lost, and open creates wal-1-<R>.
        for n in names(d) {
            if n.starts_with("wal-") {
                FsStorage.remove(&d.join(n)).unwrap();
            }
        }
        assert!(!names(d).iter().any(|n| n.starts_with("wal-")));
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert!(e.recovered().empty_log);
        assert!(names(d).contains(&segment_name(1, r)));
        assert_state(&e, tree, &model);
        commit_kv(&e, tree, &mut model, b"after", b"1");
        drop(e);
        let e = Engine::open(d, fs(), CAP, sink()).unwrap();
        assert_state(&e, tree, &model);
    }
}

/// TS-13 step 6: each counter is the maximum of `CONTROL` and the applied
/// commit frames, in both directions.
#[test]
fn counters_are_the_maximum_of_control_and_the_commit_frames() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let e = Engine::create(d, fs(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let ids = Ids {
        next_element_id: 500,
        next_catalog_id: 5,
        next_token_id: 50,
    };
    e.commit(&[put(tree, b"a", b"1")], ids).unwrap();
    drop(e);
    // The frames are above CONTROL, except where CONTROL is patched higher.
    for slot in [0, 1] {
        patch_slot(&d.join(CONTROL), slot, |c| {
            c.next_catalog_id = 70;
            c.commit_ts = 1;
        });
    }
    let e = Engine::open(d, fs(), CAP, sink()).unwrap();
    let expected = Counters {
        commit_ts: 2,
        next_element_id: 500,
        next_catalog_id: 70,
        next_token_id: 50,
    };
    assert_eq!(e.store().lock().log_ref().counters, expected);
    let c = *e.control().lock().unwrap().current();
    assert_eq!(
        (
            c.commit_ts,
            c.next_element_id,
            c.next_catalog_id,
            c.next_token_id
        ),
        (2, 500, 70, 50),
        "open's checkpoint saved the maximum"
    );
    let next = e.commit(&[put(tree, b"b", b"1")], Ids::default()).unwrap();
    assert_eq!(next.commit_ts, 3);
}

/// TS-10: a data page torn on disk is overwritten from its WAL image before
/// anything reads it.
#[test]
fn a_torn_data_page_is_repaired_from_the_wal() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let e = Engine::create(d, fs(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    commit_kv(&e, tree, &mut model, b"a", b"1");
    // Write the pages, as eviction would, without a checkpoint.
    e.pool().write_dirty().unwrap();
    e.data().sync().unwrap();
    drop(e);
    let file = FsStorage.open(&d.join("data-1")).unwrap();
    for p in [0, 1, tree.root()] {
        file.write_at(&[0xFF; 512], p * PAGE_BYTES + 1024).unwrap();
    }
    file.sync_data().unwrap();
    let e = Engine::open(d, fs(), CAP, sink()).unwrap();
    assert_state(&e, tree, &model);
}

/// AUDIT D (storage-engine-auditor, 2026-10-02): recovery trusts the page
/// number of a CRC-valid frame. A frame naming page `u64::MAX` or a page
/// whose byte offset exceeds `u64` must fail the open as corruption, with
/// no panic (`recover`: `data.ensure(page_no + 1)`; `Datafile::ensure`:
/// `(target - reserved) * PAGE_BYTES`). Fails until the page number is
/// bounded before use.
#[test]
fn audit_a_crc_valid_frame_with_a_hostile_page_number_fails_the_open_as_corruption() {
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let e = Engine::create(d, fs(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    commit_kv(&e, tree, &mut model, b"a", b"v");
    let x = e.wal().durable_lsn();
    let salt = e.wal().salt();
    drop(e);
    let mut image = vec![0u8; PAGE_SIZE];
    page::init(&mut image, page::PageType::Overflow, 1);
    let mut outcomes = Vec::new();
    for hostile in [u64::MAX, 1u64 << 52] {
        write_group(d, x, salt, hostile, &image);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Engine::open(d, fs(), CAP, sink()).map(drop)
        }));
        let outcome = match r {
            Err(p) => format!(
                "panic: {}",
                p.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default()
            ),
            Ok(Err(Error::Corruption(_))) => "corruption".to_owned(),
            Ok(other) => format!("{other:?}"),
        };
        outcomes.push((hostile, outcome));
    }
    assert!(
        outcomes.iter().all(|(_, o)| o == "corruption"),
        "{outcomes:#?}"
    );
}

/// TS-09, audit m2: a segment gets its name only once complete and synced.
/// Its zeros are written to `wal-temp` and synced, then it is renamed and
/// the directory synced (PostgreSQL `xlogtemp` and `durable_rename`); no
/// segment name is ever created directly.
#[test]
fn a_segment_gets_its_name_only_once_complete_and_synced() {
    use crate::storage::fault::Call;
    use crate::storage::wal::{TEMP_SEGMENT, parse_segment_name};
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Engine::create(d, fault.clone(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    fill_wal_to(&e, tree, &mut model, SEGMENT_SPAN + FRAME_BYTES);
    drop(e);
    let is_segment = |p: &Path| p.file_name().and_then(parse_segment_name).is_some();
    let is_temp = |p: &Path| p.file_name().is_some_and(|n| n == TEMP_SEGMENT);
    let calls = fault.calls();
    let mut installed = 0;
    let mut temp_synced = false;
    for (n, c) in calls.iter().enumerate() {
        match c {
            Call::Create(p) => {
                assert!(!is_segment(p), "a segment name was created directly: {p:?}");
                if is_temp(p) {
                    temp_synced = false;
                }
            }
            Call::WriteAt { path, .. } if is_temp(path) => temp_synced = false,
            Call::SyncData(p) if is_temp(p) => temp_synced = true,
            Call::Rename(a, b) if is_temp(a) => {
                assert!(is_segment(b), "{b:?}");
                assert!(
                    temp_synced,
                    "{b:?} was installed before its zeros were synced"
                );
                // The writer may log calls of its own meanwhile (the
                // creation runs on the checkpointer thread, task #15 D4):
                // the next change of the namespace is the sync_dir, and the
                // new segment is not written before it.
                let next = calls[n + 1..].iter().find(|c| {
                    matches!(
                        c,
                        Call::SyncDir(_) | Call::Create(_) | Call::Rename(..) | Call::Remove(_)
                    ) || matches!(c, Call::WriteAt { path, .. } if path == b)
                });
                assert!(
                    matches!(next, Some(Call::SyncDir(_))),
                    "the rename to {b:?} is not followed by sync_dir: {next:?}"
                );
                installed += 1;
            }
            _ => {}
        }
    }
    assert!(installed >= 2, "{installed} segments installed");
    fault.crash(Crash::DropAll).unwrap();
    let e = Engine::open(d, fault.clone(), CAP, sink()).unwrap();
    assert_state(&e, tree, &model);
}

/// Audit m2: a crash during a segment's creation can leave `wal-temp`,
/// partly written, with a durable name (`FaultStorage` cannot make a name
/// durable without `sync_dir`, so the test writes that state itself). Open
/// removes it and recovers as if the creation never started.
#[test]
fn open_removes_a_partial_segment_left_by_a_crash() {
    use crate::storage::wal::TEMP_SEGMENT;
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let e = Engine::create(d, fs(), CAP, sink()).unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let mut model = Model::new();
    commit_kv(&e, tree, &mut model, b"a", b"1");
    drop(e);
    let f = FsStorage.create(&d.join(TEMP_SEGMENT)).unwrap();
    f.write_at(&vec![0u8; 3 << 20], 0).unwrap();
    f.sync_data().unwrap();
    FsStorage.sync_dir(d).unwrap();
    let e = Engine::open(d, fs(), CAP, sink()).unwrap();
    assert!(
        !names(d).iter().any(|n| n == TEMP_SEGMENT),
        "{:?}",
        names(d)
    );
    assert_eq!(e.wal().segment_lsns(), [0]);
    assert_state(&e, tree, &model);
    commit_kv(&e, tree, &mut model, b"b", b"1");
    drop(e);
    let e = Engine::open(d, fs(), CAP, sink()).unwrap();
    assert_state(&e, tree, &model);
}
