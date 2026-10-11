//! Tests of the storage audit's findings on rmp #57 and #58 (FINDING 450
//! to 460) and of DECISIONs 444, 445, 448 and 461 on rmp #58 and #59.

use std::sync::mpsc;

use super::fast::{fast, generated_jsonl, pause_load};
use super::*;
use crate::index::{Entity, SchemaKind};
use crate::storage::fault::{Crash, Fault, FaultStorage, Kind};

/// Nodes of `graph` a snapshot transaction counts.
fn nodes_seen(tx: &mut crate::tx::Transaction, graph: u64) -> usize {
    let g = tx.graph(graph).unwrap();
    let mut n = 0;
    tx.scan_prefix(g.nodes, b"", |_, _| {
        n += 1;
        true
    })
    .unwrap();
    n
}

/// A lookup index on the default graph, ONLINE.
fn lookup_index(e: &Engine) -> u64 {
    let ix = e
        .create_index(DEFAULT_GRAPH_ID, SchemaKind::Lookup, Entity::Node, 0, &[])
        .unwrap();
    e.wait_index(ix).unwrap();
    ix
}

/// FINDING 450: inside the install commit's batch, after it changed its
/// pages (the `batch_body` point, commit latch held), a snapshot older
/// than the load and one begun during the batch read no node and no
/// lookup-index entry; after the commit an older snapshot still reads
/// none and a new one reads all. Fails when the trees are marked only
/// after the batch (the code before the fix).
#[test]
fn the_install_batch_hides_the_loaded_trees_from_older_snapshots() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    generated_jsonl(&e, &dir, "in", 41, 40, 30);
    let ix = lookup_index(&e);
    let mut before = e.begin(Isolation::Snapshot).unwrap();
    let (reached, go) = pause_load(&e);
    let e2 = Engine::handle(e.core_arc());
    let dir2 = dir.clone();
    let loader =
        std::thread::spawn(move || import(&e2, &fast("/public/lpg", &dir2, "in.jsonl", "r")));
    reached
        .recv_timeout(crate::test_support::WAIT_BOUND)
        .unwrap();
    let mid = crate::test_support::mid_batch(&e);
    go.send(()).unwrap();
    mid.wait();
    let mut during = e.begin(Isolation::Snapshot).unwrap();
    for (tx, what) in [(&mut before, "older"), (&mut during, "during")] {
        assert_eq!(
            nodes_seen(tx, DEFAULT_GRAPH_ID),
            0,
            "{what} snapshot, nodes"
        );
        let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
        assert!(
            tx.index_scan(&g, ix, b"", None).unwrap().is_empty(),
            "{what}: index"
        );
    }
    mid.fail.send(false).unwrap();
    loader.join().unwrap().unwrap();
    assert_eq!(nodes_seen(&mut before, DEFAULT_GRAPH_ID), 0);
    let mut after = e.begin(Isolation::Snapshot).unwrap();
    assert_eq!(nodes_seen(&mut after, DEFAULT_GRAPH_ID), 40);
    *e.core().builds.hook.lock().unwrap() = None;
    assert_intact(&e, "after the install");
}

/// FINDING 450, undo: an install batch that fails (ENOSPC in its body) is
/// undone and its marks are restored: the load fails, a later writer of
/// the graph is not refused as if a load had committed after its
/// snapshot, and the graph then loads.
#[test]
fn an_undone_install_restores_the_marks() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    generated_jsonl(&e, &dir, "in", 42, 20, 10);
    let (reached, go) = pause_load(&e);
    let e2 = Engine::handle(e.core_arc());
    let dir2 = dir.clone();
    let loader =
        std::thread::spawn(move || import(&e2, &fast("/public/lpg", &dir2, "in.jsonl", "r")));
    reached
        .recv_timeout(crate::test_support::WAIT_BOUND)
        .unwrap();
    let mid = crate::test_support::mid_batch(&e);
    go.send(()).unwrap();
    mid.wait();
    mid.fail.send(true).unwrap();
    assert_eq!(loader.join().unwrap().unwrap_err().code(), "50N00");
    *e.core().builds.hook.lock().unwrap() = None;
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(nodes_seen(&mut tx, DEFAULT_GRAPH_ID), 0);
    let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
    tx.create_node(&g, &[], &[]).unwrap();
    tx.commit().unwrap();
    assert_intact(&e, "after the undone install");
}

/// Audit (FINDING 453, M1): a serializable transaction that read the graph
/// before a load committed, then writes another graph, fails with 25N11:
/// the load's catalog-entry rewrite is a write its reads conflict with.
#[test]
fn a_serializable_reader_of_the_graph_before_a_load_cannot_commit_after_it() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    generated_jsonl(&e, &dir, "in", 43, 10, 5);
    let other = new_graph(&e, "other");
    let mut t = e.begin(Isolation::Serializable).unwrap();
    assert_eq!(nodes_seen(&mut t, DEFAULT_GRAPH_ID), 0);
    import(&e, &fast("/public/lpg", &dir, "in.jsonl", "r")).unwrap();
    let g = t.graph(other).unwrap();
    t.create_node(&g, &[], &[]).unwrap();
    let err = t.commit().unwrap_err();
    assert_eq!(err.gqlstatus().code(), "25N11");
}

/// Audit (FINDING 453, M3): an index scan from a snapshot older than the
/// load returns nothing; the loaded index trees are hidden as the graph's
/// are.
#[test]
fn an_index_scan_from_a_snapshot_older_than_the_load_returns_nothing() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    generated_jsonl(&e, &dir, "in", 44, 30, 5);
    let ix = lookup_index(&e);
    let mut old = e.begin(Isolation::Snapshot).unwrap();
    import(&e, &fast("/public/lpg", &dir, "in.jsonl", "r")).unwrap();
    let g = old.graph(DEFAULT_GRAPH_ID).unwrap();
    assert!(old.index_scan(&g, ix, b"", None).unwrap().is_empty());
    let mut new = e.begin(Isolation::Snapshot).unwrap();
    let g = new.graph(DEFAULT_GRAPH_ID).unwrap();
    // One entry per label of each node.
    assert!(!new.index_scan(&g, ix, b"", None).unwrap().is_empty());
}

/// Unique constraints on `A.uid` and `A.k` of the default graph, and a
/// CSV node file of `rows` (`id,uid,k`).
fn two_unique(e: &Engine, dir: &Path, rows: &str) {
    let a = e.token(TokenKind::Label, "A").unwrap();
    for p in ["uid", "k"] {
        let k = e.token(TokenKind::PropertyKey, p).unwrap();
        e.create_constraint(DEFAULT_GRAPH_ID, SchemaKind::Unique, Entity::Node, a, &[k])
            .unwrap();
    }
    std::fs::write(
        dir.join("n.csv"),
        format!(":ID,:LABEL,uid:long,k:long\n{rows}"),
    )
    .unwrap();
}

fn nodes_req(
    dir: &Path,
    path: &str,
    report: &str,
    mode: ImportMode,
    on: OnInvalid,
) -> ImportRequest {
    let mut r = ImportRequest::csv(path, dir, vec![InputFile::new("n.csv")], vec![], report);
    r.mode = mode;
    r.on_invalid = on;
    r
}

/// The input identifiers (`uid`) of the default graph's nodes, sorted.
fn uids(e: &Engine) -> Vec<i64> {
    let mut v: Vec<i64> = content_raw(e).into_iter().map(|(u, _)| u).collect();
    v.sort_unstable();
    v
}

fn content_raw(e: &Engine) -> Vec<(i64, i64)> {
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
    let mut out = Vec::new();
    each_node(e, &mut tx, &g, &mut |n| {
        let get = |k: &str| match n.props.iter().find(|(x, _)| x == k) {
            Some((_, Value::Int(i))) => *i,
            _ => -1,
        };
        out.push((get("uid"), get("k")));
        Ok(())
    })
    .unwrap();
    out
}

/// DECISION 461, FINDING 451: in skip mode the fast load keeps exactly the
/// records the transactional import keeps: `n3` conflicts only with `n2`,
/// which the first constraint rejects, so `n1` and `n3` load in both
/// modes; and the kept record of a group is the earliest in input order
/// (`z1` before `a1`), not the lowest identifier.
#[test]
fn skip_mode_keeps_the_same_records_in_both_modes() {
    for (rows, want) in [
        ("n1,A,1,1\nn2,A,1,2\nn3,A,3,2\n", vec![(1, 1), (3, 2)]),
        ("z1,A,1,10\na1,A,1,20\n", vec![(1, 10)]),
    ] {
        let mut got = Vec::new();
        for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
            let (d, e) = crate::tx::tests::db();
            let dir = files_dir(&d, "x");
            two_unique(&e, &dir, rows);
            let s = import(
                &e,
                &nodes_req(&dir, "/public/lpg", "r", mode, OnInvalid::Skip),
            )
            .unwrap();
            let mut c = content_raw(&e);
            c.sort_unstable();
            assert_eq!(c, want, "{mode:?} {rows:?}");
            assert_eq!(s.invalid as usize, rows.lines().count() - want.len());
            got.push(report_lines(&dir, "r").len());
        }
        assert_eq!(got[0], got[1]);
    }
}

/// FINDING 455: a record that breaks two uniqueness constraints is one
/// invalid record, one report line, in both modes.
#[test]
fn a_record_breaking_two_constraints_is_reported_once() {
    for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
        let (d, e) = crate::tx::tests::db();
        let dir = files_dir(&d, "x");
        two_unique(&e, &dir, "n1,A,1,1\nn2,A,1,1\n");
        let s = import(
            &e,
            &nodes_req(&dir, "/public/lpg", "r", mode, OnInvalid::Skip),
        )
        .unwrap();
        assert_eq!((s.nodes, s.invalid), (1, 1), "{mode:?}");
        assert_eq!(report_lines(&dir, "r").len(), 1, "{mode:?}");
        assert_eq!(uids(&e), vec![1]);
    }
}

/// FINDING 452 (FR-IMP-004): under abort both modes report the first
/// invalid record in input order: `z1` on line 3, not `a1`, which sorts
/// first.
#[test]
fn abort_reports_the_first_invalid_record_in_input_order_in_both_modes() {
    for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
        let (d, e) = crate::tx::tests::db();
        let dir = files_dir(&d, "x");
        std::fs::write(dir.join("n.csv"), ":ID,uid:long\nz1,1\nz1,2\na1,3\na1,4\n").unwrap();
        let err = import(
            &e,
            &nodes_req(&dir, "/public/lpg", "r", mode, OnInvalid::Abort),
        )
        .unwrap_err();
        assert_eq!(err.code(), "22N05");
        let lines = report_lines(&dir, "r");
        assert_eq!(lines.len(), 1, "{mode:?}");
        assert_eq!(
            (lines[0]["record"].as_u64(), lines[0]["id"].as_str()),
            (Some(3), Some("z1")),
            "{mode:?}"
        );
        assert!(uids(&e).is_empty());
    }
}

/// FINDING 454: a graph with 64 range indexes fast-loads at a 32 MiB cap:
/// the index keys share one sort.
#[test]
fn a_graph_with_sixty_four_indexes_fast_loads() {
    let (d, e) = crate::tx::tests::db_with_cap(32 << 20);
    let dir = files_dir(&d, "x");
    let want = generated_jsonl(&e, &dir, "in", 45, 25, 10);
    let a = e.token(TokenKind::Label, "A").unwrap();
    for i in 0..64 {
        let k = e.token(TokenKind::PropertyKey, &format!("q{i}")).unwrap();
        let ix = e
            .create_index(DEFAULT_GRAPH_ID, SchemaKind::Range, Entity::Node, a, &[k])
            .unwrap();
        e.wait_index(ix).unwrap();
    }
    let uid = e.token(TokenKind::PropertyKey, "uid").unwrap();
    let ix = e
        .create_index(DEFAULT_GRAPH_ID, SchemaKind::Range, Entity::Node, a, &[uid])
        .unwrap();
    e.wait_index(ix).unwrap();
    import(&e, &fast("/public/lpg", &dir, "in.jsonl", "r")).unwrap();
    assert_same(&want, &content(&e, DEFAULT_GRAPH_ID), "64 indexes");
    assert_intact(&e, "64 indexes");
}

/// An engine on a fault storage in `d`.
fn fault_engine(d: &Path, fault: &std::sync::Arc<FaultStorage>) -> Engine {
    Engine::create_tuned(
        d,
        fault.clone(),
        32 << 20,
        OWNER,
        std::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
        crate::storage::engine::Tuning::default(),
    )
    .unwrap()
}

fn reopen(d: &Path, fault: &std::sync::Arc<FaultStorage>) -> crate::storage::error::Result<Engine> {
    Engine::open_tuned(
        d,
        fault.clone(),
        32 << 20,
        std::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
        crate::storage::engine::Tuning::default(),
    )
}

/// FINDING 456: a report whose rename fails after the data committed
/// leaves the import successful, with the report's error beside the
/// counts; a retry is then refused by the report rule, not doubled
/// silently. Both modes.
#[test]
fn a_report_failure_after_the_data_commit_is_reported_beside_the_counts() {
    for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
        let d = crate::test_support::graph_dir();
        let fault = std::sync::Arc::new(FaultStorage::new(Box::new(crate::storage::FsStorage)));
        fault.skip_device_sync();
        let e = fault_engine(d.path(), &fault);
        let dir = files_dir(&d, "x");
        std::fs::write(dir.join("n.csv"), ":ID,uid:long\nn1,1\n").unwrap();
        fault.fail_nth(Kind::Rename, fault.count(Kind::Rename) + 1, Fault::Eio);
        let s = import(
            &e,
            &nodes_req(&dir, "/public/lpg", "r", mode, OnInvalid::Abort),
        )
        .unwrap();
        assert_eq!(s.nodes, 1, "{mode:?}");
        assert_eq!(
            s.report_error.as_ref().map(Error::code),
            Some("50N00"),
            "{mode:?}"
        );
        assert_eq!(uids(&e), vec![1]);
    }
}

/// FINDING 457 (a): a target created during the export is not replaced:
/// the commit fails with 22N11, the target keeps its content and no
/// temporary file is left.
#[test]
fn a_target_created_during_an_export_is_not_replaced() {
    let d = crate::test_support::graph_dir();
    let s = crate::storage::FsStorage;
    let mut o = files::Outputs::new(&s, d.path(), &["a"]).unwrap();
    let i = o.create("a").unwrap();
    std::io::Write::write_all(o.writer(i), b"export").unwrap();
    std::fs::write(d.path().join("a"), b"precious").unwrap();
    assert_eq!(o.commit().unwrap_err().code(), "22N11");
    assert_eq!(std::fs::read(d.path().join("a")).unwrap(), b"precious");
    assert!(!d.path().join("a.tmp").exists());
}

/// FINDING 457 (b): of two exports of one name, the second fails with
/// 22N11 when it creates its temporary file, and the first commits its
/// own content.
#[test]
fn two_exports_of_one_name_never_mix() {
    let d = crate::test_support::graph_dir();
    let s = crate::storage::FsStorage;
    let mut o1 = files::Outputs::new(&s, d.path(), &["a"]).unwrap();
    let i = o1.create("a").unwrap();
    std::io::Write::write_all(o1.writer(i), b"AAAAAAAAAA").unwrap();
    let mut o2 = files::Outputs::new(&s, d.path(), &["a"]).unwrap();
    assert_eq!(o2.create("a").unwrap_err().code(), "22N11");
    drop(o2);
    o1.commit().unwrap();
    assert_eq!(std::fs::read(d.path().join("a")).unwrap(), b"AAAAAAAAAA");
}

/// FINDING 458: a dump block is charged to the memory cap before its
/// buffer is made: a 16 MiB block under an 8 MiB cap fails with 51N72.
#[test]
fn a_dump_block_beyond_the_cap_fails_with_51n72() {
    let (_d, e) = crate::tx::tests::db_with_cap(8 << 20);
    let mut b = Vec::new();
    let mut w =
        dump::DumpWriter::new(&mut b, dump::Scope::Graph, &dump::DumpCatalog::default()).unwrap();
    w.node(0, 1, &vec![0u8; 16 << 20]).unwrap();
    w.finish(&[]).unwrap();
    let file = dump::Memory(b);
    let err = dump::verify(&file, "big", Some(&e.core().pool))
        .err()
        .unwrap();
    assert_eq!(err.code(), "51N72");
    assert!(e.accountant().peak() <= 8 << 20);
}

/// A file whose bytes a test changes after `verify` read them.
struct Changing(std::sync::Mutex<Vec<u8>>);

impl crate::storage::StorageFile for Changing {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        let b = self.0.lock().unwrap();
        let at = (offset as usize).min(b.len());
        let n = buf.len().min(b.len() - at);
        buf[..n].copy_from_slice(&b[at..at + n]);
        Ok(n)
    }
    fn write_at(&self, _: &[u8], _: u64) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn sync_data(&self) -> std::io::Result<()> {
        Ok(())
    }
    fn set_len(&self, _: u64) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn len(&self) -> std::io::Result<u64> {
        Ok(self.0.lock().unwrap().len() as u64)
    }
    fn reserve(&self, _: u64, _: u64) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// A one-block payload with a valid CRC-32C: kind, length, payload, CRC.
fn block(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![kind];
    b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    b.extend_from_slice(payload);
    let crc = crc32c::crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    b
}

/// FINDING 459: a node block replaced after `verify` by one of the same
/// length, a valid CRC-32C and malformed records fails the read with
/// 22N05, never a panic; `verify` refuses a block of a graph outside the
/// dump's scope.
#[test]
fn a_dump_block_changed_after_verify_or_outside_the_scope_fails_with_22n05() {
    let mut b = Vec::new();
    let mut w =
        dump::DumpWriter::new(&mut b, dump::Scope::Graph, &dump::DumpCatalog::default()).unwrap();
    w.node(0, 1, &[0]).unwrap();
    w.finish(&[]).unwrap();
    let cuts = framework_boundaries(&b);
    let (start, end) = (cuts[1], cuts[2]);
    let file = Changing(std::sync::Mutex::new(b.clone()));
    let d = dump::verify(&file, "x", None).unwrap();
    // Same length (graph 0, then a varint that never ends), valid CRC.
    let len = end - start - 9;
    let mut payload = vec![0u8];
    payload.extend(std::iter::repeat_n(0xFFu8, len - 1));
    file.0.lock().unwrap()[start..end].copy_from_slice(&block(2, &payload));
    let err = model::Source::nodes(&mut d.graph(0), &mut |_| Ok(())).unwrap_err();
    assert_eq!(err.code(), "22N05");
    // A node block of graph 5 in a graph-scope dump.
    let mut b = Vec::new();
    let mut w =
        dump::DumpWriter::new(&mut b, dump::Scope::Graph, &dump::DumpCatalog::default()).unwrap();
    w.node(5, 1, &[0]).unwrap();
    w.finish(&[]).unwrap();
    let err = dump::verify(&dump::Memory(b), "x", None).err().unwrap();
    assert_eq!(err.code(), "22N05");
}

/// The block boundaries of dump bytes `b`: after the header, after each
/// block.
fn framework_boundaries(b: &[u8]) -> Vec<usize> {
    let mut out = vec![dump::HEADER];
    let mut at = dump::HEADER;
    while at < b.len() {
        let len = u32::from_le_bytes(b[at + 1..at + 5].try_into().unwrap()) as usize;
        at += 9 + len;
        out.push(at);
    }
    out
}

/// FINDING 460: a graph created while a database import holds its
/// catalog transaction, after it read the empty catalog, fails the
/// import's commit (serializable validation); the catalog then holds the
/// concurrent graph only.
#[test]
fn a_catalog_object_created_during_a_database_import_fails_it() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let (src_d, src) = crate::tx::tests::db();
    let (ns, es) = property_graph(46, 5, 3, false);
    load_generated(&src, DEFAULT_GRAPH_ID, &ns, &es);
    new_graph(&src, "g2");
    export::stored(
        &src,
        Stored::Database,
        Format::Dump,
        &dir,
        &["db.dump"],
        &allow,
    )
    .unwrap();
    drop((src, src_d));
    let (tx_r, rx_r) = mpsc::channel();
    let (tx_g, rx_g) = mpsc::channel::<()>();
    let rx_g = std::sync::Mutex::new(rx_g);
    let hook: crate::index::build::Hook = std::sync::Arc::new(move |p| {
        if p == crate::index::build::Phase::DatabaseChecked {
            let _ = tx_r.send(());
            let _ = rx_g.lock().unwrap().recv();
        }
    });
    *e.core().builds.hook.lock().unwrap() = Some(hook);
    let e2 = Engine::handle(e.core_arc());
    let dir2 = dir.clone();
    let importer = std::thread::spawn(move || {
        import(
            &e2,
            &ImportRequest::new(Format::Dump, ImportTarget::Database, &dir2, "db.dump", "r"),
        )
    });
    rx_r.recv_timeout(crate::test_support::WAIT_BOUND).unwrap();
    new_graph(&e, "concurrent");
    tx_g.send(()).unwrap();
    let err = importer.join().unwrap().unwrap_err();
    assert!(matches!(err.code(), "25N11" | "51N41"), "{err}");
    *e.core().builds.hook.lock().unwrap() = None;
    let names: Vec<String> = e
        .core()
        .catalog
        .objects()
        .values()
        .map(|o| o.name.clone())
        .collect();
    assert_eq!(names, vec!["public", "lpg", "concurrent"]);
}

/// Audit (FINDING 453): after a fast load, a crash and a reopen, a new
/// node's identifier lies above every loaded identifier; and a crash at
/// each crash point of the reopen that replays the install group, then
/// another reopen, gives the complete graph with a clean integrity check.
#[test]
fn identifiers_and_the_install_survive_crashes_during_recovery() {
    let d = crate::test_support::graph_dir();
    let fault = std::sync::Arc::new(FaultStorage::new(Box::new(crate::storage::FsStorage)));
    fault.skip_device_sync();
    let e = fault_engine(d.path(), &fault);
    let dir = d.path().join("x");
    std::fs::create_dir_all(&dir).unwrap();
    let want = generated_jsonl(&e, &dir, "in", 47, 30, 20);
    crate::tx::tests::pause_gc(&e);
    // Into a new graph: the scratch graph keeps its own content.
    let g = new_graph(&e, "g");
    import(&e, &fast("/public/g", &dir, "in.jsonl", "r")).unwrap();
    let max_loaded = *content(&e, g).ids.iter().max().unwrap();
    drop(e);
    fault.crash(Crash::DropAll).unwrap();
    // The reopen's crash points: those of the replay of the install group.
    let p0 = fault.points();
    let e = reopen(d.path(), &fault).unwrap();
    let points = fault.points() - p0;
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let gr = tx.graph(g).unwrap();
    let id = tx.create_node(&gr, &[], &[]).unwrap();
    tx.abort();
    assert!(
        id > max_loaded,
        "new node {id} not above loaded {max_loaded}"
    );
    drop(e);
    fault.crash(Crash::DropAll).unwrap();
    assert!(points > 0);
    for k in 1..=points {
        fault.crash_after(k);
        let _ = reopen(d.path(), &fault);
        fault.crash(Crash::DropAll).unwrap();
        let e = reopen(d.path(), &fault).unwrap();
        assert_same(
            &want,
            &content(&e, g),
            &format!("crash at reopen point {k}"),
        );
        let p = e.check_integrity().unwrap();
        assert!(p.is_empty(), "point {k}: {p:?}");
        drop(e);
        fault.crash(Crash::DropAll).unwrap();
    }
    println!("RECOVERY install replay: {points} crash points of the reopen");
}

/// DECISION 444 (FR-IMP-005 acceptance): `id:ID(Person)` with `42` gives
/// the STRING property `id` = "42" by default and the INT64 42 with
/// `id_type` = `integer`; with `integer`, `042` and `42` name one node, a
/// non-integer node identifier or edge endpoint is an invalid record
/// (22018); an unnamed `:ID` stores nothing.
#[test]
fn a_named_id_column_is_stored_typed_by_id_type() {
    for (ids, want) in [
        (IdType::String, Value::string("42")),
        (IdType::Integer, Value::Int(42)),
    ] {
        for mode in [ImportMode::Transactional, ImportMode::FastLoad] {
            let (d, e) = crate::tx::tests::db();
            let dir = files_dir(&d, "x");
            std::fs::write(dir.join("p.csv"), "id:ID(Person),name\n42,a\n7,b\n").unwrap();
            std::fs::write(dir.join("u.csv"), ":ID(U),name\n9,c\n").unwrap();
            let end = if ids == IdType::Integer { "042" } else { "42" };
            std::fs::write(
                dir.join("e.csv"),
                format!(":START_ID(Person),:END_ID(Person),:TYPE\n7,{end},KNOWS\n"),
            )
            .unwrap();
            let mut r = ImportRequest::csv(
                "/public/lpg",
                &dir,
                vec![InputFile::new("p.csv"), InputFile::new("u.csv")],
                vec![InputFile::new("e.csv")],
                "r",
            );
            r.mode = mode;
            r.id_type = ids;
            let s = import(&e, &r).unwrap();
            assert_eq!((s.nodes, s.edges, s.invalid), (3, 1, 0), "{ids:?} {mode:?}");
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
            let mut found = Vec::new();
            each_node(&e, &mut tx, &g, &mut |n| {
                found.push(n.props);
                Ok(())
            })
            .unwrap();
            assert!(
                found
                    .iter()
                    .any(|p| p.contains(&("id".into(), want.clone()))),
                "{found:?}"
            );
            assert_eq!(
                found
                    .iter()
                    .filter(|p| p.iter().any(|(k, _)| k == "id"))
                    .count(),
                2
            );
        }
    }
    // A non-integer identifier under `integer`.
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    std::fs::write(dir.join("p.csv"), "id:ID,name\nx,a\n5,b\n").unwrap();
    std::fs::write(dir.join("e.csv"), ":START_ID,:END_ID,:TYPE\n5,y,R\n").unwrap();
    let mut r = ImportRequest::csv(
        "/public/lpg",
        &dir,
        vec![InputFile::new("p.csv")],
        vec![InputFile::new("e.csv")],
        "r",
    );
    r.id_type = IdType::Integer;
    r.on_invalid = OnInvalid::Skip;
    let s = import(&e, &r).unwrap();
    assert_eq!((s.nodes, s.edges, s.invalid), (1, 0, 2));
    let lines = report_lines(&dir, "r");
    assert!(lines.iter().all(|l| l["gqlstatus"] == "22018"), "{lines:?}");
}

/// DECISIONs 445 and 448: a `point` or `point[]` property holding a point
/// with a non-finite coordinate is an `lpg:` column in a CSV export; a
/// finite one stays a Neo4j `point` column; all round-trip.
#[test]
fn a_point_column_with_a_non_finite_coordinate_is_an_lpg_column() {
    use crate::types::Srid;
    let pt = |x: f64| crate::test_support::values::point(Srid::Cartesian2D, &[x, 1.0]);
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let nodes: Vec<GenNode> = [f64::NAN, 2.0]
        .into_iter()
        .enumerate()
        .map(|(i, x)| GenNode {
            labels: vec![],
            props: vec![
                ("uid".into(), Value::Int(i as i64)),
                ("p".into(), pt(x)),
                (
                    "ps".into(),
                    Value::List(vec![pt(1.0), pt(f64::INFINITY * x.signum())].into()),
                ),
                ("ok".into(), pt(3.0)),
            ],
        })
        .collect();
    load_generated(&e, DEFAULT_GRAPH_ID, &nodes, &[]);
    let want = content(&e, DEFAULT_GRAPH_ID);
    let names = export_graph(&e, DEFAULT_GRAPH_ID, Format::Csv, &dir, "p");
    let head = std::fs::read_to_string(dir.join(&names[0])).unwrap();
    let head = head.lines().next().unwrap().to_owned();
    assert!(
        head.contains("p:lpg:POINT") && head.contains("ps:lpg:LIST<POINT>"),
        "{head}"
    );
    assert!(head.contains("ok:point"), "{head}");
    let g = new_graph(&e, "back");
    import(&e, &request(Format::Csv, "/public/back", &dir, &names, "r")).unwrap();
    assert_same(&want, &content(&e, g), "points");
}
