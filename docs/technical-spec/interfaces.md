---
title: Interfaces
status: draft
last-reviewed: 2026-10-08
related:
  - architecture.md
  - bolt.md
  - ../functional-spec/application-abstractions.md
---

# Interfaces

## Public API shape

| Handle or item | Operations | Requirements |
|---|---|---|
| `Database` | `create(location, storage, memory_cap, owner, options)`, `open(location, storage, memory_cap, options)`, `close(timeout)` (default 10 s), `session(principal, home)`, `register_procedure`, `register_function`, `backup`, `restore`, `migrate` (on a closed location), `check_integrity` | FR-DB-001..009, FR-AUTH-002, FR-LIM-003, FR-API-005, FR-OPS-001..013, FR-REC-010 |
| `Session` | `execute(gql, parameters)`, `begin(options)` (isolation, limits), `explain`, `profile`, `cancel`, session limits; index and constraint management; `list_transactions`, `terminate_transaction`; `import`, `export` | FR-API-001, FR-API-004, FR-API-007..009, FR-GQL-005, FR-OBS-003..006, FR-SES-001..004, FR-TX-004, FR-IMP-001..006 |
| `Transaction` | `execute`, `commit`, `rollback`, `cancel`, `create_directory`, algorithm runs | FR-TX-001, FR-API-009, FR-CAT-004, FR-CAT-005, FR-ALG-002 |
| `ResultStream` | `async fn next()` returning one row; `summary()` | FR-API-010 |
| `bolt::Server` (feature `bolt`) | `new(database, advertised_address, default_limits, auth)`, `serve(connection)` | FR-BOLT-001, FR-BOLT-009, FR-LIM-004, ABS-001, ABS-002 |
| `bolt::Client` (feature `bolt`) | `connect(connection, auth)`, then auto-commit queries, transactions, `PULL`, bookmarks | FR-BOLT-002, FR-BOLT-019 |
| `Error` | GQLSTATUS code, message, cause | FR-BOLT-006 |

- `memory_cap` bounds the memory the crate accounts for. Page images and the free-page set are outside it, so one database can use up to about 3× `memory_cap` ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)).
- Every operation that performs I/O or waits is `async` ([TS-02](architecture.md#ts-02-async-api-and-threading)).
- Operations that need a principal live on `Session` or `Transaction`, never on `Database`. Each of them is authorised and records an owner ([TS-23](#ts-23-authentication-and-authorisation)), and the catalog changes join a transaction (FR-CAT-004).
- The exception is `create`: its required `owner` argument is the principal recorded as the owner of the default schema and default graph ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics); user decision 2026-10-07). `create` makes no authorisation request ([TS-23](#ts-23-authentication-and-authorisation)).
- GQLSTATUS codes come from the ISO conditions insert ([S45]) where it defines one. Otherwise they are Neo4j's codes [S48], such as 25N11 and 50N05 ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)). Codes that depend on rules outside the inserts are provisional (OQ-005).
- **`ResultStream::summary()`**, available once the last row is pulled:
  - counters, as Neo4j's `SummaryCounters` (D-N4DRIVER): nodes created and deleted, edges created and deleted (Neo4j's relationships), properties set, labels added and removed, catalog updates (Neo4j's `systemUpdates`, here the catalog objects a statement creates or drops), and whether any is above zero. There are no index or constraint counters: those change only through the API (FR-API-004);
  - the statement type, as Bolt's `type` (D-BOLTMSG): read, write, read-write or catalog;
  - the completion GQLSTATUS: 00000 for a result with rows, 02000 ("no data") for a result without rows, 00001 ("omitted result") for a statement with no result columns. All three are ISO conditions, and Neo4j uses the same three (D-N4STATUS, "Notifications");
  - for PROFILE, the profiled plan ([TS-20](query-engine.md#ts-20-binder-planner-and-execution)).

### GQLSTATUS codes

Each failure and its code (rmp task #39, point 5; rule above; D-N4STATUS). ISO lines are those of `ISO_IEC_39075(en)-conditions.xml`; Neo4j lines those of R-NEO `community/neo4j-gql-status/.../GqlStatusInfoCodes.java` @ `54a7dcf`, read for study only. A code that only matches Neo4j's description, with no Neo4j use of it for that case confirmed, is marked "by description".

| Failure (error kind) | GQLSTATUS | Source |
|---|---|---|
| Text `GQL.g4` does not accept | 42001 | ISO (124) |
| A construct of an unsupported feature | 42001, with the cause 51N26 naming the feature code | Neo4j's helper for an unsupported construct builds exactly this pair (R-NEO `.../gqlstatus/GqlHelper.java:1234-1244`); provisional (OQ-005) |
| A name in a statement that resolves to nothing | 42002 | ISO (125) |
| No such procedure; no such function | 42N08; 42N48 | Neo4j (2090; 2341) |
| A data exception (overflow, division by zero, invalid value type, string too long, …) | its ISO class-22 code, such as 22003, 22012, 22G03, 22001 | ISO (70-111) |
| `START TRANSACTION` while a transaction is active | 25G01 | ISO (114) |
| `COMMIT` or `ROLLBACK` with no active transaction | 2D000 | ISO class 2D (119) |
| A request on a closed session | 08000 | ISO class 08 (67) |
| `Serialization` | 25N11 | [TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks) |
| `Deadlock` | 50N05 | [TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks) |
| A request on a busy session (FR-SES-004) | 25N03 | Neo4j (1457) |
| A transaction cancelled (FR-API-009), terminated (FR-OBS-004) or timed out (FR-API-007) | 25N14, with the reason in the message | Neo4j: `Terminated` and `TransactionTimedOut` are client errors (R-NEO `community/common/.../exceptions/Status.java:151-154, 169`), and a client-error termination is 25N14 (`community/graphdb-api/.../TransactionTerminatedHelper.java:47-50`) |
| `Aborted`: a statement of a transaction an earlier failure aborted | 25N05 | Neo4j "transaction closed" (1466), by description |
| `MemoryLimit` of the memory cap | 51N72 | Neo4j "memory pool out of memory" (3380) |
| The transaction memory limit (FR-API-007) | 51N73 | Neo4j "transaction memory limit reached" (3387) |
| A row or byte limit (FR-LIM-002) | 51N66 | Neo4j "resource exhaustion" (3347), by description: no reference has a result-size limit |
| A refusal of the authoriser (FR-AUTH-001) | 42NFF | Neo4j "permission/access denied" (2797) |
| A read-only routine that writes (FR-PROC-002) | 42N02 | Neo4j "writing in read access mode" (2052), by description |
| A closed graph type violated | G2000 | ISO class G2, "graph type violation" (147) |
| `EdgesStillExist` | G1001 | ISO (143) |
| `GraphTypeInUse`, `NotEmpty` | G1000 | ISO class G1, "dependent object error" (142) |
| `CatalogObjectExists`, `DatabaseExists` | 42N11 | Neo4j "graph reference already exists" (2150), by description |
| `NotFound` through the API: a graph, schema, graph type or directory; an index; a constraint | 22N51; 22N69; 22N64 | Neo4j (839; 938; 911); by description for a schema, graph type or directory |
| `NoDatabase` | 22N51 | Neo4j "graph reference not found" (839), by description |
| `ConstraintViolation`: uniqueness or key; existence; property type | 22N79; 22N77; 22N78 | Neo4j (1003; 979; 991) |
| `InvalidArgument` | 22N11 | Neo4j (601) |
| An open that fails: `Locked`, `LockMissing`, `MigrationRequired`, `NewerFormat`, `Corruption` found at open | 51N40 | Neo4j "unable to start database" (3196), by description |
| `Unusable`, `Corruption` found while open, any call after `close` | 08N09 | Neo4j "database unavailable" (352), by description |
| `IncompleteBackup`, `LoadRunning`, `NotCleanlyClosed`, `MigrationPath` | 51N41 | Neo4j "admin operation failed" (3201-3205), by description |
| `Io`, `TreeDropped`, any other internal failure | 50N00 | Neo4j "internal error" (2812) |

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
  - After planning and before execution, the crate asks once per statement for every `(action, object)` pair the plan can perform. API operations ask once per call, except `create`, which runs before any session or principal exists and makes no request ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics) Creation). A refusal fails the request before any effect. Consequence: a statement fails if its plan could perform a refused action, even when the data would not have led it to.
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
  - **Names and calls** (provisional, OQ-005).
    - A `CALL` names its routine with `procedureReference`: an optional parent and a name, or a reference parameter (`GQL.g4:1458-1465`). No GQL statement creates a procedure, so no catalog object is one.
    - A database handle's registry holds the built-in and the application's routines in one namespace. A reference whose parent is object names only, such as `gds.pageRank.stream`, is looked up by its whole dotted text. A reference with a schema part, or a `$$` reference parameter, names no registered routine and fails with 42N08.
    - `GQL.g4` has no function call in value expressions (`valueExpressionPrimary`, `GQL.g4:2220-2235`). A registered function is therefore called with `CALL` (FR-API-006): it is a routine with exactly one result column, declared when it is registered, that returns exactly one row.
    - The binder checks a routine's mode wherever the call stands: the oracle parses a lone `CALL` as a catalog-modifying statement ([TS-19](query-engine.md#ts-19-parser) Ambiguities).
  - **Built-in procedures.**
    - The four catalog-listing procedures `show_transactions`, `show_indexes`, `show_constraints` and `show_catalog`.
    - The algorithm procedures, registered under the names, parameters, modes and output columns that FR-ALG-007 and the inventory table fix (for example `gds.pageRank.stream`, `gds.pageRank.write`; [TS-25](algorithms.md#ts-25-graph-algorithms)).
    - **Point procedures** (user decision 2026-10-08, "Valores + CALL", OQ-006; [TS-21](values-and-types.md#ts-21-values-and-types)). Mode `ReadOnly`. Arguments are positional: a record argument such as Neo4j's `{x: 1, y: 2}` needs GV45, which is not supported. `INTEGER` arguments are accepted for `FLOAT` parameters (IA005).

      | Call | Yields | Semantics |
      |---|---|---|
      | `point(x :: FLOAT, y :: FLOAT, z :: FLOAT = NULL, srid :: INTEGER = NULL)` | one row: `point` | A point. With `srid` null: SRID 7203, or 9157 when `z` is given. With `srid` 4326 or 4979: `x` is the longitude, `y` the latitude, `z` the height. `z` must be given exactly when the SRID is 9157 or 4979, and any other SRID fails; both fail with 22N11. If `x` or `y` is null, `point` is null. |
      | `point.distance(from :: POINT, to :: POINT)` | one row: `distance` (`FLOAT`) | The distance of TS-21 Spatial: metres for WGS-84, coordinate units for Cartesian. Null if either point is null or their CRSs differ. |
      | `point.nodesWithinDistance(label :: STRING, property :: STRING, center :: POINT, distance :: FLOAT)` | one row per node: `node`, `distance` | Every node of the working graph that has `label` and whose `property` is a point in `center`'s CRS at a distance of at most `distance` from it. Rows have no order. No rows when `center` or `distance` is null. |
      | `point.edgesWithinDistance(edgeType :: STRING, property :: STRING, center :: POINT, distance :: FLOAT)` | one row per edge: `edge`, `distance` | The same for edges of `edgeType`. |

      - **Index.** The two searches use the ONLINE point index on `(label or edge type, property)` when one exists (`index_within`, `src/index/mod.rs:1241`; [TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints)). Otherwise they scan the label or edge type with the same filter, so the rows are the same with and without the index (FR-TYPE-005).
      - Not specified: `point.withinBBox` and point equality, which Neo4j's point index also serves. No requirement needs them.
    - The validity of these names as GQL procedure references is provisional (OQ-005).
  - **Metrics.** An own sink trait per database handle with `counter`, `gauge` and `histogram` calls and static metric names, grouped by the categories of FR-OBS-001. Names of the Sprint 3 categories, Neo4j's without its `<prefix>.`:
    - query execution: `db.query.execution.success` and `db.query.execution.failure` (counters), `db.query.execution.latency.millis` (histogram of successful executions, in milliseconds);
    - data counts: `neo4j.count.node`, `neo4j.count.relationship` and `neo4j.count.relationship_types` (gauges; the last is the number of edge-type tokens);
    - indexes: `index.<kind>.queried` and `index.<kind>.populated` (counters), with `<kind>` `lookup`, `range` or `point` as in Neo4j, and `single_property` or `composite` for the two kinds Neo4j does not have (lpg names).
  - **Events.** `tracing` events per request carry the query text and `tx_metadata`, never parameter values.
    - Target `lpg::query`: one span per request, with fields `query`, `tx_metadata`, `principal`, `transaction_id`. Events at `INFO`: `start`; `success` with `elapsed_ms`, `peak_memory_bytes`, `buffer_hits`, `buffer_misses`; `fail` with `gqlstatus` and `message`.
    - Target `lpg::transaction`: events at `INFO` for `start`, `commit` and `rollback`, with `transaction_id`, `principal` and `tx_metadata`.
    - No field, message or `Debug` output holds a parameter value. Error messages are built without values (IV016, [TS-20](query-engine.md#ts-20-binder-planner-and-execution)).
- **Satisfies**: ABS-004, ABS-005, FR-API-005, FR-API-006, FR-PROC-001..004, FR-LIM-006, FR-ALG-002, FR-ALG-007, FR-OBS-001, FR-OBS-002, FR-OPS-005, FR-OPS-006, FR-BOLT-018.
- **Evidence**:
  - Neo4j checks procedure modes at run time (`procedure-api/.../Mode.java`, `kernel/.../ProcedureCaller.java`; R-NEO).
  - Memgraph passes an immutable graph to read procedures (R-MG `src/query/procedure/mg_procedure_impl.cpp`; line references in [M-QE](../evidence/research/research-03-query-engine.md) §Q8).
  - The `metrics` crate supports one process-global recorder, or a thread-scoped one inside a closure (D-METRICS).
  - Point procedures. Neo4j's `point(input)` builds a point from a map whose `x`, `y` without `crs` or `srid` default to Cartesian 7203 (9157 with `z`), or with `srid` 4326 or 4979 read as longitude and latitude; `point.distance(from, to)` returns a `FLOAT`, null for a null argument or for points in different CRSs (Cypher Manual, "Spatial functions", checked 2026-10-08). Memgraph has the same `point(map)` and `point.distance(x, y)` (Memgraph documentation, "Functions", checked 2026-10-08). Neo4j's point index solves `point.distance(n.prop, center) <= distance` (Cypher Manual, "Create indexes", point indexes, "Supported predicates"). Neither reference has a search procedure: they write the search as a predicate, which needs a function call GQL lacks; the search procedures' names are lpg's.
  - Names: no GQL statement creates a procedure, so a named call waits "on a name the standard never supplies" (R-GQLC README, on GP04, GP14 and GP15).
  - Metric names: Neo4j Operations Manual, "Metrics reference", tables of query execution, data count and index metrics (checked 2026-10-08).
  - Events: Neo4j logs query events `start`, `fail` and `success`, and transaction events `start`, `commit` and `rollback`, at `INFO`, with the query text, the executing user, `annotationData` (the transaction metadata), the elapsed time, page hits and faults and the error's GQLSTATUS; parameters only when parameter logging is on (Neo4j Operations Manual, "Logging", query log fields, checked 2026-10-08). FR-OBS-002 removes the parameters.
- **Crates**: `tracing` is a dependency. `metrics` is not used: it has no recorder per database handle.
- **Rejected**:
  - Run-time-only write checks: the types remove the check from the hot path.
  - Async procedures: they run on the blocking statement thread.
