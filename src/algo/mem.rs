//! The memory of an algorithm run ([TS-25] Memory; rmp #63).
//!
//! A run holds one reservation from the memory accountant for its
//! projection and state, from the projection's first array to the end of
//! the computation, in the shape of a blocking operator's
//! ([`Held::grow`], `src/query/exec/mod.rs`): reserved in chunks of at
//! least [`crate::query::exec::CHUNK`] bytes, each checked first against
//! the transaction's memory limit (51N73) and then by the buffer pool,
//! which evicts before it refuses (51N72). Arrays grow only through
//! [`Mem::push`] and [`Mem::vec`], which reserve before they allocate, so
//! the reservation always covers the capacity of every array. Not
//! charged: the moment of a reallocation, when an array's old buffer is
//! still held beside its new one, and the copy of one scan chunk
//! ([`super::projection::SCAN_CHUNK`] entries).
//!
//! [TS-25]: ../../docs/technical-spec/algorithms.md

use crate::error::Error;
use crate::query::exec::{Check, Cx, Held, State};
use crate::tx::Transaction;

/// The smallest number of elements an array grows by.
const MIN_GROWTH: usize = 1024;

/// The memory, transaction and statement of a run (module documentation).
pub(crate) struct Mem<'r> {
    pub(crate) tx: &'r mut Transaction,
    pub(crate) st: &'r mut State,
    held: Held,
}

impl<'r> Mem<'r> {
    /// A run's memory, with nothing reserved yet.
    pub(crate) fn new(tx: &'r mut Transaction, st: &'r mut State) -> Mem<'r> {
        Mem {
            tx,
            st,
            held: Held::default(),
        }
    }

    /// Reserves `bytes` more for the run.
    ///
    /// # Errors
    ///
    /// 51N72 when the memory cap refuses; 51N73 beyond the transaction's
    /// memory limit.
    pub(crate) fn grow(&mut self, bytes: usize) -> Result<(), Error> {
        let mut cx = Cx {
            tx: self.tx,
            st: self.st,
        };
        self.held.grow(&mut cx, bytes as u64)
    }

    /// A vector of `n` copies of `x`, reserved first.
    ///
    /// # Errors
    ///
    /// As [`Mem::grow`].
    pub(crate) fn vec<T: Clone>(&mut self, n: usize, x: T) -> Result<Vec<T>, Error> {
        self.grow(n.saturating_mul(size_of::<T>()))?;
        Ok(vec![x; n])
    }

    /// Pushes `x` onto `v`, reserving the growth of its capacity first.
    ///
    /// # Errors
    ///
    /// As [`Mem::grow`].
    pub(crate) fn push<T>(&mut self, v: &mut Vec<T>, x: T) -> Result<(), Error> {
        if v.len() == v.capacity() {
            let add = v.capacity().max(MIN_GROWTH);
            self.grow(add.saturating_mul(size_of::<T>()))?;
            v.reserve_exact(add);
        }
        v.push(x);
        Ok(())
    }

    /// The statement's status check, for the workers.
    pub(crate) fn check(&self) -> Option<Check> {
        self.st.check.clone()
    }

    /// Fails once the statement is stopped (25N14).
    ///
    /// # Errors
    ///
    /// As above.
    pub(crate) fn check_now(&self) -> Result<(), Error> {
        match &self.st.check {
            Some(c) => c(self.st.iterations),
            None => Ok(()),
        }
    }

    /// The bytes the run has reserved (the laboratory record).
    #[cfg(test)]
    pub(crate) fn reserved(&self) -> u64 {
        self.held.peak()
    }

    /// Ends the run and keeps `keep` bytes of its reservation for the rows
    /// it returns, which [`Mem::grow`] already charged; the rest is
    /// released.
    pub(crate) fn keep(mut self, keep: usize) -> Held {
        self.held.shrink_to(keep as u64);
        std::mem::take(&mut self.held)
    }

    /// Ends the run's reservation and returns one of `keep` bytes for the
    /// result the run returns, reserved before the run's is released, so
    /// that the accountant never counts less than is allocated.
    ///
    /// # Errors
    ///
    /// As [`Mem::grow`].
    pub(crate) fn finish(mut self, keep: usize) -> Result<Held, Error> {
        let mut out = Held::default();
        let mut cx = Cx {
            tx: self.tx,
            st: self.st,
        };
        out.grow(&mut cx, keep as u64)?;
        self.held.release();
        Ok(out)
    }
}
