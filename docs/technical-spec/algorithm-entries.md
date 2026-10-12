---
title: Graph-algorithm entries
status: draft
last-reviewed: 2026-10-12
related:
  - algorithms.md
  - ../functional-spec/graph-algorithms.md
  - ../evidence/planning/sprint-5-reuse-map.md
---

# Graph-algorithm entries

Per-entry tables of [TS-25](algorithms.md#ts-25-graph-algorithms), one row for each of the 97 entries of the [inventory](../functional-spec/graph-algorithms.md). TS-25 states the rules; this file applies them entry by entry.

## Parallel classification

Rule: TS-25 Parallelism. Evidence per entry: [reuse map](../evidence/planning/sprint-5-reuse-map.md) §2, which gives the full path and line of each reference; GDS paths are relative to `algo/src/main/java/org/neo4j/gds/` of GDS @ `417d7ecf973d`, `doc/` to its documentation sources. "Measured" rows: GDS 2026.09.0 run in `neo4j:2026.09.0-community` on a graph from `gds.graph.generate` (300 000 nodes, power-law degrees, average degree 10; 40 000 nodes for ALG-081), Neo4j JVM CPU time over wall time at `concurrency` 1 and 4 (rmp #62; the PageRank control gave 1.22 and 2.34). The ratio shows that a run used more than one core; it is not a timing of lpg.

**Parallel: 38 entries. Single-thread: 59 entries.**

| ALG | Name | Parallel | Reference |
|---|---|---|---|
| 001 | Breadth-first search | yes | GDS `paths/traverse/BFS.java:210` (`ParallelUtil.run`) |
| 002 | Depth-first search | no | GDS `doc/.../dfs.adoc:7` `:sequential: true` |
| 003 | Neighbourhood at hop | no | MAGE `neighbors` has no thread code; no G, P or A source |
| 004 | Path expansion and reachable subgraph | no | MAGE `path` has no thread code |
| 006 | Random walk | yes | GDS `traversal/RandomWalk*.java` (`RunWithConcurrency`, `ExecutorService`) |
| 007 | Topological sort | yes | GDS `dag/topologicalsort/TopologicalSort.java:99,120` |
| 008 | Ancestors and descendants | no | MAGE `graph_util` has no thread code |
| 009 | Reachability test | no | petgraph `has_path_connecting` is sequential |
| 011 | Dominators | no | petgraph `dominators::simple_fast` is sequential |
| 013 | Unweighted shortest paths | no | Memgraph built-in BFS cursors, no parallel code (`operator.cpp:2477-2745`) |
| 014 | Dijkstra | no | GDS `doc/.../dijkstra-single-source.adoc:9,28` sequential |
| 015 | Bidirectional Dijkstra | no | petgraph `bidirectional_dijkstra` is sequential |
| 018 | A* | no | GDS `doc/.../astar.adoc:33` "executed using a single thread" |
| 019 | Bellman-Ford | yes | GDS `paths/bellmanford/BellmanFord.java:108,115` (`RunWithConcurrency`) |
| 020 | Negative-cycle detection | no | petgraph `find_negative_cycle` is sequential |
| 021 | SPFA | no | petgraph `spfa` is sequential |
| 023 | Delta-stepping | yes | GDS `paths/delta/DeltaStepping.java`; `doc/.../delta-single-source.adoc:48` |
| 024 | Johnson | yes | petgraph `parallel_johnson` (`johnson.rs:203`, feature `rayon`) |
| 025 | Floyd-Warshall | no | petgraph and graphalgs are sequential |
| 026 | Seidel | no | graphalgs has no parallel code |
| 027 | All-pairs shortest distances | yes | GDS `allshortestpaths/MSBFSAllShortestPaths.java:73` |
| 028 | All shortest paths between nodes | no | Memgraph `ExpandAllShortestPathsCursor` (`operator.cpp:3637`), no parallel code |
| 029 | Yen's k shortest paths | yes | GDS `paths/yens/Yens.java:111`; `doc/.../yens.adoc:32` |
| 030 | Length of the k-th shortest path | no | petgraph `k_shortest_path` is sequential |
| 031 | All simple paths | no | petgraph sequential; MAGE `algo` has no thread code |
| 032 | Longest path in a DAG | yes | GDS `dag/longestPath/DagLongestPath.java:95` |
| 034 | Degree centrality | yes | GDS `degree/DegreeCentrality.java:166-172` |
| 035 | Betweenness centrality | yes | GDS `betweenness/*`; MAGE `std::thread` |
| 038 | Closeness centrality | yes | GDS `closeness/ClosenessCentrality.java` |
| 039 | Harmonic centrality | yes | GDS `harmonic/HarmonicCentrality.java` |
| 040 | Eigenvector centrality | yes | GDS `pagerank/EigenvectorComputation.java` run by `PageRankAlgorithm.java` |
| 041 | Katz centrality | no | MAGE `katz_centrality` has no thread code |
| 042 | PageRank | yes | petgraph `parallel_page_rank` (`page_rank.rs:133`); GDS; MAGE |
| 043 | Personalised PageRank | yes | GDS PageRank with `sourceNodes` (`doc/.../page-rank.adoc:574-591`), the same parallel `PageRankAlgorithm` as ALG-042 |
| 044 | ArticleRank | yes | GDS `PageRankAlgorithm.java` |
| 045 | HITS | yes | GDS `hits/Hits.java:55` (Pregel) |
| 059 | Influence maximisation (CELF) | yes | GDS `influenceMaximization/ICLazyForwardMC.java` |
| 061 | Louvain | yes | GDS `louvain/Louvain.java`; MAGE OpenMP |
| 062 | Leiden | yes | GDS `leiden/*`; MAGE `leiden.cpp` (`std::thread`) |
| 063 | Label propagation | yes | GDS `labelpropagation/LabelPropagation.java:118` |
| 064 | SLLPA | yes | GDS `sllpa/*` (Pregel) |
| 065 | Modularity optimisation | yes | GDS `modularityoptimization/ModularityManager.java:61-72` |
| 076 | Spectral clustering | no | only MAGE cuGraph, GPU-only (item A) |
| 077 | Balanced-cut clustering | no | only MAGE cuGraph, GPU-only (item A) |
| 078 | Approximate maximum k-cut | yes | GDS `approxmaxkcut/PlaceNodesRandomly.java:89`; `doc/.../approx-max-k-cut.adoc:51` |
| 080 | K-means | yes | GDS `kmeans/Kmeans.java` (`RunWithConcurrency`) |
| 081 | HDBSCAN | yes | measured on 40 000 nodes: CPU/wall 2.14 at `concurrency` 1, 5.04 at 4, wall time 6.86 s and 1.66 s; GDS documentation, HDBSCAN page; no class in the open source |
| 082 | Partition quality | yes | GDS `modularity/ModularityCalculator.java:82-95`, `conductance/Conductance.java:104-110` |
| 083 | Weakly connected components | yes | GDS `wcc/*`; `doc/.../wcc.adoc:29-30` |
| 084 | Connected components | no | petgraph `connected_components` is sequential |
| 085 | Strongly connected components | no | GDS `scc/Scc.java` has no concurrency; petgraph sequential |
| 086 | Condensation | no | petgraph `condensation` is sequential |
| 089 | Biconnected components | no | MAGE `biconnected_components` has no thread code |
| 090 | Articulation points | no | GDS `doc/.../articulation-points.adoc:7` sequential |
| 091 | Bridges | no | GDS `doc/.../bridges.adoc:7` sequential |
| 093 | k-core decomposition | yes | GDS `kcore/KCoreDecomposition.java:78,119` |
| 105 | Node similarity | yes | GDS `similarity/nodesim/NodeSimilarity.java` (`ExecutorService`) |
| 106 | K-nearest neighbours | yes | GDS `similarity/knn/Knn.java`; MAGE `knn.hpp` |
| 107 | Vector similarity functions | no | scalar functions |
| 112 | Adamic-Adar | no | GDS scalar function |
| 113 | Common neighbours | no | GDS scalar function |
| 114 | Preferential attachment | no | GDS scalar function |
| 115 | Resource allocation | no | GDS scalar function |
| 116 | Same community | no | GDS scalar function |
| 117 | Total neighbours | no | GDS scalar function |
| 120 | Triangle count and listing | yes | GDS `triangle/IntersectingTriangleCount.java` (`ExecutorService`) |
| 121 | Clustering coefficient | yes | GDS `triangle/LocalClusteringCoefficient.java:99` |
| 123 | Eccentricity and related measures | no | graphalgs has no parallel code |
| 128 | Girth | no | graphalgs has no parallel code |
| 143 | Laplacian matrix | no | graphalgs has no parallel code |
| 147 | Minimum spanning tree | no | GDS `doc/.../minimum-weight-spanning-tree.adoc:8,42` |
| 149 | k-spanning tree | no | GDS `doc/.../k-minimum-weight-spanning-tree.adoc:9` sequential |
| 151 | Number of spanning trees | no | graphalgs has no parallel code |
| 153 | Steiner tree | yes | GDS `steiner/ShortestPathsSteinerAlgorithm.java` (`ExecutorService`); `doc/.../directed-steiner-tree.adoc:28` (item C) |
| 154 | Prize-collecting Steiner tree | no | GDS `pricesteiner/` has no concurrency |
| 155 | Maximum flow | yes | GDS documentation, Max Flow page: "based on a parallel push-relabel algorithm"; measured CPU/wall 1.12 at `concurrency` 1, 1.24 at 4 |
| 157 | Minimum-cost flow | no | measured: CPU/wall 1.01 at `concurrency` 1 and at 4 over runs of 150 s and 173 s; the documentation's `concurrency` wording is the generic one (reuse map §5 point 1) |
| 161 | Maximum and maximal matching | no | petgraph matching is sequential |
| 163 | Bipartite matching | no | MAGE `bipartite_matching` has no thread code |
| 169 | Maximal cliques | no | petgraph `maximal_cliques` is sequential |
| 171 | Clique counting | yes | measured: CPU/wall 1.32 at `concurrency` 1, 3.97 at 4; GDS documentation, Clique Counting page |
| 173 | Greedy colouring | no | the NetworkX strategies are sequential; MAGE's metaheuristic is not provided (item B) |
| 174 | DSatur colouring | no | petgraph `dsatur_coloring` is sequential |
| 176 | K-1 colouring | yes | GDS `k1coloring/K1Coloring.java:173-180` |
| 177 | Greedy feedback arc set | no | petgraph `greedy_feedback_arc_set` is sequential |
| 178 | Set cover | no | MAGE `set_cover.py` greedy is sequential |
| 180 | Cycle detection | no | petgraph sequential; MAGE `cycles` has no thread code |
| 181 | Simple-cycle enumeration | no | graphalgs has no parallel code |
| 187 | Travelling salesman | no | MAGE `tsp.py` is sequential |
| 188 | Vehicle routing | no | MAGE `vrp.py` is sequential |
| 189 | Graph isomorphism | no | petgraph VF2 is sequential |
| 190 | Subgraph isomorphism | no | petgraph VF2 is sequential |
| 194 | Bipartiteness | no | petgraph `is_bipartite_undirected` is sequential |
| 203 | Tournament tests | no | graphalgs has no parallel code |
| 204 | Degree-sequence realisability | no | graphalgs has no parallel code |
| 205 | Transitive closure | no | petgraph `tred` is sequential |
| 206 | Transitive reduction | no | petgraph `tred` is sequential |

## Signatures

Rules (TS-25 Names and signatures):

- **GDS rows** reproduce GDS 2026.09.0 as it reports itself: names, modes and yield columns from `CALL gds.list()`; keys and defaults from the `configuration` column of a `.stats`, `.write` or `.mutate` run; required keys from GDS's "mandatory configuration parameter" errors; optional keys without a default from runs that GDS accepted, drawn from the optional keys of GDS's configuration interfaces (rmp #62). GDS's `graphName` argument is dropped; `configuration = {} :: MAP` is the one parameter. Types are GDS's; `MAP` is a GQL `RECORD`. Node columns of type `INTEGER` hold the element identifier ([TS-07](storage.md#ts-07-element-identifiers)).
- **MAGE rows** reproduce MAGE @ `c72636014a8b` (`mgp::Parameter`, `proc_add_arg`, `proc_add_opt_arg`, `@mgp.read_proc`); positional parameters, then `configuration :: RECORD = {}` for the common keys; no write mode (FR-ALG-003).
- **Derived rows** take `configuration = {} :: MAP`; keys are the arguments of the petgraph or graphalgs function, in camelCase.
- **Common keys**, accepted by every procedure and not repeated below: `nodeLabels` (`["*"]`), `relationshipTypes` (`["*"]`), `concurrency` (4), `orientation` (`"NATURAL"`; fixed to `"UNDIRECTED"` on the entries marked U). Entries marked R also take `randomSeed` (0). Entries marked U are defined only for undirected graphs: GDS 2026.09.0 rejects a directed projection for ALG-062, 090, 091, 093, 120, 121, 147, 149, 154 and 171 (measured, rmp #62); graphalgs `seidel` takes an "undirected, connected" graph (`seidel.rs:11`, ALG-026); the inventory names ALG-084 "undirected"; MAGE builds an undirected view for ALG-089, 163 and 180 (`kUndirectedGraph`, `biconnected_components_module.cpp:32`, `bipartite_matching_module.cpp:20`, `cycles_module.cpp:26`) and for ALG-173 (`python/graph_coloring.py:157-158`); petgraph treats the graph as undirected for ALG-161 (`matching.rs:190, 324`), ALG-169 (`maximal_cliques.rs:64`), ALG-174 (`coloring.rs:13`) and ALG-194 (`mod.rs:540`). Accepted with no effect: `jobId`, `logProgress`, `sudo`, `writeConcurrency`, `writeToResultStore`, and the keys marked "no effect".
- **Functions** (ALG-107, ALG-112 to ALG-117) take only the parameters shown; the common keys do not apply to them (TS-25 Orientation). GDS's functions return a bare value; `run_algorithm` returns it in one column: `similarity` for ALG-107 (`distance` for `euclideanDistance`), `score` for ALG-112 to ALG-117 (rmp #65 FINDING 550; `src/algo/functions.rs:61-139`).
- **Domains.** A domain after a default (for example "0.01 to 1") is GDS's; a value outside it fails with 22N11, as GDS rejects it (rmp #71 pass B, test `parameter_domains_as_gds`).
- **Empty projection** (tested on 51 entries). No row from 006, 007, 013, 024 to 027, 032, 034, 035, 038 to 040, 042, 044, 045, 059, 061 to 065, 078, 083 to 086, 089 to 091, 093, 105, 120, 121, 123, 143, 161, 169, 171, 173, 174, 176, 177, 180, 181, 194, 205, 206; one row from 128 (`Infinity`), 163 (0) and 203 (`true, true`) (`tests_trace_a::edge_cases_and_errors`, `tests_trace_c::errors_of_the_pass_c_entries`, the `nil` cases of `src/algo/testdata/expected/trace_b.json` and `trace_c.json`). ALG-169 yields no clique, as NetworkX `find_cliques` (`clique.py:250-251`), not petgraph's empty clique (rmp #71 finding C1). The other entries are not tested on an empty projection.
- **Write modes.** "—" means the entry has no write mode. A node-property write needs `writeProperty`; a relationship write needs `writeRelationshipType`.

Shorthands for repeated column lists, verbatim from `gds.list()`:

- **PathRows**: `(index :: INTEGER, sourceNode :: INTEGER, targetNode :: INTEGER, totalCost :: FLOAT, nodeIds :: LIST<INTEGER>, costs :: LIST<FLOAT>, path :: PATH)`.
- **PathWrite**: `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, relationshipsWritten :: INTEGER, configuration :: MAP)`; keys `writeRelationshipType` (required), `writeNodeIds` (false), `writeCosts` (false).
- **Score**: `(nodeId :: INTEGER, score :: FLOAT)`.
- **CentralityWrite**: `(nodePropertiesWritten :: INTEGER, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, centralityDistribution :: MAP, configuration :: MAP)`.
- **RankWrite**: `(ranIterations :: INTEGER, didConverge :: BOOLEAN, centralityDistribution :: MAP, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP)`.
- **ComponentWrite**: `(componentCount :: INTEGER, componentDistribution :: MAP, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP)`.
- **NodeWrite** (derived entries): `(nodePropertiesWritten :: INTEGER, configuration :: MAP)`. **RelWrite**: `(relationshipsWritten :: INTEGER, configuration :: MAP)`. **EdgePropWrite**: `(relationshipPropertiesWritten :: INTEGER, configuration :: MAP)`.

| ALG | Procedures | Required keys | Optional keys (default) | Stream yields | Write mode |
|---|---|---|---|---|---|
| 001 | `gds.bfs.stream` | `sourceNode` | `targetNodes` (`[]`), `maxDepth` (-1) | `(sourceNode :: INTEGER, nodeIds :: LIST<INTEGER>, path :: PATH)` | — |
| 002 | `gds.dfs.stream` | `sourceNode` | `targetNodes` (`[]`), `maxDepth` (-1) | as 001 | — |
| 003 | `neighbors.at_hop`, `neighbors.by_hop` | `node :: NODE`, `rel_type :: LIST<STRING>`, `distance :: INTEGER` | — | `at_hop`: `(nodes :: NODE)`; `by_hop`: `(nodes :: LIST<NODE>)` | — |
| 004 | `gds.pathExpand.stream`; `gds.pathSubgraphNodes.stream`; `gds.pathSubgraphAll.stream` | `pathExpand`: `start`, `relationships`, `labels`, `minHops`, `maxHops`; the others: `startNode` | the others: `minHops` (0), `maxHops` (unbounded), `relationshipFilter` (`[]`), `labelFilter` (`[]`), `filterStartNode` (true), `beginSequenceAtStart` (true), `bfs` (false) | `(result :: PATH)`; `(nodes :: NODE)`; `(nodes :: LIST<NODE>, rels :: LIST<RELATIONSHIP>)` | — |
| 006 R | `gds.randomWalk.stream` | — | `sourceNodes` (`[]`), `walkLength` (80), `walksPerNode` (10), `returnFactor` (1.0), `inOutFactor` (1.0), `relationshipWeightProperty`, `walkBufferSize` (1000, no effect) | `(nodeIds :: LIST<INTEGER>, path :: PATH)` | — |
| 007 | `gds.dag.topologicalSort.stream` | — | `computeMaxDistanceFromSource` (false) | `(nodeId :: INTEGER, maxDistanceFromSource :: FLOAT)` | — |
| 008 | `graph_util.ancestors`, `graph_util.descendants` | `node :: NODE` | — | `(ancestors :: LIST<NODE>)`; `(descendants :: LIST<NODE>)` | — |
| 009 | `gds.hasPathConnecting.stream` | `sourceNode`, `targetNode` | — | `(sourceNode :: INTEGER, targetNode :: INTEGER, connected :: BOOLEAN)` | — |
| 011 | `gds.dominatorsSimpleFast.stream`, `.write` | `sourceNode` (the root) | — | `(nodeId :: INTEGER, immediateDominator :: INTEGER, dominanceFrontier :: LIST<INTEGER>)` | `writeProperty` ← `immediateDominator`; NodeWrite |
| 013 | `gds.unweightedShortestPaths.stream`, `.write` | — | `sourceNode`, `targetNode` (both: one pair; one of them: single source or target; neither: all pairs) | PathRows | PathWrite |
| 014 | `gds.shortestPath.dijkstra.stream`, `.write`; `gds.allShortestPaths.dijkstra.stream`, `.write` | `sourceNode`; single-pair form also `targetNode` or `targetNodes` | `relationshipWeightProperty` | PathRows | PathWrite |
| 015 | `gds.bidirectionalDijkstra.stream`, `.write` | `sourceNode`, `targetNode` | `relationshipWeightProperty` | PathRows | PathWrite |
| 018 | `gds.shortestPath.astar.stream`, `.write` | `sourceNode`, `targetNode`, `latitudeProperty`, `longitudeProperty` | `relationshipWeightProperty` | PathRows | PathWrite |
| 019 | `gds.bellmanFord.stream`, `.write` | `sourceNode` | `relationshipWeightProperty`; write: `writeNegativeCycles` (false) | `(index :: INTEGER, sourceNode :: INTEGER, targetNode :: INTEGER, totalCost :: FLOAT, nodeIds :: LIST<INTEGER>, costs :: LIST<FLOAT>, route :: PATH, isNegativeCycle :: BOOLEAN)` | keys as PathWrite; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, postProcessingMillis :: INTEGER, relationshipsWritten :: INTEGER, containsNegativeCycle :: BOOLEAN, configuration :: MAP)` |
| 020 | `gds.findNegativeCycle.stream` | `sourceNode` | `relationshipWeightProperty` | `(nodeIds :: LIST<INTEGER>, path :: PATH)`, no row without a negative cycle | — |
| 021 | `gds.spfa.stream`, `.write` | `sourceNode` | `relationshipWeightProperty` | PathRows | PathWrite |
| 023 | `gds.allShortestPaths.delta.stream`, `.write` | `sourceNode` | `delta` (2.0), `relationshipWeightProperty` | PathRows | PathWrite |
| 024 | `gds.johnson.stream` | — | `relationshipWeightProperty` | `(sourceNodeId :: INTEGER, targetNodeId :: INTEGER, distance :: FLOAT)` | — |
| 025 | `gds.floydWarshall.stream` | — | `relationshipWeightProperty` | as 024 | — |
| 026 U | `gds.seidel.stream` | — | — | as 024; pairs in different components have no row | — |
| 027 | `gds.allShortestPaths.stream` | — | `relationshipWeightProperty` | `(sourceNodeId :: INTEGER, targetNodeId :: INTEGER, distance :: FLOAT)` | — |
| 028 | `gds.allShortestPathsBetweenNodes.stream`, `.write` | `sourceNode`, `targetNode` | `relationshipWeightProperty` | PathRows | PathWrite |
| 029 | `gds.shortestPath.yens.stream`, `.write` | `sourceNode`, `targetNode`, `k` | `relationshipWeightProperty` | PathRows | PathWrite |
| 030 | `gds.kShortestPath.stream`, `.write` | `sourceNode`, `k` | `targetNode`, `relationshipWeightProperty` | `(nodeId :: INTEGER, cost :: FLOAT)` | NodeWrite |
| 031 | `algo.all_simple_paths` | `start_node :: NODE`, `end_node :: NODE`, `relationship_types :: LIST<STRING>`, `max_length :: INTEGER` | — | `(path :: PATH)` | — |
| 032 | `gds.dag.longestPath.stream` | — | `relationshipWeightProperty` | PathRows | — |
| 034 | `gds.degree.stream`, `.write` | — | `relationshipWeightProperty`; `orientation` is GDS's own key here | Score | CentralityWrite |
| 035 | `gds.betweenness.stream`, `.write` | — | `samplingSize` (all nodes), `samplingSeed` (0), `relationshipWeightProperty` | Score | CentralityWrite |
| 038 | `gds.closeness.stream`, `.write` | — | `useWassermanFaust` (false) | Score | CentralityWrite |
| 039 | `gds.closeness.harmonic.stream`, `.write` | — | — | Score | CentralityWrite |
| 040 | `gds.eigenvector.stream`, `.write` | — | `maxIterations` (20), `tolerance` (1.0E-7), `sourceNodes` (`[]`), `scaler` (`"NONE"`), `relationshipWeightProperty` | Score | RankWrite |
| 041 | `katz_centrality.get` | — | `alpha :: FLOAT` (0.2), `epsilon :: FLOAT` (0.01) | `(node :: NODE, rank :: FLOAT)` | — |
| 042 | `gds.pageRank.stream`, `.write` | — | as 040, and `dampingFactor` (0.85) | Score | RankWrite |
| 043 | `gds.pageRank.stream`, `.write` (the procedures of ALG-042, FR-ALG-007) | — | as 042; a non-empty `sourceNodes` makes the run personalised | Score | RankWrite |
| 044 | `gds.articleRank.stream`, `.write` | — | as 042 | Score | RankWrite |
| 045 | `gds.hits.stream`, `.write` | — | `hitsIterations` (20), `authProperty` (`"auth"`; no whitespace), `hubProperty` (`"hub"`; no whitespace), `relationshipWeightProperty`, `partitioning` (`"AUTO"`; `"RANGE"`, `"DEGREE"` or `"AUTO"`; no effect), `mutateProperty` (no effect) | `(nodeId :: INTEGER, values :: MAP)` | writes `authProperty` and `hubProperty`; `(ranIterations :: INTEGER, didConverge :: BOOLEAN, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP)` |
| 059 R | `gds.influenceMaximization.celf.stream`, `.write` | `seedSetSize` | `monteCarloSimulations` (100), `propagationProbability` (0.1; 0.01 to 1) | `(nodeId :: INTEGER, spread :: FLOAT)` | `(writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, computeMillis :: INTEGER, totalSpread :: FLOAT, nodeCount :: INTEGER, configuration :: MAP)` |
| 061 | `gds.louvain.stream`, `.write` | — | `maxIterations` (10), `maxLevels` (10), `tolerance` (1.0E-4), `includeIntermediateCommunities` (false; not with `consecutiveIds` true), `seedProperty` (null), `consecutiveIds` (false), `minCommunitySize`, `forceSeedOptimization` (false, no effect), `relationshipWeightProperty` | `(nodeId :: INTEGER, intermediateCommunityIds :: LIST<INTEGER>, communityId :: INTEGER)` | `(modularity :: FLOAT, modularities :: LIST<FLOAT>, ranLevels :: INTEGER, communityCount :: INTEGER, communityDistribution :: MAP, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP)` |
| 062 U R | `gds.leiden.stream`, `.write` | — | `maxLevels` (10), `gamma` (1.0), `theta` (0.01), `tolerance` (1.0E-4), `includeIntermediateCommunities` (false; not with `consecutiveIds` true), `seedProperty` (null), `consecutiveIds` (false), `minCommunitySize`, `forceSeedOptimization` (false, no effect), `relationshipWeightProperty` | as 061 | `(ranLevels :: INTEGER, didConverge :: BOOLEAN, nodeCount :: INTEGER, communityCount :: INTEGER, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, communityDistribution :: MAP, modularities :: LIST<FLOAT>, modularity :: FLOAT, configuration :: MAP)` |
| 063 | `gds.labelPropagation.stream`, `.write` | — | `maxIterations` (10), `nodeWeightProperty` (null), `seedProperty` (null), `consecutiveIds` (false), `minCommunitySize`, `forceSeedOptimization` (false, no effect), `relationshipWeightProperty` | `(nodeId :: INTEGER, communityId :: INTEGER)` | `(ranIterations :: INTEGER, didConverge :: BOOLEAN, communityCount :: INTEGER, communityDistribution :: MAP, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP)` |
| 064 R | `gds.sllpa.stream`, `.write` | `maxIterations` | `minAssociationStrength` (0.2), `partitioning` (`"RANGE"`; `"RANGE"`, `"DEGREE"` or `"AUTO"`; no effect), `mutateProperty` (no effect) | `(nodeId :: INTEGER, values :: MAP)` | `writeProperty` (GDS's default `""` fails); `(ranIterations :: INTEGER, didConverge :: BOOLEAN, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP)` |
| 065 | `gds.modularityOptimization.stream`, `.write` | — | `maxIterations` (10), `tolerance` (1.0E-4), `seedProperty` (null), `consecutiveIds` (false), `minCommunitySize`, `forceSeedOptimization` (false, no effect), `batchSize` (10000, no effect), `relationshipWeightProperty` | `(nodeId :: INTEGER, communityId :: INTEGER)` | `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodes :: INTEGER, didConverge :: BOOLEAN, ranIterations :: INTEGER, modularity :: FLOAT, communityCount :: INTEGER, communityDistribution :: MAP, configuration :: MAP)` |
| 076 R | `cugraph.spectral_clustering.get` | `num_clusters :: INTEGER` | `num_eigenvectors :: INTEGER` (2), `ev_tolerance :: FLOAT` (0.00001, no effect), `ev_max_iter :: INTEGER` (100, no effect), `kmean_tolerance :: FLOAT` (0.00001), `kmean_max_iter :: INTEGER` (20), `weight_property :: STRING` (`"weight"`) | `(node :: NODE, cluster :: INTEGER)` | — |
| 077 R | `cugraph.balanced_cut_clustering.get` | as 076 | as 076, except `kmean_max_iter` (100) | as 076 | — |
| 078 R | `gds.maxkcut.stream` | — | `k` (2; at most 127), `iterations` (8), `vnsMaxNeighborhoodOrder` (0), `minCommunitySize`, `relationshipWeightProperty` | `(nodeId :: INTEGER, communityId :: INTEGER)` | — |
| 080 R | `gds.kmeans.stream`, `.write` | `nodeProperty` | `k` (10), `maxIterations` (10), `deltaThreshold` (0.05), `numberOfRestarts` (1), `initialSampler` (`"UNIFORM"`), `seedCentroids` (`[]`), `computeSilhouette` (false) | `(nodeId :: INTEGER, communityId :: INTEGER, distanceFromCentroid :: FLOAT, silhouette :: FLOAT)` | `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, communityDistribution :: MAP, centroids :: LIST<LIST<FLOAT>>, averageDistanceToCentroid :: FLOAT, averageSilhouette :: FLOAT, configuration :: MAP)` |
| 081 | `gds.hdbscan.stream`, `.write` | `nodeProperty` | `minClusterSize` (5), `samples` (10), `leafSize` (1, no effect) | `(nodeId :: INTEGER, label :: INTEGER)` | `(nodeCount :: INTEGER, numberOfClusters :: INTEGER, numberOfNoisePoints :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, configuration :: MAP)` |
| 082 | `gds.modularity.stream`; `gds.conductance.stream` | `communityProperty` | `relationshipWeightProperty` | `(communityId :: INTEGER, modularity :: FLOAT)`; `(community :: INTEGER, conductance :: FLOAT)` | — |
| 083 | `gds.wcc.stream`, `.write` | — | `seedProperty` (null), `threshold` (0.0; needs `relationshipWeightProperty`), `consecutiveIds` (false), `forceSeedOptimization` (false, no effect), `relationshipWeightProperty` | `(nodeId :: INTEGER, componentId :: INTEGER)` | ComponentWrite |
| 084 U | `gds.connectedComponents.stream`, `.write` | — | — | `(nodeId :: INTEGER, componentId :: INTEGER)` | NodeWrite |
| 085 | `gds.scc.stream`, `.write` | — | `consecutiveIds` (false) | `(nodeId :: INTEGER, componentId :: INTEGER)` | ComponentWrite |
| 086 | `gds.condensation.stream`, `.write` | — | — | `(componentId :: INTEGER, nodeIds :: LIST<INTEGER>, successorComponentIds :: LIST<INTEGER>)` | `writeProperty` ← `componentId`; NodeWrite |
| 089 U | `biconnected_components.get` | — | — | `(bcc_id :: INTEGER, node_from :: NODE, node_to :: NODE)` | — |
| 090 U | `gds.articulationPoints.stream`, `.write` | — | — | `(nodeId :: INTEGER, resultingComponents :: MAP)` | `(articulationPointCount :: INTEGER, nodePropertiesWritten :: INTEGER, writeMillis :: INTEGER, computeMillis :: INTEGER, configuration :: MAP)` |
| 091 U | `gds.bridges.stream` | — | — | `(from :: INTEGER, to :: INTEGER, remainingSizes :: LIST<INTEGER>)` | — |
| 093 U | `gds.kcore.stream`, `.write` | — | — | `(nodeId :: INTEGER, coreValue :: INTEGER)` | `(nodePropertiesWritten :: INTEGER, degeneracy :: INTEGER, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, postProcessingMillis :: INTEGER, writeMillis :: INTEGER, configuration :: MAP)` |
| 105 | `gds.nodeSimilarity.stream`, `.write`; `gds.nodeSimilarity.filtered.stream`, `.write` | — | `topK` (10), `bottomK` (10), `topN` (0), `bottomN` (0), `similarityCutoff` (1.0E-42; 0 to 1), `degreeCutoff` (1), `upperDegreeCutoff` (2147483647), `similarityMetric` (`"JACCARD"`), `useComponents` (false), `relationshipWeightProperty`; filtered: `sourceNodeFilter`, `targetNodeFilter` (no filter) | `(node1 :: INTEGER, node2 :: INTEGER, similarity :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, postProcessingMillis :: INTEGER, nodesCompared :: INTEGER, relationshipsWritten :: INTEGER, similarityDistribution :: MAP, configuration :: MAP)` |
| 106 R | `gds.knn.stream`, `.write`; `gds.knn.filtered.stream`, `.write` | `nodeProperties` | `topK` (10), `sampleRate` (0.5; above 0, at most 1), `deltaThreshold` (0.001), `maxIterations` (100), `randomJoins` (10), `initialSampler` (`"UNIFORM"`), `perturbationRate` (0.0), `similarityCutoff` (0.0); filtered: `sourceNodeFilter`, `targetNodeFilter` (no filter), `seedTargetNodes` (false, no effect) | `(node1 :: INTEGER, node2 :: INTEGER, similarity :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, postProcessingMillis :: INTEGER, nodesCompared :: INTEGER, relationshipsWritten :: INTEGER, didConverge :: BOOLEAN, ranIterations :: INTEGER, nodePairsConsidered :: INTEGER, similarityDistribution :: MAP, configuration :: MAP)` |
| 107 | functions `gds.similarity.cosine`, `.euclidean`, `.euclideanDistance`, `.jaccard`, `.overlap`, `.pearson` | `vector1 :: LIST<INTEGER \| FLOAT>`, `vector2 :: LIST<INTEGER \| FLOAT>` | — | `:: FLOAT` | — |
| 112 | function `gds.linkprediction.adamicAdar` | `node1 :: NODE`, `node2 :: NODE` | `config = {} :: MAP`: `relationshipQuery` (all types), `direction` (`"BOTH"`) | `:: FLOAT` | — |
| 113 | function `gds.linkprediction.commonNeighbors` | as 112 | as 112 | `:: FLOAT` | — |
| 114 | function `gds.linkprediction.preferentialAttachment` | as 112 | as 112 | `:: FLOAT` | — |
| 115 | function `gds.linkprediction.resourceAllocation` | as 112 | as 112 | `:: FLOAT` | — |
| 116 | function `gds.linkprediction.sameCommunity` | `node1 :: NODE`, `node2 :: NODE` | `communityProperty = "community" :: STRING` | `:: FLOAT` | — |
| 117 | function `gds.linkprediction.totalNeighbors` | as 112 | as 112 | `:: FLOAT` | — |
| 120 U | `gds.triangleCount.stream`, `.write`; `gds.triangles` | — | `maxDegree` (9223372036854775807), `labelFilter` (`[]`; a non-empty list fails with 22N11, rmp #67 FINDING 558; GDS's behaviour not measured) | `(nodeId :: INTEGER, triangleCount :: INTEGER)`; `gds.triangles`: `(nodeA :: INTEGER, nodeB :: INTEGER, nodeC :: INTEGER)` | `(globalTriangleCount :: INTEGER, nodeCount :: INTEGER, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP, postProcessingMillis :: INTEGER)` |
| 121 U | `gds.localClusteringCoefficient.stream`, `.write` | — | `triangleCountProperty` (null), `forceSeedOptimization` (false, no effect) | `(nodeId :: INTEGER, localClusteringCoefficient :: FLOAT)` | `(averageClusteringCoefficient :: FLOAT, nodeCount :: INTEGER, preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, configuration :: MAP, postProcessingMillis :: INTEGER)` |
| 123 | `gds.eccentricity.stream`, `.write` | — | `relationshipWeightProperty` (selects the `weighted_` forms) | `(nodeId :: INTEGER, eccentricity :: FLOAT)`; unreachable nodes give `Infinity` | `(nodePropertiesWritten :: INTEGER, diameter :: FLOAT, radius :: FLOAT, configuration :: MAP)` |
| 128 | `gds.girth.stream` | — | — | `(girth :: FLOAT)`; `Infinity` without a cycle | — |
| 143 | `gds.laplacianMatrix.stream` | — | `relationshipWeightProperty` | `(nodeId :: INTEGER, values :: LIST<FLOAT>)`, rows and columns in ascending node identifier | — |
| 147 U | `gds.spanningTree.stream`, `.write` | `sourceNode` | `objective` (`"minimum"`), `relationshipWeightProperty` | `(nodeId :: INTEGER, parentId :: INTEGER, weight :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, effectiveNodeCount :: INTEGER, relationshipsWritten :: INTEGER, totalWeight :: FLOAT, configuration :: MAP)` |
| 149 U | `gds.kSpanningTree.write` (no stream mode) | `sourceNode`, `k`, `writeProperty` | `objective` (`"minimum"`), `relationshipWeightProperty` | — | `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, postProcessingMillis :: INTEGER, effectiveNodeCount :: INTEGER, configuration :: MAP)` |
| 151 | `gds.countSpanningTrees.stream` | `rootNode` unless `orientation` is `"UNDIRECTED"` | — | `(count :: INTEGER)`; a count above the `INTEGER` range fails with 22003 | — |
| 153 | `gds.steinerTree.stream`, `.write` | `sourceNode` | `targetNodes` (`[]`), `delta` (2.0, no effect), `applyRerouting` (false), `relationshipWeightProperty` | `(nodeId :: INTEGER, parentId :: INTEGER, weight :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, effectiveNodeCount :: INTEGER, effectiveTargetNodesCount :: INTEGER, totalWeight :: FLOAT, relationshipsWritten :: INTEGER, configuration :: MAP)` |
| 154 U | `gds.prizeSteinerTree.stream`, `.write` | `prizeProperty` | `relationshipWeightProperty` | `(nodeId :: INTEGER, parentId :: INTEGER, weight :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, effectiveNodeCount :: INTEGER, totalWeight :: FLOAT, sumOfPrizes :: FLOAT, relationshipsWritten :: INTEGER, configuration :: MAP)` |
| 155 | `gds.maxFlow.stream`, `.write` | `sourceNodes`, `targetNodes` | `capacityProperty` (absent: total flow 0, measured) | `(source :: INTEGER, target :: INTEGER, flow :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, relationshipsWritten :: INTEGER, totalFlow :: FLOAT, configuration :: MAP)` |
| 157 | `gds.maxFlow.minCost.stream`, `.write` | `sourceNodes`, `targetNodes` | `capacityProperty`, `costProperty`, `alpha` (6, no effect) | `(source :: INTEGER, target :: INTEGER, flow :: FLOAT)` | `writeRelationshipType`, `writeProperty`; `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, relationshipsWritten :: INTEGER, totalFlow :: FLOAT, totalCost :: FLOAT, configuration :: MAP)` |
| 161 U | `gds.maximumMatching.stream`, `.write` | — | `greedy` (false: maximum matching; true: petgraph `greedy_matching`) | `(node1 :: INTEGER, node2 :: INTEGER)` | RelWrite |
| 163 U | `bipartite_matching.max` | — | — | `(maximum_bipartite_matching :: INTEGER)` | — |
| 169 U | `gds.maximalCliques.stream` | — | — | `(cliqueId :: INTEGER, nodeIds :: LIST<INTEGER>)` | — |
| 171 U | `gds.cliqueCounting.stream`, `.write` | — | — | `(nodeId :: INTEGER, counts :: LIST<INTEGER>)` | `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, nodePropertiesWritten :: INTEGER, globalCount :: LIST<INTEGER>, configuration :: MAP)` |
| 173 U R | `graph_coloring.color_graph` | — | `parameters :: MAP` (`{}`): `strategy` (`"largest_first"`, the NetworkX strategy names); any MAGE metaheuristic key fails with 22N11; `edge_property :: STRING` (`"weight"`, no effect) | `(node :: NODE, color :: INTEGER)` | — |
| 174 U | `gds.dsaturColoring.stream`, `.write` | — | — | `(nodeId :: INTEGER, color :: INTEGER)` | `(nodePropertiesWritten :: INTEGER, colorCount :: INTEGER, configuration :: MAP)` |
| 176 | `gds.k1coloring.stream`, `.write` | — | `maxIterations` (10, no effect), `minCommunitySize`, `batchSize` (10000, no effect) | `(nodeId :: INTEGER, color :: INTEGER)` | `(preProcessingMillis :: INTEGER, computeMillis :: INTEGER, writeMillis :: INTEGER, nodeCount :: INTEGER, colorCount :: INTEGER, ranIterations :: INTEGER, didConverge :: BOOLEAN, configuration :: MAP)` |
| 177 | `gds.greedyFeedbackArcSet.stream`, `.write` | — | — | `(sourceNode :: INTEGER, targetNode :: INTEGER, relationshipId :: INTEGER)` | `writeProperty` set to true on each arc; EdgePropWrite |
| 178 | `set_cover.greedy` | `element_vertexes :: LIST<NODE>`, `set_vertexes :: LIST<NODE>` | — | `(containing_set :: NODE)` | — |
| 180 U | `cycles.get` | — | — | `(cycle_id :: INTEGER, node :: NODE)` | — |
| 181 | `gds.elementaryCircuits.stream` | — | — | `(cycleId :: INTEGER, nodeIds :: LIST<INTEGER>, path :: PATH)` | — |
| 187 | `tsp.solve` | `points :: LIST<NODE>` | `method :: STRING` (`"1.5_approx"`; also `"2_approx"`, `"greedy"`); coordinates from node properties `lat`, `lng` | `(sources :: LIST<NODE>, destinations :: LIST<NODE>)` | — |
| 188 | `vrp.route` | `depot_node :: NODE` | `number_of_vehicles :: INTEGER` (null) | `(from_vertex :: NODE, to_vertex :: NODE)` | — |
| 189 | `gds.isIsomorphic.stream` | `otherNodeLabels`, `otherRelationshipTypes` (select the second projection) | — | `(isomorphic :: BOOLEAN)`; parallel edges are matched by their count, through `is_isomorphic_matching`, since petgraph's VF2 expects no parallel edges (`isomorphism.rs:793`) | — |
| 190 | `gds.isIsomorphicSubgraph.stream` | as 189 | — | `(isomorphic :: BOOLEAN)`: the second projection is isomorphic to a node-induced subgraph of the first (`isomorphism.rs:870-894`); parallel edges as 189 | — |
| 194 U | `gds.isBipartiteUndirected.stream`, `.write` | — | — | `(nodeId :: INTEGER, side :: INTEGER)`, side 0 or 1; no row when the projection is not bipartite | `(bipartite :: BOOLEAN, nodePropertiesWritten :: INTEGER, configuration :: MAP)` |
| 203 | `gds.isTournament.stream` | — | — | `(tournament :: BOOLEAN, transitive :: BOOLEAN)` | — |
| 204 | `gds.isDegreeSequenceGraphlike.stream` | `degreeSequence :: LIST<INTEGER>` | — | `(graphlike :: BOOLEAN)` | — |
| 205 | `gds.transitiveClosure.stream`, `.write` | — | — | `(sourceNode :: INTEGER, targetNode :: INTEGER)`; a cyclic projection fails with 22N11 | RelWrite |
| 206 | `gds.transitiveReduction.stream`, `.write` | — | — | `(sourceNode :: INTEGER, targetNode :: INTEGER, relationshipId :: INTEGER)`; a cyclic projection fails with 22N11 | `writeProperty` set to true on each kept edge; EdgePropWrite |

## Entry notes

Behaviours the signature does not show, with their evidence. The review that found them is rmp #71 ([lab report](../evidence/lab-sprint-5/report.md#71-mechanical-review)).

| ALG | Behaviour |
|---|---|
| 004, 031 | Under `UNDIRECTED`, `pathExpand` and `algo.all_simple_paths` run MAGE's search unchanged: it follows a node's incoming, then its outgoing relationships (`path.cpp` `PathExpand::DFS`:359, `algo.cpp` `DFS`:75 @ `c72636014a8b`), and both lists hold every incident edge, so a path is returned once per list at each hop (measured: 10 rows for 3 paths from node 0 to node 2, the one-edge path twice, each two-edge path four times). This is lpg's defined semantics (coordinator, rmp #71 DECISION, 2026-10-12). `pathSubgraphAll` lists each relationship once (#71 finding 3). With a `<T>` filter, MAGE's `pathSubgraphNodes` result is undefined: it keeps `std::string_view`s of a local string (`path.cpp:335, 352, 450, 471`); lpg compares type tokens (rmp #69 FINDING 542). |
| 006 | No walk starts at a node without an outgoing edge, as GDS `traversal/RandomWalkTask.java` `run` (#71 finding 1). When every outgoing weight of a node is 0, lpg ends the walk; GDS's `RandomWalkSampler.java` takes the first neighbour, read from source and not run (unverified). |
| 025 | A negative self-loop is a negative cycle and fails with 22N11; petgraph 0.8.3's `floyd_warshall` resets each self-distance to 0 (`floyd_warshall.rs:284-288`), so lpg checks first (#71 finding 2). |
| 061, 062 | With `includeIntermediateCommunities`, the write mode writes the list of intermediate communities, as GDS (`LouvainWriteStep.java:68-72`, `LeidenWriteStep.java:71-75`; rmp #71 FINDING 571). |
| 062 | `gamma` at least 0 and `maxLevels` at least 1 are lpg's bounds; GDS's are not known (unverified). |
| 063 | On a tie a node keeps its label when it is among the best, else takes the best label of least hash `mix(label ^ node ^ round)` (coordinator DECISION 575 on rmp #71). Raghavan, Albert and Kumara (Phys. Rev. E 76, 2007) break ties at random; NetworkX keeps a best current label (`label_propagation.py:220-231`). GDS takes the lowest label (`tallyVotes`), which gives one community on the ring of cliques and fails rule Q (rmp #66). |
| 064 | Takes `randomSeed`, which GDS 2026.09.0's SLLPA refuses; it changes no GDS-defined behaviour (coordinator DECISION 575 on rmp #71). |
| 080 | Stops after `maxIterations`, or after an iteration other than the first in which at most ⌊n × `deltaThreshold`⌋ nodes changed community, as GDS's code (`KmeansIterationStopper.java`), not its documentation ("fewer than"). Keeps the last assignment. The restart of least average distance wins, as GDS's documentation, not its code's array reuse. `k` above the node count puts each node in its own community. 22N11: `nodeProperty` a number, not a list; `seedCentroids` not of `k` centroids or not of the property's dimension; `seedCentroids` with `numberOfRestarts` above 1 (coordinator DECISION 575 and FINDING 572 on rmp #71; `src/algo/clustering.rs`). |
| 082 | Modularity counts a negative community value and its edges, as GDS `ModularityCalculator.create`; conductance skips it, as GDS (rmp #71 FINDING 573). |
| 090, 091 | A parallel edge is never a bridge: removing it leaves its twin. petgraph's `bridges` reports parallel edges, so lpg runs its own low-link search (rmp #67 DECISION 556; checked against edge removal, #71 pass C). |
| 106 | With `perturbationRate`, a candidate replaces only a neighbour of equal similarity in a full list, as GDS `NeighborList.add`:107-115 (rmp #71 FINDING 573). |
| 112, 113, 115 | The open-source GDS `noCommonNeighbors` counts `node2` as a common neighbour through its self-loop; lpg does not. Read from source, not run on GDS 2026.09.0 (unverified; #71 pass B). |
| 149 | Once a removal leaves the root one child, the root is a leaf candidate with its child's edge weight, and the result is a tree of `k` nodes. GDS re-queues such a root for growth (`KSpanningTree.java:213-218`): on graph `kq`, k 4, GDS 2026.09.0 writes 2 nodes and reports `effectiveNodeCount` 4; lpg writes 4 (measured, [`gds-probes-71c.txt`](../evidence/lab-sprint-5/raw/gds-probes-71c.txt); #71 finding C4). |
| 151 | Exact count: determinants modulo two primes near 2^61, joined by the Chinese remainder theorem. When the diagonal bound reaches the modulus (about 2^123), nalgebra's `f64` determinant decides the range; its relative error measured 2.1e-14 and 2.5e-15 (#71 finding C5). A general error bound for nalgebra's LU determinant is not proved (unverified). |
| 154 | The best node is detached from its parent before the other roots are pruned, so the result is the subtree of best value. GDS's `pruneUnnecessarySubTrees` prunes it with the root above it: on graph `pp`, GDS 2026.09.0 returns no node, lpg node 1, prize 10 (measured, [`gds-probes-71c.txt`](../evidence/lab-sprint-5/raw/gds-probes-71c.txt); #71 finding C4). |
| 178 | Classic greedy (Johnson 1974; Chvátal 1979), not MAGE's random order (rmp #68 DECISION 560; Verification, row 178). |

## Verification

Rules (TS-25 Verification). Oracles run outside the crate ([plan](../evidence/planning/sprint-5-plan.md) §3.3): GDS 2026.09.0 in Docker at `concurrency` at most 4; NetworkX 3.7 in the laboratory virtual environment; MAGE in its Docker image; petgraph on a `petgraph::Graph` built from the same edges, for the entries lpg runs through petgraph's own function. Comparisons:

- **X**: exact equality: integers, booleans, node and edge sets, paths where the result is unique.
- **F**: floats, `|a − b| ≤ t · max(1, |b|)`, with `t` the entry's `tolerance` or `epsilon` parameter where it has one, else 1e-9.
- **P**: partitions equal up to renaming of their identifiers.
- **V**: the result satisfies the problem's definition, and its objective value (cost, size, weight, number of colours) equals the oracle's under F; used where the reference result is not unique.
- **Q**: parity entry: bit-equal results at `concurrency` 1 to 16, and the quality measure in the row. "Ring of cliques": 4 cliques of 6 nodes, each joined to the next by one edge, whose planted partition is the cliques.

Limitation. lpg's CSV import numbers nodes in the string order of `id:ID` (0, 1, 10, 11, …), Neo4j in numeric order, so the two adjacencies list neighbours in different orders (rmp #67 FINDING 558). The second GDS oracle runs, in lpg's node order (`.o`), project through a Cypher aggregation that also lists neighbours in a different order from GDS's native projection (#71 findings C2, C6). Where a heuristic's result depends on ties, a GDS result is therefore not GDS's algorithm on lpg's adjacency. Two rule-V cases differ from GDS for this reason: ALG-149 on karate, k 5 (weight 7, GDS 5; GDS's algorithm, ported, gives lpg's tree on lpg's numbering) and ALG-153 unweighted on `uniform60` (9 edges, GDS 10; the first join takes another target at the same hop distance).

| ALG | Oracle | Rule |
|---|---|---|
| 001 | GDS `gds.bfs.stream` | V: same node set, each node at its BFS depth |
| 002 | GDS `gds.dfs.stream` | V: a DFS preorder of the projection over the same node set |
| 003 | MAGE `neighbors.*` | X |
| 004 | MAGE `path.*` | X (paths and node sets); with a `<T>` filter, `pathSubgraphNodes` node sets only (Entry notes) |
| 006 | — | Q: every walk is a walk of the projection, `walkLength` long or ending at a node with no out-edge; `walksPerNode` walks per start node with an outgoing edge, none from the others; over 10 000 walks with `returnFactor` = `inOutFactor` = 1, each node's first-step frequencies within 4 standard errors of its weighted out-edge probabilities |
| 007 | GDS | V: a topological order of the same node set; `maxDistanceFromSource` X |
| 008 | NetworkX `ancestors`, `descendants` | X |
| 009 | NetworkX `has_path` | X |
| 011 | NetworkX `immediate_dominators`, `dominance_frontiers` | X |
| 013 | NetworkX `shortest_path_length`, `shortest_path` | X lengths; V paths |
| 014 | GDS (both procedures) | F costs; V paths |
| 015 | NetworkX `bidirectional_dijkstra` | F cost; V path |
| 018 | GDS | F costs; V paths |
| 019 | GDS | F costs; V paths; `isNegativeCycle` X |
| 020 | NetworkX `negative_edge_cycle`, `find_negative_cycle` | X existence; V: the cycle's weight is negative |
| 021 | NetworkX `single_source_bellman_ford` | F costs; V paths |
| 023 | GDS | F costs; V paths |
| 024 | NetworkX `johnson`; petgraph `johnson` | F |
| 025 | NetworkX `floyd_warshall` | F |
| 026 | NetworkX `all_pairs_shortest_path_length` | X |
| 027 | GDS | F |
| 028 | NetworkX `all_shortest_paths` | X (path sets) |
| 029 | GDS | F costs in order; V paths |
| 030 | petgraph `k_shortest_path` | F |
| 031 | NetworkX `all_simple_paths` with `cutoff` | X (path sets) |
| 032 | GDS | F cost; V path |
| 034 | GDS | F |
| 035 | GDS, without sampling | F |
| 038 | GDS | F |
| 039 | GDS | F |
| 040 | GDS | F |
| 041 | MAGE `katz_centrality.get` | F (`epsilon`) |
| 042 | GDS | F |
| 043 | GDS `gds.pageRank` with `sourceNodes` | F |
| 044 | GDS | F |
| 045 | GDS | F |
| 059 | GDS at `concurrency` 1 | Q: with `propagationProbability` 1, the spread equals the exact greedy reachability cover; at the default, the spread of lpg's seed set is at least GDS's minus 3 standard errors, both re-estimated with 10 000 cascades |
| 061 | GDS | Q: the ring of cliques gives its planted partition (P); karate-club modularity recorded with GDS's |
| 062 | GDS | Q: as 061 |
| 063 | GDS | Q: as 061 |
| 064 | GDS | Q: on the ring of cliques each node's strongest label is its clique; karate-club result recorded with GDS's |
| 065 | GDS | Q: as 061 |
| 076 | — | Q: the ring of cliques with `num_clusters` 4 gives its planted partition (P); modularity recorded |
| 077 | — | Q: as 076; ratio-cut value recorded |
| 078 | GDS at `concurrency` 1 with `randomSeed` | Q: cut weight at least GDS's on the same graph; the optimum, enumerated on graphs of at most 12 nodes, recorded |
| 080 | GDS at `concurrency` 1 with `randomSeed` | Q: four well-separated planted clusters give their partition (P); inertia at most GDS's under F |
| 081 | GDS | Q: planted clusters with noise points give their partition (P), noise labels equal |
| 082 | GDS `gds.modularity`, `gds.conductance`; NetworkX `modularity`, `partition_quality`, `constant_potts_model` | F |
| 083 | GDS | P; with `seedProperty`, unseeded components are numbered in root order and GDS's in `setIdOf` call order (`HugeAtomicDisjointSetStruct.java:131-147`), so their ids may differ (#71 pass C) |
| 084 | NetworkX `connected_components` | P |
| 085 | GDS | P |
| 086 | NetworkX `condensation` | P members; X component edges |
| 089 | NetworkX `biconnected_component_edges` | P on edges |
| 090 | GDS | X |
| 091 | GDS | X (unordered pairs, `remainingSizes`) |
| 093 | GDS | X |
| 105 | GDS (both procedures) | F; V where `topK` cuts through equal similarities |
| 106 | exact brute-force top-K; GDS at `concurrency` 1 with `randomSeed` | Q: recall against the exact top-K at least GDS's on the same graph |
| 107 | GDS functions | F |
| 112 | GDS function | F |
| 113 | GDS function | F |
| 114 | GDS function | F |
| 115 | GDS function | F |
| 116 | GDS function | F |
| 117 | GDS function | F |
| 120 | GDS `gds.triangleCount`, `gds.triangles` | X |
| 121 | GDS | F |
| 123 | NetworkX `eccentricity`, `diameter`, `radius` | X unweighted; F weighted |
| 128 | NetworkX `girth` | X |
| 143 | NetworkX `laplacian_matrix` | F |
| 147 | GDS | V: total weight F |
| 149 | GDS | V: a tree of `k` nodes; total weight F |
| 151 | determinant of the reduced Laplacian over rationals (Python `fractions`) | X |
| 153 | GDS | V: a tree reaching every reachable target; total weight F |
| 154 | GDS | V: a tree; `totalWeight` and `sumOfPrizes` F |
| 155 | GDS | V: capacities and conservation hold; `totalFlow` F |
| 157 | GDS | V: as 155; `totalFlow`, `totalCost` F |
| 161 | NetworkX `max_weight_matching` (`maxcardinality`), `is_maximal_matching` | V: matching size equal; greedy form a maximal matching |
| 163 | NetworkX `hopcroft_karp_matching` | X size |
| 169 | NetworkX `find_cliques` | X (clique sets) |
| 171 | GDS | X |
| 173 | NetworkX `greedy_color`, same strategy and node order | V: proper colouring; number of colours equal |
| 174 | petgraph `dsatur_coloring` | V: proper colouring; number of colours equal |
| 176 | GDS at `concurrency` 1 | Q: proper colouring with at most GDS's number of colours |
| 177 | petgraph `greedy_feedback_arc_set` | X; V: removing the arcs leaves a DAG |
| 178 | classic greedy (Johnson 1974; Chvátal 1979) | V: a cover; size equal to the classic greedy's; equal to the exhaustive optimum on the fixtures where it is known. MAGE's `set_cover.greedy` picks sets in random order (6, 4, 7 sets in three runs; [lab report](../evidence/lab-sprint-5/report.md) §#67 and #68 and §#71, row 178) and is not an oracle (coordinator, rmp #71 DECISION, 2026-10-12) |
| 180 | MAGE `cycles.get` | X (cycles as node sets) |
| 181 | NetworkX `simple_cycles` | X (cycles up to rotation) |
| 187 | optimum enumerated on graphs of at most 9 nodes | V: a tour of every point; cost at most 2 times (`2_approx`) or 1.5 times (`1.5_approx`) the optimum |
| 188 | optimum enumerated on graphs of at most 9 nodes | Q: every non-depot node on exactly one route, each route starts and ends at the depot, at most `number_of_vehicles` routes; no 2-opt move shortens a route; ratio to the optimum recorded |
| 189 | NetworkX `is_isomorphic` on multigraphs | X |
| 190 | petgraph `is_isomorphic_subgraph` | X |
| 194 | NetworkX `is_bipartite`, bipartite `sets` | X; V sides |
| 203 | NetworkX `is_tournament`, `is_directed_acyclic_graph` (a tournament is transitive exactly when acyclic) | X |
| 204 | NetworkX `is_graphical` | X |
| 205 | NetworkX `transitive_closure_dag` | X |
| 206 | NetworkX `transitive_reduction` | X |
