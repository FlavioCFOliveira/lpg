//! Tests of the cycle, isomorphism and derived-structure families (rmp
//! #69): each entry against its oracle (`algorithm-entries.md`
//! Verification), bit-equal rows at `concurrency` 1 to 16 twice, `CALL`
//! equal to the Rust API, one worker at `concurrency` 16, the memory cap,
//! write modes. Helpers are those of `tests_paths.rs`.

use std::collections::{BTreeMap, BTreeSet};

use super::cycles::{christofides, greedy, metres, route_length, two_approx};
use super::tests_paths::{CAP, Lab, current, int, ints, nx_values, path_nodes, strs, txt};
use crate::test_support::algo::{Fixture, drain, expected, param, record, rows, with};
use crate::types::Value;
use crate::{Parameters, TransactionOptions};

/// A cycle as a sorted node multiset.
fn sorted(mut c: Vec<i64>) -> Vec<i64> {
    c.sort_unstable();
    c
}

/// A cycle up to rotation: rotated to start at its smallest node.
fn rotated(mut c: Vec<i64>) -> Vec<i64> {
    if let Some(at) = (0..c.len()).min_by_key(|i| c[*i]) {
        c.rotate_left(at);
    }
    c
}

/// rmp #69 (ALG-180, 181, 189, 190, 194, 203, 204, 205, 206) against
/// their oracles on the shared fixtures, petgraph's isomorphism test
/// graphs and seeded graphs.
#[test]
fn structure_families_match_oracles() {
    current(async {
        // 180 against MAGE (X: cycles as node sets).
        for name in ["petersen", "cyc12"] {
            let l = Lab::load(name).await;
            let rows = l.det("cycles.get", &[], record(&[]), false).await;
            let mut got: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
            for r in &rows {
                let Value::Int(id) = r[0] else { panic!() };
                got.entry(id).or_default().push(l.uid(&r[1]));
            }
            let mut want: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
            for m in expected(&format!("{name}.mage.cycles.jsonl")) {
                want.entry(m["cycle_id"].as_i64().unwrap())
                    .or_default()
                    .push(m["node"].as_i64().unwrap());
            }
            let mut g: Vec<Vec<i64>> = got.into_values().map(sorted).collect();
            let mut w: Vec<Vec<i64>> = want.into_values().map(sorted).collect();
            g.sort();
            w.sort();
            assert_eq!(g, w, "180 {name}");
            eprintln!("LAB 180 {name}: {} cycles, equal to MAGE's", g.len());
            // 194 (X bipartite) on the Petersen graph.
            if name == "petersen" {
                let rows = l
                    .det("gds.isBipartiteUndirected.stream", &[], record(&[]), false)
                    .await;
                let bip = nx_values("petersen.nx.bipartite.jsonl")[0]
                    .as_bool()
                    .unwrap();
                assert_eq!(!rows.is_empty(), bip, "194 {name}");
            }
            l.f.db.close().await.unwrap();
        }
        // 181 against NetworkX (X: cycles up to rotation).
        for name in ["cyc12", "dag40"] {
            let l = Lab::load(name).await;
            let rows = l
                .det("gds.elementaryCircuits.stream", &[], record(&[]), false)
                .await;
            let mut got: Vec<Vec<i64>> = rows
                .iter()
                .map(|r| {
                    let nodes = l.uids(&r[1]);
                    let mut closed = nodes.clone();
                    closed.push(nodes[0]);
                    assert_eq!(path_nodes(&l, &r[2]), closed);
                    l.check_path(&r[2], &closed, None, false, false);
                    rotated(nodes)
                })
                .collect();
            let mut want: Vec<Vec<i64>> = nx_values(&format!("{name}.nx.cycles.jsonl"))
                .iter()
                .map(|v| rotated(ints(v)))
                .collect();
            got.sort();
            want.sort();
            assert_eq!(got, want, "181 {name}");
            // 205 and 206 on the DAG, the refusal on a cyclic graph.
            if name == "dag40" {
                check_closure_reduction(&l).await;
            } else {
                for proc in [
                    "gds.transitiveClosure.stream",
                    "gds.transitiveReduction.stream",
                ] {
                    let e = l.call(proc, &[], &record(&[])).await.unwrap_err();
                    assert_eq!(e.code(), "22N11", "{proc} on a cyclic projection");
                }
            }
            // 203 on a non-tournament.
            if name == "dag40" {
                let rows = l
                    .det("gds.isTournament.stream", &[], record(&[]), false)
                    .await;
                let t = nx_values("dag40.nx.tournament.jsonl")[0].as_bool().unwrap();
                assert_eq!(rows[0], vec![Value::Bool(t), Value::Bool(false)]);
            }
            l.f.db.close().await.unwrap();
        }
        // 189 against NetworkX, 190 against petgraph (X).
        for (name, a, b) in [("iso100", "P", "Q"), ("tour7", "T", "U")] {
            let l = Lab::load(name).await;
            let config = record(&[
                ("nodeLabels", txt(a)),
                ("relationshipTypes", txt(a)),
                ("otherNodeLabels", txt(b)),
                ("otherRelationshipTypes", txt(b)),
            ]);
            // VF2 on the 100-node pair takes about 1.5 s per run in a debug
            // build: determinism is checked on `tour7`, the pair compared
            // once.
            let det = name == "tour7";
            let rows = if det {
                l.det("gds.isIsomorphic.stream", &[], config.clone(), false)
                    .await
            } else {
                l.once("gds.isIsomorphic.stream", &[], config.clone()).await
            };
            let want = nx_values(&format!("{name}.nx.iso.jsonl"))[0]
                .as_bool()
                .unwrap();
            assert_eq!(rows, vec![vec![Value::Bool(want)]], "189 {name}");
            let rows = if det {
                l.det(
                    "gds.isIsomorphicSubgraph.stream",
                    &[],
                    config.clone(),
                    false,
                )
                .await
            } else {
                l.once("gds.isIsomorphicSubgraph.stream", &[], config.clone())
                    .await
            };
            let (ga, gb) = (petgraph_of(name, a), petgraph_of(name, b));
            let want = petgraph::algo::is_isomorphic_subgraph(&gb, &ga);
            assert_eq!(rows, vec![vec![Value::Bool(want)]], "190 {name}");
            eprintln!("LAB 189/190 {name}: isomorphic {want}");
            if name == "tour7" {
                // 203 against NetworkX's is_tournament and, on a
                // tournament, is_directed_acyclic_graph.
                for label in ["T", "U"] {
                    let rows = l
                        .det(
                            "gds.isTournament.stream",
                            &[],
                            record(&[
                                ("nodeLabels", txt(label)),
                                ("relationshipTypes", txt(label)),
                            ]),
                            false,
                        )
                        .await;
                    let t = nx_values(&format!("tour7.nx.tournament.{label}.jsonl"))[0]
                        .as_bool()
                        .unwrap();
                    let dag = nx_values(&format!("tour7.nx.dag.{label}.jsonl"))[0]
                        .as_bool()
                        .unwrap();
                    assert!(t);
                    assert_eq!(
                        rows[0],
                        vec![Value::Bool(t), Value::Bool(dag)],
                        "203 {label}"
                    );
                }
            }
            l.f.db.close().await.unwrap();
        }
        // 194 on a bipartite graph: the sides are a 2-colouring.
        let l = Lab::load("bip20").await;
        let rows = l
            .det("gds.isBipartiteUndirected.stream", &[], record(&[]), false)
            .await;
        assert!(nx_values("bip20.nx.bipartite.jsonl")[0].as_bool().unwrap());
        assert_eq!(rows.len(), 20);
        let side: BTreeMap<i64, Value> =
            rows.iter().map(|r| (l.uid(&r[0]), r[1].clone())).collect();
        for e in l.edges.values() {
            assert_ne!(side[&e.src], side[&e.dst], "edge {e:?} inside a side");
        }
        l.f.db.close().await.unwrap();
        let l = Lab::load("karate").await;
        let rows = l
            .once("gds.isBipartiteUndirected.stream", &[], record(&[]))
            .await;
        assert_eq!(
            rows.is_empty(),
            !nx_values("karate.nx.bipartite.jsonl")[0].as_bool().unwrap()
        );
        // 204 against NetworkX's is_graphical.
        for m in expected("nx.graphical.jsonl") {
            let seq: Vec<Value> = ints(&m["sequence"]).into_iter().map(int).collect();
            let rows = l
                .det(
                    "gds.isDegreeSequenceGraphlike.stream",
                    &[],
                    record(&[("degreeSequence", Value::List(seq.into()))]),
                    false,
                )
                .await;
            assert_eq!(
                rows[0][0],
                Value::Bool(m["graphical"].as_bool().unwrap()),
                "204 {m:?}"
            );
        }
        l.f.db.close().await.unwrap();
    });
}

/// A petgraph `DiGraph` of the nodes with label `label` and the edges of
/// type `label` of fixture `name`, read from its files in `uid` order.
fn petgraph_of(name: &str, label: &str) -> petgraph::graph::DiGraph<(), ()> {
    let nodes =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir(name).join("nodes.csv"))
            .unwrap();
    let edges =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir(name).join("edges.csv"))
            .unwrap();
    let mut g = petgraph::graph::DiGraph::new();
    let mut ix = BTreeMap::new();
    for line in nodes.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        if f[1].split(';').any(|x| x == label) {
            ix.insert(f[0].to_string(), g.add_node(()));
        }
    }
    for line in edges.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        if f[2] == label {
            g.add_edge(ix[f[0]], ix[f[1]], ());
        }
    }
    g
}

/// ALG-205 and ALG-206 against NetworkX's `transitive_closure_dag` and
/// `transitive_reduction` (X), and their write modes.
async fn check_closure_reduction(l: &Lab) {
    let rows = l
        .det("gds.transitiveClosure.stream", &[], record(&[]), false)
        .await;
    let got: BTreeSet<(i64, i64)> = rows.iter().map(|r| (l.uid(&r[0]), l.uid(&r[1]))).collect();
    assert_eq!(got.len(), rows.len());
    let want: BTreeSet<(i64, i64)> = nx_values("dag40.nx.closure.jsonl")
        .iter()
        .map(|v| (v[0].as_i64().unwrap(), v[1].as_i64().unwrap()))
        .collect();
    assert_eq!(got, want, "205");
    let rows = l
        .det("gds.transitiveReduction.stream", &[], record(&[]), false)
        .await;
    let got: BTreeSet<(i64, i64)> = rows
        .iter()
        .map(|r| {
            let Value::Int(e) = r[2] else { panic!() };
            let e = l.edges[&e.cast_unsigned()];
            let (a, b) = (l.uid(&r[0]), l.uid(&r[1]));
            assert_eq!((e.src, e.dst), (a, b), "the relationship joins the pair");
            (a, b)
        })
        .collect();
    let want: BTreeSet<(i64, i64)> = nx_values("dag40.nx.reduction.jsonl")
        .iter()
        .map(|v| (v[0].as_i64().unwrap(), v[1].as_i64().unwrap()))
        .collect();
    assert_eq!(got, want, "206");
    // Write modes, rolled back.
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    let row = drain(
        tx.execute(
            "CALL gds.transitiveClosure.write($c)",
            param("c", record(&[("writeRelationshipType", txt("TC"))])),
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap()
    .remove(0);
    let pairs = nx_values("dag40.nx.closure.jsonl").len() as i64;
    assert_eq!(row[0], int(pairs), "relationshipsWritten");
    let row = drain(
        tx.execute(
            "CALL gds.transitiveReduction.write($c)",
            param(
                "c",
                record(&[
                    ("writeProperty", txt("kept")),
                    ("relationshipTypes", txt("R")),
                ]),
            ),
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap()
    .remove(0);
    let kept = drain(
        tx.execute(
            "MATCH ()-[e]->() WHERE e.kept = true RETURN count(*) AS c",
            Parameters::new(),
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(row[0], kept[0][0], "relationshipPropertiesWritten");
    tx.rollback().await.unwrap();
}

/// The shortest closed tour of `dm` through every point from point 0,
/// by enumeration.
fn optimum(dm: &[Vec<f64>]) -> f64 {
    fn permute(rest: &mut Vec<usize>, k: usize, dm: &[Vec<f64>], best: &mut f64) {
        if k == rest.len() {
            let mut prev = 0;
            let mut total = 0.0;
            for v in rest.iter().chain(std::iter::once(&0)) {
                total += dm[prev][*v];
                prev = *v;
            }
            *best = best.min(total);
            return;
        }
        for i in k..rest.len() {
            rest.swap(k, i);
            permute(rest, k + 1, dm, best);
            rest.swap(k, i);
        }
    }
    let mut rest: Vec<usize> = (1..dm.len()).collect();
    let mut best = f64::INFINITY;
    permute(&mut rest, 0, dm, &mut best);
    best
}

/// The distances between the `S` nodes of `geo30` (the first nine), as
/// the entries compute them.
fn geo_points() -> Vec<(f64, f64)> {
    let text =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir("geo30").join("nodes.csv"))
            .unwrap();
    text.lines()
        .skip(1)
        .filter(|l| l.split(',').nth(1) == Some("S"))
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (f[3].parse().unwrap(), f[4].parse().unwrap())
        })
        .collect()
}

/// rmp #69 (ALG-187 rule V, ALG-188 rule Q) on the nine `S` points of
/// `geo30` and on every subset of them of 2 to 8 points in a sample.
#[test]
fn tours_and_routes_meet_their_bounds() {
    current(async {
        let l = Lab::load("geo30").await;
        let pts = geo_points();
        assert_eq!(pts.len(), 9);
        let s_nodes: Vec<Value> = (0..9).map(|u| l.node(u)).collect();
        let dm: Vec<Vec<f64>> = pts
            .iter()
            .map(|a| {
                pts.iter()
                    .map(|b| {
                        if a == b {
                            0.0
                        } else {
                            metres(a.0, a.1, b.0, b.1)
                        }
                    })
                    .collect()
            })
            .collect();
        let opt = optimum(&dm);
        for (method, bound) in [
            ("2_approx", 2.0),
            ("1.5_approx", 1.5),
            ("greedy", f64::INFINITY),
        ] {
            let rows = l
                .det(
                    "tsp.solve",
                    &[Value::List(s_nodes.clone().into()), txt(method)],
                    record(&[]),
                    false,
                )
                .await;
            let srcs = l.uids(&rows[0][0]);
            let dsts = l.uids(&rows[0][1]);
            assert_eq!(srcs.len(), 9);
            assert_eq!(&srcs[1..], &dsts[..8], "a chain");
            assert_eq!(dsts[8], srcs[0], "closed");
            assert_eq!(
                srcs.iter().collect::<BTreeSet<_>>().len(),
                9,
                "every point once"
            );
            let cost: f64 = srcs
                .iter()
                .zip(&dsts)
                .map(|(a, b)| dm[*a as usize][*b as usize])
                .sum();
            assert!(
                cost <= bound * opt * (1.0 + 1e-12),
                "{method}: {cost} against optimum {opt}"
            );
            eprintln!(
                "LAB 187 {method}: tour {cost:.1} m, optimum {opt:.1} m, ratio {:.4}",
                cost / opt
            );
        }
        // The default method is 1.5_approx; the bounds on smaller sets.
        let rows = l
            .once(
                "tsp.solve",
                &[Value::List(s_nodes.clone().into())],
                record(&[]),
            )
            .await;
        let by_default = rows;
        let rows = l
            .once(
                "tsp.solve",
                &[Value::List(s_nodes.clone().into()), txt("1.5_approx")],
                record(&[]),
            )
            .await;
        assert_eq!(by_default, rows);
        let mut worst: (f64, f64) = (0.0, 0.0);
        for mask in 3u32..(1 << 9) {
            if mask.count_ones() < 2 || mask % 7 != 0 {
                continue;
            }
            let idx: Vec<usize> = (0..9).filter(|i| mask & (1 << i) != 0).collect();
            let sub: Vec<Vec<f64>> = idx
                .iter()
                .map(|a| idx.iter().map(|b| dm[*a][*b]).collect())
                .collect();
            let opt = optimum(&sub);
            let cost = |tour: &[usize]| tour.windows(2).map(|w| sub[w[0]][w[1]]).sum::<f64>();
            let (c2, c15) = (cost(&two_approx(&sub)), cost(&christofides(&sub)));
            assert!(c2 <= 2.0 * opt * (1.0 + 1e-12) && c15 <= 1.5 * opt * (1.0 + 1e-12));
            assert!(greedy(&sub).len() == idx.len() + 1);
            worst = (worst.0.max(c2 / opt), worst.1.max(c15 / opt));
        }
        eprintln!(
            "LAB 187 subsets: worst ratio 2_approx {:.4}, 1.5_approx {:.4}",
            worst.0, worst.1
        );
        // 188: depot uid 0, the nine S nodes.
        let s_only = record(&[("nodeLabels", txt("S"))]);
        for vehicles in [Value::Null, int(1), int(3)] {
            let rows = l
                .det(
                    "vrp.route",
                    &[l.node(0), vehicles.clone()],
                    s_only.clone(),
                    false,
                )
                .await;
            let legs: Vec<(i64, i64)> = rows.iter().map(|r| (l.uid(&r[0]), l.uid(&r[1]))).collect();
            // Split the legs into routes at the depot.
            let mut routes: Vec<Vec<usize>> = Vec::new();
            let mut cur = Vec::new();
            for (a, b) in &legs {
                if *a == 0 {
                    assert!(cur.is_empty());
                }
                if *b == 0 {
                    routes.push(std::mem::take(&mut cur));
                } else {
                    cur.push(*b as usize);
                }
            }
            assert!(cur.is_empty(), "every route ends at the depot");
            let visited: Vec<usize> = routes.iter().flatten().copied().collect();
            assert_eq!(visited.iter().collect::<BTreeSet<_>>().len(), 8);
            assert_eq!(visited.len(), 8, "every customer on exactly one route");
            let k = match vehicles {
                Value::Int(k) => k as usize,
                _ => 1,
            };
            assert!(routes.len() <= k, "at most {k} routes");
            let mut total = 0.0;
            for r in &routes {
                let mut copy = r.clone();
                super::cycles::two_opt(&dm, &mut copy);
                assert_eq!(&copy, r, "no 2-opt move shortens a route");
                total += route_length(&dm, r);
            }
            eprintln!(
                "LAB 188 vehicles {vehicles:?}: {} routes, {total:.1} m; optimum {opt:.1} m; ratio {:.4}",
                routes.len(),
                total / opt
            );
        }
        let e = l
            .call("vrp.route", &[l.node(0), int(0)], &s_only)
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        l.f.db.close().await.unwrap();
    });
}

/// rmp #69: each entry of the families runs on one worker (or the
/// statement's thread) at `concurrency` 16, and is classified so.
#[test]
fn worker_counts_of_the_structure_families() {
    current(async {
        let l = Lab::load("tour7").await;
        let cases: Vec<(&str, Vec<Value>, Value)> = vec![
            ("cycles.get", vec![], record(&[])),
            ("gds.elementaryCircuits.stream", vec![], record(&[])),
            (
                "gds.isIsomorphic.stream",
                vec![],
                record(&[
                    ("otherNodeLabels", txt("T")),
                    ("otherRelationshipTypes", txt("T")),
                ]),
            ),
            (
                "gds.isIsomorphicSubgraph.stream",
                vec![],
                record(&[
                    ("otherNodeLabels", txt("T")),
                    ("otherRelationshipTypes", txt("T")),
                ]),
            ),
            ("gds.isBipartiteUndirected.stream", vec![], record(&[])),
            ("gds.isTournament.stream", vec![], record(&[])),
            (
                "gds.isDegreeSequenceGraphlike.stream",
                vec![],
                record(&[("degreeSequence", Value::List(vec![int(1), int(1)].into()))]),
            ),
            (
                "gds.transitiveClosure.stream",
                vec![],
                record(&[("nodeLabels", txt("U")), ("relationshipTypes", txt("U"))]),
            ),
            (
                "gds.transitiveReduction.stream",
                vec![],
                record(&[("nodeLabels", txt("U")), ("relationshipTypes", txt("U"))]),
            ),
        ];
        for (proc, args, config) in cases {
            let e = super::ENTRIES.iter().find(|e| e.stream == proc).unwrap();
            assert!(!e.parallel, "{proc} is single-thread");
            crate::algo::sched::probe::take(&l.key);
            l.call(proc, &args, &with(&config, "concurrency", int(16)))
                .await
                .unwrap_or_else(|e| panic!("{proc}: {e}"));
            let w = crate::algo::sched::probe::take(&l.key);
            assert!(w.is_none() || w == Some(1), "{proc}: {w:?} workers");
        }
        l.f.db.close().await.unwrap();
        // tsp.solve and vrp.route need coordinates.
        let g = Lab::load("geo30").await;
        for (proc, args) in [
            (
                "tsp.solve",
                vec![Value::List(vec![g.node(0), g.node(1), g.node(2)].into())],
            ),
            ("vrp.route", vec![g.node(0), int(2)]),
        ] {
            let e = super::ENTRIES.iter().find(|e| e.stream == proc).unwrap();
            assert!(!e.parallel, "{proc} is single-thread");
            crate::algo::sched::probe::take(&g.key);
            g.call(proc, &args, &record(&[("concurrency", int(16))]))
                .await
                .unwrap_or_else(|e| panic!("{proc}: {e}"));
            let w = crate::algo::sched::probe::take(&g.key);
            assert!(w.is_none() || w == Some(1), "{proc}: {w:?} workers");
        }
        g.f.db.close().await.unwrap();
    });
}

/// rmp #69 (FR-LIM-001): ALG-181 on a graph whose cycles exceed a small
/// transaction memory limit, and ALG-190 under a limit below its search
/// state, fail with 51N73; the accountant's peak never exceeds the cap.
#[test]
fn cycle_and_isomorphism_outputs_respect_the_memory_cap() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: String::new(),
            edges: BTreeMap::new(),
            f,
        };
        // A complete directed graph on 8 nodes: 8 018 elementary circuits.
        let nodes: Vec<String> = (0..8)
            .map(|i| format!("(:K {{uid: {}}})", 2000 + i))
            .collect();
        rows(
            &l.f.s,
            &format!("INSERT {}", nodes.join(", ")),
            Parameters::new(),
        )
        .await
        .unwrap();
        rows(
            &l.f.s,
            "MATCH (a:K), (b:K) WHERE a.uid <> b.uid INSERT (a)-[:KK]->(b)",
            Parameters::new(),
        )
        .await
        .unwrap();
        let k = record(&[("nodeLabels", txt("K")), ("relationshipTypes", txt("KK"))]);
        let ok = l
            .once("gds.elementaryCircuits.stream", &[], k.clone())
            .await;
        let count: usize = (2..=8usize)
            .map(|r| (0..r).fold(1usize, |a, i| a * (8 - i)) / r)
            .sum();
        assert_eq!(ok.len(), count, "the circuits of K8");
        let bytes: u64 = ok.iter().map(|r| crate::query::exec::row_bytes(r)).sum();
        let small = 1 << 20;
        assert!(bytes > small);
        let iso = with(
            &with(&k, "otherNodeLabels", txt("K")),
            "otherRelationshipTypes",
            txt("KK"),
        );
        let fails = |proc: &'static str, config: Value, limit: u64| {
            let l = &l;
            async move {
                let mut limits = l.f.s.limits();
                limits.memory = Some(limit);
                l.f.s.set_limits(limits);
                let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
                let e = match tx
                    .execute(&format!("CALL {proc}($c)"), param("c", config))
                    .await
                {
                    Err(e) => e,
                    Ok(st) => drain(st).await.expect_err("over the limit"),
                };
                let _ = tx.rollback().await;
                limits.memory = None;
                l.f.s.set_limits(limits);
                e
            }
        };
        let e = fails("gds.elementaryCircuits.stream", k.clone(), small).await;
        assert_eq!(e.code(), "51N73", "181: {e}");
        eprintln!(
            "LAB 181: {} circuits, {bytes} B of rows; a {small} B limit fails with 51N73",
            ok.len()
        );
        assert_eq!(
            l.once("gds.isIsomorphicSubgraph.stream", &[], iso.clone())
                .await,
            vec![vec![Value::Bool(true)]]
        );
        let e = fails("gds.isIsomorphicSubgraph.stream", iso, 1024).await;
        assert_eq!(e.code(), "51N73", "190: {e}");
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap());
        // 194's write mode reports the sides and whether it is bipartite.
        let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
        let row = drain(
            tx.execute(
                "CALL gds.isBipartiteUndirected.write($c)",
                param("c", with(&k, "writeProperty", txt("side"))),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap()
        .remove(0);
        assert_eq!(row[..2], [Value::Bool(false), int(0)]);
        tx.rollback().await.unwrap();
        let _ = strs(&[]);
        l.f.db.close().await.unwrap();
    });
}

/// Laboratory record (rmp #69): the accountant's peak of the families on
/// the seeded fixtures; run under `docs/evidence/lab-sprint-5/heaptrack.sh`
/// for the heap peak.
#[test]
#[ignore = "laboratory measurement"]
fn lab_family69_memory() {
    current(async {
        let l = Lab::load("iso100").await;
        let iso = record(&[
            ("nodeLabels", txt("P")),
            ("relationshipTypes", txt("P")),
            ("otherNodeLabels", txt("Q")),
            ("otherRelationshipTypes", txt("Q")),
        ]);
        for (proc, config) in [
            ("gds.isIsomorphic.stream", iso.clone()),
            ("gds.isIsomorphicSubgraph.stream", iso),
            ("gds.elementaryCircuits.stream", record(&[])),
            ("cycles.get", record(&[])),
            ("gds.isBipartiteUndirected.stream", record(&[])),
            ("gds.isTournament.stream", record(&[])),
        ] {
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
    });
}
