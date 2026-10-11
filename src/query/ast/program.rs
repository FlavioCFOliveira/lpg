//! Programs, sessions, transactions and procedures (`GQL.g4` 6 to 10).

use super::{
    BindingTableExpr, CatalogModifyingStatement, CompositeQueryExpr, Expr, GraphExpr, Ident,
    LinearDataModifyingStatement, SchemaRef, StringLiteral, ValueType, Yield,
};
use crate::query::span::{BoxSpanned, OptSpanned, Span, Spanned, VecSpanned};

/// `gqlProgram`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Program {
    /// The program activity, absent in a lone `SESSION CLOSE`.
    pub(crate) activity: OptSpanned<ProgramActivity>,
    /// Whether the program ends with `SESSION CLOSE`.
    pub(crate) session_close: bool,
    /// Spans of the string literals and delimited identifiers with a
    /// `\u` or `\U` escape that names no Unicode scalar value. Lexically
    /// valid (`GQL.g4`); the binder rejects them.
    pub(crate) invalid_escapes: Vec<Span>,
}

/// `programActivity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProgramActivity {
    /// `sessionActivity`.
    Session(SessionActivity),
    /// `transactionActivity`.
    Transaction(Box<TransactionActivity>),
}

/// `sessionActivity`: session set commands, then session reset commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionActivity {
    /// `sessionSetCommand`s.
    pub(crate) set: VecSpanned<SessionSet>,
    /// `sessionResetCommand`s.
    pub(crate) reset: VecSpanned<SessionReset>,
}

/// `sessionSetCommand`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionSet {
    /// `SESSION SET SCHEMA schemaReference`.
    Schema(Spanned<SchemaRef>),
    /// `SESSION SET [PROPERTY] GRAPH graphExpression`.
    Graph(Spanned<GraphExpr>),
    /// `SESSION SET TIME ZONE 'string'`.
    TimeZone(Spanned<StringLiteral>),
    /// `sessionSetParameterClause`.
    Parameter(Box<Spanned<SessionSetParameter>>),
}

/// `sessionSetParameterClause`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionSetParameter {
    /// The parameter name, without `$`.
    pub(crate) name: Spanned<Ident>,
    /// `IF NOT EXISTS`.
    pub(crate) if_not_exists: bool,
    /// What the parameter is set to.
    pub(crate) kind: SessionSetParameterKind,
}

/// The value a session parameter is set to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionSetParameterKind {
    /// `[PROPERTY] GRAPH $p optTypedGraphInitializer`.
    Graph(Spanned<TypedGraphInitializer>),
    /// `[BINDING] TABLE $p optTypedBindingTableInitializer`.
    BindingTable(Spanned<TypedBindingTableInitializer>),
    /// `VALUE $p optTypedValueInitializer`.
    Value(Spanned<TypedValueInitializer>),
}

/// `sessionResetCommand`: `SESSION RESET` and its optional arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionReset(pub(crate) OptSpanned<SessionResetArgs>);

/// `sessionResetArguments`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionResetArgs {
    /// `[ALL] PARAMETERS`.
    Parameters,
    /// `[ALL] CHARACTERISTICS`.
    Characteristics,
    /// `SCHEMA`.
    Schema,
    /// `[PROPERTY] GRAPH`.
    Graph,
    /// `TIME ZONE`.
    TimeZone,
    /// `[PARAMETER] $name`.
    Parameter(Spanned<Ident>),
}

/// `transactionActivity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransactionActivity {
    /// `startTransactionCommand`.
    pub(crate) start: OptSpanned<StartTransaction>,
    /// `procedureSpecification`.
    pub(crate) procedure: OptSpanned<Procedure>,
    /// `endTransactionCommand`.
    pub(crate) end: OptSpanned<EndTransaction>,
}

/// `startTransactionCommand`: its transaction modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartTransaction(pub(crate) VecSpanned<TransactionMode>);

/// `transactionAccessMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionMode {
    /// `READ ONLY`.
    ReadOnly,
    /// `READ WRITE`.
    ReadWrite,
}

/// `endTransactionCommand`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndTransaction {
    /// `ROLLBACK`.
    Rollback,
    /// `COMMIT`.
    Commit,
}

/// `procedureBody` (and `procedureSpecification`, which is one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Procedure {
    /// `atSchemaClause`.
    pub(crate) at: OptSpanned<SchemaRef>,
    /// `bindingVariableDefinitionBlock`.
    pub(crate) binding_variable_defs: VecSpanned<BindingVariableDef>,
    /// The first statement of `statementBlock`.
    pub(crate) statement: Spanned<Statement>,
    /// `nextStatement`s.
    pub(crate) next_statements: VecSpanned<NextStatement>,
}

/// `statement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Statement {
    /// `compositeQueryStatement`.
    Query(CompositeQueryExpr),
    /// `linearCatalogModifyingStatement`.
    Catalog(VecSpanned<CatalogModifyingStatement>),
    /// `linearDataModifyingStatement`.
    Data(Box<LinearDataModifyingStatement>),
}

/// `nextStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NextStatement {
    /// `yieldClause`.
    pub(crate) yield_clause: OptSpanned<Yield>,
    /// The statement.
    pub(crate) statement: Spanned<Statement>,
}

/// `bindingVariableDefinition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingVariableDef {
    /// `graphVariableDefinition`.
    Graph(GraphVariableDef),
    /// `bindingTableVariableDefinition`.
    BindingTable(BindingTableVariableDef),
    /// `valueVariableDefinition`.
    Value(ValueVariableDef),
}

/// `graphVariableDefinition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphVariableDef {
    /// The variable.
    pub(crate) name: Spanned<Ident>,
    /// `optTypedGraphInitializer`.
    pub(crate) init: Spanned<TypedGraphInitializer>,
}

/// `bindingTableVariableDefinition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindingTableVariableDef {
    /// The variable.
    pub(crate) name: Spanned<Ident>,
    /// `optTypedBindingTableInitializer`.
    pub(crate) init: Spanned<TypedBindingTableInitializer>,
}

/// `valueVariableDefinition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueVariableDef {
    /// The variable.
    pub(crate) name: Spanned<Ident>,
    /// `optTypedValueInitializer`.
    pub(crate) init: Spanned<TypedValueInitializer>,
}

/// `optTypedGraphInitializer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedGraphInitializer {
    /// `graphReferenceValueType`.
    pub(crate) value_type: OptSpanned<ValueType>,
    /// `graphInitializer`.
    pub(crate) init: Spanned<GraphExpr>,
}

/// `optTypedBindingTableInitializer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedBindingTableInitializer {
    /// `bindingTableReferenceValueType`.
    pub(crate) value_type: OptSpanned<ValueType>,
    /// `bindingTableInitializer`.
    pub(crate) init: Spanned<BindingTableExpr>,
}

/// `optTypedValueInitializer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedValueInitializer {
    /// `valueType`.
    pub(crate) value_type: OptSpanned<ValueType>,
    /// `valueInitializer`.
    pub(crate) init: BoxSpanned<Expr>,
}
