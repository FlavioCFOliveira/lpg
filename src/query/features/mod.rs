//! Feature gates ([TS-20] Binder, FR-GQL-002).
//!
//! [`generated::FEATURES`] lists every optional feature of the ISO
//! features insert, machine-written by the drift test in `tests.rs`
//! (`LPG_UPDATE_FEATURES=1` rewrites it). [`SUPPORTED`] lists the codes
//! the user approved on 2026-10-08 (rmp task #37: "Seis + as exigidas",
//! then "Só GG03"); every other code is a gate that fails with the
//! "feature not supported" condition: GQLSTATUS 42001 with the cause
//! 51N26 naming the code (`interfaces.md`, "GQLSTATUS codes").
//!
//! The table layout and its generator are ported from zu @ `a821319`
//! (<https://github.com/tamnd/zu>, Apache-2.0, see `LICENSE-APACHE`),
//! `crates/zu-cli/src/statement/features.rs` and
//! `crates/zu-cli/tests/statement.rs`. Adapted: the generator reads lpg's
//! copy of the insert and writes through lpg's `check_generated`.
//!
//! [TS-20]: ../../../docs/technical-spec/query-engine.md

mod generated;
#[cfg(test)]
mod tests;

use crate::error::Error;
use crate::gqlstatus::codes;

/// One optional feature of the insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Feature {
    /// Its code, such as `GD01`.
    pub(crate) code: &'static str,
    /// The insert's description, whitespace folded.
    pub(crate) description: &'static str,
}

/// The supported optional features (TS-19 "Feature classification").
pub(crate) const SUPPORTED: [&str; 14] = [
    "GC01", "GC04", "GD01", "GG01", "GG02", "GG03", "GP04", "GP18", "GQ01", "GT01", "GT03", "GV39",
    "GV40", "GV41",
];

/// Every feature of the insert, in code order.
pub(crate) fn features() -> &'static [Feature] {
    generated::FEATURES
}

/// Whether `code` is a supported feature.
pub(crate) fn is_supported(code: &str) -> bool {
    SUPPORTED.contains(&code)
}

/// The "feature not supported" error of `code` (42001, cause 51N26).
pub(crate) fn unsupported(code: &str) -> Error {
    let description = features()
        .iter()
        .find(|f| f.code == code)
        .map_or("", |f| f.description);
    Error::new(
        codes::C42001,
        format!("feature {code} ({description}) is not supported"),
    )
    .caused_by(Error::new(
        codes::C51N26,
        format!("feature {code} is not supported in this version"),
    ))
}

/// Passes a supported feature; fails an unsupported one with
/// [`unsupported`].
///
/// # Errors
///
/// As above.
pub(crate) fn gate(code: &str) -> Result<(), Error> {
    if is_supported(code) {
        Ok(())
    } else {
        Err(unsupported(code))
    }
}
