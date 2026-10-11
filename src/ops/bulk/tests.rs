//! Tests of bulk import and export (rmp #57, #58, #59): the round trips
//! of every format by both import modes, the format and scope matrix, the
//! invalid-record report, the native dump's damage checks, the memory cap,
//! ENOSPC during an export, and the fast load's interlocks.

use std::path::{Path, PathBuf};

use super::export::{Stored, each_edge, each_node};
use super::*;
use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID, TokenKind};
use crate::lab::generator::{GenEdge, GenNode, property_graph};
use crate::storage::engine::Engine;
use crate::test_support::OWNER;
use crate::tx::Isolation;
use crate::types::Value;
use crate::types::lexical::bit_eq;

mod audit;
mod fast;
mod formats;
mod framework;

/// Every request is allowed.
fn allow(_: &[crate::auth::Request]) -> Result<(), Error> {
    Ok(())
}

/// A directory for export and report files inside the test's database
/// directory (graph data files, CLAUDE.md).
fn files_dir(d: &tempfile::TempDir, name: &str) -> PathBuf {
    let p = d.path().join(name);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Creates graph `name` in the default schema.
fn new_graph(e: &Engine, name: &str) -> u64 {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let id = tx
        .create_graph(DEFAULT_SCHEMA_ID, name, OWNER, None)
        .unwrap();
    tx.commit().unwrap();
    id
}

/// Writes the generated graph into graph `graph` in one transaction.
fn load_generated(e: &Engine, graph: u64, nodes: &[GenNode], edges: &[GenEdge]) {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let g = tx.graph(graph).unwrap();
    let props = |p: &[(String, Value)]| -> Vec<(u64, Value)> {
        let mut v: Vec<(u64, Value)> = p
            .iter()
            .map(|(k, v)| (e.token(TokenKind::PropertyKey, k).unwrap(), v.clone()))
            .collect();
        v.sort_by_key(|(k, _)| *k);
        v
    };
    let ids: Vec<u64> = nodes
        .iter()
        .map(|n| {
            let mut labels: Vec<u64> = n
                .labels
                .iter()
                .map(|l| e.token(TokenKind::Label, l).unwrap())
                .collect();
            labels.sort_unstable();
            tx.create_node(&g, &labels, &props(&n.props)).unwrap()
        })
        .collect();
    for ed in edges {
        let ty = e.token(TokenKind::EdgeType, &ed.ty).unwrap();
        tx.create_edge(&g, ids[ed.src], ids[ed.dst], ty, &props(&ed.props))
            .unwrap();
    }
    tx.commit().unwrap();
}

/// A graph's content, independent of its identifiers: each node by its
/// `uid` with its sorted labels and properties; each edge by its `eid`
/// with the `uid`s of its endpoints, its type and properties.
#[derive(Debug)]
struct Content {
    nodes: Vec<NodeContent>,
    edges: Vec<EdgeContent>,
    ids: Vec<u64>,
}

/// A node's `uid`, sorted labels and sorted properties.
type NodeContent = (i64, Vec<String>, Vec<(String, Value)>);
/// An edge's `eid`, its endpoints' `uid`s, type and sorted properties.
type EdgeContent = (i64, i64, i64, String, Vec<(String, Value)>);

fn uid(props: &[(String, Value)], key: &str) -> i64 {
    match props.iter().find(|(k, _)| k == key) {
        Some((_, Value::Int(i))) => *i,
        _ => panic!("no {key}"),
    }
}

fn content(e: &Engine, graph: u64) -> Content {
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let g = tx.graph(graph).unwrap();
    let mut nodes = Vec::new();
    let mut by_id = std::collections::HashMap::new();
    let mut ids = Vec::new();
    each_node(e, &mut tx, &g, &mut |n| {
        let u = uid(&n.props, "uid");
        by_id.insert(n.id, u);
        ids.push(n.id);
        let mut labels = n.labels;
        labels.sort();
        let mut props = n.props;
        props.sort_by(|a, b| a.0.cmp(&b.0));
        nodes.push((u, labels, props));
        Ok(())
    })
    .unwrap();
    let mut edges = Vec::new();
    each_edge(e, &mut tx, &g, &mut |ed| {
        ids.push(ed.id);
        let mut props = ed.props;
        props.sort_by(|a, b| a.0.cmp(&b.0));
        edges.push((
            uid(&props, "eid"),
            by_id[&ed.src],
            by_id[&ed.dst],
            ed.ty,
            props,
        ));
        Ok(())
    })
    .unwrap();
    tx.abort();
    nodes.sort_by_key(|n| n.0);
    edges.sort_by_key(|e| e.0);
    Content { nodes, edges, ids }
}

fn same_props(a: &[(String, Value)], b: &[(String, Value)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.0 == y.0 && bit_eq(&x.1, &y.1))
}

/// The two graphs hold the same nodes, edges, labels and property values,
/// floats bit-equal (NaN for NaN), and share no element identifier
/// (FR-IMP-005).
fn assert_same(a: &Content, b: &Content, what: &str) {
    assert_same_in(a, b, what, true);
}

/// [`assert_same`]; the identifiers are compared only with `same_db`, for
/// two graphs of one database.
fn assert_same_in(a: &Content, b: &Content, what: &str, same_db: bool) {
    assert_eq!(a.nodes.len(), b.nodes.len(), "{what}: node count");
    assert_eq!(a.edges.len(), b.edges.len(), "{what}: edge count");
    for (x, y) in a.nodes.iter().zip(&b.nodes) {
        assert_eq!((x.0, &x.1), (y.0, &y.1), "{what}: node labels");
        assert!(
            same_props(&x.2, &y.2),
            "{what}: node {} props {:?} != {:?}",
            x.0,
            x.2,
            y.2
        );
    }
    for (x, y) in a.edges.iter().zip(&b.edges) {
        assert_eq!((x.0, x.1, x.2, &x.3), (y.0, y.1, y.2, &y.3), "{what}: edge");
        assert!(
            same_props(&x.4, &y.4),
            "{what}: edge {} props {:?} != {:?}",
            x.0,
            x.4,
            y.4
        );
    }
    assert!(
        !same_db || a.ids.iter().all(|i| !b.ids.contains(i)),
        "{what}: an identifier was reused"
    );
}

/// Exports graph `graph` to `format` in `dir`, as files named after
/// `stem`; returns the file names.
fn export_graph(e: &Engine, graph: u64, format: Format, dir: &Path, stem: &str) -> Vec<String> {
    let names: Vec<String> = match format {
        Format::Csv => vec![format!("{stem}-nodes.csv"), format!("{stem}-edges.csv")],
        Format::JsonLines => vec![format!("{stem}.jsonl")],
        Format::GraphMl => vec![format!("{stem}.graphml")],
        Format::Dump => vec![format!("{stem}.dump")],
    };
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    export::stored(e, Stored::Graph(graph), format, dir, &refs, &allow)
        .unwrap_or_else(|err| panic!("export {format:?}: {err}"));
    names
}

/// The request importing the files `names` of `format` from `dir` into
/// the graph at `path`.
fn request(
    format: Format,
    path: &str,
    dir: &Path,
    names: &[String],
    report: &str,
) -> ImportRequest {
    match format {
        Format::Csv => ImportRequest::csv(
            path,
            dir,
            vec![InputFile::new(&names[0])],
            vec![InputFile::new(&names[1])],
            report,
        ),
        f => ImportRequest::new(
            f,
            ImportTarget::Graph(path.to_owned()),
            dir,
            &names[0],
            report,
        ),
    }
}

/// Imports with `req`.
fn import(e: &Engine, req: &ImportRequest) -> Result<ImportSummary, Error> {
    import::run(e, req, OWNER, None, &allow)
}

/// The lines of report `name` in `dir`.
fn report_lines(dir: &Path, name: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join(name))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn assert_intact(e: &Engine, what: &str) {
    let p = e.check_integrity().unwrap();
    assert!(p.is_empty(), "{what}: {p:?}");
}
