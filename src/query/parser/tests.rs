//! Parser tests: syntax errors, nesting limits, spans, parameters, typed
//! nodes and the ambiguity resolutions of TS-19.
//!
//! Every text passed as a literal to [`parse`], [`syntax_error`],
//! [`statement`], [`procedure`], [`returned`] or [`ast_contains`] is also
//! checked against the ANTLR oracle: `differential.rs`
//! (`test_texts_match_the_oracle_labels`) collects them from this file.

use super::{MAX_DEPTH, STACK_BUDGET, parse_program};
use crate::error::{Error, Position};
use crate::query::ast::{
    AmbientLinearQueryStatement, BinaryOp, CatalogModifyingStatement, CompositeQueryExpr,
    EdgeDirection, ElementPattern, ElementTypeSpec, Exists, Expr, FocusedLinearQueryStatement,
    Function, GraphExpr, GraphRef, GraphTypeSource, LabelExpr, LinearQueryStatement, Literal,
    MatchStatement, NumericLiteral, NumericLiteralKind, PathPatternExpr, Predicate, Procedure,
    Program, ProgramActivity, ResultStatement, ReturnItems, SelectFrom, SessionSet,
    SessionSetParameterKind, SimpleQueryStatement, Statement, UnaryFunction, UnaryOp,
};
use crate::query::span::{Span, Spanned};

/// The program `text`, which must parse.
fn parse(text: &str) -> Program {
    match parse_program(text) {
        Ok(p) => p,
        Err(e) => panic!("{text:?} does not parse: {e}"),
    }
}

/// The 42001 error of `text`, which must not parse.
fn syntax_error(text: &str) -> Error {
    match parse_program(text) {
        Ok(p) => panic!("{text:?} parses: {p:?}"),
        Err(e) => {
            assert_eq!(e.code(), "42001", "{e}");
            e
        }
    }
}

/// The procedure of a program made of one.
fn procedure(text: &str) -> Procedure {
    let program = parse(text);
    let Some(Spanned(ProgramActivity::Transaction(t), _)) = program.activity else {
        panic!("{text:?}: not a transaction activity");
    };
    t.procedure.expect("a procedure").0
}

/// The first statement of `text`.
fn statement(text: &str) -> Spanned<Statement> {
    procedure(text).statement
}

/// The linear query of a statement made of one.
fn linear(s: Statement) -> LinearQueryStatement {
    let Statement::Query(CompositeQueryExpr::Primary(q)) = s else {
        panic!("not a linear query: {s:?}");
    };
    *q
}

/// The parts of an ambient linear query.
fn ambient(s: Statement) -> (Vec<Spanned<SimpleQueryStatement>>, Spanned<ResultStatement>) {
    let LinearQueryStatement::Ambient(AmbientLinearQueryStatement::Parts { statements, result }) =
        linear(s)
    else {
        panic!("not an ambient query");
    };
    (statements, result)
}

/// The values of the items of a `RETURN`.
fn items(result: ResultStatement) -> Vec<Spanned<Expr>> {
    let ResultStatement::Return { statement, .. } = result else {
        panic!("no RETURN");
    };
    let ReturnItems::Items(items) = statement.0.items.0 else {
        panic!("RETURN *");
    };
    items.into_iter().map(|i| i.0.value).collect()
}

/// The values of the `RETURN` items of `text`, a query `RETURN …`.
fn returned(text: &str) -> Vec<Spanned<Expr>> {
    items(ambient(statement(text).0).1.0)
}

/// The text a span covers.
fn slice(text: &str, span: Span) -> &str {
    &text[span.start.offset as usize..span.end.offset as usize]
}

// ---- Syntax errors (BR-GQL-001) ------------------------------------------

#[test]
fn a_misspelt_keyword_fails_with_42001_its_position_and_the_expected_tokens() {
    // At the start: the expected set equals the oracle's for the same
    // text ("1:0 mismatched input 'MATC' expecting {'AT', 'CALL', …,
    // '{'}", `gql-oracle label`, 2026-10-09).
    let e = syntax_error("MATC (n) RETURN n");
    assert_eq!(
        e.position(),
        Some(Position {
            offset: 0,
            line: 1,
            column: 1
        })
    );
    assert_eq!(
        e.message(),
        "invalid syntax: unexpected 'MATC'; expected AT, CALL, COMMIT, CREATE, DELETE, DETACH, \
         DROP, FILTER, FINISH, FOR, INSERT, LET, LIMIT, MATCH, NODETACH, OFFSET, OPTIONAL, ORDER, \
         REMOVE, RETURN, ROLLBACK, SELECT, SESSION, SET, SKIP, START, USE, VALUE, BINDING, \
         GRAPH, PROPERTY, TABLE, '{'"
    );
    // After a statement, on the second line: the position is the
    // misspelt word's, and RETURN is among the expected tokens.
    let e = syntax_error("MATCH (n)\n  RETRN n");
    assert_eq!(
        e.position(),
        Some(Position {
            offset: 12,
            line: 2,
            column: 3
        })
    );
    assert!(
        e.message()
            .starts_with("invalid syntax: unexpected 'RETRN'; expected ")
    );
    for token in ["RETURN", "MATCH", "WHERE", "NEXT", "the end of the text"] {
        assert!(e.message().contains(token), "{token}: {e}");
    }
    assert_eq!(
        e.to_string(),
        format!("42001: {} (at line 2, column 3)", e.message())
    );
}

#[test]
fn index_management_and_permission_statements_fail_with_42001() {
    // FR-API-004: indexes and constraints are managed through the Rust
    // API only.
    let e = syntax_error("CREATE INDEX i FOR (n:L) ON (n.p)");
    assert_eq!(e.position().map(|p| p.column), Some(8));
    assert!(e.message().contains("unexpected 'INDEX'"), "{e}");
    syntax_error("DROP INDEX i");
    syntax_error("CREATE CONSTRAINT c FOR (n:L) REQUIRE n.p IS UNIQUE");
    // BR-AUTH-001: no permission statements.
    let e = syntax_error("GRANT READ ON GRAPH g TO r");
    assert_eq!(e.position().map(|p| p.offset), Some(0));
    assert!(e.message().contains("unexpected 'GRANT'"), "{e}");
    syntax_error("REVOKE READ ON GRAPH g FROM r");
    syntax_error("DENY READ ON GRAPH g TO r");
}

#[test]
fn an_unknown_character_fails_with_42001_at_its_position() {
    let e = syntax_error("RETURN 1 # 2");
    assert_eq!(e.position().map(|p| p.column), Some(10));
    assert!(e.message().contains("U+0023"), "{e}");
}

// ---- Nesting --------------------------------------------------------------

/// Texts nested `n` levels deep: every recursive rule of the parser, and
/// every left-deep chain of operators.
fn deep_texts(n: usize) -> Vec<(&'static str, String)> {
    vec![
        (
            "parentheses",
            format!("RETURN {}1{}", "(".repeat(n), ")".repeat(n)),
        ),
        ("unary minus", format!("RETURN {}1", "- ".repeat(n))),
        ("NOT", format!("RETURN {}TRUE", "NOT ".repeat(n))),
        ("+ chain", format!("RETURN 1{}", " + 1".repeat(n))),
        ("AND chain", format!("RETURN TRUE{}", " AND TRUE".repeat(n))),
        ("property chain", format!("RETURN a{}", ".b".repeat(n))),
        ("|| chain", format!("RETURN 'a'{}", " || 'a'".repeat(n))),
        ("IS TRUE chain", format!("RETURN a{}", " IS TRUE".repeat(n))),
        ("label |", format!("MATCH (a:A{}) RETURN a", "|A".repeat(n))),
        ("label !", format!("MATCH (a:{}A) RETURN a", "!".repeat(n))),
        (
            "label ()",
            format!("MATCH (a:{}A{}) RETURN a", "(".repeat(n), ")".repeat(n)),
        ),
        (
            "path ()",
            format!("MATCH {}(a){} RETURN a", "(".repeat(n), ")".repeat(n)),
        ),
        (
            "UNION chain",
            format!("RETURN 1{}", " UNION RETURN 1".repeat(n)),
        ),
        (
            "list",
            format!("RETURN {}1{}", "[".repeat(n), "]".repeat(n)),
        ),
        (
            "record",
            format!("RETURN {}1{}", "{a: ".repeat(n), "}".repeat(n)),
        ),
        (
            "LIST suffix",
            format!("RETURN CAST(1 AS INT{})", " LIST".repeat(n)),
        ),
        (
            "LIST<>",
            format!(
                "RETURN CAST(1 AS {}INT{})",
                "LIST<".repeat(n),
                ">".repeat(n)
            ),
        ),
        (
            "type |",
            format!("RETURN CAST(1 AS INT{})", " | INT".repeat(n)),
        ),
        (
            "braces",
            format!("{}RETURN 1{}", "{".repeat(n), "}".repeat(n)),
        ),
        (
            "CALL",
            format!("{}RETURN 1{}", "CALL {".repeat(n), "}".repeat(n)),
        ),
        (
            "EXISTS",
            format!(
                "RETURN {}TRUE{}",
                "EXISTS { MATCH (a) WHERE ".repeat(n),
                "}".repeat(n)
            ),
        ),
        (
            "VALUE",
            format!("RETURN {}1{}", "VALUE { RETURN ".repeat(n), "}".repeat(n)),
        ),
        (
            "simplified ()",
            format!(
                "MATCH (a)-/{}A{}/->(b) RETURN a",
                "(".repeat(n),
                ")".repeat(n)
            ),
        ),
        (
            "simplified concat",
            format!("MATCH (a)-/A{}/->(b) RETURN a", " A".repeat(n)),
        ),
        (
            "simplified &",
            format!("MATCH (a)-/A{}/->(b) RETURN a", "&A".repeat(n)),
        ),
        (
            "CASE",
            format!(
                "RETURN {}1{}",
                "CASE WHEN TRUE THEN ".repeat(n),
                " END".repeat(n)
            ),
        ),
        (
            "ABS",
            format!("RETURN {}1{}", "ABS(".repeat(n), ")".repeat(n)),
        ),
        (
            "numeric chain",
            format!("RETURN ABS(1{})", " + 1".repeat(n)),
        ),
    ]
}

/// A 100 000-deep text of every shape fails with 42001 on a 2 MiB stack,
/// the default stack of tokio's blocking threads (TS-02). Reaching the
/// limit walks the deepest path the parser admits, so this also bounds
/// the stack of the deepest accepted texts.
#[test]
fn deep_texts_fail_with_42001_on_a_2_mib_stack() {
    let results = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            deep_texts(100_000)
                .into_iter()
                .map(|(name, text)| (name, parse_program(&text).map(|_| ())))
                .collect::<Vec<_>>()
        })
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    let expected = format!(
        "invalid syntax: the text nests too deeply (more than {MAX_DEPTH} levels or {} KiB of \
         stack)",
        STACK_BUDGET >> 10
    );
    for (name, result) in results {
        let e = result.expect_err(name);
        assert_eq!(e.code(), "42001", "{name}");
        assert_eq!(e.message(), expected, "{name}");
        assert!(e.position().is_some(), "{name}");
    }
}

#[test]
fn texts_within_the_limits_parse() {
    // Nesting well inside both limits.
    for (name, text) in deep_texts(20) {
        if let Err(e) = parse_program(&text) {
            panic!("{name}: {e}");
        }
    }
    // Flat repetitions are not nesting.
    let flat = [
        format!("MATCH (a){} RETURN a", "-(a)".repeat(10_000)),
        format!("RETURN [{}1]", "1, ".repeat(10_000)),
        format!("RETURN 1{}", " NEXT RETURN 1".repeat(10_000)),
    ];
    for text in flat {
        parse_program(&text).expect("a flat repetition is not nesting");
    }
    // A chain of 200 operators, under MAX_DEPTH, parses.
    const { assert!(MAX_DEPTH > 200) };
    parse_program(&format!("RETURN a{}", " OR a".repeat(200))).expect("200 operators");
}

// ---- Spans ----------------------------------------------------------------

#[test]
fn every_node_keeps_the_span_of_its_text() {
    let text = "MATCH (n:Person)\n  RETURN n.name AS x";
    let s = statement("MATCH (n:Person)\n  RETURN n.name AS x");
    assert_eq!(slice(text, s.1), text);
    let (statements, result) = ambient(s.0);
    assert_eq!(slice(text, statements[0].1), "MATCH (n:Person)");
    assert_eq!(slice(text, result.1), "RETURN n.name AS x");
    let SimpleQueryStatement::Match(MatchStatement::Simple(table)) = &statements[0].0 else {
        panic!("not a simple match");
    };
    assert_eq!(slice(text, table.0.pattern.0.patterns[0].1), "(n:Person)");
    let ResultStatement::Return { statement, .. } = result.0 else {
        panic!("no RETURN");
    };
    let ReturnItems::Items(list) = &statement.0.items.0 else {
        panic!("RETURN *");
    };
    let item = &list[0];
    assert_eq!(slice(text, item.1), "n.name AS x");
    assert_eq!(slice(text, item.0.value.1), "n.name");
    assert_eq!(
        (item.0.value.1.start.line, item.0.value.1.start.column),
        (2, 10)
    );
    let alias = item.0.alias.as_ref().expect("alias");
    assert_eq!((&*alias.0, slice(text, alias.1)), ("x", "x"));
    let Expr::Property { source, name } = &item.0.value.0 else {
        panic!("not a property reference");
    };
    assert_eq!(slice(text, source.1), "n");
    assert_eq!(slice(text, name.1), "name");
}

#[test]
fn columns_count_characters_and_offsets_count_bytes() {
    let text = "RETURN 'λ' + $p";
    let values = returned("RETURN 'λ' + $p");
    let Expr::Binary { left, right, .. } = &values[0].0 else {
        panic!("not a binary expression");
    };
    assert_eq!(slice(text, left.1), "'λ'");
    assert_eq!(right.1.start.offset, 14);
    assert_eq!((right.1.start.line, right.1.start.column), (1, 14));
    assert_eq!(slice(text, right.1), "$p");
}

// ---- Parameters -----------------------------------------------------------

#[test]
fn parameters_are_never_inlined() {
    let (statements, result) =
        ambient(statement("MATCH (n) WHERE n.id = $id RETURN $id + 1 AS x").0);
    let SimpleQueryStatement::Match(MatchStatement::Simple(table)) = &statements[0].0 else {
        panic!("not a simple match");
    };
    let condition = table.0.pattern.0.where_clause.as_ref().expect("WHERE");
    let (op, _, right) = binary(&condition.0);
    assert_eq!((op, right), (BinaryOp::Eq, &Expr::Parameter("id".into())));
    let values = items(result.0);
    let (op, left, _) = binary(&values[0].0);
    assert_eq!((op, left), (BinaryOp::Add, &Expr::Parameter("id".into())));

    // A session parameter keeps its name.
    let program = parse("SESSION SET VALUE $x = 1");
    let Some(Spanned(ProgramActivity::Session(session), _)) = program.activity else {
        panic!("not a session activity");
    };
    let SessionSet::Parameter(param) = &session.set[0].0 else {
        panic!("not a parameter");
    };
    assert_eq!(&*param.0.name.0, "x");
    assert!(matches!(param.0.kind, SessionSetParameterKind::Value(_)));
    // A substituted parameter in a catalog reference stays a reference.
    let LinearQueryStatement::Focused(FocusedLinearQueryStatement::Result { use_graph, .. }) =
        linear(statement("USE $$g RETURN 1").0)
    else {
        panic!("not a focused query");
    };
    assert_eq!(
        use_graph.0,
        GraphExpr::Reference(GraphRef::Parameter("g".into()))
    );
}

// ---- Typed nodes ----------------------------------------------------------

/// The operator and operands of a binary expression.
fn binary(e: &Expr) -> (BinaryOp, &Expr, &Expr) {
    let Expr::Binary { op, left, right } = e else {
        panic!("not a binary expression: {e:?}");
    };
    (op.0, &left.0, &right.0)
}

/// The decimal integer literal `text`.
fn integer(text: &str) -> Expr {
    Expr::Literal(Literal::Numeric(NumericLiteral {
        kind: NumericLiteralKind::Integer(10),
        text: text.into(),
    }))
}

#[test]
fn operators_bind_by_precedence_and_associate_left() {
    // 1 + 2 * 3 - 4 is (1 + (2 * 3)) - 4.
    let values = returned("RETURN 1 + 2 * 3 - 4");
    let (op, left, right) = binary(&values[0].0);
    assert_eq!((op, right), (BinaryOp::Sub, &integer("4")));
    let (op, one, product) = binary(left);
    assert_eq!((op, one), (BinaryOp::Add, &integer("1")));
    assert_eq!(
        binary(product),
        (BinaryOp::Mul, &integer("2"), &integer("3"))
    );
    // a OR b AND NOT c is a OR (b AND (NOT c)).
    let values = returned("RETURN a OR b AND NOT c");
    let (op, a, conjunction) = binary(&values[0].0);
    assert_eq!((op, a), (BinaryOp::Or, &Expr::Variable("a".into())));
    let (op, b, not_c) = binary(conjunction);
    assert_eq!((op, b), (BinaryOp::And, &Expr::Variable("b".into())));
    let Expr::Unary { op, child } = not_c else {
        panic!("not NOT");
    };
    assert_eq!(
        (op.0, &child.0),
        (UnaryOp::Not, &Expr::Variable("c".into()))
    );
    // -2 * 3 is (-2) * 3; 1 < 2 = TRUE is (1 < 2) = TRUE.
    let values = returned("RETURN -2 * 3");
    let (op, minus_two, _) = binary(&values[0].0);
    assert_eq!(op, BinaryOp::Mul);
    assert!(matches!(minus_two, Expr::Unary { op, .. } if op.0 == UnaryOp::Minus));
    let values = returned("RETURN 1 < 2 = TRUE");
    let (op, less, _) = binary(&values[0].0);
    assert_eq!(op, BinaryOp::Eq);
    assert_eq!(binary(less).0, BinaryOp::Lt);
}

#[test]
fn literals_keep_their_kind_and_text() {
    let values = returned("RETURN 9223372036854775808, 0x1F, 1.5, 1e3, 'it''s', NULL, TRUE");
    let kinds: Vec<_> = values
        .iter()
        .map(|e| match &e.0 {
            Expr::Literal(Literal::Numeric(n)) => format!("{:?} {}", n.kind, n.text),
            Expr::Literal(Literal::String(s)) => format!("string {}", s.value),
            Expr::Literal(l) => format!("{l:?}"),
            other => panic!("not a literal: {other:?}"),
        })
        .collect();
    // The integer above INT64 is kept as text: its range is the binder's
    // check (IL010, 22003).
    assert_eq!(
        kinds,
        [
            "Integer(10) 9223372036854775808",
            "Integer(16) 1F",
            "Exact 1.5",
            "Approximate 1e3",
            "string it's",
            "Null",
            "Boolean(True)",
        ]
    );
}

#[test]
fn a_pattern_keeps_its_labels_directions_and_variables() {
    let text = "MATCH (a:A|B&!C)-[e:E]->(b) RETURN a";
    let (statements, _) = ambient(statement("MATCH (a:A|B&!C)-[e:E]->(b) RETURN a").0);
    let SimpleQueryStatement::Match(MatchStatement::Simple(table)) = &statements[0].0 else {
        panic!("not a simple match");
    };
    let path = &table.0.pattern.0.patterns[0].0.expr;
    let PathPatternExpr::Concat(factors) = &path.0 else {
        panic!("not a concatenation: {path:?}");
    };
    assert_eq!(factors.len(), 3);
    let PathPatternExpr::Element(ElementPattern::Node(a)) = &factors[0].0 else {
        panic!("not a node");
    };
    // A|B&!C is A|(B&(!C)).
    let label = a.label.as_ref().expect("label");
    assert_eq!(slice(text, label.1), "A|B&!C");
    let LabelExpr::Disjunction(left, right) = &label.0 else {
        panic!("not a disjunction: {label:?}");
    };
    assert_eq!(left.0, LabelExpr::Label("A".into()));
    let LabelExpr::Conjunction(_, not_c) = &right.0 else {
        panic!("not a conjunction: {right:?}");
    };
    assert!(matches!(not_c.0, LabelExpr::Negation(_)));
    let PathPatternExpr::Element(ElementPattern::Edge {
        direction,
        filler,
        abbreviated,
    }) = &factors[1].0
    else {
        panic!("not an edge");
    };
    assert_eq!((*direction, *abbreviated), (EdgeDirection::Right, false));
    assert_eq!(filler.variable.as_ref().map(|v| &*v.0), Some("e"));
}

// ---- Ambiguity resolutions (TS-19 "Ambiguities") ---------------------------

#[test]
fn statement_a_query_that_is_also_data_modifying_is_a_query() {
    // Alternative 1 (compositeQueryStatement) over 3.
    assert!(matches!(
        statement("MATCH (n) RETURN n").0,
        Statement::Query(_)
    ));
    // Only data-modifying: alternative 3.
    assert!(matches!(
        statement("MATCH (n) SET n.p = 1").0,
        Statement::Data(_)
    ));
}

#[test]
fn statement_a_catalog_call_is_catalog_modifying() {
    // Alternative 2 (linearCatalogModifyingStatement) over 3.
    let Statement::Catalog(list) = statement("CALL { CREATE GRAPH g ANY }").0 else {
        panic!("not catalog-modifying");
    };
    assert!(matches!(list[0].0, CatalogModifyingStatement::Call(_)));
}

#[test]
fn label_and_property_set_and_insert_filler_keep_every_part() {
    // labelAndPropertySetSpecification and insertElementPatternFiller:
    // both alternatives build the same node, so the AST carries no trace
    // of the choice; the test pins that every part is kept.
    let debug = format!("{:?}", statement("INSERT (:L {p: 1})").0);
    assert!(
        debug.contains("labels: Some") && debug.contains("properties: Some"),
        "{debug}"
    );
    let debug = format!("{:?}", statement("INSERT (a:L)").0);
    assert!(
        debug.contains("variable: Some") && debug.contains("labels: Some"),
        "{debug}"
    );
}

#[test]
fn value_function_abs_is_the_numeric_function() {
    // Alternative 1 (numericValueFunction) over 4 (durationValueFunction):
    // both give ABS of one argument; the binder decides by its type.
    let values = returned("RETURN ABS(x)");
    let Expr::Function(Function::Unary { kind, arg }) = &values[0].0 else {
        panic!("not a function");
    };
    assert_eq!(
        (*kind, &arg.0),
        (UnaryFunction::Abs, &Expr::Variable("x".into()))
    );
}

#[test]
fn exists_a_match_block_in_braces_is_the_match_block() {
    // Alternative 3 (matchStatementBlock) over 5 (nestedQuerySpecification).
    let values = returned("RETURN EXISTS { MATCH (a) MATCH (b) }");
    let Expr::Predicate(Predicate::Exists(Exists::Match(block))) = &values[0].0 else {
        panic!("not a match block: {:?}", values[0].0);
    };
    assert_eq!(block.len(), 2);
}

#[test]
fn graph_pattern_takes_the_where_clause() {
    // Alternative 1: the optional graphPatternWhereClause is taken, not
    // left to the SELECT's whereClause.
    let LinearQueryStatement::Focused(FocusedLinearQueryStatement::Select(select)) =
        linear(statement("SELECT p FROM g MATCH (p) WHERE p.x = 1").0)
    else {
        panic!("not a SELECT");
    };
    let body = select.body.expect("FROM");
    assert!(body.where_clause.is_none());
    let SelectFrom::GraphMatches(matches) = body.from.0 else {
        panic!("not a graph match list");
    };
    let MatchStatement::Simple(table) = &matches[0].0.statement.0 else {
        panic!("not a simple match");
    };
    assert!(table.0.pattern.0.where_clause.is_some());
}

/// The direction of the third element type of a `CREATE GRAPH TYPE`.
fn third_edge_direction(s: Statement) -> EdgeDirection {
    let Statement::Catalog(list) = s else {
        panic!("not catalog-modifying");
    };
    let CatalogModifyingStatement::CreateGraphType(create) = &list[0].0 else {
        panic!("not CREATE GRAPH TYPE");
    };
    let GraphTypeSource::Nested(elements) = &create.source.0 else {
        panic!("not a nested graph type");
    };
    let ElementTypeSpec::Edge(edge) = &elements.0[2].0 else {
        panic!("not an edge type");
    };
    edge.direction
}

#[test]
fn endpoint_pair_to_is_directed() {
    // Alternative 1 (endpointPairDirected) over 2 (endpointPairUndirected).
    let s =
        statement("CREATE GRAPH TYPE t { NODE A, NODE B, UNDIRECTED EDGE E CONNECTING (A TO B) }");
    assert_eq!(third_edge_direction(s.0), EdgeDirection::Right);
    // `~` is the only undirected form.
    let s =
        statement("CREATE GRAPH TYPE t { NODE A, NODE B, UNDIRECTED EDGE E CONNECTING (A ~ B) }");
    assert_eq!(third_edge_direction(s.0), EdgeDirection::Undirected);
}

#[test]
fn result_and_cast_operand_null_is_an_expression() {
    // result and castOperand: alternative 1 (an expression) over 2
    // (nullLiteral); both give the literal NULL.
    let values = returned("RETURN CASE WHEN TRUE THEN NULL END, CAST(NULL AS INT)");
    let debug = format!("{values:?}");
    assert_eq!(debug.matches("Literal(Null)").count(), 2, "{debug}");
}

/// The AST of `text` printed without spans or `Spanned` wrappers.
fn compact(text: &str) -> String {
    let full = format!("{:?}", parse(text));
    let mut out = String::new();
    let mut rest = full.as_str();
    while let Some(i) = rest.find(", Span(") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find("))").map_or(rest.len(), |j| i + j + 2);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.replace("Spanned(", "(")
}

/// Asserts that the [`compact`] AST of `text` contains `expected`.
fn ast_contains(text: &str, expected: &str) {
    let ast = compact(text);
    assert!(ast.contains(expected), "{text}\n  {ast}");
}

/// The decisions found ambiguous on 600 000 generated texts (TS-19
/// "Ambiguities", 2026-10-09): each example's AST is the one the oracle
/// builds, its lowest-numbered alternative (compared with the ANTLR
/// parse tree of the same text).
#[test]
fn ambiguous_decisions_found_by_generation_take_the_lowest_alternative() {
    // graphExpression 1 (graphReference) over 2 (objectExpressionPrimary).
    ast_contains("USE \"\" FINISH", "use_graph: (Reference(Delimited(\"\")))");
    // bindingTableExpression 2 (bindingTableReference) over 3.
    ast_contains(
        "SESSION SET TABLE $m = \"\"",
        "init: (Reference(Delimited(\"\")))",
    );
    // simpleDataAccessingStatement 1 (simpleQueryStatement) over 2.
    ast_contains("USE g CALL p()", "statements: [(Query(Call(");
    // absoluteCatalogSchemaReference 1 (`/`) over 2.
    ast_contains(
        "AT / PROPERTY GRAPH g = x FINISH",
        "at: Some((Root)), binding_variable_defs: [(Graph(",
    );
    // bindingTableReference 1 and graphReference 1 (parent and name)
    // over 3 and 4 (a substituted parameter).
    ast_contains(
        "TABLE s = $$IB BINDING TABLE t = x FINISH",
        "schema: Some((Parameter(\"IB\"))), objects: [] })), name: (\"BINDING\")",
    );
    ast_contains(
        "AT / PROPERTY GRAPH f = $$Y BINDING TABLE z = SESSION_USER FINISH",
        "schema: Some((Parameter(\"Y\"))), objects: [] })), name: (\"BINDING\")",
    );
    // primitiveCatalogModifyingStatement 3 and 4 (graph) over 5 and 6
    // (graph type); catalogGraphParentAndName and
    // catalogObjectParentReference: the longest parent.
    ast_contains(
        "AT / CREATE GRAPH TYPE . a . b . c AS COPY OF $$ys",
        "CreateGraph(CreateGraphStatement { path: (CatalogObjectRef { parent: \
             Some((ParentRef { schema: None, objects: [(\"TYPE\"), (\"a\")] })), name: (\"b\")",
    );
    ast_contains(
        "DROP GRAPH TYPE . x",
        "DropGraph(DropStatement { path: (CatalogObjectRef { parent: Some((ParentRef \
             { schema: None, objects: [(\"TYPE\")] })), name: (\"x\")",
    );
    // nodeTypePhrase and edgeTypePhrase: `TYPE` is the keyword, not a
    // type name.
    ast_contains(
        "CREATE GRAPH TYPE t { NODE TYPE { } }",
        "Node(NodeTypeSpec { name: None",
    );
    ast_contains(
        "CREATE GRAPH TYPE t { UNDIRECTED EDGE TYPE { } CONNECTING (a -> b) }",
        "Edge(EdgeTypeSpec { directed: Some(false), name: None",
    );
    // nodeTypeFiller 1: an empty key label set, then the labels.
    ast_contains(
        "SESSION SET PROPERTY GRAPH ( n IS :: VERTEX => IS NORMALIZED )",
        "key_labels: Some(None), labels: Some(([(\"NORMALIZED\")]))",
    );
    // orderByAndPageStatement: the optional OFFSET and LIMIT are taken.
    ast_contains(
        "ORDER BY x SKIP 2 FINISH",
        "statements: [(OrderByAndPage(OrderByAndPage { order_by: [(SortSpec { key: \
             (Variable(\"x\")), ordering: None, null_ordering: None })], offset: Some(",
    );
    // relativeDirectoryPath, simpleDirectoryPath, absoluteDirectoryPath:
    // the directories end where a name and a parent separator follow.
    ast_contains(
        "VALUE v = TABLE .. / d / e / f || x FINISH",
        "Relative { up: 1, path: SchemaPath { directories: [(\"d\")], name: (\"e\") } \
             })), objects: [] })), name: (\"f\")",
    );
    ast_contains(
        "VALUE v = TABLE / d / e FINISH",
        "Binary { op: (Div), left: (BindingTable((Reference(Object(CatalogObjectRef { \
             parent: Some((ParentRef { schema: Some((Root)), objects: [] })), name: (\"d\")",
    );
    // valueType: inside ANY < >, a union member may itself be a
    // union; a list's NOT NULL belongs to the list.
    ast_contains(
        "VALUE v ANY < NULL | VERTEX > = 1 FINISH",
        "value_type: Some((Union([(Union([(Null), (AnyNode { not_null: false })]))])))",
    );
    ast_contains(
        "RETURN COUNT ( * ) IS TYPED LIST < { } > NOT NULL",
        "List { element: Some((Record { fields: Some([]), not_null: false })), name: List, \
             max_length: None, not_null: true }",
    );
    // valueExpression 4 (GRAPH graphExpression) and 5 (TABLE
    // bindingTableExpression) over 7 (a property or predicate of a
    // variable named GRAPH or TABLE).
    ast_contains(
        "CALL p ( GRAPH . groups )",
        "args: [(Graph((Reference(Object(",
    );
    ast_contains(
        "VALUE w = TABLE / nfkc FINISH",
        "init: (BindingTable((Reference(Object(",
    );
    // caseOperand 1 over 2 (elementVariableReference): both a variable.
    ast_contains(
        "RETURN CASE w WHEN 1 THEN 2 END",
        "Case(Simple { operand: (Variable(\"w\"))",
    );
}
