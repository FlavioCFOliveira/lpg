//! Storage kernel errors and the database-wide "unusable" latch (TS-12).

use crate::gqlstatus::{GqlStatus, codes};
use crate::sync::atomic::{AtomicBool, Ordering};
use std::fmt;
use std::io;

/// An error of the storage kernel.
#[derive(Debug)]
pub(crate) enum Error {
    /// An I/O error of the storage implementation. A full device is
    /// [`io::ErrorKind::StorageFull`].
    Io(io::Error),
    /// A checksum or format check failed (FR-REC-003, FR-REC-005).
    Corruption(String),
    /// Create found a database at the location (FR-DB-003).
    DatabaseExists,
    /// Open found no database at the location (FR-DB-004).
    NoDatabase,
    /// The database is open, in this process or another (FR-DB-006,
    /// FR-DB-007).
    Locked,
    /// `CONTROL` exists but `LOCK` does not; open recreates nothing, since a
    /// new `LOCK` would let a second handle open the database (TS-27).
    LockMissing,
    /// The database has an older format version; an explicit migration is
    /// required (TS-28, FR-OPS-004).
    MigrationRequired {
        /// Format version found in `CONTROL`.
        found: u32,
        /// Format version this crate requires.
        required: u32,
    },
    /// The database has a newer format version than this crate supports
    /// (TS-28, FR-OPS-013).
    NewerFormat {
        /// Format version found in `CONTROL`.
        found: u32,
        /// Newest format version this crate supports.
        supported: u32,
    },
    /// An earlier checksum or I/O failure made the database unusable until
    /// it is reopened (TS-12, FR-REC-005).
    Unusable,
    /// A memory reservation did not fit the memory cap (TS-08, FR-LIM-001).
    MemoryLimit {
        /// Bytes requested.
        requested: u64,
    },
    /// An argument is outside its allowed range.
    InvalidArgument(&'static str),
    /// A backup artefact has no valid `MANIFEST`: the backup did not
    /// complete (TS-26 restore step 2).
    IncompleteBackup(String),
    /// A backup was requested while a fast initial load runs its bulk
    /// build, whose pages bypass the WAL (TS-26, FR-OPS-001).
    LoadRunning,
    /// Migration needs a clean shutdown: the database must be opened and
    /// closed with the crate version that wrote it (TS-28 step 2).
    NotCleanlyClosed {
        /// Crate version to open and close the database with.
        crate_version: &'static str,
    },
    /// The format is more than one version older than this crate's: an
    /// intermediate crate version must migrate it first (TS-28,
    /// FR-OPS-012).
    MigrationPath {
        /// Format version found.
        found: u32,
        /// The crate version that migrates format `found` to the next one.
        crate_version: Option<&'static str>,
    },
    /// The transaction cannot be serialised with the transactions that
    /// committed while it ran: its reads overlap their writes (TS-15
    /// Validation), or a resource it locked changed after its snapshot
    /// (TS-15 Write-write rule). The transaction is aborted. GQLSTATUS
    /// 25N11 ([`Error::gqlstatus`]; FR-TX-008).
    Serialization,
    /// The transaction's lock request closed a cycle of waiting
    /// transactions; it is aborted (TS-15 Deadlocks). GQLSTATUS 50N05
    /// (FR-TX-007).
    Deadlock,
    /// A transaction used a tree that was dropped at or before its
    /// snapshot.
    TreeDropped(u64),
    /// An operation of a transaction that an earlier failure aborted.
    Aborted,
    /// A catalog object or an element that the operation needs is not
    /// visible to the transaction.
    NotFound(Missing),
    /// The catalog already holds an object of that name in that parent
    /// (TS-17).
    CatalogObjectExists,
    /// A directory or schema to drop still holds an object.
    NotEmpty,
    /// A graph type to drop is the type of a graph: GQLSTATUS class G1,
    /// "dependent object error" (ISO/IEC 39075 conditions insert).
    GraphTypeInUse,
    /// A node to delete still has edges: GQLSTATUS G1001, "edges still
    /// exist" (ISO/IEC 39075 conditions insert).
    EdgesStillExist,
    /// A write, a commit or a constraint creation met data that violates
    /// a constraint (TS-18, FR-IDX-002, FR-IDX-004).
    ConstraintViolation(Violation),
}

/// What [`Error::NotFound`] did not find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Missing {
    /// A graph.
    Graph,
    /// A graph type.
    GraphType,
    /// The parent directory or schema of a catalog object.
    ParentObject,
    /// A node.
    Node,
    /// An edge.
    Edge,
    /// An endpoint node of an edge.
    EndpointNode,
    /// An index that is POPULATING.
    PopulatingIndex,
    /// An index that is ONLINE.
    OnlineIndex,
}

impl Missing {
    /// The text that names the missing object.
    fn text(self) -> &'static str {
        match self {
            Missing::Graph => "the graph",
            Missing::GraphType => "the graph type",
            Missing::ParentObject => "the parent catalog object",
            Missing::Node => "the node",
            Missing::Edge => "the edge",
            Missing::EndpointNode => "an endpoint node",
            Missing::PopulatingIndex => "a POPULATING index",
            Missing::OnlineIndex => "an ONLINE index",
        }
    }
}

/// The rule an [`Error::ConstraintViolation`] broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Violation {
    /// A uniqueness or node-key constraint being created finds two
    /// elements with one value.
    DuplicateValues,
    /// A value of a uniqueness or node-key constraint is in use.
    ValueInUse,
    /// An element lacks a property its constraint requires.
    MissingProperty,
    /// A property has another type than its constraint requires.
    PropertyType,
}

impl Violation {
    /// The text that names the rule.
    fn text(self) -> &'static str {
        match self {
            Violation::DuplicateValues => "two elements hold one value of the new constraint",
            Violation::ValueInUse => "a value of a uniqueness or node-key constraint is in use",
            Violation::MissingProperty => "an element lacks a property its constraint requires",
            Violation::PropertyType => "a property has another type than its constraint requires",
        }
    }
}

/// Result of the storage kernel.
pub(crate) type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Corruption(what) => write!(f, "corruption detected: {what}"),
            Error::DatabaseExists => f.write_str("a database already exists at the location"),
            Error::NoDatabase => f.write_str("no database at the location"),
            Error::Locked => f.write_str("the database is already open"),
            Error::LockMissing => f.write_str(
                "the lock file LOCK is missing from the database location; \
                 open creates no file",
            ),
            Error::MigrationRequired { found, required } => write!(
                f,
                "migration required: the database has format version {found}, \
                 this crate requires format version {required}"
            ),
            Error::NewerFormat { found, supported } => write!(
                f,
                "the database has format version {found}, newer than the newest \
                 supported format version {supported}"
            ),
            Error::Unusable => {
                f.write_str("the database is unusable after an earlier failure; reopen it")
            }
            Error::MemoryLimit { requested } => {
                write!(
                    f,
                    "memory cap reached: {requested} bytes could not be reserved"
                )
            }
            Error::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Error::IncompleteBackup(what) => {
                write!(f, "the backup is incomplete or damaged: {what}")
            }
            Error::LoadRunning => {
                f.write_str("a fast initial load is running; a backup cannot run until it ends")
            }
            Error::NotCleanlyClosed { crate_version } => write!(
                f,
                "the database was not closed cleanly; open and close it with lpg \
                 {crate_version} before migrating it"
            ),
            Error::MigrationPath {
                found,
                crate_version: Some(v),
            } => write!(
                f,
                "format version {found} is too old to migrate directly; migrate it with \
                 lpg {v} first"
            ),
            Error::MigrationPath {
                found,
                crate_version: None,
            } => write!(
                f,
                "format version {found} is too old to migrate and no crate version is known \
                 that migrates it"
            ),
            Error::Serialization => f.write_str(
                "the transaction could not be serialized with concurrent transactions \
                 and was aborted",
            ),
            Error::Deadlock => {
                f.write_str("the transaction was aborted to break a deadlock between lock waits")
            }
            Error::TreeDropped(id) => write!(f, "tree {id} was dropped"),
            Error::Aborted => f.write_str("the transaction was aborted by an earlier failure"),
            Error::NotFound(what) => write!(f, "{} does not exist", what.text()),
            Error::CatalogObjectExists => {
                f.write_str("a catalog object of that name already exists in that parent")
            }
            Error::NotEmpty => f.write_str("the directory or schema still holds an object"),
            Error::GraphTypeInUse => f.write_str("a graph still uses the graph type"),
            Error::EdgesStillExist => f.write_str("the node still has edges"),
            Error::ConstraintViolation(rule) => write!(f, "constraint violation: {}", rule.text()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl Error {
    /// Whether this is a reservation ENOSPC, the one I/O failure that leaves
    /// the database usable (TS-05, TS-12).
    pub(crate) fn is_no_space(&self) -> bool {
        matches!(self, Error::Io(e) if e.kind() == io::ErrorKind::StorageFull)
    }

    /// The GQLSTATUS code of the error while the database is open
    /// (`interfaces.md` "GQLSTATUS codes"; [`Error::open_gqlstatus`] for
    /// a failing open). The `match` is exhaustive, so a new kind cannot
    /// lack a code.
    pub(crate) fn gqlstatus(&self) -> GqlStatus {
        match self {
            Error::Io(_) | Error::TreeDropped(_) => codes::C50N00,
            Error::Corruption(_) | Error::Unusable => codes::C08N09,
            Error::DatabaseExists | Error::CatalogObjectExists => codes::C42N11,
            Error::NoDatabase => codes::C22N51,
            Error::Locked
            | Error::LockMissing
            | Error::MigrationRequired { .. }
            | Error::NewerFormat { .. } => codes::C51N40,
            Error::MemoryLimit { .. } => codes::C51N72,
            Error::InvalidArgument(_) => codes::C22N11,
            Error::IncompleteBackup(_)
            | Error::LoadRunning
            | Error::NotCleanlyClosed { .. }
            | Error::MigrationPath { .. } => codes::C51N41,
            Error::Serialization => codes::C25N11,
            Error::Deadlock => codes::C50N05,
            Error::Aborted => codes::C25N05,
            Error::NotFound(missing) => match missing {
                Missing::Graph | Missing::GraphType | Missing::ParentObject => codes::C22N51,
                Missing::PopulatingIndex | Missing::OnlineIndex => codes::C22N69,
                // The executor addresses only elements its own snapshot
                // read, so a missing element is an internal failure
                // (spec-alignment item: `interfaces.md` names no code).
                Missing::Node | Missing::Edge | Missing::EndpointNode => codes::C50N00,
            },
            Error::NotEmpty | Error::GraphTypeInUse => codes::CG1000,
            Error::EdgesStillExist => codes::CG1001,
            Error::ConstraintViolation(rule) => match rule {
                Violation::DuplicateValues | Violation::ValueInUse => codes::C22N79,
                Violation::MissingProperty => codes::C22N77,
                Violation::PropertyType => codes::C22N78,
            },
        }
    }

    /// The GQLSTATUS code of the error when a database open fails: as
    /// [`Error::gqlstatus`], except that corruption found at open is
    /// 51N40 "unable to start database" (`interfaces.md`).
    pub(crate) fn open_gqlstatus(&self) -> GqlStatus {
        match self {
            Error::Corruption(_) => codes::C51N40,
            other => other.gqlstatus(),
        }
    }
}

/// The database-wide "unusable" latch (TS-12).
///
/// Any component that sees a checksum failure, or a failed read, write or
/// sync other than a reservation ENOSPC, sets it; from then on every
/// operation fails with [`Error::Unusable`] until the database is reopened.
/// A new open builds a new latch.
#[derive(Debug, Default)]
pub(crate) struct Health {
    unusable: AtomicBool,
}

impl Health {
    /// Fails with [`Error::Unusable`] if the latch is set.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] after any [`Health::fail`].
    #[inline]
    pub(crate) fn check(&self) -> Result<()> {
        // Acquire pairs with the Release in `fail`: an operation that sees
        // the latch clear also sees every write made before an earlier
        // success.
        if self.unusable.load(Ordering::Acquire) {
            Err(Error::Unusable)
        } else {
            Ok(())
        }
    }

    /// Sets the latch and returns `err` for the failing operation.
    pub(crate) fn fail(&self, err: Error) -> Error {
        self.unusable.store(true, Ordering::Release);
        err
    }

    /// Whether the latch is set.
    pub(crate) fn is_unusable(&self) -> bool {
        self.unusable.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_fails_every_later_check() {
        let h = Health::default();
        h.check().unwrap();
        let e = h.fail(Error::Corruption("x".into()));
        assert!(matches!(e, Error::Corruption(_)));
        assert!(matches!(h.check(), Err(Error::Unusable)));
        assert!(h.is_unusable());
    }

    /// Every kind of storage error and its GQLSTATUS code
    /// (`interfaces.md` "GQLSTATUS codes"). Adding a kind breaks the
    /// exhaustive `match` of `gqlstatus`; this list makes each code
    /// explicit.
    #[test]
    fn every_kind_maps_to_its_code() {
        let kinds: Vec<(Error, &str)> = vec![
            (Error::Io(io::ErrorKind::Other.into()), "50N00"),
            (Error::Corruption("x".into()), "08N09"),
            (Error::DatabaseExists, "42N11"),
            (Error::NoDatabase, "22N51"),
            (Error::Locked, "51N40"),
            (Error::LockMissing, "51N40"),
            (
                Error::MigrationRequired {
                    found: 1,
                    required: 2,
                },
                "51N40",
            ),
            (
                Error::NewerFormat {
                    found: 3,
                    supported: 2,
                },
                "51N40",
            ),
            (Error::Unusable, "08N09"),
            (Error::MemoryLimit { requested: 1 }, "51N72"),
            (Error::InvalidArgument("x"), "22N11"),
            (Error::IncompleteBackup("x".into()), "51N41"),
            (Error::LoadRunning, "51N41"),
            (
                Error::NotCleanlyClosed {
                    crate_version: "0.1.0",
                },
                "51N41",
            ),
            (
                Error::MigrationPath {
                    found: 1,
                    crate_version: None,
                },
                "51N41",
            ),
            (Error::Serialization, "25N11"),
            (Error::Deadlock, "50N05"),
            (Error::TreeDropped(1), "50N00"),
            (Error::Aborted, "25N05"),
            (Error::NotFound(Missing::Graph), "22N51"),
            (Error::NotFound(Missing::GraphType), "22N51"),
            (Error::NotFound(Missing::ParentObject), "22N51"),
            (Error::NotFound(Missing::Node), "50N00"),
            (Error::NotFound(Missing::Edge), "50N00"),
            (Error::NotFound(Missing::EndpointNode), "50N00"),
            (Error::NotFound(Missing::PopulatingIndex), "22N69"),
            (Error::NotFound(Missing::OnlineIndex), "22N69"),
            (Error::CatalogObjectExists, "42N11"),
            (Error::NotEmpty, "G1000"),
            (Error::GraphTypeInUse, "G1000"),
            (Error::EdgesStillExist, "G1001"),
            (
                Error::ConstraintViolation(Violation::DuplicateValues),
                "22N79",
            ),
            (Error::ConstraintViolation(Violation::ValueInUse), "22N79"),
            (
                Error::ConstraintViolation(Violation::MissingProperty),
                "22N77",
            ),
            (Error::ConstraintViolation(Violation::PropertyType), "22N78"),
        ];
        for (kind, code) in &kinds {
            assert_eq!(kind.gqlstatus().code(), *code, "{kind:?}");
            // Every message names what failed, without the code.
            assert!(!kind.to_string().is_empty());
        }
        // Each top-level kind appears: 25 kinds, with the sub-kinds of
        // `NotFound` and `ConstraintViolation` listed separately.
        let mut names: Vec<String> = kinds
            .iter()
            .map(|(k, _)| {
                format!("{k:?}")
                    .split(['(', ' '])
                    .next()
                    .unwrap_or("")
                    .to_owned()
            })
            .collect();
        names.dedup();
        assert_eq!(names.len(), 25);
        // The two codes of Sprint 2 are unchanged (TS-15).
        assert_eq!(Error::Serialization.gqlstatus().code(), "25N11");
        assert_eq!(Error::Deadlock.gqlstatus().code(), "50N05");
        // An open reports corruption as "unable to start database".
        assert_eq!(
            Error::Corruption("x".into()).open_gqlstatus().code(),
            "51N40"
        );
        assert_eq!(Error::Locked.open_gqlstatus().code(), "51N40");
    }

    #[test]
    fn only_storage_full_is_no_space() {
        assert!(Error::Io(io::ErrorKind::StorageFull.into()).is_no_space());
        assert!(!Error::Io(io::ErrorKind::Other.into()).is_no_space());
        assert!(!Error::Unusable.is_no_space());
    }
}
