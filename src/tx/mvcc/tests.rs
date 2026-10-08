#[cfg(any(loom, not(shuttle)))]
use super::*;

#[cfg(not(any(loom, shuttle)))]
#[test]
fn a_registered_snapshot_bounds_the_horizon_until_it_ends() {
    let reg = Readers::default();
    let published = AtomicU64::new(4);
    let (r, s) = begin(&reg, &published);
    assert_eq!(s, 4);
    published.store(9, Ordering::SeqCst);
    assert_eq!(horizon(&reg, &published), 4);
    let (r2, s2) = begin(&reg, &published);
    assert_eq!(s2, 9);
    drop(r);
    assert_eq!(horizon(&reg, &published), 9);
    drop(r2);
    assert_eq!(horizon(&reg, &published), 9);
}

/// `loom` model of the snapshot registry against a garbage-collection pass
/// (TS-14 Snapshot registry, TS-30): one thread publishes a new commit
/// timestamp while a transaction registers its snapshot and a pass
/// computes the horizon. In every interleaving, the horizon computed while
/// the transaction was registered is at most its snapshot, so nothing the
/// snapshot reads is freed.
#[cfg(loom)]
#[test]
fn loom_no_snapshot_is_below_a_horizon_computed_while_it_registers() {
    loom_model(false);
}

/// The same model with the registry's first block full (one slot under
/// `loom`): the transaction registers in an overflow block, under the
/// overflow mutex (TS-14 Snapshot registry, no fixed cap).
#[cfg(loom)]
#[test]
fn loom_no_overflow_snapshot_is_below_a_horizon_computed_while_it_registers() {
    loom_model(true);
}

/// One model run; with `full`, a registration held for the whole run at a
/// value above every timestamp, so it never lowers the horizon, fills the
/// first block.
#[cfg(loom)]
fn loom_model(full: bool) {
    use crate::sync::Arc;
    use crate::sync::atomic::AtomicBool;
    loom::model(move || {
        let reg = Arc::new(Readers::default());
        let held = full.then(|| reg.enter(&AtomicU64::new(u64::MAX - 1)));
        let published = Arc::new(AtomicU64::new(1));
        let h = Arc::new(AtomicU64::new(u64::MAX));
        let gc_done = Arc::new(AtomicBool::new(false));
        let publisher = {
            let published = Arc::clone(&published);
            loom::thread::spawn(move || published.store(2, Ordering::SeqCst))
        };
        let gc = {
            let (reg, published, h, gc_done) = (
                Arc::clone(&reg),
                Arc::clone(&published),
                Arc::clone(&h),
                Arc::clone(&gc_done),
            );
            loom::thread::spawn(move || {
                h.store(horizon(&reg, &published), Ordering::SeqCst);
                gc_done.store(true, Ordering::SeqCst);
            })
        };
        let (r, s) = begin(&reg, &published);
        while !gc_done.load(Ordering::SeqCst) {
            loom::thread::yield_now();
        }
        let h = h.load(Ordering::SeqCst);
        assert!(h <= s, "horizon {h} above snapshot {s}");
        drop(r);
        drop(held);
        publisher.join().unwrap();
        gc.join().unwrap();
    });
}
