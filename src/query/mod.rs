//! The GQL front end and query engine ([TS-19], [TS-20]).
//!
//! * [`lexer`]: the case-insensitive lexer of `GQL.g4`'s tokens.
//! * [`parser`]: recursive descent per `GQL.g4` rule, with a Pratt loop
//!   for value expressions, producing the typed [`ast`].
//! * [`binder`]: the AST to the bound form ([`bound`]), with scopes,
//!   types, path-pattern rules and feature gates ([`features`]).
//! * [`expr`], [`functions`], [`aggregate`]: value expressions, the
//!   function table and the aggregate accumulators.
//! * [`plan`]: index selection, join order, EXPLAIN.
//! * [`exec`]: the pull-based batch executor and PROFILE, with the
//!   internal entry points `run`, `profile` and `explain`.
//! * [`graph_type`]: closed graph types.
//!
//! [TS-19]: ../../docs/technical-spec/query-engine.md
//! [TS-20]: ../../docs/technical-spec/query-engine.md

pub(crate) mod aggregate;
pub(crate) mod ast;
pub(crate) mod binder;
pub(crate) mod bound;
pub(crate) mod exec;
pub(crate) mod expr;
pub(crate) mod features;
pub(crate) mod functions;
pub(crate) mod graph_type;
pub(crate) mod lexer;
pub(crate) mod parser;
pub(crate) mod plan;
pub(crate) mod span;
pub(crate) mod unicode;
