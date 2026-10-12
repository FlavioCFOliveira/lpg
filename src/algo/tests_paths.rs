//! Tests of the traversal and shortest-path families (rmp #64): each
//! entry against its oracle (`algorithm-entries.md` Verification), bit-equal
//! rows at `concurrency` 1 to 16 twice, `CALL` equal to the Rust API, the
//! worker-count rule, the memory cap, write modes. Graph data lives under
//! `/tmp/lpgram` (CLAUDE.md); oracle outputs are committed under
//! `src/algo/testdata/expected`.
//!
//! The helpers ([`Lab`]) are shared with the tests of rmp #69
//! (`tests_structure.rs`).

use std::collections::{BTreeMap, BTreeSet};

use crate::test_support::algo::{
    CONCURRENCY, Fixture, determinism_async, drain, expected, first_difference, param, record,
    rows, with,
};
use crate::types::Value;
use crate::{Parameters, TransactionOptions};

/// The memory cap of these tests.
pub(super) const CAP: u64 = 32 << 20;

pub(super) fn current<F: std::future::Future<Output = ()>>(f: F) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a runtime")
        .block_on(f);
}

pub(super) fn int(i: i64) -> Value {
    Value::Int(i)
}

pub(super) fn txt(x: &str) -> Value {
    Value::string(x)
}

pub(super) fn strs(xs: &[&str]) -> Value {
    Value::List(xs.iter().map(|x| txt(x)).collect::<Vec<_>>().into())
}

/// A JSON number or integer as a float; `"inf"` as infinity.
pub(super) fn num(v: &serde_json::Value) -> f64 {
    match v {
        serde_json::Value::Number(n) => n.as_f64().expect("a number"),
        serde_json::Value::String(s) if s == "inf" => f64::INFINITY,
        other => panic!("not a number: {other}"),
    }
}

/// The integers of a JSON list.
pub(super) fn ints(v: &serde_json::Value) -> Vec<i64> {
    v.as_array()
        .expect("a list")
        .iter()
        .map(|x| x.as_i64().expect("an integer"))
        .collect()
}

/// `|a − b| ≤ t · max(1, |b|)` (rule F).
pub(super) fn close(a: f64, b: f64, t: f64) -> bool {
    a == b || (a - b).abs() <= t * b.abs().max(1.0)
}

/// An edge of a fixture: source and target `uid` and `weight` (NaN when
/// absent).
#[derive(Debug, Clone, Copy)]
pub(super) struct E {
    pub(super) src: i64,
    pub(super) dst: i64,
    pub(super) weight: f64,
}

/// A loaded fixture with its edges, for the checks.
pub(super) struct Lab {
    pub(super) f: Fixture,
    /// Edges by element identifier.
    pub(super) edges: BTreeMap<u64, E>,
    pub(super) key: String,
}

impl Lab {
    /// Fixture `name` loaded in a fresh database.
    pub(super) async fn load(name: &str) -> Lab {
        let f = Fixture::load(name, CAP).await;
        let edges = rows(
            &f.s,
            "MATCH (a)-[e]->(b) RETURN e, a.uid AS a, b.uid AS b, e.weight AS w",
            Parameters::new(),
        )
        .await
        .unwrap()
        .into_iter()
        .map(|r| {
            let Value::Edge(id) = r[0] else { panic!() };
            let w = match r[3] {
                Value::Float(w) => w,
                _ => f64::NAN,
            };
            let (Value::Int(a), Value::Int(b)) = (&r[1], &r[2]) else {
                panic!()
            };
            (
                id,
                E {
                    src: *a,
                    dst: *b,
                    weight: w,
                },
            )
        })
        .collect();
        let key = f.db.engine_handle().core().location.display().to_string();
        Lab { f, edges, key }
    }

    /// The node of `uid`.
    pub(super) fn node(&self, uid: i64) -> Value {
        Value::Node(self.f.ids[usize::try_from(uid).unwrap()])
    }

    /// The `uid` of a node value (an element identifier or a node).
    pub(super) fn uid(&self, v: &Value) -> i64 {
        match v {
            Value::Int(id) => self.f.uids[&id.cast_unsigned()],
            Value::Node(id) => self.f.uids[id],
            other => panic!("not a node: {other:?}"),
        }
    }

    /// The `uid`s of a list of nodes.
    pub(super) fn uids(&self, v: &Value) -> Vec<i64> {
        match v {
            Value::List(l) => l.iter().map(|x| self.uid(x)).collect(),
            other => panic!("not a list: {other:?}"),
        }
    }

    /// The rows of `CALL proc(args…, $c)` at concurrency `c`.
    pub(super) async fn call(
        &self,
        proc: &str,
        args: &[Value],
        config: &Value,
    ) -> Result<Vec<Vec<Value>>, crate::Error> {
        let (text, ps) = call_text(proc, args, config);
        rows(&self.f.s, &text, ps).await
    }

    /// The rows of one run of `proc` (no determinism check).
    pub(super) async fn once(&self, proc: &str, args: &[Value], config: Value) -> Vec<Vec<Value>> {
        self.call(proc, args, &config)
            .await
            .unwrap_or_else(|e| panic!("{proc}: {e}"))
    }

    /// The rows of `proc`, checked bit-equal at `concurrency` 1 to 16,
    /// twice each; each run starts at most `concurrency` workers, or one
    /// (or none: the statement's thread) when the entry is not parallel;
    /// the Rust API gives the same rows in one transaction.
    pub(super) async fn det(
        &self,
        proc: &str,
        args: &[Value],
        config: Value,
        parallel: bool,
    ) -> Vec<Vec<Value>> {
        let out = determinism_async(|c| {
            let config = with(&config, "concurrency", int(c as i64));
            async move {
                crate::algo::sched::probe::take(&self.key);
                let rows = self
                    .call(proc, args, &config)
                    .await
                    .unwrap_or_else(|e| panic!("{proc}: {e}"));
                let w = crate::algo::sched::probe::take(&self.key).unwrap_or(1);
                if parallel {
                    assert!(w <= c, "{proc}: {w} workers at concurrency {c}");
                } else {
                    assert_eq!(w, 1, "{proc}: {w} workers at concurrency {c}");
                }
                rows
            }
        })
        .await
        .unwrap_or_else(|m| panic!("{proc}: {m}"));
        // CALL and the Rust API on one snapshot (FR-ALG-002).
        let r = crate::proc::Registry::with_builtins();
        let names: Vec<String> = r
            .get(proc)
            .unwrap()
            .get()
            .sig
            .params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let tx = self.f.s.begin(TransactionOptions::default()).await.unwrap();
        let mut ps = Parameters::new();
        for (n, a) in names.iter().zip(args) {
            ps.insert(n.clone(), a.clone());
        }
        ps.insert("configuration".to_string(), config.clone());
        let by_api = drain(tx.run_algorithm(proc, ps).await.unwrap())
            .await
            .unwrap();
        let (text, cps) = call_text(proc, args, &config);
        let by_call = drain(tx.execute(&text, cps).await.unwrap()).await.unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(first_difference(&by_call, &by_api), None, "{proc}");
        assert_eq!(first_difference(&out, &by_api), None, "{proc}");
        out
    }

    /// Checks a path value against node `uid`s: each edge joins
    /// consecutive nodes (in either direction when `undirected`), and, when
    /// `costs` are given, the cumulative weights (or hop counts when not
    /// `weighted`) equal them under rule F. Returns the path's cost.
    pub(super) fn check_path(
        &self,
        path: &Value,
        nodes: &[i64],
        costs: Option<&[f64]>,
        weighted: bool,
        undirected: bool,
    ) -> f64 {
        let Value::Path(p) = path else {
            panic!("not a path: {path:?}")
        };
        let ns: Vec<i64> = p.iter().step_by(2).map(|id| self.f.uids[id]).collect();
        assert_eq!(ns, nodes, "the path's nodes");
        let mut total = 0.0;
        let mut acc = vec![0.0];
        for (i, e) in p.iter().skip(1).step_by(2).enumerate() {
            let e = self.edges[e];
            let (a, b) = (nodes[i], nodes[i + 1]);
            assert!(
                (e.src, e.dst) == (a, b) || (undirected && (e.dst, e.src) == (a, b)),
                "edge {e:?} does not join {a} and {b}"
            );
            total += if weighted { e.weight } else { 1.0 };
            acc.push(total);
        }
        if let Some(c) = costs {
            assert_eq!(c.len(), acc.len());
            for (x, y) in c.iter().zip(&acc) {
                assert!(close(*x, *y, 1e-9), "costs {c:?} instead of {acc:?}");
            }
        }
        total
    }

    /// The checked PathRows of `rows` keyed by target `uid`: (`nodeIds`
    /// as uids, `totalCost`).
    pub(super) fn path_rows(
        &self,
        rows: &[Vec<Value>],
        weighted: bool,
    ) -> BTreeMap<i64, (Vec<i64>, f64)> {
        let mut out = BTreeMap::new();
        for r in rows {
            let nodes = self.uids(&r[4]);
            let costs: Vec<f64> = match &r[5] {
                Value::List(l) => l
                    .iter()
                    .map(|x| match x {
                        Value::Float(f) => *f,
                        _ => panic!(),
                    })
                    .collect(),
                _ => panic!(),
            };
            let Value::Float(total) = r[3] else { panic!() };
            assert_eq!(costs.last().copied(), Some(total));
            assert_eq!(self.uid(&r[1]), nodes[0]);
            assert_eq!(self.uid(&r[2]), nodes[nodes.len() - 1]);
            self.check_path(&r[6], &nodes, Some(&costs), weighted, false);
            out.insert(nodes[nodes.len() - 1], (nodes, total));
        }
        out
    }
}

/// The statement `CALL proc($a0, …, $c)` and its parameters: the
/// positional arguments `args`, then their defaults for those left out,
/// then the configuration.
pub(super) fn call_text(proc: &str, args: &[Value], config: &Value) -> (String, Parameters) {
    let r = crate::proc::Registry::with_builtins();
    let sig = r
        .get(proc)
        .map(|x| x.get().sig.params.clone())
        .unwrap_or_default();
    let mut ps = Parameters::new();
    let mut list = Vec::new();
    let positional = sig.len().saturating_sub(1).max(args.len());
    for i in 0..positional {
        let v = args
            .get(i)
            .cloned()
            .or_else(|| sig.get(i).and_then(|p| p.default.clone()))
            .unwrap_or(Value::Null);
        ps.insert(format!("a{i}"), v);
        list.push(format!("$a{i}"));
    }
    ps.insert("c".to_string(), config.clone());
    list.push("$c".to_string());
    (format!("CALL {proc}({})", list.join(", ")), ps)
}

/// GDS PathRows of `file` by target uid: (`nodeIds`, `totalCost`).
pub(super) fn gds_paths(file: &str) -> BTreeMap<i64, (Vec<i64>, f64)> {
    expected(file)
        .into_iter()
        .map(|m| {
            let nodes = ints(&m["nodeIds"]);
            (
                m["targetNode"].as_i64().unwrap(),
                (nodes, num(&m["totalCost"])),
            )
        })
        .collect()
}

/// Rule F on costs by target, and the same targets.
pub(super) fn same_costs(
    what: &str,
    got: &BTreeMap<i64, (Vec<i64>, f64)>,
    want: &BTreeMap<i64, f64>,
    t: f64,
) -> f64 {
    let gk: Vec<_> = got.keys().collect();
    let wk: Vec<_> = want.keys().collect();
    assert_eq!(gk, wk, "{what}: targets");
    let mut worst = 0.0f64;
    for (k, w) in want {
        let g = got[k].1;
        assert!(close(g, *w, t), "{what}: target {k}: {g} instead of {w}");
        worst = worst.max((g - w).abs() / w.abs().max(1.0));
    }
    worst
}

/// The `{node: k, value: v}` lines of a NetworkX output.
pub(super) fn nx_map(file: &str) -> BTreeMap<i64, serde_json::Value> {
    expected(file)
        .into_iter()
        .map(|m| (m["node"].as_i64().unwrap(), m["value"].clone()))
        .collect()
}

/// The `{value: v}` lines of a NetworkX output.
pub(super) fn nx_values(file: &str) -> Vec<serde_json::Value> {
    expected(file)
        .into_iter()
        .map(|m| m["value"].clone())
        .collect()
}

/// A JSON object of string keys to numbers as a map of uid to float.
pub(super) fn num_map(v: &serde_json::Value) -> BTreeMap<i64, f64> {
    v.as_object()
        .unwrap()
        .iter()
        .filter(|(_, x)| x.as_str().is_none_or(|s| s != "inf"))
        .map(|(k, x)| (k.parse().unwrap(), num(x)))
        .collect()
}

/// The breadth-first depth of every node reachable from `s` over the
/// fixture's directed edges.
fn depths(l: &Lab, s: i64) -> BTreeMap<i64, usize> {
    let mut d = BTreeMap::from([(s, 0)]);
    let mut queue = std::collections::VecDeque::from([s]);
    while let Some(v) = queue.pop_front() {
        for e in l.edges.values().filter(|e| e.src == v) {
            if !d.contains_key(&e.dst) {
                d.insert(e.dst, d[&v] + 1);
                queue.push_back(e.dst);
            }
        }
    }
    d
}

/// ALG-001, ALG-002 (rule V): the same node set as GDS, BFS order by
/// depth, each visited node joined to an earlier one; reports whether the
/// order equals GDS's.
async fn check_traversal(
    l: &Lab,
    proc: &str,
    file: &str,
    config: Value,
    parallel: bool,
    bfs: bool,
) {
    let g = &expected(file)[0];
    let source = g["sourceNode"].as_i64().unwrap();
    let rows = l.det(proc, &[], config, parallel).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(l.uid(&rows[0][0]), source);
    assert_eq!(rows[0][2], Value::Null, "GDS's path is virtual: null");
    let got = l.uids(&rows[0][1]);
    let want = ints(&g["nodeIds"]);
    let (a, b): (BTreeSet<_>, BTreeSet<_>) = (got.iter().collect(), want.iter().collect());
    assert_eq!(a, b, "{proc} {file}: node set");
    assert_eq!(got.len(), a.len(), "{proc}: each node once");
    if bfs {
        let d = depths(l, source);
        assert!(
            got.windows(2).all(|w| d[&w[0]] <= d[&w[1]]),
            "{proc}: BFS depth order"
        );
    }
    for (i, v) in got.iter().enumerate().skip(1) {
        assert!(
            l.edges
                .values()
                .any(|e| e.dst == *v && got[..i].contains(&e.src)),
            "{proc}: {v} is not reached from an earlier node"
        );
    }
    eprintln!(
        "LAB {proc} {file}: {} nodes; order equal to GDS's: {}",
        got.len(),
        got == want
    );
}

/// rmp #64 (ALG-001, 002, 003, 004, 007, 008, 009, 011) on `zipf60`,
/// `uniform60` and `dag40`.
#[test]
fn traversal_family_matches_oracles() {
    current(async {
        let z = Lab::load("zipf60").await;
        let n1 = z.node(1);
        // 001 and 002.
        let src = |l: &Lab, s: i64| record(&[("sourceNode", l.node(s))]);
        check_traversal(
            &z,
            "gds.bfs.stream",
            "zipf60.gds.bfs.jsonl",
            src(&z, 1),
            true,
            true,
        )
        .await;
        check_traversal(
            &z,
            "gds.bfs.stream",
            "zipf60.gds.bfs.depth2.jsonl",
            with(&src(&z, 1), "maxDepth", int(2)),
            true,
            true,
        )
        .await;
        check_traversal(
            &z,
            "gds.bfs.stream",
            "zipf60.gds.bfs.target.jsonl",
            with(
                &src(&z, 1),
                "targetNodes",
                Value::List(vec![z.node(8)].into()),
            ),
            true,
            true,
        )
        .await;
        check_traversal(
            &z,
            "gds.dfs.stream",
            "zipf60.gds.dfs.jsonl",
            src(&z, 1),
            false,
            false,
        )
        .await;
        check_traversal(
            &z,
            "gds.dfs.stream",
            "zipf60.gds.dfs.target.jsonl",
            with(
                &src(&z, 1),
                "targetNodes",
                Value::List(vec![z.node(8)].into()),
            ),
            false,
            false,
        )
        .await;
        // 003 against MAGE (rule X): node sets.
        let n26 = z.node(26);
        for (proc, types, dist, file) in [
            (
                "neighbors.at_hop",
                strs(&[]),
                2,
                "zipf60.mage.athop.all2.jsonl",
            ),
            (
                "neighbors.at_hop",
                strs(&["R>"]),
                2,
                "zipf60.mage.athop.out2.jsonl",
            ),
            (
                "neighbors.at_hop",
                strs(&["<S", "R"]),
                1,
                "zipf60.mage.athop.in1.jsonl",
            ),
        ] {
            let rows = z
                .det(proc, &[n26.clone(), types, int(dist)], record(&[]), false)
                .await;
            let got: BTreeSet<i64> = rows.iter().map(|r| z.uid(&r[0])).collect();
            let want: BTreeSet<i64> = expected(file)
                .iter()
                .map(|m| m["nodes"].as_i64().unwrap())
                .collect();
            assert_eq!(got.len(), rows.len());
            assert_eq!(got, want, "{proc} {file}");
        }
        for (types, dist, file) in [
            (strs(&[]), 3, "zipf60.mage.byhop.all3.jsonl"),
            (strs(&["R>", "S>"]), 4, "zipf60.mage.byhop.out4.jsonl"),
        ] {
            let rows = z
                .det(
                    "neighbors.by_hop",
                    &[n26.clone(), types, int(dist)],
                    record(&[]),
                    false,
                )
                .await;
            let got: Vec<BTreeSet<i64>> = rows
                .iter()
                .map(|r| z.uids(&r[0]).into_iter().collect())
                .collect();
            let want: Vec<BTreeSet<i64>> = expected(file)
                .iter()
                .map(|m| ints(&m["nodes"]).into_iter().collect())
                .collect();
            assert_eq!(got, want, "by_hop {file}");
        }
        // 004 against MAGE (rule X): node multisets.
        let multiset = |xs: Vec<i64>| {
            let mut x = xs;
            x.sort_unstable();
            x
        };
        for (config, file) in [
            (
                record(&[("maxHops", int(2))]),
                "zipf60.mage.subgraphnodes.jsonl",
            ),
            (
                record(&[
                    ("relationshipFilter", strs(&["R>"])),
                    ("labelFilter", strs(&["+B", "/A"])),
                    ("maxHops", int(3)),
                ]),
                "zipf60.mage.subgraphnodes.filtered.jsonl",
            ),
            (
                record(&[
                    ("relationshipFilter", strs(&["<R>", "S>"])),
                    ("maxHops", int(3)),
                    ("filterStartNode", Value::Bool(false)),
                ]),
                "zipf60.mage.subgraphnodes.both.jsonl",
            ),
        ] {
            let rows = z
                .det(
                    "gds.pathSubgraphNodes.stream",
                    &[],
                    with(&config, "startNode", n26.clone()),
                    false,
                )
                .await;
            let got = multiset(rows.iter().map(|r| z.uid(&r[0])).collect());
            let want = multiset(
                expected(file)
                    .iter()
                    .map(|m| m["nodes"].as_i64().unwrap())
                    .collect(),
            );
            if file.contains("both") {
                // MAGE keeps `<T>` matches in a set of `string_view`s of a
                // local `std::string` (`path.cpp:335, 471`): the views
                // dangle, so which matches it repeats is undefined. The
                // node sets are compared; the repetitions are recorded.
                let (g, w): (BTreeSet<i64>, BTreeSet<i64>) = (
                    got.iter().copied().collect(),
                    want.iter().copied().collect(),
                );
                assert_eq!(g, w, "{file}: node set");
                eprintln!(
                    "LAB 004 {file}: {} rows, MAGE {} (node sets equal)",
                    got.len(),
                    want.len()
                );
            } else {
                assert_eq!(got, want, "{file}");
            }
        }
        let rows = z
            .det(
                "gds.pathSubgraphAll.stream",
                &[],
                record(&[
                    ("startNode", n26.clone()),
                    ("relationshipFilter", strs(&["R>"])),
                    ("maxHops", int(2)),
                ]),
                false,
            )
            .await;
        let m = &expected("zipf60.mage.subgraphall.jsonl")[0];
        assert_eq!(multiset(z.uids(&rows[0][0])), multiset(ints(&m["nodes"])));
        let mut got: Vec<(i64, i64)> = match &rows[0][1] {
            Value::List(l) => l
                .iter()
                .map(|e| match e {
                    Value::Edge(id) => (z.edges[id].src, z.edges[id].dst),
                    _ => panic!(),
                })
                .collect(),
            _ => panic!(),
        };
        let mut want: Vec<(i64, i64)> = m["rels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_i64().unwrap(), p[1].as_i64().unwrap()))
            .collect();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "subgraph_all rels");
        // 007: a topological order of GDS's node set; distances X.
        for (l, file) in [(&z, "zipf60.gds.topo.jsonl")] {
            check_topo(l, file, true).await;
        }
        // 008 against NetworkX (rule X).
        for (proc, file) in [
            ("graph_util.ancestors", "zipf60.nx.ancestors.jsonl"),
            ("graph_util.descendants", "zipf60.nx.descendants.jsonl"),
        ] {
            let rows = z
                .det(proc, std::slice::from_ref(&n26), record(&[]), false)
                .await;
            let got: BTreeSet<i64> = z.uids(&rows[0][0]).into_iter().collect();
            let want: BTreeSet<i64> = nx_values(file)
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect();
            assert_eq!(got, want, "{proc}");
        }
        // 009 against NetworkX (rule X).
        for (a, b, file) in [
            (1, 8, "zipf60.nx.haspath.1-8.jsonl"),
            (8, 1, "zipf60.nx.haspath.8-1.jsonl"),
        ] {
            let rows = z
                .det(
                    "gds.hasPathConnecting.stream",
                    &[],
                    record(&[("sourceNode", z.node(a)), ("targetNode", z.node(b))]),
                    false,
                )
                .await;
            assert_eq!(
                rows[0][2],
                Value::Bool(nx_values(file)[0].as_bool().unwrap())
            );
        }
        // 011 against NetworkX (rule X).
        check_dominators(&z, 1, "zipf60").await;
        let _ = n1;
        z.f.db.close().await.unwrap();

        let u = Lab::load("uniform60").await;
        check_traversal(
            &u,
            "gds.bfs.stream",
            "uniform60.gds.bfs.jsonl",
            src(&u, 14),
            true,
            true,
        )
        .await;
        check_traversal(
            &u,
            "gds.dfs.stream",
            "uniform60.gds.dfs.depth3.jsonl",
            with(&src(&u, 14), "maxDepth", int(3)),
            false,
            false,
        )
        .await;
        check_topo(&u, "uniform60.gds.topo.jsonl", true).await;
        let rows = u
            .once("graph_util.ancestors", &[u.node(14)], record(&[]))
            .await;
        let got: BTreeSet<i64> = u.uids(&rows[0][0]).into_iter().collect();
        let want: BTreeSet<i64> = nx_values("uniform60.nx.ancestors.jsonl")
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(got, want);
        let rows = u
            .once(
                "gds.hasPathConnecting.stream",
                &[],
                record(&[("sourceNode", u.node(14)), ("targetNode", u.node(15))]),
            )
            .await;
        assert_eq!(
            rows[0][2],
            Value::Bool(
                nx_values("uniform60.nx.haspath.14-15.jsonl")[0]
                    .as_bool()
                    .unwrap()
            )
        );
        check_dominators(&u, 14, "uniform60").await;
        u.f.db.close().await.unwrap();

        let d = Lab::load("dag40").await;
        check_topo(&d, "dag40.gds.topo.jsonl", true).await;
        check_topo(&d, "dag40.gds.topo.plain.jsonl", false).await;
        d.f.db.close().await.unwrap();
    });
}

/// ALG-007 (rule V, X on distances) against GDS output `file`.
async fn check_topo(l: &Lab, file: &str, distance: bool) {
    let rows = l
        .det(
            "gds.dag.topologicalSort.stream",
            &[],
            record(&[("computeMaxDistanceFromSource", Value::Bool(distance))]),
            true,
        )
        .await;
    // On a cyclic projection GDS streams one row per node of the graph:
    // the slots its result array never filled come out as internal node 0
    // with distance 0.0, a trailing run of identical rows (measured on
    // `zipf60` and `uniform60`, rmp #64). That run is not part of the
    // order.
    let mut gds = expected(file);
    let filler = |m: &serde_json::Map<String, serde_json::Value>| m["nodeId"].as_i64() == Some(0);
    let zeros = gds.iter().filter(|m| filler(m)).count();
    let mut stripped = 0;
    while zeros > 1 && gds.last().is_some_and(filler) {
        gds.pop();
        stripped += 1;
    }
    if stripped > 0 {
        eprintln!("LAB 007 {file}: {stripped} trailing filler rows of GDS left out");
    }
    let want: BTreeMap<i64, serde_json::Value> = gds
        .into_iter()
        .map(|m| {
            (
                m["nodeId"].as_i64().unwrap(),
                m["maxDistanceFromSource"].clone(),
            )
        })
        .collect();
    let got: BTreeMap<i64, Value> = rows.iter().map(|r| (l.uid(&r[0]), r[1].clone())).collect();
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>(),
        "{file}: node set"
    );
    for (k, w) in &want {
        match (&got[k], w) {
            (Value::Null, serde_json::Value::Null) => {}
            (Value::Float(g), w) => assert_eq!(*g, num(w), "{file}: node {k}"),
            (g, w) => panic!("{file}: node {k}: {g:?} instead of {w}"),
        }
    }
    let pos: BTreeMap<i64, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (l.uid(&r[0]), i))
        .collect();
    for e in l.edges.values() {
        if let (Some(a), Some(b)) = (pos.get(&e.src), pos.get(&e.dst)) {
            assert!(a < b, "{file}: edge {e:?} against the order");
        }
    }
}

/// ALG-011 against NetworkX's `immediate_dominators` (the root left out)
/// and `dominance_frontiers` (rule X).
async fn check_dominators(l: &Lab, root: i64, name: &str) {
    let rows = l
        .det(
            "gds.dominatorsSimpleFast.stream",
            &[],
            record(&[("sourceNode", l.node(root))]),
            false,
        )
        .await;
    let idom = nx_map(&format!("{name}.nx.idom.jsonl"));
    let df = nx_map(&format!("{name}.nx.frontiers.jsonl"));
    assert_eq!(rows.len(), df.len());
    for r in &rows {
        let v = l.uid(&r[0]);
        match &r[1] {
            Value::Null => assert_eq!(v, root),
            d => assert_eq!(l.uid(d), idom[&v].as_i64().unwrap(), "idom of {v}"),
        }
        let mut f = l.uids(&r[2]);
        f.sort_unstable();
        let mut want = ints(&df[&v]);
        want.sort_unstable();
        assert_eq!(f, want, "frontier of {v}");
    }
}

/// rmp #64 (ALG-013 to ALG-032) on `zipf60`: each entry against its
/// oracle, bit-equal at `concurrency` 1 to 16, `CALL` equal to the API.
#[test]
fn path_family_matches_oracles_on_zipf60() {
    current(async {
        let z = Lab::load("zipf60").await;
        let w = ("relationshipWeightProperty", txt("weight"));
        let src = record(&[("sourceNode", z.node(1)), w.clone()]);
        // 013 against NetworkX (X lengths, V paths).
        let rows = z
            .det(
                "gds.unweightedShortestPaths.stream",
                &[],
                record(&[("sourceNode", z.node(1))]),
                false,
            )
            .await;
        let got = z.path_rows(&rows, false);
        let want: BTreeMap<i64, f64> = nx_map("zipf60.nx.sp.length.source.jsonl")
            .into_iter()
            .map(|(k, v)| (k, num(&v)))
            .collect();
        same_costs("013 source", &got, &want, 0.0);
        let rows = z
            .once(
                "gds.unweightedShortestPaths.stream",
                &[],
                record(&[("targetNode", z.node(8))]),
            )
            .await;
        let by_source: BTreeMap<i64, f64> = rows
            .iter()
            .map(|r| {
                let nodes = z.uids(&r[4]);
                let Value::Float(c) = r[3] else { panic!() };
                z.check_path(&r[6], &nodes, None, false, false);
                assert_eq!(*nodes.last().unwrap(), 8);
                (nodes[0], c)
            })
            .collect();
        let want: BTreeMap<i64, f64> = nx_map("zipf60.nx.sp.length.target.jsonl")
            .into_iter()
            .map(|(k, v)| (k, num(&v)))
            .collect();
        assert_eq!(by_source, want, "013 target");
        let rows = z
            .once(
                "gds.unweightedShortestPaths.stream",
                &[],
                record(&[("sourceNode", z.node(1)), ("targetNode", z.node(8))]),
            )
            .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0][3],
            Value::Float(num(&nx_values("zipf60.nx.sp.length.pair.jsonl")[0]))
        );
        // 014 against GDS (F costs, V paths).
        let rows = z
            .det(
                "gds.shortestPath.dijkstra.stream",
                &[],
                with(&src, "targetNode", z.node(8)),
                false,
            )
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("zipf60.gds.dijkstra.pair.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "014 pair zipf60",
            same_costs("014", &z.path_rows(&rows, true), &want, 1e-9),
        );
        let rows = z
            .det(
                "gds.allShortestPaths.dijkstra.stream",
                &[],
                src.clone(),
                false,
            )
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("zipf60.gds.dijkstra.all.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "014 all zipf60",
            same_costs("014 all", &z.path_rows(&rows, true), &want, 1e-9),
        );
        let rows = z
            .once(
                "gds.allShortestPaths.dijkstra.stream",
                &[],
                record(&[("sourceNode", z.node(1))]),
            )
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("zipf60.gds.dijkstra.all.unweighted.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        same_costs("014 unweighted", &z.path_rows(&rows, false), &want, 0.0);
        // 015 against NetworkX.
        let rows = z
            .det(
                "gds.bidirectionalDijkstra.stream",
                &[],
                with(&src, "targetNode", z.node(8)),
                false,
            )
            .await;
        let got = z.path_rows(&rows, true);
        let nx = nx_values("zipf60.nx.bidijkstra.jsonl");
        report(
            "015 zipf60",
            same_costs("015", &got, &BTreeMap::from([(8, num(&nx[0]))]), 1e-9),
        );
        // 019 against GDS (F costs, V paths, X flag); rounds against the
        // reference's hop depth.
        let rows = z
            .det("gds.bellmanFord.stream", &[], src.clone(), true)
            .await;
        assert!(rows.iter().all(|r| r[7] == Value::Bool(false)));
        let want: BTreeMap<i64, f64> = gds_paths("zipf60.gds.bellmanford.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "019 zipf60",
            same_costs("019", &z.path_rows(&rows, true), &want, 1e-9),
        );
        bellman_ford_rounds(&z, &src, "zipf60.nx.bellmanford.jsonl").await;
        // 021 against NetworkX.
        let rows = z.det("gds.spfa.stream", &[], src.clone(), false).await;
        let nx = nx_values("zipf60.nx.bellmanford.jsonl");
        report(
            "021 zipf60",
            same_costs("021", &z.path_rows(&rows, true), &num_map(&nx[0]), 1e-9),
        );
        // 023 against GDS.
        let rows = z
            .det(
                "gds.allShortestPaths.delta.stream",
                &[],
                with(&src, "delta", Value::Float(0.3)),
                true,
            )
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("zipf60.gds.delta.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "023 zipf60",
            same_costs("023", &z.path_rows(&rows, true), &want, 1e-9),
        );
        // 024 and 025 against NetworkX's floyd_warshall; 027 against GDS.
        let floyd = nx_map("zipf60.nx.floyd.jsonl");
        let weighted = record(std::slice::from_ref(&w));
        for (proc, parallel) in [
            ("gds.johnson.stream", true),
            ("gds.floydWarshall.stream", false),
        ] {
            let rows = z.det(proc, &[], weighted.clone(), parallel).await;
            report(
                &format!("{proc} zipf60"),
                check_distances(&z, &rows, |s| num_map(&floyd[&s]), 1e-9),
            );
        }
        let rows = z
            .det("gds.allShortestPaths.stream", &[], weighted.clone(), true)
            .await;
        let apsp = gds_distances("zipf60.gds.apsp.jsonl");
        report(
            "027 zipf60",
            check_distances(
                &z,
                &rows,
                |s| apsp.get(&s).cloned().unwrap_or_default(),
                1e-9,
            ),
        );
        // 028 against NetworkX (X path sets).
        for (config, file) in [
            (
                record(&[("sourceNode", z.node(1)), ("targetNode", z.node(8))]),
                "zipf60.nx.allsp.jsonl",
            ),
            (
                with(&src, "targetNode", z.node(8)),
                "zipf60.nx.allsp.weighted.jsonl",
            ),
        ] {
            let weighted = file.contains("weighted");
            let rows = z
                .det(
                    "gds.allShortestPathsBetweenNodes.stream",
                    &[],
                    config,
                    false,
                )
                .await;
            check_path_set(&z, &rows, file, weighted);
        }
        // 029 against GDS (F costs in order, V paths).
        let rows = z
            .det(
                "gds.shortestPath.yens.stream",
                &[],
                with(&with(&src, "targetNode", z.node(8)), "k", int(6)),
                true,
            )
            .await;
        report("029 zipf60", check_yens(&z, &rows, "zipf60.gds.yens.jsonl"));
        // 030 against petgraph's k_shortest_path on a petgraph graph.
        check_k_shortest(&z, 1, Some(8), 3).await;
        check_k_shortest(&z, 1, None, 2).await;
        // 031 against NetworkX (X path sets) and MAGE.
        let rows = z
            .det(
                "algo.all_simple_paths",
                &[z.node(1), z.node(8), strs(&[">"]), int(7)],
                record(&[]),
                false,
            )
            .await;
        check_simple_paths(&z, &rows, "zipf60.nx.simplepaths.jsonl", false);
        check_simple_paths_mage(&z, &rows, "zipf60.mage.simplepaths.out.jsonl");
        // 032 against GDS (F cost, V path) on a cyclic graph: its DAG part.
        let rows = z
            .det("gds.dag.longestPath.stream", &[], record(&[]), true)
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("zipf60.gds.longestpath.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        same_costs("032 zipf60", &z.path_rows(&rows, false), &want, 1e-9);
        z.f.db.close().await.unwrap();
    });
}

/// Prints the largest rule-F difference of a check.
pub(super) fn report(what: &str, worst: f64) {
    eprintln!("LAB {what}: largest relative difference {worst:e}");
}

/// GDS `(sourceNodeId, targetNodeId, distance)` rows by source.
fn gds_distances(file: &str) -> BTreeMap<i64, BTreeMap<i64, f64>> {
    let mut out: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
    for m in expected(file) {
        let d = num(&m["distance"]);
        if d.is_finite() {
            out.entry(m["sourceNodeId"].as_i64().unwrap())
                .or_default()
                .insert(m["targetNodeId"].as_i64().unwrap(), d);
        }
    }
    out
}

/// Distance rows against `want(source)` under rule F; each source once
/// per target. Returns the largest relative difference.
fn check_distances(
    l: &Lab,
    rows: &[Vec<Value>],
    want: impl Fn(i64) -> BTreeMap<i64, f64>,
    t: f64,
) -> f64 {
    let mut got: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
    for r in rows {
        let Value::Float(d) = r[2] else { panic!() };
        assert!(
            got.entry(l.uid(&r[0]))
                .or_default()
                .insert(l.uid(&r[1]), d)
                .is_none()
        );
    }
    let mut worst = 0.0f64;
    let sources: BTreeSet<i64> = l.f.uids.values().copied().collect();
    for s in sources {
        let w = want(s);
        let g = got.remove(&s).unwrap_or_default();
        assert_eq!(
            g.keys().collect::<Vec<_>>(),
            w.keys().collect::<Vec<_>>(),
            "source {s}"
        );
        for (k, x) in &w {
            assert!(close(g[k], *x, t), "{s} → {k}: {} instead of {x}", g[k]);
            worst = worst.max((g[k] - x).abs() / x.abs().max(1.0));
        }
    }
    worst
}

/// ALG-028 and ALG-031: the multiset of node paths equals NetworkX's.
fn check_path_set(l: &Lab, rows: &[Vec<Value>], file: &str, weighted: bool) {
    let mut got: Vec<Vec<i64>> = rows
        .iter()
        .map(|r| {
            let nodes = l.uids(&r[4]);
            l.check_path(&r[6], &nodes, None, weighted, false);
            nodes
        })
        .collect();
    let mut want: Vec<Vec<i64>> = nx_values(file).iter().map(ints).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want, "{file}");
}

/// The node uids of a path value.
pub(super) fn path_nodes(l: &Lab, v: &Value) -> Vec<i64> {
    let Value::Path(p) = v else {
        panic!("not a path: {v:?}")
    };
    p.iter().step_by(2).map(|id| l.f.uids[id]).collect()
}

fn check_simple_paths(l: &Lab, rows: &[Vec<Value>], file: &str, undirected: bool) {
    let mut got: Vec<Vec<i64>> = rows
        .iter()
        .map(|r| {
            let nodes = path_nodes(l, &r[0]);
            l.check_path(&r[0], &nodes, None, false, undirected);
            nodes
        })
        .collect();
    let mut want: Vec<Vec<i64>> = nx_values(file).iter().map(ints).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want, "{file}");
}

fn check_simple_paths_mage(l: &Lab, rows: &[Vec<Value>], file: &str) {
    let mut got: Vec<Vec<i64>> = rows.iter().map(|r| path_nodes(l, &r[0])).collect();
    let mut want: Vec<Vec<i64>> = expected(file).iter().map(|m| ints(&m["path"])).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want, "{file}");
}

/// ALG-029: the costs in order equal GDS's (F); each path valid and
/// distinct. Returns the largest relative difference.
fn check_yens(l: &Lab, rows: &[Vec<Value>], file: &str) -> f64 {
    let want: Vec<f64> = expected(file)
        .iter()
        .map(|m| num(&m["totalCost"]))
        .collect();
    assert_eq!(rows.len(), want.len(), "{file}: k paths");
    let mut seen = BTreeSet::new();
    let mut worst = 0.0f64;
    for (r, w) in rows.iter().zip(&want) {
        let nodes = l.uids(&r[4]);
        let Value::Float(c) = r[3] else { panic!() };
        let total = l.check_path(&r[6], &nodes, None, true, false);
        assert!(close(total, c, 1e-9));
        assert!(close(c, *w, 1e-9), "{file}: {c} instead of {w}");
        worst = worst.max((c - w).abs() / w.abs().max(1.0));
        let Value::Path(p) = &r[6] else { panic!() };
        assert!(seen.insert(p.to_vec()), "{file}: a repeated path");
    }
    worst
}

/// ALG-030 against petgraph's `k_shortest_path` on a `petgraph::Graph` of
/// the fixture's edges (rule F).
async fn check_k_shortest(l: &Lab, s: i64, t: Option<i64>, k: i64) {
    use petgraph::graph::{DiGraph, NodeIndex};
    let n = l.f.ids.len();
    let mut g = DiGraph::<(), f64>::new();
    for _ in 0..n {
        g.add_node(());
    }
    for e in l.edges.values() {
        g.add_edge(
            NodeIndex::new(e.src as usize),
            NodeIndex::new(e.dst as usize),
            e.weight,
        );
    }
    let want = petgraph::algo::k_shortest_path(
        &g,
        NodeIndex::new(s as usize),
        t.map(|t| NodeIndex::new(t as usize)),
        k as usize,
        |e| *e.weight(),
    );
    let mut config = record(&[
        ("sourceNode", l.node(s)),
        ("k", int(k)),
        ("relationshipWeightProperty", txt("weight")),
    ]);
    if let Some(t) = t {
        config = with(&config, "targetNode", l.node(t));
    }
    let rows = l.det("gds.kShortestPath.stream", &[], config, false).await;
    let got: BTreeMap<i64, f64> = rows
        .iter()
        .map(|r| {
            let Value::Float(c) = r[1] else { panic!() };
            (l.uid(&r[0]), c)
        })
        .collect();
    let want: BTreeMap<i64, f64> = want
        .into_iter()
        .map(|(v, c)| (v.index() as i64, c))
        .collect();
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>()
    );
    for (v, c) in &want {
        assert!(close(got[v], *c, 1e-9), "030 node {v}");
    }
}

/// The rounds of 019's synchronous Bellman-Ford against the reference:
/// the largest number of edges on NetworkX's shortest paths, `h`; round
/// `h + 1` is the first that changes nothing.
async fn bellman_ford_rounds(l: &Lab, config: &Value, nx_file: &str) {
    crate::algo::paths::rounds::take(&l.key);
    l.once("gds.bellmanFord.stream", &[], config.clone()).await;
    let rounds = crate::algo::paths::rounds::take(&l.key).expect("rounds");
    let paths = nx_values(nx_file);
    let h = paths[1]
        .as_object()
        .unwrap()
        .values()
        .map(|p| p.as_array().unwrap().len() - 1)
        .max()
        .unwrap();
    eprintln!(
        "LAB 019 {nx_file}: {rounds} rounds; NetworkX's shortest paths have at most {h} edges"
    );
    assert_eq!(rounds, h + 1, "{nx_file}");
}

/// rmp #64 on `uniform60`: the path entries with an oracle on this graph.
#[test]
fn path_family_matches_oracles_on_uniform60() {
    current(async {
        let u = Lab::load("uniform60").await;
        let w = ("relationshipWeightProperty", txt("weight"));
        let src = record(&[("sourceNode", u.node(14)), w.clone()]);
        // 013 all pairs (X lengths).
        let rows = u
            .once("gds.unweightedShortestPaths.stream", &[], record(&[]))
            .await;
        let mut got: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
        for r in &rows {
            let nodes = u.uids(&r[4]);
            u.check_path(&r[6], &nodes, None, false, false);
            let Value::Float(c) = r[3] else { panic!() };
            got.entry(nodes[0])
                .or_default()
                .insert(*nodes.last().unwrap(), c);
        }
        for v in nx_values("uniform60.nx.sp.length.all.jsonl") {
            let s = v[0].as_i64().unwrap();
            assert_eq!(got[&s], num_map(&v[1]), "013 from {s}");
        }
        // 014 (targets and all).
        let targets = Value::List(vec![u.node(15), u.node(3), u.node(40)].into());
        let rows = u
            .once(
                "gds.shortestPath.dijkstra.stream",
                &[],
                with(&src, "targetNodes", targets),
            )
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("uniform60.gds.dijkstra.targets.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "014 targets uniform60",
            same_costs("014 targets", &u.path_rows(&rows, true), &want, 1e-9),
        );
        let rows = u
            .once("gds.allShortestPaths.dijkstra.stream", &[], src.clone())
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("uniform60.gds.dijkstra.all.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "014 all uniform60",
            same_costs("014 all", &u.path_rows(&rows, true), &want, 1e-9),
        );
        // 015.
        let rows = u
            .once(
                "gds.bidirectionalDijkstra.stream",
                &[],
                with(&src, "targetNode", u.node(15)),
            )
            .await;
        let nx = nx_values("uniform60.nx.bidijkstra.jsonl");
        report(
            "015 uniform60",
            same_costs(
                "015",
                &u.path_rows(&rows, true),
                &BTreeMap::from([(15, num(&nx[0]))]),
                1e-9,
            ),
        );
        // 019 and 023.
        let rows = u.once("gds.bellmanFord.stream", &[], src.clone()).await;
        let want: BTreeMap<i64, f64> = gds_paths("uniform60.gds.bellmanford.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "019 uniform60",
            same_costs("019", &u.path_rows(&rows, true), &want, 1e-9),
        );
        let rows = u
            .once("gds.allShortestPaths.delta.stream", &[], src.clone())
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("uniform60.gds.delta.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "023 uniform60",
            same_costs("023", &u.path_rows(&rows, true), &want, 1e-9),
        );
        // 024, 025 and 027.
        let floyd = nx_map("uniform60.nx.floyd.jsonl");
        for proc in ["gds.johnson.stream", "gds.floydWarshall.stream"] {
            let rows = u.once(proc, &[], record(std::slice::from_ref(&w))).await;
            report(
                &format!("{proc} uniform60"),
                check_distances(&u, &rows, |s| num_map(&floyd[&s]), 1e-9),
            );
        }
        let rows = u
            .once("gds.allShortestPaths.stream", &[], record(&[]))
            .await;
        let apsp = gds_distances("uniform60.gds.apsp.unweighted.jsonl");
        check_distances(
            &u,
            &rows,
            |s| apsp.get(&s).cloned().unwrap_or_default(),
            0.0,
        );
        // 026 on the undirected view (X).
        let rows = u.det("gds.seidel.stream", &[], record(&[]), false).await;
        check_seidel(&u, &rows, "uniform60.nx.apsp.length.undirected.jsonl");
        // 028 and 029.
        let rows = u
            .once(
                "gds.allShortestPathsBetweenNodes.stream",
                &[],
                record(&[("sourceNode", u.node(14)), ("targetNode", u.node(15))]),
            )
            .await;
        check_path_set(&u, &rows, "uniform60.nx.allsp.jsonl", false);
        let rows = u
            .once(
                "gds.shortestPath.yens.stream",
                &[],
                with(&with(&src, "targetNode", u.node(15)), "k", int(6)),
            )
            .await;
        report(
            "029 uniform60",
            check_yens(&u, &rows, "uniform60.gds.yens.jsonl"),
        );
        check_k_shortest(&u, 14, Some(15), 4).await;
        u.f.db.close().await.unwrap();
    });
}

/// ALG-026 against NetworkX's undirected `all_pairs_shortest_path_length`.
fn check_seidel(l: &Lab, rows: &[Vec<Value>], file: &str) {
    let want: BTreeMap<i64, BTreeMap<i64, f64>> = nx_values(file)
        .iter()
        .map(|v| (v[0].as_i64().unwrap(), num_map(&v[1])))
        .collect();
    check_distances(l, rows, |s| want.get(&s).cloned().unwrap_or_default(), 0.0);
}

/// rmp #64 on the small fixtures: negative weights and cycles (019, 020,
/// 021, 024, 025), A* on coordinates (018), Seidel on undirected graphs
/// (026), all simple paths in both directions (031), the longest path of
/// a DAG (032).
#[test]
fn path_family_matches_oracles_on_small_fixtures() {
    current(async {
        let w = ("relationshipWeightProperty", txt("weight"));
        let n = Lab::load("negw30").await;
        let src = record(&[("sourceNode", n.node(25)), w.clone()]);
        let rows = n
            .det("gds.bellmanFord.stream", &[], src.clone(), true)
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("negw30.gds.bellmanford.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "019 negw30",
            same_costs("019", &n.path_rows(&rows, true), &want, 1e-9),
        );
        bellman_ford_rounds(&n, &src, "negw30.nx.bellmanford.jsonl").await;
        let rows = n.once("gds.spfa.stream", &[], src.clone()).await;
        let nx = nx_values("negw30.nx.bellmanford.jsonl");
        report(
            "021 negw30",
            same_costs("021", &n.path_rows(&rows, true), &num_map(&nx[0]), 1e-9),
        );
        let floyd = nx_map("negw30.nx.floyd.jsonl");
        for (proc, parallel) in [
            ("gds.johnson.stream", true),
            ("gds.floydWarshall.stream", false),
        ] {
            let rows = n
                .det(proc, &[], record(std::slice::from_ref(&w)), parallel)
                .await;
            report(
                &format!("{proc} negw30"),
                check_distances(&n, &rows, |s| num_map(&floyd[&s]), 1e-9),
            );
        }
        let rows = n
            .det("gds.findNegativeCycle.stream", &[], src.clone(), false)
            .await;
        assert_eq!(
            rows.is_empty(),
            !nx_values("negw30.nx.negcycle.exists.jsonl")[0]
                .as_bool()
                .unwrap()
        );
        // Dijkstra refuses negative weights.
        let e = n
            .call("gds.allShortestPaths.dijkstra.stream", &[], &src)
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        n.f.db.close().await.unwrap();

        let c = Lab::load("negcyc30").await;
        let src = record(&[("sourceNode", c.node(25)), w.clone()]);
        let rows = c
            .det("gds.bellmanFord.stream", &[], src.clone(), true)
            .await;
        let gds = expected("negcyc30.gds.bellmanford.jsonl");
        assert!(
            gds.iter()
                .all(|m| m["isNegativeCycle"].as_bool() == Some(true))
        );
        assert!(!rows.is_empty() && rows.iter().all(|r| r[7] == Value::Bool(true)));
        for r in &rows {
            let nodes = c.uids(&r[4]);
            assert_eq!(nodes.first(), nodes.last(), "a closed cycle");
            let cost = c.check_path(&r[6], &nodes, None, true, false);
            assert!(cost < 0.0, "a negative cycle");
        }
        let rows = c
            .det("gds.findNegativeCycle.stream", &[], src.clone(), false)
            .await;
        assert!(
            nx_values("negcyc30.nx.negcycle.exists.jsonl")[0]
                .as_bool()
                .unwrap()
        );
        assert_eq!(rows.len(), 1);
        let nodes = c.uids(&rows[0][0]);
        assert!(c.check_path(&rows[0][1], &nodes, None, true, false) < 0.0);
        for proc in [
            "gds.spfa.stream",
            "gds.johnson.stream",
            "gds.floydWarshall.stream",
        ] {
            let config = if proc == "gds.spfa.stream" {
                src.clone()
            } else {
                record(std::slice::from_ref(&w))
            };
            let e = c.call(proc, &[], &config).await.unwrap_err();
            assert_eq!(e.code(), "22N11", "{proc}");
        }
        // The write mode of 019 reports the negative cycle.
        let tx = c.f.s.begin(TransactionOptions::default()).await.unwrap();
        let row = drain(
            tx.execute(
                "CALL gds.bellmanFord.write($c)",
                param("c", with(&src, "writeRelationshipType", txt("BF"))),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap()
        .remove(0);
        tx.rollback().await.unwrap();
        assert_eq!(
            row[4],
            int(0),
            "relationshipsWritten without writeNegativeCycles"
        );
        assert_eq!(row[5], Value::Bool(true), "containsNegativeCycle");
        c.f.db.close().await.unwrap();

        let g = Lab::load("geo30").await;
        for (a, b, file) in [
            (0, 2, "geo30.gds.astar.jsonl"),
            (5, 27, "geo30.gds.astar.b.jsonl"),
        ] {
            let rows = g
                .det(
                    "gds.shortestPath.astar.stream",
                    &[],
                    record(&[
                        ("sourceNode", g.node(a)),
                        ("targetNode", g.node(b)),
                        ("latitudeProperty", txt("lat")),
                        ("longitudeProperty", txt("lng")),
                        w.clone(),
                    ]),
                    false,
                )
                .await;
            let want: BTreeMap<i64, f64> =
                gds_paths(file).into_iter().map(|(k, v)| (k, v.1)).collect();
            report(
                &format!("018 {file}"),
                same_costs("018", &g.path_rows(&rows, true), &want, 1e-9),
            );
        }
        g.f.db.close().await.unwrap();

        let k = Lab::load("karate").await;
        let rows = k.det("gds.seidel.stream", &[], record(&[]), false).await;
        check_seidel(&k, &rows, "karate.nx.apsp.length.jsonl");
        k.f.db.close().await.unwrap();

        let y = Lab::load("cyc12").await;
        let rows = y.once("gds.seidel.stream", &[], record(&[])).await;
        check_seidel(&y, &rows, "cyc12.nx.apsp.length.jsonl");
        let rows = y
            .det(
                "algo.all_simple_paths",
                &[y.node(10), y.node(8), strs(&[]), int(4)],
                record(&[]),
                false,
            )
            .await;
        check_simple_paths(&y, &rows, "cyc12.nx.simplepaths.undirected.jsonl", true);
        check_simple_paths_mage(&y, &rows, "cyc12.mage.simplepaths.any.jsonl");
        // 004 path expansion against MAGE (X: paths as node sequences).
        let n5 = y.node(5);
        for (rels, labels, min, max, file) in [
            (strs(&[]), strs(&[]), 0, 3, "cyc12.mage.expand.all.jsonl"),
            (
                strs(&["R>", "<S"]),
                strs(&["+A", "-X"]),
                1,
                3,
                "cyc12.mage.expand.filtered.jsonl",
            ),
            (
                strs(&["<R>", "S"]),
                strs(&[">B"]),
                1,
                3,
                "cyc12.mage.expand.both.jsonl",
            ),
        ] {
            let rows = y
                .det(
                    "gds.pathExpand.stream",
                    &[],
                    record(&[
                        ("start", n5.clone()),
                        ("relationships", rels),
                        ("labels", labels),
                        ("minHops", int(min)),
                        ("maxHops", int(max)),
                    ]),
                    false,
                )
                .await;
            let mut got: Vec<Vec<i64>> = rows
                .iter()
                .map(|r| {
                    let nodes = path_nodes(&y, &r[0]);
                    y.check_path(&r[0], &nodes, None, false, true);
                    nodes
                })
                .collect();
            let mut want: Vec<Vec<i64>> =
                expected(file).iter().map(|m| ints(&m["result"])).collect();
            got.sort();
            want.sort();
            assert_eq!(got, want, "{file}");
        }
        y.f.db.close().await.unwrap();

        let d = Lab::load("dag40").await;
        let rows = d
            .det(
                "gds.dag.longestPath.stream",
                &[],
                record(std::slice::from_ref(&w)),
                true,
            )
            .await;
        let want: BTreeMap<i64, f64> = gds_paths("dag40.gds.longestpath.jsonl")
            .into_iter()
            .map(|(k, v)| (k, v.1))
            .collect();
        report(
            "032 dag40",
            same_costs("032", &d.path_rows(&rows, true), &want, 1e-9),
        );
        d.f.db.close().await.unwrap();
    });
}

/// rmp #64 (ALG-006, rule Q): every walk is a walk of the projection,
/// `walkLength` nodes long or ending at a node without outgoing edges;
/// `walksPerNode` walks per start node; over 10 000 walks the first-step
/// frequencies of each start node are within 4 standard errors of its
/// weighted out-edge probabilities; bit-equal at 1 to 16 with two seeds.
#[test]
fn random_walk_quality() {
    current(async {
        let z = Lab::load("zipf60").await;
        for seed in [0, 7] {
            let rows = z
                .det(
                    "gds.randomWalk.stream",
                    &[],
                    record(&[
                        ("randomSeed", int(seed)),
                        ("walkLength", int(6)),
                        ("walksPerNode", int(2)),
                        ("returnFactor", Value::Float(0.5)),
                        ("inOutFactor", Value::Float(2.0)),
                        ("relationshipWeightProperty", txt("weight")),
                    ]),
                    true,
                )
                .await;
            assert_eq!(rows.len(), 120);
            for r in &rows {
                let nodes = z.uids(&r[0]);
                z.check_path(&r[1], &nodes, None, true, false);
                let last = *nodes.last().unwrap();
                assert!(nodes.len() == 6 || !z.edges.values().any(|e| e.src == last));
            }
        }
        // First-step frequencies from three start nodes.
        let starts = [25, 26, 24];
        let rows = z
            .once(
                "gds.randomWalk.stream",
                &[],
                record(&[
                    (
                        "sourceNodes",
                        Value::List(starts.iter().map(|s| z.node(*s)).collect::<Vec<_>>().into()),
                    ),
                    ("walkLength", int(2)),
                    ("walksPerNode", int(10_000)),
                    ("relationshipWeightProperty", txt("weight")),
                ]),
            )
            .await;
        assert_eq!(rows.len(), 30_000);
        let mut worst = 0.0f64;
        for s in starts {
            let mut count: BTreeMap<i64, f64> = BTreeMap::new();
            let mut total = 0.0;
            for r in &rows {
                let nodes = z.uids(&r[0]);
                if nodes[0] == s {
                    *count.entry(nodes[1]).or_default() += 1.0;
                    total += 1.0;
                }
            }
            let mut weight: BTreeMap<i64, f64> = BTreeMap::new();
            for e in z.edges.values().filter(|e| e.src == s) {
                *weight.entry(e.dst).or_default() += e.weight;
            }
            let sum: f64 = weight.values().sum();
            for (t, w) in &weight {
                let p = w / sum;
                let se = (p * (1.0 - p) / total).sqrt();
                let f = count.get(t).copied().unwrap_or(0.0) / total;
                let z_score = (f - p).abs() / se.max(1e-12);
                worst = worst.max(z_score);
                assert!(
                    z_score <= 4.0,
                    "start {s} → {t}: frequency {f}, probability {p}"
                );
            }
        }
        eprintln!("LAB 006: largest first-step deviation {worst:.2} standard errors");
        z.f.db.close().await.unwrap();
    });
}

/// rmp #64 (FR-ALG-006): on a 2-block projection each parallel entry
/// starts more than one and at most `concurrency` workers; each other
/// entry one worker (or none: the statement's thread) at `concurrency`
/// 16.
#[test]
fn worker_counts_of_the_family() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: f.db.engine_handle().core().location.display().to_string(),
            edges: BTreeMap::new(),
            f,
        };
        // Two nodes, one edge: a 2-block projection.
        crate::test_support::algo::rows(
            &l.f.s,
            "INSERT (:Two {uid: 100})-[:T {weight: 1}]->(:Two {uid: 101})",
            Parameters::new(),
        )
        .await
        .unwrap();
        let ids: Vec<Value> = rows(
            &l.f.s,
            "MATCH (n:Two) RETURN n, n.uid AS u ORDER BY u",
            Parameters::new(),
        )
        .await
        .unwrap()
        .into_iter()
        .map(|r| r[0].clone())
        .collect();
        let (a, b) = (ids[0].clone(), ids[1].clone());
        let base = record(&[("nodeLabels", txt("Two")), ("relationshipTypes", txt("T"))]);
        let w = ("relationshipWeightProperty", txt("weight"));
        let cases: Vec<(&str, Vec<Value>, Value, bool)> = vec![
            (
                "gds.bfs.stream",
                vec![],
                record(&[("sourceNode", a.clone())]),
                true,
            ),
            (
                "gds.dfs.stream",
                vec![],
                record(&[("sourceNode", a.clone())]),
                false,
            ),
            (
                "neighbors.at_hop",
                vec![a.clone(), strs(&[]), int(1)],
                record(&[]),
                false,
            ),
            (
                "neighbors.by_hop",
                vec![a.clone(), strs(&[]), int(1)],
                record(&[]),
                false,
            ),
            (
                "gds.pathExpand.stream",
                vec![],
                record(&[
                    ("start", a.clone()),
                    ("relationships", strs(&[])),
                    ("labels", strs(&[])),
                    ("minHops", int(0)),
                    ("maxHops", int(1)),
                ]),
                false,
            ),
            (
                "gds.pathSubgraphNodes.stream",
                vec![],
                record(&[("startNode", a.clone())]),
                false,
            ),
            (
                "gds.pathSubgraphAll.stream",
                vec![],
                record(&[("startNode", a.clone())]),
                false,
            ),
            (
                "gds.randomWalk.stream",
                vec![],
                record(&[("walkLength", int(2))]),
                true,
            ),
            ("gds.dag.topologicalSort.stream", vec![], record(&[]), true),
            ("graph_util.ancestors", vec![b.clone()], record(&[]), false),
            (
                "graph_util.descendants",
                vec![a.clone()],
                record(&[]),
                false,
            ),
            (
                "gds.hasPathConnecting.stream",
                vec![],
                record(&[("sourceNode", a.clone()), ("targetNode", b.clone())]),
                false,
            ),
            (
                "gds.dominatorsSimpleFast.stream",
                vec![],
                record(&[("sourceNode", a.clone())]),
                false,
            ),
            (
                "gds.unweightedShortestPaths.stream",
                vec![],
                record(&[("sourceNode", a.clone())]),
                false,
            ),
            (
                "gds.shortestPath.dijkstra.stream",
                vec![],
                record(&[
                    ("sourceNode", a.clone()),
                    ("targetNode", b.clone()),
                    w.clone(),
                ]),
                false,
            ),
            (
                "gds.allShortestPaths.dijkstra.stream",
                vec![],
                record(&[("sourceNode", a.clone()), w.clone()]),
                false,
            ),
            (
                "gds.bidirectionalDijkstra.stream",
                vec![],
                record(&[("sourceNode", a.clone()), ("targetNode", b.clone())]),
                false,
            ),
            (
                "gds.bellmanFord.stream",
                vec![],
                record(&[("sourceNode", a.clone()), w.clone()]),
                true,
            ),
            (
                "gds.findNegativeCycle.stream",
                vec![],
                record(&[("sourceNode", a.clone()), w.clone()]),
                false,
            ),
            (
                "gds.spfa.stream",
                vec![],
                record(&[("sourceNode", a.clone()), w.clone()]),
                false,
            ),
            (
                "gds.allShortestPaths.delta.stream",
                vec![],
                record(&[("sourceNode", a.clone()), w.clone()]),
                true,
            ),
            (
                "gds.johnson.stream",
                vec![],
                record(std::slice::from_ref(&w)),
                true,
            ),
            (
                "gds.floydWarshall.stream",
                vec![],
                record(std::slice::from_ref(&w)),
                false,
            ),
            ("gds.seidel.stream", vec![], record(&[]), false),
            ("gds.allShortestPaths.stream", vec![], record(&[]), true),
            (
                "gds.allShortestPathsBetweenNodes.stream",
                vec![],
                record(&[("sourceNode", a.clone()), ("targetNode", b.clone())]),
                false,
            ),
            (
                "gds.shortestPath.yens.stream",
                vec![],
                record(&[
                    ("sourceNode", a.clone()),
                    ("targetNode", b.clone()),
                    ("k", int(2)),
                ]),
                true,
            ),
            (
                "gds.kShortestPath.stream",
                vec![],
                record(&[("sourceNode", a.clone()), ("k", int(1))]),
                false,
            ),
            (
                "algo.all_simple_paths",
                vec![a.clone(), b.clone(), strs(&[]), int(2)],
                record(&[]),
                false,
            ),
            ("gds.dag.longestPath.stream", vec![], record(&[]), true),
        ];
        let mut parallel_count = 0;
        for (proc, args, config, parallel) in &cases {
            let e = super::ENTRIES.iter().find(|e| e.stream == *proc).unwrap();
            assert_eq!(e.parallel, *parallel, "{proc}: classification");
            let mut config = config.clone();
            if let Value::Record(r) = &base {
                for (k, v) in r.iter() {
                    config = with(&config, k, v.clone());
                }
            }
            let at = if *parallel { 4 } else { 16 };
            crate::algo::sched::probe::take(&l.key);
            l.call(proc, args, &with(&config, "concurrency", int(at)))
                .await
                .unwrap_or_else(|e| panic!("{proc}: {e}"));
            let w = crate::algo::sched::probe::take(&l.key);
            if *parallel {
                parallel_count += 1;
                assert_eq!(w, Some(2), "{proc}: workers on 2 blocks at concurrency 4");
            } else {
                assert!(
                    w.is_none() || w == Some(1),
                    "{proc}: {w:?} workers at concurrency 16"
                );
            }
        }
        assert_eq!(
            parallel_count, 9,
            "parallel entries of #64: 001, 006, 007, 019, 023, 024, 027, 029, 032"
        );
        assert_eq!(CONCURRENCY, 1..=16);
        l.f.db.close().await.unwrap();
    });
}

/// rmp #64 (FR-LIM-001): ALG-028 and ALG-031 on a graph whose output
/// exceeds a small transaction memory limit fail with 51N73 and abort the
/// transaction; with a larger limit they succeed and their rows exceed the
/// small one; the accountant's peak never exceeds the cap.
#[test]
fn exponential_outputs_respect_the_memory_cap() {
    current(async {
        let f = Fixture::load("cyc12", CAP).await;
        let l = Lab {
            key: String::new(),
            edges: BTreeMap::new(),
            f,
        };
        // A layered graph: 8 layers of 4 nodes, every node joined to every
        // node of the next layer: 4^6 = 4 096 shortest (and simple) paths
        // from a node of the first layer to one of the last.
        let nodes: Vec<String> = (0..32)
            .map(|i| format!("(:L {{uid: {}, layer: {}}})", 1000 + i, i / 4))
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
            "MATCH (a:L), (b:L) WHERE b.layer = a.layer + 1 INSERT (a)-[:N]->(b)",
            Parameters::new(),
        )
        .await
        .unwrap();
        let ends = rows(
            &l.f.s,
            "MATCH (a:L {uid: 1000}), (b:L {uid: 1031}) RETURN a, b",
            Parameters::new(),
        )
        .await
        .unwrap()
        .remove(0);
        let base = record(&[("nodeLabels", txt("L")), ("relationshipTypes", txt("N"))]);
        let calls: Vec<(&str, Vec<Value>, Value)> = vec![
            (
                "gds.allShortestPathsBetweenNodes.stream",
                vec![],
                with(
                    &with(&base, "sourceNode", ends[0].clone()),
                    "targetNode",
                    ends[1].clone(),
                ),
            ),
            (
                "algo.all_simple_paths",
                vec![ends[0].clone(), ends[1].clone(), strs(&[">"]), int(10)],
                base.clone(),
            ),
        ];
        let small = 1 << 19;
        for (proc, args, config) in &calls {
            let ok = l.once(proc, args, config.clone()).await;
            assert_eq!(ok.len(), 4_096, "{proc}");
            let bytes: u64 = ok.iter().map(|r| crate::query::exec::row_bytes(r)).sum();
            assert!(bytes > small, "{proc}: {bytes} bytes of rows");
            let mut limits = l.f.s.limits();
            limits.memory = Some(small);
            l.f.s.set_limits(limits);
            let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
            let mut ps = Parameters::new();
            let mut list = Vec::new();
            for (i, a) in args.iter().enumerate() {
                ps.insert(format!("a{i}"), a.clone());
                list.push(format!("$a{i}"));
            }
            ps.insert("c".into(), config.clone());
            list.push("$c".into());
            let e = match tx
                .execute(&format!("CALL {proc}({})", list.join(", ")), ps)
                .await
            {
                Err(e) => e,
                Ok(st) => drain(st).await.expect_err("over the limit"),
            };
            assert_eq!(e.code(), "51N73", "{proc}: {e}");
            let e2 = tx
                .execute("MATCH (n) RETURN n", Parameters::new())
                .await
                .unwrap_err();
            assert_eq!(e2.code(), "25N05");
            let _ = tx.rollback().await;
            limits.memory = None;
            l.f.s.set_limits(limits);
            eprintln!(
                "LAB {proc}: 4 096 rows, {bytes} B of rows; a {small} B limit fails with 51N73"
            );
        }
        let engine = l.f.db.engine_handle();
        let acc = engine.accountant();
        assert!(
            acc.peak() <= acc.cap(),
            "peak {} over cap {}",
            acc.peak(),
            acc.cap()
        );
        l.f.db.close().await.unwrap();
    });
}

/// rmp #64 (FR-ALG-003, FR-LIM-007): the write modes of the family write
/// what the stream gives, in a transaction that rolls back.
#[test]
fn write_modes_of_the_family() {
    current(async {
        let z = Lab::load("zipf60").await;
        let w = ("relationshipWeightProperty", txt("weight"));
        let tx = z.f.s.begin(TransactionOptions::default()).await.unwrap();
        let run = |q: &'static str, c: Value| {
            let tx = &tx;
            async move {
                drain(tx.execute(q, param("c", c)).await.unwrap())
                    .await
                    .unwrap()
            }
        };
        // PathWrite: one relationship per path, with its costs.
        let row = run(
            "CALL gds.allShortestPaths.dijkstra.write($c)",
            record(&[
                ("sourceNode", z.node(1)),
                w.clone(),
                ("writeRelationshipType", txt("SP")),
                ("writeNodeIds", Value::Bool(true)),
                ("writeCosts", Value::Bool(true)),
            ]),
        )
        .await
        .remove(0);
        assert_eq!(row[4], int(47), "relationshipsWritten");
        let written = drain(
            tx.execute(
                "MATCH (a)-[e:SP]->(b) RETURN a.uid AS a, b.uid AS b, e.totalCost AS c, e.nodeIds AS n, e.costs AS k",
                Parameters::new(),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(written.len(), 47);
        let want = gds_paths("zipf60.gds.dijkstra.all.jsonl");
        for r in &written {
            let (Value::Int(b), Value::Float(c)) = (&r[1], &r[2]) else {
                panic!()
            };
            assert_eq!(r[0], int(1));
            assert!(close(*c, want[b].1, 1e-9));
            assert!(matches!(&r[3], Value::List(l) if !l.is_empty()));
            assert!(matches!(&r[4], Value::List(l) if !l.is_empty()));
        }
        // NodeWrite: 011 and 030.
        let row = run(
            "CALL gds.dominatorsSimpleFast.write($c)",
            record(&[("sourceNode", z.node(1)), ("writeProperty", txt("idom"))]),
        )
        .await
        .remove(0);
        assert_eq!(row[0], int(47), "nodePropertiesWritten");
        let row = run(
            "CALL gds.kShortestPath.write($c)",
            record(&[
                ("sourceNode", z.node(1)),
                ("k", int(1)),
                w.clone(),
                ("writeProperty", txt("ksp")),
                ("relationshipTypes", strs(&["R", "S"])),
            ]),
        )
        .await
        .remove(0);
        assert_eq!(row[0], int(47));
        let n = drain(
            tx.execute(
                "MATCH (n) WHERE n.ksp IS NOT NULL RETURN count(*) AS c",
                Parameters::new(),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(n[0][0], int(47));
        tx.rollback().await.unwrap();
        let n = rows(
            &z.f.s,
            "MATCH ()-[e:SP]->() RETURN count(*) AS c",
            Parameters::new(),
        )
        .await
        .unwrap();
        assert_eq!(n[0][0], int(0), "rolled back");
        // Modes and signatures: no write mode where #62 gives none; MAGE
        // entries have none.
        for missing in [
            "gds.bfs.write",
            "gds.johnson.write",
            "algo.all_simple_paths.write",
            "neighbors.at_hop.write",
        ] {
            let e = rows(
                &z.f.s,
                &format!("CALL {missing}($c)"),
                param("c", record(&[])),
            )
            .await
            .unwrap_err();
            assert_eq!(e.code(), "42N08", "{missing}");
        }
        let e = z
            .call("gds.bfs.stream", &[], &record(&[]))
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11", "sourceNode is required");
        let e = z
            .call(
                "neighbors.at_hop",
                &[z.node(1), int(3), int(1)],
                &record(&[]),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22G03", "rel_type is a list");
        z.f.db.close().await.unwrap();
    });
}

/// Laboratory record (rmp #64): the accountant's peak of the family's
/// entries on the Zipf fixture of 300 nodes; run under
/// `docs/evidence/lab-sprint-5/heaptrack.sh` for the heap peak.
#[test]
#[ignore = "laboratory measurement"]
fn lab_family64_memory() {
    current(async {
        let z = Lab::load("zipf300").await;
        let w = ("relationshipWeightProperty", txt("weight"));
        let s = record(&[("sourceNode", z.node(0)), w.clone()]);
        let before = z.f.db.engine_handle().accountant().peak();
        for (proc, config) in [
            ("gds.bfs.stream", record(&[("sourceNode", z.node(0))])),
            ("gds.allShortestPaths.dijkstra.stream", s.clone()),
            ("gds.bellmanFord.stream", s.clone()),
            ("gds.allShortestPaths.delta.stream", s.clone()),
            ("gds.johnson.stream", record(std::slice::from_ref(&w))),
            ("gds.floydWarshall.stream", record(std::slice::from_ref(&w))),
            (
                "gds.allShortestPaths.stream",
                record(std::slice::from_ref(&w)),
            ),
            ("gds.seidel.stream", record(&[])),
            ("gds.dag.longestPath.stream", record(&[])),
            ("gds.randomWalk.stream", record(&[])),
        ] {
            let n = z.once(proc, &[], config).await.len();
            let engine = z.f.db.engine_handle();
            let acc = engine.accountant();
            eprintln!(
                "LAB {proc}: {n} rows; accountant peak {} B (cap {} B)",
                acc.peak(),
                acc.cap()
            );
            assert!(acc.peak() <= acc.cap());
        }
        eprintln!("LAB accountant peak before the family: {before} B");
        z.f.db.close().await.unwrap();
    });
}
