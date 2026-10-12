//! [`Session`] and [`Transaction`] (TS-02; FR-SES-001 to FR-SES-004,
//! FR-GQL-005, FR-API-004, FR-API-007 to FR-API-009, FR-CAT-005,
//! FR-OBS-003 to FR-OBS-006).

use std::time::Duration;

use super::database::{DbInner, TxShared, ended};
use super::status::Reason;
use super::stream::{End, ResultStream};
use super::{Parameters, blocking};
use crate::auth::{self, Action, Object, Request};
use crate::catalog::{Kind, ROOT, TokenKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::index::{SchemaKind, TypeSet};
use crate::proc::Procs;
use crate::query::ast::{EndTransaction, ProgramActivity};
use crate::query::binder::SessionCtx;
use crate::query::bound::SessionCmd;
use crate::query::exec::{self, Cursor};
use crate::query::plan::Description;
use crate::query::span::Spanned;
use crate::storage::error::Error as StorageError;
use crate::sync::atomic::{AtomicBool, Ordering};
use crate::sync::{Arc, Mutex, PoisonError};
use crate::tx::Isolation;

pub use crate::index::PropertyType;

/// The resource limits of a transaction (FR-API-007, FR-LIM-002). Each is
/// optional and none has a default: `Limits::default()` sets none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// Time, measured from the start of the transaction.
    pub time: Option<Duration>,
    /// Bytes the transaction may hold from the memory accountant: its
    /// operators' reservations, its writes and the old versions they create
    /// (FR-LIM-007).
    pub memory: Option<u64>,
    /// Rows each statement may return.
    pub rows: Option<u64>,
    /// Bytes each statement may return, as the sizes of its values in the
    /// record encoding (FR-LIM-002).
    pub bytes: Option<u64>,
}

/// The options of [`Session::begin`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TransactionOptions {
    /// Serializable (the default) or snapshot isolation (FR-TX-004).
    pub isolation: Isolation,
    /// The transaction's limits; `None` inherits the session's.
    pub limits: Option<Limits>,
    /// The `tx_metadata` shown by the transaction list and in its events.
    pub metadata: Parameters,
}

/// An active transaction as [`Session::list_transactions`] shows it
/// (FR-OBS-003).
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionInfo {
    /// Its identifier.
    pub id: u64,
    /// Its principal.
    pub principal: String,
    /// `Running`, or `Terminated with reason: …`.
    pub status: String,
    /// The time since it started.
    pub elapsed: Duration,
    /// The statement running or last run, if any.
    pub current_query: Option<String>,
    /// Its `tx_metadata`.
    pub metadata: Parameters,
}

/// The kind of an index (FR-IDX-001). A single-property, a range and a
/// composite index are one structure, a range index (TS-18): the first
/// two on one property, the third on several.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    /// A range index on exactly one property.
    SingleProperty,
    /// A range index on two or more properties.
    Composite,
    /// A label or edge-type lookup index: no label and no property.
    Lookup,
    /// A range (ordered) index on one or more properties.
    Range,
    /// A point index on one property (FR-TYPE-005).
    Point,
}

/// The elements an index or constraint covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entity {
    /// Nodes, by label.
    Node,
    /// Edges, by edge type.
    Relationship,
}

/// One type of a property type constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSpec {
    /// The property type `T`.
    Of(PropertyType),
    /// `LIST<T NOT NULL>`.
    ListOf(PropertyType),
}

/// The kind of a constraint (FR-IDX-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Uniqueness of the properties' values.
    Unique,
    /// Existence of the property.
    Exists,
    /// The property's type is one of a closed union of types.
    PropertyType(Vec<TypeSpec>),
    /// Node key: existence and uniqueness.
    NodeKey,
}

/// What a session's requests run against.
struct SessionState {
    current_schema: u64,
    current_graph: u64,
    /// The active transaction (FR-GQL-005): one at most, begun by `START
    /// TRANSACTION` or [`Session::begin`].
    active: Option<Arc<TxShared>>,
}

/// The shared state of a session.
pub(crate) struct SessionInner {
    pub(crate) db: Arc<DbInner>,
    pub(crate) principal: String,
    home_schema: u64,
    home_graph: u64,
    state: Mutex<SessionState>,
    busy: AtomicBool,
    closed: AtomicBool,
    limits: Mutex<Limits>,
    procs: Procs,
}

impl Drop for SessionInner {
    /// Dropping the session rolls back its active transaction (FR-GQL-005).
    fn drop(&mut self) {
        let t = self
            .state
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .active
            .take();
        if let Some(t) = t {
            t.status.stop(Reason::Terminated);
            self.db.abort_tx(&t);
        }
    }
}

/// The session's busy flag, held while a request runs (FR-SES-004).
pub(crate) struct Busy(Arc<SessionInner>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

impl SessionInner {
    /// Takes the busy flag.
    ///
    /// # Errors
    ///
    /// 08N09 after the database's `close`; 08000 on a closed session;
    /// 25N03 while another request of the session runs.
    pub(crate) fn acquire(self: &Arc<Self>) -> Result<Busy, Error> {
        self.db.check_open()?;
        if self.closed.load(Ordering::Acquire) {
            return Err(session_closed());
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::new(
                codes::C25N03,
                "concurrent access violation: another request of the session is running",
            ));
        }
        Ok(Busy(self.clone()))
    }

    fn state(&self) -> crate::sync::MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn ctx(&self, st: &SessionState) -> SessionCtx {
        SessionCtx {
            principal: self.principal.clone(),
            home_schema: self.home_schema,
            home_graph: self.home_graph,
            current_schema: st.current_schema,
            current_graph: st.current_graph,
            procs: self.procs.clone(),
        }
    }

    /// The active transaction.
    pub(crate) fn active(&self) -> Option<Arc<TxShared>> {
        self.state().active.clone()
    }

    /// Clears the active transaction if it is `t`.
    pub(crate) fn clear(&self, t: &Arc<TxShared>) {
        let mut st = self.state();
        if st.active.as_ref().is_some_and(|a| Arc::ptr_eq(a, t)) {
            st.active = None;
        }
    }

    /// Ends the session (`SESSION CLOSE`): rolls back its active
    /// transaction; later requests fail with 08000.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let t = self.state().active.take();
        if let Some(t) = t {
            self.db.abort_tx(&t);
        }
    }

    /// Runs `f` on the active transaction, or on a transaction of its own
    /// committed after it (management calls that join a transaction,
    /// FR-CAT-004).
    fn in_transaction<T>(
        &self,
        f: impl FnOnce(&mut crate::tx::Transaction) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if let Some(t) = self.active() {
            t.status.check()?;
            let mut g = t.slot.lock().unwrap_or_else(PoisonError::into_inner);
            let tx = g.as_mut().ok_or_else(ended)?;
            return f(tx).inspect_err(|_| fail_in(tx));
        }
        let limits = *self.limits.lock().unwrap_or_else(PoisonError::into_inner);
        let t = self.db.begin_tx(
            &self.principal,
            Isolation::Serializable,
            &limits,
            Parameters::new(),
        )?;
        let r = {
            let mut g = t.slot.lock().unwrap_or_else(PoisonError::into_inner);
            let tx = g.as_mut().ok_or_else(ended)?;
            f(tx)
        };
        match r {
            Ok(v) => {
                self.db.commit_tx(&t)?;
                Ok(v)
            }
            Err(e) => {
                self.db.abort_tx(&t);
                Err(e)
            }
        }
    }
}

/// The error of a request on a closed session (08000, FR-GQL-005).
fn session_closed() -> Error {
    Error::new(codes::C08000, "connection exception: the session is closed")
}

/// Aborts `tx` after a failed statement, leaving it in its slot so later
/// statements fail with 25N05 (TS-20: a failed statement aborts its
/// transaction).
pub(crate) fn fail_in(tx: &mut crate::tx::Transaction) {
    if tx.check_active().is_ok() {
        tx.abort_with(StorageError::Aborted);
    }
}

/// A statement opened in its transaction, authorised and not yet pulled.
pub(crate) struct Opened {
    pub(crate) tx: Arc<TxShared>,
    pub(crate) cursor: Cursor,
    pub(crate) end: End,
    pub(crate) close: bool,
}

/// What a request does to the session's transaction.
fn end_of(e: Option<EndTransaction>) -> End {
    match e {
        Some(EndTransaction::Commit) => End::Commit,
        Some(EndTransaction::Rollback) => End::Rollback,
        None => End::Keep,
    }
}

/// Parses, chooses the transaction for, binds, plans and authorises the
/// request `text` (TS-02, TS-23; FR-GQL-005). `via` is the API
/// transaction the request is made through, which must be the active one.
/// Runs on a blocking thread.
pub(crate) fn open_request(
    s: &Arc<SessionInner>,
    text: &str,
    params: &Parameters,
    profiling: bool,
    via: Option<&Arc<TxShared>>,
) -> Result<Opened, Error> {
    let ast = match crate::query::parser::parse_program(text) {
        Ok(a) => a,
        Err(e) => {
            if let Some(t) = s.active() {
                let mut g = t.slot.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(tx) = g.as_mut() {
                    fail_in(tx);
                }
            }
            return Err(e);
        }
    };
    let (start, end) = match &ast.activity {
        Some(Spanned(ProgramActivity::Transaction(t), _)) => {
            (t.start.is_some(), t.end.as_ref().map(|e| e.0))
        }
        _ => (false, None),
    };
    let mut st = s.state();
    if let Some(v) = via
        && !st.active.as_ref().is_some_and(|a| Arc::ptr_eq(a, v))
    {
        return Err(ended());
    }
    if start && st.active.is_some() {
        return Err(Error::new(
            codes::C25G01,
            "invalid transaction state: active GQL-transaction",
        ));
    }
    if end.is_some() && st.active.is_none() && !start {
        return Err(Error::new(
            codes::C2D000,
            "invalid transaction termination: no transaction is active",
        ));
    }
    let limits = *s.limits.lock().unwrap_or_else(PoisonError::into_inner);
    let (t, end) = match &st.active {
        Some(a) => (a.clone(), end_of(end)),
        None if start => {
            let t = s.db.begin_tx(
                &s.principal,
                Isolation::Serializable,
                &limits,
                Parameters::new(),
            )?;
            st.active = Some(t.clone());
            (t, end_of(end))
        }
        None => (
            s.db.begin_tx(
                &s.principal,
                Isolation::Serializable,
                &limits,
                Parameters::new(),
            )?,
            End::Auto,
        ),
    };
    let ctx = s.ctx(&st);
    drop(st);
    let r = open_in(s, &t, text, params, profiling, &ctx);
    match r {
        Ok(cursor) => {
            let close = cursor.control().close;
            Ok(Opened {
                tx: t,
                cursor,
                end,
                close,
            })
        }
        Err(e) => {
            if end != End::Keep {
                s.db.abort_tx(&t);
                s.clear(&t);
            }
            Err(e)
        }
    }
}

fn open_in(
    s: &Arc<SessionInner>,
    t: &Arc<TxShared>,
    text: &str,
    params: &Parameters,
    profiling: bool,
    ctx: &SessionCtx,
) -> Result<Cursor, Error> {
    let mut g = t.slot.lock().unwrap_or_else(PoisonError::into_inner);
    let tx = g.as_mut().ok_or_else(ended)?;
    if let Err(e) = t.status.check() {
        fail_in(tx);
        return Err(e);
    }
    // A statement of a transaction an earlier failure aborted fails with
    // 25N05, even one that reads nothing (TS-20).
    tx.check_active()?;
    *t.current.lock().unwrap_or_else(PoisonError::into_inner) = Some(text.to_string());
    let mut cursor = if profiling {
        exec::profile(text, tx, ctx, params)?
    } else {
        exec::run(text, tx, ctx, params)?
    };
    // TS-23: every pair the plan can perform, asked before any effect.
    let asked = cursor
        .requests(tx)
        .and_then(|r| s.db.authorise_blocking(&s.principal, &r));
    if let Err(e) = asked {
        fail_in(tx);
        return Err(e);
    }
    {
        let mut st = s.state();
        for cmd in &cursor.control().session {
            match *cmd {
                SessionCmd::Schema(id) => st.current_schema = id,
                SessionCmd::Graph(id) => st.current_graph = id,
            }
        }
    }
    let shared = t.clone();
    cursor.set_check(Arc::new(move |_| shared.status.check()));
    Ok(cursor)
}

/// A session (FR-SES-001): a principal, a home schema and graph, at most
/// one active transaction and the limits its transactions inherit. A
/// cheap clone; requests of clones share the busy flag, so a request that
/// arrives while another runs fails with 25N03 (FR-SES-004). Dropping the
/// last clone, or `SESSION CLOSE`, rolls back the active transaction
/// (FR-GQL-005).
#[derive(Clone)]
pub struct Session {
    pub(crate) inner: Arc<SessionInner>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("principal", &self.inner.principal)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub(crate) fn new(
        db: Arc<DbInner>,
        principal: String,
        home_schema: u64,
        home_graph: u64,
    ) -> Session {
        let procs = Procs {
            registry: db.registry.clone(),
            host: Some(db.clone() as Arc<dyn crate::proc::Host>),
        };
        Session {
            inner: Arc::new(SessionInner {
                db,
                principal,
                home_schema,
                home_graph,
                state: Mutex::new(SessionState {
                    current_schema: home_schema,
                    current_graph: home_graph,
                    active: None,
                }),
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                limits: Mutex::new(Limits::default()),
                procs,
            }),
        }
    }

    /// The session's principal.
    #[must_use]
    pub fn principal(&self) -> &str {
        &self.inner.principal
    }

    /// The limits the session's transactions inherit.
    #[must_use]
    pub fn limits(&self) -> Limits {
        *self
            .inner
            .limits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the limits the session's later transactions inherit, and that
    /// its auto-commit statements use (FR-API-007).
    pub fn set_limits(&self, limits: Limits) {
        *self
            .inner
            .limits
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = limits;
    }

    /// Runs the GQL request `gql` with `parameters` (FR-API-001): in the
    /// active transaction if there is one, else in a transaction of its
    /// own committed when its last row is pulled (auto-commit). Session
    /// and transaction commands in the text act on the session as the API
    /// calls do (FR-GQL-005). A request without result columns runs to its
    /// end before `execute` returns.
    ///
    /// # Errors
    ///
    /// The request's GQLSTATUS errors; 25N03 while another request of the
    /// session runs; 08000 on a closed session; 25G01, 2D000 for
    /// transaction commands that do not fit the session's state; 42NFF for
    /// an action the authoriser refuses.
    pub async fn execute(&self, gql: &str, parameters: Parameters) -> Result<ResultStream, Error> {
        ResultStream::start(&self.inner, gql, parameters, false, None).await
    }

    /// Runs the graph algorithm routine `name`, such as
    /// `gds.pageRank.stream`, with `parameters` keyed by the routine's
    /// parameter names (`configuration` for the GDS-named routines), as
    /// [`Session::execute`] runs the `CALL` of it (FR-ALG-002, TS-25 Rust
    /// API): the same registry entry, binder and run.
    ///
    /// # Errors
    ///
    /// 42N08 if `name` is no algorithm routine; 22N11 for a key that is
    /// no parameter of it; as [`Session::execute`].
    pub async fn run_algorithm(
        &self,
        name: &str,
        parameters: Parameters,
    ) -> Result<ResultStream, Error> {
        let (gql, params) = crate::algo::call(&self.inner.procs, name, parameters)?;
        self.execute(&gql, params).await
    }

    /// Runs `gql` as [`Session::execute`] with every operator measured
    /// (FR-OBS-006); the stream's summary holds the profiled plan once its
    /// last row is pulled.
    ///
    /// # Errors
    ///
    /// As [`Session::execute`].
    pub async fn profile(&self, gql: &str, parameters: Parameters) -> Result<ResultStream, Error> {
        ResultStream::start(&self.inner, gql, parameters, true, None).await
    }

    /// The plan of `gql` with estimated rows, without running it
    /// (FR-OBS-005).
    ///
    /// # Errors
    ///
    /// The errors of parsing, binding and planning.
    pub async fn explain(&self, gql: &str, parameters: Parameters) -> Result<Description, Error> {
        let busy = self.inner.acquire()?;
        let s = self.inner.clone();
        let text = gql.to_string();
        blocking(move || {
            let _busy = busy;
            let ctx = s.ctx(&s.state());
            match s.active() {
                Some(t) => {
                    let mut g = t.slot.lock().unwrap_or_else(PoisonError::into_inner);
                    let tx = g.as_mut().ok_or_else(ended)?;
                    exec::explain(&text, tx, &ctx, &parameters)
                }
                None => {
                    let mut tx = s.db.engine_handle()?.begin(Isolation::Serializable)?;
                    let r = exec::explain(&text, &mut tx, &ctx, &parameters);
                    tx.abort();
                    r
                }
            }
        })
        .await?
    }

    /// Begins the session's transaction (FR-GQL-005, FR-TX-004) with
    /// `options`.
    ///
    /// # Errors
    ///
    /// 25G01 if a transaction is active; 25N03, 08000 as
    /// [`Session::execute`].
    pub async fn begin(&self, options: TransactionOptions) -> Result<Transaction, Error> {
        let _busy = self.inner.acquire()?;
        let mut st = self.inner.state();
        if st.active.is_some() {
            return Err(Error::new(
                codes::C25G01,
                "invalid transaction state: active GQL-transaction",
            ));
        }
        let limits = options.limits.unwrap_or_else(|| self.limits());
        let t = self.inner.db.begin_tx(
            &self.inner.principal,
            options.isolation,
            &limits,
            options.metadata,
        )?;
        st.active = Some(t.clone());
        Ok(Transaction {
            session: self.clone(),
            shared: t,
        })
    }

    /// Cancels the session's running statement and its transaction
    /// (FR-API-008, FR-API-009): the statement fails with 25N14 at its next
    /// check and the transaction is aborted. Returns whether a transaction
    /// was stopped; a commit under way is not.
    pub fn cancel(&self) -> bool {
        self.inner
            .active()
            .is_some_and(|t| t.status.stop(Reason::Cancelled))
    }

    /// The active transactions the session's principal may see: its own,
    /// and others' with the authoriser's consent (FR-OBS-003).
    ///
    /// # Errors
    ///
    /// 08N09 after `close`.
    pub async fn list_transactions(&self) -> Result<Vec<TransactionInfo>, Error> {
        self.inner.db.check_open()?;
        let s = self.inner.clone();
        let visible = blocking(move || s.db.visible(&s.principal)).await??;
        Ok(visible
            .iter()
            .map(|t| TransactionInfo {
                id: t.id,
                principal: t.principal.clone(),
                status: t.status_text(),
                elapsed: t.started.elapsed(),
                current_query: t.current_query(),
                metadata: t.metadata.clone(),
            })
            .collect())
    }

    /// Terminates transaction `id` (FR-OBS-004): its running statement
    /// fails with 25N14 and it is aborted. A principal may terminate its
    /// own transactions; another's needs the authoriser's consent. Returns
    /// whether it was stopped (`false` if it is committing or already
    /// stopped).
    ///
    /// # Errors
    ///
    /// 22N11 if no active transaction has that identifier; 42NFF if the
    /// authoriser refuses; 08N09 after `close`.
    pub async fn terminate_transaction(&self, id: u64) -> Result<bool, Error> {
        self.inner.db.check_open()?;
        let Some(t) = self
            .inner
            .db
            .transactions()
            .into_iter()
            .find(|t| t.id == id)
        else {
            return Err(Error::new(
                codes::C22N11,
                "invalid argument: no active transaction has this identifier",
            ));
        };
        if t.principal != self.inner.principal {
            let req: Request = (
                Action::TerminateTransaction,
                Object::Transaction(id),
                t.principal.clone(),
            );
            auth::ask_all(&*self.inner.db.auth, &self.inner.principal, &[req]).await?;
        }
        Ok(t.status.stop(Reason::Terminated))
    }

    /// Reads the graph at the absolute path `graph`: its identifier, path
    /// and owner.
    async fn graph_of(&self, graph: &str) -> Result<(u64, String, String), Error> {
        let s = self.inner.clone();
        let path = graph.to_string();
        blocking(move || {
            let mut tx = s.db.engine_handle()?.begin(Isolation::Snapshot)?;
            let names: Vec<&str> = path.split('/').filter(|n| !n.is_empty()).collect();
            let found = tx.resolve(&names)?;
            tx.abort();
            match found {
                Some(o) if o.kind == Kind::Graph => {
                    Ok((o.id, format!("/{}", names.join("/")), o.owner))
                }
                _ => Err(Error::new(
                    codes::C22N51,
                    "graph reference not found: the path names no graph",
                )),
            }
        })
        .await?
    }

    /// Asks the authoriser one request.
    async fn ask(&self, action: Action, path: String, owner: String) -> Result<(), Error> {
        auth::ask_all(
            &*self.inner.db.auth,
            &self.inner.principal,
            &[(action, Object::Catalog(path), owner)],
        )
        .await
    }

    /// Creates an index of `kind` on the graph at path `graph`, for
    /// `entity` with label or edge type `label` and `properties`
    /// (FR-API-004, FR-IDX-001), authorised as `CREATE INDEX` on the graph.
    /// Returns its identifier once its definition is committed; it is built
    /// in the background and used once ONLINE (TS-18). It runs in a
    /// transaction of its own (TS-18 Online build).
    ///
    /// # Errors
    ///
    /// 22N11 if the properties do not fit the kind; 22N51 if `graph` names
    /// no graph; 42NFF if the authoriser refuses.
    pub async fn create_index(
        &self,
        graph: &str,
        kind: IndexKind,
        entity: Entity,
        label: &str,
        properties: &[&str],
    ) -> Result<u64, Error> {
        let n = properties.len();
        let fits = match kind {
            IndexKind::SingleProperty | IndexKind::Point => n == 1,
            IndexKind::Composite => n >= 2,
            IndexKind::Range => n >= 1,
            IndexKind::Lookup => n == 0,
        };
        if !fits {
            return Err(Error::new(
                codes::C22N11,
                "invalid argument: the number of properties does not fit the index kind",
            ));
        }
        let _busy = self.inner.acquire()?;
        let (id, path, owner) = self.graph_of(graph).await?;
        self.ask(Action::CreateIndex, path, owner).await?;
        let s = self.inner.clone();
        let (label, props) = (label.to_string(), owned(properties));
        blocking(move || {
            let e = s.db.engine_handle()?;
            let (entity, token) = tokens(&e, entity, &label, kind == IndexKind::Lookup)?;
            let props = prop_tokens(&e, &props)?;
            let kind = match kind {
                IndexKind::Lookup => SchemaKind::Lookup,
                IndexKind::Point => SchemaKind::Point,
                _ => SchemaKind::Range,
            };
            e.create_index(id, kind, entity, token, &props)
                .map_err(Error::from)
        })
        .await?
    }

    /// Creates a constraint of `kind` on the graph at path `graph`
    /// (FR-API-004, FR-IDX-002), authorised as `CREATE CONSTRAINT` on the
    /// graph. It validates the existing data and commits in a transaction
    /// of its own (TS-18 Constraints).
    ///
    /// # Errors
    ///
    /// 22N79, 22N77, 22N78 if existing data violates it; 22N11 for an
    /// empty type union or properties that do not fit; 22N51; 42NFF.
    pub async fn create_constraint(
        &self,
        graph: &str,
        kind: ConstraintKind,
        entity: Entity,
        label: &str,
        properties: &[&str],
    ) -> Result<u64, Error> {
        let _busy = self.inner.acquire()?;
        let (id, path, owner) = self.graph_of(graph).await?;
        self.ask(Action::CreateConstraint, path, owner).await?;
        let s = self.inner.clone();
        let (label, props) = (label.to_string(), owned(properties));
        blocking(move || {
            let e = s.db.engine_handle()?;
            let (entity, token) = tokens(&e, entity, &label, false)?;
            let props = prop_tokens(&e, &props)?;
            let kind = match kind {
                ConstraintKind::Unique => SchemaKind::Unique,
                ConstraintKind::Exists => SchemaKind::Exists,
                ConstraintKind::NodeKey => SchemaKind::NodeKey,
                ConstraintKind::PropertyType(specs) => {
                    let set = specs
                        .iter()
                        .map(|t| match *t {
                            TypeSpec::Of(p) => TypeSet::of(p),
                            TypeSpec::ListOf(p) => TypeSet::list(p),
                        })
                        .reduce(TypeSet::or)
                        .ok_or_else(|| {
                            Error::new(
                                codes::C22N11,
                                "invalid argument: a property type constraint needs a type",
                            )
                        })?;
                    SchemaKind::Type(set)
                }
            };
            e.create_constraint(id, kind, entity, token, &props)
                .map_err(Error::from)
        })
        .await?
    }

    /// Drops index `index` of the graph at path `graph`, authorised as
    /// `DROP INDEX` on the graph, in the active transaction if there is
    /// one (FR-CAT-004).
    ///
    /// # Errors
    ///
    /// 22N69 if the graph has no such index; 22N11 for an index a
    /// constraint owns; 22N51; 42NFF.
    pub async fn drop_index(&self, graph: &str, index: u64) -> Result<(), Error> {
        self.drop_schema(graph, index, true).await
    }

    /// Drops constraint `constraint`, as [`Session::drop_index`], with
    /// `DROP CONSTRAINT`.
    ///
    /// # Errors
    ///
    /// 22N64 if the graph has no such constraint; 22N51; 42NFF.
    pub async fn drop_constraint(&self, graph: &str, constraint: u64) -> Result<(), Error> {
        self.drop_schema(graph, constraint, false).await
    }

    async fn drop_schema(&self, graph: &str, id: u64, index: bool) -> Result<(), Error> {
        let _busy = self.inner.acquire()?;
        let (g, path, owner) = self.graph_of(graph).await?;
        let action = if index {
            Action::DropIndex
        } else {
            Action::DropConstraint
        };
        self.ask(action, path, owner).await?;
        let s = self.inner.clone();
        blocking(move || {
            s.in_transaction(|tx| {
                let missing = || {
                    if index {
                        Error::new(codes::C22N69, "index does not exist")
                    } else {
                        Error::new(codes::C22N64, "constraint does not exist")
                    }
                };
                match tx.schema_def(g, id)? {
                    Some(d) if d.kind.is_index() == index => {}
                    _ => return Err(missing()),
                }
                if tx.drop_schema(g, id)? {
                    Ok(())
                } else {
                    Err(missing())
                }
            })
        })
        .await?
    }

    /// Creates the directory at the absolute path `path`, whose parent is
    /// the root or a directory (FR-CAT-005), owned by the session's
    /// principal and authorised as `CREATE DIRECTORY`, in the active
    /// transaction if there is one (FR-CAT-004).
    ///
    /// # Errors
    ///
    /// 42N11 if the name is taken; 22N51 if the parent is missing; 22N11
    /// for a parent of another kind or an invalid name; 42NFF.
    pub async fn create_directory(&self, path: &str) -> Result<u64, Error> {
        let _busy = self.inner.acquire()?;
        self.create_directory_unlocked(path).await
    }

    async fn create_directory_unlocked(&self, path: &str) -> Result<u64, Error> {
        let names: Vec<String> = path
            .split('/')
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect();
        let Some((name, parents)) = names.split_last() else {
            return Err(Error::new(
                codes::C22N11,
                "invalid argument: a directory path needs a name",
            ));
        };
        let (s, ps) = (self.inner.clone(), parents.to_vec());
        let owner = blocking(move || -> Result<String, Error> {
            if ps.is_empty() {
                return Ok(s.principal.clone());
            }
            let mut tx = s.db.engine_handle()?.begin(Isolation::Snapshot)?;
            let refs: Vec<&str> = ps.iter().map(String::as_str).collect();
            let found = tx.resolve(&refs)?;
            tx.abort();
            Ok(found.map_or_else(|| s.principal.clone(), |o| o.owner))
        })
        .await??;
        self.ask(
            Action::CreateDirectory,
            format!("/{}", names.join("/")),
            owner,
        )
        .await?;
        let (s, ps, name) = (self.inner.clone(), parents.to_vec(), name.clone());
        blocking(move || {
            let principal = s.principal.clone();
            s.in_transaction(|tx| {
                let refs: Vec<&str> = ps.iter().map(String::as_str).collect();
                let parent = if refs.is_empty() {
                    ROOT
                } else {
                    match tx.resolve(&refs)? {
                        Some(o) => o.id,
                        None => {
                            return Err(Error::new(
                                codes::C22N51,
                                "graph reference not found: the parent directory is missing",
                            ));
                        }
                    }
                };
                Ok(tx.create_directory(parent, &name, &principal)?)
            })
        })
        .await?
    }
}

/// The rows of a query-result export, pulled from its stream on a blocking
/// thread ([`Session::export`]).
struct StreamRows {
    stream: ResultStream,
    engine: crate::storage::engine::Engine,
    runtime: tokio::runtime::Handle,
}

impl crate::ops::bulk::export::Rows for StreamRows {
    fn columns(&self) -> Vec<String> {
        self.stream.columns().to_vec()
    }

    fn next_row(
        &mut self,
        contents: &mut crate::ops::bulk::export::ContentsMap,
    ) -> Result<Option<Vec<crate::types::Value>>, Error> {
        let Some(row) = self.runtime.block_on(self.stream.next())? else {
            return Ok(None);
        };
        // Between two pulls no batch holds the slot; the transaction is
        // still open, since the stream ends it only after its last batch.
        let mut g = self
            .stream
            .shared()
            .slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = g.as_mut() {
            crate::ops::bulk::export::read_contents(&self.engine, tx, &row, contents)?;
        }
        Ok(Some(row))
    }
}

impl Session {
    /// Exports `scope` in `format` into the files `files` of `location`,
    /// which must exist (FR-IMP-001, FR-IMP-003, UC-OPS-004; TS-26): a
    /// CSV graph export names a node file and an edge file, every other
    /// export one file. Each file is written as `<name>.tmp`, synced and
    /// renamed; the export fails if a target exists and removes its files
    /// on failure. A graph or database export reads one snapshot and asks
    /// `TRAVERSE` and `READ` on each graph; a query-result export runs the
    /// request as [`Session::execute`] does and asks what it asks.
    ///
    /// # Errors
    ///
    /// 22N11 for a format that does not cover the scope, wrong file names,
    /// a target that exists or a name the format cannot hold; 22N51 for a
    /// missing graph; 42NFF for a refusal; the request's errors; 50N00
    /// for I/O failures, a full device among them.
    pub async fn export(
        &self,
        scope: crate::ops::bulk::ExportScope,
        format: crate::ops::bulk::Format,
        location: &std::path::Path,
        files: &[&str],
    ) -> Result<crate::ops::bulk::ExportSummary, Error> {
        use crate::ops::bulk::{ExportScope, check_export, export};
        check_export(format, &scope, files)?;
        let (location, names) = (location.to_path_buf(), owned(files));
        match scope {
            ExportScope::Query { gql, parameters } => {
                let stream = self.execute(&gql, parameters).await?;
                let engine = self.inner.db.engine_handle()?;
                let runtime = tokio::runtime::Handle::current();
                blocking(move || {
                    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                    let mut rows = StreamRows {
                        stream,
                        engine: crate::storage::engine::Engine::handle(engine.core_arc()),
                        runtime,
                    };
                    export::query(&engine, &mut rows, format, &location, &refs)
                })
                .await?
            }
            scope => {
                let _busy = self.inner.acquire()?;
                let s = self.inner.clone();
                blocking(move || {
                    let e = s.db.engine_handle()?;
                    let stored = match &scope {
                        ExportScope::Graph(path) => {
                            export::Stored::Graph(crate::ops::bulk::import::graph_at(&e, path)?.0)
                        }
                        _ => export::Stored::Database,
                    };
                    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                    export::stored(&e, stored, format, &location, &refs, &|r| {
                        s.db.authorise_blocking(&s.principal, r)
                    })
                })
                .await?
            }
        }
    }

    /// Imports `request` (FR-IMP-001 to FR-IMP-005, UC-OPS-003; TS-26): by
    /// one transaction, or by the fast initial load into an empty graph; a
    /// database-scope native dump into a catalog that holds only the
    /// default schema and an empty default graph. New element identifiers
    /// are assigned; input identifiers only connect edges to nodes. Each
    /// invalid record is a line of the report file the request names;
    /// with [`crate::ops::bulk::OnInvalid::Abort`], the default, the import
    /// stops at the first and adds nothing. Asks `CREATE`, `SET LABEL` and
    /// `SET PROPERTY` on the target graph, and for a database dump the
    /// create actions of the objects it creates. The transaction memory
    /// limit is the session's.
    ///
    /// # Errors
    ///
    /// 22N11 for a request that does not fit or a report file that exists;
    /// 22N51; 42NFF; the code of the invalid record that aborted the
    /// import; 22N05 for a malformed header or a damaged dump; 51N41 for a
    /// fast load into a graph that is not empty or a database import into
    /// a catalog that holds other objects; 51N72, 51N73; 50N00.
    pub async fn import(
        &self,
        request: crate::ops::bulk::ImportRequest,
    ) -> Result<crate::ops::bulk::ImportSummary, Error> {
        let _busy = self.inner.acquire()?;
        let s = self.inner.clone();
        let memory = self.limits().memory;
        blocking(move || {
            let e = s.db.engine_handle()?;
            crate::ops::bulk::import::run(&e, &request, &s.principal, memory, &|r| {
                s.db.authorise_blocking(&s.principal, r)
            })
        })
        .await?
    }
}

fn owned(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| (*x).to_string()).collect()
}

/// The entity and the label or edge-type token of an index or constraint,
/// created if missing; 0 for a lookup index.
fn tokens(
    e: &crate::storage::engine::Engine,
    entity: Entity,
    label: &str,
    lookup: bool,
) -> Result<(crate::index::Entity, u64), Error> {
    let (ent, kind) = match entity {
        Entity::Node => (crate::index::Entity::Node, TokenKind::Label),
        Entity::Relationship => (crate::index::Entity::Edge, TokenKind::EdgeType),
    };
    if lookup {
        return Ok((ent, 0));
    }
    Ok((ent, e.token(kind, label)?))
}

fn prop_tokens(e: &crate::storage::engine::Engine, props: &[String]) -> Result<Vec<u64>, Error> {
    props
        .iter()
        .map(|p| e.token(TokenKind::PropertyKey, p).map_err(Error::from))
        .collect()
}

/// The session's active transaction, through the API (FR-GQL-005): GQL
/// `COMMIT` and `ROLLBACK` in the session's text act on it too. Dropping
/// the handle leaves the transaction active in its session.
pub struct Transaction {
    session: Session,
    shared: Arc<TxShared>,
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("id", &self.shared.id)
            .finish_non_exhaustive()
    }
}

impl Transaction {
    /// The transaction's identifier, as the transaction list shows it.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.shared.id
    }

    /// Runs `gql` in this transaction, as [`Session::execute`].
    ///
    /// # Errors
    ///
    /// 25N05 if this is no longer the session's active transaction; as
    /// [`Session::execute`].
    pub async fn execute(&self, gql: &str, parameters: Parameters) -> Result<ResultStream, Error> {
        ResultStream::start(
            &self.session.inner,
            gql,
            parameters,
            false,
            Some(&self.shared),
        )
        .await
    }

    /// Runs the graph algorithm routine `name` in this transaction, as
    /// [`Session::run_algorithm`].
    ///
    /// # Errors
    ///
    /// As [`Session::run_algorithm`] and [`Transaction::execute`].
    pub async fn run_algorithm(
        &self,
        name: &str,
        parameters: Parameters,
    ) -> Result<ResultStream, Error> {
        let (gql, params) = crate::algo::call(&self.session.inner.procs, name, parameters)?;
        self.execute(&gql, params).await
    }

    /// Commits the transaction (FR-TX-001). A cancelled, terminated or
    /// timed-out transaction is aborted instead and fails with 25N14.
    ///
    /// # Errors
    ///
    /// 2D000 if this is no longer the session's active transaction; 25N14;
    /// 25N05 after a failed statement; the commit's errors (25N11 for a
    /// serialization failure).
    pub async fn commit(self) -> Result<(), Error> {
        let busy = self.session.inner.acquire()?;
        self.check_active()?;
        let (s, t) = (self.session.inner.clone(), self.shared.clone());
        blocking(move || {
            let _busy = busy;
            let r = s.db.commit_tx(&t);
            s.clear(&t);
            r
        })
        .await?
    }

    /// Rolls the transaction back.
    ///
    /// # Errors
    ///
    /// 2D000 if this is no longer the session's active transaction.
    pub async fn rollback(self) -> Result<(), Error> {
        let busy = self.session.inner.acquire()?;
        self.check_active()?;
        let (s, t) = (self.session.inner.clone(), self.shared.clone());
        blocking(move || {
            let _busy = busy;
            s.db.abort_tx(&t);
            s.clear(&t);
        })
        .await
    }

    /// Cancels the running statement and the transaction (FR-API-009), as
    /// [`Session::cancel`].
    #[must_use]
    pub fn cancel(&self) -> bool {
        self.shared.status.stop(Reason::Cancelled)
    }

    /// Creates a directory in this transaction (FR-CAT-005), as
    /// [`Session::create_directory`].
    ///
    /// # Errors
    ///
    /// As [`Session::create_directory`]; 2D000 if this is no longer the
    /// session's active transaction.
    pub async fn create_directory(&self, path: &str) -> Result<u64, Error> {
        let _busy = self.session.inner.acquire()?;
        self.check_active()?;
        self.session.create_directory_unlocked(path).await
    }

    fn check_active(&self) -> Result<(), Error> {
        match self.session.inner.active() {
            Some(a) if Arc::ptr_eq(&a, &self.shared) => Ok(()),
            _ => Err(Error::new(
                codes::C2D000,
                "invalid transaction termination: the transaction is not active",
            )),
        }
    }
}
