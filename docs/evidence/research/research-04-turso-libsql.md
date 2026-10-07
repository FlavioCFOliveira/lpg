# Research 04: Turso and libSQL as references for the `lpg` kernel and its test strategy

Date: 2026-10-03. Research and measurement only. Apart from this record, no repository file was written for this research.
Builds on `research-01-storage.md` (page WAL with full page images, NO-STEAL, redo-only recovery, CRC-32C) and `research-02-layout-mvcc.md` (private write sets, commit-time validation).
lpg baseline: the uncommitted Sprint 1 tree on branch `feature/1-sprint-1-crash-safe-persistence-kernel-with-wal-re` (base `a8f2cf7`), `docs/technical-spec/storage.md`, `transactions-and-recovery.md` and `quality-and-testing.md` (TS-30).

## 1. Sources

| ID | Project | Path | Commit | Licence |
|---|---|---|---|---|
| R-TURSO | tursodatabase/turso (Rust rewrite of SQLite) | `/tmp/turso` (shallow clone) | `fc98dac` | MIT (`LICENSE.md`; workspace `Cargo.toml:105`) |
| R-LIBSQL | tursodatabase/libsql | `/tmp/libsql` | `d6c75af` | MIT (`LICENSE.md`); the vendored SQLite in `libsql-sqlite3/` is public domain (`src/test6.c` header) |

Turso's git history is not available, because the clone is shallow. Its `CHANGELOG.md` (4 436 lines) was used instead to find classes of bugs (§3).

The test inventory in §4 was collected by a read-only search agent. Its citations were spot-checked (`unreliable-libc/file.c:13-18`, `simulator/runner/file.rs:193-198`, `tests/integration/unreliable_io.rs:1-30`, `core/mvcc/yield_points.rs:5-34`, `libsql-sqlite3/test/permutations.test:117-129`). All five match.

## 2. Component affinity

| lpg component (spec) | lpg design | Turso equivalent | Assessment for lpg |
|---|---|---|---|
| WAL frames (TS-09) | Fixed-size frames. Each frame has its own CRC-32C, its LSN equal to its position, and a salt equal to the `CONTROL` sequence number. A corrupt frame followed by a valid later group fails the open (TS-13 step 5). | SQLite WAL format with a cumulative checksum and random salts (`core/storage/sqlite3_ondisk.rs:2297`, `core/storage/wal.rs:1323-1330`). The scan stops at the first bad frame, so a valid later commit is ignored (`sqlite3_ondisk.rs:1900-1932`). The MVCC logical log does the same: "first invalid frame … treated as an invalid tail" (`core/mvcc/persistent_storage/logical_log.rs:128-131`). | lpg is stricter. Turso uses the behaviour TS-13 rejects ("treating every invalid frame as the end of the log"). Nothing to adopt. |
| Group commit (TS-09, TS-16 steps 7–8) | One `lpg-wal` writer thread, one condition variable, group *k*+1 written only after the sync of group *k*. | Leader and follower with tickets, `durable_through`, `retry` and `abandoned` sets for waiters that are dropped (`core/mvcc/database/group_commit.rs:39-56, 194-292`). | The design difference is justified: lpg uses threads, Turso uses async state machines. Defect classes to test: a failed leader must not publish an unsynced prefix (`group_commit_tests.rs:306`); a failed group must not cover a later retry (`:332`); the watermark only moves forward (`:297`); a commit dropped after its record is written still commits (`:378-419`). The last one becomes relevant for Bolt cancellation (Sprint 3). |
| Checkpoint (TS-11) | Fuzzy checkpoint: redo point, write dirty slots whose page LSN is durable, `sync_data`, `CONTROL`, then recycle segments. | Ordering is documented: write the DB file, fsync it, truncate the log, fsync, and truncate the WAL last (`logical_log.rs:149-160`). | Same principle. Turso's own history shows two ordering bugs: "fsync DB file before truncating WAL after checkpoint" and "path not fsyncing the DB file after truncate checkpoint" (`CHANGELOG.md:1977, 2006`). No lpg test asserts the order data `sync_data` → `CONTROL` → recycle (§5.1). |
| Recovery (TS-13) | Redo of page images from R. Checks that segments form a chain, and treats a later valid group as corruption. | Logical log: applies only frames with `commit_ts` above a durable boundary, and resets the writer offset to the last valid frame (`logical_log.rs:133-142`). | lpg is stricter on corruption. The idea of an idempotent boundary (`persistent_tx_ts_max`) does not apply, because page images are idempotent by themselves. |
| Page checksum (TS-12) | CRC-32C over bytes 4..8192. The page number is not covered (`src/storage/page.rs:8-14`). | Optional XXH3 over the page. The page id is passed in but unused (`core/storage/checksum.rs:35-47, 81-83`). | Neither project detects a misdirected write or a stale page that still has a valid checksum. PostgreSQL mixes the block number into the checksum (`/tmp/postgres/src/include/storage/checksum_impl.h:172, 193`). See recommendation R9. |
| Buffer pool and eviction (TS-08) | Clock sweep. Dirty frames are written only when their page LSN is durable. No spill. | SIEVE with a reference counter (`core/storage/page_cache.rs:44-46, 90-99`). Dirty pages cannot be evicted unless they were spilled to the WAL (`:653-659`). | lpg rejects spill on purpose (TS-16, "Rejected"). The spill path is where Turso's rollback bugs came from: "WAL savepoint rollback corruption after cache spill" and "frame-1 checksum after savepoint rollback and cache spill" (`CHANGELOG.md:1268, 527`). The same family, an undo that interacts with eviction, produced finding D1 of the 2026-10-03 re-audit (§5.3). |
| I/O abstraction (TS-03) | `Storage` trait with `create`, `rename`, `remove` and `sync_dir`. `FaultStorage` models when a name becomes durable. | The `IO` trait includes a clock and RNG (`core/io/mod.rs:424, 479-486`) but has no `rename` and no directory sync (the only namespace operation is `remove_file`, `core/io/unix.rs:75`). `MemoryIO::sync` does nothing (`core/io/memory.rs:302-306`). | lpg's model is stronger for namespace durability. SQLite itself syncs the directory after creating a journal (`/tmp/sqlite/src/os_unix.c:3941, 6773`). Turso injects time and randomness through its IO trait. lpg does not need this today: `Tuning` already overrides the checkpoint triggers. |
| Deterministic interleaving | Real threads. Tests set up an order with sleeps and polling (`src/tx/commit/tests.rs`, `audit_a_failed_capture_never_reaches_the_wal`, sleep of 300 ms). | Named yield points and failure points at state-machine boundaries, compiled only for tests (`core/mvcc/yield_points.rs:5-90`, `yield_hooks.rs:11-69`). | Adopt the idea as test-only pause and fail points (R10). |
| Concurrency checking (TS-30) | TS-30 plans `loom`. Code uses `std::sync` directly. | A `crate::sync` facade that switches to `shuttle` under `cfg(shuttle)` (`core/sync.rs:1-40`). Shuttle runs in CI (`.github/workflows/fuzz.yml:127-153`). | A facade is the precondition for either tool (R11). |
| MVCC (Sprint 2, TS-14/15) | Private write sets, before-images in memory, commit-time validation under the latch, serializable by default. | Hekaton-style versions with commit dependencies (`core/mvcc/database/mod.rs:1449-1460`) and an eager write-write conflict check (`:10442-10470`). Snapshot isolation. | Different isolation design, so it is not a model for TS-15. Its isolation tests and Elle harness are directly useful (R12). |

## 3. Classes of bugs from Turso's history (`CHANGELOG.md`) mapped to lpg

| Turso entry (line) | Class | lpg status |
|---|---|---|
| "fsync DB file before truncating WAL after checkpoint" (1977); "not fsyncing the DB file after truncate checkpoint" (2006) | Checkpoint sync order | Implemented correctly (`src/storage/checkpoint.rs:148-166`). Not pinned by any test. |
| "WAL truncate checkpoint discarding uncheckpointed frames" (2004) | Recycling WAL that is still needed | Covered by `checkpoints_recycle_segments_and_their_old_frames_are_never_replayed`. |
| "assert checkpoint backfill crash-atomicity (reveals a NORMAL-mode torn-recovery bug)" (567) | Crash in the middle of a checkpoint | No crash-point test inside the checkpoint (R1). |
| "savepoint rollback corruption after cache spill" (1268, 527) | Undo interacting with eviction | Found in lpg: D1 (§5.3). |
| "prevent persistent 'Database is busy' after failed IO during checkpoint" (1767) | Error path that leaves shared state behind | Same class as audit finding B2. No sweep over each I/O call exists (R2). |
| "Fix MVCC begin race with blocking checkpoint" (511); "snapshot isolation violation for passive MVCC checkpoint" (528) | Checkpoint against a snapshot | Sprint 2 (R12). |

## 4. Test dimensions

### 4.1 Turso

| Dimension | What and how | Determinism, faults, oracle | Cost and CI |
|---|---|---|---|
| `testing/simulator` (limbo_sim) | Random SQL plans with properties. Memory IO keeps a volatile and a durable copy in 4 KiB blocks (`runner/memory/io.rs:25-96`). | Seeded (`runner/cli.rs:35-41`). Faults: read, write and truncate errors, latency and reordering (`profiles/io.rs:45-53`), power loss that drops unsynced blocks (`runner/env.rs:1374-1378`). Fsync faults are **disabled** because of false positives (`runner/file.rs:193-198`). No ENOSPC, no tears inside a block. Oracles: shadow model, `integrity_check`, a differential run against SQLite, and a double run that checks determinism. Shrinks failures and keeps a bug base (`shrink/plan.rs`, `runner/bugbase.rs`). | 1 000 interactions per run, in a CI matrix of about 55 runs (`.github/workflows/rust.yml:296-353`). A Docker loop runs for 24 h. |
| `testing/concurrent-simulator` (Whopper) | Concurrent fibers or real processes. | Seeded yield injection (`yield_injection.rs:1-30`). Bit flips, allocation failures, SIGKILL of worker processes (`multiprocess.rs:738-748`). `sync` does nothing (`io.rs:444-450`), so no lost-write model. Oracles: invariants (`properties.rs`), Elle histories (`elle.rs`). No shrinking. | A fast run of 100k steps takes about 1.25 s (`CONTRIBUTING.md:357-375`). CI `rust.yml:517-606`. Elle runs nightly (`elle.yml:3-20`). |
| `testing/antithesis` | Bank invariant, `integrity_check`. | Deterministic hypervisor, commercial service. | 4 h per day, 24 h on Saturday (`antithesis-schedule.yml:4-12`). |
| `testing/unreliable-libc` | `LD_PRELOAD` over `pwrite`, `pwritev`, `fsync`, `ftruncate` (`file.c:37-198`). | 1 % ENOSPC, 1 % EIO, 5 % short writes (`file.c:13-18`), seeded with `srand48` (`:199-211`). No read faults, no crash. | Runs in Antithesis only. |
| `testing/differential-oracle` | Generated SQL compared with rusqlite. | Seeded. In-memory IO without faults. Shrinks failing statements (`fuzzer/shrink.rs`). | Seeds 1–6 × 6 profiles × 1 000 statements (`rust.yml:403-446`). |
| `testing/stress` | Multithreaded DML. | Optional Shuttle scheduling (`main.rs:537-560`). Oracle: `integrity_check` in both Turso and SQLite. | `fuzz.yml:75-153`. |
| `testing/sqlancer`, `testing/sqlright` | NoREC, TLP and PQS oracles; AFL fuzzing. | Not deterministic. | Docker images or manual runs. |
| `tests/fuzz`, `fuzz/` | Differential fuzzing of expressions and DDL; libFuzzer targets. | Seed from the `SEED` variable, otherwise from the clock (`tests/integration/common.rs:535-547`). | `fuzz.yml:21-73, 155-178` (`max_total_time=300`). |
| `tests/leak` | A counting global allocator over open, workload and drop cycles, checked against a byte budget (`tests/leak/mod.rs:1-62, 175-236`). | Real IO. | 10 min in CI (`rust.yml:143-144`). |
| `tlaplus/` | WAL multiprocess protocol and transaction concurrency control (`MultiprocessWal.tla:112-182`, `SqliteTx.tla:64-94`). | Model checking. Crashes are not modelled (`MultiprocessWal.tla:39-41`). | About 32 M states, a few minutes. Not run in CI. |
| `core/mvcc` tests | About 744 tests: torn and corrupt log tails, metadata-recovery matrices, Hermitage (17 tests), group commit (20 tests), GC property tests. | Fixed yield and failure injectors (`database/tests.rs:58-210`). | `cargo nextest` in CI. |
| `tests/integration/unreliable_io.rs` | Tracks volatile and durable bytes. Can produce three outcomes: process crash, power loss, and a crash during one file's fsync with torn writeback (`:1-30`). | Deterministic. The torn subset is fixed (alternating). | Unit-test speed. |

### 4.2 libSQL

| Dimension | What and how | Faults and oracle | CI |
|---|---|---|---|
| `libsql-server` integration tests | Replication, namespaces, restarts, under turmoil network and time simulation (`tests/common/net.rs:22-34`). | No network faults are used. A "restart" is graceful. Oracles: asserts and `insta` snapshots. | `rust.yml:89-135` |
| Vendored SQLite TCL suite | Crash VFS: unsynced writes are kept in a write list, and a crash at an fsync writes, drops or garbles a subset of them, depending on the simulated device (`libsql-sqlite3/src/test6.c:13-60`). `faultsim` fails the *N*-th operation for *N* = 1, 2, … until no fault fires, then checks the result (`test/malloc_common.tcl:41-50, 326-385`). | Faults: IOERR transient and persistent, FULL, OOM, CANTOPEN. Oracle: `integrity_check` and the database signature. | CI runs `veryquick`, which **excludes** `crash*`, `walcrash*`, `*ioerr*`, `*malloc*` and `*fault*` (`test/permutations.test:117-129`). |
| Fuzzing | AFL harness for the parser only (`tools/fuzz/src/main.rs:7-17`). | — | None |

libSQL has no Rust-level storage fault injection. Its value as a reference is the SQLite fault-loop and crash-VFS method.

## 5. lpg's current strategy, measured

### 5.1 Fault model: lpg `FaultStorage` (`src/storage/fault.rs:1-49`) compared with the references

| Fault | lpg | Turso | SQLite (libSQL) |
|---|---|---|---|
| Unsynced writes lost at a crash | Yes, all or a seeded subset | limbo_sim (4 KiB blocks); `unreliable_io` | test6.c |
| Writes torn inside a block | Yes, 512-byte sectors (`Crash::Tear`) | No (limbo_sim); one fixed pattern in `unreliable_io` | Yes |
| Namespace changes lost without a directory sync | Yes | No (no `rename` or directory sync in `IO`) | Directory sync modelled |
| EIO on write or sync | One chosen call (`fail_write`, `fail_sync_data`) | Random (unreliable-libc); fsync faults off in limbo_sim | Every *N* (faultsim) |
| ENOSPC | Space budget | unreliable-libc 1 % | `full` |
| EIO on read, short read or write, bit flip on read | No | Bit flips (Whopper), short writes (unreliable-libc) | Yes |
| Failure of `rename`, `sync_dir`, `remove` or `create` (other than ENOSPC) | No | No | CANTOPEN |
| Crash at the *k*-th call, swept over *k* | No; the test chooses the crash point | `arm_crash_on_sync`, one point per test | Crash at an fsync; `faultsim` loops over *N* |
| Linux behaviour after a failed fsync (data dropped, page cache clean) | No: a failed `sync_data` keeps the writes, so a retry succeeds (`fault.rs:15-16`) | No | No |

lpg never retries a failed sync: the unusable latch is set (TS-12). The optimistic model in the last row therefore hides no lpg bug today. It would hide a retry added later.

### 5.2 Test speed, measured 2026-10-03 on the reference machine (16 threads)

| Measurement | Result |
|---|---|
| Full suite, current tree, `opt-level` 0 (the default test profile) | 28.5 s, 28.9 s, 29.2 s (3 runs). The slowest test is `a_million_random_puts_and_deletes_match_a_btreemap_model` at 24.4 s. |
| Same, `CARGO_PROFILE_DEV_OPT_LEVEL=1`, run alternately with the line above | 16.9 s, 15.5 s, 18.6 s. The slowest tests are 7–8.7 s. Debug assertions stay on. |
| Incremental build after a change to `lpg`, `opt-level` 0 against 1 | 4.8 s against 7.6 s |
| Durability tests (`storage::recovery`, `tx::commit`, `storage::checkpoint`, 35 tests) on ext4 `/tmp` against tmpfs `/dev/shm` | 3.05 s against 3.11 s wall time. The file system is not the bottleneck once `fault()` skips the device flush. An in-memory `Storage` (Turso `MemoryIO`) would bring no measured gain. |

### 5.3 Defects found while measuring

- `storage::btree::tests::concurrent_readers_during_splits_see_every_committed_key_exactly_once` failed in 1 of 2 full-suite runs ("scan not strictly ascending").
  - With 16 concurrent instances it failed 12 of 80 times on the current tree and 14 of 80 times at base `a8f2cf7`, so it is a pre-existing defect from task #14.
  - At `opt-level` 1 it failed in 2 of 3 full-suite runs, against 0 of 3 at `opt-level` 0. A faster build exposes interleaving bugs more often.
- D1 of the 2026-10-03 re-audit (test `audit_a_page_taken_by_an_undone_batch_stays_readable_after_eviction`): after an ENOSPC undo, a page the undone batch took is clean and evictable. A reader that fetches it hits a checksum failure, and the database becomes unusable.

## 6. Ranked recommendations

Scope: **in** means within the definition of the named task; **new** means it must be proposed to the user before any work starts.

| # | Recommendation | Reference | Task | Scope | Expected gain | Cost | Effect on test speed |
|---|---|---|---|---|---|---|---|
| R1 | **Crash-point sweep.** For a small fixed workload (for example 20 commits plus one checkpoint and one segment creation), crash at every *k*-th `sync_data`, `sync_dir`, `rename` and `write_at`, in each `Crash` mode. Oracle: the committed-prefix model plus the integrity check. | SQLite crash at an fsync (`test6.c`); `faultsim` loop (`malloc_common.tcl:356-385`); Turso `arm_crash_on_sync` | #20 | in | Every crash point of every write path, instead of points chosen by hand. This is the #20 definition of done. | A "crash after call *k*" hook in `FaultStorage`, plus a workload runner | Linear in the number of calls. Keep the workload under about 300 calls and run the trials in parallel. |
| R2 | **Nth-failure sweep.** EIO or ENOSPC on the *n*-th call of each kind, including new faults on `read_at`, `rename`, `sync_dir`, `create` and `remove`. Oracle: ENOSPC fails only that commit and the database stays usable; any other failure sets the unusable latch; after a reopen, exactly the committed state is present. | `faultsim` ioerr transient and persistent (`malloc_common.tcl:41-50`); Turso `CHANGELOG.md:1767` | #20 | in | Catches error paths that leave shared state behind (the class of audit finding B2) | New fault kinds in `FaultStorage` | Same as R1 |
| R3 | **Call-order oracle for checkpoint and close**: data `sync_data` before the `CONTROL` write, before any recycle `rename` or `remove`. | Turso `CHANGELOG.md:1977, 2006` | #20 | in | Pins the TS-11 order against regressions | About 30 lines of test | Milliseconds |
| R4 | **Undo, eviction and reader tests.** Pin D1 and its free-list variant (stale image on disk). | Turso `CHANGELOG.md:527, 1268` | #15 fix, #20 test | in | Keeps FR-REC-007 true under memory pressure | Test written (D1) | Under 1 s |
| R5 | **Rotating seeds.** Use a random seed unless `LPG_SEED` is set, and print it on failure. Apply to every seeded fault test. | Turso `core/storage/btree.rs:11828-11841`, `tests/integration/common.rs:535-547` | #20 | in | Each run explores new crash and tear outcomes; failures stay reproducible | Small | None |
| R6 | **`opt-level` 1 for the test profile.** | §5.2; Turso keeps `fuzzing` and `dev` per-package overrides (`Cargo.toml:179-207`) | Any Sprint 1 task (proposal: #20) | new | Suite time drops from about 29 s to about 15–19 s (−40 % to −47 %). Interleaving bugs show up more often (§5.3). | +2.8 s per incremental build | Faster |
| R7 | **Stress profile for concurrency tests**: run the concurrency tests in *N* concurrent copies, nightly or before a task closes. | Turso CI runs each simulator profile ×5 to ×100 (`rust.yml:296-353, 517-606`) | #20 (Sprint 1 paths); #14 to be reopened for the B+tree scan defect | new | Found the #14 scan defect at 12 of 80 runs | Script only | Not in the default run |
| R8 | **Linux behaviour after a failed fsync in `FaultStorage`**: a failed `sync_data` drops the unsynced writes, or reports them as clean. | PostgreSQL's handling of failed fsync (the unusable latch in TS-12 follows it) | #20 | in | Stops a future retry path from passing tests | Small | None |
| R9 | **Misdirected and lost writes**: add a fault that writes a page at the wrong offset, or drops a synced write. Bind the page number into the page CRC (seed the CRC with it). | PostgreSQL `checksum_impl.h:172, 193`; Turso and lpg both lack it (§2) | Sprint 1, before the format is frozen | new; changes TS-05 and TS-12 (page format) | Detects a valid page at the wrong place on read | One CRC seed plus a fault mode | None |
| R10 | **Test-only pause and fail points** between apply and join, inside the checkpoint revisit loop, and inside segment creation, to replace sleeps and polling. | Turso `core/mvcc/yield_points.rs:5-90`, `yield_hooks.rs:11-69` | #20 | in | Deterministic interleavings for B2, B3 and D1, with no sleeps | `cfg(test)` hooks | Faster (removes sleeps of 300 ms and polling loops) |
| R11 | **`crate::sync` facade**, so that `loom` or `shuttle` can model the commit latch, the WAL writer and the checkpointer. | Turso `core/sync.rs:1-40`; `fuzz.yml:127-153` | Sprint 2 | new; adds `shuttle` to TS-30 "Concurrency" | Random-schedule exploration of the protocols where B2, B3 and D1 lived | A module, plus replacing imports | Separate profile |
| R12 | **Isolation oracles for Sprint 2**: the 17 Hermitage scenarios, and Elle checks of list-append and rw-register histories. | Turso `core/mvcc/database/hermitage_tests.rs:104-891`; `testing/concurrent-simulator/elle.rs`; `elle.yml` | Sprint 2 | new; adds them to TS-30 | Machine-checked G0/G1/G2 anomalies for FR-TX-003..008 | Elle needs Java (installed) | Elle offline, nightly |
| R13 | **Leak and accounting test**: a counting global allocator over open, workload and close cycles, compared with the memory accountant. | Turso `tests/leak/mod.rs:1-62, 175-236` | #22 (accountant against real allocation) | in | Measured gap between accounted and allocated memory (unverified point 15) | Small | Separate binary |

Not recommended:

- **An in-memory `Storage` for speed.** §5.2 shows no gain.
- **Antithesis.** It is a commercial service.
- **TLA+ now.** Turso's specs do not model crashes (`MultiprocessWal.tla:39-41`). lpg's open defects are better pinned by R1, R2 and R10.
- **Simulator-style SQL generation.** No query layer exists before Sprint 3.

## 7. Candidates for copying (CLAUDE.md "Copying a component")

Both projects are MIT. The SQLite files vendored in libSQL are public domain. Nothing has been copied.

| Candidate | Path at commit | lpg need or task | Exact match or starting point | Adaptation needed |
|---|---|---|---|---|
| Hermitage isolation scenarios | turso `core/mvcc/database/hermitage_tests.rs` @ `fc98dac` (921 lines) | Sprint 2, TS-14/15 tests | Starting point | Replace SQL with lpg's transaction API. Change the expected outcomes: Turso asserts snapshot isolation with first-updater-wins; lpg is serializable by default, so G2 and write skew must abort. |
| Elle history recorder | turso `testing/concurrent-simulator/elle.rs` @ `fc98dac` (317 lines) | Sprint 2, R12 | Starting point | Feed it from lpg transactions. elle-cli is used as an external tool, not copied. |
| Yield and failure injection points | turso `core/mvcc/yield_points.rs`, `core/mvcc/yield_hooks.rs` @ `fc98dac` (101 + 99 lines) | #20, R10 | Starting point | lpg uses threads, not state machines: a point must block on a test-controlled barrier instead of returning a yield completion. Must stay under `cfg(test)`. |
| Synchronisation facade | turso `core/sync.rs` @ `fc98dac` (212 lines) | Sprint 2, R11 | Starting point | Add `loom` (TS-30) and `Condvar`. Must keep `#![forbid(unsafe_code)]`. |
| Leak harness | turso `tests/leak/mod.rs` @ `fc98dac` (238 lines) | #22, R13 | Starting point | lpg workload; compare with `MemoryAccountant`. |
| `LD_PRELOAD` fault library | turso `testing/unreliable-libc/file.c` @ `fc98dac` (212 lines, C) | #21 (optional faults in the `kill -9` child) | Starting point | lpg's test binary imports `pwrite64`, `pread64`, `fdatasync` and `rename` (`nm -D` on the test binary), and the library intercepts none of them. It must add those, plus a seed. This is test-only C outside the crate, so `forbid(unsafe_code)` does not apply to it. |
| Fault-loop method | libsql `libsql-sqlite3/test/malloc_common.tcl:326-385` @ `d6c75af` (TCL) | #20, R1 and R2 | Method only | Written in TCL, so it would have to be reimplemented in Rust. Copying the file brings nothing. |
| Crash VFS | libsql `libsql-sqlite3/src/test6.c` @ `d6c75af` (C) | #20 | Not a candidate | lpg's `FaultStorage` already covers lost, subset and torn writes, plus namespace durability. |
| Unreliable IO | turso `tests/integration/unreliable_io.rs` @ `fc98dac` | — | Not a candidate | Weaker than `FaultStorage`: one fixed torn pattern, and no namespace model. |

## 8. Effect on specifications and `rmp` tasks

- **TS-30 "Durability"** should name the crash-point sweep and the Nth-failure sweep (R1, R2), and the extra fault kinds (R2, R8, R9), if adopted.
- **TS-30 "Concurrency"** should add `shuttle` next to `loom` (R11), and Hermitage and Elle (R12), if adopted.
- **TS-05 and TS-12** (page header and checksum) change if the page number is bound into the CRC (R9).
- **rmp #20**: R1–R5, R8 and R10 fit its definition. R6 and R7 are new scope.
- **rmp #14** should be reopened for the concurrent-scan defect (§5.3). This is the user's decision.
- **rmp #21**: unreliable-libc is optional and needs the adaptation described in §7.
- **rmp #22**: R13.

## 9. Unverified

- The root cause of the #14 scan defect. It has been reproduced, not diagnosed.
- Turso bug details beyond the one-line `CHANGELOG.md` entries, because the history is shallow.
- Whether `opt-level` 1 changes the outcome of any test other than the flaky one. In 3 runs, only the known failures appeared.
- libSQL's `nemesis` workflow: its fault model is in a private repository (`.github/workflows/nemesis.yml:67-84`).

## 10. Proposed index row for `docs/evidence/README.md`

| ID | Folder | What it studied or measured | Date | Reproduce |
|---|---|---|---|---|
| R-TURSO-TEST | [research/research-04-turso-libsql.md](research/research-04-turso-libsql.md) | Turso and libSQL kernel components and test dimensions compared with lpg Sprint 1; test-speed and flake measurements in §5 | 2026-10-03 | Read only (source study); §5.2 commands: `cargo nextest run -p lpg` with and without `CARGO_PROFILE_DEV_OPT_LEVEL=1`; `TMPDIR=/dev/shm` |
