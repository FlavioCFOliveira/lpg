//! Tests of the components and structural-measure families (rmp #67):
//! each entry against its oracle (`algorithm-entries.md` Verification),
//! bit-equal rows at `concurrency` 1 to 16 twice, `CALL` equal to the Rust
//! API, the worker-count rule, the memory cap, write modes. Helpers are
//! those of `tests_paths.rs`.

use std::collections::{BTreeMap, BTreeSet};

use super::tests_paths::{CAP, Lab, close, current, int, ints, num, nx_values, txt};
use crate::test_support::algo::{
    Fixture, Rule, compare, drain, expected, expected_by_key, param, record, rows, with,
};
use crate::types::Value;
use crate::{Parameters, TransactionOptions};

/// `rows` keyed by the uid of the node in column 0, with column `col`.
fn by_uid(l: &Lab, rows: &[Vec<Value>], col: usize) -> BTreeMap<i64, Value> {
    rows.iter()
        .map(|r| (l.uid(&r[0]), r[col].clone()))
        .collect()
}

fn float(v: &Value) -> f64 {
    match v {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f64,
        other => panic!("not a number: {other:?}"),
    }
}

fn record_field(v: &Value, k: &str) -> Value {
    let Value::Record(r) = v else {
        panic!("not a record: {v:?}")
    };
    r.iter()
        .find(|(n, _)| &**n == k)
        .map(|(_, x)| x.clone())
        .unwrap_or(Value::Null)
}

/// One write-mode summary row of `proc` with `config`, rolled back.
async fn write_row(l: &Lab, proc: &str, config: Value) -> Vec<Value> {
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    let row = drain(
        tx.execute(&format!("CALL {proc}($c)"), param("c", config))
            .await
            .unwrap_or_else(|e| panic!("{proc}: {e}")),
    )
    .await
    .unwrap_or_else(|e| panic!("{proc}: {e}"))
    .remove(0);
    tx.rollback().await.unwrap();
    row
}

/// Partition comparison of `rows` (node, component) against oracle `file`
/// (rule P).
fn same_partition(l: &Lab, rows: &[Vec<Value>], file: &str) {
    let got = by_uid(l, rows, 1);
    let want = expected_by_key(file, "nodeId", "componentId");
    compare(Rule::Partition, &got, &want).unwrap_or_else(|e| panic!("{file}: {e}"));
}

/// rmp #67: ALG-083, 085, 090, 091, 093, 120, 121 against GDS on the
/// karate club, the seeded Zipf and uniform graphs and zipf300.
#[test]
fn gds_named_component_and_measure_entries_match_gds() {
    current(async {
        for name in ["karate", "zipf60", "uniform60", "zipf300"] {
            let l = Lab::load(name).await;
            let directed = name != "karate";
            // 083 (P), parallel.
            let rows = l.det("gds.wcc.stream", &[], record(&[]), true).await;
            same_partition(&l, &rows, &format!("{name}.gds.wcc.jsonl"));
            let u = record(&[("orientation", txt("UNDIRECTED"))]);
            let rows = l.det("gds.wcc.stream", &[], u, true).await;
            same_partition(&l, &rows, &format!("{name}.gds.wcc.u.jsonl"));
            if directed {
                let rows = l
                    .det(
                        "gds.wcc.stream",
                        &[],
                        record(&[
                            ("relationshipWeightProperty", txt("weight")),
                            ("threshold", Value::Float(0.5)),
                        ]),
                        true,
                    )
                    .await;
                same_partition(&l, &rows, &format!("{name}.gds.wcc.threshold.jsonl"));
                let rows = l
                    .once(
                        "gds.wcc.stream",
                        &[],
                        record(&[("consecutiveIds", Value::Bool(true))]),
                    )
                    .await;
                same_partition(&l, &rows, &format!("{name}.gds.wcc.consecutive.jsonl"));
                let ids: BTreeSet<i64> = rows
                    .iter()
                    .map(|r| match r[1] {
                        Value::Int(i) => i,
                        _ => panic!(),
                    })
                    .collect();
                assert_eq!(ids, (0..ids.len() as i64).collect(), "consecutive ids");
                for (proc, file) in [("gds.wcc.write", "wcc"), ("gds.scc.write", "scc")] {
                    let row = write_row(&l, proc, record(&[("writeProperty", txt("c"))])).await;
                    let want = &expected(&format!("{name}.gds.{file}.write.jsonl"))[0];
                    assert_eq!(row[0], int(want["componentCount"].as_i64().unwrap()));
                    for k in ["min", "max", "p50", "p99"] {
                        assert_eq!(
                            record_field(&row[1], k),
                            int(want["componentDistribution"][k].as_i64().unwrap()),
                            "{proc} {k}"
                        );
                    }
                    assert_eq!(row[6], int(want["written"].as_i64().unwrap()));
                }
                // 085 (P).
                let rows = l.det("gds.scc.stream", &[], record(&[]), false).await;
                same_partition(&l, &rows, &format!("{name}.gds.scc.jsonl"));
            }
            if name == "zipf300" {
                // zipf300 has only the WCC and the cases below.
                let rows = l.det("gds.kcore.stream", &[], record(&[]), true).await;
                let got = by_uid(&l, &rows, 1);
                let want = expected_by_key("zipf300.gds.kcore.jsonl", "nodeId", "coreValue");
                compare(Rule::Exact, &got, &want).unwrap();
                let rows = l
                    .det("gds.triangleCount.stream", &[], record(&[]), true)
                    .await;
                let want = expected_by_key(
                    "zipf300.gds.triangles.count.jsonl",
                    "nodeId",
                    "triangleCount",
                );
                compare(Rule::Exact, &by_uid(&l, &rows, 1), &want).unwrap();
                let rows = l
                    .det(
                        "gds.localClusteringCoefficient.stream",
                        &[],
                        record(&[]),
                        true,
                    )
                    .await;
                let want = expected_by_key("zipf300.gds.lcc.jsonl", "nodeId", "lcc");
                let worst = compare(Rule::Float(1e-9), &by_uid(&l, &rows, 1), &want).unwrap();
                eprintln!("LAB 121 zipf300: worst relative difference {worst:e}");
                l.f.db.close().await.unwrap();
                continue;
            }
            check_low_link(&l, name).await;
            // 093 (X), parallel.
            let rows = l.det("gds.kcore.stream", &[], record(&[]), true).await;
            let want = expected_by_key(&format!("{name}.gds.kcore.jsonl"), "nodeId", "coreValue");
            compare(Rule::Exact, &by_uid(&l, &rows, 1), &want).unwrap();
            let row = write_row(
                &l,
                "gds.kcore.write",
                record(&[("writeProperty", txt("k"))]),
            )
            .await;
            let want = &expected(&format!("{name}.gds.kcore.write.jsonl"))[0];
            assert_eq!(row[0], int(want["written"].as_i64().unwrap()));
            assert_eq!(row[1], int(want["degeneracy"].as_i64().unwrap()));
            // 120 (X), parallel.
            for (config, file) in [
                (record(&[]), "count"),
                (record(&[("maxDegree", int(5))]), "count.max5"),
            ] {
                let rows = l.det("gds.triangleCount.stream", &[], config, true).await;
                let want = expected_by_key(
                    &format!("{name}.gds.triangles.{file}.jsonl"),
                    "nodeId",
                    "triangleCount",
                );
                compare(Rule::Exact, &by_uid(&l, &rows, 1), &want)
                    .unwrap_or_else(|e| panic!("120 {name} {file}: {e}"));
            }
            let rows = l.det("gds.triangles", &[], record(&[]), true).await;
            let got: BTreeSet<Vec<i64>> = rows
                .iter()
                .map(|r| {
                    let mut t: Vec<i64> = r.iter().map(|x| l.uid(x)).collect();
                    t.sort_unstable();
                    t
                })
                .collect();
            assert_eq!(got.len(), rows.len(), "each triangle once");
            let want: BTreeSet<Vec<i64>> = expected(&format!("{name}.gds.triangles.list.jsonl"))
                .iter()
                .map(|m| {
                    let mut t = vec![
                        m["a"].as_i64().unwrap(),
                        m["b"].as_i64().unwrap(),
                        m["c"].as_i64().unwrap(),
                    ];
                    t.sort_unstable();
                    t
                })
                .collect();
            assert_eq!(got, want, "gds.triangles {name}");
            let row = write_row(
                &l,
                "gds.triangleCount.write",
                record(&[("writeProperty", txt("t"))]),
            )
            .await;
            let want = &expected(&format!("{name}.gds.triangles.write.jsonl"))[0];
            assert_eq!(row[0], int(want["global"].as_i64().unwrap()));
            assert_eq!(row[1], int(want["nodeCount"].as_i64().unwrap()));
            // 121 (F), parallel.
            let rows = l
                .det(
                    "gds.localClusteringCoefficient.stream",
                    &[],
                    record(&[]),
                    true,
                )
                .await;
            let want = expected_by_key(&format!("{name}.gds.lcc.jsonl"), "nodeId", "lcc");
            let worst = compare(Rule::Float(1e-9), &by_uid(&l, &rows, 1), &want).unwrap();
            let row = write_row(
                &l,
                "gds.localClusteringCoefficient.write",
                record(&[("writeProperty", txt("c"))]),
            )
            .await;
            let want = &expected(&format!("{name}.gds.lcc.write.jsonl"))[0];
            assert!(close(float(&row[0]), num(&want["average"]), 1e-9));
            eprintln!("LAB 120/121 {name}: equal to GDS; worst LCC difference {worst:e}");
            l.f.db.close().await.unwrap();
        }
    });
}

/// ALG-090 and ALG-091 against GDS (rule X), and 090's write mode.
async fn check_low_link(l: &Lab, name: &str) {
    let rows = l
        .det("gds.articulationPoints.stream", &[], record(&[]), false)
        .await;
    let got: BTreeMap<i64, (Value, Value, Value)> = rows
        .iter()
        .map(|r| {
            (
                l.uid(&r[0]),
                (
                    record_field(&r[1], "count"),
                    record_field(&r[1], "max"),
                    record_field(&r[1], "min"),
                ),
            )
        })
        .collect();
    let want: BTreeMap<i64, (Value, Value, Value)> = expected(&format!("{name}.gds.ap.jsonl"))
        .iter()
        .map(|m| {
            let c = &m["resultingComponents"];
            (
                m["nodeId"].as_i64().unwrap(),
                (
                    int(c["count"].as_i64().unwrap()),
                    int(c["max"].as_i64().unwrap()),
                    int(c["min"].as_i64().unwrap()),
                ),
            )
        })
        .collect();
    assert_eq!(got, want, "090 {name}");
    // The write mode sets 1 on articulation points, 0 elsewhere.
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    let row = drain(
        tx.execute(
            "CALL gds.articulationPoints.write($c)",
            param("c", record(&[("writeProperty", txt("ap"))])),
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap()
    .remove(0);
    let written = drain(
        tx.execute("MATCH (n) RETURN n.uid AS u, n.ap AS ap", Parameters::new())
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    let got: BTreeMap<i64, Value> = written
        .iter()
        .map(|r| match r[0] {
            Value::Int(u) => (u, r[1].clone()),
            _ => panic!(),
        })
        .collect();
    let want = expected_by_key(&format!("{name}.gds.ap.write.jsonl"), "uid", "ap");
    compare(Rule::Exact, &got, &want).unwrap();
    let w = &expected(&format!("{name}.gds.ap.write.jsonl"))[0];
    assert_eq!(
        row[0],
        int(w["count"].as_i64().unwrap()),
        "articulationPointCount"
    );
    assert_eq!(
        row[1],
        int(w["written"].as_i64().unwrap()),
        "nodePropertiesWritten"
    );
    // 091: unordered pairs with each end's side size.
    let rows = l.det("gds.bridges.stream", &[], record(&[]), false).await;
    let side = |a: i64, b: i64, sizes: [i64; 2]| -> BTreeSet<(i64, i64)> {
        [(a, sizes[0]), (b, sizes[1])].into_iter().collect()
    };
    let got: BTreeSet<BTreeSet<(i64, i64)>> = rows
        .iter()
        .map(|r| {
            let s = match &r[2] {
                Value::List(x) => [float(&x[0]) as i64, float(&x[1]) as i64],
                _ => panic!(),
            };
            side(l.uid(&r[0]), l.uid(&r[1]), s)
        })
        .collect();
    let want: BTreeSet<BTreeSet<(i64, i64)>> = expected(&format!("{name}.gds.bridges.jsonl"))
        .iter()
        .map(|m| {
            let s = ints(&m["remainingSizes"]);
            side(
                m["from"].as_i64().unwrap(),
                m["to"].as_i64().unwrap(),
                [s[0], s[1]],
            )
        })
        .collect();
    assert_eq!(got, want, "091 {name}");
    eprintln!(
        "LAB 090/091 {name}: {} articulation points, {} bridges, equal to GDS",
        expected(&format!("{name}.gds.ap.jsonl")).len(),
        want.len()
    );
}

/// rmp #67: ALG-083's seeded form against GDS on zipf60c (rule P; the
/// identifiers equal too).
#[test]
fn seeded_wcc_matches_gds() {
    current(async {
        let l = Lab::load("zipf60c").await;
        let rows = l
            .det(
                "gds.wcc.stream",
                &[],
                record(&[("seedProperty", txt("community"))]),
                true,
            )
            .await;
        same_partition(&l, &rows, "zipf60c.gds.wcc.seed.jsonl");
        let want = expected_by_key("zipf60c.gds.wcc.seed.jsonl", "nodeId", "componentId");
        compare(Rule::Exact, &by_uid(&l, &rows, 1), &want).unwrap();
        l.f.db.close().await.unwrap();
    });
}

/// The `value` node lists of a NetworkX output as a partition by uid.
fn nx_partition(file: &str) -> BTreeMap<i64, Value> {
    let mut out = BTreeMap::new();
    for (i, v) in nx_values(file).iter().enumerate() {
        for u in ints(v) {
            out.insert(u, int(i as i64));
        }
    }
    out
}

/// rmp #67: ALG-084, 086, 089, 123, 128, 143 against NetworkX.
#[test]
fn derived_component_and_measure_entries_match_networkx() {
    current(async {
        for name in ["karate", "zipf60", "uniform60", "cyc12", "zipf300"] {
            let l = Lab::load(name).await;
            if name != "karate" && name != "cyc12" {
                // 084 (P).
                let rows = l
                    .det("gds.connectedComponents.stream", &[], record(&[]), false)
                    .await;
                compare(
                    Rule::Partition,
                    &by_uid(&l, &rows, 1),
                    &nx_partition(&format!("{name}.nx.cc.jsonl")),
                )
                .unwrap();
            }
            if name == "zipf300" {
                l.f.db.close().await.unwrap();
                continue;
            }
            if name != "karate" {
                // 086: members (P) and component edges (X), by member sets.
                let rows = l
                    .det("gds.condensation.stream", &[], record(&[]), false)
                    .await;
                let members: BTreeMap<i64, Vec<i64>> = rows
                    .iter()
                    .map(|r| {
                        let Value::Int(c) = r[0] else { panic!() };
                        let mut m = l.uids(&r[1]);
                        m.sort_unstable();
                        (c, m)
                    })
                    .collect();
                let got: BTreeSet<(Vec<i64>, BTreeSet<i64>)> = rows
                    .iter()
                    .map(|r| {
                        let Value::Int(c) = r[0] else { panic!() };
                        let succ: BTreeSet<i64> = match &r[2] {
                            Value::List(x) => x
                                .iter()
                                .map(|s| match s {
                                    Value::Int(s) => members[s][0],
                                    _ => panic!(),
                                })
                                .collect(),
                            _ => panic!(),
                        };
                        (members[&c].clone(), succ)
                    })
                    .collect();
                let want: BTreeSet<(Vec<i64>, BTreeSet<i64>)> =
                    expected(&format!("{name}.nx.condensation.jsonl"))
                        .iter()
                        .map(|m| {
                            (
                                ints(&m["members"]),
                                ints(&m["successors"]).into_iter().collect(),
                            )
                        })
                        .collect();
                assert_eq!(got, want, "086 {name}");
            }
            if name != "cyc12" {
                // 089 (P on edges).
                let rows = l
                    .det("biconnected_components.get", &[], record(&[]), false)
                    .await;
                let got: BTreeMap<i64, Value> = rows
                    .iter()
                    .map(|r| {
                        let (a, b) = (l.uid(&r[1]), l.uid(&r[2]));
                        (a.min(b) * 100_000 + a.max(b), r[0].clone())
                    })
                    .collect();
                assert_eq!(got.len(), rows.len(), "each edge once");
                let want: BTreeMap<i64, Value> = expected(&format!("{name}.nx.bcc.jsonl"))
                    .iter()
                    .map(|m| {
                        let e = ints(&m["edge"]);
                        (e[0] * 100_000 + e[1], int(m["bcc"].as_i64().unwrap()))
                    })
                    .collect();
                compare(Rule::Partition, &got, &want).unwrap_or_else(|e| panic!("089 {name}: {e}"));
            }
            check_eccentricity(&l, name).await;
            check_girth_laplacian(&l, name).await;
            l.f.db.close().await.unwrap();
        }
        // 128 on Petersen, bip20 and dag40 (undirected), dag40 directed.
        for name in ["petersen", "bip20", "dag40"] {
            let l = Lab::load(name).await;
            let rows = l
                .det(
                    "gds.girth.stream",
                    &[],
                    record(&[("orientation", txt("UNDIRECTED"))]),
                    false,
                )
                .await;
            let want = num(&nx_values(&format!("{name}.nx.girth.jsonl"))[0]);
            assert_eq!(float(&rows[0][0]), want, "128 {name}");
            if name == "dag40" {
                let rows = l.once("gds.girth.stream", &[], record(&[])).await;
                assert_eq!(float(&rows[0][0]), f64::INFINITY, "a DAG has no cycle");
            }
            l.f.db.close().await.unwrap();
        }
    });
}

/// ALG-123 against NetworkX (X unweighted, F weighted).
async fn check_eccentricity(l: &Lab, name: &str) {
    let undirected = matches!(name, "karate" | "uniform60");
    let base = if undirected {
        record(&[("orientation", txt("UNDIRECTED"))])
    } else {
        record(&[])
    };
    for weighted in [false, true] {
        let config = if weighted {
            with(&base, "relationshipWeightProperty", txt("weight"))
        } else {
            base.clone()
        };
        let suffix = if weighted { ".weighted" } else { "" };
        let rows = l
            .det("gds.eccentricity.stream", &[], config.clone(), false)
            .await;
        let got = by_uid(l, &rows, 1);
        let want: BTreeMap<i64, Value> = expected(&format!("{name}.nx.ecc{suffix}.jsonl"))
            .iter()
            .map(|m| (m["node"].as_i64().unwrap(), Value::Float(num(&m["value"]))))
            .collect();
        let rule = if weighted {
            Rule::Float(1e-9)
        } else {
            Rule::Exact
        };
        compare(rule, &got, &want).unwrap_or_else(|e| panic!("123 {name}{suffix}: {e}"));
        if undirected {
            let row = write_row(
                l,
                "gds.eccentricity.write",
                with(&config, "writeProperty", txt("e")),
            )
            .await;
            let d = num(&nx_values(&format!("{name}.nx.diameter{suffix}.jsonl"))[0]);
            let r = num(&nx_values(&format!("{name}.nx.radius{suffix}.jsonl"))[0]);
            assert!(
                close(float(&row[1]), d, 1e-9) && close(float(&row[2]), r, 1e-9),
                "{row:?}"
            );
        }
    }
}

/// ALG-128 (directed, against the shortest of NetworkX's simple cycles)
/// and ALG-143 against NetworkX (F).
async fn check_girth_laplacian(l: &Lab, name: &str) {
    if name == "cyc12" {
        let rows = l.det("gds.girth.stream", &[], record(&[]), false).await;
        let shortest = nx_values("cyc12.nx.cycles.jsonl")
            .iter()
            .map(|c| ints(c).len())
            .min()
            .unwrap() as f64;
        assert_eq!(float(&rows[0][0]), shortest, "128 cyc12 directed");
        return;
    }
    if name == "karate" {
        let rows = l
            .det(
                "gds.girth.stream",
                &[],
                record(&[("orientation", txt("UNDIRECTED"))]),
                false,
            )
            .await;
        assert_eq!(
            float(&rows[0][0]),
            num(&nx_values("karate.nx.girth.jsonl")[0])
        );
    }
    if name == "uniform60" {
        return;
    }
    let base = if name == "karate" {
        record(&[("orientation", txt("UNDIRECTED"))])
    } else {
        record(&[])
    };
    let mut worst = 0.0f64;
    for (config, suffix) in [
        (base.clone(), ""),
        (
            with(&base, "relationshipWeightProperty", txt("weight")),
            ".weighted",
        ),
    ] {
        let rows = l
            .det("gds.laplacianMatrix.stream", &[], config, false)
            .await;
        // NetworkX's rows and columns are in uid order, lpg's in element
        // identifier order: row `i` and column `j` are the node of row `j`.
        let want = nx_values(&format!("{name}.nx.laplacian{suffix}.jsonl"));
        assert_eq!(rows.len(), want.len());
        let uid: Vec<usize> = rows.iter().map(|r| l.uid(&r[0]) as usize).collect();
        let mut ids: Vec<i64> = rows
            .iter()
            .map(|r| match r[0] {
                Value::Int(i) => i,
                _ => panic!(),
            })
            .collect();
        let sorted = ids.windows(2).all(|w| w[0] < w[1]);
        assert!(sorted, "rows in ascending element identifier");
        ids.clear();
        for (i, r) in rows.iter().enumerate() {
            let Value::List(xs) = &r[1] else { panic!() };
            let ws = want[uid[i]].as_array().unwrap();
            assert_eq!(xs.len(), ws.len());
            for (j, x) in xs.iter().enumerate() {
                let (a, b) = (float(x), num(&ws[uid[j]]));
                assert!(
                    close(a, b, 1e-9),
                    "143 {name}{suffix} [{i}, {j}]: {a} against {b}"
                );
                worst = worst.max((a - b).abs() / b.abs().max(1.0));
            }
        }
    }
    eprintln!("LAB 143 {name}: worst relative difference {worst:e}");
}

/// rmp #67: the parallel entries (083, 093, 120, 121) use more than one
/// and at most `concurrency` workers on a 2-block projection; the others
/// one.
#[test]
fn worker_counts_of_the_component_and_measure_families() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: f.db.engine_handle().core().location.display().to_string(),
            edges: BTreeMap::new(),
            f,
        };
        rows(
            &l.f.s,
            "INSERT (:Two {uid: 100})-[:T {weight: 1}]->(:Two {uid: 101})",
            Parameters::new(),
        )
        .await
        .unwrap();
        let base = record(&[("nodeLabels", txt("Two")), ("relationshipTypes", txt("T"))]);
        let procs = [
            "gds.wcc.stream",
            "gds.connectedComponents.stream",
            "gds.scc.stream",
            "gds.condensation.stream",
            "biconnected_components.get",
            "gds.articulationPoints.stream",
            "gds.bridges.stream",
            "gds.kcore.stream",
            "gds.triangleCount.stream",
            "gds.triangles",
            "gds.localClusteringCoefficient.stream",
            "gds.eccentricity.stream",
            "gds.girth.stream",
            "gds.laplacianMatrix.stream",
        ];
        for proc in procs {
            let e = super::ENTRIES.iter().find(|e| e.stream == proc).unwrap();
            for c in [1, 2, 16] {
                crate::algo::sched::probe::take(&l.key);
                l.call(proc, &[], &with(&base, "concurrency", int(c)))
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
        let parallel: Vec<u16> = super::ENTRIES
            .iter()
            .filter(|e| procs.contains(&e.stream) && e.parallel)
            .flat_map(|e| e.algs.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(parallel, vec![83, 93, 120, 121]);
        l.f.db.close().await.unwrap();
    });
}

/// rmp #67 (FR-LIM-001): ALG-143 on a projection whose dense matrix
/// exceeds the memory limit fails with 51N73; the accountant's peak never
/// exceeds the cap.
#[test]
fn laplacian_over_the_memory_limit_fails() {
    current(async {
        let l = Lab::load("zipf300").await;
        // 300² floats are 720 000 B.
        let ok = l.once("gds.laplacianMatrix.stream", &[], record(&[])).await;
        assert_eq!(ok.len(), 300);
        let mut limits = l.f.s.limits();
        limits.memory = Some(512 * 1024);
        l.f.s.set_limits(limits);
        let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
        let e = match tx
            .execute(
                "CALL gds.laplacianMatrix.stream($c)",
                param("c", record(&[])),
            )
            .await
        {
            Err(e) => e,
            Ok(st) => drain(st).await.expect_err("over the limit"),
        };
        let _ = tx.rollback().await;
        assert_eq!(e.code(), "51N73", "{e}");
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap());
        eprintln!(
            "LAB 143: a 300-node matrix fails under a 524288 B limit with {}; accountant peak {} B, cap {} B",
            e.code(),
            acc.peak(),
            acc.cap()
        );
        l.f.db.close().await.unwrap();
    });
}

/// GDS's k-core phases for core values `core` (`KCoreDecomposition.java`
/// `compute`, studied): from scanning degree 1, a phase that finds no node
/// at the scanning degree jumps to the smallest remaining degree, and one
/// that does removes the level's cascade and raises the degree by 1.
fn gds_kcore_phases(core: &[i64]) -> usize {
    let levels: BTreeSet<i64> = core.iter().copied().filter(|c| *c > 0).collect();
    let mut phases = 0;
    let mut scanning = 1;
    for c in levels {
        if c != scanning {
            phases += 1;
        }
        phases += 1;
        scanning = c + 1;
    }
    phases
}

/// Laboratory record (rmp #67): the rounds of ALG-083 and ALG-093 against
/// GDS's phases, and the accountant's peak of the family on zipf300; run
/// under `docs/evidence/lab-sprint-5/heaptrack.sh` for the heap peak.
#[test]
#[ignore = "laboratory measurement"]
fn lab_family67_memory_and_rounds() {
    current(async {
        for name in ["karate", "zipf60", "uniform60", "zipf300"] {
            let l = Lab::load(name).await;
            crate::algo::sched::probe::take_rounds(&l.key);
            l.once("gds.wcc.stream", &[], record(&[])).await;
            let rows = l.once("gds.kcore.stream", &[], record(&[])).await;
            let core: Vec<i64> = rows
                .iter()
                .map(|r| match r[1] {
                    Value::Int(c) => c,
                    _ => panic!(),
                })
                .collect();
            let rounds = crate::algo::sched::probe::take_rounds(&l.key);
            eprintln!(
                "LAB rounds {name}: lpg {rounds:?}; GDS k-core phases {}",
                gds_kcore_phases(&core)
            );
            if name == "zipf300" {
                for proc in [
                    "gds.wcc.stream",
                    "gds.connectedComponents.stream",
                    "gds.scc.stream",
                    "gds.condensation.stream",
                    "biconnected_components.get",
                    "gds.articulationPoints.stream",
                    "gds.bridges.stream",
                    "gds.kcore.stream",
                    "gds.triangleCount.stream",
                    "gds.triangles",
                    "gds.localClusteringCoefficient.stream",
                    "gds.eccentricity.stream",
                    "gds.girth.stream",
                    "gds.laplacianMatrix.stream",
                ] {
                    let n = l.once(proc, &[], record(&[])).await.len();
                    let engine = l.f.db.engine_handle();
                    let acc = engine.accountant();
                    eprintln!(
                        "LAB {proc}: {n} rows; accountant peak {} B (cap {} B)",
                        acc.peak(),
                        acc.cap()
                    );
                    assert!(acc.peak() <= acc.cap());
                }
            }
            l.f.db.close().await.unwrap();
        }
    });
}

/// rmp #67: ALG-121 with `triangleCountProperty` reads the counts that
/// ALG-120's write mode wrote in the same transaction, and equals the run
/// that counts them; `labelFilter` is refused.
#[test]
fn clustering_from_written_triangle_counts() {
    current(async {
        let l = Lab::load("karate").await;
        let direct = l
            .once("gds.localClusteringCoefficient.stream", &[], record(&[]))
            .await;
        let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
        drain(
            tx.execute(
                "CALL gds.triangleCount.write($c)",
                param("c", record(&[("writeProperty", txt("tc"))])),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        let seeded = drain(
            tx.execute(
                "CALL gds.localClusteringCoefficient.stream($c)",
                param("c", record(&[("triangleCountProperty", txt("tc"))])),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(seeded, direct);
        let e = l
            .call(
                "gds.triangleCount.stream",
                &[],
                &record(&[("labelFilter", txt("MrHi"))]),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        l.f.db.close().await.unwrap();
    });
}
