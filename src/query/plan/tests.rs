//! Tests of the planner (rmp #44): EXPLAIN, index seeks on ONLINE
//! indexes, equal results with and without each index kind, join order,
//! and estimates from the statistics.

use std::sync::{Arc, Mutex, mpsc};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::join::{self, Graph as JoinGraph};
use super::*;
use crate::catalog::{DEFAULT_GRAPH_ID, TokenKind};
use crate::index::build::{Hook, Phase};
use crate::query::exec::tests::support::{rows, rows_in, session};
use crate::query::exec::{Params, explain};
use crate::storage::engine::Engine;
use crate::test_support::seed;
use crate::tx::Isolation;
use crate::tx::tests::db;
use crate::types::{Point, Srid, Value};

fn plan_of(e: &Engine, text: &str, params: &Params) -> Description {
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let d =
        explain(text, &mut tx, &session(), params).unwrap_or_else(|err| panic!("{text}: {err}"));
    tx.commit().unwrap();
    d
}

/// Every node of a description, pre-order.
fn nodes(d: &Description) -> Vec<&Description> {
    let mut out = vec![d];
    for c in &d.children {
        out.extend(nodes(c));
    }
    out
}

fn has(d: &Description, name: &str) -> bool {
    nodes(d).iter().any(|n| n.name == name)
}

fn index(e: &Engine, kind: SchemaKind, label: &str, props: &[&str]) -> u64 {
    let l = e.token(TokenKind::Label, label).unwrap();
    let ps: Vec<u64> = props
        .iter()
        .map(|p| e.token(TokenKind::PropertyKey, p).unwrap())
        .collect();
    let token = if kind == SchemaKind::Lookup { 0 } else { l };
    let id = e
        .create_index(DEFAULT_GRAPH_ID, kind, Entity::Node, token, &ps)
        .unwrap();
    e.wait_index(id).unwrap();
    id
}

/// FR-OBS-005: EXPLAIN of a statement that inserts a node returns a plan,
/// and no node exists afterwards.
#[test]
fn explain_of_an_insert_returns_a_plan_and_inserts_nothing() {
    let (_d, e) = db();
    let d = plan_of(&e, "INSERT (:N {p: 1})", &Params::new());
    assert_eq!(d.name, "EmptyResult");
    assert!(has(&d, "Create"), "{d:#?}");
    assert!(d.profile.is_none());
    assert_eq!(
        rows(&e, "MATCH (n) RETURN count(*) AS c"),
        vec![vec![Value::Int(0)]]
    );
}

/// FR-IDX-001: an indexed equality and an indexed range use `IndexSeek` on
/// an ONLINE index, and not on a POPULATING one.
#[test]
fn equality_and_range_use_an_online_index_and_not_a_populating_one() {
    let (_d, e) = db();
    rows(
        &e,
        "INSERT (:P {age: 1, k: 1}), (:P {age: 5, k: 5}), (:P {age: 41, k: 41}), (:P {age: 50, k: 50})",
    );
    let id = index(&e, SchemaKind::Range, "P", &["age"]);
    let eq = plan_of(
        &e,
        "MATCH (n:P) WHERE n.age = 5 RETURN n.k AS k",
        &Params::new(),
    );
    let seek = nodes(&eq)
        .into_iter()
        .find(|n| n.name == "NodeIndexSeek")
        .expect("a seek");
    assert!(
        seek.details.contains(&format!("index #{id}")),
        "{}",
        seek.details
    );
    let range = plan_of(
        &e,
        "MATCH (n:P) WHERE n.age > 40 RETURN n.k AS k",
        &Params::new(),
    );
    assert!(has(&range, "NodeIndexSeekByRange"), "{range:#?}");
    assert_eq!(
        rows(&e, "MATCH (n:P) WHERE n.age = 5 RETURN n.k AS k"),
        vec![vec![Value::Int(5)]]
    );
    let mut r = rows(&e, "MATCH (n:P) WHERE n.age > 40 RETURN n.k AS k");
    r.sort_by(|a, b| crate::types::total_cmp(&a[0], &b[0]));
    assert_eq!(r, vec![vec![Value::Int(41)], vec![Value::Int(50)]]);
    // A second index, held POPULATING by the build's pause hook.
    let (reached_tx, reached) = mpsc::channel();
    let (resume, resume_rx) = mpsc::channel::<()>();
    let resume_rx = Mutex::new(resume_rx);
    let hook: Hook = Arc::new(move |p| {
        if p == Phase::BeforeOnline {
            let _ = reached_tx.send(());
            let _ = resume_rx.lock().unwrap().recv();
        }
    });
    *e.core().builds.hook.lock().unwrap() = Some(hook);
    let l = e.token(TokenKind::Label, "P").unwrap();
    let k = e.token(TokenKind::PropertyKey, "k").unwrap();
    let pop = e
        .create_index(DEFAULT_GRAPH_ID, SchemaKind::Range, Entity::Node, l, &[k])
        .unwrap();
    reached
        .recv_timeout(crate::test_support::WAIT_BOUND)
        .expect("the build pauses before ONLINE");
    let d = plan_of(
        &e,
        "MATCH (n:P) WHERE n.k = 5 RETURN n.age AS a",
        &Params::new(),
    );
    assert!(
        !has(&d, "NodeIndexSeek") && !has(&d, "NodeIndexSeekByRange"),
        "{d:#?}"
    );
    assert_eq!(
        rows(&e, "MATCH (n:P) WHERE n.k = 5 RETURN n.age AS a"),
        vec![vec![Value::Int(5)]]
    );
    resume.send(()).unwrap();
    e.wait_index(pop).unwrap();
    *e.core().builds.hook.lock().unwrap() = None;
    let d = plan_of(
        &e,
        "MATCH (n:P) WHERE n.k = 5 RETURN n.age AS a",
        &Params::new(),
    );
    let seek = nodes(&d)
        .into_iter()
        .find(|n| n.name == "NodeIndexSeek")
        .expect("a seek");
    assert!(
        seek.details.contains(&format!("index #{pop}")),
        "{}",
        seek.details
    );
}

fn sorted(mut r: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    r.sort_by(|a, b| {
        for (x, y) in a.iter().zip(b) {
            let o = crate::types::total_cmp(x, y);
            if o.is_ne() {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    r
}

fn point(x: i64, y: i64) -> Value {
    Value::Point(Box::new(
        Point::new(Srid::Cartesian2D, &[x as f64, y as f64]).unwrap(),
    ))
}

/// For seeded graphs and queries, results with and without each of the
/// five index kinds of FR-IDX-001 (single-property, composite, lookup,
/// range, point) are equal, and each kind's query uses its index.
#[test]
fn results_are_equal_with_and_without_each_index_kind() {
    let s = seed();
    println!("seed {s}");
    let mut rng = StdRng::seed_from_u64(s);
    let (_d, e) = db();
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let mut text = String::from("INSERT ");
    for i in 0..60 {
        let label = if rng.random_range(0..3) == 0 {
            "B"
        } else {
            "A"
        };
        let x = match rng.random_range(0..10) {
            0 => "NULL".to_string(),
            1 => format!("{}e0", rng.random_range(0..5)),
            _ => rng.random_range(0..5).to_string(),
        };
        let (px, py) = (rng.random_range(0..3), rng.random_range(0..3));
        if i > 0 {
            text.push_str(", ");
        }
        text.push_str(&format!(
            "(n{i}:{label} {{x: {x}, y: {}, loc: $p{px}{py}}})",
            rng.random_range(0..20)
        ));
    }
    for i in 0..40 {
        let (a, b) = (rng.random_range(0..60), rng.random_range(0..60));
        text.push_str(&format!(", (n{a})-[:T {{w: {i}}}]->(n{b})"));
    }
    let mut params = Params::new();
    for px in 0..3 {
        for py in 0..3 {
            params.insert(format!("p{px}{py}"), point(px, py));
        }
    }
    rows_in(&mut tx, &text, &params).unwrap();
    tx.commit().unwrap();
    // The queries, each with the operator its index kind should give.
    let queries: Vec<(&str, &str)> = vec![
        ("MATCH (n:A) WHERE n.x = $v RETURN n", "single"),
        ("MATCH (n:A {x: $v, y: $w}) RETURN n", "composite"),
        ("MATCH (n:B) RETURN n", "lookup"),
        (
            "MATCH (n:A) WHERE n.y > $lo AND n.y <= $hi RETURN n",
            "range",
        ),
        ("MATCH (n:A) WHERE n.loc = $pt RETURN n", "point"),
        (
            "MATCH (n:A)-[t:T]->(m) WHERE n.x = $v RETURN n, t.w AS w, m",
            "single",
        ),
        (
            "MATCH (n:A)-[t:T]->(m:A) WHERE n.x >= $v AND m.y < $hi RETURN t",
            "-",
        ),
    ];
    let mut cases = Vec::new();
    for _ in 0..6 {
        let mut p = Params::new();
        p.insert("v".into(), Value::Int(rng.random_range(0..5)));
        p.insert("w".into(), Value::Int(rng.random_range(0..20)));
        let lo = rng.random_range(0..20);
        p.insert("lo".into(), Value::Int(lo));
        p.insert("hi".into(), Value::Int(lo + rng.random_range(0..8)));
        p.insert(
            "pt".into(),
            point(rng.random_range(0..3), rng.random_range(0..3)),
        );
        cases.push(p);
    }
    let run_all = |e: &Engine| -> Vec<Vec<Vec<Value>>> {
        let mut out = Vec::new();
        for (q, _) in &queries {
            for p in &cases {
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                let mut p = p.clone();
                p.retain(|k, _| q.contains(&format!("${k}")));
                out.push(sorted(
                    rows_in(&mut tx, q, &p).unwrap_or_else(|err| panic!("{q}: {err}")),
                ));
                tx.commit().unwrap();
            }
        }
        out
    };
    let base = run_all(&e);
    assert!(base.iter().any(|r| !r.is_empty()));
    let kinds: [(&str, SchemaKind, &str, &[&str], &str); 5] = [
        ("single", SchemaKind::Range, "A", &["x"], "NodeIndexSeek"),
        (
            "composite",
            SchemaKind::Range,
            "A",
            &["x", "y"],
            "NodeIndexSeek",
        ),
        ("lookup", SchemaKind::Lookup, "B", &[], "NodeByLabelScan"),
        (
            "range",
            SchemaKind::Range,
            "A",
            &["y"],
            "NodeIndexSeekByRange",
        ),
        ("point", SchemaKind::Point, "A", &["loc"], "NodeIndexSeek"),
    ];
    for (name, kind, label, props, op) in kinds {
        let id = index(&e, kind, label, props);
        assert_eq!(run_all(&e), base, "results with the {name} index");
        let (q, _) = queries.iter().find(|(_, k)| *k == name).unwrap();
        let mut p = cases[0].clone();
        p.retain(|k, _| q.contains(&format!("${k}")));
        let d = plan_of(&e, q, &p);
        let used = nodes(&d).into_iter().any(|n| {
            n.name == op
                && (kind == SchemaKind::Lookup || n.details.contains(&format!("index #{id}")))
        });
        assert!(used, "{name}: {d:#?}");
    }
}

/// Random connected components of `k` edges.
fn component(rng: &mut StdRng, k: usize) -> JoinGraph {
    let n = rng.random_range(2..=k + 1);
    let mut edges = Vec::new();
    // A random tree over the n nodes, then extra edges up to k.
    for v in 1..n {
        let u = rng.random_range(0..v);
        edges.push((u, v, rng.random_range(1..100) as f64 / 1000.0));
    }
    while edges.len() < k {
        let (a, b) = (rng.random_range(0..n), rng.random_range(0..n));
        edges.push((a, b, rng.random_range(1..100) as f64 / 1000.0));
    }
    edges.truncate(k.max(n - 1));
    JoinGraph {
        nodes: (0..n).map(|_| rng.random_range(1..10_000) as f64).collect(),
        bound: vec![false; n],
        edges,
    }
}

/// Every order's cost: each start and each permutation of the edges.
fn brute_min(g: &JoinGraph) -> f64 {
    fn perms(
        rest: &mut Vec<usize>,
        cur: &mut Vec<usize>,
        g: &JoinGraph,
        start: usize,
        best: &mut f64,
    ) {
        if rest.is_empty() {
            if let Some(c) = g.cost_of(start, cur) {
                *best = best.min(c);
            }
            return;
        }
        for i in 0..rest.len() {
            let e = rest.remove(i);
            cur.push(e);
            perms(rest, cur, g, start, best);
            cur.pop();
            rest.insert(i, e);
        }
    }
    let mut best = f64::INFINITY;
    for start in 0..g.nodes.len() {
        perms(
            &mut (0..g.edges.len()).collect(),
            &mut Vec::new(),
            g,
            start,
            &mut best,
        );
    }
    best
}

/// TS-20: for 2 to 7 expansions the chosen order has the minimum cost of
/// every order; at 8 the greedy path is taken.
#[test]
fn join_order_is_exact_up_to_seven_expansions_and_greedy_at_eight() {
    let s = seed();
    println!("seed {s}");
    let mut rng = StdRng::seed_from_u64(s);
    for k in 2..=7 {
        for _ in 0..4 {
            let g = component(&mut rng, k);
            assert_eq!(g.edges.len(), k);
            let o = join::order(&g);
            assert_eq!(o.steps.len(), k);
            let want = brute_min(&g);
            assert!(
                (o.cost - want).abs() <= want * 1e-9,
                "k = {k}: chosen {} vs minimum {want}",
                o.cost
            );
            let edges: Vec<usize> = o.steps.iter().map(|s| s.edge).collect();
            assert_eq!(g.cost_of(o.start, &edges), Some(o.cost));
        }
    }
    assert_eq!(
        join::GREEDY_PLANS.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    let g = component(&mut rng, 8);
    let o = join::order(&g);
    assert_eq!(o.steps.len(), 8);
    assert_eq!(
        join::GREEDY_PLANS.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    // Through the planner: a pattern of 8 expansions.
    let (_d, e) = db();
    let d = plan_of(
        &e,
        "MATCH (a)-[]->(b)-[]->(c)-[]->(d)-[]->(e)-[]->(f)-[]->(g)-[]->(h)-[]->(i) RETURN a",
        &Params::new(),
    );
    assert_eq!(
        nodes(&d)
            .iter()
            .filter(|n| n.name.starts_with("Expand"))
            .count(),
        8
    );
    assert_eq!(
        join::GREEDY_PLANS.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

/// Estimates read the statistics: after inserting 1 000 `L` nodes, the
/// `LabelScan` estimate is 1 000.
#[test]
fn the_label_scan_estimate_reads_the_statistics() {
    let (_d, e) = db();
    index(&e, SchemaKind::Lookup, "L", &[]);
    let mut text = String::from("INSERT (:L)");
    for _ in 1..1000 {
        text.push_str(", (:L)");
    }
    rows(&e, &text);
    let d = plan_of(&e, "MATCH (n:L) RETURN n", &Params::new());
    let scan = nodes(&d)
        .into_iter()
        .find(|n| n.name == "NodeByLabelScan")
        .expect("a label scan");
    assert_eq!(scan.estimated_rows, 1000.0);
    let all = plan_of(&e, "MATCH (n) WHERE n.p = 1 RETURN n", &Params::new());
    assert!(has(&all, "AllNodesScan"));
}
