//! Import and export through the session API (rmp #57, #59): query-result
//! exports, the scope matrix and authorisation.

use super::*;
use crate::{ExportScope, Format, ImportRequest, InputFile, OnInvalid};

fn query(gql: &str) -> ExportScope {
    ExportScope::Query {
        gql: gql.into(),
        parameters: Parameters::new(),
    }
}

/// FR-IMP-003 acceptance: a query result with a node column and an
/// integer column exported to GraphML holds the node and not the integer;
/// exported to CSV and JSON Lines, each row holds both. A query-result
/// export to the native dump and a database export to CSV fail with
/// 22N11 and write no file.
#[test]
fn a_query_result_with_a_node_and_an_integer_column() {
    both(|| async {
        let d = crate::test_support::graph_dir();
        let auth = TestAuth::new();
        let db = create(&d.path().join("db").tap_mkdir(), &auth).await;
        let s = session(&db, OWNER).await;
        rows(&s, "INSERT (:P {name: 'a'}), (:P {name: 'b'})").await;
        let out = d.path().join("out").tap_mkdir();
        let q = "MATCH (n:P) RETURN n, 7 AS k";
        s.export(query(q), Format::GraphMl, &out, &["r.graphml"])
            .await
            .unwrap();
        let g = std::fs::read_to_string(out.join("r.graphml")).unwrap();
        assert_eq!(g.matches("<node ").count(), 2, "{g}");
        assert!(g.contains(">a<") && g.contains(">b<"), "{g}");
        assert!(!g.contains(">7<"), "{g}");
        let summary = s
            .export(query(q), Format::Csv, &out, &["r.csv"])
            .await
            .unwrap();
        assert_eq!(summary.rows, 2);
        let c = std::fs::read_to_string(out.join("r.csv")).unwrap();
        let lines: Vec<&str> = c.lines().collect();
        assert_eq!(lines[0], "n:lpg:NODE,k:long", "{c}");
        assert_eq!(lines.len(), 3);
        assert!(
            lines[1].contains("labels: [\\'P\\']") || lines[1].contains("labels: ['P']"),
            "{c}"
        );
        assert!(lines[1].ends_with(",7") && lines[2].ends_with(",7"), "{c}");
        s.export(query(q), Format::JsonLines, &out, &["r.jsonl"])
            .await
            .unwrap();
        let j = std::fs::read_to_string(out.join("r.jsonl")).unwrap();
        for line in j.lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["k"], 7);
            assert_eq!(v["n"]["$lpg_type"], "NODE");
            assert!(
                v["n"]["$value"]
                    .as_str()
                    .unwrap()
                    .contains("properties: RECORD {name: ")
            );
        }
        assert_eq!(j.lines().count(), 2);
        let err = s
            .export(query(q), Format::Dump, &out, &["r.dump"])
            .await
            .unwrap_err();
        assert_eq!(err.code(), "22N11");
        let err = s
            .export(
                ExportScope::Database,
                Format::Csv,
                &out,
                &["n.csv", "e.csv"],
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "22N11");
        assert!(!out.join("r.dump").exists() && !out.join("n.csv").exists());
        db.close().await.unwrap();
    });
}

trait Mkdir {
    fn tap_mkdir(self) -> Self;
}

impl Mkdir for std::path::PathBuf {
    fn tap_mkdir(self) -> Self {
        std::fs::create_dir_all(&self).unwrap();
        self
    }
}

/// TS-26 Authorisation: an export asks `TRAVERSE` and `READ` on its graph;
/// an import asks `CREATE`, `SET LABEL` and `SET PROPERTY` on its target;
/// a refusal fails with 42NFF and writes nothing.
#[test]
fn export_and_import_ask_their_actions() {
    current(async {
        let d = crate::test_support::graph_dir();
        let auth = TestAuth::new();
        let db = create(&d.path().join("db").tap_mkdir(), &auth).await;
        let s = session(&db, OWNER).await;
        rows(&s, "INSERT (:P {name: 'a'})-[:R]->(:P {name: 'b'})").await;
        let out = d.path().join("out").tap_mkdir();
        auth.clear();
        s.export(
            ExportScope::Graph("/public/lpg".into()),
            Format::JsonLines,
            &out,
            &["g.jsonl"],
        )
        .await
        .unwrap();
        let asked: Vec<Action> = auth.requests().iter().map(|r| r.1).collect();
        assert_eq!(asked, vec![Action::Traverse, Action::Read]);
        rows(&s, "CREATE GRAPH /public/g2 ANY").await;
        auth.clear();
        let mut req = ImportRequest::new(
            Format::JsonLines,
            crate::ImportTarget::Graph("/public/g2".into()),
            &out,
            "g.jsonl",
            "rep",
        );
        req.on_invalid = OnInvalid::Skip;
        let summary = s.import(req.clone()).await.unwrap();
        assert_eq!((summary.nodes, summary.edges, summary.invalid), (2, 1, 0));
        let mut asked: Vec<Action> = auth
            .requests()
            .iter()
            .filter(|r| r.2 == Object::Catalog("/public/g2".into()))
            .map(|r| r.1)
            .collect();
        asked.sort();
        assert_eq!(
            asked,
            vec![Action::Create, Action::SetLabel, Action::SetProperty]
        );
        auth.refuse(None, Action::Create, None);
        req.report = "rep2".into();
        let err = s.import(req).await.unwrap_err();
        assert_eq!(err.code(), "42NFF");
        assert_eq!(int(&s, "USE /public/g2 MATCH (n) RETURN count(n)").await, 2);
        auth.refuse(None, Action::Read, None);
        let err = s
            .export(
                ExportScope::Graph("/public/lpg".into()),
                Format::Csv,
                &out,
                &["n.csv", "e.csv"],
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "42NFF");
        assert!(!out.join("n.csv").exists());
        let _ = InputFile::new("unused");
        db.close().await.unwrap();
    });
}
