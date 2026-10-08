//! Garbage collection and page reuse (TS-14, TS-05, rmp #29). The tests
//! pause the collector's own passes and run [`crate::tx::gc::pass`] at
//! chosen points.

use super::*;
use crate::storage::btree::DIRECTORY;
use crate::storage::btree::node;
use crate::storage::error::Error;
use crate::storage::fault::Crash;
use crate::storage::page::{self, PageType};
use crate::tx::gc::pass;

fn gc(e: &Engine) -> crate::tx::gc::Pass {
    pass(e.core()).unwrap()
}

fn check(e: &Engine) {
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
}

fn free_pages(e: &Engine) -> Vec<u64> {
    e.store().lock().free_list().pages().collect()
}

/// The leaves of `t`, each as its entries' keys and stamps, decoded from
/// the pages.
fn leaves(e: &Engine, t: Tree) -> Vec<Vec<(Vec<u8>, Stamps)>> {
    let mut out = Vec::new();
    let mut stack = vec![t.root()];
    while let Some(p) = stack.pop() {
        let pin = e.pool().fetch(p).unwrap();
        let g = pin.read().unwrap();
        match page::page_type(&g) {
            Some(PageType::Inner) => {
                stack.push(node::leftmost(&g));
                node::inner_entries(&g, |_, c| stack.push(c)).unwrap();
            }
            Some(PageType::Leaf) => {
                let mut l = Vec::new();
                node::leaf_entries(&g, |_, en| l.push((en.key.to_vec(), en.stamps))).unwrap();
                out.push(l);
            }
            other => panic!("page {p} of tree {} is {other:?}", t.id()),
        }
    }
    out
}

/// An old version a running snapshot needs is never freed; once that
/// transaction ends, a pass frees it and the accountant returns exactly
/// its bytes.
#[test]
fn an_old_version_is_kept_while_a_snapshot_needs_it_and_freed_after() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    commit_puts(&e, t, &[(b"k", b"v1")]);
    let mut r = e.begin(Isolation::Snapshot).unwrap();
    commit_puts(&e, t, &[(b"k", b"v2")]);
    let versions = &e.core().mvcc.versions;
    assert_eq!(versions.len(), 1);
    let held = versions.bytes();
    assert!(held > 0);
    for _ in 0..3 {
        let p = gc(&e);
        assert_eq!(p.versions, 0);
        assert!(p.horizon <= r.snapshot());
        assert_eq!(r.get(t, b"k").unwrap(), Some(b"v1".to_vec()));
    }
    r.commit().unwrap();
    let before = e.accountant().used();
    assert_eq!(gc(&e).versions, 1);
    assert_eq!(versions.len(), 0);
    assert_eq!(versions.bytes(), 0);
    assert_eq!(before - e.accountant().used(), held);
}

/// The collector's thread runs a pass after commits: with no snapshot left,
/// the old version a commit created is freed without a pass by the test.
#[test]
fn the_collector_frees_old_versions_after_commits() {
    let (_d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"k", b"v1")]);
    commit_puts(&e, t, &[(b"k", b"v2")]);
    // Woken by the commits; polled without timing.
    crate::test_support::wait_until("the collector frees the old version", || {
        e.core().mvcc.versions.len() == 0
    });
    assert_eq!(read(&e, t, b"k"), Some(b"v2".to_vec()));
}

/// A delete-stamped entry is removed physically only after every older
/// snapshot has ended, and the integrity check passes after each pass.
#[test]
fn a_deleted_entry_is_removed_only_after_older_snapshots_end() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    commit_puts(&e, t, &[(b"a", b"1"), (b"k", b"2")]);
    let mut r = e.begin(Isolation::Snapshot).unwrap();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    assert!(tx.delete(t, b"k").unwrap());
    let d = tx.commit().unwrap().unwrap();
    for _ in 0..2 {
        assert_eq!(gc(&e).removed, 0);
        check(&e);
        let k = raw(&e, t).into_iter().find(|(k, _, _)| k == b"k").unwrap();
        assert_eq!(k.2.deleted, d);
        assert_eq!(r.get(t, b"k").unwrap(), Some(b"2".to_vec()));
    }
    assert_eq!(read(&e, t, b"k"), None);
    r.commit().unwrap();
    let p = gc(&e);
    assert_eq!(p.removed, 1);
    assert!(p.group.is_some());
    check(&e);
    assert_eq!(raw(&e, t).len(), 1);
}

/// TS-05 Reuse with MVCC: a commit's delete frees no page, so a page whose
/// entries an older snapshot still reads is never freed or reused while it
/// runs. Once it ends, the collector removes the entries, frees the emptied
/// leaves, and a later commit reuses them.
#[test]
fn pages_are_freed_and_reused_only_after_older_snapshots_end() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    let v = vec![3u8; 200];
    let keys: Vec<[u8; 4]> = (0..400u32).map(u32::to_be_bytes).collect();
    {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for k in &keys {
            tx.put(t, k, &v).unwrap();
        }
        tx.commit().unwrap();
    }
    assert!(leaves(&e, t).len() > 5);
    let mut r = e.begin(Isolation::Snapshot).unwrap();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    for k in &keys {
        tx.delete(t, k).unwrap();
    }
    tx.commit().unwrap();
    let free = free_pages(&e);
    assert_eq!(gc(&e).removed, 0);
    assert_eq!(
        free_pages(&e),
        free,
        "no page freed while the snapshot runs"
    );
    assert_eq!(all(&mut r, t).len(), 400);
    r.commit().unwrap();
    assert_eq!(gc(&e).removed, 400);
    check(&e);
    let freed: Vec<u64> = free_pages(&e)
        .into_iter()
        .filter(|p| !free.contains(p))
        .collect();
    assert!(freed.len() >= 5, "{freed:?}");
    // A later commit takes freed pages instead of growing the file.
    let count = e.data().page_count();
    {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for k in &keys {
            tx.put(t, k, &v).unwrap();
        }
        tx.commit().unwrap();
    }
    assert_eq!(e.data().page_count(), count);
    let reused = freed.iter().filter(|p| !free_pages(&e).contains(p)).count();
    assert!(
        reused >= 5,
        "{reused} of {} freed pages reused",
        freed.len()
    );
    check(&e);
}

/// A WAL group that holds only a garbage-collection batch becomes durable
/// without a commit, records an empty range of commit timestamps, and is
/// replayed by recovery: after a crash that drops every unsynced write of
/// the data file, the removed entries are still gone.
#[test]
fn a_group_of_only_collector_changes_is_durable_and_replayed() {
    let d = graph_dir();
    let f = crate::tx::commit::tests::fault();
    let t;
    {
        let e = Engine::create_tuned(
            d.path(),
            f.clone(),
            CAP,
            Arc::new(RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        pause_gc(&e);
        t = tree(&e);
        commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
        e.checkpoint().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.delete(t, b"a").unwrap();
        tx.delete(t, b"c").unwrap();
        let ts = tx.commit().unwrap().unwrap();
        let from = e.wal().durable_lsn();
        let p = gc(&e);
        assert_eq!(p.removed, 2);
        let seq = p.group.expect("the batch joined a group");
        e.wal().wait_durable(seq).unwrap();
        let mut scan = crate::storage::wal::tests::DurableScan::new(d.path(), 1, from);
        scan.advance();
        let g = scan
            .groups
            .last()
            .expect("the collector's group is durable");
        assert!(!g.pages.is_empty());
        assert!(
            g.record.first_ts > g.record.counters.commit_ts,
            "{:?}",
            g.record
        );
        assert_eq!(g.record.counters.commit_ts, ts);
        drop(e);
    }
    f.crash(Crash::DropAll).unwrap();
    let e = Engine::open_tuned(
        d.path(),
        f.clone(),
        CAP,
        Arc::new(RecordingSink::default()),
        tuning(),
    )
    .unwrap();
    assert!(e.recovered().groups >= 2);
    let keys: Vec<Vec<u8>> = raw(&e, t).into_iter().map(|(k, _, _)| k).collect();
    assert_eq!(keys, vec![b"b".to_vec()]);
    check(&e);
    e.close().unwrap();
}

/// After a leaf is rewritten, it holds no `created_ts` at or below the
/// horizon of the rewrite: decoded from the page, every entry the rewrite
/// kept is frozen, and only the newer entry carries its stamp. While an
/// older snapshot runs, the horizon is below the stamps, which stay.
#[test]
fn a_rewritten_leaf_holds_no_created_stamp_at_or_below_the_horizon() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    let keys: Vec<[u8; 2]> = (0..50u16).map(u16::to_be_bytes).collect();
    // An older snapshot keeps the horizon below ts1.
    let old = e.begin(Isolation::Snapshot).unwrap();
    let ts1 = {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for k in &keys {
            tx.put(t, k, b"x").unwrap();
        }
        tx.commit().unwrap().unwrap()
    };
    let leaf = |e: &Engine| {
        let l = leaves(e, t);
        assert_eq!(l.len(), 1, "one root leaf");
        l.into_iter().next().unwrap()
    };
    assert!(leaf(&e).iter().all(|(_, s)| s.created == ts1));
    assert!(old.snapshot() < ts1);
    let ts2 = commit_puts(&e, t, &[(b"zz", b"y")]);
    let l = leaf(&e);
    assert!(
        l.iter()
            .filter(|(k, _)| k != b"zz")
            .all(|(_, s)| s.created == ts1)
    );
    drop(old);
    // No snapshot is left: the horizon of the next rewrite is ts2.
    let horizon = e.wal().published();
    assert_eq!(horizon, ts2);
    let ts3 = commit_puts(&e, t, &[(b"zy", b"z")]);
    let l = leaf(&e);
    for (k, s) in &l {
        assert!(s.created == 0 || s.created > horizon, "{k:?} {s:?}");
    }
    let stamped: Vec<&Vec<u8>> = l
        .iter()
        .filter(|(_, s)| s.created != 0)
        .map(|(k, _)| k)
        .collect();
    assert_eq!(stamped, vec![&b"zy".to_vec()]);
    assert!(l.iter().any(|(k, s)| k == b"zy" && s.created == ts3));
    // Frozen entries are visible to every snapshot.
    let mut r = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(all(&mut r, t).len(), 52);
}

/// The pages of a dropped tree return to the free list once the horizon
/// passes the drop: not while an older snapshot can still read the tree,
/// which it does; then its directory entry is gone, its pages are free, the
/// integrity check passes, and a later snapshot finds the tree dropped.
#[test]
fn the_pages_of_a_dropped_tree_are_freed_after_the_horizon_passes() {
    let (_d, e) = db();
    let t = tree(&e);
    let other = tree(&e);
    pause_gc(&e);
    let v = vec![9u8; 3000]; // overflow chains too
    {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for i in 0..300u32 {
            tx.put(
                t,
                &i.to_be_bytes(),
                &v[..if i % 10 == 0 { 3000 } else { 100 }],
            )
            .unwrap();
        }
        tx.put(other, b"keep", b"1").unwrap();
        tx.commit().unwrap();
    }
    let tree_pages = leaves(&e, t).len();
    assert!(tree_pages > 3);
    let mut r = e.begin(Isolation::Snapshot).unwrap();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.drop_tree(t).unwrap();
    let d = tx.commit().unwrap().unwrap();
    assert_eq!(e.store().tree_status(t.id()).unwrap(), Some((t, d)));
    let free = free_pages(&e);
    assert_eq!(gc(&e).trees, 0);
    assert_eq!(free_pages(&e), free);
    assert_eq!(all(&mut r, t).len(), 300);
    check(&e);
    // A snapshot at or after the drop cannot use the tree.
    let mut late = e.begin(Isolation::Serializable).unwrap();
    assert!(matches!(late.get(t, b"x"), Err(Error::TreeDropped(_))));
    drop(late);
    r.commit().unwrap();
    let p = gc(&e);
    assert_eq!(p.trees, 1);
    assert_eq!(e.store().tree_status(t.id()).unwrap(), None);
    let freed = free_pages(&e).len() - free.len();
    assert!(freed > tree_pages, "{freed} pages freed");
    check(&e);
    assert_eq!(read(&e, other, b"keep"), Some(b"1".to_vec()));
    let mut dir = Vec::new();
    e.store()
        .scan_prefix(DIRECTORY, b"", |k, _| {
            dir.push(k.to_vec());
            true
        })
        .unwrap();
    assert!(!dir.contains(&t.id().to_be_bytes().to_vec()));
}

/// A clean close runs a last pass: the next open finds no delete-stamped
/// entry and no dropped tree left (TS-14; `tx::gc` work queues).
#[test]
fn a_clean_close_leaves_no_garbage_behind() {
    let d = graph_dir();
    let (t, gone);
    {
        let e = Engine::create_tuned(
            d.path(),
            no_flush(),
            CAP,
            Arc::new(RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        pause_gc(&e);
        t = tree(&e);
        gone = tree(&e);
        commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2")]);
        commit_puts(&e, gone, &[(b"x", b"1")]);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.delete(t, b"a").unwrap();
        tx.drop_tree(gone).unwrap();
        tx.commit().unwrap();
        e.close().unwrap();
    }
    let e = reopen(d.path());
    assert!(e.recovered().clean);
    assert_eq!(raw(&e, t).len(), 1);
    assert_eq!(e.store().tree_status(gone.id()).unwrap(), None);
    check(&e);
    e.close().unwrap();
}

/// After a crash, the collector's first pass finds the delete stamps and
/// dropped trees its in-memory queues lost, by scanning every tree.
#[test]
fn after_a_crash_the_collector_finds_the_garbage_again() {
    let d = graph_dir();
    let (t, gone);
    {
        let e = Engine::create_tuned(
            d.path(),
            no_flush(),
            CAP,
            Arc::new(RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        pause_gc(&e);
        t = tree(&e);
        gone = tree(&e);
        commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2")]);
        commit_puts(&e, gone, &[(b"x", b"1")]);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.delete(t, b"a").unwrap();
        tx.drop_tree(gone).unwrap();
        tx.commit().unwrap();
        drop(e);
    }
    let e = reopen(d.path());
    assert!(!e.recovered().clean);
    // The thread's seed scan and first pass, observed without timing.
    crate::test_support::wait_until(
        "the collector frees the dropped tree and the deleted entry",
        || e.store().tree_status(gone.id()).unwrap().is_none() && raw(&e, t).len() == 1,
    );
    check(&e);
    e.close().unwrap();
}

/// While a database is open, its threads include `lpg-gc` (TS-02). A
/// spawned thread sets its own name when it starts, so the test polls for
/// it (as `the_wal_writer_and_checkpointer_threads_are_named`); after close
/// the thread is gone.
#[test]
fn the_collector_thread_runs_while_the_database_is_open() {
    let (_d, e) = db();
    crate::test_support::wait_until("the lpg-gc thread is named", || {
        crate::test_support::thread_names()
            .iter()
            .any(|n| n == "lpg-gc")
    });
    e.close().unwrap();
    let names = crate::test_support::thread_names();
    assert!(!names.iter().any(|n| n == "lpg-gc"), "{names:?}");
}

/// Commits 400 entries into `t` (several leaves), then drops `t`, with
/// the collector paused.
fn fill_and_drop(e: &Engine, t: Tree) {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    for i in 0..400u32 {
        tx.put(t, &i.to_be_bytes(), &[7u8; 100]).unwrap();
    }
    tx.commit().unwrap();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.drop_tree(t).unwrap();
    tx.commit().unwrap();
}

/// Every page of the free list is listed once.
fn assert_free_list_distinct(e: &Engine) {
    let free = free_pages(e);
    let mut s = free.clone();
    s.sort_unstable();
    s.dedup();
    assert_eq!(free.len(), s.len(), "a page is in the free list twice");
}

/// The seed scan of a crash-open runs on `lpg-gc` while commits queue
/// their work, so a dropped tree and a delete-stamped entry can be queued
/// twice (audit Round A, P1: "page 2 is in two places: Free and Free"). The
/// seed runs here after the commits, as when it reaches the directory
/// after them; one pass collects each once.
#[test]
fn a_tree_and_an_entry_queued_twice_are_collected_once() {
    let (_d, e) = db();
    let t = tree(&e);
    let u = tree(&e);
    pause_gc(&e);
    commit_puts(&e, u, &[(b"a", b"1"), (b"b", b"2")]);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.delete(u, b"a").unwrap();
    tx.commit().unwrap();
    fill_and_drop(&e, t);
    crate::tx::gc::seed(e.core()).unwrap();
    {
        let q = e.core().mvcc.gc();
        assert_eq!(q.dropped.len(), 2);
        assert_eq!(q.deleted.len(), 2);
    }
    let p = gc(&e);
    assert_eq!((p.trees, p.removed), (1, 1));
    assert_eq!(e.store().tree_status(t.id()).unwrap(), None);
    assert_eq!(raw(&e, u).len(), 1);
    check(&e);
    assert_free_list_distinct(&e);
}

/// A batch that frees the same dropped tree twice frees its pages once:
/// the second free finds the directory entry gone and does nothing (audit
/// Round A, P1). Without the WAL: the database is dropped afterwards.
#[test]
fn freeing_a_dropped_tree_twice_in_one_batch_frees_its_pages_once() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    fill_and_drop(&e, t);
    let store = e.store();
    let mut st = store.lock();
    store.apply_internal(&mut st, None, &[], &[t, t]).unwrap();
    drop(st);
    assert_eq!(e.store().tree_status(t.id()).unwrap(), None);
    check(&e);
    assert_free_list_distinct(&e);
}

/// A key deleted, then put again, is kept by the pass that finds its old
/// delete stamp queued (audit Round A, M11): the pass removes an entry
/// only if it still carries a delete stamp at or below the horizon.
#[test]
fn a_key_put_again_after_its_delete_survives_the_pass() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    commit_puts(&e, t, &[(b"k", b"1")]);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.delete(t, b"k").unwrap();
    tx.commit().unwrap();
    commit_puts(&e, t, &[(b"k", b"2")]);
    assert_eq!(e.core().mvcc.gc().deleted.len(), 1);
    let p = gc(&e);
    assert_eq!(p.removed, 0);
    assert_eq!(read(&e, t, b"k"), Some(b"2".to_vec()));
    check(&e);
}

/// A crashed database with one delete-stamped entry left in `t`.
fn crashed_with_garbage(d: &std::path::Path) -> Tree {
    let e = Engine::create_tuned(
        d,
        no_flush(),
        CAP,
        Arc::new(RecordingSink::default()),
        tuning(),
    )
    .unwrap();
    pause_gc(&e);
    let t = tree(&e);
    commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2")]);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.delete(t, b"a").unwrap();
    tx.commit().unwrap();
    drop(e);
    t
}

/// A failed seed scan keeps the clean-shutdown flag clear (audit Round
/// A): the close after it completes, the next open is unclean and its
/// seed scan finds the garbage. Fail point: `gc::FAIL_SEED`.
#[test]
fn a_failed_seed_scan_keeps_the_clean_flag_clear() {
    let d = graph_dir();
    let t = crashed_with_garbage(d.path());
    let fail = || crate::tx::gc::FAIL_SEED.lock().unwrap();
    fail().push(d.path().to_path_buf());
    let e = reopen(d.path());
    assert!(!e.recovered().clean);
    e.close().unwrap();
    fail().retain(|p| p != d.path());
    let e = reopen(d.path());
    assert!(!e.recovered().clean, "the flag was set over a failed seed");
    crate::test_support::wait_until("the collector removes the deleted entry", || {
        raw(&e, t).len() == 1
    });
    check(&e);
    e.close().unwrap();
    assert!(reopen(d.path()).recovered().clean);
}

/// A last pass that fails at close with the database usable (here
/// `MemoryLimit` from the page-copy fail point, TS-30 R10) does not fail
/// the close (audit Round A); the flag stays clear, and the next open's
/// seed scan finds the garbage.
#[test]
fn a_failed_last_pass_still_closes_with_the_clean_flag_clear() {
    let d = graph_dir();
    let t;
    {
        let e = Engine::create_tuned(
            d.path(),
            no_flush(),
            CAP,
            Arc::new(RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        pause_gc(&e);
        t = tree(&e);
        commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2")]);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.delete(t, b"a").unwrap();
        tx.commit().unwrap();
        let pool = e.pool();
        pool.fail_copy.store(
            pool.copy_calls.load(crate::sync::atomic::Ordering::SeqCst) + 1,
            crate::sync::atomic::Ordering::SeqCst,
        );
        e.close().unwrap();
    }
    let e = reopen(d.path());
    assert!(!e.recovered().clean);
    crate::test_support::wait_until("the collector removes the deleted entry", || {
        raw(&e, t).len() == 1
    });
    check(&e);
    e.close().unwrap();
}
