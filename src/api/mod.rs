//! The public asynchronous API ([TS-02], `interfaces.md` "Public API
//! shape"; rmp #47 to #51).
//!
//! * [`Database`]: create, open, close with a timeout, sessions, the
//!   procedure and function registry, backup, restore, migration and the
//!   integrity check. A cheap clone of one engine.
//! * [`Session`]: GQL requests ([`Session::execute`]), transactions
//!   ([`Session::begin`]), EXPLAIN and PROFILE, cancellation, limits, the
//!   transaction list and termination, and the management of indexes,
//!   constraints and directories (FR-API-004, FR-CAT-005).
//! * [`Transaction`]: the session's active transaction through the API;
//!   GQL `START TRANSACTION`, `COMMIT` and `ROLLBACK` act on the same one
//!   (FR-GQL-005).
//! * [`ResultStream`]: one row per `next().await` (FR-API-010), then a
//!   [`Summary`].
//!
//! **Boundary** (TS-02): the engine is synchronous. Every request, and
//! every pull of a result batch, moves the work into
//! `tokio::task::spawn_blocking`, which runs one batch of at most 1 024
//! rows and returns; no thread is held while the caller does not pull. A
//! transaction owns what it uses (TS-02 "Owned transactions"), so it and
//! the statement's cursor move into the closure. The caller's `tracing`
//! dispatcher goes with it, so events of the blocking part reach the
//! caller's subscriber. Turso's Rust binding (R-TURSO @ `8cc059b`
//! `bindings/rust/src/{rows,connection,transaction}.rs`) and neo4rs's
//! `RowStream::next` were studied; nothing is copied.
//!
//! [TS-02]: ../../docs/technical-spec/architecture.md

mod database;
mod session;
mod status;
mod stream;
#[cfg(test)]
mod tests;

pub use database::{DEFAULT_CLOSE_TIMEOUT, Database, IntegrityReport, Options};
pub use session::{
    ConstraintKind, Entity, IndexKind, Limits, Session, Transaction, TransactionInfo,
    TransactionOptions, TypeSpec,
};
pub use stream::{Counters, ResultStream, StatementType, Summary};

use std::collections::BTreeMap;

use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// The parameters of a request, by name without `$` (IW006).
pub type Parameters = BTreeMap<String, Value>;

/// An internal failure (50N00).
pub(crate) fn internal(what: &str) -> Error {
    Error::new(codes::C50N00, format!("internal error: {what}"))
}

/// The error of any call after `close` (08N09, `interfaces.md`).
pub(crate) fn closed_error() -> Error {
    Error::new(
        codes::C08N09,
        "database unavailable: the database is closed",
    )
}

/// Runs `f` on tokio's blocking pool, with the caller's `tracing`
/// dispatcher (module documentation).
///
/// # Errors
///
/// 50N00 if `f` panicked.
pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Error> {
    let d = tracing::dispatcher::get_default(Clone::clone);
    tokio::task::spawn_blocking(move || tracing::dispatcher::with_default(&d, f))
        .await
        .map_err(|_| internal("a statement's thread panicked"))
}
