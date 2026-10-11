//! SCRATCH COPY ONLY (rmp #60 validation, never in the product tree):
//! the accountant's peak against the cap for each import mode from each
//! format and for each graph-scope export, on one generated graph
//! (property_graph seed 60, 3 000 nodes, 6 000 edges, mixed properties).
//! `lab60_setup` (run first, alone) builds the source database in
//! `$LAB60_DB` and its exports in `$LAB60_FILES`; every other test is one
//! measured operation in a fresh engine (a new accountant) and process, so
//! its accountant peak and its heaptrack peak are that operation's alone.

use super::*;

const NODES: usize = 3_000;
const EDGES: usize = 6_000;
const CAP: u64 = 32 << 20;
const FORMATS: [Format; 4] = [Format::Csv, Format::JsonLines, Format::GraphMl, Format::Dump];

fn var(n: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(n).expect(n))
}

fn names(format: Format) -> Vec<String> {
    match format {
        Format::Csv => vec!["src-nodes.csv".into(), "src-edges.csv".into()],
        Format::JsonLines => vec!["src.jsonl".into()],
        Format::GraphMl => vec!["src.graphml".into()],
        Format::Dump => vec!["src.dump".into()],
    }
}

fn open(cap: u64) -> Engine {
    Engine::open_tuned(
        &var("LAB60_DB"),
        crate::test_support::no_flush(),
        cap,
        std::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
        crate::tx::tests::tuning(),
    )
    .unwrap()
}

#[test]
#[ignore = "lab60 setup"]
fn lab60_setup() {
    let db = var("LAB60_DB");
    std::fs::create_dir_all(&db).unwrap();
    let e = Engine::create_tuned(
        &db,
        crate::test_support::no_flush(),
        256 << 20,
        OWNER,
        std::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
        crate::tx::tests::tuning(),
    )
    .unwrap();
    let (ns, es) = property_graph(60, NODES, EDGES, true);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    for f in FORMATS {
        let dir = var("LAB60_FILES").join(format!("{f:?}"));
        std::fs::create_dir_all(&dir).unwrap();
        export_graph(&e, DEFAULT_GRAPH_ID, f, &dir, "src");
    }
    e.close().unwrap();
}

fn import_peak(format: Format, mode: ImportMode) {
    let dir = var("LAB60_FILES").join(format!("{format:?}"));
    let (_d, e) = crate::tx::tests::db_with_cap(CAP);
    let before = e.accountant().peak();
    let rep = crate::test_support::graph_dir();
    let mut req = request(format, "/public/lpg", &dir, &names(format), "report");
    req.report_location = rep.path().to_path_buf();
    req.mode = mode;
    let s = import(&e, &req).unwrap();
    assert_eq!((s.nodes, s.edges, s.invalid), (NODES as u64, EDGES as u64, 0));
    let peak = e.accountant().peak();
    println!("LAB60 import format={format:?} mode={mode:?} peak_before_import={before} peak={peak} cap={CAP}");
    assert!(peak <= CAP);
}

fn export_peak(format: Format) {
    let e = open(CAP);
    let before = e.accountant().peak();
    let out = crate::test_support::graph_dir();
    let names = export_graph(&e, DEFAULT_GRAPH_ID, format, out.path(), "x");
    let bytes: u64 = names.iter().map(|n| std::fs::metadata(out.path().join(n)).unwrap().len()).sum();
    let peak = e.accountant().peak();
    println!("LAB60 export format={format:?} bytes={bytes} peak_before_export={before} peak={peak} cap={CAP}");
    assert!(peak <= CAP);
    e.close().unwrap();
}

#[test]
#[ignore = "lab60"]
fn lab60_import_csv_transactional() { import_peak(Format::Csv, ImportMode::Transactional) }
#[test]
#[ignore = "lab60"]
fn lab60_import_csv_fast() { import_peak(Format::Csv, ImportMode::FastLoad) }
#[test]
#[ignore = "lab60"]
fn lab60_import_jsonl_transactional() { import_peak(Format::JsonLines, ImportMode::Transactional) }
#[test]
#[ignore = "lab60"]
fn lab60_import_jsonl_fast() { import_peak(Format::JsonLines, ImportMode::FastLoad) }
#[test]
#[ignore = "lab60"]
fn lab60_import_graphml_transactional() { import_peak(Format::GraphMl, ImportMode::Transactional) }
#[test]
#[ignore = "lab60"]
fn lab60_import_graphml_fast() { import_peak(Format::GraphMl, ImportMode::FastLoad) }
#[test]
#[ignore = "lab60"]
fn lab60_import_dump_transactional() { import_peak(Format::Dump, ImportMode::Transactional) }
#[test]
#[ignore = "lab60"]
fn lab60_import_dump_fast() { import_peak(Format::Dump, ImportMode::FastLoad) }
#[test]
#[ignore = "lab60"]
fn lab60_export_csv() { export_peak(Format::Csv) }
#[test]
#[ignore = "lab60"]
fn lab60_export_jsonl() { export_peak(Format::JsonLines) }
#[test]
#[ignore = "lab60"]
fn lab60_export_graphml() { export_peak(Format::GraphMl) }
#[test]
#[ignore = "lab60"]
fn lab60_export_dump() { export_peak(Format::Dump) }
//! SCRATCH COPY ONLY (rmp #60 validation, never in the product tree):
//! behaviour checks for branches the coverage run found no test reaching.

use super::*;
use crate::index::{Entity, PropertyType, SchemaKind, TypeSet};

fn report_set(dir: &Path, name: &str) -> Vec<(String, u64, String)> {
    let mut v: Vec<(String, u64, String)> = report_lines(dir, name)
        .iter()
        .map(|l| {
            (
                l["file"].as_str().unwrap().to_owned(),
                l["record"].as_u64().unwrap(),
                l["gqlstatus"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    v.sort();
    v
}

/// The input of `the_invalid_records_of_a_fast_load_are_reported`, imported
/// by both modes under skip and abort: same counts and report lines.
#[test]
#[ignore = "lab60"]
fn lab60_invalid_records_both_modes() {
    let mut out = Vec::new();
    for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
        let (d, e) = crate::tx::tests::db();
        let dir = files_dir(&d, "x");
        let t = |k, n: &str| e.token(k, n).unwrap();
        let (a, b) = (t(TokenKind::Label, "A"), t(TokenKind::Label, "B"));
        let (uid, p1) = (t(TokenKind::PropertyKey, "uid"), t(TokenKind::PropertyKey, "p1"));
        e.create_constraint(DEFAULT_GRAPH_ID, SchemaKind::Unique, Entity::Node, a, &[uid]).unwrap();
        e.create_constraint(DEFAULT_GRAPH_ID, SchemaKind::Exists, Entity::Node, b, &[uid]).unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Type(TypeSet::of(PropertyType::Integer)),
            Entity::Node,
            a,
            &[p1],
        )
        .unwrap();
        std::fs::write(
            dir.join("n.csv"),
            ":ID,:LABEL,uid:long,p1:lpg:annotated\nn1,A,1,INTEGER:5\nn1,A,2,\nn3,B,,\nn4,A,1,\nn5,A,7,STRING:'x'\nn6,,6,\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("e.csv"),
            ":START_ID,:END_ID,:TYPE\nn1,n9,R\nn1,n3,R\nn1,n4,R\nn1,n5,R\nn1,n6,R\n",
        )
        .unwrap();
        let req = |report: &str, on: OnInvalid| {
            let mut r = ImportRequest::csv(
                "/public/lpg",
                &dir,
                vec![InputFile::new("n.csv")],
                vec![InputFile::new("e.csv")],
                report,
            );
            r.mode = mode;
            r.on_invalid = on;
            r
        };
        let err = import(&e, &req("abort", OnInvalid::Abort)).unwrap_err();
        let abort = report_set(&dir, "abort");
        let s = import(&e, &req("skip", OnInvalid::Skip)).unwrap();
        let skip = report_set(&dir, "skip");
        assert_intact(&e, "skip");
        println!(
            "LAB60B invalid mode={mode:?} abort_code={} abort={abort:?} skip_counts={:?} skip={skip:?}",
            err.code(),
            (s.nodes, s.edges, s.invalid)
        );
        out.push((err.code().to_owned(), abort, (s.nodes, s.edges, s.invalid), skip));
    }
    assert_eq!(out[0], out[1]);
}

/// A unique constraint on an edge type, by both modes under skip and
/// abort: the same edges are rejected with the same code.
#[test]
#[ignore = "lab60"]
fn lab60_edge_uniqueness_both_modes() {
    let case = |mode: ImportMode, on: OnInvalid| {
        let (d, e) = crate::tx::tests::db();
        let dir = files_dir(&d, "x");
        let r = e.token(TokenKind::EdgeType, "R").unwrap();
        let k = e.token(TokenKind::PropertyKey, "k").unwrap();
        e.create_constraint(DEFAULT_GRAPH_ID, SchemaKind::Unique, Entity::Edge, r, &[k]).unwrap();
        std::fs::write(dir.join("n.csv"), ":ID
n1
n2
").unwrap();
        std::fs::write(
            dir.join("e.csv"),
            ":START_ID,:END_ID,:TYPE,k:long
n1,n2,R,1
n2,n1,R,1
n1,n1,R,2
n2,n2,R,2
n1,n2,S,1
",
        )
        .unwrap();
        let mut req = ImportRequest::csv(
            "/public/lpg",
            &dir,
            vec![InputFile::new("n.csv")],
            vec![InputFile::new("e.csv")],
            "rep",
        );
        req.mode = mode;
        req.on_invalid = on;
        let res = import(&e, &req);
        let rep = report_set(&dir, "rep");
        let summary = match &res {
            Ok(s) => format!("ok {:?}", (s.nodes, s.edges, s.invalid)),
            Err(err) => format!("err {}", err.code()),
        };
        let n = content_count(&e);
        assert_intact(&e, "edge-unique");
        println!("LAB60B edge-unique mode={mode:?} on={on:?} {summary} report={rep:?} graph_nodes_edges={n:?}");
        (summary, rep, n)
    };
    for on in [OnInvalid::Skip, OnInvalid::Abort] {
        assert_eq!(case(ImportMode::Transactional, on), case(ImportMode::FastLoad, on));
    }
}

fn content_count(e: &Engine) -> (usize, usize) {
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
    let (mut n, mut m) = (0, 0);
    each_node(e, &mut tx, &g, &mut |_| { n += 1; Ok(()) }).unwrap();
    each_edge(e, &mut tx, &g, &mut |_| { m += 1; Ok(()) }).unwrap();
    tx.abort();
    (n, m)
}

/// A GraphML `<default>` (NetworkX `readwrite/graphml.py`): a node
/// without the key's data gets the default; a node with data keeps its
/// value; both modes.
#[test]
#[ignore = "lab60"]
fn lab60_graphml_defaults_both_modes() {
    for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
        let (d, e) = crate::tx::tests::db();
        let dir = files_dir(&d, "x");
        std::fs::write(
            dir.join("g.graphml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns">
<key id="d0" for="node" attr.name="w" attr.type="double"><default>1.5</default></key>
<key id="d1" for="node" attr.name="uid" attr.type="long"/>
<graph id="G" edgedefault="directed">
<node id="n0"><data key="d1">0</data></node>
<node id="n1"><data key="d1">1</data><data key="d0">2.5</data></node>
</graph></graphml>
"#,
        )
        .unwrap();
        let mut req = request(Format::GraphMl, "/public/lpg", &dir, &["g.graphml".into()], "r");
        req.mode = mode;
        let s = import(&e, &req).unwrap();
        let c = content(&e, DEFAULT_GRAPH_ID);
        let w: Vec<_> = c.nodes.iter().map(|n| (n.0, n.2.iter().find(|p| p.0 == "w").map(|p| p.1.clone()))).collect();
        println!("LAB60B graphml-default mode={mode:?} nodes={} w={w:?}", s.nodes);
        assert_eq!(w, vec![(0, Some(Value::Float(1.5))), (1, Some(Value::Float(2.5)))]);
    }
}

/// The edge record form with properties (the closure of `write_edge` the
/// coverage run never executed): the text holds the properties and reads
/// back to the edge.
#[test]
#[ignore = "lab60"]
fn lab60_edge_form_with_properties() {
    use crate::types::lexical::{Contents, TypeName, lexical_with, parse};
    struct C;
    impl Contents for C {
        fn node(&self, _: u64) -> (Vec<String>, Vec<(String, Value)>) {
            (Vec::new(), Vec::new())
        }
        fn edge(&self, _: u64) -> (String, u64, u64, Vec<(String, Value)>) {
            ("R".into(), 1, 2, vec![("s".into(), Value::string("a")), ("w".into(), Value::Float(1.5))])
        }
    }
    let text = lexical_with(&Value::Edge(7), &C);
    println!("LAB60B edge-form {text}");
    assert_eq!(text, "RECORD {id: 7, type: 'R', start: 1, end: 2, properties: RECORD {s: 'a', w: 1.5}}");
    assert_eq!(parse(&TypeName::of(&Value::Edge(7)), &text), Some(Value::Edge(7)));
}
