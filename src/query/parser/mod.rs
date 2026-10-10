//! The GQL parser ([TS-19]): recursive descent, one function per
//! `GQL.g4` rule (opengql/grammar @ `16ea71b`), named after the rule, with
//! a Pratt loop for the left-recursive expression rules.
//!
//! **Acceptance.** It accepts exactly the texts the ANTLR oracle generated
//! from the same grammar accepts (NFR-VER-002). Where `GQL.g4` is not
//! LL(1) the functions look further ahead, or try an alternative and fall
//! back, and every ambiguous decision takes its lowest-numbered
//! alternative, as the oracle does (TS-19 "Ambiguities"). The decisions
//! that need care are named in the function that makes them.
//!
//! **Errors.** A text the grammar rejects fails with GQLSTATUS 42001, the
//! position of the furthest token any alternative reached, and the
//! tokens that would have been accepted there.
//!
//! **Nesting.** Every recursive rule, and every operator of a left-deep
//! chain (`a + b + c`, `a.b.c`, label and type unions, query
//! conjunctions), takes one level of [`MAX_DEPTH`], so no AST the parser
//! returns is deeper than that, and code that walks it recursively
//! (`Drop` included) cannot overflow the stack. The parse itself also
//! stops when its stack use passes [`STACK_BUDGET`], measured from the
//! address of a local variable as PostgreSQL's `stack_is_too_deep` does
//! (`src/backend/utils/misc/stack_depth.c` @ `1b5dd3a`): a level costs
//! from a few hundred bytes to about 15 KiB of stack depending on the
//! rule, so a count alone would either overflow on the costly rules or
//! refuse ordinary texts. Both limits fail with 42001.
//!
//! Rule functions are ported from miniGU @ `fc2ccf5`
//! (<https://github.com/TuGraph-family/miniGU>, Copyright 2022 AntGroup
//! CO., Ltd., Apache-2.0, see `LICENSE-APACHE`),
//! `minigu/gql/parser/src/parser/impls/*.rs`, from `winnow` combinators
//! to plain recursive descent; the rules miniGU lacks are written after
//! the same pattern. The depth guard is copied from
//! datafusion-sqlparser-rs @ `14cbf75` (Apache-2.0)
//! `src/parser/mod.rs:95-140`, and the precedence loop of [`expr`]
//! follows its `parse_subexpr` (`:1424-1460`).
//!
//! [TS-19]: ../../../docs/technical-spec/query-engine.md

mod catalog;
#[cfg(test)]
mod differential;
mod expr;
mod pattern;
mod program;
mod query;
#[cfg(test)]
mod tests;
mod types;

use super::ast::{Ident, Program};
use super::lexer::{Keyword, Token, TokenKind, tokenize};
use super::span::{Location, Span, Spanned};
use crate::error::{Error, Position};
use crate::gqlstatus::codes;
use recursion::{DepthGuard, RecursionCounter};
use std::collections::BTreeSet;

/// The deepest nesting the parser accepts, in levels: one per recursive
/// rule entered and one per operator of a left-deep chain. It bounds the
/// depth of the AST.
pub(crate) const MAX_DEPTH: usize = 256;

/// The most stack the parse may use, in bytes, measured from the frame
/// of [`parse_program`]. The parser runs on tokio's blocking threads
/// (TS-02), whose default stack is 2 MiB; the rest is left to the
/// caller and to the frames between two checks. `tests.rs`
/// (`deep_texts_fail_with_42001_on_a_2_mib_stack`) runs every recursive
/// shape on a 2 MiB stack.
pub(crate) const STACK_BUDGET: usize = 1 << 20;

/// The address of a local variable of the calling frame: a measure of
/// the stack in use.
#[inline(never)]
fn stack_address() -> usize {
    let marker = 0u8;
    std::ptr::from_ref(std::hint::black_box(&marker)).addr()
}

/// Parses `text` as a `gqlProgram`.
///
/// # Errors
///
/// GQLSTATUS 42001 with the position and the expected tokens when
/// `GQL.g4` does not accept `text` (BR-GQL-001), or when it nests deeper
/// than [`MAX_DEPTH`].
pub(crate) fn parse_program(text: &str) -> Result<Program, Error> {
    let tokens = match tokenize(text) {
        Ok(t) => t,
        Err(e) => {
            let c = text[e.at.offset as usize..].chars().next().unwrap_or(' ');
            return Err(Error::new(
                codes::C42001,
                format!(
                    "invalid syntax: no token can start with the character U+{:04X}",
                    u32::from(c)
                ),
            )
            .at(position(e.at)));
        }
    };
    let stack_base = stack_address();
    let mut p = Parser::new(text, &tokens, stack_base);
    let error = match p.gql_program() {
        Ok(program) => return Ok(program),
        Err(Fail) => p.error(),
    };
    if p.too_deep.is_some() {
        return Err(error);
    }
    // A decision taken on local evidence may have closed the only path
    // to a parse. Retry with one logged decision changed to another of
    // its alternatives, latest decision first; keep the change that lets
    // the parse reach furthest and repeat, within a budget of parses.
    let mut forced: Vec<Forced> = Vec::new();
    let mut last = (p.furthest, std::mem::take(&mut p.decisions));
    let mut budget = MAX_RETRY_PARSES;
    loop {
        let (furthest, mut decisions) = last;
        decisions.sort_by_key(|d| (std::cmp::Reverse(d.pos), d.site));
        decisions.dedup_by_key(|d| (d.pos, d.site));
        let mut best: Option<(Forced, usize, Vec<Decision>)> = None;
        'candidates: for d in &decisions {
            if forced.iter().any(|f| (f.0, f.1) == (d.pos, d.site)) {
                continue;
            }
            for alternative in (0..d.count).filter(|&a| a != d.chosen) {
                if budget == 0 {
                    break 'candidates;
                }
                budget -= 1;
                let mut q = Parser::new(text, &tokens, stack_base);
                q.forced.clone_from(&forced);
                q.forced.push((d.pos, d.site, alternative));
                match q.gql_program() {
                    Ok(program) => return Ok(program),
                    Err(Fail) if q.furthest > best.as_ref().map_or(furthest, |b| b.1) => {
                        best = Some(((d.pos, d.site, alternative), q.furthest, q.decisions));
                    }
                    Err(Fail) => {}
                }
            }
        }
        match best {
            Some((flip, reached, decisions)) => {
                forced.push(flip);
                last = (reached, decisions);
            }
            None => return Err(error),
        }
    }
}

/// A forced decision: the token, the [`Site`] and the alternative.
type Forced = (usize, u8, usize);

/// How many parses a failed parse may retry, in all.
const MAX_RETRY_PARSES: usize = 256;

/// A decision taken on local evidence, which a failed parse retries.
#[derive(Debug, Clone, Copy)]
pub(super) struct Decision {
    /// The token where it was taken.
    pos: usize,
    /// Which kind of decision ([`Site`]).
    site: u8,
    /// The alternative taken.
    chosen: usize,
    /// The number of alternatives.
    count: usize,
}

/// The kinds of retried decisions.
pub(super) struct Site;

impl Site {
    /// The split of a catalog path among its matches.
    pub(super) const PATH: u8 = 1;
    /// `GRAPH`/`TABLE` as a keyword or as a variable in an expression.
    pub(super) const KEYWORD_OR_VARIABLE: u8 = 2;
    /// `TYPE` as a keyword or as a name after a node or edge synonym.
    pub(super) const TYPE_KEYWORD: u8 = 3;
    /// `LABEL`/`LABELS` and a name as a label set phrase, or as a name.
    pub(super) const LABEL_PHRASE: u8 = 4;
    /// `<` after `ANY`, `LIST` or `ARRAY` in a type: the type's own
    /// bracket, or a comparison after the type.
    pub(super) const TYPE_ANGLE: u8 = 5;
    /// `NODE`/`EDGE` and a filler: a closed element type, or the open
    /// type followed by something else.
    pub(super) const ELEMENT_TYPE: u8 = 6;
}

fn position(at: Location) -> Position {
    Position {
        offset: at.offset as usize,
        line: at.line,
        column: at.column,
    }
}

/// A failed parse: the reason is in the parser's expected-token record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fail;

/// The result of a rule function.
pub(super) type PResult<T> = Result<T, Fail>;

/// What the parser expected at a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Expected {
    /// A token kind.
    Token(TokenKind),
    /// A described construct.
    Rule(&'static str),
}

/// The parser state.
pub(super) struct Parser<'a> {
    text: &'a str,
    tokens: &'a [Token],
    pos: usize,
    /// The furthest token index at which an alternative failed.
    furthest: usize,
    /// What was expected at `furthest`.
    expected: BTreeSet<Expected>,
    depth: RecursionCounter,
    /// [`stack_address`] in [`parse_program`].
    stack_base: usize,
    /// Set when the nesting limit is reached: every alternative fails
    /// from then on.
    too_deep: Option<usize>,
    invalid_escapes: Vec<Span>,
    /// The retried decisions taken.
    decisions: Vec<Decision>,
    /// Decisions to force: token, site, alternative.
    forced: Vec<Forced>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, tokens: &'a [Token], stack_base: usize) -> Self {
        Parser {
            text,
            tokens,
            pos: 0,
            furthest: 0,
            expected: BTreeSet::new(),
            depth: RecursionCounter::new(MAX_DEPTH),
            stack_base,
            too_deep: None,
            invalid_escapes: Vec::new(),
            decisions: Vec::new(),
            forced: Vec::new(),
        }
    }

    /// Takes a decision among `count` alternatives at the current token:
    /// the first, or the one a retry forces. Logged when there is a
    /// choice.
    pub(super) fn decide(&mut self, site: u8, count: usize) -> usize {
        let chosen = self
            .forced
            .iter()
            .find(|&&(pos, s, a)| pos == self.pos && s == site && a < count)
            .map_or(0, |f| f.2);
        if count > 1 {
            self.decisions.push(Decision {
                pos: self.pos,
                site,
                chosen,
                count,
            });
        }
        chosen
    }

    /// The 42001 error of a failed parse.
    fn error(&self) -> Error {
        if let Some(at) = self.too_deep {
            return Error::new(
                codes::C42001,
                format!(
                    "invalid syntax: the text nests too deeply (more than {MAX_DEPTH} levels \
                     or {} KiB of stack)",
                    STACK_BUDGET >> 10
                ),
            )
            .at(position(self.token_at(at).span.start));
        }
        let token = self.token_at(self.furthest);
        let found = match token.kind {
            TokenKind::Eof => "the end of the text".to_owned(),
            _ => format!("'{}'", token.text(self.text)),
        };
        let mut expected: Vec<&str> = self
            .expected
            .iter()
            .map(|e| match e {
                Expected::Token(k) => k.describe(),
                Expected::Rule(r) => r,
            })
            .collect();
        expected.dedup();
        Error::new(
            codes::C42001,
            format!(
                "invalid syntax: unexpected {found}; expected {}",
                expected.join(", ")
            ),
        )
        .at(position(token.span.start))
    }

    // ---- Tokens ------------------------------------------------------

    fn token_at(&self, i: usize) -> Token {
        let last = self.tokens.len() - 1;
        self.tokens[i.min(last)]
    }

    /// The kind of the current token.
    fn peek(&self) -> TokenKind {
        self.token_at(self.pos).kind
    }

    /// The kind of the token `n` places after the current one.
    fn peek_nth(&self, n: usize) -> TokenKind {
        self.token_at(self.pos + n).kind
    }

    /// Records that `e` was expected at the current token.
    fn expect_here(&mut self, e: Expected) {
        if self.pos > self.furthest {
            self.furthest = self.pos;
            self.expected.clear();
        }
        if self.pos == self.furthest {
            self.expected.insert(e);
        }
    }

    /// Fails, recording `e` as expected at the current token.
    fn fail<T>(&mut self, e: Expected) -> PResult<T> {
        self.expect_here(e);
        Err(Fail)
    }

    /// Fails with the construct `what` expected here.
    fn fail_rule<T>(&mut self, what: &'static str) -> PResult<T> {
        self.fail(Expected::Rule(what))
    }

    /// Whether the current token is `kind`; records it as expected if
    /// not.
    fn at(&mut self, kind: TokenKind) -> bool {
        if self.peek() == kind {
            true
        } else {
            self.expect_here(Expected::Token(kind));
            false
        }
    }

    /// Whether the current token is the keyword `k`.
    fn at_kw(&mut self, k: Keyword) -> bool {
        self.at(TokenKind::Keyword(k))
    }

    /// Consumes the current token if it is `kind`.
    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Consumes the current token if it is the keyword `k`.
    fn eat_kw(&mut self, k: Keyword) -> bool {
        self.eat(TokenKind::Keyword(k))
    }

    /// Consumes a token of `kind` or fails.
    fn expect(&mut self, kind: TokenKind) -> PResult<Span> {
        if self.at(kind) {
            self.pos += 1;
            Ok(self.token_at(self.pos - 1).span)
        } else {
            Err(Fail)
        }
    }

    /// Consumes the keyword `k` or fails.
    fn expect_kw(&mut self, k: Keyword) -> PResult<Span> {
        self.expect(TokenKind::Keyword(k))
    }

    /// Consumes and returns the current token.
    fn bump(&mut self) -> Token {
        let t = self.token_at(self.pos);
        self.pos += 1;
        t
    }

    /// The start of the current token.
    fn start(&self) -> Location {
        self.token_at(self.pos).span.start
    }

    /// The span from `start` to the end of the last consumed token.
    fn span_from(&self, start: Location) -> Span {
        let end = if self.pos == 0 {
            start
        } else {
            self.token_at(self.pos - 1).span.end
        };
        Span::new(start, end.max(start))
    }

    /// `value` with the span from `start`.
    fn spanned<T>(&self, start: Location, value: T) -> Spanned<T> {
        Spanned(value, self.span_from(start))
    }

    // ---- Backtracking ------------------------------------------------

    /// Runs `f`; on failure restores the position and returns `None`.
    /// The nesting limit is not backtracked over.
    fn try_parse<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<Option<T>> {
        let saved = self.pos;
        let escapes = self.invalid_escapes.len();
        match f(self) {
            Ok(v) => Ok(Some(v)),
            Err(Fail) if self.too_deep.is_some() => Err(Fail),
            Err(Fail) => {
                self.pos = saved;
                self.invalid_escapes.truncate(escapes);
                Ok(None)
            }
        }
    }

    /// Enters one level of nesting, or fails at [`MAX_DEPTH`] or when
    /// the stack in use passes [`STACK_BUDGET`]. Stacks grow down on the
    /// supported platforms; the absolute distance is taken, as
    /// PostgreSQL does.
    fn nest(&mut self) -> PResult<DepthGuard> {
        let guard = self.depth.try_decrease();
        match guard {
            Some(g) if self.stack_base.abs_diff(stack_address()) <= STACK_BUDGET => Ok(g),
            _ => {
                self.too_deep.get_or_insert(self.pos);
                Err(Fail)
            }
        }
    }

    // ---- Identifiers -------------------------------------------------

    /// Whether `kind` is a `regularIdentifier`: `REGULAR_IDENTIFIER` or a
    /// non-reserved word.
    fn is_regular_identifier(kind: TokenKind) -> bool {
        match kind {
            TokenKind::RegularIdentifier => true,
            TokenKind::Keyword(k) => k.is_non_reserved(),
            _ => false,
        }
    }

    /// Whether `kind` is an `identifier`: a regular or a delimited one.
    fn is_identifier(kind: TokenKind) -> bool {
        Self::is_regular_identifier(kind)
            || matches!(kind, TokenKind::DoubleQuoted | TokenKind::AccentQuoted)
    }

    /// Whether the current token is a regular identifier; records it as
    /// expected if not.
    fn at_regular_identifier(&mut self) -> bool {
        if Self::is_regular_identifier(self.peek()) {
            true
        } else {
            self.expect_here(Expected::Token(TokenKind::RegularIdentifier));
            false
        }
    }

    /// `regularIdentifier`: `REGULAR_IDENTIFIER | nonReservedWords`, as
    /// written (IW023).
    fn regular_identifier(&mut self) -> PResult<Spanned<Ident>> {
        if !self.at_regular_identifier() {
            return Err(Fail);
        }
        let t = self.bump();
        Ok(Spanned(t.text(self.text).into(), t.span))
    }

    /// `identifier`: a regular identifier, or a delimited identifier
    /// (`DOUBLE_QUOTED_CHARACTER_SEQUENCE`,
    /// `ACCENT_QUOTED_CHARACTER_SEQUENCE`) with its escapes resolved.
    fn identifier(&mut self) -> PResult<Spanned<Ident>> {
        match self.peek() {
            TokenKind::DoubleQuoted | TokenKind::AccentQuoted => {
                let t = self.bump();
                Ok(Spanned(self.unquote(t).into(), t.span))
            }
            k if Self::is_regular_identifier(k) => self.regular_identifier(),
            _ => {
                self.expect_here(Expected::Token(TokenKind::RegularIdentifier));
                self.fail(Expected::Token(TokenKind::AccentQuoted))
            }
        }
    }

    /// Whether the current token is an `identifier`; records it if not.
    fn at_identifier(&mut self) -> bool {
        if Self::is_identifier(self.peek()) {
            true
        } else {
            self.expect_here(Expected::Token(TokenKind::RegularIdentifier));
            self.expect_here(Expected::Token(TokenKind::AccentQuoted));
            false
        }
    }

    /// The value of a quoted token: the text inside the quotes, with
    /// doubled quotes made single and, unless the token starts with `@`
    /// (`NO_ESCAPE`), the escapes of `GQL.g4:3393-3421` resolved. An
    /// escape that names no Unicode scalar value becomes U+FFFD and is
    /// recorded in `invalid_escapes`.
    fn unquote(&mut self, t: Token) -> String {
        let raw = t.text(self.text);
        let (no_escape, raw) = match raw.strip_prefix('@') {
            Some(r) => (true, r),
            None => (false, raw),
        };
        let quote = raw.chars().next().unwrap_or('\'');
        let inner = &raw[1..raw.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars().peekable();
        let mut invalid = false;
        while let Some(c) = chars.next() {
            if c == quote {
                // A doubled quote: the lexer guarantees the second one.
                chars.next();
                out.push(quote);
            } else if c == '\\' {
                let Some(e) = chars.next() else { break };
                if no_escape {
                    out.push('\\');
                    out.push(e);
                    continue;
                }
                let hex = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>, n: usize| {
                    let digits: String = chars.by_ref().take(n).collect();
                    u32::from_str_radix(&digits, 16)
                        .ok()
                        .and_then(char::from_u32)
                };
                match e {
                    't' => out.push('\t'),
                    'b' => out.push('\u{0008}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    'f' => out.push('\u{000C}'),
                    'u' | 'U' => match hex(&mut chars, if e == 'u' { 4 } else { 6 }) {
                        Some(ch) => out.push(ch),
                        None => {
                            invalid = true;
                            out.push(char::REPLACEMENT_CHARACTER);
                        }
                    },
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        if invalid {
            self.invalid_escapes.push(t.span);
        }
        out
    }
}

/// Recursion counter and guard, copied from datafusion-sqlparser-rs @
/// `14cbf75` `src/parser/mod.rs:95-140` (Apache-2.0; NOTICE: "Apache
/// DataFusion sqlparser-rs, Copyright 2024-2026 The Apache Software
/// Foundation"). Adapted: `try_decrease` returns an `Option`.
mod recursion {
    use std::cell::Cell;
    use std::rc::Rc;

    /// Tracks the remaining nesting depth. The guard holds an `Rc` to the
    /// counter so the parser can stay borrowed mutably.
    pub(crate) struct RecursionCounter {
        remaining_depth: Rc<Cell<usize>>,
    }

    impl RecursionCounter {
        /// A counter of `remaining_depth` levels.
        pub(crate) fn new(remaining_depth: usize) -> Self {
            Self {
                remaining_depth: Rc::new(remaining_depth.into()),
            }
        }

        /// Takes one level, or `None` when none is left. The level is
        /// given back when the guard drops.
        pub(crate) fn try_decrease(&self) -> Option<DepthGuard> {
            let old_value = self.remaining_depth.get();
            if old_value == 0 {
                None
            } else {
                self.remaining_depth.set(old_value - 1);
                Some(DepthGuard {
                    remaining_depth: Rc::clone(&self.remaining_depth),
                })
            }
        }
    }

    /// Gives one level back on drop.
    pub(crate) struct DepthGuard {
        remaining_depth: Rc<Cell<usize>>,
    }

    impl Drop for DepthGuard {
        fn drop(&mut self) {
            let old_value = self.remaining_depth.get();
            self.remaining_depth.set(old_value.saturating_add(1));
        }
    }
}
