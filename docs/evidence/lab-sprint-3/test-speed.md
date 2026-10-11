# Test speed

Measurements of the test gates before and after the test-speed changes of 2026-10-11, and the proofs that the tests lost no effectiveness. Base: commit `0a6e549`. Machine: 16 logical CPUs, 30 GiB RAM, rustc 1.99.0, cargo-nextest 0.9.148. The lab directories are `/data/tmp/lpg-lab` (ext4 on `/dev/nvme0n1p1`) and `/mnt/lpg-xfs` (XFS on `/dev/loop59`, whose backing file `/data/tmp/lpg-xfs.img` is on the same ext4). The "before" figures of the full run and the stress profile come from a build of `0a6e549` in a separate worktree, measured the same day. No figure here describes production performance, and no product test asserts on time.

## Changes

| Change | Where |
|---|---|
| The runs of one fault-injection sweep run in parallel, two threads per lab directory, with the same seeds and oracles; the sweep asserts that the runs that ended equal the runs planned | `src/suites/fault.rs` (`run_all`, `SWEEP_THREADS`, `sweep_dir`) |
| The lab directories of both crash suites run at once | `src/suites/mod.rs` (`each_dir`), `src/suites/fault.rs`, `src/suites/kill.rs` (`kill_dir`) |
| `elle-cli` runs with `-XX:TieredStopAtLevel=1`; histories go to `/tmp/lpg-histories/` instead of `target/lpg-histories/` in the repository | `src/test_support/history.rs` |
| The slowest tests start first (nextest `priority`) | `.config/nextest.toml` |
| One script runs the independent gates at once, then the stress profile alone | `.config/gates.sh` |

## Wall time of each gate

| Gate | Before | After | Raw |
|---|---|---|---|
| `cargo nextest run -p lpg` (default, `/tmp/lpgram`) | 18.25, 18.33, 18.33 s | 15.15, 15.18, 15.29 s | [measurements](raw/test-speed-measurements.txt) §1, §6 |
| Full run, `LPG_LAB_EXT4` and `LPG_LAB_XFS` | 274.0 s | 147.4 s (final run); 145.4 and 144.3 s in two other runs of the same tree | [before](raw/nextest-full-before-test-speed.log), [after](raw/nextest-full-after-test-speed.log), [configurations](raw/test-speed-configurations.txt), [gates](raw/gates-test-speed.txt) |
| Full run, `LPG_LAB_EXT4` only | 156.1 s (158.6 s in round D) | not run alone | [configurations](raw/test-speed-configurations.txt) |
| Stress profile (lab directories set) | 111.4 s | 61.2–64.9 s (four runs) | [before](raw/stress-before-test-speed.txt), [gates](raw/gates-test-speed.txt) |
| `loom` (cold build and run) | 47 s | 47 s (unchanged; no change touches it) | [gates](raw/gates-test-speed.txt) |
| `shuttle` (cold build and run) | 56 s | 56 s (unchanged) | [gates](raw/gates-test-speed.txt) |
| `cargo fmt --check`, `clippy` (warm), `clippy --cfg fuzzing` (new target directory) | 1, 0, 5 s | 1, 0, 5 s | [gates](raw/gates-test-speed.txt) |
| Every gate, lab directories set | 494 s: the old tree's gates one after the other, summed (1 + 0 + 5 + 274 + 47 + 56 + 111) | 269 s with `.config/gates.sh` (332 s for the same commands one after the other) | [gates](raw/gates-test-speed.txt) |
| Every gate, no lab directories | 186 s, the new tree's gates one after the other | 148 s with `.config/gates.sh` | [gates](raw/gates-test-speed.txt) |

Each lab-run figure is one run, except where several are listed. Three runs of the adopted tree alone took 145.4, 144.3 and 147.4 s (nextest summary); a single run of the old tree was measured.

## Levers

| Lever | Measurement | Decision |
|---|---|---|
| 1. Parallel runs of a fault sweep | Restore sweep alone on ext4, `LPG_SEED=3`: 99.3 s on 1 thread, 56.6 s on 2, 33.6 s on 4, 30.4 s on 8, 37.4 s on 16; CPU 100, 113, 133, 241 and 565 s, mostly kernel time. Full run: 163.8 s with 4 threads per directory, 145.4 s with 2 | Kept, 2 threads per directory |
| 2. Kill suite directories at once | ext4 45.9 s and XFS 54.0 s one after the other before; at once, 70.3 and 78.7 s in the final run, where they overlap the sweeps. Iterations inside one directory chain the database and stay in order | Kept |
| 3. `disk-heavy` cap | Default run: 754 MiB peak in `/tmp/lpgram` with 3, 1.9 GiB with 16, and 19.0–19.6 s against 20.0–20.3 s. Full run with cap 2 and 4 threads per directory: 147.9 s. XFS: kill alone peaks at 3.5 GiB, a sweep at up to 1.1 GiB, the final run at 4.7 GiB on an 8 GiB file system | Kept at 3: a higher cap is not faster and the XFS lab has no room for it |
| 4a. Elle JVM C1 only | 4.7–4.9 s of CPU per history fell to 1.9–2.0 s, wall time 1.6 s against 1.5 s; JSON output byte-identical on 11 histories. Default run 19.0–19.6 s to 15.5–15.7 s; stress 111.4 s to 61–65 s | Kept |
| 4b. Priorities | Default run 15.52–15.71 s without, 15.29–15.43 s with | Kept (it also starts the critical lab suites first) |
| 4c. Restore sweep with 4 threads, the others 2 | Full run 154.3 s (commits sweep slowed to 120 s) | Rejected |
| 4d. Internal parallelism of the million random operations, the ten-times-cap B+tree test, the Elle tests and the index crash test | The first two run one dependent sequence of operations each. A default run uses 13 of 16 cores on average (254 CPU-seconds in 19.4 s before the JVM change), so splitting a test adds no throughput; the index crash test resumes the chain at its first installed point, an order-dependent choice. None of them is the critical path of the full run | Rejected, no change |
| 5a. `opt-level` 2 | Default run 14.7–14.9 s against 15.3–15.4 s; kill alone 40.0 s against 41.0–41.6 s; rebuild after an edit 8.2 and 14.1 s against 7.4 and 12.8 s; crate rebuild without incremental 40.5 s against 37.2 s | Rejected: the edit-and-test cycle gets slower |
| 5b. Linker | `Linker: LLD 23.1.1` in the test binary's `.comment`; LLD is the default on `x86_64-unknown-linux-gnu` since Rust 1.90 (Rust blog, 2025-09-18) | Nothing to change |
| 5c. One build for `loom` and `shuttle` | Each `--cfg` changes `RUSTFLAGS`, which rebuilds every crate; `src/sync.rs` picks one checker per build (`all(shuttle, not(loom))`) | Not possible |
| 6. Gate runner | 494 s to 269 s with the lab directories; 186 s to 148 s without; no gate failed. Under it, nextest (lab) took 191.9 s, `loom` 187 s and `shuttle` 204 s, against 144.3, 47 and 56 s alone | Kept |
| 7. Fuzz fork mode | `ignore_timeouts` and `ignore_ooms` default to 1 in fork mode (`FuzzerFlags.def:69-70`); a non-ignored exit stops all jobs (`FuzzerFork.cpp:457-475`) | Recorded as the recipe in TS-30 |
| — Larger `glibc` mmap threshold for the sweeps | Page faults of the restore sweep fell from 8.76 M to 0.08 M; wall time 33.6 s to 31.5 s, kernel time stayed at 78–83 s | Rejected: not where the time goes |

The kernel time of the sweeps is file I/O: in the first 30 s of the restore sweep, `pwrite64` took 18.2 s in 83 532 calls and `pread64` 10.4 s in 478 196. Kernel symbols are hidden on this machine (`kernel.kptr_restrict = 1`), so the kernel functions behind the contention past 4 threads are not identified: unverified.

## Effectiveness

| Proof | Result | Raw |
|---|---|---|
| Test count | 508 run, 28 skipped, before and after, on ext4 and XFS | [before](raw/nextest-full-before-test-speed.log), [after](raw/nextest-full-after-test-speed.log) |
| Case counts of every sweep | The 16 `SUITE fault` lines (8 workloads × 2 file systems) agree in crash points, runs per crash mode, failure runs and page-copy failure runs before and after (`diff` empty) | same |
| Kill suite | 200 iterations per directory; every required count above 0 on both file systems (kills in a checkpoint, a backup and an index build, recycling checkpoints, printed drops, finished builds) | [after](raw/nextest-full-after-test-speed.log) |
| Mutant MF: the WAL group write's `sync_data` loop removed | `suites::fault::commits` fails on ext4 and XFS (`Corruption("page 7 fails its checksum")` at reopen) | [mutants](raw/test-speed-mutants.txt) |
| Mutant MF2: the checkpoint's data-file sync removed | `suites::fault::checkpoint` fails | same |
| Mutant MD: only the last planned run of each directory fails | `suites::fault::commits` fails with "run 2077 of 2078 (FailCopy(1381))" on both directories: no case is skipped and a single failing case fails the test | same |
| Mutant MK: recovery never writes the pages of the last valid WAL group | `suites::kill::kill_and_reopen` fails on ext4 (iteration 0) and XFS (iteration 9): "17 commits present, last printed 17" | same |
| Round D mutation set M1–M7, same command | The same 8 tests fail and the same scenarios are listed as in round D; files restored, `git diff` empty | [round D](raw/mutations-round-d.txt), [now](raw/mutations-test-speed.txt) |
| Elle JVM flag | Byte-identical `elle-cli` output on 11 histories; the tests that need an invalid verdict pass | [measurements](raw/test-speed-measurements.txt) §4 |
| `loom`, `shuttle`, stress | 13 and 4 models pass; 69 stress tests × 16 copies pass, in every gate run | [gates](raw/gates-test-speed.txt) |

All mutants were applied by hand, run and reverted; the working tree held only the test-speed changes after each.

## RAM drive audit

- Graph data: every test that is not a lab run creates its directories in `/tmp/lpgram` (`graph_dir`, `graph_base`, `test_dirs`). The crash suites run on `/tmp/lpgram` without the lab variables and on ext4 and XFS with them; the lab runs stay on persistent storage.
- Written elsewhere and moved: the Elle histories, from `target/lpg-histories/` in the repository on disk to `/tmp/lpg-histories/`.
- Written elsewhere and left: the ignored laboratory runs of `storage::fs` (`LPG_TEST_DIR`, else the system temporary directory, which is `/tmp`), and `lab::tests::text_round_trip` (a report file in a temporary directory under `/tmp`); neither is graph data. Files written only on request (`LPG_TCK_REPORT`, `LPG_DIFF_OUT`, `LPG_MICRO_OUT`, the corpus and generated-source updates) go where the caller names.
- Builds: `cargo metadata` resolves the target directory of `.`, `fuzz/` and `tools/gql-oracle/` to `/dev/shm/lpg-target`; `cargo llvm-cov show-env` reports `/dev/shm/lpg-target` and its `profraw` files there. `cargo fuzz` without `--target-dir` uses that directory, but `cargo fuzz coverage` uses `fuzz/target/<triple>/coverage` (cargo-fuzz 0.13.2 `src/project.rs:354-371`), so the recipe always passes `--target-dir`.
- `cargo-mutants` is not installed, so its effect could not be measured. Its documentation says it copies the tree to a temporary directory and does not copy `target/`; whether the copied `.cargo/config.toml` then sends every job's build to the one shared `/dev/shm/lpg-target` is unverified.

## Space

| Location | Before | After |
|---|---|---|
| `/tmp/lpgram`, default run | not measured on the old tree | 754 MiB |
| `/tmp/lpgram`, full run | – | 530 MiB |
| ext4 lab, full run | 10.2 GiB | 5.7 GiB |
| XFS lab, full run (8 GiB file system) | 6.5 GiB | 4.7 GiB |

Peaks are `du -sk` sampled every 0.2 s.

## References studied

- nextest @ `b5e6fca`, `site/src/docs/configuration/`: `test-priorities.md` (higher priority runs first), `test-groups.md` (`max-threads`), `threads-required.md` (a heavy test counts as several threads; "benchmark your test runs"). `--partition` shards a run across machines and does not apply to one host.
- libSQL @ `d6c75af`, `libsql-sqlite3/test/testrunner.tcl`: jobs on as many processes as there are cores (lines 165-200); slow and very slow scripts get a higher priority (lines 1026-1028).
- Turso @ `be5bab8`, `.config/nextest.toml`: `loom` and `shuttle` in their own profiles with one test thread each; no parallel crash driver was found in its tests.
- The Cargo Book, "Profiles": `test` inherits `dev`; `dev` has `debug-assertions` and `overflow-checks` on.
- Rust blog, "Announcing Rust 1.90.0": LLD is the default linker on `x86_64-unknown-linux-gnu`.
- llvm-project @ `f3653503`, `compiler-rt/lib/fuzzer/FuzzerFork.cpp` and `FuzzerFlags.def`.
- cargo-fuzz 0.13.2 (`984c861`), `src/project.rs`.
