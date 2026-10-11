//! JSON Lines (TS-26 Export layouts; FR-IMP-006; rmp #59).
//!
//! **Layout**, as APOC writes it (R-APOC @ `e52bc20`
//! `core/src/main/java/apoc/export/json/JsonFormatSerializer.java:37-116`,
//! Apache-2.0, `Copyright (c) "Neo4j" Neo4j Sweden AB [http://neo4j.com]`;
//! notice in `LICENSE-APACHE`; the layout is ported): one object per node,
//! `{"type": "node", "id", "labels", "properties"}`, and per edge,
//! `{"type": "relationship", "id", "label", "properties", "start",
//! "end"}`; `id` is a JSON string; `start` and `end` are `{"id",
//! "labels"}`; `labels` and `properties` are left out when empty, as
//! APOC does. Import reads only the `id` of `start` and `end`.
//!
//! **Values** (TS-26 table): a boolean, an integer, a finite float, a
//! string and a list of BOOLEAN, INTEGER, FLOAT, STRING, a temporal
//! instant or POINT are JSON values (an array's elements by the same
//! rules); every other value, a non-finite float among them, is a tagged
//! object `{"$lpg_type": <type name>, "$value": <lexical form>}`. Every
//! JSON object in a value position is read as a tagged object. A JSON
//! `null` property value is invalid (22004); an integer outside the `i64`
//! range is invalid (22003); serde_json reads `-0` and integers of larger
//! magnitude as floats, which import keeps (TS-26).
//!
//! The reader reads its file twice, nodes first, then edges (TS-26
//! Records); each line is one `serde_json::from_slice` call.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value as Json};

use super::ExportSummary;
use super::export::{ContentsMap, EdgeOut, NodeOut, Rows, each_edge_tx, each_node};
use super::files::{Reader, Writer, io_error};
use super::model::{EdgeIn, Invalid, NodeIn, Pos, Sink, Source, lexical_error};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::storage::Storage;
use crate::storage::engine::Engine;
use crate::storage::graph::Graph;
use crate::sync::Arc;
use crate::tx::Transaction;
use crate::types::Value;
use crate::types::lexical::{Contents, NoContents, TypeName, lexical_with, parse};

const TYPE: &str = "$lpg_type";
const VALUE: &str = "$value";

/// Whether a list of element type `t` is a JSON array (TS-26 table).
fn array_element(t: &TypeName) -> bool {
    matches!(
        t,
        TypeName::Boolean
            | TypeName::Integer
            | TypeName::Float
            | TypeName::String
            | TypeName::Date
            | TypeName::LocalTime
            | TypeName::ZonedTime
            | TypeName::LocalDateTime
            | TypeName::ZonedDateTime
            | TypeName::Point
    )
}

fn tagged(v: &Value, c: &dyn Contents) -> Json {
    let mut m = Map::new();
    m.insert(TYPE.into(), Json::String(TypeName::of(v).to_string()));
    m.insert(VALUE.into(), Json::String(lexical_with(v, c)));
    Json::Object(m)
}

/// The JSON form of `v` (module documentation).
pub(crate) fn to_json(v: &Value, c: &dyn Contents) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Float(x) if x.is_finite() => {
            serde_json::Number::from_f64(*x).map_or_else(|| tagged(v, c), Json::Number)
        }
        Value::String(s) => Json::String(s.to_string()),
        Value::List(items) => match TypeName::of(v) {
            TypeName::List(Some(t)) if array_element(&t) => {
                Json::Array(items.iter().map(|i| to_json(i, c)).collect())
            }
            _ => tagged(v, c),
        },
        _ => tagged(v, c),
    }
}

/// The value of the JSON form `j` (module documentation).
///
/// # Errors
///
/// 22004 for `null`, 22003 for an integer outside the `i64` range, 22N05
/// for an object that is not a tagged object or names an unknown type,
/// and the error of [`lexical_error`] for a lexical form that does not
/// read.
pub(crate) fn from_json(j: &Json) -> Result<Value, Error> {
    Ok(match j {
        Json::Null => {
            return Err(Error::new(
                codes::C22004,
                "null value not allowed: a JSON null property value",
            ));
        }
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if n.is_u64() {
                return Err(Error::new(
                    codes::C22003,
                    "numeric value out of range: a JSON integer outside the 64-bit range",
                ));
            } else {
                Value::Float(n.as_f64().unwrap_or(f64::NAN))
            }
        }
        Json::String(s) => Value::string(s),
        Json::Array(items) => Value::List(
            items
                .iter()
                .map(from_json)
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        ),
        Json::Object(m) => {
            let (Some(Json::String(t)), Some(Json::String(text)), 2) =
                (m.get(TYPE), m.get(VALUE), m.len())
            else {
                return Err(super::invalid_input(
                    "a JSON object that is not a tagged object",
                ));
            };
            let t =
                TypeName::parse(t).ok_or_else(|| super::invalid_input("an unknown type name"))?;
            parse(&t, text).ok_or_else(|| lexical_error(&t))?
        }
    })
}

fn properties(props: &[(String, Value)], c: &dyn Contents) -> Json {
    Json::Object(
        props
            .iter()
            .map(|(k, v)| (k.clone(), to_json(v, c)))
            .collect(),
    )
}

fn labels_json(labels: &[String]) -> Json {
    Json::Array(labels.iter().map(|l| Json::String(l.clone())).collect())
}

/// The object of node `n` (APOC layout).
pub(crate) fn node_object(n: &NodeOut) -> Json {
    let mut m = Map::new();
    m.insert("type".into(), Json::String("node".into()));
    m.insert("id".into(), Json::String(n.id.to_string()));
    if !n.labels.is_empty() {
        m.insert("labels".into(), labels_json(&n.labels));
    }
    if !n.props.is_empty() {
        m.insert("properties".into(), properties(&n.props, &NoContents));
    }
    Json::Object(m)
}

/// The object of edge `e`, its endpoints with labels `start` and `end`
/// (APOC layout).
pub(crate) fn edge_object(e: &EdgeOut, start: &[String], end: &[String]) -> Json {
    let end_point = |id: u64, labels: &[String]| {
        let mut m = Map::new();
        m.insert("id".into(), Json::String(id.to_string()));
        if !labels.is_empty() {
            m.insert("labels".into(), labels_json(labels));
        }
        Json::Object(m)
    };
    let mut m = Map::new();
    m.insert("type".into(), Json::String("relationship".into()));
    m.insert("id".into(), Json::String(e.id.to_string()));
    m.insert("label".into(), Json::String(e.ty.clone()));
    if !e.props.is_empty() {
        m.insert("properties".into(), properties(&e.props, &NoContents));
    }
    m.insert("start".into(), end_point(e.src, start));
    m.insert("end".into(), end_point(e.dst, end));
    Json::Object(m)
}

fn line(w: &mut Writer, j: &Json) -> Result<(), Error> {
    serde_json::to_writer(&mut *w, j).map_err(|e| io_error(e.into()))?;
    w.write_all(b"\n").map_err(io_error)
}

/// Writes graph `g` as JSON Lines: its nodes, then its edges.
///
/// # Errors
///
/// Storage and I/O errors.
pub(crate) fn write_graph(
    engine: &Engine,
    tx: &mut Transaction,
    g: &Graph,
    w: &mut Writer,
    summary: &mut ExportSummary,
) -> Result<(), Error> {
    each_node(engine, tx, g, &mut |n| {
        summary.nodes += 1;
        line(w, &node_object(&n))
    })?;
    // The endpoints' labels, read per edge in the same snapshot.
    let label_names = |tx: &mut Transaction, id: u64| -> Result<Vec<String>, Error> {
        Ok(match tx.node(g, id)? {
            Some((labels, _)) => labels
                .into_iter()
                .filter_map(|t| engine.token_name(t).map(|(_, n)| n))
                .collect(),
            None => Vec::new(),
        })
    };
    each_edge_tx(engine, tx, g, &mut |tx, e| {
        let (s, t) = (label_names(tx, e.src)?, label_names(tx, e.dst)?);
        summary.edges += 1;
        line(w, &edge_object(&e, &s, &t))
    })
}

/// Writes the rows of a query result, one object per row, its keys the
/// result's columns (TS-26 Query results).
///
/// # Errors
///
/// The statement's errors; I/O errors.
pub(crate) fn write_rows(
    rows: &mut dyn Rows,
    w: &mut Writer,
    summary: &mut ExportSummary,
) -> Result<(), Error> {
    let columns = rows.columns();
    let mut contents = ContentsMap::default();
    while let Some(row) = rows.next_row(&mut contents)? {
        let m: Map<String, Json> = columns
            .iter()
            .zip(&row)
            .map(|(c, v)| (c.clone(), to_json(v, &contents)))
            .collect();
        line(w, &Json::Object(m))?;
        summary.rows += 1;
        contents = ContentsMap::default();
    }
    Ok(())
}

/// The callback of [`each_line`]: a line's number and its object, or an
/// invalid record.
pub(crate) type LineFn<'a> =
    dyn FnMut(u64, Result<Map<String, Json>, Invalid>) -> Result<(), Error> + 'a;

/// A JSON Lines input file.
pub(crate) struct Input<'s> {
    storage: &'s dyn Storage,
    location: PathBuf,
    name: Arc<str>,
}

impl<'s> Input<'s> {
    pub(crate) fn new(storage: &'s dyn Storage, location: &Path, name: &str) -> Input<'s> {
        Input {
            storage,
            location: location.to_path_buf(),
            name: Arc::from(name),
        }
    }

    /// Calls `f` with each non-empty line's number and object.
    fn lines(&self, f: &mut LineFn<'_>) -> Result<(), Error> {
        let r = Reader::open(self.storage, &self.location, &self.name)?;
        each_line(r, &self.name, f)
    }
}

/// Calls `f` with each non-empty line of `r` and its object, or an
/// invalid record for a line that is not a JSON object (the reader of
/// [`Input`] and of the `jsonl_read` fuzz target).
pub(crate) fn each_line(
    mut r: impl BufRead,
    name: &Arc<str>,
    f: &mut LineFn<'_>,
) -> Result<(), Error> {
    let mut buf = Vec::new();
    let mut n = 0u64;
    loop {
        buf.clear();
        if r.read_until(b'\n', &mut buf).map_err(io_error)? == 0 {
            return Ok(());
        }
        n += 1;
        let text = buf.strip_suffix(b"\n").unwrap_or(&buf);
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        if text.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let item = match serde_json::from_slice::<Json>(text) {
            Ok(Json::Object(m)) => Ok(m),
            _ => Err(Invalid::new(
                name,
                Pos::Line(n),
                None,
                codes::C22N05,
                "input failed validation: a line that is not a JSON object",
            )),
        };
        f(n, item)?;
    }
}

/// The identifier of a JSON `id` field: a string, or a number written as
/// text.
fn id_of(j: Option<&Json>) -> Option<String> {
    match j? {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn props_of(m: &Map<String, Json>) -> Result<Vec<(String, Value)>, Error> {
    match m.get("properties") {
        None => Ok(Vec::new()),
        Some(Json::Object(p)) => p
            .iter()
            .map(|(k, v)| Ok((k.clone(), from_json(v)?)))
            .collect(),
        Some(_) => Err(super::invalid_input(
            "properties that are not a JSON object",
        )),
    }
}

/// The node record of object `m` on line `n`.
pub(crate) fn node_of(name: &Arc<str>, n: u64, m: &Map<String, Json>) -> Result<NodeIn, Invalid> {
    let id = id_of(m.get("id"));
    let bad = |e: Error| Invalid {
        file: name.clone(),
        pos: Pos::Line(n),
        id: id.clone(),
        error: Box::new(e),
    };
    let labels = match m.get("labels") {
        None => Vec::new(),
        Some(Json::Array(a)) => a
            .iter()
            .map(|l| l.as_str().map(str::to_owned))
            .collect::<Option<_>>()
            .ok_or_else(|| bad(super::invalid_input("a label that is not a string")))?,
        Some(_) => return Err(bad(super::invalid_input("labels that are not an array"))),
    };
    let props = props_of(m).map_err(bad)?;
    Ok(NodeIn {
        file: name.clone(),
        pos: Pos::Line(n),
        id: id.clone(),
        space: String::new(),
        labels,
        props,
    })
}

/// The edge record of object `m` on line `n`.
pub(crate) fn edge_of(name: &Arc<str>, n: u64, m: &Map<String, Json>) -> Result<EdgeIn, Invalid> {
    let id = id_of(m.get("id"));
    let bad = |e: Error| Invalid {
        file: name.clone(),
        pos: Pos::Line(n),
        id: id.clone(),
        error: Box::new(e),
    };
    let end = |k: &str| match m.get(k) {
        Some(Json::Object(o)) => id_of(o.get("id")),
        _ => None,
    };
    let (Some(start), Some(stop)) = (end("start"), end("end")) else {
        return Err(bad(super::invalid_input(
            "an edge without start or end identifier",
        )));
    };
    let Some(Json::String(ty)) = m.get("label") else {
        return Err(bad(super::invalid_input("an edge without a type")));
    };
    let props = props_of(m).map_err(bad)?;
    Ok(EdgeIn {
        file: name.clone(),
        pos: Pos::Line(n),
        id: id.clone(),
        start: (String::new(), start),
        end: (String::new(), stop),
        ty: ty.clone(),
        props,
    })
}

/// The kind of object `m`: `Some(true)` for a node, `Some(false)` for an
/// edge, `None` otherwise.
fn kind(m: &Map<String, Json>) -> Option<bool> {
    match m.get("type")?.as_str()? {
        "node" => Some(true),
        "relationship" => Some(false),
        _ => None,
    }
}

impl Source for Input<'_> {
    fn nodes(&mut self, f: &mut Sink<'_, NodeIn>) -> Result<(), Error> {
        let name = self.name.clone();
        self.lines(&mut |n, item| match item {
            Err(inv) => f(Err(inv)),
            Ok(m) => match kind(&m) {
                Some(true) => f(node_of(&name, n, &m)),
                Some(false) => Ok(()),
                None => f(Err(Invalid::new(
                    &name,
                    Pos::Line(n),
                    None,
                    codes::C22N05,
                    "input failed validation: an object that is neither a node nor a relationship",
                ))),
            },
        })
    }

    fn edges(&mut self, f: &mut Sink<'_, EdgeIn>) -> Result<(), Error> {
        let name = self.name.clone();
        self.lines(&mut |n, item| match item {
            Ok(m) if kind(&m) == Some(false) => f(edge_of(&name, n, &m)),
            // Reported by the node pass.
            _ => Ok(()),
        })
    }
}

/// Reads `data` as a JSON Lines input, as an import does (the
/// `jsonl_read` fuzz target): never panics.
#[cfg(any(test, fuzzing))]
pub(crate) fn read_all(data: &[u8]) -> u64 {
    let name: Arc<str> = Arc::from("fuzz");
    let mut n = 0u64;
    let _ = each_line(data, &name, &mut |line, item| {
        if let Ok(m) = item {
            let ok = match kind(&m) {
                Some(true) => node_of(&name, line, &m).is_ok(),
                Some(false) => edge_of(&name, line, &m).is_ok(),
                None => false,
            };
            n += u64::from(ok);
        }
        Ok(())
    });
    n
}
