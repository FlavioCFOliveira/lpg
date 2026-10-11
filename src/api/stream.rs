//! [`ResultStream`]: a statement's rows, one `next().await` at a time
//! (FR-API-010), and its [`Summary`] (`interfaces.md`
//! `ResultStream::summary()`, #39 point 11).
//!
//! Each pull of an empty buffer moves the cursor into `spawn_blocking`,
//! runs one batch, counts it against the row and byte limits (FR-LIM-002)
//! and returns it. After the last batch, the same blocking call ends the
//! request: an auto-commit transaction commits, `COMMIT` or `ROLLBACK` in
//! the text ends the active one, `SESSION CLOSE` closes the session. A
//! stream dropped before its end drops the cursor, which releases the
//! statement's memory, and rolls back a transaction the request would have
//! ended; the session's explicit transaction stays active.

use std::collections::VecDeque;
use std::time::Instant;

use super::database::{TxShared, ended};
use super::session::{Busy, SessionInner, fail_in, open_request};
use super::{Parameters, blocking};
use crate::error::Error;
use crate::gqlstatus::{GqlStatus, codes};
use crate::obs::names::query_execution;
use crate::query::bound::StatementKind;
use crate::query::exec::{Batch, Cursor};
use crate::query::plan::Description;
use crate::storage::counters;
use crate::sync::{Arc, PoisonError};
use crate::types::Value;
use crate::types::record::value_size;

/// What the end of a request does to its transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// Nothing: the session's active transaction goes on.
    Keep,
    /// Commits the request's own transaction (auto-commit).
    Auto,
    /// `COMMIT`: commits the active transaction.
    Commit,
    /// `ROLLBACK`: rolls the active transaction back.
    Rollback,
}

/// The write counters of a statement, as Neo4j's `SummaryCounters`
/// (`interfaces.md`). The deletes and label changes stay zero while their
/// statements (GD02 to GD04) are not supported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Nodes created.
    pub nodes_created: u64,
    /// Nodes deleted.
    pub nodes_deleted: u64,
    /// Edges created (Neo4j's relationships).
    pub edges_created: u64,
    /// Edges deleted.
    pub edges_deleted: u64,
    /// Properties set.
    pub properties_set: u64,
    /// Labels added.
    pub labels_added: u64,
    /// Labels removed.
    pub labels_removed: u64,
    /// Catalog objects created or dropped (Neo4j's `systemUpdates`).
    pub catalog_updates: u64,
}

impl Counters {
    /// Whether any counter is above zero.
    #[must_use]
    pub fn contains_updates(&self) -> bool {
        *self != Counters::default()
    }
}

/// What a statement does, as Bolt's `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementType {
    /// Reads only.
    Read,
    /// Writes only.
    Write,
    /// Reads and writes.
    ReadWrite,
    /// Modifies the catalog.
    Catalog,
}

/// The summary of a statement, available once its last row is pulled.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Its write counters.
    pub counters: Counters,
    /// Its type.
    pub statement_type: StatementType,
    /// Its completion condition: 00000 for a result with rows, 02000 for a
    /// result without rows, 00001 for a statement with no result columns.
    pub gqlstatus: GqlStatus,
    /// The profiled plan (PROFILE only).
    pub profile: Option<Description>,
}

/// What a statement has produced so far.
#[derive(Debug, Clone, Copy, Default)]
struct Progress {
    rows: u64,
    bytes: u64,
    hits: u64,
    misses: u64,
    peak: u64,
    /// The highest number of the statement's operators running at once
    /// (NFR-PERF-007), measured by PROFILE; for the conformance runner.
    #[cfg(test)]
    max_running: usize,
}

/// The outcome of one blocking step.
enum Step {
    Rows(Batch),
    Done(Box<Summary>),
}

/// The rows of a statement (FR-API-010).
pub struct ResultStream {
    session: Arc<SessionInner>,
    tx: Arc<TxShared>,
    cursor: Option<Cursor>,
    columns: Vec<String>,
    buffer: VecDeque<Vec<Value>>,
    end: End,
    close: bool,
    done: bool,
    summary: Option<Summary>,
    progress: Progress,
    span: tracing::Span,
    started: Instant,
    busy: Option<Busy>,
}

impl std::fmt::Debug for ResultStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultStream")
            .field("columns", &self.columns)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

/// The error of a row or byte limit (51N66, `interfaces.md`).
fn limit_error(what: &str) -> Error {
    Error::new(
        codes::C51N66,
        format!("resource exhaustion: the statement returned more {what} than its limit"),
    )
}

/// One step of a stream on its blocking thread (module documentation).
fn step(
    s: &Arc<SessionInner>,
    t: &Arc<TxShared>,
    cursor: &mut Cursor,
    end: End,
    close: bool,
    p: &mut Progress,
) -> Result<Step, Error> {
    let silent = cursor.columns().is_empty();
    loop {
        let mut g = t.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(tx) = g.as_mut() else {
            drop(g);
            return Err(stop(s, t, end, ended()));
        };
        let before = counters::current();
        let r = cursor.next_batch(tx);
        #[cfg(test)]
        {
            p.max_running = p.max_running.max(cursor.max_running().unwrap_or(0));
        }
        let after = counters::current();
        p.hits += after.hits - before.hits;
        p.misses += after.misses - before.misses;
        p.peak = p.peak.max(cursor.peak_memory()).max(tx.memory_held());
        match r {
            Ok(Some(b)) => {
                if silent {
                    continue;
                }
                p.rows += b.len() as u64;
                p.bytes += b.rows().flat_map(|r| r.iter()).map(value_size).sum::<u64>();
                let lim = t.limits;
                let over = if lim.rows.is_some_and(|l| p.rows > l) {
                    Some(limit_error("rows"))
                } else if lim.bytes.is_some_and(|l| p.bytes > l) {
                    Some(limit_error("bytes"))
                } else {
                    None
                };
                if let Some(e) = over {
                    fail_in(tx);
                    drop(g);
                    return Err(stop(s, t, end, e));
                }
                return Ok(Step::Rows(b));
            }
            Ok(None) => {
                drop(g);
                let summary = Summary {
                    counters: {
                        let c = cursor.counters();
                        Counters {
                            nodes_created: c.nodes_created,
                            edges_created: c.edges_created,
                            properties_set: c.properties_set,
                            catalog_updates: c.catalog_updates,
                            ..Counters::default()
                        }
                    },
                    statement_type: if cursor.is_catalog() {
                        StatementType::Catalog
                    } else {
                        match cursor.kind() {
                            StatementKind::Read => StatementType::Read,
                            StatementKind::Write => StatementType::Write,
                            StatementKind::ReadWrite => StatementType::ReadWrite,
                        }
                    },
                    gqlstatus: if silent {
                        codes::C00001
                    } else if p.rows == 0 {
                        codes::C02000
                    } else {
                        codes::C00000
                    },
                    profile: cursor.profile(),
                };
                let r = match end {
                    End::Keep => Ok(()),
                    End::Auto | End::Commit => s.db.commit_tx(t),
                    End::Rollback => {
                        s.db.abort_tx(t);
                        Ok(())
                    }
                };
                if matches!(end, End::Commit | End::Rollback) {
                    s.clear(t);
                }
                if close {
                    s.close();
                }
                return r.map(|()| Step::Done(Box::new(summary)));
            }
            Err(e) => {
                drop(g);
                return Err(stop(s, t, end, e));
            }
        }
    }
}

/// Ends the request's transaction after its statement failed with `e`,
/// when the request would have ended it, and returns `e`.
fn stop(s: &SessionInner, t: &Arc<TxShared>, end: End, e: Error) -> Error {
    if end != End::Keep {
        s.db.abort_tx(t);
        s.clear(t);
    }
    e
}

impl ResultStream {
    /// Opens the request `gql` (module documentation); a statement without
    /// result columns runs to its end here.
    pub(crate) async fn start(
        s: &Arc<SessionInner>,
        gql: &str,
        params: Parameters,
        profiling: bool,
        via: Option<&Arc<TxShared>>,
    ) -> Result<ResultStream, Error> {
        let busy = s.acquire()?;
        let span = tracing::info_span!(
            target: "lpg::query",
            "query",
            query = %gql,
            tx_metadata = tracing::field::Empty,
            principal = %s.principal,
            transaction_id = tracing::field::Empty,
        );
        tracing::info!(target: "lpg::query", parent: &span, "start");
        let started = Instant::now();
        let (s2, text, via2) = (s.clone(), gql.to_string(), via.cloned());
        let opened = blocking(move || open_request(&s2, &text, &params, profiling, via2.as_ref()))
            .await
            .and_then(|r| r);
        let o = match opened {
            Ok(o) => o,
            Err(e) => {
                record_failure(s, &span, &e);
                return Err(e);
            }
        };
        span.record("transaction_id", o.tx.id);
        span.record("tx_metadata", tracing::field::debug(&o.tx.metadata));
        let columns = o
            .cursor
            .columns()
            .iter()
            .map(|c| (*c).to_string())
            .collect();
        let silent = o.cursor.columns().is_empty();
        let mut stream = ResultStream {
            session: s.clone(),
            tx: o.tx,
            cursor: Some(o.cursor),
            columns,
            buffer: VecDeque::new(),
            end: o.end,
            close: o.close,
            done: false,
            summary: None,
            progress: Progress::default(),
            span,
            started,
            busy: Some(busy),
        };
        if silent {
            stream.next().await?;
        }
        Ok(stream)
    }

    /// The highest number of operators of a profiled statement that ran at
    /// once (NFR-PERF-007); 0 for a statement that is not profiled.
    #[cfg(test)]
    pub(crate) fn max_running(&self) -> usize {
        self.progress.max_running
    }

    /// The result column names.
    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The next row, one value per column; `None` after the last.
    ///
    /// # Errors
    ///
    /// The statement's errors (its transaction is then aborted); 51N66 for
    /// a row or byte limit; 25N14 when cancelled, terminated or timed out;
    /// the commit's errors for an auto-commit statement.
    pub async fn next(&mut self) -> Result<Option<Vec<Value>>, Error> {
        loop {
            if let Some(r) = self.buffer.pop_front() {
                return Ok(Some(r));
            }
            if self.done {
                return Ok(None);
            }
            let Some(cursor) = self.cursor.take() else {
                // A previous `next()` was dropped before it returned: its
                // batch was lost with its cursor.
                self.done = true;
                let e = stop(&self.session, &self.tx, self.end, ended());
                record_failure(&self.session, &self.span, &e);
                self.busy = None;
                return Err(e);
            };
            let (s, t, end, close) = (self.session.clone(), self.tx.clone(), self.end, self.close);
            let mut p = self.progress;
            let r = blocking(move || {
                let mut c = cursor;
                let r = step(&s, &t, &mut c, end, close, &mut p);
                (c, p, r)
            })
            .await;
            let (c, p, r) = match r {
                Ok(x) => x,
                Err(e) => {
                    self.done = true;
                    record_failure(&self.session, &self.span, &e);
                    self.busy = None;
                    return Err(e);
                }
            };
            self.cursor = Some(c);
            self.progress = p;
            match r {
                Ok(Step::Rows(b)) => {
                    self.buffer.extend(b.rows().map(<[Value]>::to_vec));
                }
                Ok(Step::Done(summary)) => {
                    self.done = true;
                    self.summary = Some(*summary);
                    self.record_success();
                    self.busy = None;
                    return Ok(None);
                }
                Err(e) => {
                    self.done = true;
                    record_failure(&self.session, &self.span, &e);
                    self.busy = None;
                    return Err(e);
                }
            }
        }
    }

    /// The summary, once the last row is pulled.
    #[must_use]
    pub fn summary(&self) -> Option<&Summary> {
        self.summary.as_ref()
    }

    fn record_success(&self) {
        let ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let sink = &self.session.db.sink;
        sink.counter(query_execution::SUCCESS, 1);
        sink.histogram(query_execution::LATENCY_MILLIS, ms);
        tracing::info!(
            target: "lpg::query",
            parent: &self.span,
            elapsed_ms = ms,
            peak_memory_bytes = self.progress.peak,
            buffer_hits = self.progress.hits,
            buffer_misses = self.progress.misses,
            "success"
        );
    }

    /// Rows produced by the statement's scans so far (FR-API-010 checks).
    #[cfg(test)]
    pub(crate) fn scan_rows(&self) -> u64 {
        self.cursor.as_ref().map_or(0, Cursor::scan_rows)
    }

    /// The identifier of the statement's transaction.
    #[cfg(test)]
    pub(crate) fn transaction_id(&self) -> u64 {
        self.tx.id
    }
}

/// Counts and reports a failed request.
fn record_failure(s: &SessionInner, span: &tracing::Span, e: &Error) {
    s.db.sink.counter(query_execution::FAILURE, 1);
    tracing::info!(
        target: "lpg::query",
        parent: span,
        gqlstatus = e.code(),
        message = e.message(),
        "fail"
    );
}

impl Drop for ResultStream {
    fn drop(&mut self) {
        if !self.done {
            // The cursor goes first: its reservations are released.
            self.cursor = None;
            if self.end != End::Keep {
                self.session.db.abort_tx(&self.tx);
                self.session.clear(&self.tx);
            }
        }
    }
}
