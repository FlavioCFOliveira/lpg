//! The import and export framework (rmp #57): the native dump of a
//! database and of a graph, the format and scope matrix, abort and skip
//! with the report, damaged dumps, the memory cap and ENOSPC during an
//! export.

use super::*;
use crate::index::{Entity, SchemaKind, State};
use crate::storage::Storage;
use crate::storage::fault::{Fault, FaultStorage, Kind};

/// rmp #57 acceptance: a database with two graphs, an index and a
/// constraint, exported to a native dump and imported into a new
/// database, has the same nodes, edges, labels and property values per
/// graph, new element identifiers, and the definitions, ONLINE and built
/// (TS-26 item D); the integrity check, which compares each index with
/// its data, passes.
#[test]
fn a_database_with_two_graphs_an_index_and_a_constraint_round_trips_through_a_dump() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let g2 = new_graph(&e, "second");
    let (ns, es) = property_graph(11, 15, 25, true);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    let (ns2, es2) = property_graph(12, 8, 10, false);
    load_generated(&e, g2, &ns2, &es2);
    let label = e.token(TokenKind::Label, "A").unwrap();
    let uid = e.token(TokenKind::PropertyKey, "uid").unwrap();
    let ix = e
        .create_index(
            DEFAULT_GRAPH_ID,
            SchemaKind::Range,
            Entity::Node,
            label,
            &[uid],
        )
        .unwrap();
    e.wait_index(ix).unwrap();
    e.create_constraint(g2, SchemaKind::Unique, Entity::Node, label, &[uid])
        .unwrap();
    let want = (content(&e, DEFAULT_GRAPH_ID), content(&e, g2));
    export::stored(
        &e,
        Stored::Database,
        Format::Dump,
        &dir,
        &["db.dump"],
        &allow,
    )
    .unwrap();

    let (d2, e2) = crate::tx::tests::db();
    std::fs::copy(dir.join("db.dump"), d2.path().join("db.dump")).unwrap();
    let req = ImportRequest::new(
        Format::Dump,
        ImportTarget::Database,
        d2.path(),
        "db.dump",
        "r",
    );
    let s = import(&e2, &req).unwrap();
    assert_eq!((s.nodes, s.edges, s.invalid), (23, 35, 0));
    let second = crate::ops::bulk::import::graph_at(&e2, "/public/second")
        .unwrap()
        .0;
    // Another database: its identifiers are its own (BR-TYPE-002).
    assert_same_in(
        &want.0,
        &content(&e2, DEFAULT_GRAPH_ID),
        "default graph",
        false,
    );
    assert_same_in(&want.1, &content(&e2, second), "second graph", false);
    let defs: Vec<crate::index::SchemaDef> = e2.core().catalog.schema().values().cloned().collect();
    assert!(defs.iter().any(|d| d.graph == DEFAULT_GRAPH_ID
        && d.kind == SchemaKind::Range
        && d.link == 0
        && d.state == State::Online));
    assert!(
        defs.iter()
            .any(|d| d.graph == second && d.kind == SchemaKind::Unique)
    );
    assert!(
        defs.iter()
            .any(|d| d.graph == second && d.kind == SchemaKind::Range && d.link != 0)
    );
    assert_intact(&e2, "imported database");
    // The constraint is enforced on the imported graph.
    let mut tx = e2.begin(Isolation::Serializable).unwrap();
    let g = tx.graph(second).unwrap();
    let a2 = e2.token(TokenKind::Label, "A").unwrap();
    let u2 = e2.token(TokenKind::PropertyKey, "uid").unwrap();
    let taken = want
        .1
        .nodes
        .iter()
        .find(|n| n.1.contains(&"A".to_owned()))
        .map(|n| n.0);
    if let Some(u) = taken {
        assert!(tx.create_node(&g, &[a2], &[(u2, Value::Int(u))]).is_err());
    }
    tx.abort();
}

/// FR-IMP-003 acceptance: a database dump into a database whose catalog
/// holds another graph fails with 51N41 and changes nothing.
#[test]
fn a_database_dump_into_a_catalog_with_another_object_fails_and_changes_nothing() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    export::stored(
        &e,
        Stored::Database,
        Format::Dump,
        &dir,
        &["db.dump"],
        &allow,
    )
    .unwrap();
    new_graph(&e, "other");
    let before = e.core().catalog.objects().clone();
    let req = ImportRequest::new(Format::Dump, ImportTarget::Database, &dir, "db.dump", "r");
    let err = import(&e, &req).unwrap_err();
    assert_eq!(err.code(), "51N41");
    assert_eq!(*e.core().catalog.objects(), before);
}

/// rmp #57 acceptance: a graph-scope dump holds exactly that graph's
/// elements.
#[test]
fn a_graph_scope_dump_holds_exactly_its_graph() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let g2 = new_graph(&e, "second");
    let (ns, es) = property_graph(3, 6, 4, false);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    let (ns2, es2) = property_graph(4, 9, 7, false);
    load_generated(&e, g2, &ns2, &es2);
    let s = export::stored(
        &e,
        Stored::Graph(g2),
        Format::Dump,
        &dir,
        &["g.dump"],
        &allow,
    )
    .unwrap();
    assert_eq!((s.nodes, s.edges), (9, 7));
    let bytes = std::fs::read(dir.join("g.dump")).unwrap();
    assert_eq!(dump::read_all(&bytes).unwrap(), 16);
    let g3 = new_graph(&e, "third");
    let req = request(Format::Dump, "/public/third", &dir, &["g.dump".into()], "r");
    import(&e, &req).unwrap();
    assert_same(&content(&e, g2), &content(&e, g3), "graph dump");
}

/// rmp #57 acceptance (FR-IMP-003): a query-result export to the native
/// dump and a database export to CSV fail with 22N11 and write no file;
/// so do a wrong number of file names and an existing target.
#[test]
fn excluded_pairs_and_existing_targets_fail_before_any_file() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let query = ExportScope::Query {
        gql: "MATCH (n) RETURN n".into(),
        parameters: Parameters::new(),
    };
    assert_eq!(
        check_export(Format::Dump, &query, &["a"])
            .unwrap_err()
            .code(),
        "22N11"
    );
    assert_eq!(
        check_export(Format::Csv, &ExportScope::Database, &["a", "b"])
            .unwrap_err()
            .code(),
        "22N11"
    );
    assert_eq!(
        check_export(
            Format::Csv,
            &ExportScope::Graph("/public/lpg".into()),
            &["a"]
        )
        .unwrap_err()
        .code(),
        "22N11"
    );
    assert_eq!(
        check_export(Format::Dump, &ExportScope::Database, &["a/b"])
            .unwrap_err()
            .code(),
        "22N11"
    );
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
    std::fs::write(dir.join("taken.jsonl"), b"").unwrap();
    let err = export::stored(
        &e,
        Stored::Graph(DEFAULT_GRAPH_ID),
        Format::JsonLines,
        &dir,
        &["taken.jsonl"],
        &allow,
    )
    .unwrap_err();
    assert_eq!(err.code(), "22N11");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    // A text format cannot import a database; equal delimiters fail.
    let mut req = ImportRequest::new(Format::Csv, ImportTarget::Database, &dir, "a", "r");
    assert_eq!(import(&e, &req).unwrap_err().code(), "22N11");
    req.target = ImportTarget::Graph("/public/lpg".into());
    req.delimiter = b';';
    assert_eq!(import(&e, &req).unwrap_err().code(), "22N11");
    // A refusal of the authoriser fails the export before any file.
    let deny = |_: &[crate::auth::Request]| -> Result<(), Error> {
        Err(Error::new(crate::gqlstatus::codes::C42NFF, "denied"))
    };
    let err = export::stored(
        &e,
        Stored::Graph(DEFAULT_GRAPH_ID),
        Format::JsonLines,
        &dir,
        &["d.jsonl"],
        &deny,
    )
    .unwrap_err();
    assert_eq!(err.code(), "42NFF");
    assert!(!dir.join("d.jsonl").exists() && !dir.join("d.jsonl.tmp").exists());
}

/// Two nodes and an edge in CSV, with `bad` replacing the second node's
/// `uid` field.
fn csv_input(dir: &Path, bad_second: &str, bad_third: &str) {
    std::fs::write(
        dir.join("n.csv"),
        format!(":ID,uid:long\nn1,1\nn2,{bad_second}\nn3,{bad_third}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("e.csv"),
        ":START_ID,:END_ID,:TYPE,eid:long\nn1,n3,R,7\n",
    )
    .unwrap();
}

fn csv_request(dir: &Path, report: &str, on: OnInvalid) -> ImportRequest {
    let mut r = ImportRequest::csv(
        "/public/lpg",
        dir,
        vec![InputFile::new("n.csv")],
        vec![InputFile::new("e.csv")],
        report,
    );
    r.on_invalid = on;
    r
}

/// FR-IMP-004 acceptance: with one invalid record, the default aborts,
/// adds nothing and the report names the record; skip imports the valid
/// records, the report names the skipped one and the counts give the
/// valid records as imported and one as invalid; with two invalid
/// records, abort names only the first.
#[test]
fn one_invalid_record_aborts_by_default_and_is_skipped_on_request() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    csv_input(&dir, "x", "3");
    for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
        let mut req = csv_request(&dir, &format!("abort-{mode:?}"), OnInvalid::Abort);
        req.mode = mode;
        let err = import(&e, &req).unwrap_err();
        assert_eq!(err.code(), "22018", "{mode:?}");
        let lines = report_lines(&dir, &format!("abort-{mode:?}"));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["file"], "n.csv");
        assert_eq!(lines[0]["record"], 3);
        assert_eq!(lines[0]["id"], "n2");
        assert_eq!(lines[0]["gqlstatus"], "22018");
        assert!(content(&e, DEFAULT_GRAPH_ID).nodes.is_empty(), "{mode:?}");
    }
    let mut req = csv_request(&dir, "skip", OnInvalid::Skip);
    req.mode = ImportMode::Transactional;
    let s = import(&e, &req).unwrap();
    assert_eq!((s.nodes, s.edges, s.invalid), (2, 1, 1));
    assert_eq!(report_lines(&dir, "skip").len(), 1);
    assert_eq!(content(&e, DEFAULT_GRAPH_ID).nodes.len(), 2);
    // Two invalid records under abort: the report names only the first.
    let g = new_graph(&e, "two");
    csv_input(&dir, "x", "y");
    let mut req = csv_request(&dir, "two", OnInvalid::Abort);
    req.target = ImportTarget::Graph("/public/two".into());
    import(&e, &req).unwrap_err();
    let lines = report_lines(&dir, "two");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["record"], 3);
    assert!(content(&e, g).nodes.is_empty());
}

/// The offsets of the block boundaries of dump `b`: after the header and
/// after each block.
fn boundaries(b: &[u8]) -> Vec<usize> {
    let mut out = vec![dump::HEADER];
    let mut at = dump::HEADER;
    while at < b.len() {
        let len = u32::from_le_bytes(b[at + 1..at + 5].try_into().unwrap()) as usize;
        at += 9 + len;
        out.push(at);
    }
    out
}

/// rmp #57 acceptance: a dump truncated at every block boundary, and one
/// with one flipped byte in each block and in the header, fails with
/// 22N05 and adds nothing, whatever the invalid-record option.
#[test]
fn a_damaged_dump_fails_and_adds_nothing() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let src = new_graph(&e, "src");
    let (ns, es) = property_graph(5, 40, 60, true);
    load_generated(&e, src, &ns, &es);
    export::stored(
        &e,
        Stored::Graph(src),
        Format::Dump,
        &dir,
        &["g.dump"],
        &allow,
    )
    .unwrap();
    let good = std::fs::read(dir.join("g.dump")).unwrap();
    let cuts = boundaries(&good);
    assert!(
        cuts.len() >= 5,
        "catalog, nodes, edges, definitions, trailer"
    );
    let mut cases: Vec<Vec<u8>> = cuts[..cuts.len() - 1]
        .iter()
        .map(|&c| good[..c].to_vec())
        .collect();
    cases.push(Vec::new());
    let mut starts = vec![0];
    starts.extend(&cuts[..cuts.len() - 1]);
    let ends: Vec<usize> = cuts.clone();
    for (s, t) in starts.iter().zip(&ends) {
        let mut b = good.clone();
        b[(s + t) / 2] ^= 0x10;
        cases.push(b);
    }
    for (i, b) in cases.iter().enumerate() {
        std::fs::write(dir.join(format!("bad{i}.dump")), b).unwrap();
        for on in [OnInvalid::Abort, OnInvalid::Skip] {
            for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
                let mut req = request(
                    Format::Dump,
                    "/public/lpg",
                    &dir,
                    &[format!("bad{i}.dump")],
                    &format!("r{i}-{on:?}-{mode:?}"),
                );
                req.on_invalid = on;
                req.mode = mode;
                let err = import(&e, &req).unwrap_err();
                assert_eq!(err.code(), "22N05", "case {i} {on:?} {mode:?}: {err}");
                assert!(content(&e, DEFAULT_GRAPH_ID).nodes.is_empty());
            }
        }
    }
}

/// rmp #57 acceptance (FR-IMP-002, FR-LIM-007): an import needing more
/// memory than the cap fails with 51N72 and the graph is unchanged; the
/// accountant's peak never exceeds the cap.
#[test]
fn an_import_beyond_the_memory_cap_fails_and_changes_nothing() {
    const CAP: u64 = 8 << 20;
    let (d, e) = crate::tx::tests::db_with_cap(CAP);
    let dir = files_dir(&d, "x");
    let big = "x".repeat(16 << 10);
    let mut text = String::new();
    for i in 0..800 {
        text.push_str(&format!(
            "{{\"type\":\"node\",\"id\":\"{i}\",\"properties\":{{\"s\":\"{big}\"}}}}\n"
        ));
    }
    std::fs::write(dir.join("big.jsonl"), text).unwrap();
    let req = request(
        Format::JsonLines,
        "/public/lpg",
        &dir,
        &["big.jsonl".into()],
        "r",
    );
    let err = import(&e, &req).unwrap_err();
    assert!(matches!(err.code(), "51N72" | "51N73"), "{err}");
    assert!(content(&e, DEFAULT_GRAPH_ID).nodes.is_empty());
    assert!(
        e.accountant().peak() <= CAP,
        "peak {} > cap {CAP}",
        e.accountant().peak()
    );
    println!(
        "MEMORY transactional import: peak={} cap={CAP}",
        e.accountant().peak()
    );
}

/// rmp #57 acceptance (UC-OPS-004 A1): ENOSPC at each write of an export
/// fails it with 50N00 and leaves no target or temporary file.
#[test]
fn enospc_at_each_write_of_an_export_fails_it_and_leaves_no_file() {
    let d = crate::test_support::graph_dir();
    let fault = std::sync::Arc::new(FaultStorage::new(Box::new(crate::storage::FsStorage)));
    fault.skip_device_sync();
    let e = Engine::create_tuned(
        d.path(),
        fault.clone(),
        32 << 20,
        OWNER,
        std::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
        crate::storage::engine::Tuning::default(),
    )
    .unwrap();
    let dir = files_dir(&d, "x");
    let (ns, es) = property_graph(9, 150, 200, true);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    for (fi, format) in [
        Format::Dump,
        Format::Csv,
        Format::JsonLines,
        Format::GraphMl,
    ]
    .into_iter()
    .enumerate()
    {
        let names = |i: usize| -> Vec<String> {
            match format {
                Format::Csv => vec![format!("n{fi}-{i}.csv"), format!("e{fi}-{i}.csv")],
                _ => vec![format!("f{fi}-{i}")],
            }
        };
        let before = fault.count(Kind::WriteAt);
        let ok = names(0);
        let refs: Vec<&str> = ok.iter().map(String::as_str).collect();
        export::stored(
            &e,
            Stored::Graph(DEFAULT_GRAPH_ID),
            format,
            &dir,
            &refs,
            &allow,
        )
        .unwrap();
        let writes = fault.count(Kind::WriteAt) - before;
        assert!(writes > 0);
        for n in 1..=writes {
            let names = names(n as usize);
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            fault.fail_nth(
                Kind::WriteAt,
                fault.count(Kind::WriteAt) + n,
                Fault::NoSpace,
            );
            let err = export::stored(
                &e,
                Stored::Graph(DEFAULT_GRAPH_ID),
                format,
                &dir,
                &refs,
                &allow,
            )
            .unwrap_err();
            assert_eq!(err.code(), "50N00", "{format:?} write {n}");
            let left = fault.list(&dir).unwrap();
            for name in &names {
                assert!(
                    !left
                        .iter()
                        .any(|l| l.to_string_lossy().starts_with(name.as_str())),
                    "{format:?} write {n}: {name} left"
                );
            }
        }
        println!("ENOSPC {format:?}: {writes} write points");
    }
}
