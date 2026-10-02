//! The memory accountant (TS-08, FR-LIM-003).
//!
//! One accountant per database holds an atomic byte budget equal to the
//! memory cap. Every user that TS-08 lists reserves from it before it
//! allocates; in Sprint 1 these are buffer-pool frames and sort buffers.
//! Memory TS-08 places outside the cap (WAL page images, the free-page set,
//! the 1 MiB copy buffer) never reserves.
//!
//! The accountant only counts. Making room when a reservation fails is the
//! buffer pool's job ([`super::pool::Pool::reserve`]), since only the pool
//! holds memory it can give back.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The byte budget of one database.
#[derive(Debug)]
pub(crate) struct MemoryAccountant {
    cap: u64,
    used: AtomicU64,
    /// Highest value `used` ever reached, for tests and the laboratory.
    peak: AtomicU64,
}

impl MemoryAccountant {
    /// An accountant with budget `cap` bytes.
    pub(crate) fn new(cap: u64) -> Arc<MemoryAccountant> {
        Arc::new(MemoryAccountant {
            cap,
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        })
    }

    /// The memory cap in bytes.
    pub(crate) fn cap(&self) -> u64 {
        self.cap
    }

    /// Bytes reserved now.
    pub(crate) fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    /// Highest number of bytes ever reserved at once.
    pub(crate) fn peak(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }

    /// Adds `bytes` to the reserved total if the total stays within the
    /// cap. Returns whether it did. The caller must [`release`] the same
    /// amount later.
    ///
    /// [`release`]: MemoryAccountant::release
    #[must_use]
    pub(crate) fn try_charge(&self, bytes: u64) -> bool {
        // The compare-and-swap checks the cap on every reservation, so no
        // interleaving of reservations can exceed it. Relaxed suffices: the
        // counter orders no other memory.
        let mut used = self.used.load(Ordering::Relaxed);
        loop {
            let Some(total) = used.checked_add(bytes).filter(|&t| t <= self.cap) else {
                return false;
            };
            match self
                .used
                .compare_exchange_weak(used, total, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => {
                    self.peak.fetch_max(total, Ordering::Relaxed);
                    return true;
                }
                Err(now) => used = now,
            }
        }
    }

    /// Returns `bytes` charged earlier.
    pub(crate) fn release(&self, bytes: u64) {
        let before = self.used.fetch_sub(bytes, Ordering::Relaxed);
        debug_assert!(before >= bytes, "released more than was charged");
    }

    /// Reserves `bytes` as a guard that releases them when dropped, or
    /// returns `None` if they do not fit.
    #[must_use]
    pub(crate) fn try_reserve(self: &Arc<Self>, bytes: u64) -> Option<Reservation> {
        self.try_charge(bytes).then(|| Reservation {
            accountant: Arc::clone(self),
            bytes,
        })
    }
}

/// Bytes reserved from a [`MemoryAccountant`]; released on drop.
#[derive(Debug)]
#[must_use = "dropping a reservation releases it at once"]
pub(crate) struct Reservation {
    accountant: Arc<MemoryAccountant>,
    bytes: u64,
}

impl Reservation {
    /// Bytes held.
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.accountant.release(self.bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_stop_at_the_cap_and_release_on_drop() {
        let a = MemoryAccountant::new(100);
        let r1 = a.try_reserve(60).unwrap();
        assert!(a.try_reserve(41).is_none());
        let r2 = a.try_reserve(40).unwrap();
        assert_eq!((a.used(), a.peak()), (100, 100));
        assert!(!a.try_charge(1));
        drop(r1);
        assert_eq!(a.used(), 40);
        assert_eq!(r2.bytes(), 40);
        drop(r2);
        assert_eq!(a.used(), 0);
        assert!(!a.try_charge(u64::MAX));
    }

    /// 16 threads reserve and release concurrently; every successful
    /// reservation sees the total within the cap, and the peak never
    /// exceeds it.
    #[test]
    fn concurrent_reservations_never_exceed_the_cap() {
        const CAP: u64 = 1 << 20;
        let a = MemoryAccountant::new(CAP);
        std::thread::scope(|s| {
            for t in 0..16u64 {
                let a = &a;
                s.spawn(move || {
                    let mut held = Vec::new();
                    for i in 0..20_000u64 {
                        let bytes = 1 + (i * 7919 + t * 104_729) % 65_536;
                        if let Some(r) = a.try_reserve(bytes) {
                            assert!(a.used() <= CAP);
                            held.push(r);
                        }
                        if held.len() > 4 || i % 3 == 0 {
                            held.pop();
                        }
                    }
                });
            }
        });
        assert!(a.peak() <= CAP && a.peak() > CAP / 2, "peak {}", a.peak());
        assert_eq!(a.used(), 0);
    }
}
