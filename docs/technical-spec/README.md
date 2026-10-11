---
title: Technical specification
status: draft
last-reviewed: 2026-10-11
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
| [query-engine.md](query-engine.md) | Parser, feature classification of `GQL.g4`, binder, planner, execution, limits, EXPLAIN and PROFILE. | TS-19, TS-20 |
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
- **Coverage.** Every requirement of the functional specification is listed in the `Satisfies` line of at least one decision, except these release, process and documentation requirements, which need no technical decision: FR-GQL-003, FR-GQL-006, NFR-PERF-001, NFR-REL-001, NFR-REL-003, NFR-REL-005, NFR-DOC-001 (met by the conventions of this folder) and the withdrawn FR-REC-009.
- **Functional sources.** `[Sn]` refers to the sources of the [functional specification](../functional-spec/README.md#sources).
- **Provisional.** A decision that depends on the ISO/IEC 39075:2024 text, which is not available, is marked "provisional (OQ-005)".
- **Unverified.** A claim not confirmed in a source or a measurement is marked "unverified". The full list is in [quality-and-testing.md](quality-and-testing.md#unverified-points).
- **Simplicity.** Only what a requirement needs is specified ([NFR-SIMP-001](../functional-spec/non-functional-requirements.md#simplicity)). An option, setting or extension point appears only when a requirement or a measurement demands it.
- **Spelling.** British English ([NFR-DOC-001](../functional-spec/non-functional-requirements.md#documentation)).
- **No copying.** Reference projects were read for study only; no code or text was copied ([`CLAUDE.md`](../../CLAUDE.md), "Open-source references").

## Terms

Functional terms are in the [functional glossary](../functional-spec/glossary.md). Technical terms used here:

- **Cache slot**: a buffer-pool slot that holds one page in memory ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
- **Frame**: a fixed-size WAL record holding one page image or a group's commit record ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
- **Group**: the unit of durability in the WAL: page images of one or more members (commits and internal batches) plus one commit frame ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
- **Internal batch**: a logged page change made under the commit latch without a commit timestamp: a garbage-collection batch or a checkpoint's statistics batch ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
- **LSN**: log sequence number, a byte position in the WAL's stream of frames ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
- **NO-STEAL**: the rule that a change reaches disk only after its commit is durable, and is visible only after its commit is published ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
- **Redo point**: the LSN from which recovery replays the WAL ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)).
- **Stamp**: the `created_ts` and `deleted_ts` commit timestamps carried by an entry of a stamped tree; in a node or edge record, `created_ts` holds the record's `version_ts` ([TS-06](storage.md#ts-06-graph-layout)).

## Sources

Checked between 2026-09-24 and 2026-10-08. Citations added on or after 2026-10-07 that use a later commit than the table below name it, as `R-PG @ 1f22453` (2026-10-07), `R-FJALL @ 17ae672` (2026-10-05), `R-MG @ 847e224` (2026-10-07) and `R-TURSO @ 8cc059b` (2026-10-07). Crate sources are cited by crate version, as `crossbeam-epoch 0.9.18`.

### Reference source code (shallow clones, read for study only)

| ID | Project | Commit (date) | Licence |
|---|---|---|---|
| R-NEO | Neo4j community, <https://github.com/neo4j/neo4j> | `54a7dcf` (2026-09-14) | GPLv3 |
| R-MG | Memgraph, <https://github.com/memgraph/memgraph> | `33813d7` (2026-09-24) | BSL-1.1 |
| R-PG | PostgreSQL, <https://github.com/postgres/postgres> | `8982935` (2026-09-24) | PostgreSQL |
| R-INNO | MySQL InnoDB (`storage/innobase`), <https://github.com/mysql/mysql-server> | `3b99be40` (2026-09-28) | GPLv2 |
| R-KUZU | Kuzu (archived 2025-10-10), <https://github.com/kuzudb/kuzu> | `89f0263` (2025-10-10) | MIT |
| R-SQLITE | SQLite, <https://github.com/sqlite/sqlite> | `c62a443` (2026-09-24) | public domain |
| R-DUCK | DuckDB, <https://github.com/duckdb/duckdb> | `0b9cba2` (2026-09-24) | MIT |
| R-REDB | redb, <https://github.com/cberner/redb> | `7677a88` (2026-09-23) | MIT/Apache-2.0 |
| R-LMDB | LMDB, <https://github.com/LMDB/lmdb> | `700e10f` (2026-08-19) | OpenLDAP Public License 2.8 |
| R-ROCKS | RocksDB, <https://github.com/facebook/rocksdb> | `4cd0450` (2026-10-01) | GPLv2 or Apache-2.0 |
| R-LEVELDB | LevelDB, <https://github.com/google/leveldb> | `7ee830d` (2026-03-10) | BSD-3-Clause |
| R-FJALL | fjall, <https://github.com/fjall-rs/fjall> | `3adaa50` (2026-08-30) | MIT/Apache-2.0 |
| R-TURSO | Turso, <https://github.com/tursodatabase/turso> | `fc98dac` (2026-10-02) | MIT |
| R-LIBSQL | libSQL, <https://github.com/tursodatabase/libsql> | `d6c75af` (2026-08-23) | MIT |
| R-NEO4RS | neo4rs, <https://github.com/neo4j-labs/neo4rs> | `1a35661` (2026-09-25) | MIT |
| R-PETGRAPH | petgraph, <https://github.com/petgraph/petgraph> | `a4d94bd` (2026-09-13) | MIT/Apache-2.0 |
| R-RWX | rustworkx, <https://github.com/Qiskit/rustworkx> | `25398ab` | Apache-2.0 |
| R-GALGS | graphalgs, <https://github.com/starovoid/graphalgs> | `1f8a412` | MIT |
| R-ANTLR | antlr4rust/antlr4, <https://github.com/antlr4rust/antlr4> | `9b5bd1e` (2026-09-27) | BSD-3-Clause |
| R-GQL | opengql/grammar, <https://github.com/opengql/grammar>; the trusted GQL syntax reference ([TS-19](query-engine.md#ts-19-parser)) | `16ea71b` (2025-06-17), v1.9.0 | Apache-2.0 |
| R-ZU | zu, <https://github.com/tamnd/zu>; `docs/gql-implementation-defined.md` states its choice for every implementation-defined item | `a821319` (2026-10-02) | Apache-2.0 |
| R-GRAFEO | Grafeo, <https://github.com/GrafeoDB/grafeo> | `3c23dbe` (2026-10-06) | Apache-2.0 |
| R-GQLC | gql-compat, <https://github.com/tamnd/gql-compat> | `9de1991` (2026-08-24) | Apache-2.0 |
| R-TCK | opengql/tck, <https://github.com/opengql/tck> | `e0700a7` (2024-09-10) | Apache-2.0 |
| R-OCTCK | openCypher, <https://github.com/opencypher/openCypher> (its `tck/`) | `677cbaf` (2026-03-20) | Apache-2.0 |
| R-DF | Apache DataFusion, <https://github.com/apache/datafusion> | `54a4bbf` (2026-10-08) | Apache-2.0 |
| R-GRAMMARINATOR | grammarinator, <https://github.com/renatahodovan/grammarinator> | `3a6a745` (2026-10-02) | BSD-3-Clause |
| R-APOC | APOC, <https://github.com/neo4j/apoc> | `e52bc20` (2026-10-09) | Apache-2.0 |
| R-NX | NetworkX 3.7, read from its PyPI wheel, <https://github.com/networkx/networkx> | 3.7 | BSD-3-Clause |
| R-LDBCI | LDBC SNB Interactive v1 implementations, <https://github.com/ldbc/ldbc_snb_interactive_v1_impls> | `11db98c` (2026-03-26) | Apache-2.0 |
| R-ISO | ISO/IEC 39075:2024 electronic inserts, [docs/standards/iso-iec-39075/ed-1/](../standards/iso-iec-39075/ed-1/README.md) ([S45]); supplementary source for GQLSTATUS codes, feature identifiers and implementation-defined items | edition 1 (2024-03-23) | ISO terms of use |

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
| D-JIFF | jiff 0.2.38, `TimeZoneDatabase`: <https://docs.rs/jiff/0.2.38/jiff/tz/struct.TimeZoneDatabase.html>; `Timestamp` (nanosecond precision; "Jiff does not support leap seconds"): <https://docs.rs/jiff/0.2.38/jiff/struct.Timestamp.html> (checked 2026-10-07) |
| D-CTZ | chrono-tz 0.10.4: <https://docs.rs/chrono-tz/0.10.4/chrono_tz/> |
| D-TIME | time 0.3.55: <https://docs.rs/time/0.3.55/time/> |
| D-TOKIO | tokio 1.53.1, `task::spawn_blocking`: <https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html> |
| D-RAYON | rayon 1.12.0, `ParallelIterator::reduce` and `sum`: <https://docs.rs/rayon/1.12.0/rayon/iter/trait.ParallelIterator.html> |
| D-AFIT | Rust blog, "Announcing `async fn` and return-position `impl Trait` in traits" (2023-12-21): <https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/> |
| D-METRICS | metrics 0.24.6: <https://docs.rs/metrics/0.24.6/metrics/> |
| D-RUST | rust-lang/rust `RELEASES.md`, <https://github.com/rust-lang/rust/blob/master/RELEASES.md> (1.85.0: "The 2024 Edition is now stable"; 1.89.0 stabilises `File::lock` and `File::try_lock`; latest stable 1.98.1, 2026-09-03) |
| D-DENY | cargo-deny, "licenses" configuration: <https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html> (licences not in `allow` are denied unless a listed licence satisfies the expression; checked 2026-10-02) |
| D-RUSTIX | rustix 1.1.5, `rustix::fs` (`fstatvfs`, `fallocate`): <https://docs.rs/rustix/1.1.5/rustix/fs/index.html> |
| D-KUZU | X. Feng, G. Jin, Z. Chen, C. Liu, S. Salihoğlu, "KÙZU Graph Database Management System", CIDR 2023, <https://www.cidrdb.org/cidr2023/papers/p48-jin.pdf> |
| D-HYPER | T. Neumann, T. Mühlbauer, A. Kemper, "Fast Serializable Multi-Version Concurrency Control for Main-Memory Database Systems", SIGMOD 2015 ([S42] of the functional specification) |
| D-POSIX | POSIX.1-2024 (IEEE Std 1003.1-2024), `<sys/types.h>`: "`blkcnt_t` and `off_t` shall be signed integer types": <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_types.h.html> |
| D-CARGO | The Cargo Book, "Profiles": <https://doc.rust-lang.org/cargo/reference/profiles.html> (`test` inherits `dev`; `dev` defaults `opt-level = 0`, `debug-assertions = true`, `overflow-checks = true`; checked 2026-10-03) |
| D-N4GQL | Neo4j Cypher Manual (Cypher 25), GQL conformance: "Supported mandatory GQL features" and "Supported optional GQL features": <https://neo4j.com/docs/cypher-manual/current/appendix/gql-conformance/> (checked 2026-10-08) |
| D-N4TYPES | Neo4j Cypher Manual, "Property, structural, and constructed values" (types and their synonyms, type normalization), "Graph references", "Match modes", "Aggregating functions", "Naming rules and recommendations", "Operators", "Spatial functions", "Create indexes" (point indexes), graph types ("Set graph types", "Show graph types"): <https://neo4j.com/docs/cypher-manual/current/> (checked 2026-10-08) |
| D-MGFUNC | Memgraph documentation, "Functions": <https://memgraph.com/docs/querying/functions> (checked 2026-10-08) |
| D-N4STATUS | Neo4j Status Codes, "List of GQLSTATUS error codes" and "Notifications": <https://neo4j.com/docs/status-codes/current/> (checked 2026-10-08) |
| D-BOLTMSG | Neo4j Bolt, "Message" (`RUN`, `PULL` and `FAILURE` metadata): <https://neo4j.com/docs/bolt/current/bolt/message/> (checked 2026-10-08) |
| D-N4DRIVER | Neo4j Java Driver API, `SummaryCounters`: <https://neo4j.com/docs/api/java-driver/current/org.neo4j.driver/org/neo4j/driver/summary/SummaryCounters.html> (checked 2026-10-08) |
| D-N4OPS | Neo4j Operations Manual, "Metrics reference" and "Logging" (query log): <https://neo4j.com/docs/operations-manual/current/monitoring/> (checked 2026-10-08) |
| D-JAVA | Java SE API, `Double.valueOf(String)` (round to nearest under IEEE 754); The Java Language Specification, §5.1.2 "Widening Primitive Conversion" |
| D-UCD | Unicode Character Database, `DerivedCoreProperties.txt` of versions 13.0.0 to 16.0.0: <https://www.unicode.org/Public/> (downloaded 2026-10-08) |
| D-SHUTTLE | shuttle 0.9.5: <https://docs.rs/shuttle/0.9.5/shuttle/> ("randomized testing, rather than the exhaustive testing that Loom offers"; `shuttle::sync` provides `Mutex`, `RwLock`, `Condvar`, `Arc`, `atomic`) |
| D-RUST190 | Rust blog, "Announcing Rust 1.90.0" (2025-09-18): "The `x86_64-unknown-linux-gnu` target will now use the LLD linker for linking Rust crates by default." <https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/> (checked 2026-10-11) |
| D-FORK | LLVM libFuzzer, `compiler-rt/lib/fuzzer/FuzzerFlags.def:63-71` (`fork`; `ignore_timeouts` and `ignore_ooms` default 1, `ignore_crashes` 0) and `FuzzerFork.cpp:457-475` (a timeout or OOM exit is counted and fuzzing continues when ignored; any other non-zero exit stops every job), llvm-project @ `f3653503` (checked 2026-10-11) |
| D-XML | W3C, "Extensible Markup Language (XML) 1.0 (Fifth Edition)": §2.2 `Char`, §2.11 end-of-line handling: <https://www.w3.org/TR/xml/> |
| D-GMLXSD | GraphML 1.0 schema files, <http://graphml.graphdrawing.org/xmlns/1.0/>: `graphml.xsd` (SHA-256 `c5d04fb9…af52`), `graphml-structure.xsd` (`78091238…a04b`), `graphml-attributes.xsd` (`e13e2d5d…36bb`), `graphml-parseinfo.xsd` (`6d025e0b…92ca`) (fetched 2026-10-11) |
| D-CARGOFEAT | The Cargo Book, "Features", "Feature unification": "Cargo will use the union of all features enabled on that dependency when building it": <https://doc.rust-lang.org/cargo/reference/features.html> (checked 2026-10-11) |
| D-CARGOFUZZ | cargo-fuzz 0.13.2, `src/project.rs:354-371`: without `--target-dir`, cargo chooses the target directory, except for coverage builds, which use `<current directory>/target/<triple>/coverage` (tag `0.13.2`, commit `984c861`, checked 2026-10-11) |

### Measurements and research notes

Kept in the repository under [`docs/evidence/`](../evidence/README.md), which also says how to reproduce each item.

| ID | File | Content |
|---|---|---|
| M-LAYOUT | [evidence/layout-bench/results.md](../evidence/layout-bench/results.md) (2026-10-02), with raw outputs and source in [evidence/layout-bench/](../evidence/layout-bench/) | Rust benchmark of seven adjacency layouts on the reference hardware ([NFR-PERF-006](../functional-spec/non-functional-requirements.md#performance)): 1 M nodes / 10 M edges (Zipf and uniform, online and bulk) at 100 %, 25 %, 10 % and 1 % cache; 10 M nodes / 100 M edges for two layouts. |
| M-QE | [evidence/research/research-03-query-engine.md](../evidence/research/research-03-query-engine.md) (2026-09-24), with the experiments [antlr-exp](../evidence/antlr-exp/), [tokio-exp](../evidence/tokio-exp/), [exec-exp](../evidence/exec-exp/), [value-exp](../evidence/value-exp/) | Query-engine research; antlr4rust parse times on `GQL.g4`; `spawn_blocking` hand-off cost; batch versus row execution; value sizes; clock cost. |
| M-LOCK | [evidence/research/research-01-storage.md](../evidence/research/research-01-storage.md) §Q8 and [evidence/locktest/](../evidence/locktest/main.rs) (2026-09-24) | `File::try_lock` behaviour within and across processes. |
| P-STO | [evidence/research/research-01-storage.md](../evidence/research/research-01-storage.md) (2026-09-24) | Storage evidence with file and line references. |
| P-TX | [evidence/research/research-02-layout-mvcc.md](../evidence/research/research-02-layout-mvcc.md) (2026-09-24), with the page model [evidence/layout-sim/](../evidence/layout-sim/pagesim.py) | Layout, MVCC, validation, locks and indexes evidence with file and line references. |
| M-FEAT | [evidence/planning/round-s/](../evidence/planning/round-s/README.md) (2026-10-08) | Rule-to-feature classification of `GQL.g4`; TCK and gql-compat selection counts; ambiguous `GQL.g4` decisions; keyword differences with the ISO BNF; Unicode version of the oracle's ANTLR jar. |
| M-S4 | [evidence/planning/sprint-4-plan.md](../evidence/planning/sprint-4-plan.md) (2026-10-11) | Sprint 4 plan: specification gaps and their references (§3), reuse inventory (§4); Neo4j 2026.09.0 CSV import probe and GraphML probes with NetworkX 3.7 and xmlschema 4.3.2 (Appendix A). |
| P-TEST | [evidence/research/research-04-turso-libsql.md](../evidence/research/research-04-turso-libsql.md) (2026-10-03) | Turso and libSQL kernel components and test dimensions compared with lpg Sprint 1, with file and line references; recommendations R1–R13; test-speed and flaky-test measurements (§5). |
