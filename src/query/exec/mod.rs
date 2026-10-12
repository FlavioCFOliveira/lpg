//! The executor (rmp #45; [TS-20] Execution, EXPLAIN and PROFILE): pull
//! based, one batch of at most [`BATCH`] rows at a time, on the calling
//! thread.
//!
//! * **Operator interface** ([`Operator`]): `next_batch` returns the next
//!   batch of rows or `None` when exhausted; `reset` makes it start again
//!   (the right side of an `Apply` runs once per left row). Rows are the
//!   statement's frame of slots ([`super::bound::Query::slots`]); a batch
//!   holds its rows in one vector, row after row, so an expression reads a
//!   row as a slice (TS-20 rows of `Value`; DECISION on rmp #45). The pull
//!   trait follows Grafeo @ `3c23dbe`
//!   `crates/grafeo-core/src/execution/operators/mod.rs:302-322`
//!   (`next`, `reset`, `name`) and DataFusion @ `54a4bbf` (a stream the
//!   consumer pulls, `physical-plan/src/execution_plan.rs:696-700`);
//!   adapted: the transaction and the statement state are passed to each
//!   call ([`Cx`]), so an operator tree owns nothing borrowed.
//! * **Cursor** ([`Cursor`]): owns the operator tree and the statement
//!   state and is `Send`, so it can move into `spawn_blocking` with an
//!   owned transaction (TS-02 "Owned transactions", #39 point 4; the
//!   transaction becomes owned in rmp #47). [`Cursor::set_check`] installs
//!   the status check the cursor calls before each batch and that loops
//!   call every [`CHECK_EVERY`] iterations (#39 point 9; wired by #49).
//! * **Memory** (#39 point 8): sort, aggregation and distinct reserve
//!   through the buffer pool and the memory accountant in chunks of the
//!   larger of the shortfall and [`CHUNK`] bytes ([`Held`]); a refusal
//!   fails the statement with 51N72 and the transaction is aborted.
//! * **Writes** ([`write`]) are blocking: an `INSERT`, `SET` or `REMOVE`
//!   reads all its input before it writes, so a statement sees the
//!   binding table of the one before it and its own writes do not feed its
//!   input (as Neo4j's `Eager`).
//! * **Failure.** Any error of a statement aborts its transaction: its
//!   writes are discarded and later statements fail with 25N05, as a Bolt
//!   failure ends an explicit Neo4j transaction.
//! * **PROFILE** ([`profile`]): each operator wrapped in a decorator that
//!   measures it; a normal plan holds no decorator.
//!
//! The internal entry points [`run`], [`profile`] and [`explain`] take
//! the text, the transaction, the session and the parameters; the public
//! API of rmp #47 calls them.
//!
//! [TS-20]: ../../../../docs/technical-spec/query-engine.md

mod ops;
pub(crate) mod profile;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod write;

use std::collections::BTreeMap;

use super::binder::{SessionCtx, bind};
use super::bound::{Body, SessionCmd, StatementKind};
use super::expr::{Env, Slot};
use super::graph_type::GraphType;
use super::plan::{self, Description, Plan};
use crate::catalog::{Definition, Kind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::ast::EndTransaction;
use crate::storage::error::Error as StorageError;
use crate::storage::graph::Graph;
use crate::storage::memory::Reservation;
use crate::tx::Transaction;
use crate::types::Value;

/// The most rows of a batch (TS-20).
pub(crate) const BATCH: usize = 1024;
/// The reservation chunk of blocking operators (#39 point 8: 128 KiB).
pub(crate) const CHUNK: u64 = 128 * 1024;
/// Iterations between two status checks of a long loop (#39 point 9).
pub(crate) const CHECK_EVERY: usize = 1024;

/// A batch: up to [`BATCH`] rows of `width` slots, row after row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Batch {
    width: usize,
    rows: usize,
    data: Vec<Value>,
}

impl Batch {
    /// An empty batch of rows of `width` slots.
    pub(crate) fn new(width: usize) -> Batch {
        Batch {
            width,
            rows: 0,
            data: Vec::new(),
        }
    }

    /// The number of rows.
    pub(crate) fn len(&self) -> usize {
        self.rows
    }

    /// Whether the batch has no row.
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the batch is full.
    pub(crate) fn is_full(&self) -> bool {
        self.len() >= BATCH
    }

    /// Row `i`.
    pub(crate) fn row(&self, i: usize) -> &[Value] {
        &self.data[i * self.width..(i + 1) * self.width]
    }

    /// Appends a row of `width` values.
    pub(crate) fn push(&mut self, row: &[Value]) {
        debug_assert_eq!(row.len(), self.width);
        self.data.extend_from_slice(row);
        self.rows += 1;
    }

    /// Appends a row, moving its values.
    pub(crate) fn push_owned(&mut self, row: Vec<Value>) {
        debug_assert_eq!(row.len(), self.width);
        self.data.extend(row);
        self.rows += 1;
    }

    /// The rows, each as a vector.
    pub(crate) fn rows(&self) -> impl Iterator<Item = &[Value]> {
        (0..self.rows).map(|i| self.row(i))
    }

    /// The width.
    pub(crate) fn width(&self) -> usize {
        self.width
    }
}

/// The bytes a value holds: its 24 bytes and its heap payload.
pub(crate) fn value_bytes(v: &Value) -> usize {
    24 + match v {
        Value::String(s) => s.len() + 16,
        Value::Bytes(b) => b.len() + 16,
        Value::List(l) => 16 + l.iter().map(value_bytes).sum::<usize>(),
        Value::Record(r) => {
            16 + r
                .iter()
                .map(|(n, v)| n.len() + 16 + value_bytes(v))
                .sum::<usize>()
        }
        Value::Path(p) => 16 + 8 * p.len(),
        Value::Point(_) => 32,
        Value::ZonedDateTime(_) => 96,
        _ => 0,
    }
}

/// The bytes a row holds.
pub(crate) fn row_bytes(row: &[Value]) -> u64 {
    (24 + row.iter().map(value_bytes).sum::<usize>()) as u64
}

/// A status check (rmp #49 wires cancellation, termination and the time
/// limit through it), given the statement's loop iterations so far.
/// Shared, so a nested statement of a procedure checks the same status.
pub(crate) type Check = crate::sync::Arc<dyn Fn(usize) -> Result<(), Error> + Send + Sync>;

/// The counters of a statement's writes, for `ResultStream::summary()`
/// (rmp #47).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counters {
    pub(crate) nodes_created: u64,
    pub(crate) edges_created: u64,
    pub(crate) properties_set: u64,
    pub(crate) catalog_updates: u64,
}

/// The state of one statement, owned by its cursor.
pub(crate) struct State {
    /// The frame width.
    pub(crate) width: usize,
    /// The parameter values, by index.
    pub(crate) params: Vec<Value>,
    /// The graphs, by index, and their object identifiers.
    pub(crate) graphs: Vec<Graph>,
    /// The closed graph type of each graph, `None` for an open one.
    pub(crate) graph_types: Vec<Option<GraphType>>,
    /// The argument rows of the `Apply` operators and subqueries running.
    pub(crate) args: Vec<Vec<Value>>,
    /// The `EXISTS` subplans, taken out while one runs.
    pub(crate) subs: Vec<Option<Box<dyn Operator>>>,
    /// The statement clock.
    pub(crate) now: jiff::Timestamp,
    /// The session: its principal, home and current schema and graph, and
    /// its procedures (nested statements of a procedure run with it).
    pub(crate) session: SessionCtx,
    /// The status check.
    pub(crate) check: Option<Check>,
    /// Loop iterations counted by `tick`.
    pub(crate) iterations: usize,
    /// Rows produced by the scan and seek operators (FR-API-010 checks).
    pub(crate) scan_rows: u64,
    /// Calls of the closed-graph-type check (FR-GQL-004 checks).
    pub(crate) type_checks: u64,
    /// The write counters.
    pub(crate) counters: Counters,
    /// The most bytes the transaction held when an operator reserved
    /// (the request's `peak_memory_bytes` event field, rmp #51).
    pub(crate) peak_memory: u64,
    /// PROFILE's statistics, by operator, when profiling.
    pub(crate) profile: Option<profile::Profiler>,
}

impl State {
    /// Counts one iteration of a loop and runs the status check every
    /// [`CHECK_EVERY`] iterations.
    ///
    /// # Errors
    ///
    /// The status check's error.
    pub(crate) fn tick(&mut self) -> Result<(), Error> {
        self.iterations += 1;
        if self.iterations.is_multiple_of(CHECK_EVERY)
            && let Some(c) = &self.check
        {
            c(self.iterations)?;
        }
        Ok(())
    }

    /// The argument row: the innermost `Apply`'s, or a row of NULLs.
    pub(crate) fn argument(&self) -> Vec<Value> {
        self.args
            .last()
            .cloned()
            .unwrap_or_else(|| vec![Value::Null; self.width])
    }
}

/// What an operator runs with: the transaction and the statement's state.
pub(crate) struct Cx<'a> {
    pub(crate) tx: &'a mut Transaction,
    pub(crate) st: &'a mut State,
}

/// A pull-based operator (module documentation).
pub(crate) trait Operator: Send {
    /// The next batch, `None` when exhausted. A returned batch is never
    /// empty.
    ///
    /// # Errors
    ///
    /// The statement's errors.
    fn next_batch(&mut self, cx: &mut Cx<'_>) -> Result<Option<Batch>, Error>;

    /// Starts again from the beginning, with the current argument row.
    fn reset(&mut self);

    /// The most bytes the operator has reserved at once (blocking
    /// operators; PROFILE).
    fn held(&self) -> u64 {
        0
    }
}

/// The memory a blocking operator holds, reserved in chunks (module
/// documentation).
#[derive(Debug, Default)]
pub(crate) struct Held {
    res: Option<Reservation>,
    /// The transaction's operator counter its reservation is added to
    /// (the memory limit, rmp #49).
    counted: Option<crate::sync::Arc<crate::sync::atomic::AtomicU64>>,
    used: u64,
    /// The most bytes reserved at once (PROFILE's peak memory).
    peak: u64,
}

impl Held {
    /// Records `bytes` more held, reserving when the reservation no longer
    /// covers them.
    ///
    /// # Errors
    ///
    /// 51N72 when the memory cap refuses the reservation.
    pub(crate) fn grow(&mut self, cx: &mut Cx<'_>, bytes: u64) -> Result<(), Error> {
        self.used += bytes;
        let have = self.res.as_ref().map_or(0, Reservation::bytes);
        if self.used > have {
            let need = (self.used - have).max(CHUNK);
            cx.tx.check_memory(need)?;
            let r = cx.tx.engine().core().pool.reserve(need)?;
            let c = self
                .counted
                .get_or_insert_with(|| cx.tx.operator_bytes.clone());
            c.fetch_add(need, crate::sync::atomic::Ordering::Relaxed);
            cx.st.peak_memory = cx.st.peak_memory.max(cx.tx.memory_held());
            match &mut self.res {
                Some(h) => h.absorb(r),
                None => self.res = Some(r),
            }
            self.peak = self
                .peak
                .max(self.res.as_ref().map_or(0, Reservation::bytes));
        }
        Ok(())
    }

    /// The most bytes reserved at once.
    pub(crate) fn peak(&self) -> u64 {
        self.peak
    }

    /// Keeps `bytes` held and releases the rest of the reservation, so an
    /// algorithm run hands its rows' share over without reserving it twice
    /// (rmp #64).
    pub(crate) fn shrink_to(&mut self, bytes: u64) {
        if let Some(r) = &mut self.res {
            let extra = r.bytes().saturating_sub(bytes);
            r.shrink(extra);
            if let Some(c) = &self.counted {
                c.fetch_sub(extra, crate::sync::atomic::Ordering::Relaxed);
            }
        }
        self.used = self.used.min(bytes);
    }

    /// Releases everything.
    pub(crate) fn release(&mut self) {
        if let (Some(r), Some(c)) = (self.res.take(), &self.counted) {
            c.fetch_sub(r.bytes(), crate::sync::atomic::Ordering::Relaxed);
        }
        self.used = 0;
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release();
    }
}

impl Env for Cx<'_> {
    fn param(&self, i: usize) -> Value {
        self.st.params.get(i).cloned().unwrap_or(Value::Null)
    }

    fn property(&mut self, element: &Value, key: u64) -> Result<Value, Error> {
        for g in self.st.graphs.clone() {
            match *element {
                Value::Node(id) => {
                    if let Some((_, props)) = self.tx.node(&g, id)? {
                        return Ok(props
                            .into_iter()
                            .find(|(k, _)| *k == key)
                            .map_or(Value::Null, |(_, v)| v));
                    }
                }
                Value::Edge(id) => {
                    if let Some(e) = self.tx.edge(&g, id)? {
                        return Ok(e
                            .props
                            .into_iter()
                            .find(|(k, _)| *k == key)
                            .map_or(Value::Null, |(_, v)| v));
                    }
                }
                _ => {}
            }
        }
        // An element no graph of the statement holds (deleted by another
        // statement of the transaction): its properties read as NULL.
        Ok(Value::Null)
    }

    fn tokens(&mut self, element: &Value) -> Result<Vec<u64>, Error> {
        for g in self.st.graphs.clone() {
            match *element {
                Value::Node(id) => {
                    if let Some((labels, _)) = self.tx.node(&g, id)? {
                        return Ok(labels);
                    }
                }
                Value::Edge(id) => {
                    if let Some(e) = self.tx.edge(&g, id)? {
                        return Ok(vec![e.ty]);
                    }
                }
                _ => {}
            }
        }
        Ok(Vec::new())
    }

    fn exists(&mut self, sub: usize, row: &[Value]) -> Result<bool, Error> {
        let mut op = self.st.subs[sub]
            .take()
            .ok_or_else(|| super::expr::internal("a subquery that runs inside itself"))?;
        self.st.args.push(row.to_vec());
        op.reset();
        let r = loop {
            match op.next_batch(self) {
                Ok(Some(b)) if !b.is_empty() => break Ok(true),
                Ok(Some(_)) => {}
                Ok(None) => break Ok(false),
                Err(e) => break Err(e),
            }
        };
        self.st.args.pop();
        self.st.subs[sub] = Some(op);
        r
    }

    fn now(&self) -> jiff::Timestamp {
        self.st.now
    }

    fn principal(&self) -> &str {
        &self.st.session.principal
    }

    fn token(&self, kind: crate::catalog::TokenKind, name: &str) -> Option<u64> {
        self.tx.engine().token_id(kind, name)
    }
}

/// The parameters of a statement, by name.
pub(crate) type Params = BTreeMap<String, Value>;

/// The transaction and session commands of a program, for the session of
/// rmp #47, which acts on them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Control {
    pub(crate) session: Vec<SessionCmd>,
    pub(crate) start: bool,
    pub(crate) end: Option<EndTransaction>,
    pub(crate) close: bool,
}

/// A running statement: owned and `Send` (module documentation).
pub(crate) struct Cursor {
    root: Box<dyn Operator>,
    st: State,
    plan: Plan,
    graph_ids: Vec<u64>,
    columns: Vec<(Box<str>, Slot)>,
    kind: StatementKind,
    catalog: bool,
    control: Control,
    done: bool,
}

impl std::fmt::Debug for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cursor")
            .field("columns", &self.columns)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

/// Aborts `tx` because the statement failed with `e`, and returns `e`.
fn fail(tx: &mut Transaction, e: Error) -> Error {
    if tx.check_active().is_ok() {
        tx.abort_with(StorageError::Aborted);
    }
    e
}

impl Cursor {
    /// The result column names.
    pub(crate) fn columns(&self) -> Vec<&str> {
        self.columns.iter().map(|(n, _)| &**n).collect()
    }

    /// Read, write, both, or catalog (`None` for a catalog statement).
    pub(crate) fn kind(&self) -> StatementKind {
        self.kind
    }

    /// The program's session and transaction commands.
    pub(crate) fn control(&self) -> &Control {
        &self.control
    }

    /// Installs the status check (rmp #49).
    pub(crate) fn set_check(&mut self, check: Check) {
        self.st.check = Some(check);
    }

    /// Every authorisation request the statement's plan can perform
    /// (TS-23), asked before its first batch.
    ///
    /// # Errors
    ///
    /// Storage errors of the catalog reads.
    pub(crate) fn requests(
        &self,
        tx: &mut Transaction,
    ) -> Result<Vec<crate::auth::Request>, Error> {
        crate::auth::requests(&self.plan, &self.graph_ids, tx, &self.st.session.principal)
    }

    /// Whether the statement is catalog-modifying.
    pub(crate) fn is_catalog(&self) -> bool {
        self.catalog
    }

    /// The most bytes the transaction held when one of the statement's
    /// operators reserved memory.
    pub(crate) fn peak_memory(&self) -> u64 {
        self.st.peak_memory
    }

    /// Loop iterations counted so far (cancellation checks, rmp #49).
    #[cfg(test)]
    pub(crate) fn iterations(&self) -> usize {
        self.st.iterations
    }

    /// The statement's write counters.
    pub(crate) fn counters(&self) -> Counters {
        self.st.counters
    }

    /// Rows produced by its scans and seeks so far.
    #[cfg(test)]
    pub(crate) fn scan_rows(&self) -> u64 {
        self.st.scan_rows
    }

    /// Calls of the closed-graph-type check so far.
    #[cfg(test)]
    pub(crate) fn type_checks(&self) -> u64 {
        self.st.type_checks
    }

    /// The next batch of result rows, each with one value per column; `None`
    /// when the statement is done. Any failure aborts `tx`.
    ///
    /// # Errors
    ///
    /// The statement's errors; 25N05 once `tx` is aborted.
    pub(crate) fn next_batch(&mut self, tx: &mut Transaction) -> Result<Option<Batch>, Error> {
        if self.done {
            return Ok(None);
        }
        if let Some(c) = &self.st.check
            && let Err(e) = c(self.st.iterations)
        {
            self.done = true;
            return Err(fail(tx, e));
        }
        let mut cx = Cx {
            tx,
            st: &mut self.st,
        };
        match self.root.next_batch(&mut cx) {
            Ok(Some(b)) => {
                let mut out = Batch::new(self.columns.len());
                for row in b.rows() {
                    let r: Vec<Value> = self.columns.iter().map(|(_, s)| row[*s].clone()).collect();
                    out.push_owned(r);
                }
                Ok(Some(out))
            }
            Ok(None) => {
                self.done = true;
                Ok(None)
            }
            Err(e) => {
                self.done = true;
                Err(fail(cx.tx, e))
            }
        }
    }

    /// The profiled plan, once the statement is done (PROFILE only).
    pub(crate) fn profile(&self) -> Option<Description> {
        let p = self.st.profile.as_ref()?;
        let mut d = self.plan.describe();
        p.annotate(&mut d);
        Some(d)
    }

    /// The highest number of operators running at once (NFR-PERF-007);
    /// PROFILE only.
    #[cfg(test)]
    pub(crate) fn max_running(&self) -> Option<usize> {
        self.st.profile.as_ref().map(profile::Profiler::max_running)
    }
}

/// A statement parsed, bound and planned.
pub(crate) struct Prepared {
    plan: Plan,
    params: Vec<Value>,
    graph_ids: Vec<u64>,
    columns: Vec<(Box<str>, Slot)>,
    kind: StatementKind,
    /// Whether the statement is catalog-modifying.
    catalog: bool,
    control: Control,
}

fn prepare(
    text: &str,
    tx: &mut Transaction,
    session: &SessionCtx,
    params: &Params,
) -> Result<Prepared, Error> {
    let ast = super::parser::parse_program(text)?;
    let program = bind(text, &ast, tx, session)?;
    let control = Control {
        session: program.session.clone(),
        start: program.start,
        end: program.end,
        close: program.close,
    };
    match program.body {
        None => Ok(Prepared {
            plan: plan::plan_catalog(&[]),
            params: Vec::new(),
            graph_ids: Vec::new(),
            columns: Vec::new(),
            kind: StatementKind::Read,
            catalog: false,
            control,
        }),
        Some(Body::Catalog(ops)) => Ok(Prepared {
            plan: plan::plan_catalog(&ops),
            params: Vec::new(),
            graph_ids: Vec::new(),
            columns: Vec::new(),
            kind: StatementKind::Write,
            catalog: true,
            control,
        }),
        Some(Body::Query(q)) => {
            let mut values = Vec::with_capacity(q.params.len());
            for name in &q.params {
                match params.get(&**name) {
                    Some(v) => values.push(v.clone()),
                    None => {
                        return Err(Error::new(
                            codes::C42002,
                            format!("invalid reference: the parameter ${name} is not given"),
                        ));
                    }
                }
            }
            let plan = plan::plan(&q, tx, &values)?;
            Ok(Prepared {
                plan,
                params: values,
                graph_ids: q.graphs.clone(),
                columns: q.columns.clone(),
                kind: q.kind,
                catalog: false,
                control,
            })
        }
    }
}

/// The closed graph type of graph `id`, `None` for an open graph.
fn graph_type_of(tx: &mut Transaction, id: u64) -> Result<Option<GraphType>, Error> {
    let Some(o) = tx.object(id)? else {
        return Ok(None);
    };
    let Definition::Graph(d) = o.definition else {
        return Ok(None);
    };
    if d.graph_type == 0 {
        return Ok(None);
    }
    match tx.object(d.graph_type)? {
        Some(t) if t.kind == Kind::GraphType => match t.definition {
            Definition::GraphType(bytes) => GraphType::decode(&bytes)
                .map(Some)
                .ok_or_else(|| super::expr::internal("a malformed graph type definition")),
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn start(
    text: &str,
    tx: &mut Transaction,
    session: &SessionCtx,
    params: &Params,
    profiling: bool,
) -> Result<Cursor, Error> {
    let p = prepare(text, tx, session, params)?;
    start_prepared(p, tx, session, profiling)
}

/// The cursor of the prepared statement `p`.
fn start_prepared(
    p: Prepared,
    tx: &mut Transaction,
    session: &SessionCtx,
    profiling: bool,
) -> Result<Cursor, Error> {
    let mut graphs = Vec::new();
    let mut graph_types = Vec::new();
    for &g in &p.graph_ids {
        graphs.push(tx.graph(g)?);
        graph_types.push(graph_type_of(tx, g)?);
    }
    let mut profiler = profiling.then(profile::Profiler::default);
    let root = ops::build(&p.plan.root, &mut profiler);
    let subs = p
        .plan
        .subs
        .iter()
        .map(|s| Some(ops::build(s, &mut profiler)))
        .collect();
    let st = State {
        width: p.plan.width,
        params: p.params,
        graphs,
        graph_types,
        args: Vec::new(),
        subs,
        now: crate::types::temporal::now(),
        session: session.clone(),
        check: None,
        iterations: 0,
        scan_rows: 0,
        type_checks: 0,
        counters: Counters::default(),
        peak_memory: 0,
        profile: profiler,
    };
    Ok(Cursor {
        root,
        st,
        plan: p.plan,
        graph_ids: p.graph_ids,
        columns: p.columns,
        kind: p.kind,
        catalog: p.catalog,
        control: p.control,
        done: false,
    })
}

/// Parses, binds and plans `text` in `tx` for `session` with `params`, and
/// returns its cursor (the internal entry point of rmp #47). Any failure
/// aborts `tx`.
///
/// # Errors
///
/// The errors of the parser (42001), the binder, the planner and the
/// first operators.
pub(crate) fn run(
    text: &str,
    tx: &mut Transaction,
    session: &SessionCtx,
    params: &Params,
) -> Result<Cursor, Error> {
    start(text, tx, session, params, false).map_err(|e| fail(tx, e))
}

/// [`run`] with every operator measured (PROFILE).
///
/// # Errors
///
/// As [`run`].
pub(crate) fn profile(
    text: &str,
    tx: &mut Transaction,
    session: &SessionCtx,
    params: &Params,
) -> Result<Cursor, Error> {
    start(text, tx, session, params, true).map_err(|e| fail(tx, e))
}

/// The plan of `text` with estimated rows, without executing it
/// (EXPLAIN, FR-OBS-005). A failure aborts `tx`.
///
/// # Errors
///
/// As [`run`], without the operators'.
pub(crate) fn explain(
    text: &str,
    tx: &mut Transaction,
    session: &SessionCtx,
    params: &Params,
) -> Result<Description, Error> {
    match prepare(text, tx, session, params) {
        Ok(p) => Ok(p.plan.describe()),
        Err(e) => Err(fail(tx, e)),
    }
}

/// Runs `text`, a statement of a procedure's context, in the transaction
/// `tx` of the calling statement whose state is `outer`, and returns its
/// rows (TS-24). With `read_only`, a statement that would modify data
/// fails with 42N02 before it runs. The statement is authorised as the
/// caller's statements are, through the session's host, and checks the
/// caller's status.
///
/// # Errors
///
/// 42N02; 25G01 and 2D000 for a transaction command, 42000 for a session
/// command; the statement's errors.
pub(crate) fn run_nested(
    text: &str,
    tx: &mut Transaction,
    outer: &State,
    params: &Params,
    read_only: bool,
) -> Result<Vec<Vec<Value>>, Error> {
    let session = &outer.session;
    let p = prepare(text, tx, session, params)?;
    if p.control.start {
        return Err(Error::new(
            codes::C25G01,
            "invalid transaction state: active GQL-transaction",
        ));
    }
    if p.control.end.is_some() {
        return Err(Error::new(
            codes::C2D000,
            "invalid transaction termination: a procedure cannot end its caller's transaction",
        ));
    }
    if p.control.close || !p.control.session.is_empty() {
        return Err(Error::new(
            codes::C42000,
            "syntax error or access rule violation: a session command in a procedure",
        ));
    }
    if read_only && (p.catalog || p.kind != StatementKind::Read) {
        return Err(Error::new(
            codes::C42N02,
            "writing in read access mode: a read-only procedure ran a statement that modifies data",
        ));
    }
    if let Some(h) = &session.procs.host {
        let reqs = crate::auth::requests(&p.plan, &p.graph_ids, tx, &session.principal)?;
        h.authorise(&session.principal, &reqs)?;
    }
    let mut c = start_prepared(p, tx, session, false)?;
    if let Some(ch) = &outer.check {
        c.set_check(ch.clone());
    }
    let mut out = Vec::new();
    while let Some(b) = c.next_batch(tx)? {
        out.extend(b.rows().map(<[Value]>::to_vec));
    }
    Ok(out)
}

/// Every result row of a cursor (tests and the reference checks).
///
/// # Errors
///
/// As [`Cursor::next_batch`].
#[cfg(test)]
pub(crate) fn collect(c: &mut Cursor, tx: &mut Transaction) -> Result<Vec<Vec<Value>>, Error> {
    let mut out = Vec::new();
    while let Some(b) = c.next_batch(tx)? {
        out.extend(b.rows().map(<[Value]>::to_vec));
    }
    Ok(out)
}
