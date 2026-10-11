//! Drift tests of the generated keyword and identifier tables, and the
//! lexer's longest-match rules.
//!
//! `keyword_table_matches_the_grammar` regenerates `keywords.rs` from the
//! keyword rules of `GQL.g4` (`tools/gql-oracle/grammar/GQL.g4`, the
//! unmodified opengql/grammar @ `16ea71b`, lines 3276-3584);
//! `identifier_tables_match_the_ucd` regenerates `unicode.rs` from the
//! Unicode 15.0.0 files in `docs/standards/unicode-15.0.0/`. Both fail on
//! any difference; `LPG_UPDATE_KEYWORDS=1` and `LPG_UPDATE_UNICODE=1`
//! rewrite the files.
//!
//! The keyword generator is ported from zu @ `a821319` (Apache-2.0, see
//! `LICENSE-APACHE`), `crates/zu-common/tests/keyword_table.rs`: the
//! three-way split into reserved, pre-reserved and non-reserved words and
//! the shape assertions. Adapted: the input is `GQL.g4`'s lexer rules,
//! not the ISO BNF, and the output is an enum with a sorted lookup table.

use super::{TokenKind as T, tokenize};
use crate::gqlstatus::tests::{check_generated, rustfmt};
use std::fmt::Write as _;
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The keyword rules of `GQL.g4`: `(section, token name, text)` in
/// grammar order.
fn grammar_keywords() -> Vec<(&'static str, String, String)> {
    let g4 = std::fs::read_to_string(manifest_dir().join("tools/gql-oracle/grammar/GQL.g4"))
        .expect("GQL.g4");
    let mut section = None;
    let mut out = Vec::new();
    for line in g4.lines() {
        let line = line.trim();
        match line {
            "// Reserved words" => section = Some("Reserved"),
            "// Prereserved words" => section = Some("PreReserved"),
            "// Nonreserved words" => section = Some("NonReserved"),
            _ => {}
        }
        if line.starts_with("fragment ") || line.starts_with("// 21.4") {
            if section == Some("NonReserved") {
                break;
            }
            continue;
        }
        let Some(section) = section else { continue };
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        let Some(text) = rest
            .strip_prefix('\'')
            .and_then(|r| r.split_once("';"))
            .map(|(t, _)| t)
        else {
            continue;
        };
        out.push((section, name.trim().to_owned(), text.to_owned()));
    }
    out
}

fn camel(text: &str) -> String {
    text.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| {
                f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()
            })
        })
        .collect()
}

fn render_keywords(words: &[(&str, String, String)]) -> String {
    let mut out = String::from(
        "//! Generated from the keyword rules of `GQL.g4` (opengql/grammar @\n\
         //! `16ea71b`, lines 3276-3584). Do not edit by hand: run\n\
         //! `LPG_UPDATE_KEYWORDS=1 cargo test -p lpg\n\
         //! query::lexer::tests::keyword_table_matches_the_grammar`.\n\
         \n\
         /// A keyword token of `GQL.g4`, named after its text.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n\
         pub(crate) enum Keyword {\n",
    );
    for (_, name, text) in words {
        writeln!(out, "    /// `{text}` (`{name}`)\n    {},", camel(text)).expect("write");
    }
    out.push_str(
        "}\n\n\
         /// How a keyword may be used (`GQL.g4` sections of 21.3).\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n\
         pub(crate) enum Reservation {\n\
         \x20   /// Never an identifier.\n    Reserved,\n\
         \x20   /// Reserved for a later edition; never an identifier.\n    PreReserved,\n\
         \x20   /// An identifier where the grammar allows `nonReservedWords`.\n    NonReserved,\n\
         }\n\n\
         /// Every keyword: its text, the keyword and its reservation, sorted\n\
         /// by text for a binary search.\n\
         pub(crate) static KEYWORDS: &[(&str, Keyword, Reservation)] = &[\n",
    );
    let mut sorted: Vec<_> = words.iter().collect();
    sorted.sort_by(|a, b| a.2.cmp(&b.2));
    for (section, _, text) in &sorted {
        writeln!(
            out,
            "    (\"{text}\", Keyword::{}, Reservation::{section}),",
            camel(text)
        )
        .expect("write");
    }
    out.push_str(
        "];\n\n\
         impl Keyword {\n\
         \x20   /// The keyword's text, upper case.\n\
         \x20   pub(crate) fn text(self) -> &'static str {\n\
         \x20       match self {\n",
    );
    for (_, _, text) in words {
        writeln!(out, "            Keyword::{} => \"{text}\",", camel(text)).expect("write");
    }
    out.push_str(
        "        }\n    }\n\n\
         \x20   /// Whether the keyword is a non-reserved word, which the grammar\n\
         \x20   /// also accepts as a regular identifier (`nonReservedWords`).\n\
         \x20   pub(crate) fn is_non_reserved(self) -> bool {\n\
         \x20       matches!(\n\
         \x20           self,\n",
    );
    let non: Vec<String> = words
        .iter()
        .filter(|w| w.0 == "NonReserved")
        .map(|w| format!("Keyword::{}", camel(&w.2)))
        .collect();
    writeln!(out, "            {}", non.join("\n                | ")).expect("write");
    out.push_str("        )\n    }\n}\n");
    out
}

#[test]
fn keyword_table_matches_the_grammar() {
    let words = grammar_keywords();
    let rendered = rustfmt(&render_keywords(&words));
    check_generated(
        "src/query/lexer/keywords.rs",
        &rendered,
        "LPG_UPDATE_KEYWORDS",
    );
}

/// The counts TS-19 records: 218 reserved, 39 pre-reserved and 47
/// non-reserved words, each text once, and the non-reserved words equal
/// to the alternatives of the parser rule `nonReservedWords`.
#[test]
fn the_grammar_has_the_keyword_shape_ts19_records() {
    let words = grammar_keywords();
    let count = |s: &str| words.iter().filter(|w| w.0 == s).count();
    assert_eq!(count("Reserved"), 218);
    assert_eq!(count("PreReserved"), 39);
    assert_eq!(count("NonReserved"), 47);
    let mut texts: Vec<_> = words.iter().map(|w| w.2.as_str()).collect();
    texts.sort_unstable();
    texts.dedup();
    assert_eq!(texts.len(), words.len(), "each keyword text once");
    let mut camels: Vec<_> = words.iter().map(|w| camel(&w.2)).collect();
    camels.sort_unstable();
    camels.dedup();
    assert_eq!(camels.len(), words.len(), "each variant name once");
    // `nonReservedWords` (GQL.g4) lists exactly the non-reserved tokens.
    let g4 = std::fs::read_to_string(manifest_dir().join("tools/gql-oracle/grammar/GQL.g4"))
        .expect("GQL.g4");
    let rule = &g4[g4.find("\nnonReservedWords").expect("rule")..];
    let rule = &rule[..rule.find(';').expect("end")];
    let mut listed: Vec<&str> = rule
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty() && *w != "nonReservedWords")
        .collect();
    listed.sort_unstable();
    let mut non: Vec<&str> = words
        .iter()
        .filter(|w| w.0 == "NonReserved")
        .map(|w| w.1.as_str())
        .collect();
    non.sort_unstable();
    assert_eq!(listed, non);
    // The six BNF words TS-19 says GQL.g4 lacks are not keyword rules.
    for w in ["TRUE", "FALSE", "UNKNOWN", "IMPLIES", "WHITESPACE", "TEMP"] {
        assert!(!words.iter().any(|k| k.2 == w), "{w}");
    }
}

/// Inclusive `(first, last)` code-point ranges.
type Ranges = Vec<(u32, u32)>;

/// `(first, last)` ranges of the code points with `property` in a UCD
/// file whose lines are `XXXX[..YYYY] ; property # comment`.
fn ucd_ranges(file: &str, property: &str) -> Vec<(u32, u32)> {
    let text = std::fs::read_to_string(
        manifest_dir()
            .join("docs/standards/unicode-15.0.0")
            .join(file),
    )
    .expect(file);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut fields = line.split(';').map(str::trim);
        let (Some(range), Some(prop)) = (fields.next(), fields.next()) else {
            continue;
        };
        if prop != property {
            continue;
        }
        let (a, b) = range.split_once("..").unwrap_or((range, range));
        out.push((
            u32::from_str_radix(a, 16).expect("hex"),
            u32::from_str_radix(b, 16).expect("hex"),
        ));
    }
    out
}

/// Sorted, merged ranges.
fn merge(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (a, b) in ranges {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn render_table(out: &mut String, name: &str, doc: &str, ranges: &[(u32, u32)]) {
    let count: u32 = ranges.iter().map(|(a, b)| b - a + 1).sum();
    writeln!(
        out,
        "{doc}\n/// {} ranges, {count} code points.",
        ranges.len()
    )
    .expect("write");
    writeln!(
        out,
        "#[rustfmt::skip]\npub(super) static {name}: &[(u32, u32)] = &["
    )
    .expect("write");
    for chunk in ranges.chunks(4) {
        out.push_str("   ");
        for (a, b) in chunk {
            write!(out, " (0x{a:04X}, 0x{b:04X}),").expect("write");
        }
        out.push('\n');
    }
    out.push_str("];\n\n");
}

fn render_unicode(start: &[(u32, u32)], extend: &[(u32, u32)]) -> String {
    let mut out = String::from(
        "//! Generated from the Unicode Character Database 15.0.0\n\
         //! (`docs/standards/unicode-15.0.0/`). Do not edit by hand: run\n\
         //! `LPG_UPDATE_UNICODE=1 cargo test -p lpg\n\
         //! query::lexer::tests::identifier_tables_match_the_ucd`.\n\n",
    );
    render_table(
        &mut out,
        "IDENTIFIER_START",
        "/// `IDENTIFIER_START` of `GQL.g4:3606-3609`: `ID_Start` or `Pc`.",
        start,
    );
    render_table(
        &mut out,
        "IDENTIFIER_EXTEND",
        "/// `IDENTIFIER_EXTEND` of `GQL.g4:3611-3613`: `ID_Continue`.",
        extend,
    );
    out.push_str(
        "/// Whether `c` is in `table`.\n\
         pub(super) fn in_table(table: &[(u32, u32)], c: char) -> bool {\n\
         \x20   let c = u32::from(c);\n\
         \x20   table\n\
         \x20       .binary_search_by(|&(a, b)| {\n\
         \x20           if b < c {\n\
         \x20               std::cmp::Ordering::Less\n\
         \x20           } else if a > c {\n\
         \x20               std::cmp::Ordering::Greater\n\
         \x20           } else {\n\
         \x20               std::cmp::Ordering::Equal\n\
         \x20           }\n\
         \x20       })\n\
         \x20       .is_ok()\n\
         }\n",
    );
    out
}

fn identifier_sets() -> (Ranges, Ranges) {
    let mut start = ucd_ranges("DerivedCoreProperties.txt", "ID_Start");
    start.extend(ucd_ranges("DerivedGeneralCategory.txt", "Pc"));
    let extend = ucd_ranges("DerivedCoreProperties.txt", "ID_Continue");
    (merge(start), merge(extend))
}

#[test]
fn identifier_tables_match_the_ucd() {
    let (start, extend) = identifier_sets();
    let rendered = rustfmt(&render_unicode(&start, &extend));
    check_generated(
        "src/query/lexer/unicode.rs",
        &rendered,
        "LPG_UPDATE_UNICODE",
    );
}

/// The sizes the Round S probe measured for the oracle's jar: 136 345
/// `ID_Start` and 139 482 `ID_Continue` code points; `Pc` adds none to
/// `ID_Start` that `ID_Continue` lacks.
#[test]
fn identifier_sets_have_the_measured_sizes() {
    let size = |r: &[(u32, u32)]| -> u32 { r.iter().map(|(a, b)| b - a + 1).sum() };
    assert_eq!(
        size(&merge(ucd_ranges("DerivedCoreProperties.txt", "ID_Start"))),
        136_345
    );
    let (start, extend) = identifier_sets();
    assert_eq!(size(&extend), 139_482);
    // Pc has 10 code points; `_` (U+005F) is already ID_Continue but not
    // ID_Start.
    assert_eq!(size(&start), 136_345 + 10);
    for c in ['a', 'λ', '_', '\u{203F}', 'é'] {
        assert!(super::is_identifier_start(c), "{c:?}");
    }
    for c in ['1', '$', '€', '\u{00B7}'] {
        assert!(!super::is_identifier_start(c), "{c:?}");
    }
    for c in ['1', '\u{00B7}', '\u{0301}', '_'] {
        assert!(super::is_identifier_extend(c), "{c:?}");
    }
    // Private use (Co) is never part of a regular identifier (IA020).
    assert!(!super::is_identifier_extend('\u{E000}'));
}

fn kinds(text: &str) -> Vec<T> {
    tokenize(text)
        .expect("tokens")
        .iter()
        .map(|t| t.kind)
        .filter(|k| *k != T::Eof)
        .collect()
}

#[test]
fn longest_match_and_rule_order() {
    use super::Keyword as K;
    // Keywords are case-insensitive and win ties with identifiers.
    assert_eq!(
        kinds("match MaTcH matchx"),
        [
            T::Keyword(K::Match),
            T::Keyword(K::Match),
            T::RegularIdentifier
        ]
    );
    assert_eq!(
        kinds("true Unknown => implies"),
        [T::BooleanLiteral, T::BooleanLiteral, T::Implies, T::Implies]
    );
    // Non-ASCII case variants of keyword letters are identifiers.
    assert_eq!(kinds("\u{212A}EEP"), [T::RegularIdentifier]);
    // Numbers.
    assert_eq!(
        kinds("1 1_000 1. .5 1.5e-3 1e5m 2f 3d 4M"),
        [
            T::Integer,
            T::Integer,
            T::Common,
            T::Common,
            T::Scientific,
            T::ScientificExact,
            T::IntegerApproximate,
            T::IntegerApproximate,
            T::IntegerExact,
        ]
    );
    assert_eq!(
        kinds("0x1F 0X1F 0o17 0b10 0b2"),
        [
            T::Hexadecimal,
            T::Integer,
            T::RegularIdentifier,
            T::Octal,
            T::Binary,
            T::Integer,
            T::RegularIdentifier,
        ]
    );
    assert_eq!(kinds("1..2"), [T::Common, T::Common]);
    assert_eq!(
        kinds("1_ 1e"),
        [
            T::Integer,
            T::RegularIdentifier,
            T::Integer,
            T::RegularIdentifier
        ]
    );
    // Strings: doubled quotes, escapes, the longest closed prefix.
    assert_eq!(
        kinds(r"'a''b' 'it\'s' @'x'"),
        [T::SingleQuoted, T::SingleQuoted, T::SingleQuoted]
    );
    assert_eq!(kinds("'a''"), [T::SingleQuoted, T::Quote]);
    assert_eq!(
        kinds(r"'\q'"),
        [T::Quote, T::ReverseSolidus, T::RegularIdentifier, T::Quote]
    );
    assert_eq!(
        kinds("\"x\" `y` @"),
        [T::DoubleQuoted, T::AccentQuoted, T::NoEscape]
    );
    assert_eq!(
        kinds("X'0A 1b' x'0'"),
        [T::ByteString, T::RegularIdentifier, T::SingleQuoted]
    );
    // Parameters.
    assert_eq!(
        kinds("$a $1 $\"q\" $$b $ $$"),
        [
            T::GeneralParameter,
            T::GeneralParameter,
            T::GeneralParameter,
            T::SubstitutedParameter,
            T::DollarSign,
            T::DoubleDollar,
        ]
    );
    // Operators and comments.
    assert_eq!(
        kinds("<-[ <- <-> ]-> ]- |+| || | :: :"),
        [
            T::LeftArrowBracket,
            T::LeftArrow,
            T::LeftMinusRight,
            T::BracketRightArrow,
            T::RightBracketMinus,
            T::MultisetAlternation,
            T::Concatenation,
            T::VerticalBar,
            T::DoubleColon,
            T::Colon,
        ]
    );
    assert_eq!(
        kinds("a -- c\nb // d\r/* e */c /* f"),
        [
            T::RegularIdentifier,
            T::RegularIdentifier,
            T::RegularIdentifier,
            T::Solidus,
            T::Asterisk,
            T::RegularIdentifier,
        ]
    );
    assert_eq!(
        kinds("(a)-->(b)"),
        [T::LeftParen, T::RegularIdentifier, T::RightParen]
    );
    // `_` is an identifier, not UNDERSCORE.
    assert_eq!(kinds("_"), [T::RegularIdentifier]);
    // White space of GQL.g4 only.
    assert_eq!(
        kinds("a\u{3000}\u{00A0}\u{2007}b"),
        [T::RegularIdentifier, T::RegularIdentifier]
    );
    assert!(tokenize("a\u{0085}b").is_err());
}

#[test]
fn an_unknown_character_is_an_error_at_its_position() {
    let e = tokenize("MATCH\n  #").expect_err("no rule matches #");
    assert_eq!((e.at.line, e.at.column, e.at.offset), (2, 3, 8));
    for bad in [";", "^", "#", "\u{FEFF}"] {
        assert!(tokenize(bad).is_err(), "{bad:?}");
    }
}
