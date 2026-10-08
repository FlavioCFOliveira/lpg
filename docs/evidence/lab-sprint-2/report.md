---
title: Sprint 2 correctness laboratory
status: living record
last-reviewed: 2026-10-08
related:
  - ../README.md
  - ../lab-sprint-1/report.md
---

# Sprint 2 correctness laboratory

Record of the laboratory used to verify Sprint 2, the transactional graph store (rmp #24 lab, #27 MVCC, #28 isolation, #29 garbage collection, #30 values), and of the gates of audit Round A, run on 2026-10-08. Sprint 2 verifies correctness only. No timing figure in this record is a performance claim; the seconds the suites print measure how long the runs took and nothing else.

## Environment

| Item | Value | Source |
|---|---|---|
| CPU | AMD Ryzen 9 5900HX, 16 logical CPUs, 2 threads per core | `lscpu` |
| Memory | 30 GiB | `free -g` |
| Operating system | Ubuntu 26.04.1 LTS, kernel 7.0.0-38-generic | `/etc/os-release`, `uname -r` |
| `LPG_LAB_EXT4` | `/data/tmp/lpg-lab`: ext4 on `/dev/nvme1n1p1` (Samsung SSD 970 EVO Plus 1TB), `rw,nosuid,nodev,relatime` | `findmnt -T`, `/sys/block/nvme1n1/device/model` |
| `LPG_LAB_XFS` | `/mnt/lpg-xfs`: XFS on `/dev/loop0`, backed by `/data/tmp/lpg-xfs.img` (8 GiB) on the ext4 `/data`, loop direct I/O on (`/sys/block/loop0/loop/dio` = 1), `rw,relatime,inode64,logbufs=8,logbsize=32k,noquota` | `findmnt -T`, `losetup -l` |
| `/tmp/lpgram` | `tmpfs` (`rw,nosuid,nodev,nr_inodes=1048576,inode64,usrquota`). It speeds up the default test run only. It is not a production scenario, and no durability or I/O claim rests on it (CLAUDE.md). | `findmnt -T /tmp` |

The NVMe device name changed from `nvme0n1` (Sprint 1 record) to `nvme1n1`, and the loop device from `loop80` to `loop0`. The device model and the image file are the same.

## Tools

| Tool | Version | Command or source |
|---|---|---|
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | `rustc --version` |
| Rust MSRV | 1.89.0 (29483883e 2025-08-04) | `rustc +1.89 --version` |
| cargo-nextest | 0.9.146 (8af696ddc 2026-09-21) | `cargo nextest --version` |
| cargo-deny | 0.20.2 | `cargo deny --version` |
| loom | 0.7.2 (dev dependency, `cfg(loom)`) | `Cargo.lock` |
| shuttle | 0.9.5 (dev dependency, `cfg(shuttle)`) | `Cargo.lock` |
| jiff | 0.2.38, features `std`, `tzdb-zoneinfo` | `Cargo.lock`, `Cargo.toml` |
| elle-cli | 0.1.11, standalone jar at `~/opt/elle-cli-0.1.11/target/elle-cli-0.1.11-standalone.jar` (SHA-256 `85c39a6247509075bf21762d4c8ed3a2cb3d2c9b2c169cb2c80dde25fa760388`), bundling Elle 0.2.7 (revision `d1606aa7cf65146104d06d6a173e6f6d197237f7`, from the jar's `META-INF/maven/elle/elle/pom.properties`) | `sha256sum`, `unzip -p` |
| Java | OpenJDK 25.0.4.1 (build 25.0.4.1+1-1-26.04.4-Ubuntu) | `java -version` |
| IANA tzdata | 2026e, `tzdata2026e.tar.gz` (SHA-256 `b26882805f26aac59d5b222978e6580484b834ccdc98be89df2f05a6dc53a652`), fetched 2026-10-08 from `data.iana.org`; public domain (the release's `LICENSE`). It is the source of `src/types/zone_links.rs`. | `curl`, `sha256sum` |

The tests find elle-cli through `LPG_ELLE_CLI`, or else at the path above (`src/test_support/history.rs`, `elle_jar`).

## Model checkers

The crate imports every synchronisation primitive through `crate::sync`, which switches to `loom` or `shuttle` under their `cfg` (TS-30 R11).

| Checker | Command | Tests | Result (2026-10-08) |
|---|---|---|---|
| loom | `RUSTFLAGS="--cfg loom" cargo test --lib --target-dir target/loom loom_` | 8 models: memory accountant, lock table (4), published timestamps, snapshot registry against the GC horizon (first block, overflow block) | 8 passed, 17.30 s |
| shuttle | `RUSTFLAGS="--cfg shuttle" cargo test --lib --target-dir target/shuttle shuttle_` | 3: memory accountant, commits and checkpoints, batches and checkpoints | 3 passed, 18.44 s |

**The registry models fail when their synchronisation is removed.** Each mutation below was applied alone, the two registry models were run, and the source was then restored.

| Mutation of `src/storage/freelist.rs` | `loom_no_snapshot_…` (first block) | `loom_no_overflow_snapshot_…` |
|---|---|---|
| None | passed | passed |
| SeqCst fence after the first-block CAS removed | **failed** ("horizon 2 above snapshot 1") | passed |
| SeqCst fence in `horizon` removed | **failed** ("horizon 2 above snapshot 1") | passed |
| Overflow slot stored after the overflow mutex is released | passed | **failed** ("horizon 2 above snapshot 1") |

The overflow path is ordered by the overflow mutex, not by fences. A fence after an overflow registration was tried and then removed, because the model passed without it. The first-block model needs both fences.

**Limits of loom and shuttle.**

- loom treats `SeqCst` loads and stores as `AcqRel`, which is weaker, so it may report failures that cannot happen; `fence(SeqCst)` is modelled (loom 0.7.2 `README.md:70-74`). A passing model is therefore not weakened by this limit.
- loom explores small models only. Under `cfg(loom)` the registry has one slot per block (`SLOTS = 1`), so the models cover one registration in the first block, or one in an overflow block with the first block held.
- loom and shuttle model no time. `crate::sync::thread::pause` only yields under them.
- shuttle runs random schedules. Its documentation says that a passing test does not prove the code correct (D-SHUTTLE, shuttle 0.9.5).
- `Arc` comes from `std` under both checkers (`src/sync.rs`), so `Arc` leaks are not checked.
- The test-only fail point `gc::FAIL_SEED` is compiled out under `loom` and `shuttle`.

## Elle

The tests run transactions of Elle's list-append and rw-register workloads against lpg, record them as EDN histories under `target/lpg-histories/` (evidence, not graph data), and check them with `java -jar elle-cli … --model <model> --consistency-models <level>` (`src/test_support/history.rs`, `src/tx/tests/elle.rs`).

| Test | Workload | Model checked | Expected |
|---|---|---|---|
| `elle_accepts_a_two_thread_rw_register_history` | rw-register, 2 threads × 40 transactions, 3 keys, serializable | serializable | valid |
| `elle_finds_no_anomaly_in_eight_thread_list_append_histories` | list-append, 8 threads × 40, 6 keys, at serializable and at snapshot isolation | the level's own model | valid |
| `elle_finds_no_anomaly_in_eight_thread_rw_register_histories` | rw-register, 8 threads × 40, 4 keys, at both levels | the level's own model | valid |
| `elle_reports_g2_item_for_a_snapshot_isolation_write_skew` | write skew run at snapshot isolation | serializable | `G2-item` reported |
| `elle_reports_a_hand_written_g1c_cycle_invalid` | hand-written G1c history | serializable | `G1c` reported |

The last two tests show that the checker finds anomalies when they are present. All five passed in every default run below. The workloads are seeded (`LPG_SEED`, TS-30 R5).

**Limits of the Elle checks.** Elle checks only the histories the workloads produce: up to 8 threads, 40 transactions per thread, 3 to 6 keys. It infers the version order from unique written values. Tree drops, garbage collection and crashes are not part of these histories; the targeted tests and the durability suites cover them.

## Audit Round A: regression tests and mutation checks

Each fix has a test that fails when the fix is removed. Each mutation was applied alone, the named test was run, and the source was then restored.

| Finding | Test | Mutation | Result with the mutation |
|---|---|---|---|
| 1 (dedupe of the GC queue) | `tx::tests::gc::a_tree_and_an_entry_queued_twice_are_collected_once` | entry dedupe removed; tree dedupe removed | failed; failed |
| 1 (`free_tree` guard) | `tx::tests::gc::freeing_a_dropped_tree_twice_in_one_batch_frees_its_pages_once` | guard removed | failed |
| 2 | `tx::tests::isolation::a_drop_committed_after_a_read_of_the_tree_fails_the_readers_commit` | directory read not recorded | failed |
| 3 | `tx::tests::isolation::three_hundred_transactions_on_one_thread_all_begin` | registration spins on the first block (old behaviour) | did not finish; killed after 60 s |
| 4 (fixed links) | `types::tests::zone_links_are_the_formats_fixed_table_not_the_hosts` | the host's view of `Europe/Oslo` | failed |
| 4 (stored zone) | `types::tests::a_stored_zone_the_host_cannot_apply_keeps_its_name_and_offset` | decode requires the zone; the stored offset is ignored | failed; failed |
| 5 (M11) | `tx::tests::gc::a_key_put_again_after_its_delete_survives_the_pass` | stamp recheck in `pass` removed | failed |
| 5 (M15) | `tx::tests::isolation::deleting_an_absent_key_conflicts_with_its_concurrent_insert` | point read in `delete` removed | failed |
| 6 | `suites::fault::transactions` | `free_tree` frees no pages | failed |
| 7 | `tx::tests::gc::a_failed_seed_scan_keeps_the_clean_flag_clear` | a failed seed treated as done | failed |
| 8 | `tx::tests::gc::a_failed_last_pass_still_closes_with_the_clean_flag_clear` | the failure propagated; the flag set after a failure | failed; failed |
| 9 | `tx::tests::isolation::a_lock_upgrade_charges_no_second_lock_entry` | upgrade charged as a new entry | failed |

The kill-suite sample (3 iterations in `/tmp/lpgram`) did not detect finding 1 with both fixes removed, in 3 of 3 runs. The deterministic tests of finding 1 detect it.

## Space used in `/tmp/lpgram` by the default run

Each test was first measured alone (`du -sk /tmp/lpgram` every 20 ms). 35 of the 296 tests peak at 60 MiB or more; the largest is `suites::kill::kill_and_reopen` at 274 MiB. Without a cap, a default run peaked at 1 320 868 KiB (1.26 GiB). The test group `disk-heavy` (`.config/nextest.toml`) now runs at most three tests at once from the modules that hold those 35 tests. From the measured peaks, the worst case is 990 MiB.

Default runs, `cargo nextest run -p lpg --no-fail-fast`, sampled every 20 ms (each `du` takes time, so the observed interval is about 38 ms):

| Run | Result | Wall time | Peak in `/tmp/lpgram` |
|---|---|---|---|
| Without the group | 296 passed | 13.578 s | 1 320 868 KiB |
| 1 | 296 passed | 13.202 s | 610 948 KiB |
| 2 | 296 passed | 13.117 s | 559 872 KiB |
| 3 | 296 passed | 13.196 s | 646 260 KiB |
| Final (after the last change) | 296 passed | 12.855 s | 588 660 KiB |

`/tmp/lpgram` was empty after every run. A test process that is killed (by a timeout, or by `kill -9` of `nextest`) leaves its `tempfile` directory behind, because its `Drop` does not run. A run cut off by `timeout` reproduced this with one leaked 16 MiB directory. No test of a complete run leaves one.

## Gates of audit Round A (2026-10-08)

| Gate | Command | Result |
|---|---|---|
| Default test run | `cargo nextest run -p lpg --no-fail-fast` | `296 tests run: 296 passed, 24 skipped`, 12.855 s |
| ext4 and XFS suites | `LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs cargo nextest run -p lpg --no-fail-fast --no-capture -E 'test(/^suites::/)'` | `14 tests run: 14 passed`, 494.191 s; kill suite rerun after its change: `1 test run: 1 passed`, 57.032 s |
| Stress profile on ext4 | `LPG_LAB_EXT4=/data/tmp/lpg-lab cargo nextest run -p lpg --profile stress --no-fail-fast` | `38 tests run: 38 passed`, 58.082 s; 16 copies of each, 0 copies failed; `/tmp/lpgram` peak 390 832 KiB |
| loom | see Model checkers | 8 passed |
| shuttle | see Model checkers | 3 passed |
| Formatting | `cargo fmt --all -- --check` | no difference |
| Lints | `cargo clippy --all-targets -- -D warnings` | no warning |
| MSRV | `cargo +1.89 build --all-targets` | built |
| Dependencies | `cargo deny check` | `advisories ok, bans ok, licenses ok, sources ok` |
| Documentation | `cargo doc --no-deps` | 0 warnings (also 0 with `--document-private-items`) |

Suite lines on persistent storage (full runs):

    SUITE fault fs=ext4 workload=Backup crash_points=106 runs: drop-all 106, keep-random 106, tear-512 106 failure_runs=162 copy_failure_runs=0
    SUITE fault fs=xfs workload=Backup crash_points=106 runs: drop-all 106, keep-random 106, tear-512 106 failure_runs=162 copy_failure_runs=0
    SUITE fault fs=ext4 workload=BulkLoad crash_points=104 runs: drop-all 104, keep-random 104, tear-512 104 failure_runs=185 copy_failure_runs=0
    SUITE fault fs=xfs workload=BulkLoad crash_points=104 runs: drop-all 104, keep-random 104, tear-512 104 failure_runs=185 copy_failure_runs=0
    SUITE fault fs=ext4 workload=Checkpoint crash_points=31 runs: drop-all 31, keep-random 31, tear-512 31 failure_runs=29 copy_failure_runs=0
    SUITE fault fs=xfs workload=Checkpoint crash_points=31 runs: drop-all 31, keep-random 31, tear-512 31 failure_runs=29 copy_failure_runs=0
    SUITE fault fs=ext4 workload=Commits crash_points=149 runs: drop-all 149, keep-random 149, tear-512 149 failure_runs=236 copy_failure_runs=1380
    SUITE fault fs=xfs workload=Commits crash_points=149 runs: drop-all 149, keep-random 149, tear-512 149 failure_runs=236 copy_failure_runs=1380
    SUITE fault fs=ext4 workload=Migration crash_points=9 runs: drop-all 9, keep-random 9, tear-512 9 failure_runs=6 copy_failure_runs=0
    SUITE fault fs=xfs workload=Migration crash_points=9 runs: drop-all 9, keep-random 9, tear-512 9 failure_runs=6 copy_failure_runs=0
    SUITE fault fs=ext4 workload=RecoveryCheckpoint crash_points=149 runs: drop-all 149, keep-random 149, tear-512 149 failure_runs=2 copy_failure_runs=0
    SUITE fault fs=xfs workload=RecoveryCheckpoint crash_points=149 runs: drop-all 149, keep-random 149, tear-512 149 failure_runs=2 copy_failure_runs=0
    SUITE fault fs=ext4 workload=Restore crash_points=169 runs: drop-all 169, keep-random 169, tear-512 169 failure_runs=159 copy_failure_runs=0
    SUITE fault fs=xfs workload=Restore crash_points=169 runs: drop-all 169, keep-random 169, tear-512 169 failure_runs=159 copy_failure_runs=0
    SUITE fault fs=ext4 workload=Transactions crash_points=28 runs: drop-all 28, keep-random 28, tear-512 28 failure_runs=42 copy_failure_runs=97
    SUITE fault fs=xfs workload=Transactions crash_points=28 runs: drop-all 28, keep-random 28, tear-512 28 failure_runs=42 copy_failure_runs=97
    SUITE kill fs=ext4 dir=/data/tmp/lpg-lab iterations=200 printed_commits=26034 recovery_ms_p50=10.7 recovery_ms_p99=16.0 replayed_bytes_p50=230272 replayed_bytes_p99=575680 child_commits_per_s_p50=1747 kills_in_checkpoint=200 kills_in_backup=200 recycling_checkpoints=15 printed_drops=420 seconds=25.0
    SUITE kill fs=xfs dir=/mnt/lpg-xfs iterations=200 printed_commits=6366 recovery_ms_p50=20.2 recovery_ms_p99=29.5 replayed_bytes_p50=172704 replayed_bytes_p99=304288 child_commits_per_s_p50=367 kills_in_checkpoint=200 kills_in_backup=200 recycling_checkpoints=1 printed_drops=166 seconds=31.8

In the first full run of the kill suite, the transaction thread dropped its first tree only after 16 commits, and on XFS no kill came that late (`printed_drops=0`). The thread now drops at its first commit, and a full run fails when no drop was printed. The fault and kill suites ran on ext4 and XFS because they need a real `fsync`; their samples in `/tmp/lpgram` only shorten the default run.

`/tmp/lpgram`, `/data/tmp/lpg-lab` and `/mnt/lpg-xfs` were empty after the gates.

## Re-audit of Round A: findings N1 to N4 (2026-10-08)

| Finding | Change | Test | Mutation |
|---|---|---|---|
| N1 (#30) A zoned date-time before 1970 with a fractional second did not decode: jiff gives the fractional second the instant's sign, and the record wrote it as an unsigned varint. | The record keeps the floor of the Unix seconds and nanoseconds in `0..1e9`, as Neo4j's property store does (R-NEO `community/record-storage-engine/.../store/TemporalType.java:418-446`, `community/values/.../utils/TemporalValueWriterAdapter.java:68-80`); decoding rejects nanoseconds outside `0..1e9` (`src/types/record.rs:210-219, 454`). No other record field has the pattern: times of day are non-negative, offsets, years and `YEAR TO MONTH` are zigzag, and `DAY TO SECOND` keeps nanoseconds in `0..1e9`. | `a_fractional_instant_before_1970_round_trips_through_the_record_encoding`; `random_values_round_trip_through_the_record_encoding` (20 000 seeded values); `random_value` now draws instants around the epoch with and without a fraction, and `key_bytes_sort_in_the_approved_order` passes with them, so the key encoding orders them correctly. | Old encoder and decoder restored: both new tests fail (`... 1969-12-31T23:59:59.5+00:00[UTC] ... does not decode`). |
| N2 (#24) The kill suite's full-run requirement of a recycling checkpoint failed on XFS when the workload wrote too little WAL. | The first child of a full run commits filler entries (prefix `f/`, tree 2) until the durable WAL passes its first segment, then checkpoints before its workload and asserts that the checkpoint recycled or removed a segment (`fill_one_segment`, `src/suites/kill.rs:127-152`). | `suites::kill::kill_and_reopen`, 10 full runs on XFS and 2 on ext4 (below). | Not applicable: the requirement now holds by construction, and the child asserts it. |
| N3 (#24) The crash oracles did not check that delete stamps are collected after a crash. | `assert_stamps_collected` (`src/suites/mod.rs:53`): once the reopened database's seed scan has run, a pass with no snapshot open leaves no entry of a live tree with a delete stamp at or below its horizon. It runs in the fault `Transactions` workload (`src/suites/fault.rs:494`) and in the kill suite (`src/suites/kill.rs:485`). | `suites::fault::transactions`, `suites::kill::kill_and_reopen` | Seed scan made to skip delete-stamped entries: both suites fail (`keeps delete stamp 11 at or below the horizon 16`; `keeps delete stamp 927 at or below the horizon 928`). |
| N4 (#24) A garbage-collector regression hung the default run. | `test_support::wait_until` (`src/test_support/mod.rs:58-72`) bounds the waits on the collector to 30 s and fails with a message; used in four collector tests (`src/tx/tests/gc.rs:88, 419, 570, 610`) and both suites. The default nextest profile kills a test after 10 periods of 60 s (`.config/nextest.toml:28`); the slowest legitimate test, `suites::fault::restore_` in full on both lab file systems, took 189.9 s. | — | Under the N3 mutation, `after_a_crash_the_collector_finds_the_garbage_again`, `a_failed_seed_scan_keeps_the_clean_flag_clear` and `a_failed_last_pass_still_closes_with_the_clean_flag_clear` fail at 30.05 s (`not reached within 30s: ...`); the run ended in 33 s. |

Kill suite, full runs after the N2 change (before the child's assertion was added; the gate run below includes it):

| Run | File system | `recycling_checkpoints` | `printed_commits` | `kills_in_checkpoint` | `kills_in_backup` | `printed_drops` |
|---|---|---|---|---|---|---|
| 1 | XFS | 10 | 14 351 | 200 | 200 | 265 |
| 2 | XFS | 10 | 14 339 | 200 | 200 | 258 |
| 3 | XFS | 10 | 14 416 | 200 | 200 | 258 |
| 4 | XFS | 7 | 13 308 | 200 | 200 | 249 |
| 5 | XFS | 1 | 7 783 | 200 | 200 | 179 |
| 6 | XFS | 2 | 5 699 | 199 | 200 | 160 |
| 7 | XFS | 4 | 6 140 | 200 | 200 | 167 |
| 8 | XFS | 2 | 6 025 | 200 | 200 | 159 |
| 9 | XFS | 2 | 5 861 | 199 | 200 | 159 |
| 10 | XFS | 2 | 6 133 | 200 | 200 | 165 |
| 1 | ext4 | 4 | 11 776 | 199 | 200 | 227 |
| 2 | ext4 | 6 | 12 173 | 199 | 200 | 228 |

The printed commits vary between runs (5 699 to 14 416 on XFS); the cause was not investigated. The suite checks correctness only, and every count it requires was above 0 in every run.

Gates after the four fixes:

| Gate | Result |
|---|---|
| Default test run, `cargo nextest run -p lpg --no-fail-fast` | `298 tests run: 298 passed, 24 skipped`, 12.830 s; `/tmp/lpgram` peak 616 108 KiB (339 samples, about 38 ms apart) |
| ext4 and XFS suites, `-E 'test(/^suites::/)'` | `14 tests run: 14 passed (3 slow)`, 493.976 s |
| Stress profile on ext4 | `38 tests run: 38 passed`, 58.667 s; 16 copies of each, 0 copies failed; `/tmp/lpgram` peak 356 576 KiB |
| loom | 8 passed, 17.54 s |
| shuttle | 3 passed, 18.37 s |
| Formatting, lints | no difference; no warning |
| MSRV, `cargo +1.89 build --all-targets` | built |
| `cargo deny check` | `advisories ok, bans ok, licenses ok, sources ok` |
| `cargo doc --no-deps` | 0 warnings |

    SUITE kill fs=ext4 dir=/data/tmp/lpg-lab iterations=200 printed_commits=26965 recovery_ms_p50=10.3 recovery_ms_p99=16.6 replayed_bytes_p50=263168 replayed_bytes_p99=518112 child_commits_per_s_p50=1692 kills_in_checkpoint=200 kills_in_backup=198 recycling_checkpoints=20 printed_drops=428 seconds=28.8
    SUITE kill fs=xfs dir=/mnt/lpg-xfs iterations=200 printed_commits=5986 recovery_ms_p50=20.7 recovery_ms_p99=31.1 replayed_bytes_p50=172704 replayed_bytes_p99=296064 child_commits_per_s_p50=353 kills_in_checkpoint=200 kills_in_backup=200 recycling_checkpoints=4 printed_drops=157 seconds=32.8
    SUITE fault fs=ext4 workload=Transactions crash_points=28 runs: drop-all 28, keep-random 28, tear-512 28 failure_runs=42 copy_failure_runs=97 seconds=5.0
    SUITE fault fs=xfs workload=Transactions crash_points=28 runs: drop-all 28, keep-random 28, tear-512 28 failure_runs=42 copy_failure_runs=97 seconds=4.8

The other fault-suite lines have the same counts as in the Round A gates. `/tmp/lpgram`, `/data/tmp/lpg-lab` and `/mnt/lpg-xfs` were empty after the gates.

## Audit round 3: findings L1 and L2 (2026-10-08)

| Finding | Fix | Test | Mutation (fix removed) |
|---|---|---|---|
| L1: the record encoder wrote zoned-time offsets outside ±93 599 s and day-to-second nanoseconds of 1e9 or more, which the decoder rejects; `temporal::parse` produced such offsets (`10:00:00+99:00`) | `record::encode` returns `TooLarge::Range` for them (`src/types/record.rs`, `put_value`); `split_offset` checks its arithmetic and returns a jiff `Offset` (`src/types/temporal.rs`) | `the_record_encoder_accepts_exactly_what_the_decoder_accepts` (20 000 seeded values with extreme fields: the encoder accepts exactly the in-domain values, each decodes bit-identically and re-encodes to the same bytes); `an_out_of_range_parsed_offset_is_rejected` | offset check removed: property test fails; nanosecond check removed: property test fails; parse range check removed: offset test fails |
| L2: `DayToSecond::new` wrapped on overflow; duration parsing used unchecked arithmetic (`P106751991167301DT0H0M0S` panicked in the test profile) | `DayToSecond::new` returns `Option` (`src/types/mod.rs`); parsing uses `checked_*` for both duration types | `duration_overflow_is_rejected` | `new` with `as i64`: fails; unchecked day-to-second parsing: fails; unchecked year-to-month parsing: fails |

Also closed: the remaining unbounded waits use `test_support::wait_until` (30 s): `until_waiting` (`src/tx/tests/mod.rs`), the `lpg-gc` thread-name wait (`src/tx/tests/gc.rs`) and both waits of the concurrent backup test (`src/ops/backup/tests.rs`), which now stops its committers on drop so a failed wait cannot hang the scope's join. With the bound set to 2 s and each condition made unreachable, the 8 affected tests failed in 2.0 to 2.3 s with the `not reached` message. In the kill suite, a failed filler step is printed by the child (`FILL failed: <message>`) and reported by the parent; with an injected failure the parent reported `the child's filler step failed: injected filler failure`, where before it reported `the child commits: Disconnected`.

Gates:

| Gate | Result |
|---|---|
| Default test run, `cargo nextest run -p lpg --no-fail-fast` | `301 tests run: 301 passed, 24 skipped`, 12.979 s; `/tmp/lpgram` peak 629 828 KiB (503 samples) |
| Kill suite on XFS | `1 passed`; `printed_commits=13959 kills_in_checkpoint=200 kills_in_backup=200 recycling_checkpoints=8 printed_drops=255` |
| Kill suite on ext4 | `1 passed`; `printed_commits=21207 kills_in_checkpoint=200 kills_in_backup=200 recycling_checkpoints=15 printed_drops=352` |
| Formatting, lints | no difference; no warning |
| MSRV, `cargo +1.89 build --all-targets` | built |
| `cargo doc --no-deps` | 0 warnings |

`/tmp/lpgram`, `/data/tmp/lpg-lab` and `/mnt/lpg-xfs` were empty after the gates.

Follow-up: `split_offset` now accepts only jiff's numeric offset syntax (two-digit fields, hours at most 25, minutes and seconds at most 59; jiff 0.2.38 `src/fmt/offset.rs:591-623`, jiff-core 0.1.1 `src/bounds.rs:277-281`), so `+05:99`, `+05:00:60` and `+5:00` are rejected (`an_out_of_range_parsed_offset_is_rejected`; with the minute and second limits removed it fails on `+05:99`, and with one-digit fields allowed it fails too); default run `301 tests run: 301 passed, 24 skipped`, 13.278 s, `/tmp/lpgram` peak 584 432 KiB, empty afterwards; formatting and lints clean.
