//! The bound expression form and its evaluation (rmp #42; [TS-20]
//! "Predicates use three-valued logic"; [TS-21] implementation-defined
//! choices).
//!
//! The binder ([`super::binder`]) turns every value expression into an
//! [`Expr`] whose variables are slots of the statement's row and whose
//! parameters are indexes into its parameter table: no parameter becomes a
//! literal. [`eval`] evaluates an expression over one row of a batch
//! ([`Row`]); what it needs from the graph and the statement (properties,
//! labels, `EXISTS` subqueries, the statement clock) comes through
//! [`Env`].
//!
//! * **Three-valued logic.** A boolean is `Value::Bool` or `Value::Null`
//!   (UNKNOWN). `AND`, `OR`, `NOT` and `IS [NOT] TRUE | FALSE | UNKNOWN`
//!   follow the truth tables of ISO/IEC 39075 (and SQL); filters keep only
//!   TRUE ([`is_true`]).
//! * **Comparison** (IV002, IV010; user decision 2026-10-07): values of
//!   different type groups ([`crate::types::Group`]) are not comparable,
//!   so `=` is FALSE and `<`, `<=`, `>`, `>=` are NULL; integers and floats
//!   are one group and compare exactly. Within a group the order is
//!   [`crate::types::total_cmp`]. NaN equals nothing and orders with
//!   nothing (Neo4j: `NaN = NaN` is false).
//! * **Arithmetic** (IA010 "Erro sempre", IA011, ID063–ID067): exact
//!   `INT64`; an overflow, `INT64` minimum ÷ −1 and an infinite or NaN
//!   `FLOAT64` result raise 22003; a zero divisor raises 22012; integer
//!   division truncates toward zero; an `INT64` meets a `FLOAT64` as a
//!   `FLOAT64`. A value of the wrong type raises 22G03.
//!
//! The evaluator's shape (one recursive `match` over a bound tree that
//! reads a row and an environment, errors as values) follows Oxigraph @
//! `fda6793` `lib/spareval/src/expression.rs` and Grafeo @ `3c23dbe`
//! `crates/grafeo-core/src/execution/operators/filter.rs`, studied only.
//!
//! [TS-20]: ../../../docs/technical-spec/query-engine.md
//! [TS-21]: ../../../docs/technical-spec/values-and-types.md

use std::cmp::Ordering;

use super::ast::NormalForm;
use super::functions::{self, Func};
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::sync::Arc;
use crate::types::{DayToSecond, Group, Value, total_cmp};

/// A slot of the statement's row.
pub(crate) type Slot = usize;

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// An arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// A truth value of `IS [NOT] TRUE | FALSE | UNKNOWN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    True,
    False,
    Unknown,
}

/// A label, edge type or property key: its name, and its token if the
/// token table held the name when the statement was bound. A missing token
/// is looked up again when the expression is evaluated, since an earlier
/// statement of the same transaction, or an `INSERT` of the same
/// statement, may have created it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tok {
    /// What the name names.
    pub(crate) kind: TokenKind,
    /// The name.
    pub(crate) name: Arc<str>,
    /// The token, if known at bind time.
    pub(crate) token: Option<u64>,
}

impl Tok {
    /// The token: the bound one, else the current one from `env`.
    pub(crate) fn resolve(&self, env: &dyn Env) -> Option<u64> {
        self.token.or_else(|| env.token(self.kind, &self.name))
    }
}

/// A label expression over an element's tokens: its labels, or an edge's
/// type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LabelTest {
    Label(Tok),
    And(Box<LabelTest>, Box<LabelTest>),
    Or(Box<LabelTest>, Box<LabelTest>),
    Not(Box<LabelTest>),
}

impl LabelTest {
    /// Whether an element with `tokens` satisfies the expression.
    pub(crate) fn matches(&self, tokens: &[u64], env: &dyn Env) -> bool {
        match self {
            LabelTest::Label(t) => t.resolve(env).is_some_and(|t| tokens.contains(&t)),
            LabelTest::And(a, b) => a.matches(tokens, env) && b.matches(tokens, env),
            LabelTest::Or(a, b) => a.matches(tokens, env) || b.matches(tokens, env),
            LabelTest::Not(a) => !a.matches(tokens, env),
        }
    }

    /// The labels every matching element has: the conjuncts that are
    /// plain labels (for label scans and estimates).
    pub(crate) fn required(&self) -> Vec<Option<u64>> {
        match self {
            LabelTest::Label(t) => vec![t.token],
            LabelTest::And(a, b) => {
                let mut v = a.required();
                v.extend(b.required());
                v
            }
            _ => Vec::new(),
        }
    }
}

/// A bound value expression.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    /// A literal or a folded constant.
    Lit(Value),
    /// Parameter `i` of the statement's parameter table.
    Param(usize),
    /// The value of a slot.
    Slot(Slot),
    /// A property of an element.
    Prop(Box<Expr>, Tok),
    /// Whether an element satisfies a label expression.
    Labels(Box<Expr>, LabelTest),
    /// `NOT`.
    Not(Box<Expr>),
    /// `AND`.
    And(Box<Expr>, Box<Expr>),
    /// `OR`.
    Or(Box<Expr>, Box<Expr>),
    /// `IS [NOT] TRUE | FALSE | UNKNOWN`.
    IsTruth(Box<Expr>, bool, Truth),
    /// `IS [NOT] NULL`.
    IsNull(Box<Expr>, bool),
    /// `IS [NOT] [form] NORMALIZED`.
    IsNormalized(Box<Expr>, bool, NormalForm),
    /// A comparison.
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// An arithmetic operation.
    Arith(ArithOp, Box<Expr>, Box<Expr>),
    /// Unary minus.
    Neg(Box<Expr>),
    /// Unary plus: the operand, which must be a number or a duration.
    Pos(Box<Expr>),
    /// `||` of character strings.
    Concat(Box<Expr>, Box<Expr>),
    /// A function of the function table.
    Call(Func, Vec<Expr>),
    /// A searched `CASE`: the result of the first condition that is TRUE,
    /// else the `ELSE` result.
    Case(Vec<(Expr, Expr)>, Box<Expr>),
    /// `COALESCE`.
    Coalesce(Vec<Expr>),
    /// A record constructor, only as the argument of a temporal function
    /// (`recordConstructor` inside GV39–GV41).
    Record(Vec<(Arc<str>, Expr)>),
    /// `EXISTS` subquery `i` of the statement.
    Exists(usize),
    /// `SESSION_USER` (ID061: the session's principal, a `STRING`).
    SessionUser,
}

/// What an expression reads beyond its row.
pub(crate) trait Env {
    /// Parameter `i`.
    fn param(&self, i: usize) -> Value;
    /// Property `key` of the node or edge `element`; NULL if absent.
    ///
    /// # Errors
    ///
    /// Storage errors.
    fn property(&mut self, element: &Value, key: u64) -> Result<Value, Error>;
    /// The tokens of the node or edge `element`: its labels, or its type.
    ///
    /// # Errors
    ///
    /// Storage errors.
    fn tokens(&mut self, element: &Value) -> Result<Vec<u64>, Error>;
    /// Whether `EXISTS` subquery `sub` has a row for `row`.
    ///
    /// # Errors
    ///
    /// The subquery's errors.
    fn exists(&mut self, sub: usize, row: &[Value]) -> Result<bool, Error>;
    /// The statement's clock: the instant every current-time function of
    /// one statement reads.
    fn now(&self) -> jiff::Timestamp;
    /// The session's principal (ID061).
    fn principal(&self) -> &str;
    /// The token of `name` of `kind`, if the token table holds it.
    fn token(&self, kind: TokenKind, name: &str) -> Option<u64>;
}

/// An environment without graph, parameters or subqueries: evaluation of
/// constant expressions (folding) and of the expression tests.
pub(crate) struct NoEnv {
    pub(crate) now: jiff::Timestamp,
}

impl Env for NoEnv {
    fn param(&self, _: usize) -> Value {
        Value::Null
    }
    fn property(&mut self, _: &Value, _: u64) -> Result<Value, Error> {
        Err(internal("a property read without a graph"))
    }
    fn tokens(&mut self, _: &Value) -> Result<Vec<u64>, Error> {
        Err(internal("a label read without a graph"))
    }
    fn exists(&mut self, _: usize, _: &[Value]) -> Result<bool, Error> {
        Err(internal("a subquery without a graph"))
    }
    fn now(&self) -> jiff::Timestamp {
        self.now
    }
    fn principal(&self) -> &str {
        ""
    }
    fn token(&self, _: TokenKind, _: &str) -> Option<u64> {
        None
    }
}

/// An internal error (50N00).
pub(crate) fn internal(what: &str) -> Error {
    Error::new(codes::C50N00, format!("internal error: {what}"))
}

/// The 22G03 "invalid value type" error.
pub(crate) fn invalid_type(what: &str) -> Error {
    Error::new(codes::C22G03, format!("invalid value type: {what}"))
}

/// The 22003 "numeric value out of range" error.
pub(crate) fn out_of_range(what: &str) -> Error {
    Error::new(codes::C22003, format!("numeric value out of range: {what}"))
}

/// The 22012 "division by zero" error.
pub(crate) fn division_by_zero() -> Error {
    Error::new(codes::C22012, "division by zero")
}

/// The type name of a value, for messages (IV008).
pub(crate) fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NULL",
        Value::Bool(_) => "BOOLEAN",
        Value::Int(_) => "INTEGER",
        Value::Float(_) => "FLOAT",
        Value::Node(_) => "NODE",
        Value::Edge(_) => "RELATIONSHIP",
        Value::Date(_) => "DATE",
        Value::LocalTime(_) => "LOCAL TIME",
        Value::LocalDateTime(_) => "LOCAL DATETIME",
        Value::ZonedTime(_) => "ZONED TIME",
        Value::ZonedDateTime(_) => "ZONED DATETIME",
        Value::YearToMonth(_) => "DURATION(YEAR TO MONTH)",
        Value::DayToSecond(_) => "DURATION(DAY TO SECOND)",
        Value::String(_) => "STRING",
        Value::Bytes(_) => "BYTES",
        Value::List(_) => "LIST",
        Value::Record(_) => "RECORD",
        Value::Path(_) => "PATH",
        Value::Point(_) => "POINT",
    }
}

/// Whether a filter keeps a row: only TRUE.
pub(crate) fn is_true(v: &Value) -> bool {
    matches!(v, Value::Bool(true))
}

/// The truth of a search condition's value: TRUE keeps the row, FALSE and
/// NULL (unknown) drop it.
///
/// # Errors
///
/// 22G03, invalid value type, for a value that is not a boolean
/// (gql-compat `condition/22g03/non-boolean-search-condition`, ISO
/// subclauses 19.1 and 16.13).
pub(crate) fn condition(v: &Value) -> Result<bool, Error> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Null => Ok(false),
        other => Err(invalid_type(&format!(
            "a search condition of {}",
            type_name(other)
        ))),
    }
}

/// A truth value: `Some(b)`, or `None` for UNKNOWN (NULL).
fn truth(v: Value, op: &str) -> Result<Option<bool>, Error> {
    match v {
        Value::Bool(b) => Ok(Some(b)),
        Value::Null => Ok(None),
        other => Err(invalid_type(&format!(
            "{op} needs BOOLEAN operands, not {}",
            type_name(&other)
        ))),
    }
}

fn of_truth(t: Option<bool>) -> Value {
    t.map_or(Value::Null, Value::Bool)
}

/// `a AND b` in three-valued logic.
pub(crate) fn and3(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

/// `a OR b` in three-valued logic.
pub(crate) fn or3(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// `a = b`: `None` for UNKNOWN (IV002).
pub(crate) fn equals(a: &Value, b: &Value) -> Option<bool> {
    match (a, b) {
        (Value::Null, _) | (_, Value::Null) => None,
        (Value::Float(x), _) | (_, Value::Float(x)) if x.is_nan() => Some(false),
        _ if a.group() != b.group() => Some(false),
        (Value::List(x), Value::List(y)) => {
            if x.len() != y.len() {
                return Some(false);
            }
            let mut acc = Some(true);
            for (p, q) in x.iter().zip(y.iter()) {
                acc = and3(acc, equals(p, q));
                if acc == Some(false) {
                    break;
                }
            }
            acc
        }
        (Value::Record(x), Value::Record(y)) => {
            if x.len() != y.len() {
                return Some(false);
            }
            let mut acc = Some(true);
            for (name, p) in x.iter() {
                match y.iter().find(|(n, _)| n == name) {
                    None => return Some(false),
                    Some((_, q)) => acc = and3(acc, equals(p, q)),
                }
            }
            acc
        }
        _ => Some(total_cmp(a, b) == Ordering::Equal),
    }
}

/// Whether a value holds a NULL anywhere.
fn holds_null(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::List(l) => l.iter().any(holds_null),
        _ => false,
    }
}

/// The order of `a` and `b` for `<`, `<=`, `>`, `>=`: `None` for UNKNOWN.
/// `Some(None)` when either is NaN: every such comparison is FALSE.
fn order(a: &Value, b: &Value) -> Option<Option<Ordering>> {
    if a.is_null() || b.is_null() || a.group() != b.group() {
        return None;
    }
    match (a, b) {
        (Value::Float(x), _) | (_, Value::Float(x)) if x.is_nan() => Some(None),
        (Value::YearToMonth(_), Value::DayToSecond(_))
        | (Value::DayToSecond(_), Value::YearToMonth(_)) => None,
        _ => match a.group() {
            Group::Number
            | Group::String
            | Group::Bytes
            | Group::Boolean
            | Group::Date
            | Group::LocalTime
            | Group::LocalDateTime
            | Group::ZonedTime
            | Group::ZonedDateTime
            | Group::Duration => Some(Some(total_cmp(a, b))),
            Group::List if !holds_null(a) && !holds_null(b) => Some(Some(total_cmp(a, b))),
            _ => None,
        },
    }
}

/// A comparison in three-valued logic.
pub(crate) fn compare(op: CmpOp, a: &Value, b: &Value) -> Value {
    let t = match op {
        CmpOp::Eq => equals(a, b),
        CmpOp::Ne => equals(a, b).map(|e| !e),
        _ => order(a, b).map(|o| {
            o.is_some_and(|o| match op {
                CmpOp::Lt => o == Ordering::Less,
                CmpOp::Le => o != Ordering::Greater,
                CmpOp::Gt => o == Ordering::Greater,
                CmpOp::Ge => o != Ordering::Less,
                CmpOp::Eq | CmpOp::Ne => unreachable!("handled above"),
            })
        }),
    };
    of_truth(t)
}

/// A finite `FLOAT64` result, else 22003 (IA010: GA01 not supported).
pub(crate) fn finite(f: f64) -> Result<Value, Error> {
    if f.is_finite() {
        Ok(Value::Float(f))
    } else {
        Err(out_of_range("a FLOAT64 result that is infinite or NaN"))
    }
}

/// `a op b` (IA010, IA011, ID063–ID067; temporal arithmetic of GV39–GV41).
///
/// # Errors
///
/// 22003, 22012, 22G03, 22008 as described in the module documentation.
pub(crate) fn arith(op: ArithOp, a: &Value, b: &Value) -> Result<Value, Error> {
    use Value as V;
    if a.is_null() || b.is_null() {
        return Ok(V::Null);
    }
    match (a, b) {
        (V::Int(x), V::Int(y)) => {
            let (x, y) = (*x, *y);
            let r = match op {
                ArithOp::Add => x.checked_add(y),
                ArithOp::Sub => x.checked_sub(y),
                ArithOp::Mul => x.checked_mul(y),
                ArithOp::Div => {
                    if y == 0 {
                        return Err(division_by_zero());
                    }
                    x.checked_div(y)
                }
            };
            r.map(V::Int)
                .ok_or_else(|| out_of_range("an INT64 result outside its range"))
        }
        (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => {
            let (x, y) = (as_f64(a), as_f64(b));
            let r = match op {
                ArithOp::Add => x + y,
                ArithOp::Sub => x - y,
                ArithOp::Mul => x * y,
                ArithOp::Div => {
                    if y == 0.0 {
                        return Err(division_by_zero());
                    }
                    x / y
                }
            };
            finite(r)
        }
        _ => functions::temporal_arith(op, a, b),
    }
}

/// A number as `f64` (ID063: an `INT64` operand is converted, round to
/// nearest, ties to even, IA005).
pub(crate) fn as_f64(v: &Value) -> f64 {
    match *v {
        Value::Int(i) => i as f64,
        Value::Float(f) => f,
        _ => f64::NAN,
    }
}

/// Unary minus.
///
/// # Errors
///
/// 22003 for `−INT64` minimum; 22G03 for a value that is not a number or
/// a duration.
pub(crate) fn negate(v: &Value) -> Result<Value, Error> {
    match v {
        Value::Null => Ok(Value::Null),
        Value::Int(i) => i
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| out_of_range("the negation of the INT64 minimum")),
        Value::Float(f) => Ok(Value::Float(-f)),
        Value::YearToMonth(m) => m
            .checked_neg()
            .map(Value::YearToMonth)
            .ok_or_else(|| Error::new(codes::C22015, "interval field overflow")),
        Value::DayToSecond(d) => negate_d2s(*d).map(Value::DayToSecond),
        other => Err(invalid_type(&format!(
            "unary minus of {}",
            type_name(other)
        ))),
    }
}

/// The negation of a day-to-second duration.
pub(crate) fn negate_d2s(d: DayToSecond) -> Result<DayToSecond, Error> {
    d.seconds
        .checked_neg()
        .and_then(|s| DayToSecond::new(s, -i64::from(d.nanos)))
        .ok_or_else(|| Error::new(codes::C22015, "interval field overflow"))
}

/// `a || b` of character strings (IW017; IL009: longer than 2³² − 1 bytes
/// raises 22001).
///
/// # Errors
///
/// As above; 22G03 for operands that are not character strings.
pub(crate) fn concat(a: &Value, b: &Value) -> Result<Value, Error> {
    match (a, b) {
        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
        (Value::String(x), Value::String(y)) => {
            if x.len() + y.len() > u32::MAX as usize {
                return Err(Error::new(
                    codes::C22001,
                    "string data, right truncation: a concatenation longer than 4 294 967 295 bytes",
                ));
            }
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(x);
            s.push_str(y);
            Ok(Value::string(&s))
        }
        _ => Err(invalid_type(&format!(
            "|| of {} and {}",
            type_name(a),
            type_name(b)
        ))),
    }
}

/// Evaluates `e` over `row`.
///
/// # Errors
///
/// The data exceptions of the operators and functions; the errors of
/// `env`.
pub(crate) fn eval(e: &Expr, row: &[Value], env: &mut dyn Env) -> Result<Value, Error> {
    Ok(match e {
        Expr::Lit(v) => v.clone(),
        Expr::Param(i) => env.param(*i),
        Expr::Slot(s) => row.get(*s).cloned().unwrap_or(Value::Null),
        Expr::Prop(src, key) => {
            let el = eval(src, row, env)?;
            match (&el, key.resolve(env)) {
                (Value::Null, _) => Value::Null,
                (Value::Node(_) | Value::Edge(_), None) => Value::Null,
                (Value::Node(_) | Value::Edge(_), Some(k)) => env.property(&el, k)?,
                (other, _) => {
                    return Err(invalid_type(&format!("a property of {}", type_name(other))));
                }
            }
        }
        Expr::Labels(src, test) => {
            let el = eval(src, row, env)?;
            match &el {
                Value::Null => Value::Null,
                Value::Node(_) | Value::Edge(_) => {
                    let tokens = env.tokens(&el)?;
                    Value::Bool(test.matches(&tokens, env))
                }
                other => {
                    return Err(invalid_type(&format!(
                        "a label test of {}",
                        type_name(other)
                    )));
                }
            }
        }
        Expr::Not(x) => of_truth(truth(eval(x, row, env)?, "NOT")?.map(|b| !b)),
        Expr::And(a, b) => {
            let l = truth(eval(a, row, env)?, "AND")?;
            if l == Some(false) {
                return Ok(Value::Bool(false));
            }
            of_truth(and3(l, truth(eval(b, row, env)?, "AND")?))
        }
        Expr::Or(a, b) => {
            let l = truth(eval(a, row, env)?, "OR")?;
            if l == Some(true) {
                return Ok(Value::Bool(true));
            }
            of_truth(or3(l, truth(eval(b, row, env)?, "OR")?))
        }
        Expr::IsTruth(x, not, t) => {
            let v = truth(eval(x, row, env)?, "IS")?;
            let is = match t {
                Truth::True => v == Some(true),
                Truth::False => v == Some(false),
                Truth::Unknown => v.is_none(),
            };
            Value::Bool(is != *not)
        }
        Expr::IsNull(x, not) => Value::Bool(eval(x, row, env)?.is_null() != *not),
        Expr::IsNormalized(x, not, form) => match eval(x, row, env)? {
            Value::Null => Value::Null,
            Value::String(s) => Value::Bool(super::unicode::is_normalized(&s, *form) != *not),
            other => {
                return Err(invalid_type(&format!(
                    "IS NORMALIZED of {}",
                    type_name(&other)
                )));
            }
        },
        Expr::Cmp(op, a, b) => {
            let (l, r) = (eval(a, row, env)?, eval(b, row, env)?);
            compare(*op, &l, &r)
        }
        Expr::Arith(op, a, b) => {
            let (l, r) = (eval(a, row, env)?, eval(b, row, env)?);
            arith(*op, &l, &r)?
        }
        Expr::Neg(x) => negate(&eval(x, row, env)?)?,
        Expr::Pos(x) => match eval(x, row, env)? {
            v @ (Value::Null
            | Value::Int(_)
            | Value::Float(_)
            | Value::YearToMonth(_)
            | Value::DayToSecond(_)) => v,
            other => {
                return Err(invalid_type(&format!(
                    "unary plus of {}",
                    type_name(&other)
                )));
            }
        },
        Expr::Concat(a, b) => {
            let (l, r) = (eval(a, row, env)?, eval(b, row, env)?);
            concat(&l, &r)?
        }
        Expr::Call(f, args) => {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(eval(a, row, env)?);
            }
            functions::call(*f, &vals, env.now())?
        }
        Expr::Case(whens, otherwise) => {
            for (c, r) in whens {
                if is_true(&eval(c, row, env)?) {
                    return eval(r, row, env);
                }
            }
            eval(otherwise, row, env)?
        }
        Expr::Coalesce(xs) => {
            for x in xs {
                let v = eval(x, row, env)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Value::Null
        }
        Expr::Record(fields) => {
            let mut out = Vec::with_capacity(fields.len());
            for (n, x) in fields {
                out.push((n.clone(), eval(x, row, env)?));
            }
            Value::Record(Arc::from(out))
        }
        Expr::Exists(i) => Value::Bool(env.exists(*i, row)?),
        Expr::SessionUser => Value::string(env.principal()),
    })
}

impl Value {
    /// Whether the value is NULL.
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl Expr {
    /// Calls `f` on every sub-expression, this one first.
    pub(crate) fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Lit(_) | Expr::Param(_) | Expr::Slot(_) | Expr::Exists(_) | Expr::SessionUser => {
            }
            Expr::Prop(x, _)
            | Expr::Labels(x, _)
            | Expr::Not(x)
            | Expr::IsTruth(x, _, _)
            | Expr::IsNull(x, _)
            | Expr::IsNormalized(x, _, _)
            | Expr::Neg(x)
            | Expr::Pos(x) => x.walk(f),
            Expr::And(a, b)
            | Expr::Or(a, b)
            | Expr::Cmp(_, a, b)
            | Expr::Arith(_, a, b)
            | Expr::Concat(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Expr::Call(_, xs) | Expr::Coalesce(xs) => xs.iter().for_each(|x| x.walk(f)),
            Expr::Case(ws, e) => {
                for (c, r) in ws {
                    c.walk(f);
                    r.walk(f);
                }
                e.walk(f);
            }
            Expr::Record(fs) => fs.iter().for_each(|(_, x)| x.walk(f)),
        }
    }

    /// The slots the expression reads.
    pub(crate) fn slots(&self) -> Vec<Slot> {
        let mut out = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Slot(s) = e
                && !out.contains(s)
            {
                out.push(*s);
            }
        });
        out
    }

    /// Whether the expression reads an `EXISTS` subquery.
    pub(crate) fn has_subquery(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| found |= matches!(e, Expr::Exists(_)));
        found
    }

    /// Whether the value depends only on the expression's literals:
    /// no slot, parameter, subquery, session or clock.
    pub(crate) fn is_constant(&self) -> bool {
        let mut constant = true;
        self.walk(&mut |e| match e {
            Expr::Param(_)
            | Expr::Slot(_)
            | Expr::Exists(_)
            | Expr::SessionUser
            | Expr::Prop(..)
            | Expr::Labels(..) => constant = false,
            Expr::Call(f, _) if !functions::row(*f).deterministic => constant = false,
            _ => {}
        });
        constant
    }

    /// The conjuncts of an `AND` tree.
    pub(crate) fn conjuncts(self) -> Vec<Expr> {
        match self {
            Expr::And(a, b) => {
                let mut v = a.conjuncts();
                v.extend(b.conjuncts());
                v
            }
            e => vec![e],
        }
    }

    /// Folds constant sub-expressions into literals (rmp #44 "constant
    /// folding"). A sub-expression whose evaluation fails is kept, so the
    /// error is raised when, and only if, a row evaluates it.
    #[must_use]
    pub(crate) fn fold(self) -> Expr {
        if !matches!(self, Expr::Lit(_)) && self.is_constant() {
            let mut env = NoEnv {
                now: jiff::Timestamp::UNIX_EPOCH,
            };
            if let Ok(v) = eval(&self, &[], &mut env) {
                return Expr::Lit(v);
            }
            return self;
        }
        let b = |x: Box<Expr>| Box::new(x.fold());
        match self {
            Expr::Prop(x, k) => Expr::Prop(b(x), k),
            Expr::Labels(x, t) => Expr::Labels(b(x), t),
            Expr::Not(x) => Expr::Not(b(x)),
            Expr::And(x, y) => Expr::And(b(x), b(y)),
            Expr::Or(x, y) => Expr::Or(b(x), b(y)),
            Expr::IsTruth(x, n, t) => Expr::IsTruth(b(x), n, t),
            Expr::IsNull(x, n) => Expr::IsNull(b(x), n),
            Expr::IsNormalized(x, n, f) => Expr::IsNormalized(b(x), n, f),
            Expr::Cmp(o, x, y) => Expr::Cmp(o, b(x), b(y)),
            Expr::Arith(o, x, y) => Expr::Arith(o, b(x), b(y)),
            Expr::Neg(x) => Expr::Neg(b(x)),
            Expr::Pos(x) => Expr::Pos(b(x)),
            Expr::Concat(x, y) => Expr::Concat(b(x), b(y)),
            Expr::Call(f, xs) => Expr::Call(f, xs.into_iter().map(Expr::fold).collect()),
            Expr::Coalesce(xs) => Expr::Coalesce(xs.into_iter().map(Expr::fold).collect()),
            Expr::Case(ws, e) => Expr::Case(
                ws.into_iter().map(|(c, r)| (c.fold(), r.fold())).collect(),
                b(e),
            ),
            Expr::Record(fs) => Expr::Record(fs.into_iter().map(|(n, x)| (n, x.fold())).collect()),
            e => e,
        }
    }
}

/// The rows' order of `ORDER BY` (IS001): the total order, NULL last
/// ascending; descending reverses it.
pub(crate) fn sort_cmp(a: &Value, b: &Value, descending: bool) -> Ordering {
    let o = total_cmp(a, b);
    if descending { o.reverse() } else { o }
}

#[cfg(test)]
pub(crate) mod tests;
