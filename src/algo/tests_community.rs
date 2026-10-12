//! Tests of the community-detection and clustering family (rmp #66): the
//! parity entries against their quality measures (`algorithm-entries.md`
//! Verification, rule Q) on the ring of cliques `rc24`, the karate club,
//! the planted communities `planted60` and the planted point clusters
//! `blobs40`; ALG-082 against GDS and NetworkX; bit-equal rows at
//! `concurrency` 1 to 16 twice (two seeds for the randomised entries);
//! `CALL` equal to the Rust API; the worker-count rule; the memory cap;
//! the write modes.

use std::collections::{BTreeMap, BTreeSet};

use super::tests_paths::{CAP, Lab, current, int, report, txt};
use crate::test_support::algo::{
    Fixture, Rule, compare, drain, expected, expected_by_key, param, record, rows, with,
};
use crate::types::Value;
use crate::{Parameters, TransactionOptions};

fn undirected(pairs: &[(&str, Value)]) -> Value {
    let mut all = pairs.to_vec();
    all.push(("orientation", txt("UNDIRECTED")));
    record(&all)
}

/// The integer column `column` of fixture `name`'s nodes, by uid.
fn node_column(name: &str, column: &str) -> BTreeMap<i64, Value> {
    let text =
        std::fs::read_to_string(crate::test_support::algo::fixture_dir(name).join("nodes.csv"))
            .unwrap();
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap().split(',').collect();
    let at = head
        .iter()
        .position(|h| h.split(':').next() == Some(column))
        .unwrap();
    lines
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (f[2].parse().unwrap(), Value::Int(f[at].parse().unwrap()))
        })
        .collect()
}

/// `(uid, column)` of rows whose node is in column 0 and value in `col`.
fn by_uid(l: &Lab, rows: &[Vec<Value>], col: usize) -> BTreeMap<i64, Value> {
    rows.iter()
        .map(|r| (l.uid(&r[0]), r[col].clone()))
        .collect()
}

/// Modularity of `part` on the fixture's edges read as an undirected
/// unweighted multigraph (NetworkX's formula).
fn modularity(l: &Lab, part: &BTreeMap<i64, Value>) -> f64 {
    let m = l.edges.len() as f64;
    let mut inside: BTreeMap<String, f64> = BTreeMap::new();
    let mut degree: BTreeMap<String, f64> = BTreeMap::new();
    for e in l.edges.values() {
        let (a, b) = (format!("{:?}", part[&e.src]), format!("{:?}", part[&e.dst]));
        *degree.entry(a.clone()).or_default() += 1.0;
        *degree.entry(b.clone()).or_default() += 1.0;
        if a == b {
            *inside.entry(a).or_default() += 1.0;
        }
    }
    degree
        .iter()
        .map(|(c, d)| inside.get(c).copied().unwrap_or(0.0) / m - (d / (2.0 * m)).powi(2))
        .sum()
}

fn communities(part: &BTreeMap<i64, Value>) -> usize {
    part.values()
        .map(|v| format!("{v:?}"))
        .collect::<BTreeSet<_>>()
        .len()
}

/// A summary column of the write mode of `proc` (rolled back).
async fn summary(l: &Lab, proc: &str, args: &[Value], config: &Value, column: &str) -> Value {
    let write = proc.replace(".stream", ".write");
    let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
    let (text, ps) =
        super::tests_paths::call_text(&write, args, &with(config, "writeProperty", txt("c")));
    let row = drain(
        tx.execute(&text, ps)
            .await
            .unwrap_or_else(|e| panic!("{write}: {e}")),
    )
    .await
    .unwrap()
    .remove(0);
    tx.rollback().await.unwrap();
    let r = crate::proc::Registry::with_builtins();
    let cols = r.get(&write).unwrap().get().sig.columns.clone();
    row[cols.iter().position(|c| c == column).unwrap()].clone()
}

/// The partition of a GDS `(nodeId, communityId)` output.
fn gds_partition(file: &str) -> BTreeMap<i64, Value> {
    expected_by_key(file, "nodeId", "communityId")
}

/// The modularity-based entries (061, 062, 063, 065) on fixture `name`:
/// determinism (two seeds for 062), the partition, and its modularity
/// against GDS's; on `rc24` the partition must be the planted one.
async fn modularity_entries(l: &Lab, name: &str) {
    let planted = (name == "rc24").then(|| node_column(name, "community"));
    for (proc, gds, seeded) in [
        ("gds.louvain.stream", "louvain", false),
        ("gds.leiden.stream", "leiden", true),
        ("gds.labelPropagation.stream", "lpa", false),
        ("gds.modularityOptimization.stream", "mo", false),
    ] {
        let seeds: &[i64] = if seeded { &[42, 7] } else { &[0] };
        for seed in seeds {
            let config = if seeded {
                undirected(&[("randomSeed", int(*seed))])
            } else {
                undirected(&[])
            };
            let rows = l.det(proc, &[], config.clone(), true).await;
            let col = rows[0].len() - 1;
            let part = by_uid(l, &rows, col);
            let theirs = gds_partition(&format!("{name}.gds.{gds}.jsonl"));
            let (q, qg) = (modularity(l, &part), modularity(l, &theirs));
            eprintln!(
                "LAB {name} {proc} seed {seed}: modularity {q:.6} ({} communities); GDS {qg:.6} ({} communities)",
                communities(&part),
                communities(&theirs)
            );
            if let Some(p) = &planted {
                compare(Rule::Partition, &part, p)
                    .unwrap_or_else(|e| panic!("{name} {proc}: not the planted partition: {e}"));
            }
            // Rounds against the reference.
            if *seed == seeds[0] {
                let column = match gds {
                    "louvain" | "leiden" => "ranLevels",
                    _ => "ranIterations",
                };
                let got = summary(l, proc, &[], &config, column).await;
                let want = &expected(&format!("{name}.gds.{gds}.stats.jsonl"))[0][column];
                eprintln!("LAB {name} {proc}: {column} {got:?}; GDS {want}");
            }
        }
    }
    // 064: the strongest label of each node.
    for seed in [42, 7] {
        let config = undirected(&[
            ("maxIterations", int(20)),
            ("randomSeed", int(seed)),
            ("minAssociationStrength", Value::Float(0.0)),
        ]);
        let rows = l.det("gds.sllpa.stream", &[], config, true).await;
        let strongest: BTreeMap<i64, Value> = rows
            .iter()
            .map(|r| {
                let Value::Record(m) = &r[1] else { panic!() };
                let Value::List(ids) = &m[0].1 else { panic!() };
                (l.uid(&r[0]), ids.first().cloned().unwrap_or(Value::Null))
            })
            .collect();
        let gds: Vec<usize> = expected(&format!("{name}.gds.sllpa.jsonl"))
            .iter()
            .map(|m| m["communityIds"].as_array().unwrap().len())
            .collect();
        eprintln!(
            "LAB {name} 064 seed {seed}: {} strongest labels; GDS (default strength) {} nodes, {} labels in all",
            communities(&strongest),
            gds.len(),
            gds.iter().sum::<usize>()
        );
        if let Some(p) = &planted {
            compare(Rule::Partition, &strongest, p)
                .unwrap_or_else(|e| panic!("{name} 064: not the planted cliques: {e}"));
        }
    }
}

/// The cut weight of `part` over the fixture's edges (unweighted).
fn cut(l: &Lab, part: &BTreeMap<i64, Value>) -> usize {
    l.edges
        .values()
        .filter(|e| part[&e.src] != part[&e.dst])
        .count()
}

/// ALG-078 rule Q: the cut at least GDS's, at two seeds.
async fn max_k_cut(l: &Lab, name: &str, k: i64, file: &str) -> usize {
    let gds = cut(l, &gds_partition(file));
    let mut best = 0;
    for seed in [42, 7] {
        let rows = l
            .det(
                "gds.maxkcut.stream",
                &[],
                record(&[("k", int(k)), ("randomSeed", int(seed))]),
                true,
            )
            .await;
        let part = by_uid(l, &rows, 1);
        let c = cut(l, &part);
        eprintln!("LAB {name} 078 k {k} seed {seed}: cut {c}; GDS {gds}");
        assert!(c >= gds, "078 {name}: cut {c} below GDS's {gds}");
        best = best.max(c);
    }
    best
}

/// rmp #66 on the ring of cliques: every parity entry gives the planted
/// partition; 082 equals GDS and NetworkX.
#[test]
fn community_family_quality_on_the_ring_of_cliques() {
    current(async {
        let l = Lab::load("rc24").await;
        modularity_entries(&l, "rc24").await;
        let planted = node_column("rc24", "community");
        // 076 and 077 (rule Q): the planted partition with 4 clusters.
        for (proc, what) in [
            ("cugraph.spectral_clustering.get", "076"),
            ("cugraph.balanced_cut_clustering.get", "077"),
        ] {
            for seed in [42, 7] {
                let rows = l
                    .det(proc, &[int(4)], record(&[("randomSeed", int(seed))]), false)
                    .await;
                let part = by_uid(&l, &rows, 1);
                compare(Rule::Partition, &part, &planted).unwrap_or_else(|e| panic!("{what}: {e}"));
                let ratio: f64 = {
                    let mut sizes: BTreeMap<String, f64> = BTreeMap::new();
                    for v in part.values() {
                        *sizes.entry(format!("{v:?}")).or_default() += 1.0;
                    }
                    let mut x = 0.0;
                    for (c, s) in &sizes {
                        let out = l
                            .edges
                            .values()
                            .filter(|e| {
                                (format!("{:?}", part[&e.src]) == *c)
                                    != (format!("{:?}", part[&e.dst]) == *c)
                            })
                            .count() as f64;
                        x += out / s;
                    }
                    x
                };
                eprintln!(
                    "LAB rc24 {what} seed {seed}: planted partition; modularity {:.6}; ratio cut {ratio:.6}",
                    modularity(&l, &part)
                );
            }
        }
        // 078: two communities.
        max_k_cut(&l, "rc24", 2, "rc24.gds.maxkcut.jsonl").await;
        // 082 against GDS (rule F) and NetworkX's modularity.
        for (orientation, suffix) in [("NATURAL", ""), ("UNDIRECTED", ".undirected")] {
            let config = record(&[
                ("communityProperty", txt("community")),
                ("orientation", txt(orientation)),
            ]);
            let rows = l
                .det("gds.modularity.stream", &[], config.clone(), true)
                .await;
            let got: BTreeMap<i64, Value> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Int(c) => (*c, r[1].clone()),
                    _ => panic!(),
                })
                .collect();
            let want = expected_by_key(
                &format!("rc24.gds.modularity{suffix}.jsonl"),
                "communityId",
                "modularity",
            );
            report(
                "rc24 082 modularity",
                compare(Rule::Float(1e-9), &got, &want).unwrap(),
            );
            if suffix == ".undirected" {
                let sum: f64 = got
                    .values()
                    .map(|v| match v {
                        Value::Float(x) => *x,
                        _ => 0.0,
                    })
                    .sum();
                let nx = expected("rc24.nx.modularity.jsonl")[0]["value"]
                    .as_f64()
                    .unwrap();
                assert!(
                    super::tests_paths::close(sum, nx, 1e-9),
                    "082: {sum} against NetworkX {nx}"
                );
            }
            let rows = l.det("gds.conductance.stream", &[], config, true).await;
            let got: BTreeMap<i64, Value> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Int(c) => (*c, r[1].clone()),
                    _ => panic!(),
                })
                .collect();
            let want = expected_by_key(
                &format!("rc24.gds.conductance{suffix}.jsonl"),
                "community",
                "conductance",
            );
            report(
                "rc24 082 conductance",
                compare(Rule::Float(1e-9), &got, &want).unwrap(),
            );
        }
        l.f.db.close().await.unwrap();
    });
}

/// rmp #66 on the karate club and the planted communities: the
/// modularity of each parity entry's partition recorded with GDS's;
/// 078's cut at least GDS's and the optimum of `cyc12`; 082 on the
/// weighted planted graph.
#[test]
fn community_family_quality_on_karate_and_planted_graphs() {
    current(async {
        let k = Lab::load("karate").await;
        modularity_entries(&k, "karate").await;
        max_k_cut(&k, "karate", 2, "karate.gds.maxkcut.jsonl").await;
        k.f.db.close().await.unwrap();
        let p = Lab::load("planted60").await;
        modularity_entries(&p, "planted60").await;
        let planted = node_column("planted60", "community");
        eprintln!(
            "LAB planted60: planted modularity {:.6}",
            modularity(&p, &planted)
        );
        max_k_cut(&p, "planted60", 2, "planted60.gds.maxkcut.jsonl").await;
        max_k_cut(&p, "planted60", 4, "planted60.gds.maxkcut.k4.jsonl").await;
        for (config, file, nx) in [
            (
                record(&[("communityProperty", txt("community"))]),
                "planted60.gds.modularity.jsonl",
                None,
            ),
            (
                undirected(&[
                    ("communityProperty", txt("community")),
                    ("relationshipWeightProperty", txt("weight")),
                ]),
                "planted60.gds.modularity.weighted.undirected.jsonl",
                Some("planted60.nx.modularity.weighted.jsonl"),
            ),
        ] {
            let rows = p.once("gds.modularity.stream", &[], config.clone()).await;
            let got: BTreeMap<i64, Value> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Int(c) => (*c, r[1].clone()),
                    _ => panic!(),
                })
                .collect();
            report(
                file,
                compare(
                    Rule::Float(1e-9),
                    &got,
                    &expected_by_key(file, "communityId", "modularity"),
                )
                .unwrap(),
            );
            if let Some(nx) = nx {
                let sum: f64 = got
                    .values()
                    .map(|v| match v {
                        Value::Float(x) => *x,
                        _ => 0.0,
                    })
                    .sum();
                let want = expected(nx)[0]["value"].as_f64().unwrap();
                assert!(
                    super::tests_paths::close(sum, want, 1e-9),
                    "082 weighted: {sum} against {want}"
                );
            }
            let rows = p.once("gds.conductance.stream", &[], config).await;
            let got: BTreeMap<i64, Value> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Int(c) => (*c, r[1].clone()),
                    _ => panic!(),
                })
                .collect();
            let file = file.replace("modularity", "conductance");
            report(
                &file,
                compare(
                    Rule::Float(1e-9),
                    &got,
                    &expected_by_key(&file, "community", "conductance"),
                )
                .unwrap(),
            );
        }
        // 116 against GDS's `sameCommunity`.
        for m in expected("planted60.gds.lp.sameCommunity.jsonl") {
            let (a, b) = (m["a"].as_i64().unwrap(), m["b"].as_i64().unwrap());
            let got = rows(
                &p.f.s,
                "CALL gds.linkprediction.sameCommunity($a, $b)",
                Parameters::from([("a".to_string(), p.node(a)), ("b".to_string(), p.node(b))]),
            )
            .await
            .unwrap();
            assert_eq!(
                got[0][0],
                Value::Float(m["value"].as_f64().unwrap()),
                "116 {a}-{b}"
            );
        }
        let got = rows(
            &p.f.s,
            "CALL gds.linkprediction.sameCommunity($a, $b, 'missing')",
            Parameters::from([("a".to_string(), p.node(0)), ("b".to_string(), p.node(1))]),
        )
        .await
        .unwrap();
        let want = expected("planted60.gds.lp.sameCommunity.other.jsonl")[0]["value"]
            .as_f64()
            .unwrap();
        assert_eq!(got[0][0], Value::Float(want), "116 without the property");
        // 076 and 077 recorded on the planted graph and the karate club.
        spectral_record(&p, "planted60", 4).await;
        p.f.db.close().await.unwrap();
        let k = Lab::load("karate").await;
        spectral_record(&k, "karate", 2).await;
        k.f.db.close().await.unwrap();
        // 082 on the Zipf graph against GDS and NetworkX.
        let z = Lab::load("zipf60c").await;
        for (pairs, suffix, nx) in [
            (vec![], "", None),
            (
                vec![("orientation", txt("UNDIRECTED"))],
                ".undirected",
                Some("zipf60c.nx.modularity.jsonl"),
            ),
            (
                vec![
                    ("orientation", txt("UNDIRECTED")),
                    ("relationshipWeightProperty", txt("weight")),
                ],
                ".weighted.undirected",
                Some("zipf60c.nx.modularity.weighted.jsonl"),
            ),
        ] {
            let mut all = pairs.clone();
            all.push(("communityProperty", txt("community")));
            let config = record(&all);
            let det = suffix.is_empty();
            for (proc, what, col) in [
                ("gds.modularity.stream", "modularity", "communityId"),
                ("gds.conductance.stream", "conductance", "community"),
            ] {
                let rows = if det {
                    z.det(proc, &[], config.clone(), true).await
                } else {
                    z.once(proc, &[], config.clone()).await
                };
                let got: BTreeMap<i64, Value> = rows
                    .iter()
                    .map(|r| match &r[0] {
                        Value::Int(c) => (*c, r[1].clone()),
                        _ => panic!(),
                    })
                    .collect();
                let file = format!("zipf60c.gds.{what}{suffix}.jsonl");
                report(
                    &file,
                    compare(Rule::Float(1e-9), &got, &expected_by_key(&file, col, what)).unwrap(),
                );
                if let (Some(nx), "modularity") = (nx, what) {
                    let sum: f64 = got
                        .values()
                        .map(|v| match v {
                            Value::Float(x) => *x,
                            _ => 0.0,
                        })
                        .sum();
                    let want = expected(nx)[0]["value"].as_f64().unwrap();
                    assert!(
                        super::tests_paths::close(sum, want, 1e-9),
                        "082 {nx}: {sum} against {want}"
                    );
                }
            }
        }
        z.f.db.close().await.unwrap();
        // 078 against the optimum of a 12-node graph, by enumeration.
        let c = Lab::load("cyc12").await;
        for (kk, file) in [
            (2i64, "cyc12.gds.maxkcut.jsonl"),
            (3, "cyc12.gds.maxkcut.k3.jsonl"),
        ] {
            let got = max_k_cut(&c, "cyc12", kk, file).await;
            let edges: Vec<(usize, usize)> = c
                .edges
                .values()
                .map(|e| (e.src as usize, e.dst as usize))
                .collect();
            let mut best = 0;
            let mut part = [0usize; 12];
            let total = (kk as usize).pow(12);
            for code in 0..total {
                let mut x = code;
                for p in part.iter_mut() {
                    *p = x % kk as usize;
                    x /= kk as usize;
                }
                best = best.max(edges.iter().filter(|(a, b)| part[*a] != part[*b]).count());
            }
            eprintln!("LAB cyc12 078 k {kk}: lpg {got}; optimum {best}");
            assert!(got <= best);
        }
        c.f.db.close().await.unwrap();
    });
}

/// ALG-076 and 077 on `name` with `k` clusters: determinism at two seeds,
/// the modularity and ratio cut recorded.
async fn spectral_record(l: &Lab, name: &str, k: i64) {
    for (proc, what) in [
        ("cugraph.spectral_clustering.get", "076"),
        ("cugraph.balanced_cut_clustering.get", "077"),
    ] {
        for seed in [42, 7] {
            let rows = l
                .det(proc, &[int(k)], record(&[("randomSeed", int(seed))]), false)
                .await;
            let part = by_uid(l, &rows, 1);
            let mut sizes: BTreeMap<String, f64> = BTreeMap::new();
            for v in part.values() {
                *sizes.entry(format!("{v:?}")).or_default() += 1.0;
            }
            let mut ratio = 0.0;
            for (c, s) in &sizes {
                let out = l
                    .edges
                    .values()
                    .filter(|e| {
                        (format!("{:?}", part[&e.src]) == *c)
                            != (format!("{:?}", part[&e.dst]) == *c)
                    })
                    .count() as f64;
                ratio += out / s;
            }
            eprintln!(
                "LAB {name} {what} k {k} seed {seed}: modularity {:.6}; ratio cut {ratio:.6}; {} clusters",
                modularity(l, &part),
                sizes.len()
            );
        }
    }
}

/// rmp #66 on the planted point clusters: 080 gives the four clusters
/// with an inertia at most GDS's; 081 gives them with the noise points
/// labelled −1, as GDS does.
#[test]
fn clustering_quality_on_planted_points() {
    current(async {
        let l = Lab::load("blobs40").await;
        let planted = node_column("blobs40", "community");
        let clusters: BTreeMap<i64, Value> = planted
            .iter()
            .filter(|(_, v)| **v != int(-1))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let inertia = |rows: &[(i64, f64)]| rows.iter().map(|(_, d)| d * d).sum::<f64>();
        let gds: Vec<(i64, f64)> = expected("blobs40.gds.kmeans.pp.jsonl")
            .iter()
            .map(|m| {
                (
                    m["nodeId"].as_i64().unwrap(),
                    m["distanceFromCentroid"].as_f64().unwrap(),
                )
            })
            .collect();
        let gds_inertia = inertia(&gds);
        // Rule Q with GDS's KMEANS++ run: with UNIFORM, four distinct
        // uniform samples hit the four clusters with probability
        // 27/35 x 18/34 x 9/33 = 0.11, for GDS as for lpg.
        for (sampler, seed) in [("KMEANS++", 42), ("KMEANS++", 7)] {
            let config = record(&[
                ("nodeLabels", txt("C")),
                ("nodeProperty", txt("vec")),
                ("k", int(4)),
                ("initialSampler", txt(sampler)),
                ("randomSeed", int(seed)),
                ("computeSilhouette", Value::Bool(seed == 42)),
            ]);
            let rows = l.det("gds.kmeans.stream", &[], config, true).await;
            compare(Rule::Partition, &by_uid(&l, &rows, 1), &clusters)
                .unwrap_or_else(|e| panic!("080 {sampler} {seed}: {e}"));
            let ours: Vec<(i64, f64)> = rows
                .iter()
                .map(|r| match r[2] {
                    Value::Float(d) => (l.uid(&r[0]), d),
                    _ => panic!(),
                })
                .collect();
            let x = inertia(&ours);
            eprintln!(
                "LAB blobs40 080 {sampler} seed {seed}: inertia {x:.9}; GDS {gds_inertia:.9}"
            );
            assert!(
                x <= gds_inertia * (1.0 + 1e-9),
                "080: inertia {x} above GDS's {gds_inertia}"
            );
        }
        compare(
            Rule::Partition,
            &gds_partition("blobs40.gds.kmeans.pp.jsonl"),
            &clusters,
        )
        .expect("GDS's k-means finds the clusters too");
        // Seed centroids at the four corners.
        let corner = |x: f64, y: f64| Value::List(vec![Value::Float(x), Value::Float(y)].into());
        let config = record(&[
            ("nodeLabels", txt("C")),
            ("nodeProperty", txt("vec")),
            ("k", int(4)),
            (
                "seedCentroids",
                Value::List(
                    vec![
                        corner(0.0, 0.0),
                        corner(10.0, 0.0),
                        corner(0.0, 10.0),
                        corner(10.0, 10.0),
                    ]
                    .into(),
                ),
            ),
        ]);
        let rows = l.det("gds.kmeans.stream", &[], config, true).await;
        let part = by_uid(&l, &rows, 1);
        compare(Rule::Exact, &part, &clusters).expect("080 seedCentroids: cluster c at corner c");
        // UNIFORM: deterministic; whether it finds the clusters is recorded.
        let config = record(&[
            ("nodeLabels", txt("C")),
            ("nodeProperty", txt("vec")),
            ("k", int(4)),
            ("randomSeed", int(42)),
        ]);
        let rows = l.det("gds.kmeans.stream", &[], config, true).await;
        eprintln!(
            "LAB blobs40 080 UNIFORM seed 42: planted partition {}; GDS's UNIFORM run: {}",
            compare(Rule::Partition, &by_uid(&l, &rows, 1), &clusters).is_ok(),
            compare(
                Rule::Partition,
                &gds_partition("blobs40.gds.kmeans.jsonl"),
                &clusters
            )
            .is_ok()
        );
        let config = record(&[
            ("nodeProperty", txt("vec")),
            ("minClusterSize", int(5)),
            ("samples", int(3)),
        ]);
        let rows = l.det("gds.hdbscan.stream", &[], config, true).await;
        let ours = by_uid(&l, &rows, 1);
        let gds = expected_by_key("blobs40.gds.hdbscan.jsonl", "nodeId", "label");
        for part in [&ours, &gds] {
            compare(Rule::Partition, part, &planted).unwrap_or_else(|e| panic!("081: {e}"));
            for (u, v) in &planted {
                assert_eq!(*v == int(-1), part[u] == int(-1), "081: noise of node {u}");
            }
        }
        eprintln!("LAB blobs40 081: the planted partition, 4 noise points, as GDS's");
        l.f.db.close().await.unwrap();
    });
}

/// rmp #66: each parallel entry starts two workers on a 2-block
/// projection at `concurrency` 4; each other entry one at 16.
#[test]
fn worker_counts_of_the_community_family() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: f.db.engine_handle().core().location.display().to_string(),
            edges: BTreeMap::new(),
            f,
        };
        rows(
            &l.f.s,
            "INSERT (:Two {uid: 100, c: 1, vec: $a})-[:T {weight: 1}]->(:Two {uid: 101, c: 2, vec: $b})",
            Parameters::from([
                ("a".to_string(), Value::List(vec![Value::Float(1.0), Value::Float(2.0)].into())),
                ("b".to_string(), Value::List(vec![Value::Float(2.0), Value::Float(1.0)].into())),
            ]),
        )
        .await
        .unwrap();
        let base = [("nodeLabels", txt("Two")), ("relationshipTypes", txt("T"))];
        type Case<'a> = (&'a str, Vec<Value>, Vec<(&'a str, Value)>);
        let cases: Vec<Case<'_>> = vec![
            ("gds.louvain.stream", vec![], vec![]),
            ("gds.leiden.stream", vec![], vec![]),
            ("gds.labelPropagation.stream", vec![], vec![]),
            ("gds.sllpa.stream", vec![], vec![("maxIterations", int(2))]),
            ("gds.modularityOptimization.stream", vec![], vec![]),
            ("gds.maxkcut.stream", vec![], vec![]),
            (
                "gds.modularity.stream",
                vec![],
                vec![("communityProperty", txt("c"))],
            ),
            (
                "gds.conductance.stream",
                vec![],
                vec![("communityProperty", txt("c"))],
            ),
            (
                "gds.kmeans.stream",
                vec![],
                vec![("nodeProperty", txt("vec")), ("k", int(2))],
            ),
            (
                "gds.hdbscan.stream",
                vec![],
                vec![
                    ("nodeProperty", txt("vec")),
                    ("samples", int(1)),
                    ("minClusterSize", int(2)),
                ],
            ),
            ("cugraph.spectral_clustering.get", vec![int(2)], vec![]),
            ("cugraph.balanced_cut_clustering.get", vec![int(2)], vec![]),
        ];
        for (proc, args, pairs) in cases {
            let e = super::ENTRIES.iter().find(|e| e.stream == proc).unwrap();
            let c = if e.parallel { 4 } else { 16 };
            let mut all = pairs.clone();
            all.extend(base.iter().cloned());
            all.push(("concurrency", int(c)));
            crate::algo::sched::probe::take(&l.key);
            let r = l.call(proc, &args, &record(&all)).await;
            let w = crate::algo::sched::probe::take(&l.key);
            if proc.starts_with("cugraph") {
                // Two nodes cannot be split into more than one cluster
                // below the node count: 22N11, before any worker.
                assert_eq!(r.unwrap_err().code(), "22N11");
                assert!(!e.parallel);
                continue;
            }
            r.unwrap_or_else(|e| panic!("{proc}: {e}"));
            if e.parallel {
                assert_eq!(w, Some(2), "{proc}: {w:?} workers on 2 blocks");
            } else {
                assert!(w.is_none() || w == Some(1), "{proc}: {w:?} workers");
            }
            eprintln!("LAB workers {proc}: {w:?}");
        }
        // 076 and 077 on three nodes: one worker at 16.
        rows(
            &l.f.s,
            "MATCH (b:Two {uid: 101}) INSERT (b)-[:T]->(:Two {uid: 102, c: 1})",
            Parameters::new(),
        )
        .await
        .unwrap();
        for proc in [
            "cugraph.spectral_clustering.get",
            "cugraph.balanced_cut_clustering.get",
        ] {
            crate::algo::sched::probe::take(&l.key);
            l.call(
                proc,
                &[int(2), int(1)],
                &record(&[("nodeLabels", txt("Two")), ("concurrency", int(16))]),
            )
            .await
            .unwrap_or_else(|e| panic!("{proc}: {e}"));
            let w = crate::algo::sched::probe::take(&l.key);
            assert!(w.is_none() || w == Some(1), "{proc}: {w:?} workers");
        }
        l.f.db.close().await.unwrap();
    });
}

/// rmp #66 (FR-LIM-001): a run under a transaction memory limit smaller
/// than its projection fails with 51N73; the write modes; the
/// accountant's peak never exceeds the cap.
#[test]
fn community_memory_cap_and_write_modes() {
    current(async {
        let l = Lab::load("planted60").await;
        for proc in ["gds.louvain.stream", "gds.kmeans.stream"] {
            let mut limits = l.f.s.limits();
            limits.memory = Some(4096);
            l.f.s.set_limits(limits);
            let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
            let config = record(&[("nodeProperty", txt("vec")), ("k", int(2))]);
            let config = if proc.contains("kmeans") {
                config
            } else {
                record(&[])
            };
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
            assert_eq!(e.code(), "51N73", "{proc}: {e}");
        }
        let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
        for (proc, config, column, property) in [
            ("gds.louvain.write", undirected(&[]), 9, "comm"),
            (
                "gds.sllpa.write",
                undirected(&[("maxIterations", int(5))]),
                5,
                "labels",
            ),
            (
                "gds.kmeans.write",
                record(&[("nodeProperty", txt("vec")), ("k", int(3))]),
                4,
                "cluster",
            ),
        ] {
            let row = drain(
                tx.execute(
                    &format!("CALL {proc}($c)"),
                    param("c", with(&config, "writeProperty", txt(property))),
                )
                .await
                .unwrap_or_else(|e| panic!("{proc}: {e}")),
            )
            .await
            .unwrap()
            .remove(0);
            assert_eq!(row[column], int(60), "{proc} nodePropertiesWritten");
            let n = drain(
                tx.execute(
                    &format!("MATCH (n) WHERE n.{property} IS NOT NULL RETURN count(*) AS c"),
                    Parameters::new(),
                )
                .await
                .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(n[0][0], int(60), "{proc}");
        }
        tx.rollback().await.unwrap();
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap());
        l.f.db.close().await.unwrap();
    });
}

/// Laboratory record (rmp #66): the accountant's peak of the family on
/// `zipf300`; run under `docs/evidence/lab-sprint-5/heaptrack.sh` for the
/// heap peak.
#[test]
#[ignore = "laboratory measurement"]
fn lab_family66_memory() {
    current(async {
        let z = Lab::load("zipf300").await;
        let u = || ("orientation", txt("UNDIRECTED"));
        for (proc, args, config) in [
            ("gds.louvain.stream", vec![], record(&[u()])),
            ("gds.leiden.stream", vec![], record(&[u()])),
            ("gds.labelPropagation.stream", vec![], record(&[u()])),
            (
                "gds.sllpa.stream",
                vec![],
                record(&[u(), ("maxIterations", int(20))]),
            ),
            ("gds.modularityOptimization.stream", vec![], record(&[u()])),
            ("gds.maxkcut.stream", vec![], record(&[])),
            (
                "gds.kmeans.stream",
                vec![],
                record(&[("nodeProperty", txt("vec")), ("k", int(5))]),
            ),
            (
                "gds.hdbscan.stream",
                vec![],
                record(&[("nodeProperty", txt("vec"))]),
            ),
            ("cugraph.spectral_clustering.get", vec![int(5)], record(&[])),
            (
                "cugraph.balanced_cut_clustering.get",
                vec![int(5)],
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

/// rmp #66: the options of the community entries on the ring of cliques
/// and the planted graph: `seedProperty`, `includeIntermediateCommunities`,
/// `consecutiveIds`, `minCommunitySize`, `nodeWeightProperty`, the
/// k-cut's `minCommunitySize` and `vnsMaxNeighborhoodOrder`, k-means
/// restarts, and their argument errors.
#[test]
fn community_options() {
    current(async {
        let l = Lab::load("rc24").await;
        let planted = node_column("rc24", "community");
        // Seeds: label propagation keeps them, Louvain starts from them.
        let seeded = undirected(&[("seedProperty", txt("community"))]);
        let rows = l
            .det("gds.labelPropagation.stream", &[], seeded.clone(), true)
            .await;
        assert_eq!(by_uid(&l, &rows, 1), planted, "063 keeps the seeds");
        let rows = l.det("gds.louvain.stream", &[], seeded, true).await;
        compare(Rule::Partition, &by_uid(&l, &rows, 2), &planted).unwrap();
        // Intermediate and consecutive identifiers; GDS refuses both at
        // once (`LouvainBaseConfig.validate`, rmp #71).
        let config = undirected(&[("includeIntermediateCommunities", Value::Bool(true))]);
        let rows = l.det("gds.louvain.stream", &[], config.clone(), true).await;
        let levels = summary(&l, "gds.louvain.stream", &[], &config, "ranLevels").await;
        for r in &rows {
            let Value::List(inter) = &r[1] else {
                panic!("{r:?}")
            };
            assert_eq!(Value::Int(inter.len() as i64), levels);
            assert_eq!(inter.last(), Some(&r[2]));
        }
        let config = undirected(&[("consecutiveIds", Value::Bool(true))]);
        let rows = l.det("gds.louvain.stream", &[], config.clone(), true).await;
        for r in &rows {
            assert!(matches!(r[2], Value::Int(c) if (0..4).contains(&c)));
        }
        let both = with(&config, "includeIntermediateCommunities", Value::Bool(true));
        let e = l.call("gds.louvain.stream", &[], &both).await.unwrap_err();
        assert_eq!(e.code(), "22N11");
        // `minCommunitySize` keeps the cliques of 6 and drops them at 7.
        for (min, kept) in [(6, 24), (7, 0)] {
            for proc in [
                "gds.louvain.stream",
                "gds.leiden.stream",
                "gds.labelPropagation.stream",
                "gds.modularityOptimization.stream",
            ] {
                let rows = l
                    .once(proc, &[], undirected(&[("minCommunitySize", int(min))]))
                    .await;
                assert_eq!(rows.len(), kept, "{proc} minCommunitySize {min}");
            }
        }
        l.f.db.close().await.unwrap();
        let p = Lab::load("planted60").await;
        // Weighted label propagation with node weights.
        let config = undirected(&[
            ("nodeWeightProperty", txt("community")),
            ("relationshipWeightProperty", txt("weight")),
        ]);
        let rows = p
            .det("gds.labelPropagation.stream", &[], config, true)
            .await;
        assert_eq!(rows.len(), 60);
        // The k-cut's minimum community size and VNS.
        let config = record(&[
            ("k", int(4)),
            ("minCommunitySize", int(12)),
            ("vnsMaxNeighborhoodOrder", int(3)),
        ]);
        let rows = p.det("gds.maxkcut.stream", &[], config, true).await;
        let mut sizes: BTreeMap<String, usize> = BTreeMap::new();
        for r in &rows {
            *sizes.entry(format!("{:?}", r[1])).or_default() += 1;
        }
        assert!(sizes.values().all(|s| *s >= 12), "{sizes:?}");
        let e = p
            .call(
                "gds.maxkcut.stream",
                &[],
                &record(&[("k", int(4)), ("minCommunitySize", int(16))]),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        // K-means restarts: GDS keeps the restart of least average
        // distance to the centroid (`Kmeans.updateBestSolution`).
        let base = record(&[("nodeProperty", txt("vec")), ("k", int(4))]);
        let inertia = |rows: &[Vec<Value>]| {
            rows.iter()
                .map(|r| match r[2] {
                    Value::Float(d) => d,
                    _ => panic!(),
                })
                .sum::<f64>()
        };
        let one = inertia(&p.once("gds.kmeans.stream", &[], base.clone()).await);
        let five = inertia(
            &p.det(
                "gds.kmeans.stream",
                &[],
                with(&base, "numberOfRestarts", int(5)),
                true,
            )
            .await,
        );
        eprintln!(
            "LAB planted60 080: total distance {one:.6} with one restart, {five:.6} with five"
        );
        assert!(five <= one);
        for (proc, config) in [
            (
                "gds.kmeans.stream",
                with(
                    &base,
                    "seedCentroids",
                    Value::List(vec![Value::List(vec![Value::Float(1.0)].into())].into()),
                ),
            ),
            (
                "gds.hdbscan.stream",
                record(&[("nodeProperty", txt("vec")), ("samples", int(61))]),
            ),
            (
                "gds.kmeans.stream",
                record(&[("nodeProperty", txt("missing"))]),
            ),
        ] {
            let e = p.call(proc, &[], &config).await.unwrap_err();
            assert_eq!(e.code(), "22N11", "{proc} {config:?}");
        }
        p.f.db.close().await.unwrap();
    });
}
