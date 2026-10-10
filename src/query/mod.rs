//! The GQL front end and query engine ([TS-19], [TS-20]).
//!
//! * [`lexer`]: the case-insensitive lexer of `GQL.g4`'s tokens.
//! * [`parser`]: recursive descent per `GQL.g4` rule, with a Pratt loop
//!   for value expressions, producing the typed [`ast`].
//!
//! [TS-19]: ../../docs/technical-spec/query-engine.md
//! [TS-20]: ../../docs/technical-spec/query-engine.md

pub(crate) mod ast;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod span;
