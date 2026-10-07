---
title: Catalog and indexes
status: draft
last-reviewed: 2026-10-07
related:
  - storage.md
  - transactions-and-recovery.md
  - query-engine.md
---

# Catalog and indexes

## TS-17 Catalog and statistics

- **Decision**:
  - **Catalog tree.** One B+tree per database, with immutable stamped entries ([TS-14](transactions-and-recovery.md#ts-14-mvcc)). Catalog writes join the transaction's write set, so DDL and data changes commit and roll back together.
    - `(parent_id, name) → (object_id, kind, owner, definition)` for directories, schemas, graph types and graphs. `object_id` is monotonic and never reused. Its counter, and the token counter, are persisted like the element-identifier counter ([TS-07](storage.md#ts-07-element-identifiers)).
    - Reverse entries `object_id → (parent_id, name)` for paths and listing.
    - Token tables `(kind, name) ↔ token_id` for labels, edge types and property keys.
    - Index and constraint definitions keyed by `(graph_id, object_id)`.
  - **Tokens.** Token entries carry no stamps and are never deleted ([TS-06](storage.md#ts-06-graph-layout)). Lookups use an in-memory map of the committed tokens, loaded at open.
    - A missing token is created at once, in an inner transaction: under a per-database token latch, the creator looks the name up again, draws a token identifier ([TS-07](storage.md#ts-07-element-identifiers)), commits the entry as a commit of its own ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol)), adds it to the map and releases the latch.
    - The token stays whether the transaction that needed it commits or aborts. Two transactions that need the same name therefore get the same token, under snapshot isolation too.
  - **Catalog names.** Creating or dropping a catalog object takes the X lock on its `(parent_id, name)` ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)). Of two transactions that create the same name, the second waits for the first; if the first commits, the write-write rule fails the second with 25N11, under snapshot isolation too.
  - **Creation.** A new database's catalog contains a default schema and a default graph. `create` takes their owner principal as a required argument and records it as the owner of both (user decision 2026-10-07, rmp task #25; [interfaces.md](interfaces.md#public-api-shape)). Directories are created only through the Rust API.
    - **Durability.** `create` writes the catalog tree, with the two entries frozen (no stamps, [TS-06](storage.md#ts-06-graph-layout)), directly into `data-1` before its `sync_data`, and only then writes `CONTROL` ([TS-05](storage.md#ts-05-files-and-pages) Create). No WAL is involved: a crash before `CONTROL` exists leaves no database, and after it the objects are on disk. Every open therefore finds them.
    - **No authorisation.** `create` makes no authorisation request ([TS-23](interfaces.md#ts-23-authentication-and-authorisation)): no session exists, and the owner it receives is the first principal the catalog knows.
  - **Dropping a graph.** `DROP GRAPH` stamps the entry deleted; garbage collection frees the graph's trees after the horizon passes.
  - **Closed graph types.** Graph types are immutable once created. Each `INSERT`, `SET` and `REMOVE` on a graph of a closed type is checked at the end of its statement against that type, over the elements the statement wrote. Open graph types cost nothing. The exact matching rules are in the General Rules, outside the ISO inserts: provisional (OQ-005).
  - **Statistics.** Node counts per label; edge counts per `(label, edge type, direction)` with the label on one side; entry and distinct-key counts per index.
    - Kept in memory and updated by the committer when it applies a commit ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol) step 6).
    - **Label changes.** When a commit adds or removes a label of node `n`, the committer changes that label's edge counts by every adjacency entry of `n` that the commit neither creates nor deletes, read from `n`'s adjacency prefix ([TS-06](storage.md#ts-06-graph-layout) Expansion). Edges that the commit creates or deletes are counted with `n`'s labels after or before the commit. This scan of the node's edges runs inside the serial section, because writers of edges at `n` hold only the S existence lock, which a label change does not block ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)).
    - Saved by each checkpoint to the statistics tree, with the last `commit_ts` they include ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)), and loaded at open.
    - **After a crash.** If recovery applied a commit above that `commit_ts`, open recounts every statistic exactly, by one scan of each node tree, adjacency tree and index, before it returns ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 7). No transaction runs during the recount, so it needs no snapshot.
- **Satisfies**: FR-CAT-001, FR-CAT-002, FR-CAT-003, FR-CAT-004, FR-CAT-005, FR-DB-001, FR-DB-005, FR-AUTH-002, FR-GQL-004, FR-PROC-004.
- **Evidence**:
  - Token maps: Neo4j (R-NEO `community/token-api/.../TokenHolder.java`) and Memgraph (R-MG `src/storage/v2/name_id_mapper.hpp`).
  - Token creation. Neo4j creates a missing token in a `synchronized` method that looks the name up again first (R-NEO `community/token-api/.../CreatingTokenHolder.java:62-71`), through an "inner transaction" (`TokenCreator.java:34`).
  - Catalog names. PostgreSQL keeps catalog names unique with unique indexes, for example on `(relname, relnamespace)` (R-PG @ `1f22453` `src/include/catalog/pg_class.h:163`; `pg_namespace.h:60`). An insert that meets an uncommitted duplicate waits for its transaction to end (`src/backend/access/nbtree/nbtinsert.c:584-599, 220-235`), then fails if it committed (`nbtinsert.c:669`).
  - Bootstrap catalog. PostgreSQL `initdb` runs the server in bootstrap mode (`--boot`, R-PG @ `1f22453` `src/bin/initdb/initdb.c:1550, 1614`), which writes the initial catalog directly. Neo4j instead creates its initial database entries in a normal transaction at each start, when they are missing (R-NEO `community/kernel/.../dbms/database/SystemGraphInitializer.java:25-28`; `AbstractSystemGraphComponent.java:81-92` initialises only when it detects `UNINITIALIZED`; `community/neo4j/.../dbms/database/DefaultSystemGraphComponent.java:114-120`).
  - No authentication at bootstrap. In bootstrap and standalone mode PostgreSQL sets a fixed superuser identity and performs no client authentication; only the multi-user case calls `PerformAuthentication` (R-PG @ `1f22453` `src/backend/utils/init/postinit.c:907-922, 944-949`). `initdb` runs only those modes (`initdb.c:227, 1614, 3122`).
  - Owner at creation. PostgreSQL `initdb` takes the bootstrap superuser's name (`-U`, R-PG @ `1f22453` `src/bin/initdb/initdb.c:3302-3303`) and writes it into the bootstrap catalog (`initdb.c:1588-1589`), where that role owns the initial objects (`src/include/catalog/pg_authid.dat:18-25`). `initdb` defaults the name to the operating-system user (`initdb.c:3483-3485`); `create` has no such source, so the argument is required.
  - Count statistics: Neo4j keeps per-label counts and uses histograms only in tests; Kuzu uses default selectivities (M-QE §Q3.1).
  - Label changes. Neo4j updates the relationship counts of an added or removed label from the node's degrees per type and direction before the transaction, and the transaction's own relationship changes compensate (R-NEO `community/storage-engine-util/.../TransactionCountingStateVisitor.java:126-143`).
  - Recount. Neo4j's counts store is a tree written at checkpoint; when it cannot be trusted, start-up rebuilds it by a full count before transactions run (R-NEO `community/storage-engine-util/.../GBPTreeCountsStore.java:46-50`; `GBPTreeGenericCountsStore.java:195-201, 252-267`).
  - The ISO grammar has no statement that creates a directory ([S45], verified under OQ-005); the opengql grammar agrees (R-GQL `GQL.g4:3496-3537`, from M-QE §Q1.2).
- **Rejected**:
  - A catalog file separate from the transactional trees: FR-CAT-004 requires DDL and data changes to be atomic together.
  - Durable counters updated by every commit: one page image per commit on a hot page ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
  - Stamped token entries created in the user's transaction: two transactions could commit the same name with two identifiers.
  - `create` recording no owner, passed to authorisation as absent; or a reserved principal name such as `system` (user decision 2026-10-07).
  - The first open committing the default schema and default graph (Neo4j): the owner would have to be kept outside the catalog until then, and every open would check for the objects. Writing them at create, before `CONTROL`, is atomic with the creation of the database.
  - A recount in the background after open: it would need a snapshot held for the whole count, whose old versions can make writers fail ([TS-18](#ts-18-indexes-and-constraints) rejects the same for index builds), or a protocol for concurrent changes that no reference shows.
  - Estimates rebuilt by sampling (the earlier rule): a sampler adds code, while one ordered scan per tree gives exact counts (NFR-SIMP-001).

## TS-18 Indexes and constraints

- **Decision**:
  - **Indexes** are B+trees of stamped entries with compressed leaves ([TS-06](storage.md#ts-06-graph-layout)) and order-preserving key encodings ([TS-21](values-and-types.md#ts-21-values-and-types)):

    | Kind | Key |
    |---|---|
    | label or edge-type lookup | `(token, element_id)` |
    | single-property, range | `(index_id, value, element_id)` |
    | composite | `(index_id, value_1, …, value_n, element_id)` |
    | point | `(index_id, Hilbert key, element_id)` per CRS (2D or 3D), with exact distance as a post-filter |

    A reader uses an index only from the snapshot at which the index became ONLINE. The label or edge-type lookup tree exists only when that index has been created (FR-IDX-001).
  - **Hilbert keys** (point index). 60 bits: 30 per axis in 2D, 20 per axis in 3D. The extents are fixed per CRS: for WGS-84 (SRIDs 4326 and 4979), longitude −180 to 180 and latitude −90 to 90; every other axis (Cartesian x, y and z, WGS-84 height) −1 000 000 to 1 000 000. A coordinate outside its extent is clamped to the edge; the exact-distance post-filter keeps results correct. There is no setting (NFR-SIMP-001).
  - **Online build** (giant-graph path):
    1. A transaction commits the definition in state POPULATING, at timestamp `S`. The create call then starts the build on its own thread ([TS-02](architecture.md#ts-02-async-api-and-threading)).
    2. **Committers.** Each committer reads, under the commit latch, the latest committed index definitions from the in-memory catalog cache, which every catalog commit updates when it is applied. Its write set holds entries only for the indexes its snapshot saw ([TS-14](transactions-and-recovery.md#ts-14-mvcc)). Before it registers its write-key set ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol) step 5), it adds to its batch the changes of every index that the cache holds and the write set lacks, derived from the write set's record changes; the write-write rule ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)) makes the versions they replace the latest committed ones. For a POPULATING index the changes (insert, and delete) go to a small **side tree**, an ordinary logged B+tree; for an ONLINE index, to the index. Entries for an index that the cache no longer holds are dropped.
    3. The builder scans the latest committed state of the graph without a snapshot: it reads committed pages under their latches and treats delete-stamped entries as absent. Its tree read operations register for page reuse only ([TS-05](storage.md#ts-05-files-and-pages) Reuse), so it holds no old version.
    4. It sorts the keys with an external merge sort into temporary files through the storage trait, and builds the index bottom-up into new pages, filled to 90 %. The built entries carry no stamps (frozen, [TS-06](storage.md#ts-06-graph-layout)).
    5. It applies the side tree to the new index in commit order, idempotently: an insert of a present key and a delete of an absent key do nothing; an insert adds a frozen entry and a delete removes the entry. The result equals the state after the last applied change, whatever the scan saw of changes after `S`.
    6. **ONLINE commit.** A normal transaction applies, under the commit latch, the side-tree changes committed since step 5 read it, drops the side tree and marks the index ONLINE, in one batch. No commit can fall between the last applied change and the state change. Readers use the index only from that snapshot on, which is above every change the build applied, so frozen entries and removed entries are correct for every reader of the index.
    7. At open, a build thread starts for every POPULATING index and rebuilds it from step 3. The side tree is logged, so it still holds every change since `S`.

    Sort memory is charged to the cap. Writes never wait for a build, and no snapshot is held, so old versions do not accumulate (FR-LIM-005).
  - **Constraints.**
    - Uniqueness and node key: exclusive lock on `(constraint, value)` at write time ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)), checked again when the commit is applied.
    - Property existence and property type: checked over the write set at commit.
    - Creation takes the exclusive schema lock on its label or edge type in its graph. It validates the latest committed data, which is stable because writers of that label or type wait. It builds its owned index by steps 3 and 4 only: no writer of the label or type runs, so there is no side tree, and the creating transaction commits the constraint and its index ONLINE together. It fails without creating the constraint if any element violates it. Writers of other labels and types do not wait.
  - Every index and constraint is defined on one graph.
- **Satisfies**: FR-IDX-001, FR-IDX-002, FR-IDX-003, FR-IDX-004, FR-IDX-005, FR-IDX-006, FR-TYPE-005, FR-API-004, NFR-PERF-005.
- **Evidence**:
  - Neo4j point index: Hilbert-curve keys in its B+tree (R-NEO `community/spatial-index/.../curves/HilbertSpaceFillingCurve2D.java`, `HilbertSpaceFillingCurve3D.java`; `PointIndexReader`). Memgraph uses an in-memory R-tree (R-MG `src/storage/v2/indices/point_index_expensive_header.hpp:18`).
  - Hilbert keys. Neo4j uses 60 bits, "divisible by both 2 and 3", with `NBR_OF_BITS / dimensions` levels per axis (R-NEO `community/kernel/.../index/schema/config/SpaceFillingCurveSettings.java:53-60, 69`); its default extents are longitude ±180, latitude ±90 and ±1 000 000 on every other axis (`EnvelopeSettings.java:30-35, 61-80`); a point outside the extents "will be mapped as if on the edges" (`SpaceFillingCurveSettings.java:43-48`) by clamping (`community/spatial-index/.../curves/SpaceFillingCurve.java:328-334`).
  - Neo4j index population queues concurrent updates and flushes them into the populated index (R-NEO `kernel/.../api/index/MultipleIndexPopulator.java:92-118`). It applies the remaining queued updates inside the flip to ONLINE, under the populator lock (`MultipleIndexPopulator.java:708-712`). At start-up it repopulates every index found in state POPULATING (`kernel/.../api/index/IndexingService.java:328-342`).
  - PostgreSQL builds B-trees bottom-up from sorted input (R-PG `src/backend/access/nbtree/nbtsort.c:4`) and sorts larger-than-memory input by external k-way merge (R-PG `src/backend/utils/sort/tuplesort.c:11-15`).
  - Memgraph index entries carry timestamps and are pruned by garbage collection (R-MG `src/storage/v2/inmemory/label_property_index.hpp:62`).
  - Neo4j uniqueness takes an `INDEX_ENTRY` lock (P-TX §L4.1), and its writers take shared label schema locks (R-NEO `Operations.java:792`).
- **Crates**: `rstar` 0.13.0 (an in-memory R*-tree) and the 2D-only `fast_hilbert` and `hilbert_2d`: study only. The 2D and 3D Hilbert encoding is the crate's own (SRIDs 4979 and 9157 are 3D).
- **Rejected**:
  - Building by inserting entries one at a time into the live tree: random I/O over the whole index at 1–10 % cache (NFR-PERF-005).
  - An R-tree: a second tree type with its own WAL and recovery code.
  - Index entries rechecked against records on every hit (Memgraph): one record read per hit.
  - An in-memory population queue (Neo4j): its size is bounded only by write traffic during a long build.
  - Scanning a pinned snapshot: under FR-LIM-005, old versions held for a long build would make concurrent writers fail instead of proceeding (FR-IDX-005).
  - Marking the index ONLINE after the replay, outside the commit latch: a commit between the end of the replay and the ONLINE commit would reach neither the index nor a replayed side tree.
  - Configurable Hilbert extents per CRS (Neo4j settings): no requirement needs them (NFR-SIMP-001).

The side-tree protocol, including its idempotent replay over an unpinned scan, is a design that has not been tested; it needs the deterministic concurrency tests of [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) (unverified).
