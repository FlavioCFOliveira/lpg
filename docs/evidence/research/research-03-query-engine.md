# Research 03: query engine evidence for the `lpg` technical specification

Date: 2026-09-24. Research only; no repository file was modified.
Builds on `research-01-storage.md` (own engine, 8 KiB pages, synchronous `pread`/`pwrite`, clock-sweep buffer manager, one memory accountant shared by cache and operators) and `research-02-layout-mvcc.md` (node tree, edge tree, clustered adjacency B+tree keyed `(node, direction, type, neighbour, edge id)`, private write sets, HyPer-style commit-time range validation).

## 0. Requirements this research is checked against

Read at `140e230` (branch `develop`): FR-API-001/005..010 (Rust API, registered procedures and functions, `CALL`, per-session/transaction limits, cooperative cancellation, streamed results pulled incrementally), FR-SES-004, FR-GQL-001..006 and BR-GQL-001/002 (full ISO/IEC 39075:2024 target, standard syntax only, implementation-defined choices decided by the user), FR-CAT-001..004 (directories, schemas, graph types, graphs; transactional catalog), FR-IDX-001..005, FR-PROC-001..004, FR-TYPE-001..007, FR-LIM-001..004, FR-OBS-005/006 (plan without execution; execution with per-operator statistics), FR-ALG-002 (algorithms via `CALL`), NFR-PERF-001/002 (LDBC SNB Interactive and BI, compared with Neo4j and Memgraph), NFR-PLAT-003 (async on tokio), NFR-SAFE-001 (`#![forbid(unsafe_code)]`), NFR-SIMP-001, NFR-DEP-001/002, NFR-VER-001/002.

## 1. Sources

Local clones (shallow; commit, commit date):

| Repository | Path | Commit | Licence |
|---|---|---|---|
| opengql/grammar | `/tmp/opengql-grammar` | `16ea71b` (2025-06-17), `version.json` 1.9.0 | Apache-2.0 |
| opengql/tck | `/tmp/opengql-tck` | `e0700a7` (2024-09-10) | Apache-2.0 |
| opencypher/openCypher | `/tmp/opencypher` | `677cbaf` (2026-03-20) | Apache-2.0 (`LICENSE`, `NOTICE`) |
| antlr4rust/antlr4 (fork with Rust target) | `/tmp/antlr4rust` | `ee89472` (2026-03-02) | BSD-3-Clause |
| neo4j/neo4j | `/tmp/neo4j` | `54a7dcf` (2026-09-14) | GPLv3 (study only) |
| memgraph/memgraph | `/tmp/memgraph` | `33813d7` (2026-09-24) | BSL-1.1 (study only) |
| kuzudb/kuzu (archived 2025-10-10) | `/tmp/kuzu` | `89f0263` (2025-10-10) | MIT |
| duckdb/duckdb (sparse checkout) | `/tmp/duckdb` | `0b9cba2` (2026-09-24) | MIT |

Papers: [GPML] A. Deutsch et al., *Graph Pattern Matching in GQL and SQL/PGQ*, SIGMOD 2022, arXiv 2112.06217v1 (read in full text, `scratchpad/deutsch-gpml.txt`). [KUZU] Feng et al., CIDR 2023 (from research-02). [HYPER] Neumann et al., SIGMOD 2015 (from research-02).

Official documentation (fetched 2026-09-24): Neo4j Cypher Manual v25 pages "Runtimes: concepts", "Execution plans", "Ordering, equality and comparison", "Graph types"; Neo4j Status Codes "42001"; tokio 1.53.1 docs (`tokio::task`, `spawn_blocking`, `block_in_place`); DuckDB blog "Runtime-Extensible SQL Parsers Using PEG" (Mühleisen, Raasveldt, 2024-11-22); A. Ryhl, "Async: What is blocking?" (2020-12-21; community source, tokio maintainer); crates.io API (2026-09-24); GitHub API for opengql repositories.

The ISO/IEC 39075:2024 text was **not** available. Clause numbers below come from the section comments of `GQL.g4` (e.g. `// 16.6 <path pattern prefix>`), which mirror the ISO clause numbering; they are not verified against the normative text (OQ-005).

Own measurements (all on this host: AMD Ryzen 9 5900HX, 16 threads, Linux, rustc 1.96.0, `--release`; clocksource `hpet`, TSC not available as clocksource). Reproduction material is in `scratchpad/antlr-exp/`, `scratchpad/tokio-exp/`, `scratchpad/exec-exp/`, `scratchpad/value-exp/`.

---

## Q1. Parser

### Q1.1 What the references use (verified in source)

| System | Parser | Evidence |
|---|---|---|
| Neo4j (Cypher 5 and Cypher 25) | ANTLR 4 (Java), one grammar per language version | `community/cypher/front-end/parser/v5/.../Cypher5Parser.g4`, `v25/.../Cypher25Parser.g4` (2 597 lines) + `Cypher25Lexer.g4` (1 723 lines) |
| Neo4j, parse strategy | SLL first with `BailErrorStrategy`; on failure re-parse with full LL and a custom `CypherErrorStrategy` "to get the correct error messages" | `common/antlr-ast-common/.../AntlrAstParser.scala:50-72` |
| Memgraph | ANTLR 4 (C++) | `src/query/frontend/opencypher/grammar/Cypher.g4`, `MemgraphCypher.g4` |
| Memgraph, parse avoidance | A hand-written lexer strips literals into parameters; the AST is cached by the stripped text (`query_ast_cache_max_size`, default 1000) and the plan is cached too | `src/query/cypher_query_interpreter.cpp:45, 104-160`; `interpreter.cpp:4102` |
| Kuzu | ANTLR 4 (C++) | `src/antlr4/Cypher.g4`, `third_party/antlr4_cypher` |
| DuckDB | Moved from the PostgreSQL-derived YACC parser to a runtime PEG (packrat) parser | `src/parser/peg/` (`parser_packrat.cpp`, `grammar/statements/*.gram`), `src/parser/parser.cpp:270, 359`; no `libpg_query` in the tree at `0b9cba2` |
| sqlparser-rs (Rust, used by DataFusion, Polars, GreptimeDB ...) | Hand-written recursive descent for statements, Pratt parser for expressions; the README states hand-written code performs better than generated code and gives better debugging | `sqlparser-0.59.0/README.md` l.160-185 |

DuckDB's own measurement (blog, 2024-11-22): its PEG parser took about 0.3 ms per query against 0.03 ms for the PostgreSQL YACC parser (10×); the authors accept this because "parsing comprises only a tiny fraction" of analytical queries. That argument does not carry over to lpg's OLTP workload (LDBC SNB Interactive short reads are sub-millisecond in [KUZU] Table 1).

### Q1.2 The opengql grammar (verified)

- `GQL.g4`: 3 774 lines, 574 parser rules, 366 token rules (218 reserved, 39 pre-reserved, remaining non-reserved words and operators), `caseInsensitive = true`.
- README: generated from the ISO BNF XML, "hand tweaked to remove ambiguities", "more permissive than a GQL implementation would be", "Not a lot of testing has been done to date".
- Samples: 14 files, 42 lines in total.
- No statement exists to create a directory, alter a graph type, or define a constraint: `DIRECTORY`, `ALTER`, `CONSTRAINT`, `PROCEDURE`, `FUNCTION` are only pre-reserved words (`GQL.g4:3496-3537`).
- No `x IN list` predicate rule exists (`IN` is only a reserved word, `GQL.g4:3366`); a test query using `IN [1,2,3]` failed to parse.

### Q1.3 Measurement: antlr4rust with the unmodified opengql grammar

Setup: ANTLR tool jar `antlr4-4.13.3-SNAPSHOT-complete.jar` from the antlr4rust v0.5.0 release; crate `antlr4rust = 0.5.2`; 20 valid inputs (the 14 samples plus 6 LDBC-like GQL queries of 78-287 bytes) and 1 invalid input; warm mean over 300-2 000 parses; ambiguity and full-context counts via `ErrorListener::report_ambiguity` / `report_attempting_full_context`. Code: `scratchpad/antlr-exp/bench/src/main.rs`.

| Measure | Result |
|---|---|
| Generated Rust | 113 264 lines (`gqlparser.rs` 82 137 lines, 2.9 MB); generated code contains no `unsafe` |
| Runtime crate | 21 `unsafe` occurrences in `runtime/Rust/src` (allowed in a dependency, but see NFR-DEP-001) |
| Build | 50 s wall, 2.3 GB peak RSS to rebuild the bench crate; stripped binary 10.5 MB (sqlparser-rs bench: 4.7 MB) |
| Default LL prediction, 200-290-byte MATCH queries | **2.9-6.8 ms** per parse (warm) |
| Inputs with true grammar ambiguities (full-LL) | **10 of 20** valid inputs; ambiguous rules: `statement` (alternatives 1 and 3: composite query vs. linear data-modifying), `labelAndPropertySetSpecification`, `insertElementPatternFiller` |
| SLL prediction (Neo4j's fast path) | 32-129 µs per parse; 3-8 µs for session commands |
| Baseline: sqlparser-rs 0.59 (hand-written RD + Pratt), equivalent SQL of 140-318 bytes | 24-45 µs per parse |
| Tool chain | Java 11+ and the fork's jar are needed to regenerate; the fork README: "I'm mostly trying to solve my problem"; crate 0.5.x |

Interpretation: the ISO-derived grammar is ambiguous (the three ambiguous rules have alternatives with the form `a? b | a b?`), so ANTLR falls back to full-context prediction on common queries and pays 50-100× the SLL cost. Even in SLL mode the ANTLR parser is 1.5-3× slower than a hand-written Rust parser on similar input, builds a generic parse tree that needs a second pass to a typed AST, and gives generic error messages (Neo4j wrote a custom error strategy for that). The sqlparser comparison is a proxy (different language, similar length); it is not a GQL hand-written parser measurement.

### Q1.4 Rust parser options

| Option | Licence, maintenance (crates.io, 2026-09-24) | Assessment |
|---|---|---|
| antlr4rust + opengql grammar | BSD-3-Clause; 0.5.2 (2025-10-25); community fork | Measured above: ambiguity cost, build weight, Java tool chain, generic tree and errors. **Rejected as the product parser.** Kept as a test oracle (Q9). |
| lalrpop (LR(1)) | MIT/Apache-2.0; 0.23.1 (2026-03-11) | The ISO BNF is not LR(1) as shown by the ambiguities; each conflict must be removed by rewriting the grammar, and error messages are LR-generic (DuckDB blog makes the same point for YACC). Not measured. **Rejected.** |
| pest / rust-peg (PEG) | pest MIT/Apache-2.0 2.9.2; peg MIT 0.8.6 | Ordered choice silently resolves ambiguity; DuckDB's README warns that "more specific alternatives must come before general ones". pest returns an untyped pair tree. DuckDB measured PEG at 10× YACC. **Rejected.** |
| chumsky / winnow / nom (combinators) | chumsky MIT 0.13.0; winnow MIT 1.0.4; nom MIT 8.0.0 | Still hand-authored rule by rule; adds a dependency and generic-heavy compile times without removing work. Not measured. **Rejected** (no requirement they serve better than plain code). |
| **Hand-written recursive descent + Pratt, hand-written lexer** | No dependency | Fastest measured family (sqlparser-rs), typed AST built directly, exact positions for GQLSTATUS errors, no tool chain, no `unsafe`. |

**Recommendation Q1: a hand-written lexer and recursive-descent parser with a Pratt expression parser, producing a typed AST with source spans.**
- Structure the parser rule by rule along the ISO clauses (7.x session commands to 21.x lexical elements, as listed in the grammar comments).
- Resolve each BNF ambiguity found in Q1.3 explicitly and record the choice in the technical specification.
- Keyword lookup: one case-insensitive match over the 218 reserved and 39 pre-reserved words; non-reserved words are identifiers in identifier position.
- Syntax errors: one GQLSTATUS code with position and expected-token set. Neo4j uses `42001` "syntax error or access rule violation - invalid syntax" (Neo4j Status Codes page); whether `42001` is the ISO code is UNVERIFIED (OQ-005).
- Parameters are never inlined, so a later plan cache can key on the text (Memgraph and Neo4j both cache; see Q3.6).

---

## Q2. Semantic analysis, binding and the type system

### Q2.1 Evidence

- **GQL variable kinds** ([GPML] §4.4, §4.6): a reference is *singleton* or *group* ("a reference is group if you have to cross a quantifier to get from the reference to the declaration"); variables bound by only one operand of a union or by `?` are *conditional singletons*; "Implicit equi-joins on conditional singletons are disallowed"; `SAME` and `ALL_DIFFERENT` require unconditional singletons.
- **Termination rule** ([GPML] §5): "Every unbounded quantifier ... must be contained in the scope of either a restrictor or a selector or both." This is a compile-time check.
- **Evaluation model** ([GPML] §6): normalisation (alternating node and edge patterns, sugar expanded, fresh variables for anonymous elements), expansion into rigid patterns, rigid-pattern matching joined on equal names, reduction and deduplication. Path pattern union `|` deduplicates; multiset alternation `|+|` does not (§4.5).
- **Prefilters vs postfilters** ([GPML] §5.2): a `WHERE` inside a selector's scope filters before selection, one outside filters after.
- **Types in the grammar** (clause 18.9, `GQL.g4:1719-1733, 1740-1898`): predefined types (boolean; character and byte strings; signed and unsigned binary integers up to 256 bits; `DECIMAL(p,s)`; `FLOAT16`..`FLOAT256`; zoned and local datetime, date, zoned and local time; two duration kinds `YEAR TO MONTH` and `DAY TO SECOND`), reference types (graph, binding table, node, edge), `PATH`, `LIST<T>`, `RECORD`, and dynamic union types `ANY VALUE`, `ANY PROPERTY VALUE`, closed unions `ANY <T1 | T2>`. Which of these are optional features is UNVERIFIED (OQ-005).
- **Truth values**: `TRUE`, `FALSE`, `UNKNOWN` literals, `IS [NOT] TRUE|FALSE|UNKNOWN`, `IS [NOT] NULL`, `NULLS FIRST|LAST` (`GQL.g4:3111-3115, 2042-2058, 1357-1360`), i.e. SQL-style three-valued logic.
- **References**:
  - Neo4j: a separate semantic phase with scopes and type checking (`front-end/ast/.../semantics/`: `SemanticState`, `SemanticPatternCheck`, `SemanticExpressionCheck`, `CypherTypeChecking`, `scoping/`), and `SemanticFeature` flags that gate features per language version.
  - Memgraph: `SymbolGenerator` assigns every variable a symbol and a frame slot (`frontend/semantic/symbol_generator.cpp`); `rw_checker.cpp` classifies read/write.
  - Kuzu: a `Binder` resolves names against the catalog and types every expression (`src/binder/`, `expression_binder.cpp`).

### Q2.2 Recommendation Q2

One binder pass from the AST to a typed logical IR, no separate rewriting of the AST.
- **Scopes follow GQL composition**: a statement's working record/binding table defines the in-scope variables; `LET`, `FOR`, `YIELD`, `NEXT`, `RETURN` and subqueries open and close scopes (clauses 9-14 per grammar comments). The binder resolves every reference to a slot index, as Memgraph's symbol table does, so execution never looks names up.
- **Each variable carries**: kind (node, edge, path, value, graph, binding table), degree (singleton, group, conditional singleton per [GPML]) and a static type.
- **Graph-pattern checks at bind time**: normalisation per [GPML] §6.2, the termination rule, the conditional-singleton join rule, prefilter/postfilter placement, label-expression and property references resolved to catalog token ids.
- **Types**: static types where the catalog gives them (closed graph types, literals, function signatures); otherwise the dynamic type `ANY PROPERTY VALUE` with a runtime type check at the operator. The binder therefore gives errors early for closed graphs and costs nothing extra for open graphs.
- **Three-valued logic**: a `Tri { True, False, Unknown }` result for every predicate; `WHERE` and filters keep only `True`. Null propagation per operator is table-driven and tested (Q9).
- **Feature gating** (FR-GQL-002): each GQL feature ID maps to a check in the binder that raises a GQLSTATUS "feature not supported" error until delivered, as Neo4j's `SemanticFeature` does. The exact code is UNVERIFIED (OQ-005).

Rejected: binding interleaved with parsing (mixes concerns and blocks the Q9 differential test of the parser); dynamic-only typing (loses compile-time errors on closed graph types, FR-GQL-004).

---

## Q3. Logical planning, optimisation and statistics

### Q3.1 Evidence

| System | Join/expansion ordering | Cost/cardinality | Statistics |
|---|---|---|---|
| Neo4j | IDP1 (Kossmann and Stocker) with `maxTableSize` and a per-iteration time limit (`idp/IDPSolver.scala:88-103, 183-186`); table threshold 128 (`GraphDatabaseInternalSettings.java:358-359`) | `CardinalityCostModel` | `GraphStatistics.scala`: node count total and per label; `patternStepCardinality(fromLabel?, type?, toLabel?)`; per-index `uniqueValueSelectivity` and `indexPropertyIsNotNullSelectivity`; histograms "only implemented for integration testing purposes". The counts API takes one label at a time on either side (`kernel-api/.../Read.java:350-411`); `estimateCountsForNode` is "fast but possibly not precise ... enough for cardinality estimation" |
| Neo4j, plan cache | Cache of 1 000 plans per database (`GraphDatabaseSettings.java:360-361`); re-plan when statistics diverge by `dbms.cypher.statistics_divergence_threshold` 0.75 after `min_replan_interval` 10 s (`:373-378`) | | |
| Memgraph | Rule-based planner; `VariableStartPlanner` generates plans from different start nodes and the cheapest wins (`plan/variable_start_planner.hpp:327-333`, `planner.hpp:137-201`); an e-graph planner is behind an experimental flag (`cypher_query_interpreter.cpp:244-259`) | Fixed cost and cardinality constants: Expand ×3, ExpandVariable ×9, Filter ×0.25 (`plan/cost_constants.hpp:30-48`) | Vertex counts per label, label+property, value; `ANALYZE GRAPH` stats: count, distinct values, average group size, average degree (`label_property_index_stats.hpp:19-21`, `label_index_stats.hpp:19-21`) |
| Kuzu | Exact DP up to 7 relationships, approximate beyond (`subplans_table.h:11`, `plan_join_order.cpp` `planLevel`); worst-case-optimal join (`Intersect`) considered when a level has ≥ 2 relationships closing on a node (`plan_join_order.cpp` `planWCOJoin`) | Cost = child cost + cardinality; hash-join build penalty 2 (`cost_model.cpp`, `constants.h:159-161`) | Node count per table, edge count per table, distinct count per column via HyperLogLog (`storage/stats/column_stats.h:19,56`); default selectivities 0.01 (equality) and 0.1 (other) |
| DuckDB | DP over connected subgraph pairs; switches to greedy operator ordering after 10 000 pairs (`optimizer/join_order/plan_enumerator.cpp:243-244, 410`) | cardinality-based | |

Variable-length and shortest-path execution:
- Neo4j: `VarExpand`, `PruningVarExpand` and `BFSPruningVarExpand` when only distinct end nodes are needed; quantified path patterns as `RepeatTrail`/`RepeatWalk`/`RepeatAcyclic` apply-style operators that run the inner pattern plan per iteration (`LogicalPlan.scala:3314-3480, 5663-5804`); selectors (`SHORTEST k`, `ALL SHORTEST`, groups) by `StatefulShortestPath` over an NFA compiled from the pattern, executed by a product-graph path-propagating BFS (`runtime-util/.../ppbfs/PGPathPropagatingBFS.java:41`, `LogicalPlan.scala:3578-3592`), rewritten to the classic bidirectional `FindShortestPaths` when the pattern allows (`StatefulShortestToFindShortestRewriter.scala`).
- Kuzu: a `RecursiveExtend` operator family for walk, trail, acyclic, shortest, all shortest and weighted variants (`cardinality_estimator.cpp:214-243`, `processor/operator/recursive_extend.cpp`).
- GQL semantics to respect ([GPML] Fig. 8): `ANY SHORTEST` (one per endpoint partition, non-deterministic), `ALL SHORTEST`, `ANY k`, `SHORTEST k`, `SHORTEST k GROUP`; restrictors `WALK`, `TRAIL`, `SIMPLE`, `ACYCLIC`; selectors apply after restrictors (§5.1). Grammar: clauses 16.6 and 16.11 (`GQL.g4:898-962, 1125-1130`).
- [KUZU] §4.1: on LDBC IS (short reads) the benefit came from sideways information passing and short expansions from a primary-key start; "factorization ... benefit is not very visible because the joins are not the dominant factor".

### Q3.2 Recommendation Q3: cost-based DP over expansions, Kuzu-style thresholds, a small statistic set

Pipeline: typed IR → normalised query graph per `MATCH` → rule rewrites → join/expansion ordering → physical plan.

1. **Rule rewrites (always applied)**: predicate pushdown into scans and expansions; label and type predicates folded into `Expand`; property equality and range on indexed keys become index seeks; projection pruning (read only the properties used); `LIMIT` and top-k pushdown; constant folding. This is Kuzu's optimizer list (`src/optimizer/`: filter, projection and limit push-down, top-k).
2. **Ordering**: exact DP over connected sub-patterns up to a threshold, greedy extension beyond (Kuzu uses 7 relationships; DuckDB switches after 10 000 pairs). The threshold is to be fixed by measurement on LDBC BI. Neo4j's IDP is the more elaborate variant; rejected until DP-plus-greedy fails a measured case.
3. **Physical operators for patterns**: `NodeById`, label scan, index seek/range, `Expand(All)` (a prefix scan of the adjacency tree on `(node, dir, type)`), and **`Expand(Into)`** when both endpoints are bound: with the research-02 key this is a point lookup on `(node, dir, type, neighbour)`, O(log n) instead of O(degree). Hash join only to connect components or when both sides are large.
4. **Cycles**: plan them with `Expand` + `Expand(Into)`. A worst-case-optimal intersection (Kuzu `Intersect`) is **deferred**: the adjacency tree is already sorted by neighbour inside a `(node, dir, type)` prefix, so a leapfrog intersection can be added later without a storage change, but only if the NFR-PERF-002 baseline shows cyclic queries failing a target.
5. **Cost model**: sum of intermediate cardinalities (Kuzu `cost_model.cpp`), plus a per-page I/O term for scans. Constants to be calibrated by measurement.
6. **Variable-length and QPP**: one `Repeat` operator family parameterised by restrictor (walk, trail, simple, acyclic), iterating the inner sub-plan per hop as Neo4j does; a distinct-end-node pruning variant when only endpoints are used; enforce restrictor state per path.
7. **Selectors**: one BFS-based operator over the product of the graph and the pattern's NFA (Neo4j PPBFS approach) serving `ANY SHORTEST`, `ALL SHORTEST`, `SHORTEST k`, `SHORTEST k GROUP`, `ANY k`; a fast path for single-edge-pattern `ANY SHORTEST` using bidirectional BFS (Neo4j `FindShortestPaths` rewrite). Frontier and visited sets reserve from the memory accountant (Q4.4).

### Q3.3 Statistics to keep

| Statistic | Used for | Reference |
|---|---|---|
| Nodes: total and per label | scan cardinality, expansion rate denominators | Neo4j `nodesWithLabelCardinality`; Kuzu table card |
| Edges: per type, per `(label, type, direction)` with the label on one side only | expansion rate = edges(label,type,dir) / nodes(label) | Neo4j counts store API (one label per side); Kuzu `getExtensionRate` |
| Per index: entry count and distinct-key count | equality selectivity = 1/distinct; `IS NOT NULL` selectivity = entries/nodes(label) | Neo4j `uniqueValueSelectivity`; Kuzu HLL; Memgraph `ANALYZE GRAPH` |
| Defaults when no statistic applies | equality 0.01, other predicates 0.1 | Kuzu `constants.h:159-160` |
| No histograms | | Neo4j uses them only in integration tests |

Persistence and freshness: statistics are estimates, as Neo4j's `estimateCountsForNode` is. Keep counters in memory, updated at commit apply with the transaction's deltas, and persist them at checkpoint. After a crash they are stale by the commits since the last checkpoint, which planning tolerates. Distinct-key counts are computed when an index is built and refreshed by sampling. Rationale: with research-01's full-page-image WAL, a durable counter page updated by every commit would log one 8 KiB image per commit and make the counter page a latch hot spot. This consequence is derived, not measured.

### Q3.4 Plan cache

Neo4j and Memgraph both cache plans (Q1.1, Q3.1). **Deferred until measured**: build it only if the baseline shows parse and plan time is a significant share of LDBC Interactive latency. The key would be the query text plus the catalog version, with invalidation on DDL and on statistics divergence.

---

## Q4. Execution model

### Q4.1 Evidence

| System | Model | Evidence |
|---|---|---|
| Neo4j slotted (Community default) | Pull, row at a time, "iterator or Volcano model", one slot per variable | Cypher Manual v25, Runtimes: concepts |
| Neo4j pipelined (Enterprise default) | Push, batches of "roughly 100 and 1000 rows" (morsels), pipelines and buffers | same page |
| Neo4j parallel | Opt-in (`CYPHER runtime = parallel`), read-only, for "analytical, graph-global queries"; may underperform on graph-local queries; candidate when a query takes longer than about 500 ms | same page |
| Memgraph | Pull cursors, `bool Pull(Frame&, ExecutionContext&)` | `plan/operator.hpp:102-131` |
| Memgraph parallelism | Opt-in via a non-standard `PARALLEL EXECUTION` hint; parallel operators only for scans, aggregation, order by | `MemgraphCypher.g4:426`; `plan/operator.hpp:200-224`; `parallel_checker.hpp:38-41` |
| Kuzu | Vectorised pull inside pipelines (`getNextTuple`), morsel-driven parallel pipelines, factorised vectors; **the whole result is materialised before return** (`processor.cpp:25-43`: schedule, wait, then `sink->getQueryResult()`) | [KUZU] §3.4 "morsel-driven parallelism" |
| DuckDB | Push pipelines, vectors of 2 048 (`vector_size.hpp:16`); streaming to the client through a bounded buffer whose sinks park when full (`main/buffered_data/simple_buffered_data.cpp:34-125`) | |

Cancellation checks:
- Memgraph checks an atomic transaction status, shutdown flag, a coarse-clock deadline and an exception flag, **every 20th call** ("Reducing the frequency of those reads will reduce their impact on performance") (`plan/operator.cpp:510-528`, `context.hpp:88-110`).
- Kuzu checks an interrupt flag on every `getNextTuple` (`physical_operator.cpp:186-189`).

Async guidance:
- tokio: "Code running in asynchronous tasks should not perform operations that can block".
- `spawn_blocking`: "Specialized CPU-bound executors, such as rayon, may also be a good fit". A running `spawn_blocking` task cannot be aborted. The blocking pool defaults to 512 threads.
- `block_in_place` "panics if called from a `current_thread` runtime".
- Community rule of thumb (Ryhl): "no more than 10 to 100 microseconds between each `.await`".
- research-01 fixed synchronous `pread`/`pwrite` for page I/O, so any page miss blocks the calling thread.

### Q4.2 Measurements

| Experiment | Result |
|---|---|
| Hand-off cost of a resumable cursor moved to `spawn_blocking` and back, per pull (`tokio-exp`) | 13.9 µs |
| Dedicated thread + `mpsc` capacity 1, per item | 13.8 µs |
| Inline on the async task + `yield_now` | 8 ns |
| Toy scan→filter→project pipeline, 50 M rows, 24-byte value enum, dynamic dispatch (`exec-exp/e.rs`) | row at a time (slotted frame) 3.02 ns/row; batch of 1 024 rows 1.32 ns/row; batch **2.3× faster** |
| `Instant::now()` + `elapsed()` pair on this host (HPET clocksource) | 2 753 ns; on TSC hosts the cost is expected to be far lower, but that is UNVERIFIED here |
| `core::arch::x86_64::_rdtsc` under `#![forbid(unsafe_code)]` | does not compile (E0133, unsafe function) |

The toy pipeline measures interpretation overhead only, not storage cost.

### Q4.3 Recommendation Q4: pull-based, batch-at-a-time, single-threaded per statement, run off the async workers

1. **Operator interface**: pull, `next_batch(&mut self, ctx, out: &mut Batch) -> Result<bool>`. A batch holds up to N rows (start with 1 024; tune by measurement) stored as columns of `Value`. This is the Volcano interface at batch granularity: Kuzu's `getNextTuple` shape, and the batch size of Neo4j's pipelined runtime without its push machinery.
   - Batches amortise per-call costs: dynamic dispatch (2.3× in the toy), cancellation checks, profiling clock reads (Q7), memory-accountant calls, and the async hand-off.
   - Pull gives FR-API-010 directly: the first batch leaves as soon as the pipeline produces it; only sort, aggregation and similar blocking operators materialise, and they are charged to the memory limit.
2. **No factorised representation.** Kuzu's own evaluation shows little benefit on LDBC IS short reads, and it adds flatten/unflatten logic to every operator. Revisit only if BI many-to-many joins fail a target.
3. **Threading**: each pull of a batch runs as a synchronous step on a blocking thread, never on a tokio worker, because a page miss blocks. `ResultStream::next().await` sends the owned cursor to `spawn_blocking`; the closure fills one batch (bounded by rows, bytes and a work budget) and returns the cursor.
   - No thread is held while the caller does not pull: this suits Bolt `PULL n` with idle clients.
   - The hand-off costs about 14 µs, about 14 ns per row at 1 024 rows.
   - A dedicated producer thread with a channel costs the same per crossing but holds a thread per open result; rejected.
   - `block_in_place` rejected because it panics on a `current_thread` runtime, which the application may use.
   - Inline execution on the async task rejected: blocking reads on tokio workers.
   - Whether a small crate-owned pool should replace `spawn_blocking` is left to measurement.
4. **Cancellation**: one `AtomicU8` status per transaction, set by the Rust API cancel, Bolt `RESET`, termination (FR-OBS-004) and a tokio timer that fires at the time limit, so the execution thread never reads a clock (clock reads cost 2.75 µs on this host). Operators check the flag once per batch and inside long loops (path search, sort) every K iterations, as Memgraph does every 20 calls. Lock waits wake on cancel (research-02 L4).
5. **Memory accounting**: operators that retain data (hash tables, sort buffers, aggregation state, distinct sets, path-search frontiers, the private write set) reserve bytes from a per-statement tracker. The tracker takes chunks from the transaction limit and the database-wide accountant (research-01 Q3), as Neo4j's `LocalMemoryTracker` batches reservations from its parent pool to "reduce contention" (`LocalMemoryTracker.java:25-31`, default grab 1 024 B; lpg's chunk size to be measured). Streaming operators reserve only their batch buffers. Byte sizes are estimates (Neo4j `HeapEstimator`); there is no global-allocator hook, because a library cannot install one for the application.
6. **Intra-query parallelism: not in the first design.** Evidence that it is needed for lpg's target workloads is missing: Neo4j keeps it opt-in and read-only for queries over about 500 ms; Memgraph keeps it opt-in for scans and aggregation. It would also require non-standard syntax (Memgraph's hint) or an API option (BR-GQL-001). Inter-query parallelism comes from concurrent sessions. The batch interface allows morsel-parallel scans and aggregations later if the LDBC BI baseline shows the need.

---

## Q5. Values, expression evaluation and property encoding

### Q5.1 Evidence

- Memgraph `PropertyStore` stores all properties of an element in one flat buffer: a 1-byte metadata field per property (4 bits type, 2 bits id size, 2 bits payload size), integers in 1/2/4/8 bytes; "approximately 10 times less memory" than a map of `PropertyValue`, at the cost of O(n) get and set (`storage/v2/property_store.cpp:63-118`). Its in-memory `PropertyValue` union is "50+ bytes".
- Tokens: Neo4j `TokenHolder` (`token-api/.../TokenHolder.java`) and Memgraph `name_id_mapper.hpp` map label, type and property names to integer ids.
- Neo4j ordering and comparison (Cypher Manual v25): global `ORDER BY` order across types `MAP, NODE, RELATIONSHIP, LIST, PATH, VECTOR, POINT, ZONED DATETIME, LOCAL DATETIME, DATE, ZONED TIME, LOCAL TIME, DURATION, STRING, BOOLEAN, UUID, numbers`; "null is ordered after all other values"; "Comparing incomparable values will throw an error"; comparisons with NaN are false. These are Neo4j rules, not GQL rules. The GQL rules for cross-type comparison and for ordering values of different types are UNVERIFIED (OQ-005; BR-GQL-002 makes the implementation-defined parts user decisions).
- GQL durations are two types (`YEAR TO MONTH`, `DAY TO SECOND`; `GQL.g4:1891-1898`), unlike Neo4j's single duration (months, days, seconds, nanoseconds).

Measured Rust layouts (`value-exp/v.rs`, `w.rs`):

| Layout | `size_of` |
|---|---|
| Enum with `Arc<str>`, `Arc<[u8]>`, `Arc<[Value]>` (fat pointers), common temporal types inline, rare types boxed | **24 B** (also `Option<Value>`) |
| Same with thin pointers (`Arc<String>`, `Arc<Vec<_>>`) | 16 B, one more indirection per string/list |
| Owned `String`/`Vec`/`BTreeMap` variants | 32 B |
| Rare types (decimal, 128/256-bit integers, points, zoned time with named zone) | 48 B, boxed |

### Q5.2 Recommendation Q5

- **Runtime value**: a 24-byte `enum Value` with inline null, boolean, `i64`, `f64`, node id, edge id, date, local datetime, local time, and `DAY TO SECOND`/`YEAR TO MONTH` durations. It holds strings, bytes, lists, records and paths as reference-counted slices, and a boxed variant for rare types (wide integers, decimals, `FLOAT16/128/256`, points, zoned values with a named time zone). Cloning a row is then reference-count increments only.
  - The 16-byte thin-pointer layout is rejected until measurement shows memory per batch matters more than the extra indirection.
  - Which numeric types need their own variant depends on the feature list (OQ-005).
- **Evaluation**: the binder compiles expressions to a tree of typed closures or small enum nodes evaluated over a whole batch column (one dispatch per batch per node). Three-valued results use `Tri`. Specialised kernels for the static types known from closed graph types; generic kernels with runtime type checks otherwise.
- **Record row format** (node and edge values in the research-02 trees): label token ids (sorted), then properties sorted by property-key token id, each a compact header (type tag, varint key-id delta, length class) plus payload, with integers in minimal width (Memgraph `PropertyStore` scheme). Values over an inline threshold go to overflow pages (research-02 L1.6). Reading one property scans the header list and decodes only the requested payloads (projection pushdown, Q3.2). Linear lookup is acceptable because typical property counts are small; measure before adding an offset table.
- **Index keys**: order-preserving byte encoding per type (research-02 L5), separate from the row format.
- **Comparison and ordering**: one total order used by `ORDER BY`, `DISTINCT` and grouping; comparison predicates return `Unknown` or raise per the GQL rules once verified. Until then every cross-type rule is an open implementation-defined choice for the user (BR-GQL-002).

---

## Q6. The GQL catalog on the same machinery

### Q6.1 Evidence

- Catalog objects in the grammar (clauses 12, 17): schemas addressed by absolute or relative directory paths (`/dir/sub/schema`, `../`), graphs and graph types inside schemas, `HOME_GRAPH`/`HOME_SCHEMA`, `CURRENT_GRAPH`, reference parameters (`GQL.g4:1381-1472`). Statements: `CREATE [OR REPLACE] [PROPERTY] GRAPH [IF NOT EXISTS] ... {ANY | OF type | LIKE graph} [AS COPY OF graph]`, `DROP GRAPH`, `CREATE/DROP GRAPH TYPE`, `CREATE/DROP SCHEMA` (`GQL.g4:301-350`).
- **No GQL statement creates a directory or alters a graph type** (`DIRECTORY` and `ALTER` are pre-reserved only). Graph types are therefore immutable once created, per the grammar; the normative confirmation is UNVERIFIED.
- Node and edge types: node type patterns with key label sets, label sets, property types (`propertyName [TYPED] valueType`), edge types with endpoint node types, directed and undirected (clauses 18.2-18.7, `GQL.g4:1501-1707`).
- Neo4j graph types: Enterprise-only, introduced 2026.02, GA from 2026.06, **open only**: "a graph type only constrains the data included by the graph type" (Cypher Manual v25, Graph types). Neo4j offers no closed-type reference.
- Kuzu requires a declared schema (node and rel tables) for all data, which is the closest reference to closed-type enforcement (catalog + binder, `src/binder/`).

### Q6.2 Recommendation Q6

- **One catalog B+tree per database**, with the research-02 immutable-entry scheme (`created_ts`, `deleted_ts`, visibility by stamp) and range validation (L3):
  - `(parent_id, name) → (object_id, kind, owner, definition)` for directories, schemas, graphs and graph types; `object_id` monotonic, never reused;
  - `(object_id) → name` reverse entry for listing and paths;
  - token tables `(kind, name) → token_id` and `(kind, token_id) → name` for labels, edge types and property keys (Neo4j/Memgraph token maps);
  - index and constraint definitions keyed by `(graph_id, index_id)`.
- **Transactional catalog (FR-CAT-004)** comes free: catalog writes join the private write set, commit with the data, and roll back with it. A `DROP GRAPH` stamps the catalog entry deleted; the graph's trees are reclaimed by GC after the horizon passes (research-02 L2.3).
- **Graph type validation at write**, only for closed graph types:
  - Graph types are immutable, so existing data never needs revalidation.
  - Each `INSERT`, `SET` and `REMOVE` is checked against the graph's type at statement end, over the elements the statement touched (the write set), not over the graph.
  - Rules: a node must match a node type (label set and property types); an edge must match an edge type, including its endpoint node types.
  - The exact matching rule (key label sets vs. full label sets, additional properties) is UNVERIFIED and must come from the ISO text.
  - Precompile each graph type into a lookup `label-set → node type` and `(type, src node type, dst node type) → edge type` held with the cached catalog snapshot.
  - Open graph types cost nothing at write.
- **Directories**: since GQL has no statement to create one, directory creation is a Rust API operation (FR-API-002). Whether that satisfies FR-CAT-001 is a question for the user (see Decisions).
- **Listing**: `show_catalog()` (FR-PROC-004) is a range scan of the catalog tree.

---

## Q7. EXPLAIN and PROFILE equivalents

### Q7.1 Evidence

- Neo4j `PROFILE` per operator (Cypher Manual v25, Execution plans): Rows, Estimated Rows, DB Hits ("entity reads, property reads, and index entry reads"), Memory (Bytes) ("Per-operator peak heap memory used"), Page Cache Hits/Misses (Enterprise), Time (pipelined/parallel runtime only), Pipeline; "profiling your query uses more resources". `EXPLAIN` "plans the query but does not run it. Only returns estimated rows."
- Memgraph: RAII `ScopedProfile` per `Pull`, counting hits and TSC cycles into a tree of `ProfilingStats` (`plan/scoped_profile.hpp`, `profile.hpp:27-34`). It is constructed only when `context.is_profile_query` (`operator.cpp:698-707`), so a non-profiled query pays one untaken branch per `Pull`.
- On this host a clock read pair costs 2.75 µs and `rdtsc` is not usable under `forbid(unsafe_code)` (Q4.2).

### Q7.2 Recommendation Q7

- **EXPLAIN (FR-OBS-005)**: return the physical plan tree with per-operator estimated rows and the operator's details (index used, direction, types, restrictor/selector) through the Rust API.
- **PROFILE (FR-OBS-006)**: plan once, then wrap every operator in a `Profiled` decorator that records, per operator:
  - rows produced and batches produced;
  - wall time per `next_batch` call (two clock reads per batch, not per row; about 5.5 µs per batch on an HPET host, much less on TSC hosts);
  - storage accesses (B+tree entries read, "db hits") and buffer-pool hits and misses, attributed through a "current operator" index in the statement context;
  - peak reserved memory from the operator's tracker.
- **Zero cost when not profiling**: the decorator is absent from normal plans. Storage and buffer counters are plain per-statement `u64` increments that FR-OBS-001 metrics need anyway.
- Time for an operator includes its children; report self time by subtraction, as Memgraph's tree does.

---

## Q8. Procedures and functions (`CALL`)

### Q8.1 Evidence

- GQL: `[OPTIONAL] CALL procedureCall`; named call `procedureReference ( args ) [YIELD ...]`; inline call with a variable scope clause; separate statement positions for catalog-modifying (12.8), data-modifying (13.6) and query (14.5) procedure calls (`GQL.g4:728-763`, clause comments).
- Neo4j: procedure `Mode` `READ`, `WRITE`, `SCHEMA`, `DBMS` (`procedure-api/.../Mode.java`). Enforcement is at run time in the kernel: the caller wraps the security context in `RestrictedAccessMode(mode, procedureMode)`, whose `allowsWrites()` is the conjunction (`kernel/.../ProcedureCaller.java:83, 112, 184`; `RestrictedAccessMode.java`).
- Memgraph: read procedures receive an immutable graph; mutation attempts return `MGP_ERROR_IMMUTABLE_OBJECT` (`procedure/mg_procedure_impl.cpp:190-192, 232-240, 2562`).

### Q8.2 Recommendation Q8

- **Registration (ABS-005, FR-PROC-003)**: per database handle, a signature per procedure: name, typed parameters (with defaults), typed result columns, and mode `ReadOnly` or `ReadWrite`. Functions add a scalar result type and a determinism flag. The binder resolves `CALL` and `YIELD` against the signature and types the result columns, like Neo4j's procedure signatures.
- **Calling convention**:
  - The procedure runs synchronously on the statement's execution thread (Q4.3), receives a context and the arguments, and returns an iterator of result rows that the `ProcedureCall` operator pulls batch by batch. Streaming therefore works for large outputs, such as algorithm results (FR-ALG-003).
  - The context gives access to the caller's transaction snapshot, the cancellation status (the procedure must poll it) and the memory tracker, to which the procedure charges retained memory.
  - Memory the application's code allocates without reserving it cannot be enforced; state this limitation in the API documentation.
- **Read-only enforcement (FR-PROC-002)**:
  1. at compile time, the read-only context type exposes no write methods;
  2. at plan time, the binder rejects a `ReadWrite` procedure inside a `READ ONLY` transaction (clause 8.2 access mode, `GQL.g4:119-122`) and in query-statement positions if the ISO syntax rules forbid it (UNVERIFIED);
  3. at run time, any GQL the procedure executes through its context is classified by the planner; a data-modifying statement from a read-only context fails with a GQLSTATUS error.

  Step 3 gives the GQLSTATUS error that FR-PROC-002 requires. Steps 1 and 2 cost nothing at run time.
- Rejected: run-time-only checks on every write call (Neo4j and Memgraph style). They work, but Rust's types remove the check from the hot path.

---

## Q9. GQL conformance testing

### Q9.1 Evidence

| Artefact | Licence | Content (measured) | Relevance |
|---|---|---|---|
| opengql/grammar `GQL.g4` + samples | Apache-2.0 | 574 parser rules; 14 samples (42 lines); "Not a lot of testing"; "more permissive"; ambiguous on 10/20 of our inputs (Q1.3) | Permitted by `CLAUDE.md` and NFR-VER-002 |
| opengql/tck | Apache-2.0 | 14 feature files, 92 scenarios (Cucumber/Gherkin), "derived from the openCypher TCK"; last commit 2024-09-10 | GQL syntax. **Not** covered by the `CLAUDE.md` exception |
| openCypher TCK | Apache-2.0 (`LICENSE`, `NOTICE`; the NOTICE restricts use of the name "Cypher") | 220 feature files, 1 615 scenarios; 1 092 (67.6 %) use `CREATE (`, `MERGE`, `WITH` or `UNWIND` in a query or setup step (keyword heuristic, `opencypher` at `677cbaf`) | Cypher, not GQL; excluded by BR-GQL-001 for execution and by `CLAUDE.md` for copying |
| MGQL (Thimmaiah, Lin, Gligoric, arXiv 2608.24565, 2026-08-25) | CC-BY-4.0 per arXiv metadata | Mechanised small-step semantics of "a substantial read-only fragment of GQL" | Possible semantic oracle; artefact language, availability and TCK validation UNVERIFIED |

### Q9.2 Recommendation Q9

1. **Parser acceptance oracle**:
   - Generate a parser from the unmodified `GQL.g4` with antlr4rust in a dev-only test crate. It is never a dependency of `lpg`.
   - Differential-test lpg's parser against it: same accept/reject decision on every input.
   - Inputs: the 14 samples, every GQL text in lpg's own tests, and sentences generated from `GQL.g4` by a small generator written in the project (walk the grammar, bounded depth), reusing the grammar file as the artefact NFR-VER-002 allows.
   - Differences are triaged against the ISO text, because the grammar is more permissive than the standard.
   - Rejected: grammarinator as the generator, since the GitHub API reports its licence as `NOASSERTION` (UNVERIFIED licence).
2. **Semantic conformance**:
   - Write lpg's own Given/When/Then scenarios, traced to ISO clause and feature ID (NFR-VER-001), for 3VL tables, null propagation, path modes and selectors ([GPML] Fig. 7-8 give the expected behaviour), and catalog and session commands.
   - A small Rust runner reads a Gherkin-like format; no Cucumber dependency is needed.
3. **opengql/tck and openCypher TCK**: not used unless the user extends the `CLAUDE.md` exception. With that exception, opengql/tck's 92 scenarios are directly relevant; the openCypher TCK would need translation to GQL for two-thirds of its scenarios.
4. **Performance conformance**: LDBC SNB Interactive and BI (NFR-PERF-002) exercise the planner, statistics and execution choices above.

---

## Summary of recommendations

| Q | Recommendation | Main rejected alternatives |
|---|---|---|
| Q1 | Hand-written lexer + recursive descent + Pratt; typed AST with spans; ambiguities resolved explicitly | antlr4rust (2.9-6.8 ms LL / 32-129 µs SLL per query, 113 k generated lines, Java tool chain); lalrpop (non-LR(1) BNF); pest/peg (ordered choice hides ambiguity, 10× in DuckDB); combinators (a dependency without removing work) |
| Q2 | One binder pass to a typed IR; scopes per GQL composition; variable kind, degree (singleton/group/conditional) and static or dynamic type; `Tri` 3VL; per-feature gating | AST rewriting in the parser; dynamic-only typing |
| Q3 | Rule rewrites + exact DP up to a measured threshold, greedy beyond; Expand/Expand(Into) on the adjacency key; `Repeat` for QPP; NFA product-graph BFS for selectors; stats = counts per label, per `(label,type,dir)`, per index entries and distinct keys; estimates persisted at checkpoint; no WCOJ and no plan cache until measured | IDP (more complex), rule-only planning, histograms, durable per-commit counters |
| Q4 | Pull, batch-at-a-time (1 024 rows, columns of `Value`), single thread per statement, each batch pulled on `spawn_blocking`; cancellation via one atomic flag set by API/Bolt/timer, checked per batch; operators reserve memory in chunks; no intra-query parallelism at first | Push pipelines (DuckDB/Neo4j pipelined: buffers plus backpressure), row-at-a-time (2.3× slower in the toy), factorisation (Kuzu), inline execution on tokio workers, `block_in_place`, dedicated thread per result |
| Q5 | 24-byte `Value` enum, rare types boxed; batch-column evaluation; Memgraph-style compact sorted property encoding in records, lazy decode; separate order-preserving index keys | 16-byte thin pointers (until measured), owned `String`/`Vec` (32 B) |
| Q6 | Catalog B+tree with stamped immutable entries; tokens; closed graph types validated at statement end over the write set; directories via Rust API | Separate catalog file or format; revalidation of whole graphs |
| Q7 | EXPLAIN = plan + estimates; PROFILE = `Profiled` decorators recording rows, batches, per-batch time, storage and buffer counters, peak memory; absent in normal plans | Per-row timing (clock cost), always-on instrumentation |
| Q8 | Typed signatures; synchronous iterator-returning procedures on the execution thread; read-only enforced by context types, plan-time checks and planner classification of nested GQL | Run-time-only write checks |
| Q9 | Differential parser testing against an antlr4rust parser generated from `GQL.g4` in a dev-only crate, plus a grammar-based sentence generator; own clause-traced scenarios | Copying opengql/tck or openCypher TCK (not permitted by `CLAUDE.md` without the user's decision) |

## Decisions for the user (product choices evidence cannot settle)

1. **Test suites**: extend the `CLAUDE.md` exception to opengql/tck (Apache-2.0, 92 GQL scenarios), and possibly to the openCypher TCK (Apache-2.0, Cypher syntax), or keep writing all scenarios in-house?
2. **Generated oracle parser**: may a parser generated from `GQL.g4` with antlr4rust (BSD-3-Clause runtime, Java tool at generation time) live in a dev-only test crate under the grammar exception?
3. **Directories**: GQL has no statement that creates a directory, so FR-CAT-001's "the application creates a directory" can only happen through the Rust API (or a built-in procedure). Which one?
4. **Implementation-defined GQL rules** needed by Q2/Q5 (BR-GQL-002): cross-type comparison and ordering, the total order used by `ORDER BY`/grouping, numeric types supported in the first release, and the GQLSTATUS codes for syntax errors, unsupported features and read-only violations. All wait for the ISO text (OQ-005).
5. **Intra-query parallelism**: accept "none in the first design, revisit after the LDBC BI baseline"? Any later opt-in needs an API option, since BR-GQL-001 forbids hints.
6. **Memory of application procedures**: accept that memory allocated by registered procedures without reserving it through the context is outside the FR-LIM-003 accounting?

## Unverified points

1. All GQL clause numbers, feature optionality, type-matching rules for closed graph types, cross-type comparison semantics and GQLSTATUS codes: from grammar comments, Neo4j and [GPML], not from ISO/IEC 39075:2024 (OQ-005).
2. Parser speed of a hand-written GQL parser: only a SQL proxy (sqlparser-rs 0.59) was measured, on one host.
3. The toy execution benchmark measures interpretation overhead only; the real ratio with B+tree access and property decoding is unmeasured.
4. Clock cost on TSC hosts; this host uses HPET (2.75 µs per clock pair). A `spawn_blocking` hand-off (13.9 µs) was measured on this host only.
5. Kuzu's statistics persistence path (stats inside the node-group collection serialised with the table, `node_table.cpp:707-713, 815-816`) was read only partially.
6. Memgraph's `plan_v2` e-graph planner: experimental status verified, its quality not assessed.
7. Neo4j pipelined-runtime internals and its speed-up over slotted: no official number found; only the model description was verified.
8. The claim that durable per-commit statistics would cost one 8 KiB WAL page image per commit is derived from research-01's WAL design, not measured.
9. MGQL artefact availability and scope; grammarinator licence (`NOASSERTION`).
10. Whether `CALL` of a data-modifying procedure inside a query statement is a syntax error in ISO GQL.
