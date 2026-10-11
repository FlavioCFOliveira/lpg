//! Value types and graph type specifications (`GQL.g4` 18).

use super::{Expected, Fail, PResult, Parser};
use crate::query::ast::{
    EdgeDirection, EdgeTypeSpec, ElementTypeSpec, FieldType, IntegerWidth, LabelSetSpec,
    ListTypeName, NestedGraphType, NodeTypeRef, NodeTypeSpec, NumericKind, StringTypeKind,
    TemporalKind, TypeFiller, UnsignedInteger, ValueType,
};
use crate::query::lexer::{Keyword as K, TokenKind as T};
use crate::query::span::{OptSpanned, Spanned, VecSpanned};

impl Parser<'_> {
    /// `valueType`, a left-recursive rule of 10 alternatives: a Pratt
    /// loop over ANTLR's precedences, with the list suffix
    /// (`listValueTypeAlt2`, precedence 7) and the union `|`
    /// (`closedDynamicUnionTypeAtl2`, precedence 1).
    pub(super) fn value_type(&mut self) -> PResult<Spanned<ValueType>> {
        self.value_type_bp(0)
    }

    fn value_type_bp(&mut self, min: u8) -> PResult<Spanned<ValueType>> {
        let _guard = self.nest()?;
        let start = self.start();
        let mut left = self.value_type_primary()?;
        let mut chain = Vec::new();
        loop {
            if (self.at_kw(K::List) || self.at_kw(K::Array)) && 7 >= min {
                chain.push(self.nest()?);
                // valueType listValueTypeName ([maxLength])? notNull?
                let name = self.list_value_type_name();
                let max_length = self.opt_bracketed_max_length()?;
                let not_null = self.not_null()?;
                let element = Some(Box::new(self.spanned(start, left)));
                left = ValueType::List {
                    element,
                    name,
                    max_length,
                    not_null,
                };
            } else if self.at(T::VerticalBar) && 1 >= min {
                chain.push(self.nest()?);
                self.bump();
                let right = self.value_type_bp(2)?;
                let l = self.spanned(start, left);
                left = ValueType::Union(vec![l, right]);
            } else {
                return Ok(self.spanned(start, left));
            }
        }
    }

    /// `listValueTypeName: listValueTypeNameSynonym`: `LIST | ARRAY`.
    fn list_value_type_name(&mut self) -> ListTypeName {
        if self.eat_kw(K::Array) {
            ListTypeName::Array
        } else {
            self.bump();
            ListTypeName::List
        }
    }

    /// `(LEFT_BRACKET maxLength RIGHT_BRACKET)?`.
    fn opt_bracketed_max_length(&mut self) -> PResult<OptSpanned<UnsignedInteger>> {
        if self.eat(T::LeftBracket) {
            let n = self.unsigned_integer()?;
            self.expect(T::RightBracket)?;
            Ok(Some(n))
        } else {
            Ok(None)
        }
    }

    /// `notNull: NOT NULL`, optional.
    pub(super) fn not_null(&mut self) -> PResult<bool> {
        if self.eat_kw(K::Not) {
            self.expect_kw(K::Null)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// The non-left-recursive alternatives of `valueType`:
    /// `predefinedType`, `pathValueType`, the list forms 1 and 3,
    /// `recordType`, and the dynamic union types.
    fn value_type_primary(&mut self) -> PResult<ValueType> {
        match (self.peek(), self.peek_nth(1)) {
            (T::Keyword(K::Any), _) => self.any_value_type(),
            (T::Keyword(K::Property), T::Keyword(K::Value)) => {
                self.pos += 2;
                // dynamicPropertyValueType: ANY? PROPERTY VALUE notNull?
                Ok(ValueType::PropertyValue {
                    not_null: self.not_null()?,
                })
            }
            (T::Keyword(K::Path), _) => {
                // pathValueType: PATH notNull?
                self.bump();
                Ok(ValueType::Path {
                    not_null: self.not_null()?,
                })
            }
            (T::Keyword(K::List | K::Array), _) => {
                let name = self.list_value_type_name();
                let element = if self.at_type_angle() {
                    self.bump();
                    let t = self.value_type()?;
                    self.expect(T::RightAngleBracket)?;
                    Some(Box::new(t))
                } else {
                    None
                };
                let max_length = self.opt_bracketed_max_length()?;
                let not_null = self.not_null()?;
                Ok(ValueType::List {
                    element,
                    name,
                    max_length,
                    not_null,
                })
            }
            (T::Keyword(K::Record), _) | (T::LeftBrace, _) => self.record_type(),
            _ => self.predefined_type(),
        }
    }

    /// The `valueType` forms that start with `ANY`: `ANY VALUE? notNull?`,
    /// `ANY VALUE? < valueType (| valueType)* >`, `ANY PROPERTY VALUE`,
    /// `ANY PROPERTY? GRAPH`, `ANY NODE`, `ANY EDGE`, `ANY RECORD`.
    fn any_value_type(&mut self) -> PResult<ValueType> {
        self.expect_kw(K::Any)?;
        match (self.peek(), self.peek_nth(1)) {
            (T::Keyword(K::Property), T::Keyword(K::Value)) => {
                self.pos += 2;
                Ok(ValueType::PropertyValue {
                    not_null: self.not_null()?,
                })
            }
            (T::Keyword(K::Property), T::Keyword(K::Graph)) | (T::Keyword(K::Graph), _) => {
                // openGraphReferenceValueType: ANY PROPERTY? GRAPH notNull?
                self.eat_kw(K::Property);
                self.bump();
                Ok(ValueType::AnyGraph {
                    not_null: self.not_null()?,
                })
            }
            (T::Keyword(K::Node | K::Vertex), _) => {
                self.bump();
                Ok(ValueType::AnyNode {
                    not_null: self.not_null()?,
                })
            }
            (T::Keyword(K::Edge | K::Relationship), _) => {
                self.bump();
                Ok(ValueType::AnyEdge {
                    not_null: self.not_null()?,
                })
            }
            (T::Keyword(K::Record), _) => {
                self.bump();
                Ok(ValueType::Record {
                    fields: None,
                    not_null: self.not_null()?,
                })
            }
            _ => {
                self.eat_kw(K::Value);
                if self.at_type_angle() {
                    self.bump();
                    // closedDynamicUnionTypeAtl1.
                    let mut members = vec![self.value_type()?];
                    while self.eat(T::VerticalBar) {
                        members.push(self.value_type()?);
                    }
                    self.expect(T::RightAngleBracket)?;
                    return Ok(ValueType::Union(members));
                }
                Ok(ValueType::AnyValue {
                    not_null: self.not_null()?,
                })
            }
        }
    }

    /// `recordType: ANY? RECORD notNull? | RECORD? fieldTypesSpecification
    /// notNull?`.
    fn record_type(&mut self) -> PResult<ValueType> {
        let record = self.eat_kw(K::Record);
        if record && !self.at(T::LeftBrace) {
            return Ok(ValueType::Record {
                fields: None,
                not_null: self.not_null()?,
            });
        }
        let fields = self.field_types_specification()?;
        Ok(ValueType::Record {
            fields: Some(fields),
            not_null: self.not_null()?,
        })
    }

    /// `fieldTypesSpecification: { fieldTypeList? }`; `fieldType:
    /// fieldName typed? valueType`. Also `propertyTypesSpecification`
    /// (`propertyType: propertyName typed? propertyValueType`).
    fn field_types_specification(&mut self) -> PResult<VecSpanned<FieldType>> {
        self.expect(T::LeftBrace)?;
        let mut fields = Vec::new();
        if !self.eat(T::RightBrace) {
            loop {
                let start = self.start();
                let name = self.identifier()?;
                self.typed();
                let value_type = self.value_type()?;
                fields.push(self.spanned(start, FieldType { name, value_type }));
                if !self.eat(T::Comma) {
                    break;
                }
            }
            self.expect(T::RightBrace)?;
        }
        Ok(fields)
    }

    /// `predefinedType: booleanType | characterStringType |
    /// byteStringType | numericType | temporalType | referenceValueType |
    /// immaterialValueType`.
    fn predefined_type(&mut self) -> PResult<ValueType> {
        let k = match self.peek() {
            T::Keyword(k) => k,
            T::LeftParen => {
                // referenceValueType: a closed node or edge type pattern.
                return self.closed_element_reference_type();
            }
            _ => return self.fail_rule("a value type"),
        };
        match k {
            K::Bool | K::Boolean => {
                self.bump();
                Ok(ValueType::Bool {
                    not_null: self.not_null()?,
                })
            }
            K::String | K::Char | K::Varchar => self.string_type(false),
            K::Bytes | K::Binary | K::Varbinary => self.string_type(true),
            K::Null => {
                // immaterialValueType: nullType | emptyType.
                self.bump();
                if self.at_kw(K::Not) && self.peek_nth(1) == T::Keyword(K::Null) {
                    self.pos += 2;
                    return Ok(ValueType::Empty);
                }
                Ok(ValueType::Null)
            }
            K::Nothing => {
                self.bump();
                Ok(ValueType::Empty)
            }
            K::Zoned | K::Timestamp | K::Local | K::Date | K::Time | K::Duration => {
                self.temporal_type()
            }
            K::Property | K::Graph => {
                // closedGraphReferenceValueType: PROPERTY? GRAPH
                // nestedGraphTypeSpecification notNull?
                self.eat_kw(K::Property);
                self.expect_kw(K::Graph)?;
                let graph_type = self.nested_graph_type_specification()?;
                Ok(ValueType::Graph {
                    graph_type,
                    not_null: self.not_null()?,
                })
            }
            K::Binding | K::Table => self.binding_table_type_value(),
            K::Node | K::Vertex => {
                if self.starts_node_type_after_synonym() {
                    let start = self.start();
                    let n = self.node_type_specification()?;
                    return Ok(ValueType::Node {
                        node_type: Box::new(self.spanned(start, n)),
                        not_null: self.not_null()?,
                    });
                }
                self.bump();
                Ok(ValueType::AnyNode {
                    not_null: self.not_null()?,
                })
            }
            K::Edge | K::Relationship | K::Directed | K::Undirected => {
                if matches!(k, K::Directed | K::Undirected) || self.starts_edge_type_after_synonym()
                {
                    let start = self.start();
                    let e = self.edge_type_specification()?;
                    return Ok(ValueType::Edge {
                        edge_type: Box::new(self.spanned(start, e)),
                        not_null: self.not_null()?,
                    });
                }
                self.bump();
                Ok(ValueType::AnyEdge {
                    not_null: self.not_null()?,
                })
            }
            _ => self.numeric_type(),
        }
    }

    /// `bindingTableReferenceValueType: bindingTableType notNull?`;
    /// `bindingTableType: BINDING? TABLE fieldTypesSpecification`.
    fn binding_table_type_value(&mut self) -> PResult<ValueType> {
        self.eat_kw(K::Binding);
        self.expect_kw(K::Table)?;
        let fields = self.field_types_specification()?;
        Ok(ValueType::BindingTable {
            fields,
            not_null: self.not_null()?,
        })
    }

    /// `graphReferenceValueType: openGraphReferenceValueType |
    /// closedGraphReferenceValueType`.
    pub(super) fn graph_reference_value_type(&mut self) -> PResult<Spanned<ValueType>> {
        let start = self.start();
        let t = if self.at_kw(K::Any) {
            self.bump();
            if self.at_kw(K::Property) && self.peek_nth(1) == T::Keyword(K::Graph) {
                self.bump();
            }
            self.expect_kw(K::Graph)?;
            ValueType::AnyGraph {
                not_null: self.not_null()?,
            }
        } else {
            self.eat_kw(K::Property);
            self.expect_kw(K::Graph)?;
            let graph_type = self.nested_graph_type_specification()?;
            ValueType::Graph {
                graph_type,
                not_null: self.not_null()?,
            }
        };
        Ok(self.spanned(start, t))
    }

    /// `bindingTableReferenceValueType`.
    pub(super) fn binding_table_reference_value_type(&mut self) -> PResult<Spanned<ValueType>> {
        let start = self.start();
        let t = self.binding_table_type_value()?;
        Ok(self.spanned(start, t))
    }

    /// `characterStringType` and `byteStringType`: `STRING|BYTES ((min,)?
    /// max)?`, `CHAR|BINARY (fixed)?`, `VARCHAR|VARBINARY (max)?`, then
    /// `notNull?`.
    fn string_type(&mut self, bytes: bool) -> PResult<ValueType> {
        let kind = match self.bump().kind {
            T::Keyword(K::String | K::Bytes) => StringTypeKind::Variable,
            T::Keyword(K::Char | K::Binary) => StringTypeKind::Fixed,
            _ => StringTypeKind::VariableMax,
        };
        let mut min_length = None;
        let mut max_length = None;
        if self.eat(T::LeftParen) {
            let n = self.unsigned_integer()?;
            if kind == StringTypeKind::Variable && self.eat(T::Comma) {
                min_length = Some(n);
                max_length = Some(self.unsigned_integer()?);
            } else {
                max_length = Some(n);
            }
            self.expect(T::RightParen)?;
        }
        let not_null = self.not_null()?;
        Ok(if bytes {
            ValueType::ByteString {
                kind,
                min_length,
                max_length,
                not_null,
            }
        } else {
            ValueType::CharacterString {
                kind,
                min_length,
                max_length,
                not_null,
            }
        })
    }

    /// `numericType: exactNumericType | approximateNumericType`.
    fn numeric_type(&mut self) -> PResult<ValueType> {
        let T::Keyword(k) = self.peek() else {
            return self.fail_rule("a value type");
        };
        let bits = |n: u16| IntegerWidth::Bits(n);
        // (signed, width) of the binary exact numeric types.
        let integer = match k {
            K::Int8 => Some((true, bits(8))),
            K::Int16 => Some((true, bits(16))),
            K::Int32 => Some((true, bits(32))),
            K::Int64 => Some((true, bits(64))),
            K::Int128 => Some((true, bits(128))),
            K::Int256 => Some((true, bits(256))),
            K::Smallint => Some((true, IntegerWidth::Small)),
            K::Int => Some((true, IntegerWidth::Regular)),
            K::Bigint => Some((true, IntegerWidth::Big)),
            K::Uint8 => Some((false, bits(8))),
            K::Uint16 => Some((false, bits(16))),
            K::Uint32 => Some((false, bits(32))),
            K::Uint64 => Some((false, bits(64))),
            K::Uint128 => Some((false, bits(128))),
            K::Uint256 => Some((false, bits(256))),
            K::Usmallint => Some((false, IntegerWidth::Small)),
            K::Uint => Some((false, IntegerWidth::Regular)),
            K::Ubigint => Some((false, IntegerWidth::Big)),
            _ => None,
        };
        if let Some((signed, width)) = integer {
            self.bump();
            let precision = if width == IntegerWidth::Regular {
                self.opt_parenthesized_precision()?
            } else {
                None
            };
            return Ok(ValueType::Numeric {
                kind: NumericKind::Integer { signed, width },
                precision,
                scale: None,
                not_null: self.not_null()?,
            });
        }
        match k {
            K::Signed | K::Unsigned => {
                let signed = k == K::Signed;
                self.bump();
                self.verbose_binary_exact_numeric_type(signed)
            }
            K::Integer8
            | K::Integer16
            | K::Integer32
            | K::Integer64
            | K::Integer128
            | K::Integer256
            | K::Small
            | K::Integer
            | K::Big => self.verbose_binary_exact_numeric_type(true),
            K::Decimal | K::Dec => {
                // decimalExactNumericType: (DECIMAL | DEC) (( precision (,
                // scale)? ) notNull?)?
                self.bump();
                let (mut precision, mut scale, mut not_null) = (None, None, false);
                if self.eat(T::LeftParen) {
                    precision = Some(self.unsigned_decimal_integer()?);
                    if self.eat(T::Comma) {
                        scale = Some(self.unsigned_decimal_integer()?);
                    }
                    self.expect(T::RightParen)?;
                    not_null = self.not_null()?;
                }
                Ok(ValueType::Numeric {
                    kind: NumericKind::Decimal,
                    precision,
                    scale,
                    not_null,
                })
            }
            K::Float16 | K::Float32 | K::Float64 | K::Float128 | K::Float256 => {
                self.bump();
                let width = match k {
                    K::Float16 => 16,
                    K::Float32 => 32,
                    K::Float64 => 64,
                    K::Float128 => 128,
                    _ => 256,
                };
                Ok(ValueType::Numeric {
                    kind: NumericKind::FloatBits(width),
                    precision: None,
                    scale: None,
                    not_null: self.not_null()?,
                })
            }
            K::Float => {
                self.bump();
                let (mut precision, mut scale) = (None, None);
                if self.eat(T::LeftParen) {
                    precision = Some(self.unsigned_decimal_integer()?);
                    if self.eat(T::Comma) {
                        scale = Some(self.unsigned_decimal_integer()?);
                    }
                    self.expect(T::RightParen)?;
                }
                Ok(ValueType::Numeric {
                    kind: NumericKind::Float,
                    precision,
                    scale,
                    not_null: self.not_null()?,
                })
            }
            K::Real | K::Double => {
                self.bump();
                let kind = if k == K::Real {
                    NumericKind::Real
                } else {
                    self.eat_kw(K::Precision);
                    NumericKind::Double
                };
                Ok(ValueType::Numeric {
                    kind,
                    precision: None,
                    scale: None,
                    not_null: self.not_null()?,
                })
            }
            _ => self.fail_rule("a value type"),
        }
    }

    /// `(LEFT_PAREN precision RIGHT_PAREN)?`.
    fn opt_parenthesized_precision(&mut self) -> PResult<OptSpanned<UnsignedInteger>> {
        if self.eat(T::LeftParen) {
            let p = self.unsigned_decimal_integer()?;
            self.expect(T::RightParen)?;
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }

    /// `verboseBinaryExactNumericType: INTEGER8 … INTEGER256 | SMALL
    /// INTEGER | INTEGER (precision)? | BIG INTEGER`, then `notNull?`.
    fn verbose_binary_exact_numeric_type(&mut self, signed: bool) -> PResult<ValueType> {
        let width = match self.peek() {
            T::Keyword(K::Integer8) => IntegerWidth::Bits(8),
            T::Keyword(K::Integer16) => IntegerWidth::Bits(16),
            T::Keyword(K::Integer32) => IntegerWidth::Bits(32),
            T::Keyword(K::Integer64) => IntegerWidth::Bits(64),
            T::Keyword(K::Integer128) => IntegerWidth::Bits(128),
            T::Keyword(K::Integer256) => IntegerWidth::Bits(256),
            T::Keyword(K::Small) => {
                self.bump();
                IntegerWidth::Small
            }
            T::Keyword(K::Big) => {
                self.bump();
                IntegerWidth::Big
            }
            T::Keyword(K::Integer) => IntegerWidth::Regular,
            _ => return self.fail(Expected::Token(T::Keyword(K::Integer))),
        };
        let precision = match width {
            IntegerWidth::Bits(_) => {
                self.bump();
                None
            }
            IntegerWidth::Small | IntegerWidth::Big => {
                self.expect_kw(K::Integer)?;
                None
            }
            IntegerWidth::Regular => {
                self.bump();
                self.opt_parenthesized_precision()?
            }
        };
        Ok(ValueType::Numeric {
            kind: NumericKind::Integer { signed, width },
            precision,
            scale: None,
            not_null: self.not_null()?,
        })
    }

    /// `temporalType: temporalInstantType | temporalDurationType`.
    fn temporal_type(&mut self) -> PResult<ValueType> {
        let kind = match self.bump().kind {
            T::Keyword(K::Zoned) => {
                if self.eat_kw(K::Datetime) {
                    TemporalKind::ZonedDatetime
                } else {
                    self.expect_kw(K::Time)?;
                    TemporalKind::ZonedTime
                }
            }
            T::Keyword(K::Local) => {
                if self.eat_kw(K::Datetime) {
                    TemporalKind::LocalDatetime
                } else {
                    self.expect_kw(K::Time)?;
                    TemporalKind::LocalTime
                }
            }
            T::Keyword(K::Timestamp) => {
                if self.eat_kw(K::With) {
                    self.expect_kw(K::Time)?;
                    self.expect_kw(K::Zone)?;
                    TemporalKind::ZonedDatetime
                } else if self.at_kw(K::Without) {
                    self.bump();
                    self.expect_kw(K::Time)?;
                    self.expect_kw(K::Zone)?;
                    TemporalKind::LocalDatetime
                } else {
                    TemporalKind::LocalDatetime
                }
            }
            T::Keyword(K::Date) => TemporalKind::Date,
            T::Keyword(K::Time) => {
                let zoned = if self.eat_kw(K::With) {
                    true
                } else {
                    self.expect_kw(K::Without)?;
                    false
                };
                self.expect_kw(K::Time)?;
                self.expect_kw(K::Zone)?;
                if zoned {
                    TemporalKind::ZonedTime
                } else {
                    TemporalKind::LocalTime
                }
            }
            _ => {
                // temporalDurationType: DURATION ( temporalDurationQualifier ).
                self.expect(T::LeftParen)?;
                let year_to_month = self.temporal_duration_qualifier()?;
                self.expect(T::RightParen)?;
                if year_to_month {
                    TemporalKind::DurationYearToMonth
                } else {
                    TemporalKind::DurationDayToSecond
                }
            }
        };
        Ok(ValueType::Temporal {
            kind,
            not_null: self.not_null()?,
        })
    }

    /// `temporalDurationQualifier: YEAR TO MONTH | DAY TO SECOND`:
    /// whether it is `YEAR TO MONTH`.
    pub(super) fn temporal_duration_qualifier(&mut self) -> PResult<bool> {
        if self.eat_kw(K::Year) {
            self.expect_kw(K::To)?;
            self.expect_kw(K::Month)?;
            Ok(true)
        } else {
            self.expect_kw(K::Day)?;
            self.expect_kw(K::To)?;
            self.expect_kw(K::Second)?;
            Ok(false)
        }
    }

    /// A closed node or edge reference type written as a pattern,
    /// starting with `(`.
    fn closed_element_reference_type(&mut self) -> PResult<ValueType> {
        let start = self.start();
        match self.element_type_specification()? {
            ElementTypeSpec::Node(n) => Ok(ValueType::Node {
                node_type: Box::new(self.spanned(start, *n)),
                not_null: self.not_null()?,
            }),
            ElementTypeSpec::Edge(e) => Ok(ValueType::Edge {
                edge_type: Box::new(self.spanned(start, *e)),
                not_null: self.not_null()?,
            }),
        }
    }

    /// Whether `NODE`/`VERTEX` at the current token starts a closed node
    /// type (`nodeTypeSpecification`) rather than the open `NODE`: a
    /// name, `TYPE`, or a filler follows.
    ///
    /// The closed type is tried first; a failed parse retries the open
    /// one ([`super::Site::ELEMENT_TYPE`]).
    fn starts_node_type_after_synonym(&mut self) -> bool {
        let k = self.peek_nth(1);
        (Self::is_identifier(k) || Self::starts_type_filler(k))
            && self.decide(super::Site::ELEMENT_TYPE, 2) == 0
    }

    /// Whether `EDGE`/`RELATIONSHIP` starts a closed edge type: a name
    /// follows (a closed edge type pattern needs one after the synonym).
    fn starts_edge_type_after_synonym(&mut self) -> bool {
        Self::is_identifier(self.peek_nth(1)) && self.decide(super::Site::ELEMENT_TYPE, 2) == 0
    }

    /// Whether `k` starts a `nodeTypeFiller` or `edgeTypeFiller`.
    fn starts_type_filler(k: T) -> bool {
        matches!(
            k,
            T::Keyword(K::Is | K::Label | K::Labels) | T::Colon | T::Implies | T::LeftBrace
        )
    }

    /// `nestedGraphTypeSpecification: { graphTypeSpecificationBody }`;
    /// `elementTypeList: elementTypeSpecification (COMMA
    /// elementTypeSpecification)*`.
    pub(super) fn nested_graph_type_specification(&mut self) -> PResult<Spanned<NestedGraphType>> {
        let start = self.start();
        self.expect(T::LeftBrace)?;
        let mut list = Vec::new();
        loop {
            let s = self.start();
            let e = self.element_type_specification()?;
            list.push(self.spanned(s, e));
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RightBrace)?;
        Ok(self.spanned(start, list))
    }

    /// `elementTypeSpecification: nodeTypeSpecification |
    /// edgeTypeSpecification`. A first `( … )` is a node type pattern
    /// unless an arc follows it.
    fn element_type_specification(&mut self) -> PResult<ElementTypeSpec> {
        let _guard = self.nest()?;
        match self.peek() {
            T::LeftParen => {
                let start = self.start();
                let (alias, filler) = self.parenthesized_node_type()?;
                if matches!(
                    self.peek(),
                    T::MinusLeftBracket | T::LeftArrowBracket | T::TildeLeftBracket
                ) {
                    let first = self.node_type_reference(alias, filler)?;
                    let first = self.spanned(start, first);
                    return Ok(ElementTypeSpec::Edge(Box::new(
                        self.edge_type_pattern_rest(None, None, first)?,
                    )));
                }
                Ok(ElementTypeSpec::Node(Box::new(NodeTypeSpec {
                    name: None,
                    alias,
                    filler,
                    phrase: false,
                })))
            }
            T::Keyword(K::Node | K::Vertex) => Ok(ElementTypeSpec::Node(Box::new(
                self.node_type_specification()?,
            ))),
            _ => Ok(ElementTypeSpec::Edge(Box::new(
                self.edge_type_specification()?,
            ))),
        }
    }

    /// `( localNodeTypeAlias? nodeTypeFiller? )`. `LABEL` and `LABELS`
    /// are non-reserved: followed by a name they start a filler.
    fn parenthesized_node_type(
        &mut self,
    ) -> PResult<(OptSpanned<crate::query::ast::Ident>, OptSpanned<TypeFiller>)> {
        self.expect(T::LeftParen)?;
        let label_phrase = self.at_label_phrase();
        let alias = if self.at_regular_identifier() && !label_phrase {
            Some(self.regular_identifier()?)
        } else {
            None
        };
        let filler = self.opt_type_filler()?;
        self.expect(T::RightParen)?;
        Ok((alias, filler))
    }

    /// `sourceNodeTypeReference` / `destinationNodeTypeReference`: `(
    /// alias )` or `( nodeTypeFiller? )`, not both.
    fn node_type_reference(
        &mut self,
        alias: OptSpanned<crate::query::ast::Ident>,
        filler: OptSpanned<TypeFiller>,
    ) -> PResult<NodeTypeRef> {
        match (alias, filler) {
            (Some(a), None) => Ok(NodeTypeRef::Alias(a.0)),
            (None, f) => Ok(NodeTypeRef::Filler(f)),
            (Some(_), Some(_)) => self.fail_rule("a node type alias or filler, not both"),
        }
    }

    /// `nodeTypeSpecification: nodeTypePattern | nodeTypePhrase`;
    /// `nodeTypePattern: (nodeSynonym TYPE? nodeTypeName)? ( … )`;
    /// `nodeTypePhrase: nodeSynonym TYPE? nodeTypePhraseFiller (AS
    /// localNodeTypeAlias)?`.
    fn node_type_specification(&mut self) -> PResult<NodeTypeSpec> {
        if self.at(T::LeftParen) {
            let (alias, filler) = self.parenthesized_node_type()?;
            return Ok(NodeTypeSpec {
                name: None,
                alias,
                filler,
                phrase: false,
            });
        }
        if !(self.eat_kw(K::Node) || self.eat_kw(K::Vertex)) {
            return Err(Fail);
        }
        self.type_keyword();
        // nodeTypePhraseFiller: nodeTypeName nodeTypeFiller? |
        // nodeTypeFiller. LABEL/LABELS then a name start a filler.
        let label_phrase = self.at_label_phrase();
        let name = if self.at_identifier() && !label_phrase {
            Some(self.identifier()?)
        } else {
            None
        };
        if name.is_some() && self.at(T::LeftParen) {
            let (alias, filler) = self.parenthesized_node_type()?;
            return Ok(NodeTypeSpec {
                name,
                alias,
                filler,
                phrase: false,
            });
        }
        let filler = self.opt_type_filler()?;
        if name.is_none() && filler.is_none() {
            return self.fail_rule("a node type name or filler");
        }
        let alias = if self.eat_kw(K::As) {
            Some(self.regular_identifier()?)
        } else {
            None
        };
        Ok(NodeTypeSpec {
            name,
            alias,
            filler,
            phrase: true,
        })
    }

    /// Whether `LABEL`/`LABELS` and a name start a label set phrase here
    /// rather than a name (both are possible: `LABEL` is non-reserved).
    /// The phrase is tried first; a failed parse retries the name
    /// ([`super::Site::LABEL_PHRASE`]).
    fn at_label_phrase(&mut self) -> bool {
        matches!(self.peek(), T::Keyword(K::Label | K::Labels))
            && Self::is_identifier(self.peek_nth(1))
            && self.decide(super::Site::LABEL_PHRASE, 2) == 0
    }

    /// Whether a `<` here opens the type's own brackets: tried first; a
    /// failed parse retries reading the type without them, the `<` then
    /// being a comparison ([`super::Site::TYPE_ANGLE`]).
    fn at_type_angle(&mut self) -> bool {
        self.at(T::LeftAngleBracket) && self.decide(super::Site::TYPE_ANGLE, 2) == 0
    }

    /// `TYPE?` after a node or edge synonym: the keyword when what
    /// follows can still be a name or a filler; otherwise `TYPE` is the
    /// name itself (it is non-reserved).
    ///
    /// When both readings are possible the keyword is taken first, as the
    /// oracle's `TYPE?` decision does; a failed parse retries the name
    /// ([`super::Site::TYPE_KEYWORD`]).
    fn type_keyword(&mut self) {
        if self.peek() == T::Keyword(K::Type) {
            let next = self.peek_nth(1);
            if (Self::is_identifier(next) || Self::starts_type_filler(next))
                && self.decide(super::Site::TYPE_KEYWORD, 2) == 0
            {
                self.bump();
            }
        }
    }

    /// `nodeTypeFiller` and `edgeTypeFiller`, optional:
    /// `keyLabelSet impliedContent? | impliedContent`, where `keyLabelSet:
    /// labelSetPhrase? IMPLIES` and `impliedContent: labelSet |
    /// propertyTypes | labelSet propertyTypes`.
    fn opt_type_filler(&mut self) -> PResult<OptSpanned<TypeFiller>> {
        let start = self.start();
        let first = self.opt_label_set_phrase()?;
        let (key_labels, labels) = if self.eat(T::Implies) {
            (Some(first), self.opt_label_set_phrase()?)
        } else {
            (None, first)
        };
        let properties = if self.at(T::LeftBrace) {
            Some(self.field_types_specification()?)
        } else {
            None
        };
        if key_labels.is_none() && labels.is_none() && properties.is_none() {
            return Ok(None);
        }
        Ok(Some(self.spanned(
            start,
            TypeFiller {
                key_labels,
                labels,
                properties,
            },
        )))
    }

    /// `labelSetPhrase: LABEL labelName | LABELS labelSetSpecification |
    /// isOrColon labelSetSpecification`, optional.
    fn opt_label_set_phrase(&mut self) -> PResult<Option<Spanned<LabelSetSpec>>> {
        let start = self.start();
        let labels = match self.peek() {
            T::Keyword(K::Label) if Self::is_identifier(self.peek_nth(1)) => {
                self.bump();
                vec![self.identifier()?]
            }
            T::Keyword(K::Labels) if Self::is_identifier(self.peek_nth(1)) => {
                self.bump();
                self.label_set_specification()?
            }
            T::Keyword(K::Is) | T::Colon => {
                self.bump();
                self.label_set_specification()?
            }
            _ => {
                self.at(T::Colon);
                self.at_kw(K::Label);
                return Ok(None);
            }
        };
        Ok(Some(self.spanned(start, labels)))
    }

    /// `edgeTypeSpecification: edgeTypePattern | edgeTypePhrase`;
    /// `edgeTypePattern: (edgeKind? edgeSynonym TYPE? edgeTypeName)?
    /// (edgeTypePatternDirected | edgeTypePatternUndirected)`;
    /// `edgeTypePhrase: edgeKind edgeSynonym TYPE? edgeTypePhraseFiller
    /// endpointPairPhrase`.
    fn edge_type_specification(&mut self) -> PResult<EdgeTypeSpec> {
        let directed = if self.eat_kw(K::Directed) {
            Some(true)
        } else if self.eat_kw(K::Undirected) {
            Some(false)
        } else {
            None
        };
        if self.at(T::LeftParen) && directed.is_none() {
            let start = self.start();
            let (alias, filler) = self.parenthesized_node_type()?;
            let first = self.node_type_reference(alias, filler)?;
            let first = self.spanned(start, first);
            return self.edge_type_pattern_rest(None, None, first);
        }
        if !(self.eat_kw(K::Edge) || self.eat_kw(K::Relationship)) {
            return Err(Fail);
        }
        self.type_keyword();
        let label_phrase = self.at_label_phrase();
        let name = if self.at_identifier() && !label_phrase {
            Some(self.identifier()?)
        } else {
            None
        };
        if self.at(T::LeftParen) {
            let Some(name) = name else {
                return Err(Fail);
            };
            let start = self.start();
            let (alias, filler) = self.parenthesized_node_type()?;
            let first = self.node_type_reference(alias, filler)?;
            let first = self.spanned(start, first);
            let mut e = self.edge_type_pattern_rest(Some(name), directed, first)?;
            e.directed = directed;
            return Ok(e);
        }
        // edgeTypePhrase: the edge kind is mandatory.
        if directed.is_none() {
            return self.fail(Expected::Token(T::LeftParen));
        }
        let filler = self.opt_type_filler()?;
        if name.is_none() && filler.is_none() {
            return self.fail_rule("an edge type name or filler");
        }
        // endpointPairPhrase: CONNECTING endpointPair.
        self.expect_kw(K::Connecting)?;
        let (direction, source, destination) = self.endpoint_pair()?;
        Ok(EdgeTypeSpec {
            directed,
            name,
            filler,
            direction,
            source,
            destination,
            phrase: true,
        })
    }

    /// The arc and destination of an edge type pattern after its first
    /// node type reference: `-[ filler ]->`, `<-[ filler ]-` or `~[
    /// filler ]~`, then the other node type reference.
    fn edge_type_pattern_rest(
        &mut self,
        name: OptSpanned<crate::query::ast::Ident>,
        directed: Option<bool>,
        first: Spanned<NodeTypeRef>,
    ) -> PResult<EdgeTypeSpec> {
        let (direction, close) = if self.eat(T::MinusLeftBracket) {
            (EdgeDirection::Right, T::BracketRightArrow)
        } else if self.eat(T::LeftArrowBracket) {
            (EdgeDirection::Left, T::RightBracketMinus)
        } else {
            self.expect(T::TildeLeftBracket)?;
            (EdgeDirection::Undirected, T::RightBracketTilde)
        };
        let filler = self.opt_type_filler()?;
        if filler.is_none() {
            return self.fail_rule("an edge type filler");
        }
        self.expect(close)?;
        let start = self.start();
        let (alias, f) = self.parenthesized_node_type()?;
        let second = self.node_type_reference(alias, f)?;
        let second = self.spanned(start, second);
        // edgeTypePatternPointingLeft: destination <-[ … ]- source.
        let (source, destination) = if direction == EdgeDirection::Left {
            (second, first)
        } else {
            (first, second)
        };
        Ok(EdgeTypeSpec {
            directed,
            name,
            filler,
            direction,
            source,
            destination,
            phrase: false,
        })
    }

    /// `endpointPair: endpointPairDirected | endpointPairUndirected`:
    /// `( a TO|-> b )`, `( b <- a )`, `( a TO|~ b )`. `TO` is directed
    /// (TS-19 ambiguity `endpointPair`, alternative 1).
    fn endpoint_pair(
        &mut self,
    ) -> PResult<(EdgeDirection, Spanned<NodeTypeRef>, Spanned<NodeTypeRef>)> {
        self.expect(T::LeftParen)?;
        let a = self.regular_identifier()?;
        let direction = if self.eat(T::LeftArrow) {
            EdgeDirection::Left
        } else if self.eat(T::Tilde) {
            EdgeDirection::Undirected
        } else if self.eat(T::RightArrow) || self.eat_kw(K::To) {
            EdgeDirection::Right
        } else {
            return Err(Fail);
        };
        let b = self.regular_identifier()?;
        self.expect(T::RightParen)?;
        let alias = |s: Spanned<crate::query::ast::Ident>| Spanned(NodeTypeRef::Alias(s.0), s.1);
        Ok(if direction == EdgeDirection::Left {
            (direction, alias(b), alias(a))
        } else {
            (direction, alias(a), alias(b))
        })
    }
}
