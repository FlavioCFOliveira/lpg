//! Expected values of the TCK result tables and their comparison with
//! lpg's values (own code).
//!
//! A cell is a value literal of the openCypher TCK notation: `null`,
//! booleans, integers, floats, `'strings'`, lists `[…]`, maps `{k: v}`,
//! nodes `(:L {k: v})` and relationships `[:T {k: v}]`. A temporal value is
//! written as a string, as the openCypher TCK does; it equals an lpg
//! temporal value when the string parses, as a value of that type, to an
//! equal value.

use std::collections::BTreeMap;

use crate::types::Value;
use crate::types::temporal;

/// An expected value.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Expected {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Expected>),
    Map(BTreeMap<String, Expected>),
    Node {
        labels: Vec<String>,
        props: BTreeMap<String, Expected>,
    },
    Edge {
        types: Vec<String>,
        props: BTreeMap<String, Expected>,
    },
}

/// The labels and properties of an element, for comparing nodes and
/// relationships.
pub(super) type Element = (Vec<String>, BTreeMap<String, Value>);

/// Parses the cell `text`.
///
/// # Errors
///
/// A message for a cell that is not a value literal.
pub(super) fn parse(text: &str) -> Result<Expected, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        at: 0,
        text,
    };
    let v = p.value()?;
    p.ws();
    if p.at != p.s.len() {
        return Err(format!("trailing text in {text:?} at {}", p.at));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    text: &'a str,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.at < self.s.len() && self.s[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(format!(
                "expected '{}' in {:?} at {}",
                c as char, self.text, self.at
            ))
        }
    }

    fn word(&mut self) -> String {
        self.ws();
        let start = self.at;
        while self.at < self.s.len()
            && (self.s[self.at].is_ascii_alphanumeric() || self.s[self.at] == b'_')
        {
            self.at += 1;
        }
        self.text[start..self.at].to_string()
    }

    /// A map key or a label: a word or a backquoted name.
    fn name(&mut self) -> Result<String, String> {
        if self.eat(b'`') {
            let start = self.at;
            while self.at < self.s.len() && self.s[self.at] != b'`' {
                self.at += 1;
            }
            let n = self.text[start..self.at].to_string();
            self.expect(b'`')?;
            return Ok(n);
        }
        let w = self.word();
        if w.is_empty() {
            return Err(format!("expected a name in {:?} at {}", self.text, self.at));
        }
        Ok(w)
    }

    fn props(&mut self) -> Result<BTreeMap<String, Expected>, String> {
        let mut m = BTreeMap::new();
        self.expect(b'{')?;
        if self.eat(b'}') {
            return Ok(m);
        }
        loop {
            let k = self.name()?;
            self.expect(b':')?;
            let v = self.value()?;
            m.insert(k, v);
            if self.eat(b'}') {
                return Ok(m);
            }
            self.expect(b',')?;
        }
    }

    /// `:A:B` names, then optional properties, up to `close`.
    fn element(&mut self, close: u8) -> Result<(Vec<String>, BTreeMap<String, Expected>), String> {
        let mut names = Vec::new();
        while self.eat(b':') {
            names.push(self.name()?);
        }
        let props = if self.peek() == Some(b'{') {
            self.props()?
        } else {
            BTreeMap::new()
        };
        self.expect(close)?;
        Ok((names, props))
    }

    fn string(&mut self) -> Result<String, String> {
        let quote = self.s[self.at];
        self.at += 1;
        let mut out = String::new();
        let rest = &self.text[self.at..];
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            if c == quote as char {
                self.at += i + 1;
                return Ok(out);
            }
            if c == '\\' {
                match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 't')) => out.push('\t'),
                    Some((_, 'r')) => out.push('\r'),
                    Some((_, 'b')) => out.push('\u{8}'),
                    Some((_, 'f')) => out.push('\u{c}'),
                    Some((_, 'u')) => {
                        let hex: String = chars.by_ref().take(4).map(|(_, c)| c).collect();
                        let code = u32::from_str_radix(&hex, 16)
                            .map_err(|_| format!("bad \\u escape in {:?}", self.text))?;
                        out.push(char::from_u32(code).ok_or("bad \\u escape")?);
                    }
                    Some((_, other)) => out.push(other),
                    None => break,
                }
            } else {
                out.push(c);
            }
        }
        Err(format!("unterminated string in {:?}", self.text))
    }

    fn number(&mut self) -> Result<Expected, String> {
        let start = self.at;
        if matches!(self.s.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        let rest = &self.text[self.at..];
        for special in ["NaN", "Infinity", "Inf"] {
            if rest.starts_with(special) {
                self.at += special.len();
                let neg = self.s[start] == b'-';
                return Ok(Expected::Float(match special {
                    "NaN" => f64::NAN,
                    _ if neg => f64::NEG_INFINITY,
                    _ => f64::INFINITY,
                }));
            }
        }
        let mut float = false;
        while self.at < self.s.len() {
            match self.s[self.at] {
                b'0'..=b'9' => {}
                b'.' | b'e' | b'E' => float = true,
                b'-' | b'+' if matches!(self.s[self.at - 1], b'e' | b'E') => {}
                _ => break,
            }
            self.at += 1;
        }
        let t = &self.text[start..self.at];
        if float {
            t.parse::<f64>()
                .map(Expected::Float)
                .map_err(|e| format!("{t:?}: {e}"))
        } else {
            t.parse::<i64>()
                .map(Expected::Int)
                .map_err(|e| format!("{t:?}: {e}"))
        }
    }

    fn value(&mut self) -> Result<Expected, String> {
        match self.peek() {
            Some(b'\'' | b'"') => self.string().map(Expected::String),
            Some(b'[') => {
                self.at += 1;
                // A relationship `[:T …]`, or a list.
                if self.peek() == Some(b':') {
                    let (types, props) = self.element(b']')?;
                    return Ok(Expected::Edge { types, props });
                }
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Ok(Expected::List(items));
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(b']') {
                        return Ok(Expected::List(items));
                    }
                    self.expect(b',')?;
                }
            }
            Some(b'{') => self.props().map(Expected::Map),
            Some(b'(') => {
                self.at += 1;
                let (labels, props) = self.element(b')')?;
                Ok(Expected::Node { labels, props })
            }
            Some(b'0'..=b'9' | b'-' | b'+' | b'.') => self.number(),
            Some(_) => {
                let w = self.word();
                match w.to_ascii_lowercase().as_str() {
                    "null" => Ok(Expected::Null),
                    "true" => Ok(Expected::Bool(true)),
                    "false" => Ok(Expected::Bool(false)),
                    "nan" => Ok(Expected::Float(f64::NAN)),
                    _ => Err(format!("unknown value {w:?} in {:?}", self.text)),
                }
            }
            None => Err(format!("empty value in {:?}", self.text)),
        }
    }
}

/// The months and nanoseconds of the ISO 8601 duration `text`, as the
/// openCypher TCK writes them: `P[nY][nM][nW][nD][T[nH][nM][n[.f]S]]`, with
/// an optional leading minus sign.
fn duration(text: &str) -> Option<(i128, i128)> {
    let (neg, body) = text.strip_prefix('-').map_or((false, text), |b| (true, b));
    let body = body.strip_prefix('P')?;
    let (date, time) = body.split_once('T').unwrap_or((body, ""));
    let (mut months, mut nanos) = (0i128, 0i128);
    let mut number = String::new();
    for c in date.chars() {
        match c {
            '0'..='9' => number.push(c),
            'Y' | 'M' | 'W' | 'D' => {
                let n: i128 = std::mem::take(&mut number).parse().ok()?;
                match c {
                    'Y' => months += n * 12,
                    'M' => months += n,
                    'W' => nanos += n * 7 * 86_400_000_000_000,
                    _ => nanos += n * 86_400_000_000_000,
                }
            }
            _ => return None,
        }
    }
    for c in time.chars() {
        match c {
            '0'..='9' | '.' => number.push(c),
            'H' | 'M' | 'S' => {
                let n = std::mem::take(&mut number);
                let (whole, frac) = n.split_once('.').unwrap_or((&n, ""));
                let whole: i128 = whole.parse().ok()?;
                // Only seconds carry a fraction, of at most nine digits.
                let frac: i128 = match (frac.is_empty(), c) {
                    (true, _) => 0,
                    (false, 'S') if frac.len() <= 9 => format!("{frac:0<9}").parse().ok()?,
                    _ => return None,
                };
                nanos += match c {
                    'H' => whole * 3_600_000_000_000,
                    'M' => whole * 60_000_000_000,
                    _ => whole * 1_000_000_000 + frac,
                };
            }
            _ => return None,
        }
    }
    if !number.is_empty() {
        return None;
    }
    Some(if neg {
        (-months, -nanos)
    } else {
        (months, nanos)
    })
}

/// Whether `actual` equals `expected`. `element` gives the labels and
/// properties of a node or edge identifier.
pub(super) fn equals(
    expected: &Expected,
    actual: &Value,
    element: &mut dyn FnMut(&Value) -> Option<Element>,
) -> bool {
    match (expected, actual) {
        (Expected::Null, Value::Null) => true,
        (Expected::Bool(a), Value::Bool(b)) => a == b,
        (Expected::Int(a), Value::Int(b)) => a == b,
        (Expected::Float(a), Value::Float(b)) => a == b || (a.is_nan() && b.is_nan()),
        (Expected::String(a), Value::String(b)) => a.as_str() == &**b,
        (Expected::String(a), Value::YearToMonth(m)) => duration(a) == Some((i128::from(*m), 0)),
        (Expected::String(a), Value::DayToSecond(d)) => {
            let nanos = i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos);
            duration(a) == Some((0, nanos))
        }
        (Expected::String(a), v) => temporal::parse(v, a).is_some_and(|e| &e == v),
        (Expected::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| equals(x, y, element))
        }
        (Expected::Map(a), Value::Record(b)) => {
            a.len() == b.len()
                && b.iter()
                    .all(|(k, v)| a.get(&**k).is_some_and(|x| equals(x, v, element)))
        }
        (Expected::Node { labels, props }, v @ Value::Node(_))
        | (
            Expected::Edge {
                types: labels,
                props,
            },
            v @ Value::Edge(_),
        ) => element(v).is_some_and(|(got_labels, got_props)| {
            let mut want: Vec<&String> = labels.iter().collect();
            want.sort();
            let mut got: Vec<&String> = got_labels.iter().collect();
            got.sort();
            want == got
                && props.len() == got_props.len()
                && got_props
                    .iter()
                    .all(|(k, v)| props.get(k).is_some_and(|x| equals(x, v, element)))
        }),
        _ => false,
    }
}
