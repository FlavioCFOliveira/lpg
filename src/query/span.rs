// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.
//
// NOTICE of the source project:
//   Apache DataFusion sqlparser-rs
//   Copyright 2024-2026 The Apache Software Foundation
//   This product includes software developed at
//   The Apache Software Foundation (http://www.apache.org/).

//! Source locations and spans of GQL text.
//!
//! [`Location`] and [`Span`] are copied from datafusion-sqlparser-rs @
//! `14cbf75` (<https://github.com/apache/datafusion-sqlparser-rs>,
//! Apache-2.0, see `LICENSE-APACHE`), `src/tokenizer.rs:546-708`, with its
//! documentation shortened; adapted: the `offset` field (a byte offset
//! from the start of the text) and `u32` line and column. [`Spanned`] is
//! copied from miniGU @ `fc2ccf5` (<https://github.com/TuGraph-family/miniGU>,
//! Copyright 2022 AntGroup CO., Ltd., Apache-2.0),
//! `minigu/gql/parser/src/span.rs`; adapted to hold a [`Span`] instead
//! of a byte range.

use std::cmp;
use std::fmt;

/// A location in the text: line and column from 1, and the byte offset
/// from 0. Lines start after U+000A only; columns count characters.
#[derive(Eq, PartialEq, Hash, Clone, Copy, Ord, PartialOrd, Default)]
pub(crate) struct Location {
    /// Byte offset from the start of the text.
    pub(crate) offset: u32,
    /// Line number, from 1. Line 0 marks an empty location.
    pub(crate) line: u32,
    /// Column, from 1. Column 0 marks an empty location.
    pub(crate) column: u32,
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Location({},{})", self.line, self.column)
    }
}

/// A part of the text, from `start` (inclusive) to `end` (exclusive).
#[derive(Eq, PartialEq, Hash, Clone, PartialOrd, Ord, Copy, Default)]
pub(crate) struct Span {
    /// Start location (inclusive).
    pub(crate) start: Location,
    /// End location: the location just after the last character.
    pub(crate) end: Location,
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Span({:?}..{:?})", self.start, self.end)
    }
}

impl Span {
    /// An empty span: no knowledge of the location.
    const EMPTY: Span = Span {
        start: Location {
            offset: 0,
            line: 0,
            column: 0,
        },
        end: Location {
            offset: 0,
            line: 0,
            column: 0,
        },
    };

    /// The span from `start` to `end`.
    pub(crate) fn new(start: Location, end: Location) -> Span {
        Span { start, end }
    }

    /// The smallest span that contains `self` and `other`; an empty span
    /// yields the other one.
    pub(crate) fn union(&self, other: &Span) -> Span {
        match (self, other) {
            (&Span::EMPTY, _) => *other,
            (_, &Span::EMPTY) => *self,
            _ => Span {
                start: cmp::min(self.start, other.start),
                end: cmp::max(self.end, other.end),
            },
        }
    }
}

/// A value with the span of the text it was parsed from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Spanned<T>(pub(crate) T, pub(crate) Span);

/// A list of spanned values.
pub(crate) type VecSpanned<T> = Vec<Spanned<T>>;
/// A boxed spanned value.
pub(crate) type BoxSpanned<T> = Box<Spanned<T>>;
/// An optional spanned value.
pub(crate) type OptSpanned<T> = Option<Spanned<T>>;
