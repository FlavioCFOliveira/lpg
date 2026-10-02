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
