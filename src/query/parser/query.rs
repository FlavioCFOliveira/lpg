//! Statements: queries, data-modifying and catalog-modifying statements
//! (`GQL.g4` 9.2, 12 to 16).
//!
//! **`statement`.** Its three alternatives (`compositeQueryStatement`,
//! `linearCatalogModifyingStatement`, `linearDataModifyingStatement`)
//! are sequences of the same building blocks, which differ only in
//! which blocks they admit and in what order: `MATCH (n) RETURN n` is
//! both a query and a data-modifying statement, `CALL p()` both a
//! catalog- and a data-modifying one. [`Parser::statement`] therefore
//! parses the blocks once ([`Item`]) and then checks them against each
//! alternative in order; the first that admits them wins, which is the
//! oracle's lowest-alternative resolution (TS-19 "Ambiguities", rows 1
//! and 2).

use super::catalog::follows_use;
use super::{Expected, Fail, PResult, Parser};
use crate::query::ast::{
    AmbientLinearQueryStatement, CallProcedureStatement, CatalogModifyingStatement,
    CompositeQueryExpr, DataModifyingBody, DataModifyingStatement, FocusedLinearQueryStatement,
    FocusedLinearQueryStatementPart, ForOrdinalityOrOffset, ForStatement, GraphExpr, GroupBy,
    InlineProcedureCall, LetVariableDef, LinearDataModifyingStatement, LinearQueryStatement,
    MatchStatement, NamedProcedureCall, NonNegativeInteger, NullOrdering, OptionalOperand,
    OrderByAndPage, Ordering, Procedure, ProcedureCall, QueryConjunction, RemoveItem,
    ResultStatement, ReturnItem, ReturnItems, ReturnStatement, SelectBody, SelectFrom,
    SelectGraphMatch, SelectStatement, SetItem, SetOpKind, SetQuantifier,
    SimpleDataAccessingStatement, SimpleQueryStatement, SortSpec, Statement, UnsignedInteger,
    Yield, YieldItem,
};
use crate::query::lexer::{Keyword as K, TokenKind as T};
use crate::query::span::{Spanned, VecSpanned};

/// Whether `next` (then `after`) starts a statement.
pub(super) fn starts_statement(next: T, after: T) -> bool {
    match next {
        T::Keyword(
            K::Match
            | K::Let
            | K::For
            | K::Filter
            | K::Order
            | K::Offset
            | K::Skip
            | K::Limit
            | K::Call
            | K::Return
            | K::Finish
            | K::Select
            | K::Use
            | K::Insert
            | K::Set
            | K::Remove
            | K::Delete
            | K::Detach
            | K::Nodetach
            | K::Create
            | K::Drop,
        )
        | T::LeftBrace => true,
        T::Keyword(K::Optional) => matches!(
            after,
            T::Keyword(K::Match | K::Call) | T::LeftBrace | T::LeftParen
        ),
        _ => false,
    }
}

/// The tokens [`starts_statement`] accepts, recorded as the expected
/// tokens where a statement may start.
const STATEMENT_STARTS: [T; 23] = [
    T::Keyword(K::Match),
    T::Keyword(K::Optional),
    T::Keyword(K::Let),
    T::Keyword(K::For),
    T::Keyword(K::Filter),
    T::Keyword(K::Order),
    T::Keyword(K::Offset),
    T::Keyword(K::Skip),
    T::Keyword(K::Limit),
    T::Keyword(K::Call),
    T::Keyword(K::Return),
    T::Keyword(K::Finish),
    T::Keyword(K::Select),
    T::Keyword(K::Use),
    T::Keyword(K::Insert),
    T::Keyword(K::Set),
    T::Keyword(K::Remove),
    T::Keyword(K::Delete),
    T::Keyword(K::Detach),
    T::Keyword(K::Nodetach),
    T::Keyword(K::Create),
    T::Keyword(K::Drop),
    T::LeftBrace,
];

impl Parser<'_> {
    /// Records the tokens that start a statement as expected here.
    pub(super) fn expect_statement(&mut self) {
        for k in STATEMENT_STARTS {
            self.expect_here(Expected::Token(k));
        }
    }
}

/// A building block of a statement.
enum Item {
    /// `useGraphClause`.
    Use(Spanned<GraphExpr>),
    /// `simpleQueryStatement`, `CALL` included.
    Query(Spanned<SimpleQueryStatement>),
    /// `primitiveDataModifyingStatement`.
    Data(Spanned<DataModifyingStatement>),
    /// `primitiveCatalogModifyingStatement`.
    Catalog(Spanned<CatalogModifyingStatement>),
    /// `primitiveResultStatement`.
    Result(Spanned<ResultStatement>),
    /// `selectStatement`.
    Select(Box<Spanned<SelectStatement>>),
    /// `{ procedureBody }`.
    Nested(Spanned<Procedure>),
    /// `queryConjunction`.
    Conjunction(Spanned<QueryConjunction>),
}

impl Item {
    fn span(&self) -> crate::query::span::Span {
        match self {
            Item::Use(s) => s.1,
            Item::Query(s) => s.1,
            Item::Data(s) => s.1,
            Item::Catalog(s) => s.1,
            Item::Result(s) => s.1,
            Item::Select(s) => s.1,
            Item::Nested(s) => s.1,
            Item::Conjunction(s) => s.1,
        }
    }

    fn is_query(&self) -> bool {
        matches!(self, Item::Query(_))
    }

    fn is_call(&self) -> bool {
        matches!(self, Item::Query(Spanned(SimpleQueryStatement::Call(_), _)))
    }
}

/// Where an alternative stopped admitting the items: the index of the
/// first item it rejects (`items.len()` when it needs more).
type Mismatch = usize;

impl Parser<'_> {
    /// `statement: compositeQueryStatement |
    /// linearCatalogModifyingStatement | linearDataModifyingStatement`.
    pub(super) fn statement(&mut self) -> PResult<Spanned<Statement>> {
        let start = self.start();
        let mut items = Vec::new();
        let mut starts = Vec::new();
        // Each query conjunction nests the composite query one level
        // deeper (`build_query`): it counts against `MAX_DEPTH`.
        let mut chain = Vec::new();
        loop {
            let at = self.pos;
            match self.statement_item()? {
                Some(item) => {
                    if matches!(item, Item::Conjunction(_)) {
                        chain.push(self.nest()?);
                    }
                    starts.push(at);
                    items.push(item);
                }
                None => break,
            }
        }
        if items.is_empty() {
            self.expect_statement();
            return Err(Fail);
        }
        let span = self.span_from(start);
        let failed = match classify_query(&items) {
            Ok(()) => return Ok(Spanned(Statement::Query(build_query(items)), span)),
            Err(a) => a,
        };
        let failed = match classify_catalog(&items) {
            Ok(()) => return Ok(Spanned(Statement::Catalog(build_catalog(items)), span)),
            Err(b) => failed.max(b),
        };
        let failed = match classify_data(&items) {
            Ok(()) => return Ok(Spanned(Statement::Data(Box::new(build_data(items))), span)),
            Err(c) => failed.max(c),
        };
        // Report at the first item no alternative admits, or after the
        // last item when every alternative needs more.
        if let Some(&at) = starts.get(failed) {
            self.pos = at;
        }
        self.expect_here(Expected::Rule(
            "a statement that can follow the previous one",
        ));
        if failed >= items.len() {
            self.expect_here(Expected::Token(T::Keyword(K::Return)));
            self.expect_here(Expected::Token(T::Keyword(K::Finish)));
        }
        Err(Fail)
    }

    /// One building block of a statement, or `None` at a token that
    /// starts none.
    fn statement_item(&mut self) -> PResult<Option<Item>> {
        let start = self.start();
        let item = match (self.peek(), self.peek_nth(1)) {
            (T::Keyword(K::Use), _) => {
                self.bump();
                Item::Use(self.graph_expression(&follows_use)?)
            }
            (
                T::Keyword(K::Insert | K::Set | K::Remove | K::Delete | K::Detach | K::Nodetach),
                _,
            ) => {
                let d = self.primitive_data_modifying_statement()?;
                Item::Data(self.spanned(start, d))
            }
            (T::Keyword(K::Create | K::Drop), _) => {
                let c = self.primitive_catalog_modifying_statement()?;
                Item::Catalog(self.spanned(start, c))
            }
            (T::Keyword(K::Return | K::Finish), _) => {
                let r = self.primitive_result_statement()?;
                Item::Result(self.spanned(start, r))
            }
            (T::Keyword(K::Select), _) => {
                let s = self.select_statement()?;
                Item::Select(Box::new(self.spanned(start, s)))
            }
            (T::LeftBrace, _) => Item::Nested(self.nested_procedure_specification()?),
            (T::Keyword(K::Union | K::Except | K::Intersect | K::Otherwise), _) => {
                let c = self.query_conjunction()?;
                Item::Conjunction(self.spanned(start, c))
            }
            (k, a) if starts_statement(k, a) => {
                let q = self.simple_query_statement()?;
                Item::Query(self.spanned(start, q))
            }
            _ => {
                self.expect_statement();
                return Ok(None);
            }
        };
        Ok(Some(item))
    }

    /// `queryConjunction: setOperator | OTHERWISE`; `setOperator: (UNION |
    /// EXCEPT | INTERSECT) setQuantifier?`.
    fn query_conjunction(&mut self) -> PResult<QueryConjunction> {
        if self.eat_kw(K::Otherwise) {
            return Ok(QueryConjunction::Otherwise);
        }
        let kind = match self.bump().kind {
            T::Keyword(K::Union) => SetOpKind::Union,
            T::Keyword(K::Except) => SetOpKind::Except,
            _ => SetOpKind::Intersect,
        };
        Ok(QueryConjunction::SetOp {
            kind,
            quantifier: self.set_quantifier(),
        })
    }

    /// `setQuantifier: DISTINCT | ALL`, optional.
    pub(super) fn set_quantifier(&mut self) -> Option<SetQuantifier> {
        if self.eat_kw(K::Distinct) {
            Some(SetQuantifier::Distinct)
        } else if self.eat_kw(K::All) {
            Some(SetQuantifier::All)
        } else {
            None
        }
    }

    /// `simpleQueryStatement: primitiveQueryStatement |
    /// callQueryStatement`; `primitiveQueryStatement: matchStatement |
    /// letStatement | forStatement | filterStatement |
    /// orderByAndPageStatement`.
    fn simple_query_statement(&mut self) -> PResult<SimpleQueryStatement> {
        Ok(match (self.peek(), self.peek_nth(1)) {
            (T::Keyword(K::Call), _) | (T::Keyword(K::Optional), T::Keyword(K::Call)) => {
                SimpleQueryStatement::Call(self.call_procedure_statement()?)
            }
            (T::Keyword(K::Match | K::Optional), _) => {
                SimpleQueryStatement::Match(self.match_statement()?)
            }
            (T::Keyword(K::Let), _) => SimpleQueryStatement::Let(self.let_statement()?),
            (T::Keyword(K::For), _) => SimpleQueryStatement::For(self.for_statement()?),
            (T::Keyword(K::Filter), _) => SimpleQueryStatement::Filter(self.filter_statement()?),
            _ => SimpleQueryStatement::OrderByAndPage(self.order_by_and_page_statement()?),
        })
    }

    /// `matchStatement: simpleMatchStatement | optionalMatchStatement`.
    pub(super) fn match_statement(&mut self) -> PResult<MatchStatement> {
        if self.at_kw(K::Optional) {
            let start = self.start();
            let o = self.optional_match_statement()?;
            return Ok(MatchStatement::Optional(self.spanned(start, o)));
        }
        Ok(MatchStatement::Simple(Box::new(
            self.simple_match_statement()?,
        )))
    }

    /// `simpleMatchStatement: MATCH graphPatternBindingTable`.
    fn simple_match_statement(
        &mut self,
    ) -> PResult<Spanned<crate::query::ast::GraphPatternBindingTable>> {
        self.expect_kw(K::Match)?;
        let start = self.start();
        let t = self.graph_pattern_binding_table()?;
        Ok(self.spanned(start, t))
    }

    /// `optionalMatchStatement: OPTIONAL optionalOperand`;
    /// `optionalOperand: simpleMatchStatement | { matchStatementBlock } |
    /// ( matchStatementBlock )`.
    fn optional_match_statement(&mut self) -> PResult<OptionalOperand> {
        self.expect_kw(K::Optional)?;
        let close = if self.eat(T::LeftBrace) {
            T::RightBrace
        } else if self.eat(T::LeftParen) {
            T::RightParen
        } else {
            return Ok(OptionalOperand::Match(Box::new(
                self.simple_match_statement()?,
            )));
        };
        let block = self.match_statement_block()?;
        self.expect(close)?;
        Ok(OptionalOperand::Block(block))
    }

    /// `matchStatementBlock: matchStatement+`.
    pub(super) fn match_statement_block(&mut self) -> PResult<VecSpanned<MatchStatement>> {
        let mut block = Vec::new();
        loop {
            let start = self.start();
            let m = self.match_statement()?;
            block.push(self.spanned(start, m));
            let more = self.at_kw(K::Match)
                || (self.at_kw(K::Optional)
                    && matches!(
                        self.peek_nth(1),
                        T::Keyword(K::Match) | T::LeftBrace | T::LeftParen
                    ));
            if !more {
                return Ok(block);
            }
        }
    }

    /// `letStatement: LET letVariableDefinitionList`.
    fn let_statement(&mut self) -> PResult<VecSpanned<LetVariableDef>> {
        self.expect_kw(K::Let)?;
        self.let_variable_definition_list()
    }

    /// `letVariableDefinitionList: letVariableDefinition (COMMA
    /// letVariableDefinition)*`.
    pub(super) fn let_variable_definition_list(&mut self) -> PResult<VecSpanned<LetVariableDef>> {
        let mut defs = Vec::new();
        loop {
            let start = self.start();
            let d = self.let_variable_definition()?;
            defs.push(self.spanned(start, d));
            if !self.eat(T::Comma) {
                return Ok(defs);
            }
        }
    }

    /// `letVariableDefinition: valueVariableDefinition | bindingVariable =
    /// valueExpression`.
    fn let_variable_definition(&mut self) -> PResult<LetVariableDef> {
        if self.at_kw(K::Value) {
            return Ok(LetVariableDef::Value(self.value_variable_definition()?));
        }
        let variable = self.regular_identifier()?;
        self.expect(T::Equals)?;
        let value = self.value_expression()?;
        Ok(LetVariableDef::Assign { variable, value })
    }

    /// `forStatement: FOR forItem forOrdinalityOrOffset?`; `forItem:
    /// forItemAlias forItemSource`; `forItemAlias: bindingVariable IN`.
    fn for_statement(&mut self) -> PResult<ForStatement> {
        self.expect_kw(K::For)?;
        let alias = self.regular_identifier()?;
        self.expect_kw(K::In)?;
        let source = self.value_expression()?;
        let ordinality_or_offset = if self.at_kw(K::With) {
            // forOrdinalityOrOffset: WITH (ORDINALITY | OFFSET) bindingVariable.
            let start = self.start();
            self.bump();
            let ordinality = if self.eat_kw(K::Ordinality) {
                true
            } else {
                self.expect_kw(K::Offset)?;
                false
            };
            let variable = self.regular_identifier()?;
            Some(self.spanned(
                start,
                ForOrdinalityOrOffset {
                    ordinality,
                    variable,
                },
            ))
        } else {
            None
        };
        Ok(ForStatement {
            alias,
            source,
            ordinality_or_offset,
        })
    }

    /// `filterStatement: FILTER (whereClause | searchCondition)`.
    fn filter_statement(&mut self) -> PResult<Spanned<crate::query::ast::Expr>> {
        self.expect_kw(K::Filter)?;
        self.eat_kw(K::Where);
        self.value_expression()
    }

    /// `orderByAndPageStatement: orderByClause offsetClause? limitClause?
    /// | offsetClause limitClause? | limitClause`. Greedy: an `OFFSET` or
    /// `LIMIT` after an `ORDER BY` belongs to it (the `?` decisions take
    /// their first alternative).
    pub(super) fn order_by_and_page_statement(&mut self) -> PResult<OrderByAndPage> {
        let mut order_by = Vec::new();
        let mut any = false;
        if self.at_kw(K::Order) {
            order_by = self.order_by_clause()?;
            any = true;
        }
        let offset = if self.at_kw(K::Offset) || self.at_kw(K::Skip) {
            any = true;
            Some(self.offset_clause()?)
        } else {
            None
        };
        let limit = if self.at_kw(K::Limit) {
            any = true;
            Some(self.limit_clause()?)
        } else {
            None
        };
        if !any {
            return Err(Fail);
        }
        Ok(OrderByAndPage {
            order_by,
            offset,
            limit,
        })
    }

    /// `orderByClause: ORDER BY sortSpecificationList`.
    fn order_by_clause(&mut self) -> PResult<VecSpanned<SortSpec>> {
        self.expect_kw(K::Order)?;
        self.expect_kw(K::By)?;
        self.sort_specification_list()
    }

    /// `sortSpecificationList: sortSpecification (COMMA
    /// sortSpecification)*`.
    fn sort_specification_list(&mut self) -> PResult<VecSpanned<SortSpec>> {
        let mut specs = Vec::new();
        loop {
            let start = self.start();
            let s = self.sort_specification()?;
            specs.push(self.spanned(start, s));
            if !self.eat(T::Comma) {
                return Ok(specs);
            }
        }
    }

    /// `sortSpecification: sortKey orderingSpecification? nullOrdering?`.
    fn sort_specification(&mut self) -> PResult<SortSpec> {
        let key = self.value_expression()?;
        let start = self.start();
        let ordering = match self.peek() {
            T::Keyword(K::Asc | K::Ascending) => Some(Ordering::Asc),
            T::Keyword(K::Desc | K::Descending) => Some(Ordering::Desc),
            _ => None,
        };
        let ordering = ordering.map(|o| {
            self.bump();
            self.spanned(start, o)
        });
        let null_ordering = if self.at_kw(K::Nulls) {
            // nullOrdering: NULLS FIRST | NULLS LAST.
            let start = self.start();
            self.bump();
            let n = if self.eat_kw(K::First) {
                NullOrdering::First
            } else {
                self.expect_kw(K::Last)?;
                NullOrdering::Last
            };
            Some(self.spanned(start, n))
        } else {
            None
        };
        Ok(SortSpec {
            key,
            ordering,
            null_ordering,
        })
    }

    /// `offsetClause: offsetSynonym nonNegativeIntegerSpecification`.
    fn offset_clause(&mut self) -> PResult<Spanned<NonNegativeInteger>> {
        if !self.eat_kw(K::Skip) {
            self.expect_kw(K::Offset)?;
        }
        self.non_negative_integer_specification()
    }

    /// `limitClause: LIMIT nonNegativeIntegerSpecification`.
    fn limit_clause(&mut self) -> PResult<Spanned<NonNegativeInteger>> {
        self.expect_kw(K::Limit)?;
        self.non_negative_integer_specification()
    }

    /// `nonNegativeIntegerSpecification: unsignedInteger |
    /// dynamicParameterSpecification`.
    pub(super) fn non_negative_integer_specification(
        &mut self,
    ) -> PResult<Spanned<NonNegativeInteger>> {
        if self.at(T::GeneralParameter) {
            let p = self.general_parameter()?;
            return Ok(Spanned(NonNegativeInteger::Parameter(p.0), p.1));
        }
        let i = self.unsigned_integer()?;
        Ok(Spanned(NonNegativeInteger::Integer(i.0), i.1))
    }

    /// `unsignedInteger: UNSIGNED_DECIMAL_INTEGER |
    /// UNSIGNED_HEXADECIMAL_INTEGER | UNSIGNED_OCTAL_INTEGER |
    /// UNSIGNED_BINARY_INTEGER`.
    pub(super) fn unsigned_integer(&mut self) -> PResult<Spanned<UnsignedInteger>> {
        let radix = match self.peek() {
            T::Integer => 10,
            T::Hexadecimal => 16,
            T::Octal => 8,
            T::Binary => 2,
            _ => return self.fail(Expected::Token(T::Integer)),
        };
        let t = self.bump();
        let text = t.text(self.text);
        let digits = if radix == 10 { text } else { &text[2..] };
        Ok(Spanned(
            UnsignedInteger {
                radix,
                digits: digits.into(),
            },
            t.span,
        ))
    }

    /// `unsignedDecimalInteger: UNSIGNED_DECIMAL_INTEGER`.
    pub(super) fn unsigned_decimal_integer(&mut self) -> PResult<Spanned<UnsignedInteger>> {
        if !self.at(T::Integer) {
            return Err(Fail);
        }
        self.unsigned_integer()
    }

    /// `primitiveResultStatement: returnStatement
    /// orderByAndPageStatement? | FINISH`.
    fn primitive_result_statement(&mut self) -> PResult<ResultStatement> {
        if self.eat_kw(K::Finish) {
            return Ok(ResultStatement::Finish);
        }
        let start = self.start();
        let r = self.return_statement()?;
        let statement = Box::new(self.spanned(start, r));
        let order_by = if self.at_kw(K::Order)
            || self.at_kw(K::Offset)
            || self.at_kw(K::Skip)
            || self.at_kw(K::Limit)
        {
            let start = self.start();
            let o = self.order_by_and_page_statement()?;
            Some(Box::new(self.spanned(start, o)))
        } else {
            None
        };
        Ok(ResultStatement::Return {
            statement,
            order_by,
        })
    }

    /// `returnStatement: RETURN returnStatementBody`;
    /// `returnStatementBody: setQuantifier? (ASTERISK | returnItemList)
    /// groupByClause?`.
    fn return_statement(&mut self) -> PResult<ReturnStatement> {
        self.expect_kw(K::Return)?;
        let start = self.start();
        let quantifier = self.set_quantifier().map(|q| self.spanned(start, q));
        let items = self.return_items()?;
        let group_by = if self.at_kw(K::Group) {
            Some(self.group_by_clause()?)
        } else {
            None
        };
        Ok(ReturnStatement {
            quantifier,
            items,
            group_by,
        })
    }

    /// `ASTERISK | returnItemList` (and `selectItemList`): `returnItem
    /// (COMMA returnItem)*`; `returnItem: aggregatingValueExpression
    /// returnItemAlias?`; `returnItemAlias: AS identifier`.
    fn return_items(&mut self) -> PResult<Spanned<ReturnItems>> {
        let start = self.start();
        if self.eat(T::Asterisk) {
            return Ok(self.spanned(start, ReturnItems::All));
        }
        let mut items = Vec::new();
        loop {
            let item_start = self.start();
            let value = self.value_expression()?;
            let alias = if self.eat_kw(K::As) {
                Some(self.identifier()?)
            } else {
                None
            };
            items.push(self.spanned(item_start, ReturnItem { value, alias }));
            if !self.eat(T::Comma) {
                return Ok(self.spanned(start, ReturnItems::Items(items)));
            }
        }
    }

    /// `groupByClause: GROUP BY groupingElementList`;
    /// `groupingElementList: groupingElement (COMMA groupingElement)* |
    /// emptyGroupingSet`.
    fn group_by_clause(&mut self) -> PResult<Spanned<GroupBy>> {
        let start = self.start();
        self.expect_kw(K::Group)?;
        self.expect_kw(K::By)?;
        let mut elements = Vec::new();
        if self.at(T::LeftParen) && self.peek_nth(1) == T::RightParen {
            self.pos += 2;
        } else {
            loop {
                elements.push(self.regular_identifier()?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
        }
        Ok(self.spanned(start, elements))
    }

    /// `selectStatement: SELECT setQuantifier? (ASTERISK |
    /// selectItemList) (selectStatementBody whereClause? groupByClause?
    /// havingClause? orderByClause? offsetClause? limitClause?)?`.
    fn select_statement(&mut self) -> PResult<SelectStatement> {
        self.expect_kw(K::Select)?;
        let start = self.start();
        let quantifier = self.set_quantifier().map(|q| self.spanned(start, q));
        let items = self.return_items()?;
        if !self.at_kw(K::From) {
            return Ok(SelectStatement {
                quantifier,
                items,
                body: None,
            });
        }
        let from = self.select_statement_body()?;
        let where_clause = if self.eat_kw(K::Where) {
            Some(self.value_expression()?)
        } else {
            None
        };
        let group_by = if self.at_kw(K::Group) {
            Some(self.group_by_clause()?)
        } else {
            None
        };
        let having = if self.eat_kw(K::Having) {
            Some(self.value_expression()?)
        } else {
            None
        };
        let order_by = if self.at_kw(K::Order) {
            Some(self.order_by_clause()?)
        } else {
            None
        };
        let offset = if self.at_kw(K::Offset) || self.at_kw(K::Skip) {
            Some(self.offset_clause()?)
        } else {
            None
        };
        let limit = if self.at_kw(K::Limit) {
            Some(self.limit_clause()?)
        } else {
            None
        };
        Ok(SelectStatement {
            quantifier,
            items,
            body: Some(SelectBody {
                from,
                where_clause,
                group_by,
                having,
                order_by,
                offset,
                limit,
            }),
        })
    }

    /// `selectStatementBody: FROM (selectGraphMatchList |
    /// selectQuerySpecification)`; `selectGraphMatchList:
    /// selectGraphMatch (COMMA selectGraphMatch)*`; `selectGraphMatch:
    /// graphExpression matchStatement`; `selectQuerySpecification:
    /// nestedQuerySpecification | graphExpression
    /// nestedQuerySpecification`.
    fn select_statement_body(&mut self) -> PResult<Spanned<SelectFrom>> {
        let start = self.start();
        self.expect_kw(K::From)?;
        let body_start = self.start();
        if self.at(T::LeftBrace) && !self.at_record_literal() {
            let q = self.nested_procedure_specification()?;
            return Ok(self.spanned(
                body_start,
                SelectFrom::Query {
                    graph: None,
                    query: Box::new(q),
                },
            ));
        }
        let mut matches = Vec::new();
        loop {
            let m_start = self.start();
            let graph = self.graph_expression(&super::catalog::follows_select_graph)?;
            if matches.is_empty() && self.at(T::LeftBrace) {
                let q = self.nested_procedure_specification()?;
                return Ok(self.spanned(
                    start,
                    SelectFrom::Query {
                        graph: Some(graph),
                        query: Box::new(q),
                    },
                ));
            }
            let s_start = self.start();
            let m = self.match_statement()?;
            let statement = self.spanned(s_start, m);
            matches.push(self.spanned(m_start, SelectGraphMatch { graph, statement }));
            if !self.eat(T::Comma) {
                return Ok(self.spanned(body_start, SelectFrom::GraphMatches(matches)));
            }
        }
    }

    /// `callProcedureStatement: OPTIONAL? CALL procedureCall`.
    pub(super) fn call_procedure_statement(&mut self) -> PResult<CallProcedureStatement> {
        let optional = self.eat_kw(K::Optional);
        self.expect_kw(K::Call)?;
        let start = self.start();
        let call = self.procedure_call()?;
        Ok(CallProcedureStatement {
            optional,
            procedure: self.spanned(start, call),
        })
    }

    /// `procedureCall: inlineProcedureCall | namedProcedureCall`.
    fn procedure_call(&mut self) -> PResult<ProcedureCall> {
        if self.at(T::LeftParen) || self.at(T::LeftBrace) {
            return Ok(ProcedureCall::Inline(self.inline_procedure_call()?));
        }
        Ok(ProcedureCall::Named(Box::new(self.named_procedure_call()?)))
    }

    /// `inlineProcedureCall: variableScopeClause?
    /// nestedProcedureSpecification`; `variableScopeClause: (
    /// bindingVariableReferenceList? )`.
    fn inline_procedure_call(&mut self) -> PResult<InlineProcedureCall> {
        let scope = if self.eat(T::LeftParen) {
            let mut vars = Vec::new();
            if !self.eat(T::RightParen) {
                loop {
                    vars.push(self.regular_identifier()?);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.expect(T::RightParen)?;
            }
            Some(vars)
        } else {
            None
        };
        let procedure = Box::new(self.nested_procedure_specification()?);
        Ok(InlineProcedureCall { scope, procedure })
    }

    /// `namedProcedureCall: procedureReference LEFT_PAREN
    /// procedureArgumentList? RIGHT_PAREN yieldClause?`.
    fn named_procedure_call(&mut self) -> PResult<NamedProcedureCall> {
        let name = self.procedure_reference()?;
        self.expect(T::LeftParen)?;
        let mut args = Vec::new();
        if !self.eat(T::RightParen) {
            loop {
                args.push(self.value_expression()?);
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.expect(T::RightParen)?;
        }
        let yield_clause = if self.at_kw(K::Yield) {
            let start = self.start();
            let y = self.yield_clause()?;
            Some(self.spanned(start, y))
        } else {
            None
        };
        Ok(NamedProcedureCall {
            name,
            args,
            yield_clause,
        })
    }

    /// `yieldClause: YIELD yieldItemList`; `yieldItem: yieldItemName
    /// yieldItemAlias?`; `yieldItemAlias: AS bindingVariable`.
    pub(super) fn yield_clause(&mut self) -> PResult<Yield> {
        self.expect_kw(K::Yield)?;
        let mut items = Vec::new();
        loop {
            let start = self.start();
            let name = self.identifier()?;
            let alias = if self.eat_kw(K::As) {
                Some(self.regular_identifier()?)
            } else {
                None
            };
            items.push(self.spanned(start, YieldItem { name, alias }));
            if !self.eat(T::Comma) {
                return Ok(items);
            }
        }
    }

    /// `primitiveDataModifyingStatement: insertStatement | setStatement |
    /// removeStatement | deleteStatement`.
    fn primitive_data_modifying_statement(&mut self) -> PResult<DataModifyingStatement> {
        match self.peek() {
            T::Keyword(K::Insert) => {
                // insertStatement: INSERT insertGraphPattern.
                self.bump();
                Ok(DataModifyingStatement::Insert(self.insert_graph_pattern()?))
            }
            T::Keyword(K::Set) => self.set_statement(),
            T::Keyword(K::Remove) => self.remove_statement(),
            _ => self.delete_statement(),
        }
    }

    /// `setStatement: SET setItemList`; `setItem: setPropertyItem |
    /// setAllPropertiesItem | setLabelItem`.
    fn set_statement(&mut self) -> PResult<DataModifyingStatement> {
        self.expect_kw(K::Set)?;
        let mut items = Vec::new();
        loop {
            let start = self.start();
            let variable = self.regular_identifier()?;
            let item = if self.eat(T::Period) {
                // setPropertyItem: v . propertyName = valueExpression.
                let property = self.identifier()?;
                self.expect(T::Equals)?;
                let value = self.value_expression()?;
                SetItem::Property {
                    variable,
                    property,
                    value,
                }
            } else if self.eat(T::Equals) {
                // setAllPropertiesItem: v = { propertyKeyValuePairList? }.
                self.expect(T::LeftBrace)?;
                let properties = if self.at(T::RightBrace) {
                    Vec::new()
                } else {
                    self.property_key_value_pair_list()?
                };
                self.expect(T::RightBrace)?;
                SetItem::AllProperties {
                    variable,
                    properties,
                }
            } else {
                // setLabelItem: v isOrColon labelName.
                self.is_or_colon()?;
                let label = self.identifier()?;
                SetItem::Label { variable, label }
            };
            items.push(self.spanned(start, item));
            if !self.eat(T::Comma) {
                return Ok(DataModifyingStatement::Set(items));
            }
        }
    }

    /// `isOrColon: IS | COLON`.
    pub(super) fn is_or_colon(&mut self) -> PResult<()> {
        if self.eat_kw(K::Is) || self.eat(T::Colon) {
            Ok(())
        } else {
            self.fail(Expected::Token(T::Colon))
        }
    }

    /// `removeStatement: REMOVE removeItemList`; `removeItem:
    /// removePropertyItem | removeLabelItem`.
    fn remove_statement(&mut self) -> PResult<DataModifyingStatement> {
        self.expect_kw(K::Remove)?;
        let mut items = Vec::new();
        loop {
            let start = self.start();
            let variable = self.regular_identifier()?;
            let item = if self.eat(T::Period) {
                RemoveItem::Property {
                    variable,
                    property: self.identifier()?,
                }
            } else {
                self.is_or_colon()?;
                RemoveItem::Label {
                    variable,
                    label: self.identifier()?,
                }
            };
            items.push(self.spanned(start, item));
            if !self.eat(T::Comma) {
                return Ok(DataModifyingStatement::Remove(items));
            }
        }
    }

    /// `deleteStatement: (DETACH | NODETACH)? DELETE deleteItemList`.
    fn delete_statement(&mut self) -> PResult<DataModifyingStatement> {
        let detach = if self.eat_kw(K::Detach) {
            Some(true)
        } else if self.eat_kw(K::Nodetach) {
            Some(false)
        } else {
            None
        };
        self.expect_kw(K::Delete)?;
        let mut items = Vec::new();
        loop {
            items.push(self.value_expression()?);
            if !self.eat(T::Comma) {
                return Ok(DataModifyingStatement::Delete { detach, items });
            }
        }
    }
}

// ---- Classification of the statement items --------------------------

/// `compositeQueryStatement`: linear queries joined by conjunctions.
fn classify_query(items: &[Item]) -> Result<(), Mismatch> {
    let mut start = 0;
    for (i, item) in items.iter().enumerate() {
        if matches!(item, Item::Conjunction(_)) {
            linear_query_shape(&items[start..i]).map_err(|k| start + k)?;
            start = i + 1;
        }
    }
    linear_query_shape(&items[start..]).map_err(|k| start + k)
}

/// `linearQueryStatement`: where `part` stops being one, or `Ok`.
fn linear_query_shape(part: &[Item]) -> Result<(), Mismatch> {
    match part {
        [] => Err(0),
        [Item::Select(_)] | [Item::Nested(_)] => Ok(()),
        [Item::Use(_), Item::Result(_) | Item::Nested(_)] => Ok(()),
        [Item::Use(_), ..] => {
            // (USE simpleQueryStatement+)+ primitiveResultStatement.
            let mut i = 0;
            loop {
                if !matches!(part.get(i), Some(Item::Use(_))) {
                    return Err(i);
                }
                i += 1;
                let first = i;
                while part.get(i).is_some_and(Item::is_query) {
                    i += 1;
                }
                if i == first {
                    return Err(i);
                }
                match part.get(i) {
                    Some(Item::Result(_)) if i + 1 == part.len() => return Ok(()),
                    Some(Item::Use(_)) => {}
                    _ => return Err(i),
                }
            }
        }
        _ => {
            // simpleLinearQueryStatement? primitiveResultStatement.
            let mut i = 0;
            while part.get(i).is_some_and(Item::is_query) {
                i += 1;
            }
            match part.get(i) {
                Some(Item::Result(_)) if i + 1 == part.len() => Ok(()),
                _ => Err(i),
            }
        }
    }
}

/// `linearCatalogModifyingStatement: simpleCatalogModifyingStatement+`.
fn classify_catalog(items: &[Item]) -> Result<(), Mismatch> {
    match items
        .iter()
        .position(|i| !(matches!(i, Item::Catalog(_)) || i.is_call()))
    {
        Some(k) => Err(k),
        None => Ok(()),
    }
}

/// `linearDataModifyingStatement`: `useGraphClause?` then a nested
/// procedure, or `(simpleQueryStatement | primitiveDataModifyingStatement)+
/// primitiveResultStatement?`.
fn classify_data(items: &[Item]) -> Result<(), Mismatch> {
    let skip = usize::from(matches!(items.first(), Some(Item::Use(_))));
    let body = &items[skip..];
    match body {
        [Item::Nested(_)] => Ok(()),
        _ => {
            let mut i = 0;
            while matches!(body.get(i), Some(Item::Query(_) | Item::Data(_))) {
                i += 1;
            }
            if i == 0 {
                return Err(skip);
            }
            match body.get(i) {
                None => Ok(()),
                Some(Item::Result(_)) if i + 1 == body.len() => Ok(()),
                _ => Err(skip + i),
            }
        }
    }
}

fn build_query(items: Vec<Item>) -> CompositeQueryExpr {
    let mut parts: Vec<Vec<Item>> = vec![Vec::new()];
    let mut conjunctions = Vec::new();
    for item in items {
        match item {
            Item::Conjunction(c) => {
                conjunctions.push(c);
                parts.push(Vec::new());
            }
            other => parts.last_mut().map_or((), |p| p.push(other)),
        }
    }
    let mut parts = parts.into_iter().map(build_linear_query);
    let first = parts.next().unwrap_or_else(|| {
        Spanned(
            LinearQueryStatement::Ambient(AmbientLinearQueryStatement::Parts {
                statements: Vec::new(),
                result: Spanned(ResultStatement::Finish, Default::default()),
            }),
            Default::default(),
        )
    });
    let mut expr = CompositeQueryExpr::Primary(Box::new(first.0));
    let mut span = first.1;
    for (conjunction, right) in conjunctions.into_iter().zip(parts) {
        let left_span = span;
        span = left_span.union(&right.1).union(&conjunction.1);
        expr = CompositeQueryExpr::Conjunction {
            conjunction,
            left: Box::new(Spanned(expr, left_span)),
            right: Box::new(right),
        };
    }
    expr
}

fn build_linear_query(part: Vec<Item>) -> Spanned<LinearQueryStatement> {
    let span = part
        .iter()
        .map(Item::span)
        .reduce(|a, b| a.union(&b))
        .unwrap_or_default();
    let mut it = part.into_iter().peekable();
    let q = match it.next() {
        Some(Item::Select(s)) => {
            LinearQueryStatement::Focused(FocusedLinearQueryStatement::Select(Box::new(s.0)))
        }
        Some(Item::Nested(n)) => {
            LinearQueryStatement::Ambient(AmbientLinearQueryStatement::Nested(Box::new(n)))
        }
        Some(Item::Use(g)) => match it.next() {
            Some(Item::Result(r)) => {
                LinearQueryStatement::Focused(FocusedLinearQueryStatement::Result {
                    use_graph: g,
                    result: r,
                })
            }
            Some(Item::Nested(n)) => {
                LinearQueryStatement::Focused(FocusedLinearQueryStatement::Nested {
                    use_graph: g,
                    query: Box::new(n),
                })
            }
            first => {
                let mut parts = Vec::new();
                let mut use_graph = g;
                let mut statements = Vec::new();
                let mut next = first;
                let result = loop {
                    match next {
                        Some(Item::Query(q)) => statements.push(q),
                        Some(Item::Use(g2)) => {
                            let span = statements
                                .iter()
                                .fold(use_graph.1, |s, q: &Spanned<_>| s.union(&q.1));
                            parts.push(Spanned(
                                FocusedLinearQueryStatementPart {
                                    use_graph,
                                    statements: std::mem::take(&mut statements),
                                },
                                span,
                            ));
                            use_graph = g2;
                        }
                        Some(Item::Result(r)) => break r,
                        _ => break Spanned(ResultStatement::Finish, Default::default()),
                    }
                    next = it.next();
                };
                let span = statements
                    .iter()
                    .fold(use_graph.1, |s, q: &Spanned<_>| s.union(&q.1));
                parts.push(Spanned(
                    FocusedLinearQueryStatementPart {
                        use_graph,
                        statements,
                    },
                    span,
                ));
                LinearQueryStatement::Focused(FocusedLinearQueryStatement::Parts { parts, result })
            }
        },
        first => {
            let mut statements = Vec::new();
            let mut next = first;
            let result = loop {
                match next {
                    Some(Item::Query(q)) => statements.push(q),
                    Some(Item::Result(r)) => break r,
                    _ => break Spanned(ResultStatement::Finish, Default::default()),
                }
                next = it.next();
            };
            LinearQueryStatement::Ambient(AmbientLinearQueryStatement::Parts { statements, result })
        }
    };
    Spanned(q, span)
}

fn build_catalog(items: Vec<Item>) -> VecSpanned<CatalogModifyingStatement> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Catalog(c) => Some(c),
            Item::Query(Spanned(SimpleQueryStatement::Call(c), span)) => {
                Some(Spanned(CatalogModifyingStatement::Call(c), span))
            }
            _ => None,
        })
        .collect()
}

fn build_data(items: Vec<Item>) -> LinearDataModifyingStatement {
    let mut it = items.into_iter().peekable();
    let use_graph = match it.peek() {
        Some(Item::Use(_)) => match it.next() {
            Some(Item::Use(g)) => Some(g),
            _ => None,
        },
        _ => None,
    };
    let mut statements = Vec::new();
    let mut result = None;
    for item in it {
        match item {
            Item::Nested(n) => {
                return LinearDataModifyingStatement {
                    use_graph,
                    body: DataModifyingBody::Nested(Box::new(n)),
                };
            }
            Item::Query(Spanned(q, span)) => {
                statements.push(Spanned(
                    SimpleDataAccessingStatement::Query(Box::new(q)),
                    span,
                ));
            }
            Item::Data(Spanned(d, span)) => {
                statements.push(Spanned(SimpleDataAccessingStatement::Modifying(d), span));
            }
            Item::Result(r) => result = Some(r),
            _ => {}
        }
    }
    LinearDataModifyingStatement {
        use_graph,
        body: DataModifyingBody::Statements { statements, result },
    }
}
