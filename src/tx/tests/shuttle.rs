//! `shuttle` schedules of the commit latch, the WAL writer and the
//! checkpointer (TS-30 Concurrency): the real engine, its threads included,
//! runs under 1 000 random schedules per test. Shuttle explores schedules
//! at random and is not a proof ("Shuttle is not sound", D-SHUTTLE); the
//! `loom` models cover the small protocols exhaustively.

use super::*;
use crate::storage::error::Error;

/// Two threads increment one counter with read-modify-write transactions,
/// one under snapshot isolation and one serializable; a third writes
/// disjoint keys, a fourth runs checkpoints. Every
/// increment that committed is counted exactly once (no lost update), each
/// failed one failed with `Serialization`, and every disjoint write is
/// present after a clean close and reopen.
#[test]
fn shuttle_commits_and_checkpoints_under_random_schedules() {
    ::shuttle::check_random(
        || {
            let (d, e) = db();
            let t = tree(&e);
            commit_puts(&e, t, &[(b"n", b"0")]);
            let committed = ::shuttle::thread::scope(|s| {
                // One incrementer per level: a lost update needs the
                // write-write rule to be stopped under snapshot isolation.
                let incr: Vec<_> = [Isolation::Snapshot, Isolation::Serializable]
                    .into_iter()
                    .map(|iso| {
                        let e = &e;
                        s.spawn(move || {
                            let mut ok = 0u64;
                            for _ in 0..2 {
                                let mut tx = e.begin(iso).unwrap();
                                let n: u64 = String::from_utf8(tx.get(t, b"n").unwrap().unwrap())
                                    .unwrap()
                                    .parse()
                                    .unwrap();
                                let r = tx
                                    .put(t, b"n", (n + 1).to_string().as_bytes())
                                    .and_then(|()| tx.commit().map(drop));
                                match r {
                                    Ok(()) => ok += 1,
                                    Err(Error::Serialization | Error::Aborted) => {}
                                    Err(other) => panic!("{other:?}"),
                                }
                            }
                            ok
                        })
                    })
                    .collect();
                s.spawn(|| {
                    for i in 0..3u8 {
                        commit_puts(&e, t, &[(&[b'd', i], &[i])]);
                    }
                });
                s.spawn(|| {
                    e.checkpoint().unwrap();
                    e.checkpoint().unwrap();
                });
                incr.into_iter().map(|h| h.join().unwrap()).sum::<u64>()
            });
            assert!(committed >= 1);
            assert_eq!(read(&e, t, b"n"), Some(committed.to_string().into_bytes()));
            e.close().unwrap();
            let e = reopen(d.path());
            assert_eq!(read(&e, t, b"n"), Some(committed.to_string().into_bytes()));
            for i in 0..3u8 {
                assert_eq!(read(&e, t, &[b'd', i]), Some(vec![i]));
            }
            let r = e.store().check().unwrap();
            assert!(r.is_ok(), "{:?}", r.problems);
            e.close().unwrap();
        },
        1000,
    );
}

/// Batches committed without the transaction layer (TS-16 steps 2, 3,
/// 6-9) from two threads while a third runs checkpoints: under 1 000
/// random schedules, every batch is applied once and present after reopen,
/// and the integrity check passes.
#[test]
fn shuttle_batches_and_checkpoints_under_random_schedules() {
    use crate::storage::btree::Write;
    use crate::tx::commit::Ids;
    ::shuttle::check_random(
        || {
            let (d, e) = db();
            let t = tree(&e);
            ::shuttle::thread::scope(|s| {
                for th in 0..2u8 {
                    let e = &e;
                    s.spawn(move || {
                        for i in 0..3u8 {
                            let k = [th, i];
                            let w = Write {
                                tree: t,
                                key: &k,
                                value: Some(&k),
                                stamp: 0,
                            };
                            e.commit(&[w], Ids::default()).unwrap();
                        }
                    });
                }
                s.spawn(|| {
                    e.checkpoint().unwrap();
                    e.checkpoint().unwrap();
                });
            });
            drop(e);
            let e = reopen(d.path());
            let mut n = 0;
            e.store()
                .scan_prefix(t, b"", |k, v| {
                    assert_eq!(k, v);
                    n += 1;
                    true
                })
                .unwrap();
            assert_eq!(n, 6);
            let r = e.store().check().unwrap();
            assert!(r.is_ok(), "{:?}", r.problems);
            e.close().unwrap();
        },
        1000,
    );
}
