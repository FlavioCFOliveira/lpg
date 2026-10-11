//! Hermitage isolation scenarios (TS-30 Isolation, R12), at both levels.
//!
//! Each scenario starts from two rows, 1 → 10 and 2 → 20, and runs twice:
//! on entries of a tree, and on nodes of the default graph whose property
//! `V` holds the value ([`Kind`], rmp #35). It interleaves two or three
//! transactions step by step; a step that must wait for a lock runs on its
//! own thread, and the test goes on once the lock table shows it waiting. Expected outcome: serializable
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

use std::collections::BTreeMap;

use super::*;
use crate::catalog::{DEFAULT_GRAPH_ID, TokenKind};
use crate::storage::error::{Error, Result};
use crate::sync::Mutex;
use crate::types::Value;

const LEVELS: [Isolation; 2] = [Isolation::Serializable, Isolation::Snapshot];

/// What the rows of a scenario are (rmp #35: Hermitage on graph
/// operations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Entries `k → v` of a tree.
    Entries,
    /// Nodes of the default graph with label `R` and properties `K` = k
    /// and `V` = v: a read is a node read, a write a property update or a
    /// node creation, a delete a node deletion, and a predicate read a
    /// scan of the node tree.
    Nodes,
}

/// Every level with every kind of row.
fn cases() -> impl Iterator<Item = (Isolation, Kind)> {
    LEVELS
        .into_iter()
        .flat_map(|iso| [Kind::Entries, Kind::Nodes].map(|k| (iso, k)))
}

/// The rows of one scenario.
enum Data {
    Entries(Tree),
    Nodes {
        /// Label `R`, property keys `K` and `V`.
        tokens: (u64, u64, u64),
        /// The node of each row created so far.
        ids: Mutex<BTreeMap<u8, u64>>,
    },
}

fn setup(kind: Kind) -> (tempfile::TempDir, Engine, Data) {
    let (d, e) = db();
    let data = match kind {
        Kind::Entries => Data::Entries(tree(&e)),
        Kind::Nodes => Data::Nodes {
            tokens: (
                e.token(TokenKind::Label, "R").unwrap(),
                e.token(TokenKind::PropertyKey, "K").unwrap(),
                e.token(TokenKind::PropertyKey, "V").unwrap(),
            ),
            ids: Mutex::new(BTreeMap::new()),
        },
    };
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    set(&mut tx, &data, 1, 10).unwrap();
    set(&mut tx, &data, 2, 20).unwrap();
    tx.commit().unwrap();
    (d, e, data)
}

/// Property `key` of `props`, an integer.
fn int(props: &[(u64, Value)], key: u64) -> i64 {
    match props.iter().find(|p| p.0 == key) {
        Some((_, Value::Int(v))) => *v,
        p => panic!("property {key}: {p:?}"),
    }
}

fn val(tx: &mut Transaction, t: &Data, k: u8) -> Option<i64> {
    match t {
        Data::Entries(t) => tx
            .get(*t, &[b'0' + k])
            .unwrap()
            .map(|v| String::from_utf8(v).unwrap().parse().unwrap()),
        Data::Nodes { tokens, ids } => {
            let id = *ids.lock().unwrap().get(&k)?;
            let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
            tx.node(&g, id).unwrap().map(|(_, p)| int(&p, tokens.2))
        }
    }
}

fn set(tx: &mut Transaction, t: &Data, k: u8, v: i64) -> Result<()> {
    match t {
        Data::Entries(t) => tx.put(*t, &[b'0' + k], v.to_string().as_bytes()),
        Data::Nodes { tokens, ids } => {
            let g = tx.graph(DEFAULT_GRAPH_ID)?;
            let id = ids.lock().unwrap().get(&k).copied();
            match id {
                Some(id) => tx.set_property(&g, id, tokens.2, Value::Int(v)),
                None => {
                    let props = [(tokens.1, Value::Int(k.into())), (tokens.2, Value::Int(v))];
                    let id = tx.create_node(&g, &[tokens.0], &props)?;
                    ids.lock().unwrap().insert(k, id);
                    Ok(())
                }
            }
        }
    }
}

/// Deletes row `k`: the entry, or the node.
fn del(tx: &mut Transaction, t: &Data, k: u8) -> Result<bool> {
    match t {
        Data::Entries(t) => tx.delete(*t, &[b'0' + k]),
        Data::Nodes { ids, .. } => {
            let g = tx.graph(DEFAULT_GRAPH_ID)?;
            let id = ids.lock().unwrap()[&k];
            tx.delete_node(&g, id, false)
        }
    }
}

/// `(key, value)` of every row whose value satisfies `pred`, read by a
/// scan of the whole tree or node tree (a predicate read).
fn select(tx: &mut Transaction, t: &Data, pred: impl Fn(i64) -> bool) -> Vec<(u8, i64)> {
    let mut rows: Vec<(u8, i64)> = match t {
        Data::Entries(t) => all(tx, *t)
            .into_iter()
            .map(|(k, v)| (k[0] - b'0', String::from_utf8(v).unwrap().parse().unwrap()))
            .collect(),
        Data::Nodes { tokens, .. } => {
            let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
            all(tx, g.nodes)
                .into_iter()
                .map(|(_, v)| {
                    let (_, p) = crate::types::record::decode(&v).unwrap();
                    (u8::try_from(int(&p, tokens.1)).unwrap(), int(&p, tokens.2))
                })
                .collect()
        }
    };
    rows.sort_unstable();
    rows.retain(|&(_, v)| pred(v));
    rows
}

fn state(e: &Engine, t: &Data) -> Vec<(u8, i64)> {
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
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
            assert!(
                matches!(a, Err(Error::Serialization)),
                "{iso:?} {kind:?} {a:?}"
            );
            assert!(matches!(b, Err(Error::Aborted)) && matches!(c, Err(Error::Aborted)));
        });
        assert_eq!(state(&e, t), vec![(1, 11), (2, 21)], "{iso:?}");
    }
}

/// G1a, aborted reads: T2 never reads T1's write, before or after T1
/// aborts.
#[test]
fn g1a_aborted_reads_are_prevented() {
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
            assert!(
                matches!(a, Err(Error::Serialization)),
                "{iso:?} {kind:?} {a:?}"
            );
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
    for (iso, kind) in cases() {
        // Read predicates.
        let (_d, e, data) = setup(kind);
        let t = &data;
        let mut t1 = e.begin(iso).unwrap();
        assert!(select(&mut t1, t, |v| v == 30).is_empty());
        let mut t2 = e.begin(iso).unwrap();
        set(&mut t2, t, 3, 30).unwrap();
        t2.commit().unwrap();
        assert!(select(&mut t1, t, |v| v % 3 == 0).is_empty());
        assert_eq!(t1.commit().unwrap(), None);
        // Write predicates.
        let (_d, e, data) = setup(kind);
        let t = &data;
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
                let r = del(&mut t2, t, 2);
                (r, t2.commit())
            });
            until_waiting(e, 1);
            t1.commit().unwrap();
            let (a, b) = t2.join().unwrap();
            assert!(
                matches!(a, Err(Error::Serialization)),
                "{iso:?} {kind:?} {a:?}"
            );
            assert!(matches!(b, Err(Error::Aborted)));
        });
        assert_eq!(state(&e, t), vec![(1, 20), (2, 30)]);
    }
}

/// P4, lost update: both read the row; T2's update waits for T1's, then
/// fails once T1 commits, so T1's update is not lost.
#[test]
fn p4_lost_update_is_prevented() {
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
            assert!(
                matches!(a, Err(Error::Serialization)),
                "{iso:?} {kind:?} {a:?}"
            );
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
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
        assert!(matches!(del(&mut t1, t, 2), Err(Error::Serialization)));
        assert!(matches!(t1.commit(), Err(Error::Aborted)));
        assert_eq!(state(&e, t), vec![(1, 12), (2, 18)]);
    }
}

/// G2-item, write skew: both read both rows, each updates one. Under
/// serializable isolation the second commit fails; under snapshot
/// isolation both commit, the anomaly snapshot isolation allows.
#[test]
fn g2_item_write_skew_fails_only_under_serializable() {
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
    for (iso, kind) in cases() {
        let (_d, e, data) = setup(kind);
        let t = &data;
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
