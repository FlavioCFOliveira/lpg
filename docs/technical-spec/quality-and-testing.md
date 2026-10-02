---
title: Quality and testing
status: draft
last-reviewed: 2026-10-02
related:
  - dependencies.md
  - ../functional-spec/non-functional-requirements.md
---

# Quality and testing

## TS-29 Rust toolchain

- **Decision**: Rust edition 2024 with `std`. The minimum supported Rust version is 1.89, declared in `Cargo.toml`. It is raised only in a minor release, with a release note.
- **Satisfies**: NFR-PLAT-001, NFR-PLAT-002, NFR-REL-006.
- **Evidence**:
  - Edition 2024 needs Rust 1.85 or later.
  - `File::try_lock` ([TS-27](storage.md#ts-27-single-process-lock)) was stabilised in 1.89.0 (D-RUST).
  - `nalgebra` 0.35.0 declares `rust-version` 1.89.0 on crates.io.
  - The latest stable release is 1.98.1, of 2026-09-03 (D-RUST).
- **Rejected**: following the latest stable release, which would raise the minimum version with every Rust release.

## TS-30 Testing and conformance

- **Decision**:
  - **Parser.** The parser is tested against an oracle generated from the unmodified `GQL.g4` with antlr4rust, in a test-only crate that `lpg` does not depend on. Both must accept and reject the same inputs. Inputs:
    - the grammar's samples;
    - every GQL text in lpg's tests;
    - sentences generated from `GQL.g4` by a generator written in lpg.

    The grammar accepts more than the standard, so differences are triaged against the ISO text.
  - **Semantics.** Scenarios from opengql/tck, and those openCypher TCK scenarios that apply to GQL, run through a small Gherkin-style runner written in lpg. lpg's own scenarios cover the rest. Every scenario names its ISO clause or feature identifier. The licences and attribution of the test artefacts are kept unchanged.
  - **Durability.**
    - A fault-injecting storage implementation keeps unsynced writes in memory. On a simulated crash it drops or tears them, and it fails chosen `sync_data` calls.
    - Separately, a child process is killed with `kill -9` during a write workload on ext4 and on XFS (loop-mounted).
  - **Concurrency.** Commit latch and publish order, the lock table, deadlock detection and the index side-tree protocol are tested with `loom`.
  - **Bolt.**
    - The server endpoint is tested with the official Neo4j drivers for Java, Python, JavaScript, .NET and Go, and with the crate's own client.
    - The client endpoint is tested against the crate's server and against a Neo4j server that supports Bolt 5.0 or later.
    - `neo4rs` is excluded.
  - **Licences.** `cargo-deny` checks licences on every build of the dependency set.
  - **Performance.** The NFR-PERF-002 baseline (LDBC SNB Interactive and BI, micro-workloads; against Neo4j and Memgraph) runs on the reference hardware. Giant-graph efficiency is measured with the cache at 1 % and 10 % of the data (NFR-PERF-005, NFR-PERF-006). The M-LAYOUT harness is the starting point for the micro-workloads.
- **Satisfies**: NFR-VER-001, NFR-VER-002, NFR-VER-003, NFR-VER-004, NFR-VER-005, NFR-PLAT-004, NFR-DEP-002, FR-GQL-002, NFR-PERF-002, NFR-PERF-005, NFR-PERF-006.
- **Evidence**:
  - SQLite simulates OS crashes and power failure in a test VFS (R-SQLITE `src/test6.c`).
  - turso has a deterministic simulator and a differential oracle (R-TURSO `testing/simulator`, `testing/differential-oracle`).
  - The opengql grammar ships 14 sample files (M-QE §Q9).
  - neo4rs offers Bolt 4.x only ([S37]).
- **Crates**:
  - `antlr4rust` 0.6.0: dev dependency of the oracle crate.
  - `loom` 0.7.2 (MIT, 68.6 M downloads): dev dependency.
  - `tempfile` 3.27.0: dev dependency.
  - Cucumber runners: not used, since a small runner suffices.
- **Rejected**:
  - Power-loss tests that need root (device-mapper targets): the fault-injecting storage covers torn and lost writes without privileges.
  - Generating test sentences with grammarinator: its licence is reported as `NOASSERTION` (M-QE §Q9).

## Unverified points

1. The GQL clause numbers, feature identifiers, numeric-type optionality, comparison and ordering rules, closed-graph-type matching rules and GQLSTATUS codes. ISO/IEC 39075:2024 has not been consulted (OQ-005). They affect [TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics), [TS-19](query-engine.md#ts-19-parser), [TS-20](query-engine.md#ts-20-binder-planner-and-execution) and [TS-21](values-and-types.md#ts-21-values-and-types).
2. WAL volume and commit latency with full page images ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
3. The throughput limit of the serial apply section ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol)).
4. Behaviour of compressed leaves under concurrent writers. M-LAYOUT is single-threaded, without latches, WAL or checksums ([TS-06](storage.md#ts-06-graph-layout)).
5. The minimum pool size of 1 % of the cap, the `std` latches compared with `parking_lot`, and the cost of double caching in the operating system ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
6. The index side-tree protocol ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints)).
7. Parse speed of the hand-written GQL parser. Only a SQL proxy was measured ([TS-19](query-engine.md#ts-19-parser)).
8. Which inventory entries have a parallel version in GDS, MAGE, petgraph or graphalgs (not yet classified, [TS-25](algorithms.md#ts-25-graph-algorithms)); determinism statements of GDS for the algorithms listed in [algorithms.md](algorithms.md#ts-25-graph-algorithms), and convergence of synchronous rounds.
9. Bolt 6.1 changes ([bolt.md](bolt.md)).
10. Whether `cargo-deny` accepts the older licence notation of `crc32c` (`Apache-2.0/MIT`) and `csv` (`Unlicense/MIT`) without configuration.
11. Whether Neo4j's `point` header accepts a `srid` key ([TS-26](operations.md#ts-26-backup-restore-import-and-export)).
12. M-LAYOUT limits: one host, synthetic graphs without properties, no prefetch. The 100 M point covers bulk load of two layouts only.
