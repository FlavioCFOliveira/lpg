//! [`Database`]: one open database, a cheap clone of one engine (TS-02).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use super::session::{Limits, Session};
use super::status::{Reason, Status};
use super::{Parameters, blocking, closed_error, internal};
use crate::auth::{self, Action, Authoriser, Object, Request};
use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID, Kind, TokenKind};
use crate::error::Error;
use crate::obs::MetricsSink;
use crate::obs::names::{data_counts, transactions};
use crate::proc::{
    Body, Function, Host, Parameter, ReadProcedure, Registry, Routine, Signature, WriteProcedure,
};
use crate::storage::Storage;
use crate::storage::engine::Engine;
use crate::storage::error::Error as StorageError;
use crate::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::{Arc, Condvar, Mutex, PoisonError, RwLock};
use crate::tx::{Isolation, Transaction};
use crate::types::{DayToSecond, Value, ZonedDateTime};

/// The default timeout of [`Database::close`]: 10 seconds, Neo4j's
/// `db.shutdown_transaction_end_timeout` (FR-DB-008).
pub const DEFAULT_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// The application's abstractions for one database handle.
#[derive(Clone)]
pub struct Options {
    /// Authorises every catalog and data action (ABS-002, FR-AUTH-001).
    pub authoriser: Arc<dyn Authoriser>,
    /// Receives the database's metrics (ABS-004); `None` discards them.
    pub metrics: Option<Arc<dyn MetricsSink>>,
}

impl Options {
    /// Options with `authoriser` and no metrics sink.
    #[must_use]
    pub fn new(authoriser: Arc<dyn Authoriser>) -> Options {
        Options {
            authoriser,
            metrics: None,
        }
    }

    /// The options with `sink` as the metrics sink.
    #[must_use]
    pub fn with_metrics(mut self, sink: Arc<dyn MetricsSink>) -> Options {
        self.metrics = Some(sink);
        self
    }

    fn sink(&self) -> Arc<dyn MetricsSink> {
        self.metrics
            .clone()
            .unwrap_or_else(|| Arc::new(NullSink) as Arc<dyn MetricsSink>)
    }
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("metrics", &self.metrics.is_some())
            .finish_non_exhaustive()
    }
}

/// The sink of a database opened without one.
struct NullSink;

impl MetricsSink for NullSink {
    fn counter(&self, _: &'static str, _: u64) {}
    fn gauge(&self, _: &'static str, _: u64) {}
    fn histogram(&self, _: &'static str, _: u64) {}
}

/// The result of [`Database::check_integrity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    /// Every problem found; empty when the database is intact.
    pub problems: Vec<String>,
}

impl IntegrityReport {
    /// Whether no problem was found.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// An open database (FR-DB-002): a cheap clone of one engine. `close`
/// acts on the engine; afterwards every clone fails with 08N09. Dropping
/// the last clone, and every session made from it, without `close` stops
/// the engine without a final checkpoint, which the next open treats as a
/// crash (FR-DB-009).
#[derive(Clone)]
pub struct Database {
    pub(crate) inner: Arc<DbInner>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("location", &self.inner.location)
            .finish_non_exhaustive()
    }
}

/// The error of an open, create or restore: corruption found at open is
/// 51N40 (`interfaces.md`).
fn open_error(e: &StorageError) -> Error {
    Error::new(e.open_gqlstatus(), e.to_string())
}

impl Database {
    /// Creates a database at `location`, an existing directory, and opens
    /// it (FR-DB-003). `memory_cap` bounds the memory the database accounts
    /// for (FR-LIM-003). `owner` is recorded as the owner of the default
    /// schema `/public` and default graph `/public/lpg`; it is required and
    /// has no default (FR-DB-005). No authorisation request is made
    /// (FR-AUTH-001).
    ///
    /// # Errors
    ///
    /// 42N11 if the location holds a database; 22N11 for a cap too small
    /// or an empty owner; 51N40 if another handle holds the location; 50N00
    /// for I/O errors.
    pub async fn create(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        owner: &str,
        options: Options,
    ) -> Result<Database, Error> {
        let (loc, owner_s, sink) = (location.to_path_buf(), owner.to_string(), options.sink());
        let engine = blocking(move || Engine::create(&loc, storage, memory_cap, &owner_s, sink))
            .await?
            .map_err(|e| open_error(&e))?;
        Ok(Database::wrap(engine, options))
    }

    /// Opens the database at `location` (FR-DB-004, FR-DB-006, FR-DB-007).
    ///
    /// # Errors
    ///
    /// 22N51 if the location holds no database; 51N40 if it is open
    /// elsewhere, needs migration, has a newer format or is corrupt; 50N00
    /// for I/O errors.
    pub async fn open(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        options: Options,
    ) -> Result<Database, Error> {
        let (loc, sink) = (location.to_path_buf(), options.sink());
        let engine = blocking(move || Engine::open(&loc, storage, memory_cap, sink))
            .await?
            .map_err(|e| open_error(&e))?;
        Ok(Database::wrap(engine, options))
    }

    /// Restores the backup at `artefact` into `target`, a location that
    /// holds no database, and opens it (FR-OPS-002, FR-OPS-009). Backup and
    /// restore are API calls only (FR-OPS-010).
    ///
    /// # Errors
    ///
    /// 42N11 if `target` holds a database; 51N41 for an incomplete backup;
    /// 51N40 for another format or a corrupt artefact; 50N00 for I/O
    /// errors.
    pub async fn restore(
        artefact: &Path,
        target: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        options: Options,
    ) -> Result<Database, Error> {
        let (a, t, sink) = (artefact.to_path_buf(), target.to_path_buf(), options.sink());
        let engine =
            blocking(move || crate::ops::restore::restore(&a, &t, storage, memory_cap, sink))
                .await?
                .map_err(|e| open_error(&e))?;
        Ok(Database::wrap(engine, options))
    }

    /// Migrates the closed database at `location` to the current format
    /// (FR-OPS-004). Returns whether it changed anything.
    ///
    /// # Errors
    ///
    /// 51N41 if the database was not closed cleanly or no migration path
    /// exists; 51N40 if it is open elsewhere; 50N00 for I/O errors.
    pub async fn migrate(location: &Path, storage: Arc<dyn Storage>) -> Result<bool, Error> {
        let loc = location.to_path_buf();
        blocking(move || crate::ops::migrate::migrate(&loc, &*storage))
            .await?
            .map(|m| m.is_some())
            .map_err(|e| open_error(&e))
    }

    fn wrap(engine: Engine, options: Options) -> Database {
        let core = engine.core_arc();
        let (location, sink) = (core.location.clone(), core.sink.clone());
        drop(core);
        Database {
            inner: Arc::new(DbInner {
                location,
                sink,
                engine: RwLock::new(Some(engine)),
                closed: AtomicBool::new(false),
                auth: options.authoriser,
                registry: Arc::new(RwLock::new(Registry::with_builtins())),
                txs: Mutex::new(BTreeMap::new()),
                txs_changed: Condvar::new(),
                next_tx: AtomicU64::new(1),
            }),
        }
    }

    /// Opens a session of `principal`, trusted and not authenticated
    /// (FR-SES-001), with the home schema and home graph at the given
    /// paths, or the defaults `/public` and `/public/lpg` (FR-SES-002). Its
    /// time zone is UTC (FR-SES-003).
    ///
    /// # Errors
    ///
    /// 08N09 after `close`; 22N51 if a home path names no schema or graph.
    pub async fn session(
        &self,
        principal: &str,
        home_schema: Option<&str>,
        home_graph: Option<&str>,
    ) -> Result<Session, Error> {
        self.inner.check_open()?;
        let db = self.inner.clone();
        let (hs, hg) = (
            home_schema.map(str::to_string),
            home_graph.map(str::to_string),
        );
        let (schema, graph) = blocking(move || -> Result<(u64, u64), Error> {
            let mut tx = db.engine_handle()?.begin(Isolation::Snapshot)?;
            let mut find = |p: Option<String>, kind: Kind, default: u64| match p {
                None => Ok(default),
                Some(p) => {
                    let names: Vec<&str> = p.split('/').filter(|n| !n.is_empty()).collect();
                    match tx.resolve(&names)? {
                        Some(o) if o.kind == kind => Ok(o.id),
                        _ => Err(Error::new(
                            crate::gqlstatus::codes::C22N51,
                            "graph reference not found: the home path names no object of its kind",
                        )),
                    }
                }
            };
            let s = find(hs, Kind::Schema, DEFAULT_SCHEMA_ID)?;
            let g = find(hg, Kind::Graph, DEFAULT_GRAPH_ID)?;
            Ok((s, g))
        })
        .await??;
        Ok(Session::new(
            self.inner.clone(),
            principal.to_string(),
            schema,
            graph,
        ))
    }

    /// Registers the read-only procedure `procedure` under `signature`
    /// (FR-API-005). Registrations are not persisted (FR-PROC-003). The
    /// registry lives in memory: registering performs no I/O.
    ///
    /// # Errors
    ///
    /// 22N11 if the name is taken, empty or has a schema part, or if a
    /// parameter without a default follows one with a default.
    pub fn register_procedure(
        &self,
        signature: Signature,
        procedure: Arc<dyn ReadProcedure>,
    ) -> Result<(), Error> {
        self.register(Routine {
            sig: signature,
            body: Body::Read(procedure),
        })
    }

    /// Registers the read-write procedure `procedure` under `signature`;
    /// it runs in the caller's transaction (FR-PROC-002).
    ///
    /// # Errors
    ///
    /// As [`Database::register_procedure`].
    pub fn register_write_procedure(
        &self,
        signature: Signature,
        procedure: Arc<dyn WriteProcedure>,
    ) -> Result<(), Error> {
        self.register(Routine {
            sig: signature,
            body: Body::Write(procedure),
        })
    }

    /// Registers the function `function` named `name` with `params` and
    /// the one result column `column` (FR-API-006): it is called with
    /// `CALL` and returns one row.
    ///
    /// # Errors
    ///
    /// As [`Database::register_procedure`].
    pub fn register_function(
        &self,
        name: &str,
        params: Vec<Parameter>,
        column: &str,
        function: Arc<dyn Function>,
    ) -> Result<(), Error> {
        self.register(Routine {
            sig: Signature::new(name, params, &[column]),
            body: Body::Function(function),
        })
    }

    fn register(&self, r: Routine) -> Result<(), Error> {
        self.inner.check_open()?;
        self.inner
            .registry
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .register(r)
    }

    /// The names of every built-in and registered routine.
    #[must_use]
    pub fn routines(&self) -> Vec<String> {
        self.inner
            .registry
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .names()
    }

    /// Takes an online, consistent backup into `target`, an empty
    /// directory (FR-OPS-001, FR-OPS-007, FR-OPS-008).
    ///
    /// # Errors
    ///
    /// 08N09 after `close`; 22N11 if `target` is not empty; 51N41 while a
    /// fast initial load runs; 50N00 for I/O errors.
    pub async fn backup(&self, target: &Path) -> Result<(), Error> {
        self.inner.check_open()?;
        let (db, t) = (self.inner.clone(), target.to_path_buf());
        blocking(move || {
            db.engine_handle()?
                .backup(&t)
                .map(drop)
                .map_err(Error::from)
        })
        .await?
    }

    /// Checks the whole open database (FR-REC-010) and sets the data count
    /// gauges from the elements it counts (TS-24 Metrics).
    ///
    /// # Errors
    ///
    /// 08N09 after `close` or once the database is unusable; 50N00 for
    /// I/O errors.
    pub async fn check_integrity(&self) -> Result<IntegrityReport, Error> {
        self.inner.check_open()?;
        let db = self.inner.clone();
        blocking(move || db.check_integrity()).await?
    }

    /// Closes the database with the default timeout of 10 seconds
    /// ([`DEFAULT_CLOSE_TIMEOUT`], FR-DB-008).
    ///
    /// # Errors
    ///
    /// As [`Database::close_with_timeout`].
    pub async fn close(&self) -> Result<(), Error> {
        self.close_with_timeout(DEFAULT_CLOSE_TIMEOUT).await
    }

    /// Closes the database (FR-DB-008): waits up to `timeout` for the
    /// active transactions to end, aborts those that remain, then runs the
    /// final checkpoint. Afterwards every clone fails with 08N09.
    ///
    /// # Errors
    ///
    /// 08N09 if the database is already closed or unusable; 50N00 for
    /// I/O errors of the final checkpoint.
    pub async fn close_with_timeout(&self, timeout: Duration) -> Result<(), Error> {
        let db = self.inner.clone();
        {
            let _g = db.txs.lock().unwrap_or_else(PoisonError::into_inner);
            if db.closed.swap(true, Ordering::AcqRel) {
                return Err(closed_error());
            }
        }
        blocking(move || db.close(timeout)).await?
    }

    /// The engine, for the crate's tests.
    #[cfg(test)]
    pub(crate) fn engine_handle(&self) -> Engine {
        self.inner.engine_handle().expect("an open database")
    }
}

/// The shared state of a database handle. It holds the engine's shared
/// state only through the owning handle, so that once `close` has dropped
/// it and the transactions have ended, the files are closed and `LOCK` is
/// released (TS-27) while clones of the `Database` still exist.
pub(crate) struct DbInner {
    pub(crate) location: std::path::PathBuf,
    pub(crate) sink: Arc<dyn MetricsSink>,
    /// The owning engine handle, until `close`.
    engine: RwLock<Option<Engine>>,
    closed: AtomicBool,
    pub(crate) auth: Arc<dyn Authoriser>,
    pub(crate) registry: Arc<RwLock<Registry>>,
    /// The active transactions, by identifier.
    txs: Mutex<BTreeMap<u64, Arc<TxShared>>>,
    /// Signalled when a transaction ends.
    txs_changed: Condvar,
    next_tx: AtomicU64,
}

impl DbInner {
    /// Fails with 08N09 after `close`, or once the database is unusable.
    pub(crate) fn check_open(&self) -> Result<(), Error> {
        if self.closed.load(Ordering::Acquire) {
            return Err(closed_error());
        }
        self.engine_handle()?.check().map_err(Error::from)
    }

    /// A handle of the engine for one operation.
    ///
    /// # Errors
    ///
    /// 08N09 after `close`.
    pub(crate) fn engine_handle(&self) -> Result<Engine, Error> {
        match &*self.engine.read().unwrap_or_else(PoisonError::into_inner) {
            Some(e) => Ok(Engine::handle(e.core_arc())),
            None => Err(closed_error()),
        }
    }

    /// Begins and registers a transaction of `principal` (rmp #47, #49):
    /// its memory limit is set in the kernel and its time limit starts a
    /// timer that stops it.
    pub(crate) fn begin_tx(
        &self,
        principal: &str,
        isolation: Isolation,
        limits: &Limits,
        metadata: Parameters,
    ) -> Result<Arc<TxShared>, Error> {
        let mut tx = self.engine_handle()?.begin(isolation)?;
        tx.memory_limit = limits.memory;
        let shared = Arc::new(TxShared {
            id: self.next_tx.fetch_add(1, Ordering::Relaxed),
            principal: principal.to_string(),
            metadata,
            started: Instant::now(),
            start_ts: crate::types::temporal::now(),
            status: Status::default(),
            slot: Mutex::new(Some(tx)),
            current: Mutex::new(None),
            limits: *limits,
            timer: Mutex::new(None),
        });
        {
            let mut txs = self.txs.lock().unwrap_or_else(PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) {
                return Err(closed_error());
            }
            txs.insert(shared.id, shared.clone());
        }
        if let Some(d) = limits.time
            && let Ok(h) = tokio::runtime::Handle::try_current()
        {
            let s = shared.clone();
            let task = h.spawn(async move {
                tokio::time::sleep(d).await;
                s.status.stop(Reason::TimedOut);
            });
            *shared.timer.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
        }
        self.sink.counter(transactions::STARTED, 1);
        tracing::info!(
            target: "lpg::transaction",
            transaction_id = shared.id,
            principal = %shared.principal,
            tx_metadata = ?shared.metadata,
            "start"
        );
        Ok(shared)
    }

    /// Ends `t`: unregisters it, stops its timer and reports it.
    pub(crate) fn end_tx(&self, t: &TxShared, committed: bool) {
        if let Some(task) = t
            .timer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
        let removed = {
            let mut txs = self.txs.lock().unwrap_or_else(PoisonError::into_inner);
            let r = txs.remove(&t.id).is_some();
            self.txs_changed.notify_all();
            r
        };
        if !removed {
            return;
        }
        if !committed {
            self.sink.counter(transactions::ROLLBACKS, 1);
        }
        tracing::info!(
            target: "lpg::transaction",
            transaction_id = t.id,
            principal = %t.principal,
            tx_metadata = ?t.metadata,
            "{}",
            if committed { "commit" } else { "rollback" }
        );
    }

    /// Commits `t` (rmp #49 cancel against commit): a stopped transaction
    /// is aborted instead.
    ///
    /// # Errors
    ///
    /// 25N14 for a stopped transaction; 25N05 for one already ended; the
    /// commit's errors.
    pub(crate) fn commit_tx(&self, t: &TxShared) -> Result<(), Error> {
        if let Err(r) = t.status.begin_commit() {
            self.abort_tx(t);
            return Err(r.error());
        }
        let tx = t.take();
        let Some(tx) = tx else {
            self.end_tx(t, false);
            return Err(ended());
        };
        let r = tx.commit();
        self.end_tx(t, r.is_ok());
        r.map(drop).map_err(Error::from)
    }

    /// Aborts `t` and ends it.
    pub(crate) fn abort_tx(&self, t: &TxShared) {
        if let Some(tx) = t.take() {
            tx.abort();
        }
        self.end_tx(t, false);
    }

    /// The active transactions.
    pub(crate) fn transactions(&self) -> Vec<Arc<TxShared>> {
        self.txs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// Asks every request for `principal`, from a blocking thread of the
    /// runtime (`Handle::block_on`, which may run there).
    pub(crate) fn authorise_blocking(
        &self,
        principal: &str,
        requests: &[Request],
    ) -> Result<(), Error> {
        if requests.is_empty() {
            return Ok(());
        }
        let h = tokio::runtime::Handle::try_current()
            .map_err(|_| internal("an authorisation request outside a tokio runtime"))?;
        h.block_on(auth::ask_all(&*self.auth, principal, requests))
    }

    fn close(&self, timeout: Duration) -> Result<(), Error> {
        let deadline = Instant::now() + timeout;
        let mut txs = self.txs.lock().unwrap_or_else(PoisonError::into_inner);
        while !txs.is_empty() {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            txs = self
                .txs_changed
                .wait_timeout(txs, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        let left: Vec<Arc<TxShared>> = txs.values().cloned().collect();
        drop(txs);
        for t in left {
            // A running statement stops at its next check; then its
            // transaction is aborted here.
            t.status.stop(Reason::Terminated);
            self.abort_tx(&t);
        }
        let engine = self
            .engine
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match engine {
            Some(e) => e.close().map_err(Error::from),
            None => Err(closed_error()),
        }
    }

    fn check_integrity(&self) -> Result<IntegrityReport, Error> {
        let e = self.engine_handle()?;
        let mut problems = e.store().check()?.problems;
        problems.extend(e.check_graphs()?);
        // The data counts (TS-24): every node and edge the snapshot sees.
        let mut tx = e.begin(Isolation::Snapshot)?;
        let (mut nodes, mut edges) = (0u64, 0u64);
        for g in tx.graph_ids()? {
            let g = tx.graph(g)?;
            tx.scan_range(g.nodes, b"", None, |_, _| {
                nodes += 1;
                true
            })?;
            tx.scan_range(g.edges, b"", None, |_, _| {
                edges += 1;
                true
            })?;
        }
        tx.abort();
        let types = e
            .core()
            .catalog
            .tokens
            .all()
            .iter()
            .filter(|(_, k, _)| *k == TokenKind::EdgeType)
            .count() as u64;
        let sink = &self.sink;
        sink.gauge(data_counts::NODE, nodes);
        sink.gauge(data_counts::RELATIONSHIP, edges);
        sink.gauge(data_counts::RELATIONSHIP_TYPES, types);
        Ok(IntegrityReport { problems })
    }

    /// The `show_transactions` rows and the `list_transactions` entries
    /// `principal` may see: its own, and those of others the authoriser
    /// lets it see (FR-OBS-003).
    pub(crate) fn visible(&self, principal: &str) -> Result<Vec<Arc<TxShared>>, Error> {
        let mut out = Vec::new();
        for t in self.transactions() {
            if t.principal == principal {
                out.push(t);
                continue;
            }
            let req: Request = (
                Action::ShowTransaction,
                Object::Transaction(t.id),
                t.principal.clone(),
            );
            if self.authorise_blocking(principal, &[req]).is_ok() {
                out.push(t);
            }
        }
        Ok(out)
    }
}

impl Host for DbInner {
    fn authorise(&self, principal: &str, requests: &[Request]) -> Result<(), Error> {
        self.authorise_blocking(principal, requests)
    }

    fn transactions(&self, principal: &str) -> Vec<Vec<Value>> {
        self.visible(principal)
            .unwrap_or_default()
            .iter()
            .map(|t| t.row())
            .collect()
    }
}

/// The error of a request on a transaction that has ended.
pub(crate) fn ended() -> Error {
    Error::new(
        crate::gqlstatus::codes::C25N05,
        "invalid transaction state: the transaction has ended",
    )
}

/// One active transaction, as the session, the transaction list and close
/// share it.
pub(crate) struct TxShared {
    pub(crate) id: u64,
    pub(crate) principal: String,
    pub(crate) metadata: Parameters,
    pub(crate) started: Instant,
    start_ts: jiff::Timestamp,
    pub(crate) status: Status,
    /// The kernel transaction; taken when it ends. A batch holds the lock
    /// while it runs.
    pub(crate) slot: Mutex<Option<Transaction>>,
    /// The text of the statement running or last run.
    pub(crate) current: Mutex<Option<String>>,
    pub(crate) limits: Limits,
    timer: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TxShared {
    /// Takes the kernel transaction, ending its use.
    pub(crate) fn take(&self) -> Option<Transaction> {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// The status as the transaction list shows it: `Running`, or
    /// `Terminated` with the reason.
    pub(crate) fn status_text(&self) -> String {
        match self.status.reason() {
            None => "Running".to_string(),
            Some(r) => format!("Terminated with reason: {}", r.text()),
        }
    }

    /// The text of the current statement, if any.
    pub(crate) fn current_query(&self) -> Option<String> {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The `show_transactions` row (FR-PROC-004).
    fn row(&self) -> Vec<Value> {
        let elapsed = self.started.elapsed();
        let since = DayToSecond::new(
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
            i64::from(elapsed.subsec_nanos()),
        )
        .map_or(Value::Null, Value::DayToSecond);
        let start = Value::ZonedDateTime(Box::new(ZonedDateTime::new(
            self.start_ts.to_zoned(jiff::tz::TimeZone::UTC),
        )));
        let meta: Vec<(Arc<str>, Value)> = self
            .metadata
            .iter()
            .map(|(k, v)| (Arc::from(k.as_str()), v.clone()))
            .collect();
        vec![
            Value::string(&format!("transaction-{}", self.id)),
            Value::string(&self.principal),
            self.current_query()
                .map_or(Value::Null, |q| Value::string(&q)),
            start,
            Value::string(&self.status_text()),
            since,
            Value::Record(meta.into()),
        ]
    }
}
