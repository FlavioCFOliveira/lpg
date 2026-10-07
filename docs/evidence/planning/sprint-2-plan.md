# Sprint 2 plan: transactional graph store (draft for task #2)

Prepared 2026-10-07 on branch `feature/2-transactional-graph-store-mvcc-serializable-catalo` at `30dd486`. Read-only: no repository file and no `rmp` record was changed.

## 0. Basis, method and limits

* **Read:** `CLAUDE.md`, `README.md`; `rmp sprint get 2`, sprint comments #6, #7, #8, #9, #33, #46, #51, #54; task #2 and its comments #71, #153; Sprint 1 planning comments #1–#7 on task #1; tasks #7, #8, #9, #12, #15, #16, #20, #22, #23 (style and granularity) and #8's comments #8–#16.
* **Specification read:** `docs/technical-spec/` (transactions-and-recovery, storage, values-and-types, catalog-and-indexes, quality-and-testing, dependencies, architecture, interfaces, README), `docs/functional-spec/functional-requirements.md`, `non-functional-requirements.md`, `open-questions.md`, the ISO implementation-defined and features inserts in `docs/standards/iso-iec-39075/ed-1/`.
* **Reference clones made for this plan** (study only; CLAUDE.md "Open-source references"): `/tmp/turso` @ `8cc059b` (2026-10-07, MIT, full shallow clone as instructed); `/tmp/neo4j` @ `54a7dcf` (sparse: `community/spatial-index`, `community/lock`, `community/token-api`, value and schema-config packages; GPLv3, never copied); `/tmp/postgres` @ `1f22453` (sparse: nbtree, heap, lmgr, `src/include/access`; PostgreSQL licence). No other clone existed in `/tmp`.
* **Tool state measured 2026-10-07** (read-only commands): `rustup check` → stable 1.99.0, nightly 1.101.0-nightly (2026-10-06), `rustup` 1.29.1, all "up to date"; toolchain 1.89 present; `cargo install --list` → cargo-deny 0.20.2, cargo-llvm-cov 0.9.1, cargo-nextest 0.9.146, inferno 0.12.8, cargo-fuzz 0.13.2; Java `openjdk 25.0.4.1`; `elle-cli`, `lein` absent; XFS mounted at `/mnt/lpg-xfs` (`mount`), ext4 lab directory `/data/tmp/lpg-lab` (`docs/evidence/lab-sprint-1/report.md:15-16`); `kernel.perf_event_paranoid` = 1 (not needed: no timing in Sprint 2).
* **crates.io, 2026-10-07:** loom 0.7.2 (MIT), shuttle 0.9.5 (Apache-2.0), jiff **0.2.38** (2026-10-06; TS-21 and TS-31 cite 0.2.37), others in §4.
* **Unverified (stated plainly):** the latest `elle-cli` release and its command-line flags (GitHub API returned no data, probably rate limiting); Memgraph source lines (not cloned; cited only through the existing TS-14 citations); the PostgreSQL `FOR KEY SHARE` and unique-catalog-index lines, the Neo4j counts-store and `IndexingService` restart paths (named as "to verify" in the gap task).

## 1. Requirements digest

Scope ids from sprint comment #6 and exit ids from #8, ranges expanded. "S3" marks an acceptance criterion that needs a GQL statement, a procedure, a session or the authorisation abstraction: verified in Sprint 3 (comment #6); Sprint 2 verifies its storage-level equivalent through the crate-internal API. FS = `docs/functional-spec/functional-requirements.md`, NFS = `non-functional-requirements.md`, TS paths under `docs/technical-spec/`.

### Technical decisions

| Id | Demands (1–2 lines) | Where |
|---|---|---|
| TS-05 | Change: a page freed by commit `c` is reusable only when every registered reader value is ≥ `c`; with MVCC the registered value is the snapshot, so reuse follows the GC horizon. Records above 2 048 bytes use overflow chains. | `storage.md:73-77`, `:63` |
| TS-06 | Node, edge and adjacency trees per graph; adjacency key `(node_id, direction, edge_type, neighbour_id, edge_id)`, two entries per edge; field-delta varint leaves (layout B); every entry stamped `created_ts`/`deleted_ts`, `created_ts` omitted when frozen, `deleted_ts` while live; stamp encoding added with MVCC; directory and free list never stamped. | `storage.md:129-140, 150` |
| TS-07 | One 64-bit element counter per database, monotonic, never reused, < 2^63, high-water mark in commit frames and `CONTROL`. | `storage.md:191-195` |
| TS-11 | Change: each checkpoint saves the statistics and the counters. | `transactions-and-recovery.md:103` |
| TS-12 | Integrity check adds cross-tree checks: adjacency against edges, each index against data. | `storage.md:242-245` |
| TS-14 | Private write sets with reservations (write, before-image, write-key entry); immutable stamped entries; mutable records with newest committed version in page plus in-memory old-version chains; committed write-key sets; GC after commits and every 30 s under the commit latch; no abort of others, no spill; snapshot = highest published `commit_ts`. Recovery starts with an empty old-version store (TS-13 step 7). | `transactions-and-recovery.md:176-188`, `:150` |
| TS-15 | Read sets as `(tree, key range)` intervals with merging on memory refusal; validation under the latch after `commit_ts`; 25N11 on overlap; none for read-only or SI; lock table X/S on six resource kinds held to commit end; write-write rule (lock on element with `version_ts` > snapshot → 25N11); deadlock DFS when a request would wait, requester aborted with 50N05. | `transactions-and-recovery.md:206-223` |
| TS-16 | Complete commit protocol steps 1–11; change: step 6 copies each record's before-image into reserved old-version memory; steps 4, 5, 10 added; SI writers run every step except 4. | `transactions-and-recovery.md:243-281` |
| TS-17 | Catalog tree with stamped entries joining the write set; `(parent_id, name)` → object; reverse entries; token tables; index/constraint definitions; default schema and graph at create; `DROP GRAPH` stamps, GC frees trees; statistics in memory, updated at apply, saved at checkpoint, estimates after crash until rebuilt. Closed-graph-type check is Sprint 3 (comment #6). | `catalog-and-indexes.md:16-24` |
| TS-18 | Index kinds (lookup, single-property range, composite, Hilbert point with exact post-filter); index used only from its ONLINE snapshot; online build steps 1–7 with a logged side tree, unpinned scan, external sort, bottom-up build, idempotent replay, ONLINE commit, rebuild after crash; constraints: `(constraint, value)` X lock and recheck at apply, existence and type at commit, creation under X schema lock, failure leaves no constraint. | `catalog-and-indexes.md:37-61`, `:77` |
| TS-21 | 24-byte `Value`; temporal via `jiff` from OS zone data, UTC default; points with 4 SRIDs and Neo4j distances; record encoding; order-preserving index-key encoding; lexical forms; open points wait for BR-GQL-002 approval. | `values-and-types.md:16-35` |
| TS-30 | (exit #8, #7) `crate::sync` facade; `loom` for commit latch and publish order, lock table, deadlock detection, side tree; `shuttle` for commit latch, WAL writer, checkpointer; Hermitage and Elle isolation oracles; stress profile. | `quality-and-testing.md:48-55, 85-86` |

### Functional and non-functional requirements

| Id | Demands | Acceptance (FS line) | Verified |
|---|---|---|---|
| FR-TX-001 | ACID transactions. | NFR-VER-004 tests hold A, C, I, D (FS:197) | S2 |
| FR-TX-002 | Commit returns after its log records are synced; group commit allowed. | flush covers records (FS:199) | S2 (re-run of Sprint 1 tests) |
| FR-TX-003 | Serializable by default. | non-serial combined effect → one fails (FS:201) | S2 |
| FR-TX-004 | Snapshot isolation per transaction, Rust API only; Bolt always serializable. | SI write skew both commit (FS:203); Bolt part Sprint 6 | S2 (Bolt part Sprint 6) |
| FR-TX-005 | Wait for a held write lock. | T2 waits, continues after T1 ends (FS:205) | S2 |
| FR-TX-006 | Reads and writes never wait for each other. | read returns last committed state immediately (FS:207) | S2 |
| FR-TX-007 | Detect deadlocks; abort the requester with 50N05. | T2 aborted, T1 continues (FS:209) | S2 (error kind carries 50N05) |
| FR-TX-008 | Serialisation failure is GQLSTATUS 25N11. | write skew → at most one commits (FS:211) | S2 (error kind carries 25N11) |
| BR-TX-001 | No automatic retry. | the failed transaction ran exactly once (FS:213) | S2 |
| FR-TX-009 | Commit-time read-set validation; abort on overlap. | T1 fails, m invisible (FS:215) | S2 |
| FR-TX-010 | Read-only transactions not validated, read start snapshot. | T1 commits, reads pre-start state (FS:217) | S2 |
| FR-STO-003 | Many concurrent readers and writers. | 16 threads, disjoint data, all present (FS:190) | S2 |
| FR-LIM-005 | Old versions over the cap fail the needing statement; no abort of others; no spill. | long reader continues, statement fails, nothing written to storage (FS:230) | S2 (internal operation; GQLSTATUS S3) |
| FR-LIM-007 | One transaction's writes plus old versions fit the cap. | update-all statement fails, nothing changed (FS:234) | S2 (internal operation; statement S3) |
| FR-TYPE-001 | GV39, GV40, GV41 temporal types with IANA zones. | zoned date-time round trip (FS:165) | S2 |
| FR-TYPE-002 | Point2D/3D in SRIDs 7203, 9157, 4326, 4979. | round trip per CRS (FS:167) | S2 |
| FR-TYPE-003 | Element identifier stable for the element's life. | same id after many changes and reopen (FS:169) | S2 |
| FR-TYPE-004 | Distance; null across CRSs. | (0,0)-(3,4) = 5; mixed → null (FS:171) | S2 (function; GQL call S3) |
| FR-TYPE-005 | Point index serving distance queries. | indexed result = unindexed (FS:173) | S3 ("a query"); S2 verifies index range = full scan |
| FR-TYPE-006 | Current time from the OS clock, no clock abstraction. | within 1 s of OS time (FS:175) | S3 ("a request"); S2 verifies the clock source |
| FR-TYPE-007 | IANA zone data from the OS. | offset matches host `tzdata` (FS:177) | S3 ("a request"); S2 verifies via `jiff` |
| BR-TYPE-001 | No element identifier reuse. | no new element gets a deleted id (FS:179) | S2 |
| FR-DB-001 | A database is one full GQL catalog. | 2 schemas + 3 graphs belong to it (FS:45) | S2 via internal API |
| FR-DB-005 | Default schema and graph at create. | session home fallback (FS:53) | S3 (session); S2 verifies both objects exist |
| FR-CAT-001 | Full catalog: directories, schemas, graph types, graphs. | listed by procedure (FS:105) | S3 (procedure); S2 internal listing |
| FR-CAT-002 | Persist the catalog. | present after reopen (FS:107) | S2 |
| FR-CAT-004 | DDL and data in one transaction; rollback undoes both. | graph + nodes rolled back (FS:111) | S2 via internal API |
| FR-IDX-001 | Index kinds: single, composite, lookup, range, point. | listing procedure lists 5 (FS:118) | S3 (procedure); S2 creates and queries each |
| FR-IDX-002 | Constraints: unique, existence, type, node key. | node-key violations fail (FS:120) | S2 internal write (GQLSTATUS S3) |
| FR-IDX-003 | Each index/constraint on one graph. | duplicates in g2 allowed (FS:122) | S2 |
| FR-IDX-004 | Violated data fails creation; no constraint exists. | (FS:124) | S2 |
| FR-IDX-005 | Writes accepted during an index build. | write commits without waiting, index includes it (FS:126) | S2 |
| FR-IDX-006 | Constraint creation blocks only writers of its label/type. | L waits, M commits (FS:128) | S2 |
| FR-AUTH-002 | Record an owner on each catalog object; pass it to authorisation. | request names owner (FS:79) | S3 (authorisation); S2 records and reads the owner |
| NFR-PERF-003 | CPU, memory, I/O efficiency primary, MVCC-compatible. | none (NFS:20) | Structure in S2; measurement Sprint 6 |
| NFR-PERF-004 | Layout MVCC-compatible, chosen by device I/O then CPU (layout B). | none (NFS:21) | Structure in S2; measurement Sprint 6 |
| NFR-PERF-005 | Graphs far larger than memory, judged at 1–10 % cache. | none (NFS:23) | Structure in S2 (bottom-up build, no pinned snapshot); measurement Sprint 6 |
| BR-GQL-002 | (risk, comment #7) each implementation-defined item approved by the user before use. | (FS:97) | S2 task #26 |

**Exit conditions (#8) and their Sprint 2 form:** loom models (commit latch and publish order, lock table, deadlock detection, side tree); write skew 25N11 / SI both commit; closed wait cycle → requester 50N05; 16 disjoint writers commit; reads never wait; cap failures only for the operation; constraint creation blocks only its label/type; index build blocks no writer; Sprint 1 crash, backup and restore suites with transactional workloads. **Deferred to Sprint 6 (comment #54):** the laboratory report on traversal device I/O and CPU at 1/10/100 % cache against M-LAYOUT layout B with properties; closing unverified point 6; data for points 3, 4, 13. **Unverified point 7** (side-tree protocol) is closed in Sprint 2 by correctness evidence (loom model and crash sweep), which TS-18 (`catalog-and-indexes.md:77`) names as its required instrument; this is not a timing measurement.

## 2. Code reconnaissance

Sprint 1 code: 26 573 lines (`find src -type f | xargs wc -l`). `src/tx/` holds only `commit.rs` (176 lines) and its tests (1 588 lines). No `catalog`, `index`, `types` or `sync` module exists; `std::sync` is imported directly in 89 places (`grep -rn 'std::sync' src | wc -l`), and `thread_local!` is used at `src/storage/btree/mod.rs:421` and `src/storage/freelist.rs:393`.

### Changes named in scope comment #6

| Change | Files and functions that change | Existing tests and suites to re-run |
|---|---|---|
| **Before-images in commit step 6 (TS-16)** | `src/tx/commit.rs:95-157` (`commit_steps`, `commit_latched`: today steps 2, 3, 6–9 only, doc at `:1-19`); `src/storage/btree/mod.rs:83-92` (`Write` has only put/delete) and `:107-135` (`Undo`); `src/storage/btree/write.rs:172-308` (`apply_batch`, `run`: the undo must also drop the old versions it added, TS-16:261), `:563-625` (`put`, `delete` must return the replaced record and write stamps); `src/storage/btree/node.rs:1-41` (leaf format: no stamp fields today) | `tx::commit` (all 1 588 lines, incl. `audit_a_failed_capture_never_reaches_the_wal`, `sixteen_threads_committing_disjoint_batches_apply_each_exactly_once`), `storage::btree`, `storage::btree::bulk`, `storage::check`, `suites::fault` (`Arm::FailCopy`, `fault.rs:168-175`), stress profile (`.config/nextest.toml`) |
| **Statistics saved by the checkpoint (TS-11)** | `src/storage/checkpoint.rs:148-241` (`run_with`; doc `:1-2` says "Saving statistics arrives with Sprint 2"); step order at `:157` (1), `:185` (3), `:187` (4), `:201` (5); `src/storage/engine.rs:533-638` (`start`: load at open), `:676` (`close`) | `storage::checkpoint` (incl. `fuzzy_checkpoints_under_concurrent_commits_lose_nothing`), the R3 call-order test in `suites::fault`, `storage::recovery`, `storage::engine`, `ops::backup` (backup starts with `run_held`, `checkpoint.rs:144`) |
| **Page reuse only after the GC horizon (TS-05)** | `src/storage/freelist.rs:262-313` (`reusable`, `allocate` take a `horizon` in batch numbers), `:373-462` (`Readers` registry of batch numbers with SeqCst fences); `src/storage/btree/write.rs:207, 266, 301` (batch number = `applied + 1`, stored on success and on undo), `:402-413` (`extend_fence`); `src/storage/btree/mod.rs:634-647` (`Store::reader`, `Store::horizon`) | `storage::freelist` (incl. `registry_horizon_is_the_lowest_registered_value`), `storage::btree` (`concurrent_readers_during_splits_see_every_committed_key_exactly_once`, `a_scan_skips_keys_inserted_behind_it_into_a_range_freed_pages_gave_away`, `tests.rs:581, 973`), `tx::commit` fence tests (`a_page_taken_by_an_undone_batch_waits_for_earlier_readers`, `pages_freed_by_an_undurable_commit_keep_the_undo_image_once_it_is_durable`), stress profile |

Also touched: recovery step 7 (`src/storage/recovery.rs:2` names it as Sprint 2); the GC thread joins `lpg-wal` and `lpg-checkpoint` in `engine.rs:624-636`; WAL groups are closed only when a commit joined (`Core::close_group`, `engine.rs:252-260`; `OpenGroup::has_commits`, `wal.rs:380`), which GC changes need (gap 4); publication is the WAL writer's `published` atomic (`wal.rs:523-524, 707, 977`).

### Reusable Sprint 1 pieces

* **Online index build (TS-18 step 4):** `Sorter`/`Sorted` external merge sort through the storage trait with memory from the accountant (`src/storage/sort.rs:179-346, 562-597`); bottom-up build `Engine::bulk_build_pages` and `Built::install` (`src/storage/btree/bulk.rs:479, 424`), which already commit through `commit_latched` (`commit.rs:114-123`).
* **Integrity check:** `Store::check` (`src/storage/check.rs:460`), amcheck-style invariants and page membership; cross-tree checks are announced at `check.rs:34-35`.
* **Crash instruments:** `FaultStorage` (`src/storage/fault.rs`), the fault suite with `Workload` (`src/suites/fault.rs:154-164`) and `Arm` (`:168-175`), the `kill -9` suite (`src/suites/kill.rs`), `suite_dirs` choosing `LPG_LAB_EXT4`/`LPG_LAB_XFS` or `/tmp/lpgram` (`src/suites/mod.rs:23-40`), the child re-exec helper (`src/test_support/child.rs`), `NoFlush` storage (`test_support/mod.rs`).
* **Pause points:** `TestPoints` (`wal.rs:500-526`), `after_apply` (`commit.rs:136`), `checkpoint_revisit` (`checkpoint.rs:176`): reused for deterministic reader/committer and builder interleavings.
* **Stress profile:** `.config/nextest.toml` and `.config/stress-copies.sh` (16 copies, `LPG_SEED`).
* **Accountant:** `MemoryAccountant::try_reserve`/`Reservation` (`src/storage/memory.rs:145-181`) for write sets, old versions, read sets, write-key sets.
* **Varints:** `put_varint`/`get_varint` (`src/storage/btree/node.rs:104-119`), so no varint crate is needed.

## 3. Specification gaps

### 3.1 Decidable from references (task #25 records each decision)

| # | Gap (cited) | Proposed decision | Reference |
|---|---|---|---|
| 1 | Which trees carry which stamps, and how a record's `version_ts` relates to `created_ts` (TS-06 `storage.md:133, 138`; TS-14 `transactions-and-recovery.md:179-180`). | Immutable-entry trees (adjacency, index, catalog, side tree) carry `created_ts`/`deleted_ts`. Record entries (node, edge) keep `version_ts` in the `created_ts` field plus `deleted_ts`; a creation's before-image is "absent". | Memgraph delta chains with object-create/delete deltas (R-MG `src/storage/v2/delta.hpp`, cited in TS-14:191). |
| 2 | Stamp encoding in compressed leaves and the format version: leaves change while `FORMAT_VERSION` is 1 (`src/storage/engine.rs:49`). | Two flag bits in the entry `tag` varint (frozen, deleted) and varint stamps; format version stays 1, because no release exists (no git tag; crates.io: "crate `lpg` does not exist", 2026-10-07), as TS-06:185 allows. | TS-06:185, TS-28; LevelDB entry tag (`table/block_builder.cc`). |
| 3 | Page-reuse registry under MVCC: TS-05:73-77 registers the snapshot, but the index builder scans with no snapshot (TS-18 `catalog-and-indexes.md:50`), and the undo fence (TS-16:262) needs a counter that undone batches advance, while `commit_ts` is given back (TS-16:245; code `write.rs:207, 266, 301`). | One registry in the commit-timestamp domain: a transaction registers its snapshot; an unpinned reader (builder, integrity check) registers the published timestamp when its operation starts; freed pages are tagged with the freeing `commit_ts`; the undo fence keeps its own batch counter. | PostgreSQL recycles a deleted nbtree page only when its delete XID is invisible to every snapshot (`src/include/access/nbtree.h:290-316`, `GlobalVisCheckRemovableFullXid`); LMDB oldest reader (TS-05:113). |
| 4 | GC page changes without a commit: TS-09:30, 34 puts them in a group, but the writer closes only groups a commit joined (`engine.rs:252-260`, `wal.rs:380`); GC-freed pages have no `commit_ts` tag. | A GC batch joins the open group as a member with no timestamp; the writer closes any group with a member; its freed pages are tagged with the last drawn `commit_ts` + 1. | PostgreSQL logs pruning as its own WAL record, outside any commit (`src/backend/access/heap/pruneheap.c:1359, 2619`). |
| 5 | Snapshot registration racing a GC pass (TS-14:182-188 silent). | Reuse the `Readers` store-buffering protocol (SeqCst CAS, then fence; the GC fences before reading the slots, `freelist.rs:420-461`); prove it with a `loom` model. | Sprint 1 `Readers` (`freelist.rs:437-441, 454-461`). |
| 6 | Read-set intervals: bound inclusivity, the key that each write registers in the write-key set (record, adjacency, index, catalog), and which ranges merge first on a refused reservation (TS-15:206-208). | Half-open `[lo, hi)` byte intervals per tree; every batch entry's `(tree, key)` is a write key; merge the two closest intervals of the largest tree first, then whole trees. | fjall `OptimisticTxDatabase` conflict manager (R-FJALL `src/tx/optimistic/conflict_manager.rs`, TS-15:232). |
| 7 | Lock queue policy: whether a new S waits behind a queued X, which FR-IDX-006's acceptance needs (FS:128); S→X upgrade (TS-15:211-223 silent). | A request waits if it conflicts with a granted lock or an earlier waiting request; waits-for edges go to both. An upgrade waits for the other S holders; two upgraders form a cycle and the requester aborts. | PostgreSQL: a request conflicting with `lock->waitMask` waits (`src/backend/storage/lmgr/lock.c:1140`); Neo4j Forseti refuses new shared holders once the update flag is set (`SharedLock.java:199-210`) and upgrades in place (`ForsetiClient.java:782-800`). |
| 8 | Write-write rule for the S node-existence lock: TS-15:222 compares `version_ts`, which label and property changes also raise, so an edge create would fail on a mere property change of its endpoint. | For existence locks, compare the endpoint's `deleted_ts` (and creation) only; for content and edge X locks, `version_ts`. | PostgreSQL `FOR KEY SHARE` conflicts only with key updates and deletes (`heapam.c` tuple lock table; line to record in #25). |
| 9 | Lock-table memory is not among the cap's users (TS-08 `storage.md:209`). | A lock entry is reserved with the write that takes it (TS-14:177); a refused reservation fails that operation. | PostgreSQL's bounded lock table fails the requester with "out of shared memory" (`max_locks_per_transaction`; line to record). |
| 10 | Name uniqueness under SI: two transactions creating the same token or catalog name both commit (TS-17:17-19; TS-15 table has no name resource). | Tokens: created in an inner transaction under a token latch, never rolled back. Catalog names: X lock on `(parent_id, name)` plus the write-write rule. | Neo4j creates tokens in an "inner transaction" under a synchronized holder (`token-api/.../TokenCreator.java:34`, `CreatingTokenHolder.java:63`); PostgreSQL unique catalog indexes (to record). |
| 11 | When element, catalog-object and token identifiers are drawn (TS-07 `storage.md:192-193`). | At write time from an in-memory atomic; the commit raises the high-water mark (`Ids`, `commit.rs:29-40`); ids of aborted or unrecovered transactions are skipped, never reused (BR-TYPE-001). | Sequences in PostgreSQL and Neo4j id generators are non-transactional (to record). |
| 12 | Where statistics are saved and how they are rebuilt after a crash ("by sampling", TS-17:24). | A statistics tree written by an internal logged batch at the start of each checkpoint, before step 1, so its images lie below the redo point; loaded at open; after recovery an exact background recount replaces "sampling" (amends TS-17:24). | Neo4j counts store is a checkpointed tree rebuilt by a full count when missing (path to record in #25); NFR-SIMP-001 (counting needs no sampler). |
| 13 | Edge counts per `(label, edge type, direction)` when a node gains or loses a label (TS-17:24). | The committer updates the edge counts from the node's adjacency prefix in the same apply. | Neo4j counts updates on label change (to record in #25). |
| 14 | Index build protocol details (TS-18 `catalog-and-indexes.md:46-54`): how a committer learns of a POPULATING index; the race between the end of replay and the ONLINE commit; stamps of bulk-built entries; who runs the build and its restart (TS-02 `architecture.md:50` lists three threads). | Committers read the latest committed catalog cache under the latch; the ONLINE commit replays the side tree's tail under the latch; bulk-built entries are frozen (no stamps); one `lpg-index-build` thread per build, started by the create call and by open for every POPULATING index (TS-02 updated). | Neo4j queues updates and flushes them into the populated index (`MultipleIndexPopulator.java:92-118`, TS-18:65). |
| 15 | A constraint's owned index is built under the X schema lock (TS-18:57-60). | No side tree: no writer of the label or type can run. | Follows from TS-18:60. |
| 16 | Hilbert precision and extents per CRS (TS-18:44, TS-21:28 silent). | 60 bits total (30 per axis in 2D, 20 in 3D); WGS-84 longitude ±180, latitude ±90; every other axis ±1 000 000; points outside clamp to the edge cells; the exact-distance post-filter keeps results correct. Fixed, not configurable (NFR-SIMP-001). | Neo4j `SpaceFillingCurveSettings.java:60, 69`; `EnvelopeSettings.java:30-35, 69-77`; `HilbertSpaceFillingCurve2D.java:112`. |
| 17 | TS-31 (`dependencies.md:20-36`) omits `shuttle`, which TS-30 (`quality-and-testing.md:86`) lists as a dev dependency; `jiff` 0.2.38 supersedes 0.2.37 (`values-and-types.md:44`). | Add `shuttle` 0.9.5 to TS-31; record `jiff` 0.2.38. | crates.io data, 2026-10-07. |
| 18 | Cross-tree integrity semantics under stamps (TS-12 `storage.md:245`). | Every live edge record has exactly two live adjacency entries with its creation stamp; deleted edges have matching deletion stamps; every index entry matches a record at the same stamps, and vice versa. | PostgreSQL amcheck `heapallindexed` option (`contrib/amcheck/verify_nbtree.c`, cited by `check.rs:16-17`; the option itself not read in source). |

### 3.2 Needs a user decision (task #25)

19. **Owner of the default schema and default graph**, which `create` makes with no principal (FR-AUTH-002 FS:78; FR-DB-005 FS:52; TS-17:21; TS-23 `interfaces.md:27`, principal comes from a session). Options:
    * **(a, recommended)** `create` takes the owner principal as a required argument, as `initdb` names the bootstrap superuser that owns the initial objects (PostgreSQL; source line to record). Satisfies "an owner on each catalog object" literally.
    * (b) A "no owner" value, passed to authorisation as absent.
    * (c) A reserved principal name such as `system`.

### 3.3 BR-GQL-002 implementation-defined items that need the user's approval before the encoding task (task #26)

Line numbers are in `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-implementation-defined.xml`.

1. **Cross-type comparison**: IV002 (line 398, inequality of essentially comparable values) and IV010 (line 410, comparison of universally comparable values). (a, recommended) values of different type groups are not comparable: `=` is false, `<` is null; integers and floats compare by numeric value (Neo4j `NumberValue.java:57-69`). (b) a type error. (c) compare by the total order of item 2.
2. **Total order for `ORDER BY` and index keys**: IS001 (line 390, NULL ordering), IA015 (line 98, string padding), IA016 (line 102, trailing X'00' in byte strings). (a, recommended) Neo4j's group order (`ValueGroup.java:55-66`: geometry, zoned date-time, local date-time, date, zoned time, local time, duration, text, boolean, number), NULL last ascending; strings by code point (UTF-8 byte order), no padding; byte strings differing in trailing zeros are different; zoned values by instant then zone; durations by (months, days, seconds, nanoseconds); points by CRS then coordinates; lists lexicographic; NaN above every number and −0.0 = +0.0 (Neo4j position to verify in `Values.COMPARATOR`). (b) the same with NULL first. (c) a different group order chosen by the user.
3. **Numeric types beyond 64 bits and unsigned** (TS-21:35; features GV11, GV13–GV17, GV20, GV25, GV26 in the features insert lines 777-837; ID028 line 174, ID034 178, ID037 182, IL011 362, IL023 382). (a, recommended) not in Sprint 2: INT64 and FLOAT64 only (ID028 = 64-bit two's complement, ID037 = IEEE binary64), type tags reserved; no FR requires wider types and optional features come in later phases (FR-GQL-006). (b) add INT128/UINT128 and UINT64 with Rust's native `i128`/`u128`/`u64` (no crate), defer the rest. (c) all of them with `ethnum` or `bnum`, `rust_decimal`, `half` (§4).
4. **Temporal limits**: IL024 (line 386, fractional seconds precision), IA026 (line 130, leap seconds), ID048 (line 186, default time zone displacement). (a, recommended) nanoseconds, no leap seconds (both as `jiff`), default UTC as TS-21:20 already states. (b) microseconds.
5. **Size limits used by the record encoding**: IL001 (342, label-set cardinality), IL002 (346, property-set cardinality), IL013 (366, string length), IL015 (370, constructed-value cardinality). (a, recommended) bounded by a `u32` length or count field (2^32 − 1) and by the memory cap. (b) lower fixed limits.
6. **Directory nesting depth** IL020 (378), needed by the catalog task. (a, recommended) bounded only by the 1 024-byte key limit (TS-05 `storage.md:64`). (b) a fixed depth.
7. **Confirm** that FR-TX-003 and FR-TX-004 (FS:200-203) record the approval of ID006 (154), IE002 (306) and IE004 (314), so no new question is needed.

### 3.4 Decided in this planning task (no spec change; for task #2's DECISION comment)

* Internal error kinds `Serialization` and `Deadlock` carry their GQLSTATUS codes 25N11 and 50N05 now (exit #8; `interfaces.md:28`); the full mapping stays in Sprint 3 (Sprint 1 decision, comment #4 on task #1). Constraint violations use an internal kind; their GQLSTATUS code is Sprint 3.
* `loom` models run on the real synchronisation components (publisher, lock table, deadlock detector, snapshot registry, side-tree protocol), which are kept free of file I/O so that `loom` can drive them; the rest of the crate still compiles under `cfg(loom)`.
* Elle runs as an external `java -jar` tool (TS-30:55). History files are evidence, not graph data, so they go under `target/lpg-histories/`, never into `/tmp/lpgram`; the recorder renders a `String` and the test helper writes it, keeping `std::fs` out of `src/` outside `src/storage/fs.rs` and `src/lab/` (task #22 grep).
* The timing tools of comment #9 (`perf sched`, `perf trace`, `heaptrack`, throughput benchmarks, cache-ratio runs) are not set up: deferred to Sprint 6 (comment #54).
* The memory-leak binary of R13 (`unsafe impl GlobalAlloc`, TS-30:56) is outside Sprint 2: FR-LIM-005/007 are verified through the accountant.
* New modules follow TS-01 (`architecture.md:40`): `tx` (MVCC, locks), `catalog`, `index`, `types`.

## 4. Crates

Dependency policy: NFR-DEP-001/002 (NFS:52-53), TS-31 allow-list (`dependencies.md:15`), `deny.toml` (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception today).

| Component | Crate (version, licence, downloads, last release) | Use |
|---|---|---|
| Model checking | `loom` 0.7.2, MIT, 69.9 M, 2024-04-23 | **dev dependency** under `cfg(loom)` (TS-30:85) |
| Random schedules | `shuttle` 0.9.5, Apache-2.0, 6.1 M, 2026-10-01 | **dev dependency** under `cfg(shuttle)` (TS-30:86; add to TS-31, gap 17) |
| Sync facade | Turso `core/sync.rs` @ `8cc059b` (MIT, 212 lines) | **copy candidate** (R11), without `lock_arc`, whose `unsafe` transmute (`core/sync.rs:46-58`) `forbid(unsafe_code)` rejects |
| Temporal | `jiff` 0.2.38, Unlicense OR MIT, 216.0 M, 2026-10-06 | **dependency** (TS-21:44), at the latest release |
| Isolation oracles | Turso `core/mvcc/database/hermitage_tests.rs` (921 lines) and `testing/concurrent-simulator/elle.rs` (317 lines), MIT | **copy candidates** (R12), adapted; Elle itself is the external `elle-cli` jar (licence and version unverified, not distributed) |
| MVCC | Turso `core/mvcc` (MIT); fjall `OptimisticTxDatabase`; `concread` 0.5.10 (MPL-2.0, not on the allow-list); `skipdb` 0.2.1 and `txn` 0.1.3 (8 k and 7 k downloads) | **study only** (TS-14:196, TS-15:232; TS-04: none works without its own storage) |
| Lock table, deadlock detection | no crate implements a transactional lock table; `parking_lot`'s `deadlock_detection` covers mutex lock order only and `parking_lot` is excluded (TS-08:226); `petgraph` 0.8.3 cycle detection is a TS-31 runtime crate for Sprint 5 | **own code** (a DFS of a few dozen lines); petgraph not pulled in early (NFR-SIMP-001) |
| Hilbert curve | `fast_hilbert` 2.1.0 (MIT, 2D only, 371 k), `hilbert_2d` 1.1.0 (2D only), `lindel` 0.1.1 (MIT OR Apache-2.0, n-D, 41 k, 2021), `hilbert` 0.1.2 (MIT, 2021) | **study only**; own 2D/3D encoding (TS-18:69) |
| Order-preserving keys | `storekey` 0.11.0 (Apache-2.0, 2.3 M), `memcomparable` 0.2.0 (Apache-2.0, 734 k) | **study only**: the order is lpg's approved total order (gap 3.3 item 2) |
| Varints | `integer-encoding` 4.1.0 (MIT) | **not used**: `node.rs:104-119` already has them |
| Wider numerics (only if task #26 picks option c) | `ethnum` 1.5.3, `bnum` 0.14.4 (MSRV 1.87), `rust_decimal` 1.43.0, `bigdecimal` 0.4.11, `half` 2.7.1 | **dependency only on that approval**; option (b) needs no crate |
| Spatial | `geo` 0.33.1, `rstar` 0.13.0 | **not used** (TS-21:47, TS-18:69) |
| Seeded tests | `rand` 0.10.3 (already a dependency) | **reused**; no property-testing crate is added |

## 5. Task decomposition

Twelve tasks. Ids #24–#35 are provisional: `rmp` assigns them at creation (highest existing id is #23). Every field below is plain Markdown with `*` bullets; §8 checks lengths and the "no line starting with `- `" rule. Every test that writes graph data uses `/tmp/lpgram/<run>/` (≤ 1 GiB), except the runs that need real `fsync` or a named file system, which use `LPG_LAB_EXT4=/data/tmp/lpg-lab` and `LPG_LAB_XFS=/mnt/lpg-xfs`.

**Grouping (CLAUDE.md "Synergy and convergence"):** the spec-gap points are one task, but the BR-GQL-002 approvals are a second one, because merging them would make the MVCC code wait for value-type answers it does not need. MVCC (#27), isolation (#28) and GC (#29) are split because each has its own `loom` model and its own Sprint 1 change. Catalog (#31) comes before the graph trees (#32), because labels, types and property keys are tokens. Indexes (#33) and constraints (#34) are split because a constraint's owned index reuses #33's build. The crash suites and the exit gate are one final task (#35), so that the whole test set runs once on the final code.

---

### #24 CHORE, priority 9, depends on: none

**Title:** Set up the Sprint 2 correctness laboratory: toolchain, loom, shuttle, Elle

**-fr**
```text
**Every Sprint 2 concurrency and isolation claim must be checked by an instrument, not by reasoning.** Before any MVCC code exists, the laboratory needs the latest toolchain, `loom` and `shuttle` reachable through a `crate::sync` facade (TS-30 Concurrency, R11), and a recorder of transaction histories checked by Elle (R12), so that later tasks only add models and scenarios.

* R11 and R12 of `docs/evidence/research/research-04-turso-libsql.md` were approved by the user on 2026-10-03 (task #2, NOTE 71).
* Correctness only: no timing tool and no timing measurement (user decisions 2026-10-03 and 2026-10-07; sprint UPDATE 54).
```

**-tr**
```text
* **Toolchain:** `rustup self update`, `rustup update stable nightly`, keep toolchain 1.89 (MSRV, TS-29). Reinstall `cargo-nextest`, `cargo-llvm-cov` and `cargo-deny` with `cargo install --locked <crate>@<max_stable_version>` when crates.io lists a newer release. Record each version command and output.
* **Dev dependencies** (TS-30): `loom` 0.7.2 under `[target.'cfg(loom)'.dev-dependencies]`, `shuttle` 0.9.5 under `[target.'cfg(shuttle)'.dev-dependencies]`; `[lints.rust] unexpected_cfgs` with `check-cfg = ['cfg(loom)', 'cfg(shuttle)']`. Run `cargo deny check licenses`; add to `deny.toml` only licences from the NFR-DEP-002 list that the new tree brings.
* **`src/sync.rs`** (R11): re-exports `Arc`, `Mutex`, `MutexGuard`, `RwLock`, `Condvar`, `atomic` (with `fence`), `thread` and `thread_local!` from `std`, `loom` or `shuttle` by `cfg`. Starting point: Turso `core/sync.rs` @ `8cc059b` (MIT), copied without `lock_arc` (its `unsafe` transmute, `core/sync.rs:46-58`), with licence, attribution and source in the file header and in this task's log. Replace every `std::sync` import in `src/` (89 today) and the two `thread_local!` uses (`src/storage/btree/mod.rs:421`, `src/storage/freelist.rs:393`).
* **Smoke models:** a `loom::model` and a `shuttle::check_random` of concurrent `MemoryAccountant::try_charge`/`release` (`src/storage/memory.rs:112-142`) that never exceed the cap; test names start with `loom_` and `shuttle_`.
* **History recorder** `src/test_support/history.rs` (`cfg(test)`): list-append and rw-register operations with invoke, ok and fail events, rendered as Elle EDN in a `String`. Starting point: Turso `testing/concurrent-simulator/elle.rs` @ `8cc059b` (MIT), file writing removed. A helper writes the EDN under `target/lpg-histories/` (never `/tmp/lpgram`) and runs `java -jar "$LPG_ELLE_CLI"` with the model and consistency level.
* **Elle:** install the latest `elle-cli` release jar outside the repository, without root (Java `openjdk 25.0.4.1` is installed). If it cannot be fetched or does not run on this Java, record a `FINDING` and stop; never fall back to a weaker checker.
* **Record:** `docs/evidence/lab-sprint-2/report.md` with Environment and Tools sections (name, version, command); no timing.
* Out of scope: `perf`, `heaptrack`, `fio`, benchmarks and cache-ratio runs (Sprint 6).
```

**-ac**
```text
* `rustup check` reports no update. Each cargo tool's `--version` equals its crates.io `max_stable_version`, shown in the report.
* `cargo +1.89 build`, `cargo +stable build`, `cargo nextest run -p lpg` and `cargo deny check licenses` exit 0.
* `grep -rn 'std::sync' src --include=*.rs` matches only `src/sync.rs`.
* `RUSTFLAGS="--cfg loom" cargo test -p lpg --lib --release loom_` and `RUSTFLAGS="--cfg shuttle" cargo test -p lpg --lib shuttle_` each run at least one test and pass. With the cap comparison removed from `try_charge`, both fail, and pass again once it is restored.
* A test records a two-thread rw-register history; `elle-cli` reports it valid. A hand-written history with a G1c cycle is reported invalid.
* The report lists every tool with its version and command. A `FINDING` comment names any missing item.
```

---

### #25 TASK, priority 9, depends on: none

**Title:** Resolve the specification gaps that block Sprint 2 implementation

**-fr**
```text
**Sprint 2 code must implement what the specification states, not assumptions made while coding.** Planning (task #2) found 19 points where `docs/technical-spec/` or `docs/functional-spec/` is silent, ambiguous or in conflict for the Sprint 2 scope. Each point blocks the task named beside it.

Points 1 to 18 are decided from references. Point 19 needs **a user decision**. The implementation-defined value items of BR-GQL-002 are task #26, not this one.
```

**-tr**
```text
Owners: `technical-writer` for `docs/technical-spec/`, `specification-manager` for `docs/functional-spec/`. Decide points 1 to 18 as `CLAUDE.md` "Research and decision-making" requires; the proposal and its reference are in task #2's plan. Record lines marked "to record" from a clone under `/tmp`.

### Decide from references
1. Stamps per tree; record `version_ts` in the `created_ts` field (TS-06, TS-14). Blocks #27.
2. Stamp flags and varints in the leaf tag; format version stays 1, no release exists (TS-06:185, TS-28). Blocks #27.
3. One reuse registry in the commit-timestamp domain; unpinned readers register the published timestamp; the undo fence keeps its batch counter (TS-05:73-77, TS-16:245, 262; PostgreSQL `nbtree.h:290-316`). Blocks #29.
4. GC batches join a WAL group without a timestamp; their freed pages get the last drawn `commit_ts` + 1 (TS-09:30, 34; `engine.rs:252-260`; PostgreSQL `pruneheap.c:2619`). Blocks #29.
5. Snapshot registration against the GC horizon uses the `Readers` fence protocol (`freelist.rs:420-461`). Blocks #27, #29.
6. Read-set intervals `[lo, hi)`, write keys, merge order (TS-15; fjall `conflict_manager.rs`). Blocks #28.
7. A request waits behind earlier conflicting waiters; S to X upgrade in place (PostgreSQL `lock.c:1140`; Forseti `SharedLock.java:199-210`). Blocks #28.
8. The write-write rule of the S node-existence lock checks deletion, not `version_ts` (TS-15:222; PostgreSQL `FOR KEY SHARE`, to record). Blocks #28, #32.
9. Lock entries are reserved with the write that takes them (TS-08:209). Blocks #28.
10. Tokens created in an inner transaction under a latch; catalog names under an X `(parent_id, name)` lock (TS-17; Neo4j `CreatingTokenHolder.java:63`). Blocks #31.
11. Identifiers drawn at write time; commits raise the high-water mark (TS-07). Blocks #31, #32.
12. Statistics tree written at checkpoint start, loaded at open, exact background recount after recovery, replacing "sampling" (TS-11:103, TS-17:24; Neo4j counts store, to record). Blocks #32.
13. Edge counts updated from the adjacency prefix on a label change (TS-17:24). Blocks #32.
14. Index build: catalog cache under the latch, tail replayed in the ONLINE commit, frozen bulk entries, `lpg-index-build` thread restarted at open (TS-18:46-54, TS-02:50). Blocks #33.
15. A constraint's owned index needs no side tree (TS-18:57-60). Blocks #34.
16. Hilbert: 60 bits, WGS-84 ±180/±90, other axes ±1 000 000, clamped (Neo4j `SpaceFillingCurveSettings.java:60, 69`, `EnvelopeSettings.java:30-35`). Blocks #30.
17. Add `shuttle` 0.9.5 to TS-31; record `jiff` 0.2.38 in TS-21 and TS-31. Blocks #30.
18. Cross-tree integrity rules with stamps (TS-12:245). Blocks #32, #33.

### Ask the user
19. Owner of the default schema and graph made by `create`, which has no principal (FR-AUTH-002, FR-DB-005, TS-17:21). Options: (a, recommended) `create` takes the owner principal (PostgreSQL `initdb` bootstrap superuser); (b) no owner, passed to authorisation as absent; (c) a reserved principal name. Blocks #31.

Out of scope: BR-GQL-002 items (task #26) and any design for Sprints 3 to 6.
```

**-ac**
```text
* Each of points 1 to 19 is answered in the `docs/technical-spec/` or `docs/functional-spec/` section that it cites, with its evidence: the reference studied (project, file and line) or the user's decision and its date.
* A `DECISION` comment on this task maps each point to the file and heading that now answer it.
* The closing commit changes only files under `docs/technical-spec/` and `docs/functional-spec/` (`git show --stat`).
```

---

### #26 TASK, priority 9, depends on: none

**Title:** Obtain the user's approval of the implementation-defined value items

**-fr**
```text
**The record and index-key encodings fix comparison and order on disk, so every implementation-defined choice they depend on must have the user's explicit approval first** (BR-GQL-002; sprint DECISION 7). TS-21 lists these choices as open (`docs/technical-spec/values-and-types.md:35`). This task blocks the encoding task #30, and item 6 also blocks the catalog task #31.
```

**-tr**
```text
Owner: `specification-manager`, with `technical-writer` for TS-21. Ask the user each item below with two to four options, the recommended one first, each with its ISO code, the insert line in `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-implementation-defined.xml`, and the reference behaviour. Record the answers, then close.

1. Cross-type comparison, IV002 (line 398) and IV010 (line 410). Recommended: different type groups are not comparable (`=` false, `<` null); integers and floats compare by value (Neo4j `NumberValue.java:57-69`).
2. Total order for `ORDER BY` and index keys: IS001 (390), IA015 (98), IA016 (102). Recommended: Neo4j group order (`ValueGroup.java:55-66`), NULL last ascending, strings by code point without padding, trailing X'00' significant, zoned values by instant then zone, durations by (months, days, seconds, nanoseconds), points by CRS then coordinates, lists lexicographic, NaN above every number, -0.0 equal to +0.0 (the Neo4j NaN position is verified in source first).
3. Numeric types beyond 64 bits and unsigned (TS-21:35; GV11, GV13 to GV17, GV20, GV25, GV26; ID028 174, ID034 178, ID037 182, IL011 362, IL023 382). Recommended: INT64 and FLOAT64 only in Sprint 2, type tags reserved. Other options: native `i128`/`u128`/`u64`; all, with crates.
4. Temporal: IL024 (386), IA026 (130), ID048 (186). Recommended: nanoseconds, no leap seconds, UTC default (TS-21:20).
5. Size limits: IL001 (342), IL002 (346), IL013 (366), IL015 (370). Recommended: a `u32` length or count field and the memory cap.
6. Directory nesting, IL020 (378). Recommended: only the 1 024-byte key limit (TS-05).
7. Confirm that FR-TX-003 and FR-TX-004 already record approval of ID006 (154), IE002 (306) and IE004 (314).

Out of scope: every other implementation-defined item, which belongs to the sprint that first needs it.
```

**-ac**
```text
* TS-21 "Open points" is replaced by the approved choice for items 1 to 6, each with its ISO code and the date of the user's decision. BR-GQL-002 or OQ-005 in `docs/functional-spec/` lists the approved codes, including item 7.
* A `DECISION` comment on this task quotes the user's answer to each item.
* The closing commit changes only files under `docs/` (`git show --stat`).
```

---

### #27 TASK, priority 8, depends on: #24, #25

**Title:** Implement MVCC: stamped entries, private write sets, old versions, snapshots

**-fr**
```text
**A reader must see one committed snapshot, never part of a commit or an unpublished one, and a writer's changes must stay private until its commit** (FR-TX-001, FR-TX-006, TS-14). Writes and the old versions they create must fit the memory cap; beyond it only the operation that needs the memory fails, no other transaction is aborted and nothing is spilled (FR-LIM-005, FR-LIM-007).

This changes Sprint 1 code: before-images in commit step 6 (TS-16), stamps in the compressed leaves (TS-06), and an empty old-version store at recovery (TS-13 step 7).
```

**-tr**
```text
* **Leaves** (`src/storage/btree/node.rs:1-41`): stamps as #25 points 1 and 2 decide; the directory and the free list stay unstamped. `Write` (`src/storage/btree/mod.rs:83-92`) gains: insert a stamped entry, set a delete stamp, and replace a record returning its before-image; `put` and `delete` (`write.rs:563-625`) implement them.
* **Transactions** (`src/tx/`): `begin(isolation)` with the snapshot = highest published `commit_ts` (`Wal::published`, `wal.rs:707`), registered as #25 point 5 decides; serializable by default, snapshot isolation by choice; private write set per tree, merged over the snapshot on reads; each write reserves itself, its before-image and its write-key entry (TS-14:177) through `MemoryAccountant::try_reserve` (`memory.rs:145`). A refused reservation fails that operation with `MemoryLimit` and aborts the transaction (FR-LIM-001).
* **Old-version store:** in memory, per `(tree, key)`, newest to oldest, each version with its `version_ts`; a read whose snapshot is below the page's `version_ts` walks the chain.
* **Commit** (`src/tx/commit.rs:95-157`): step 1 returns at once for a transaction that wrote nothing; the batch is built from the write set; step 6 stamps entries with `commit_ts` and moves each before-image into the old-version memory reserved earlier, before the page changes; the undo also removes them (TS-16:261). Steps 4, 5 and 10 belong to #28.
* **Recovery:** step 7 starts with an empty store (`recovery.rs:2`, `engine.rs:533-638`).
* **Crate-internal API for tests:** `begin`, `get`, `scan_prefix`, `put`, `delete`, `commit`, `abort` over trees.
* Re-run the Sprint 1 suites named in task #2's plan §2 for this change, and add the new concurrency tests to the stress profile (`.config/nextest.toml`).
```

**-ac**
```text
* `cargo nextest run -p lpg` passes, every Sprint 1 test included. New tests show that:
  * while a commit is paused at `after_apply` (`src/tx/commit.rs:136`), a reader at an earlier snapshot reads none of its changes, and after publication a new transaction reads all of them;
  * a key updated by three commits, read at four snapshots, returns each version once;
  * a transaction reads its own writes; no other transaction does; an abort leaves no trace;
  * a transaction that wrote nothing commits without drawing a `commit_ts`;
  * with a long reader holding old versions and the cap nearly used, a writer's operation fails with `MemoryLimit`, its transaction is aborted, the reader still reads its snapshot, and the accountant never exceeds the cap (FR-LIM-005);
  * a transaction whose writes exceed the cap fails and no change is visible (FR-LIM-007);
  * a page-copy failure in step 6 (`FailCopy`) leaves no stamp and no old version;
  * after a drop without close, reopen shows exactly the stamped committed entries and an empty old-version store.
* The stress profile runs the new concurrency tests with no failure.
* Inserting the before-image after the page change (mutation) makes the paused-commit test fail; the original passes again.
```

---

### #28 TASK, priority 8, depends on: #27

**Title:** Implement serializable validation, snapshot isolation, locks and deadlocks

**-fr**
```text
**Transactions are serializable by default, snapshot isolation is available per transaction, writers wait for write locks, and deadlocks abort the requester** (FR-TX-003 to FR-TX-010, BR-TX-001, FR-STO-003, TS-15, TS-16 steps 4, 5 and 10).

Exit conditions served (sprint DECISION 8): write skew fails one transaction with GQLSTATUS 25N11 under serializable isolation and lets both commit under snapshot isolation; a closed wait cycle aborts the requester with 50N05; 16 threads writing disjoint data all commit; reads never wait for writes; `loom` models pass for the commit latch and publish order, the lock table and deadlock detection.
```

**-tr**
```text
* **Read sets** (TS-15:206-208, #25 point 6): intervals reserved from the accountant, merged when a reservation is refused.
* **Write-key sets:** registered at step 5 under the commit latch, withdrawn by the undo. **Validation** at step 4 against every commit in (snapshot, `commit_ts`); an overlap fails with error kind `Serialization` (GQLSTATUS 25N11) and aborts. No validation for read-only or snapshot-isolation transactions.
* **Lock table** `src/tx/lock.rs`: the resources and modes of TS-15:213-220, with #25 points 7 to 10; locks held until step 10. Write-write rule (TS-15:222, #25 point 8): kind `Serialization`. **Deadlocks:** depth-first search of the waits-for graph from a requester that would wait; a closed cycle aborts the requester with kind `Deadlock` (GQLSTATUS 50N05); waiters wake when their blocker ends or aborts.
* No retry anywhere (BR-TX-001).
* **`loom` models** (`cfg(loom)`): the publisher of the commit latch and publish order (published timestamps increase and never exceed the durable ones); lock grant and release; deadlock detection on two- and three-transaction cycles. **`shuttle`:** random schedules of the commit latch, the WAL writer and the checkpointer (TS-30:51).
* **Hermitage** (R12): adapt Turso `core/mvcc/database/hermitage_tests.rs` @ `8cc059b` (MIT; attribution and source recorded) to the internal API: G0, G1a, G1b, G1c, OTV, PMP, P4, G-single, G2-item, G2. Expected: serializable prevents all; snapshot isolation allows only G2-item and G2.
* **Elle** (R12): list-append and rw-register histories of 8 threads, through the #24 recorder, at both levels.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. New tests, synchronised with barriers and pause points, never sleeps, show:
  * write skew: under serializable exactly one commits and the other fails with `Serialization` (25N11), its writes never visible; under snapshot isolation both commit (FR-TX-004, FR-TX-008);
  * FR-TX-009: T1 reads n and writes m; T2 changes n and commits; T1's commit fails and m's change is never visible;
  * FR-TX-010: a read-only T1 commits and read only the state before it started;
  * FR-TX-005: T2 waits for T1's lock and continues after T1 ends;
  * FR-TX-007: T1 holds A and waits for B; T2 holds B and requests A; T2 fails with `Deadlock` (50N05) and T1 completes;
  * FR-TX-006: a read completes while a writer holds the X lock with uncommitted writes, and while a commit is paused under the latch;
  * FR-STO-003: 16 threads with disjoint writes all commit and every write is present after reopen;
  * BR-TX-001: a failed transaction ran its operations once.
* The three `loom` models pass. Each fails under its mutation (publish before durable; grant ignoring waiters; search ignoring waiting requests).
* The `shuttle` tests pass at least 1 000 schedules each.
* Every Hermitage scenario gives its expected outcome at each level. Elle reports no anomaly for the serializable histories, and reports G2-item for a snapshot-isolation write-skew history.
* The stress profile runs the new concurrency tests with no failure.
```

---

### #29 TASK, priority 8, depends on: #28

**Title:** Implement garbage collection and page reuse after the GC horizon

**-fr**
```text
**Old versions, committed write-key sets and deleted entries must be freed as soon as no snapshot needs them, and a freed page may be reused only after the garbage-collection horizon** (TS-14 Garbage collection; TS-05 Reuse). Memory is released without aborting any transaction and without spilling to storage (FR-LIM-005).

This changes Sprint 1 code: page reuse, which today follows a registry of write-batch numbers (`src/storage/freelist.rs:262-313, 373-462`; `src/storage/btree/write.rs:207, 266, 301`).
```

**-tr**
```text
* **Thread** `lpg-gc` (TS-02:50): after commits (notified) and every 30 seconds (overridable under `cfg(test)`, as the checkpoint interval is). It runs under the commit latch as a batch that joins a WAL group without a timestamp (#25 point 4), stops at close and drop, and never runs on an unusable database.
* **Work** (TS-14:182-186): free old versions below the oldest active snapshot; free write-key sets older than the oldest snapshot of an active serializable transaction; remove entries whose `deleted_ts` is below the oldest snapshot, logged like any page change; drop `created_ts` at or below the horizon whenever a page is rewritten; free the pages of a dropped tree once the horizon passes its drop (used by #31).
* **Reuse horizon** (#25 points 3 and 5): the registry holds snapshots and the published timestamps of unpinned readers; `FreeList` tags become commit timestamps; the undo fence keeps its batch counter (`write.rs:402-413`).
* **`loom` model:** a transaction registering its snapshot against a GC pass computing the horizon.
* Re-run `storage::freelist`, `storage::btree` (with `concurrent_readers_during_splits_see_every_committed_key_exactly_once` and `a_scan_skips_keys_inserted_behind_it_into_a_range_freed_pages_gave_away`), the `tx::commit` fence tests, the `suites::fault` sample and the stress profile.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes, every Sprint 1 test included. New tests show that:
  * an old version a running snapshot needs is never freed, and is freed after that transaction ends; the accountant returns to its earlier level;
  * a page freed by a commit is not reused while a transaction with an older snapshot runs, and is reused after it ends;
  * a group holding only GC changes becomes durable, and recovery replays it;
  * a delete-stamped entry is removed physically only after every older snapshot has ended, and the integrity check passes after each GC pass;
  * after a rewrite, a page holds no `created_ts` at or below the horizon (decoded from the page);
  * the pages of a dropped tree return to the free list after the horizon passes;
  * while a database is open, `/proc/<pid>/task/*/comm` lists `lpg-gc`.
* The `loom` model passes, and fails when the fence of the registration is removed.
* The stress profile runs the new and the re-run concurrency tests with no failure.
```

---

### #30 TASK, priority 8, depends on: #24, #25, #26

**Title:** Implement values, record and index-key encodings, temporal and spatial types

**-fr**
```text
**Every GQL value the store keeps must round-trip exactly, and index keys must sort in the total order the user approved** (TS-21; BR-GQL-002 answers of task #26). Temporal values use IANA zones from the operating system and the system clock (FR-TYPE-001, FR-TYPE-006, FR-TYPE-007). Points carry one of four SRIDs and have Neo4j's distance (FR-TYPE-002, FR-TYPE-004). One Hilbert encoding serves the point index and the spatial types (sprint DECISION 7; TS-18).
```

**-tr**
```text
* **Module** `src/types/` (TS-01): `Value`, 24 bytes, with the variants of TS-21:16-19 restricted to the types #26 approved.
* **Temporal:** `jiff` at its latest release (0.2.38 on 2026-10-07) as a dependency; zone data from `/usr/share/zoneinfo` or `TZDIR`; precision, leap seconds and default zone as #26 item 4 decides; the current time from the system clock.
* **Spatial:** `Point2D` and `Point3D` with SRID 7203, 9157, 4326 or 4979; distance as TS-21:23-27 (Neo4j `CRSCalculator.java:130-166` studied only, GPL, never copied).
* **Record encoding** (TS-21:29): sorted label tokens, then properties sorted by key token, each with a header (type tag, key-token delta, length class) and a minimum-width payload; long records use the overflow chains of TS-05. Memgraph's property store is the reference (R-MG `property_store.cpp:63-118`).
* **Index-key encoding** (TS-21:30): byte strings whose byte order equals the approved total order (#26 items 1 and 2), for single and composite keys; `storekey` and `memcomparable` studied only.
* **Hilbert** 2D and 3D, own code (TS-18:69), with the precision and extents of #25 point 16, plus the decomposition of a query box into key ranges; `fast_hilbert`, `lindel` and Neo4j's curves studied only.
* **Lexical forms** of TS-21:31-33.
```

**-ac**
```text
* `cargo nextest run -p lpg types` passes. The tests show that:
  * every approved type round-trips through the record encoding, also after commit and reopen; a zoned date-time in `Europe/Lisbon` keeps its instant and zone name (FR-TYPE-001); a point in each of the four CRSs keeps its coordinates and SRID (FR-TYPE-002);
  * the distance of Cartesian (0, 0) and (3, 4) is 5, and a Cartesian point and a WGS-84 point give null (FR-TYPE-004); WGS-84 2D and 3D distances equal constants computed independently from the TS-21 formulas and listed in the test;
  * for at least 100 000 seeded pairs within and across type groups, and for composite keys, the byte order of the encodings equals the approved order (seed printed on failure, `LPG_SEED` overrides);
  * the Hilbert encoding is a bijection on a small grid in 2D and 3D; for seeded boxes, the decomposed ranges contain every grid point inside the box (checked by brute force);
  * the offset `jiff` gives for a zone equals the output of `TZ=<zone> date +%z` at the same instant (FR-TYPE-007), and the current timestamp lies between two `SystemTime::now()` reads around it (FR-TYPE-006);
  * `std::mem::size_of::<Value>()` is 24.
* `cargo nextest run -p lpg` passes, `cargo deny check licenses` exits 0, and `cargo tree -e normal --depth 1` adds only `jiff` (and any crate #26 approved).
```

---

### #31 TASK, priority 8, depends on: #29, #30

**Title:** Implement the persistent catalog: objects, tokens, owners, default graph

**-fr**
```text
**A database is one full, persistent GQL catalog whose changes commit and roll back together with data changes** (FR-DB-001, FR-CAT-001, FR-CAT-002, FR-CAT-004, TS-17). A new database holds a default schema and a default graph (FR-DB-005). Each catalog object records its owner (FR-AUTH-002); passing it to authorisation, the closed-graph-type check and the listing procedure are Sprint 3.
```

**-tr**
```text
* **Module** `src/catalog/`: one catalog tree per database, created by `create`, with stamped entries (TS-17:17-20): `(parent_id, name) → (object_id, kind, owner, definition)` for directories, schemas, graph types and graphs; reverse entries `object_id → (parent_id, name)`; token tables `(kind, name) ↔ token_id`; index and constraint definitions keyed `(graph_id, object_id)` (filled by #33 and #34).
* **Identifiers:** catalog-object and token counters as #25 point 11 decides, raised through `Ids` (`src/tx/commit.rs:29-40`).
* **Concurrency:** tokens created in an inner transaction under a latch; catalog names under an X `(parent_id, name)` lock (#25 point 10).
* **Owners** as #25 point 19 decides.
* **Create** writes the default schema and the default graph in its first commit (FR-DB-005, TS-17:21).
* **Graphs:** creating a graph creates its node, edge and adjacency trees in the same transaction; graph types are stored immutable (their check is Sprint 3); `DROP GRAPH` sets the delete stamp and #29's GC frees the trees after the horizon (TS-17:22). Directory nesting follows #26 item 6.
* **Catalog cache:** the latest committed catalog, read under the commit latch (used by #33).
* **Crate-internal API for tests:** create, drop, look up and list objects; resolve tokens.
```

**-ac**
```text
* `cargo nextest run -p lpg catalog` passes. The tests show that:
  * a new database holds the default schema and the default graph, also after reopen (FR-DB-005);
  * two schemas and three graphs created in a database belong to its catalog, and their reverse entries resolve to their paths (FR-DB-001);
  * a committed catalog change is present after close and reopen, and after a drop without close (FR-CAT-002);
  * a transaction that creates a graph and other objects, then aborts, leaves none of them (FR-CAT-004, catalog part; the data part is in #32);
  * two concurrent transactions creating the same graph name: exactly one commits, at both isolation levels;
  * two concurrent transactions creating the same label get the same token identifier, and tokens survive reopen;
  * every object kind returns the owner recorded at its creation (FR-AUTH-002, storage part);
  * a dropped graph's trees are freed after every older snapshot ends, and the integrity check passes;
  * after a crash the catalog-object and token counters are at least every committed value.
* `cargo nextest run -p lpg` and the stress profile pass.
```

---

### #32 TASK, priority 8, depends on: #31

**Title:** Implement node, edge and adjacency trees, element ids and statistics

**-fr**
```text
**Each graph stores its nodes, edges and adjacency in the layout chosen on evidence (TS-06, M-LAYOUT layout B), with stamps, stable identifiers that are never reused, and statistics that survive checkpoints** (FR-TYPE-003, BR-TYPE-001, FR-STO-003, FR-CAT-004, TS-07, TS-17 Statistics, TS-12 adjacency check; NFR-PERF-003 to NFR-PERF-005 structure only, measurement in Sprint 6).

This changes Sprint 1 code: the checkpoint saves the statistics (TS-11).
```

**-tr**
```text
* **Trees per graph** (TS-06:129-132): node tree `node_id → record`, edge tree `edge_id → (source, target, type, record)`, adjacency tree keyed `(node_id, direction, edge_type, neighbour_id, edge_id)` with two entries per edge and field-delta varint leaves (TS-06:136; `docs/evidence/layout-bench/`), stamps as #25 points 1 and 2 decide; records use #30's encoding.
* **Element identifiers** (TS-07): one database counter, drawn as #25 point 11 decides.
* **Crate-internal API:** create and delete node (deleting a node with live edges fails unless its edges are deleted in the same call), set and remove labels and properties, create and delete edges, expand a node by `(direction, edge_type)` prefix (TS-06:150), each taking the locks of TS-15:213-220 (graph S, label or type schema S, node content X, node existence S or X, edge X).
* **Statistics** (TS-17:24): node counts per label, edge counts per `(label, edge type, direction)`, updated at apply (#25 point 13); written at the start of each checkpoint, loaded at open, recounted in the background after recovery (#25 point 12), in `src/storage/checkpoint.rs:148-241` and `src/storage/engine.rs:533-638`.
* **Integrity:** adjacency against edges (`src/storage/check.rs:34-35`) with the rules of #25 point 18.
* Re-run `storage::checkpoint`, `storage::recovery`, `storage::engine`, the R3 call-order test and the `suites::fault` sample.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes, every Sprint 1 test included. The tests show that:
  * a node whose labels and properties change in 50 commits keeps its identifier, also after reopen (FR-TYPE-003);
  * after a node is deleted and 10 000 elements are created, before and after a crash and reopen, none has its identifier (BR-TYPE-001);
  * 16 threads creating disjoint nodes and edges in their own transactions all commit, and every write is present after reopen (FR-STO-003);
  * a transaction that creates a graph and inserts nodes, then aborts, leaves neither (FR-CAT-004);
  * an expansion at a snapshot returns exactly the live edges of that node, direction and type;
  * an edge created towards a node that a concurrent transaction deletes: one of them fails or waits, and the integrity check finds no dangling adjacency entry;
  * the integrity check reports an injected adjacency entry without an edge, and an edge with a missing adjacency entry;
  * the statistics equal a full recount after a seeded workload, survive a clean reopen, and equal a recount again after a drop without close once the background recount ends;
  * the fault-storage call log of a checkpoint shows the WAL `sync_data` of the statistics batch before the `CONTROL` write.
* Field-delta leaves decode to the keys encoded, for seeded key sets.
* The stress profile runs the new concurrency tests with no failure.
```

---

### #33 TASK, priority 8, depends on: #32

**Title:** Implement the index kinds and the online index build with its side tree

**-fr**
```text
**Every index kind must return exactly what a scan of the data returns, and building an index must block no writer** (FR-IDX-001, FR-IDX-003, FR-IDX-005, FR-TYPE-005, TS-18 Indexes and Online build, TS-12 index check). The build follows the giant-graph path: no pinned snapshot, an external sort and a bottom-up build (NFR-PERF-005, structure only).

Exit condition served: an index build blocks no writer, and the `loom` model of the side tree passes (sprint DECISION 8). This closes unverified point 7 (`docs/technical-spec/quality-and-testing.md:106`) by correctness evidence.
```

**-tr**
```text
* **Module** `src/index/`: definitions in the catalog (TS-17:20) with states POPULATING and ONLINE; the kinds and keys of TS-18:39-44 with #30's key encoding; maintenance in commit step 6 for ONLINE indexes; a reader uses an index only from the snapshot of its ONLINE commit (TS-18:46).
* **Online build** (TS-18:47-56 with #25 point 14): the definition commits at `S`; from `S` committers, reading the catalog cache under the latch, write insert and delete records to a side tree, a logged B+tree keyed in commit order; thread `lpg-index-build` scans the latest committed pages under their latches, registered as an unpinned reader (#25 point 3), sorts with `Sorter` (`src/storage/sort.rs:179-346`, memory from the accountant), builds with `Engine::bulk_build_pages` and `Built::install` (`src/storage/btree/bulk.rs:479, 424`) into frozen entries, replays the side tree idempotently, and the ONLINE commit replays the remaining tail under the latch and drops the side tree. Open restarts every POPULATING build from the scan.
* **Point index:** Hilbert key per CRS plus the exact distance as a post-filter (TS-18:44).
* **Read sets** for index ranges and Hilbert ranges (TS-15:206).
* **Statistics:** entry and distinct-key counts per index (TS-17:24).
* **Integrity:** each index against the data (TS-12:245, #25 point 18).
* **`loom` model:** committers writing side-tree records against the builder's replay and the ONLINE switch.
* Update `quality-and-testing.md` point 7 and `catalog-and-indexes.md:77` to cite the evidence.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. The tests show that:
  * one index of each kind, created through the internal API, returns for seeded queries the same elements as a full scan at the same snapshot (FR-IDX-001, storage part; the listing procedure is Sprint 3);
  * with the build paused after its scan, during its replay and before its ONLINE commit, 8 writer threads commit inserts, updates and deletes while the builder stays paused, and the ONLINE index passes the integrity check against the data (FR-IDX-005);
  * an index on graph g1 holds no entry of graph g2 (FR-IDX-003);
  * distance queries over seeded points in the four CRSs, including points outside the curve extents, return the same set with and without the point index (FR-TYPE-005, storage part);
  * a crash at every crash point of a build (fault storage) reopens with the index POPULATING, and the restarted build ends ONLINE and passes the integrity check;
  * index statistics equal a recount.
* The `loom` model passes, and fails when the ONLINE commit skips the tail replay.
* `quality-and-testing.md` no longer lists point 7 as unverified and cites these tests.
* The stress profile runs the new concurrency tests with no failure.
```

---

### #34 TASK, priority 8, depends on: #33

**Title:** Implement uniqueness, existence, type and node-key constraints

**-fr**
```text
**Constraints must reject every violating write, a creation over violating data must fail and leave no constraint, and a creation must block only the writers of its label or edge type** (FR-IDX-002, FR-IDX-003, FR-IDX-004, FR-IDX-006, TS-18 Constraints, TS-15 unique-value and schema locks).

Exit condition served: a constraint creation blocks only writers of its label or edge type (sprint DECISION 8).
```

**-tr**
```text
* **Definitions** in the catalog; uniqueness and node key own an index, built under the X schema lock without a side tree (#25 point 15), with #33's bulk path.
* **Writes:** an X lock on `(constraint, value)` when a constrained value is written, and a recheck when the commit is applied; property existence and property type checked over the write set at commit (TS-18:57-59). A violation fails with an internal kind `ConstraintViolation`; its GQLSTATUS code is Sprint 3.
* **Creation** (TS-18:60): the X schema lock on its label or edge type in its graph, validation of the latest committed data, then the owned index; on any violation, nothing is created (FR-IDX-004).
* **Crate-internal API:** create and drop each constraint kind.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. The tests show that:
  * with a node-key constraint on label L and property p, inserting an L node without p fails, and inserting one with a used p value fails (FR-IDX-002, storage part);
  * 16 threads inserting the same unique value: exactly one commits, at both isolation levels;
  * a uniqueness constraint on graph g1 lets duplicate values commit in graph g2 (FR-IDX-003);
  * two nodes with the same p: creating a uniqueness constraint on p fails, and afterwards no constraint and no owned index exist and the integrity check passes (FR-IDX-004);
  * with validation paused at a pause point, an L writer is seen waiting in the lock table and an M writer commits; after the pause ends, the L writer succeeds or fails against the constraint (FR-IDX-006);
  * existence and type constraints reject violating commits and accept the others;
  * constraints survive reopen and a drop without close.
* The stress profile runs the new concurrency tests with no failure.
```

---

### #35 TASK, priority 7, depends on: #34

**Title:** Run the crash suites on transactional workloads and verify every exit criterion

**-fr**
```text
**The sprint ends only on evidence.** The Sprint 1 crash, backup and restore suites must pass with transactional workloads (FR-TX-001 through NFR-VER-004), on ext4 and XFS (NFR-PLAT-004), and every exit condition of sprint DECISION 8 that is not timing must hold on the final code. Timing items are deferred to Sprint 6 (sprint UPDATE 54).
```

**-tr**
```text
* **`src/suites/fault.rs`:** a workload `Transactions` (`Workload`, `fault.rs:154-164`): serializable and snapshot-isolation transactions over nodes, edges and catalog objects, an index build and GC passes, with a model of the commits that returned. Crash-point sweep, failure sweep and `FailCopy`, in every crash mode. Oracle: a committed prefix that includes every returned commit; no entry stamped above the recovered `commit_ts`; the integrity check with the cross-tree checks.
* **`src/suites/kill.rs`:** the child runs transactional threads, an index build, GC and backups; the same oracle.
* **Backup and restore** under four transactional committers restore to one commit point and pass the cross-tree checks.
* **Matrix:** with `LPG_LAB_EXT4=/data/tmp/lpg-lab` and `LPG_LAB_XFS=/mnt/lpg-xfs`: the full test run, both suites, the stress profile, the `loom` and `shuttle` runs, Hermitage and Elle on graph operations.
* **Gates:** `cargo +1.89 build`, `cargo deny check licenses`, `cargo clippy --all-targets -- -D warnings`, `forbid(unsafe_code)`, the FR-STO-002 grep of task #22.
* **Record** in `docs/evidence/lab-sprint-2/report.md`: each exit condition with its test names, the suites' counts per workload and mode, and the coverage of `src/tx`, `src/catalog` and `src/index` from the suites (`cargo llvm-cov nextest -p lpg`) with the error branches not reached. No timing.
```

**-ac**
```text
* `cargo nextest run -p lpg` with both lab variables set, `cargo +1.89 build`, `cargo deny check licenses` and `cargo clippy --all-targets -- -D warnings` exit 0; outputs saved in `docs/evidence/lab-sprint-2/raw/`.
* `suites::fault` prints a count above 0 for every workload and crash mode, including `Transactions`, on ext4 and on XFS. `suites::kill` passes 200 iterations in each directory with transactional children.
* The stress profile, `RUSTFLAGS="--cfg loom" cargo test -p lpg --lib --release loom_` (commit latch and publish order, lock table, deadlock detection, snapshot registry, side tree) and the `shuttle` run pass.
* The report maps each exit condition of sprint DECISION 8 (minus timing) to passing tests, and lists the deferred timing items as Sprint 6.
* `grep -rnE 'std::fs|OpenOptions|File::(open|create)' src --include=*.rs` matches only `src/storage/fs.rs` and `src/lab/`; `grep -n 'forbid(unsafe_code)' src/lib.rs` matches.
```

## 6. Execution order and dependency edges

Order for `rmp sprint reorder 2`: **#2, #24, #25, #26, #27, #28, #29, #30, #31, #32, #33, #34, #35.**

Edges for `rmp task add-dep` (task → depends on):

| Task | Depends on | Why |
|---|---|---|
| #24 | none | first task after planning (task #2 `-fr`) |
| #25 | none | specification only; runs while #24 waits on any tool |
| #26 | none | user questions only; asked early so the answers arrive before #30 |
| #27 | #24, #25 | needs `crate::sync` and points 1, 2, 5 |
| #28 | #27 | validation and locks sit on write sets and snapshots |
| #29 | #28 | GC frees write-key sets (#28) and old versions (#27) |
| #30 | #24, #25, #26 | encodings need the approvals (sprint DECISION 7) and points 16, 17 |
| #31 | #29, #30 | dropping a graph uses GC; definitions use value types |
| #32 | #31 | records hold tokens; graphs come from the catalog |
| #33 | #32 | indexes cover graph data; build reuses statistics and integrity hooks |
| #34 | #33 | constraints own indexes built by #33's path |
| #35 | #34 | runs on the final code |

#30 can run before #27 if the user answers #26 first: the edges allow it, and `rmp sprint reorder` can move it then.

## 7. Traceability

Every id of scope comment #6 and exit comment #8 maps to at least one task; no task maps to an item outside them (task #24 maps to the laboratory comment #9 and to TS-30, as Sprint 1's #7 did).

| Item | Tasks |
|---|---|
| TS-05 (reuse after GC horizon; record overflow) | #29, #30 |
| TS-06 | #27, #32, #33 |
| TS-07 | #31, #32 |
| TS-11 (statistics at checkpoint) | #32 |
| TS-12 (cross-tree checks) | #32, #33, #35 |
| TS-14 (incl. TS-13 step 7, GC thread) | #27, #29 |
| TS-15 | #28, #31, #32, #34 |
| TS-16 | #27, #28 |
| TS-17 | #31, #32 |
| TS-18 | #33, #34 |
| TS-21 | #26, #30 |
| TS-30 (exit: loom models; R11, R12) | #24, #28, #29, #33, #35 |
| FR-TX-001 | #27, #28, #35 |
| FR-TX-002 | #35 |
| FR-TX-003 | #28 |
| FR-TX-004 | #28 (Bolt part: Sprint 6) |
| FR-TX-005 | #28 |
| FR-TX-006 | #27, #28 |
| FR-TX-007 | #28 |
| FR-TX-008 | #28 |
| FR-TX-009 | #28 |
| FR-TX-010 | #27, #28 |
| BR-TX-001 | #28 |
| FR-STO-003 | #28, #32 |
| FR-LIM-005 | #27, #29 |
| FR-LIM-007 | #27 |
| FR-TYPE-001 | #30 |
| FR-TYPE-002 | #30 |
| FR-TYPE-003 | #32 |
| FR-TYPE-004 | #30 |
| FR-TYPE-005 | #30, #33 |
| FR-TYPE-006 | #30 |
| FR-TYPE-007 | #30 |
| BR-TYPE-001 | #32 |
| FR-DB-001 | #31 |
| FR-DB-005 | #31 |
| FR-CAT-001 | #31 |
| FR-CAT-002 | #31 |
| FR-CAT-004 | #31, #32 |
| FR-IDX-001 | #33 |
| FR-IDX-002 | #34 |
| FR-IDX-003 | #33, #34 |
| FR-IDX-004 | #34 |
| FR-IDX-005 | #33 |
| FR-IDX-006 | #34 |
| FR-AUTH-002 (owner recorded) | #25, #31 |
| NFR-PERF-003 | #27, #32 (structure); measurement: **Sprint 6** |
| NFR-PERF-004 | #32 (layout B with stamps); measurement: **Sprint 6** |
| NFR-PERF-005 | #33 (bottom-up build, no pinned snapshot); measurement: **Sprint 6** |
| BR-GQL-002 (comment #7 risk) | #26 |
| Unverified point 7 (side tree) | #33 |
| Laboratory context #9: toolchain refresh, `loom`, anomaly tests with histories | #24, #28 |

**Deferred to Sprint 6 (timing or efficiency measurement; sprint UPDATE 54):** traversal device I/O and CPU at 1 %, 10 % and 100 % cache against M-LAYOUT layout B with properties (#8); closing unverified point 6 (#8); data for unverified points 3, 4 and 13 (#8, #9); off-CPU and wait analysis with `perf sched` and `perf trace`, and time in the serial commit section (#9); the commit benchmark at 1–16 threads with latency percentiles (#9); the accountant breakdown over time against RSS and `heaptrack` on encodings (#9); bytes per node and per edge and device bytes per traversal (#9).

## 8. Checks run on this document

The `text` blocks of §5 were checked by script after writing (result appended below): character count per field, and no line starting with `- `.

Result (2026-10-07): 36 fields, longest 3 112 characters (limit 4 096), no line starting with `- `; the 12 titles are 62–79 characters, plain text. A search for open-ended wording (`etc.`, `as needed`, `if necessary`, `where appropriate`, `TBD`) found none.

## 9. Tool status for task #2's last acceptance criterion

* At their latest release, with no action needed: Rust stable 1.99.0 and nightly 1.101.0 (2026-10-06), `rustup` 1.29.1 (`rustup check`: all up to date), toolchain 1.89, `cargo-nextest` 0.9.146, `cargo-llvm-cov` 0.9.1, `cargo-deny` 0.20.2 (versions shown by `cargo install --list`; compare with crates.io `max_stable_version` before recording the `TEST` comment).
* **Pending, no root needed:** `elle-cli` is absent (`command -v`); task #24 installs it. Its latest version could not be read on 2026-10-07 (GitHub API returned no data); record this as a `FINDING` on task #2 if it is still unreadable when the plan is entered.
* **Needs root: none.** XFS is mounted at `/mnt/lpg-xfs`; the timing tools (`perf`, `heaptrack`, `fio`) are not needed until Sprint 6.

## 10. rmp operations to enter the plan (for the roadmap-manager, not run here)

1. `rmp task create` × 12 with the titles, types, priorities and `-fr`/`-tr`/`-ac` of §5; then `rmp sprint add-tasks 2 <ids>`.
2. `rmp task add-dep` for each edge of §6, using the real ids.
3. `rmp sprint reorder 2` in the order of §6.
4. On task #2: a `DECISION` with the §7 table (real ids), a `DECISION` with the choices of §3.4 and the grouping rationale of §5, a `FINDING` with the 19 + 7 specification points of §3, and a `TEST` with the tool versions of §9.
