//! Export (TS-26 Export layouts; FR-IMP-001, FR-IMP-003, UC-OPS-004; rmp
//! #57, #59).
//!
//! One read transaction per export, at snapshot isolation: a read-only
//! transaction reads one consistent snapshot and records no read set, so a
//! large export holds no memory per entry read (TS-15). The graph's
//! elements are read in chunks ([`each_node`], [`each_edge`]); CSV and
//! GraphML read them twice, first to type each property column or key
//! ([`Analysis`]), then to write. A query result is read once: rows of a
//! CSV export are spooled to an external sort keyed by row number, whose
//! files go through the storage trait in the database location and whose
//! memory is one reservation ([`Engine::sorter`]); a GraphML export sorts
//! the result's nodes and edges the same way, which also removes
//! duplicates.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use super::dump::{DumpCatalog, DumpObject, DumpWriter, Scope as DumpScope};
use super::files::{Outputs, io_error};
use super::{ExportSummary, Format};
use crate::catalog::{Definition, Kind, ROOT, TokenKind};
use crate::error::Error;
use crate::index::SchemaDef;
use crate::storage::engine::Engine;
use crate::storage::graph::{Graph, decode_edge, element_of};
use crate::tx::{Isolation, Transaction};
use crate::types::Value;
use crate::types::lexical::{Contents, TypeName};
use crate::types::record;

/// Records read per chunk of an export scan (FR-API-010: a scan holds
/// what one chunk needs).
const CHUNK: usize = 1024;

/// A node as an export writes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeOut {
    pub(crate) id: u64,
    pub(crate) labels: Vec<String>,
    pub(crate) props: Vec<(String, Value)>,
}

/// An edge as an export writes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeOut {
    pub(crate) id: u64,
    pub(crate) src: u64,
    pub(crate) dst: u64,
    pub(crate) ty: String,
    pub(crate) props: Vec<(String, Value)>,
}

/// The name of token `id`.
fn token_name(engine: &Engine, id: u64) -> Result<String, Error> {
    engine
        .token_name(id)
        .map(|(_, n)| n)
        .ok_or_else(|| crate::api::internal("a record names an unknown token"))
}

fn named(engine: &Engine, props: Vec<(u64, Value)>) -> Result<Vec<(String, Value)>, Error> {
    props
        .into_iter()
        .map(|(k, v)| Ok((token_name(engine, k)?, v)))
        .collect()
}

/// Calls `f` with each raw record of `tree` the snapshot sees, in key
/// order, reading in chunks.
/// The callback of [`each_raw`]: the transaction, a key and its value.
type RawFn<'a> = dyn FnMut(&mut Transaction, &[u8], &[u8]) -> Result<(), Error> + 'a;

fn each_raw(
    tx: &mut Transaction,
    tree: crate::storage::btree::Tree,
    f: &mut RawFn<'_>,
) -> Result<(), Error> {
    let mut lo = Vec::new();
    loop {
        let mut chunk: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let resume = tx.scan_chunk(tree, &lo, None, CHUNK, |k, v| {
            chunk.push((k.to_vec(), v.to_vec()));
            true
        })?;
        for (k, v) in &chunk {
            f(tx, k, v)?;
        }
        match resume {
            Some(r) => lo = r,
            None => return Ok(()),
        }
    }
}

/// Calls `f` with each node of `g` the snapshot sees.
///
/// # Errors
///
/// Storage errors; 50N00 for a malformed record; the errors of `f`.
pub(crate) fn each_node(
    engine: &Engine,
    tx: &mut Transaction,
    g: &Graph,
    f: &mut dyn FnMut(NodeOut) -> Result<(), Error>,
) -> Result<(), Error> {
    each_raw(tx, g.nodes, &mut |_, k, v| {
        let id = element_of(k).ok_or_else(|| crate::api::internal("a malformed node key"))?;
        let (labels, props) =
            record::decode(v).ok_or_else(|| crate::api::internal("a malformed node record"))?;
        f(NodeOut {
            id,
            labels: labels
                .into_iter()
                .map(|t| token_name(engine, t))
                .collect::<Result<_, _>>()?,
            props: named(engine, props)?,
        })
    })
}

/// Calls `f` with each edge of `g` the snapshot sees.
///
/// # Errors
///
/// As [`each_node`].
pub(crate) fn each_edge(
    engine: &Engine,
    tx: &mut Transaction,
    g: &Graph,
    f: &mut dyn FnMut(EdgeOut) -> Result<(), Error>,
) -> Result<(), Error> {
    each_edge_tx(engine, tx, g, &mut |_, e| f(e))
}

/// [`each_edge`] whose callback also receives the transaction, to read the
/// endpoints in the same snapshot.
///
/// # Errors
///
/// As [`each_node`].
pub(crate) fn each_edge_tx(
    engine: &Engine,
    tx: &mut Transaction,
    g: &Graph,
    f: &mut dyn FnMut(&mut Transaction, EdgeOut) -> Result<(), Error>,
) -> Result<(), Error> {
    each_raw(tx, g.edges, &mut |tx, k, v| {
        let id = element_of(k).ok_or_else(|| crate::api::internal("a malformed edge key"))?;
        let e = decode_edge(v).ok_or_else(|| crate::api::internal("a malformed edge record"))?;
        f(
            tx,
            EdgeOut {
                id,
                src: e.src,
                dst: e.dst,
                ty: token_name(engine, e.ty)?,
                props: named(engine, e.props)?,
            },
        )
    })
}

/// What the values of one property, or of one result column, are
/// (TS-26 Type annotations: uniform or mixed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Seen {
    /// The type names of the values.
    pub(crate) types: BTreeSet<TypeName>,
    /// A string value, or a string element of a list, is empty.
    pub(crate) empty_string: bool,
    /// A float value is NaN or infinite.
    pub(crate) non_finite: bool,
    /// A point value, or a point element of a list, has a NaN or infinite
    /// coordinate.
    pub(crate) non_finite_point: bool,
}

impl Seen {
    pub(crate) fn add(&mut self, v: &Value) {
        self.types.insert(TypeName::of(v));
        let finite = |p: &crate::types::Point| p.coords().iter().all(|c| c.is_finite());
        match v {
            Value::String(s) => self.empty_string |= s.is_empty(),
            Value::Float(x) => self.non_finite |= !x.is_finite(),
            Value::Point(p) => self.non_finite_point |= !finite(p),
            Value::List(items) => {
                for i in items.iter() {
                    match i {
                        Value::String(s) => self.empty_string |= s.is_empty(),
                        Value::Float(x) => self.non_finite |= !x.is_finite(),
                        Value::Point(p) => self.non_finite_point |= !finite(p),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// The one type of a uniform property, `None` for a mixed one or one
    /// with no value.
    pub(crate) fn uniform(&self) -> Option<&TypeName> {
        match self.types.len() {
            1 => self.types.iter().next(),
            _ => None,
        }
    }
}

/// The property names of the nodes and edges an export writes, each with
/// what its values are.
#[derive(Debug, Default)]
pub(crate) struct Analysis {
    pub(crate) node: BTreeMap<String, Seen>,
    pub(crate) edge: BTreeMap<String, Seen>,
}

impl Analysis {
    pub(crate) fn node(&mut self, n: &NodeOut) {
        for (k, v) in &n.props {
            self.node.entry(k.clone()).or_default().add(v);
        }
    }

    pub(crate) fn edge(&mut self, e: &EdgeOut) {
        for (k, v) in &e.props {
            self.edge.entry(k.clone()).or_default().add(v);
        }
    }
}

/// Contents of the nodes and edges of a query result, read in its
/// transaction ([`Contents`]).
#[derive(Debug, Default, Clone)]
pub(crate) struct ContentsMap {
    pub(crate) nodes: HashMap<u64, super::model::Labelled>,
    pub(crate) edges: HashMap<u64, EdgeContents>,
}

/// An edge's type, start, end and properties.
pub(crate) type EdgeContents = (String, u64, u64, Vec<(String, Value)>);

impl Contents for ContentsMap {
    fn node(&self, id: u64) -> (Vec<String>, Vec<(String, Value)>) {
        self.nodes.get(&id).cloned().unwrap_or_default()
    }

    fn edge(&self, id: u64) -> (String, u64, u64, Vec<(String, Value)>) {
        self.edges
            .get(&id)
            .cloned()
            .unwrap_or_else(|| (String::new(), 0, 0, Vec::new()))
    }
}

/// The node and edge identifiers `v` holds, also inside paths, lists and
/// records (TS-26 Query results).
pub(crate) fn elements(v: &Value, nodes: &mut Vec<u64>, edges: &mut Vec<u64>) {
    match v {
        Value::Node(id) => nodes.push(*id),
        Value::Edge(id) => edges.push(*id),
        Value::Path(ids) => {
            for (i, id) in ids.iter().enumerate() {
                if i % 2 == 0 {
                    nodes.push(*id);
                } else {
                    edges.push(*id);
                }
            }
        }
        Value::List(items) => {
            for i in items.iter() {
                elements(i, nodes, edges);
            }
        }
        Value::Record(fields) => {
            for (_, i) in fields.iter() {
                elements(i, nodes, edges);
            }
        }
        _ => {}
    }
}

/// Reads the contents of the nodes and edges `values` hold into `into`,
/// from transaction `tx`: each is looked up in every graph the snapshot
/// sees, since element identifiers are unique in the database (TS-07).
///
/// # Errors
///
/// Storage errors; 50N00 for a malformed record.
pub(crate) fn read_contents(
    engine: &Engine,
    tx: &mut Transaction,
    values: &[Value],
    into: &mut ContentsMap,
) -> Result<(), Error> {
    let (mut nodes, mut edges) = (Vec::new(), Vec::new());
    for v in values {
        elements(v, &mut nodes, &mut edges);
    }
    if nodes.is_empty() && edges.is_empty() {
        return Ok(());
    }
    let graphs: Vec<Graph> = tx
        .graph_ids()?
        .into_iter()
        .map(|g| tx.graph(g))
        .collect::<Result<_, _>>()?;
    for id in nodes {
        if into.nodes.contains_key(&id) {
            continue;
        }
        for g in &graphs {
            if let Some((labels, props)) = tx.node(g, id)? {
                let labels = labels
                    .into_iter()
                    .map(|t| token_name(engine, t))
                    .collect::<Result<_, _>>()?;
                into.nodes.insert(id, (labels, named(engine, props)?));
                break;
            }
        }
    }
    for id in edges {
        if into.edges.contains_key(&id) {
            continue;
        }
        for g in &graphs {
            if let Some(e) = tx.edge(g, id)? {
                let ty = token_name(engine, e.ty)?;
                into.edges
                    .insert(id, (ty, e.src, e.dst, named(engine, e.props)?));
                break;
            }
        }
    }
    Ok(())
}

/// The scope of an export of stored data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stored {
    Database,
    Graph(u64),
}

/// Exports `scope` to `format` into the files `names` of `location`
/// (checked by [`super::check_export`]), in one snapshot-isolation
/// transaction.
///
/// # Errors
///
/// 22N11 if a target file exists or a name cannot be written in the
/// format; 50N00 for I/O failures, ENOSPC among them: every file the
/// export wrote is then removed (UC-OPS-004 A1).
pub(crate) fn stored(
    engine: &Engine,
    scope: Stored,
    format: Format,
    location: &Path,
    names: &[&str],
    auth: &dyn Fn(&[crate::auth::Request]) -> Result<(), Error>,
) -> Result<ExportSummary, Error> {
    let storage = engine.core().storage.clone();
    let mut tx = engine.begin(Isolation::Snapshot)?;
    let r = (|| {
        // TS-26 Authorisation: TRAVERSE and READ on each graph of the
        // scope, as the snapshot sees it, before any file is written.
        let graphs = match scope {
            Stored::Graph(id) => vec![id],
            Stored::Database => tx.graph_ids()?,
        };
        let mut requests = Vec::new();
        for g in graphs {
            let path = tx
                .path(g)?
                .map(|p| format!("/{}", p.join("/")))
                .ok_or_else(|| crate::api::internal("a graph without a path"))?;
            let owner = tx.object(g)?.map(|o| o.owner).unwrap_or_default();
            for a in [crate::auth::Action::Traverse, crate::auth::Action::Read] {
                requests.push((a, crate::auth::Object::Catalog(path.clone()), owner.clone()));
            }
        }
        auth(&requests)?;
        let mut out = Outputs::new(&*storage, location, names)?;
        let files: Vec<usize> = names
            .iter()
            .map(|n| out.create(n))
            .collect::<Result<_, _>>()?;
        let summary = stored_in(engine, &mut tx, scope, format, &mut out, &files)?;
        out.commit()?;
        Ok(summary)
    })();
    tx.abort();
    r
}

fn stored_in(
    engine: &Engine,
    tx: &mut Transaction,
    scope: Stored,
    format: Format,
    out: &mut Outputs<'_>,
    files: &[usize],
) -> Result<ExportSummary, Error> {
    let mut summary = ExportSummary::default();
    match (format, scope) {
        (Format::Dump, _) => dump(engine, tx, scope, out.writer(files[0]), &mut summary)?,
        (_, Stored::Database) => unreachable!("checked: only the dump covers the database"),
        (Format::JsonLines, Stored::Graph(id)) => {
            let g = tx.graph(id)?;
            super::jsonl::write_graph(engine, tx, &g, out.writer(files[0]), &mut summary)?;
        }
        (Format::Csv, Stored::Graph(id)) => {
            let g = tx.graph(id)?;
            let mut a = Analysis::default();
            each_node(engine, tx, &g, &mut |n| {
                a.node(&n);
                Ok(())
            })?;
            each_edge(engine, tx, &g, &mut |e| {
                a.edge(&e);
                Ok(())
            })?;
            super::csv::write_graph(engine, tx, &g, &a, out, files, &mut summary)?;
        }
        (Format::GraphMl, Stored::Graph(id)) => {
            let g = tx.graph(id)?;
            let mut a = Analysis::default();
            each_node(engine, tx, &g, &mut |n| {
                a.node(&n);
                Ok(())
            })?;
            each_edge(engine, tx, &g, &mut |e| {
                a.edge(&e);
                Ok(())
            })?;
            let w = out.writer(files[0]);
            let mut gw = super::graphml::GraphWriter::start(w, &a)?;
            each_node(engine, tx, &g, &mut |n| {
                summary.nodes += 1;
                gw.node(&n)
            })?;
            each_edge(engine, tx, &g, &mut |e| {
                summary.edges += 1;
                gw.edge(&e)
            })?;
            gw.finish()?;
        }
    }
    Ok(summary)
}

/// Every catalog object the snapshot sees, from the root down.
fn all_objects(tx: &mut Transaction) -> Result<Vec<crate::catalog::Object>, Error> {
    let mut out = Vec::new();
    let mut todo = vec![ROOT];
    while let Some(p) = todo.pop() {
        for o in tx.children(p)? {
            if matches!(o.kind, Kind::Directory | Kind::Schema) {
                todo.push(o.id);
            }
            out.push(o);
        }
    }
    out.sort_by_key(|o| o.id);
    Ok(out)
}

/// Writes the native dump of `scope` (TS-26): every token, the catalog
/// objects of a database, the raw records of each graph, then the index
/// and constraint definitions.
fn dump(
    engine: &Engine,
    tx: &mut Transaction,
    scope: Stored,
    w: &mut super::files::Writer,
    summary: &mut ExportSummary,
) -> Result<(), Error> {
    let tokens: Vec<(u64, TokenKind, String)> = engine.core().catalog.tokens.all();
    let mut catalog = DumpCatalog {
        tokens,
        objects: Vec::new(),
    };
    // The dump's graphs: (dump identifier, graph).
    let mut graphs: Vec<(u64, u64)> = Vec::new();
    let dscope = match scope {
        Stored::Database => {
            for o in all_objects(tx)? {
                let (graph_type, definition) = match &o.definition {
                    Definition::Graph(d) => (d.graph_type, Vec::new()),
                    Definition::GraphType(b) => (0, b.clone()),
                    Definition::None => (0, Vec::new()),
                };
                if o.kind == Kind::Graph {
                    graphs.push((o.id, o.id));
                }
                catalog.objects.push(DumpObject {
                    id: o.id,
                    parent: o.parent,
                    kind: o.kind,
                    name: o.name,
                    graph_type,
                    definition,
                });
            }
            DumpScope::Database
        }
        Stored::Graph(id) => {
            graphs.push((0, id));
            DumpScope::Graph
        }
    };
    let mut dw = DumpWriter::new(w, dscope, &catalog).map_err(io_error)?;
    let mut defs: Vec<SchemaDef> = Vec::new();
    let mut handles = Vec::new();
    for &(did, id) in &graphs {
        let g = tx.graph(id)?;
        handles.push((did, g));
        for mut d in tx.schema_defs(id)? {
            d.graph = did;
            defs.push(d);
        }
    }
    for (did, g) in &handles {
        each_raw(tx, g.nodes, &mut |_, k, v| {
            let id = element_of(k).ok_or_else(|| crate::api::internal("a malformed node key"))?;
            summary.nodes += 1;
            dw.node(*did, id, v).map_err(io_error)
        })?;
    }
    for (did, g) in &handles {
        each_raw(tx, g.edges, &mut |_, k, v| {
            let id = element_of(k).ok_or_else(|| crate::api::internal("a malformed edge key"))?;
            if v.len() < 24 {
                return Err(crate::api::internal("a malformed edge record"));
            }
            let be = |i: usize| u64::from_be_bytes(v[i..i + 8].try_into().expect("8 bytes"));
            summary.edges += 1;
            dw.edge(*did, id, (be(0), be(8), be(16)), &v[24..])
                .map_err(io_error)
        })?;
    }
    dw.finish(&defs).map_err(io_error)
}

/// The rows of a query result, with the contents of the nodes and edges
/// each row holds, read in the query's transaction (the API's
/// [`crate::Session::export`]).
pub(crate) trait Rows {
    /// The result's column names.
    fn columns(&self) -> Vec<String>;
    /// The next row, or `None` after the last; `contents` receives the
    /// contents of its nodes and edges.
    ///
    /// # Errors
    ///
    /// The statement's errors.
    fn next_row(&mut self, contents: &mut ContentsMap) -> Result<Option<Vec<Value>>, Error>;
}

/// Exports the query result `rows` to `format` into the files `names` of
/// `location` (TS-26 Query results): CSV and JSON Lines one record per
/// row, GraphML the nodes and edges the result holds.
///
/// # Errors
///
/// The statement's errors; as [`stored`].
pub(crate) fn query(
    engine: &Engine,
    rows: &mut dyn Rows,
    format: Format,
    location: &Path,
    names: &[&str],
) -> Result<ExportSummary, Error> {
    let storage = engine.core().storage.clone();
    let mut out = Outputs::new(&*storage, location, names)?;
    let file = out.create(names[0])?;
    let mut summary = ExportSummary::default();
    let w = out.writer(file);
    match format {
        Format::JsonLines => super::jsonl::write_rows(rows, w, &mut summary)?,
        Format::Csv => super::csv::write_rows(engine, rows, w, &mut summary)?,
        Format::GraphMl => super::graphml::write_rows(engine, rows, w, &mut summary)?,
        Format::Dump => unreachable!("checked: the dump does not cover query results"),
    }
    out.commit()?;
    Ok(summary)
}
