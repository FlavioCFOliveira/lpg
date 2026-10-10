//! Value types and graph type specifications (`GQL.g4` 18).

use super::{Ident, LabelSetSpec, UnsignedInteger};
use crate::query::span::{BoxSpanned, OptSpanned, Spanned, VecSpanned};

/// `valueType`. `not_null` is `notNull`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValueType {
    /// `BOOL` or `BOOLEAN`.
    Bool {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `STRING [(min, max)]`, `CHAR [(n)]`, `VARCHAR [(max)]`.
    CharacterString {
        /// The spelling.
        kind: StringTypeKind,
        /// `minLength`.
        min_length: OptSpanned<UnsignedInteger>,
        /// `maxLength` or `fixedLength`.
        max_length: OptSpanned<UnsignedInteger>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `BYTES [(min, max)]`, `BINARY [(n)]`, `VARBINARY [(max)]`.
    ByteString {
        /// The spelling.
        kind: StringTypeKind,
        /// `minLength`.
        min_length: OptSpanned<UnsignedInteger>,
        /// `maxLength` or `fixedLength`.
        max_length: OptSpanned<UnsignedInteger>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `exactNumericType` and `approximateNumericType`.
    Numeric {
        /// The type.
        kind: NumericKind,
        /// `precision`.
        precision: OptSpanned<UnsignedInteger>,
        /// `scale`.
        scale: OptSpanned<UnsignedInteger>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `temporalType`.
    Temporal {
        /// The type.
        kind: TemporalKind,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `openGraphReferenceValueType`: `ANY [PROPERTY] GRAPH`.
    AnyGraph {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `closedGraphReferenceValueType`.
    Graph {
        /// `nestedGraphTypeSpecification`.
        graph_type: Spanned<NestedGraphType>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `bindingTableReferenceValueType`.
    BindingTable {
        /// `fieldTypesSpecification`.
        fields: VecSpanned<FieldType>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `openNodeReferenceValueType`: `[ANY] NODE`.
    AnyNode {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `closedNodeReferenceValueType`.
    Node {
        /// `nodeTypeSpecification`.
        node_type: BoxSpanned<NodeTypeSpec>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `openEdgeReferenceValueType`: `[ANY] EDGE`.
    AnyEdge {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `closedEdgeReferenceValueType`.
    Edge {
        /// `edgeTypeSpecification`.
        edge_type: BoxSpanned<EdgeTypeSpec>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `nullType`: `NULL`.
    Null,
    /// `emptyType`: `NULL NOT NULL` or `NOTHING`.
    Empty,
    /// `pathValueType`: `PATH`.
    Path {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// The three list type forms; `element` is absent in `LIST`.
    List {
        /// The element type.
        element: Option<BoxSpanned<ValueType>>,
        /// `LIST` or `ARRAY`.
        name: ListTypeName,
        /// `maxLength`.
        max_length: OptSpanned<UnsignedInteger>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `recordType`: `fields` is `None` for `[ANY] RECORD`.
    Record {
        /// `fieldTypesSpecification`.
        fields: Option<VecSpanned<FieldType>>,
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `openDynamicUnionType`: `ANY [VALUE]`.
    AnyValue {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `dynamicPropertyValueType`: `[ANY] PROPERTY VALUE`.
    PropertyValue {
        /// `NOT NULL`.
        not_null: bool,
    },
    /// `closedDynamicUnionType`: `ANY [VALUE] <A | B …>` or `A | B`.
    Union(VecSpanned<ValueType>),
}

/// How a string type is spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringTypeKind {
    /// `STRING` or `BYTES`: a length range.
    Variable,
    /// `CHAR` or `BINARY`: a fixed length.
    Fixed,
    /// `VARCHAR` or `VARBINARY`: a maximum length.
    VariableMax,
}

/// A numeric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericKind {
    /// A binary exact numeric type.
    Integer {
        /// `false` for the `U…` and `UNSIGNED` types.
        signed: bool,
        /// The width.
        width: IntegerWidth,
    },
    /// `DECIMAL` or `DEC`.
    Decimal,
    /// `FLOAT16` … `FLOAT256`: the width in bits.
    FloatBits(u16),
    /// `FLOAT [(precision [, scale])]`.
    Float,
    /// `REAL`.
    Real,
    /// `DOUBLE [PRECISION]`.
    Double,
}

/// The width of a binary exact numeric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegerWidth {
    /// `INT8`, `INTEGER8`, …: the width in bits.
    Bits(u16),
    /// `SMALLINT`, `SMALL INTEGER`, `USMALLINT`.
    Small,
    /// `INT`, `INTEGER`, `UINT` with an optional precision.
    Regular,
    /// `BIGINT`, `BIG INTEGER`, `UBIGINT`.
    Big,
}

/// A temporal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalKind {
    /// `ZONED DATETIME` or `TIMESTAMP WITH TIME ZONE`.
    ZonedDatetime,
    /// `LOCAL DATETIME` or `TIMESTAMP [WITHOUT TIME ZONE]`.
    LocalDatetime,
    /// `DATE`.
    Date,
    /// `ZONED TIME` or `TIME WITH TIME ZONE`.
    ZonedTime,
    /// `LOCAL TIME` or `TIME WITHOUT TIME ZONE`.
    LocalTime,
    /// `DURATION(YEAR TO MONTH)`.
    DurationYearToMonth,
    /// `DURATION(DAY TO SECOND)`.
    DurationDayToSecond,
}

/// `listValueTypeNameSynonym`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListTypeName {
    /// `LIST`.
    List,
    /// `ARRAY`.
    Array,
}

/// `fieldType` and `propertyType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldType {
    /// The field or property name.
    pub(crate) name: Spanned<Ident>,
    /// Its type.
    pub(crate) value_type: Spanned<ValueType>,
}

/// `nestedGraphTypeSpecification`: its `elementTypeList`.
pub(crate) type NestedGraphType = VecSpanned<ElementTypeSpec>;

/// `elementTypeSpecification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElementTypeSpec {
    /// `nodeTypeSpecification`.
    Node(Box<NodeTypeSpec>),
    /// `edgeTypeSpecification`.
    Edge(Box<EdgeTypeSpec>),
}

/// `nodeTypeSpecification`: the pattern and the phrase forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeTypeSpec {
    /// `nodeTypeName`.
    pub(crate) name: OptSpanned<Ident>,
    /// `localNodeTypeAlias`.
    pub(crate) alias: OptSpanned<Ident>,
    /// `nodeTypeFiller`.
    pub(crate) filler: OptSpanned<TypeFiller>,
    /// Whether it is written as a phrase (`NODE …`) rather than a pattern.
    pub(crate) phrase: bool,
}

/// `nodeTypeFiller` and `edgeTypeFiller`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeFiller {
    /// `…KeyLabelSet`: `Some` when `IMPLIES` (or `=>`) is written, with the
    /// key labels before it.
    pub(crate) key_labels: Option<Option<Spanned<LabelSetSpec>>>,
    /// `…LabelSet`.
    pub(crate) labels: Option<Spanned<LabelSetSpec>>,
    /// `…PropertyTypes`.
    pub(crate) properties: Option<VecSpanned<FieldType>>,
}

/// `edgeTypeSpecification`: the pattern and the phrase forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdgeTypeSpec {
    /// `edgeKind`: `DIRECTED` (`Some(true)`) or `UNDIRECTED`.
    pub(crate) directed: Option<bool>,
    /// `edgeTypeName`.
    pub(crate) name: OptSpanned<Ident>,
    /// `edgeTypeFiller`.
    pub(crate) filler: OptSpanned<TypeFiller>,
    /// `Right`, `Left` or `Undirected`.
    pub(crate) direction: super::EdgeDirection,
    /// The source node type.
    pub(crate) source: Spanned<NodeTypeRef>,
    /// The destination node type.
    pub(crate) destination: Spanned<NodeTypeRef>,
    /// Whether it is written as a phrase (`… CONNECTING (…)`).
    pub(crate) phrase: bool,
}

/// `sourceNodeTypeReference`, `destinationNodeTypeReference`, and the
/// aliases of an `endpointPair`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeTypeRef {
    /// `( alias )` or an endpoint alias.
    Alias(Ident),
    /// `( nodeTypeFiller? )`.
    Filler(OptSpanned<TypeFiller>),
}
