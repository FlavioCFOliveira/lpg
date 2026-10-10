//! Catalog object references and object expressions (`GQL.g4` 11, 17).

use super::{Expr, Ident, Procedure};
use crate::query::span::{BoxSpanned, OptSpanned, Spanned, VecSpanned};

/// `schemaReference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SchemaRef {
    /// `/`: the root directory.
    Root,
    /// `absoluteDirectoryPath schemaName`.
    Absolute(SchemaPath),
    /// `relativeDirectoryPath schemaName`: `..` repeated `up` times, then
    /// directories and the schema.
    Relative {
        /// How many `..` the path starts with (at least one).
        up: u32,
        /// The directories and the schema name.
        path: SchemaPath,
    },
    /// `HOME_SCHEMA`.
    Home,
    /// `CURRENT_SCHEMA` or `.`.
    Current,
    /// `$$name`: a reference parameter.
    Parameter(Ident),
}

/// `absoluteDirectoryPath schemaName` (also `catalogSchemaParentAndName`):
/// the directories below the root, then the schema name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaPath {
    /// `simpleDirectoryPath`.
    pub(crate) directories: VecSpanned<Ident>,
    /// `schemaName`.
    pub(crate) name: Spanned<Ident>,
}

/// `catalogObjectParentReference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParentRef {
    /// The schema, if one is written.
    pub(crate) schema: OptSpanned<SchemaRef>,
    /// The object names, each followed by `.` in the text.
    pub(crate) objects: VecSpanned<Ident>,
}

/// A catalog object named by an optional parent and a name
/// (`catalogGraphParentAndName`, `catalogGraphTypeParentAndName`,
/// `catalogProcedureParentAndName`, and the first alternative of
/// `graphReference` and `bindingTableReference`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogObjectRef {
    /// `catalogObjectParentReference`.
    pub(crate) parent: OptSpanned<ParentRef>,
    /// The object's name.
    pub(crate) name: Spanned<Ident>,
}

/// `graphReference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphRef {
    /// `catalogObjectParentReference graphName`.
    Object(CatalogObjectRef),
    /// `delimitedGraphName`.
    Delimited(Ident),
    /// `HOME_GRAPH` or `HOME_PROPERTY_GRAPH`.
    Home,
    /// `$$name`.
    Parameter(Ident),
}

/// `graphTypeReference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphTypeRef {
    /// `catalogGraphTypeParentAndName`.
    Object(CatalogObjectRef),
    /// `$$name`.
    Parameter(Ident),
}

/// `bindingTableReference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingTableRef {
    /// `catalogObjectParentReference bindingTableName`.
    Object(CatalogObjectRef),
    /// `delimitedBindingTableName`.
    Delimited(Ident),
    /// `$$name`.
    Parameter(Ident),
}

/// `procedureReference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcedureRef {
    /// `catalogProcedureParentAndName`.
    Object(CatalogObjectRef),
    /// `$$name`.
    Parameter(Ident),
}

/// `graphExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphExpr {
    /// `graphReference`.
    Reference(GraphRef),
    /// `objectExpressionPrimary`.
    Object(ObjectExpr),
    /// `objectNameOrBindingVariable`.
    Name(Ident),
    /// `CURRENT_GRAPH` or `CURRENT_PROPERTY_GRAPH`.
    Current,
}

/// `bindingTableExpression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingTableExpr {
    /// `nestedBindingTableQuerySpecification`.
    Nested(BoxSpanned<Procedure>),
    /// `bindingTableReference`.
    Reference(BindingTableRef),
    /// `objectExpressionPrimary`.
    Object(ObjectExpr),
    /// `objectNameOrBindingVariable`.
    Name(Ident),
}

/// `objectExpressionPrimary`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObjectExpr {
    /// `VARIABLE valueExpressionPrimary`.
    Variable(BoxSpanned<Expr>),
    /// `parenthesizedValueExpression`: the expression inside.
    Parenthesized(BoxSpanned<Expr>),
    /// `nonParenthesizedValueExpressionPrimarySpecialCase`.
    Primary(BoxSpanned<Expr>),
}
