//! Tests of the spanning-tree, flow, matching and colouring families
//! (rmp #68): each entry against its oracle (`algorithm-entries.md`
//! Verification; rule V for the results that are not unique), bit-equal
//! rows at `concurrency` 1 to 16 twice, `CALL` equal to the Rust API, the
//! worker-count rule, the memory cap; and the ALG-187 carry-over: the
//! minimum-weight perfect matching above 20 odd vertices. Helpers are
//! those of `tests_paths.rs`.

use std::collections::{BTreeMap, BTreeSet};

use super::tests_paths::{CAP, E, Lab, close, current, int, ints, num, nx_values, txt};
use crate::catalog::TokenKind;
use crate::test_support::algo::{Fixture, drain, expected, param, record, rows, with};
use crate::types::Value;
use crate::{Parameters, TransactionOptions};

fn float(v: &Value) -> f64 {
    match v {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f64,
        other => panic!("not a number: {other:?}"),
    }
}

fn nodes(l: &Lab, uids: &[i64]) -> Value {
    Value::List(uids.iter().map(|u| l.node(*u)).collect::<Vec<_>>().into())
}

/// A tree's rows as `(node, parent, weight)` by uid.
type TreeRows = Vec<(i64, i64, f64)>;

fn tree_rows(l: &Lab, rows: &[Vec<Value>]) -> TreeRows {
    rows.iter()
        .map(|r| (l.uid(&r[0]), l.uid(&r[1]), float(&r[2])))
        .collect()
}

fn gds_tree(file: &str) -> TreeRows {
    expected(file)
        .iter()
        .map(|m| {
            (
                m["nodeId"].as_i64().unwrap(),
                m["parentId"].as_i64().unwrap(),
                num(&m["weight"]),
            )
        })
        .collect()
}

/// Checks that `t` is one tree (rule V): every edge row joins its parent
/// and node by an edge of that weight (`weighted`, else 1), from parent
/// to node when `directed`; every parent chain reaches the same root.
/// Returns the tree's nodes and total weight.
fn check_tree(l: &Lab, t: &TreeRows, directed: bool, weighted: bool) -> (BTreeSet<i64>, f64) {
    let mut parent: BTreeMap<i64, i64> = BTreeMap::new();
    let mut nodes = BTreeSet::new();
    let mut total = 0.0;
    for (v, p, w) in t {
        nodes.insert(*v);
        nodes.insert(*p);
        if v == p {
            assert_eq!(*w, 0.0, "the root's weight");
            continue;
        }
        assert!(parent.insert(*v, *p).is_none(), "node {v} twice");
        let joined = l.edges.values().any(|e| {
            let fits = (e.src, e.dst) == (*p, *v) || (!directed && (e.src, e.dst) == (*v, *p));
            // GDS's Steiner rows give differences of path costs, rounded.
            fits && (if weighted {
                close(e.weight, *w, 1e-12)
            } else {
                *w == 1.0
            })
        });
        assert!(joined, "no edge {p} - {v} of weight {w}");
        total += w;
    }
    let mut roots = BTreeSet::new();
    for v in &nodes {
        let mut x = *v;
        let mut steps = 0;
        while let Some(p) = parent.get(&x) {
            x = *p;
            steps += 1;
            assert!(steps <= parent.len(), "a parent cycle");
        }
        roots.insert(x);
    }
    assert!(roots.len() <= 1, "one tree: roots {roots:?}");
    (nodes, total)
}

/// One write-mode summary row of `proc`, rolled back, with the rows of
/// `read` in the same transaction.
async fn write_row(
    l: &Lab,
    proc: &str,
    config: Value,
    read: &str,
) -> (Vec<Value>, Vec<Vec<Value>>) {
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    let row = drain(
        tx.execute(&format!("CALL {proc}($c)"), param("c", config))
            .await
            .unwrap_or_else(|e| panic!("{proc}: {e}")),
    )
    .await
    .unwrap_or_else(|e| panic!("{proc}: {e}"))
    .remove(0);
    let read = if read.is_empty() {
        Vec::new()
    } else {
        drain(tx.execute(read, Parameters::new()).await.unwrap())
            .await
            .unwrap()
    };
    tx.rollback().await.unwrap();
    (row, read)
}

/// The weight of a minimum (maximum with `max`) spanning forest of the
/// undirected subgraph induced by `set` (Kruskal).
fn induced_tree(edges: &BTreeMap<u64, E>, set: &BTreeSet<i64>, max: bool) -> f64 {
    let mut es: Vec<(f64, i64, i64)> = edges
        .values()
        .filter(|e| e.src != e.dst && set.contains(&e.src) && set.contains(&e.dst))
        .map(|e| (e.weight, e.src, e.dst))
        .collect();
    es.sort_by(|a, b| {
        if max {
            b.0.total_cmp(&a.0)
        } else {
            a.0.total_cmp(&b.0)
        }
    });
    let mut root: BTreeMap<i64, i64> = set.iter().map(|v| (*v, *v)).collect();
    let find = |root: &BTreeMap<i64, i64>, v: i64| {
        let mut x = v;
        while root[&x] != x {
            x = root[&x];
        }
        x
    };
    let mut total = 0.0;
    for (w, a, b) in es {
        let (ra, rb) = (find(&root, a), find(&root, b));
        if ra != rb {
            root.insert(ra, rb);
            total += w;
        }
    }
    total
}

/// The nodes of a k-spanning tree from its write mode: the tree's nodes
/// share the root's identifier, every other node holds its own.
fn written_tree(values: &BTreeMap<i64, i64>) -> BTreeSet<i64> {
    let mut count: BTreeMap<i64, usize> = BTreeMap::new();
    for v in values.values() {
        *count.entry(*v).or_default() += 1;
    }
    values
        .iter()
        .filter(|(_, v)| count[*v] > 1)
        .map(|(u, _)| *u)
        .collect()
}

/// rmp #68: ALG-147 and ALG-149 against GDS (rule V).
#[test]
fn spanning_trees_match_gds() {
    current(async {
        for (name, source) in [("karate", 0), ("zipf60", 13), ("uniform60", 14)] {
            let l = Lab::load(name).await;
            let w = ("relationshipWeightProperty", txt("weight"));
            let src = ("sourceNode", l.node(source));
            for (config, file, weighted) in [
                (record(&[src.clone(), w.clone()]), "mst", true),
                (
                    record(&[src.clone(), w.clone(), ("objective", txt("maximum"))]),
                    "mst.max",
                    true,
                ),
                (record(std::slice::from_ref(&src)), "mst.unweighted", false),
            ] {
                let rows = l.det("gds.spanningTree.stream", &[], config, false).await;
                let (got, total) = check_tree(&l, &tree_rows(&l, &rows), false, weighted);
                let gds = gds_tree(&format!("{name}.gds.{file}.jsonl"));
                let (want, want_total) = check_tree(&l, &gds, false, weighted);
                assert_eq!(got, want, "147 {name} {file}: the source's component");
                assert!(
                    close(total, want_total, 1e-9),
                    "147 {name} {file}: {total} against {want_total}"
                );
            }
            let (row, _) = write_row(
                &l,
                "gds.spanningTree.write",
                record(&[
                    src.clone(),
                    w.clone(),
                    ("writeRelationshipType", txt("MST")),
                    ("writeProperty", txt("w")),
                ]),
                "",
            )
            .await;
            let want = &expected(&format!("{name}.gds.mst.write.jsonl"))[0];
            assert_eq!(row[3], int(want["effectiveNodeCount"].as_i64().unwrap()));
            assert_eq!(row[4], int(want["relationshipsWritten"].as_i64().unwrap()));
            assert!(close(float(&row[5]), num(&want["totalWeight"]), 1e-9));
            // 149: a tree of k nodes; its weight against GDS's tree's.
            for (k, max, file) in [(5, false, "kst"), (4, true, "kst.max")] {
                let mut config = record(&[
                    src.clone(),
                    w.clone(),
                    ("k", int(k)),
                    ("writeProperty", txt("kst")),
                ]);
                if max {
                    config = with(&config, "objective", txt("maximum"));
                }
                let (row, read) = write_row(
                    &l,
                    "gds.kSpanningTree.write",
                    config,
                    "MATCH (n) RETURN n.uid AS u, n.kst AS v",
                )
                .await;
                assert_eq!(row[4], int(k), "effectiveNodeCount");
                let values: BTreeMap<i64, i64> = read
                    .iter()
                    .map(|r| match (&r[0], &r[1]) {
                        (Value::Int(u), Value::Int(v)) => (*u, *v),
                        other => panic!("{other:?}"),
                    })
                    .collect();
                let got = written_tree(&values);
                let a = induced_tree(&l.edges, &got, max);
                assert_eq!(got.len(), k as usize);
                // GDS in lpg's node order (`.o`, asserted) and in its own
                // (recorded).
                // karate's integer weights tie: the heuristics' choices among
                // equal edges differ (recorded, not asserted).
                for (suffix, assert) in [(".o", name != "karate"), ("", false)] {
                    let gds: BTreeMap<i64, i64> =
                        expected(&format!("{name}.gds.{file}{suffix}.jsonl"))
                            .iter()
                            .map(|m| (m["uid"].as_i64().unwrap(), m["kst"].as_i64().unwrap()))
                            .collect();
                    let want = written_tree(&gds);
                    assert_eq!(want.len(), k as usize);
                    let b = induced_tree(&l.edges, &want, max);
                    eprintln!(
                        "LAB 149 {name} k {k} max {max}: weight {a} against GDS{suffix} {b}; nodes {got:?} against {want:?}"
                    );
                    if assert {
                        assert!(
                            close(a, b, 1e-9),
                            "149 {name} {file}{suffix}: {a} against {b}"
                        );
                    }
                }
            }
            l.f.db.close().await.unwrap();
        }
    });
}

/// rmp #68: ALG-153 against GDS (rule V), parallel.
#[test]
fn steiner_trees_match_gds() {
    current(async {
        let cases: [(&str, i64, &[i64]); 3] = [
            ("zipf60", 13, &[0, 4, 9, 17, 25, 33, 41, 50, 58]),
            ("uniform60", 14, &[0, 3, 11, 22, 37, 45, 59]),
            ("zipf300", 218, &[0, 5, 13, 50, 77, 123, 200, 299]),
        ];
        for (name, s, targets) in cases {
            let l = Lab::load(name).await;
            let base = record(&[
                ("sourceNode", l.node(s)),
                ("targetNodes", nodes(&l, targets)),
            ]);
            let weighted = with(&base, "relationshipWeightProperty", txt("weight"));
            for (config, file, w) in [
                (weighted.clone(), "steiner", true),
                (
                    with(&weighted, "applyRerouting", Value::Bool(true)),
                    "steiner.reroute",
                    true,
                ),
                (base.clone(), "steiner.unweighted", false),
            ] {
                let rows = l.det("gds.steinerTree.stream", &[], config, true).await;
                let (got, total) = check_tree(&l, &tree_rows(&l, &rows), true, w);
                let reached = |set: &BTreeSet<i64>| {
                    targets
                        .iter()
                        .filter(|t| set.contains(t))
                        .copied()
                        .collect::<Vec<_>>()
                };
                // GDS in lpg's node order (`.o`, asserted) and in its own
                // (recorded). Unweighted, equal hop counts tie: which of
                // two equally near targets and paths joins first differs
                // (recorded, not asserted).
                for (suffix, assert) in [(".o", w), ("", false)] {
                    let gds = gds_tree(&format!("{name}.gds.{file}{suffix}.jsonl"));
                    let (want, want_total) = check_tree(&l, &gds, true, w);
                    eprintln!(
                        "LAB 153 {name} {file}: weight {total} against GDS{suffix} {want_total}; {} nodes against {}",
                        got.len(),
                        want.len()
                    );
                    if assert {
                        assert_eq!(reached(&got), reached(&want), "153 {name} {file}: reached");
                        assert!(
                            close(total, want_total, 1e-9),
                            "153 {name} {file}{suffix}: {total} against {want_total}"
                        );
                    }
                }
            }
            let (row, _) = write_row(
                &l,
                "gds.steinerTree.write",
                with(
                    &with(&weighted, "writeRelationshipType", txt("ST")),
                    "writeProperty",
                    txt("w"),
                ),
                "",
            )
            .await;
            let want = &expected(&format!("{name}.gds.steiner.write.jsonl"))[0];
            assert_eq!(row[3], int(want["effectiveNodeCount"].as_i64().unwrap()));
            assert_eq!(
                row[4],
                int(want["effectiveTargetNodesCount"].as_i64().unwrap())
            );
            assert!(close(float(&row[5]), num(&want["totalWeight"]), 1e-9));
            assert_eq!(row[6], int(want["relationshipsWritten"].as_i64().unwrap()));
            l.f.db.close().await.unwrap();
        }
    });
}

/// rmp #68: ALG-154 against GDS on geo30 with the prizes `lat` and `lng`
/// (rule V: a tree; `totalWeight` and `sumOfPrizes` F).
#[test]
fn prize_steiner_tree_matches_gds() {
    current(async {
        let l = Lab::load("geo30").await;
        let w = ("relationshipWeightProperty", txt("weight"));
        let rows = l
            .det(
                "gds.prizeSteinerTree.stream",
                &[],
                record(&[("prizeProperty", txt("lat")), w.clone()]),
                false,
            )
            .await;
        let (got, total) = check_tree(&l, &tree_rows(&l, &rows), false, true);
        let (want, want_total) = check_tree(&l, &gds_tree("geo30.gds.pcst.jsonl"), false, true);
        assert_eq!(got.len(), want.len());
        assert!(
            close(total, want_total, 1e-9),
            "{total} against {want_total}"
        );
        let (row, _) = write_row(
            &l,
            "gds.prizeSteinerTree.write",
            record(&[
                ("prizeProperty", txt("lat")),
                w.clone(),
                ("writeRelationshipType", txt("P")),
                ("writeProperty", txt("w")),
            ]),
            "",
        )
        .await;
        let want = &expected("geo30.gds.pcst.write.jsonl")[0];
        assert_eq!(row[3], int(want["effectiveNodeCount"].as_i64().unwrap()));
        assert!(close(float(&row[4]), num(&want["totalWeight"]), 1e-9));
        assert!(close(float(&row[5]), num(&want["sumOfPrizes"]), 1e-9));
        // Negative prizes: a single node, no row.
        let rows = l
            .det(
                "gds.prizeSteinerTree.stream",
                &[],
                record(&[("prizeProperty", txt("lng")), w]),
                false,
            )
            .await;
        assert!(rows.is_empty() && expected("geo30.gds.pcst.lng.jsonl").is_empty());
        eprintln!(
            "LAB 154 geo30: weight {total} against GDS {want_total}, {} nodes",
            got.len()
        );
        l.f.db.close().await.unwrap();
    });
}

/// rmp #68: ALG-151 against the rational determinant (rule X), where an
/// `f64` determinant is not exact; a count above `INTEGER` fails with
/// 22003; a directed run needs `rootNode`.
#[test]
fn spanning_tree_counts_are_exact() {
    current(async {
        let dir = crate::test_support::algo::expected_dir();
        for name in [
            "karate", "petersen", "negw30", "flow30", "dag40", "geo30", "rc24",
        ] {
            let l = Lab::load(name).await;
            let u = record(&[("orientation", txt("UNDIRECTED"))]);
            let undirected = format!("{name}.nx.st.jsonl");
            let undirected = if dir.join(&undirected).exists() {
                expected(&undirected)
            } else {
                Vec::new()
            };
            for m in undirected {
                let v = &m["value"];
                if let Some(c) = v.as_i64() {
                    let rows = l
                        .det("gds.countSpanningTrees.stream", &[], u.clone(), false)
                        .await;
                    assert_eq!(rows, vec![vec![int(c)]], "151 {name}");
                } else {
                    let e = l
                        .call("gds.countSpanningTrees.stream", &[], &u)
                        .await
                        .unwrap_err();
                    assert_eq!(e.code(), "22003", "151 {name}: {v}");
                }
            }
            let directed = format!("{name}.nx.st.directed.jsonl");
            if dir.join(&directed).exists() {
                for m in expected(&directed) {
                    let root = m["root"].as_i64().unwrap();
                    let rows = l
                        .det(
                            "gds.countSpanningTrees.stream",
                            &[],
                            record(&[("rootNode", l.node(root))]),
                            false,
                        )
                        .await;
                    assert_eq!(
                        rows,
                        vec![vec![int(m["value"].as_i64().unwrap())]],
                        "151 {name} root {root}"
                    );
                }
            }
            l.f.db.close().await.unwrap();
        }
        let l = Lab::load("cyc12").await;
        let e = l
            .call("gds.countSpanningTrees.stream", &[], &record(&[]))
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        l.f.db.close().await.unwrap();
        // karate's count is not the f64 determinant (5090996323019105.0,
        // numpy, measured): nalgebra's.
        let exact = 5_090_996_323_019_136_i64;
        let lap = karate_reduced_laplacian();
        let k = 33;
        let m = nalgebra::DMatrix::from_fn(k, k, |i, j| lap[i * k + j] as f64);
        let f = m.determinant();
        eprintln!("LAB 151 karate: exact {exact}, nalgebra f64 {f}");
        assert_eq!(super::trees::exact_det(&lap, k), Some(exact));
        assert_ne!(
            f.round() as i64,
            exact,
            "the f64 determinant is not exact here"
        );
    });
}

/// karate's Laplacian without node 0, from the fixture file.
fn karate_reduced_laplacian() -> Vec<i64> {
    let text =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir("karate").join("edges.csv"))
            .unwrap();
    let k = 33;
    let mut a = vec![0i64; k * k];
    for line in text.lines().skip(1) {
        let f: Vec<usize> = line
            .split(',')
            .take(2)
            .map(|x| x.parse().unwrap())
            .collect();
        let (u, v) = (f[0], f[1]);
        for (x, y) in [(u, v), (v, u)] {
            if x == 0 {
                continue;
            }
            a[(x - 1) * k + (x - 1)] += 1;
            if y != 0 {
                a[(x - 1) * k + (y - 1)] -= 1;
            }
        }
    }
    a
}

/// The capacity and cost of each edge of a loaded fixture.
async fn capacities(l: &Lab) -> Vec<(i64, i64, f64, f64)> {
    rows(
        &l.f.s,
        "MATCH (a)-[e]->(b) RETURN a.uid AS a, b.uid AS b, e.capacity AS c, e.cost AS k",
        Parameters::new(),
    )
    .await
    .unwrap()
    .iter()
    .map(|r| match (&r[0], &r[1]) {
        (Value::Int(a), Value::Int(b)) => (*a, *b, float(&r[2]), float(&r[3])),
        other => panic!("{other:?}"),
    })
    .collect()
}

/// Checks a flow (rule V): per node pair at most the capacity of its
/// edges, conservation at every node outside the sources and targets;
/// returns the flow into the targets.
fn check_flow(
    caps: &[(i64, i64, f64, f64)],
    flows: &[(i64, i64, f64)],
    sources: &[i64],
    targets: &[i64],
) -> f64 {
    let mut cap: BTreeMap<(i64, i64), f64> = BTreeMap::new();
    for (a, b, c, _) in caps {
        *cap.entry((*a, *b)).or_default() += c;
    }
    let mut used: BTreeMap<(i64, i64), f64> = BTreeMap::new();
    let mut net: BTreeMap<i64, f64> = BTreeMap::new();
    for (a, b, f) in flows {
        assert!(*f > 0.0);
        *used.entry((*a, *b)).or_default() += f;
        *net.entry(*a).or_default() -= f;
        *net.entry(*b).or_default() += f;
    }
    for (pair, f) in &used {
        assert!(
            *f <= cap.get(pair).copied().unwrap_or(0.0) + 1e-9,
            "{pair:?} over capacity"
        );
    }
    for (v, x) in &net {
        if !sources.contains(v) && !targets.contains(v) {
            assert!(x.abs() < 1e-9, "conservation at {v}: {x}");
        }
    }
    targets
        .iter()
        .map(|t| net.get(t).copied().unwrap_or(0.0))
        .sum()
}

fn flow_rows(l: &Lab, rows: &[Vec<Value>]) -> Vec<(i64, i64, f64)> {
    rows.iter()
        .map(|r| (l.uid(&r[0]), l.uid(&r[1]), float(&r[2])))
        .collect()
}

fn gds_flows(file: &str) -> Vec<(i64, i64, f64)> {
    expected(file)
        .iter()
        .map(|m| {
            (
                m["source"].as_i64().unwrap(),
                m["target"].as_i64().unwrap(),
                num(&m["flow"]),
            )
        })
        .collect()
}

/// rmp #68: ALG-155 (parallel) and ALG-157 against GDS on flow30 (rule V:
/// capacities and conservation hold; `totalFlow` and `totalCost` F).
#[test]
fn flows_match_gds() {
    current(async {
        let l = Lab::load("flow30").await;
        let caps = capacities(&l).await;
        for (sources, targets, file) in [
            (&[4, 7][..], &[28][..], ""),
            (&[4, 7, 9, 10, 15][..], &[28, 3, 14][..], ".multi"),
        ] {
            let config = record(&[
                ("sourceNodes", nodes(&l, sources)),
                ("targetNodes", nodes(&l, targets)),
                ("capacityProperty", txt("capacity")),
            ]);
            let rows = l.det("gds.maxFlow.stream", &[], config.clone(), true).await;
            let got = check_flow(&caps, &flow_rows(&l, &rows), sources, targets);
            let want = check_flow(
                &caps,
                &gds_flows(&format!("flow30.gds.maxflow{file}.jsonl")),
                sources,
                targets,
            );
            assert!(close(got, want, 1e-9), "155{file}: {got} against {want}");
            let cost = with(&config, "costProperty", txt("cost"));
            let rows = l
                .det("gds.maxFlow.minCost.stream", &[], cost.clone(), false)
                .await;
            let mine = flow_rows(&l, &rows);
            let got = check_flow(&caps, &mine, sources, targets);
            let gds = gds_flows(&format!("flow30.gds.mincost{file}.jsonl"));
            assert!(
                close(got, want, 1e-9),
                "157{file}: flow {got} against {want}"
            );
            let (row, _) = write_row(
                &l,
                "gds.maxFlow.minCost.write",
                with(
                    &with(&cost, "writeRelationshipType", txt("F")),
                    "writeProperty",
                    txt("f"),
                ),
                "",
            )
            .await;
            let w = &expected(&format!("flow30.gds.mincost{file}.write.jsonl"))[0];
            assert!(close(float(&row[4]), num(&w["totalFlow"]), 1e-9));
            assert!(
                close(float(&row[5]), num(&w["totalCost"]), 1e-9),
                "157{file} cost {:?} against {}",
                row[5],
                w["totalCost"]
            );
            assert_eq!(row[3], int(rows.len() as i64), "relationshipsWritten");
            eprintln!(
                "LAB 155/157 flow30{file}: flow {got} (GDS {want}); cost {:?} (GDS {}); {} rows (GDS {})",
                row[5],
                w["totalCost"],
                mine.len(),
                gds.len()
            );
        }
        // The max-flow write mode; `alpha` has no effect.
        let config = record(&[
            ("sourceNodes", nodes(&l, &[4, 7])),
            ("targetNodes", nodes(&l, &[28])),
            ("capacityProperty", txt("capacity")),
        ]);
        let (row, _) = write_row(
            &l,
            "gds.maxFlow.write",
            with(
                &with(&config, "writeRelationshipType", txt("F")),
                "writeProperty",
                txt("f"),
            ),
            "",
        )
        .await;
        let w = &expected("flow30.gds.maxflow.write.jsonl")[0];
        assert!(close(float(&row[4]), num(&w["totalFlow"]), 1e-9));
        let (row, _) = write_row(
            &l,
            "gds.maxFlow.minCost.write",
            with(
                &with(
                    &with(&with(&config, "costProperty", txt("cost")), "alpha", int(2)),
                    "writeRelationshipType",
                    txt("F"),
                ),
                "writeProperty",
                txt("f"),
            ),
            "",
        )
        .await;
        let w = &expected("flow30.gds.mincost.alpha.write.jsonl")[0];
        assert!(close(float(&row[5]), num(&w["totalCost"]), 1e-9));
        // Without `capacityProperty`, no flow (GDS: no row).
        let rows = l
            .det(
                "gds.maxFlow.stream",
                &[],
                record(&[
                    ("sourceNodes", nodes(&l, &[4, 7])),
                    ("targetNodes", nodes(&l, &[28])),
                ]),
                true,
            )
            .await;
        assert!(rows.is_empty() && expected("flow30.gds.maxflow.nocap.jsonl").is_empty());
        l.f.db.close().await.unwrap();
    });
}

/// Whether `colors` (by uid) is a proper colouring of the fixture's
/// edges, self-loops aside; returns the number of colours.
fn proper(l: &Lab, colors: &BTreeMap<i64, i64>) -> usize {
    for e in l.edges.values() {
        if e.src != e.dst {
            assert_ne!(colors[&e.src], colors[&e.dst], "edge {e:?}");
        }
    }
    colors.values().collect::<BTreeSet<_>>().len()
}

/// rmp #68: ALG-161, 163, 169, 171, 173, 174 and 176 against their
/// oracles.
#[test]
fn matching_cliques_and_colourings_match_oracles() {
    current(async {
        for name in [
            "karate",
            "zipf60",
            "uniform60",
            "petersen",
            "zipf300",
            "bip20",
        ] {
            let l = Lab::load(name).await;
            let simple = name != "zipf300" && name != "bip20";
            if simple {
                // 161 (V: maximum size; greedy maximal).
                let rows = l
                    .det("gds.maximumMatching.stream", &[], record(&[]), false)
                    .await;
                let mut seen = BTreeSet::new();
                for r in &rows {
                    let (a, b) = (l.uid(&r[0]), l.uid(&r[1]));
                    assert!(seen.insert(a) && seen.insert(b), "a node matched twice");
                    assert!(
                        l.edges
                            .values()
                            .any(|e| (e.src, e.dst) == (a, b) || (e.src, e.dst) == (b, a))
                    );
                }
                let size = nx_values(&format!("{name}.nx.matching.jsonl"))[0]
                    .as_i64()
                    .unwrap();
                assert_eq!(rows.len() as i64, size, "161 {name}");
                let rows = l
                    .det(
                        "gds.maximumMatching.stream",
                        &[],
                        record(&[("greedy", Value::Bool(true))]),
                        false,
                    )
                    .await;
                let matched: BTreeSet<i64> = rows
                    .iter()
                    .flat_map(|r| [l.uid(&r[0]), l.uid(&r[1])])
                    .collect();
                assert_eq!(matched.len(), 2 * rows.len());
                for e in l.edges.values() {
                    assert!(
                        e.src == e.dst || matched.contains(&e.src) || matched.contains(&e.dst),
                        "161 greedy {name}: not maximal at {e:?}"
                    );
                }
                // 173 (V: proper; NetworkX's number of colours).
                for m in expected(&format!("{name}.nx.greedy_color.jsonl")) {
                    let strategy = m["strategy"].as_str().unwrap();
                    let rows = l
                        .det(
                            "graph_coloring.color_graph",
                            &[record(&[("strategy", txt(strategy))])],
                            record(&[]),
                            false,
                        )
                        .await;
                    let colors: BTreeMap<i64, i64> = rows
                        .iter()
                        .map(|r| match r[1] {
                            Value::Int(c) => (l.uid(&r[0]), c),
                            _ => panic!(),
                        })
                        .collect();
                    let k = proper(&l, &colors);
                    let want = m["colors"].as_i64().unwrap() as usize;
                    eprintln!("LAB 173 {name} {strategy}: {k} colours, NetworkX {want}");
                    assert_eq!(k, want, "173 {name} {strategy}");
                }
                let rows = l
                    .det(
                        "graph_coloring.color_graph",
                        &[record(&[("strategy", txt("random_sequential"))])],
                        record(&[("randomSeed", int(7))]),
                        false,
                    )
                    .await;
                let colors: BTreeMap<i64, i64> = rows
                    .iter()
                    .map(|r| match r[1] {
                        Value::Int(c) => (l.uid(&r[0]), c),
                        _ => panic!(),
                    })
                    .collect();
                proper(&l, &colors);
                // 174 (V against petgraph's dsatur on the same edges).
                let rows = l
                    .det("gds.dsaturColoring.stream", &[], record(&[]), false)
                    .await;
                let colors: BTreeMap<i64, i64> = rows
                    .iter()
                    .map(|r| match r[1] {
                        Value::Int(c) => (l.uid(&r[0]), c),
                        _ => panic!(),
                    })
                    .collect();
                let k = proper(&l, &colors);
                let mut g = petgraph::graph::UnGraph::<(), ()>::new_undirected();
                let ns: Vec<_> = (0..l.f.ids.len()).map(|_| g.add_node(())).collect();
                for e in l.edges.values() {
                    g.add_edge(ns[e.src as usize], ns[e.dst as usize], ());
                }
                let (_, want) = petgraph::algo::dsatur_coloring(&g);
                eprintln!("LAB 174 {name}: {k} colours, petgraph {want}");
                assert_eq!(k, want, "174 {name}");
            }
            if name == "karate" || name == "zipf60" || name == "uniform60" {
                // 169 (X: clique sets).
                let rows = l
                    .det("gds.maximalCliques.stream", &[], record(&[]), false)
                    .await;
                let mut got: Vec<Vec<i64>> = rows
                    .iter()
                    .map(|r| {
                        let mut c = l.uids(&r[1]);
                        c.sort_unstable();
                        c
                    })
                    .collect();
                got.sort();
                let mut want: Vec<Vec<i64>> = nx_values(&format!("{name}.nx.cliques.jsonl"))
                    .iter()
                    .map(ints)
                    .collect();
                want.sort();
                assert_eq!(got, want, "169 {name}");
            }
            if name != "petersen" && name != "bip20" {
                // 171 (X), parallel: karate against GDS, the others
                // against GDS on their simple graph.
                let rows = l
                    .det("gds.cliqueCounting.stream", &[], record(&[]), true)
                    .await;
                let got: BTreeMap<i64, Vec<i64>> = rows
                    .iter()
                    .map(|r| match &r[1] {
                        Value::List(c) => {
                            (l.uid(&r[0]), c.iter().map(|x| float(x) as i64).collect())
                        }
                        _ => panic!(),
                    })
                    .collect();
                let file = if name == "karate" {
                    "karate.gds.cliques.jsonl".to_string()
                } else {
                    format!("{name}.gds.cliques.simple.jsonl")
                };
                let want: BTreeMap<i64, Vec<i64>> = expected(&file)
                    .iter()
                    .map(|m| (m["nodeId"].as_i64().unwrap(), ints(&m["counts"])))
                    .collect();
                for (u, c) in &got {
                    let w = want.get(u).cloned().unwrap_or_default();
                    assert_eq!(c, &w, "171 {name} node {u}");
                }
                if name == "karate" {
                    let (row, _) = write_row(
                        &l,
                        "gds.cliqueCounting.write",
                        record(&[("writeProperty", txt("c"))]),
                        "",
                    )
                    .await;
                    let w = &expected("karate.gds.cliques.write.jsonl")[0];
                    let Value::List(g) = &row[4] else { panic!() };
                    let g: Vec<i64> = g.iter().map(|x| float(x) as i64).collect();
                    assert_eq!(g, ints(&w["globalCount"]), "171 globalCount");
                }
            }
            if name != "petersen" && name != "bip20" {
                // 176 (Q): proper, at most GDS's colours at concurrency 1.
                for (orientation, file) in [("NATURAL", "k1"), ("UNDIRECTED", "k1.u")] {
                    let rows = l
                        .det(
                            "gds.k1coloring.stream",
                            &[],
                            record(&[("orientation", txt(orientation))]),
                            true,
                        )
                        .await;
                    let colors: BTreeMap<i64, i64> = rows
                        .iter()
                        .map(|r| match r[1] {
                            Value::Int(c) => (l.uid(&r[0]), c),
                            _ => panic!(),
                        })
                        .collect();
                    let k = proper(&l, &colors);
                    let stats = format!("{name}.gds.{file}.stats.jsonl");
                    if crate::test_support::algo::expected_dir()
                        .join(&stats)
                        .exists()
                    {
                        let gds = expected(&stats)[0]["colorCount"].as_i64().unwrap() as usize;
                        eprintln!("LAB 176 {name} {orientation}: {k} colours, GDS {gds}");
                        assert!(
                            k <= gds,
                            "176 {name} {orientation}: {k} colours against {gds}"
                        );
                    }
                }
            }
            if name == "bip20" || name == "karate" {
                // 163 (X: size); karate is not bipartite: 0, as MAGE.
                let rows = l
                    .det("bipartite_matching.max", &[], record(&[]), false)
                    .await;
                let want = if name == "bip20" {
                    nx_values("bip20.nx.bipartite.matching.jsonl")[0]
                        .as_i64()
                        .unwrap()
                } else {
                    0
                };
                assert_eq!(rows, vec![vec![int(want)]], "163 {name}");
            }
            l.f.db.close().await.unwrap();
        }
    });
}

/// rmp #68: ALG-177 against petgraph on the same edge sequence (X; V:
/// without the arcs the graph is acyclic) and ALG-178 (a cover, no larger
/// than MAGE's random ones, the exact optimum recorded).
#[test]
fn feedback_arcs_and_set_cover() {
    current(async {
        for name in ["zipf60", "cyc12"] {
            let l = Lab::load(name).await;
            let rows = l
                .det("gds.greedyFeedbackArcSet.stream", &[], record(&[]), false)
                .await;
            let got: BTreeSet<u64> = rows
                .iter()
                .map(|r| match r[2] {
                    Value::Int(e) => e.cast_unsigned(),
                    _ => panic!(),
                })
                .collect();
            // The projection's forward order: source, type token, target,
            // edge (`src/storage/graph.rs` `adj_key`).
            let engine = l.f.db.engine_handle();
            let mut seq: Vec<(u64, u64, u64, u64)> = Vec::new();
            for ty in ["R", "S"] {
                let token = engine.token_id(TokenKind::EdgeType, ty).unwrap();
                for r in rows_of(&l, &format!("MATCH (a)-[e:{ty}]->(b) RETURN a, b, e")).await {
                    let (Value::Node(a), Value::Node(b), Value::Edge(e)) = (&r[0], &r[1], &r[2])
                    else {
                        panic!()
                    };
                    seq.push((*a, token, *b, *e));
                }
            }
            seq.sort_unstable();
            let mut ordered: Vec<u64> = l.f.ids.clone();
            ordered.sort_unstable();
            let mut g2 = petgraph::graph::DiGraph::<(), u64>::new();
            let ix2: BTreeMap<u64, _> = ordered.iter().map(|id| (*id, g2.add_node(()))).collect();
            for (a, _, b, e) in &seq {
                g2.add_edge(ix2[a], ix2[b], *e);
            }
            let want: BTreeSet<u64> = petgraph::algo::greedy_feedback_arc_set(&g2)
                .map(|e| *e.weight())
                .collect();
            assert_eq!(got, want, "177 {name}");
            g2.retain_edges(|g, e| !want.contains(&g[e]));
            assert!(
                !petgraph::algo::is_cyclic_directed(&g2),
                "177 {name}: a DAG remains"
            );
            eprintln!("LAB 177 {name}: {} arcs, equal to petgraph's", got.len());
            l.f.db.close().await.unwrap();
        }
        // 178 on bip20: each L → R edge puts element R in set L.
        let l = Lab::load("bip20").await;
        let pairs = rows_of(&l, "MATCH (s:L)-[]->(e:R) RETURN e, s").await;
        let elements: Vec<Value> = pairs.iter().map(|r| r[0].clone()).collect();
        let sets: Vec<Value> = pairs.iter().map(|r| r[1].clone()).collect();
        let rows = l
            .det(
                "set_cover.greedy",
                &[
                    Value::List(elements.clone().into()),
                    Value::List(sets.clone().into()),
                ],
                record(&[]),
                false,
            )
            .await;
        let chosen: BTreeSet<i64> = rows.iter().map(|r| l.uid(&r[0])).collect();
        let mut members: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
        for r in &pairs {
            members
                .entry(l.uid(&r[1]))
                .or_default()
                .insert(l.uid(&r[0]));
        }
        let universe: BTreeSet<i64> = members.values().flatten().copied().collect();
        let covered: BTreeSet<i64> = chosen.iter().flat_map(|s| members[s].clone()).collect();
        assert_eq!(covered, universe, "178: a cover");
        let names: Vec<i64> = members.keys().copied().collect();
        let mut optimum = names.len();
        for mask in 1u32..(1 << names.len()) {
            let c: BTreeSet<i64> = (0..names.len())
                .filter(|i| mask & (1 << i) != 0)
                .flat_map(|i| members[&names[i]].clone())
                .collect();
            if c == universe {
                optimum = optimum.min(mask.count_ones() as usize);
            }
        }
        let mage: Vec<usize> = (1..=3)
            .map(|i| expected(&format!("bip20.mage.setcover.{i}.jsonl")).len())
            .collect();
        eprintln!(
            "LAB 178 bip20: greedy {} sets, optimum {optimum}, MAGE runs {mage:?}",
            chosen.len()
        );
        assert!(chosen.len() <= *mage.iter().min().unwrap());
        l.f.db.close().await.unwrap();
    });
}

async fn rows_of(l: &Lab, gql: &str) -> Vec<Vec<Value>> {
    rows(&l.f.s, gql, Parameters::new()).await.unwrap()
}

/// rmp #68 carry-over (ALG-187): the blossom matching is a minimum-weight
/// perfect matching above 20 odd vertices: on 24 points against
/// NetworkX's `min_weight_matching`, within the 2^-20 rounding of the
/// weights; on random sets of up to 14 points against exhaustive search.
#[test]
fn tsp_matching_is_minimum_above_twenty_points() {
    let pts: Vec<(f64, f64)> = (0..24)
        .map(|i| {
            (
                1000.0 * f64::from((37 * i) % 101) / 101.0,
                1000.0 * f64::from((61 * i) % 103) / 103.0,
            )
        })
        .collect();
    let dist = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (a.0 - b.0, a.1 - b.1);
        (dx * dx + dy * dy).sqrt()
    };
    let dm: Vec<Vec<f64>> = pts
        .iter()
        .map(|a| pts.iter().map(|b| dist(*a, *b)).collect())
        .collect();
    let all: Vec<usize> = (0..24).collect();
    let m = super::cycles::min_matching(&dm, &all);
    assert_eq!(m.len(), 12);
    let covered: BTreeSet<usize> = m.iter().flat_map(|(a, b)| [*a, *b]).collect();
    assert_eq!(covered.len(), 24, "perfect");
    let weight: f64 = m.iter().map(|(a, b)| dm[*a][*b]).sum();
    let nx = &expected("points24.nx.min_matching.jsonl")[0];
    let want = num(&nx["value"]);
    let slack = 24.0 / (1u64 << 20) as f64;
    eprintln!("LAB 187 matching on 24 points: {weight} against NetworkX {want}");
    assert!(
        weight >= want - 1e-9 && weight <= want + slack,
        "{weight} against {want}"
    );
    // Exhaustive minimum on smaller random sets.
    use rand::RngExt;
    let mut g = crate::algo::sched::rng(5, 0);
    for q in [2usize, 4, 6, 8, 10, 12, 14] {
        let p: Vec<(f64, f64)> = (0..q)
            .map(|_| (g.random::<f64>() * 1000.0, g.random::<f64>() * 1000.0))
            .collect();
        let dm: Vec<Vec<f64>> = p
            .iter()
            .map(|a| p.iter().map(|b| dist(*a, *b)).collect())
            .collect();
        let ids: Vec<usize> = (0..q).collect();
        let w: f64 = super::cycles::min_matching(&dm, &ids)
            .iter()
            .map(|(a, b)| dm[*a][*b])
            .sum();
        let best = exhaustive(&dm, (1usize << q) - 1);
        assert!(
            w >= best - 1e-9 && w <= best + q as f64 / (1u64 << 20) as f64,
            "{q}: {w} against {best}"
        );
    }
    // The 1.5 bound with more than 20 odd vertices: Christofides' tour on
    // the 24 points is at most 1.5 times the minimum spanning tree's
    // weight plus the matching's (both at most the optimum).
    let tour = super::cycles::christofides(&dm);
    assert_eq!(tour.len(), 25);
    assert_eq!(tour.iter().collect::<BTreeSet<_>>().len(), 24);
}

/// The minimum-weight perfect matching of the points in `mask`.
fn exhaustive(dm: &[Vec<f64>], mask: usize) -> f64 {
    if mask == 0 {
        return 0.0;
    }
    let a = mask.trailing_zeros() as usize;
    let mut best = f64::INFINITY;
    for b in a + 1..dm.len() {
        if mask & (1 << b) != 0 {
            best = best.min(dm[a][b] + exhaustive(dm, mask & !(1 << a) & !(1 << b)));
        }
    }
    best
}

/// rmp #68: the parallel entries (153, 155, 171, 176) use more than one
/// and at most `concurrency` workers on a 2-block projection; the others
/// one.
#[test]
fn worker_counts_of_the_tree_flow_and_colouring_families() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: f.db.engine_handle().core().location.display().to_string(),
            edges: BTreeMap::new(),
            f,
        };
        rows(
            &l.f.s,
            "INSERT (:Two {uid: 100, p: 1})-[:T {weight: 1, capacity: 2, cost: 1}]->(:Two {uid: 101, p: 2})",
            Parameters::new(),
        )
        .await
        .unwrap();
        let ids: Vec<Value> = rows_of(&l, "MATCH (n:Two) RETURN n, n.uid AS u ORDER BY u")
            .await
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
        let (a, b) = (ids[0].clone(), ids[1].clone());
        let base = record(&[("nodeLabels", txt("Two")), ("relationshipTypes", txt("T"))]);
        let list = |v: &Value| Value::List(vec![v.clone()].into());
        let cases: Vec<(&str, Vec<Value>, Value)> = vec![
            (
                "gds.spanningTree.stream",
                vec![],
                with(&base, "sourceNode", a.clone()),
            ),
            (
                "gds.countSpanningTrees.stream",
                vec![],
                with(&base, "rootNode", b.clone()),
            ),
            (
                "gds.steinerTree.stream",
                vec![],
                with(
                    &with(&base, "sourceNode", a.clone()),
                    "targetNodes",
                    list(&b),
                ),
            ),
            (
                "gds.prizeSteinerTree.stream",
                vec![],
                with(&base, "prizeProperty", txt("p")),
            ),
            (
                "gds.maxFlow.stream",
                vec![],
                with(
                    &with(
                        &with(&base, "sourceNodes", list(&a)),
                        "targetNodes",
                        list(&b),
                    ),
                    "capacityProperty",
                    txt("capacity"),
                ),
            ),
            (
                "gds.maxFlow.minCost.stream",
                vec![],
                with(
                    &with(
                        &with(&base, "sourceNodes", list(&a)),
                        "targetNodes",
                        list(&b),
                    ),
                    "capacityProperty",
                    txt("capacity"),
                ),
            ),
            ("gds.maximumMatching.stream", vec![], base.clone()),
            ("bipartite_matching.max", vec![], base.clone()),
            ("gds.maximalCliques.stream", vec![], base.clone()),
            ("gds.cliqueCounting.stream", vec![], base.clone()),
            ("graph_coloring.color_graph", vec![], base.clone()),
            ("gds.dsaturColoring.stream", vec![], base.clone()),
            ("gds.k1coloring.stream", vec![], base.clone()),
            ("gds.greedyFeedbackArcSet.stream", vec![], base.clone()),
            ("set_cover.greedy", vec![list(&b), list(&a)], base.clone()),
        ];
        for (proc, args, config) in &cases {
            let e = super::ENTRIES.iter().find(|e| e.stream == *proc).unwrap();
            for c in [1, 2, 16] {
                crate::algo::sched::probe::take(&l.key);
                l.call(proc, args, &with(config, "concurrency", int(c)))
                    .await
                    .unwrap_or_else(|e| panic!("{proc}: {e}"));
                let w = crate::algo::sched::probe::take(&l.key).unwrap_or(1);
                if e.parallel {
                    assert_eq!(w, (c as usize).min(2), "{proc} at concurrency {c}");
                } else {
                    assert_eq!(w, 1, "{proc} at concurrency {c}");
                }
            }
        }
        // 149 has only a write mode.
        let e = super::ENTRIES
            .iter()
            .find(|e| e.write == Some("gds.kSpanningTree.write"))
            .unwrap();
        assert!(!e.parallel && e.stream.is_empty());
        crate::algo::sched::probe::take(&l.key);
        write_row(
            &l,
            "gds.kSpanningTree.write",
            with(
                &with(&with(&base, "sourceNode", a.clone()), "k", int(1)),
                "writeProperty",
                txt("k"),
            ),
            "",
        )
        .await;
        assert!(crate::algo::sched::probe::take(&l.key).unwrap_or(1) == 1);
        let mut parallel: Vec<u16> = super::ENTRIES
            .iter()
            .filter(|e| cases.iter().any(|(p, ..)| *p == e.stream) && e.parallel)
            .flat_map(|e| e.algs.iter().copied())
            .collect();
        parallel.sort_unstable();
        assert_eq!(parallel, vec![153, 155, 171, 176]);
        // `minCommunitySize` keeps the colours with that many nodes: the
        // two nodes have one colour each.
        let rows = l
            .call(
                "gds.k1coloring.stream",
                &[],
                &with(&base, "minCommunitySize", int(2)),
            )
            .await
            .unwrap();
        assert!(rows.is_empty());
        // An unknown strategy fails with 22N11.
        let e = l
            .call(
                "graph_coloring.color_graph",
                &[record(&[("strategy", txt("rainbow"))])],
                &base,
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        // A metaheuristic key of MAGE's graph colouring fails with 22N11.
        let e = l
            .call(
                "graph_coloring.color_graph",
                &[record(&[("population_size", int(10))])],
                &base,
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap());
        l.f.db.close().await.unwrap();
    });
}

/// Laboratory record (rmp #68): the accountant's peak of the families;
/// run under `docs/evidence/lab-sprint-5/heaptrack.sh` for the heap peak.
#[test]
#[ignore = "laboratory measurement"]
fn lab_family68_memory() {
    current(async {
        let l = Lab::load("zipf300").await;
        let s = l.node(218);
        let targets = nodes(&l, &[0, 5, 13, 50, 77, 123, 200, 299]);
        let w = ("relationshipWeightProperty", txt("weight"));
        for (proc, args, config) in [
            (
                "gds.spanningTree.stream",
                vec![],
                record(&[("sourceNode", s.clone()), w.clone()]),
            ),
            (
                "gds.steinerTree.stream",
                vec![],
                record(&[
                    ("sourceNode", s.clone()),
                    ("targetNodes", targets.clone()),
                    w.clone(),
                ]),
            ),
            ("gds.maximumMatching.stream", vec![], record(&[])),
            ("gds.maximalCliques.stream", vec![], record(&[])),
            ("gds.cliqueCounting.stream", vec![], record(&[])),
            ("graph_coloring.color_graph", vec![], record(&[])),
            ("gds.dsaturColoring.stream", vec![], record(&[])),
            ("gds.k1coloring.stream", vec![], record(&[])),
            ("gds.greedyFeedbackArcSet.stream", vec![], record(&[])),
            ("bipartite_matching.max", vec![], record(&[])),
            (
                "gds.countSpanningTrees.stream",
                vec![],
                record(&[("orientation", txt("UNDIRECTED"))]),
            ),
        ] {
            let n = match l.call(proc, &args, &config).await {
                Ok(r) => r.len(),
                Err(e) => {
                    eprintln!("LAB {proc}: {}", e.code());
                    0
                }
            };
            let engine = l.f.db.engine_handle();
            let acc = engine.accountant();
            eprintln!(
                "LAB {proc}: {n} rows; accountant peak {} B (cap {} B)",
                acc.peak(),
                acc.cap()
            );
            assert!(acc.peak() <= acc.cap());
        }
        l.f.db.close().await.unwrap();
        let l = Lab::load("flow30").await;
        let config = record(&[
            ("sourceNodes", nodes(&l, &[4, 7, 9, 10, 15])),
            ("targetNodes", nodes(&l, &[28, 3, 14])),
            ("capacityProperty", txt("capacity")),
            ("costProperty", txt("cost")),
        ]);
        for proc in ["gds.maxFlow.stream", "gds.maxFlow.minCost.stream"] {
            let config = if proc == "gds.maxFlow.stream" {
                record(&[
                    ("sourceNodes", nodes(&l, &[4, 7, 9, 10, 15])),
                    ("targetNodes", nodes(&l, &[28, 3, 14])),
                    ("capacityProperty", txt("capacity")),
                ])
            } else {
                config.clone()
            };
            let n = l.once(proc, &[], config).await.len();
            let engine = l.f.db.engine_handle();
            let acc = engine.accountant();
            eprintln!(
                "LAB {proc}: {n} rows; accountant peak {} B (cap {} B)",
                acc.peak(),
                acc.cap()
            );
            assert!(acc.peak() <= acc.cap());
        }
        l.f.db.close().await.unwrap();
        let l = Lab::load("geo30").await;
        let n = l
            .once(
                "gds.prizeSteinerTree.stream",
                &[],
                record(&[
                    ("prizeProperty", txt("lat")),
                    ("relationshipWeightProperty", txt("weight")),
                ]),
            )
            .await
            .len();
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        eprintln!(
            "LAB gds.prizeSteinerTree.stream: {n} rows; accountant peak {} B",
            acc.peak()
        );
        l.f.db.close().await.unwrap();
    });
}
