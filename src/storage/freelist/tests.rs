use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use super::*;
use crate::obs::recording::RecordingSink;
use crate::storage::FsStorage;
use crate::storage::btree::tests::{adj_key, engine};
use crate::storage::btree::{Store, Tree, Write};
use crate::storage::engine::Engine;
use crate::test_support::test_dirs;

fn put_all(s: &Store<'_>, tree: Tree, keys: &[Vec<u8>], v: &[u8]) {
    for chunk in keys.chunks(200) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(v),
            })
            .collect();
        s.apply(&batch).unwrap();
    }
}

fn delete_all(s: &Store<'_>, tree: Tree, keys: &[Vec<u8>]) {
    for chunk in keys.chunks(200) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: None,
            })
            .collect();
        s.apply(&batch).unwrap();
    }
}

fn listed(s: &Store<'_>) -> (Vec<u64>, Vec<u64>) {
    let st = s.lock();
    let f = st.free_list();
    (f.pages().collect(), f.trunk_pages().collect())
}

fn reopen(e: Engine, dir: &std::path::Path) -> Engine {
    e.pool().write_dirty().unwrap();
    e.data().sync().unwrap();
    drop(e);
    Engine::open(
        dir,
        Arc::new(FsStorage),
        32 << 20,
        Arc::new(RecordingSink::default()),
    )
    .unwrap()
}

#[test]
fn registry_horizon_is_the_lowest_registered_value() {
    let r = Readers::default();
    let applied = AtomicU64::new(5);
    assert_eq!(r.horizon(), u64::MAX);
    let a = r.enter(&applied);
    applied.store(9, Ordering::SeqCst);
    let b = r.enter(&applied);
    assert_eq!(r.horizon(), 5);
    drop(a);
    assert_eq!(r.horizon(), 9);
    drop(b);
    assert_eq!(r.horizon(), u64::MAX);
    // More registrations than slots from many threads: all are released.
    std::thread::scope(|sc| {
        for _ in 0..8 {
            sc.spawn(|| {
                for _ in 0..10_000 {
                    let _g = r.enter(&applied);
                    assert!(r.horizon() <= 9);
                }
            });
        }
    });
    assert_eq!(r.horizon(), u64::MAX);
}

/// TS-05 Reuse: a page freed by batch `c` is not reused while a reader
/// that registered a value below `c` runs, and is reused once it ends.
#[test]
fn freed_pages_wait_for_older_readers() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 32 << 20);
        let s = e.store();
        let tree = s.create_tree().unwrap();
        let keys: Vec<Vec<u8>> = (0..3000u64).map(|i| adj_key(i, 0, 0, i, i)).collect();
        put_all(&s, tree, &keys, &[3u8; 60]);
        let reader = s.reader();
        // Empty the leaves holding keys 1000..2000.
        delete_all(&s, tree, &keys[1000..2000]);
        let (freed, _) = listed(&s);
        assert!(freed.len() >= 5, "{} pages freed", freed.len());
        {
            let st = s.lock();
            assert_eq!(st.free_list().reusable(s.horizon()), 0);
        }
        // Allocation now takes reserved pages, not the freed ones.
        let more: Vec<Vec<u8>> = (0..3000u64)
            .map(|i| adj_key(10_000 + i, 0, 0, i, i))
            .collect();
        let count0 = e.data().page_count();
        put_all(&s, tree, &more[..1500], &[4u8; 60]);
        assert!(e.data().page_count() > count0);
        assert_eq!(listed(&s).0, freed);
        drop(reader);
        {
            let st = s.lock();
            assert_eq!(st.free_list().reusable(s.horizon()), freed.len() as u64);
        }
        let count1 = e.data().page_count();
        // Fewer new leaves than freed pages: no reserved page is taken.
        put_all(&s, tree, &more[1500..1800], &[4u8; 60]);
        assert_eq!(
            e.data().page_count(),
            count1,
            "freed pages were not reused first"
        );
        assert!(listed(&s).0.len() < freed.len());
        // The rest exhausts the list, then takes reserved pages.
        put_all(&s, tree, &more[1800..], &[4u8; 60]);
        let (left, _) = listed(&s);
        assert!(left.is_empty() || e.data().page_count() == count1);
        assert!(s.check().unwrap().is_ok());
    }
}

/// The persisted list grows beyond one trunk, survives a reopen, and is
/// consumed again, trunks included.
#[test]
fn trunk_chain_grows_persists_and_is_consumed() {
    for dir in test_dirs() {
        let e = engine(dir.path(), 32 << 20);
        let s = e.store();
        let tree = s.create_tree().unwrap();
        let value: Vec<u8> = (0..100 * 8000).map(|i| (i % 253) as u8).collect();
        let keys: Vec<Vec<u8>> = (0..25u32).map(|i| i.to_be_bytes().to_vec()).collect();
        for k in &keys {
            s.apply(&[Write {
                tree,
                key: k,
                value: Some(&value),
            }])
            .unwrap();
        }
        let peak = e.data().page_count();
        delete_all(&s, tree, &keys);
        let (pages, trunks) = listed(&s);
        assert!(pages.len() > 2 * TRUNK_CAP, "{} pages", pages.len());
        // A trunk added mid-batch may be one of the pages just freed.
        let min = pages.len().div_ceil(TRUNK_CAP);
        assert!(
            (min..=min + 1).contains(&trunks.len()),
            "{} trunks",
            trunks.len()
        );
        let r = s.check().unwrap();
        assert!(r.is_ok(), "{:?}", r.problems);
        assert_eq!(
            (r.free_pages, r.trunk_pages),
            (pages.len() as u64, trunks.len() as u64)
        );
        let e = reopen(e, dir.path());
        let s = e.store();
        assert_eq!(listed(&s), (pages.clone(), trunks.clone()));
        {
            let pin = e.pool().fetch(0).unwrap();
            let h = crate::storage::datafile::Header::decode(&pin.read().unwrap()).unwrap();
            assert_eq!(h.free_list_head, trunks[0]);
        }
        let tree = s.tree(tree.id()).unwrap().unwrap();
        for k in &keys {
            s.apply(&[Write {
                tree,
                key: k,
                value: Some(&value),
            }])
            .unwrap();
        }
        assert!(
            e.data().page_count() <= peak + 2,
            "free pages were not reused"
        );
        let (left, trunks_left) = listed(&s);
        assert!(
            left.len() < 2 * trunks.len() + 10,
            "{} pages still listed",
            left.len()
        );
        assert!(trunks_left.len() <= 1);
        let r = s.check().unwrap();
        assert!(r.is_ok(), "{:?}", r.problems);
        let mut out = Vec::new();
        for k in &keys {
            assert!(s.get(tree, k, &mut out).unwrap());
            assert_eq!(out, value);
        }
    }
}

#[test]
fn a_cyclic_chain_fails_the_open() {
    let dir = &test_dirs()[0];
    let e = engine(dir.path(), 32 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let value = vec![1u8; 50_000];
    s.apply(&[Write {
        tree,
        key: b"k",
        value: Some(&value),
    }])
    .unwrap();
    s.apply(&[Write {
        tree,
        key: b"k",
        value: None,
    }])
    .unwrap();
    let (_, trunks) = listed(&s);
    {
        let pin = e.pool().fetch(trunks[0]).unwrap();
        let mut g = pin.write().unwrap();
        g[NEXT..NEXT + 8].copy_from_slice(&trunks[0].to_le_bytes());
    }
    e.pool().write_dirty().unwrap();
    e.data().sync().unwrap();
    drop(e);
    let r = Engine::open(
        dir.path(),
        Arc::new(FsStorage),
        32 << 20,
        Arc::new(RecordingSink::default()),
    );
    assert!(matches!(r, Err(Error::Corruption(_))), "{:?}", r.err());
}
