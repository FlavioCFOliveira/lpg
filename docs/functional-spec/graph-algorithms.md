---
title: Graph-algorithm inventory
status: draft
last-reviewed: 2026-10-02
related:
  - functional-requirements.md
  - open-questions.md
  - README.md
---

# Graph-algorithm inventory

## Overview

This file is the closed inventory of graph algorithms that the crate provides ([FR-ALG-005](functional-requirements.md#graph-algorithms)). The user decided that the crate covers the known algorithms of five reference sources, then reduced the list on 2026-09-24 ([NFR-SIMP-001](non-functional-requirements.md#simplicity)): an entry is kept only when Neo4j Graph Data Science, Memgraph MAGE, petgraph or graphalgs provides it, and graph learning, embeddings and dynamic algorithms are excluded. An algorithm not listed here requires a change to this specification. The whole inventory ships in the first release ([FR-ALG-005](functional-requirements.md#graph-algorithms)). Withdrawn identifiers are listed in [Withdrawn entries](#withdrawn-entries).

Each kept entry names the algorithm once and lists every reference source that provides it, with the source's own procedure or function names. Variants that differ only in output form (path or length, single-source or all-pairs, stream or write) are one entry.

## Reference sources

Each source was read on 2026-09-24. Codes used in the tables:

- **G**: Neo4j Graph Data Science, "Graph Algorithms" operations-reference page (tiers production-quality and beta) [S26].
- **M**: Memgraph, "Available algorithms" page (MAGE) [S27], with procedure names read from the MAGE source at commit `c72636014a8b144b5ec1969a8de39f5369a53f5c`. "Memgraph built-in" marks the deep path traversal algorithms that the page lists as part of Memgraph itself.
- **P**: petgraph 0.8.3, module `petgraph::algo`, read from the source at commit `a4d94bd2c39ac198c22682b2dcb1ff21583d0db0` [S28].
- **A**: graphalgs 0.2.0, read from the source at commit `1f8a412ef4afb944177b5d0fee335e46314f903f` [S29].
- **N**: NetworkX 3.7 (listed for reference only; no entry is kept on NetworkX alone), the `__all__` lists of the modules under `networkx/algorithms`, read from the source at tag `networkx-3.7` [S30].

Every entry below was verified in at least one of these sources. No entry is unverified.

## Inclusion rule

An entry is included when a source provides it as a procedure or function that computes a result (a value, a score, a set, a partition, a path, a tree, a flow or a derived graph) from an input graph. The following are excluded, with the reason:

- Graph generators and graph randomisation (for example NetworkX `bipartite/generators`, `swap`, `random_tournament`; graphalgs `generate`; the MAGE cuGraph graph generator): they create graphs rather than analyse one.
- Import, export and encoding functions (for example NetworkX `bipartite/edgelist`, `bipartite/matrix`, `tree/coding`, `tournament_matrix`, `planar_drawing`; graphalgs `adj_matrix` and Prüfer coding; MAGE `gnn` export): bulk import and export are specified separately ([Import and export](functional-requirements.md#import-and-export)).
- General graph operators (NetworkX `operators` and `minors`: union, product, complement, contraction, quotient graph).
- Helper classes, callbacks and exception types (for example NetworkX `isomorphism/matchhelpers`, `flow/utils`, `connectivity/utils`, `PlanarEmbedding`, `EdgePartition`, `NotATree`).
- MAGE utility and integration modules that are not graph algorithms (for example `collections`, `text`, `map`, `date`, `json_util`, `migrate`, `llm`, `elasticsearch_synchronization`, `graph_analyzer`, `distance_calculator`), and MAGE `nxalg`, `igraph` and `cugraph` where they re-expose an algorithm listed here.
- Neo4j Graph Data Science data-preparation procedures `gds.scaleProperties` and `gds.splitRelationships`.
- Entries that only NetworkX provides, graph learning and embeddings, and dynamic algorithms (withdrawn; see [Withdrawn entries](#withdrawn-entries)).

## Traversal and ordering

| ID | Algorithm | Sources |
|---|---|---|
| ALG-001 | Breadth-first search | G `gds.bfs`; M Memgraph built-in breadth-first search; N `bfs_edges`, `bfs_tree`, `bfs_layers`, `bfs_predecessors`, `bfs_successors`, `bfs_labeled_edges`, `generic_bfs_edges`, `edge_bfs` |
| ALG-002 | Depth-first search | G `gds.dfs`; M Memgraph built-in depth-first search; N `dfs_edges`, `dfs_tree`, `dfs_preorder_nodes`, `dfs_postorder_nodes`, `dfs_predecessors`, `dfs_successors`, `dfs_labeled_edges`, `edge_dfs` |
| ALG-003 | Neighbourhood at a given hop distance | M `neighbors.at_hop`, `neighbors.by_hop`; N `descendants_at_distance` |
| ALG-004 | Path expansion and reachable subgraph | M `path.expand`, `path.subgraph_nodes`, `path.subgraph_all` |
| ALG-006 | Random walk | G `gds.randomWalk`; N `random_walk` |
| ALG-007 | Topological sort | G `gds.dag.topologicalSort`; M `graph_util.topological_sort`; P `toposort`; N `topological_sort`, `lexicographical_topological_sort`, `all_topological_sorts`, `topological_generations` |
| ALG-008 | Ancestors and descendants | M `graph_util.ancestors`, `graph_util.descendants`; N `ancestors`, `descendants` |
| ALG-009 | Reachability test | P `has_path_connecting`; N `has_path`, `is_reachable` |
| ALG-011 | Dominators and dominance frontiers | P `dominators::simple_fast`; N `immediate_dominators`, `dominance_frontiers` |

## Shortest and weighted paths

| ID | Algorithm | Sources |
|---|---|---|
| ALG-013 | Unweighted shortest paths (single-pair, single-source, single-target, all-pairs, bidirectional) | M Memgraph built-in breadth-first search; N `shortest_path`, `shortest_path_length`, `bidirectional_shortest_path`, `single_source_shortest_path`, `single_source_shortest_path_length`, `single_target_shortest_path`, `single_target_shortest_path_length`, `all_pairs_shortest_path`, `all_pairs_shortest_path_length`, `predecessor` |
| ALG-014 | Dijkstra shortest path (single-pair and single-source) | G `gds.shortestPath.dijkstra`, `gds.allShortestPaths.dijkstra`; M Memgraph built-in weighted shortest path; P `dijkstra`, `with_dynamic_goal`; N `dijkstra_path`, `dijkstra_path_length`, `single_source_dijkstra`, `single_source_dijkstra_path`, `single_source_dijkstra_path_length`, `dijkstra_predecessor_and_distance` |
| ALG-015 | Bidirectional Dijkstra | P `bidirectional_dijkstra`; N `bidirectional_dijkstra` |
| ALG-018 | A* search | G `gds.shortestPath.astar`; M `algo.astar`; P `astar`; N `astar_path`, `astar_path_length` |
| ALG-019 | Bellman-Ford | G `gds.bellmanFord`; P `bellman_ford`; N `bellman_ford_path`, `bellman_ford_path_length`, `single_source_bellman_ford`, `single_source_bellman_ford_path`, `single_source_bellman_ford_path_length`, `all_pairs_bellman_ford_path`, `all_pairs_bellman_ford_path_length`, `bellman_ford_predecessor_and_distance` |
| ALG-020 | Negative-cycle detection | P `find_negative_cycle`; N `negative_edge_cycle`, `find_negative_cycle` |
| ALG-021 | Shortest Path Faster Algorithm (SPFA) | P `spfa` |
| ALG-023 | Delta-stepping single-source shortest paths | G `gds.allShortestPaths.delta` |
| ALG-024 | Johnson all-pairs shortest paths | P `johnson`, `parallel_johnson`; N `johnson` |
| ALG-025 | Floyd-Warshall all-pairs shortest paths | P `floyd_warshall`, `floyd_warshall_path`; A `floyd_warshall`; N `floyd_warshall`, `floyd_warshall_predecessor_and_distance`, `floyd_warshall_numpy`, `floyd_warshall_tree`, `reconstruct_path` |
| ALG-026 | Seidel all-pairs shortest paths (unweighted, undirected) | A `seidel` |
| ALG-027 | All-pairs shortest distances | G `gds.allShortestPaths`; A `shortest_distances`, `distance_map` |
| ALG-028 | All shortest paths between nodes | M Memgraph built-in all shortest paths; N `all_shortest_paths`, `single_source_all_shortest_paths`, `all_pairs_all_shortest_paths` |
| ALG-029 | Yen's k shortest paths | G `gds.shortestPath.yens`; M Memgraph built-in k shortest paths; N `shortest_simple_paths` |
| ALG-030 | Length of the k-th shortest path | P `k_shortest_path` |
| ALG-031 | All simple paths | M `algo.all_simple_paths`; P `all_simple_paths`, `all_simple_paths_multi`; N `all_simple_paths`, `all_simple_edge_paths`, `is_simple_path` |
| ALG-032 | Longest path in a directed acyclic graph | G `gds.dag.longestPath`; N `dag_longest_path`, `dag_longest_path_length` |

## Centrality

| ID | Algorithm | Sources |
|---|---|---|
| ALG-034 | Degree centrality | G `gds.degree`; M `degree_centrality`; N `degree_centrality`, `in_degree_centrality`, `out_degree_centrality` |
| ALG-035 | Betweenness centrality | G `gds.betweenness`; M `betweenness_centrality`; N `betweenness_centrality` |
| ALG-038 | Closeness centrality | G `gds.closeness`; N `closeness_centrality`, `incremental_closeness_centrality` |
| ALG-039 | Harmonic centrality | G `gds.closeness.harmonic`; N `harmonic_centrality` |
| ALG-040 | Eigenvector centrality | G `gds.eigenvector`; N `eigenvector_centrality`, `eigenvector_centrality_numpy` |
| ALG-041 | Katz centrality | M `katz_centrality`; N `katz_centrality`, `katz_centrality_numpy` |
| ALG-042 | PageRank | G `gds.pageRank`; M `pagerank`; P `page_rank`, `parallel_page_rank`; N `pagerank`, `google_matrix` |
| ALG-043 | Personalised PageRank | M cuGraph module `personalized_pagerank` |
| ALG-044 | ArticleRank | G `gds.articleRank` |
| ALG-045 | HITS (hubs and authorities) | G `gds.hits`; N `hits` |
| ALG-059 | Influence maximisation (CELF) | G `gds.influenceMaximization.celf` |

## Community detection and clustering

| ID | Algorithm | Sources |
|---|---|---|
| ALG-061 | Louvain | G `gds.louvain`; M `community_detection`; N `louvain_communities`, `louvain_partitions` |
| ALG-062 | Leiden | G `gds.leiden`; M `leiden_community_detection`; N `leiden_communities`, `leiden_partitions` |
| ALG-063 | Label propagation | G `gds.labelPropagation`; N `label_propagation_communities`, `asyn_lpa_communities`, `fast_label_propagation_communities` |
| ALG-064 | Speaker-listener label propagation (overlapping communities) | G `gds.sllpa` |
| ALG-065 | Modularity optimisation | G `gds.modularityOptimization` |
| ALG-076 | Spectral clustering | M cuGraph module `spectral_clustering` |
| ALG-077 | Balanced-cut clustering | M cuGraph module `balanced_cut_clustering` |
| ALG-078 | Approximate maximum k-cut | G `gds.maxkcut` |
| ALG-080 | K-means clustering | G `gds.kmeans`; M `kmeans_clustering` |
| ALG-081 | HDBSCAN clustering | G `gds.hdbscan` |
| ALG-082 | Partition quality (modularity, conductance, Potts model, coverage and performance) | G `gds.modularity`, `gds.conductance`; N `modularity`, `overlapping_modularity`, `constant_potts_model`, `partition_quality`, `is_partition`, `is_cover` |

## Components and connectivity

| ID | Algorithm | Sources |
|---|---|---|
| ALG-083 | Weakly connected components | G `gds.wcc`; M `weakly_connected_components`, `union_find`; N `weakly_connected_components`, `number_weakly_connected_components`, `is_weakly_connected` |
| ALG-084 | Connected components (undirected) | P `connected_components`; N `connected_components`, `number_connected_components`, `is_connected`, `node_connected_component` |
| ALG-085 | Strongly connected components | G `gds.scc`; P `scc`, `tarjan_scc`, `kosaraju_scc`; N `strongly_connected_components`, `kosaraju_strongly_connected_components`, `number_strongly_connected_components`, `is_strongly_connected` |
| ALG-086 | Condensation | P `condensation`; N `condensation` |
| ALG-089 | Biconnected components | M `biconnected_components`; N `biconnected_components`, `biconnected_component_edges`, `is_biconnected` |
| ALG-090 | Articulation points | G `gds.articulationPoints`; P `articulation_points`; A `articulation_points`; N `articulation_points` |
| ALG-091 | Bridges | G `gds.bridges`; M `bridges`; P `bridges`; A `find_bridges`; N `bridges`, `has_bridges`, `local_bridges` |
| ALG-093 | k-core decomposition | G `gds.kcore`; N `core_number`, `k_core`, `k_shell`, `k_crust`, `k_corona`, `onion_layers` |

## Similarity and link prediction

| ID | Algorithm | Sources |
|---|---|---|
| ALG-105 | Neighbourhood node similarity (Jaccard, overlap, cosine) | G `gds.nodeSimilarity`, `gds.nodeSimilarity.filtered`; M `node_similarity.jaccard`, `node_similarity.overlap`, `node_similarity.cosine` and their pairwise forms; N `jaccard_coefficient` |
| ALG-106 | K-nearest neighbours | G `gds.knn`, `gds.knn.filtered`; M `knn` |
| ALG-107 | Vector similarity functions (cosine, Euclidean, Jaccard, overlap, Pearson) | G `gds.similarity.cosine`, `gds.similarity.euclidean`, `gds.similarity.euclideanDistance`, `gds.similarity.jaccard`, `gds.similarity.overlap`, `gds.similarity.pearson` |
| ALG-112 | Adamic-Adar index | G `gds.linkprediction.adamicAdar`; N `adamic_adar_index` |
| ALG-113 | Common neighbours | G `gds.linkprediction.commonNeighbors` |
| ALG-114 | Preferential attachment | G `gds.linkprediction.preferentialAttachment`; N `preferential_attachment` |
| ALG-115 | Resource allocation index | G `gds.linkprediction.resourceAllocation`; N `resource_allocation_index` |
| ALG-116 | Same community | G `gds.linkprediction.sameCommunity` |
| ALG-117 | Total neighbours | G `gds.linkprediction.totalNeighbors` |

## Structural measures

| ID | Algorithm | Sources |
|---|---|---|
| ALG-120 | Triangle count and triangle listing | G `gds.triangleCount`, `gds.triangles`; N `triangles`, `all_triangles` |
| ALG-121 | Clustering coefficient and transitivity | G `gds.localClusteringCoefficient`; N `clustering`, `average_clustering` (exact and approximate), `transitivity`, `square_clustering`, `generalized_degree` |
| ALG-123 | Eccentricity, diameter, radius, centre and periphery | A `eccentricity`, `diameter`, `radius`, `center`, `periphery` and their `weighted_` forms; N `eccentricity`, `diameter` (exact and approximate), `harmonic_diameter`, `radius`, `center`, `periphery` |
| ALG-128 | Girth | A `girth`; N `girth` |
| ALG-143 | Laplacian matrix | A `laplacian_matrix` |

## Spanning trees, flows and cuts

| ID | Algorithm | Sources |
|---|---|---|
| ALG-147 | Minimum spanning tree (Kruskal, Prim, Borůvka) | G `gds.spanningTree`; P `min_spanning_tree`, `min_spanning_tree_prim`; A `prim`, `boruvka`; N `minimum_spanning_tree`, `minimum_spanning_edges` |
| ALG-149 | k-spanning tree | G `gds.kSpanningTree` |
| ALG-151 | Number of spanning trees | A `count_spanning_trees`; N `number_of_spanning_trees` |
| ALG-153 | Steiner tree | G `gds.steinerTree`; P `steiner_tree`; N `steiner_tree`, `metric_closure` |
| ALG-154 | Prize-collecting Steiner tree | G `gds.prizeSteinerTree` |
| ALG-155 | Maximum flow | G `gds.maxFlow`; M `max_flow`; P `ford_fulkerson`, `dinics`; N `maximum_flow`, `maximum_flow_value`, `edmonds_karp`, `dinitz`, `preflow_push`, `shortest_augmenting_path`, `boykov_kolmogorov` |
| ALG-157 | Minimum-cost flow | G `gds.maxFlow.minCost`; N `min_cost_flow`, `min_cost_flow_cost`, `max_flow_min_cost`, `cost_of_flow`, `network_simplex`, `capacity_scaling` |

## Matching, covering, cliques and colouring

| ID | Algorithm | Sources |
|---|---|---|
| ALG-161 | Maximum and maximal matching | P `maximum_matching`, `greedy_matching`; N `maximal_matching`, `is_matching`, `is_maximal_matching`, `is_perfect_matching` |
| ALG-163 | Bipartite matching | M `bipartite_matching`; N `hopcroft_karp_matching`, `eppstein_matching`, bipartite `maximum_matching`, `minimum_weight_full_matching`, `to_vertex_cover` |
| ALG-169 | Maximal cliques (Bron-Kerbosch) | P `maximal_cliques`; N `find_cliques`, `find_cliques_recursive`, `enumerate_all_cliques`, `node_clique_number`, `number_of_cliques`, `make_max_clique_graph`, `make_clique_bipartite` |
| ALG-171 | Clique counting | G `gds.cliqueCounting` |
| ALG-173 | Greedy colouring | M `graph_coloring`; N `greedy_color` with the strategies `strategy_largest_first`, `strategy_smallest_last`, `strategy_independent_set`, `strategy_connected_sequential`, `strategy_connected_sequential_bfs`, `strategy_connected_sequential_dfs`, `strategy_random_sequential` |
| ALG-174 | DSatur colouring | P `dsatur_coloring`; N `strategy_saturation_largest_first` |
| ALG-176 | K-1 colouring | G `gds.k1coloring` |
| ALG-177 | Greedy feedback arc set | P `greedy_feedback_arc_set` |
| ALG-178 | Set cover | M `set_cover` |

## Cycles, tours and routing

| ID | Algorithm | Sources |
|---|---|---|
| ALG-180 | Cycle detection and acyclicity test | M `cycles`; P `is_cyclic_directed`, `is_cyclic_undirected`; N `find_cycle`, `is_directed_acyclic_graph` |
| ALG-181 | Simple-cycle enumeration | A `elementary_circuits`; N `simple_cycles`, `recursive_simple_cycles` |
| ALG-187 | Travelling salesman | M `tsp`; N `traveling_salesman_problem`, `christofides`, `asadpour_atsp`, `greedy_tsp`, `simulated_annealing_tsp`, `threshold_accepting_tsp` |
| ALG-188 | Vehicle routing | M `vrp` |

## Isomorphism and graph recognition

| ID | Algorithm | Sources |
|---|---|---|
| ALG-189 | Graph isomorphism (VF2, VF2++) | P `is_isomorphic`, `is_isomorphic_matching`; N `is_isomorphic`, `could_be_isomorphic`, `fast_could_be_isomorphic`, `faster_could_be_isomorphic`, `GraphMatcher`, `DiGraphMatcher`, `MultiGraphMatcher`, `MultiDiGraphMatcher`, `vf2pp_isomorphism`, `vf2pp_is_isomorphic`, `vf2pp_all_isomorphisms` |
| ALG-190 | Subgraph isomorphism and monomorphism | P `is_isomorphic_subgraph`, `is_isomorphic_subgraph_matching`, `subgraph_isomorphisms_iter`; N `ISMAGS`, `vf2pp_subgraph_isomorphism`, `vf2pp_subgraph_is_isomorphic`, `vf2pp_all_subgraph_isomorphisms`, `vf2pp_monomorphism`, `vf2pp_is_monomorphic`, `vf2pp_all_monomorphisms` |
| ALG-194 | Bipartiteness test and bipartition | P `is_bipartite_undirected`; N `is_bipartite`, `is_bipartite_node_set`, bipartite `color`, bipartite `sets` |
| ALG-203 | Tournament tests | A `is_tournament`, `is_tournament_transitive`; N `is_tournament`, `score_sequence`, tournament `is_strongly_connected` |
| ALG-204 | Degree-sequence realisability | A `is_degree_sequence_graphlike`; N `is_graphical`, `is_multigraphical`, `is_pseudographical`, `is_digraphical`, `is_valid_degree_sequence_erdos_gallai`, `is_valid_degree_sequence_havel_hakimi` |

## Derived structures

| ID | Algorithm | Sources |
|---|---|---|
| ALG-205 | Transitive closure | P `dag_transitive_reduction_closure`; N `transitive_closure`, `transitive_closure_dag` |
| ALG-206 | Transitive reduction | P `dag_transitive_reduction_closure`, `dag_to_toposorted_adjacency_list`; N `transitive_reduction` |

## Derived procedure names

Neither Neo4j Graph Data Science nor Memgraph MAGE provides these entries as a usable procedure: either no procedure exists, or the MAGE name does not parse under opengql/grammar because it starts with the reserved word `PATH` (ALG-004). Their procedure names follow the rule in [FR-ALG-007](functional-requirements.md#graph-algorithms) (user decision 2026-10-02): the `gds.` namespace and a camelCase name derived from the first function that petgraph or graphalgs lists for the entry, or from each MAGE name for ALG-004, with `.`, `::` and `_` removed. Where an entry has no function name (ALG-013, ALG-028) or shares its first function with another entry (ALG-205, ALG-206), the name is derived from the entry's algorithm name in this inventory, without its parenthetical. Each name takes the mode suffixes `.stream` and `.write`.

| ID | Procedure name | Derived from |
|---|---|---|
| ALG-004 | `gds.pathExpand`, `gds.pathSubgraphNodes`, `gds.pathSubgraphAll` | MAGE `path.expand`, `path.subgraph_nodes`, `path.subgraph_all` (`PATH` is reserved) |
| ALG-009 | `gds.hasPathConnecting` | petgraph `has_path_connecting` |
| ALG-011 | `gds.dominatorsSimpleFast` | petgraph `dominators::simple_fast` |
| ALG-013 | `gds.unweightedShortestPaths` | algorithm name "Unweighted shortest paths" |
| ALG-015 | `gds.bidirectionalDijkstra` | petgraph `bidirectional_dijkstra` |
| ALG-020 | `gds.findNegativeCycle` | petgraph `find_negative_cycle` |
| ALG-021 | `gds.spfa` | petgraph `spfa` |
| ALG-024 | `gds.johnson` | petgraph `johnson` |
| ALG-025 | `gds.floydWarshall` | petgraph `floyd_warshall` |
| ALG-026 | `gds.seidel` | graphalgs `seidel` |
| ALG-028 | `gds.allShortestPathsBetweenNodes` | algorithm name "All shortest paths between nodes" |
| ALG-030 | `gds.kShortestPath` | petgraph `k_shortest_path` |
| ALG-084 | `gds.connectedComponents` | petgraph `connected_components` |
| ALG-086 | `gds.condensation` | petgraph `condensation` |
| ALG-123 | `gds.eccentricity` | graphalgs `eccentricity` |
| ALG-128 | `gds.girth` | graphalgs `girth` |
| ALG-143 | `gds.laplacianMatrix` | graphalgs `laplacian_matrix` |
| ALG-151 | `gds.countSpanningTrees` | graphalgs `count_spanning_trees` |
| ALG-161 | `gds.maximumMatching` | petgraph `maximum_matching` |
| ALG-169 | `gds.maximalCliques` | petgraph `maximal_cliques` |
| ALG-174 | `gds.dsaturColoring` | petgraph `dsatur_coloring` |
| ALG-177 | `gds.greedyFeedbackArcSet` | petgraph `greedy_feedback_arc_set` |
| ALG-181 | `gds.elementaryCircuits` | graphalgs `elementary_circuits` |
| ALG-189 | `gds.isIsomorphic` | petgraph `is_isomorphic` |
| ALG-190 | `gds.isIsomorphicSubgraph` | petgraph `is_isomorphic_subgraph` |
| ALG-194 | `gds.isBipartiteUndirected` | petgraph `is_bipartite_undirected` |
| ALG-203 | `gds.isTournament` | graphalgs `is_tournament` |
| ALG-204 | `gds.isDegreeSequenceGraphlike` | graphalgs `is_degree_sequence_graphlike` |
| ALG-205 | `gds.transitiveClosure` | algorithm name "Transitive closure" |
| ALG-206 | `gds.transitiveReduction` | algorithm name "Transitive reduction" |

## Withdrawn entries

These identifiers were withdrawn on 2026-09-24 with the reason "NFR-SIMP-001, user decision 2026-09-24". They are kept here so that no identifier is reused. The user withdrew three groups: graph learning and embeddings (ALG-215 to ALG-222), dynamic algorithms (ALG-223 to ALG-227), and every entry that only NetworkX provides.

| ID | Algorithm | Group |
|---|---|---|
| ALG-005 | Beam search | Only in NetworkX |
| ALG-010 | Lowest common ancestor | Only in NetworkX |
| ALG-012 | Antichains | Only in NetworkX |
| ALG-016 | Multi-source Dijkstra | Only in NetworkX |
| ALG-017 | All-pairs Dijkstra | Only in NetworkX |
| ALG-022 | Goldberg-Radzik | Only in NetworkX |
| ALG-033 | Average shortest path length | Only in NetworkX |
| ALG-036 | Edge betweenness centrality | Only in NetworkX |
| ALG-037 | Subset betweenness centrality | Only in NetworkX |
| ALG-046 | Current-flow betweenness centrality | Only in NetworkX |
| ALG-047 | Current-flow closeness and information centrality | Only in NetworkX |
| ALG-048 | Load centrality | Only in NetworkX |
| ALG-049 | Percolation centrality | Only in NetworkX |
| ALG-050 | Reaching centrality | Only in NetworkX |
| ALG-051 | Second-order centrality | Only in NetworkX |
| ALG-052 | Subgraph centrality and Estrada index | Only in NetworkX |
| ALG-053 | Communicability and communicability betweenness | Only in NetworkX |
| ALG-054 | Trophic levels | Only in NetworkX |
| ALG-055 | VoteRank | Only in NetworkX |
| ALG-056 | Laplacian centrality | Only in NetworkX |
| ALG-057 | Dispersion | Only in NetworkX |
| ALG-058 | Group centrality | Only in NetworkX |
| ALG-060 | Closeness vitality | Only in NetworkX |
| ALG-066 | Greedy modularity maximisation | Only in NetworkX |
| ALG-067 | Girvan-Newman (edge-betweenness partitioning) | Only in NetworkX |
| ALG-068 | Edge current-flow betweenness partitioning | Only in NetworkX |
| ALG-069 | k-clique communities | Only in NetworkX |
| ALG-070 | Asynchronous fluid communities | Only in NetworkX |
| ALG-071 | Kernighan-Lin bisection | Only in NetworkX |
| ALG-072 | Spectral modularity bipartition | Only in NetworkX |
| ALG-073 | Greedy node-swap bipartition | Only in NetworkX |
| ALG-074 | Lukes tree partitioning | Only in NetworkX |
| ALG-075 | Greedy source expansion (local community) | Only in NetworkX |
| ALG-079 | Approximate maximum cut | Only in NetworkX |
| ALG-087 | Attracting components | Only in NetworkX |
| ALG-088 | Semiconnectedness test | Only in NetworkX |
| ALG-092 | Chain decomposition | Only in NetworkX |
| ALG-094 | k-truss | Only in NetworkX |
| ALG-095 | k-components | Only in NetworkX |
| ALG-096 | k-edge-connected components and subgraphs | Only in NetworkX |
| ALG-097 | k-edge augmentation | Only in NetworkX |
| ALG-098 | Node connectivity | Only in NetworkX |
| ALG-099 | Edge connectivity | Only in NetworkX |
| ALG-100 | Minimum node and edge cuts | Only in NetworkX |
| ALG-101 | Stoer-Wagner global minimum cut | Only in NetworkX |
| ALG-102 | All minimum node cuts | Only in NetworkX |
| ALG-103 | Edge-disjoint and node-disjoint paths | Only in NetworkX |
| ALG-104 | kl-connected subgraph | Only in NetworkX |
| ALG-108 | SimRank | Only in NetworkX |
| ALG-109 | Panther similarity | Only in NetworkX |
| ALG-110 | Graph edit distance | Only in NetworkX |
| ALG-111 | Weisfeiler-Lehman graph hashing | Only in NetworkX |
| ALG-118 | Community-aware link prediction (Soundarajan-Hopcroft, within-inter-cluster) | Only in NetworkX |
| ALG-119 | Common-neighbour and centrality-based parameterised algorithm (CCPA) | Only in NetworkX |
| ALG-122 | Triadic census | Only in NetworkX |
| ALG-124 | Barycentre and centroid | Only in NetworkX |
| ALG-125 | Resistance distance and Kemeny constant | Only in NetworkX |
| ALG-126 | Efficiency | Only in NetworkX |
| ALG-127 | Topological indices (Wiener, Schultz, Gutman, hyper-Wiener) | Only in NetworkX |
| ALG-129 | Rich-club coefficient | Only in NetworkX |
| ALG-130 | s-metric | Only in NetworkX |
| ALG-131 | Reciprocity | Only in NetworkX |
| ALG-132 | Assortativity and mixing | Only in NetworkX |
| ALG-133 | Average degree connectivity and average neighbour degree | Only in NetworkX |
| ALG-134 | Flow hierarchy | Only in NetworkX |
| ALG-135 | Non-randomness | Only in NetworkX |
| ALG-136 | Small-world coefficients | Only in NetworkX |
| ALG-137 | Structural holes | Only in NetworkX |
| ALG-138 | Cut, boundary and expansion measures | Only in NetworkX |
| ALG-139 | CD index | Only in NetworkX |
| ALG-140 | Number of walks | Only in NetworkX |
| ALG-141 | Voronoi cells | Only in NetworkX |
| ALG-142 | Isolates | Only in NetworkX |
| ALG-144 | Graph polynomials (Tutte, chromatic) | Only in NetworkX |
| ALG-145 | d-separation | Only in NetworkX |
| ALG-146 | Tree broadcasting | Only in NetworkX |
| ALG-148 | Maximum spanning tree | Only in NetworkX |
| ALG-150 | Random and partitioned spanning trees | Only in NetworkX |
| ALG-152 | Optimum branchings and arborescences (Edmonds) | Only in NetworkX |
| ALG-156 | Minimum s-t cut by maximum flow | Only in NetworkX |
| ALG-158 | Gomory-Hu tree | Only in NetworkX |
| ALG-159 | Graph spanner | Only in NetworkX |
| ALG-160 | Junction tree | Only in NetworkX |
| ALG-162 | Maximum-weight and minimum-weight matching | Only in NetworkX |
| ALG-164 | Approximate minimum maximal matching | Only in NetworkX |
| ALG-165 | Edge cover | Only in NetworkX |
| ALG-166 | Approximate minimum-weight vertex cover | Only in NetworkX |
| ALG-167 | Dominating sets | Only in NetworkX |
| ALG-168 | Independent sets | Only in NetworkX |
| ALG-170 | Maximum and maximum-weight clique | Only in NetworkX |
| ALG-172 | Approximate densest subgraph | Only in NetworkX |
| ALG-175 | Equitable colouring | Only in NetworkX |
| ALG-179 | Ramsey approximation | Only in NetworkX |
| ALG-182 | Cycle basis and minimum cycle basis | Only in NetworkX |
| ALG-183 | Chordless cycles | Only in NetworkX |
| ALG-184 | Aperiodicity test | Only in NetworkX |
| ALG-185 | Eulerian circuits and paths | Only in NetworkX |
| ALG-186 | Hamiltonian path in a tournament | Only in NetworkX |
| ALG-191 | Time-respecting graph matching | Only in NetworkX |
| ALG-192 | Tree isomorphism | Only in NetworkX |
| ALG-193 | Planarity test | Only in NetworkX |
| ALG-195 | Chordal graph recognition and completion | Only in NetworkX |
| ALG-196 | Treewidth heuristics | Only in NetworkX |
| ALG-197 | Perfect graph test | Only in NetworkX |
| ALG-198 | Distance-regular and strongly regular tests | Only in NetworkX |
| ALG-199 | Regular graph test and k-factor | Only in NetworkX |
| ALG-200 | Threshold graph recognition | Only in NetworkX |
| ALG-201 | Asteroidal-triple-free test | Only in NetworkX |
| ALG-202 | Tree, forest, arborescence and branching tests | Only in NetworkX |
| ALG-207 | Directed acyclic graph to branching | Only in NetworkX |
| ALG-208 | Moral graph | Only in NetworkX |
| ALG-209 | Graph summarisation | Only in NetworkX |
| ALG-210 | Bipartite projections | Only in NetworkX |
| ALG-211 | Bipartite centrality | Only in NetworkX |
| ALG-212 | Bipartite clustering | Only in NetworkX |
| ALG-213 | Bipartite measures (density, degrees, modularity, node redundancy, spectral bipartivity, maximal extendability) | Only in NetworkX |
| ALG-214 | BiRank | Only in NetworkX |
| ALG-215 | Fast random projection (FastRP) | Graph learning and embeddings |
| ALG-216 | node2vec | Graph learning and embeddings |
| ALG-217 | HashGNN | Graph learning and embeddings |
| ALG-218 | GraphSAGE | Graph learning and embeddings |
| ALG-219 | Graph neural network link prediction | Graph learning and embeddings |
| ALG-220 | Graph neural network node classification | Graph learning and embeddings |
| ALG-221 | Temporal graph networks | Graph learning and embeddings |
| ALG-222 | Semi-supervised node classification (harmonic function, local and global consistency) | Graph learning and embeddings |
| ALG-223 | Online betweenness centrality | Dynamic algorithm |
| ALG-224 | Online community detection (label propagation) | Dynamic algorithm |
| ALG-225 | Online Katz centrality | Dynamic algorithm |
| ALG-226 | Online node2vec | Dynamic algorithm |
| ALG-227 | Online PageRank | Dynamic algorithm |
