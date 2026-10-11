//! Binding of catalog references, catalog-modifying statements, graph type
//! specifications and value types (rmp #43 and #46).
//!
//! References resolve against the catalog as the transaction sees it.
//! Catalog-modifying statements keep absolute paths and resolve their
//! parents when they run ([`crate::query::bound::CatalogOp`]), so one list
//! can create a schema and then a graph in it. No statement creates a
//! directory (FR-CAT-005): directories are created through the API.

use std::collections::BTreeSet;

use super::Binder;
use super::statements::rule;
use crate::catalog::Kind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::ast;
use crate::query::bound::{CatalogOp, GraphTypeOf, GraphTypeSource};
use crate::query::features::{gate, unsupported};
use crate::query::graph_type::{EdgeType, GraphType, NodeType, PropType};
use crate::query::span::Spanned;

/// The 42002 "invalid reference" error.
fn invalid_reference(what: &str) -> Error {
    Error::new(codes::C42002, format!("invalid reference: {what}"))
}

type Path = Vec<Box<str>>;

impl Binder<'_> {
    /// The path of catalog object `id`.
    fn path_of(&mut self, id: u64) -> Result<Path, Error> {
        match self.tx.path(id)? {
            Some(p) => Ok(p.into_iter().map(Into::into).collect()),
            None => Err(invalid_reference("an object the transaction does not see")),
        }
    }

    /// The absolute path of a schema reference.
    fn schema_path(&mut self, r: &ast::SchemaRef) -> Result<Path, Error> {
        Ok(match r {
            ast::SchemaRef::Root => {
                return Err(invalid_reference("the root directory is not a schema"));
            }
            ast::SchemaRef::Absolute(p) => absolute(p),
            ast::SchemaRef::Relative { up, path } => {
                let mut base = self.path_of(self.current_schema)?;
                for _ in 0..*up {
                    if base.pop().is_none() {
                        return Err(invalid_reference("a relative path above the root"));
                    }
                }
                base.extend(absolute(path));
                base
            }
            ast::SchemaRef::Home => self.path_of(self.session.home_schema)?,
            ast::SchemaRef::Current => self.path_of(self.current_schema)?,
            ast::SchemaRef::Parameter(_) => return Err(unsupported("GE08")),
        })
    }

    /// The object at `path`, which must be of `kind`.
    fn resolve_path(&mut self, path: &Path, kind: Kind) -> Result<u64, Error> {
        let names: Vec<&str> = path.iter().map(|s| &**s).collect();
        match self.tx.resolve(&names)? {
            Some(o) if o.kind == kind => Ok(o.id),
            _ => Err(invalid_reference(&format!(
                "no {} at /{}",
                match kind {
                    Kind::Directory => "directory",
                    Kind::Schema => "schema",
                    Kind::Graph => "graph",
                    Kind::GraphType => "graph type",
                },
                names.join("/")
            ))),
        }
    }

    /// The schema a schema reference names.
    pub(super) fn resolve_schema(&mut self, r: &ast::SchemaRef) -> Result<u64, Error> {
        let p = self.schema_path(r)?;
        self.resolve_path(&p, Kind::Schema)
    }

    /// The path of a graph or graph type reference: its parent schema
    /// (the current schema by default) and its name.
    fn object_path(&mut self, r: &ast::CatalogObjectRef) -> Result<Path, Error> {
        let mut p = match &r.parent {
            None => self.path_of(self.current_schema)?,
            Some(Spanned(parent, _)) => {
                if !parent.objects.is_empty() {
                    return Err(invalid_reference(
                        "a graph or graph type lies in a schema, not in another object",
                    ));
                }
                match &parent.schema {
                    Some(Spanned(s, _)) => self.schema_path(s)?,
                    None => self.path_of(self.current_schema)?,
                }
            }
        };
        p.push(r.name.0.clone());
        Ok(p)
    }

    /// The graph a graph expression names.
    pub(super) fn resolve_graph_expr(&mut self, g: &ast::GraphExpr) -> Result<u64, Error> {
        let path = match g {
            ast::GraphExpr::Current => return Ok(self.current_graph),
            ast::GraphExpr::Reference(r) => match r {
                ast::GraphRef::Home => return Ok(self.session.home_graph),
                ast::GraphRef::Parameter(_) => return Err(unsupported("GE08")),
                ast::GraphRef::Object(o) => self.object_path(o)?,
                ast::GraphRef::Delimited(n) => {
                    let mut p = self.path_of(self.current_schema)?;
                    p.push(n.clone());
                    p
                }
            },
            ast::GraphExpr::Name(n) => {
                let mut p = self.path_of(self.current_schema)?;
                p.push(n.clone());
                p
            }
            ast::GraphExpr::Object(_) => return Err(unsupported("GE01")),
        };
        self.resolve_path(&path, Kind::Graph)
    }

    pub(super) fn bind_catalog(
        &mut self,
        c: &ast::CatalogModifyingStatement,
    ) -> Result<CatalogOp, Error> {
        use ast::CatalogModifyingStatement as C;
        self.writes = true;
        match c {
            C::Call(call) => {
                self.bind_call(call, false)?;
                Err(rule("a procedure call is not a catalog statement"))
            }
            C::CreateSchema(s) => {
                gate("GC01")?;
                if s.if_not_exists {
                    return Err(unsupported("GC02"));
                }
                Ok(CatalogOp::CreateSchema(absolute(&s.path.0)))
            }
            C::DropSchema(s) => {
                gate("GC01")?;
                if s.if_exists {
                    return Err(unsupported("GC02"));
                }
                Ok(CatalogOp::DropSchema(absolute(&s.path.0)))
            }
            C::CreateGraph(g) => {
                gate("GC04")?;
                if g.kind.0 == ast::CreateKind::CreateIfNotExists {
                    return Err(self.locate(unsupported("GC05"), g.kind.1));
                }
                if let Some(Spanned(_, span)) = &g.source {
                    return Err(self.locate(unsupported("GG05"), *span));
                }
                let path = self.at(g.path.1, |b| b.object_path(&g.path.0))?;
                let graph_type = match &g.graph_type.0 {
                    ast::OfGraphType::Any => {
                        gate("GG01")?;
                        GraphTypeOf::Open
                    }
                    ast::OfGraphType::Like(_) => return Err(unsupported("GG04")),
                    ast::OfGraphType::Ref(Spanned(r, span)) => {
                        gate("GG02")?;
                        match r {
                            ast::GraphTypeRef::Parameter(_) => return Err(unsupported("GE08")),
                            ast::GraphTypeRef::Object(o) => {
                                GraphTypeOf::Named(self.at(*span, |b| b.object_path(o))?)
                            }
                        }
                    }
                    ast::OfGraphType::Nested(Spanned(spec, span)) => {
                        gate("GG02")?;
                        gate("GG03")?;
                        GraphTypeOf::Inline(self.at(*span, |b| b.graph_type(spec))?)
                    }
                };
                Ok(CatalogOp::CreateGraph {
                    path,
                    replace: g.kind.0 == ast::CreateKind::CreateOrReplace,
                    graph_type,
                })
            }
            C::DropGraph(d) => {
                gate("GC04")?;
                if d.if_exists {
                    return Err(unsupported("GC05"));
                }
                Ok(CatalogOp::DropGraph(
                    self.at(d.path.1, |b| b.object_path(&d.path.0))?,
                ))
            }
            C::CreateGraphType(t) => {
                gate("GC01")?;
                if t.kind.0 == ast::CreateKind::CreateIfNotExists {
                    return Err(self.locate(unsupported("GC03"), t.kind.1));
                }
                let path = self.at(t.path.1, |b| b.object_path(&t.path.0))?;
                let source = match &t.source.0 {
                    ast::GraphTypeSource::Copy(Spanned(r, span)) => match r {
                        ast::GraphTypeRef::Parameter(_) => return Err(unsupported("GE08")),
                        ast::GraphTypeRef::Object(o) => {
                            GraphTypeSource::Copy(self.at(*span, |b| b.object_path(o))?)
                        }
                    },
                    ast::GraphTypeSource::Like(_) => return Err(unsupported("GG04")),
                    ast::GraphTypeSource::Nested(Spanned(spec, span)) => {
                        gate("GG03")?;
                        GraphTypeSource::Inline(self.at(*span, |b| b.graph_type(spec))?)
                    }
                };
                Ok(CatalogOp::CreateGraphType {
                    path,
                    replace: t.kind.0 == ast::CreateKind::CreateOrReplace,
                    source,
                })
            }
            C::DropGraphType(d) => {
                gate("GC01")?;
                if d.if_exists {
                    return Err(unsupported("GC03"));
                }
                Ok(CatalogOp::DropGraphType(
                    self.at(d.path.1, |b| b.object_path(&d.path.0))?,
                ))
            }
        }
    }

    /// A graph type specification (GG03), with its rules
    /// ([`crate::query::graph_type`]).
    fn graph_type(&mut self, spec: &ast::NestedGraphType) -> Result<GraphType, Error> {
        let mut g = GraphType::default();
        let mut aliases: Vec<(Box<str>, usize)> = Vec::new();
        for Spanned(el, span) in spec {
            if let ast::ElementTypeSpec::Node(n) = el {
                let t = self.at(*span, |_| node_type(n))?;
                if g.nodes.iter().any(|o| o.labels == t.labels) {
                    return Err(self.locate(rule("two node types with the same label set"), *span));
                }
                if let Some(Spanned(a, _)) = &n.alias {
                    aliases.push((a.clone(), g.nodes.len()));
                }
                g.nodes.push(t);
            }
        }
        for Spanned(el, span) in spec {
            if let ast::ElementTypeSpec::Edge(e) = el {
                let t = self.at(*span, |_| edge_type(e, &mut g, &aliases))?;
                if g.edges.iter().any(|o| o.label == t.label) {
                    return Err(self.locate(rule("two edge types with the same label"), *span));
                }
                g.edges.push(t);
            }
        }
        Ok(g)
    }
}

/// The names of an absolute schema path.
fn absolute(p: &ast::SchemaPath) -> Path {
    p.directories
        .iter()
        .map(|Spanned(d, _)| d.clone())
        .chain(std::iter::once(p.name.0.clone()))
        .collect()
}

/// The labels and properties of an element type.
type Filler = (BTreeSet<String>, Vec<(String, PropType)>);

/// The labels and properties of an element type filler.
fn filler(f: Option<&Spanned<ast::TypeFiller>>) -> Result<Filler, Error> {
    let Some(Spanned(f, _)) = f else {
        return Ok((BTreeSet::new(), Vec::new()));
    };
    if f.key_labels.is_some() {
        return Err(unsupported("GG21"));
    }
    let labels = f
        .labels
        .iter()
        .flat_map(|Spanned(ls, _)| ls.iter())
        .map(|Spanned(l, _)| l.to_string())
        .collect();
    let mut props: Vec<(String, PropType)> = Vec::new();
    for Spanned(ft, _) in f.properties.iter().flatten() {
        let name = ft.name.0.to_string();
        if props.iter().any(|(n, _)| *n == name) {
            return Err(rule(&format!("the property '{name}' is declared twice")));
        }
        props.push((name, prop_type(&ft.value_type.0)?));
    }
    props.sort();
    Ok((labels, props))
}

fn node_type(n: &ast::NodeTypeSpec) -> Result<NodeType, Error> {
    if n.name.is_some() {
        return Err(unsupported("GG20"));
    }
    let (labels, props) = filler(n.filler.as_ref())?;
    if labels.is_empty() {
        return Err(Error::new(
            codes::C42012,
            "number of node type key labels below supported minimum: a node type has at least one label",
        ));
    }
    Ok(NodeType { labels, props })
}

/// An edge type; endpoint fillers that no node type has yet declare one
/// (provisional, OQ-005).
fn edge_type(
    e: &ast::EdgeTypeSpec,
    g: &mut GraphType,
    aliases: &[(Box<str>, usize)],
) -> Result<EdgeType, Error> {
    if e.directed == Some(false) {
        return Err(unsupported("GH02"));
    }
    if e.name.is_some() {
        return Err(unsupported("GG20"));
    }
    let (labels, props) = filler(e.filler.as_ref())?;
    let label = match labels.len() {
        1 => labels.into_iter().next().expect("one label"),
        0 => {
            return Err(Error::new(
                codes::C42014,
                "number of edge type key labels below supported minimum: an edge type has one label",
            ));
        }
        _ => {
            return Err(Error::new(
                codes::C42015,
                "number of edge type key labels exceeds supported maximum: an edge type has one label",
            ));
        }
    };
    let mut implied: Vec<NodeType> = Vec::new();
    let mut endpoint = |r: &Spanned<ast::NodeTypeRef>| -> Result<usize, Error> {
        match &r.0 {
            ast::NodeTypeRef::Alias(a) => aliases
                .iter()
                .find(|(n, _)| n == a)
                .map(|(_, i)| *i)
                .ok_or_else(|| invalid_reference(&format!("no node type is named '{a}'"))),
            ast::NodeTypeRef::Filler(f) => {
                let (labels, props) = filler(f.as_ref())?;
                if labels.is_empty() {
                    return Err(invalid_reference("an endpoint with no label"));
                }
                let all = g.nodes.iter().chain(implied.iter());
                match all.clone().position(|n| n.labels == labels) {
                    Some(i) => {
                        let n = all.clone().nth(i).expect("found");
                        if !props.is_empty() && n.props != props {
                            return Err(rule(
                                "an endpoint redeclares a node type with other properties",
                            ));
                        }
                        Ok(i)
                    }
                    None => {
                        implied.push(NodeType { labels, props });
                        Ok(g.nodes.len() + implied.len() - 1)
                    }
                }
            }
        }
    };
    let (a, b) = (endpoint(&e.source)?, endpoint(&e.destination)?);
    drop(endpoint);
    let (src, dst) = match e.direction {
        ast::EdgeDirection::Right => (a, b),
        ast::EdgeDirection::Left => (b, a),
        ast::EdgeDirection::Undirected => return Err(unsupported("GH02")),
        _ => return Err(unsupported("G043")),
    };
    g.nodes.extend(implied);
    Ok(EdgeType {
        label,
        src,
        dst,
        props,
    })
}

/// The property type of a value type, with its gates.
fn prop_type(t: &ast::ValueType) -> Result<PropType, Error> {
    use ast::ValueType as V;
    let nn = |not_null: bool| -> Result<(), Error> {
        if not_null {
            Err(unsupported("GV90"))
        } else {
            Ok(())
        }
    };
    Ok(match t {
        V::Bool { not_null } => {
            nn(*not_null)?;
            PropType::Bool
        }
        V::CharacterString {
            kind,
            min_length,
            max_length,
            not_null,
        } => {
            match kind {
                ast::StringTypeKind::Fixed => return Err(unsupported("GV32")),
                ast::StringTypeKind::VariableMax if max_length.is_some() => {
                    return Err(unsupported("GV31"));
                }
                _ => {}
            }
            if min_length.is_some() {
                return Err(unsupported("GV30"));
            }
            if max_length.is_some() {
                return Err(unsupported("GV31"));
            }
            nn(*not_null)?;
            PropType::String
        }
        V::ByteString { .. } => return Err(unsupported("GV35")),
        V::Numeric {
            kind,
            precision,
            scale,
            not_null,
        } => {
            use ast::IntegerWidth as W;
            use ast::NumericKind as N;
            if scale.is_some() {
                return Err(unsupported("GV17"));
            }
            let t = match kind {
                N::Integer { signed, width } => {
                    let code = match (signed, width) {
                        (true, W::Regular) => None,
                        (true, W::Bits(8)) => Some("GV02"),
                        (true, W::Bits(16)) => Some("GV04"),
                        (true, W::Bits(32)) => Some("GV07"),
                        (true, W::Bits(64)) => Some("GV12"),
                        (true, W::Bits(128)) => Some("GV14"),
                        (true, W::Bits(_)) => Some("GV16"),
                        (true, W::Small) => Some("GV18"),
                        (true, W::Big) => Some("GV19"),
                        (false, W::Bits(8)) => Some("GV01"),
                        (false, W::Bits(16)) => Some("GV03"),
                        (false, W::Bits(32)) => Some("GV06"),
                        (false, W::Bits(64)) => Some("GV11"),
                        (false, W::Bits(128)) => Some("GV13"),
                        (false, W::Bits(_)) => Some("GV15"),
                        (false, W::Small) => Some("GV05"),
                        (false, W::Regular) => Some("GV08"),
                        (false, W::Big) => Some("GV10"),
                    };
                    if let Some(c) = code {
                        return Err(unsupported(c));
                    }
                    if precision.is_some() {
                        return Err(unsupported("GV09"));
                    }
                    PropType::Int
                }
                N::Decimal => return Err(unsupported("GV17")),
                N::FloatBits(16) => return Err(unsupported("GV20")),
                N::FloatBits(32) => return Err(unsupported("GV21")),
                N::FloatBits(64) => return Err(unsupported("GV24")),
                N::FloatBits(128) => return Err(unsupported("GV25")),
                N::FloatBits(_) => return Err(unsupported("GV26")),
                N::Float => {
                    if precision.is_some() {
                        return Err(unsupported("GV22"));
                    }
                    PropType::Float
                }
                N::Real | N::Double => return Err(unsupported("GV23")),
            };
            nn(*not_null)?;
            t
        }
        V::Temporal { kind, not_null } => {
            nn(*not_null)?;
            match kind {
                ast::TemporalKind::ZonedDatetime => PropType::ZonedDateTime,
                ast::TemporalKind::LocalDatetime => PropType::LocalDateTime,
                ast::TemporalKind::Date => PropType::Date,
                ast::TemporalKind::ZonedTime => PropType::ZonedTime,
                ast::TemporalKind::LocalTime => PropType::LocalTime,
                ast::TemporalKind::DurationYearToMonth => PropType::YearToMonth,
                ast::TemporalKind::DurationDayToSecond => PropType::DayToSecond,
            }
        }
        V::AnyGraph { .. } | V::Graph { .. } => return Err(unsupported("GV60")),
        V::BindingTable { .. } => return Err(unsupported("GV61")),
        V::AnyNode { .. } | V::Node { .. } | V::AnyEdge { .. } | V::Edge { .. } => {
            return Err(rule(
                "a node or edge reference type is not a property value type",
            ));
        }
        V::Null => return Err(unsupported("GV71")),
        V::Empty => return Err(unsupported("GV72")),
        V::Path { .. } => return Err(unsupported("GV55")),
        V::List { .. } => return Err(unsupported("GV50")),
        V::Record { .. } => return Err(unsupported("GV45")),
        V::AnyValue { .. } => return Err(unsupported("GV66")),
        V::PropertyValue { .. } => return Err(unsupported("GV68")),
        V::Union(_) => return Err(unsupported("GV65")),
    })
}
