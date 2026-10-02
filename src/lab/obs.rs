//! `lab::obs`: cost of one `counter` call on the recording sink, through
//! `&dyn MetricsSink`, for `perf stat`. `LPG_PERF_N` sets the number of
//! calls (default 10 000 000); `LPG_PERF_N=0` measures the fixed cost to
//! subtract.

use std::hint::black_box;

use crate::obs::MetricsSink;
use crate::obs::names::transactions::COMMITTED;
use crate::obs::recording::RecordingSink;

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn counter_calls() {
    let n: u64 = std::env::var("LPG_PERF_N").map_or(10_000_000, |v| v.parse().expect("number"));
    let recording = RecordingSink::default();
    let sink: &dyn MetricsSink = black_box(&recording);
    for _ in 0..n {
        sink.counter(COMMITTED, black_box(1));
    }
    assert_eq!(recording.snapshot().counter(COMMITTED), n);
}
