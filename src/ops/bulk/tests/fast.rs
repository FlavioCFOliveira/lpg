//! The fast initial load (rmp #58): the empty-graph rule, constraints and
//! indexes built in the load, invalid records, the interlocks with
//! writers, readers and backups, and snapshot visibility.

use std::sync::mpsc;

use super::*;
use crate::index::build::Phase;
use crate::index::{Entity, PropertyType, SchemaKind, TypeSet};

/// Exports a generated graph to `dir/name.jsonl` from a scratch graph and
/// returns its content.
pub(super) fn generated_jsonl(
    e: &Engine,
    dir: &Path,
    name: &str,
    seed: u64,
    n: usize,
    m: usize,
) -> Content {
    let src = new_graph(e, &format!("src-{name}"));
    let (ns, es) = property_graph(seed, n, m, true);
    load_generated(e, src, &ns, &es);
    export::stored(
        e,
        Stored::Graph(src),
        Format::JsonLines,
        dir,
        &[&format!("{name}.jsonl")],
        &allow,
    )
    .unwrap();
    content(e, src)
}

pub(super) fn fast(path: &str, dir: &Path, file: &str, report: &str) -> ImportRequest {
    let mut r = ImportRequest::new(
        Format::JsonLines,
        ImportTarget::Graph(path.into()),
        dir,
        file,
        report,
    );
    r.mode = ImportMode::FastLoad;
    r
}

/// rmp #58 acceptance (FR-IMP-002, UC-OPS-003 A2): a fast load into a
/// graph with one node fails with 51N41 and changes nothing.
#[test]
fn a_fast_load_into_a_graph_with_one_node_fails_and_changes_nothing() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    generated_jsonl(&e, &dir, "in", 1, 5, 4);
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
    let k = e.token(TokenKind::PropertyKey, "uid").unwrap();
    tx.create_node(&g, &[], &[(k, Value::Int(-1))]).unwrap();
    tx.commit().unwrap();
    let err = import(&e, &fast("/public/lpg", &dir, "in.jsonl", "r")).unwrap_err();
    assert_eq!(err.code(), "51N41");
    assert_eq!(content(&e, DEFAULT_GRAPH_ID).nodes.len(), 1);
    // A graph whose only node was deleted is empty once no snapshot can
    // see the node.
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let id = content(&e, DEFAULT_GRAPH_ID).ids[0];
    tx.delete_node(&g, id, true).unwrap();
    tx.commit().unwrap();
    import(&e, &fast("/public/lpg", &dir, "in.jsonl", "r2")).unwrap();
    assert_eq!(content(&e, DEFAULT_GRAPH_ID).nodes.len(), 5);
    assert_intact(&e, "after the load");
}

/// rmp #58 acceptance: a fast load into an empty graph with a uniqueness,
/// an existence and a type constraint, a range, a point and a lookup
/// index on nodes and a range index on edges equals the input; each index
/// returns the rows a scan returns; the integrity check (which compares
/// every ONLINE index with the data and the statistics with a recount)
/// passes. The sorts' reservations stay within the cap.
#[test]
fn a_fast_load_with_constraints_and_indexes_equals_the_input() {
    const CAP: u64 = 32 << 20;
    let (d, e) = crate::tx::tests::db_with_cap(CAP);
    let dir = files_dir(&d, "x");
    let want = generated_jsonl(&e, &dir, "in", 2, 400, 700);
    let g = new_graph(&e, "g");
    let t = |k, n: &str| e.token(k, n).unwrap();
    let (a, b) = (t(TokenKind::Label, "A"), t(TokenKind::Label, "B"));
    let r = t(TokenKind::EdgeType, "R");
    let (uid, p1, p12, eid) = (
        t(TokenKind::PropertyKey, "uid"),
        t(TokenKind::PropertyKey, "p1"),
        t(TokenKind::PropertyKey, "p12"),
        t(TokenKind::PropertyKey, "eid"),
    );
    e.create_constraint(g, SchemaKind::Unique, Entity::Node, a, &[uid])
        .unwrap();
    e.create_constraint(g, SchemaKind::Exists, Entity::Node, b, &[uid])
        .unwrap();
    e.create_constraint(
        g,
        SchemaKind::Type(TypeSet::of(PropertyType::Integer)),
        Entity::Node,
        a,
        &[p1],
    )
    .unwrap();
    let mut ixs = Vec::new();
    for (kind, ent, tok, props) in [
        (SchemaKind::Range, Entity::Node, a, vec![p1]),
        (SchemaKind::Point, Entity::Node, a, vec![p12]),
        (SchemaKind::Lookup, Entity::Node, 0, vec![]),
        (SchemaKind::Range, Entity::Edge, r, vec![eid]),
    ] {
        let ix = e.create_index(g, kind, ent, tok, &props).unwrap();
        e.wait_index(ix).unwrap();
        ixs.push(ix);
    }
    let s = import(&e, &fast("/public/g", &dir, "in.jsonl", "r")).unwrap();
    assert_eq!((s.nodes, s.edges, s.invalid), (400, 700, 0));
    assert_same(&want, &content(&e, g), "fast load");
    assert_intact(&e, "fast load with indexes");
    // The range index on A.p1 returns the nodes a scan finds.
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let gr = tx.graph(g).unwrap();
    let mut by_index = tx.index_scan(&gr, ixs[0], b"", None).unwrap();
    by_index.sort_unstable();
    let mut by_scan = Vec::new();
    each_node(&e, &mut tx, &gr, &mut |n| {
        if n.labels.contains(&"A".to_owned()) && n.props.iter().any(|(k, _)| k == "p1") {
            by_scan.push(n.id);
        }
        Ok(())
    })
    .unwrap();
    by_scan.sort_unstable();
    assert_eq!(by_index, by_scan);
    assert!(!by_scan.is_empty());
    tx.abort();
    assert!(e.accountant().peak() <= CAP);
    println!("MEMORY fast load: peak={} cap={CAP}", e.accountant().peak());
}

/// rmp #58 acceptance: a duplicate input identifier, an edge to an unknown
/// node, an edge whose endpoint was skipped and each constraint violation
/// are reported; abort adds nothing; skip loads the rest.
#[test]
fn the_invalid_records_of_a_fast_load_are_reported() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let t = |k, n: &str| e.token(k, n).unwrap();
    let (a, b) = (t(TokenKind::Label, "A"), t(TokenKind::Label, "B"));
    let (uid, p1) = (
        t(TokenKind::PropertyKey, "uid"),
        t(TokenKind::PropertyKey, "p1"),
    );
    e.create_constraint(
        DEFAULT_GRAPH_ID,
        SchemaKind::Unique,
        Entity::Node,
        a,
        &[uid],
    )
    .unwrap();
    e.create_constraint(
        DEFAULT_GRAPH_ID,
        SchemaKind::Exists,
        Entity::Node,
        b,
        &[uid],
    )
    .unwrap();
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
        ":ID,:LABEL,uid:long,p1:lpg:annotated\n\
         n1,A,1,INTEGER:5\n\
         n1,A,2,\n\
         n3,B,,\n\
         n4,A,1,\n\
         n5,A,7,STRING:'x'\n\
         n6,,6,\n",
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
        r.mode = ImportMode::FastLoad;
        r.on_invalid = on;
        r
    };
    let err = import(&e, &req("abort", OnInvalid::Abort)).unwrap_err();
    assert_eq!(err.code(), "22N05");
    assert_eq!(report_lines(&dir, "abort").len(), 1);
    assert!(content(&e, DEFAULT_GRAPH_ID).nodes.is_empty());
    let s = import(&e, &req("skip", OnInvalid::Skip)).unwrap();
    assert_eq!((s.nodes, s.edges, s.invalid), (2, 1, 8));
    let lines = report_lines(&dir, "skip");
    let codes: Vec<(String, String)> = lines
        .iter()
        .map(|l| {
            (
                l["file"].as_str().unwrap().to_owned(),
                l["gqlstatus"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    for want in [
        ("n.csv", "22N05"),
        ("n.csv", "22N77"),
        ("n.csv", "22N78"),
        ("n.csv", "22N79"),
    ] {
        assert!(
            codes.iter().any(|c| (c.0.as_str(), c.1.as_str()) == want),
            "{want:?} in {codes:?}"
        );
    }
    assert_eq!(codes.iter().filter(|c| c.0 == "e.csv").count(), 4);
    assert!(
        lines
            .iter()
            .any(|l| l["reason"].as_str().unwrap().contains("no node"))
    );
    assert!(
        lines
            .iter()
            .filter(|l| l["reason"].as_str().unwrap().contains("skipped"))
            .count()
            == 3
    );
    assert_intact(&e, "skip");
}

/// A hook that pauses a fast load at [`Phase::LoadBuilt`] until `go`
/// receives, after signalling `reached`.
pub(super) fn pause_load(e: &Engine) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (rt, rr) = mpsc::channel();
    let (gt, gr) = mpsc::channel::<()>();
    let gr = std::sync::Mutex::new(gr);
    let hook: crate::index::build::Hook = std::sync::Arc::new(move |p| {
        if p == Phase::LoadBuilt {
            let _ = rt.send(());
            let _ = gr.lock().unwrap().recv();
        }
    });
    *e.core().builds.hook.lock().unwrap() = Some(hook);
    (rr, gt)
}

/// rmp #58 acceptance (FR-IMP-002, FR-OPS-001): while a load is paused
/// before its install, a writer of the graph waits until it ends (then,
/// its snapshot older than the load, fails with 25N11 under the
/// write-write rule); a backup fails with 51N41 and writes nothing to its
/// target; a reader sees the empty graph. A snapshot taken before the
/// install reads the graph empty after it; one taken after reads it all.
#[test]
fn during_a_load_writers_wait_backups_fail_and_older_snapshots_read_the_graph_empty() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let target = files_dir(&d, "backup");
    let want = generated_jsonl(&e, &dir, "in", 3, 300, 500);
    let (reached, go) = pause_load(&e);
    let e2 = Engine::handle(e.core_arc());
    let dir2 = dir.clone();
    let loader =
        std::thread::spawn(move || import(&e2, &fast("/public/lpg", &dir2, "in.jsonl", "r")));
    reached
        .recv_timeout(crate::test_support::WAIT_BOUND)
        .unwrap();
    // A reader sees the empty graph.
    assert!(content(&e, DEFAULT_GRAPH_ID).nodes.is_empty());
    let before = e.begin(Isolation::Snapshot).unwrap();
    // A backup fails with 51N41 and writes nothing.
    let err = e.backup(&target).unwrap_err();
    assert_eq!(err.gqlstatus().code(), "51N41");
    assert!(std::fs::read_dir(&target).unwrap().next().is_none());
    // A writer of the graph waits for the load.
    let e3 = Engine::handle(e.core_arc());
    let writer = std::thread::spawn(move || {
        let mut tx = e3.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
        let r = tx.create_node(&g, &[], &[]);
        let ok = r.is_ok() && tx.commit().is_ok();
        (ok, r.err().map(|e| e.gqlstatus().code()))
    });
    crate::tx::tests::until_waiting(&e, 1);
    assert!(!writer.is_finished());
    go.send(()).unwrap();
    loader.join().unwrap().unwrap();
    let (ok, code) = writer.join().unwrap();
    assert!(!ok && code == Some("25N11"), "the writer: {ok} {code:?}");
    // The snapshot taken before the install still reads the graph empty.
    let mut old = before;
    let g = old.graph(DEFAULT_GRAPH_ID).unwrap();
    let mut n = 0;
    old.scan_prefix(g.nodes, b"", |_, _| {
        n += 1;
        true
    })
    .unwrap();
    assert_eq!(n, 0);
    assert!(
        old.expand(&g, want.ids[0], crate::storage::graph::OUT, None)
            .unwrap()
            .is_empty()
    );
    old.abort();
    assert_same(&want, &content(&e, DEFAULT_GRAPH_ID), "after the install");
    *e.core().builds.hook.lock().unwrap() = None;
    assert_intact(&e, "after the load");
}

/// rmp #58 acceptance (FR-OPS-001, UC-OPS-003 A4): a load requested while
/// a backup holds the backup-or-load latch starts only after the backup
/// ends.
#[test]
fn a_load_requested_during_a_backup_waits_for_it() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    generated_jsonl(&e, &dir, "in", 4, 30, 40);
    let latch = e.core().try_backup_or_load().expect("no load runs");
    let waits = e
        .core()
        .load_waits
        .load(std::sync::atomic::Ordering::SeqCst);
    let e2 = Engine::handle(e.core_arc());
    let dir2 = dir.clone();
    let loader =
        std::thread::spawn(move || import(&e2, &fast("/public/lpg", &dir2, "in.jsonl", "r")));
    crate::test_support::wait_until("the load waits for the backup", || {
        e.core()
            .load_waits
            .load(std::sync::atomic::Ordering::SeqCst)
            > waits
    });
    assert!(!loader.is_finished());
    assert!(content(&e, DEFAULT_GRAPH_ID).nodes.is_empty());
    drop(latch);
    loader.join().unwrap().unwrap();
    assert_eq!(content(&e, DEFAULT_GRAPH_ID).nodes.len(), 30);
}

/// rmp #58 acceptance: a fast load whose sorts spill runs keeps the
/// accountant within the cap; the temporary-file bytes it writes and
/// reads through the storage trait (the fault storage's counters, rmp #54)
/// are non-zero, equal (every run is read back once) and printed for the
/// laboratory report; no temporary file is left.
#[test]
fn a_spilling_fast_load_stays_within_the_cap_and_counts_its_temporary_bytes() {
    use crate::storage::Storage;
    use crate::storage::fault::{FaultStorage, Kind};
    const CAP: u64 = 8 << 20;
    let d = crate::test_support::graph_dir();
    let fault = std::sync::Arc::new(FaultStorage::new(Box::new(crate::storage::FsStorage)));
    fault.skip_device_sync();
    let e = Engine::create_tuned(
        d.path(),
        fault.clone(),
        CAP,
        OWNER,
        std::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
        crate::storage::engine::Tuning::default(),
    )
    .unwrap();
    let dir = files_dir(&d, "x");
    let mut text = String::new();
    for i in 0..3_000 {
        text.push_str(&format!(
            "{{\"type\":\"node\",\"id\":\"{i}\",\"labels\":[\"A\"],\"properties\":{{\"uid\":{i},\"s\":\"{}\"}}}}\n",
            "y".repeat(200)
        ));
    }
    for i in 0..3_000 {
        text.push_str(&format!(
            "{{\"type\":\"relationship\",\"label\":\"R\",\"start\":{{\"id\":\"{i}\"}},\"end\":{{\"id\":\"{}\"}}}}\n",
            (i * 7) % 3_000
        ));
    }
    std::fs::write(dir.join("big.jsonl"), text).unwrap();
    let s = import(&e, &fast("/public/lpg", &dir, "big.jsonl", "r")).unwrap();
    assert_eq!((s.nodes, s.edges), (3_000, 3_000));
    let written = fault.bytes(Kind::WriteAt, crate::storage::sort::SORT_TEMP);
    let read = fault.bytes(Kind::ReadAt, crate::storage::sort::SORT_TEMP);
    println!(
        "FASTLOAD temp-file bytes: written={written} read={read}; peak={} cap={CAP}",
        e.accountant().peak()
    );
    assert!(written > 0, "precondition: the sorts spill");
    assert_eq!(read, written);
    assert!(e.accountant().peak() <= CAP);
    assert!(
        !fault
            .list(d.path())
            .unwrap()
            .iter()
            .any(|n| crate::storage::sort::is_sort_temp(n))
    );
    assert_intact(&e, "spilling load");
}
