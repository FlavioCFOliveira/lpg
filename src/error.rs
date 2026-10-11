//! The public error type (`interfaces.md` "Public API shape", `Error`
//! row; FR-API-001, FR-BOLT-006).
//!
//! Every failure the public API returns is one [`Error`]: a GQLSTATUS
//! code, an English message that never contains a value (IV016,
//! FR-OBS-002), an optional position in the GQL text, and an optional
//! cause, itself an [`Error`], to any depth (IA002). The shape follows
//! Bolt's `FAILURE` message, which carries `gql_status`, `message` and a
//! nested `cause`, and Neo4j's diagnostic record, whose `_position` holds
//! the offset, line and column of a syntax error.

use crate::gqlstatus::GqlStatus;
use crate::storage::error as storage;
use std::fmt;

/// A position in a GQL text.
///
/// `line` and `column` start at 1. A new line starts after U+000A only,
/// as in the ANTLR oracle (IA023); `column` counts Unicode scalar values.
/// `offset` is the byte offset of the same place, always on a character
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// Byte offset from the start of the text, from 0.
    pub offset: usize,
    /// Line, from 1.
    pub line: u32,
    /// Column in characters, from 1.
    pub column: u32,
}

impl Position {
    /// The position `offset` bytes into `text`. An offset past the end
    /// is the end of the text; an offset inside a character is the start
    /// of that character.
    ///
    /// Ported from zu @ `a821319` (Apache-2.0, see `LICENSE-APACHE`),
    /// `crates/zu-common/src/gqlstatus.rs` `Position::of`.
    #[must_use]
    pub fn of(text: &str, offset: usize) -> Self {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let mut line = 1u32;
        let mut column = 1u32;
        for ch in text[..offset].chars() {
            if ch == '\n' {
                line = line.saturating_add(1);
                column = 1;
            } else {
                column = column.saturating_add(1);
            }
        }
        Position {
            offset,
            line,
            column,
        }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// An error of the public API: a GQLSTATUS code, a message, an optional
/// position in the GQL text and an optional cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    status: GqlStatus,
    message: String,
    position: Option<Position>,
    cause: Option<Box<Error>>,
}

impl Error {
    /// An error with `status` and `message`, without position or cause.
    pub(crate) fn new(status: GqlStatus, message: impl Into<String>) -> Self {
        Error {
            status,
            message: message.into(),
            position: None,
            cause: None,
        }
    }

    /// The error with its position set.
    #[must_use]
    pub(crate) fn at(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }

    /// The error with `cause` as its cause (IA002: a cause to any depth).
    #[must_use]
    pub(crate) fn caused_by(mut self, cause: Error) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    /// The GQLSTATUS of the error.
    #[must_use]
    pub fn status(&self) -> GqlStatus {
        self.status
    }

    /// The five-character GQLSTATUS code, for example `42001`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.status.code()
    }

    /// The message: what failed, in English, without values.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The position in the GQL text the error refers to, if any.
    #[must_use]
    pub fn position(&self) -> Option<Position> {
        self.position
    }

    /// The error that caused this one, if any.
    #[must_use]
    pub fn cause(&self) -> Option<&Error> {
        self.cause.as_deref()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status.code(), self.message)?;
        if let Some(p) = self.position {
            write!(f, " (at {p})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_deref().map(|e| e as _)
    }
}

impl From<storage::Error> for Error {
    /// The error of a storage-kernel failure while the database is open
    /// (`storage::Error::gqlstatus`); the message is the kernel's.
    fn from(e: storage::Error) -> Self {
        Error::new(e.gqlstatus(), e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gqlstatus::codes;

    #[test]
    fn error_exposes_code_message_position_and_cause() {
        // No failure carries a cause yet (the binder's "feature not
        // supported" is the first, rmp #43): the field is set directly.
        let e = Error {
            cause: Some(Box::new(Error::new(codes::C51N26, "inner"))),
            ..Error::new(codes::C42001, "invalid syntax").at(Position::of("MATCH\n (n", 8))
        };
        assert_eq!(e.code(), "42001");
        assert_eq!(e.status(), codes::C42001);
        assert_eq!(e.message(), "invalid syntax");
        assert_eq!(
            e.position(),
            Some(Position {
                offset: 8,
                line: 2,
                column: 3
            })
        );
        let cause = e.cause().expect("cause");
        assert_eq!(cause.code(), "51N26");
        assert!(cause.cause().is_none());
        // `Display` names the code; `source` is the cause.
        assert_eq!(e.to_string(), "42001: invalid syntax (at line 2, column 3)");
        let source = std::error::Error::source(&e).expect("source");
        // Without a position, `Display` is the code and the message.
        assert_eq!(source.to_string(), "51N26: inner");
    }

    #[test]
    fn position_counts_lines_at_line_feed_only_and_columns_in_characters() {
        let text = "a\r\nλb\ncd";
        assert_eq!(
            Position::of(text, 0),
            Position {
                offset: 0,
                line: 1,
                column: 1
            }
        );
        // U+000D does not start a line (IA023).
        assert_eq!(
            Position::of(text, 2),
            Position {
                offset: 2,
                line: 1,
                column: 3
            }
        );
        // `λ` is two bytes and one column.
        assert_eq!(
            Position::of(text, 5),
            Position {
                offset: 5,
                line: 2,
                column: 2
            }
        );
        // Inside a character: its start. Past the end: the end.
        assert_eq!(Position::of(text, 4).offset, 3);
        assert_eq!(
            Position::of(text, 99),
            Position {
                offset: 9,
                line: 3,
                column: 3
            }
        );
    }

    #[test]
    fn a_storage_error_keeps_its_code_and_message() {
        let e = Error::from(storage::Error::Serialization);
        assert_eq!(e.code(), "25N11");
        assert_eq!(e.message(), storage::Error::Serialization.to_string());
        assert_eq!(Error::from(storage::Error::Deadlock).code(), "50N05");
    }
}
