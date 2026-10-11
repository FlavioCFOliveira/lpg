//! Imports the plan Appendix A Neo4j CSV probe (nodes.csv, edges.csv in the
//! given directory) with lpg's CSV importer, both modes, and prints what the
//! graph holds. Usage: probe <csv-dir> <db-base>
use std::{path::Path, sync::Arc};
use lpg::auth::{Action, Authoriser, BoxFuture, Object};
use lpg::storage::FsStorage;
use lpg::{Database, ImportMode, ImportRequest, InputFile, Options, Parameters};

struct AllowAll;
impl Authoriser for AllowAll {
    fn authorise<'a>(&'a self, _: &'a str, _: Action, _: &'a Object, _: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (csv, base) = (Path::new(&a[1]), Path::new(&a[2]));
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).build().unwrap();
    rt.block_on(async {
        for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
            let dir = base.join(format!("{mode:?}"));
            std::fs::create_dir_all(dir.join("db")).unwrap();
            let db = Database::create(&dir.join("db"), Arc::new(FsStorage), 64 << 20, "o", Options::new(Arc::new(AllowAll))).await.unwrap();
            let s = db.session("o", None, None).await.unwrap();
            let mut r = ImportRequest::csv("/public/lpg", csv, vec![InputFile::new("nodes.csv")], vec![InputFile::new("edges.csv")], "report.jsonl");
            r.report_location = dir.clone();
            r.mode = mode;
            match s.import(r).await {
                Ok(sum) => println!("{mode:?}: nodes={} edges={} invalid={}", sum.nodes, sum.edges, sum.invalid),
                Err(e) => println!("{mode:?}: import error {} {e}", e.code()),
            }
            if let Ok(rep) = std::fs::read_to_string(dir.join("report.jsonl")) { print!("{rep}"); }
            let mut st = s.execute("MATCH (n:A) RETURN n.loc AS loc, n.loc3 AS loc3, n.d AS d ORDER BY d", Parameters::new()).await.unwrap();
            while let Some(row) = st.next().await.unwrap() { println!("  {row:?}"); }
            let mut st = s.execute("MATCH (b:B)-[e:R]->(:A) RETURN COUNT(*) AS c", Parameters::new()).await.unwrap();
            while let Some(row) = st.next().await.unwrap() { println!("  edges {row:?}"); }
            db.close().await.ok();
        }
    });
}
