//! The built-in routines ([TS-24] Built-in procedures; rmp #50).
//!
//! * The four catalog-listing procedures of FR-PROC-004, with the columns
//!   of the Neo4j `SHOW` command that lists the same objects: values as
//!   Neo4j writes them (index types `RANGE`, `LOOKUP`, `POINT`; constraint
//!   types such as `NODE_PROPERTY_UNIQUENESS`; property types such as
//!   `STRING | LIST<STRING NOT NULL>`; Cypher Manual "List indexes" and
//!   "List constraints", and "Transaction clauses" for `SHOW TRANSACTIONS`,
//!   read 2026-10-11). Indexes and constraints have no stored name: each
//!   is listed as `index_<id>` or `constraint_<id>`, as Neo4j generates a
//!   name for one created without a name. A transaction's identifier is
//!   `transaction-<id>`, Neo4j's `<database>-transaction-<id>` without a
//!   database name: one handle is one database.
//! * The point procedures of the user decision of 2026-10-08 (OQ-006,
//!   "Valores + CALL"): `point`, `point.distance`,
//!   `point.nodesWithinDistance`, `point.edgesWithinDistance`. The two
//!   searches use the ONLINE point index on the label or edge type and
//!   property when one exists ([`Transaction::index_within`]), else scan
//!   the elements with the same filter, so both give the same rows
//!   (FR-TYPE-005).
//!
//! No built-in creates a directory (FR-CAT-005) or takes a backup or
//! restores one (FR-OPS-010).
//!
//! [TS-24]: ../../../docs/technical-spec/interfaces.md

use super::{Body, Parameter, ReadContext, Routine, Rows, Signature};
use crate::auth::Action;
use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID, Kind, ROOT, TokenKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::index::{Entity, SchemaDef, SchemaKind, State, decode_element};
use crate::storage::graph::{Graph, element_key, element_of};
use crate::tx::Transaction;
use crate::types::Value;
use crate::types::spatial::{self, Point, Srid};

/// A built-in routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Builtin {
    ShowTransactions,
    ShowIndexes,
    ShowConstraints,
    ShowCatalog,
    Point,
    Distance,
    NodesWithin,
    EdgesWithin,
}

/// Every built-in routine with its signature.
pub(crate) fn all() -> Vec<Routine> {
    let r = |b: Builtin, name: &str, params: Vec<Parameter>, cols: &[&str]| Routine {
        sig: Signature::new(name, params, cols),
        body: Body::Builtin(b),
    };
    let p = Parameter::new;
    vec![
        r(
            Builtin::ShowTransactions,
            "show_transactions",
            vec![],
            &[
                "transactionId",
                "username",
                "currentQuery",
                "startTime",
                "status",
                "elapsedTime",
                "metaData",
            ],
        ),
        r(
            Builtin::ShowIndexes,
            "show_indexes",
            vec![],
            &[
                "id",
                "name",
                "state",
                "populationPercent",
                "type",
                "entityType",
                "labelsOrTypes",
                "properties",
                "owningConstraint",
                "graph",
            ],
        ),
        r(
            Builtin::ShowConstraints,
            "show_constraints",
            vec![],
            &[
                "id",
                "name",
                "type",
                "entityType",
                "labelsOrTypes",
                "properties",
                "propertyType",
                "ownedIndex",
                "graph",
            ],
        ),
        r(
            Builtin::ShowCatalog,
            "show_catalog",
            vec![],
            &["name", "type", "default", "home"],
        ),
        r(
            Builtin::Point,
            "point",
            vec![
                p("x"),
                p("y"),
                Parameter::with_default("z", Value::Null),
                Parameter::with_default("srid", Value::Null),
            ],
            &["point"],
        ),
        r(
            Builtin::Distance,
            "point.distance",
            vec![p("from"), p("to")],
            &["distance"],
        ),
        r(
            Builtin::NodesWithin,
            "point.nodesWithinDistance",
            vec![p("label"), p("property"), p("center"), p("distance")],
            &["node", "distance"],
        ),
        r(
            Builtin::EdgesWithin,
            "point.edgesWithinDistance",
            vec![p("edgeType"), p("property"), p("center"), p("distance")],
            &["edge", "distance"],
        ),
    ]
}

/// The actions a built-in's call performs (TS-23): `SHOW INDEX` and `SHOW
/// CONSTRAINT` on every graph, `TRAVERSE` and `READ` of the working graph
/// for the point searches. `show_transactions` asks per transaction of
/// another principal when it runs (FR-OBS-003); `show_catalog` lists
/// catalog objects, for which TS-23 names no action.
pub(crate) fn requests(b: Builtin) -> Vec<(Action, bool)> {
    match b {
        Builtin::ShowIndexes => vec![(Action::ShowIndex, true)],
        Builtin::ShowConstraints => vec![(Action::ShowConstraint, true)],
        Builtin::NodesWithin | Builtin::EdgesWithin => {
            vec![(Action::Traverse, false), (Action::Read, false)]
        }
        _ => Vec::new(),
    }
}

/// Runs built-in `b` with `args`.
///
/// # Errors
///
/// The routine's errors: 22N11 for an argument outside its domain, 22G03
/// for one of the wrong type; storage errors.
pub(crate) fn run(b: Builtin, cx: &mut ReadContext<'_>, args: &[Value]) -> Result<Rows, Error> {
    let rows = match b {
        Builtin::ShowTransactions => match &cx.st.session.procs.host {
            Some(h) => h.transactions(&cx.st.session.principal),
            None => Vec::new(),
        },
        Builtin::ShowIndexes => schema_rows(cx.tx, true)?,
        Builtin::ShowConstraints => schema_rows(cx.tx, false)?,
        Builtin::ShowCatalog => catalog_rows(cx)?,
        Builtin::Point => vec![vec![point(args)?]],
        Builtin::Distance => vec![vec![distance(&args[0], &args[1])?]],
        Builtin::NodesWithin => within(cx, Entity::Node, args)?,
        Builtin::EdgesWithin => within(cx, Entity::Edge, args)?,
    };
    Ok(Box::new(rows.into_iter().map(Ok)))
}

fn s(x: &str) -> Value {
    Value::string(x)
}

fn list(xs: Vec<String>) -> Value {
    Value::List(xs.into_iter().map(|x| s(&x)).collect::<Vec<_>>().into())
}

/// The name of token `id`, or its number if it has none.
fn token_name(tx: &Transaction, id: u64) -> String {
    tx.engine()
        .token_name(id)
        .map_or_else(|| format!("#{id}"), |(_, n)| n)
}

/// The rows of `show_indexes` (`indexes`) or `show_constraints`.
fn schema_rows(tx: &mut Transaction, indexes: bool) -> Result<Vec<Vec<Value>>, Error> {
    let mut out = Vec::new();
    for g in tx.graph_ids()? {
        let path = crate::auth::path_of(tx, g)?;
        for d in tx.schema_defs(g)? {
            if d.kind.is_index() != indexes {
                continue;
            }
            out.push(if indexes {
                index_row(tx, &d, &path)
            } else {
                constraint_row(tx, &d, &path)
            });
        }
    }
    Ok(out)
}

fn entity_name(e: Entity) -> &'static str {
    match e {
        Entity::Node => "NODE",
        Entity::Edge => "RELATIONSHIP",
    }
}

fn index_row(tx: &Transaction, d: &SchemaDef, graph: &str) -> Vec<Value> {
    let lookup = d.kind == SchemaKind::Lookup;
    let (labels, props) = if lookup {
        (Value::Null, Value::Null)
    } else {
        (
            list(vec![token_name(tx, d.token)]),
            list(d.props.iter().map(|p| token_name(tx, *p)).collect()),
        )
    };
    vec![
        Value::Int(i64::try_from(d.id).unwrap_or(i64::MAX)),
        s(&format!("index_{}", d.id)),
        s(match d.state {
            State::Online => "ONLINE",
            State::Populating => "POPULATING",
        }),
        // The build does not record its progress: unknown until ONLINE.
        match d.state {
            State::Online => Value::Float(100.0),
            State::Populating => Value::Null,
        },
        s(match d.kind {
            SchemaKind::Lookup => "LOOKUP",
            SchemaKind::Point => "POINT",
            _ => "RANGE",
        }),
        s(entity_name(d.entity)),
        labels,
        props,
        if d.link == 0 {
            Value::Null
        } else {
            s(&format!("constraint_{}", d.link))
        },
        s(graph),
    ]
}

fn constraint_row(tx: &Transaction, d: &SchemaDef, graph: &str) -> Vec<Value> {
    let side = match d.entity {
        Entity::Node => "NODE",
        Entity::Edge => "RELATIONSHIP",
    };
    let kind = match d.kind {
        SchemaKind::Unique => format!("{side}_PROPERTY_UNIQUENESS"),
        SchemaKind::Exists => format!("{side}_PROPERTY_EXISTENCE"),
        SchemaKind::Type(_) => format!("{side}_PROPERTY_TYPE"),
        _ => format!("{side}_KEY"),
    };
    vec![
        Value::Int(i64::try_from(d.id).unwrap_or(i64::MAX)),
        s(&format!("constraint_{}", d.id)),
        s(&kind),
        s(entity_name(d.entity)),
        list(vec![token_name(tx, d.token)]),
        list(d.props.iter().map(|p| token_name(tx, *p)).collect()),
        match d.kind {
            SchemaKind::Type(t) => s(&t.describe()),
            _ => Value::Null,
        },
        if d.link == 0 {
            Value::Null
        } else {
            s(&format!("index_{}", d.link))
        },
        s(graph),
    ]
}

/// The rows of `show_catalog`: every catalog object, by path.
fn catalog_rows(cx: &mut ReadContext<'_>) -> Result<Vec<Vec<Value>>, Error> {
    let (home_schema, home_graph) = (cx.st.session.home_schema, cx.st.session.home_graph);
    let mut out = Vec::new();
    let mut stack = vec![(ROOT, String::new())];
    while let Some((parent, path)) = stack.pop() {
        for o in cx.tx.children(parent)? {
            let p = format!("{path}/{}", o.name);
            out.push(vec![
                s(&p),
                s(match o.kind {
                    Kind::Directory => "directory",
                    Kind::Schema => "schema",
                    Kind::GraphType => "graph type",
                    Kind::Graph => "graph",
                }),
                Value::Bool(o.id == DEFAULT_SCHEMA_ID || o.id == DEFAULT_GRAPH_ID),
                Value::Bool(o.id == home_schema || o.id == home_graph),
            ]);
            if matches!(o.kind, Kind::Directory | Kind::Schema) {
                stack.push((o.id, p));
            }
        }
    }
    out.sort_by(|a, b| match (&a[0], &b[0]) {
        (Value::String(x), Value::String(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    });
    Ok(out)
}

/// A number argument as a float (IA005: integers are accepted); `None`
/// for null.
fn float(v: &Value) -> Result<Option<f64>, Error> {
    match v {
        Value::Null => Ok(None),
        Value::Int(i) => Ok(Some(*i as f64)),
        Value::Float(f) => Ok(Some(*f)),
        _ => Err(Error::new(
            codes::C22G03,
            "invalid value type: a point coordinate or distance must be a number",
        )),
    }
}

fn invalid(what: &'static str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// `point(x, y, z, srid)` (TS-24 Point procedures).
fn point(args: &[Value]) -> Result<Value, Error> {
    let (x, y, z) = (float(&args[0])?, float(&args[1])?, float(&args[2])?);
    let srid = match &args[3] {
        Value::Null => None,
        Value::Int(i) => Some(*i),
        _ => {
            return Err(Error::new(
                codes::C22G03,
                "invalid value type: an SRID must be an integer",
            ));
        }
    };
    let srid = match srid {
        None if z.is_some() => Srid::Cartesian3D,
        None => Srid::Cartesian2D,
        Some(code) => u32::try_from(code)
            .ok()
            .and_then(Srid::from_code)
            .ok_or_else(|| invalid("an SRID other than 7203, 9157, 4326 and 4979"))?,
    };
    let three = matches!(srid, Srid::Cartesian3D | Srid::Wgs84_3D);
    if three != z.is_some() {
        return Err(invalid("z must be given exactly for SRIDs 9157 and 4979"));
    }
    let (Some(x), Some(y)) = (x, y) else {
        return Ok(Value::Null);
    };
    let mut c = vec![x, y];
    c.extend(z);
    Point::new(srid, &c)
        .map(|p| Value::Point(Box::new(p)))
        .ok_or_else(|| invalid("coordinates that do not fit the SRID"))
}

fn point_arg(v: &Value) -> Result<Option<&Point>, Error> {
    match v {
        Value::Null => Ok(None),
        Value::Point(p) => Ok(Some(p)),
        _ => Err(Error::new(
            codes::C22G03,
            "invalid value type: a point argument must be a point",
        )),
    }
}

/// `point.distance(from, to)`.
fn distance(a: &Value, b: &Value) -> Result<Value, Error> {
    let (Some(a), Some(b)) = (point_arg(a)?, point_arg(b)?) else {
        return Ok(Value::Null);
    };
    Ok(spatial::distance(a, b).map_or(Value::Null, Value::Float))
}

fn string_arg(v: &Value) -> Result<Option<&str>, Error> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        _ => Err(Error::new(
            codes::C22G03,
            "invalid value type: a label, edge type or property name must be a string",
        )),
    }
}

/// `point.nodesWithinDistance` and `point.edgesWithinDistance`.
fn within(
    cx: &mut ReadContext<'_>,
    entity: Entity,
    args: &[Value],
) -> Result<Vec<Vec<Value>>, Error> {
    let (token, prop) = (string_arg(&args[0])?, string_arg(&args[1])?);
    let (Some(center), Some(radius)) = (point_arg(&args[2])?, float(&args[3])?) else {
        return Ok(Vec::new());
    };
    let center = *center;
    let kind = match entity {
        Entity::Node => TokenKind::Label,
        Entity::Edge => TokenKind::EdgeType,
    };
    let engine = cx.tx.engine();
    let (Some(token), Some(prop)) = (
        token.and_then(|t| engine.token_id(kind, t)),
        prop.and_then(|p| engine.token_id(TokenKind::PropertyKey, p)),
    ) else {
        return Ok(Vec::new());
    };
    let g = cx.st.graphs[cx.graph];
    let wrap = |id: u64| match entity {
        Entity::Node => Value::Node(id),
        Entity::Edge => Value::Edge(id),
    };
    let index = cx.tx.schema_defs(g.id)?.into_iter().find(|d| {
        d.kind == SchemaKind::Point
            && d.state == State::Online
            && d.entity == entity
            && d.token == token
            && d.props == [prop]
    });
    let mut out = Vec::new();
    match index {
        Some(d) => {
            // The index filters by the exact distance of the record the
            // snapshot sees; the record is read again for the distance.
            let tree = tree_of(&g, entity);
            for id in cx.tx.index_within(&g, d.id, &center, radius)? {
                let Some(v) = cx.tx.get(tree, &element_key(id))? else {
                    continue;
                };
                if let Some(p) = point_of(entity, &v, token, prop)?
                    && let Some(dist) = spatial::distance(&center, &p)
                {
                    out.push(vec![wrap(id), Value::Float(dist)]);
                }
            }
        }
        None => {
            for (id, p) in elements(cx.tx, &g, entity, token, prop)? {
                if let Some(dist) = spatial::distance(&center, &p).filter(|d| *d <= radius) {
                    out.push(vec![wrap(id), Value::Float(dist)]);
                }
            }
        }
    }
    cx.st.scan_rows += out.len() as u64;
    Ok(out)
}

fn tree_of(g: &Graph, entity: Entity) -> crate::storage::btree::Tree {
    match entity {
        Entity::Node => g.nodes,
        Entity::Edge => g.edges,
    }
}

/// The point that property `prop` of record `v` holds, if the element has
/// token `token`.
fn point_of(entity: Entity, v: &[u8], token: u64, prop: u64) -> Result<Option<Point>, Error> {
    let el = decode_element(entity, v)
        .ok_or_else(|| crate::query::expr::internal("a malformed element record"))?;
    if !el.tokens.contains(&token) {
        return Ok(None);
    }
    Ok(match el.props.iter().find(|(k, _)| *k == prop) {
        Some((_, Value::Point(p))) => Some(**p),
        _ => None,
    })
}

/// The elements of `entity` in `g` with token `token` whose property
/// `prop` is a point, with that point: a scan of every element.
fn elements(
    tx: &mut Transaction,
    g: &Graph,
    entity: Entity,
    token: u64,
    prop: u64,
) -> Result<Vec<(u64, Point)>, Error> {
    let mut found = Vec::new();
    let mut bad = false;
    tx.scan_range(tree_of(g, entity), b"", None, |k, v| {
        match element_of(k) {
            Some(id) => found.push((id, v.to_vec())),
            None => bad = true,
        }
        !bad
    })?;
    if bad {
        return Err(crate::query::expr::internal("a malformed element key"));
    }
    let mut out = Vec::new();
    for (id, v) in found {
        if let Some(p) = point_of(entity, &v, token, prop)? {
            out.push((id, p));
        }
    }
    Ok(out)
}
