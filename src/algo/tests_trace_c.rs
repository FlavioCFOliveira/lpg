//! rmp #71 pass C: the mechanical review of the components and structural
//! measures (task #67) and of the spanning trees, flows, matching,
//! covering, cliques and colouring entries (task #68), with the blossom
//! matching that ALG-187 calls.
//!
//! * **Reference cases** ([`reference_traces`]): each case of
//!   `testdata/expected/trace_c.json` names a trace graph, a call and the
//!   event sequence of the algorithm's reference executed on that graph,
//!   written by `docs/evidence/lab-sprint-5/trace_ref_c.py` (each reference
//!   function cites its source, file and line; each case has a `source`).
//!   The trace graphs are pass A's ([`super::tests_trace_a::build`]) or a
//!   CSV fixture (pass B's [`super::tests_trace_b::build_b`]). A case may
//!   also give the expected rows, an expected error code, the expected
//!   k-spanning tree (written mode) or the size of a maximum matching.
//!   Parallel entries run at `concurrency` 1 and 16 on projections of one
//!   node per block, and must start more than one worker at 16.
//! * **Blossom** ([`blossom_steps_follow_networkx`]): `blossom.rs` against
//!   NetworkX's `max_weight_matching` instrumented the same way, and
//!   [`blossom_never_allocates_blossom_n`] on random graphs.
//! * The other tests: ALG-151's exact decision, the matching sizes, and
//!   the semantic edge cases and error codes.

use std::collections::{BTreeMap, BTreeSet};

use rand::RngExt;

use super::tests_paths::{current, int, txt};
use super::tests_trace_a::value;
use super::tests_trace_b::build_b;
use super::trace::{Recording, TraceGraph, assert_steps, only};
use crate::Parameters;
use crate::test_support::algo::{expected_dir, record, rows, with};
use crate::types::Value;

/// The reference file.
fn reference() -> serde_json::Value {
    let text = std::fs::read_to_string(expected_dir().join("trace_c.json")).expect("trace_c.json");
    serde_json::from_str(&text).expect("JSON")
}

/// The strings of array `key` of `c` (none: empty).
fn strs(c: &serde_json::Value, key: &str) -> Vec<String> {
    c.get(key)
        .and_then(|x| x.as_array())
        .map(|xs| xs.iter().map(|x| x.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// The node of a node value, as its index (trace graphs: element
/// identifier `i` is node `i`).
fn ix(g: &TraceGraph, v: &Value) -> usize {
    g.uid(v)
}

/// Checks the rows of a maximum matching: pairs of adjacent distinct
/// nodes, no node twice, `size` pairs.
fn check_matching(
    g: &TraceGraph,
    spec: &serde_json::Value,
    rows: &[Vec<Value>],
    size: usize,
) -> Option<String> {
    let edges: BTreeSet<(usize, usize)> = spec["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let (a, b) = (
                e[0].as_u64().unwrap() as usize,
                e[1].as_u64().unwrap() as usize,
            );
            (a.min(b), a.max(b))
        })
        .collect();
    let mut seen = BTreeSet::new();
    for r in rows {
        let (a, b) = (ix(g, &r[0]), ix(g, &r[1]));
        if a == b || !edges.contains(&(a.min(b), a.max(b))) || !seen.insert(a) || !seen.insert(b) {
            return Some(format!("not a matching: {rows:?}"));
        }
    }
    (rows.len() != size).then(|| format!("{} pairs instead of {size}", rows.len()))
}

/// Runs case `name` (module documentation); returns its failures.
async fn check_case(
    name: &str,
    c: &serde_json::Value,
    all: &serde_json::Value,
    graphs: &mut BTreeMap<String, TraceGraph>,
) -> Vec<String> {
    let mut failures = Vec::new();
    let gname = c["graph"].as_str().expect("graph").to_string();
    if !graphs.contains_key(&gname) {
        graphs.insert(gname.clone(), build_b(&all["graphs"][&gname]).await);
    }
    let g = &graphs[&gname];
    let proc = c["proc"].as_str().expect("proc");
    let args: Vec<Value> = c["args"]
        .as_array()
        .expect("args")
        .iter()
        .map(|x| value(g, x))
        .collect();
    let kinds = strs(c, "kinds");
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    let want = strs(c, "want");
    let want: Vec<&str> = want.iter().map(String::as_str).collect();
    let want_rows: Option<Vec<Vec<Value>>> = c.get("rows").map(|rs| {
        rs.as_array()
            .expect("rows")
            .iter()
            .map(|r| {
                r.as_array()
                    .expect("a row")
                    .iter()
                    .map(|x| value(g, x))
                    .collect()
            })
            .collect()
    });
    for conc in c["concurrency"].as_array().expect("concurrency") {
        let how = format!("{name} at concurrency {conc}");
        let config = with(&value(g, &c["config"]), "concurrency", value(g, conc));
        g.workers();
        let (out, events) = g.try_trace(proc, &args, &config).await;
        if conc.as_u64() == Some(16) && g.workers().unwrap_or(1) < 2 {
            failures.push(format!("{how}: one worker at concurrency 16"));
        }
        match (&out, c.get("error").and_then(|x| x.as_str())) {
            (Err(e), Some(code)) if e.code() == code => {}
            (Err(e), _) => failures.push(format!("{how}: {e}")),
            (Ok(_), Some(code)) => failures.push(format!("{how}: no error, {code} expected")),
            (Ok(rows), None) => {
                if let Some(w) = &want_rows
                    && rows != w
                {
                    failures.push(format!("{how}: rows {rows:?} instead of {w:?}"));
                }
                if let Some(size) = c.get("matching_size").and_then(serde_json::Value::as_u64)
                    && let Some(f) = check_matching(g, &all["graphs"][&gname], rows, size as usize)
                {
                    failures.push(format!("{how}: {f}"));
                }
            }
        }
        // ALG-149 (write mode): the tree's nodes hold the root's identifier.
        if let Some(tree) = c.get("tree") {
            let root = c["root"].as_u64().unwrap() as i64;
            let want: BTreeSet<i64> = tree
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            let got: BTreeSet<i64> = rows(&g.s, "MATCH (n) WHERE n.kst = $r RETURN n.uid", {
                let mut ps = Parameters::new();
                ps.insert("r".to_string(), int(root));
                ps
            })
            .await
            .expect("read the tree")
            .iter()
            .map(|r| match r[0] {
                Value::Int(u) => u,
                _ => panic!("a uid"),
            })
            .collect();
            if got != want {
                failures.push(format!("{how}: tree {got:?} instead of {want:?}"));
            }
        }
        let got = only(&events, &kinds);
        let result = std::panic::catch_unwind(|| assert_steps(&how, &got, &want));
        if let Err(e) = result {
            failures.push(
                e.downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "a failure".to_string()),
            );
        }
    }
    failures
}

/// rmp #71 pass C: every reference case (module documentation); set
/// `TRACE_CASE` to run the cases whose names start with it.
#[test]
fn reference_traces() {
    current(async {
        let all = reference();
        let mut graphs: BTreeMap<String, TraceGraph> = BTreeMap::new();
        let mut failures = Vec::new();
        let filter = std::env::var("TRACE_CASE").unwrap_or_default();
        let mut ran = 0;
        for (name, c) in all["cases"].as_object().expect("cases") {
            if !name.starts_with(&filter) {
                continue;
            }
            ran += 1;
            failures.extend(check_case(name, c, &all, &mut graphs).await);
        }
        assert!(ran > 0, "no case matched {filter:?}");
        eprintln!("rmp #71 pass C: {ran} reference cases");
        for g in graphs.into_values() {
            g.db.close().await.unwrap();
        }
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    });
}

/// The blossom events (`blossom.rs`; module documentation).
const BLOSSOM_KINDS: &[&str] = &["blossom", "expand", "augment", "augblossom", "delta"];

/// The edges of a reference blossom case.
fn blossom_edges(b: &serde_json::Value) -> Vec<(usize, usize, i128)> {
    b["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e[0].as_u64().unwrap() as usize,
                e[1].as_u64().unwrap() as usize,
                i128::from(e[2].as_i64().unwrap()),
            )
        })
        .collect()
}

/// rmp #71 pass C: `blossom.rs` forms, expands and augments through the
/// same blossoms, and changes the duals by the same types and amounts,
/// as NetworkX's `max_weight_matching` on NetworkX's blossom test cases
/// (`trace_ref_c.py` `blossom_ref`).
#[test]
fn blossom_steps_follow_networkx() {
    let all = reference();
    for (i, b) in all["blossom"].as_array().unwrap().iter().enumerate() {
        let n = b["n"].as_u64().unwrap() as usize;
        let edges = blossom_edges(b);
        let rec = Recording::start();
        let mate = super::blossom::max_weight_matching(n, &edges, b["maxcard"].as_bool().unwrap());
        let got = only(&rec.take(), BLOSSOM_KINDS);
        drop(rec);
        let want = strs(b, "want");
        let want: Vec<&str> = want.iter().map(String::as_str).collect();
        assert_steps(&format!("blossom case {i}"), &got, &want);
        let pairs: Vec<(u64, u64)> = mate
            .iter()
            .enumerate()
            .filter_map(|(v, m)| m.filter(|w| v < *w).map(|w| (v as u64, w as u64)))
            .collect();
        let want_pairs: Vec<(u64, u64)> = b["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_u64().unwrap(), p[1].as_u64().unwrap()))
            .collect();
        assert_eq!(pairs, want_pairs, "blossom case {i}");
    }
}

/// rmp #71 pass C: rustworkx's `augment_blossom` tests `> num_nodes`
/// where lpg tests `>= n` (`blossom.rs` header). Blossom ids are taken
/// from the end of the free list `n..2n` and returned on expansion; the
/// blossoms alive at once form a laminar family whose members have at
/// least three children, so at most `(n - 1) / 2` exist and id `n` is
/// never taken: the two tests agree. Measured here on random graphs.
#[test]
fn blossom_never_allocates_blossom_n() {
    let mut g = super::sched::rng(71, 0);
    let mut formed = 0usize;
    let mut lowest_gap = usize::MAX;
    for _ in 0..3000 {
        let n = g.random_range(2..=14usize);
        let mut edges = Vec::new();
        for a in 0..n {
            for b in a + 1..n {
                if g.random_range(0..100u32) < 45 {
                    edges.push((a, b, i128::from(g.random_range(1..=40u32))));
                }
            }
        }
        let rec = Recording::start();
        let _ = super::blossom::max_weight_matching(n, &edges, g.random_range(0..2u32) == 1);
        for e in only(&rec.take(), &["blossom"]) {
            let id: usize = e.split(' ').nth(1).unwrap().parse().unwrap();
            assert!(id > n, "blossom {id} on {n} vertices: {edges:?}");
            assert!(
                id >= 2 * n - (n - 1) / 2,
                "more than (n - 1) / 2 blossoms alive: {id} on {n}"
            );
            lowest_gap = lowest_gap.min(id - n);
            formed += 1;
        }
    }
    eprintln!("rmp #71 pass C: {formed} blossoms formed, smallest id - n = {lowest_gap}");
    assert!(formed > 1000, "too few blossoms: {formed}");
}

/// rmp #71 pass C, ALG-151: the exact decision (`trees.rs` `exact_det`).
/// A determinant M + P1 - 1 (M = P1 P2, the residues' modulus) has the
/// residue P1 - 1, inside the INTEGER range: only nalgebra's estimate
/// shows that the count is above it, and the entry fails. On the trace
/// graphs whose diagonal bound exceeds M (a ring of 130 nodes, a chain of
/// 12 cycles), the estimate is within 1e-9 of the exact count.
#[test]
fn count_spanning_trees_decides_exactly() {
    const P1: i64 = 2_305_843_009_213_693_951;
    const P2: i64 = 4_611_686_018_427_387_847;
    assert_eq!(super::trees::exact_det(&[P1, -1, -1, P2 + 1], 2), None);
    // det = P1 - 1 itself, with a bound below M: exact.
    assert_eq!(super::trees::exact_det(&[P1 - 1, 0, 0, 1], 2), Some(P1 - 1));
    current(async {
        let all = reference();
        for name in [
            "151 ring of 130: bound above M",
            "151 chain of 12 30-cycles: count above 2^53",
        ] {
            let c = &all["cases"][name];
            let g = build_b(&all["graphs"][c["graph"].as_str().unwrap()]).await;
            let (rows, events) = g
                .trace(
                    "gds.countSpanningTrees.stream",
                    &[],
                    &value(&g, &c["config"]),
                )
                .await;
            let exact = c["rows"][0][0].as_i64().unwrap();
            assert_eq!(rows, vec![vec![int(exact)]], "{name}");
            let f = only(&events, &["f64"]);
            assert_eq!(f.len(), 1, "{name}: the estimate decides: {events:?}");
            let d: f64 = f[0].trim_start_matches("f64: ").parse().unwrap();
            let rel = (d - exact as f64).abs() / exact as f64;
            eprintln!(
                "rmp #71 pass C: 151 {name}: nalgebra {d:?}, exact {exact}, relative error {rel:e}"
            );
            assert!(rel < 1e-9, "{name}: {d} against {exact}");
            g.db.close().await.unwrap();
        }
    });
}

/// rmp #71 pass C, regression of ALG-169: the empty projection has no
/// maximal clique, as NetworkX's `find_cliques` (`clique.py:250-251`);
/// before the fix lpg returned one row with the empty clique, as
/// petgraph's `maximal_cliques` does.
#[test]
fn maximal_cliques_of_the_empty_projection_are_none() {
    current(async {
        let empty = TraceGraph::new(0, &[]).await;
        let (rows, _) = empty
            .trace("gds.maximalCliques.stream", &[], &record(&[]))
            .await;
        assert!(rows.is_empty(), "{rows:?}");
        empty.db.close().await.unwrap();
    });
}

/// The error code of `CALL proc(args…, config)`.
async fn code(g: &TraceGraph, proc: &str, args: &[Value], cfg: Value) -> Option<String> {
    g.try_trace(proc, args, &cfg)
        .await
        .0
        .map_err(|e| e.code().to_string())
        .err()
}

/// rmp #71 pass C, semantic checks: the empty projection's special rows,
/// orientation on directed-only entries, and the error codes of
/// `algorithm-entries.md` and TS-25 (22N11 for a missing or invalid key, a
/// node outside the projection, a negative-cost cycle, an unsupported
/// parameter; 22G03 for a value of the wrong type).
#[test]
fn errors_of_the_pass_c_entries() {
    current(async {
        let empty = TraceGraph::new(0, &[]).await;
        let (girth, _) = empty.trace("gds.girth.stream", &[], &record(&[])).await;
        assert_eq!(
            girth,
            vec![vec![Value::Float(f64::INFINITY)]],
            "girth without a cycle"
        );
        let g = TraceGraph::with_nodes(
            &[":N", ":N", ":M"],
            &[(0, 1, "R", Some(1.0)), (1, 0, "R", Some(2.0))],
        )
        .await;
        let outside = g.node(2);
        let n = |s: &str| record(&[("nodeLabels", Value::List(vec![txt(s)].into()))]);
        let cases: Vec<(&str, Vec<Value>, Value, &str, &str)> = vec![
            (
                "gds.countSpanningTrees.stream",
                vec![],
                record(&[]),
                "22N11",
                "151 rootNode needed unless UNDIRECTED",
            ),
            (
                "gds.spanningTree.stream",
                vec![],
                record(&[]),
                "22N11",
                "147 sourceNode missing",
            ),
            (
                "gds.spanningTree.stream",
                vec![],
                with(&n("N"), "sourceNode", outside.clone()),
                "22N11",
                "147 node outside",
            ),
            (
                "gds.kSpanningTree.write",
                vec![],
                record(&[
                    ("sourceNode", g.node(0)),
                    ("k", int(0)),
                    ("writeProperty", txt("k")),
                ]),
                "22N11",
                "149 k below 1",
            ),
            (
                "gds.steinerTree.stream",
                vec![],
                with(
                    &with(&n("N"), "sourceNode", g.node(0)),
                    "targetNodes",
                    Value::List(vec![outside.clone()].into()),
                ),
                "22N11",
                "153 target outside",
            ),
            (
                "gds.maxFlow.stream",
                vec![],
                record(&[("targetNodes", Value::List(vec![g.node(1)].into()))]),
                "22N11",
                "155 sourceNodes missing",
            ),
            (
                "gds.k1coloring.stream",
                vec![],
                record(&[("maxIterations", int(0))]),
                "22N11",
                "176 maxIterations below 1",
            ),
            (
                "gds.triangleCount.stream",
                vec![],
                record(&[("labelFilter", Value::List(vec![txt("N")].into()))]),
                "22N11",
                "120 labelFilter",
            ),
            (
                "graph_coloring.color_graph",
                vec![record(&[("strategy", txt("no_such"))])],
                record(&[]),
                "22N11",
                "173 unknown strategy",
            ),
            (
                "graph_coloring.color_graph",
                vec![record(&[("population_size", int(10))])],
                record(&[]),
                "22N11",
                "173 a MAGE metaheuristic key",
            ),
            (
                "graph_coloring.color_graph",
                vec![record(&[("strategy", int(1))])],
                record(&[]),
                "22G03",
                "173 strategy not a string",
            ),
            (
                "graph_coloring.color_graph",
                vec![txt("x")],
                record(&[]),
                "22G03",
                "173 parameters not a record",
            ),
            (
                "set_cover.greedy",
                vec![txt("x"), Value::List(vec![].into())],
                record(&[]),
                "22G03",
                "178 elements not a list",
            ),
        ];
        for (proc, args, cfg, want, what) in cases {
            assert_eq!(
                code(&g, proc, &args, cfg).await.as_deref(),
                Some(want),
                "{what}"
            );
        }
        // 157: a negative-cost cycle with capacity fails with 22N11.
        rows(
            &empty.s,
            "INSERT (a:N {uid: 0})-[:R {capacity: 1e0, cost: -1e0}]->(b:N {uid: 1}), (b)-[:R {capacity: 1e0, cost: -1e0}]->(a)",
            Parameters::new(),
        )
        .await
        .expect("insert");
        let (ids, _) = empty.trace("gds.wcc.stream", &[], &record(&[])).await;
        let (a, b) = (ids[0][0].clone(), ids[1][0].clone());
        let cfg = record(&[
            ("sourceNodes", Value::List(vec![a].into())),
            ("targetNodes", Value::List(vec![b].into())),
            ("capacityProperty", txt("capacity")),
            ("costProperty", txt("cost")),
        ]);
        assert_eq!(
            code(&empty, "gds.maxFlow.minCost.stream", &[], cfg)
                .await
                .as_deref(),
            Some("22N11"),
            "157 negative-cost cycle"
        );
        empty.db.close().await.unwrap();
        g.db.close().await.unwrap();
    });
}
