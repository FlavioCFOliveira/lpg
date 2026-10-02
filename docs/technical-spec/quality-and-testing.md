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
  - Edition 2024 needs Rust 1.85 or later ("The 2024 Edition is now stable", Rust 1.85.0 release notes, D-RUST).
  - `File::try_lock` ([TS-27](storage.md#ts-27-single-process-lock)) was stabilised in 1.89.0 (D-RUST).
  - `nalgebra` 0.35.0 declares `rust-version` 1.89.0 on crates.io.
  - The latest stable release is 1.98.1, of 2026-09-03 (D-RUST).
- **Rejected**: following the latest stable release, which would raise the minimum version with every Rust release.

## TS-30 Testing and conformance

- **Decision**:
  - **Parser.**
    - An oracle generated with antlr4rust from the unmodified `GQL.g4` of opengql/grammar, the trusted syntax reference ([TS-19](query-engine.md#ts-19-parser)), lives in a test-only crate that `lpg` does not depend on.
    - lpg's parser must accept and reject the same inputs as the oracle. Inputs:
      - the grammar's samples;
      - every GQL text in lpg's tests and in the conformance scenarios;
      - sentences generated from `GQL.g4` by a generator written in lpg.
    - A difference is a defect in lpg's parser, unless it is a type or function combination that the binder rejects on purpose ([TS-20](query-engine.md#ts-20-binder-planner-and-execution)).
  - **Semantics.** Scenarios from opengql/tck, and those openCypher TCK scenarios that apply to GQL, run through a small Gherkin-style runner written in lpg. lpg's own scenarios cover the rest. Every scenario names its clause or its feature identifier from the ISO features insert [S45]. Expected GQLSTATUS codes come from the ISO conditions insert, or from Neo4j where it has none ([interfaces.md](interfaces.md#public-api-shape)). The licences and attribution of the test artefacts are kept unchanged.
  - **Durability.**
    - A fault-injecting storage implementation keeps unsynced writes in memory. On a simulated crash it drops or tears them, and it fails chosen `sync_data` calls.
    - Separately, a child process is killed with `kill -9` during a write workload on ext4 and on XFS. On the reference machine the tests use the ext4 filesystem and an XFS directory that the user mounts and provides; the tests run without privileges.
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
  - Power-loss tests that need root (device-mapper targets) and loop mounts: the reference machine has no root access ([M-LAYOUT](../evidence/layout-bench/results.md) "Method"); the fault-injecting storage covers torn and lost writes without privileges.
  - Generating test sentences with grammarinator: its licence is reported as `NOASSERTION` (M-QE §Q9).

## Unverified points

1. Everything that needs the ISO/IEC 39075:2024 text, which is not available (OQ-005): Syntax, General and Conformance Rules; the content of minimum conformance; closed-graph-type matching; cross-type comparison and ordering; numeric types beyond 64 bits; the permission for GQLSTATUS 25N11 and a "feature not supported" code; the validity of procedure names. These affect [TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics), [TS-19](query-engine.md#ts-19-parser), [TS-20](query-engine.md#ts-20-binder-planner-and-execution), [TS-21](values-and-types.md#ts-21-values-and-types), [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events) and [TS-25](algorithms.md#ts-25-graph-algorithms).
2. WAL behaviour with full page images ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)): volume and commit latency; WAL bytes between checkpoints, since the triggers count transactions; WAL growth while a backup holds recycling; recovery time.
3. The throughput of the serial commit section, now including validation and the page copies of step 6 ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol)).
4. Compressed leaves with readers running concurrently with the single committer, the garbage collector, the index builder and the fast load ([TS-06](storage.md#ts-06-graph-layout)).
5. The minimum pool size (provisionally 1 % of the cap), `std` latches compared with `parking_lot`, and the cost of double caching in the operating system ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
6. The free-page reserve estimate and the 64 MiB extent size ([TS-05](storage.md#ts-05-files-and-pages)); derived, not measured.
7. The index side-tree protocol with its idempotent replay ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints)).
8. Parse speed of the hand-written GQL parser; only a SQL proxy was measured ([TS-19](query-engine.md#ts-19-parser)).
9. The parallel classification of every inventory entry other than ALG-024 and ALG-042; the GDS determinism statements for the algorithms listed in [algorithms.md](algorithms.md#ts-25-graph-algorithms); convergence of synchronous rounds.
10. Bolt 6.1 changes ([bolt.md](bolt.md)).
11. Whether `cargo-deny` accepts the older licence notation of `crc32c` (`Apache-2.0/MIT`) and `csv` (`Unlicense/MIT`) without configuration.
12. Whether Neo4j's `point` header accepts a `srid` key, and whether GraphML readers accept an extension attribute on `<data>` and `<key>` ([TS-26](operations.md#ts-26-backup-restore-import-and-export)).
13. The M-LAYOUT limits: one host; synthetic graphs without properties; misses read with `O_DIRECT` and no read-ahead, while lpg reads through the operating-system cache; a 100 M point for bulk load of two layouts only.
14. Taken from references, not measured: the 2 048-byte overflow threshold and the 1 024-byte key limit ([TS-05](storage.md#ts-05-files-and-pages)); the shared-prefix key encoding of trees other than the adjacency tree ([TS-06](storage.md#ts-06-graph-layout)); the 1 MiB copy buffer ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
15. The real peak memory of one database against its cap, including WAL page images and the free-page set outside the cap; the arithmetic worst case is about 3× the cap ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
