//! Test-only GQL instruments for lpg (TS-30, NFR-VER-002).
//!
//! * [`accepts`]: the parser oracle, the ANTLR parser that `generate.sh`
//!   generates from the unmodified `grammar/GQL.g4` (opengql/grammar @
//!   `16ea71b`) with the antlr4rust v0.6.0 tool jar. Default LL prediction.
//! * [`g4`]: a reader of the `GQL.g4` constructs.
//! * [`generate`]: a seeded sentence generator over [`g4::Grammar`], with a
//!   shrinker and near-miss mutants.
//!
//! The oracle code started from `docs/evidence/antlr-exp/bench/src/main.rs`
//! (lpg's own code).
//!
//! The generated parser uses `unsafe` (antlr4rust's `cast_mut`); it is
//! allowed in the three generated modules only.

#![deny(unsafe_code)]

#[allow(warnings, clippy::all, unsafe_code)]
#[rustfmt::skip]
#[path = "gen/gqllexer.rs"]
mod gqllexer;
#[allow(warnings, clippy::all, unsafe_code)]
#[rustfmt::skip]
#[path = "gen/gqllistener.rs"]
mod gqllistener;
#[allow(warnings, clippy::all, unsafe_code)]
#[rustfmt::skip]
#[path = "gen/gqlparser.rs"]
mod gqlparser;

pub mod g4;
pub mod generate;

use antlr4rust::InputStream;
use antlr4rust::common_token_stream::CommonTokenStream;
use antlr4rust::error_listener::ErrorListener;
use antlr4rust::errors::ANTLRError;
use antlr4rust::parser::Parser;
use antlr4rust::recognizer::Recognizer;
use antlr4rust::token_factory::TokenFactory;
use std::cell::RefCell;
use std::rc::Rc;

/// Collects the first syntax error reported by the lexer or the parser.
struct FirstError(Rc<RefCell<Option<String>>>);

impl<'a, T: Recognizer<'a>> ErrorListener<'a, T> for FirstError {
    fn syntax_error(
        &self,
        _recognizer: &T,
        _offending: Option<&<T::TF as TokenFactory<'a>>::Inner>,
        line: isize,
        column: isize,
        msg: &str,
        _e: Option<&ANTLRError>,
    ) {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(format!("{line}:{column} {msg}"));
        }
    }
}

/// Parses `text` from `gqlProgram` and returns the first syntax error, or
/// `None` when the oracle accepts it.
///
/// The parser runs with ANTLR's default error strategy and the default LL
/// prediction mode. Deeply nested inputs recurse deeply: callers parsing
/// arbitrary text should run this on a thread with a large stack.
#[must_use]
pub fn first_error(text: &str) -> Option<String> {
    let err = Rc::new(RefCell::new(None));
    let mut lexer = gqllexer::GQLLexer::new(InputStream::new(text));
    lexer.remove_error_listeners();
    lexer.add_error_listener(Box::new(FirstError(err.clone())));
    let mut parser = gqlparser::GQLParser::new(CommonTokenStream::new(lexer));
    parser.remove_error_listeners();
    parser.add_error_listener(Box::new(FirstError(err.clone())));
    let result = parser.gqlProgram();
    let first = err.borrow_mut().take();
    match (result, first) {
        (_, Some(e)) => Some(e),
        (Err(e), None) => Some(format!("{e:?}")),
        (Ok(_), None) => None,
    }
}

/// Whether the oracle accepts `text` as a `gqlProgram`: the parse returns
/// without error and neither the lexer nor the parser reported one.
#[must_use]
pub fn accepts(text: &str) -> bool {
    first_error(text).is_none()
}

/// The prediction mode of a freshly built parser (asserted by the tests to
/// be LL, ANTLR's default).
#[must_use]
pub fn default_prediction_mode() -> antlr4rust::PredictionMode {
    let lexer = gqllexer::GQLLexer::new(InputStream::new(""));
    let parser = gqlparser::GQLParser::new(CommonTokenStream::new(lexer));
    parser.get_interpreter().get_prediction_mode()
}
