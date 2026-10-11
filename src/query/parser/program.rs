//! Programs, sessions, transactions, procedure bodies and binding
//! variable definitions (`GQL.g4` 6 to 10).

use super::{PResult, Parser};
use crate::query::ast::{
    BindingTableVariableDef, BindingVariableDef, EndTransaction, GraphVariableDef, NextStatement,
    Procedure, Program, ProgramActivity, SessionActivity, SessionReset, SessionResetArgs,
    SessionSet, SessionSetParameter, SessionSetParameterKind, StartTransaction,
    TransactionActivity, TransactionMode, TypedBindingTableInitializer, TypedGraphInitializer,
    TypedValueInitializer, ValueVariableDef,
};
use crate::query::lexer::{Keyword as K, TokenKind as T};
use crate::query::span::Spanned;

impl Parser<'_> {
    /// `gqlProgram: programActivity sessionCloseCommand? EOF |
    /// sessionCloseCommand EOF`.
    pub(super) fn gql_program(&mut self) -> PResult<Program> {
        let activity = if self.at_session_close() {
            None
        } else {
            let start = self.start();
            let a = self.program_activity()?;
            Some(self.spanned(start, a))
        };
        let session_close = self.at_session_close();
        if session_close {
            self.session_close_command()?;
        }
        self.expect(T::Eof)?;
        Ok(Program {
            activity,
            session_close,
            invalid_escapes: std::mem::take(&mut self.invalid_escapes),
        })
    }

    fn at_session_close(&mut self) -> bool {
        self.at_kw(K::Session) && self.peek_nth(1) == T::Keyword(K::Close)
    }

    /// `programActivity: sessionActivity | transactionActivity`.
    fn program_activity(&mut self) -> PResult<ProgramActivity> {
        if self.at_kw(K::Session) {
            Ok(ProgramActivity::Session(self.session_activity()?))
        } else {
            Ok(ProgramActivity::Transaction(Box::new(
                self.transaction_activity()?,
            )))
        }
    }

    /// `sessionActivity: sessionResetCommand+ | sessionSetCommand+
    /// sessionResetCommand*`.
    fn session_activity(&mut self) -> PResult<SessionActivity> {
        let mut set = Vec::new();
        let mut reset = Vec::new();
        let at = |p: &mut Self, k: K| p.at_kw(K::Session) && p.peek_nth(1) == T::Keyword(k);
        if !at(self, K::Reset) {
            loop {
                let start = self.start();
                let s = self.session_set_command()?;
                set.push(self.spanned(start, s));
                if !at(self, K::Set) {
                    break;
                }
            }
        }
        while at(self, K::Reset) {
            let start = self.start();
            let r = self.session_reset_command()?;
            reset.push(self.spanned(start, r));
        }
        if set.is_empty() && reset.is_empty() {
            self.expect_kw(K::Set)?;
        }
        Ok(SessionActivity { set, reset })
    }

    /// `sessionSetCommand: SESSION SET (sessionSetSchemaClause |
    /// sessionSetGraphClause | sessionSetTimeZoneClause |
    /// sessionSetParameterClause)`.
    ///
    /// `sessionSetGraphClause` (`[PROPERTY] GRAPH graphExpression`) and a
    /// graph parameter (`[PROPERTY] GRAPH $p …`) share their start: a
    /// parameter follows `IF`, or a `$p` followed by its initializer or
    /// type (`=`, `::`, `TYPED`, `ANY`, `PROPERTY`, `GRAPH`).
    fn session_set_command(&mut self) -> PResult<SessionSet> {
        self.expect_kw(K::Session)?;
        self.expect_kw(K::Set)?;
        let start = self.start();
        if self.eat_kw(K::Schema) {
            let s = self.schema_reference(&super::catalog::follows_session)?;
            return Ok(SessionSet::Schema(s));
        }
        if self.at_kw(K::Time) {
            self.bump();
            self.expect_kw(K::Zone)?;
            let s = self.character_string_literal()?;
            return Ok(SessionSet::TimeZone(s));
        }
        let graph = match (self.peek(), self.peek_nth(1)) {
            (T::Keyword(K::Graph), _) => Some(1),
            (T::Keyword(K::Property), T::Keyword(K::Graph)) => Some(2),
            _ => None,
        };
        if let Some(n) = graph {
            let param = match self.peek_nth(n) {
                T::Keyword(K::If) => true,
                T::GeneralParameter => matches!(
                    self.peek_nth(n + 1),
                    T::Equals
                        | T::DoubleColon
                        | T::Keyword(K::Typed | K::Any | K::Property | K::Graph)
                ),
                _ => false,
            };
            if !param {
                self.pos += n;
                let g = self.graph_expression(&super::catalog::follows_session)?;
                return Ok(SessionSet::Graph(g));
            }
        }
        let p = self.session_set_parameter_clause()?;
        Ok(SessionSet::Parameter(Box::new(self.spanned(start, p))))
    }

    /// `sessionSetParameterClause: sessionSetGraphParameterClause |
    /// sessionSetBindingTableParameterClause |
    /// sessionSetValueParameterClause`.
    fn session_set_parameter_clause(&mut self) -> PResult<SessionSetParameter> {
        let start = self.start();
        if self.eat_kw(K::Value) {
            let (name, if_not_exists) = self.session_set_parameter_name()?;
            let init = self.opt_typed_value_initializer()?;
            return Ok(SessionSetParameter {
                name,
                if_not_exists,
                kind: SessionSetParameterKind::Value(init),
            });
        }
        if self.eat_kw(K::Binding) {
            self.expect_kw(K::Table)?;
        } else if self.eat_kw(K::Table) {
        } else {
            if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
                self.bump();
            }
            self.expect_kw(K::Graph)?;
            let (name, if_not_exists) = self.session_set_parameter_name()?;
            let init = self.opt_typed_graph_initializer(&super::catalog::follows_session)?;
            return Ok(SessionSetParameter {
                name,
                if_not_exists,
                kind: SessionSetParameterKind::Graph(init),
            });
        }
        let _ = start;
        let (name, if_not_exists) = self.session_set_parameter_name()?;
        let init = self.opt_typed_binding_table_initializer(&super::catalog::follows_session)?;
        Ok(SessionSetParameter {
            name,
            if_not_exists,
            kind: SessionSetParameterKind::BindingTable(init),
        })
    }

    /// `sessionSetParameterName: (IF NOT EXISTS)?
    /// sessionParameterSpecification`.
    fn session_set_parameter_name(&mut self) -> PResult<(Spanned<crate::query::ast::Ident>, bool)> {
        let if_not_exists = self.eat_kw(K::If);
        if if_not_exists {
            self.expect_kw(K::Not)?;
            self.expect_kw(K::Exists)?;
        }
        Ok((self.general_parameter()?, if_not_exists))
    }

    /// `sessionResetCommand: SESSION RESET sessionResetArguments?`.
    fn session_reset_command(&mut self) -> PResult<SessionReset> {
        self.expect_kw(K::Session)?;
        self.expect_kw(K::Reset)?;
        let start = self.start();
        let args = self.session_reset_arguments()?;
        Ok(SessionReset(args.map(|a| self.spanned(start, a))))
    }

    /// `sessionResetArguments: ALL? (PARAMETERS | CHARACTERISTICS) |
    /// SCHEMA | PROPERTY? GRAPH | TIME ZONE | PARAMETER?
    /// sessionParameterSpecification`.
    fn session_reset_arguments(&mut self) -> PResult<Option<SessionResetArgs>> {
        let all = self.eat_kw(K::All);
        if self.eat_kw(K::Parameters) {
            return Ok(Some(SessionResetArgs::Parameters));
        }
        if self.eat_kw(K::Characteristics) {
            return Ok(Some(SessionResetArgs::Characteristics));
        }
        if all {
            self.expect_kw(K::Parameters)?;
        }
        if self.eat_kw(K::Schema) {
            return Ok(Some(SessionResetArgs::Schema));
        }
        if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
            self.bump();
            self.bump();
            return Ok(Some(SessionResetArgs::Graph));
        }
        if self.eat_kw(K::Graph) {
            return Ok(Some(SessionResetArgs::Graph));
        }
        if self.eat_kw(K::Time) {
            self.expect_kw(K::Zone)?;
            return Ok(Some(SessionResetArgs::TimeZone));
        }
        if self.eat_kw(K::Parameter) {
            return Ok(Some(SessionResetArgs::Parameter(self.general_parameter()?)));
        }
        if self.at(T::GeneralParameter) {
            return Ok(Some(SessionResetArgs::Parameter(self.general_parameter()?)));
        }
        Ok(None)
    }

    /// `sessionCloseCommand: SESSION CLOSE`.
    fn session_close_command(&mut self) -> PResult<()> {
        self.expect_kw(K::Session)?;
        self.expect_kw(K::Close)?;
        Ok(())
    }

    /// `transactionActivity: startTransactionCommand
    /// (procedureSpecification endTransactionCommand?)? |
    /// procedureSpecification endTransactionCommand? |
    /// endTransactionCommand`.
    fn transaction_activity(&mut self) -> PResult<TransactionActivity> {
        let mut activity = TransactionActivity {
            start: None,
            procedure: None,
            end: None,
        };
        let start = self.start();
        if self.at_kw(K::Start) {
            let s = self.start_transaction_command()?;
            activity.start = Some(self.spanned(start, s));
            if !self.starts_procedure_body() {
                return Ok(activity);
            }
        } else if self.at_end_transaction() {
            let e = self.end_transaction_command()?;
            activity.end = Some(self.spanned(start, e));
            return Ok(activity);
        }
        let start = self.start();
        let p = self.procedure_body()?;
        activity.procedure = Some(self.spanned(start, p));
        if self.at_end_transaction() {
            let start = self.start();
            let e = self.end_transaction_command()?;
            activity.end = Some(self.spanned(start, e));
        }
        Ok(activity)
    }

    fn at_end_transaction(&mut self) -> bool {
        self.at_kw(K::Commit) | self.at_kw(K::Rollback)
    }

    /// `startTransactionCommand: START TRANSACTION
    /// transactionCharacteristics?`.
    fn start_transaction_command(&mut self) -> PResult<StartTransaction> {
        self.expect_kw(K::Start)?;
        self.expect_kw(K::Transaction)?;
        let modes = if self.at_kw(K::Read) {
            self.transaction_characteristics()?
        } else {
            Vec::new()
        };
        Ok(StartTransaction(modes))
    }

    /// `transactionCharacteristics: transactionMode (COMMA
    /// transactionMode)*`.
    fn transaction_characteristics(&mut self) -> PResult<Vec<Spanned<TransactionMode>>> {
        let mut modes = Vec::new();
        loop {
            let start = self.start();
            let m = self.transaction_access_mode()?;
            modes.push(self.spanned(start, m));
            if !self.eat(T::Comma) {
                return Ok(modes);
            }
        }
    }

    /// `transactionMode: transactionAccessMode`; `transactionAccessMode:
    /// READ ONLY | READ WRITE`.
    fn transaction_access_mode(&mut self) -> PResult<TransactionMode> {
        self.expect_kw(K::Read)?;
        if self.eat_kw(K::Only) {
            return Ok(TransactionMode::ReadOnly);
        }
        self.expect_kw(K::Write)?;
        Ok(TransactionMode::ReadWrite)
    }

    /// `endTransactionCommand: rollbackCommand | commitCommand`.
    fn end_transaction_command(&mut self) -> PResult<EndTransaction> {
        if self.eat_kw(K::Rollback) {
            return Ok(EndTransaction::Rollback);
        }
        self.expect_kw(K::Commit)?;
        Ok(EndTransaction::Commit)
    }

    /// Whether the current token can start a `procedureBody`.
    pub(super) fn starts_procedure_body(&mut self) -> bool {
        match self.peek() {
            T::Keyword(K::At | K::Value) => true,
            T::Keyword(K::Property | K::Graph | K::Binding | K::Table) => true,
            k => {
                let s = super::query::starts_statement(k, self.peek_nth(1));
                if !s {
                    self.expect_statement();
                }
                s
            }
        }
    }

    /// `nestedProcedureSpecification`, `nestedQuerySpecification` and
    /// `nestedDataModifyingProcedureSpecification`: `{ procedureBody }`.
    pub(super) fn nested_procedure_specification(&mut self) -> PResult<Spanned<Procedure>> {
        self.expect(T::LeftBrace)?;
        let start = self.start();
        let body = self.procedure_body()?;
        let body = self.spanned(start, body);
        self.expect(T::RightBrace)?;
        Ok(body)
    }

    /// `procedureBody: atSchemaClause? bindingVariableDefinitionBlock?
    /// statementBlock` (`procedureSpecification` is one).
    pub(super) fn procedure_body(&mut self) -> PResult<Procedure> {
        let _guard = self.nest()?;
        let at = if self.eat_kw(K::At) {
            Some(self.schema_reference(&super::catalog::follows_at_schema)?)
        } else {
            None
        };
        let binding_variable_defs = self.binding_variable_definition_block()?;
        let (statement, next_statements) = self.statement_block()?;
        Ok(Procedure {
            at,
            binding_variable_defs,
            statement,
            next_statements,
        })
    }

    /// `bindingVariableDefinitionBlock: bindingVariableDefinition+`
    /// (empty when absent).
    fn binding_variable_definition_block(&mut self) -> PResult<Vec<Spanned<BindingVariableDef>>> {
        let mut defs = Vec::new();
        while let Some(d) = self.binding_variable_definition()? {
            defs.push(d);
        }
        Ok(defs)
    }

    /// `bindingVariableDefinition: graphVariableDefinition |
    /// bindingTableVariableDefinition | valueVariableDefinition`.
    fn binding_variable_definition(&mut self) -> PResult<Option<Spanned<BindingVariableDef>>> {
        let start = self.start();
        let def = match (self.peek(), self.peek_nth(1)) {
            (T::Keyword(K::Graph), _) | (T::Keyword(K::Property), T::Keyword(K::Graph)) => {
                BindingVariableDef::Graph(self.graph_variable_definition()?)
            }
            (T::Keyword(K::Table), _) | (T::Keyword(K::Binding), T::Keyword(K::Table)) => {
                BindingVariableDef::BindingTable(self.binding_table_variable_definition()?)
            }
            (T::Keyword(K::Value), _) => {
                BindingVariableDef::Value(self.value_variable_definition()?)
            }
            _ => {
                self.at_kw(K::Property);
                self.at_kw(K::Graph);
                self.at_kw(K::Binding);
                self.at_kw(K::Table);
                self.at_kw(K::Value);
                return Ok(None);
            }
        };
        Ok(Some(self.spanned(start, def)))
    }

    /// `graphVariableDefinition: PROPERTY? GRAPH bindingVariable
    /// optTypedGraphInitializer`.
    fn graph_variable_definition(&mut self) -> PResult<GraphVariableDef> {
        self.eat_kw(K::Property);
        self.expect_kw(K::Graph)?;
        let name = self.regular_identifier()?;
        let init = self.opt_typed_graph_initializer(&super::catalog::follows_binding_definition)?;
        Ok(GraphVariableDef { name, init })
    }

    /// `optTypedGraphInitializer: (typed? graphReferenceValueType)?
    /// graphInitializer`; `graphInitializer: = graphExpression`.
    pub(super) fn opt_typed_graph_initializer(
        &mut self,
        follow: &super::catalog::Follow,
    ) -> PResult<Spanned<TypedGraphInitializer>> {
        let start = self.start();
        let value_type = if self.at(T::Equals) {
            None
        } else {
            self.typed();
            Some(self.graph_reference_value_type()?)
        };
        self.expect(T::Equals)?;
        let init = self.graph_expression(follow)?;
        Ok(self.spanned(start, TypedGraphInitializer { value_type, init }))
    }

    /// `bindingTableVariableDefinition: BINDING? TABLE bindingVariable
    /// optTypedBindingTableInitializer`.
    fn binding_table_variable_definition(&mut self) -> PResult<BindingTableVariableDef> {
        self.eat_kw(K::Binding);
        self.expect_kw(K::Table)?;
        let name = self.regular_identifier()?;
        let init =
            self.opt_typed_binding_table_initializer(&super::catalog::follows_binding_definition)?;
        Ok(BindingTableVariableDef { name, init })
    }

    /// `optTypedBindingTableInitializer: (typed?
    /// bindingTableReferenceValueType)? bindingTableInitializer`.
    pub(super) fn opt_typed_binding_table_initializer(
        &mut self,
        follow: &super::catalog::Follow,
    ) -> PResult<Spanned<TypedBindingTableInitializer>> {
        let start = self.start();
        let value_type = if self.at(T::Equals) {
            None
        } else {
            self.typed();
            Some(self.binding_table_reference_value_type()?)
        };
        self.expect(T::Equals)?;
        let init = self.binding_table_expression(follow)?;
        Ok(self.spanned(start, TypedBindingTableInitializer { value_type, init }))
    }

    /// `valueVariableDefinition: VALUE bindingVariable
    /// optTypedValueInitializer`.
    pub(super) fn value_variable_definition(&mut self) -> PResult<ValueVariableDef> {
        self.expect_kw(K::Value)?;
        let name = self.regular_identifier()?;
        let init = self.opt_typed_value_initializer()?;
        Ok(ValueVariableDef { name, init })
    }

    /// `optTypedValueInitializer: (typed? valueType)? valueInitializer`;
    /// `valueInitializer: = valueExpression`.
    pub(super) fn opt_typed_value_initializer(
        &mut self,
    ) -> PResult<Spanned<TypedValueInitializer>> {
        let start = self.start();
        let value_type = if self.at(T::Equals) {
            None
        } else {
            self.typed();
            Some(self.value_type()?)
        };
        self.expect(T::Equals)?;
        let init = Box::new(self.value_expression()?);
        Ok(self.spanned(start, TypedValueInitializer { value_type, init }))
    }

    /// `typed: DOUBLE_COLON | TYPED`, optional: whether one was written.
    pub(super) fn typed(&mut self) -> bool {
        self.eat(T::DoubleColon) || self.eat_kw(K::Typed)
    }

    /// `statementBlock: statement nextStatement*`.
    fn statement_block(
        &mut self,
    ) -> PResult<(
        Spanned<crate::query::ast::Statement>,
        Vec<Spanned<NextStatement>>,
    )> {
        let statement = self.statement()?;
        let mut next = Vec::new();
        while self.at_kw(K::Next) {
            let start = self.start();
            let n = self.next_statement()?;
            next.push(self.spanned(start, n));
        }
        Ok((statement, next))
    }

    /// `nextStatement: NEXT yieldClause? statement`.
    fn next_statement(&mut self) -> PResult<NextStatement> {
        self.expect_kw(K::Next)?;
        let yield_clause = if self.at_kw(K::Yield) {
            let start = self.start();
            let y = self.yield_clause()?;
            Some(self.spanned(start, y))
        } else {
            None
        };
        let statement = self.statement()?;
        Ok(NextStatement {
            yield_clause,
            statement,
        })
    }

    /// `GENERAL_PARAMETER_REFERENCE`: the name after `$`.
    pub(super) fn general_parameter(&mut self) -> PResult<Spanned<crate::query::ast::Ident>> {
        let span = self.expect(T::GeneralParameter)?;
        let t = self.token_at(self.pos - 1);
        Ok(Spanned(self.parameter_name(t, 1).into(), span))
    }

    /// The name of a parameter token after its `skip` dollar signs: a
    /// delimited name with its escapes resolved, or the text.
    pub(super) fn parameter_name(&mut self, t: crate::query::lexer::Token, skip: usize) -> String {
        let name = &t.text(self.text)[skip..];
        if name.starts_with(['"', '`', '@']) {
            let mut inner = t;
            inner.span.start.offset += u32::try_from(skip).unwrap_or(0);
            self.unquote(inner)
        } else {
            name.to_owned()
        }
    }
}
