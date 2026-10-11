//! Binding of graph patterns and insert patterns (rmp #43; [GPML] §4–§6,
//! M-QE §Q2).
//!
//! * **Degrees** ([GPML] §4.4, §4.6): a variable declared in only some
//!   operands of a path pattern union or multiset alternation, or under
//!   `?`, is a conditional singleton; one under a quantifier is a group
//!   variable. The analysis runs over the whole pattern before the feature
//!   gates, so its rule is checked even where the constructs that produce
//!   conditional singletons are gated.
//! * **Join rule**: "implicit equi-joins on conditional singletons are
//!   disallowed" ([GPML] §4.6): a conditional singleton that another path
//!   pattern of the graph pattern, or an earlier statement, also binds
//!   fails with 42000.
//! * **Normalisation** (§6.2): a path pattern that starts or ends with an
//!   edge gets an anonymous node there; adjacent node patterns (G046) and
//!   adjacent edge patterns (G047) are gated. An edge pointing left is not
//!   supported (G043), so every bound edge points right.
//! * **Edge uniqueness** (ID086, DIFFERENT EDGES): two edge variables of
//!   one graph pattern never bind the same edge, by a `<>` predicate per
//!   pair.
//! * Element predicates become conjuncts: label tests, property
//!   specifications (`=`), element `WHERE` clauses (a reference to another
//!   element of the pattern is G041), and the graph pattern's `WHERE`.

use std::collections::BTreeMap;

use super::Binder;
use super::statements::rule;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::ast;
use crate::query::bound::{
    Degree, IEdge, INode, InsertPattern, Name, PEdge, PNode, Pattern, Ty, VarKind,
};
use crate::query::expr::{CmpOp, Expr, LabelTest, Slot};
use crate::query::features::unsupported;
use crate::query::span::Spanned;

/// A variable's declarations in one path pattern: its kind and degree.
type Decls = BTreeMap<Box<str>, (VarKind, Degree)>;

/// Degree analysis of one path pattern expression (module documentation).
fn degrees(e: &ast::PathPatternExpr, cond: bool, group: bool, out: &mut Decls) {
    let degree = if group {
        Degree::Group
    } else if cond {
        Degree::Conditional
    } else {
        Degree::Singleton
    };
    match e {
        ast::PathPatternExpr::Union(xs) | ast::PathPatternExpr::Alternation(xs) => {
            let branches: Vec<Decls> = xs
                .iter()
                .map(|Spanned(x, _)| {
                    let mut d = Decls::new();
                    degrees(x, cond, group, &mut d);
                    d
                })
                .collect();
            for b in &branches {
                for (name, (kind, d)) in b {
                    let everywhere = branches.iter().all(|o| o.contains_key(name));
                    let d = if everywhere {
                        *d
                    } else {
                        Degree::Conditional.max_of(*d)
                    };
                    merge(out, name, *kind, d);
                }
            }
        }
        ast::PathPatternExpr::Concat(xs) => {
            for Spanned(x, _) in xs {
                degrees(x, cond, group, out);
            }
        }
        ast::PathPatternExpr::Quantified { path, .. } => degrees(&path.0, cond, true, out),
        ast::PathPatternExpr::Questioned(x) => degrees(&x.0, true, group, out),
        ast::PathPatternExpr::Grouped(g) => degrees(&g.expr.0, cond, group, out),
        ast::PathPatternExpr::Element(el) => {
            let (filler, kind) = match el {
                ast::ElementPattern::Node(f) => (f, VarKind::Node),
                ast::ElementPattern::Edge { filler, .. } => (filler, VarKind::Edge),
            };
            if let Some(Spanned(v, _)) = &filler.variable {
                merge(out, v, kind, degree);
            }
        }
        ast::PathPatternExpr::Simplified(_) => {}
    }
}

impl Degree {
    /// The weaker of two degrees: group over conditional over singleton.
    fn max_of(self, other: Degree) -> Degree {
        match (self, other) {
            (Degree::Group, _) | (_, Degree::Group) => Degree::Group,
            (Degree::Conditional, _) | (_, Degree::Conditional) => Degree::Conditional,
            _ => Degree::Singleton,
        }
    }
}

fn merge(out: &mut Decls, name: &str, kind: VarKind, d: Degree) {
    out.entry(name.into())
        .and_modify(|(_, old)| *old = old.max_of(d))
        .or_insert((kind, d));
}

/// The feature gates of a path pattern expression.
fn gates(e: &ast::PathPatternExpr) -> Result<(), Error> {
    match e {
        ast::PathPatternExpr::Union(_) => Err(unsupported("G032")),
        ast::PathPatternExpr::Alternation(_) => Err(unsupported("G030")),
        ast::PathPatternExpr::Concat(xs) => {
            for Spanned(x, _) in xs {
                gates(x)?;
            }
            Ok(())
        }
        ast::PathPatternExpr::Quantified { path, .. } => Err(unsupported(
            if matches!(
                path.0,
                ast::PathPatternExpr::Element(ast::ElementPattern::Edge { .. })
            ) {
                "G036"
            } else {
                "G035"
            },
        )),
        ast::PathPatternExpr::Questioned(_) => Err(unsupported("G037")),
        ast::PathPatternExpr::Grouped(_) => Err(unsupported("G038")),
        ast::PathPatternExpr::Simplified(_) => Err(unsupported("G039")),
        ast::PathPatternExpr::Element(ast::ElementPattern::Node(_)) => Ok(()),
        ast::PathPatternExpr::Element(ast::ElementPattern::Edge {
            direction,
            abbreviated,
            ..
        }) => {
            if *abbreviated {
                return Err(unsupported("G044"));
            }
            match direction {
                ast::EdgeDirection::Right => Ok(()),
                ast::EdgeDirection::Undirected => Err(unsupported("GH02")),
                _ => Err(unsupported("G043")),
            }
        }
    }
}

/// The element patterns of a supported path pattern, in order.
fn elements(e: &ast::PathPatternExpr) -> Vec<&ast::ElementPattern> {
    match e {
        ast::PathPatternExpr::Concat(xs) => {
            xs.iter().flat_map(|Spanned(x, _)| elements(x)).collect()
        }
        ast::PathPatternExpr::Element(el) => vec![el],
        _ => Vec::new(),
    }
}

impl Binder<'_> {
    /// Binds the graph pattern of a `MATCH` in graph `graph`; its
    /// variables join the scope.
    pub(super) fn bind_graph_pattern(
        &mut self,
        gp: &ast::GraphPattern,
        graph: usize,
    ) -> Result<Pattern, Error> {
        if let Some(Spanned(m, span)) = &gp.match_mode {
            let code = match m {
                ast::MatchMode::DifferentEdges => "G002",
                ast::MatchMode::RepeatableElements => "G003",
            };
            return Err(self.locate(unsupported(code), *span));
        }
        if let Some(Spanned(k, span)) = &gp.keep {
            let code = match k {
                ast::PathPatternPrefix::PathMode(_) => "G006",
                ast::PathPatternPrefix::PathSearch(_) => "G007",
            };
            return Err(self.locate(unsupported(code), *span));
        }
        // Degrees and the conditional-singleton join rule.
        let per_path: Vec<Decls> = gp
            .patterns
            .iter()
            .map(|Spanned(p, _)| {
                let mut d = Decls::new();
                degrees(&p.expr.0, false, false, &mut d);
                d
            })
            .collect();
        for (i, decls) in per_path.iter().enumerate() {
            for (name, (_, d)) in decls {
                if *d != Degree::Conditional {
                    continue;
                }
                let elsewhere = per_path
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && o.contains_key(name));
                if elsewhere || self.lookup(name).is_some() {
                    return Err(rule(&format!(
                        "an implicit join on the conditional singleton '{name}'"
                    )));
                }
            }
        }
        // Feature gates.
        for Spanned(p, span) in &gp.patterns {
            self.at(*span, |b| {
                if p.variable.is_some() {
                    return Err(unsupported("G004"));
                }
                if let Some(Spanned(prefix, _)) = &p.prefix {
                    return Err(unsupported(match prefix {
                        ast::PathPatternPrefix::PathMode(ast::PathMode::Walk) => "G010",
                        ast::PathPatternPrefix::PathMode(ast::PathMode::Trail) => "G011",
                        ast::PathPatternPrefix::PathMode(ast::PathMode::Simple) => "G012",
                        ast::PathPatternPrefix::PathMode(ast::PathMode::Acyclic) => "G013",
                        ast::PathPatternPrefix::PathSearch(_) => "G005",
                    }));
                }
                let _ = &b;
                gates(&p.expr.0)
            })?;
        }
        // Variables: first every element of every path pattern, so that
        // element predicates can name their own element.
        let mut nodes: Vec<PNode> = Vec::new();
        let mut edges: Vec<PEdge> = Vec::new();
        let mut fresh: Vec<Slot> = Vec::new();
        let mut local: Vec<(Box<str>, Slot)> = Vec::new();
        let mut pending: Vec<(Slot, VarKind, &ast::ElementPatternFiller)> = Vec::new();
        for Spanned(p, span) in &gp.patterns {
            let els = elements(&p.expr.0);
            let mut prev_node: Option<usize> = None;
            let mut open_edge: Option<(Slot, &ast::ElementPatternFiller)> = None;
            let mut last_was_node = false;
            for el in els {
                match el {
                    ast::ElementPattern::Node(f) => {
                        if last_was_node {
                            return Err(self.locate(unsupported("G046"), *span));
                        }
                        let slot =
                            self.element_var(f, VarKind::Node, graph, &mut local, &mut fresh)?;
                        let idx = node_index(&mut nodes, slot, &fresh);
                        pending.push((slot, VarKind::Node, f));
                        if let Some((e, ef)) = open_edge.take() {
                            let src = match prev_node {
                                Some(n) => n,
                                None => {
                                    let a = self.hidden(VarKind::Node, Some(graph));
                                    fresh.push(a);
                                    node_index(&mut nodes, a, &fresh)
                                }
                            };
                            edges.push(PEdge {
                                slot: e,
                                src,
                                dst: idx,
                                ty: None,
                                bound: !fresh.contains(&e),
                            });
                            pending.push((e, VarKind::Edge, ef));
                        }
                        prev_node = Some(idx);
                        last_was_node = true;
                    }
                    ast::ElementPattern::Edge { filler, .. } => {
                        if !last_was_node && open_edge.is_some() {
                            return Err(self.locate(unsupported("G047"), *span));
                        }
                        let slot =
                            self.element_var(filler, VarKind::Edge, graph, &mut local, &mut fresh)?;
                        open_edge = Some((slot, filler));
                        last_was_node = false;
                    }
                }
            }
            if let Some((e, ef)) = open_edge {
                let src = match prev_node {
                    Some(n) => n,
                    None => {
                        let a = self.hidden(VarKind::Node, Some(graph));
                        fresh.push(a);
                        node_index(&mut nodes, a, &fresh)
                    }
                };
                let b = self.hidden(VarKind::Node, Some(graph));
                fresh.push(b);
                let dst = node_index(&mut nodes, b, &fresh);
                edges.push(PEdge {
                    slot: e,
                    src,
                    dst,
                    ty: None,
                    bound: !fresh.contains(&e),
                });
                pending.push((e, VarKind::Edge, ef));
            }
        }
        for (name, slot) in &local {
            self.scope.push((name.clone(), *slot));
        }
        // Element predicates.
        let mut predicates = Vec::new();
        for (slot, kind, f) in pending {
            let edge = kind == VarKind::Edge;
            if let Some(Spanned(l, span)) = &f.label {
                let test = self.at(*span, |b| b.label_test(l, edge))?;
                match (&test, edge) {
                    (LabelTest::Label(t), true) if t.token.is_some() => {
                        for e in edges.iter_mut().filter(|e| e.slot == slot) {
                            e.ty = Some(t.token);
                        }
                    }
                    _ => predicates.push(Expr::Labels(Box::new(Expr::Slot(slot)), test)),
                }
            }
            match &f.predicate {
                None => {}
                Some(Spanned(ast::ElementPatternPredicate::Property(pairs), _)) => {
                    for Spanned(kv, span) in pairs {
                        let key = self.tok(crate::catalog::TokenKind::PropertyKey, &kv.name.0);
                        let v = self.at(*span, |b| b.bind_value(&kv.value))?;
                        predicates.push(Expr::Cmp(
                            CmpOp::Eq,
                            Box::new(Expr::Prop(Box::new(Expr::Slot(slot)), key)),
                            Box::new(v),
                        ));
                    }
                }
                Some(Spanned(ast::ElementPatternPredicate::Where(w), span)) => {
                    let e = self.at(*span, |b| b.bind_value(w))?;
                    let others: Vec<Slot> = local
                        .iter()
                        .map(|(_, s)| *s)
                        .filter(|s| *s != slot && fresh.contains(s))
                        .collect();
                    if e.slots().iter().any(|s| others.contains(s)) {
                        return Err(self.locate(unsupported("G041"), *span));
                    }
                    predicates.extend(e.conjuncts());
                }
            }
        }
        if let Some(w) = &gp.where_clause {
            let e = self.at(w.1, |b| b.bind_value(w))?;
            predicates.extend(e.conjuncts());
        }
        // Edge uniqueness (ID086).
        let edge_slots: Vec<Slot> = {
            let mut v: Vec<Slot> = edges.iter().map(|e| e.slot).collect();
            v.dedup();
            v
        };
        for (i, a) in edge_slots.iter().enumerate() {
            for b in &edge_slots[i + 1..] {
                if a != b {
                    predicates.push(Expr::Cmp(
                        CmpOp::Ne,
                        Box::new(Expr::Slot(*a)),
                        Box::new(Expr::Slot(*b)),
                    ));
                }
            }
        }
        Ok(Pattern {
            nodes,
            edges,
            predicates,
        })
    }

    /// The slot of an element variable of a pattern: an outer variable of
    /// the same kind (an implicit join), a variable this pattern already
    /// declared, or a new one.
    fn element_var(
        &mut self,
        f: &ast::ElementPatternFiller,
        kind: VarKind,
        graph: usize,
        local: &mut Vec<(Box<str>, Slot)>,
        fresh: &mut Vec<Slot>,
    ) -> Result<Slot, Error> {
        let Some(Spanned(name, span)) = &f.variable else {
            let s = self.hidden(kind, Some(graph));
            fresh.push(s);
            return Ok(s);
        };
        let found = local
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| *s)
            .or_else(|| self.lookup(name));
        if let Some(s) = found {
            if self.slots[s].kind != kind {
                return Err(self.locate(
                    rule(&format!(
                        "'{name}' is bound twice, as a {} and as a {}",
                        kind_name(self.slots[s].kind),
                        kind_name(kind)
                    )),
                    *span,
                ));
            }
            return Ok(s);
        }
        let ty = if kind == VarKind::Node {
            Ty::Node
        } else {
            Ty::Edge
        };
        let s = self.slot(name, kind, ty, Some(graph));
        fresh.push(s);
        local.push((name.clone(), s));
        Ok(s)
    }

    /// Binds the insert path patterns of an `INSERT` in graph `graph`; its
    /// new variables join the scope.
    pub(super) fn bind_insert(
        &mut self,
        patterns: &[Spanned<ast::InsertPathPattern>],
        graph: usize,
    ) -> Result<InsertPattern, Error> {
        let mut out = InsertPattern {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let mut local: Vec<(Box<str>, usize)> = Vec::new();
        let mut new_names: Vec<(Box<str>, Slot)> = Vec::new();
        for Spanned(p, pspan) in patterns {
            let mut prev = self.at(p.start.1, |b| {
                b.insert_node(&p.start.0, graph, &mut out, &mut local, &mut new_names)
            })?;
            for (Spanned(e, espan), Spanned(n, nspan)) in &p.steps {
                let next = self.at(*nspan, |b| {
                    b.insert_node(n, graph, &mut out, &mut local, &mut new_names)
                })?;
                let (src, dst) = match e.direction {
                    ast::EdgeDirection::Right => (prev, next),
                    ast::EdgeDirection::Left => (next, prev),
                    ast::EdgeDirection::Undirected => {
                        return Err(self.locate(unsupported("GH02"), *espan));
                    }
                    _ => return Err(self.locate(unsupported("G043"), *espan)),
                };
                let edge = self.at(*espan, |b| {
                    b.insert_edge(&e.filler, graph, src, dst, &mut new_names)
                })?;
                out.edges.push(edge);
                prev = next;
            }
            let _ = pspan;
        }
        self.scope.extend(new_names);
        Ok(out)
    }

    fn insert_props(
        &mut self,
        props: Option<&Vec<Spanned<ast::PropertyKeyValuePair>>>,
    ) -> Result<Vec<(Name, Expr)>, Error> {
        let mut out = Vec::new();
        for Spanned(kv, span) in props.into_iter().flatten() {
            let key = self.prop_key(&kv.name.0);
            if out.iter().any(|(k, _): &(Name, Expr)| k.name == key.name) {
                return Err(self.locate(
                    Error::new(
                        codes::C22G0M,
                        format!(
                            "multiple assignments to a graph element property: '{}'",
                            key.name
                        ),
                    ),
                    *span,
                ));
            }
            let v = self.at(*span, |b| b.bind_value(&kv.value))?;
            out.push((key, v));
        }
        Ok(out)
    }

    fn insert_node(
        &mut self,
        n: &ast::InsertElement,
        graph: usize,
        out: &mut InsertPattern,
        local: &mut Vec<(Box<str>, usize)>,
        new_names: &mut Vec<(Box<str>, Slot)>,
    ) -> Result<usize, Error> {
        let specified = n.labels.is_some() || n.properties.is_some();
        if let Some(Spanned(name, _)) = &n.variable {
            if let Some((_, i)) = local.iter().find(|(x, _)| x == name) {
                if specified {
                    return Err(rule(&format!(
                        "the inserted node '{name}' is given labels or properties twice"
                    )));
                }
                return Ok(*i);
            }
            if let Some(s) = self.lookup(name) {
                if self.slots[s].kind != VarKind::Node {
                    return Err(rule(&format!("'{name}' is not a node variable")));
                }
                if specified {
                    return Err(rule(&format!(
                        "the existing node '{name}' cannot be given labels or properties by INSERT"
                    )));
                }
                out.nodes.push(INode {
                    slot: s,
                    existing: true,
                    labels: Vec::new(),
                    props: Vec::new(),
                });
                local.push((name.clone(), out.nodes.len() - 1));
                return Ok(out.nodes.len() - 1);
            }
        }
        let labels: Vec<Name> = n
            .labels
            .iter()
            .flat_map(|Spanned(ls, _)| ls.iter())
            .map(|Spanned(l, _)| self.label(l, false))
            .collect();
        let props = self.insert_props(n.properties.as_ref())?;
        let slot = match &n.variable {
            Some(Spanned(name, _)) => {
                let s = self.slot(name, VarKind::Node, Ty::Node, Some(graph));
                new_names.push((name.clone(), s));
                local.push((name.clone(), out.nodes.len()));
                s
            }
            None => self.hidden(VarKind::Node, Some(graph)),
        };
        out.nodes.push(INode {
            slot,
            existing: false,
            labels,
            props,
        });
        Ok(out.nodes.len() - 1)
    }

    fn insert_edge(
        &mut self,
        e: &ast::InsertElement,
        graph: usize,
        src: usize,
        dst: usize,
        new_names: &mut Vec<(Box<str>, Slot)>,
    ) -> Result<IEdge, Error> {
        let labels: Vec<&Spanned<ast::Ident>> = e
            .labels
            .iter()
            .flat_map(|Spanned(ls, _)| ls.iter())
            .collect();
        let ty = match labels.as_slice() {
            [one] => self.label(&one.0, true),
            [] => {
                return Err(Error::new(
                    codes::C42006,
                    "number of edge labels below supported minimum: an inserted edge has one edge type",
                ));
            }
            _ => {
                return Err(Error::new(
                    codes::C42007,
                    "number of edge labels exceeds supported maximum: an inserted edge has one edge type",
                ));
            }
        };
        let props = self.insert_props(e.properties.as_ref())?;
        let slot = match &e.variable {
            Some(Spanned(name, _)) => {
                if self.lookup(name).is_some() || new_names.iter().any(|(n, _)| n == name) {
                    return Err(rule(&format!("the edge variable '{name}' is bound twice")));
                }
                let s = self.slot(name, VarKind::Edge, Ty::Edge, Some(graph));
                new_names.push((name.clone(), s));
                s
            }
            None => self.hidden(VarKind::Edge, Some(graph)),
        };
        Ok(IEdge {
            slot,
            src,
            dst,
            ty,
            props,
        })
    }
}

fn kind_name(k: VarKind) -> &'static str {
    match k {
        VarKind::Node => "node",
        VarKind::Edge => "edge",
        VarKind::Value => "value",
    }
}

/// The index of node `slot` in `nodes`, added if new; a node not in
/// `fresh` was bound by an earlier statement.
fn node_index(nodes: &mut Vec<PNode>, slot: Slot, fresh: &[Slot]) -> usize {
    match nodes.iter().position(|n| n.slot == slot) {
        Some(i) => i,
        None => {
            nodes.push(PNode {
                slot,
                bound: !fresh.contains(&slot),
            });
            nodes.len() - 1
        }
    }
}
