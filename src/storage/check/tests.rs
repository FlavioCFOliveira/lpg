use std::path::Path;

use super::*;
use crate::storage::btree::node::{Encoder, Entries};
use crate::storage::btree::tests::{adj_key, engine};
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::{Engine, data_name};
use crate::storage::page::PAGE_BYTES;
use crate::storage::{FsStorage, Storage};
use crate::test_support::test_dirs;

/// A database with a tree of height 3 and its leaves in key order.
fn built(dir: &Path) -> (Engine, Tree, Vec<u64>) {
    let e = engine(dir, 32 << 20);
    let s = e.store();
    let tree = s.create_tree().unwrap();
    let keys: Vec<Vec<u8>> = (0..40_000u64)
        .map(|i| adj_key(i / 3, 0, 1, i * 7919 % 10_007, i))
        .collect();
    let values = [0x5Au8; 200];
    for chunk in keys.chunks(1000) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(&values),
            })
            .collect();
        s.apply(&batch).unwrap();
    }
    let r = s.check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    let stats = r.trees.iter().find(|t| t.id == tree.id()).unwrap();
    assert!(stats.height >= 3, "height {}", stats.height);
    assert_eq!(stats.entries, 40_000);
    let l = leaves(&e, tree);
    (e, tree, l)
}

fn leaves(e: &Engine, tree: Tree) -> Vec<u64> {
    let mut p = tree.root();
    loop {
        let pin = e.pool().fetch(p).unwrap();
        let g = pin.read().unwrap();
        if node::level(&g) == 0 {
            break;
        }
        p = node::leftmost(&g);
    }
    let mut out = Vec::new();
    while p != NONE {
        out.push(p);
        let pin = e.pool().fetch(p).unwrap();
        p = node::right(&pin.read().unwrap());
    }
    out
}

fn has(r: &Report, needle: &str) -> bool {
    r.problems.iter().any(|p| p.contains(needle))
}

#[test]
fn a_clean_database_passes_with_its_figures() {
    for dir in test_dirs() {
        let (e, tree, leaves) = built(dir.path());
        let r = e.store().check().unwrap();
        assert!(r.is_ok());
        assert_eq!(r.trees.len(), 2);
        let t = &r.trees[1];
        assert_eq!(t.id, tree.id());
        assert_eq!(t.leaves, leaves.len() as u64);
        assert_eq!(
            1 + r
                .trees
                .iter()
                .map(|t| t.leaves + t.inner + t.overflow)
                .sum::<u64>(),
            r.page_count
        );
    }
}

/// AC: a flipped bit on disk is reported, and makes the database unusable.
#[test]
fn a_flipped_bit_is_reported() {
    for dir in test_dirs() {
        let (e, _, leaves) = built(dir.path());
        e.pool().write_dirty().unwrap();
        e.data().sync().unwrap();
        let victim = leaves[leaves.len() / 2];
        let file = FsStorage.open(&dir.path().join(data_name(1))).unwrap();
        let at = victim * PAGE_BYTES + 3000;
        let mut b = [0u8; 1];
        file.read_at(&mut b, at).unwrap();
        b[0] ^= 0x10;
        file.write_at(&b, at).unwrap();
        let r = e.store().check().unwrap();
        assert!(
            has(&r, &format!("page {victim} fails its checksum")),
            "{:?}",
            r.problems
        );
        assert!(matches!(e.check(), Err(Error::Unusable)));
    }
}

/// AC: a pair of swapped keys is reported.
#[test]
fn swapped_keys_are_reported() {
    for dir in test_dirs() {
        let (e, tree, leaves) = built(dir.path());
        let victim = leaves[leaves.len() / 3];
        {
            let pin = e.pool().fetch(victim).unwrap();
            let mut g = pin.write().unwrap();
            let mut es = Entries::default();
            node::leaf_decode(&g, 0, &mut es).unwrap();
            let mut swapped = Entries::default();
            for i in 0..es.len() {
                let j = match i {
                    5 => 6,
                    6 => 5,
                    _ => i,
                };
                swapped.push(es.key(j), es.value(j));
            }
            let (hk, right) = (node::high_key(&g).map(<[u8]>::to_vec), node::right(&g));
            let at = node::Place {
                tree: tree.id(),
                level: 0,
                high_key: hk.as_deref(),
                right,
            };
            assert!(node::write_leaf(
                &mut g,
                at,
                &swapped,
                0..es.len(),
                &mut Encoder::default()
            ));
        }
        let r = e.store().check().unwrap();
        assert!(
            has(&r, &format!("page {victim}: keys 5 and 6 out of order")),
            "{:?}",
            r.problems
        );
    }
}

/// AC: a broken sibling link is reported.
#[test]
fn a_broken_sibling_link_is_reported() {
    for dir in test_dirs() {
        let (e, _, leaves) = built(dir.path());
        let victim = leaves[10];
        {
            let pin = e.pool().fetch(victim).unwrap();
            node::set_right(&mut pin.write().unwrap(), leaves[12]);
        }
        let r = e.store().check().unwrap();
        assert!(
            has(
                &r,
                &format!(
                    "page {victim} links right to {}, but the next page of level 0 is {}",
                    leaves[12], leaves[11]
                )
            ),
            "{:?}",
            r.problems
        );
    }
}

/// AC: a page that is both in a tree and in the free list is reported.
#[test]
fn a_page_in_a_tree_and_the_free_list_is_reported() {
    for dir in test_dirs() {
        let (e, tree, leaves) = built(dir.path());
        let s = e.store();
        // Free an overflow chain, so the free list has a trunk.
        let big = vec![9u8; 30_000];
        s.apply(&[Write {
            tree,
            key: b"big",
            value: Some(&big),
        }])
        .unwrap();
        s.apply(&[Write {
            tree,
            key: b"big",
            value: None,
        }])
        .unwrap();
        let trunk = s.lock().free_list().head();
        assert_ne!(trunk, 0);
        assert!(s.check().unwrap().is_ok());
        let victim = leaves[7];
        {
            let pin = e.pool().fetch(trunk).unwrap();
            let mut g = pin.write().unwrap();
            // First listed entry (offset 40) becomes a tree leaf.
            g[40..48].copy_from_slice(&victim.to_le_bytes());
        }
        let r = s.check().unwrap();
        assert!(
            has(&r, &format!("page {victim} is in two places")),
            "{:?}",
            r.problems
        );
    }
}

#[test]
fn a_leaked_page_and_a_stale_header_are_reported() {
    let dir = &test_dirs()[0];
    let (e, _, _) = built(dir.path());
    let leaked = e.data().allocate().unwrap();
    let r = e.store().check().unwrap();
    assert!(
        has(&r, &format!("page {leaked} is in no tree and not free")),
        "{:?}",
        r.problems
    );
    assert!(has(&r, "page 0 holds"), "{:?}", r.problems);
}

/// A leaf whose high key differs from its parent's separator is reported
/// (amcheck: "mismatch between parent key and child high key").
#[test]
fn a_high_key_that_differs_from_the_parent_is_reported() {
    let dir = &test_dirs()[0];
    let (e, tree, leaves) = built(dir.path());
    let victim = leaves[20];
    {
        let pin = e.pool().fetch(victim).unwrap();
        let mut g = pin.write().unwrap();
        let mut es = Entries::default();
        node::leaf_decode(&g, 0, &mut es).unwrap();
        let mut hk = node::high_key(&g).unwrap().to_vec();
        hk.push(0);
        let at = node::Place {
            tree: tree.id(),
            level: 0,
            high_key: Some(&hk),
            right: node::right(&g),
        };
        assert!(node::write_leaf(
            &mut g,
            at,
            &es,
            0..es.len(),
            &mut Encoder::default()
        ));
    }
    let r = e.store().check().unwrap();
    assert!(
        has(&r, &format!("page {victim}: high key differs")),
        "{:?}",
        r.problems
    );
}
