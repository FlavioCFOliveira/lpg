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
    - The header holds the frame's LSN, its salt and a CRC-32C of the frame. A commit frame also records the LSN of its group's first frame. A frame is valid when its CRC-32C matches and its LSN equals its position.
    - **Salt.** Each open draws one salt: the sequence number of the `CONTROL` slot written by the checkpoint of [TS-13](#ts-13-recovery) step 8. Every frame written until the database closes carries it. Sequence numbers only grow ([TS-05](storage.md#ts-05-files-and-pages)), so each open's salt is above the salt of every frame already on disk.
    - **Valid group.** A group is valid when its commit frame is valid, the first LSN it records is the position where the group starts, every frame from there to the commit frame is valid and carries the commit frame's salt, and that salt is at least the salt of the group before it. A group that is not valid did not commit.
    - **Why.** After recovery discards a torn tail, the writer reuses its positions ([TS-13](#ts-13-recovery) step 5). Frames of the discarded group can remain beyond the new groups with a valid CRC-32C and a matching LSN. Their salt is lower, or their commit frame records another first LSN, so they never form a valid group.
  - **LSN.** A `u64` byte position in the WAL's frame stream: the frames of every segment, in segment order.
    - A frame's LSN is the position of its first byte. Frames never span segments: a segment holds ⌊16 MiB / frame size⌋ frames, and the bytes after its last frame are not part of the stream.
    - The next segment's first LSN is this segment's first LSN plus that number of frame sizes. Segment boundaries are therefore not multiples of 16 MiB; each segment's first LSN is read from its name.
    - A segment *holds* the LSNs from its first LSN up to the next segment's first LSN.
    - The **WAL end** is the LSN of the next frame to write; after the last frame of a segment, it is the next segment's first LSN. The **durable LSN** is the WAL end after the last synced group.
    - Group *k*+1 starts at the WAL end after group *k*. A page takes the next frame position of the open group the first time it changes under the commit latch ([TS-16](#ts-16-commit-protocol) step 6, or garbage collection); a page already in the open group keeps its position. The commit frame takes the last position when the writer closes the group.
    - **Page LSN** (page header, [TS-05](storage.md#ts-05-files-and-pages)): the LSN that follows the frame holding the page's latest image. It is set in the same latched write that changes the page, so a changed page can never be written to `data` before its group is durable: a cache slot is written only when its page LSN is at most the durable LSN ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). A page never logged holds 0.
    - **Start.** Create writes redo point 0 ([TS-05](storage.md#ts-05-files-and-pages)), so the first segment of generation 1 is `wal-1-0000000000000000`. A migrated generation starts at the redo point R ([TS-28](operations.md#ts-28-format-versioning-and-migration)).
  - **Ordering.** Group *k*+1 is written only after the `sync_data` of group *k* has returned, so only the last group can be partly on disk.
  - **Capture.** Images are copied under the commit latch ([TS-16](#ts-16-commit-protocol)), so an image never holds a change from a later group. Image memory is outside the memory cap ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). Garbage collection changes pages under the same latch, so its changes also belong to exactly one group.
  - **Writer.** One dedicated thread per database writes each group and calls `sync_data`. Committers wait on a notification, so no tokio worker blocks on a flush.
  - **Segments.** 16 MiB each. A new segment is created by writing zeros; a recycled one ([TS-11](#ts-11-checkpoint) step 5) keeps its old frames, which are invalid at their new positions because their LSNs differ. Each is a file `wal-<g>-<lsn>` in the database location ([TS-05](storage.md#ts-05-files-and-pages)): `<g>` is the generation, in decimal as in `data-<g>`; `<lsn>` is the segment's first LSN, a `u64`, written as 16 lowercase hexadecimal digits, zero-padded. A byte-wise sort of the names `list` returns therefore orders the segments of one generation by LSN. The writer creates every segment, the first one included.
    - Before writing a group, the writer makes sure that every segment the group's frames reach, and the segment that will hold the WAL end after the group, exists: recycled, or created with `sync_data` and then `sync_dir`.
    - Once any frame is written, the segment that holds the WAL end therefore exists. Open relies on this to detect a removed segment ([TS-13](#ts-13-recovery) step 4).
  - **Errors.**
    - ENOSPC while creating a segment fails the transaction that needed the space; reads continue (FR-REC-007). At open ([TS-13](#ts-13-recovery) step 8), it fails the open.
    - Any other failed WAL write or `sync_data` sets the unusable latch ([TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check)).
- **Satisfies**: FR-TX-001, FR-TX-002, FR-REC-001, FR-REC-002, FR-REC-006, FR-REC-007, FR-REC-008, FR-OBS-001.
- **Evidence**:
  - PostgreSQL WAL: CRC-32C per record (R-PG `src/include/access/xlogrecord.h:49`); segments zero-filled at creation, `wal_init_zero = true` (`src/backend/access/transam/xlog.c:134`); segment names are fixed-width, zero-padded hexadecimal (`%08X%08X%08X`, `src/include/access/xlog_internal.h:164-170`).
  - SQLite WAL: fixed-size page-image frames, each transaction closed by a commit frame (R-SQLITE `src/wal.c`, header comment).
  - Neo4j appends to its log, then applies to the store (R-NEO `kernel/.../api/InternalTransactionCommitProcess.java:77-80`). A dedicated `LOG_WRITER` thread drains commits in batches (`kernel/.../wal/TransactionLogQueue.java:111, 220`).
  - InnoDB: dedicated `log_writer` and `log_flusher` threads (R-INNO `log/log0write.cc:108-122`).
  - turso: leader and waiter group commit (R-TURSO `core/mvcc/database/group_commit.rs`).
  - Memgraph writes its WAL under `engine_lock_`, so commits are ordered by timestamp (R-MG `src/storage/v2/inmemory/storage.cpp:1081-1100`).
  - LSN as a byte position: PostgreSQL `XLogRecPtr` (R-PG `src/include/access/xlogdefs.h:17-21`) and InnoDB `lsn_t` (R-INNO `include/log0types.h:62-63`). SQLite numbers frames instead (`mxFrame`, R-SQLITE `src/wal.c:328`; `walFrameOffset`, `src/wal.c:498-500`).
  - Bytes outside the log are outside the LSN space: an InnoDB redo file's LSN capacity is its size minus its header, and each file starts at the end LSN of the file before it (R-INNO `log/log0files_dict.cc:252-277`).
  - Page LSN: PostgreSQL's `XLogInsert` returns the end of the record, "used as LSN for data pages", up to which WAL must be flushed before the page is written (R-PG `src/backend/access/transam/xloginsert.c:474-477`). InnoDB sets a page's newest modification to the end LSN of the mini-transaction that changed it (R-INNO `include/buf0flu.ic:57-85`).
  - Start LSN: PostgreSQL and InnoDB start above 0 because 0 is their invalid LSN (R-PG `src/include/access/xlogdefs.h:28`, first redo point at `src/backend/access/transam/xlog.c:5719`; R-INNO `include/log0constants.h:152-153`). lpg has no invalid LSN: 0 marks a page never logged, which every durable LSN allows to be written.
  - Group identity. SQLite accepts a frame only if its salts match the WAL header and its checksum, computed cumulatively over every earlier frame, matches; it changes the salts after each checkpoint "to prevent old and new frames in the WAL from being considered valid at the same time" (R-SQLITE `src/wal.c:59-68, 95-98`; `walRestartHdr`, `src/wal.c:2234-2241`). PostgreSQL checks that each WAL page header holds its own address, which "typically fails when an old WAL segment is recycled" (R-PG `src/backend/access/transam/xlogreader.c:1349-1367`), and that each record links to the previous one (`xlogreader.c:1193-1212`).
  - Segment creation ahead of use: PostgreSQL creates the next segment once the current one is 75 % used (R-PG `src/backend/access/transam/xlog.c:3802-3824`). InnoDB marks a redo file full only after a newer file exists (R-INNO `log/log0files_governor.cc:1748-1789`).
- **Crates**: `okaywal` 0.3.1 (17 k downloads, last release 2023-11-26) and the fjall journal: study only.
- **Known cost**: one 8 KiB image per page per group, and a WAL bounded by checkpoint triggers that count transactions, not bytes ([TS-11](#ts-11-checkpoint)). The NFR-PERF-002 baseline measures the volume (unverified).
- **Rejected**:
  - Physiological records with full-page writes (PostgreSQL): less WAL volume, but more record types and redo code. It is the fallback if the measured volume misses a target.
  - Undo plus redo (ARIES, InnoDB): undo is needed only when uncommitted data can reach disk.
  - No log, copy-on-write (redb, LMDB): FR-TX-002 and FR-REC-008 name a log; both references allow one writer at a time.
  - Variable-size frames: after a torn frame, the frames that follow could not be located.
  - Frame numbers as LSNs (SQLite): LSNs continue across generations ([TS-28](operations.md#ts-28-format-versioning-and-migration)), so a frame size changed by a later format would change the meaning of every stored page LSN and redo point.
  - Segment boundaries at multiples of 16 MiB (PostgreSQL): a generation starts at R, which can be any frame boundary, so its first segment would be short or frames would span segments.
  - A cumulative checksum (SQLite): recovery from the redo point would need the checksum of the frame before it, stored in `CONTROL` at each checkpoint.
  - A link to the previous record (PostgreSQL `xl_prev`): with fixed-size frames it always equals the frame's own LSN minus one frame size, so it adds nothing to the LSN check.
  - A random salt (SQLite's second salt): a collision is improbable, not impossible; the `CONTROL` sequence number is already persisted and strictly increasing.
  - Setting the page LSN when the group is written: between the change and that moment, eviction could write the changed page before its group is durable.

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
    2. Write every cache slot dirty at that point, following the WAL-before-data rule. Pages are written into space already reserved ([TS-05](storage.md#ts-05-files-and-pages)), so a checkpoint never extends a file.
    3. `sync_data` the `data` file.
    4. Write the next `CONTROL` slot ([TS-05](storage.md#ts-05-files-and-pages)).
    5. Recycle each segment that holds only LSNs below the redo point ([TS-09](#ts-09-wal-and-group-commit)). Rename it to the name after the newest segment and call `sync_dir`, so that it becomes a later segment of the WAL. If the segments after the one holding the WAL end already hold at least as many LSNs as the WAL used since the previous checkpoint's redo point, remove it instead.
  - Triggers: every 15 minutes or every 100 000 transactions, whichever comes first; at clean close; at the start of a backup.
  - Each checkpoint saves the statistics ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)) and the counters ([TS-05](storage.md#ts-05-files-and-pages)).
  - **Clean-shutdown flag** (in each `CONTROL` slot, [TS-05](storage.md#ts-05-files-and-pages)):
    - Clean close ends or aborts every transaction (FR-DB-008), stops accepting commits and waits until the last group is durable. It then runs the checkpoint, and writes that checkpoint's `CONTROL` slot with the flag set. No commit can follow it.
    - Every other `CONTROL` write has the flag cleared. The first one after open is the checkpoint of [TS-13](#ts-13-recovery) step 8, which completes before open returns, so the flag is clear on disk before any commit.
    - Closing an unusable database ([TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check)) runs no checkpoint, so the flag stays clear and the next open runs recovery (FR-REC-006), as after a drop (FR-DB-009).
    - Create and migration write the flag set; restore writes it clear ([TS-05](storage.md#ts-05-files-and-pages), [TS-28](operations.md#ts-28-format-versioning-and-migration), [TS-26](operations.md#ts-26-backup-restore-import-and-export)).
    - A set flag means the redo point is the WAL end and every change up to it is in `data`. [TS-13](#ts-13-recovery) step 4 (empty log) and [TS-28](operations.md#ts-28-format-versioning-and-migration) step 2 (migration) rely on it.
- **Satisfies**: FR-REC-001, FR-REC-002, FR-REC-006, FR-REC-007, FR-DB-008, FR-DB-009, FR-OBS-001, FR-OPS-001, FR-OPS-011.
- **Evidence**:
  - PostgreSQL fixes the redo pointer first, then writes the buffers dirty at that moment (P-STO §Q4).
  - Clean-shutdown state. PostgreSQL keeps one state in `pg_control` (`DBState`, R-PG `src/include/catalog/pg_control.h:97-106`). The shutdown checkpoint sets `DB_SHUTDOWNED` in the control-file update that records the checkpoint (`src/backend/access/transam/xlog.c:8089-8092`), and panics if any WAL was written during it (`xlog.c:8076-8078`). Crash recovery sets `DB_IN_CRASH_RECOVERY` before replay (`src/backend/access/transam/xlogrecovery.c:914`); the end of start-up sets `DB_IN_PRODUCTION` (`xlog.c:6913-6922`). `pg_upgrade` refuses a cluster that was not shut down cleanly (`src/bin/pg_upgrade/controldata.c:160-164`).
  - Recycling. PostgreSQL renames a segment it no longer needs to a future segment name (`RemoveXlogFile`, R-PG `src/backend/access/transam/xlog.c:4133-4151`; `InstallXLogFileSegment` with `durable_rename`, `xlog.c:3675-3720`), up to the WAL it expects before the next checkpoint (`XLOGfileslop`, `xlog.c:2303-2345`), and removes the rest (`xlog.c:4152-4159`). SQLite reuses its WAL file from the start, with new salts (`walRestartHdr`, R-SQLITE `src/wal.c:2234-2241`).
  - Trust trade-off of recycling. A recycled segment holds frames whose CRC-32C is valid. They are rejected only because the LSN in each header differs from the new position ([TS-09](#ts-09-wal-and-group-commit)); the CRC-32C covers the LSN, so a bit flip cannot make one match. The fault model of FR-REC-002 (torn and lost writes) cannot create a match either: a frame with the right LSN was written at that position.
  - Neo4j `db.checkpoint` is `PERIODIC`, with `db.checkpoint.interval.time` 15m and `db.checkpoint.interval.tx` 100000 (D-CFG). These defaults are adopted.
  - PostgreSQL also bounds WAL by bytes (`max_wal_size`). A byte trigger is added only if the baseline shows WAL size misses a target (NFR-SIMP-001).
- **Rejected**:
  - A sharp checkpoint that stops commits: no requirement accepts the stall.
  - A WAL-size trigger alone (Kuzu): Neo4j's defaults apply to undecided behaviour.
  - Removing every recycled segment: each new segment then costs 16 MiB of zero writes and new space, and ENOSPC there fails a commit (FR-REC-007).
  - Keeping every recycled segment: the WAL would never shrink after a burst of writes.
  - Clearing the flag only at the next periodic checkpoint: commits made before it would follow a slot still marked clean, and migration would discard them. PostgreSQL notes the same kind of window between allowing WAL writes and recording `DB_IN_PRODUCTION` on disk (R-PG `src/backend/access/transam/xlog.c:6904-6906`).
  - Clearing the flag with a separate `CONTROL` write at the first commit: an extra synced write on the commit path, while open already writes `CONTROL` at step 8 of [TS-13](#ts-13-recovery).
  - A shutdown-in-progress state (`DB_SHUTDOWNING`, R-PG `src/backend/access/transam/xlog.c:7733-7739`): no lpg step reads it.

## TS-13 Recovery

- **Decision**: open performs these steps, in order:
  1. If `CONTROL` is absent, fail without creating any file (FR-DB-004).
  2. Take `LOCK` ([TS-27](storage.md#ts-27-single-process-lock)).
  3. Read both `CONTROL` slots and use the valid one with the higher sequence number. If only one is valid, use it; if none is, fail ([TS-05](storage.md#ts-05-files-and-pages)). Check the format version ([TS-28](operations.md#ts-28-format-versioning-and-migration)).
  4. Check the WAL segments of generation `<g>` against the redo point R, then read the WAL from R, frame by frame ([TS-09](#ts-09-wal-and-group-commit) defines which LSNs a segment holds):
     - Segments that hold only LSNs below R are ignored; the checkpoint of step 8 recycles them.
     - If no segment holds R, the log is empty and R is its end only when `CONTROL` shows a clean shutdown ([TS-11](#ts-11-checkpoint)) and no segment starts after R. This is the state after create, after migration ([TS-28](operations.md#ts-28-format-versioning-and-migration)), and after a clean close whose segments were then removed, which loses nothing. In every other case a segment is missing.
     - From the segment that holds R to the newest segment, each must start where the one before it ends. A gap means a segment is missing.
     - When a segment holds R: if the last frame position of the newest segment holds a valid frame, the next segment is missing: [TS-09](#ts-09-wal-and-group-commit) creates it before writing that frame.
     - A missing segment fails the open as corruption (FR-REC-008). These checks detect a segment removed from outside, which no checksum can.
     - Apply the page images of each group whose commit frame is valid, in order. Before writing a page beyond the end of `data-<g>`, extend the file as [TS-05](storage.md#ts-05-files-and-pages) states.
  5. Where the next group is not valid ([TS-09](#ts-09-wal-and-group-commit)), at LSN X, look beyond X up to the end of the WAL for a later valid group: a valid group whose recorded first LSN is above X and whose salt is at least that of the last applied group (any salt if none was applied).
     - If one exists, the WAL is corrupt and the open fails (FR-REC-003, FR-REC-008).
     - Otherwise the group at X was not synced. It is a torn tail and is discarded, and the WAL end is X. Leftover frames beyond X are overwritten by later groups; until then the salt and first-LSN rules keep them from forming a valid group.
  6. Restore every counter (commit timestamp, element identifier, catalog object, token) as the maximum of `CONTROL` and the applied commit frames.
  7. Start with an empty old-version store.
  8. If the log was empty, create `wal-<g>-<R>` ([TS-09](#ts-09-wal-and-group-commit)). Then run a checkpoint; its `CONTROL` slot clears the clean-shutdown flag ([TS-11](#ts-11-checkpoint)).

  No undo phase is needed (NO-STEAL).

  **Damage in the newest group.** A bit flip inside the newest synced group, with no later valid commit frame, cannot be told from a torn tail. Step 5 discards that group and its commits. This is media failure, repaired by restoring a backup (FR-REC-004), as in PostgreSQL and SQLite (user decision 2026-10-02).
- **Satisfies**: FR-REC-001, FR-REC-002, FR-REC-003, FR-REC-004, FR-REC-008, FR-DB-004, FR-DB-009.
- **Evidence**:
  - SQLite ignores frames after the last valid commit frame (R-SQLITE `src/wal.c`).
  - PostgreSQL ends recovery at a broken record at the end of WAL and remembers its position (`abortedRecPtr`, R-PG `src/backend/access/transam/xlogrecovery.c:321`).
  - The rule in step 5 relies on the ordering in [TS-09](#ts-09-wal-and-group-commit): a valid group after one that is not valid can exist only if the earlier group was already synced, which is corruption.
  - Missing segments (step 4). InnoDB refuses to start when its redo files do not form one chain of consecutive LSN ranges, naming the missing file (R-INNO `log/log0files_finder.cc:121-163`), and when the newest existing file is marked full, which "means that the real newest log file was lost" (`log/log0files_finder.cc:350-355`). PostgreSQL refuses to start when the WAL holding its checkpoint record or redo location cannot be read (R-PG `src/backend/access/transam/xlogrecovery.c:742-764`). RocksDB detects missing WAL files only with options that are off by default, `track_and_verify_wals_in_manifest` and `track_and_verify_wals` (R-ROCKS `include/rocksdb/options.h:673-707`).
- **Rejected**:
  - Ending the log at the first segment that does not exist (PostgreSQL crash recovery: a missing segment is logged at `DEBUG2` and ends the read, R-PG `src/backend/access/transam/xlogrecovery.c:3633-3637, 4386-4392`): synced commits in a removed segment, or in the segments after it, would be lost without an error (FR-REC-001, FR-REC-008).
  - Recording each new segment in `CONTROL` (RocksDB tracks WALs in its MANIFEST): a `CONTROL` write per segment, while the creation order of [TS-09](#ts-09-wal-and-group-commit) detects the same loss with no extra write.
  - A segment header with a "full" flag (InnoDB): segments have no header; the creation order lets the last frame position serve the same purpose.
  - Failing the open on any valid frame after an invalid one: the OS may write a later block of an unsynced group before an earlier one, so a normal power loss would refuse to open (FR-REC-002).
  - Treating every invalid frame as the end of the log: corruption inside the synced WAL would go unnoticed (FR-REC-008).
  - Writing the synced WAL end to `CONTROL` after each group sync, to tell damage in the newest group from a torn tail: a second synced write per group (user decision 2026-10-02).

## TS-14 MVCC

- **Decision**:
  - **Private write sets.** A transaction's writes (records, adjacency, index and catalog entries) stay in memory until commit. The transaction's reads merge them over its snapshot.
    - When a write enters the write set, the transaction reserves from the memory accountant the write itself, the before-image that applying it will create, and its entry in the committed write-key set.
    - A failed reservation fails that statement (FR-LIM-005, FR-LIM-007), so applying the commit needs new memory only for its page copies, whose failure is undone ([TS-16](#ts-16-commit-protocol) step 6).
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
  - Reserving before-images at commit apply: a failure would then fail the commit after its work, instead of the statement that made the write.
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
  2. **Reserve file space.** Outside the latch, make sure the free-page reserve covers this commit's estimate ([TS-05](storage.md#ts-05-files-and-pages)). ENOSPC fails the transaction before it takes the latch; reads continue (FR-REC-007). The estimate is not a bound; step 6 meets an overrun.
  3. Take the commit latch and draw `commit_ts`. A commit that fails while it holds the latch (steps 4 and 6) gives `commit_ts` back before releasing the latch: no other commit has drawn or used it, so the next commit draws the same value and the published timestamps have no gap.
  4. **Serializable transactions only:** validate the read set ([TS-15](#ts-15-serializable-isolation-and-write-locks)). On failure, release the latch and abort.
  5. Register the write-key set.
  6. **Apply the batch** to pages, copying each record's before-image into the old-version memory reserved earlier ([TS-14](#ts-14-mvcc)). Nothing is pinned or reserved up front: pages are fetched as the apply reaches them, and a root-to-leaf path is pinned only while a split runs (user decision 2026-10-02).
     - **Page copies.** Before its first change in this step, each existing page is copied into memory reserved from the accountant ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). New pages need no copy. The copies are in memory, exist only to undo this step, and are released when it ends; they are not the MVCC before-images of [TS-14](#ts-14-mvcc).
     - **Overrun.** Pages come from the free list, then from the reserve of step 2. If the reserve runs out, the committer extends the file under the latch, as [TS-05](storage.md#ts-05-files-and-pages) extends it.
     - **Undo.** If anything fails in this step (ENOSPC from that extension, a refused memory reservation, an I/O or checksum error), the committer:
       1. restores every copied page, newest first change first;
       2. restores the in-memory state the step changed: the free list and its reuse tags, `page_count` and the data file's allocation position, the next tree identifier, the next frame position of the open group ([TS-09](#ts-09-wal-and-group-commit)), and the old versions it added. The tree directory, the roots and the persisted free list are pages, so step 1 restores them;
       3. returns each page the step took to the free list or the reserve it came from. A reader may have reached it through a link the step wrote, so the next commit that takes one of these pages first waits until every tree read operation that started before the undo ended has ended, the reuse rule of [TS-05](storage.md#ts-05-files-and-pages) applied to the undo;
       4. releases the copies, withdraws the write-key set, gives back `commit_ts`, releases the latch and fails the commit.
     - ENOSPC and memory failures fail only this commit; the database stays usable (FR-REC-007, FR-LIM-005, FR-LIM-007). An I/O or checksum failure is undone the same way and also sets the unusable latch, as [TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check) requires (FR-REC-005, FR-REC-006).
  7. Join the open WAL group, then release the latch.
  8. Wait until the group is durable.
  9. Publish `commit_ts`, in timestamp order.
  10. Release the locks.
  11. Return, with a Bolt bookmark when the commit came over Bolt.

  Snapshot-isolation transactions that wrote data run every step except validation, so they are logged, durable and published like any other.

  A failure in step 6 is undone as stated there. Step 7 cannot fail for lack of memory: page images are outside the cap ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). A failed write or sync of the group fails its commits and makes the database unusable (FR-REC-006).

  Transactions run concurrently; only steps 3–7 are serialised.

  **Commit point.** A transaction is committed when its commit frame is on stable storage (step 8). If the process stops before step 11 returns, the caller cannot know the outcome; recovery keeps the commit exactly when its group was synced ([TS-13](#ts-13-recovery)). User decision 2026-10-02 (FR-REC-001).

  **Batch.** Step 6 applies a batch: an ordered list of `(tree, key, put value | delete)`.
  - The transaction layer ([TS-14](#ts-14-mvcc), [TS-15](#ts-15-serializable-isolation-and-write-locks)) builds the batch from the write set. Without that layer, the batch is the whole input of a commit, which then runs steps 2, 3 and 6–9 only, with no before-images.
  - Each entry reserves its bytes from the memory accountant ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)) when it is added; a failed reservation fails the statement that adds it. The reservation is released when step 6 ends.
- **Satisfies**: FR-TX-001, FR-TX-002, FR-TX-004, FR-TX-009, FR-TX-010, FR-STO-003, FR-BOLT-017, FR-REC-005, FR-REC-006, FR-REC-007, FR-LIM-005, FR-LIM-007.
- **Evidence**:
  - Memgraph serialises timestamp assignment and WAL order under `engine_lock_` (R-MG `src/storage/v2/inmemory/storage.cpp:1081`).
  - Neo4j returns from commit only after it has appended to the log and applied to the store (R-NEO `InternalTransactionCommitProcess.java:77-80`).
  - HyPer commits update transactions serially, in commit-timestamp order (D-HYPER §3.1).
  - PostgreSQL extends relation files in the session that needs the space, so ENOSPC fails that session's work with "Check free disk space" rather than the server (R-PG `src/backend/storage/smgr/md.c:529-536, 596-629`).
  - Only a commit that can no longer fail keeps its place in commit order. PostgreSQL writes the commit record, which fixes that order, inside the commit critical section (R-PG `src/backend/access/transam/xact.c:1469-1495`). Memgraph draws its commit timestamp under `engine_lock_` and, when a later check fails, marks that timestamp finished so it does not block others (R-MG `src/storage/v2/inmemory/storage.cpp:1081-1089, 1026-1033`).
  - Every page a change needs is pinned and every condition checked before the section in which a failure would leave unlogged changes in the pool (R-PG `src/backend/access/transam/README:439-446`). PostgreSQL's section covers one WAL record; a batch has no small bound, so step 6 undoes instead.
  - Undo from the original pages. SQLite saves a page to its rollback journal before the page's first change (`pagerAddPageToRollbackJournal`, R-SQLITE `src/pager.c:6053`, called from `pager_write`, `6150-6159`); for a statement it records a page in the sub-journal (`subjournalPage`, `src/pager.c:4590-4600`) and plays the sub-journal back on rollback (`pagerPlaybackSavepoint`, `3460`). On `SQLITE_FULL` or `SQLITE_NOMEM` it rolls back only the failing statement when it has a statement journal (`src/vdbeaux.c:3363-3393`). LMDB changes copies of pages, never the originals (`mdb_page_touch`, R-LMDB `libraries/liblmdb/mdb.c:3013-3019`), so `mdb_txn_abort` (`mdb.c:3849-3853`) discards them.
  - Space at the point of need. LMDB reports `MDB_MAP_FULL` when an allocation finds no room (`mdb_page_alloc`, R-LMDB `libraries/liblmdb/mdb.c:2858-2865`), and SQLite's `SQLITE_FULL` arises from the write that needs the space; both then roll back. PostgreSQL extends the file in the session that needs the page (above).
  - A batch of puts and deletes applied as one unit: LevelDB `WriteBatch` (R-LEVELDB `db/write_batch.cc:5-14`), whose writer joins several batches into one log write (`db/db_impl.cc:1226, 1281`).
- **Rejected**:
  - Parallel apply under page latches only: harder to give groups a consistent cut, and no measurement shows the serial section limits a requirement (unverified until the NFR-PERF-002 baseline).
  - Publishing before durability: a reader would see a commit that a crash could lose.
  - Marking a failed commit's timestamp as finished (Memgraph): publication would need a second state per timestamp, while returning the value under the latch needs none.
  - Drawing `commit_ts` after the apply of step 6: validation (step 4) is defined against its own `commit_ts` ([TS-15](#ts-15-serializable-isolation-and-write-locks)), and the steps would need renumbering.
  - Spilling dirty pages of an unfinished commit to the WAL when the pool is full (SQLite `pagerStress`, R-SQLITE `src/pager.c:4663`; LMDB `mdb_page_spill`, R-LMDB `libraries/liblmdb/mdb.c:2543`): pages would then have to be read back from the WAL, which [TS-09](#ts-09-wal-and-group-commit) never does.
  - Reserving the worst case before the latch (option B; InnoDB reserves per operation in proportion to tree height, R-INNO `btr/btr0cur.cc:2984-2990, 4691-4697`): one entry can split a leaf and every level above it, so the reserve would be about (height + 1) pages per entry, kept in a file that never shrinks (about 38 GiB for 10^6 entries at height 4); cache slots for the same worst case would have to fit the pool, failing large batches (FR-LIM-007); and the height can grow within a batch, with no simple bound. User decision 2026-10-02.
  - Keeping the estimate and making the database unusable when it is overrun (option C): contradicts FR-REC-007 and FR-LIM-005. User decision 2026-10-02.
  - Failing an overrun with ENOSPC without extending the file: the estimate is not a bound, so commits would fail while the device has space.
  - Undoing, releasing the latch, extending and retrying: the same result after a second apply; an extension under the latch happens at most once per 64 MiB extent ([TS-05](storage.md#ts-05-files-and-pages)).
  - Pinning every page and reserving cache slots before the apply (the earlier step 6): the pages a batch changes are known only by applying it.
  - Earlier rejected, now adopted: fetching pages as the apply reaches them. It was rejected because a failure would leave changed pages with no undo; the page copies remove that reason (user decision 2026-10-02).
