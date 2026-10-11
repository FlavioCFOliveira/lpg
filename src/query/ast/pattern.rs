//! Graph patterns, insert patterns and label expressions (`GQL.g4` 16).

use super::{Expr, Ident, NonNegativeInteger, UnsignedInteger};
use crate::query::span::{BoxSpanned, OptSpanned, Spanned, VecSpanned};

/// `graphPatternBindingTable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphPatternBindingTable {
    /// `graphPattern`.
    pub(crate) pattern: Spanned<GraphPattern>,
    /// `graphPatternYieldClause`: the yielded variables.
    pub(crate) yield_clause: VecSpanned<Ident>,
}

/// `graphPattern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphPattern {
    /// `matchMode`.
    pub(crate) match_mode: OptSpanned<MatchMode>,
    /// `pathPatternList`.
    pub(crate) patterns: VecSpanned<PathPattern>,
    /// `keepClause`.
    pub(crate) keep: OptSpanned<PathPatternPrefix>,
    /// `graphPatternWhereClause`.
    pub(crate) where_clause: OptSpanned<Expr>,
}

/// `matchMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchMode {
    /// `REPEATABLE ELEMENT [BINDINGS]` or `REPEATABLE ELEMENTS`.
    RepeatableElements,
    /// `DIFFERENT EDGE [BINDINGS]` or `DIFFERENT EDGES` (and the
    /// `RELATIONSHIP` synonyms).
    DifferentEdges,
}

/// `pathPattern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathPattern {
    /// `pathVariableDeclaration`.
    pub(crate) variable: OptSpanned<Ident>,
    /// `pathPatternPrefix`.
    pub(crate) prefix: OptSpanned<PathPatternPrefix>,
    /// `pathPatternExpression`.
    pub(crate) expr: Spanned<PathPatternExpr>,
}

/// `pathMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathMode {
    /// `WALK`.
    Walk,
    /// `TRAIL`.
    Trail,
    /// `SIMPLE`.
    Simple,
    /// `ACYCLIC`.
    Acyclic,
}

/// `pathSearchPrefix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathSearch {
    /// `allPathSearch`.
    All(OptSpanned<PathMode>),
    /// `anyPathSearch`.
    Any {
        /// `numberOfPaths`.
        number: OptSpanned<NonNegativeInteger>,
        /// `pathMode`.
        mode: OptSpanned<PathMode>,
    },
    /// `allShortestPathSearch`.
    AllShortest(OptSpanned<PathMode>),
    /// `anyShortestPathSearch`.
    AnyShortest(OptSpanned<PathMode>),
    /// `countedShortestPathSearch`.
    CountedShortest {
        /// `numberOfPaths`.
        number: Spanned<NonNegativeInteger>,
        /// `pathMode`.
        mode: OptSpanned<PathMode>,
    },
    /// `countedShortestGroupSearch`.
    CountedShortestGroup {
        /// `numberOfGroups`.
        number: OptSpanned<NonNegativeInteger>,
        /// `pathMode`.
        mode: OptSpanned<PathMode>,
    },
}

/// `pathPatternPrefix`. The optional `PATH`/`PATHS` word is not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathPatternPrefix {
    /// `pathModePrefix`.
    PathMode(PathMode),
    /// `pathSearchPrefix`.
    PathSearch(PathSearch),
}

/// `pathPatternExpression` and its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathPatternExpr {
    /// `pathTerm (| pathTerm)+`.
    Union(VecSpanned<PathPatternExpr>),
    /// `pathTerm (|+| pathTerm)+`.
    Alternation(VecSpanned<PathPatternExpr>),
    /// `pathTerm` of two or more factors.
    Concat(VecSpanned<PathPatternExpr>),
    /// `pathPrimary graphPatternQuantifier`.
    Quantified {
        /// The primary.
        path: BoxSpanned<PathPatternExpr>,
        /// The quantifier.
        quantifier: Spanned<PatternQuantifier>,
    },
    /// `pathPrimary ?`.
    Questioned(BoxSpanned<PathPatternExpr>),
    /// `parenthesizedPathPatternExpression`.
    Grouped(GroupedPathPattern),
    /// `elementPattern`.
    Element(ElementPattern),
    /// `simplifiedPathPatternExpression`.
    Simplified(SimplifiedPathPattern),
}

/// `parenthesizedPathPatternExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupedPathPattern {
    /// `subpathVariableDeclaration`.
    pub(crate) variable: OptSpanned<Ident>,
    /// `pathModePrefix`.
    pub(crate) mode: OptSpanned<PathMode>,
    /// `pathPatternExpression`.
    pub(crate) expr: BoxSpanned<PathPatternExpr>,
    /// `parenthesizedPathPatternWhereClause`.
    pub(crate) where_clause: OptSpanned<Expr>,
}

/// `graphPatternQuantifier`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternQuantifier {
    /// `*`.
    Asterisk,
    /// `+`.
    Plus,
    /// `fixedQuantifier`: `{n}`.
    Fixed(Spanned<UnsignedInteger>),
    /// `generalQuantifier`: `{m,n}` with either bound optional.
    General {
        /// `lowerBound`.
        lower_bound: OptSpanned<UnsignedInteger>,
        /// `upperBound`.
        upper_bound: OptSpanned<UnsignedInteger>,
    },
}

/// `elementPattern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElementPattern {
    /// `nodePattern`.
    Node(ElementPatternFiller),
    /// `edgePattern`; an abbreviated edge has an empty filler.
    Edge {
        /// The direction.
        direction: EdgeDirection,
        /// The filler.
        filler: ElementPatternFiller,
        /// Whether the edge is abbreviated (`->`, `-`, …).
        abbreviated: bool,
    },
}

/// The direction of an edge pattern, insert edge or simplified pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeDirection {
    /// `<-[]-`, `<-`.
    Left,
    /// `~[]~`, `~`.
    Undirected,
    /// `-[]->`, `->`.
    Right,
    /// `<~[]~`, `<~`.
    LeftOrUndirected,
    /// `~[]~>`, `~>`.
    UndirectedOrRight,
    /// `<-[]->`, `<->`.
    LeftOrRight,
    /// `-[]-`, `-`.
    AnyDirection,
}

/// `elementPatternFiller`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ElementPatternFiller {
    /// `elementVariableDeclaration`.
    pub(crate) variable: OptSpanned<Ident>,
    /// `isLabelExpression`.
    pub(crate) label: OptSpanned<LabelExpr>,
    /// `elementPatternPredicate`.
    pub(crate) predicate: OptSpanned<ElementPatternPredicate>,
}

/// `elementPatternPredicate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElementPatternPredicate {
    /// `elementPatternWhereClause`.
    Where(Spanned<Expr>),
    /// `elementPropertySpecification`.
    Property(VecSpanned<PropertyKeyValuePair>),
}

/// `propertyKeyValuePair`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PropertyKeyValuePair {
    /// `propertyName`.
    pub(crate) name: Spanned<Ident>,
    /// The value.
    pub(crate) value: Spanned<Expr>,
}

/// `labelExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LabelExpr {
    /// `labelExpression & labelExpression`.
    Conjunction(BoxSpanned<LabelExpr>, BoxSpanned<LabelExpr>),
    /// `labelExpression | labelExpression`.
    Disjunction(BoxSpanned<LabelExpr>, BoxSpanned<LabelExpr>),
    /// `!labelExpression`.
    Negation(BoxSpanned<LabelExpr>),
    /// `labelName`.
    Label(Ident),
    /// `%`.
    Wildcard,
    /// `( labelExpression )`.
    Parenthesized(BoxSpanned<LabelExpr>),
}

/// `labelSetSpecification`: `L1 & L2 & …`.
pub(crate) type LabelSetSpec = VecSpanned<Ident>;

/// `insertPathPattern`: a node, then edge and node pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InsertPathPattern {
    /// The first `insertNodePattern`.
    pub(crate) start: Spanned<InsertElement>,
    /// The `insertEdgePattern insertNodePattern` pairs.
    pub(crate) steps: Vec<(Spanned<InsertEdge>, Spanned<InsertElement>)>,
}

/// `insertEdgePattern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InsertEdge {
    /// `Left`, `Right` or `Undirected`.
    pub(crate) direction: EdgeDirection,
    /// The filler.
    pub(crate) filler: InsertElement,
}

/// `insertElementPatternFiller` (absent fields when the pattern is empty).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InsertElement {
    /// `elementVariableDeclaration`.
    pub(crate) variable: OptSpanned<Ident>,
    /// `labelSetSpecification`.
    pub(crate) labels: Option<Spanned<LabelSetSpec>>,
    /// `elementPropertySpecification`.
    pub(crate) properties: Option<VecSpanned<PropertyKeyValuePair>>,
}

/// `simplifiedPathPatternExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SimplifiedPathPattern {
    /// The default direction of its edges.
    pub(crate) direction: EdgeDirection,
    /// `simplifiedContents`.
    pub(crate) contents: BoxSpanned<SimplifiedExpr>,
}

/// The parts of a simplified path pattern (`GQL.g4` 16.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SimplifiedExpr {
    /// `simplifiedPathUnion`.
    Union(VecSpanned<SimplifiedExpr>),
    /// `simplifiedMultisetAlternation`.
    MultisetAlternation(VecSpanned<SimplifiedExpr>),
    /// `simplifiedTerm` concatenation.
    Concat(BoxSpanned<SimplifiedExpr>, BoxSpanned<SimplifiedExpr>),
    /// `simplifiedFactorLow & simplifiedFactorHigh`.
    Conjunction(BoxSpanned<SimplifiedExpr>, BoxSpanned<SimplifiedExpr>),
    /// `simplifiedQuantified`.
    Quantified {
        /// `simplifiedTertiary`.
        expr: BoxSpanned<SimplifiedExpr>,
        /// The quantifier.
        quantifier: Spanned<PatternQuantifier>,
    },
    /// `simplifiedQuestioned`.
    Questioned(BoxSpanned<SimplifiedExpr>),
    /// `simplifiedDirectionOverride`.
    DirectionOverride {
        /// The direction.
        direction: EdgeDirection,
        /// `simplifiedSecondary`.
        expr: BoxSpanned<SimplifiedExpr>,
    },
    /// `simplifiedNegation`.
    Negation(BoxSpanned<SimplifiedExpr>),
    /// `labelName`.
    Label(Ident),
    /// `( simplifiedContents )`.
    Parenthesized(BoxSpanned<SimplifiedExpr>),
}
