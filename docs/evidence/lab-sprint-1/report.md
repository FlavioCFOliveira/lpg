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

## #14

`src/storage/freelist.rs` (persisted free list, in-memory free-page set, reader registry), `src/storage/btree/` (`node.rs` page layouts, `mod.rs` reads and the page-change path, `write.rs` the writer), `src/storage/check.rs` (integrity check). Changed: `pool.rs` (`create`, `resident`, `write_dirty`), `datafile.rs` (page 1 written at create; `page_count`; unverified read for the check), `engine.rs` (tree state, free list loaded at open). Laboratory: `src/lab/btree.rs`. Gates: raw/t14-quality-gate.txt; suite: raw/t14-nextest.txt (101 passed, both lab directories); coverage: raw/t14-coverage.txt.

**Acceptance** (`cargo nextest run -p lpg storage::freelist storage::btree storage::check`, all pass):

| Check | Test | Result |
|---|---|---|
| 10^6 random puts and deletes match a `BTreeMap`; check passes every 10^4 ops | `btree::tests::a_million_random_puts_and_deletes_match_a_btreemap_model` | pass; grow, shrink (range deletes, 1 351 leaves freed in a probe run) and grow phases; `get`, 8- and 13-byte prefix scans and a final full scan compared; freed pages reused |
| A tree 10× the cap reads back intact | `a_tree_ten_times_larger_than_the_cap_reads_back_intact` | pass; 260 000 entries, cap 2 MiB, every value by `get` and by a full scan |
| Overflow values round-trip | `overflow_values_round_trip` | pass; 0, 2 048, 2 049, 8 156, 8 157, 100 000 and 1 MiB bytes, replaced inline↔overflow, deleted; chains freed |
| Check reports a flipped bit, swapped keys, a broken sibling link, a page in a tree and the free list | `check::tests::*` | pass; each test first checks the clean database passes; a high-key mismatch and a leaked page are also reported |
| Keys of 1 024 bytes; longer refused | `keys_at_the_limit_work_and_longer_keys_are_refused` | pass; 300 keys of 1 024 B with 2 048 B values; a batch with a 1 025 B key fails with nothing applied |
| Right-most split leaves the left page 90 % full | `rightmost_split_fills_left_page_to_90_percent_and_others_to_half` | pass; every non-last leaf of an append-only tree in (0.90 − one entry, 0.90]; random order: minimum fill ≥ 0.5 − one entry |
| Concurrent readers during splits see every committed key once | `concurrent_readers_during_splits_see_every_committed_key_exactly_once`, `stale_descents_recover_by_moving_right`, `emptying_a_subtree_frees_its_inner_pages` | pass; 4 readers against one writer with leaf, inner and root splits and page frees; the second test reproduces a stale pointer to a split page and to a deleted page deterministically |

Non-vacuity: 13 deliberate mutations, each reverted, each failed the tests named: right-most fill 0.80; reuse ignoring readers; no move-right on high key; no move-right on deleted page; scan ignoring the prefix end; no key limit; no unlink from the left sibling; freeing the last child of a parent; restart every 17 entries; the check without double-claim detection, without the sibling-link, key-order or high-key check. A first version of the model test emptied no leaf, so the unlink mutation survived; range deletes were added and it now fails.

Line coverage (`cargo llvm-cov nextest`): `btree/node.rs` 96.8 %, `btree/write.rs` 95.7 %, `btree/mod.rs` 90.0 %, `freelist.rs` 94.2 %, `check.rs` 82.4 % (unreached: reports of malformed pages other than those tested).

**Laboratory.** `lab::btree::btree` on `LPG_LAB_EXT4`: M-LAYOUT's graph (10^6 nodes, 10^7 edges, Zipf 0.8 and uniform), 2×10^7 adjacency keys `(u64, u8, u32, u64, u64)`, 29 bytes, empty values, inserted in random order one edge (two entries) per batch with the whole tree in cache, written and synced; then per cache share, 3 rounds, median [min, max] (raw/t14-lab-btree.md, raw/t14-lab-btree-shape.md). Cold = OS cache of `data-1` evicted before each round, pool kept. Put = one key per batch.

| Dist | Cache | get ns | prefix scan ns (entries) | put ns | KiB read/op cold: get, scan, put |
|---|---|---|---|---|---|
| Zipf | 100 % | 1 421 [1 405, 1 432] | 21 718 [21 554, 21 926] (1 007) | 5 905 [5 878, 6 026] | 0.01, 0.01, 0.00 |
| Zipf | 10 % | 3 618 [3 616, 3 630] | 25 210 [25 090, 25 434] | 11 132 [10 815, 11 515] | 7.20, 12.66, 4.25 |
| Zipf | 1 % | 3 803 [3 801, 3 807] | 29 558 [29 442, 29 764] | 11 978 [11 969, 12 660] | 8.13, 18.07, 8.56 |
| Uniform | 100 % | 1 417 [1 403, 1 436] | 1 591 [1 586, 1 638] (3.5) | 5 799 [5 798, 5 888] | 0.01, 0.01, 0.00 |
| Uniform | 10 % | 3 668 [3 659, 3 681] | 3 874 [3 823, 3 875] | 11 224 [10 766, 11 536] | 7.18, 7.22, 3.09 |
| Uniform | 1 % | 3 796 [3 780, 3 832] | 4 050 [4 037, 4 126] | 12 160 [12 159, 12 830] | 8.11, 8.65, 7.99 |

Cold latencies: get 80 µs (10 %) and 88–92 µs (1 %); cold put at 10 % varies from 7.6 to 69 µs between rounds (raw table). Scan prefixes are taken from sampled entries, so Zipf scans favour high-degree nodes (about 1 000 entries per scan). Build: 5.9 µs per entry (Zipf) and 5.3 µs (uniform), 0.6 s to write the 61 234 and 63 892 dirty pages.

**Shape against M-LAYOUT layout B** (online, random order; `docs/evidence/layout-bench/results.md`, "Space"):

| | lpg #14 Zipf | lpg #14 uniform | M-LAYOUT B Zipf | M-LAYOUT B uniform |
|---|---|---|---|---|
| leaf bytes per entry | 16.81 | 17.88 | 11.8 | 12.3 |
| leaf fill | 0.673 | 0.686 | 0.75 | 0.75 |
| height | 3 | 3 | 4 | 4 |
| leaves | 60 970 | 63 618 | 38 267 | 40 025 |

The differences have measured or stated causes, not all attributed: lpg compresses keys as byte strings (TS-06 for every tree but the adjacency tree), while B stores field deltas as varints (the adjacency encoding, Sprint 2); lpg has no stamps, B carries two per entry; lpg's page header is 50 bytes. Fill 0.67–0.69 equals the ln 2 ≈ 0.69 expected of balanced splits under random inserts; the cause of B's 0.75 was not investigated. Height 3 against 4: lpg truncates separators (suffix truncation) and has 8 KiB inner pages of about 230 children (60 970 leaves under 261 non-root inner pages).

**CPU.** `perf stat` of `lab::btree::btree_fixed`, uniform, per operation = (run with N − run with 0) / N, 3 interleaved rounds (raw/t14-perf-stat.txt, script raw/t14-perf.sh). Zen 3 has no per-process LLC-miss event; as in #13, DRAM fills are `ls_any_fills_from_sys.mem_io_local`; `cache-misses` is the generic event.

| Cache | Op | Cycles | Instructions | IPC | Branch misses | cache-misses | DRAM fills |
|---|---|---|---|---|---|---|---|
| 100 % | get | 6 447 [6 340, 6 718] | 4 501 | 0.70 | 10 | 38 | 15 |
| 10 % | get | 16 250 [16 157, 16 955] | 19 321 | 1.19 | 70 | 103 | 90 |
| 100 % | scan | 7 327 [7 038, 8 050] | 7 632 | 1.04 | 14 | 42 | 19 |
| 100 % | put | 25 933 [25 749, 26 078] | 62 990 | 2.43 | 171 | 124 | 36 |

Flame graphs (`perf record --call-graph dwarf`, `inferno`, samples under `get_loop` / `put_loop` only; perf.data kept outside the repository): raw/t14-flamegraph-get.svg, raw/t14-flamegraph-put.svg, folded stacks raw/t14-perf-get.folded, raw/t14-perf-put.folded. `read_hpet` frames are samples taken in the HPET clock-source read (attribution not established; 3.5 % in an earlier `get` profile).

- `get` at 100 %: `leaf_get` 59.0 % of samples, of which `find_group` (binary search over restart points) 49.2 %; `inner_child` 29.0 %. `raw_entry` holds 55.3 % self time: the first touch of each restart entry, at random offsets of a page not in the CPU caches (15 DRAM fills per get).
- In-leaf search on 2 000 resident leaf images (about 16 MB, outside L2): 236 [235, 276] ns per `leaf_get` (Zipf), 249 [237, 283] (uniform); decoding with key reconstruction 10.4–11.2 ns per entry; 329–347 entries per leaf.
- `put` at 100 %: `leaf_apply` 91.7 %, of which decoding 45.1 % and re-encoding 40.1 %; `memcpy` of a few bytes at a time is 23.7 % self. TS-06 keeps every 16th entry a restart point, so an insert or delete shifts every later restart point and the leaf is rewritten from the key's group to its end (half a leaf on average).

**Improvement applied.** The first profile of `put` (raw/t14-perf-put-before.folded) showed the byte-wise `common_prefix` at 35.9 %. Comparing 8 bytes at a time took `put` from 6 714 to 6 030 ns, and reusing the shared lengths decoded from the page took it to 5 847 ns, −13 % in all (raw/t14-put-optimisation.txt, 3 runs each).

**Allocations.** heaptrack, run with N − run with 0 (raw/t14-heaptrack-summary.txt): `get` 1 call in 10^6; prefix scan 5.1 calls per scan (the copy buffers of each scan); `put` 1.01 calls per put (the tree list of `apply`).

**Finding: in-leaf format.** At 100 % cache, in-leaf search is 59 % of `get` (about 0.84 µs of 1.42 µs), mostly cache misses of the restart binary search. The paper "B-Trees Are Back" measures lookup gains of 16–64 % from heads and up to 26 % from hints, in memory, on slotted leaves without delta encoding, at a cost of about 5.7 B per record for heads (its §4.2–4.3). On this workload that would be +32–34 % leaf bytes (5.7 on 16.8–17.9 B), and at 1–10 % cache a get already reads 7.2–8.1 KiB from the device, where the cost is I/O: cold gets take 78–92 µs against 1.4 µs warm. No alternative was measured here, so no claim that it would win is made. A hints array over the restart keys (64 bytes in the header, no per-entry cost) addresses the same misses and is a candidate for a spec decision with measurement.

**Finding: put cost of the literal restart rule.** A put costs 5.8–5.9 µs at 100 % cache, against M-LAYOUT B's 3.8–4.7 µs for a whole edge (two entries and an edge-table insert), whose leaves re-encode only the group holding the key (groups of up to 32 entries). Here decoding and re-encoding the tail of the leaf take 85 % of `put`. Two options keep the format: copy the bytes of unchanged entries and re-encode only the two entries per moved group boundary; or, with a spec change, let groups vary as in M-LAYOUT. Neither is implemented.

**Finding: deletes can split.** With delta encoding, removing an entry can lengthen the leaf (the entry that becomes a restart point stores its whole key). The 10^6-operation test hit it; such a delete now splits the leaf. The TS-05 reserve bound counts write-set bytes, so a batch of many such deletes, or of deletes freeing long overflow chains that need new free-list trunks, is not covered by it.

**Limitations of #14.**

1. ext4 only; one session; CPU governor `powersave`.
2. Single-threaded measurements; reader concurrency was tested, not measured.
3. Puts during the read workloads add 66 000 entries per share (0.3 %) to each database.
4. M-LAYOUT read misses with `O_DIRECT`; lpg reads through the OS cache, evicted with `fadvise` in cold mode.

Reproduction:

    cargo test -p lpg --profile lab --lib --no-run      # BIN below
    LPG_LAB_EXT4=… LPG_LAB_XFS=… LPG_LAB_OUT=raw BIN --exact lab::btree::btree --ignored --test-threads=1
    BIN=… SCRATCH=<outside the repository> raw/t14-perf.sh   # perf stat, perf record + inferno, heaptrack
    LPG_BTREE_DIST=uniform LPG_BTREE_SHARE=100 LPG_BTREE_MODE=put LPG_PERF_N=200000 \
        BIN --exact lab::btree::btree_fixed --ignored --test-threads=1 --nocapture

Both lab directories were emptied after the runs.

### Put optimisation

**Change.** `node::leaf_apply` no longer decodes and re-encodes the leaf from the key's restart group to its end. An insert or delete shifts every later entry by one position, so every later group boundary moves by one entry. Only the entries whose encoding changes are re-encoded: the changed entry, the entry after it (its predecessor changed), and at each later boundary the entry that becomes a restart point (it now stores its whole key) and the one that stops being one (it now stores a delta against its predecessor). Every other entry keeps its predecessor and its restart status, so its bytes are copied unchanged, one copy per run. The leaf is still walked from the key's group to its end, to find the boundaries and reconstruct the keys they need. The TS-06 format is unchanged. The decode-and-re-encode path remains for splits (`leaf_decode_with`, `write_leaf`) and as the test reference (`leaf_apply_reencode`, test builds only).

**References.** LevelDB `table/block_builder.cc:71-104` (`Add`) and RocksDB `table/block_based/block_builder.cc` (`AddWithLastKeyImpl`, `Finish`) only build blocks by appending, and never change a finished block in place except for RocksDB's one-off rewrite of restart keys in `Finish`. RocksDB's data-block hash index (`table/block_based/data_block_hash_index.h`) maps keys to restart intervals for point reads and does not affect writes. SQLite `src/btree.c` (`insertCell` 7363, `dropCell` 7299) and LeanStore `backend/leanstore/storage/btree/core/BTreeNode.cpp` (`insert` 88-100, `removeSlot` 398-401) change slotted pages in place: they write the new cell once and `memmove` the slot array, never re-encoding unchanged cells. The pattern adopted is theirs (copy unchanged bytes, encode only what changed), applied to a delta-encoded stream, where entries at moved group boundaries also change.

**Byte equality.** `node::tests::apply_matches_the_full_reencode` applies each change with both paths to copies of the same page and requires equal outcomes and identical page bytes, including the zeroed free gap and the restart array. It covers 120 random leaves of 1 000 operations each: short keys over a three-letter alphabet, and keys with a 150-byte common prefix (two-byte shared varints); inline values up to 300 bytes (two-byte tags) and overflow values; with and without a high key; inserts, replacements, deletes and deletes of absent keys. A probe run counted 120 000 operations, 313 deletes that grew a leaf, 12 008 puts that did not fit and 66 495 changes that moved at least one later boundary; the test asserts minimums for all three counts. `deletes_that_grow_the_leaf_match_the_full_reencode` builds a leaf whose restart keys share 602 bytes with their successors. A delete with one later boundary grows the leaf by more than 500 bytes and still fits; a delete with nine later boundaries does not fit, and both paths return `Full` with the page unchanged. Non-vacuity: two mutations, each reverted, made the random test fail: copying entries whose predecessor changed, and re-encoding a boundary delta against a stale key. End to end, the 2×10^7-entry databases built by the new code have `data-1` files byte-identical to those built by the old code (`cmp`, both distributions; raw/t14-opt-build.txt).

**A/B method.** The pre-change working tree and the changed one were copied outside the repository and built with the `lab` profile. The two copies differ only in `btree/node.rs` and `btree/write.rs`. The old code built both databases once (pristine copies kept). For each full `lab::btree` run, the pristine databases were restored first, in the order before, after, before, after (raw/t14-opt-lab-btree-{before,after}-{1,2}.md). `btree_fixed` and `perf stat` were interleaved per configuration over 3 rounds (raw/t14-opt-perf-stat.txt). Script: raw/t14-opt-ab.sh. Same host, session and parameters as above.

Full `lab::btree`, warm OS cache, ns per operation, median [min, max] of 3 rounds, two runs per version:

| Dist | Cache | Op | Before, run 1 | Before, run 2 | After, run 1 | After, run 2 | Change |
|---|---|---|---|---|---|---|---|
| Zipf | 100 % | put | 5 759 [5 756, 6 022] | 5 764 [5 737, 5 994] | 3 820 [3 753, 4 015] | 3 792 [3 678, 3 963] | −33.9 % |
| Zipf | 100 % | get | 1 428 [1 398, 1 433] | 1 417 [1 411, 1 439] | 1 435 [1 389, 1 440] | 1 420 [1 398, 1 434] | +0.4 % |
| Zipf | 10 % | put | 11 161 [10 686, 11 410] | 11 404 [10 686, 11 507] | 9 209 [8 737, 9 636] | 9 320 [8 646, 9 507] | −17.9 % |
| Zipf | 10 % | get | 3 677 [3 677, 3 693] | 3 745 [3 742, 3 761] | 3 751 [3 742, 3 771] | 3 723 [3 714, 3 738] | +0.7 % |
| Zipf | 1 % | put | 12 225 [11 980, 12 970] | 12 256 [12 014, 12 969] | 10 402 [9 904, 11 190] | 10 249 [10 022, 11 001] | −15.6 % |
| Zipf | 1 % | get | 3 946 [3 939, 3 962] | 3 982 [3 978, 3 982] | 3 923 [3 908, 3 939] | 3 932 [3 930, 3 972] | −0.9 % |
| Uniform | 100 % | put | 5 858 [5 842, 5 883] | 5 774 [5 707, 5 893] | 3 722 [3 658, 3 915] | 3 750 [3 677, 3 909] | −35.8 % |
| Uniform | 100 % | get | 1 436 [1 421, 1 440] | 1 429 [1 425, 1 435] | 1 419 [1 412, 1 419] | 1 399 [1 392, 1 413] | −1.6 % |
| Uniform | 10 % | put | 11 523 [10 545, 11 636] | 11 327 [10 674, 11 687] | 9 270 [8 607, 9 542] | 9 219 [8 612, 9 520] | −19.1 % |
| Uniform | 10 % | get | 3 662 [3 658, 3 678] | 3 714 [3 711, 3 718] | 3 692 [3 684, 3 707] | 3 679 [3 665, 3 680] | −0.1 % |
| Uniform | 1 % | put | 12 287 [12 188, 12 952] | 12 304 [12 213, 13 313] | 10 269 [10 151, 10 915] | 10 227 [10 100, 10 917] | −16.7 % |
| Uniform | 1 % | get | 3 845 [3 832, 3 856] | 3 873 [3 872, 3 886] | 3 812 [3 794, 3 819] | 3 801 [3 800, 3 824] | −1.4 % |

Change = mean of the two after medians against the mean of the two before medians. At 10 % and 1 % cache, a put also pays for evicting and writing back pages, which the change does not affect, so the relative gain is smaller. Get changes stay within the run-to-run spread of the old code (1.8 %: 3 677 against 3 745 ns), and get executes the same instruction count (below); the get path code is unchanged. Cold rows are dominated by device reads and show no change outside their spread. Cold put: Zipf 1 % 87.9 and 89.9 µs before, 91.3 and 91.6 µs after, with overlapping ranges ([86.4, 89.7], [89.5, 92.3] against [88.3, 91.9], [88.0, 96.5]). Uniform 1 %: 82.8 and 83.9 µs before, 80.1 and 83.1 µs after. Integrity check: it passed after every share in all four runs (the workload asserts it), and the shape figures are identical across runs.

`perf stat` of `btree_fixed`, 100 % cache, per operation, median [min, max] of 3 interleaved rounds:

| Dist | Op | Version | ns | Cycles | Instructions | IPC | Branch misses | cache-misses | DRAM fills |
|---|---|---|---|---|---|---|---|---|---|
| Uniform | put | before | 5 839 [5 798, 5 867] | 25 761 [25 133, 26 252] | 63 053 | 2.45 | 173 | 118 | 37 |
| Uniform | put | after | 3 763 [3 741, 3 773] | 16 770 [16 740, 16 968] | 35 990 | 2.13 | 93 | 96 | 40 |
| Zipf | put | before | 5 901 [5 869, 5 909] | 26 092 [25 877, 27 208] | 66 776 | 2.56 | 133 | 111 | 38 |
| Zipf | put | after | 3 824 [3 790, 4 009] | 18 875 [16 093, 18 998] | 38 164 | 2.04 | 79 | 86 | 44 |
| Uniform | get | before | 1 418 [1 409, 1 420] | 6 427 [6 407, 6 506] | 4 607 | 0.72 | 11 | 41 | 16 |
| Uniform | get | after | 1 432 [1 408, 1 436] | 6 532 [6 332, 6 533] | 4 596 | 0.70 | 11 | 41 | 16 |
| Zipf | get | before | 1 412 [1 411, 1 430] | 6 440 [6 419, 6 710] | 4 618 | 0.72 | 10 | 41 | 17 |
| Zipf | get | after | 1 426 [1 419, 1 427] | 6 251 [5 999, 6 523] | 4 627 | 0.72 | 10 | 39 | 15 |

Put instructions fall by 43 % and cycles by 28–35 %; IPC drops from 2.45–2.56 to 2.04–2.13. The data files were hashed before and after these runs and were unchanged. Building the databases (two entries per batch) took 3 839 ns per entry (Zipf) and 3 508 ns (uniform), against 5 856 and 5 547 ns with the old code (−34 % and −37 %; raw/t14-opt-build.txt).

**Profile after** (`perf record --call-graph dwarf`, uniform, 100 %; raw/t14-opt-flamegraph-put.svg, raw/t14-opt-perf-put.folded). `leaf_apply` takes 87.7 % of `put_loop` samples, against 91.7 % before, of a put that is 36 % shorter. Its main direct children (the rest, each under 4 %, are index checks, `is_multiple_of`, key comparison and `value_of`):

| Part | Before | After |
|---|---|---|
| Decode into `Entries` (`leaf_decode`) | 45.1 % | — |
| Re-encode (`write_entries`; after: `encode_parts` + `common_prefix`) | 40.1 % | 4.7 % |
| Page write (`commit_tail`) | in `write_entries` | 3.7 % |
| Walk: `Vec::extend_from_slice` (key reconstruction and run copies; the profile does not separate them), `raw_entry`, `truncate`, `leaf_apply` self | — | 54.6 % |
| `find_group` | 2.9 % | 9.6 % |

Re-encoding fell from 40.1 % to 4.7 % of samples. The walk of the leaf tail now dominates `put`: its bytes are parsed and the key chain is rebuilt entry by entry to reach each boundary key. The run copies and the key reconstruction both go through `Vec::extend_from_slice` (23.6 %) and `memcpy`; how the time splits between them is not measured. Some samples are attributed to `Pool::fetch` under `leaf_apply` (1.8 %), which the code does not call; this is taken as a DWARF unwinding artefact, not verified.

**Costs.** `node.rs` grows by about 200 lines outside its tests (199: the new `leaf_apply`, `encode_parts`, `commit_tail`, and the reference kept for tests), and `Encoder` holds three more scratch key buffers. A replacement of an existing key's value also walks the tail, although nothing after the entry needs re-encoding there; this case was not measured (the workloads insert new keys).

**Limitations.** As for #14 above (ext4, one session, `powersave` governor, single-threaded). The cold-cache rows were not analysed beyond their spread.

Both lab directories were emptied after the runs.
