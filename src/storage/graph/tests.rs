//! Tests of the graph layout (rmp #32): stable identifiers, concurrent
//! writers, expansion at a snapshot, node deletion against concurrent edge
//! creation, the cross-tree integrity check and the statistics.

#[cfg(not(any(loom, shuttle)))]
mod graph {
    use std::collections::{BTreeMap, BTreeSet, HashSet};

    use rand::rngs::StdRng;
    use rand::{RngExt, SeedableRng};

    use super::super::*;
    use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID, TokenKind};
    use crate::storage::error::Error;
    use crate::test_support::{OWNER, seed};
    use crate::tx::Isolation;
    use crate::tx::tests::{db, pause_gc, reopen, until_waiting};

    fn graph(tx: &mut Transaction<'_>) -> Graph {
        tx.graph(DEFAULT_GRAPH_ID).unwrap()
    }

    fn assert_intact(e: &Engine) {
        let p = e.check_integrity().unwrap();
        assert!(p.is_empty(), "{p:?}");
    }

    /// The statistics equal an exact recount of the latest committed state.
    fn assert_stats_exact(e: &Engine, what: &str) {
        let got = e.core().catalog.stats().counts().clone();
        let want = recount(e.core()).unwrap();
        assert!(got == want, "{what}: statistics {got:?}, recount {want:?}");
    }

    fn labels(e: &Engine, n: usize) -> Vec<u64> {
        (0..n)
            .map(|i| e.token(TokenKind::Label, &format!("L{i}")).unwrap())
            .collect()
    }

    fn types(e: &Engine, n: usize) -> Vec<u64> {
        (0..n)
            .map(|i| e.token(TokenKind::EdgeType, &format!("T{i}")).unwrap())
            .collect()
    }

    /// FR-TYPE-003: a node whose labels and properties change in 50 commits
    /// keeps its identifier, also after a clean reopen and after a drop
    /// without close; the changes draw no identifier.
    #[test]
    fn a_node_changed_in_fifty_commits_keeps_its_identifier_also_after_reopen() {
        let (d, e) = db();
        let l = labels(&e, 3);
        let t = types(&e, 1);
        let key = e.token(TokenKind::PropertyKey, "p").unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let id = tx
            .create_node(&g, &[l[0]], &[(key, Value::Int(0))])
            .unwrap();
        let other = tx.create_node(&g, &[l[1]], &[]).unwrap();
        tx.create_edge(&g, id, other, t[0], &[]).unwrap();
        tx.commit().unwrap();
        let drawn = e.core().ids.current().next_element_id;
        let mut model: (BTreeSet<u64>, BTreeMap<u64, Value>) =
            ([l[0]].into(), [(key, Value::Int(0))].into());
        for i in 1..=50i64 {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let lab = l[i as usize % 3];
            match i % 5 {
                0 => {
                    tx.add_label(&g, id, lab).unwrap();
                    model.0.insert(lab);
                }
                1 => {
                    tx.remove_label(&g, id, lab).unwrap();
                    model.0.remove(&lab);
                }
                2 => {
                    tx.set_property(&g, id, key, Value::Int(i)).unwrap();
                    model.1.insert(key, Value::Int(i));
                }
                3 => {
                    tx.remove_property(&g, id, key).unwrap();
                    model.1.remove(&key);
                }
                _ => {
                    let props = [(key, Value::string(&format!("v{i}")))];
                    tx.set_node(&g, id, &[lab], &props).unwrap();
                    model = ([lab].into(), props.into_iter().collect());
                }
            }
            tx.commit().unwrap();
        }
        assert_eq!(e.core().ids.current().next_element_id, drawn);
        // An edge's properties change the same way.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let edge = tx.expand(&g, id, OUT, None).unwrap()[0].edge;
        tx.set_edge(&g, edge, &[(key, Value::Bool(true))]).unwrap();
        tx.commit().unwrap();
        assert_eq!(e.core().ids.current().next_element_id, drawn);
        let check = |e: &Engine| {
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            let g = graph(&mut tx);
            let (ls, ps) = tx
                .node(&g, id)
                .unwrap()
                .expect("the node keeps its identifier");
            assert_eq!(ls, model.0.iter().copied().collect::<Vec<_>>());
            assert_eq!(ps, model.1.clone().into_iter().collect::<Vec<_>>());
            let out = tx.expand(&g, id, OUT, None).unwrap();
            assert_eq!(out.len(), 1);
            let r = tx.edge(&g, out[0].edge).unwrap().unwrap();
            assert_eq!(r.props, [(key, Value::Bool(true))]);
            tx.abort();
            assert_stats_exact(e, "fifty changes");
            assert_intact(e);
        };
        check(&e);
        e.close().unwrap();
        let e = reopen(d.path());
        check(&e);
        drop(e);
        check(&reopen(d.path()));
    }

    /// Creates `n` elements in transactions of 500, nodes and edges in
    /// turn, and returns their identifiers.
    fn create_many(e: &Engine, n: usize) -> Vec<u64> {
        let ty = types(e, 1)[0];
        let mut ids = Vec::with_capacity(n);
        while ids.len() < n {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let mut prev = None;
            for _ in 0..250 {
                if ids.len() >= n {
                    break;
                }
                let node = tx.create_node(&g, &[], &[]).unwrap();
                ids.push(node);
                if let Some(p) = prev
                    && ids.len() < n
                {
                    ids.push(tx.create_edge(&g, p, node, ty, &[]).unwrap());
                }
                prev = Some(node);
            }
            tx.commit().unwrap();
        }
        ids
    }

    /// BR-TYPE-001, TS-07: after a node (and its edge) is deleted, the
    /// 10 000 elements created next, then 10 000 more after a crash (a drop
    /// without close) and reopen, never take an identifier that any
    /// element had, nor one another's; an identifier an aborted
    /// transaction drew is skipped.
    #[test]
    fn no_identifier_is_reused_after_a_delete_and_ten_thousand_creations_across_a_crash() {
        let (d, e) = db();
        let t = types(&e, 1);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let a = tx.create_node(&g, &[], &[]).unwrap();
        let b = tx.create_node(&g, &[], &[]).unwrap();
        let edge = tx.create_edge(&g, a, b, t[0], &[]).unwrap();
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        assert!(tx.delete_node(&g, a, true).unwrap());
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let aborted = tx.create_node(&g, &[], &[]).unwrap();
        tx.abort();
        let mut seen: HashSet<u64> = [a, b, edge, aborted].into();
        let ids = create_many(&e, 10_000);
        assert_eq!(ids.len(), 10_000);
        for id in ids {
            assert!(seen.insert(id), "identifier {id} reused");
        }
        drop(e);
        let e = reopen(d.path());
        assert!(e.core().ids.current().next_element_id > *seen.iter().max().unwrap());
        for id in create_many(&e, 10_000) {
            assert!(seen.insert(id), "after the crash: identifier {id} reused");
        }
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        assert!(tx.node(&g, a).unwrap().is_none());
        assert!(tx.edge(&g, edge).unwrap().is_none());
        tx.abort();
        assert_intact(&e);
    }

    /// FR-STO-003: 16 threads creating disjoint nodes and edges in their
    /// own serializable transactions all commit, and every write is
    /// present after a reopen; the statistics stay exact.
    #[test]
    fn sixteen_threads_creating_disjoint_nodes_and_edges_all_commit_and_survive_reopen() {
        const THREADS: usize = 16;
        const TXS: usize = 25;
        let (d, e) = db();
        let l = labels(&e, THREADS);
        let t = types(&e, 2);
        let made: Vec<Vec<(u64, u64, u64)>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..THREADS)
                .map(|i| {
                    let (e, l, t) = (&e, &l, &t);
                    s.spawn(move || {
                        let mut out = Vec::new();
                        let mut prev: Option<u64> = None;
                        for _ in 0..TXS {
                            let mut tx = e.begin(Isolation::Serializable).unwrap();
                            let g = graph(&mut tx);
                            let a = tx.create_node(&g, &[l[i]], &[]).unwrap();
                            let b = tx.create_node(&g, &[], &[]).unwrap();
                            let ab = tx.create_edge(&g, a, b, t[0], &[]).unwrap();
                            if let Some(p) = prev {
                                tx.create_edge(&g, p, a, t[1], &[]).unwrap();
                            }
                            tx.commit().expect("disjoint writers all commit");
                            out.push((a, b, ab));
                            prev = Some(b);
                        }
                        out
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_stats_exact(&e, "after the writers");
        e.close().unwrap();
        let e = reopen(d.path());
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        for (i, writes) in made.iter().enumerate() {
            for (k, &(a, b, ab)) in writes.iter().enumerate() {
                assert_eq!(tx.node(&g, a).unwrap().unwrap().0, [l[i]]);
                assert!(tx.node(&g, b).unwrap().is_some());
                let r = tx.edge(&g, ab).unwrap().unwrap();
                assert_eq!((r.src, r.dst, r.ty), (a, b, t[0]));
                let out = tx.expand(&g, a, OUT, Some(t[0])).unwrap();
                assert_eq!(out.len(), 1);
                assert_eq!((out[0].nbr, out[0].edge), (b, ab));
                let into = tx.expand(&g, a, IN, Some(t[1])).unwrap();
                assert_eq!(into.len(), usize::from(k > 0));
            }
        }
        tx.abort();
        let n = e.core().catalog.stats().counts().clone();
        for &label in &l {
            let key = crate::catalog::stats::StatKey::Nodes {
                graph: DEFAULT_GRAPH_ID,
                label,
            };
            assert_eq!(n.get(&key), Some(&(TXS as u64)));
        }
        assert_stats_exact(&e, "after the reopen");
        assert_intact(&e);
    }

    /// FR-CAT-004: a transaction that creates a graph and inserts nodes
    /// and edges into it, then aborts, leaves neither the graph nor its
    /// trees nor any statistic; committed, the same transaction leaves all
    /// of them.
    #[test]
    fn an_aborted_graph_creation_with_nodes_leaves_neither() {
        let (_d, e) = db();
        let l = labels(&e, 1);
        let t = types(&e, 1);
        let directory = |e: &Engine| {
            let mut n = 0;
            e.store()
                .scan_prefix(crate::storage::btree::DIRECTORY, b"", |_, _| {
                    n += 1;
                    true
                })
                .unwrap();
            n
        };
        let trees = directory(&e);
        for commit in [false, true] {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let id = tx
                .create_graph(DEFAULT_SCHEMA_ID, "g", OWNER, None)
                .unwrap();
            let g = tx.graph(id).unwrap();
            assert!(g.nodes.is_pending() && g.adj.is_pending());
            let a = tx.create_node(&g, &l, &[]).unwrap();
            let b = tx.create_node(&g, &l, &[]).unwrap();
            let ab = tx.create_edge(&g, a, b, t[0], &[]).unwrap();
            assert_eq!(tx.expand(&g, a, OUT, None).unwrap()[0].edge, ab);
            assert_eq!(tx.node(&g, a).unwrap().unwrap().0, l);
            if commit {
                tx.commit().unwrap();
            } else {
                tx.abort();
            }
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            let found = tx.lookup(DEFAULT_SCHEMA_ID, "g").unwrap();
            assert_eq!(found.is_some(), commit);
            if commit {
                let g = tx.graph(id).unwrap();
                assert!(!g.nodes.is_pending());
                assert_eq!(tx.edge(&g, ab).unwrap().unwrap().src, a);
                assert_eq!(directory(&e), trees + 3);
            } else {
                assert!(matches!(tx.graph(id), Err(Error::NotFound(_))));
                assert_eq!(directory(&e), trees, "an aborted graph left a tree");
            }
            tx.abort();
            let has_stats = e
                .core()
                .catalog
                .stats()
                .counts()
                .keys()
                .any(|k| k.graph() == id);
            assert_eq!(has_stats, commit);
            assert_stats_exact(&e, "graph creation");
            assert_intact(&e);
        }
    }

    type Edges = BTreeMap<u64, (u64, u64, u64)>;

    /// The adjacency entries the model `m` gives `node` in `dir`, of type
    /// `ty` or every type, in key order.
    fn model_expand(m: &Edges, node: u64, dir: u8, ty: Option<u64>) -> Vec<Adj> {
        let mut v: Vec<Adj> = m
            .iter()
            .filter_map(|(&edge, &(src, dst, t))| {
                let (at, nbr) = if dir == OUT { (src, dst) } else { (dst, src) };
                (at == node && ty.is_none_or(|x| x == t)).then_some(Adj {
                    node,
                    dir,
                    ty: t,
                    nbr,
                    edge,
                })
            })
            .collect();
        v.sort();
        v
    }

    /// Commits `rounds` random transactions on the nodes `nodes`: each
    /// creates edges of the types `t` (self-loops included) and deletes
    /// live ones, as the model `m` follows.
    fn churn(e: &Engine, rng: &mut StdRng, nodes: &[u64], t: &[u64], m: &mut Edges, rounds: usize) {
        for _ in 0..rounds {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            for _ in 0..rng.random_range(1..12) {
                if !m.is_empty() && rng.random_range(0..3) == 0 {
                    let k = *m.keys().nth(rng.random_range(0..m.len())).unwrap();
                    assert!(tx.delete_edge(&g, k).unwrap());
                    m.remove(&k);
                } else {
                    let (a, b) = (
                        nodes[rng.random_range(0..nodes.len())],
                        nodes[rng.random_range(0..nodes.len())],
                    );
                    let ty = t[rng.random_range(0..t.len())];
                    let id = tx.create_edge(&g, a, b, ty, &[]).unwrap();
                    m.insert(id, (a, b, ty));
                }
            }
            tx.commit().unwrap();
        }
    }

    /// TS-06 Expansion: an expansion at a snapshot returns exactly the live
    /// edges of that node, direction and type at that snapshot, while
    /// later commits create and delete edges; the transaction's own
    /// changes are merged in; a new snapshot sees the later state.
    #[test]
    fn an_expansion_at_a_snapshot_returns_exactly_the_live_edges_of_its_direction_and_type() {
        let seed = seed();
        let mut rng = StdRng::seed_from_u64(seed);
        let (_d, e) = db();
        let t = types(&e, 3);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let nodes: Vec<u64> = (0..20)
            .map(|_| tx.create_node(&g, &[], &[]).unwrap())
            .collect();
        tx.commit().unwrap();
        let mut m = Edges::new();
        churn(&e, &mut rng, &nodes, &t, &mut m, 30);
        for iso in [Isolation::Snapshot, Isolation::Serializable] {
            let mut s = e.begin(iso).unwrap();
            let at_s = m.clone();
            churn(&e, &mut rng, &nodes, &t, &mut m, 20);
            assert!(at_s != m, "precondition: later commits changed the edges");
            let g = graph(&mut s);
            let mut nonempty = 0;
            for &n in &nodes {
                for dir in [OUT, IN] {
                    for ty in [None, Some(t[0]), Some(t[1]), Some(t[2])] {
                        let got = s.expand(&g, n, dir, ty).unwrap();
                        assert_eq!(got, model_expand(&at_s, n, dir, ty), "seed {seed}");
                        nonempty += usize::from(!got.is_empty());
                    }
                }
            }
            assert!(nonempty > 50, "{nonempty} non-empty expansions");
            // Its own changes.
            let mut own = at_s.clone();
            let x = s.create_edge(&g, nodes[0], nodes[1], t[0], &[]).unwrap();
            own.insert(x, (nodes[0], nodes[1], t[0]));
            // An edge live at the snapshot and still live now: deleting it
            // meets no later change.
            let y = *at_s.keys().find(|k| m.contains_key(k)).unwrap();
            assert!(s.delete_edge(&g, y).unwrap());
            own.remove(&y);
            for &n in &nodes[..4] {
                for dir in [OUT, IN] {
                    assert_eq!(
                        s.expand(&g, n, dir, None).unwrap(),
                        model_expand(&own, n, dir, None)
                    );
                }
            }
            s.abort();
            let mut now = e.begin(Isolation::Snapshot).unwrap();
            let g = graph(&mut now);
            for &n in &nodes {
                for dir in [OUT, IN] {
                    assert_eq!(
                        now.expand(&g, n, dir, None).unwrap(),
                        model_expand(&m, n, dir, None)
                    );
                }
            }
            now.abort();
        }
        assert_stats_exact(&e, "churn");
        assert_intact(&e);
    }

    /// TS-15 node existence, rmp #32: an edge created towards a node that a
    /// concurrent transaction deletes: the later of the two to lock waits
    /// for the earlier and then fails with 25N11, in either order, at both
    /// isolation levels, with and without detach; the integrity check then
    /// finds no edge to a missing node and no dangling adjacency entry.
    /// Without concurrency, a node with an edge is deleted only with
    /// detach (G1001 otherwise).
    #[test]
    fn an_edge_to_a_node_deleted_concurrently_fails_or_waits_and_leaves_no_dangling_entry() {
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            for deleter_first in [true, false] {
                for detach in [false, true] {
                    let what = format!("{iso:?} deleter first {deleter_first} detach {detach}");
                    let (_d, e) = db();
                    let t = types(&e, 1)[0];
                    let mut tx = e.begin(Isolation::Serializable).unwrap();
                    let g = graph(&mut tx);
                    let a = tx.create_node(&g, &[], &[]).unwrap();
                    let b = tx.create_node(&g, &[], &[]).unwrap();
                    tx.commit().unwrap();
                    let mut creator = e.begin(iso).unwrap();
                    let mut deleter = e.begin(iso).unwrap();
                    let r = std::thread::scope(|s| {
                        if deleter_first {
                            let g = graph(&mut deleter);
                            assert!(deleter.delete_node(&g, b, detach).unwrap());
                            let h = s.spawn(move || {
                                let g = graph(&mut creator);
                                creator
                                    .create_edge(&g, a, b, t, &[])
                                    .and_then(|_| creator.commit().map(drop))
                            });
                            until_waiting(&e, 1);
                            deleter.commit().unwrap();
                            h.join().unwrap()
                        } else {
                            let g = graph(&mut creator);
                            creator.create_edge(&g, a, b, t, &[]).unwrap();
                            let h = s.spawn(move || {
                                let g = graph(&mut deleter);
                                deleter
                                    .delete_node(&g, b, detach)
                                    .and_then(|_| deleter.commit().map(drop))
                            });
                            until_waiting(&e, 1);
                            creator.commit().unwrap();
                            h.join().unwrap()
                        }
                    });
                    assert!(matches!(r, Err(Error::Serialization)), "{what}: {r:?}");
                    let mut tx = e.begin(Isolation::Snapshot).unwrap();
                    let g = graph(&mut tx);
                    let node = tx.node(&g, b).unwrap();
                    let edges = tx.expand(&g, a, OUT, None).unwrap();
                    assert_eq!(node.is_some(), !deleter_first, "{what}");
                    assert_eq!(edges.len(), usize::from(!deleter_first), "{what}");
                    tx.abort();
                    assert_intact(&e);
                    assert_stats_exact(&e, &what);
                    if !deleter_first {
                        // No concurrency: G1001 without detach, else the
                        // edge goes with the node.
                        let mut tx = e.begin(iso).unwrap();
                        let g = graph(&mut tx);
                        let err = tx.delete_node(&g, b, false).unwrap_err();
                        assert!(matches!(err, Error::EdgesStillExist), "{what}: {err:?}");
                        assert!(tx.delete_node(&g, b, true).unwrap());
                        tx.commit().unwrap();
                        let mut tx = e.begin(Isolation::Snapshot).unwrap();
                        let g = graph(&mut tx);
                        assert!(tx.expand(&g, a, OUT, None).unwrap().is_empty());
                        tx.abort();
                        assert_intact(&e);
                    }
                }
            }
        }
    }

    /// TS-15, TS-17 Label changes: a label added to a node while a
    /// concurrent transaction creates an edge at it fails neither, at both
    /// isolation levels and in both commit orders (the edge creator holds
    /// only existence S and records no read of the node), and the edge
    /// statistics count the edge with the node's label exactly once.
    #[test]
    fn a_label_change_and_a_concurrent_edge_at_its_node_both_commit_with_exact_statistics() {
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            for labeller_first in [true, false] {
                let (_d, e) = db();
                let l = labels(&e, 1);
                let t = types(&e, 1);
                let mut tx = e.begin(Isolation::Serializable).unwrap();
                let g = graph(&mut tx);
                let a = tx.create_node(&g, &[], &[]).unwrap();
                let hub = tx.create_node(&g, &[], &[]).unwrap();
                tx.create_edge(&g, a, hub, t[0], &[]).unwrap();
                tx.commit().unwrap();
                let mut labeller = e.begin(iso).unwrap();
                let mut creator = e.begin(iso).unwrap();
                let g1 = graph(&mut labeller);
                labeller.add_label(&g1, hub, l[0]).unwrap();
                let g2 = graph(&mut creator);
                creator.create_edge(&g2, a, hub, t[0], &[]).unwrap();
                if labeller_first {
                    labeller.commit().unwrap();
                    creator.commit().unwrap();
                } else {
                    creator.commit().unwrap();
                    labeller.commit().unwrap();
                }
                let key = crate::catalog::stats::StatKey::Edges {
                    graph: DEFAULT_GRAPH_ID,
                    label: l[0],
                    ty: t[0],
                    dir: IN,
                };
                assert_eq!(e.core().catalog.stats().counts().get(&key), Some(&2));
                assert_stats_exact(&e, &format!("{iso:?} labeller first {labeller_first}"));
                assert_intact(&e);
            }
        }
    }

    /// TS-12, rmp #32: the integrity check reports an adjacency entry
    /// without an edge, an edge with a missing adjacency entry and an edge
    /// whose endpoint is not a node, each injected below the graph API
    /// (each delete removed by a garbage-collection pass, since the check
    /// reads delete-stamped entries too), and nothing for an intact graph.
    #[test]
    fn the_integrity_check_reports_an_adjacency_entry_without_edge_and_an_edge_without_adjacency() {
        let (_d, e) = db();
        pause_gc(&e);
        let ty = types(&e, 1)[0];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let a = tx.create_node(&g, &[], &[]).unwrap();
        let b = tx.create_node(&g, &[], &[]).unwrap();
        let ab = tx.create_edge(&g, a, b, ty, &[]).unwrap();
        tx.commit().unwrap();
        assert_intact(&e);
        let stray = adj_key(Adj {
            node: a,
            dir: OUT,
            ty,
            nbr: b,
            edge: 777_777,
        });
        let raw = |f: &dyn Fn(&mut Transaction<'_>, &Graph)| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            f(&mut tx, &g);
            tx.commit().unwrap();
            crate::tx::gc::pass(e.core()).unwrap();
        };
        let reported = |needle: &str| {
            let p = e.check_graphs().unwrap();
            assert!(p.iter().any(|x| x.contains(needle)), "{needle}: {p:?}");
        };
        raw(&|tx, g| tx.put(g.adj, &stray, &[]).unwrap());
        reported("belongs to no edge");
        raw(&|tx, g| assert!(tx.delete(g.adj, &stray).unwrap()));
        assert_intact(&e);
        let in_entry = adj_key(Adj {
            node: b,
            dir: IN,
            ty,
            nbr: a,
            edge: ab,
        });
        raw(&|tx, g| assert!(tx.delete(g.adj, &in_entry).unwrap()));
        reported(&format!("edge {ab} has no adjacency entry"));
        raw(&|tx, g| tx.put(g.adj, &in_entry, &[]).unwrap());
        assert_intact(&e);
        raw(&|tx, g| assert!(tx.delete(g.nodes, &element_key(b)).unwrap()));
        reported("which is not a node");
    }

    /// One random transaction of the statistics workload on graph `id`:
    /// nodes with random labels, edges of random types, labels added to
    /// and removed from nodes that have edges, property changes, edge
    /// deletions and detached node deletions; a third of them abort.
    /// Returns how many label changes hit a node with an edge.
    fn workload_tx(e: &Engine, rng: &mut StdRng, id: u64, l: &[u64], t: &[u64]) -> usize {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx.graph(id).unwrap();
        let mut nodes = Vec::new();
        tx.scan_prefix(g.nodes, b"", |k, _| {
            nodes.push(u64::from_be_bytes(k.try_into().unwrap()));
            true
        })
        .unwrap();
        let mut label_changes = 0;
        for _ in 0..rng.random_range(1..10) {
            let pick = |rng: &mut StdRng, nodes: &[u64]| nodes[rng.random_range(0..nodes.len())];
            match rng.random_range(0..10) {
                0..=2 => {
                    let ls: Vec<u64> = l
                        .iter()
                        .copied()
                        .filter(|_| rng.random_range(0..2) == 0)
                        .collect();
                    nodes.push(tx.create_node(&g, &ls, &[]).unwrap());
                }
                3..=5 if !nodes.is_empty() => {
                    let (a, b) = (pick(rng, &nodes), pick(rng, &nodes));
                    tx.create_edge(&g, a, b, t[rng.random_range(0..t.len())], &[])
                        .unwrap();
                }
                6 if !nodes.is_empty() => {
                    let n = pick(rng, &nodes);
                    if !tx.expand(&g, n, OUT, None).unwrap().is_empty()
                        || !tx.expand(&g, n, IN, None).unwrap().is_empty()
                    {
                        label_changes += 1;
                    }
                    let lab = l[rng.random_range(0..l.len())];
                    if rng.random_range(0..2) == 0 {
                        tx.add_label(&g, n, lab).unwrap();
                    } else {
                        tx.remove_label(&g, n, lab).unwrap();
                    }
                }
                7 if !nodes.is_empty() => {
                    let n = pick(rng, &nodes);
                    if let Some(a) = tx.expand(&g, n, OUT, None).unwrap().first() {
                        assert!(tx.delete_edge(&g, a.edge).unwrap());
                    }
                }
                8 if nodes.len() > 4 => {
                    let n = pick(rng, &nodes);
                    assert!(tx.delete_node(&g, n, true).unwrap());
                    nodes.retain(|x| *x != n);
                }
                _ if !nodes.is_empty() => {
                    let n = pick(rng, &nodes);
                    tx.set_property(&g, n, 0, Value::Int(rng.random())).unwrap();
                }
                _ => {}
            }
        }
        if rng.random_range(0..3) == 0 {
            tx.abort();
            0
        } else {
            tx.commit().unwrap();
            label_changes
        }
    }

    /// TS-17 Statistics, TS-11, TS-13 step 7: the statistics equal a full
    /// recount after a seeded workload on two graphs (one then dropped),
    /// survive a clean reopen without a recount, and after a drop without
    /// close, whose statistics tree holds older counts, are recounted
    /// before open returns and equal a recount again.
    #[test]
    fn statistics_equal_a_recount_after_a_seeded_workload_a_clean_reopen_and_a_crash_reopen() {
        let seed = seed();
        let mut rng = StdRng::seed_from_u64(seed);
        let (d, e) = db();
        let l = labels(&e, 4);
        let t = types(&e, 3);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let other = tx
            .create_graph(DEFAULT_SCHEMA_ID, "other", OWNER, None)
            .unwrap();
        tx.commit().unwrap();
        let mut label_changes = 0;
        for i in 0..150 {
            let id = if i % 3 == 0 { other } else { DEFAULT_GRAPH_ID };
            label_changes += workload_tx(&e, &mut rng, id, &l, &t);
        }
        assert!(
            label_changes > 10,
            "{label_changes} label changes at nodes with edges"
        );
        let counts = e.core().catalog.stats().counts().clone();
        assert!(
            counts
                .keys()
                .any(|k| matches!(k, crate::catalog::stats::StatKey::Edges { .. })),
            "precondition: edge statistics"
        );
        assert!(counts.keys().any(|k| k.graph() == other));
        assert_stats_exact(&e, &format!("seed {seed}: workload"));
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "other").unwrap());
        tx.commit().unwrap();
        assert!(
            !e.core()
                .catalog
                .stats()
                .counts()
                .keys()
                .any(|k| k.graph() == other)
        );
        assert_stats_exact(&e, &format!("seed {seed}: drop"));
        // A clean reopen loads them: the tree holds the last commit.
        let before = e.core().catalog.stats().counts().clone();
        e.close().unwrap();
        let e = reopen(d.path());
        {
            let st = e.core().catalog.stats();
            assert_eq!(*st.counts(), before);
            assert_eq!(
                st.saved_ts(),
                e.control().lock().unwrap().current().commit_ts,
                "seed {seed}: a clean reopen needs no recount"
            );
        }
        // Saved by a checkpoint, then changed, then a crash.
        for _ in 0..20 {
            workload_tx(&e, &mut rng, DEFAULT_GRAPH_ID, &l, &t);
        }
        e.checkpoint().unwrap();
        let saved = e.core().catalog.stats().counts().clone();
        let mut changed = false;
        for _ in 0..40 {
            workload_tx(&e, &mut rng, DEFAULT_GRAPH_ID, &l, &t);
            changed |= *e.core().catalog.stats().counts() != saved;
        }
        assert!(changed, "precondition: the counts moved after the save");
        let final_counts = e.core().catalog.stats().counts().clone();
        drop(e);
        let e = reopen(d.path());
        assert_eq!(
            crate::catalog::stats::load(e.core())
                .unwrap()
                .counts()
                .clone(),
            saved,
            "precondition: the tree holds the older counts"
        );
        assert_eq!(
            *e.core().catalog.stats().counts(),
            final_counts,
            "seed {seed}"
        );
        assert_stats_exact(&e, &format!("seed {seed}: crash reopen"));
        assert_intact(&e);
    }

    /// TS-11 Statistics first, rmp #32 audit B3: a checkpoint's
    /// statistics batch lies below the redo point it records. The durable
    /// WAL (read through the wrapped storage, which a `FaultStorage`
    /// changes only at `sync_data`) holds the batch's group, which writes
    /// the statistics tree, and no group at or after `CONTROL`'s redo
    /// point; the statistics tree then holds the counts and the last
    /// commit timestamp, and a checkpoint with nothing changed writes no
    /// WAL.
    #[test]
    fn a_checkpoints_statistics_batch_lies_below_its_redo_point() {
        use crate::storage::fault::{Call, FaultStorage};
        use crate::storage::wal::parse_segment_name;
        use crate::storage::wal::tests::DurableScan;
        let dir = crate::test_support::graph_dir();
        let fault = crate::sync::Arc::new(FaultStorage::new(Box::new(crate::storage::FsStorage)));
        fault.skip_device_sync();
        let e = Engine::create_tuned(
            dir.path(),
            fault.clone(),
            crate::tx::tests::CAP,
            OWNER,
            crate::sync::Arc::new(crate::obs::recording::RecordingSink::default()),
            crate::tx::tests::tuning(),
        )
        .unwrap();
        pause_gc(&e);
        let l = labels(&e, 1);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        tx.create_node(&g, &l, &[]).unwrap();
        let ts = tx.commit().unwrap().unwrap();
        let from = e.wal().durable_lsn();
        e.checkpoint().unwrap();
        let redo = e.control().lock().unwrap().current().redo_point;
        let stats_root = e
            .store()
            .tree(crate::catalog::STATS_TREE)
            .unwrap()
            .unwrap()
            .root();
        let mut scan = DurableScan::new(dir.path(), 1, from);
        scan.advance();
        let batch = scan
            .groups
            .iter()
            .find(|g| g.pages.contains(&stats_root))
            .expect("the statistics batch's group is durable");
        assert!(batch.start < redo, "{} {redo}", batch.start);
        let mut after = DurableScan::new(dir.path(), 1, redo);
        after.advance();
        assert!(
            after.groups.is_empty(),
            "a durable group at or after the redo point {redo}: {:?}",
            after.groups.iter().map(|g| g.start).collect::<Vec<_>>()
        );
        let st = crate::catalog::stats::load(e.core()).unwrap();
        assert_eq!(st.saved_ts(), ts);
        assert_eq!(st.counts(), e.core().catalog.stats().counts());
        assert_eq!(st.counts().len(), 1);
        // Nothing changed: the next checkpoint writes no WAL.
        let is_wal = |p: &std::path::Path| p.file_name().and_then(parse_segment_name).is_some();
        let from = fault.calls().len();
        e.checkpoint().unwrap();
        assert!(
            !fault.calls()[from..]
                .iter()
                .any(|c| matches!(c, Call::WriteAt { path, .. } if is_wal(path)))
        );
    }

    /// TS-12, rmp #32 audit B1: with no transaction running, the check
    /// compares the delete stamps of every edge record and its two
    /// adjacency entries. An edge deleted through the API is not reported;
    /// a record deleted in one commit and its entries in a later one, and
    /// one entry deleted alone, are, each injected below the graph API.
    #[test]
    fn the_integrity_check_compares_every_delete_stamp_of_an_edge_and_its_entries() {
        let (_d, e) = db();
        pause_gc(&e);
        let ty = types(&e, 1)[0];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let a = tx.create_node(&g, &[], &[]).unwrap();
        let b = tx.create_node(&g, &[], &[]).unwrap();
        let [x, y, z] = [0; 3].map(|_| tx.create_edge(&g, a, b, ty, &[]).unwrap());
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        assert!(tx.delete_edge(&g, z).unwrap());
        tx.commit().unwrap();
        assert_intact(&e);
        let entry = |edge, dir| {
            let (node, nbr) = if dir == OUT { (a, b) } else { (b, a) };
            adj_key(Adj {
                node,
                dir,
                ty,
                nbr,
                edge,
            })
        };
        let raw = |keys: &[(bool, Vec<u8>)]| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            for (edge_tree, k) in keys {
                let t = if *edge_tree { g.edges } else { g.adj };
                assert!(tx.delete(t, k).unwrap());
            }
            tx.commit().unwrap().unwrap()
        };
        let reported = |needle: &str| {
            let p = e.check_graphs().unwrap();
            assert!(p.iter().any(|m| m.contains(needle)), "{needle}: {p:?}");
        };
        let t1 = raw(&[(true, element_key(x).to_vec())]);
        reported(&format!("edge {x} deleted at {t1}, its entry"));
        let t2 = raw(&[
            (false, entry(x, OUT).to_vec()),
            (false, entry(x, IN).to_vec()),
        ]);
        assert!(t2 > t1);
        reported(&format!(
            "edge {x} deleted at {t1}, its entry Adj {{ node: {a}, dir: 0"
        ));
        reported(&format!("at {t2}"));
        let t3 = raw(&[(false, entry(y, IN).to_vec())]);
        reported(&format!(
            "edge {y} deleted at 0, its entry Adj {{ node: {b}, dir: 1"
        ));
        reported(&format!("at {t3}"));
    }

    /// TS-12, rmp #32 audit N1: the check compares an edge with its
    /// endpoints by deletion time. A node record delete-stamped at t1 below
    /// the graph API while its edge is live is reported (a live edge needs
    /// a live endpoint); once the edge and its entries are deleted at
    /// t2 > t1, it is reported again (a deleted edge's endpoint was not
    /// deleted before it). The other endpoint is never reported.
    #[test]
    fn the_integrity_check_reports_an_edge_whose_endpoint_was_deleted_first() {
        let (_d, e) = db();
        pause_gc(&e);
        let ty = types(&e, 1)[0];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let a = tx.create_node(&g, &[], &[]).unwrap();
        let b = tx.create_node(&g, &[], &[]).unwrap();
        let x = tx.create_edge(&g, a, b, ty, &[]).unwrap();
        tx.commit().unwrap();
        assert_intact(&e);
        let raw = |keys: &[(Tree, Vec<u8>)]| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            for (t, k) in keys {
                assert!(tx.delete(*t, k).unwrap());
            }
            tx.commit().unwrap().unwrap()
        };
        let endpoint = |at: u64, n: u64| {
            let p = e.check_graphs().unwrap();
            let msg = |n| format!("edge {x} (deleted at {at}) has endpoint {n},");
            assert!(p.iter().any(|m| m.contains(&msg(n))), "{n}: {p:?}");
            assert!(!p.iter().any(|m| m.contains(&msg(b))), "{p:?}");
        };
        let t1 = raw(&[(g.nodes, element_key(a).to_vec())]);
        endpoint(0, a);
        let entry = |dir| {
            let (node, nbr) = if dir == OUT { (a, b) } else { (b, a) };
            adj_key(Adj {
                node,
                dir,
                ty,
                nbr,
                edge: x,
            })
            .to_vec()
        };
        let t2 = raw(&[
            (g.edges, element_key(x).to_vec()),
            (g.adj, entry(OUT)),
            (g.adj, entry(IN)),
        ]);
        assert!(t2 > t1);
        endpoint(t2, a);
    }

    /// TS-12, rmp #32 audit B1: the check runs under the commit latch, so
    /// no commit and no garbage-collection pass changes a tree while it
    /// reads them: while one thread creates and deletes edges, and the
    /// collector removes the deleted ones after each commit, every check
    /// reports nothing.
    #[test]
    fn the_integrity_check_reports_nothing_while_edges_change_and_are_collected() {
        use crate::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        let (_d, e) = db();
        let ty = types(&e, 1)[0];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let nodes: Vec<u64> = (0..4)
            .map(|_| tx.create_node(&g, &[], &[]).unwrap())
            .collect();
        tx.commit().unwrap();
        let stop = AtomicBool::new(false);
        // Commits by the writer, each followed by a collector pass.
        let commits = AtomicU64::new(0);
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut i = 0usize;
                while !stop.load(Ordering::SeqCst) {
                    let mut tx = e.begin(Isolation::Serializable).unwrap();
                    let g = graph(&mut tx);
                    let (a, b) = (nodes[i % 4], nodes[(i / 4) % 4]);
                    let made: Vec<u64> = (0..4)
                        .map(|_| tx.create_edge(&g, a, b, ty, &[]).unwrap())
                        .collect();
                    tx.commit().unwrap();
                    let mut tx = e.begin(Isolation::Serializable).unwrap();
                    let g = graph(&mut tx);
                    for x in made {
                        assert!(tx.delete_edge(&g, x).unwrap());
                    }
                    tx.commit().unwrap();
                    commits.fetch_add(2, Ordering::SeqCst);
                    i += 1;
                }
            });
            let mut checks = 0;
            while commits.load(Ordering::SeqCst) < 40 || checks < 20 {
                let p = e.check_graphs().unwrap();
                if !p.is_empty() {
                    stop.store(true, Ordering::SeqCst);
                    panic!("check {checks}: {p:?}");
                }
                checks += 1;
            }
            stop.store(true, Ordering::SeqCst);
        });
    }

    /// rmp #31 audit B7: a commit that takes a statistic below zero (the
    /// statistics made wrong below the API) does not panic under the
    /// commit latch, in this debug build too, and commits: the count is
    /// clamped to zero. The integrity check reports the drift while it
    /// lasts.
    #[test]
    fn a_statistic_taken_below_zero_is_clamped_and_the_drift_reported() {
        let (_d, e) = db();
        let l = labels(&e, 1);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        let n = tx.create_node(&g, &l, &[]).unwrap();
        tx.commit().unwrap();
        e.core().catalog.stats().replace(BTreeMap::new());
        let p = e.check_graphs().unwrap();
        assert!(
            p.iter().any(|m| m.contains("is 0, a recount gives 1")),
            "{p:?}"
        );
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        assert!(tx.delete_node(&g, n, false).unwrap());
        tx.commit().expect("a count below zero fails nothing");
        assert!(!e.core().health.is_unusable());
        assert!(e.core().catalog.stats().counts().is_empty());
        assert_stats_exact(&e, "clamped");
        assert_intact(&e);
    }

    /// rmp #32 audit B8, TS-17 Tokens: a node's labels must be label
    /// tokens and an edge's type an edge-type token; any other identifier
    /// (another kind's token, or none) is refused with InvalidArgument,
    /// which writes nothing and leaves the transaction usable.
    #[test]
    fn graph_writes_refuse_label_and_edge_type_identifiers_that_are_not_such_tokens() {
        let (_d, e) = db();
        let l = labels(&e, 1)[0];
        let t = types(&e, 1)[0];
        let none = l.max(t) + 1000;
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = graph(&mut tx);
        for bad in [t, none] {
            let r = tx.create_node(&g, &[l, bad], &[]);
            assert!(matches!(r, Err(Error::InvalidArgument(_))), "{bad}: {r:?}");
        }
        let n = tx.create_node(&g, &[l], &[]).unwrap();
        for bad in [l, none] {
            let r = tx.create_edge(&g, n, n, bad, &[]);
            assert!(matches!(r, Err(Error::InvalidArgument(_))), "{bad}: {r:?}");
            let r = tx.add_label(&g, n, if bad == l { t } else { bad });
            assert!(matches!(r, Err(Error::InvalidArgument(_))), "{bad}: {r:?}");
            let r = tx.set_node(&g, n, &[bad, t], &[]);
            assert!(matches!(r, Err(Error::InvalidArgument(_))), "{bad}: {r:?}");
        }
        tx.create_edge(&g, n, n, t, &[]).unwrap();
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = graph(&mut tx);
        let mut nodes = 0;
        tx.scan_prefix(g.nodes, b"", |_, _| {
            nodes += 1;
            true
        })
        .unwrap();
        assert_eq!(nodes, 1);
        assert_eq!(tx.node(&g, n).unwrap().unwrap().0, [l]);
        assert_eq!(tx.expand(&g, n, OUT, None).unwrap().len(), 1);
        tx.abort();
        assert_intact(&e);
    }

    /// rmp #32 audit B8 and N2, TS-08: the cross-tree check holds one
    /// entry at a time and reads pages through the buffer pool, within the
    /// memory cap. Under a 4 MiB cap, with every byte the pool gives up
    /// taken, so the pool keeps only its floor of 5 pages (1 % of the cap),
    /// the check covers a graph whose trees span more than 3 times as many
    /// pages, reserves nothing, and reports exactly the problem injected;
    /// with the memory back it reports the same. Its peak heap use on this
    /// thread, which the accountant does not see, grows by at most 10 %
    /// when the graph grows 4 times.
    #[test]
    fn the_graph_check_stays_within_the_memory_cap() {
        let (_d, e) = crate::tx::tests::db_with_cap(4 << 20);
        pause_gc(&e);
        let ty = types(&e, 1)[0];
        let mut nodes = Vec::new();
        // Grows the graph to `n` nodes and `8 * n` edges.
        let mut grow = |n: usize| {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = graph(&mut tx);
            let from = nodes.len();
            nodes.extend((from..n).map(|_| tx.create_node(&g, &[], &[]).unwrap()));
            for i in from * 8..n * 8 {
                tx.create_edge(&g, nodes[i % n], nodes[(i * 7 + 1) % n], ty, &[])
                    .unwrap();
            }
            tx.commit().unwrap();
            g
        };
        // The peak heap bytes of one check, with the pool at its floor.
        let peak = || {
            let mut hog = Vec::new();
            while let Ok(r) = e.pool().reserve(8192) {
                hog.push(r);
            }
            let held = e.core().accountant.used();
            let mut p = Vec::new();
            let info = allocation_counter::measure(|| p = e.check_graphs().unwrap());
            assert_eq!(
                e.core().accountant.used(),
                held,
                "the check reserved memory"
            );
            (info.bytes_max, p)
        };
        grow(125);
        let (small, _) = peak();
        let g = grow(500);
        let stray = adj_key(Adj {
            node: nodes[0],
            dir: OUT,
            ty,
            nbr: nodes[1],
            edge: 999_999,
        });
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.put(g.adj, &stray, &[]).unwrap();
        tx.commit().unwrap();
        let report = e.store().check().unwrap();
        let used: u64 = report
            .trees
            .iter()
            .filter(|t| [g.nodes.id(), g.edges.id(), g.adj.id()].contains(&t.id))
            .map(|t| t.leaves + t.inner)
            .sum();
        let floor = (4u64 << 20) / 1000 * 10 / 8192;
        assert!(
            used > 3 * floor,
            "precondition: {used} pages, floor {floor}"
        );
        let (large, p) = peak();
        // Measured: equal peaks (120 009 bytes); an uncharged `Vec` of every
        // adjacency key raises them to 206 547 and 552 304.
        assert!(
            large <= small + small / 10,
            "peak heap {small} bytes at 1 000 edges, {large} at 4 000"
        );
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("belongs to no edge"), "{p:?}");
        assert_eq!(e.check_graphs().unwrap(), p);
    }
}

/// `shuttle` schedules of graph writers and checkpoints (TS-30
/// Concurrency): two threads create nodes and edges in the default graph
/// and add labels to nodes that have edges, a third runs checkpoints;
/// under 300 random schedules the statistics equal a recount, survive a
/// clean reopen, and the integrity check passes.
#[cfg(all(shuttle, not(loom)))]
#[test]
fn shuttle_graph_writers_and_checkpoints_keep_the_statistics_exact() {
    use super::{Engine, recount};
    use crate::catalog::{DEFAULT_GRAPH_ID, TokenKind};
    use crate::tx::Isolation;
    use crate::tx::tests::{db, reopen};
    ::shuttle::check_random(
        || {
            let (d, e) = db();
            let l = e.token(TokenKind::Label, "L").unwrap();
            let ty = e.token(TokenKind::EdgeType, "T").unwrap();
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
            let hub = tx.create_node(&g, &[], &[]).unwrap();
            tx.commit().unwrap();
            ::shuttle::thread::scope(|s| {
                for _ in 0..2 {
                    let e = &e;
                    s.spawn(move || {
                        for _ in 0..2 {
                            let mut tx = e.begin(Isolation::Serializable).unwrap();
                            let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
                            let n = tx.create_node(&g, &[l], &[]).unwrap();
                            tx.create_edge(&g, n, hub, ty, &[]).unwrap();
                            tx.commit().unwrap();
                        }
                    });
                }
                s.spawn(|| {
                    // The hub gains a label while edges reach it: the
                    // committer counts its edges inside the serial section.
                    let mut tx = e.begin(Isolation::Snapshot).unwrap();
                    let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
                    tx.add_label(&g, hub, l).unwrap();
                    match tx.commit() {
                        Ok(_) | Err(crate::storage::error::Error::Serialization) => {}
                        Err(other) => panic!("{other:?}"),
                    }
                });
                s.spawn(|| {
                    e.checkpoint().unwrap();
                });
            });
            let exact = |e: &Engine| {
                let got = e.core().catalog.stats().counts().clone();
                assert_eq!(got, recount(e.core()).unwrap());
                let p = e.check_integrity().unwrap();
                assert!(p.is_empty(), "{p:?}");
            };
            exact(&e);
            e.close().unwrap();
            let e = reopen(d.path());
            exact(&e);
            e.close().unwrap();
        },
        300,
    );
}
