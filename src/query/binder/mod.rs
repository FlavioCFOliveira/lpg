//! The binder (rmp #43; [TS-20] Binder): one pass from the AST of a
//! program to its bound form ([`super::bound`]).
//!
//! * **Scopes** follow GQL's linear composition: each statement of a
//!   linear statement sees the variables of the binding table the
//!   previous one produced; a projection (`RETURN`, `SELECT`, the result
//!   of a nested query) replaces them with its columns; `EXISTS`
//!   subqueries see the outer variables and add their own, which do not
//!   escape. Every variable is a slot ([`super::bound::SlotInfo`]).
//! * **Path patterns** are normalised and checked
//!   ([`pattern`]): degrees, the conditional-singleton join rule, edge
//!   uniqueness (ID086).
//! * **Catalog resolution**: labels, edge types and property keys to
//!   tokens (absent tokens stay names: no element has them, and an insert
//!   creates them when it runs); graphs and schemas by path; procedures
//!   are #50's registry (round C): until then a `CALL` fails with 42N08.
//! * **Feature gates**: every construct outside the approved set fails
//!   with [`super::features::unsupported`].
//! * **Parameters** are [`super::expr::Expr::Param`] indexes, never values.
//!
//! The binder's layout (one `bind_*` function per AST rule over a
//! `Binder` that holds the current and home schema and graph) is ported
//! from miniGU @ `fc2ccf5` (<https://github.com/TuGraph-family/miniGU>,
//! Copyright 2022 AntGroup CO., Ltd., Apache-2.0, see `LICENSE-APACHE`),
//! `minigu/gql/planner/src/binder/{mod,query,procedure_spec,catalog,
//! object_ref}.rs`, matching the AST copied from miniGU in rmp #41;
//! miniGU leaves most constructs `not_implemented`, so their bodies are
//! lpg's. Studied: zu @ `a821319` `crates/zu-query/src/binder.rs` (scopes,
//! implicit grouping, projection checks); Kuzu @ `89f0263`
//! `src/binder/` (MIT; binder scopes and query-graph collection); frogql @
//! `b5dd0f0` `src/typing/checker.rs` (MIT; path-pattern typing).
//!
//! [TS-20]: ../../../../docs/technical-spec/query-engine.md

mod catalog;
mod expr;
mod pattern;
mod statements;
#[cfg(test)]
mod tests;

use super::ast;
use super::bound::{
    Body, Degree, Part, Program, Query, SessionCmd, SlotInfo, StatementKind, Ty, VarKind,
};
use super::expr::Slot;
use super::features::{gate, unsupported};
use super::span::{Span, Spanned};
#[cfg(any(test, fuzzing))]
use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID};
use crate::error::{Error, Position};
use crate::gqlstatus::codes;
use crate::tx::Transaction;

/// What a statement runs against: the session's principal and its home
/// and current schema and graph (ID001, ID002, `interfaces.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionCtx {
    /// The principal (ID001).
    pub(crate) principal: String,
    /// The home schema's object identifier.
    pub(crate) home_schema: u64,
    /// The home graph's object identifier.
    pub(crate) home_graph: u64,
    /// The current schema's object identifier.
    pub(crate) current_schema: u64,
    /// The current graph's object identifier.
    pub(crate) current_graph: u64,
    /// The database handle's procedures and functions, and the API host
    /// (rmp #50).
    pub(crate) procs: crate::proc::Procs,
}

impl SessionCtx {
    /// A session of `principal` at the defaults of FR-DB-005: `/public` and
    /// `/public/lpg` (ID002).
    #[cfg(any(test, fuzzing))]
    pub(crate) fn new(principal: &str) -> SessionCtx {
        SessionCtx {
            principal: principal.to_string(),
            home_schema: DEFAULT_SCHEMA_ID,
            home_graph: DEFAULT_GRAPH_ID,
            current_schema: DEFAULT_SCHEMA_ID,
            current_graph: DEFAULT_GRAPH_ID,
            procs: crate::proc::Procs::builtins(),
        }
    }
}

/// Binds the parsed program `p` of `text` in `tx` for `session`.
///
/// # Errors
///
/// The "feature not supported" error of an unsupported construct
/// (42001 with cause 51N26); 42002 for a name that resolves to nothing;
/// 42000 for a rule of the bound form that the program breaks (a
/// duplicate binding, an implicit join on a conditional singleton, an
/// aggregate where none is allowed); the data exceptions of literals;
/// 42N08 for a procedure call; storage errors of catalog reads.
pub(crate) fn bind(
    text: &str,
    p: &ast::Program,
    tx: &mut Transaction,
    session: &SessionCtx,
) -> Result<Program, Error> {
    for code in super::lexer::comment_features(text) {
        gate(code)?;
    }
    if let Some(span) = p.invalid_escapes.first() {
        return Err(Error::new(
            codes::C42001,
            "invalid syntax: an escape that names no Unicode scalar value",
        )
        .at(Position::of(text, span.start.offset as usize)));
    }
    let mut b = Binder::new(text, tx, session);
    let mut out = Program {
        session: Vec::new(),
        start: false,
        body: None,
        end: None,
        close: p.session_close,
    };
    match &p.activity {
        None => {}
        Some(Spanned(ast::ProgramActivity::Session(s), _)) => {
            for Spanned(set, span) in &s.set {
                out.session.push(b.at(*span, |b| b.bind_session_set(set))?);
            }
            if let Some(Spanned(r, span)) = s.reset.first() {
                let code = match &r.0 {
                    None => "GS04",
                    Some(Spanned(a, _)) => match a {
                        ast::SessionResetArgs::Characteristics => "GS04",
                        ast::SessionResetArgs::Schema => "GS05",
                        ast::SessionResetArgs::Graph => "GS06",
                        ast::SessionResetArgs::TimeZone => "GS07",
                        ast::SessionResetArgs::Parameters => "GS08",
                        ast::SessionResetArgs::Parameter(_) => "GS16",
                    },
                };
                return Err(b.locate(unsupported(code), *span));
            }
        }
        Some(Spanned(ast::ProgramActivity::Transaction(t), _)) => {
            if let Some(Spanned(start, span)) = &t.start {
                b.at(*span, |_| gate("GT01"))?;
                if !start.0.is_empty() {
                    return Err(b.locate(unsupported("GT02"), *span));
                }
                out.start = true;
            }
            if let Some(Spanned(proc_, span)) = &t.procedure {
                out.body = Some(b.at(*span, |b| b.bind_procedure(proc_))?);
            }
            if let Some(Spanned(end, span)) = &t.end {
                b.at(*span, |_| gate("GT01"))?;
                out.end = Some(*end);
            }
        }
    }
    Ok(out)
}

/// The binder's state for one program.
pub(crate) struct Binder<'a> {
    text: &'a str,
    tx: &'a mut Transaction,
    session: &'a SessionCtx,
    current_schema: u64,
    current_graph: u64,
    slots: Vec<SlotInfo>,
    params: Vec<Box<str>>,
    graphs: Vec<u64>,
    subqueries: Vec<Vec<Part>>,
    /// The pipeline of the statement being bound.
    parts: Vec<Part>,
    /// Its result columns.
    columns: Vec<(Box<str>, Slot)>,
    /// The visible variables: name and slot.
    scope: Vec<(Box<str>, Slot)>,
    reads: bool,
    writes: bool,
    /// Binding a query, where only reads are allowed (rmp #50).
    reads_only: bool,
}

impl<'a> Binder<'a> {
    fn new(text: &'a str, tx: &'a mut Transaction, session: &'a SessionCtx) -> Self {
        Binder {
            text,
            tx,
            session,
            current_schema: session.current_schema,
            current_graph: session.current_graph,
            slots: Vec::new(),
            params: Vec::new(),
            graphs: Vec::new(),
            subqueries: Vec::new(),
            parts: Vec::new(),
            columns: Vec::new(),
            scope: Vec::new(),
            reads: false,
            writes: false,
            reads_only: false,
        }
    }

    /// `e` with the position of `span`, if it has none.
    fn locate(&self, e: Error, span: Span) -> Error {
        if e.position().is_some() || span == Span::default() {
            e
        } else {
            e.at(Position::of(self.text, span.start.offset as usize))
        }
    }

    /// Runs `f`, giving its error the position of `span`.
    fn at<T>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        f(self).map_err(|e| self.locate(e, span))
    }

    /// The source text of `span`.
    fn text_of(&self, span: Span) -> &str {
        self.text
            .get(span.start.offset as usize..span.end.offset as usize)
            .unwrap_or("")
    }

    /// A new slot.
    fn slot(&mut self, name: &str, kind: VarKind, ty: Ty, graph: Option<usize>) -> Slot {
        self.slots.push(SlotInfo {
            name: name.into(),
            kind,
            degree: Degree::Singleton,
            ty,
            graph,
        });
        self.slots.len() - 1
    }

    /// A slot no name refers to.
    fn hidden(&mut self, kind: VarKind, graph: Option<usize>) -> Slot {
        let n = format!("${}", self.slots.len());
        let ty = match kind {
            VarKind::Node => Ty::Node,
            VarKind::Edge => Ty::Edge,
            VarKind::Value => Ty::Any,
        };
        self.slot(&n, kind, ty, graph)
    }

    /// The visible variable `name`.
    fn lookup(&self, name: &str) -> Option<Slot> {
        self.scope
            .iter()
            .rev()
            .find(|(n, _)| &**n == name)
            .map(|(_, s)| *s)
    }

    /// The index of graph `id` in the statement's graph list.
    fn graph_index(&mut self, id: u64) -> usize {
        match self.graphs.iter().position(|g| *g == id) {
            Some(i) => i,
            None => {
                self.graphs.push(id);
                self.graphs.len() - 1
            }
        }
    }

    /// The index of parameter `name`.
    fn param(&mut self, name: &str) -> usize {
        match self.params.iter().position(|p| &**p == name) {
            Some(i) => i,
            None => {
                self.params.push(name.into());
                self.params.len() - 1
            }
        }
    }

    // ---------------------------------------------------------------
    // Sessions and procedures.

    fn bind_session_set(&mut self, set: &ast::SessionSet) -> Result<SessionCmd, Error> {
        match set {
            ast::SessionSet::Schema(Spanned(r, span)) => {
                let id = self.at(*span, |b| b.resolve_schema(r))?;
                self.current_schema = id;
                Ok(SessionCmd::Schema(id))
            }
            ast::SessionSet::Graph(Spanned(g, span)) => {
                let id = self.at(*span, |b| b.resolve_graph_expr(g))?;
                self.current_graph = id;
                Ok(SessionCmd::Graph(id))
            }
            ast::SessionSet::TimeZone(_) => Err(unsupported("GS15")),
            ast::SessionSet::Parameter(p) => Err(unsupported(match &p.0.kind {
                ast::SessionSetParameterKind::Graph(_) => "GS01",
                ast::SessionSetParameterKind::BindingTable(_) => "GS02",
                ast::SessionSetParameterKind::Value(_) => "GS03",
            })),
        }
    }

    fn bind_procedure(&mut self, p: &ast::Procedure) -> Result<Body, Error> {
        if let Some(Spanned(_, span)) = &p.at {
            return Err(self.locate(unsupported("GP16"), *span));
        }
        if let Some(Spanned(d, span)) = p.binding_variable_defs.first() {
            let code = match d {
                ast::BindingVariableDef::Graph(_) => "GP11",
                ast::BindingVariableDef::BindingTable(_) => "GP08",
                ast::BindingVariableDef::Value(_) => "GP05",
            };
            return Err(self.locate(unsupported(code), *span));
        }
        if let Some(Spanned(_, span)) = p.next_statements.first() {
            return Err(self.locate(unsupported("GQ20"), *span));
        }
        let Spanned(st, span) = &p.statement;
        self.at(*span, |b| b.bind_statement(st))
    }

    fn bind_statement(&mut self, st: &ast::Statement) -> Result<Body, Error> {
        match st {
            ast::Statement::Catalog(list)
                if list.len() == 1
                    && matches!(list[0].0, ast::CatalogModifyingStatement::Call(_)) =>
            {
                // A lone `CALL`, which the oracle parses as a
                // catalog-modifying statement (TS-19 Ambiguities): its
                // result is the columns it yields, as Neo4j's standalone
                // `CALL` returns the procedure's columns.
                let Spanned(ast::CatalogModifyingStatement::Call(c), span) = &list[0] else {
                    unreachable!("matched above");
                };
                self.at(*span, |b| b.bind_call(c))?;
                if let Some(Part::Call { yields, .. }) = self.parts.last() {
                    self.columns = yields
                        .iter()
                        .map(|(_, s)| (self.slots[*s].name.clone(), *s))
                        .collect();
                }
                Ok(Body::Query(self.finish()))
            }
            ast::Statement::Catalog(list) => {
                let mut ops = Vec::new();
                for Spanned(c, span) in list {
                    ops.push(self.at(*span, |b| b.bind_catalog(c))?);
                }
                Ok(Body::Catalog(ops))
            }
            ast::Statement::Query(q) => {
                self.bind_composite(q)?;
                Ok(Body::Query(self.finish()))
            }
            ast::Statement::Data(d) => {
                self.bind_data(d)?;
                Ok(Body::Query(self.finish()))
            }
        }
    }

    /// The bound query of the parts bound so far.
    fn finish(&mut self) -> Query {
        let kind = match (self.reads, self.writes) {
            (_, false) => StatementKind::Read,
            (false, true) => StatementKind::Write,
            (true, true) => StatementKind::ReadWrite,
        };
        Query {
            slots: std::mem::take(&mut self.slots),
            params: std::mem::take(&mut self.params),
            graphs: std::mem::take(&mut self.graphs),
            parts: std::mem::take(&mut self.parts),
            columns: std::mem::take(&mut self.columns),
            subqueries: std::mem::take(&mut self.subqueries),
            kind,
        }
    }
}
