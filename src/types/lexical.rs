//! Lexical forms and type names of every runtime value (TS-21 "Lexical
//! forms and type names"; FR-IMP-006; rmp #56).
//!
//! A **lexical form** is the text of a CSV field, of a GraphML `<data>`
//! element and of the `"$value"` string of a JSON Lines tagged object; a
//! **type name** is the text of an `lpg:<type name>` CSV column, of
//! `"$lpg_type"` and of the GraphML type attribute (TS-26). One table
//! serves the three text formats ([`lexical_with`], [`parse`],
//! [`TypeName`]).
//!
//! Sources, as TS-21 cites them:
//!
//! - **Literal syntax** of the GQL grammar, `opengql/grammar` @ `16ea71b`
//!   (Apache-2.0; used as a specification, no text copied): byte strings
//!   `BYTE_STRING_LITERAL` (`GQL.g4:3188`), lists
//!   `listValueConstructorByEnumeration` (`:2500`), records
//!   `recordConstructor`, `field` (`:2514, 2528`), strings
//!   `characterStringLiteral` and its escapes (`:2972, 3117, 3145,
//!   3157-3186`), accent-quoted names (`:3125`).
//! - **Temporal values and points**: the crate's own forms
//!   ([`super::temporal::format`], [`super::temporal::parse`],
//!   [`super::Point::lexical`], [`super::Point::parse`]).
//! - **Nodes, edges and paths**: the maps of APOC's `FormatUtils.toMap`
//!   (R-APOC @ `e52bc20`
//!   `common/src/main/java/apoc/export/util/FormatUtils.java:66-100`,
//!   Apache-2.0, `Copyright (c) "Neo4j" Neo4j Sweden AB [http://neo4j.com]`;
//!   notice in `LICENSE-APACHE`), ported: a node is
//!   `{id, labels, properties}`, an edge `{id, type, start, end,
//!   properties}` and a path the list of its nodes and edges, written in
//!   the record form below.
//! - **Floats**: Rust's shortest round-trip text (`f64` `Debug`), and
//!   `NaN`, `Infinity`, `-Infinity`, as Neo4j's CSV import reads them with
//!   `Double.parseDouble` (TS-26).
//!
//! The reader is strict: a text that is not exactly a form of its type
//! is refused (`None`), never repaired, so an import loses nothing
//! silently (rmp #55 DECISION 406).

use std::fmt::{self, Write as _};

use super::{Point, Value, temporal};
use crate::sync::Arc;

/// The deepest nesting of lists and records the reader accepts: deeper
/// texts are refused, so a hostile input cannot exhaust the stack.
pub(crate) const MAX_DEPTH: usize = 64;

/// A type name of TS-21 (IV008 names), as written after `lpg:`, in
/// `"$lpg_type"` and in the GraphML type attribute.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum TypeName {
    Null,
    Boolean,
    Integer,
    Float,
    String,
    Bytes,
    Date,
    LocalTime,
    ZonedTime,
    LocalDateTime,
    ZonedDateTime,
    YearToMonth,
    DayToSecond,
    Point,
    /// `LIST<T>`, or `LIST<ANY>` for `None`.
    List(Option<Box<TypeName>>),
    Record,
    Node,
    Relationship,
    Path,
}

impl TypeName {
    /// The type name of `v` (TS-21 table): a list is `LIST<T>` when it is
    /// not empty, holds no null and every element has type name `T`, and
    /// `LIST<ANY>` otherwise.
    pub(crate) fn of(v: &Value) -> TypeName {
        match v {
            Value::Null => TypeName::Null,
            Value::Bool(_) => TypeName::Boolean,
            Value::Int(_) => TypeName::Integer,
            Value::Float(_) => TypeName::Float,
            Value::String(_) => TypeName::String,
            Value::Bytes(_) => TypeName::Bytes,
            Value::Date(_) => TypeName::Date,
            Value::LocalTime(_) => TypeName::LocalTime,
            Value::ZonedTime(_) => TypeName::ZonedTime,
            Value::LocalDateTime(_) => TypeName::LocalDateTime,
            Value::ZonedDateTime(_) => TypeName::ZonedDateTime,
            Value::YearToMonth(_) => TypeName::YearToMonth,
            Value::DayToSecond(_) => TypeName::DayToSecond,
            Value::Point(_) => TypeName::Point,
            Value::List(items) => {
                let mut it = items.iter().map(TypeName::of);
                let inner = match it.next() {
                    Some(first) if first != TypeName::Null && it.all(|t| t == first) => {
                        Some(Box::new(first))
                    }
                    _ => None,
                };
                TypeName::List(inner)
            }
            Value::Record(_) => TypeName::Record,
            Value::Node(_) => TypeName::Node,
            Value::Edge(_) => TypeName::Relationship,
            Value::Path(_) => TypeName::Path,
        }
    }

    /// The type name written `text`, read case-sensitively (TS-21); `None`
    /// for any other text.
    pub(crate) fn parse(text: &str) -> Option<TypeName> {
        Some(match text {
            "NULL" => TypeName::Null,
            "BOOLEAN" => TypeName::Boolean,
            "INTEGER" => TypeName::Integer,
            "FLOAT" => TypeName::Float,
            "STRING" => TypeName::String,
            "BYTES" => TypeName::Bytes,
            "DATE" => TypeName::Date,
            "LOCAL TIME" => TypeName::LocalTime,
            "ZONED TIME" => TypeName::ZonedTime,
            "LOCAL DATETIME" => TypeName::LocalDateTime,
            "ZONED DATETIME" => TypeName::ZonedDateTime,
            "DURATION(YEAR TO MONTH)" => TypeName::YearToMonth,
            "DURATION(DAY TO SECOND)" => TypeName::DayToSecond,
            "POINT" => TypeName::Point,
            "RECORD" => TypeName::Record,
            "NODE" => TypeName::Node,
            "RELATIONSHIP" => TypeName::Relationship,
            "PATH" => TypeName::Path,
            "LIST<ANY>" => TypeName::List(None),
            _ => {
                let inner = text.strip_prefix("LIST<")?.strip_suffix('>')?;
                match TypeName::parse(inner)? {
                    TypeName::Null => return None,
                    t => TypeName::List(Some(Box::new(t))),
                }
            }
        })
    }

    /// Whether a value of this type is written in element form as its
    /// lexical form alone (TS-21 "Element form").
    fn self_describing(&self) -> bool {
        matches!(
            self,
            TypeName::Null
                | TypeName::Boolean
                | TypeName::Integer
                | TypeName::Float
                | TypeName::String
                | TypeName::Bytes
                | TypeName::List(_)
                | TypeName::Record
        )
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TypeName::Null => "NULL",
            TypeName::Boolean => "BOOLEAN",
            TypeName::Integer => "INTEGER",
            TypeName::Float => "FLOAT",
            TypeName::String => "STRING",
            TypeName::Bytes => "BYTES",
            TypeName::Date => "DATE",
            TypeName::LocalTime => "LOCAL TIME",
            TypeName::ZonedTime => "ZONED TIME",
            TypeName::LocalDateTime => "LOCAL DATETIME",
            TypeName::ZonedDateTime => "ZONED DATETIME",
            TypeName::YearToMonth => "DURATION(YEAR TO MONTH)",
            TypeName::DayToSecond => "DURATION(DAY TO SECOND)",
            TypeName::Point => "POINT",
            TypeName::Record => "RECORD",
            TypeName::Node => "NODE",
            TypeName::Relationship => "RELATIONSHIP",
            TypeName::Path => "PATH",
            TypeName::List(None) => "LIST<ANY>",
            TypeName::List(Some(t)) => return write!(f, "LIST<{t}>"),
        })
    }
}

/// The type name of `v` as text ([`TypeName::of`]).
#[cfg(test)]
pub(crate) fn type_name(v: &Value) -> String {
    TypeName::of(v).to_string()
}

/// What the lexical form of a node or an edge shows besides its
/// identifier: the labels and properties of a node, the type, endpoints
/// and properties of an edge (TS-21 node and edge rows). The export reads
/// them in its transaction; [`NoContents`] shows none.
pub(crate) trait Contents {
    /// The labels and properties of node `id`.
    fn node(&self, id: u64) -> (Vec<String>, Vec<(String, Value)>);
    /// The edge type, start node, end node and properties of edge `id`.
    fn edge(&self, id: u64) -> (String, u64, u64, Vec<(String, Value)>);
}

/// Contents that show nothing: a node without labels or properties, an
/// edge of type `''` from node 0 to node 0. The reader keeps only the
/// identifier, so these forms read back to the same value.
pub(crate) struct NoContents;

impl Contents for NoContents {
    fn node(&self, _: u64) -> (Vec<String>, Vec<(String, Value)>) {
        (Vec::new(), Vec::new())
    }

    fn edge(&self, _: u64) -> (String, u64, u64, Vec<(String, Value)>) {
        (String::new(), 0, 0, Vec::new())
    }
}

/// The text of a float (TS-21): Rust's shortest round-trip `Debug` text,
/// which always holds a `.` or an `e`; `NaN`, `Infinity`, `-Infinity`.
pub(crate) fn float_text(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        format!("{x:?}")
    }
}

/// Reads the float form (TS-21): `NaN`, `Infinity`, `-Infinity`, or a text
/// holding `.`, `e` or `E` that `f64::from_str` accepts, which returns the
/// closest representable value.
pub(crate) fn parse_float(text: &str) -> Option<f64> {
    match text {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ if text.contains(['.', 'e', 'E']) => {
            // `f64::from_str` also accepts `inf` and `NaN` spellings, none
            // of which holds `.` or `e`; it also accepts `infinity`, which
            // holds neither, so the check above refuses all of them.
            text.parse::<f64>().ok().filter(|x| x.is_finite())
        }
        _ => None,
    }
}

/// Writes `s` as a single-quoted string (TS-21): `\` as `\\`, `'` as `\'`,
/// U+0000-U+001F, U+FFFE and U+FFFF as `\uXXXX` in upper-case
/// hexadecimal; every other character as itself.
pub(crate) fn quote(s: &str, q: char, out: &mut String) {
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            '\u{0}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push(q);
}

/// The lexical form of `v`, nodes and edges shown without contents
/// ([`NoContents`]).
pub(crate) fn lexical(v: &Value) -> String {
    lexical_with(v, &NoContents)
}

/// The lexical form of `v` (TS-21 table), the contents of nodes and edges
/// from `c`.
pub(crate) fn lexical_with(v: &Value, c: &dyn Contents) -> String {
    let mut out = String::new();
    write_lexical(v, c, &mut out);
    out
}

fn write_lexical(v: &Value, c: &dyn Contents, out: &mut String) {
    match v {
        Value::Null => out.push_str("NULL"),
        Value::Bool(b) => out.push_str(if *b { "TRUE" } else { "FALSE" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(x) => out.push_str(&float_text(*x)),
        Value::String(s) => quote(s, '\'', out),
        Value::Bytes(b) => {
            out.push_str("X'");
            for byte in b.iter() {
                let _ = write!(out, "{byte:02X}");
            }
            out.push('\'');
        }
        Value::Date(_)
        | Value::LocalTime(_)
        | Value::LocalDateTime(_)
        | Value::ZonedTime(_)
        | Value::ZonedDateTime(_)
        | Value::YearToMonth(_)
        | Value::DayToSecond(_) => {
            out.push_str(&temporal::format(v).expect("a temporal value"));
        }
        Value::Point(p) => out.push_str(&p.lexical()),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_element(item, c, out);
            }
            out.push(']');
        }
        Value::Record(fields) => write_record(fields.iter().map(|(k, v)| (&**k, v)), c, out),
        Value::Node(id) => write_node(*id, c, out),
        Value::Edge(id) => {
            let (ty, start, end, _) = c.edge(*id);
            write_edge(*id, (&ty, start, end), c, out);
        }
        Value::Path(ids) => {
            out.push('[');
            for (i, id) in ids.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let mut inner = String::new();
                if i % 2 == 0 {
                    out.push_str("NODE ");
                    write_node(*id, c, &mut inner);
                } else {
                    out.push_str("RELATIONSHIP ");
                    let (ty, start, end, _) = c.edge(*id);
                    // An edge without contents takes its path neighbours
                    // as its endpoints; the reader keeps only `id`.
                    let (start, end) = if ty.is_empty() && start == 0 && end == 0 {
                        (ids[i - 1], ids.get(i + 1).copied().unwrap_or(0))
                    } else {
                        (start, end)
                    };
                    write_edge(*id, (&ty, start, end), c, &mut inner);
                }
                quote(&inner, '\'', out);
            }
            out.push(']');
        }
    }
}

/// The element form of `v` inside a list or a record (TS-21): the lexical
/// form of a self-describing type, else the type name, a space and the
/// lexical form written as a string.
fn write_element(v: &Value, c: &dyn Contents, out: &mut String) {
    let t = TypeName::of(v);
    if t.self_describing() {
        write_lexical(v, c, out);
    } else {
        let _ = write!(out, "{t} ");
        quote(&lexical_with(v, c), '\'', out);
    }
}

/// Whether `name` is written bare in a record: `[A-Za-z_][A-Za-z0-9_]*`.
fn bare_name(name: &str) -> bool {
    let mut b = name.bytes();
    b.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == b'_')
        && b.all(|x| x.is_ascii_alphanumeric() || x == b'_')
}

fn write_record<'a>(
    fields: impl Iterator<Item = (&'a str, &'a Value)>,
    c: &dyn Contents,
    out: &mut String,
) {
    out.push_str("RECORD {");
    for (i, (name, v)) in fields.enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if bare_name(name) {
            out.push_str(name);
        } else {
            quote(name, '`', out);
        }
        out.push_str(": ");
        write_element(v, c, out);
    }
    out.push('}');
}

fn write_node(id: u64, c: &dyn Contents, out: &mut String) {
    let (labels, props) = c.node(id);
    let _ = write!(out, "RECORD {{id: {id}, labels: [");
    for (i, l) in labels.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        quote(l, '\'', out);
    }
    out.push_str("], properties: ");
    write_record(props.iter().map(|(k, v)| (k.as_str(), v)), c, out);
    out.push('}');
}

fn write_edge(id: u64, (ty, start, end): (&str, u64, u64), c: &dyn Contents, out: &mut String) {
    let (_, _, _, props) = c.edge(id);
    let _ = write!(out, "RECORD {{id: {id}, type: ");
    quote(ty, '\'', out);
    let _ = write!(out, ", start: {start}, end: {end}, properties: ");
    write_record(props.iter().map(|(k, v)| (k.as_str(), v)), c, out);
    out.push('}');
}

/// `<type name>:<lexical form>`, an `lpg:annotated` CSV field (TS-26).
pub(crate) fn annotated(v: &Value, c: &dyn Contents) -> String {
    format!("{}:{}", TypeName::of(v), lexical_with(v, c))
}

/// Reads `<type name>:<lexical form>`: the first `:` ends the type name,
/// which holds none (TS-21).
#[cfg(test)]
pub(crate) fn parse_annotated(text: &str) -> Option<Value> {
    let (t, rest) = text.split_once(':')?;
    parse(&TypeName::parse(t)?, rest)
}

/// Reads the lexical form `text` of a value of type `t` (TS-21); `None`
/// if `text` is not exactly one.
pub(crate) fn parse(t: &TypeName, text: &str) -> Option<Value> {
    Some(match t {
        TypeName::Null => {
            if text.eq_ignore_ascii_case("NULL") {
                Value::Null
            } else {
                return None;
            }
        }
        TypeName::Boolean => {
            if text.eq_ignore_ascii_case("TRUE") {
                Value::Bool(true)
            } else if text.eq_ignore_ascii_case("FALSE") {
                Value::Bool(false)
            } else {
                return None;
            }
        }
        TypeName::Integer => Value::Int(text.parse().ok()?),
        TypeName::Float => Value::Float(parse_float(text)?),
        TypeName::Date => temporal::parse(&Value::Date(jiff::civil::Date::ZERO), text)?,
        TypeName::LocalTime => temporal::parse(&Value::LocalTime(jiff::civil::Time::MIN), text)?,
        TypeName::LocalDateTime => {
            temporal::parse(&Value::LocalDateTime(jiff::civil::DateTime::ZERO), text)?
        }
        TypeName::ZonedTime => temporal::parse(
            &Value::ZonedTime(super::ZonedTime {
                time: jiff::civil::Time::MIN,
                offset: 0,
            }),
            text,
        )?,
        TypeName::ZonedDateTime => {
            let like = Value::ZonedDateTime(Box::new(super::ZonedDateTime::with_offset(
                jiff::Timestamp::UNIX_EPOCH,
                jiff::tz::Offset::UTC,
            )));
            temporal::parse(&like, text)?
        }
        TypeName::YearToMonth => temporal::parse(&Value::YearToMonth(0), text)?,
        TypeName::DayToSecond => temporal::parse(
            &Value::DayToSecond(super::DayToSecond {
                seconds: 0,
                nanos: 0,
            }),
            text,
        )?,
        TypeName::Point => Value::Point(Box::new(Point::parse(text)?)),
        TypeName::String
        | TypeName::Bytes
        | TypeName::List(_)
        | TypeName::Record
        | TypeName::Node
        | TypeName::Relationship
        | TypeName::Path => {
            let mut p = Parser {
                s: text.as_bytes(),
                text,
                at: 0,
            };
            let v = p.typed(t, 0)?;
            if p.at != text.len() {
                return None;
            }
            v
        }
    })
}

/// A recursive-descent reader of the structured forms: strings, byte
/// strings, lists, records, nodes, edges and paths.
struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.at..].starts_with(lit.as_bytes()) {
            self.at += lit.len();
            true
        } else {
            false
        }
    }

    /// Eats `lit` ignoring ASCII case.
    fn eat_word(&mut self, lit: &str) -> bool {
        let end = self.at + lit.len();
        if self
            .s
            .get(self.at..end)
            .is_some_and(|b| b.eq_ignore_ascii_case(lit.as_bytes()))
        {
            self.at = end;
            true
        } else {
            false
        }
    }

    fn spaces(&mut self) {
        while self.peek() == Some(b' ') {
            self.at += 1;
        }
    }

    /// A value of type `t` in its lexical form.
    fn typed(&mut self, t: &TypeName, depth: usize) -> Option<Value> {
        match t {
            TypeName::String => Some(Value::String(Arc::from(self.quoted(b'\'')?))),
            TypeName::Bytes => self.bytes(),
            TypeName::List(inner) => {
                let items = self.list(depth)?;
                if let Some(inner) = inner {
                    // `LIST<T>`: not empty, no null, every element of T.
                    if items.is_empty() || items.iter().any(|v| TypeName::of(v) != **inner) {
                        return None;
                    }
                }
                Some(Value::List(items.into()))
            }
            TypeName::Record => self.record(depth),
            TypeName::Node => self.element_id("id", depth).map(Value::Node),
            TypeName::Relationship => self.element_id("id", depth).map(Value::Edge),
            TypeName::Path => {
                let items = self.list(depth)?;
                let mut ids = Vec::with_capacity(items.len());
                for (i, v) in items.iter().enumerate() {
                    match (i % 2, v) {
                        (0, Value::Node(id)) | (1, Value::Edge(id)) => ids.push(*id),
                        _ => return None,
                    }
                }
                if ids.len() % 2 != 1 {
                    return None;
                }
                Some(Value::Path(ids.into()))
            }
            other => {
                // A type whose form holds no nested value: read to the end.
                let rest = &self.text[self.at..];
                self.at = self.s.len();
                parse(other, rest)
            }
        }
    }

    /// The identifier field `field` of a node or edge record form.
    fn element_id(&mut self, field: &str, depth: usize) -> Option<u64> {
        let Value::Record(fields) = self.record(depth)? else {
            return None;
        };
        match fields.iter().find(|(k, _)| &**k == field)?.1 {
            Value::Int(id) => u64::try_from(id).ok(),
            _ => None,
        }
    }

    /// A string quoted by `q` (TS-21): `''`-style doubling of `q` and every
    /// GQL escape; raw CR and LF are refused, as is an escape that names no
    /// Unicode scalar value.
    fn quoted(&mut self, q: u8) -> Option<String> {
        if self.peek() != Some(q) {
            return None;
        }
        self.at += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.at..];
            let c = rest.chars().next()?;
            self.at += c.len_utf8();
            match c {
                c if c as u32 == u32::from(q) => {
                    if self.peek() == Some(q) {
                        self.at += 1;
                        out.push(c);
                    } else {
                        return Some(out);
                    }
                }
                '\r' | '\n' => return None,
                '\\' => {
                    let e = self.peek()?;
                    self.at += 1;
                    out.push(match e {
                        b'\\' => '\\',
                        b'\'' => '\'',
                        b'"' => '"',
                        b'`' => '`',
                        b't' => '\t',
                        b'b' => '\u{8}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b'f' => '\u{c}',
                        b'u' => self.hex_char(4)?,
                        b'U' => self.hex_char(6)?,
                        _ => return None,
                    });
                }
                c => out.push(c),
            }
        }
    }

    /// `n` hexadecimal digits naming a Unicode scalar value.
    fn hex_char(&mut self, n: usize) -> Option<char> {
        let digits = self.text.get(self.at..self.at + n)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.at += n;
        char::from_u32(u32::from_str_radix(digits, 16).ok()?)
    }

    /// `X'…'`: pairs of hexadecimal digits with spaces anywhere between
    /// them (`BYTE_STRING_LITERAL`).
    fn bytes(&mut self) -> Option<Value> {
        if !(self.eat("X'") || self.eat("x'")) {
            return None;
        }
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        loop {
            let b = self.peek()?;
            self.at += 1;
            match b {
                b' ' => {}
                b'\'' if high.is_none() => return Some(Value::Bytes(out.into())),
                b if b.is_ascii_hexdigit() => {
                    let d = (b as char).to_digit(16)? as u8;
                    match high.take() {
                        None => high = Some(d),
                        Some(h) => out.push(h << 4 | d),
                    }
                }
                _ => return None,
            }
        }
    }

    /// `[`, elements in element form separated by `, `, `]`; spaces
    /// around the separators are allowed.
    fn list(&mut self, depth: usize) -> Option<Vec<Value>> {
        if depth >= MAX_DEPTH || !self.eat("[") {
            return None;
        }
        let mut items = Vec::new();
        self.spaces();
        if self.eat("]") {
            return Some(items);
        }
        loop {
            self.spaces();
            items.push(self.element(depth + 1)?);
            self.spaces();
            if self.eat("]") {
                return Some(items);
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    /// `RECORD {`, fields `name: element` separated by `, `, `}`.
    fn record(&mut self, depth: usize) -> Option<Value> {
        if depth >= MAX_DEPTH || !self.eat_word("RECORD") {
            return None;
        }
        self.spaces();
        if !self.eat("{") {
            return None;
        }
        let mut fields: Vec<(Arc<str>, Value)> = Vec::new();
        self.spaces();
        if self.eat("}") {
            return Some(Value::Record(fields.into()));
        }
        loop {
            self.spaces();
            let name = if self.peek() == Some(b'`') {
                self.quoted(b'`')?
            } else {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    self.at += 1;
                }
                let n = &self.text[start..self.at];
                if !bare_name(n) {
                    return None;
                }
                n.to_owned()
            };
            if fields.iter().any(|(k, _)| **k == *name) {
                // A record's fields have distinct names (GV45).
                return None;
            }
            self.spaces();
            if !self.eat(":") {
                return None;
            }
            self.spaces();
            let v = self.element(depth + 1)?;
            fields.push((Arc::from(name), v));
            self.spaces();
            if self.eat("}") {
                return Some(Value::Record(fields.into()));
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    /// One value in element form (TS-21).
    fn element(&mut self, depth: usize) -> Option<Value> {
        match self.peek()? {
            b'\'' => self.typed(&TypeName::String, depth),
            b'[' => self.typed(&TypeName::List(None), depth),
            b'X' | b'x' if self.s.get(self.at + 1) == Some(&b'\'') => self.bytes(),
            _ => {
                if self.eat_word("RECORD") {
                    self.at -= "RECORD".len();
                    return self.record(depth);
                }
                // A number or a word, up to the next separator.
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|b| !matches!(b, b',' | b']' | b'}' | b' ' | b'\''))
                {
                    self.at += 1;
                }
                let word = &self.text[start..self.at];
                if word.eq_ignore_ascii_case("NULL") {
                    return Some(Value::Null);
                }
                if word.eq_ignore_ascii_case("TRUE") {
                    return Some(Value::Bool(true));
                }
                if word.eq_ignore_ascii_case("FALSE") {
                    return Some(Value::Bool(false));
                }
                if let Some(x) = parse_float(word) {
                    return Some(Value::Float(x));
                }
                if let Ok(i) = word.parse::<i64>() {
                    return Some(Value::Int(i));
                }
                // A typed element: the type name runs to the opening quote
                // of its lexical form, a string.
                self.at = start;
                let quote = self.text[start..].find('\'')? + start;
                let name = self.text[start..quote].strip_suffix(' ')?;
                let t = TypeName::parse(name)?;
                if t.self_describing() {
                    return None;
                }
                self.at = quote;
                let inner = self.quoted(b'\'')?;
                let mut p = Parser {
                    s: inner.as_bytes(),
                    text: &inner,
                    at: 0,
                };
                let v = p.typed(&t, depth)?;
                (p.at == inner.len()).then_some(v)
            }
        }
    }
}

/// Whether `v` can be a property value (IV011, TS-21): a boolean,
/// integer, float, string, byte string, temporal instant, duration or
/// point, or a list of one of these without null.
pub(crate) fn is_property_value(v: &Value) -> bool {
    match v {
        Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Date(_)
        | Value::LocalTime(_)
        | Value::LocalDateTime(_)
        | Value::ZonedTime(_)
        | Value::ZonedDateTime(_)
        | Value::YearToMonth(_)
        | Value::DayToSecond(_)
        | Value::Point(_) => true,
        Value::List(items) => items
            .iter()
            .all(|v| !matches!(v, Value::List(_)) && is_property_value(v)),
        Value::Null | Value::Record(_) | Value::Node(_) | Value::Edge(_) | Value::Path(_) => false,
    }
}

/// Bit equality of values: floats and point coordinates by their bits,
/// any NaN equal to any NaN; zoned date-times by instant, offset and zone
/// name; every other value by `==`, recursively. The round-trip
/// properties compare with it (rmp #56 acceptance).
#[cfg(any(test, fuzzing))]
pub(crate) fn bit_eq(a: &Value, b: &Value) -> bool {
    let f = |x: f64, y: f64| (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits();
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => f(*x, *y),
        (Value::Point(x), Value::Point(y)) => {
            x.srid() == y.srid() && x.coords().iter().zip(y.coords()).all(|(p, q)| f(*p, *q))
        }
        (Value::ZonedDateTime(x), Value::ZonedDateTime(y)) => {
            x.zoned.timestamp() == y.zoned.timestamp()
                && x.zoned.offset() == y.zoned.offset()
                && x.zone_name() == y.zone_name()
        }
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| bit_eq(p, q))
        }
        (Value::Record(x), Value::Record(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|((k, p), (l, q))| k == l && bit_eq(p, q))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests;
