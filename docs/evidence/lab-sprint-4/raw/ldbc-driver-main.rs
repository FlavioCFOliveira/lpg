//! Imports the Neo4j-layout LDBC SNB files (manifest.tsv of convert.py) into
//! a new lpg database with lpg's CSV importer, then checks counts and the
//! type of the `id` property. Usage: ldbc-driver <csv-dir> <db-dir> <mode>
//! where mode is `transactional` or `fast`.
use std::{collections::BTreeMap, path::Path, sync::Arc};

use lpg::auth::{Action, Authoriser, BoxFuture, Object};
use lpg::storage::FsStorage;
use lpg::{Database, IdType, ImportMode, ImportRequest, InputFile, Options, Parameters, Value};

struct AllowAll;
impl Authoriser for AllowAll {
    fn authorise<'a>(&'a self, _: &'a str, _: Action, _: &'a Object, _: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

async fn rows(s: &lpg::Session, q: &str) -> Vec<Vec<Value>> {
    let mut st = s.execute(q, Parameters::new()).await.unwrap_or_else(|e| panic!("{q}: {e}"));
    let mut out = Vec::new();
    while let Some(r) = st.next().await.unwrap_or_else(|e| panic!("{q}: {e}")) {
        out.push(r);
    }
    out
}

async fn count(s: &lpg::Session, q: &str) -> i64 {
    match rows(s, q).await.as_slice() {
        [r] => match r.as_slice() {
            [Value::Int(n)] => *n,
            other => panic!("{q}: {other:?}"),
        },
        other => panic!("{q}: {other:?}"),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (csv, db_dir, mode) = (Path::new(&a[1]), Path::new(&a[2]), a[3].as_str());
    let manifest = std::fs::read_to_string(csv.join("manifest.tsv")).unwrap();
    let (mut nodes, mut edges) = (Vec::new(), Vec::new());
    let (mut want_nodes, mut want_edges) = (0i64, 0i64);
    let mut want_label: BTreeMap<String, i64> = BTreeMap::new();
    let mut want_type: BTreeMap<String, i64> = BTreeMap::new();
    for line in manifest.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let n: i64 = f[3].parse().unwrap();
        let mut file = InputFile::new(f[1]);
        if f[0] == "nodes" {
            file.labels = f[2].split(';').map(str::to_owned).collect();
            for l in &file.labels {
                *want_label.entry(l.clone()).or_default() += n;
            }
            want_nodes += n;
            nodes.push(file);
        } else {
            file.edge_type = Some(f[2].to_owned());
            *want_type.entry(f[2].to_owned()).or_default() += n;
            want_edges += n;
            edges.push(file);
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).build().unwrap();
    rt.block_on(async {
        std::fs::create_dir_all(db_dir.join("db")).unwrap();
        std::fs::create_dir_all(db_dir.join("report")).unwrap();
        let db = Database::create(&db_dir.join("db"), Arc::new(FsStorage), 512 << 20, "owner", Options::new(Arc::new(AllowAll)))
            .await
            .unwrap();
        let s = db.session("owner", None, None).await.unwrap();
        let mut r = ImportRequest::csv("/public/lpg", csv, nodes, edges, "report.jsonl");
        r.report_location = db_dir.join("report");
        r.delimiter = b'|';
        r.id_type = IdType::Integer;
        r.mode = if mode == "fast" { ImportMode::FastLoad } else { ImportMode::Transactional };
        let summary = s.import(r).await.unwrap_or_else(|e| panic!("import: {e}"));
        println!("summary nodes={} edges={} invalid={} report_error={:?}", summary.nodes, summary.edges, summary.invalid, summary.report_error);
        let report = std::fs::read_to_string(db_dir.join("report/report.jsonl")).unwrap();
        println!("report lines={}", report.lines().count());
        let got_nodes = count(&s, "MATCH (n) RETURN COUNT(*) AS c").await;
        let got_edges = count(&s, "MATCH ()-[e]->() RETURN COUNT(*) AS c").await;
        println!("files nodes={want_nodes} edges={want_edges}; graph nodes={got_nodes} edges={got_edges}");
        let mut ok = got_nodes == want_nodes && got_edges == want_edges && summary.invalid == 0
            && summary.nodes as i64 == want_nodes && summary.edges as i64 == want_edges;
        for (l, n) in &want_label {
            let g = count(&s, &format!("MATCH (n:{l}) RETURN COUNT(*) AS c")).await;
            println!("label {l} files={n} graph={g}");
            ok &= g == *n;
        }
        for (t, n) in &want_type {
            let g = count(&s, &format!("MATCH ()-[e:{t}]->() RETURN COUNT(*) AS c")).await;
            println!("type {t} files={n} graph={g}");
            ok &= g == *n;
        }
        // Every node has an `id` property and it is an INT64.
        let ids = rows(&s, "MATCH (n) RETURN n.id AS id").await;
        let ints = ids.iter().filter(|r| matches!(r.as_slice(), [Value::Int(_)])).count();
        println!("id values={} int64={}", ids.len(), ints);
        ok &= ints as i64 == want_nodes;
        let p = rows(&s, "MATCH (p:Person) WHERE p.id = 14 RETURN p.firstName AS f, p.creationDate AS c, p.speaks AS s").await;
        println!("person 14: {p:?}");
        let k = count(&s, "MATCH (a:Person)-[:KNOWS]->(b:Person) WHERE a.id = 14 RETURN COUNT(*) AS c").await;
        println!("person 14 KNOWS out-degree={k}");
        db.close().await.ok();
        println!("RESULT {}", if ok { "PASS" } else { "FAIL" });
        assert!(ok);
    });
}
