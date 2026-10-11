//! Graph patterns, insert patterns, label expressions and simplified
//! path patterns (`GQL.g4` 16.3 to 16.12).

use super::{Expected, Fail, PResult, Parser};
use crate::query::ast::{
    EdgeDirection, ElementPattern, ElementPatternFiller, ElementPatternPredicate, GraphPattern,
    GraphPatternBindingTable, GroupedPathPattern, InsertEdge, InsertElement, InsertPathPattern,
    LabelExpr, LabelSetSpec, MatchMode, PathMode, PathPattern, PathPatternExpr, PathPatternPrefix,
    PathSearch, PatternQuantifier, PropertyKeyValuePair, SimplifiedExpr, SimplifiedPathPattern,
};
use crate::query::lexer::{Keyword as K, TokenKind as T};
use crate::query::span::{Spanned, VecSpanned};

/// Whether `kind` starts a `pathPrimary` (and so a `pathTerm`).
fn starts_path_primary(kind: T) -> bool {
    matches!(
        kind,
        T::LeftParen
            | T::LeftArrowBracket
            | T::TildeLeftBracket
            | T::MinusLeftBracket
            | T::LeftArrowTildeBracket
            | T::LeftArrow
            | T::Tilde
            | T::RightArrow
            | T::LeftArrowTilde
            | T::TildeRightArrow
            | T::LeftMinusRight
            | T::MinusSign
            | T::LeftMinusSlash
            | T::TildeSlash
            | T::MinusSlash
            | T::LeftTildeSlash
    )
}

/// The path mode a keyword names.
fn path_mode(kind: T) -> Option<PathMode> {
    match kind {
        T::Keyword(K::Walk) => Some(PathMode::Walk),
        T::Keyword(K::Trail) => Some(PathMode::Trail),
        T::Keyword(K::Simple) => Some(PathMode::Simple),
        T::Keyword(K::Acyclic) => Some(PathMode::Acyclic),
        _ => None,
    }
}

impl Parser<'_> {
    /// `graphPatternBindingTable: graphPattern graphPatternYieldClause?`.
    ///
    /// `graphPatternYieldItemList: graphPatternYieldItem (COMMA …)*`: a
    /// comma continues the list unless what follows starts the next
    /// `selectGraphMatch` (`graphExpression matchStatement`), whose
    /// graph may be a lone name before `MATCH`/`OPTIONAL`, or a path.
    pub(super) fn graph_pattern_binding_table(&mut self) -> PResult<GraphPatternBindingTable> {
        let start = self.start();
        let g = self.graph_pattern()?;
        let pattern = self.spanned(start, g);
        let mut yield_clause = Vec::new();
        if self.eat_kw(K::Yield) {
            loop {
                yield_clause.push(self.regular_identifier()?);
                let more = self.peek() == T::Comma
                    && Self::is_regular_identifier(self.peek_nth(1))
                    && !matches!(
                        self.peek_nth(2),
                        T::Keyword(K::Match | K::Optional) | T::Period | T::Solidus
                    );
                if !more {
                    break;
                }
                self.bump();
            }
        }
        Ok(GraphPatternBindingTable {
            pattern,
            yield_clause,
        })
    }

    /// `graphPattern: matchMode? pathPatternList keepClause?
    /// graphPatternWhereClause?`. The `WHERE` belongs to the graph
    /// pattern (TS-19 ambiguity `graphPattern`, alternative 1).
    pub(super) fn graph_pattern(&mut self) -> PResult<GraphPattern> {
        let _guard = self.nest()?;
        let match_mode = self.match_mode()?;
        let patterns = self.path_pattern_list()?;
        let keep = if self.eat_kw(K::Keep) {
            // keepClause: KEEP pathPatternPrefix.
            let start = self.start();
            let p = self.path_pattern_prefix()?;
            Some(self.spanned(start, p))
        } else {
            None
        };
        let where_clause = if self.eat_kw(K::Where) {
            Some(self.value_expression()?)
        } else {
            None
        };
        Ok(GraphPattern {
            match_mode,
            patterns,
            keep,
            where_clause,
        })
    }

    /// `matchMode: repeatableElementsMatchMode | differentEdgesMatchMode`,
    /// optional. `REPEATABLE` and `DIFFERENT` are non-reserved: followed
    /// by `=` they name a path variable instead. `BINDINGS` is optional
    /// and non-reserved: it is skipped when `=` follows it.
    fn match_mode(&mut self) -> PResult<Option<Spanned<MatchMode>>> {
        let start = self.start();
        if self.peek_nth(1) == T::Equals {
            return Ok(None);
        }
        let bindings_follow =
            |p: &Self| p.peek() == T::Keyword(K::Bindings) && p.peek_nth(1) != T::Equals;
        if self.eat_kw(K::Repeatable) {
            // elementBindingsOrElements: ELEMENT BINDINGS? | ELEMENTS.
            if self.eat_kw(K::Element) {
                if bindings_follow(self) {
                    self.bump();
                }
            } else {
                self.expect_kw(K::Elements)?;
            }
            return Ok(Some(self.spanned(start, MatchMode::RepeatableElements)));
        }
        if self.eat_kw(K::Different) {
            // edgeBindingsOrEdges: edgeSynonym BINDINGS? | edgesSynonym.
            if self.eat_kw(K::Edge) || self.eat_kw(K::Relationship) {
                if bindings_follow(self) {
                    self.bump();
                }
            } else if !(self.eat_kw(K::Edges) || self.eat_kw(K::Relationships)) {
                return Err(Fail);
            }
            return Ok(Some(self.spanned(start, MatchMode::DifferentEdges)));
        }
        Ok(None)
    }

    /// `pathPatternList: pathPattern (COMMA pathPattern)*`. A comma
    /// continues the list only when a path pattern follows it; otherwise
    /// it separates the `selectGraphMatch`es of a `SELECT`.
    fn path_pattern_list(&mut self) -> PResult<VecSpanned<PathPattern>> {
        let mut list = Vec::new();
        loop {
            let start = self.start();
            let p = self.path_pattern()?;
            list.push(self.spanned(start, p));
            if !(self.peek() == T::Comma && self.starts_path_pattern(1)) {
                return Ok(list);
            }
            self.bump();
        }
    }

    /// Whether the token `n` places ahead starts a `pathPattern`.
    fn starts_path_pattern(&self, n: usize) -> bool {
        let k = self.peek_nth(n);
        starts_path_primary(k)
            || (Self::is_regular_identifier(k) && self.peek_nth(n + 1) == T::Equals)
            || path_mode(k).is_some()
            || matches!(k, T::Keyword(K::All | K::Any | K::Shortest))
    }

    /// `pathPattern: pathVariableDeclaration? pathPatternPrefix?
    /// pathPatternExpression`; `pathVariableDeclaration: pathVariable =`.
    fn path_pattern(&mut self) -> PResult<PathPattern> {
        let variable = if Self::is_regular_identifier(self.peek()) && self.peek_nth(1) == T::Equals
        {
            let v = self.regular_identifier()?;
            self.bump();
            Some(v)
        } else {
            None
        };
        let prefix = if path_mode(self.peek()).is_some()
            || matches!(self.peek(), T::Keyword(K::All | K::Any | K::Shortest))
        {
            let start = self.start();
            let p = self.path_pattern_prefix()?;
            Some(self.spanned(start, p))
        } else {
            None
        };
        let start = self.start();
        let expr = self.path_pattern_expression()?;
        Ok(PathPattern {
            variable,
            prefix,
            expr: self.spanned(start, expr),
        })
    }

    /// `pathPatternPrefix: pathModePrefix | pathSearchPrefix`.
    fn path_pattern_prefix(&mut self) -> PResult<PathPatternPrefix> {
        if let Some(mode) = path_mode(self.peek()) {
            // pathModePrefix: pathMode pathOrPaths?.
            self.bump();
            self.path_or_paths();
            return Ok(PathPatternPrefix::PathMode(mode));
        }
        Ok(PathPatternPrefix::PathSearch(self.path_search_prefix()?))
    }

    /// `pathOrPaths: PATH | PATHS`, optional.
    fn path_or_paths(&mut self) {
        if !self.eat_kw(K::Path) {
            self.eat_kw(K::Paths);
        }
    }

    /// `pathMode`, optional, with its span.
    fn opt_path_mode(&mut self) -> Option<Spanned<PathMode>> {
        let start = self.start();
        let mode = path_mode(self.peek())?;
        self.bump();
        Some(self.spanned(start, mode))
    }

    /// `pathSearchPrefix: allPathSearch | anyPathSearch |
    /// shortestPathSearch` with the shortest searches
    /// `allShortestPathSearch`, `anyShortestPathSearch`,
    /// `countedShortestPathSearch` and `countedShortestGroupSearch`.
    fn path_search_prefix(&mut self) -> PResult<PathSearch> {
        if self.eat_kw(K::All) {
            let shortest = self.eat_kw(K::Shortest);
            let mode = self.opt_path_mode();
            self.path_or_paths();
            return Ok(if shortest {
                PathSearch::AllShortest(mode)
            } else {
                PathSearch::All(mode)
            });
        }
        if self.eat_kw(K::Any) {
            if self.eat_kw(K::Shortest) {
                let mode = self.opt_path_mode();
                self.path_or_paths();
                return Ok(PathSearch::AnyShortest(mode));
            }
            let number = if matches!(
                self.peek(),
                T::Integer | T::Hexadecimal | T::Octal | T::Binary | T::GeneralParameter
            ) {
                Some(self.non_negative_integer_specification()?)
            } else {
                None
            };
            let mode = self.opt_path_mode();
            self.path_or_paths();
            return Ok(PathSearch::Any { number, mode });
        }
        self.expect_kw(K::Shortest)?;
        let number = if matches!(
            self.peek(),
            T::Integer | T::Hexadecimal | T::Octal | T::Binary | T::GeneralParameter
        ) {
            Some(self.non_negative_integer_specification()?)
        } else {
            None
        };
        let mode = self.opt_path_mode();
        self.path_or_paths();
        if self.eat_kw(K::Group) || self.eat_kw(K::Groups) {
            return Ok(PathSearch::CountedShortestGroup { number, mode });
        }
        match number {
            Some(number) => Ok(PathSearch::CountedShortest { number, mode }),
            None => self.fail(Expected::Token(T::Keyword(K::Group))),
        }
    }

    /// `pathPatternExpression: pathTerm | pathTerm
    /// (MULTISET_ALTERNATION_OPERATOR pathTerm)+ | pathTerm (VERTICAL_BAR
    /// pathTerm)+`.
    pub(super) fn path_pattern_expression(&mut self) -> PResult<PathPatternExpr> {
        let _guard = self.nest()?;
        let start = self.start();
        let first = self.path_term()?;
        let first = self.spanned(start, first);
        let op = if self.at(T::MultisetAlternation) {
            T::MultisetAlternation
        } else if self.at(T::VerticalBar) {
            T::VerticalBar
        } else {
            return Ok(first.0);
        };
        let mut terms = vec![first];
        while self.eat(op) {
            let start = self.start();
            let t = self.path_term()?;
            terms.push(self.spanned(start, t));
        }
        Ok(if op == T::VerticalBar {
            PathPatternExpr::Union(terms)
        } else {
            PathPatternExpr::Alternation(terms)
        })
    }

    /// `pathTerm: pathFactor+`.
    fn path_term(&mut self) -> PResult<PathPatternExpr> {
        let mut factors = Vec::new();
        loop {
            let start = self.start();
            let f = self.path_factor()?;
            factors.push(self.spanned(start, f));
            if !starts_path_primary(self.peek()) {
                for k in [
                    T::LeftParen,
                    T::MinusLeftBracket,
                    T::RightArrow,
                    T::MinusSign,
                ] {
                    self.expect_here(Expected::Token(k));
                }
                break;
            }
        }
        Ok(if factors.len() == 1 {
            factors
                .pop()
                .map_or(PathPatternExpr::Concat(Vec::new()), |f| f.0)
        } else {
            PathPatternExpr::Concat(factors)
        })
    }

    /// `pathFactor: pathPrimary | pathPrimary graphPatternQuantifier |
    /// pathPrimary QUESTION_MARK`.
    fn path_factor(&mut self) -> PResult<PathPatternExpr> {
        let start = self.start();
        let primary = self.path_primary()?;
        let primary = self.spanned(start, primary);
        if self.eat(T::QuestionMark) {
            return Ok(PathPatternExpr::Questioned(Box::new(primary)));
        }
        if let Some(q) = self.graph_pattern_quantifier()? {
            return Ok(PathPatternExpr::Quantified {
                path: Box::new(primary),
                quantifier: q,
            });
        }
        Ok(primary.0)
    }

    /// `graphPatternQuantifier: ASTERISK | PLUS_SIGN | fixedQuantifier |
    /// generalQuantifier`, optional; `fixedQuantifier: { unsignedInteger
    /// }`; `generalQuantifier: { lowerBound? , upperBound? }`.
    pub(super) fn graph_pattern_quantifier(
        &mut self,
    ) -> PResult<Option<Spanned<PatternQuantifier>>> {
        let start = self.start();
        let q = if self.eat(T::Asterisk) {
            PatternQuantifier::Asterisk
        } else if self.eat(T::PlusSign) {
            PatternQuantifier::Plus
        } else if self.eat(T::LeftBrace) {
            let lower = if self.at(T::Comma) {
                None
            } else {
                Some(self.unsigned_integer()?)
            };
            if let (Some(n), true) = (&lower, self.at(T::RightBrace)) {
                let n = n.clone();
                self.bump();
                return Ok(Some(self.spanned(start, PatternQuantifier::Fixed(n))));
            }
            self.expect(T::Comma)?;
            let upper = if self.at(T::RightBrace) {
                None
            } else {
                Some(self.unsigned_integer()?)
            };
            self.expect(T::RightBrace)?;
            PatternQuantifier::General {
                lower_bound: lower,
                upper_bound: upper,
            }
        } else {
            return Ok(None);
        };
        Ok(Some(self.spanned(start, q)))
    }

    /// `pathPrimary: elementPattern | parenthesizedPathPatternExpression |
    /// simplifiedPathPatternExpression`.
    ///
    /// A `(` opens a parenthesized path pattern when a path pattern, a
    /// subpath variable (`name =`), or a path mode followed by a path or
    /// `PATH`/`PATHS` comes next; otherwise it opens a node pattern.
    fn path_primary(&mut self) -> PResult<PathPatternExpr> {
        match self.peek() {
            T::LeftParen => {
                let (a, b) = (self.peek_nth(1), self.peek_nth(2));
                let grouped = starts_path_primary(a)
                    || (Self::is_regular_identifier(a) && b == T::Equals)
                    || (path_mode(a).is_some()
                        && (starts_path_primary(b) || matches!(b, T::Keyword(K::Path | K::Paths))));
                if grouped {
                    Ok(PathPatternExpr::Grouped(
                        self.parenthesized_path_pattern_expression()?,
                    ))
                } else {
                    Ok(PathPatternExpr::Element(self.node_pattern()?))
                }
            }
            T::LeftMinusSlash | T::TildeSlash | T::MinusSlash | T::LeftTildeSlash => Ok(
                PathPatternExpr::Simplified(self.simplified_path_pattern_expression()?),
            ),
            _ => Ok(PathPatternExpr::Element(self.edge_pattern()?)),
        }
    }

    /// `nodePattern: ( elementPatternFiller )`.
    fn node_pattern(&mut self) -> PResult<ElementPattern> {
        self.expect(T::LeftParen)?;
        let filler = self.element_pattern_filler()?;
        self.expect(T::RightParen)?;
        Ok(ElementPattern::Node(filler))
    }

    /// `elementPatternFiller: elementVariableDeclaration?
    /// isLabelExpression? elementPatternPredicate?`.
    fn element_pattern_filler(&mut self) -> PResult<ElementPatternFiller> {
        let variable = if self.at_regular_identifier() {
            Some(self.regular_identifier()?)
        } else {
            None
        };
        let label = if self.at_kw(K::Is) || self.at(T::Colon) {
            // isLabelExpression: isOrColon labelExpression.
            self.bump();
            let start = self.start();
            let l = self.label_expression()?;
            Some(self.spanned(start, l))
        } else {
            None
        };
        let start = self.start();
        let predicate = if self.eat_kw(K::Where) {
            // elementPatternWhereClause: WHERE searchCondition.
            let e = self.value_expression()?;
            Some(self.spanned(start, ElementPatternPredicate::Where(e)))
        } else if self.at(T::LeftBrace) {
            let props = self.element_property_specification()?;
            Some(self.spanned(start, ElementPatternPredicate::Property(props)))
        } else {
            None
        };
        Ok(ElementPatternFiller {
            variable,
            label,
            predicate,
        })
    }

    /// `elementPropertySpecification: { propertyKeyValuePairList }`.
    pub(super) fn element_property_specification(
        &mut self,
    ) -> PResult<VecSpanned<PropertyKeyValuePair>> {
        self.expect(T::LeftBrace)?;
        let list = self.property_key_value_pair_list()?;
        self.expect(T::RightBrace)?;
        Ok(list)
    }

    /// `propertyKeyValuePairList: propertyKeyValuePair (COMMA
    /// propertyKeyValuePair)*`; `propertyKeyValuePair: propertyName COLON
    /// valueExpression`.
    pub(super) fn property_key_value_pair_list(
        &mut self,
    ) -> PResult<VecSpanned<PropertyKeyValuePair>> {
        let mut list = Vec::new();
        loop {
            let start = self.start();
            let name = self.identifier()?;
            self.expect(T::Colon)?;
            let value = self.value_expression()?;
            list.push(self.spanned(start, PropertyKeyValuePair { name, value }));
            if !self.eat(T::Comma) {
                return Ok(list);
            }
        }
    }

    /// `edgePattern: fullEdgePattern | abbreviatedEdgePattern`, with the
    /// seven `fullEdge…` rules: the opening token and the closing token
    /// together give the direction.
    fn edge_pattern(&mut self) -> PResult<ElementPattern> {
        use EdgeDirection as D;
        let abbreviated = match self.peek() {
            T::LeftArrow => Some(D::Left),
            T::Tilde => Some(D::Undirected),
            T::RightArrow => Some(D::Right),
            T::LeftArrowTilde => Some(D::LeftOrUndirected),
            T::TildeRightArrow => Some(D::UndirectedOrRight),
            T::LeftMinusRight => Some(D::LeftOrRight),
            T::MinusSign => Some(D::AnyDirection),
            _ => None,
        };
        if let Some(direction) = abbreviated {
            self.bump();
            return Ok(ElementPattern::Edge {
                direction,
                filler: ElementPatternFiller::default(),
                abbreviated: true,
            });
        }
        let open = self.peek();
        let closes: &[(T, D)] = match open {
            T::LeftArrowBracket => &[
                (T::RightBracketMinus, D::Left),
                (T::BracketRightArrow, D::LeftOrRight),
            ],
            T::TildeLeftBracket => &[
                (T::RightBracketTilde, D::Undirected),
                (T::BracketTildeRightArrow, D::UndirectedOrRight),
            ],
            T::MinusLeftBracket => &[
                (T::BracketRightArrow, D::Right),
                (T::RightBracketMinus, D::AnyDirection),
            ],
            T::LeftArrowTildeBracket => &[(T::RightBracketTilde, D::LeftOrUndirected)],
            _ => {
                for k in [
                    T::LeftParen,
                    T::MinusLeftBracket,
                    T::LeftArrowBracket,
                    T::TildeLeftBracket,
                    T::LeftArrowTildeBracket,
                    T::RightArrow,
                    T::MinusSign,
                ] {
                    self.expect_here(Expected::Token(k));
                }
                return Err(Fail);
            }
        };
        self.bump();
        let filler = self.element_pattern_filler()?;
        for &(close, direction) in closes {
            if self.eat(close) {
                return Ok(ElementPattern::Edge {
                    direction,
                    filler,
                    abbreviated: false,
                });
            }
        }
        Err(Fail)
    }

    /// `parenthesizedPathPatternExpression: ( subpathVariableDeclaration?
    /// pathModePrefix? pathPatternExpression
    /// parenthesizedPathPatternWhereClause? )`.
    fn parenthesized_path_pattern_expression(&mut self) -> PResult<GroupedPathPattern> {
        self.expect(T::LeftParen)?;
        let variable = if Self::is_regular_identifier(self.peek()) && self.peek_nth(1) == T::Equals
        {
            // subpathVariableDeclaration: subpathVariable =.
            let v = self.regular_identifier()?;
            self.bump();
            Some(v)
        } else {
            None
        };
        let mode = match self.opt_path_mode() {
            Some(m) => {
                self.path_or_paths();
                Some(m)
            }
            None => None,
        };
        let start = self.start();
        let expr = self.path_pattern_expression()?;
        let expr = Box::new(self.spanned(start, expr));
        let where_clause = if self.eat_kw(K::Where) {
            Some(self.value_expression()?)
        } else {
            None
        };
        self.expect(T::RightParen)?;
        Ok(GroupedPathPattern {
            variable,
            mode,
            expr,
            where_clause,
        })
    }

    /// `labelExpression`: a Pratt loop over ANTLR's precedences of the
    /// left-recursive rule (6 alternatives): `!` (prefix, 6), `&` (5),
    /// `|` (4); a name, `%` or `( labelExpression )` as primary.
    pub(super) fn label_expression(&mut self) -> PResult<LabelExpr> {
        self.label_expression_bp(0)
    }

    fn label_expression_bp(&mut self, min: u8) -> PResult<LabelExpr> {
        let _guard = self.nest()?;
        let start = self.start();
        let mut left = if self.eat(T::ExclamationMark) {
            let s = self.start();
            let operand = self.label_expression_bp(6)?;
            LabelExpr::Negation(Box::new(self.spanned(s, operand)))
        } else if self.eat(T::Percent) {
            LabelExpr::Wildcard
        } else if self.eat(T::LeftParen) {
            let s = self.start();
            let inner = self.label_expression_bp(0)?;
            let inner = self.spanned(s, inner);
            self.expect(T::RightParen)?;
            LabelExpr::Parenthesized(Box::new(inner))
        } else {
            self.at(T::ExclamationMark);
            self.at(T::Percent);
            self.at(T::LeftParen);
            LabelExpr::Label(self.identifier()?.0)
        };
        let mut chain = Vec::new();
        loop {
            let (prec, conjunction) = if self.at(T::Ampersand) {
                (5, true)
            } else if self.at(T::VerticalBar) {
                (4, false)
            } else {
                return Ok(left);
            };
            if prec < min {
                return Ok(left);
            }
            chain.push(self.nest()?);
            let l = self.spanned(start, left);
            self.bump();
            let s = self.start();
            let right = self.label_expression_bp(prec + 1)?;
            let r = Box::new(self.spanned(s, right));
            left = if conjunction {
                LabelExpr::Conjunction(Box::new(l), r)
            } else {
                LabelExpr::Disjunction(Box::new(l), r)
            };
        }
    }

    /// `insertGraphPattern: insertPathPatternList`;
    /// `insertPathPatternList: insertPathPattern (COMMA
    /// insertPathPattern)*`.
    pub(super) fn insert_graph_pattern(&mut self) -> PResult<VecSpanned<InsertPathPattern>> {
        let mut list = Vec::new();
        loop {
            let start = self.start();
            let p = self.insert_path_pattern()?;
            list.push(self.spanned(start, p));
            if !self.eat(T::Comma) {
                return Ok(list);
            }
        }
    }

    /// `insertPathPattern: insertNodePattern (insertEdgePattern
    /// insertNodePattern)*`.
    fn insert_path_pattern(&mut self) -> PResult<InsertPathPattern> {
        let start = self.insert_node_pattern()?;
        let mut steps = Vec::new();
        while let Some(edge) = self.insert_edge_pattern()? {
            let node = self.insert_node_pattern()?;
            steps.push((edge, node));
        }
        Ok(InsertPathPattern { start, steps })
    }

    /// `insertNodePattern: ( insertElementPatternFiller? )`.
    fn insert_node_pattern(&mut self) -> PResult<Spanned<InsertElement>> {
        let start = self.start();
        self.expect(T::LeftParen)?;
        let filler = self.insert_element_pattern_filler()?;
        self.expect(T::RightParen)?;
        Ok(self.spanned(start, filler))
    }

    /// `insertEdgePattern: insertEdgePointingLeft |
    /// insertEdgePointingRight | insertEdgeUndirected`, optional.
    fn insert_edge_pattern(&mut self) -> PResult<Option<Spanned<InsertEdge>>> {
        let start = self.start();
        let (direction, close) = if self.eat(T::LeftArrowBracket) {
            (EdgeDirection::Left, T::RightBracketMinus)
        } else if self.eat(T::MinusLeftBracket) {
            (EdgeDirection::Right, T::BracketRightArrow)
        } else if self.eat(T::TildeLeftBracket) {
            (EdgeDirection::Undirected, T::RightBracketTilde)
        } else {
            return Ok(None);
        };
        let filler = self.insert_element_pattern_filler()?;
        self.expect(close)?;
        Ok(Some(self.spanned(start, InsertEdge { direction, filler })))
    }

    /// `insertElementPatternFiller: elementVariableDeclaration
    /// labelAndPropertySetSpecification? | elementVariableDeclaration?
    /// labelAndPropertySetSpecification` (empty when absent), with
    /// `labelAndPropertySetSpecification: isOrColon labelSetSpecification
    /// elementPropertySpecification? | (isOrColon
    /// labelSetSpecification)? elementPropertySpecification`.
    fn insert_element_pattern_filler(&mut self) -> PResult<InsertElement> {
        let variable = if self.at_regular_identifier() {
            Some(self.regular_identifier()?)
        } else {
            None
        };
        let labels = if self.at_kw(K::Is) || self.at(T::Colon) {
            self.bump();
            let start = self.start();
            let l = self.label_set_specification()?;
            Some(self.spanned(start, l))
        } else {
            None
        };
        let properties = if self.at(T::LeftBrace) {
            Some(self.element_property_specification()?)
        } else {
            None
        };
        Ok(InsertElement {
            variable,
            labels,
            properties,
        })
    }

    /// `labelSetSpecification: labelName (AMPERSAND labelName)*`.
    pub(super) fn label_set_specification(&mut self) -> PResult<LabelSetSpec> {
        let mut labels = vec![self.identifier()?];
        while self.eat(T::Ampersand) {
            labels.push(self.identifier()?);
        }
        Ok(labels)
    }

    /// `simplifiedPathPatternExpression`: one of the seven
    /// `simplifiedDefaulting…` rules, `open simplifiedContents close`.
    fn simplified_path_pattern_expression(&mut self) -> PResult<SimplifiedPathPattern> {
        use EdgeDirection as D;
        let closes: &[(T, D)] = match self.bump().kind {
            T::LeftMinusSlash => &[
                (T::SlashMinus, D::Left),
                (T::SlashMinusRight, D::LeftOrRight),
            ],
            T::TildeSlash => &[
                (T::SlashTilde, D::Undirected),
                (T::SlashTildeRight, D::UndirectedOrRight),
            ],
            T::MinusSlash => &[
                (T::SlashMinusRight, D::Right),
                (T::SlashMinus, D::AnyDirection),
            ],
            _ => &[(T::SlashTilde, D::LeftOrUndirected)],
        };
        let start = self.start();
        let contents = self.simplified_contents()?;
        let contents = Box::new(self.spanned(start, contents));
        for &(close, direction) in closes {
            if self.eat(close) {
                return Ok(SimplifiedPathPattern {
                    direction,
                    contents,
                });
            }
        }
        Err(Fail)
    }

    /// `simplifiedContents: simplifiedTerm | simplifiedPathUnion |
    /// simplifiedMultisetAlternation`.
    fn simplified_contents(&mut self) -> PResult<SimplifiedExpr> {
        let _guard = self.nest()?;
        let start = self.start();
        let first = self.simplified_term()?;
        let first = self.spanned(start, first);
        let op = if self.at(T::VerticalBar) {
            T::VerticalBar
        } else if self.at(T::MultisetAlternation) {
            T::MultisetAlternation
        } else {
            return Ok(first.0);
        };
        let mut terms = vec![first];
        while self.eat(op) {
            let start = self.start();
            let t = self.simplified_term()?;
            terms.push(self.spanned(start, t));
        }
        Ok(if op == T::VerticalBar {
            SimplifiedExpr::Union(terms)
        } else {
            SimplifiedExpr::MultisetAlternation(terms)
        })
    }

    /// Whether the current token starts a `simplifiedFactorLow`.
    fn starts_simplified_factor(&self) -> bool {
        let k = self.peek();
        Self::is_identifier(k)
            || matches!(
                k,
                T::LeftParen
                    | T::ExclamationMark
                    | T::LeftAngleBracket
                    | T::Tilde
                    | T::LeftArrowTilde
                    | T::MinusSign
            )
    }

    /// `simplifiedTerm: simplifiedFactorLow | simplifiedTerm
    /// simplifiedFactorLow` (concatenation, left-associative).
    fn simplified_term(&mut self) -> PResult<SimplifiedExpr> {
        let start = self.start();
        let mut left = self.simplified_factor_low()?;
        let mut chain = Vec::new();
        while self.starts_simplified_factor() {
            chain.push(self.nest()?);
            let l = self.spanned(start, left);
            let s = self.start();
            let r = self.simplified_factor_low()?;
            let r = self.spanned(s, r);
            left = SimplifiedExpr::Concat(Box::new(l), Box::new(r));
        }
        Ok(left)
    }

    /// `simplifiedFactorLow: simplifiedFactorHigh | simplifiedFactorLow
    /// AMPERSAND simplifiedFactorHigh` (left-associative).
    fn simplified_factor_low(&mut self) -> PResult<SimplifiedExpr> {
        let start = self.start();
        let mut left = self.simplified_factor_high()?;
        let mut chain = Vec::new();
        while self.eat(T::Ampersand) {
            chain.push(self.nest()?);
            let l = self.spanned(start, left);
            let s = self.start();
            let r = self.simplified_factor_high()?;
            let r = self.spanned(s, r);
            left = SimplifiedExpr::Conjunction(Box::new(l), Box::new(r));
        }
        Ok(left)
    }

    /// `simplifiedFactorHigh: simplifiedTertiary | simplifiedQuantified |
    /// simplifiedQuestioned`.
    fn simplified_factor_high(&mut self) -> PResult<SimplifiedExpr> {
        let start = self.start();
        let tertiary = self.simplified_tertiary()?;
        let tertiary = self.spanned(start, tertiary);
        if self.eat(T::QuestionMark) {
            return Ok(SimplifiedExpr::Questioned(Box::new(tertiary)));
        }
        if let Some(q) = self.graph_pattern_quantifier()? {
            return Ok(SimplifiedExpr::Quantified {
                expr: Box::new(tertiary),
                quantifier: q,
            });
        }
        Ok(tertiary.0)
    }

    /// `simplifiedTertiary: simplifiedDirectionOverride |
    /// simplifiedSecondary`, with the seven `simplifiedOverride…` rules:
    /// a prefix (`<`, `~`, `<~`, `-`) and an optional `>` after the
    /// secondary.
    fn simplified_tertiary(&mut self) -> PResult<SimplifiedExpr> {
        use EdgeDirection as D;
        let prefix = match self.peek() {
            T::LeftAngleBracket => Some(1),
            T::Tilde => Some(2),
            T::LeftArrowTilde => Some(3),
            T::MinusSign => Some(4),
            _ => None,
        };
        if prefix.is_some() {
            self.bump();
        }
        let start = self.start();
        let secondary = self.simplified_secondary()?;
        let secondary = Box::new(self.spanned(start, secondary));
        let right = prefix.is_none_or(|p| p <= 2) && self.eat(T::RightAngleBracket);
        let direction = match (prefix, right) {
            (None, false) => return Ok(secondary.0),
            (None, true) => D::Right,
            (Some(1), false) => D::Left,
            (Some(1), true) => D::LeftOrRight,
            (Some(2), false) => D::Undirected,
            (Some(2), true) => D::UndirectedOrRight,
            (Some(3), _) => D::LeftOrUndirected,
            _ => D::AnyDirection,
        };
        Ok(SimplifiedExpr::DirectionOverride {
            direction,
            expr: secondary,
        })
    }

    /// `simplifiedSecondary: simplifiedPrimary | simplifiedNegation`;
    /// `simplifiedNegation: ! simplifiedPrimary`.
    fn simplified_secondary(&mut self) -> PResult<SimplifiedExpr> {
        if self.eat(T::ExclamationMark) {
            let start = self.start();
            let p = self.simplified_primary()?;
            return Ok(SimplifiedExpr::Negation(Box::new(self.spanned(start, p))));
        }
        self.simplified_primary()
    }

    /// `simplifiedPrimary: labelName | ( simplifiedContents )`.
    fn simplified_primary(&mut self) -> PResult<SimplifiedExpr> {
        if self.eat(T::LeftParen) {
            let start = self.start();
            let c = self.simplified_contents()?;
            let c = self.spanned(start, c);
            self.expect(T::RightParen)?;
            return Ok(SimplifiedExpr::Parenthesized(Box::new(c)));
        }
        self.at(T::LeftParen);
        self.at(T::ExclamationMark);
        Ok(SimplifiedExpr::Label(self.identifier()?.0))
    }
}
