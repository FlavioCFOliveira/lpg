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

- **Decision**: a redo-only write-ahead log of full page images, under NO-STEAL: a change reaches disk only after its commit is durable, and is visible only after its commit is published ([TS-16](#ts-16-commit-protocol)).
  - **Group.** The unit of durability. A group holds:
    - the final image of every page changed by its commits, each page once;
    - one commit frame, which records the group's range of commit timestamps and the high-water marks of the element-identifier, catalog-object and token counters ([TS-05](storage.md#ts-05-files-and-pages)).
  - **Frames.**
    - Every frame has the same size: a header plus 8 KiB, with the commit frame padded. A frame's position is therefore known without reading the frames before it.
    - The header holds the frame's LSN, the segment salt and a CRC-32C. A frame is valid when all three match its position.
    - A group whose commit frame is not valid did not commit.
  - **Ordering.** Group *k*+1 is written only after the `sync_data` of group *k* has returned, so only the last group can be partly on disk.
  - **Capture.** Images are copied under the commit latch ([TS-16](#ts-16-commit-protocol)), so an image never holds a change from a later group. Garbage collection changes pages under the same latch, so its changes also belong to exactly one group.
  - **Writer.** One dedicated thread per database writes each group and calls `sync_data`. Committers wait on a notification, so no tokio worker blocks on a flush.
  - **Segments.** 16 MiB each, created by writing zeros, named by their first LSN.
  - **Errors.**
    - ENOSPC while creating a segment fails the transaction that needed the space; reads continue (FR-REC-007).
    - Any other failed WAL write or `sync_data` sets the unusable latch ([TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check)).
- **Satisfies**: FR-TX-001, FR-TX-002, FR-REC-001, FR-REC-002, FR-REC-006, FR-REC-007, FR-REC-008, FR-OBS-001.
- **Evidence**:
  - PostgreSQL WAL: CRC-32C per record (R-PG `src/include/access/xlogrecord.h:49`); segments zero-filled at creation, `wal_init_zero = true` (`src/backend/access/transam/xlog.c:134`).
  - SQLite WAL: fixed-size page-image frames, each transaction closed by a commit frame (R-SQLITE `src/wal.c`, header comment).
  - Neo4j appends to its log, then applies to the store (R-NEO `kernel/.../api/InternalTransactionCommitProcess.java:77-80`). A dedicated `LOG_WRITER` thread drains commits in batches (`kernel/.../wal/TransactionLogQueue.java:111, 220`).
  - InnoDB: dedicated `log_writer` and `log_flusher` threads (R-INNO `log/log0write.cc:108-122`).
  - turso: leader and waiter group commit (R-TURSO `core/mvcc/database/group_commit.rs`).
  - Memgraph writes its WAL under `engine_lock_`, so commits are ordered by timestamp (R-MG `src/storage/v2/inmemory/storage.cpp:1081-1100`).
- **Crates**: `okaywal` 0.3.1 (17 k downloads, last release 2023-11-26) and the fjall journal: study only.
- **Known cost**: one 8 KiB image per page per group, and a WAL bounded by checkpoint triggers that count transactions, not bytes ([TS-11](#ts-11-checkpoint)). The NFR-PERF-002 baseline measures the volume (unverified).
- **Rejected**:
  - Physiological records with full-page writes (PostgreSQL): less WAL volume, but more record types and redo code. It is the fallback if the measured volume misses a target.
  - Undo plus redo (ARIES, InnoDB): undo is needed only when uncommitted data can reach disk.
  - No log, copy-on-write (redb, LMDB): FR-TX-002 and FR-REC-008 name a log; both references allow one writer at a time.
  - Variable-size frames: after a torn frame, the frames that follow could not be located.

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
  - A fuzzy checkpoint runs these steps:
    1. Record the redo point (the current WAL end).
    2. Write every frame dirty at that point, following the WAL-before-data rule. Pages are written into space already reserved ([TS-05](storage.md#ts-05-files-and-pages)), so a checkpoint never extends a file.
    3. `sync_data` the `data` file.
    4. Write the next `CONTROL` slot ([TS-05](storage.md#ts-05-files-and-pages)).
    5. Recycle the segments below the redo point.
  - Triggers: every 15 minutes or every 100 000 transactions, whichever comes first; at clean close; at the start of a backup.
  - Each checkpoint saves the statistics ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)) and the counters ([TS-05](storage.md#ts-05-files-and-pages)).
- **Satisfies**: FR-REC-001, FR-REC-002, FR-REC-007, FR-DB-008, FR-OBS-001, FR-OPS-001.
- **Evidence**:
  - PostgreSQL fixes the redo pointer first, then writes the buffers dirty at that moment (P-STO §Q4).
  - Neo4j `db.checkpoint` is `PERIODIC`, with `db.checkpoint.interval.time` 15m and `db.checkpoint.interval.tx` 100000 (D-CFG). These defaults are adopted.
  - PostgreSQL also bounds WAL by bytes (`max_wal_size`). A byte trigger is added only if the baseline shows WAL size misses a target (NFR-SIMP-001).
- **Rejected**:
  - A sharp checkpoint that stops commits: no requirement accepts the stall.
  - A WAL-size trigger alone (Kuzu): Neo4j's defaults apply to undecided behaviour.

## TS-13 Recovery

- **Decision**: open performs these steps, in order:
  1. If `CONTROL` is absent, fail without creating any file (FR-DB-004).
  2. Take `LOCK` ([TS-27](storage.md#ts-27-single-process-lock)).
  3. Read both `CONTROL` slots and use the valid one with the higher sequence number. Check the format version ([TS-28](operations.md#ts-28-format-versioning-and-migration)).
  4. Read the WAL from the redo point, frame by frame. Apply the page images of each group whose commit frame is valid, in order.
  5. At the first invalid frame, look at the frames that follow it up to the end of the WAL:
     - if any of them is a valid commit frame of a later group, the WAL is corrupt and the open fails (FR-REC-003, FR-REC-008);
     - otherwise the invalid frame belongs to the last group, which was not synced. That group is a torn tail and is discarded.
  6. Restore every counter (commit timestamp, element identifier, catalog object, token) as the maximum of `CONTROL` and the applied commit frames.
  7. Start with an empty old-version store.
  8. Run a checkpoint.

  No undo phase is needed (NO-STEAL).
- **Satisfies**: FR-REC-001, FR-REC-002, FR-REC-003, FR-REC-008, FR-DB-004, FR-DB-009.
- **Evidence**:
  - SQLite ignores frames after the last valid commit frame (R-SQLITE `src/wal.c`).
  - PostgreSQL ends recovery at a broken record at the end of WAL and remembers its position (`abortedRecPtr`, R-PG `src/backend/access/transam/xlogrecovery.c:321`).
  - The rule in step 5 relies on the ordering in [TS-09](#ts-09-wal-and-group-commit): a valid commit frame after an invalid frame can exist only if the invalid frame was already synced, which is corruption.
- **Rejected**:
  - Failing the open on any valid frame after an invalid one: the OS may write a later block of an unsynced group before an earlier one, so a normal power loss would refuse to open (FR-REC-002).
  - Treating every invalid frame as the end of the log: corruption inside the synced WAL would go unnoticed (FR-REC-008).

## TS-14 MVCC

- **Decision**:
  - **Private write sets.** A transaction's writes (records, adjacency, index and catalog entries) stay in memory until commit. The transaction's reads merge them over its snapshot.
    - When a write enters the write set, the transaction reserves from the memory accountant the write itself, the before-image that applying it will create, and its entry in the committed write-key set.
    - A failed reservation fails that statement (FR-LIM-005, FR-LIM-007), so applying the commit never needs new memory.
  - **Immutable entries.** Adjacency, index and catalog entries are never edited in place. Visibility is a comparison of their stamps ([TS-06](storage.md#ts-06-graph-layout)) with the snapshot. An update is a delete stamp plus a new entry.
  - **Mutable records.** For node and edge labels and properties, the newest committed version is in the page, with its `version_ts`. Older versions live in an in-memory, non-durable old-version store, as before-image chains from newest to oldest.
  - **Committed write-key sets.** Each read-write commit registers the keys it wrote, for validation ([TS-15](#ts-15-serializable-isolation-and-write-locks)). They are charged to the cap.
  - **Garbage collection.** Runs after commits and every 30 seconds, under the commit latch:
    - frees old versions below the oldest active snapshot;
    - frees write-key sets older than the oldest snapshot of any active serializable transaction;
    - removes entries whose `deleted_ts` is below the oldest active snapshot, logged like any page change;
    - freezes stamps ([TS-06](storage.md#ts-06-graph-layout)) when a page is next rewritten.
  - **No escape from the cap.** No other transaction is aborted, and nothing is spilled to storage.
  - **Snapshot.** The highest published commit timestamp ([TS-16](#ts-16-commit-protocol)).
- **Satisfies**: FR-TX-001, FR-TX-003, FR-TX-004, FR-TX-006, FR-TX-010, FR-LIM-003, FR-LIM-005, FR-LIM-007, FR-ALG-004, NFR-PERF-003, NFR-PERF-004.
- **Evidence**:
  - Memgraph keeps newest-to-oldest delta chains and collects garbage from the oldest active start timestamp (R-MG `src/storage/v2/delta.hpp`; `inmemory/storage.cpp:3044, 3115`), every 30 s by default (`src/flags/general.cpp:105`).
  - Neo4j keeps transaction state in memory until commit (R-NEO `kernel/.../api/state/TxState.java`).
  - Kuzu keeps uncommitted inserts in local storage (P-TX §L1.1).
  - HyPer keeps undo buffers and recently committed changes only while an older transaction is active (D-HYPER §2.3, §2.4).
  - Wu et al.: newest-to-oldest delta storage performs better (P-TX §L2.1).
- **Crates**: turso `core/mvcc` (an in-memory MVCC after Larson et al., MIT), surrealkv and fjall snapshots: study only. None can be used without its own storage ([TS-04](storage.md#ts-04-own-storage-engine)).
- **Rejected**:
  - Versions kept in the pages (PostgreSQL heap): space in pages and vacuum.
  - Uncommitted writes in place (Memgraph, HyPer): a page image would then carry uncommitted data ([TS-09](#ts-09-wal-and-group-commit)).
  - Reserving before-images at commit apply: a failure there would leave pages half-changed with no undo.
  - Aborting the oldest snapshot or spilling old versions: excluded by FR-LIM-005.

## TS-15 Serializable isolation and write locks

- **Decision**:
  - **Read sets.** A serializable transaction records its reads as `(tree, key range)` intervals: point reads, adjacency prefixes, label-index prefixes, index ranges, Hilbert ranges and whole trees.
    - Read-set memory is reserved from the accountant.
    - When a reservation fails, the transaction merges ranges up to whole trees. Only if merging frees nothing does the statement fail (FR-LIM-001).
  - **Validation.** Under the commit latch, after `commit_ts` is drawn ([TS-16](#ts-16-commit-protocol)), the read set is checked against the write-key sets of every commit with a timestamp between this transaction's snapshot and its own `commit_ts`. Any overlap fails the commit with GQLSTATUS 25N11, provisional (OQ-005), and aborts the transaction (FR-TX-008, FR-TX-009).
  - **No validation for** read-only transactions (FR-TX-010) or snapshot-isolation transactions (FR-TX-004).
  - **Lock table.** Keyed by `(graph, resource)`, with X and S modes; locks are held until the commit ends.

    | Resource | Mode | Taken by |
    |---|---|---|
    | node content | X | label or property change, delete |
    | node existence | X / S | node delete / edge create or delete at each endpoint |
    | edge | X | edge change or delete |
    | unique value `(constraint, value)` | X | writer of a constrained value |
    | label or edge type (schema) | S / X | every writer of that label or type / constraint creation ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints)) |
    | graph | S / X | every writer of the graph / fast initial load ([TS-26](operations.md#ts-26-backup-restore-import-and-export)) |

  - **Write-write rule.** A transaction that obtains a lock on an element whose `version_ts` is newer than its snapshot fails with GQLSTATUS 25N11.
  - **Deadlocks.** Checked when a lock request would wait: a depth-first search of the waits-for graph from the requester. If the request closes a cycle, the requester aborts with GQLSTATUS 50N05. Waiters wake on cancellation.
- **Satisfies**: FR-TX-003, FR-TX-004, FR-TX-005, FR-TX-006, FR-TX-007, FR-TX-008, FR-TX-009, FR-TX-010, BR-TX-001, FR-STO-003, FR-IDX-006, FR-IMP-002.
- **Evidence**:
  - HyPer validates an update transaction's reads at commit against the changes of transactions that committed during its lifetime. Update transactions "are, however, committed serially", and their commit timestamps "determine the serialization order". Read-only transactions read a stable snapshot without validation (D-HYPER §2.3, §3.1, §3.2; [S42]).
  - The project's own research prescribes the same critical section: "Registration of `commit_ts` and of the write set happens in one short critical section, so no concurrent committer is missed" ([P-TX](../evidence/research/research-02-layout-mvcc.md) §L3.2).
  - PostgreSQL REPEATABLE READ and SERIALIZABLE: a writer waits for a concurrent updater and fails if that updater commits ("could not serialize access due to concurrent update", R-PG `src/backend/executor/nodeModifyTable.c:403`).
  - Neo4j detects deadlocks while a lock request waits (R-NEO `community/lock/.../forseti/ForsetiClient.java:836`). Its writers take shared label and relationship-type schema locks (`kernel/.../newapi/Operations.java:505, 792`). GQLSTATUS 50N05 and 25N11 come from the Neo4j status codes [S48]; the ISO conditions insert has no serialisation-failure subclass [S45].
  - Memgraph has no serializable level and does not wait for locks (R-MG `src/storage/v2/isolation_level.hpp:20`, `mvcc.hpp:170-212`).
  - PostgreSQL SSI uses predicate locks at tuple, page or relation granularity (R-PG `src/backend/storage/lmgr/README-SSI:300-322`).
- **Crates**: fjall `OptimisticTxDatabase` records single, range and whole reads and rejects a commit that overlaps a later commit's writes (R-FJALL `src/tx/optimistic/conflict_manager.rs`, `oracle.rs`). It is the same family of method; study only ([TS-04](storage.md#ts-04-own-storage-engine)). `txn` and `skipdb` (about 7 k downloads each) see too little use (NFR-DEP-001).
- **Rejected**:
  - Validation before `commit_ts` is drawn: a transaction committing in the gap would be missed, and serial order would no longer be commit order.
  - PostgreSQL SSI: fewer false aborts, but far more code (`predicate.c`, about 5 000 lines); the user chose commit-time validation (FR-TX-009).
  - One writer at a time (Kuzu, redb, LMDB): fails FR-STO-003 and FR-TX-005.
  - Strict two-phase locking: read locks fail FR-TX-006.
  - Timeout-then-check deadlock detection (PostgreSQL `deadlock_timeout`): late, and its victim is not necessarily the requester (FR-TX-007).

## TS-16 Commit protocol

- **Decision**: a commit runs these steps in order:
  1. **Read-only transaction** (it wrote nothing, at any isolation level): end the snapshot and return. Nothing is validated, logged or published (FR-TX-010).
  2. **Reserve file space.** Outside the latch, make sure the free-page reserve ([TS-05](storage.md#ts-05-files-and-pages)) covers this commit's bound. ENOSPC fails the transaction; reads continue (FR-REC-007).
  3. Take the commit latch and draw `commit_ts`.
  4. **Serializable transactions only:** validate the read set ([TS-15](#ts-15-serializable-isolation-and-write-locks)). On failure, release the latch and abort.
  5. Register the write-key set.
  6. Apply the write set to pages, copying each before-image into the reserved old-version memory first.
  7. Join the open WAL group, then release the latch.
  8. Wait until the group is durable.
  9. Publish `commit_ts`, in timestamp order.
  10. Release the locks.
  11. Return, with a Bolt bookmark when the commit came over Bolt.

  Snapshot-isolation transactions that wrote data run every step except validation, so they are logged, durable and published like any other.

  Steps 6 and 7 cannot fail for lack of memory or space, because both were reserved earlier ([TS-14](#ts-14-mvcc), step 2). An I/O failure there, or a failed sync of the group, fails the commit and makes the database unusable (FR-REC-006).

  Transactions run concurrently; only steps 3–7 are serialised.
- **Satisfies**: FR-TX-001, FR-TX-002, FR-TX-004, FR-TX-009, FR-TX-010, FR-STO-003, FR-BOLT-017, FR-REC-006, FR-REC-007, FR-LIM-005.
- **Evidence**:
  - Memgraph serialises timestamp assignment and WAL order under `engine_lock_` (R-MG `src/storage/v2/inmemory/storage.cpp:1081`).
  - Neo4j returns from commit only after it has appended to the log and applied to the store (R-NEO `InternalTransactionCommitProcess.java:77-80`).
  - HyPer commits update transactions serially, in commit-timestamp order (D-HYPER §3.1).
  - PostgreSQL extends relation files in the session that needs the space, so ENOSPC fails that session's work with "Check free disk space" rather than the server (R-PG `src/backend/storage/smgr/md.c:529-536, 596-629`).
- **Rejected**:
  - Parallel apply under page latches only: harder to give groups a consistent cut, and no measurement shows the serial section limits a requirement (unverified until the NFR-PERF-002 baseline).
  - Publishing before durability: a reader would see a commit that a crash could lose.
