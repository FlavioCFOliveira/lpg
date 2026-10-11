//! Value expressions, predicates, functions and literals (`GQL.g4` 19 to
//! 21).
//!
//! `valueExpression` is left-recursive; ANTLR rewrites it into a loop in
//! which each alternative has the precedence `16 - alternative`: `+`/`-`
//! prefix 15, `*` `/` 14, `+` `-` 13, `||` 12, comparison 11,
//! `IS [NOT] NORMALIZED` 9, `NOT` prefix 8, `IS [NOT] TRUE` 7, `AND` 6,
//! `OR`/`XOR` 5. The Pratt loop of [`Parser::value_expression_bp`]
//! applies an operator of precedence `p` when `p` is at least the
//! minimum, parses a binary operator's right operand at `p + 1` and a
//! prefix operator's operand at `p`, as ANTLR's rewrite does
//! (`LeftRecursiveRuleAnalyzer`). The predicates of 19.2 are primaries:
//! `x IS NULL` binds to the primary before it.

use super::{Expected, Fail, PResult, Parser};
use crate::query::ast::{
    Aggregate, BinaryFunction, BinaryOp, Case, DatetimeFunction, Exists, Expr, Field, Function,
    GeneralSetFunction, LabelExpr, Literal, NormalForm, NumericLiteral, NumericLiteralKind,
    Predicate, SimpleWhen, Statement, StringLiteral, TemporalLiteralKind, TrimSpecification,
    TruthValue, UnaryFunction, UnaryOp, WhenOperand,
};
use crate::query::ast::{
    DataModifyingBody, ListTypeName, SimpleDataAccessingStatement, SimpleQueryStatement,
};
use crate::query::lexer::{Keyword as K, TokenKind as T};
use crate::query::span::{BoxSpanned, Spanned, VecSpanned};

/// What follows `IS [NOT]` in an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IsTail {
    Null,
    Type,
    Truth,
    Normalized,
    Directed,
    Labeled,
    Source,
    Destination,
}

/// The comparison operator a token is.
fn comparison(kind: T) -> Option<BinaryOp> {
    Some(match kind {
        T::Equals => BinaryOp::Eq,
        T::NotEquals => BinaryOp::Ne,
        T::LeftAngleBracket => BinaryOp::Lt,
        T::RightAngleBracket => BinaryOp::Gt,
        T::LessThanOrEquals => BinaryOp::Le,
        T::GreaterThanOrEquals => BinaryOp::Ge,
        _ => return None,
    })
}

/// The one-argument numeric functions of `numericValueFunction` whose
/// argument is a `numericValueExpression`.
fn numeric_unary(k: K) -> Option<UnaryFunction> {
    use UnaryFunction as F;
    Some(match k {
        K::Sin => F::Sin,
        K::Cos => F::Cos,
        K::Tan => F::Tan,
        K::Cot => F::Cot,
        K::Sinh => F::Sinh,
        K::Cosh => F::Cosh,
        K::Tanh => F::Tanh,
        K::Asin => F::Asin,
        K::Acos => F::Acos,
        K::Atan => F::Atan,
        K::Degrees => F::Degrees,
        K::Radians => F::Radians,
        K::Log10 => F::Log10,
        K::Ln => F::Ln,
        K::Exp => F::Exp,
        K::Sqrt => F::Sqrt,
        K::Floor => F::Floor,
        K::Ceil | K::Ceiling => F::Ceiling,
        _ => return None,
    })
}

/// Whether `k` starts a `numericValueFunction`.
fn starts_numeric_function(k: T) -> bool {
    match k {
        T::Keyword(k) => {
            numeric_unary(k).is_some()
                || matches!(
                    k,
                    K::CharLength
                        | K::CharacterLength
                        | K::ByteLength
                        | K::OctetLength
                        | K::PathLength
                        | K::Cardinality
                        | K::Size
                        | K::Abs
                        | K::Mod
                        | K::Log
                        | K::Power
                )
        }
        _ => false,
    }
}

/// Whether `k` (then `next`) starts a `valueFunction`.
fn starts_value_function(k: T, next: T) -> bool {
    starts_numeric_function(k)
        || matches!(
            k,
            T::Keyword(
                K::DurationBetween
                    | K::CurrentDate
                    | K::CurrentTime
                    | K::CurrentTimestamp
                    | K::ZonedTime
                    | K::ZonedDatetime
                    | K::LocalTime
                    | K::LocalTimestamp
                    | K::LocalDatetime
                    | K::Left
                    | K::Right
                    | K::Trim
                    | K::Upper
                    | K::Lower
                    | K::Btrim
                    | K::Ltrim
                    | K::Rtrim
                    | K::Normalize
            )
        )
        || (matches!(k, T::Keyword(K::Date | K::Duration | K::Elements)) && next == T::LeftParen)
}

impl Parser<'_> {
    /// `valueExpression` (also `searchCondition`,
    /// `booleanValueExpression`, `aggregatingValueExpression` and the
    /// other rules that are one `valueExpression`).
    pub(super) fn value_expression(&mut self) -> PResult<Spanned<Expr>> {
        self.value_expression_bp(0)
    }

    fn value_expression_bp(&mut self, min: u8) -> PResult<Spanned<Expr>> {
        let _guard = self.nest()?;
        let start = self.start();
        let mut left = self.value_expression_prefix()?;
        // Each operator applied deepens the tree by one level: the
        // chain counts against the nesting limit (`MAX_DEPTH`).
        let mut chain = Vec::new();
        loop {
            let kind = self.peek();
            let (prec, op) = match kind {
                T::Asterisk => (14, Some(BinaryOp::Mul)),
                T::Solidus => (14, Some(BinaryOp::Div)),
                T::PlusSign => (13, Some(BinaryOp::Add)),
                T::MinusSign => (13, Some(BinaryOp::Sub)),
                T::Concatenation => (12, Some(BinaryOp::Concat)),
                T::Keyword(K::And) => (6, Some(BinaryOp::And)),
                T::Keyword(K::Or) => (5, Some(BinaryOp::Or)),
                T::Keyword(K::Xor) => (5, Some(BinaryOp::Xor)),
                T::Keyword(K::Is) => match self.is_tail(0) {
                    Some(IsTail::Normalized) => (9, None),
                    Some(IsTail::Truth) => (7, None),
                    _ => break,
                },
                k => match comparison(k) {
                    Some(op) => (11, Some(op)),
                    None => {
                        for k in [T::Keyword(K::And), T::Keyword(K::Or), T::Keyword(K::Is)] {
                            self.expect_here(Expected::Token(k));
                        }
                        break;
                    }
                },
            };
            if prec < min {
                break;
            }
            chain.push(self.nest()?);
            let l = Box::new(self.spanned(start, left));
            let op_start = self.start();
            match op {
                Some(op) => {
                    self.bump();
                    let op = self.spanned(op_start, op);
                    let right = Box::new(self.value_expression_bp(prec + 1)?);
                    left = Expr::Binary { op, left: l, right };
                }
                None if prec == 7 => {
                    // valueExpression IS NOT? truthValue.
                    self.bump();
                    let not = self.eat_kw(K::Not);
                    let value = self.truth_value()?;
                    left = Expr::IsTruth {
                        expr: l,
                        not,
                        value,
                    };
                }
                None => {
                    // valueExpression normalizedPredicatePart2.
                    let (not, form) = self.normalized_predicate_part2()?;
                    left = Expr::IsNormalized { expr: l, not, form };
                }
            }
        }
        Ok(self.spanned(start, left))
    }

    /// What follows the `IS` `n` tokens ahead, after an optional `NOT`.
    fn is_tail(&self, n: usize) -> Option<IsTail> {
        if self.peek_nth(n) != T::Keyword(K::Is) {
            return None;
        }
        let mut k = self.peek_nth(n + 1);
        if k == T::Keyword(K::Not) {
            k = self.peek_nth(n + 2);
        }
        Some(match k {
            T::Keyword(K::Null) => IsTail::Null,
            T::DoubleColon | T::Keyword(K::Typed) => IsTail::Type,
            T::BooleanLiteral => IsTail::Truth,
            T::Keyword(K::Nfc | K::Nfd | K::Nfkc | K::Nfkd | K::Normalized) => IsTail::Normalized,
            T::Keyword(K::Directed) => IsTail::Directed,
            T::Keyword(K::Labeled) => IsTail::Labeled,
            T::Keyword(K::Source) => IsTail::Source,
            T::Keyword(K::Destination) => IsTail::Destination,
            _ => return None,
        })
    }

    /// `truthValue: BOOLEAN_LITERAL`.
    fn truth_value(&mut self) -> PResult<TruthValue> {
        if !self.at(T::BooleanLiteral) {
            return Err(Fail);
        }
        let t = self.bump();
        Ok(match t.text(self.text).to_ascii_uppercase().as_str() {
            "TRUE" => TruthValue::True,
            "FALSE" => TruthValue::False,
            _ => TruthValue::Unknown,
        })
    }

    /// `normalizedPredicatePart2: IS NOT? normalForm? NORMALIZED`.
    fn normalized_predicate_part2(&mut self) -> PResult<(bool, Option<NormalForm>)> {
        self.expect_kw(K::Is)?;
        let not = self.eat_kw(K::Not);
        let form = self.normal_form();
        self.expect_kw(K::Normalized)?;
        Ok((not, form))
    }

    /// `normalForm: NFC | NFD | NFKC | NFKD`, optional.
    fn normal_form(&mut self) -> Option<NormalForm> {
        let form = match self.peek() {
            T::Keyword(K::Nfc) => NormalForm::Nfc,
            T::Keyword(K::Nfd) => NormalForm::Nfd,
            T::Keyword(K::Nfkc) => NormalForm::Nfkc,
            T::Keyword(K::Nfkd) => NormalForm::Nfkd,
            _ => return None,
        };
        self.bump();
        Some(form)
    }

    /// The prefix and primary alternatives of `valueExpression`, in the
    /// oracle's order: sign (1), `predicate` (6), `NOT` (8), `PROPERTY?
    /// GRAPH graphExpression` (12), `BINDING? TABLE
    /// bindingTableExpression` (13), `valueFunction` (14),
    /// `valueExpressionPrimary` (15).
    fn value_expression_prefix(&mut self) -> PResult<Expr> {
        let start = self.start();
        match (self.peek(), self.peek_nth(1)) {
            (T::PlusSign | T::MinusSign, _) => {
                let op = if self.bump().kind == T::PlusSign {
                    UnaryOp::Plus
                } else {
                    UnaryOp::Minus
                };
                let op = self.spanned(start, op);
                let child = Box::new(self.value_expression_bp(15)?);
                Ok(Expr::Unary { op, child })
            }
            (T::Keyword(K::Not), _) => {
                self.bump();
                let op = self.spanned(start, UnaryOp::Not);
                let child = Box::new(self.value_expression_bp(8)?);
                Ok(Expr::Unary { op, child })
            }
            (T::Keyword(K::Exists | K::AllDifferent | K::Same | K::PropertyExists), _) => {
                Ok(Expr::Predicate(self.keyword_predicate()?))
            }
            (T::Keyword(K::Graph | K::Table), _)
            | (T::Keyword(K::Property), T::Keyword(K::Graph))
            | (T::Keyword(K::Binding), T::Keyword(K::Table)) => {
                // A predicate on the variable `graph` (alternative 6)
                // comes before the graph expression (12).
                if let Some(p) = self.try_parse(|p| {
                    let primary = p.value_expression_primary()?;
                    match p.predicate_tail(primary)? {
                        Ok(pred) => Ok(pred),
                        Err(_) => Err(Fail),
                    }
                })? {
                    return Ok(Expr::Predicate(p));
                }
                let graph = matches!(self.peek(), T::Keyword(K::Graph | K::Property));
                let (before, escapes) = (self.pos, self.invalid_escapes.len());
                let alternative = self.try_parse(|p| {
                    if graph {
                        p.eat_kw(K::Property);
                        p.bump();
                        let g = p.graph_expression(&super::catalog::follows_expression)?;
                        Ok(Expr::Graph(Box::new(g)))
                    } else {
                        p.eat_kw(K::Binding);
                        p.bump();
                        let t = p.binding_table_expression(&super::catalog::follows_expression)?;
                        Ok(Expr::BindingTable(Box::new(t)))
                    }
                })?;
                // Both readings parse here: the keyword first (the lower
                // alternative); a failed parse retries the variable
                // ([`super::Site::KEYWORD_OR_VARIABLE`]).
                if let Some(e) = alternative {
                    let after = self.pos;
                    self.pos = before;
                    if self.decide(super::Site::KEYWORD_OR_VARIABLE, 2) == 0 {
                        self.pos = after;
                        return Ok(e);
                    }
                    self.invalid_escapes.truncate(escapes);
                }
                self.primary_or_predicate()
            }
            (k, next) if starts_value_function(k, next) => {
                Ok(Expr::Function(self.value_function()?))
            }
            _ => self.primary_or_predicate(),
        }
    }

    /// A `valueExpressionPrimary`, and the predicate it starts when one
    /// of the predicate parts follows it.
    fn primary_or_predicate(&mut self) -> PResult<Expr> {
        let primary = self.value_expression_primary()?;
        match self.predicate_tail(primary)? {
            Ok(p) => Ok(Expr::Predicate(p)),
            Err(primary) => Ok(primary.0),
        }
    }

    /// The predicate `primary` starts: `nullPredicate`,
    /// `valueTypePredicate`, and, on a lone variable,
    /// `directedPredicate`, `labeledPredicate` and
    /// `sourceDestinationPredicate`. `Err(primary)` when none follows.
    fn predicate_tail(
        &mut self,
        primary: Spanned<Expr>,
    ) -> PResult<Result<Predicate, Spanned<Expr>>> {
        let variable = match &primary.0 {
            Expr::Variable(v) => Some(Spanned(v.clone(), primary.1)),
            _ => None,
        };
        if self.at(T::Colon) {
            if let Some(element) = variable {
                // labeledPredicate with isLabeledOrColon = COLON.
                self.bump();
                let label = self.spanned_label_expression()?;
                return Ok(Ok(Predicate::Labeled {
                    element,
                    not: false,
                    label,
                }));
            }
            return Ok(Err(primary));
        }
        let Some(tail) = self.is_tail(0) else {
            return Ok(Err(primary));
        };
        match tail {
            IsTail::Null => {
                self.bump();
                let not = self.eat_kw(K::Not);
                self.expect_kw(K::Null)?;
                Ok(Ok(Predicate::Null {
                    expr: Box::new(primary),
                    not,
                }))
            }
            IsTail::Type => {
                // valueTypePredicatePart2: IS NOT? typed valueType.
                self.bump();
                let not = self.eat_kw(K::Not);
                self.typed();
                let value_type = Box::new(self.value_type()?);
                Ok(Ok(Predicate::ValueType {
                    expr: Box::new(primary),
                    not,
                    value_type,
                }))
            }
            IsTail::Truth | IsTail::Normalized => Ok(Err(primary)),
            _ => {
                let Some(variable) = variable else {
                    return Ok(Err(primary));
                };
                self.bump();
                let not = self.eat_kw(K::Not);
                Ok(Ok(match self.bump().kind {
                    T::Keyword(K::Directed) => Predicate::Directed {
                        edge: variable,
                        not,
                    },
                    T::Keyword(K::Labeled) => Predicate::Labeled {
                        element: variable,
                        not,
                        label: self.spanned_label_expression()?,
                    },
                    T::Keyword(K::Source) => {
                        self.expect_kw(K::Of)?;
                        Predicate::Source {
                            node: variable,
                            not,
                            edge: self.regular_identifier()?,
                        }
                    }
                    _ => {
                        self.expect_kw(K::Of)?;
                        Predicate::Destination {
                            node: variable,
                            not,
                            edge: self.regular_identifier()?,
                        }
                    }
                }))
            }
        }
    }

    fn spanned_label_expression(&mut self) -> PResult<Spanned<LabelExpr>> {
        let start = self.start();
        let l = self.label_expression()?;
        Ok(self.spanned(start, l))
    }

    /// The predicates that start with a keyword: `existsPredicate`,
    /// `all_differentPredicate`, `samePredicate`,
    /// `property_existsPredicate`.
    fn keyword_predicate(&mut self) -> PResult<Predicate> {
        match self.bump().kind {
            T::Keyword(K::Exists) => Ok(Predicate::Exists(self.exists_predicate()?)),
            T::Keyword(k @ (K::AllDifferent | K::Same)) => {
                // ( elementVariableReference , elementVariableReference
                // (, elementVariableReference)* )
                self.expect(T::LeftParen)?;
                let mut vars = vec![self.regular_identifier()?];
                self.expect(T::Comma)?;
                vars.push(self.regular_identifier()?);
                while self.eat(T::Comma) {
                    vars.push(self.regular_identifier()?);
                }
                self.expect(T::RightParen)?;
                Ok(if k == K::Same {
                    Predicate::Same(vars)
                } else {
                    Predicate::AllDifferent(vars)
                })
            }
            _ => {
                self.expect(T::LeftParen)?;
                let element = self.regular_identifier()?;
                self.expect(T::Comma)?;
                let property = self.identifier()?;
                self.expect(T::RightParen)?;
                Ok(Predicate::PropertyExists { element, property })
            }
        }
    }

    /// `existsPredicate: EXISTS ({ graphPattern } | ( graphPattern ) | {
    /// matchStatementBlock } | ( matchStatementBlock ) |
    /// nestedQuerySpecification)`.
    ///
    /// Inside braces, a match block is also a nested query; the match
    /// block wins (TS-19 ambiguity `existsPredicate`, alternative 3). A
    /// body made only of match statements is that block.
    fn exists_predicate(&mut self) -> PResult<Exists> {
        if self.eat(T::LeftParen) {
            let e = if matches!(self.peek(), T::Keyword(K::Match | K::Optional)) {
                Exists::Match(self.match_statement_block()?)
            } else {
                let start = self.start();
                let g = self.graph_pattern()?;
                Exists::Pattern(Box::new(self.spanned(start, g)))
            };
            self.expect(T::RightParen)?;
            return Ok(e);
        }
        if !self.at(T::LeftBrace) {
            return Err(Fail);
        }
        let (a, b) = (self.peek_nth(1), self.peek_nth(2));
        let pattern = !matches!(a, T::Keyword(K::Match | K::Optional))
            && ((Self::is_regular_identifier(a) && b == T::Equals) || !(self.starts_body_at(1)));
        if pattern {
            self.bump();
            let start = self.start();
            let g = self.graph_pattern()?;
            let g = self.spanned(start, g);
            self.expect(T::RightBrace)?;
            return Ok(Exists::Pattern(Box::new(g)));
        }
        let body = self.nested_procedure_specification()?;
        Ok(match match_block(&body.0) {
            Some(block) => Exists::Match(block),
            None => Exists::Nested(Box::new(body)),
        })
    }

    /// Whether the token `n` ahead starts a `procedureBody`.
    fn starts_body_at(&self, n: usize) -> bool {
        let (k, next) = (self.peek_nth(n), self.peek_nth(n + 1));
        matches!(k, T::Keyword(K::At | K::Value))
            || matches!(
                (k, next),
                (T::Keyword(K::Graph | K::Table), _)
                    | (T::Keyword(K::Property), T::Keyword(K::Graph))
                    | (T::Keyword(K::Binding), T::Keyword(K::Table))
            )
            || super::query::starts_statement(k, next)
    }

    /// `valueExpressionPrimary`: a primary, then `. propertyName` any
    /// number of times (the left-recursive alternative).
    pub(super) fn value_expression_primary(&mut self) -> PResult<Spanned<Expr>> {
        let _guard = self.nest()?;
        let start = self.start();
        let mut e = self.value_expression_primary_atom()?;
        let mut chain = Vec::new();
        while self.peek() == T::Period && Self::is_identifier(self.peek_nth(1)) {
            chain.push(self.nest()?);
            let source = Box::new(self.spanned(start, e));
            self.bump();
            let name = self.identifier()?;
            e = Expr::Property { source, name };
        }
        Ok(self.spanned(start, e))
    }

    /// The non-left-recursive alternatives of `valueExpressionPrimary`.
    fn value_expression_primary_atom(&mut self) -> PResult<Expr> {
        let start = self.start();
        match (self.peek(), self.peek_nth(1)) {
            (T::LeftParen, _) => {
                // parenthesizedValueExpression: ( valueExpression ).
                self.bump();
                let e = self.value_expression()?;
                self.expect(T::RightParen)?;
                Ok(Expr::Parenthesized(Box::new(e)))
            }
            (
                T::Keyword(
                    K::Count
                    | K::Avg
                    | K::Max
                    | K::Min
                    | K::Sum
                    | K::CollectList
                    | K::StddevSamp
                    | K::StddevPop
                    | K::PercentileCont
                    | K::PercentileDisc,
                ),
                _,
            ) => Ok(Expr::Aggregate(self.aggregate_function()?)),
            (T::GeneralParameter, _) => Ok(Expr::Parameter(self.general_parameter()?.0)),
            (T::Keyword(K::SessionUser), _) => {
                self.bump();
                Ok(Expr::SessionUser)
            }
            (T::Keyword(K::Path), T::LeftBracket) => {
                // pathValueConstructorByEnumeration: PATH [ pathElementList ].
                self.pos += 2;
                let mut elements = vec![self.value_expression_primary()?];
                while self.eat(T::Comma) {
                    elements.push(self.value_expression_primary()?);
                    self.expect(T::Comma)?;
                    elements.push(self.value_expression_primary()?);
                }
                self.expect(T::RightBracket)?;
                Ok(Expr::Path(elements))
            }
            (T::Keyword(K::Value), _) => {
                // valueQueryExpression: VALUE nestedQuerySpecification.
                self.bump();
                Ok(Expr::ValueQuery(Box::new(
                    self.nested_procedure_specification()?,
                )))
            }
            (T::Keyword(K::Nullif | K::Coalesce | K::Case), _) => {
                Ok(Expr::Case(self.case_expression()?))
            }
            (T::Keyword(K::Cast), _) => {
                // castSpecification: CAST ( castOperand AS castTarget ).
                self.bump();
                self.expect(T::LeftParen)?;
                let operand = Box::new(self.value_expression()?);
                self.expect_kw(K::As)?;
                let target = Box::new(self.value_type()?);
                self.expect(T::RightParen)?;
                Ok(Expr::Cast { operand, target })
            }
            (T::Keyword(K::ElementId), _) => {
                // element_idFunction: ELEMENT_ID ( elementVariableReference ).
                self.bump();
                self.expect(T::LeftParen)?;
                let v = self.regular_identifier()?;
                self.expect(T::RightParen)?;
                Ok(Expr::ElementId(v))
            }
            (T::Keyword(K::Let), _) => {
                // letValueExpression: LET letVariableDefinitionList IN
                // valueExpression END.
                self.bump();
                let definitions = self.let_variable_definition_list()?;
                self.expect_kw(K::In)?;
                let value = Box::new(self.value_expression()?);
                self.expect_kw(K::End)?;
                Ok(Expr::Let { definitions, value })
            }
            (k, _) if Self::is_regular_identifier(k) => {
                Ok(Expr::Variable(self.regular_identifier()?.0))
            }
            _ => match self.unsigned_literal()? {
                Some(l) => Ok(Expr::Literal(l)),
                None => {
                    let _ = start;
                    self.fail_rule("a value expression")
                }
            },
        }
    }

    /// `unsignedLiteral: unsignedNumericLiteral | generalLiteral`, or
    /// `None` when the current token starts none.
    fn unsigned_literal(&mut self) -> PResult<Option<Literal>> {
        let numeric = match self.peek() {
            T::Integer => Some(NumericLiteralKind::Integer(10)),
            T::Hexadecimal => Some(NumericLiteralKind::Integer(16)),
            T::Octal => Some(NumericLiteralKind::Integer(8)),
            T::Binary => Some(NumericLiteralKind::Integer(2)),
            T::ScientificExact | T::CommonExact | T::Common | T::IntegerExact => {
                Some(NumericLiteralKind::Exact)
            }
            T::ScientificApproximate
            | T::Scientific
            | T::CommonApproximate
            | T::IntegerApproximate => Some(NumericLiteralKind::Approximate),
            _ => None,
        };
        if let Some(kind) = numeric {
            let t = self.bump();
            let text = t.text(self.text);
            let text = match kind {
                NumericLiteralKind::Integer(r) if r != 10 => &text[2..],
                _ => text,
            };
            return Ok(Some(Literal::Numeric(NumericLiteral {
                kind,
                text: text.into(),
            })));
        }
        let temporal = |k: K| match k {
            K::Date => Some(TemporalLiteralKind::Date),
            K::Time => Some(TemporalLiteralKind::Time),
            K::Datetime => Some(TemporalLiteralKind::Datetime),
            K::Timestamp => Some(TemporalLiteralKind::Timestamp),
            _ => None,
        };
        Ok(Some(match self.peek() {
            T::BooleanLiteral => Literal::Boolean(self.truth_value()?),
            T::SingleQuoted | T::DoubleQuoted => {
                Literal::String(self.character_string_literal()?.0)
            }
            T::ByteString => {
                let t = self.bump();
                let text = t.text(self.text);
                let digits: String = text[2..text.len() - 1]
                    .chars()
                    .filter(|c| *c != ' ')
                    .collect();
                Literal::Bytes(digits.into())
            }
            T::Keyword(K::Null) => {
                self.bump();
                Literal::Null
            }
            T::Keyword(K::Duration) => {
                // durationLiteral: DURATION durationString.
                self.bump();
                Literal::Duration(self.character_string_literal()?)
            }
            T::Keyword(k) if temporal(k).is_some() => {
                self.bump();
                Literal::Temporal {
                    kind: temporal(k).unwrap_or(TemporalLiteralKind::Date),
                    value: self.character_string_literal()?,
                }
            }
            T::Keyword(K::List | K::Array) | T::LeftBracket => {
                // listValueConstructorByEnumeration: listValueTypeName? [
                // listElementList? ].
                let name = match self.peek() {
                    T::Keyword(K::List) => Some(ListTypeName::List),
                    T::Keyword(K::Array) => Some(ListTypeName::Array),
                    _ => None,
                };
                if name.is_some() {
                    self.bump();
                }
                self.expect(T::LeftBracket)?;
                let mut elements = Vec::new();
                if !self.eat(T::RightBracket) {
                    loop {
                        elements.push(self.value_expression()?);
                        if !self.eat(T::Comma) {
                            break;
                        }
                    }
                    self.expect(T::RightBracket)?;
                }
                Literal::List { name, elements }
            }
            T::Keyword(K::Record) | T::LeftBrace => Literal::Record(self.record_constructor()?),
            _ => {
                for k in [
                    T::Integer,
                    T::SingleQuoted,
                    T::BooleanLiteral,
                    T::LeftBracket,
                    T::LeftBrace,
                    T::GeneralParameter,
                    T::LeftParen,
                ] {
                    self.expect_here(Expected::Token(k));
                }
                return Ok(None);
            }
        }))
    }

    /// `characterStringLiteral: SINGLE_QUOTED_CHARACTER_SEQUENCE |
    /// DOUBLE_QUOTED_CHARACTER_SEQUENCE`.
    pub(super) fn character_string_literal(&mut self) -> PResult<Spanned<StringLiteral>> {
        if !(self.at(T::SingleQuoted) || self.at(T::DoubleQuoted)) {
            return Err(Fail);
        }
        let t = self.bump();
        let no_escape = t.text(self.text).starts_with('@');
        let value = self.unquote(t);
        Ok(Spanned(
            StringLiteral {
                value: value.into(),
                no_escape,
            },
            t.span,
        ))
    }

    /// `recordConstructor: RECORD? fieldsSpecification`;
    /// `fieldsSpecification: { fieldList? }`; `field: fieldName COLON
    /// valueExpression`.
    fn record_constructor(&mut self) -> PResult<VecSpanned<Field>> {
        self.eat_kw(K::Record);
        self.expect(T::LeftBrace)?;
        let mut fields = Vec::new();
        if !self.eat(T::RightBrace) {
            loop {
                let start = self.start();
                let name = self.identifier()?;
                self.expect(T::Colon)?;
                let value = self.value_expression()?;
                fields.push(self.spanned(start, Field { name, value }));
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.expect(T::RightBrace)?;
        }
        Ok(fields)
    }

    /// `aggregateFunction: COUNT ( * ) | generalSetFunction |
    /// binarySetFunction`.
    fn aggregate_function(&mut self) -> PResult<Aggregate> {
        let kind = self.bump().kind;
        self.expect(T::LeftParen)?;
        if kind == T::Keyword(K::Count) && self.eat(T::Asterisk) {
            self.expect(T::RightParen)?;
            return Ok(Aggregate::CountAll);
        }
        let general = match kind {
            T::Keyword(K::Avg) => Some(GeneralSetFunction::Avg),
            T::Keyword(K::Count) => Some(GeneralSetFunction::Count),
            T::Keyword(K::Max) => Some(GeneralSetFunction::Max),
            T::Keyword(K::Min) => Some(GeneralSetFunction::Min),
            T::Keyword(K::Sum) => Some(GeneralSetFunction::Sum),
            T::Keyword(K::CollectList) => Some(GeneralSetFunction::CollectList),
            T::Keyword(K::StddevSamp) => Some(GeneralSetFunction::StddevSamp),
            T::Keyword(K::StddevPop) => Some(GeneralSetFunction::StddevPop),
            _ => None,
        };
        let quantifier = self.set_quantifier();
        if let Some(kind) = general {
            // generalSetFunction: type ( setQuantifier? valueExpression ).
            let expr = Box::new(self.value_expression()?);
            self.expect(T::RightParen)?;
            return Ok(Aggregate::General {
                kind,
                quantifier,
                expr,
            });
        }
        // binarySetFunction: type ( dependentValueExpression ,
        // independentValueExpression ), both numeric.
        let dependent = Box::new(self.numeric_value_expression()?);
        self.expect(T::Comma)?;
        let independent = Box::new(self.numeric_value_expression()?);
        self.expect(T::RightParen)?;
        Ok(Aggregate::Binary {
            continuous: kind == T::Keyword(K::PercentileCont),
            quantifier,
            dependent,
            independent,
        })
    }

    /// `caseExpression: caseAbbreviation | caseSpecification`.
    fn case_expression(&mut self) -> PResult<Case> {
        match self.bump().kind {
            T::Keyword(K::Nullif) => {
                // NULLIF ( valueExpression , valueExpression ).
                self.expect(T::LeftParen)?;
                let a = Box::new(self.value_expression()?);
                self.expect(T::Comma)?;
                let b = Box::new(self.value_expression()?);
                self.expect(T::RightParen)?;
                Ok(Case::NullIf(a, b))
            }
            T::Keyword(K::Coalesce) => {
                // COALESCE ( valueExpression (, valueExpression)+ ).
                self.expect(T::LeftParen)?;
                let mut args = vec![self.value_expression()?];
                self.expect(T::Comma)?;
                args.push(self.value_expression()?);
                while self.eat(T::Comma) {
                    args.push(self.value_expression()?);
                }
                self.expect(T::RightParen)?;
                Ok(Case::Coalesce(args))
            }
            _ => {
                if self.at_kw(K::When) {
                    self.searched_case()
                } else {
                    self.simple_case()
                }
            }
        }
    }

    /// `searchedCase: CASE searchedWhenClause+ elseClause? END`.
    fn searched_case(&mut self) -> PResult<Case> {
        let mut whens = Vec::new();
        while self.eat_kw(K::When) {
            let condition = self.value_expression()?;
            self.expect_kw(K::Then)?;
            let result = self.value_expression()?;
            whens.push((condition, result));
        }
        let else_result = self.else_clause()?;
        self.expect_kw(K::End)?;
        Ok(Case::Searched { whens, else_result })
    }

    /// `elseClause: ELSE result`, optional.
    fn else_clause(&mut self) -> PResult<Option<BoxSpanned<Expr>>> {
        if self.eat_kw(K::Else) {
            Ok(Some(Box::new(self.value_expression()?)))
        } else {
            Ok(None)
        }
    }

    /// `simpleCase: CASE caseOperand simpleWhenClause+ elseClause? END`;
    /// `caseOperand: nonParenthesizedValueExpressionPrimary |
    /// elementVariableReference`.
    fn simple_case(&mut self) -> PResult<Case> {
        let operand = Box::new(self.non_parenthesized_value_expression_primary()?);
        let mut whens = Vec::new();
        loop {
            let start = self.start();
            self.expect_kw(K::When)?;
            let mut operands = Vec::new();
            loop {
                let s = self.start();
                let w = self.when_operand()?;
                operands.push(self.spanned(s, w));
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.expect_kw(K::Then)?;
            let result = self.value_expression()?;
            whens.push(self.spanned(start, SimpleWhen { operands, result }));
            if !self.at_kw(K::When) {
                break;
            }
        }
        let else_result = self.else_clause()?;
        self.expect_kw(K::End)?;
        Ok(Case::Simple {
            operand,
            whens,
            else_result,
        })
    }

    /// `nonParenthesizedValueExpressionPrimary`: a primary that is not
    /// just a parenthesized expression.
    fn non_parenthesized_value_expression_primary(&mut self) -> PResult<Spanned<Expr>> {
        let e = self.value_expression_primary()?;
        if matches!(e.0, Expr::Parenthesized(_)) {
            return self.fail_rule("a primary that is not parenthesized");
        }
        Ok(e)
    }

    /// `whenOperand: nonParenthesizedValueExpressionPrimary | compOp
    /// valueExpression | nullPredicatePart2 | valueTypePredicatePart2 |
    /// normalizedPredicatePart2 | directedPredicatePart2 |
    /// labeledPredicatePart2 | sourcePredicatePart2 |
    /// destinationPredicatePart2`.
    fn when_operand(&mut self) -> PResult<WhenOperand> {
        if let Some(op) = comparison(self.peek()) {
            self.bump();
            return Ok(WhenOperand::Comparison {
                op,
                value: self.value_expression()?,
            });
        }
        if self.eat(T::Colon) {
            return Ok(WhenOperand::Labeled {
                not: false,
                label: self.spanned_label_expression()?,
            });
        }
        let Some(tail) = self.is_tail(0) else {
            self.at_kw(K::Is);
            return Ok(WhenOperand::Value(
                self.non_parenthesized_value_expression_primary()?,
            ));
        };
        if tail == IsTail::Normalized {
            let (not, form) = self.normalized_predicate_part2()?;
            return Ok(WhenOperand::Normalized { not, form });
        }
        self.bump();
        let not = self.eat_kw(K::Not);
        Ok(match tail {
            IsTail::Null => {
                self.expect_kw(K::Null)?;
                WhenOperand::Null { not }
            }
            IsTail::Type => {
                self.typed();
                WhenOperand::ValueType {
                    not,
                    value_type: Box::new(self.value_type()?),
                }
            }
            IsTail::Directed => {
                self.bump();
                WhenOperand::Directed { not }
            }
            IsTail::Labeled => {
                self.bump();
                WhenOperand::Labeled {
                    not,
                    label: self.spanned_label_expression()?,
                }
            }
            IsTail::Source => {
                self.bump();
                self.expect_kw(K::Of)?;
                WhenOperand::Source {
                    not,
                    edge: self.regular_identifier()?,
                }
            }
            IsTail::Destination => {
                self.bump();
                self.expect_kw(K::Of)?;
                WhenOperand::Destination {
                    not,
                    edge: self.regular_identifier()?,
                }
            }
            // `IS TRUE` is no when operand.
            IsTail::Truth | IsTail::Normalized => return self.fail_rule("a when operand"),
        })
    }

    /// `numericValueExpression`, left-recursive with 5 alternatives: sign
    /// prefix (5), `*` `/` (4), `+` `-` (3); `valueExpressionPrimary` or
    /// `numericValueFunction` as primary.
    pub(super) fn numeric_value_expression(&mut self) -> PResult<Spanned<Expr>> {
        self.numeric_value_expression_bp(0)
    }

    fn numeric_value_expression_bp(&mut self, min: u8) -> PResult<Spanned<Expr>> {
        let _guard = self.nest()?;
        let start = self.start();
        let mut left = match self.peek() {
            T::PlusSign | T::MinusSign => {
                let op = if self.bump().kind == T::PlusSign {
                    UnaryOp::Plus
                } else {
                    UnaryOp::Minus
                };
                let op = self.spanned(start, op);
                let child = Box::new(self.numeric_value_expression_bp(5)?);
                Expr::Unary { op, child }
            }
            k if starts_numeric_function(k) => Expr::Function(self.value_function()?),
            _ => self.value_expression_primary()?.0,
        };
        let mut chain = Vec::new();
        loop {
            let (prec, op) = match self.peek() {
                T::Asterisk => (4, BinaryOp::Mul),
                T::Solidus => (4, BinaryOp::Div),
                T::PlusSign => (3, BinaryOp::Add),
                T::MinusSign => (3, BinaryOp::Sub),
                _ => break,
            };
            if prec < min {
                break;
            }
            chain.push(self.nest()?);
            let l = Box::new(self.spanned(start, left));
            let op_start = self.start();
            self.bump();
            let op = self.spanned(op_start, op);
            let right = Box::new(self.numeric_value_expression_bp(prec + 1)?);
            left = Expr::Binary { op, left: l, right };
        }
        Ok(self.spanned(start, left))
    }

    /// `valueFunction: numericValueFunction | datetimeSubtraction |
    /// datetimeValueFunction | durationValueFunction |
    /// characterOrByteStringFunction | listValueFunction`. `ABS` is the
    /// numeric function (TS-19 ambiguity `valueFunction`, alternative 1).
    fn value_function(&mut self) -> PResult<Function> {
        let T::Keyword(k) = self.bump().kind else {
            return Err(Fail);
        };
        if let Some(kind) = numeric_unary(k) {
            return self.unary_function(kind, true);
        }
        let unary = |kind| Some(kind);
        let value_arg = match k {
            K::CharLength | K::CharacterLength => unary(UnaryFunction::CharLength),
            K::ByteLength | K::OctetLength => unary(UnaryFunction::ByteLength),
            K::PathLength => unary(UnaryFunction::PathLength),
            K::Cardinality => unary(UnaryFunction::Cardinality),
            K::Size => unary(UnaryFunction::Size),
            K::Abs => unary(UnaryFunction::Abs),
            K::Upper => unary(UnaryFunction::Upper),
            K::Lower => unary(UnaryFunction::Lower),
            K::Elements => unary(UnaryFunction::Elements),
            _ => None,
        };
        if let Some(kind) = value_arg {
            return self.unary_function(kind, false);
        }
        match k {
            K::Mod | K::Log | K::Power => {
                let kind = match k {
                    K::Mod => BinaryFunction::Mod,
                    K::Log => BinaryFunction::Log,
                    _ => BinaryFunction::Power,
                };
                self.expect(T::LeftParen)?;
                let left = Box::new(self.numeric_value_expression()?);
                self.expect(T::Comma)?;
                let right = Box::new(self.numeric_value_expression()?);
                self.expect(T::RightParen)?;
                Ok(Function::Binary { kind, left, right })
            }
            K::Left | K::Right => {
                // subCharacterOrByteString: (LEFT | RIGHT) ( valueExpression
                // , stringLength ).
                self.expect(T::LeftParen)?;
                let left = Box::new(self.value_expression()?);
                self.expect(T::Comma)?;
                let right = Box::new(self.numeric_value_expression()?);
                self.expect(T::RightParen)?;
                let kind = if k == K::Left {
                    BinaryFunction::Left
                } else {
                    BinaryFunction::Right
                };
                Ok(Function::Binary { kind, left, right })
            }
            K::Trim => self.trim_function(),
            K::Btrim | K::Ltrim | K::Rtrim => {
                // trimMultiCharacterCharacterString.
                self.expect(T::LeftParen)?;
                let source = Box::new(self.value_expression()?);
                let characters = if self.eat(T::Comma) {
                    Some(Box::new(self.value_expression()?))
                } else {
                    None
                };
                self.expect(T::RightParen)?;
                let kind = match k {
                    K::Btrim => TrimSpecification::Both,
                    K::Ltrim => TrimSpecification::Leading,
                    _ => TrimSpecification::Trailing,
                };
                Ok(Function::MultiTrim {
                    kind,
                    source,
                    characters,
                })
            }
            K::Normalize => {
                // normalizeCharacterString: NORMALIZE ( valueExpression (,
                // normalForm)? ).
                self.expect(T::LeftParen)?;
                let source = Box::new(self.value_expression()?);
                let form = if self.eat(T::Comma) {
                    match self.normal_form() {
                        Some(f) => Some(f),
                        None => return self.fail_rule("a normal form"),
                    }
                } else {
                    None
                };
                self.expect(T::RightParen)?;
                Ok(Function::Normalize { source, form })
            }
            K::DurationBetween => {
                // datetimeSubtraction: DURATION_BETWEEN ( v1 , v2 )
                // temporalDurationQualifier?.
                self.expect(T::LeftParen)?;
                let left = Box::new(self.value_expression()?);
                self.expect(T::Comma)?;
                let right = Box::new(self.value_expression()?);
                self.expect(T::RightParen)?;
                let year_to_month = if self.at_kw(K::Year) || self.at_kw(K::Day) {
                    Some(self.temporal_duration_qualifier()?)
                } else {
                    None
                };
                Ok(Function::DurationBetween {
                    left,
                    right,
                    year_to_month,
                })
            }
            K::Duration => {
                // durationFunction: DURATION ( durationString |
                // recordConstructor ).
                self.expect(T::LeftParen)?;
                let p = self.temporal_function_parameters()?;
                self.expect(T::RightParen)?;
                Ok(Function::Duration(Box::new(p)))
            }
            _ => self.datetime_function(k),
        }
    }

    /// A function of one argument in parentheses: a
    /// `numericValueExpression` or a `valueExpression`.
    fn unary_function(&mut self, kind: UnaryFunction, numeric: bool) -> PResult<Function> {
        self.expect(T::LeftParen)?;
        let arg = if numeric {
            self.numeric_value_expression()?
        } else {
            self.value_expression()?
        };
        self.expect(T::RightParen)?;
        Ok(Function::Unary {
            kind,
            arg: Box::new(arg),
        })
    }

    /// `TRIM ( trimOperands )` (`trimSingleCharacterOrByteString`) or
    /// `TRIM ( listValueExpression , numericValueExpression )`
    /// (`trimListFunction`); `trimOperands: (trimSpecification?
    /// trimCharacterOrByteString? FROM)? trimCharacterOrByteStringSource`.
    fn trim_function(&mut self) -> PResult<Function> {
        self.expect(T::LeftParen)?;
        let specification = match self.peek() {
            T::Keyword(K::Leading) => Some(TrimSpecification::Leading),
            T::Keyword(K::Trailing) => Some(TrimSpecification::Trailing),
            T::Keyword(K::Both) => Some(TrimSpecification::Both),
            _ => None,
        };
        let f = if specification.is_some() || self.at_kw(K::From) {
            if specification.is_some() {
                self.bump();
            }
            let characters = if self.eat_kw(K::From) {
                None
            } else {
                let c = self.value_expression()?;
                self.expect_kw(K::From)?;
                Some(Box::new(c))
            };
            Function::Trim {
                specification,
                characters,
                source: Box::new(self.value_expression()?),
            }
        } else {
            let first = Box::new(self.value_expression()?);
            if self.eat_kw(K::From) {
                Function::Trim {
                    specification: None,
                    characters: Some(first),
                    source: Box::new(self.value_expression()?),
                }
            } else if self.eat(T::Comma) {
                Function::Binary {
                    kind: BinaryFunction::TrimList,
                    left: first,
                    right: Box::new(self.numeric_value_expression()?),
                }
            } else {
                Function::Trim {
                    specification: None,
                    characters: None,
                    source: first,
                }
            }
        };
        self.expect(T::RightParen)?;
        Ok(f)
    }

    /// `datetimeValueFunction`: `dateFunction`, `timeFunction`,
    /// `datetimeFunction`, `localtimeFunction`, `localdatetimeFunction`.
    fn datetime_function(&mut self, k: K) -> PResult<Function> {
        let (kind, parens) = match k {
            K::CurrentDate => (DatetimeFunction::CurrentDate, Some(false)),
            K::Date => (DatetimeFunction::Date, Some(true)),
            K::CurrentTime => (DatetimeFunction::CurrentTime, Some(false)),
            K::ZonedTime => (DatetimeFunction::ZonedTime, Some(true)),
            K::LocalTime => (DatetimeFunction::LocalTime, None),
            K::CurrentTimestamp => (DatetimeFunction::CurrentTimestamp, Some(false)),
            K::ZonedDatetime => (DatetimeFunction::ZonedDatetime, Some(true)),
            K::LocalTimestamp => (DatetimeFunction::LocalTimestamp, Some(false)),
            _ => (DatetimeFunction::LocalDatetime, Some(true)),
        };
        // `Some(true)`: parentheses required; `None`: optional (LOCAL_TIME).
        let open = match parens {
            Some(false) => false,
            Some(true) => {
                self.expect(T::LeftParen)?;
                true
            }
            None => self.eat(T::LeftParen),
        };
        let mut parameters = None;
        if open && !self.eat(T::RightParen) {
            parameters = Some(Box::new(self.temporal_function_parameters()?));
            self.expect(T::RightParen)?;
        }
        Ok(Function::Datetime { kind, parameters })
    }

    /// `dateFunctionParameters`, `timeFunctionParameters`,
    /// `datetimeFunctionParameters` and `durationFunctionParameters`: a
    /// character string literal or a `recordConstructor`.
    fn temporal_function_parameters(&mut self) -> PResult<Spanned<Expr>> {
        let start = self.start();
        if self.at(T::SingleQuoted) || self.at(T::DoubleQuoted) {
            let s = self.character_string_literal()?;
            return Ok(Spanned(Expr::Literal(Literal::String(s.0)), s.1));
        }
        if !(self.at_kw(K::Record) || self.at(T::LeftBrace)) {
            return Err(Fail);
        }
        let r = self.record_constructor()?;
        Ok(self.spanned(start, Expr::Literal(Literal::Record(r))))
    }
}

/// The match statements of a nested procedure made of match statements
/// only, which `EXISTS { … }` reads as a match block.
fn match_block(
    body: &crate::query::ast::Procedure,
) -> Option<VecSpanned<crate::query::ast::MatchStatement>> {
    if body.at.is_some()
        || !body.binding_variable_defs.is_empty()
        || !body.next_statements.is_empty()
    {
        return None;
    }
    let Statement::Data(data) = &body.statement.0 else {
        return None;
    };
    if data.use_graph.is_some() {
        return None;
    }
    let DataModifyingBody::Statements {
        statements,
        result: None,
    } = &data.body
    else {
        return None;
    };
    statements
        .iter()
        .map(|s| match &s.0 {
            SimpleDataAccessingStatement::Query(q) => match &**q {
                SimpleQueryStatement::Match(m) => Some(Spanned(m.clone(), s.1)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}
