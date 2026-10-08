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

Source line references in this section are at commit `4ba70cc`; later commits moved some of them.

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

## Exit gate of Sprint 2 (rmp #35, 2026-10-08)

Code: commit `55a54a3` plus the working-tree changes of rmp #35. Raw outputs, text only: [`raw/`](raw/) (`environment.txt` records the tool versions and mounts). Sprint 2 verifies correctness only (sprint UPDATE 54); the seconds printed below are run durations, not performance claims.

**Storage of each result.** Every `SUITE` line with `fs=ext4` or `fs=xfs`, and every test of the full run that reads `LPG_LAB_EXT4` or `LPG_LAB_XFS`, ran on persistent storage: ext4 on NVMe (`/data/tmp/lpg-lab`) and XFS on a loop device with direct I/O backed by a file on that ext4 (`/mnt/lpg-xfs`). All other tests keep their graph data in `/tmp/lpgram`, a `tmpfs`; their results say nothing about durability or I/O on a disk. The fault suite runs on `FaultStorage`, which models durability itself and skips the device flush (DECISION 77 on rmp #15); the kill suite uses `FsStorage` with real `fsync`.

**What the ext4 and XFS results show.** File-system compatibility and consistency after a process crash, nothing more. `kill -9` ends the process but leaves its writes in the page cache, which the kernel writes back, and `FaultStorage` skips the device flush, so neither suite can observe a missing `fsync`: in the exit-gate audit, the kill suite passed with the WAL's `sync_data` removed (mutation M6, FINDING 245 on rmp #35). **Durability across a power loss on ext4 and XFS is unverified.** Verifying it needs a power cut, or a block-level record of writes and flushes (`dm-log-writes`, `dm-flakey`), which need root; it is TS-30 unverified point 19.

### What rmp #35 added

| Area | Change | File |
|---|---|---|
| Fault suite, `Transactions` | Every third transaction runs under snapshot isolation; each updates property `P` of a node and of an edge of the default graph; batch 10 deletes a node with its edge; after batch 2 the workload creates a range index on nodes and one on edges and waits for each online build. The model covers the graph properties. | `src/suites/fault.rs` |
| Oracle: commit timestamp | `assert_no_stamp_above`: after a reopen with index builds held (`open_held`), no entry of a live tree has a created or delete stamp, and no tree a drop stamp, above the recovered `commit_ts`. | `src/suites/mod.rs` |
| Oracle: indexes | `finish_builds`: the held builds are restarted and waited for; every index is then ONLINE, so the integrity check compares it with the data (TS-12 index check); a returned index creation must survive. | `src/suites/mod.rs` |
| Oracle: returned indexes in the kill suite (audit F1) | The child prints `INDEX end <id>` when a build returns and `INDEX drop <id>` when the drop of the index it built before commits. After `finish_builds`, `check_indexes` requires every index the parent kept ONLINE at the last close and the last index whose build returned in the child to be present and ONLINE, and every printed drop to be absent; the index before the last returned one may be absent, since its drop may have been in flight. The kept indexes make the check independent of the child's speed: on XFS no child build returned in the full run, but the kept indexes are checked in every iteration after the first. | `src/suites/kill.rs` (`check_indexes`) |
| Integrity check: catalog against directory (audit F1) | `catalog::check`, run first by `Engine::check_graphs` under the commit latch, reads the latest committed objects and index and constraint definitions from the catalog tree, not from the cache, and reports: a cache entry that differs from them; a tree they name (a graph's node, edge and adjacency trees, an index's tree and side tree, the catalog and statistics trees) that is not a live tree of the directory; a live tree that none of them names. `Engine::check_graphs_except` and `check_integrity_except` take the trees a test created outside the catalog (`create_trees`), which the fault and kill suites pass. `catalog::open` and the check share one reader of the definitions (`definitions`). | `src/catalog/mod.rs`, `src/storage/graph.rs` |
| Garbage collector (audit F3, product fix) | A pass that fails after taking its work from the queues now puts the work back on every error path: the reads that find what is left to do (`get_stamped`, `tree_status`, which return `MemoryLimit` from `Pool::fetch` when every frame is pinned or dirty) and the internal batch. The work after the dequeue moved into `remove`, whose error the caller handles once. Before, the `?` of those reads returned without the requeue, and the work was lost until an unclean open. Test-only fail point `Mvcc::gc_fail_read`. | `src/tx/gc.rs` (`pass`, `remove`), `src/tx/mvcc.rs` |
| Test support: `pause_gc` | `pause_gc` returns only after a garbage-collection pass in flight ends (`gc_paused` held by the `lpg-gc` thread across its pass); removes a race that failed `own_writes_are_private_and_an_abort_leaves_no_trace` 39 times in 38 400 parallel runs (0 after) | `src/tx/mvcc.rs`, `src/tx/gc.rs`, `src/tx/tests/mod.rs`, `src/suites/fault.rs` |
| File system of each run (audit F7) | `suite_dirs` measures each directory's file-system type and source with `findmnt --target` and fails the run if a lab directory's type differs from its variable's; the `SUITE` lines print `fs=<type> source=<device>`. | `src/suites/mod.rs` |
| Kill suite | Transactions alternate serializable and snapshot isolation and write property `P` on nodes (label `N` as well as the iteration's label) and edges; a sixth thread builds node and edge indexes in turn, keeping the previous one ONLINE; the parent applies both new oracles and keeps the newest node and edge index for the next child. | `src/suites/kill.rs` |
| Hermitage | All ten scenarios run at both levels on tree entries and on nodes of the default graph (node reads, property updates, node creation and deletion, scans of the node tree). | `src/tx/tests/hermitage.rs` |
| Elle | List-append on node list properties and rw-register on edge properties, 8 threads × 40 transactions, at both levels, with the seven templates of Turso's chaotic Elle workloads (copied, see below). | `src/tx/tests/elle.rs` |
| Stress profile | `elle_finds_no_anomaly_in_eight_thread_graph_histories` added. The Hermitage tests already in the filter now include the node cases. | `.config/nextest.toml` |

**Copied code.** The seven transaction templates and their weights come from Turso `testing/concurrent-simulator/chaotic_elle.rs` at commit `8cc059b` (MIT, "Copyright 2024 the Turso authors"), adapted to this crate's recorder; the notice is kept in `src/tx/tests/elle.rs`. Elle (EPL-2.0) is only run as the external `elle-cli` tool.

### Mutation checks of the new oracles

Each mutation was applied alone, the named tests were run in the `/tmp/lpgram` sample, and the source was then restored (byte comparison with a saved copy).

| Oracle | Mutation | Result |
|---|---|---|
| Prefix model with graph properties (fault) | The model forgets returned transaction 5 | `suites::fault::transactions` failed: "the visible state is not a prefix with every returned commit" |
| No stamp above the recovered `commit_ts` (fault, kill) | Recovery ignores the `commit_ts` of the replayed groups (`src/storage/recovery.rs`, step 6) | Both failed in `assert_no_stamp_above`: "stamped Stamps { created: 13, … } above the recovered commit_ts 0"; "created: 3329 … above the recovered commit_ts 3298" |
| Index check (fault) | Commits keep stale edge-index entries (old keys not removed) | `suites::fault::transactions` failed: "index 4: entry … matches no record of element 8" |
| Index check (kill) | Commits add no edge-index entries | `suites::kill::kill_and_reopen` failed: "index 23: element 3110 has no entry". The stale-entry mutation above is not reachable by the kill workload, which never updates an edge. Before the child kept an index ONLINE, the kill suite did not detect this mutation either. |
| Hermitage on nodes | Serializable validation disabled (`Committed::validate` returns `Ok`), node cases only | G1c, G2-item and G2 failed |
| Elle on graph operations | Same mutation | 3 of 3 runs failed with `G2-item` (serializable list-append) |

**Audit fixes (F1, F3).** Mutation M3 of the exit-gate audit drops the ONLINE index definitions from the catalog cache at open (`schema.retain(|_, d| d.state != State::Online)` in `catalog::open`). Each run below was made with that mutation alone, or with the fix reverted, and the source was then restored (byte comparison with a saved copy).

| Oracle | Mutation | Result |
|---|---|---|
| A returned index creation survives the crash (fault) | M3 | `suites::fault::transactions` (sample) failed in the integrity check: "Transactions CrashAfter(80, DropAll): [\"definition 3: the cache holds None, the catalog tree Some(SchemaDef { id: 3, … state: Online, tree: 8, … })\"]" |
| A returned index creation survives the crash (kill) | M3 | `suites::kill::kill_and_reopen` failed in every run: `/tmp/lpgram` sample, "iteration 0: index 21, whose build returned, is lost"; full run on ext4, "iteration 1: index 3, kept ONLINE at the last close, is lost"; full run on XFS alone, the same message |
| Catalog against directory (integrity check) | M3 | `index::tests::index::the_integrity_check_reconciles_the_directory_with_the_catalog` failed after its clean reopen: "definition 3: the cache holds None, the catalog tree Some(…)". Without the mutation the same test passes; it injects a tree that no definition names (reported as `tree <id>: no catalog definition names it`, and not reported once passed as a raw tree) and drops an ONLINE index's tree (reported as `definition <id>: its tree <id> is missing or dropped`) |
| GC work put back after a failed pass | `src/tx/gc.rs` of commit `55a54a3` (before the fix), with the fail point added before its first read | `tx::tests::gc::a_pass_that_fails_after_taking_its_work_puts_it_back` failed: the next pass found nothing, "Pass { horizon: 5, …, removed: 0, trees: 0, group: None }"; with the fix it removes the entry and frees the tree, and the integrity check passes |

The assertion that an index creation that returned survives the crash is therefore mutation-checked in the fault suite and, since F1, in the kill suite.

### Exit conditions of sprint DECISION 8

| Exit condition | Passing tests (default run and the lab run below) |
|---|---|
| `loom` models: commit latch and publish order, lock table, deadlock detection, index side tree | `storage::wal::tests::loom_published_timestamps_increase_and_never_exceed_the_durable_ones`; `tx::lock::tests::loom_lock_grants_follow_the_queue`; `loom_two_transaction_cycle_fails_exactly_one_request`, `loom_three_transaction_cycle_fails_exactly_one_request`, `loom_cycle_through_a_waiting_request_fails_the_requester`; `index::tests::loom_side_tree_replay_and_online_switch`; also the snapshot registry (`tx::mvcc::tests::loom_no_snapshot_…`, `loom_no_overflow_snapshot_…`) and four more (`raw/loom.log`, 11 passed) |
| Write skew: 25N11 under serializable, both commit under snapshot isolation; a closed wait cycle aborts the requester with 50N05 | `tx::tests::isolation::write_skew_fails_one_transaction_under_serializable_and_none_under_snapshot`; `tx::tests::hermitage::g2_item_write_skew_fails_only_under_serializable` (entries and nodes); `tx::tests::isolation::a_closed_wait_cycle_aborts_the_requester_with_deadlock` |
| 16 threads writing disjoint data all commit; reads never wait for writes | `tx::tests::isolation::sixteen_threads_with_disjoint_writes_all_commit_and_survive_reopen`; `storage::graph::tests::graph::sixteen_threads_creating_disjoint_nodes_and_edges_all_commit_and_survive_reopen`; `tx::commit::tests::sixteen_threads_committing_disjoint_batches_apply_each_exactly_once`; `tx::tests::isolation::reads_never_wait_for_writers_or_the_commit_latch` |
| Write sets and old versions beyond the cap fail only the operation that needs the memory | `tx::tests::mvcc::old_versions_at_the_cap_fail_only_the_writer`; `tx::tests::mvcc::a_transaction_beyond_the_cap_fails_and_changes_nothing`; `index::tests::index::a_commit_whose_index_records_exceed_the_cap_fails_and_applies_nothing`; `index::tests::index::an_indexed_commit_over_the_cap_still_fails_at_both_steps` |
| A constraint creation blocks only writers of its label; an index build blocks no writer | `index::tests::index::a_constraint_creation_blocks_only_the_writers_of_its_label`; `a_constraint_paused_in_its_page_writes_blocks_only_the_writers_of_its_label`; `writers_commit_while_an_index_build_is_paused_in_its_page_writes`; `eight_writers_commit_while_a_build_is_paused_and_the_online_index_matches_the_data` |
| The Sprint 1 crash, backup and restore suites pass with transactional workloads | `suites::fault::{commits, transactions, checkpoint, recovery_checkpoint, backup, restore_, migration, bulk_load}` and `suites::kill::kill_and_reopen` on ext4 and XFS (lines below); `ops::backup::tests::a_backup_under_four_committers_restores_to_one_commit_point`; `index::tests::index::a_backup_under_four_committers_and_an_index_build_restores_to_one_commit_point` (four transactional node committers, an online build, the restored state equals the commits up to one commit point and passes the integrity check with the cross-tree and index checks); Hermitage and Elle on graph operations (above) |
| Laboratory report: traversal device I/O and CPU at 1 %, 10 % and 100 % cache; data for unverified points 3, 4 and 13; close points 6 and 7 | **Deferred to Sprint 6** (timing, sprint UPDATE 54): the traversal I/O and CPU measurements and points 3 (serial commit section), 4 (compressed leaves under concurrent readers) and 13 (M-LAYOUT limits). Point 7 is closed (TS-30 Unverified points). Point 6 (free-page reserve estimate and extent size) is **deferred to Sprint 6** (DECISION 244 on rmp #35): it is derived, not measured, and no test measures it; the reserve is not a correctness bound (TS-05). Point 15 (real peak memory against the cap, and the R13 binary that would measure it) is likewise **deferred to Sprint 6** (DECISION 244). |

### Gates (final code)

| Gate | Command | Result | Log |
|---|---|---|---|
| Full run with both lab directories | `LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs cargo nextest run -p lpg --no-fail-fast --success-output immediate` | exit 0; `373 tests run: 373 passed (3 slow), 24 skipped` (second run on the final code; the first failed, see below) | `raw/full-run-lab.log`, `raw/full-run-lab-2-failed.log` |
| Stress profile, both lab variables set | `LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs cargo nextest run -p lpg --profile stress --no-fail-fast` | exit 0; `63 tests run: 63 passed` (16 copies each) | `raw/stress.log` |
| loom | `RUSTFLAGS="--cfg loom" cargo test -p lpg --lib --release --target-dir target/loom loom_` | exit 0; 11 passed | `raw/loom.log` |
| shuttle | `RUSTFLAGS="--cfg shuttle" cargo test -p lpg --lib --target-dir target/shuttle shuttle_` | exit 0; 4 passed | `raw/shuttle.log` |
| MSRV | `cargo +1.89 build`; `cargo +1.89 build --all-targets` | exit 0; exit 0 | `raw/msrv-build*.log` |
| Licences | `cargo deny check licenses`; `cargo deny check` | `licenses ok`; `advisories ok, bans ok, licenses ok, sources ok` | `raw/deny-*.log` |
| Lints, format, docs | `cargo clippy --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo doc --no-deps` | exit 0, no warning | `raw/clippy.log`, `raw/fmt.log`, `raw/doc.log` |
| `forbid(unsafe_code)` | `grep -n 'forbid(unsafe_code)' src/lib.rs` | `9:#![forbid(unsafe_code)]` | `raw/greps.log` |
| FR-STO-002 grep | `grep -rnE 'std::fs\|OpenOptions\|File::(open\|create)' src --include=*.rs` | Product code matches only `src/storage/fs.rs`. The other matches are test code compiled only under `cfg(test)` (`src/lib.rs:50-55`, `src/ops/restore.rs:270`): `src/ops/restore/tests.rs:633,634,659`, `src/suites/kill.rs:358,362`, `src/test_support/mod.rs:102`, `src/test_support/history.rs:171,207,209`. This meets the amended acceptance criterion (UPDATE 243 on rmp #35): test-harness file access is not the crate's storage I/O. No match in `src/lab/`. | `raw/greps.log` |

The first full lab run failed (`raw/full-run-lab-1-failed.log`): the kill suite's full-run requirement that a child's index build return held on ext4 (3) but not on XFS (0 of 200 iterations). A child's build scans the whole default graph while six threads write, and the kill comes 5 to 150 ms after the first commit. The requirement was replaced by one that does not depend on speed: at least one build interrupted by a kill is finished by the parent (`restarted_index_builds`, 200 on each file system); the returned builds are still printed. The code under test did not change between the two runs.

After the audit fixes, the first full run on the final code failed one test, `tx::tests::mvcc::own_writes_are_private_and_an_abort_leaves_no_trace` (`src/tx/tests/mvcc.rs:148`: accountant 854 016 bytes, expected 854 114) (`raw/full-run-lab-2-failed.log`). The cause was a race in the test support: `pause_gc` only set a flag and did not wait for a garbage-collection pass already started by the test's first commit, which could free a committed write-key set after the test read its baseline. Temporary counters over 600 rounds of 64 parallel copies (38 400 runs, tmpfs) showed all 39 failures had a pass in flight at the pause that freed one write-key set after the baseline; no run without a pass in flight failed. **Fixed:** `gc_paused` is a `Mutex<bool>` (`src/tx/mvcc.rs:93-97`) that the `lpg-gc` thread holds from its flag check to the end of its pass (`src/tx/gc.rs:284-296`), so `pause_gc` (`src/tx/tests/mod.rs:125-134`) returns only after a pass in flight ends; the fault suite's `Transactions` workload now calls `pause_gc` (`src/suites/fault.rs:319`). Same load, 38 400 runs each: 37 and 39 failures before the fix, 0 after, 36 with the old behaviour restored (rmp #35 TEST 248). The gate logs below were all regenerated on this final code.

`/tmp/lpgram`, `/data/tmp/lpg-lab` and `/mnt/lpg-xfs` were empty after the full run and after the stress run (last lines of the logs).

### Suite counts, final run (persistent storage)

The fault suite prints, per workload and file system, the crash points and the runs of each crash mode, the failure runs, and the page-copy failure runs. Every crash-mode count, and every failure count, is above 0. `copy_failure_runs` is 0 by design for the workloads that do not run the page-copy failure sweep (only `Commits` and `Transactions` do).

    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=Checkpoint crash_points=34 runs: drop-all 34, keep-random 34, tear-512 34 failure_runs=32 copy_failure_runs=0
    SUITE fault fs=xfs source=/dev/loop0 workload=Checkpoint crash_points=34 runs: drop-all 34, keep-random 34, tear-512 34 failure_runs=32 copy_failure_runs=0
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=BulkLoad crash_points=108 runs: drop-all 108, keep-random 108, tear-512 108 failure_runs=191 copy_failure_runs=0
    SUITE fault fs=xfs source=/dev/loop0 workload=BulkLoad crash_points=108 runs: drop-all 108, keep-random 108, tear-512 108 failure_runs=191 copy_failure_runs=0
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=Migration crash_points=9 runs: drop-all 9, keep-random 9, tear-512 9 failure_runs=6 copy_failure_runs=0
    SUITE fault fs=xfs source=/dev/loop0 workload=Migration crash_points=9 runs: drop-all 9, keep-random 9, tear-512 9 failure_runs=6 copy_failure_runs=0
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=Backup crash_points=109 runs: drop-all 109, keep-random 109, tear-512 109 failure_runs=164 copy_failure_runs=0
    SUITE fault fs=xfs source=/dev/loop0 workload=Backup crash_points=109 runs: drop-all 109, keep-random 109, tear-512 109 failure_runs=164 copy_failure_runs=0
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=RecoveryCheckpoint crash_points=149 runs: drop-all 149, keep-random 149, tear-512 149 failure_runs=2 copy_failure_runs=0
    SUITE fault fs=xfs source=/dev/loop0 workload=RecoveryCheckpoint crash_points=149 runs: drop-all 149, keep-random 149, tear-512 149 failure_runs=2 copy_failure_runs=0
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=Transactions crash_points=168 runs: drop-all 168, keep-random 168, tear-512 168 failure_runs=308 copy_failure_runs=185
    SUITE fault fs=xfs source=/dev/loop0 workload=Transactions crash_points=168 runs: drop-all 168, keep-random 168, tear-512 168 failure_runs=308 copy_failure_runs=185
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=Commits crash_points=152 runs: drop-all 152, keep-random 152, tear-512 152 failure_runs=241 copy_failure_runs=1381
    SUITE fault fs=xfs source=/dev/loop0 workload=Commits crash_points=152 runs: drop-all 152, keep-random 152, tear-512 152 failure_runs=241 copy_failure_runs=1381
    SUITE kill fs=ext4 source=/dev/nvme1n1p1 dir=/data/tmp/lpg-lab iterations=200 printed_commits=25960 recovery_ms_p50=15.7 recovery_ms_p99=33.2 replayed_bytes_p50=370080 replayed_bytes_p99=781280 child_commits_per_s_p50=1767 kills_in_checkpoint=200 kills_in_backup=198 recycling_checkpoints=29 printed_drops=1314 kills_in_index_build=200 index_builds=3 restarted_index_builds=200 kept_indexes_checked=324
    SUITE kill fs=xfs source=/dev/loop0 dir=/mnt/lpg-xfs iterations=200 printed_commits=6919 recovery_ms_p50=28.1 recovery_ms_p99=51.3 replayed_bytes_p50=304288 replayed_bytes_p99=567456 child_commits_per_s_p50=414 kills_in_checkpoint=200 kills_in_backup=200 recycling_checkpoints=8 printed_drops=540 kills_in_index_build=199 index_builds=0 restarted_index_builds=199 kept_indexes_checked=198
    SUITE fault fs=ext4 source=/dev/nvme1n1p1 workload=Restore crash_points=169 runs: drop-all 169, keep-random 169, tear-512 169 failure_runs=159 copy_failure_runs=0
    SUITE fault fs=xfs source=/dev/loop0 workload=Restore crash_points=169 runs: drop-all 169, keep-random 169, tear-512 169 failure_runs=159 copy_failure_runs=0

The `Transactions` workload had 28 crash points at the Round A gate and 71 before rmp #35; the index builds, property updates and node deletion raise it to 168. `fs=` and `source=` are measured with `findmnt` (audit F7). `index_builds=0` on XFS means no child's build returned before its kill; every interrupted build was finished by the parent and then checked. `kept_indexes_checked` counts the indexes the parent kept ONLINE at a close and found ONLINE after the next kill (audit F1): 324 on ext4 and 198 on XFS, so on XFS the survival of returned index creations is checked through them.

### Coverage of `src/tx`, `src/catalog`, `src/index`

`cargo-llvm-cov` 0.9.1 on the final code, re-run after the audit fixes (F6). Line and region figures are llvm-cov's per-file summaries, test files (`tests.rs`, `tests/`) excluded; they still include the inline `#[cfg(test)] mod tests { … }` blocks of `tx/conflict.rs` and `tx/versions.rs`. "Unreached error branches" counts source lines that hold a code region with an execution count of 0 in every instantiation, where the region starts at a `?` or the line contains `Err(`, `return Err`, `map_err`, `ok_or`, `abort_with` or `Error::`; regions inside inline `#[cfg(test)] mod … { … }` blocks are now excluded (audit F6). The figures come from `cargo llvm-cov report --json`; `raw/coverage-*.txt` lists each file's figures and each counted line as `file:line`, and `raw/coverage-*-run.log` the test runs.

| Run | `src/tx` lines / regions | `src/catalog` | `src/index` | Unreached error branches (excluded as inline test code) |
|---|---|---|---|---|
| Suites only, full on ext4: `LPG_LAB_EXT4=/data/tmp/lpg-lab cargo llvm-cov nextest -p lpg --no-report -E 'test(/^suites::/)'` (14 passed) | 72.1 % / 66.6 % | 70.7 % / 70.2 % | 63.3 % / 62.3 % | 352 (12) |
| Default run on `/tmp/lpgram`: `cargo llvm-cov nextest -p lpg --no-report` (373 passed) | 95.2 % / 94.1 % | 92.1 % / 89.8 % | 92.1 % / 90.7 % | 295 (2) |

In the default run the unreached error branches are in `catalog/mod.rs` (81), `catalog/stats.rs` (21), `index/build.rs` (51), `index/mod.rs` (85), `index/side.rs` (2), `tx/commit.rs` (12), `tx/conflict.rs` (1), `tx/gc.rs` (4) and `tx/mod.rs` (38). Most are `?` propagations of storage errors from reads and writes, malformed-record `Corruption` returns (catalog, statistics and index definitions, side-tree keys), argument checks (`not an index kind`, `not a constraint kind`, `not a point index`), and the `abort_with` paths of identifier exhaustion. The counts differ from the earlier run (293 and 350): the audit fixes added such lines (`catalog::check`, `src/catalog/mod.rs:636, 640, 695`), and the inline test lines are no longer counted. No test reaches them; whether each is reachable without corrupt input was not analysed.

### Specification change

TS-30 (`docs/technical-spec/quality-and-testing.md`): the transaction workload, kill suite, Hermitage and Elle descriptions follow the code; R13 (NOTE 212 on rmp #35) now states that the separate memory-test binary is planned and not built, so unverified point 15 stays open. The thread-heap measurement with `allocation-counter` is the only memory test that exists.

After the exit-gate audit (FINDING 245 on rmp #35), TS-30 also: lists the fail point `Mvcc::gc_fail_read` and corrects the lines of `gc::FAIL_SEED`; adds the catalog reconciliation to the fault oracle and the index oracle to the kill suite (F1); states that the suites measure each directory's file system with `findmnt` (F7); states what the ext4 and XFS runs show and adds unverified point 19, power-loss durability on ext4 and XFS (F2); and labels points 6 and 15 as deferred to Sprint 6 (DECISION 244, F5).
