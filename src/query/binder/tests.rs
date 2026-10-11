//! Tests of the binder (rmp #43): feature gates, approved constructs,
//! binding errors, parameters, and catalog statements (rmp #46).

use super::*;
use crate::query::bound::{Body, CatalogOp, Part};
use crate::query::expr::Expr;
use crate::query::parser::parse_program;
use crate::test_support::OWNER;
use crate::tx::Isolation;
use crate::tx::tests::db;

fn bind_text(e: &crate::storage::engine::Engine, text: &str) -> Result<Program, Error> {
    let p = parse_program(text).unwrap_or_else(|err| panic!("{text}: {err}"));
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    bind(text, &p, &mut tx, &SessionCtx::new(OWNER))
}

/// One statement per unsupported construct class: each fails with
/// 42001 and the cause 51N26 naming its feature code.
#[test]
fn each_unsupported_construct_class_fails_with_feature_not_supported() {
    let (_d, e) = db();
    let cases: &[(&str, &str)] = &[
        ("MATCH (a) RETURN a UNION MATCH (b) RETURN b", "GQ03"),
        ("MATCH (a) RETURN a OTHERWISE MATCH (b) RETURN b", "GQ02"),
        ("MATCH (a) RETURN a EXCEPT ALL MATCH (b) RETURN b", "GQ05"),
        ("MATCH (a) RETURN a INTERSECT MATCH (b) RETURN b", "GQ06"),
        ("MATCH (a) LET x = 1 RETURN x", "GQ09"),
        ("FOR x IN [1] RETURN x", "GQ10"),
        ("MATCH (a) FILTER a.p = 1 RETURN a", "GQ08"),
        ("MATCH (a) RETURN a ORDER BY a OFFSET 1", "GQ12"),
        ("MATCH (a) RETURN a LIMIT 1", "GQ13"),
        ("MATCH (a) RETURN a.p AS p GROUP BY p", "GQ15"),
        ("MATCH (a) RETURN a NEXT MATCH (b) RETURN b", "GQ20"),
        ("MATCH (a) YIELD a RETURN a", "GQ19"),
        ("MATCH p = (a) RETURN a", "G004"),
        ("MATCH DIFFERENT EDGES (a) RETURN a", "G002"),
        ("MATCH REPEATABLE ELEMENTS (a) RETURN a", "G003"),
        ("MATCH TRAIL (a)-[e]->(b) RETURN a", "G011"),
        ("MATCH ANY SHORTEST (a)-[e]->(b) RETURN a", "G005"),
        ("MATCH (a)<-[e]-(b) RETURN a", "G043"),
        ("MATCH (a)~[e]~(b) RETURN a", "GH02"),
        ("MATCH (a)->(b) RETURN a", "G044"),
        ("MATCH (a)-[e]->{1,2}(b) RETURN a", "G036"),
        ("MATCH ((a)-[e]->(b)){1,2} RETURN a", "G035"),
        ("MATCH ((a)-[e]->(b))? RETURN a", "G037"),
        ("MATCH ((a)-[e]->(b)) RETURN a", "G038"),
        ("MATCH (a)-[e]->(b) | (a)-[f]->(b) RETURN a", "G032"),
        ("MATCH (a)-[e]->(b) |+| (a)-[f]->(b) RETURN a", "G030"),
        ("MATCH (a)-/T/->(b) RETURN a", "G039"),
        ("MATCH (a)(b) RETURN a", "G046"),
        ("MATCH (a:%) RETURN a", "G074"),
        ("MATCH (a WHERE a.p = b.p)-[e]->(b) RETURN a", "G041"),
        ("MATCH (a) RETURN ELEMENT_ID(a)", "G100"),
        ("MATCH (a)-[e]->(b) WHERE e IS DIRECTED RETURN a", "G110"),
        ("MATCH (a) WHERE a IS LABELED L RETURN a", "G111"),
        ("MATCH (a)-[e]->(b) WHERE a IS SOURCE OF e RETURN a", "G112"),
        ("MATCH (a), (b) WHERE ALL_DIFFERENT(a, b) RETURN a", "G113"),
        ("MATCH (a), (b) WHERE SAME(a, b) RETURN a", "G114"),
        ("MATCH (a) WHERE PROPERTY_EXISTS(a, p) RETURN a", "G115"),
        ("MATCH (a) RETURN a ORDER BY a NULLS FIRST", "GA03"),
        ("RETURN CAST(1 AS STRING)", "GA05"),
        ("RETURN 1 IS TYPED INT", "GA06"),
        ("RETURN TRUE XOR FALSE", "GE07"),
        ("RETURN LET x = 1 IN x END", "GE03"),
        ("RETURN VALUE { RETURN 1 }", "GQ18"),
        ("RETURN [1, 2]", "GV50"),
        ("RETURN {a: 1}", "GV45"),
        ("RETURN X'00'", "GV35"),
        ("RETURN 1.5", "GL04"),
        ("RETURN 0x1F", "GL01"),
        ("RETURN 0o17", "GL02"),
        ("RETURN 0b1", "GL03"),
        ("RETURN 1.5e0f", "GL08"),
        ("RETURN @'a'", "GL11"),
        ("RETURN 1 -- c", "GB02"),
        ("RETURN 1 // c", "GB03"),
        ("RETURN ABS(1)", "GF01"),
        ("RETURN SIN(1)", "GF02"),
        ("RETURN LN(1)", "GF03"),
        ("RETURN SIZE([1])", "GF13"),
        ("RETURN CARDINALITY([1])", "GF12"),
        ("RETURN TRIM(LEADING 'a' FROM 'ab')", "GF06"),
        ("RETURN BTRIM('ab', 'a')", "GF05"),
        ("MATCH (a) RETURN COLLECT_LIST(a)", "GF10"),
        ("MATCH (a) RETURN PERCENTILE_CONT(a.p, 0.5)", "GF11"),
        ("MATCH (a), (b) RETURN a ORDER BY b", "GA07"),
        ("MATCH (a) RETURN a.p AS q ORDER BY a.p + 1", "GQ14"),
        ("MATCH (a) RETURN count(*) AS c ORDER BY count(*)", "GF20"),
        ("MATCH (a) RETURN a AS b ORDER BY a", "GQ16"),
        ("OPTIONAL { MATCH (a) MATCH (b) } RETURN a", "GQ21"),
        (
            "MATCH (a) WHERE EXISTS { MATCH (a) MATCH (b) } RETURN a",
            "GQ22",
        ),
        ("MATCH (a) SET a:L", "GD02"),
        ("MATCH (a) REMOVE a:L", "GD02"),
        ("MATCH (a) DELETE a", "GD04"),
        ("INSERT (a)~[:T]~(b)", "GH02"),
        ("START TRANSACTION READ ONLY", "GT02"),
        ("SESSION SET TIME ZONE 'UTC'", "GS15"),
        ("SESSION SET VALUE $x = 1", "GS03"),
        ("SESSION RESET", "GS04"),
        ("AT /public MATCH (a) RETURN a", "GP16"),
        ("VALUE x = 1 RETURN x", "GP05"),
        ("CALL { RETURN 1 AS x } RETURN x", "GP01"),
        ("CREATE SCHEMA IF NOT EXISTS /s", "GC02"),
        ("CREATE GRAPH IF NOT EXISTS g ANY", "GC05"),
        ("CREATE GRAPH TYPE IF NOT EXISTS t { (:A) }", "GC03"),
        ("CREATE GRAPH g LIKE lpg", "GG04"),
        ("CREATE GRAPH g ANY AS COPY OF lpg", "GG05"),
        ("CREATE GRAPH TYPE t { NODE TYPE N (:A) }", "GG20"),
        ("CREATE GRAPH TYPE t { (:A {p INT64}) }", "GV12"),
        ("CREATE GRAPH TYPE t { (:A {p FLOAT64}) }", "GV24"),
        ("CREATE GRAPH TYPE t { (:A {p STRING NOT NULL}) }", "GV90"),
        ("CREATE GRAPH TYPE t { (:A {p DECIMAL}) }", "GV17"),
        ("USE $$g MATCH (a) RETURN a", "GE08"),
    ];
    let mut seen = std::collections::BTreeSet::new();
    let mut bad = Vec::new();
    for (text, code) in cases {
        assert!(!crate::query::features::is_supported(code));
        let got = parse_program(text)
            .map_err(|e| e.to_string())
            .and_then(|p| {
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                match bind(text, &p, &mut tx, &SessionCtx::new(OWNER)) {
                    Ok(_) => Err("bound".to_string()),
                    Err(err) => match err.cause() {
                        Some(c)
                            if err.code() == "42001"
                                && c.code() == "51N26"
                                && c.message().contains(code) =>
                        {
                            Ok(())
                        }
                        _ => Err(format!(
                            "{err} / {:?}",
                            err.cause().map(|c| c.message().to_string())
                        )),
                    },
                }
            });
        match got {
            Ok(()) => {
                seen.insert(*code);
            }
            Err(why) => bad.push(format!("{text} (want {code}): {why}")),
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    assert!(seen.len() >= 80, "{}", seen.len());
}

/// One statement per approved construct binds.
#[test]
fn each_approved_construct_binds() {
    let (_d, e) = db();
    let ok: &[&str] = &[
        "MATCH (a:L {p: 1})-[e:T]->(b) WHERE a.q > 2 RETURN a, e, b",
        "MATCH (a:L&M|!N) RETURN a",
        "MATCH (a) OPTIONAL MATCH (a)-[e]->(b) RETURN a, b",
        // An edge variable of an earlier statement joins.
        "MATCH ()-[r]->() MATCH ()-[r]->() RETURN r",
        "MATCH (a) OPTIONAL { MATCH (a)-[e]->(b) } RETURN a, b",
        "MATCH (a) WHERE EXISTS { (a)-[]->() } RETURN a",
        "MATCH (a) WHERE EXISTS { MATCH (a)-[]->(b) } RETURN a",
        "MATCH (a) RETURN DISTINCT a.p AS p ORDER BY p DESC",
        "MATCH (a) RETURN a.p AS p, count(*) AS c, sum(a.q) AS s, avg(a.q) AS v, min(a.q) AS mi, max(a.q) AS ma",
        "MATCH (a) RETURN count(DISTINCT a.p) AS c, stddev_samp(a.q) AS s, stddev_pop(a.q) AS t",
        "MATCH (a) RETURN *",
        "MATCH (a) ORDER BY a RETURN a",
        "SELECT a.p AS p FROM lpg MATCH (a) WHERE a.p > 1",
        "SELECT count(*) AS c FROM lpg MATCH (a) HAVING count(*) > 0",
        "SELECT 1 AS one",
        "RETURN 1 + 2 * 3 - 4 / 2 AS x, 'a' || 'b' AS y, -5 AS z, +1 AS w",
        "RETURN 1e3 AS f, -9223372036854775808 AS m",
        "RETURN TRUE AND NOT FALSE OR UNKNOWN AS b, NULL IS NULL AS n, TRUE IS NOT FALSE AS t",
        "RETURN 1 = 1 AS a, 1 <> 2 AS b, 1 < 2 AS c, 1 <= 2 AS d, 1 > 2 AS e, 1 >= 2 AS f",
        "RETURN CASE WHEN 1 = 1 THEN 'x' ELSE 'y' END AS a, CASE 1 WHEN 1, 2 THEN 'one' WHEN > 5 THEN 'big' ELSE 'z' END AS b",
        "RETURN NULLIF(1, 2) AS a, COALESCE(NULL, 1) AS b",
        "RETURN CHAR_LENGTH('a') AS a, UPPER('a') AS b, LOWER('A') AS c, LEFT('ab', 1) AS d, RIGHT('ab', 1) AS e, TRIM(' a ') AS f",
        "RETURN NORMALIZE('a', NFD) AS a, 'a' IS NORMALIZED AS b",
        "RETURN DATE '2020-01-01' AS a, TIME '10:00' AS b, TIMESTAMP '2020-01-01T10:00:00+01:00' AS c, DURATION 'P1Y' AS d",
        "RETURN CURRENT_DATE AS a, CURRENT_TIME AS b, CURRENT_TIMESTAMP AS c, LOCAL_TIME AS d, LOCAL_TIMESTAMP AS e",
        "RETURN DATE('2020-01-01') AS a, DATE({`year`: 2020, `month`: 1, `day`: 1}) AS b, ZONED_DATETIME('2020-01-01T00:00Z') AS c",
        "RETURN DURATION_BETWEEN(DATE '2020-01-01', DATE '2020-02-01') AS a, DURATION({`days`: 1}) AS b, ABS(DURATION 'P1D') AS c",
        "RETURN SESSION_USER AS u, $p AS p",
        "INSERT (a:L {p: 1})-[:T {w: 2}]->(b:M), (c)<-[:U]-(a)",
        "MATCH (a) SET a.p = 1, a = {q: 2} REMOVE a.r",
        "MATCH (a) INSERT (a)-[:T]->(:N) RETURN a",
        "USE lpg MATCH (a) RETURN a",
        "USE lpg INSERT (:L)",
        "START TRANSACTION",
        "COMMIT",
        "ROLLBACK",
        "SESSION SET SCHEMA /public",
        "SESSION SET GRAPH lpg",
        "SESSION CLOSE",
        "CREATE SCHEMA /s",
        "DROP SCHEMA /public",
        "CREATE GRAPH g ANY",
        "CREATE OR REPLACE GRAPH /public/g { (:A {p STRING, q INT})-[:T {w FLOAT}]->(:B) }",
        "CREATE GRAPH TYPE t { (a :A {p BOOL, d DATE, z ZONED DATETIME, y DURATION(YEAR TO MONTH)}), (:B)<-[:T]-(a) }",
        "DROP GRAPH lpg",
        "DROP GRAPH TYPE t",
    ];
    let bad: Vec<String> = ok
        .iter()
        .filter_map(|text| {
            let p = match parse_program(text) {
                Ok(p) => p,
                Err(err) => return Some(format!("{text}: {err}")),
            };
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            bind(text, &p, &mut tx, &SessionCtx::new(OWNER))
                .err()
                .map(|err| format!("{text}: {err}"))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn undefined_variables_duplicate_bindings_and_conditional_joins_fail() {
    let (_d, e) = db();
    let code = |t: &str| match bind_text(&e, t) {
        Ok(_) => "bound",
        Err(err) => err.code(),
    };
    assert_eq!(code("MATCH (a) RETURN b"), "42002");
    assert_eq!(code("MATCH (a) SET b.p = 1"), "42002");
    assert_eq!(code("MATCH (a)-[a]->(b) RETURN a"), "42000");
    // A variable bound as two element kinds: 42000 with the cause 22N27,
    // invalid entity type (gql-compat `condition/42002/redeclared-variable`;
    // DECISION 386 on rmp #52), in one path pattern or across two.
    for t in [
        "MATCH (a)-[a]->(b) RETURN a",
        "MATCH (p)-[k]->(q), (k) RETURN p",
    ] {
        let err = bind_text(&e, t).unwrap_err();
        assert_eq!(err.code(), "42000", "{t}");
        assert_eq!(err.cause().map(Error::code), Some("22N27"), "{t}");
    }
    assert_eq!(code("MATCH (a) RETURN a.p AS x, a.q AS x"), "42000");
    assert_eq!(
        code("MATCH (a) INSERT (a)-[e:T]->(b), (a)-[e:T]->(c)"),
        "42000"
    );
    // `c` is bound in one operand of the union only: a conditional
    // singleton, joined implicitly with the second path pattern.
    assert_eq!(
        code("MATCH (a)-[e]->(b) | (a)-[f]->(c), (c)-[g]->(d) RETURN a"),
        "42000"
    );
    // The same through an earlier statement.
    assert_eq!(
        code("MATCH (c) MATCH (a)-[e]->(b) | (a)-[f]->(c) RETURN a"),
        "42000"
    );
    assert_eq!(code("RETURN count(count(*)) AS c"), "42000");
    assert_eq!(code("MATCH (a) WHERE count(*) > 1 RETURN a"), "42000");
    assert_eq!(
        code("MATCH (a) RETURN a.p AS p, a.q + count(*) AS c"),
        "42000"
    );
    assert_eq!(code("INSERT (a)-[:T&U]->(b)"), "42007");
    assert_eq!(code("INSERT (a)-[]->(b)"), "42006");
    assert_eq!(code("RETURN 9223372036854775808 AS x"), "22003");
    // A static type error is a syntax rule violation (opengql/tck
    // `Boolean4.feature` [4]: 42000), found when the statement is
    // compiled: a non-boolean literal or an element reference as a boolean
    // operand or search condition (openCypher TCK `Pattern1.feature` [11]).
    assert_eq!(code("RETURN NOT 1"), "42000");
    assert_eq!(code("RETURN 'a' AND TRUE"), "42000");
    assert_eq!(code("MATCH (n) WHERE (n) RETURN n"), "42000");
    assert_eq!(code("MATCH (n)-[r]->() WHERE NOT r RETURN n"), "42000");
    // An edge variable of two edge patterns of one graph pattern
    // (openCypher TCK `Match3.feature` [29]), in one path or in two.
    assert_eq!(code("MATCH (a)-[r]->()-[r]->(a) RETURN r"), "42000");
    assert_eq!(code("MATCH ()-[r]->(), ()-[r]->() RETURN r"), "42000");
    assert_eq!(code("RETURN DATE '2020-13-01'"), "22007");
    // An invalid reference in a catalog-modifying statement is 42000 with
    // the cause 42002 (DECISION 386 on rmp #52).
    let err = bind_text(&e, "CREATE GRAPH TYPE t { (x :A), (:A)-[:T]->(y) }").unwrap_err();
    assert_eq!(err.code(), "42000");
    assert_eq!(err.cause().map(Error::code), Some("42002"));
    assert_eq!(code("CREATE GRAPH TYPE t { (:A), (:A) }"), "42000");
    assert_eq!(code("USE nosuchgraph MATCH (a) RETURN a"), "42002");
    assert_eq!(code("CALL my.proc()"), "42N08");
}

/// A parameter stays a parameter: the bound form holds an index into the
/// parameter table, and no literal with its value.
#[test]
fn a_parameter_stays_a_parameter_in_the_bound_form() {
    let (_d, e) = db();
    let b = bind_text(&e, "MATCH (a) WHERE a.p = $x + 1 RETURN $x AS y").unwrap();
    let Some(Body::Query(q)) = b.body else {
        panic!("a query")
    };
    assert_eq!(q.params, vec![Box::<str>::from("x")]);
    let mut params = 0;
    let mut lits = Vec::new();
    for part in &q.parts {
        let mut visit = |x: &Expr| {
            x.walk(&mut |y| match y {
                Expr::Param(0) => params += 1,
                Expr::Lit(v) => lits.push(v.clone()),
                _ => {}
            })
        };
        match part {
            Part::Match { pattern, .. } => pattern.predicates.iter().for_each(&mut visit),
            Part::Project(p) => p.items.iter().for_each(|(_, x)| visit(x)),
            _ => {}
        }
    }
    assert_eq!(params, 2);
    // The only literal is the `1` of `+ 1`.
    assert_eq!(lits, vec![crate::types::Value::Int(1)]);
}

/// FR-CAT-005: no statement of the grammar creates a directory. Every
/// catalog-modifying rule binds to a schema, graph or graph type
/// operation; a schema in a missing directory is not created.
#[test]
fn no_statement_creates_a_directory() {
    let (_d, e) = db();
    let texts = [
        "CREATE SCHEMA /d/e/s",
        "DROP SCHEMA /d/s",
        "CREATE GRAPH g ANY",
        "CREATE OR REPLACE GRAPH g ANY",
        "CREATE GRAPH g { (:A) }",
        "CREATE GRAPH TYPE t { (:A) }",
        "CREATE OR REPLACE GRAPH TYPE t { (:A) }",
        "DROP GRAPH g",
        "DROP GRAPH TYPE t",
    ];
    for t in texts {
        let b = bind_text(&e, t).unwrap_or_else(|err| panic!("{t}: {err}"));
        let Some(Body::Catalog(ops)) = b.body else {
            panic!("{t}: a catalog statement")
        };
        for op in ops {
            assert!(
                matches!(
                    op,
                    CatalogOp::CreateSchema(_)
                        | CatalogOp::DropSchema(_)
                        | CatalogOp::CreateGraph { .. }
                        | CatalogOp::DropGraph(_)
                        | CatalogOp::CreateGraphType { .. }
                        | CatalogOp::DropGraphType(_)
                ),
                "{t}"
            );
        }
    }
    // Running it: the missing directory is an invalid reference, and no
    // directory appears.
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let r = crate::query::exec::run(
        "CREATE SCHEMA /d/s",
        &mut tx,
        &SessionCtx::new(OWNER),
        &Default::default(),
    );
    let err = match r {
        Ok(mut c) => crate::query::exec::collect(&mut c, &mut tx).expect_err("no directory d"),
        Err(err) => err,
    };
    assert_eq!(err.code(), "42000");
    assert_eq!(err.cause().map(Error::code), Some("42002"));
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    assert!(tx.lookup(crate::catalog::ROOT, "d").unwrap().is_none());
}
