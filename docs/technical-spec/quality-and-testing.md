---
title: Quality and testing
status: draft
last-reviewed: 2026-10-03
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
    - **Fault-injecting storage** (`src/storage/fault.rs`). It keeps in memory the unsynced writes, and the namespace changes not yet covered by a `sync_dir`. On a simulated crash it drops the unsynced writes, keeps a seeded subset of them, or tears them at 512-byte sectors; unsynced namespace changes are always lost.
    - **Faults** (R2, R9). EIO on the *n*-th call of `read_at`, `write_at`, `sync_data`, `create`, `rename`, `remove` and `sync_dir`. ENOSPC from a space budget. A misdirected write, which puts a page at another page's offset. A lost write, which drops a synced write.
    - **Failed `sync_data`** (R8). The file's unsynced writes stop being pending: a later `sync_data` succeeds without making them durable, and a crash loses them. This is the write-back failure PostgreSQL documents, where the data may be "entirely forgotten" and a retry "may be reported as successful" (R-PG `doc/src/sgml/config.sgml:12258-12264`).
    - **Crash-point sweep** (R1). A small fixed workload (commits, one checkpoint, one segment creation) is crashed after the *k*-th call of `write_at`, `sync_data`, `rename` and `sync_dir`, for every *k*, in every crash mode.
    - **Nth-failure sweep** (R2). The same workload fails the *n*-th call of each kind, for every *n*, with EIO and with ENOSPC.
    - **Oracle of both sweeps.** ENOSPC fails only the commit that needed the space, and the database stays usable. Any other failure sets the unusable latch. After a reopen, the state equals the state after a prefix of the commits that includes every acknowledged commit, and the integrity check ([TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check)) passes.
    - **Checkpoint order** (R3). The call log pins the order of [TS-11](transactions-and-recovery.md#ts-11-checkpoint): data `sync_data`, then the `CONTROL` write and its `sync_data`, then each recycling `rename` or `remove` with its `sync_dir`.
    - **Seeds** (R5). Every seeded fault test draws a random seed unless the environment variable `LPG_SEED` is set, and prints the seed when it fails.
    - **Pause and fail points** (R10). Test-only points (`cfg(test)`) between the apply and the join of a commit, inside the checkpoint's revisit loop, and inside segment creation make interleavings deterministic. They replace sleeps and polling. A test installs a hook that runs at a named point.
      - In the code: `segment_created`, after a segment is created and its name is durable, and `before_close`, in the WAL writer after its health check and before it takes the commit latch to close a group (`TestPoints`, `src/storage/wal.rs:500-526`; run at `wal.rs:678` and `wal.rs:813`). Their first user is `audit_a_failed_capture_never_reaches_the_wal` (`src/tx/commit/tests.rs:707`).
      - Not in the code yet: the points between the apply and the join of a commit and inside the checkpoint's revisit loop.
    - **Process crash.** A child process is killed with `kill -9` during a write workload on ext4 and on XFS. On the reference machine the tests use the ext4 filesystem and an XFS directory that the user mounts and provides; the tests run without privileges.
  - **Regression tests.** A test added for a defect must fail when the defect's fix is reverted (mutation) in the default test build, where debug assertions are on (Speed, below). Audit finding m-c: `audit_a_failed_capture_never_reaches_the_wal` detected its defect only in release builds, because in debug a debug assertion in the WAL writer failed first; it now fails under the mutation in debug and in release (TEST 83 and 84 on rmp task #15).
  - **Concurrency.**
    - **`crate::sync` facade** (R11). The crate imports every synchronisation primitive (`Mutex`, `RwLock`, `Condvar`, atomics, `Arc`) from `crate::sync`, which re-exports `std::sync`, or `loom::sync` under `cfg(loom)`, or `shuttle::sync` under `cfg(shuttle)`.
    - `loom` explores every interleaving of small models: commit latch and publish order, the lock table, deadlock detection and the index side-tree protocol.
    - `shuttle` runs randomised schedules of larger tests: the commit latch, the WAL writer and the checkpointer (D-SHUTTLE).
    - **Stress profile** (R7). The concurrency tests also run as at least 16 concurrent copies, outside the default run, before a task closes.
  - **Isolation** (from Sprint 2, with [TS-14](transactions-and-recovery.md#ts-14-mvcc) and [TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks); R12).
    - The Hermitage scenarios run against lpg's transaction API. lpg is serializable by default, so G2 and write skew must abort (P-TEST §7).
    - Elle checks list-append and rw-register histories recorded from lpg transactions. `elle-cli` runs as an external tool, outside the crate.
  - **Memory** (R13). A separate test binary counts allocations with a global allocator over cycles of open, workload and close, and compares them with the memory accountant ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). It measures the gap of [unverified point 15](#unverified-points). A global allocator needs `unsafe impl GlobalAlloc` (`pub unsafe trait GlobalAlloc`, <https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html>); whether test code outside the crate root may use it under NFR-SAFE-001 is not decided.
  - **Speed.**
    - Tests run as fast as possible without losing effectiveness: a change that makes them faster is accepted only when measurement shows it removes no tested property (user requirement, DECISION 67 on rmp task #15).
    - **`NoFlush` storage** (`src/test_support/mod.rs:74-87`): the file system with `sync_data` and `sync_dir` as no-ops; every other call is unchanged. The engine, B+tree, free-list and pool tests, the commit test `audit_a_failed_capture_never_reaches_the_wal`, and the data-file and `CONTROL` unit tests use it. None of them crashes the operating system, so what they read back comes from the operating-system cache whether or not it was flushed; the flush proves nothing there. The tests of the file-system storage (`storage::fs`) and of the fault-injecting storage (`storage::fault`) keep real flushes. Crash tests use the fault-injecting storage, which models durability itself (DECISION 77 on rmp task #15).
    - Tests build at `opt-level = 1` (`[profile.test]`, `Cargo.toml:21-24`). The `test` profile inherits `dev` (D-CARGO), so debug assertions and overflow checks stay on; `cargo test --message-format=json` reports both on at `opt-level` 1 (2026-10-03).
  - **Bolt.**
    - The server endpoint is tested with the official Neo4j drivers for Java, Python, JavaScript, .NET and Go, and with the crate's own client.
    - The client endpoint is tested against the crate's server and against a Neo4j server that supports Bolt 5.0 or later.
    - `neo4rs` is excluded.
  - **Licences.** `cargo-deny` checks licences on every build of the dependency set.
  - **Performance.** The NFR-PERF-002 baseline (LDBC SNB Interactive and BI, micro-workloads; against Neo4j and Memgraph) runs on the reference hardware. Giant-graph efficiency is measured with the cache at 1 % and 10 % of the data (NFR-PERF-005, NFR-PERF-006). The M-LAYOUT harness is the starting point for the micro-workloads.
- **Satisfies**: NFR-VER-001, NFR-VER-002, NFR-VER-003, NFR-VER-004, NFR-VER-005, NFR-PLAT-004, NFR-DEP-002, FR-GQL-002, FR-LIM-003, NFR-PERF-002, NFR-PERF-005, NFR-PERF-006.
- **Evidence**:
  - SQLite simulates OS crashes and power failure in a test VFS (R-SQLITE `src/test6.c`).
  - turso has a deterministic simulator and a differential oracle (R-TURSO `testing/simulator`, `testing/differential-oracle`).
  - The fault model and test dimensions of Turso and libSQL compared with lpg's, and recommendations R1–R13: [P-TEST](../evidence/research/research-04-turso-libsql.md) §2–§6. On 2026-10-03 the user adopted R6 (`opt-level` 1), R7, R9, R11 and R12, which widen the scope; R1–R3, R5, R8, R10 and R13 fall within the existing scope of tasks #20 and #22. They are recorded in rmp task #20 comments 69 and 70, task #2 comment 71, task #23, DECISION 67 on task #15, and sprint 1 comment 28.
  - Nth-failure loop. SQLite's `faultsim` fails the *N*-th operation for *N* = 1, 2, … until no fault fires, then checks the result (R-LIBSQL `libsql-sqlite3/test/malloc_common.tcl:41-50, 326-385`).
  - Checkpoint order. Turso fixed two checkpoint ordering bugs: "fsync DB file before truncating WAL after checkpoint" and "path not fsyncing the DB file after truncate checkpoint" (R-TURSO `CHANGELOG.md:1977, 2006`).
  - Seeds. Turso takes its seed from the `SEED` variable, otherwise from the clock (R-TURSO `tests/integration/common.rs:535-547`).
  - Pause and fail points. Turso injects yields and failures at named points, compiled only for tests (R-TURSO `core/mvcc/yield_points.rs:5-90`, `core/mvcc/yield_hooks.rs:11-69`).
  - Facade. Turso's `core/sync.rs:1-40` switches to `shuttle` under `cfg(shuttle)`; its CI runs the shuttle tests (`.github/workflows/fuzz.yml:127-153`).
  - Isolation. Turso's Hermitage tests (R-TURSO `core/mvcc/database/hermitage_tests.rs:104-891`) and Elle recorder (`testing/concurrent-simulator/elle.rs`).
  - Leaks. Turso's leak test is a separate binary with a counting global allocator and a per-cycle byte budget (R-TURSO `tests/leak/mod.rs:1-62, 175-236`).
  - Stress profile. With 16 concurrent copies, `concurrent_readers_during_splits_see_every_committed_key_exactly_once` failed in 12 of 80 runs; alone, in 0 of 40 (rmp comment 65 on task #15; P-TEST §5.3).
  - Speed. Full suite, 2026-10-03, two runs each: 24.6 and 28.0 s at `opt-level` 0, 14.4 and 19.7 s at 1 (DECISION 67). P-TEST §5.2: 28.5–29.2 s against 15.5–18.6 s over three runs each, and 2.8 s more per incremental build. `opt-level` 1 exposed races that 0 hid (DECISION 67). `NoFlush` (DECISION 77): real flushes per suite fell from 506 to 104, iowait from 31–40 % to 0–1 %, and the full suite from 9.7, 19.9 and 19.8 s to 7.7, 7.9 and 7.7 s.
  - The opengql grammar ships 14 sample files (M-QE §Q9).
  - neo4rs offers Bolt 4.x only ([S37]).
- **Crates**:
  - `antlr4rust` 0.6.0: dev dependency of the oracle crate.
  - `loom` 0.7.2 (MIT, 68.6 M downloads): dev dependency.
  - `shuttle` 0.9.5 (Apache-2.0, 6.0 M downloads, released 2026-10-01, crates.io): dev dependency. Its documentation states: "Shuttle is not sound (a passing Shuttle test does not prove the code is correct), but it scales to much larger test cases than Loom" (D-SHUTTLE).
  - `tempfile` 3.27.0: dev dependency.
  - Cucumber runners: not used, since a small runner suffices.
- **Rejected**:
  - Power-loss tests that need root (device-mapper targets) and loop mounts: the reference machine has no root access ([M-LAYOUT](../evidence/layout-bench/results.md) "Method"); the fault-injecting storage covers torn and lost writes without privileges.
  - Generating test sentences with grammarinator: its licence is reported as `NOASSERTION` (M-QE §Q9).
  - Running the `NoFlush` tests on the fault-injecting storage: it holds unsynced writes in memory, and several of these tests read files back through the file system directly (DECISION 77).
  - A smaller WAL segment in tests: the segment size is a specified constant ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)) (DECISION 77).
  - An in-memory `Storage` or tmpfs for speed: 35 durability tests took 3.05 s on ext4 against 3.11 s on tmpfs (P-TEST §5.2).
  - TLA+ models of the WAL now: Turso's do not model process crashes (R-TURSO `tlaplus/multiprocess/MultiprocessWal.tla:38-40`); the sweeps above cover lpg's crash paths.
  - Antithesis: a commercial service.

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
16. Which pairs of page numbers share a page checksum for the same content, and so escape the page number binding ([TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check)); not computed.
17. That PostgreSQL's backend `durable_rename` (`fd.c`) syncs the renamed file and its directory as the frontend version does: `fd.c` is not in the R-PG clone ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
