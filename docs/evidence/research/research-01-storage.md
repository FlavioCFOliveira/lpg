# Research 01: storage engine evidence for the `lpg` technical specification

Date: 2026-09-24. Research only; no repository file was modified.

## 0. Baseline: the requirements this research is checked against

Read from `/data/dev/xumiga/lpg` at commit `140e230` (branch `develop`):

| Requirement | Consequence for the storage layer |
|---|---|
| FR-STO-001 | Disk-resident data, in-memory cache, graph larger than memory. |
| FR-STO-002, ABS-003 | **Every** file operation passes through an application-replaceable storage trait; the default is a filesystem implementation. |
| FR-STO-003, FR-TX-005..007 | Multiple concurrent writer transactions, write locks, deadlock detection. |
| FR-TX-006 | Reads never wait for writes and writes never wait for reads (MVCC). |
| FR-TX-002 | Commit returns only after "the transaction's log records" are fsynced; group commit allowed. |
| FR-REC-002 | Recovery after power loss / OS crash **including torn page writes**. |
| FR-REC-003, -005, -008, -010 | Checksums; verify log + metadata at every open; **a read that reaches a corrupted page fails** (FR-REC-005 acceptance); on-demand full check. |
| FR-REC-006 | fsync failure: commit fails, handle unusable until reopen (PostgreSQL `data_sync_retry=off`). |
| FR-REC-007 | Storage full: writes fail, **reads continue**. |
| FR-LIM-003 | One database-wide memory cap covering cache **and** query execution. |
| FR-OPS-001/007/008 | Online, consistent, full backup. |
| FR-DB-006/007 | Refuse a second open from another process **and** from the same process. |
| FR-OPS-004/011..013 | Versioned format, in-place, step-by-step migration. |
| FR-TYPE-003, BR-TYPE-001/002 | Element identifiers stable, never reused, preserved by backup/restore/migration. |
| NFR-PLAT-001/002/004, NFR-SAFE-001 | Linux, ext4/XFS, Rust 2024 + std, `#![forbid(unsafe_code)]` in the crate. |
| NFR-DEP-001/002 | Minimal, widely used, maintained, MIT-compatible dependencies (`cargo-deny`). |
| NFR-VER-004 | `kill -9` and fsync-failure injection through ABS-003. |
| NFR-SIMP-001 | Simplest solution that meets a decided requirement. |

## 1. Sources consulted

Local shallow clones (commit, commit date):

| Repository | Path | Commit |
|---|---|---|
| cberner/redb | `/tmp/redb` | `7677a88` (2026-09-23), crate 4.3.0 |
| fjall-rs/fjall, fjall-rs/lsm-tree | `/tmp/fjall`, `/tmp/lsm-tree` | `3adaa50` (2026-08-30), `3a0214a` (2026-09-10) |
| spacejam/sled | `/tmp/sled` | `e449d17` (2026-04-04), `1.0.0-alpha.124` |
| meilisearch/heed | `/tmp/heed` | `14e3e49` (2026-05-21) |
| LMDB/lmdb | `/tmp/lmdb` | `700e10f` (2026-08-19) |
| rust-rocksdb/rust-rocksdb | `/tmp/rust-rocksdb` | `46d7e4f` (2026-08-16) |
| facebook/rocksdb | `/tmp/rocksdb` | `662cdbc` (2026-09-23) |
| surrealdb/surrealkv | `/tmp/surrealkv` | `9b87842` (2026-08-26) |
| postgres/postgres | `/tmp/postgres` | `8982935` (2026-09-24) |
| sqlite/sqlite | `/tmp/sqlite` | `c62a443` (2026-09-24) |
| mysql/mysql-server (sparse: `storage/innobase`) | `/tmp/mysql-server` | `a1ef44f` (2026-09-24) |
| neo4j/neo4j | `/tmp/neo4j` | `54a7dcf` (2026-09-14) |
| duckdb/duckdb (sparse: buffer manager) | `/tmp/duckdb` | `0b9cba2` (2026-09-24) |
| RazrFalcon/memmap2-rs | `/tmp/memmap2-rs` | `a02e2a4` |

Other sources: crates.io API (queried 2026-09-24); Rust `RELEASES.md` (rust-lang/rust master); Rust std source of the installed toolchain (rustc 1.96.0, `library/std/src/sys/fs/unix.rs`, `library/std/src/fs.rs`); Linux man pages `flock(2)`, `fsync(2)`, `fdatasync(2)` (installed on the host); PostgreSQL documentation sources (`doc/src/sgml/config.sgml`, `backup.sgml`); MySQL 8.4 Reference Manual, InnoDB startup options page; Neo4j Operations Manual, online backup page.

## Q1. Build vs reuse

### Candidate matrix (evidence per cell)

crates.io data queried 2026-09-24 (`recent` = last 90 days).

| Crate | Licence | Downloads (total / recent), last release | Concurrent writers | Pluggable I/O (ABS-003) | fsync failure | Torn writes | Checksums | Memory cap | Online backup | MVCC snapshots | Callable under `forbid(unsafe_code)` |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **redb** 4.3.0 | MIT OR Apache-2.0 | 11.8 M / 5.25 M, 2026-09-15 | **No**: "Only a single write may be in progress at a time" (`src/db.rs`, doc of `begin_write`); `docs/design.md` l.7 "supports a single writer" | **Yes**: `pub trait StorageBackend` (`src/db.rs:74`): `len`, `read`, `set_len`, `sync_data`, `write`, `close`, range locks; `Builder::create_with_backend` (`src/db.rs:2255`) | Latches: any failed `read/write/set_len/sync_data` sets `io_failed`; every later call returns `PreviousIo` (`src/tree_store/page_store/cached_file.rs:197-259`) | Copy-on-write + two commit slots; 1PC+C (checksum-validated) or 2PC (`docs/design.md` §"1-phase + checksum durable commits", §"2-phase durable commits") | XXH3-128 Merkle tree over pages + slot checksum (`docs/design.md` l.134-144, l.318). **Verified only in repair and `check_integrity`**, not on ordinary reads (callers of `verify_primary_checksums`: `src/db.rs:1081, 1455, 1594` only) | `Builder::set_cache_size` (`src/db.rs:2153`); read + write buffer bounded (`cached_file.rs:287-292`) | No backup API (no match for "backup" in `src/db.rs`) | Yes, read transactions are snapshots | Yes |
| **fjall** 3.1.10 (+ lsm-tree) | MIT OR Apache-2.0 | 1.66 M / 0.55 M, 2026-08-30 | Optimistic transactions with conflict detection (`src/tx/optimistic/`) | **No**: files opened with `std::fs` directly (71 call sites across fjall + lsm-tree); no filesystem trait | Poisons the database on journal persist failure (`src/db.rs` `persist`, `src/poison.rs`) | LSM journal with checksums | xxh3 (dependency `xxhash-rust`) | `cache_size`, `max_write_buffer_size` (`src/builder.rs`) | No backup API found | Yes | Yes |
| **sled** | MIT OR Apache-2.0 | 16.1 M / 3.3 M; last stable 0.34.7 on 2024-10-11; git head is `1.0.0-alpha.124` | — | No filesystem trait found | — | — | — | — | — | — | Yes |
| **heed** 0.22.1 (LMDB) | heed MIT; `lmdb-master-sys` Apache-2.0 bundling LMDB under OpenLDAP Public License 2.8 (`/tmp/lmdb/libraries/liblmdb/LICENSE`) | 5.8 M / 1.9 M, 2026-04-07 | **No**: "only one write transaction may be active at a time" (`lmdb.h` l.25) | **No**: memory-mapped file | — | — | — | No cap: data served from the OS page cache via mmap | `mdb_env_copy` (runs in parallel with writes, `lmdb.h` l.736) | Yes | **No**: `EnvOpenOptions::open` is `pub unsafe fn` (`heed/src/envs/env_open_options.rs:233`), because of mmap UB | 
| **rocksdb** 0.25.0 (rust-rocksdb, C++ RocksDB 11.8.1) | Apache-2.0 (binding); librocksdb-sys "MIT/Apache-2.0/BSD-3-Clause" | 55.4 M / 6.7 M, 2026-08-16 | Yes (TransactionDB) | **No from Rust**: `Env` only exposes `new()` (default) and `mem_env()` (`src/env.rs:34,45`); the RocksDB C API offers only `rocksdb_create_default_env` and `rocksdb_create_mem_env` (`include/rocksdb/c.h:3611-3612`), no custom FileSystem | — | — | Block checksums | Block cache | Checkpoint / BackupEngine (`src/checkpoint.rs`, `src/backup.rs`) | Yes | Yes (safe wrapper), but builds C++ |
| **surrealkv** 0.21.4 | Apache-2.0 | 1.05 M / 0.36 M, 2026-08-26 | — | Internal `vfs::File` trait (`src/vfs.rs:9`), but files are opened with `std::fs` in 22 places; no application-supplied filesystem | — | — | — | — | — | — | Yes |
| persy 1.8.1 | MPL-2.0 (weak copyleft) | 0.74 M | not assessed further | | | | | | | | |
| canopydb 0.2.5 | MIT | 6.7 k (too little use for NFR-DEP-001) | | | | | | | | | |
| nebari 0.5.5 | MIT OR Apache-2.0 | last release 2023-02-27 (not maintained per NFR-DEP-001) | | | | | | | | | |
| okaywal 0.3.1 (WAL only) | MIT OR Apache-2.0 | 17 k, last release 2023-11-26 | | | | | | | | | |

Cells marked "—" were not investigated further because the candidate had already failed a hard constraint.

### Disqualifying findings

- **heed/LMDB**: the crate cannot call `EnvOpenOptions::open` under `#![forbid(unsafe_code)]` (it is an `unsafe fn`). It also has one writer, no pluggable I/O and no memory cap. **Rejected.**
- **rocksdb**: I/O cannot be routed through a Rust trait (the C API exposes only the default and in-memory Env). That fails FR-STO-002 and NFR-VER-004 (fsync-failure injection through ABS-003). It also adds a C++ build. **Rejected.**
- **fjall, sled, surrealkv**: no application-supplied filesystem, so they fail FR-STO-002 and NFR-VER-004. sled's last stable release is from 2024 and the development line is still alpha. **Rejected.**
- **persy, canopydb, nebari, okaywal**: MPL-2.0 licence, too little use, or no longer maintained (NFR-DEP-001/002). **Rejected.**

### redb, the only candidate with a pluggable backend: gap analysis

| Requirement | redb status | Evidence |
|---|---|---|
| FR-STO-002 / ABS-003 | Met: the lpg storage trait can be adapted to `StorageBackend`. | `src/db.rs:74-114` |
| FR-REC-006 fsync failure | Met: `io_failed` latch, reopen runs repair. | `cached_file.rs:243-251` |
| FR-REC-002 torn writes | Met **only with 2-phase commit**. With the default 1PC+C, a primary slot that fails verification is **silently rolled back to the secondary** (`do_repair`, `src/db.rs:1466-1490`). This cannot tell a torn uncommitted commit from media corruption of a commit that already returned. With 2PC, a failing primary is reported as `Corrupted("Primary is corrupted despite 2-phase commit")`. So lpg would have to force 2PC (two fsyncs per commit). | `src/db.rs:1466-1471` |
| **FR-REC-005** "a read that reaches a corrupted page fails" | **Not met.** Ordinary B-tree reads do not verify checksums. Verification runs only in repair and `check_integrity`, and `check_integrity` takes `&mut self` and requires that no transaction is in progress (`src/db.rs:874-877`). | callers of `verify_primary_checksums`; `btree.rs:942` used only there |
| **FR-REC-007** storage full: reads continue | **Not met as shipped.** A failed `write` or `set_len` (ENOSPC) sets `io_failed`, after which **every read** returns `PreviousIo`. | `cached_file.rs:235-259` |
| FR-STO-003 concurrent writers | Not met at storage level: one write transaction at a time. It could be met at the lpg level by lpg's own concurrency control funnelling commits into one redb write transaction. lpg needs a lock manager, deadlock detection and SSI in any design. | `src/db.rs` `begin_write` doc |
| FR-TX-002 "log records ... on stable storage" | No log exists (shadow paging), so the requirement's wording does not apply as written. | `docs/design.md` §File format |
| FR-REC-008 "verify the log and the metadata at every open" | Slot checksums are verified when the header is parsed (`header.rs:500-520`). There is no log. | |
| FR-OPS-001 online backup | No API. Could be built as a read-snapshot logical copy. | |
| FR-LIM-003 joint cap | Only a fixed cache size set at build time (`Builder::set_cache_size`), so only a static split between cache and query memory is possible. | `src/db.rs:2153` |
| Format stability | Four major versions in about 2 years (3.0.0 2025-08-09, 4.0.0 2026-04-02, 5.0.0 in progress). File format v2 was dropped in 3.0 after an `upgrade()` step in 2.6. lpg's migration (FR-OPS-012) would inherit redb's format churn. | `/tmp/redb/CHANGELOG.md` l.3, 306, 359-362, 437-448 |
| Crash-open cost | Repair after an unclean shutdown takes "3 full scans" of the file (comments in `do_repair`), so open time is proportional to database size unless quick-repair is used. | `src/db.rs:1473, 1493` |
| Dependencies / unsafe | No required runtime dependency on Linux except `libc`; `unsafe` is used internally (`src/sync.rs`), which NFR-SAFE-001 permits for dependencies. | `Cargo.toml` l.26-36 |

**Conclusion for Q1.** No existing crate meets the requirements as written. redb comes closest, but it fails FR-REC-005 (no checksum verification on ordinary reads) and FR-REC-007 (ENOSPC latches the whole handle, so reads fail). It also needs forced 2PC, has no log (FR-TX-002/FR-REC-008 wording), and has no backup API. Closing the checksum gap from outside redb is not practical: its checksums live in parent pages, which an I/O-level adapter cannot see.

**Recommendation Q1:** lpg implements its own storage engine (page file, buffer manager, WAL, checkpoint, recovery) on `std` only. Minimal dependencies: a CRC-32C crate (Q6); lock primitives from `std`, or `parking_lot` if measurement justifies it.

**Rejected alternatives**: redb as-is (FR-REC-005, FR-REC-007); redb plus an lpg WAL in front (still fails FR-REC-005 and FR-REC-007 inside redb); heed (`unsafe` open); rocksdb (no Rust-pluggable FS, C++); fjall, sled, surrealkv (no pluggable FS).

**Product decision (not answerable by evidence):** if the user relaxed FR-REC-005 (corruption detected at open after a crash and by the on-demand check, not on every read) **and** FR-REC-007 (accept that a full disk makes the handle unusable, or fund an unverified space-reserve adapter), redb would become a viable substrate. It would save the page store, B-tree, cache and crash-consistency work, at the cost of 2PC commits and format coupling to redb. See the summary.

## Q2. Page-based B+tree with buffer manager vs LSM

Evidence:

- PostgreSQL, InnoDB, SQLite and Neo4j all use fixed-size pages with a buffer manager and update pages in place. Default page sizes: PostgreSQL relation block 8 KiB (`meson_options.txt` `blocksize` default `8`); InnoDB 16 KiB (`include/univ.i:311,322`, `UNIV_PAGE_SIZE_SHIFT_DEF = 14`); SQLite 4096 (`src/sqliteLimit.h:214`); Neo4j page cache `PAGE_SIZE = 8192` (`community/io/.../pagecache/PageCache.java:51`).
- An LSM engine needs a memtable, sorted runs, a manifest, compaction scheduling and usually Bloom filters (see the module list of `/tmp/lsm-tree/src`: `compaction`, `manifest.rs`, `memtable`, `version`, `vlog`, `table`). A B+tree page store needs the page file, buffer pool and WAL, which lpg needs anyway for FR-REC and FR-LIM.
- **Identifier constraint (BR-TYPE-001)**: Neo4j's record stores address records by offset and reuse identifiers ([S11] in the spec). Because lpg must never reuse an identifier, an offset-addressed record store would never reclaim space. The element identifier must therefore be a logical key. SQLite's rowid tables are B+trees keyed by a 64-bit integer, a proven shape for that.

**Recommendation Q2:** a page-based store in which every structure is a B+tree over fixed-size pages: nodes and edges keyed by a monotonic 64-bit element identifier; adjacency keyed by (node id, direction, edge type, edge id); property indexes as further B+trees. Use one page size, fixed at database creation and recorded in the header. Take **8 KiB** (PostgreSQL and Neo4j default) as the starting value. No measurement exists yet; confirm it with the NFR-PERF-002 baseline.

**Rejected**: LSM, because it needs more components (compaction, manifest, filters) and no stated requirement calls for its write-optimised profile (performance targets are open, NFR-PERF-001). Offset-addressed fixed-size record stores (Neo4j style), because of BR-TYPE-001. LeanStore/Umbra-style pointer swizzling and vmcache, which rely on raw pointers or virtual-memory tricks that `forbid(unsafe_code)` excludes. The last point comes from trained knowledge of the papers (Leis et al., ICDE 2018; Neumann and Freitag, CIDR 2020; Leis et al., SIGMOD 2023); I did not re-read the papers for this report.

## Q3. Buffer manager without `unsafe`, and the joint memory cap

Evidence:

- **PostgreSQL** uses a clock sweep: an atomic hand `nextVictimBuffer`, and per-buffer `usage_count` that is decremented as the hand passes; pinned buffers are skipped (`src/backend/storage/buffer/freelist.c:38-42, 106-160, 239-260`; `src/backend/storage/buffer/README` l.178-195).
- **Neo4j** uses a clock arm with a usage counter capped at 4 and eviction driven by demand for free pages (`community/io/.../impl/muninn/page-cache-internals.adoc` l.66-80).
- **InnoDB** uses an LRU list (`storage/innobase/buf/buf0lru.cc`). I did not inspect the midpoint-insertion details for this report.
- **DuckDB** has one `BufferPool` that "defines memory limits and implements priority eviction among all users of the pool" (`src/include/duckdb/storage/buffer/buffer_pool.hpp:38-39`). Memory is tracked per `MemoryTag` (`UpdateUsedMemory(MemoryTag, …)`, `EvictBlocks(…, MemoryTag, extra_memory, memory_limit)`, l.60, 92). This is a reference for one cap shared by the cache and operators.
- **Memory mapping needs `unsafe`**: `memmap2` `Mmap::map` / `MmapOptions::map` are `pub unsafe fn` (`/tmp/memmap2-rs/src/lib.rs:469, 809`). heed documents the same reason. The buffer manager must therefore use explicit `pread`/`pwrite`: `std::os::unix::fs::FileExt::{read_at, write_at}`, which are safe.
- Positional I/O and `sync_data` in std: on Linux `File::sync_data` calls `fdatasync` (`library/std/src/sys/fs/unix.rs:1405-1426`).

**Recommendation Q3:**

- **Frames.** Each frame is a heap buffer (`Box<[u8]>`) behind a per-frame reader-writer latch. A sharded `HashMap<PageId, FrameId>` serves as page table, with an atomic pin count and an atomic usage count per frame. Eviction uses a **clock sweep** as in PostgreSQL and Neo4j; it skips pinned frames, and before writing a dirty page it enforces WAL-before-data (page LSN ≤ durable WAL LSN).
- **Memory cap.** A single **memory accountant** holds the FR-LIM-003 budget as an atomic byte counter. Buffer frames and query operators both reserve from it, as in DuckDB. When a reservation fails, the pool evicts clean unpinned frames, and flushes dirty ones after the WAL is durable. If the reservation still cannot be met, the statement fails under FR-LIM-001. A minimum pool size is kept so the engine can make progress. The exact minimum is not yet measured.
- **Rejected**: a static cache/query split (simpler, but it does not share memory as FR-LIM-003 intends: "covers the cache and query execution together"); `mmap` (needs `unsafe`, and the OS page cache cannot be capped by the crate); LRU lists (more bookkeeping under concurrency than a clock, with no stated requirement it serves better).
- **UNVERIFIED**: whether `std::sync::RwLock` is fast enough, compared with `parking_lot`, has to be measured. The choice of lock crate is deferred to measurement.

## Q4. WAL design

Evidence:

- **PostgreSQL WAL** is redo-only. Records carry `pg_crc32c xl_crc` (`src/include/access/xlogrecord.h:49`). Segments are 16 MiB by default (`src/include/pg_config_manual.h:20`) and are preallocated at creation; ENOSPC there is an `ERROR`, not a `PANIC` (`xlog.c:3370-3405`). A failed WAL write or fsync is a `PANIC` (`xlog.c:2529-2531`, `issue_xlog_fsync`). Checkpoints: the redo pointer is fixed first, then every buffer dirty at that moment is flushed (`BufferSync`, `BM_CHECKPOINT_NEEDED`, `bufmgr.c:3575-3612`).
- **InnoDB** keeps redo logs for pages, undo logs for MVCC and rollback, and fuzzy checkpoints (`include/log0chkp.h`). Its redo log is physiological, written by mini-transactions (trained knowledge; not re-read here).
- **SQLite WAL** records whole page images; a frame is valid only when its salt and cumulative checksum match (`src/wal.c` l.18-75).
- **redb** needs no WAL because it uses shadow paging (Q1).

**Recommendation Q4:**

- **Records: redo-only, NO-STEAL, one full page image per modified page per commit** (SQLite WAL style). Uncommitted changes never reach data pages. At commit, the transaction's page images and a commit record are appended, then one `fdatasync` covers a group of commits (group commit, FR-TX-002).
  - Recovery replays page images after the checkpoint redo point. It needs no undo, no CLRs and no LSN comparison beyond "is this frame valid".
  - A torn WAL tail is an uncommitted suffix, detected by CRC-32C per record plus a per-segment salt or epoch. Any valid record with an invalid record *before* it is corruption: FR-REC-003, refuse to open.
- **Checkpoints: PostgreSQL-style.** Fix the redo LSN first, flush every page dirty at that instant, `fdatasync` the data files, then write the checkpoint LSN to the control file, then recycle segments older than it.
- **Segments:** fixed-size, preallocated by writing zeros (std only; PostgreSQL `wal_init_zero` pattern). ENOSPC then surfaces when a segment is created, before any commit depends on it, so the transaction fails and reads continue (FR-REC-007). Data-file growth follows the same rule: extend before logging, as PostgreSQL `mdextend` errors with "Check free disk space" rather than panicking (`smgr/md.c:525-536`).
- **I/O errors:** a failed WAL write or fsync, or a failed data-file write or fsync, latches the handle (FR-REC-006, as PostgreSQL PANIC and redb `io_failed` do). Only ENOSPC from *preallocation* is non-fatal.
- **Rejected**:
  - ARIES undo/redo: needed only when uncommitted data can reach pages (STEAL). NO-STEAL removes it, and FR-LIM-001 already bounds transaction memory.
  - Physiological records plus full-page writes (PostgreSQL): less WAL volume, but more record types and redo logic; worth adopting only if the baseline shows WAL volume fails a target.
  - Logical graph-operation records: redo would have to re-run index maintenance.
- **Open dependency (not a storage-only decision):** under NO-STEAL with in-place page updates, old versions for MVCC readers (FR-TX-006) must live somewhere: in-memory version chains (Memgraph style) or versioned records in pages. This belongs to the transactions/MVCC research. The choice decides whether the "one page image per commit" WAL stays redo-only.

## Q5. Torn-page protection

Evidence:

- **PostgreSQL** `full_page_writes` logs "the entire content of each disk page to WAL during the first modification of that page after a checkpoint", because a torn page cannot be repaired from row-level records (`doc/src/sgml/config.sgml`, `guc-full-page-writes`). The decision is `needs_backup = (page_lsn <= RedoRecPtr)` (`xloginsert.c:677-697`).
- **InnoDB doublewrite buffer**: pages are written to a separate doublewrite area and synced before being written in place (`include/buf0dblwr.h:185` "Writes a page to the doublewrite buffer on disk, syncs it"). It needs its own files (`#ib_*.dblwr`) and ordering.
- **SQLite WAL**: every committed page is a full image in the WAL. The database file is written only during checkpoint, and the WAL is reset only after that write is synced (`wal.c` header comment).
- **redb**: copy-on-write, so no page is overwritten in place.

**Recommendation Q5:** with the Q4 design (page images in the WAL), torn-page protection is inherent. A data page is only ever written back after its image is durable in the WAL at or after the current redo point, and recovery overwrites the whole page from that image. No doublewrite area and no separate full-page-write rule are needed. Page checksums (Q6) additionally detect a torn page at read time.

**Rejected**: the InnoDB doublewrite buffer (an extra file, an extra fsync ordering and a recovery pass); PostgreSQL FPW (only relevant if Q4 moves to physiological records); relying on atomic sector writes (no guarantee for 8 KiB pages on ext4/XFS; PostgreSQL keeps `pg_control` ≤ 512 bytes precisely because only that size is treated as atomic, `pg_control.h:290-305`).

## Q6. Checksums

Evidence:

| Engine | Algorithm | Source |
|---|---|---|
| PostgreSQL pages | 16-bit checksum derived from an FNV-1a variant, computed as 32 parallel lanes; on by default in initdb | `src/include/storage/checksum_impl.h:20-78`; `src/bin/initdb/initdb.c:167` |
| PostgreSQL WAL | CRC-32C | `xlogrecord.h:49` |
| InnoDB pages | CRC-32C (polynomial `0x1EDC6F41`); default `innodb_checksum_algorithm=crc32` | `include/ut0crc32.h:154`; MySQL 8.4 manual |
| Neo4j transaction log | `java.util.zip.CRC32C` | `community/io/.../fs/ChecksumWriter.java:24-31` |
| redb | XXH3-128 Merkle | `docs/design.md` l.318 |
| SQLite WAL | custom 64-bit cumulative checksum | `wal.c` l.65-75 |

Candidate crates (crates.io, 2026-09-24):

- `crc32c` 0.6.8: Apache-2.0/MIT, 83.8 M downloads, **no runtime dependencies**, last release 2024-06-09.
- `crc-fast` 1.10.0: MIT OR Apache-2.0, 55.4 M downloads, depends on `spin`.
- `crc32fast` 1.5.2: MIT OR Apache-2.0, 706 M downloads, but it computes CRC-32/IEEE, not CRC-32C.

**Recommendation Q6:** CRC-32C on every WAL record, on every page (a checksum field in the page header, verified on every read from disk, which satisfies FR-REC-005 directly) and on the control file. Use the **`crc32c`** crate: Castagnoli polynomial as in the WAL of PostgreSQL, InnoDB and Neo4j, zero runtime dependencies, permissive licence.
**Rejected**: XXH3 (not what the page-store references use, and the `xxhash-rust` licence BSL-1.0 would add another licence to review); PostgreSQL's 16-bit FNV (weaker width); `crc32fast` (different polynomial from the references).
**UNVERIFIED**: `crc32c` has had no release since 2024-06; whether the repository is actively maintained was not checked. Re-check this under NFR-DEP-001 before adoption; `crc-fast` is the fallback.

## Q7. Online consistent full backup

Evidence:

- **PostgreSQL** base backup: it increments `runningBackups`, which forces full-page writes (`doPageWrites = fullPageWrites || runningBackups > 0`, `xlog.c:937`). It then forces a checkpoint (`xlog.c:10007-10022`), copies the data files while the server runs, and keeps the WAL from the start point to the stop point. Restore replays that WAL (`doc/src/sgml/backup.sgml`, low-level API).
- **Neo4j** full backup copies the store and runs recovery unless `--skip-recovery`, in which case "recovery will have to be done during restore time". The artifact records the lowest and highest transaction IDs (Neo4j Operations Manual, online backup page).
- **LMDB** `mdb_env_copy` runs in parallel with writers (`lmdb.h` l.736).
- xtrabackup and InnoDB clone were **not verified** for this report.

**Recommendation Q7:** the PostgreSQL pattern, which fits the Q4 design directly:

1. Record the current checkpoint redo LSN as `start`, and pin WAL recycling at `start`.
2. Copy the control data and every data file page by page through ABS-003. Pages may be fuzzy or torn.
3. Force a WAL flush, record `stop`, and copy WAL `[start, stop]`.
4. Unpin recycling.

Restore writes into an empty location (FR-OPS-009) and replays WAL up to `stop`, which yields the state of one commit point (FR-OPS-001). The copy is physical, so element identifiers are preserved (BR-TYPE-002). Because every modified page already has a full image in the WAL (Q4), no special "force page images during backup" mode is needed, unlike PostgreSQL.

**Rejected**: a logical dump at an MVCC snapshot (it re-encodes everything and must still preserve identifiers; a native dump already exists for export, FR-IMP-001); blocking checkpoints during the copy (not needed once WAL recycling is pinned).

## Q8. Single-process lock

Evidence:

- `File::lock`, `lock_shared`, `try_lock`, `try_lock_shared` and `unlock` are listed as stabilised in **Rust 1.89.0 (2025-08-07)** (rust-lang/rust `RELEASES.md`, "Version 1.89.0", Stabilized APIs).
- On Linux they call `flock` with `LOCK_EX | LOCK_NB` (`library/std/src/sys/fs/unix.rs`, `try_lock`). The std documentation says this "may change in the future" (`library/std/src/fs.rs:943-954`).
- `flock(2)`: locks belong to the *open file description*; a second `open()` in the same process gets an independent description and "may be denied by a lock that the calling process has already placed via another file descriptor".
- **Measured** on the host (rustc 1.96.0, `#![forbid(unsafe_code)]`, test program in scratchpad `locktest/main.rs`):
  - same-process second fd `try_lock` → `WouldBlock`;
  - closing that second fd does not release the first lock;
  - another process `flock -n` → exit code 1.
- POSIX `fcntl` record locks are per process, so they would not detect a same-process double open (trained knowledge of `fcntl(2)`; not re-read here).

**Recommendation Q8:** a lock file in the database directory, locked with `File::try_lock` (std, no dependency). This covers FR-DB-006 **and** FR-DB-007. MSRV must be ≥ 1.89 (edition 2024 alone requires ≥ 1.85). Because FR-STO-002 requires every file operation to pass through ABS-003, the lock acquisition should be a method of the storage trait, with the default implementation using `try_lock`.
**Rejected**: `fs2` (last release 2018) or `fs4` (an unnecessary dependency since 1.89); `fcntl` locks (per process, released when any descriptor closes); PostgreSQL-style pid files (they need stale-lock detection).

## Q9. Database directory layout

Evidence:

- **PostgreSQL**: `PG_VERSION`; `global/pg_control`, an 8 KiB file whose content is ≤ 512 bytes and CRC-protected for atomic writes (`pg_control.h:290-307`); relation files segmented at 1 GiB (`meson_options.txt` `segsize=1`); `pg_wal/` 16 MiB segments; `postmaster.pid`.
- **SQLite**: one database file whose header begins with "SQLite format 3" (`btreeInt.h:249`), plus `-wal` and `-shm` files.
- **InnoDB**: a system tablespace, per-table `.ibd` files, `#innodb_redo/` and `.dblwr` files (the file names come from trained knowledge; only the doublewrite module was read).
- **redb**: one file with a super-header (magic, god byte, page size) and two checksummed commit slots (`docs/design.md` l.30-60).

**Recommendation Q9:**

```
<db>/
  LOCK            flock target (Q8), empty
  CONTROL         fixed-size record ≤ 512 bytes padded to one page: magic, format version,
                  page size, checkpoint redo LSN, next element id, CRC-32C;
                  replaced by write-temp + fdatasync + rename + fsync(dir)
  data            single paged data file (all B+trees), page 0 = file header (magic, format version)
  wal/            fixed-size preallocated segments named by starting LSN (hex)
```

Keep one data file until measurement shows a need for segmenting it; PostgreSQL's 1 GiB segments exist for historical file-size limits, which ext4/XFS do not impose at this scale.

- **Directory fsync**: the directory must be fsynced after create and rename, because `fsync(2)` states that an "explicit fsync() on a file descriptor for the directory is also needed".
- **Format version**: stored in both `CONTROL` and the data-file header. An open fails with "migration required" on an older version (FR-OPS-004) and fails without writing on a newer one (FR-OPS-013).
- **FR-REC-008 at every open**: verify the `CONTROL` CRC and every WAL record from the checkpoint redo point to the end.

**Rejected**: separate per-graph or per-structure files (more files to fsync, rename and back up, with no requirement served); `CONTROL` as a page of the data file (its atomic replacement and the write ordering are simpler as a small separate file, following PostgreSQL).

## Unverified items

1. `crc32c` repository maintenance activity since 2024-06 (Q6).
2. InnoDB midpoint-LRU details, redo record classes, xtrabackup/clone mechanics, InnoDB file names (Q3, Q4, Q7, Q9): trained knowledge, not re-read.
3. The content of the LeanStore, Umbra and vmcache papers (Q2): trained knowledge, not re-read.
4. `std::sync::RwLock` vs `parking_lot` performance for frame latches (Q3): needs measurement.
5. The 8 KiB page size (Q2) and "one page image per commit" WAL volume (Q4): suitability needs the NFR-PERF-002 baseline.
6. Whether a redb `StorageBackend` adapter with a pre-zeroed space reserve could stop ENOSPC from latching redb: this design was not built or tested.
