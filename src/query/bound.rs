//! The bound form: what the binder ([`super::binder`]) produces and the
//! planner ([`super::plan`]) consumes ([TS-20] Binder; rmp #43).
//!
//! A statement binds to a [`Query`]: one row of slots for the whole
//! statement (its *frame*), the parameter names in order of first use,
//! the graphs it reads or writes, and a pipeline of [`Part`]s, each
//! operating on the binding table the previous one produced, as GQL's
//! linear composition does. Every variable is a slot with a kind, a
//! degree and a static type ([`SlotInfo`]); parameters are
//! [`super::expr::Expr::Param`] indexes and are never inlined.
//!
//! The layout (a bound procedure of bound statements with a data schema,
//! catalog statements resolved against the catalog, graph references
//! resolved per part) follows miniGU @ `fc2ccf5`
//! `minigu/gql/planner/src/bound/*.rs` (Apache-2.0), adapted to slots: a
//! frame of slots per statement, as Memgraph's symbol table and frame
//! (R-MG `src/query/frontend/semantic/symbol_table.hpp`, studied only)
//! and Neo4j's slotted runtime do.
//!
//! [TS-20]: ../../../docs/technical-spec/query-engine.md

use super::aggregate::AggKind;
use super::ast::EndTransaction;
use super::expr::{Expr, Slot};

/// The kind of a variable (TS-20 Binder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VarKind {
    Node,
    Edge,
    Value,
}

/// The degree of a variable ([GPML] §4.4, §4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Degree {
    /// Bound to one element in every match.
    Singleton,
    /// Bound in only some matches: declared in one operand of a union or
    /// alternation, or under `?`.
    Conditional,
    /// Bound to a list of elements: declared under a quantifier.
    Group,
}

/// The static type of a variable: an element reference type, or dynamic.
/// Values are typed when they are evaluated (TS-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    /// A dynamic type: known only at run time.
    Any,
    /// A node reference (IW011, IW012).
    Node,
    /// An edge reference (IW011).
    Edge,
}

/// One slot of the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlotInfo {
    /// The variable's name; generated for anonymous elements and hidden
    /// slots (`$`-prefixed, which no identifier can be).
    pub(crate) name: Box<str>,
    /// Its kind.
    pub(crate) kind: VarKind,
    /// Its degree.
    pub(crate) degree: Degree,
    /// Its static type.
    pub(crate) ty: Ty,
    /// The graph its element lies in: an index into [`Query::graphs`].
    pub(crate) graph: Option<usize>,
}

/// A bound program (`gqlProgram`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Program {
    /// `SESSION SET SCHEMA` and `SESSION SET GRAPH` commands, resolved to
    /// catalog object identifiers.
    pub(crate) session: Vec<SessionCmd>,
    /// `START TRANSACTION` (GT01).
    pub(crate) start: bool,
    /// The procedure body.
    pub(crate) body: Option<Body>,
    /// `COMMIT` or `ROLLBACK` (GT01).
    pub(crate) end: Option<EndTransaction>,
    /// `SESSION CLOSE`.
    pub(crate) close: bool,
}

/// A session command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionCmd {
    /// The session's current schema becomes this schema.
    Schema(u64),
    /// The session's current graph becomes this graph.
    Graph(u64),
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    /// A query or data-modifying statement.
    Query(Query),
    /// Catalog-modifying statements (GC01, GC04), run in order.
    Catalog(Vec<CatalogOp>),
}

/// What a statement does, as Bolt's `type` (`interfaces.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatementKind {
    Read,
    Write,
    ReadWrite,
}

/// A query or data-modifying statement.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Query {
    /// The frame.
    pub(crate) slots: Vec<SlotInfo>,
    /// The parameter names, in order of first use.
    pub(crate) params: Vec<Box<str>>,
    /// The graphs used, as catalog object identifiers.
    pub(crate) graphs: Vec<u64>,
    /// The pipeline.
    pub(crate) parts: Vec<Part>,
    /// The result columns: name and slot; empty for `FINISH` and for a
    /// statement without a result.
    pub(crate) columns: Vec<(Box<str>, Slot)>,
    /// The `EXISTS` subqueries, by [`Expr::Exists`] index.
    pub(crate) subqueries: Vec<Vec<Part>>,
    /// Read, write or both.
    pub(crate) kind: StatementKind,
}

/// One stage of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Part {
    /// `MATCH` or `OPTIONAL MATCH` of a graph pattern in a graph.
    Match {
        optional: bool,
        graph: usize,
        pattern: Pattern,
    },
    /// Keeps the rows for which the predicate is TRUE.
    Filter(Expr),
    /// A projection: `RETURN`, `SELECT`, the result of a nested query.
    Project(Project),
    /// `ORDER BY`: keys and whether each is descending.
    Sort(Vec<(Expr, bool)>),
    /// `INSERT`.
    Insert {
        graph: usize,
        pattern: InsertPattern,
    },
    /// `SET`.
    Set { graph: usize, items: Vec<SetItem> },
    /// `REMOVE`.
    Remove {
        graph: usize,
        items: Vec<(Slot, PropKey)>,
    },
    /// A named procedure call (GP04; rmp #50): the routine, one argument
    /// per parameter (defaults filled in), the result columns it yields
    /// (column index, slot), and its working graph.
    Call {
        routine: crate::proc::RoutineRef,
        args: Vec<Expr>,
        yields: Vec<(usize, Slot)>,
        graph: usize,
    },
}

/// A projection. Without aggregates, every item is computed per row.
/// With aggregates, the plain items are the grouping keys (implicit
/// grouping, as Neo4j and zu `binder.rs:4558-4600` do), each aggregate
/// is computed per group, and `post` computes the items that combine
/// aggregates from the key and aggregate slots.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Project {
    /// Per-row items, or grouping keys: output slot and expression.
    pub(crate) items: Vec<(Slot, Expr)>,
    /// Aggregates: output slot, function, `DISTINCT`, argument.
    pub(crate) aggregates: Vec<(Slot, AggKind, bool, Expr)>,
    /// Items over the key and aggregate slots, computed per group.
    pub(crate) post: Vec<(Slot, Expr)>,
    /// `DISTINCT`: the output rows are deduplicated over `output`.
    pub(crate) distinct: bool,
    /// `HAVING`.
    pub(crate) having: Option<Expr>,
    /// The slots the projection outputs, in item order.
    pub(crate) output: Vec<Slot>,
}

/// A graph pattern of one `MATCH`, normalised: node and edge variables,
/// and the conjuncts of its label tests, property specifications, element
/// `WHERE` clauses, graph-pattern `WHERE` clause and edge uniqueness.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pattern {
    /// The node variables.
    pub(crate) nodes: Vec<PNode>,
    /// The edges, each pointing right after normalisation.
    pub(crate) edges: Vec<PEdge>,
    /// The predicates, as conjuncts.
    pub(crate) predicates: Vec<Expr>,
}

/// A node variable of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PNode {
    /// Its slot.
    pub(crate) slot: Slot,
    /// Whether an earlier part bound it (an implicit join).
    pub(crate) bound: bool,
}

/// An edge of a pattern, from `src` to `dst` (indexes into
/// [`Pattern::nodes`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PEdge {
    /// Its slot.
    pub(crate) slot: Slot,
    /// Its source node.
    pub(crate) src: usize,
    /// Its target node.
    pub(crate) dst: usize,
    /// Its edge type when its label expression is one edge type: `Some`
    /// of the token, `Some(None)` for a type no edge has.
    pub(crate) ty: Option<Option<u64>>,
    /// Whether an earlier part bound it.
    pub(crate) bound: bool,
}

/// A label, edge type or property key by name, with its token if one
/// exists (an insert creates a missing one when it runs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Name {
    /// The name.
    pub(crate) name: Box<str>,
    /// The token, if the token table holds the name.
    pub(crate) token: Option<u64>,
}

/// A property key.
pub(crate) type PropKey = Name;

/// An insert graph pattern.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InsertPattern {
    /// Its nodes: new ones, and bound variables it refers to.
    pub(crate) nodes: Vec<INode>,
    /// Its edges, oriented from source to target.
    pub(crate) edges: Vec<IEdge>,
}

/// A node of an insert pattern.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct INode {
    /// Its slot.
    pub(crate) slot: Slot,
    /// Whether it refers to a node an earlier part bound.
    pub(crate) existing: bool,
    /// Its labels.
    pub(crate) labels: Vec<Name>,
    /// Its properties.
    pub(crate) props: Vec<(PropKey, Expr)>,
}

/// An edge of an insert pattern.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IEdge {
    /// Its slot.
    pub(crate) slot: Slot,
    /// Its source node (an index into [`InsertPattern::nodes`]).
    pub(crate) src: usize,
    /// Its target node.
    pub(crate) dst: usize,
    /// Its edge type.
    pub(crate) ty: Name,
    /// Its properties.
    pub(crate) props: Vec<(PropKey, Expr)>,
}

/// A `SET` item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SetItem {
    /// `SET x.p = v`.
    Prop(Slot, PropKey, Expr),
    /// `SET x = {p: v, …}`: replaces every property.
    All(Slot, Vec<(PropKey, Expr)>),
}

/// A catalog-modifying statement, with absolute paths: parents are
/// resolved when it runs, so a statement can use an object an earlier
/// statement of the same list created.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CatalogOp {
    /// `CREATE SCHEMA`: the directories and the schema's name.
    CreateSchema(Vec<Box<str>>),
    /// `DROP SCHEMA`.
    DropSchema(Vec<Box<str>>),
    /// `CREATE [OR REPLACE] GRAPH`: the schema path and graph name, and its
    /// type.
    CreateGraph {
        path: Vec<Box<str>>,
        replace: bool,
        graph_type: GraphTypeOf,
    },
    /// `DROP GRAPH`.
    DropGraph(Vec<Box<str>>),
    /// `CREATE [OR REPLACE] GRAPH TYPE`.
    CreateGraphType {
        path: Vec<Box<str>>,
        replace: bool,
        source: GraphTypeSource,
    },
    /// `DROP GRAPH TYPE`.
    DropGraphType(Vec<Box<str>>),
}

/// The error of a catalog-modifying statement that fails with 42N11 (the
/// object exists), 42002 (a name resolves to nothing or to an object of
/// another kind) or G1000 (the object still has dependants): 42000, with
/// that error as its cause, as opengql/tck expects 42000 for each of them
/// (`Create1.feature`, `drop1.feature`). The pair follows the 42001 +
/// 51N26 pattern of an unsupported feature (`interfaces.md` "GQLSTATUS
/// codes"; DECISION 386 on rmp #52). Any other error is returned as it is.
pub(crate) fn catalog_failure(e: crate::error::Error) -> crate::error::Error {
    use crate::gqlstatus::codes;
    let what = match e.status() {
        s if s == codes::C42N11 => "the object already exists",
        s if s == codes::C42002 => "a name resolves to no object of the required kind",
        s if s == codes::CG1000 => "the object still has dependent objects",
        _ => return e,
    };
    let mut top = crate::error::Error::new(
        codes::C42000,
        format!("syntax error or access rule violation: {what}"),
    );
    if let Some(p) = e.position() {
        top = top.at(p);
    }
    top.caused_by(e)
}

/// The type of a graph being created.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GraphTypeOf {
    /// `ANY`: an open graph type (GG01).
    Open,
    /// A named graph type (GG02): its path.
    Named(Vec<Box<str>>),
    /// An inline specification (GG02, GG03).
    Inline(super::graph_type::GraphType),
}

/// The source of a graph type being created.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GraphTypeSource {
    /// `AS COPY OF` a named graph type: its path.
    Copy(Vec<Box<str>>),
    /// An inline specification (GG03).
    Inline(super::graph_type::GraphType),
}
