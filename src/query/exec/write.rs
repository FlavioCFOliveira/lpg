//! The data-modifying and catalog-modifying operators (rmp #45, #46):
//! `INSERT`, `SET`, `REMOVE`, the closed-graph-type check at the end of
//! each of them (TS-17, FR-GQL-004), and the catalog statements (GC01,
//! GC04; FR-CAT-001, FR-CAT-004).
//!
//! * A write operator is blocking: it reads all its input, then writes
//!   row by row in the caller's transaction, then emits its rows (module
//!   documentation of [`super`]).
//! * **Closed graph types.** After a write operator on a graph of a closed
//!   type has written every row, each element it created or changed is
//!   checked against the type ([`crate::query::graph_type`]); one that no
//!   element type describes fails the statement with G2000, and the
//!   statement's failure aborts the transaction, so none of its writes
//!   stays. A graph of an open type runs no check ([`super::State::type_checks`]).
//! * **Property values** (IV011): a property is set to a property value;
//!   NULL removes it (Neo4j: setting a property to null removes it); any
//!   other value fails with 22G03.
//! * **Catalog statements** run in the caller's transaction (GP18), each
//!   recording the session's principal as owner (FR-AUTH-002). An inline
//!   graph type of `CREATE GRAPH` becomes a graph type object named
//!   `$graph type of <graph>` in the same schema, dropped with the graph.

use std::collections::BTreeSet;

use super::ops::Queue;
use super::{Batch, Cx, Held, Operator, row_bytes};
use crate::catalog::{Definition, Kind, ROOT, TokenKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::bound::{CatalogOp, GraphTypeOf, GraphTypeSource, InsertPattern, Name, SetItem};
use crate::query::expr::{Slot, eval, invalid_type, type_name};
use crate::query::graph_type::GraphType;
use crate::storage::graph::Graph;
use crate::types::Value;

/// What a write operator writes.
enum Kind_ {
    Insert(InsertPattern),
    Set(Vec<SetItem>),
    Remove(Vec<(Slot, Name)>),
}

/// A write operator.
struct Write {
    child: Box<dyn Operator>,
    graph: usize,
    kind: Kind_,
    out: Queue,
    held: Held,
    done: bool,
}

pub(super) fn insert(
    child: Box<dyn Operator>,
    graph: usize,
    p: InsertPattern,
) -> Box<dyn Operator> {
    write(child, graph, Kind_::Insert(p))
}

pub(super) fn set(
    child: Box<dyn Operator>,
    graph: usize,
    items: Vec<SetItem>,
) -> Box<dyn Operator> {
    write(child, graph, Kind_::Set(items))
}

pub(super) fn remove(
    child: Box<dyn Operator>,
    graph: usize,
    items: Vec<(Slot, Name)>,
) -> Box<dyn Operator> {
    write(child, graph, Kind_::Remove(items))
}

fn write(child: Box<dyn Operator>, graph: usize, kind: Kind_) -> Box<dyn Operator> {
    Box::new(Write {
        child,
        graph,
        kind,
        out: Queue::default(),
        held: Held::default(),
        done: false,
    })
}

/// Fails unless `v` is a property value (IV011).
fn property_value(v: &Value) -> Result<(), Error> {
    let scalar = |v: &Value| {
        matches!(
            v,
            Value::Bool(_)
                | Value::Int(_)
                | Value::Float(_)
                | Value::String(_)
                | Value::Bytes(_)
                | Value::Date(_)
                | Value::LocalTime(_)
                | Value::LocalDateTime(_)
                | Value::ZonedTime(_)
                | Value::ZonedDateTime(_)
                | Value::YearToMonth(_)
                | Value::DayToSecond(_)
                | Value::Point(_)
        )
    };
    let ok = match v {
        Value::List(l) => l
            .iter()
            .all(|x| scalar(x) && std::mem::discriminant(x) == std::mem::discriminant(&l[0])),
        v => scalar(v),
    };
    if ok {
        Ok(())
    } else {
        Err(invalid_type(&format!(
            "{} is not a property value",
            type_name(v)
        )))
    }
}

/// The token of a property key, created if missing.
fn key_token(cx: &mut Cx<'_, '_>, k: &Name) -> Result<u64, Error> {
    match k.token {
        Some(t) => Ok(t),
        None => Ok(cx.tx.token(TokenKind::PropertyKey, &k.name)?),
    }
}

/// The properties `props` evaluated over `row`: NULLs dropped, the others
/// checked.
fn eval_props(
    cx: &mut Cx<'_, '_>,
    props: &[(Name, crate::query::expr::Expr)],
    row: &[Value],
) -> Result<Vec<(u64, Value)>, Error> {
    let mut out = Vec::with_capacity(props.len());
    for (k, e) in props {
        let v = eval(e, row, cx)?;
        if v.is_null() {
            continue;
        }
        property_value(&v)?;
        out.push((key_token(cx, k)?, v));
    }
    Ok(out)
}

fn null_element() -> Error {
    Error::new(
        codes::C22004,
        "null value not allowed: a write to a NULL node or edge",
    )
}

impl Write {
    /// Writes one row; records the elements written.
    fn apply(
        &self,
        cx: &mut Cx<'_, '_>,
        g: &Graph,
        mut row: Vec<Value>,
        written: &mut Vec<Value>,
    ) -> Result<Vec<Value>, Error> {
        match &self.kind {
            Kind_::Insert(p) => {
                let mut ids = Vec::with_capacity(p.nodes.len());
                for n in &p.nodes {
                    if n.existing {
                        match row[n.slot] {
                            Value::Node(id) => ids.push(id),
                            _ => return Err(null_element()),
                        }
                        continue;
                    }
                    let mut labels = Vec::with_capacity(n.labels.len());
                    for l in &n.labels {
                        labels.push(match l.token {
                            Some(t) => t,
                            None => cx.tx.token(TokenKind::Label, &l.name)?,
                        });
                    }
                    let props = eval_props(cx, &n.props, &row)?;
                    let id = cx.tx.create_node(g, &labels, &props)?;
                    cx.st.counters.nodes_created += 1;
                    cx.st.counters.properties_set += props.len() as u64;
                    row[n.slot] = Value::Node(id);
                    written.push(Value::Node(id));
                    ids.push(id);
                }
                for e in &p.edges {
                    let ty = match e.ty.token {
                        Some(t) => t,
                        None => cx.tx.token(TokenKind::EdgeType, &e.ty.name)?,
                    };
                    let props = eval_props(cx, &e.props, &row)?;
                    let id = cx.tx.create_edge(g, ids[e.src], ids[e.dst], ty, &props)?;
                    cx.st.counters.edges_created += 1;
                    cx.st.counters.properties_set += props.len() as u64;
                    row[e.slot] = Value::Edge(id);
                    written.push(Value::Edge(id));
                }
            }
            Kind_::Set(items) => {
                for item in items {
                    match item {
                        SetItem::Prop(s, k, e) => {
                            let v = eval(e, &row, cx)?;
                            if !v.is_null() {
                                property_value(&v)?;
                            }
                            let key = key_token(cx, k)?;
                            change(cx, g, &row[*s], |props| {
                                props.retain(|(x, _)| *x != key);
                                if !v.is_null() {
                                    props.push((key, v.clone()));
                                }
                            })?;
                            cx.st.counters.properties_set += 1;
                            written.push(row[*s].clone());
                        }
                        SetItem::All(s, ps) => {
                            let props = eval_props(cx, ps, &row)?;
                            let n = props.len() as u64;
                            change(cx, g, &row[*s], move |old| *old = props.clone())?;
                            cx.st.counters.properties_set += n;
                            written.push(row[*s].clone());
                        }
                    }
                }
            }
            Kind_::Remove(items) => {
                for (s, k) in items {
                    if let Some(key) = k.token {
                        change(cx, g, &row[*s], |props| props.retain(|(x, _)| *x != key))?;
                    }
                    written.push(row[*s].clone());
                }
            }
        }
        Ok(row)
    }
}

/// Changes the properties of the node or edge `el` of `g` with `f`; a NULL
/// element is left alone (a `SET` on an optional match's NULL).
fn change(
    cx: &mut Cx<'_, '_>,
    g: &Graph,
    el: &Value,
    f: impl FnOnce(&mut Vec<(u64, Value)>),
) -> Result<(), Error> {
    match *el {
        Value::Null => Ok(()),
        Value::Node(id) => {
            let Some((labels, mut props)) = cx.tx.node(g, id)? else {
                return Ok(());
            };
            f(&mut props);
            Ok(cx.tx.set_node(g, id, &labels, &props)?)
        }
        Value::Edge(id) => {
            let Some(mut e) = cx.tx.edge(g, id)? else {
                return Ok(());
            };
            f(&mut e.props);
            Ok(cx.tx.set_edge(g, id, &e.props)?)
        }
        ref other => Err(invalid_type(&format!("a write to {}", type_name(other)))),
    }
}

/// The names of tokens.
fn names(cx: &Cx<'_, '_>, tokens: &[u64]) -> BTreeSet<String> {
    tokens
        .iter()
        .filter_map(|t| cx.tx.engine().token_name(*t).map(|(_, n)| n))
        .collect()
}

fn named_props(cx: &Cx<'_, '_>, props: Vec<(u64, Value)>) -> Vec<(String, Value)> {
    props
        .into_iter()
        .filter_map(|(k, v)| cx.tx.engine().token_name(k).map(|(_, n)| (n, v)))
        .collect()
}

fn violation(what: String) -> Error {
    Error::new(codes::CG2000, format!("graph type violation: {what}"))
}

/// The closed-graph-type check of the elements a statement wrote
/// (module documentation).
fn check_closed(
    cx: &mut Cx<'_, '_>,
    g: &Graph,
    gt: &GraphType,
    written: &[Value],
) -> Result<(), Error> {
    cx.st.type_checks += 1;
    let mut seen = BTreeSet::new();
    for el in written {
        match *el {
            Value::Node(id) if seen.insert((0u8, id)) => {
                let Some((labels, props)) = cx.tx.node(g, id)? else {
                    continue;
                };
                let labels = names(cx, &labels);
                let props = named_props(cx, props);
                if gt.node_type(&labels, &props).is_none() {
                    return Err(violation(format!(
                        "no node type describes a node with labels {labels:?} and properties {:?}",
                        props.iter().map(|(n, _)| n).collect::<Vec<_>>()
                    )));
                }
            }
            Value::Edge(id) if seen.insert((1u8, id)) => {
                let Some(e) = cx.tx.edge(g, id)? else {
                    continue;
                };
                let label = cx
                    .tx
                    .engine()
                    .token_name(e.ty)
                    .map(|(_, n)| n)
                    .unwrap_or_default();
                let props = named_props(cx, e.props);
                let src = cx.tx.node(g, e.src)?.map(|(l, _)| l).unwrap_or_default();
                let dst = cx.tx.node(g, e.dst)?.map(|(l, _)| l).unwrap_or_default();
                let (src, dst) = (names(cx, &src), names(cx, &dst));
                if !gt.edge_matches(&label, &props, &src, &dst) {
                    return Err(violation(format!(
                        "no edge type describes an edge of type '{label}' from {src:?} to {dst:?}"
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl Operator for Write {
    fn next_batch(&mut self, cx: &mut Cx<'_, '_>) -> Result<Option<Batch>, Error> {
        if !self.done {
            self.done = true;
            let mut rows = Vec::new();
            while let Some(b) = self.child.next_batch(cx)? {
                for row in b.rows() {
                    self.held.grow(cx, row_bytes(row))?;
                    rows.push(row.to_vec());
                }
            }
            let g = cx.st.graphs[self.graph];
            let mut written = Vec::new();
            for row in rows {
                cx.st.tick()?;
                let r = self.apply(cx, &g, row, &mut written)?;
                self.out.push(r);
            }
            if let Some(gt) = cx.st.graph_types[self.graph].clone() {
                check_closed(cx, &g, &gt, &written)?;
            }
        }
        let b = self.out.take(cx.st.width);
        if self.out.is_empty() {
            self.held.release();
        }
        Ok(b)
    }

    fn reset(&mut self) {
        self.child.reset();
        self.out.clear();
        self.held.release();
        self.done = false;
    }

    fn held(&self) -> u64 {
        self.held.peak()
    }
}

/// The catalog-statement operator.
struct Catalog {
    ops: Vec<CatalogOp>,
    done: bool,
}

pub(super) fn catalog(ops: Vec<CatalogOp>) -> Box<dyn Operator> {
    Box::new(Catalog { ops, done: false })
}

fn invalid_reference(path: &[Box<str>], what: &str) -> Error {
    Error::new(
        codes::C42002,
        format!(
            "invalid reference: no {what} at /{}",
            path.iter().map(|s| &**s).collect::<Vec<_>>().join("/")
        ),
    )
}

/// The object at `path` if it is of `kind`.
fn resolve(cx: &mut Cx<'_, '_>, path: &[Box<str>], kind: Kind) -> Result<Option<u64>, Error> {
    let names: Vec<&str> = path.iter().map(|s| &**s).collect();
    Ok(cx
        .tx
        .resolve(&names)?
        .filter(|o| o.kind == kind)
        .map(|o| o.id))
}

/// The parent of `path` (a directory or the root) and its name.
fn in_directory<'p>(cx: &mut Cx<'_, '_>, path: &'p [Box<str>]) -> Result<(u64, &'p str), Error> {
    let (name, dirs) = path.split_last().expect("a path has a name");
    let parent = if dirs.is_empty() {
        ROOT
    } else {
        resolve(cx, dirs, Kind::Directory)?.ok_or_else(|| invalid_reference(dirs, "directory"))?
    };
    Ok((parent, name))
}

/// The schema of `path` (all but its last name) and its name.
fn in_schema<'p>(cx: &mut Cx<'_, '_>, path: &'p [Box<str>]) -> Result<(u64, &'p str), Error> {
    let (name, schema) = path.split_last().expect("a path has a name");
    let s =
        resolve(cx, schema, Kind::Schema)?.ok_or_else(|| invalid_reference(schema, "schema"))?;
    Ok((s, name))
}

/// The name of the graph type object of a graph's inline type.
fn inline_type_name(graph: &str) -> String {
    format!("$graph type of {graph}")
}

/// Drops `name` of `kind` in `parent`; 42002 if there is none.
fn drop_named(
    cx: &mut Cx<'_, '_>,
    parent: u64,
    name: &str,
    kind: Kind,
    path: &[Box<str>],
    what: &str,
) -> Result<(), Error> {
    match cx.tx.lookup(parent, name)? {
        Some(o) if o.kind == kind => {
            let inline = match (&o.definition, kind) {
                (Definition::Graph(d), Kind::Graph) if d.graph_type != 0 => cx
                    .tx
                    .object(d.graph_type)?
                    .filter(|t| t.name == inline_type_name(name))
                    .map(|t| t.name),
                _ => None,
            };
            cx.tx.drop_object(parent, name)?;
            if let Some(t) = inline {
                cx.tx.drop_object(parent, &t)?;
            }
            Ok(())
        }
        _ => Err(invalid_reference(path, what)),
    }
}

fn run_catalog(cx: &mut Cx<'_, '_>, op: &CatalogOp) -> Result<(), Error> {
    let owner = cx.st.principal.clone();
    match op {
        CatalogOp::CreateSchema(path) => {
            let (parent, name) = in_directory(cx, path)?;
            cx.tx.create_schema(parent, name, &owner)?;
        }
        CatalogOp::DropSchema(path) => {
            let (parent, name) = in_directory(cx, path)?;
            drop_named(cx, parent, name, Kind::Schema, path, "schema")?;
        }
        CatalogOp::CreateGraph {
            path,
            replace,
            graph_type,
        } => {
            let (schema, name) = in_schema(cx, path)?;
            if *replace
                && cx
                    .tx
                    .lookup(schema, name)?
                    .is_some_and(|o| o.kind == Kind::Graph)
            {
                drop_named(cx, schema, name, Kind::Graph, path, "graph")?;
            }
            let gt = match graph_type {
                GraphTypeOf::Open => None,
                GraphTypeOf::Named(p) => Some(
                    resolve(cx, p, Kind::GraphType)?
                        .ok_or_else(|| invalid_reference(p, "graph type"))?,
                ),
                GraphTypeOf::Inline(t) => Some(cx.tx.create_graph_type(
                    schema,
                    &inline_type_name(name),
                    &owner,
                    &t.encode(),
                )?),
            };
            cx.tx.create_graph(schema, name, &owner, gt)?;
        }
        CatalogOp::DropGraph(path) => {
            let (schema, name) = in_schema(cx, path)?;
            drop_named(cx, schema, name, Kind::Graph, path, "graph")?;
        }
        CatalogOp::CreateGraphType {
            path,
            replace,
            source,
        } => {
            let (schema, name) = in_schema(cx, path)?;
            let bytes = match source {
                GraphTypeSource::Inline(t) => t.encode(),
                GraphTypeSource::Copy(p) => {
                    let id = resolve(cx, p, Kind::GraphType)?
                        .ok_or_else(|| invalid_reference(p, "graph type"))?;
                    match cx.tx.object(id)?.map(|o| o.definition) {
                        Some(Definition::GraphType(b)) => b,
                        _ => return Err(invalid_reference(p, "graph type")),
                    }
                }
            };
            if *replace
                && cx
                    .tx
                    .lookup(schema, name)?
                    .is_some_and(|o| o.kind == Kind::GraphType)
            {
                drop_named(cx, schema, name, Kind::GraphType, path, "graph type")?;
            }
            cx.tx.create_graph_type(schema, name, &owner, &bytes)?;
        }
        CatalogOp::DropGraphType(path) => {
            let (schema, name) = in_schema(cx, path)?;
            drop_named(cx, schema, name, Kind::GraphType, path, "graph type")?;
        }
    }
    cx.st.counters.catalog_updates += 1;
    Ok(())
}

impl Operator for Catalog {
    fn next_batch(&mut self, cx: &mut Cx<'_, '_>) -> Result<Option<Batch>, Error> {
        if !self.done {
            self.done = true;
            for op in &self.ops {
                run_catalog(cx, op)?;
            }
        }
        Ok(None)
    }

    fn reset(&mut self) {
        self.done = false;
    }
}
