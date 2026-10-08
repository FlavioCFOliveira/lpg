//! MVCC (TS-14, rmp #27): snapshots, private write sets, old versions,
//! before-images in commit step 6, memory caps.

use super::*;
use crate::storage::error::Error;
use crate::sync::{Mutex, mpsc};

/// While a commit is paused after its apply step, under the commit latch
/// (TS-16 between steps 6 and 7), a reader whose snapshot is older reads
/// none of its changes, and so does a reader that begins during the pause,
/// since nothing is published yet; once the commit returns, a new
/// transaction reads all of them (FR-TX-001, FR-TX-006).
#[test]
fn a_paused_commit_is_invisible_until_published_then_wholly_visible() {
    let (_d, e) = db();
    let t = tree(&e);
    let keys: Vec<String> = (0..40).map(|i| format!("k{i:02}")).collect();
    {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for k in &keys {
            tx.put(t, k.as_bytes(), b"old").unwrap();
        }
        tx.commit().unwrap();
    }
    let mut early = e.begin(Isolation::Serializable).unwrap();
    let before = all(&mut early, t);
    assert_eq!(before.len(), 40);
    let (paused_tx, paused) = mpsc::channel::<()>();
    let (go, go_rx) = mpsc::channel::<()>();
    let (paused_tx, go_rx) = (Mutex::new(paused_tx), Mutex::new(go_rx));
    *e.wal().points.after_apply.lock().unwrap() = Some(Box::new(move || {
        paused_tx.lock().unwrap().send(()).unwrap();
        go_rx.lock().unwrap().recv().unwrap();
    }));
    std::thread::scope(|s| {
        let w = s.spawn(|| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            for (i, k) in keys.iter().enumerate() {
                match i % 4 {
                    0 => {
                        tx.delete(t, k.as_bytes()).unwrap();
                    }
                    _ => tx.put(t, k.as_bytes(), b"new value").unwrap(),
                }
            }
            tx.put(t, b"z-created", b"new").unwrap();
            tx.commit().unwrap().unwrap()
        });
        paused.recv().unwrap();
        // The commit is applied to the pages but not published. What the
        // readers see is collected first and asserted after the commit is
        // released, so a failure cannot leave it paused.
        let scanned = all(&mut early, t);
        let points: Vec<Option<Vec<u8>>> = before
            .iter()
            .map(|(k, _)| early.get(t, k).unwrap())
            .collect();
        let created = early.get(t, b"z-created").unwrap();
        let mut during = e.begin(Isolation::Snapshot).unwrap();
        let during_snapshot = during.snapshot();
        let during_scan = all(&mut during, t);
        drop(during);
        go.send(()).unwrap();
        let ts = w.join().unwrap();
        assert_eq!(scanned, before);
        let want: Vec<Option<Vec<u8>>> = before.iter().map(|(_, v)| Some(v.clone())).collect();
        assert_eq!(points, want);
        assert_eq!(created, None);
        assert_eq!(during_snapshot, early.snapshot());
        assert_eq!(during_scan, before);
        assert_eq!(ts, early.snapshot() + 1);
    });
    *e.wal().points.after_apply.lock().unwrap() = None;
    // The early snapshot still reads the old state, from old versions.
    assert_eq!(all(&mut early, t), before);
    early.commit().unwrap();
    let mut late = e.begin(Isolation::Serializable).unwrap();
    let after = all(&mut late, t);
    let mut want: Vec<(Vec<u8>, Vec<u8>)> = keys
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 4 != 0)
        .map(|(_, k)| (k.as_bytes().to_vec(), b"new value".to_vec()))
        .collect();
    want.push((b"z-created".to_vec(), b"new".to_vec()));
    assert_eq!(after, want);
}

/// A key updated by three commits, read at four snapshots (one before
/// each commit and one after the last), returns each version exactly once,
/// by point read and by scan.
#[test]
fn a_key_updated_three_times_reads_each_version_at_its_snapshot() {
    let (_d, e) = db();
    let t = tree(&e);
    pause_gc(&e);
    let mut snaps = vec![e.begin(Isolation::Snapshot).unwrap()];
    for v in [b"v1", b"v2", b"v3"] {
        commit_puts(&e, t, &[(b"k", v)]);
        snaps.push(e.begin(Isolation::Snapshot).unwrap());
    }
    let seen: Vec<Option<Vec<u8>>> = snaps
        .iter_mut()
        .map(|tx| tx.get(t, b"k").unwrap())
        .collect();
    assert_eq!(
        seen,
        vec![
            None,
            Some(b"v1".to_vec()),
            Some(b"v2".to_vec()),
            Some(b"v3".to_vec())
        ]
    );
    for (tx, want) in snaps.iter_mut().zip(&seen) {
        let scanned: Vec<Vec<u8>> = all(tx, t).into_iter().map(|(_, v)| v).collect();
        assert_eq!(scanned, want.iter().cloned().collect::<Vec<_>>());
    }
    // Two replaced versions are held for the older snapshots.
    assert_eq!(e.core().mvcc.versions.len(), 2);
}

/// A transaction reads its own writes; no other transaction does; an abort
/// leaves no trace: no page change, no old version, no lock, no memory.
#[test]
fn own_writes_are_private_and_an_abort_leaves_no_trace() {
    let (_d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2")]);
    pause_gc(&e);
    let pages = raw(&e, t);
    let used = e.accountant().used();
    let mut w = e.begin(Isolation::Serializable).unwrap();
    w.put(t, b"a", b"10").unwrap();
    w.put(t, b"c", b"30").unwrap();
    assert!(w.delete(t, b"b").unwrap());
    assert_eq!(w.get(t, b"a").unwrap(), Some(b"10".to_vec()));
    assert_eq!(w.get(t, b"b").unwrap(), None);
    assert_eq!(all(&mut w, t), kv(&[("a", "10"), ("c", "30")]));
    let mut other = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(all(&mut other, t), kv(&[("a", "1"), ("b", "2")]));
    other.commit().unwrap();
    assert!(e.accountant().used() > used, "the write set is charged");
    w.abort();
    assert_eq!(raw(&e, t), pages);
    assert_eq!(e.core().mvcc.versions.len(), 0);
    assert!(e.core().mvcc.locks.is_empty());
    assert_eq!(e.accountant().used(), used);
    let mut after = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(all(&mut after, t), kv(&[("a", "1"), ("b", "2")]));
}

/// A transaction that wrote nothing commits without drawing a commit
/// timestamp: nothing is published or logged (TS-16 step 1, FR-TX-010).
#[test]
fn a_read_only_transaction_draws_no_commit_timestamp() {
    let (_d, e) = db();
    let t = tree(&e);
    let ts = commit_puts(&e, t, &[(b"a", b"1")]);
    let end = e.store().lock().log_ref().group.end();
    let mut r = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(r.get(t, b"a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(r.commit().unwrap(), None);
    let st = e.store().lock();
    assert_eq!(st.log_ref().counters.commit_ts, ts);
    assert_eq!(st.log_ref().group.end(), end);
    drop(st);
    assert_eq!(e.wal().published(), ts);
    assert_eq!(commit_puts(&e, t, &[(b"a", b"2")]), ts + 1);
}

/// FR-LIM-005: a long reader holds old versions while a writer replaces
/// large values until the cap is nearly used. The writer's operation that
/// needs memory beyond the cap fails with `MemoryLimit` and aborts its
/// transaction; the reader still reads its whole snapshot; the accountant
/// never exceeded the cap; the failed operation changed no page.
#[test]
fn old_versions_at_the_cap_fail_only_the_writer() {
    const SMALL: u64 = 8 << 20;
    let (_d, e) = db_with_cap(SMALL);
    let t = tree(&e);
    pause_gc(&e);
    let big = vec![7u8; 20_000];
    let keys: Vec<Vec<u8>> = (0..40u32).map(|i| i.to_be_bytes().to_vec()).collect();
    {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for k in &keys {
            tx.put(t, k, &big).unwrap();
        }
        tx.commit().unwrap();
    }
    let mut reader = e.begin(Isolation::Snapshot).unwrap();
    let snapshot = all(&mut reader, t);
    let mut round = 0u8;
    let mut committed = snapshot.clone();
    let failure = 'fill: loop {
        round += 1;
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let v = vec![round; 20_000];
        for k in &keys {
            if let Err(err) = tx.put(t, k, &v) {
                let pages = e.data().page_count();
                // The transaction is aborted: every later operation fails.
                assert!(matches!(tx.get(t, k), Err(Error::Aborted)));
                assert!(matches!(tx.commit(), Err(Error::Aborted)));
                assert_eq!(e.data().page_count(), pages);
                break 'fill err;
            }
        }
        // The commit's page copies also need memory (TS-16 step 6): a
        // refusal there fails the commit, which is undone.
        if let Err(err) = tx.commit() {
            let mut check = e.begin(Isolation::Snapshot).unwrap();
            assert_eq!(all(&mut check, t), committed);
            break 'fill err;
        }
        committed = keys.iter().map(|k| (k.clone(), v.clone())).collect();
        assert!(round < 200, "the cap was never reached");
    };
    assert!(matches!(failure, Error::MemoryLimit { .. }), "{failure:?}");
    assert!(e.core().mvcc.versions.len() >= 40);
    assert!(
        e.accountant().peak() <= SMALL,
        "peak {}",
        e.accountant().peak()
    );
    // The reader is not aborted and reads its snapshot.
    assert_eq!(all(&mut reader, t), snapshot);
    for (k, v) in &snapshot {
        assert_eq!(reader.get(t, k).unwrap().as_ref(), Some(v));
    }
    reader.commit().unwrap();
    // No other transaction was aborted: new ones run.
    commit_puts(&e, t, &[(b"after", b"1")]);
}

/// FR-LIM-007: a transaction whose writes exceed the cap fails at the
/// write that does not fit; nothing it wrote is visible and its memory is
/// returned.
#[test]
fn a_transaction_beyond_the_cap_fails_and_changes_nothing() {
    const SMALL: u64 = 8 << 20;
    let (_d, e) = db_with_cap(SMALL);
    let t = tree(&e);
    commit_puts(&e, t, &[(b"keep", b"1")]);
    let used = e.accountant().used();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let v = vec![1u8; 100_000];
    let mut written = 0u32;
    let err = loop {
        match tx.put(t, &written.to_be_bytes(), &v) {
            Ok(()) => written += 1,
            Err(e) => break e,
        }
        assert!(written < 1000, "the cap was never reached");
    };
    assert!(matches!(err, Error::MemoryLimit { .. }), "{err:?}");
    assert!(written > 10);
    drop(tx);
    assert!(e.accountant().used() <= used);
    assert!(e.accountant().peak() <= SMALL);
    let mut r = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(all(&mut r, t), kv(&[("keep", "1")]));
}

/// A page-copy failure in step 6 (the `FailCopy` arm of the fault suite)
/// is undone: no entry carries the commit's stamp, no old version is left,
/// the write keys are withdrawn, and the next commit takes the same
/// timestamp (TS-16 step 6 undo).
#[test]
fn a_failed_page_copy_leaves_no_stamp_and_no_old_version() {
    let (_d, e) = db();
    let t = tree(&e);
    let ts = commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2")]);
    pause_gc(&e);
    let pages = raw(&e, t);
    let sets = e.core().mvcc.committed.len();
    let pool = e.pool();
    let n = pool.copy_calls.load(crate::sync::atomic::Ordering::SeqCst);
    pool.fail_copy
        .store(n + 1, crate::sync::atomic::Ordering::SeqCst);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.put(t, b"a", b"10").unwrap();
    tx.delete(t, b"b").unwrap();
    tx.put(t, b"c", b"3").unwrap();
    let err = tx.commit().unwrap_err();
    assert!(matches!(err, Error::MemoryLimit { .. }), "{err:?}");
    assert_eq!(raw(&e, t), pages);
    assert!(
        raw(&e, t)
            .iter()
            .all(|(_, _, s)| s.created <= ts && s.deleted == 0)
    );
    assert_eq!(e.core().mvcc.versions.len(), 0);
    assert_eq!(e.core().mvcc.committed.len(), sets);
    assert!(e.core().mvcc.locks.is_empty());
    e.check().unwrap();
    assert_eq!(commit_puts(&e, t, &[(b"a", b"11")]), ts + 1);
    assert_eq!(read(&e, t, b"a"), Some(b"11".to_vec()));
}

/// After a drop without close, reopen shows exactly the committed entries,
/// stamped, and an empty old-version store (TS-13 step 7), even though
/// old versions existed when the engine was dropped.
#[test]
fn reopen_after_a_drop_shows_the_committed_stamped_entries_and_no_old_version() {
    let d = graph_dir();
    let t;
    let (t1, t2, t3);
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
        t1 = commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
        let mut old = e.begin(Isolation::Snapshot).unwrap();
        t2 = commit_puts(&e, t, &[(b"a", b"10")]);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.delete(t, b"b").unwrap();
        t3 = tx.commit().unwrap().unwrap();
        assert_eq!(old.get(t, b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(e.core().mvcc.versions.len(), 1);
        // An uncommitted transaction is lost with the drop.
        let mut lost = e.begin(Isolation::Serializable).unwrap();
        lost.put(t, b"d", b"4").unwrap();
        drop(lost);
        drop(old);
    }
    let e = reopen(d.path());
    assert!(!e.recovered().clean);
    assert_eq!(e.core().mvcc.versions.len(), 0);
    let mut r = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(all(&mut r, t), kv(&[("a", "10"), ("c", "3")]));
    r.commit().unwrap();
    // The entries carry their stamps (`b` until garbage collection removes
    // it); a created stamp may have been frozen by a later rewrite.
    let stamps: Vec<(Vec<u8>, Stamps)> = raw(&e, t)
        .into_iter()
        .filter(|(k, _, _)| k.as_slice() != b"b")
        .map(|(k, _, s)| (k, s))
        .collect();
    assert_eq!(stamps.len(), 2);
    for (k, s) in stamps {
        let want = if k == b"a" { t2 } else { t1 };
        assert!(s.created == want || s.created == 0, "{k:?} {s:?}");
        assert_eq!(s.deleted, 0);
    }
    assert!(t3 > t2);
    e.close().unwrap();
}
