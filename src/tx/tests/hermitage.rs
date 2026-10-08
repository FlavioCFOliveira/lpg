//! Hermitage isolation scenarios (TS-30 Isolation, R12), at both levels.
//!
//! Each scenario starts from a tree `test` holding keys 1 → 10 and 2 → 20
//! and interleaves two or three transactions step by step; a step that
//! must wait for a lock runs on its own thread, and the test goes on once
//! the lock table shows it waiting. Expected outcome: serializable
//! isolation prevents every anomaly; snapshot isolation prevents all but
//! G2-item and G2 (TS-30: "lpg is serializable by default, so G2 and write
//! skew must abort").
//!
//! **Origin.** The scenarios follow Turso's adaptation of Hermitage,
//! `core/mvcc/database/hermitage_tests.rs` (R-TURSO @ `8cc059b`, MIT
//! licence, "Copyright 2024 the Turso authors";
//! <https://github.com/tursodatabase/turso>; its header credits
//! <https://github.com/ept/hermitage>), the starting point approved as
//! R12. They are rewritten for lpg's transaction API, and the expectations
//! changed: Turso is snapshot isolation that fails a write-write conflict
//! at once, while lpg's writers wait for the lock and then apply the
//! write-write rule (TS-15), and its serializable level also fails G2-item
//! and G2. Turso's licence notice: "Permission is hereby granted, free of
//! charge, to any person obtaining a copy of this software and associated
//! documentation files (the "Software"), to deal in the Software without
//! restriction, including without limitation the rights to use, copy,
//! modify, merge, publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software is furnished to
//! do so, subject to the following conditions: The above copyright notice
//! and this permission notice shall be included in all copies or
//! substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
//! WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED."

use super::*;
use crate::storage::error::Error;

const LEVELS: [Isolation; 2] = [Isolation::Serializable, Isolation::Snapshot];

fn setup() -> (tempfile::TempDir, Engine, Tree) {
    let (d, e) = db();
    let t = tree(&e);
    commit_puts(&e, t, &[(b"1", b"10"), (b"2", b"20")]);
    (d, e, t)
}

fn val(tx: &mut Transaction<'_>, t: Tree, k: u8) -> Option<i64> {
    tx.get(t, &[b'0' + k])
        .unwrap()
        .map(|v| String::from_utf8(v).unwrap().parse().unwrap())
}

fn set(tx: &mut Transaction<'_>, t: Tree, k: u8, v: i64) -> crate::storage::error::Result<()> {
    tx.put(t, &[b'0' + k], v.to_string().as_bytes())
}

/// `(key, value)` of every row whose value satisfies `pred`, read by a
/// scan of the whole tree (a predicate read).
fn select(tx: &mut Transaction<'_>, t: Tree, pred: impl Fn(i64) -> bool) -> Vec<(u8, i64)> {
    all(tx, t)
        .into_iter()
        .map(|(k, v)| (k[0] - b'0', String::from_utf8(v).unwrap().parse().unwrap()))
        .filter(|&(_, v)| pred(v))
        .collect()
}

fn state(e: &Engine, t: Tree) -> Vec<(u8, i64)> {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let rows = select(&mut tx, t, |_| true);
    tx.commit().unwrap();
    rows
}

/// G0, write cycles: T2's update of a row T1 updated waits for T1, then
/// fails under the write-write rule once T1 commits; no cycle of
/// uncommitted writes can form.
#[test]
fn g0_write_cycles_are_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        set(&mut t1, t, 1, 11).unwrap();
        std::thread::scope(|s| {
            let e = &e;
            let t2 = s.spawn(move || {
                let mut t2 = e.begin(iso).unwrap();
                let r = set(&mut t2, t, 1, 12);
                (r, set(&mut t2, t, 2, 22), t2.commit())
            });
            until_waiting(e, 1);
            set(&mut t1, t, 2, 21).unwrap();
            t1.commit().unwrap();
            let (a, b, c) = t2.join().unwrap();
            assert!(matches!(a, Err(Error::Serialization)), "{iso:?} {a:?}");
            assert!(matches!(b, Err(Error::Aborted)) && matches!(c, Err(Error::Aborted)));
        });
        assert_eq!(state(&e, t), vec![(1, 11), (2, 21)], "{iso:?}");
    }
}

/// G1a, aborted reads: T2 never reads T1's write, before or after T1
/// aborts.
#[test]
fn g1a_aborted_reads_are_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        set(&mut t1, t, 1, 101).unwrap();
        assert_eq!(select(&mut t2, t, |_| true), vec![(1, 10), (2, 20)]);
        t1.abort();
        assert_eq!(select(&mut t2, t, |_| true), vec![(1, 10), (2, 20)]);
        assert_eq!(t2.commit().unwrap(), None);
        assert_eq!(state(&e, t), vec![(1, 10), (2, 20)]);
    }
}

/// G1b, intermediate reads: T2 never reads T1's intermediate value, nor its
/// final one while its snapshot is older.
#[test]
fn g1b_intermediate_reads_are_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        set(&mut t1, t, 1, 101).unwrap();
        assert_eq!(val(&mut t2, t, 1), Some(10));
        set(&mut t1, t, 1, 11).unwrap();
        t1.commit().unwrap();
        assert_eq!(val(&mut t2, t, 1), Some(10));
        assert_eq!(t2.commit().unwrap(), None);
        assert_eq!(state(&e, t), vec![(1, 11), (2, 20)]);
    }
}

/// G1c, circular information flow: neither transaction reads the other's
/// uncommitted write. The two reads form an anti-dependency cycle, so the
/// second commit fails under serializable isolation; under snapshot
/// isolation both commit.
#[test]
fn g1c_circular_information_flow_is_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        set(&mut t1, t, 1, 11).unwrap();
        set(&mut t2, t, 2, 22).unwrap();
        assert_eq!(val(&mut t1, t, 2), Some(20));
        assert_eq!(val(&mut t2, t, 1), Some(10));
        t1.commit().unwrap();
        let r2 = t2.commit();
        if iso == Isolation::Serializable {
            assert!(matches!(r2, Err(Error::Serialization)), "{r2:?}");
            assert_eq!(state(&e, t), vec![(1, 11), (2, 20)]);
        } else {
            r2.unwrap();
            assert_eq!(state(&e, t), vec![(1, 11), (2, 22)]);
        }
    }
}

/// OTV, observed transaction vanishes: T2's conflicting write waits for T1
/// and then fails; T3, which began after T1 committed, reads T1's values
/// and keeps reading them.
#[test]
fn otv_observed_transaction_vanishes_is_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        set(&mut t1, t, 1, 11).unwrap();
        set(&mut t1, t, 2, 19).unwrap();
        std::thread::scope(|s| {
            let e = &e;
            let t2 = s.spawn(move || {
                let mut t2 = e.begin(iso).unwrap();
                let r = set(&mut t2, t, 1, 12);
                (r, t2.commit())
            });
            until_waiting(e, 1);
            t1.commit().unwrap();
            let mut t3 = e.begin(iso).unwrap();
            assert_eq!(val(&mut t3, t, 1), Some(11));
            let (a, b) = t2.join().unwrap();
            assert!(matches!(a, Err(Error::Serialization)), "{iso:?} {a:?}");
            assert!(matches!(b, Err(Error::Aborted)));
            assert_eq!(val(&mut t3, t, 2), Some(19));
            assert_eq!(val(&mut t3, t, 1), Some(11));
            t3.commit().unwrap();
        });
        assert_eq!(state(&e, t), vec![(1, 11), (2, 19)]);
    }
}

/// PMP, predicate-many-preceders: a predicate read does not see a row
/// another transaction inserted and committed after the snapshot; and a
/// predicate delete of a row another transaction updated waits for it,
/// then fails under the write-write rule.
#[test]
fn pmp_predicate_many_preceders_is_prevented() {
    for iso in LEVELS {
        // Read predicates.
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        assert!(select(&mut t1, t, |v| v == 30).is_empty());
        let mut t2 = e.begin(iso).unwrap();
        set(&mut t2, t, 3, 30).unwrap();
        t2.commit().unwrap();
        assert!(select(&mut t1, t, |v| v % 3 == 0).is_empty());
        assert_eq!(t1.commit().unwrap(), None);
        // Write predicates.
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        for (k, v) in select(&mut t1, t, |_| true) {
            set(&mut t1, t, k, v + 10).unwrap();
        }
        std::thread::scope(|s| {
            let e = &e;
            let t2 = s.spawn(move || {
                let mut t2 = e.begin(iso).unwrap();
                let rows = select(&mut t2, t, |v| v == 20);
                assert_eq!(rows, vec![(2, 20)]);
                let r = t2.delete(t, b"2");
                (r, t2.commit())
            });
            until_waiting(e, 1);
            t1.commit().unwrap();
            let (a, b) = t2.join().unwrap();
            assert!(matches!(a, Err(Error::Serialization)), "{iso:?} {a:?}");
            assert!(matches!(b, Err(Error::Aborted)));
        });
        assert_eq!(state(&e, t), vec![(1, 20), (2, 30)]);
    }
}

/// P4, lost update: both read the row; T2's update waits for T1's, then
/// fails once T1 commits, so T1's update is not lost.
#[test]
fn p4_lost_update_is_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        assert_eq!(val(&mut t1, t, 1), Some(10));
        std::thread::scope(|s| {
            let e = &e;
            let mut t2 = e.begin(iso).unwrap();
            assert_eq!(val(&mut t2, t, 1), Some(10));
            set(&mut t1, t, 1, 11).unwrap();
            let t2 = s.spawn(move || {
                let r = set(&mut t2, t, 1, 11);
                (r, t2.commit())
            });
            until_waiting(e, 1);
            t1.commit().unwrap();
            let (a, b) = t2.join().unwrap();
            assert!(matches!(a, Err(Error::Serialization)), "{iso:?} {a:?}");
            assert!(matches!(b, Err(Error::Aborted)));
        });
        assert_eq!(state(&e, t), vec![(1, 11), (2, 20)]);
    }
}

/// G-single, read skew: T1 reads x before and y after T2 changed both and
/// committed, and sees both at its snapshot; a write predicate of T1 on a
/// row T2 changed fails under the write-write rule.
#[test]
fn g_single_read_skew_is_prevented() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        assert_eq!(val(&mut t1, t, 1), Some(10));
        assert_eq!(val(&mut t2, t, 1), Some(10));
        assert_eq!(val(&mut t2, t, 2), Some(20));
        set(&mut t2, t, 1, 12).unwrap();
        set(&mut t2, t, 2, 18).unwrap();
        t2.commit().unwrap();
        assert_eq!(val(&mut t1, t, 2), Some(20));
        // A write predicate on the changed row.
        let rows = select(&mut t1, t, |v| v == 20);
        assert_eq!(rows, vec![(2, 20)]);
        assert!(matches!(t1.delete(t, b"2"), Err(Error::Serialization)));
        assert!(matches!(t1.commit(), Err(Error::Aborted)));
        assert_eq!(state(&e, t), vec![(1, 12), (2, 18)]);
    }
}

/// G2-item, write skew: both read both rows, each updates one. Under
/// serializable isolation the second commit fails; under snapshot
/// isolation both commit, the anomaly snapshot isolation allows.
#[test]
fn g2_item_write_skew_fails_only_under_serializable() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        for tx in [&mut t1, &mut t2] {
            assert_eq!(select(tx, t, |v| v == 10 || v == 20).len(), 2);
        }
        set(&mut t1, t, 1, 11).unwrap();
        set(&mut t2, t, 2, 21).unwrap();
        t1.commit().unwrap();
        let r2 = t2.commit();
        if iso == Isolation::Serializable {
            assert!(matches!(r2, Err(Error::Serialization)), "{r2:?}");
            assert_eq!(state(&e, t), vec![(1, 11), (2, 20)]);
        } else {
            r2.unwrap();
            assert_eq!(state(&e, t), vec![(1, 11), (2, 21)]);
        }
    }
}

/// G2, anti-dependency cycles on predicates: both read a predicate that
/// matches nothing, then each inserts a matching row. Under serializable
/// isolation the second commit fails, since its predicate read covers the
/// first's insert; under snapshot isolation both commit.
#[test]
fn g2_anti_dependency_cycles_fail_only_under_serializable() {
    for iso in LEVELS {
        let (_d, e, t) = setup();
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        assert!(select(&mut t1, t, |v| v % 3 == 0).is_empty());
        assert!(select(&mut t2, t, |v| v % 3 == 0).is_empty());
        set(&mut t1, t, 3, 30).unwrap();
        set(&mut t2, t, 4, 42).unwrap();
        t1.commit().unwrap();
        let r2 = t2.commit();
        if iso == Isolation::Serializable {
            assert!(matches!(r2, Err(Error::Serialization)), "{r2:?}");
            assert_eq!(state(&e, t), vec![(1, 10), (2, 20), (3, 30)]);
        } else {
            r2.unwrap();
            assert_eq!(state(&e, t), vec![(1, 10), (2, 20), (3, 30), (4, 42)]);
        }
    }
}
