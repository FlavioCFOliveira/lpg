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

Graphs far larger than memory are a primary target: efficiency is judged with the cache holding 1 % to 10 % of the data on the reference hardware ([NFR-PERF-005](../functional-spec/non-functional-requirements.md#performance), [NFR-PERF-006](../functional-spec/non-functional-requirements.md#performance)). Every decision below is checked against that case.

## TS-04 Own storage engine

- **Decision**: lpg implements its own page file, buffer pool, B+tree, WAL, checkpoint and recovery, on `std` only.
- **Satisfies**: FR-STO-001..003, FR-STO-002, FR-TX-002, FR-REC-002, FR-REC-005, FR-REC-007, FR-REC-008, FR-LIM-003, FR-OPS-001, NFR-SAFE-001, NFR-PERF-003.
- **Evidence**: assessment of each embedded store as a dependency (P-STO §Q1, with file and line references):

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
  - A database location holds these files:
    - `LOCK`: lock target ([TS-27](#ts-27-single-process-lock)).
    - `CONTROL`: magic, format version, page size, database UUID, checkpoint redo point, next element identifier, CRC-32C. It is at most 512 bytes and is replaced by write to a temporary file, `sync_data`, `rename`, then `sync_dir`.
    - `data`: one paged file. Page 0 holds the magic and the format version.
    - `wal/`: WAL segments ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).
  - Pages are 8 KiB, fixed at creation. Page header: CRC-32C, page LSN, page type, tree identifier.
  - Leaf and inner pages are slotted. Values above an inline threshold go to chains of overflow pages.
  - Free pages are kept in a persisted free list plus an in-memory set. A page freed by a commit is reused only after the garbage-collection horizon has passed that commit ([TS-14](transactions-and-recovery.md#ts-14-mvcc)).
- **Satisfies**: FR-STO-001, FR-REC-003, FR-REC-008, FR-OPS-004, FR-TYPE-003.
- **Evidence**:
  - Default page sizes: PostgreSQL 8 KiB; Neo4j `PAGE_SIZE = 8192` (R-NEO `community/io/.../pagecache/PageCache.java:51`); InnoDB 16 KiB; SQLite and Kuzu 4 KiB (R-KUZU `CMakeLists.txt:114`).
  - PostgreSQL keeps `pg_control` within 512 bytes so that its write is atomic (P-STO §Q9).
  - SQLite uses overflow pages for large cells.
  - M-LAYOUT ran all layouts on 8 KiB pages.
- **Rejected**:
  - One file per graph or per tree: more files to sync, rename and back up, for no requirement.
  - `CONTROL` stored as a page of `data`: atomic replacement is simpler as a small separate file.

## TS-06 Graph layout

- **Decision**:
  - Each graph has three B+trees:
    - **Node tree**, keyed by `node_id`: label tokens and properties ([TS-21](values-and-types.md#ts-21-values-and-types)).
    - **Edge tree**, keyed by `edge_id`: source, target, type and properties.
    - **Adjacency tree**, keyed `(node_id, direction, edge_type, neighbour_id, edge_id)`, with two entries per edge (OUT at the source, IN at the target). Each entry carries the stamps `created_ts` and `deleted_ts`.
  - **Compressed leaves from the start**, in every tree:
    - entries are sorted; every 16th entry is a restart point holding its full key;
    - the other entries store their key as a delta from the previous key, with integers as varints;
    - `created_ts` is omitted once the entry is visible to every snapshot ("frozen"), and `deleted_ts` is omitted while the entry is live.
  - Bulk-built leaves are filled to 90 %. On the right-most leaf of the node and edge trees, which only ever grow at the right, splits leave the left page 90 % full.
  - Expanding a node is one descent plus a sequential leaf scan of the `(node_id, direction, edge_type)` prefix.
  - A compressed CSR copy is recorded only as a possible future read-only projection. It is not specified.
- **Satisfies**: NFR-PERF-003, NFR-PERF-004, NFR-PERF-005, FR-STO-001, FR-STO-003, FR-TX-006, FR-TYPE-003, BR-TYPE-001, FR-CAT-001.
- **Evidence** (measured, M-LAYOUT; reference hardware, 8 KiB pages, synthetic Zipf and uniform graphs):
  - **I/O.** Layouts measured: A, fixed-width adjacency tree; B, the same tree with compressed leaves; C, per-node blocks; D, Neo4j-style record chains; F and G, CSR, fixed and compressed. At 25 %, 10 % and 1 % cache, B read the fewest device bytes of the MVCC-capable layouts A–D in 60 of 60 cells. A read 1.18–9.85× more than B. Record chains read 9.5–4 567× more and node blocks 1.38–7.06× more.
  - **Space.** B used 46–61 B/edge in total and 18–33 B/edge for adjacency. A used 130–162 B/edge, 2.6–2.8× more.
  - **Cost.** With every page in memory, B needs 1.2–1.85× A's CPU time per traversal. A single-edge insert costs 3.8–4.7 µs against 2.3–2.8 µs, and a delete 4.6–5.0 µs against 2.4–2.6 µs. On this NVMe one device miss cost 38–109 µs.
  - **Scale.** At 10 M nodes / 100 M edges, B has height 4 and 19.7 B/edge of adjacency. It bulk-loads at 1.88 M edges/s using a 1 GiB build cache. At 10 % cache a uniform 1-hop reads 6.7 KiB in 78.7 µs.
  - **CSR.** Compressed CSR (G) read fewer bytes than B at 25 % and 10 % cache. At 1 % cache it read as much or more in 16 of 20 cells, because of its per-node offset array. It holds one snapshot, with no stamps and no in-place update.
  - **References.** Neo4j record chains: node 15 B, relationship 34 B with pointers for both endpoints; identifiers are file offsets and are reused (R-NEO `record-storage-engine/.../NodeRecordFormat.java:32`, `RelationshipRecordFormat.java:31-35`). Neo4j's newer block format keeps adjacency per node, with a B+tree sorted by type and direction for dense nodes (Operations Manual, "Store formats"; P-TX §L1.1). Kuzu's columnar CSR allows one write transaction at a time (R-KUZU `src/transaction/transaction_manager.cpp:35-38`). Memgraph keeps adjacency in memory (R-MG `src/storage/v2/vertex.hpp`).
  - **Limits** (M-LAYOUT "Limits of the method"): one host and device; synthetic graphs without properties; single-threaded pool without latches, WAL or checksums; no prefetch. The 100 M point covers B and G with bulk load only. Concurrent writers on compressed leaves were not measured.
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
  - Each WAL commit frame records the counter's high-water mark; recovery takes the highest. `CONTROL` stores it at each checkpoint.
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
  - **Eviction.** A clock sweep. Before a dirty frame is written, its page LSN must be durable in the WAL. Frames are written only by eviction and by checkpoints.
  - **I/O.** Positional `read_at` and `write_at` through the storage trait ([TS-03](interfaces.md#ts-03-storage-abstraction)), using the operating-system cache. No mmap and no direct I/O.
  - **Memory accountant.** One per database, an atomic byte budget equal to the memory cap. Frames, operator state, private write sets, old versions, read sets, index-population queues and algorithm projections all reserve from it.
    - When a reservation fails, the pool evicts clean frames, then dirty frames whose WAL is durable.
    - If the reservation still fails, the statement that asked fails (FR-LIM-001). For old versions this is FR-LIM-005: no other transaction is aborted and nothing is spilled.
    - The pool never shrinks below a minimum set by measurement; until it is measured, 1 % of the cap.
  - **Default cap.** 100 % of `MemTotal` when `SwapTotal` > 0, otherwise 90 %, both read from `/proc/meminfo`.
  - **Giant graphs.** With the cache at 1–10 % of the data, B+tree inner pages (about 0.5 % of a tree in M-LAYOUT) stay resident under the clock sweep, so a typical expansion costs about one leaf miss (M-LAYOUT: 0.8–1.2 misses per uniform 1-hop at 100 M edges).
- **Satisfies**: FR-STO-001, FR-LIM-001, FR-LIM-003, FR-LIM-005, FR-LIM-006, NFR-SAFE-001, NFR-PERF-005.
- **Evidence**:
  - Clock sweep: PostgreSQL (R-PG `src/backend/storage/buffer/README:175`) and Neo4j usage counts (R-NEO `io/.../impl/muninn/MuninnPageCache.java:983-984`).
  - DuckDB has one buffer pool that sets memory limits for every user of the pool (R-DUCK `buffer_pool.hpp:38-39`, from P-STO §Q3).
  - `memmap2::Mmap::map` is an `unsafe fn` (P-STO §Q3).
  - PostgreSQL uses the operating-system cache by default; direct I/O is the developer option `debug_io_direct`, empty by default (R-PG `doc/src/sgml/config.sgml:12891-12906`).
- **Crates**: none provides a buffer pool usable under `#![forbid(unsafe_code)]`. `parking_lot` is not used; whether `std` latches are fast enough is to be measured (unverified).
- **Known consequence**: pages cached by the pool are also in the operating-system cache, and that memory is outside the cap. With the default cap (90–100 % of RAM) that cache is small. If the NFR-PERF-005 measurements show the double caching costs a target, direct I/O with page-aligned frames (`#[repr(align(4096))]`, which needs no `unsafe`) is the measured alternative.
- **Rejected**:
  - mmap: needs `unsafe`, and the crate could not cap the operating-system cache.
  - A fixed split between cache and execution memory: FR-LIM-003 sets one cap for both.
  - LRU lists: more bookkeeping under concurrency than a clock, and no requirement favours them.

## TS-12 Checksums, corruption and integrity check

- **Decision**:
  - CRC-32C protects every page, every WAL frame and `CONTROL`. A page checksum is verified on every read from disk.
  - A checksum or flush failure while the database is open sets a database-wide "unusable" latch: the failing operation and every later one fail until the database is reopened.
  - At open, `CONTROL` and the WAL from the redo point are verified ([TS-13](transactions-and-recovery.md#ts-13-recovery)).
  - The on-demand integrity check reads every page and checks its checksum, the B+tree invariants (key order, separators, sibling links, free-list membership) and the consistency between trees (adjacency against the edge tree, each index against the data).
- **Satisfies**: FR-REC-003, FR-REC-005, FR-REC-006, FR-REC-008, FR-REC-010.
- **Evidence**:
  - CRC-32C is used by the PostgreSQL WAL (R-PG `src/include/access/xlogrecord.h:49`), InnoDB pages, and Neo4j log entries (R-NEO `community/wal/.../CommitLogEntrySerializerV4_2.java:67`). redb uses XXH3-128 (P-STO §Q6).
  - Neo4j ships a consistency checker (R-NEO `community/consistency-check`).
- **Crates**:
  - `crc32c` 0.6.8: dependency. Apache-2.0/MIT, 85.9 M downloads, no runtime dependencies, repository active in 2026-02.
  - `crc-fast`: fallback (it depends on `spin`).
  - `crc32fast`: rejected, it computes CRC-32/IEEE, not CRC-32C.
  - `xxhash-rust`: rejected, its BSL-1.0 licence is not used elsewhere in the dependency set.
- **Rejected**: checksums verified only by repair or by a check (redb), which fails FR-REC-005.

## TS-27 Single-process lock

- **Decision**: on open and create, the crate locks `LOCK` with `File::try_lock` through `Storage::try_lock`. A second open from another process, or from the same process, gets `WouldBlock` and the open fails. The lock is released when the handle closes or is dropped.
- **Satisfies**: FR-DB-006, FR-DB-007, FR-STO-004, FR-STO-002.
- **Evidence**:
  - `File::try_lock` was stabilised in Rust 1.89.0 (D-RUST); on Linux it uses `flock` (P-STO §Q8).
  - Measured (M-LOCK): a second descriptor in the same process gets `WouldBlock`, and another process is refused.
  - fjall locks its database the same way (R-FJALL `src/locked_file.rs`).
- **Crates**: `fs2` (last release 2018), `fs4` and `fd-lock` are not needed since Rust 1.89.
- **Rejected**:
  - `fcntl` record locks: per process, so they cannot detect a second open in the same process.
  - PID files: they need stale-lock detection.
