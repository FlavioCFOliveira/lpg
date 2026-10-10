//! The lexer of `GQL.g4` (opengql/grammar @ `16ea71b`, [TS-19]).
//!
//! It reproduces the ANTLR lexer the oracle generates from the same
//! grammar: at each position the longest match over all token rules
//! wins, and a tie goes to the rule written first. Keywords are
//! case-insensitive (`options { caseInsensitive = true; }`); the few
//! fragments the grammar marks `caseInsensitive=false` (`\t`, `\b`, `\n`,
//! `\r`, `\f`, `\u`, `\U` escapes and the `0x`, `0o`, `0b` prefixes) are
//! case-sensitive. White space and comments go to ANTLR's hidden channel
//! and are skipped. A character no token rule matches is an error, as
//! ANTLR's "token recognition error".
//!
//! Rule order, which breaks ties: `IMPLIES` (`GQL.g4:3227`), the literal
//! tokens (`BOOLEAN_LITERAL` to `UNSIGNED_BINARY_INTEGER`, 3363-3271),
//! the keywords (3276-3584), `REGULAR_IDENTIFIER`, the parameter
//! references, then the operators and punctuation (3612-3714).
//!
//! The keyword table ([`keywords`]) and the identifier character tables
//! ([`unicode`]) are generated, with drift tests in `tests.rs`.
//!
//! [TS-19]: ../../../docs/technical-spec/query-engine.md

#[allow(
    clippy::all,
    reason = "generated from GQL.g4 by the drift test of tests.rs"
)]
pub(crate) mod keywords;
#[cfg(test)]
mod tests;
mod unicode;

use super::span::{Location, Span};
pub(crate) use keywords::Keyword;

/// Whether a byte is a digit of a prefixed integer's radix.
type DigitTest = fn(u8) -> bool;

/// The kind of a token: one token rule of `GQL.g4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TokenKind {
    /// A keyword: a reserved, pre-reserved or non-reserved word.
    Keyword(Keyword),
    /// `IMPLIES`: the word or `=>`.
    Implies,
    /// `BOOLEAN_LITERAL`: `TRUE`, `FALSE` or `UNKNOWN`.
    BooleanLiteral,
    /// `SINGLE_QUOTED_CHARACTER_SEQUENCE`.
    SingleQuoted,
    /// `DOUBLE_QUOTED_CHARACTER_SEQUENCE`.
    DoubleQuoted,
    /// `ACCENT_QUOTED_CHARACTER_SEQUENCE`.
    AccentQuoted,
    /// `NO_ESCAPE`: a lone `@`.
    NoEscape,
    /// `BYTE_STRING_LITERAL`.
    ByteString,
    /// `UNSIGNED_DECIMAL_IN_SCIENTIFIC_NOTATION_WITH_EXACT_NUMBER_SUFFIX`.
    ScientificExact,
    /// `UNSIGNED_DECIMAL_IN_SCIENTIFIC_NOTATION_WITHOUT_SUFFIX`.
    Scientific,
    /// `UNSIGNED_DECIMAL_IN_SCIENTIFIC_NOTATION_WITH_APPROXIMATE_NUMBER_SUFFIX`.
    ScientificApproximate,
    /// `UNSIGNED_DECIMAL_IN_COMMON_NOTATION_WITH_EXACT_NUMBER_SUFFIX`.
    CommonExact,
    /// `UNSIGNED_DECIMAL_IN_COMMON_NOTATION_WITHOUT_SUFFIX`.
    Common,
    /// `UNSIGNED_DECIMAL_IN_COMMON_NOTATION_WITH_APPROXIMATE_NUMBER_SUFFIX`.
    CommonApproximate,
    /// `UNSIGNED_DECIMAL_INTEGER_WITH_EXACT_NUMBER_SUFFIX`.
    IntegerExact,
    /// `UNSIGNED_DECIMAL_INTEGER_WITH_APPROXIMATE_NUMBER_SUFFIX`.
    IntegerApproximate,
    /// `UNSIGNED_DECIMAL_INTEGER`.
    Integer,
    /// `UNSIGNED_HEXADECIMAL_INTEGER`.
    Hexadecimal,
    /// `UNSIGNED_OCTAL_INTEGER`.
    Octal,
    /// `UNSIGNED_BINARY_INTEGER`.
    Binary,
    /// `REGULAR_IDENTIFIER`.
    RegularIdentifier,
    /// `SUBSTITUTED_PARAMETER_REFERENCE`: `$$` and a name.
    SubstitutedParameter,
    /// `GENERAL_PARAMETER_REFERENCE`: `$` and a name.
    GeneralParameter,
    /// `|+|`
    MultisetAlternation,
    /// `]->`
    BracketRightArrow,
    /// `]~>`
    BracketTildeRightArrow,
    /// `||`
    Concatenation,
    /// `::`
    DoubleColon,
    /// `$$`
    DoubleDollar,
    /// `..`
    DoublePeriod,
    /// `>=`
    GreaterThanOrEquals,
    /// `<-`
    LeftArrow,
    /// `<~`
    LeftArrowTilde,
    /// `<-[`
    LeftArrowBracket,
    /// `<~[`
    LeftArrowTildeBracket,
    /// `<->`
    LeftMinusRight,
    /// `<-/`
    LeftMinusSlash,
    /// `<~/`
    LeftTildeSlash,
    /// `<=`
    LessThanOrEquals,
    /// `-[`
    MinusLeftBracket,
    /// `-/`
    MinusSlash,
    /// `<>`
    NotEquals,
    /// `->`
    RightArrow,
    /// `]-`
    RightBracketMinus,
    /// `]~`
    RightBracketTilde,
    /// `/-`
    SlashMinus,
    /// `/->`
    SlashMinusRight,
    /// `/~`
    SlashTilde,
    /// `/~>`
    SlashTildeRight,
    /// `~[`
    TildeLeftBracket,
    /// `~>`
    TildeRightArrow,
    /// `~/`
    TildeSlash,
    /// `&`
    Ampersand,
    /// `*`
    Asterisk,
    /// `:`
    Colon,
    /// `,`
    Comma,
    /// `$`
    DollarSign,
    /// `"` that opens no string
    DoubleQuote,
    /// `=`
    Equals,
    /// `!`
    ExclamationMark,
    /// `>`
    RightAngleBracket,
    /// `` ` `` that opens no string
    GraveAccent,
    /// `{`
    LeftBrace,
    /// `[`
    LeftBracket,
    /// `(`
    LeftParen,
    /// `<`
    LeftAngleBracket,
    /// `-`
    MinusSign,
    /// `%`
    Percent,
    /// `.`
    Period,
    /// `+`
    PlusSign,
    /// `?`
    QuestionMark,
    /// `'` that opens no string
    Quote,
    /// `\`
    ReverseSolidus,
    /// `}`
    RightBrace,
    /// `]`
    RightBracket,
    /// `)`
    RightParen,
    /// `/`
    Solidus,
    /// `~`
    Tilde,
    /// `|`
    VerticalBar,
    /// The end of the text.
    Eof,
}

impl TokenKind {
    /// How the kind is named in an expected-token list.
    pub(crate) fn describe(self) -> &'static str {
        use TokenKind as T;
        match self {
            T::Keyword(k) => k.text(),
            T::Implies => "IMPLIES",
            T::BooleanLiteral => "TRUE, FALSE or UNKNOWN",
            T::SingleQuoted | T::DoubleQuoted => "a character string literal",
            T::AccentQuoted => "a delimited identifier",
            T::NoEscape => "'@'",
            T::ByteString => "a byte string literal",
            T::ScientificExact
            | T::Scientific
            | T::ScientificApproximate
            | T::CommonExact
            | T::Common
            | T::CommonApproximate
            | T::IntegerExact
            | T::IntegerApproximate => "a numeric literal",
            T::Integer | T::Hexadecimal | T::Octal | T::Binary => "an unsigned integer",
            T::RegularIdentifier => "an identifier",
            T::SubstitutedParameter => "a reference parameter ($$name)",
            T::GeneralParameter => "a parameter ($name)",
            T::MultisetAlternation => "'|+|'",
            T::BracketRightArrow => "']->'",
            T::BracketTildeRightArrow => "']~>'",
            T::Concatenation => "'||'",
            T::DoubleColon => "'::'",
            T::DoubleDollar => "'$$'",
            T::DoublePeriod => "'..'",
            T::GreaterThanOrEquals => "'>='",
            T::LeftArrow => "'<-'",
            T::LeftArrowTilde => "'<~'",
            T::LeftArrowBracket => "'<-['",
            T::LeftArrowTildeBracket => "'<~['",
            T::LeftMinusRight => "'<->'",
            T::LeftMinusSlash => "'<-/'",
            T::LeftTildeSlash => "'<~/'",
            T::LessThanOrEquals => "'<='",
            T::MinusLeftBracket => "'-['",
            T::MinusSlash => "'-/'",
            T::NotEquals => "'<>'",
            T::RightArrow => "'->'",
            T::RightBracketMinus => "']-'",
            T::RightBracketTilde => "']~'",
            T::SlashMinus => "'/-'",
            T::SlashMinusRight => "'/->'",
            T::SlashTilde => "'/~'",
            T::SlashTildeRight => "'/~>'",
            T::TildeLeftBracket => "'~['",
            T::TildeRightArrow => "'~>'",
            T::TildeSlash => "'~/'",
            T::Ampersand => "'&'",
            T::Asterisk => "'*'",
            T::Colon => "':'",
            T::Comma => "','",
            T::DollarSign => "'$'",
            T::DoubleQuote => "'\"'",
            T::Equals => "'='",
            T::ExclamationMark => "'!'",
            T::RightAngleBracket => "'>'",
            T::GraveAccent => "'`'",
            T::LeftBrace => "'{'",
            T::LeftBracket => "'['",
            T::LeftParen => "'('",
            T::LeftAngleBracket => "'<'",
            T::MinusSign => "'-'",
            T::Percent => "'%'",
            T::Period => "'.'",
            T::PlusSign => "'+'",
            T::QuestionMark => "'?'",
            T::Quote => "'''",
            T::ReverseSolidus => "'\\'",
            T::RightBrace => "'}'",
            T::RightBracket => "']'",
            T::RightParen => "')'",
            T::Solidus => "'/'",
            T::Tilde => "'~'",
            T::VerticalBar => "'|'",
            T::Eof => "the end of the text",
        }
    }
}

/// A token: its kind and its span in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token {
    /// The token rule.
    pub(crate) kind: TokenKind,
    /// Where it is.
    pub(crate) span: Span,
}

impl Token {
    /// The token's text in `source`.
    pub(crate) fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start.offset as usize..self.span.end.offset as usize]
    }
}

/// A character no token rule matches, at `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LexError {
    /// Where the character is.
    pub(crate) at: Location,
}

/// Splits `text` into tokens, the last one [`TokenKind::Eof`].
///
/// # Errors
///
/// [`LexError`] at the first character no token rule matches.
pub(crate) fn tokenize(text: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        line: 1,
        column: 1,
    };
    // A token averages several bytes; this avoids most regrowth.
    let mut out = Vec::with_capacity(text.len() / 4 + 1);
    loop {
        let start = lexer.location();
        let Some((len, kind)) = lexer.next_match()? else {
            out.push(Token {
                kind: TokenKind::Eof,
                span: Span::new(start, start),
            });
            return Ok(out);
        };
        lexer.advance(len);
        if let Some(kind) = kind {
            out.push(Token {
                kind,
                span: Span::new(start, lexer.location()),
            });
        }
    }
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    column: u32,
}

/// Whether `c` is in `WHITESPACE` (`GQL.g4:3726-3756`).
fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'
            | '\n'
            | '\u{000B}'
            | '\u{000C}'
            | '\r'
            | '\u{001C}'..='\u{001F}'
            | '\u{1680}'
            | '\u{180E}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{00A0}'
            | '\u{202F}'
    )
}

/// The keyword spelled `word`, ignoring ASCII case. Only ASCII letters
/// fold: `caseInsensitive` in ANTLR adds the upper and lower case of each
/// literal character, and every keyword is ASCII.
fn keyword(word: &str) -> Option<Keyword> {
    if word.len() > 32 || !word.is_ascii() {
        return None;
    }
    let mut upper = [0u8; 32];
    let upper = &mut upper[..word.len()];
    upper.copy_from_slice(word.as_bytes());
    upper.make_ascii_uppercase();
    keywords::KEYWORDS
        .binary_search_by(|(text, _, _)| text.as_bytes().cmp(upper))
        .ok()
        .map(|i| keywords::KEYWORDS[i].1)
}

/// Whether `c` can start a `REGULAR_IDENTIFIER`: `ID_Start` or `Pc`.
pub(crate) fn is_identifier_start(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic() || c == '_';
    }
    unicode::in_table(unicode::IDENTIFIER_START, c)
}

/// Whether `c` can continue a `REGULAR_IDENTIFIER`: `ID_Continue`.
pub(crate) fn is_identifier_extend(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || c == '_';
    }
    unicode::in_table(unicode::IDENTIFIER_EXTEND, c)
}

impl Lexer<'_> {
    fn location(&self) -> Location {
        Location {
            offset: u32::try_from(self.pos).unwrap_or(u32::MAX),
            line: self.line,
            column: self.column,
        }
    }

    fn advance(&mut self, len: usize) {
        for c in self.text[self.pos..self.pos + len].chars() {
            if c == '\n' {
                self.line = self.line.saturating_add(1);
                self.column = 1;
            } else {
                self.column = self.column.saturating_add(1);
            }
        }
        self.pos += len;
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.bytes.get(at).copied()
    }

    fn char_at(&self, at: usize) -> Option<char> {
        self.text.get(at..).and_then(|s| s.chars().next())
    }

    /// The longest token at the current position: its length in bytes
    /// and its kind, `None` for a hidden token; `Ok(None)` at the end.
    fn next_match(&self) -> Result<Option<(usize, Option<TokenKind>)>, LexError> {
        let p = self.pos;
        let Some(c) = self.char_at(p) else {
            return Ok(None);
        };
        let found = if is_whitespace(c) {
            let mut end = p;
            while let Some(c) = self.char_at(end).filter(|&c| is_whitespace(c)) {
                end += c.len_utf8();
            }
            Some((end - p, None))
        } else if c.is_ascii_digit() || c == '.' {
            self.number_or_period(p)
        } else if is_identifier_start(c) {
            Some(self.word(p))
        } else {
            self.other(p, c)
        };
        found.map(Some).ok_or(LexError {
            at: self.location(),
        })
    }

    /// A word: a keyword, a boolean literal, `IMPLIES`, a regular
    /// identifier, or a byte string literal (`X'…'`).
    fn word(&self, p: usize) -> (usize, Option<TokenKind>) {
        let mut end = p;
        let mut chars = self.text[p..].chars();
        if let Some(c) = chars.next() {
            end += c.len_utf8();
        }
        for c in chars {
            if !is_identifier_extend(c) {
                break;
            }
            end += c.len_utf8();
        }
        let word = &self.text[p..end];
        let kind = if word.eq_ignore_ascii_case("IMPLIES") {
            TokenKind::Implies
        } else if ["TRUE", "FALSE", "UNKNOWN"]
            .iter()
            .any(|b| word.eq_ignore_ascii_case(b))
        {
            TokenKind::BooleanLiteral
        } else if let Some(k) = keyword(word) {
            TokenKind::Keyword(k)
        } else {
            TokenKind::RegularIdentifier
        };
        let mut best = (end - p, Some(kind));
        if matches!(self.byte(p), Some(b'x' | b'X'))
            && let Some(len) = self.byte_string(p)
            && len > best.0
        {
            best = (len, Some(TokenKind::ByteString));
        }
        best
    }

    /// `BYTE_STRING_LITERAL`: `'X' QUOTE SPACE* (HEX_DIGIT SPACE* HEX_DIGIT
    /// SPACE*)* QUOTE`, starting at the `X`.
    fn byte_string(&self, p: usize) -> Option<usize> {
        let mut i = p + 1;
        if self.byte(i)? != b'\'' {
            return None;
        }
        i += 1;
        let skip_spaces = |mut i: usize| {
            while self.byte(i) == Some(b' ') {
                i += 1;
            }
            i
        };
        i = skip_spaces(i);
        loop {
            match self.byte(i)? {
                b'\'' => return Some(i + 1 - p),
                b if b.is_ascii_hexdigit() => {
                    i = skip_spaces(i + 1);
                    if !self.byte(i)?.is_ascii_hexdigit() {
                        return None;
                    }
                    i = skip_spaces(i + 1);
                }
                _ => return None,
            }
        }
    }

    /// `DIGIT (UNDERSCORE? DIGIT)*` from `i`: its end, or `None`.
    fn decimal_integer(&self, i: usize) -> Option<usize> {
        if !self.byte(i)?.is_ascii_digit() {
            return None;
        }
        let mut end = i + 1;
        loop {
            match self.byte(end) {
                Some(b) if b.is_ascii_digit() => end += 1,
                Some(b'_') if self.byte(end + 1).is_some_and(|b| b.is_ascii_digit()) => end += 2,
                _ => return Some(end),
            }
        }
    }

    /// The numeric literals, or `.` and `..`.
    fn number_or_period(&self, p: usize) -> Option<(usize, Option<TokenKind>)> {
        use TokenKind as T;
        let mut best: Option<(usize, T)> = None;
        let mut offer = |end: usize, kind: T| {
            if best.is_none_or(|(len, _)| end - p > len) {
                best = Some((end - p, kind));
            }
        };
        if self.byte(p) == Some(b'.') {
            offer(p + 1, T::Period);
            if self.byte(p + 1) == Some(b'.') {
                offer(p + 2, T::DoublePeriod);
            }
        }
        // Prefixed integers; the prefixes are case-sensitive.
        if self.byte(p) == Some(b'0') {
            let digit: Option<(T, DigitTest)> = match self.byte(p + 1) {
                Some(b'x') => Some((T::Hexadecimal, |b: u8| b.is_ascii_hexdigit())),
                Some(b'o') => Some((T::Octal, |b: u8| (b'0'..=b'7').contains(&b))),
                Some(b'b') => Some((T::Binary, |b: u8| b == b'0' || b == b'1')),
                _ => None,
            };
            if let Some((kind, ok)) = digit {
                let mut end = p + 2;
                loop {
                    match self.byte(end) {
                        Some(b) if ok(b) => end += 1,
                        Some(b'_') if self.byte(end + 1).is_some_and(ok) => end += 2,
                        _ => break,
                    }
                }
                if end > p + 2 {
                    offer(end, kind);
                }
            }
        }
        // Decimal: integer, common notation, scientific notation.
        let integer = self.decimal_integer(p);
        let common = match integer {
            Some(end) if self.byte(end) == Some(b'.') => {
                Some(self.decimal_integer(end + 1).unwrap_or(end + 1))
            }
            Some(_) => None,
            None if self.byte(p) == Some(b'.') => self.decimal_integer(p + 1),
            None => None,
        };
        let scientific = common.or(integer).and_then(|mantissa| {
            if !matches!(self.byte(mantissa), Some(b'e' | b'E')) {
                return None;
            }
            let mut i = mantissa + 1;
            if matches!(self.byte(i), Some(b'+' | b'-')) {
                i += 1;
            }
            self.decimal_integer(i)
        });
        let suffix = |end: usize| match self.byte(end) {
            Some(b'm' | b'M') => Some(true),
            Some(b'f' | b'F' | b'd' | b'D') => Some(false),
            _ => None,
        };
        for (end, plain, exact, approximate) in [
            (integer, T::Integer, T::IntegerExact, T::IntegerApproximate),
            (common, T::Common, T::CommonExact, T::CommonApproximate),
            (
                scientific,
                T::Scientific,
                T::ScientificExact,
                T::ScientificApproximate,
            ),
        ] {
            let Some(end) = end else { continue };
            offer(end, plain);
            match suffix(end) {
                Some(true) => offer(end + 1, exact),
                Some(false) => offer(end + 1, approximate),
                None => {}
            }
        }
        best.map(|(len, kind)| (len, Some(kind)))
    }

    /// A quoted character sequence with the quote `q` opening at `p`: its
    /// length, or `None`. The longest match ends at the last closing
    /// quote seen before the sequence becomes invalid, since a doubled
    /// quote both closes the sequence and continues it.
    fn quoted(&self, p: usize, q: u8) -> Option<usize> {
        let mut i = p + 1;
        let mut accepted = None;
        loop {
            match self.byte(i) {
                None | Some(b'\r' | b'\n') => return accepted,
                Some(b) if b == q => {
                    accepted = Some(i + 1 - p);
                    if self.byte(i + 1) == Some(q) {
                        i += 2;
                    } else {
                        return accepted;
                    }
                }
                Some(b'\\') => match self.byte(i + 1) {
                    Some(b'\\' | b'\'' | b'"' | b'`' | b't' | b'b' | b'n' | b'r' | b'f') => {
                        i += 2;
                    }
                    Some(b'u') if self.hex_digits(i + 2, 4) => i += 6,
                    Some(b'U') if self.hex_digits(i + 2, 6) => i += 8,
                    _ => return accepted,
                },
                Some(_) => {
                    i += self.char_at(i).map_or(1, char::len_utf8);
                }
            }
        }
    }

    fn hex_digits(&self, i: usize, n: usize) -> bool {
        (i..i + n).all(|j| self.byte(j).is_some_and(|b| b.is_ascii_hexdigit()))
    }

    /// `PARAMETER_NAME` from `i` (`SEPARATED_IDENTIFIER`: a delimited
    /// identifier, with an optional `@`, or `ID_Continue+`): its end.
    fn parameter_name(&self, i: usize) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut take = |end: usize| best = Some(best.map_or(end, |b: usize| b.max(end)));
        let quote_at = if self.byte(i) == Some(b'@') { i + 1 } else { i };
        if let Some(q @ (b'"' | b'`')) = self.byte(quote_at)
            && let Some(len) = self.quoted(quote_at, q)
        {
            take(quote_at + len);
        }
        let mut end = i;
        while let Some(c) = self.char_at(end).filter(|&c| is_identifier_extend(c)) {
            end += c.len_utf8();
        }
        if end > i {
            take(end);
        }
        best
    }

    /// Operators, punctuation, comments, strings and parameters.
    fn other(&self, p: usize, c: char) -> Option<(usize, Option<TokenKind>)> {
        use TokenKind as T;
        let rest = &self.bytes[p..];
        let starts = |s: &[u8]| rest.starts_with(s);
        // Comments are hidden. A bracketed comment ends at the first
        // `*/`; unterminated, it is no comment.
        if starts(b"/*")
            && let Some(end) = self.text[p + 2..].find("*/")
        {
            return Some((end + 4, None));
        }
        if starts(b"//") || starts(b"--") {
            let end = self.text[p..]
                .find(['\r', '\n'])
                .unwrap_or(self.text.len() - p);
            return Some((end, None));
        }
        let kind_len = |len: usize, kind: T| Some((len, Some(kind)));
        match c {
            '\'' | '"' | '`' => {
                let q = c as u8;
                if let Some(len) = self.quoted(p, q) {
                    return kind_len(len, string_kind(q));
                }
                kind_len(
                    1,
                    match c {
                        '\'' => T::Quote,
                        '"' => T::DoubleQuote,
                        _ => T::GraveAccent,
                    },
                )
            }
            '@' => {
                if let Some(q @ (b'\'' | b'"' | b'`')) = self.byte(p + 1)
                    && let Some(len) = self.quoted(p + 1, q)
                {
                    return kind_len(len + 1, string_kind(q));
                }
                kind_len(1, T::NoEscape)
            }
            '$' => {
                if starts(b"$$") {
                    match self.parameter_name(p + 2) {
                        Some(end) => kind_len(end - p, T::SubstitutedParameter),
                        None => kind_len(2, T::DoubleDollar),
                    }
                } else {
                    match self.parameter_name(p + 1) {
                        Some(end) => kind_len(end - p, T::GeneralParameter),
                        None => kind_len(1, T::DollarSign),
                    }
                }
            }
            _ => OPERATORS
                .iter()
                .find(|(text, _)| starts(text.as_bytes()))
                .map(|&(text, kind)| (text.len(), Some(kind))),
        }
    }
}

fn string_kind(q: u8) -> TokenKind {
    match q {
        b'\'' => TokenKind::SingleQuoted,
        b'"' => TokenKind::DoubleQuoted,
        _ => TokenKind::AccentQuoted,
    }
}

/// Operators and punctuation, longest first (so the first match is the
/// longest). `=>` is `IMPLIES`, whose rule precedes `RIGHT_DOUBLE_ARROW`.
const OPERATORS: &[(&str, TokenKind)] = &[
    ("|+|", TokenKind::MultisetAlternation),
    ("]->", TokenKind::BracketRightArrow),
    ("]~>", TokenKind::BracketTildeRightArrow),
    ("<-[", TokenKind::LeftArrowBracket),
    ("<~[", TokenKind::LeftArrowTildeBracket),
    ("<->", TokenKind::LeftMinusRight),
    ("<-/", TokenKind::LeftMinusSlash),
    ("<~/", TokenKind::LeftTildeSlash),
    ("/->", TokenKind::SlashMinusRight),
    ("/~>", TokenKind::SlashTildeRight),
    ("||", TokenKind::Concatenation),
    ("::", TokenKind::DoubleColon),
    (">=", TokenKind::GreaterThanOrEquals),
    ("<-", TokenKind::LeftArrow),
    ("<~", TokenKind::LeftArrowTilde),
    ("<=", TokenKind::LessThanOrEquals),
    ("-[", TokenKind::MinusLeftBracket),
    ("-/", TokenKind::MinusSlash),
    ("<>", TokenKind::NotEquals),
    ("->", TokenKind::RightArrow),
    ("]-", TokenKind::RightBracketMinus),
    ("]~", TokenKind::RightBracketTilde),
    ("=>", TokenKind::Implies),
    ("/-", TokenKind::SlashMinus),
    ("/~", TokenKind::SlashTilde),
    ("~[", TokenKind::TildeLeftBracket),
    ("~>", TokenKind::TildeRightArrow),
    ("~/", TokenKind::TildeSlash),
    ("&", TokenKind::Ampersand),
    ("*", TokenKind::Asterisk),
    (":", TokenKind::Colon),
    (",", TokenKind::Comma),
    ("=", TokenKind::Equals),
    ("!", TokenKind::ExclamationMark),
    (">", TokenKind::RightAngleBracket),
    ("{", TokenKind::LeftBrace),
    ("[", TokenKind::LeftBracket),
    ("(", TokenKind::LeftParen),
    ("<", TokenKind::LeftAngleBracket),
    ("-", TokenKind::MinusSign),
    ("%", TokenKind::Percent),
    ("+", TokenKind::PlusSign),
    ("?", TokenKind::QuestionMark),
    ("\\", TokenKind::ReverseSolidus),
    ("}", TokenKind::RightBrace),
    ("]", TokenKind::RightBracket),
    (")", TokenKind::RightParen),
    ("/", TokenKind::Solidus),
    ("~", TokenKind::Tilde),
    ("|", TokenKind::VerticalBar),
];
