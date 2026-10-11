//! Elle-checked histories (TS-30 Isolation, R12; rmp #24 and #28).
//!
//! Threads run transactions of Elle's list-append and rw-register
//! workloads against lpg and record them in a
//! [`crate::test_support::history::History`]; `elle-cli` then checks the
//! history against the consistency model of the isolation level used.
//! Written values are unique per key, as Elle needs to infer the version
//! order. The workloads are seeded (`LPG_SEED`, TS-30 R5).

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::*;
use crate::test_support::history::{Event, History, Op, elle};
use crate::test_support::seed;

/// Runs `threads` threads of `txs` transactions each at `iso` on `keys`
/// keys and records them; `list` picks list-append, else rw-register.
fn workload(iso: Isolation, list: bool, threads: usize, txs: usize, keys: u64) -> History {
    let (_d, e) = db();
    let t = tree(&e);
    let h = History::default();
    let base = seed();
    std::thread::scope(|s| {
        for th in 0..threads {
            let (e, h) = (&e, &h);
            s.spawn(move || {
                let mut rng = StdRng::seed_from_u64(base.wrapping_add(th as u64));
                for i in 0..txs {
                    let n = rng.random_range(1..=4);
                    let mut ops: Vec<Op> = (0..n)
                        .map(|j| {
                            let key = rng.random_range(0..keys);
                            let value = (th * 1_000_000 + i * 10 + j) as i64;
                            match (list, rng.random_range(0..2)) {
                                (true, 0) => Op::Append { key, value },
                                (true, _) => Op::ReadList { key, result: None },
                                (false, 0) => Op::Write { key, value },
                                (false, _) => Op::Read { key, result: None },
                            }
                        })
                        .collect();
                    h.record(Event::Invoke, th, ops.clone());
                    let ok = run_ops(e, t, iso, &mut ops);
                    h.record(if ok { Event::Ok } else { Event::Fail }, th, ops);
                }
            });
        }
    });
    e.close().unwrap();
    h
}

/// Runs `ops` in one transaction, filling in the reads; `true` if it
/// committed.
fn run_ops(e: &Engine, t: Tree, iso: Isolation, ops: &mut [Op]) -> bool {
    let mut tx = e.begin(iso).unwrap();
    let parse = |v: Option<Vec<u8>>| -> Vec<i64> {
        v.map(|v| {
            String::from_utf8(v)
                .unwrap()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().unwrap())
                .collect()
        })
        .unwrap_or_default()
    };
    for op in ops.iter_mut() {
        let r = match op {
            Op::Append { key, value } => {
                let k = key.to_be_bytes();
                tx.get(t, &k).and_then(|v| {
                    let mut list = parse(v);
                    list.push(*value);
                    let text: Vec<String> = list.iter().map(i64::to_string).collect();
                    tx.put(t, &k, text.join(",").as_bytes())
                })
            }
            Op::ReadList { key, result } => tx.get(t, &key.to_be_bytes()).map(|v| {
                *result = Some(parse(v));
            }),
            Op::Write { key, value } => tx.put(t, &key.to_be_bytes(), value.to_string().as_bytes()),
            Op::Read { key, result } => tx.get(t, &key.to_be_bytes()).map(|v| {
                *result = Some(v.map(|v| String::from_utf8(v).unwrap().parse().unwrap()));
            }),
        };
        if r.is_err() {
            return false;
        }
    }
    tx.commit().is_ok()
}

/// rmp #24 acceptance: a two-thread rw-register history recorded from lpg
/// transactions is valid for Elle.
#[test]
fn elle_accepts_a_two_thread_rw_register_history() {
    let h = workload(Isolation::Serializable, false, 2, 40, 3);
    let v = elle(
        "rw-register-2-threads",
        &h.to_edn(),
        "rw-register",
        "serializable",
    );
    assert!(v.valid, "{}\n{}", v.path.display(), v.output);
}

/// Eight threads of list-append transactions, at each level: Elle finds no
/// anomaly under the level's consistency model (serializable,
/// snapshot-isolation).
#[test]
fn elle_finds_no_anomaly_in_eight_thread_list_append_histories() {
    for (iso, model) in [
        (Isolation::Serializable, "serializable"),
        (Isolation::Snapshot, "snapshot-isolation"),
    ] {
        let h = workload(iso, true, 8, 40, 6);
        let v = elle(
            &format!("list-append-8-{model}"),
            &h.to_edn(),
            "list-append",
            model,
        );
        assert!(v.valid, "{iso:?}: {}\n{}", v.path.display(), v.output);
    }
}

/// Eight threads of rw-register transactions, at each level.
#[test]
fn elle_finds_no_anomaly_in_eight_thread_rw_register_histories() {
    for (iso, model) in [
        (Isolation::Serializable, "serializable"),
        (Isolation::Snapshot, "snapshot-isolation"),
    ] {
        let h = workload(iso, false, 8, 40, 4);
        let v = elle(
            &format!("rw-register-8-{model}"),
            &h.to_edn(),
            "rw-register",
            model,
        );
        assert!(v.valid, "{iso:?}: {}\n{}", v.path.display(), v.output);
    }
}

/// A write skew run under snapshot isolation commits both transactions;
/// Elle reports the history as not serializable, with a G2-item cycle. The
/// same interleaving under serializable isolation fails one commit, and
/// that history is valid. The list-append model is used: its reads reveal
/// the order of versions, which the two anti-dependency edges need.
#[test]
fn elle_reports_g2_item_for_a_snapshot_isolation_write_skew() {
    for iso in [Isolation::Snapshot, Isolation::Serializable] {
        let (_d, e) = db();
        let t = tree(&e);
        let h = History::default();
        let (x, y) = (1u64, 2u64);
        let list = |tx: &mut Transaction, k: u64| -> Vec<i64> {
            tx.get(t, &k.to_be_bytes())
                .unwrap()
                .map(|v| {
                    String::from_utf8(v)
                        .unwrap()
                        .split(',')
                        .map(|s| s.parse().unwrap())
                        .collect()
                })
                .unwrap_or_default()
        };
        // An initial transaction appends to both lists.
        let init = vec![
            Op::Append { key: x, value: 1 },
            Op::Append { key: y, value: 2 },
        ];
        h.record(Event::Invoke, 0, init.clone());
        commit_puts(&e, t, &[(&x.to_be_bytes(), b"1"), (&y.to_be_bytes(), b"2")]);
        h.record(Event::Ok, 0, init);
        // T1 reads both and appends to x; T2 reads both and appends to y.
        let ops = |rx: Option<Vec<i64>>, ry: Option<Vec<i64>>, key, value| {
            vec![
                Op::ReadList { key: x, result: rx },
                Op::ReadList { key: y, result: ry },
                Op::Append { key, value },
            ]
        };
        let mut t1 = e.begin(iso).unwrap();
        let mut t2 = e.begin(iso).unwrap();
        h.record(Event::Invoke, 1, ops(None, None, x, 11));
        h.record(Event::Invoke, 2, ops(None, None, y, 22));
        let (r1x, r1y) = (list(&mut t1, x), list(&mut t1, y));
        let (r2x, r2y) = (list(&mut t2, x), list(&mut t2, y));
        t1.put(t, &x.to_be_bytes(), b"1,11").unwrap();
        t2.put(t, &y.to_be_bytes(), b"2,22").unwrap();
        let done = |ok: bool| if ok { Event::Ok } else { Event::Fail };
        let ok1 = t1.commit().is_ok();
        let ok2 = t2.commit().is_ok();
        h.record(done(ok1), 1, ops(Some(r1x), Some(r1y), x, 11));
        h.record(done(ok2), 2, ops(Some(r2x), Some(r2y), y, 22));
        // A final reader shows the order of the versions.
        let mut r = e.begin(iso).unwrap();
        let fin = vec![
            Op::ReadList {
                key: x,
                result: None,
            },
            Op::ReadList {
                key: y,
                result: None,
            },
        ];
        h.record(Event::Invoke, 3, fin);
        let (fx, fy) = (list(&mut r, x), list(&mut r, y));
        r.commit().unwrap();
        h.record(
            Event::Ok,
            3,
            vec![
                Op::ReadList {
                    key: x,
                    result: Some(fx),
                },
                Op::ReadList {
                    key: y,
                    result: Some(fy),
                },
            ],
        );
        let name = format!("write-skew-{iso:?}");
        let v = elle(&name, &h.to_edn(), "list-append", "serializable");
        if iso == Isolation::Snapshot {
            assert!(ok1 && ok2);
            assert!(!v.valid, "{}", v.output);
            assert!(v.anomalies.iter().any(|a| a == "G2-item"), "{}", v.output);
        } else {
            assert!(ok1 && !ok2);
            assert!(v.valid, "{}", v.output);
        }
    }
}

// ---------------------------------------------------------------------------
// Elle on graph operations (rmp #35).
//
// The transactions follow the seven templates of Turso's chaotic Elle
// workloads, `testing/concurrent-simulator/chaotic_elle.rs` (R-TURSO @
// `8cc059b`, MIT licence, "Copyright 2024 the Turso authors";
// <https://github.com/tursodatabase/turso>), copied and adapted: the
// templates build `Op`s of this crate's recorder directly, keys are
// integers, and the template weights are Turso's (rw-register favours
// read-modify-write 3 to 1). Turso's licence notice: "Permission is hereby
// granted, free of charge, to any person obtaining a copy of this software
// and associated documentation files (the "Software"), to deal in the
// Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions: The above
// copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED
// "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED."

/// One planned transaction: its operations, and whether it rolls back.
struct Plan {
    ops: Vec<Op>,
    rollback: bool,
}

/// The template generator of Turso's `ChaoticElleProfile`, for `keys`
/// keys; values come from one counter, so each is unique.
struct Templates<'a> {
    list: bool,
    keys: u64,
    next: &'a crate::sync::atomic::AtomicU64,
}

impl Templates<'_> {
    fn value(&self) -> i64 {
        let v = self
            .next
            .fetch_add(1, crate::sync::atomic::Ordering::Relaxed);
        i64::try_from(v).expect("fewer than 2^63 values")
    }

    fn read(&self, key: u64) -> Op {
        if self.list {
            Op::ReadList { key, result: None }
        } else {
            Op::Read { key, result: None }
        }
    }

    fn write(&self, key: u64) -> Op {
        let value = self.value();
        if self.list {
            Op::Append { key, value }
        } else {
            Op::Write { key, value }
        }
    }

    fn distinct(&self, rng: &mut StdRng, n: usize) -> Vec<u64> {
        let mut keys = Vec::with_capacity(n);
        while keys.len() < n {
            let k = rng.random_range(0..self.keys);
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        keys
    }

    /// Picks a template, with Turso's weights, and plans it.
    fn plan(&self, rng: &mut StdRng) -> Plan {
        let ops = |ops| Plan {
            ops,
            rollback: false,
        };
        let pick = if self.list {
            rng.random_range(0..7u32)
        } else {
            // 0..=2: read-modify-write; then one in nine each.
            match rng.random_range(0..9u32) {
                0..=2 => 2,
                3 => 0,
                4 => 1,
                5 => 3,
                6 => 4,
                7 => 5,
                _ => 6,
            }
        };
        match pick {
            // ReadThenWriteElsewhere: G-single, G2.
            0 => {
                let k = self.distinct(rng, 2);
                ops(vec![self.read(k[0]), self.write(k[1])])
            }
            // MultiKeyReader: snapshot inconsistency.
            1 => {
                let n = rng.random_range(2..=4);
                ops(self
                    .distinct(rng, n)
                    .into_iter()
                    .map(|k| self.read(k))
                    .collect())
            }
            // ReadModifyWrite: G0, G2.
            2 => {
                let k = rng.random_range(0..self.keys);
                ops(vec![self.read(k), self.write(k)])
            }
            // Aborter: G1a.
            3 => {
                let n = rng.random_range(1..=3);
                Plan {
                    ops: (0..n)
                        .map(|_| self.write(rng.random_range(0..self.keys)))
                        .collect(),
                    rollback: true,
                }
            }
            // LongReader: snapshot stability.
            4 => {
                let k = rng.random_range(0..self.keys);
                let mut v = vec![self.read(k)];
                for _ in 0..rng.random_range(1..=3) {
                    let mut o = rng.random_range(0..self.keys);
                    while o == k {
                        o = rng.random_range(0..self.keys);
                    }
                    v.push(if rng.random_bool(0.5) {
                        self.read(o)
                    } else {
                        self.write(o)
                    });
                }
                v.push(self.read(k));
                ops(v)
            }
            // BlindMultiWriter: G0.
            5 => {
                let n = rng.random_range(2..=3);
                ops((0..n)
                    .map(|_| self.write(rng.random_range(0..self.keys)))
                    .collect())
            }
            // MixedReadWrite: general dependencies.
            _ => {
                let n = rng.random_range(2..=5);
                ops((0..n)
                    .map(|_| {
                        let k = rng.random_range(0..self.keys);
                        if rng.random_bool(0.5) {
                            self.read(k)
                        } else {
                            self.write(k)
                        }
                    })
                    .collect())
            }
        }
    }
}

/// The graph of [`graph_workload`]: list `k` is property `L` of node
/// `nodes[k]`, a list of integers (absent: empty); register `k` is
/// property `V` of edge `edges[k]` (absent: nil).
struct GraphKeys {
    nodes: Vec<u64>,
    edges: Vec<u64>,
    l: u64,
    v: u64,
}

fn graph_keys(e: &Engine, keys: u64) -> GraphKeys {
    use crate::catalog::{DEFAULT_GRAPH_ID, TokenKind};
    let label = e.token(TokenKind::Label, "E").unwrap();
    let ty = e.token(TokenKind::EdgeType, "R").unwrap();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
    let hub = tx.create_node(&g, &[label], &[]).unwrap();
    let nodes: Vec<u64> = (0..keys)
        .map(|_| tx.create_node(&g, &[label], &[]).unwrap())
        .collect();
    let edges = nodes
        .iter()
        .map(|&n| tx.create_edge(&g, hub, n, ty, &[]).unwrap())
        .collect();
    tx.commit().unwrap();
    GraphKeys {
        nodes,
        edges,
        l: e.token(TokenKind::PropertyKey, "L").unwrap(),
        v: e.token(TokenKind::PropertyKey, "V").unwrap(),
    }
}

/// Runs `plan` in one transaction at `iso` on the graph: an append reads
/// the node's list and sets it with the value appended; a write sets the
/// edge's property. `true` if it committed.
fn run_graph_ops(e: &Engine, k: &GraphKeys, iso: Isolation, plan: &mut Plan) -> bool {
    use crate::catalog::DEFAULT_GRAPH_ID;
    use crate::storage::error::Result;
    use crate::types::Value;
    let mut tx = e.begin(iso).unwrap();
    let list = |tx: &mut Transaction, key: u64| -> Result<Vec<i64>> {
        let g = tx.graph(DEFAULT_GRAPH_ID)?;
        let (_, props) = tx.node(&g, k.nodes[key as usize])?.expect("the node");
        Ok(match props.iter().find(|p| p.0 == k.l) {
            None => Vec::new(),
            Some((_, Value::List(l))) => l
                .iter()
                .map(|x| match x {
                    Value::Int(i) => *i,
                    x => panic!("list element {x:?}"),
                })
                .collect(),
            Some(p) => panic!("property L: {p:?}"),
        })
    };
    for op in &mut plan.ops {
        let r = match op {
            Op::Append { key, value } => list(&mut tx, *key).and_then(|mut l| {
                l.push(*value);
                let g = tx.graph(DEFAULT_GRAPH_ID)?;
                let l: Vec<Value> = l.into_iter().map(Value::Int).collect();
                tx.set_property(&g, k.nodes[*key as usize], k.l, Value::List(l.into()))
            }),
            Op::ReadList { key, result } => list(&mut tx, *key).map(|l| *result = Some(l)),
            Op::Write { key, value } => tx.graph(DEFAULT_GRAPH_ID).and_then(|g| {
                tx.set_edge(&g, k.edges[*key as usize], &[(k.v, Value::Int(*value))])
            }),
            Op::Read { key, result } => tx.graph(DEFAULT_GRAPH_ID).and_then(|g| {
                let edge = tx.edge(&g, k.edges[*key as usize])?.expect("the edge");
                *result = Some(edge.props.iter().find(|p| p.0 == k.v).map(|p| match p.1 {
                    Value::Int(i) => i,
                    ref x => panic!("property V: {x:?}"),
                }));
                Ok(())
            }),
        };
        if r.is_err() {
            return false;
        }
    }
    if plan.rollback {
        tx.abort();
        return false;
    }
    tx.commit().is_ok()
}

/// [`workload`] on the graph ([`GraphKeys`]), with the transactions of
/// [`Templates`].
fn graph_workload(iso: Isolation, list: bool, threads: usize, txs: usize, keys: u64) -> History {
    let (_d, e) = db();
    let k = graph_keys(&e, keys);
    let h = History::default();
    let next = crate::sync::atomic::AtomicU64::new(1);
    let base = seed();
    std::thread::scope(|s| {
        for th in 0..threads {
            let (e, h, k) = (&e, &h, &k);
            let t = Templates {
                list,
                keys,
                next: &next,
            };
            s.spawn(move || {
                let mut rng = StdRng::seed_from_u64(base.wrapping_add(th as u64));
                for _ in 0..txs {
                    let mut plan = t.plan(&mut rng);
                    h.record(Event::Invoke, th, plan.ops.clone());
                    let ok = run_graph_ops(e, k, iso, &mut plan);
                    h.record(if ok { Event::Ok } else { Event::Fail }, th, plan.ops);
                }
            });
        }
    });
    e.close().unwrap();
    h
}

/// rmp #35: eight threads of Turso's chaotic templates on graph operations
/// (list-append on node list properties, rw-register on edge properties),
/// at each level: Elle finds no anomaly under the level's consistency
/// model.
#[test]
fn elle_finds_no_anomaly_in_eight_thread_graph_histories() {
    for (iso, model) in [
        (Isolation::Serializable, "serializable"),
        (Isolation::Snapshot, "snapshot-isolation"),
    ] {
        for (list, keys, name) in [(true, 6, "list-append"), (false, 4, "rw-register")] {
            let h = graph_workload(iso, list, 8, 40, keys);
            let v = elle(&format!("graph-{name}-8-{model}"), &h.to_edn(), name, model);
            assert!(
                v.valid,
                "{iso:?} {name}: {}\n{}",
                v.path.display(),
                v.output
            );
        }
    }
}
