//! Binding of value expressions, literals and functions (rmp #43, with
//! the expression form of #42).

use super::Binder;
use super::statements::{rule, undefined};
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::aggregate::AggKind;
use crate::query::ast;
use crate::query::bound::{Name, Part, VarKind};
use crate::query::expr::{ArithOp, CmpOp, Expr, LabelTest, Slot, Tok, Truth, invalid_type};
use crate::query::features::{gate, unsupported};
use crate::query::functions::Func;
use crate::query::span::Spanned;
use crate::sync::Arc;
use crate::types::{Value, ZonedDateTime, ZonedTime, temporal};

/// Where an expression is bound: whether aggregates may appear, and the
/// aggregates found so far.
pub(super) struct ExprCtx<'x> {
    aggs: Option<&'x mut Vec<(Slot, AggKind, bool, Expr)>>,
    in_aggregate: bool,
    /// Whether the expression holds an aggregate.
    pub(super) saw_aggregate: bool,
}

impl<'x> ExprCtx<'x> {
    /// A context where aggregates are collected into `aggs`.
    pub(super) fn aggregating(aggs: &'x mut Vec<(Slot, AggKind, bool, Expr)>) -> ExprCtx<'x> {
        ExprCtx {
            aggs: Some(aggs),
            in_aggregate: false,
            saw_aggregate: false,
        }
    }

    /// A context without aggregates.
    pub(super) fn plain() -> ExprCtx<'static> {
        ExprCtx {
            aggs: None,
            in_aggregate: false,
            saw_aggregate: false,
        }
    }
}

/// The value of a decimal integer literal, underscores removed (IL010:
/// one above `INT64` maximum raises 22003).
fn integer(text: &str) -> Result<i64, Error> {
    let digits: String = text.chars().filter(|c| *c != '_').collect();
    digits
        .parse::<i64>()
        .map_err(|_| Error::new(codes::C22003, format!("numeric value out of range: {text}")))
}

/// The literal `9 223 372 036 854 775 808`, whose negation is `INT64`
/// minimum (IL010).
fn is_min_magnitude(e: &ast::Expr) -> bool {
    match e {
        ast::Expr::Literal(ast::Literal::Numeric(n)) => {
            n.kind == ast::NumericLiteralKind::Integer(10)
                && n.text.chars().filter(|c| *c != '_').collect::<String>() == "9223372036854775808"
        }
        _ => false,
    }
}

impl Binder<'_> {
    /// Binds a value expression where aggregates are not allowed, and
    /// folds its constant parts.
    pub(super) fn bind_value(&mut self, e: &Spanned<ast::Expr>) -> Result<Expr, Error> {
        let mut ctx = ExprCtx::plain();
        let x = self.at(e.1, |b| b.bind_expr(&e.0, &mut ctx))?;
        Ok(x.fold())
    }

    fn sub(&mut self, e: &Spanned<ast::Expr>, ctx: &mut ExprCtx<'_>) -> Result<Box<Expr>, Error> {
        let x = self.at(e.1, |b| b.bind_expr(&e.0, ctx))?;
        Ok(Box::new(x))
    }

    /// Fails with 22G03 when `e` is a literal that is not a boolean (TS-19
    /// Narrowing: `GQL.g4` accepts `NOT 1`).
    fn boolean(&self, e: &Expr, op: &str) -> Result<(), Error> {
        match e {
            Expr::Lit(Value::Bool(_) | Value::Null) => Ok(()),
            Expr::Lit(v) => Err(invalid_type(&format!(
                "{op} needs BOOLEAN operands, not {}",
                crate::query::expr::type_name(v)
            ))),
            _ => Ok(()),
        }
    }

    pub(super) fn bind_expr(
        &mut self,
        e: &ast::Expr,
        ctx: &mut ExprCtx<'_>,
    ) -> Result<Expr, Error> {
        use ast::Expr as A;
        Ok(match e {
            A::Unary { op, child } => match op.0 {
                ast::UnaryOp::Plus => Expr::Pos(self.sub(child, ctx)?),
                ast::UnaryOp::Minus if is_min_magnitude(&child.0) => {
                    Expr::Lit(Value::Int(i64::MIN))
                }
                ast::UnaryOp::Minus => Expr::Neg(self.sub(child, ctx)?),
                ast::UnaryOp::Not => {
                    let x = self.sub(child, ctx)?;
                    self.boolean(&x, "NOT")?;
                    Expr::Not(x)
                }
            },
            A::Binary { op, left, right } => {
                if op.0 == ast::BinaryOp::Xor {
                    return Err(self.locate(unsupported("GE07"), op.1));
                }
                let (l, r) = (self.sub(left, ctx)?, self.sub(right, ctx)?);
                match op.0 {
                    ast::BinaryOp::Mul => Expr::Arith(ArithOp::Mul, l, r),
                    ast::BinaryOp::Div => Expr::Arith(ArithOp::Div, l, r),
                    ast::BinaryOp::Add => Expr::Arith(ArithOp::Add, l, r),
                    ast::BinaryOp::Sub => Expr::Arith(ArithOp::Sub, l, r),
                    ast::BinaryOp::Concat => Expr::Concat(l, r),
                    ast::BinaryOp::Eq => Expr::Cmp(CmpOp::Eq, l, r),
                    ast::BinaryOp::Ne => Expr::Cmp(CmpOp::Ne, l, r),
                    ast::BinaryOp::Lt => Expr::Cmp(CmpOp::Lt, l, r),
                    ast::BinaryOp::Gt => Expr::Cmp(CmpOp::Gt, l, r),
                    ast::BinaryOp::Le => Expr::Cmp(CmpOp::Le, l, r),
                    ast::BinaryOp::Ge => Expr::Cmp(CmpOp::Ge, l, r),
                    ast::BinaryOp::And => {
                        self.boolean(&l, "AND")?;
                        self.boolean(&r, "AND")?;
                        Expr::And(l, r)
                    }
                    ast::BinaryOp::Or => {
                        self.boolean(&l, "OR")?;
                        self.boolean(&r, "OR")?;
                        Expr::Or(l, r)
                    }
                    ast::BinaryOp::Xor => unreachable!("gated above"),
                }
            }
            A::IsTruth { expr, not, value } => {
                let x = self.sub(expr, ctx)?;
                self.boolean(&x, "IS")?;
                let t = match value {
                    ast::TruthValue::True => Truth::True,
                    ast::TruthValue::False => Truth::False,
                    ast::TruthValue::Unknown => Truth::Unknown,
                };
                Expr::IsTruth(x, *not, t)
            }
            A::IsNormalized { expr, not, form } => Expr::IsNormalized(
                self.sub(expr, ctx)?,
                *not,
                form.unwrap_or(ast::NormalForm::Nfc),
            ),
            A::Predicate(p) => match p {
                ast::Predicate::Exists(x) => self.bind_exists(x)?,
                ast::Predicate::Null { expr, not } => Expr::IsNull(self.sub(expr, ctx)?, *not),
                ast::Predicate::ValueType { .. } => return Err(unsupported("GA06")),
                ast::Predicate::Directed { .. } => return Err(unsupported("G110")),
                ast::Predicate::Labeled { .. } => return Err(unsupported("G111")),
                ast::Predicate::Source { .. } | ast::Predicate::Destination { .. } => {
                    return Err(unsupported("G112"));
                }
                ast::Predicate::AllDifferent(_) => return Err(unsupported("G113")),
                ast::Predicate::Same(_) => return Err(unsupported("G114")),
                ast::Predicate::PropertyExists { .. } => return Err(unsupported("G115")),
            },
            A::Graph(_) => return Err(unsupported("GE01")),
            A::BindingTable(_) => return Err(unsupported("GE02")),
            A::Function(f) => self.bind_function(f, ctx)?,
            A::Aggregate(a) => self.bind_aggregate(a, ctx)?,
            A::Literal(l) => Expr::Lit(self.literal(l)?),
            A::Parameter(p) => Expr::Param(self.param(p)),
            A::SessionUser => Expr::SessionUser,
            A::Variable(v) => match self.lookup(v) {
                Some(s) => Expr::Slot(s),
                None => return Err(undefined(v)),
            },
            A::Property { source, name } => {
                let src = self.sub(source, ctx)?;
                if let Expr::Slot(s) = *src
                    && self.slots[s].kind == VarKind::Value
                    && self.slots[s].ty != crate::query::bound::Ty::Any
                {
                    return Err(invalid_type(&format!(
                        "a property of the value '{}'",
                        self.slots[s].name
                    )));
                }
                Expr::Prop(src, self.tok(TokenKind::PropertyKey, &name.0))
            }
            A::Parenthesized(x) => *self.sub(x, ctx)?,
            A::Path(_) => return Err(unsupported("GE06")),
            A::ValueQuery(_) => return Err(unsupported("GQ18")),
            A::Case(c) => self.bind_case(c, ctx)?,
            A::Cast { .. } => return Err(unsupported("GA05")),
            A::ElementId(_) => return Err(unsupported("G100")),
            A::Let { .. } => return Err(unsupported("GE03")),
        })
    }

    fn bind_aggregate(&mut self, a: &ast::Aggregate, ctx: &mut ExprCtx<'_>) -> Result<Expr, Error> {
        if ctx.aggs.is_none() {
            return Err(rule("an aggregate function is not allowed here"));
        }
        if ctx.in_aggregate {
            return Err(rule("an aggregate function inside an aggregate function"));
        }
        let (kind, distinct, arg) = match a {
            ast::Aggregate::CountAll => (AggKind::CountStar, false, Expr::Lit(Value::Null)),
            ast::Aggregate::General {
                kind,
                quantifier,
                expr,
            } => {
                let k = match kind {
                    ast::GeneralSetFunction::Avg => AggKind::Avg,
                    ast::GeneralSetFunction::Count => AggKind::Count,
                    ast::GeneralSetFunction::Max => AggKind::Max,
                    ast::GeneralSetFunction::Min => AggKind::Min,
                    ast::GeneralSetFunction::Sum => AggKind::Sum,
                    ast::GeneralSetFunction::StddevSamp => AggKind::StddevSamp,
                    ast::GeneralSetFunction::StddevPop => AggKind::StddevPop,
                    ast::GeneralSetFunction::CollectList => return Err(unsupported("GF10")),
                };
                ctx.in_aggregate = true;
                let x = self.at(expr.1, |b| b.bind_expr(&expr.0, ctx));
                ctx.in_aggregate = false;
                (
                    k,
                    *quantifier == Some(ast::SetQuantifier::Distinct),
                    x?.fold(),
                )
            }
            ast::Aggregate::Binary { .. } => return Err(unsupported("GF11")),
        };
        let slot = self.hidden(VarKind::Value, None);
        ctx.saw_aggregate = true;
        ctx.aggs
            .as_mut()
            .expect("checked above")
            .push((slot, kind, distinct, arg));
        Ok(Expr::Slot(slot))
    }

    fn bind_exists(&mut self, x: &ast::Exists) -> Result<Expr, Error> {
        let scope = self.scope.clone();
        let parts = std::mem::take(&mut self.parts);
        let columns = std::mem::take(&mut self.columns);
        let r = (|| -> Result<(), Error> {
            match x {
                ast::Exists::Pattern(p) => {
                    let graph = self.graph_index(self.current_graph);
                    let pattern = self.at(p.1, |b| b.bind_graph_pattern(&p.0, graph))?;
                    self.parts.push(Part::Match {
                        optional: false,
                        graph,
                        pattern,
                    });
                    Ok(())
                }
                ast::Exists::Match(stmts) => {
                    if stmts.len() > 1 {
                        return Err(unsupported("GQ22"));
                    }
                    let graph = self.graph_index(self.current_graph);
                    for Spanned(m, span) in stmts {
                        let t = match m {
                            ast::MatchStatement::Simple(t) => &t.0,
                            ast::MatchStatement::Optional(_) => {
                                return Err(self.locate(
                                    rule("OPTIONAL MATCH as the only statement of EXISTS"),
                                    *span,
                                ));
                            }
                        };
                        if !t.yield_clause.is_empty() {
                            return Err(unsupported("GQ19"));
                        }
                        let pattern =
                            self.at(*span, |b| b.bind_graph_pattern(&t.pattern.0, graph))?;
                        self.parts.push(Part::Match {
                            optional: false,
                            graph,
                            pattern,
                        });
                    }
                    Ok(())
                }
                ast::Exists::Nested(p) => self.at(p.1, |b| b.bind_nested(&p.0)),
            }
        })();
        let sub = std::mem::replace(&mut self.parts, parts);
        self.columns = columns;
        self.scope = scope;
        r?;
        self.reads = true;
        self.subqueries.push(sub);
        Ok(Expr::Exists(self.subqueries.len() - 1))
    }

    fn bind_case(&mut self, c: &ast::Case, ctx: &mut ExprCtx<'_>) -> Result<Expr, Error> {
        let null = || Box::new(Expr::Lit(Value::Null));
        Ok(match c {
            ast::Case::NullIf(a, b) => {
                let (x, y) = (self.sub(a, ctx)?, self.sub(b, ctx)?);
                Expr::Case(
                    vec![(Expr::Cmp(CmpOp::Eq, x.clone(), y), Expr::Lit(Value::Null))],
                    x,
                )
            }
            ast::Case::Coalesce(xs) => {
                let mut out = Vec::new();
                for x in xs {
                    out.push(*self.sub(x, ctx)?);
                }
                Expr::Coalesce(out)
            }
            ast::Case::Simple {
                operand,
                whens,
                else_result,
            } => {
                let op = self.sub(operand, ctx)?;
                let mut arms = Vec::new();
                for Spanned(w, span) in whens {
                    let mut cond: Option<Expr> = None;
                    for Spanned(o, ospan) in &w.operands {
                        let test = self.at(*ospan, |b| b.when_operand(&op, o, ctx))?;
                        cond = Some(match cond {
                            None => test,
                            Some(c) => Expr::Or(Box::new(c), Box::new(test)),
                        });
                    }
                    let r = self.at(*span, |b| b.bind_expr(&w.result.0, ctx))?;
                    arms.push((cond.unwrap_or(Expr::Lit(Value::Bool(false))), r));
                }
                let e = match else_result {
                    Some(x) => self.sub(x, ctx)?,
                    None => null(),
                };
                Expr::Case(arms, e)
            }
            ast::Case::Searched { whens, else_result } => {
                let mut arms = Vec::new();
                for (c, r) in whens {
                    let c = *self.sub(c, ctx)?;
                    self.boolean(&c, "WHEN")?;
                    arms.push((c, *self.sub(r, ctx)?));
                }
                let e = match else_result {
                    Some(x) => self.sub(x, ctx)?,
                    None => null(),
                };
                Expr::Case(arms, e)
            }
        })
    }

    fn when_operand(
        &mut self,
        op: &Expr,
        o: &ast::WhenOperand,
        ctx: &mut ExprCtx<'_>,
    ) -> Result<Expr, Error> {
        let op = Box::new(op.clone());
        Ok(match o {
            ast::WhenOperand::Value(v) => Expr::Cmp(CmpOp::Eq, op, self.sub(v, ctx)?),
            ast::WhenOperand::Comparison { op: c, value } => {
                let c = match c {
                    ast::BinaryOp::Eq => CmpOp::Eq,
                    ast::BinaryOp::Ne => CmpOp::Ne,
                    ast::BinaryOp::Lt => CmpOp::Lt,
                    ast::BinaryOp::Gt => CmpOp::Gt,
                    ast::BinaryOp::Le => CmpOp::Le,
                    ast::BinaryOp::Ge => CmpOp::Ge,
                    _ => {
                        return Err(rule(
                            "a WHEN comparison with an operator that compares nothing",
                        ));
                    }
                };
                Expr::Cmp(c, op, self.sub(value, ctx)?)
            }
            ast::WhenOperand::Null { not } => Expr::IsNull(op, *not),
            ast::WhenOperand::Normalized { not, form } => {
                Expr::IsNormalized(op, *not, form.unwrap_or(ast::NormalForm::Nfc))
            }
            ast::WhenOperand::ValueType { .. } => return Err(unsupported("GA06")),
            ast::WhenOperand::Directed { .. } => return Err(unsupported("G110")),
            ast::WhenOperand::Labeled { .. } => return Err(unsupported("G111")),
            ast::WhenOperand::Source { .. } | ast::WhenOperand::Destination { .. } => {
                return Err(unsupported("G112"));
            }
        })
    }

    /// The argument of a temporal function: a record constructor is
    /// allowed there (`recordConstructor` inside GV39–GV41).
    fn temporal_arg(
        &mut self,
        e: &Spanned<ast::Expr>,
        ctx: &mut ExprCtx<'_>,
    ) -> Result<Expr, Error> {
        match &e.0 {
            ast::Expr::Literal(ast::Literal::Record(fields)) => {
                let mut out = Vec::new();
                for Spanned(f, span) in fields {
                    let v = self.at(*span, |b| b.bind_expr(&f.value.0, ctx))?;
                    out.push((Arc::from(&*f.name.0), v));
                }
                Ok(Expr::Record(out))
            }
            _ => Ok(*self.sub(e, ctx)?),
        }
    }

    fn bind_function(&mut self, f: &ast::Function, ctx: &mut ExprCtx<'_>) -> Result<Expr, Error> {
        use ast::Function as F;
        Ok(match f {
            F::Unary { kind, arg } => {
                let func = match kind {
                    ast::UnaryFunction::CharLength => Func::CharLength,
                    ast::UnaryFunction::Upper => Func::Upper,
                    ast::UnaryFunction::Lower => Func::Lower,
                    ast::UnaryFunction::Abs => {
                        if let ast::Expr::Literal(ast::Literal::Numeric(_)) = arg.0 {
                            return Err(unsupported("GF01"));
                        }
                        gate("GV41")?;
                        Func::Abs
                    }
                    ast::UnaryFunction::ByteLength => return Err(unsupported("GV35")),
                    ast::UnaryFunction::PathLength | ast::UnaryFunction::Elements => {
                        return Err(unsupported("GF04"));
                    }
                    ast::UnaryFunction::Cardinality => return Err(unsupported("GF12")),
                    ast::UnaryFunction::Size => return Err(unsupported("GF13")),
                    ast::UnaryFunction::Sin
                    | ast::UnaryFunction::Cos
                    | ast::UnaryFunction::Tan
                    | ast::UnaryFunction::Cot
                    | ast::UnaryFunction::Sinh
                    | ast::UnaryFunction::Cosh
                    | ast::UnaryFunction::Tanh
                    | ast::UnaryFunction::Asin
                    | ast::UnaryFunction::Acos
                    | ast::UnaryFunction::Atan
                    | ast::UnaryFunction::Degrees
                    | ast::UnaryFunction::Radians => return Err(unsupported("GF02")),
                    ast::UnaryFunction::Log10 | ast::UnaryFunction::Ln => {
                        return Err(unsupported("GF03"));
                    }
                    ast::UnaryFunction::Exp => return Err(unsupported("GF03")),
                    ast::UnaryFunction::Sqrt
                    | ast::UnaryFunction::Floor
                    | ast::UnaryFunction::Ceiling => return Err(unsupported("GF01")),
                };
                Expr::Call(func, vec![*self.sub(arg, ctx)?])
            }
            F::Binary { kind, left, right } => {
                let func = match kind {
                    ast::BinaryFunction::Left => Func::Left,
                    ast::BinaryFunction::Right => Func::Right,
                    ast::BinaryFunction::Mod | ast::BinaryFunction::Power => {
                        return Err(unsupported("GF01"));
                    }
                    ast::BinaryFunction::Log => return Err(unsupported("GF03")),
                    ast::BinaryFunction::TrimList => return Err(unsupported("GV50")),
                };
                Expr::Call(func, vec![*self.sub(left, ctx)?, *self.sub(right, ctx)?])
            }
            F::Trim {
                specification,
                characters,
                source,
            } => {
                if specification.is_some() || characters.is_some() {
                    return Err(unsupported("GF06"));
                }
                Expr::Call(Func::Trim, vec![*self.sub(source, ctx)?])
            }
            F::MultiTrim { .. } => return Err(unsupported("GF05")),
            F::Normalize { source, form } => Expr::Call(
                Func::Normalize(form.unwrap_or(ast::NormalForm::Nfc)),
                vec![*self.sub(source, ctx)?],
            ),
            F::Datetime { kind, parameters } => {
                use ast::DatetimeFunction as D;
                let (code, now, of) = match kind {
                    D::CurrentDate => ("GV39", Func::CurrentDate, Func::Date),
                    D::Date => ("GV39", Func::CurrentDate, Func::Date),
                    D::CurrentTime => ("GV40", Func::CurrentTime, Func::ZonedTime),
                    D::ZonedTime => ("GV40", Func::CurrentTime, Func::ZonedTime),
                    D::LocalTime => ("GV39", Func::CurrentLocalTime, Func::LocalTime),
                    D::CurrentTimestamp => ("GV40", Func::CurrentTimestamp, Func::ZonedDatetime),
                    D::ZonedDatetime => ("GV40", Func::CurrentTimestamp, Func::ZonedDatetime),
                    D::LocalTimestamp => ("GV39", Func::CurrentLocalTimestamp, Func::LocalDatetime),
                    D::LocalDatetime => ("GV39", Func::CurrentLocalTimestamp, Func::LocalDatetime),
                };
                gate(code)?;
                match parameters {
                    None => Expr::Call(now, Vec::new()),
                    Some(p) => Expr::Call(of, vec![self.temporal_arg(p, ctx)?]),
                }
            }
            F::DurationBetween {
                left,
                right,
                year_to_month,
            } => {
                gate("GV41")?;
                let func = if *year_to_month == Some(true) {
                    Func::DurationBetweenMonths
                } else {
                    Func::DurationBetween
                };
                Expr::Call(func, vec![*self.sub(left, ctx)?, *self.sub(right, ctx)?])
            }
            F::Duration(x) => {
                gate("GV41")?;
                Expr::Call(Func::Duration, vec![self.temporal_arg(x, ctx)?])
            }
        })
    }

    /// The value of a literal, with its feature gates.
    pub(super) fn literal(&mut self, l: &ast::Literal) -> Result<Value, Error> {
        use ast::Literal as L;
        Ok(match l {
            L::Numeric(n) => numeric(n)?,
            L::Boolean(t) => match t {
                ast::TruthValue::True => Value::Bool(true),
                ast::TruthValue::False => Value::Bool(false),
                ast::TruthValue::Unknown => Value::Null,
            },
            L::String(s) => {
                if s.no_escape {
                    gate("GL11")?;
                }
                Value::string(&s.value)
            }
            L::Bytes(_) => return Err(unsupported("GV35")),
            L::Temporal { kind, value } => {
                if value.0.no_escape {
                    gate("GL11")?;
                }
                temporal_literal(*kind, &value.0.value)?
            }
            L::Duration(value) => {
                gate("GV41")?;
                let s = &value.0.value;
                crate::query::functions::parse_duration(s).ok_or_else(|| {
                    Error::new(codes::C22G0H, format!("invalid duration format: '{s}'"))
                })?
            }
            L::Null => Value::Null,
            L::List { .. } => return Err(unsupported("GV50")),
            L::Record(_) => return Err(unsupported("GV45")),
        })
    }

    /// A property key by name.
    pub(super) fn prop_key(&self, name: &str) -> Name {
        Name {
            name: name.into(),
            token: self.tx.engine().token_id(TokenKind::PropertyKey, name),
        }
    }

    /// A token reference of `kind` by name.
    pub(super) fn tok(&self, kind: TokenKind, name: &str) -> Tok {
        Tok {
            kind,
            name: Arc::from(name),
            token: self.tx.engine().token_id(kind, name),
        }
    }

    /// A label (`edge` false) or an edge type by name.
    pub(super) fn label(&self, name: &str, edge: bool) -> Name {
        let kind = if edge {
            TokenKind::EdgeType
        } else {
            TokenKind::Label
        };
        Name {
            name: name.into(),
            token: self.tx.engine().token_id(kind, name),
        }
    }

    /// A label expression over labels or edge types (G074 for `%`).
    pub(super) fn label_test(&self, e: &ast::LabelExpr, edge: bool) -> Result<LabelTest, Error> {
        Ok(match e {
            ast::LabelExpr::Label(n) => LabelTest::Label(self.tok(
                if edge {
                    TokenKind::EdgeType
                } else {
                    TokenKind::Label
                },
                n,
            )),
            ast::LabelExpr::Conjunction(a, b) => LabelTest::And(
                Box::new(self.label_test(&a.0, edge)?),
                Box::new(self.label_test(&b.0, edge)?),
            ),
            ast::LabelExpr::Disjunction(a, b) => LabelTest::Or(
                Box::new(self.label_test(&a.0, edge)?),
                Box::new(self.label_test(&b.0, edge)?),
            ),
            ast::LabelExpr::Negation(a) => LabelTest::Not(Box::new(self.label_test(&a.0, edge)?)),
            ast::LabelExpr::Parenthesized(a) => self.label_test(&a.0, edge)?,
            ast::LabelExpr::Wildcard => return Err(unsupported("G074")),
        })
    }
}

/// The value of a numeric literal, with the gates of its form (GL01–GL10).
fn numeric(n: &ast::NumericLiteral) -> Result<Value, Error> {
    let t = &*n.text;
    let last = t.chars().last().unwrap_or(' ').to_ascii_lowercase();
    let sci = t.contains(['e', 'E']) && !t.starts_with("0x") && !t.starts_with("0X");
    match n.kind {
        ast::NumericLiteralKind::Integer(10) => Ok(Value::Int(integer(t)?)),
        ast::NumericLiteralKind::Integer(16) => Err(unsupported("GL01")),
        ast::NumericLiteralKind::Integer(8) => Err(unsupported("GL02")),
        ast::NumericLiteralKind::Integer(_) => Err(unsupported("GL03")),
        ast::NumericLiteralKind::Exact => Err(unsupported(if sci {
            "GL06"
        } else if last == 'm' {
            "GL05"
        } else {
            "GL04"
        })),
        ast::NumericLiteralKind::Approximate => {
            if matches!(last, 'f' | 'd') {
                return Err(unsupported(if sci { "GL08" } else { "GL07" }));
            }
            let digits: String = t.chars().filter(|c| *c != '_').collect();
            let f: f64 = digits.parse().map_err(|_| {
                Error::new(codes::C22003, format!("numeric value out of range: {t}"))
            })?;
            if f.is_finite() {
                Ok(Value::Float(f))
            } else {
                Err(Error::new(
                    codes::C22003,
                    format!("numeric value out of range: {t}"),
                ))
            }
        }
    }
}

/// The value of a temporal literal (GV39, GV40); 22007 for a string that
/// is not one.
fn temporal_literal(kind: ast::TemporalLiteralKind, s: &str) -> Result<Value, Error> {
    use jiff::civil::{Date, DateTime, Time};
    let zt = Value::ZonedTime(ZonedTime {
        time: Time::midnight(),
        offset: 0,
    });
    let lt = Value::LocalTime(Time::midnight());
    let zdt = Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(
        jiff::Timestamp::UNIX_EPOCH,
        jiff::tz::Offset::UTC,
    )));
    let ldt = Value::LocalDateTime(DateTime::constant(2000, 1, 1, 0, 0, 0, 0));
    let v = match kind {
        ast::TemporalLiteralKind::Date => {
            gate("GV39")?;
            temporal::parse(&Value::Date(Date::constant(2000, 1, 1)), s)
        }
        ast::TemporalLiteralKind::Time => match temporal::parse(&zt, s) {
            Some(v) => {
                gate("GV40")?;
                Some(v)
            }
            None => {
                gate("GV39")?;
                temporal::parse(&lt, s)
            }
        },
        ast::TemporalLiteralKind::Datetime | ast::TemporalLiteralKind::Timestamp => {
            match temporal::parse(&zdt, s) {
                Some(v) => {
                    gate("GV40")?;
                    Some(v)
                }
                None => {
                    gate("GV39")?;
                    temporal::parse(&ldt, s)
                }
            }
        }
    };
    v.ok_or_else(|| {
        Error::new(
            codes::C22007,
            format!("invalid date, time, or datetime format: '{s}'"),
        )
    })
}
