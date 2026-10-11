//! The typed AST of GQL programs ([TS-19]).
//!
//! Every node of `GQL.g4`'s parse is represented, each child with its
//! [`Span`](super::span::Span). Pure noise words (`PROPERTY` before
//! `GRAPH`, `BINDING` before `TABLE`, `TYPED` or `::`, an optional `AS`)
//! are not kept. Parameters stay parameters: no value is ever inlined.
//!
//! Copied from miniGU @ `fc2ccf5` (<https://github.com/TuGraph-family/miniGU>,
//! Copyright 2022 AntGroup CO., Ltd., Apache-2.0, see `LICENSE-APACHE`),
//! `minigu/gql/parser/src/ast/*.rs`: the node names and shapes of
//! programs, sessions, transactions, procedures, catalog statements,
//! queries, graph patterns, label expressions, value types and value
//! expressions. Adapted: `SmolStr` is `Box<str>`; miniGU's vector and
//! index extensions are removed (lpg has no syntax beyond `GQL.g4`); and
//! the constructs miniGU leaves out (`LET`, `FOR`, `FILTER`, `SELECT`,
//! data-modifying statements, simplified path patterns, closed graph and
//! reference types, dynamic union types, the predicates and the
//! functions of 19 and 20) are added after the same pattern.
//!
//! [TS-19]: ../../../docs/technical-spec/query-engine.md

mod expr;
mod pattern;
mod program;
mod query;
mod reference;
mod types;

pub(crate) use expr::*;
pub(crate) use pattern::*;
pub(crate) use program::*;
pub(crate) use query::*;
pub(crate) use reference::*;
pub(crate) use types::*;

/// A name: an identifier as written, or a delimited identifier with its
/// escapes resolved (IW023). Never case-folded.
pub(crate) type Ident = Box<str>;
