---
title: Non-functional requirements
status: draft
last-reviewed: 2026-10-08
related:
  - functional-requirements.md
  - graph-algorithms.md
  - open-questions.md
---

# Non-functional requirements

Each requirement records a user decision unless it states otherwise. External facts cite the [sources](README.md#sources). Each testable requirement is followed by its acceptance criterion in Given/When/Then form.

## Performance

- NFR-PERF-001 Performance targets (for example throughput, latency and memory) are to be defined by measurement. The project SHALL fix them from a measured baseline ([NFR-PERF-002](#performance)). Until then, no performance target applies; the numbers are open ([OQ-001](open-questions.md#oq-001)).
- NFR-PERF-002 The baseline SHALL be measured on three workloads: the LDBC Social Network Benchmark Interactive workload, the LDBC Social Network Benchmark Business Intelligence workload [S32], and the project's own micro-workloads. The crate SHALL be compared with Neo4j and Memgraph on the same workloads. The baseline SHALL be measured on the reference hardware ([NFR-PERF-006](#performance)) (user decision 2026-10-02). Each micro-workload SHALL be defined as a GQL text with its expected result, and its result SHALL be checked, before its timing is measured; the timing is measured later (user decision 2026-10-08, "Definir agora, medir no S6": define now, measure in a later sprint).
  - Acceptance: Given the baseline report, when it is inspected, then it contains measured results of the crate, Neo4j and Memgraph for each of the three workloads, measured on the reference hardware, with the versions used.
- NFR-PERF-003 Performance and resource efficiency in CPU, memory and I/O SHALL be a primary requirement of the crate, together with compatibility with multiversion concurrency control (MVCC) (user decision 2026-09-24, confirmed 2026-10-02).
- NFR-PERF-004 The layout in which the crate stores graphs SHALL be fully compatible with MVCC and fast for traversals (user decision 2026-09-24, confirmed 2026-10-02). Among the layouts considered, it SHALL be chosen on evidence by these criteria, in this order (user decision 2026-10-02): first, the least device I/O and storage space for traversals with the cache holding between 1% and 10% of the data ([NFR-PERF-005](#performance)); second, the least CPU time for traversals when the data is fully resident in memory.
  - Acceptance: Given the record of the layout choice, when it is inspected, then it cites the evidence for the chosen layout's MVCC compatibility, and it compares the layouts considered first on device I/O and storage space with the cache holding between 1% and 10% of the data, then on CPU time when the data is fully resident.
- NFR-PERF-005 The crate SHALL handle graphs far larger than memory ([FR-STO-001](functional-requirements.md#storage)) with maximum efficiency. No absolute graph size is set. Efficiency SHALL be judged by measurements on the reference hardware ([NFR-PERF-006](#performance)) with the cache holding between 1% and 10% of the data. The numeric targets are open ([NFR-PERF-001](#performance), [OQ-001](open-questions.md#oq-001)) (user decision 2026-10-02).
  - Acceptance: Given an efficiency measurement, when its report is inspected, then it names the reference hardware and states the cache size and the data size, and the cache holds between 1% and 10% of the data.
- NFR-PERF-006 The reference hardware SHALL be the machine the user designated on 2026-10-02, until the user designates another: an AMD Ryzen 9 5900HX processor (8 cores, 16 threads, 16 MiB L3 cache), 30.77 GiB of RAM (`MemTotal` 32 263 888 KiB), a Samsung 970 EVO Plus 1 TB NVMe SSD with an ext4 filesystem, and Linux (user decision 2026-10-02). On 2026-10-02 `lscpu`, `/proc/meminfo`, `lsblk` and `df` on that machine reported these values.
- NFR-PERF-007 The GQL query engine SHALL NOT execute a single query in parallel until the baseline ([NFR-PERF-002](#performance)) contains measured results for the LDBC Social Network Benchmark Business Intelligence workload. After that, parallel execution of a single query MAY be added only as an option that the application enables through the Rust API and that is off by default (user decision 2026-09-24, confirmed 2026-10-02). This requirement applies to the GQL query engine only; graph-algorithm runs, including those invoked with `CALL`, follow [FR-ALG-006](functional-requirements.md#graph-algorithms) (user decision 2026-10-02).
  - Acceptance: Given a release published before the baseline contains Business Intelligence results, when any query that invokes no graph algorithm runs, then its execution uses no more than one thread at a time; and given a later release that offers parallel execution, when a query runs without the application enabling the option, then its execution uses no more than one thread at a time.

## Platform

- NFR-PLAT-001 The crate SHALL support Linux only.
  - Acceptance: Given the crate's documentation and test configuration, when they are inspected, then Linux is the only supported and tested operating system.
- NFR-PLAT-002 The crate SHALL use Rust edition 2024 and the standard library (`std`).
  - Acceptance: Given `Cargo.toml`, when it is inspected, then `edition` is "2024" and the crate is not `no_std`.
- NFR-PLAT-003 The public Rust API SHALL be asynchronous and run on the tokio runtime.
  - Acceptance: Given the public API, when it is inspected, then every operation that performs I/O or waits is an `async` function usable on a tokio runtime.
- NFR-PLAT-004 The crate's durability guarantees ([FR-TX-002](functional-requirements.md#transactions), [Recovery](functional-requirements.md#recovery)) SHALL apply on the ext4 and XFS filesystems. Other filesystems, including network shares such as NFS, are not supported for those guarantees (Neo4j default: the only supported Linux filesystems are ext4 and XFS; network shares such as NFS do not meet the POSIX requirement [S21]).
  - Acceptance: Given the crash and fault-injection tests of [NFR-VER-004](#verification), when they run on ext4 and on XFS, then they pass on both.

## Safety

- NFR-SAFE-001 The crate SHALL forbid `unsafe` code (`#![forbid(unsafe_code)]`).
  - Acceptance: Given the crate root, when it is inspected, then it contains `#![forbid(unsafe_code)]` and the crate builds.

## Simplicity

- NFR-SIMP-001 The specification and the design SHALL include only what a decided requirement needs, and SHALL choose the simplest solution that meets it. Added complexity (an option, a configuration setting, an abstraction or an extension point) SHALL be accepted only when the user decides it or when measured evidence shows that the simpler solution fails a stated requirement. This records the "Simplicity" criterion of the project's `CLAUDE.md`.
  - Acceptance: Given any option, configuration setting, abstraction or extension point in this specification or in the design, when it is traced, then it leads to a user decision or to a recorded measurement that shows the simpler alternative fails a stated requirement.

## Dependencies

- NFR-DEP-001 Dependencies SHALL be minimal, widely used and maintained.
- NFR-DEP-002 Each dependency's licence SHALL be compatible with the MIT licence. `cargo-deny` SHALL check this. The licences counted as compatible are MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, BSL-1.0, CC0-1.0 and Unicode-3.0. Copyleft licences SHALL be excluded. A dependency offered under alternative licences SHALL pass when one alternative is on the list (user decision 2026-10-02).
  - Acceptance: Given the dependency tree, when `cargo-deny` checks licences, then every dependency has a licence on the list, or, if it offers alternatives, at least one alternative on the list.

## Release

- NFR-REL-001 The crate SHALL follow Semantic Versioning. Versions 0.x are unstable.
- NFR-REL-002 The crate SHALL be published on crates.io under the name `lpg`. On 2026-09-24 no crate of that name existed [S5].
- NFR-REL-003 The crate SHALL be licensed under the MIT licence.
  - Acceptance: Given the published crate, when its metadata is inspected, then its licence is MIT.
- NFR-REL-004 The first release SHALL include: the Rust API; storage; transactions; the catalog; GQL minimum conformance; the server-side and client-side Bolt endpoints; the whole graph-algorithm library; and import and export.
  - Acceptance: Given the first release, when its documentation and tests are inspected, then each listed item is present and tested, including every kept entry of the graph-algorithm inventory ([FR-ALG-005](functional-requirements.md#graph-algorithms)).
- NFR-REL-005 Version 1.0 SHALL require full GQL conformance: minimum conformance plus all optional features ([FR-GQL-001](functional-requirements.md#gql)). Every release before full conformance SHALL be a 0.x version.
  - Acceptance: Given a proposed 1.0 release, when its list of supported feature identifiers is compared with ISO/IEC 39075:2024, then every optional feature is listed; otherwise the release is numbered 0.x.
- NFR-REL-006 The minimum supported Rust version (MSRV) SHALL be fixed and declared in `Cargo.toml`. It SHALL be raised only in a minor release, with a note in that release's notes.
  - Acceptance: Given two consecutive releases with different MSRVs, when their versions are compared, then the later one is a minor release (not a patch) and its release notes state the new MSRV.

## Verification

- NFR-VER-001 GQL conformance tests SHALL be traced to clauses and feature identifiers of ISO/IEC 39075.
  - Acceptance: Given any conformance test, when it is inspected, then it names the clause or feature identifier it verifies.
- NFR-VER-002 The ISO/IEC 39075:2024 text is not available to the project. The opengql/grammar repository [S4], at commit `16ea71b`, SHALL be the authority for GQL syntax, and the parser generated from it SHALL be the test oracle for the crate's parser. The ISO electronic inserts [S45] SHALL serve only as a supplementary source for GQLSTATUS conditions, feature identifiers and implementation-defined items (user decision 2026-10-02). The project SHALL use these Apache-2.0 projects as test artefacts for GQL conformance verification, and SHALL preserve their licence and attribution unchanged (user decision 2026-10-02; gql-compat added by user decision 2026-10-08, "Como input, traduzido": as an input, translated). The project's `CLAUDE.md` lists the same projects as GQL conformance test artefacts (section "Open-source references").
  - The opengql/grammar repository [S4], as the grammar authority and, through a parser generated from it in a test-only crate, as the test oracle.
  - The opengql/tck repository [S39].
  - The openCypher TCK [S40], limited to the scenarios that apply to GQL.
  - gql-compat [S55], at commit `9de1991`, as an input only: its mandatory and condition cases whose feature identifiers the supported feature set includes ([FR-GQL-002](functional-requirements.md#gql)) are translated into the project's own scenarios, with attribution. On 2026-10-08, 106 of its 107 mandatory cases and 24 of its 69 condition cases applied (technical specification [TS-30](../technical-spec/quality-and-testing.md#ts-30-testing-and-conformance)).
  - Acceptance: Given the repository, when the material from these projects is inspected, then each keeps its Apache-2.0 licence and attribution unchanged, the parser generated from opengql/grammar is in a test-only crate that the published crate does not depend on, and every openCypher TCK scenario used applies to GQL; given a scenario translated from gql-compat, when it is inspected, then it names the gql-compat case it comes from and keeps gql-compat's Apache-2.0 attribution; and given any test request, when the crate's parser and the parser generated from opengql/grammar parse it, then both accept it or both reject it.
- NFR-VER-003 Integration tests SHALL verify the Bolt endpoints against the currently supported official Neo4j drivers for Java, Python, JavaScript, .NET and Go, and against the crate's own Bolt client. The `neo4rs` crate is excluded until a release of it offers Bolt 5.0 or later: its releases 0.8.0 and 0.9.0-rc.10 offer only Bolt 4.x versions [S37], which [FR-BOLT-003](functional-requirements.md#bolt) refuses (user decision 2026-10-02).
  - Acceptance: Given the integration test suite, when it runs, then it exercises the server-side endpoint with each listed driver and all tests pass.
- NFR-VER-004 Fault-injection and crash tests SHALL verify durability and recovery, including process termination by `kill -9` and fsync failures simulated through the storage abstraction ([ABS-003](application-abstractions.md#abstractions)).
  - Acceptance: Given the fault-injection suite, when it runs, then it includes `kill -9` and simulated fsync-failure cases and all pass.
- NFR-VER-005 Integration tests SHALL verify the client-side Bolt endpoint against the crate's server-side endpoint and against a Neo4j server that supports Bolt 5.0 or later ([FR-BOLT-020](functional-requirements.md#bolt)).
  - Acceptance: Given the client integration suite, when it runs against both servers, then all tests pass.

## Documentation

- NFR-DOC-001 All documentation SHALL use British English spelling. Proper names and standard terms, such as "Labeled Property Graph", the GQL "catalog" and the "serializable" isolation level, keep their established spelling.
