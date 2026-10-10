//! Catalog-modifying, data-modifying and query statements (`GQL.g4` 12
//! to 16).

use super::{
    CatalogObjectRef, Expr, GraphExpr, GraphPatternBindingTable, GraphTypeRef, Ident,
    InsertPathPattern, NestedGraphType, Procedure, ProcedureRef, PropertyKeyValuePair, SchemaPath,
    ValueVariableDef,
};
use crate::query::span::{BoxSpanned, OptSpanned, Spanned, VecSpanned};

/// `simpleCatalogModifyingStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CatalogModifyingStatement {
    /// `callCatalogModifyingProcedureStatement`.
    Call(CallProcedureStatement),
    /// `createSchemaStatement`.
    CreateSchema(CreateSchemaStatement),
    /// `dropSchemaStatement`.
    DropSchema(DropSchemaStatement),
    /// `createGraphStatement`.
    CreateGraph(Box<CreateGraphStatement>),
    /// `dropGraphStatement`.
    DropGraph(DropStatement),
    /// `createGraphTypeStatement`.
    CreateGraphType(Box<CreateGraphTypeStatement>),
    /// `dropGraphTypeStatement`.
    DropGraphType(DropStatement),
}

/// `createSchemaStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateSchemaStatement {
    /// `catalogSchemaParentAndName`.
    pub(crate) path: Spanned<SchemaPath>,
    /// `IF NOT EXISTS`.
    pub(crate) if_not_exists: bool,
}

/// `dropSchemaStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DropSchemaStatement {
    /// `catalogSchemaParentAndName`.
    pub(crate) path: Spanned<SchemaPath>,
    /// `IF EXISTS`.
    pub(crate) if_exists: bool,
}

/// How a graph or graph type is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateKind {
    /// `CREATE`.
    Create,
    /// `CREATE … IF NOT EXISTS`.
    CreateIfNotExists,
    /// `CREATE OR REPLACE`.
    CreateOrReplace,
}

/// `createGraphStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateGraphStatement {
    /// `catalogGraphParentAndName`.
    pub(crate) path: Spanned<CatalogObjectRef>,
    /// How it is created.
    pub(crate) kind: Spanned<CreateKind>,
    /// `openGraphType` or `ofGraphType`.
    pub(crate) graph_type: Spanned<OfGraphType>,
    /// `graphSource`: `AS COPY OF graphExpression`.
    pub(crate) source: OptSpanned<GraphExpr>,
}

/// `openGraphType` and `ofGraphType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OfGraphType {
    /// `openGraphType`: `ANY [[PROPERTY] GRAPH]`.
    Any,
    /// `graphTypeLikeGraph`.
    Like(Spanned<GraphExpr>),
    /// `[typed] graphTypeReference`.
    Ref(Spanned<GraphTypeRef>),
    /// `[typed] [[PROPERTY] GRAPH] nestedGraphTypeSpecification`.
    Nested(Spanned<NestedGraphType>),
}

/// `dropGraphStatement` and `dropGraphTypeStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DropStatement {
    /// The object.
    pub(crate) path: Spanned<CatalogObjectRef>,
    /// `IF EXISTS`.
    pub(crate) if_exists: bool,
}

/// `createGraphTypeStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateGraphTypeStatement {
    /// `catalogGraphTypeParentAndName`.
    pub(crate) path: Spanned<CatalogObjectRef>,
    /// How it is created.
    pub(crate) kind: Spanned<CreateKind>,
    /// `graphTypeSource`.
    pub(crate) source: Spanned<GraphTypeSource>,
}

/// `graphTypeSource`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphTypeSource {
    /// `[AS] COPY OF graphTypeReference`.
    Copy(Spanned<GraphTypeRef>),
    /// `graphTypeLikeGraph`.
    Like(Spanned<GraphExpr>),
    /// `[AS] nestedGraphTypeSpecification`.
    Nested(Spanned<NestedGraphType>),
}

/// `linearDataModifyingStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinearDataModifyingStatement {
    /// `useGraphClause` of the focused form.
    pub(crate) use_graph: OptSpanned<GraphExpr>,
    /// The body.
    pub(crate) body: DataModifyingBody,
}

/// The body of a linear data-modifying statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DataModifyingBody {
    /// `simpleLinearDataAccessingStatement primitiveResultStatement?`.
    Statements {
        /// `simpleDataAccessingStatement`s.
        statements: VecSpanned<SimpleDataAccessingStatement>,
        /// `primitiveResultStatement`.
        result: OptSpanned<ResultStatement>,
    },
    /// `nestedDataModifyingProcedureSpecification`.
    Nested(BoxSpanned<Procedure>),
}

/// `simpleDataAccessingStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SimpleDataAccessingStatement {
    /// `simpleQueryStatement`; a `CALL` is always this alternative.
    Query(Box<SimpleQueryStatement>),
    /// `primitiveDataModifyingStatement`.
    Modifying(DataModifyingStatement),
}

/// `primitiveDataModifyingStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DataModifyingStatement {
    /// `insertStatement`.
    Insert(VecSpanned<InsertPathPattern>),
    /// `setStatement`.
    Set(VecSpanned<SetItem>),
    /// `removeStatement`.
    Remove(VecSpanned<RemoveItem>),
    /// `deleteStatement`.
    Delete {
        /// `DETACH` (`Some(true)`), `NODETACH` (`Some(false)`) or neither.
        detach: Option<bool>,
        /// `deleteItemList`.
        items: VecSpanned<Expr>,
    },
}

/// `setItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetItem {
    /// `setPropertyItem`: `v.p = expr`.
    Property {
        /// The variable.
        variable: Spanned<Ident>,
        /// The property.
        property: Spanned<Ident>,
        /// The value.
        value: Spanned<Expr>,
    },
    /// `setAllPropertiesItem`: `v = {…}`.
    AllProperties {
        /// The variable.
        variable: Spanned<Ident>,
        /// The properties.
        properties: VecSpanned<PropertyKeyValuePair>,
    },
    /// `setLabelItem`: `v IS L` or `v:L`.
    Label {
        /// The variable.
        variable: Spanned<Ident>,
        /// The label.
        label: Spanned<Ident>,
    },
}

/// `removeItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoveItem {
    /// `removePropertyItem`: `v.p`.
    Property {
        /// The variable.
        variable: Spanned<Ident>,
        /// The property.
        property: Spanned<Ident>,
    },
    /// `removeLabelItem`: `v IS L` or `v:L`.
    Label {
        /// The variable.
        variable: Spanned<Ident>,
        /// The label.
        label: Spanned<Ident>,
    },
}

/// `compositeQueryExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompositeQueryExpr {
    /// `compositeQueryExpression queryConjunction compositeQueryPrimary`.
    Conjunction {
        /// The conjunction.
        conjunction: Spanned<QueryConjunction>,
        /// The left operand.
        left: BoxSpanned<CompositeQueryExpr>,
        /// The right operand.
        right: BoxSpanned<LinearQueryStatement>,
    },
    /// `compositeQueryPrimary`.
    Primary(Box<LinearQueryStatement>),
}

/// `queryConjunction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryConjunction {
    /// `setOperator`.
    SetOp {
        /// The operator.
        kind: SetOpKind,
        /// Its set quantifier.
        quantifier: Option<SetQuantifier>,
    },
    /// `OTHERWISE`.
    Otherwise,
}

/// The set operator of `setOperator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetOpKind {
    /// `UNION`.
    Union,
    /// `EXCEPT`.
    Except,
    /// `INTERSECT`.
    Intersect,
}

/// `setQuantifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetQuantifier {
    /// `DISTINCT`.
    Distinct,
    /// `ALL`.
    All,
}

/// `linearQueryStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinearQueryStatement {
    /// `focusedLinearQueryStatement`.
    Focused(FocusedLinearQueryStatement),
    /// `ambientLinearQueryStatement`.
    Ambient(AmbientLinearQueryStatement),
}

/// `focusedLinearQueryStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FocusedLinearQueryStatement {
    /// `focusedLinearQueryStatementPart* focusedLinearQueryAndPrimitiveResultStatementPart`.
    Parts {
        /// The parts, the last one included.
        parts: VecSpanned<FocusedLinearQueryStatementPart>,
        /// The result statement of the last part.
        result: Spanned<ResultStatement>,
    },
    /// `focusedPrimitiveResultStatement`.
    Result {
        /// The graph.
        use_graph: Spanned<GraphExpr>,
        /// The result statement.
        result: Spanned<ResultStatement>,
    },
    /// `focusedNestedQuerySpecification`.
    Nested {
        /// The graph.
        use_graph: Spanned<GraphExpr>,
        /// The nested query.
        query: BoxSpanned<Procedure>,
    },
    /// `selectStatement`.
    Select(Box<SelectStatement>),
}

/// `focusedLinearQueryStatementPart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusedLinearQueryStatementPart {
    /// The graph.
    pub(crate) use_graph: Spanned<GraphExpr>,
    /// `simpleLinearQueryStatement`.
    pub(crate) statements: VecSpanned<SimpleQueryStatement>,
}

/// `ambientLinearQueryStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AmbientLinearQueryStatement {
    /// `simpleLinearQueryStatement? primitiveResultStatement`.
    Parts {
        /// The statements.
        statements: VecSpanned<SimpleQueryStatement>,
        /// The result statement.
        result: Spanned<ResultStatement>,
    },
    /// `nestedQuerySpecification`.
    Nested(BoxSpanned<Procedure>),
}

/// `simpleQueryStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SimpleQueryStatement {
    /// `matchStatement`.
    Match(MatchStatement),
    /// `letStatement`.
    Let(VecSpanned<LetVariableDef>),
    /// `forStatement`.
    For(ForStatement),
    /// `filterStatement`: the search condition.
    Filter(Spanned<Expr>),
    /// `orderByAndPageStatement`.
    OrderByAndPage(OrderByAndPage),
    /// `callQueryStatement`.
    Call(CallProcedureStatement),
}

/// `matchStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MatchStatement {
    /// `simpleMatchStatement`.
    Simple(BoxSpanned<GraphPatternBindingTable>),
    /// `optionalMatchStatement`.
    Optional(Spanned<OptionalOperand>),
}

/// `optionalOperand`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OptionalOperand {
    /// `simpleMatchStatement`.
    Match(BoxSpanned<GraphPatternBindingTable>),
    /// `{ matchStatementBlock }` or `( matchStatementBlock )`.
    Block(VecSpanned<MatchStatement>),
}

/// `letVariableDefinition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LetVariableDef {
    /// `valueVariableDefinition`.
    Value(ValueVariableDef),
    /// `bindingVariable = valueExpression`.
    Assign {
        /// The variable.
        variable: Spanned<Ident>,
        /// The value.
        value: Spanned<Expr>,
    },
}

/// `forStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForStatement {
    /// `forItemAlias`.
    pub(crate) alias: Spanned<Ident>,
    /// `forItemSource`.
    pub(crate) source: Spanned<Expr>,
    /// `forOrdinalityOrOffset`.
    pub(crate) ordinality_or_offset: OptSpanned<ForOrdinalityOrOffset>,
}

/// `forOrdinalityOrOffset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForOrdinalityOrOffset {
    /// `ORDINALITY` (`true`) or `OFFSET` (`false`).
    pub(crate) ordinality: bool,
    /// The variable.
    pub(crate) variable: Spanned<Ident>,
}

/// `orderByAndPageStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderByAndPage {
    /// `orderByClause`.
    pub(crate) order_by: VecSpanned<SortSpec>,
    /// `offsetClause`.
    pub(crate) offset: OptSpanned<NonNegativeInteger>,
    /// `limitClause`.
    pub(crate) limit: OptSpanned<NonNegativeInteger>,
}

/// `sortSpecification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SortSpec {
    /// `sortKey`.
    pub(crate) key: Spanned<Expr>,
    /// `orderingSpecification`.
    pub(crate) ordering: OptSpanned<Ordering>,
    /// `nullOrdering`.
    pub(crate) null_ordering: OptSpanned<NullOrdering>,
}

/// `orderingSpecification`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ordering {
    /// `ASC` or `ASCENDING`.
    Asc,
    /// `DESC` or `DESCENDING`.
    Desc,
}

/// `nullOrdering`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NullOrdering {
    /// `NULLS FIRST`.
    First,
    /// `NULLS LAST`.
    Last,
}

/// `nonNegativeIntegerSpecification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NonNegativeInteger {
    /// `unsignedInteger`.
    Integer(UnsignedInteger),
    /// `$name`.
    Parameter(Ident),
}

/// `unsignedInteger`: its text (underscores kept) and radix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnsignedInteger {
    /// The radix: 2, 8, 10 or 16.
    pub(crate) radix: u32,
    /// The digits, without the `0x`, `0o` or `0b` prefix.
    pub(crate) digits: Box<str>,
}

/// `primitiveResultStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResultStatement {
    /// `returnStatement orderByAndPageStatement?`.
    Return {
        /// `returnStatement`.
        statement: BoxSpanned<ReturnStatement>,
        /// `orderByAndPageStatement`.
        order_by: Option<BoxSpanned<OrderByAndPage>>,
    },
    /// `FINISH`.
    Finish,
}

/// `returnStatementBody`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReturnStatement {
    /// `setQuantifier`.
    pub(crate) quantifier: OptSpanned<SetQuantifier>,
    /// `*` or `returnItemList`.
    pub(crate) items: Spanned<ReturnItems>,
    /// `groupByClause`.
    pub(crate) group_by: OptSpanned<GroupBy>,
}

/// `*` or a list of items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReturnItems {
    /// `*`.
    All,
    /// `returnItemList` or `selectItemList`.
    Items(VecSpanned<ReturnItem>),
}

/// `returnItem` and `selectItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReturnItem {
    /// The value.
    pub(crate) value: Spanned<Expr>,
    /// `AS identifier`.
    pub(crate) alias: OptSpanned<Ident>,
}

/// `groupingElementList`: binding variables, or `()` (empty).
pub(crate) type GroupBy = VecSpanned<Ident>;

/// `selectStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectStatement {
    /// `setQuantifier`.
    pub(crate) quantifier: OptSpanned<SetQuantifier>,
    /// `*` or `selectItemList`.
    pub(crate) items: Spanned<ReturnItems>,
    /// `selectStatementBody` and the clauses after it.
    pub(crate) body: Option<SelectBody>,
}

/// `selectStatementBody` and the clauses that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectBody {
    /// `FROM …`.
    pub(crate) from: Spanned<SelectFrom>,
    /// `whereClause`.
    pub(crate) where_clause: OptSpanned<Expr>,
    /// `groupByClause`.
    pub(crate) group_by: OptSpanned<GroupBy>,
    /// `havingClause`.
    pub(crate) having: OptSpanned<Expr>,
    /// `orderByClause`.
    pub(crate) order_by: Option<VecSpanned<SortSpec>>,
    /// `offsetClause`.
    pub(crate) offset: OptSpanned<NonNegativeInteger>,
    /// `limitClause`.
    pub(crate) limit: OptSpanned<NonNegativeInteger>,
}

/// What `SELECT … FROM` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectFrom {
    /// `selectGraphMatchList`.
    GraphMatches(VecSpanned<SelectGraphMatch>),
    /// `selectQuerySpecification`.
    Query {
        /// The graph, if one is written.
        graph: OptSpanned<GraphExpr>,
        /// The nested query.
        query: BoxSpanned<Procedure>,
    },
}

/// `selectGraphMatch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectGraphMatch {
    /// The graph.
    pub(crate) graph: Spanned<GraphExpr>,
    /// The match statement.
    pub(crate) statement: Spanned<MatchStatement>,
}

/// `callProcedureStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallProcedureStatement {
    /// `OPTIONAL`.
    pub(crate) optional: bool,
    /// `procedureCall`.
    pub(crate) procedure: Spanned<ProcedureCall>,
}

/// `procedureCall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcedureCall {
    /// `inlineProcedureCall`.
    Inline(InlineProcedureCall),
    /// `namedProcedureCall`.
    Named(Box<NamedProcedureCall>),
}

/// `inlineProcedureCall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineProcedureCall {
    /// `variableScopeClause`: `None` without the clause, `Some` with the
    /// listed variables (possibly none).
    pub(crate) scope: Option<VecSpanned<Ident>>,
    /// `nestedProcedureSpecification`.
    pub(crate) procedure: BoxSpanned<Procedure>,
}

/// `namedProcedureCall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedProcedureCall {
    /// `procedureReference`.
    pub(crate) name: Spanned<ProcedureRef>,
    /// `procedureArgumentList`.
    pub(crate) args: VecSpanned<Expr>,
    /// `yieldClause`.
    pub(crate) yield_clause: OptSpanned<Yield>,
}

/// `yieldItemList`.
pub(crate) type Yield = VecSpanned<YieldItem>;

/// `yieldItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct YieldItem {
    /// `yieldItemName`.
    pub(crate) name: Spanned<Ident>,
    /// `yieldItemAlias`.
    pub(crate) alias: OptSpanned<Ident>,
}
