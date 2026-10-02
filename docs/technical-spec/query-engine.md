---
title: Query engine
status: draft
last-reviewed: 2026-10-02
related:
  - architecture.md
  - catalog-and-indexes.md
  - values-and-types.md
  - quality-and-testing.md
---

# Query engine

## TS-19 Parser

- **Decision**:
  - **Grammar reference.** `GQL.g4` of opengql/grammar at commit `16ea71b` (R-GQL) is the trusted reference for GQL syntax (user decision 2026-10-02). The ISO/IEC 39075:2024 text is not available.
  - **Supplementary source.** The ISO electronic inserts in [docs/standards/iso-iec-39075/ed-1/](../standards/iso-iec-39075/ed-1/README.md) supply only what the grammar does not contain: GQLSTATUS condition codes, feature identifiers and the list of implementation-defined items. They are not the grammar authority.
  - **Parser.** A hand-written, case-insensitive lexer, a recursive-descent parser and a Pratt parser for expressions, producing a typed AST with source spans.
    - It follows the rules of `GQL.g4` clause by clause; each function names the rule it implements.
    - Each ambiguity of `GQL.g4` is resolved explicitly, and the resolution is recorded in this file as it is implemented.
  - **Narrowing.** `GQL.g4` states that it is "more permissive than a GQL implementation would be" and leaves invalid type and function combinations to the implementation. The binder therefore rejects those combinations ([TS-20](#ts-20-binder-planner-and-execution)).
  - **Test oracle.** The ANTLR-generated parser from the same `GQL.g4` ([TS-30](quality-and-testing.md#ts-30-testing-and-conformance)).
  - **Syntax errors.** Input that `GQL.g4` does not accept fails with GQLSTATUS 42001 "syntax error or access rule violation – invalid syntax" (ISO conditions insert, class 42 subclass 001), reported with the position and the expected tokens.
  - **Provisional rules.** Rules that need the ISO text are provisional (OQ-005), for example identifier character sets and every Syntax Rule.
  - **Parameters** are never inlined into the text.
- **Satisfies**: FR-GQL-001, FR-GQL-002, BR-GQL-001, BR-GQL-002, FR-API-004, NFR-VER-002, NFR-SAFE-001, NFR-DEP-001, NFR-PERF-003.
- **Evidence**:
  - Neo4j, Memgraph and Kuzu generate their parsers with ANTLR 4 (R-NEO `community/cypher/front-end/parser/v25/.../Cypher25Parser.g4`; R-MG `src/query/frontend/opencypher/grammar/Cypher.g4`; R-KUZU `src/antlr4/Cypher.g4`).
  - `GQL.g4` has 3 774 lines (R-GQL).
  - Measured with antlr4rust 0.5.2 (R-ANTLR) and the unmodified `GQL.g4` ([M-QE](../evidence/research/research-03-query-engine.md) §Q1.3; [antlr-exp](../evidence/antlr-exp/bench/src/main.rs)):
    - default LL mode: 2.9–6.8 ms per 200–290-byte MATCH query, with grammar ambiguities on 10 of 20 inputs;
    - SLL mode: 32–129 µs;
    - a hand-written SQL parser (`sqlparser`) on comparable SQL: 24–45 µs.
  - The generated parser has 113 000 lines and no `unsafe`.
- **Crates**:
  - `antlr4rust` 0.6.0 (BSD-3-Clause, 3.3 M downloads, released 2026-09-27): dev dependency of the test-only oracle crate only.
  - `sqlparser` 0.63.0: study only, as the reference for hand-written recursive descent with Pratt expressions.
  - `pest`, `peg`, `lalrpop`, `chumsky`, `nom`, `logos`: not used.
  - `gqlparser`, `drasi-query-gql`, `gqlitedb` (3–12 k downloads): too little use (NFR-DEP-001); study only.
- **Rejected**:
  - An ANTLR-generated product parser: measured parse cost, 113 000 generated lines, a Java tool chain, a generic parse tree.
  - LR generators: `GQL.g4` is ambiguous, so each conflict would need a rewrite.
  - PEG: ordered choice hides ambiguity.
  - Parser combinators: still hand-written rule by rule, plus a dependency.

## TS-20 Binder, planner and execution

- **Decision**:
  - **Binder.** One pass from the AST to a typed intermediate form.
    - Every variable is resolved to a slot, with its kind (node, edge, path, value, graph, binding table), its degree (singleton, group, conditional singleton) and its static or dynamic type.
    - Path-pattern rules are checked at bind time: normalisation, termination of unbounded quantifiers, conditional-singleton joins.
    - Type and function combinations that `GQL.g4` accepts but GQL does not allow are rejected here ([TS-19](#ts-19-parser)).
    - Predicates use three-valued logic.
    - Each optional feature of the ISO features insert maps to a gate that fails with a "feature not supported" GQLSTATUS until the feature is delivered. The ISO conditions insert has no general code for this, so the code is provisional (OQ-005).
    - Each implementation-defined item is implemented only after the user approves its choice (BR-GQL-002).
  - **Planner.**
    - Rule rewrites: predicate, projection and limit pushdown; index seeks for indexed equality and ranges; constant folding.
    - Join order: exact dynamic programming over expansions up to 7 edges, greedy beyond.
    - Cost: the sum of intermediate cardinalities from the [TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics) statistics, with defaults of 0.01 for equality and 0.1 for other predicates.
    - Operators: `NodeById`, `LabelScan`, `IndexSeek`, `Expand` (a prefix scan of the adjacency key), `ExpandInto` (a point lookup of the key `(node, direction, type, neighbour)`), `Repeat` per path restrictor, and a product-graph breadth-first search for path selectors.
    - No plan cache until a measurement shows parse and plan time matter.
  - **Execution.**
    - Pull-based, batch at a time: `next_batch` fills up to 1 024 rows held as columns of `Value`.
    - One thread per statement, through `spawn_blocking` per batch ([TS-02](architecture.md#ts-02-async-api-and-threading)). No intra-query parallelism (NFR-PERF-007).
    - Only blocking operators (sort, aggregation, hash build) hold rows, and they reserve memory in chunks from the statement's tracker.
  - **Limits and cancellation.**
    - One atomic status per transaction. It is set by Rust API cancel, Bolt `RESET`, termination, and a tokio timer. The timer measures the time limit, or `tx_timeout`, from the start of the transaction (FR-API-007).
    - Operators check it once per batch, and every K iterations in long loops.
    - Row and byte limits are counted at the result sink.
    - Crossing any limit fails the statement and aborts the transaction.
  - **EXPLAIN and PROFILE.**
    - EXPLAIN returns the plan with estimated rows per operator and does not execute.
    - PROFILE wraps each operator in a decorator that records rows, batches, time per batch, storage entries read, buffer hits and misses, and peak memory. Normal plans contain no decorator.
- **Satisfies**: FR-API-007, FR-API-008, FR-API-009, FR-API-010, FR-CAT-003, FR-GQL-002, FR-GQL-004, BR-GQL-002, FR-LIM-001, FR-LIM-002, FR-LIM-004, FR-BOLT-015, FR-BOLT-021, FR-OBS-004, FR-OBS-005, FR-OBS-006, NFR-PERF-007.
- **Evidence** (M-QE §Q2–Q4, §Q7, with file and line references):
  - Memgraph: symbol table and pull cursors; cancellation checked every 20th pull (R-MG `src/query/plan/operator.cpp:510-528`).
  - Kuzu: binder; dynamic-programming join order up to 7 relationships; cost as a sum of cardinalities; default selectivities. Kuzu materialises the whole result before returning, which fails FR-API-010.
  - Neo4j: chunked memory reservations (`LocalMemoryTracker`) and the `PROFILE` columns.
  - Measured on the reference hardware: batches of 1 024 rows were 2.3× faster than row-at-a-time in a toy pipeline, and an `Instant::now()` pair costs 2.75 µs, which is why time is read per batch, not per row.
- **Crates**: none used. DataFusion-class engines are relational and push-based; study only.
- **Rejected**:
  - Push pipelines (DuckDB, Neo4j pipelined): buffers and backpressure for no stated need.
  - Row-at-a-time execution: measured 2.3× slower.
  - Factorised vectors (Kuzu): little benefit on short reads, per Kuzu's own evaluation (D-KUZU §4.1, quoted in M-QE §Q3.1).
  - IDP join ordering (Neo4j): more complex, with no measured case that needs it.
  - Execution inline on tokio workers: page misses block.
