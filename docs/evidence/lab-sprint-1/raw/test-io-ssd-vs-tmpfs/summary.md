# Test-suite I/O: ext4 NVMe vs tmpfs (`/tmp/lpgram`)

Session 2026-10-07 20:26–20:36. Host: AMD Ryzen 9 5900HX (16 threads), governor `performance`, Linux 7.0.0-38-generic, fio 3.41, cargo 1.99.0, cargo-nextest 0.9.146, source `ae8357e` plus the uncommitted working tree of that time. SSD: `/data` ext4 on `/dev/nvme1n1p1` (Samsung SSD 970 EVO Plus 1TB). tmpfs: `/tmp` (systemd `tmp.mount`, `usrquota`). This measures test-suite speed only. tmpfs figures say nothing about production I/O or durability (CLAUDE.md).

## Device level (fio, 3 interleaved reps x 10 s, psync, iodepth 1, buffered)

O_DIRECT is not supported on tmpfs, so no job uses it. Median [min–max]. Raw: `fio-*.json`, `fio-log.txt`, `fio-summary.txt`; script `fio-device.sh`.

| Job | ext4 NVMe | tmpfs |
|---|---|---|
| a. 8 KiB overwrite + fdatasync, IOPS | 1186 [1163–1193] | 388 401 [387 541–388 447] |
| a. fdatasync p50 / p99 µs | 807 [807–815] / 1057 [1057–1860] | 0.4 / 0.4 |
| a. write() p50 µs | 8.0 [8.0–8.1] | 1.8 [1.8–1.8] |
| b. 8 KiB buffered random read, IOPS | 462 184 [461 932–478 433] | 651 101 [617 948–661 064] |
| b. read p50 / p99 µs | 1.4 / 3.6 [3.5–3.6] | 1.3 [1.3–1.4] / 2.0 [1.9–2.2] |
| c. 8 KiB buffered write, new 512 MiB file, no sync, MiB/s | 2098 [2090–2098] | 3793 [3793–3821] |
| c. write() p50 µs | 3.2 [3.1–3.2] | 1.8 |

NVMe state during job a: `f_await` 0.79–0.81 ms (fast state; 11 637–11 932 flushes per run, one per fdatasync). Job b on ext4 read 32 768 x 8 KiB from the device per run (fio `invalidate=1` drops the file's page cache first), then served from the page cache. Job c caused no device writes during the run.

## Test suite (`cargo nextest run -p lpg`, same binary, interleaved tmpfs/ssd x 3)

SSD configuration: `/tmp/lpgram` was a symlink to `/data/tmp/lpg-lab/lpgram`; verified with `findmnt -T` (ext4, nvme1n1p1) and `strace -e openat`. Binary sha256 identical before and after (`test-binary-sha256-*.txt`); cargo rebuilt nothing (`Finished ... 0.01s`). All runs: 211 passed, 24 skipped, exit 0. Raw: `suite-*.txt`, `suite-log.txt`, `suite-summary.txt`; script `suite-runs.sh`.

| | tmpfs | ext4 NVMe |
|---|---|---|
| wall s | 9.24 [9.22–9.28] | 10.22 [10.08–10.32] |
| user s | 38.27 [38.23–38.77] | 42.49 [41.97–42.93] |
| sys s | 38.66 [38.13–38.98] | 44.24 [43.91–44.88] |
| NVMe writes / flushes per run | 3–65 / 1–2 | 4 661–5 101 / 385–397 (`f_await` 0.96–1.04 ms) |

Wall ratio ext4/tmpfs: 1.106 (medians); per-rep pairs 1.106, 1.086, 1.119; all nine pairs 1.086–1.119. The ranges do not overlap. Within-configuration spread (noise floor): 0.06 s (tmpfs), 0.24 s (ext4).

Wall time is set by the longest test, `storage::btree::tests::a_million_random_puts_and_deletes_match_a_btreemap_model` (8.87 s tmpfs, 9.69 s ext4 in the suite).

## Tests with the largest difference

Suite medians (s), and each test run alone (test binary, `--exact`, 3 interleaved reps; `top-tests-log.txt`, `strace-c-t*.txt`, `strace-cw-t1-*.txt`).

| # | Test | suite tmpfs | suite ext4 | alone tmpfs | alone ext4 |
|---|---|---|---|---|---|
| 1 | storage::fault::tests::crash_keeps_or_loses_an_unsynced_create_and_rename_in_order | 0.043 | 1.248 | 0.012 | 0.389–0.446 |
| 2 | storage::btree::tests::a_million_random_puts_and_deletes_match_a_btreemap_model | 8.874 | 9.694 | 4.94–5.01 | 4.95–5.01 |
| 3 | storage::btree::tests::a_tree_ten_times_larger_than_the_cap_reads_back_intact | 5.917 | 6.735 | 2.73 | 2.66–2.70 |
| 4 | suites::fault::migration | 1.904 | 2.312 | 0.558–0.562 | 0.552–0.561 |
| 5 | tx::commit::tests::an_undone_batch_that_took_an_emptied_trunk_leaves_the_free_list_intact | 1.752 | 2.153 | 0.618–0.627 | 0.584–0.607 |

- Test 1 (established): 128 `fdatasync` reach the device on ext4 (128 NVMe flushes); `strace -c -w` gives 128 fdatasync = 0.370 s wall on ext4 (2.89 ms each) and none measurable on tmpfs. Other calls (519 openat, 218 unlinkat, 210 rename, 128 pwrite64, 128 fsync) are identical in count.
- Tests 2–5: run alone they show no difference beyond noise, and their own I/O calls are identical in count in both configurations (e.g. test 3: 443 471 pread64, 203 355 pwrite64, no sync calls). Their in-suite difference appears only in composition. Cause hypothesised, not established: contention with the device-bound I/O of concurrent tests (ext4 writeback and journal work, waits on syncs).
- Whole suite under `strace -f -c -w` (one run each, `strace-cw-suite-*.txt`): 267 fdatasync and 163–172 fsync, 3 fallocate, ~565 000 pwrite64, ~1 430 000 pread64 in both. fdatasync 3.9 ms vs 0.09 ms mean, fsync 3.4 ms vs 0.09 ms. strace slowed the suite to ~39.6 s in both configurations, so these runs give counts, not timings.

## fallocate behaviour

`test_support::reserve` (src/test_support/mod.rs:146–162) decides by lexical path prefix, `path.starts_with(GRAPH_DIR)`, not by file-system type. With the symlink, paths still begin with `/tmp/lpgram`, so the test storages skip `fallocate` (use `set_len`) in both configurations: identical behaviour. Consequence: in the SSD configuration the test files are sparse, unlike a run with `LPG_LAB_EXT4`. Both configurations made 3 `fallocate` calls in the whole suite; which tests make them was not identified (the `reserve` documentation says `FsStorage` always calls `fallocate`).

## Stress profile (SSD only)

`cargo nextest run -p lpg --profile stress`, `/tmp/lpgram` symlinked to ext4: 19 passed, wall 12.77 s, user 105.51 s, sys 78.85 s; NVMe 21 144 writes (2.76 GB), 2 flushes. Peak space sampled every 2 s: 3090 MiB (true peak may be higher). Not run on tmpfs: it exceeds the 1 GiB limit of `/tmp/lpgram`. One run only, no spread.

## Limitations

1. Three runs per configuration; one session; one NVMe state (fast, `f_await` 0.8–1.0 ms). In the slow state (2.6–3.0 ms, report.md) the ext4 figures would be expected to change; not measured.
2. Load average at run start was 0.4–3.9 (1-minute), raised mostly by the preceding runs (5 s gap); idle `mysqld`, `mongod`, `java`, `dockerd` were running (below 1.5 % CPU each at 20:25).
3. Wall time includes cargo's freshness check and nextest start-up (identical in both).
4. The cause of the in-suite slowdown of tests 2–5 and of the higher user (+4.2 s) and sys (+5.6 s) CPU on ext4 is not established.
5. fio job b on ext4 includes a cold read of the 256 MiB file at start (fio `invalidate=1`).
