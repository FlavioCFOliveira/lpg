//! Tests of the catalog (rmp #31): objects, owners, the default schema and
//! graph, names under concurrency, tokens, dropped graphs, counters.

#[cfg(not(any(loom, shuttle)))]
mod catalog {
    use super::super::*;
    use crate::storage::Storage;
    use crate::storage::error::Error;
    use crate::sync::Arc;
    use crate::test_support::{OWNER, graph_dir, mid_batch, no_flush};
    use crate::tx::Isolation;
    use crate::tx::tests::{CAP, db, pause_gc, reopen, tuning, until_waiting};

    /// Every object the snapshot sees: path, kind, owner.
    fn listing(e: &Engine) -> Vec<(Vec<String>, Kind, String)> {
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let mut out = Vec::new();
        let mut stack = vec![ROOT];
        while let Some(p) = stack.pop() {
            for o in tx.children(p).unwrap() {
                stack.push(o.id);
                out.push((tx.path(o.id).unwrap().unwrap(), o.kind, o.owner));
            }
        }
        tx.abort();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn assert_intact(e: &Engine) {
        let p = e.check_integrity().unwrap();
        assert!(p.is_empty(), "{p:?}");
    }

    /// FR-DB-005, TS-17 Creation: a new database holds the default schema
    /// and the default graph, owned by the principal create received, with
    /// the default graph's trees, also after a clean reopen and after a
    /// drop without close. `create` requires a non-empty owner and leaves
    /// no database when refused.
    #[test]
    fn a_new_database_holds_the_default_schema_and_graph_also_after_reopen() {
        let d = graph_dir();
        let e = Engine::create_tuned(
            d.path(),
            no_flush(),
            CAP,
            "p1",
            Arc::new(crate::obs::recording::RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        let expected = vec![
            (
                vec![DEFAULT_SCHEMA.to_owned()],
                Kind::Schema,
                "p1".to_owned(),
            ),
            (
                vec![DEFAULT_SCHEMA.to_owned(), DEFAULT_GRAPH.to_owned()],
                Kind::Graph,
                "p1".to_owned(),
            ),
        ];
        let check = |e: &Engine| {
            assert_eq!(listing(e), expected);
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let g = tx
                .resolve(&[DEFAULT_SCHEMA, DEFAULT_GRAPH])
                .unwrap()
                .unwrap();
            assert_eq!((g.id, g.parent), (DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID));
            let graph = tx.graph(g.id).unwrap();
            let n = tx.create_node(&graph, &[], &[]).unwrap();
            assert!(tx.node(&graph, n).unwrap().is_some());
            tx.abort();
            assert_eq!(e.core().catalog.objects().len(), 2);
            assert_intact(e);
        };
        check(&e);
        e.close().unwrap();
        let e = reopen(d.path());
        check(&e);
        drop(e);
        let e = reopen(d.path());
        check(&e);
        // The first object a transaction creates takes the identifier
        // after the two (TS-05 Create).
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert_eq!(tx.create_schema(ROOT, "s", "p1").unwrap(), FIRST_OBJECT_ID);
        tx.abort();
        // The owner is required: an empty one is refused, with no database.
        let d2 = graph_dir();
        let r = Engine::create(
            d2.path(),
            no_flush(),
            CAP,
            "",
            Arc::new(crate::obs::recording::RecordingSink::default()),
        );
        assert!(matches!(r, Err(Error::InvalidArgument(_))), "{:?}", r.err());
        assert!(
            !crate::storage::FsStorage
                .list(d2.path())
                .unwrap()
                .iter()
                .any(|n| n == crate::storage::engine::CONTROL)
        );
    }

    /// Creates, in one transaction, a directory `d`, schemas `s1` (in the
    /// root) and `s2` (in `d`), and graphs `g1`, `g2` (in `s1`) and `g3`
    /// (in `s2`); returns their identifiers and paths.
    fn tree_of_objects(e: &Engine) -> Vec<(u64, Vec<&'static str>)> {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let d = tx.create_directory(ROOT, "d", OWNER).unwrap();
        let s1 = tx.create_schema(ROOT, "s1", OWNER).unwrap();
        let s2 = tx.create_schema(d, "s2", OWNER).unwrap();
        let g1 = tx.create_graph(s1, "g1", OWNER, None).unwrap();
        let g2 = tx.create_graph(s1, "g2", OWNER, None).unwrap();
        let g3 = tx.create_graph(s2, "g3", OWNER, None).unwrap();
        tx.commit().unwrap();
        vec![
            (d, vec!["d"]),
            (s1, vec!["s1"]),
            (s2, vec!["d", "s2"]),
            (g1, vec!["s1", "g1"]),
            (g2, vec!["s1", "g2"]),
            (g3, vec!["d", "s2", "g3"]),
        ]
    }

    /// FR-DB-001, FR-CAT-001: two schemas and three graphs created in a
    /// database belong to its catalog, their reverse entries resolve to
    /// their paths, and each path resolves back to its object. Objects of
    /// a kind their parent cannot hold are refused.
    #[test]
    fn two_schemas_and_three_graphs_belong_to_the_catalog_and_resolve_to_their_paths() {
        let (_d, e) = db();
        let made = tree_of_objects(&e);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        for (id, path) in &made {
            let p = tx.path(*id).unwrap().unwrap();
            assert_eq!(p, *path);
            assert_eq!(tx.resolve(path).unwrap().unwrap().id, *id);
        }
        let graphs: Vec<u64> = made[3..].iter().map(|m| m.0).collect();
        let mut all = graphs.clone();
        all.push(DEFAULT_GRAPH_ID);
        all.sort_unstable();
        assert_eq!(tx.graph_ids().unwrap(), all);
        let names: Vec<String> = tx
            .children(made[1].0)
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(names, ["g1", "g2"]);
        // Kinds and parents (FR-CAT-001).
        let (s1, g1) = (made[1].0, made[3].0);
        for r in [
            tx.create_graph(ROOT, "x", OWNER, None),
            tx.create_schema(s1, "x", OWNER),
            tx.create_directory(g1, "x", OWNER),
        ] {
            assert!(matches!(r, Err(Error::InvalidArgument(_))), "{r:?}");
        }
        assert!(matches!(
            tx.create_schema(999, "x", OWNER),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            tx.create_graph(s1, "g1", OWNER, None),
            Err(Error::CatalogObjectExists)
        ));
        tx.abort();
        assert_intact(&e);
    }

    /// FR-CAT-002: a committed catalog change is present after a close and
    /// a reopen, and after a drop without close; the catalog cache is
    /// loaded again at open.
    #[test]
    fn a_committed_catalog_change_survives_close_and_reopen_and_a_drop_without_close() {
        let (d, e) = db();
        let made = tree_of_objects(&e);
        let before = listing(&e);
        e.close().unwrap();
        let e = reopen(d.path());
        assert_eq!(listing(&e), before);
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let s3 = tx.create_schema(ROOT, "s3", OWNER).unwrap();
        tx.create_graph(s3, "g4", OWNER, None).unwrap();
        assert!(tx.drop_object(made[1].0, "g2").unwrap());
        tx.commit().unwrap();
        let after = listing(&e);
        drop(e);
        let e = reopen(d.path());
        assert_eq!(listing(&e), after);
        assert!(after.iter().any(|o| o.0 == ["s3", "g4"]));
        assert!(!after.iter().any(|o| o.0 == ["s1", "g2"]));
        let cache: Vec<String> = e
            .core()
            .catalog
            .objects()
            .values()
            .map(|o| o.name.clone())
            .collect();
        assert_eq!(cache.len(), after.len());
        assert_intact(&e);
    }

    /// FR-CAT-004 (catalog part): a transaction that creates a graph and
    /// other objects, then aborts, leaves none of them, nor any tree; the
    /// same holds for one dropped without commit.
    #[test]
    fn an_aborted_transaction_leaves_none_of_its_catalog_objects() {
        let (_d, e) = db();
        let before = listing(&e);
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
        for abort in [true, false] {
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let d = tx.create_directory(ROOT, "d", OWNER).unwrap();
            let s = tx.create_schema(d, "s", OWNER).unwrap();
            let t = tx.create_graph_type(s, "t", OWNER, b"(:A)").unwrap();
            tx.create_graph(s, "g", OWNER, Some(t)).unwrap();
            assert_eq!(tx.children(s).unwrap().len(), 2, "precondition");
            if abort {
                tx.abort();
            } else {
                drop(tx);
            }
            assert_eq!(listing(&e), before);
            assert_eq!(directory(&e), trees, "a tree was created");
        }
        assert_eq!(e.core().catalog.objects().len(), 2);
        assert_intact(&e);
    }

    /// TS-17 Catalog names: of two transactions that create the same graph
    /// name, the second waits for the first; if the first commits, the
    /// second fails with 25N11, at both isolation levels; if the first
    /// aborts, the second commits. Exactly one graph of that name exists.
    #[test]
    fn two_concurrent_creators_of_one_graph_name_exactly_one_commits_at_both_levels() {
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            for first_commits in [true, false] {
                let (_d, e) = db();
                let mut t1 = e.begin(iso).unwrap();
                let t2 = e.begin(iso).unwrap();
                t1.create_graph(DEFAULT_SCHEMA_ID, "g", "a", None).unwrap();
                let r2 = std::thread::scope(|s| {
                    let h = s.spawn(move || {
                        let mut t2 = t2;
                        t2.create_graph(DEFAULT_SCHEMA_ID, "g", "b", None)
                            .and_then(|_| t2.commit().map(drop))
                    });
                    until_waiting(&e, 1);
                    if first_commits {
                        t1.commit().unwrap();
                    } else {
                        t1.abort();
                    }
                    h.join().unwrap()
                });
                let mut tx = e.begin(Isolation::Snapshot).unwrap();
                let g = tx.lookup(DEFAULT_SCHEMA_ID, "g").unwrap().unwrap();
                if first_commits {
                    let err = r2.unwrap_err();
                    assert!(matches!(err, Error::Serialization), "{iso:?}: {err:?}");
                    assert_eq!(err.gqlstatus().code(), "25N11");
                    assert_eq!(g.owner, "a");
                } else {
                    r2.unwrap();
                    assert_eq!(g.owner, "b");
                }
                let n = tx
                    .children(DEFAULT_SCHEMA_ID)
                    .unwrap()
                    .iter()
                    .filter(|o| o.name == "g")
                    .count();
                assert_eq!(n, 1);
                tx.abort();
                assert_intact(&e);
            }
        }
    }

    /// The parent locks (module documentation): a schema dropped while a
    /// graph is created in it, in either order, never leaves a graph in a
    /// dropped schema, at both isolation levels: the second to lock fails
    /// with 25N11.
    #[test]
    fn a_schema_dropped_while_a_graph_is_created_in_it_leaves_no_orphan() {
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            for dropper_first in [true, false] {
                let (_d, e) = db();
                let mut tx = e.begin(iso).unwrap();
                let s = tx.create_schema(ROOT, "s", OWNER).unwrap();
                tx.commit().unwrap();
                let mut creator = e.begin(iso).unwrap();
                let mut dropper = e.begin(iso).unwrap();
                let r = std::thread::scope(|sc| {
                    if dropper_first {
                        assert!(dropper.drop_object(ROOT, "s").unwrap());
                        let h = sc.spawn(move || {
                            creator
                                .create_graph(s, "g", OWNER, None)
                                .and_then(|_| creator.commit().map(drop))
                        });
                        until_waiting(&e, 1);
                        dropper.commit().unwrap();
                        h.join().unwrap()
                    } else {
                        creator.create_graph(s, "g", OWNER, None).unwrap();
                        let h = sc.spawn(move || {
                            dropper
                                .drop_object(ROOT, "s")
                                .and_then(|_| dropper.commit().map(drop))
                        });
                        until_waiting(&e, 1);
                        creator.commit().unwrap();
                        h.join().unwrap()
                    }
                });
                assert!(
                    matches!(r, Err(Error::Serialization)),
                    "{iso:?} dropper first {dropper_first}: {r:?}"
                );
                let mut tx = e.begin(Isolation::Snapshot).unwrap();
                let schema = tx.lookup(ROOT, "s").unwrap();
                let graph = tx.lookup(s, "g").unwrap();
                assert_eq!(schema.is_some(), graph.is_some(), "{iso:?}");
                assert_eq!(schema.is_some(), !dropper_first);
                tx.abort();
                assert_intact(&e);
            }
        }
    }

    /// FR-CAT-001, rmp #31 audit B6: a graph type that a graph uses cannot
    /// be dropped (G1, dependent object), also when this transaction
    /// created the graph; once the graph is dropped, in the same
    /// transaction or an earlier one, it can.
    #[test]
    fn a_graph_type_in_use_by_a_graph_cannot_be_dropped() {
        let (_d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let gt = tx
            .create_graph_type(DEFAULT_SCHEMA_ID, "t", OWNER, b"def")
            .unwrap();
        tx.create_graph_type(DEFAULT_SCHEMA_ID, "unused", OWNER, b"x")
            .unwrap();
        tx.create_graph(DEFAULT_SCHEMA_ID, "g", OWNER, Some(gt))
            .unwrap();
        // Its own graph counts.
        let r = tx.drop_object(DEFAULT_SCHEMA_ID, "t");
        assert!(matches!(r, Err(Error::GraphTypeInUse)), "{r:?}");
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let r = tx.drop_object(DEFAULT_SCHEMA_ID, "t");
        assert!(matches!(r, Err(Error::GraphTypeInUse)), "{r:?}");
        assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "unused").unwrap());
        assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "g").unwrap());
        assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "t").unwrap());
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        assert!(tx.lookup(DEFAULT_SCHEMA_ID, "t").unwrap().is_none());
        tx.abort();
        assert_intact(&e);
    }

    /// FR-CAT-001, rmp #31 audit B6: a graph created with a graph type
    /// while a concurrent transaction drops that graph type, in either
    /// order, at both isolation levels: the second to lock the graph
    /// type's name waits for the first, then fails with 25N11, so no graph
    /// is left with a dropped graph type.
    #[test]
    fn a_graph_type_dropped_while_a_graph_of_it_is_created_leaves_no_dangling_type() {
        for iso in [Isolation::Serializable, Isolation::Snapshot] {
            for dropper_first in [true, false] {
                let what = format!("{iso:?} dropper first {dropper_first}");
                let (_d, e) = db();
                let mut tx = e.begin(iso).unwrap();
                let gt = tx
                    .create_graph_type(DEFAULT_SCHEMA_ID, "t", OWNER, b"def")
                    .unwrap();
                tx.commit().unwrap();
                let mut creator = e.begin(iso).unwrap();
                let mut dropper = e.begin(iso).unwrap();
                let r = std::thread::scope(|sc| {
                    if dropper_first {
                        assert!(dropper.drop_object(DEFAULT_SCHEMA_ID, "t").unwrap());
                        let h = sc.spawn(move || {
                            creator
                                .create_graph(DEFAULT_SCHEMA_ID, "g", OWNER, Some(gt))
                                .and_then(|_| creator.commit().map(drop))
                        });
                        until_waiting(&e, 1);
                        dropper.commit().unwrap();
                        h.join().unwrap()
                    } else {
                        creator
                            .create_graph(DEFAULT_SCHEMA_ID, "g", OWNER, Some(gt))
                            .unwrap();
                        let h = sc.spawn(move || {
                            dropper
                                .drop_object(DEFAULT_SCHEMA_ID, "t")
                                .and_then(|_| dropper.commit().map(drop))
                        });
                        until_waiting(&e, 1);
                        creator.commit().unwrap();
                        h.join().unwrap()
                    }
                });
                assert!(matches!(r, Err(Error::Serialization)), "{what}: {r:?}");
                let mut tx = e.begin(Isolation::Snapshot).unwrap();
                let ty = tx.lookup(DEFAULT_SCHEMA_ID, "t").unwrap();
                let graph = tx.lookup(DEFAULT_SCHEMA_ID, "g").unwrap();
                assert_eq!(ty.is_some(), !dropper_first, "{what}");
                assert_eq!(graph.is_some(), !dropper_first, "{what}");
                tx.abort();
                assert_intact(&e);
            }
        }
    }

    /// TS-17 Tokens: concurrent transactions that need the same label get
    /// the same token identifier, created once, whether they commit or
    /// abort; tokens survive a close and reopen and a drop without close,
    /// and a new name gets a new identifier.
    #[test]
    fn two_concurrent_creators_of_one_label_get_one_token_that_survives_reopen() {
        let (d, e) = db();
        let barrier = std::sync::Barrier::new(8);
        let ids: Vec<u64> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8)
                .map(|i| {
                    let (e, barrier) = (&e, &barrier);
                    s.spawn(move || {
                        let iso = if i % 2 == 0 {
                            Isolation::Snapshot
                        } else {
                            Isolation::Serializable
                        };
                        let mut tx = e.begin(iso).unwrap();
                        barrier.wait();
                        let id = tx.token(TokenKind::Label, "Person").unwrap();
                        if i % 3 == 0 {
                            tx.abort();
                        } else {
                            tx.commit().unwrap();
                        }
                        id
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(ids.windows(2).all(|w| w[0] == w[1]), "{ids:?}");
        let id = ids[0];
        assert_eq!(e.token_id(TokenKind::Label, "Person"), Some(id));
        assert_eq!(
            e.token_name(id),
            Some((TokenKind::Label, "Person".to_owned()))
        );
        // One commit created it: one name entry and one identifier entry.
        assert_eq!(e.core().catalog.tokens.all().len(), 1);
        // Kinds are separate name spaces.
        let ty = e.token(TokenKind::EdgeType, "Person").unwrap();
        assert_ne!(ty, id);
        e.close().unwrap();
        let e = reopen(d.path());
        assert_eq!(e.token_id(TokenKind::Label, "Person"), Some(id));
        assert_eq!(e.token_id(TokenKind::EdgeType, "Person"), Some(ty));
        let k = e.token(TokenKind::PropertyKey, "name").unwrap();
        drop(e);
        let e = reopen(d.path());
        assert_eq!(e.token_id(TokenKind::PropertyKey, "name"), Some(k));
        assert_eq!(e.token(TokenKind::Label, "Person").unwrap(), id);
        let new = e.token(TokenKind::Label, "Other").unwrap();
        assert!(![id, ty, k].contains(&new));
        assert_intact(&e);
    }

    /// FR-AUTH-002 (storage part): every object kind returns the owner
    /// recorded at its creation, also after reopen.
    #[test]
    fn every_object_kind_returns_the_owner_recorded_at_creation() {
        let (d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let dir = tx.create_directory(ROOT, "d", "alice").unwrap();
        let s = tx.create_schema(dir, "s", "bob").unwrap();
        let t = tx
            .create_graph_type(s, "t", "carol", b"definition")
            .unwrap();
        let g = tx.create_graph(s, "g", "dave", Some(t)).unwrap();
        tx.commit().unwrap();
        let check = |e: &Engine| {
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            for (id, kind, owner) in [
                (dir, Kind::Directory, "alice"),
                (s, Kind::Schema, "bob"),
                (t, Kind::GraphType, "carol"),
                (g, Kind::Graph, "dave"),
                (DEFAULT_SCHEMA_ID, Kind::Schema, OWNER),
                (DEFAULT_GRAPH_ID, Kind::Graph, OWNER),
            ] {
                let o = tx.object(id).unwrap().unwrap();
                assert_eq!((o.kind, o.owner.as_str()), (kind, owner));
            }
            let o = tx.object(t).unwrap().unwrap();
            assert_eq!(o.definition, Definition::GraphType(b"definition".to_vec()));
            let Definition::Graph(def) = tx.object(g).unwrap().unwrap().definition else {
                panic!("a graph");
            };
            assert_eq!(def.graph_type, t);
            tx.abort();
        };
        check(&e);
        e.close().unwrap();
        check(&reopen(d.path()));
    }

    /// TS-17 Dropping a graph: the dropped graph's three trees stay while a
    /// snapshot older than the drop runs, which still reads its nodes;
    /// once that snapshot ends, garbage collection frees them, and the
    /// integrity check passes.
    #[test]
    fn a_dropped_graphs_trees_are_freed_after_every_older_snapshot_ends() {
        let (_d, e) = db();
        pause_gc(&e);
        let ty = e.token(TokenKind::EdgeType, "T").unwrap();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let g = tx
            .create_graph(DEFAULT_SCHEMA_ID, "g", OWNER, None)
            .unwrap();
        let graph = tx.graph(g).unwrap();
        let (a, b) = (
            tx.create_node(&graph, &[], &[]).unwrap(),
            tx.create_node(&graph, &[], &[]).unwrap(),
        );
        tx.create_edge(&graph, a, b, ty, &[]).unwrap();
        tx.commit().unwrap();
        let mut old = e.begin(Isolation::Snapshot).unwrap();
        let graph = old.graph(g).unwrap();
        let trees = [graph.nodes, graph.edges, graph.adj];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert!(tx.drop_object(DEFAULT_SCHEMA_ID, "g").unwrap());
        tx.commit().unwrap();
        let gc = || crate::tx::gc::pass(e.core()).unwrap();
        let p = gc();
        assert_eq!(p.trees, 0, "freed under an older snapshot");
        for t in trees {
            assert!(e.store().tree(t.id()).unwrap().is_some());
        }
        assert!(old.node(&graph, a).unwrap().is_some());
        assert_eq!(old.expand(&graph, a, 0, None).unwrap().len(), 1);
        old.abort();
        let p = gc();
        assert_eq!(p.trees, 3);
        for t in trees {
            assert!(e.store().tree(t.id()).unwrap().is_none());
        }
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        assert!(tx.lookup(DEFAULT_SCHEMA_ID, "g").unwrap().is_none());
        assert!(matches!(tx.graph(g), Err(Error::NotFound(_))));
        tx.abort();
        assert_intact(&e);
    }

    /// TS-07, TS-13 step 6: after a crash (a drop without close, so
    /// `CONTROL` holds the counters of the open's checkpoint only), the
    /// catalog-object and token counters are above every committed value,
    /// recovered from the WAL commit frames.
    #[test]
    fn after_a_crash_the_catalog_and_token_counters_cover_every_committed_value() {
        let (d, e) = db();
        let mut tokens = Vec::new();
        for i in 0..20 {
            tokens.push(e.token(TokenKind::Label, &format!("L{i}")).unwrap());
        }
        // After the tokens: the last commit frames are the transaction's,
        // which must carry the catalog counter itself.
        let made = tree_of_objects(&e);
        // A transaction that draws an identifier and aborts skips it.
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let skipped = tx.create_schema(ROOT, "x", OWNER).unwrap();
        tx.abort();
        let c = *e.control().lock().unwrap().current();
        assert!(
            c.next_catalog_id <= made[0].0,
            "precondition: CONTROL is old"
        );
        drop(e);
        let e = reopen(d.path());
        let ids = e.core().ids.current();
        let max_object = made.iter().map(|m| m.0).max().unwrap();
        assert!(ids.next_catalog_id > max_object, "{ids:?}");
        assert!(ids.next_token_id > *tokens.iter().max().unwrap(), "{ids:?}");
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let next = tx.create_schema(ROOT, "y", OWNER).unwrap();
        assert!(next > max_object);
        assert_ne!(next, skipped.min(max_object));
        tx.commit().unwrap();
        let t = e.token(TokenKind::Label, "new").unwrap();
        assert!(!tokens.contains(&t));
    }

    /// Names: an empty name or one longer than [`MAX_NAME`] bytes is
    /// refused, the longest one is accepted, and directories nest as deep
    /// as needed (BR-GQL-002 IL020: bounded by the key size only).
    #[test]
    fn names_are_bounded_by_the_key_size_and_directories_nest_deeply() {
        let (_d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        let long = "n".repeat(MAX_NAME);
        assert!(tx.create_schema(ROOT, &long, OWNER).is_ok());
        for bad in ["", &"n".repeat(MAX_NAME + 1)] {
            assert!(matches!(
                tx.create_schema(ROOT, bad, OWNER),
                Err(Error::InvalidArgument(_))
            ));
        }
        let mut parent = ROOT;
        for i in 0..200 {
            parent = tx
                .create_directory(parent, &format!("d{i}"), OWNER)
                .unwrap();
        }
        let leaf = tx.create_schema(parent, "s", OWNER).unwrap();
        tx.commit().unwrap();
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let path = tx.path(leaf).unwrap().unwrap();
        assert_eq!(path.len(), 201);
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        assert_eq!(tx.resolve(&refs).unwrap().unwrap().id, leaf);
        tx.abort();
        assert_intact(&e);
    }

    /// Round C audit, unverified point (TS-16 step 6): a schema drop, or a
    /// graph-type drop, checks the latest committed graphs without the
    /// commit latch while the drop of a graph created after its snapshot is
    /// half applied; that drop then fails with ENOSPC and is undone. The
    /// schema or graph-type drop fails with 25N11, so no graph is left in a
    /// dropped schema or of a dropped graph type.
    #[test]
    fn a_drop_that_saw_an_undone_drop_of_a_newer_graph_fails() {
        for (of_type, iso) in [false, true]
            .into_iter()
            .flat_map(|t| [(t, Isolation::Snapshot), (t, Isolation::Serializable)])
        {
            let (_d, e) = db();
            pause_gc(&e);
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let s = tx.create_schema(ROOT, "s", OWNER).unwrap();
            let gt = tx
                .create_graph_type(DEFAULT_SCHEMA_ID, "t", OWNER, b"def")
                .unwrap();
            tx.commit().unwrap();
            // Under snapshot isolation only the latest committed check
            // stops the drop; a serializable dropper's read set also does.
            let mut dropper = e.begin(iso).unwrap();
            let parent = if of_type { DEFAULT_SCHEMA_ID } else { s };
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            tx.create_graph(parent, "g", OWNER, of_type.then_some(gt))
                .unwrap();
            tx.commit().unwrap();
            let r = std::thread::scope(|sc| {
                let mid = mid_batch(&e);
                let child = sc.spawn(|| {
                    let mut tx = e.begin(Isolation::Serializable)?;
                    tx.drop_object(parent, "g")?;
                    tx.commit()
                });
                mid.wait();
                // The dropper sees the half-applied drop and waits on the
                // graph's name until the child's drop is undone.
                let d = sc.spawn(move || {
                    if of_type {
                        dropper.drop_object(DEFAULT_SCHEMA_ID, "t")?;
                    } else {
                        dropper.drop_object(ROOT, "s")?;
                    }
                    dropper.commit().map(drop)
                });
                crate::test_support::wait_until("the dropper waits", || {
                    e.core().mvcc.locks.waiting() >= 1
                });
                mid.fail.send(true).unwrap();
                let c = child.join().unwrap();
                assert!(c.as_ref().is_err_and(Error::is_no_space), "{c:?}");
                d.join().unwrap()
            });
            assert!(
                matches!(r, Err(Error::Serialization)),
                "graph type {of_type}, {iso:?}: {r:?}"
            );
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            assert!(tx.lookup(parent, "g").unwrap().is_some());
            tx.abort();
            assert_intact(&e);
        }
    }

    /// Round C audit N2: a graph created and dropped after a schema or
    /// graph-type dropper's snapshot does not fail the drop under snapshot
    /// isolation, since the serial order creator, graph dropper, dropper
    /// exists. A serializable dropper still fails at commit: its read of
    /// the children (or of the graphs) overlaps the graph's creation,
    /// committed after its snapshot (FR-TX-009).
    #[test]
    fn a_child_created_and_dropped_after_the_snapshot_fails_only_a_serializable_drop() {
        for (of_type, iso) in [false, true]
            .into_iter()
            .flat_map(|t| [(t, Isolation::Snapshot), (t, Isolation::Serializable)])
        {
            let (_d, e) = db();
            pause_gc(&e);
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            let s = tx.create_schema(ROOT, "s", OWNER).unwrap();
            let gt = tx
                .create_graph_type(DEFAULT_SCHEMA_ID, "t", OWNER, b"def")
                .unwrap();
            tx.commit().unwrap();
            let mut dropper = e.begin(iso).unwrap();
            let parent = if of_type { DEFAULT_SCHEMA_ID } else { s };
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            tx.create_graph(parent, "g", OWNER, of_type.then_some(gt))
                .unwrap();
            tx.commit().unwrap();
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            assert!(tx.drop_object(parent, "g").unwrap());
            tx.commit().unwrap();
            let (dparent, dname) = if of_type {
                (DEFAULT_SCHEMA_ID, "t")
            } else {
                (ROOT, "s")
            };
            assert!(dropper.drop_object(dparent, dname).unwrap(), "{iso:?}");
            let r = dropper.commit();
            if iso == Isolation::Snapshot {
                assert!(r.is_ok(), "graph type {of_type}: {r:?}");
            } else {
                assert!(
                    matches!(r, Err(Error::Serialization)),
                    "graph type {of_type}: {r:?}"
                );
            }
            let mut tx = e.begin(Isolation::Snapshot).unwrap();
            assert!(tx.lookup(parent, "g").unwrap().is_none());
            assert_eq!(
                tx.lookup(dparent, dname).unwrap().is_none(),
                iso == Isolation::Snapshot
            );
            tx.abort();
            assert_intact(&e);
        }
    }

    /// A dropped directory or schema must be empty.
    #[test]
    fn only_an_empty_directory_or_schema_is_dropped() {
        let (_d, e) = db();
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        assert!(matches!(
            tx.drop_object(ROOT, DEFAULT_SCHEMA),
            Err(Error::NotEmpty)
        ));
        let d = tx.create_directory(ROOT, "d", OWNER).unwrap();
        tx.create_schema(d, "s", OWNER).unwrap();
        assert!(matches!(tx.drop_object(ROOT, "d"), Err(Error::NotEmpty)));
        assert!(tx.drop_object(d, "s").unwrap());
        assert!(tx.drop_object(ROOT, "d").unwrap());
        assert!(!tx.drop_object(ROOT, "d").unwrap());
        tx.commit().unwrap();
        assert_intact(&e);
    }
}

/// `loom` model of the token latch (TS-17 Tokens): two threads that need
/// the same missing token get the same identifier, and its entries are
/// made once.
#[cfg(loom)]
#[test]
fn loom_two_creators_of_one_token_get_one_identifier() {
    use super::{TokenKind, Tokens};
    use crate::sync::Arc;
    use crate::sync::atomic::{AtomicU64, Ordering};
    loom::model(|| {
        let tokens = Arc::new(Tokens::default());
        let next = Arc::new(AtomicU64::new(7));
        let made = Arc::new(AtomicU64::new(0));
        let hs: Vec<_> = (0..2)
            .map(|_| {
                let (t, n, m) = (tokens.clone(), next.clone(), made.clone());
                loom::thread::spawn(move || {
                    t.get_or_create(TokenKind::Label, "L", || {
                        m.fetch_add(1, Ordering::SeqCst);
                        Ok(n.fetch_add(1, Ordering::SeqCst))
                    })
                    .unwrap()
                })
            })
            .collect();
        let ids: Vec<u64> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(ids[0], ids[1]);
        assert_eq!(made.load(Ordering::SeqCst), 1);
        assert_eq!(tokens.get(TokenKind::Label, "L"), Some(ids[0]));
    });
}
