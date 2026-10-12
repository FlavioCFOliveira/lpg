//! rmp #71 pass A: the mechanical review of the traversal, path, cycle,
//! isomorphism and derived-structure entries (tasks #64 and #69).
//!
//! * **Reference cases** ([`reference_traces`]): each case of
//!   `testdata/expected/trace_a.json` names a trace graph, a call and the
//!   event sequence of the algorithm's reference executed on that graph,
//!   written by `docs/evidence/lab-sprint-5/trace_ref_a.py` (each reference
//!   function cites its source, file and line). The test runs the call on
//!   lpg at each listed `concurrency` (1 and 16 for parallel entries: a
//!   graph of n ≤ 64 nodes has n blocks of one node) and compares lpg's
//!   events of the case's kinds with the reference, step by step.
//! * The other tests cover what an event list does not: the random walk's
//!   step distribution, the tour and route steps, the write modes one by
//!   one, the semantic edge cases, and the regressions of the review.

use std::collections::BTreeMap;

use super::tests_paths::{current, int, strs, txt};
use super::trace::{TraceGraph, assert_steps, only};
use crate::test_support::algo::{expected_dir, record, with};
use crate::types::Value;

fn nodes(g: &TraceGraph, xs: &[usize]) -> Value {
    Value::List(xs.iter().map(|i| g.node(*i)).collect::<Vec<_>>().into())
}

/// A JSON value of a case as a parameter: `{"node": i}` is node `i`.
fn value(g: &TraceGraph, v: &serde_json::Value) -> Value {
    use serde_json::Value as J;
    match v {
        J::Null => Value::Null,
        J::Bool(b) => Value::Bool(*b),
        J::Number(x) if x.is_f64() => Value::Float(x.as_f64().expect("a float")),
        J::Number(x) => int(x.as_i64().expect("an integer")),
        J::String(x) => txt(x),
        J::Array(xs) => Value::List(xs.iter().map(|x| value(g, x)).collect::<Vec<_>>().into()),
        J::Object(m) => match m.get("node") {
            Some(i) if m.len() == 1 => {
                g.node(usize::try_from(i.as_u64().expect("a node")).unwrap())
            }
            _ => {
                let pairs: Vec<(String, Value)> =
                    m.iter().map(|(k, x)| (k.clone(), value(g, x))).collect();
                let refs: Vec<(&str, Value)> =
                    pairs.iter().map(|(k, x)| (k.as_str(), x.clone())).collect();
                record(&refs)
            }
        },
    }
}

/// The trace graph `spec` of the reference file.
async fn build(spec: &serde_json::Value) -> TraceGraph {
    let n = usize::try_from(spec["n"].as_u64().expect("n")).unwrap();
    let labels: Vec<String> = match spec.get("nodes") {
        Some(l) => l
            .as_array()
            .expect("labels")
            .iter()
            .map(|x| x.as_str().expect("a label").to_string())
            .collect(),
        None => vec![":N".to_string(); n],
    };
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let edges: Vec<(usize, usize, String, Option<f64>)> = spec["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .map(|e| {
            (
                usize::try_from(e[0].as_u64().unwrap()).unwrap(),
                usize::try_from(e[1].as_u64().unwrap()).unwrap(),
                e[2].as_str().unwrap().to_string(),
                e[3].as_f64(),
            )
        })
        .collect();
    let edges: Vec<(usize, usize, &str, Option<f64>)> = edges
        .iter()
        .map(|(a, b, t, w)| (*a, *b, t.as_str(), *w))
        .collect();
    TraceGraph::with_nodes(&labels, &edges).await
}

/// rmp #71: every reference case (module documentation). Failures are
/// collected and reported together.
#[test]
fn reference_traces() {
    current(async {
        let text =
            std::fs::read_to_string(expected_dir().join("trace_a.json")).expect("trace_a.json");
        let all: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        let mut graphs: BTreeMap<String, TraceGraph> = BTreeMap::new();
        let mut failures = Vec::new();
        let filter = std::env::var("TRACE_CASE").unwrap_or_default();
        for (name, c) in all["cases"].as_object().expect("cases") {
            if !name.starts_with(&filter) {
                continue;
            }
            let gname = c["graph"].as_str().expect("graph").to_string();
            if !graphs.contains_key(&gname) {
                graphs.insert(gname.clone(), build(&all["graphs"][&gname]).await);
            }
            let g = &graphs[&gname];
            let proc = c["proc"].as_str().expect("proc");
            let args: Vec<Value> = c["args"]
                .as_array()
                .expect("args")
                .iter()
                .map(|x| value(g, x))
                .collect();
            let kinds: Vec<&str> = c["kinds"]
                .as_array()
                .expect("kinds")
                .iter()
                .map(|x| x.as_str().unwrap())
                .collect();
            let want: Vec<&str> = c["want"]
                .as_array()
                .expect("want")
                .iter()
                .map(|x| x.as_str().unwrap())
                .collect();
            for conc in c["concurrency"].as_array().expect("concurrency") {
                let config = with(&value(g, &c["config"]), "concurrency", value(g, conc));
                g.workers();
                let (rows, events) = g.try_trace(proc, &args, &config).await;
                // At 16 a parallel entry runs its blocks on several workers.
                if conc.as_u64() == Some(16) && g.workers().unwrap_or(1) < 2 {
                    failures.push(format!("{name}: one worker at concurrency 16"));
                }
                if let Err(e) = &rows {
                    failures.push(format!("{name} at concurrency {conc}: {e}"));
                    continue;
                }
                if let Some(want_rows) = c.get("rows") {
                    let want_rows: Vec<Vec<Value>> = want_rows
                        .as_array()
                        .expect("rows")
                        .iter()
                        .map(|r| {
                            r.as_array()
                                .expect("a row")
                                .iter()
                                .map(|x| value(g, x))
                                .collect()
                        })
                        .collect();
                    let rows = rows.as_ref().expect("rows");
                    if *rows != want_rows {
                        failures.push(format!("{name}: rows {rows:?} instead of {want_rows:?}"));
                    }
                }
                let got = only(&events, &kinds);
                let result = std::panic::catch_unwind(|| {
                    assert_steps(&format!("{name} at concurrency {conc}"), &got, &want);
                });
                if let Err(e) = result {
                    let msg = e
                        .downcast_ref::<String>()
                        .cloned()
                        .unwrap_or_else(|| "a failure".to_string());
                    failures.push(msg);
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    });
}

/// rmp #71, ALG-001 and ALG-002 against GDS (known input: lpg's order
/// differed from GDS's in 5 of 7 lab cases). `trace_ref_a.py` runs GDS's
/// BFS and DFS (ported from `BFS.java`, `BFSTask.java`, `DFS.java`) on
/// each fixture twice: on GDS's node numbering (Neo4j's import order,
/// the numeric `uid`), where it reproduces GDS's 7 outputs exactly, and on
/// lpg's (element identifiers, which lpg's CSV import assigns in the
/// string order of `id:ID`), where it gives the orders this test expects
/// of lpg. So lpg's orders are GDS's algorithm on lpg's numbering; they
/// equal GDS's where both numberings give the same neighbour order (2 of
/// 7). GDS documents only that BFS visits "in order of increasing
/// distance" (`doc/.../algorithms/bfs.adoc:17`), not a neighbour order.
#[test]
fn bfs_and_dfs_orders_are_gds_executions_on_lpg_numbering() {
    current(async {
        let text =
            std::fs::read_to_string(expected_dir().join("trace_a.json")).expect("trace_a.json");
        let all: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        let mut equal = 0;
        for (file, c) in all["fixtures"].as_object().expect("fixtures") {
            let f =
                crate::test_support::algo::Fixture::load(c["fixture"].as_str().unwrap(), 32 << 20)
                    .await;
            let node = |u: &serde_json::Value| {
                Value::Node(f.ids[usize::try_from(u.as_u64().unwrap()).unwrap()])
            };
            let mut cfg = vec![("sourceNode", node(&c["source"]))];
            if let Some(d) = c["maxDepth"].as_i64() {
                cfg.push(("maxDepth", int(d)));
            }
            let targets: Vec<Value> = c["targets"].as_array().unwrap().iter().map(node).collect();
            cfg.push(("targetNodes", Value::List(targets.into())));
            let rows = f
                .call(c["proc"].as_str().unwrap(), record(&cfg))
                .await
                .unwrap();
            let Value::List(l) = &rows[0][1] else {
                panic!()
            };
            let got: Vec<i64> = l
                .iter()
                .map(|v| match v {
                    Value::Int(id) => f.uids[&id.cast_unsigned()],
                    _ => panic!(),
                })
                .collect();
            let want: Vec<i64> = c["lpg"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            assert_eq!(
                got, want,
                "{file}: lpg's order is not GDS's algorithm on lpg's numbering"
            );
            equal += usize::from(c["gds_equal"].as_bool().unwrap());
        }
        assert_eq!(equal, 2, "orders equal to GDS's");
    });
}

/// The `dir` trace graph of `trace_ref_a.py` (weighted, a parallel edge,
/// a self-loop), with optional node bodies.
const DIR: &[(usize, usize, &str, Option<f64>)] = &[
    (0, 1, "R", Some(4.0)),
    (0, 2, "R", Some(1.0)),
    (2, 1, "R", Some(2.0)),
    (1, 3, "R", Some(1.0)),
    (2, 3, "R", Some(5.0)),
    (3, 4, "R", Some(3.0)),
    (1, 4, "R", Some(7.0)),
    (4, 5, "R", Some(1.0)),
    (2, 2, "R", Some(1.0)),
    (0, 2, "R", Some(3.0)),
    (5, 3, "R", Some(2.0)),
    (6, 0, "R", Some(1.0)),
];

/// The integers of an event's text, in order.
fn ints_of(text: &str) -> Vec<usize> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .map(|x| x.parse().unwrap())
        .collect()
}

fn w() -> (&'static str, Value) {
    ("relationshipWeightProperty", txt("weight"))
}

/// rmp #71, ALG-006: every step draws from node2vec's biased
/// distribution (Grover and Leskovec 2016, "node2vec", §3.2.2: weight ×
/// 1/p back to the previous node, × 1 to a node the previous node has an
/// edge to, × 1/q otherwise; GDS `RandomWalkSampler.java` `walkOneStep`
/// tests `graph.exists(previous, next)`): the trace's cumulative weights
/// equal the definition's, summed in adjacency order, and the step takes
/// the first neighbour whose cumulative weight exceeds the draw. The
/// steps are the same at `concurrency` 1 and 16.
#[test]
fn random_walk_steps_follow_node2vec() {
    current(async {
        let g = TraceGraph::new(7, DIR).await;
        let (p, q) = (0.5, 2.0);
        // Outgoing edges of v in adjacency order (neighbour, edge index).
        let out = |v: usize| {
            let mut e: Vec<(usize, usize)> = DIR
                .iter()
                .enumerate()
                .filter(|(_, e)| e.0 == v)
                .map(|(i, e)| (e.1, i))
                .collect();
            e.sort_unstable();
            e
        };
        let mut seen = Vec::new();
        for c in [1, 16] {
            let cfg = record(&[
                ("walkLength", int(6)),
                ("walksPerNode", int(3)),
                ("returnFactor", Value::Float(p)),
                ("inOutFactor", Value::Float(q)),
                ("randomSeed", int(7)),
                ("concurrency", int(c)),
                w(),
            ]);
            let (rows, ev) = g.trace("gds.randomWalk.stream", &[], &cfg).await;
            let steps = only(&ev, &["step"]);
            assert!(steps.len() > 40, "{} steps", steps.len());
            for e in &steps {
                let (left, pick) = e["step: ".len()..].rsplit_once(" -> ").unwrap();
                let (left, u) = left.rsplit_once(' ').unwrap();
                let u: f64 = u.parse().unwrap();
                let (prev, rest) = left.split_once(' ').unwrap();
                let v: usize = rest.split_once(' ').unwrap().0.parse().unwrap();
                let prev: Option<usize> = (prev != "None").then(|| ints_of(prev)[0]);
                let mut total = 0.0;
                let mut want = Vec::new();
                for (t, i) in out(v) {
                    let mut x = DIR[i].3.unwrap();
                    if let Some(prev) = prev {
                        x *= if t == prev {
                            1.0 / p
                        } else if out(prev).iter().any(|(y, _)| *y == t) {
                            1.0
                        } else {
                            1.0 / q
                        };
                    }
                    total += x;
                    want.push((t, total));
                }
                assert_eq!(left, format!("{prev:?} {v} {want:?}"), "{e}");
                let at = want
                    .iter()
                    .position(|(_, c)| *c > u)
                    .unwrap_or(want.len() - 1);
                assert_eq!(pick, want[at].0.to_string(), "{e}");
            }
            seen.push((rows, steps));
        }
        assert_eq!(seen[0], seen[1], "the steps at concurrency 1 and 16");
    });
}

/// rmp #71 regression, ALG-006: GDS starts no walk from a node without
/// outgoing edges (`traversal/RandomWalkTask.java` `run`: `if
/// (graph.degree(nodeId) == 0) continue;`); lpg returned `walksPerNode`
/// one-node walks for it. On the DAG of the trace cases node 4 has no
/// outgoing edge.
#[test]
fn random_walk_starts_no_walk_from_a_node_without_out_edges() {
    current(async {
        let g = TraceGraph::new(
            6,
            &[
                (0, 1, "R", None),
                (0, 2, "R", None),
                (1, 3, "R", None),
                (2, 3, "R", None),
                (3, 4, "R", None),
                (5, 4, "R", None),
            ],
        )
        .await;
        let (rows, _) = g
            .trace(
                "gds.randomWalk.stream",
                &[],
                &record(&[
                    ("sourceNodes", nodes(&g, &[4, 0])),
                    ("walksPerNode", int(2)),
                    ("walkLength", int(3)),
                ]),
            )
            .await;
        let starts: Vec<usize> = rows.iter().map(|r| g.uids(&r[0])[0]).collect();
        assert_eq!(starts, vec![0, 0], "walks only from node 0");
        let (rows, _) = g
            .trace(
                "gds.randomWalk.stream",
                &[],
                &record(&[("walksPerNode", int(1)), ("walkLength", int(3))]),
            )
            .await;
        let starts: Vec<usize> = rows.iter().map(|r| g.uids(&r[0])[0]).collect();
        assert_eq!(starts, vec![0, 1, 2, 3, 5], "no walk from node 4");
    });
}

/// rmp #71, ALG-009, 018, 020, 021, 025, 030: lpg calls petgraph 0.8.3
/// on the projection; the reference is the same petgraph function on a
/// petgraph `DiGraph` of the same edges (the trace graphs are tie-free).
#[test]
fn petgraph_entries_equal_petgraph_on_the_same_graph() {
    use petgraph::graph::{DiGraph, NodeIndex};
    current(async {
        let neg: &[(usize, usize, &str, Option<f64>)] = &[
            (0, 1, "R", Some(4.0)),
            (0, 2, "R", Some(2.0)),
            (2, 1, "R", Some(-3.0)),
            (1, 3, "R", Some(2.0)),
            (2, 3, "R", Some(5.0)),
            (3, 4, "R", Some(-1.0)),
            (4, 2, "R", Some(3.0)),
            (5, 0, "R", Some(1.0)),
        ];
        let negcyc: Vec<_> = neg
            .iter()
            .map(|e| {
                if (e.0, e.1) == (4, 2) {
                    (4, 2, "R", Some(-2.0))
                } else {
                    *e
                }
            })
            .collect();
        let digraph = |n: usize, edges: &[(usize, usize, &str, Option<f64>)]| {
            let mut d: DiGraph<(), f64> = DiGraph::new();
            for _ in 0..n {
                d.add_node(());
            }
            for e in edges {
                d.add_edge(NodeIndex::new(e.0), NodeIndex::new(e.1), e.3.unwrap_or(1.0));
            }
            d
        };
        let ix = NodeIndex::new;
        // 009: every pair of `dir`.
        let g = TraceGraph::new(7, DIR).await;
        let d = digraph(7, DIR);
        for a in 0..7 {
            for b in 0..7 {
                let (rows, _) = g
                    .trace(
                        "gds.hasPathConnecting.stream",
                        &[],
                        &record(&[("sourceNode", g.node(a)), ("targetNode", g.node(b))]),
                    )
                    .await;
                let want = petgraph::algo::has_path_connecting(&d, ix(a), ix(b), None);
                assert_eq!(rows[0][2], Value::Bool(want), "009 {a} {b}");
            }
        }
        // 030: k-th shortest path lengths, with and without a target.
        for (k, goal) in [(1, None), (2, None), (3, None), (2, Some(5))] {
            let mut cfg = vec![("sourceNode", g.node(0)), ("k", int(k)), w()];
            if let Some(t) = goal {
                cfg.push(("targetNode", g.node(t)));
            }
            let (_, ev) = g
                .trace("gds.kShortestPath.stream", &[], &record(&cfg))
                .await;
            let mut want: Vec<(usize, f64)> =
                petgraph::algo::k_shortest_path(&d, ix(0), goal.map(ix), k as usize, |e| {
                    *e.weight()
                })
                .into_iter()
                .map(|(v, c)| (v.index(), c))
                .collect();
            want.sort_unstable_by_key(|x| x.0);
            assert_steps(
                "030",
                &only(&ev, &["petgraph"]),
                &[&format!("petgraph: {want:?}")],
            );
        }
        // 018: A* with GDS's haversine heuristic.
        let coords = [
            (38.7, -9.1),
            (41.1, -8.6),
            (37.0, -7.9),
            (40.2, -8.4),
            (38.5, -7.9),
            (41.5, -8.4),
            (39.0, -9.0),
        ];
        let bodies: Vec<String> = coords
            .iter()
            .map(|(a, b)| format!(":N {{lat: {a:e}, lng: {b:e},"))
            .collect();
        let bodies: Vec<&str> = bodies.iter().map(String::as_str).collect();
        let geo = TraceGraph::with_nodes(&bodies, DIR).await;
        for t in [3, 5] {
            let (_, ev) = geo
                .trace(
                    "gds.shortestPath.astar.stream",
                    &[],
                    &record(&[
                        ("sourceNode", geo.node(0)),
                        ("targetNode", geo.node(t)),
                        ("latitudeProperty", txt("lat")),
                        ("longitudeProperty", txt("lng")),
                        w(),
                    ]),
                )
                .await;
            let want = petgraph::algo::astar(
                &d,
                ix(0),
                |v| v.index() == t,
                |e| *e.weight(),
                |v| {
                    super::haversine_nm(
                        coords[v.index()].0,
                        coords[v.index()].1,
                        coords[t].0,
                        coords[t].1,
                    )
                },
            )
            .map(|(c, path)| (c, path.iter().map(|v| v.index()).collect::<Vec<_>>()));
            assert_steps(
                "018",
                &only(&ev, &["petgraph"]),
                &[&format!("petgraph: {want:?}")],
            );
        }
        // 021: SPFA distances and predecessors; 025: Floyd-Warshall rows.
        let g = TraceGraph::new(6, neg).await;
        let d = digraph(6, neg);
        let (_, ev) = g
            .trace(
                "gds.spfa.stream",
                &[],
                &record(&[("sourceNode", g.node(0)), w()]),
            )
            .await;
        let want = petgraph::algo::spfa(&d, ix(0), |e| *e.weight()).unwrap();
        let preds: Vec<Option<usize>> = want
            .predecessors
            .iter()
            .map(|x| x.map(|v| v.index()))
            .collect();
        assert_steps(
            "021",
            &only(&ev, &["petgraph"]),
            &[&format!("petgraph: {:?} {preds:?}", want.distances)],
        );
        let (rows, _) = g
            .trace("gds.floydWarshall.stream", &[], &record(&[w()]))
            .await;
        let fw = petgraph::algo::floyd_warshall(&d, |e| *e.weight()).unwrap();
        let mut want: Vec<Vec<Value>> = fw
            .into_iter()
            .filter(|(_, x)| *x < f64::MAX)
            .map(|((a, b), x)| {
                vec![
                    int(a.index() as i64),
                    int(b.index() as i64),
                    Value::Float(x),
                ]
            })
            .collect();
        want.sort_by_key(|r| match (&r[0], &r[1]) {
            (Value::Int(a), Value::Int(b)) => (*a, *b),
            _ => unreachable!(),
        });
        assert_eq!(rows, want, "025");
        // 020: the negative cycle petgraph finds.
        let g = TraceGraph::new(6, &negcyc).await;
        let d = digraph(6, &negcyc);
        let (rows, ev) = g
            .trace(
                "gds.findNegativeCycle.stream",
                &[],
                &record(&[("sourceNode", g.node(0)), w()]),
            )
            .await;
        let want = petgraph::algo::find_negative_cycle(&d, ix(0))
            .map(|c| c.iter().map(|v| v.index()).collect::<Vec<_>>());
        assert_steps(
            "020",
            &only(&ev, &["petgraph"]),
            &[&format!("petgraph: {want:?}")],
        );
        let cycle = g.path_uids(&rows[0][1]);
        let weight: f64 = cycle
            .windows(2)
            .map(|x| {
                negcyc
                    .iter()
                    .find(|e| (e.0, e.1) == (x[0], x[1]))
                    .unwrap()
                    .3
                    .unwrap()
            })
            .sum();
        assert!(weight < 0.0, "020: {cycle:?} weighs {weight}");
    });
}

/// rmp #71 regression, ALG-025: a negative self-loop is a negative cycle,
/// which fails 025 with 22N11 (`algorithm-entries.md`, TS-25 Costs), as it
/// fails 021 and 024. petgraph 0.8.3's `floyd_warshall` sets every
/// node's distance to itself to 0 after loading the edges
/// (`src/algo/floyd_warshall.rs:284-288`), so it misses such a cycle and
/// lpg returned distances.
#[test]
fn floyd_warshall_fails_on_a_negative_self_loop() {
    current(async {
        let g = TraceGraph::new(
            3,
            &[
                (0, 1, "R", Some(1.0)),
                (1, 1, "R", Some(-1.0)),
                (1, 2, "R", Some(1.0)),
            ],
        )
        .await;
        for proc in ["gds.floydWarshall.stream", "gds.johnson.stream"] {
            let (rows, _) = g.try_trace(proc, &[], &record(&[w()])).await;
            assert_eq!(
                rows.map_err(|e| e.code().to_string()).err().as_deref(),
                Some("22N11"),
                "{proc}"
            );
        }
        // A positive self-loop is no cycle to report.
        let g = TraceGraph::new(2, &[(0, 1, "R", Some(1.0)), (1, 1, "R", Some(2.0))]).await;
        let (rows, _) = g
            .trace("gds.floydWarshall.stream", &[], &record(&[w()]))
            .await;
        assert_eq!(rows.len(), 3);
    });
}

/// rmp #71, ALG-188: 2-opt on one route against the reference sequence
/// of `trace_ref_a.py` (`savings_steps_route`: Croes 1958, first
/// improvement, the scan continuing after a reversal) on a route the
/// savings pass of the trace graph does not produce (its routes have no
/// improving move).
#[test]
fn two_opt_moves() {
    let text = std::fs::read_to_string(expected_dir().join("trace_a.json")).expect("trace_a.json");
    let all: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let c = &all["two_opt"];
    let dm: Vec<Vec<f64>> = c["dm"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect()
        })
        .collect();
    let mut route: Vec<usize> = c["route"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect();
    let want: Vec<&str> = c["want"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    let rec = super::trace::Recording::start();
    super::cycles::two_opt(&dm, &mut route);
    assert_steps("188 2-opt", &rec.take(), &want);
}

/// rmp #71, ALG-187: the Euler circuit and the shortcut of Christofides'
/// algorithm (the tree, odd nodes and matching are reference cases): the
/// circuit starts at the first tree edge's first end and uses every edge
/// of the tree and the matching once; the tour is its first visits,
/// closed.
#[test]
fn christofides_circuit_and_shortcut() {
    current(async {
        let bodies = [
            ":N {lat: 3.87e1, lng: -9.14e0,",
            ":N {lat: 4.11e1, lng: -8.61e0,",
            ":N {lat: 3.70e1, lng: -7.93e0,",
            ":N {lat: 4.02e1, lng: -8.41e0,",
            ":N {lat: 3.85e1, lng: -7.91e0,",
            ":N {lat: 4.15e1, lng: -8.42e0,",
        ];
        let g = TraceGraph::with_nodes(&bodies, &[]).await;
        let (rows, ev) = g
            .trace("tsp.solve", &[nodes(&g, &[0, 1, 2, 3, 4, 5])], &record(&[]))
            .await;
        let get = |k: &str| ints_of(&only(&ev, &[k])[0][k.len() + 2..]);
        let (tree, matching, circuit) = (get("mst"), get("matching"), get("circuit"));
        let mut edges: Vec<(usize, usize)> = tree
            .chunks(2)
            .chain(matching.chunks(2))
            .map(|e| (e[0].min(e[1]), e[0].max(e[1])))
            .collect();
        assert_eq!(circuit[0], tree[0]);
        assert_eq!(circuit.first(), circuit.last());
        let mut walked: Vec<(usize, usize)> = circuit
            .windows(2)
            .map(|e| (e[0].min(e[1]), e[0].max(e[1])))
            .collect();
        edges.sort_unstable();
        walked.sort_unstable();
        assert_eq!(
            walked, edges,
            "an Euler circuit of the tree and the matching"
        );
        let mut first: Vec<usize> = Vec::new();
        for v in &circuit {
            if !first.contains(v) {
                first.push(*v);
            }
        }
        assert_eq!(g.uids(&rows[0][0]), first, "the tour's sources");
    });
}

/// rmp #71, ALG-015 (known input: the fallback after a heap empties was
/// never run). The fallback is reached only when t is unreachable from s,
/// with no meeting, and then returns no path, as petgraph does: if t is
/// reachable, the meeting `best` is set once t's predecessor on a
/// shortest path settles (t is settled backwards from the first round),
/// and when the forward heap pops t (or the backward heap pops s),
/// d(u) + d(v) ≥ d(s, t) ≥ best stops the search; a heap empties only
/// after that pop. Checked on every pair of three graphs: every
/// `fallback` event has no meeting, and the cost equals Dijkstra's.
#[test]
fn bidirectional_fallback_has_no_meeting() {
    current(async {
        let und: &[(usize, usize, &str, Option<f64>)] = &[
            (0, 1, "R", Some(1.0)),
            (1, 2, "R", Some(1.0)),
            (2, 0, "R", Some(1.0)),
            (2, 3, "R", Some(1.0)),
            (3, 4, "R", Some(1.0)),
            (4, 2, "R", Some(1.0)),
            (1, 3, "R", Some(1.0)),
            (1, 0, "R", Some(1.0)),
            (4, 4, "R", Some(1.0)),
        ];
        let dag: &[(usize, usize, &str, Option<f64>)] = &[
            (0, 1, "R", Some(3.0)),
            (0, 2, "R", Some(1.0)),
            (1, 3, "R", Some(2.0)),
            (2, 3, "R", Some(4.0)),
            (3, 4, "R", Some(1.0)),
            (0, 3, "R", Some(1.0)),
            (1, 4, "R", Some(5.0)),
            (5, 4, "R", Some(2.0)),
            (0, 1, "R", Some(2.0)),
        ];
        let mut runs = 0;
        for (n, edges, o) in [
            (7, DIR, "NATURAL"),
            (5, und, "UNDIRECTED"),
            (6, dag, "NATURAL"),
        ] {
            let g = TraceGraph::new(n, edges).await;
            for s in 0..n {
                for t in 0..n {
                    let cfg = record(&[
                        ("sourceNode", g.node(s)),
                        ("targetNode", g.node(t)),
                        ("orientation", txt(o)),
                        w(),
                    ]);
                    let (rows, ev) = g.trace("gds.bidirectionalDijkstra.stream", &[], &cfg).await;
                    for e in only(&ev, &["fallback"]) {
                        assert_eq!(e, "fallback: None", "{s} {t}");
                    }
                    let (want, _) = g.trace("gds.shortestPath.dijkstra.stream", &[], &cfg).await;
                    assert_eq!(
                        rows.first().map(|r| r[3].clone()),
                        want.first().map(|r| r[3].clone()),
                        "{s} {t}"
                    );
                    runs += 1;
                }
            }
        }
        assert_eq!(runs, 49 + 25 + 36);
    });
}

/// rmp #71 regression, ALG-004: `pathSubgraphAll` returns the
/// relationships between the returned nodes, each once (MAGE `path.cpp`
/// `SubgraphAll`: each node's outgoing relationships to a returned node).
/// Under `UNDIRECTED` lpg's `out` lists each edge from both ends, so each
/// relationship came twice (a self-loop four times).
#[test]
fn path_subgraph_all_lists_each_relationship_once() {
    current(async {
        let g = TraceGraph::new(
            3,
            &[
                (0, 1, "R", None),
                (1, 2, "R", None),
                (2, 2, "R", None),
                (0, 1, "R", None),
            ],
        )
        .await;
        for o in ["NATURAL", "REVERSE", "UNDIRECTED"] {
            let (rows, _) = g
                .trace(
                    "gds.pathSubgraphAll.stream",
                    &[],
                    &record(&[("startNode", nodes(&g, &[0])), ("orientation", txt(o))]),
                )
                .await;
            let Value::List(rels) = &rows[0][1] else {
                panic!()
            };
            let mut ids: Vec<u64> = rels
                .iter()
                .map(|r| match r {
                    Value::Edge(id) => *id,
                    _ => panic!(),
                })
                .collect();
            ids.sort_unstable();
            // Every node is reached (the empty filter follows both
            // directions); edge i has element identifier 3 + i.
            assert_eq!(g.uids(&rows[0][0]).len(), 3, "{o}");
            assert_eq!(ids, vec![3, 4, 5, 6], "{o}");
        }
    });
}

/// rmp #71: PathWrite, one write mode at a time (known input: the eight
/// modes shared one tested code path). Each write in a transaction
/// creates one `WR` relationship per stream row, from `sourceNode` to
/// `targetNode`, with `totalCost` and, when asked, `nodeIds` and `costs`
/// (GDS's PathWrite keys); `relationshipsWritten` counts them.
#[test]
fn path_write_modes_one_by_one() {
    use crate::TransactionOptions;
    use crate::test_support::algo::drain;
    current(async {
        let coords = [
            (38.7, -9.1),
            (41.1, -8.6),
            (37.0, -7.9),
            (40.2, -8.4),
            (38.5, -7.9),
            (41.5, -8.4),
            (39.0, -9.0),
        ];
        let bodies: Vec<String> = coords
            .iter()
            .map(|(a, b)| format!(":N {{lat: {a:e}, lng: {b:e},"))
            .collect();
        let bodies: Vec<&str> = bodies.iter().map(String::as_str).collect();
        let g = TraceGraph::with_nodes(&bodies, DIR).await;
        let (s, t) = (g.node(0), g.node(5));
        let pair = |extra: &[(&'static str, Value)]| -> Vec<(&'static str, Value)> {
            let mut c = vec![
                ("sourceNode", s.clone()),
                ("targetNode", t.clone()),
                ("relationshipTypes", strs(&["R"])),
                w(),
            ];
            c.extend_from_slice(extra);
            c
        };
        let modes: Vec<(&str, Vec<(&str, Value)>)> = vec![
            (
                "gds.unweightedShortestPaths",
                vec![
                    ("sourceNode", s.clone()),
                    ("relationshipTypes", strs(&["R"])),
                ],
            ),
            ("gds.shortestPath.dijkstra", pair(&[])),
            ("gds.bidirectionalDijkstra", pair(&[])),
            (
                "gds.shortestPath.astar",
                pair(&[
                    ("latitudeProperty", txt("lat")),
                    ("longitudeProperty", txt("lng")),
                ]),
            ),
            (
                "gds.spfa",
                vec![
                    ("sourceNode", s.clone()),
                    ("relationshipTypes", strs(&["R"])),
                    w(),
                ],
            ),
            (
                "gds.allShortestPaths.delta",
                vec![
                    ("sourceNode", s.clone()),
                    ("relationshipTypes", strs(&["R"])),
                    w(),
                ],
            ),
            ("gds.allShortestPathsBetweenNodes", pair(&[])),
            ("gds.shortestPath.yens", pair(&[("k", int(3))])),
        ];
        for (base, cfg) in modes {
            for flags in [false, true] {
                let tx = g.s.begin(TransactionOptions::default()).await.unwrap();
                let (text, ps) =
                    super::tests_paths::call_text(&format!("{base}.stream"), &[], &record(&cfg));
                let stream = drain(tx.execute(&text, ps).await.unwrap()).await.unwrap();
                assert!(!stream.is_empty(), "{base}");
                let mut wcfg = cfg.clone();
                wcfg.push(("writeRelationshipType", txt("WR")));
                if flags {
                    wcfg.push(("writeNodeIds", Value::Bool(true)));
                    wcfg.push(("writeCosts", Value::Bool(true)));
                }
                let (text, ps) =
                    super::tests_paths::call_text(&format!("{base}.write"), &[], &record(&wcfg));
                let summary = drain(tx.execute(&text, ps).await.unwrap()).await.unwrap();
                assert_eq!(
                    summary[0][4],
                    int(stream.len() as i64),
                    "{base}: relationshipsWritten"
                );
                let written = drain(
                    tx.execute(
                        "MATCH (a)-[e:WR]->(b) RETURN a, b, e.totalCost, e.nodeIds, e.costs",
                        crate::Parameters::new(),
                    )
                    .await
                    .unwrap(),
                )
                .await
                .unwrap();
                let key = |r: &Vec<Value>| format!("{r:?}");
                let mut got: Vec<String> = written
                    .iter()
                    .map(|r| {
                        let a = int(g.uid(&r[0]) as i64);
                        let b = int(g.uid(&r[1]) as i64);
                        key(&vec![a, b, r[2].clone(), r[3].clone(), r[4].clone()])
                    })
                    .collect();
                let mut want: Vec<String> = stream
                    .iter()
                    .map(|r| {
                        let (ids, costs) = if flags {
                            (r[4].clone(), r[5].clone())
                        } else {
                            (Value::Null, Value::Null)
                        };
                        key(&vec![
                            int(g.uid(&r[1]) as i64),
                            int(g.uid(&r[2]) as i64),
                            r[3].clone(),
                            ids,
                            costs,
                        ])
                    })
                    .collect();
                got.sort();
                want.sort();
                assert_eq!(got, want, "{base} with flags {flags}");
                tx.rollback().await.unwrap();
            }
        }
    });
}

async fn count(g: &TraceGraph, proc: &str, cfg: Value) -> usize {
    g.trace(proc, &[], &cfg).await.0.len()
}

async fn code(g: &TraceGraph, proc: &str, cfg: Value) -> Option<String> {
    g.try_trace(proc, &[], &cfg)
        .await
        .0
        .map_err(|e| e.code().to_string())
        .err()
}

/// rmp #71, semantic checks of pass A: the empty projection, one node,
/// and the error codes of `algorithm-entries.md` and TS-25 (22N11 for a
/// missing or unknown key, a node outside the projection, a negative
/// cost where costs must be non-negative and a negative cycle; 22G03 for
/// a value of the wrong type).
#[test]
fn edge_cases_and_errors() {
    current(async {
        let empty = TraceGraph::new(0, &[]).await;
        for proc in [
            "gds.randomWalk.stream",
            "gds.dag.topologicalSort.stream",
            "gds.unweightedShortestPaths.stream",
            "gds.johnson.stream",
            "gds.floydWarshall.stream",
            "gds.seidel.stream",
            "gds.allShortestPaths.stream",
            "gds.dag.longestPath.stream",
            "cycles.get",
            "gds.elementaryCircuits.stream",
            "gds.isBipartiteUndirected.stream",
            "gds.transitiveClosure.stream",
            "gds.transitiveReduction.stream",
        ] {
            let (rows, _) = empty.trace(proc, &[], &record(&[])).await;
            assert!(rows.is_empty(), "{proc} on the empty projection");
        }
        let (rows, _) = empty
            .trace("gds.isTournament.stream", &[], &record(&[]))
            .await;
        assert_eq!(
            rows,
            vec![vec![Value::Bool(true), Value::Bool(true)]],
            "NetworkX: the empty graph is a tournament"
        );
        // One node with a self-loop.
        let one = TraceGraph::new(1, &[(0, 0, "R", Some(1.0))]).await;
        let src = record(&[("sourceNode", one.node(0))]);
        for proc in ["gds.bfs.stream", "gds.dfs.stream"] {
            let (rows, _) = one.trace(proc, &[], &src).await;
            assert_eq!(one.uids(&rows[0][1]), vec![0], "{proc}");
        }
        assert_eq!(
            count(&one, "gds.dag.topologicalSort.stream", record(&[])).await,
            0,
            "a self-loop is a cycle"
        );
        assert_eq!(
            count(&one, "gds.elementaryCircuits.stream", record(&[])).await,
            1,
            "the self-loop circuit"
        );
        assert_eq!(
            count(&one, "cycles.get", record(&[])).await,
            2,
            "MAGE's self-loop cycle: two rows"
        );
        assert_eq!(
            count(&one, "gds.isBipartiteUndirected.stream", record(&[])).await,
            0,
            "not bipartite"
        );
        assert_eq!(count(&one, "gds.seidel.stream", record(&[])).await, 1);
        // Errors.
        let g = TraceGraph::with_nodes(
            &[":N", ":N", ":M"],
            &[(0, 1, "R", Some(-1.0)), (1, 0, "R", Some(-1.0))],
        )
        .await;
        let s = ("sourceNode", g.node(0));
        assert_eq!(
            code(&g, "gds.bfs.stream", record(&[])).await.as_deref(),
            Some("22N11"),
            "missing key"
        );
        assert_eq!(
            code(
                &g,
                "gds.bfs.stream",
                record(&[s.clone(), ("noSuchKey", int(1))])
            )
            .await
            .as_deref(),
            Some("22N11"),
            "unknown key"
        );
        assert_eq!(
            code(
                &g,
                "gds.bfs.stream",
                record(&[("sourceNode", g.node(2)), ("nodeLabels", strs(&["N"]))])
            )
            .await
            .as_deref(),
            Some("22N11"),
            "a node outside the projection"
        );
        assert_eq!(
            code(&g, "gds.bfs.stream", record(&[("sourceNode", txt("x"))]))
                .await
                .as_deref(),
            Some("22G03"),
            "wrong type"
        );
        assert_eq!(
            code(
                &g,
                "gds.bfs.stream",
                record(&[s.clone(), ("maxDepth", int(-2))])
            )
            .await
            .as_deref(),
            Some("22N11"),
            "maxDepth below -1"
        );
        for proc in [
            "gds.allShortestPaths.dijkstra.stream",
            "gds.allShortestPaths.delta.stream",
        ] {
            assert_eq!(
                code(&g, proc, record(&[s.clone(), w()])).await.as_deref(),
                Some("22N11"),
                "{proc}: a negative cost"
            );
        }
        assert_eq!(
            code(&g, "gds.spfa.stream", record(&[s.clone(), w()]))
                .await
                .as_deref(),
            Some("22N11"),
            "021: a negative cycle"
        );
        assert_eq!(
            code(
                &g,
                "gds.shortestPath.dijkstra.stream",
                record(std::slice::from_ref(&s))
            )
            .await
            .as_deref(),
            Some("22N11"),
            "014: no target"
        );
    });
}
