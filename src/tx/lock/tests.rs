use super::*;

fn key(k: &str) -> Resource {
    Resource::Key {
        tree: 1,
        key: k.as_bytes().into(),
    }
}

/// Waits, without timing, until `n` requests wait in `lt`; a missing wait
/// fails the test after [`crate::test_support::WAIT_BOUND`].
#[cfg(not(any(loom, shuttle)))]
fn until_waiting(lt: &LockTable, n: usize) {
    crate::test_support::wait_until("lock requests wait", || lt.waiting() >= n);
}

#[cfg(not(any(loom, shuttle)))]
mod threads {
    use super::*;
    use crate::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn shared_locks_coexist_and_exclusive_waits_for_them() {
        let lt = LockTable::default();
        let a = key("a");
        assert_eq!(lt.lock(1, &a, Mode::S).unwrap(), Granted::Now);
        assert_eq!(lt.lock(2, &a, Mode::S).unwrap(), Granted::Now);
        assert_eq!(lt.lock(1, &a, Mode::S).unwrap(), Granted::Already);
        let done = AtomicU64::new(0);
        std::thread::scope(|s| {
            s.spawn(|| {
                assert_eq!(lt.lock(3, &a, Mode::X).unwrap(), Granted::AfterWait);
                done.store(1, Ordering::SeqCst);
                lt.release_all(3);
            });
            until_waiting(&lt, 1);
            lt.release_all(1);
            // Still held in S by 2.
            assert_eq!(lt.waiting(), 1);
            assert_eq!(done.load(Ordering::SeqCst), 0);
            lt.release_all(2);
        });
        assert_eq!(done.load(Ordering::SeqCst), 1);
        assert!(lt.is_empty());
    }

    /// A new S request waits behind a waiting X request (TS-15 Queue), and
    /// is granted after it.
    #[test]
    fn a_shared_request_waits_behind_a_waiting_exclusive_one() {
        let lt = LockTable::default();
        let a = key("a");
        lt.lock(1, &a, Mode::S).unwrap();
        let order = AtomicU64::new(0);
        let (x_at, s_at) = (AtomicU64::new(0), AtomicU64::new(0));
        std::thread::scope(|s| {
            s.spawn(|| {
                lt.lock(2, &a, Mode::X).unwrap();
                x_at.store(order.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                lt.release_all(2);
            });
            until_waiting(&lt, 1);
            s.spawn(|| {
                lt.lock(3, &a, Mode::S).unwrap();
                s_at.store(order.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                lt.release_all(3);
            });
            until_waiting(&lt, 2);
            lt.release_all(1);
        });
        assert_eq!(
            (x_at.load(Ordering::SeqCst), s_at.load(Ordering::SeqCst)),
            (1, 2)
        );
        assert!(lt.is_empty());
    }

    /// An S holder's upgrade goes ahead of a waiting X request; a second
    /// upgrader closes a cycle and fails with `Deadlock` (TS-15 Upgrade).
    #[test]
    fn upgrades_go_first_and_two_upgraders_deadlock() {
        let lt = LockTable::default();
        let a = key("a");
        lt.lock(1, &a, Mode::S).unwrap();
        lt.lock(2, &a, Mode::S).unwrap();
        std::thread::scope(|s| {
            let w = s.spawn(|| lt.lock(3, &a, Mode::X));
            until_waiting(&lt, 1);
            let up = s.spawn(|| lt.lock(1, &a, Mode::X));
            until_waiting(&lt, 2);
            // 2 also upgrades: 1 waits for 2's S, 2 would wait for 1's S.
            assert!(matches!(lt.lock(2, &a, Mode::X), Err(Error::Deadlock)));
            lt.release_all(2);
            assert_eq!(up.join().unwrap().unwrap(), Granted::Upgraded);
            assert_eq!(lt.held(1), vec![(a.clone(), Mode::X)]);
            assert_eq!(lt.waiting(), 1);
            lt.release_all(1);
            assert_eq!(w.join().unwrap().unwrap(), Granted::AfterWait);
            lt.release_all(3);
        });
        assert!(lt.is_empty());
    }

    /// FR-TX-007 at the lock table: T1 holds A and waits for B; T2 holds B
    /// and requests A, which closes the cycle: T2's request fails with
    /// `Deadlock` (50N05) and T1 gets B once T2 releases.
    #[test]
    fn the_request_that_closes_a_two_transaction_cycle_fails() {
        let lt = LockTable::default();
        let (a, b) = (key("a"), key("b"));
        lt.lock(1, &a, Mode::X).unwrap();
        lt.lock(2, &b, Mode::X).unwrap();
        std::thread::scope(|s| {
            let t1 = s.spawn(|| lt.lock(1, &b, Mode::X));
            until_waiting(&lt, 1);
            let e = lt.lock(2, &a, Mode::X).unwrap_err();
            assert!(matches!(e, Error::Deadlock));
            assert_eq!(e.gqlstatus().code(), "50N05");
            lt.release_all(2);
            assert_eq!(t1.join().unwrap().unwrap(), Granted::AfterWait);
        });
        lt.release_all(1);
        assert!(lt.is_empty());
    }

    #[test]
    fn the_request_that_closes_a_three_transaction_cycle_fails() {
        let lt = LockTable::default();
        let (a, b, c) = (key("a"), key("b"), key("c"));
        lt.lock(1, &a, Mode::X).unwrap();
        lt.lock(2, &b, Mode::X).unwrap();
        lt.lock(3, &c, Mode::X).unwrap();
        std::thread::scope(|s| {
            let t1 = s.spawn(|| lt.lock(1, &b, Mode::X));
            until_waiting(&lt, 1);
            let t2 = s.spawn(|| lt.lock(2, &c, Mode::X));
            until_waiting(&lt, 2);
            assert!(matches!(lt.lock(3, &a, Mode::X), Err(Error::Deadlock)));
            lt.release_all(3);
            assert_eq!(t2.join().unwrap().unwrap(), Granted::AfterWait);
            lt.release_all(2);
            assert_eq!(t1.join().unwrap().unwrap(), Granted::AfterWait);
            lt.release_all(1);
        });
        assert!(lt.is_empty());
    }

    /// A cycle closed only through a waiting-request edge: T1 holds S(a);
    /// T2 waits for X(a) behind it; T3 holds X(b) and waits for S(a) behind
    /// T2's waiting request; T1's request for X(b) closes T1 → T3 → T2 →
    /// T1 and fails.
    #[test]
    fn a_cycle_through_a_waiting_request_is_found() {
        let lt = LockTable::default();
        let (a, b) = (key("a"), key("b"));
        lt.lock(1, &a, Mode::S).unwrap();
        lt.lock(3, &b, Mode::X).unwrap();
        std::thread::scope(|s| {
            let t2 = s.spawn(|| {
                let r = lt.lock(2, &a, Mode::X);
                lt.release_all(2);
                r
            });
            until_waiting(&lt, 1);
            let t3 = s.spawn(|| {
                let r = lt.lock(3, &a, Mode::S);
                lt.release_all(3);
                r
            });
            until_waiting(&lt, 2);
            assert!(matches!(lt.lock(1, &b, Mode::X), Err(Error::Deadlock)));
            lt.release_all(1);
            assert_eq!(t2.join().unwrap().unwrap(), Granted::AfterWait);
            assert_eq!(t3.join().unwrap().unwrap(), Granted::AfterWait);
        });
        assert!(lt.is_empty());
    }
}

/// `loom` model of grant and release (TS-30): T1 holds S(a); T2 asks for
/// X(a) and waits; T3 then asks for S(a). In every interleaving, T2 is
/// granted before T3, which waits behind T2's waiting request.
#[cfg(loom)]
#[test]
fn loom_lock_grants_follow_the_queue() {
    use crate::sync::Arc;
    use crate::sync::atomic::{AtomicU64, Ordering};
    loom::model(|| {
        let lt = Arc::new(LockTable::default());
        let order = Arc::new(AtomicU64::new(0));
        lt.lock(1, &key("a"), Mode::S).unwrap();
        let t2 = {
            let (lt, order) = (Arc::clone(&lt), Arc::clone(&order));
            loom::thread::spawn(move || {
                lt.lock(2, &key("a"), Mode::X).unwrap();
                let at = order.fetch_add(1, Ordering::SeqCst);
                lt.release_all(2);
                at
            })
        };
        while lt.waiting() < 1 {
            loom::thread::yield_now();
        }
        let t3 = {
            let (lt, order) = (Arc::clone(&lt), Arc::clone(&order));
            loom::thread::spawn(move || {
                lt.lock(3, &key("a"), Mode::S).unwrap();
                let at = order.fetch_add(1, Ordering::SeqCst);
                lt.release_all(3);
                at
            })
        };
        lt.release_all(1);
        let (x, s) = (t2.join().unwrap(), t3.join().unwrap());
        assert!(x < s, "X granted at {x}, the later S at {s}");
        assert!(lt.is_empty());
    });
}

/// `loom` model of deadlock detection on a two-transaction cycle (TS-30):
/// T1 holds a and T2 holds b, taken before the threads start (locks belong
/// to transactions, not threads); then each asks for the other's. In every
/// interleaving, exactly one request fails with `Deadlock` and the other is
/// granted.
#[cfg(loom)]
#[test]
fn loom_two_transaction_cycle_fails_exactly_one_request() {
    use crate::sync::Arc;
    loom::model(|| {
        let lt = Arc::new(LockTable::default());
        lt.lock(1, &key("a"), Mode::X).unwrap();
        lt.lock(2, &key("b"), Mode::X).unwrap();
        let run = |tx: TxId, second: &'static str| {
            let lt = Arc::clone(&lt);
            loom::thread::spawn(move || {
                let r = lt.lock(tx, &key(second), Mode::X);
                lt.release_all(tx);
                r.is_err()
            })
        };
        let t1 = run(1, "b");
        let t2 = run(2, "a");
        let failed = usize::from(t1.join().unwrap()) + usize::from(t2.join().unwrap());
        assert_eq!(failed, 1);
        assert!(lt.is_empty());
    });
}

/// `loom` model of deadlock detection on a three-transaction cycle: T1
/// holds a and asks for b, T2 holds b and asks for c, T3 holds c and asks
/// for a. Exactly one request fails in every interleaving.
#[cfg(loom)]
#[test]
fn loom_three_transaction_cycle_fails_exactly_one_request() {
    use crate::sync::Arc;
    loom::model(|| {
        let lt = Arc::new(LockTable::default());
        for (tx, k) in [(1, "a"), (2, "b"), (3, "c")] {
            lt.lock(tx, &key(k), Mode::X).unwrap();
        }
        let run = |tx: TxId, second: &'static str| {
            let lt = Arc::clone(&lt);
            loom::thread::spawn(move || {
                let r = lt.lock(tx, &key(second), Mode::X);
                lt.release_all(tx);
                r.is_err()
            })
        };
        let ts = [run(1, "b"), run(2, "c"), run(3, "a")];
        let failed: usize = ts.into_iter().map(|t| usize::from(t.join().unwrap())).sum();
        assert_eq!(failed, 1);
        assert!(lt.is_empty());
    });
}

/// `loom` model of a cycle closed through a waiting-request edge (see
/// `a_cycle_through_a_waiting_request_is_found`): T1's request fails with
/// `Deadlock` in every interleaving, and the others then complete.
#[cfg(loom)]
#[test]
fn loom_cycle_through_a_waiting_request_fails_the_requester() {
    use crate::sync::Arc;
    loom::model(|| {
        let lt = Arc::new(LockTable::default());
        lt.lock(1, &key("a"), Mode::S).unwrap();
        lt.lock(3, &key("b"), Mode::X).unwrap();
        let t2 = {
            let lt = Arc::clone(&lt);
            loom::thread::spawn(move || {
                lt.lock(2, &key("a"), Mode::X).unwrap();
                lt.release_all(2);
            })
        };
        while lt.waiting() < 1 {
            loom::thread::yield_now();
        }
        let t3 = {
            let lt = Arc::clone(&lt);
            loom::thread::spawn(move || {
                lt.lock(3, &key("a"), Mode::S).unwrap();
                lt.release_all(3);
            })
        };
        while lt.waiting() < 2 {
            loom::thread::yield_now();
        }
        assert!(matches!(
            lt.lock(1, &key("b"), Mode::X),
            Err(Error::Deadlock)
        ));
        lt.release_all(1);
        t2.join().unwrap();
        t3.join().unwrap();
        assert!(lt.is_empty());
    });
}
