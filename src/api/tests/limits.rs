//! rmp #49: cancellation, limits, the transaction list and termination
//! (FR-API-007, FR-API-008, FR-API-009, FR-LIM-001, FR-LIM-002,
//! FR-LIM-003, FR-LIM-005, FR-LIM-007, FR-OBS-003, FR-OBS-004).

use std::time::{Duration, Instant};

use super::api::wait_for;
use super::*;
use crate::proc::{ReadContext, ReadProcedure, Rows, Signature};
use crate::query::exec::{CHECK_EVERY, Params, run};
use crate::test_support::{WAIT_BOUND, graph_dir};
use crate::{Limits, TransactionOptions};

/// A procedure that signals once it runs, then checks the statement's
/// status until it fails, and fails with its error: it runs exactly until
/// the statement is cancelled, terminated or timed out.
pub(super) struct Spin {
    started: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl Spin {
    pub(super) fn new() -> (Arc<Spin>, std::sync::mpsc::Receiver<()>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Arc::new(Spin {
                started: std::sync::Mutex::new(Some(tx)),
            }),
            rx,
        )
    }

    pub(super) fn quiet() -> Arc<Spin> {
        Arc::new(Spin {
            started: std::sync::Mutex::new(None),
        })
    }
}

impl ReadProcedure for Spin {
    fn call(&self, cx: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        if let Some(tx) = self.started.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
        let deadline = Instant::now() + WAIT_BOUND;
        loop {
            cx.check()?;
            assert!(Instant::now() < deadline, "the statement was never stopped");
            std::thread::yield_now();
        }
    }
}

fn spin_signature() -> Signature {
    Signature::new("test.spin", vec![], &["one"])
}

/// FR-API-007, with the limit scaled to 200 ms: the second statement
/// fails with 25N14 once 200 ms have passed since the transaction
/// started; the transaction is aborted.
#[test]
fn the_second_statement_fails_after_the_transaction_time_limit() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        db.register_procedure(spin_signature(), Spin::quiet())
            .unwrap();
        let s = session(&db, OWNER).await;
        let t0 = Instant::now();
        let tx = s
            .begin(TransactionOptions {
                limits: Some(Limits {
                    time: Some(Duration::from_millis(200)),
                    ..Limits::default()
                }),
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        tx.execute("INSERT (:Timed)", Parameters::new())
            .await
            .unwrap();
        let mut st = tx
            .execute("CALL test.spin()", Parameters::new())
            .await
            .unwrap();
        let e = st.next().await.unwrap_err();
        assert!(t0.elapsed() >= Duration::from_millis(200));
        assert_eq!(e.code(), "25N14");
        assert!(e.message().contains("timed out"), "{}", e.message());
        drop(st);
        assert_eq!(tx.commit().await.unwrap_err().code(), "25N14");
        assert_eq!(int(&s, "MATCH (n:Timed) RETURN count(*) AS c").await, 0);
        db.close().await.unwrap();
    });
}

/// FR-API-008, FR-API-009: a cancelled statement fails with 25N14, its
/// transaction aborts, and the node it wrote first is never visible;
/// through the session and through the transaction.
#[test]
fn a_cancelled_statement_fails_and_its_write_is_never_visible() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let (spin, started) = Spin::new();
        db.register_procedure(spin_signature(), spin).unwrap();
        let s = session(&db, OWNER).await;
        for via_session in [true, false] {
            let tx = Arc::new(s.begin(TransactionOptions::default()).await.unwrap());
            tx.execute("INSERT (:Cancelled)", Parameters::new())
                .await
                .unwrap();
            let t = tx.clone();
            let running = tokio::spawn(async move {
                let mut st = t.execute("CALL test.spin()", Parameters::new()).await?;
                st.next().await
            });
            wait_for(&started).await;
            assert!(if via_session { s.cancel() } else { tx.cancel() });
            let e = running.await.unwrap().unwrap_err();
            assert_eq!(e.code(), "25N14");
            assert!(e.message().contains("cancelled"));
            let tx = Arc::into_inner(tx).unwrap();
            assert!(tx.commit().await.is_err());
            let other = session(&db, "p2").await;
            assert_eq!(
                int(&other, "MATCH (n:Cancelled) RETURN count(*) AS c").await,
                0
            );
        }
        // No transaction is active: a cancel has nothing to stop.
        assert!(!s.cancel());
        db.close().await.unwrap();
    });
}

/// A cancel during a long expansion, sort and aggregation stops each
/// within `CHECK_EVERY` (K = 1 024) iterations of the loop, counted by the
/// statement: the cancel takes effect right after the third check, and
/// the statement fails at most K iterations later, long before its end.
#[test]
fn a_cancel_stops_expansion_sort_and_aggregation_within_k_iterations() {
    let (_d, e) = crate::tx::tests::db();
    let session = crate::query::exec::tests::support::session();
    {
        let mut tx = e.begin(crate::Isolation::Serializable).unwrap();
        let mut c = run("INSERT (h:Hub)", &mut tx, &session, &Params::new()).unwrap();
        while c.next_batch(&mut tx).unwrap().is_some() {}
        tx.commit().unwrap();
        for chunk in 0..5 {
            let text = format!(
                "MATCH (h:Hub) INSERT {}",
                (0..1000)
                    .map(|i| format!("(h)-[:E]->(:S {{v: {}, g: {}}})", chunk * 1000 + i, i % 7))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let mut tx = e.begin(crate::Isolation::Serializable).unwrap();
            let mut c = run(&text, &mut tx, &session, &Params::new()).unwrap();
            while c.next_batch(&mut tx).unwrap().is_some() {}
            tx.commit().unwrap();
        }
    }
    for q in [
        "MATCH (h:Hub)-[e:E]->(b) RETURN b",
        "MATCH (n:S) RETURN n.v AS v ORDER BY v DESC",
        "MATCH (n:S) RETURN n.g AS g, count(*) AS c",
    ] {
        // The whole run, uncancelled.
        let mut tx = e.begin(crate::Isolation::Serializable).unwrap();
        let mut c = run(q, &mut tx, &session, &Params::new()).unwrap();
        while c.next_batch(&mut tx).unwrap().is_some() {}
        let total = c.iterations();
        tx.commit().unwrap();
        // Cancelled right after the third check.
        let mut tx = e.begin(crate::Isolation::Serializable).unwrap();
        let mut c = run(q, &mut tx, &session, &Params::new()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let at = Arc::new(Mutex::new((None::<usize>, None::<usize>)));
        let (calls2, at2) = (calls.clone(), at.clone());
        c.set_check(Arc::new(move |it| {
            let n = calls2.fetch_add(1, Ordering::SeqCst) + 1;
            let mut a = at2.lock().unwrap();
            match a.0 {
                None if n == 3 => {
                    a.0 = Some(it);
                    Ok(())
                }
                None => Ok(()),
                Some(_) => {
                    a.1 = Some(it);
                    Err(Error::new(crate::gqlstatus::codes::C25N14, "cancelled"))
                }
            }
        }));
        let err = loop {
            match c.next_batch(&mut tx) {
                Ok(Some(_)) => {}
                Ok(None) => panic!("{q}: the cancel was not seen"),
                Err(err) => break err,
            }
        };
        assert_eq!(err.code(), "25N14");
        let (cancel, stop) = *at.lock().unwrap();
        let (cancel, stop) = (cancel.unwrap(), stop.unwrap());
        assert!(stop - cancel <= CHECK_EVERY, "{q}: {cancel} -> {stop}");
        assert!(
            total > stop + CHECK_EVERY,
            "{q}: {total} iterations in all, stopped at {stop}"
        );
        assert!(tx.check_active().is_err(), "the transaction is aborted");
    }
}

/// FR-LIM-001, FR-LIM-003: a statement over the transaction's memory
/// limit fails with 51N73 and aborts its transaction, whose earlier writes
/// are not visible; the accountant never exceeds the cap.
#[test]
fn a_statement_over_the_memory_limit_fails_and_earlier_writes_vanish() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        let text = format!(
            "INSERT {}",
            (0..300)
                .map(|i| format!("(:M {{v: {i}}})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        rows(&s, &text).await;
        let tx = s
            .begin(TransactionOptions {
                limits: Some(Limits {
                    memory: Some(1 << 20),
                    ..Limits::default()
                }),
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        tx.execute("INSERT (:Early), (:Early)", Parameters::new())
            .await
            .unwrap();
        let mut st = tx
            .execute(
                "MATCH (a:M), (b:M) RETURN a.v AS x ORDER BY x",
                Parameters::new(),
            )
            .await
            .unwrap();
        let e = st.next().await.unwrap_err();
        assert_eq!(e.code(), "51N73", "{e}");
        drop(st);
        assert_eq!(
            tx.execute("RETURN 1 AS x", Parameters::new())
                .await
                .unwrap_err()
                .code(),
            "25N05"
        );
        tx.rollback().await.unwrap();
        assert_eq!(int(&s, "MATCH (n:Early) RETURN count(*) AS c").await, 0);
        // The same statement without a limit runs.
        let mut st = s
            .execute(
                "MATCH (a:M), (b:M) RETURN a.v AS x ORDER BY x",
                Parameters::new(),
            )
            .await
            .unwrap();
        assert!(st.next().await.unwrap().is_some());
        drop(st);
        let engine = db.engine_handle();
        let acc = engine.accountant();
        assert!(
            acc.peak() <= acc.cap(),
            "peak {} over cap {}",
            acc.peak(),
            acc.cap()
        );
        db.close().await.unwrap();
    });
}

/// FR-LIM-002 and the second acceptance of FR-API-007: a row limit of 10
/// fails an 11-row query (and a 15-row one) with 51N66; a transaction's
/// row limit of 20 replaces the session's; with no limit all rows return;
/// a byte limit passes at its boundary and fails one byte below.
#[test]
fn row_and_byte_limits_fail_the_statement_that_crosses_them() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        let text = format!(
            "INSERT {}",
            (0..15)
                .map(|i| format!("(:R {{v: {i}}})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        rows(&s, &text).await;
        let eleven = "MATCH (n:R) WHERE n.v < 11 RETURN n.v AS v";
        let fifteen = "MATCH (n:R) RETURN n.v AS v";
        assert_eq!(rows(&s, fifteen).await.len(), 15);
        s.set_limits(Limits {
            rows: Some(10),
            ..Limits::default()
        });
        assert_eq!(error(&s, eleven).await.code(), "51N66");
        assert_eq!(error(&s, fifteen).await.code(), "51N66");
        assert_eq!(
            rows(&s, "MATCH (n:R) WHERE n.v < 10 RETURN n.v AS v")
                .await
                .len(),
            10
        );
        let tx = s
            .begin(TransactionOptions {
                limits: Some(Limits {
                    rows: Some(20),
                    ..Limits::default()
                }),
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        let mut st = tx.execute(fifteen, Parameters::new()).await.unwrap();
        let mut n = 0;
        while st.next().await.unwrap().is_some() {
            n += 1;
        }
        assert_eq!(n, 15);
        drop(st);
        tx.commit().await.unwrap();
        // Bytes: the record-encoding sizes of the values returned.
        let three = "MATCH (n:R) WHERE n.v < 3 RETURN n.v AS v";
        let exact: u64 = (0..3)
            .map(|i| crate::types::record::value_size(&Value::Int(i)))
            .sum();
        s.set_limits(Limits {
            bytes: Some(exact),
            ..Limits::default()
        });
        assert_eq!(rows(&s, three).await.len(), 3);
        s.set_limits(Limits {
            bytes: Some(exact - 1),
            ..Limits::default()
        });
        assert_eq!(error(&s, three).await.code(), "51N66");
        s.set_limits(Limits::default());
        assert_eq!(rows(&s, fifteen).await.len(), 15);
        db.close().await.unwrap();
    });
}

/// FR-LIM-007 through statements: a transaction whose writes would exceed
/// the memory cap fails with 51N72 at the statement that needs the memory,
/// is aborted, and none of its writes is visible. FR-LIM-005: a long
/// reader holds old versions while statements replace large values; the
/// statement that needs memory beyond the cap fails with 51N72, and the
/// reader still reads its snapshot. The accountant never exceeds the cap.
#[test]
fn writes_and_old_versions_at_the_cap_fail_with_51n72() {
    both(|| async {
        const SMALL: u64 = 8 << 20;
        let d = graph_dir();
        let db = create_with(
            d.path(),
            &TestAuth::new(),
            &Arc::new(RecordingSink::default()),
            SMALL,
        )
        .await;
        let s = session(&db, OWNER).await;
        let big = |c: char| Value::string(&c.to_string().repeat(20_000));
        // FR-LIM-007.
        let tx = s.begin(TransactionOptions::default()).await.unwrap();
        let mut failed = None;
        for i in 0..2000 {
            if let Err(e) = tx
                .execute("INSERT (:Large {s: $s})", param("s", big('a')))
                .await
            {
                failed = Some((i, e));
                break;
            }
        }
        let (i, e) = failed.expect("the cap was never reached");
        assert!(i > 0, "the first insert failed: {e}");
        assert_eq!(e.code(), "51N72", "{e}");
        tx.rollback().await.unwrap();
        assert_eq!(int(&s, "MATCH (n:Large) RETURN count(*) AS c").await, 0);
        // FR-LIM-005.
        crate::tx::tests::pause_gc(&db.engine_handle());
        let text = format!(
            "INSERT {}",
            (0..40)
                .map(|i| format!("(:V {{k: {i}, s: $s}})"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        rows_with(&s, &text, param("s", big('0'))).await.unwrap();
        let reader = session(&db, "reader").await;
        let rt = reader
            .begin(TransactionOptions {
                isolation: crate::Isolation::Snapshot,
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        assert_eq!(read_zeros(&rt).await, Some(vec![Value::Int(40)]));
        let mut failure = None;
        for round in 1..200u32 {
            let c = char::from_digit(round % 10, 10).unwrap();
            if let Err(e) = rows_with(&s, "MATCH (n:V) SET n.s = $s", param("s", big(c))).await {
                failure = Some(e);
                break;
            }
        }
        let e = failure.expect("the cap was never reached");
        assert_eq!(e.code(), "51N72", "{e}");
        assert_eq!(
            read_zeros(&rt).await,
            Some(vec![Value::Int(40)]),
            "the reader's snapshot"
        );
        rt.commit().await.unwrap();
        let engine = db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= SMALL, "peak {}", acc.peak());
        db.close().await.unwrap();
    });
}

/// The count of `:V` nodes whose `s` is the first value, in `t`.
async fn read_zeros(t: &crate::Transaction) -> Option<Vec<Value>> {
    let zeros = Value::string(&"0".repeat(20_000));
    let mut st = t
        .execute(
            "MATCH (n:V) WHERE n.s = $s RETURN count(*) AS c",
            param("s", zeros),
        )
        .await
        .unwrap();
    let r = st.next().await.unwrap();
    while st.next().await.unwrap().is_some() {}
    r
}

/// FR-OBS-003: a principal sees its own transactions, with principal,
/// status, elapsed time, current statement and metadata; another's only
/// with the authoriser's consent, in `list_transactions` and in
/// `show_transactions`.
#[test]
fn a_principal_sees_its_own_transactions_and_others_with_consent() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        auth.refuse(Some("p1"), Action::ShowTransaction, None);
        let (p1, p2) = (session(&db, "p1").await, session(&db, "p2").await);
        let meta = param("app", string("a1"));
        let t1 = p1
            .begin(TransactionOptions {
                metadata: meta.clone(),
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        let mut st = t1
            .execute("MATCH (n) RETURN count(*) AS c", Parameters::new())
            .await
            .unwrap();
        while st.next().await.unwrap().is_some() {}
        drop(st);
        let t2 = p2.begin(TransactionOptions::default()).await.unwrap();
        let seen = p1.list_transactions().await.unwrap();
        assert_eq!(seen.len(), 1);
        let me = &seen[0];
        assert_eq!(
            (me.id, me.principal.as_str(), me.status.as_str()),
            (t1.id(), "p1", "Running")
        );
        assert_eq!(
            me.current_query.as_deref(),
            Some("MATCH (n) RETURN count(*) AS c")
        );
        assert_eq!(me.metadata, meta);
        assert!(me.elapsed > Duration::ZERO);
        let rows1 = rows(
            &p1,
            "CALL show_transactions() YIELD username RETURN username",
        )
        .await;
        // Its own explicit transaction is active, so the CALL runs in it.
        assert_eq!(rows1, [[string("p1")]]);
        let mut all: Vec<String> = p2
            .list_transactions()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.principal)
            .collect();
        all.sort();
        assert_eq!(all, ["p1", "p2"]);
        let row = rows(&p2, "CALL show_transactions()").await;
        let p1_row = row
            .iter()
            .find(|r| r[1] == string("p1"))
            .expect("p1's transaction");
        assert_eq!(p1_row[0], string(&format!("transaction-{}", t1.id())));
        assert!(matches!(p1_row[3], Value::ZonedDateTime(_)));
        assert_eq!(p1_row[4], string("Running"));
        assert!(matches!(p1_row[5], Value::DayToSecond(_)));
        assert_eq!(
            p1_row[6],
            Value::Record(vec![(Arc::from("app"), string("a1"))].into())
        );
        t1.rollback().await.unwrap();
        t2.rollback().await.unwrap();
        db.close().await.unwrap();
    });
}

/// FR-OBS-004: a terminated transaction's running statement fails with
/// 25N14 and its writes are discarded. A principal terminates its own
/// transactions without a request; another's needs consent.
#[test]
fn a_terminated_transactions_statement_fails_and_its_writes_vanish() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let (spin, started) = Spin::new();
        db.register_procedure(spin_signature(), spin).unwrap();
        auth.refuse(Some("p3"), Action::TerminateTransaction, None);
        let (p1, p2, p3) = (
            session(&db, "p1").await,
            session(&db, "p2").await,
            session(&db, "p3").await,
        );
        let t1 = Arc::new(p1.begin(TransactionOptions::default()).await.unwrap());
        t1.execute("INSERT (:Doomed)", Parameters::new())
            .await
            .unwrap();
        let t = t1.clone();
        let running = tokio::spawn(async move {
            let mut st = t.execute("CALL test.spin()", Parameters::new()).await?;
            st.next().await
        });
        wait_for(&started).await;
        assert_eq!(
            p3.terminate_transaction(t1.id()).await.unwrap_err().code(),
            "42NFF"
        );
        auth.clear();
        assert!(p2.terminate_transaction(t1.id()).await.unwrap());
        assert_eq!(
            auth.requests(),
            [(
                "p2".to_string(),
                Action::TerminateTransaction,
                Object::Transaction(t1.id()),
                "p1".to_string()
            )]
        );
        let e = running.await.unwrap().unwrap_err();
        assert_eq!(e.code(), "25N14");
        assert!(e.message().contains("terminated"));
        let t1 = Arc::into_inner(t1).unwrap();
        assert!(t1.commit().await.is_err());
        assert_eq!(int(&p2, "MATCH (n:Doomed) RETURN count(*) AS c").await, 0);
        // Its own: no request.
        let own = p1.begin(TransactionOptions::default()).await.unwrap();
        auth.clear();
        assert!(p1.terminate_transaction(own.id()).await.unwrap());
        assert!(auth.requests().is_empty());
        assert_eq!(own.commit().await.unwrap_err().code(), "25N14");
        assert_eq!(
            p1.terminate_transaction(9999).await.unwrap_err().code(),
            "22N11"
        );
        db.close().await.unwrap();
    });
}
