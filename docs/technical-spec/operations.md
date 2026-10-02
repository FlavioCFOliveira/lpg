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
    - **Artefact.** A directory, written through the storage trait, holding:
      - `CONTROL`, the copy from step 2;
      - `data`, the copy of `data-<g>`;
      - `wal/`, the WAL bytes from `start` to `stop`, in segment files named as in the database;
      - `MANIFEST`: format version, database UUID, generation `<g>`, page size, `start`, `stop`, the length of `data-<g>` at `stop`, the length and CRC-32C of every other file, and a CRC-32C of the manifest itself.
    - **Completion marker.** `MANIFEST` is written last. After every other file has had `sync_data` and the directory `sync_dir`, it is written as `MANIFEST.tmp`, synced, renamed to `MANIFEST`, and the directory synced again. An artefact without a valid `MANIFEST` is incomplete.
  - **Restore** writes into a location that holds no database. Steps:
    1. If the target holds `CONTROL`, fail (FR-OPS-009).
    2. Read `MANIFEST`. If it is absent or its CRC-32C fails, fail: the backup is incomplete. Check its format version as open does ([TS-28](#ts-28-format-versioning-and-migration)).
    3. Check the length and CRC-32C of every file against `MANIFEST`. On a mismatch, fail (FR-REC-003).
    4. Create and lock `LOCK`. Write `data-<g>`, reserved with `fallocate` to the length recorded at `stop`, and the `wal-<g>/` segments, each 16 MiB, zero-filled, with the copied bytes at their offsets. `sync_data` each file, then `sync_dir`.
    5. Write `CONTROL` last: one valid slot holding the copied authoritative slot with its redo point set to `start` and the clean-shutdown flag cleared; the other slot zeroed. `sync_data`, then `sync_dir`.
    6. Open the database. Recovery ([TS-13](transactions-and-recovery.md#ts-13-recovery)) replays the WAL from `start` to its end, which is `stop`.

    The result is the state of one commit point, with element identifiers unchanged. A restore interrupted before step 5 leaves no `CONTROL`, so the location holds no database; a new restore overwrites the files it left.
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
    2. External merge sort of the input into temporary files through the storage trait. On the sorted runs, check the constraints already defined on the graph. Each violation is an invalid record, handled by the abort-or-skip option (FR-IMP-004). An edge whose endpoint was skipped is invalid as well.
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
    3. If `available_space` is less than the size of `data-<g>` plus `wal-<g>/`, fail and change nothing.
    4. Write `data-<g+1>` in the new format inside the location, with an empty `wal-<g+1>/`, then sync them and `sync_dir`.
    5. Write the next `CONTROL` slot with generation `<g+1>` and version N. This one slot write is the switch.
    6. Remove the generation-`<g>` files.
  - **Crash safety.** A crash before step 5 leaves the old database intact; the next open removes `<g+1>`. A crash after step 5 leaves the new database; the next open removes `<g>`.
  - **Rules.** Element identifiers are copied unchanged. An older format needs the intermediate crate versions; the error names the version required. There is no downgrade.
- **Satisfies**: FR-OPS-004, FR-OPS-011, FR-OPS-012, FR-OPS-013, BR-TYPE-002, FR-REC-007.
- **Evidence**:
  - Neo4j migrates only a stopped database and rewrites all data, and does not support downgrades ([S8], [S9], [S10]).
  - PostgreSQL's `pg_upgrade` refuses a source cluster that was not shut down cleanly [S47].
- **Rejected**:
  - Rewriting pages one by one inside the live files: a crash midway would leave a mixed format.
  - Migration at open: FR-OPS-004 requires an explicit migration.
  - Switching by renaming files: several renames cannot be made atomic, while one `CONTROL` slot write is.
