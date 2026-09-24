---
title: Non-functional requirements
status: draft
last-reviewed: 2026-09-24
related:
  - functional-requirements.md
  - graph-algorithms.md
  - open-questions.md
---

# Non-functional requirements

Each requirement records a user decision. External facts cite the [sources](README.md#sources). Each testable requirement is followed by its acceptance criterion in Given/When/Then form.

## Performance

- NFR-PERF-001 Performance targets (for example throughput, latency and memory) are to be defined by measurement. The project SHALL fix them from a measured baseline ([NFR-PERF-002](#performance)). Until then, no performance target applies; the numbers are open ([OQ-001](open-questions.md#oq-001)).
- NFR-PERF-002 The baseline SHALL be measured on three workloads: the LDBC Social Network Benchmark Interactive workload, the LDBC Social Network Benchmark Business Intelligence workload [S32], and the project's own micro-workloads. The crate SHALL be compared with Neo4j and Memgraph on the same workloads.
  - Acceptance: Given the baseline report, when it is inspected, then it contains measured results of the crate, Neo4j and Memgraph for each of the three workloads, with the hardware and versions used.

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
- NFR-DEP-002 Each dependency's licence SHALL be compatible with the MIT licence. `cargo-deny` SHALL check this.
  - Acceptance: Given the dependency tree, when `cargo-deny` checks licences, then it reports no licence incompatible with MIT.

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
- NFR-VER-002 The project SHALL use the opengql/grammar repository (Apache-2.0 [S4]) as a test and grammar artefact, and SHALL preserve its attribution and licence. The project's `CLAUDE.md` permits this use as the sole exception to its no-copying rule (section "Open-source references").
  - Acceptance: Given the repository, when the opengql/grammar material is inspected, then its Apache-2.0 licence and attribution are present and unchanged.
- NFR-VER-003 Integration tests SHALL verify the Bolt endpoints against the currently supported official Neo4j drivers for Java, Python, JavaScript, .NET and Go, against the crate's own Bolt client and against `neo4rs`.
  - Acceptance: Given the integration test suite, when it runs, then it exercises the server-side endpoint with each listed driver and all tests pass.
- NFR-VER-004 Fault-injection and crash tests SHALL verify durability and recovery, including process termination by `kill -9` and fsync failures simulated through the storage abstraction ([ABS-003](application-abstractions.md#abstractions)).
  - Acceptance: Given the fault-injection suite, when it runs, then it includes `kill -9` and simulated fsync-failure cases and all pass.
- NFR-VER-005 Integration tests SHALL verify the client-side Bolt endpoint against the crate's server-side endpoint and against a Neo4j server that supports Bolt 5.0 or later ([FR-BOLT-020](functional-requirements.md#bolt)).
  - Acceptance: Given the client integration suite, when it runs against both servers, then all tests pass.

## Documentation

- NFR-DOC-001 All documentation SHALL use British English spelling. Proper names and standard terms, such as "Labeled Property Graph", the GQL "catalog" and the "serializable" isolation level, keep their established spelling.
