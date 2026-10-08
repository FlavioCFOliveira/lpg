//! Isolation and locks (TS-15, TS-16, rmp #28): serializable validation,
//! snapshot isolation, lock waits, deadlocks. Interleavings are set by lock
//! waits observed in the lock table and by pause points, never by sleeps.

use super::*;
use crate::obs::names::transactions::{COMMITTED, FAILED_COMMITS};
use crate::storage::error::Error;
use crate::sync::{Mutex, mpsc};
use crate::tx::conflict::WriteKeys;
use crate::tx::lock::{Mode, Resource};

/// Write skew (FR-TX-004, FR-TX-008): T1 and T2 both read x and y; T1
/// writes x, T2 writes y. Under serializable isolation exactly one
/// commits, the other fails with `Serialization` (GQLSTATUS 25N11) and its
/// write is never visible; under snapshot isolation both commit.
#[test]
fn write_skew_fails_one_transaction_under_serializable_and_none_under_snapshot() {
    for iso in [Isolation::Serializable, Isolation::Snapshot] {
        let (_d, e) = db();
        let t = tree(&e);
        commit_puts(&e, t, &[(b"x", b"1"), (b"y", b"1")]);
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        for tx in [&mut t1, &mut t2] {
            assert_eq!(tx.get(t, b"x").unwrap(), Some(b"1".to_vec()));
            assert_eq!(tx.get(t, b"y").unwrap(), Some(b"1".to_vec()));
        }
        t1.put(t, b"x", b"0").unwrap();
        t2.put(t, b"y", b"0").unwrap();
        t1.commit().unwrap();
        let r2 = t2.commit();
        match iso {
            Isolation::Serializable => {
                let err = r2.unwrap_err();
                assert!(matches!(err, Error::Serialization), "{err:?}");
                assert_eq!(err.gqlstatus(), Some("25N11"));
                assert_eq!(read(&e, t, b"y"), Some(b"1".to_vec()));
            }
            Isolation::Snapshot => {
                r2.unwrap();
                assert_eq!(read(&e, t, b"y"), Some(b"0".to_vec()));
            }
        }
        assert_eq!(read(&e, t, b"x"), Some(b"0".to_vec()));
    }
}

/// FR-TX-009: T1 reads n and writes m; T2 changes n and commits; T1's
/// commit fails and m's change is never visible.
#[test]
fn a_commit_whose_read_was_overwritten_fails_validation() {
    let (_d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"n", b"1"), (b"m", b"1")]);
    let mut t1 = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(t1.get(t, b"n").unwrap(), Some(b"1".to_vec()));
    t1.put(t, b"m", b"2").unwrap();
    commit_puts(&e, t, &[(b"n", b"2")]);
    assert!(matches!(t1.commit(), Err(Error::Serialization)));
    assert_eq!(read(&e, t, b"m"), Some(b"1".to_vec()));
    // A scan's range counts too: a key inserted into it fails the reader.
    let mut t3 = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(all(&mut t3, t).len(), 2);
    t3.put(t, b"m", b"3").unwrap();
    commit_puts(&e, t, &[(b"new", b"1")]);
    assert!(matches!(t3.commit(), Err(Error::Serialization)));
    assert_eq!(read(&e, t, b"m"), Some(b"1".to_vec()));
}

/// FR-TX-010: a read-only transaction is not validated: it commits, and it
/// read only the state from before it started, although another
/// transaction changed what it read meanwhile.
#[test]
fn a_read_only_transaction_reads_its_start_state_and_commits() {
    let (_d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"a", b"1"), (b"b", b"1")]);
    let mut t1 = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(t1.get(t, b"a").unwrap(), Some(b"1".to_vec()));
    commit_puts(&e, t, &[(b"a", b"2"), (b"b", b"2"), (b"c", b"2")]);
    assert_eq!(t1.get(t, b"b").unwrap(), Some(b"1".to_vec()));
    assert_eq!(all(&mut t1, t), kv(&[("a", "1"), ("b", "1")]));
    assert_eq!(t1.commit().unwrap(), None);
}

/// FR-TX-005: T2 waits for the X lock T1 holds and continues once T1
/// ends. After T1 aborts, T2 commits; after T1 commits a change of the key,
/// T2 continues and fails with `Serialization` under the write-write rule
/// (TS-15), at both levels.
#[test]
fn a_writer_waits_for_a_held_lock_and_continues_when_its_holder_ends() {
    for (iso, holder_commits) in [
        (Isolation::Serializable, false),
        (Isolation::Serializable, true),
        (Isolation::Snapshot, true),
    ] {
        let (_d, e) = db();
        let t = tree(&e);
        commit_puts(&e, t, &[(b"k", b"0")]);
        let mut t1 = e.begin(iso).unwrap();
        t1.put(t, b"k", b"1").unwrap();
        std::thread::scope(|s| {
            let e = &e;
            let t2 = s.spawn(move || {
                let mut t2 = e.begin(iso).unwrap();
                let r = t2.put(t, b"k", b"2");
                (r, t2.commit())
            });
            until_waiting(e, 1);
            if holder_commits {
                t1.commit().unwrap();
            } else {
                t1.abort();
            }
            let (put, commit) = t2.join().unwrap();
            if holder_commits {
                assert!(matches!(put, Err(Error::Serialization)), "{put:?}");
                assert!(matches!(commit, Err(Error::Aborted)));
                assert_eq!(read(e, t, b"k"), Some(b"1".to_vec()));
            } else {
                put.unwrap();
                commit.unwrap();
                assert_eq!(read(e, t, b"k"), Some(b"2".to_vec()));
            }
        });
        assert!(e.core().mvcc.locks.is_empty());
    }
}

/// FR-TX-007: T1 holds A and waits for B; T2 holds B and requests A. T2
/// fails with `Deadlock` (GQLSTATUS 50N05) and is aborted; T1 completes and
/// commits.
#[test]
fn a_closed_wait_cycle_aborts_the_requester_with_deadlock() {
    let (_d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"A", b"0"), (b"B", b"0")]);
    let mut t2 = e.begin(Isolation::Serializable).unwrap();
    t2.put(t, b"B", b"2").unwrap();
    std::thread::scope(|s| {
        let e = &e;
        let t1 = s.spawn(move || {
            let mut t1 = e.begin(Isolation::Serializable).unwrap();
            t1.put(t, b"A", b"1").unwrap();
            // Signalled to the test by the lock wait below.
            t1.put(t, b"B", b"1").unwrap();
            t1.commit().unwrap()
        });
        // T1 holds A and waits for B.
        until_waiting(e, 1);
        let err = t2.put(t, b"A", b"2").unwrap_err();
        assert!(matches!(err, Error::Deadlock), "{err:?}");
        assert_eq!(err.gqlstatus(), Some("50N05"));
        assert!(matches!(t2.commit(), Err(Error::Aborted)));
        assert!(t1.join().unwrap().is_some());
    });
    assert_eq!(read(&e, t, b"A"), Some(b"1".to_vec()));
    assert_eq!(read(&e, t, b"B"), Some(b"1".to_vec()));
}

/// FR-TX-006: a read completes while a writer holds the X lock with
/// uncommitted writes, and while a commit is paused under the commit latch:
/// readers take neither locks nor the latch.
#[test]
fn reads_never_wait_for_writers_or_the_commit_latch() {
    let (_d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"k", b"0")]);
    let mut w = e.begin(Isolation::Serializable).unwrap();
    w.put(t, b"k", b"1").unwrap();
    // The writer holds X on k; the read returns the committed value.
    assert_eq!(read(&e, t, b"k"), Some(b"0".to_vec()));
    // The commit pauses under the latch until the read has completed.
    let (read_tx, read_rx) = mpsc::channel::<Option<Vec<u8>>>();
    let read_rx = Mutex::new(read_rx);
    let (paused_tx, paused) = mpsc::channel::<()>();
    let paused_tx = Mutex::new(paused_tx);
    *e.wal().points.after_apply.lock().unwrap() = Some(Box::new(move || {
        paused_tx.lock().unwrap().send(()).unwrap();
        let v = read_rx.lock().unwrap().recv().unwrap();
        assert_eq!(v, Some(b"0".to_vec()));
    }));
    std::thread::scope(|s| {
        let c = s.spawn(move || w.commit().unwrap());
        paused.recv().unwrap();
        // Under the latch held by the paused commit.
        read_tx.send(read(&e, t, b"k")).unwrap();
        c.join().unwrap();
    });
    *e.wal().points.after_apply.lock().unwrap() = None;
    assert_eq!(read(&e, t, b"k"), Some(b"1".to_vec()));
}

/// FR-STO-003: 16 threads write disjoint keys in transactions at both
/// levels; every commit succeeds and every write is present after reopen.
#[test]
fn sixteen_threads_with_disjoint_writes_all_commit_and_survive_reopen() {
    let (d, e) = db();
    let t = tree(&e);
    const TXS: usize = 20;
    std::thread::scope(|s| {
        for th in 0..16usize {
            let e = &e;
            s.spawn(move || {
                let iso = if th % 2 == 0 {
                    Isolation::Serializable
                } else {
                    Isolation::Snapshot
                };
                for i in 0..TXS {
                    let mut tx = e.begin(iso).unwrap();
                    for j in 0..3 {
                        let k = format!("t{th:02}-{i:03}-{j}");
                        tx.put(t, k.as_bytes(), k.as_bytes()).unwrap();
                    }
                    assert!(tx.commit().unwrap().is_some());
                }
            });
        }
    });
    e.close().unwrap();
    let e = reopen(d.path());
    let mut r = e.begin(Isolation::Serializable).unwrap();
    let got = all(&mut r, t);
    assert_eq!(got.len(), 16 * TXS * 3);
    assert!(got.iter().all(|(k, v)| k == v));
    r.commit().unwrap();
    e.close().unwrap();
}

/// BR-TX-001: no automatic retry. A transaction that fails validation ran
/// each of its operations exactly once, the commit was attempted once and
/// failed once, and its write is not applied.
#[test]
fn a_failed_transaction_ran_its_operations_once() {
    let d = graph_dir();
    let sink = Arc::new(RecordingSink::default());
    let e = Engine::create_tuned(d.path(), no_flush(), CAP, sink.clone(), tuning()).unwrap();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"n", b"1")]);
    let before = sink.snapshot();
    let calls = std::cell::Cell::new(0);
    let mut t1 = e.begin(Isolation::Serializable).unwrap();
    let op = |tx: &mut Transaction<'_>, write: bool| {
        calls.set(calls.get() + 1);
        if write {
            tx.put(t, b"m", b"1").map(|()| None)
        } else {
            tx.get(t, b"n")
        }
    };
    op(&mut t1, false).unwrap();
    op(&mut t1, true).unwrap();
    commit_puts(&e, t, &[(b"n", b"2")]);
    let mid = sink.snapshot();
    assert!(matches!(t1.commit(), Err(Error::Serialization)));
    let after = sink.snapshot();
    assert_eq!(calls.get(), 2);
    assert_eq!(
        after.counter(FAILED_COMMITS) - mid.counter(FAILED_COMMITS),
        1
    );
    assert_eq!(after.counter(COMMITTED), mid.counter(COMMITTED));
    assert_eq!(mid.counter(COMMITTED) - before.counter(COMMITTED), 1);
    assert_eq!(read(&e, t, b"m"), None);
}

/// Write skew through a tree drop (audit Round A, P2). T1 reads tree `t`;
/// T2 reads key x of `u` (absent), drops `t` and commits; T1 then writes x
/// and commits. No serial order explains both commits: T1 first makes T2
/// read x present, T2 first makes T1 read a dropped tree. T1's first use
/// of `t` recorded a read of its directory entry, which the drop writes,
/// so T1 fails validation (TS-15).
#[test]
fn a_drop_committed_after_a_read_of_the_tree_fails_the_readers_commit() {
    let (_d, e) = db();
    let t = tree(&e);
    let u = tree(&e);
    commit_puts(&e, t, &[(b"k", b"v")]);
    let mut t1 = e.begin(Isolation::Serializable).unwrap();
    let mut t2 = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(t1.get(t, b"k").unwrap(), Some(b"v".to_vec()));
    assert_eq!(t2.get(u, b"x").unwrap(), None);
    t2.drop_tree(t).unwrap();
    t2.commit().unwrap();
    t1.put(u, b"x", b"1").unwrap();
    assert!(matches!(t1.commit(), Err(Error::Serialization)));
    assert_eq!(read(&e, u, b"x"), None);
}

/// Serializable delete of an absent key against a concurrent insert of
/// that key (audit Round A, M15). T1 deletes absent k, which records a
/// read of k, and writes j; T2 reads j (absent) and inserts k, and commits
/// first. Write skew: T1 must fail validation, and its write of j is never
/// visible.
#[test]
fn deleting_an_absent_key_conflicts_with_its_concurrent_insert() {
    let (_d, e) = db();
    let t = tree(&e);
    let mut t1 = e.begin(Isolation::Serializable).unwrap();
    let mut t2 = e.begin(Isolation::Serializable).unwrap();
    assert!(!t1.delete(t, b"k").unwrap());
    t1.put(t, b"j", b"1").unwrap();
    assert_eq!(t2.get(t, b"j").unwrap(), None);
    t2.put(t, b"k", b"2").unwrap();
    t2.commit().unwrap();
    assert!(matches!(t1.commit(), Err(Error::Serialization)));
    assert_eq!(read(&e, t, b"j"), None);
    assert_eq!(read(&e, t, b"k"), Some(b"2".to_vec()));
}

/// An S→X upgrade of a tree lock is charged nothing more (audit Round A):
/// dropping a tree the transaction already wrote charges only the drop's
/// own write key and entry, not a second lock entry.
#[test]
fn a_lock_upgrade_charges_no_second_lock_entry() {
    let (_d, e) = db();
    let t = tree(&e);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    tx.put(t, b"k", b"v").unwrap();
    let before = tx.charge.as_ref().unwrap().bytes();
    tx.drop_tree(t).unwrap();
    let after = tx.charge.as_ref().unwrap().bytes();
    let key = t.id().to_be_bytes();
    assert_eq!(
        after - before,
        WriteKeys::charge_of(&key) + WRITE_OVERHEAD + 16
    );
    assert!(
        e.core()
            .mvcc
            .locks
            .held(tx.id)
            .contains(&(Resource::Tree(t.id()), Mode::X))
    );
    tx.abort();
}

/// FR-TX-006, README "Reads never block": one thread holds 300
/// serializable transactions, more than the 256 slots of each registry's
/// first block, and every begin returns. The horizons stay the lowest
/// snapshot, also when it is held only in an overflow block, and return to
/// the published timestamp once every transaction ends.
#[test]
fn three_hundred_transactions_on_one_thread_all_begin() {
    let (_d, e) = db();
    let t = tree(&e);
    let a = commit_puts(&e, t, &[(b"k", b"1")]);
    let mvcc = &e.core().mvcc;
    let published = e.core().wal.progress.published_ts();
    let first: Vec<_> = (0..256)
        .map(|_| e.begin(Isolation::Serializable).unwrap())
        .collect();
    assert!(first.iter().all(|tx| tx.snapshot() == a));
    let b = commit_puts(&e, t, &[(b"k", b"2")]);
    let rest: Vec<_> = (0..44)
        .map(|_| e.begin(Isolation::Serializable).unwrap())
        .collect();
    assert!(rest.iter().all(|tx| tx.snapshot() == b));
    assert_eq!(mvcc.snapshots.overflow_blocks(), 1);
    assert_eq!(mvcc.serializable.overflow_blocks(), 1);
    assert_eq!(crate::tx::mvcc::horizon(&mvcc.snapshots, published), a);
    drop(first);
    let c = commit_puts(&e, t, &[(b"k", b"3")]);
    assert!(c > b);
    // Only overflow slots hold snapshot b now.
    assert_eq!(crate::tx::mvcc::horizon(&mvcc.snapshots, published), b);
    assert_eq!(crate::tx::mvcc::horizon(&mvcc.serializable, published), b);
    drop(rest);
    assert_eq!(crate::tx::mvcc::horizon(&mvcc.snapshots, published), c);
    assert_eq!(crate::tx::mvcc::horizon(&mvcc.serializable, published), c);
}
