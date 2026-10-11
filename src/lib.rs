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

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the catalog API's first non-test callers come with Sprint 3"
    )
)]
pub(crate) mod catalog;
mod error;
#[cfg(fuzzing)]
pub mod fuzz;
pub mod gqlstatus;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the index API's first non-test callers come with Sprint 3"
    )
)]
pub(crate) mod index;
pub mod obs;
pub mod ops;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the query module's first non-test callers come with rmp #47"
    )
)]
pub(crate) mod query;
pub mod storage;
mod sync;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the transaction API's first non-test callers come with Sprint 3"
    )
)]
pub mod tx;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the value types' first non-test callers come with rmp #31-#33"
    )
)]
pub(crate) mod types;

pub use error::{Error, Position};

#[cfg(test)]
mod lab;
#[cfg(test)]
mod suites;
#[cfg(test)]
mod test_support;
