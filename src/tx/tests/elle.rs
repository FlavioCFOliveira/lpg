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
        let list = |tx: &mut Transaction<'_>, k: u64| -> Vec<i64> {
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
