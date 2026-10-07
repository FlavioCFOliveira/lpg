---
title: Storage
status: draft
last-reviewed: 2026-10-03
related:
  - architecture.md
  - transactions-and-recovery.md
  - catalog-and-indexes.md
  - interfaces.md
---

# Storage

Graphs far larger than memory are a primary target. Efficiency is judged with the cache holding 1 % to 10 % of the data, on the reference hardware ([NFR-PERF-005](../functional-spec/non-functional-requirements.md#performance), [NFR-PERF-006](../functional-spec/non-functional-requirements.md#performance)). Every decision below is checked against that case.

## TS-04 Own storage engine

- **Decision**: lpg implements its own page file, buffer pool, B+tree, WAL, checkpoint and recovery, on `std`, with `rustix` for the two file-system calls that `std` lacks ([TS-03](interfaces.md#ts-03-storage-abstraction)).
- **Satisfies**: FR-STO-001, FR-STO-002, FR-STO-003, FR-TX-002, FR-REC-002, FR-REC-005, FR-REC-007, FR-REC-008, FR-LIM-003, FR-OPS-001, NFR-SAFE-001, NFR-PERF-003.
- **Evidence**: assessment of each embedded store as a dependency ([P-STO](../evidence/research/research-01-storage.md) §Q1, with file and line references):

  | Crate | Verdict | Blocking reason |
  |---|---|---|
  | redb 4.3.0 | study only | One writer at a time. Checksums are verified only by repair and `check_integrity`, not on reads (FR-REC-005). After ENOSPC every read fails (FR-REC-007). No log, no backup API. |
  | fjall 3.1.10 | study only | No pluggable filesystem (FR-STO-002, NFR-VER-004). |
  | sled 0.34.7 | study only | No filesystem trait; last stable release 2021. |
  | heed 0.22.1 (LMDB) | study only | `EnvOpenOptions::open` is an `unsafe fn` (NFR-SAFE-001); one writer; no memory cap. |
  | rocksdb 0.25.0 | study only | The C API exposes only the default and in-memory `Env`, so ABS-003 is impossible; C++ build. |
  | surrealkv 0.21.4 | study only | Opens files with `std::fs` directly. |
  | turso_core 0.8.1 | study only | A SQL engine, not a storage library. |

- **Rejected**: every crate above, for the reason given.

## TS-05 Files and pages

- **Decision**:
  - **Files.** A database location holds:
    - `LOCK`, the lock target ([TS-27](#ts-27-single-process-lock));
    - `CONTROL`;
    - `data-<g>`, one paged file whose page 0 is the header page (below);
    - `wal-<g>-<lsn>`, the WAL segments ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit));
    - `wal-temp`, present only while a segment is being created ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit));
    - `CONTROL.tmp`, present only while create or restore writes `CONTROL` (below);
    - `BACKUP_END`, present after a restore until an open has recovered past the backup's end ([TS-26](operations.md#ts-26-backup-restore-import-and-export));
    - `sort-temp-<sort>-<run>`, the run files of an external sort of the database ([TS-26](operations.md#ts-26-backup-restore-import-and-export) fast-load step 2). A sort removes its files when it ends, whether it succeeds or fails. They are never synced: they hold no state a crash must keep (`src/storage/sort.rs:24-33`).

    `<g>` is the file-set generation, starting at 1. Only migration changes it ([TS-28](operations.md#ts-28-format-versioning-and-migration)).

    **Leftovers.** Only names the crate creates are ever removed; `LOCK` and files the crate does not create never are (`is_leftover`, `src/storage/engine.rs:123-128`).
    - **At open**, after `CONTROL` is read: `data-<h>` and every `wal-<h>-<lsn>` with `<h>` ≠ `<g>`, `wal-temp`, `CONTROL.tmp` and every `sort-temp-*`. If open removed any file, it then calls `sync_dir` once (`src/storage/engine.rs:482-504`).
    - **At create**, before its first `sync_dir` (below): every `data-<h>` and `wal-<h>-<lsn>` of any generation, `wal-temp`, `CONTROL.tmp`, every `sort-temp-*`, and `BACKUP_END` (`src/storage/engine.rs:345-358`). A location without `CONTROL` holds no database, so these files belong to none. An interrupted restore can leave the WAL segments of another database; without their removal, the new database would replay them from redo point 0.

    Every file lies directly in the location; there is no subdirectory, because the storage trait has no directory operation ([TS-03](interfaces.md#ts-03-storage-abstraction)). One `sync_dir` of the location therefore makes the names of `CONTROL`, data and WAL files durable (user decision 2026-10-02).
  - **`CONTROL`** has two 4 KiB slots. Once `CONTROL` exists, they are written alternately in place with `write_at` and then `sync_data`. Each slot holds:
    - a sequence number, magic, format version, page size, database UUID and active generation `<g>`;
    - a clean-shutdown flag ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)) and the checkpoint redo point;
    - the counters: highest commit timestamp, next element identifier, next catalog object identifier, next token identifier;
    - a CRC-32C.

    The valid slot with the higher sequence number is authoritative. Page 0 of `data-<g>` must hold the same format version; a mismatch is corruption. If `data-<g>` is absent while `CONTROL` exists, the open fails as corruption (FR-REC-003); open never recreates it.
    - **One invalid slot** at open is treated as a torn `CONTROL` write: the database opens from the valid slot, the next `CONTROL` write (always to the non-authoritative slot) rewrites the invalid one, and the event is counted in a metric ([TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events)). This is the exception to FR-REC-003 set by the user decision of 2026-10-02. If both slots are invalid, the open fails (FR-REC-003).
  - **Pages.** 8 KiB, fixed at creation. Page header: CRC-32C bound to the page number ([TS-12](#ts-12-checksums-corruption-and-integrity-check)), page LSN, page type, tree identifier. The page number itself is not stored. Leaf and inner pages are slotted.
  - **Overflow.** A value longer than 2 048 bytes, a quarter of the page, is stored whole in a chain of overflow pages. Its entry keeps the value length and the first overflow page number.
  - **Key size.** A key is at most 1 024 bytes; a write with a longer key fails. A leaf therefore always holds its high key ([TS-06](#ts-06-graph-layout)) and at least two entries of the largest size, so every split succeeds.
  - **Header page and tree directory.**
    - Page 0 holds the magic, the format version, the first trunk page of the free list, the next tree identifier and `page_count`.
    - `page_count` is the high-water mark of `data-<g>`: pages below it are in use or in the free list; pages at or above it are reserved but unused. Without it, the reserved but unused pages could not be told from used pages after a reopen.
    - Page 1 is the root of the **tree directory**, a B+tree that maps each tree identifier to its root page. The directory is tree 0. Other tree identifiers are `u64`, start at 1, are monotonic and are never reused.
    - A root page keeps its number for the life of its tree: a root split moves the root's entries into new child pages and leaves the root in place. The directory therefore changes only when a tree is created, dropped, or installed by a bottom-up build ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints), [TS-26](operations.md#ts-26-backup-restore-import-and-export)).
    - Pages 0 and 1 change like any other page: under the commit latch, with their images in the WAL ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)). Each change is therefore atomic with its commit.
  - **Free space.**
    - Free pages are kept in a persisted free list plus an in-memory set. The persisted list is a chain of trunk pages; each holds free page numbers and the next trunk page.
    - **Reuse.** A page freed by the commit with timestamp `c` enters the persisted list in that commit. It enters the in-memory set, and may be reused, only when no tree reader can still hold its number ([TS-06](#ts-06-graph-layout)):
      - each tree read operation registers, when it starts, the `commit_ts` of the last commit whose apply step has finished ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol));
      - the page becomes reusable when every registered value is at least `c`.

      With MVCC the registered value is the snapshot, so the rule becomes the garbage-collection horizon ([TS-14](transactions-and-recovery.md#ts-14-mvcc)). At open no reader exists, so every listed page is reusable.
    - **Underflow.** A delete never merges or redistributes pages. A page is freed only when it is empty:
      - it is removed from its parent, and its key range passes to its right sibling;
      - it is unlinked from its left sibling, keeps its right link and is marked deleted, so a reader that reaches it moves right ([TS-06](#ts-06-graph-layout));
      - an inner page left without children is freed the same way;
      - the root and the right-most page of each level are never freed, so the height of a tree never decreases.
    - `data-<g>` grows in extents of 64 MiB, or 1 % of the file if that is larger. Each extent is reserved with `fallocate` before use; this extension is the only one.
    - Recovery ([TS-13](transactions-and-recovery.md#ts-13-recovery)) and restore ([TS-26](operations.md#ts-26-backup-restore-import-and-export)) extend the file the same way, before they write a page beyond its end. ENOSPC there fails the open or the restore; no database is open, so no reads are lost.
    - Before taking the commit latch, a committing transaction makes sure the reserve of unused pages covers its **estimate** ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol) step 2):

      E = 2 × (⌈B / 4 KiB⌉ + T) + Σ<sub>t</sub> h<sub>t</sub> + Σ<sub>t ∈ D</sub> (h<sub>t</sub> + 1) + ⌈d / 1 019⌉

      - B: the batch's key and value bytes; T: the trees it writes; h<sub>t</sub>: the height of tree t; D: the trees it deletes from; d: its deletes.
      - 2 × (⌈B / 4 KiB⌉ + T) + Σ h<sub>t</sub>: splits leave pages about half full, and a split creates at most one page per level.
      - Σ<sub>t ∈ D</sub> (h<sub>t</sub> + 1): one split caused by deletes per tree ([TS-06](#ts-06-graph-layout)); a split creates at most one page per level, and two at the root.
      - ⌈d / 1 019⌉: new free-list trunk pages, counting one freed page per delete. A trunk holds 1 019 page numbers (8 KiB less a 40-byte header, 8 bytes each). A freed page never becomes a trunk, because a reader may still hold it.
    - E is not a bound. One put or delete can split a full leaf and every level above it, and the overflow chains a batch frees are known only when it reads the old entries. When the reserve runs out during apply, the committer extends the file again under the commit latch; ENOSPC there is undone ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol) step 6).
    - If an extension hits ENOSPC, that transaction fails and reads continue (FR-REC-007).
    - Checkpoints and eviction write only into reserved space.
  - **Create** fails, leaving everything unchanged, if `CONTROL` already exists (FR-DB-003). Otherwise it creates and locks `LOCK`, checks again that `CONTROL` does not exist, removes the leftovers (above), creates `data-1` with `sync_data`, and calls `sync_dir`. It then writes `CONTROL` last, with redo point 0 ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)) and the clean-shutdown flag set (`src/storage/engine.rs:330-381`). The first `sync_dir` ensures `CONTROL` can never be durable while `data-1` is not. Create writes no WAL segment; the WAL writer creates the first one ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)). A location is a database exactly when `CONTROL` exists.
    - **Atomic `CONTROL` creation.** Create and restore ([TS-26](operations.md#ts-26-backup-restore-import-and-export) step 5) write both slots to `CONTROL.tmp`, call `sync_data`, rename it to `CONTROL`, and call `sync_dir` (`write_new`, `src/storage/control.rs:130-144`). A failure or a crash at any point leaves either no `CONTROL` or a complete, synced one. An existing `CONTROL.tmp` is replaced.
- **Satisfies**: FR-STO-001, FR-STO-003, FR-DB-003, FR-DB-004, FR-REC-003, FR-REC-007, FR-REC-008, FR-OPS-004, FR-OPS-011, FR-TYPE-003.
- **Evidence**:
  - Default page sizes: PostgreSQL 8 KiB; Neo4j `PAGE_SIZE = 8192` (R-NEO `community/io/.../pagecache/PageCache.java:51`); InnoDB 16 KiB; SQLite and Kuzu 4 KiB (R-KUZU `CMakeLists.txt:114`). M-LAYOUT ran on 8 KiB pages.
  - Two checksummed commit slots in the file header: redb (`docs/design.md` l.30-60, [P-STO](../evidence/research/research-01-storage.md) §Q9). PostgreSQL keeps `pg_control` within 512 bytes (P-STO §Q9).
  - PostgreSQL extends relation files with `posix_fallocate` in the session that needs the space, and reports ENOSPC there with "Check free disk space" (R-PG `src/backend/storage/smgr/md.c:529-536, 596-629`; `src/backend/storage/file/fd.c:2396-2425`).
  - Overflow threshold. PostgreSQL moves values out of line when a tuple exceeds the size at which four fit on a page (`TOAST_TUPLES_PER_PAGE = 4`, R-PG `src/include/access/heaptoast.h:23-26, 46-48`). SQLite caps a cell's local payload at 64/255 of the usable page (R-SQLITE `src/btree.c:3458-3471`). Both keep at least four entries per page.
  - Key size. PostgreSQL limits a B-tree item to a third of the page so that three items fit on every page (R-PG `src/include/access/nbtree.h:157-169`). LMDB also fixes a maximum key size (R-LMDB `libraries/liblmdb/mdb.c:713, 12685`).
  - Recovery extends the file. PostgreSQL redo extends a relation when a logged page lies beyond its end, "in recovery only" (R-PG `src/backend/access/transam/xlogutils.c:525-543`). SQLite's checkpoint gives the file a size hint before writing pages beyond its end (R-SQLITE `src/wal.c:2378`).
  - Header page and directory. SQLite page 1 holds the first free-list trunk page (R-SQLITE `src/btreeInt.h:68-69`) and is the root of the schema tree, which maps names to root pages (`src/sqliteInt.h:1285`). A root split keeps the root in place (`src/btree.c:9060-9068`, `balance_deeper`). LMDB's meta page holds the free-page tree and the main tree, which maps names to tree roots (R-LMDB `libraries/liblmdb/mdb.c:1377, 12331-12341`). redb's header holds the root of its table tree, which maps names to table roots (R-REDB `docs/design.md:16, 132`).
  - Page count. SQLite's file header holds the database size in pages at offset 28 (R-SQLITE `src/btreeInt.h:67`). InnoDB's space header, on the first page of the space, holds `FSP_FREE_LIMIT`, at and above which pages are free by definition (R-INNO `include/fsp0fsp.h:128-130, 141-146`).
  - Flat files. SQLite names its WAL by appending `-wal` to the database path, so the WAL lies beside the database (R-SQLITE `src/pager.c:4979-4984`). A redb database is one file (R-REDB `docs/design.md:14`).
  - Atomic `CONTROL` creation. PostgreSQL's `durable_rename` fsyncs the file, renames it, then fsyncs the file and its directory, so that a crash leaves "no mixed state or truncated files" (R-PG `src/backend/storage/file/fd.c:762-783`).
  - Leftovers. PostgreSQL removes leftover temporary files at postmaster startup (`RemovePgTempFiles`, R-PG `src/backend/storage/file/fd.c:3300-3321`, called at `src/backend/postmaster/postmaster.c:1332`).
  - Missing data file. PostgreSQL reports an error when the first segment of a relation is missing, and never creates one in its place (R-PG `src/backend/storage/smgr/md.c:665-697`).
  - Underflow. PostgreSQL deletes only empty pages: merging "might miss the items" in a concurrent scan; it never deletes the right-most page of a level, so height never decreases (R-PG `src/backend/access/nbtree/README:234-241, 247-253, 365-367`).
  - Reuse. PostgreSQL recycles a deleted page only when "there is no possible scan or search that has a reference to it" (R-PG `src/backend/access/nbtree/README:386-391`). LMDB reuses freed pages only below its oldest reader (R-LMDB `libraries/liblmdb/mdb.c:2640-2656, 2802-2812`). redb reuses a freed page "only after all read transactions that could reference it have completed" (R-REDB `docs/design.md:388-389`).
- **Rejected**:
  - A `wal-<g>/` directory (PostgreSQL `pg_wal`, R-PG `src/include/access/xlog_internal.h:148`; RocksDB `Env::CreateDir` and `CreateDirIfMissing`, R-ROCKS `include/rocksdb/env.h:360-365`, used for the WAL directory at `db/db_impl/db_impl_open.cc:2687`): [TS-03](interfaces.md#ts-03-storage-abstraction) would need `create_dir` and `remove_dir`, which every application storage would have to implement, and each segment would need a `sync_dir` of a second directory (user decision 2026-10-02).
  - Replacing `CONTROL` by rename at each checkpoint: it creates a file, which can hit ENOSPC outside any transaction.
  - Growing the file when a checkpoint or eviction first writes a page: ENOSPC would then hit background work and make the database unusable, which FR-REC-007 forbids.
  - One file per graph or per tree: more files to sync, rename and back up, for no requirement.
  - Tree roots or the free-list head in `CONTROL`: `CONTROL` is written only at checkpoints and is not logged, so a commit's change to them would not be atomic with the commit.
  - Moving roots (PostgreSQL metapage, LMDB, redb): the directory would change at every root split.
  - Counting one split per delete in the estimate (the worst case): the reserve would grow by up to (height + 1) pages per delete and stay in the file, which never shrinks, while deletes rarely split and an overrun is handled at apply.
  - Merging or redistributing underfull pages (LMDB below 25 % fill, R-LMDB `mdb.c:1136-1139, 10335-10348`; SQLite at less than one third full, R-SQLITE `src/btree.c:9175-9179`): moving entries across page boundaries can make a concurrent scan miss or repeat them.
  - Keeping only a value prefix inline (SQLite): two places to read one value, for no requirement.
  - Keys in overflow pages (SQLite index trees): inner pages and high keys would then need overflow reads during descent.

## TS-06 Graph layout

- **Decision**:
  - Each graph has three B+trees:
    - **Node tree**, keyed by `node_id`: label tokens and properties ([TS-21](values-and-types.md#ts-21-values-and-types)).
    - **Edge tree**, keyed by `edge_id`: source, target, type and properties.
    - **Adjacency tree**, keyed `(node_id, direction, edge_type, neighbour_id, edge_id)`, with two entries per edge (OUT at the source, IN at the target).
  - Every entry carries the **stamps** `created_ts` and `deleted_ts`.
  - **Compressed leaves from the start**, in every tree:
    - entries are sorted; every 16th entry is a restart point holding its full key;
    - in the adjacency tree, the other entries store their key fields as deltas from the previous key, with integers as varints (M-LAYOUT layout B);
    - in every other tree, whose keys are byte strings, the other entries store the length of the prefix shared with the previous key, the length of the rest of the key, and the rest; both lengths are varints;
    - `created_ts` is omitted once the entry is visible to every snapshot ("frozen"), and `deleted_ts` is omitted while the entry is live.
  - **A delete can split a leaf.** Removing an entry moves every later entry one position earlier, so each later restart point becomes a different entry, which then stores its full key instead of a suffix. The leaf can therefore grow; if it no longer fits, it splits as for a put.
  - **Stamp encoding.** The leaf format has no stamp fields until MVCC is built ([TS-14](transactions-and-recovery.md#ts-14-mvcc)); the stamp encoding is added with it. The tree directory and the free list ([TS-05](#ts-05-files-and-pages)) never carry stamps.
  - **Fill.** Bulk-built leaves are filled to 90 %. When the right-most leaf of any tree splits, the left page is left 90 % full. There is no per-tree setting.
  - **Concurrent readers** (Lehman and Yao's B-link tree):
    - every page holds a high key, the upper bound of its keys, and a link to its right sibling;
    - a reader holds at most one page latch at a time; when its key is above a page's high key, or the page is marked deleted ([TS-05](#ts-05-files-and-pages)), it follows the right link;
    - a scan copies the matching entries of one leaf under its latch, records that leaf's right link, releases the latch, and continues at the recorded link;
    - a scan keeps its position in the key space: the highest high key of the leaves it has passed. On each later leaf it takes only keys at or above that position (`scan_prefix`, `src/storage/btree/mod.rs:560-608`; `Copied::take`, `mod.rs:704-749`). A freed page gives its key space to its right sibling ([TS-05](#ts-05-files-and-pages) Underflow), so a leaf the scan reaches later can hold keys inserted behind it; without the position, the scan would return them out of order (FINDING 75 on rmp task #14);
    - the committer splits a page under its exclusive latch: it fills the new right page, links it, and only then latches the parent to insert the separator, holding the child latch until it has the parent latch.

    A key present and unchanged for a whole read operation is returned exactly once. Until MVCC is built, a reader may see a concurrent commit in part, and before that commit is durable or published. Stamps and snapshots remove both effects ([TS-14](transactions-and-recovery.md#ts-14-mvcc)).
  - **Expansion.** Expanding a node is one descent plus a sequential leaf scan of the `(node_id, direction, edge_type)` prefix.
- **Satisfies**: NFR-PERF-003, NFR-PERF-004, NFR-PERF-005, FR-STO-001, FR-STO-003, FR-TX-006, FR-TYPE-003, BR-TYPE-001, FR-CAT-001.
- **Evidence**: measured ([M-LAYOUT](../evidence/layout-bench/results.md)) on the reference hardware, 8 KiB pages, synthetic Zipf and uniform graphs. Layouts are ranked by NFR-PERF-004's criteria, in its order.
  - **Layouts measured.** A, fixed-width adjacency tree; B, the same tree with compressed leaves; C, per-node blocks; D, Neo4j-style record chains; F and G, CSR, fixed and compressed.
  - **First criterion: device I/O and space at 1–10 % cache.**
    - At 25 %, 10 % and 1 % cache, B read the fewest device bytes of the MVCC-capable layouts A–D in 60 of 60 cells. A read 1.18–9.85× more than B. Record chains read 9.5–4 567× more and node blocks 1.38–7.06× more.
    - B used 46–61 B/edge in total and 18–33 B/edge for adjacency; A used 130–162 B/edge, 2.6–2.8× more.
    - At 10 M nodes / 100 M edges, B has height 4 and 19.7 B/edge of adjacency. It bulk-loads at 1.88 M edges/s with a 1 GiB build cache. At 10 % cache a uniform 1-hop reads 6.7 KiB in 78.7 µs.
  - **Second criterion: CPU when fully resident.**
    - B needs 1.2–1.85× A's time per traversal; node blocks (C) need 0.20–0.51× B's.
    - A single-edge insert costs 3.8–4.7 µs against A's 2.3–2.8 µs, and a delete 4.6–5.0 µs against 2.4–2.6 µs.
    - B wins the first criterion, so the second does not change the choice.
  - **CSR (F, G).** It holds one snapshot, with no stamps and no in-place update, so it fails MVCC compatibility. At 1 % cache, G read as much as B or more in 16 of 20 cells.
  - **References.**
    - Neo4j record chains: node 15 B, relationship 34 B with pointers for both endpoints; identifiers are file offsets and are reused (R-NEO `record-storage-engine/.../NodeRecordFormat.java:32`, `RelationshipRecordFormat.java:31-35`).
    - Neo4j's newer block format keeps adjacency per node, with a B+tree sorted by type and direction for dense nodes (Operations Manual, "Store formats"; P-TX §L1.1).
    - Kuzu's columnar CSR allows one write transaction at a time (R-KUZU `src/transaction/transaction_manager.cpp:35-38`).
    - Memgraph keeps adjacency in memory (R-MG `src/storage/v2/vertex.hpp`).
    - Shared-prefix keys with a restart point every 16 entries: LevelDB blocks (R-LEVELDB `table/block_builder.cc:5-22`; `block_restart_interval = 16`, `include/leveldb/options.h:106`).
    - A delete that needs new pages: InnoDB reserves file space before a pessimistic delete "so that the node pointer updates will not fail because of lack of space" (R-INNO `btr/btr0cur.cc:4686-4697`, `btr_cur_pessimistic_delete`).
    - Right-most leaf split. PostgreSQL leaves the left page fillfactor full whenever the right-most leaf of any B-tree index splits, default 90 (R-PG `src/backend/access/nbtree/nbtsplitloc.c:95-100, 287-291`; `src/include/access/nbtree.h:201`). SQLite's `balance_quick` applies to every rowid table when a row is appended to the right-most leaf (R-SQLITE `src/btree.c:9217-9238`).
    - Concurrent readers. PostgreSQL implements Lehman and Yao's right links and high keys (R-PG `src/backend/access/nbtree/README:17-29`), holds one read latch at a time and couples latches only when moving right or up (`README:106-110`), and makes a scan follow the right link recorded when it read the page (`README:88-101`).
    - Scan position. PostgreSQL's scan relies on items never moving across pre-existing page boundaries (R-PG `src/backend/access/nbtree/README:95-98`), while its page deletion makes the deleted page's key space belong to its right sibling (`README:250-253`). FINDING 75 attributes PostgreSQL's protection against the keys this exposes to its MVCC snapshot; the cited lines do not state it (not confirmed). lpg has no snapshot until MVCC is built ([TS-14](transactions-and-recovery.md#ts-14-mvcc)). Measured with 16 concurrent copies (FINDING 75): before the fix, 39 of 48 runs failed, each with keys out of order; after it, 0 of 160; test `a_scan_skips_keys_inserted_behind_it_into_a_range_freed_pages_gave_away` (`src/storage/btree/tests.rs:581`) fails with the fix reverted.
  - **Limits of M-LAYOUT** ("Limits of the method"):
    - one host and one device; synthetic graphs without properties;
    - misses were read with `O_DIRECT` and no read-ahead, while TS-08 reads through the operating-system cache, so I/O under kernel read-ahead is unmeasured;
    - a single-threaded pool without latches, WAL or checksums. In lpg, page writers are the single committer under the commit latch ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol)), the index builder and the fast load; readers run concurrently with them. That concurrency was not measured;
    - the 100 M point covers B and G with bulk load only.
- **Crates**: indradb 5.0.0 and cozo 0.7.6 encode graphs as sorted key-value entries (MPL-2.0; 41 k and 115 k downloads): study only. No crate offers a paged graph layout usable here.
- **Rejected**:
  - Fixed-width leaves (A): 2.6–2.8× larger, and more device bytes in every cache-pressure cell.
  - Record chains (D): device bytes, about 5 existing records rewritten per insert, and both endpoints locked exclusively.
  - Per-node blocks (C): more device bytes and 2.2–2.8× larger; fastest only when fully resident.
  - CSR as the primary store: one snapshot; no MVCC; no concurrent writers.
  - Memgraph's in-memory adjacency: fails FR-STO-001.
  - Reserving the stamp encoding before MVCC: it fixes now a format whose use is not yet built (NFR-SIMP-001). If a release falls between the two, the change is a format migration ([TS-28](operations.md#ts-28-format-versioning-and-migration)).
  - A per-tree fill setting: no requirement needs one (NFR-SIMP-001).
  - Latch coupling on the way down (crabbing): a reader would hold a parent while it waits for a child, and the committer holds the child while it waits for the parent, which can deadlock (R-PG `README:106-110`).

## TS-07 Element identifiers

- **Decision**:
  - One 64-bit counter per database serves nodes and edges in every graph. It is monotonic, never reused, and kept below 2^63.
  - Its high-water mark is stored in each WAL commit frame and in `CONTROL` ([TS-05](#ts-05-files-and-pages)), and recovery takes the highest. The commit-timestamp, catalog-object and token counters follow the same rule.
  - The Bolt `element_id` is the decimal text of the identifier, and the legacy integer `id` field is the same value.
  - Backup, restore and migration copy identifiers unchanged. Import assigns new ones ([TS-26](operations.md#ts-26-backup-restore-import-and-export)).
- **Satisfies**: FR-TYPE-003, BR-TYPE-001, BR-TYPE-002, FR-IMP-005, FR-BOLT-001.
- **Evidence**: Neo4j reuses the identifiers of deleted elements ([S11] of the functional specification), which BR-TYPE-001 forbids. Bolt carries `element_id` from version 5.0 (D-SS).
- **Rejected**:
  - One counter per graph: the same value would identify elements in two graphs.
  - Identifiers equal to record positions (Neo4j): reuse would be needed to reclaim space.

## TS-08 Buffer pool and memory cap

- **Decision**:
  - **Cache slots.** Each cache slot is a `Box<[u8]>` of one page behind a `std::sync::RwLock` latch. The page table is sharded, and each cache slot has an atomic pin count and an atomic usage count.
  - **Eviction.** A clock sweep. Before a dirty cache slot is written, its page LSN must be durable in the WAL. Cache slots are written only by eviction and by checkpoints, always into reserved space ([TS-05](#ts-05-files-and-pages)).
  - **I/O.** Positional `read_at` and `write_at` through the storage trait ([TS-03](interfaces.md#ts-03-storage-abstraction)), using the operating-system cache. No mmap and no direct I/O.
  - **Memory accountant.** One per database, an atomic byte budget equal to the memory cap. The cap is a required argument of create and open; there is no default (FR-LIM-003).
    - Users that reserve from it: cache slots, operator state, commit batches and the page copies of a commit's apply step ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol)), private write sets with their before-image and write-key reservations, old versions, committed write-key sets, read sets, sort buffers, and algorithm projections.
    - When a reservation fails, the pool evicts clean cache slots, then dirty cache slots whose WAL is durable. If the reservation still fails, the statement that asked fails (FR-LIM-001, FR-LIM-005, FR-LIM-007).
    - **Page copies** ([TS-16](transactions-and-recovery.md#ts-16-commit-protocol) step 6): one page for each existing page the commit changes, held only while step 6 runs. Their number is not known in advance, so nothing is reserved up front: a refused reservation, for a copy or for a cache slot, fails that commit and undoes its changes.
    - The pool never shrinks below a minimum, provisionally 1 % of the cap, to be fixed by measurement.
    - **Outside the cap** (user decision 2026-10-02):
      - WAL group page images ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)): one per page changed by the open group and by the group being written, so at most two per cache slot, about twice the pool size;
      - the in-memory free-page set ([TS-05](#ts-05-files-and-pages)): 8 bytes per free page.
    - **Worst case.** One database's memory can therefore reach about 3× the cap: the cap itself, plus page images of up to twice the pool, which is at most the cap. This is arithmetic, not a measurement (user decision 2026-10-02); the real peak is to be measured ([unverified points](quality-and-testing.md#unverified-points)).
    - Backup, restore and migration each copy through one fixed 1 MiB buffer, outside the cap. Its size does not depend on the data, and restore and migration run with no open database, so no accountant exists for them ([TS-26](operations.md#ts-26-backup-restore-import-and-export), [TS-28](operations.md#ts-28-format-versioning-and-migration)).
  - **Giant graphs.** With the cache at 1–10 % of the data, B+tree inner pages (about 0.5 % of a tree in M-LAYOUT) stay resident under the clock sweep, so a typical expansion costs about one leaf miss (M-LAYOUT: 0.8–1.2 misses per uniform 1-hop at 100 M edges).
- **Satisfies**: FR-STO-001, FR-LIM-001, FR-LIM-003, FR-LIM-005, FR-LIM-006, FR-LIM-007, NFR-SAFE-001, NFR-PERF-005.
- **Evidence**:
  - Clock sweep: PostgreSQL (R-PG `src/backend/storage/buffer/README:175`) and Neo4j usage counts (R-NEO `io/.../impl/muninn/MuninnPageCache.java:983-984`).
  - DuckDB has one buffer pool that sets memory limits for every user of the pool (R-DUCK `buffer_pool.hpp:38-39`, from P-STO §Q3).
  - `memmap2::Mmap::map` is an `unsafe fn` (P-STO §Q3).
  - PostgreSQL uses the operating-system cache by default; direct I/O is the developer option `debug_io_direct`, empty by default (R-PG `doc/src/sgml/config.sgml:12891-12906`).
  - Copy buffers of fixed size: LMDB's environment copy uses `MDB_WBUF` = 1 MiB (R-LMDB `libraries/liblmdb/mdb.c:11140`); PostgreSQL's base backup uses `SINK_BUFFER_LENGTH` = max(32 KiB, page size) (R-PG `src/backend/backup/basebackup.c:62`).
- **Crates**: none provides a buffer pool usable under `#![forbid(unsafe_code)]`. `parking_lot` is not used; whether `std` latches are fast enough is to be measured (unverified).
- **Known consequence**: pages cached by the pool are also in the operating-system cache, outside the cap. If the NFR-PERF-005 measurements show this double caching costs a target, direct I/O with page-aligned cache slots (`#[repr(align(4096))]`, which needs no `unsafe`) is the measured alternative.
- **Rejected**:
  - mmap: needs `unsafe`, and the crate could not cap the operating-system cache.
  - A fixed split between cache and execution memory: FR-LIM-003 sets one cap for both.
  - A default cap derived from physical memory: FR-LIM-003 removed it, because several databases may share one process.
  - LRU lists: more bookkeeping under concurrency than a clock, and no requirement favours them.

## TS-12 Checksums, corruption and integrity check

- **Decision**:
  - CRC-32C protects every page, every WAL frame and each `CONTROL` slot. A page checksum is verified on every read from disk.
  - **Page number binding.** A page's CRC-32C is computed over its page number, as 8 little-endian bytes, followed by bytes 4 to 8 191 of the page. A valid image of page A read at page B's position therefore fails B's checksum: a misdirected write, or a stale page from another position, is detected. Such a read fails with corruption and sets the unusable latch, like any checksum failure (below). The WAL frame CRC-32C already covers the frame's page number ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
  - **Limit.** The checksum has 32 bits and a data file can hold up to 2<sup>50</sup> − 1 pages ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 4), so some pairs of page numbers give the same checksum for the same content. A misplaced page is therefore not always detected. Which pairs collide was not computed (unverified).
  - A checksum failure, or a failed write or sync other than a reservation ENOSPC ([TS-05](#ts-05-files-and-pages), [TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)), sets a database-wide "unusable" latch while the database is open. The failing operation and every later one fail until the database is reopened.
  - At open, `CONTROL` and the WAL from the redo point are verified ([TS-13](transactions-and-recovery.md#ts-13-recovery)).
  - The on-demand integrity check reads every page and checks:
    - its checksum;
    - the B+tree invariants: key order, separators, sibling links, free-list membership;
    - the consistency between trees: adjacency against the edge tree, each index against the data.
- **Satisfies**: FR-REC-003, FR-REC-005, FR-REC-006, FR-REC-007, FR-REC-008, FR-REC-010.
- **Evidence**:
  - CRC-32C is used by the PostgreSQL WAL (R-PG `src/include/access/xlogrecord.h:49`), InnoDB pages, and Neo4j log entries (R-NEO `community/wal/.../CommitLogEntrySerializerV4_2.java:67`). redb uses XXH3-128 (P-STO §Q6).
  - Neo4j ships a consistency checker (R-NEO `community/consistency-check`).
  - Page number binding. PostgreSQL mixes the block number into every page checksum "to detect the case where a page is somehow moved to a different location" (R-PG `src/include/storage/checksum_impl.h:167-169, 172, 193`). InnoDB stores the page number in the page header (`FIL_PAGE_OFFSET`, R-INNO `include/fil0types.h:44-46`) and stops when a page it reads holds another number (`buf/buf0buf.cc:5800-5821`). Turso passes the page number to its checksum functions but does not use it (R-TURSO `core/storage/checksum.rs:35, 81-83`); neither did lpg before task #23 (`src/storage/page.rs:8, 79-81`) ([P-TEST](../evidence/research/research-04-turso-libsql.md) §2, R9). The page format changes before it is frozen (user decision 2026-10-03, rmp task #23).
- **Crates**:
  - `crc32c` 0.6.8: dependency. Apache-2.0/MIT, 85.9 M downloads, no runtime dependencies, repository active in 2026-02.
  - `crc-fast`: fallback.
  - `crc32fast`: rejected, it computes CRC-32/IEEE, not CRC-32C.
  - `xxhash-rust`: rejected, its BSL-1.0 licence is not used elsewhere.
- **Rejected**:
  - Checksums verified only by repair or by a check (redb), which fails FR-REC-005.
  - No page number binding (Turso): a valid page at the wrong position passes its checksum (FR-REC-005, FR-REC-008).
  - XOR of the page number into the checksum (PostgreSQL, whose block numbers have 32 bits): lpg page numbers have 64 bits, so only 32 of them could be mixed in, and pages p and p + 2<sup>32</sup> would always share a checksum.
  - Seeding the CRC-32C with the page number: the seed of `crc32c_append` is a `u32` (`crc32c` 0.6.8 `src/lib.rs:49`), the same truncation.
  - Storing the page number in the header and comparing it on read (InnoDB): 8 more header bytes per page for what the checksum binds at no cost in space.

## TS-27 Single-process lock

- **Decision**:
  - Open first checks that `CONTROL` exists; if not, it fails without creating any file (FR-DB-004).
  - If `CONTROL` exists and `LOCK` is missing, open fails with the error "`LOCK` missing", distinct from corruption, and creates no file (user decision 2026-10-02). Create, restore and migration always leave `LOCK` in place ([TS-05](#ts-05-files-and-pages), [TS-26](operations.md#ts-26-backup-restore-import-and-export), [TS-28](operations.md#ts-28-format-versioning-and-migration)), so it is missing only after an external deletion, or when the location is a backup artefact ([TS-26](operations.md#ts-26-backup-restore-import-and-export)). Recreating it would let a second handle open while the first still holds the lock on the deleted file (FR-DB-006, FR-DB-007).
  - Create and open then lock `LOCK` with `File::try_lock` through `Storage::try_lock`. A second open from another process, or from the same process, gets `WouldBlock` and fails.
  - The lock is released when the last clone of the handle is closed or dropped ([TS-02](architecture.md#ts-02-async-api-and-threading)).
- **Satisfies**: FR-DB-004, FR-DB-006, FR-DB-007, FR-STO-002, FR-STO-004.
- **Evidence**:
  - `File::try_lock` was stabilised in Rust 1.89.0 (D-RUST); on Linux it uses `flock` (P-STO §Q8).
  - Measured ([M-LOCK](../evidence/locktest/main.rs)): a second descriptor in the same process gets `WouldBlock`, and another process is refused.
  - fjall locks its database the same way (R-FJALL `src/locked_file.rs`).
- **Crates**: `fs2` (last release 2018), `fs4` and `fd-lock` are not needed since Rust 1.89.
- **Rejected**:
  - `fcntl` record locks: per process, so they cannot detect a second open in the same process.
  - PID files: they need stale-lock detection.
  - Recreating a missing `LOCK` and accepting the risk (LMDB opens its lock file with `O_CREAT`, R-LMDB `libraries/liblmdb/mdb.c:5339, 5346, 5417`; `mdb_env_setup_locks`, `5957`): two handles can then hold the database at once.
  - Recreating a missing `LOCK` and rechecking it periodically (PostgreSQL creates `postmaster.pid` with `O_CREAT | O_EXCL` at start, R-PG `src/backend/utils/init/miscinit.c:1226`; once a minute it reopens the file, and a missing file or a foreign PID forces an immediate shutdown, `miscinit.c:1649-1707`, `src/backend/postmaster/postmaster.c:1794-1813`): a background task, and two handles coexist until the next recheck, which then stops the first one.
