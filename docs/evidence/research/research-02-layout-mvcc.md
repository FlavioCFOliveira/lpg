# Research 02: graph layout, MVCC, serializability, locks and indexes for `lpg`

Date: 2026-09-24. Research only; no repository file was modified.
Builds on `research-01-storage.md` (own page-based engine, 8 KiB pages, clock-sweep buffer manager, no mmap, NO-STEAL, WAL with full page images at commit, redo-only recovery, group commit, CRC-32C).

## 0. Decision criteria and requirements

Binding criterion (user): the layout must be (a) fully compatible with MVCC, (b) fast for graph traversals, (c) least costly in CPU, memory, I/O and space. User clarification (2026-09-24): performance and resource efficiency come first. Index-free adjacency must be evaluated seriously. If a specification constraint excludes the fastest layout, that trade-off must be reported, not dropped silently.

Requirements read at `140e230`: FR-TX-003..008 and BR-TX-001 (serializable by default, SI option, reads never block writes and writes never block reads, writers wait on write locks, deadlock aborts the requester), FR-STO-001/003 (larger than memory, concurrent writers), FR-LIM-001/003 (memory cap covers cache and execution), FR-CAT-001/004 (several graphs, transactional catalog), FR-TYPE-003 and BR-TYPE-001/002 (stable, never reused identifiers), FR-IDX-001..005 (five index kinds, four constraint kinds, online build), FR-ALG-003/004 (algorithms read the caller's snapshot; write-back in the caller's transaction), NFR-SAFE-001 (`forbid(unsafe_code)`), NFR-SIMP-001.

## 1. Sources

Local clones (shallow, commit, commit date):

| Repository | Path | Commit |
|---|---|---|
| memgraph/memgraph (BSL-1.1, read for study only) | `/tmp/memgraph` | `33813d7` (2026-09-24) |
| kuzudb/kuzu (MIT; **archived** on GitHub, last push 2025-10-10, per `gh api repos/kuzudb/kuzu` and README) | `/tmp/kuzu` | `89f0263` (2025-10-10) |
| neo4j/neo4j (GPLv3 community) | `/tmp/neo4j` | `54a7dcf` (2026-09-14) |
| postgres/postgres | `/tmp/postgres` | `8982935` (2026-09-24) |
| duckdb, mysql-server, sqlite | as in research-01 | |

Papers (PDFs downloaded to the scratchpad and read with `pdftotext`):

- [HYPER] T. Neumann, T. Mühlbauer, A. Kemper. *Fast Serializable Multi-Version Concurrency Control for Main-Memory Database Systems*. SIGMOD 2015. `db.in.tum.de/~muehlbau/papers/mvcc.pdf`.
- [WU] Y. Wu, J. Arulraj, J. Lin, R. Xian, A. Pavlo. *An Empirical Evaluation of In-Memory Multi-Version Concurrency Control*. PVLDB 10(7), 2017.
- [KUZU] X. Feng, G. Jin, Z. Chen, C. Liu, S. Salihoğlu. *KÙZU Graph Database Management System*. CIDR 2023.
- [GUPTA] P. Gupta, A. Mhedhbi, S. Salihoglu. *Columnar Storage and List-based Processing for Graph Database Management Systems*. PVLDB 14(11), 2021.
- [LIVE] X. Zhu et al. *LiveGraph: A Transactional Graph Storage System with Purely Sequential Adjacency List Scans*. PVLDB 13(7), 2020 (arXiv 1910.05773).
- [PG-SSI] PostgreSQL `src/backend/storage/lmgr/README-SSI` (it cites Cahill et al. and Ports & Grittner). I did **not** re-read the Cahill and Ports & Grittner papers themselves.

Official documentation (fetched 2026-09-24): Neo4j Operations Manual "Store formats", "Space reuse", "Concurrent data access"; Memgraph "Storage memory usage"; MySQL 8.4 "Undo Logs" and "InnoDB Multi-Versioning"; PostgreSQL 17 release notes; DuckDB "Concurrency".

Own measurement: page-touch simulation, `scratchpad/layout-sim/pagesim.py`, results in `layout-sim/results.txt` and `results-bt18.txt` (section L1.3).

---

## L1. Graph layout

### L1.1 Candidates and evidence

**A. Neo4j record format ("index-free adjacency", aligned/standard)**
- Fixed-size records addressed by id: node 15 B, relationship 34 B, relationship group 25 B (`community/record-storage-engine/.../format/standard/NodeRecordFormat.java:32`, `RelationshipRecordFormat.java:35`, `RelationshipGroupRecordFormat.java:38`). The relationship record holds both endpoints plus prev/next pointers for **both** endpoint chains (layout comment, `RelationshipRecordFormat.java:31-35`).
- Nodes with more than 50 relationships become "dense". They get per-type group records with separate chains per direction (`GraphDatabaseSettings.java:797`, `db.relationship_grouping_threshold` default 50, "Not applicable for the block format").
- An insert modifies existing records. The new relationship becomes first in chain: the old first record's prev pointer and first-in-chain flag, and the node or group head pointer, are rewritten, for each endpoint (`RelationshipCreator.java:329-350`).
- Locking: for a sparse node the insert takes an **exclusive NODE lock on both endpoints** (`RelationshipModifier.java:141`). For a dense node it takes a shared RELATIONSHIP_GROUP lock plus relationship locks (`:160`). The Operations Manual confirms this: "Sparse nodes: write locks acquired on the specific relationship and both connected nodes".
- Isolation: Neo4j is read-committed ("A transaction that reads a node/relationship does not block another transaction from writing to that node/relationship", Concurrent data access page). The page does not mention MVCC.
- Identifiers are physical offsets: "the ID represents the element's physical location in the store file" (Store formats page). Deleted IDs are reused once all overlapping transactions finish (Space reuse page).

**B. Neo4j block format (verified)**
- Exists: Enterprise Edition only. Introduced in 5.14.0 and the default since 5.22.0. The aligned format stays the Community default. Standard and high_limit are deprecated from 5.23 (Store formats page).
- Design, per the Store formats page: `block.x1.db` holds a 128 B static block per node at "offset = 128 B * nodeId". The block is two 64 B records: node data (labels, properties) and relationship data ("the relationships and relationship data connected to this node"). Overflow goes to dynamic stores (relationship dynamic record ≤ 2047 B, "hundreds of relationships"). Dense nodes go to `block.relationship.dense.db`, a B+tree "sorted by relationship type and direction for any given node". The stated purpose: "enhance data locality ... drastically reducing the amount of pointer chasing".
- Finding: Neo4j's newest format **dropped per-relationship linked chains** in favour of adjacency co-located per node, with a per-node sorted B+tree for high degree. It still uses offset-addressed ids with reuse. Its source is not public (Enterprise), so the internals beyond the manual are **UNVERIFIED**. Whether a relationship is stored at both endpoints is also UNVERIFIED (the manual summary is ambiguous).

**C. Kuzu (columnar node groups + CSR)**
- Paper: "Kùzu is a columnar system similar to modern read-optimized analytical DBMSs". Node properties are in column files. Edges are "double indexed and stored in CSR-based adjacency list indices". "Edge properties are similarly stored in 'parallel' but separate CSR-based structures and double-indexed ... This has storage and update costs yet ensures that we can scan any node's edges and properties of these edges sequentially in both forward and backward directions". 4 KB pages, GClock ([KUZU] §2). Source confirms per-direction `RelTableData` with a full column set each (`src/storage/table/rel_table_data.cpp:89-104`). Packed-CSR density constants are at `include/common/constants.h:78-79`.
- Transactions: "serializable as we currently allow one writer transaction" ([KUZU] §2). Source: `transaction_manager.cpp:35-38` throws "Only one write transaction at a time is allowed in the system." unless the `debug_enable_multi_writes` setting is on (`include/main/settings.h:100`).
- Uncommitted inserts are kept in a transaction-local `LocalStorage` and merged at commit (`src/storage/local_storage/local_storage.cpp:58-78`). Updates use in-memory `UndoBuffer`/`UpdateInfo` version chains.
- Project status: archived 2025-10-10.

**D. Memgraph in-memory**
- `Vertex` holds `labels`, `in_edges`, `out_edges` as `small_vector<tuple<EdgeTypeId, Vertex*, EdgeRef>>`, a `PropertyStore`, a spin lock and a delta pointer (`src/storage/v2/vertex.hpp:28-66`). This is per-vertex adjacency in RAM, with raw pointers.
- MVCC: writes are applied in place, and before-image deltas form a newest-to-oldest chain (`delta.hpp`, `delta_action.hpp`: `ADD_OUT_EDGE`, `REMOVE_IN_EDGE`, `SET_PROPERTY`, ...).
- Write-write conflict: `PrepareForWrite` returns a serialization error at once; there is no waiting (`mvcc.hpp:111-137`).
- Isolation levels: `SNAPSHOT_ISOLATION, READ_COMMITTED, READ_UNCOMMITTED` (`isolation_level.hpp:20`). **No serializable level.**
- Needs the whole graph in RAM, so it fails FR-STO-001. Pointer-based chains would need `Arc`/arena indices under `forbid(unsafe_code)`.

**E. Memgraph on-disk (RocksDB)**
- "The on-disk transactional storage mode is still in the experimental phase". "you should expect slower performance when compared to the in-memory transactional storage mode". "supports only snapshot isolation level". Conflicts are checked at commit through RocksDB transactions (Memgraph storage memory usage page).
- Keys are `|`-separated strings; the edge value is serialised with the src/dst gids (`src/utils/rocksdb_serialization.hpp:122,153`).
- Research-01 rejected RocksDB (no Rust-pluggable filesystem).

**F. Clustered B+tree adjacency (research-01 baseline)**
- One B+tree keyed by (node id, direction, edge type, neighbour id, edge id), with two entries per edge (OUT at the source, IN at the target). Enumerating the neighbours of (v, dir, type) is one descent plus a sequential leaf scan.
- The same shape as the Neo4j block format's dense store ("sorted by relationship type and direction for any given node").

**G. Per-node adjacency blocks / logs (LiveGraph TEL, the Neo4j block format for small degree)**
- LiveGraph stores each vertex's adjacency as a contiguous log that is copied to a block twice the size when it fills. Each entry carries creation and invalidation timestamps, so an MVCC scan stays sequential ([LIVE] §3-4).
- It runs on one memory-mapped file with its own allocator ([LIVE] §3; "We plan to replace mmap").

### L1.2 Published measurements (in memory)

[LIVE] §2.1: 10^8 adjacency-list scans, Kronecker graphs of 2^20 to 2^26 vertices, average degree 4, single thread, all in memory.
- Per-edge scan: TEL is ">29× over linked list, 20× over LSMT, and 6× over B+ tree". The B+tree is LMDB, the LSMT is RocksDB, and the linked list is an in-memory C++ prototype standing in for Neo4j's layout.
- "CSR's latency for scans is 43% that of TEL".
- Last-level cache misses relative to TEL: B+tree 7.09×, LSMT 11.18×, linked list 63.54×.
- With LinkBench over Optane storage, average latencies for MySQL/MyRocks/Neo4j were 0.187/0.214/0.236 ms (TAO), against LMDB 0.0109 ms and LiveGraph 0.0044 ms (Table 3 and §7.2). Neo4j ran read-committed.

Caveats: the "B+tree" is LMDB's generic key-value cursor over mmap, not a graph-specific leaf scan. All numbers are in memory. [LIVE] does not isolate on-disk I/O per hop.

### L1.3 Own measurement: pages touched per expansion (on-disk I/O model)

No source quantifies I/O per hop for these layouts over 8 KiB pages, so I simulated it. `layout-sim/pagesim.py` is pure Python and deterministic (seed 42).
- Graph: 100 000 nodes and 1 000 000 edges; endpoints drawn from a Zipf-like distribution (α = 0.8 and 0.5).
- Two creation orders: *random* (online transactional growth) and *sorted by source* (best case, bulk import).
- Metric: distinct pages read to list the outgoing neighbours of a node. This is the cold-cache I/O of one expansion; B+tree inner pages are excluded and assumed cached.
- Means: *uniform* averages over start nodes; *degree-weighted* approximates the nodes a multi-hop traversal actually reaches.

Layouts modelled:
- **REC**: Neo4j record style, 34 B records, id = creation order, 240 records per page; sparse node walks its whole chain, dense node (> 50) walks its outgoing chain plus the group record.
- **BT47**: fixed 47 B entries (29 B key + 16 B create/delete stamps + 2 B slot).
- **BT31**: the same without stamps.
- **BT18**: a *model* of a prefix-compressed leaf with frozen stamps.
- **CSR**: packed 16 B entries; a lower bound that cannot be updated in place.

B+tree leaves split 50/50 on real insertion order.

| α=0.8, random order | pages/expand uniform | pages/expand degree-weighted | p99 | bytes/edge | pages dirtied per insert | existing records rewritten per insert |
|---|---|---|---|---|---|---|
| REC | 15.74 | **450.30** | 92 | **35.5** | 4.69 | 3.90 |
| BT47 | 1.08 | 10.31 | 2 | 137.0 (fill 0.70) | 3.02 | 0 |
| BT31 | 1.05 | 7.12 | 2 | 89.7 | 3.01 | 0 |
| BT18 (model) | 1.03 | 4.77 | 2 | 56.3 | 3.01 | 0 |
| CSR | 1.02 | 3.15 | 2 | 33.6 | n/a | n/a |

| other runs | REC uniform / weighted | BT47 uniform / weighted | CSR uniform / weighted |
|---|---|---|---|
| α=0.8, sorted | 7.11 / 11.15 | 1.08 / 13.63 | 1.02 / 3.15 |
| α=0.5, random | 18.97 / 39.59 | 1.07 / 1.26 | 1.02 / 1.06 |
| α=0.5, sorted | 10.04 / 10.25 | 1.07 / 1.27 | 1.02 / 1.06 |

Reading of the measurement (model, not a timing benchmark):
- **I/O per hop.** Record chains need about 15-19 pages per expansion of a typical node, against about 1 for a clustered B+tree (14.6-17.7× by uniform mean). With skew and random growth, the degree-weighted cost is 450 against 10 (43.7×; 31× at α=0.5). Records scatter because they are placed by creation time, not by owner node, which is the cause the Neo4j block format addresses.
- **Exception, reported as measured.** In the sorted-import order at α=0.8, the degree-weighted mean is **better for records (11.15) than for BT47 (13.63)**. The cause is 50/50 leaf splits under sorted insertion (fill 0.63) combined with IN entries arriving in random order. Uniform means in that run still favour the B+tree (7.11 against 1.08).
- **Space.** Records are the most compact updatable layout: 35.5 B/edge, both directions sharing one record. Fixed-entry B+trees cost 2.5-3.9× more (90-151 B/edge; sorted insert with 50/50 splits gives fill 0.63). Prefix compression plus frozen stamps would bring this to about 56 B/edge (1.6× records, 1.7× CSR). The B+tree totals exclude the edge-id tree (about 45 B/edge, appended at the right edge). The record totals exclude property and group records.
- **Writes.** A record insert rewrites about 3.9 existing records across about 4.7 pages and needs exclusive locks on both endpoints (Neo4j, above). A B+tree insert rewrites no existing record, dirties about 3 pages, and needs no endpoint lock for the adjacency itself.

### L1.4 Index-free adjacency with never-reused identifiers (the BR-TYPE-001 question)

BR-TYPE-001 only forbids **reusing the identifier**, not reusing the **slot**. Two designs keep the record layout:

- **R1: id = slot, never reuse.** Deleted records are never reclaimed, so the file grows with every element ever created. Reclaiming by compaction moves records and changes ids, which violates FR-TYPE-003. It fails criterion (c) under deletes. **Rejected.**
- **R2: logical id plus slot indirection.** Records live in reusable slots; chain pointers hold slot numbers. Each record stores its immutable logical element id, and a logical-id → slot map (a B+tree, about 18-20 B per live element) serves lookups by element id.
  - Traversal cost is unchanged, because chains use slots directly.
  - Lookup by element id costs one extra descent.
  - Slots are reused only after the GC horizon, as Neo4j already does with IDs.
  - R2 is compatible with BR-TYPE-001.

**Conclusion: BR-TYPE-001 is not what excludes index-free adjacency.** With R2 the record layout satisfies the identifier rule at about 20 B per element plus one descent per id lookup. Relaxing BR-TYPE-001 would change nothing below. The evidence against the record layout comes from criteria (b) and (a):
1. I/O per hop (L1.3: 6.6-44× more pages than a clustered layout in 7 of 8 measured means; records win 1.2× only in the sorted-import degree-weighted case) and in-memory scan speed ([LIVE]: linked list 29× slower than a sequential log, 63× more LLC misses).
2. MVCC and write cost: each insert rewrites about 4 existing records (head pointers and neighbours' prev pointers) and locks both endpoints exclusively.
   - MVCC can avoid versioning pointers: readers skip invisible records by timestamp, and GC does the physical unlinking.
   - The cost is a GC that must lock and rewrite neighbouring chain records.
   - Hub nodes also serialise every writer that adds an edge (FR-TX-005 waits).

The **fast form of index-free adjacency is the per-node block** (Neo4j block format, LiveGraph TEL), not the chain. It is assessed in L1.5.

### L1.5 Assessment against the three criteria

| Layout | (a) MVCC | (b) traversal | (c) resources | Other |
|---|---|---|---|---|
| Record chains (A, with R2) | Possible with stamps plus deferred unlink; pointer rewrites on neighbours; endpoint X-locks | Worst: 15-450 pages/hop (L1.3); 29× slower in memory ([LIVE]) | Best space (35.5 B/edge); worst write amplification (about 4.7 pages/insert) | Neo4j abandoned chains in its newest format |
| Per-node blocks (B/G) | Good: inline double timestamps give sequential MVCC scans ([LIVE]) | Near CSR for small and medium degree; dense nodes need a B+tree anyway (Neo4j dense store) | Compact; growth needs relocation (copy O(degree), WAL images of the whole list), a variable-size block allocator and a second structure for dense nodes | Neo4j code is closed; LiveGraph relies on mmap, which lpg forbids |
| Columnar + CSR (C) | Harder: Kuzu allows one writer; updates need re-sorting and gaps ([GUPTA] §7) | Best scans | Best compression | Fails FR-STO-003 as built by its reference; requires schema-known columns |
| Memgraph in-memory (D) | Delta chains, no waiting, no serializable level | Very good in RAM | RAM ≥ graph | Fails FR-STO-001 |
| Memgraph on-disk (E) | SI only, experimental, RocksDB | "slower" (vendor) | LSM | RocksDB rejected in research-01 |
| **Clustered B+tree (F)** | **Insert-only stamped entries: no versions, no endpoint X-lock for adjacency** | **≈1 page/hop typical; 1.2-4.3× the CSR lower bound degree-weighted (1.5-1.8× with the compression model); loses to records only in the sorted-import degree-weighted case** | **2.5-3.9× record space fixed-width; about 1.6× with prefix compression** | **One mechanism shared by adjacency, indexes, catalog and records** |

### L1.6 Recommendation L1

**One clustered B+tree per graph for adjacency.**
- Key: `(node_id, direction, edge_type, neighbour_id, edge_id)`, two entries per edge.
- Each entry carries `created_ts` and `deleted_ts` (LiveGraph's double timestamps inside a B+tree).
- Edges are also in an **edge B+tree** keyed by `edge_id` → (src, dst, type, properties).
- Nodes are in a **node B+tree** keyed by `node_id` → (labels, properties, row format, overflow for large values).
- Identifiers are monotonic and never reused, so node and edge trees append at the right edge. Use rightmost-split fill as PostgreSQL does (`nbtsplitloc.c:95`, `BTREE_DEFAULT_FILLFACTOR 90`, `nbtree.h:201`) or SQLite `balance_quick` (`btree.c:8039`).
- Start with fixed-width entries (simplest). Add leaf prefix compression and stamp freezing (the BT18 model) only when the NFR-PERF-002 baseline shows that space or I/O fails a target. PostgreSQL nbtree deduplication (`nbtdedup.c`) is the reference for keeping a key once per run.
- Why: it meets (a) with zero version creation for adjacency, meets (b) within a small factor of CSR on I/O, and uses one structure for every tree the engine needs, as NFR-SIMP-001 requires. It is also what Neo4j's block format uses for dense nodes.

Rejected:
- Record chains, with or without R2: I/O and CPU per hop, write amplification, endpoint X-locks.
- Per-node blocks: best in-memory traversal, but a second allocator plus relocation plus a B+tree for dense nodes. It is kept as the **candidate optimisation** to test with the L6 microbenchmark if B+tree CPU per edge fails a measured target.
- Columnar/CSR as primary storage: single-writer reference, update complexity.
- Memgraph modes: RAM-bound; SI only.

**Open (evidence cannot settle):** where edge properties live.
- Option (i), simplest: in the edge tree. A property-filtered expansion then reads edge records in creation order, which is random.
- Option (ii): inline in the OUT adjacency entry, with the IN entry keys-only and an edge-id index. [GUPTA] Table 3 measured 1.9-4.7× faster filtered forward expansions when edge properties are stored in forward adjacency order.
- Recommendation: (i) now, (ii) only if the L6 benchmark shows the gap on LDBC.

### L1.7 The user's question: benefits of a columnar approach, and which layout most favours traversals

Benefits of columnar, with evidence:
1. **Compression and memory.** [GUPTA] §8.2 measured on LDBC100 against their own row layout: 2.96× and 2.74× smaller forward and backward adjacency lists, 1.58× smaller vertex properties (column layout "saves on storing the keys of the properties explicitly"), and 3.82× smaller edge properties.
2. **Sequential property access** in adjacency order: filtered 1-hop and 2-hop queries 1.9-4.7× faster ([GUPTA] Table 3).
3. **Scans of one property over many nodes** read only that column ([KUZU] "read-optimized analytical").

Costs:
- "the columnar storage techniques we covered are read-optimized and necessarily add several complexities to updates ... handling deletions or insertions requires resorting the CSRs and recalculating the CSR offsets" ([GUPTA] §7).
- The reference system allows one writer ([KUZU] §2; `transaction_manager.cpp:35-38`).
- Column savings assume properties known per label. GQL graphs may be schema-free, so columns become sparse and need NULL compression ([GUPTA] +NULL step).
- For an OLTP engine with concurrent writers and MVCC (lpg), row storage per element is the evidence-backed choice. The columnar benefits are reachable where they matter, in **algorithm projections** (L2.5).

Which layout most favours traversals:
- Contiguous per-node adjacency (CSR, then per-node logs/blocks). CSR scans are 0.43× TEL latency, and TEL is 6× faster than a B+tree and 29× faster than a linked list in memory ([LIVE]).
- On disk, CSR ≤ clustered B+tree (degree-weighted 1.2-4.3× more pages; 1.5-1.8× with the compression model) < record chains (6.6-44× more than the B+tree, except the sorted-import degree-weighted case where records win by 1.2×; L1.3).
- Among layouts that meet MVCC plus concurrent writers plus larger than memory under lpg's constraints, the clustered B+tree is the simplest that keeps most of the locality.

---

## L2. MVCC version storage

### L2.1 Evidence

| Engine | Where old versions live | GC | Notes |
|---|---|---|---|
| PostgreSQL | In the heap: each tuple has `t_xmin` and `t_xmax` (`htup_details.h:124-125`); oldest-to-newest ([WU] Table 1) | VACUUM | Indexes point at every version; `old_snapshot_threshold` was removed in PG 17 because it produced "snapshot too old" errors; "might be re-added ... if an improved implementation is found" (PG 17 release notes) |
| InnoDB | Latest version in place; before-images in undo logs (`DB_TRX_ID`, `DB_ROLL_PTR`) | Purge, once "no active transaction needs them for consistent read" | Long consistent reads make "the rollback segment ... grow too big". Temporary-table undo is "not redo-logged, as they are not required for crash recovery" (MySQL 8.4 manual, Undo Logs; Multi-Versioning) |
| HyPer | In place; before-image deltas in per-transaction undo buffers, newest-to-oldest | Continuous, at commit, by oldest active start time ([HYPER] §2.4) | Write-write conflict aborts; no waiting ([HYPER] §2.3) |
| Memgraph | In place; delta chain newest-to-oldest | `CollectGarbage` from `commit_log_->OldestActive()` (`inmemory/storage.cpp:3044,3115`) | About 56 B per change (docs) |
| Kuzu | In-memory `UndoBuffer`/`UpdateInfo`; inserts in `LocalStorage` until commit | At commit/checkpoint | One writer |
| DuckDB | In-memory undo; optimistic; "any transactions that modify the same rows at the same time will cause a transaction conflict error" (docs) | | Isolation level not stated on that page (UNVERIFIED) |
| LiveGraph | Inline double timestamps in adjacency logs; vertex versions chained | Compaction | [LIVE] §4 |

[WU] §8: delta storage "is able to sustain a comparatively high performance ... especially when only a subset of the attributes ... is modified. But this scheme suffers from low table scan performance". Newest-to-oldest "always performs better than O2N" (§7). Transaction-level GC gave "the best performance with the smallest memory footprint".

### L2.2 The interaction that decides the design: several writers plus page-image WAL plus NO-STEAL

With the research-01 WAL (the full image of every modified page at commit), a page must never hold **another transaction's uncommitted change** when its image is logged. Otherwise the committed image carries uncommitted data. Two known ways out:
- in-place uncommitted writes with a logical WAL (Memgraph, HyPer);
- uncommitted writes kept private until commit (Kuzu `LocalStorage` for inserts).

The page-image WAL is fixed, so only the second is compatible.

### L2.3 Recommendation L2: deferred update plus two version mechanisms

1. **Private write set.** A transaction's writes (records, adjacency, index and catalog entries) are buffered in memory and charged to its memory limit (FR-LIM-001). Reads-own-writes merge the private set over the snapshot. Shared pages only ever hold committed or committing data. The buffer pool therefore never holds uncommitted state, and NO-STEAL costs nothing at the page level.
2. **Commit.**
   - Validate (L3).
   - Draw `commit_ts`.
   - Apply the write set under page latches, stamping `commit_ts`, and before each record is overwritten copy its before-image to the version store.
   - Append the page images, log order equal to apply order per page, captured under the page latch.
   - Group fsync.
   - **Publish** `commit_ts` in timestamp order. A snapshot is the highest timestamp below which every commit is published, never the raw counter.
   - Release locks.
   - After the apply starts, only a crash or an I/O failure can stop the commit, and both are handled by FR-REC-006.
3. **Immutable entries** (adjacency, label index, property and point indexes, catalog entries) carry `created_ts` and `deleted_ts`, and visibility is a stamp comparison. An update is a delete plus an insert, as HyPer does for indexed attributes ([HYPER] §2.5). They need **no** version store.
4. **Mutable records** (node and edge properties and labels): the newest committed version is in place with its `version_ts`. When `version_ts > snapshot`, the reader follows a newest-to-oldest before-image chain in the version store (InnoDB, HyPer, Memgraph, per [WU]). Deltas only, per [WU].
5. **Version store is not durable.** No snapshot survives a crash, so before-images are never WAL-logged. MySQL does the same with temporary-table undo, "not required for crash recovery". The store is dropped at open.
6. **GC.** Horizon = oldest active snapshot (Memgraph `OldestActive`, HyPer). Before-images older than the horizon are freed. Entries with `deleted_ts` below the horizon are physically removed from pages, and those removals are logged like any page change.

Long-running readers (FR-ALG-004):
- A reader pins the horizon. Memory grows only with **record updates** during the run. Edge and node inserts and deletes create no versions; they leave dead entries in pages until GC (disk, not RAM, like PostgreSQL bloat).
- The version store is charged to the FR-LIM-003 cap.
- What happens when the cap is reached is a product decision (see the open decisions).

Rejected:
- PostgreSQL heap versions: every version in pages, VACUUM, index entries per version.
- In-place uncommitted writes: incompatible with page-image WAL for several writers (L2.2).
- A durable undo log (InnoDB): only needed for STEAL.

### L2.4 Interaction with the memory cap and long readers

Charged to the FR-LIM-003 accountant: private write sets, version store, read sets (L3) and algorithm projections. Dead stamped entries cost disk space only.

### L2.5 Algorithms over snapshots

- Neo4j GDS and similar tools build in-memory projections. That was **not re-verified** for this report (UNVERIFIED).
- With the L1 layout, a projection is a single sequential scan of the adjacency tree in key order, which is already source-sorted. It yields a CSR (the columnar format that [LIVE] and [GUPTA] measure as fastest) with no sort, charged to the memory cap.
- A read-only algorithm run (stream mode) needs no read set and cannot fail serialisation (L3).

---

## L3. Serializable isolation with non-blocking reads

### L3.1 Evidence
- **PostgreSQL SSI.** SI plus detection of the "dangerous structure" of two adjacent rw-conflicts. SIREAD locks at tuple, page or relation granularity, promoted "as needed to avoid resource exhaustion". Summaries of committed transactions spill to SLRU. Read-only transactions can opt out or be DEFERRABLE (`README-SSI` l.150-180, 255-300, 515-590). Implementation size: `predicate.c` 4 993 lines.
- **HyPer precision locking.**
  - Update transactions log **read predicates** (point and range lookups, scan restrictions) and, at commit, validate against the before and after images of transactions that committed during their lifetime. They are aborted on intersection.
  - The serialisation order is commit time.
  - Read-only transactions need no validation ("This is sufficient for serializability of read-only transactions").
  - Validation cost scales with recent writes, not with the read-set size ([HYPER] §2.3).
  - [WU] §8 notes MVOCC read-set validation overhead in their implementation, and that "none of the systems ... adopt" MVTO.
- **Memgraph**: no serializable level (`isolation_level.hpp:20`).
- **Kuzu**: serializable by single writer.
- **DuckDB**: optimistic write-conflict errors.

### L3.2 Recommendation L3: HyPer-style commit-time validation over key ranges

Every read in the L1/L5 design is a lookup or a range on a B+tree key:

| Read | Range |
|---|---|
| Node by id | Point on the node tree |
| Expand | (v, dir, type) prefix on the adjacency tree |
| Label scan | (label) prefix on the label index |
| Index seek or range | Key range on that index |
| Point query | Curve ranges |
| Full scan | Whole tree |
| Catalog lookup | Point on the catalog tree |

Every write inserts, stamps or updates keys on the same trees. The read set is therefore a list of `(tree, key range)` intervals, and validation checks the keys written by transactions with `commit_ts` in (snapshot, own `commit_ts`) against those intervals.
- Phantoms are covered: an inserted edge or label entry falls in the scanned prefix.
- Point reads are merged into ranges, and ranges are coarsened to a whole tree when the read set exceeds its memory budget, as HyPer coarsens index nested-loop values and PostgreSQL promotes SIREAD locks.
- Registration of `commit_ts` and of the write set happens in one short critical section, so no concurrent committer is missed.
- Read-only transactions skip validation.
- Snapshot isolation mode (FR-TX-004) skips validation.

Write-write rule with waiting locks (FR-TX-005/006), PostgreSQL REPEATABLE READ semantics: the second writer "will wait for the first updating transaction to commit or roll back ... if the first updater commits ... will be rolled back" (`doc/src/sgml/mvcc.sgml:541-553`). This is checked on lock grant: if the element's `version_ts` is newer than the snapshot, the transaction fails with a serialisation failure.

Why rejected:
- PostgreSQL SSI: fewer false aborts (it needs two adjacent rw-edges), but it keeps SIREAD locks alive after commit, tracks rw-conflict graphs and needs summarisation. It is several thousand lines in the reference, and it can abort read-only transactions unless the snapshot is safe (README-SSI).
- Strict 2PL: read locks contradict FR-TX-006.

Consequence to report:
- A read-write transaction that reads a large part of the graph (an algorithm in write-back mode, FR-ALG-003) has a coarse read set. It fails validation if any concurrent commit wrote in that range during its run.
- PostgreSQL SSI would abort it less often. The cost of failing, and whether it is acceptable, is a product question (see the open decisions).
- Not measured here.

Attribute-level refinement (HyPer SR-AL) is an optimisation for later.

---

## L4. Write locks and deadlock detection

### L4.1 Evidence
- **PostgreSQL**: optimistic waiting; the detector runs after `deadlock_timeout` (default 1 s) over the waits-for graph; the victim is "usually ... the detecting process' transaction" (`lmgr/README` l.338-398; `deadlock.c:204-243`).
- **Neo4j Forseti**:
  - Dreadlocks wait lists, with possible false positives confirmed by a graph traversal (`ForsetiLockManager.java:63-105`).
  - The victim is "the client with the fewest number of held locks" (`ForsetiClient.java:900-915`), not necessarily the requester.
  - Sparse-node edge inserts take X locks on both nodes; dense nodes take a shared "NODE DELETE prevention lock" (Concurrent data access page).
  - Uniqueness uses an exclusive `INDEX_ENTRY` lock on (label, value) (`Operations.java:1195-1198`, `ResourceType.java:83`).
- **Memgraph**: no waiting; immediate serialization error (`mvcc.hpp:111-137`), which does not meet FR-TX-005.

### L4.2 Recommendation L4
- **Lock table.** A sharded `HashMap<LockKey, LockState>`, keys `(graph, resource)`. Locks are held to the end of the transaction (after publish). Resources and modes:

  | Resource | Mode | Taken by |
  |---|---|---|
  | node content | X | label or property change, delete |
  | node existence | X | node delete |
  | node existence | **S** | edge create and delete on both endpoints |
  | edge | X | edge change or delete |
  | unique-value key `(constraint, value)` | X | writer of a value under a uniqueness constraint (Neo4j `INDEX_ENTRY`) |

  - The S mode prevents a dangling edge under SI when one transaction deletes a node while another adds an edge to it. Neither sees the other in its snapshot, and apply-time checks on two different pages can race. Neo4j solves the same case with its shared "NODE DELETE prevention lock".
  - Splitting *content* from *existence* keeps edge inserts on a hub from waiting behind a property update of the hub.
- **Deadlock detection at request time.** Before blocking, the requester adds its wait edges (to every holder whose mode conflicts) under one detector mutex. It runs a DFS from itself and, if it finds a cycle, aborts **itself** (FR-TX-007: "WHEN a lock request closes a wait cycle ... abort the requesting transaction").
  - Waits are FIFO. When a grant re-points a waiter's edge, that waiter re-runs the check.
  - No timer, unlike PostgreSQL, so detection is immediate and exact.
  - Cost is proportional to the waiting part of the graph, usually tiny. Not measured.
  - Rejected: PostgreSQL timeout-then-check (delays detection; the victim is the detector, not necessarily the cycle-closing requester); Forseti victim policy (fewest locks, contradicts FR-TX-007); Memgraph no-wait (contradicts FR-TX-005).
- **Cancellation.** A waiter wakes on cancel, removes its edges and aborts.

---

## L5. Indexes and constraints on this layout

### L5.1 Evidence
- **PostgreSQL**: index tuples point to heap tuple versions. A CREATE INDEX CONCURRENTLY build commits the catalog entry not-ready. It then waits for all transactions that might have the table open with the old index list, builds from a new snapshot, validates, waits again, and marks the index valid (`indexcmds.c:1660-1800`). Writers never wait for the build, but the build waits for writers. Unique insert waits on the conflicting in-progress transaction (`nbtinsert.c:228-230`).
- **InnoDB**: secondary entries are delete-marked, not updated in place. A reader checks visibility through the clustered record and undo (MySQL Multi-Versioning page).
- **Memgraph**: label index entries are `(Vertex*, timestamp)`, a superset that the reader re-checks; obsolete entries are removed by GC (`inmemory/label_index.hpp:37-45`, `label_index.cpp:218,258-308`). The point index is an in-memory `boost::geometry` R-tree (`indices/point_index_expensive_header.hpp:18,86`).
- **Neo4j**:
  - Population scans the store while concurrent updates are queued with `queueConcurrentUpdate` and merged (`MultipleIndexPopulator.java` l.90-117).
  - **Point index** = space-filling curve keys in the native B+tree: `PointIndexReader` turns a query envelope into curve ranges with `getTilesIntersectingEnvelope` (`PointIndexReader.java:130`); a Hilbert curve implementation is in `spatial-index/.../HilbertSpaceFillingCurve2D.java`. **Verified.**
- **HyPer**: an update of an indexed attribute is a delete plus a re-insert, and the index keeps both ([HYPER] §2.5).

### L5.2 Recommendation L5
- **All indexes are B+trees of immutable stamped entries** (L2.3), so visibility needs no record lookup:
  - label and edge-type lookup: `(token, element_id)`;
  - single-property and range: `(index, order-preserving value encoding, element_id)`;
  - composite: concatenated encodings;
  - **point**: Hilbert key (Neo4j approach) plus exact distance post-filter.

  Rejected: an R-tree (a second tree type with its own WAL and recovery code); Memgraph-style superset plus re-check (one record read per hit).
- **Online build (FR-IDX-005).**
  1. Commit the index catalog entry in state POPULATING. From that commit on, every committing writer maintains the index at apply time. DDL applies take an exclusive commit-apply latch and DML applies a shared one, so no writer applies against a stale index list. Writers never wait for the build.
  2. The builder scans a snapshot at or after the entry's publication and inserts entries *only if absent*.
  3. A writer that must delete-stamp an entry the builder has not yet inserted writes a **tombstone** entry. The builder's later insert of the same key then skips.
  4. When the scan ends, mark the index ONLINE in a normal transaction. Snapshots older than the ONLINE timestamp do not use the index, so the builder may give its entries `created_ts = 0`.

  This is the Neo4j queue-and-merge idea done with stamps instead of a queue. **UNVERIFIED design**: it needs a deterministic concurrency test before adoption.
- **Uniqueness and node key.** Exclusive unique-value lock at write time (Neo4j `INDEX_ENTRY`), so a second writer waits (FR-TX-005) and then fails if the first committed. The unique index is checked again at apply time.
- **Existence and property type.** Checked on the transaction's own writes at commit.
- **Constraint creation (FR-IDX-004).** The creating transaction reads the whole label range, so it fails validation (L3) if a concurrent commit touched that label. The alternative is to block writers of that label during creation, which reintroduces S locks on labels. See the open decisions.

---

## L6. Microbenchmark (the ranking left open by the sources)

Built (cheap): the page-count model in L1.3. It ranks I/O and space. It does **not** rank CPU per edge in lpg's own code: [LIVE] gives 6× B+tree against a per-node log, but with LMDB's generic cursor.

To specify, in Rust, before the adjacency format is frozen:
- **Implementations.**
  - (1) clustered B+tree leaf scan over 8 KiB pages in a `Box<[u8]>` buffer pool, fixed 47 B and prefix-compressed;
  - (2) per-node block layout (inline small lists, doubling blocks, B+tree above a threshold);
  - (3) record chains with R2 indirection;
  - (4) CSR as the lower bound.
  - All use lpg's frame and latch code, with the same MVCC stamp check.
- **Data.** LDBC SNB SF1 and SF10 (`knows`, `hasCreator`, `replyOf`), plus a Zipf generator (α 0.5 and 0.8), in both random and sorted creation orders.
- **Metrics.**
  - ns per edge and per expansion (1-hop and 2-hop, start nodes uniform and degree-weighted);
  - page reads per expansion, cold (cache cleared through ABS-003) and warm;
  - bytes per edge on disk; RSS and allocations;
  - insert throughput and pages dirtied per insert with 1 and 16 writers;
  - full-graph projection-to-CSR time.
- **Tools.** criterion, `perf stat` (LLC misses), heaptrack or dhat, and the ABS-003 recorder for I/O.
- **Decision rule.** Keep the B+tree unless (2) is faster on degree-weighted 2-hop by more than the margin the user sets against NFR-PERF-002, **and** its write and space costs stay within that margin.

---

## Summary of recommendations

| Q | Recommendation |
|---|---|
| L1 | Row storage. Per graph: node B+tree (id → labels and properties), edge B+tree (id → endpoints, type, properties), one clustered adjacency B+tree `(node, dir, type, nbr, edge)` with 2 entries per edge and inline created/deleted stamps. Monotonic never-reused ids. Fixed-width entries first; compression by measurement. |
| L2 | Deferred update (private write sets). In-place newest committed record plus a non-durable, in-memory newest-to-oldest before-image store for mutable records. Stamped immutable entries for adjacency, indexes and catalog. GC horizon = oldest active snapshot. Everything charged to the memory cap. |
| L3 | HyPer-style commit validation of `(tree, key-range)` read sets against write sets committed during the transaction; read-only transactions and SI skip it; PostgreSQL RR wait-then-fail rule for write-write. |
| L4 | Lock table with X plus S (node existence) modes. Request-time DFS on the waits-for graph; the requester is the victim. |
| L5 | Every index is a B+tree of stamped entries; Hilbert-curve point index; online build by stamps plus tombstones plus POPULATING/ONLINE states; unique-value X locks. |
| L6 | Page model built and run. The Rust CPU benchmark is specified above and gates the choice between B+tree and per-node blocks. |

## Open decisions for the user (evidence cannot settle)

1. **When the version store (with long-running snapshot readers) would exceed the memory cap:**
   - (a) fail the writer's statement under FR-LIM-001; readers are protected, writers can fail while a long reader runs;
   - (b) abort the oldest snapshot; PostgreSQL removed its "snapshot too old" feature in 17;
   - (c) spill before-images to an unlogged temporary file through the buffer pool (the InnoDB temporary-undo analogue), with no failures and more code.

   The simplest is (a).
2. **Write-back algorithms, and other large read-write transactions, under serializable:** with HyPer-style validation they fail if anything they read changed during the run. Is that acceptable, given BR-TX-001 (no automatic retry)? If not, the alternative is PostgreSQL SSI, which has fewer false aborts and much more code.
3. **Constraint creation:** fail on concurrent conflicting writes (simplest, may starve on busy labels), or block writers of that label during creation.
4. **Space against simplicity:** fixed-width adjacency entries cost about 137 B/edge against about 36 B/edge for Neo4j-style records (L1.3). Accept this until the baseline, or fund prefix compression and stamp freezing (about 56 B/edge modelled) up front?

## Unverified points

1. Neo4j block format internals beyond the Operations Manual, including whether a relationship is stored at both endpoints. The format is closed-source Enterprise.
2. The L1.3 numbers are a page-count **model** (synthetic Zipf graph; 50/50 splits; inner pages assumed cached; BT18 ignores per-run header cost). They are not timings and not LDBC.
3. The CPU cost per edge of an lpg B+tree leaf scan against per-node blocks: [LIVE]'s 6× used LMDB; needs L6.
4. Neo4j's relationship-id allocation locality (per-transaction id ranges) was not checked; REC assumes creation order.
5. DuckDB's isolation level; Oracle "snapshot too old" behaviour; Neo4j GDS projection internals; the Cahill and Ports & Grittner papers themselves (only PostgreSQL's README-SSI was read).
6. The online-build tombstone protocol (L5.2) and the commit critical section (L3.2) are designs, not tested code.
7. The Memgraph docs quote "about 56 B per change" comes from a fetched summary, not a verbatim line.
