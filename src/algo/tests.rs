//! Tests of the algorithm runtime and its pilot entry, PageRank (rmp #61,
//! #63; FR-ALG-002 to FR-ALG-007, FR-LIM-001, FR-LIM-007). Graph data
//! lives under `/tmp/lpgram` (CLAUDE.md); oracle outputs are committed
//! under `src/algo/testdata/expected` ([`crate::test_support::algo`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use super::config::{Config, SEED};
use super::mem::Mem;
use super::projection::{self, Any, Orientation, Projection, Spec};
use super::sched::{self, Sched, probe};
use super::{AlgoMode, ENTRIES, Run, pagerank};
use crate::error::Error;
use crate::proc::{ReadContext, ReadProcedure, Rows, Signature};
use crate::test_support::algo::{
    AllowAll, Case, Fixture, Rule, check_cases, compare, determinism, drain, expected,
    expected_by_key, first_difference, param, record, rows, with,
};
use crate::test_support::{OWNER, graph_dir, no_flush};
use crate::types::Value;
use crate::{Database, Options, Parameters, Session, TransactionOptions};

/// The memory cap of these tests.
const CAP: u64 = 32 << 20;

fn current<F: std::future::Future<Output = ()>>(f: F) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a runtime")
        .block_on(f);
}

fn int(i: i64) -> Value {
    Value::Int(i)
}

fn txt(x: &str) -> Value {
    Value::string(x)
}

fn empty() -> Value {
    record(&[])
}

async fn new_db(cap: u64) -> (tempfile::TempDir, Database, Session) {
    let dir = graph_dir();
    let db = Database::create(
        dir.path(),
        no_flush(),
        cap,
        OWNER,
        Options::new(Arc::new(AllowAll)),
    )
    .await
    .expect("create");
    let s = db.session(OWNER, None, None).await.expect("session");
    (dir, db, s)
}

async fn exec(s: &Session, gql: &str) {
    rows(s, gql, Parameters::new())
        .await
        .unwrap_or_else(|e| panic!("{gql}: {e}"));
}

fn probe_key(db: &Database) -> String {
    db.engine_handle().core().location.display().to_string()
}

/// rmp #61: the determinism harness passes a closure that adds floats in
/// block order, and fails a closure whose addition order depends on
/// `concurrency`, naming the `concurrency` value and the row.
#[test]
fn determinism_harness_detects_order_dependence() {
    let xs: Vec<f64> = (0..1000).map(|i| 1.0 / f64::from(i + 1)).collect();
    let block_ordered = |c: usize| {
        let sched = Sched::new(xs.len(), c, true, None);
        let partial = sched.map(|_, r| xs[r].iter().sum::<f64>()).unwrap();
        vec![vec![Value::Float(partial.iter().sum())]]
    };
    assert!(determinism(block_ordered).is_ok());
    // One partial sum per worker: the order of additions depends on c.
    let per_worker = |c: usize| {
        let chunk = xs.len().div_ceil(c);
        let partial: Vec<f64> = xs.chunks(chunk).map(|p| p.iter().sum()).collect();
        vec![vec![int(0)], vec![Value::Float(partial.iter().sum())]]
    };
    let m = determinism(per_worker).unwrap_err();
    assert!(m.concurrency > 1 && m.row == 1, "{m}");
    let text = m.to_string();
    assert!(text.contains(&format!("concurrency {}", m.concurrency)) && text.contains("row 1"));
    assert_eq!(
        first_difference(&[vec![int(1)]], &[vec![int(1)], vec![int(2)]]).map(|d| d.0),
        Some(1)
    );
}

/// rmp #61: the worker-count probe reports k for a call that starts k
/// scoped threads, for k = 1, 4 and 16; the barrier keeps all k alive at
/// once, so they are k distinct threads.
#[test]
fn worker_count_probe_counts_threads() {
    for k in [1, 4, 16] {
        let key = format!("probe-self-test-{k}");
        let barrier = std::sync::Barrier::new(k);
        sched::workers(k, Some(&key), |_| {
            barrier.wait();
        })
        .unwrap();
        assert_eq!(probe::take(&key), Some(k));
        assert_eq!(probe::take(&key), None);
    }
}

/// The parameter record: the common keys, `randomSeed`, the entry's and
/// `writeProperty`, in that order; the seed key defaults to 0.
#[test]
fn parameter_record_of_an_entry() {
    let e = &pagerank::ENTRY;
    let keys = super::keys(e, AlgoMode::Write);
    let names: Vec<&str> = keys.iter().map(|k| k.name).collect();
    assert_eq!(
        &names[..4],
        [
            "nodeLabels",
            "relationshipTypes",
            "concurrency",
            "orientation"
        ]
    );
    assert_eq!(names.last(), Some(&"writeProperty"));
    assert!(!names.contains(&"randomSeed"));
    let c = Config::parse(&empty(), &[SEED]).unwrap();
    assert_eq!(c.seed(), 0);
    let c = Config::parse(&record(&[("randomSeed", int(-1))]), &[SEED]).unwrap();
    assert_eq!(c.seed(), u64::MAX);
    // Every entry is registered under its stream and write names.
    let r = crate::proc::Registry::with_builtins();
    for e in ENTRIES {
        assert!(r.get(e.stream).is_some());
        assert_eq!(
            e.write.is_some_and(|w| r.get(w).is_some()),
            e.write.is_some()
        );
    }
}

/// A read-only procedure that builds a projection of the working graph in
/// the caller's transaction and hands it to the test.
struct Probe {
    spec: Spec,
    orientation: Orientation,
    out: Mutex<Option<Projection<u32>>>,
    /// The PageRank outputs on the `u32` and the `usize` projection.
    ranks: Mutex<Vec<super::Output>>,
    /// The heap peak of the build on the statement's thread
    /// (`allocation-counter`).
    heap: Mutex<u64>,
    /// The bytes the run reserved once the projection was built.
    reserved: Mutex<u64>,
}

impl ReadProcedure for Probe {
    fn call(&self, cx: &mut ReadContext<'_>, _: &[Value]) -> Result<Rows, Error> {
        let g = cx.st.graphs[cx.graph];
        let mut mem = Mem::new(cx.tx, cx.st);
        let mut built = None;
        let info = allocation_counter::measure(|| {
            built = Some(projection::build(
                &mut mem,
                &g,
                &self.spec,
                self.orientation,
            ));
        });
        *self.heap.lock().unwrap() = info.bytes_max;
        *self.reserved.lock().unwrap() = mem.reserved();
        let Any::Small(p) = built.expect("built")? else {
            panic!("a small projection");
        };
        // PageRank on the same projection with `usize` indexes.
        let wide = Projection::<usize> {
            ids: p.ids.clone(),
            fwd: projection::Csr {
                row: p.fwd.row.clone(),
                col: p.fwd.col.iter().map(|x| *x as usize).collect(),
            },
            rev: projection::Csr {
                row: p.rev.row.clone(),
                col: p.rev.col.iter().map(|x| *x as usize).collect(),
            },
            edge_ids: p.edge_ids.clone(),
            rev_slot: p.rev_slot.clone(),
            edge_props: p.edge_props.clone(),
            node_props: p.node_props.clone(),
            edge_types: p.edge_types.clone(),
            labels: p.labels.clone(),
            orientation: p.orientation,
        };
        let cfg = Config::parse(&empty(), &super::keys(&pagerank::ENTRY, AlgoMode::Stream))?;
        let mut outs = Vec::new();
        for any in [Any::Small(p.clone()), Any::Large(wide)] {
            let sched = Sched::new(p.ids.len(), 4, true, None);
            outs.push((pagerank::ENTRY.run)(&mut Run {
                graph: &any,
                cfg: &cfg,
                sched: &sched,
                mem: &mut mem,
                args: &[],
                other: None,
            })?);
        }
        *self.ranks.lock().unwrap() = outs;
        *self.out.lock().unwrap() = Some(p);
        Ok(Box::new(std::iter::empty()))
    }
}

/// Builds the projection of `spec` in `s`'s database through [`Probe`].
async fn project(db: &Database, s: &Session, spec: Spec, orientation: Orientation) -> Arc<Probe> {
    let p = Arc::new(Probe {
        spec,
        orientation,
        out: Mutex::new(None),
        ranks: Mutex::new(Vec::new()),
        heap: Mutex::new(0),
        reserved: Mutex::new(0),
    });
    let name = format!("algo_probe_{}", Arc::as_ptr(&p) as usize);
    db.register_procedure(Signature::new(&name, vec![], &[]), p.clone())
        .unwrap();
    exec(s, &format!("CALL {name}()")).await;
    p
}

/// rmp #63: a multigraph projection keeps every parallel edge; its
/// forward and reverse rows equal the adjacency the graph's edges give;
/// orientation views; label, type and property arrays; petgraph's
/// `toposort`, `kosaraju_scc` and `dinics` run on it; PageRank gives the
/// same output on `u32` and `usize` indexes.
#[test]
fn projection_keeps_the_multigraph_and_runs_petgraph() {
    current(async {
        let (_d, db, s) = new_db(CAP).await;
        // a→b twice (parallel), a→c, b→c, c→c (self-loop), d isolated;
        // an edge of type T and a node e without label A.
        rows(
            &s,
            "INSERT (a:A {name: 'a', k: 1, w: 'x'}), (b:A {name: 'b', k: $f}), (c:A {name: 'c'}), \
             (d:A {name: 'd'}), (e:B {name: 'e'}), \
             (a)-[:R {cap: 3}]->(b), (a)-[:R {cap: 4}]->(b), (a)-[:R {cap: 1}]->(c), \
             (b)-[:R {cap: 5}]->(c), (c)-[:R]->(c), (a)-[:T]->(c), (e)-[:R]->(a)",
            param("f", Value::Float(2.5)),
        )
        .await
        .unwrap();
        let ids: BTreeMap<String, u64> =
            rows(&s, "MATCH (n) RETURN n, n.name AS name", Parameters::new())
                .await
                .unwrap()
                .into_iter()
                .map(|r| match (&r[0], &r[1]) {
                    (Value::Node(id), Value::String(name)) => (name.to_string(), *id),
                    _ => panic!("{r:?}"),
                })
                .collect();
        let spec = Spec {
            labels: Some(vec!["A".into()]),
            types: Some(vec!["R".into()]),
            edge_props: vec!["cap".into()],
            node_props: vec!["k".into(), "w".into()],
            ..Spec::default()
        };
        let probe = project(&db, &s, spec.clone(), Orientation::Natural).await;
        let p = probe.out.lock().unwrap().take().unwrap();
        let ix = |n: &str| p.dense(ids[n]).unwrap();
        assert_eq!(p.node_count(), 4, "e has no label A");
        assert_eq!(p.dense(ids["e"]), None);
        // The edges a→b ×2, a→c, b→c, c→c of type R between A nodes.
        let mut fwd: Vec<(usize, usize, f64)> = (0..4)
            .flat_map(|v| p.out(v).map(move |(t, slot)| (v, t as usize, slot)))
            .map(|(v, t, slot)| (v, t, p.edge_prop(0, slot)))
            .collect();
        fwd.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let (a, b, c) = (ix("a"), ix("b"), ix("c"));
        let mut want = vec![
            (a, b, 3.0),
            (a, b, 4.0),
            (a, c, 1.0),
            (b, c, 5.0),
            (c, c, f64::NAN),
        ];
        want.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.total_cmp(&y.2)));
        assert_eq!(fwd.len(), want.len());
        for (g, w) in fwd.iter().zip(&want) {
            assert_eq!((g.0, g.1), (w.0, w.1));
            assert!(g.2 == w.2 || (g.2.is_nan() && w.2.is_nan()));
        }
        // Reverse rows are the same edges seen from their targets.
        let mut rev: Vec<(usize, usize)> = (0..4)
            .flat_map(|v| p.inc(v).map(move |(u, _)| (u as usize, v)))
            .collect();
        rev.sort_unstable();
        let mut fwd_pairs: Vec<(usize, usize)> = fwd.iter().map(|e| (e.0, e.1)).collect();
        fwd_pairs.sort_unstable();
        assert_eq!(rev, fwd_pairs);
        // Every reverse entry points at the forward slot of the same edge.
        for v in 0..4 {
            for (u, slot) in p.inc(v) {
                assert!(p.out(u as usize).any(|(t, s)| t as usize == v && s == slot));
            }
        }
        // Node property arrays: numbers, else NaN.
        assert_eq!(p.node_props[0][a], 1.0);
        assert_eq!(p.node_props[0][b], 2.5);
        assert!(p.node_props[0][c].is_nan() && p.node_props[1][a].is_nan());
        // Orientation views: REVERSE swaps, UNDIRECTED sees both ends and
        // the self-loop twice.
        let mut r = p.clone();
        r.orientation = Orientation::Reverse;
        assert_eq!(r.out_degree(c), p.inc(c).count());
        let mut u = p.clone();
        u.orientation = Orientation::Undirected;
        assert_eq!(u.out_degree(c), 4, "a→c, b→c and c→c twice");
        assert_eq!(u.out_degree(a), 3);
        // petgraph functions compile and run on the projection.
        let dag = {
            let mut q = p.clone();
            // Without the self-loop the graph is acyclic.
            q = drop_self_loops(&q);
            q
        };
        let order = petgraph::algo::toposort(&dag, None).unwrap();
        assert_eq!(order.len(), 4);
        assert!(
            petgraph::algo::toposort(&p, None).is_err(),
            "the self-loop is a cycle"
        );
        let scc = petgraph::algo::kosaraju_scc(&p);
        assert_eq!(scc.len(), 4);
        let (flow, _) = petgraph::algo::dinics(&dag, a as u32, c as u32);
        assert_eq!(flow, 1.0 + 5.0, "a→c (1) and a→b→c (min(3 + 4, 5))");
        // Edge-type filter: with every type, the T edge joins.
        let all = project(
            &db,
            &s,
            Spec {
                types: None,
                ..spec
            },
            Orientation::Natural,
        )
        .await;
        assert_eq!(all.out.lock().unwrap().as_ref().unwrap().edge_ids.len(), 6);
        // PageRank: the same output on u32 and usize indexes.
        let outs = probe.ranks.lock().unwrap().clone();
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0], outs[1]);
        db.close().await.unwrap();
    });
}

/// `p` without its self-loops, rebuilt from its forward rows.
fn drop_self_loops(p: &Projection<u32>) -> Projection<u32> {
    let n = p.node_count();
    let mut fwd = projection::Csr {
        row: vec![0],
        col: Vec::new(),
    };
    let mut edges = Vec::new();
    let mut props = Vec::new();
    for v in 0..n {
        for (t, slot) in p.out(v) {
            if t as usize != v {
                fwd.col.push(t);
                edges.push((v, t as usize, fwd.col.len() - 1));
                props.push(p.edge_props[0][slot]);
            }
        }
        fwd.row.push(fwd.col.len());
    }
    let mut rev = projection::Csr {
        row: vec![0],
        col: Vec::new(),
    };
    let mut rev_slot = Vec::new();
    for v in 0..n {
        for (u, _, slot) in edges.iter().filter(|e| e.1 == v) {
            rev.col.push(*u as u32);
            rev_slot.push(*slot);
        }
        rev.row.push(rev.col.len());
    }
    Projection {
        ids: p.ids.clone(),
        fwd,
        rev,
        edge_ids: (0..edges.len() as u64).collect(),
        rev_slot,
        edge_props: vec![props],
        node_props: Vec::new(),
        edge_types: Vec::new(),
        labels: (Vec::new(), Vec::new()),
        orientation: Orientation::Natural,
    }
}

/// The PageRank cases compared with GDS 2026.09.0 on the Zipf fixture
/// (`docs/evidence/lab-sprint-5/gds_oracle.sh`, rule F with the entry's
/// `tolerance`).
fn zipf_cases(f: &Fixture) -> Vec<Case> {
    let sources = Value::List(
        [0, 7, 42]
            .iter()
            .map(|u| Value::Node(f.ids[*u]))
            .collect::<Vec<_>>()
            .into(),
    );
    let case = |name, config, expected| Case {
        name,
        config,
        expected,
        columns: ("nodeId", "score"),
        rule: Rule::Float(1e-7),
    };
    vec![
        case("natural", empty(), "zipf300.gds.pagerank.natural.jsonl"),
        case(
            "converged",
            record(&[("maxIterations", int(100))]),
            "zipf300.gds.pagerank.converged.jsonl",
        ),
        case(
            "weighted",
            record(&[("relationshipWeightProperty", txt("weight"))]),
            "zipf300.gds.pagerank.weighted.jsonl",
        ),
        case(
            "personalised",
            record(&[("sourceNodes", sources)]),
            "zipf300.gds.pagerank.personalised.jsonl",
        ),
        case(
            "reverse",
            record(&[("orientation", txt("REVERSE"))]),
            "zipf300.gds.pagerank.reverse.jsonl",
        ),
        case(
            "undirected",
            record(&[("orientation", txt("undirected"))]),
            "zipf300.gds.pagerank.undirected.jsonl",
        ),
        Case {
            rule: Rule::Float(1e-4),
            ..case(
                "scaled",
                record(&[
                    ("scaler", txt("L2NORM")),
                    ("dampingFactor", Value::Float(0.5)),
                    ("tolerance", Value::Float(1e-4)),
                    ("maxIterations", int(7)),
                ]),
                "zipf300.gds.pagerank.scaled.jsonl",
            )
        },
    ]
}

/// rmp #63 (FR-ALG-002, FR-ALG-006, FR-ALG-007): on the seeded Zipf
/// fixture, every PageRank case is bit-equal at `concurrency` 1 to 16,
/// twice each, starts at most `concurrency` workers, and matches GDS under
/// rule F; `ranIterations` and `didConverge` equal GDS's; `CALL` and
/// `run_algorithm` give bit-equal rows on one snapshot.
#[test]
fn pagerank_is_deterministic_and_matches_gds() {
    current(async {
        let f = Fixture::load("zipf300", CAP).await;
        let worst = check_cases(&f, "gds.pageRank.stream", &zipf_cases(&f)).await;
        for (name, w) in &worst {
            eprintln!("PageRank {name}: largest relative difference to GDS {w:e}");
        }
        // ranIterations and didConverge against GDS's stats mode.
        for (config, file) in [
            (empty(), "zipf300.gds.pagerank.natural.stats.jsonl"),
            (
                record(&[("maxIterations", int(100))]),
                "zipf300.gds.pagerank.converged.stats.jsonl",
            ),
            (
                record(&[("relationshipWeightProperty", txt("weight"))]),
                "zipf300.gds.pagerank.weighted.stats.jsonl",
            ),
            (
                record(&[
                    ("scaler", txt("L2NORM")),
                    ("dampingFactor", Value::Float(0.5)),
                    ("tolerance", Value::Float(1e-4)),
                    ("maxIterations", int(7)),
                ]),
                "zipf300.gds.pagerank.scaled.stats.jsonl",
            ),
        ] {
            let tx = f.s.begin(TransactionOptions::default()).await.unwrap();
            let config = with(&config, "writeProperty", txt("pr"));
            let row = drain(
                tx.execute("CALL gds.pageRank.write($c)", param("c", config))
                    .await
                    .unwrap(),
            )
            .await
            .unwrap()
            .remove(0);
            tx.rollback().await.unwrap();
            let gds = &expected(file)[0];
            eprintln!(
                "PageRank {file}: lpg ranIterations {:?} didConverge {:?}; GDS {} {}",
                row[0], row[1], gds["ranIterations"], gds["didConverge"]
            );
            assert_eq!(
                row[0],
                int(gds["ranIterations"].as_i64().unwrap()),
                "{file}"
            );
            assert_eq!(
                row[1],
                Value::Bool(gds["didConverge"].as_bool().unwrap()),
                "{file}"
            );
        }
        // CALL and the Rust API on one snapshot (FR-ALG-002).
        let tx = f.s.begin(TransactionOptions::default()).await.unwrap();
        let config = record(&[("relationshipWeightProperty", txt("weight"))]);
        let by_call = drain(
            tx.execute("CALL gds.pageRank.stream($c)", param("c", config.clone()))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        let by_api = drain(
            tx.run_algorithm(
                "gds.pageRank.stream",
                param("configuration", config.clone()),
            )
            .await
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(first_difference(&by_call, &by_api), None);
        assert_eq!(by_call.len(), 300);
        tx.commit().await.unwrap();
        let by_session = drain(
            f.s.run_algorithm("gds.pageRank.stream", param("configuration", config))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(first_difference(&by_call, &by_session), None);
        // The Rust API's errors: an unknown routine, a key that is no
        // parameter.
        let e =
            f.s.run_algorithm("gds.pageRank.stats", Parameters::new())
                .await
                .unwrap_err();
        assert_eq!(e.code(), "42N08");
        let e =
            f.s.run_algorithm("show_indexes", Parameters::new())
                .await
                .unwrap_err();
        assert_eq!(e.code(), "42N08");
        let e =
            f.s.run_algorithm("gds.pageRank.stream", param("graphName", txt("g")))
                .await
                .unwrap_err();
        assert_eq!(e.code(), "22N11");
        f.db.close().await.unwrap();
    });
}

/// rmp #61 and #63 on the karate club: NetworkX's PageRank values sum to
/// 1 within 1e-12; lpg's `UNDIRECTED` PageRank matches GDS's 34 rows.
#[test]
fn karate_pagerank_matches_the_oracles() {
    let nx = expected("karate.nx.pagerank.jsonl");
    assert_eq!(nx.len(), 34);
    let sum: f64 = nx.iter().map(|m| m["value"].as_f64().unwrap()).sum();
    assert!((sum - 1.0).abs() <= 1e-12, "{sum}");
    current(async {
        let f = Fixture::load("karate", CAP).await;
        let rows = f
            .call(
                "gds.pageRank.stream",
                record(&[("orientation", txt("UNDIRECTED"))]),
            )
            .await
            .unwrap();
        let want = expected_by_key("karate.gds.pagerank.undirected.jsonl", "nodeId", "score");
        assert_eq!(want.len(), 34);
        let worst = compare(Rule::Float(1e-7), &f.by_uid(&rows), &want).unwrap();
        eprintln!("PageRank karate: largest relative difference to GDS {worst:e}");
        f.db.close().await.unwrap();
    });
}

/// rmp #63 (FR-ALG-006): without `concurrency`, a 2-block projection runs
/// on 2 workers (more than one, at most 4); with 1, one; a 1-node
/// projection, one; for k in 1 to 16, at most k (here min(k, blocks)).
#[test]
fn worker_counts_follow_concurrency() {
    current(async {
        let (_d, db, s) = new_db(CAP).await;
        let key = probe_key(&db);
        exec(&s, "INSERT (a:A)-[:R]->(b:A)").await;
        let workers = |config: Value| {
            let (s, key) = (&s, &key);
            async move {
                probe::take(key);
                rows(s, "CALL gds.pageRank.stream($c)", param("c", config))
                    .await
                    .unwrap();
                probe::take(key).unwrap()
            }
        };
        let two = workers(empty()).await;
        assert!(two > 1 && two <= 4, "{two} workers on 2 blocks");
        assert_eq!(workers(record(&[("concurrency", int(1))])).await, 1);
        let lone = record(&[("nodeLabels", txt("Lone"))]);
        assert_eq!(workers(lone.clone()).await, 1, "no node: one worker");
        exec(&s, "INSERT (:Lone)").await;
        assert_eq!(workers(lone).await, 1, "a 1-node projection: one worker");
        db.close().await.unwrap();
        let f = Fixture::load("zipf300", CAP).await;
        let key = probe_key(&f.db);
        let blocks = Sched::new(300, 1, true, None).blocks();
        for k in 1..=16 {
            probe::take(&key);
            f.call("gds.pageRank.stream", record(&[("concurrency", int(k))]))
                .await
                .unwrap();
            let w = probe::take(&key).unwrap();
            assert_eq!(w, (k as usize).min(blocks), "concurrency {k}");
        }
        let e = f
            .call("gds.pageRank.stream", record(&[("concurrency", int(0))]))
            .await
            .unwrap_err();
        assert_eq!(e.code(), "22N11");
        f.db.close().await.unwrap();
    });
}

/// rmp #63 (FR-ALG-004): a label filter returns only nodes of the label; a
/// node another transaction commits after the snapshot is absent; a
/// projection the memory refuses fails the statement with a GQLSTATUS
/// error and aborts the transaction; the accountant never exceeds the
/// cap.
#[test]
fn filters_snapshot_and_memory_cap() {
    current(async {
        let (_d, db, s) = new_db(CAP).await;
        exec(
            &s,
            "INSERT (a:A)-[:R]->(b:B), (b)-[:R]->(c:A&B), (c)-[:R]->(a), (d:B)-[:R]->(a)",
        )
        .await;
        let ids = |rows: &[Vec<Value>]| -> BTreeSet<u64> {
            rows.iter()
                .map(|r| match r[0] {
                    Value::Int(i) => i.cast_unsigned(),
                    _ => panic!(),
                })
                .collect()
        };
        let only_a = rows(
            &s,
            "CALL gds.pageRank.stream($c)",
            param("c", record(&[("nodeLabels", txt("A"))])),
        )
        .await
        .unwrap();
        let a_nodes: BTreeSet<u64> = rows(&s, "MATCH (n:A) RETURN n", Parameters::new())
            .await
            .unwrap()
            .into_iter()
            .map(|r| match r[0] {
                Value::Node(id) => id,
                _ => panic!(),
            })
            .collect();
        assert_eq!(ids(&only_a), a_nodes);
        assert_eq!(a_nodes.len(), 2);
        // Snapshot: a node committed by another session after T's snapshot.
        let t = s.begin(TransactionOptions::default()).await.unwrap();
        let before = drain(
            t.execute("CALL gds.pageRank.stream()", Parameters::new())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        let other = db.session(OWNER, None, None).await.unwrap();
        exec(&other, "INSERT (:A)-[:R]->(:A)").await;
        let after = drain(
            t.execute("CALL gds.pageRank.stream()", Parameters::new())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(ids(&before), ids(&after));
        assert_eq!(after.len(), 4);
        t.commit().await.unwrap();
        assert_eq!(
            rows(&s, "CALL gds.pageRank.stream()", Parameters::new())
                .await
                .unwrap()
                .len(),
            6
        );
        // The transaction's memory limit below the projection: 51N73.
        let mut limits = s.limits();
        limits.memory = Some(1024);
        s.set_limits(limits);
        let t = s.begin(TransactionOptions::default()).await.unwrap();
        let e = fails(&t, "CALL gds.pageRank.stream()", Parameters::new()).await;
        assert_eq!(e.code(), "51N73", "{e}");
        let e2 = t
            .execute("MATCH (n) RETURN n", Parameters::new())
            .await
            .unwrap_err();
        assert_eq!(e2.code(), "25N05", "the transaction is aborted: {e2}");
        let _ = t.rollback().await;
        limits.memory = None;
        s.set_limits(limits);
        // The memory cap: every byte the pool can give is taken, so the
        // projection's first chunk is refused: 51N72.
        let engine = db.engine_handle();
        let mut taken = Vec::new();
        while let Ok(r) = engine.core().pool.reserve(64 << 10) {
            taken.push(r);
        }
        let t = s.begin(TransactionOptions::default()).await.unwrap();
        let e = fails(&t, "CALL gds.pageRank.stream()", Parameters::new()).await;
        assert_eq!(e.code(), "51N72", "{e}");
        let e2 = t
            .execute("MATCH (n) RETURN n", Parameters::new())
            .await
            .unwrap_err();
        assert_eq!(e2.code(), "25N05", "the transaction is aborted: {e2}");
        let _ = t.rollback().await;
        drop(taken);
        let acc = engine.accountant();
        assert!(
            acc.peak() <= acc.cap(),
            "peak {} over cap {}",
            acc.peak(),
            acc.cap()
        );
        db.close().await.unwrap();
    });
}

/// rmp #63 (FR-ALG-003, FR-ALG-007, FR-LIM-007): `.write` in a transaction
/// that rolls back leaves no score property, and committed writes the
/// stream's values; `.stream` changes no property; a write-back the cap
/// refuses fails with 51N72; `.stats` is no routine (42N08); a graph-name
/// argument is refused; the modes have GDS's columns.
#[test]
fn write_mode_and_signatures() {
    current(async {
        let (_d, db, s) = new_db(CAP).await;
        exec(
            &s,
            "INSERT (a:A)-[:R]->(b:A), (b)-[:R]->(c:A), (c)-[:R]->(a), (c)-[:R]->(b)",
        )
        .await;
        let count = |s: &Session| {
            let s = s.clone();
            async move {
                rows(
                    &s,
                    "MATCH (n) WHERE n.pr IS NOT NULL RETURN count(*) AS c",
                    Parameters::new(),
                )
                .await
                .unwrap()[0][0]
                    .clone()
            }
        };
        let w = record(&[("writeProperty", txt("pr"))]);
        let t = s.begin(TransactionOptions::default()).await.unwrap();
        let row = drain(
            t.execute("CALL gds.pageRank.write($c)", param("c", w.clone()))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(row.len(), 1);
        assert_eq!(row[0][7], int(3), "nodePropertiesWritten");
        t.rollback().await.unwrap();
        assert_eq!(count(&s).await, int(0));
        let stream = rows(&s, "CALL gds.pageRank.stream()", Parameters::new())
            .await
            .unwrap();
        assert_eq!(stream.len(), 3);
        assert_eq!(count(&s).await, int(0), "stream writes nothing");
        let row = rows(&s, "CALL gds.pageRank.write($c)", param("c", w.clone()))
            .await
            .unwrap();
        let Value::Record(conf) = &row[0][8] else {
            panic!()
        };
        assert!(
            conf.iter()
                .any(|(k, v)| &**k == "writeProperty" && *v == txt("pr"))
        );
        assert!(
            conf.iter()
                .any(|(k, v)| &**k == "dampingFactor" && *v == Value::Float(0.85))
        );
        let Value::Record(dist) = &row[0][2] else {
            panic!()
        };
        assert_eq!(dist.len(), 9);
        let written: BTreeMap<i64, Value> =
            rows(&s, "MATCH (n) RETURN n, n.pr AS pr", Parameters::new())
                .await
                .unwrap()
                .into_iter()
                .map(|r| match r[0] {
                    Value::Node(id) => (id.cast_signed(), r[1].clone()),
                    _ => panic!(),
                })
                .collect();
        let streamed: BTreeMap<i64, Value> = stream
            .into_iter()
            .map(|r| match r[0] {
                Value::Int(id) => (id, r[1].clone()),
                _ => panic!(),
            })
            .collect();
        assert_eq!(written, streamed);
        // Errors of the mode and the arguments.
        for (gql, code) in [
            ("CALL gds.pageRank.stats($c)", "42N08"),
            ("CALL gds.pageRank.mutate($c)", "42N08"),
            ("CALL gds.pageRank.stream('g', $c)", "22N11"),
            ("CALL gds.pageRank.write()", "22N11"),
        ] {
            let e = rows(&s, gql, param("c", empty())).await.unwrap_err();
            assert_eq!(e.code(), code, "{gql}: {e}");
        }
        let e = rows(
            &s,
            "MATCH (n) CALL gds.pageRank.write($c) YIELD nodePropertiesWritten RETURN 1",
            param("c", w.clone()),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code(), "42N02", "a write mode in a query: {e}");
        let r = crate::proc::Registry::with_builtins();
        let cols = |name: &str| r.get(name).unwrap().get().sig.columns.clone();
        assert_eq!(cols("gds.pageRank.stream"), ["nodeId", "score"]);
        assert_eq!(cols("gds.pageRank.write"), super::RANK_WRITE);
        assert_eq!(
            r.get("gds.pageRank.write").unwrap().get().mode(),
            crate::proc::Mode::ReadWrite
        );
        db.close().await.unwrap();

        // A write-back the memory cap refuses: big node records, and only
        // a little more than the projection's first chunk left.
        let (_d, db, s) = new_db(CAP).await;
        let big = Value::string(&"x".repeat(20_000));
        let text = format!("INSERT {}", vec!["(:Heavy {s: $s})"; 20].join(", "));
        rows(&s, &text, param("s", big)).await.unwrap();
        let engine = db.engine_handle();
        let mut taken = Vec::new();
        while let Ok(r) = engine.core().pool.reserve(64 << 10) {
            taken.push(r);
        }
        // 256 KiB free: the run's 128 KiB chunk fits, 20 × 20 KB of new
        // records do not.
        taken.truncate(taken.len() - 4);
        let t = s.begin(TransactionOptions::default()).await.unwrap();
        let e = fails(&t, "CALL gds.pageRank.write($c)", param("c", w)).await;
        assert_eq!(e.code(), "51N72", "{e}");
        let _ = t.rollback().await;
        drop(taken);
        assert_eq!(count(&s).await, int(0));
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap());
        db.close().await.unwrap();
    });
}

/// Laboratory record (rmp #63): the accountant's peak against the cap and
/// the projection's bytes on a seeded Zipf graph of 20 000 nodes and
/// 200 000 edges; run under `docs/evidence/lab-sprint-5/heaptrack.sh` for
/// the heap peak of the projection.
#[test]
#[ignore = "laboratory measurement"]
fn lab_zipf_projection_memory() {
    current(async {
        let files = tempfile::tempdir().unwrap();
        let g = crate::lab::generator::power_law(11, 20_000, 200_000, true, true);
        crate::lab::generator::write_oracle(files.path(), &g);
        let cap: u64 = 64 << 20;
        let (_d, db, s) = new_db(cap).await;
        crate::lab::generator::load_oracle(&s, files.path()).await;
        let engine = db.engine_handle();
        let before = engine.accountant().peak();
        let probe = project(&db, &s, Spec::default(), Orientation::Natural).await;
        let bytes = probe.out.lock().unwrap().as_ref().unwrap().bytes();
        let heap = *probe.heap.lock().unwrap();
        let reserved = *probe.reserved.lock().unwrap();
        let pr = rows(
            &s,
            "CALL gds.pageRank.write($c)",
            param("c", record(&[("writeProperty", txt("pr"))])),
        )
        .await
        .unwrap();
        let acc = engine.accountant();
        eprintln!(
            "LAB zipf 20000/200000: projection arrays {bytes} B; heap peak of the build {heap} B, reserved by the run {reserved} B; accountant peak before {before} B, after {} B, cap {} B; ranIterations {:?}",
            acc.peak(),
            acc.cap(),
            pr[0][0]
        );
        assert!(acc.peak() <= acc.cap());
        db.close().await.unwrap();
    });
}

/// Every entry declares its procedures and columns; a write mode reports
/// what it wrote.
#[test]
fn every_entry_has_its_columns() {
    for e in ENTRIES {
        assert!(!e.algs.is_empty() && !e.stream_columns.is_empty(), "{e:?}");
        assert!(
            e.write.is_none() || e.write_columns.iter().any(|c| c.ends_with("Written")),
            "{e:?}"
        );
        assert!(format!("{e:?}").starts_with(e.stream));
    }
}

/// The error of `gql` in `t`, whether the statement fails when it starts
/// or while its rows are pulled.
async fn fails(t: &crate::Transaction, gql: &str, params: Parameters) -> Error {
    match t.execute(gql, params).await {
        Err(e) => e,
        Ok(st) => drain(st).await.expect_err("the statement fails"),
    }
}
