//! Conformance runner (TS-30 Semantics; rmp #52): the scenarios of
//! `tests/tck/` run through the public API.
//!
//! * **Artefacts** (`tests/tck/README.md`): the opengql/tck and openCypher
//!   TCK feature files that hold a scenario selected by the TS-30 rule,
//!   unmodified, with their `LICENSE` and `NOTICE`; lpg's own scenarios in
//!   `tests/tck/lpg/features/`, among them the gql-compat cases translated
//!   with attribution.
//! * **Selection** (`tests/tck/selection.tsv`): one line per scenario
//!   instance of the vendored files and per gql-compat case, written by
//!   `docs/evidence/lab-sprint-3/tck_manifest.py` from the ANTLR parse of
//!   `GQL.g4` and the rule-to-feature anchors. A `selected` instance must
//!   pass; the others are skipped with the codes that exclude them.
//!   [`EXCEPTIONS`] lists the selected instances that do something else,
//!   with why: an engine gate the rule cannot see, an expectation outside
//!   GQL, or a dispute reported to the user. Each must do exactly what the
//!   list says ([`run::Instead`]).
//! * **Clause or feature code**: every scenario carries a feature-code tag
//!   or a `@clause-N.N` tag, or its file has a clause in
//!   `tests/tck/clauses.tsv`; every clause is a subclause heading of
//!   `GQL.g4`.

mod run;
pub(crate) mod scan;
mod value;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use run::Instead;
use scan::Scenario;

use crate::test_support::graph_base;

fn tck_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tck")
}

/// The artefacts: directory under `tests/tck/` and whether the selection
/// manifest covers it (lpg's own scenarios all run).
const ARTEFACTS: [(&str, bool); 3] = [("opengql-tck", true), ("opencypher", true), ("lpg", false)];

/// Why a scenario does something other than what it expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// An engine feature gate excludes it, though the TS-30 rule selected
    /// it: the gate's code has no `GQL.g4` construct of its own (a lexical
    /// or semantic code), so the rule cannot see it.
    Gate,
    /// What it expects is outside GQL as the cited source defines it.
    NotGql,
    /// It contradicts a recorded lpg decision or is defective; reported to
    /// the user, awaiting a decision. Its current behaviour is pinned.
    Disputed,
}

/// A selected scenario that does something else: artefact, file, title,
/// example row, why, what it does instead, and the evidence.
type Exception = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Why,
    Instead,
    &'static str,
);

const RETURN6: &str = "clauses/return/Return6.feature";
const ORDER_BY2: &str = "clauses/return-orderby/ReturnOrderBy2.feature";
const ORDER_BY6: &str = "clauses/return-orderby/ReturnOrderBy6.feature";
const TEMPORAL1: &str = "expressions/temporal/Temporal1.feature";
const TEMPORAL2: &str = "expressions/temporal/Temporal2.feature";
const CREATE1: &str = "statements/catalog-modifying/create/schemas/Create1.feature";
const DROP1: &str = "statements/catalog-modifying/drop/drop1.feature";
const DUR7: &str = "Scenario Outline: [7] Should parse duration from string";
/// `--` starts a comment in GQL (`GQL.g4` `SIMPLE_COMMENT_MINUS`), feature
/// GB02; the query's `(a)--(b)` is a comment from `--` to the line end.
const GB02: &str = "`--` begins a GB02 comment in GQL";
const GF20: &str = "an aggregate in a sort key is GF20 (rmp #43 binder)";
/// `<duration string>`: `<iso8601 years and months>` or `<iso8601 days and
/// time>`, a fraction only on seconds, no weeks, no mixed form
/// (`ISO_IEC_39075(en).bnf.txt:2639-2670`); lpg's two duration types
/// (GV41, TS-21) hold no mixed value.
const DURATION: &str = "not a GQL duration string (ISO BNF insert 2639-2670; GV41 types)";
const CODES: &str = "lpg's code for this failure is recorded in interfaces.md \"GQLSTATUS codes\"; opengql/tck expects 42000";
const DIRS: &str = "needs CREATE SCHEMA to create its parent directory; FR-CAT-005: directories only through the API";

const GC_QUERY: &str = "gql-compat/10-mandatory-query.feature";
const GC_SESSION: &str = "gql-compat/13-mandatory-session.feature";
const GC_CONDITIONS: &str = "gql-compat/30-conditions.feature";
const GC_VALUE: &str = "gql-compat/31-conditions-value.feature";

const EXCEPTIONS: &[Exception] = &[
    (
        "lpg",
        GC_QUERY,
        "Scenario: [mandatory/query/order-by-as-a-statement] An order by and page statement stands on its own inside a linear query statement",
        "",
        Why::Gate,
        Instead::Gate("GQ14"),
        "a sort key that is not a result column is GQ14 (TS-20, rmp #43); gql-compat places it in the mandatory surface",
    ),
    (
        "lpg",
        GC_SESSION,
        "Scenario: [mandatory/session/set-schema-at-the-root] A session may be pointed at the root of the catalog",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        "the root is a directory, not a schema (TS-17); whether `/` names a schema needs the ISO text (unverified point 1)",
    ),
    (
        "lpg",
        GC_CONDITIONS,
        "Scenario: [condition/42002/redeclared-variable] Binding one variable to two different element types is an invalid reference",
        "",
        Why::Disputed,
        Instead::Fails("42000"),
        "a variable bound twice is 42000 (DECISION 326 on rmp #43); gql-compat expects 42002",
    ),
    (
        "lpg",
        GC_VALUE,
        "Scenario: [condition/22g04/comparing-a-number-with-a-string] Comparing values of unrelated types raises values not comparable",
        "",
        Why::Disputed,
        Instead::Succeeds,
        "`<` across type groups is NULL (IV002, IV010; user decision 2026-10-07); gql-compat expects 22G04",
    ),
    (
        "opencypher",
        RETURN6,
        "Scenario: [19] Handle returned property accesses inside an expression which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        RETURN6,
        "Scenario: [20] Fail if not returned variables are used inside an expression which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        RETURN6,
        "Scenario: [21] Fail if more complex expressions, even if returned, are used inside expression which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        ORDER_BY6,
        "Scenario: [2] Handle returned aliases inside an order by item which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        ORDER_BY6,
        "Scenario: [3] Handle returned property accesses inside an order by item which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        ORDER_BY6,
        "Scenario: [4] Fail if not returned variables are used inside an order by item which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        ORDER_BY6,
        "Scenario: [5] Fail if more complex expressions, even if returned, are used inside an order by item which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        "expressions/existentialSubqueries/ExistentialSubquery2.feature",
        "Scenario: [3] Full existential subquery with update clause should fail",
        "",
        Why::Gate,
        Instead::Gate("GB02"),
        GB02,
    ),
    (
        "opencypher",
        ORDER_BY2,
        "Scenario: [14] Fail on aggregation in ORDER BY after RETURN",
        "",
        Why::Gate,
        Instead::Gate("GF20"),
        GF20,
    ),
    (
        "opencypher",
        ORDER_BY6,
        "Scenario: [1] Handle constants and parameters inside an order by item which contains an aggregation expression",
        "",
        Why::Gate,
        Instead::Gate("GF20"),
        GF20,
    ),
    (
        "opencypher",
        TEMPORAL1,
        "Scenario Outline: [12] Should construct duration",
        "{years: 12, months: 5, days: 14, hours: 16, minutes: 12, seconds: 70} | 'P12Y5M14DT16H13M10S'",
        Why::NotGql,
        Instead::Fails("22G07"),
        "a duration of years and days: no GV41 duration type holds it (TS-21)",
    ),
    (
        "opencypher",
        TEMPORAL2,
        DUR7,
        "'P5M1.5D' | 'P5M1DT12H'",
        Why::NotGql,
        Instead::Fails("22G0H"),
        DURATION,
    ),
    (
        "opencypher",
        TEMPORAL2,
        DUR7,
        "'P0.75M' | 'P22DT19H51M49.5S'",
        Why::NotGql,
        Instead::Fails("22G0H"),
        DURATION,
    ),
    (
        "opencypher",
        TEMPORAL2,
        DUR7,
        "'P2.5W' | 'P17DT12H'",
        Why::NotGql,
        Instead::Fails("22G0H"),
        DURATION,
    ),
    (
        "opencypher",
        TEMPORAL2,
        DUR7,
        "'P12Y5M14DT16H12M70S' | 'P12Y5M14DT16H13M10S'",
        Why::NotGql,
        Instead::Fails("22G0H"),
        DURATION,
    ),
    (
        "opencypher",
        TEMPORAL2,
        DUR7,
        "'P2012-02-02T14:37:21.545' | 'P2012Y2M2DT14H37M21.545S'",
        Why::NotGql,
        Instead::Fails("22G0H"),
        DURATION,
    ),
    (
        "opencypher",
        "clauses/call/Call1.feature",
        "Scenario: [12] In-query call to procedure that has outputs fails if no outputs are yielded",
        "",
        Why::Disputed,
        Instead::Succeeds,
        "a CALL without YIELD binds every column (DECISION 360 on rmp #50)",
    ),
    (
        "opengql-tck",
        "expressions/aggregation/Aggregation3.feature",
        "Scenario: [1] Sum only non-null values",
        "",
        Why::Disputed,
        Instead::Fails("42000"),
        "defective: its setup INSERTs give the bound 'p' labels, and its header names n.name, sum(n.num)",
    ),
    (
        "opengql-tck",
        "Debug.feature",
        "Scenario: [4] Raise error condition creating a schema whose name identifies a directory",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        DIRS,
    ),
    (
        "opengql-tck",
        CREATE1,
        "Scenario: [2] Create a schema, in a directory",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        DIRS,
    ),
    (
        "opengql-tck",
        CREATE1,
        "Scenario: [4] Raise error condition creating a schema whose name identifies a directory",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        DIRS,
    ),
    (
        "opengql-tck",
        DROP1,
        "Scenario: [3] Raise error condition dropping a schema whose name identifies a directory",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        DIRS,
    ),
    (
        "opengql-tck",
        CREATE1,
        "Scenario: [3] Raise error condition creating a schema that already exists",
        "",
        Why::Disputed,
        Instead::Fails("42N11"),
        CODES,
    ),
    (
        "opengql-tck",
        CREATE1,
        "Scenario: [5] Raise error condition creating a schema whose name identifies a graph",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        CODES,
    ),
    (
        "opengql-tck",
        CREATE1,
        "Scenario: [6] Raise error condition creating a schema whose name identifies a graph type",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        CODES,
    ),
    (
        "opengql-tck",
        DROP1,
        "Scenario: [2] Raise error condition dropping a schema that doesn't exists",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        CODES,
    ),
    (
        "opengql-tck",
        DROP1,
        "Scenario: [4] Raise error condition dropping a schema whose name identifies a graph",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        CODES,
    ),
    (
        "opengql-tck",
        DROP1,
        "Scenario: [5] Raise error condition dropping a schema whose name identifies a graph type",
        "",
        Why::Disputed,
        Instead::Fails("42002"),
        CODES,
    ),
    (
        "opengql-tck",
        DROP1,
        "Scenario: [6] Raise error condition dropping a non-empty schema",
        "",
        Why::Disputed,
        Instead::Fails("G1000"),
        CODES,
    ),
];

/// The steps whose docstring is a GQL text the runner executes.
const STATEMENT_STEPS: [&str; 5] = [
    "having executed:",
    "having executed the program:",
    "executing query:",
    "executing the program:",
    "executing control query:",
];

/// Every GQL text of every scenario of `tests/tck/`, selected or not
/// (outlines expanded), and the named-graph scripts: the scenario texts of
/// the parser differential (TS-30 Parser).
pub(crate) fn scenario_texts() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (artefact, _) in ARTEFACTS {
        for s in scenarios(artefact) {
            for st in &s.steps {
                if STATEMENT_STEPS.contains(&st.text.as_str())
                    && let Some(d) = &st.doc
                {
                    out.insert(d.clone());
                }
            }
        }
        let graphs = tck_dir().join(artefact).join("data/graphs");
        if let Ok(entries) = std::fs::read_dir(graphs) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "gql") {
                    out.insert(std::fs::read_to_string(p).expect("a graph script"));
                }
            }
        }
    }
    out
}

/// One line of `selection.tsv`.
struct Selection {
    state: String,
    codes: String,
}

/// `(artefact, file, title, row)` → selection.
fn selection() -> BTreeMap<(String, String, String, String), Selection> {
    let text = std::fs::read_to_string(tck_dir().join("selection.tsv")).expect("selection.tsv");
    text.lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 6, "a selection line: {l}");
            (
                (f[0].into(), f[1].into(), f[2].into(), f[3].into()),
                Selection {
                    state: f[4].into(),
                    codes: f[5].into(),
                },
            )
        })
        .collect()
}

/// Every scenario instance of an artefact.
fn scenarios(artefact: &str) -> Vec<Scenario> {
    scan::scan_tree(&tck_dir().join(artefact).join("features"))
}

/// The key of a scenario in the selection manifest.
fn key(artefact: &str, s: &Scenario) -> (String, String, String, String) {
    (
        artefact.to_string(),
        s.file.clone(),
        s.title.clone(),
        s.row.clone(),
    )
}

/// A scenario's name in reports.
fn name(artefact: &str, s: &Scenario) -> String {
    if s.row.is_empty() {
        format!("{artefact}/{}: {}", s.file, s.title)
    } else {
        format!("{artefact}/{}: {} [{}]", s.file, s.title, s.row)
    }
}

/// What happened to one scenario.
enum Verdict {
    Passed,
    Failed(String),
    /// Not selected: the manifest's state and codes.
    Unselected(String),
    /// Listed in [`EXCEPTIONS`] and did what the list says.
    Excepted(Why, String),
}

/// The exception of the scenario `k`, if any.
fn exception(k: &(String, String, String, String)) -> Option<&'static Exception> {
    EXCEPTIONS
        .iter()
        .find(|e| (e.0, e.1, e.2, e.3) == (k.0.as_str(), k.1.as_str(), k.2.as_str(), k.3.as_str()))
}

/// Runs every selected scenario of `artefact`; the verdicts by name.
fn run_artefact(artefact: &str, manifest: bool) -> Vec<(String, Verdict)> {
    let sel = selection();
    let all = scenarios(artefact);
    let data = tck_dir().join(artefact).join("data");
    // Name, scenario to run (None: not selected), exception, skip reason.
    let mut work: Vec<(String, Option<&Scenario>, Option<&Exception>, String)> = Vec::new();
    for s in &all {
        let n = name(artefact, s);
        let k = key(artefact, s);
        let ex = exception(&k);
        if !manifest {
            work.push((n, Some(s), ex, String::new()));
            continue;
        }
        match sel.get(&k) {
            Some(x) if x.state == "selected" => work.push((n, Some(s), ex, String::new())),
            Some(x) => work.push((n, None, None, format!("{} {}", x.state, x.codes))),
            None => panic!("{n} is not in selection.tsv: rerun tck_manifest.py"),
        }
    }
    let base = graph_base();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let chunk = work.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = work
            .chunks(chunk)
            .map(|part| {
                let data = &data;
                scope.spawn(move || {
                    part.iter()
                        .map(|(n, s, ex, why)| {
                            let verdict = match s {
                                None => Verdict::Unselected(why.clone()),
                                Some(s) => {
                                    let dir = tempfile::tempdir_in(base).expect("a scenario dir");
                                    match (run::run(s, dir.path(), data, ex.map(|e| e.5)), ex) {
                                        (Ok(()), None) => Verdict::Passed,
                                        (Ok(()), Some(e)) => {
                                            Verdict::Excepted(e.4, format!("{:?}: {}", e.5, e.6))
                                        }
                                        (Err(err), None) => Verdict::Failed(err),
                                        (Err(err), Some(e)) => Verdict::Failed(format!(
                                            "listed as {:?} {:?}, but: {err}",
                                            e.4, e.5
                                        )),
                                    }
                                }
                            };
                            (n.clone(), verdict)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a scenario thread"))
            .collect()
    })
}

/// Counts of one artefact's verdicts.
#[derive(Debug, Default)]
struct Counts {
    passed: usize,
    failed: usize,
    unselected: usize,
    gate: usize,
    not_gql: usize,
    disputed: usize,
}

/// Prints the verdicts and their counts; `LPG_TCK_REPORT` names a file
/// that receives one line per scenario.
fn report(artefact: &str, verdicts: &[(String, Verdict)]) -> Counts {
    let mut c = Counts::default();
    let mut lines = String::new();
    for (n, v) in verdicts {
        let (tag, why) = match v {
            Verdict::Passed => {
                c.passed += 1;
                ("PASS", "")
            }
            Verdict::Failed(e) => {
                c.failed += 1;
                println!("FAIL {n}\n     {e}");
                ("FAIL", e.as_str())
            }
            Verdict::Unselected(why) => {
                c.unselected += 1;
                ("UNSELECTED", why.as_str())
            }
            Verdict::Excepted(w, why) => {
                match w {
                    Why::Gate => c.gate += 1,
                    Why::NotGql => c.not_gql += 1,
                    Why::Disputed => c.disputed += 1,
                }
                (
                    match w {
                        Why::Gate => "GATE",
                        Why::NotGql => "NOT-GQL",
                        Why::Disputed => "DISPUTED",
                    },
                    why.as_str(),
                )
            }
        };
        lines.push_str(&format!("{tag}\t{n}\t{why}\n"));
    }
    println!("{artefact}: {} instances; {c:?}", verdicts.len());
    println!(
        "NFR-PERF-007: {} statements profiled to completion, at most {} operator(s) running at once",
        run::PROFILED.load(std::sync::atomic::Ordering::Relaxed),
        run::MAX_RUNNING.load(std::sync::atomic::Ordering::Relaxed)
    );
    if let Some(path) = std::env::var_os("LPG_TCK_REPORT") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open LPG_TCK_REPORT");
        file.write_all(lines.as_bytes()).expect("write the report");
    }
    c
}

#[test]
fn opengql_tck_selected_scenarios_pass() {
    let c = report("opengql-tck", &run_artefact("opengql-tck", true));
    assert_eq!(c.failed, 0, "failed scenarios");
    assert!(c.passed > 0);
}

#[test]
fn opencypher_tck_selected_scenarios_pass() {
    let c = report("opencypher", &run_artefact("opencypher", true));
    assert_eq!(c.failed, 0, "failed scenarios");
    assert!(c.passed > 0);
}

#[test]
fn lpg_scenarios_pass() {
    let c = report("lpg", &run_artefact("lpg", false));
    assert_eq!(c.failed, 0, "failed scenarios");
    assert!(c.passed > 0);
}

/// The selection manifest lists exactly the instances of the vendored
/// files, and every selected instance is a vendored one.
#[test]
fn the_selection_manifest_matches_the_vendored_files() {
    let sel = selection();
    let mut scanned = BTreeSet::new();
    for (artefact, manifest) in ARTEFACTS {
        if !manifest {
            continue;
        }
        for s in scenarios(artefact) {
            assert!(
                scanned.insert(key(artefact, &s)),
                "duplicate instance {}",
                name(artefact, &s)
            );
        }
    }
    let listed: BTreeSet<_> = sel
        .keys()
        .filter(|k| k.0 != "gql-compat")
        .cloned()
        .collect();
    let missing: Vec<_> = listed.difference(&scanned).collect();
    let extra: Vec<_> = scanned.difference(&listed).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "not scanned {missing:?}; not listed {extra:?}"
    );
    // Every exception names a scenario that exists and, for a vendored
    // artefact, a selected one.
    let mut own = BTreeSet::new();
    for s in scenarios("lpg") {
        own.insert(key("lpg", &s));
    }
    for e in EXCEPTIONS {
        let k = (e.0.into(), e.1.into(), e.2.into(), e.3.into());
        if e.0 == "lpg" {
            assert!(own.contains(&k), "{e:?}");
        } else {
            assert_eq!(
                sel.get(&k).map(|s| s.state.as_str()),
                Some("selected"),
                "{e:?}"
            );
        }
    }
}

/// The subclause headings of `GQL.g4`, such as `14.4 <match statement>`.
fn grammar_clauses() -> BTreeSet<String> {
    let g4 = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/gql-oracle/grammar/GQL.g4"),
    )
    .expect("GQL.g4");
    g4.lines()
        .filter_map(|l| l.strip_prefix("// "))
        .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_scenario_names_its_clause_or_feature_code() {
    let clauses = grammar_clauses();
    let numbers: BTreeSet<&str> = clauses
        .iter()
        .filter_map(|c| c.split_whitespace().next())
        .collect();
    let codes: BTreeSet<&str> = crate::query::features::features()
        .iter()
        .map(|f| f.code)
        .collect();
    let mapped: BTreeMap<(String, String), String> =
        std::fs::read_to_string(tck_dir().join("clauses.tsv"))
            .expect("clauses.tsv")
            .lines()
            .skip(1)
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                assert_eq!(f.len(), 3, "a clauses.tsv line: {l}");
                assert!(clauses.contains(f[2]), "{} is not a GQL.g4 subclause", f[2]);
                ((f[0].into(), f[1].into()), f[2].into())
            })
            .collect();
    let mut unnamed = Vec::new();
    let mut count = 0;
    for (artefact, _) in ARTEFACTS {
        for s in scenarios(artefact) {
            count += 1;
            let by_code = s.tags.iter().any(|t| codes.contains(t.as_str()));
            let by_tag = s
                .tags
                .iter()
                .filter_map(|t| t.strip_prefix("clause-"))
                .inspect(|n| assert!(numbers.contains(n), "{n} is not a GQL.g4 subclause"))
                .count()
                > 0;
            let by_file = mapped.contains_key(&(artefact.to_string(), s.file.clone()));
            if !(by_code || by_tag || by_file) {
                unnamed.push(name(artefact, &s));
            }
        }
    }
    assert!(count > 0);
    assert!(
        unnamed.is_empty(),
        "scenarios without a clause or code: {unnamed:#?}"
    );
}

/// The runner fails a scenario whose expected rows are wrong, and passes
/// the same scenario with the right rows (a permanent check that it
/// compares rows at all).
#[test]
fn a_wrong_expected_row_fails_the_runner() {
    let scenario = |rows: &str, order: &str| {
        let src = format!(
            "Feature: f\n  Scenario: s\n    Given any graph\n    When executing query:\n      \
             \"\"\"\n      MATCH (n:N) RETURN n.v AS v\n      \"\"\"\n    \
             Then the result should be, {order}:\n      | v |\n{rows}"
        );
        let mut s = scan::scan("self-test.feature", &src).remove(0);
        // Two nodes, v = 1 and v = 2.
        s.steps.insert(
            1,
            scan::Step {
                text: "having executed:".into(),
                doc: Some("INSERT (:N {v: 1}), (:N {v: 2})".into()),
                table: Vec::new(),
            },
        );
        let dir = tempfile::tempdir_in(graph_base()).expect("a scenario dir");
        run::run(&s, dir.path(), dir.path(), None)
    };
    assert!(scenario("      | 1 |\n      | 2 |\n", "in any order").is_ok());
    assert!(scenario("      | 2 |\n      | 1 |\n", "in any order").is_ok());
    for (rows, order) in [
        ("      | 1 |\n      | 3 |\n", "in any order"),
        ("      | 1 |\n", "in any order"),
        ("      | 1 |\n      | 2 |\n      | 2 |\n", "in any order"),
        ("      | '1' |\n      | 2 |\n", "in any order"),
        ("      | 1.0 |\n      | 2 |\n", "in any order"),
    ] {
        assert!(scenario(rows, order).is_err(), "{rows} {order}");
    }
}
