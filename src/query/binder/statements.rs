//! Binding of query and data-modifying statements: linear composition,
//! `MATCH`, `RETURN`, `SELECT`, `ORDER BY`, `INSERT`, `SET`, `REMOVE`
//! (rmp #43).

use super::Binder;
use super::expr::ExprCtx;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::aggregate::AggKind;
use crate::query::ast;
use crate::query::bound::{Part, Project, SetItem, Ty, VarKind};
use crate::query::expr::{Expr, Slot};
use crate::query::features::{gate, unsupported};
use crate::query::span::Spanned;

/// A 42000 error: a syntax or access rule the statement breaks.
pub(super) fn rule(what: &str) -> Error {
    Error::new(
        codes::C42000,
        format!("syntax error or access rule violation: {what}"),
    )
}

/// One projected item: its column name and its value.
struct Item<'x> {
    name: Box<str>,
    src: Src<'x>,
}

/// The value of a projected item.
enum Src<'x> {
    /// An expression of the statement.
    Expr(&'x Spanned<ast::Expr>),
    /// A variable of `*`.
    Slot(Slot),
}

impl Binder<'_> {
    pub(super) fn bind_composite(&mut self, q: &ast::CompositeQueryExpr) -> Result<(), Error> {
        // A query allows only reads (a read-write procedure fails, rmp #50).
        let outer = std::mem::replace(&mut self.reads_only, true);
        let r = self.bind_composite_inner(q);
        self.reads_only = outer;
        r
    }

    fn bind_composite_inner(&mut self, q: &ast::CompositeQueryExpr) -> Result<(), Error> {
        match q {
            ast::CompositeQueryExpr::Conjunction { conjunction, .. } => {
                let code = match conjunction.0 {
                    ast::QueryConjunction::Otherwise => "GQ02",
                    ast::QueryConjunction::SetOp { kind, quantifier } => match (kind, quantifier) {
                        (ast::SetOpKind::Union, _) => "GQ03",
                        (ast::SetOpKind::Except, Some(ast::SetQuantifier::All)) => "GQ05",
                        (ast::SetOpKind::Except, _) => "GQ04",
                        (ast::SetOpKind::Intersect, Some(ast::SetQuantifier::All)) => "GQ07",
                        (ast::SetOpKind::Intersect, _) => "GQ06",
                    },
                };
                Err(self.locate(unsupported(code), conjunction.1))
            }
            ast::CompositeQueryExpr::Primary(lq) => self.bind_linear_query(lq),
        }
    }

    fn use_graph(&mut self, g: &Spanned<ast::GraphExpr>) -> Result<(), Error> {
        self.at(g.1, |b| {
            gate("GQ01")?;
            b.current_graph = b.resolve_graph_expr(&g.0)?;
            Ok(())
        })
    }

    fn bind_linear_query(&mut self, lq: &ast::LinearQueryStatement) -> Result<(), Error> {
        match lq {
            ast::LinearQueryStatement::Focused(f) => match f {
                ast::FocusedLinearQueryStatement::Parts { parts, result } => {
                    for Spanned(part, _) in parts {
                        self.use_graph(&part.use_graph)?;
                        for Spanned(s, span) in &part.statements {
                            self.at(*span, |b| b.bind_simple_query(s))?;
                        }
                    }
                    self.at(result.1, |b| b.bind_result(&result.0))
                }
                ast::FocusedLinearQueryStatement::Result { use_graph, result } => {
                    self.use_graph(use_graph)?;
                    self.at(result.1, |b| b.bind_result(&result.0))
                }
                ast::FocusedLinearQueryStatement::Nested { use_graph, query } => {
                    self.use_graph(use_graph)?;
                    self.at(query.1, |b| b.bind_nested(&query.0))
                }
                ast::FocusedLinearQueryStatement::Select(s) => self.bind_select(s),
            },
            ast::LinearQueryStatement::Ambient(a) => match a {
                ast::AmbientLinearQueryStatement::Parts { statements, result } => {
                    for Spanned(s, span) in statements {
                        self.at(*span, |b| b.bind_simple_query(s))?;
                    }
                    self.at(result.1, |b| b.bind_result(&result.0))
                }
                ast::AmbientLinearQueryStatement::Nested(p) => {
                    self.at(p.1, |b| b.bind_nested(&p.0))
                }
            },
        }
    }

    /// A nested query or data-modifying procedure, bound inline: its
    /// statements continue the pipeline and its result becomes the scope.
    pub(super) fn bind_nested(&mut self, p: &ast::Procedure) -> Result<(), Error> {
        if let Some(Spanned(_, span)) = &p.at {
            return Err(self.locate(unsupported("GP16"), *span));
        }
        if let Some(Spanned(_, span)) = p.binding_variable_defs.first() {
            return Err(self.locate(unsupported("GP17"), *span));
        }
        if let Some(Spanned(_, span)) = p.next_statements.first() {
            return Err(self.locate(unsupported("GQ20"), *span));
        }
        let Spanned(st, span) = &p.statement;
        self.at(*span, |b| match st {
            ast::Statement::Query(q) => b.bind_composite(q),
            ast::Statement::Data(d) => b.bind_data(d),
            ast::Statement::Catalog(_) => Err(rule(
                "a catalog-modifying statement inside a nested query or data-modifying procedure",
            )),
        })
    }

    pub(super) fn bind_data(&mut self, d: &ast::LinearDataModifyingStatement) -> Result<(), Error> {
        if let Some(g) = &d.use_graph {
            self.use_graph(g)?;
        }
        match &d.body {
            ast::DataModifyingBody::Statements { statements, result } => {
                for Spanned(s, span) in statements {
                    self.at(*span, |b| match s {
                        ast::SimpleDataAccessingStatement::Query(q) => b.bind_simple_query(q),
                        ast::SimpleDataAccessingStatement::Modifying(m) => b.bind_modifying(m),
                    })?;
                }
                if let Some(Spanned(r, span)) = result {
                    self.at(*span, |b| b.bind_result(r))?;
                }
                Ok(())
            }
            ast::DataModifyingBody::Nested(p) => self.at(p.1, |b| b.bind_nested(&p.0)),
        }
    }

    fn bind_simple_query(&mut self, s: &ast::SimpleQueryStatement) -> Result<(), Error> {
        match s {
            ast::SimpleQueryStatement::Match(m) => self.bind_match(m, false),
            ast::SimpleQueryStatement::Let(_) => Err(unsupported("GQ09")),
            ast::SimpleQueryStatement::For(_) => Err(unsupported("GQ10")),
            ast::SimpleQueryStatement::Filter(_) => Err(unsupported("GQ08")),
            ast::SimpleQueryStatement::OrderByAndPage(o) => {
                self.page(o)?;
                let mut keys = Vec::new();
                for Spanned(spec, span) in &o.order_by {
                    keys.push(self.at(*span, |b| b.bind_scope_sort_key(spec))?);
                }
                if !keys.is_empty() {
                    self.parts.push(Part::Sort(keys));
                }
                Ok(())
            }
            ast::SimpleQueryStatement::Call(c) => self.bind_call(c),
        }
    }

    /// `OFFSET` and `LIMIT` (GQ12, GQ13).
    fn page(&self, o: &ast::OrderByAndPage) -> Result<(), Error> {
        if let Some(Spanned(_, span)) = &o.offset {
            return Err(self.locate(unsupported("GQ12"), *span));
        }
        if let Some(Spanned(_, span)) = &o.limit {
            return Err(self.locate(unsupported("GQ13"), *span));
        }
        Ok(())
    }

    /// A `CALL` of a named procedure (GP04; rmp #50, TS-24 Names and
    /// calls): the routine is looked up by its whole dotted name in the
    /// session's registry; a schema part or a reference parameter names no
    /// routine (42N08). A read-write routine fails with 42N02 inside a
    /// query, where only reads are allowed. Arguments fill the parameters
    /// in order, then their defaults. Without `YIELD`, every result column
    /// becomes a variable of its name.
    pub(super) fn bind_call(&mut self, c: &ast::CallProcedureStatement) -> Result<(), Error> {
        let n = match &c.procedure.0 {
            ast::ProcedureCall::Inline(_) => return Err(unsupported("GP01")),
            ast::ProcedureCall::Named(n) => n,
        };
        gate("GP04")?;
        if c.optional {
            // `OPTIONAL CALL` has no feature code of its own in the
            // features insert; it is outside the approved set (TS-19).
            return Err(rule("OPTIONAL CALL is not supported"));
        }
        let text = self.text_of(n.name.1).to_string();
        let no_such = || Error::new(codes::C42N08, format!("no such procedure: {text}"));
        let name = match &n.name.0 {
            ast::ProcedureRef::Parameter(_) => return Err(no_such()),
            ast::ProcedureRef::Object(o) => match &o.parent {
                Some(Spanned(p, _)) if p.schema.is_some() => return Err(no_such()),
                Some(Spanned(p, _)) => {
                    let mut parts: Vec<&str> = p.objects.iter().map(|Spanned(i, _)| &**i).collect();
                    parts.push(&o.name.0);
                    parts.join(".")
                }
                None => o.name.0.to_string(),
            },
        };
        let routine = self.session.procs.get(&name).ok_or_else(no_such)?;
        let (mode, params, columns) = {
            let r = routine.get();
            (r.mode(), r.sig.params.clone(), r.sig.columns.clone())
        };
        if mode == crate::proc::Mode::ReadWrite && self.reads_only {
            return Err(Error::new(
                codes::C42N02,
                format!(
                    "writing in read access mode: the procedure {name} may modify data and is called in a query"
                ),
            ));
        }
        if n.args.len() > params.len() {
            return Err(Error::new(
                codes::C22N11,
                format!(
                    "invalid argument: the procedure {name} takes at most {} arguments",
                    params.len()
                ),
            ));
        }
        let mut args = Vec::with_capacity(params.len());
        for a in &n.args {
            args.push(self.bind_value(a)?);
        }
        for p in &params[n.args.len()..] {
            match &p.default {
                Some(v) => args.push(Expr::Lit(v.clone())),
                None => {
                    return Err(Error::new(
                        codes::C22N11,
                        format!(
                            "invalid argument: the procedure {name} needs its argument {}",
                            p.name
                        ),
                    ));
                }
            }
        }
        let wanted: Vec<(usize, Box<str>)> = match &n.yield_clause {
            None => columns
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.as_str().into()))
                .collect(),
            Some(Spanned(items, _)) => {
                let mut out = Vec::new();
                for Spanned(item, span) in items {
                    let col = columns
                        .iter()
                        .position(|c| **c == *item.name.0)
                        .ok_or_else(|| {
                            self.locate(
                                Error::new(
                                    codes::C42002,
                                    format!("invalid reference: the procedure {name} has no result column {}", item.name.0),
                                ),
                                *span,
                            )
                        })?;
                    let var = item.alias.as_ref().map_or(&item.name.0, |a| &a.0);
                    out.push((col, var.clone()));
                }
                out
            }
        };
        let mut yields = Vec::new();
        for (col, var) in wanted {
            if self.lookup(&var).is_some() {
                return Err(rule(&format!("the variable '{var}' is already bound")));
            }
            let slot = self.slot(&var, VarKind::Value, Ty::Any, None);
            self.scope.push((var, slot));
            yields.push((col, slot));
        }
        let graph = self.graph_index(self.current_graph);
        match mode {
            crate::proc::Mode::ReadOnly => self.reads = true,
            crate::proc::Mode::ReadWrite => self.writes = true,
        }
        self.parts.push(Part::Call {
            routine,
            args,
            yields,
            graph,
        });
        Ok(())
    }

    fn bind_match(&mut self, m: &ast::MatchStatement, optional: bool) -> Result<(), Error> {
        match m {
            ast::MatchStatement::Simple(t) => self.at(t.1, |b| b.match_table(&t.0, optional)),
            ast::MatchStatement::Optional(Spanned(op, span)) => match op {
                ast::OptionalOperand::Match(t) => self.at(t.1, |b| b.match_table(&t.0, true)),
                ast::OptionalOperand::Block(stmts) => {
                    if stmts.len() > 1 {
                        return Err(self.locate(unsupported("GQ21"), *span));
                    }
                    match stmts.first() {
                        Some(Spanned(inner, s)) => self.at(*s, |b| b.bind_match(inner, true)),
                        None => Ok(()),
                    }
                }
            },
        }
    }

    fn match_table(
        &mut self,
        t: &ast::GraphPatternBindingTable,
        optional: bool,
    ) -> Result<(), Error> {
        if let Some(Spanned(_, span)) = t.yield_clause.first() {
            return Err(self.locate(unsupported("GQ19"), *span));
        }
        let graph = self.graph_index(self.current_graph);
        let pattern = self.at(t.pattern.1, |b| b.bind_graph_pattern(&t.pattern.0, graph))?;
        self.reads = true;
        self.parts.push(Part::Match {
            optional,
            graph,
            pattern,
        });
        Ok(())
    }

    fn bind_modifying(&mut self, m: &ast::DataModifyingStatement) -> Result<(), Error> {
        gate("GD01")?;
        let graph = self.graph_index(self.current_graph);
        self.writes = true;
        match m {
            ast::DataModifyingStatement::Insert(patterns) => {
                let pattern = self.bind_insert(patterns, graph)?;
                self.parts.push(Part::Insert { graph, pattern });
            }
            ast::DataModifyingStatement::Set(items) => {
                let mut out = Vec::new();
                for Spanned(item, span) in items {
                    out.push(self.at(*span, |b| b.bind_set_item(item))?);
                }
                self.parts.push(Part::Set { graph, items: out });
            }
            ast::DataModifyingStatement::Remove(items) => {
                let mut out = Vec::new();
                for Spanned(item, span) in items {
                    out.push(self.at(*span, |b| match item {
                        ast::RemoveItem::Property { variable, property } => {
                            let s = b.element(&variable.0)?;
                            Ok((s, b.prop_key(&property.0)))
                        }
                        ast::RemoveItem::Label { .. } => Err(unsupported("GD02")),
                    })?);
                }
                self.parts.push(Part::Remove { graph, items: out });
            }
            ast::DataModifyingStatement::Delete { items, .. } => {
                let simple = items
                    .iter()
                    .all(|Spanned(e, _)| matches!(e, ast::Expr::Variable(_)));
                return Err(unsupported(if simple { "GD04" } else { "GD03" }));
            }
        }
        Ok(())
    }

    /// The slot of the visible element variable `name`.
    pub(super) fn element(&self, name: &str) -> Result<Slot, Error> {
        match self.lookup(name) {
            Some(s) if self.slots[s].kind != VarKind::Value => Ok(s),
            Some(_) => Err(rule(&format!("'{name}' is not a node or edge variable"))),
            None => Err(undefined(name)),
        }
    }

    fn bind_set_item(&mut self, item: &ast::SetItem) -> Result<SetItem, Error> {
        match item {
            ast::SetItem::Property {
                variable,
                property,
                value,
            } => {
                let s = self.element(&variable.0)?;
                let key = self.prop_key(&property.0);
                let v = self.bind_value(value)?;
                Ok(SetItem::Prop(s, key, v))
            }
            ast::SetItem::AllProperties {
                variable,
                properties,
            } => {
                let s = self.element(&variable.0)?;
                let mut props = Vec::new();
                for Spanned(kv, span) in properties {
                    let key = self.prop_key(&kv.name.0);
                    let v = self.at(*span, |b| b.bind_value(&kv.value))?;
                    props.push((key, v));
                }
                Ok(SetItem::All(s, props))
            }
            ast::SetItem::Label { .. } => Err(unsupported("GD02")),
        }
    }

    // ---------------------------------------------------------------
    // Results and projections.

    fn bind_result(&mut self, r: &ast::ResultStatement) -> Result<(), Error> {
        match r {
            ast::ResultStatement::Finish => {
                self.columns.clear();
                self.scope.clear();
                Ok(())
            }
            ast::ResultStatement::Return {
                statement,
                order_by,
            } => {
                let before = self.scope.clone();
                self.at(statement.1, |b| b.bind_return(&statement.0))?;
                if let Some(o) = order_by {
                    self.page(&o.0)?;
                    let mut keys = Vec::new();
                    for Spanned(spec, span) in &o.0.order_by {
                        keys.push(self.at(*span, |b| b.bind_result_sort_key(spec, &before))?);
                    }
                    if !keys.is_empty() {
                        self.parts.push(Part::Sort(keys));
                    }
                }
                Ok(())
            }
        }
    }

    fn bind_return(&mut self, r: &ast::ReturnStatement) -> Result<(), Error> {
        if let Some(Spanned(_, span)) = &r.group_by {
            return Err(self.locate(unsupported("GQ15"), *span));
        }
        let distinct = matches!(r.quantifier, Some(Spanned(ast::SetQuantifier::Distinct, _)));
        let items = self.items(&r.items)?;
        self.bind_projection(&items, distinct, None)
    }

    /// The items of `RETURN` or `SELECT`; `*` gives every visible variable.
    fn items<'x>(&self, items: &'x Spanned<ast::ReturnItems>) -> Result<Vec<Item<'x>>, Error> {
        match &items.0 {
            // `*`: every visible variable, in the order they were declared.
            ast::ReturnItems::All => {
                let mut out: Vec<Item<'x>> = Vec::new();
                for (n, s) in &self.scope {
                    if !n.starts_with('$') && !out.iter().any(|i| i.name == *n) {
                        out.push(Item {
                            name: n.clone(),
                            src: Src::Slot(*s),
                        });
                    }
                }
                if out.is_empty() {
                    return Err(self.locate(rule("RETURN * with no variable in scope"), items.1));
                }
                Ok(out)
            }
            ast::ReturnItems::Items(list) => Ok(list
                .iter()
                .map(|Spanned(i, _)| {
                    let name: Box<str> = match (&i.alias, &i.value.0) {
                        (Some(Spanned(a, _)), _) => a.clone(),
                        (None, ast::Expr::Variable(v)) => v.clone(),
                        (None, _) => self.text_of(i.value.1).trim().into(),
                    };
                    Item {
                        name,
                        src: Src::Expr(&i.value),
                    }
                })
                .collect()),
        }
    }

    /// Binds a projection and makes its columns the scope (zu
    /// `binder.rs:4492-4620`: implicit grouping by the non-aggregate
    /// items).
    fn bind_projection(
        &mut self,
        items: &[Item<'_>],
        distinct: bool,
        having: Option<&Spanned<ast::Expr>>,
    ) -> Result<(), Error> {
        let mut aggs: Vec<(Slot, AggKind, bool, Expr)> = Vec::new();
        let mut bound: Vec<(Box<str>, Expr, bool)> = Vec::new();
        for it in items {
            if bound.iter().any(|(n, _, _)| *n == it.name) {
                let e = rule(&format!("the column name '{}' is used twice", it.name));
                return Err(match it.src {
                    Src::Expr(x) => self.locate(e, x.1),
                    Src::Slot(_) => e,
                });
            }
            match it.src {
                Src::Slot(s) => bound.push((it.name.clone(), Expr::Slot(s), false)),
                Src::Expr(x) => {
                    let mut ctx = ExprCtx::aggregating(&mut aggs);
                    let e = self.at(x.1, |b| b.bind_expr(&x.0, &mut ctx))?;
                    let saw = ctx.saw_aggregate;
                    bound.push((it.name.clone(), e, saw));
                }
            }
        }
        let having = match having {
            None => None,
            Some(h) => {
                let mut ctx = ExprCtx::aggregating(&mut aggs);
                let e = self.at(h.1, |b| b.bind_expr(&h.0, &mut ctx))?;
                Some(e)
            }
        };
        let grouped = !aggs.is_empty() || having.is_some();
        let mut output = Vec::new();
        let mut plain = Vec::new();
        let mut post = Vec::new();
        let mut scope = Vec::new();
        for (name, e, saw) in bound {
            let (kind, ty, graph) = match &e {
                Expr::Slot(s) => (self.slots[*s].kind, self.slots[*s].ty, self.slots[*s].graph),
                _ => (VarKind::Value, Ty::Any, None),
            };
            let out = self.slot(&name, kind, ty, graph);
            output.push(out);
            scope.push((name, out));
            if grouped && saw {
                post.push((out, e));
            } else {
                plain.push((out, e));
            }
        }
        let agg_slots: Vec<Slot> = aggs.iter().map(|a| a.0).collect();
        let rewrite = |e: Expr| -> Result<Expr, Error> { regroup(e, &plain, &agg_slots) };
        let post = post
            .into_iter()
            .map(|(s, e)| Ok((s, rewrite(e)?)))
            .collect::<Result<Vec<_>, Error>>()?;
        let having = having.map(rewrite).transpose()?;
        self.parts.push(Part::Project(Project {
            items: plain,
            aggregates: aggs,
            post,
            distinct,
            having,
            output: output.clone(),
        }));
        self.columns = scope.clone();
        self.scope = scope;
        Ok(())
    }

    /// A sort key after `RETURN`: a result column (GQ14 for any other
    /// expression, GQ16 for a variable the projection renamed, GA07 for one
    /// it discarded, GF20 for an aggregate).
    fn bind_result_sort_key(
        &mut self,
        spec: &ast::SortSpec,
        before: &[(Box<str>, Slot)],
    ) -> Result<(Expr, bool), Error> {
        if let Some(Spanned(_, span)) = &spec.null_ordering {
            return Err(self.locate(unsupported("GA03"), *span));
        }
        let desc = matches!(spec.ordering, Some(Spanned(ast::Ordering::Desc, _)));
        match &spec.key.0 {
            ast::Expr::Variable(v) => {
                if let Some((_, s)) = self.columns.iter().find(|(n, _)| n == v) {
                    return Ok((Expr::Slot(*s), desc));
                }
                if let Some((_, pre)) = before.iter().rev().find(|(n, _)| n == v) {
                    let renamed = self.parts.iter().rev().find_map(|p| match p {
                        Part::Project(pr) => {
                            Some(pr.items.iter().any(|(_, e)| *e == Expr::Slot(*pre)))
                        }
                        _ => None,
                    });
                    return Err(unsupported(if renamed == Some(true) {
                        "GQ16"
                    } else {
                        "GA07"
                    }));
                }
                Err(undefined(v))
            }
            e => Err(unsupported(if has_aggregate(e) { "GF20" } else { "GQ14" })),
        }
    }

    /// A sort key of an `ORDER BY` statement: a visible variable (GQ14
    /// for any other expression, GF20 for an aggregate).
    fn bind_scope_sort_key(&mut self, spec: &ast::SortSpec) -> Result<(Expr, bool), Error> {
        if let Some(Spanned(_, span)) = &spec.null_ordering {
            return Err(self.locate(unsupported("GA03"), *span));
        }
        let desc = matches!(spec.ordering, Some(Spanned(ast::Ordering::Desc, _)));
        match &spec.key.0 {
            ast::Expr::Variable(v) => match self.lookup(v) {
                Some(s) => Ok((Expr::Slot(s), desc)),
                None => Err(undefined(v)),
            },
            e => Err(unsupported(if has_aggregate(e) { "GF20" } else { "GQ14" })),
        }
    }

    fn bind_select(&mut self, sel: &ast::SelectStatement) -> Result<(), Error> {
        let distinct = matches!(
            sel.quantifier,
            Some(Spanned(ast::SetQuantifier::Distinct, _))
        );
        let mut having = None;
        let mut order = None;
        if let Some(body) = &sel.body {
            match &body.from.0 {
                ast::SelectFrom::GraphMatches(list) => {
                    for Spanned(gm, span) in list {
                        self.at(*span, |b| {
                            b.current_graph = b.resolve_graph_expr(&gm.graph.0)?;
                            b.bind_match(&gm.statement.0, false)
                        })?;
                    }
                }
                ast::SelectFrom::Query { graph, query } => {
                    if let Some(g) = graph {
                        self.at(g.1, |b| {
                            b.current_graph = b.resolve_graph_expr(&g.0)?;
                            Ok(())
                        })?;
                    }
                    self.at(query.1, |b| b.bind_nested(&query.0))?;
                }
            }
            if let Some(w) = &body.where_clause {
                let e = self.at(w.1, |b| b.bind_value(w))?;
                self.parts.push(Part::Filter(e));
            }
            if let Some(Spanned(_, span)) = &body.group_by {
                return Err(self.locate(unsupported("GQ15"), *span));
            }
            if let Some(Spanned(_, span)) = &body.offset {
                return Err(self.locate(unsupported("GQ12"), *span));
            }
            if let Some(Spanned(_, span)) = &body.limit {
                return Err(self.locate(unsupported("GQ13"), *span));
            }
            having = body.having.as_ref();
            order = body.order_by.as_ref();
        }
        let before = self.scope.clone();
        let items = self.items(&sel.items)?;
        self.bind_projection(&items, distinct, having)?;
        if let Some(specs) = order {
            let mut keys = Vec::new();
            for Spanned(spec, span) in specs {
                keys.push(self.at(*span, |b| b.bind_result_sort_key(spec, &before))?);
            }
            if !keys.is_empty() {
                self.parts.push(Part::Sort(keys));
            }
        }
        Ok(())
    }
}

/// The 42002 "invalid reference" error of an undefined variable.
pub(super) fn undefined(name: &str) -> Error {
    Error::new(
        codes::C42002,
        format!("invalid reference: the variable '{name}' is not defined"),
    )
}

/// Whether an AST expression holds an aggregate.
fn has_aggregate(e: &ast::Expr) -> bool {
    // Aggregates sit only in value positions; a textual walk over the
    // expression's debug form would be fragile, so the binder asks the
    // one shape that matters for sort keys: an aggregate call at the top,
    // or under unary and binary operators and parentheses.
    match e {
        ast::Expr::Aggregate(_) => true,
        ast::Expr::Unary { child, .. } | ast::Expr::Parenthesized(child) => has_aggregate(&child.0),
        ast::Expr::Binary { left, right, .. } => has_aggregate(&left.0) || has_aggregate(&right.0),
        _ => false,
    }
}

/// Rewrites an item that combines aggregates, or `HAVING`, to read only
/// the grouping keys' and aggregates' output slots: a slot outside an
/// aggregate must be a grouping key that is exactly that slot.
fn regroup(e: Expr, keys: &[(Slot, Expr)], aggs: &[Slot]) -> Result<Expr, Error> {
    let mut bad = None;
    let out = map_slots(e, &mut |s| {
        if aggs.contains(&s) {
            return Expr::Slot(s);
        }
        match keys.iter().find(|(_, k)| *k == Expr::Slot(s)) {
            Some((o, _)) => Expr::Slot(*o),
            None => {
                bad = Some(s);
                Expr::Slot(s)
            }
        }
    });
    match bad {
        Some(_) => Err(rule(
            "a value read outside an aggregate must be a grouping key returned as a plain variable",
        )),
        None => Ok(out),
    }
}

/// `e` with every slot reference replaced by `f(slot)`.
pub(super) fn map_slots(e: Expr, f: &mut dyn FnMut(Slot) -> Expr) -> Expr {
    let b = |x: Box<Expr>, f: &mut dyn FnMut(Slot) -> Expr| Box::new(map_slots(*x, f));
    match e {
        Expr::Slot(s) => f(s),
        Expr::Prop(x, k) => Expr::Prop(b(x, f), k),
        Expr::Labels(x, t) => Expr::Labels(b(x, f), t),
        Expr::Not(x) => Expr::Not(b(x, f)),
        Expr::And(x, y) => {
            let x = b(x, f);
            Expr::And(x, b(y, f))
        }
        Expr::Or(x, y) => {
            let x = b(x, f);
            Expr::Or(x, b(y, f))
        }
        Expr::IsTruth(x, n, t) => Expr::IsTruth(b(x, f), n, t),
        Expr::IsNull(x, n) => Expr::IsNull(b(x, f), n),
        Expr::IsNormalized(x, n, form) => Expr::IsNormalized(b(x, f), n, form),
        Expr::Cmp(o, x, y) => {
            let x = b(x, f);
            Expr::Cmp(o, x, b(y, f))
        }
        Expr::Arith(o, x, y) => {
            let x = b(x, f);
            Expr::Arith(o, x, b(y, f))
        }
        Expr::Neg(x) => Expr::Neg(b(x, f)),
        Expr::Pos(x) => Expr::Pos(b(x, f)),
        Expr::Concat(x, y) => {
            let x = b(x, f);
            Expr::Concat(x, b(y, f))
        }
        Expr::Call(func, xs) => Expr::Call(func, xs.into_iter().map(|x| map_slots(x, f)).collect()),
        Expr::Coalesce(xs) => Expr::Coalesce(xs.into_iter().map(|x| map_slots(x, f)).collect()),
        Expr::Case(ws, e) => {
            let ws = ws
                .into_iter()
                .map(|(c, r)| {
                    let c = map_slots(c, f);
                    (c, map_slots(r, f))
                })
                .collect();
            Expr::Case(ws, b(e, f))
        }
        Expr::Record(fs) => {
            Expr::Record(fs.into_iter().map(|(n, x)| (n, map_slots(x, f))).collect())
        }
        e @ (Expr::Lit(_) | Expr::Param(_) | Expr::Exists(_) | Expr::SessionUser) => e,
    }
}
