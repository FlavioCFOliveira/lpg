// Portions ported from Grafeo; licensed under the Apache License, Version
// 2.0 (see `LICENSE-APACHE` at the repository root).
//
// NOTICE of the source project:
//   Grafeo
//   Copyright 2025-2026 S.T. Grond
//   This product includes software developed at
//   https://github.com/GrafeoDB/grafeo
//   Licensed under the Apache License, Version 2.0.

//! The read operators of the executor (rmp #45) and the construction of
//! an operator tree from a plan.
//!
//! The algorithms of expansion, aggregation, distinct and sort are ported
//! from Grafeo @ `3c23dbe` (<https://github.com/GrafeoDB/grafeo>,
//! Apache-2.0, see `LICENSE-APACHE`),
//! `crates/grafeo-core/src/execution/operators/{expand,aggregate,distinct,sort}.rs`:
//! an expansion keeps its input chunk, the row it is at and that row's
//! neighbours, and fills its output chunk across calls; aggregation and
//! sort drain their input, then emit; distinct keeps the keys it has
//! seen. Adapted: rows of lpg [`Value`]s, adjacency from lpg's trees, the
//! memory of the blocking ones reserved in chunks ([`Held`]), and a status
//! check every 1 024 iterations. DataFusion @ `54a4bbf`
//! `physical-plan/src/sorts/sort.rs` (reservation per batch) and Kuzu @
//! `89f0263` `src/processor/operator/` were studied.

use crate::obs::names::indexes;
use std::collections::{HashMap, VecDeque};

use super::profile::{Profiled, Profiler};
use super::write;
use super::{BATCH, Batch, CHECK_EVERY, Cx, Held, Operator, row_bytes};
use crate::error::Error;
use crate::query::aggregate::{Accumulator, AggKind};
use crate::query::expr::{Expr, Slot, condition, eval, is_true, sort_cmp};
use crate::query::plan::{Node, Op, Seek};
use crate::storage::graph::{Adj, element_of};
use crate::types::{Value, key};

/// Rows produced and not yet returned; a call returns at most [`BATCH`].
#[derive(Debug, Default)]
pub(super) struct Queue {
    rows: VecDeque<Vec<Value>>,
}

impl Queue {
    pub(super) fn push(&mut self, row: Vec<Value>) {
        self.rows.push_back(row);
    }

    pub(super) fn full(&self) -> bool {
        self.rows.len() >= BATCH
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// A batch of up to [`BATCH`] queued rows, `None` if none is queued.
    pub(super) fn take(&mut self, width: usize) -> Option<Batch> {
        if self.rows.is_empty() {
            return None;
        }
        let mut b = Batch::new(width);
        while let Some(r) = self.rows.pop_front() {
            b.push_owned(r);
            if b.is_full() {
                break;
            }
        }
        Some(b)
    }

    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Builds the operator tree of `n`; with a profiler, each operator is
/// wrapped in its decorator, numbered in pre-order.
pub(super) fn build(n: &Node, prof: &mut Option<Profiler>) -> Box<dyn Operator> {
    let id = prof.as_mut().map(Profiler::register);
    let mut kids: Vec<Box<dyn Operator>> = n.children.iter().map(|c| build(c, prof)).collect();
    let mut child = || kids.remove(0);
    let op: Box<dyn Operator> = match &n.op {
        Op::Argument => Box::new(Argument { done: false }),
        Op::AllNodesScan { graph, slot } => Box::new(Scan::new(*graph, *slot, ScanKind::All)),
        Op::LabelScan {
            graph,
            slot,
            label,
            index,
        } => Box::new(
            Scan::new(
                *graph,
                *slot,
                ScanKind::Index {
                    index: *index,
                    lo: label.to_be_bytes().to_vec(),
                    hi: label.checked_add(1).map(|l| l.to_be_bytes().to_vec()),
                },
            )
            .counting(indexes::LOOKUP_QUERIED),
        ),
        Op::IndexSeek {
            graph,
            slot,
            index,
            seek,
            props,
            ..
        } => Box::new(
            Scan::new(
                *graph,
                *slot,
                ScanKind::Seek {
                    index: *index,
                    seek: seek.clone(),
                },
            )
            .counting(match seek {
                Seek::Point(_) => indexes::POINT_QUERIED,
                Seek::Range(..) => indexes::RANGE_QUERIED,
                Seek::Eq(_) if props.len() > 1 => indexes::COMPOSITE_QUERIED,
                Seek::Eq(_) => indexes::SINGLE_PROPERTY_QUERIED,
            }),
        ),
        Op::Expand {
            graph,
            from,
            edge,
            to,
            dir,
            ty,
            into,
            bound_edge,
        } => Box::new(Expand {
            child: child(),
            graph: *graph,
            from: *from,
            edge: *edge,
            to: *to,
            dir: *dir,
            ty: *ty,
            into: *into,
            bound_edge: *bound_edge,
            input: None,
            row: 0,
            adj: Vec::new(),
            pos: 0,
            out: Queue::default(),
            done: false,
        }),
        Op::Filter(e) => Box::new(Filter {
            child: child(),
            pred: e.clone(),
        }),
        Op::Apply { optional, nulls } => {
            let left = child();
            let right = child();
            Box::new(Apply {
                left,
                right,
                optional: *optional,
                nulls: nulls.clone(),
                input: None,
                row: 0,
                active: false,
                matched: false,
                out: Queue::default(),
                done: false,
            })
        }
        Op::Project(items) => Box::new(Project {
            child: child(),
            items: items.clone(),
        }),
        Op::Aggregate {
            keys,
            aggs,
            post,
            having,
        } => Box::new(Aggregate {
            child: child(),
            keys: keys.clone(),
            aggs: aggs.clone(),
            post: post.clone(),
            having: having.clone(),
            out: Queue::default(),
            held: Held::default(),
            done: false,
        }),
        Op::Distinct(slots) => Box::new(Distinct {
            child: child(),
            slots: slots.clone(),
            seen: std::collections::HashSet::new(),
            held: Held::default(),
        }),
        Op::Sort(keys) => Box::new(Sort {
            child: child(),
            keys: keys.clone(),
            out: Queue::default(),
            held: Held::default(),
            done: false,
        }),
        Op::Insert { graph, pattern } => write::insert(child(), *graph, pattern.clone()),
        Op::Set { graph, items } => write::set(child(), *graph, items.clone()),
        Op::Remove { graph, items } => write::remove(child(), *graph, items.clone()),
        Op::Catalog(ops) => write::catalog(ops.clone()),
        Op::ProcedureCall {
            routine,
            args,
            yields,
            graph,
        } => Box::new(Call {
            child: child(),
            routine: routine.clone(),
            args: args.clone(),
            yields: yields.clone(),
            graph: *graph,
            input: None,
            row: 0,
            current: None,
            held: Held::default(),
        }),
        Op::Produce(_) => child(),
    };
    match id {
        Some(i) => Box::new(Profiled::new(op, i)),
        None => op,
    }
}

/// The argument row, once.
struct Argument {
    done: bool,
}

impl Operator for Argument {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        let mut b = Batch::new(cx.st.width);
        b.push_owned(cx.st.argument());
        Ok(Some(b))
    }

    fn reset(&mut self) {
        self.done = false;
    }
}

/// A key range: from, and up to (exclusive) or the end.
type KeyRange = (Vec<u8>, Option<Vec<u8>>);

/// What a scan reads.
enum ScanKind {
    /// The node tree.
    All,
    /// A range of an index: a label lookup index.
    Index {
        index: u64,
        lo: Vec<u8>,
        hi: Option<Vec<u8>>,
    },
    /// An index seek, whose range comes from the argument row.
    Seek { index: u64, seek: Seek },
}

/// A scan or seek: the argument row extended by each node found, read in
/// chunks of [`BATCH`] entries (FR-API-010).
struct Scan {
    graph: usize,
    slot: Slot,
    kind: ScanKind,
    /// The key to resume from; `None` before the first chunk.
    resume: Option<Vec<u8>>,
    /// The seek's range, computed at the first chunk.
    range: Option<KeyRange>,
    arg: Option<Vec<Value>>,
    done: bool,
    /// The `index.<kind>.queried` metric of an index read (TS-24), counted
    /// once per run of the scan.
    metric: Option<&'static str>,
    counted: bool,
}

impl Scan {
    fn new(graph: usize, slot: Slot, kind: ScanKind) -> Scan {
        Scan {
            graph,
            slot,
            kind,
            resume: None,
            range: None,
            arg: None,
            done: false,
            metric: None,
            counted: false,
        }
    }

    /// The scan, counting `metric` once per run.
    fn counting(mut self, metric: &'static str) -> Scan {
        self.metric = Some(metric);
        self
    }

    /// The key range of a range seek.
    fn seek_range(
        &self,
        seek: &Seek,
        cx: &mut Cx<'_>,
        arg: &[Value],
    ) -> Result<Option<KeyRange>, Error> {
        let succ = |p: &[u8]| crate::tx::conflict::Interval::prefix(p).hi;
        Ok(match seek {
            Seek::Eq(values) => {
                let mut vs = Vec::new();
                for e in values {
                    let v = eval(e, arg, cx)?;
                    if v.is_null() {
                        return Ok(None);
                    }
                    vs.push(v);
                }
                let lo = key::encode_all(&vs);
                let hi = succ(&lo);
                Some((lo, hi))
            }
            Seek::Range(lo, hi) => {
                let lo = lo.as_ref().map(|e| eval(e, arg, cx)).transpose()?;
                let hi = hi.as_ref().map(|e| eval(e, arg, cx)).transpose()?;
                if lo.as_ref().is_some_and(Value::is_null)
                    || hi.as_ref().is_some_and(Value::is_null)
                {
                    return Ok(None);
                }
                let enc = |v: &Value| {
                    let mut k = Vec::new();
                    key::encode(v, &mut k);
                    k
                };
                // The bounds' type group: a range never crosses it.
                let group = enc(lo.as_ref().or(hi.as_ref()).expect("a range has a bound"))[0];
                let l = lo.as_ref().map_or_else(|| vec![group], enc);
                let h = match &hi {
                    Some(v) => succ(&enc(v)),
                    None => group.checked_add(1).map(|g| vec![g]),
                };
                Some((l, h))
            }
            Seek::Point(_) => None,
        })
    }
}

impl Operator for Scan {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        if self.done {
            return Ok(None);
        }
        if !self.counted {
            self.counted = true;
            if let Some(m) = self.metric {
                cx.tx.engine().core().sink.counter(m, 1);
            }
        }
        let arg = match &self.arg {
            Some(a) => a.clone(),
            None => {
                let a = cx.st.argument();
                self.arg = Some(a.clone());
                a
            }
        };
        let g = cx.st.graphs[self.graph];
        let mut ids: Vec<u64> = Vec::new();
        match &self.kind {
            ScanKind::All => {
                let lo = self.resume.take().unwrap_or_default();
                let r = cx.tx.scan_chunk(g.nodes, &lo, None, BATCH, |k, _| {
                    if let Some(id) = element_of(k) {
                        ids.push(id);
                    }
                    true
                })?;
                self.done = r.is_none();
                self.resume = r;
            }
            ScanKind::Index { index, lo, hi } => {
                let from = self.resume.take().unwrap_or_else(|| lo.clone());
                let (v, r) = cx
                    .tx
                    .index_scan_chunk(&g, *index, &from, hi.as_deref(), BATCH)?;
                ids = v;
                self.done = r.is_none();
                self.resume = r;
            }
            ScanKind::Seek { index, seek } => {
                if let Seek::Point(e) = seek {
                    if let Value::Point(p) = eval(e, &arg, cx)? {
                        ids = cx.tx.index_within(&g, *index, &p, 0.0)?;
                    }
                    self.done = true;
                } else {
                    let range = match &self.range {
                        Some(r) => Some(r.clone()),
                        None => self.seek_range(seek, cx, &arg)?,
                    };
                    match range {
                        None => self.done = true,
                        Some((lo, hi)) => {
                            let from = self.resume.take().unwrap_or_else(|| lo.clone());
                            let (v, r) =
                                cx.tx
                                    .index_scan_chunk(&g, *index, &from, hi.as_deref(), BATCH)?;
                            ids = v;
                            self.done = r.is_none();
                            self.resume = r;
                            self.range = Some((lo, hi));
                        }
                    }
                }
            }
        }
        if ids.is_empty() {
            return Ok(None);
        }
        cx.st.scan_rows += ids.len() as u64;
        let mut b = Batch::new(cx.st.width);
        for id in ids {
            cx.st.tick()?;
            let mut row = arg.clone();
            row[self.slot] = Value::Node(id);
            b.push_owned(row);
        }
        // A chunk merged with the transaction's own writes can hold more
        // than a batch; the excess is rare and small, and is returned as
        // one larger batch rather than lost.
        Ok(Some(b))
    }

    fn reset(&mut self) {
        self.resume = None;
        self.range = None;
        self.arg = None;
        self.done = false;
        self.counted = false;
    }
}

/// `Expand(All)` and `Expand(Into)`.
struct Expand {
    child: Box<dyn Operator>,
    graph: usize,
    from: Slot,
    edge: Slot,
    to: Slot,
    dir: u8,
    ty: Option<u64>,
    into: bool,
    bound_edge: bool,
    input: Option<Batch>,
    row: usize,
    adj: Vec<Adj>,
    pos: usize,
    out: Queue,
    done: bool,
}

impl Operator for Expand {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        while !self.out.full() && !self.done {
            if self.pos < self.adj.len() {
                let a = self.adj[self.pos];
                self.pos += 1;
                cx.st.tick()?;
                let input = self.input.as_ref().expect("an input row");
                let row = input.row(self.row - 1);
                if self.into && row[self.to] != Value::Node(a.nbr) {
                    continue;
                }
                if self.bound_edge && row[self.edge] != Value::Edge(a.edge) {
                    continue;
                }
                let mut r = row.to_vec();
                r[self.edge] = Value::Edge(a.edge);
                r[self.to] = Value::Node(a.nbr);
                self.out.push(r);
                continue;
            }
            let need = self.input.as_ref().is_none_or(|b| self.row >= b.len());
            if need {
                match self.child.next_batch(cx)? {
                    Some(b) => {
                        self.input = Some(b);
                        self.row = 0;
                    }
                    None => {
                        self.done = true;
                        break;
                    }
                }
            }
            let input = self.input.as_ref().expect("an input batch");
            let from = input.row(self.row)[self.from].clone();
            self.row += 1;
            self.pos = 0;
            self.adj.clear();
            if let Value::Node(id) = from {
                let g = cx.st.graphs[self.graph];
                self.adj = cx.tx.expand(&g, id, self.dir, self.ty)?;
            }
        }
        Ok(self.out.take(cx.st.width))
    }

    fn reset(&mut self) {
        self.child.reset();
        self.input = None;
        self.row = 0;
        self.adj.clear();
        self.pos = 0;
        self.out.clear();
        self.done = false;
    }
}

/// Keeps the rows whose predicate is TRUE.
struct Filter {
    child: Box<dyn Operator>,
    pred: Expr,
}

impl Operator for Filter {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        loop {
            let Some(b) = self.child.next_batch(cx)? else {
                return Ok(None);
            };
            let mut out = Batch::new(b.width());
            for row in b.rows() {
                cx.st.tick()?;
                if condition(&eval(&self.pred, row, cx)?)? {
                    out.push(row);
                }
            }
            if !out.is_empty() {
                return Ok(Some(out));
            }
        }
    }

    fn reset(&mut self) {
        self.child.reset();
    }
}

/// `Apply` and `Optional`: the right side once per left row.
struct Apply {
    left: Box<dyn Operator>,
    right: Box<dyn Operator>,
    optional: bool,
    nulls: Vec<Slot>,
    input: Option<Batch>,
    row: usize,
    active: bool,
    matched: bool,
    out: Queue,
    done: bool,
}

impl Operator for Apply {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        while !self.out.full() && !self.done {
            if self.active {
                let r = self.right.next_batch(cx);
                match r {
                    Ok(Some(b)) => {
                        self.matched = true;
                        for row in b.rows() {
                            self.out.push(row.to_vec());
                        }
                    }
                    Ok(None) => {
                        let left = cx.st.args.pop().expect("the argument of this Apply");
                        self.active = false;
                        if self.optional && !self.matched {
                            let mut row = left;
                            for s in &self.nulls {
                                row[*s] = Value::Null;
                            }
                            self.out.push(row);
                        }
                    }
                    Err(e) => {
                        cx.st.args.pop();
                        self.active = false;
                        return Err(e);
                    }
                }
                continue;
            }
            let need = self.input.as_ref().is_none_or(|b| self.row >= b.len());
            if need {
                match self.left.next_batch(cx)? {
                    Some(b) => {
                        self.input = Some(b);
                        self.row = 0;
                    }
                    None => {
                        self.done = true;
                        break;
                    }
                }
            }
            let row = self
                .input
                .as_ref()
                .expect("a left batch")
                .row(self.row)
                .to_vec();
            self.row += 1;
            cx.st.tick()?;
            cx.st.args.push(row);
            self.right.reset();
            self.active = true;
            self.matched = false;
        }
        Ok(self.out.take(cx.st.width))
    }

    fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.input = None;
        self.row = 0;
        self.active = false;
        self.out.clear();
        self.done = false;
    }
}

/// `ProcedureCall` (rmp #50): for each input row, calls the routine once
/// with its arguments and returns the input row extended by each row the
/// routine returns. The rows of the batch being built are charged to the
/// memory cap and released when the batch is returned (FR-LIM-006).
struct Call {
    child: Box<dyn Operator>,
    routine: crate::proc::RoutineRef,
    args: Vec<Expr>,
    yields: Vec<(usize, Slot)>,
    graph: usize,
    input: Option<Batch>,
    row: usize,
    /// The input row of the running call, and the rows it still returns.
    current: Option<(Vec<Value>, crate::proc::Rows)>,
    held: Held,
}

impl Operator for Call {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        self.held.release();
        let mut out = Batch::new(cx.st.width);
        let columns = self.routine.get().sig.columns.len();
        loop {
            if let Some((input, rows)) = &mut self.current {
                while !out.is_full() {
                    cx.st.tick()?;
                    let Some(r) = rows.next() else {
                        self.current = None;
                        break;
                    };
                    let values = r?;
                    if values.len() != columns {
                        return Err(Error::new(
                            crate::gqlstatus::codes::C50N00,
                            "internal error: a procedure returned a row of the wrong width",
                        ));
                    }
                    let mut row = input.clone();
                    for (c, s) in &self.yields {
                        row[*s] = values[*c].clone();
                    }
                    self.held.grow(cx, row_bytes(&row))?;
                    out.push_owned(row);
                }
                if out.is_full() {
                    return Ok(Some(out));
                }
            }
            if self.input.as_ref().is_none_or(|b| self.row >= b.len()) {
                match self.child.next_batch(cx)? {
                    Some(b) => {
                        self.input = Some(b);
                        self.row = 0;
                    }
                    None => return Ok((!out.is_empty()).then_some(out)),
                }
            }
            let input = self
                .input
                .as_ref()
                .expect("an input batch")
                .row(self.row)
                .to_vec();
            self.row += 1;
            let mut args = Vec::with_capacity(self.args.len());
            for a in &self.args {
                args.push(eval(a, &input, cx)?);
            }
            let rows = self
                .routine
                .get()
                .invoke(cx, &mut self.held, self.graph, &args)?;
            self.current = Some((input, rows));
        }
    }

    fn reset(&mut self) {
        self.child.reset();
        self.input = None;
        self.row = 0;
        self.current = None;
        self.held.release();
    }

    fn held(&self) -> u64 {
        self.held.peak()
    }
}

/// Computes slots per row.
struct Project {
    child: Box<dyn Operator>,
    items: Vec<(Slot, Expr)>,
}

impl Operator for Project {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        let Some(b) = self.child.next_batch(cx)? else {
            return Ok(None);
        };
        let mut out = Batch::new(b.width());
        for row in b.rows() {
            cx.st.tick()?;
            let mut r = row.to_vec();
            for (s, e) in &self.items {
                r[*s] = eval(e, row, cx)?;
            }
            out.push_owned(r);
        }
        Ok(Some(out))
    }

    fn reset(&mut self) {
        self.child.reset();
    }
}

/// `EagerAggregation`: groups by the keys (implicit grouping), then
/// computes the aggregates, the items over them and `HAVING`.
struct Aggregate {
    child: Box<dyn Operator>,
    keys: Vec<(Slot, Expr)>,
    aggs: Vec<(Slot, AggKind, bool, Expr)>,
    post: Vec<(Slot, Expr)>,
    having: Option<Expr>,
    out: Queue,
    held: Held,
    done: bool,
}

impl Operator for Aggregate {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        if !self.done {
            self.done = true;
            // Groups in order of first appearance, by the order keys of
            // their key values (equal values have equal keys).
            let mut index: HashMap<Vec<u8>, usize> = HashMap::new();
            let mut groups: Vec<(Vec<Value>, Vec<Accumulator>)> = Vec::new();
            while let Some(b) = self.child.next_batch(cx)? {
                for row in b.rows() {
                    cx.st.tick()?;
                    let mut kv = Vec::with_capacity(self.keys.len());
                    for (_, e) in &self.keys {
                        kv.push(eval(e, row, cx)?);
                    }
                    let k = key::encode_all(&kv);
                    let at = match index.get(&k) {
                        Some(i) => *i,
                        None => {
                            let accs: Vec<Accumulator> = self
                                .aggs
                                .iter()
                                .map(|(_, kind, d, _)| Accumulator::new(*kind, *d))
                                .collect();
                            self.held.grow(
                                cx,
                                row_bytes(&kv) + k.len() as u64 + 64 * accs.len() as u64 + 48,
                            )?;
                            index.insert(k, groups.len());
                            groups.push((kv, accs));
                            groups.len() - 1
                        }
                    };
                    for (i, (_, _, _, e)) in self.aggs.iter().enumerate() {
                        let v = eval(e, row, cx)?;
                        let acc = &mut groups[at].1[i];
                        let before = acc.held();
                        acc.update(&v)?;
                        let after = acc.held();
                        if after > before {
                            self.held.grow(cx, after - before)?;
                        }
                    }
                }
            }
            if groups.is_empty() && self.keys.is_empty() {
                groups.push((
                    Vec::new(),
                    self.aggs
                        .iter()
                        .map(|(_, kind, d, _)| Accumulator::new(*kind, *d))
                        .collect(),
                ));
            }
            let width = cx.st.width;
            let base = cx.st.argument();
            for (kv, accs) in groups {
                cx.st.tick()?;
                let mut row = if base.len() == width {
                    base.clone()
                } else {
                    vec![Value::Null; width]
                };
                for ((s, _), v) in self.keys.iter().zip(kv) {
                    row[*s] = v;
                }
                for ((s, _, _, _), a) in self.aggs.iter().zip(accs) {
                    row[*s] = a.finish()?;
                }
                for (s, e) in &self.post {
                    let v = eval(e, &row, cx)?;
                    row[*s] = v;
                }
                if let Some(h) = &self.having
                    && !is_true(&eval(h, &row, cx)?)
                {
                    continue;
                }
                self.out.push(row);
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

/// Removes duplicate rows over its slots.
struct Distinct {
    child: Box<dyn Operator>,
    slots: Vec<Slot>,
    seen: std::collections::HashSet<Vec<u8>>,
    held: Held,
}

impl Operator for Distinct {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        loop {
            let Some(b) = self.child.next_batch(cx)? else {
                return Ok(None);
            };
            let mut out = Batch::new(b.width());
            for row in b.rows() {
                cx.st.tick()?;
                let vals: Vec<Value> = self.slots.iter().map(|s| row[*s].clone()).collect();
                let k = key::encode_all(&vals);
                if !self.seen.contains(&k) {
                    self.held.grow(cx, k.len() as u64 + 48)?;
                    self.seen.insert(k);
                    out.push(row);
                }
            }
            if !out.is_empty() {
                return Ok(Some(out));
            }
        }
    }

    fn reset(&mut self) {
        self.child.reset();
        self.seen.clear();
        self.held.release();
    }

    fn held(&self) -> u64 {
        self.held.peak()
    }
}

/// Sorts its input by its keys (stable), NULL last ascending (IS001).
struct Sort {
    child: Box<dyn Operator>,
    keys: Vec<(Expr, bool)>,
    out: Queue,
    held: Held,
    done: bool,
}

impl Operator for Sort {
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error> {
        if !self.done {
            self.done = true;
            let mut rows: Vec<(Vec<Value>, Vec<Value>)> = Vec::new();
            while let Some(b) = self.child.next_batch(cx)? {
                for row in b.rows() {
                    cx.st.tick()?;
                    let mut ks = Vec::with_capacity(self.keys.len());
                    for (e, _) in &self.keys {
                        ks.push(eval(e, row, cx)?);
                    }
                    self.held.grow(cx, row_bytes(row) + row_bytes(&ks))?;
                    rows.push((ks, row.to_vec()));
                }
            }
            let keys = &self.keys;
            let mut n = 0usize;
            let mut checked: Result<(), Error> = Ok(());
            rows.sort_by(|a, b| {
                n += 1;
                if n.is_multiple_of(CHECK_EVERY) && checked.is_ok() {
                    checked = cx
                        .st
                        .check
                        .as_ref()
                        .map_or(Ok(()), |c| c(cx.st.iterations + n));
                }
                for (i, (_, desc)) in keys.iter().enumerate() {
                    let o = sort_cmp(&a.0[i], &b.0[i], *desc);
                    if o.is_ne() {
                        return o;
                    }
                }
                std::cmp::Ordering::Equal
            });
            // The comparisons are the sort's loop iterations.
            cx.st.iterations += n;
            checked?;
            for (_, r) in rows {
                self.out.push(r);
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
