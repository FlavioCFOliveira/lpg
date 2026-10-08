use crate::sync::Arc;
use crate::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::collections::BTreeMap;

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{RngExt, SeedableRng};

use super::node::{self, BODY, MAX_INLINE, USABLE};
use super::*;
use crate::obs::recording::RecordingSink;
use crate::storage::engine::Engine;
use crate::storage::page::PAGE_SIZE;
use crate::test_support::test_dirs;

pub(crate) fn engine(dir: &std::path::Path, cap: u64) -> Engine {
    Engine::create(
        dir,
        crate::test_support::no_flush(),
        cap,
        Arc::new(RecordingSink::default()),
    )
    .unwrap()
}

/// The adjacency-shaped key `(node, direction, type, neighbour, edge)`,
/// big-endian, 29 bytes (TS-06).
pub(crate) fn adj_key(node: u64, dir: u8, ty: u32, nbr: u64, edge: u64) -> Vec<u8> {
    let mut k = Vec::with_capacity(29);
    k.extend_from_slice(&node.to_be_bytes());
    k.push(dir);
    k.extend_from_slice(&ty.to_be_bytes());
    k.extend_from_slice(&nbr.to_be_bytes());
    k.extend_from_slice(&edge.to_be_bytes());
    k
}

type Model = BTreeMap<Vec<u8>, Vec<u8>>;

fn put(s: &Store<'_>, tree: Tree, k: &[u8], v: &[u8]) {
    s.apply(&[Write {
        tree,
        key: k,
        value: Some(v),
        stamp: 0,
    }])
    .unwrap();
}

fn scan(s: &Store<'_>, tree: Tree, prefix: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    s.scan_prefix(tree, prefix, |k, v| {
        out.push((k.to_vec(), v.to_vec()));
        true
    })
    .unwrap();
    out
}

fn model_scan(m: &Model, prefix: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    m.range(prefix.to_vec()..)
        .take_while(|(k, _)| k.starts_with(prefix))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn assert_check_ok(s: &Store<'_>) -> crate::storage::check::Report {
    let r = s.check().unwrap();
    assert!(
        r.is_ok(),
        "integrity check: {:#?}",
        &r.problems[..r.problems.len().min(20)]
    );
    r
}

fn tree_stats(r: &crate::storage::check::Report, id: u64) -> &crate::storage::check::TreeStats {
    r.trees.iter().find(|t| t.id == id).expect("tree in report")
}

fn value_for(rng: &mut StdRng) -> Vec<u8> {
    let len = if rng.random_range(0..2000) == 0 {
        rng.random_range(MAX_INLINE + 1..4 * node::OVERFLOW_CAP)
    } else {
        rng.random_range(0..48)
    };
    let b: u8 = rng.random();
    (0..len).map(|i| b.wrapping_add(i as u8)).collect()
}

/// AC: 10^6 random puts and deletes; `get` and prefix scans match a
/// `BTreeMap` model, and the integrity check passes after every 10^4
/// operations. Three phases (grow, shrink, grow) so that pages are freed
/// and reused.
#[test]
fn a_million_random_puts_and_deletes_match_a_btreemap_model() {
    const OPS: usize = 1_000_000;
    const BATCH: usize = 100;
    let dir = &test_dirs()[0];
    let e = engine(dir.path(), 64 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let mut model = Model::new();
    let mut rng = StdRng::seed_from_u64(14);
    let mut keys: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::new();
    let (mut max_free, mut reused_after_peak) = (0u64, false);
    let mut cursor: Vec<u8> = Vec::new();
    let mut done = 0;
    while done < OPS {
        let put_share = match done {
            0..400_000 => 75,
            400_000..750_000 => 20,
            _ => 65,
        };
        let mut batch_keys: Vec<(Vec<u8>, Option<Vec<u8>>)> = Vec::with_capacity(BATCH);
        for _ in 0..BATCH {
            if rng.random_range(0..100) < put_share || keys.is_empty() {
                let k = adj_key(
                    rng.random_range(0..3000),
                    rng.random_range(0..2),
                    rng.random_range(0..4),
                    rng.random_range(0..3000),
                    rng.random_range(0..1 << 20),
                );
                batch_keys.push((k, Some(value_for(&mut rng))));
            } else {
                // Existing keys: in the shrink phase mostly a contiguous run
                // from a cursor, which empties whole leaves and subtrees;
                // otherwise random ones, and sometimes absent ones.
                let k = if put_share < 50 && rng.random_range(0..4) != 0 {
                    match model.range(cursor.clone()..).next() {
                        Some((k, _)) => {
                            cursor = k.clone();
                            cursor.push(0);
                            k.clone()
                        }
                        None => {
                            cursor.clear();
                            continue;
                        }
                    }
                } else if rng.random_range(0..10) == 0 {
                    adj_key(rng.random_range(0..3000), 0, 0, 0, 0)
                } else {
                    let i = rng.random_range(0..keys.len());
                    keys.swap_remove(i)
                };
                batch_keys.push((k, None));
            }
        }
        let batch: Vec<Write<'_>> = batch_keys
            .iter()
            .map(|(k, v)| Write {
                tree,
                key: k,
                value: v.as_deref(),
                stamp: 0,
            })
            .collect();
        s.apply(&batch).unwrap();
        for (k, v) in batch_keys {
            match v {
                Some(v) => {
                    if model.insert(k.clone(), v).is_none() {
                        keys.push(k);
                    }
                }
                None => {
                    model.remove(&k);
                }
            }
        }
        // `keys` may hold a deleted key twice; resync it with the model.
        done += BATCH;
        if done % 1_000 == 0 {
            cursor = adj_key(rng.random_range(0..3000), 0, 0, 0, 0);
        }
        if done % 10_000 == 0 {
            keys = model.keys().cloned().collect();
            let r = assert_check_ok(&s);
            assert_eq!(tree_stats(&r, tree.id()).entries, model.len() as u64);
            if r.free_pages + r.trunk_pages > max_free {
                max_free = r.free_pages + r.trunk_pages;
            } else if done > 750_000 && r.free_pages + r.trunk_pages < max_free {
                reused_after_peak = true;
            }
            for _ in 0..200 {
                let k = adj_key(
                    rng.random_range(0..3000),
                    rng.random_range(0..2),
                    rng.random_range(0..4),
                    rng.random_range(0..3000),
                    0,
                );
                // A random probe (almost always absent) and a present key.
                assert_eq!(s.get(tree, &k, &mut out).unwrap(), model.contains_key(&k));
                if let Some(k) = keys.get(rng.random_range(0..keys.len().max(1))) {
                    assert!(s.get(tree, k, &mut out).unwrap());
                    assert_eq!(&out, &model[k]);
                }
            }
            for _ in 0..20 {
                let node = rng.random_range(0..3000u64);
                let full = adj_key(node, rng.random_range(0..2), rng.random_range(0..4), 0, 0);
                for prefix in [&full[..8], &full[..13]] {
                    assert_eq!(scan(&s, tree, prefix), model_scan(&model, prefix));
                }
            }
        }
    }
    assert_eq!(scan(&s, tree, &[]), model_scan(&model, &[]));
    assert!(max_free > 0, "the shrink phase freed no page");
    assert!(reused_after_peak, "freed pages were not reused");
}

/// AC: a tree ten times larger than the memory cap reads back intact.
#[test]
fn a_tree_ten_times_larger_than_the_cap_reads_back_intact() {
    const CAP: u64 = 2 << 20;
    let dir = &test_dirs()[0];
    let e = engine(dir.path(), CAP);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let mut rng = StdRng::seed_from_u64(10);
    let mut entries: Vec<(Vec<u8>, Vec<u8>)> = (0..260_000u64)
        .map(|i| {
            let k = adj_key(rng.random_range(0..50_000), 0, 1, rng.random(), i);
            let v = (0..rng.random_range(0..80))
                .map(|j| (i as u8).wrapping_add(j))
                .collect();
            (k, v)
        })
        .collect();
    // A batch copies each existing page it changes into memory reserved
    // from the cap (TS-16 step 6, TS-08 page copies), so a batch must touch
    // fewer pages than the cap holds: 100 random keys change at most about
    // 100 leaves and their parents, about 0.9 MiB of copies under a 2 MiB
    // cap. With 500 keys the copies alone need up to 4 MiB, and the batch
    // fails with `MemoryLimit`, as FR-LIM-007 requires.
    for chunk in entries.chunks(100) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|(k, v)| Write {
                tree,
                key: k,
                value: Some(v),
                stamp: 0,
            })
            .collect();
        s.apply(&batch).unwrap();
    }
    let r = assert_check_ok(&s);
    let bytes = r.page_count * PAGE_SIZE as u64;
    assert!(bytes >= 10 * CAP, "tree of {bytes} B is not 10x the cap");
    entries.shuffle(&mut rng);
    let mut out = Vec::new();
    for (k, v) in &entries {
        assert!(s.get(tree, k, &mut out).unwrap());
        assert_eq!(&out, v);
    }
    entries.sort();
    assert_eq!(scan(&s, tree, &[]), entries);
    assert!(e.accountant().peak() <= CAP);
}

/// AC: overflow values round-trip, through replacement and deletion.
#[test]
fn overflow_values_round_trip() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 16 << 20);
        let s = e.store();
        let tree = s.create_tree().unwrap();
        let sizes = [
            0,
            MAX_INLINE,
            MAX_INLINE + 1,
            node::OVERFLOW_CAP,
            node::OVERFLOW_CAP + 1,
            100_000,
            1 << 20,
        ];
        let value =
            |n: usize, salt: u8| -> Vec<u8> { (0..n).map(|i| (i % 251) as u8 ^ salt).collect() };
        let mut out = Vec::new();
        for (i, &n) in sizes.iter().enumerate() {
            put(&s, tree, &[i as u8], &value(n, 1));
        }
        for (i, &n) in sizes.iter().enumerate() {
            assert!(s.get(tree, &[i as u8], &mut out).unwrap());
            assert_eq!(out, value(n, 1), "size {n}");
        }
        let r = assert_check_ok(&s);
        let expected: u64 = sizes
            .iter()
            .filter(|&&n| n > MAX_INLINE)
            .map(|&n| n.div_ceil(node::OVERFLOW_CAP) as u64)
            .sum();
        assert_eq!(tree_stats(&r, tree.id()).overflow, expected);
        // Replace each value by the next size: inline <-> overflow.
        for (i, &n) in sizes.iter().enumerate() {
            let n2 = sizes[(i + 1) % sizes.len()];
            put(&s, tree, &[i as u8], &value(n2, 2));
            let _ = n;
        }
        for i in 0..sizes.len() {
            assert!(s.get(tree, &[i as u8], &mut out).unwrap());
            assert_eq!(out, value(sizes[(i + 1) % sizes.len()], 2));
        }
        let got = scan(&s, tree, &[]);
        assert_eq!(got.len(), sizes.len());
        let r = assert_check_ok(&s);
        assert!(r.free_pages > 0, "replaced chains were not freed");
        for i in 0..sizes.len() {
            s.apply(&[Write {
                tree,
                key: &[i as u8],
                value: None,
                stamp: 0,
            }])
            .unwrap();
        }
        let r = assert_check_ok(&s);
        assert_eq!(tree_stats(&r, tree.id()).overflow, 0);
        assert_eq!(tree_stats(&r, tree.id()).entries, 0);
    }
}

/// AC: keys of 1 024 bytes work, splits included; a longer key is
/// refused and the batch changes nothing.
#[test]
fn keys_at_the_limit_work_and_longer_keys_are_refused() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 16 << 20);
        let s = e.store();
        let tree = s.create_tree().unwrap();
        let mut rng = StdRng::seed_from_u64(1024);
        let mut model = Model::new();
        for i in 0..300u32 {
            // Keys share no prefix beyond their first bytes, so delta
            // encoding saves nothing; values at the inline maximum.
            let mut k: Vec<u8> = (0..MAX_KEY).map(|_| rng.random()).collect();
            k[..4].copy_from_slice(&i.to_be_bytes());
            let v: Vec<u8> = (0..MAX_INLINE).map(|_| rng.random()).collect();
            put(&s, tree, &k, &v);
            model.insert(k, v);
        }
        let r = assert_check_ok(&s);
        assert!(tree_stats(&r, tree.id()).height >= 2);
        let mut out = Vec::new();
        for (k, v) in &model {
            assert!(s.get(tree, k, &mut out).unwrap());
            assert_eq!(&out, v);
        }
        let long = vec![7u8; MAX_KEY + 1];
        let before = (e.data().page_count(), s.check().unwrap().trees);
        let batch = [
            Write {
                tree,
                key: b"ok",
                value: Some(b"v"),
                stamp: 0,
            },
            Write {
                tree,
                key: &long,
                value: Some(b"v"),
                stamp: 0,
            },
        ];
        assert!(matches!(s.apply(&batch), Err(Error::InvalidArgument(_))));
        assert!(
            !s.get(tree, b"ok", &mut out).unwrap(),
            "batch was partly applied"
        );
        assert_eq!((e.data().page_count(), s.check().unwrap().trees), before);
        assert!(!s.get(tree, &long, &mut out).unwrap());
        e.check().unwrap();
    }
}

/// Fill of each leaf: bytes used (high key, entries, restart array) over
/// the usable bytes, in key order, read through the right links.
fn leaf_fills(s: &Store<'_>, tree: Tree) -> Vec<f64> {
    // Left-most leaf: follow left-most children down.
    let mut p = tree.root();
    loop {
        let pin = s.pool.fetch(p).unwrap();
        let g = pin.read().unwrap();
        if node::level(&g) == 0 {
            break;
        }
        p = node::leftmost(&g);
    }
    let mut fills = Vec::new();
    while p != node::NONE {
        let pin = s.pool.fetch(p).unwrap();
        let g = pin.read().unwrap();
        let v = node::leaf_entries(&g, |_, _| {}).unwrap();
        let used = node::high_key(&g).map_or(0, <[u8]>::len) + v.used_bytes();
        fills.push(used as f64 / USABLE as f64);
        p = node::right(&g);
    }
    fills
}

/// AC: when the right-most leaf splits, the left page is left 90 % full;
/// other splits leave both pages at least half full.
#[test]
fn rightmost_split_fills_left_page_to_90_percent_and_others_to_half() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 16 << 20);
        let s = e.store();
        // Appends: every split is a right-most split.
        let seq = s.create_tree().unwrap();
        let v = [5u8; 40];
        for i in 0..20_000u64 {
            put(&s, seq, &adj_key(i / 7, 0, 0, i, i), &v);
        }
        let fills = leaf_fills(&s, seq);
        assert!(fills.len() > 20);
        // One entry is at most 3 + 16 + 1 + 40 bytes here.
        let entry = 60.0 / USABLE as f64;
        for (i, f) in fills[..fills.len() - 1].iter().enumerate() {
            assert!(*f <= 0.90 && *f > 0.90 - entry, "leaf {i}: fill {f:.4}");
        }
        // Random order: splits are balanced.
        let rnd = s.create_tree().unwrap();
        let mut rng = StdRng::seed_from_u64(9);
        for i in 0..20_000u64 {
            put(
                &s,
                rnd,
                &adj_key(rng.random_range(0..5000), 0, 0, rng.random(), i),
                &v,
            );
        }
        let fills = leaf_fills(&s, rnd);
        let min = fills[..fills.len() - 1].iter().copied().fold(1.0, f64::min);
        assert!(min >= 0.5 - entry, "minimum leaf fill {min:.4}");
        let mean = fills.iter().sum::<f64>() / fills.len() as f64;
        assert!(mean > 0.6 && mean < 0.8, "mean leaf fill {mean:.4}");
        assert_check_ok(&s);
        let _ = BODY;
    }
}

/// The value of a stable key: 100 bytes derived from it.
fn stable_value(k: &[u8]) -> Vec<u8> {
    k[k.len() - 4..].repeat(25)
}

/// AC: readers running during splits (B-link) return every committed key
/// exactly once. One writer inserts stable keys in random order, causing
/// leaf, inner and root splits, while inserting and deleting volatile
/// keys, which empties, frees and reuses pages. Readers check that every
/// stable key committed before their operation started is found, and that
/// scans return keys strictly ascending with each committed stable key
/// exactly once.
#[test]
fn concurrent_readers_during_splits_see_every_committed_key_exactly_once() {
    const STABLE: usize = 60_000;
    const READERS: usize = 4;
    let dir = &test_dirs()[0];
    // A small cap, so pages are also evicted and re-read meanwhile.
    let e = engine(dir.path(), 4 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let mut rng = StdRng::seed_from_u64(77);
    let mut stable: Vec<Vec<u8>> = (0..STABLE as u64)
        .map(|i| {
            let mut k = b"s".to_vec();
            k.extend_from_slice(&adj_key(rng.random_range(0..2000), 0, 0, rng.random(), i));
            k
        })
        .collect();
    stable.shuffle(&mut rng);
    let stable = &stable;
    let committed = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let reads = AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for r in 0..READERS {
            let (committed, stop, reads) = (&committed, &stop, &reads);
            sc.spawn(move || {
                let mut rng = StdRng::seed_from_u64(1000 + r as u64);
                let mut out = Vec::new();
                while !stop.load(Ordering::Acquire) {
                    let c = committed.load(Ordering::Acquire);
                    if c == 0 {
                        continue;
                    }
                    if r % 2 == 0 {
                        for _ in 0..100 {
                            let k = &stable[rng.random_range(0..c)];
                            assert!(s.get(tree, k, &mut out).unwrap(), "committed key missing");
                            assert_eq!(out, stable_value(k));
                        }
                    } else {
                        let mut seen: Vec<Vec<u8>> = Vec::with_capacity(c + 1000);
                        s.scan_prefix(tree, &[], |k, _| {
                            if let Some(last) = seen.last() {
                                assert!(k > last.as_slice(), "scan not strictly ascending");
                            }
                            seen.push(k.to_vec());
                            true
                        })
                        .unwrap();
                        let mut want: Vec<&Vec<u8>> = stable[..c].iter().collect();
                        want.sort();
                        let got: Vec<&Vec<u8>> = seen.iter().filter(|k| k[0] == b's').collect();
                        // Every committed stable key once; later ones may
                        // also appear.
                        let mut j = 0;
                        for w in want {
                            while j < got.len() && got[j] < w {
                                j += 1;
                            }
                            assert!(j < got.len() && got[j] == w, "committed key missed by scan");
                        }
                    }
                    reads.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        let mut wrng = StdRng::seed_from_u64(78);
        let mut volatile: Vec<Vec<u8>> = Vec::new();
        let mut max_listed = 0;
        for (i, chunk) in stable.chunks(50).enumerate() {
            let mut keys: Vec<(Vec<u8>, Option<Vec<u8>>)> = chunk
                .iter()
                .map(|k| (k.clone(), Some(stable_value(k))))
                .collect();
            // Volatile keys: inserted in bursts, then deleted, so leaves
            // in their range empty and are freed while readers run.
            if i % 20 < 10 {
                for _ in 0..60 {
                    let mut k = b"v".to_vec();
                    k.extend_from_slice(&wrng.random::<u64>().to_be_bytes());
                    volatile.push(k.clone());
                    keys.push((k, Some(vec![1u8; 100])));
                }
            } else {
                for _ in 0..60 {
                    if let Some(k) = volatile.pop() {
                        keys.push((k, None));
                    }
                }
            }
            let batch: Vec<Write<'_>> = keys
                .iter()
                .map(|(k, v)| Write {
                    tree,
                    key: k,
                    value: v.as_deref(),
                    stamp: 0,
                })
                .collect();
            s.apply(&batch).unwrap();
            committed.store((i + 1) * 50, Ordering::Release);
            max_listed = max_listed.max(s.lock().free_list().pages().count());
        }
        // Reads overlapped the writes, and the writes freed pages.
        assert!(
            reads.load(Ordering::Relaxed) >= 10,
            "readers did not run during the writes"
        );
        assert!(max_listed > 0, "no page was freed while readers ran");
        while reads.load(Ordering::Relaxed) < 50 {
            std::thread::yield_now();
        }
        stop.store(true, Ordering::Release);
    });
    let r = assert_check_ok(&s);
    assert!(
        tree_stats(&r, tree.id()).height >= 3,
        "no inner split happened"
    );
}

/// #14: the scan keeps its key-space position. After the scan has copied
/// the first leaf, every leaf but the last is emptied and freed, so their
/// ranges join the last leaf (TS-05 Underflow), and a key is inserted
/// there below the keys already returned. The scan, which continues at the
/// first leaf's recorded right link, must not return it: keys come back in
/// strictly ascending order, and the key present for the whole scan once.
#[test]
fn a_scan_skips_keys_inserted_behind_it_into_a_range_freed_pages_gave_away() {
    let dir = &test_dirs()[0];
    let e = engine(dir.path(), 16 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let keys: Vec<Vec<u8>> = (0..3000).map(|i| format!("a{i:05}").into_bytes()).collect();
    let v = [7u8; 100];
    let mut batch: Vec<Write<'_>> = keys
        .iter()
        .map(|k| Write {
            tree,
            key: k,
            value: Some(&v),
            stamp: 0,
        })
        .collect();
    batch.push(Write {
        tree,
        key: b"z",
        value: Some(&v),
        stamp: 0,
    });
    s.apply(&batch).unwrap();
    let behind = b"a00000b".to_vec();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    s.scan_prefix(tree, &[], |k, _| {
        if seen.is_empty() {
            let deletes: Vec<Write<'_>> = keys
                .iter()
                .map(|k| Write {
                    tree,
                    key: k,
                    value: None,
                    stamp: 0,
                })
                .collect();
            s.apply(&deletes).unwrap();
            put(&s, tree, &behind, &v);
        }
        seen.push(k.to_vec());
        true
    })
    .unwrap();
    assert!(
        seen.windows(2).all(|w| w[0] < w[1]),
        "scan not strictly ascending"
    );
    assert_eq!(seen.iter().filter(|k| k.as_slice() == b"z").count(), 1);
    assert!(
        seen.len() > 1 && seen.len() < keys.len(),
        "the first leaf was copied"
    );
    assert_check_ok(&s);
}

#[test]
fn directory_maps_tree_ids_to_roots_and_survives_reopen() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 16 << 20);
        let s = e.store();
        let trees: Vec<Tree> = (0..300).map(|_| s.create_tree().unwrap()).collect();
        for (i, t) in trees.iter().enumerate() {
            assert_eq!(t.id(), i as u64 + 1);
            put(&s, *t, b"k", &t.id().to_le_bytes());
            assert_eq!(s.tree(t.id()).unwrap(), Some(*t));
        }
        assert_eq!(s.tree(301).unwrap(), None);
        assert_check_ok(&s);
        e.pool().write_dirty().unwrap();
        e.data().sync().unwrap();
        drop(e);
        let e = Engine::open(
            dir.path(),
            crate::test_support::no_flush(),
            16 << 20,
            Arc::new(RecordingSink::default()),
        )
        .unwrap();
        let s = e.store();
        let mut out = Vec::new();
        for t in &trees {
            assert_eq!(s.tree(t.id()).unwrap(), Some(*t));
            assert!(s.get(*t, b"k", &mut out).unwrap());
            assert_eq!(out, t.id().to_le_bytes());
        }
        assert_eq!(s.create_tree().unwrap().id(), 301);
        let r = assert_check_ok(&s);
        assert_eq!(r.trees.len(), 302);
    }
}

#[test]
fn a_failure_after_the_first_change_makes_the_database_unusable() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 16 << 20);
        let s = e.store();
        let tree = s.create_tree().unwrap();
        put(&s, tree, b"a", b"1");
        // Corrupt the root leaf in the pool: the next put fails mid-batch.
        {
            let pin = e.pool().fetch(tree.root()).unwrap();
            let mut g = pin.write().unwrap();
            g[26] = 0xFF; // entry count
        }
        let r = s.apply(&[Write {
            tree,
            key: b"b",
            value: Some(b"2"),
            stamp: 0,
        }]);
        assert!(matches!(r, Err(Error::Corruption(_))), "{r:?}");
        assert!(matches!(e.check(), Err(Error::Unusable)));
    }
}

/// B-link recovery, deterministically: a reader that reaches a page
/// through a pointer read before a split (the left half) or before a
/// delete (a page since marked deleted) still finds every key by moving
/// right. A `Tree` handle rooted at that page stands for the stale
/// pointer.
#[test]
fn stale_descents_recover_by_moving_right() {
    let dir = &test_dirs()[0];
    let e = engine(dir.path(), 16 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let key = |i: u64| adj_key(i, 0, 0, i, i);
    let v = [1u8; 100];
    for i in (0..4000).step_by(2) {
        put(&s, tree, &key(i), &v);
    }
    let leaf_of = |k: &[u8]| {
        let mut p = tree.root();
        loop {
            let pin = s.pool.fetch(p).unwrap();
            let g = pin.read().unwrap();
            if node::level(&g) == 0 {
                return p;
            }
            p = node::inner_child(&g, k).unwrap().1;
        }
    };
    let mut out = Vec::new();
    // Split: remember the leaf holding key 2000, then fill it until it
    // splits more than once.
    let l = leaf_of(&key(2000));
    for i in (1001..3001).step_by(2) {
        put(&s, tree, &key(i), &v);
    }
    assert_ne!(leaf_of(&key(2999)), l, "the leaf did not split");
    let stale = Tree::new(tree.id(), l);
    for i in 2000..3001 {
        assert!(
            s.get(stale, &key(i), &mut out).unwrap(),
            "key {i} lost after a split"
        );
    }
    let from_stale: Vec<_> = scan(&s, stale, &[]).into_iter().map(|(k, _)| k).collect();
    let all: Vec<_> = scan(&s, tree, &[]).into_iter().map(|(k, _)| k).collect();
    assert!(all.ends_with(&from_stale) && from_stale.len() > all.len() / 2);
    // Delete: empty a leaf in the middle; it is freed and marked deleted.
    let d = leaf_of(&key(1500));
    let doomed: Vec<Vec<u8>> = scan(&s, tree, &[])
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| leaf_of(k) == d)
        .collect();
    // A reader registered before the delete keeps the page from reuse.
    let reader = s.reader();
    for k in &doomed {
        s.apply(&[Write {
            tree,
            key: k,
            value: None,
            stamp: 0,
        }])
        .unwrap();
    }
    {
        let pin = s.pool.fetch(d).unwrap();
        assert!(
            node::is_deleted(&pin.read().unwrap()),
            "the emptied leaf was not freed"
        );
    }
    let stale = Tree::new(tree.id(), d);
    let after = key(u64::from_be_bytes(doomed.last().unwrap()[..8].try_into().unwrap()) + 1);
    assert!(
        s.get(stale, &after, &mut out).unwrap(),
        "key right of a deleted page lost"
    );
    assert!(!s.get(stale, &doomed[0], &mut out).unwrap());
    // A key of the deleted page's range, put again, now lives in its right
    // sibling, below the deleted page's high key.
    put(&s, tree, &doomed[0], b"again");
    assert!(
        s.get(stale, &doomed[0], &mut out).unwrap(),
        "key moved right of a deleted page lost"
    );
    assert_eq!(out, b"again");
    drop(reader);
    assert_check_ok(&s);
}

/// TS-05 Underflow: emptying the key range of an inner page frees its
/// leaves and then the inner page with its last leaf, as one chain; the
/// last child of a parent stays; the height never decreases.
#[test]
fn emptying_a_subtree_frees_its_inner_pages() {
    let dir = &test_dirs()[0];
    let e = engine(dir.path(), 32 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let v = [7u8; 200];
    let keys: Vec<Vec<u8>> = (0..60_000u64).map(|i| adj_key(i, 0, 0, i, i)).collect();
    for chunk in keys.chunks(1000) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(&v),
                stamp: 0,
            })
            .collect();
        s.apply(&batch).unwrap();
    }
    let before = assert_check_ok(&s);
    let b = tree_stats(&before, tree.id()).clone();
    assert_eq!(b.height, 3);
    // The range of the root's second child.
    let (lo, hi) = {
        let pin = s.pool.fetch(tree.root()).unwrap();
        let mut pv = node::Pivots::default();
        node::inner_decode(&pin.read().unwrap(), &mut pv).unwrap();
        assert!(pv.len() >= 2, "root has {} keys", pv.len());
        (pv.key(0).to_vec(), pv.key(1).to_vec())
    };
    let doomed: Vec<&Vec<u8>> = keys.iter().filter(|k| **k >= lo && **k < hi).collect();
    for chunk in doomed.chunks(500) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: None,
                stamp: 0,
            })
            .collect();
        s.apply(&batch).unwrap();
    }
    let after = assert_check_ok(&s);
    let a = tree_stats(&after, tree.id());
    assert_eq!(a.height, 3, "the height decreased");
    assert_eq!(a.inner, b.inner - 1, "the inner page was not freed");
    assert!(a.leaves < b.leaves);
    assert_eq!(a.entries, (keys.len() - doomed.len()) as u64);
    let mut out = Vec::new();
    assert!(!s.get(tree, doomed[0], &mut out).unwrap());
    assert!(s.get(tree, &keys[0], &mut out).unwrap());
    let first_after = keys.iter().find(|k| **k >= hi).unwrap();
    assert!(s.get(tree, first_after, &mut out).unwrap());
    // The freed range takes keys again, now through its right neighbour.
    put(&s, tree, doomed[0], b"back");
    assert!(s.get(tree, doomed[0], &mut out).unwrap());
    assert_check_ok(&s);
}

/// Audit M1 (TS-16 step 6 undo, item 3): a second undo while a fence is in
/// force keeps the first undo's pages fenced and waits for the readers of
/// the later undo, which started no later than those of the first.
#[test]
fn a_second_undo_extends_the_reuse_fence() {
    use super::write::extend_fence;
    let mut fence = None;
    extend_fence(&mut fence, 7, &[]);
    assert!(fence.is_none(), "an undo that took no page sets no fence");
    extend_fence(&mut fence, 7, &[10, 11]);
    extend_fence(&mut fence, 9, &[12]);
    let (batch, pages) = fence.clone().unwrap();
    assert_eq!(batch, 9);
    assert_eq!(pages, [10, 11, 12].into_iter().collect());
    extend_fence(&mut fence, 8, &[11, 13]);
    let (batch, pages) = fence.unwrap();
    assert_eq!(batch, 9, "the higher batch number is kept");
    assert_eq!(pages, [10, 11, 12, 13].into_iter().collect());
}

/// AUDIT E (re-audit 2026-10-03, D1 follow-up; TS-16 step 6 undo, item 3).
/// The D1 fix keeps the batch's image only for a taken page that was not
/// in the pool (`Saved::Absent`, `PinnedPage::undo_keep`). A taken page
/// that was in the pool (`Saved::Copy` in `PageWriter::create`) gets its
/// image from before the batch back, although it is free before and after
/// the batch. A page freed by an earlier commit usually is in the pool. A
/// reader that read the undone batch's link to it (here: the leaf entry of
/// "x" and its first overflow page) then reads the freed page of another
/// tree: corruption, and the unusable latch, after a mere ENOSPC or memory
/// limit (FR-REC-007).
#[test]
fn audit_a_resident_free_page_taken_by_an_undone_batch_stays_readable() {
    use crate::storage::error::Error;
    use crate::tx::commit::Ids;
    use crate::tx::commit::tests::fault;
    let dir = &test_dirs()[0];
    let fault = fault();
    let e = Engine::create(
        dir.path(),
        fault.clone(),
        32 << 20,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let trees = e.create_trees(2).unwrap();
    let (t1, t2) = (trees[0], trees[1]);
    let w = |tree, key: &'static [u8], value: Option<&'static [u8]>| Write {
        tree,
        key,
        value,
        stamp: 0,
    };
    let other: &'static [u8] = Box::leak(vec![5u8; 100_000].into_boxed_slice());
    e.commit(&[w(t2, b"big", Some(other))], Ids::default())
        .unwrap();
    e.commit(&[w(t2, b"big", None)], Ids::default()).unwrap();
    let c0 = e
        .store()
        .lock()
        .free_list()
        .pages()
        .next()
        .expect("freed pages");
    {
        let (pin, _) = e
            .pool()
            .resident(c0)
            .expect("precondition: freed page in the pool");
        assert_eq!(crate::storage::page::tree(&pin.read().unwrap()), t2.id());
    }
    // B: "x" takes the freed pages of t2 (FIFO), "y" overruns the reserve.
    let unused = e.data().unused_pages();
    let x: &'static [u8] = Box::leak(vec![8u8; 100_000].into_boxed_slice());
    let y: &'static [u8] = Box::leak(vec![9u8; ((unused + 64) * 8192) as usize].into_boxed_slice());
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let r = e.commit(
        &[w(t1, b"x", Some(x)), w(t1, b"y", Some(y))],
        Ids::default(),
    );
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    assert!(
        r.as_ref()
            .is_err_and(|e| e.is_no_space() || matches!(e, Error::MemoryLimit { .. })),
        "{r:?}"
    );
    assert!(
        e.check().is_ok(),
        "precondition: the undo left the database usable"
    );
    // The reader that read "x"'s entry (length, first page c0) during B.
    let mut out = Vec::new();
    let read = e.store().read_overflow(t1, x.len() as u64, c0, &mut out);
    let health = e.check();
    assert!(
        health.is_ok(),
        "an ENOSPC undo left the database unusable once a reader followed the undone \
         batch's link to page {c0}: read = {read:?}, check = {health:?}"
    );
}

/// Keys a scan would pass from leaf `page` on, at or above `from`, as a
/// reader that reached `page` by a right link does (TS-06).
fn scan_from(s: &Store<'_>, tree: Tree, mut page: u64, from: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut from = from.to_vec();
    let mut copied = Copied::default();
    let mut key_buf = Vec::new();
    let mut out = Vec::new();
    loop {
        let pin = s.pool.fetch(page)?;
        let g = pin.read()?;
        if crate::storage::page::page_type(&g) != Some(crate::storage::page::PageType::Leaf)
            || crate::storage::page::tree(&g) != tree.id()
        {
            return Err(s.corrupt(page, "not a leaf of this tree"));
        }
        let next = copied
            .take(&g, &mut from, &[], &mut key_buf)
            .ok_or_else(|| s.corrupt(page, "malformed leaf"))?;
        out.extend((0..copied.len()).map(|i| copied.get(i).0.to_vec()));
        match next {
            Some(p) => page = p,
            None => return Ok(out),
        }
    }
}

/// Finding E, same-tree variant (TS-16 step 6 undo, item 3). An undone
/// batch splits leaf L into a resident free page R that was a deleted leaf
/// of the same tree, whose stale right link points far to the right. A
/// reader that read the batch's L moves right to R from the separator. It
/// must find the batch's image of R and pass every committed key from the
/// separator on, exactly once; R's older image would silently skip every
/// key up to the end of its stale link.
#[test]
fn a_reader_moving_right_into_a_split_page_of_an_undone_batch_misses_no_key() {
    use crate::storage::error::Error;
    use crate::tx::commit::Ids;
    use crate::tx::commit::tests::fault;
    let dir = &test_dirs()[0];
    let fault = fault();
    let e = Engine::create(
        dir.path(),
        fault.clone(),
        32 << 20,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let tree = e.create_trees(1).unwrap()[0];
    let keys: Vec<Vec<u8>> = (0..2000).map(|i| format!("k{i:05}").into_bytes()).collect();
    let v = [1u8; 100];
    for chunk in keys.chunks(200) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(&v),
                stamp: 0,
            })
            .collect();
        e.commit(&batch, Ids::default()).unwrap();
    }
    // Empty the leaves of k01500..k01799: freed, marked deleted, in the pool.
    let gone = &keys[1500..1800];
    let batch: Vec<Write<'_>> = gone
        .iter()
        .map(|k| Write {
            tree,
            key: k,
            value: None,
            stamp: 0,
        })
        .collect();
    e.commit(&batch, Ids::default()).unwrap();
    let committed: Vec<Vec<u8>> = keys.iter().filter(|k| !gone.contains(k)).cloned().collect();
    let r = e.store().lock().free_list().pages().next().expect("freed");
    {
        let (pin, _) = e.pool().resident(r).expect("precondition: in the pool");
        let g = pin.read().unwrap();
        assert!(node::is_deleted(&g) && crate::storage::page::tree(&g) == tree.id());
    }
    // The batch: a 2 000-byte value splits the full leaf of k00100 into R;
    // the second value overruns the reserve.
    let split: &[u8] = b"k00100x";
    let wide = [2u8; 2000];
    let huge = vec![9u8; ((e.data().unused_pages() + 64) * 8192) as usize];
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let res = e.commit(
        &[
            Write {
                tree,
                key: split,
                value: Some(&wide),
                stamp: 0,
            },
            Write {
                tree,
                key: b"y",
                value: Some(&huge),
                stamp: 0,
            },
        ],
        Ids::default(),
    );
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    assert!(
        res.as_ref()
            .is_err_and(|e| e.is_no_space() || matches!(e, Error::MemoryLimit { .. })),
        "{res:?}"
    );
    let s = e.store();
    let got = scan_from(&s, tree, r, b"k00100").unwrap();
    // The same put, now committed, splits the same leaf the same way: its
    // high key is the separator the reader started from, and R its right.
    e.commit(
        &[Write {
            tree,
            key: split,
            value: Some(&wide),
            stamp: 0,
        }],
        Ids::default(),
    )
    .unwrap();
    let (sep, right) = s
        .at_leaf(tree, b"k00100", |p, _| {
            Ok((node::high_key(p).unwrap().to_vec(), node::right(p)))
        })
        .unwrap();
    assert_eq!(right, r, "precondition: the split took page R");
    let want: Vec<&Vec<u8>> = committed.iter().filter(|k| **k >= sep).collect();
    let got: Vec<&Vec<u8>> = got
        .iter()
        .filter(|k| k.as_slice() != split && **k >= sep)
        .collect();
    assert!(
        got == want,
        "the reader missed or repeated committed keys: got {} from {:?}, want {} from {:?}",
        got.len(),
        got.first().map(|k| String::from_utf8_lossy(k)),
        want.len(),
        want.first().map(|k| String::from_utf8_lossy(k)),
    );
    assert_check_ok(&s);
}
