---
title: Sprint 1 laboratory setup
status: frozen record
last-reviewed: 2026-10-02
related:
  - ../README.md
---

# Sprint 1 laboratory setup

Record of the laboratory verified on 2026-10-02 before any Sprint 1 kernel code was written. Raw outputs are in [raw/](raw/). `perf.data` files were kept outside the repository.

| Variable | Path | Filesystem | Backing device |
|---|---|---|---|
| `LPG_LAB_EXT4` | `/data/tmp/lpg-lab` | ext4 (`rw,nosuid,nodev,relatime`) | `/dev/nvme0n1p1`, Samsung SSD 970 EVO Plus 1TB |
| `LPG_LAB_XFS` | `/mnt/lpg-xfs` | XFS on `/dev/loop80` | 8 GiB image `/data/tmp/lpg-xfs.img` on the ext4 `/data` filesystem, loop direct I/O on |

## Environment

| Item | Value | Source |
|---|---|---|
| CPU | AMD Ryzen 9 5900HX, 1 socket, 8 cores, 16 threads, 1 NUMA node; L1d 256 KiB, L2 4 MiB, L3 16 MiB; max 4890 MHz | raw/lscpu.txt |
| Memory | MemTotal 32263884 kB; SwapTotal 2097148 kB | raw/meminfo.txt |
| Kernel | 6.8.0-146-generic (Ubuntu 24.04.5 LTS) | raw/uname.txt, raw/os-release.txt |
| CPU frequency | driver `amd-pstate-epp`, governor `powersave` on all 16 CPUs, EPP `balance_performance` | raw/cpu-governor.txt |
| Storage | `nvme0n1`: Samsung SSD 970 EVO Plus 1TB (from `/sys/block/nvme0n1/device/model`), firmware 2B2QEXM7, 512-byte sectors, scheduler `none`; `/data` is ext4 on `nvme0n1p1` | raw/lsblk.txt, raw/nvme0n1-sysfs.txt |
| XFS loop device | `/dev/loop80`, backing file `/data/tmp/lpg-xfs.img`, `DIO=1` (`/sys/block/loop80/loop/dio` = 1); XFS `crc=1`, `rmapbt=1` | raw/mounts.txt, raw/xfs_info.txt |
| perf access | `kernel.perf_event_paranoid` = 1, `kernel.kptr_restrict` = 0, lockdown `none` | raw/perf-kernel-samples.txt |

Load average was 0.40 before the fio runs. The rise to about 2.3 during the runs is fio itself (blocked I/O counts towards the Linux load average); see raw/fio-run-log.txt.

## Tools

All commands ran as the unprivileged user (uid 1000). Full outputs: raw/tools-versions.txt. `rustup check` was re-run at 17:20 with the same result.

| Tool | Version | Command |
|---|---|---|
| rustup | 1.29.1 (up to date) | `rustup --version`, `rustup check` |
| Rust stable | 1.99.0 (b940084d7 2026-09-28), up to date | `rustc +stable --version`, `rustup check` |
| Rust nightly | 1.101.0-nightly (c36f14571 2026-10-01), up to date | `rustc +nightly --version`, `rustup check` |
| Rust 1.89 | 1.89.0 (29483883e 2025-08-04) | `rustc +1.89 --version`, `rustup toolchain list` |
| llvm-tools (stable) | `llvm-tools-x86_64-unknown-linux-gnu` | `rustup component list --installed` |
| cargo-nextest | 0.9.146 (crates.io latest) | `cargo nextest --version`, `cargo search cargo-nextest` |
| cargo-llvm-cov | 0.9.1 (crates.io latest) | `cargo llvm-cov --version`, `cargo search cargo-llvm-cov` |
| cargo-deny | 0.20.2 (crates.io latest) | `cargo deny --version`, `cargo search cargo-deny` |
| inferno | 0.12.8 (crates.io latest) | `cargo install --list` (binaries have no `--version` flag), `cargo search inferno` |
| perf | 6.8.12 | `perf --version` |
| heaptrack | 1.5.0 (distribution package) | `heaptrack --version` |
| fio | 3.36 (distribution package) | `fio --version` |
| mkfs.xfs | 6.6.0 (distribution package) | `mkfs.xfs -V` |
| fincore | util-linux 2.39.3 (distribution package) | `fincore --version` |
| strace | 6.8 (distribution package) | `strace -V` |
| iostat | sysstat 12.6.1 (distribution package) | `iostat -V` |
| pidstat | sysstat 12.6.1 (distribution package) | `pidstat -V` |

Checks:

- `perf stat -e cycles,instructions,cache-misses,branch-misses -- true` succeeds without sudo (raw/perf-stat-true.txt).
- `perf record -g` of the fio write+fdatasync job works without sudo and names every kernel frame: 0 of 20830 (ext4) and 0 of 15556 (XFS) kernel callchain entries are unresolved. Kernel stacks below `fdatasync` include, for ext4, `__x64_sys_fdatasync`, `ext4_sync_file`, `ext4_writepages`, `ext4_fc_commit`, `jbd2_complete_transaction`, `jbd2_log_wait_commit`, `blk_mq_submit_bio`, `nvme_queue_rqs` and `nvme_prep_rq`; for XFS, `xfs_file_fsync`, `xfs_log_force_seq`, `xlog_force_lsn`, `xlog_cil_push_now`, `xlog_sync`, `xlog_write_iclog`, `iomap_writepages`, `blk_mq_submit_bio` and `loop_queue_rq`. Folded stacks: raw/perf-fdatasync-ext4.folded, raw/perf-fdatasync-xfs.folded. Flame graphs: raw/flamegraph-fdatasync-ext4.svg, raw/flamegraph-fdatasync-xfs.svg. Summary: raw/perf-kernel-samples.txt.
- `stat -f -c %T /mnt/lpg-xfs` prints `xfs`. The user created a file there, `fdatasync` returned 0, and the file was removed (raw/xfs-access-check.txt, re-run with loop direct I/O on).

## Device ceiling

fio 3.36, one job, `ioengine=psync`, `iodepth=1`, 30 s time-based runs, three repetitions interleaved (ext4 read, XFS read, ext4 write, XFS write, repeated), 17:23 to 17:29. Values are the median of three runs, with the range in brackets. Raw JSON: `raw/fio-<job>-<fs>-rep<N>.json`. Device counters are fields 1 (read I/Os), 5 (write I/Os) and 16 (flush I/Os) of `/sys/block/<dev>/stat`, per the kernel's `Documentation/block/stat.rst` (v6.8).

- Random read: `--rw=randread --bs=8k --direct=1 --size=4G --randrepeat=0 --norandommap` on a 4 GiB file written beforehand with `direct=1`. Latency is fio `clat`.
- Write with fdatasync: `--rw=write --bs=8k --direct=0 --fdatasync=1 --size=1G` on a new file for each run (fio preallocates it). Write latency is fio `clat` of `write()`; fdatasync latency is fio `sync` latency (`jobs[0].sync.lat_ns`).

### LPG_LAB_EXT4 (`/data/tmp/lpg-lab`)

| Metric | Value |
|---|---|
| 8 KiB random read IOPS | 12943 [12903–13022] |
| Random read p50 / p99 (µs) | 67.1 [67.1–67.1] / 103.9 [103.9–103.9] |
| Write+fdatasync IOPS | 156.1 [151.6–156.2] |
| `write()` p50 / p99 (µs) | 29.8 [29.6–41.7] / 93.7 [63.7–126.5] |
| `fdatasync` p50 / p99 (µs) | 6324 [6259–6455] / 9503 [9503–9503] |
| `fdatasync` mean (µs) | 6357 [6347–6522] |

Per run, `nvme0n1` completed 387095 to 390659 reads, equal to fio `total_ios`, and one flush per `fdatasync` (4557 to 4700 flushes for 4548 to 4687 calls).

### LPG_LAB_XFS (`/mnt/lpg-xfs`)

| Metric | Value |
|---|---|
| 8 KiB random read IOPS | 9413 [9407–9452] |
| Random read p50 / p99 (µs) | 101.9 [101.9–101.9] / 134.1 [132.1–136.2] |
| Write+fdatasync IOPS | 77.1 [76.9–77.6] |
| `write()` p50 / p99 (µs) | 25.2 [25.2–53.5] / 58.6 [50.9–111.1] |
| `fdatasync` p50 / p99 (µs) | 12780 [12648–12911] / 17170 [16056–17433] |
| `fdatasync` mean (µs) | 12915 [12841–12929] |

Reads reach the device: per run, `nvme0n1` completed 282308 to 283623 reads for 282232 to 283559 `loop80` reads, equal to fio `total_ios`; `fincore` showed 62.3 MiB of the image resident after layout. Each XFS `fdatasync` caused two flushes on `loop80` and two on `nvme0n1` (4616 to 4658 flushes for 2307 to 2329 calls), against one for ext4. This accounts for the roughly doubled `fdatasync` latency.

### Limitations

1. The XFS directory still passes through ext4. `/mnt/lpg-xfs` is a loop device whose backing file is on the ext4 `/data` filesystem, so each XFS flush becomes an ext4 `fsync` of the image and a device flush. XFS `fdatasync` figures measure that stack, not XFS on raw NVMe. Use them for XFS functional testing and relative comparisons only.
2. `fdatasync` latency on `nvme0n1` is not stable over minutes. The ext4 figures above differ from an earlier session the same afternoon (17:06 to 17:11: `fdatasync` p50 2933 µs, 326.9 IOPS; random read 9939 IOPS; its raw JSON files were replaced by this run and are not kept). The 10 s perf runs at 17:21 saw 3326 (ext4) and 3249 (XFS) device flushes, about twice the rate of the fio runs from 17:23. Two 10 s ext4 spot checks after the runs gave `fdatasync` p50 6652 µs and 3097 µs (raw/fio-run-log.txt). The cause is not established; the drive's Composite temperature was 61 to 67 °C with no alarm. Compare durability figures only within interleaved runs of one session.
3. perf records only the profiled process. Device flushes that `jbd2` or the loop worker issue in their own threads (for example `blkdev_issue_flush`) do not appear in the fio stacks. A system-wide `perf record -a` fails without sudo because `perf_event_paranoid` is 1 (raw/perf-kernel-samples.txt).
4. `kernel.kptr_restrict = 0` is not persistent. `/etc/sysctl.d/99-lpg-perf.conf` sets only `kernel.perf_event_paranoid = 1`, and `/etc/sysctl.d/10-kernel-hardening.conf` sets `kernel.kptr_restrict = 1`, so kernel frames will be unresolved again after a reboot.
5. The CPU ran under the `powersave` governor (EPP `balance_performance`); frequency was not pinned.
6. Each figure comes from three 30 s runs in one session; longer-term variation was not measured.

## Reproduction

    # Layout, per directory D:
    fio --name=layout --filename=$D/fio-randread.dat --size=4G --bs=1M --rw=write --create_only=1
    fio --name=layout --filename=$D/fio-randread.dat --size=4G --bs=1M --rw=write --direct=1 --ioengine=psync
    # rep 1..3, job randread|fsyncwrite, fs ext4|xfs, interleaved; before and after each run,
    # record fields 1, 5 and 16 of /sys/block/nvme0n1/stat and /sys/block/loop80/stat:
    fio --name=randread-8k --filename=$D/fio-randread.dat --size=4G --bs=8k --rw=randread \
        --direct=1 --ioengine=psync --iodepth=1 --numjobs=1 --time_based --runtime=30 \
        --randrepeat=0 --norandommap --output-format=json --output=raw/fio-randread-<fs>-rep<N>.json
    rm -f $D/fio-fsyncwrite.dat
    fio --name=seqwrite-8k-fdatasync --filename=$D/fio-fsyncwrite.dat --size=1G --bs=8k --rw=write \
        --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --fdatasync=1 --time_based --runtime=30 \
        --output-format=json --output=raw/fio-fsyncwrite-<fs>-rep<N>.json
    # Profile (perf.data outside the repository):
    perf record -g -o <scratch>/<fs>.perf.data -- fio --name=seqwrite-8k-fdatasync \
        --filename=$D/fio-perf.dat --size=1G --bs=8k --rw=write --direct=0 --ioengine=psync \
        --iodepth=1 --fdatasync=1 --time_based --runtime=10 --minimal
    perf script -i <scratch>/<fs>.perf.data | inferno-collapse-perf > raw/perf-fdatasync-<fs>.folded
    inferno-flamegraph raw/perf-fdatasync-<fs>.folded > raw/flamegraph-fdatasync-<fs>.svg

All test files were removed from both lab directories after the runs.

## #9

Crate `lpg` created: `Cargo.toml` (edition 2024, `rust-version = "1.89"`, licence MIT, `exclude = ["docs/evidence/"]`, profile `lab` = `release` + `debug = true`), `deny.toml`, `src/lib.rs` (`#![forbid(unsafe_code)]`, Linux-only statement; modules `storage`, `tx`, `ops`, `obs`). Build and lint outputs: raw/t9-t10-t11-quality-gate.txt.

Dependencies, checked with `cargo search` and `cargo info` on 2026-10-02: the latest releases are the versions TS-31 names, so no version differs. Runtime: `rustix` 1.1.5 (feature `fs`), `crc32c` 0.6.8, `rand` 0.10.3 (default features). Dev: `tempfile` 3.27.0.

| Check | Result |
|---|---|
| `cargo +1.89 build`, `cargo +stable build` | exit 0 and exit 0 |
| `cargo deny check licenses` | `licenses ok`, exit 0 (cargo-deny gathered 21 crates, dev-dependencies included) |
| `cargo tree -e normal --depth 1` | `crc32c`, `rand`, `rustix` (raw/t9-cargo-tree.txt) |
| Unique crates in `cargo tree -e normal` | 11 besides `lpg`: bitflags, cfg-if, chacha20, cpufeatures, crc32c, getrandom, libc, linux-raw-sys, rand, rand_core, rustix |
| Clean build, `cargo clean; cargo build --profile lab --timings`, 3 runs | 1.74 s [1.74, 1.75] wall time (cargo `Finished` line), 22 units (raw/t9-clean-build-timings.txt, raw/t9-cargo-timing-lab.html) |

`deny.toml` allows `Apache-2.0`, `Apache-2.0 WITH LLVM-exception` and `MIT`: the licences of the resolved tree that are on the NFR-DEP-002 list. It has no `clarify` and no exceptions. `r-efi` (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`, a target-specific dependency of `getrandom`) passes through its MIT alternative. TS-31 records the same allow-list as NFR-DEP-002.

**Unverified point 11** (cargo-deny 0.20.2; raw/t9-point11-cargo-deny.txt):

- `crc32c` 0.6.8 (`Apache-2.0/MIT`): cargo-deny reads it as two alternatives, Apache-2.0 and MIT. `cargo deny check licenses` on `lpg` passes with the allow-list only.
- `csv` 1.4.0 (`Unlicense/MIT`): checked in a throwaway manifest outside the repository that depends only on `csv = "=1.4.0"`, with the full NFR-DEP-002 list and nothing else. cargo-deny reads `csv`, `csv-core` and `memchr` as Unlicense or MIT, and `ryu` as Apache-2.0 or BSL-1.0. The check passes, exit 0.

Both pass without configuration beyond the allow-list; no entry was added. Point 11 is resolved.

## #10

`src/storage/mod.rs` defines the dyn-compatible traits `Storage`, `StorageFile` and `StorageLock` (TS-03). `src/storage/fs.rs` holds `FsStorage`; `src/storage/fault.rs` (`#[cfg(test)]`) holds `FaultStorage`, which wraps any `Box<dyn Storage>`. `File::try_lock` compiles and returns `WouldBlock` for a second descriptor on Rust 1.89.0 without a feature gate (probe built with `rustc +1.89`).

Tests: `cargo nextest run -p lpg storage::fs storage::fault` with `LPG_LAB_EXT4` and `LPG_LAB_XFS` set: 13 passed; each fault test runs in both directories (raw/t10-t11-nextest.txt). `same_contract_through_both_impls` drives `FsStorage` and `FaultStorage` as `Box<dyn Storage>`. Fault modes covered: dropped, partly kept (seeded) and torn (512-byte sectors) unsynced writes; unsynced create, rename and remove lost in a crash; a failed `sync_data`; a failed `write_at`; ENOSPC from `write_at`, `reserve` and `create` with `available_space` reporting the budget; the call log in order. Three mutations of `fault.rs` (no namespace undo; no tearing; operations dropped at `sync_data`) each made at least one test fail; the original then passed again.

`grep -rnE 'std::fs|OpenOptions|File::(open|create)' src --include=*.rs` matches only `src/storage/fs.rs` (7 lines).

**System calls.** `strace -c -f` of `storage::fs::tests::lab_fixed_syscall_run` (profile `lab`, `LPG_TEST_DIR=/data/tmp/lpg-lab`): 10 `fallocate`, 10 000 `pwrite64`, 100 `fdatasync` and 10 002 `pread64` (raw/t10-strace-c-fixed-run.txt). The two extra `pread64` calls are the dynamic loader reading the program headers of `libc.so.6` at process start; exactly 10 000 `pread64` calls read 8 192 bytes (raw/t10-strace-pread64-origin.txt). Each `read_at`, `write_at`, `sync_data` and `reserve` therefore costs one system call.

**Cached 8 KiB `read_at`.** `perf stat -r 3 -e cycles,instructions,cycles:u,instructions:u` of `storage::fs::tests::lab_cached_read_at`, 10^6 reads at random offsets of a 64 MiB file resident in the OS cache, in `/data/tmp/lpg-lab`. Per read = (run − setup-only run of the same round) / 10^6; three interleaved rounds, median [min, max] (raw/t10-perf-stat-read-at.txt):

| Path | cycles | instructions | user cycles | user instructions |
|---|---|---|---|---|
| `Box<dyn StorageFile>::read_at` (`FsStorage`) | 6 117 [6 064, 6 162] | 2 560 [2 559, 2 561] | 219 [216, 220] | 136 [136, 136] |
| `FileExt::read_at` on a `File` | 6 043 [5 946, 6 065] | 2 516 [2 515, 2 516] | 157 [157, 159] | 92 [92, 92] |

The trait path adds 44 user-space instructions and about 60 user cycles per read: the dynamic call and the short-read loop of `FsFile::read_at`. Total cycles differ by 1.2 %, within the per-run variance perf reported (up to 1.48 %), so no total-cycle difference is established. Kernel cycles (copying 8 KiB from the page cache) are 96–97 % of the cycles of either path.

## #11

`src/obs/mod.rs` defines `MetricsSink` (`Send + Sync`, dyn-compatible; `counter`, `gauge` and `histogram` with `&'static str` names and `u64` values) and the name constants in `obs::names`, one module per FR-OBS-001 category the kernel produces: `transactions`, `transaction_log`, `checkpointing`, `cache`, `store_size`. The names reuse Neo4j's metric names (Neo4j Operations Manual, "Metrics reference", read on 2026-10-02); kind and unit are stated on each constant. `src/obs/recording.rs` (`#[cfg(test)]`) holds `RecordingSink` and the `Metrics` snapshot with `since` (difference). `cargo nextest run -p lpg obs::`: 4 passed; `every_call_kind_through_dyn_sink` drives all three calls through `&dyn MetricsSink`.

`src/lab/` (`#[cfg(test)]`): harness (`mod.rs`), generators derived from M-LAYOUT `generator.rs` (`generator.rs`: Zipf α = 0.8 and uniform samplers over a seeded node permutation, adjacency keys `(u64, u8, u32, u64, u64)`, `memory_cap` for 100 %, 10 % and 1 % of the data pages, a parameter only until the pool exists), `lab::storage` and `lab::obs`. Each round is timed as one batch with one `Instant` pair; the clock source is still `hpet` (`/sys/devices/system/clocksource/clocksource0/current_clocksource`). `/proc` reads, table writes and `fadvise` use `rustix` directly, because they are not database files.

**lab::storage** ran 17:50:47–17:51:53 inside one session, between two sets of the task #7 fio jobs (30 s each, before 17:48:46–17:50:47 and after 17:51:53–17:53:55; script raw/t11-session.sh, log raw/t11-session-log.txt, fio JSON raw/t11-fio-*.json). Tables: raw/lab-storage-ext4.md, raw/lab-storage-xfs.md. Data file 1 GiB, 3 rounds; ns/op median [min, max]. The fio column gives the mean time per operation, 10^9 / IOPS, before and after.

| Directory | Workload | lab::storage ns/op | KiB read/op | fio ns/op (before, after) | Ratio lab / fio |
|---|---|---|---|---|---|
| ext4 | `read_at` warm (OS cache) | 1 411 [1 286, 1 423] | 0.00 | none (fio read with `direct=1`) | — |
| ext4 | `read_at` cold (evicted, distinct pages) | 95 298 [94 713, 95 432] | 8.11 | 73 888, 79 343 | 1.20–1.29 |
| ext4 | `write_at` + `sync_data` | 5 733 534 [4 306 828, 6 694 410] | 0.00 (8.00 written) | 6 856 490, 5 649 219 | 0.84–1.01 |
| XFS | `read_at` warm (OS cache) | 1 253 [1 248, 1 294] | 0.00 | none | — |
| XFS | `read_at` cold | 106 823 [106 770, 106 830] | 8.14 | 110 540, 111 571 | 0.96–0.97 |
| XFS | `write_at` + `sync_data` | 10 615 366 [9 794 341, 13 217 804] | 0.00 (8.00 written) | 13 628 520, 13 643 929 | 0.78 |

VmRSS at the end of the runs: 5 884 KiB (ext4) and 6 460 KiB (XFS); VmHWM 5 884 and 6 460 KiB. `FsStorage` emits no metrics, so the sink difference column is empty (`-`).

- **Cold reads.** `read_bytes` per operation is 8.11 KiB (ext4) and 8.14 KiB (XFS), against 8 KiB expected: `POSIX_FADV_DONTNEED` evicted the file and each read missed. The extra 0.11–0.14 KiB per read is not attributed. On ext4 a buffered cold read takes 1.20–1.29× fio's `O_DIRECT` read; on XFS it takes 0.96–0.97×. lab::storage reads through the OS cache and fio did not, so these ratios compare two different paths; the cause of the ext4 gap is not established.
- **Write + `sync_data`.** On ext4 the lab range [4.31, 6.69] ms overlaps fio's before/after values [5.65, 6.86] ms, which differ by 21 % within 3 minutes: no difference from the ceiling is established (Limitation 2 of task #7). On XFS lab::storage took 0.78× fio's time per operation in every round, while fio was stable (13.63 and 13.64 ms). The cause is not established; one known difference is that lab::storage syncs its reserved file and directory before timing, and fio does not.
- **Warm reads** have no device ceiling; 1.25–1.41 µs per 8 KiB read is the system call and page-cache copy cost (see #10).

**`counter` on `RecordingSink`.** `perf stat -r 3 -e task-clock,cycles,instructions` of `lab::obs::counter_calls`, 10^7 calls through `&dyn MetricsSink` minus a run with 0 calls, three interleaved rounds (raw/t11-perf-stat-counter.txt): 7.75 ns [7.73, 7.77], 34.3 cycles [34.33, 34.35] and 136.1 instructions [136.09, 136.10] per call, single-threaded and uncontended (one mutex lock and one map update per call).

**Limitations of #9–#11.**

1. CPU governor `powersave` (EPP `balance_performance`), frequency not pinned, as in task #7.
2. The task #7 limitation on `fdatasync` variance applies: the ext4 fio write job changed by 21 % between the two sets of one session.
3. The `perf stat` files of #10 were written under the user's locale, which uses a comma as decimal separator; the counter values are integers and are unaffected, but the variance column reads `0,29%` as two fields. The #11 file was written with `LC_ALL=C`.
4. Each figure comes from one session with 3 rounds; longer-term variation was not measured.

Reproduction:

    cargo test -p lpg --profile lab --lib --no-run     # prints the test binary, below BIN
    LPG_TEST_DIR=/data/tmp/lpg-lab strace -c -f -o raw/t10-strace-c-fixed-run.txt \
        BIN --exact storage::fs::tests::lab_fixed_syscall_run --ignored --test-threads=1
    LPG_TEST_DIR=/data/tmp/lpg-lab LPG_PERF_MODE=<setup|dyn|direct> LPG_PERF_N=1000000 \
        perf stat -r 3 -x, -e cycles,instructions,cycles:u,instructions:u \
        BIN --exact storage::fs::tests::lab_cached_read_at --ignored --test-threads=1
    raw/t11-session.sh                                 # fio, lab::storage, fio
    LC_ALL=C LPG_PERF_N=<0|10000000> perf stat -r 3 -x, -e task-clock,cycles,instructions \
        BIN --exact lab::obs::counter_calls --ignored --test-threads=1

## #12

`src/storage/control.rs` (two 4 KiB `CONTROL` slots with CRC-32C), `src/storage/page.rs` (8 KiB pages; header with CRC-32C, page LSN, tree identifier and page type), `src/storage/datafile.rs` (header page and extent growth), `src/storage/engine.rs` (create and open), `src/storage/error.rs` (error type and the unusable latch) and `src/test_support/child.rs` (re-runs one test in a child process). Gates and suite: raw/t12-t13-quality-gate.txt, raw/t12-t13-nextest.txt (71 passed, both lab directories); coverage: raw/t12-t13-coverage.txt.

**Acceptance.** `cargo nextest run -p lpg storage::control storage::page storage::datafile storage::engine storage::pool storage::memory`: 44 passed. Every engine test runs in `LPG_LAB_EXT4` and `LPG_LAB_XFS`.

| Check | Test | Result |
|---|---|---|
| Create over an existing database fails; every file unchanged | `create_over_an_existing_database_fails_and_changes_nothing` | pass; method: every file read in full through `FsStorage` before and after and compared byte for byte, a stronger check than a digest |
| Open on an empty or missing directory fails and creates nothing | `open_without_a_database_fails_and_creates_nothing` | pass |
| Second open in the same process fails; the first handle keeps working | `second_open_in_the_same_process_fails_and_the_first_keeps_working` | pass |
| Open from a child process fails while held, succeeds after close | `second_open_from_a_child_process_fails` | pass |
| Both `CONTROL` slots invalid: corruption | `both_control_slots_invalid_is_corruption` | pass |
| One invalid slot: opens from the other, counted, repaired by the next write | `one_invalid_control_slot_opens_from_the_other_and_is_counted`, `control::one_invalid_slot_…` | pass |
| Page-0 version ≠ `CONTROL`: corruption | `page_zero_version_differing_from_control_is_corruption` | pass |
| Newer version fails, all files unchanged | `newer_format_fails_and_changes_nothing` | pass |
| Older version: `migration required` | `older_format_requires_migration` | pass |
| `ensure` under a full `FaultStorage` returns ENOSPC; a read still succeeds | `enospc_on_ensure_keeps_reads_working` | pass |
| `memory_cap` on create and open | `grep -n memory_cap src/storage/engine.rs` | lines 98 and 172 (raw/t12-t13-quality-gate.txt) |

**System calls.** `strace -c -f` of `lab::files::files_one` with `LPG_FILES_MODE` `none`, `create` and `create_open`; one create is `create − none`, one open is `create_open − create` (raw/t12-strace-c-*.txt). ext4 and XFS give the same counts.

| Call | `fdatasync` | `fsync` | `fallocate` | `pwrite64` | `pread64` | `flock` |
|---|---|---|---|---|---|---|
| create | 2 (`data-1`, `CONTROL`) | 2 (`sync_dir` after `data-1`, after `CONTROL`) | 1 (first 64 MiB extent) | 2 (page 0, both `CONTROL` slots) | 0 | 1 |
| open | 0 | 0 | 0 | 0 | 2 (`CONTROL`, page 0) | 1 |

**Wall time.** `lab::files::files`, 100 creates and 100 opens per directory, each call timed alone, memory cap 64 MiB (raw/t12-lab-files.md):

| Directory | create µs, median [min, max] | create p90 | open µs, median [min, max] | open p90 |
|---|---|---|---|---|
| ext4 | 5 947 [5 305, 13 559] | 6 503 | 85 [82, 253] | 104 |
| XFS | 11 806 [11 352, 28 185] | 12 864 | 83 [76, 150] | 115 |

Create performs the four syncs above; open performs none. How the create time divides among the syncs was not measured.

**Extent growth (first data for unverified point 6).** `strace -f -T -e trace=fallocate` of `lab::files::files_growth`, growth from the first extent to 4 GiB, one extent per `ensure`, three runs per directory (raw/t12-strace-fallocate-*.txt, summary raw/t12-fallocate-summary.txt):

| Directory | `fallocate` of 64 MiB, median per run (µs) | max (µs) | total for 4 GiB (ms) |
|---|---|---|---|
| ext4 | 37, 32, 24 | 71, 56, 41 | 2.47, 2.12, 1.61 |
| XFS | 39, 40, 37 | 54, 3 083, 3 137 | 2.64, 5.72, 5.77 |

Up to 6.25 GiB an extent is 64 MiB, so growth takes 16 extents per GiB; above it an extent is 1 % of the file, so a file of S GiB takes 100 / S extents per GiB. The measured runs took 64 extents for 4 GiB. Two of 192 XFS calls took about 3.1 ms; their cause was not investigated. These figures cover the extent size only; the free-page reserve bound of point 6 needs commits (#16) and stays unverified. `fallocate` changes the file size, which becomes durable only with the next `sync_data` of the file.

## #13

`src/storage/pool.rs` (buffer pool), `src/storage/memory.rs` (memory accountant). Laboratory: `src/lab/pool.rs`, run on `LPG_LAB_EXT4` with a 1 GiB database of 131 072 pages (`$LPG_LAB_EXT4/pool-db`, removed afterwards), OS cache warm unless stated. One operation is `fetch`, shared latch, one byte read and unpin. Cache share = pool page buffers as a percentage of the data pages; the cap is sized so (`cap_for`).

**Acceptance.** In the 44 tests above:

| Check | Test | Result |
|---|---|---|
| 16 threads fetching and reserving never exceed the cap, checked on every reservation | `memory::concurrent_reservations_never_exceed_the_cap`, `pool::sixteen_threads_fetching_and_reserving_stay_within_the_cap` | pass; the compare-and-swap in `try_charge` checks the cap on every reservation, each thread asserts it after each one, and the recorded peak is at most the cap |
| Pages worth 10× the cap are returned intact | `pages_worth_ten_times_the_cap_are_returned_intact` | pass; 2 560 pages, cap 2 MiB, three passes, every byte compared |
| A flipped bit fails its read; every later operation fails until reopen | `flipped_bit_fails_the_read_and_every_later_operation_until_reopen` | pass; after reopen the good page reads again and the bad one fails again |
| A frame with page LSN > durable LSN is never written | `frame_with_lsn_above_durable_is_never_written` | pass; `FaultStorage` call log shows no `write_at` at that offset under three passes of eviction pressure, then exactly one once the durable LSN reaches it |

The suite also covers: all frames pinned gives a memory-limit error and leaves the database usable; a reservation shrinks the pool to its floor and no further; reservations take clean frames before dirty ones. Fifty debug and thirty release repetitions of the pool tests passed.

**Latency per operation.** `lab::pool::pool`, 3 rounds, median [min, max] (raw/t13-lab-pool.md):

| Cache | miss (ns) | hit (ns) | uniform random (ns, hit rate) | Zipf 0.8 random (ns, hit rate) |
|---|---|---|---|---|
| 100 % | 12 084 [11 818, 12 122] (pool growing) | 50 [50, 63] | 162 [158, 166], 1.000 | 217 [211, 247], 1.000 |
| 10 % | 3 063 [3 029, 3 109] (with eviction) | 31 [31, 31] | 3 084 [3 038, 3 118], 0.100 | 1 994 [1 977, 2 035], 0.477 |
| 1 % | 2 683 [2 665, 2 730] (with eviction) | 22 [22, 22] | 2 921 [2 915, 2 948], 0.010 | 2 524 [2 493, 2 550], 0.213 |

Misses are sequential first touches, one `pread64` each. At 100 % no frame is evicted and each miss also allocates and first-touches a new 8 KiB buffer; a run of the same workload before the final page-table change measured 8 056 [7 958, 8 169] ns, and the cause of the difference was not established. Hits are random within a resident set of a quarter of the pool; the misses in the "hit" rows of the raw table are the loading of that set before the timed rounds. A hit costs more at 100 % than at 1 % because the frames touched span 1 GiB instead of 10 MiB.

**CPU counters.** `perf stat` of `lab::pool::pool_fixed`, per operation = (run with N operations − run with 0) / N, 3 interleaved rounds, median [min, max] (raw/t13-perf-stat.txt, raw/t13-perf-stat-per-op.txt, script raw/t13-perf-stat.sh). The Zen 3 core PMU has no `LLC-load-misses`; the L3-miss column counts `ls_any_fills_from_sys.mem_io_local`, data-cache fills from local DRAM. The per-operation figures include drawing the page number.

| Cache | Mode | Cycles | Instructions | IPC | Branch misses | DRAM fills |
|---|---|---|---|---|---|---|
| 100 % | hit | 347 [176, 384] | 303 [153, 318] | 0.87 | 1.0 | 0.3 |
| 100 % | miss | 37 832 [37 056, 38 498] | 34 514 | 0.91 | 469 | 97 |
| 10 % | hit | 138 [135, 162] | 238 | 1.72 | 0.1 | 0.1 |
| 10 % | miss | 13 674 [13 408, 16 045] | 16 680 | 1.22 | 67 | 86 |
| 1 % | hit | 119 [119, 119] | 260 | 2.19 | 0.3 | 0.0 |
| 1 % | miss | 12 528 [12 496, 12 607] | 16 439 | 1.31 | 65 | 104 |

**Reads equal misses.** `strace -c -f` of `pool_fixed`, 30 000 operations per mode (raw/t13-strace-c-*.txt, counters raw/t13-strace-c-counters.txt): in all six runs `pread64` = miss counter + 4 (32 772 = 32 768 + 4; 95 540; 3 281; 43 112; 331; 31 315). The 4 are the dynamic loader's 2 reads (#10) and the open's reads of `CONTROL` and page 0 (#12 table).

**Allocations.** heaptrack of `pool_fixed`, run with N minus run with 0 (raw/t13-heaptrack-summary.txt):

| Cache | Per hit (N = 10^6) | Per miss (N = 30 000) |
|---|---|---|
| 100 % | 0 (1 call in 10^6: the harness's pick vector) | 1.0 (30 000 calls: the new page buffer; the pool is growing) |
| 10 % | 0 (2 calls in 10^6, harness) | 0 (0 calls) |
| 1 % | 0 (2 calls in 10^6, harness) | 0.00003 (1 call) |

A first run sized each page-table shard for its mean occupancy and made 80 allocations in 30 000 misses at 10 %. With twice the mean occupancy it made none (same raw file, before and after); the change costs 0.26 % of the cap at 10 % and is kept.

**Accounted bytes against VmRSS.** VmRSS growth from before the open to the end of `pool_fixed`, one process per run (raw/t13-strace-c-counters.txt): with the pool full, growth / accounted = 1.0022 (100 %, 95 536 buffers), 1.0046 (10 %, 13 108) and 1.0342 (1 %, 1 311). The remaining bytes are not accounted: harness allocations, sink state and allocator overhead (not attributed further).

### Unverified point 5

**Pool floor.** `lab::pool::pool_floor`: cap sized for a pool of 10 % of the data; after filling the pool, a competing user reserves 1 MiB, then 8 KiB, chunks until refused (raw/t13-lab-pool-floor.md, raw/t13-lab-pool-floor-hits.md).

| Case | Page buffers | Uniform ns/fetch | Uniform hit rate | Zipf ns/fetch | Zipf hit rate |
|---|---|---|---|---|---|
| floor 1 %, no competitor | 13 108 | 3 067 | 0.099 | 2 005 | 0.476 |
| floor 0.5 %, competing | 66 | 4 073 | 0.004 | 4 054 | 0.049 |
| floor 1 %, competing | 132 | 3 453 | 0.004 | 3 379 | 0.077 |
| floor 2 %, competing | 265 | 3 202 | 0.005 | 3 068 | 0.110 |
| floor 5 %, competing | 663 | 3 104 | 0.008 | 2 846 | 0.164 |

Part of the extra cost at small floors is the clock sweep itself: the frame table keeps one slot per page of the cap, and at the floor almost all slots are empty, so a sweep passes about 13 108 / 132 ≈ 100 slots per victim at 1 %. **Decision: keep the floor at 1 % of the cap.** The floor matters only when other users hold the rest of the cap; from 0.5 % to 1 % a Zipf fetch gets 17 % faster, and each further doubling gains less (9 % to 2 %, 7 % to 5 %) while withholding more memory from statements. Constraint found by the 16-thread test: when pins exceed the floor, a fetch fails with a memory-limit error, so the floor must cover the pages that can be pinned at once.

**`std` latches against `parking_lot`.** `lab::pool::pool_threads`, 400 000 uniform random fetches per thread, 3 rounds per run, 3 runs per build interleaved; `parking_lot` 0.12.5 replaced the frame and shard `RwLock`s in a copy outside the repository; `Cargo.toml` here has no `parking_lot` (raw/t13-lab-pool-threads-*.md, summary raw/t13-lab-pool-threads-summary.md). Mfetch/s, median of the runs:

| Cache | Threads | std | parking_lot | Ratio |
|---|---|---|---|---|
| 100 % | 1 | 7.38 | 8.96 | 1.21 |
| 100 % | 4 | 31.30 | 36.34 | 1.16 |
| 100 % | 8 | 59.12 | 64.59 | 1.09 |
| 100 % | 16 | 75.73 | 76.24 | 1.01 |
| 10 % | 1 | 0.33 | 0.34 | 1.03 |
| 10 % | 4 | 1.03 | 1.05 | 1.02 |
| 10 % | 8 | 1.30 | 1.28 | 0.99 |
| 10 % | 16 | 1.21 | 1.20 | 0.99 |

A `perf record -g` of the std build shows the miss path bound by the kernel page-cache copy (`_copy_to_iter`, 40.5 % of samples) and CRC-32C (18.3 %); page-table and latch symbols are below 2 % each (raw/t13-perf-report-threads.txt). **Decision: keep `std` latches.** `parking_lot` saves about 20 ns per uncontended hit (132 → 112 ns at one thread), the gain shrinks to nothing at 16 threads, and it does not change the miss path. No performance target exists yet against which 20 ns per hit would matter, and the poisoning of `std` latches is what turns a panic under a write latch into the unusable latch. To revisit when the NFR-PERF-002 baseline sets targets.

**Double caching.** `lab::pool::pool_double_cache`, pool of 10 % (raw/t13-lab-pool-double-cache.md, raw/t13-lab-pool-double-cache-notes.md): after `fadvise(DONTNEED)` `fincore` shows 0 bytes of `data-1` resident; after the pool is filled with uniform random pages (13 108 buffers, 107 380 736 B) it shows 199 311 360 B resident: the OS cache holds every page read, including the pool's 107 MB. With the pool full, 10 000 distinct random fetches per round (about 6.5 % of them hits, by the sink counters) cost 3 065 [3 026, 3 219] ns per fetch with the OS cache warm and 93 267 [92 559, 93 558] ns with it evicted, 7.51 KiB read from the device per fetch. **Decision: keep buffered I/O (TS-08).** The duplicate is outside the cap but reclaimable by the kernel, and a pool miss served from the OS cache costs about 1/30 of a device read; direct I/O would remove the duplicate and make every pool miss a device read. No requirement yet bounds memory outside the cap.

**Limitations of #12–#13.**

1. CPU governor `powersave`, frequency not pinned; one session; the task #7 `fdatasync` variance applies to the create timings.
2. `lab::pool` ran on ext4 only; misses were served by the OS cache except in the double-caching run.
3. Per-operation counters include the harness's page draw. The 100 % hit row of `perf stat` varies by a factor of two between rounds.
4. VmRSS comparisons include memory the harness allocates.
5. The two 3.1 ms XFS `fallocate` calls and the 8.1 against 12.1 µs growing-pool miss were not attributed.

Reproduction:

    cargo test -p lpg --profile lab --lib --no-run      # BIN below
    LPG_LAB_EXT4=… LPG_LAB_XFS=… LPG_LAB_OUT=raw BIN --exact lab::files::files --ignored --test-threads=1
    LPG_TEST_DIR=<dir> LPG_FILES_MODE=<none|create|create_open> strace -c -f -o raw/t12-strace-c-<mode>-<fs>.txt \
        BIN --exact lab::files::files_one --ignored --test-threads=1
    LPG_TEST_DIR=<dir> LPG_GROW_GIB=4 strace -f -T -e trace=fallocate -o raw/t12-strace-fallocate-<fs>-rep<N>.txt \
        BIN --exact lab::files::files_growth --ignored --test-threads=1
    LPG_LAB_EXT4=… LPG_LAB_OUT=raw BIN --exact lab::pool::<pool|pool_floor|pool_double_cache> --ignored --test-threads=1
    LPG_LATCH=<label> … BIN --exact lab::pool::pool_threads --ignored --test-threads=1
    raw/t13-perf-stat.sh                                # perf stat of pool_fixed
    LPG_POOL_SHARE=<100|10|1> LPG_POOL_MODE=<hit|miss> LPG_PERF_N=<N|0> \
        <strace -c -f | heaptrack> BIN --exact lab::pool::pool_fixed --ignored --test-threads=1
