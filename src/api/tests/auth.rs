//! rmp #48: authorisation and the index, constraint and directory API
//! (FR-AUTH-001, FR-AUTH-002, FR-DB-005, BR-AUTH-001, FR-API-004,
//! FR-IDX-001, FR-CAT-005, FR-OPS-008).

use std::collections::BTreeSet;

use super::*;
use crate::test_support::graph_dir;
use crate::{ConstraintKind, Entity, IndexKind, PropertyType, TypeSpec};

/// Waits for every index build the database runs.
pub(super) fn wait_builds(db: &Database) {
    let e = db.engine_handle();
    let ids: Vec<u64> = e.core().builds.threads().iter().map(|(i, _)| *i).collect();
    for i in ids {
        e.wait_index(i).unwrap();
    }
}

/// FR-AUTH-001: an authoriser that refuses "create node" on g1 for p1
/// fails p1's insert with 42NFF, and no node exists.
#[test]
fn a_refused_create_fails_the_insert_and_creates_nothing() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let owner = session(&db, OWNER).await;
        rows(&owner, "CREATE GRAPH /public/g1 ANY").await;
        auth.refuse(
            Some("p1"),
            Action::Create,
            Some(Object::Catalog("/public/g1".into())),
        );
        let p1 = session(&db, "p1").await;
        let e = error(&p1, "USE /public/g1 INSERT (:N)").await;
        assert_eq!(e.code(), "42NFF");
        assert_eq!(
            int(&owner, "USE /public/g1 MATCH (n) RETURN count(*) AS c").await,
            0
        );
        // Another principal, and another graph, are not refused.
        rows(&p1, "INSERT (:N)").await;
        assert_eq!(int(&owner, "MATCH (n:N) RETURN count(*) AS c").await, 1);
        db.close().await.unwrap();
    });
}

/// FR-AUTH-002, FR-DB-005: `create` makes no request; requests on the
/// default schema and graph name the `create` owner; a graph p1 creates
/// is named with owner p1 in every later request.
#[test]
fn create_asks_nothing_and_every_request_names_the_owner() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        assert!(auth.requests().is_empty(), "{:?}", auth.requests());
        let p2 = session(&db, "p2").await;
        rows(&p2, "MATCH (n) RETURN n").await;
        let on_default: Vec<_> = auth
            .requests()
            .into_iter()
            .filter(|r| r.2 == Object::Catalog("/public/lpg".into()))
            .collect();
        assert!(!on_default.is_empty());
        assert!(on_default.iter().all(|r| r.3 == OWNER), "{on_default:?}");
        let p1 = session(&db, "p1").await;
        rows(&p1, "CREATE GRAPH /public/g ANY").await;
        let create = auth
            .requests()
            .into_iter()
            .find(|r| r.1 == Action::CreateGraph)
            .unwrap();
        assert_eq!(create.2, Object::Catalog("/public/g".into()));
        assert_eq!(create.3, OWNER, "the owner of the parent schema");
        auth.clear();
        rows(&p2, "USE /public/g INSERT (:X)").await;
        rows(&p2, "USE /public/g MATCH (x:X) SET x.v = 1").await;
        p2.create_index("/public/g", IndexKind::Lookup, Entity::Node, "", &[])
            .await
            .unwrap();
        let on_g: Vec<_> = auth
            .requests()
            .into_iter()
            .filter(|r| r.2 == Object::Catalog("/public/g".into()))
            .collect();
        assert!(on_g.len() >= 4, "{on_g:?}");
        assert!(on_g.iter().all(|r| r.3 == "p1"), "{on_g:?}");
        db.close().await.unwrap();
    });
}

/// A counting authoriser receives each `(action, object)` pair once per
/// statement.
#[test]
fn each_pair_is_asked_once_per_statement() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let s = session(&db, "p").await;
        rows(&s, "INSERT (:A {v: 1}), (:A {v: 2})").await;
        auth.clear();
        rows(
            &s,
            "MATCH (a:A), (b:A) WHERE a.v < b.v SET a.w = b.v REMOVE b.x INSERT (:Z)",
        )
        .await;
        let reqs = auth.requests();
        let pairs: Vec<(Action, Object)> = reqs.iter().map(|r| (r.1, r.2.clone())).collect();
        let distinct: BTreeSet<_> = pairs.iter().cloned().collect();
        assert_eq!(pairs.len(), distinct.len(), "a pair asked twice: {reqs:?}");
        let g = Object::Catalog("/public/lpg".into());
        let want: BTreeSet<_> = [
            (Action::Traverse, g.clone()),
            (Action::Read, g.clone()),
            (Action::SetProperty, g.clone()),
            (Action::Create, g),
        ]
        .into_iter()
        .collect();
        assert_eq!(distinct, want);
        db.close().await.unwrap();
    });
}

/// FR-API-004, FR-IDX-001 (creation), FR-IDX-002: one index of each of
/// the five kinds and one constraint of each of the four kinds created
/// through the API exist; each is authorised on its graph; each drop
/// removes it.
#[test]
fn every_index_and_constraint_kind_is_managed_through_the_api() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let s = session(&db, OWNER).await;
        let g = "/public/lpg";
        let mut idx = Vec::new();
        for (kind, label, props) in [
            (IndexKind::SingleProperty, "A", &["x"][..]),
            (IndexKind::Composite, "A", &["x", "y"][..]),
            (IndexKind::Lookup, "", &[][..]),
            (IndexKind::Range, "A", &["y"][..]),
            (IndexKind::Point, "A", &["loc"][..]),
        ] {
            idx.push(
                s.create_index(g, kind, Entity::Node, label, props)
                    .await
                    .unwrap(),
            );
        }
        let mut con = Vec::new();
        for kind in [
            ConstraintKind::Unique,
            ConstraintKind::Exists,
            ConstraintKind::PropertyType(vec![
                TypeSpec::Of(PropertyType::Integer),
                TypeSpec::ListOf(PropertyType::String),
            ]),
            ConstraintKind::NodeKey,
        ] {
            con.push(
                s.create_constraint(g, kind, Entity::Node, "C", &["k"])
                    .await
                    .unwrap(),
            );
        }
        wait_builds(&db);
        let listed: Vec<Value> = rows(&s, "CALL show_indexes() YIELD id RETURN id")
            .await
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
        for i in &idx {
            assert!(listed.contains(&Value::Int(*i as i64)), "{listed:?}");
        }
        assert_eq!(rows(&s, "CALL show_constraints()").await.len(), 4);
        let asked: Vec<Action> = auth.requests().iter().map(|r| r.1).collect();
        assert_eq!(
            asked.iter().filter(|a| **a == Action::CreateIndex).count(),
            5
        );
        assert_eq!(
            asked
                .iter()
                .filter(|a| **a == Action::CreateConstraint)
                .count(),
            4
        );
        // A wrong number of properties, an empty type union.
        assert_eq!(
            s.create_index(g, IndexKind::Composite, Entity::Node, "A", &["x"])
                .await
                .unwrap_err()
                .code(),
            "22N11"
        );
        assert_eq!(
            s.create_constraint(
                g,
                ConstraintKind::PropertyType(vec![]),
                Entity::Node,
                "C",
                &["k"]
            )
            .await
            .unwrap_err()
            .code(),
            "22N11"
        );
        s.drop_index(g, idx[0]).await.unwrap();
        assert_eq!(s.drop_index(g, idx[0]).await.unwrap_err().code(), "22N69");
        assert_eq!(s.drop_index(g, con[0]).await.unwrap_err().code(), "22N69");
        s.drop_constraint(g, con[0]).await.unwrap();
        assert_eq!(
            s.drop_constraint(g, con[0]).await.unwrap_err().code(),
            "22N64"
        );
        assert_eq!(rows(&s, "CALL show_constraints()").await.len(), 3);
        // A refused principal creates nothing.
        auth.refuse(Some("p9"), Action::CreateIndex, None);
        let p9 = session(&db, "p9").await;
        assert_eq!(
            p9.create_index(g, IndexKind::Lookup, Entity::Relationship, "", &[])
                .await
                .unwrap_err()
                .code(),
            "42NFF"
        );
        db.close().await.unwrap();
    });
}

/// FR-CAT-005: a directory created through the API, in a transaction or
/// on its own, is listed by the catalog with its owner; a rolled-back one
/// is not.
#[test]
fn a_directory_is_created_through_the_api_and_listed() {
    both(|| async {
        let d = graph_dir();
        let auth = TestAuth::new();
        let db = create(d.path(), &auth).await;
        let s = session(&db, "p1").await;
        s.create_directory("/d").await.unwrap();
        let tx = s.begin(crate::TransactionOptions::default()).await.unwrap();
        tx.create_directory("/d/e").await.unwrap();
        tx.rollback().await.unwrap();
        let tx = s.begin(crate::TransactionOptions::default()).await.unwrap();
        tx.create_directory("/d/f").await.unwrap();
        tx.commit().await.unwrap();
        let cat = rows(&s, "CALL show_catalog()").await;
        let dirs: Vec<Value> = cat
            .iter()
            .filter(|r| r[1] == string("directory"))
            .map(|r| r[0].clone())
            .collect();
        assert_eq!(dirs, [string("/d"), string("/d/f")]);
        assert!(
            auth.requests()
                .iter()
                .any(|r| r.1 == Action::CreateDirectory && r.2 == Object::Catalog("/d".into()))
        );
        assert_eq!(s.create_directory("/d").await.unwrap_err().code(), "42N11");
        let e = db.engine_handle();
        let mut t = e.begin(crate::Isolation::Snapshot).unwrap();
        assert_eq!(t.resolve(&["d"]).unwrap().unwrap().owner, "p1");
        t.abort();
        db.close().await.unwrap();
    });
}

/// BR-AUTH-001: no statement grants or revokes permissions.
#[test]
fn grant_and_revoke_are_syntax_errors() {
    both(|| async {
        let d = graph_dir();
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        for t in [
            "GRANT TRAVERSE ON GRAPH lpg TO p1",
            "REVOKE TRAVERSE ON GRAPH lpg FROM p1",
            "GRANT ROLE r TO p1",
        ] {
            assert_eq!(error(&s, t).await.code(), "42001", "{t}");
        }
        db.close().await.unwrap();
    });
}

/// FR-OPS-008: a database with two graphs, an index and a constraint,
/// backed up and restored through the API, holds all four.
#[test]
fn a_restored_backup_holds_graphs_index_and_constraint() {
    both(|| async {
        let (d, b, r) = (graph_dir(), graph_dir(), graph_dir());
        let db = create(d.path(), &TestAuth::new()).await;
        let s = session(&db, OWNER).await;
        rows(
            &s,
            "CREATE GRAPH /public/g1 ANY CREATE GRAPH /public/g2 ANY",
        )
        .await;
        let i = s
            .create_index("/public/g1", IndexKind::Range, Entity::Node, "L", &["p"])
            .await
            .unwrap();
        let c = s
            .create_constraint(
                "/public/g2",
                ConstraintKind::Unique,
                Entity::Node,
                "M",
                &["q"],
            )
            .await
            .unwrap();
        wait_builds(&db);
        db.backup(b.path()).await.unwrap();
        db.close().await.unwrap();
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
        let names: Vec<Value> = rows(&rs, "CALL show_catalog() YIELD name RETURN name")
            .await
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
        assert!(names.contains(&string("/public/g1")) && names.contains(&string("/public/g2")));
        let ix = rows(&rs, "CALL show_indexes() YIELD id, graph RETURN id, graph").await;
        assert!(
            ix.contains(&vec![Value::Int(i as i64), string("/public/g1")]),
            "{ix:?}"
        );
        let cs = rows(
            &rs,
            "CALL show_constraints() YIELD id, graph RETURN id, graph",
        )
        .await;
        assert_eq!(cs, [[Value::Int(c as i64), string("/public/g2")]]);
        restored.close().await.unwrap();
    });
}
