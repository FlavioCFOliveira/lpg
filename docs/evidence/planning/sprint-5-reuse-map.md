# Sprint 5 reuse map: graph-algorithm library (97 entries)

Prepared 2026-10-12 for rmp task #5 (plan: [sprint-5-plan.md](sprint-5-plan.md) §4). Scope: the 97 kept entries of [graph-algorithms.md](../../functional-spec/graph-algorithms.md). The count was measured: an `awk` pass over the inventory tables before "Derived procedure names" gives 97 unique identifiers. Rules applied: `CLAUDE.md` "Open-source references" and "Copying code"; [TS-25](../../technical-spec/algorithms.md#ts-25-graph-algorithms).

**Method.** Each licence was read in the cloned repository's licence file (file named in §1) or, for a crate that was not cloned, in the crates.io API `license` field of its version (User-Agent `lpg-planning (https://github.com/FlavioCFOliveira/lpg)`, queried 2026-10-12). Clones are shallow (`--depth 1`) under `/tmp/<project>`. Points checked again during planning are marked "verified 2026-10-12" with their evidence (§6). Points that remain open are listed in §5.

**Verdicts.** **call**: lpg calls the function of a dependency. **copy**: Rust code copied, then adapted. **port**: code rewritten in Rust from another language, keeping its structure. **study**: read only; nothing copied. GPL, BSL and Memgraph Enterprise code is study only.

## 1. Sources

The abbreviations in the first column are used in §2.

| Abbr. | Source | Version or commit | Licence (file read) | Language | Verdict | Reason |
|---|---|---|---|---|---|---|
| PG | petgraph | 0.8.3 (crates.io source, `~/.cargo/registry/src/*/petgraph-0.8.3`); git `86647f9c96ea` in `/tmp/petgraph` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | Rust | **call**, **copy** | Already a TS-25 dependency; 548.2 M downloads. 10 of its functions do not compile on `Csr` (§3.1). |
| RWX | rustworkx-core, and the `rustworkx` binding crate `src/` | workspace 0.18.0 at `25398ab47f66`; crates.io 0.18.1 | Apache-2.0 (`/tmp/rustworkx/LICENSE`; `Cargo.toml:25`) | Rust | **copy** (core); **port** (`src/`, pyo3-bound) | 1.57 M downloads. It depends on rayon, ndarray and hashbrown (`rustworkx-core/Cargo.toml`), so lpg copies code and does not depend on it (TS-25 rejects it as a dependency). |
| GA | graphalgs | 0.2.0 at `1f8a412ef4af` (the inventory's commit) | MIT (`/tmp/graphalgs/LICENSE`, `Cargo.toml`) | Rust | **copy** | Uses petgraph 0.8 and nalgebra 0.33; lpg uses nalgebra 0.35 (§3.4). No rayon, so no parallel code. |
| PF | pathfinding | 4.16.0 at `cd4600b36ec0` | Apache-2.0/MIT (`Cargo.toml`) | Rust | **copy** | 3.05 M downloads. Yen, Edmonds-Karp, Kuhn-Munkres, cliques. Generic over closures, not over petgraph traits. |
| GR | graphrs | 0.12.0 at `1cddb87eb47c` | MIT (`Cargo.toml`) | Rust | copy (fallback) | 47 k downloads. Louvain, Leiden, clustering, centrality. Uses rayon (`Cargo.toml:31`). |
| GN | graphina | 0.4.0-alpha.6 at `212717ec4749` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | Rust | copy (fallback only) | Alpha release, 3.4 k downloads. The only Rust source found for harmonic centrality, personalised PageRank, label propagation, spectral clustering (nalgebra 0.33.2) and a TSP approximation. |
| NP | network_partitions (graspologic-native) | 0.3.0 at `773181488b4e` | MIT (`/tmp/graspologic-native/LICENSE`, package `Cargo.toml`) | Rust | **copy** | Microsoft's Leiden. Depends on `rand = "0.10"`, the major version lpg uses, and optionally on petgraph 0.8. Not published on crates.io (the API answers "crate `network_partitions` does not exist"). |
| HD | hdbscan | 0.12.0 at `82ac6aa9a378` | MIT OR Apache-2.0 (`LICENSE-*`, `Cargo.toml`) | Rust | **copy** | 158 k downloads. Depends on `kdtree = "0.7.0"` (`Cargo.toml:15`), whose licence is MIT OR Apache-2.0 (verified 2026-10-12, §6). rayon is optional (feature `parallel`). |
| LF | linfa-clustering | 0.8.1 at `3b411a4c985c` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE2`) | Rust | port | k-means over ndarray: the logic ports, the types do not. |
| — | hopcroft-karp | 0.2.1 (crates.io) | MIT (crates.io) | Rust | copy (candidate) | 70.8 k downloads. Not cloned; code not read (§5). |
| — | vf2 | 1.0.1 (crates.io) | MIT OR Apache-2.0 (crates.io) | Rust | study (fallback) | 17.8 k downloads. petgraph's VF2 is preferred. Not cloned. |
| — | vrp-core | 1.25.0 (crates.io) | Apache-2.0 (crates.io) | Rust | study | A large metaheuristic vehicle-routing framework. Not cloned. |
| — | leiden-rs, xgraph, dachshund, fusion-blossom | crates.io | MIT OR Apache-2.0; MIT; MIT; MIT | Rust | not studied | NP covers Leiden; the others add nothing over PG and RWX for the inventory (not read). |
| — | single-clustering, fa-leiden-cd | crates.io | not a standard SPDX identifier | Rust | rejected | Licence not a standard permissive identifier. |
| — | steiner-tree | 0.0.5 | GPL-3.0-or-later (crates.io) | Rust | rejected | GPL; it also solves the rectilinear (geometric) problem, not the graph problem. |
| RAPH | Raphtory | 0.17.0 at `1d6ac0b32a83` | GPL-3.0 (`/tmp/raphtory/LICENSE`) | Rust | **study only** | Copyleft. |
| GRAF | grafeo | `3c23dbea11d8` | Apache-2.0 (`/tmp/grafeo/LICENSE`) | Rust | not used | No relevant algorithm module found; not inspected in depth (§5). |
| MINI | minigu | `fc2ccf59f5cc` | Apache-2.0 (`/tmp/minigu/LICENSE`) | Rust | not used | As grafeo (§5). |
| NX | NetworkX | `6da4704cbf32` (3.7.1rc0.dev0, 2026-10-06); PyPI 3.7 in the laboratory | BSD-3-Clause (`LICENSE.txt`) | Python | **port**; **test oracle** | Reference implementations for almost every entry; test suites under `networkx/algorithms/**/tests/`. |
| NK | NetworKit | `5f03e17cec09` | MIT (`License.txt:6`) | C++ (OpenMP) | **port** | Parallel PLM (Louvain), ParallelLeiden, PLP, CoreDecomposition, Betweenness, PageRank, Closeness (OpenMP pragmas found by `grep`); Dinic; SuccessiveShortestPath. |
| KZ | Kùzu | `89f0263cc7a1` (sparse checkout; read with `git show`) | MIT (`/tmp/kuzu/LICENSE`) | C++ | **port** | Parallel frontier and morsel framework (`src/function/gds/gds_frontier.cpp`, `frontier_morsel.cpp`); `extension/algo` (WCC, SCC colouring, k-core, Louvain, PageRank, Kruskal spanning forest). |
| MG | Memgraph MAGE | `c72636014a8b` (2026-01-23; the inventory's commit) | Apache-2.0 (`/tmp/mage/LICENSE`). Exceptions: `cpp/betweenness_centrality_module/*online*` carry the Memgraph Enterprise License (header line 3) and serve withdrawn entries only; `cpp/community_detection_module/grappolo` is BSD-3-Clause (`grappolo/LICENSE`), but files such as `Utility/utilityDataStructures.cpp:12-21` carry a GPL header | C++, Python | **port** (grappolo: study only) | The only permissive source for several MAGE-only entries (`path.expand`, `neighbors`, set cover, TSP, node similarity, KNN). |
| MGc | Memgraph core | `c829e614d426` | BSL-1.1 (change date 2030-09-10) and MEL (`LICENSE`, `licenses/BSL.txt`) | C++ | **study only** | Not permissive. Built-in BFS, weighted, all and k shortest path cursors (`src/query/plan/operator.cpp:2477-4044`). |
| CG | cuGraph | `c9406a29ba00` | Apache-2.0 (`LICENSE`) | CUDA C++ | port of the method only | GPU code (RAFT). Gives the structure of spectral and balanced-cut clustering (`cpp/src/community/legacy/spectral_clustering.cu:36,110`). |
| LA | LAGraph | `7543de1f3103` | BSD-2-Clause (`LICENSE`) | C (GraphBLAS) | port of the method only | Linear-algebra formulations; not portable without a GraphBLAS. |
| GDS | Neo4j Graph Data Science | source `417d7ecf973d` (2026-10-08); plugin 2026.09.0 inside image `neo4j:2026.09.0-community` | GPL-3.0 (`LICENSE.txt`) | Java | **study only**; **run** as a test oracle | Parameters, output columns and parallel classification. HDBSCAN, maximum flow, minimum-cost flow, clique counting and prize-collecting Steiner tree have no class in the open repository (`grep`), but their procedures are in the shipped plugin and run (verified 2026-10-12, §6). |
| IG | igraph | `912f99d59134` | GPL-2.0 (`COPYING`) | C | **study only** | Copyleft; needed by no entry. |

## 2. Per-entry table (97 entries)

The column "Par." applies TS-25: **yes** exactly when GDS, MAGE, petgraph or graphalgs provides a parallel version. Evidence types:

* GDS source that uses `ParallelUtil`, `RunWithConcurrency`, `PartitionUtils`, `ExecutorService` or Pregel (`grep` over `algo/src/main/java/org/neo4j/gds`);
* the GDS documentation attribute `:sequential: true`, whose partial `common-configuration-jobid-concurrency-entries.adoc` renders "The algorithm is single-threaded";
* MAGE `std::thread`, `std::async`, OpenMP or `multiprocessing` (`grep`);
* petgraph `#[cfg(feature = "rayon")]`;
* graphalgs, which has no rayon dependency and so is never parallel.

GDS paths are relative to `/tmp/graph-data-science/algo/src/main/java/org/neo4j/gds/` unless they start with `doc/`. "PG ✓" and "PG ✗" mean that the petgraph function compiles, or does not compile, on `Csr` (compile probe, §3.1).

### Traversal and ordering

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 001 | BFS | PG `visit::Bfs` (`src/visit/traversal.rs`), PG ✓ | RWX `traversal/bfs_visit.rs` (`breadth_first_search`, `bfs_layers`) | **yes**: GDS `paths/traverse/BFS.java:210` `ParallelUtil.run(bfsTaskList…)` | Parallel level-synchronous frontier: port KZ `src/function/gds/gds_frontier.cpp`, `frontier_morsel.cpp` (MIT) into the block scheme. |
| 002 | DFS | PG `visit::Dfs`, `DfsPostOrder` | RWX `traversal/dfs_edges.rs`, `dfs_visit.rs` | no: `doc/.../algorithms/dfs.adoc:7` `:sequential: true`; no parallel code in MGc DFS | — |
| 003 | Neighbourhood at hop | MG `cpp/neighbors_module/algorithm/neighbors.cpp` (`at_hop`, `by_hop`) | RWX `bfs_layers`; NX `traversal/breadth_first_search.py` `descendants_at_distance` | no: MG `neighbors` absent from the thread `grep`; no G, P or A source | — |
| 004 | Path expansion and subgraph | MG `cpp/path_module/algorithm/path.cpp` (`expand`, `subgraph_nodes`, `subgraph_all`) | — | no: MG `path` absent from the thread `grep` | Port the label and relationship filter semantics from MG. |
| 006 | Random walk | RWX `traversal/random_walk.rs:55` `generate_random_path` | NX `walks.py` | **yes**: GDS `traversal/RandomWalk*.java` (`RunWithConcurrency`, `ExecutorService`) | GDS has the node2vec bias (`returnFactor`, `inOutFactor`); RWX does not, so that part is written. Per-block generator (§3.3). |
| 007 | Topological sort | PG `src/algo/mod.rs:211` `toposort`, **PG ✗** (needs `IntoNeighborsDirected`): copy | RWX `dag_algo.rs` `lexicographical_topological_sort`; MG `graph_util_module/algorithms/topological_sort.hpp` | **yes**: GDS `dag/topologicalsort/TopologicalSort.java:99,120` (`parallelForEachNode`, ForkJoin) | Parallel Kahn by levels fits synchronous rounds. |
| 008 | Ancestors and descendants | RWX `traversal/mod.rs` `ancestors`, `descendants` | MG `graph_util_module/algorithms/ancestors.hpp`, `descendants.hpp` | no: MG absent from the thread `grep` | Ancestors need the reverse adjacency (§3.1). |
| 009 | Reachability test | PG `has_path_connecting` (`mod.rs:369`), PG ✓: call | — | no | — |
| 011 | Dominators and frontiers | PG `dominators::simple_fast` (`dominators.rs:178`), PG ✓: call | RWX `src/dominance.rs:39,80` (`immediate_dominators`, `dominance_frontiers`; pyo3: port the frontier part) | no | — |

### Shortest and weighted paths

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 013 | Unweighted shortest paths | RWX BFS and PF `directed/bfs.rs:65,176` (`bfs`, `bfs_bidirectional`) | NX `shortest_paths/unweighted.py` | no: sources M (MGc cursors `operator.cpp:2477-2745`, no parallel code in that range) and N only | — |
| 014 | Dijkstra | PG `dijkstra` (`dijkstra.rs:88`), PG ✓ (distances only) | RWX `shortest_path/dijkstra.rs` (with paths) | no: `doc/.../dijkstra-single-source.adoc:9,28` sequential | Paths: copy the RWX predecessor tracking. |
| 015 | Bidirectional Dijkstra | PG `dijkstra.rs:287`, **PG ✗** (`IntoEdgesDirected`): copy | NX `shortest_paths/weighted.py` | no | Needs the reverse adjacency. |
| 018 | A* | PG `astar` (`astar.rs:79`), PG ✓: call | MG `cpp/algo_module/algorithm/algo.cpp` (`astar`) | no: `doc/.../astar.adoc:33` "executed using a single thread" | — |
| 019 | Bellman-Ford | PG `bellman_ford` (`bellman_ford.rs:94`), PG ✓ (sequential) | NX `weighted.py` | **yes**: GDS `paths/bellmanford/BellmanFord.java:108,115` (`RunWithConcurrency`) | The parallel form is synchronous relaxation rounds (round k reads round k−1); written in lpg. |
| 020 | Negative-cycle detection | PG `find_negative_cycle` (`bellman_ford.rs:170`), PG ✓: call | NX | no | — |
| 021 | SPFA | PG `spfa` (`spfa.rs:81`), PG ✓: call | — | no | — |
| 023 | Delta-stepping | No permissive Rust source. Port the method of LA `src/algorithm/LAGr_SingleSourceShortestPath.c` (BSD-2-Clause) | GDS `paths/delta/DeltaStepping.java` (study) | **yes**: GDS `paths/delta/DeltaStepping.java` (`PartitionUtils`, `ExecutorService`); `doc/.../delta-single-source.adoc:48` | Written over the CSR: bucket loop with a parallel relaxation per block. |
| 024 | Johnson | PG `johnson.rs:95`, PG ✓; `parallel_johnson` `johnson.rs:203-205` | NX | **yes**: PG `parallel_johnson` (`#[cfg(feature = "rayon")]`, `johnson.rs:13,203`) | Copy `parallel_johnson` and replace `par_iter` by blocks. Already classified in TS-25. |
| 025 | Floyd-Warshall | PG `floyd_warshall` (`floyd_warshall.rs:91,201`), PG ✓: call | GA `shortest_path/floyd_warshall.rs` | no: PG and GA are sequential | — |
| 026 | Seidel | GA `shortest_path/seidel.rs:58` (nalgebra `DMatrix`): copy | — | no: GA | Adapt from nalgebra 0.33 to 0.35. |
| 027 | All-pairs shortest distances | GA `shortest_path/shortest_distances.rs:24`, `floyd_warshall.rs:137` `distance_map` | RWX `shortest_path/distance_matrix.rs` (rayon BFS per source, `:154`) | **yes**: GDS `allshortestpaths/MSBFSAllShortestPaths.java:73` (`executorService.submit`) | One BFS or Dijkstra per source block: a direct fit for the block scheme. |
| 028 | All shortest paths between nodes | RWX `shortest_path/all_shortest_paths.rs`, `single_source_all_shortest_paths.rs` | NX `shortest_paths/generic.py` | no: MGc `ExpandAllShortestPathsCursor` (`operator.cpp:3637`), no parallel code | — |
| 029 | Yen's k shortest paths | PF `directed/yen.rs:143`: copy | NX `simple_paths.py` `shortest_simple_paths` | **yes**: GDS `paths/yens/Yens.java:111` (`RunWithConcurrency`); `doc/.../yens.adoc:32` | Parallel over the spur nodes of one round. |
| 030 | Length of the k-th shortest path | PG `k_shortest_path` (`k_shortest_path.rs:84`), PG ✓: call | RWX `shortest_path/k_shortest_path.rs` | no | — |
| 031 | All simple paths | PG `simple_paths.rs:70,207`, **PG ✗** (`IntoNeighborsDirected`): copy | RWX `connectivity/all_simple_paths.rs`; MG `algo.cpp` | no: MG `algo` absent from the thread `grep` | The output can be exponential in size: memory accounting needed. |
| 032 | Longest path in a DAG | RWX `dag_algo.rs` `longest_path`, `longest_path_length` | NX `dag.py` | **yes**: GDS `dag/longestPath/DagLongestPath.java:95` (`parallelForEachNode`, ForkJoin) | — |

### Centrality

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 034 | Degree centrality | Written (trivial) | MG `cpp/degree_centrality_module/algorithm/degree_centrality.cpp`; RWX `centrality.rs` `degree_centrality` | **yes**: GDS `degree/DegreeCentrality.java:166-172` | — |
| 035 | Betweenness | RWX `centrality.rs` `betweenness_centrality` (Brandes; rayon `CondIterator` at `:110`): copy | MG `cpp/betweenness_centrality_module/algorithm/betweenness_centrality.cpp` (Apache-2.0; `std::thread`) | **yes**: GDS `betweenness/*` (3 parallel files); MG (thread `grep`) | Per-source-block partial sums merged in block order. GDS sampling (`samplingSize`, `samplingSeed`). |
| 038 | Closeness | RWX `centrality.rs` `closeness_centrality`, `newman_weighted_closeness_centrality` | NX `centrality/closeness.py`; NK `centrality/Closeness.cpp` | **yes**: GDS `closeness/ClosenessCentrality.java` (`PartitionUtils`, `ExecutorService`) | GDS uses MSBFS and offers the Wasserman-Faust formula. |
| 039 | Harmonic | GN `centrality/harmonic.rs` | NX `centrality/harmonic.py`; NK `HarmonicCloseness.cpp` | **yes**: GDS `harmonic/HarmonicCentrality.java` | Same per-source BFS kernel as 038. |
| 040 | Eigenvector | RWX `centrality.rs` `eigenvector_centrality` | NX `centrality/eigenvector.py` | **yes**: GDS `pagerank/EigenvectorComputation.java`, run by `PageRankAlgorithm.java` (parallel) | Shares the power-iteration kernel with 042, 043 and 044. |
| 041 | Katz | RWX `centrality.rs` `katz_centrality` | MG `cpp/katz_centrality_module/algorithm/katz.cpp`; NX `katz.py` | no: MG `katz` absent from the thread `grep`; not in GDS, PG or GA | — |
| 042 | PageRank | PG `page_rank.rs:63`, PG ✓; `parallel_page_rank` `page_rank.rs:133-134`: copy | MG `cpp/pagerank_module/algorithm/pagerank.cpp`; KZ `extension/algo/src/function/page_rank.cpp` | **yes**: PG `parallel_page_rank` (`#[cfg(feature = "rayon")]`); GDS; MG | Already classified in TS-25. |
| 043 | Personalised PageRank | The 042 kernel with a teleport vector; GN `centrality/personalized_pagerank.rs` | NX `link_analysis/pagerank_alg.py` (`personalization`) | **yes**: GDS `doc/.../page-rank.adoc:574-591` (`sourceNodes`, same parallel `PageRankAlgorithm`); MG cuGraph (GPU) `cpp/cugraph_module/algorithms/personalized_pagerank.cu:143` | The inventory lists only MAGE cuGraph for this entry; the GDS evidence comes from GDS PageRank (plan §3.2 item A). |
| 044 | ArticleRank | No permissive source; derived from the 042 kernel | GDS `pagerank/ArticleRankComputation.java` (study) | **yes**: GDS `PageRankAlgorithm.java` | Written from scratch (a short formula). |
| 045 | HITS | RWX `src/link_analysis.rs:272` `hits` (ndarray and sprs: port) | NX `link_analysis/hits_alg.py`; LA `experimental/algorithm/LAGraph_HITS.c` | **yes**: GDS `hits/Hits.java:55` (Pregel, `ExecutorService`) | Synchronous rounds are natural. |
| 059 | Influence maximisation (CELF) | No permissive source found | GDS `influenceMaximization/CELF.java`, `ICLazyForwardMC.java` (study) | **yes**: GDS `influenceMaximization/ICLazyForwardMC.java` (`RunWithConcurrency`, `PartitionUtils`) | Written from scratch: Monte Carlo independent-cascade simulations per block, one generator per block. |

### Community detection and clustering

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 061 | Louvain | GR `community/louvain.rs` (MIT) | NK `community/PLM.cpp` (parallel, MIT); KZ `extension/algo/src/function/louvain.cpp`; GN `community/louvain.rs` | **yes**: GDS `louvain/Louvain.java`; MG `community_detection_module` (grappolo, OpenMP) | Do **not** port grappolo (GPL headers). The parallel local move needs synchronous rounds (TS-25). |
| 062 | Leiden | NP `leiden/*.rs` (MIT, rand 0.10): copy | GR `community/leiden/`; NK `community/ParallelLeiden.cpp` | **yes**: GDS `leiden/*` (6 parallel files); MG `leiden_community_detection_module/algorithm/leiden.cpp` (`std::thread`) | NP is sequential; its local-moving phase is parallelised per block. |
| 063 | Label propagation | GN `community/label_propagation.rs` | NK `community/PLP.cpp` (parallel); NX `community/label_propagation.py` | **yes**: GDS `labelpropagation/LabelPropagation.java:118` | Synchronous label propagation can oscillate on bipartite structures; the tie-break rule must be fixed. |
| 064 | SLLPA | No permissive source found | GDS `sllpa/SpeakerListenerLPA.java` (study; Pregel) | **yes**: GDS `sllpa/*` (3 parallel files) | Written from scratch. |
| 065 | Modularity optimisation | lpg's 061 local-move kernel | GDS `modularityoptimization/*` (study) | **yes**: GDS `modularityoptimization/ModularityManager.java:61-72` | — |
| 076 | Spectral clustering | GN `community/spectral.rs` (nalgebra `DMatrix`), nalgebra `SymmetricEigen` and lpg k-means (080) | CG `cpp/src/community/legacy/spectral_clustering.cu:110` `spectralModularityMaximization` (method); MG wrapper `cpp/cugraph_module/algorithms/spectral_clustering.cu:94` | **yes by the literal rule** only: the one source is MAGE's cuGraph GPU module | Needs a user decision (plan §3.2 item A). TS-25 names dense nalgebra. |
| 077 | Balanced-cut clustering | The same pipeline on the Laplacian; CG `spectral_clustering.cu:36` `balancedCutClustering_impl` (method) | MG `cpp/cugraph_module/algorithms/balanced_cut_clustering.cu` | **yes by the literal rule** only: MAGE cuGraph GPU | As 076. TS-25 does not list 077 among the nalgebra entries. |
| 078 | Approximate maximum k-cut | No permissive source found | GDS `approxmaxkcut/*` (study; GRASP and local search) | **yes**: GDS `approxmaxkcut/PlaceNodesRandomly.java:89`; `doc/.../approx-max-k-cut.adoc:51` "parallelized" | Written from scratch. |
| 080 | K-means | LF `algorithms/linfa-clustering/src/k_means/algorithm.rs`, `init.rs` (ndarray: port) | MG `python/kmeans.py` (an sklearn wrapper; not portable) | **yes**: GDS `kmeans/Kmeans.java` (`RunWithConcurrency`) | GDS uses a k-means++ sampler (`kmeans/KmeansSampler.java`). |
| 081 | HDBSCAN | HD `src/hdbscan.rs`, `core_distances*`, `min_spanning_tree.rs`, `union_find.rs`: copy | — | **yes**: GDS online documentation <https://neo4j.com/docs/graph-data-science/current/algorithms/hdbscan/>: `concurrency` is "The number of concurrent threads used for running the algorithm" (the sequential wording is absent) | Not in the open GDS repository; in the shipped plugin (§6). |
| 082 | Partition quality | NX `community/quality.py` (`modularity`, `partition_quality`, `constant_potts_model`): port | NK `community/Modularity.cpp`, `Conductance.cpp`, `Coverage.cpp`; LA `experimental/algorithm/LAGr_PartitionQuality.c`, `LAGr_Modularity.c` | **yes**: GDS `modularity/ModularityCalculator.java:82-95`, `conductance/Conductance.java:104-110` | — |

### Components and connectivity

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 083 | WCC | PG `unionfind::UnionFind` (`src/unionfind.rs:17`) and an edge scan (PG `connected_components` returns only the count, `mod.rs:136`) | KZ `extension/algo/src/function/weakly_connected_components.cpp`; NK `components/ParallelConnectedComponents.cpp`; LA `src/algorithm/LG_CC_FastSV6.c` | **yes**: GDS `wcc/*` (4 parallel files); `doc/.../wcc.adoc:29-30` | Parallel form: minimum-label rounds, or a union-find per block merged in block order. |
| 084 | Connected components (undirected) | PG `connected_components` (`mod.rs:136`), PG ✓; labels with `UnionFind` | RWX `connectivity/conn_components.rs` | no: P and N only | — |
| 085 | SCC | PG `tarjan_scc` (`scc/tarjan_scc.rs:267`), PG ✓: call; `kosaraju_scc` PG ✗ | KZ `strongly_connected_components.cpp` (parallel forward and backward colouring) | no: GDS `scc/Scc.java` has no concurrency (`grep`); PG is sequential | — |
| 086 | Condensation | PG `condensation` (`mod.rs:477`) takes an owned `Graph`: copy its logic over the `tarjan_scc` output | NX `components/strongly_connected.py` | no | — |
| 089 | Biconnected components | RWX `connectivity/biconnected.rs` (component output) | MG `cpp/biconnected_components_module/algorithm/biconnected_components.cpp`; NX `components/biconnected.py` | no: MG absent from the thread `grep` | — |
| 090 | Articulation points | PG `articulation_points` (`articulation_points.rs:51`), PG ✓: call | GA `connect/articulation_points.rs:38` | no: `doc/.../articulation-points.adoc:7` sequential | — |
| 091 | Bridges | PG `bridges` (`bridges.rs:54`), PG ✓: call | GA `connect/find_bridges.rs`; MG `bridges.cpp` | no: `doc/.../bridges.adoc:7` sequential | — |
| 093 | k-core | RWX `connectivity/core_number.rs`: copy | KZ `extension/algo/src/function/k_core_decomposition.cpp` (parallel); NK `centrality/CoreDecomposition.cpp` | **yes**: GDS `kcore/KCoreDecomposition.java:78,119`; `doc/.../k-core.adoc:29` | — |

### Similarity and link prediction

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 105 | Node similarity | MG `cpp/node_similarity_module/algorithms/node_similarity.hpp`: port | NX `link_prediction.py` `jaccard_coefficient` | **yes**: GDS `similarity/nodesim/NodeSimilarity.java` (`ExecutorService`) | GDS `topK`, `degreeCutoff`, `similarityCutoff`. |
| 106 | KNN | MG `cpp/knn_module/algorithms/knn.hpp`: port | GDS `similarity/knn/*` (study; NN-descent) | **yes**: GDS `similarity/knn/Knn.java`; MG `knn.hpp` (thread `grep`) | GDS is deterministic only at `concurrency` 1 (TS-25); lpg is deterministic at every value. |
| 107 | Vector similarity functions | Written from scratch (scalar formulas) | GDS `algorithms/similarity` (study) | no: scalar functions; no source has a parallel version | Not a graph traversal. |
| 112 | Adamic-Adar | NX `link_prediction.py` | GN `links/allocation.rs`; NK `linkprediction/AdamicAdarIndex.cpp` | no: the GDS link-prediction functions are scalar | — |
| 113 | Common neighbours | Written (set intersection) | NK `linkprediction/` | no | — |
| 114 | Preferential attachment | NX `link_prediction.py` | GN `links/attachment.rs` | no | — |
| 115 | Resource allocation | NX `link_prediction.py` | GN `links/allocation.rs`; NK `ResourceAllocationIndex.cpp` | no | — |
| 116 | Same community | Written (property equality) | NK `linkprediction/SameCommunityIndex.cpp` | no | — |
| 117 | Total neighbours | Written (union size) | — | no | — |

### Structural measures

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 120 | Triangle count and listing | GN `parallel/triangles.rs` (rayon) | LA `src/algorithm/LAGr_TriangleCount.c` (method); NX `cluster.py` `triangles`, `all_triangles` | **yes**: GDS `triangle/IntersectingTriangleCount.java` (`ExecutorService`) | Degree-ordered forward intersection over sorted CSR rows; counts per block, merged in order. |
| 121 | Clustering coefficient and transitivity | RWX `transitivity.rs` (`graph_transitivity`, `digraph_transitivity`; rayon at `:65,139`) | NX `cluster.py`; GR `cluster/*.rs` (including `square.rs`) | **yes**: GDS `triangle/LocalClusteringCoefficient.java:99` | — |
| 123 | Eccentricity, diameter, radius, centre, periphery | GA `metrics.rs:28-466`: copy | NX `distance_measures.py` | no: GA | — |
| 128 | Girth | GA `metrics.rs:534`: copy | NX `cycles.py` | no | — |
| 143 | Laplacian matrix | GA `spec/laplacian.rs:109` (nalgebra `DMatrix`): copy | — | no | A TS-25 nalgebra entry. |

### Spanning trees, flows and cuts

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 147 | Minimum spanning tree (Kruskal, Prim, Borůvka) | PG `min_spanning_tree`, `min_spanning_tree_prim` (`min_spanning_tree.rs:90,262`), PG ✓: call | GA `mst/boruvka.rs:46`, `mst/prim.rs:41`; KZ `spanning_forest.cpp` (Kruskal) | no: `doc/.../minimum-weight-spanning-tree.adoc:8,42` | — |
| 149 | k-spanning tree | No permissive source | GDS `kspanningtree/KSpanningTree.java` (study) | no: `doc/.../k-minimum-weight-spanning-tree.adoc:9` sequential | Written from scratch on top of 147. |
| 151 | Number of spanning trees | GA `spec/st.rs:79` (nalgebra determinant): copy | NX `tree/mst.py` `number_of_spanning_trees` | no | GA returns `mtrx.determinant() as usize` (`st.rs:86`): rounding of the `f64` loses exactness for large counts. NetworkX returns a float too (§6). |
| 153 | Steiner tree | PG `steiner_tree.rs:179` (requires `UnGraph`): copy; RWX `steiner_tree.rs` (rayon `:79`) | NX `approximation/steinertree.py` | **yes**: GDS `steiner/ShortestPathsSteinerAlgorithm.java` (`ExecutorService`); `doc/.../directed-steiner-tree.adoc:28` | GDS `gds.steinerTree` is a **rooted directed** heuristic; PG, RWX and NX are the undirected metric-closure 2-approximation. Semantics to decide (plan §3.2 item C). |
| 154 | Prize-collecting Steiner tree | No permissive source verified | GDS `pricesteiner/PCSTFast.java` (study) | no: GDS `pricesteiner/` has no concurrency (`grep`) | Written from scratch (Goemans-Williamson growth and strong pruning). |
| 155 | Maximum flow | PG `maximum_flow/dinics.rs:73`, **PG ✗** (`IntoEdgesDirected`, `EdgeIndexable`): copy | NK `flow/Dinic.cpp`; PF `directed/edmonds_karp.rs:53`; MG `python/max_flow.py` | **yes**: GDS online documentation <https://neo4j.com/docs/graph-data-science/current/algorithms/max-flow/>, "based on a parallel push-relabel algorithm" | A deterministic parallel push-relabel is not trivial. Not in the open GDS repository; in the shipped plugin (§6). |
| 157 | Minimum-cost flow | NK `flow/SuccessiveShortestPath.cpp` (MIT): port | NX `flow/networksimplex.py`, `capacityscaling.py`, `mincost.py` | **yes (weak evidence)**: GDS online documentation <https://neo4j.com/docs/graph-data-science/current/algorithms/min-cost-max-flow/>: `concurrency` "number of concurrent threads"; "cost-scaling push-relabel" | As 155. |

### Matching, covering, cliques and colouring

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 161 | Maximum and maximal matching | PG `maximum_matching` (`matching.rs:373`), `greedy_matching` (`:211`), PG ✓ (undirected): call | NX `matching.py` | no | — |
| 163 | Bipartite matching | MG `cpp/bipartite_matching_module/algorithm/bipartite_matching.cpp`: port; `hopcroft-karp` 0.2.1 (MIT) | PF `kuhn_munkres.rs:117,259` (minimum-weight full matching); NX `bipartite/matching.py` | no: MG absent from the thread `grep` | — |
| 169 | Maximal cliques | PG `maximal_cliques` (`maximal_cliques.rs:115`), PG ✓: call | PF `undirected/cliques.rs` | no | — |
| 171 | Clique counting | No permissive source found | — | **yes**: GDS online documentation <https://neo4j.com/docs/graph-data-science/current/algorithms/clique-counting/>: `concurrency` "number of concurrent threads" | Written from scratch (enumeration per block of root nodes). Not in the open GDS repository; in the shipped plugin (§6). |
| 173 | Greedy colouring | RWX `coloring.rs:106` (`ColoringStrategy::{Degree, Saturation, IndependentSet}`) | NX `coloring/greedy_coloring.py` (all 7 strategies) | **undecided**: MG `graph_coloring` runs a quantum-annealing-style metaheuristic with `multiprocessing` (`python/mage/graph_coloring_module/algorithms/meta_heuristics/parallel_algorithm.py:9,55`); the NX strategies are sequential | The literal rule says yes, but the MAGE algorithm is not greedy. Needs a user decision (plan §3.2 item B). |
| 174 | DSatur | PG `dsatur_coloring` (`coloring.rs:63`), PG ✓: call | — | no | — |
| 176 | K-1 colouring | No permissive source found | GDS `k1coloring/K1Coloring.java` (study); LA `LAGraph_coloring_independent_set.c` (method) | **yes**: GDS `k1coloring/K1Coloring.java:173-180` | Written from scratch: speculative colouring and conflict-repair rounds. |
| 177 | Greedy feedback arc set | PG `feedback_arc_set.rs:73`, **PG ✗** (bound `u32: GraphIndex`): copy | — | no | — |
| 178 | Set cover | MG `python/set_cover.py` and `python/mage/constraint_programming/solver.py` (greedy): port | — | no | The MAGE constraint-programming variant uses GEKKO; only the greedy one is ported. |

### Cycles, tours and routing

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 180 | Cycle detection | PG `is_cyclic_directed`, `is_cyclic_undirected` (`mod.rs:284,170`), PG ✓: call | RWX `connectivity/find_cycle.rs` (returns the cycle); MG `cpp/cycles_module/algorithm/cycles.cpp` | no | — |
| 181 | Simple-cycle enumeration | GA `elementary_circuits.rs:71`: copy | RWX `connectivity/johnson_simple_cycles.rs` | no: GA | Exponential output. |
| 187 | Travelling salesman | MG `python/mage/geography/travelling_salesman.py` (2-approximation, 1.5-approximation, greedy): port | NX `approximation/traveling_salesman.py`; GN `approximation/tsp.rs` | no: MG Python is sequential | MG builds a dense distance matrix from coordinates. |
| 188 | Vehicle routing | No portable source: MG `python/vrp.py` uses the GEKKO MINLP solver (`constraint_programming/vrp_cp_solver.py`) | `vrp-core` 1.25.0 (Apache-2.0, large) | no: MG is sequential | Needs a user decision on the algorithm (plan §3.2 item D). |

### Isomorphism and graph recognition

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 189 | Graph isomorphism (VF2) | PG `isomorphism.rs:801,827`, **PG ✗** (`IntoNeighborsDirected`): copy, or call on a petgraph `Graph` built from the projection | RWX `src/isomorphism/vf2.rs` (pyo3); `vf2` crate | no | VF2++ (NX `isomorphism/vf2pp.py`) exists only in NetworkX. |
| 190 | Subgraph isomorphism and monomorphism | PG `isomorphism.rs:894,920,959`, PG ✗ | as 189 | no | — |
| 194 | Bipartiteness | PG `is_bipartite_undirected` (`mod.rs:557`), PG ✓: call | RWX `coloring.rs` `two_color` (returns the sets) | no | — |
| 203 | Tournament tests | GA `tournament.rs:36,121`: copy | NX `tournament.py` | no | — |
| 204 | Degree-sequence realisability | GA `spec/mod.rs:29`: copy | NX `graphical.py` | no | — |

### Derived structures

| ALG | Name | Best copy or port source | Fallback | Par. and reference | Notes |
|---|---|---|---|---|---|
| 205 | Transitive closure | PG `tred.rs:120` `dag_transitive_reduction_closure` (takes an adjacency list): call after lpg's topological sort; `dag_to_toposorted_adjacency_list` (`tred.rs:71`) PG ✗: copy | NX `dag.py` | no | petgraph handles DAGs only; NetworkX also handles cyclic graphs. |
| 206 | Transitive reduction | as 205 | RWX `src/dag_algo/mod.rs:675` | no | — |

### Parallel tally

Counted from the 97 rows above:

* **parallel: 41**: 001 006 007 019 023 024 027 029 032 034 035 038 039 040 042 043 044 045 059 061 062 063 064 065 076 077 078 080 081 082 083 093 105 106 120 121 153 155 157 171 176 (076 and 077 by the literal rule only; 043 and 153 depend on plan §3.2 items A and C);
* **undecided: 1**: 173;
* **single-thread: 55**.

## 3. Shared infrastructure

### 3.1 CSR projection: limits of petgraph `Csr`

Measured with a probe crate that depends on `petgraph = "=0.8.3"` with default features, built with `cargo check --offline` (build directory deleted after the run). Each petgraph algorithm was called on `Csr<(), f64 or u64, Directed or Undirected, u32>`.

* **Compile on `Csr`:** `astar`, `bellman_ford`, `find_negative_cycle`, `dsatur_coloring`, `articulation_points`, `bridges`, `maximal_cliques`, `dijkstra`, `with_dynamic_goal`, `k_shortest_path`, `greedy_matching`, `maximum_matching`, `connected_components`, `is_cyclic_undirected`, `is_cyclic_directed`, `has_path_connecting`, `is_bipartite_undirected`, `page_rank`, `min_spanning_tree`, `min_spanning_tree_prim`, `tarjan_scc`, `dominators::simple_fast`, `johnson`, `spfa`, `floyd_warshall`, `floyd_warshall_path`.
* **Do not compile on `Csr`:** `bidirectional_dijkstra` (`IntoEdgesDirected`); `toposort`, `kosaraju_scc` and `scc`, `all_simple_paths`, `is_isomorphic` and `is_isomorphic_subgraph`, `tred::dag_to_toposorted_adjacency_list` (`IntoNeighborsDirected`); `dinics` and `ford_fulkerson` (`IntoEdgesDirected`, `EdgeIndexable`, `DataMap`); `greedy_feedback_arc_set` (`u32: GraphIndex`). Not callable by signature: `condensation` (owned `Graph`, `mod.rs:477`) and `steiner_tree` (`&UnGraph`, `steiner_tree.rs:179`).
* `Csr` has no incoming-edge access. Its trait implementations are `IntoEdgeReferences`, `IntoEdges`, `Visitable`, `IntoNeighbors`, `NodeIndexable`, `NodeCompactIndexable`, `IntoNodeIdentifiers`, `NodeCount`, `EdgeCount`, `GraphProp`, `IntoNodeReferences` and `GetAdjacencyMatrix` (`csr.rs:547-860`).
* **Parallel edges:** `Csr::from_sorted_edges` rejects a repeated `(source, target)` pair (`csr.rs:243` requires `m > last_target`), and `add_edge` returns `false` for an existing edge (`csr.rs:363-364`).
* **Undirected graphs:** `from_sorted_edges` does not mirror edges (`csr.rs:249-252` push only the given edge); `add_edge` does (`csr.rs:340-345`). An undirected projection must supply both directions, sorted.
* **lpg's adjacency tree (verified 2026-10-12):** `create_edge` writes one entry per direction, `(src, OUT, dst)` and `(dst, IN, src)` (`src/storage/graph.rs:529`), keyed `(node, direction, edge type, neighbour, edge id)` (`adj_key`, `graph.rs:125`). One ordered scan therefore yields, node by node, the outgoing row and then the incoming row, each sorted by type and neighbour. Parallel edges are distinct keys (the edge id), so the graph is a multigraph.
* **Consequence.** TS-25's "one `Csr` and petgraph" does not hold for about 10 entries, nor for a multigraph. The simplest design consistent with the evidence: lpg builds its own forward and reverse CSR arrays (the `row` and `column` layout of `Csr`, `csr.rs`) in the one scan, keeps parallel edges, and implements petgraph's visit traits, including `IntoNeighborsDirected` and `IntoEdgesDirected`, on a thin wrapper, so that petgraph functions compile unchanged. Not built or measured; this changes TS-25 (plan §3.1).

### 3.2 Block scheduler and parallel frontier

* PG `parallel_page_rank` (`page_rank.rs:133` onwards) and `parallel_johnson` (`johnson.rs:203` onwards): the per-node work and the reductions to re-host on `std::thread::scope` blocks.
* KZ `src/function/gds/frontier_morsel.cpp`, `gds_frontier.cpp`, `gds_task.cpp` (MIT): morsel dispatch over a dense and sparse frontier pair and level-synchronous BFS. Port the dense and sparse frontier switch. Kùzu dispatches morsels dynamically; lpg keeps the block-index order for merges (TS-25).
* RWX `centrality.rs:110` (`CondIterator` with `parallel_threshold`): the per-source decomposition of betweenness and closeness, the same shape as lpg's blocks.
* NK OpenMP loops (`community/PLM.cpp`, `PLP.cpp`, `centrality/CoreDecomposition.cpp`) are asynchronous; port them only after converting them to synchronous rounds.

### 3.3 Deterministic generator per block

* rand 0.10.3 re-exports `ChaCha8Rng`, `ChaCha12Rng` and `ChaCha20Rng` (`rand-0.10.3/src/rngs/mod.rs:116`) **only with its feature `chacha`**, which is not a default feature (`Cargo.toml` `[features]`; verified 2026-10-12, §6). chacha20 0.10.2, already in lpg's tree through rand's default `std_rng` feature, has `set_stream(u64)` (`chacha20-0.10.2/src/rng.rs:263`). `ChaCha8Rng::seed_from_u64(seed)` followed by `set_stream(block_index)` gives one independent, portable stream per block.
* Do not use `StdRng`: its documentation states "Non-portable: any future library version may replace the algorithm" and "even with a fixed seed, output is not portable" (`rand-0.10.3/src/rngs/std.rs:20-37`).
* NP `random_vector.rs` (MIT) already uses rand 0.10 for Leiden's random order.

### 3.4 Dense linear algebra (nalgebra)

* GA `spec/laplacian.rs:109` builds the Laplacian as a `DMatrix` (ALG-143); `spec/st.rs:79-86` uses `determinant()` (ALG-151, `f64` cast to `usize`); `shortest_path/seidel.rs` uses `DMatrix` products (ALG-026). GA pins nalgebra 0.33; lpg uses 0.35.
* GN `community/spectral.rs` uses a nalgebra `DMatrix` (ALG-076).
* nalgebra 0.35.0 provides `SymmetricEigen::new` and `try_new` (`src/linalg/symmetric_eigen.rs:60,79`) and `determinant` (`src/linalg/determinant.rs:16`). Both compile and give exact results on the K3 Laplacian with `default-features = false, features = ["std"]` (verified 2026-10-12, §6). The GA and GN code itself was not compiled against 0.35.

### 3.5 Test oracles

* NX tests (BSD-3-Clause; expected values may be copied with attribution): `networkx/algorithms/**/tests/test_*.py` for every entry with an N source. The laboratory venv `/tmp/lpg-lab-venv` runs NetworkX 3.7 with numpy 2.5.4 and scipy 1.18.1, which `pagerank`, `hits` and the Laplacian functions import (`pagerank_alg.py:116,459`, `hits_alg.py:147`, `laplacianmatrix.py:123`).
* PG tests (MIT OR Apache-2.0): `tests/*.rs` per algorithm (`articulation_points.rs`, `coloring.rs`, `dinics.rs`, `floyd_warshall.rs`, `johnson.rs`, `k_shortest_path.rs`, `matching.rs`, `maximal_cliques.rs`, `page_rank.rs`, `spfa.rs`, `steiner_trees.rs`, `iso.rs`, among others) and the graph files `tests/res/graph_{100n_100e,1000n_1000e}{,_iso}.txt`.
* RWX: inline `#[cfg(test)]` modules (for example `centrality.rs:1384` onwards) and generators, including `generators/karate_club.rs` and `petersen_graph.rs` (Apache-2.0), usable as fixtures.
* GA: inline tests (`spec/st.rs:97,129`, `laplacian.rs:150,184`).
* GDS: the documentation examples (`doc/modules/ROOT/pages/algorithms/*.adoc`) are GPL text and are not copied. Expected values for GDS-named entries are produced by running GDS 2026.09.0 in Docker on lpg's test graphs (§6); an output of a run is a measurement, not a copy.

## 4. Proposed families

"Copy share" is the share of entries whose core code comes from a permissive source (call, copy or port). It is an **estimate from this map, not a measurement**.

| Group | Inventory families and entries | Shared kernels and tests | Copy share (estimate) |
|---|---|---|---|
| Projection and runtime | — | forward and reverse CSR, identifier map, memory reservation, block scheduler, per-block ChaCha streams, parallel BFS frontier | low (KZ frontier port, PG `Csr` layout) |
| Traversal and paths | Traversal (9) and shortest paths (17): 26 | BFS and Dijkstra kernels; PG and NX tests | high: about 22 call or copy (PG, RWX, GA, PF, MG); new: 023 delta-stepping and the parallel 019, 027, 029, 032 |
| Centrality and similarity | Centrality (11) and similarity and link prediction (9): 20 | power iteration (040, 042, 043, 044, 045); per-source BFS (035, 038, 039); neighbour-set operations (105, 112–117) | medium: about 13 copy or port; new: 044, 059, 107, 113, 116, 117 |
| Community and clustering | Community (11) | local-move kernel (061, 062, 065); label propagation (063, 064); k-means (080, reused by 076 and 077) | medium: 061, 062, 063, 080, 081, 082 copy or port; new: 064, 065, 078; 076 and 077 through nalgebra |
| Components and structure | Components (8) and structural measures (5): 13 | union-find, DFS low-link, triangle intersection | high: about 11 call or copy; new: parallel WCC and the triangle kernel |
| Trees, flows, matching and colouring | Spanning trees and flows (7) and matching and colouring (9): 16 | spanning trees, augmenting paths, colouring | medium: about 10 copy; new: 149, 154, 171, 176 and the parallel 155 and 157 |
| Cycles, isomorphism and derived structures | Cycles and routing (4), isomorphism and recognition (5), derived (2): 11 | DFS cycles, VF2, topological sort | high, except 188 (no portable source) |

Total: 26 + 20 + 11 + 13 + 16 + 11 = 97.

Entries with no permissive source, to be written over petgraph's visit traits: 023 (method ported from LA only), 044, 059, 064, 065, 078, 107, 113, 116, 117, 149, 154, 171, 176, and 188 (no suitable source).

## 5. Open points

1. GDS HDBSCAN, maximum flow, minimum-cost flow and clique counting are classified from the online documentation only. The wording "number of concurrent threads" is the non-sequential documentation partial, not proof of a parallel implementation; only maximum flow says "parallel push-relabel". Their procedures run in the shipped plugin (§6), which does not show whether they are parallel.
2. Whether a GPU (cuGraph) implementation counts as a "parallel version" under TS-25. ALG-076 and ALG-077 depend on it; ALG-043 is parallel through GDS PageRank either way. Needs a user decision (plan §3.2 item A).
3. ALG-173: MAGE `graph_coloring` is a parallel metaheuristic, not a greedy algorithm. Needs a user decision (plan §3.2 item B).
4. ALG-153: GDS's Steiner tree is rooted and directed; petgraph, RWX and NX are undirected. Needs a user decision (plan §3.2 item C).
5. ALG-188: no portable vehicle-routing source; MAGE relies on GEKKO. Needs a user decision (plan §3.2 item D).
6. The code of `hopcroft-karp`, `vf2` and `vrp-core` was not cloned or read; `pcst_fast`, a possible prize-collecting Steiner source, was not checked. (The `kdtree` 0.7.0 licence is closed: §6.)
7. The algorithm modules of grafeo and minigu were not inspected in depth.
8. The MAGE parallel classification rests on a `grep` for `std::thread|std::async|#pragma omp|std::jthread|std::execution::par|thread_pool|ThreadPool` (C++) and `multiprocessing|ThreadPool|concurrent.futures` (Python); a module parallelised by another mechanism would be missed.
9. The GA and GN nalgebra code (written for 0.33) was not compiled against 0.35; only the nalgebra 0.35 functions it calls were (§3.4).
10. The forward and reverse CSR wrapper (§3.1) was not built or measured.

## 6. Verified during planning (2026-10-12)

| Point | Evidence |
|---|---|
| `kdtree` 0.7.0 licence | crates.io API `crates/kdtree/0.7.0`: `MIT OR Apache-2.0`. |
| GDS plugin runs; procedures absent from the open repository are present | `docker run -e NEO4J_AUTH=none -e 'NEO4J_PLUGINS=["graph-data-science"]' neo4j:2026.09.0-community`: the image installs the bundled plugin; `RETURN gds.version()` gives `"2026.09.0"`; `gds.list()` lists 493 procedures, among them `gds.hdbscan.*`, `gds.maxFlow.*`, `gds.maxFlow.minCost.*`, `gds.cliqueCounting.*` and `gds.prizeSteinerTree.*`. |
| GDS without a licence key caps `concurrency` at 4 | `gds.pageRank.stream('g', {concurrency: 8})` failed: "You are currently using an unlicensed GDS and cannot exceed concurrency=4". |
| GDS PageRank is not normalised to sum 1 | On a directed 3-cycle, `gds.pageRank.stream` at `concurrency` 1 returned 0.9612404689154856 for each node; NetworkX normalises PageRank to sum 1. Entries with GDS names follow GDS's values (FR-ALG-007). |
| rand `chacha` feature | `rand-0.10.3/Cargo.toml` `[features]`: `default = ["std", "std_rng", "sys_rng", "thread_rng"]`; `chacha = ["dep:chacha20"]`; the re-export is `#[cfg(feature = "chacha")]` (`src/rngs/mod.rs`). A probe with `features = ["chacha"]` compiled; two generators seeded 0 on stream 7 gave equal first outputs, and stream 8 a different one. |
| nalgebra 0.35 API | Probe with `nalgebra = { version = "=0.35.0", default-features = false, features = ["std"] }`: `SymmetricEigen::new` on the K3 Laplacian gave eigenvalues 3, 0 (−2.2e−16) and 3; the determinant of its reduced Laplacian gave 3 (K3 has 3 spanning trees). |
| NetworkX spanning-tree count is a float | `nx.number_of_spanning_trees(nx.cycle_graph(5))` returned `4.999999999999999` (NetworkX 3.7, scipy 1.18.1). |
| lpg adjacency entries per direction | `src/storage/graph.rs:529` (`create_edge`): `for (node, dir, nbr) in [(src, OUT, dst), (dst, IN, src)]`. |
