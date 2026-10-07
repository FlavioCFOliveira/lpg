use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::*;
use crate::obs::names::cache::ACCOUNTED_BYTES;
use crate::obs::recording::RecordingSink;
use crate::storage::engine::{Engine, data_name};
use crate::storage::fault::{Call, FaultStorage};
use crate::storage::page::{HEADER_LEN, PageType};
use crate::storage::{FsStorage, Storage};
use crate::test_support::test_dirs;

/// Writes `n` sealed pages after page 0, each tagged with its number, and
/// syncs them. Returns their page numbers.
pub(crate) fn write_pages(data: &Datafile, n: u64) -> std::ops::Range<u64> {
    data.ensure(n).unwrap();
    let first = data.allocate().unwrap();
    for _ in 1..n {
        data.allocate().unwrap();
    }
    let mut buf = vec![0u8; PAGE_SIZE];
    for p in first..first + n {
        tagged(&mut buf, p);
        data.write_page(p, &buf).unwrap();
    }
    data.sync().unwrap();
    first..first + n
}

/// The sealed image of test page `p`.
fn tagged(buf: &mut [u8], p: u64) {
    page::init(buf, PageType::Leaf, p);
    for at in (HEADER_LEN..PAGE_SIZE - 8).step_by(776) {
        buf[at..at + 8].copy_from_slice(&p.to_le_bytes());
    }
    page::seal(buf, p);
}

/// Checks that `buf` is the intact image of test page `p`.
fn check(buf: &[u8], p: u64) {
    let mut expected = vec![0u8; PAGE_SIZE];
    tagged(&mut expected, p);
    assert!(buf == expected.as_slice(), "page {p} is not intact");
}

fn engine(dir: &Path, storage: Arc<dyn Storage>, cap: u64) -> (Engine, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    let e = Engine::create(dir, storage, cap, sink.clone()).unwrap();
    (e, sink)
}

#[test]
fn hits_and_misses_are_counted_and_published() {
    for dir in test_dirs() {
        let (e, sink) = engine(dir.path(), crate::test_support::no_flush(), 16 << 20);
        let pages = write_pages(e.data(), 100);
        for _ in 0..2 {
            for p in pages.clone() {
                check(&e.pool().fetch(p).unwrap().read().unwrap(), p);
            }
        }
        e.pool().publish_metrics();
        let m = sink.snapshot();
        assert_eq!(m.counter(PAGE_FAULTS), 100);
        assert_eq!(m.counter(HITS), 100);
        assert_eq!(m.counter(EVICTIONS), 0);
        assert_eq!(m.gauge(ACCOUNTED_BYTES), Some(e.accountant().used()));
        e.pool().publish_metrics();
        assert_eq!(sink.snapshot().since(&m).counter(HITS), 0, "deltas only");
    }
}

/// FR-STO-001: data ten times larger than the memory cap is served intact,
/// with accounted bytes within the cap.
#[test]
fn pages_worth_ten_times_the_cap_are_returned_intact() {
    const CAP: u64 = 2 << 20;
    for dir in test_dirs() {
        let (e, sink) = engine(dir.path(), crate::test_support::no_flush(), CAP);
        let n = 10 * CAP / PAGE_BYTES;
        let pages = write_pages(e.data(), n);
        let mut rng = StdRng::seed_from_u64(5);
        for round in 0..3 {
            for i in pages.clone() {
                let p = if round == 1 {
                    rng.random_range(pages.clone())
                } else {
                    i
                };
                check(&e.pool().fetch(p).unwrap().read().unwrap(), p);
                assert!(e.accountant().used() <= CAP);
            }
        }
        assert!(e.accountant().peak() <= CAP);
        e.pool().publish_metrics();
        let m = sink.snapshot();
        assert!(m.counter(EVICTIONS) > 2 * n, "{m}");
        assert_eq!(m.counter(HITS) + m.counter(PAGE_FAULTS), 3 * n);
    }
}

/// TS-08: 16 threads fetching pages and reserving memory never push the
/// accounted bytes above the cap; every successful reservation checks it.
#[test]
fn sixteen_threads_fetching_and_reserving_stay_within_the_cap() {
    const CAP: u64 = 4 << 20;
    let dir = test_dirs().remove(0);
    let (e, _) = engine(dir.path(), crate::test_support::no_flush(), CAP);
    let pages = write_pages(e.data(), 3 * CAP / PAGE_BYTES);
    let ok = AtomicU64::new(0);
    let refused = AtomicU64::new(0);
    std::thread::scope(|s| {
        for t in 0..16u64 {
            let (e, pages, ok, refused) = (&e, pages.clone(), &ok, &refused);
            s.spawn(move || {
                let mut rng = StdRng::seed_from_u64(t);
                let mut held = Vec::new();
                for i in 0..3000 {
                    let p = rng.random_range(pages.clone());
                    match e.pool().fetch(p) {
                        Ok(page) => check(&page.read().unwrap(), p),
                        // Other threads' reservations can leave the pool at
                        // its floor with every frame pinned.
                        Err(Error::MemoryLimit { .. }) => {
                            refused.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(other) => panic!("{other}"),
                    }
                    if i % 10 == 0 {
                        match e.pool().reserve(rng.random_range(1..=256 << 10)) {
                            Ok(r) => {
                                assert!(e.accountant().used() <= CAP);
                                ok.fetch_add(1, Ordering::Relaxed);
                                held.push(r);
                            }
                            Err(Error::MemoryLimit { .. }) => {
                                refused.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(other) => panic!("{other}"),
                        }
                        if held.len() > 3 {
                            drop(held.remove(0));
                        }
                    }
                    assert!(e.accountant().used() <= CAP);
                }
            });
        }
    });
    assert!(
        e.accountant().peak() <= CAP,
        "peak {}",
        e.accountant().peak()
    );
    assert!(ok.load(Ordering::Relaxed) > 0);
    assert!(e.pool().buffer_frames() >= e.pool().floor_frames());
    eprintln!(
        "reservations: {} granted, {} refused; peak {} of {CAP}",
        ok.load(Ordering::Relaxed),
        refused.load(Ordering::Relaxed),
        e.accountant().peak()
    );
}

/// FR-REC-005: a flipped bit fails the read; every later operation fails
/// until the database is reopened.
#[test]
fn flipped_bit_fails_the_read_and_every_later_operation_until_reopen() {
    for dir in test_dirs() {
        let d = dir.path();
        let (e, _) = engine(d, crate::test_support::no_flush(), 16 << 20);
        let pages = write_pages(e.data(), 10);
        let (good, bad) = (pages.start, pages.start + 5);
        check(&e.pool().fetch(good).unwrap().read().unwrap(), good);
        // Flip one bit of `bad` on disk; it is not in the pool.
        let file = FsStorage.open(&d.join(data_name(1))).unwrap();
        let at = bad * PAGE_BYTES + 4000;
        let mut b = [0u8];
        file.read_at(&mut b, at).unwrap();
        file.write_at(&[b[0] ^ 0x04], at).unwrap();
        file.sync_data().unwrap();

        let err = e.pool().fetch(bad).err().unwrap();
        assert!(matches!(err, Error::Corruption(_)), "{err}");
        assert!(matches!(e.pool().fetch(good), Err(Error::Unusable)));
        assert!(matches!(e.pool().fetch(bad), Err(Error::Unusable)));
        assert!(matches!(e.pool().reserve(1), Err(Error::Unusable)));
        assert!(matches!(e.data().ensure(1), Err(Error::Unusable)));
        assert!(matches!(e.check(), Err(Error::Unusable)));
        drop(e);

        let e = Engine::open(
            d,
            crate::test_support::no_flush(),
            16 << 20,
            Arc::new(RecordingSink::default()),
        )
        .unwrap();
        check(&e.pool().fetch(good).unwrap().read().unwrap(), good);
        assert!(matches!(e.pool().fetch(bad), Err(Error::Corruption(_))));
    }
}

/// A hit on a page that is still being read waits for the read, then fails
/// with the latch the failed read set.
#[test]
fn waiter_on_a_failed_load_fails() {
    let dir = test_dirs().remove(0);
    let (e, _) = engine(dir.path(), crate::test_support::no_flush(), 16 << 20);
    let pages = write_pages(e.data(), 2);
    // Page `pages.end` is reserved but never written: its read fails.
    let unwritten = pages.end;
    let first = e.pool().fetch(unwritten);
    assert!(matches!(first, Err(Error::Corruption(_))));
    assert!(matches!(e.pool().fetch(pages.start), Err(Error::Unusable)));
}

/// TS-08, TS-16: a frame whose page LSN is above the durable LSN is never
/// written, however much eviction pressure there is; once its LSN is
/// durable, eviction writes it, and the image on disk is valid.
#[test]
fn frame_with_lsn_above_durable_is_never_written() {
    const CAP: u64 = 1 << 20;
    for dir in test_dirs() {
        let d = dir.path();
        let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
        let (e, _) = engine(d, fault.clone(), CAP);
        let pages = write_pages(e.data(), 4 * CAP / PAGE_BYTES);
        let target = pages.start + 3;
        let target_offset = target * PAGE_BYTES;
        e.durable_lsn().store(50, Ordering::Release);
        {
            let pinned = e.pool().fetch(target).unwrap();
            let mut w = pinned.write().unwrap();
            page::set_lsn(&mut w, 100);
            w[HEADER_LEN] = 0xAB;
        }
        let writes_to_target = || {
            fault
                .calls()
                .iter()
                .filter(|c| matches!(c, Call::WriteAt { offset, .. } if *offset == target_offset))
                .count()
        };
        let before = writes_to_target();
        for _ in 0..3 {
            for p in pages.clone() {
                if p != target {
                    drop(e.pool().fetch(p).unwrap().read().unwrap());
                }
            }
        }
        assert_eq!(
            writes_to_target(),
            before,
            "a non-durable page reached disk"
        );
        {
            let pinned = e.pool().fetch(target).unwrap();
            let r = pinned.read().unwrap();
            assert_eq!((page::lsn(&r), r[HEADER_LEN]), (100, 0xAB), "frame kept");
        }
        // Once the WAL is durable up to its LSN, eviction writes it.
        e.durable_lsn().store(100, Ordering::Release);
        for p in pages.clone() {
            if p != target {
                drop(e.pool().fetch(p).unwrap().read().unwrap());
            }
        }
        assert_eq!(writes_to_target(), before + 1);
        let mut buf = vec![0u8; PAGE_SIZE];
        e.data().read_page(target, &mut buf).unwrap();
        assert_eq!((page::lsn(&buf), buf[HEADER_LEN]), (100, 0xAB));
        assert!(e.check().is_ok());
    }
}

/// Pinned frames are never evicted; with every frame pinned a miss fails
/// with a memory-limit error and leaves the database usable.
#[test]
fn pinned_frames_stay_and_all_pinned_is_a_memory_limit() {
    const CAP: u64 = 1 << 20;
    let dir = test_dirs().remove(0);
    let (e, _) = engine(dir.path(), crate::test_support::no_flush(), CAP);
    let pages = write_pages(e.data(), 2 * CAP / PAGE_BYTES);
    let mut pinned = Vec::new();
    let mut next = pages.clone();
    let err = loop {
        let p = next.next().unwrap();
        match e.pool().fetch(p) {
            Ok(page) => pinned.push(page),
            Err(err) => break err,
        }
    };
    assert!(matches!(err, Error::MemoryLimit { .. }), "{err}");
    for (i, page) in pinned.iter().enumerate() {
        check(&page.read().unwrap(), pages.start + i as u64);
    }
    e.check().unwrap();
    drop(pinned);
    let p = pages.end - 1;
    check(&e.pool().fetch(p).unwrap().read().unwrap(), p);
}

/// TS-08: a reservation by another user makes the pool give back clean
/// frames, never below its floor, and the pool keeps serving pages.
#[test]
fn reserve_shrinks_the_pool_down_to_its_floor() {
    const CAP: u64 = 8 << 20;
    let dir = test_dirs().remove(0);
    let (e, _) = engine(dir.path(), crate::test_support::no_flush(), CAP);
    let pages = write_pages(e.data(), 2 * CAP / PAGE_BYTES);
    for p in pages.clone() {
        e.pool().fetch(p).unwrap();
    }
    let full = e.pool().buffer_frames();
    assert!(full > CAP / PAGE_BYTES * 9 / 10, "pool filled to {full}");
    let half = e.pool().reserve(CAP / 2).unwrap();
    assert!(e.pool().buffer_frames() < full);
    let mut held = vec![half];
    while let Ok(r) = e.pool().reserve(256 << 10) {
        held.push(r);
    }
    assert_eq!(e.pool().buffer_frames(), e.pool().floor_frames());
    assert!(e.accountant().used() <= CAP);
    for p in pages.clone().step_by(7) {
        check(&e.pool().fetch(p).unwrap().read().unwrap(), p);
    }
    drop(held);
    // Freed memory lets the pool grow again.
    for p in pages.clone() {
        e.pool().fetch(p).unwrap();
    }
    assert!(e.pool().buffer_frames() > e.pool().floor_frames());
}

/// A dirty frame whose LSN is durable is written before it is evicted by a
/// reservation, and clean frames go first.
#[test]
fn reserve_takes_clean_frames_before_dirty_ones() {
    const CAP: u64 = 1 << 20;
    let dir = test_dirs().remove(0);
    let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    let (e, _) = engine(dir.path(), fault.clone(), CAP);
    let pages = write_pages(e.data(), CAP / PAGE_BYTES / 2);
    for p in pages.clone() {
        e.pool().fetch(p).unwrap();
    }
    let dirty = pages.start;
    drop(e.pool().fetch(dirty).unwrap().write().unwrap());
    let writes = || {
        fault
            .calls()
            .iter()
            .filter(|c| matches!(c, Call::WriteAt { .. }))
            .count()
    };
    let before = writes();
    // Free a few clean frames' worth: no write needed.
    let spare = CAP - e.accountant().used();
    let r = e.pool().reserve(spare + 4 * PAGE_BYTES).unwrap();
    assert_eq!(writes(), before);
    drop(r);
    // With every frame dirty (LSN 0, durable), freeing four frames writes
    // four pages.
    for p in pages.clone() {
        drop(e.pool().fetch(p).unwrap().write().unwrap());
    }
    let before = writes();
    let spare = CAP - e.accountant().used();
    let held = e.pool().reserve(spare + 4 * PAGE_BYTES).unwrap();
    assert_eq!(writes(), before + 4);
    drop(held);
    let mut held = Vec::new();
    while let Ok(r) = e.pool().reserve(PAGE_BYTES) {
        held.push(r);
    }
    assert_eq!(e.pool().buffer_frames(), e.pool().floor_frames());
}

/// Task #15: a page copy's reservation takes the clock's first victim,
/// dirty or clean, as a miss does, and never runs the clean-only sweep,
/// which visited and aged the whole frame table on a pool of dirty frames
/// (80.6 % of put time at 10 % cache, laboratory report #15). Here the one
/// clean frame has the highest usage count: the clean-only sweep would age
/// every frame until it could take it, writing nothing; the miss-path
/// sweep takes a dirty frame at usage 0 first and writes it.
#[test]
fn a_page_copy_reservation_takes_the_first_victim_dirty_or_clean() {
    const CAP: u64 = 1 << 20;
    let dir = test_dirs().remove(0);
    let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    let (e, _) = engine(dir.path(), fault.clone(), CAP);
    let pages = write_pages(e.data(), CAP / PAGE_BYTES / 2);
    for p in pages.clone() {
        e.pool().fetch(p).unwrap();
    }
    let clean = pages.start;
    for p in pages.clone().skip(1) {
        drop(e.pool().fetch(p).unwrap().write().unwrap());
    }
    for _ in 0..MAX_USAGE {
        e.pool().fetch(clean).unwrap();
    }
    let writes = || {
        fault
            .calls()
            .iter()
            .filter(|c| matches!(c, Call::WriteAt { .. }))
            .count()
    };
    let spare = CAP - e.accountant().used();
    assert!(e.accountant().try_charge(spare));
    let before = writes();
    let copy = e.pool().reserve_page_copy().unwrap();
    assert_eq!(writes(), before + 1, "a dirty victim was written");
    assert!(e.pool().resident(clean).is_some(), "the clean frame stays");
    drop(copy);
    e.accountant().release(spare);
}
