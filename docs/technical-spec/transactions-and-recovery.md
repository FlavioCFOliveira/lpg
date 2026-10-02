---
title: Transactions and recovery
status: draft
last-reviewed: 2026-10-02
related:
  - storage.md
  - catalog-and-indexes.md
  - architecture.md
---

# Transactions and recovery

## TS-09 WAL and group commit

- **Decision**: a redo-only write-ahead log of full page images, under NO-STEAL (uncommitted data never reaches shared pages, [TS-14](#ts-14-mvcc)).
  - **Group.** The unit of durability. A group holds the final image of every page dirtied by its commits, each page once, followed by one commit frame. The commit frame lists the group's commit timestamps and the element-identifier high-water mark ([TS-07](storage.md#ts-07-element-identifiers)).
  - **Frames.** Each frame carries a CRC-32C and the salt of its segment. A group whose commit frame is missing or invalid did not commit.
  - **Capture.** Images are copied under the commit latch ([TS-16](#ts-16-commit-protocol)), so an image never holds a change from a later group.
  - **Writer.** One dedicated thread per database writes each group and calls `sync_data`. Committers wait on a notification, so no tokio worker blocks on a flush.
  - **Segments.** 16 MiB each, created by writing zeros, named by their first LSN.
  - **Errors.**
    - ENOSPC while creating a segment fails the transaction that needed the space; reads continue.
    - Any other failed WAL or data write or `sync_data` sets the unusable latch ([TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check)).
- **Satisfies**: FR-TX-001, FR-TX-002, FR-REC-001, FR-REC-002, FR-REC-006, FR-REC-007, FR-REC-008, FR-OBS-001.
- **Evidence**:
  - PostgreSQL WAL: CRC-32C per record (R-PG `src/include/access/xlogrecord.h:49`); segments zero-filled at creation, `wal_init_zero = true` (`src/backend/access/transam/xlog.c:134`).
  - SQLite WAL: page-image frames, with a commit frame that closes each transaction (R-SQLITE `src/wal.c`, header comment).
  - Neo4j appends to its log, then applies to the store (R-NEO `kernel/.../api/InternalTransactionCommitProcess.java:77-80`). A dedicated `LOG_WRITER` thread drains commits in batches (`kernel/.../wal/TransactionLogQueue.java:111, 220`).
  - InnoDB: dedicated `log_writer` and `log_flusher` threads (R-INNO `log/log0write.cc:108-122`).
  - turso: leader and waiter group commit (R-TURSO `core/mvcc/database/group_commit.rs`).
  - Memgraph writes its WAL under `engine_lock_`, so commits are ordered by timestamp (R-MG `src/storage/v2/inmemory/storage.cpp:1081-1100`).
- **Crates**: `okaywal` 0.3.1 (17 k downloads, last release 2023-11-26) and the fjall journal: study only.
- **Known cost**: one 8 KiB image per page per group, which may be large for small transactions. The NFR-PERF-002 baseline measures it (unverified).
- **Rejected**:
  - Physiological records with full-page writes (PostgreSQL): less WAL volume, but more record types and redo code. It is the fallback if the measured volume misses a target.
  - Undo plus redo (ARIES, InnoDB): undo is needed only when uncommitted data can reach pages.
  - No log, copy-on-write (redb, LMDB): FR-TX-002 and FR-REC-008 name a log; one writer at a time in both references.
  - A leader committer that performs the flush itself: the leader would be a tokio task blocked on a flush.

## TS-10 Torn-page protection

- **Decision**: no separate mechanism. TS-09 already protects against torn writes:
  - a data page is written in place only after its image is durable in the WAL;
  - a WAL segment is recycled only after the checkpoint that covers it has synced `data`;
  - recovery overwrites whole pages from their images.

  Page checksums detect any other torn page.
- **Satisfies**: FR-REC-002, FR-REC-003.
- **Evidence**:
  - PostgreSQL writes a full page image on the first change after a checkpoint (R-PG `src/backend/access/transam/xloginsert.c`, `needs_backup = (page_lsn <= RedoRecPtr)`).
  - InnoDB doublewrite buffer (R-INNO `include/buf0dblwr.h`, `buf/buf0dblwr.cc`).
  - Kuzu shadow file applied at checkpoint (R-KUZU `src/storage/shadow_file.cpp:66-79, 91-135`).
  - SQLite WAL page images; redb and LMDB copy-on-write.
- **Rejected**: the doublewrite file (an extra file, an extra sync ordering, an extra recovery pass); a separate full-page-write rule (redundant with TS-09); relying on atomic 8 KiB writes (not guaranteed; PostgreSQL relies only on 512 bytes).

## TS-11 Checkpoint

- **Decision**:
  - A fuzzy checkpoint:
    1. record the redo point (the current WAL end);
    2. write every frame dirty at that point, following the WAL-before-data rule;
    3. `sync_data` the `data` file;
    4. replace `CONTROL`;
    5. recycle the segments below the redo point.
  - Triggers: every 15 minutes or every 100 000 transactions, whichever comes first; at clean close; at the start of a backup.
  - Statistics ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)) and the identifier counter are saved at each checkpoint.
- **Satisfies**: FR-REC-001, FR-REC-002, FR-DB-008, FR-OBS-001, FR-OPS-001.
- **Evidence**:
  - PostgreSQL fixes the redo pointer first, then writes the buffers dirty at that moment (P-STO §Q4).
  - Neo4j `db.checkpoint` is `PERIODIC`, with `db.checkpoint.interval.time` 15m and `db.checkpoint.interval.tx` 100000 (D-CFG). These defaults are adopted.
- **Rejected**:
  - A sharp checkpoint that stops commits: no requirement accepts the stall.
  - A WAL-size trigger alone (Kuzu): Neo4j's defaults apply to undecided behaviour.

## TS-13 Recovery

- **Decision**: open performs, in order:
  1. take `LOCK`;
  2. verify `CONTROL` and the format version ([TS-28](operations.md#ts-28-format-versioning-and-migration));
  3. read the WAL from the redo point and apply the page images of each complete group, in order;
  4. stop at the first invalid frame. If any valid frame follows an invalid one, the WAL is corrupt and the open fails;
  5. restore the identifier counter from the highest commit frame;
  6. start with an empty old-version store;
  7. run a checkpoint.

  No undo phase is needed (NO-STEAL).
- **Satisfies**: FR-REC-001, FR-REC-002, FR-REC-003, FR-REC-008, FR-DB-009, FR-DB-004.
- **Evidence**: SQLite WAL recovery uses only frames up to the last valid commit frame (R-SQLITE `src/wal.c`). PostgreSQL treats an invalid record at the WAL end as the end of the log (P-STO §Q4).
- **Rejected**: treating every invalid frame as the end of the log, which would hide corruption inside the WAL (FR-REC-008).

## TS-14 MVCC

- **Decision**:
  - **Private write sets.** A transaction's writes (records, adjacency, index and catalog entries) stay in memory, charged to the memory cap, until commit. The transaction's reads merge them over its snapshot.
  - **Immutable entries.** Adjacency, index and catalog entries are never edited in place. Visibility is a comparison of their stamps with the snapshot. An update is a delete stamp plus a new entry.
  - **Mutable records.** For node and edge labels and properties, the newest committed version is in the page, with its `version_ts`. Older versions live in an in-memory old-version store, as before-image chains from newest to oldest. That store is not durable and is charged to the memory cap.
  - **Memory limit.** When an old version cannot be reserved, the statement that needs it fails (FR-LIM-005). No other transaction is aborted, and nothing is spilled.
  - **Garbage collection.** The horizon is the oldest active snapshot. Collection runs after commits and every 30 seconds:
    - it frees old versions below the horizon;
    - it removes entries whose `deleted_ts` is below the horizon, logged like any page change;
    - it freezes stamps ([TS-06](storage.md#ts-06-graph-layout)) when a page is next rewritten.
  - **Snapshot.** The highest published commit timestamp ([TS-16](#ts-16-commit-protocol)).
- **Satisfies**: FR-TX-001, FR-TX-003, FR-TX-004, FR-TX-006, FR-TX-010, FR-LIM-003, FR-LIM-005, FR-ALG-004, NFR-PERF-003, NFR-PERF-004.
- **Evidence**:
  - Memgraph keeps newest-to-oldest delta chains and collects garbage from the oldest active start timestamp (R-MG `src/storage/v2/delta.hpp`; `inmemory/storage.cpp:3044, 3115`), every 30 s by default (`src/flags/general.cpp:105`).
  - Neo4j keeps transaction state in memory until commit (R-NEO `kernel/.../api/state/TxState.java`).
  - Kuzu keeps uncommitted inserts in local storage (P-TX §L1.1).
  - HyPer keeps undo buffers and collects from the oldest active transaction (D-HYPER §2).
  - Wu et al.: newest-to-oldest delta storage performs better (P-TX §L2.1).
- **Crates**: turso `core/mvcc` (an in-memory MVCC after Larson et al., MIT), surrealkv and fjall snapshots: study only. None can be used without its own storage ([TS-04](storage.md#ts-04-own-storage-engine)).
- **Rejected**:
  - Versions kept in the pages (PostgreSQL heap): space in pages and vacuum.
  - Uncommitted writes in place (Memgraph, HyPer): a page image would then carry uncommitted data ([TS-09](#ts-09-wal-and-group-commit)).
  - A durable undo log (InnoDB): needed only when uncommitted data can reach pages.
  - Aborting the oldest snapshot or spilling old versions: excluded by FR-LIM-005.

## TS-15 Serializable isolation and write locks

- **Decision**:
  - **Read sets.** A serializable read-write transaction records its reads as `(tree, key range)` intervals: point reads, adjacency prefixes, label-index prefixes, index ranges, Hilbert ranges and whole trees. When the read set exceeds its memory budget, ranges coarsen to whole trees.
  - **Validation at commit.** The read set is checked against the keys written by every transaction that committed after this transaction's snapshot. Any overlap is a serialisation failure, and the transaction aborts (FR-TX-009).
  - **No validation for** read-only transactions (FR-TX-010) or snapshot-isolation transactions (FR-TX-004).
  - **Lock table.** Keyed by `(graph, resource)`, with X and S modes; locks are held until the commit ends.

    | Resource | Mode | Taken by |
    |---|---|---|
    | node content | X | label or property change, delete |
    | node existence | X / S | node delete / edge create or delete at each endpoint |
    | edge | X | edge change or delete |
    | unique value `(constraint, value)` | X | writer of a constrained value |
    | label or edge type (schema) | S / X | every writer of that label or type / constraint creation ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints)) |

  - **Write-write rule.** A transaction that obtains a lock on an element whose `version_ts` is newer than its snapshot fails with a serialisation failure.
  - **Deadlocks.** Checked when a lock request would wait: a depth-first search of the waits-for graph from the requester. If the request closes a cycle, the requester aborts. Waiters wake on cancellation.
- **Satisfies**: FR-TX-003, FR-TX-004, FR-TX-005, FR-TX-006, FR-TX-007, FR-TX-008, FR-TX-009, FR-TX-010, BR-TX-001, FR-STO-003, FR-IDX-006.
- **Evidence**:
  - HyPer validates update transactions at commit against the writes committed during their lifetime; read-only transactions read a stable snapshot without validation (D-HYPER §2.3, §3.2; [S42]).
  - PostgreSQL REPEATABLE READ and SERIALIZABLE: a writer waits for a concurrent updater and fails if that updater commits ("could not serialize access due to concurrent update", R-PG `src/backend/executor/nodeModifyTable.c:403`).
  - Neo4j detects deadlocks while a lock request waits (R-NEO `community/lock/.../forseti/ForsetiClient.java:836`), and its writers take shared label and relationship-type schema locks (`kernel/.../newapi/Operations.java:505, 792`).
  - Memgraph has no serializable level and does not wait for locks (R-MG `src/storage/v2/isolation_level.hpp:20`, `mvcc.hpp:170-212`).
  - PostgreSQL SSI uses predicate locks at tuple, page or relation granularity (R-PG `src/backend/storage/lmgr/README-SSI:300-322`).
- **Crates**: fjall `OptimisticTxDatabase` records single, range and whole reads and rejects a commit that overlaps a later commit's writes (R-FJALL `src/tx/optimistic/conflict_manager.rs`, `oracle.rs`). It is the same family of method; study only ([TS-04](storage.md#ts-04-own-storage-engine)). `txn` and `skipdb` (about 7 k downloads each) see too little use (NFR-DEP-001).
- **Rejected**:
  - PostgreSQL SSI: fewer false aborts, but far more code (`predicate.c`, about 5 000 lines); the user chose commit-time validation (FR-TX-009).
  - One writer at a time (Kuzu, redb, LMDB): fails FR-STO-003 and FR-TX-005.
  - Strict two-phase locking: read locks fail FR-TX-006.
  - Timeout-then-check deadlock detection (PostgreSQL `deadlock_timeout`): late, and its victim is not necessarily the requester (FR-TX-007).

## TS-16 Commit protocol

- **Decision**: a commit runs these steps in order:
  1. Read-only or snapshot-isolation transaction: release locks and return; nothing is validated or logged (FR-TX-010).
  2. Validate the read set ([TS-15](#ts-15-serializable-isolation-and-write-locks)).
  3. Take the commit latch and draw `commit_ts`.
  4. Apply the write set to pages, copying each before-image into the old-version store first.
  5. Join the open WAL group, then release the latch.
  6. Wait until the group is durable.
  7. Publish `commit_ts`, in timestamp order.
  8. Release the locks.
  9. Return, with a Bolt bookmark when the commit came over Bolt.

  Transactions run concurrently; only steps 3–5 are serialised. If the group fails to sync, the commit fails and the database becomes unusable.
- **Satisfies**: FR-TX-002, FR-TX-009, FR-TX-010, FR-STO-003, FR-BOLT-017, FR-REC-006.
- **Evidence**: Memgraph serialises timestamp assignment and WAL order under `engine_lock_` (R-MG `src/storage/v2/inmemory/storage.cpp:1081`). Neo4j returns from commit only after it has appended to the log and applied to the store (R-NEO `InternalTransactionCommitProcess.java:77-80`).
- **Rejected**:
  - Parallel apply under page latches only: harder to give groups a consistent cut, and no measurement shows the serial section limits a requirement (unverified until the NFR-PERF-002 baseline).
  - Publishing before durability: a reader would see a commit that a crash could lose.
