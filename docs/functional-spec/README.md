---
title: Functional specification
status: draft
last-reviewed: 2026-09-24
related:
  - ../../README.md
---

# Functional specification

This folder is the functional specification of the `lpg` crate. It states what the crate does, not how. The project [README](../../README.md) gives the summary; this folder is authoritative on detail.

## Files

| File | Content |
|---|---|
| [scope.md](scope.md) | Scope, non-goals and actors. |
| [application-abstractions.md](application-abstractions.md) | The closed list of abstractions the application implements. |
| [functional-requirements.md](functional-requirements.md) | Functional requirements (`FR-`) and business rules (`BR-`), each with its acceptance criterion. |
| [use-cases.md](use-cases.md) | Use cases (`UC-`) for the main flows. |
| [graph-algorithms.md](graph-algorithms.md) | The closed graph-algorithm inventory (`ALG-`), with withdrawn entries. |
| [non-functional-requirements.md](non-functional-requirements.md) | Non-functional requirements (`NFR-`). |
| [glossary.md](glossary.md) | Defined terms. |
| [open-questions.md](open-questions.md) | Open questions (`OQ-`): the undecided points, and the closed ones with pointers to their decisions. |

## Conventions

- **Requirement keywords.** The key words MUST, MUST NOT, SHALL, SHALL NOT, SHOULD, SHOULD NOT and MAY are to be interpreted as described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in capitals.
- **Identifiers.** `FR-<MODULE>-<NNN>`, `NFR-<CATEGORY>-<NNN>`, `BR-<MODULE>-<NNN>`, `UC-<MODULE>-<NNN>`, `ABS-<NNN>`, `ALG-<NNN>` and `OQ-<NNN>`. An identifier is stable once assigned and is never renumbered or reused. A withdrawn identifier keeps its entry, marked "Withdrawn" with the date and the reason.
- **Requirement style.** EARS for functional requirements, one requirement per line. Each functional requirement and business rule, and each testable non-functional requirement, is followed by an indented acceptance criterion in Given/When/Then form.
- **Spelling.** British English. Proper names and standard terms keep their established spelling ([NFR-DOC-001](non-functional-requirements.md#documentation)).
- **Evidence.** Each external fact cites its source or is marked unverified. Each requirement records a user decision.
- **Adopted defaults.** For points the user did not decide, the user instructed that the defaults and behaviours of Neo4j and Memgraph apply, Neo4j first where they differ and Memgraph where Neo4j has no equivalent; earlier user decisions and project rules take precedence. A requirement that adopts such a default says so and cites the source.
- **Simplicity.** The specification includes only what a decided requirement needs ([NFR-SIMP-001](non-functional-requirements.md#simplicity)).

## Sources

External facts in this folder were checked on 2026-09-24 against these sources. Neo4j pages marked "current" were at version 2026.09 on that date.

- [S1] Neo4j, Bolt Protocol, "Bolt protocol and Neo4j compatibility": <https://neo4j.com/docs/bolt/current/bolt-compatibility/> (latest documented version: Bolt 6.0).
- [S2] Neo4j, Bolt Protocol, "Messages": <https://neo4j.com/docs/bolt/current/bolt/message/> (LOGON added in 5.1; TELEMETRY added in 5.4; FAILURE carries `gql_status` from 5.7, with `code` renamed `neo4j_code`).
- [S3] Neo4j, Bolt Protocol, "Structure semantics": <https://neo4j.com/docs/bolt/current/bolt/structure-semantics/> (`UnsupportedType` introduced in Bolt 6.0).
- [S4] opengql/grammar: <https://github.com/opengql/grammar> (Apache-2.0; an ANTLR grammar for GQL, which the repository states ISO standardised in April 2024).
- [S5] crates.io API, `GET /api/v1/crates/lpg`: returned "crate `lpg` does not exist" on 2026-09-24.
- [S6] Neo4j Operations Manual 2026.09, "Backup and restore planning": <https://neo4j.com/docs/operations-manual/current/backup-restore/planning/> (online backup and restore; full and differential backups; per-database backups; `neo4j-admin` commands).
- [S7] Neo4j Operations Manual, "Restore a database backup": <https://neo4j.com/docs/operations-manual/current/backup-restore/restore-backup/> (`--restore-until` point-in-time restore; `--overwrite-destination` on a stopped database).
- [S8] Neo4j Operations Manual, "Migrate a database": <https://neo4j.com/docs/operations-manual/current/database-administration/standard-databases/migrate-database/> (runs only on a stopped database).
- [S9] Neo4j Operations Manual, "Store formats": <https://neo4j.com/docs/operations-manual/current/database-internals/store-formats/> (a format change rewrites all data and changes internal node and relationship identifiers).
- [S10] Neo4j Upgrade and Migration Guide, "Introduction": <https://neo4j.com/docs/upgrade-migration-guide/current/> ("Neo4j does not support downgrades"; rollback by restoring a pre-upgrade backup; upgrades pass through each LTS version).
- [S11] Neo4j Cypher Manual, "Scalar functions": <https://neo4j.com/docs/cypher-manual/current/functions/scalar/> (Neo4j reuses internal identifiers of deleted elements).
- [S12] Neo4j Operations Manual, "Full import": <https://neo4j.com/docs/operations-manual/current/import/full-import/> (into a non-existent or empty database; `:ID` values connect relationships to nodes; `--skip-bad-relationships` and `--skip-duplicate-nodes` default to false).
- [S13] Neo4j Status Codes, "List of Neo4j error codes": <https://neo4j.com/docs/status-codes/current/errors/all-errors/> (`InvalidBookmark`, `DeadlockDetected`, `DatabaseNotFound`, `TransactionAccessedConcurrently`, `ForbiddenDueToTransactionType`).
- [S14] Neo4j Operations Manual 2026.09, "Configuration settings": <https://neo4j.com/docs/operations-manual/current/configuration/configuration-settings/> (`db.shutdown_transaction_end_timeout` 10s; `dbms.routing_ttl` 5m; `server.bolt.advertised_address`; `db.temporal.timezone` Z; `db.logs.query.enabled` VERBOSE; `db.logs.query.parameter_logging_enabled` true; `dbms.memory.transaction.total.max`).
- [S15] Neo4j Operations Manual, "Metrics reference": <https://neo4j.com/docs/operations-manual/current/monitoring/metrics/reference/> (metric categories).
- [S16] Neo4j Operations Manual, "Show and terminate transactions" (reached through the Cypher Manual address): <https://neo4j.com/docs/cypher-manual/current/clauses/transaction-clauses/> (users always see and may terminate their own transactions; others need privileges; default columns `database`, `transactionId`, `currentQueryId`, `connectionId`, `clientAddress`, `username`, `currentQuery`, `startTime`, `status`, `elapsedTime`; optional `metaData` and `parameters`).
- [S17] Neo4j Cypher Manual, "Understanding query plans": <https://neo4j.com/docs/cypher-manual/current/planning-and-tuning/execution-plans/> (`EXPLAIN` plans without running; `PROFILE` runs and reports measurements).
- [S18] Neo4j Cypher Manual, "Instant type functions": <https://neo4j.com/docs/cypher-manual/current/functions/temporal/> (transaction, statement and realtime clocks; the realtime clock is "the live clock of the system").
- [S19] Memgraph, "Data types": <https://memgraph.com/docs/fundamentals/data-types> (database time zone defaults to UTC; dependence on the system `tzdata`; CRSs with SRIDs 4326, 4979, 7203, 9157).
- [S20] Neo4j Cypher Manual, "Spatial values": <https://neo4j.com/docs/cypher-manual/current/values-and-types/spatial/> (CRSs and SRIDs; distance in metres for WGS-84, Euclidean for Cartesian, null across CRSs; point index).
- [S21] Neo4j Operations Manual, "System requirements": <https://neo4j.com/docs/operations-manual/current/installation/requirements/> (fsync required for ACID; on Linux only ext4 and XFS are supported; NFS does not meet the POSIX requirement).
- [S22] PostgreSQL 18 documentation, "Error Handling", `data_sync_retry`: <https://www.postgresql.org/docs/current/runtime-config-error-handling.html> (default off: a flush failure raises PANIC and recovery replays the WAL).
- [S23] Neo4j Cypher Manual, "Currently unsupported mandatory GQL features": <https://neo4j.com/docs/cypher-manual/current/appendix/gql-conformance/unsupported-mandatory/> (session and transaction commands handled through driver APIs).
- [S24] Neo4j Python Driver Manual, "Run your own transactions": <https://neo4j.com/docs/python-manual/current/transactions/> (managed transactions are retried automatically on transient failures).
- [S25] Memgraph, "Client libraries": <https://memgraph.com/docs/client-libraries> (Memgraph supports Bolt 1, 4.0, 4.1, 4.3 and 5.2).
- [S26] Neo4j Graph Data Science, "Graph Algorithms" (operations reference, algorithm references): <https://neo4j.com/docs/graph-data-science/current/operations-reference/algorithm-references/>.
- [S27] Memgraph, "Available algorithms": <https://memgraph.com/docs/advanced-algorithms/available-algorithms>, and the MAGE source <https://github.com/memgraph/mage> at commit `c72636014a8b144b5ec1969a8de39f5369a53f5c`.
- [S28] petgraph 0.8.3 source: <https://github.com/petgraph/petgraph> at commit `a4d94bd2c39ac198c22682b2dcb1ff21583d0db0`, module `crates/petgraph/src/algo`.
- [S29] graphalgs 0.2.0 source: <https://github.com/starovoid/graphalgs> at commit `1f8a412ef4afb944177b5d0fee335e46314f903f`.
- [S30] NetworkX 3.7 source: <https://github.com/networkx/networkx> at tag `networkx-3.7`, package `networkx/algorithms`.
- [S31] Neo4j Operations Manual, "Create databases": <https://neo4j.com/docs/operations-manual/current/database-administration/standard-databases/create-databases/> (`CREATE DATABASE` fails if the database exists).
- [S32] LDBC, "LDBC Social Network Benchmark (LDBC SNB)": <https://ldbcouncil.org/benchmarks/snb/> (Interactive and Business Intelligence workloads).
- [S33] Memgraph, "Configuration": <https://memgraph.com/docs/database-management/configuration> (`--memory-limit=0` means 100% of physical memory with swap enabled, 90% otherwise).
- [S34] Neo4j Cypher Manual, "Show indexes": <https://neo4j.com/docs/cypher-manual/current/indexes/search-performance-indexes/list-indexes/> (default columns `id`, `name`, `state`, `populationPercent`, `type`, `entityType`, `labelsOrTypes`, `properties`, `indexProvider`, `owningConstraint`, `lastRead`, `readCount`).
- [S35] Neo4j Cypher Manual, "Show constraints": <https://neo4j.com/docs/cypher-manual/current/schema/constraints/list-constraints/> (default columns `id`, `name`, `type`, `entityType`, `labelsOrTypes`, `properties`, `enforcedLabel`, `ownedIndex`, `propertyType`).
- [S36] Neo4j Operations Manual, "Show databases": <https://neo4j.com/docs/operations-manual/current/database-administration/standard-databases/listing-databases/> (default columns include `name`, `type`, `aliases`, `access`, `currentStatus`, `default`, `home`).

The ISO/IEC 39075:2024 text has not yet been consulted. Every GQL feature identifier cited here (for example GG01, GG02, GV39) is unverified until it is checked against that text; see [OQ-005](open-questions.md#oq-005).
