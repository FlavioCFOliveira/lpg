//! rmp #50: the procedure and function registry, `CALL` and the built-in
//! procedures (FR-API-005, FR-API-006, FR-PROC-001 to FR-PROC-004,
//! FR-LIM-006, FR-CAT-001, FR-CAT-005, FR-IDX-001, FR-OPS-010,
//! FR-TYPE-005).

use std::collections::BTreeSet;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::auth::wait_builds;
use super::*;
use crate::proc::{
    Body, Function, Parameter, ReadContext, ReadProcedure, Routine, Rows, Signature, WriteContext,
    WriteProcedure,
};
use crate::query::exec::{Params, profile};
use crate::test_support::{graph_dir, seed};
use crate::types::{Point, Srid};
use crate::{ConstraintKind, Entity, IndexKind, PlanDescription, TransactionOptions};

/// Returns one row: `(1, 'one')`, and records each call's arguments.
#[derive(Default)]
struct One(Mutex<Vec<Vec<Value>>>);

impl ReadProcedure for One {
    fn call(&self, _: &mut ReadContext<'_>, args: &[Value]) -> Result<Rows, Error> {
        self.0.lock().unwrap().push(args.to_vec());
        Ok(Box::new(std::iter::once(Ok(vec![
            Value::Int(1),
            string("one"),
        ]))))
    }
}

/// `inc(x)`: `x + 1`.
struct Inc;

impl Function for Inc {
    fn call(&self, _: &mut ReadContext<'_>, args: &[Value]) -> Result<Value, Error> {
        match args[0] {
            Value::Int(i) => Ok(Value::Int(i + 1)),
            _ => Ok(Value::Null),
        }
    }
}

/// Inserts a node through its context.
struct Writer;

impl WriteProcedure for Writer {
    fn call(&self, cx: &mut WriteContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        assert_eq!(cx.principal(), OWNER);
        // Its read-only part, through `DerefMut`.
        cx.query("MATCH (n) RETURN count(*) AS c", &Params::new())?;
        cx.execute("INSERT (:ByProc)", &Params::new())?;
        Ok(Box::new(std::iter::once(Ok(vec![Value::Bool(true)]))))
    }
}

/// A read-only procedure that tries to write through nested GQL.
struct Sneaky;

impl ReadProcedure for Sneaky {
    fn call(&self, cx: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        cx.query("INSERT (:Sneaky)", &Params::new())?;
        Ok(Box::new(std::iter::empty()))
    }
}

/// Counts nodes through nested read-only GQL.
struct Counter;

impl ReadProcedure for Counter {
    fn call(&self, cx: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        let r = cx.query("MATCH (n) RETURN count(*) AS c", &Params::new())?;
        Ok(Box::new(r.into_iter().map(Ok)))
    }
}

/// A procedure without result columns.
struct Void;

impl ReadProcedure for Void {
    fn call(&self, _: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        Ok(Box::new(std::iter::empty()))
    }
}

fn register_all(db: &Database, one: &Arc<One>) {
    db.register_procedure(
        Signature::new(
            "test.one",
            vec![
                Parameter::new("a"),
                Parameter::new("b"),
                Parameter::with_default("c", Value::Int(7)),
            ],
            &["n", "name"],
        ),
        one.clone(),
    )
    .unwrap();
    db.register_function("test.inc", vec![Parameter::new("x")], "y", Arc::new(Inc))
        .unwrap();
    db.register_write_procedure(
        Signature::new("test.write", vec![], &["done"]),
        Arc::new(Writer),
    )
    .unwrap();
    db.register_procedure(
        Signature::new("test.sneaky", vec![], &["x"]),
        Arc::new(Sneaky),
    )
    .unwrap();
    db.register_procedure(
        Signature::new("test.tally", vec![], &["c"]),
        Arc::new(Counter),
    )
    .unwrap();
}

/// FR-API-005, FR-API-006, FR-PROC-001: a registered procedure is called
/// once with the request's arguments (a default filling the last) and its
/// row is the result; a function returns one row of its one column; a
/// name with a schema part, an unknown name and a wrong argument count
/// fail.
#[test]
fn registered_procedures_and_functions_are_called_with_call() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let one = Arc::new(One::default());
        register_all(&db, &one);
        let sess = session(&db, OWNER).await;
        let mut st = sess
            .execute("CALL test.one(1, 'x')", Parameters::new())
            .await
            .unwrap();
        assert_eq!(st.columns(), ["n", "name"]);
        assert_eq!(
            st.next().await.unwrap(),
            Some(vec![Value::Int(1), string("one")])
        );
        assert_eq!(st.next().await.unwrap(), None);
        assert_eq!(
            *one.0.lock().unwrap(),
            [vec![Value::Int(1), string("x"), Value::Int(7)]]
        );
        let r = rows(&sess, "CALL test.one(2, 'y') YIELD name AS z RETURN z").await;
        assert_eq!(r, [[string("one")]]);
        let mut st = sess
            .execute("CALL test.inc(41)", Parameters::new())
            .await
            .unwrap();
        assert_eq!(st.columns(), ["y"]);
        assert_eq!(st.next().await.unwrap(), Some(vec![Value::Int(42)]));
        assert_eq!(st.next().await.unwrap(), None);
        // A call per input row.
        rows(&sess, "INSERT (:I {v: 1}), (:I {v: 2})").await;
        let r = rows(
            &sess,
            "MATCH (i:I) CALL test.inc(i.v) YIELD y RETURN y ORDER BY y",
        )
        .await;
        assert_eq!(r, [[Value::Int(2)], [Value::Int(3)]]);
        for (t, code) in [
            ("CALL /public/test.one(1, 2)", "42N08"),
            ("CALL test.none()", "42N08"),
            ("CALL test.one(1)", "22N11"),
            ("CALL test.one(1, 2, 3, 4)", "22N11"),
            ("CALL test.one(1, 2) YIELD nope RETURN nope", "42002"),
        ] {
            assert_eq!(error(&sess, t).await.code(), code, "{t}");
        }
        db.close().await.unwrap();
    });
}

/// FR-PROC-002: a read-write procedure's node is gone after rollback and
/// kept after commit; a read-only procedure that writes fails with 42N02;
/// a read-write procedure in a query fails with 42N02 at bind time;
/// nested reads see the caller's snapshot.
#[test]
fn read_write_procedures_run_in_the_callers_transaction_and_read_only_ones_cannot_write() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        register_all(&db, &Arc::new(One::default()));
        let sess = session(&db, OWNER).await;
        let tx = sess.begin(TransactionOptions::default()).await.unwrap();
        let mut st = tx
            .execute("CALL test.write()", Parameters::new())
            .await
            .unwrap();
        assert_eq!(st.next().await.unwrap(), Some(vec![Value::Bool(true)]));
        assert_eq!(st.next().await.unwrap(), None);
        // The nested read sees the transaction's own write.
        let mut st = tx
            .execute("CALL test.tally()", Parameters::new())
            .await
            .unwrap();
        assert_eq!(st.next().await.unwrap(), Some(vec![Value::Int(1)]));
        drop(st);
        tx.rollback().await.unwrap();
        assert_eq!(int(&sess, "MATCH (n:ByProc) RETURN count(*) AS c").await, 0);
        rows(&sess, "CALL test.write()").await;
        assert_eq!(int(&sess, "MATCH (n:ByProc) RETURN count(*) AS c").await, 1);
        assert_eq!(error(&sess, "CALL test.sneaky()").await.code(), "42N02");
        assert_eq!(int(&sess, "MATCH (n:Sneaky) RETURN count(*) AS c").await, 0);
        assert_eq!(
            error(&sess, "MATCH (n) CALL test.write() YIELD done RETURN done")
                .await
                .code(),
            "42N02"
        );
        // Outside a query, as a data-modifying statement, it may write;
        // as part of a larger statement it needs YIELD (Neo4j).
        rows(&sess, "MATCH (n:ByProc) CALL test.write() YIELD done").await;
        assert_eq!(int(&sess, "MATCH (n:ByProc) RETURN count(*) AS c").await, 2);
        db.close().await.unwrap();
    });
}

/// A `CALL` without `YIELD` that is part of a larger statement fails with
/// 42002 and the cause 42N25 when the procedure has result columns, as
/// Neo4j fails it; a standalone `CALL` without `YIELD` returns every
/// column, and a procedure without result columns needs no `YIELD`
/// (Neo4j `ResolvedCall.resultCheck`; DECISION 386 on rmp #52).
#[test]
fn an_in_query_call_without_yield_fails_and_a_standalone_one_returns_every_column() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        register_all(&db, &Arc::new(One::default()));
        db.register_procedure(Signature::new("test.void", vec![], &[]), Arc::new(Void))
            .unwrap();
        let sess = session(&db, OWNER).await;
        let st = sess
            .execute("CALL test.one(1, 'x')", Parameters::new())
            .await
            .unwrap();
        assert_eq!(st.columns(), ["n", "name"]);
        drop(st);
        for q in [
            "CALL test.one(1, 'x') RETURN n",
            "CALL test.one(1, 'x') RETURN 1 AS k",
            "MATCH (m) CALL test.tally() RETURN m",
            "MATCH (m) CALL test.write()",
        ] {
            let e = error(&sess, q).await;
            assert_eq!(e.code(), "42002", "{q}");
            assert_eq!(e.cause().map(Error::code), Some("42N25"), "{q}");
        }
        assert_eq!(
            rows(&sess, "CALL test.one(1, 'x') YIELD n RETURN n")
                .await
                .len(),
            1
        );
        rows(&sess, "CALL test.void() RETURN 1 AS k").await;
        db.close().await.unwrap();
    });
}

/// FR-PROC-003: registrations are not persisted: after reopen the call
/// fails with 42N08 until the procedure is registered again.
#[test]
fn registrations_are_lost_at_reopen() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let one = Arc::new(One::default());
        register_all(&db, &one);
        let sess = session(&db, OWNER).await;
        assert_eq!(
            rows(&sess, "CALL test.one(1, 2) YIELD n RETURN n")
                .await
                .len(),
            1
        );
        drop(sess);
        db.close().await.unwrap();
        let db = Database::open(d.path(), no_flush(), CAP, Options::new(auth))
            .await
            .unwrap();
        let sess = session(&db, OWNER).await;
        assert_eq!(error(&sess, "CALL test.one(1, 2)").await.code(), "42N08");
        register_all(&db, &one);
        assert_eq!(
            rows(&sess, "CALL test.one(1, 2) YIELD n RETURN n")
                .await
                .len(),
            1
        );
        assert_eq!(
            db.register_procedure(Signature::new("test.one", vec![], &["x"]), one.clone())
                .unwrap_err()
                .code(),
            "22N11"
        );
        db.close().await.unwrap();
    });
}

/// FR-PROC-004: one graph, index, constraint and transaction are each
/// listed with the specified columns; FR-CAT-001, FR-CAT-005: the catalog
/// listing holds a directory, a schema, a graph type and two graphs;
/// FR-IDX-001: the index listing shows five indexes with their kinds.
#[test]
fn the_show_procedures_list_their_objects_with_the_specified_columns() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let sess = session(&db, OWNER).await;
        sess.create_directory("/dir").await.unwrap();
        rows(
            &sess,
            "CREATE SCHEMA /dir/s CREATE GRAPH TYPE /dir/s/t {(:A)} CREATE GRAPH /dir/s/g1 ANY CREATE GRAPH /dir/s/g2 :: /dir/s/t",
        )
        .await;
        let g = "/dir/s/g1";
        for (kind, props) in [
            (IndexKind::SingleProperty, &["x"][..]),
            (IndexKind::Composite, &["x", "y"][..]),
            (IndexKind::Lookup, &[][..]),
            (IndexKind::Range, &["y"][..]),
            (IndexKind::Point, &["loc"][..]),
        ] {
            sess.create_index(g, kind, Entity::Node, "A", props)
                .await
                .unwrap();
        }
        let c = sess
            .create_constraint(g, ConstraintKind::NodeKey, Entity::Node, "K", &["id"])
            .await
            .unwrap();
        wait_builds(&db);
        let columns = |sql: &str| {
            let sess = sess.clone();
            let sql = sql.to_string();
            async move {
                let mut st = sess.execute(&sql, Parameters::new()).await.unwrap();
                let cols = st.columns().to_vec();
                let mut out = Vec::new();
                while let Some(r) = st.next().await.unwrap() {
                    out.push(r);
                }
                (cols, out)
            }
        };
        // show_catalog.
        let (cols, cat) = columns("CALL show_catalog()").await;
        assert_eq!(cols, ["name", "type", "default", "home"]);
        let want = [
            ("/dir", "directory", false, false),
            ("/dir/s", "schema", false, false),
            ("/dir/s/g1", "graph", false, false),
            ("/dir/s/g2", "graph", false, false),
            ("/dir/s/t", "graph type", false, false),
            ("/public", "schema", true, true),
            ("/public/lpg", "graph", true, true),
        ];
        let got: Vec<Vec<Value>> = cat;
        for (n, t, def, home) in want {
            assert!(
                got.contains(&vec![
                    string(n),
                    string(t),
                    Value::Bool(def),
                    Value::Bool(home)
                ]),
                "{n}: {got:?}"
            );
        }
        // show_indexes: five, with their kinds, and the constraint's own.
        let (cols, ix) = columns("CALL show_indexes()").await;
        assert_eq!(
            cols,
            [
                "id",
                "name",
                "state",
                "populationPercent",
                "type",
                "entityType",
                "labelsOrTypes",
                "properties",
                "owningConstraint",
                "graph"
            ]
        );
        let list =
            |xs: &[&str]| Value::List(xs.iter().map(|x| string(x)).collect::<Vec<_>>().into());
        let shape: BTreeSet<(String, String)> = ix
            .iter()
            .filter(|r| r[8] == Value::Null)
            .map(|r| (format!("{:?}", r[4]), format!("{:?}", r[7])))
            .collect();
        let want: BTreeSet<(String, String)> = [
            (string("RANGE"), list(&["x"])),
            (string("RANGE"), list(&["x", "y"])),
            (string("LOOKUP"), Value::Null),
            (string("RANGE"), list(&["y"])),
            (string("POINT"), list(&["loc"])),
        ]
        .iter()
        .map(|(a, b)| (format!("{a:?}"), format!("{b:?}")))
        .collect();
        assert_eq!(shape, want);
        assert_eq!(ix.iter().filter(|r| r[8] == Value::Null).count(), 5);
        assert!(
            ix.iter()
                .all(|r| r[2] == string("ONLINE") && r[3] == Value::Float(100.0))
        );
        assert!(
            ix.iter()
                .all(|r| r[5] == string("NODE") && r[9] == string(g))
        );
        let owned = ix
            .iter()
            .find(|r| r[8] != Value::Null)
            .expect("the key's index");
        assert_eq!(owned[8], string(&format!("constraint_{c}")));
        // show_constraints.
        let (cols, cs) = columns("CALL show_constraints()").await;
        assert_eq!(
            cols,
            [
                "id",
                "name",
                "type",
                "entityType",
                "labelsOrTypes",
                "properties",
                "propertyType",
                "ownedIndex",
                "graph"
            ]
        );
        assert_eq!(cs.len(), 1);
        let r = &cs[0];
        assert_eq!(r[0], Value::Int(c as i64));
        assert_eq!(r[1], string(&format!("constraint_{c}")));
        assert_eq!(r[2], string("NODE_KEY"));
        assert_eq!(
            (&r[3], &r[4], &r[5], &r[6]),
            (&string("NODE"), &list(&["K"]), &list(&["id"]), &Value::Null)
        );
        assert_eq!(r[7], owned[1]);
        assert_eq!(r[8], string(g));
        // show_transactions: the caller's own transaction.
        let tx = sess
            .begin(TransactionOptions {
                metadata: param("k", Value::Int(1)),
                ..TransactionOptions::default()
            })
            .await
            .unwrap();
        let mut st = tx
            .execute("CALL show_transactions()", Parameters::new())
            .await
            .unwrap();
        assert_eq!(
            st.columns(),
            [
                "transactionId",
                "username",
                "currentQuery",
                "startTime",
                "status",
                "elapsedTime",
                "metaData"
            ]
        );
        let row = st.next().await.unwrap().unwrap();
        assert_eq!(row[0], string(&format!("transaction-{}", tx.id())));
        assert_eq!(row[1], string(OWNER));
        assert_eq!(row[2], string("CALL show_transactions()"));
        assert_eq!(
            row[6],
            Value::Record(vec![(Arc::from("k"), Value::Int(1))].into())
        );
        assert_eq!(st.next().await.unwrap(), None);
        drop(st);
        tx.commit().await.unwrap();
        db.close().await.unwrap();
    });
}

/// FR-OPS-010, FR-CAT-005: no built-in routine backs up, restores or
/// creates a directory.
#[test]
fn no_built_in_backs_up_restores_or_creates_a_directory() {
    current(async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let names = db.routines();
        assert_eq!(names.len(), 8, "{names:?}");
        for n in &names {
            let n = n.to_lowercase();
            assert!(
                !n.contains("backup") && !n.contains("restore") && !n.contains("directory"),
                "{n}"
            );
        }
        db.close().await.unwrap();
    });
}

/// Allocates 64 MiB on its own, then returns 1 000 rows.
struct Hog;

impl ReadProcedure for Hog {
    fn call(&self, cx: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        // What a routine charges itself is counted (until its batch is
        // returned); its own 64 MiB below are not.
        cx.reserve(4096)?;
        let mut v = vec![0u8; 64 << 20];
        for i in (0..v.len()).step_by(4096) {
            v[i] = 1;
        }
        let sum = std::hint::black_box(&v)
            .iter()
            .step_by(4096)
            .map(|b| u64::from(*b))
            .sum::<u64>();
        drop(v);
        Ok(Box::new((0..1000).map(move |i| {
            Ok(vec![Value::Int(i), Value::Int(sum as i64)])
        })))
    }
}

fn find<'a>(d: &'a PlanDescription, name: &str) -> Option<&'a PlanDescription> {
    if d.name == name {
        return Some(d);
    }
    d.children.iter().find_map(|c| find(c, name))
}

/// FR-LIM-006: a routine that allocates 64 MiB on its own and returns
/// rows leaves the accountant's peak below 64 MiB (the cap is 256 MiB, so
/// the 64 MiB would fit), while its returned rows are charged; the
/// per-thread heap shows the 64 MiB were allocated.
#[test]
fn a_routines_own_memory_is_not_charged_and_its_rows_are() {
    let (_d, e) = crate::tx::tests::db_with_cap(256 << 20);
    let mut session = crate::query::exec::tests::support::session();
    let procs = crate::proc::Procs::builtins();
    procs
        .registry
        .write()
        .unwrap()
        .register(Routine {
            sig: Signature::new("test.hog", vec![], &["i", "sum"]),
            body: Body::Read(Arc::new(Hog)),
        })
        .unwrap();
    session.procs = procs;
    let mut tx = e.begin(crate::Isolation::Serializable).unwrap();
    let mut c = profile("CALL test.hog()", &mut tx, &session, &Params::new()).unwrap();
    let mut n = 0;
    let info = allocation_counter::measure(|| {
        while let Some(b) = c.next_batch(&mut tx).unwrap() {
            n += b.len();
        }
    });
    tx.commit().unwrap();
    assert_eq!(n, 1000);
    assert!(info.bytes_max >= 64 << 20, "heap peak {}", info.bytes_max);
    let acc = e.accountant();
    assert!(acc.peak() < 64 << 20, "accountant peak {}", acc.peak());
    let d = c.profile().unwrap();
    let call = find(&d, "ProcedureCall").expect("the call");
    let charged = call.profile.unwrap().peak_memory;
    assert!(charged >= 1000 * 2 * 24, "rows charged: {charged}");
}

/// A point in `srid` with random coordinates in a small area.
fn random_point(rng: &mut StdRng, srid: Srid) -> Point {
    let geo = matches!(srid, Srid::Wgs84_2D | Srid::Wgs84_3D);
    let mut c = Vec::new();
    for axis in 0..srid_dims(srid) {
        c.push(if geo && axis < 2 {
            rng.random_range(-0.5..0.5)
        } else {
            rng.random_range(-50.0..50.0)
        });
    }
    Point::new(srid, &c).unwrap()
}

fn srid_dims(s: Srid) -> usize {
    match s {
        Srid::Cartesian2D | Srid::Wgs84_2D => 2,
        _ => 3,
    }
}

/// FR-TYPE-005: for seeded points in the four CRSs, on nodes and edges,
/// the distance-within procedures return the same rows with and without
/// the point index, and with the index read fewer entries.
#[test]
fn the_point_searches_return_the_same_rows_with_and_without_the_index() {
    current(async {
        let sd = seed();
        let mut rng = StdRng::seed_from_u64(sd);
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let sess = session(&db, OWNER).await;
        let srids = [
            Srid::Cartesian2D,
            Srid::Cartesian3D,
            Srid::Wgs84_2D,
            Srid::Wgs84_3D,
        ];
        for i in 0..200 {
            let srid = srids[i % 4];
            let mut p = Parameters::new();
            p.insert(
                "a".into(),
                Value::Point(Box::new(random_point(&mut rng, srid))),
            );
            p.insert(
                "b".into(),
                Value::Point(Box::new(random_point(&mut rng, srid))),
            );
            rows_with(&sess, "INSERT (:P {loc: $a})-[:Q {loc: $b}]->(:Other)", p)
                .await
                .unwrap();
        }
        let mut cases = Vec::new();
        for i in 0..24 {
            let srid = srids[i % 4];
            let geo = matches!(srid, Srid::Wgs84_2D | Srid::Wgs84_3D);
            let r = if geo {
                rng.random_range(1_000.0..60_000.0)
            } else {
                rng.random_range(1.0..40.0)
            };
            cases.push((random_point(&mut rng, srid), r));
        }
        let run = |q: &'static str| {
            let sess = sess.clone();
            let cases = cases.clone();
            async move {
                let mut all = Vec::new();
                let mut entries = 0;
                for (c, r) in &cases {
                    let mut p = Parameters::new();
                    p.insert("c".into(), Value::Point(Box::new(*c)));
                    p.insert("r".into(), Value::Float(*r));
                    let mut st = sess.profile(q, p).await.unwrap();
                    let mut got = BTreeSet::new();
                    while let Some(row) = st.next().await.unwrap() {
                        got.insert(format!("{:?}", row));
                    }
                    let d = st.summary().unwrap().profile.clone().unwrap();
                    entries += find(&d, "ProcedureCall")
                        .unwrap()
                        .profile
                        .unwrap()
                        .entries_read;
                    all.push(got);
                }
                (all, entries)
            }
        };
        let nodes = "CALL point.nodesWithinDistance('P', 'loc', $c, $r) YIELD node, distance RETURN node, distance";
        let edges = "CALL point.edgesWithinDistance('Q', 'loc', $c, $r) YIELD edge, distance RETURN edge, distance";
        let (n0, ne0) = run(nodes).await;
        let (e0, ee0) = run(edges).await;
        assert!(n0.iter().any(|s| !s.is_empty()), "seed {sd}: no node found");
        assert!(e0.iter().any(|s| !s.is_empty()), "seed {sd}: no edge found");
        sess.create_index("/public/lpg", IndexKind::Point, Entity::Node, "P", &["loc"])
            .await
            .unwrap();
        sess.create_index(
            "/public/lpg",
            IndexKind::Point,
            Entity::Relationship,
            "Q",
            &["loc"],
        )
        .await
        .unwrap();
        wait_builds(&db);
        let (n1, ne1) = run(nodes).await;
        let (e1, ee1) = run(edges).await;
        assert_eq!(n0, n1, "seed {sd}");
        assert_eq!(e0, e1, "seed {sd}");
        assert!(
            ne1 < ne0 && ee1 < ee0,
            "seed {sd}: entries {ne0} -> {ne1}, {ee0} -> {ee1}"
        );
        db.close().await.unwrap();
    });
}

/// The point constructor and distance (TS-24 Point procedures).
#[test]
fn point_and_distance_follow_their_specification() {
    current(async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let sess = session(&db, OWNER).await;
        let pt = |q: &'static str| {
            let sess = sess.clone();
            async move { rows(&sess, q).await[0][0].clone() }
        };
        let p2 = Value::Point(Box::new(
            Point::new(Srid::Cartesian2D, &[1.0, 2.0]).unwrap(),
        ));
        assert_eq!(pt("CALL point(1, 2e0)").await, p2);
        let p3 = Value::Point(Box::new(
            Point::new(Srid::Cartesian3D, &[1.0, 2.0, 3.0]).unwrap(),
        ));
        assert_eq!(pt("CALL point(1, 2, 3)").await, p3);
        let w = Value::Point(Box::new(Point::new(Srid::Wgs84_2D, &[10.0, 20.0]).unwrap()));
        assert_eq!(pt("CALL point(10, 20, NULL, 4326)").await, w);
        assert_eq!(pt("CALL point(NULL, 2)").await, Value::Null);
        assert_eq!(pt("CALL point.distance(NULL, NULL)").await, Value::Null);
        let mut p = Parameters::new();
        p.insert("a".into(), p2.clone());
        p.insert(
            "b".into(),
            Value::Point(Box::new(
                Point::new(Srid::Cartesian2D, &[4.0, 6.0]).unwrap(),
            )),
        );
        p.insert("w".into(), w);
        let r = rows_with(
            &sess,
            "CALL point.distance($a, $b) YIELD distance RETURN distance",
            p.clone(),
        )
        .await
        .unwrap();
        assert_eq!(r, [[Value::Float(5.0)]]);
        let r = rows_with(&sess, "CALL point.distance($a, $w)", p)
            .await
            .unwrap();
        assert_eq!(r, [[Value::Null]]);
        for t in [
            "CALL point(1, 2, 3, 4326)",
            "CALL point(1, 2, NULL, 4979)",
            "CALL point(1, 2, NULL, 1)",
        ] {
            assert_eq!(error(&sess, t).await.code(), "22N11", "{t}");
        }
        assert_eq!(error(&sess, "CALL point('a', 2)").await.code(), "22G03");
        db.close().await.unwrap();
    });
}
