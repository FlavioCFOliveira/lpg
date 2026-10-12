//! rmp #71 pass B: the mechanical review of the PageRank pilot (task #63),
//! the centrality, similarity and link-prediction entries (task #65) and
//! the community-detection and clustering entries (task #66).
//!
//! * **Reference cases** ([`reference_traces`]): each case of
//!   `testdata/expected/trace_b.json` names a trace graph, a call and the
//!   event sequence of the algorithm's reference executed on that graph,
//!   written by `docs/evidence/lab-sprint-5/trace_ref_b.py` (each reference
//!   function cites its source, file and line). The runner and the trace
//!   graphs are pass A's ([`super::tests_trace_a`]). A case may also give
//!   the expected rows, and `differs`: an event sequence lpg must not
//!   produce (the sequential local move, which a colour-class step is
//!   not). GDS functions (ALG-107, ALG-112 to ALG-117) are called once,
//!   without configuration keys, and compared by rows.
//! * Randomised entries are compared step by step too: the reference
//!   script ports lpg's generator (`ChaCha8Rng` and the `rand` samplers),
//!   which [`python_generator_is_chacha8`] checks against the real one.
//! * The other tests: the round counts of 063 and 065 against GDS's, the
//!   regressions of the review, and the semantic edge cases.

use std::collections::BTreeMap;

use rand::{Rng, RngExt};

use super::tests_paths::{current, int, txt};
use super::tests_trace_a::{build, value};
use super::trace::{Recording, TraceGraph, assert_steps, only};
use crate::Parameters;
use crate::test_support::algo::{Fixture, expected_dir, record, rows, with};
use crate::types::Value;

/// The reference file.
fn reference() -> serde_json::Value {
    let text = std::fs::read_to_string(expected_dir().join("trace_b.json")).expect("trace_b.json");
    serde_json::from_str(&text).expect("JSON")
}

/// The trace graph `spec` of the reference file: pass A's, or a fixture.
async fn build_b(spec: &serde_json::Value) -> TraceGraph {
    match spec.get("fixture").and_then(serde_json::Value::as_str) {
        Some(name) => TraceGraph::fixture(name).await,
        None => build(spec).await,
    }
}

/// The rows of a GDS function called with `args` only.
async fn call_function(
    g: &TraceGraph,
    proc: &str,
    args: &[Value],
) -> (Result<Vec<Vec<Value>>, crate::Error>, Vec<String>) {
    let mut ps = Parameters::new();
    let mut list = Vec::new();
    for (i, a) in args.iter().enumerate() {
        ps.insert(format!("a{i}"), a.clone());
        list.push(format!("$a{i}"));
    }
    let quoted: Vec<String> = proc.split('.').map(|x| format!("`{x}`")).collect();
    let text = format!("CALL {}({})", quoted.join("."), list.join(", "));
    let rec = Recording::start();
    let out = rows(&g.s, &text, ps).await;
    (out, rec.take())
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
    let mut args: Vec<Value> = c["args"]
        .as_array()
        .expect("args")
        .iter()
        .map(|x| value(g, x))
        .collect();
    let strs = |key: &str| -> Vec<String> {
        c.get(key)
            .and_then(|x| x.as_array())
            .map(|xs| xs.iter().map(|x| x.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    };
    let kinds = strs("kinds");
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    let want = strs("want");
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
    let mut runs = Vec::new();
    if c.get("function").and_then(serde_json::Value::as_bool) == Some(true) {
        if c["config"].as_object().is_some_and(|m| !m.is_empty()) {
            args.push(value(g, &c["config"]));
        }
        runs.push(("once".to_string(), call_function(g, proc, &args).await));
    } else {
        for conc in c["concurrency"].as_array().expect("concurrency") {
            let config = with(&value(g, &c["config"]), "concurrency", value(g, conc));
            g.workers();
            let out = g.try_trace(proc, &args, &config).await;
            // At 16 a parallel entry runs its blocks on several workers.
            if conc.as_u64() == Some(16) && g.workers().unwrap_or(1) < 2 {
                failures.push(format!("{name}: one worker at concurrency 16"));
            }
            runs.push((format!("concurrency {conc}"), out));
        }
    }
    for (how, (result, events)) in runs {
        let rows = match result {
            Ok(rows) => rows,
            Err(e) => {
                failures.push(format!("{name} at {how}: {e}"));
                continue;
            }
        };
        if let Some(want_rows) = &want_rows
            && rows != *want_rows
        {
            failures.push(format!(
                "{name} at {how}: rows {rows:?} instead of {want_rows:?}"
            ));
        }
        let got = only(&events, &kinds);
        if let Some(d) = c.get("differs") {
            let d: Vec<String> = d
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect();
            if got == d {
                failures.push(format!("{name} at {how}: equals the `differs` sequence"));
            }
        }
        let result = std::panic::catch_unwind(|| {
            assert_steps(&format!("{name} at {how}"), &got, &want);
        });
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

/// Runs the cases whose names start with one of `prefixes` (all with
/// `""`) and asserts that none fails.
fn run_cases(prefixes: &[&str]) {
    current(async {
        let all = reference();
        let mut graphs: BTreeMap<String, TraceGraph> = BTreeMap::new();
        let mut failures = Vec::new();
        let filter = std::env::var("TRACE_CASE").unwrap_or_default();
        let mut ran = 0;
        for (name, c) in all["cases"].as_object().expect("cases") {
            if !name.starts_with(&filter) || !prefixes.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            ran += 1;
            failures.extend(check_case(name, c, &all, &mut graphs).await);
        }
        assert!(ran > 0, "no case matched {prefixes:?}");
        eprintln!("rmp #71 pass B: {ran} reference cases");
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

/// rmp #71 pass B: every reference case (module documentation).
#[test]
fn reference_traces() {
    run_cases(&[""]);
}

/// The reference script's port of `ChaCha8Rng` and of the `rand` samplers
/// lpg calls (`random_range` on `usize`, exclusive and inclusive,
/// `random::<f64>`, `next_u32`, `next_u64`, `set_word_pos`) draws what
/// the real generator draws.
#[test]
fn python_generator_is_chacha8() {
    let all = reference();
    let mut draws = 0;
    for c in all["rng"].as_array().expect("rng") {
        let mut g = super::sched::rng(c["seed"].as_u64().unwrap(), 0);
        g.set_stream(c["stream"].as_u64().unwrap());
        if let Some(pos) = c["pos"].as_u64() {
            g.set_word_pos(u128::from(pos));
        }
        let ops = c["ops"].as_array().unwrap();
        let want = c["results"].as_array().unwrap();
        for (op, w) in ops.iter().zip(want) {
            let op = op.as_array().unwrap();
            let arg = |i: usize| usize::try_from(op[i].as_u64().unwrap()).unwrap();
            let got = match op[0].as_str().unwrap() {
                "u64" => serde_json::Value::from(g.next_u64().to_string()),
                "u32" => serde_json::Value::from(g.next_u32()),
                "f64" => serde_json::Value::from(g.random::<f64>()),
                "range" => serde_json::Value::from(g.random_range(arg(1)..arg(2))),
                "incl" => serde_json::Value::from(g.random_range(arg(1)..=arg(2))),
                other => panic!("operation {other}"),
            };
            assert_eq!(&got, w, "seed {} stream {}: {op:?}", c["seed"], c["stream"]);
            draws += 1;
        }
    }
    assert_eq!(draws, 600);
}

/// rmp #71, known inputs of #65 and #66: 063 and 065 round counts differ
/// from GDS's. `trace_ref_b.py` `analysis` runs, on the lab fixtures and
/// UNDIRECTED as GDS ran them, GDS's label propagation (asynchronous in
/// node order, the lowest label on a tie) and the colour-class modularity
/// optimisation, on GDS's node numbering (import order) and on lpg's
/// (the string order of `id:ID`). This test checks that the ports give
/// GDS's recorded results on GDS's numbering, and that lpg's runs give
/// the ports' results on lpg's numbering: the differences are the node
/// numbering (065) and the tie rule with the update order (063).
#[test]
fn round_counts_against_gds_follow_numbering_and_tie_rule() {
    current(async {
        let all = reference();
        let pair = |v: &serde_json::Value| (v[0].as_i64().unwrap(), v[1].as_i64().unwrap());
        for name in ["rc24", "karate", "planted60"] {
            let a = &all["analysis"][name];
            assert_eq!(
                pair(&a["lpa gds numbering, gds tie, gds order"]),
                pair(&a["gds_lpa"]),
                "{name}: GDS's LPA ported, on GDS's numbering"
            );
            let mo = &a["mo gds numbering, lpg classes"];
            assert_eq!(
                (mo["lpg_rounds"].as_i64(), mo["communities"].as_i64()),
                (a["gds_mo"][0].as_i64(), a["gds_mo"][1].as_i64()),
                "{name}: modularity optimisation on GDS's numbering"
            );
            let f = Fixture::load(name, 32 << 20).await;
            let summary = |proc: &'static str| {
                let s = &f.s;
                async move {
                    let config = record(&[
                        ("orientation", txt("UNDIRECTED")),
                        ("writeProperty", txt("c")),
                    ]);
                    let (text, ps) = super::tests_paths::call_text(proc, &[], &config);
                    rows(s, &text, ps).await.expect(proc).remove(0)
                }
            };
            let lpa = summary("gds.labelPropagation.write").await;
            let want = pair(&a["lpa lpg numbering, lpg tie, classes order"]);
            assert_eq!(
                (lpa[0].clone(), lpa[2].clone()),
                (int(want.0), int(want.1)),
                "{name}: lpg's LPA (ranIterations, communityCount)"
            );
            let mo = summary("gds.modularityOptimization.write").await;
            let lm = &a["mo lpg numbering, lpg classes"];
            assert_eq!(
                (mo[6].clone(), mo[8].clone()),
                (
                    int(lm["lpg_rounds"].as_i64().unwrap()),
                    int(lm["communities"].as_i64().unwrap())
                ),
                "{name}: lpg's modularity optimisation (ranIterations, communityCount)"
            );
            f.db.close().await.unwrap();
        }
    });
}

/// rmp #71 regression (ALG-061, ALG-062): with
/// `includeIntermediateCommunities`, GDS writes the list of intermediate
/// communities (`LouvainWriteStep.java:68-72`, `LeidenWriteStep.java:
/// 71-75`); lpg wrote the final community.
#[test]
fn louvain_and_leiden_write_the_intermediate_communities() {
    current(async {
        let all = reference();
        let g = build(&all["graphs"]["cm"]).await;
        for proc in ["gds.louvain", "gds.leiden"] {
            let config = record(&[("includeIntermediateCommunities", Value::Bool(true))]);
            let (stream, _) = g.trace(&format!("{proc}.stream"), &[], &config).await;
            let want: BTreeMap<usize, Value> = stream
                .iter()
                .map(|r| (g.uid(&r[0]), r[1].clone()))
                .collect();
            assert!(want.values().all(|v| matches!(v, Value::List(_))));
            let property = format!("{}Levels", &proc[4..]);
            let config = with(&config, "writeProperty", txt(&property));
            g.trace(&format!("{proc}.write"), &[], &config).await;
            let got: BTreeMap<usize, Value> = rows(
                &g.s,
                &format!("MATCH (n) RETURN n.uid, n.{property}"),
                Parameters::new(),
            )
            .await
            .unwrap()
            .into_iter()
            .map(|r| {
                let Value::Int(u) = r[0] else { panic!() };
                (usize::try_from(u).unwrap(), r[1].clone())
            })
            .collect();
            assert_eq!(got, want, "{proc}.write: the intermediate communities");
        }
        g.db.close().await.unwrap();
    });
}

/// rmp #71 regression (ALG-106): `perturbationRate` lets a candidate of
/// the same similarity as a full list's worst replace it (GDS
/// `NeighborList.add`:107-115; `knn-specific-algo-config.adoc:21`); lpg
/// let any candidate no better than the worst replace it.
#[test]
fn knn_perturbation_replaces_only_an_equal_neighbour() {
    run_cases(&["106 perturbationRate 1"]);
}

/// rmp #71 regressions (ALG-080): GDS stops from the second iteration at
/// most `(long) (n × deltaThreshold)` swaps (`KmeansIterationStopper`),
/// keeps the last assignment (`KmeansTask.calculateFinalDistance`) and
/// the restart of least average distance (`Kmeans.updateBestSolution`);
/// lpg stopped below `n × deltaThreshold` swaps, reassigned the points to
/// the final centroids and kept the restart of least inertia.
#[test]
fn kmeans_stops_and_reports_as_gds() {
    run_cases(&[
        "080 maxIterations 1",
        "080 deltaThreshold 0.1",
        "080 restarts",
    ]);
}

/// rmp #71 regressions (ALG-080, GDS `Kmeans.compute`,
/// `CommunityAlgorithms.kMeans`, `KmeansBaseConfig.
/// nodePropertyTypeValidation`): more clusters than nodes put each node in
/// its own community; seed centroids need exactly `k` and one restart; a
/// number property is refused. lpg failed the first and accepted the
/// others.
#[test]
fn kmeans_validates_as_gds() {
    current(async {
        let all = reference();
        let g = build_b(&all["graphs"]["pts"]).await;
        let base = record(&[("nodeProperty", txt("vec"))]);
        let (rows, _) = g
            .trace("gds.kmeans.stream", &[], &with(&base, "k", int(11)))
            .await;
        let want: Vec<Vec<Value>> = (0..10)
            .map(|i| vec![int(i), int(i), Value::Float(0.0), Value::Float(-1.0)])
            .collect();
        assert_eq!(rows, want, "k above the node count");
        let seeds = |n: usize| {
            Value::List(
                (0..n)
                    .map(|i| Value::List(vec![Value::Float(i as f64), Value::Float(0.0)].into()))
                    .collect::<Vec<_>>()
                    .into(),
            )
        };
        for (config, what) in [
            (with(&base, "seedCentroids", seeds(2)), "2 seeds, k 10"),
            (
                with(
                    &with(&with(&base, "seedCentroids", seeds(2)), "k", int(2)),
                    "numberOfRestarts",
                    int(2),
                ),
                "seeds and restarts",
            ),
            (record(&[("nodeProperty", txt("x"))]), "a number property"),
        ] {
            let (out, _) = g.try_trace("gds.kmeans.stream", &[], &config).await;
            assert_eq!(out.expect_err(what).code(), "22N11", "{what}");
        }
        let config = with(&with(&base, "seedCentroids", seeds(2)), "k", int(2));
        let (rows, _) = g.trace("gds.kmeans.stream", &[], &config).await;
        assert_eq!(rows.len(), 10, "2 seeds, k 2");
        g.db.close().await.unwrap();
    });
}

/// rmp #71 regression (ALG-082): GDS's modularity counts every community
/// value, a negative one included (`ModularityCalculator.create`,
/// `RelationshipCountCollector.run`); lpg skipped negative communities and
/// their edges. Conductance skips them, as GDS's (`Conductance.java:
/// 276-280`).
#[test]
fn modularity_counts_a_negative_community() {
    current(async {
        let g = TraceGraph::with_nodes(
            &[
                ":N {community: 0,",
                ":N {community: 0,",
                ":N {community: -1,",
                ":N {community: -1,",
            ],
            &[
                (0, 1, "R", None),
                (1, 2, "R", None),
                (2, 3, "R", None),
                (3, 2, "R", None),
            ],
        )
        .await;
        let config = record(&[("communityProperty", txt("community"))]);
        let (rows, _) = g.trace("gds.modularity.stream", &[], &config).await;
        // m = 4; community 0: e = 1, K = 2; community -1: e = 2, K = 2.
        assert_eq!(
            rows,
            vec![
                vec![int(-1), Value::Float((2.0 - 4.0 / 4.0) / 4.0)],
                vec![int(0), Value::Float((1.0 - 4.0 / 4.0) / 4.0)]
            ]
        );
        let (rows, _) = g.trace("gds.conductance.stream", &[], &config).await;
        assert_eq!(rows, vec![vec![int(0), Value::Float(0.5)]]);
        g.db.close().await.unwrap();
    });
}

/// The error codes of the pass B entries (TS-25: a value outside its
/// domain or a missing required key fails with 22N11, a value of the
/// wrong type with 22G03).
#[test]
fn errors_of_the_pass_b_entries() {
    current(async {
        let all = reference();
        let g = build_b(&all["graphs"]["pts"]).await;
        let r = |pairs: &[(&str, Value)]| record(pairs);
        let f = |x: f64| Value::Float(x);
        let cases: Vec<(&str, Vec<Value>, Value, &str)> = vec![
            (
                "gds.pageRank.stream",
                vec![],
                r(&[("dampingFactor", f(1.0))]),
                "22N11",
            ),
            (
                "gds.pageRank.stream",
                vec![],
                r(&[("maxIterations", int(0))]),
                "22N11",
            ),
            (
                "gds.pageRank.stream",
                vec![],
                r(&[("tolerance", txt("x"))]),
                "22G03",
            ),
            (
                "gds.eigenvector.stream",
                vec![],
                r(&[("dampingFactor", f(0.5))]),
                "22N11",
            ),
            ("gds.degree.stream", vec![], r(&[("foo", int(1))]), "22N11"),
            (
                "gds.betweenness.stream",
                vec![],
                r(&[("samplingSize", int(-1))]),
                "22N11",
            ),
            (
                "gds.hits.stream",
                vec![],
                r(&[("orientation", txt("UNDIRECTED"))]),
                "22N11",
            ),
            (
                "gds.influenceMaximization.celf.stream",
                vec![],
                r(&[]),
                "22N11",
            ),
            (
                "gds.nodeSimilarity.stream",
                vec![],
                r(&[("topK", int(1)), ("bottomK", int(1))]),
                "22N11",
            ),
            (
                "gds.nodeSimilarity.stream",
                vec![],
                r(&[("degreeCutoff", int(3)), ("upperDegreeCutoff", int(2))]),
                "22N11",
            ),
            ("gds.knn.stream", vec![], r(&[]), "22N11"),
            ("gds.sllpa.stream", vec![], r(&[]), "22N11"),
            ("gds.maxkcut.stream", vec![], r(&[("k", int(1))]), "22N11"),
            (
                "gds.kmeans.stream",
                vec![],
                r(&[("nodeProperty", txt("vec")), ("k", int(0))]),
                "22N11",
            ),
            (
                "gds.hdbscan.stream",
                vec![],
                r(&[("nodeProperty", txt("vec")), ("minClusterSize", int(1))]),
                "22N11",
            ),
            ("gds.modularity.stream", vec![], r(&[]), "22N11"),
            (
                "katz_centrality.get",
                vec![txt("x"), f(0.01)],
                r(&[]),
                "22G03",
            ),
        ];
        for (proc, args, config, code) in cases {
            let (out, _) = g.try_trace(proc, &args, &config).await;
            let e = out.expect_err(&format!("{proc} {config:?}"));
            assert_eq!(e.code(), code, "{proc} {config:?}: {e}");
        }
        g.db.close().await.unwrap();
    });
}

/// rmp #71 regressions: the GDS 2026.09.0 parameter domains of the pass B
/// entries (GDS @ `417d7ecf973d`): `InfluenceMaximizationBaseConfig`
/// (`propagationProbability` in [0.01, 1]), `NodeSimilarityBaseConfig`
/// (`similarityCutoff` in [0, 1]), `KnnBaseConfig` (`sampleRate` in
/// (0, 1]), `ApproxMaxKCutBaseConfig` (`k` at most 127),
/// `LouvainBaseConfig.validate` and `LeidenBaseConfig.validate`
/// (`includeIntermediateCommunities` excludes `consecutiveIds`),
/// `HitsConfig` (no whitespace in `authProperty`, `hubProperty`), Pregel
/// `Partitioning.parse` (`partitioning` one of RANGE, DEGREE, AUTO). lpg
/// accepted each of these values.
#[test]
fn parameter_domains_as_gds() {
    current(async {
        let all = reference();
        let g = build_b(&all["graphs"]["pts"]).await;
        let r = |pairs: &[(&str, Value)]| record(pairs);
        let f = |x: f64| Value::Float(x);
        let both = [
            ("includeIntermediateCommunities", Value::Bool(true)),
            ("consecutiveIds", Value::Bool(true)),
        ];
        let cases: Vec<(&str, Value)> = vec![
            (
                "gds.influenceMaximization.celf.stream",
                r(&[
                    ("seedSetSize", int(1)),
                    ("propagationProbability", f(0.005)),
                ]),
            ),
            (
                "gds.nodeSimilarity.stream",
                r(&[("similarityCutoff", f(1.5))]),
            ),
            (
                "gds.knn.stream",
                r(&[("nodeProperties", txt("x")), ("sampleRate", f(0.0))]),
            ),
            ("gds.maxkcut.stream", r(&[("k", int(128))])),
            ("gds.louvain.stream", r(&both)),
            ("gds.leiden.stream", r(&both)),
            ("gds.hits.stream", r(&[("hubProperty", txt("a hub"))])),
            ("gds.hits.stream", r(&[("partitioning", txt("FOO"))])),
            (
                "gds.sllpa.stream",
                r(&[("maxIterations", int(3)), ("partitioning", txt("FOO"))]),
            ),
        ];
        let mut accepted = Vec::new();
        for (proc, config) in cases {
            let (out, _) = g.try_trace(proc, &[], &config).await;
            match out {
                Err(e) if e.code() == "22N11" => {}
                other => accepted.push(format!("{proc} {config:?}: {other:?}")),
            }
        }
        assert!(
            accepted.is_empty(),
            "{} not refused:\n{}",
            accepted.len(),
            accepted.join("\n")
        );
        // The bounds themselves are accepted.
        for (proc, config) in [
            (
                "gds.influenceMaximization.celf.stream",
                r(&[("seedSetSize", int(1)), ("propagationProbability", f(0.01))]),
            ),
            (
                "gds.nodeSimilarity.stream",
                r(&[("similarityCutoff", f(1.0))]),
            ),
            ("gds.maxkcut.stream", r(&[("k", int(127))])),
            ("gds.hits.stream", r(&[("partitioning", txt("degree"))])),
        ] {
            let (out, _) = g.try_trace(proc, &[], &config).await;
            out.unwrap_or_else(|e| panic!("{proc} {config:?}: {e}"));
        }
        g.db.close().await.unwrap();
    });
}

/// rmp #71, known input of #65: HITS's write mode counts two properties
/// per node, as GDS's (`proc/centrality/.../hits/HitsWriteProcTest.java:
/// 82`: 6 on 3 nodes, `ranIterations` 4 with `hitsIterations` 1).
#[test]
fn hits_write_counts_two_properties_per_node() {
    current(async {
        let all = reference();
        let g = build_b(&all["graphs"]["pr"]).await;
        let config = record(&[("hitsIterations", int(1))]);
        let (rows, _) = g.trace("gds.hits.write", &[], &config).await;
        assert_eq!((rows[0][0].clone(), rows[0][5].clone()), (int(4), int(12)));
        g.db.close().await.unwrap();
    });
}
