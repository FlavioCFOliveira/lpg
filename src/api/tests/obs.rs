//! rmp #51: `tracing` events and metrics (FR-OPS-005, FR-OBS-002,
//! FR-OBS-001, FR-OPS-006). The subscriber is the minimal one below, which
//! records every field and message; no `tracing-subscriber` dependency.

use std::fmt::Debug;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

use super::auth::wait_builds;
use super::*;
use crate::obs::names::*;
use crate::test_support::graph_dir;
use crate::{Entity, IndexKind, TransactionOptions};

/// Everything recorded: one line per span, span record and event, with
/// its target, name and every field.
#[derive(Clone, Default)]
struct Recorder {
    lines: Arc<Mutex<Vec<String>>>,
    next: Arc<AtomicUsize>,
}

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push_str(&format!(" {}={value}", field.name()));
    }
}

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut f = Fields(format!(
            "span {} {}",
            span.metadata().target(),
            span.metadata().name()
        ));
        span.record(&mut f);
        self.lines.lock().unwrap().push(f.0);
        Id::from_u64(self.next.fetch_add(1, Ordering::SeqCst) as u64 + 1)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut f = Fields(format!("record {}", span.into_u64()));
        values.record(&mut f);
        self.lines.lock().unwrap().push(f.0);
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut f = Fields(format!("event {}", event.metadata().target()));
        event.record(&mut f);
        self.lines.lock().unwrap().push(f.0);
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

impl Recorder {
    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}

/// Runs `f` on a `current_thread` runtime with `r` as the default
/// subscriber; the API carries it to its blocking threads.
fn recorded<Fut: Future<Output = ()>>(r: &Recorder, f: Fut) {
    tracing::subscriber::with_default(r.clone(), || current(f));
}

use std::future::Future;

/// FR-OPS-005: a subscriber receives the events of a query: its span with
/// the text and principal, `start` and `success`, and the transaction's
/// `start` and `commit`.
#[test]
fn a_subscriber_receives_events_for_a_query() {
    let r = Recorder::default();
    recorded(&r, async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, "p1").await;
        rows(&s, "INSERT (:Ev)").await;
        db.close().await.unwrap();
    });
    let lines = r.lines();
    let has = |needle: &[&str]| lines.iter().any(|l| needle.iter().all(|n| l.contains(n)));
    assert!(
        has(&[
            "span lpg::query query",
            "query=INSERT (:Ev)",
            "principal=p1"
        ]),
        "{lines:#?}"
    );
    assert!(has(&["event lpg::query", "message=start"]));
    assert!(has(&[
        "event lpg::query",
        "message=success",
        "elapsed_ms=",
        "peak_memory_bytes=",
        "buffer_hits=",
        "buffer_misses="
    ]));
    assert!(has(&[
        "event lpg::transaction",
        "message=start",
        "principal=p1"
    ]));
    assert!(has(&["event lpg::transaction", "message=commit"]));
    assert!(has(&["record", "transaction_id="]));
}

/// FR-OBS-002: with the parameter value "secret-42", the events carry the
/// query text and no recorded field or message holds the value, also when
/// the statement fails; neither do the errors' `Debug` and `Display`.
#[test]
fn no_event_or_error_holds_a_parameter_value() {
    let r = Recorder::default();
    let errors = Arc::new(Mutex::new(Vec::new()));
    let errs = errors.clone();
    recorded(&r, async move {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, "p1").await;
        let p = || param("p", s_("secret-42"));
        rows_with(&s, "INSERT (:Sec {v: $p})", p()).await.unwrap();
        rows_with(&s, "MATCH (n:Sec) WHERE n.v = $p RETURN n.v AS v", p())
            .await
            .unwrap();
        for bad in [
            "RETURN $p + 1 AS x",
            "RETURN -$p AS x",
            "MATCH (n:Sec) WHERE n.v = $p RETURN n.v / 0 AS x",
            "CALL point($p, 1)",
        ] {
            let e = rows_with(&s, bad, p()).await.unwrap_err();
            errs.lock().unwrap().push(format!("{e:?} {e}"));
        }
        let tx = s
            .begin(TransactionOptions {
                metadata: param("app", s_("visible-metadata")),
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        tx.execute("INSERT (:Sec {v: $p})", p()).await.unwrap();
        tx.rollback().await.unwrap();
        db.close().await.unwrap();
    });
    let lines = r.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("query=INSERT (:Sec {v: $p})"))
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("message=fail") && l.contains("gqlstatus="))
    );
    assert!(
        lines.iter().any(|l| l.contains("visible-metadata")),
        "tx_metadata is recorded"
    );
    for l in &lines {
        assert!(!l.contains("secret-42"), "{l}");
    }
    let errors = errors.lock().unwrap();
    assert_eq!(errors.len(), 4);
    for e in errors.iter() {
        assert!(!e.contains("secret-42"), "{e}");
    }
}

fn s_(x: &str) -> Value {
    Value::string(x)
}

/// FR-OBS-001: a recording sink receives at least one metric of each
/// category but Bolt during a workload that covers them; FR-OPS-006:
/// committing transactions produces transaction metrics.
#[test]
fn every_category_but_bolt_reports_a_metric() {
    both(|| async {
        let d = graph_dir();
        let sink = Arc::new(RecordingSink::default());
        let auth = TestAuth::new();
        let db = create_with(d.path(), &auth, &sink, CAP).await;
        let s = session(&db, OWNER).await;
        rows(&s, "INSERT (:M {v: 1})-[:R]->(:M {v: 2})").await;
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        tx.execute("INSERT (:M {v: 3})", Parameters::new())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        tx.rollback().await.unwrap();
        let _ = error(&s, "RETURN 1 / 0 AS x").await;
        s.create_index("/public/lpg", IndexKind::Lookup, Entity::Node, "", &[])
            .await
            .unwrap();
        wait_builds(&db);
        assert_eq!(int(&s, "MATCH (n:M) RETURN count(*) AS c").await, 3);
        db.check_integrity().await.unwrap();
        drop(s);
        db.close().await.unwrap();
        // The whole workload, from the database's creation on.
        let m = sink.snapshot();
        // FR-OPS-006.
        assert!(m.counter(transactions::COMMITTED) >= 2, "{m}");
        assert!(m.counter(transactions::STARTED) >= 4, "{m}");
        assert!(m.counter(transactions::ROLLBACKS) >= 1, "{m}");
        // FR-OBS-001, category by category.
        assert!(m.counter(transaction_log::APPENDED_BYTES) > 0, "log: {m}");
        assert!(m.counter(checkpointing::EVENTS) > 0, "checkpoint: {m}");
        assert!(
            m.counter(cache::HITS) + m.counter(cache::PAGE_FAULTS) > 0,
            "cache: {m}"
        );
        assert!(m.counter(query_execution::SUCCESS) >= 3, "{m}");
        assert!(m.counter(query_execution::FAILURE) >= 1, "{m}");
        assert!(
            !m.samples(query_execution::LATENCY_MILLIS).is_empty(),
            "{m}"
        );
        assert!(
            m.gauge(store_size::DATABASE).is_some() || m.gauge(store_size::FULL).is_some(),
            "store: {m}"
        );
        assert_eq!(m.gauge(data_counts::NODE), Some(3), "{m}");
        assert_eq!(m.gauge(data_counts::RELATIONSHIP), Some(1), "{m}");
        assert_eq!(m.gauge(data_counts::RELATIONSHIP_TYPES), Some(1), "{m}");
        assert_eq!(m.counter(indexes::LOOKUP_POPULATED), 1, "{m}");
        assert!(m.counter(indexes::LOOKUP_QUERIED) >= 1, "{m}");
    });
}
