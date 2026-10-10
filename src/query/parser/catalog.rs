//! Catalog object references, graph and binding table expressions, and
//! catalog-modifying statements (`GQL.g4` 11, 12, 17).
//!
//! **Catalog paths.** The reference rules (`schemaReference`,
//! `catalogObjectParentReference`, `graphReference`, …) are regular
//! languages over names, `/`, `.` and `..`, and they are not LL(k): in
//! `/a/b/c` the parser cannot tell whether `b` is a directory or a schema
//! until it sees what follows the whole path. Each rule therefore lists
//! every way it can match at the current token, in the oracle's
//! preference order (lower alternative first, an optional part present
//! before absent, a loop's longer iteration first), and the caller takes
//! the first match whose next token can follow it in its context. The
//! follow sets are the `follows_*` functions below.

use super::{Expected, PResult, Parser};
use crate::query::ast::{
    BindingTableExpr, BindingTableRef, CatalogModifyingStatement, CatalogObjectRef,
    CreateGraphStatement, CreateGraphTypeStatement, CreateKind, CreateSchemaStatement,
    DropSchemaStatement, DropStatement, GraphExpr, GraphRef, GraphTypeRef, GraphTypeSource, Ident,
    ObjectExpr, OfGraphType, ParentRef, ProcedureRef, SchemaPath, SchemaRef,
};
use crate::query::lexer::{Keyword as K, TokenKind as T};
use crate::query::span::{Span, Spanned};

/// A follow predicate: whether the token `next` (followed by `after`)
/// can follow the construct in its context.
pub(super) type Follow = dyn Fn(T, T) -> bool;

/// After a session command: another session command or the end.
pub(super) fn follows_session(next: T, _: T) -> bool {
    matches!(next, T::Keyword(K::Session) | T::Eof)
}

/// Whether `next` (then `after`) starts a binding variable definition:
/// `GRAPH`, `TABLE` or `VALUE` before its variable, `PROPERTY GRAPH`,
/// `BINDING TABLE`.
fn starts_binding_definition(next: T, after: T) -> bool {
    matches!(
        (next, after),
        (T::Keyword(K::Property), T::Keyword(K::Graph))
            | (T::Keyword(K::Binding), T::Keyword(K::Table))
    ) || (matches!(next, T::Keyword(K::Graph | K::Table | K::Value))
        && Parser::is_regular_identifier(after))
}

/// After `AT schemaReference`: a binding variable definition or a
/// statement.
pub(super) fn follows_at_schema(next: T, after: T) -> bool {
    starts_binding_definition(next, after) || super::query::starts_statement(next, after)
}

/// After a binding variable definition's initializer: another definition
/// or a statement.
pub(super) fn follows_binding_definition(next: T, after: T) -> bool {
    follows_at_schema(next, after)
}

/// After `USE graphExpression`: the statement it focuses.
pub(super) fn follows_use(next: T, after: T) -> bool {
    super::query::starts_statement(next, after)
        && !matches!(next, T::Keyword(K::Use | K::Select | K::Create | K::Drop))
}

/// After a primitive catalog-modifying statement: another one, a `CALL`,
/// or the end of the statement.
pub(super) fn follows_catalog_statement(next: T, after: T) -> bool {
    matches!(
        next,
        T::Keyword(K::Create | K::Drop | K::Call | K::Next | K::Commit | K::Rollback | K::Session)
            | T::Eof
            | T::RightBrace
    ) || (next == T::Keyword(K::Optional) && after == T::Keyword(K::Call))
}

/// After the graph of `SELECT … FROM graphExpression`: its match
/// statement or nested query.
pub(super) fn follows_select_graph(next: T, _: T) -> bool {
    matches!(next, T::Keyword(K::Match | K::Optional) | T::LeftBrace)
}

/// After an expression: anything but the continuation of a catalog
/// path (a name, `.`, `..`), which no expression rule accepts. The names
/// that can follow an expression start a binding variable definition.
pub(super) fn follows_expression(next: T, after: T) -> bool {
    if matches!(next, T::Period | T::DoublePeriod) {
        return false;
    }
    !Parser::is_identifier(next) || starts_binding_definition(next, after)
}

/// A matched schema reference, by token index.
#[derive(Debug, Clone, Copy)]
enum SchemaPlan {
    Root,
    /// Directories: `count` names at `first`, `first + 2`, …; the schema
    /// name at `name`.
    Absolute {
        first: usize,
        count: usize,
        name: usize,
    },
    Relative {
        up: u32,
        first: usize,
        count: usize,
        name: usize,
    },
    Home,
    Current,
    Parameter(usize),
}

/// A matched `catalogObjectParentReference`.
#[derive(Debug, Clone, Copy)]
struct ParentPlan {
    start: usize,
    /// The schema and the index after it.
    schema: Option<(SchemaPlan, usize)>,
    /// Object names at `first`, `first + 2`, …
    first: usize,
    count: usize,
}

/// A matched catalog object: an optional parent and a name.
#[derive(Debug, Clone, Copy)]
struct ObjectPlan {
    parent: Option<ParentPlan>,
    name: usize,
}

/// A matched graph, graph type, binding table or procedure reference.
#[derive(Debug, Clone, Copy)]
enum RefPlan {
    Object(ObjectPlan),
    Delimited(usize),
    Home,
    Parameter(usize),
}

impl Parser<'_> {
    fn kind_at(&self, i: usize) -> T {
        self.token_at(i).kind
    }

    fn name_at(&self, i: usize) -> bool {
        Self::is_identifier(self.kind_at(i))
    }

    /// `(name SOLIDUS)+` from `i`: the largest count.
    fn name_slash_count(&self, i: usize, sep: T) -> usize {
        let mut n = 0;
        while self.name_at(i + 2 * n) && self.kind_at(i + 2 * n + 1) == sep {
            n += 1;
        }
        n
    }

    /// `schemaReference` matches at `i`, in preference order.
    fn schema_reference_plans(&self, i: usize) -> Vec<(usize, SchemaPlan)> {
        let mut out = Vec::new();
        match self.kind_at(i) {
            // absoluteCatalogSchemaReference: SOLIDUS | absoluteDirectoryPath schemaName;
            // absoluteDirectoryPath: SOLIDUS simpleDirectoryPath?
            T::Solidus => {
                out.push((i + 1, SchemaPlan::Root));
                let max = self.name_slash_count(i + 1, T::Solidus);
                for count in (0..=max).rev() {
                    let name = i + 1 + 2 * count;
                    if self.name_at(name) {
                        out.push((
                            name + 1,
                            SchemaPlan::Absolute {
                                first: i + 1,
                                count,
                                name,
                            },
                        ));
                    }
                }
            }
            T::Keyword(K::HomeSchema) => out.push((i + 1, SchemaPlan::Home)),
            T::Keyword(K::CurrentSchema) | T::Period => out.push((i + 1, SchemaPlan::Current)),
            // relativeDirectoryPath: .. (/ ..)* / simpleDirectoryPath?
            T::DoublePeriod => {
                let mut j = i + 1;
                let mut up = 1u32;
                while self.kind_at(j) == T::Solidus && self.kind_at(j + 1) == T::DoublePeriod {
                    j += 2;
                    up = up.saturating_add(1);
                }
                if self.kind_at(j) == T::Solidus {
                    let max = self.name_slash_count(j + 1, T::Solidus);
                    for count in (0..=max).rev() {
                        let name = j + 1 + 2 * count;
                        if self.name_at(name) {
                            out.push((
                                name + 1,
                                SchemaPlan::Relative {
                                    up,
                                    first: j + 1,
                                    count,
                                    name,
                                },
                            ));
                        }
                    }
                }
            }
            T::SubstitutedParameter => out.push((i + 1, SchemaPlan::Parameter(i))),
            _ => {}
        }
        out
    }

    /// `catalogObjectParentReference` matches at `i`: `schemaReference
    /// SOLIDUS? (objectName PERIOD)*` or `(objectName PERIOD)+`.
    fn parent_reference_plans(&self, i: usize) -> Vec<(usize, ParentPlan)> {
        let mut out = Vec::new();
        for (end, schema) in self.schema_reference_plans(i) {
            let solidus: &[usize] = if self.kind_at(end) == T::Solidus {
                &[1, 0]
            } else {
                &[0]
            };
            for &s in solidus {
                let first = end + s;
                let max = self.name_slash_count(first, T::Period);
                for count in (0..=max).rev() {
                    out.push((
                        first + 2 * count,
                        ParentPlan {
                            start: i,
                            schema: Some((schema, end)),
                            first,
                            count,
                        },
                    ));
                }
            }
        }
        let max = self.name_slash_count(i, T::Period);
        for count in (1..=max).rev() {
            out.push((
                i + 2 * count,
                ParentPlan {
                    start: i,
                    schema: None,
                    first: i,
                    count,
                },
            ));
        }
        out
    }

    /// Catalog object matches at `i`: a parent (mandatory or optional)
    /// and a name.
    fn object_plans(&self, i: usize, parent_optional: bool) -> Vec<(usize, ObjectPlan)> {
        let mut out = Vec::new();
        for (end, parent) in self.parent_reference_plans(i) {
            if self.name_at(end) {
                out.push((
                    end + 1,
                    ObjectPlan {
                        parent: Some(parent),
                        name: end,
                    },
                ));
            }
        }
        if parent_optional && self.name_at(i) {
            out.push((
                i + 1,
                ObjectPlan {
                    parent: None,
                    name: i,
                },
            ));
        }
        out
    }

    /// The first plan whose end is followed by a token `follow` accepts.
    /// The plans whose end is followed by a token `follow` accepts take
    /// part in a decision ([`super::Site::PATH`]); the first is taken
    /// unless a retry forces another.
    fn choose<P: Copy>(&mut self, plans: &[(usize, P)], follow: &Follow) -> Option<(usize, P)> {
        let passing: Vec<(usize, P)> = plans
            .iter()
            .copied()
            .filter(|&(end, _)| follow(self.kind_at(end), self.kind_at(end + 1)))
            .collect();
        if passing.is_empty() {
            return None;
        }
        let k = self.decide(super::Site::PATH, passing.len());
        Some(passing[k])
    }

    /// The span of tokens `from..to`.
    fn span_of(&self, from: usize, to: usize) -> Span {
        Span::new(
            self.token_at(from).span.start,
            self.token_at(to.max(from + 1) - 1).span.end,
        )
    }

    /// The identifier at token `i`.
    fn name_from(&mut self, i: usize) -> Spanned<Ident> {
        let t = self.token_at(i);
        let name = match t.kind {
            T::DoubleQuoted | T::AccentQuoted => self.unquote(t),
            _ => t.text(self.text).to_owned(),
        };
        Spanned(name.into(), t.span)
    }

    fn names_from(&mut self, first: usize, count: usize) -> Vec<Spanned<Ident>> {
        (0..count).map(|k| self.name_from(first + 2 * k)).collect()
    }

    fn parameter_from(&mut self, i: usize, skip: usize) -> Ident {
        let t = self.token_at(i);
        self.parameter_name(t, skip).into()
    }

    fn build_schema(&mut self, plan: SchemaPlan) -> SchemaRef {
        match plan {
            SchemaPlan::Root => SchemaRef::Root,
            SchemaPlan::Absolute { first, count, name } => SchemaRef::Absolute(SchemaPath {
                directories: self.names_from(first, count),
                name: self.name_from(name),
            }),
            SchemaPlan::Relative {
                up,
                first,
                count,
                name,
            } => SchemaRef::Relative {
                up,
                path: SchemaPath {
                    directories: self.names_from(first, count),
                    name: self.name_from(name),
                },
            },
            SchemaPlan::Home => SchemaRef::Home,
            SchemaPlan::Current => SchemaRef::Current,
            SchemaPlan::Parameter(i) => SchemaRef::Parameter(self.parameter_from(i, 2)),
        }
    }

    fn build_object(&mut self, plan: ObjectPlan) -> CatalogObjectRef {
        let parent = plan.parent.map(|p| {
            let schema = p.schema.map(|(s, end)| {
                let span = self.span_of(p.start, end);
                Spanned(self.build_schema(s), span)
            });
            let objects = self.names_from(p.first, p.count);
            Spanned(
                ParentRef { schema, objects },
                self.span_of(p.start, plan.name),
            )
        });
        CatalogObjectRef {
            parent,
            name: self.name_from(plan.name),
        }
    }

    /// Consumes the tokens up to `end` and returns their span.
    fn commit_to(&mut self, end: usize) -> Span {
        let span = self.span_of(self.pos, end);
        self.pos = end;
        span
    }

    /// Records the path tokens as expected and fails.
    fn fail_path<V>(&mut self, what: &'static str) -> PResult<V> {
        // The furthest point a path reached: after its last path token.
        let mut i = self.pos;
        while self.name_at(i)
            || matches!(
                self.kind_at(i),
                T::Solidus | T::Period | T::DoublePeriod | T::SubstitutedParameter
            )
        {
            i += 1;
        }
        let saved = self.pos;
        self.pos = i;
        self.expect_here(Expected::Rule("the continuation of a catalog path"));
        self.pos = saved;
        self.fail_rule(what)
    }

    /// `schemaReference: absoluteCatalogSchemaReference |
    /// relativeCatalogSchemaReference | referenceParameterSpecification`
    /// (with `absoluteCatalogSchemaReference`,
    /// `relativeCatalogSchemaReference`, `predefinedSchemaReference`,
    /// `absoluteDirectoryPath`, `relativeDirectoryPath`,
    /// `simpleDirectoryPath`).
    pub(super) fn schema_reference(&mut self, follow: &Follow) -> PResult<Spanned<SchemaRef>> {
        let plans = self.schema_reference_plans(self.pos);
        let Some((end, plan)) = self.choose(&plans, follow) else {
            return self.fail_path("a schema reference");
        };
        let span = self.span_of(self.pos, end);
        let schema = self.build_schema(plan);
        self.pos = end;
        Ok(Spanned(schema, span))
    }

    /// `catalogSchemaParentAndName: absoluteDirectoryPath schemaName`.
    fn catalog_schema_parent_and_name(&mut self) -> PResult<Spanned<SchemaPath>> {
        let plans: Vec<_> = self
            .schema_reference_plans(self.pos)
            .into_iter()
            .filter(|(_, p)| matches!(p, SchemaPlan::Absolute { .. }))
            .collect();
        let Some((end, plan)) = self.choose(&plans, &follows_catalog_statement) else {
            if !self.at(T::Solidus) {
                return Err(super::Fail);
            }
            return self.fail_path("a schema path");
        };
        let SchemaRef::Absolute(path) = self.build_schema(plan) else {
            return Err(super::Fail);
        };
        let span = self.commit_to(end);
        Ok(Spanned(path, span))
    }

    /// `catalogGraphParentAndName`, `catalogGraphTypeParentAndName` and
    /// `catalogProcedureParentAndName`: `catalogObjectParentReference?
    /// name`.
    fn catalog_object_parent_and_name(
        &mut self,
        follow: &Follow,
        what: &'static str,
    ) -> PResult<Spanned<CatalogObjectRef>> {
        let plans = self.object_plans(self.pos, true);
        let Some((end, plan)) = self.choose(&plans, follow) else {
            if !self.at_identifier() {
                self.at(T::Solidus);
                self.at(T::Period);
                self.at(T::DoublePeriod);
            }
            return self.fail_path(what);
        };
        let object = self.build_object(plan);
        let span = self.commit_to(end);
        Ok(Spanned(object, span))
    }

    /// The `graphReference` plans: parent and name; a delimited name;
    /// the home graph; a reference parameter.
    fn graph_reference_plans(&self, i: usize) -> Vec<(usize, RefPlan)> {
        let mut out: Vec<(usize, RefPlan)> = self
            .object_plans(i, false)
            .into_iter()
            .map(|(e, p)| (e, RefPlan::Object(p)))
            .collect();
        match self.kind_at(i) {
            T::DoubleQuoted | T::AccentQuoted => out.push((i + 1, RefPlan::Delimited(i))),
            T::Keyword(K::HomeGraph | K::HomePropertyGraph) => out.push((i + 1, RefPlan::Home)),
            T::SubstitutedParameter => out.push((i + 1, RefPlan::Parameter(i))),
            _ => {}
        }
        out
    }

    fn build_graph_ref(&mut self, plan: RefPlan) -> GraphRef {
        match plan {
            RefPlan::Object(o) => GraphRef::Object(self.build_object(o)),
            RefPlan::Delimited(i) => GraphRef::Delimited(self.name_from(i).0),
            RefPlan::Home => GraphRef::Home,
            RefPlan::Parameter(i) => GraphRef::Parameter(self.parameter_from(i, 2)),
        }
    }

    /// `graphTypeReference: catalogGraphTypeParentAndName |
    /// referenceParameterSpecification`.
    pub(super) fn graph_type_reference(
        &mut self,
        follow: &Follow,
    ) -> PResult<Spanned<GraphTypeRef>> {
        let mut plans: Vec<(usize, RefPlan)> = self
            .object_plans(self.pos, true)
            .into_iter()
            .map(|(e, p)| (e, RefPlan::Object(p)))
            .collect();
        if self.peek() == T::SubstitutedParameter {
            plans.push((self.pos + 1, RefPlan::Parameter(self.pos)));
        }
        let Some((end, plan)) = self.choose(&plans, follow) else {
            self.at(T::SubstitutedParameter);
            return self.fail_path("a graph type reference");
        };
        let r = match plan {
            RefPlan::Object(o) => GraphTypeRef::Object(self.build_object(o)),
            RefPlan::Parameter(i) => GraphTypeRef::Parameter(self.parameter_from(i, 2)),
            RefPlan::Delimited(_) | RefPlan::Home => return Err(super::Fail),
        };
        let span = self.commit_to(end);
        Ok(Spanned(r, span))
    }

    /// `procedureReference: catalogProcedureParentAndName |
    /// referenceParameterSpecification`, followed by `(`.
    pub(super) fn procedure_reference(&mut self) -> PResult<Spanned<ProcedureRef>> {
        let follow = |next: T, _: T| next == T::LeftParen;
        let mut plans: Vec<(usize, RefPlan)> = self
            .object_plans(self.pos, true)
            .into_iter()
            .map(|(e, p)| (e, RefPlan::Object(p)))
            .collect();
        if self.peek() == T::SubstitutedParameter {
            plans.push((self.pos + 1, RefPlan::Parameter(self.pos)));
        }
        let Some((end, plan)) = self.choose(&plans, &follow) else {
            self.at(T::SubstitutedParameter);
            self.at_identifier();
            return self.fail_path("a procedure reference");
        };
        let r = match plan {
            RefPlan::Object(o) => ProcedureRef::Object(self.build_object(o)),
            RefPlan::Parameter(i) => ProcedureRef::Parameter(self.parameter_from(i, 2)),
            RefPlan::Delimited(_) | RefPlan::Home => return Err(super::Fail),
        };
        let span = self.commit_to(end);
        Ok(Spanned(r, span))
    }

    /// `graphExpression: graphReference | objectExpressionPrimary |
    /// objectNameOrBindingVariable | currentGraph`.
    ///
    /// The alternatives overlap (`a.b` is both a graph reference and a
    /// property reference; `"g"` is both a delimited graph name and a
    /// string literal); the lowest that can be followed in `follow` wins.
    pub(super) fn graph_expression(&mut self, follow: &Follow) -> PResult<Spanned<GraphExpr>> {
        let start = self.start();
        if self.eat_kw(K::CurrentGraph) || self.eat_kw(K::CurrentPropertyGraph) {
            return Ok(self.spanned(start, GraphExpr::Current));
        }
        let plans = self.graph_reference_plans(self.pos);
        if let Some((end, plan)) = self.choose(&plans, follow) {
            let r = self.build_graph_ref(plan);
            let span = self.commit_to(end);
            return Ok(Spanned(GraphExpr::Reference(r), span));
        }
        if let Some(o) = self.try_parse(|p| p.object_expression_primary())? {
            return Ok(self.spanned(start, GraphExpr::Object(o)));
        }
        if self.at_regular_identifier() {
            let name = self.regular_identifier()?;
            return Ok(Spanned(GraphExpr::Name(name.0), name.1));
        }
        self.at_kw(K::CurrentGraph);
        self.fail_rule("a graph expression")
    }

    /// `objectExpressionPrimary: VARIABLE valueExpressionPrimary |
    /// parenthesizedValueExpression |
    /// nonParenthesizedValueExpressionPrimarySpecialCase`.
    pub(super) fn object_expression_primary(&mut self) -> PResult<ObjectExpr> {
        if self.eat_kw(K::Variable) {
            let e = self.value_expression_primary()?;
            return Ok(ObjectExpr::Variable(Box::new(e)));
        }
        let e = self.value_expression_primary()?;
        match e.0 {
            crate::query::ast::Expr::Parenthesized(inner) => Ok(ObjectExpr::Parenthesized(inner)),
            crate::query::ast::Expr::Variable(_) => self.fail_rule("an object expression"),
            _ => Ok(ObjectExpr::Primary(Box::new(e))),
        }
    }

    /// `bindingTableExpression: nestedBindingTableQuerySpecification |
    /// bindingTableReference | objectExpressionPrimary |
    /// objectNameOrBindingVariable`.
    ///
    /// A `{` opens a nested query unless it starts a record literal
    /// (`{}` or `{ name :`), which only `objectExpressionPrimary` accepts.
    pub(super) fn binding_table_expression(
        &mut self,
        follow: &Follow,
    ) -> PResult<Spanned<BindingTableExpr>> {
        let start = self.start();
        if self.at(T::LeftBrace) && !self.at_record_literal() {
            let q = self.nested_procedure_specification()?;
            return Ok(self.spanned(start, BindingTableExpr::Nested(Box::new(q))));
        }
        let mut plans: Vec<(usize, RefPlan)> = self
            .object_plans(self.pos, false)
            .into_iter()
            .map(|(e, p)| (e, RefPlan::Object(p)))
            .collect();
        match self.peek() {
            T::DoubleQuoted | T::AccentQuoted => {
                plans.push((self.pos + 1, RefPlan::Delimited(self.pos)))
            }
            T::SubstitutedParameter => plans.push((self.pos + 1, RefPlan::Parameter(self.pos))),
            _ => {}
        }
        if let Some((end, plan)) = self.choose(&plans, follow) {
            let r = match plan {
                RefPlan::Object(o) => BindingTableRef::Object(self.build_object(o)),
                RefPlan::Delimited(i) => BindingTableRef::Delimited(self.name_from(i).0),
                RefPlan::Parameter(i) => BindingTableRef::Parameter(self.parameter_from(i, 2)),
                RefPlan::Home => return Err(super::Fail),
            };
            let span = self.commit_to(end);
            return Ok(Spanned(BindingTableExpr::Reference(r), span));
        }
        if let Some(o) = self.try_parse(|p| p.object_expression_primary())? {
            return Ok(self.spanned(start, BindingTableExpr::Object(o)));
        }
        if self.at_regular_identifier() {
            let name = self.regular_identifier()?;
            return Ok(Spanned(BindingTableExpr::Name(name.0), name.1));
        }
        self.fail_rule("a binding table expression")
    }

    /// Whether the current `{` starts a record literal: `{}` or
    /// `{ fieldName :`.
    pub(super) fn at_record_literal(&self) -> bool {
        self.peek() == T::LeftBrace
            && (self.peek_nth(1) == T::RightBrace
                || (Self::is_identifier(self.peek_nth(1)) && self.peek_nth(2) == T::Colon))
    }

    /// `simpleCatalogModifyingStatement: primitiveCatalogModifyingStatement
    /// | callCatalogModifyingProcedureStatement`, without the `CALL`
    /// alternative (the statement items handle it).
    ///
    /// `primitiveCatalogModifyingStatement`: `CREATE … GRAPH TYPE` and
    /// `CREATE … GRAPH type` share their start, since `TYPE` is a
    /// non-reserved word: `createGraphStatement` (alternative 3) is tried
    /// first, then `createGraphTypeStatement` (5); the same for `DROP`.
    pub(super) fn primitive_catalog_modifying_statement(
        &mut self,
    ) -> PResult<CatalogModifyingStatement> {
        let follow = &follows_catalog_statement;
        let after = |p: &Self| follow(p.peek(), p.peek_nth(1));
        if self.at_kw(K::Create) {
            if self.peek_nth(1) == T::Keyword(K::Schema) {
                return Ok(CatalogModifyingStatement::CreateSchema(
                    self.create_schema_statement()?,
                ));
            }
            let graph = self.try_parse(|p| {
                let s = p.create_graph_statement()?;
                if after(p) {
                    Ok(s)
                } else {
                    p.fail_rule("the end of a statement")
                }
            })?;
            if let Some(s) = graph {
                return Ok(CatalogModifyingStatement::CreateGraph(Box::new(s)));
            }
            return Ok(CatalogModifyingStatement::CreateGraphType(Box::new(
                self.create_graph_type_statement()?,
            )));
        }
        if !self.at_kw(K::Drop) {
            return Err(super::Fail);
        }
        if self.peek_nth(1) == T::Keyword(K::Schema) {
            return Ok(CatalogModifyingStatement::DropSchema(
                self.drop_schema_statement()?,
            ));
        }
        let graph = self.try_parse(|p| {
            let s = p.drop_graph_statement()?;
            if after(p) {
                Ok(s)
            } else {
                p.fail_rule("the end of a statement")
            }
        })?;
        if let Some(s) = graph {
            return Ok(CatalogModifyingStatement::DropGraph(s));
        }
        Ok(CatalogModifyingStatement::DropGraphType(
            self.drop_graph_type_statement()?,
        ))
    }

    /// `createSchemaStatement: CREATE SCHEMA (IF NOT EXISTS)?
    /// catalogSchemaParentAndName`.
    fn create_schema_statement(&mut self) -> PResult<CreateSchemaStatement> {
        self.expect_kw(K::Create)?;
        self.expect_kw(K::Schema)?;
        let if_not_exists = self.if_not_exists()?;
        let path = self.catalog_schema_parent_and_name()?;
        Ok(CreateSchemaStatement {
            path,
            if_not_exists,
        })
    }

    fn if_not_exists(&mut self) -> PResult<bool> {
        if self.eat_kw(K::If) {
            self.expect_kw(K::Not)?;
            self.expect_kw(K::Exists)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn if_exists(&mut self) -> PResult<bool> {
        if self.eat_kw(K::If) {
            self.expect_kw(K::Exists)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// `dropSchemaStatement: DROP SCHEMA (IF EXISTS)?
    /// catalogSchemaParentAndName`.
    fn drop_schema_statement(&mut self) -> PResult<DropSchemaStatement> {
        self.expect_kw(K::Drop)?;
        self.expect_kw(K::Schema)?;
        let if_exists = self.if_exists()?;
        let path = self.catalog_schema_parent_and_name()?;
        Ok(DropSchemaStatement { path, if_exists })
    }

    /// `CREATE (PROPERTY? GRAPH [TYPE] (IF NOT EXISTS)? | OR REPLACE
    /// PROPERTY? GRAPH [TYPE])`: the kind.
    fn create_kind(&mut self, graph_type: bool) -> PResult<Spanned<CreateKind>> {
        let start = self.start();
        self.expect_kw(K::Create)?;
        let replace = self.eat_kw(K::Or);
        if replace {
            self.expect_kw(K::Replace)?;
        }
        if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
            self.bump();
        }
        self.expect_kw(K::Graph)?;
        if graph_type {
            self.expect_kw(K::Type)?;
        }
        let kind = if replace {
            CreateKind::CreateOrReplace
        } else if self.if_not_exists()? {
            CreateKind::CreateIfNotExists
        } else {
            CreateKind::Create
        };
        Ok(self.spanned(start, kind))
    }

    /// `createGraphStatement: CREATE (PROPERTY? GRAPH (IF NOT EXISTS)? |
    /// OR REPLACE PROPERTY? GRAPH) catalogGraphParentAndName
    /// (openGraphType | ofGraphType) graphSource?`.
    fn create_graph_statement(&mut self) -> PResult<CreateGraphStatement> {
        let kind = self.create_kind(false)?;
        let path = self.catalog_object_parent_and_name(&starts_of_graph_type, "a graph name")?;
        let graph_type = self.of_graph_type()?;
        let source = if self.at_kw(K::As) {
            Some(self.graph_source()?)
        } else {
            None
        };
        Ok(CreateGraphStatement {
            path,
            kind,
            graph_type,
            source,
        })
    }

    /// `openGraphType: typed? ANY (PROPERTY? GRAPH)?` and `ofGraphType:
    /// graphTypeLikeGraph | typed? graphTypeReference | typed? (PROPERTY?
    /// GRAPH)? nestedGraphTypeSpecification`.
    ///
    /// After `typed?`, `GRAPH` and `PROPERTY` are non-reserved words, so
    /// they name a graph type unless `[PROPERTY] GRAPH {` follows.
    fn of_graph_type(&mut self) -> PResult<Spanned<OfGraphType>> {
        let start = self.start();
        if self.at_kw(K::Like) {
            let g = self.graph_type_like_graph()?;
            return Ok(self.spanned(start, OfGraphType::Like(g)));
        }
        self.typed();
        if self.eat_kw(K::Any) {
            if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
                self.bump();
            }
            self.eat_kw(K::Graph);
            return Ok(self.spanned(start, OfGraphType::Any));
        }
        let nested = match (self.peek(), self.peek_nth(1), self.peek_nth(2)) {
            (T::LeftBrace, _, _) => Some(0),
            (T::Keyword(K::Graph), T::LeftBrace, _) => Some(1),
            (T::Keyword(K::Property), T::Keyword(K::Graph), T::LeftBrace) => Some(2),
            _ => None,
        };
        if let Some(n) = nested {
            self.pos += n;
            let g = self.nested_graph_type_specification()?;
            return Ok(self.spanned(start, OfGraphType::Nested(g)));
        }
        self.at(T::LeftBrace);
        self.at_kw(K::Any);
        self.at_kw(K::Like);
        let r = self.graph_type_reference(&follows_of_graph_type)?;
        Ok(self.spanned(start, OfGraphType::Ref(r)))
    }

    /// `graphTypeLikeGraph: LIKE graphExpression`.
    fn graph_type_like_graph(&mut self) -> PResult<Spanned<GraphExpr>> {
        self.expect_kw(K::Like)?;
        self.graph_expression(&follows_of_graph_type)
    }

    /// `graphSource: AS COPY OF graphExpression`.
    fn graph_source(&mut self) -> PResult<Spanned<GraphExpr>> {
        self.expect_kw(K::As)?;
        self.expect_kw(K::Copy)?;
        self.expect_kw(K::Of)?;
        self.graph_expression(&follows_catalog_statement)
    }

    /// `dropGraphStatement: DROP PROPERTY? GRAPH (IF EXISTS)?
    /// catalogGraphParentAndName`.
    fn drop_graph_statement(&mut self) -> PResult<DropStatement> {
        self.expect_kw(K::Drop)?;
        if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
            self.bump();
        }
        self.expect_kw(K::Graph)?;
        let if_exists = self.if_exists()?;
        let path =
            self.catalog_object_parent_and_name(&follows_catalog_statement, "a graph name")?;
        Ok(DropStatement { path, if_exists })
    }

    /// `createGraphTypeStatement: CREATE (PROPERTY? GRAPH TYPE (IF NOT
    /// EXISTS)? | OR REPLACE PROPERTY? GRAPH TYPE)
    /// catalogGraphTypeParentAndName graphTypeSource`.
    fn create_graph_type_statement(&mut self) -> PResult<CreateGraphTypeStatement> {
        let kind = self.create_kind(true)?;
        let path = self.catalog_object_parent_and_name(
            &|next, _| matches!(next, T::Keyword(K::As | K::Copy | K::Like) | T::LeftBrace),
            "a graph type name",
        )?;
        let source = self.graph_type_source()?;
        Ok(CreateGraphTypeStatement { path, kind, source })
    }

    /// `graphTypeSource: AS? copyOfGraphType | graphTypeLikeGraph | AS?
    /// nestedGraphTypeSpecification`.
    fn graph_type_source(&mut self) -> PResult<Spanned<GraphTypeSource>> {
        let start = self.start();
        if self.at_kw(K::Like) {
            let g = self.graph_type_like_graph()?;
            return Ok(self.spanned(start, GraphTypeSource::Like(g)));
        }
        self.eat_kw(K::As);
        if self.eat_kw(K::Copy) {
            // copyOfGraphType: COPY OF graphTypeReference.
            self.expect_kw(K::Of)?;
            let r = self.graph_type_reference(&follows_catalog_statement)?;
            return Ok(self.spanned(start, GraphTypeSource::Copy(r)));
        }
        let g = self.nested_graph_type_specification()?;
        Ok(self.spanned(start, GraphTypeSource::Nested(g)))
    }

    /// `dropGraphTypeStatement: DROP PROPERTY? GRAPH TYPE (IF EXISTS)?
    /// catalogGraphTypeParentAndName`.
    fn drop_graph_type_statement(&mut self) -> PResult<DropStatement> {
        self.expect_kw(K::Drop)?;
        if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
            self.bump();
        }
        self.expect_kw(K::Graph)?;
        self.expect_kw(K::Type)?;
        let if_exists = self.if_exists()?;
        let path =
            self.catalog_object_parent_and_name(&follows_catalog_statement, "a graph type name")?;
        Ok(DropStatement { path, if_exists })
    }
}

/// After the name of a graph being created: its graph type.
fn starts_of_graph_type(next: T, _: T) -> bool {
    matches!(
        next,
        T::DoubleColon
            | T::Keyword(K::Typed | K::Any | K::Like)
            | T::LeftBrace
            | T::Solidus
            | T::Period
            | T::DoublePeriod
            | T::SubstitutedParameter
            | T::Keyword(K::HomeSchema | K::CurrentSchema)
    ) || Parser::is_identifier(next)
}

/// After the graph type of a graph being created: its source or the end
/// of the statement.
fn follows_of_graph_type(next: T, after: T) -> bool {
    next == T::Keyword(K::As) || follows_catalog_statement(next, after)
}
