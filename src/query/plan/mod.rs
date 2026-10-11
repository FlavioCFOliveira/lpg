//! The planner (rmp #44; [TS-20] Planner, EXPLAIN): from a bound
//! [`Query`] to a tree of physical operators ([`Node`]) with estimated
//! rows, and the plan description EXPLAIN returns ([`Description`]).
//!
//! * **Logical and physical in one pass.** The bound form is already the
//!   logical pipeline (one part per statement); the planner lowers each
//!   part to operators and applies the rewrites while it does:
//!   - **predicate pushdown**: each conjunct of a pattern is placed right
//!     after the first operator that binds all its slots; one that reads
//!     only earlier slots runs before the pattern's first expansion;
//!   - **projection pushdown**: a projection computes only its items, and
//!     only the slots the result reads are returned;
//!   - **constant folding**: done by the binder ([`Expr::fold`]);
//!   - **index selection** ([`TS-18`], FR-IDX-001): on ONLINE indexes only
//!     (an index the snapshot sees POPULATING is ignored), an equality on
//!     a leading prefix of a range index's properties, then a point index
//!     for an equality with a point, then a range on a single-property
//!     range index, then a label lookup index; the predicate stays as a
//!     filter after the seek, so a seek only has to return a superset;
//!   - **join order**: [`join`].
//!   - Limit pushdown: not done, `LIMIT` (GQ13) is not supported.
//! * **Estimates** read the TS-17 statistics: node counts per label, edge
//!   counts per `(label, type, direction)`; 0.01 per equality and 0.1 per
//!   other predicate (TS-20).
//!
//! The plan description's fields (operator name, details, identifiers,
//! estimated rows, children; PROFILE adds measured counters) are Neo4j's
//! plan description (Cypher Manual, "Execution plans"; Bolt `PULL`
//! success `plan` and `profile`); Grafeo @ `3c23dbe`
//! `crates/grafeo-engine/src/query/optimizer/{cardinality,cost}.rs` and
//! miniGU @ `fc2ccf5` `minigu/gql/planner/src/optimizer` were studied.
//!
//! [TS-20]: ../../../../docs/technical-spec/query-engine.md
//! [TS-18]: ../../../../docs/technical-spec/catalog-and-indexes.md

pub(crate) mod join;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use super::aggregate::AggKind;
use super::bound::{
    CatalogOp, InsertPattern, PEdge, Part, Pattern, Project, PropKey, Query, SetItem,
};
use super::expr::{CmpOp, Expr, Slot};
use crate::catalog::stats::StatKey;
use crate::error::Error;
use crate::index::{Entity, SchemaDef, SchemaKind, State};
use crate::storage::graph::{IN, OUT};
use crate::tx::Transaction;
use crate::types::Value;

/// An index seek.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Seek {
    /// Equality on the leading properties of a range index.
    Eq(Vec<Expr>),
    /// A range on a single-property range index: lower and upper bound,
    /// inclusive; the filter after the seek makes them exact.
    Range(Option<Expr>, Option<Expr>),
    /// Equality with a point, through a point index.
    Point(Expr),
}

/// A physical operator.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    /// The argument row: the outer row under an `Apply`, or one row of
    /// NULLs at the top.
    Argument,
    /// Every node of a graph.
    AllNodesScan { graph: usize, slot: Slot },
    /// The nodes of a label, through a label lookup index.
    LabelScan {
        graph: usize,
        slot: Slot,
        label: u64,
        index: u64,
    },
    /// The nodes an index seek finds.
    IndexSeek {
        graph: usize,
        slot: Slot,
        index: u64,
        label: u64,
        props: Vec<u64>,
        seek: Seek,
    },
    /// An expansion: from a bound node over its adjacency in one
    /// direction, of one type or any; `into` checks a bound target.
    Expand {
        graph: usize,
        from: Slot,
        edge: Slot,
        to: Slot,
        dir: u8,
        ty: Option<u64>,
        into: bool,
        bound_edge: bool,
    },
    /// Keeps the rows for which the predicate is TRUE.
    Filter(Expr),
    /// For each row of the first child, the rows of the second, run with
    /// that row as its argument. `optional`: a row with no match gives one
    /// row with `nulls` set to NULL.
    Apply { optional: bool, nulls: Vec<Slot> },
    /// Computes slots.
    Project(Vec<(Slot, Expr)>),
    /// Groups by keys and aggregates (blocking).
    Aggregate {
        keys: Vec<(Slot, Expr)>,
        aggs: Vec<(Slot, AggKind, bool, Expr)>,
        post: Vec<(Slot, Expr)>,
        having: Option<Expr>,
    },
    /// Removes duplicate rows over slots (blocking: it holds the seen
    /// keys).
    Distinct(Vec<Slot>),
    /// Sorts (blocking).
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
    /// Catalog-modifying statements.
    Catalog(Vec<CatalogOp>),
    /// A named procedure call (rmp #50), once per input row.
    ProcedureCall {
        routine: crate::proc::RoutineRef,
        args: Vec<Expr>,
        yields: Vec<(usize, Slot)>,
        graph: usize,
    },
    /// The result: the columns (none for a statement without result).
    Produce(Vec<(Box<str>, Slot)>),
}

/// A plan node: an operator, its inputs and its estimated rows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    /// The operator.
    pub(crate) op: Op,
    /// Its inputs.
    pub(crate) children: Vec<Node>,
    /// Estimated rows it produces.
    pub(crate) est: f64,
}

/// A planned statement: its root, the subplans of its `EXISTS`
/// subqueries, and the frame width.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    /// The root.
    pub(crate) root: Node,
    /// The `EXISTS` subplans, by index.
    pub(crate) subs: Vec<Node>,
    /// The number of slots.
    pub(crate) width: usize,
    /// The names of the slots, for plan descriptions.
    pub(crate) names: Vec<Box<str>>,
}

/// Selectivity of an equality (TS-20).
pub(crate) const EQ_SELECTIVITY: f64 = 0.01;
/// Selectivity of any other predicate (TS-20).
pub(crate) const OTHER_SELECTIVITY: f64 = 0.1;

fn leaf(op: Op, est: f64) -> Node {
    Node {
        op,
        children: Vec::new(),
        est,
    }
}

fn over(op: Op, input: Node, est: f64) -> Node {
    Node {
        op,
        children: vec![input],
        est,
    }
}

/// The statistics and indexes the planner reads, per graph.
pub(crate) struct Estimates {
    graphs: Vec<u64>,
    stats: std::collections::BTreeMap<StatKey, u64>,
    indexes: Vec<Vec<SchemaDef>>,
}

impl Estimates {
    fn nodes(&self, g: usize, label: u64) -> f64 {
        *self
            .stats
            .get(&StatKey::Nodes {
                graph: self.graphs[g],
                label,
            })
            .unwrap_or(&0) as f64
    }

    /// The estimate of every node of graph `g`: the sum of its label
    /// counts (the statistics hold no total; an unlabelled node is not
    /// counted, so this is a lower bound for multi-labelled-free graphs).
    fn all_nodes(&self, g: usize) -> f64 {
        let id = self.graphs[g];
        self.stats
            .iter()
            .filter(|(k, _)| matches!(k, StatKey::Nodes { graph, .. } if *graph == id))
            .map(|(_, c)| *c as f64)
            .sum::<f64>()
            .max(1.0)
    }

    /// Edges of type `ty` (any type for `None`) leaving (`dir` OUT) or
    /// entering (IN) nodes with `label` (any label for `None`).
    fn edges(&self, g: usize, label: Option<u64>, ty: Option<u64>, dir: u8) -> f64 {
        let id = self.graphs[g];
        self.stats
            .iter()
            .filter(|(k, _)| match k {
                StatKey::Edges {
                    graph,
                    label: l,
                    ty: t,
                    dir: d,
                } => {
                    *graph == id
                        && *d == dir
                        && label.is_none_or(|x| x == *l)
                        && ty.is_none_or(|x| x == *t)
                }
                _ => false,
            })
            .map(|(_, c)| *c as f64)
            .sum()
    }
}

/// The planner of one statement.
struct Planner<'q> {
    q: &'q Query,
    est: Estimates,
    params: &'q [Value],
}

/// The labels a pattern requires of `slot`.
fn required_labels(preds: &[Expr], slot: Slot) -> Vec<Option<u64>> {
    preds
        .iter()
        .filter_map(|p| match p {
            Expr::Labels(x, t) if **x == Expr::Slot(slot) => Some(t.required()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// `(key, op, other side)` of a comparison of `slot`'s property, with the
/// operator as seen from the property side.
fn prop_cmp(p: &Expr, slot: Slot) -> Option<(u64, CmpOp, &Expr)> {
    let Expr::Cmp(op, a, b) = p else { return None };
    let is_prop = |e: &Expr| match e {
        Expr::Prop(x, k) if **x == Expr::Slot(slot) => k.token,
        _ => None,
    };
    if let Some(k) = is_prop(a) {
        return Some((k, *op, b));
    }
    let flip = match op {
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Ge => CmpOp::Le,
        o => *o,
    };
    is_prop(b).map(|k| (k, flip, &**a))
}

impl Planner<'_> {
    /// Whether `e` can be evaluated before the pattern binds anything:
    /// it reads only `bound` slots and no subquery.
    fn independent(e: &Expr, bound: &BTreeSet<Slot>) -> bool {
        !e.has_subquery() && e.slots().iter().all(|s| bound.contains(s))
    }

    /// The value of a seek argument at plan time, when it is a literal or
    /// a parameter.
    fn known(&self, e: &Expr) -> Option<Value> {
        match e {
            Expr::Lit(v) => Some(v.clone()),
            Expr::Param(i) => self.params.get(*i).cloned(),
            _ => None,
        }
    }

    /// The cheapest access to the unbound node `slot` of graph `g` (module
    /// documentation), with its estimate.
    fn access(&self, g: usize, slot: Slot, preds: &[Expr], bound: &BTreeSet<Slot>) -> Node {
        let labels: Vec<u64> = required_labels(preds, slot).into_iter().flatten().collect();
        let absent_label = required_labels(preds, slot).iter().any(Option::is_none);
        let label_est = labels
            .iter()
            .map(|&l| self.est.nodes(g, l))
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))));
        let base = if absent_label {
            0.0
        } else {
            label_est.unwrap_or_else(|| self.est.all_nodes(g))
        };
        let cmps: Vec<(u64, CmpOp, &Expr)> = preds
            .iter()
            .filter_map(|p| prop_cmp(p, slot))
            .filter(|(_, _, e)| Self::independent(e, bound))
            .collect();
        let online = self.est.indexes[g]
            .iter()
            .filter(|d| d.state == State::Online && d.entity == Entity::Node);
        let mut best: Option<Node> = None;
        let consider = |n: Node, best: &mut Option<Node>| {
            if best.as_ref().is_none_or(|b| n.est < b.est) {
                *best = Some(n);
            }
        };
        for d in online {
            if d.kind != SchemaKind::Lookup && !labels.contains(&d.token) {
                continue;
            }
            match d.kind {
                SchemaKind::Range => {
                    let mut eqs = Vec::new();
                    for &p in &d.props {
                        match cmps.iter().find(|(k, op, _)| *k == p && *op == CmpOp::Eq) {
                            Some((_, _, e)) => eqs.push((*e).clone()),
                            None => break,
                        }
                    }
                    if !eqs.is_empty() {
                        let est = base * EQ_SELECTIVITY.powi(eqs.len() as i32);
                        consider(
                            leaf(
                                Op::IndexSeek {
                                    graph: g,
                                    slot,
                                    index: d.id,
                                    label: d.token,
                                    props: d.props.clone(),
                                    seek: Seek::Eq(eqs),
                                },
                                est,
                            ),
                            &mut best,
                        );
                    } else if d.props.len() == 1 {
                        let p = d.props[0];
                        let lo = cmps
                            .iter()
                            .find(|(k, op, _)| *k == p && matches!(op, CmpOp::Gt | CmpOp::Ge))
                            .map(|(_, _, e)| (*e).clone());
                        let hi = cmps
                            .iter()
                            .find(|(k, op, _)| *k == p && matches!(op, CmpOp::Lt | CmpOp::Le))
                            .map(|(_, _, e)| (*e).clone());
                        if lo.is_some() || hi.is_some() {
                            let n = usize::from(lo.is_some()) + usize::from(hi.is_some());
                            consider(
                                leaf(
                                    Op::IndexSeek {
                                        graph: g,
                                        slot,
                                        index: d.id,
                                        label: d.token,
                                        props: d.props.clone(),
                                        seek: Seek::Range(lo, hi),
                                    },
                                    base * OTHER_SELECTIVITY.powi(n as i32),
                                ),
                                &mut best,
                            );
                        }
                    }
                }
                SchemaKind::Point => {
                    let p = d.props[0];
                    if let Some((_, _, e)) = cmps.iter().find(|(k, op, e)| {
                        *k == p
                            && *op == CmpOp::Eq
                            && matches!(self.known(e), Some(Value::Point(_)))
                    }) {
                        consider(
                            leaf(
                                Op::IndexSeek {
                                    graph: g,
                                    slot,
                                    index: d.id,
                                    label: d.token,
                                    props: d.props.clone(),
                                    seek: Seek::Point((*e).clone()),
                                },
                                base * EQ_SELECTIVITY,
                            ),
                            &mut best,
                        );
                    }
                }
                SchemaKind::Lookup => {
                    for &l in &labels {
                        consider(
                            leaf(
                                Op::LabelScan {
                                    graph: g,
                                    slot,
                                    label: l,
                                    index: d.id,
                                },
                                self.est.nodes(g, l),
                            ),
                            &mut best,
                        );
                    }
                }
                _ => {}
            }
        }
        best.unwrap_or_else(|| leaf(Op::AllNodesScan { graph: g, slot }, self.est.all_nodes(g)))
    }

    /// `n(v)` of an unbound node: its base estimate times the
    /// selectivities of the predicates that read only it.
    fn node_estimate(&self, g: usize, slot: Slot, preds: &[Expr]) -> f64 {
        let labels = required_labels(preds, slot);
        if labels.iter().any(Option::is_none) {
            return 0.0;
        }
        let mut n = labels
            .iter()
            .flatten()
            .map(|&l| self.est.nodes(g, l))
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
            .unwrap_or_else(|| self.est.all_nodes(g));
        for p in preds {
            if matches!(p, Expr::Labels(..)) {
                continue;
            }
            if p.slots() == [slot] {
                n *= match p {
                    Expr::Cmp(CmpOp::Eq, ..) => EQ_SELECTIVITY,
                    _ => OTHER_SELECTIVITY,
                };
            }
        }
        n
    }

    /// `p(e)`: edges of its type between labelled endpoints over the
    /// product of the endpoint counts.
    fn edge_probability(&self, g: usize, e: &PEdge, pattern: &Pattern) -> f64 {
        let ty = match e.ty {
            Some(None) => return 0.0,
            Some(Some(t)) => Some(t),
            None => None,
        };
        let label_of = |n: usize| {
            required_labels(&pattern.predicates, pattern.nodes[n].slot)
                .into_iter()
                .flatten()
                .next()
        };
        let (ls, ld) = (label_of(e.src), label_of(e.dst));
        let count = match (ls, ld) {
            (Some(l), _) => self.est.edges(g, Some(l), ty, OUT),
            (None, Some(l)) => self.est.edges(g, Some(l), ty, IN),
            (None, None) => self.est.edges(g, None, ty, OUT),
        };
        let n = |l: Option<u64>| l.map_or_else(|| self.est.all_nodes(g), |l| self.est.nodes(g, l));
        let denom = (n(ls) * n(ld)).max(1.0);
        count / denom
    }

    /// Plans a `MATCH` over `input`; `bound` holds the slots the input
    /// binds.
    fn pattern(&self, input: Node, g: usize, p: &Pattern, bound: &mut BTreeSet<Slot>) -> Node {
        let mut pending: Vec<Expr> = p.predicates.clone();
        // Filters that read only the input go first.
        let mut cur = input;
        let take_ready = |pending: &mut Vec<Expr>, bound: &BTreeSet<Slot>| {
            let (ready, rest): (Vec<Expr>, Vec<Expr>) = std::mem::take(pending)
                .into_iter()
                .partition(|e| !e.has_subquery() && e.slots().iter().all(|s| bound.contains(s)));
            *pending = rest;
            ready
        };
        let filter = |cur: Node, ready: Vec<Expr>| -> Node {
            ready.into_iter().fold(cur, |c, e| {
                let sel = match e {
                    Expr::Cmp(CmpOp::Eq, ..) => EQ_SELECTIVITY,
                    _ => OTHER_SELECTIVITY,
                };
                let est = c.est * sel;
                over(Op::Filter(e), c, est)
            })
        };
        let ready = take_ready(&mut pending, bound);
        cur = filter(cur, ready);
        // Components of the pattern.
        let mut comp = vec![usize::MAX; p.nodes.len()];
        let mut ncomp = 0;
        for start in 0..p.nodes.len() {
            if comp[start] != usize::MAX {
                continue;
            }
            let mut stack = vec![start];
            comp[start] = ncomp;
            while let Some(v) = stack.pop() {
                for e in &p.edges {
                    for (a, b) in [(e.src, e.dst), (e.dst, e.src)] {
                        if a == v && comp[b] == usize::MAX {
                            comp[b] = ncomp;
                            stack.push(b);
                        }
                    }
                }
            }
            ncomp += 1;
        }
        for c in 0..ncomp {
            let members: Vec<usize> = (0..p.nodes.len()).filter(|&v| comp[v] == c).collect();
            let edges: Vec<usize> = (0..p.edges.len())
                .filter(|&e| comp[p.edges[e].src] == c)
                .collect();
            let local = |v: usize| members.iter().position(|&m| m == v).expect("member");
            let jg = join::Graph {
                nodes: members
                    .iter()
                    .map(|&v| {
                        if bound.contains(&p.nodes[v].slot) {
                            1.0
                        } else {
                            self.node_estimate(g, p.nodes[v].slot, &p.predicates)
                        }
                    })
                    .collect(),
                bound: members
                    .iter()
                    .map(|&v| bound.contains(&p.nodes[v].slot))
                    .collect(),
                edges: edges
                    .iter()
                    .map(|&e| {
                        let pe = &p.edges[e];
                        (
                            local(pe.src),
                            local(pe.dst),
                            self.edge_probability(g, pe, p),
                        )
                    })
                    .collect(),
            };
            let order = join::order(&jg);
            let start = members[order.start];
            let start_slot = p.nodes[start].slot;
            if !bound.contains(&start_slot) {
                let acc = self.access(g, start_slot, &p.predicates, bound);
                cur = if matches!(cur.op, Op::Argument) {
                    acc
                } else {
                    let est = cur.est * acc.est;
                    Node {
                        op: Op::Apply {
                            optional: false,
                            nulls: Vec::new(),
                        },
                        children: vec![cur, acc],
                        est,
                    }
                };
                bound.insert(start_slot);
            }
            let ready = take_ready(&mut pending, bound);
            cur = filter(cur, ready);
            for step in &order.steps {
                let pe = &p.edges[edges[step.edge]];
                let from = members[step.from];
                let (dir, to) = if from == pe.src {
                    (OUT, pe.dst)
                } else {
                    (IN, pe.src)
                };
                let (from_slot, to_slot) = (p.nodes[from].slot, p.nodes[to].slot);
                let est = cur.est
                    * jg.edges[step.edge].2
                    * if step.into { 1.0 } else { jg.nodes[local(to)] };
                cur = over(
                    Op::Expand {
                        graph: g,
                        from: from_slot,
                        edge: pe.slot,
                        to: to_slot,
                        dir,
                        ty: pe.ty.flatten(),
                        into: step.into || bound.contains(&to_slot),
                        bound_edge: bound.contains(&pe.slot),
                    },
                    cur,
                    est,
                );
                if let Some(None) = pe.ty {
                    // An edge type no edge has: nothing matches.
                    cur = over(Op::Filter(Expr::Lit(Value::Bool(false))), cur, 0.0);
                }
                bound.insert(pe.slot);
                bound.insert(to_slot);
                let ready = take_ready(&mut pending, bound);
                cur = filter(cur, ready);
            }
        }
        for n in &p.nodes {
            bound.insert(n.slot);
        }
        // Predicates with subqueries, last.
        let rest = std::mem::take(&mut pending);
        filter(cur, rest)
    }

    /// Plans the parts `parts` over `input`.
    fn parts(&self, mut cur: Node, parts: &[Part], bound: &mut BTreeSet<Slot>) -> Node {
        for part in parts {
            cur = match part {
                Part::Match {
                    optional: false,
                    graph,
                    pattern,
                } => self.pattern(cur, *graph, pattern, bound),
                Part::Match {
                    optional: true,
                    graph,
                    pattern,
                } => {
                    let mut inner = bound.clone();
                    let right = self.pattern(leaf(Op::Argument, 1.0), *graph, pattern, &mut inner);
                    let nulls: Vec<Slot> = inner.difference(bound).copied().collect();
                    *bound = inner;
                    let est = cur.est * right.est.max(1.0);
                    Node {
                        op: Op::Apply {
                            optional: true,
                            nulls,
                        },
                        children: vec![cur, right],
                        est,
                    }
                }
                Part::Filter(e) => {
                    let est = cur.est * OTHER_SELECTIVITY;
                    over(Op::Filter(e.clone()), cur, est)
                }
                Part::Project(pr) => self.project(cur, pr, bound),
                Part::Sort(keys) => {
                    let est = cur.est;
                    over(Op::Sort(keys.clone()), cur, est)
                }
                Part::Insert { graph, pattern } => {
                    for n in &pattern.nodes {
                        bound.insert(n.slot);
                    }
                    for e in &pattern.edges {
                        bound.insert(e.slot);
                    }
                    let est = cur.est;
                    over(
                        Op::Insert {
                            graph: *graph,
                            pattern: pattern.clone(),
                        },
                        cur,
                        est,
                    )
                }
                Part::Set { graph, items } => {
                    let est = cur.est;
                    over(
                        Op::Set {
                            graph: *graph,
                            items: items.clone(),
                        },
                        cur,
                        est,
                    )
                }
                Part::Remove { graph, items } => {
                    let est = cur.est;
                    over(
                        Op::Remove {
                            graph: *graph,
                            items: items.clone(),
                        },
                        cur,
                        est,
                    )
                }
                Part::Call {
                    routine,
                    args,
                    yields,
                    graph,
                } => {
                    bound.extend(yields.iter().map(|(_, s)| *s));
                    let est = cur.est;
                    over(
                        Op::ProcedureCall {
                            routine: routine.clone(),
                            args: args.clone(),
                            yields: yields.clone(),
                            graph: *graph,
                        },
                        cur,
                        est,
                    )
                }
            };
        }
        cur
    }

    fn project(&self, cur: Node, pr: &Project, bound: &mut BTreeSet<Slot>) -> Node {
        bound.extend(pr.output.iter().copied());
        let mut node = if pr.aggregates.is_empty() && pr.having.is_none() {
            let est = cur.est;
            over(Op::Project(pr.items.clone()), cur, est)
        } else {
            let est = if pr.items.is_empty() { 1.0 } else { cur.est };
            over(
                Op::Aggregate {
                    keys: pr.items.clone(),
                    aggs: pr.aggregates.clone(),
                    post: pr.post.clone(),
                    having: pr.having.clone(),
                },
                cur,
                est,
            )
        };
        if pr.distinct {
            let est = node.est;
            node = over(Op::Distinct(pr.output.clone()), node, est);
        }
        node
    }
}

/// Plans the bound query `q` in `tx`; `params` are the statement's
/// parameter values (read only to choose a point index).
///
/// # Errors
///
/// Storage errors of the index definitions read.
pub(crate) fn plan(q: &Query, tx: &mut Transaction, params: &[Value]) -> Result<Plan, Error> {
    let mut indexes = Vec::new();
    for &g in &q.graphs {
        indexes.push(tx.schema_defs(g)?);
    }
    let stats = tx.engine().core().catalog.stats().counts().clone();
    let p = Planner {
        q,
        est: Estimates {
            graphs: q.graphs.clone(),
            stats,
            indexes,
        },
        params,
    };
    let mut bound = BTreeSet::new();
    let body = p.parts(leaf(Op::Argument, 1.0), &q.parts, &mut bound);
    let est = body.est;
    let root = over(Op::Produce(q.columns.clone()), body, est);
    let mut subs = Vec::new();
    for sub in &p.q.subqueries {
        // The outer slots are bound in the subquery's argument row.
        let mut b: BTreeSet<Slot> = (0..q.slots.len())
            .filter(|s| !sub_declares(sub, *s))
            .collect();
        subs.push(p.parts(leaf(Op::Argument, 1.0), sub, &mut b));
    }
    Ok(Plan {
        root,
        subs,
        width: q.slots.len(),
        names: q.slots.iter().map(|s| s.name.clone()).collect(),
    })
}

/// Whether a subquery declares `slot` (a node or edge of one of its
/// patterns that is not bound outside).
fn sub_declares(parts: &[Part], slot: Slot) -> bool {
    parts.iter().any(|p| match p {
        Part::Match { pattern, .. } => {
            pattern.nodes.iter().any(|n| n.slot == slot && !n.bound)
                || pattern.edges.iter().any(|e| e.slot == slot && !e.bound)
        }
        Part::Project(pr) => pr.output.contains(&slot) || pr.aggregates.iter().any(|a| a.0 == slot),
        _ => false,
    })
}

/// A plan for catalog-modifying statements.
pub(crate) fn plan_catalog(ops: &[CatalogOp]) -> Plan {
    let root = over(
        Op::Produce(Vec::new()),
        leaf(Op::Catalog(ops.to_vec()), 0.0),
        0.0,
    );
    Plan {
        root,
        subs: Vec::new(),
        width: 0,
        names: Vec::new(),
    }
}

/// One node of a plan description (TS-20 "EXPLAIN and PROFILE",
/// "Result").
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    /// The operator name (Neo4j's where Neo4j has the operator).
    pub name: String,
    /// Details: index, direction, types, labels, expressions.
    pub details: String,
    /// The variables it binds or reads.
    pub identifiers: Vec<String>,
    /// Estimated rows.
    pub estimated_rows: f64,
    /// PROFILE's measured counters; `None` for EXPLAIN.
    pub profile: Option<super::exec::profile::ProfileStats>,
    /// Its inputs.
    pub children: Vec<Description>,
}

impl Plan {
    /// The plan description of EXPLAIN.
    pub(crate) fn describe(&self) -> Description {
        describe(&self.root, &self.names)
    }
}

fn name_of(names: &[Box<str>], s: Slot) -> String {
    names
        .get(s)
        .map_or_else(|| format!("${s}"), |n| n.to_string())
}

/// The operator name of `op` (TS-20 "Operator names").
pub(crate) fn op_name(n: &Node) -> &'static str {
    match &n.op {
        Op::Argument => "Argument",
        Op::AllNodesScan { .. } => "AllNodesScan",
        Op::LabelScan { .. } => "NodeByLabelScan",
        Op::IndexSeek {
            seek: Seek::Range(..),
            ..
        } => "NodeIndexSeekByRange",
        Op::IndexSeek { .. } => "NodeIndexSeek",
        Op::Expand { into: false, .. } => "Expand(All)",
        Op::Expand { into: true, .. } => "Expand(Into)",
        Op::Filter(_) => "Filter",
        Op::Apply {
            optional: false, ..
        } => "Apply",
        Op::Apply { optional: true, .. } => "Optional",
        Op::Project(_) => "Projection",
        Op::Aggregate { .. } => "EagerAggregation",
        Op::Distinct(_) => "Distinct",
        Op::Sort(_) => "Sort",
        Op::Insert { .. } => "Create",
        Op::Set { items, .. } => {
            if items.iter().any(|i| matches!(i, SetItem::All(..))) {
                "SetProperties"
            } else {
                "SetProperty"
            }
        }
        Op::Remove { .. } => "RemoveProperty",
        Op::Catalog(_) => "CatalogStatements",
        Op::ProcedureCall { .. } => "ProcedureCall",
        Op::Produce(cols) if cols.is_empty() => "EmptyResult",
        Op::Produce(_) => "ProduceResults",
    }
}

fn describe(n: &Node, names: &[Box<str>]) -> Description {
    let slot = |s: &Slot| name_of(names, *s);
    let (details, identifiers): (String, Vec<String>) = match &n.op {
        Op::AllNodesScan { slot: s, .. } => (slot(s), vec![slot(s)]),
        Op::LabelScan { slot: s, label, .. } => {
            (format!("{}:label#{label}", slot(s)), vec![slot(s)])
        }
        Op::IndexSeek {
            slot: s,
            index,
            label,
            props,
            seek,
            ..
        } => (
            format!(
                "index #{index} on :label#{label}({}) {}",
                props
                    .iter()
                    .map(|p| format!("key#{p}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                match seek {
                    Seek::Eq(_) => "equality",
                    Seek::Range(..) => "range",
                    Seek::Point(_) => "point equality",
                }
            ),
            vec![slot(s)],
        ),
        Op::Expand {
            from,
            edge,
            to,
            dir,
            ty,
            ..
        } => (
            format!(
                "({})-[{}{}]-{}({})",
                slot(from),
                slot(edge),
                ty.map_or(String::new(), |t| format!(":type#{t}")),
                if *dir == OUT { ">" } else { "<" },
                slot(to)
            ),
            vec![slot(from), slot(edge), slot(to)],
        ),
        Op::Filter(e) => (format!("{e:?}"), e.slots().iter().map(slot).collect()),
        Op::Project(items) => (String::new(), items.iter().map(|(s, _)| slot(s)).collect()),
        Op::Aggregate { keys, aggs, .. } => (
            format!(
                "keys: {}; aggregates: {}",
                keys.iter()
                    .map(|(s, _)| slot(s))
                    .collect::<Vec<_>>()
                    .join(", "),
                aggs.iter()
                    .map(|(_, k, d, _)| format!("{k:?}{}", if *d { " DISTINCT" } else { "" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            keys.iter().map(|(s, _)| slot(s)).collect(),
        ),
        Op::Distinct(ss) => (String::new(), ss.iter().map(slot).collect()),
        Op::Sort(keys) => (
            keys.iter()
                .map(|(e, d)| format!("{e:?} {}", if *d { "DESC" } else { "ASC" }))
                .collect::<Vec<_>>()
                .join(", "),
            Vec::new(),
        ),
        Op::Insert { pattern, .. } => (
            format!(
                "{} nodes, {} edges",
                pattern.nodes.iter().filter(|n| !n.existing).count(),
                pattern.edges.len()
            ),
            pattern.nodes.iter().map(|n| slot(&n.slot)).collect(),
        ),
        Op::Produce(cols) => (
            String::new(),
            cols.iter().map(|(n, _)| n.to_string()).collect(),
        ),
        Op::Catalog(ops) => (format!("{} statements", ops.len()), Vec::new()),
        Op::ProcedureCall {
            routine, yields, ..
        } => (
            format!("{routine:?}"),
            yields.iter().map(|(_, s)| slot(s)).collect(),
        ),
        _ => (String::new(), Vec::new()),
    };
    Description {
        name: op_name(n).to_string(),
        details,
        identifiers,
        estimated_rows: n.est,
        profile: None,
        children: n.children.iter().map(|c| describe(c, names)).collect(),
    }
}
