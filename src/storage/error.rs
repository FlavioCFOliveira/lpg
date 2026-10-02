//! Storage kernel errors and the database-wide "unusable" latch (TS-12).

use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

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

    #[test]
    fn only_storage_full_is_no_space() {
        assert!(Error::Io(io::ErrorKind::StorageFull.into()).is_no_space());
        assert!(!Error::Io(io::ErrorKind::Other.into()).is_no_space());
        assert!(!Error::Unusable.is_no_space());
    }
}
