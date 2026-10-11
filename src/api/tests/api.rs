//! rmp #47: the async API (FR-API-001, FR-API-002, FR-API-010, FR-DB-002,
//! FR-DB-008, FR-SES-001 to FR-SES-004, FR-GQL-005, FR-TX-004, BR-TX-001,
//! FR-OPS-001, FR-OPS-002, FR-REC-004, FR-REC-010).

use std::future::Future;
use std::time::{Duration, Instant};

use super::*;
use crate::api::DEFAULT_CLOSE_TIMEOUT;
use crate::proc::{ReadContext, ReadProcedure, Rows, Signature};
use crate::test_support::graph_dir;
use crate::{Isolation, Limits, TransactionOptions};

/// FR-API-001, FR-API-002: a request runs through the API; a graph a
/// committed transaction creates is in the catalog of a new session.
#[test]
fn a_committed_graph_is_listed_by_a_new_session() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, "p1").await;
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        let mut st = tx
            .execute("CREATE GRAPH /public/g ANY", Parameters::new())
            .await
            .unwrap();
        assert!(st.next().await.unwrap().is_none());
        assert_eq!(st.summary().unwrap().counters.catalog_updates, 1);
        tx.commit().await.unwrap();
        let s2 = session(&db, "p2").await;
        let names: Vec<Value> = rows(&s2, "CALL show_catalog()")
            .await
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
        assert!(names.contains(&string("/public/g")), "{names:?}");
        assert_eq!(
            int(&s2, "USE /public/g MATCH (n) RETURN count(*) AS c").await,
            0
        );
        db.close().await.unwrap();
    });
}

/// FR-API-010: the first of 1 000 000 rows comes after at most one batch
/// of each scan run, before the rest is produced; all rows then come.
#[test]
fn the_first_of_a_million_rows_comes_before_the_rest_is_produced() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        let text = format!(
            "INSERT {}",
            (0..1000).map(|_| "(:N)").collect::<Vec<_>>().join(", ")
        );
        rows(&s, &text).await;
        let mut st = s
            .execute("MATCH (a:N), (b:N) RETURN a", Parameters::new())
            .await
            .unwrap();
        assert!(st.next().await.unwrap().is_some());
        // One run of the left scan and two of the right one (rmp #45).
        assert!(st.scan_rows() <= 3000, "{} rows scanned", st.scan_rows());
        let mut n = 1u64;
        while st.next().await.unwrap().is_some() {
            n += 1;
        }
        assert_eq!(n, 1_000_000);
        assert_eq!(st.summary().unwrap().gqlstatus.code(), "00000");
        db.close().await.unwrap();
    });
}

/// FR-DB-002: two databases in one process are independent; closing one
/// leaves the other usable.
#[test]
fn two_databases_are_independent_handles() {
    both(|| async {
        let (d1, d2) = (graph_dir(), graph_dir());
        // FR-DB-004, FR-DB-003 through the API.
        let empty = graph_dir();
        let o = Options::new(TestAuth::new());
        assert_eq!(
            Database::open(empty.path(), no_flush(), CAP, o.clone())
                .await
                .unwrap_err()
                .code(),
            "22N51"
        );
        let a = create(d1.path(), &TestAuth::new()).await;
        assert_eq!(
            Database::create(d1.path(), no_flush(), CAP, OWNER, o)
                .await
                .unwrap_err()
                .code(),
            "42N11"
        );
        let b = create(d2.path(), &TestAuth::new()).await;
        let (sa, sb) = (session(&a, OWNER).await, session(&b, OWNER).await);
        rows(&sa, "INSERT (:A), (:A)").await;
        rows(&sb, "INSERT (:B)").await;
        assert_eq!(int(&sa, "MATCH (n) RETURN count(*) AS c").await, 2);
        assert_eq!(int(&sb, "MATCH (n) RETURN count(*) AS c").await, 1);
        a.close().await.unwrap();
        assert_eq!(error(&sa, "MATCH (n) RETURN n").await.code(), "08N09");
        assert_eq!(int(&sb, "MATCH (n) RETURN count(*) AS c").await, 1);
        b.close().await.unwrap();
    });
}

/// FR-DB-008: the default timeout is 10 s; with 200 ms, close waits that
/// long for a transaction that does not end, then aborts it, and its write
/// is not visible after reopening.
#[test]
fn close_waits_for_the_timeout_then_aborts_the_transaction() {
    assert_eq!(DEFAULT_CLOSE_TIMEOUT, Duration::from_secs(10));
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let s = session(&db, OWNER).await;
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        tx.execute("INSERT (:Kept)", Parameters::new())
            .await
            .unwrap();
        let t0 = Instant::now();
        db.close_with_timeout(Duration::from_millis(200))
            .await
            .unwrap();
        assert!(t0.elapsed() >= Duration::from_millis(200));
        assert_eq!(tx.commit().await.unwrap_err().code(), "08N09");
        drop(s);
        let db = Database::open(d.path(), no_flush(), CAP, Options::new(auth))
            .await
            .unwrap();
        let s = session(&db, OWNER).await;
        assert_eq!(int(&s, "MATCH (n:Kept) RETURN count(*) AS c").await, 0);
        db.close().await.unwrap();
    });
}

/// A procedure that signals once it runs, then blocks until `release`
/// sends, so a test can act while a request of its session runs.
struct Gate {
    started: std::sync::mpsc::SyncSender<()>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl ReadProcedure for Gate {
    fn call(&self, _: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        self.started.send(()).expect("the test waits");
        self.release
            .lock()
            .unwrap()
            .recv_timeout(crate::test_support::WAIT_BOUND)
            .expect("the test releases");
        Ok(Box::new(std::iter::once(Ok(vec![Value::Int(1)]))))
    }
}

/// Waits, yielding to the runtime, until `rx` receives.
pub(super) async fn wait_for(rx: &std::sync::mpsc::Receiver<()>) {
    let deadline = Instant::now() + crate::test_support::WAIT_BOUND;
    loop {
        match rx.try_recv() {
            Ok(()) => return,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                assert!(Instant::now() < deadline, "the pause point was not reached");
                tokio::task::yield_now().await;
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// FR-SES-001 to FR-SES-004: the principal is trusted, not authenticated;
/// the home graph is the caller's, else the default; the time zone is
/// UTC; a request that arrives while another runs fails with 25N03 and
/// the first completes.
#[test]
fn sessions_trust_their_principal_and_refuse_a_concurrent_request() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let s = session(&db, OWNER).await;
        rows(&s, "CREATE GRAPH /public/g1 ANY").await;
        rows(&s, "USE /public/g1 INSERT (:InG1)").await;
        // FR-SES-001: no authentication call.
        let p1 = db.session("p1", None, Some("/public/g1")).await.unwrap();
        assert_eq!(p1.principal(), "p1");
        assert_eq!(
            rows(&p1, "RETURN SESSION_USER AS u").await,
            [[string("p1")]]
        );
        assert_eq!(auth.authentications.load(Ordering::SeqCst), 0);
        // FR-SES-002: home graph g1, else the default graph.
        assert_eq!(int(&p1, "MATCH (n:InG1) RETURN count(*) AS c").await, 1);
        let p2 = session(&db, "p2").await;
        assert_eq!(int(&p2, "MATCH (n:InG1) RETURN count(*) AS c").await, 0);
        // FR-SES-003: UTC.
        match rows(&p1, "RETURN CURRENT_TIMESTAMP AS t").await[0][0].clone() {
            Value::ZonedDateTime(z) => assert_eq!(z.zoned().offset().seconds(), 0),
            other => panic!("{other:?}"),
        }
        // FR-SES-004.
        let (started_tx, started) = std::sync::mpsc::sync_channel(1);
        let (release, release_rx) = std::sync::mpsc::channel();
        db.register_procedure(
            Signature::new("test.gate", vec![], &["one"]),
            Arc::new(Gate {
                started: started_tx,
                release: std::sync::Mutex::new(release_rx),
            }),
        )
        .unwrap();
        let first = p1.clone();
        let running = tokio::spawn(async move {
            let mut st = first.execute("CALL test.gate()", Parameters::new()).await?;
            let row = st.next().await?;
            Ok::<_, Error>((row, st.next().await?))
        });
        wait_for(&started).await;
        assert_eq!(error(&p1, "RETURN 1 AS x").await.code(), "25N03");
        release.send(()).unwrap();
        let (row, end) = running.await.unwrap().unwrap();
        assert_eq!(row, Some(vec![Value::Int(1)]));
        assert_eq!(end, None);
        assert_eq!(rows(&p1, "RETURN 1 AS x").await, [[Value::Int(1)]]);
        db.close().await.unwrap();
    });
}

/// FR-GQL-005: text and API act on one active transaction: 25G01 for a
/// second start, unchanged; 2D000 for an end with none; `SESSION CLOSE`
/// rolls back and later requests fail with 08000; dropping the session
/// rolls back.
#[test]
fn text_and_api_act_on_one_transaction() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        rows(&s, "START TRANSACTION").await;
        rows(&s, "INSERT (:T {v: 1})").await;
        assert_eq!(
            s.begin(TransactionOptions::default())
                .await
                .unwrap_err()
                .code(),
            "25G01"
        );
        assert_eq!(error(&s, "START TRANSACTION").await.code(), "25G01");
        // Unchanged: its write is still there, then committed by text.
        assert_eq!(int(&s, "MATCH (n:T) RETURN count(*) AS c").await, 1);
        rows(&s, "COMMIT").await;
        assert_eq!(error(&s, "COMMIT").await.code(), "2D000");
        assert_eq!(error(&s, "ROLLBACK").await.code(), "2D000");
        // A transaction begun through the API, committed by text.
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        let st = tx
            .execute("INSERT (:T {v: 2})", Parameters::new())
            .await
            .unwrap();
        let summary = st.summary().unwrap();
        assert!(summary.counters.contains_updates());
        assert_eq!(summary.counters.nodes_created, 1);
        assert_eq!(summary.counters.properties_set, 1);
        assert_eq!(summary.gqlstatus.code(), "00001");
        assert_eq!(summary.statement_type, crate::StatementType::Write);
        drop(st);
        rows(&s, "COMMIT").await;
        assert_eq!(
            tx.execute("RETURN 1 AS x", Parameters::new())
                .await
                .unwrap_err()
                .code(),
            "25N05"
        );
        assert_eq!(tx.commit().await.unwrap_err().code(), "2D000");
        let other = session(&db, OWNER).await;
        assert_eq!(int(&other, "MATCH (n:T) RETURN count(*) AS c").await, 2);
        // SESSION CLOSE rolls back.
        rows(&s, "START TRANSACTION INSERT (:T {v: 3})").await;
        rows(&s, "SESSION CLOSE").await;
        assert_eq!(error(&s, "RETURN 1 AS x").await.code(), "08000");
        assert_eq!(int(&other, "MATCH (n:T) RETURN count(*) AS c").await, 2);
        // Dropping the session rolls back.
        let s3 = session(&db, OWNER).await;
        rows(&s3, "START TRANSACTION INSERT (:T {v: 4})").await;
        drop(s3);
        assert_eq!(int(&other, "MATCH (n:T) RETURN count(*) AS c").await, 2);
        db.close().await.unwrap();
    });
}

/// FR-TX-004: write skew under snapshot isolation commits both
/// transactions; under serializable isolation the second fails with
/// 25N11.
#[test]
fn write_skew_commits_under_snapshot_isolation_only() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let setup = session(&db, OWNER).await;
        for (iso, expect_both) in [
            (Isolation::Snapshot, true),
            (Isolation::Serializable, false),
        ] {
            rows(&setup, "MATCH (n:D) SET n.flag = false").await;
            rows(
                &setup,
                "INSERT (:D {k: 1, flag: true}), (:D {k: 2, flag: true})",
            )
            .await;
            let (a, b) = (session(&db, OWNER).await, session(&db, OWNER).await);
            let opts = TransactionOptions {
                isolation: iso,
                ..TransactionOptions::default()
            };
            let ta = a.begin(opts.clone()).await.unwrap();
            let tb = b.begin(opts).await.unwrap();
            for (t, k) in [(&ta, 1), (&tb, 2)] {
                let mut st = t
                    .execute(
                        "MATCH (n:D) WHERE n.flag = true RETURN count(*) AS c",
                        Parameters::new(),
                    )
                    .await
                    .unwrap();
                assert!(st.next().await.unwrap().is_some());
                assert!(st.next().await.unwrap().is_none());
                t.execute(
                    &format!("MATCH (n:D) WHERE n.k = {k} AND n.flag = true SET n.flag = false"),
                    Parameters::new(),
                )
                .await
                .unwrap();
            }
            ta.commit().await.unwrap();
            let rb = tb.commit().await;
            if expect_both {
                rb.unwrap();
                assert_eq!(
                    int(
                        &setup,
                        "MATCH (n:D) WHERE n.flag = true RETURN count(*) AS c"
                    )
                    .await,
                    0
                );
            } else {
                assert_eq!(rb.unwrap_err().code(), "25N11");
            }
        }
        db.close().await.unwrap();
    });
}

/// Counts its calls.
struct Counting(AtomicUsize);

impl ReadProcedure for Counting {
    fn call(&self, _: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(std::iter::once(Ok(vec![Value::Int(1)]))))
    }
}

/// BR-TX-001: a transaction that fails is not retried: its statement ran
/// once.
#[test]
fn a_failed_transaction_runs_once() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let calls = Arc::new(Counting(AtomicUsize::new(0)));
        db.register_procedure(
            Signature::new("test.tally", vec![], &["one"]),
            calls.clone(),
        )
        .unwrap();
        let (a, b) = (session(&db, OWNER).await, session(&db, OWNER).await);
        rows(&a, "INSERT (:C {v: 0})").await;
        let ta = a.begin(TransactionOptions::default()).await.unwrap();
        let tb = b.begin(TransactionOptions::default()).await.unwrap();
        let text = "CALL test.tally() YIELD one MATCH (n:C) SET n.v = one";
        ta.execute(text, Parameters::new()).await.unwrap();
        ta.commit().await.unwrap();
        // `tb`'s snapshot is older than `ta`'s commit: its write fails the
        // write-write rule, and nothing runs it again.
        assert_eq!(
            tb.execute(text, Parameters::new())
                .await
                .unwrap_err()
                .code(),
            "25N11"
        );
        assert_eq!(tb.commit().await.unwrap_err().code(), "25N05");
        assert_eq!(
            calls.0.load(Ordering::SeqCst),
            2,
            "one call per transaction"
        );
        db.close().await.unwrap();
    });
}

/// FR-OPS-001, FR-OPS-002, FR-REC-004, FR-REC-010: an online backup
/// under a concurrent writer restores the state of one commit point; the
/// restored database passes the integrity check; migrating a current
/// database changes nothing.
#[test]
fn backup_restore_integrity_check_and_migration_through_the_api() {
    both(|| async {
        let (d, b, r) = (graph_dir(), graph_dir(), graph_dir());
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        rows(&s, "INSERT (:W {i: 0})").await;
        let writer = {
            let s = session(&db, OWNER).await;
            tokio::spawn(async move {
                for i in 1..=50 {
                    rows_with(&s, "INSERT (:W {i: $i})", param("i", Value::Int(i)))
                        .await
                        .unwrap();
                }
            })
        };
        db.backup(b.path()).await.unwrap();
        writer.await.unwrap();
        assert!(db.check_integrity().await.unwrap().is_ok());
        let restored = Database::restore(
            b.path(),
            r.path(),
            no_flush(),
            CAP,
            Options::new(TestAuth::new()),
        )
        .await
        .unwrap();
        let rs = session(&restored, OWNER).await;
        let got: Vec<i64> = rows(&rs, "MATCH (n:W) RETURN n.i AS i ORDER BY i")
            .await
            .into_iter()
            .map(|r| match r[0] {
                Value::Int(i) => i,
                ref v => panic!("{v:?}"),
            })
            .collect();
        // One commit point: a prefix of the writer's commits.
        assert!(!got.is_empty());
        assert_eq!(got, (0..got.len() as i64).collect::<Vec<_>>());
        assert!(restored.check_integrity().await.unwrap().is_ok());
        restored.close().await.unwrap();
        db.close().await.unwrap();
        assert!(!Database::migrate(d.path(), no_flush()).await.unwrap());
    });
}

/// A `ResultStream` dropped after one row ends its transaction's read:
/// the accountant returns to its earlier level.
#[test]
fn a_dropped_stream_returns_its_memory() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        let text = format!(
            "INSERT {}",
            (0..2000)
                .map(|i| format!("(:S {{v: {i}}})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        rows(&s, &text).await;
        let engine = db.engine_handle();
        let acc = engine.accountant();
        let q = "MATCH (n:S) RETURN n.v AS v ORDER BY v DESC";
        // Warm: the pages the query reads are in the pool.
        assert_eq!(rows(&s, q).await.len(), 2000);
        let before = acc.used();
        let mut st = s
            .execute(
                "MATCH (n:S) RETURN n.v AS v ORDER BY v DESC",
                Parameters::new(),
            )
            .await
            .unwrap();
        assert_eq!(st.next().await.unwrap(), Some(vec![Value::Int(1999)]));
        assert!(acc.used() > before, "the sort holds its rows");
        let id = st.transaction_id();
        drop(st);
        assert_eq!(acc.used(), before);
        assert!(
            s.list_transactions()
                .await
                .unwrap()
                .iter()
                .all(|t| t.id != id)
        );
        db.close().await.unwrap();
    });
}

/// Polls `f` once, so it is built, and drops it: the call is a future.
fn is_async<F: Future>(f: F) {
    drop(f);
}

/// Every public operation that performs I/O or waits is `async`: each
/// call below returns a future, built here and never polled.
#[test]
fn every_io_operation_is_async() {
    current(async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        let p = d.path();
        let o = Options::new(TestAuth::new());
        is_async(Database::create(p, no_flush(), CAP, OWNER, o.clone()));
        is_async(Database::open(p, no_flush(), CAP, o.clone()));
        is_async(Database::restore(p, p, no_flush(), CAP, o));
        is_async(Database::migrate(p, no_flush()));
        is_async(db.session("x", None, None));
        is_async(db.backup(p));
        is_async(db.check_integrity());
        is_async(db.close());
        is_async(db.close_with_timeout(Duration::ZERO));
        is_async(s.execute("RETURN 1 AS x", Parameters::new()));
        is_async(s.profile("RETURN 1 AS x", Parameters::new()));
        is_async(s.explain("RETURN 1 AS x", Parameters::new()));
        is_async(s.begin(TransactionOptions::default()));
        is_async(s.list_transactions());
        is_async(s.terminate_transaction(1));
        is_async(s.create_index(
            "/public/lpg",
            crate::IndexKind::Lookup,
            crate::Entity::Node,
            "",
            &[],
        ));
        is_async(s.create_constraint(
            "/public/lpg",
            crate::ConstraintKind::Exists,
            crate::Entity::Node,
            "L",
            &["p"],
        ));
        is_async(s.drop_index("/public/lpg", 1));
        is_async(s.drop_constraint("/public/lpg", 1));
        is_async(s.create_directory("/d"));
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        is_async(tx.execute("RETURN 1 AS x", Parameters::new()));
        is_async(tx.create_directory("/d"));
        let mut st = s.execute("RETURN 1 AS x", Parameters::new()).await;
        if let Ok(st) = &mut st {
            is_async(st.next());
        }
        drop(st);
        is_async(tx.commit());
        let tx = s.begin(TransactionOptions::default()).await;
        if let Ok(tx) = tx {
            is_async(tx.rollback());
        }
        let _ = Limits::default();
        drop(s);
        db.close().await.unwrap();
    });
}

/// EXPLAIN and PROFILE through the API (FR-OBS-005, FR-OBS-006).
#[test]
fn explain_plans_without_running_and_profile_measures() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        let plan = s.explain("INSERT (:E)", Parameters::new()).await.unwrap();
        assert_eq!(plan.name, "EmptyResult");
        assert_eq!(int(&s, "MATCH (n:E) RETURN count(*) AS c").await, 0);
        rows(&s, "INSERT (:E), (:E)").await;
        let mut st = s
            .profile("MATCH (n:E) RETURN n", Parameters::new())
            .await
            .unwrap();
        let mut n = 0;
        while st.next().await.unwrap().is_some() {
            n += 1;
        }
        let p = st
            .summary()
            .unwrap()
            .profile
            .clone()
            .expect("a profiled plan");
        assert_eq!(p.profile.unwrap().rows, n);
        db.close().await.unwrap();
    });
}
