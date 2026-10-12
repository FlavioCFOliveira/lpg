//! Tests of the centrality, similarity and link-prediction families (rmp
//! #65): each entry against its oracle (`algorithm-entries.md`
//! Verification) on the shared fixtures, the seeded Zipf graph `zipf60`
//! and the uniform graph `uniform60`; bit-equal rows at `concurrency` 1
//! to 16 twice for each entry (two seeds for the randomised ones); `CALL`
//! equal to the Rust API; the worker-count rule; the memory cap; the
//! write modes; the quality measures of the parity entries 059 and 106.
//! Oracle outputs: `docs/evidence/lab-sprint-5/oracle-cases/*_65_66.sh`.

use std::collections::{BTreeMap, BTreeSet};

use rand::{RngExt, SeedableRng};

use super::tests_paths::{CAP, Lab, current, int, report, txt};
use crate::test_support::algo::{
    Fixture, Rule, compare, drain, expected, expected_by_key, param, record, rows, with,
};
use crate::types::Value;
use crate::{Parameters, TransactionOptions};

fn weight() -> (&'static str, Value) {
    ("relationshipWeightProperty", txt("weight"))
}

/// The configuration of graph `g` (NATURAL) or `u` (UNDIRECTED) of the
/// GDS cases with `pairs`.
fn cfg(undirected: bool, pairs: &[(&str, Value)]) -> Value {
    let mut all: Vec<(&str, Value)> = pairs.to_vec();
    if undirected {
        all.push(("orientation", txt("UNDIRECTED")));
    }
    record(&all)
}

/// Score rows by `uid`.
fn scores(l: &Lab, rows: &[Vec<Value>]) -> BTreeMap<i64, Value> {
    l.f.by_uid(rows)
}

/// Rule F against a GDS Score output; returns the largest difference.
fn check_scores(l: &Lab, rows: &[Vec<Value>], file: &str, t: f64) -> f64 {
    let want = expected_by_key(file, "nodeId", "score");
    compare(Rule::Float(t), &scores(l, rows), &want).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// `ranIterations` and `didConverge` of the write mode of `proc` in a
/// rolled-back transaction.
async fn rank_summary(l: &Lab, proc: &str, config: &Value, property: &str) -> (Value, Value) {
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    let write = proc.replace(".stream", ".write");
    let mut c = config.clone();
    if !property.is_empty() {
        c = with(&c, "writeProperty", txt(property));
    }
    let row = drain(
        tx.execute(&format!("CALL {write}($c)"), param("c", c))
            .await
            .unwrap_or_else(|e| panic!("{write}: {e}")),
    )
    .await
    .unwrap()
    .remove(0);
    tx.rollback().await.unwrap();
    let r = crate::proc::Registry::with_builtins();
    let cols = r.get(&write).unwrap().get().sig.columns.clone();
    let at = |c: &str| row[cols.iter().position(|x| x == c).unwrap()].clone();
    (at("ranIterations"), at("didConverge"))
}

/// The GDS `ranIterations` of a stats output.
fn gds_rounds(file: &str) -> (i64, bool) {
    let m = &expected(file)[0];
    (
        m["ranIterations"].as_i64().unwrap(),
        m["didConverge"].as_bool().unwrap(),
    )
}

/// The score entries of a fixture, checked against GDS (rule F); the
/// first case of each procedure also bit-equal at `concurrency` 1 to 16.
async fn score_family(l: &Lab, f: &str, undirected: bool) {
    let w = weight();
    let mut cases: Vec<(&str, Value, String)> = vec![(
        "gds.degree.stream",
        cfg(undirected, &[]),
        format!("{f}.gds.degree.jsonl"),
    )];
    if !undirected {
        // GDS's degree `orientation` on a NATURAL graph; on its UNDIRECTED
        // graph `UNDIRECTED` would count each edge twice more, which one
        // `orientation` key cannot express.
        cases.push((
            "gds.degree.stream",
            record(&[("orientation", txt("REVERSE")), w.clone()]),
            format!("{f}.gds.degree.reverse.weighted.jsonl"),
        ));
        cases.push((
            "gds.degree.stream",
            record(&[("orientation", txt("UNDIRECTED"))]),
            format!("{f}.gds.degree.undirected.jsonl"),
        ));
    }
    cases.extend(vec![
        (
            "gds.betweenness.stream",
            cfg(undirected, &[]),
            format!("{f}.gds.betweenness.jsonl"),
        ),
        (
            "gds.betweenness.stream",
            cfg(undirected, std::slice::from_ref(&w)),
            format!("{f}.gds.betweenness.weighted.jsonl"),
        ),
        (
            "gds.closeness.stream",
            cfg(undirected, &[]),
            format!("{f}.gds.closeness.jsonl"),
        ),
        (
            "gds.closeness.stream",
            cfg(undirected, &[("useWassermanFaust", Value::Bool(true))]),
            format!("{f}.gds.closeness.wf.jsonl"),
        ),
        (
            "gds.closeness.harmonic.stream",
            cfg(undirected, &[]),
            format!("{f}.gds.harmonic.jsonl"),
        ),
        (
            "gds.eigenvector.stream",
            cfg(undirected, &[]),
            format!("{f}.gds.eigenvector.jsonl"),
        ),
        (
            "gds.eigenvector.stream",
            cfg(undirected, std::slice::from_ref(&w)),
            format!("{f}.gds.eigenvector.weighted.jsonl"),
        ),
        (
            "gds.eigenvector.stream",
            cfg(
                undirected,
                &[
                    ("maxIterations", int(100)),
                    ("tolerance", Value::Float(1e-9)),
                ],
            ),
            format!("{f}.gds.eigenvector.converged.jsonl"),
        ),
        (
            "gds.articleRank.stream",
            cfg(undirected, &[]),
            format!("{f}.gds.articlerank.jsonl"),
        ),
        (
            "gds.articleRank.stream",
            cfg(undirected, std::slice::from_ref(&w)),
            format!("{f}.gds.articlerank.weighted.jsonl"),
        ),
        (
            "gds.articleRank.stream",
            cfg(
                undirected,
                &[
                    ("maxIterations", int(100)),
                    ("tolerance", Value::Float(1e-9)),
                ],
            ),
            format!("{f}.gds.articlerank.converged.jsonl"),
        ),
    ]);
    let mut seen = BTreeSet::new();
    for (proc, config, file) in cases {
        let rows = if seen.insert(proc) {
            l.det(proc, &[], config.clone(), true).await
        } else {
            l.once(proc, &[], config.clone()).await
        };
        report(&file, check_scores(l, &rows, &file, 1e-9));
    }
    // Rounds to convergence against GDS's `ranIterations`.
    for (proc, file) in [
        ("gds.eigenvector.stream", "eigenvector"),
        ("gds.articleRank.stream", "articlerank"),
    ] {
        let config = cfg(
            undirected,
            &[
                ("maxIterations", int(100)),
                ("tolerance", Value::Float(1e-9)),
            ],
        );
        let got = rank_summary(l, proc, &config, "x").await;
        let want = gds_rounds(&format!("{f}.gds.{file}.converged.stats.jsonl"));
        eprintln!(
            "LAB {f} {proc}: ranIterations {:?} converged {:?}; GDS {} {}",
            got.0, got.1, want.0, want.1
        );
        assert_eq!(got, (int(want.0), Value::Bool(want.1)), "{f} {proc} rounds");
    }
    // Default eigenvector rounds.
    let got = rank_summary(l, "gds.eigenvector.stream", &cfg(undirected, &[]), "x").await;
    let want = gds_rounds(&format!("{f}.gds.eigenvector.stats.jsonl"));
    assert_eq!(
        got,
        (int(want.0), Value::Bool(want.1)),
        "{f} eigenvector rounds"
    );
}

/// HITS rows against GDS's `{nodeId, auth, hub}`.
fn check_hits(l: &Lab, rows: &[Vec<Value>], file: &str) -> f64 {
    let mut worst = 0.0f64;
    for key in ["auth", "hub"] {
        let got: BTreeMap<i64, Value> = rows
            .iter()
            .map(|r| {
                let Value::Record(m) = &r[1] else { panic!() };
                let v = m.iter().find(|(k, _)| &**k == key).unwrap().1.clone();
                (l.uid(&r[0]), v)
            })
            .collect();
        let want = expected_by_key(file, "nodeId", key);
        worst = worst
            .max(compare(Rule::Float(1e-9), &got, &want).unwrap_or_else(|e| panic!("{file}: {e}")));
    }
    worst
}

/// Similarity rows `(node1, node2, similarity)` as uid triples.
fn triples(l: &Lab, rows: &[Vec<Value>]) -> Vec<(i64, i64, f64)> {
    rows.iter()
        .map(|r| {
            let Value::Float(x) = r[2] else {
                panic!("{r:?}")
            };
            (l.uid(&r[0]), l.uid(&r[1]), x)
        })
        .collect()
}

/// GDS similarity rows as uid triples.
fn gds_triples(file: &str) -> Vec<(i64, i64, f64)> {
    expected(file)
        .into_iter()
        .map(|m| {
            (
                m["node1"].as_i64().unwrap(),
                m["node2"].as_i64().unwrap(),
                m["similarity"].as_f64().unwrap(),
            )
        })
        .collect()
}

/// Rule F, and V where a cut goes through equal similarities: per source,
/// the similarities in order are equal under F, and each pair both give
/// has the same similarity.
fn check_similarities(got: &[(i64, i64, f64)], want: &[(i64, i64, f64)], what: &str) {
    let by_source = |xs: &[(i64, i64, f64)]| {
        let mut m: BTreeMap<i64, Vec<(i64, f64)>> = BTreeMap::new();
        for (a, b, x) in xs {
            m.entry(*a).or_default().push((*b, *x));
        }
        m
    };
    let (g, w) = (by_source(got), by_source(want));
    assert_eq!(
        g.keys().collect::<Vec<_>>(),
        w.keys().collect::<Vec<_>>(),
        "{what}: sources"
    );
    for (s, gl) in &g {
        let wl = &w[s];
        let mut gs: Vec<f64> = gl.iter().map(|x| x.1).collect();
        let mut ws: Vec<f64> = wl.iter().map(|x| x.1).collect();
        gs.sort_by(f64::total_cmp);
        ws.sort_by(f64::total_cmp);
        assert_eq!(
            gs.len(),
            ws.len(),
            "{what}: source {s}: {gl:?} against {wl:?}"
        );
        for (a, b) in gs.iter().zip(&ws) {
            assert!(
                super::tests_paths::close(*a, *b, 1e-9),
                "{what}: source {s}: {gl:?} against {wl:?}"
            );
        }
        let wm: BTreeMap<i64, f64> = wl.iter().copied().collect();
        for (t, x) in gl {
            if let Some(y) = wm.get(t) {
                assert!(
                    super::tests_paths::close(*x, *y, 1e-9),
                    "{what}: pair {s}-{t}"
                );
            }
        }
    }
}

/// The `vec` of each node of fixture `name`, by uid.
fn vectors(name: &str) -> Vec<Vec<f64>> {
    let text =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir(name).join("nodes.csv"))
            .unwrap();
    text.lines()
        .skip(1)
        .map(|l| {
            l.split(',')
                .nth(3)
                .unwrap()
                .split(';')
                .map(|x| x.parse().unwrap())
                .collect()
        })
        .collect()
}

/// KNN's cosine similarity, `(cos + 1) / 2`.
fn knn_cosine(a: &[f64], b: &[f64]) -> f64 {
    f64::midpoint(super::functions::cosine(a, b), 1.0)
}

/// KNN's Euclidean similarity, `1 / (1 + d)`.
fn knn_euclidean(a: &[f64], b: &[f64]) -> f64 {
    1.0 / (1.0 + super::functions::euclidean_distance(a, b))
}

/// KNN's Pearson similarity, `(r + 1) / 2`.
fn knn_pearson(a: &[f64], b: &[f64]) -> f64 {
    f64::midpoint(super::functions::pearson(a, b), 1.0)
}

/// The share of the exact top-`k` that `found` (source, target) pairs
/// hold; a found target as similar as the `k`-th exact one counts.
fn recall(
    vs: &[Vec<f64>],
    found: &[(i64, i64, f64)],
    k: usize,
    sources: &[i64],
    targets: &[i64],
) -> f64 {
    recall_by(knn_cosine, vs, found, k, sources, targets)
}

/// [`recall`] under the similarity `sim`.
fn recall_by(
    sim: fn(&[f64], &[f64]) -> f64,
    vs: &[Vec<f64>],
    found: &[(i64, i64, f64)],
    k: usize,
    sources: &[i64],
    targets: &[i64],
) -> f64 {
    let mut hits = 0usize;
    let mut total = 0usize;
    for s in sources {
        let mut exact: Vec<f64> = targets
            .iter()
            .filter(|t| *t != s)
            .map(|t| sim(&vs[*s as usize], &vs[*t as usize]))
            .collect();
        exact.sort_by(|a, b| b.total_cmp(a));
        exact.truncate(k);
        let kth = exact.last().copied().unwrap_or(f64::INFINITY);
        total += exact.len();
        hits += found
            .iter()
            .filter(|(a, b, _)| a == s && sim(&vs[*s as usize], &vs[*b as usize]) >= kth - 1e-12)
            .count()
            .min(exact.len());
    }
    hits as f64 / total as f64
}

/// The nodes of fixture `name` with label `label` (all for `None`), by uid.
fn labelled(name: &str, label: Option<&str>) -> Vec<i64> {
    let text =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir(name).join("nodes.csv"))
            .unwrap();
    text.lines()
        .skip(1)
        .filter(|l| label.is_none_or(|x| l.split(',').nth(1).unwrap().split(';').any(|y| y == x)))
        .map(|l| l.split(',').nth(2).unwrap().parse().unwrap())
        .collect()
}

/// rmp #65 on `zipf60` (the seeded Zipf graph): the centrality, similarity
/// and link-prediction entries against GDS and MAGE.
#[test]
fn centrality_family_matches_oracles_on_zipf60() {
    current(async {
        let l = Lab::load("zipf60").await;
        score_family(&l, "zipf60", false).await;
        for (proc, file) in [
            ("gds.betweenness.stream", "betweenness"),
            ("gds.closeness.stream", "closeness"),
            ("gds.closeness.harmonic.stream", "harmonic"),
            ("gds.articleRank.stream", "articlerank"),
        ] {
            let rows = l.once(proc, &[], cfg(true, &[])).await;
            let file = format!("zipf60.gds.{file}.undirected.jsonl");
            report(&file, check_scores(&l, &rows, &file, 1e-9));
        }
        // 043: personalised PageRank, and its rounds.
        let ppr = record(&[
            (
                "sourceNodes",
                Value::List(vec![l.node(1), l.node(8)].into()),
            ),
            ("maxIterations", int(100)),
            ("tolerance", Value::Float(1e-9)),
        ]);
        let rows = l.det("gds.pageRank.stream", &[], ppr.clone(), true).await;
        report(
            "zipf60 043",
            check_scores(&l, &rows, "zipf60.gds.ppr.jsonl", 1e-9),
        );
        let got = rank_summary(&l, "gds.pageRank.stream", &ppr, "x").await;
        let want = gds_rounds("zipf60.gds.ppr.stats.jsonl");
        eprintln!("LAB zipf60 043: ranIterations {:?}; GDS {}", got.0, want.0);
        assert_eq!(got, (int(want.0), Value::Bool(want.1)));
        // 045: HITS, its rounds, and its refusal of UNDIRECTED.
        let rows = l.det("gds.hits.stream", &[], record(&[]), true).await;
        report("zipf60 045", check_hits(&l, &rows, "zipf60.gds.hits.jsonl"));
        let five = record(&[("hitsIterations", int(5))]);
        let rows = l.once("gds.hits.stream", &[], five.clone()).await;
        report(
            "zipf60 045 (5)",
            check_hits(&l, &rows, "zipf60.gds.hits.5.jsonl"),
        );
        let got = rank_summary(&l, "gds.hits.stream", &five, "").await;
        let want = gds_rounds("zipf60.gds.hits.stats.jsonl");
        eprintln!("LAB zipf60 045: ranIterations {:?}; GDS {}", got.0, want.0);
        assert_eq!(got, (int(want.0), Value::Bool(want.1)));
        let e = l
            .call("gds.hits.stream", &[], &cfg(true, &[]))
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        // 105.
        let w = weight();
        for (config, file, det) in [
            (record(&[]), "nodesim", true),
            (
                record(&[
                    ("similarityMetric", txt("OVERLAP")),
                    ("topK", int(3)),
                    ("topN", int(20)),
                ]),
                "nodesim.overlap",
                false,
            ),
            (
                record(&[
                    ("similarityMetric", txt("COSINE")),
                    w.clone(),
                    ("bottomK", int(2)),
                ]),
                "nodesim.cosine.weighted",
                false,
            ),
            (
                record(&[w.clone(), ("topK", int(4))]),
                "nodesim.jaccard.weighted",
                false,
            ),
            (
                record(&[
                    ("degreeCutoff", int(3)),
                    ("similarityCutoff", Value::Float(0.2)),
                    ("useComponents", Value::Bool(true)),
                ]),
                "nodesim.cutoffs",
                false,
            ),
        ] {
            let rows = if det {
                l.det("gds.nodeSimilarity.stream", &[], config, true).await
            } else {
                l.once("gds.nodeSimilarity.stream", &[], config).await
            };
            let file = format!("zipf60.gds.{file}.jsonl");
            if file.contains("overlap") {
                // topN cuts through ties (rule V): equal similarities, and
                // each pair's similarity is its overlap.
                let got = triples(&l, &rows);
                let mut a: Vec<f64> = got.iter().map(|t| t.2).collect();
                let mut b: Vec<f64> = gds_triples(&file).iter().map(|t| t.2).collect();
                a.sort_by(f64::total_cmp);
                b.sort_by(f64::total_cmp);
                assert_eq!(a, b, "{file}");
                let out = |u: i64| -> BTreeSet<i64> {
                    l.edges
                        .values()
                        .filter(|e| e.src == u && e.dst != u)
                        .map(|e| e.dst)
                        .collect()
                };
                for (s, t, x) in got {
                    let (na, nb) = (out(s), out(t));
                    let o = na.intersection(&nb).count() as f64 / na.len().min(nb.len()) as f64;
                    assert_eq!(o, x, "{file}: pair {s}-{t}");
                }
                continue;
            }
            check_similarities(&triples(&l, &rows), &gds_triples(&file), &file);
        }
        let filtered = record(&[
            ("sourceNodeFilter", txt("A")),
            ("targetNodeFilter", txt("B")),
        ]);
        let rows = l
            .det("gds.nodeSimilarity.filtered.stream", &[], filtered, true)
            .await;
        check_similarities(
            &triples(&l, &rows),
            &gds_triples("zipf60.gds.nodesim.filtered.jsonl"),
            "105 filtered",
        );
        let e = l
            .call(
                "gds.nodeSimilarity.stream",
                &[],
                &record(&[("topK", int(2)), ("bottomK", int(2))]),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        // 106 (rule Q): recall against the exact top-K at least GDS's.
        knn_quality(&l, "zipf60").await;
        // 112 to 117 and 107 against the GDS functions.
        link_prediction(&l).await;
        similarity_functions(&l).await;
        // 059 (rule Q).
        celf_quality(&l, "zipf60").await;
        // 041 on the Zipf graph, with a small enough attenuation.
        katz(&l, "zipf60", 0.01, 0.0001).await;
        // 035 with sampling: bit-equal at every `concurrency`, and the
        // seed changes the sample.
        let sampled = |seed: i64| record(&[("samplingSize", int(20)), ("samplingSeed", int(seed))]);
        let a = l.det("gds.betweenness.stream", &[], sampled(3), true).await;
        let b = l.once("gds.betweenness.stream", &[], sampled(4)).await;
        assert_ne!(a, b, "035: two sampling seeds, one sample");
        l.f.db.close().await.unwrap();
    });
}

/// rmp #65 on `uniform60` (the uniform graph) and on the karate club.
#[test]
fn centrality_family_matches_oracles_on_uniform60_and_karate() {
    current(async {
        let l = Lab::load("uniform60").await;
        score_family(&l, "uniform60", false).await;
        let ppr = record(&[
            ("sourceNodes", Value::List(vec![l.node(14)].into())),
            ("maxIterations", int(100)),
            ("tolerance", Value::Float(1e-9)),
        ]);
        let rows = l.once("gds.pageRank.stream", &[], ppr.clone()).await;
        report(
            "uniform60 043",
            check_scores(&l, &rows, "uniform60.gds.ppr.jsonl", 1e-9),
        );
        let got = rank_summary(&l, "gds.pageRank.stream", &ppr, "x").await;
        let want = gds_rounds("uniform60.gds.ppr.stats.jsonl");
        eprintln!(
            "LAB uniform60 043: ranIterations {:?}; GDS {}",
            got.0, want.0
        );
        assert_eq!(got, (int(want.0), Value::Bool(want.1)));
        let rows = l.once("gds.hits.stream", &[], record(&[])).await;
        report(
            "uniform60 045",
            check_hits(&l, &rows, "uniform60.gds.hits.jsonl"),
        );
        let rows = l.once("gds.nodeSimilarity.stream", &[], record(&[])).await;
        check_similarities(
            &triples(&l, &rows),
            &gds_triples("uniform60.gds.nodesim.jsonl"),
            "uniform60 105",
        );
        knn_quality(&l, "uniform60").await;
        celf_quality(&l, "uniform60").await;
        link_prediction_uniform(&l).await;
        // 041 against MAGE (rule F with `epsilon`).
        katz(&l, "uniform60", 0.05, 0.01).await;
        l.f.db.close().await.unwrap();
        let k = Lab::load("karate").await;
        score_family(&k, "karate", true).await;
        let rows = k
            .once("gds.nodeSimilarity.stream", &[], cfg(true, &[]))
            .await;
        check_similarities(
            &triples(&k, &rows),
            &gds_triples("karate.gds.nodesim.jsonl"),
            "karate 105",
        );
        katz(&k, "karate", 0.2, 0.01).await;
        k.f.db.close().await.unwrap();
        let c = Lab::load("cyc12").await;
        katz(&c, "cyc12", 0.1, 0.001).await;
        c.f.db.close().await.unwrap();
    });
}

/// ALG-041 against MAGE's `katz_centrality.get` (rule F with `epsilon`).
async fn katz(l: &Lab, name: &str, alpha: f64, epsilon: f64) {
    let args = [Value::Float(alpha), Value::Float(epsilon)];
    let rows = l
        .det("katz_centrality.get", &args, record(&[]), false)
        .await;
    let got: BTreeMap<i64, Value> = rows.iter().map(|r| (l.uid(&r[0]), r[1].clone())).collect();
    let want = expected_by_key(&format!("{name}.mage.katz.jsonl"), "node", "rank");
    let worst =
        compare(Rule::Float(epsilon), &got, &want).unwrap_or_else(|e| panic!("041 {name}: {e}"));
    report(&format!("{name} 041"), worst);
}

/// ALG-106 rule Q on `name`: recall against the exact top-5 at least
/// GDS's, at two seeds, and the filtered form.
async fn knn_quality(l: &Lab, name: &str) {
    let vs = vectors(name);
    let all = labelled(name, None);
    let gds = gds_triples(&format!("{name}.gds.knn.jsonl"));
    let gds_recall = recall(&vs, &gds, 5, &all, &all);
    for seed in [42, 7] {
        let config = record(&[
            ("nodeProperties", txt("vec")),
            ("topK", int(5)),
            ("randomSeed", int(seed)),
        ]);
        let rows = l.det("gds.knn.stream", &[], config, true).await;
        let got = triples(l, &rows);
        let r = recall(&vs, &got, 5, &all, &all);
        eprintln!("LAB {name} 106 seed {seed}: recall {r:.4}; GDS {gds_recall:.4}");
        assert!(
            r >= gds_recall,
            "106 {name}: recall {r} below GDS's {gds_recall}"
        );
    }
    if name == "uniform60" {
        // The other metrics of a list property.
        for (metric, sim) in [
            ("EUCLIDEAN", knn_euclidean as fn(&[f64], &[f64]) -> f64),
            ("PEARSON", knn_pearson),
        ] {
            let file = format!("uniform60.gds.knn.{}.jsonl", metric.to_lowercase());
            let g = recall_by(sim, &vs, &gds_triples(&file), 5, &all, &all);
            let config = record(&[
                ("nodeProperties", record(&[("vec", txt(metric))])),
                ("topK", int(5)),
                ("randomSeed", int(42)),
            ]);
            let rows = l.once("gds.knn.stream", &[], config).await;
            let got = triples(l, &rows);
            for (s, t, x) in &got {
                let want = sim(&vs[*s as usize], &vs[*t as usize]);
                assert!(super::tests_paths::close(*x, want, 1e-12), "106 {metric}");
            }
            let r = recall_by(sim, &vs, &got, 5, &all, &all);
            eprintln!("LAB uniform60 106 {metric}: recall {r:.4}; GDS {g:.4}");
            assert!(r >= g, "106 {metric}");
        }
    }
    if name == "zipf60" {
        // The random-walk sampler and perturbations: deterministic, and
        // the recall recorded.
        let config = record(&[
            ("nodeProperties", txt("vec")),
            ("topK", int(5)),
            ("initialSampler", txt("RANDOMWALK")),
            ("perturbationRate", Value::Float(0.2)),
            ("randomJoins", int(0)),
        ]);
        let rows = l.det("gds.knn.stream", &[], config, true).await;
        let r = recall(&vs, &triples(l, &rows), 5, &all, &all);
        eprintln!("LAB zipf60 106 RANDOMWALK, perturbation 0.2, no random joins: recall {r:.4}");
        let (a, b) = (labelled(name, Some("A")), labelled(name, Some("B")));
        let config = record(&[
            ("nodeProperties", txt("vec")),
            ("topK", int(5)),
            ("randomSeed", int(42)),
            ("sourceNodeFilter", txt("A")),
            ("targetNodeFilter", txt("B")),
        ]);
        let rows = l.det("gds.knn.filtered.stream", &[], config, true).await;
        let got = triples(l, &rows);
        assert!(got.iter().all(|(s, t, _)| a.contains(s) && b.contains(t)));
        let gds = gds_triples("zipf60.gds.knn.filtered.jsonl");
        let (r, g) = (recall(&vs, &got, 5, &a, &b), recall(&vs, &gds, 5, &a, &b));
        eprintln!("LAB zipf60 106 filtered: recall {r:.4}; GDS {g:.4}");
        assert!(r >= g);
    }
}

/// ALG-112 to 117 against GDS's functions.
async fn link_prediction(l: &Lab) {
    let mut checked = 0;
    for d in ["BOTH", "OUTGOING", "INCOMING"] {
        for (a, b) in [(1, 8), (26, 1), (3, 40), (26, 26)] {
            for f in [
                "adamicAdar",
                "commonNeighbors",
                "preferentialAttachment",
                "resourceAllocation",
                "totalNeighbors",
            ] {
                let file = format!("zipf60.gds.lp.{f}.{d}.{a}-{b}.jsonl");
                let want = expected(&file)[0]["value"].as_f64().unwrap();
                let got = rows(
                    &l.f.s,
                    &format!("CALL gds.linkprediction.{f}($a, $b, $c)"),
                    Parameters::from([
                        ("a".to_string(), l.node(a)),
                        ("b".to_string(), l.node(b)),
                        ("c".to_string(), record(&[("direction", txt(d))])),
                    ]),
                )
                .await
                .unwrap();
                let Value::Float(x) = got[0][0] else { panic!() };
                assert!(
                    super::tests_paths::close(x, want, 1e-9),
                    "{file}: {x} instead of {want}"
                );
                checked += 1;
            }
        }
    }
    for f in [
        "adamicAdar",
        "commonNeighbors",
        "preferentialAttachment",
        "resourceAllocation",
        "totalNeighbors",
    ] {
        let want = expected(&format!("zipf60.gds.lp.{f}.R.1-8.jsonl"))[0]["value"]
            .as_f64()
            .unwrap();
        let got = rows(
            &l.f.s,
            &format!("CALL gds.linkprediction.{f}($a, $b, $c)"),
            Parameters::from([
                ("a".to_string(), l.node(1)),
                ("b".to_string(), l.node(8)),
                ("c".to_string(), record(&[("relationshipQuery", txt("R"))])),
            ]),
        )
        .await
        .unwrap();
        assert_eq!(got[0][0], Value::Float(want), "{f} R");
        checked += 1;
    }
    eprintln!("LAB 112-115, 117: {checked} calls equal to GDS's");
    // The Rust API reaches the same functions (FR-ALG-002).
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    for (name, ps) in [
        (
            "gds.linkprediction.adamicAdar",
            Parameters::from([
                ("node1".to_string(), l.node(1)),
                ("node2".to_string(), l.node(8)),
            ]),
        ),
        (
            "gds.similarity.cosine",
            Parameters::from([
                ("vector1".to_string(), list("[1, 2, 3]")),
                ("vector2".to_string(), list("[3, 2, 1]")),
            ]),
        ),
    ] {
        let by_api = drain(tx.run_algorithm(name, ps.clone()).await.unwrap())
            .await
            .unwrap();
        let args: Vec<String> = ps.keys().map(|k| format!("${k}")).collect();
        let by_call = drain(
            tx.execute(&format!("CALL {name}({})", args.join(", ")), ps)
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(by_api, by_call, "{name}");
    }
    tx.rollback().await.unwrap();
}

/// ALG-112 to 115 and 117 on `uniform60` against GDS's functions.
async fn link_prediction_uniform(l: &Lab) {
    for d in ["BOTH", "OUTGOING"] {
        for (a, b) in [(14, 15), (3, 21)] {
            for f in [
                "adamicAdar",
                "commonNeighbors",
                "preferentialAttachment",
                "resourceAllocation",
                "totalNeighbors",
            ] {
                let file = format!("uniform60.gds.lp.{f}.{d}.{a}-{b}.jsonl");
                let want = expected(&file)[0]["value"].as_f64().unwrap();
                let got = rows(
                    &l.f.s,
                    &format!("CALL gds.linkprediction.{f}($a, $b, $c)"),
                    Parameters::from([
                        ("a".to_string(), l.node(a)),
                        ("b".to_string(), l.node(b)),
                        ("c".to_string(), record(&[("direction", txt(d))])),
                    ]),
                )
                .await
                .unwrap();
                let Value::Float(x) = got[0][0] else { panic!() };
                assert!(
                    super::tests_paths::close(x, want, 1e-9),
                    "{file}: {x} instead of {want}"
                );
            }
        }
    }
}

/// A GQL list of the numbers of a Cypher list literal.
fn list(text: &str) -> Value {
    let inner = text.trim().trim_start_matches('[').trim_end_matches(']');
    Value::List(
        inner
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| {
                if x == "null" {
                    Value::Null
                } else if x.contains('.') {
                    Value::Float(x.parse().unwrap())
                } else {
                    Value::Int(x.parse().unwrap())
                }
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

/// ALG-107 against GDS's functions: equal values, and an error where GDS
/// fails.
async fn similarity_functions(l: &Lab) {
    let vectors = [
        ("[1, 2, 3]", "[3, 2, 1]"),
        ("[1.5, 0, -2]", "[0.5, 4, 1]"),
        ("[1, 1, 2, 3]", "[1, 2, 2, 5.0]"),
        ("[1, 2]", "[1.0, 2]"),
        ("[0, 0, 0]", "[1, 2, 3]"),
        ("[]", "[]"),
        ("[1, null, 3]", "[2, 2, null]"),
    ];
    let mut equal = 0;
    for (i, (a, b)) in vectors.iter().enumerate() {
        for f in [
            "cosine",
            "euclidean",
            "euclideanDistance",
            "jaccard",
            "overlap",
            "pearson",
        ] {
            let file = format!("zipf60.gds.simfn.{f}.{i}.jsonl");
            let path = crate::test_support::algo::expected_dir().join(&file);
            let got = rows(
                &l.f.s,
                &format!("CALL gds.similarity.{f}($a, $b)"),
                Parameters::from([("a".to_string(), list(a)), ("b".to_string(), list(b))]),
            )
            .await;
            if path.exists() {
                let want = &expected(&file)[0]["value"];
                let got = got.unwrap_or_else(|e| panic!("{file}: {e}"));
                match (&got[0][0], want) {
                    (Value::Float(x), serde_json::Value::Number(n)) => {
                        assert!(
                            super::tests_paths::close(*x, n.as_f64().unwrap(), 1e-9),
                            "{file}: {x}"
                        );
                    }
                    (Value::Float(x), serde_json::Value::String(s)) if s == "nan" => {
                        assert!(x.is_nan(), "{file}: {x}");
                    }
                    other => panic!("{file}: {other:?}"),
                }
            } else {
                assert!(got.is_err(), "{file}: GDS failed, lpg gave {got:?}");
            }
            equal += 1;
        }
    }
    eprintln!("LAB 107: {equal} calls equal to GDS's (values or errors)");
}

/// One cascade sample of the independent cascade model on the edges of
/// `l`: the live edges of cascade `j`.
fn live_edges(l: &Lab, p: f64, j: u64) -> BTreeMap<i64, Vec<i64>> {
    let mut g = rand::rngs::ChaCha8Rng::seed_from_u64(1_000_000 + j);
    let mut out: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for e in l.edges.values() {
        if g.random::<f64>() < p {
            out.entry(e.src).or_default().push(e.dst);
        }
    }
    out
}

/// The nodes reachable from `set` over `adj`, the set included.
fn reach(adj: &BTreeMap<i64, Vec<i64>>, set: &[i64]) -> usize {
    let mut seen: BTreeSet<i64> = set.iter().copied().collect();
    let mut q: Vec<i64> = set.to_vec();
    while let Some(u) = q.pop() {
        for w in adj.get(&u).into_iter().flatten() {
            if seen.insert(*w) {
                q.push(*w);
            }
        }
    }
    seen.len()
}

/// ALG-059 rule Q on `name`: with `propagationProbability` 1 the seeds
/// and gains are the greedy reachability cover; at the default, the
/// spread of lpg's seeds is at least GDS's minus three standard errors,
/// both estimated with 10 000 cascades.
async fn celf_quality(l: &Lab, name: &str) {
    for seed in [42, 7] {
        let config = record(&[("seedSetSize", int(5)), ("randomSeed", int(seed))]);
        l.det("gds.influenceMaximization.celf.stream", &[], config, true)
            .await;
    }
    if name == "zipf60" {
        let config = record(&[
            ("seedSetSize", int(4)),
            ("propagationProbability", Value::Float(1.0)),
        ]);
        let rows = l
            .once("gds.influenceMaximization.celf.stream", &[], config)
            .await;
        let all = live_edges(l, 1.0, 0);
        // The greedy cover, ties to the lowest element identifier.
        let mut uids = labelled(name, None);
        uids.sort_by_key(|u| l.f.ids[*u as usize]);
        let mut set: Vec<i64> = Vec::new();
        let mut want = Vec::new();
        let mut total = 0usize;
        for _ in 0..4 {
            let (best, gain) = uids
                .iter()
                .filter(|u| !set.contains(u))
                .map(|u| {
                    let mut s = set.clone();
                    s.push(*u);
                    (*u, reach(&all, &s) - total)
                })
                .fold((-1, 0), |acc, x| if x.1 > acc.1 { x } else { acc });
            set.push(best);
            total += gain;
            want.push((best, gain as f64));
        }
        let got: Vec<(i64, f64)> = rows
            .iter()
            .map(|r| {
                let Value::Float(x) = r[1] else { panic!() };
                (l.uid(&r[0]), x)
            })
            .collect();
        assert_eq!(got, want, "059 with probability 1");
        let gds: f64 = expected("zipf60.gds.celf.p1.jsonl")
            .iter()
            .map(|m| m["spread"].as_f64().unwrap())
            .sum();
        eprintln!("LAB zipf60 059 p=1: cover {total}; GDS total spread {gds}");
    }
    let config = record(&[("seedSetSize", int(5)), ("randomSeed", int(42))]);
    let rows = l
        .once("gds.influenceMaximization.celf.stream", &[], config)
        .await;
    let ours: Vec<i64> = rows.iter().map(|r| l.uid(&r[0])).collect();
    let theirs: Vec<i64> = expected(&format!("{name}.gds.celf.jsonl"))
        .iter()
        .map(|m| m["nodeId"].as_i64().unwrap())
        .collect();
    let n = 10_000u64;
    let (mut a, mut b, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for j in 0..n {
        let adj = live_edges(l, 0.1, j);
        let x = reach(&adj, &ours) as f64;
        let y = reach(&adj, &theirs) as f64;
        a += x;
        b += y;
        bb += y * y;
    }
    let (ma, mb) = (a / n as f64, b / n as f64);
    let se = ((bb / n as f64 - mb * mb) / n as f64).sqrt();
    eprintln!(
        "LAB {name} 059: lpg seeds {ours:?} spread {ma:.4}; GDS seeds {theirs:?} spread {mb:.4} (SE {se:.4})"
    );
    assert!(
        ma >= mb - 3.0 * se,
        "059 {name}: {ma} below {mb} - 3 x {se}"
    );
}

/// rmp #65 on `zipf300`, whose blocks hold five nodes: the parallel
/// forms with more than one source, target or node per block against GDS.
#[test]
fn centrality_family_matches_gds_with_multi_node_blocks() {
    current(async {
        let l = Lab::load("zipf300").await;
        let w = weight();
        for (proc, config, file, det) in [
            (
                "gds.degree.stream",
                record(std::slice::from_ref(&w)),
                "degree.weighted",
                false,
            ),
            ("gds.betweenness.stream", record(&[]), "betweenness", true),
            (
                "gds.betweenness.stream",
                record(std::slice::from_ref(&w)),
                "betweenness.weighted",
                false,
            ),
            (
                "gds.betweenness.stream",
                cfg(true, &[]),
                "betweenness.undirected",
                false,
            ),
            ("gds.closeness.stream", record(&[]), "closeness", true),
            (
                "gds.closeness.harmonic.stream",
                record(&[]),
                "harmonic",
                false,
            ),
            ("gds.eigenvector.stream", record(&[]), "eigenvector", true),
            (
                "gds.eigenvector.stream",
                record(&[
                    (
                        "sourceNodes",
                        Value::List(vec![l.node(0), l.node(7), l.node(42)].into()),
                    ),
                    w.clone(),
                ]),
                "eigenvector.sources",
                false,
            ),
            ("gds.articleRank.stream", record(&[]), "articlerank", false),
        ] {
            let rows = if det {
                l.det(proc, &[], config, true).await
            } else {
                l.once(proc, &[], config).await
            };
            let file = format!("zipf300.gds.{file}.jsonl");
            report(&file, check_scores(&l, &rows, &file, 1e-9));
        }
        let rows = l.once("gds.hits.stream", &[], record(&[])).await;
        report(
            "zipf300 045",
            check_hits(&l, &rows, "zipf300.gds.hits.jsonl"),
        );
        for (config, file) in [
            (record(&[]), "nodesim"),
            (
                record(&[("similarityMetric", txt("COSINE")), ("topK", int(3))]),
                "nodesim.cosine",
            ),
            (
                record(&[
                    ("similarityMetric", txt("OVERLAP")),
                    w.clone(),
                    ("topK", int(3)),
                ]),
                "nodesim.overlap.weighted",
            ),
        ] {
            let rows = l.det("gds.nodeSimilarity.stream", &[], config, true).await;
            let file = format!("zipf300.gds.{file}.jsonl");
            check_similarities(&triples(&l, &rows), &gds_triples(&file), &file);
        }
        l.f.db.close().await.unwrap();
    });
}

/// rmp #65: each parallel entry starts two workers on a 2-block
/// projection at `concurrency` 4; each other entry one at 16.
#[test]
fn worker_counts_of_the_centrality_families() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: f.db.engine_handle().core().location.display().to_string(),
            edges: BTreeMap::new(),
            f,
        };
        rows(
            &l.f.s,
            "INSERT (:Two {uid: 100, vec: $a})-[:T {weight: 1}]->(:Two {uid: 101, vec: $b})",
            Parameters::from([
                (
                    "a".to_string(),
                    Value::List(vec![Value::Float(1.0), Value::Float(2.0)].into()),
                ),
                (
                    "b".to_string(),
                    Value::List(vec![Value::Float(2.0), Value::Float(1.0)].into()),
                ),
            ]),
        )
        .await
        .unwrap();
        let base = record(&[("nodeLabels", txt("Two")), ("relationshipTypes", txt("T"))]);
        let cases: Vec<(&str, Vec<Value>, Value)> = vec![
            ("gds.degree.stream", vec![], record(&[])),
            ("gds.betweenness.stream", vec![], record(&[])),
            ("gds.closeness.stream", vec![], record(&[])),
            ("gds.closeness.harmonic.stream", vec![], record(&[])),
            ("gds.eigenvector.stream", vec![], record(&[])),
            ("gds.articleRank.stream", vec![], record(&[])),
            ("gds.hits.stream", vec![], record(&[])),
            (
                "gds.influenceMaximization.celf.stream",
                vec![],
                record(&[("seedSetSize", int(1))]),
            ),
            ("gds.nodeSimilarity.stream", vec![], record(&[])),
            (
                "gds.knn.stream",
                vec![],
                record(&[("nodeProperties", txt("vec")), ("topK", int(1))]),
            ),
            (
                "katz_centrality.get",
                vec![Value::Float(0.2), Value::Float(0.01)],
                record(&[]),
            ),
        ];
        for (proc, args, config) in cases {
            let e = super::ENTRIES.iter().find(|e| e.stream == proc).unwrap();
            let mut config = config;
            if let Value::Record(b) = &base {
                for (k, v) in b.iter() {
                    config = with(&config, k, v.clone());
                }
            }
            let c = if e.parallel { 4 } else { 16 };
            crate::algo::sched::probe::take(&l.key);
            l.call(proc, &args, &with(&config, "concurrency", int(c)))
                .await
                .unwrap_or_else(|e| panic!("{proc}: {e}"));
            let w = crate::algo::sched::probe::take(&l.key);
            if e.parallel {
                assert_eq!(w, Some(2), "{proc}: {w:?} workers on 2 blocks");
            } else {
                assert!(w.is_none() || w == Some(1), "{proc}: {w:?} workers");
            }
            eprintln!("LAB workers {proc}: {w:?}");
        }
        l.f.db.close().await.unwrap();
    });
}

/// rmp #65 (FR-LIM-001): a run under a transaction memory limit smaller
/// than its projection fails with 51N73; the write modes write what the
/// rows describe; the accountant's peak never exceeds the cap.
#[test]
fn centrality_memory_cap_and_write_modes() {
    current(async {
        let l = Lab::load("zipf60").await;
        for proc in ["gds.betweenness.stream", "gds.nodeSimilarity.stream"] {
            let mut limits = l.f.s.limits();
            limits.memory = Some(4096);
            l.f.s.set_limits(limits);
            let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
            let e = match tx
                .execute(&format!("CALL {proc}($c)"), param("c", record(&[])))
                .await
            {
                Err(e) => e,
                Ok(st) => drain(st).await.expect_err("over the limit"),
            };
            let _ = tx.rollback().await;
            limits.memory = None;
            l.f.s.set_limits(limits);
            assert_eq!(e.code(), "51N73", "{proc}: {e}");
        }
        let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
        let count = |q: &str| {
            let q = q.to_string();
            let tx = &tx;
            async move {
                let r = drain(tx.execute(&q, Parameters::new()).await.unwrap())
                    .await
                    .unwrap();
                r[0][0].clone()
            }
        };
        let row = drain(
            tx.execute(
                "CALL gds.degree.write($c)",
                param("c", record(&[("writeProperty", txt("deg"))])),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap()
        .remove(0);
        assert_eq!(row[0], int(60), "nodePropertiesWritten");
        assert_eq!(
            count("MATCH (n) WHERE n.deg IS NOT NULL RETURN count(*) AS c").await,
            int(60)
        );
        let row = drain(
            tx.execute("CALL gds.hits.write($c)", param("c", record(&[])))
                .await
                .unwrap(),
        )
        .await
        .unwrap()
        .remove(0);
        assert_eq!(row[0], int(80), "ranIterations");
        assert_eq!(
            count("MATCH (n) WHERE n.auth IS NOT NULL AND n.hub IS NOT NULL RETURN count(*) AS c")
                .await,
            int(60)
        );
        let streamed = drain(
            tx.execute(
                "CALL gds.nodeSimilarity.stream($c)",
                param("c", record(&[("topK", int(1))])),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap()
        .len() as i64;
        let row = drain(
            tx.execute(
                "CALL gds.nodeSimilarity.write($c)",
                param(
                    "c",
                    record(&[
                        ("topK", int(1)),
                        ("writeRelationshipType", txt("SIM")),
                        ("writeProperty", txt("score")),
                    ]),
                ),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap()
        .remove(0);
        assert_eq!(row[5], int(streamed), "relationshipsWritten");
        assert_eq!(
            count("MATCH ()-[e:SIM]->() WHERE e.score > 0 RETURN count(*) AS c").await,
            int(streamed)
        );
        tx.rollback().await.unwrap();
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap());
        l.f.db.close().await.unwrap();
    });
}

/// Laboratory record (rmp #65): the accountant's peak of the families on
/// `zipf300`; run under `docs/evidence/lab-sprint-5/heaptrack.sh` for the
/// heap peak.
#[test]
#[ignore = "laboratory measurement"]
fn lab_family65_memory() {
    current(async {
        let z = Lab::load("zipf300").await;
        let w = weight();
        for (proc, args, config) in [
            ("gds.degree.stream", vec![], record(&[])),
            (
                "gds.betweenness.stream",
                vec![],
                record(std::slice::from_ref(&w)),
            ),
            ("gds.closeness.stream", vec![], record(&[])),
            ("gds.closeness.harmonic.stream", vec![], record(&[])),
            ("gds.eigenvector.stream", vec![], record(&[])),
            ("gds.articleRank.stream", vec![], record(&[])),
            ("gds.hits.stream", vec![], record(&[])),
            (
                "gds.influenceMaximization.celf.stream",
                vec![],
                record(&[("seedSetSize", int(5))]),
            ),
            ("gds.nodeSimilarity.stream", vec![], record(&[])),
            (
                "gds.knn.stream",
                vec![],
                record(&[("nodeProperties", txt("vec"))]),
            ),
            (
                "katz_centrality.get",
                vec![Value::Float(0.01), Value::Float(0.01)],
                record(&[]),
            ),
        ] {
            let n = z.once(proc, &args, config).await.len();
            let engine = z.f.db.engine_handle();
            let acc = engine.accountant();
            eprintln!(
                "LAB {proc}: {n} rows; accountant peak {} B (cap {} B)",
                acc.peak(),
                acc.cap()
            );
            assert!(acc.peak() <= acc.cap());
        }
        z.f.db.close().await.unwrap();
    });
}

/// rmp #65: the options of the similarity entries: `bottomN`, a node
/// list as a filter (the same rows as the label), the degree cutoffs'
/// order, KNN's scalar and set metrics, and their argument errors.
#[test]
fn similarity_options() {
    current(async {
        let l = Lab::load("zipf60").await;
        let rows = l
            .det(
                "gds.nodeSimilarity.stream",
                &[],
                record(&[("bottomN", int(5))]),
                true,
            )
            .await;
        let sims: Vec<f64> = triples(&l, &rows).iter().map(|t| t.2).collect();
        assert_eq!(sims.len(), 5);
        assert!(sims.windows(2).all(|w| w[0] <= w[1]), "{sims:?}");
        let nodes = |label: &str| {
            Value::List(
                labelled("zipf60", Some(label))
                    .into_iter()
                    .map(|u| l.node(u))
                    .collect::<Vec<_>>()
                    .into(),
            )
        };
        let by_label = l
            .once(
                "gds.nodeSimilarity.filtered.stream",
                &[],
                record(&[
                    ("sourceNodeFilter", txt("A")),
                    ("targetNodeFilter", txt("B")),
                ]),
            )
            .await;
        let by_nodes = l
            .once(
                "gds.nodeSimilarity.filtered.stream",
                &[],
                record(&[
                    ("sourceNodeFilter", nodes("A")),
                    ("targetNodeFilter", nodes("B")),
                ]),
            )
            .await;
        assert_eq!(by_label, by_nodes);
        for config in [
            record(&[("degreeCutoff", int(3)), ("upperDegreeCutoff", int(2))]),
            record(&[("topN", int(2)), ("bottomN", int(2))]),
        ] {
            let e = l
                .call("gds.nodeSimilarity.stream", &[], &config)
                .await
                .unwrap_err();
            assert_eq!(e.code(), "22N11", "{config:?}");
        }
        for metric in ["JACCARD", "OVERLAP"] {
            let rows = l
                .det(
                    "gds.knn.stream",
                    &[],
                    record(&[
                        ("nodeProperties", record(&[("vec", txt(metric))])),
                        ("topK", int(2)),
                    ]),
                    true,
                )
                .await;
            assert_eq!(rows.len(), 120, "{metric}");
        }
        let e = l
            .call(
                "gds.knn.stream",
                &[],
                &record(&[("nodeProperties", txt("missing"))]),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        l.f.db.close().await.unwrap();
        // A scalar property: `1 / (1 + |a − b|)`.
        let p = Lab::load("planted60").await;
        let community = |u: i64| (u / 15) as f64;
        let rows = p
            .det(
                "gds.knn.stream",
                &[],
                record(&[("nodeProperties", txt("community")), ("topK", int(3))]),
                true,
            )
            .await;
        for (a, b, x) in triples(&p, &rows) {
            assert_eq!(x, 1.0 / (1.0 + (community(a) - community(b)).abs()));
            assert_eq!(x, 1.0, "the 3 nearest share the community");
        }
        p.f.db.close().await.unwrap();
    });
}
