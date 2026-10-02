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
  - **Backup** is physical and full, and runs through the Rust API only:
    1. Force a checkpoint and take `start` = its redo point. Hold WAL recycling at `start`.
    2. Copy `CONTROL` and every page of `data` through the storage trait.
    3. Flush the WAL, take `stop`, and copy the WAL from `start` to `stop`.
    4. Release WAL recycling.

    Every page written while step 2 runs was changed after `start`, so its full image is in the copied WAL. The copy is sequential.
  - **Restore** writes into a location that holds no database, then runs recovery up to `stop`. The result is the state of one commit point, with element identifiers unchanged. Restore over an existing database fails.
  - **Export formats** (database, graph or query result, as FR-IMP-003 assigns):
    - **CSV**: separate node and edge files with Neo4j header fields `:ID`, `:LABEL`, `:START_ID`, `:END_ID`, `:TYPE`, and arrays separated by `;`.
    - **JSON Lines**: one object per node (`type`, `id`, `labels`, `properties`) and per edge (`type`, `id`, `label`, `properties`, `start`, `end`), as APOC writes them.
    - **GraphML**: one `<key>` per property; labels in a `labels` data key, as APOC writes them.
    - **Native dump**: an lpg binary format with a format version and CRC-32C per block, using the record encoding of [TS-21](values-and-types.md#ts-21-values-and-types).
  - **Type annotations (FR-IMP-006).** Values are written in the lexical forms of [TS-21](values-and-types.md#ts-21-values-and-types):

    | GQL value type | CSV header type | JSON Lines | GraphML |
    |---|---|---|---|
    | BOOLEAN | `boolean` | JSON boolean | `attr.type="boolean"` |
    | 64-bit integer | `long` | JSON number | `attr.type="long"` |
    | 64-bit float | `double` | JSON number | `attr.type="double"` |
    | character string | `string` | JSON string | `attr.type="string"` |
    | DATE, LOCAL TIME, ZONED TIME, LOCAL DATETIME, ZONED DATETIME | `date`, `localtime`, `time`, `localdatetime`, `datetime` | tagged object | `string` + lpg type attribute |
    | point | `point` | tagged object | `string` + lpg type attribute |
    | list of one of the above | `type[]` | JSON array | `string` + lpg type attribute |
    | any other type (both duration kinds, byte string, record, nested list, other numerics) | `lpg:<type name>` | tagged object | `string` + lpg type attribute |

    - A tagged object has exactly two keys: `"$lpg_type"` and `"$value"`. Every JSON object in a property position is read as a tagged object.
    - The GraphML type attribute is in the namespace `https://crates.io/crates/lpg/graphml`.
  - **Import.**
    - **Transactional import** runs as normal transactions in batches.
    - **Fast initial load** targets an empty graph only:
      1. external merge sort of the input keys into temporary files through the storage trait;
      2. bottom-up build of the node, edge, adjacency and label-index trees into new pages, 90 % full;
      3. `sync_data`;
      4. one WAL group that installs the tree roots.

      If the load is interrupted, the roots are not installed and the graph stays empty.
    - Both modes assign new element identifiers. Input identifiers only connect edges to nodes.
    - Invalid records abort the import or are skipped, as the option says; the default is abort. Every invalid record is reported.
- **Satisfies**: FR-OPS-001, FR-OPS-002, FR-OPS-003, FR-OPS-007, FR-OPS-008, FR-OPS-009, FR-OPS-010, FR-REC-004, FR-IMP-001, FR-IMP-002, FR-IMP-003, FR-IMP-004, FR-IMP-005, FR-IMP-006, BR-TYPE-002, NFR-PERF-005.
- **Evidence**:
  - PostgreSQL base backup: checkpoint, file copy while running, then the WAL from start to stop replayed at restore. Neo4j full backup copies the store and recovers (P-STO §Q7).
  - Neo4j import header format and its data types (D-HDR; [S38]).
  - APOC JSON Lines and GraphML layouts (D-APOC). GraphML allows six `attr.type` values (D-GML).
  - External sort and bottom-up build: R-PG `src/backend/utils/sort/tuplesort.c:11-15`, `src/backend/access/nbtree/nbtsort.c:4`.
  - Measured: bottom-up load of the adjacency tree at 1.88 M edges/s with a 1 GiB build cache, at 100 M edges (M-LAYOUT).
- **Crates**:
  - `csv` 1.4.0 (Unlicense/MIT): dependency.
  - `serde_json` 1.0.151 with `serde`: dependency.
  - `quick-xml` 0.42.0 (MIT): dependency.
- **Rejected**:
  - Logical backup at an MVCC snapshot: a long reader holds old versions in memory for the whole copy (FR-LIM-005).
  - Differential backup and point-in-time restore: excluded by FR-OPS-007.
  - Fast load by ordinary inserts: random I/O at 1–10 % cache.

Whether Neo4j's `point` header accepts a `srid` key in the map was not confirmed (unverified).

## TS-28 Format versioning and migration

- **Decision**:
  - One `u32` format version is stored in `CONTROL` and in page 0 of `data`.
  - An open fails with "migration required" when the version is older, and fails without writing anything when it is newer.
  - Migration from N−1 to N runs on a closed database:
    1. take `LOCK`;
    2. write the new-format files into a temporary directory inside the location and sync them;
    3. switch by replacing `CONTROL` (rename, then `sync_dir`);
    4. remove the old files.

    A crash before step 3 leaves the old database intact.
  - Element identifiers are copied unchanged. An older format needs the intermediate crate versions; the error names the version required. There is no downgrade.
- **Satisfies**: FR-OPS-004, FR-OPS-011, FR-OPS-012, FR-OPS-013, BR-TYPE-002.
- **Evidence**: Neo4j migrates only a stopped database and rewrites all data; it does not support downgrades ([S8], [S9], [S10] of the functional specification).
- **Rejected**:
  - Rewriting pages one by one inside the live files: a crash midway would leave a mixed format.
  - Migration at open: FR-OPS-004 requires an explicit migration.
