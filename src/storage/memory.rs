//! The memory accountant (TS-08, FR-LIM-003).
//!
//! One accountant per database holds an atomic byte budget equal to the
//! memory cap. Every user that TS-08 lists reserves from it before it
//! allocates; in Sprint 1 these are buffer-pool frames, the page copies of
//! a commit's apply step (TS-16) and sort buffers.
//! Memory TS-08 places outside the cap (WAL page images, the free-page set,
//! the 1 MiB copy buffer) never reserves.
//!
//! The accountant only counts. Making room when a reservation fails is the
//! buffer pool's job ([`super::pool::Pool::reserve`]), since only the pool
//! holds memory it can give back.
//!
//! Every change of the reserved total is published to the database's
//! metrics sink as the gauge [`ACCOUNTED_BYTES`], as the store-size gauges
//! are published on each change ([`super::datafile`]). Before task #15 the
//! gauge was set only when the pool was created, so it read 6 875 136
//! while the accountant held 49 956 864 bytes (laboratory report, #15
//! "Open point 6").

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::obs::MetricsSink;
use crate::obs::names::cache::ACCOUNTED_BYTES;

/// The byte budget of one database.
pub(crate) struct MemoryAccountant {
    cap: u64,
    used: AtomicU64,
    /// Highest value `used` ever reached, for tests and the laboratory.
    peak: AtomicU64,
    /// Receives [`ACCOUNTED_BYTES`] after every change, if set.
    sink: Option<Arc<dyn MetricsSink>>,
    /// Serialises the gauge publications. Each change publishes the total it
    /// reads under this latch after making the change, so the last
    /// publication reads the total after the last change: the gauge never
    /// stays at a stale value once the changes stop.
    publish: Mutex<()>,
}

impl fmt::Debug for MemoryAccountant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryAccountant")
            .field("cap", &self.cap)
            .field("used", &self.used)
            .field("peak", &self.peak)
            .finish_non_exhaustive()
    }
}

impl MemoryAccountant {
    /// An accountant with budget `cap` bytes that publishes nothing.
    pub(crate) fn new(cap: u64) -> Arc<MemoryAccountant> {
        Arc::new(MemoryAccountant {
            cap,
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            sink: None,
            publish: Mutex::new(()),
        })
    }

    /// An accountant with budget `cap` bytes that publishes the reserved
    /// total to `sink` after every change (FR-OBS-001, cache category).
    pub(crate) fn with_sink(cap: u64, sink: Arc<dyn MetricsSink>) -> Arc<MemoryAccountant> {
        let a = Arc::new(MemoryAccountant {
            cap,
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            sink: Some(sink),
            publish: Mutex::new(()),
        });
        a.publish();
        a
    }

    /// Sends the reserved total to the sink.
    fn publish(&self) {
        if let Some(sink) = &self.sink {
            // A panic in a sink cannot leave `()` inconsistent.
            let _one_at_a_time = self.publish.lock().unwrap_or_else(PoisonError::into_inner);
            // Relaxed suffices: the change this publication follows is
            // ordered before the latch by program order, and the latch
            // orders every earlier publication's change before this load.
            sink.gauge(ACCOUNTED_BYTES, self.used.load(Ordering::Relaxed));
        }
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
                    self.publish();
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
        self.publish();
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

    /// Takes over the bytes of `other`, a reservation from the same
    /// accountant, so that one guard releases both.
    pub(crate) fn absorb(&mut self, mut other: Reservation) {
        debug_assert!(Arc::ptr_eq(&self.accountant, &other.accountant));
        self.bytes += other.bytes;
        other.bytes = 0;
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

    /// The gauge follows every change and ends at the final total (task
    /// #15: it was set only when the pool was created).
    #[test]
    fn the_gauge_follows_every_change() {
        let sink = Arc::new(crate::obs::recording::RecordingSink::default());
        let a = MemoryAccountant::with_sink(100, sink.clone());
        assert_eq!(sink.snapshot().gauge(ACCOUNTED_BYTES), Some(0));
        let r = a.try_reserve(60).unwrap();
        assert_eq!(sink.snapshot().gauge(ACCOUNTED_BYTES), Some(60));
        assert!(a.try_reserve(41).is_none());
        assert_eq!(sink.snapshot().gauge(ACCOUNTED_BYTES), Some(60));
        drop(r);
        assert_eq!(sink.snapshot().gauge(ACCOUNTED_BYTES), Some(0));
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..1000 {
                        drop(a.try_reserve(3));
                    }
                });
            }
            let _held = a.try_reserve(10).unwrap();
        });
        let _held = a.try_reserve(7).unwrap();
        assert_eq!(sink.snapshot().gauge(ACCOUNTED_BYTES), Some(7));
    }

    /// 16 threads reserve and release concurrently; every successful
    /// reservation sees the total within the cap, and the peak never
    /// exceeds it.
    #[test]
    fn concurrent_reservations_never_exceed_the_cap() {
        const CAP: u64 = 1 << 20;
        const CHUNK: u64 = 64 << 10;
        let a = MemoryAccountant::new(CAP);
        let all = std::sync::Barrier::new(16);
        std::thread::scope(|s| {
            for t in 0..16u64 {
                let (a, all) = (&a, &all);
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
                    held.clear();
                    // Fill: every thread races to the cap and holds what it
                    // got. Nothing is released, so once a reservation is
                    // refused the free bytes are below one chunk for good.
                    all.wait();
                    while let Some(r) = a.try_reserve(CHUNK) {
                        assert!(a.used() <= CAP);
                        held.push(r);
                    }
                    all.wait();
                    assert!(a.used() > CAP - CHUNK, "used {}", a.used());
                    all.wait();
                });
            }
        });
        assert!(a.peak() <= CAP && a.peak() > CAP / 2, "peak {}", a.peak());
        assert_eq!(a.used(), 0);
    }
}
