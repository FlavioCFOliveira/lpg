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
  - A hand-written, case-insensitive lexer, a recursive-descent parser and a Pratt parser for expressions, producing a typed AST with source spans.
  - The parser follows the productions of `GQL.g4` clause by clause. Each ambiguity of that grammar is resolved explicitly, and the resolution is recorded in this file as it is implemented.
  - Syntax that ISO/IEC 39075 does not define is a syntax error. A syntax error carries one GQLSTATUS code, the position and the expected tokens; the code is unverified (OQ-005).
  - Parameters are never inlined into the text.
- **Satisfies**: FR-GQL-001, FR-GQL-002, BR-GQL-001, FR-API-004, NFR-SAFE-001, NFR-DEP-001, NFR-PERF-003.
- **Evidence**:
  - Neo4j, Memgraph and Kuzu generate their parsers with ANTLR 4 (R-NEO `community/cypher/front-end/parser/v25/.../Cypher25Parser.g4`; R-MG `src/query/frontend/opencypher/grammar/Cypher.g4`; R-KUZU `src/antlr4/Cypher.g4`).
  - `GQL.g4` has 3 774 lines. Its README calls it "more permissive" than an implementation would be (R-GQL).
  - Measured with antlr4rust 0.5.2 and the unmodified grammar (M-QE §Q1.3):
    - default LL mode: 2.9–6.8 ms per 200–290-byte MATCH query, with grammar ambiguities on 10 of 20 inputs;
    - SLL mode: 32–129 µs;
    - a hand-written SQL parser (`sqlparser`) on comparable SQL: 24–45 µs.
  - The generated parser has 113 000 lines and no `unsafe`.
- **Crates**:
  - `antlr4rust` 0.6.0 (BSD-3-Clause, 3.3 M downloads, released 2026-09-27): dev dependency of the test-only oracle crate only ([TS-30](quality-and-testing.md#ts-30-testing-and-conformance)).
  - `sqlparser` 0.63.0: study only, as the reference for hand-written recursive descent with Pratt expressions.
  - `pest`, `peg`, `lalrpop`, `chumsky`, `nom`, `logos`: not used.
  - `gqlparser`, `drasi-query-gql`, `gqlitedb` (3–12 k downloads): too little use (NFR-DEP-001); study only.
- **Rejected**:
  - An ANTLR-generated product parser: measured parse cost, 113 000 generated lines, a Java tool chain, a generic parse tree.
  - LR generators: the ISO grammar is ambiguous, so each conflict would need a rewrite.
  - PEG: ordered choice hides ambiguity.
  - Parser combinators: still hand-written rule by rule, plus a dependency.

## TS-20 Binder, planner and execution

- **Decision**:
  - **Binder.** One pass from the AST to a typed intermediate form.
    - Every variable is resolved to a slot, with its kind (node, edge, path, value, graph, binding table), its degree (singleton, group, conditional singleton) and its static or dynamic type.
    - Path-pattern rules are checked at bind time: normalisation, termination of unbounded quantifiers, conditional-singleton joins.
    - Predicates use three-valued logic.
    - Each GQL feature maps to a gate that raises a "feature not supported" GQLSTATUS until the feature is delivered.
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
    - One atomic status per transaction. It is set by Rust API cancel, Bolt `RESET`, termination, and a tokio timer at the time limit or `tx_timeout`.
    - Operators check it once per batch, and every K iterations in long loops.
    - Row and byte limits are counted at the result sink.
    - Crossing any limit fails the statement and aborts the transaction.
  - **EXPLAIN and PROFILE.**
    - EXPLAIN returns the plan with estimated rows per operator and does not execute.
    - PROFILE wraps each operator in a decorator that records rows, batches, time per batch, storage entries read, buffer hits and misses, and peak memory. Normal plans contain no decorator.
- **Satisfies**: FR-API-007, FR-API-008, FR-API-009, FR-API-010, FR-CAT-003, FR-GQL-002, FR-GQL-004, FR-LIM-001, FR-LIM-002, FR-LIM-004, FR-BOLT-015, FR-BOLT-021, FR-OBS-004, FR-OBS-005, FR-OBS-006, NFR-PERF-007.
- **Evidence** (M-QE §Q2–Q4, §Q7, with file and line references):
  - Memgraph: symbol table and pull cursors; cancellation checked every 20th pull (R-MG `src/query/plan/operator.cpp:510-528`).
  - Kuzu: binder; dynamic-programming join order up to 7 relationships; cost as a sum of cardinalities; default selectivities. Kuzu materialises the whole result before returning, which fails FR-API-010.
  - Neo4j: chunked memory reservations (`LocalMemoryTracker`) and the `PROFILE` columns.
  - Measured on the reference hardware: batches of 1 024 rows were 2.3× faster than row-at-a-time in a toy pipeline, and an `Instant::now()` pair costs 2.75 µs, which is why time is read per batch, not per row.
- **Crates**: none used. DataFusion-class engines are relational and push-based; study only.
- **Rejected**:
  - Push pipelines (DuckDB, Neo4j pipelined): buffers and backpressure for no stated need.
  - Row-at-a-time execution: measured 2.3× slower.
  - Factorised vectors (Kuzu): little benefit on short reads, per Kuzu's own evaluation.
  - IDP join ordering (Neo4j): more complex, with no measured case that needs it.
  - Execution inline on tokio workers: page misses block.
