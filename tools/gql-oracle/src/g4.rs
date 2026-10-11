//! A small reader of the ANTLR 4 constructs that `GQL.g4` uses: parser and
//! lexer rules, fragments, alternatives, groups, `?`, `*`, `+` (and their
//! non-greedy forms), string literals, token and fragment references,
//! character sets (with ranges, escapes, `\p{..}` and `~` negation), the
//! wildcard `.`, alternative labels `#x`, element labels `x =`, rule
//! `options { caseInsensitive = .. }` and lexer commands `-> ..`.
//!
//! Anything else is an error, so a grammar change that brings a new
//! construct fails loudly instead of being misread.

use std::collections::BTreeMap;
use std::fmt;

/// One element of a rule's right-hand side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Every child, in order.
    Seq(Vec<Node>),
    /// Exactly one child.
    Alt(Vec<Node>),
    /// The child or nothing (`?`).
    Opt(Box<Node>),
    /// Zero or more of the child (`*`).
    Star(Box<Node>),
    /// One or more of the child (`+`).
    Plus(Box<Node>),
    /// A rule reference, resolved to an index into [`Grammar::rules`].
    Ref(usize),
    /// The `EOF` token.
    Eof,
    /// A string literal (lexer) as its characters.
    Lit(String),
    /// A character set; `negated` for `~[..]`.
    Set { negated: bool, items: Vec<SetItem> },
    /// The wildcard `.`.
    Any,
}

/// One member of a character set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetItem {
    /// An inclusive range of code points (a single character is `lo == hi`).
    Range(char, char),
    /// A Unicode property `\p{Name}`.
    Property(String),
}

/// The kind of a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Lower-case name: a parser rule.
    Parser,
    /// Upper-case name: a token.
    Token,
    /// `fragment`: part of a token.
    Fragment,
}

/// One rule of the grammar.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub kind: Kind,
    pub body: Node,
    /// Whether literals and sets match case-insensitively.
    pub case_insensitive: bool,
    /// Whether the token is sent to a hidden channel or skipped (`-> ..`).
    pub hidden: bool,
}

/// A grammar: its rules in file order.
#[derive(Debug, Clone)]
pub struct Grammar {
    pub rules: Vec<Rule>,
    by_name: BTreeMap<String, usize>,
}

/// A reading error with its byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GQL.g4 byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

impl Grammar {
    /// Reads a grammar from its text.
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] on a construct the reader does not know or on an
    /// unresolved rule reference.
    pub fn parse(text: &str) -> Result<Self, Error> {
        Reader::new(text).grammar()
    }

    /// The index of the rule called `name`.
    #[must_use]
    pub fn index(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    /// The parser rules reachable from `start` through parser rules, in
    /// rule order.
    #[must_use]
    pub fn reachable(&self, start: usize) -> Vec<usize> {
        let mut seen = vec![false; self.rules.len()];
        let mut stack = vec![start];
        while let Some(r) = stack.pop() {
            if seen[r] || self.rules[r].kind != Kind::Parser {
                continue;
            }
            seen[r] = true;
            refs(&self.rules[r].body, &mut stack);
        }
        (0..self.rules.len()).filter(|&r| seen[r]).collect()
    }
}

fn refs(n: &Node, out: &mut Vec<usize>) {
    match n {
        Node::Seq(c) | Node::Alt(c) => c.iter().for_each(|x| refs(x, out)),
        Node::Opt(c) | Node::Star(c) | Node::Plus(c) => refs(c, out),
        Node::Ref(r) => out.push(*r),
        Node::Eof | Node::Lit(_) | Node::Set { .. } | Node::Any => {}
    }
}

/// Unresolved form of a node: references hold names until every rule is read.
enum Raw {
    Seq(Vec<Raw>),
    Alt(Vec<Raw>),
    Opt(Box<Raw>),
    Star(Box<Raw>),
    Plus(Box<Raw>),
    Name(String, usize),
    Lit(String),
    Set { negated: bool, items: Vec<SetItem> },
    Any,
}

struct Reader<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            s: text.as_bytes(),
            text,
            pos: 0,
        }
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        Err(Error {
            offset: self.pos,
            message: message.into(),
        })
    }

    /// Skips white space and comments.
    fn ws(&mut self) {
        loop {
            while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.s[self.pos..].starts_with(b"//") {
                while self.pos < self.s.len() && self.s[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else if self.s[self.pos..].starts_with(b"/*") {
                match self.text[self.pos + 2..].find("*/") {
                    Some(i) => self.pos += i + 4,
                    None => self.pos = self.s.len(),
                }
            } else {
                return;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.pos).copied()
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), Error> {
        if self.eat(b) {
            Ok(())
        } else {
            self.err(format!("expected '{}'", b as char))
        }
    }

    fn ident(&mut self) -> Result<String, Error> {
        self.ws();
        let start = self.pos;
        while self.pos < self.s.len()
            && (self.s[self.pos].is_ascii_alphanumeric() || self.s[self.pos] == b'_')
        {
            self.pos += 1;
        }
        if start == self.pos {
            return self.err("expected an identifier");
        }
        Ok(self.text[start..self.pos].to_owned())
    }

    /// `options { caseInsensitive = true|false; }`; returns the value.
    fn options(&mut self) -> Result<Option<bool>, Error> {
        self.expect(b'{')?;
        let mut value = None;
        while !self.eat(b'}') {
            let key = self.ident()?;
            self.expect(b'=')?;
            let v = self.ident()?;
            self.expect(b';')?;
            if key != "caseInsensitive" {
                return self.err(format!("unknown option {key}"));
            }
            value = Some(v == "true");
        }
        Ok(value)
    }

    fn grammar(&mut self) -> Result<Grammar, Error> {
        if self.ident()? != "grammar" {
            return self.err("expected 'grammar'");
        }
        self.ident()?;
        self.expect(b';')?;
        let mut grammar_ci = false;
        let mut raws = Vec::new();
        while self.peek().is_some() {
            let mut name = self.ident()?;
            if name == "options" {
                grammar_ci = self.options()?.unwrap_or(false);
                continue;
            }
            let fragment = name == "fragment";
            if fragment {
                name = self.ident()?;
            }
            let mut ci = None;
            if self.peek() == Some(b'o') {
                if self.ident()? != "options" {
                    return self.err("expected 'options' or ':'");
                }
                ci = self.options()?;
            }
            self.expect(b':')?;
            let body = self.alternatives()?;
            let mut hidden = false;
            self.ws();
            if self.s[self.pos..].starts_with(b"->") {
                self.pos += 2;
                hidden = true;
                while self.peek() != Some(b';') {
                    self.pos += 1;
                }
            }
            self.expect(b';')?;
            let kind = if fragment {
                Kind::Fragment
            } else if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                Kind::Token
            } else {
                Kind::Parser
            };
            raws.push((name, kind, body, ci.unwrap_or(grammar_ci), hidden));
        }
        let by_name: BTreeMap<String, usize> = raws
            .iter()
            .enumerate()
            .map(|(i, r)| (r.0.clone(), i))
            .collect();
        let mut rules = Vec::with_capacity(raws.len());
        for (name, kind, body, case_insensitive, hidden) in raws {
            let body = resolve(body, &by_name)?;
            rules.push(Rule {
                name,
                kind,
                body,
                case_insensitive,
                hidden,
            });
        }
        Ok(Grammar { rules, by_name })
    }

    /// `a | b | ..` up to `;`, `)` or `->`.
    fn alternatives(&mut self) -> Result<Raw, Error> {
        let mut alts = vec![self.sequence()?];
        while self.eat(b'|') {
            alts.push(self.sequence()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap_or(Raw::Seq(Vec::new()))
        } else {
            Raw::Alt(alts)
        })
    }

    fn sequence(&mut self) -> Result<Raw, Error> {
        let mut items = Vec::new();
        loop {
            match self.peek() {
                None | Some(b';' | b'|' | b')') => break,
                Some(b'-') if self.s[self.pos..].starts_with(b"->") => break,
                Some(b'#') => {
                    // Alternative label: ends the alternative.
                    self.pos += 1;
                    self.ident()?;
                }
                _ => {
                    let atom = self.element()?;
                    items.push(atom);
                }
            }
        }
        Ok(if items.len() == 1 {
            items.pop().unwrap_or(Raw::Seq(Vec::new()))
        } else {
            Raw::Seq(items)
        })
    }

    fn element(&mut self) -> Result<Raw, Error> {
        let atom = self.atom()?;
        let wrapped = match self.peek() {
            Some(b'?') => {
                self.pos += 1;
                Raw::Opt(Box::new(atom))
            }
            Some(b'*') => {
                self.pos += 1;
                Raw::Star(Box::new(atom))
            }
            Some(b'+') => {
                self.pos += 1;
                Raw::Plus(Box::new(atom))
            }
            _ => return Ok(atom),
        };
        // Non-greedy marker: the generator does not distinguish it.
        if self.s.get(self.pos) == Some(&b'?') {
            self.pos += 1;
        }
        Ok(wrapped)
    }

    fn atom(&mut self) -> Result<Raw, Error> {
        let at = self.pos;
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let inner = self.alternatives()?;
                self.expect(b')')?;
                Ok(inner)
            }
            Some(b'\'') => {
                self.pos += 1;
                Ok(Raw::Lit(self.literal()?))
            }
            Some(b'[') => {
                self.pos += 1;
                Ok(Raw::Set {
                    negated: false,
                    items: self.set()?,
                })
            }
            Some(b'~') => {
                self.pos += 1;
                if !self.eat(b'[') {
                    return self.err("only '~[..]' negation is supported");
                }
                Ok(Raw::Set {
                    negated: true,
                    items: self.set()?,
                })
            }
            Some(b'.') => {
                self.pos += 1;
                Ok(Raw::Any)
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let name = self.ident()?;
                // Element label `x =` or `x +=`.
                let save = self.pos;
                if self.eat(b'=') || (self.eat(b'+') && self.eat(b'=')) {
                    return self.atom();
                }
                self.pos = save;
                Ok(Raw::Name(name, at))
            }
            _ => self.err("unexpected character"),
        }
    }

    fn escape(&mut self) -> Result<char, Error> {
        let c = self.s.get(self.pos).copied();
        self.pos += 1;
        Ok(match c {
            Some(b'n') => '\n',
            Some(b'r') => '\r',
            Some(b't') => '\t',
            Some(b'f') => '\u{c}',
            Some(b'b') => '\u{8}',
            Some(b'u') => {
                let hex = self.text.get(self.pos..self.pos + 4).unwrap_or("");
                self.pos += 4;
                match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) => ch,
                    None => return self.err("bad \\u escape"),
                }
            }
            Some(b @ (b'\\' | b'\'' | b']' | b'-' | b'[')) => b as char,
            _ => return self.err("unknown escape"),
        })
    }

    fn literal(&mut self) -> Result<String, Error> {
        let mut out = String::new();
        loop {
            let Some(ch) = self.text[self.pos..].chars().next() else {
                return self.err("unterminated literal");
            };
            self.pos += ch.len_utf8();
            match ch {
                '\'' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c => out.push(c),
            }
        }
    }

    fn set(&mut self) -> Result<Vec<SetItem>, Error> {
        let mut items = Vec::new();
        loop {
            let Some(ch) = self.text[self.pos..].chars().next() else {
                return self.err("unterminated set");
            };
            self.pos += ch.len_utf8();
            let lo = match ch {
                ']' => return Ok(items),
                '\\' if self.s.get(self.pos) == Some(&b'p') => {
                    self.pos += 1;
                    self.expect(b'{')?;
                    let name = self.ident()?;
                    self.expect(b'}')?;
                    items.push(SetItem::Property(name));
                    continue;
                }
                '\\' => self.escape()?,
                c => c,
            };
            if self.s.get(self.pos) == Some(&b'-') && self.s.get(self.pos + 1) != Some(&b']') {
                self.pos += 1;
                let Some(h) = self.text[self.pos..].chars().next() else {
                    return self.err("unterminated range");
                };
                self.pos += h.len_utf8();
                let hi = if h == '\\' { self.escape()? } else { h };
                items.push(SetItem::Range(lo, hi));
            } else {
                items.push(SetItem::Range(lo, lo));
            }
        }
    }
}

fn resolve(raw: Raw, names: &BTreeMap<String, usize>) -> Result<Node, Error> {
    let all = |v: Vec<Raw>| {
        v.into_iter()
            .map(|x| resolve(x, names))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match raw {
        Raw::Seq(v) => Node::Seq(all(v)?),
        Raw::Alt(v) => Node::Alt(all(v)?),
        Raw::Opt(x) => Node::Opt(Box::new(resolve(*x, names)?)),
        Raw::Star(x) => Node::Star(Box::new(resolve(*x, names)?)),
        Raw::Plus(x) => Node::Plus(Box::new(resolve(*x, names)?)),
        Raw::Name(n, _) if n == "EOF" => Node::Eof,
        Raw::Name(n, at) => match names.get(&n) {
            Some(&i) => Node::Ref(i),
            None => {
                return Err(Error {
                    offset: at,
                    message: format!("undefined rule {n}"),
                });
            }
        },
        Raw::Lit(s) => Node::Lit(s),
        Raw::Set { negated, items } => Node::Set { negated, items },
        Raw::Any => Node::Any,
    })
}
