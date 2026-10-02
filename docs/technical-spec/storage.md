---
title: Storage
status: draft
last-reviewed: 2026-10-02
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
    - `data-<g>`, one paged file whose page 0 holds the magic and the format version;
    - `wal-<g>/`, the WAL segments ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).

    `<g>` is the file-set generation, starting at 1. Only migration changes it ([TS-28](operations.md#ts-28-format-versioning-and-migration)). At open, files of any other generation are removed.
  - **`CONTROL`** has two 4 KiB slots, written alternately in place with `write_at` and then `sync_data`. Each slot holds:
    - a sequence number, magic, format version, page size, database UUID and active generation `<g>`;
    - a clean-shutdown flag and the checkpoint redo point;
    - the counters: highest commit timestamp, next element identifier, next catalog object identifier, next token identifier;
    - a CRC-32C.

    The valid slot with the higher sequence number is authoritative. Page 0 of `data-<g>` must hold the same format version; a mismatch is corruption.
  - **Pages.** 8 KiB, fixed at creation. Page header: CRC-32C, page LSN, page type, tree identifier. Leaf and inner pages are slotted. Values above an inline threshold go to chains of overflow pages.
  - **Free space.**
    - Free pages are kept in a persisted free list plus an in-memory set. A page freed by a commit is reused only after the garbage-collection horizon has passed that commit ([TS-14](transactions-and-recovery.md#ts-14-mvcc)).
    - `data-<g>` grows in extents of 64 MiB, or 1 % of the file if that is larger. Each extent is reserved with `fallocate` before use; this extension is the only one.
    - Before taking the commit latch, a committing transaction makes sure the reserve of unused pages covers its bound: 2 × (⌈write-set bytes / 4 KiB⌉ + trees written) + the sum of the heights of the trees written. Each leaf split creates at most one page per level, and splits leave pages at least half full.
    - If a reservation hits ENOSPC, that transaction fails and reads continue (FR-REC-007).
    - Checkpoints and eviction write only into reserved space.
  - **Create** fails, leaving everything unchanged, if `CONTROL` already exists (FR-DB-003). Otherwise it creates and locks `LOCK`, creates `data-1` and `wal-1/`, and writes `CONTROL` last, then `sync_dir`. A location is a database exactly when `CONTROL` exists.
- **Satisfies**: FR-STO-001, FR-DB-003, FR-DB-004, FR-REC-003, FR-REC-007, FR-REC-008, FR-OPS-004, FR-OPS-011, FR-TYPE-003.
- **Evidence**:
  - Default page sizes: PostgreSQL 8 KiB; Neo4j `PAGE_SIZE = 8192` (R-NEO `community/io/.../pagecache/PageCache.java:51`); InnoDB 16 KiB; SQLite and Kuzu 4 KiB (R-KUZU `CMakeLists.txt:114`). M-LAYOUT ran on 8 KiB pages.
  - Two checksummed commit slots in the file header: redb (`docs/design.md` l.30-60, [P-STO](../evidence/research/research-01-storage.md) §Q9). PostgreSQL keeps `pg_control` within 512 bytes (P-STO §Q9).
  - PostgreSQL extends relation files with `posix_fallocate` in the session that needs the space, and reports ENOSPC there with "Check free disk space" (R-PG `src/backend/storage/smgr/md.c:529-536, 596-629`; `src/backend/storage/file/fd.c:2396-2425`).
  - SQLite uses overflow pages for large cells.
- **Rejected**:
  - Replacing `CONTROL` by rename at each checkpoint: it creates a file, which can hit ENOSPC outside any transaction.
  - Growing the file when a checkpoint or eviction first writes a page: ENOSPC would then hit background work and make the database unusable, which FR-REC-007 forbids.
  - One file per graph or per tree: more files to sync, rename and back up, for no requirement.

## TS-06 Graph layout

- **Decision**:
  - Each graph has three B+trees:
    - **Node tree**, keyed by `node_id`: label tokens and properties ([TS-21](values-and-types.md#ts-21-values-and-types)).
    - **Edge tree**, keyed by `edge_id`: source, target, type and properties.
    - **Adjacency tree**, keyed `(node_id, direction, edge_type, neighbour_id, edge_id)`, with two entries per edge (OUT at the source, IN at the target).
  - Every entry carries the **stamps** `created_ts` and `deleted_ts`.
  - **Compressed leaves from the start**, in every tree:
    - entries are sorted; every 16th entry is a restart point holding its full key;
    - the other entries store their key as a delta from the previous key, with integers as varints;
    - `created_ts` is omitted once the entry is visible to every snapshot ("frozen"), and `deleted_ts` is omitted while the entry is live.
  - **Fill.** Bulk-built leaves are filled to 90 %. On the right-most leaf of the node and edge trees, which only ever grow at the right, splits leave the left page 90 % full.
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
  - **Frames.** Each frame is a `Box<[u8]>` of one page behind a `std::sync::RwLock` latch. The page table is sharded, and each frame has an atomic pin count and an atomic usage count.
  - **Eviction.** A clock sweep. Before a dirty frame is written, its page LSN must be durable in the WAL. Frames are written only by eviction and by checkpoints, always into reserved space ([TS-05](#ts-05-files-and-pages)).
  - **I/O.** Positional `read_at` and `write_at` through the storage trait ([TS-03](interfaces.md#ts-03-storage-abstraction)), using the operating-system cache. No mmap and no direct I/O.
  - **Memory accountant.** One per database, an atomic byte budget equal to the memory cap. The cap is a required argument of create and open; there is no default (FR-LIM-003).
    - Users that reserve from it: frames, operator state, private write sets with their before-image and write-key reservations, old versions, committed write-key sets, read sets, sort buffers, and algorithm projections.
    - When a reservation fails, the pool evicts clean frames, then dirty frames whose WAL is durable. If the reservation still fails, the statement that asked fails (FR-LIM-001, FR-LIM-005, FR-LIM-007).
    - The pool never shrinks below a minimum, provisionally 1 % of the cap, to be fixed by measurement.
  - **Giant graphs.** With the cache at 1–10 % of the data, B+tree inner pages (about 0.5 % of a tree in M-LAYOUT) stay resident under the clock sweep, so a typical expansion costs about one leaf miss (M-LAYOUT: 0.8–1.2 misses per uniform 1-hop at 100 M edges).
- **Satisfies**: FR-STO-001, FR-LIM-001, FR-LIM-003, FR-LIM-005, FR-LIM-006, FR-LIM-007, NFR-SAFE-001, NFR-PERF-005.
- **Evidence**:
  - Clock sweep: PostgreSQL (R-PG `src/backend/storage/buffer/README:175`) and Neo4j usage counts (R-NEO `io/.../impl/muninn/MuninnPageCache.java:983-984`).
  - DuckDB has one buffer pool that sets memory limits for every user of the pool (R-DUCK `buffer_pool.hpp:38-39`, from P-STO §Q3).
  - `memmap2::Mmap::map` is an `unsafe fn` (P-STO §Q3).
  - PostgreSQL uses the operating-system cache by default; direct I/O is the developer option `debug_io_direct`, empty by default (R-PG `doc/src/sgml/config.sgml:12891-12906`).
- **Crates**: none provides a buffer pool usable under `#![forbid(unsafe_code)]`. `parking_lot` is not used; whether `std` latches are fast enough is to be measured (unverified).
- **Known consequence**: pages cached by the pool are also in the operating-system cache, outside the cap. If the NFR-PERF-005 measurements show this double caching costs a target, direct I/O with page-aligned frames (`#[repr(align(4096))]`, which needs no `unsafe`) is the measured alternative.
- **Rejected**:
  - mmap: needs `unsafe`, and the crate could not cap the operating-system cache.
  - A fixed split between cache and execution memory: FR-LIM-003 sets one cap for both.
  - A default cap derived from physical memory: FR-LIM-003 removed it, because several databases may share one process.
  - LRU lists: more bookkeeping under concurrency than a clock, and no requirement favours them.

## TS-12 Checksums, corruption and integrity check

- **Decision**:
  - CRC-32C protects every page, every WAL frame and each `CONTROL` slot. A page checksum is verified on every read from disk.
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
- **Crates**:
  - `crc32c` 0.6.8: dependency. Apache-2.0/MIT, 85.9 M downloads, no runtime dependencies, repository active in 2026-02.
  - `crc-fast`: fallback.
  - `crc32fast`: rejected, it computes CRC-32/IEEE, not CRC-32C.
  - `xxhash-rust`: rejected, its BSL-1.0 licence is not used elsewhere.
- **Rejected**: checksums verified only by repair or by a check (redb), which fails FR-REC-005.

## TS-27 Single-process lock

- **Decision**:
  - Open first checks that `CONTROL` exists; if not, it fails without creating any file (FR-DB-004).
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
