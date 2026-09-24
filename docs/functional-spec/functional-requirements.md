---
title: Functional requirements
status: draft
last-reviewed: 2026-09-24
related:
  - scope.md
  - application-abstractions.md
  - non-functional-requirements.md
  - use-cases.md
  - graph-algorithms.md
  - glossary.md
  - open-questions.md
---

# Functional requirements

Each requirement records a user decision. A requirement marked "Neo4j default" or "Memgraph default" records the behaviour of that product, adopted under the user's instruction to assume the defaults of Neo4j and Memgraph for points the user did not decide, with Neo4j taking precedence where the two differ. External facts cite the [sources](README.md#sources) as [S1] to [S36]. Each requirement is followed by its acceptance criterion in Given/When/Then form.

## API

- FR-API-001 The crate SHALL provide an in-process Rust API that executes GQL requests and returns their results. This API is the crate's primary interface.
  - Acceptance: Given an open database, when the application submits a GQL request through the Rust API, then the crate returns the request's result or a GQLSTATUS error in the same process, with no Bolt endpoint involved.
- FR-API-002 The crate SHALL implement GQL sessions, transactions and catalog semantics and SHALL expose them through the Rust API.
  - Acceptance: Given an open database, when the application opens a session, starts a transaction, creates a graph and commits through the Rust API, then a new session sees the graph in the catalog.
- FR-API-003 The Bolt endpoints SHALL be an optional layer on top of the Rust API. An application SHALL be able to use the crate without them.
  - Acceptance: Given an application built without the Bolt endpoints, when it creates a database and runs GQL requests, then every Rust API operation succeeds.
- FR-API-004 The crate SHALL provide index and constraint management through the Rust API only.
  - Acceptance: Given an open database, when the application creates an index through the Rust API, then the index exists; and when a GQL request uses any index-management syntax that is not standard GQL, then the request fails with a GQLSTATUS syntax error.
- FR-API-005 The crate SHALL let the application register procedures and functions through the Rust API, using the procedures and functions abstraction ([ABS-005](application-abstractions.md#abstractions)).
  - Acceptance: Given an open database handle, when the application registers a procedure through the Rust API, then a GQL `CALL` of that procedure name on that handle succeeds.
- FR-API-006 WHEN a GQL request invokes a registered procedure or function with the standard `CALL` statement, the crate SHALL execute it.
  - Acceptance: Given a registered procedure that returns one row, when a request invokes it with `CALL`, then the result contains that row.
- FR-API-007 The crate SHALL enforce resource limits that are configurable per session and per transaction, covering memory, time and result size. The limits are detailed in [Resource limits](#resource-limits).
  - Acceptance: Given a session whose time limit is 1 second, when a statement runs longer than 1 second, then the statement fails with a GQLSTATUS error.
- FR-API-008 The crate SHALL support cooperative cancellation of a running query, requested through Bolt `RESET`, through the Bolt `tx_timeout` field or through the Rust API.
  - Acceptance: Given a running statement, when cancellation is requested through each of the three means, then in each case the statement stops and fails with a GQLSTATUS error.
- FR-API-009 WHEN the application cancels a running statement through the Rust API, the crate SHALL fail the statement with a GQLSTATUS error and SHALL abort its transaction, as Bolt `RESET` does ([FR-BOLT-015](#bolt)).
  - Acceptance: Given a transaction that has written a node and is running a second statement, when the application cancels the statement, then the statement fails with a GQLSTATUS error, the transaction is aborted, and the written node is not visible to any later transaction.
- FR-API-010 The crate SHALL stream results: the caller SHALL pull result rows incrementally, and the crate SHALL NOT require the whole result to be materialised before the first row is returned.
  - Acceptance: Given a query whose result has one million rows, when the caller pulls the first row, then the crate returns it before the remaining rows have been produced.

## Database lifecycle

- FR-DB-001 A database SHALL be one full GQL catalog: its directory hierarchy, schemas, graph types and multiple graphs ([FR-CAT-001](#catalog)).
  - Acceptance: Given a new database, when the application creates two schemas and three graphs in it, then all five objects belong to that database's catalog.
- FR-DB-002 One process MAY open several databases. The crate SHALL give access to each open database through an independent handle.
  - Acceptance: Given two databases at different paths, when one process opens both, then each handle runs requests against its own catalog only, and closing one handle leaves the other usable.
- FR-DB-003 Creating a database and opening a database SHALL be separate operations. IF the create operation targets a location that already holds a database, THEN the crate SHALL fail the operation with an error and SHALL leave the existing database unchanged (Neo4j default: `CREATE DATABASE` fails when the database exists [S31]).
  - Acceptance: Given a location that holds a database, when the application requests create at that location, then the operation fails with an error and the existing data is unchanged.
- FR-DB-004 IF the open operation targets a location that holds no database, THEN the crate SHALL fail the operation with an error and SHALL NOT create a database.
  - Acceptance: Given an empty location, when the application requests open, then the operation fails with an error and the location still holds no database.
- FR-DB-005 WHEN the crate creates a database, its catalog SHALL contain a default schema and a default graph. They are the session home fallback ([FR-SES-002](#sessions)).
  - Acceptance: Given a newly created database, when a session starts with no home schema or home graph supplied, then its home schema is the default schema and its home graph is the default graph.
- FR-DB-006 IF a process tries to open a database that another process has open, THEN the crate SHALL detect it and refuse the open with an error. This enforces [NG-4](scope.md#non-goals).
  - Acceptance: Given a database open in process A, when process B requests open of the same database, then process B receives an error and process A is unaffected.
- FR-DB-007 IF a process tries to open a database that the same process already has open, THEN the crate SHALL refuse the second open with an error.
  - Acceptance: Given a database open in a process, when the same process requests open of it again, then the request fails with an error and the first handle remains usable.
- FR-DB-008 WHEN the application closes a database that has active sessions or transactions, the crate SHALL wait for them to end for up to a timeout and SHALL then abort the transactions that remain. The timeout SHALL default to 10 seconds (Neo4j default: `db.shutdown_transaction_end_timeout` is 10 s [S14]); the application MAY pass another value to the close operation.
  - Acceptance: Given an open transaction that does not end, when the application closes the database with the default timeout, then close completes about 10 seconds later, the transaction is aborted and its effects are not visible after reopening.
- FR-DB-009 IF a database handle is dropped without being closed, THEN the crate SHALL treat it as a crash; the next open SHALL run recovery ([FR-REC-001](#recovery)).
  - Acceptance: Given a handle with committed and uncommitted transactions, when the handle is dropped without close and the database is reopened, then every committed transaction is present and no uncommitted effect is present.

## Sessions

- FR-SES-001 WHEN the application opens a session through the Rust API, it SHALL supply the session's principal directly. The crate SHALL trust that principal and SHALL NOT authenticate it.
  - Acceptance: Given an open database, when the application opens a session with principal "p1", then the session runs requests as "p1" without calling the authentication part of [ABS-002](application-abstractions.md#abstractions).
- FR-SES-002 The crate SHALL set a new session's home schema and home graph from the values the application supplies (Rust API) or that the authentication and authorisation abstraction returns (Bolt). IF no value is supplied, THEN the crate SHALL use the catalog's default schema and default graph ([FR-DB-005](#database-lifecycle)).
  - Acceptance: Given a session opened with home graph "g1", when a request reads the home graph, then it reads "g1"; and given a session opened with no home graph, then it reads the default graph.
- FR-SES-003 A new session's time zone SHALL be UTC (Neo4j default: `db.temporal.timezone` is `Z` [S14]; Memgraph default: UTC [S19]).
  - Acceptance: Given a new session with no time-zone setting, when a request obtains the current local date-time, then its offset is +00:00.
- FR-SES-004 IF a session receives a request while another request of the same session is running, THEN the crate SHALL refuse the second request with a GQLSTATUS error and SHALL continue the first (comparable Neo4j behaviour: `Neo.ClientError.Transaction.TransactionAccessedConcurrently` [S13]).
  - Acceptance: Given a session running a long statement, when a second request is submitted on the same session, then the second request fails with a GQLSTATUS error and the first completes normally.

## Authorisation

- FR-AUTH-001 Before each catalog action and each data action, the crate SHALL ask the authentication and authorisation abstraction ([ABS-002](application-abstractions.md#abstractions)) whether the session's principal may perform that action on that object. IF the abstraction refuses, THEN the crate SHALL fail the statement with a GQLSTATUS error and SHALL NOT perform the action.
  - Acceptance: Given an abstraction that refuses the action "create node" on graph "g1" for principal "p1", when "p1" runs a statement that creates a node in "g1", then the statement fails with a GQLSTATUS error and no node is created.
- FR-AUTH-002 The crate SHALL record an owner on each catalog object and SHALL pass the owner to the authentication and authorisation abstraction with each authorisation request that concerns that object.
  - Acceptance: Given a graph created by principal "p1", when any principal acts on that graph, then the authorisation request the abstraction receives names "p1" as the owner.
- BR-AUTH-001 Authorisation SHALL add no GQL syntax. Permissions are managed by the application through the abstraction ([BR-GQL-001](#gql)).
  - Acceptance: Given the crate's supported syntax, when it is compared with ISO/IEC 39075, then it contains no statement for granting or revoking permissions that the standard does not define.

## GQL

- FR-GQL-001 The crate SHALL target full conformance to ISO/IEC 39075:2024: minimum conformance plus all optional features.
  - Acceptance: Given the published list of supported feature identifiers ([FR-GQL-002](#gql)), when version 1.0 is released, then the list contains every optional feature of ISO/IEC 39075:2024 ([NFR-REL-005](non-functional-requirements.md#release)).
- FR-GQL-002 The project SHALL deliver GQL conformance in phases and SHALL publish the list of supported GQL feature identifiers.
  - Acceptance: Given any release, when its documentation is inspected, then it contains the list of supported GQL feature identifiers, and conformance tests exist for each listed feature ([NFR-VER-001](non-functional-requirements.md#verification)).
- FR-GQL-003 The crate SHALL follow the latest edition of ISO/IEC 39075 together with its published corrigenda. WHEN a new edition is published, the project SHALL evaluate it.
  - Acceptance: Given a published corrigendum or new edition, when the next release is prepared, then the specification records its evaluation.
- FR-GQL-004 The crate SHALL support open graph types (feature GG01) and closed graph types (feature GG02). The feature identifiers are unverified (OQ-005).
  - Acceptance: Given a closed graph type, when a statement inserts a node whose label the type does not allow, then the statement fails with a GQLSTATUS error; and given an open graph, when the same statement runs, then it succeeds.
- FR-GQL-005 The crate SHALL support GQL session and transaction statements both in GQL text and through the Rust API, with identical semantics. Over Bolt, [FR-BOLT-016](#bolt) applies.
  - Acceptance: Given two sessions, when one starts and commits a transaction with GQL text and the other with the Rust API, then both transactions have the same observable effects.
- BR-GQL-001 The crate SHALL accept only standard GQL syntax and SHALL add no syntax of its own. Index and constraint management ([FR-API-004](#api)) and extensions ([FR-API-005](#api)) are provided without new syntax.
  - Acceptance: Given a request that uses syntax not defined by ISO/IEC 39075, when the crate parses it, then it fails with a GQLSTATUS syntax error.
- BR-GQL-002 Each choice that ISO/IEC 39075 leaves implementation-defined SHALL be decided one by one, with the user's explicit approval, and SHALL be recorded in this specification. Until the normative text is available, these choices stay open under [OQ-005](open-questions.md#oq-005).
  - Acceptance: Given an implementation-defined choice implemented by the crate, when the specification is inspected, then it records that choice and its approval by the user.
- FR-GQL-006 The project SHALL deliver the optional GQL features in this order: first those that Neo4j or Memgraph already supports, then the rest (user decision 2026-09-24). The concrete list depends on the ISO/IEC 39075 text (OQ-005).
  - Acceptance: Given the release history, when the delivered optional features are compared, then every delivered feature that neither Neo4j nor Memgraph supports was delivered no earlier than the last feature that one of them supports.

## Catalog

- FR-CAT-001 The crate SHALL provide the full GQL catalog: the directory hierarchy, schemas, graph types and multiple graphs per database ([FR-DB-001](#database-lifecycle)).
  - Acceptance: Given an open database, when the application creates a directory, a schema in it, a graph type and two graphs, then all are listed by the catalog-listing procedure ([FR-PROC-004](#procedures-and-functions)).
- FR-CAT-002 The crate SHALL persist the catalog.
  - Acceptance: Given a committed catalog change, when the database is closed and reopened, then the change is present.
- FR-CAT-003 The crate SHALL execute queries that span more than one graph of the catalog.
  - Acceptance: Given two graphs in one database, when one query reads both, then its result combines data from both graphs.
- FR-CAT-004 Catalog changes SHALL be fully transactional: a transaction MAY mix catalog changes and data changes, and a rollback SHALL undo both. Neo4j forbids mixing schema changes and data changes in one transaction (`Neo.ClientError.Transaction.ForbiddenDueToTransactionType` [S13]); the user's decision overrides that default.
  - Acceptance: Given a transaction that creates a graph and inserts nodes into it, when the transaction rolls back, then neither the graph nor the nodes exist.

## Indexes and constraints

- FR-IDX-001 The crate SHALL provide these index kinds: single-property, composite, label or edge-type lookup, range (ordered), and point ([FR-TYPE-005](#types)).
  - Acceptance: Given a graph, when the application creates one index of each kind through the Rust API, then each creation succeeds and the index-listing procedure lists five indexes with their kinds.
- FR-IDX-002 The crate SHALL provide these constraint kinds: uniqueness, property existence, property type, and node key.
  - Acceptance: Given a node-key constraint on label L and property p, when a statement inserts an L node without p, or with a p value already used, then the statement fails with a GQLSTATUS error.
- FR-IDX-003 The crate SHALL define each index and each constraint on one graph.
  - Acceptance: Given a uniqueness constraint on graph "g1", when a statement inserts duplicate values in graph "g2", then the statement succeeds.
- FR-IDX-004 IF existing data violates a constraint that the application creates, THEN the creation SHALL fail with an error and the constraint SHALL NOT exist.
  - Acceptance: Given two nodes with the same value of p, when the application creates a uniqueness constraint on p, then creation fails and no constraint exists.
- FR-IDX-005 WHILE an index is being built, the crate SHALL accept writes to its graph.
  - Acceptance: Given an index build in progress on a large graph, when a transaction writes to that graph, then the transaction commits without waiting for the build to finish, and the finished index includes the write.

## Procedures and functions

- FR-PROC-001 The crate SHALL call registered procedures and functions through the procedures and functions abstraction ([ABS-005](application-abstractions.md#abstractions)).
  - Acceptance: Given a procedure registered through ABS-005, when a request invokes it with `CALL`, then the application's implementation is called once with the request's arguments.
- FR-PROC-002 Each registered procedure SHALL be declared read-only or read-write. A read-write procedure MAY modify data and SHALL run in the caller's transaction. IF a read-only procedure attempts to modify data, THEN the crate SHALL fail the statement with a GQLSTATUS error.
  - Acceptance: Given a read-write procedure that creates a node, when a transaction calls it and then rolls back, then the node does not exist; and given a read-only procedure that attempts the same, then the statement fails with a GQLSTATUS error.
- FR-PROC-003 Registrations SHALL belong to one database handle and SHALL NOT be persisted. The application SHALL register its procedures and functions again after each open.
  - Acceptance: Given a procedure registered on a handle, when the database is closed and reopened, then a `CALL` of that procedure fails with a GQLSTATUS error until it is registered again.
- FR-PROC-004 The crate SHALL provide four built-in procedures, invoked with `CALL`. Their names and result columns follow the Neo4j `SHOW` commands that list the same objects, adapted to GQL `CALL` and to a single embedded database; Neo4j columns that concern clusters, servers, connections, index usage tracking or Java memory are left out (user decision 2026-09-24). Whether these names are valid GQL procedure names is unverified until checked against ISO/IEC 39075 (OQ-005).
  - `show_transactions()` lists the active transactions ([FR-OBS-003](#observability)), following `SHOW TRANSACTIONS` [S16]: `transactionId`, `username` (the principal), `currentQuery`, `startTime`, `status`, `elapsedTime`, `metaData` (the `tx_metadata`). Neo4j's `parameters` column is left out ([FR-OBS-002](#observability)).
  - `show_indexes()` lists the indexes, following `SHOW INDEXES` [S34]: `id`, `name`, `state`, `populationPercent`, `type`, `entityType`, `labelsOrTypes`, `properties`, `owningConstraint`, plus `graph` (the graph the index is defined on, [FR-IDX-003](#indexes-and-constraints)).
  - `show_constraints()` lists the constraints, following `SHOW CONSTRAINTS` [S35]: `id`, `name`, `type`, `entityType`, `labelsOrTypes`, `properties`, `propertyType`, `ownedIndex`, plus `graph`.
  - `show_catalog()` lists the catalog objects, following `SHOW DATABASES` [S36], which lists Neo4j's databases: `name` (the catalog path), `type` (directory, schema, graph type or graph), `default` (true for the default schema and default graph) and `home` (true for the session's home schema and home graph).
  - Acceptance: Given a database with one graph, one index, one constraint and one active transaction, when each built-in procedure is called, then its result contains that graph, index, constraint and transaction respectively, with the columns listed above.

## Graph algorithms

- FR-ALG-001 The crate SHALL provide a graph-algorithm library for analytics. Its content is the inventory in [graph-algorithms.md](graph-algorithms.md) ([FR-ALG-005](#graph-algorithms)).
  - Acceptance: Given a release, when its documentation is inspected, then it lists which inventory entries (`ALG-`) it provides.
- FR-ALG-002 The crate SHALL make each provided algorithm invocable both as a procedure with `CALL` and through the Rust API.
  - Acceptance: Given a provided algorithm, when it runs once through `CALL` and once through the Rust API on the same snapshot with the same parameters, then both results are equal.
- FR-ALG-003 An algorithm run SHALL either stream its results to the caller or write them back to properties, as the caller chooses. A write-back SHALL take place in the caller's transaction.
  - Acceptance: Given PageRank run in write-back mode inside a transaction, when the transaction rolls back, then no score property exists; and when the same run is in stream mode, then the scores are returned as rows and no property changes.
- FR-ALG-004 An algorithm SHALL read its input from the whole graph, optionally filtered by node labels and edge types, as seen by the snapshot of the caller's transaction.
  - Acceptance: Given a graph with labels A and B, when an algorithm runs filtered to label A, then its result covers only A nodes; and when another transaction commits a new A node during the run, then the result does not include that node.
- FR-ALG-005 The crate SHALL provide every algorithm in the closed inventory in [graph-algorithms.md](graph-algorithms.md), and the whole inventory SHALL ship in the first release ([NFR-REL-004](non-functional-requirements.md#release)).
  - Acceptance: Given the first release, when its list of provided algorithms is compared with the inventory, then every kept `ALG-` entry is provided.

## Types

- FR-TYPE-001 The crate SHALL support the GQL temporal types (feature GV39) with IANA time zones. The feature identifier is unverified (OQ-005).
  - Acceptance: Given a zoned date-time value in "Europe/Lisbon", when it is stored and read back, then the instant and the zone name are unchanged.
- FR-TYPE-002 The crate SHALL support the spatial types Point2D and Point3D as a declared extension, in these coordinate reference systems: Cartesian 2D (SRID 7203), Cartesian 3D (SRID 9157), WGS-84 2D (SRID 4326) and WGS-84 3D (SRID 4979) (Neo4j default [S20]; Memgraph supports the same four [S19]). Their standing against ISO/IEC 39075:2024 is unverified (OQ-006).
  - Acceptance: Given one point in each of the four coordinate reference systems, when each is stored and read back, then its coordinates and SRID are unchanged.
- FR-TYPE-003 The crate SHALL keep each element identifier stable for the lifetime of its element.
  - Acceptance: Given a node, when its properties and labels change across many transactions and the database is reopened, then its element identifier is unchanged.
- FR-TYPE-004 The crate SHALL compute the distance between two points of the same coordinate reference system: in metres for WGS-84 points, and in coordinate units by the Euclidean formula for Cartesian points. IF the two points have different coordinate reference systems, THEN the result SHALL be null (Neo4j default [S20]).
  - Acceptance: Given Cartesian points (0, 0) and (3, 4), when their distance is computed, then it is 5; and given a Cartesian point and a WGS-84 point, then the result is null.
- FR-TYPE-005 The crate SHALL provide a point index that serves distance queries on point properties ([FR-IDX-001](#indexes-and-constraints)) (Neo4j default: point index [S20]).
  - Acceptance: Given a point index on property p, when a query selects nodes within a distance of a point, then its result equals the result of the same query without the index.
- FR-TYPE-006 The crate SHALL read the current time from the operating-system clock. There SHALL be no clock abstraction (Neo4j default: the realtime clock is "the live clock of the system" [S18]).
  - Acceptance: Given the operating-system clock, when a request obtains the current timestamp, then the value is within one second of the operating-system time.
- FR-TYPE-007 The crate SHALL read IANA time-zone data from the operating system (Memgraph default: Memgraph's time-zone support depends on the host's `tzdata` version [S19]; no Neo4j source consulted states where Neo4j's time-zone data comes from).
  - Acceptance: Given a host whose `tzdata` defines zone Z, when a request uses zone Z, then the offset matches the host's `tzdata`.
- BR-TYPE-001 The crate SHALL NOT reuse an element identifier. Neo4j reuses identifiers of deleted elements [S11]; the user's decision overrides that default.
  - Acceptance: Given a node that is deleted, when many new elements are created, then none of them receives the deleted node's element identifier.
- BR-TYPE-002 Backup and restore, and format migration, SHALL preserve element identifiers. Import SHALL assign new element identifiers (Neo4j default for import: the `:ID` values of the input only link edges to nodes [S12]). Neo4j changes internal identifiers when the store format changes [S9]; the user's decision ([FR-TYPE-003](#types)) overrides that default for migration.
  - Acceptance: Given a node with element identifier X, when the database is backed up and restored, or migrated, then the node has identifier X; and when the node is exported and imported, then the imported node has a new identifier.

## Storage

- FR-STO-001 The crate SHALL store databases persistently on disk and SHALL cache data in memory. A graph MAY exceed the available memory.
  - Acceptance: Given a memory cap smaller than the graph, when a query reads the whole graph, then it returns the correct result.
- FR-STO-002 The crate SHALL perform storage I/O through the storage abstraction ([ABS-003](application-abstractions.md#abstractions)).
  - Acceptance: Given a storage abstraction that records every call, when the database is created, written and read, then every file operation passes through the abstraction.
- FR-STO-003 The crate SHALL support many threads of one process reading and writing one database concurrently, including multiple concurrent writers.
  - Acceptance: Given 16 threads, each writing disjoint data in its own transaction, when all commit, then every write is present.
- FR-STO-004 The crate SHALL NOT support access to one database from more than one process. A second process's open is refused ([FR-DB-006](#database-lifecycle)).
  - Acceptance: As for FR-DB-006.

## Transactions

- FR-TX-001 The crate SHALL provide ACID transactions.
  - Acceptance: Given the transaction tests of [NFR-VER-004](non-functional-requirements.md#verification), when they run, then atomicity, consistency, isolation and durability hold in every case.
- FR-TX-002 WHEN a transaction commits, the crate SHALL return from the commit only after the transaction's log records are on stable storage (fsync). The crate MAY commit several transactions with one flush (group commit).
  - Acceptance: Given a storage abstraction that records flushes, when a commit returns, then a flush covering the transaction's log records has completed.
- FR-TX-003 The crate SHALL run transactions at the serializable isolation level by default.
  - Acceptance: Given two concurrent transactions without an isolation choice, when their combined effect would not match any serial order, then one of them fails ([FR-TX-008](#transactions)).
- FR-TX-004 The crate SHALL offer snapshot isolation as an additional, implementation-defined isolation mode, selected per transaction through the Rust API only. Over Bolt, every transaction SHALL be serializable.
  - Acceptance: Given a Rust API transaction started with snapshot isolation, when it runs a write-skew pattern with a concurrent transaction, then both may commit; and given a Bolt transaction, then it runs serializable.
- FR-TX-005 IF a transaction needs a write lock held by another transaction, THEN the crate SHALL make it wait for that lock. Deadlocks are handled by [FR-TX-007](#transactions).
  - Acceptance: Given transaction T1 holding a write lock, when T2 requests it, then T2 waits and continues after T1 ends.
- FR-TX-006 A read SHALL NOT wait for a write, and a write SHALL NOT wait for a read. Serializable isolation is kept by failing transactions ([FR-TX-008](#transactions)), not by read locks.
  - Acceptance: Given a transaction holding uncommitted writes on a node, when another transaction reads that node, then the read returns immediately with the last committed state.
- FR-TX-007 The crate SHALL detect deadlocks. WHEN a lock request closes a wait cycle, the crate SHALL abort the requesting transaction with a GQLSTATUS error (comparable Neo4j behaviour: `Neo.TransientError.Transaction.DeadlockDetected` [S13]).
  - Acceptance: Given T1 holding lock A and waiting for B, when T2, holding B, requests A, then T2 is aborted with a GQLSTATUS error and T1 continues.
- FR-TX-008 A serializable transaction MAY fail with a serialisation-failure GQLSTATUS error. The GQLSTATUS code is unverified until checked against ISO/IEC 39075 (OQ-005).
  - Acceptance: Given two concurrent serializable transactions in a write-skew pattern, when both try to commit, then at most one commits and the other fails with a serialisation-failure GQLSTATUS error.
- BR-TX-001 The crate SHALL NOT retry a failed transaction automatically, on any interface. The application decides whether to retry. The Neo4j drivers retry managed transactions automatically [S24]; the user's decision overrides that default.
  - Acceptance: Given a transaction that fails with a serialisation failure or a deadlock, when the failure is reported, then the crate has run the transaction exactly once.

## Resource limits

- FR-LIM-001 IF a statement exceeds any resource limit, THEN the crate SHALL fail the statement with a GQLSTATUS error and SHALL abort its transaction.
  - Acceptance: Given a transaction with a memory limit, when a statement exceeds it, then the statement fails with a GQLSTATUS error and the transaction's earlier writes are not visible to any later transaction.
- FR-LIM-002 The crate SHALL support two result-size limits: a maximum number of rows and a maximum number of bytes. Each SHALL be optional and configurable.
  - Acceptance: Given a row limit of 10, when a query produces 11 rows, then the statement fails with a GQLSTATUS error; and given no limits, then the query returns all its rows.
- FR-LIM-003 The crate SHALL enforce a database-wide memory cap that covers the cache and query execution together. Unless the application sets it, the cap SHALL be 100% of physical memory when swap is enabled and 90% of physical memory otherwise (Memgraph default: `--memory-limit=0` [S33]; Neo4j has no single equivalent, as it limits transaction memory, `dbms.memory.transaction.total.max`, separately from the page cache [S14]).
  - Acceptance: Given a memory cap, when concurrent queries and caching run under load, then the memory the crate accounts for never exceeds the cap, and a statement that would exceed it fails under FR-LIM-001.
- FR-LIM-004 The application SHALL set the default limits of Bolt sessions.
  - Acceptance: Given application-set Bolt default limits, when a Bolt session runs a statement that exceeds them, then the statement fails under FR-LIM-001.

## Recovery

- FR-REC-001 WHEN the crate reopens a database after a process crash, the crate SHALL recover every committed transaction and no effect of an uncommitted one.
  - Acceptance: Given a process killed with `kill -9` during a write workload, when the database is reopened, then every transaction whose commit returned is present and no other is ([NFR-VER-004](non-functional-requirements.md#verification)).
- FR-REC-002 WHEN the crate reopens a database after a power loss or operating-system crash, including torn page writes, the crate SHALL recover every committed transaction and no effect of an uncommitted one.
  - Acceptance: Given a simulated power loss that tears page writes, when the database is reopened, then every transaction whose commit returned is present and no other is.
- FR-REC-003 The crate SHALL detect corruption with checksums. IF corruption is detected when opening, THEN the crate SHALL report it and refuse to open the database. Corruption detected while the database is open is handled by [FR-REC-005](#recovery).
  - Acceptance: Given a database file with one flipped bit in a checked region, when the database is opened, then the open fails with an error that reports corruption.
- FR-REC-004 The crate SHALL repair media failure by restoring a backup ([FR-OPS-002](#operations)).
  - Acceptance: Given a backup and a destroyed database, when the backup is restored, then the restored database opens and contains the backed-up data.
- FR-REC-005 IF a checksum fails while the database is open, THEN the crate SHALL fail the operation that detected it and SHALL make the database unusable until it is reopened.
  - Acceptance: Given an open database whose page has been corrupted on disk, when a read reaches that page, then the read fails with a GQLSTATUS error and every later operation on the handle fails until the database is closed and reopened.
- FR-REC-006 IF a flush to stable storage fails, THEN the crate SHALL fail the commit and SHALL make the database unusable until it is reopened. Reopening runs recovery. This matches PostgreSQL's default `data_sync_retry = off`, under which a flush failure crashes the server and recovery replays the log, because after a write-back failure the state of the operating-system page cache is unknown and a retry can report success for lost data [S22].
  - Acceptance: Given a storage abstraction that fails the next flush, when a transaction commits, then the commit fails with a GQLSTATUS error and every later operation on the handle fails until the database is reopened.
- FR-REC-007 IF the storage is full, THEN the crate SHALL fail writes with a GQLSTATUS error and SHALL continue to serve reads.
  - Acceptance: Given a storage abstraction that reports no free space, when a transaction writes, then it fails with a GQLSTATUS error; and when another transaction reads, then it succeeds.
- FR-REC-008 WHEN the crate opens a database, it SHALL always verify the log and the metadata. This is the only verification performed at open.
  - Acceptance: Given a database with a corrupted log record, when it is opened, then the open fails with an error that reports corruption.
- FR-REC-009 Withdrawn 2026-09-24 (NFR-SIMP-001, user decision 2026-09-24). It specified an open option that verifies the whole database. Integrity is covered by the check at every open ([FR-REC-008](#recovery)) and the on-demand integrity check ([FR-REC-010](#recovery)).
  - Acceptance: Not applicable (withdrawn).
- FR-REC-010 The crate SHALL provide an integrity check of an open database, on demand, through the Rust API.
  - Acceptance: Given an open database with a corrupted data page, when the application runs the integrity check, then the check reports the corruption.

## Bolt

- FR-BOLT-001 The crate SHALL provide a server-side Bolt endpoint: PackStream encoding, messages and the protocol state machine, over connections supplied through the transport abstraction ([ABS-001](application-abstractions.md#abstractions)).
  - Acceptance: Given a connection supplied through ABS-001, when an official Neo4j driver connects, authenticates and runs a query, then it receives the result ([NFR-VER-003](non-functional-requirements.md#verification)).
- FR-BOLT-002 The crate SHALL provide a client-side Bolt endpoint over connections supplied through the transport abstraction.
  - Acceptance: Given a connection supplied through ABS-001 to a Bolt server, when the client endpoint authenticates and runs a query, then it receives the result.
- FR-BOLT-003 The crate SHALL support Bolt versions 5.0 to the latest 6.x, and SHALL NOT support versions 1.x to 4.4. "The latest 6.x" means the latest 6.x version that the Neo4j Bolt documentation [S1] lists when a crate version is released. On 2026-09-24 that version is 6.0 [S1].
  - Acceptance: Given a client that offers only Bolt 4.4, when it performs the handshake, then the negotiation fails; and given clients that offer 5.0 and the latest listed 6.x, then each negotiation succeeds.
- FR-BOLT-004 The `RUN` message SHALL carry GQL text only. The GQL-over-Bolt mapping is [FR-BOLT-007](#bolt), [FR-BOLT-009](#bolt), [FR-BOLT-014](#bolt) to [FR-BOLT-018](#bolt) and [FR-BOLT-021](#bolt). The user reports that no official mapping exists; this is unverified.
  - Acceptance: Given a `RUN` message with GQL text, when it is processed, then the crate executes it as GQL.
- FR-BOLT-005 IF a GQL value cannot be represented in PackStream, THEN on Bolt 6.0 and later the crate SHALL send the `UnsupportedType` structure, introduced in Bolt 6.0 [S3], and on earlier versions the crate SHALL return an explicit error.
  - Acceptance: Given a query returning a value with no PackStream representation, when it runs over Bolt 6.0, then the record contains `UnsupportedType`; and over Bolt 5.8, then the query fails with an error.
- BR-BOLT-001 The crate SHALL NOT convert a value lossily to make it representable in PackStream.
  - Acceptance: As for FR-BOLT-005: no record carries a converted value in place of the original.
- FR-BOLT-006 The crate SHALL use GQLSTATUS for errors internally and in the Rust API. On Bolt versions earlier than 5.7, the crate SHALL map GQLSTATUS to a code and message; `FAILURE` carries `gql_status` from Bolt 5.7 [S2].
  - Acceptance: Given a failing query, when it runs over Bolt 5.7 or later, then `FAILURE` carries `gql_status`; and over Bolt 5.6, then `FAILURE` carries a code and a message.
- FR-BOLT-007 The crate SHALL resolve the Bolt `db` field as a catalog reference. IF `db` is absent, null or empty, THEN the transaction SHALL use the session's home graph (Neo4j default: absent `db` selects the default database [S2]). IF `db` names a graph, THEN that graph SHALL be the transaction's current graph. IF `db` names a schema, THEN that schema SHALL be the transaction's current schema. IF `db` names neither, THEN the crate SHALL fail the request with a GQLSTATUS error (comparable Neo4j behaviour: `Neo.ClientError.Database.DatabaseNotFound` [S13]).
  - Acceptance: Given graph "/s/g1", when `BEGIN` carries `db` "/s/g1", then queries in the transaction read "g1"; and when `db` names no catalog object, then the request fails with a GQLSTATUS error.
- FR-BOLT-008 The crate SHALL support bookmarks, as [FR-BOLT-017](#bolt) details.
  - Acceptance: As for FR-BOLT-017.
- FR-BOLT-009 WHEN the crate receives `ROUTE`, it SHALL return a routing table that names a single instance in the roles `ROUTE`, `READ` and `WRITE`. The address SHALL be the advertised address that the application supplies (Neo4j default: `server.bolt.advertised_address` [S14]). The table's time to live SHALL be 300 seconds (Neo4j default: `dbms.routing_ttl` is 5 minutes [S14]).
  - Acceptance: Given the advertised address "db.example:7687", when a driver sends `ROUTE`, then the response lists that address once in each of the three roles, with `ttl` 300.
- FR-BOLT-010 WHEN the crate receives `TELEMETRY` (added in Bolt 5.4 [S2]), it SHALL accept and ignore it.
  - Acceptance: Given a Bolt 5.4 connection, when a `TELEMETRY` message arrives, then the crate replies `SUCCESS` and no state changes.
- FR-BOLT-011 WHEN the crate receives `HELLO` or `LOGON` (added in Bolt 5.1 [S2]), it SHALL parse the authentication scheme and credentials and pass them to the authentication and authorisation abstraction ([ABS-002](application-abstractions.md#abstractions)).
  - Acceptance: Given a recording abstraction, when a driver authenticates with scheme "basic", then the abstraction receives scheme "basic" and the credentials.
- FR-BOLT-012 The crate SHALL delegate `imp_user` requests to the authentication and authorisation abstraction.
  - Acceptance: Given a `BEGIN` with `imp_user` "u2", when it is processed, then the abstraction receives the impersonation request for "u2" and its answer decides the transaction's principal.
- FR-BOLT-013 WHEN the authentication and authorisation abstraction returns a principal, the crate SHALL bind it to the GQL session, which SHALL use it for ownership and authorisation. WHEN it returns a refusal, the crate SHALL refuse the authentication.
  - Acceptance: Given an abstraction that refuses, when a driver authenticates, then it receives `FAILURE` and cannot run queries.
- FR-BOLT-014 Each Bolt connection SHALL carry one GQL session. The crate SHALL create the session when authentication succeeds (`HELLO` in Bolt 5.0, `LOGON` from Bolt 5.1) and SHALL end it on `LOGOFF`, `GOODBYE` or connection close.
  - Acceptance: Given an authenticated connection, when two auto-commit queries run on it, then both run in the same GQL session; and after `LOGOFF`, then that session no longer exists.
- FR-BOLT-015 WHEN the crate receives `RESET`, it SHALL stop the running statement, abort the open transaction, and return the session to the state it had just after authentication, as if `HELLO` (and `LOGON`) had just completed [S2]. The principal SHALL be kept.
  - Acceptance: Given a connection with an open transaction and a running statement, when `RESET` arrives, then the statement stops, the transaction's writes are discarded and the connection is ready with the same principal.
- FR-BOLT-016 Over Bolt, transactions SHALL be controlled with the `BEGIN`, `COMMIT` and `ROLLBACK` messages; a `RUN` outside an explicit transaction SHALL run in an auto-commit transaction. IF a `RUN` carries a GQL session statement or a GQL transaction statement, THEN the crate SHALL refuse it with a GQLSTATUS error (Neo4j default: session and transaction commands are provided through the driver APIs, not in query text [S23]).
  - Acceptance: Given a connection, when `RUN` carries `START TRANSACTION`, then the crate replies with a GQLSTATUS error; and when `BEGIN`, `RUN` and `COMMIT` are sent, then the writes are committed.
- FR-BOLT-017 WHEN a transaction commits over Bolt, the crate SHALL return a bookmark. WHEN `BEGIN` or an auto-commit `RUN` carries bookmarks that this database issued, the crate SHALL start the transaction without waiting, because on a single instance every committed transaction is already visible. IF a bookmark was not issued by this database, THEN the crate SHALL fail the request with a GQLSTATUS error (comparable Neo4j behaviour: `Neo.ClientError.Transaction.InvalidBookmark` [S13]).
  - Acceptance: Given a bookmark returned by a commit, when a later `BEGIN` carries it, then the transaction starts at once and sees the committed writes; and given an arbitrary string as bookmark, then the request fails with a GQLSTATUS error.
- FR-BOLT-018 The crate SHALL attach the `tx_metadata` of `BEGIN` or of an auto-commit `RUN` to the transaction, SHALL show it in the list of active transactions ([FR-OBS-003](#observability)), and SHALL include it in the transaction's `tracing` events (Neo4j default: metadata is shown by `SHOW TRANSACTIONS` [S16] and is "mainly used for logging" [S2]).
  - Acceptance: Given `BEGIN` with `tx_metadata` {app: "x"}, when the active-transaction procedure is called, then the transaction's row shows {app: "x"}.
- FR-BOLT-019 The client-side endpoint SHALL offer: connection over ABS-001 and authentication; auto-commit queries; explicit transactions with begin, commit and rollback; incremental result streaming; and bookmark passing. It SHALL NOT offer managed transactions with automatic retry ([BR-TX-001](#transactions)).
  - Acceptance: Given the client endpoint and a Bolt server, when an application runs an auto-commit query, an explicit transaction and a transaction chained with a bookmark, then each works; and when a transaction fails transiently, then the client reports the failure without retrying.
- FR-BOLT-020 The client-side endpoint SHALL work with the crate's server-side endpoint and with Neo4j servers that support Bolt 5.0 or later (Neo4j 5.0 onwards [S1]). Queries sent to Neo4j are still GQL text ([FR-BOLT-004](#bolt)); whether Neo4j accepts a given query is Neo4j's behaviour. Memgraph is not a target of the client-side endpoint (user decision 2026-09-24, [NFR-SIMP-001](non-functional-requirements.md#simplicity)).
  - Acceptance: Given the crate's server endpoint and a Neo4j server of a supported version, when the client endpoint runs the test suite of [NFR-VER-005](non-functional-requirements.md#verification) against each, then every test passes.
- FR-BOLT-021 The Bolt `tx_timeout` field SHALL set the transaction's time limit; WHEN it expires, the crate SHALL cancel the running statement under [FR-LIM-001](#resource-limits).
  - Acceptance: Given `BEGIN` with `tx_timeout` 1000, when the transaction runs longer than 1 second, then the statement fails with a GQLSTATUS error and the transaction is aborted.

## Operations

- FR-OPS-001 The crate SHALL take an online, consistent backup of a database.
  - Acceptance: Given a database under a concurrent write workload, when a backup is taken and restored, then the restored database equals the state of one commit point.
- FR-OPS-002 The crate SHALL restore a database from a backup.
  - Acceptance: As for FR-REC-004.
- FR-OPS-003 The crate SHALL import and export data in bulk, as [Import and export](#import-and-export) details.
  - Acceptance: As for FR-IMP-001 to FR-IMP-005.
- FR-OPS-004 The crate SHALL version its on-disk format and SHALL migrate an older format only through an explicit migration.
  - Acceptance: Given a database in an older format, when it is opened, then the open fails with an error that states that migration is required.
- FR-OPS-005 The crate SHALL emit events through the `tracing` crate.
  - Acceptance: Given a `tracing` subscriber, when a query runs, then the subscriber receives events for it.
- FR-OPS-006 The crate SHALL expose metrics through the metrics sink abstraction ([ABS-004](application-abstractions.md#abstractions)).
  - Acceptance: Given a recording metrics sink, when transactions commit, then the sink receives transaction metrics.
- FR-OPS-007 A backup SHALL be a full backup of the database. Neo4j also offers differential backups and point-in-time restore [S6] [S7]; they are excluded (user decision 2026-09-24, [NFR-SIMP-001](non-functional-requirements.md#simplicity)).
  - Acceptance: Given a backup, when it is restored alone, then the result is a complete database.
- FR-OPS-008 A backup SHALL cover the whole database: the catalog, every graph, and every index and constraint definition. It SHALL NOT cover registered procedures and functions ([FR-PROC-003](#procedures-and-functions)) or data held by the application's abstractions.
  - Acceptance: Given a database with two graphs, an index and a constraint, when it is backed up and restored, then both graphs, the index and the constraint exist.
- FR-OPS-009 A restore SHALL create a database at a location that holds no database. IF the location holds a database, THEN the restore SHALL fail with an error. Neo4j can also overwrite a stopped database [S7]; restore over an existing database is excluded (user decision 2026-09-24, [NFR-SIMP-001](non-functional-requirements.md#simplicity)).
  - Acceptance: Given a location holding a database, when a restore targets it, then the restore fails and the existing database is unchanged.
- FR-OPS-010 Backup and restore SHALL be available through the Rust API only, not through `CALL` (Neo4j default: backup and restore are administration commands, not Cypher [S6]).
  - Acceptance: Given the list of built-in procedures, when it is inspected, then it contains no backup or restore procedure.
- FR-OPS-011 Format migration SHALL run on a closed database and SHALL rewrite it in place. Neo4j runs migration on a stopped database and rewrites all data in the new format [S8] [S9]; whether Neo4j writes in place or to a copy is not stated in those sources.
  - Acceptance: Given a closed database in the previous format version, when the application runs migration, then the database at the same location opens in the current format; and given an open database, then migration fails with an error.
- FR-OPS-012 Each crate version SHALL migrate from the format version that immediately precedes its own. An older format SHALL be migrated step by step through intermediate crate versions (comparable Neo4j behaviour: upgrades pass through every LTS version between the current and the target version [S10]).
  - Acceptance: Given a database two format versions old, when the current crate version tries to migrate it, then migration fails with an error that names the intermediate version required.
- FR-OPS-013 The crate SHALL NOT migrate a database to an earlier format version. Rollback of a migration SHALL be done by restoring a backup taken before it (Neo4j default: downgrades are not supported [S10]). IF a database's format is newer than the crate supports, THEN the open SHALL fail with an error.
  - Acceptance: Given a database written by a newer crate version, when an older crate version opens it, then the open fails with an error and the files are unchanged.

## Import and export

- FR-IMP-001 The crate SHALL import and export these formats: CSV (separate node files and edge files), JSON Lines, GraphML, and a native dump format.
  - Acceptance: Given a graph, when it is exported and imported in each of the four formats, then each imported graph has the same nodes, edges, labels and property values as the original.
- FR-IMP-002 The crate SHALL offer two import modes: a transactional import into an existing graph, and a fast initial load into an empty graph. IF the fast initial load targets a graph that is not empty, THEN it SHALL fail with an error (Neo4j default: full import requires a non-existent or empty database [S12]).
  - Acceptance: Given a graph with one node, when a fast initial load targets it, then the load fails; and when a transactional import targets it, then the imported data is added.
- FR-IMP-003 The crate SHALL export a whole database, one graph, or the result of a query. The native dump SHALL cover the database and graph scopes; CSV, JSON Lines and GraphML SHALL cover the graph and query-result scopes.
  - Acceptance: Given a database with two graphs, when the database is exported to the native dump, a graph to each of the four formats, and a query result to CSV, JSON Lines and GraphML, then each export contains exactly the data of its scope; and when a database export to CSV or a query-result export to the native dump is requested, then the request fails with an error.
- FR-IMP-004 IF an import meets an invalid record, THEN the crate SHALL either abort the import or skip the record, as an import option chooses, and SHALL report every invalid record in both cases. The option SHALL default to abort (Neo4j default: `--skip-bad-relationships` and `--skip-duplicate-nodes` are false [S12]).
  - Acceptance: Given an input with one invalid record, when it is imported with the default option, then the import aborts, no data is added and the report names the record; and when it is imported with the skip option, then the valid records are imported and the report names the skipped record.
- FR-IMP-005 Import SHALL assign new element identifiers ([BR-TYPE-002](#types)). Identifiers in the input SHALL serve only to connect edges to nodes within the same import.
  - Acceptance: Given a CSV node file with identifier "n1" and an edge file that references "n1", when they are imported, then the edge connects to the imported node, whose element identifier is newly assigned.

## Observability

- FR-OBS-001 The crate SHALL provide metrics in these categories: transactions, transaction log, checkpointing, cache, query execution, Bolt, store size, data counts and indexes. These are the Neo4j metric categories that apply to a single embedded instance; clustering, routing, pool and Java Virtual Machine metrics are excluded [S15].
  - Acceptance: Given a recording metrics sink, when a workload covering each category runs, then the sink receives at least one metric of each category.
- FR-OBS-002 The `tracing` events of each executed request SHALL include the query text and SHALL NOT include parameter values. This deviates from the Neo4j default, which logs parameters (`db.logs.query.parameter_logging_enabled` is true [S14]), because parameter values can carry personal or confidential data (user decision 2026-09-24).
  - Acceptance: Given a `tracing` subscriber, when a query runs with parameter value "secret-42", then an event carries the query text and no event contains "secret-42".
- FR-OBS-003 The crate SHALL list the active transactions, each with its principal, status, elapsed time, current statement and `tx_metadata`. A principal SHALL always see its own transactions; seeing other principals' transactions SHALL require the authorisation abstraction's consent (Neo4j default: users always see their own transactions; others need a privilege [S16]).
  - Acceptance: Given principals "p1" and "p2" with one transaction each and an abstraction that refuses "p1" access to others' transactions, when "p1" lists active transactions, then only its own transaction is listed.
- FR-OBS-004 The crate SHALL let the application terminate an active transaction through the Rust API. A principal MAY always terminate its own transactions; terminating another principal's transaction SHALL require the authorisation abstraction's consent (Neo4j default [S16]). A terminated transaction SHALL be aborted and its running statement SHALL fail with a GQLSTATUS error.
  - Acceptance: Given a running transaction, when the application terminates it, then its statement fails with a GQLSTATUS error and its writes are discarded.
- FR-OBS-005 The Rust API SHALL return the execution plan of a request without executing it (Neo4j default: `EXPLAIN` plans the query without running it [S17]). No GQL syntax is added ([BR-GQL-001](#gql)).
  - Acceptance: Given a request that inserts a node, when its plan is requested, then a plan is returned and no node is inserted.
- FR-OBS-006 The Rust API SHALL execute a request and return its execution plan with measured statistics per operator, including rows produced (Neo4j default: `PROFILE` runs the query and reports real measurements [S17]).
  - Acceptance: Given a query returning 10 rows, when it is profiled, then the result contains the 10 rows and a plan whose final operator reports 10 rows.
