//! Value expressions, predicates, functions and literals (`GQL.g4` 19 to
//! 21).

use super::{
    BindingTableExpr, GraphExpr, GraphPattern, Ident, LabelExpr, LetVariableDef, ListTypeName,
    MatchStatement, Procedure, SetQuantifier, ValueType,
};
use crate::query::span::{BoxSpanned, Spanned, VecSpanned};

/// `valueExpression` and the expression rules it absorbs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Expr {
    /// `+e`, `-e`, `NOT e`.
    Unary {
        /// The operator.
        op: Spanned<UnaryOp>,
        /// The operand.
        child: BoxSpanned<Expr>,
    },
    /// The binary operators of `valueExpression` and
    /// `numericValueExpression`.
    Binary {
        /// The operator.
        op: Spanned<BinaryOp>,
        /// The left operand.
        left: BoxSpanned<Expr>,
        /// The right operand.
        right: BoxSpanned<Expr>,
    },
    /// `e IS [NOT] TRUE|FALSE|UNKNOWN`.
    IsTruth {
        /// The operand.
        expr: BoxSpanned<Expr>,
        /// `NOT`.
        not: bool,
        /// The truth value.
        value: TruthValue,
    },
    /// `e IS [NOT] [normalForm] NORMALIZED` (`normalizedPredicatePart2`).
    IsNormalized {
        /// The operand.
        expr: BoxSpanned<Expr>,
        /// `NOT`.
        not: bool,
        /// `normalForm`.
        form: Option<NormalForm>,
    },
    /// `predicate`.
    Predicate(Predicate),
    /// `[PROPERTY] GRAPH graphExpression`.
    Graph(BoxSpanned<GraphExpr>),
    /// `[BINDING] TABLE bindingTableExpression`.
    BindingTable(BoxSpanned<BindingTableExpr>),
    /// `valueFunction`, and the functions of `numericValueExpression`.
    Function(Function),
    /// An aggregate function (`aggregateFunction`).
    Aggregate(Aggregate),
    /// `unsignedLiteral`.
    Literal(Literal),
    /// `$name` (`dynamicParameterSpecification`).
    Parameter(Ident),
    /// `SESSION_USER`.
    SessionUser,
    /// `bindingVariableReference`.
    Variable(Ident),
    /// `valueExpressionPrimary . propertyName`.
    Property {
        /// The source.
        source: BoxSpanned<Expr>,
        /// The property.
        name: Spanned<Ident>,
    },
    /// `( valueExpression )`.
    Parenthesized(BoxSpanned<Expr>),
    /// `pathValueConstructor`: `PATH[n, e, n, …]`.
    Path(VecSpanned<Expr>),
    /// `valueQueryExpression`: `VALUE { … }`.
    ValueQuery(BoxSpanned<Procedure>),
    /// `caseExpression`.
    Case(Case),
    /// `castSpecification`.
    Cast {
        /// `castOperand`.
        operand: BoxSpanned<Expr>,
        /// `castTarget`.
        target: BoxSpanned<ValueType>,
    },
    /// `element_idFunction`.
    ElementId(Spanned<Ident>),
    /// `letValueExpression`.
    Let {
        /// `letVariableDefinitionList`.
        definitions: VecSpanned<LetVariableDef>,
        /// The value.
        value: BoxSpanned<Expr>,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `NOT`.
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `||`.
    Concat,
    /// `=`.
    Eq,
    /// `<>`.
    Ne,
    /// `<`.
    Lt,
    /// `>`.
    Gt,
    /// `<=`.
    Le,
    /// `>=`.
    Ge,
    /// `AND`.
    And,
    /// `OR`.
    Or,
    /// `XOR`.
    Xor,
}

/// `truthValue` (`BOOLEAN_LITERAL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TruthValue {
    /// `TRUE`.
    True,
    /// `FALSE`.
    False,
    /// `UNKNOWN`.
    Unknown,
}

/// `normalForm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalForm {
    /// `NFC`.
    Nfc,
    /// `NFD`.
    Nfd,
    /// `NFKC`.
    Nfkc,
    /// `NFKD`.
    Nfkd,
}

/// `predicate` (19.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Predicate {
    /// `existsPredicate`.
    Exists(Exists),
    /// `nullPredicate`.
    Null {
        /// The operand.
        expr: BoxSpanned<Expr>,
        /// `NOT`.
        not: bool,
    },
    /// `valueTypePredicate`.
    ValueType {
        /// The operand.
        expr: BoxSpanned<Expr>,
        /// `NOT`.
        not: bool,
        /// The type.
        value_type: BoxSpanned<ValueType>,
    },
    /// `directedPredicate`.
    Directed {
        /// The edge variable.
        edge: Spanned<Ident>,
        /// `NOT`.
        not: bool,
    },
    /// `labeledPredicate`.
    Labeled {
        /// The element variable.
        element: Spanned<Ident>,
        /// `IS NOT LABELED`.
        not: bool,
        /// The label expression.
        label: Spanned<LabelExpr>,
    },
    /// `sourceDestinationPredicate` with `sourcePredicatePart2`.
    Source {
        /// The node variable.
        node: Spanned<Ident>,
        /// `NOT`.
        not: bool,
        /// The edge variable.
        edge: Spanned<Ident>,
    },
    /// `sourceDestinationPredicate` with `destinationPredicatePart2`.
    Destination {
        /// The node variable.
        node: Spanned<Ident>,
        /// `NOT`.
        not: bool,
        /// The edge variable.
        edge: Spanned<Ident>,
    },
    /// `all_differentPredicate`.
    AllDifferent(VecSpanned<Ident>),
    /// `samePredicate`.
    Same(VecSpanned<Ident>),
    /// `property_existsPredicate`.
    PropertyExists {
        /// The element variable.
        element: Spanned<Ident>,
        /// The property.
        property: Spanned<Ident>,
    },
}

/// The forms of `existsPredicate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Exists {
    /// `EXISTS { graphPattern }` or `EXISTS ( graphPattern )`.
    Pattern(BoxSpanned<GraphPattern>),
    /// `EXISTS { matchStatementBlock }` or `EXISTS ( … )`.
    Match(VecSpanned<MatchStatement>),
    /// `EXISTS nestedQuerySpecification`.
    Nested(BoxSpanned<Procedure>),
}

/// `caseExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Case {
    /// `NULLIF(a, b)`.
    NullIf(BoxSpanned<Expr>, BoxSpanned<Expr>),
    /// `COALESCE(a, b, …)`.
    Coalesce(VecSpanned<Expr>),
    /// `simpleCase`.
    Simple {
        /// `caseOperand`.
        operand: BoxSpanned<Expr>,
        /// `simpleWhenClause`s.
        whens: VecSpanned<SimpleWhen>,
        /// `elseClause`.
        else_result: Option<BoxSpanned<Expr>>,
    },
    /// `searchedCase`.
    Searched {
        /// `searchedWhenClause`s: condition and result.
        whens: Vec<(Spanned<Expr>, Spanned<Expr>)>,
        /// `elseClause`.
        else_result: Option<BoxSpanned<Expr>>,
    },
}

/// `simpleWhenClause`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SimpleWhen {
    /// `whenOperandList`.
    pub(crate) operands: VecSpanned<WhenOperand>,
    /// `result`.
    pub(crate) result: Spanned<Expr>,
}

/// `whenOperand`: a value compared with the case operand, or the second
/// part of a predicate applied to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WhenOperand {
    /// `nonParenthesizedValueExpressionPrimary`: compared for equality.
    Value(Spanned<Expr>),
    /// `compOp valueExpression`.
    Comparison {
        /// The comparison operator.
        op: BinaryOp,
        /// The right operand.
        value: Spanned<Expr>,
    },
    /// `nullPredicatePart2`.
    Null {
        /// `NOT`.
        not: bool,
    },
    /// `valueTypePredicatePart2`.
    ValueType {
        /// `NOT`.
        not: bool,
        /// The type.
        value_type: BoxSpanned<ValueType>,
    },
    /// `normalizedPredicatePart2`.
    Normalized {
        /// `NOT`.
        not: bool,
        /// `normalForm`.
        form: Option<NormalForm>,
    },
    /// `directedPredicatePart2`.
    Directed {
        /// `NOT`.
        not: bool,
    },
    /// `labeledPredicatePart2`.
    Labeled {
        /// `IS NOT LABELED`.
        not: bool,
        /// The label expression.
        label: Spanned<LabelExpr>,
    },
    /// `sourcePredicatePart2`.
    Source {
        /// `NOT`.
        not: bool,
        /// The edge variable.
        edge: Spanned<Ident>,
    },
    /// `destinationPredicatePart2`.
    Destination {
        /// `NOT`.
        not: bool,
        /// The edge variable.
        edge: Spanned<Ident>,
    },
}

/// The aggregate functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Aggregate {
    /// `COUNT(*)`.
    CountAll,
    /// `generalSetFunction`.
    General {
        /// `generalSetFunctionType`.
        kind: GeneralSetFunction,
        /// `setQuantifier`.
        quantifier: Option<SetQuantifier>,
        /// The argument.
        expr: BoxSpanned<Expr>,
    },
    /// `binarySetFunction`.
    Binary {
        /// `PERCENTILE_CONT` (`true`) or `PERCENTILE_DISC`.
        continuous: bool,
        /// `setQuantifier` of the dependent value.
        quantifier: Option<SetQuantifier>,
        /// `dependentValueExpression`.
        dependent: BoxSpanned<Expr>,
        /// `independentValueExpression`.
        independent: BoxSpanned<Expr>,
    },
}

/// `generalSetFunctionType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneralSetFunction {
    /// `AVG`.
    Avg,
    /// `COUNT`.
    Count,
    /// `MAX`.
    Max,
    /// `MIN`.
    Min,
    /// `SUM`.
    Sum,
    /// `COLLECT_LIST`.
    CollectList,
    /// `STDDEV_SAMP`.
    StddevSamp,
    /// `STDDEV_POP`.
    StddevPop,
}

/// The built-in functions of `valueFunction` and `numericValueFunction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Function {
    /// A function of one argument.
    Unary {
        /// The function.
        kind: UnaryFunction,
        /// The argument.
        arg: BoxSpanned<Expr>,
    },
    /// A function of two arguments.
    Binary {
        /// The function.
        kind: BinaryFunction,
        /// The first argument.
        left: BoxSpanned<Expr>,
        /// The second argument.
        right: BoxSpanned<Expr>,
    },
    /// `trimSingleCharacterOrByteString`.
    Trim {
        /// `trimSpecification`.
        specification: Option<TrimSpecification>,
        /// `trimCharacterOrByteString`.
        characters: Option<BoxSpanned<Expr>>,
        /// `trimCharacterOrByteStringSource`.
        source: BoxSpanned<Expr>,
    },
    /// `trimMultiCharacterCharacterString`.
    MultiTrim {
        /// `BTRIM`, `LTRIM` or `RTRIM`.
        kind: TrimSpecification,
        /// The string.
        source: BoxSpanned<Expr>,
        /// The characters.
        characters: Option<BoxSpanned<Expr>>,
    },
    /// `normalizeCharacterString`.
    Normalize {
        /// The string.
        source: BoxSpanned<Expr>,
        /// `normalForm`.
        form: Option<NormalForm>,
    },
    /// `datetimeValueFunction`.
    Datetime {
        /// The function.
        kind: DatetimeFunction,
        /// Its parameters: `None` without parentheses or without
        /// parameters inside them.
        parameters: Option<BoxSpanned<Expr>>,
    },
    /// `datetimeSubtraction`.
    DurationBetween {
        /// The first datetime.
        left: BoxSpanned<Expr>,
        /// The second datetime.
        right: BoxSpanned<Expr>,
        /// `temporalDurationQualifier`: `YEAR TO MONTH` (`Some(true)`).
        year_to_month: Option<bool>,
    },
    /// `durationFunction`.
    Duration(BoxSpanned<Expr>),
}

/// Functions of one argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryFunction {
    /// `CHAR_LENGTH`, `CHARACTER_LENGTH`.
    CharLength,
    /// `BYTE_LENGTH`, `OCTET_LENGTH`.
    ByteLength,
    /// `PATH_LENGTH`.
    PathLength,
    /// `CARDINALITY`.
    Cardinality,
    /// `SIZE`.
    Size,
    /// `ABS` (numeric, or duration: decided by the binder).
    Abs,
    /// `SIN`.
    Sin,
    /// `COS`.
    Cos,
    /// `TAN`.
    Tan,
    /// `COT`.
    Cot,
    /// `SINH`.
    Sinh,
    /// `COSH`.
    Cosh,
    /// `TANH`.
    Tanh,
    /// `ASIN`.
    Asin,
    /// `ACOS`.
    Acos,
    /// `ATAN`.
    Atan,
    /// `DEGREES`.
    Degrees,
    /// `RADIANS`.
    Radians,
    /// `LOG10`.
    Log10,
    /// `LN`.
    Ln,
    /// `EXP`.
    Exp,
    /// `SQRT`.
    Sqrt,
    /// `FLOOR`.
    Floor,
    /// `CEIL`, `CEILING`.
    Ceiling,
    /// `UPPER`.
    Upper,
    /// `LOWER`.
    Lower,
    /// `ELEMENTS`.
    Elements,
}

/// Functions of two arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryFunction {
    /// `MOD`.
    Mod,
    /// `LOG(base, x)`.
    Log,
    /// `POWER`.
    Power,
    /// `LEFT(s, n)`.
    Left,
    /// `RIGHT(s, n)`.
    Right,
    /// `TRIM(list, n)` (`trimListFunction`).
    TrimList,
}

/// `trimSpecification` and the multi-character trims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrimSpecification {
    /// `LEADING`, `LTRIM`.
    Leading,
    /// `TRAILING`, `RTRIM`.
    Trailing,
    /// `BOTH`, `BTRIM`.
    Both,
}

/// `datetimeValueFunction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatetimeFunction {
    /// `CURRENT_DATE`.
    CurrentDate,
    /// `DATE(…)`.
    Date,
    /// `CURRENT_TIME`.
    CurrentTime,
    /// `ZONED_TIME(…)`.
    ZonedTime,
    /// `LOCAL_TIME [(…)]`.
    LocalTime,
    /// `CURRENT_TIMESTAMP`.
    CurrentTimestamp,
    /// `ZONED_DATETIME(…)`.
    ZonedDatetime,
    /// `LOCAL_TIMESTAMP`.
    LocalTimestamp,
    /// `LOCAL_DATETIME(…)`.
    LocalDatetime,
}

/// `unsignedLiteral`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Literal {
    /// `unsignedNumericLiteral`.
    Numeric(NumericLiteral),
    /// `BOOLEAN_LITERAL`.
    Boolean(TruthValue),
    /// `characterStringLiteral`.
    String(StringLiteral),
    /// `BYTE_STRING_LITERAL`: the hexadecimal digits, spaces removed.
    Bytes(Box<str>),
    /// `temporalLiteral`.
    Temporal {
        /// `DATE`, `TIME`, `DATETIME` or `TIMESTAMP`.
        kind: TemporalLiteralKind,
        /// The string.
        value: Spanned<StringLiteral>,
    },
    /// `durationLiteral`.
    Duration(Spanned<StringLiteral>),
    /// `NULL`.
    Null,
    /// `listLiteral`.
    List {
        /// `listValueTypeName`.
        name: Option<ListTypeName>,
        /// `listElementList`.
        elements: VecSpanned<Expr>,
    },
    /// `recordLiteral` (and `recordConstructor`).
    Record(VecSpanned<Field>),
}

/// `field`: `name: value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    /// `fieldName`.
    pub(crate) name: Spanned<Ident>,
    /// The value.
    pub(crate) value: Spanned<Expr>,
}

/// The kind of a temporal literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalLiteralKind {
    /// `DATE`.
    Date,
    /// `TIME`.
    Time,
    /// `DATETIME`.
    Datetime,
    /// `TIMESTAMP`.
    Timestamp,
}

/// `characterStringLiteral`: the value with its escapes resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StringLiteral {
    /// The value. An escape that names no character is U+FFFD and is
    /// listed in [`super::Program::invalid_escapes`].
    pub(crate) value: Box<str>,
    /// `@` (`NO_ESCAPE`): escapes are not resolved.
    pub(crate) no_escape: bool,
}

/// `unsignedNumericLiteral`: its token and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NumericLiteral {
    /// The kind.
    pub(crate) kind: NumericLiteralKind,
    /// The text as written, without the `0x`/`0o`/`0b` prefix.
    pub(crate) text: Box<str>,
}

/// The token of a numeric literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericLiteralKind {
    /// An `unsignedInteger` of radix 10, 16, 8 or 2.
    Integer(u32),
    /// An exact literal with a decimal point, an exponent or `M`.
    Exact,
    /// An approximate literal (`E` without `M`, or `F`/`D`).
    Approximate,
}
