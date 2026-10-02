---
title: Catalog and indexes
status: draft
last-reviewed: 2026-10-02
related:
  - storage.md
  - transactions-and-recovery.md
  - query-engine.md
---

# Catalog and indexes

## TS-17 Catalog and statistics

- **Decision**:
  - **Catalog tree.** One B+tree per database, with immutable stamped entries ([TS-14](transactions-and-recovery.md#ts-14-mvcc)). Catalog writes join the transaction's write set, so DDL and data changes commit and roll back together.
    - `(parent_id, name) → (object_id, kind, owner, definition)` for directories, schemas, graph types and graphs. `object_id` is monotonic and never reused.
    - Reverse entries `object_id → (parent_id, name)` for paths and listing.
    - Token tables `(kind, name) ↔ token_id` for labels, edge types and property keys.
    - Index and constraint definitions keyed by `(graph_id, object_id)`.
  - **Creation.** A new database's catalog contains a default schema and a default graph. Directories are created only through the Rust API.
  - **Dropping a graph.** `DROP GRAPH` stamps the entry deleted; garbage collection frees the graph's trees after the horizon passes.
  - **Closed graph types.** Graph types are immutable once created. Each `INSERT`, `SET` and `REMOVE` on a graph of a closed type is checked at the end of its statement against that type, over the elements the statement wrote. Open graph types cost nothing. The exact matching rules wait for the ISO text (OQ-005, unverified).
  - **Statistics.** Node counts per label; edge counts per `(label, edge type, direction)` with the label on one side; entry and distinct-key counts per index. They are kept in memory, updated when commits are applied, and saved at checkpoint. After a crash they are estimates until rebuilt by sampling.
- **Satisfies**: FR-CAT-001, FR-CAT-002, FR-CAT-003, FR-CAT-004, FR-CAT-005, FR-DB-001, FR-DB-005, FR-AUTH-002, FR-GQL-004, FR-PROC-004.
- **Evidence**:
  - Token maps: Neo4j (R-NEO `community/token-api/.../TokenHolder.java`) and Memgraph (R-MG `src/storage/v2/name_id_mapper.hpp`).
  - Count statistics: Neo4j keeps per-label counts and uses histograms only in tests; Kuzu uses default selectivities (M-QE §Q3.1).
  - The opengql grammar has no statement that creates a directory or alters a graph type; `DIRECTORY` and `ALTER` are only reserved words (R-GQL `GQL.g4:3496-3537`, from M-QE §Q1.2).
- **Rejected**:
  - A catalog file separate from the transactional trees: FR-CAT-004 requires DDL and data changes to be atomic together.
  - Durable counters updated by every commit: one page image per commit on a hot page ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)).

## TS-18 Indexes and constraints

- **Decision**:
  - **Indexes** are B+trees of stamped entries with compressed leaves ([TS-06](storage.md#ts-06-graph-layout)) and order-preserving key encodings ([TS-21](values-and-types.md#ts-21-values-and-types)):

    | Kind | Key |
    |---|---|
    | label or edge-type lookup | `(token, element_id)` |
    | single-property, range | `(index_id, value, element_id)` |
    | composite | `(index_id, value_1, …, value_n, element_id)` |
    | point | `(index_id, Hilbert key, element_id)` per CRS (2D or 3D), with exact distance as a post-filter |

    A reader uses an index only from the snapshot at which the index became ONLINE.
  - **Online build** (giant-graph path):
    1. A transaction commits the definition in state POPULATING, at timestamp `S`.
    2. From `S` on, committing writers record their index changes (insert, and delete stamp) in a small **side tree**, which is an ordinary logged B+tree.
    3. The builder scans the snapshot `S`, sorts the keys with an external merge sort into temporary files through the storage trait, and builds the index bottom-up into new pages, filled to 90 %.
    4. The builder then applies the side tree to the new index in commit order, drops the side tree, and marks the index ONLINE in a normal transaction.
    5. After a crash, a POPULATING index is rebuilt from step 3.

    Sort and side-tree memory is charged to the cap. Writes never wait for a build.
  - **Constraints.**
    - Uniqueness and node key: exclusive lock on `(constraint, value)` at write time ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)), checked again when the commit is applied.
    - Property existence and property type: checked over the write set at commit.
    - Creation takes the exclusive schema lock on its label or edge type in its graph, validates the existing data (building its owned index as above), and fails without creating the constraint if any element violates it. Writers of that label or type wait meanwhile; writers of others do not.
  - Every index and constraint is defined on one graph.
- **Satisfies**: FR-IDX-001, FR-IDX-002, FR-IDX-003, FR-IDX-004, FR-IDX-005, FR-IDX-006, FR-TYPE-005, FR-API-004, NFR-PERF-005.
- **Evidence**:
  - Neo4j point index: Hilbert-curve keys in its B+tree (R-NEO `community/spatial-index/.../curves/HilbertSpaceFillingCurve2D.java`, `HilbertSpaceFillingCurve3D.java`; `PointIndexReader`). Memgraph uses an in-memory R-tree (R-MG `src/storage/v2/indices/point_index_expensive_header.hpp:18`).
  - Neo4j index population queues concurrent updates and flushes them into the populated index (R-NEO `kernel/.../api/index/MultipleIndexPopulator.java:92-118`).
  - PostgreSQL builds B-trees bottom-up from sorted input (R-PG `src/backend/access/nbtree/nbtsort.c:4`) and sorts larger-than-memory input by external k-way merge (R-PG `src/backend/utils/sort/tuplesort.c:11-15`).
  - Memgraph index entries carry timestamps and are pruned by garbage collection (R-MG `src/storage/v2/inmemory/label_property_index.hpp:62`).
  - Neo4j uniqueness takes an `INDEX_ENTRY` lock (P-TX §L4.1), and its writers take shared label schema locks (R-NEO `Operations.java:792`).
- **Crates**: `rstar` 0.13.0 (an in-memory R*-tree) and the 2D-only `fast_hilbert` and `hilbert_2d`: study only. The 2D and 3D Hilbert encoding is the crate's own (SRIDs 4979 and 9157 are 3D).
- **Rejected**:
  - Building by inserting entries one at a time into the live tree: random I/O over the whole index at 1–10 % cache (NFR-PERF-005).
  - An R-tree: a second tree type with its own WAL and recovery code.
  - Index entries rechecked against records on every hit (Memgraph): one record read per hit.
  - An in-memory population queue (Neo4j): its size is bounded only by write traffic during a long build.

The side-tree protocol is a design that has not been tested; it needs the deterministic concurrency tests of [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) (unverified).
