//! A thread-safe [`MetricsSink`] that records every call, for tests and the
//! laboratory.

use crate::sync::Mutex;
use std::collections::BTreeMap;
use std::fmt;

use super::MetricsSink;

/// Recorded metrics: counter totals, last gauge values and histogram samples
/// in arrival order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Metrics {
    counters: BTreeMap<&'static str, u64>,
    gauges: BTreeMap<&'static str, u64>,
    histograms: BTreeMap<&'static str, Vec<u64>>,
}

impl Metrics {
    /// Returns what happened after `before`, a snapshot of the same sink:
    /// counter increments, gauges whose value differs from `before`, and the
    /// histogram samples recorded since then.
    pub(crate) fn since(&self, before: &Metrics) -> Metrics {
        let counters = self
            .counters
            .iter()
            .map(|(&k, &v)| (k, v - before.counters.get(k).copied().unwrap_or(0)))
            .filter(|&(_, v)| v > 0)
            .collect();
        let gauges = self
            .gauges
            .iter()
            .filter(|&(k, v)| before.gauges.get(k) != Some(v))
            .map(|(&k, &v)| (k, v))
            .collect();
        let histograms = self
            .histograms
            .iter()
            .map(|(&k, v)| {
                (
                    k,
                    v[before.histograms.get(k).map_or(0, Vec::len)..].to_vec(),
                )
            })
            .filter(|(_, v)| !v.is_empty())
            .collect();
        Metrics {
            counters,
            gauges,
            histograms,
        }
    }

    /// Counter total, 0 if never incremented.
    pub(crate) fn counter(&self, name: &str) -> u64 {
        self.counters.get(name).copied().unwrap_or(0)
    }

    /// Last gauge value, if set.
    pub(crate) fn gauge(&self, name: &str) -> Option<u64> {
        self.gauges.get(name).copied()
    }

    /// Histogram samples in arrival order.
    pub(crate) fn samples(&self, name: &str) -> &[u64] {
        self.histograms.get(name).map_or(&[], Vec::as_slice)
    }
}

/// Compact one-line form, `-` when empty: `name +n`, `name =v`,
/// `name n samples`.
impl fmt::Display for Metrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        parts.extend(self.counters.iter().map(|(k, v)| format!("{k} +{v}")));
        parts.extend(self.gauges.iter().map(|(k, v)| format!("{k} ={v}")));
        parts.extend(
            self.histograms
                .iter()
                .map(|(k, v)| format!("{k} {} samples", v.len())),
        );
        if parts.is_empty() {
            f.write_str("-")
        } else {
            f.write_str(&parts.join("; "))
        }
    }
}

/// Records every call under one mutex.
#[derive(Debug, Default)]
pub(crate) struct RecordingSink(Mutex<Metrics>);

impl RecordingSink {
    /// Returns a copy of everything recorded so far.
    pub(crate) fn snapshot(&self) -> Metrics {
        self.lock().clone()
    }

    fn lock(&self) -> crate::sync::MutexGuard<'_, Metrics> {
        self.0
            .lock()
            .expect("recording sink poisoned by a panicking test")
    }
}

impl MetricsSink for RecordingSink {
    fn counter(&self, name: &'static str, delta: u64) {
        *self.lock().counters.entry(name).or_insert(0) += delta;
    }

    fn gauge(&self, name: &'static str, value: u64) {
        self.lock().gauges.insert(name, value);
    }

    fn histogram(&self, name: &'static str, value: u64) {
        self.lock().histograms.entry(name).or_default().push(value);
    }
}

#[cfg(test)]
mod tests {
    use crate::sync::Arc;

    use super::*;

    #[test]
    fn concurrent_calls_are_all_recorded() {
        let sink = Arc::new(RecordingSink::default());
        std::thread::scope(|s| {
            for _ in 0..8 {
                let sink = &sink;
                s.spawn(move || {
                    for i in 0..1000 {
                        sink.counter("c", 1);
                        sink.histogram("h", i);
                    }
                });
            }
        });
        let m = sink.snapshot();
        assert_eq!(m.counter("c"), 8000);
        assert_eq!(m.samples("h").len(), 8000);
    }

    #[test]
    fn difference_and_display() {
        let sink = RecordingSink::default();
        sink.counter("c", 4);
        sink.gauge("g", 1);
        sink.gauge("same", 5);
        sink.histogram("h", 1);
        let before = sink.snapshot();
        assert_eq!(sink.snapshot().since(&before).to_string(), "-");
        sink.counter("c", 3);
        sink.gauge("g", 2);
        sink.gauge("same", 5);
        sink.histogram("h", 2);
        sink.histogram("h", 3);
        let diff = sink.snapshot().since(&before);
        assert_eq!(diff.counter("c"), 3);
        assert_eq!(diff.gauge("g"), Some(2));
        assert_eq!(diff.gauge("same"), None);
        assert_eq!(diff.samples("h"), [2, 3]);
        assert_eq!(diff.to_string(), "c +3; g =2; h 2 samples");
    }
}
