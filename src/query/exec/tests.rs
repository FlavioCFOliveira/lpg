//! Tests of the executor (rmp #45) and of the statements it runs.

pub(crate) mod support {
    //! Helpers shared by the engine tests.

    use crate::error::Error;
    use crate::query::binder::SessionCtx;
    use crate::query::exec::{Params, collect, run};
    use crate::storage::engine::Engine;
    use crate::test_support::OWNER;
    use crate::tx::{Isolation, Transaction};
    use crate::types::Value;

    /// The default session of the tests.
    pub(crate) fn session() -> SessionCtx {
        SessionCtx::new(OWNER)
    }

    /// Runs `text` in `tx` with `params` and returns its rows.
    pub(crate) fn rows_in(
        tx: &mut Transaction,
        text: &str,
        params: &Params,
    ) -> Result<Vec<Vec<Value>>, Error> {
        let mut c = run(text, tx, &session(), params)?;
        collect(&mut c, tx)
    }

    /// Runs `text` in a transaction of its own, committed, and returns its
    /// rows.
    pub(crate) fn rows(e: &Engine, text: &str) -> Vec<Vec<Value>> {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let r =
            rows_in(&mut tx, text, &Params::new()).unwrap_or_else(|err| panic!("{text}: {err}"));
        tx.commit().unwrap();
        r
    }

    /// The error of `text`, run in a transaction of its own.
    pub(crate) fn error(e: &Engine, text: &str) -> Error {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        match rows_in(&mut tx, text, &Params::new()) {
            Ok(r) => panic!("{text}: expected an error, got {r:?}"),
            Err(err) => err,
        }
    }
}

use support::*;

use crate::tx::tests::db;
use crate::types::Value;

#[test]
fn smoke_insert_match_return() {
    let (_d, e) = db();
    rows(
        &e,
        "INSERT (:P {name: 'a', age: 30})-[:K {w: 1}]->(:P {name: 'b', age: 40})",
    );
    let r = rows(
        &e,
        "MATCH (a:P)-[k:K]->(b:P) RETURN a.name AS x, b.age AS y, k.w AS w",
    );
    assert_eq!(
        r,
        vec![vec![Value::string("a"), Value::Int(40), Value::Int(1)]]
    );
    let r = rows(&e, "MATCH (n:P) RETURN count(*) AS c, sum(n.age) AS s");
    assert_eq!(r, vec![vec![Value::Int(2), Value::Int(70)]]);
    let r = rows(&e, "MATCH (n:P) RETURN n.name AS n ORDER BY n DESC");
    assert_eq!(r, vec![vec![Value::string("b")], vec![Value::string("a")]]);
    let r = rows(
        &e,
        "MATCH (n:P) WHERE n.age > 35 SET n.age = n.age + 1 RETURN n.age AS a",
    );
    assert_eq!(r, vec![vec![Value::Int(41)]]);
    let r = rows(
        &e,
        "MATCH (n:P) OPTIONAL MATCH (n)-[:K]->(m) RETURN n.name AS a, m.name AS b ORDER BY a",
    );
    assert_eq!(
        r,
        vec![
            vec![Value::string("a"), Value::string("b")],
            vec![Value::string("b"), Value::Null]
        ]
    );
    let r = rows(
        &e,
        "MATCH (n:P) WHERE EXISTS { (n)-[:K]->() } RETURN n.name AS a",
    );
    assert_eq!(r, vec![vec![Value::string("a")]]);
    assert_eq!(error(&e, "MATCH (n) RETURN m").code(), "42002");
}

mod reference {
    //! The reference evaluator of rmp #45: a brute-force evaluator over a
    //! dump of the snapshot, the oracle for seeded random queries. It
    //! shares no code with the engine: it enumerates every binding of a
    //! generated pattern and applies its own three-valued logic over
    //! integer properties.

    use std::collections::BTreeMap;

    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};

    use super::support::rows_in;
    use crate::catalog::DEFAULT_GRAPH_ID;
    use crate::query::exec::Params;
    use crate::storage::engine::Engine;
    use crate::storage::graph::{decode_edge, element_of};
    use crate::test_support::seed;
    use crate::tx::Isolation;
    use crate::tx::tests::db;
    use crate::types::{Value, record, total_cmp};

    #[derive(Debug, Clone)]
    struct DNode {
        id: u64,
        labels: Vec<String>,
        p: Option<i64>,
        q: Option<i64>,
    }

    #[derive(Debug, Clone)]
    struct DEdge {
        src: u64,
        dst: u64,
        ty: String,
        w: Option<i64>,
    }

    /// The nodes and edges of the default graph, read from its trees.
    fn dump(e: &Engine) -> (Vec<DNode>, Vec<DEdge>) {
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
        let name = |t: u64| e.token_name(t).unwrap().1;
        let prop = |ps: &[(u64, Value)], n: &str| {
            ps.iter()
                .find(|(k, _)| name(*k) == n)
                .and_then(|(_, v)| match v {
                    Value::Int(i) => Some(*i),
                    _ => None,
                })
        };
        let mut raw = Vec::new();
        tx.scan_prefix(g.nodes, b"", |k, v| {
            raw.push((element_of(k).unwrap(), record::decode(v).unwrap()));
            true
        })
        .unwrap();
        let nodes = raw
            .into_iter()
            .map(|(id, (labels, props))| DNode {
                id,
                labels: labels.into_iter().map(name).collect(),
                p: prop(&props, "p"),
                q: prop(&props, "q"),
            })
            .collect();
        let mut raw = Vec::new();
        tx.scan_prefix(g.edges, b"", |k, v| {
            raw.push((element_of(k).unwrap(), decode_edge(v).unwrap()));
            true
        })
        .unwrap();
        let edges = raw
            .into_iter()
            .map(|(_, r)| DEdge {
                src: r.src,
                dst: r.dst,
                ty: name(r.ty),
                w: prop(&r.props, "w"),
            })
            .collect();
        (nodes, edges)
    }

    #[derive(Debug, Clone, Copy)]
    enum Col {
        Node(usize, bool),
        Edge(usize),
        Opt,
    }

    #[derive(Debug, Clone)]
    enum Pred {
        Cmp(usize, bool, u8, i64),
        Null(usize, bool),
        Not(Box<Pred>),
        And(Box<Pred>, Box<Pred>),
        Or(Box<Pred>, Box<Pred>),
    }

    #[derive(Debug, Clone, Copy)]
    enum Agg {
        CountStar,
        Count(Col),
        Sum(Col),
        Min(Col),
        Max(Col),
    }

    #[derive(Debug, Clone)]
    struct Query {
        labels: Vec<Option<&'static str>>,
        specs: Vec<Option<i64>>,
        types: Vec<Option<&'static str>>,
        pred: Option<Pred>,
        optional: Option<(usize, Option<&'static str>)>,
        cols: Vec<Col>,
        distinct: bool,
        order: bool,
        group: Option<(Option<Col>, Vec<Agg>)>,
    }

    fn col_text(c: Col) -> String {
        match c {
            Col::Node(i, p) => format!("n{i}.{}", if p { "p" } else { "q" }),
            Col::Edge(i) => format!("e{i}.w"),
            Col::Opt => "x.q".into(),
        }
    }

    fn pred_text(p: &Pred) -> String {
        match p {
            Pred::Cmp(n, pp, op, c) => format!(
                "n{n}.{} {} {c}",
                if *pp { "p" } else { "q" },
                ["<", "=", ">=", "<>"][*op as usize]
            ),
            Pred::Null(n, pp) => format!("n{n}.{} IS NULL", if *pp { "p" } else { "q" }),
            Pred::Not(x) => format!("NOT ({})", pred_text(x)),
            Pred::And(a, b) => format!("({}) AND ({})", pred_text(a), pred_text(b)),
            Pred::Or(a, b) => format!("({}) OR ({})", pred_text(a), pred_text(b)),
        }
    }

    impl Query {
        fn text(&self) -> String {
            let mut s = String::from("MATCH ");
            for i in 0..self.labels.len() {
                if i > 0 {
                    s.push_str(&format!(
                        "-[e{}{}]->",
                        i - 1,
                        self.types[i - 1].map_or(String::new(), |t| format!(":{t}"))
                    ));
                }
                s.push_str(&format!(
                    "(n{i}{}{})",
                    self.labels[i].map_or(String::new(), |l| format!(":{l}")),
                    self.specs[i].map_or(String::new(), |v| format!(" {{p: {v}}}"))
                ));
            }
            if let Some(p) = &self.pred {
                s.push_str(&format!(" WHERE {}", pred_text(p)));
            }
            if let Some((n, t)) = self.optional {
                s.push_str(&format!(
                    " OPTIONAL MATCH (n{n})-[o{}]->(x)",
                    t.map_or(String::new(), |t| format!(":{t}"))
                ));
            }
            match &self.group {
                Some((key, aggs)) => {
                    let mut items = Vec::new();
                    if let Some(k) = key {
                        items.push(format!("{} AS k", col_text(*k)));
                    }
                    for (i, a) in aggs.iter().enumerate() {
                        items.push(match a {
                            Agg::CountStar => format!("count(*) AS c{i}"),
                            Agg::Count(c) => format!("count({}) AS c{i}", col_text(*c)),
                            Agg::Sum(c) => format!("sum({}) AS c{i}", col_text(*c)),
                            Agg::Min(c) => format!("min({}) AS c{i}", col_text(*c)),
                            Agg::Max(c) => format!("max({}) AS c{i}", col_text(*c)),
                        });
                    }
                    s.push_str(&format!(" RETURN {}", items.join(", ")));
                }
                None => {
                    let items: Vec<String> = self
                        .cols
                        .iter()
                        .enumerate()
                        .map(|(i, c)| format!("{} AS c{i}", col_text(*c)))
                        .collect();
                    s.push_str(&format!(
                        " RETURN {}{}",
                        if self.distinct { "DISTINCT " } else { "" },
                        items.join(", ")
                    ));
                    if self.order {
                        s.push_str(" ORDER BY c0");
                    }
                }
            }
            s
        }
    }

    fn gen_pred(rng: &mut StdRng, n: usize, depth: u32) -> Pred {
        match if depth == 0 {
            0
        } else {
            rng.random_range(0..5)
        } {
            0 | 1 => Pred::Cmp(
                rng.random_range(0..n),
                rng.random_bool(0.5),
                rng.random_range(0..4),
                rng.random_range(0..6),
            ),
            2 => Pred::Null(rng.random_range(0..n), rng.random_bool(0.5)),
            3 => Pred::Not(Box::new(gen_pred(rng, n, depth - 1))),
            _ => {
                let (a, b) = (gen_pred(rng, n, depth - 1), gen_pred(rng, n, depth - 1));
                if rng.random_bool(0.5) {
                    Pred::And(Box::new(a), Box::new(b))
                } else {
                    Pred::Or(Box::new(a), Box::new(b))
                }
            }
        }
    }

    fn gen_query(rng: &mut StdRng) -> Query {
        let n = rng.random_range(1..=3);
        let label = |rng: &mut StdRng| [None, Some("A"), Some("B")][rng.random_range(0..3)];
        let ty = |rng: &mut StdRng| [None, Some("T"), Some("U")][rng.random_range(0..3)];
        let optional = rng
            .random_bool(0.25)
            .then(|| (rng.random_range(0..n), ty(rng)));
        let col = |rng: &mut StdRng| match rng.random_range(0..6) {
            0 if n > 1 => Col::Edge(rng.random_range(0..n - 1)),
            1 if optional.is_some() => Col::Opt,
            _ => Col::Node(rng.random_range(0..n), rng.random_bool(0.5)),
        };
        let group = rng.random_bool(0.3).then(|| {
            let key = rng.random_bool(0.6).then(|| col(rng));
            let aggs = (0..rng.random_range(1..=3))
                .map(|_| match rng.random_range(0..5) {
                    0 => Agg::CountStar,
                    1 => Agg::Count(col(rng)),
                    2 => Agg::Sum(col(rng)),
                    3 => Agg::Min(col(rng)),
                    _ => Agg::Max(col(rng)),
                })
                .collect();
            (key, aggs)
        });
        Query {
            labels: (0..n).map(|_| label(rng)).collect(),
            specs: (0..n)
                .map(|_| rng.random_bool(0.2).then(|| rng.random_range(0..5)))
                .collect(),
            types: (0..n.saturating_sub(1)).map(|_| ty(rng)).collect(),
            pred: rng.random_bool(0.6).then(|| gen_pred(rng, n, 2)),
            optional,
            cols: (0..rng.random_range(1..=3)).map(|_| col(rng)).collect(),
            distinct: rng.random_bool(0.3),
            order: rng.random_bool(0.3),
            group,
        }
    }

    /// A binding: node indexes, edge indexes, and the optional neighbour.
    struct Binding {
        nodes: Vec<usize>,
        edges: Vec<usize>,
        opt: Option<usize>,
    }

    fn eval3(p: &Pred, b: &Binding, ns: &[DNode]) -> Option<bool> {
        let get = |n: usize, pp: bool| {
            let d = &ns[b.nodes[n]];
            if pp { d.p } else { d.q }
        };
        match p {
            Pred::Cmp(n, pp, op, c) => get(*n, *pp).map(|v| match op {
                0 => v < *c,
                1 => v == *c,
                2 => v >= *c,
                _ => v != *c,
            }),
            Pred::Null(n, pp) => Some(get(*n, *pp).is_none()),
            Pred::Not(x) => eval3(x, b, ns).map(|v| !v),
            Pred::And(x, y) => match (eval3(x, b, ns), eval3(y, b, ns)) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
            Pred::Or(x, y) => match (eval3(x, b, ns), eval3(y, b, ns)) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
        }
    }

    fn value(c: Col, b: &Binding, ns: &[DNode], es: &[DEdge]) -> Option<i64> {
        match c {
            Col::Node(n, p) => {
                let d = &ns[b.nodes[n]];
                if p { d.p } else { d.q }
            }
            Col::Edge(i) => es[b.edges[i]].w,
            Col::Opt => b.opt.and_then(|o| ns[o].q),
        }
    }

    fn v(x: Option<i64>) -> Value {
        x.map_or(Value::Null, Value::Int)
    }

    /// Every result row of `q`, by enumeration.
    fn oracle(q: &Query, ns: &[DNode], es: &[DEdge]) -> Vec<Vec<Value>> {
        let n = q.labels.len();
        let node_ok = |i: usize, d: &DNode| {
            q.labels[i].is_none_or(|l| d.labels.iter().any(|x| x == l))
                && q.specs[i].is_none_or(|c| d.p == Some(c))
        };
        let mut bindings = Vec::new();
        fn walk(
            q: &Query,
            ns: &[DNode],
            es: &[DEdge],
            node_ok: &dyn Fn(usize, &DNode) -> bool,
            cur: &mut Binding,
            out: &mut Vec<Binding>,
        ) {
            let i = cur.nodes.len();
            if i == q.labels.len() {
                out.push(Binding {
                    nodes: cur.nodes.clone(),
                    edges: cur.edges.clone(),
                    opt: None,
                });
                return;
            }
            if i == 0 {
                for (k, d) in ns.iter().enumerate() {
                    if node_ok(0, d) {
                        cur.nodes.push(k);
                        walk(q, ns, es, node_ok, cur, out);
                        cur.nodes.pop();
                    }
                }
                return;
            }
            let from = ns[cur.nodes[i - 1]].id;
            for (ei, e) in es.iter().enumerate() {
                if e.src != from
                    || cur.edges.contains(&ei)
                    || q.types[i - 1].is_some_and(|t| t != e.ty)
                {
                    continue;
                }
                let Some(k) = ns.iter().position(|d| d.id == e.dst) else {
                    continue;
                };
                if !node_ok(i, &ns[k]) {
                    continue;
                }
                cur.nodes.push(k);
                cur.edges.push(ei);
                walk(q, ns, es, node_ok, cur, out);
                cur.nodes.pop();
                cur.edges.pop();
            }
        }
        let mut cur = Binding {
            nodes: Vec::new(),
            edges: Vec::new(),
            opt: None,
        };
        walk(q, ns, es, &node_ok, &mut cur, &mut bindings);
        let _ = n;
        bindings.retain(|b| {
            q.pred
                .as_ref()
                .is_none_or(|p| eval3(p, b, ns) == Some(true))
        });
        if let Some((at, ty)) = q.optional {
            let mut out = Vec::new();
            for b in bindings {
                let from = ns[b.nodes[at]].id;
                let hits: Vec<usize> = es
                    .iter()
                    .filter(|e| e.src == from && ty.is_none_or(|t| t == e.ty))
                    .filter_map(|e| ns.iter().position(|d| d.id == e.dst))
                    .collect();
                if hits.is_empty() {
                    out.push(b);
                } else {
                    for h in hits {
                        out.push(Binding {
                            nodes: b.nodes.clone(),
                            edges: b.edges.clone(),
                            opt: Some(h),
                        });
                    }
                }
            }
            bindings = out;
        }
        match &q.group {
            None => {
                let mut rows: Vec<Vec<Value>> = bindings
                    .iter()
                    .map(|b| q.cols.iter().map(|c| v(value(*c, b, ns, es))).collect())
                    .collect();
                if q.distinct {
                    rows = sorted(rows);
                    rows.dedup();
                }
                rows
            }
            Some((key, aggs)) => {
                let mut groups: BTreeMap<Option<i64>, Vec<&Binding>> = BTreeMap::new();
                for b in &bindings {
                    groups
                        .entry(key.and_then(|k| value(k, b, ns, es)))
                        .or_default()
                        .push(b);
                }
                if groups.is_empty() && key.is_none() {
                    groups.insert(None, Vec::new());
                }
                groups
                    .into_iter()
                    .map(|(k, bs)| {
                        let mut row = Vec::new();
                        if key.is_some() {
                            row.push(v(k));
                        }
                        for a in aggs {
                            let vals = |c: Col| -> Vec<i64> {
                                bs.iter().filter_map(|b| value(c, b, ns, es)).collect()
                            };
                            row.push(match a {
                                Agg::CountStar => Value::Int(bs.len() as i64),
                                Agg::Count(c) => Value::Int(vals(*c).len() as i64),
                                Agg::Sum(c) => {
                                    let xs = vals(*c);
                                    if xs.is_empty() {
                                        Value::Null
                                    } else {
                                        Value::Int(xs.iter().sum())
                                    }
                                }
                                Agg::Min(c) => v(vals(*c).into_iter().min()),
                                Agg::Max(c) => v(vals(*c).into_iter().max()),
                            });
                        }
                        row
                    })
                    .collect()
            }
        }
    }

    fn sorted(mut r: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
        r.sort_by(|a, b| {
            for (x, y) in a.iter().zip(b) {
                let o = total_cmp(x, y);
                if o.is_ne() {
                    return o;
                }
            }
            a.len().cmp(&b.len())
        });
        r
    }

    /// A seeded random graph of integer properties.
    fn graph(e: &Engine, rng: &mut StdRng) {
        let n = rng.random_range(8..16);
        let mut text = String::from("INSERT ");
        let prop = |rng: &mut StdRng, name: &str| {
            if rng.random_range(0..4) == 0 {
                String::new()
            } else {
                format!("{name}: {}", rng.random_range(0..5))
            }
        };
        for i in 0..n {
            let labels = ["", ":A", ":B", ":A&B"][rng.random_range(0..4)];
            let props: Vec<String> = [prop(rng, "p"), prop(rng, "q")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            if i > 0 {
                text.push_str(", ");
            }
            let props = if props.is_empty() {
                String::new()
            } else {
                format!(" {{{}}}", props.join(", "))
            };
            text.push_str(&format!("(n{i}{labels}{props})"));
        }
        for _ in 0..rng.random_range(n..2 * n) {
            let (a, b) = (rng.random_range(0..n), rng.random_range(0..n));
            let ty = if rng.random_bool(0.5) { "T" } else { "U" };
            let w = prop(rng, "w");
            let w = if w.is_empty() { w } else { format!(" {{{w}}}") };
            text.push_str(&format!(", (n{a})-[:{ty}{w}]->(n{b})"));
        }
        super::support::rows(e, &text);
    }

    /// rmp #45: for 5 000 seeded queries over seeded graphs, the results
    /// equal the reference evaluator's (seed printed).
    #[test]
    fn five_thousand_seeded_queries_equal_the_reference_evaluator() {
        let s = seed();
        println!("seed {s}");
        let mut rng = StdRng::seed_from_u64(s);
        let mut checked = 0;
        let mut nonempty = 0;
        for _ in 0..5 {
            let (_d, e) = db();
            graph(&e, &mut rng);
            let (ns, es) = dump(&e);
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            for _ in 0..1000 {
                let q = gen_query(&mut rng);
                let text = q.text();
                let got = rows_in(&mut tx, &text, &Params::new())
                    .unwrap_or_else(|err| panic!("seed {s}: {text}: {err}"));
                if q.order && q.group.is_none() {
                    for w in got.windows(2) {
                        assert!(
                            crate::query::expr::sort_cmp(&w[0][0], &w[1][0], false).is_le(),
                            "seed {s}: {text}: not sorted"
                        );
                    }
                }
                let want = oracle(&q, &ns, &es);
                assert_eq!(sorted(got.clone()), sorted(want), "seed {s}: {text}");
                nonempty += usize::from(!got.is_empty());
                checked += 1;
            }
            tx.commit().unwrap();
        }
        assert_eq!(checked, 5000);
        // The generated queries are not vacuous.
        assert!(nonempty > 2500, "{nonempty} non-empty results");
    }
}

mod execution {
    //! Streaming, one thread, PROFILE and memory (rmp #45).

    use super::support::{rows, session};
    use crate::catalog::TokenKind;
    use crate::query::exec::{BATCH, Params, collect, profile, run};
    use crate::tx::Isolation;
    use crate::tx::tests::{db, db_with_cap};
    use crate::types::Value;

    fn insert_nodes(e: &crate::storage::engine::Engine, n: usize, label: &str) {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(crate::catalog::DEFAULT_GRAPH_ID).unwrap();
        let l = e.token(TokenKind::Label, label).unwrap();
        let k = e.token(TokenKind::PropertyKey, "k").unwrap();
        for i in 0..n {
            tx.create_node(&g, &[l], &[(k, Value::Int(i as i64))])
                .unwrap();
        }
        tx.commit().unwrap();
    }

    /// FR-API-010: a scan reads in chunks of one batch, so the first batch
    /// of a 3 000-node scan comes after the scan produced 1 024 rows; and
    /// a query of 1 000 000 rows returns its first batch after each of its
    /// scans produced at most 1 024 rows.
    #[test]
    fn the_first_batch_comes_after_at_most_one_batch_of_scanned_rows() {
        let (_d, e) = db();
        insert_nodes(&e, 3000, "N");
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = run("MATCH (a) RETURN a", &mut tx, &session(), &Params::new()).unwrap();
        let b = c.next_batch(&mut tx).unwrap().unwrap();
        assert_eq!(b.len(), BATCH);
        assert_eq!(c.scan_rows(), BATCH as u64);
        assert_eq!(collect(&mut c, &mut tx).unwrap().len(), 3000 - BATCH);
        drop(tx);

        let (_d, e) = db();
        insert_nodes(&e, 1000, "N");
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = run(
            "MATCH (a), (b) RETURN a, b",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        let first = c.next_batch(&mut tx).unwrap().unwrap();
        assert_eq!(first.len(), BATCH);
        // The left scan ran once (1 000 rows) and the right scan twice
        // (1 000 rows each): every scan run produced at most 1 024 rows, and
        // 3 000 rows of 1 000 000 were produced before the first batch.
        assert_eq!(c.scan_rows(), 3000);
        let mut total = first.len();
        while let Some(b) = c.next_batch(&mut tx).unwrap() {
            assert!(b.len() <= BATCH);
            total += b.len();
        }
        assert_eq!(total, 1_000_000);
    }

    /// NFR-PERF-007: across 1 000 batches, the number of operators running
    /// at once never exceeds 1.
    #[test]
    fn across_a_thousand_batches_at_most_one_operator_runs_at_once() {
        let (_d, e) = db();
        insert_nodes(&e, 1013, "N");
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = profile(
            "MATCH (a), (b) RETURN a, b",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        let mut batches = 0;
        while let Some(_b) = c.next_batch(&mut tx).unwrap() {
            batches += 1;
            assert_eq!(c.max_running(), Some(1));
        }
        assert!(batches >= 1000, "{batches}");
        assert_eq!(c.max_running(), Some(1));
    }

    /// FR-OBS-006: profiling a query that returns 10 rows returns the rows
    /// and a final operator reporting 10 rows; each operator's entries read
    /// equals the transaction layer's independent count.
    #[test]
    fn profile_reports_rows_and_entries_read_per_operator() {
        let (_d, e) = db();
        let mut text = String::from("INSERT ");
        for i in 0..10 {
            if i > 0 {
                text.push_str(", ");
            }
            text.push_str(&format!("(:P {{v: {i}}})-[:K]->(:Q {{w: {}}})", i * 10));
        }
        text.push_str(", (:Q {w: -1})");
        rows(&e, &text);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = profile(
            "MATCH (p:P)-[:K]->(q) WHERE q.w >= 0 RETURN p.v AS v, q.w AS w",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        let r = collect(&mut c, &mut tx).unwrap();
        assert_eq!(r.len(), 10);
        let d = c.profile().unwrap();
        assert_eq!(d.name, "ProduceResults");
        assert_eq!(d.profile.unwrap().rows, 10);
        let mut all = Vec::new();
        fn walk<'a>(
            d: &'a crate::query::plan::Description,
            out: &mut Vec<&'a crate::query::plan::Description>,
        ) {
            out.push(d);
            for c in &d.children {
                walk(c, out);
            }
        }
        walk(&d, &mut all);
        let mut total = 0;
        for n in &all {
            let p = n.profile.unwrap();
            assert_eq!(p.entries_read, p.tx_entries, "{} {}", n.name, n.details);
            total += p.entries_read;
        }
        // 21 node records and 10 edges were read at least once.
        assert!(total >= 31, "{total}");
        assert!(
            all.iter()
                .any(|n| n.name.starts_with("Expand") && n.profile.unwrap().entries_read >= 10)
        );
        // A normal plan holds no decorator: run returns no profile.
        let mut c = run("MATCH (p:P) RETURN p", &mut tx, &session(), &Params::new()).unwrap();
        collect(&mut c, &mut tx).unwrap();
        assert!(c.profile().is_none());
    }

    /// FR-LIM-003: a sort and an aggregation over an input larger than the
    /// memory cap fail with 51N72, the transaction is aborted, and the
    /// accountant never exceeds the cap.
    #[test]
    fn a_sort_and_an_aggregation_beyond_the_cap_fail_and_abort() {
        const CAP: u64 = 8 << 20;
        for text in [
            "MATCH (a), (b) RETURN a, b ORDER BY a",
            "MATCH (a), (b) RETURN a, b, count(*) AS c",
        ] {
            let (_d, e) = db_with_cap(CAP);
            insert_nodes(&e, 400, "N");
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let mut c = run(text, &mut tx, &session(), &Params::new()).unwrap();
            let err = collect(&mut c, &mut tx).unwrap_err();
            assert_eq!(err.code(), "51N72", "{text}: {err}");
            assert!(
                tx.check_active().is_err(),
                "{text}: the transaction is aborted"
            );
            let next = run("MATCH (a) RETURN a", &mut tx, &session(), &Params::new()).unwrap_err();
            assert_eq!(next.code(), "25N05");
            let acc = &e.core().accountant;
            assert!(
                acc.peak() <= acc.cap(),
                "{text}: peak {} cap {}",
                acc.peak(),
                acc.cap()
            );
        }
    }
}

mod catalog {
    //! Catalog statements, multi-graph queries and closed graph types
    //! (rmp #46).

    use super::support::{error, rows, rows_in, session};
    use crate::catalog::{Kind, ROOT};
    use crate::query::exec::{Params, run};
    use crate::test_support::OWNER;
    use crate::tx::Isolation;
    use crate::tx::tests::{db, reopen};
    use crate::types::Value;

    /// FR-GQL-004: a closed graph type rejects an inserted node whose label
    /// it does not allow, and the same statement succeeds on an open graph;
    /// the open graph runs the check 0 times.
    #[test]
    fn a_closed_graph_type_rejects_what_it_does_not_describe() {
        let (_d, e) = db();
        rows(
            &e,
            "CREATE GRAPH c { (:Person {name STRING, age INT})-[:Knows {since INT}]->(:Person) }",
        );
        let err = error(&e, "USE c INSERT (:Robot {name: 'r'})");
        assert_eq!(err.code(), "G2000", "{err}");
        // An undeclared property, a wrong property type, an edge type not
        // declared.
        assert_eq!(
            error(&e, "USE c INSERT (:Person {nick: 'x'})").code(),
            "G2000"
        );
        assert_eq!(
            error(&e, "USE c INSERT (:Person {age: 'x'})").code(),
            "G2000"
        );
        assert_eq!(
            error(&e, "USE c INSERT (:Person)-[:Likes]->(:Person)").code(),
            "G2000"
        );
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut cur = run(
            "USE c INSERT (:Person {name: 'a', age: 3})-[:Knows {since: 1}]->(:Person)",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        crate::query::exec::collect(&mut cur, &mut tx).unwrap();
        assert_eq!(cur.type_checks(), 1);
        let mut cur = run(
            "USE lpg INSERT (:Robot {name: 'r'})",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        crate::query::exec::collect(&mut cur, &mut tx).unwrap();
        assert_eq!(cur.type_checks(), 0);
        tx.commit().unwrap();
        assert_eq!(
            rows(&e, "USE c MATCH (p:Person) RETURN count(*) AS n"),
            vec![vec![Value::Int(2)]]
        );
        assert_eq!(
            rows(&e, "MATCH (r:Robot) RETURN r.name AS n"),
            vec![vec![Value::string("r")]]
        );
    }

    /// A failed closed-type check leaves none of the statement's writes.
    #[test]
    fn a_failed_closed_type_check_leaves_none_of_the_statement_writes() {
        let (_d, e) = db();
        rows(&e, "CREATE GRAPH c { (:Person {name STRING}) }");
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let err = rows_in(
            &mut tx,
            "USE c INSERT (:Person {name: 'ok'}), (:Person {name: 'ok2'}), (:Robot)",
            &Params::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "G2000");
        assert!(tx.check_active().is_err());
        drop(tx);
        assert_eq!(
            rows(&e, "USE c MATCH (p) RETURN count(*) AS n"),
            vec![vec![Value::Int(0)]]
        );
    }

    /// FR-CAT-004: a transaction that creates a graph and inserts nodes
    /// into it, then rolls back, leaves neither.
    #[test]
    fn a_rolled_back_transaction_leaves_neither_the_graph_nor_its_nodes() {
        let (_d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        rows_in(&mut tx, "CREATE GRAPH g ANY", &Params::new()).unwrap();
        rows_in(
            &mut tx,
            "USE g INSERT (:N {x: 1}), (:N {x: 2})",
            &Params::new(),
        )
        .unwrap();
        assert_eq!(
            rows_in(
                &mut tx,
                "USE g MATCH (n:N) RETURN count(*) AS c",
                &Params::new()
            )
            .unwrap(),
            vec![vec![Value::Int(2)]]
        );
        tx.abort();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert!(tx.resolve(&["public", "g"]).unwrap().is_none());
        drop(tx);
        assert_eq!(error(&e, "USE g MATCH (n) RETURN n").code(), "42002");
        assert_eq!(
            rows(&e, "MATCH (n:N) RETURN count(*) AS c"),
            vec![vec![Value::Int(0)]]
        );
    }

    /// FR-CAT-003: one query reads two graphs and its result combines both.
    #[test]
    fn one_query_reads_two_graphs() {
        let (_d, e) = db();
        rows(&e, "CREATE GRAPH g2 ANY");
        rows(&e, "INSERT (:A {x: 1}), (:A {x: 2})");
        rows(&e, "USE g2 INSERT (:B {y: 10})");
        let mut r = rows(
            &e,
            "USE lpg MATCH (a:A) USE g2 MATCH (b:B) RETURN a.x AS x, b.y AS y",
        );
        r.sort_by(|p, q| crate::types::total_cmp(&p[0], &q[0]));
        assert_eq!(
            r,
            vec![
                vec![Value::Int(1), Value::Int(10)],
                vec![Value::Int(2), Value::Int(10)]
            ]
        );
    }

    /// FR-CAT-001 (statement part): a directory (through the API), a schema
    /// in it, a graph type and two graphs created in one transaction are
    /// present after reopen, owned by the session's principal.
    #[test]
    fn catalog_objects_created_in_one_transaction_survive_reopen() {
        let (d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.create_directory(ROOT, "dir", OWNER).unwrap();
        for text in [
            "CREATE SCHEMA /dir/s",
            "CREATE GRAPH TYPE /dir/s/t { (:A {p STRING}) }",
            "CREATE GRAPH /dir/s/g1 TYPED /dir/s/t",
            "CREATE GRAPH /dir/s/g2 ANY",
            "USE /dir/s/g1 INSERT (:A {p: 'x'})",
        ] {
            rows_in(&mut tx, text, &Params::new()).unwrap_or_else(|err| panic!("{text}: {err}"));
        }
        tx.commit().unwrap();
        drop(e);
        let e = reopen(d.path());
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for (path, kind) in [
            (&["dir"][..], Kind::Directory),
            (&["dir", "s"][..], Kind::Schema),
            (&["dir", "s", "t"][..], Kind::GraphType),
            (&["dir", "s", "g1"][..], Kind::Graph),
            (&["dir", "s", "g2"][..], Kind::Graph),
        ] {
            let o = tx
                .resolve(path)
                .unwrap()
                .unwrap_or_else(|| panic!("{path:?}"));
            assert_eq!(o.kind, kind);
            assert_eq!(o.owner, OWNER);
        }
        drop(tx);
        assert_eq!(
            rows(&e, "USE /dir/s/g1 MATCH (a:A) RETURN a.p AS p"),
            vec![vec![Value::string("x")]]
        );
        // The type is closed after reopen too.
        assert_eq!(error(&e, "USE /dir/s/g1 INSERT (:B)").code(), "G2000");
    }

    #[test]
    fn drop_statements_and_their_errors() {
        let (_d, e) = db();
        rows(&e, "CREATE SCHEMA /s");
        rows(&e, "CREATE GRAPH /s/g { (:A) }");
        assert_eq!(error(&e, "CREATE GRAPH /s/g ANY").code(), "42N11");
        rows(&e, "CREATE OR REPLACE GRAPH /s/g ANY");
        assert_eq!(error(&e, "DROP SCHEMA /s").code(), "G1000");
        rows(&e, "DROP GRAPH /s/g");
        rows(&e, "DROP SCHEMA /s");
        assert_eq!(error(&e, "DROP GRAPH /s/g").code(), "42002");
        rows(&e, "CREATE GRAPH TYPE t { (:A) }");
        rows(&e, "CREATE GRAPH h TYPED t");
        assert_eq!(error(&e, "DROP GRAPH TYPE t").code(), "G1000");
        rows(&e, "CREATE GRAPH TYPE t2 COPY OF t");
        rows(&e, "CREATE GRAPH h2 TYPED t2");
        assert_eq!(error(&e, "USE h2 INSERT (:B)").code(), "G2000");
    }
}

mod cursor {
    //! What the cursor gives the API of rmp #47 and the cancellation of
    //! rmp #49.

    use super::support::session;
    use crate::error::Error;
    use crate::gqlstatus::codes;
    use crate::query::bound::StatementKind;
    use crate::query::exec::{Params, collect, run};
    use crate::tx::Isolation;
    use crate::tx::tests::db;

    fn assert_send<T: Send + 'static>() {}

    #[test]
    fn the_cursor_is_send_and_reports_columns_kind_control_and_counters() {
        assert_send::<crate::query::exec::Cursor>();
        let (_d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = run(
            "START TRANSACTION INSERT (a:A {x: 1})-[:T]->(:B) RETURN a.x AS x COMMIT",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        assert_eq!(c.columns(), vec!["x"]);
        assert_eq!(c.kind(), StatementKind::Write);
        assert!(c.control().start);
        assert_eq!(
            c.control().end,
            Some(crate::query::ast::EndTransaction::Commit)
        );
        collect(&mut c, &mut tx).unwrap();
        let n = c.counters();
        assert_eq!(
            (n.nodes_created, n.edges_created, n.properties_set),
            (2, 1, 1)
        );
        let c = run("MATCH (a) RETURN a", &mut tx, &session(), &Params::new()).unwrap();
        assert_eq!(c.kind(), StatementKind::Read);
    }

    /// The status check runs before each batch and every 1 024 loop
    /// iterations; its error fails the statement and aborts the
    /// transaction.
    #[test]
    fn a_failing_status_check_fails_the_statement_and_aborts_the_transaction() {
        let (_d, e) = db();
        super::support::rows(&e, "INSERT (:N), (:N), (:N)");
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = run(
            "MATCH (a), (b), (c) RETURN a",
            &mut tx,
            &session(),
            &Params::new(),
        )
        .unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        c.set_check(std::sync::Arc::new(move |_| {
            if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                Err(Error::new(codes::C25N14, "cancelled"))
            } else {
                Ok(())
            }
        }));
        assert!(c.next_batch(&mut tx).unwrap().is_some());
        let err = c.next_batch(&mut tx).unwrap_err();
        assert_eq!(err.code(), "25N14");
        assert!(tx.check_active().is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}

mod memory {
    //! The blocking operators on one input (rmp #45 evidence): each test
    //! profiles one blocking operator over the same 10 000 rows (the
    //! product of 100 nodes with themselves) and prints its peak
    //! reservation; `heaptrack-tests.sh` measures each test's peak heap.

    use super::support::session;
    use crate::catalog::TokenKind;
    use crate::query::exec::{Params, collect, profile};
    use crate::query::plan::Description;
    use crate::tx::Isolation;
    use crate::tx::tests::db;
    use crate::types::Value;

    fn find<'a>(d: &'a Description, name: &str) -> Option<&'a Description> {
        if d.name == name {
            return Some(d);
        }
        d.children.iter().find_map(|c| find(c, name))
    }

    /// Runs `text` profiled over 100 nodes and returns the rows and the
    /// operator `name`'s peak reservation.
    fn peak(text: &str, name: &str) -> (usize, u64) {
        let (_d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(crate::catalog::DEFAULT_GRAPH_ID).unwrap();
        let l = e.token(TokenKind::Label, "N").unwrap();
        let k = e.token(TokenKind::PropertyKey, "k").unwrap();
        for i in 0..100 {
            tx.create_node(&g, &[l], &[(k, Value::Int(i % 10))])
                .unwrap();
        }
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut c = profile(text, &mut tx, &session(), &Params::new()).unwrap();
        let n = collect(&mut c, &mut tx).unwrap().len();
        let d = c.profile().unwrap();
        let p = find(&d, name)
            .unwrap_or_else(|| panic!("{name}: {d:#?}"))
            .profile
            .unwrap();
        println!("{name}\trows {n}\tpeak reservation {} bytes", p.peak_memory);
        (n, p.peak_memory)
    }

    #[test]
    fn blocking_sort_peak() {
        let (n, p) = peak("MATCH (a), (b) RETURN a, b ORDER BY a", "Sort");
        assert_eq!(n, 10_000);
        assert!(p >= crate::query::exec::CHUNK);
    }

    #[test]
    fn blocking_aggregate_peak() {
        let (n, p) = peak(
            "MATCH (a), (b) RETURN a, b, count(*) AS c",
            "EagerAggregation",
        );
        assert_eq!(n, 10_000);
        assert!(p >= crate::query::exec::CHUNK);
    }

    #[test]
    fn blocking_distinct_peak() {
        let (n, p) = peak("MATCH (a), (b) RETURN DISTINCT a, b", "Distinct");
        assert_eq!(n, 10_000);
        assert!(p >= crate::query::exec::CHUNK);
    }

    #[test]
    fn blocking_insert_peak() {
        let (n, p) = peak("MATCH (a), (b) INSERT (a)-[:T]->(b) RETURN a", "Create");
        assert_eq!(n, 10_000);
        assert!(p >= crate::query::exec::CHUNK);
    }
}

mod statements {
    //! Statements of the approved set end to end (rmp #42–#45).

    use super::support::{error, rows, rows_in};
    use crate::query::exec::Params;
    use crate::test_support::OWNER;
    use crate::tx::Isolation;
    use crate::tx::tests::db;
    use crate::types::Value;

    #[test]
    fn set_remove_session_user_clock_and_select_run_end_to_end() {
        let (_d, e) = db();
        rows(&e, "INSERT (:N {a: 1, b: 2})-[:T {w: 1}]->(:M {a: 5})");
        assert_eq!(
            rows(&e, "MATCH (n:N) SET n = {c: 3} RETURN n.a AS a, n.c AS c"),
            vec![vec![Value::Null, Value::Int(3)]]
        );
        assert_eq!(
            rows(&e, "MATCH (n:N) REMOVE n.c RETURN n.c AS c"),
            vec![vec![Value::Null]]
        );
        assert_eq!(
            rows(
                &e,
                "MATCH (:N)-[t:T]->() SET t.w = t.w + 1 REMOVE t.x RETURN t.w AS w"
            ),
            vec![vec![Value::Int(2)]]
        );
        assert_eq!(
            rows(&e, "RETURN SESSION_USER AS u"),
            vec![vec![Value::string(OWNER)]]
        );
        let r = rows(
            &e,
            "RETURN CURRENT_DATE AS d, CURRENT_DATE = CURRENT_DATE AS s",
        );
        assert!(matches!(r[0][0], Value::Date(_)) && r[0][1] == Value::Bool(true));
        assert_eq!(
            rows(&e, "SELECT m.a AS a FROM lpg MATCH (m:M) WHERE m.a > 1"),
            vec![vec![Value::Int(5)]]
        );
        // EXISTS with a nested query: its filter, projection, aggregation
        // and sort run again for every outer row.
        rows(&e, "INSERT (:N {a: 7}), (:N {a: 8})");
        let mut r = rows(
            &e,
            "MATCH (n:N) WHERE EXISTS { MATCH (n)-[:T]->(m) WHERE m.a > 1 RETURN DISTINCT m.a AS x, count(*) AS c ORDER BY x } RETURN count(*) AS c",
        );
        r.sort_by(|p, q| crate::types::total_cmp(&p[0], &q[0]));
        assert_eq!(r, vec![vec![Value::Int(1)]]);
        // An insert from an optional match's NULL node fails with 22004.
        assert_eq!(
            error(
                &e,
                "MATCH (n:M) OPTIONAL MATCH (n)-[:T]->(x) INSERT (x)-[:T]->(:Z)"
            )
            .code(),
            "22004"
        );
        // A list parameter holding a NULL compares as UNKNOWN.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let mut p = Params::new();
        p.insert(
            "l".into(),
            Value::List(vec![Value::Int(1), Value::Null].into()),
        );
        assert_eq!(
            rows_in(&mut tx, "RETURN $l < $l AS lt, $l = $l AS eq", &p).unwrap(),
            vec![vec![Value::Null, Value::Null]]
        );
        // Setting a property to a value that is not a property value.
        p.insert(
            "bad".into(),
            Value::List(vec![Value::Int(1), Value::string("a")].into()),
        );
        assert_eq!(
            rows_in(&mut tx, "MATCH (n:M) SET n.p = $bad", &p)
                .unwrap_err()
                .code(),
            "22G03"
        );
    }
}
