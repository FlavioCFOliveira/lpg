//! GraphML (TS-26 Export layouts; FR-IMP-001, FR-IMP-006; rmp #59).
//!
//! **Layout**: APOC's GraphML layout without its non-standard attributes
//! (R-APOC @ `e52bc20`
//! `core/src/main/java/apoc/export/graphml/XmlGraphMLWriter.java:129-170`,
//! Apache-2.0, `Copyright (c) "Neo4j" Neo4j Sweden AB [http://neo4j.com]`;
//! notice in `LICENSE-APACHE`; the layout is ported): one `<key>` per
//! property name and element kind; node labels only in the data key
//! `labels`, written `:A:B`, and the edge type only in the data key
//! `label`, `\` escaping `:` and `\` in both. `<node>` and `<edge>` carry
//! no `labels` or `label` attribute, which the GraphML 1.0 schema rejects.
//!
//! **Types** (TS-26 table): a uniform boolean, integer, finite float or
//! string property has `attr.type` `boolean`, `long`, `double` or
//! `string`; any other uniform property is `attr.type="string"` with the
//! lpg type attribute on its `<key>`; a mixed property is
//! `attr.type="string"` with the type attribute on each `<data>`. The
//! attribute is `type` in the namespace `https://crates.io/crates/lpg/graphml`
//! ([`LPG_NS`]), declared by the extension schema `graphml-lpg.xsd` and
//! `graphml-lpg-type.xsd` beside this file (TS-26 item B). A reader that
//! ignores it reads a string.
//!
//! **XML characters** (W3C XML 1.0 §2.2 `Char`): a string value that holds
//! a character XML excludes is written annotated as `STRING` in its
//! escaped lexical form; a label, edge type or property name that holds
//! one fails the export (22N11). `\r` is written `&#13;` (quick-xml
//! `escape`), and the reader applies XML 1.0 end-of-line handling.
//!
//! **Reader**: the key table, `attr.type` mapping, `<default>`, boolean
//! literals and empty `<data>` are ported from NetworkX's
//! `readwrite/graphml.py` (3.7, lines 436-500 and 1001-1093). NetworkX is
//! distributed under the 3-clause BSD licence:
//!
//! > Copyright (c) 2004-2026, NetworkX Developers
//! > Aric Hagberg <hagberg@lanl.gov>
//! > Dan Schult <dschult@colgate.edu>
//! > Pieter Swart <swart@lanl.gov>
//! > All rights reserved.
//! >
//! > Redistribution and use in source and binary forms, with or without
//! > modification, are permitted provided that the following conditions
//! > are met:
//! >
//! >   * Redistributions of source code must retain the above copyright
//! >     notice, this list of conditions and the following disclaimer.
//! >
//! >   * Redistributions in binary form must reproduce the above copyright
//! >     notice, this list of conditions and the following disclaimer in the
//! >     documentation and/or other materials provided with the
//! >     distribution.
//! >
//! >   * Neither the name of the NetworkX Developers nor the names of its
//! >     contributors may be used to endorse or promote products derived
//! >     from this software without specific prior written permission.
//! >
//! > THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
//! > "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
//! > LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
//! > A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
//! > OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//! > SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
//! > LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
//! > DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
//! > THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
//! > (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
//! > OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//!
//! The file is read twice, nodes first, then edges: GraphML allows an edge
//! before its nodes, and NetworkX reads every node before any edge
//! (`readwrite/graphml.py:902-960`).

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

use super::ExportSummary;
use super::export::{Analysis, ContentsMap, EdgeOut, NodeOut, Rows, Seen};
use super::files::{Reader, Writer, io_error};
use super::model::{EdgeIn, Invalid, NodeIn, Pos, Sink, Source, lexical_error};
use crate::error::Error;
use crate::storage::Storage;
use crate::storage::btree::node::put_varint;
use crate::storage::engine::Engine;
use crate::sync::Arc;
use crate::types::Value;
use crate::types::lexical::{TypeName, float_text, lexical, parse};

/// The GraphML namespace.
pub(crate) const GRAPHML_NS: &str = "http://graphml.graphdrawing.org/xmlns";
/// The namespace of lpg's type attribute (TS-26).
pub(crate) const LPG_NS: &str = "https://crates.io/crates/lpg/graphml";

/// Whether XML 1.0 `Char` excludes `c` (§2.2).
pub(crate) fn xml_excluded(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// `s` escaped for XML text content: `&`, `<`, `>`, `'`, `"` and `\r`.
fn text(s: &str) -> std::borrow::Cow<'_, str> {
    quick_xml::escape::escape(s)
}

/// `s` escaped for an attribute value: as [`text`], and tab and line feed
/// as character references, which attribute-value normalisation would
/// otherwise turn into spaces (XML 1.0 §3.3.3).
fn attr(s: &str) -> String {
    text(s).replace('\t', "&#9;").replace('\n', "&#10;")
}

/// How a key's values are written.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyKind {
    /// A GraphML `attr.type`.
    Native(&'static str),
    /// `attr.type="string"`, the type attribute on the key.
    Typed(TypeName),
    /// `attr.type="string"`, the type attribute on each value.
    Mixed,
}

fn key_kind(s: &Seen) -> KeyKind {
    match s.uniform() {
        Some(TypeName::Boolean) => KeyKind::Native("boolean"),
        Some(TypeName::Integer) => KeyKind::Native("long"),
        Some(TypeName::Float) if !s.non_finite => KeyKind::Native("double"),
        Some(TypeName::String) => KeyKind::Native("string"),
        Some(t) => KeyKind::Typed(t.clone()),
        None => KeyKind::Mixed,
    }
}

/// Fails with 22N11 if `name` holds a character XML excludes.
fn check_name(what: &str, name: &str) -> Result<(), Error> {
    if name.chars().any(xml_excluded) {
        return Err(super::invalid_argument(&format!(
            "the {what} {name:?} holds a character XML 1.0 excludes"
        )));
    }
    Ok(())
}

/// `:A:B`, `\` escaping `:` and `\` (TS-26).
fn labels_text(labels: &[String]) -> String {
    let mut out = String::new();
    for l in labels {
        out.push(':');
        for c in l.chars() {
            if c == ':' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

/// Writes a GraphML document: its keys, then nodes and edges.
pub(crate) struct GraphWriter<'w> {
    w: &'w mut Writer,
    node_keys: BTreeMap<String, (String, KeyKind)>,
    edge_keys: BTreeMap<String, (String, KeyKind)>,
}

fn put(w: &mut Writer, s: &str) -> Result<(), Error> {
    w.write_all(s.as_bytes()).map_err(io_error)
}

impl<'w> GraphWriter<'w> {
    /// Writes the header and the keys of the properties `a` lists.
    ///
    /// # Errors
    ///
    /// 22N11 for a property name XML cannot hold; I/O errors.
    pub(crate) fn start(w: &'w mut Writer, a: &Analysis) -> Result<GraphWriter<'w>, Error> {
        put(
            w,
            &format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<graphml xmlns=\"{GRAPHML_NS}\" \
                 xmlns:lpg=\"{LPG_NS}\">\n<key id=\"labels\" for=\"node\" attr.name=\"labels\" \
                 attr.type=\"string\"/>\n<key id=\"label\" for=\"edge\" attr.name=\"label\" \
                 attr.type=\"string\"/>\n"
            ),
        )?;
        let mut n = 0usize;
        let mut keys = |props: &BTreeMap<String, Seen>, domain: &str, w: &mut Writer| {
            let mut out = BTreeMap::new();
            for (name, seen) in props {
                check_name("property name", name)?;
                let id = format!("k{n}");
                n += 1;
                let kind = key_kind(seen);
                let (ty, lpg) = match &kind {
                    KeyKind::Native(t) => (*t, String::new()),
                    KeyKind::Typed(t) => {
                        ("string", format!(" lpg:type=\"{}\"", attr(&t.to_string())))
                    }
                    KeyKind::Mixed => ("string", String::new()),
                };
                put(
                    w,
                    &format!(
                        "<key id=\"{id}\" for=\"{domain}\" attr.name=\"{}\" attr.type=\"{ty}\"{lpg}/>\n",
                        attr(name)
                    ),
                )?;
                out.insert(name.clone(), (id, kind));
            }
            Ok::<_, Error>(out)
        };
        let node_keys = keys(&a.node, "node", w)?;
        let edge_keys = keys(&a.edge, "edge", w)?;
        put(w, "<graph id=\"G\" edgedefault=\"directed\">\n")?;
        Ok(GraphWriter {
            w,
            node_keys,
            edge_keys,
        })
    }

    fn data(&mut self, keys: bool, name: &str, v: &Value) -> Result<(), Error> {
        let map = if keys {
            &self.node_keys
        } else {
            &self.edge_keys
        };
        let Some((id, kind)) = map.get(name) else {
            return Err(crate::api::internal("a property without a GraphML key"));
        };
        let (ty, body): (Option<String>, String) = match kind {
            KeyKind::Native("boolean") => (
                None,
                if v == &Value::Bool(true) {
                    "true"
                } else {
                    "false"
                }
                .into(),
            ),
            KeyKind::Native("long") | KeyKind::Native("double") => (
                None,
                match v {
                    Value::Float(x) => float_text(*x),
                    v => lexical(v),
                },
            ),
            KeyKind::Native(_) => match v {
                Value::String(s) if s.chars().any(xml_excluded) => {
                    (Some("STRING".into()), lexical(v))
                }
                Value::String(s) => (None, s.to_string()),
                v => (Some(TypeName::of(v).to_string()), lexical(v)),
            },
            KeyKind::Typed(_) => (None, lexical(v)),
            KeyKind::Mixed => (Some(TypeName::of(v).to_string()), lexical(v)),
        };
        let id = id.clone();
        let lpg = ty.map_or_else(String::new, |t| format!(" lpg:type=\"{}\"", attr(&t)));
        put(
            self.w,
            &format!("<data key=\"{id}\"{lpg}>{}</data>", text(&body)),
        )
    }

    /// Writes node `n`.
    ///
    /// # Errors
    ///
    /// 22N11 for a label XML cannot hold; I/O errors.
    pub(crate) fn node(&mut self, n: &NodeOut) -> Result<(), Error> {
        put(self.w, &format!("<node id=\"n{}\">", n.id))?;
        if !n.labels.is_empty() {
            for l in &n.labels {
                check_name("label", l)?;
            }
            put(
                self.w,
                &format!(
                    "<data key=\"labels\">{}</data>",
                    text(&labels_text(&n.labels))
                ),
            )?;
        }
        for (k, v) in &n.props {
            self.data(true, k, v)?;
        }
        put(self.w, "</node>\n")
    }

    /// Writes edge `e`.
    ///
    /// # Errors
    ///
    /// 22N11 for an edge type XML cannot hold; I/O errors.
    pub(crate) fn edge(&mut self, e: &EdgeOut) -> Result<(), Error> {
        check_name("edge type", &e.ty)?;
        put(
            self.w,
            &format!(
                "<edge id=\"e{}\" source=\"n{}\" target=\"n{}\"><data key=\"label\">{}</data>",
                e.id,
                e.src,
                e.dst,
                text(&labels_text(std::slice::from_ref(&e.ty))[1..])
            ),
        )?;
        for (k, v) in &e.props {
            self.data(false, k, v)?;
        }
        put(self.w, "</edge>\n")
    }

    /// Writes the end of the document.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub(crate) fn finish(self) -> Result<(), Error> {
        put(self.w, "</graph>\n</graphml>\n")
    }
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// The sort record of the properties `props`: their names, then the TS-21
/// record encoding of their values.
fn encode_props(props: &[(String, Value)], out: &mut Vec<u8>) -> Result<(), Error> {
    put_varint(out, props.len() as u64);
    for (k, _) in props {
        put_bytes(out, k.as_bytes());
    }
    let values: Vec<(u64, Value)> = props
        .iter()
        .enumerate()
        .map(|(i, (_, v))| (i as u64, v.clone()))
        .collect();
    crate::types::record::encode(&[], &values, out)
        .map_err(|_| super::invalid_argument("a value too large to export"))
}

/// A reader of the sort records [`write_rows`] writes.
struct Cur<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    fn varint(&mut self) -> u64 {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.b[self.at];
            self.at += 1;
            v |= u64::from(b & 0x7F) << shift;
            if b < 0x80 {
                return v;
            }
            shift += 7;
        }
    }

    fn string(&mut self) -> String {
        let n = self.varint() as usize;
        let s = String::from_utf8_lossy(&self.b[self.at..self.at + n]).into_owned();
        self.at += n;
        s
    }

    fn props(&mut self) -> Vec<(String, Value)> {
        let n = self.varint() as usize;
        let names: Vec<String> = (0..n).map(|_| self.string()).collect();
        let (_, values) =
            crate::types::record::decode(&self.b[self.at..]).expect("encoded by the export");
        names
            .into_iter()
            .zip(values)
            .map(|(k, (_, v))| (k, v))
            .collect()
    }
}

/// Writes the nodes and edges a query result holds, also inside paths,
/// lists and records, and leaves out its other values (TS-26 Query
/// results). They are sorted first, which removes duplicates and puts the
/// nodes before the edges, and their keys are typed on the way.
///
/// # Errors
///
/// The statement's errors; as [`GraphWriter`].
pub(crate) fn write_rows(
    engine: &Engine,
    rows: &mut dyn Rows,
    w: &mut Writer,
    summary: &mut ExportSummary,
) -> Result<(), Error> {
    let memory =
        (engine.core().accountant.cap() / 64).clamp(crate::storage::sort::MIN_MEMORY, 16 << 20);
    let mut sort = engine.sorter(memory)?;
    let mut a = Analysis::default();
    let mut contents = ContentsMap::default();
    while let Some(row) = rows.next_row(&mut contents)? {
        let (mut ns, mut es) = (Vec::new(), Vec::new());
        for v in &row {
            super::export::elements(v, &mut ns, &mut es);
        }
        for id in ns {
            let (labels, props) = contents.nodes.get(&id).cloned().unwrap_or_default();
            let n = NodeOut { id, labels, props };
            a.node(&n);
            let mut b = Vec::new();
            put_varint(&mut b, n.labels.len() as u64);
            for l in &n.labels {
                put_bytes(&mut b, l.as_bytes());
            }
            encode_props(&n.props, &mut b)?;
            let mut k = vec![0u8];
            k.extend_from_slice(&id.to_be_bytes());
            sort.push(&k, &b)?;
        }
        for id in es {
            let Some((ty, src, dst, props)) = contents.edges.get(&id).cloned() else {
                continue;
            };
            let e = EdgeOut {
                id,
                src,
                dst,
                ty,
                props,
            };
            a.edge(&e);
            let mut b = Vec::new();
            put_bytes(&mut b, e.ty.as_bytes());
            put_varint(&mut b, src);
            put_varint(&mut b, dst);
            encode_props(&e.props, &mut b)?;
            let mut k = vec![1u8];
            k.extend_from_slice(&id.to_be_bytes());
            sort.push(&k, &b)?;
        }
        contents = ContentsMap::default();
    }
    let mut gw = GraphWriter::start(w, &a)?;
    let mut sorted = sort.finish()?;
    let mut last: Option<Vec<u8>> = None;
    while let Some((k, v)) = sorted.next_record()? {
        if last.as_deref() == Some(k) {
            continue;
        }
        last = Some(k.to_vec());
        let id = u64::from_be_bytes(k[1..].try_into().expect("8 bytes"));
        let mut c = Cur { b: v, at: 0 };
        if k[0] == 0 {
            let n = c.varint() as usize;
            let labels = (0..n).map(|_| c.string()).collect();
            let props = c.props();
            gw.node(&NodeOut { id, labels, props })?;
            summary.nodes += 1;
        } else {
            let ty = c.string();
            let (src, dst) = (c.varint(), c.varint());
            let props = c.props();
            gw.edge(&EdgeOut {
                id,
                src,
                dst,
                ty,
                props,
            })?;
            summary.edges += 1;
        }
    }
    gw.finish()
}

// ---------------------------------------------------------------------------
// Import.

/// A `<key>` of the file read.
#[derive(Debug, Clone)]
struct Key {
    name: String,
    /// NetworkX's `attr.type`, `string` when absent.
    ty: String,
    /// The lpg type attribute of the key.
    lpg: Option<String>,
    /// The `<default>` value.
    default: Option<String>,
}

/// An element being read: a node or an edge.
#[derive(Debug, Default)]
struct Element {
    edge: bool,
    ordinal: u64,
    id: Option<String>,
    source: Option<String>,
    target: Option<String>,
    /// `(key id, lpg type attribute, text)` of each `<data>`.
    data: Vec<(String, Option<String>, String)>,
}

/// What the parser is inside.
enum Inside {
    Nothing,
    Key(String, Key),
    Default(String, Key, String),
    Element(Element),
    Data(Element, String, Option<String>, String),
}

fn malformed(e: impl std::fmt::Display) -> Error {
    let _ = e;
    super::invalid_input("a malformed GraphML document")
}

/// Whether a resolved name is in the GraphML namespace, or in none.
fn graphml(ns: &ResolveResult<'_>) -> bool {
    match ns {
        ResolveResult::Bound(n) => n.as_ref() == GRAPHML_NS,
        ResolveResult::Unbound => true,
        ResolveResult::Unknown(_) => false,
    }
}

/// Reads a GraphML document from `r`, calling `node` and `edge` for each
/// element in document order, with the key table read so far.
/// The callback of [`parse_document`]: the key table and an element.
type ElementFn<'a> = dyn FnMut(&HashMap<String, Key>, Element) -> Result<(), Error> + 'a;

/// An element's labels, edge type and properties.
type Decoded = (Vec<String>, Option<String>, Vec<(String, Value)>);

fn parse_document(r: impl BufRead, on: &mut ElementFn<'_>) -> Result<(), Error> {
    let mut reader = quick_xml::NsReader::from_reader(r);
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut keys: HashMap<String, Key> = HashMap::new();
    let mut inside = Inside::Nothing;
    let mut ordinal = 0u64;
    loop {
        let (ns, ev) = reader
            .read_resolved_event_into(&mut buf)
            .map_err(malformed)?;
        let in_graphml = graphml(&ns);
        match ev {
            Event::Eof => {
                return match inside {
                    Inside::Nothing => Ok(()),
                    _ => Err(malformed("unexpected end")),
                };
            }
            Event::Start(e) => {
                let local = e.local_name();
                let local = local.as_ref();
                let mut attrs: HashMap<(bool, String), String> = HashMap::new();
                for a in e.attributes() {
                    let a = a.map_err(malformed)?;
                    let (ans, an) = reader.resolver().resolve_attribute(a.key);
                    let lpg = matches!(&ans, ResolveResult::Bound(n) if n.as_ref() == LPG_NS);
                    let v = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(malformed)?
                        .into_owned();
                    attrs.insert((lpg, an.as_ref().to_owned()), v);
                }
                let get = |k: &str| attrs.get(&(false, k.to_owned())).cloned();
                let lpg = attrs.get(&(true, "type".to_owned())).cloned();
                inside = match (inside, in_graphml, local) {
                    (Inside::Nothing, true, "key") => {
                        let id = get("id").ok_or_else(|| malformed("key without id"))?;
                        let name = get("attr.name").ok_or_else(|| malformed("key without name"))?;
                        let ty = get("attr.type").unwrap_or_else(|| "string".into());
                        if !matches!(
                            ty.as_str(),
                            "string" | "int" | "integer" | "long" | "float" | "double" | "boolean"
                        ) {
                            return Err(super::invalid_input("an unknown GraphML attr.type"));
                        }
                        Inside::Key(
                            id,
                            Key {
                                name,
                                ty,
                                lpg,
                                default: None,
                            },
                        )
                    }
                    (Inside::Key(id, k), true, "default") => Inside::Default(id, k, String::new()),
                    (Inside::Nothing, true, "node" | "edge") => {
                        ordinal += 1;
                        Inside::Element(Element {
                            edge: local == "edge",
                            ordinal,
                            id: get("id"),
                            source: get("source"),
                            target: get("target"),
                            data: Vec::new(),
                        })
                    }
                    (Inside::Element(el), true, "data") => {
                        let key = get("key").ok_or_else(|| malformed("data without key"))?;
                        Inside::Data(el, key, lpg, String::new())
                    }
                    // Any other element (the graph, nested graphs, ports,
                    // yFiles extensions) is passed through.
                    (other, _, _) => other,
                };
            }
            Event::Text(t) => append(&mut inside, &t.xml10_content()),
            Event::CData(t) => append(&mut inside, &t.xml10_content()),
            Event::GeneralRef(r) => {
                let s = match r.resolve_char_ref().map_err(malformed)? {
                    Some(c) => c.to_string(),
                    None => match r.xml10_content().as_ref() {
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "amp" => "&".into(),
                        "apos" => "'".into(),
                        "quot" => "\"".into(),
                        _ => return Err(malformed("an unknown entity")),
                    },
                };
                append(&mut inside, &s);
            }
            Event::End(e) => {
                let local = e.local_name();
                inside = match (inside, local.as_ref()) {
                    (Inside::Key(id, k), "key") => {
                        keys.insert(id, k);
                        Inside::Nothing
                    }
                    (Inside::Default(id, mut k, text), "default") => {
                        k.default = Some(text);
                        Inside::Key(id, k)
                    }
                    (Inside::Element(el), "node" | "edge") => {
                        on(&keys, el)?;
                        Inside::Nothing
                    }
                    (Inside::Data(mut el, key, lpg, text), "data") => {
                        el.data.push((key, lpg, text));
                        Inside::Element(el)
                    }
                    (other, _) => other,
                };
            }
            _ => {}
        }
        buf.clear();
    }
}

fn append(inside: &mut Inside, s: &str) {
    match inside {
        Inside::Default(_, _, t) | Inside::Data(_, _, _, t) => t.push_str(s),
        _ => {}
    }
}

/// The value of `text` under key `k`, with the data's own lpg type
/// attribute `lpg` (module documentation).
fn data_value(k: &Key, lpg: Option<&str>, text: &str) -> Result<Value, Error> {
    if let Some(t) = lpg.or(k.lpg.as_deref()) {
        let t = TypeName::parse(t).ok_or_else(|| super::invalid_input("an unknown type name"))?;
        return parse(&t, text).ok_or_else(|| lexical_error(&t));
    }
    let err = |t: TypeName| lexical_error(&t);
    Ok(match k.ty.as_str() {
        "boolean" => match text.to_ascii_lowercase().as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => return Err(err(TypeName::Boolean)),
        },
        "int" | "integer" | "long" => {
            Value::Int(text.trim().parse().map_err(|_| err(TypeName::Integer))?)
        }
        "float" | "double" => Value::Float(match text.trim() {
            "NaN" => f64::NAN,
            "Infinity" | "INF" | "inf" => f64::INFINITY,
            "-Infinity" | "-INF" | "-inf" => f64::NEG_INFINITY,
            t => t.parse().map_err(|_| err(TypeName::Float))?,
        }),
        _ => Value::string(text),
    })
}

/// The labels of `:A:B`, `\` escaping `:` and `\`.
fn parse_labels(text: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in text.chars() {
        if esc {
            out.last_mut().expect("one").push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == ':' {
            out.push(String::new());
        } else {
            out.last_mut().expect("one").push(c);
        }
    }
    if text.starts_with(':') {
        out.remove(0);
    }
    out
}

/// The type and properties of element `el` under `keys`: the special keys
/// `labels` (nodes) and `label` (edges), the other keys by name, and each
/// key's default for an element without that data.
fn decode(keys: &HashMap<String, Key>, el: &Element) -> Result<Decoded, Error> {
    let mut labels = Vec::new();
    let mut ty = None;
    let mut props: Vec<(String, Value)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (key, lpg, text) in &el.data {
        let k = keys
            .get(key)
            .ok_or_else(|| super::invalid_input("a data element of no declared key"))?;
        seen.push(key);
        if key == "labels" && !el.edge {
            labels = parse_labels(text);
        } else if key == "label" && el.edge {
            ty = Some(parse_labels(text).concat());
        } else {
            let v = data_value(k, lpg.as_deref(), text)?;
            props.retain(|(n, _)| *n != k.name);
            props.push((k.name.clone(), v));
        }
    }
    let mut defaults: Vec<(&String, &Key)> = keys
        .iter()
        .filter(|(id, k)| k.default.is_some() && !seen.contains(&id.as_str()))
        .collect();
    defaults.sort_by_key(|(id, _)| id.as_str());
    for (id, k) in defaults {
        let text = k.default.as_deref().unwrap_or_default();
        if id == "labels" && !el.edge {
            labels = parse_labels(text);
        } else if id == "label" && el.edge {
            ty = Some(parse_labels(text).concat());
        } else if id != "labels" && id != "label" && !props.iter().any(|(n, _)| *n == k.name) {
            props.push((k.name.clone(), data_value(k, None, text)?));
        }
    }
    Ok((labels, ty, props))
}

/// The node record of element `el` of file `name`.
fn node_record(
    name: &Arc<str>,
    keys: &HashMap<String, Key>,
    el: &Element,
) -> Result<NodeIn, Invalid> {
    let (labels, _, props) = decode(keys, el).map_err(|e| Invalid {
        file: name.clone(),
        pos: Pos::Ordinal(el.ordinal),
        id: el.id.clone(),
        error: Box::new(e),
    })?;
    Ok(NodeIn {
        file: name.clone(),
        pos: Pos::Ordinal(el.ordinal),
        id: el.id.clone(),
        space: String::new(),
        labels,
        props,
    })
}

/// The edge record of element `el` of file `name`.
fn edge_record(
    name: &Arc<str>,
    keys: &HashMap<String, Key>,
    el: &Element,
) -> Result<EdgeIn, Invalid> {
    let invalid = |e: Error| Invalid {
        file: name.clone(),
        pos: Pos::Ordinal(el.ordinal),
        id: el.id.clone(),
        error: Box::new(e),
    };
    let (_, ty, props) = decode(keys, el).map_err(invalid)?;
    let (Some(s), Some(t)) = (&el.source, &el.target) else {
        return Err(invalid(super::invalid_input(
            "an edge without source or target",
        )));
    };
    let Some(ty) = ty else {
        return Err(invalid(super::invalid_input("an edge without a type")));
    };
    Ok(EdgeIn {
        file: name.clone(),
        pos: Pos::Ordinal(el.ordinal),
        id: el.id.clone(),
        start: (String::new(), s.clone()),
        end: (String::new(), t.clone()),
        ty,
        props,
    })
}

/// A GraphML input file.
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
}

impl Source for Input<'_> {
    fn nodes(&mut self, f: &mut Sink<'_, NodeIn>) -> Result<(), Error> {
        let r = Reader::open(self.storage, &self.location, &self.name)?;
        let name = self.name.clone();
        parse_document(r, &mut |keys, el| {
            if el.edge {
                Ok(())
            } else {
                f(node_record(&name, keys, &el))
            }
        })
    }

    fn edges(&mut self, f: &mut Sink<'_, EdgeIn>) -> Result<(), Error> {
        let r = Reader::open(self.storage, &self.location, &self.name)?;
        let name = self.name.clone();
        parse_document(r, &mut |keys, el| {
            if el.edge {
                f(edge_record(&name, keys, &el))
            } else {
                Ok(())
            }
        })
    }
}

/// Reads `data` as a GraphML document, as an import does (the
/// `graphml_read` fuzz target): never panics.
#[cfg(any(test, fuzzing))]
pub(crate) fn read_all(data: &[u8]) -> u64 {
    let name: Arc<str> = Arc::from("fuzz");
    let mut n = 0u64;
    let _ = parse_document(data, &mut |keys, el| {
        let ok = if el.edge {
            edge_record(&name, keys, &el).is_ok()
        } else {
            node_record(&name, keys, &el).is_ok()
        };
        n += u64::from(ok);
        Ok(())
    });
    n
}
