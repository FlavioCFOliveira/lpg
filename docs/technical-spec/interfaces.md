---
title: Interfaces
status: draft
last-reviewed: 2026-10-02
related:
  - architecture.md
  - bolt.md
  - ../functional-spec/application-abstractions.md
---

# Interfaces

## Public API shape

| Handle or item | Operations | Requirements |
|---|---|---|
| `Database` | `create(location, storage, memory_cap, options)`, `open(location, storage, memory_cap, options)`, `close(timeout)` (default 10 s), `session(principal, home)`, `register_procedure`, `register_function`, `backup`, `restore`, `migrate` (on a closed location), `check_integrity` | FR-DB-001..009, FR-LIM-003, FR-API-005, FR-OPS-001..013, FR-REC-010 |
| `Session` | `execute(gql, parameters)`, `begin(options)` (isolation, limits), `explain`, `profile`, `cancel`, session limits; index and constraint management; `list_transactions`, `terminate_transaction`; `import`, `export` | FR-API-001, FR-API-004, FR-API-007..009, FR-GQL-005, FR-OBS-003..006, FR-SES-001..004, FR-TX-004, FR-IMP-001..006 |
| `Transaction` | `execute`, `commit`, `rollback`, `cancel`, `create_directory`, algorithm runs | FR-TX-001, FR-API-009, FR-CAT-004, FR-CAT-005, FR-ALG-002 |
| `ResultStream` | `async fn next()` returning one row; `summary()` | FR-API-010 |
| `bolt::Server` (feature `bolt`) | `new(database, advertised_address, default_limits, auth)`, `serve(connection)` | FR-BOLT-001, FR-BOLT-009, FR-LIM-004, ABS-001, ABS-002 |
| `bolt::Client` (feature `bolt`) | `connect(connection, auth)`, then auto-commit queries, transactions, `PULL`, bookmarks | FR-BOLT-002, FR-BOLT-019 |
| `Error` | GQLSTATUS code, message, cause | FR-BOLT-006 |

- Every operation that performs I/O or waits is `async` ([TS-02](architecture.md#ts-02-async-api-and-threading)).
- Operations that need a principal live on `Session` or `Transaction`, never on `Database`. Each of them is authorised and records an owner ([TS-23](#ts-23-authentication-and-authorisation)), and the catalog changes join a transaction (FR-CAT-004).
- GQLSTATUS codes come from the ISO conditions insert ([S45]) where it defines one. Otherwise they are Neo4j's codes [S48], such as 25N11 and 50N05 ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)). Codes that depend on rules outside the inserts are provisional (OQ-005).

## TS-03 Storage abstraction

- **Decision**: a synchronous, dyn-compatible trait at file level, implemented by the application or by the crate's default.
  - Store operations: `open`, `create`, `remove`, `rename`, `list`, `sync_dir`, `try_lock`, `available_space`.
  - File operations: `read_at`, `write_at`, `sync_data`, `set_len`, `len`, `reserve(offset, len)`.
  - The default uses `std::fs` with `std::os::unix::fs::FileExt`, and `rustix` for `available_space` (`fstatvfs`) and `reserve` (`fallocate`). `std` offers neither.
  - Every file the crate touches passes through the trait: `CONTROL`, `LOCK`, data, WAL, backup, restore, import, export, migration and sort files.
- **Satisfies**: ABS-003, FR-STO-002, NFR-VER-004, FR-REC-006, FR-REC-007, FR-DB-006, FR-OPS-011.
- **Evidence**:
  - redb `StorageBackend` (`len`, `read`, `set_len`, `sync_data`, `write`, `close`; R-REDB `src/db.rs:74`).
  - turso `IO` and `File` traits (R-TURSO `core/io/mod.rs:153, 424`).
  - SQLite's crash-simulating VFS (R-SQLITE `src/test6.c`).
  - PostgreSQL reserves file space with `posix_fallocate` (R-PG `src/backend/storage/file/fd.c:2396-2425`).
  - `rustix::fs::fstatvfs` and `rustix::fs::fallocate` are safe functions (D-RUSTIX).
- **Crates**: `rustix` 1.1.5 (feature `fs`): dependency. `fs4` (which wraps `rustix` for `available_space`) and `nix`: not used.
- **Rejected**:
  - An async trait: the engine core is synchronous ([TS-02](architecture.md#ts-02-async-api-and-threading)).
  - Block-level or memory-mapped I/O: mmap needs `unsafe` ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
  - Reserving space by writing zeros: twice the writes for every extent.

## Transport (ABS-001)

The transport abstraction is the bound `tokio::io::AsyncRead + AsyncWrite + Unpin + Send + 'static` on the connection the application passes to the Bolt endpoints. See [TS-22](bolt.md#ts-22-bolt-and-packstream).

## TS-23 Authentication and authorisation

- **Decision**:
  - One dyn-compatible trait. Its methods return boxed futures (`Pin<Box<dyn Future<Output = …> + Send + '_>>`):
    - `authenticate(scheme, credentials)` → principal with optional home schema and home graph, or refusal;
    - `impersonate(principal, target)` → principal or refusal;
    - `authorise(principal, action, object, owner)` → allow or deny.
  - Actions use Neo4j's privilege names:
    - data actions: `TRAVERSE`, `READ`, `CREATE`, `DELETE`, `SET LABEL`, `REMOVE LABEL`, `SET PROPERTY`;
    - catalog actions: `CREATE` and `DROP` per kind (directory, schema, graph type, graph);
    - `CREATE INDEX`, `DROP INDEX`, `SHOW INDEX`, `CREATE CONSTRAINT`, `DROP CONSTRAINT`, `SHOW CONSTRAINT`;
    - `SHOW TRANSACTION`, `TERMINATE TRANSACTION`.
  - The object of a data action is its graph. The owner is the one recorded on the catalog object ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)).
  - After planning and before execution, the crate asks once per statement for every `(action, object)` pair the plan can perform. API operations ask once per call. A refusal fails the request before any effect. Consequence: a statement fails if its plan could perform a refused action, even when the data would not have led it to.
  - Rust API sessions receive their principal from the application and are not authenticated.
- **Satisfies**: ABS-002, FR-AUTH-001, FR-AUTH-002, BR-AUTH-001, FR-API-004, FR-SES-001, FR-SES-002, FR-BOLT-011, FR-BOLT-012, FR-BOLT-013, FR-OBS-003, FR-OBS-004.
- **Evidence**: Neo4j read, write and database privilege names (D-PRIV, [S46]); `async fn` in traits is not object-safe (D-AFIT).
- **Crates**: `async-trait` is not used; the boxed futures give the same dyn compatibility without a dependency.
- **Rejected**:
  - A check per element during execution (Neo4j's fine-grained model): the object of an action here is a catalog object, so the plan determines every check.
  - A synchronous trait: an application check may need I/O.

## TS-24 Procedures, functions, metrics and events

- **Decision**:
  - **Procedures and functions.**
    - Each database handle keeps a registry of typed signatures: name, parameters with defaults, result columns, and mode `ReadOnly` or `ReadWrite`. Functions add a result type. Nothing is persisted.
    - Implementations are synchronous and return row iterators. They run on the statement's thread with a context that gives the caller's snapshot, the cancellation status and the memory tracker.
    - Read-only is enforced in three ways: the read-only context type has no write methods; the binder rejects a `ReadWrite` procedure where only reads are allowed; and nested GQL run through a read-only context fails if it modifies data.
    - Rows a procedure returns are charged to the memory cap. Memory it allocates on its own is not.
  - **Built-in procedures.**
    - The four catalog-listing procedures `show_transactions`, `show_indexes`, `show_constraints` and `show_catalog`.
    - The algorithm procedures, registered under the names, parameters, modes and output columns that FR-ALG-007 and the inventory table fix (for example `gds.pageRank.stream`, `gds.pageRank.write`; [TS-25](algorithms.md#ts-25-graph-algorithms)).
    - The validity of these names as GQL procedure references is provisional (OQ-005).
  - **Metrics.** An own sink trait per database handle with `counter`, `gauge` and `histogram` calls and static metric names, grouped by the categories of FR-OBS-001.
  - **Events.** `tracing` events per request carry the query text and `tx_metadata`, never parameter values.
- **Satisfies**: ABS-004, ABS-005, FR-API-005, FR-API-006, FR-PROC-001..004, FR-LIM-006, FR-ALG-002, FR-ALG-007, FR-OBS-001, FR-OBS-002, FR-OPS-005, FR-OPS-006, FR-BOLT-018.
- **Evidence**:
  - Neo4j checks procedure modes at run time (`procedure-api/.../Mode.java`, `kernel/.../ProcedureCaller.java`; R-NEO).
  - Memgraph passes an immutable graph to read procedures (R-MG `src/query/procedure/mg_procedure_impl.cpp`; line references in [M-QE](../evidence/research/research-03-query-engine.md) §Q8).
  - The `metrics` crate supports one process-global recorder, or a thread-scoped one inside a closure (D-METRICS).
- **Crates**: `tracing` is a dependency. `metrics` is not used: it has no recorder per database handle.
- **Rejected**:
  - Run-time-only write checks: the types remove the check from the hot path.
  - Async procedures: they run on the blocking statement thread.
