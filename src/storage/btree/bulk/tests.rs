use crate::sync::Arc;
use std::collections::BTreeMap;
use std::path::Path;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::*;
use crate::obs::recording::RecordingSink;
use crate::storage::btree::Write;
use crate::storage::fault::{Crash, FaultStorage};
use crate::storage::page::PageType;
use crate::storage::sort::Sorter;
use crate::storage::{FsStorage, Storage};
use crate::test_support::{seed, test_dirs};

const CAP: u64 = 32 << 20;

type Model = BTreeMap<Vec<u8>, Vec<u8>>;

/// Entries already in key order, from memory.
struct VecSource {
    v: Vec<(Vec<u8>, Vec<u8>)>,
    i: usize,
}

impl SortedEntries for VecSource {
    fn next_entry(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        let e = self
            .v
            .get(self.i)
            .map(|(k, v)| (k.as_slice(), v.as_slice()));
        self.i += 1;
        Ok(e)
    }
}

fn source(m: &Model) -> VecSource {
    VecSource {
        v: m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        i: 0,
    }
}

/// `n` random entries: keys of 4 to 60 bytes, values of 0 to 120 bytes,
/// and every 997th value 5 000 bytes, so overflow chains are built too.
fn model(n: usize, seed: u64) -> Model {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut m = Model::new();
    while m.len() < n {
        let mut k = vec![0u8; rng.random_range(4..=60)];
        rng.fill(&mut k[..]);
        let len = if m.len().is_multiple_of(997) {
            5_000
        } else {
            rng.random_range(0..=120)
        };
        m.insert(k, vec![rng.random::<u8>(); len]);
    }
    m
}

fn open(dir: &Path, storage: Arc<dyn Storage>) -> Engine {
    Engine::open(dir, storage, CAP, Arc::new(RecordingSink::default())).unwrap()
}

/// Every entry of `tree`, by a full scan.
fn entries(e: &Engine, tree: Tree) -> Model {
    let mut out = Model::new();
    e.store()
        .scan_prefix(tree, b"", |k, v| {
            out.insert(k.to_vec(), v.to_vec());
            true
        })
        .unwrap();
    out
}

/// `a` and `b` answer the same gets and prefix scans.
fn assert_same_answers(e: &Engine, a: Tree, b: Tree, m: &Model) {
    let s = e.store();
    let (mut va, mut vb) = (Vec::new(), Vec::new());
    for (k, v) in m {
        assert!(s.get(a, k, &mut va).unwrap() && s.get(b, k, &mut vb).unwrap());
        assert!(va == *v && vb == *v, "get differs");
        let mut miss = k.clone();
        miss.push(0);
        if !m.contains_key(&miss) {
            assert!(!s.get(a, &miss, &mut va).unwrap() && !s.get(b, &miss, &mut vb).unwrap());
        }
    }
    for prefix in [&[][..], &[0x00], &[0x7f], &[0xff], &[0x10, 0x20]] {
        let scan = |t: Tree| {
            let mut out = Vec::new();
            s.scan_prefix(t, prefix, |k, v| {
                out.push((k.to_vec(), v.to_vec()));
                true
            })
            .unwrap();
            out
        };
        assert!(scan(a) == scan(b), "prefix scan {prefix:?} differs");
    }
}

/// rmp #17: a tree built bottom-up from a sorted stream answers every get
/// and prefix scan exactly like a tree built by puts from the same entries,
/// passes the integrity check, and reads back the same after a reopen.
#[test]
fn a_bulk_built_tree_answers_like_a_tree_built_by_puts() {
    let dir = test_dirs().remove(0);
    let storage = crate::test_support::no_flush();
    let e = Engine::create(
        dir.path(),
        storage.clone(),
        CAP,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let m = model(6_000, seed());
    let by_put = e.create_trees(1).unwrap()[0];
    let shuffled: Vec<(&Vec<u8>, &Vec<u8>)> = {
        let mut v: Vec<_> = m.iter().collect();
        let mut rng = StdRng::seed_from_u64(1);
        for i in (1..v.len()).rev() {
            v.swap(i, rng.random_range(0..=i));
        }
        v
    };
    for chunk in shuffled.chunks(500) {
        let batch: Vec<Write<'_>> = chunk
            .iter()
            .map(|(k, v)| Write {
                tree: by_put,
                key: k,
                value: Some(v),
                stamp: 0,
            })
            .collect();
        e.commit(&batch, Ids::default()).unwrap();
    }
    let built = e.bulk_build(&mut source(&m)).unwrap();
    assert_eq!(built.id(), by_put.id() + 1);
    assert_same_answers(&e, by_put, built, &m);
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    assert!(r.trees[2].height >= 2 && r.trees[2].entries == m.len() as u64);
    e.close().unwrap();
    let e = open(dir.path(), storage);
    assert!(entries(&e, built) == m);
    assert_same_answers(&e, by_put, built, &m);
}

/// The leaves of `tree` from left to right: their high-key length, used
/// body bytes and number of entries.
fn leaves(e: &Engine, tree: Tree) -> Vec<(usize, usize, usize)> {
    let pool = e.pool();
    let mut p = tree.root();
    loop {
        let pin = pool.fetch(p).unwrap();
        let g = pin.read().unwrap();
        if page::page_type(&g) == Some(PageType::Leaf) {
            break;
        }
        p = node::leftmost(&g);
    }
    let mut out = Vec::new();
    while p != NONE {
        let pin = pool.fetch(p).unwrap();
        let g = pin.read().unwrap();
        let v = node::leaf_view(&g).unwrap();
        out.push((
            node::high_key(&g).map_or(0, <[u8]>::len),
            v.used_bytes(),
            v.count(),
        ));
        p = node::right(&g);
    }
    out
}

/// rmp #17: every leaf except the last is filled to at most 90 % of its
/// usable bytes, and to more than 90 % minus one entry of the largest
/// size; the bulk-built tree is denser than the tree of the same entries
/// built by puts.
#[test]
fn bulk_built_leaves_are_filled_to_ninety_percent() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(
        dir.path(),
        crate::test_support::no_flush(),
        CAP,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let mut m = model(8_000, seed());
    m.retain(|_, v| v.len() <= MAX_INLINE);
    let tree = e.bulk_build(&mut source(&m)).unwrap();
    let ls = leaves(&e, tree);
    assert!(ls.len() > 10);
    assert_eq!(ls.iter().map(|l| l.2).sum::<usize>(), m.len());
    let max_key = m.keys().map(Vec::len).max().unwrap();
    let max_value = m.values().map(Vec::len).max().unwrap();
    // The largest entry: two varints of key lengths, one of the value
    // length, the key, the value and a restart slot; the high key it may
    // bring is at most one key long.
    let largest = 3 + 3 + 3 + max_key + max_value + 2 + max_key;
    // TS-06: 90 % of the bytes a leaf can use, computed here rather than
    // taken from the code under test.
    let ninety = node::USABLE * 9 / 10;
    for (i, &(hk, used, _)) in ls[..ls.len() - 1].iter().enumerate() {
        let fill = hk + used;
        assert!(fill <= ninety, "leaf {i}: {fill} > {ninety}");
        assert!(fill + largest > ninety, "leaf {i}: {fill} too empty");
    }
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
}

/// rmp #17: a crash after the pages are written and synced but before the
/// install commit leaves no tree, and the pages are in the reserve again;
/// the integrity check passes. After a build that was installed, the tree
/// survives the same crash.
#[test]
fn a_crash_before_the_install_leaves_the_tree_absent_and_its_pages_free() {
    let dir = test_dirs().remove(0);
    let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    fault.skip_device_sync();
    let e = Engine::create(
        dir.path(),
        fault.clone(),
        CAP,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let small = e.create_trees(1).unwrap()[0];
    e.commit(
        &[Write {
            tree: small,
            key: b"a",
            value: Some(b"1"),
            stamp: 0,
        }],
        Ids::default(),
    )
    .unwrap();
    let pages = e.data().page_count();
    let m = model(3_000, seed());
    let built = e.bulk_build_pages(&mut source(&m)).unwrap();
    assert!(built.pages > 10 && built.entries == m.len() as u64);
    drop(built);
    assert_eq!(
        e.data().page_count(),
        pages,
        "the pages are back in the reserve"
    );
    drop(e);
    fault.crash(Crash::Tear { seed: seed() }).unwrap();
    let e = open(dir.path(), fault.clone());
    assert_eq!(e.data().page_count(), pages);
    assert!(e.store().tree(small.id() + 1).unwrap().is_none());
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    // The same build, installed, survives a crash.
    let tree = e.bulk_build(&mut source(&m)).unwrap();
    drop(e);
    fault.crash(Crash::DropAll).unwrap();
    let e = open(dir.path(), fault);
    assert!(entries(&e, tree) == m);
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
}

/// Keys out of order fail the build before the install, give the pages
/// back and leave the database usable; an empty input builds an empty
/// tree.
#[test]
fn unsorted_input_fails_the_build_and_empty_input_builds_an_empty_tree() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(
        dir.path(),
        crate::test_support::no_flush(),
        CAP,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let pages = e.data().page_count();
    let mut bad = VecSource {
        v: (0..5_000u32)
            .map(|i| ((i ^ 7).to_be_bytes().to_vec(), vec![1; 50]))
            .collect(),
        i: 0,
    };
    let r = e.bulk_build(&mut bad);
    assert!(matches!(r, Err(Error::InvalidArgument(_))), "{r:?}");
    assert_eq!(e.data().page_count(), pages);
    let empty = e
        .bulk_build(&mut VecSource {
            v: Vec::new(),
            i: 0,
        })
        .unwrap();
    assert!(!e.store().get(empty, b"x", &mut Vec::new()).unwrap());
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
}

/// The sort feeds the build: unsorted input of 10 times the sort's memory
/// becomes a tree equal to the model.
#[test]
fn a_sorted_spill_feeds_the_build() {
    let dir = test_dirs().remove(0);
    let e = Engine::create(
        dir.path(),
        crate::test_support::no_flush(),
        CAP,
        Arc::new(RecordingSink::default()),
    )
    .unwrap();
    let mut m = model(10_000, seed());
    m.retain(|_, v| v.len() <= 120);
    let tmp = tempfile::tempdir_in(dir.path()).unwrap();
    let storage = FsStorage;
    let mut sorter = Sorter::new(&storage, tmp.path(), "bulk", e.pool(), 64 << 10).unwrap();
    for (k, v) in m.iter().rev() {
        sorter.push(k, v).unwrap();
    }
    assert!(sorter.runs() > 5);
    let mut sorted = sorter.finish().unwrap();
    let tree = e.bulk_build(&mut sorted).unwrap();
    drop(sorted);
    assert!(entries(&e, tree) == m);
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
}

/// Audit finding 2 (2026-10-03; TS-16 step 6 undo, item 3). Commit c frees
/// pages and is not durable yet; batch d takes them and pages of the
/// reserve, overruns the reserve and is undone. Every page d took keeps
/// d's image in a dirty frame whose page LSN is c's, so once c is durable
/// eviction may write it. A bulk build then takes those reserve pages and
/// writes each one directly. An eviction of the stale frame between that
/// write and the frame refresh (the pause point `after_direct_write`)
/// must not reach the data file: the build writes under the frame's
/// exclusive latch ([`crate::storage::pool::Pool::write_new_page`]).
/// Otherwise the stale image overwrites a built page, and the reopened
/// database fails its integrity check or answers differently.
#[test]
fn audit_a_stale_frame_never_overwrites_a_bulk_built_page() {
    use crate::sync::atomic::Ordering;
    use std::collections::BTreeSet;

    use crate::tx::commit::tests::{audit_join, fault, put, use_up_reserve};
    let dir = test_dirs().remove(0);
    let d = dir.path();
    let fault = fault();
    let e = Arc::new(
        Engine::create(d, fault.clone(), CAP, Arc::new(RecordingSink::default())).unwrap(),
    );
    let tree = e.create_trees(1).unwrap()[0];
    let mut puts = Model::new();
    let medium = vec![5u8; 100_000];
    e.commit(&[put(tree, b"big", &medium)], Ids::default())
        .unwrap();
    use_up_reserve(&e, tree, &mut puts, 600);
    // c: frees the pages of "big", joined but not durable.
    let seq = {
        let store = e.store();
        let mut st = store.lock();
        store
            .apply_batch(
                &mut st,
                Some(e.wal()),
                &[Write {
                    tree,
                    key: b"big",
                    value: None,
                    stamp: 0,
                }],
                0,
            )
            .unwrap();
        audit_join(&mut st)
    };
    // d: "d1" takes c's pages and reserve pages (its chain is allocated
    // whole before it is written), then "d2" overruns the reserve and meets
    // ENOSPC.
    let reserve: BTreeSet<u64> = (e.data().page_count()..e.data().reserved_pages()).collect();
    fault.set_space(0);
    e.core().skip_reserve.store(true, Ordering::Relaxed);
    let d1 = vec![8u8; 2 << 20];
    let huge = vec![9u8; 8 << 20];
    let r = e.commit(
        &[put(tree, b"d1", &d1), put(tree, b"d2", &huge)],
        Ids::default(),
    );
    assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
    e.core().skip_reserve.store(false, Ordering::Relaxed);
    fault.set_space(u64::MAX);
    e.wal().wait_durable(seq).unwrap();
    let stale: Vec<u64> = reserve
        .iter()
        .copied()
        .filter(|&p| e.pool().resident(p).is_some_and(|(_, dirty)| dirty))
        .collect();
    assert!(
        !stale.is_empty(),
        "precondition: reserve pages with a dirty frame of d's image"
    );
    // Each direct write of the build is followed by an eviction attempt of
    // every stale frame.
    {
        let e2 = Arc::clone(&e);
        let stale = stale.clone();
        *e.pool().after_direct_write.lock().unwrap() = Some(Box::new(move || {
            for &p in &stale {
                e2.pool().evict_write(p).unwrap();
            }
        }));
    }
    let m = model(3_000, 7);
    let built = e.bulk_build(&mut source(&m));
    // The hook holds a clone of the engine: remove it before the close.
    *e.pool().after_direct_write.lock().unwrap() = None;
    let built = built.unwrap();
    let e = Arc::into_inner(e).expect("no other handle");
    e.close().unwrap();
    let e = open(d, fault);
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "{:?}", r.problems);
    assert!(
        entries(&e, built) == m,
        "the built tree differs after a reopen"
    );
    assert!(
        entries(&e, tree) == puts,
        "the first tree differs after a reopen"
    );
}
