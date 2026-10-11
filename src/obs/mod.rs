//! Metrics (TS-24, FR-OBS-001).
//!
//! Each database handle reports its metrics to one [`MetricsSink`] that the
//! application supplies. Metric names are `&'static str` constants grouped by
//! the FR-OBS-001 categories in [`names`]. They reuse the Neo4j metric names
//! (Neo4j Operations Manual, "Metrics reference") without Neo4j's
//! `<prefix>.database.<db>.` part, because the sink belongs to one database.
//! The kind and unit of each metric are this crate's and are stated on its
//! constant.

#[cfg(test)]
pub(crate) mod recording;

/// Receives the metrics of one database. Calls may come from any thread.
///
/// Values are integers; each name's unit is stated on its constant.
pub trait MetricsSink: Send + Sync {
    /// Adds `delta` to the counter `name`.
    fn counter(&self, name: &'static str, delta: u64);

    /// Sets the gauge `name` to `value`.
    fn gauge(&self, name: &'static str, value: u64);

    /// Records one sample `value` in the histogram `name`.
    fn histogram(&self, name: &'static str, value: u64);
}

/// Metric names, one module per FR-OBS-001 category. The Bolt category
/// comes with the Bolt endpoints (Sprint 6).
pub mod names {
    /// Transactions.
    pub mod transactions {
        /// Counter: transactions started.
        pub const STARTED: &str = "transaction.started";
        /// Counter: transactions committed.
        pub const COMMITTED: &str = "transaction.committed";
        /// Counter: transactions rolled back.
        pub const ROLLBACKS: &str = "transaction.rollbacks";
        /// Counter: commits that failed and were undone or lost their
        /// group (TS-16). lpg name.
        pub const FAILED_COMMITS: &str = "transaction.failed_commits";
        /// Histogram: latency of one successful commit, from its call to
        /// its return, in nanoseconds. lpg name.
        pub const COMMIT_LATENCY: &str = "transaction.commit_latency";
    }

    /// Transaction log (WAL).
    pub mod transaction_log {
        /// Counter: bytes appended to the log.
        pub const APPENDED_BYTES: &str = "log.appended_bytes";
        /// Counter: log flushes (`sync_data` calls on the log).
        pub const FLUSHES: &str = "log.flushes";
        /// Counter: groups written (TS-09). lpg name.
        pub const GROUPS: &str = "log.groups";
        /// Counter: frames written. lpg name.
        pub const FRAMES: &str = "log.frames";
        /// Histogram: commits per group written.
        pub const APPEND_BATCH_SIZE: &str = "log.append_batch_size";
        /// Counter: segments created. lpg name.
        pub const SEGMENTS_CREATED: &str = "log.segments_created";
        /// Gauge: bytes of all WAL segments. lpg name.
        pub const SIZE: &str = "log.size";
    }

    /// Checkpointing.
    pub mod checkpointing {
        /// Counter: checkpoints completed.
        pub const EVENTS: &str = "check_point.events";
        /// Histogram: duration of one checkpoint, in nanoseconds.
        pub const DURATION: &str = "check_point.duration";
        /// Counter: bytes written by checkpoints.
        pub const FLUSHED_BYTES: &str = "check_point.flushed_bytes";
        /// Counter: pages written by checkpoints. lpg name.
        pub const PAGES_WRITTEN: &str = "check_point.pages_written";
        /// Counter: WAL segments renamed for reuse by checkpoints. lpg name.
        pub const SEGMENTS_RECYCLED: &str = "check_point.segments_recycled";
        /// Counter: WAL segments removed by checkpoints. lpg name.
        pub const SEGMENTS_REMOVED: &str = "check_point.segments_removed";
        /// Counter: opens that found one invalid `CONTROL` slot and used the
        /// other (TS-05; FR-REC-003 exception). The next `CONTROL` write
        /// repairs the slot.
        pub const CONTROL_SLOT_INVALID: &str = "check_point.control_slot_invalid";
    }

    /// Cache (buffer pool).
    pub mod cache {
        /// Counter: page requests served from the pool.
        pub const HITS: &str = "page_cache.hits";
        /// Counter: page requests that read the page from storage.
        pub const PAGE_FAULTS: &str = "page_cache.page_faults";
        /// Counter: frames evicted.
        pub const EVICTIONS: &str = "page_cache.evictions";
        /// Counter: dirty pages written to storage.
        pub const FLUSHES: &str = "page_cache.flushes";
        /// Gauge: bytes reserved from the database's memory accountant, by
        /// the pool and every other user (TS-08).
        pub const ACCOUNTED_BYTES: &str = "page_cache.accounted_bytes";
    }

    /// Query execution (TS-24 Metrics; rmp #51). Emitted by the API when a
    /// statement ends.
    pub mod query_execution {
        /// Counter: statements that succeeded.
        pub const SUCCESS: &str = "db.query.execution.success";
        /// Counter: statements that failed.
        pub const FAILURE: &str = "db.query.execution.failure";
        /// Histogram: latency of one successful statement, from the request
        /// to its last row, in milliseconds.
        pub const LATENCY_MILLIS: &str = "db.query.execution.latency.millis";
    }

    /// Data counts (TS-24 Metrics; rmp #51). Gauges set by
    /// `Database::check_integrity`, which counts every element: the
    /// statistics hold no total count (TS-17).
    pub mod data_counts {
        /// Gauge: nodes in every graph of the database.
        pub const NODE: &str = "neo4j.count.node";
        /// Gauge: edges in every graph of the database.
        pub const RELATIONSHIP: &str = "neo4j.count.relationship";
        /// Gauge: edge-type tokens.
        pub const RELATIONSHIP_TYPES: &str = "neo4j.count.relationship_types";
    }

    /// Indexes (TS-24 Metrics; rmp #51): `index.<kind>.queried` when a
    /// statement's operator reads an index, `index.<kind>.populated` when
    /// an index becomes ONLINE. `<kind>` is Neo4j's `lookup`, `range` or
    /// `point`, or lpg's `single_property` or `composite`.
    pub mod indexes {
        /// Counter: label lookup index reads.
        pub const LOOKUP_QUERIED: &str = "index.lookup.queried";
        /// Counter: range scans of a single-property range index.
        pub const RANGE_QUERIED: &str = "index.range.queried";
        /// Counter: point index reads.
        pub const POINT_QUERIED: &str = "index.point.queried";
        /// Counter: equality seeks of a single-property range index.
        pub const SINGLE_PROPERTY_QUERIED: &str = "index.single_property.queried";
        /// Counter: equality seeks of a composite range index.
        pub const COMPOSITE_QUERIED: &str = "index.composite.queried";
        /// Counter: label lookup indexes populated.
        pub const LOOKUP_POPULATED: &str = "index.lookup.populated";
        /// Counter: single-property range indexes populated.
        pub const RANGE_POPULATED: &str = "index.range.populated";
        /// Counter: point indexes populated.
        pub const POINT_POPULATED: &str = "index.point.populated";
        /// Counter: composite range indexes populated.
        pub const COMPOSITE_POPULATED: &str = "index.composite.populated";
    }

    /// Store size.
    pub mod store_size {
        /// Gauge: bytes of all files of the database.
        pub const FULL: &str = "store.size.full";
        /// Gauge: bytes of the data file.
        pub const DATABASE: &str = "store.size.database";
        /// Gauge: bytes of the data file reserved with `reserve` and not yet
        /// in use (TS-05).
        pub const RESERVED_UNUSED: &str = "store.size.reserved_unused";
    }
}

#[cfg(test)]
mod tests {
    use super::names::*;
    use super::recording::RecordingSink;
    use super::*;

    fn drive(sink: &dyn MetricsSink) {
        sink.counter(transactions::COMMITTED, 2);
        sink.counter(transactions::COMMITTED, 3);
        sink.gauge(store_size::DATABASE, 8192);
        sink.gauge(store_size::DATABASE, 16384);
        sink.histogram(checkpointing::DURATION, 10);
        sink.histogram(checkpointing::DURATION, 30);
    }

    #[test]
    fn every_call_kind_through_dyn_sink() {
        let sink = RecordingSink::default();
        let before = sink.snapshot();
        drive(&sink);
        let diff = sink.snapshot().since(&before);
        assert_eq!(diff.counter(transactions::COMMITTED), 5);
        assert_eq!(diff.gauge(store_size::DATABASE), Some(16384));
        assert_eq!(diff.samples(checkpointing::DURATION), [10, 30]);
        assert_eq!(diff.counter(transactions::ROLLBACKS), 0);
    }

    #[test]
    fn names_are_unique() {
        let all = [
            transactions::STARTED,
            transactions::COMMITTED,
            transactions::ROLLBACKS,
            transactions::FAILED_COMMITS,
            transactions::COMMIT_LATENCY,
            transaction_log::APPENDED_BYTES,
            transaction_log::FLUSHES,
            transaction_log::GROUPS,
            transaction_log::FRAMES,
            transaction_log::APPEND_BATCH_SIZE,
            transaction_log::SEGMENTS_CREATED,
            transaction_log::SIZE,
            checkpointing::EVENTS,
            checkpointing::DURATION,
            checkpointing::FLUSHED_BYTES,
            checkpointing::PAGES_WRITTEN,
            checkpointing::SEGMENTS_RECYCLED,
            checkpointing::SEGMENTS_REMOVED,
            checkpointing::CONTROL_SLOT_INVALID,
            cache::HITS,
            cache::PAGE_FAULTS,
            cache::EVICTIONS,
            cache::FLUSHES,
            cache::ACCOUNTED_BYTES,
            store_size::FULL,
            store_size::DATABASE,
            store_size::RESERVED_UNUSED,
            query_execution::SUCCESS,
            query_execution::FAILURE,
            query_execution::LATENCY_MILLIS,
            data_counts::NODE,
            data_counts::RELATIONSHIP,
            data_counts::RELATIONSHIP_TYPES,
            indexes::LOOKUP_QUERIED,
            indexes::RANGE_QUERIED,
            indexes::POINT_QUERIED,
            indexes::SINGLE_PROPERTY_QUERIED,
            indexes::COMPOSITE_QUERIED,
            indexes::LOOKUP_POPULATED,
            indexes::RANGE_POPULATED,
            indexes::POINT_POPULATED,
            indexes::COMPOSITE_POPULATED,
        ];
        let set: std::collections::BTreeSet<_> = all.iter().collect();
        assert_eq!(set.len(), all.len());
    }
}
