//! The exit check of Sprint 5 (rmp #70): every one of the 97 inventory
//! entries in one place, on one graph.
//!
//! [`all_97_entries_meet_the_exit_conditions`] checks, for each entry:
//!
//! * **Registration** (FR-ALG-001, FR-ALG-005): every algorithm routine
//!   of the registry is in the table below and every table row is
//!   registered; the table covers the 97 inventory numbers.
//! * **Classification** (FR-ALG-006): [`Entry::parallel`](super::Entry)
//!   equals the parallel column of `algorithm-entries.md`.
//! * **Determinism** (TS-25): bit-equal rows at `concurrency` 1 to 16,
//!   twice each.
//! * **Workers** (FR-ALG-006): a parallel entry starts exactly
//!   `min(concurrency, 11)` workers, so one at `concurrency` 1 and more
//!   than one and at most `concurrency` above it; any other entry at most
//!   one.
//! * **`CALL` and the Rust API** (FR-ALG-002): equal rows in one
//!   transaction, so on one snapshot.
//! * **Memory cap** (FR-ALG-004): under a 4 KiB transaction memory limit,
//!   which a streaming `MATCH` of the 11 nodes stays within (a blocking
//!   operator reserves 128 KiB chunks, `CHUNK`), the entry fails with
//!   51N73 and aborts the transaction (25N05 afterwards). A projection's
//!   first array reserves at least 1 024 elements (`mem.rs`), above the
//!   limit. The 12 functions build no projection and are left out.
//!
//! The graph has 11 nodes, so every projection of all nodes has 11
//! one-node blocks (`sched.rs`).
//!
//! [`rounds_against_the_references`] pins the round counts of ALG-106 and
//! ALG-176 against GDS's; [`the_documented_entries_are_the_registered_ones`]
//! checks the FR-ALG-001 table of `Session::run_algorithm` against the
//! registry. Graph data lives under `/tmp/lpgram` (CLAUDE.md).

use std::collections::BTreeSet;
use std::sync::Arc;

use super::tests_paths::{call_text, current, int, strs, txt};
use crate::test_support::algo::{AllowAll, drain, first_difference, record, rows};
use crate::test_support::{OWNER, graph_dir, no_flush};
use crate::types::Value;
use crate::{Database, Options, Parameters, Session, TransactionOptions};

/// The 97 entries of the inventory (`algorithm-entries.md`).
const INVENTORY: [u16; 97] = [
    1, 2, 3, 4, 6, 7, 8, 9, 11, 13, 14, 15, 18, 19, 20, 21, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
    34, 35, 38, 39, 40, 41, 42, 43, 44, 45, 59, 61, 62, 63, 64, 65, 76, 77, 78, 80, 81, 82, 83, 84,
    85, 86, 89, 90, 91, 93, 105, 106, 107, 112, 113, 114, 115, 116, 117, 120, 121, 123, 128, 143,
    147, 149, 151, 153, 154, 155, 157, 161, 163, 169, 171, 173, 174, 176, 177, 178, 180, 181, 187,
    188, 189, 190, 194, 203, 204, 205, 206,
];

/// The 38 parallel entries (`algorithm-entries.md` Parallel
/// classification).
const PARALLEL: [u16; 38] = [
    1, 6, 7, 19, 23, 24, 27, 29, 32, 34, 35, 38, 39, 40, 42, 43, 44, 45, 59, 61, 62, 63, 64, 65,
    78, 80, 81, 82, 83, 93, 105, 106, 120, 121, 153, 155, 171, 176,
];

/// The nodes of the exit graph.
const NODES: usize = 11;

/// The transaction memory limit of the over-cap check.
const LIMIT: u64 = 4096;

/// One call of the table: the inventory number, the routine, its
/// positional arguments and its configuration (none: a function without
/// a configuration parameter).
struct Call {
    alg: u16,
    proc: &'static str,
    args: Vec<Value>,
    config: Option<Value>,
}

/// The exit graph: 11 nodes `(:G:A)` (uids 0 to 4) and `(:G:B)` (5 to
/// 10) with `lat`, `lng`, `vec`, `p` and `c`; 19 edges `:D` from a lower
/// to a higher uid (a DAG; node 10 hangs from node 9 by a bridge) and 3
/// edges `:C` back (a matching, so bipartite), each with `weight`,
/// `capacity`, `cost` and `neg` (`weight`, but −20 on 9 → 0, which closes
/// a negative cycle).
struct Graph {
    _dir: tempfile::TempDir,
    db: Database,
    s: Session,
    ids: Vec<Value>,
    key: String,
}

impl Graph {
    async fn new() -> Graph {
        let dir = graph_dir();
        let db = Database::create(
            dir.path(),
            no_flush(),
            32 << 20,
            OWNER,
            Options::new(Arc::new(AllowAll)),
        )
        .await
        .expect("create");
        let s = db.session(OWNER, None, None).await.expect("session");
        let lat = [
            38.712, 38.719, 38.731, 38.705, 38.744, 38.751, 38.698, 38.737, 38.726, 38.709, 38.716,
        ];
        let lng = [
            -9.139, -9.151, -9.127, -9.162, -9.144, -9.118, -9.133, -9.171, -9.109, -9.156, -9.148,
        ];
        let vec = [
            [0.0, 0.3],
            [0.4, 0.1],
            [0.2, 0.9],
            [1.1, 0.5],
            [0.7, 1.3],
            [10.2, 9.8],
            [9.6, 10.4],
            [10.9, 10.1],
            [9.9, 9.3],
            [10.5, 11.0],
            [10.1, 10.7],
        ];
        let prize = [3.0, 1.0, 4.0, 1.5, 5.0, 9.0, 2.0, 6.0, 5.5, 3.5, 2.5];
        let mut parts = Vec::new();
        let mut ps = Parameters::new();
        for i in 0..11 {
            let label = if i < 5 { "G&A" } else { "G&B" };
            parts.push(format!(
                "(n{i}:{label} {{uid: {i}, lat: {:e}, lng: {:e}, vec: $v{i}, p: {:e}, c: {}}})",
                lat[i],
                lng[i],
                prize[i],
                usize::from(i >= 5)
            ));
            ps.insert(
                format!("v{i}"),
                Value::List(vec![Value::Float(vec[i][0]), Value::Float(vec[i][1])].into()),
            );
        }
        let edges: [(usize, usize, &str, f64); 22] = [
            (0, 1, "D", 1.5),
            (0, 2, "D", 2.25),
            (1, 2, "D", 0.75),
            (1, 3, "D", 3.0),
            (2, 3, "D", 1.25),
            (2, 4, "D", 4.5),
            (3, 4, "D", 1.0),
            (3, 5, "D", 2.5),
            (4, 5, "D", 1.75),
            (4, 6, "D", 3.25),
            (5, 6, "D", 0.5),
            (5, 7, "D", 2.0),
            (6, 7, "D", 1.125),
            (6, 8, "D", 2.75),
            (7, 8, "D", 0.625),
            (7, 9, "D", 3.5),
            (8, 9, "D", 1.375),
            (0, 9, "D", 9.0),
            (9, 0, "C", 2.0),
            (5, 3, "C", 1.0),
            (8, 6, "C", 0.875),
            (9, 10, "D", 1.0),
        ];
        for (i, (a, b, t, w)) in edges.iter().enumerate() {
            parts.push(format!(
                "(n{a})-[:{t} {{weight: {w:e}, neg: {:e}, capacity: {}, cost: {}}}]->(n{b})",
                if (*a, *b) == (9, 0) { -20.0 } else { *w },
                (i * 3) % 7 + 1,
                (i * 5) % 4 + 1
            ));
        }
        rows(&s, &format!("INSERT {}", parts.join(", ")), ps)
            .await
            .expect("insert the exit graph");
        let ids = rows(
            &s,
            "MATCH (n) RETURN n, n.uid AS u ORDER BY u",
            Parameters::new(),
        )
        .await
        .expect("the nodes")
        .into_iter()
        .map(|r| r[0].clone())
        .collect();
        let key = db.engine_handle().core().location.display().to_string();
        Graph {
            _dir: dir,
            db,
            s,
            ids,
            key,
        }
    }

    fn n(&self, i: usize) -> Value {
        self.ids[i].clone()
    }

    fn nodes(&self, is: &[usize]) -> Value {
        Value::List(is.iter().map(|i| self.n(*i)).collect::<Vec<_>>().into())
    }

    /// The rows of one call in transaction `tx`: the stream rows, or for
    /// the write mode of ALG-149 the summary columns other than the
    /// `…Millis` times and `configuration` (which echoes `concurrency`),
    /// followed by each node's written property.
    async fn rows_in(
        &self,
        tx: &crate::Transaction,
        call: &Call,
        config: Option<&Value>,
        by_api: bool,
    ) -> Result<Vec<Vec<Value>>, crate::Error> {
        let stream = if by_api {
            let r = crate::proc::Registry::with_builtins();
            let params = r
                .get(call.proc)
                .expect("registered")
                .get()
                .sig
                .params
                .clone();
            let mut ps = Parameters::new();
            for (p, a) in params.iter().zip(&call.args) {
                ps.insert(p.name.clone(), a.clone());
            }
            if let Some(c) = config {
                ps.insert("configuration".to_string(), c.clone());
            }
            tx.run_algorithm(call.proc, ps).await?
        } else {
            let (text, ps) = text_of(call, config);
            tx.execute(&text, ps).await?
        };
        let mut out = drain(stream).await?;
        if call.proc.ends_with(".write") {
            let columns = super::ENTRIES
                .iter()
                .find(|e| e.write == Some(call.proc))
                .expect("a write entry")
                .write_columns;
            for r in &mut out {
                *r = r
                    .iter()
                    .zip(columns)
                    .filter(|(_, c)| !c.ends_with("Millis") && **c != "configuration")
                    .map(|(v, _)| v.clone())
                    .collect();
            }
            out.extend(
                drain(
                    tx.execute(
                        "MATCH (n) RETURN n.uid AS u, n.exit AS x ORDER BY u",
                        Parameters::new(),
                    )
                    .await?,
                )
                .await?,
            );
        }
        Ok(out)
    }

    /// The rows of one call at `concurrency` `c` (none: a function) in a
    /// transaction rolled back afterwards, with the workers it started
    /// (none counted as one: the statement's thread).
    async fn run(&self, call: &Call, c: Option<usize>) -> (Vec<Vec<Value>>, usize) {
        let config = call.config.as_ref().map(|cfg| match c {
            Some(c) => crate::test_support::algo::with(cfg, "concurrency", int(c as i64)),
            None => cfg.clone(),
        });
        super::sched::probe::take(&self.key);
        let tx = self.s.begin(TransactionOptions::default()).await.unwrap();
        let out = self
            .rows_in(&tx, call, config.as_ref(), false)
            .await
            .unwrap_or_else(|e| panic!("ALG-{:03} {}: {e}", call.alg, call.proc));
        tx.rollback().await.unwrap();
        let w = super::sched::probe::take(&self.key).unwrap_or(1);
        (out, w)
    }
}

/// The statement of a call: [`call_text`] for a routine with a
/// configuration, else its positional arguments only.
fn text_of(call: &Call, config: Option<&Value>) -> (String, Parameters) {
    match config {
        Some(c) => call_text(call.proc, &call.args, c),
        None => {
            let mut ps = Parameters::new();
            let names: Vec<String> = (0..call.args.len()).map(|i| format!("$a{i}")).collect();
            for (i, a) in call.args.iter().enumerate() {
                ps.insert(format!("a{i}"), a.clone());
            }
            (format!("CALL {}({})", call.proc, names.join(", ")), ps)
        }
    }
}

/// The table: one call per routine (module documentation).
fn table(g: &Graph) -> Vec<Call> {
    let r = |pairs: &[(&str, Value)]| Some(record(pairs));
    let w = || ("relationshipWeightProperty", txt("weight"));
    let dag = || ("relationshipTypes", txt("D"));
    let src = || ("sourceNode", g.n(0));
    let dst = || ("targetNode", g.n(9));
    let seed = || ("randomSeed", int(7));
    let call = |alg: u16, proc: &'static str, args: Vec<Value>, config: Option<Value>| Call {
        alg,
        proc,
        args,
        config,
    };
    let v1 = Value::List(vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.5)].into());
    let v2 = Value::List(vec![Value::Float(2.0), Value::Float(0.5), Value::Float(3.0)].into());
    let pair =
        |proc: &'static str, alg: u16| call(alg, proc, vec![g.n(0), g.n(3), record(&[])], None);
    let mut t = vec![
        call(1, "gds.bfs.stream", vec![], r(&[src()])),
        call(2, "gds.dfs.stream", vec![], r(&[src()])),
        call(
            3,
            "neighbors.at_hop",
            vec![g.n(0), strs(&[]), int(2)],
            r(&[]),
        ),
        call(
            3,
            "neighbors.by_hop",
            vec![g.n(0), strs(&[]), int(2)],
            r(&[]),
        ),
        call(
            4,
            "gds.pathExpand.stream",
            vec![],
            r(&[
                ("start", g.n(0)),
                ("relationships", strs(&[])),
                ("labels", strs(&[])),
                ("minHops", int(0)),
                ("maxHops", int(2)),
            ]),
        ),
        call(
            4,
            "gds.pathSubgraphNodes.stream",
            vec![],
            r(&[("startNode", g.n(0))]),
        ),
        call(
            4,
            "gds.pathSubgraphAll.stream",
            vec![],
            r(&[("startNode", g.n(0))]),
        ),
        call(
            6,
            "gds.randomWalk.stream",
            vec![],
            r(&[("walkLength", int(4)), ("walksPerNode", int(2)), seed()]),
        ),
        call(7, "gds.dag.topologicalSort.stream", vec![], r(&[dag()])),
        call(8, "graph_util.ancestors", vec![g.n(9)], r(&[])),
        call(8, "graph_util.descendants", vec![g.n(0)], r(&[])),
        call(
            9,
            "gds.hasPathConnecting.stream",
            vec![],
            r(&[src(), dst()]),
        ),
        call(11, "gds.dominatorsSimpleFast.stream", vec![], r(&[src()])),
        call(
            13,
            "gds.unweightedShortestPaths.stream",
            vec![],
            r(&[src()]),
        ),
        call(
            14,
            "gds.shortestPath.dijkstra.stream",
            vec![],
            r(&[src(), dst(), w()]),
        ),
        call(
            14,
            "gds.allShortestPaths.dijkstra.stream",
            vec![],
            r(&[src(), w()]),
        ),
        call(
            15,
            "gds.bidirectionalDijkstra.stream",
            vec![],
            r(&[src(), dst(), w()]),
        ),
        call(
            18,
            "gds.shortestPath.astar.stream",
            vec![],
            r(&[
                src(),
                dst(),
                ("latitudeProperty", txt("lat")),
                ("longitudeProperty", txt("lng")),
                w(),
            ]),
        ),
        call(19, "gds.bellmanFord.stream", vec![], r(&[src(), w()])),
        call(
            20,
            "gds.findNegativeCycle.stream",
            vec![],
            r(&[src(), ("relationshipWeightProperty", txt("neg"))]),
        ),
        call(21, "gds.spfa.stream", vec![], r(&[src(), w()])),
        call(
            23,
            "gds.allShortestPaths.delta.stream",
            vec![],
            r(&[src(), w()]),
        ),
        call(24, "gds.johnson.stream", vec![], r(&[w()])),
        call(25, "gds.floydWarshall.stream", vec![], r(&[w()])),
        call(26, "gds.seidel.stream", vec![], r(&[])),
        call(27, "gds.allShortestPaths.stream", vec![], r(&[w()])),
        call(
            28,
            "gds.allShortestPathsBetweenNodes.stream",
            vec![],
            r(&[src(), dst()]),
        ),
        call(
            29,
            "gds.shortestPath.yens.stream",
            vec![],
            r(&[src(), dst(), ("k", int(3)), w()]),
        ),
        call(
            30,
            "gds.kShortestPath.stream",
            vec![],
            r(&[src(), ("k", int(2)), w()]),
        ),
        call(
            31,
            "algo.all_simple_paths",
            vec![g.n(0), g.n(9), strs(&[]), int(4)],
            r(&[]),
        ),
        call(32, "gds.dag.longestPath.stream", vec![], r(&[dag(), w()])),
        call(34, "gds.degree.stream", vec![], r(&[])),
        call(35, "gds.betweenness.stream", vec![], r(&[])),
        call(38, "gds.closeness.stream", vec![], r(&[])),
        call(39, "gds.closeness.harmonic.stream", vec![], r(&[])),
        call(40, "gds.eigenvector.stream", vec![], r(&[])),
        call(
            41,
            "katz_centrality.get",
            vec![Value::Float(0.2), Value::Float(0.01)],
            r(&[]),
        ),
        call(42, "gds.pageRank.stream", vec![], r(&[w()])),
        call(
            43,
            "gds.pageRank.stream",
            vec![],
            r(&[("sourceNodes", g.nodes(&[0, 5]))]),
        ),
        call(44, "gds.articleRank.stream", vec![], r(&[])),
        call(45, "gds.hits.stream", vec![], r(&[])),
        call(
            59,
            "gds.influenceMaximization.celf.stream",
            vec![],
            r(&[("seedSetSize", int(2)), seed()]),
        ),
        call(61, "gds.louvain.stream", vec![], r(&[])),
        call(62, "gds.leiden.stream", vec![], r(&[seed()])),
        call(63, "gds.labelPropagation.stream", vec![], r(&[])),
        call(
            64,
            "gds.sllpa.stream",
            vec![],
            r(&[("maxIterations", int(4)), seed()]),
        ),
        call(65, "gds.modularityOptimization.stream", vec![], r(&[])),
        call(76, "cugraph.spectral_clustering.get", vec![int(2)], r(&[])),
        call(
            77,
            "cugraph.balanced_cut_clustering.get",
            vec![int(2)],
            r(&[]),
        ),
        call(78, "gds.maxkcut.stream", vec![], r(&[seed()])),
        call(
            80,
            "gds.kmeans.stream",
            vec![],
            r(&[("nodeProperty", txt("vec")), ("k", int(2)), seed()]),
        ),
        call(
            81,
            "gds.hdbscan.stream",
            vec![],
            r(&[
                ("nodeProperty", txt("vec")),
                ("samples", int(2)),
                ("minClusterSize", int(2)),
            ]),
        ),
        call(
            82,
            "gds.modularity.stream",
            vec![],
            r(&[("communityProperty", txt("c"))]),
        ),
        call(
            82,
            "gds.conductance.stream",
            vec![],
            r(&[("communityProperty", txt("c"))]),
        ),
        call(83, "gds.wcc.stream", vec![], r(&[])),
        call(84, "gds.connectedComponents.stream", vec![], r(&[])),
        call(85, "gds.scc.stream", vec![], r(&[])),
        call(86, "gds.condensation.stream", vec![], r(&[])),
        call(89, "biconnected_components.get", vec![], r(&[])),
        call(90, "gds.articulationPoints.stream", vec![], r(&[])),
        call(91, "gds.bridges.stream", vec![], r(&[])),
        call(93, "gds.kcore.stream", vec![], r(&[])),
        call(105, "gds.nodeSimilarity.stream", vec![], r(&[])),
        call(
            105,
            "gds.nodeSimilarity.filtered.stream",
            vec![],
            r(&[
                ("sourceNodeFilter", txt("A")),
                ("targetNodeFilter", txt("B")),
            ]),
        ),
        call(
            106,
            "gds.knn.stream",
            vec![],
            r(&[("nodeProperties", txt("vec")), ("topK", int(2)), seed()]),
        ),
        call(
            106,
            "gds.knn.filtered.stream",
            vec![],
            r(&[
                ("nodeProperties", txt("vec")),
                ("topK", int(2)),
                seed(),
                ("sourceNodeFilter", txt("A")),
                ("targetNodeFilter", txt("B")),
            ]),
        ),
        call(120, "gds.triangleCount.stream", vec![], r(&[])),
        call(120, "gds.triangles", vec![], r(&[])),
        call(121, "gds.localClusteringCoefficient.stream", vec![], r(&[])),
        call(123, "gds.eccentricity.stream", vec![], r(&[])),
        call(128, "gds.girth.stream", vec![], r(&[])),
        call(143, "gds.laplacianMatrix.stream", vec![], r(&[w()])),
        call(147, "gds.spanningTree.stream", vec![], r(&[src(), w()])),
        call(
            149,
            "gds.kSpanningTree.write",
            vec![],
            r(&[src(), ("k", int(4)), ("writeProperty", txt("exit")), w()]),
        ),
        call(
            151,
            "gds.countSpanningTrees.stream",
            vec![],
            r(&[("orientation", txt("UNDIRECTED"))]),
        ),
        call(
            153,
            "gds.steinerTree.stream",
            vec![],
            r(&[src(), ("targetNodes", g.nodes(&[6, 9])), w()]),
        ),
        call(
            154,
            "gds.prizeSteinerTree.stream",
            vec![],
            r(&[("prizeProperty", txt("p")), w()]),
        ),
        call(
            155,
            "gds.maxFlow.stream",
            vec![],
            r(&[
                ("sourceNodes", g.nodes(&[0])),
                ("targetNodes", g.nodes(&[9])),
                ("capacityProperty", txt("capacity")),
            ]),
        ),
        call(
            157,
            "gds.maxFlow.minCost.stream",
            vec![],
            r(&[
                ("sourceNodes", g.nodes(&[0])),
                ("targetNodes", g.nodes(&[9])),
                ("capacityProperty", txt("capacity")),
            ]),
        ),
        call(161, "gds.maximumMatching.stream", vec![], r(&[])),
        call(163, "bipartite_matching.max", vec![], r(&[])),
        call(169, "gds.maximalCliques.stream", vec![], r(&[])),
        call(171, "gds.cliqueCounting.stream", vec![], r(&[])),
        call(173, "graph_coloring.color_graph", vec![], r(&[])),
        call(174, "gds.dsaturColoring.stream", vec![], r(&[])),
        call(176, "gds.k1coloring.stream", vec![], r(&[])),
        call(177, "gds.greedyFeedbackArcSet.stream", vec![], r(&[])),
        call(
            178,
            "set_cover.greedy",
            vec![g.nodes(&[0, 1, 2, 1, 3]), g.nodes(&[5, 5, 6, 7, 7])],
            r(&[]),
        ),
        call(180, "cycles.get", vec![], r(&[])),
        call(181, "gds.elementaryCircuits.stream", vec![], r(&[])),
        call(
            187,
            "tsp.solve",
            vec![g.nodes(&[0, 1, 2, 3, 4, 5, 6, 7])],
            r(&[]),
        ),
        call(188, "vrp.route", vec![g.n(0), int(2)], r(&[])),
        call(
            189,
            "gds.isIsomorphic.stream",
            vec![],
            r(&[
                ("otherNodeLabels", txt("G")),
                ("otherRelationshipTypes", strs(&["D", "C"])),
            ]),
        ),
        call(
            190,
            "gds.isIsomorphicSubgraph.stream",
            vec![],
            r(&[
                ("otherNodeLabels", txt("A")),
                ("otherRelationshipTypes", txt("D")),
            ]),
        ),
        call(
            194,
            "gds.isBipartiteUndirected.stream",
            vec![],
            r(&[("relationshipTypes", txt("C"))]),
        ),
        call(203, "gds.isTournament.stream", vec![], r(&[])),
        call(
            204,
            "gds.isDegreeSequenceGraphlike.stream",
            vec![],
            r(&[(
                "degreeSequence",
                Value::List(vec![int(2), int(2), int(2)].into()),
            )]),
        ),
        call(205, "gds.transitiveClosure.stream", vec![], r(&[dag()])),
        call(206, "gds.transitiveReduction.stream", vec![], r(&[dag()])),
    ];
    for f in [
        "gds.similarity.cosine",
        "gds.similarity.pearson",
        "gds.similarity.euclideanDistance",
        "gds.similarity.euclidean",
        "gds.similarity.jaccard",
        "gds.similarity.overlap",
    ] {
        t.push(call(107, f, vec![v1.clone(), v2.clone()], None));
    }
    t.push(pair("gds.linkprediction.adamicAdar", 112));
    t.push(pair("gds.linkprediction.commonNeighbors", 113));
    t.push(pair("gds.linkprediction.preferentialAttachment", 114));
    t.push(pair("gds.linkprediction.resourceAllocation", 115));
    t.push(call(
        116,
        "gds.linkprediction.sameCommunity",
        vec![g.n(0), g.n(3), txt("c")],
        None,
    ));
    t.push(pair("gds.linkprediction.totalNeighbors", 117));
    t
}

/// rmp #70: the exit conditions 1 to 5 of Sprint 5 for all 97 entries
/// (module documentation).
#[test]
fn all_97_entries_meet_the_exit_conditions() {
    current(async {
        let g = Graph::new().await;
        let table = table(&g);

        // Registration: the table names exactly the registered algorithm
        // routines (stream modes, the write-only ALG-149, the functions),
        // and covers the inventory.
        let mut registered: BTreeSet<&str> = super::ENTRIES
            .iter()
            .map(|e| {
                if e.stream.is_empty() {
                    e.write.unwrap()
                } else {
                    e.stream
                }
            })
            .collect();
        for r in super::functions::routines() {
            registered.insert(
                table
                    .iter()
                    .find(|c| c.proc == r.sig.name)
                    .unwrap_or_else(|| panic!("{} is not in the table", r.sig.name))
                    .proc,
            );
        }
        let named: BTreeSet<&str> = table.iter().map(|c| c.proc).collect();
        assert_eq!(named, registered, "the table names every routine once");
        let reg = crate::proc::Registry::with_builtins();
        for c in &table {
            assert!(reg.get(c.proc).is_some(), "{} is registered", c.proc);
        }
        let covered: BTreeSet<u16> = table.iter().map(|c| c.alg).collect();
        assert_eq!(covered, INVENTORY.iter().copied().collect());
        let by_entries: BTreeSet<u16> = super::ENTRIES
            .iter()
            .flat_map(|e| e.algs.iter().copied())
            .chain([107, 112, 113, 114, 115, 116, 117])
            .collect();
        assert_eq!(by_entries, covered, "the entries serve the inventory");

        let mut parallel_seen = BTreeSet::new();
        for call in &table {
            let entry = super::ENTRIES.iter().find(|e| {
                e.stream == call.proc || (e.stream.is_empty() && e.write == Some(call.proc))
            });
            let parallel = PARALLEL.contains(&call.alg);
            // Classification against `algorithm-entries.md`.
            if let Some(e) = entry {
                assert_eq!(
                    e.parallel, parallel,
                    "ALG-{:03} {}: classification",
                    call.alg, call.proc
                );
                assert!(e.algs.contains(&call.alg), "{}", call.proc);
            } else {
                assert!(!parallel, "{} is a function", call.proc);
            }
            if parallel {
                parallel_seen.insert(call.alg);
            }

            // Determinism and workers: bit-equal at 1 to 16, twice each.
            let conc = call.config.as_ref().map(|_| 1);
            let (first, w1) = g.run(call, conc).await;
            let mut workers = vec![w1];
            for c in 1..=16usize {
                for rep in 1..=2 {
                    let (rows, w) = g.run(call, call.config.as_ref().map(|_| c)).await;
                    if let Some((row, detail)) = first_difference(&first, &rows) {
                        panic!(
                            "ALG-{:03} {}: concurrency {c}, run {rep}, row {row}: {detail}",
                            call.alg, call.proc
                        );
                    }
                    if parallel {
                        assert!(
                            w == c.min(NODES),
                            "ALG-{:03} {}: {w} workers at concurrency {c}",
                            call.alg,
                            call.proc
                        );
                    } else {
                        assert_eq!(
                            w, 1,
                            "ALG-{:03} {}: {w} workers at concurrency {c}",
                            call.alg, call.proc
                        );
                    }
                    if rep == 1 {
                        workers.push(w);
                    }
                }
            }

            // CALL and the Rust API on one snapshot.
            let tx = g.s.begin(TransactionOptions::default()).await.unwrap();
            let by_call = g
                .rows_in(&tx, call, call.config.as_ref(), false)
                .await
                .unwrap();
            let by_api = g
                .rows_in(&tx, call, call.config.as_ref(), true)
                .await
                .unwrap();
            tx.rollback().await.unwrap();
            assert_eq!(
                first_difference(&by_call, &by_api),
                None,
                "ALG-{:03} {}: CALL and the Rust API",
                call.alg,
                call.proc
            );
            assert_eq!(first_difference(&first, &by_call), None, "{}", call.proc);

            // The memory cap: a 4 KiB transaction limit.
            let over = if call.config.is_some() {
                let mut limits = g.s.limits();
                limits.memory = Some(LIMIT);
                g.s.set_limits(limits);
                let tx = g.s.begin(TransactionOptions::default()).await.unwrap();
                let plain = drain(
                    tx.execute("MATCH (n) RETURN n.uid AS u", Parameters::new())
                        .await
                        .unwrap(),
                )
                .await
                .expect("a plain statement stays within the limit");
                assert_eq!(plain.len(), NODES);
                let e = match g.rows_in(&tx, call, call.config.as_ref(), false).await {
                    Err(e) => e,
                    Ok(r) => panic!(
                        "ALG-{:03} {}: {} rows under a {LIMIT} B limit",
                        call.alg,
                        call.proc,
                        r.len()
                    ),
                };
                assert_eq!(e.code(), "51N73", "ALG-{:03} {}: {e}", call.alg, call.proc);
                let after = tx
                    .execute("MATCH (n) RETURN n", Parameters::new())
                    .await
                    .unwrap_err();
                assert_eq!(
                    after.code(),
                    "25N05",
                    "{}: the transaction is aborted",
                    call.proc
                );
                let _ = tx.rollback().await;
                limits.memory = None;
                g.s.set_limits(limits);
                "51N73, 25N05"
            } else {
                "function: no projection"
            };
            eprintln!(
                "EXIT ALG-{:03} {}: {} rows; parallel {parallel}; workers at concurrency 1..16 {:?}; CALL = API; over the limit: {over}",
                call.alg,
                call.proc,
                first.len(),
                &workers[1..]
            );
        }
        assert_eq!(parallel_seen, PARALLEL.iter().copied().collect());
        let engine = g.db.engine_handle();
        let acc = engine.accountant();
        assert!(acc.peak() <= acc.cap(), "accountant peak within the cap");
        eprintln!(
            "EXIT accountant peak {} B, cap {} B; {} routines, {} entries",
            acc.peak(),
            acc.cap(),
            table.len(),
            covered.len()
        );
        g.db.close().await.unwrap();
    });
}

/// rmp #70: the rounds of the parallel entries whose count no earlier
/// task compared with the reference, on the reference's graphs.
///
/// * ALG-176: lpg's Jones-Plassmann rounds (the `round` events of the
///   trace) against GDS 2026.09.0's `ranIterations` (`*.gds.k1.stats`,
///   rmp #68). GDS counts speculative colouring and validation passes, a
///   different quantity; both are recorded.
/// * ALG-106: `ranIterations` and `didConverge` of `gds.knn.write`
///   against `gds.knn.stats` at `concurrency` 1 with the same seed
///   (`oracle-cases/gds_cases_70.sh`). lpg's sampling is not GDS's
///   (parity entry, TS-25), so equal counts are not expected.
///
/// The measured values are pinned, so a change of round count fails.
#[test]
fn rounds_against_the_references() {
    use super::tests_paths::Lab;
    use crate::test_support::algo::expected;
    current(async {
        // Measured on 2026-10-12 (rmp #70).
        let k1 = [
            ("karate", 6usize),
            ("zipf60", 9),
            ("uniform60", 13),
            ("zipf300", 24),
        ];
        for (f, pinned) in k1 {
            let l = Lab::load(f).await;
            let rec = super::trace::Recording::start();
            let rows = l.once("gds.k1coloring.stream", &[], record(&[])).await;
            let rounds = super::trace::only(&rec.take(), &["round"]).len();
            drop(rec);
            let gds = &expected(&format!("{f}.gds.k1.stats.jsonl"))[0];
            eprintln!(
                "LAB rounds 176 {f}: lpg {rounds} Jones-Plassmann rounds, {} rows; GDS ranIterations {}, colorCount {}",
                rows.len(),
                gds["ranIterations"],
                gds["colorCount"]
            );
            assert_eq!(rounds, pinned, "ALG-176 rounds on {f}");
            l.f.db.close().await.unwrap();
        }
        for (f, pinned) in [("zipf60", 7), ("uniform60", 6)] {
            let l = Lab::load(f).await;
            let tx = l.f.s.begin(TransactionOptions::default()).await.unwrap();
            let (text, ps) = call_text(
                "gds.knn.write",
                &[],
                &record(&[
                    ("nodeProperties", txt("vec")),
                    ("topK", int(5)),
                    ("randomSeed", int(42)),
                    ("writeRelationshipType", txt("SIM")),
                    ("writeProperty", txt("score")),
                ]),
            );
            let row = drain(tx.execute(&text, ps).await.unwrap())
                .await
                .unwrap()
                .remove(0);
            tx.rollback().await.unwrap();
            let cols = super::similarity::KNN.write_columns;
            let at = |c: &str| row[cols.iter().position(|x| *x == c).unwrap()].clone();
            let gds = &expected(&format!("{f}.gds.knn.stats.jsonl"))[0];
            eprintln!(
                "LAB rounds 106 {f}: lpg ranIterations {:?}, didConverge {:?}; GDS ranIterations {}, didConverge {}",
                at("ranIterations"),
                at("didConverge"),
                gds["ranIterations"],
                gds["didConverge"]
            );
            assert_eq!(
                (at("ranIterations"), at("didConverge")),
                (int(pinned), Value::Bool(true)),
                "ALG-106 rounds on {f}"
            );
            l.f.db.close().await.unwrap();
        }
    });
}

/// The FR-ALG-001 table of the crate documentation (`Session::run_algorithm`)
/// as the registry gives it: one row per inventory entry with its name
/// (`algorithm-entries.md`) and its routines.
fn documented_table() -> Vec<String> {
    let names = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs/technical-spec/algorithm-entries.md"),
    )
    .expect("algorithm-entries.md");
    let mut out = Vec::new();
    for alg in INVENTORY {
        let name = names
            .lines()
            .find_map(|l| {
                let cells: Vec<&str> = l.split('|').map(str::trim).collect();
                (cells.len() > 3 && cells[1] == format!("{alg:03}")).then(|| cells[2].to_string())
            })
            .unwrap_or_else(|| panic!("ALG-{alg:03} in algorithm-entries.md"));
        let mut procs: Vec<String> = Vec::new();
        for e in super::ENTRIES.iter().filter(|e| e.algs.contains(&alg)) {
            if !e.stream.is_empty() {
                procs.push(e.stream.to_string());
            }
            procs.extend(e.write.map(str::to_string));
        }
        for r in super::functions::routines() {
            let n = r.sig.name.as_str();
            let of = match n {
                _ if n.starts_with("gds.similarity.") => 107,
                "gds.linkprediction.adamicAdar" => 112,
                "gds.linkprediction.commonNeighbors" => 113,
                "gds.linkprediction.preferentialAttachment" => 114,
                "gds.linkprediction.resourceAllocation" => 115,
                "gds.linkprediction.sameCommunity" => 116,
                "gds.linkprediction.totalNeighbors" => 117,
                other => panic!("an unknown function {other}"),
            };
            if of == alg {
                procs.push(n.to_string());
            }
        }
        let parallel = if PARALLEL.contains(&alg) { "yes" } else { "no" };
        let procs: Vec<String> = procs.iter().map(|p| format!("`{p}`")).collect();
        out.push(format!(
            "| {alg:03} | {name} | {parallel} | {} |",
            procs.join(", ")
        ));
    }
    out
}

/// The crate documentation lists every provided entry with its routines
/// (FR-ALG-001), as the registry has them.
#[test]
fn the_documented_entries_are_the_registered_ones() {
    let want = documented_table();
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/session.rs"),
    )
    .expect("session.rs");
    let got: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("/// "))
        .filter(|l| l.starts_with("| ") && l.as_bytes().get(2).is_some_and(u8::is_ascii_digit))
        .map(str::to_string)
        .collect();
    if got != want {
        panic!(
            "the FR-ALG-001 table of `Session::run_algorithm` differs from the registry; expected:\n{}",
            want.join("\n")
        );
    }
}
