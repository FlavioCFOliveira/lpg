//! Tests of indexes (rmp #33) and constraints (rmp #34), and the `loom`
//! model of the side-tree protocol.

#[cfg(not(any(loom, shuttle)))]
mod index {
    use std::collections::BTreeSet;

    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};

    use super::super::build::{Hook, Phase};
    use super::super::*;
    use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID, TokenKind};
    use crate::storage::btree::Write;
    use crate::storage::error::Error;
    use crate::sync::{Arc, Mutex, mpsc};
    use crate::test_support::{OWNER, graph_dir, mid_batch, seed};
    use crate::tx::Isolation;
    use crate::tx::conflict::WriteKeys;
    use crate::tx::tests::{db, pause_gc, reopen, until_waiting};
    use crate::tx::versions::version_bytes;
    use crate::types::spatial::Srid;
    use crate::types::total_cmp;

    pub(super) fn assert_intact(e: &Engine) {
        let p = e.check_integrity().unwrap();
        assert!(p.is_empty(), "{p:?}");
    }

    fn graph(tx: &mut Transaction<'_>) -> Graph {
        tx.graph(DEFAULT_GRAPH_ID).unwrap()
    }

    fn tok(e: &Engine, kind: TokenKind, name: &str) -> u64 {
        e.token(kind, name).unwrap()
    }

    /// The pause hook of a test: at each phase of `phases`, the build sends
    /// the phase on `reached` and waits for a message on `resume`.
    pub(super) struct Gate {
        pub(super) reached: mpsc::Receiver<Phase>,
        pub(super) resume: mpsc::Sender<()>,
    }

    pub(super) fn gate(e: &Engine, phases: &[Phase]) -> Gate {
        let (rt, rr) = mpsc::channel();
        let (st, sr) = mpsc::channel::<()>();
        let sr = Mutex::new(sr);
        let phases = phases.to_vec();
        let hook: Hook = Arc::new(move |p| {
            if phases.contains(&p) {
                let _ = rt.send(p);
                let _ = sr.lock().unwrap().recv();
            }
        });
        *e.core().builds.hook.lock().unwrap() = Some(hook);
        Gate {
            reached: rr,
            resume: st,
        }
    }

    impl Gate {
        pub(super) fn wait(&self, p: Phase) {
            let got = self
                .reached
                .recv_timeout(crate::test_support::WAIT_BOUND)
                .expect("the build reaches its pause point");
            assert_eq!(got, p);
        }
    }

    /// Every element of `entity` of `g` that `tx` sees, with its record.
    fn scan(tx: &mut Transaction<'_>, g: &Graph, entity: Entity) -> Vec<(u64, Element)> {
        let tree = match entity {
            Entity::Node => g.nodes,
            Entity::Edge => g.edges,
        };
        let mut out = Vec::new();
        tx.scan_prefix(tree, b"", |k, v| {
            out.push((element_of(k).unwrap(), decode_element(entity, v).unwrap()));
            true
        })
        .unwrap();
        out
    }

    fn get(el: &Element, k: u64) -> Option<&Value> {
        el.props
            .iter()
            .find(|(p, v)| *p == k && *v != Value::Null)
            .map(|(_, v)| v)
    }

    fn enc(v: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        key::encode(v, &mut out);
        out
    }

    fn succ(b: &[u8]) -> Option<Vec<u8>> {
        crate::tx::conflict::Interval::prefix(b).hi
    }

    /// The tokens of the index tests.
    struct Tokens {
        l: u64,
        m: u64,
        t: u64,
        u: u64,
        p: u64,
        q: u64,
        loc: u64,
        w: u64,
    }

    fn tokens(e: &Engine) -> Tokens {
        Tokens {
            l: tok(e, TokenKind::Label, "L"),
            m: tok(e, TokenKind::Label, "M"),
            t: tok(e, TokenKind::EdgeType, "T"),
            u: tok(e, TokenKind::EdgeType, "U"),
            p: tok(e, TokenKind::PropertyKey, "p"),
            q: tok(e, TokenKind::PropertyKey, "q"),
            loc: tok(e, TokenKind::PropertyKey, "loc"),
            w: tok(e, TokenKind::PropertyKey, "w"),
        }
    }

    /// A value of property `p`: integers and floats of a few values, so
    /// that 2 and 2.0 meet, a string, or nothing.
    fn small_value(rng: &mut StdRng) -> Option<Value> {
        match rng.random_range(0..6) {
            0 => None,
            1 => Some(Value::Float(f64::from(rng.random_range(0..5)))),
            2 => Some(Value::string(&format!("s{}", rng.random_range(0..4)))),
            _ => Some(Value::Int(rng.random_range(0..5))),
        }
    }

    fn node_props(rng: &mut StdRng, k: &Tokens) -> Vec<(u64, Value)> {
        let mut props = Vec::new();
        if let Some(v) = small_value(rng) {
            props.push((k.p, v));
        }
        if rng.random_bool(0.7) {
            props.push((k.q, Value::Int(rng.random_range(0..3))));
        }
        if rng.random_bool(0.6) {
            let pt = Point::new(
                Srid::Cartesian2D,
                &[rng.random_range(-50.0..50.0), rng.random_range(-50.0..50.0)],
            )
            .unwrap();
            props.push((k.loc, Value::Point(Box::new(pt))));
        }
        props
    }

    fn node_labels(rng: &mut StdRng, k: &Tokens) -> Vec<u64> {
        match rng.random_range(0..4) {
            0 => vec![],
            1 => vec![k.m],
            2 => vec![k.l, k.m],
            _ => vec![k.l],
        }
    }

    /// Seeds `n` random nodes and `n` random edges between them into `g`.
    fn seed_graph(e: &Engine, rng: &mut StdRng, k: &Tokens, graph_id: u64, n: usize) -> Vec<u64> {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(graph_id).unwrap();
        let mut nodes = Vec::new();
        for _ in 0..n {
            let labels = node_labels(rng, k);
            let props = node_props(rng, k);
            nodes.push(tx.create_node(&g, &labels, &props).unwrap());
        }
        for _ in 0..n {
            let (a, b) = (nodes[rng.random_range(0..n)], nodes[rng.random_range(0..n)]);
            let ty = if rng.random_bool(0.5) { k.t } else { k.u };
            let props: Vec<(u64, Value)> = small_value(rng).map(|v| (k.w, v)).into_iter().collect();
            tx.create_edge(&g, a, b, ty, &props).unwrap();
        }
        tx.commit().unwrap();
        nodes
    }

    /// The indexes of the tests: node and edge lookups, a range index on
    /// `L.p`, a composite on `L.(p, q)`, a range on `T.w` and a point index
    /// on `L.loc`.
    struct Indexes {
        node_lookup: u64,
        edge_lookup: u64,
        range: u64,
        composite: u64,
        edge_range: u64,
        point: u64,
    }

    fn create_indexes(e: &Engine, k: &Tokens, graph: u64) -> Indexes {
        let ix = Indexes {
            node_lookup: e
                .create_index(graph, SchemaKind::Lookup, Entity::Node, 0, &[])
                .unwrap(),
            edge_lookup: e
                .create_index(graph, SchemaKind::Lookup, Entity::Edge, 0, &[])
                .unwrap(),
            range: e
                .create_index(graph, SchemaKind::Range, Entity::Node, k.l, &[k.p])
                .unwrap(),
            composite: e
                .create_index(graph, SchemaKind::Range, Entity::Node, k.l, &[k.p, k.q])
                .unwrap(),
            edge_range: e
                .create_index(graph, SchemaKind::Range, Entity::Edge, k.t, &[k.w])
                .unwrap(),
            point: e
                .create_index(graph, SchemaKind::Point, Entity::Node, k.l, &[k.loc])
                .unwrap(),
        };
        for id in [
            ix.node_lookup,
            ix.edge_lookup,
            ix.range,
            ix.composite,
            ix.edge_range,
            ix.point,
        ] {
            e.wait_index(id).unwrap();
            assert_eq!(e.core().catalog.schema()[&id].state, State::Online);
        }
        ix
    }

    /// Checks seeded queries of every index of `ix` against full scans in
    /// one transaction, which first makes changes of its own.
    fn assert_queries_match_scans(e: &Engine, rng: &mut StdRng, k: &Tokens, ix: &Indexes) {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        // Changes of the transaction itself, which the index entries do not
        // hold yet.
        let nodes: Vec<u64> = scan(&mut tx, &g, Entity::Node)
            .iter()
            .map(|x| x.0)
            .collect();
        for _ in 0..5 {
            let labels = node_labels(rng, k);
            let props = node_props(rng, k);
            tx.create_node(&g, &labels, &props).unwrap();
        }
        tx.set_property(&g, nodes[0], k.p, Value::Int(3)).unwrap();
        tx.add_label(&g, nodes[1], k.l).unwrap();
        tx.delete_node(&g, nodes[2], true).unwrap();
        let ns = scan(&mut tx, &g, Entity::Node);
        let es = scan(&mut tx, &g, Entity::Edge);
        let ids = |v: Vec<u64>| v.into_iter().collect::<BTreeSet<u64>>();
        // Lookups.
        for (index, all, token) in [
            (ix.node_lookup, &ns, k.l),
            (ix.node_lookup, &ns, k.m),
            (ix.edge_lookup, &es, k.t),
            (ix.edge_lookup, &es, k.u),
        ] {
            let want: BTreeSet<u64> = all
                .iter()
                .filter(|(_, el)| el.tokens.contains(&token))
                .map(|x| x.0)
                .collect();
            let got = tx
                .index_scan(
                    &g,
                    index,
                    &token.to_be_bytes(),
                    Some(&(token + 1).to_be_bytes()),
                )
                .unwrap();
            assert_eq!(ids(got), want, "lookup {index} of token {token}");
        }
        let values: Vec<Value> = (0..5)
            .map(Value::Int)
            .chain((0..5).map(|i| Value::Float(f64::from(i) + 0.5)))
            .chain((0..4).map(|i| Value::string(&format!("s{i}"))))
            .collect();
        for _ in 0..40 {
            let a = &values[rng.random_range(0..values.len())];
            let b = &values[rng.random_range(0..values.len())];
            // Equality on the single-property and composite indexes.
            let eq = |el: &Element, prop: u64, label: u64, also: Option<u64>| {
                el.tokens.contains(&label)
                    && get(el, prop).is_some_and(|v| total_cmp(v, a).is_eq())
                    && also.is_none_or(|q| get(el, q).is_some())
            };
            let want: BTreeSet<u64> = ns
                .iter()
                .filter(|(_, el)| eq(el, k.p, k.l, None))
                .map(|x| x.0)
                .collect();
            let got = tx
                .index_scan(&g, ix.range, &enc(a), succ(&enc(a)).as_deref())
                .unwrap();
            assert_eq!(ids(got), want, "L.p = {a:?}");
            let want: BTreeSet<u64> = ns
                .iter()
                .filter(|(_, el)| eq(el, k.p, k.l, Some(k.q)))
                .map(|x| x.0)
                .collect();
            let got = tx
                .index_scan(&g, ix.composite, &enc(a), succ(&enc(a)).as_deref())
                .unwrap();
            assert_eq!(ids(got), want, "L.(p, q) with p = {a:?}");
            // A range on the single-property index and on the edge index.
            let (lo, hi) = if total_cmp(a, b).is_le() {
                (a, b)
            } else {
                (b, a)
            };
            let within = |v: &Value| total_cmp(v, lo).is_ge() && total_cmp(v, hi).is_lt();
            let want: BTreeSet<u64> = ns
                .iter()
                .filter(|(_, el)| el.tokens.contains(&k.l) && get(el, k.p).is_some_and(within))
                .map(|x| x.0)
                .collect();
            let got = tx
                .index_scan(&g, ix.range, &enc(lo), Some(&enc(hi)))
                .unwrap();
            assert_eq!(ids(got), want, "{lo:?} <= L.p < {hi:?}");
            let want: BTreeSet<u64> = es
                .iter()
                .filter(|(_, el)| el.tokens.contains(&k.t) && get(el, k.w).is_some_and(within))
                .map(|x| x.0)
                .collect();
            let got = tx
                .index_scan(&g, ix.edge_range, &enc(lo), Some(&enc(hi)))
                .unwrap();
            assert_eq!(ids(got), want, "{lo:?} <= T.w < {hi:?}");
            // Distance.
            let c = Point::new(
                Srid::Cartesian2D,
                &[rng.random_range(-60.0..60.0), rng.random_range(-60.0..60.0)],
            )
            .unwrap();
            let r = rng.random_range(0.0..40.0);
            let want: BTreeSet<u64> = ns
                .iter()
                .filter(|(_, el)| {
                    el.tokens.contains(&k.l)
                        && matches!(get(el, k.loc), Some(Value::Point(p))
                            if spatial::distance(&c, p).is_some_and(|d| d <= r))
                })
                .map(|x| x.0)
                .collect();
            let got = tx.index_within(&g, ix.point, &c, r).unwrap();
            assert_eq!(ids(got), want, "within {r} of {c:?}");
        }
        tx.abort();
    }

    /// FR-IDX-001 (storage part): one index of each kind, built over
    /// seeded data, returns for seeded queries the same elements as a full
    /// scan at the same snapshot, also with the transaction's own changes
    /// and after further commits maintain them.
    #[test]
    fn one_index_of_each_kind_returns_what_a_full_scan_returns() {
        let (_d, e) = db();
        let mut rng = StdRng::seed_from_u64(seed());
        let k = tokens(&e);
        let nodes = seed_graph(&e, &mut rng, &k, DEFAULT_GRAPH_ID, 300);
        let ix = create_indexes(&e, &k, DEFAULT_GRAPH_ID);
        assert_queries_match_scans(&e, &mut rng, &k, &ix);
        // Commits after the indexes are ONLINE maintain them.
        for i in 0..30 {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let n = nodes[i * 7 % nodes.len()];
            if tx.node(&g, n).unwrap().is_some() {
                match i % 3 {
                    0 => tx.set_node(&g, n, &node_labels(&mut rng, &k), &node_props(&mut rng, &k)),
                    1 => tx.delete_node(&g, n, true).map(drop),
                    _ => tx.remove_property(&g, n, k.p),
                }
                .unwrap();
            }
            tx.create_node(&g, &[k.l], &node_props(&mut rng, &k))
                .unwrap();
            tx.commit().unwrap();
        }
        assert_queries_match_scans(&e, &mut rng, &k, &ix);
        assert_intact(&e);
    }

    /// One writer's random commit: an insert, an update or a delete of a
    /// node of `L`, or of an edge of `T`.
    fn writer_commit(e: &Engine, rng: &mut StdRng, k: &Tokens) {
        loop {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let r = (|| -> crate::storage::error::Result<()> {
                let id = rng.random_range(0..400u64);
                match rng.random_range(0..4) {
                    0 => {
                        let a = tx.create_node(&g, &[k.l], &node_props(rng, k))?;
                        tx.create_edge(&g, a, a, k.t, &[(k.w, Value::Int(1))])?;
                    }
                    1 => {
                        if tx.node(&g, id)?.is_some() {
                            tx.set_node(&g, id, &node_labels(rng, k), &node_props(rng, k))?;
                        }
                    }
                    2 => {
                        if tx.node(&g, id)?.is_some() {
                            tx.delete_node(&g, id, true)?;
                        }
                    }
                    _ => {
                        if tx.edge(&g, id)?.is_some() {
                            tx.set_edge(&g, id, &[(k.w, Value::Int(rng.random_range(0..3)))])?;
                        }
                    }
                }
                Ok(())
            })();
            match r.and_then(|()| tx.commit().map(drop)) {
                Ok(()) => return,
                Err(Error::Serialization | Error::Deadlock | Error::Aborted) => continue,
                Err(x) => panic!("{x}"),
            }
        }
    }

    /// Eight writer threads each commit `n` random changes while the build
    /// stays paused; returns once they have all committed.
    fn eight_writers(e: &Engine, k: &Tokens, n: usize, round: u64) {
        std::thread::scope(|s| {
            for w in 0..8 {
                s.spawn(move || {
                    let mut rng = StdRng::seed_from_u64(seed() ^ (round << 8) ^ w);
                    for _ in 0..n {
                        writer_commit(e, &mut rng, k);
                    }
                });
            }
        });
    }

    /// FR-IDX-005: with the build paused after its scan, during its replay
    /// and before its ONLINE commit, 8 writer threads commit inserts,
    /// updates and deletes while the builder stays paused; the ONLINE index
    /// then passes the integrity check against the data, and its
    /// statistics equal a recount.
    #[test]
    fn eight_writers_commit_while_a_build_is_paused_and_the_online_index_matches_the_data() {
        let (_d, e) = db();
        let mut rng = StdRng::seed_from_u64(seed());
        let k = tokens(&e);
        seed_graph(&e, &mut rng, &k, DEFAULT_GRAPH_ID, 300);
        let g = gate(&e, &[Phase::Scanned, Phase::Replaying, Phase::BeforeOnline]);
        let mut ids = Vec::new();
        // Two builds, one after the other; each pauses at the three points
        // while eight writers commit.
        for (i, (kind, entity, token, props)) in [
            (SchemaKind::Range, Entity::Node, k.l, vec![k.p]),
            (SchemaKind::Lookup, Entity::Edge, 0, vec![]),
        ]
        .into_iter()
        .enumerate()
        {
            let id = e
                .create_index(DEFAULT_GRAPH_ID, kind, entity, token, &props)
                .unwrap();
            ids.push(id);
            for (j, p) in [Phase::Scanned, Phase::Replaying, Phase::BeforeOnline]
                .into_iter()
                .enumerate()
            {
                g.wait(p);
                assert_eq!(e.core().catalog.schema()[&id].state, State::Populating);
                // Enough side-tree records that the replay takes more than
                // one chunk.
                eight_writers(&e, &k, 25, (i * 3 + j) as u64);
                g.resume.send(()).unwrap();
            }
            e.wait_index(id).unwrap();
            assert_eq!(e.core().catalog.schema()[&id].state, State::Online);
        }
        assert_intact(&e);
        let st = e.core().catalog.stats().counts().clone();
        assert!(
            st.keys()
                .any(|s| matches!(s, StatKey::Index { index, .. } if *index == ids[0]))
        );
        assert_queries_after_writers(&e, &k, &ids);
    }

    /// The range index of `ids[0]` answers like a scan after the writers.
    fn assert_queries_after_writers(e: &Engine, k: &Tokens, ids: &[u64]) {
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        let ns = scan(&mut tx, &g, Entity::Node);
        for v in (0..5).map(Value::Int) {
            let want: BTreeSet<u64> = ns
                .iter()
                .filter(|(_, el)| {
                    el.tokens.contains(&k.l)
                        && get(el, k.p).is_some_and(|x| total_cmp(x, &v).is_eq())
                })
                .map(|x| x.0)
                .collect();
            let got: BTreeSet<u64> = tx
                .index_scan(&g, ids[0], &enc(&v), succ(&enc(&v)).as_deref())
                .unwrap()
                .into_iter()
                .collect();
            assert_eq!(got, want);
        }
        tx.abort();
    }

    /// FR-IDX-003: an index on graph g1 holds no entry of graph g2, whose
    /// elements have the same labels and properties.
    #[test]
    fn an_index_on_one_graph_holds_no_entry_of_another() {
        let (_d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g2 = tx
            .create_graph(DEFAULT_SCHEMA_ID, "g2", OWNER, None)
            .unwrap();
        tx.commit().unwrap();
        let mut ids = [Vec::new(), Vec::new()];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for (i, gid) in [DEFAULT_GRAPH_ID, g2].into_iter().enumerate() {
            let g = tx.graph(gid).unwrap();
            for v in 0..20 {
                ids[i].push(
                    tx.create_node(&g, &[k.l], &[(k.p, Value::Int(v % 4))])
                        .unwrap(),
                );
            }
        }
        tx.commit().unwrap();
        let range = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        let lookup = e
            .create_index(DEFAULT_GRAPH_ID, SchemaKind::Lookup, Entity::Node, 0, &[])
            .unwrap();
        e.wait_index(range).unwrap();
        e.wait_index(lookup).unwrap();
        // Both graphs change after the build, in one transaction.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for (i, gid) in [DEFAULT_GRAPH_ID, g2].into_iter().enumerate() {
            let g = tx.graph(gid).unwrap();
            ids[i].push(tx.create_node(&g, &[k.l], &[(k.p, Value::Int(1))]).unwrap());
        }
        tx.commit().unwrap();
        let store = e.store();
        let mut in_index = BTreeSet::new();
        for index in [range, lookup] {
            let t = store
                .tree(e.core().catalog.schema()[&index].tree)
                .unwrap()
                .unwrap();
            store
                .scan_prefix_stamped(t, b"", |key, _, _| {
                    in_index.insert(element_of_key(key).unwrap());
                    true
                })
                .unwrap();
        }
        let g1: BTreeSet<u64> = ids[0].iter().copied().collect();
        assert_eq!(in_index, g1, "the entries are exactly g1's nodes");
        assert!(ids[1].iter().all(|n| !in_index.contains(n)));
        // An index of g1 is not one of g2.
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = tx.graph(g2).unwrap();
        assert!(matches!(
            tx.index_scan(&g, range, b"", None),
            Err(Error::NotFound(_))
        ));
        tx.abort();
        // An index of g2 goes with g2.
        let ix2 = e
            .create_index(g2, SchemaKind::Lookup, Entity::Node, 0, &[])
            .unwrap();
        e.wait_index(ix2).unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "g2").unwrap());
        tx.commit().unwrap();
        assert!(e.core().catalog.schema().values().all(|s| s.graph != g2));
        assert_intact(&e);
    }

    /// TS-15 Read sets: a serializable read of an index range, or of the
    /// Hilbert ranges of a distance query, fails validation when a commit
    /// after its snapshot adds an element in it; under snapshot isolation
    /// the reader commits.
    #[test]
    fn an_index_range_read_conflicts_with_a_later_insert_into_it() {
        let (_d, e) = db();
        let k = tokens(&e);
        let range = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        let point = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Point,
                Entity::Node,
                k.l,
                &[k.loc],
            )
            .unwrap();
        e.wait_index(range).unwrap();
        e.wait_index(point).unwrap();
        let mut round = 0;
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            for by_point in [false, true] {
                // A value and a place of this round's own.
                round += 1;
                let x = 100.0 * f64::from(round);
                let c = Point::new(Srid::Cartesian2D, &[x, 0.0]).unwrap();
                let v = Value::Int(i64::from(round));
                let mut t1 = e.begin(iso).unwrap();
                let g = graph(&mut t1);
                let found = if by_point {
                    t1.index_within(&g, point, &c, 5.0).unwrap()
                } else {
                    let v = enc(&v);
                    t1.index_scan(&g, range, &v, succ(&v).as_deref()).unwrap()
                };
                assert!(found.is_empty());
                let mut t2 = e.begin(Isolation::Serializable).unwrap();
                let g2 = graph(&mut t2);
                let pt = Point::new(Srid::Cartesian2D, &[x + 1.0, 1.0]).unwrap();
                t2.create_node(
                    &g2,
                    &[k.l],
                    &[(k.p, v.clone()), (k.loc, Value::Point(Box::new(pt)))],
                )
                .unwrap();
                t2.commit().unwrap();
                t1.create_node(&g, &[k.m], &[]).unwrap();
                let r = t1.commit();
                match iso {
                    Isolation::Serializable => {
                        assert!(matches!(r, Err(Error::Serialization)), "{by_point}: {r:?}");
                    }
                    Isolation::Snapshot => {
                        r.unwrap();
                    }
                }
            }
        }
        assert_intact(&e);
    }

    /// FR-TYPE-005 (storage part): distance queries over seeded points in
    /// the four systems, among them points outside the curve extents,
    /// WGS-84 points beyond the date line or a pole, and SRID 4979 points
    /// with negative heights, return the same set with and without the
    /// point index.
    #[test]
    fn distance_queries_return_the_same_set_with_and_without_the_point_index() {
        let (_d, e) = db();
        let mut rng = StdRng::seed_from_u64(seed());
        let k = tokens(&e);
        let systems = [
            Srid::Cartesian2D,
            Srid::Cartesian3D,
            Srid::Wgs84_2D,
            Srid::Wgs84_3D,
        ];
        let coord = |rng: &mut StdRng, s: Srid, i: usize| -> f64 {
            let wide = rng.random_bool(0.15);
            match (s, i) {
                (Srid::Wgs84_2D | Srid::Wgs84_3D, 0) if !wide => rng.random_range(-180.0..180.0),
                (Srid::Wgs84_2D | Srid::Wgs84_3D, 1) if !wide => rng.random_range(-90.0..90.0),
                // Longitudes and latitudes past their extents.
                (Srid::Wgs84_2D | Srid::Wgs84_3D, 0 | 1) => rng.random_range(-400.0..400.0),
                // Heights: negative ones, some below the extent.
                (Srid::Wgs84_3D, _) if wide => rng.random_range(-3_000_000.0..3_000_000.0),
                (Srid::Wgs84_3D, _) => rng.random_range(-20_000.0..20_000.0),
                // Cartesian, some past ±1 000 000.
                _ if wide => rng.random_range(-5_000_000.0..5_000_000.0),
                _ => rng.random_range(-2_000.0..2_000.0),
            }
        };
        let point = |rng: &mut StdRng, s: Srid| {
            let c: Vec<f64> = (0..s.dims()).map(|i| coord(rng, s, i)).collect();
            Point::new(s, &c).unwrap()
        };
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        for _ in 0..600 {
            let s = systems[rng.random_range(0..4)];
            let p = point(&mut rng, s);
            tx.create_node(&g, &[k.l], &[(k.loc, Value::Point(Box::new(p)))])
                .unwrap();
        }
        // Points near the date line, near the poles and on the edges.
        for c in [
            [179.99, 0.0],
            [-179.99, 0.0],
            [10.0, 89.99],
            [-50.0, -89.99],
            [180.0, 90.0],
        ] {
            for s in [Srid::Wgs84_2D, Srid::Wgs84_3D] {
                let mut v = c.to_vec();
                if s == Srid::Wgs84_3D {
                    v.push(-1_000.0);
                }
                let p = Point::new(s, &v).unwrap();
                tx.create_node(&g, &[k.l], &[(k.loc, Value::Point(Box::new(p)))])
                    .unwrap();
            }
        }
        // SRID 4979 points 3 000 km below the surface, along the equator:
        // at that height an angle covers about half the distance it covers
        // on the sphere, so a box sized with the plain radius misses them.
        let deep = -3_000_000.0;
        for lon in [10.0, 20.0, 30.0, 40.0] {
            let p = Point::new(Srid::Wgs84_3D, &[lon, 0.0, deep]).unwrap();
            tx.create_node(&g, &[k.l], &[(k.loc, Value::Point(Box::new(p)))])
                .unwrap();
        }
        // Audit F3 (M6): a point within `wide` of the deep origin whose
        // mean height with it is below the origin's: at height `deep - a`,
        // with `a = wide² / 2 (R + deep)`, the angle it may lie at exceeds
        // `wide / (R + deep)`, the angular radius of a box sized on the
        // origin's own height.
        let wide: f64 = 3_000_000.0;
        let sphere = spatial::EARTH_RADIUS + deep;
        let a = wide * wide / (2.0 * sphere);
        let angle = (wide * wide * 0.998 - a * a).sqrt() / (sphere - a / 2.0);
        assert!(angle > wide / sphere * 1.01, "precondition: {angle}");
        let low = Point::new(Srid::Wgs84_3D, &[0.0, angle.to_degrees(), deep - a]).unwrap();
        tx.create_node(&g, &[k.l], &[(k.loc, Value::Point(Box::new(low)))])
            .unwrap();
        tx.commit().unwrap();
        let ix = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Point,
                Entity::Node,
                k.l,
                &[k.loc],
            )
            .unwrap();
        e.wait_index(ix).unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let ns = scan(&mut tx, &g, Entity::Node);
        let mut nonempty = 0;
        let origin = Point::new(Srid::Wgs84_3D, &[0.0, 0.0, deep]).unwrap();
        assert!(spatial::distance(&origin, &low).is_some_and(|d| d <= wide));
        // The boxes themselves hold it: the Hilbert ranges cover more than
        // the boxes, so the query alone may not show a box too small.
        let (lc, boxes) = (low.coords(), spatial::distance_boxes(&origin, wide));
        assert!(
            boxes.iter().any(|(lo, hi)| {
                (0..3).all(|i| lo.coords()[i] <= lc[i] && lc[i] <= hi.coords()[i])
            }),
            "{low:?} lies outside {boxes:?}"
        );
        for r in [10.0_f64, 20.0, 30.0, 40.0]
            .map(|lon| (spatial::EARTH_RADIUS + deep) * lon.to_radians() * 1.000_001)
            .into_iter()
            .chain([wide])
        {
            let got = tx.index_within(&g, ix, &origin, r).unwrap();
            let want: Vec<u64> = ns
                .iter()
                .filter(|(_, el)| {
                    matches!(get(el, k.loc), Some(Value::Point(p))
                        if spatial::distance(&origin, p).is_some_and(|d| d <= r))
                })
                .map(|x| x.0)
                .collect();
            assert_eq!(got, want, "within {r} of {origin:?}");
            assert!(!want.is_empty());
        }
        for i in 0..240 {
            let s = systems[i % 4];
            let c = point(&mut rng, s);
            let r = match rng.random_range(0..5) {
                0 => 0.0,
                1 => rng.random_range(0.0..1_000.0),
                2 => rng.random_range(0.0..200_000.0),
                3 => rng.random_range(0.0..3_000_000.0),
                _ => rng.random_range(0.0..30_000_000.0),
            };
            let want: BTreeSet<u64> = ns
                .iter()
                .filter(|(_, el)| {
                    matches!(get(el, k.loc), Some(Value::Point(p))
                        if spatial::distance(&c, p).is_some_and(|d| d <= r))
                })
                .map(|x| x.0)
                .collect();
            let got: BTreeSet<u64> = tx
                .index_within(&g, ix, &c, r)
                .unwrap()
                .into_iter()
                .collect();
            assert_eq!(got, want, "within {r} of {c:?}");
            nonempty += usize::from(!want.is_empty());
        }
        assert!(nonempty > 60, "the queries find points: {nonempty}");
        tx.abort();
        assert_intact(&e);
    }

    /// TS-17: an index's statistics are its entries and distinct keys; they
    /// follow commits, the integrity check compares them with a recount,
    /// and a dropped index's statistics go.
    #[test]
    fn index_statistics_equal_a_recount() {
        let (_d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let mut nodes = Vec::new();
        for v in [1, 1, 2, 3] {
            nodes.push(tx.create_node(&g, &[k.l], &[(k.p, Value::Int(v))]).unwrap());
        }
        tx.create_node(&g, &[k.m], &[(k.p, Value::Int(1))]).unwrap();
        tx.commit().unwrap();
        let ix = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        e.wait_index(ix).unwrap();
        let counts = |e: &Engine| {
            let st = e.core().catalog.stats();
            let c = st.counts();
            let get = |distinct| {
                c.get(&StatKey::Index {
                    graph: DEFAULT_GRAPH_ID,
                    index: ix,
                    distinct,
                })
                .copied()
                .unwrap_or(0)
            };
            (get(false), get(true))
        };
        assert_eq!(counts(&e), (4, 3));
        // 1, 1, 2, 3 → 2.0, 2, 3, a new 3 and a new 9: five values, three
        // distinct; value 1 goes, value 9 comes.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.set_property(&g, nodes[1], k.p, Value::Float(2.0))
            .unwrap();
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(3))]).unwrap();
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(9))]).unwrap();
        tx.delete_node(&g, nodes[0], false).unwrap();
        tx.commit().unwrap();
        assert_eq!(counts(&e), (5, 3));
        assert_intact(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert!(tx.drop_schema(DEFAULT_GRAPH_ID, ix).unwrap());
        tx.commit().unwrap();
        assert_eq!(counts(&e), (0, 0));
        assert!(e.core().catalog.schema().is_empty());
        assert_intact(&e);
    }

    // -----------------------------------------------------------------------
    // Constraints (rmp #34).

    /// FR-IDX-002: with a node-key constraint on label L and property p,
    /// inserting an L node without p fails, and inserting one with a used p
    /// value fails, at write time or at commit; other inserts commit.
    #[test]
    fn a_node_key_rejects_a_node_without_its_property_or_with_a_used_value() {
        let (_d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(1))]).unwrap();
        tx.commit().unwrap();
        let c = e
            .create_constraint(
                DEFAULT_GRAPH_ID,
                SchemaKind::NodeKey,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        let violates = |r: crate::storage::error::Result<Option<u64>>| {
            assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
        };
        // Without p: at commit.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.q, Value::Int(1))]).unwrap();
        violates(tx.commit());
        // A used value, 1.0 = 1: at write time.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let r = tx.create_node(&g, &[k.l], &[(k.p, Value::Float(1.0))]);
        assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
        tx.abort();
        // Twice in one transaction: at commit.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(2))]).unwrap();
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(2))]).unwrap();
        violates(tx.commit());
        // A label added to a node with a used value: at write time.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let n = tx.create_node(&g, &[k.m], &[(k.p, Value::Int(1))]).unwrap();
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        assert!(matches!(
            tx.add_label(&g, n, k.l),
            Err(Error::ConstraintViolation(_))
        ));
        tx.abort();
        // Others commit; a value freed by its node's change is usable.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(2))]).unwrap();
        tx.create_node(&g, &[k.m], &[]).unwrap();
        tx.commit().unwrap();
        assert_eq!(e.core().catalog.schema()[&c].kind, SchemaKind::NodeKey);
        assert_intact(&e);
    }

    /// FR-IDX-002, TS-15 unique value: 16 threads inserting the same unique
    /// value: exactly one commits, at both isolation levels.
    #[test]
    fn sixteen_threads_inserting_one_unique_value_exactly_one_commits_at_both_levels() {
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            let (_d, e) = db();
            let k = tokens(&e);
            e.create_constraint(
                DEFAULT_GRAPH_ID,
                SchemaKind::Unique,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
            let barrier = crate::sync::Barrier::new(16);
            let committed: usize = std::thread::scope(|s| {
                let hs: Vec<_> = (0..16)
                    .map(|_| {
                        let (e, barrier, k) = (&e, &barrier, &k);
                        s.spawn(move || {
                            barrier.wait();
                            let mut tx = e.begin(iso).unwrap();
                            let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
                            let r = tx
                                .create_node(&g, &[k.l], &[(k.p, Value::string("v"))])
                                .and_then(|_| tx.commit().map(drop));
                            match r {
                                Ok(()) => 1,
                                Err(Error::ConstraintViolation(_)) => 0,
                                Err(x) => panic!("{x}"),
                            }
                        })
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap()).sum()
            });
            assert_eq!(committed, 1, "{iso:?}");
            assert_intact(&e);
        }
    }

    /// FR-IDX-003: a uniqueness constraint on graph g1 lets duplicate values
    /// commit in graph g2.
    #[test]
    fn a_uniqueness_constraint_on_one_graph_lets_duplicates_commit_in_another() {
        let (_d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g2 = tx
            .create_graph(DEFAULT_SCHEMA_ID, "g2", OWNER, None)
            .unwrap();
        tx.commit().unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Unique,
            Entity::Node,
            k.l,
            &[k.p],
        )
        .unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(7))]).unwrap();
        tx.commit().unwrap();
        // Graph g2 takes value 7 three times, in one transaction with a
        // write to g1.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(g2).unwrap();
        for _ in 0..3 {
            tx.create_node(&g, &[k.l], &[(k.p, Value::Int(7))]).unwrap();
        }
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(8))]).unwrap();
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let r = tx.create_node(&g, &[k.l], &[(k.p, Value::Int(7))]);
        assert!(matches!(r, Err(Error::ConstraintViolation(_))));
        tx.abort();
        assert_intact(&e);
    }

    /// FR-IDX-004: two nodes with the same p: creating a uniqueness
    /// constraint on p fails; afterwards no constraint and no owned index
    /// exist, no statistic of one remains, and the integrity check passes.
    /// The same for a node key over a node without p, an existence
    /// constraint and a type constraint.
    #[test]
    fn a_constraint_over_violating_data_fails_and_leaves_nothing() {
        let (_d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        for i in 0..200 {
            tx.create_node(&g, &[k.l], &[(k.p, Value::Int(i))]).unwrap();
        }
        tx.create_node(&g, &[k.l], &[(k.p, Value::Float(5.0))])
            .unwrap();
        tx.create_node(&g, &[k.l], &[(k.q, Value::string("x"))])
            .unwrap();
        tx.commit().unwrap();
        let pages = e.data().page_count();
        for (kind, props) in [
            (SchemaKind::Unique, vec![k.p]),
            (SchemaKind::NodeKey, vec![k.q]),
            (SchemaKind::Exists, vec![k.p]),
            (
                SchemaKind::Type(TypeSet::of(PropertyType::Integer)),
                vec![k.p],
            ),
        ] {
            let r = e.create_constraint(DEFAULT_GRAPH_ID, kind, Entity::Node, k.l, &props);
            assert!(
                matches!(r, Err(Error::ConstraintViolation(_))),
                "{kind:?}: {r:?}"
            );
            assert!(e.core().catalog.schema().is_empty());
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            assert!(tx.schema_defs(DEFAULT_GRAPH_ID).unwrap().is_empty());
            tx.abort();
            assert!(
                !e.core()
                    .catalog
                    .stats()
                    .counts()
                    .keys()
                    .any(|s| matches!(s, StatKey::Index { .. }))
            );
            assert_eq!(
                e.data().page_count(),
                pages,
                "{kind:?}: the built pages are free"
            );
            assert_intact(&e);
        }
        // Once the data complies, the uniqueness constraint is created.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let ns = scan(&mut tx, &g, Entity::Node);
        let dup = ns
            .iter()
            .find(|(_, el)| get(el, k.p) == Some(&Value::Float(5.0)))
            .unwrap()
            .0;
        tx.delete_node(&g, dup, false).unwrap();
        tx.commit().unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Unique,
            Entity::Node,
            k.l,
            &[k.p],
        )
        .unwrap();
        assert_eq!(
            e.core().catalog.schema().len(),
            2,
            "the constraint and its index"
        );
        assert_intact(&e);
    }

    /// FR-IDX-006: with validation paused, a writer of an L node is seen
    /// waiting in the lock table and a writer of an M node commits; after
    /// the pause ends, the L writer succeeds or fails against the
    /// constraint.
    #[test]
    fn a_constraint_creation_blocks_only_the_writers_of_its_label() {
        for clash in [false, true] {
            let (_d, e) = db();
            let k = tokens(&e);
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            tx.create_node(&g, &[k.l], &[(k.p, Value::Int(1))]).unwrap();
            tx.commit().unwrap();
            std::thread::scope(|s| {
                // Inside the scope: a failing assertion drops it, which
                // resumes the paused creator before the scope joins it.
                let gt = gate(&e, &[Phase::Validating]);
                let creator = s.spawn(|| {
                    e.create_constraint(
                        DEFAULT_GRAPH_ID,
                        SchemaKind::Unique,
                        Entity::Node,
                        k.l,
                        &[k.p],
                    )
                });
                gt.wait(Phase::Validating);
                let writer = s.spawn(|| {
                    let mut tx = e.begin(Isolation::Serializable).unwrap();
                    let g = graph(&mut tx);
                    let v = if clash { 1 } else { 2 };
                    tx.create_node(&g, &[k.l], &[(k.p, Value::Int(v))])
                        .and_then(|_| tx.commit().map(drop))
                });
                until_waiting(&e, 1);
                // An M writer commits while the L writer waits.
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                let g = graph(&mut tx);
                tx.create_node(&g, &[k.m], &[(k.p, Value::Int(1))]).unwrap();
                tx.commit().unwrap();
                assert!(!writer.is_finished(), "the L writer still waits");
                gt.resume.send(()).unwrap();
                creator.join().unwrap().unwrap();
                let r = writer.join().unwrap();
                if clash {
                    assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
                } else {
                    r.unwrap();
                }
            });
            assert_intact(&e);
        }
    }

    /// A pause in the page writes of a bulk build: at the first page it
    /// writes, the build signals `reached` and waits for `resume`
    /// ([`crate::storage::wal::TestPoints`] `bulk_page`). A dropped `resume`
    /// lets it go on, so a failing test does not hang.
    struct PageGate {
        reached: mpsc::Receiver<()>,
        resume: mpsc::Sender<()>,
    }

    fn page_gate(e: &Engine) -> PageGate {
        let (rt, rr) = mpsc::channel();
        let (st, sr) = mpsc::channel::<()>();
        let sr = Mutex::new(sr);
        let first = crate::sync::atomic::AtomicBool::new(true);
        *e.core().wal.points.bulk_page.lock().unwrap() = Some(Box::new(move || {
            if first.swap(false, crate::sync::atomic::Ordering::SeqCst) {
                let _ = rt.send(());
                let _ = sr.lock().unwrap().recv();
            }
        }));
        PageGate {
            reached: rr,
            resume: st,
        }
    }

    impl PageGate {
        fn wait(&self) {
            self.reached
                .recv_timeout(crate::test_support::WAIT_BOUND)
                .expect("the build reaches its page writes");
        }
    }

    /// The commit latch becomes free. Other threads of the engine (the WAL
    /// writer, the checkpointer, the collector) take it for short moments,
    /// so one probe is not enough; a latch held for the whole pause never
    /// becomes free, and the test fails.
    fn until_latch_free(e: &Engine) {
        crate::test_support::wait_until("the commit latch is free", || e.store().latch_is_free());
    }

    /// FR-IDX-005 and TS-18 (rmp #33, sprint DECISION 65): with an index
    /// build paused inside its page writes, the commit latch is free, and
    /// eight writer threads commit inserts, updates and deletes of the
    /// indexed label and of others while the builder stays paused. The build
    /// then ends ONLINE, the index matches the data and the database passes
    /// the integrity check.
    #[test]
    fn writers_commit_while_an_index_build_is_paused_in_its_page_writes() {
        let (_d, e) = db();
        let mut rng = StdRng::seed_from_u64(seed());
        let k = tokens(&e);
        seed_graph(&e, &mut rng, &k, DEFAULT_GRAPH_ID, 300);
        let gate = page_gate(&e);
        let id = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        gate.wait();
        until_latch_free(&e);
        eight_writers(&e, &k, 10, 0);
        assert_eq!(e.core().catalog.schema()[&id].state, State::Populating);
        gate.resume.send(()).unwrap();
        e.wait_index(id).unwrap();
        assert_eq!(e.core().catalog.schema()[&id].state, State::Online);
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
    }

    /// FR-IDX-006 (rmp #34, sprint DECISION 65): with the owned index of a
    /// uniqueness constraint paused inside its page writes, the commit latch
    /// is free and a writer of another label commits, while a writer of the
    /// constrained label waits for the schema lock until the creation ends.
    #[test]
    fn a_constraint_paused_in_its_page_writes_blocks_only_the_writers_of_its_label() {
        let (_d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(1))]).unwrap();
        tx.commit().unwrap();
        std::thread::scope(|s| {
            // Inside the scope: a failing assertion drops it before the
            // scope joins the paused creator.
            let gate = page_gate(&e);
            let creator = s.spawn(|| {
                e.create_constraint(
                    DEFAULT_GRAPH_ID,
                    SchemaKind::Unique,
                    Entity::Node,
                    k.l,
                    &[k.p],
                )
            });
            gate.wait();
            until_latch_free(&e);
            let writer = s.spawn(|| {
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                let g = graph(&mut tx);
                tx.create_node(&g, &[k.l], &[(k.p, Value::Int(2))])
                    .and_then(|_| tx.commit().map(drop))
            });
            until_waiting(&e, 1);
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            tx.create_node(&g, &[k.m], &[(k.p, Value::Int(1))]).unwrap();
            tx.commit().unwrap();
            assert!(!writer.is_finished(), "the L writer still waits");
            gate.resume.send(()).unwrap();
            creator.join().unwrap().unwrap();
            writer.join().unwrap().unwrap();
        });
        assert_intact(&e);
    }

    /// Property existence and property type constraints reject the commits
    /// that violate them and accept the others, on nodes and on edges; an
    /// element without the property is not subject to the type
    /// constraint.
    #[test]
    fn existence_and_type_constraints_reject_violating_commits_and_accept_others() {
        let (_d, e) = db();
        let k = tokens(&e);
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Exists,
            Entity::Node,
            k.l,
            &[k.p],
        )
        .unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Type(TypeSet::of(PropertyType::String)),
            Entity::Node,
            k.m,
            &[k.q],
        )
        .unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Type(TypeSet::of(PropertyType::Integer)),
            Entity::Edge,
            k.t,
            &[k.w],
        )
        .unwrap();
        let attempt = |f: &dyn Fn(&mut Transaction<'_>, &Graph)| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            f(&mut tx, &g);
            tx.commit()
        };
        let bad = |r: crate::storage::error::Result<Option<u64>>| {
            assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
        };
        bad(attempt(&|tx, g| {
            tx.create_node(g, &[k.l], &[(k.q, Value::Int(1))]).unwrap();
        }));
        bad(attempt(&|tx, g| {
            tx.create_node(g, &[k.l], &[(k.p, Value::Null)]).unwrap();
        }));
        bad(attempt(&|tx, g| {
            tx.create_node(g, &[k.m], &[(k.q, Value::Int(1))]).unwrap();
        }));
        let n = attempt(&|tx, g| {
            tx.create_node(
                g,
                &[k.l, k.m],
                &[(k.p, Value::Int(1)), (k.q, Value::string("a"))],
            )
            .unwrap();
            // M without q: not subject to the type constraint.
            tx.create_node(g, &[k.m], &[]).unwrap();
        });
        n.unwrap();
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        let lm = scan(&mut tx, &g, Entity::Node)
            .into_iter()
            .find(|(_, el)| el.tokens.len() == 2)
            .unwrap()
            .0;
        tx.abort();
        // Removing p from the L node, or giving q another type, fails.
        bad(attempt(&|tx, g| tx.remove_property(g, lm, k.p).unwrap()));
        bad(attempt(&|tx, g| {
            tx.set_property(g, lm, k.q, Value::Bool(true)).unwrap();
        }));
        // Removing label L first makes removing p valid.
        attempt(&|tx, g| {
            tx.remove_label(g, lm, k.l).unwrap();
            tx.remove_property(g, lm, k.p).unwrap();
        })
        .unwrap();
        // Edges: type T needs an integer w.
        bad(attempt(&|tx, g| {
            tx.create_edge(g, lm, lm, k.t, &[(k.w, Value::Float(1.5))])
                .unwrap();
        }));
        attempt(&|tx, g| {
            tx.create_edge(g, lm, lm, k.t, &[(k.w, Value::Int(1))])
                .unwrap();
            tx.create_edge(g, lm, lm, k.u, &[(k.w, Value::Float(1.5))])
                .unwrap();
        })
        .unwrap();
        assert_intact(&e);
    }

    /// Constraints and indexes survive a clean reopen and a drop without
    /// close, and are enforced after both.
    #[test]
    fn constraints_survive_reopen_and_a_drop_without_close() {
        let (d, e) = db();
        let k = tokens(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[k.l], &[(k.p, Value::Int(1))]).unwrap();
        tx.commit().unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::NodeKey,
            Entity::Node,
            k.l,
            &[k.p],
        )
        .unwrap();
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Exists,
            Entity::Edge,
            k.t,
            &[k.w],
        )
        .unwrap();
        let ix = e
            .create_index(DEFAULT_GRAPH_ID, SchemaKind::Lookup, Entity::Node, 0, &[])
            .unwrap();
        e.wait_index(ix).unwrap();
        let schema = e.core().catalog.schema().clone();
        let enforced = |e: &Engine| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let r = tx.create_node(&g, &[k.l], &[(k.p, Value::Int(1))]);
            assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
            tx.abort();
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let n = tx.create_node(&g, &[k.m], &[]).unwrap();
            tx.create_edge(&g, n, n, k.t, &[]).unwrap();
            assert!(matches!(tx.commit(), Err(Error::ConstraintViolation(_))));
            assert_eq!(*e.core().catalog.schema(), schema);
            assert_intact(e);
        };
        enforced(&e);
        e.close().unwrap();
        let e = reopen(d.path());
        enforced(&e);
        drop(e);
        let e = reopen(d.path());
        enforced(&e);
    }
    /// Audit F1 (rmp #35): the integrity check reads the catalog tree, not
    /// only the cache. After a clean reopen an ONLINE index's definition is
    /// in the cache and its tree in the directory; a tree no definition
    /// names is reported unless the caller created it outside the catalog;
    /// a definition whose tree is dropped is reported.
    #[test]
    fn the_integrity_check_reconciles_the_directory_with_the_catalog() {
        let (d, e) = db();
        let l = tok(&e, TokenKind::Label, "L");
        let p = tok(&e, TokenKind::PropertyKey, "P");
        let ix = e
            .create_index(DEFAULT_GRAPH_ID, SchemaKind::Range, Entity::Node, l, &[p])
            .unwrap();
        e.wait_index(ix).unwrap();
        e.close().unwrap();
        let e = reopen(d.path());
        assert_intact(&e);
        pause_gc(&e);
        let orphan = e.create_trees(1).unwrap()[0].id();
        assert_eq!(
            e.check_integrity().unwrap(),
            [format!("tree {orphan}: no catalog definition names it")]
        );
        assert!(e.check_integrity_except(&[orphan]).unwrap().is_empty());
        let id = e.core().catalog.schema()[&ix].tree;
        let t = e.store().tree(id).unwrap().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.drop_tree(t).unwrap();
        tx.commit().unwrap();
        let p = e.check_integrity_except(&[orphan]).unwrap();
        let want = format!("definition {ix}: its tree {id} is missing or dropped");
        assert!(p.contains(&want), "{p:?}");
    }

    // -----------------------------------------------------------------------
    // Round C audit, round 1 (rmp #33, #34).

    /// As [`gate`], but the build pauses only the `n`-th time it reaches
    /// `phase`, for each `(phase, n)` of `at`; every phase it reaches is
    /// recorded in the returned list.
    fn gate_at(e: &Engine, at: &[(Phase, usize)]) -> (Gate, Arc<Mutex<Vec<Phase>>>) {
        let (rt, rr) = mpsc::channel();
        let (st, sr) = mpsc::channel::<()>();
        let sr = Mutex::new(sr);
        let at = at.to_vec();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let hook: Hook = Arc::new(move |p| {
            let n = {
                let mut l = log.lock().unwrap();
                l.push(p);
                l.iter().filter(|x| **x == p).count()
            };
            if at.contains(&(p, n)) {
                let _ = rt.send(p);
                let _ = sr.lock().unwrap().recv();
            }
        });
        *e.core().builds.hook.lock().unwrap() = Some(hook);
        (
            Gate {
                reached: rr,
                resume: st,
            },
            seen,
        )
    }

    /// Commits `n` nodes of label `l` with `p` = 0, 1, ... in `g`.
    fn seed_l(e: &Engine, graph_id: u64, l: u64, p: u64, n: i64) -> Vec<u64> {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(graph_id).unwrap();
        let ids = (0..n)
            .map(|i| {
                tx.create_node(&g, &[l], &[(p, Value::Int(i % 50))])
                    .unwrap()
            })
            .collect();
        tx.commit().unwrap();
        ids
    }

    /// Commits a node of label `l` with `p` = `v` in the default graph.
    fn commit_node(
        e: &Engine,
        l: u64,
        p: u64,
        v: i64,
    ) -> crate::storage::error::Result<Option<u64>> {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &[l], &[(p, Value::Int(v))])?;
        tx.commit()
    }

    /// The elements of index `id` of the default graph with value `v`.
    fn with_value(e: &Engine, id: u64, v: &Value) -> Vec<u64> {
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        let k = enc(v);
        let out = tx.index_scan(&g, id, &k, succ(&k).as_deref()).unwrap();
        tx.abort();
        out
    }

    /// Audit F1 (FR-IDX-001, TS-18 step 3): the build's scan, which reads
    /// without the commit latch, sees a commit's batch half applied; the
    /// commit then fails with ENOSPC and is undone. The build scans again,
    /// so the index holds no entry of the commit's node (without the undo
    /// check at the install it keeps one for good).
    #[test]
    fn a_build_whose_scan_saw_an_undone_commit_scans_again() {
        let (_d, e) = db();
        pause_gc(&e);
        let k = tokens(&e);
        seed_l(&e, DEFAULT_GRAPH_ID, k.l, k.p, 20);
        std::thread::scope(|s| {
            let (gt, seen) = gate_at(&e, &[(Phase::Scanning, 1), (Phase::Scanned, 1)]);
            let id = e
                .create_index(
                    DEFAULT_GRAPH_ID,
                    SchemaKind::Range,
                    Entity::Node,
                    k.l,
                    &[k.p],
                )
                .unwrap();
            gt.wait(Phase::Scanning);
            let mid = mid_batch(&e);
            let committer = s.spawn(|| commit_node(&e, k.l, k.p, 99));
            mid.wait();
            gt.resume.send(()).unwrap();
            gt.wait(Phase::Scanned);
            mid.fail.send(true).unwrap();
            let r = committer.join().unwrap();
            assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
            gt.resume.send(()).unwrap();
            e.wait_index(id).unwrap();
            assert_eq!(e.core().catalog.schema()[&id].state, State::Online);
            assert_eq!(with_value(&e, id, &Value::Int(99)), Vec::<u64>::new());
            let scans = seen
                .lock()
                .unwrap()
                .iter()
                .filter(|p| **p == Phase::Scanning)
                .count();
            assert_eq!(scans, 2, "the build scans again after the undo");
        });
        assert_intact(&e);
    }

    /// Audit F1, the replay: a chunk of side-tree records is read while a
    /// commit's batch, which wrote one, is half applied; the commit is then
    /// undone. The chunk is read again under the latch's check, so the
    /// index holds no entry of the undone node.
    #[test]
    fn a_replay_chunk_read_during_an_undone_commit_is_read_again() {
        let (_d, e) = db();
        pause_gc(&e);
        let k = tokens(&e);
        seed_l(&e, DEFAULT_GRAPH_ID, k.l, k.p, 20);
        std::thread::scope(|s| {
            let (gt, _) = gate_at(
                &e,
                &[
                    (Phase::Scanned, 1),
                    (Phase::Replaying, 1),
                    (Phase::ChunkRead, 2),
                ],
            );
            let id = e
                .create_index(
                    DEFAULT_GRAPH_ID,
                    SchemaKind::Range,
                    Entity::Node,
                    k.l,
                    &[k.p],
                )
                .unwrap();
            gt.wait(Phase::Scanned);
            // A first chunk to replay.
            commit_node(&e, k.l, k.p, 5).unwrap();
            gt.resume.send(()).unwrap();
            gt.wait(Phase::Replaying);
            let mid = mid_batch(&e);
            let committer = s.spawn(|| commit_node(&e, k.l, k.p, 99));
            mid.wait();
            gt.resume.send(()).unwrap();
            gt.wait(Phase::ChunkRead);
            mid.fail.send(true).unwrap();
            let r = committer.join().unwrap();
            assert!(r.as_ref().is_err_and(Error::is_no_space), "{r:?}");
            gt.resume.send(()).unwrap();
            e.wait_index(id).unwrap();
            assert_eq!(with_value(&e, id, &Value::Int(99)), Vec::<u64>::new());
            assert_eq!(with_value(&e, id, &Value::Int(5)).len(), 2);
        });
        assert_intact(&e);
    }

    /// Audit F2 (FR-IDX-005): a build interrupted after its install
    /// committed resumes, at the next open, at its replay: it does not scan
    /// again and frees none of the installed tree's pages (a rebuild would
    /// free them all in its one latched install). Writers commit while the
    /// resumed build is paused; the index then matches the data.
    #[test]
    fn a_build_restarted_after_its_install_resumes_at_its_replay() {
        let (d, e) = db();
        let k = tokens(&e);
        seed_l(&e, DEFAULT_GRAPH_ID, k.l, k.p, 3000);
        let core = e.core_arc();
        let gt = gate(&e, &[Phase::Installed]);
        let id = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        gt.wait(Phase::Installed);
        // A close while the build is paused: the build stops at its next
        // step, its index POPULATING.
        std::thread::scope(|s| {
            let closer = s.spawn(move || e.close());
            crate::test_support::wait_until("the close stops the builds", || core.builds.stopped());
            gt.resume.send(()).unwrap();
            closer.join().unwrap().unwrap();
        });
        drop((core, gt));
        let hold = || super::super::build::HOLD_RESTARTS.lock().unwrap();
        hold().push(d.path().to_path_buf());
        let e = reopen(d.path());
        hold().retain(|x| x != d.path());
        pause_gc(&e);
        let def = e.core().catalog.schema()[&id].clone();
        assert_eq!(def.state, State::Populating);
        let t = e.store().tree(def.tree).unwrap().unwrap();
        assert!(
            !e.store().is_empty_leaf(t).unwrap(),
            "precondition: the tree was installed"
        );
        let most = Arc::new(crate::sync::atomic::AtomicUsize::new(0));
        let m = most.clone();
        *e.core().wal.points.batch_body.lock().unwrap() = Some(Box::new(move |freed| {
            m.fetch_max(freed, crate::sync::atomic::Ordering::SeqCst);
            Ok(())
        }));
        let (gt, seen) = gate_at(&e, &[(Phase::Installed, 1)]);
        restart_builds(&e).unwrap();
        gt.wait(Phase::Installed);
        until_latch_free(&e);
        eight_writers(&e, &k, 10, 0);
        gt.resume.send(()).unwrap();
        e.wait_index(id).unwrap();
        assert_eq!(e.core().catalog.schema()[&id].state, State::Online);
        assert!(
            !seen.lock().unwrap().contains(&Phase::Scanning),
            "the restarted build scanned again"
        );
        // The collector is paused: no batch of this run frees a page.
        assert_eq!(most.load(crate::sync::atomic::Ordering::SeqCst), 0);
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
    }

    /// Where a build is paused when its index or graph is dropped.
    #[derive(Debug, Clone, Copy)]
    enum At {
        Scan,
        PageWrites,
        Replay,
    }

    /// Audit F3 (M8, M9): an index, or its graph, is dropped while its
    /// build is paused before its scan, in its page writes or in its
    /// replay, between reading a chunk and committing it; garbage
    /// collection then runs and new commits reuse the pages it frees. The
    /// trees the build reads stay allocated (`Builds::uses`), so the build
    /// ends without error, and it writes nothing to the dropped index tree
    /// (the install's check, and `Chunk::Gone` for the read chunk, Round C
    /// audit N4).
    #[test]
    fn a_build_whose_index_or_graph_is_dropped_while_paused_reads_and_writes_no_freed_tree() {
        for drop_graph in [false, true] {
            for at in [At::Scan, At::PageWrites, At::Replay] {
                let (_d, e) = db();
                pause_gc(&e);
                let k = tokens(&e);
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                let g2 = tx
                    .create_graph(DEFAULT_SCHEMA_ID, "g2", OWNER, None)
                    .unwrap();
                tx.commit().unwrap();
                seed_l(&e, g2, k.l, k.p, 300);
                let pages = match at {
                    At::PageWrites => Some(page_gate(&e)),
                    _ => None,
                };
                let (gt, _) = gate_at(
                    &e,
                    match at {
                        At::Scan => &[(Phase::Scanning, 1)][..],
                        At::PageWrites => &[],
                        At::Replay => &[(Phase::Scanned, 1), (Phase::ChunkRead, 1)],
                    },
                );
                let id = e
                    .create_index(g2, SchemaKind::Range, Entity::Node, k.l, &[k.p])
                    .unwrap();
                let def = e.core().catalog.schema()[&id].clone();
                match at {
                    At::Scan => gt.wait(Phase::Scanning),
                    At::PageWrites => pages.as_ref().unwrap().wait(),
                    At::Replay => {
                        gt.wait(Phase::Scanned);
                        seed_l(&e, g2, k.l, k.p, 1);
                        gt.resume.send(()).unwrap();
                        gt.wait(Phase::ChunkRead);
                    }
                }
                let entries = |e: &Engine| {
                    let store = e.store();
                    let t = store.tree(def.tree).unwrap().unwrap();
                    let mut n = Vec::new();
                    store
                        .scan_prefix(t, b"", |k, _| {
                            n.push(k.to_vec());
                            true
                        })
                        .unwrap();
                    n
                };
                let before = entries(&e);
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                if drop_graph {
                    assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "g2").unwrap());
                } else {
                    assert!(tx.drop_schema(g2, id).unwrap());
                }
                tx.commit().unwrap();
                crate::tx::gc::pass(e.core()).unwrap();
                // Commits that would take the pages the pass freed.
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                let g = graph(&mut tx);
                let big = Value::String("x".repeat(3000).into());
                for _ in 0..40 {
                    tx.create_node(&g, &[k.m], &[(k.q, big.clone())]).unwrap();
                }
                tx.commit().unwrap();
                match at {
                    At::PageWrites => pages.as_ref().unwrap().resume.send(()).unwrap(),
                    _ => gt.resume.send(()).unwrap(),
                }
                e.wait_index(id)
                    .unwrap_or_else(|x| panic!("{at:?} graph {drop_graph}: {x}"));
                assert!(e.core().catalog.schema().is_empty());
                assert_eq!(
                    entries(&e),
                    before,
                    "{at:?} graph {drop_graph}: the build wrote to its dropped index tree"
                );
                crate::tx::gc::pass(e.core()).unwrap();
                assert!(e.store().tree(def.tree).unwrap().is_none());
                assert_intact(&e);
            }
        }
    }

    /// Audit F3 (M15, TS-12): the index check finds an entry whose element
    /// has no record, a record without its entry, and an entry with a
    /// wrong value, in both directions.
    #[test]
    fn the_index_check_reports_entries_and_records_that_do_not_match() {
        let (_d, e) = db();
        let k = tokens(&e);
        let n = seed_l(&e, DEFAULT_GRAPH_ID, k.l, k.p, 3);
        let id = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        e.wait_index(id).unwrap();
        assert_intact(&e);
        let t = e
            .store()
            .tree(e.core().catalog.schema()[&id].tree)
            .unwrap()
            .unwrap();
        let key = |v: i64, el: u64| {
            [
                &id.to_be_bytes()[..],
                &enc(&Value::Int(v)),
                &el.to_be_bytes(),
            ]
            .concat()
        };
        let (stray, lost, right, wrong) = (key(7, 999), key(0, n[0]), key(1, n[1]), key(5, n[1]));
        let batch: Vec<Write<'_>> = [
            (&stray, true),
            (&lost, false),
            (&right, false),
            (&wrong, true),
        ]
        .into_iter()
        .map(|(key, put)| Write {
            tree: t,
            key,
            value: put.then_some(&[][..]),
            stamp: 0,
        })
        .collect();
        e.commit(&batch, crate::tx::commit::Ids::default()).unwrap();
        let p = e.check_integrity().unwrap();
        let has = |s: &str| p.iter().any(|x| x.contains(s));
        assert!(has("matches no record of element 999"), "{p:?}");
        assert!(has(&format!("element {} has no entry", n[0])), "{p:?}");
        assert!(has(&format!("element {} has no entry", n[1])), "{p:?}");
        assert!(
            has(&format!("matches no record of element {}", n[1])),
            "{p:?}"
        );
    }

    /// Audit F3 (M17, TS-18): a writer of a value of a uniqueness
    /// constraint holds the X lock on `(constraint, value)` until it ends; a
    /// second writer of that value waits in the lock table, then fails if
    /// the first committed and succeeds if it aborted.
    #[test]
    fn a_second_writer_of_a_unique_value_waits_for_the_first_to_end() {
        let (_d, e) = db();
        let k = tokens(&e);
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Unique,
            Entity::Node,
            k.l,
            &[k.p],
        )
        .unwrap();
        for (v, commit) in [(7, true), (8, false)] {
            let mut first = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut first);
            first
                .create_node(&g, &[k.l], &[(k.p, Value::Int(v))])
                .unwrap();
            let r = std::thread::scope(|s| {
                let second = s.spawn(|| commit_node(&e, k.l, k.p, v));
                until_waiting(&e, 1);
                assert!(!second.is_finished(), "the second writer waits");
                if commit {
                    first.commit().unwrap();
                } else {
                    first.abort();
                }
                second.join().unwrap()
            });
            if commit {
                assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
            } else {
                r.unwrap();
            }
        }
        assert_intact(&e);
    }

    /// Audit F5 (FR-IDX-002, rmp DECISION 224): a property type constraint
    /// holds a closed union of types and typed lists, as Neo4j allows. A
    /// union accepts each member type and rejects the others; `LIST<T NOT
    /// NULL>` accepts lists of `T`, the empty list among them, and rejects
    /// mixed lists and lists holding null; existing data is validated when
    /// the constraint is created; the union survives a reopen.
    #[test]
    fn type_constraints_accept_closed_unions_and_typed_lists() {
        use PropertyType as P;
        let (d, e) = db();
        let k = tokens(&e);
        let list = |v: Vec<Value>| Value::List(v.into());
        let union = TypeSet::of(P::Integer)
            .or(TypeSet::of(P::Float))
            .or(TypeSet::of(P::String))
            .or(TypeSet::list(P::Integer));
        // BYTES, an lpg extension (user decision 2026-10-08, rmp #34), in
        // a union.
        let bytes = TypeSet::of(P::Bytes)
            .or(TypeSet::list(P::Bytes))
            .or(TypeSet::of(P::Boolean));
        let b = |x: &[u8]| Value::Bytes(x.into());
        let attempt_on = |label: u64, key: u64, v: Value| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            tx.create_node(&g, &[label], &[(key, v)]).unwrap();
            tx.commit()
        };
        let attempt = |label: u64, v: Value| attempt_on(label, k.q, v);
        // Existing data: an L node violates the union, M nodes do not.
        attempt(k.l, Value::Bool(true)).unwrap();
        for v in [
            Value::Int(1),
            Value::Float(2.5),
            Value::string("a"),
            list(vec![Value::Int(1), Value::Int(2)]),
            list(vec![]),
        ] {
            attempt(k.m, v).unwrap();
        }
        let r = e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Type(union),
            Entity::Node,
            k.l,
            &[k.q],
        );
        assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
        assert!(e.core().catalog.schema().is_empty());
        let c = e
            .create_constraint(
                DEFAULT_GRAPH_ID,
                SchemaKind::Type(union),
                Entity::Node,
                k.m,
                &[k.q],
            )
            .unwrap();
        for v in [
            Value::Int(3),
            Value::Float(1.5),
            Value::string("b"),
            list(vec![Value::Int(4), Value::Int(5)]),
            list(vec![]),
        ] {
            attempt(k.m, v.clone()).unwrap_or_else(|x| panic!("{v:?}: {x}"));
        }
        for v in [
            Value::Bool(true),
            list(vec![Value::Int(1), Value::Float(2.0)]),
            list(vec![Value::Int(1), Value::Null]),
            list(vec![Value::string("a")]),
            list(vec![list(vec![Value::Int(1)])]),
            b(b"x"),
            list(vec![b(b"x")]),
        ] {
            let r = attempt(k.m, v.clone());
            assert!(
                matches!(r, Err(Error::ConstraintViolation(_))),
                "{v:?}: {r:?}"
            );
        }
        let cb = e
            .create_constraint(
                DEFAULT_GRAPH_ID,
                SchemaKind::Type(bytes),
                Entity::Node,
                k.m,
                &[k.w],
            )
            .unwrap();
        for v in [
            b(b""),
            b(&[0, 255]),
            list(vec![b(b"a"), b(b"b")]),
            list(vec![]),
            Value::Bool(false),
        ] {
            attempt_on(k.m, k.w, v.clone()).unwrap_or_else(|x| panic!("{v:?}: {x}"));
        }
        for v in [
            Value::Int(1),
            Value::string("x"),
            list(vec![b(b"a"), Value::Null]),
            list(vec![b(b"a"), Value::string("a")]),
            list(vec![Value::Bool(true)]),
        ] {
            let r = attempt_on(k.m, k.w, v.clone());
            assert!(
                matches!(r, Err(Error::ConstraintViolation(_))),
                "{v:?}: {r:?}"
            );
        }
        // Uniqueness: 1 and 1.0 are one value (TS-21 key encoding).
        e.create_constraint(
            DEFAULT_GRAPH_ID,
            SchemaKind::Unique,
            Entity::Node,
            k.l,
            &[k.p],
        )
        .unwrap();
        commit_node(&e, k.l, k.p, 1).unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let r = tx.create_node(&g, &[k.l], &[(k.p, Value::Float(1.0))]);
        assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
        tx.abort();
        drop(e);
        let e = reopen(d.path());
        assert_eq!(e.core().catalog.schema()[&c].kind, SchemaKind::Type(union));
        assert_eq!(e.core().catalog.schema()[&cb].kind, SchemaKind::Type(bytes));
        assert_intact(&e);
    }

    // -----------------------------------------------------------------------
    // Backups during builds (FR-OPS-001, TS-26) and the batch charge of
    // derived writes (FR-LIM-007, TS-16 Batch).

    /// Restores the backup in `artefact` into `target` and opens it without
    /// restarting its builds, so that the test reads the restored state
    /// first ([`super::super::build::HOLD_RESTARTS`]).
    fn restore_held(artefact: &std::path::Path, target: &std::path::Path) -> Engine {
        let hold = || super::super::build::HOLD_RESTARTS.lock().unwrap();
        hold().push(target.to_path_buf());
        let r = crate::ops::restore::restore(
            artefact,
            target,
            crate::test_support::no_flush(),
            crate::tx::tests::CAP,
            Arc::new(crate::obs::recording::RecordingSink::default()),
        );
        hold().retain(|x| x != target);
        r.unwrap()
    }

    /// Restarts the builds of the restored database `r`, waits for index
    /// `id` to become ONLINE and checks the database and the index.
    fn finish_restored_build(r: &Engine, id: u64) {
        restart_builds(r).unwrap();
        r.wait_index(id).unwrap();
        assert_eq!(r.core().catalog.schema()[&id].state, State::Online);
        assert_intact(r);
        assert_queries_after_writers(r, &tokens(r), &[id]);
    }

    /// Backs `e` up into `target`, pausing the backup once its data file is
    /// copied (test point `backup_copied`) while `during` runs. A panic in
    /// `during` lets the backup go on, so a failing test does not hang.
    fn backup_around(e: &Engine, target: &std::path::Path, during: impl FnOnce()) {
        let (rt, rr) = mpsc::channel();
        let (st, sr) = mpsc::channel::<()>();
        {
            let (rt, sr) = (Mutex::new(rt), Mutex::new(sr));
            *e.core().wal.points.backup_copied.lock().unwrap() = Some(Box::new(move || {
                let _ = rt.lock().unwrap().send(());
                let _ = sr.lock().unwrap().recv();
            }));
        }
        std::thread::scope(|s| {
            let backup = s.spawn(|| e.backup(target));
            let resume = st;
            rr.recv_timeout(crate::test_support::WAIT_BOUND)
                .expect("the backup copies the data file");
            during();
            drop(resume);
            backup.join().unwrap().unwrap();
        });
        *e.core().wal.points.backup_copied.lock().unwrap() = None;
    }

    /// An install or release step of a bulk build waits for the backup, or
    /// `done` holds (the build went on without waiting).
    fn until_a_step_waits_or(e: &Engine, done: impl Fn() -> bool) {
        crate::test_support::wait_until("a build step waits for the backup", || {
            e.core()
                .install_waits
                .load(crate::sync::atomic::Ordering::SeqCst)
                > 0
                || done()
        });
    }

    fn install_waits(e: &Engine) -> u64 {
        e.core()
            .install_waits
            .load(crate::sync::atomic::Ordering::SeqCst)
    }

    /// TS-28: every page below the page count of `r` has a valid image:
    /// its frame if the pool holds it dirty, else its image on disk.
    /// Migration reads every page; the integrity check reads the tree and
    /// free-listed pages but not the pages of a build record.
    fn assert_every_page_valid(r: &Engine) {
        let mut buf = vec![0u8; crate::storage::page::PAGE_SIZE];
        let bad: Vec<u64> = (0..r.core().data.page_count())
            .filter(|&p| {
                !matches!(r.pool().resident(p), Some((_, true)))
                    && !r.core().data.read_page_unverified(p, &mut buf).unwrap()
            })
            .collect();
        assert!(bad.is_empty(), "pages without a valid image: {bad:?}");
    }

    /// Commits `n` nodes of label `l` with `p` = 0, 1, ..., n - 1.
    fn seed_unique(e: &Engine, l: u64, p: u64, n: i64) {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        for i in 0..n {
            tx.create_node(&g, &[l], &[(p, Value::Int(i))]).unwrap();
        }
        tx.commit().unwrap();
    }

    /// Where a build is paused while a backup runs.
    #[derive(Debug, Clone, Copy)]
    enum Paused {
        Scan,
        PageWrites,
        Replay,
    }

    /// FR-OPS-001, TS-26: a backup taken while an index build is paused in
    /// its scan, in its page writes or in its replay succeeds. The restored
    /// database passes the integrity check with its index POPULATING; its
    /// restarted build ends ONLINE and the index answers as a full scan.
    /// Before the fix, a backup during the page writes failed with
    /// [`Error::LoadRunning`].
    #[test]
    fn a_backup_taken_while_a_build_is_paused_restores_and_the_rebuilt_index_matches_the_data() {
        for at in [Paused::Scan, Paused::PageWrites, Paused::Replay] {
            let (_d, e) = db();
            let k = tokens(&e);
            seed_l(&e, DEFAULT_GRAPH_ID, k.l, k.p, 300);
            let (target, restored) = (graph_dir(), graph_dir());
            let phases: &[(Phase, usize)] = match at {
                Paused::Scan => &[(Phase::Scanning, 1)],
                Paused::PageWrites => &[],
                Paused::Replay => &[(Phase::Scanned, 1), (Phase::ChunkRead, 1)],
            };
            let (gt, _) = gate_at(&e, phases);
            let pages = matches!(at, Paused::PageWrites).then(|| page_gate(&e));
            let id = e
                .create_index(
                    DEFAULT_GRAPH_ID,
                    SchemaKind::Range,
                    Entity::Node,
                    k.l,
                    &[k.p],
                )
                .unwrap();
            match at {
                Paused::Scan => gt.wait(Phase::Scanning),
                Paused::PageWrites => pages.as_ref().unwrap().wait(),
                Paused::Replay => {
                    gt.wait(Phase::Scanned);
                    // Side-tree records for the replay.
                    for v in 0..20 {
                        commit_node(&e, k.l, k.p, v % 5).unwrap();
                    }
                    gt.resume.send(()).unwrap();
                    gt.wait(Phase::ChunkRead);
                }
            }
            let r = e.backup(target.path());
            gt.resume.send(()).unwrap();
            if let Some(p) = &pages {
                p.resume.send(()).unwrap();
            }
            r.unwrap_or_else(|x| panic!("{at:?}: the backup failed: {x}"));
            e.wait_index(id).unwrap();
            assert_intact(&e);
            assert_queries_after_writers(&e, &k, &[id]);
            let r = restore_held(target.path(), restored.path());
            assert_eq!(r.core().catalog.schema()[&id].state, State::Populating);
            assert_intact(&r);
            assert_every_page_valid(&r);
            finish_restored_build(&r, id);
        }
    }

    /// FR-OPS-001, TS-26: a uniqueness constraint whose owned index is
    /// built while a backup is paused after copying the data file reaches
    /// its commit, which installs the index, during the backup: the commit
    /// waits for the backup, so the restored database, taken before it,
    /// has no constraint, releases the build's pages at its open and passes
    /// the integrity check; the source commits the constraint after the
    /// backup. Without the wait, the backup's WAL holds the install of
    /// pages its data copy lacks, and the restored database fails the
    /// check.
    #[test]
    fn a_constraint_commit_during_a_backup_waits_and_the_backup_restores_intact() {
        let (_d, e) = db();
        let k = tokens(&e);
        seed_unique(&e, k.l, k.p, 2000);
        let (target, restored) = (graph_dir(), graph_dir());
        let id = std::thread::scope(|s| {
            let mut creator = None;
            backup_around(&e, target.path(), || {
                let c = s.spawn(|| {
                    e.create_constraint(
                        DEFAULT_GRAPH_ID,
                        SchemaKind::Unique,
                        Entity::Node,
                        k.l,
                        &[k.p],
                    )
                });
                until_a_step_waits_or(&e, || c.is_finished());
                creator = Some(c);
            });
            creator.unwrap().join().unwrap().unwrap()
        });
        assert_eq!(e.core().catalog.schema()[&id].state, State::Online);
        assert_intact(&e);
        let r = restore_held(target.path(), restored.path());
        assert_intact(&r);
        assert_every_page_valid(&r);
        assert!(!r.core().catalog.schema().contains_key(&id));
        assert!(r.store().build_records().unwrap().is_empty());
        assert!(install_waits(&e) > 0, "the commit did not wait");
    }

    /// FR-OPS-001, TS-26: a uniqueness constraint over data with a
    /// duplicate fails after its owned index has written pages, while a
    /// backup is paused after copying the data file. Its pages are released
    /// through the free list (a commit has taken pages above its run): each
    /// release step writes page images and frees them, so it waits for the
    /// backup. The restored database holds the build's record, which its
    /// open releases, and passes the integrity check. Without the wait, the
    /// backup's WAL frees pages whose images its data copy lacks.
    #[test]
    fn a_build_release_during_a_backup_waits_and_the_backup_restores_intact() {
        let (_d, e) = db();
        let k = tokens(&e);
        seed_unique(&e, k.l, k.p, 2000);
        // The largest value twice: the duplicate is found last.
        commit_node(&e, k.l, k.p, 1999).unwrap();
        let (target, restored) = (graph_dir(), graph_dir());
        std::thread::scope(|s| {
            // Inside the scope: a failing assertion drops it, which lets the
            // paused build go on, before the scope joins the creator.
            let pages = page_gate(&e);
            let creator = s.spawn(|| {
                e.create_constraint(
                    DEFAULT_GRAPH_ID,
                    SchemaKind::Unique,
                    Entity::Node,
                    k.l,
                    &[k.p],
                )
            });
            pages.wait();
            // Pages above the build's run, so that its release frees.
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            for i in 0..200 {
                let v = Value::string(&format!("{i:0>1000}"));
                tx.create_node(&g, &[k.m], &[(k.q, v)]).unwrap();
            }
            tx.commit().unwrap();
            let records = e.store().build_records().unwrap();
            let &(f, l) = records[0].1.last().unwrap();
            assert!(
                e.core().data.page_count() > f + l,
                "precondition: pages above the run"
            );
            backup_around(&e, target.path(), || {
                pages.resume.send(()).unwrap();
                until_a_step_waits_or(&e, || creator.is_finished());
            });
            let r = creator.join().unwrap();
            assert!(matches!(r, Err(Error::ConstraintViolation(_))), "{r:?}");
        });
        assert!(e.store().build_records().unwrap().is_empty());
        assert_intact(&e);
        let r = restore_held(target.path(), restored.path());
        assert_intact(&r);
        assert_every_page_valid(&r);
        assert!(r.store().build_records().unwrap().is_empty());
        assert!(install_waits(&e) > 0, "the release did not wait");
    }

    /// FR-OPS-001, TS-26, rmp #18's commit-point property with a build:
    /// four threads commit nodes while an index build runs and a backup,
    /// paused after copying the data file, overlaps the build's page writes
    /// and install. The restored database holds exactly the commits up to
    /// one commit point, every commit that returned before the backup,
    /// passes the integrity check, and its restarted build ends ONLINE with
    /// an index that answers as a full scan.
    #[test]
    fn a_backup_under_four_committers_and_an_index_build_restores_to_one_commit_point() {
        let (_d, e) = db();
        let k = tokens(&e);
        seed_l(&e, DEFAULT_GRAPH_ID, k.l, k.p, 3000);
        let seeded: BTreeSet<u64> = {
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            let g = graph(&mut tx);
            let s = scan(&mut tx, &g, Entity::Node)
                .into_iter()
                .map(|x| x.0)
                .collect();
            tx.abort();
            s
        };
        let (target, restored) = (graph_dir(), graph_dir());
        let done = Mutex::new(Vec::<(u64, u64)>::new());
        let stop = crate::sync::atomic::AtomicBool::new(false);
        let count = || done.lock().unwrap().len();
        let (id, before) = std::thread::scope(|s| {
            for t in 0..4u64 {
                let (e, done, stop, k) = (&e, &done, &stop, &k);
                s.spawn(move || {
                    let mut rng = StdRng::seed_from_u64(seed() ^ t);
                    while !stop.load(crate::sync::atomic::Ordering::Acquire) {
                        let mut tx = e.begin(Isolation::Serializable).unwrap();
                        let g = graph(&mut tx);
                        let v = Value::Int(rng.random_range(0..5));
                        let r = tx
                            .create_node(&g, &[k.l], &[(k.p, v)])
                            .and_then(|n| tx.commit().map(|ts| (ts.unwrap(), n)));
                        match r {
                            Ok(d) => done.lock().unwrap().push(d),
                            Err(Error::Serialization | Error::Deadlock | Error::Aborted) => {}
                            Err(x) => panic!("{x}"),
                        }
                    }
                });
            }
            // Ends the committers also when an assertion below fails.
            struct Stop<'a>(&'a crate::sync::atomic::AtomicBool);
            impl Drop for Stop<'_> {
                fn drop(&mut self) {
                    self.0.store(true, crate::sync::atomic::Ordering::Release);
                }
            }
            let _stop = Stop(&stop);
            crate::test_support::wait_until("50 commits return", || count() >= 50);
            let pages = page_gate(&e);
            let id = e
                .create_index(
                    DEFAULT_GRAPH_ID,
                    SchemaKind::Range,
                    Entity::Node,
                    k.l,
                    &[k.p],
                )
                .unwrap();
            pages.wait();
            let before = done.lock().unwrap().iter().map(|d| d.0).max().unwrap();
            backup_around(&e, target.path(), || {
                pages.resume.send(()).unwrap();
                until_a_step_waits_or(&e, || e.core().catalog.schema()[&id].state == State::Online);
                let n = count();
                crate::test_support::wait_until("commits during the backup", || count() >= n + 20);
            });
            let n = count();
            crate::test_support::wait_until("commits after the backup", || count() >= n + 20);
            (id, before)
        });
        e.wait_index(id).unwrap();
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
        assert!(install_waits(&e) > 0, "the install did not wait");
        let done = done.into_inner().unwrap();
        let r = restore_held(target.path(), restored.path());
        let got: BTreeSet<u64> = {
            let mut tx = r.begin(Isolation::Snapshot).unwrap();
            let g = graph(&mut tx);
            let s = scan(&mut tx, &g, Entity::Node)
                .into_iter()
                .map(|x| x.0)
                .filter(|n| !seeded.contains(n))
                .collect();
            tx.abort();
            s
        };
        let ts_of: std::collections::BTreeMap<u64, u64> =
            done.iter().map(|&(t, n)| (n, t)).collect();
        let point = got.iter().map(|n| ts_of[n]).max().unwrap();
        let want: BTreeSet<u64> = done.iter().filter(|d| d.0 <= point).map(|d| d.1).collect();
        assert!(
            got == want,
            "the restored state is not the state after commit {point}"
        );
        assert!(
            point >= before,
            "a commit that returned before the backup is missing"
        );
        assert_eq!(r.core().catalog.schema()[&id].state, State::Populating);
        assert_intact(&r);
        assert_every_page_valid(&r);
        finish_restored_build(&r, id);
    }

    /// FR-LIM-007, TS-16 Batch: the side-tree records a commit derives for
    /// a POPULATING index reserve their bytes under the commit latch. A
    /// commit whose records exceed the memory left fails with the
    /// memory-limit error and applies nothing; the database stays usable,
    /// a later commit succeeds and the build ends with an index that
    /// answers as a full scan. Without the reservation the commit
    /// succeeds.
    #[test]
    fn a_commit_whose_index_records_exceed_the_cap_fails_and_applies_nothing() {
        let (_d, e) = db();
        let k = tokens(&e);
        let (gt, _) = gate_at(&e, &[(Phase::Scanning, 1)]);
        let id = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        gt.wait(Phase::Scanning);
        let side = e.core().catalog.schema()[&id].side;
        let side = e.store().tree(side).unwrap().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        // One small side-tree record per node, each charged its bytes and
        // a write's overhead (measured 2026-10-08: the commit fails with 24
        // pages left and succeeds with 32; without the reservation it
        // succeeds with 8).
        for i in 0..2000 {
            tx.create_node(&g, &[k.l], &[(k.p, Value::Int(i % 5))])
                .unwrap();
        }
        // Hold every byte the pool can give up, then free 8 pages.
        let mut hog = Vec::new();
        while let Ok(r) = e.pool().reserve(8192) {
            hog.push(r);
        }
        hog.truncate(hog.len() - 8);
        let r = tx.commit();
        drop(hog);
        assert!(matches!(r, Err(Error::MemoryLimit { .. })), "{r:?}");
        assert!(!e.core().health.is_unusable());
        let mut n = 0;
        e.store()
            .scan_prefix(side, b"", |_, _| {
                n += 1;
                true
            })
            .unwrap();
        assert_eq!(n, 0, "the failed commit wrote side-tree records");
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        assert!(scan(&mut tx, &g, Entity::Node).is_empty());
        tx.abort();
        commit_node(&e, k.l, k.p, 1).unwrap();
        gt.resume.send(()).unwrap();
        e.wait_index(id).unwrap();
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
    }

    /// A database under a 4 MiB memory cap, with garbage collection paused
    /// and an ONLINE range index on `L.p`, and the index's identifier.
    fn small_cap_db() -> (tempfile::TempDir, Engine, Tokens, u64) {
        let (d, e) = crate::tx::tests::db_with_cap(4 << 20);
        pause_gc(&e);
        let k = tokens(&e);
        let id = e
            .create_index(
                DEFAULT_GRAPH_ID,
                SchemaKind::Range,
                Entity::Node,
                k.l,
                &[k.p],
            )
            .unwrap();
        e.wait_index(id).unwrap();
        (d, e, k, id)
    }

    /// Fills the cache up to the memory cap with 6 MiB of node records that
    /// no index covers.
    fn fill_cache(e: &Engine, k: &Tokens) {
        let big = Value::string(&"x".repeat(3000));
        for _ in 0..10 {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            for _ in 0..200 {
                tx.create_node(&g, &[k.m], &[(k.q, big.clone())]).unwrap();
            }
            tx.commit().unwrap();
        }
        // Reading them back grows the cache through its miss path.
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        assert!(scan(&mut tx, &g, Entity::Node).len() >= 2000);
        tx.abort();
    }

    /// Asserts that less than one page of the cap is free.
    fn assert_cache_full(e: &Engine) {
        let a = &e.core().accountant;
        assert!(
            a.cap() - a.used() < 8192,
            "precondition: {} of {} bytes used",
            a.used(),
            a.cap()
        );
    }

    /// Every byte the pool can give up, held.
    fn hog_all(e: &Engine) -> Vec<crate::storage::memory::Reservation> {
        let mut hog = Vec::new();
        while let Ok(r) = e.pool().reserve(8192) {
            hog.push(r);
        }
        hog
    }

    /// Releases `bytes` of `hog`.
    fn release(hog: &mut Vec<crate::storage::memory::Reservation>, mut bytes: u64) {
        while bytes > 0 {
            let r = hog.last_mut().expect("the hog holds enough");
            let n = bytes.min(r.bytes());
            r.shrink(n);
            bytes -= n;
            if r.bytes() == 0 {
                hog.pop();
            }
        }
    }

    /// The index key of node `node` with `L.p = v`.
    fn key_of(e: &Engine, id: u64, k: &Tokens, node: u64, v: i64) -> Vec<u8> {
        let def = e.core().catalog.schema()[&id].clone();
        let el = Element {
            tokens: vec![k.l],
            props: vec![(k.p, Value::Int(v))],
        };
        let mut keys = index_keys(&def, node, &el);
        assert_eq!(keys.len(), 1);
        keys.remove(0)
    }

    /// A transaction that creates 2 000 nodes of `L` with `p` in 0..5, and
    /// their nodes.
    fn create_indexed<'e>(e: &'e Engine, k: &Tokens) -> (Transaction<'e>, Vec<(u64, i64)>) {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let nodes = (0..2000)
            .map(|i| {
                let v = i % 5;
                (
                    tx.create_node(&g, &[k.l], &[(k.p, Value::Int(v))]).unwrap(),
                    v,
                )
            })
            .collect();
        (tx, nodes)
    }

    /// A transaction that sets `L.p = v` on `nodes`.
    fn set_p<'e>(e: &'e Engine, k: &Tokens, nodes: &[u64], v: i64) -> Transaction<'e> {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        for &n in nodes {
            tx.set_property(&g, n, k.p, Value::Int(v)).unwrap();
        }
        tx
    }

    /// 2 000 committed nodes of `L` whose index entries for `p = 1` are
    /// delete-stamped: `p` was set to 1, then to 5.
    fn deleted_at_one(e: &Engine, k: &Tokens) -> Vec<u64> {
        let (tx, nodes) = create_indexed(e, k);
        tx.commit().unwrap();
        let nodes: Vec<u64> = nodes.into_iter().map(|(n, _)| n).collect();
        set_p(e, k, &nodes, 1).commit().unwrap();
        set_p(e, k, &nodes, 5).commit().unwrap();
        nodes
    }

    /// The bytes requested by the memory-limit error of a commit made while
    /// every byte the pool can give up is held except `free` and 8 pages,
    /// the room the pool needs for the pages the commit reads.
    fn commit_with_free(e: &Engine, tx: Transaction<'_>, free: u64) -> u64 {
        // A checkpoint first: no frame becomes evictable while the hog is
        // held because its page LSN becomes durable.
        e.checkpoint().unwrap();
        let mut hog = hog_all(e);
        release(&mut hog, free + 8 * 8192);
        let r = tx.commit();
        drop(hog);
        match r {
            Err(Error::MemoryLimit { requested }) => requested,
            r => panic!("expected a memory-limit error: {r:?}"),
        }
    }

    /// Audit round 3 H1, TS-08, FR-LIM-007: with the cache full up to the
    /// cap and nothing else holding memory, a commit on an ONLINE index
    /// reserves the write keys of its index entries (TS-15) through the
    /// pool, which evicts cache frames before it refuses. Measured with
    /// `accountant.try_reserve` at that step (2026-10-08): the commit fails
    /// with `MemoryLimit { requested: 148000 }`.
    #[test]
    fn an_indexed_commit_on_a_full_cache_evicts_for_its_write_keys() {
        let (_d, e, k, id) = small_cap_db();
        fill_cache(&e, &k);
        let (tx, _) = create_indexed(&e, &k);
        assert_cache_full(&e);
        tx.commit().unwrap();
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
    }

    /// As above, for the before-images of the index entries a commit puts
    /// again over a delete-stamped entry (TS-14): the pool evicts for them.
    /// Measured with `accountant.try_reserve` at that step (2026-10-08):
    /// the commit fails with `MemoryLimit { requested: 113 }`.
    #[test]
    fn a_commit_that_puts_index_entries_again_on_a_full_cache_evicts_for_their_before_images() {
        let (_d, e, k, id) = small_cap_db();
        let nodes = deleted_at_one(&e, &k);
        fill_cache(&e, &k);
        let tx = set_p(&e, &k, &nodes, 1);
        assert_cache_full(&e);
        tx.commit().unwrap();
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
    }

    /// The memory cap still holds at both steps: when the pool has too
    /// little left to evict, a commit fails at the step that reserves the
    /// write keys of its index entries, or at a before-image of an index
    /// entry put again, with the memory-limit error for exactly those
    /// bytes, applies nothing, and leaves the database usable.
    #[test]
    fn an_indexed_commit_over_the_cap_still_fails_at_both_steps() {
        let (_d, e, k, id) = small_cap_db();
        let charge = |nodes: &[(u64, i64)]| -> u64 {
            nodes
                .iter()
                .map(|&(n, v)| WriteKeys::charge_of(&key_of(&e, id, &k, n, v)))
                .sum()
        };
        // Nothing beyond the pool's room free: the commit fails at its
        // first reservation, the bytes of its batch's index entries.
        let (tx, nodes) = create_indexed(&e, &k);
        let batch = commit_with_free(&e, tx, 0);
        let more = charge(&nodes);
        assert_ne!(batch, more);
        // The batch and half the write keys free.
        let (tx, nodes) = create_indexed(&e, &k);
        assert_eq!(charge(&nodes), more);
        assert_eq!(commit_with_free(&e, tx, batch + more / 2), more);
        assert!(!e.core().health.is_unusable());
        assert_queries_after_writers(&e, &k, &[id]);
        // `p = 1` put again over 2 000 delete-stamped entries.
        let ids = deleted_at_one(&e, &k);
        let batch = commit_with_free(&e, set_p(&e, &k, &ids, 1), 0);
        let at = |v| ids.iter().map(|&n| (n, v)).collect::<Vec<_>>();
        let more = charge(&at(5)) + charge(&at(1));
        assert_ne!(batch, more);
        let image = version_bytes(&key_of(&e, id, &k, ids[0], 1), 0);
        // The batch, the write keys and half the before-images free.
        let free = batch + more + 1000 * image;
        assert_eq!(commit_with_free(&e, set_p(&e, &k, &ids, 1), free), image);
        assert!(!e.core().health.is_unusable());
        assert_queries_after_writers(&e, &k, &[id]);
        set_p(&e, &k, &ids, 1).commit().unwrap();
        assert_intact(&e);
        assert_queries_after_writers(&e, &k, &[id]);
    }
}

/// Crash tests of the online build (rmp #33): fault storage.
#[cfg(not(any(loom, shuttle)))]
mod crash {
    use super::super::build::{Hook, Phase};
    use super::super::*;
    use crate::catalog::{DEFAULT_GRAPH_ID, TokenKind};
    use crate::obs::recording::RecordingSink;
    use crate::storage::FsStorage;
    use crate::storage::fault::{Crash, FaultStorage};
    use crate::sync::Arc;
    use crate::tx::Isolation;
    use crate::tx::tests::{CAP, tuning};

    fn open(dir: &std::path::Path, f: Arc<FaultStorage>) -> Engine {
        Engine::open_tuned(dir, f, CAP, Arc::new(RecordingSink::default()), tuning()).unwrap()
    }

    /// Creates a database with 150 nodes and runs a build of a range index
    /// on it, whose replay has records: at its scan's end, the hook commits
    /// changes. At the first page the build writes, another hook commits
    /// nodes whose records take pages above the build's run, so its unused
    /// pages are released through the free list ([`crate::storage::btree::
    /// bulk`]). If `point` is given, the process stops after that many
    /// crash-point calls of the build ([`FaultStorage::crash_after`]).
    /// Returns the
    /// crash points before the build, the point count at the ONLINE commit
    /// and the point count after the build.
    fn run(dir: &std::path::Path, f: &Arc<FaultStorage>, point: Option<u64>) -> (u64, u64, u64) {
        let e = Engine::create_tuned(
            dir,
            f.clone(),
            CAP,
            crate::test_support::OWNER,
            Arc::new(RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        let l = e.token(TokenKind::Label, "L").unwrap();
        let p = e.token(TokenKind::PropertyKey, "p").unwrap();
        let q = e.token(TokenKind::PropertyKey, "q").unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
        let mut nodes = Vec::new();
        for i in 0..150 {
            nodes.push(
                tx.create_node(&g, &[l], &[(p, Value::Int(i % 40))])
                    .unwrap(),
            );
        }
        tx.commit().unwrap();
        let before = f.points();
        let online = Arc::new(crate::sync::atomic::AtomicU64::new(0));
        // A weak reference: the hook lives in the core.
        let (core, at, ff) = (Arc::downgrade(&e.core_arc()), online.clone(), f.clone());
        let hook: Hook = Arc::new(move |phase| match phase {
            Phase::Scanned => {
                let Some(core) = core.upgrade() else { return };
                let h = Engine::handle(core);
                for &n in &nodes[..30] {
                    let r = h.begin(Isolation::Serializable).and_then(|mut tx| {
                        let g = tx.graph(DEFAULT_GRAPH_ID)?;
                        tx.set_property(&g, n, p, Value::Int(1000 + n as i64))?;
                        tx.commit()
                    });
                    if r.is_err() {
                        return;
                    }
                }
            }
            Phase::BeforeOnline => at.store(ff.points(), crate::sync::atomic::Ordering::SeqCst),
            _ => {}
        });
        *e.core().builds.hook.lock().unwrap() = Some(hook);
        let (core, first) = (
            Arc::downgrade(&e.core_arc()),
            crate::sync::atomic::AtomicBool::new(true),
        );
        *e.core().wal.points.bulk_page.lock().unwrap() = Some(Box::new(move || {
            if !first.swap(false, crate::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let Some(core) = core.upgrade() else { return };
            let big = Value::String("x".repeat(3000).into());
            let _ = Engine::handle(core)
                .begin(Isolation::Serializable)
                .and_then(|mut tx| {
                    let g = tx.graph(DEFAULT_GRAPH_ID)?;
                    for _ in 0..5 {
                        tx.create_node(&g, &[l], &[(q, big.clone())])?;
                    }
                    tx.commit()
                });
        }));
        if let Some(k) = point {
            f.crash_after(k);
        }
        if let Ok(id) = e.create_index(DEFAULT_GRAPH_ID, SchemaKind::Range, Entity::Node, l, &[p]) {
            let r = e.wait_index(id);
            if point.is_none() {
                r.unwrap();
                let c = e.store().check().unwrap();
                assert!(c.is_ok(), "{:?}", c.problems);
                assert!(e.store().build_records().unwrap().is_empty());
                // The build used one page of its 64-page run; the other 63
                // were released through the free list, not the reserve.
                assert!(
                    c.free_pages >= crate::storage::btree::bulk::MIN_RUN - 1,
                    "precondition: the unused pages went through the free list: {}",
                    c.free_pages
                );
            }
        }
        let after = f.points();
        drop(e);
        (
            before,
            online.load(crate::sync::atomic::Ordering::SeqCst),
            after,
        )
    }

    /// rmp #33: a crash at every crash point of a build (fault storage)
    /// reopens with the index POPULATING, unless its ONLINE commit was
    /// reached, and the build restarted at open ends ONLINE and passes the
    /// integrity check.
    #[test]
    fn a_crash_at_every_point_of_a_build_reopens_populating_and_the_restarted_build_ends_online() {
        let base = crate::test_support::graph_base();
        let fault = || {
            let f = Arc::new(FaultStorage::new(Box::new(FsStorage)));
            f.skip_device_sync();
            f
        };
        let reference = tempfile::tempdir_in(base).unwrap();
        let (first, online, last) = run(reference.path(), &fault(), None);
        assert!(first < online && online < last);
        let mut populating = 0;
        let mut resumed = false;
        for j in 1..=last - first {
            let d = tempfile::tempdir_in(base).unwrap();
            let f = fault();
            // The process stops after crash-point call `k`. The point count
            // when this run reached its ONLINE commit, if it did, is its
            // own: WAL groups may batch differently from run to run.
            let (before, online, _) = run(d.path(), &f, Some(j));
            let k = before + j;
            let mode = match k % 3 {
                0 => Crash::DropAll,
                1 => Crash::KeepRandom { seed: k },
                _ => Crash::Tear { seed: k },
            };
            f.crash(mode).unwrap();
            let hold = || super::super::build::HOLD_RESTARTS.lock().unwrap();
            hold().push(d.path().to_path_buf());
            let mut e = open(d.path(), f.clone());
            hold().retain(|x| x != d.path());
            // The open released the pages of an interrupted build: no
            // record is left, and every page is in a tree or free.
            assert!(
                e.store().build_records().unwrap().is_empty(),
                "point {k}: a build record survives the open"
            );
            let p = e.check_integrity().unwrap();
            assert!(p.is_empty(), "point {k}, before the restart: {p:?}");
            // Released pages hold valid images: once a checkpoint has
            // written the pool, every page below the page count verifies
            // (TS-28 migration reads them all).
            e.checkpoint().unwrap();
            let mut buf = vec![0u8; crate::storage::page::PAGE_SIZE];
            for pg in 0..e.data().page_count() {
                assert!(
                    e.data().read_page_unverified(pg, &mut buf).unwrap(),
                    "point {k}: page {pg} holds no valid image"
                );
            }
            let defs: Vec<SchemaDef> = e.core().catalog.schema().values().cloned().collect();
            assert!(defs.len() <= 1, "point {k}");
            if let Some(def) = defs.first() {
                if online == 0 || k <= online {
                    assert_eq!(def.state, State::Populating, "point {k}");
                }
                populating += usize::from(def.state == State::Populating);
                let installed = def.state == State::Populating && {
                    let t = e.store().tree(def.tree).unwrap().unwrap();
                    !e.store().is_empty_leaf(t).unwrap()
                };
                if installed && !resumed {
                    // Audit F2: the first point that leaves the tree
                    // installed and the index POPULATING. The build then
                    // resumes at its replay; a chain of crashes stops the
                    // resumed build at its first, second, ... crash point,
                    // each followed by an open, until it ends.
                    resumed = true;
                    e = resume_chain(d.path(), &f, e, def.id, k);
                }
                restart_builds(&e).unwrap();
                e.wait_index(def.id).unwrap();
                assert_eq!(
                    e.core().catalog.schema()[&def.id].state,
                    State::Online,
                    "point {k}"
                );
            }
            let p = e.check_integrity().unwrap();
            assert!(p.is_empty(), "point {k}: {p:?}");
            assert!(e.store().build_records().unwrap().is_empty(), "point {k}");
            drop(e);
        }
        assert!(
            populating > 10,
            "most points reopen POPULATING: {populating}"
        );
        assert!(resumed, "a point leaves the index tree installed");
    }

    /// The crash chain of a build resumed at its replay (audit F2): the
    /// database at `dir`, open as `e` with its build not restarted, runs
    /// the build with a crash after its `m`-th crash point, for `m` = 1,
    /// 2, ..., reopening after each crash, until a run ends without one.
    /// After each open no build record is left and the database passes the
    /// integrity check. Returns the database, open, its build not
    /// restarted.
    fn resume_chain(
        dir: &std::path::Path,
        f: &Arc<FaultStorage>,
        mut e: Engine,
        id: u64,
        k: u64,
    ) -> Engine {
        let hold = || super::super::build::HOLD_RESTARTS.lock().unwrap();
        for m in 1.. {
            let before = f.points();
            f.crash_after(m);
            restart_builds(&e).unwrap();
            let r = e.wait_index(id);
            if f.points() < before + m {
                // No crash: the resumed build ended.
                r.unwrap();
                f.crash_after(1 << 40);
                return e;
            }
            drop(e);
            let mode = match m % 3 {
                0 => Crash::DropAll,
                1 => Crash::KeepRandom { seed: k ^ m },
                _ => Crash::Tear { seed: k ^ m },
            };
            f.crash(mode).unwrap();
            hold().push(dir.to_path_buf());
            e = open(dir, f.clone());
            hold().retain(|x| x != dir);
            assert!(
                e.store().build_records().unwrap().is_empty(),
                "point {k}, resumed point {m}: a build record survives the open"
            );
            let p = e.check_integrity().unwrap();
            assert!(p.is_empty(), "point {k}, resumed point {m}: {p:?}");
        }
        unreachable!("the chain ends when a run does not crash")
    }
}

/// `loom` model of the side-tree protocol (TS-18 steps 2, 5 and 6; rmp
/// #33): two committers change the indexed data, under the commit latch,
/// writing side-tree records while the index is POPULATING and the index
/// once it is ONLINE, against a builder that scans without the latch,
/// builds, replays the records it read in a chunk, and switches the index
/// ONLINE under the latch with [`super::side::online_tail`]. Whatever the
/// interleaving, the ONLINE index equals the data.
#[cfg(loom)]
#[test]
fn loom_side_tree_replay_and_online_switch() {
    use std::collections::{BTreeMap, BTreeSet};

    use super::side::{self, Op, Target};
    use crate::storage::error::Result;
    use crate::sync::{Arc, Mutex};

    struct Index<'a>(&'a mut BTreeSet<u8>);
    impl Target for Index<'_> {
        fn present(&mut self, key: &[u8]) -> Result<bool> {
            Ok(self.0.contains(&key[0]))
        }
        fn set(&mut self, key: &[u8], present: bool) {
            if present {
                self.0.insert(key[0]);
            } else {
                self.0.remove(&key[0]);
            }
        }
    }

    #[derive(Default)]
    struct Db {
        online: bool,
        data: BTreeMap<u8, bool>,
        side: Vec<(Op, Vec<u8>)>,
        index: BTreeSet<u8>,
    }

    loom::model(|| {
        let db = Arc::new(Mutex::new(Db {
            data: [(0, true), (1, false)].into(),
            ..Db::default()
        }));
        // Committers: element 0 is deleted, element 1 created.
        let committers: Vec<_> = [(0u8, false), (1u8, true)]
            .into_iter()
            .map(|(k, present)| {
                let db = db.clone();
                loom::thread::spawn(move || {
                    let mut d = db.lock().unwrap();
                    d.data.insert(k, present);
                    let op = if present { Op::Insert } else { Op::Delete };
                    if d.online {
                        let _ = Index(&mut d.index).set(&[k], present);
                    } else {
                        d.side.push((op, vec![k]));
                    }
                })
            })
            .collect();
        // The builder: a scan without the latch, key by key.
        let mut scanned = BTreeSet::new();
        for k in [0u8, 1] {
            if db.lock().unwrap().data.get(&k).copied().unwrap_or(false) {
                scanned.insert(k);
            }
        }
        db.lock().unwrap().index = scanned;
        // One replay chunk: the records are read, then applied under the
        // latch.
        let chunk: Vec<(Op, Vec<u8>)> = db.lock().unwrap().side.clone();
        {
            let mut d = db.lock().unwrap();
            side::replay(
                &mut Index(&mut d.index),
                chunk.iter().map(|(o, k)| (*o, k.as_slice())),
            )
            .unwrap();
        }
        // The ONLINE switch replays the tail under the latch.
        {
            let mut d = db.lock().unwrap();
            let tail: Vec<(Op, Vec<u8>)> = d.side[chunk.len()..].to_vec();
            side::online_tail(
                &mut Index(&mut d.index),
                tail.iter().map(|(o, k)| (*o, k.as_slice())),
            )
            .unwrap();
            d.online = true;
        }
        for c in committers {
            c.join().unwrap();
        }
        let d = db.lock().unwrap();
        let want: BTreeSet<u8> = d
            .data
            .iter()
            .filter(|(_, p)| **p)
            .map(|(k, _)| *k)
            .collect();
        assert_eq!(d.index, want);
    });
}
