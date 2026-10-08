---
title: Operations
status: draft
last-reviewed: 2026-10-02
related:
  - storage.md
  - transactions-and-recovery.md
  - values-and-types.md
---

# Operations

## TS-26 Backup, restore, import and export

- **Decision**:
  - **Backup** is physical and full, and runs through the Rust API only.
    - If a fast initial load is running, the backup fails with an error (FR-OPS-001).
    - Steps:
      1. Force a checkpoint and take `start` = its redo point. Hold WAL recycling at `start`.
      2. Copy `CONTROL` and every page of `data-<g>` through the storage trait.
      3. Flush the WAL, take `stop` and the length of `data-<g>`, and copy the WAL from `start` to `stop`.
      4. Release WAL recycling.
      5. Write the manifest, as below.
    - Every page written during step 2 was changed after `start` through a logged commit, so its full image is in the copied WAL. A fast initial load, whose pages bypass the WAL, cannot run during a backup: a load requested while a backup runs waits until the backup ends.
    - **Exclusion.** A backup and the bulk build of a fast initial load (fast-load steps 3, 5 and 6, below) exclude each other through one latch (`Core::backup_or_load`, `src/storage/engine.rs:199`). The backup tries the latch without waiting (`Core::try_backup_or_load`, `src/storage/engine.rs:244-250`): if a build holds it, the backup fails with `Error::LoadRunning` before it writes anything to the target (`src/ops/backup.rs:235-237`, `src/storage/error.rs:56`); otherwise it holds the latch from step 1 to its end. The build waits for the latch while a backup holds it (`Core::backup_or_load`, `src/storage/engine.rs:226-238`), takes it before the commit latch, and holds it until its tree is installed or dropped (`src/storage/btree/bulk.rs:482`).
    - **Target.** A location the caller provides. It must exist, because the storage trait cannot create a directory ([TS-03](interfaces.md#ts-03-storage-abstraction)), and it must be empty: if `list` returns any entry, the backup fails before writing. Files of an earlier backup can therefore never mix with this one.
    - **Artefact.** Flat files directly in the target, written through the storage trait, as in the database location ([TS-05](storage.md#ts-05-files-and-pages)):
      - `CONTROL`, the copy from step 2;
      - `data-<g>`, the copy of `data-<g>`;
      - `wal-<g>-<lsn>`, the WAL bytes from `start` to `stop`: one file per database segment that holds an LSN from `start` to `stop`, both included, named as that segment ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)). The file of the segment that holds `stop` may be empty; restore still needs it, because open requires the segment that holds the WAL end ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 4);
      - `MANIFEST`: format version, database UUID, generation `<g>`, page size, `start`, `stop`, the length of `data-<g>` at `stop`, the length and CRC-32C of every other file, and a CRC-32C of the manifest itself.

      The artefact holds no `LOCK`, so an open of the target fails ([TS-27](storage.md#ts-27-single-process-lock)); only restore reads it.
    - **Completion marker.** `MANIFEST` is written last. After every other file has had `sync_data` and the target `sync_dir`, it is written as `MANIFEST.tmp`, synced, renamed to `MANIFEST`, and the target synced again. An artefact without a valid `MANIFEST` is incomplete.
  - **Restore** reads the artefact's flat files and writes into a location that holds no database. Steps:
    1. If the target holds `CONTROL`, fail (FR-OPS-009).
    2. Read the artefact's `MANIFEST`. If it is absent or its CRC-32C fails, fail: the backup is incomplete. Check its format version as open does ([TS-28](#ts-28-format-versioning-and-migration)).
    3. Check the length and CRC-32C of the artefact's `CONTROL`, `data-<g>` and every `wal-<g>-<lsn>` against `MANIFEST`. On a mismatch, fail (FR-REC-003).
    4. Create and lock `LOCK` in the target. List the target again; if it holds `CONTROL`, fail with "database exists": a create may have finished since step 1 (`src/ops/restore.rs:208-219`). Write `data-<g>` from the artefact's `data-<g>`, reserved with `fallocate` to the length recorded at `stop`, and one segment `wal-<g>-<lsn>` per artefact file of the same name ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)), each 16 MiB, zero-filled, with the copied bytes at their offsets. Write `BACKUP_END`: `stop` and the CRC-32C of those 8 bytes, little-endian (`encode_backup_end`, `src/storage/engine.rs:68-78`). `sync_data` each file, then `sync_dir`.
    5. Write `CONTROL` last, atomically ([TS-05](storage.md#ts-05-files-and-pages)): one valid slot holding the artefact's authoritative slot with its redo point set to `start` and the clean-shutdown flag cleared; the other slot zeroed.
    6. Open the database. Recovery ([TS-13](transactions-and-recovery.md#ts-13-recovery)) replays the WAL from `start` to its end, which is `stop`.

    **Backup end.** While `BACKUP_END` exists, every open of the location reads it before recovery and fails as corruption if the file fails its CRC-32C or if the recovered WAL ends before `stop`. That open writes no `CONTROL` slot. After the open checkpoint has recorded a redo point at or past `stop` ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 8), open removes `BACKUP_END` and calls `sync_dir` (`src/storage/engine.rs:494-503, 530-535, 560-567`). A later open of a restore whose WAL is incomplete therefore fails too, not only the open of step 6.

    The result is the state of one commit point, with element identifiers unchanged. A restore interrupted before step 5 completes leaves no `CONTROL`, so the location holds no database; a new restore, after step 4 takes `LOCK`, removes `wal-temp` and every `data-<h>` and `wal-<h>-<lsn>` the artefact does not contain, of any generation, then overwrites the files it writes (`src/ops/restore.rs:220-226`).
  - **Export layouts** (database, graph or query result, as FR-IMP-003 assigns):
    - **CSV**: separate node and edge files with the Neo4j header fields `:ID`, `:LABEL`, `:START_ID`, `:END_ID`, `:TYPE`.
      - Fields are quoted as RFC 4180 requires.
      - Inside an array or `:LABEL` field, elements are separated by `;`, and `\` escapes `;` and `\`.
      - An absent property is an empty field.
    - **JSON Lines**: one object per node (`type`, `id`, `labels`, `properties`) and per edge (`type`, `id`, `label`, `properties`, `start`, `end`), as APOC writes them [S44].
    - **GraphML**: one `<key>` per property; labels in a `labels` data key, as APOC writes them [S44].
    - **Native dump**: an lpg binary format with a format version and a CRC-32C per block, using the record encoding of [TS-21](values-and-types.md#ts-21-values-and-types).
    - **Query results**: CSV and JSON Lines write one record per result row, with the result's columns. GraphML writes the nodes and edges the result contains, including those inside paths and lists, and leaves out the other values.
  - **Type annotations (FR-IMP-006).** Values use the lexical forms of [TS-21](values-and-types.md#ts-21-values-and-types). A property, or a query-result column, is **uniform** when all its exported values have one type, and **mixed** otherwise.

    | Value | CSV | JSON Lines | GraphML |
    |---|---|---|---|
    | BOOLEAN, uniform | `boolean` column | JSON boolean | `attr.type="boolean"` |
    | 64-bit integer, uniform | `long` column | JSON number without fraction or exponent | `attr.type="long"` |
    | 64-bit float, finite, uniform | `double` column | JSON number with a fraction or exponent | `attr.type="double"` |
    | 64-bit float, NaN or ±infinity | annotated | tagged object | `attr.type="string"` + lpg type attribute |
    | character string, uniform, never empty | `string` column | JSON string | `attr.type="string"` |
    | DATE, LOCAL TIME, ZONED TIME, LOCAL DATETIME, ZONED DATETIME, uniform | `date`, `localtime`, `time`, `localdatetime`, `datetime` column | tagged object | `attr.type="string"` + lpg type attribute |
    | point, uniform | `point` column | tagged object | `attr.type="string"` + lpg type attribute |
    | list of one of the above, uniform | `<type>[]` column | JSON array | `attr.type="string"` + lpg type attribute |
    | any other type (both duration kinds, byte string, record, nested list, other numerics, node, edge, path) | `lpg:<type name>` column | tagged object | `attr.type="string"` + lpg type attribute |
    | any value of a mixed property or column, or a string column that holds an empty string | `lpg:annotated` column; each value written as `<type name>:<lexical form>` | each value by its own row above | `attr.type="string"`, lpg type attribute on each `<data>` element |

    - A tagged object has exactly the two keys `"$lpg_type"` and `"$value"`. Every JSON object in a property or value position is read as a tagged object.
    - The GraphML type attribute is in the namespace `https://crates.io/crates/lpg/graphml`.
  - **Transactional import** runs as one transaction. It adds all records or none (FR-IMP-004), and its writes with their old versions must fit the memory cap (FR-LIM-007).
  - **Fast initial load** targets an empty graph.
    1. Wait for a running backup to end. Take the X graph lock ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)), so writers of the graph wait; readers keep reading the empty graph. If the graph is not empty, fail.
    2. External merge sort of the input into temporary files through the storage trait, in the database location ([TS-05](storage.md#ts-05-files-and-pages)). On the sorted runs, check the constraints already defined on the graph. Each violation is an invalid record, handled by the abort-or-skip option (FR-IMP-004). An edge whose endpoint was skipped is invalid as well.
    3. Build the node, edge and adjacency trees bottom-up into newly reserved pages, filled to 90 % ([TS-05](storage.md#ts-05-files-and-pages)).
    4. Then build every index already defined on the graph bottom-up from sorted runs. The label or edge-type lookup tree is built only if that index exists.
    5. `sync_data`.
    6. Write one WAL group that installs the tree roots and the new allocation state.
    7. Release the graph lock.

    If the load fails or crashes before step 6, the recovered allocation state does not include its pages, so they are free again and the graph stays empty. ENOSPC during reservation fails the load.
  - **Element identifiers.** Both import modes assign new identifiers; input identifiers only connect edges to nodes.
- **Satisfies**: FR-OPS-001, FR-OPS-002, FR-OPS-003, FR-OPS-007, FR-OPS-008, FR-OPS-009, FR-OPS-010, FR-REC-003, FR-REC-004, FR-IMP-001, FR-IMP-002, FR-IMP-003, FR-IMP-004, FR-IMP-005, FR-IMP-006, FR-IDX-001, FR-LIM-007, BR-TYPE-002, NFR-PERF-005.
- **Evidence**:
  - PostgreSQL base backup: checkpoint, file copy while running, then the WAL from start to stop replayed at restore. Neo4j full backup copies the store and recovers ([P-STO](../evidence/research/research-01-storage.md) §Q7).
  - PostgreSQL's backup manifest records the WAL range (R-PG `src/backend/backup/basebackup.c:657-660`) and a CRC-32C per file by default (`basebackup.c:729`). `pg_basebackup` syncs the data, then durably renames `backup_manifest.tmp` to `backup_manifest`, so that an interrupted copy is not "mistaken for a valid backup" (`src/bin/pg_basebackup/pg_basebackup.c:2298-2321`).
  - PostgreSQL starts recovery of a restored backup from the start location in `backup_label`, not from the copied `pg_control` (R-PG `src/backend/access/transam/xlogrecovery.c:543-557, 1217-1230`). A checkpoint that completes during the copy can leave a later redo point in the copied `CONTROL`; restore therefore sets it to `start`.
  - `CONTROL` written last makes the location a database only when every other file is durable, as at create ([TS-05](storage.md#ts-05-files-and-pages)).
  - Backup end. PostgreSQL reads `backup_label` at the start of recovery and copies its requirement to reach the end of the backup into its control data, `ControlFile->backupEndRequired` (R-PG `src/backend/access/transam/xlogrecovery.c:345-347, 548-549, 943-963`); if the WAL ends first, startup fails with "WAL ends before end of online backup" (`src/backend/access/transam/xlog.c:6583-6599`).
  - Target. `pg_basebackup` requires its target directory to be empty, or creates it (R-PG `src/bin/pg_basebackup/pg_basebackup.c:739-782`, called at `2830`), and writes `backup_manifest` at the top of the target (`pg_basebackup.c:2313-2314`). RocksDB `Checkpoint` refuses a target that exists (R-ROCKS `utilities/checkpoint/checkpoint_impl.cc:409-417`) and copies every file, WAL included, flat into one directory by its file name (`checkpoint_impl.cc:472-484`). The SQLite backup API copies into one destination database (R-SQLITE `src/backup.c:133-146`). lpg keeps the empty-target rule, and cannot create the target.
  - Neo4j imports only into an empty database, and creates indexes and constraints during the import with `--schema` [S12].
  - Neo4j import header format and data types (D-HDR; [S38]).
  - APOC JSON Lines and GraphML layouts, and APOC query-result export (D-APOC; [S44]). GraphML allows six `attr.type` values (D-GML).
  - External sort and bottom-up build: R-PG `src/backend/utils/sort/tuplesort.c:11-15`, `src/backend/access/nbtree/nbtsort.c:4`.
  - Measured: bottom-up load of the adjacency tree at 1.88 M edges/s with a 1 GiB build cache, at 100 M edges ([M-LAYOUT](../evidence/layout-bench/results.md)).
- **Crates**:
  - `csv` 1.4.0 (Unlicense/MIT): dependency.
  - `serde_json` 1.0.151 with `serde`: dependency.
  - `quick-xml` 0.42.0 (MIT): dependency.
- **Rejected**:
  - Logical backup at an MVCC snapshot: a long reader holds old versions in memory for the whole copy (FR-LIM-005).
  - Restoring the copied `CONTROL` unchanged: its redo point can be later than `start`, and recovery would then skip changes to pages copied before it.
  - A manifest without file checksums: a backup damaged on its medium would be found only when a damaged page is read.
  - A `wal/` subdirectory in the artefact (`pg_basebackup` plain format writes WAL into `pg_wal/`, R-PG `src/bin/pg_basebackup/pg_basebackup.c:648-652, 684-696`; RocksDB `BackupEngine` keeps `meta/`, `private/<id>/` and `shared_checksum/` subdirectories, R-ROCKS `utilities/backup/backup_engine.cc:74-80`, because it shares files between incremental backups): [TS-03](interfaces.md#ts-03-storage-abstraction) has no directory operation, and the database itself keeps its WAL flat ([TS-05](storage.md#ts-05-files-and-pages)). Backups here are full only (FR-OPS-007).
  - A target created by the backup (`pg_basebackup`, RocksDB `Checkpoint`): the storage trait cannot create a directory.
  - Transactional import in independent batches: an abort would leave earlier batches committed, against FR-IMP-004.
  - Fast load by ordinary inserts: random I/O at 1–10 % cache.
  - APOC's conversion of mixed-type properties to plain strings: lossy, against FR-IMP-006.

Not confirmed in the official documentation (unverified): whether Neo4j's `point` header accepts a `srid` key, and whether GraphML validators accept an attribute from another namespace on `<data>` and `<key>`.

## TS-28 Format versioning and migration

- **Decision**:
  - **Version.** One `u32` format version, stored in each `CONTROL` slot and in page 0 of `data-<g>` ([TS-05](storage.md#ts-05-files-and-pages)). The `CONTROL` slot layout is the same in every format version, so any crate version can read it.
  - **At open.** An older version fails with "migration required". A newer version fails without writing anything.
  - **Migration** from N−1 to N runs on the closed location:
    1. Take `LOCK`; if the database is open, fail.
    2. If `CONTROL` does not show a clean shutdown, fail, asking for the database to be opened and closed with the previous crate version.
    3. If `available_space` is less than the size of `data-<g>` plus its `wal-<g>-<lsn>` segments, fail and change nothing.
    4. Write `data-<g+1>` in the new format inside the location, `sync_data` it, then `sync_dir`. Every page of `data-<g+1>` has a page LSN of at most R, the redo point of the authoritative `<g>` slot; copying each page's old LSN meets this. Step 4 writes no WAL segment of generation `<g+1>`.
    5. Write the next `CONTROL` slot with generation `<g+1>`, version N and redo point R; its other fields are those of the authoritative `<g>` slot. This one slot write is the switch.
    6. Remove the generation-`<g>` files.
  - **WAL position.** LSNs continue across generations. Step 2 requires a clean shutdown, which ends with the clean-close checkpoint ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)), so R is the end of the `<g>` WAL and nothing follows it. The WAL writer creates the first segment of `<g+1>` as `wal-<g+1>-<R>`, starting at LSN R ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)). Until then the database has no `<g+1>` segment; the step-5 slot keeps the clean-shutdown flag set, so open reads this as an empty log ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 4). Every page LSN is therefore at most the durable LSN, as the WAL-before-data rule requires ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap), [TS-10](transactions-and-recovery.md#ts-10-torn-page-protection)).
  - **Crash safety.** A crash before step 5 leaves the old database intact; the next open removes `<g+1>`. A crash after step 5 leaves the new database; the next open removes `<g>`.
  - **Rules.** Element identifiers are copied unchanged. An older format needs the intermediate crate versions; the error names the version required. There is no downgrade.
- **Satisfies**: FR-OPS-004, FR-OPS-011, FR-OPS-012, FR-OPS-013, BR-TYPE-002, FR-REC-007.
- **Evidence**:
  - Neo4j migrates only a stopped database and rewrites all data, and does not support downgrades ([S8], [S9], [S10]).
  - PostgreSQL's `pg_upgrade` refuses a source cluster that was not shut down cleanly [S47].
  - WAL position. `pg_upgrade` transfers relation files by copy, clone or link, so their page LSNs are kept (R-PG `src/bin/pg_upgrade/relfilenumber.c:531, 590-611`). It then sets the new cluster's next WAL file to the old cluster's "First log segment after reset" (`src/bin/pg_upgrade/pg_upgrade.c:866-872`; `src/bin/pg_upgrade/controldata.c:295-311`). `pg_resetwal` never moves the WAL start backwards: it starts beyond the old redo point and every existing segment (`src/bin/pg_resetwal/pg_resetwal.c:961-1020, 517-518`) and sets the redo point there (`pg_resetwal.c:914-919`). A page LSN beyond the flushed WAL is an error (`src/backend/access/transam/xlog.c:3026-3030`).
- **Rejected**:
  - Rewriting pages one by one inside the live files: a crash midway would leave a mixed format.
  - Migration at open: FR-OPS-004 requires an explicit migration.
  - Switching by renaming files: several renames cannot be made atomic, while one `CONTROL` slot write is.
  - Restarting LSNs at zero in `<g+1>`: step 4 would have to reset every page LSN, and no requirement gains from it.
  - Writing a first `<g+1>` segment during migration (`pg_resetwal` writes a segment holding a checkpoint record, `WriteEmptyXLOG`, `pg_resetwal.c:1138-1221`): lpg's checkpoint writes no WAL record ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)), so the segment would hold no frame.
