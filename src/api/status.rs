//! The status of a transaction ([TS-20] Limits and cancellation; rmp #49).
//!
//! One atomic per transaction holds whether it may go on and, once it may
//! not, why: an API cancel (FR-API-009), a termination (FR-OBS-004) or its
//! time limit's timer (FR-API-007). Operators read it before each batch
//! and every [`crate::query::exec::CHECK_EVERY`] loop iterations, through
//! the cursor's status check; a stopped status fails the statement with
//! 25N14 and the reason, and the statement's failure aborts the
//! transaction.
//!
//! **Cancel against commit.** A commit first moves the status from
//! `ACTIVE` to `COMMITTING` with a compare-and-swap; a cancel, termination
//! or timeout moves it from `ACTIVE` to its reason the same way. Exactly
//! one of the two swaps succeeds, so either the commit proceeds and the
//! stop has no effect, or the stop wins and the commit aborts the
//! transaction; never both. The `loom` model below checks it, and fails
//! when the commit ignores the status (mutation).
//!
//! The atomic is the only shared state the swaps decide: no data is
//! published through it, so `AcqRel` on the swaps and `Acquire` on the
//! loads are enough; the single location's modification order gives the
//! mutual exclusion. zu @ `a821319` keeps one atomic interrupt flag per
//! query (`crates/zu-common/src/interrupt.rs`, studied); Memgraph checks an
//! abort reason per transaction in its cursors
//! (`src/query/plan/operator.cpp:510-528`, BSL, studied only).
//!
//! [TS-20]: ../../../docs/technical-spec/query-engine.md

use crate::error::Error;
use crate::gqlstatus::codes;
use crate::sync::atomic::{AtomicU32, Ordering};

/// The transaction may go on.
const ACTIVE: u32 = 0;
/// A commit is under way: stops have no effect.
const COMMITTING: u32 = 1;
/// Stopped by an API cancel.
const CANCELLED: u32 = 2;
/// Stopped by a termination.
const TERMINATED: u32 = 3;
/// Stopped by the time limit.
const TIMED_OUT: u32 = 4;

/// Why a transaction was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    Cancelled,
    Terminated,
    TimedOut,
}

impl Reason {
    fn code(self) -> u32 {
        match self {
            Reason::Cancelled => CANCELLED,
            Reason::Terminated => TERMINATED,
            Reason::TimedOut => TIMED_OUT,
        }
    }

    /// The reason in words, for the error message and the transaction
    /// list.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Reason::Cancelled => "cancelled",
            Reason::Terminated => "terminated",
            Reason::TimedOut => "timed out",
        }
    }

    /// The error of a statement of a transaction stopped for this reason
    /// (25N14, `interfaces.md`).
    pub(crate) fn error(self) -> Error {
        Error::new(
            codes::C25N14,
            format!(
                "transaction termination client error: the transaction was {}",
                self.text()
            ),
        )
    }
}

/// The status of one transaction (module documentation).
#[derive(Debug)]
pub(crate) struct Status(AtomicU32);

impl Default for Status {
    fn default() -> Self {
        Status(AtomicU32::new(ACTIVE))
    }
}

impl Status {
    /// Stops the transaction for `reason` if it is active. Returns whether
    /// this call stopped it; a commit under way, or an earlier stop, makes
    /// it a no-op.
    pub(crate) fn stop(&self, reason: Reason) -> bool {
        self.0
            .compare_exchange(ACTIVE, reason.code(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Starts the commit: from now on stops have no effect.
    ///
    /// # Errors
    ///
    /// The reason the transaction was stopped, if it was.
    pub(crate) fn begin_commit(&self) -> Result<(), Reason> {
        match self
            .0
            .compare_exchange(ACTIVE, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(now) => match Status::reason_of(now) {
                Some(r) => Err(r),
                // Already committing: the caller holds the transaction, so
                // this does not happen; it is not a stop either.
                None => Ok(()),
            },
        }
    }

    fn reason_of(v: u32) -> Option<Reason> {
        match v {
            CANCELLED => Some(Reason::Cancelled),
            TERMINATED => Some(Reason::Terminated),
            TIMED_OUT => Some(Reason::TimedOut),
            _ => None,
        }
    }

    /// Why the transaction was stopped, if it was.
    pub(crate) fn reason(&self) -> Option<Reason> {
        Status::reason_of(self.0.load(Ordering::Acquire))
    }

    /// The status check of the transaction's statements.
    ///
    /// # Errors
    ///
    /// 25N14 with the reason once the transaction is stopped.
    pub(crate) fn check(&self) -> Result<(), Error> {
        match self.reason() {
            Some(r) => Err(r.error()),
            None => Ok(()),
        }
    }
}

#[cfg(all(test, loom))]
mod loom_tests {
    use super::*;
    use crate::sync::Arc;
    use crate::sync::atomic::AtomicBool;

    /// A cancel racing a commit: the commit side either begins the commit
    /// or aborts; the cancel side reports whether it stopped the
    /// transaction. With `ignore_status` the commit stores `COMMITTING`
    /// without looking (the mutation).
    fn model(ignore_status: bool) {
        loom::model(move || {
            let status = Arc::new(Status::default());
            let committed = Arc::new(AtomicBool::new(false));
            let s = status.clone();
            let c = committed.clone();
            let commit = loom::thread::spawn(move || {
                let ok = if ignore_status {
                    s.0.store(COMMITTING, Ordering::Release);
                    true
                } else {
                    s.begin_commit().is_ok()
                };
                c.store(ok, Ordering::Release);
            });
            let stopped = status.stop(Reason::Cancelled);
            commit.join().expect("the commit thread");
            let committed = committed.load(Ordering::Acquire);
            // Exactly one: the commit completes and the cancel had no
            // effect, or the cancel stopped it and the commit aborts.
            assert!(
                committed != stopped,
                "committed: {committed}, cancel took effect: {stopped}"
            );
        });
    }

    #[test]
    fn loom_a_cancel_racing_a_commit_either_aborts_or_has_no_effect() {
        model(false);
    }

    #[test]
    fn loom_the_model_fails_when_the_commit_ignores_the_status() {
        let r = std::panic::catch_unwind(|| model(true));
        assert!(r.is_err(), "the mutation was not detected");
    }
}
