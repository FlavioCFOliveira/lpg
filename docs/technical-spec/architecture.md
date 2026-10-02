---
title: Architecture
status: draft
last-reviewed: 2026-10-02
related:
  - README.md
  - interfaces.md
  - storage.md
  - transactions-and-recovery.md
  - query-engine.md
---

# Architecture

## Overview

`lpg` is one Rust library crate that the application links into its process ([scope](../functional-spec/scope.md)). It has no server, no sockets and no global state shared between database handles. Each open database is an independent engine instance with its own files, threads and memory cap.

## Components

| Component | Responsibility | Specified in |
|---|---|---|
| API | Database, session, transaction and result handles; async boundary | [TS-02](#ts-02-async-api-and-threading), [interfaces.md](interfaces.md) |
| GQL front end | Lexer, parser, binder, planner | [query-engine.md](query-engine.md) |
| Executor | Pull-based batch operators, limits, cancellation, EXPLAIN and PROFILE | [TS-20](query-engine.md#ts-20-binder-planner-and-execution) |
| Transactions | MVCC, read-set validation, lock table, commit | [transactions-and-recovery.md](transactions-and-recovery.md) |
| Catalog and indexes | Catalog tree, tokens, statistics, indexes, constraints | [catalog-and-indexes.md](catalog-and-indexes.md) |
| Storage | Storage trait, page file, buffer pool, B+trees, graph layout | [storage.md](storage.md) |
| Durability | WAL, checkpoint, recovery | [transactions-and-recovery.md](transactions-and-recovery.md) |
| Values | Runtime values, encodings, temporal and spatial types | [values-and-types.md](values-and-types.md) |
| Procedures | Registry, built-in procedures, application procedures | [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events) |
| Algorithms | Snapshot projection, algorithm library | [algorithms.md](algorithms.md) |
| Operations | Backup, restore, import, export, migration | [operations.md](operations.md) |
| Bolt (feature `bolt`) | Server and client endpoints, PackStream | [bolt.md](bolt.md) |

A request flows: API or Bolt → parser → binder → planner → authorisation ([TS-23](interfaces.md#ts-23-authentication-and-authorisation)) → executor → transactions → storage. A commit flows: validation → apply → WAL group → publish ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol)).

## TS-01 Crate layout

- **Decision**: one crate `lpg` with `#![forbid(unsafe_code)]` at its root. The Bolt endpoints are compiled only with the Cargo feature `bolt`, which is off by default. Internal modules: `api`, `session`, `tx`, `storage`, `catalog`, `index`, `gql`, `types`, `proc`, `algo`, `ops`, `obs`, `bolt`. The algorithm library is always compiled. The test-only parser oracle is a separate, unpublished crate in the same repository ([TS-30](quality-and-testing.md#ts-30-testing-and-conformance)).
- **Satisfies**: FR-API-003, NFR-REL-002, NFR-SAFE-001, FR-ALG-005, NFR-VER-002.
- **Evidence**: redb, fjall and neo4rs each publish one crate (R-REDB, R-FJALL, R-NEO4RS).
- **Rejected**: several published crates (engine, Bolt, algorithms): NFR-REL-002 names one crate and no requirement needs a split.

## TS-02 Async API and threading

- **Decision**:
  - The engine core is synchronous. The public API is `async` and runs on the application's tokio runtime.
  - Statement work runs on tokio's blocking pool: each pull of a result batch moves the statement's cursor into `tokio::task::spawn_blocking`, fills one batch and returns ([TS-20](query-engine.md#ts-20-binder-planner-and-execution)). No thread is held while the caller does not pull.
  - Each database handle owns three `std::thread`s: the WAL writer ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)), the checkpointer ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)) and the garbage collector ([TS-14](transactions-and-recovery.md#ts-14-mvcc)). Closing the handle stops them after a final checkpoint; dropping it stops them without one, which recovery treats as a crash (FR-DB-009).
  - Algorithm runs use their own scoped worker threads ([TS-25](algorithms.md#ts-25-graph-algorithms)).
  - Handles: `Database`, `Session`, `Transaction`, `ResultStream` with `async fn next()`. The API shape is in [interfaces.md](interfaces.md#public-api-shape).
  - `Database` is a cheap clone of one shared engine. `close` acts on the engine, and afterwards every clone fails. "Dropped without close" (FR-DB-009) means that the last clone was dropped while the engine was open.
  - A session holds an atomic "busy" flag. A request that arrives while another runs fails with a GQLSTATUS error (FR-SES-004).
  - Session and transaction statements in GQL text, and the matching API calls, reach the same session and transaction functions (FR-GQL-005).
- **Satisfies**: NFR-PLAT-003, FR-API-001, FR-API-002, FR-API-010, FR-DB-002, FR-DB-008, FR-DB-009, FR-GQL-005, FR-SES-004, NFR-PERF-007.
- **Evidence**: tokio states that a running `spawn_blocking` task cannot be aborted and recommends a separate executor or a limit for many CPU-bound jobs (D-TOKIO). Memgraph pulls rows through synchronous cursors (R-MG `src/query/plan/operator.hpp:102-131`), and Neo4j's community runtime is a pull, row-at-a-time iterator model (Cypher Manual, "Runtimes: concepts"; [M-QE](../evidence/research/research-03-query-engine.md) §Q4.1). Measured on the reference hardware (M-QE §Q4.2): one `spawn_blocking` hand-off costs 13.9 µs; a thread with a channel costs 13.8 µs per item; at 1 024 rows per batch the hand-off costs about 14 ns per row.
- **Crates**: tokio is a dependency ([TS-31](dependencies.md#ts-31-dependency-set)). `futures` (for a `Stream` trait) is not used: `next()` follows neo4rs `RowStream::next` (R-NEO4RS).
- **Rejected**:
  - An async engine throughout: page misses are blocking `pread` calls ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
  - A dedicated thread per open result: same hand-off cost, and it holds a thread while a Bolt client is idle.
  - `tokio::task::block_in_place`: it panics on a `current_thread` runtime (tokio documentation, quoted in M-QE §Q4.1), which the application may use.
