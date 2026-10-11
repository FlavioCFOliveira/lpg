//! GQLSTATUS condition codes (`interfaces.md` "GQLSTATUS codes").
//!
//! Every failure the public API returns carries a five-character
//! GQLSTATUS code: a two-character class and a three-character subclass.
//! Codes come from the ISO/IEC 39075:2024 conditions insert where it
//! defines the condition; otherwise they are Neo4j's codes (rmp task #39,
//! point 5).
//!
//! * [`generated`] holds every condition of the unmodified insert
//!   `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-conditions.xml`,
//!   machine-written by the drift test in `tests.rs`, which regenerates it
//!   and fails on any difference (`LPG_UPDATE_GQLSTATUS=1` rewrites it).
//! * [`neo4j`] lists by hand the Neo4j codes lpg raises, each with the
//!   Neo4j page that defines it.
//!
//! A [`GqlStatus`] is an index into those two tables, so it is two bytes,
//! `Copy`, and a code outside them cannot be constructed.
//!
//! Copied from zu @ `a821319` (<https://github.com/tamnd/zu>, Apache-2.0,
//! see `LICENSE-APACHE` at the repository root),
//! `crates/zu-common/src/gqlstatus.rs`: `Severity`, `Condition`,
//! `GqlStatus` and the class-row convention. Adapted: the Neo4j table is
//! lpg's; zu's diagnostic record, documentation URLs and retry advice are
//! not copied.

mod generated;
pub(crate) mod neo4j;
#[cfg(test)]
pub(crate) mod tests;

use std::fmt;

/// What a condition means for the statement that raised it: the class
/// category letter of the ISO conditions insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// `S`: successful completion.
    Success,
    /// `N`: no data.
    NoData,
    /// `W`: warning.
    Warning,
    /// `I`: informational.
    Informational,
    /// `X`: exception; the statement did not succeed.
    Exception,
}

impl Severity {
    /// The insert's category letter: `S`, `N`, `W`, `I` or `X`.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Severity::Success => 'S',
            Severity::NoData => 'N',
            Severity::Warning => 'W',
            Severity::Informational => 'I',
            Severity::Exception => 'X',
        }
    }
}

/// One row of a condition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Condition {
    /// The five-character GQLSTATUS value, for example `22012`.
    pub code: &'static str,
    /// The class category.
    pub severity: Severity,
    /// The class name, for example `data exception`.
    pub class: &'static str,
    /// The subclass name, for example `division by zero`; `None` on a
    /// class row, whose code ends in `000`.
    pub subclass: Option<&'static str>,
}

/// A GQLSTATUS value: an ISO condition or one of the Neo4j codes lpg
/// raises.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GqlStatus(u16);

impl GqlStatus {
    /// Looks a code up by its five characters; `None` for a code lpg does
    /// not define.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        let iso = generated::CONDITIONS.iter().position(|c| c.code == code);
        let index = match iso {
            Some(i) => i,
            None => {
                generated::CONDITIONS.len()
                    + neo4j::CONDITIONS.iter().position(|c| c.code == code)?
            }
        };
        u16::try_from(index).ok().map(GqlStatus)
    }

    /// The row this status names.
    #[must_use]
    pub fn condition(self) -> &'static Condition {
        let i = usize::from(self.0);
        match generated::CONDITIONS.get(i) {
            Some(c) => c,
            None => &neo4j::CONDITIONS[i - generated::CONDITIONS.len()],
        }
    }

    /// The five-character code.
    #[must_use]
    pub fn code(self) -> &'static str {
        self.condition().code
    }

    /// The class category.
    #[must_use]
    pub fn severity(self) -> Severity {
        self.condition().severity
    }

    /// The two-character class, for example `42`.
    #[must_use]
    pub fn class(self) -> &'static str {
        &self.condition().code[..2]
    }

    /// Whether the code is defined by the ISO conditions insert rather
    /// than taken from Neo4j.
    #[must_use]
    pub fn is_iso(self) -> bool {
        usize::from(self.0) < generated::CONDITIONS.len()
    }

    /// The standard description: the class name, then the subclass name
    /// when there is one.
    #[must_use]
    pub fn description(self) -> String {
        let c = self.condition();
        match c.subclass {
            Some(sub) => format!("{}, {sub}", c.class),
            None => c.class.to_owned(),
        }
    }
}

impl fmt::Debug for GqlStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GqlStatus({})", self.code())
    }
}

impl fmt::Display for GqlStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One constant per condition: `C` followed by the code. The ISO codes
/// are generated; the Neo4j codes are listed in [`neo4j`].
pub mod codes {
    pub use super::generated::codes::*;
    pub use super::neo4j::codes::*;
}

/// Every ISO condition, in code order.
#[cfg(test)]
pub(crate) fn iso_conditions() -> &'static [Condition] {
    generated::CONDITIONS
}
