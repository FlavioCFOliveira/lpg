---
title: Technical specification
status: draft
last-reviewed: 2026-10-02
related:
  - ../../README.md
  - ../functional-spec/README.md
---

# Technical specification

This folder states how the `lpg` crate is built. The [functional specification](../functional-spec/README.md) states what it does and is authoritative on requirements; this folder does not restate them.

## Files

| File | Content | Decisions |
|---|---|---|
| [architecture.md](architecture.md) | Components, crate layout, threads, the async boundary. | TS-01, TS-02 |
| [interfaces.md](interfaces.md) | The traits the application implements and the public API shape. | TS-03, TS-23, TS-24 |
| [storage.md](storage.md) | Files, pages, graph layout, identifiers, buffer pool, checksums, the process lock. | TS-04 to TS-08, TS-12, TS-27 |
| [transactions-and-recovery.md](transactions-and-recovery.md) | WAL, torn pages, checkpoints, recovery, MVCC, serializable isolation, locks, commit. | TS-09 to TS-11, TS-13 to TS-16 |
| [catalog-and-indexes.md](catalog-and-indexes.md) | Catalog, statistics, indexes, constraints. | TS-17, TS-18 |
| [query-engine.md](query-engine.md) | Parser, binder, planner, execution, limits, EXPLAIN and PROFILE. | TS-19, TS-20 |
| [values-and-types.md](values-and-types.md) | Runtime values, temporal and spatial types, encodings. | TS-21 |
| [bolt.md](bolt.md) | Bolt and PackStream endpoints. | TS-22 |
| [algorithms.md](algorithms.md) | Graph-algorithm runs, projection, parallelism, determinism. | TS-25 |
| [operations.md](operations.md) | Backup, restore, import, export, format migration. | TS-26, TS-28 |
| [quality-and-testing.md](quality-and-testing.md) | Toolchain, tests, conformance, measurement, unverified points. | TS-29, TS-30 |
| [dependencies.md](dependencies.md) | The dependency set and the crates considered. | TS-31 |

Start with [architecture.md](architecture.md).

## Conventions

- **Decision identifiers.** `TS-NN`, stable once assigned, never reused. Each decision lives in one file and is cited elsewhere by link.
- **Decision form.** Each decision states: the decision; the requirements it satisfies (`FR-`, `NFR-`, `BR-`, `ABS-` identifiers of the functional specification); the evidence; the Rust crates assessed, each as a dependency or study only; the rejected options, one line each.
- **Basis.** Every decision follows the "Research and decision-making" procedure of the project's `CLAUDE.md`: it rests on the recurring approach of the reference projects for that area, on official documentation, or on a measurement listed below.
- **Coverage.** Every requirement of the functional specification is traced by at least one decision, except these release and process requirements, which need no technical decision: FR-GQL-003, FR-GQL-006, NFR-PERF-001, NFR-REL-001, NFR-REL-003, NFR-REL-005 (and the withdrawn FR-REC-009).
- **Unverified.** A claim not confirmed in a source or a measurement is marked "unverified". The full list is in [quality-and-testing.md](quality-and-testing.md#unverified-points).
- **Simplicity.** Only what a requirement needs is specified ([NFR-SIMP-001](../functional-spec/non-functional-requirements.md#simplicity)). An option, setting or extension point appears only when a requirement or a measurement demands it.
- **Spelling.** British English ([NFR-DOC-001](../functional-spec/non-functional-requirements.md#documentation)).
- **No copying.** Reference projects were read for study only; no code or text was copied ([`CLAUDE.md`](../../CLAUDE.md), "Open-source references").

## Terms

Functional terms are in the [functional glossary](../functional-spec/glossary.md). Technical terms used here:

- **Frame**: a buffer-pool slot that holds one page in memory.
- **Group**: the unit of durability in the WAL: page images of one or more commits plus one commit frame ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
- **LSN**: log sequence number, the byte position of a record in the WAL.
- **NO-STEAL**: the rule that uncommitted data is never written to shared pages.
- **Redo point**: the LSN from which recovery replays the WAL ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)).
- **Stamp**: the `created_ts` and `deleted_ts` commit timestamps carried by an immutable entry ([TS-14](transactions-and-recovery.md#ts-14-mvcc)).

## Sources

Checked between 2026-09-24 and 2026-10-02.

### Reference source code (shallow clones, read for study only)

| ID | Project | Commit (date) | Licence |
|---|---|---|---|
| R-NEO | Neo4j community, <https://github.com/neo4j/neo4j> | `54a7dcf` (2026-09-14) | GPLv3 |
| R-MG | Memgraph, <https://github.com/memgraph/memgraph> | `33813d7` (2026-09-24) | BSL-1.1 |
| R-PG | PostgreSQL, <https://github.com/postgres/postgres> | `8982935` (2026-09-24) | PostgreSQL |
| R-INNO | MySQL InnoDB (`storage/innobase`), <https://github.com/mysql/mysql-server> | `a1ef44f` (2026-09-24) | GPLv2 |
| R-KUZU | Kuzu (archived 2025-10-10), <https://github.com/kuzudb/kuzu> | `89f0263` (2025-10-10) | MIT |
| R-SQLITE | SQLite, <https://github.com/sqlite/sqlite> | `c62a443` (2026-09-24) | public domain |
| R-DUCK | DuckDB, <https://github.com/duckdb/duckdb> | `0b9cba2` (2026-09-24) | MIT |
| R-REDB | redb, <https://github.com/cberner/redb> | `7677a88` (2026-09-23) | MIT/Apache-2.0 |
| R-FJALL | fjall, <https://github.com/fjall-rs/fjall> | `3adaa50` (2026-08-30) | MIT/Apache-2.0 |
| R-TURSO | turso, <https://github.com/tursodatabase/turso> | `300b692` (2026-10-02) | MIT |
| R-NEO4RS | neo4rs, <https://github.com/neo4j-labs/neo4rs> | `1a35661` (2026-09-25) | MIT |
| R-PETGRAPH | petgraph, <https://github.com/petgraph/petgraph> | `a4d94bd` (2026-09-13) | MIT/Apache-2.0 |
| R-RWX | rustworkx, <https://github.com/Qiskit/rustworkx> | `25398ab` | Apache-2.0 |
| R-GALGS | graphalgs, <https://github.com/starovoid/graphalgs> | `1f8a412` | MIT |
| R-ANTLR | antlr4rust/antlr4, <https://github.com/antlr4rust/antlr4> | `9b5bd1e` (2026-09-27) | BSD-3-Clause |
| R-GQL | opengql/grammar, <https://github.com/opengql/grammar> | `16ea71b` (2025-06-17), v1.9.0 | Apache-2.0 |

### Official documentation

| ID | Source |
|---|---|
| D-HS | Neo4j Bolt, "Handshake": <https://neo4j.com/docs/bolt/current/bolt/handshake/> |
| D-BC | Neo4j Bolt, "Bolt protocol and Neo4j compatibility": <https://neo4j.com/docs/bolt/current/bolt-compatibility/> |
| D-SS | Neo4j Bolt, "Structure semantics": <https://neo4j.com/docs/bolt/current/bolt/structure-semantics/> |
| D-CFG | Neo4j Operations Manual 2026.09, "Configuration settings": <https://neo4j.com/docs/operations-manual/current/configuration/configuration-settings/> |
| D-PRIV | Neo4j Operations Manual 2026.09, read, write and database privileges: <https://neo4j.com/docs/operations-manual/current/authentication-authorization/> |
| D-HDR | Neo4j Operations Manual 2026.09, "Import: header format": <https://neo4j.com/docs/operations-manual/current/import/header-format/> |
| D-APOC | APOC 2026.09, `apoc.export.json` and `apoc.export.graphml`: <https://neo4j.com/docs/apoc/current/export/> |
| D-GML | GraphML Primer: <http://graphml.ethz.ch/primer/graphml-primer.html> |
| D-GDS | Neo4j Graph Data Science 2026.09: "Graph management", "Running algorithms", and the pages of KNN, Approximate Maximum k-cut, SLLPA, Label Propagation and Betweenness Centrality: <https://neo4j.com/docs/graph-data-science/current/> |
| D-JIFF | jiff 0.2.37, `TimeZoneDatabase`: <https://docs.rs/jiff/0.2.37/jiff/tz/struct.TimeZoneDatabase.html> |
| D-CTZ | chrono-tz 0.10.4: <https://docs.rs/chrono-tz/0.10.4/chrono_tz/> |
| D-TIME | time 0.3.55: <https://docs.rs/time/0.3.55/time/> |
| D-TOKIO | tokio 1.53.1, `task::spawn_blocking`: <https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html> |
| D-RAYON | rayon 1.12.0, `ParallelIterator::reduce` and `sum`: <https://docs.rs/rayon/1.12.0/rayon/iter/trait.ParallelIterator.html> |
| D-AFIT | Rust blog, "Announcing `async fn` and return-position `impl Trait` in traits" (2023-12-21): <https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/> |
| D-METRICS | metrics 0.24.6: <https://docs.rs/metrics/0.24.6/metrics/> |
| D-RUST | rust-lang/rust `RELEASES.md` (Rust 1.89.0 stabilises `File::lock` and `File::try_lock`; latest stable 1.98.1, 2026-09-03) |
| D-HYPER | T. Neumann, T. Mühlbauer, A. Kemper, "Fast Serializable Multi-Version Concurrency Control for Main-Memory Database Systems", SIGMOD 2015 ([S42] of the functional specification) |

### Measurements and research notes

Kept in the repository under [`docs/evidence/`](../evidence/README.md), which also says how to reproduce each item.

| ID | File | Content |
|---|---|---|
| M-LAYOUT | [evidence/layout-bench/results.md](../evidence/layout-bench/results.md) (2026-10-02), with raw outputs and source in [evidence/layout-bench/](../evidence/layout-bench/) | Rust benchmark of seven adjacency layouts on the reference hardware ([NFR-PERF-006](../functional-spec/non-functional-requirements.md#performance)): 1 M nodes / 10 M edges (Zipf and uniform, online and bulk) at 100 %, 25 %, 10 % and 1 % cache; 10 M nodes / 100 M edges for two layouts. |
| M-QE | [evidence/research/research-03-query-engine.md](../evidence/research/research-03-query-engine.md) (2026-09-24), with the experiments [antlr-exp](../evidence/antlr-exp/), [tokio-exp](../evidence/tokio-exp/), [exec-exp](../evidence/exec-exp/), [value-exp](../evidence/value-exp/) | Query-engine research; antlr4rust parse times on `GQL.g4`; `spawn_blocking` hand-off cost; batch versus row execution; value sizes; clock cost. |
| M-LOCK | [evidence/research/research-01-storage.md](../evidence/research/research-01-storage.md) §Q8 and [evidence/locktest/](../evidence/locktest/main.rs) (2026-09-24) | `File::try_lock` behaviour within and across processes. |
| P-STO | [evidence/research/research-01-storage.md](../evidence/research/research-01-storage.md) (2026-09-24) | Storage evidence with file and line references. |
| P-TX | [evidence/research/research-02-layout-mvcc.md](../evidence/research/research-02-layout-mvcc.md) (2026-09-24), with the page model [evidence/layout-sim/](../evidence/layout-sim/pagesim.py) | Layout, MVCC, validation, locks and indexes evidence with file and line references. |
