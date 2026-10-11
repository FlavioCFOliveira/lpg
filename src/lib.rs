//! `lpg` is an embeddable Labeled Property Graph database queried with GQL
//! (ISO/IEC 39075:2024).
//!
//! # Platform
//!
//! Linux is the only supported platform. The crate uses Rust edition 2024
//! with `std`, and `unsafe` code is forbidden.

#![forbid(unsafe_code)]
// Under the model checkers only the `loom_*` and `shuttle_*` tests are
// built in; the code the other tests use is then unused (TS-30).
#![cfg_attr(any(loom, shuttle), allow(dead_code, unused_imports))]

pub mod api;
pub mod auth;
pub(crate) mod catalog;
mod error;
#[cfg(fuzzing)]
pub mod fuzz;
pub mod gqlstatus;
pub(crate) mod index;
pub mod obs;
pub mod ops;
pub mod proc;
pub(crate) mod query;
pub mod storage;
mod sync;
pub mod tx;
pub(crate) mod types;

pub use api::{
    ConstraintKind, Counters, DEFAULT_CLOSE_TIMEOUT, Database, Entity, IndexKind, IntegrityReport,
    Limits, Options, Parameters, ResultStream, Session, StatementType, Summary, Transaction,
    TransactionInfo, TransactionOptions, TypeSpec,
};
pub use error::{Error, Position};
pub use index::PropertyType;
pub use query::exec::profile::ProfileStats;
pub use query::plan::Description as PlanDescription;
pub use tx::Isolation;
pub use types::{DayToSecond, Point, Srid, Value, ZonedDateTime, ZonedTime};

#[cfg(test)]
mod lab;
#[cfg(test)]
mod suites;
#[cfg(test)]
mod test_support;
