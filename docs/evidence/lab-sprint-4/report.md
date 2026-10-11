---
title: Sprint 4 correctness laboratory
status: living record
last-reviewed: 2026-10-11
related:
  - ../README.md
  - ../lab-sprint-3/report.md
  - ../planning/sprint-4-plan.md
---

# Sprint 4 correctness laboratory

Validation pass of Sprint 4, bulk import and export (rmp #60), with the acceptance items that tasks #54, #56, #57, #58 and #59 left for it. The first pass used commit `bcd5085` with a clean working tree, on 2026-10-11. A second pass, the same day, closed the gaps the first left (see [Second pass](#second-pass)); its runs used `bcd5085` plus the uncommitted test changes listed there. Sprint 4 verifies correctness only (sprint UPDATE 56): durations printed by tools are not performance claims.

Graph data of the default suite lived in `/tmp/lpgram` (tmpfs). No durability or I/O claim rests on a tmpfs run. The flush-dependent suites also ran on ext4 (`LPG_LAB_EXT4=/data/tmp/lpg-lab`) and XFS (`LPG_LAB_XFS=/mnt/lpg-xfs`).

Some measurements needed a **scratch copy**: `git archive bcd5085`, unpacked into the session scratchpad, with three test-only changes, never in the product tree ([diff](raw/scratch-copy-diff.txt)):

* `LPG_FORCE_FULL` forces the full fault sweep on tmpfs;
* the kill suite's child stderr is inherited instead of discarded;
* two test modules hold the memory measurements and four behaviour checks ([source](raw/scratch-tests.rs)).

No product code was changed by this pass.

### Second pass

Test-only changes on top of `bcd5085`, uncommitted at the time of the runs:

* `src/types/lexical/tests.rs`: `an_edge_record_form_holds_its_properties` writes an edge record form with properties and reads it back bit-equal (#56).
* `src/ops/bulk/tests/fast.rs`: `both_import_modes_report_the_same_invalid_records` and `both_import_modes_enforce_edge_uniqueness`, promoted from the scratch checks `lab60_invalid_records_both_modes` and `lab60_edge_uniqueness_both_modes`. They assert the exact error codes, counts and report lines the scratch checks printed. The input of the existing fast-load invalid-record test moved into a shared helper, `invalid_records_input`.
* `src/ops/bulk/tests/formats.rs`: `a_graphml_default_applies_to_nodes_without_the_data_in_both_modes` (from `lab60_graphml_defaults_both_modes`) and `the_neo4j_point_probe_imports_in_both_modes` (the Appendix A probe of [`neo4j-probe-into-lpg.rs`](raw/neo4j-probe-into-lpg.rs): point header options, point maps with and without `srid`, a geographic map, `NaN` and `Infinity`).
* `src/test_support/child.rs`: `spawn` pipes the child's standard error and drains it on a thread into a buffer; `Spawned::kill` kills and reaps the child and returns that text. `run` adds the child's standard error to its "ran no test" message. The new test `a_spawned_child_s_standard_error_is_kept` checks it; `child_failure_is_reported` now also checks that the child's panic message is captured.
* `src/suites/kill.rs`: every child-based failure of `kill_and_reopen` ("the child commits", a failed filler step, unreadable child output) prints the child's standard error.

The memory-measurement tests `lab60_*` stay in the scratch copy only.

## Environment

| Item | Value | Source |
|---|---|---|
| CPU | AMD Ryzen 9 5900HX, 16 logical CPUs, 2 threads per core | `lscpu` |
| Memory | 30 GiB | `free -g` |
| Operating system | Ubuntu 26.04.1 LTS, kernel 7.0.0-38-generic | `/etc/os-release`, `uname -r` |
| `/tmp/lpgram` | tmpfs (`size=15867156k,nr_inodes=1048576,inode64,usrquota`) | `findmnt -T /tmp` |
| ext4 lab directory | `/data/tmp/lpg-lab` on `/dev/nvme0n1p1`, ext4 | `findmnt -T` |
| XFS lab directory | `/mnt/lpg-xfs` on `/dev/loop59`, XFS, 8 GiB | `findmnt` |
| Build output | `/dev/shm/lpg-target` (tmpfs) | `.cargo/config.toml` |

## Tools

crates.io versions come from `https://crates.io/api/v1/crates/<name>` (`max_stable_version`), queried with the User-Agent `lpg-lab (https://github.com/FlavioCFOliveira/lpg)`. No tool needed an update ([raw](raw/toolchain.txt), [raw](raw/tools.txt)).

| Tool | Version | crates.io | Command |
|---|---|---|---|
| rustup | 1.29.1 (d95a37b6a 2026-08-13) | — | `rustup self update` ("unchanged") |
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | — | `rustup update stable nightly` ("unchanged") |
| Rust nightly | 1.101.0-nightly (6f5da406b 2026-10-10) | — | same |
| Rust MSRV | 1.89.0 (29483883e 2025-08-04) | — | `rustc +1.89 --version` |
| `rustup check` | every toolchain and rustup "up to date" | — | `rustup check` |
| cargo-nextest | 0.9.148 (cd1d6d546 2026-10-07) | 0.9.148 | `cargo nextest --version` |
| cargo-deny | 0.20.2 | 0.20.2 | `cargo deny --version` |
| cargo-llvm-cov | 0.9.1 | 0.9.1 | `cargo llvm-cov --version` |
| cargo-fuzz | 0.13.2 | 0.13.2 | `cargo fuzz --version` |
| heaptrack | 1.5.0 | — | `heaptrack --version` |
| fio | 3.43 (not run: Sprint 6) | — | `~/.local/bin/fio --version` |
| iostat, pidstat | sysstat 12.8.0 (not run: Sprint 6) | — | `~/.local/bin/iostat -V`, `pidstat -V` |
| Docker | 27.2.0 | — | `docker --version` |
| Neo4j | 2026.09.0 community, image `dd2aca3682c1` | — | `neo4j-admin --version` in the image |
| LDBC SNB datagen | image `ldbc/datagen-standalone:0.5.1-19-829b7a04-2.12_spark3.2` (`f6f3c449a392`) | — | `docker images` |
| Python | 3.14.4 (system); venv `/tmp/lpg-lab-venv` | — | `python3 --version` |
| NetworkX, xmlschema, elementpath | 3.7, 4.3.2, 5.1.4 | — | `import …; __version__` in the venv |
| GNU c++filt | 2.46 | — | `c++filt --version` |

## Runs

| Run | Command | Result | Raw |
|---|---|---|---|
| Full suite, tmpfs, first pass | `cargo nextest run -p lpg --no-fail-fast` | exit 0; 581 passed, 29 skipped | — |
| Gates, ext4 and XFS, first pass | `LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs .config/gates.sh` | **exit 1**: fmt, clippy, `--cfg fuzzing` clippy, loom (13 passed), shuttle (4 passed), stress (69 passed) clean; nextest 580 passed, **1 failed**: `suites::kill::kill_and_reopen` on XFS, "the child commits: Disconnected" (see Findings) | [raw](raw/gates-first-run.txt) |
| Full suite, tmpfs, second pass (final tree) | `cargo nextest run -p lpg --no-fail-fast`, nothing else running | **exit 0**; 587 passed, 29 skipped | [raw](raw/nextest-tmpfs.txt) |
| Gates, ext4 and XFS, second pass (final tree) | the same `gates.sh` command, nothing else running; free space sampled every 2 s | **exit 0**: fmt, clippy, `--cfg fuzzing` clippy, loom (13 passed), shuttle (4 passed), stress (69 passed); nextest 587 passed, `suites::kill::kill_and_reopen` passed; XFS used at most 3 376 MiB of 8 GiB | [raw](raw/gates.txt), [df](raw/gates-df-samples.txt) |
| Full suite, ext4 and XFS, rerun | `LPG_LAB_EXT4=… LPG_LAB_XFS=… cargo nextest run -p lpg --no-fail-fast` | exit 0; 581 passed, 29 skipped | [raw](raw/nextest-lab-rerun.txt) |
| Kill suite reruns | 14 more runs: 2 on XFS with the failed run's seeds, 1 with 2.8 GiB free on XFS, 6 kill-only and 5 `suites::` runs on ext4 and XFS in the scratch copy with child stderr visible | all exit 0; no child wrote a panic or an error | [raw](raw/kill-reruns.txt) |
| Fault matrix, full, tmpfs | scratch copy: `LPG_FORCE_FULL=1 cargo nextest run -p lpg -E 'test(=suites::fault::bulk_load) \| test(=suites::fault::fast_load)' --no-capture` | exit 0; counts below | [raw](raw/fault-full-tmpfs.txt) |
| Fault matrix, full, ext4 and XFS | the same filter with `LPG_LAB_EXT4` and `LPG_LAB_XFS`, unmodified tree | exit 0; counts below | [raw](raw/fault-full-lab.txt) |
| Sprint 4 tests with output | `cargo nextest run -p lpg -E 'test(/^ops::bulk::/) \| test(/^types::lexical::/) \| test(/^api::tests::bulk::/) \| …' --no-capture` | exit 0; 75 passed | [raw](raw/sprint4-tests.txt) |
| Fuzz, five targets | `cargo +nightly fuzz run --target-dir /dev/shm/lpg-target/fuzz-s4 <target> fuzz/corpus/<target> -- -runs=1000000 [-fork=N -ignore_timeouts=0 -ignore_ooms=0]` | each exit 0, no artifact; table below | [raw](raw/) `fuzz-*.txt` |
| Coverage, second pass | `CARGO_TARGET_DIR=/dev/shm/lpg-target/cov cargo +nightly llvm-cov nextest -p lpg --branch --no-fail-fast --json` | exit 0; 587 passed, 29 skipped; tables below | [raw](raw/coverage-summary.tsv) |
| Memory | scratch copy: `lab60_*` tests, each alone, plainly and under `heaptrack --record-only` | every accountant peak within its cap; table below | [raw](raw/accountant-peaks.txt), [raw](raw/heaptrack-peaks.tsv) |
| External checks | `graphml_check.py`, `neo4j_import_check.sh` | as predicted; section below | [raw](raw/) |
| LDBC SNB SF 0.003 | datagen image, header rewrite, lpg CSV import in both modes | counts equal; `id` INT64; section below | [raw](raw/ldbc-import-transactional.txt) |
| Licences | `cargo deny check licenses` | exit 0, "licenses ok" | [raw](raw/cargo-deny-licenses.txt) |
| MSRV build | `cargo +1.89 build --target-dir /dev/shm/lpg-target/msrv` | exit 0 | [raw](raw/cargo-1.89-build.txt) |
| Dependency tree | `cargo tree -p lpg -e normal` | 27 crates; see AC #60 | [raw](raw/cargo-tree-lpg-normal.txt) |

### Fault matrix counts

Each sweep runs every crash point in three crash modes, and every failure point of ENOSPC on reserve, create and write and of EIO on write and sync. The oracle accepts a reopened graph that is empty or complete, and requires the integrity check to pass.

| Workload | File system | Crash points | Crash runs (drop-all, keep-random, tear-512) | Failure runs | Result |
|---|---|---|---|---|---|
| BulkLoad | tmpfs | 108 | 108 + 108 + 108 | 191 | pass |
| BulkLoad | ext4 | 108 | 108 + 108 + 108 | 191 | pass |
| BulkLoad | XFS | 108 | 108 + 108 + 108 | 191 | pass |
| FastLoad | tmpfs | 296 | 296 + 296 + 296 | 564 | pass |
| FastLoad | ext4 | 296 | 296 + 296 + 296 | 564 | pass |
| FastLoad | XFS | 296 | 296 + 296 + 296 | 564 | pass |

The five `suites::` reruns repeated both full sweeps on ext4 and XFS five more times with the same counts, all passing ([raw](raw/kill-reruns.txt)).

### Fuzzing

No `-max_len` was set. libFuzzer then sets the length limit from the largest corpus input (the `lim` column), so the mutations can still reach the longest seeds. The slow text readers ran in fork mode instead, with `-ignore_timeouts=0 -ignore_ooms=0`, so a timeout or an out-of-memory also stops the run. Corpora stay in `fuzz/corpus/` (gitignored). The single-use target directory was deleted after the runs.

| Target | Mode | Result | Final coverage | Raw |
|---|---|---|---|---|
| `lexical_parse` | single process | "Done 1000000 runs", exit 0 | cov 3 933, lim 4 096 | [raw](raw/fuzz-lexical_parse.txt) |
| `dump_read` | single process | "Done 1000000 runs", exit 0 | cov 1 580, lim 8 213 | [raw](raw/fuzz-dump_read.txt) |
| `csv_read` | `-fork=2` | "fuzzed for 1023928 iterations", `oom/timeout/crash: 0/0/0`, exit 0 | cov 4 334 | [raw](raw/fuzz-csv_read.txt) |
| `jsonl_read` | `-fork=4` | "fuzzed for 1003667 iterations", `0/0/0`, exit 0 | cov 4 184 | [raw](raw/fuzz-jsonl_read.txt) |
| `graphml_read` | `-fork=3` | "fuzzed for 1020436 iterations", `0/0/0`, exit 0 | cov 4 785 | [raw](raw/fuzz-graphml_read.txt) |

`fuzz/artifacts/<target>/` gained no file in any run.

## Exit conditions of sprint DECISION 16 (timing excluded, plan §1.2)

### 1. Lossless round trips in four formats

**Met.**

* `ops::bulk::tests::formats::two_hundred_generated_graphs_round_trip_through_every_format_and_mode` covers 100 fixed seeds and 100 seeds from `LPG_SEED`, every property kind and mixed-type properties, with NaN, infinities, −0.0, points with NaN coordinates and named and offset zones. Each graph goes through the dump, CSV, JSON Lines and GraphML, by transactional import and by fast load. The result has the same nodes, edges, labels and values, floats bit-equal and NaN for NaN.
* `…a_generated_graph_round_trips_through_every_format_and_mode` passes.
* The six FR-IMP-006 cases pass as named tests (`fr_imp_006_*`).
* `types::lexical::tests::every_generated_value_round_trips_through_its_lexical_form` passes.
* All of them pass in the tmpfs, lab and coverage runs.
* External readers agree: see [External checks](#external-checks-54-59) and [LDBC](#ldbc-snb-sf-0003-item-a-54).

### 2. Scope contents and excluded pairs

**Met.**

* `framework::a_graph_scope_dump_holds_exactly_its_graph`: 9 nodes and 7 edges of the second graph only.
* `framework::a_database_with_two_graphs_an_index_and_a_constraint_round_trips_through_a_dump`.
* `framework::excluded_pairs_and_existing_targets_fail_before_any_file`: 22N11, no file.
* `api::tests::bulk::a_query_result_with_a_node_and_an_integer_column`: GraphML holds the nodes and not the integer; CSV and JSON Lines hold both per row; query to dump and database to CSV fail with 22N11.

Coverage limit: no test exports a query result holding edges or paths to GraphML (see [Coverage](#coverage)).

### 3. Invalid-record report and new identifiers

**Met.**

* `framework::one_invalid_record_aborts_by_default_and_is_skipped_on_request` (both modes).
* `fast::the_invalid_records_of_a_fast_load_are_reported`.
* `audit::abort_reports_the_first_invalid_record_in_input_order_in_both_modes`, `audit::a_record_breaking_two_constraints_is_reported_once`, `audit::skip_mode_keeps_the_same_records_in_both_modes`.
* The round-trip suite asserts that no identifier is reused (FR-IMP-005).
* `audit::a_named_id_column_is_stored_typed_by_id_type`.

* `fast::both_import_modes_report_the_same_invalid_records` (second pass) runs the input of the fast-load test through a transactional import and a fast load. Both give abort 22N05 with one line (`n.csv` record 3), and skip counts (2, 1, 8) with the same eight lines: codes 22N05, 22N77, 22N78, 22N79 and four edge lines.
* `fast::both_import_modes_enforce_edge_uniqueness` (second pass): a uniqueness constraint on an edge type rejects the same edges with 22N79 in both modes.

### 4. Fast load: non-empty graph, writers, backup

**Met.**

* `fast::a_fast_load_into_a_graph_with_one_node_fails_and_changes_nothing`: 51N41.
* `fast::during_a_load_writers_wait_backups_fail_and_older_snapshots_read_the_graph_empty`: the writer waits on the lock; a backup fails with 51N41 and writes nothing; readers see the empty graph.
* `fast::a_load_requested_during_a_backup_waits_for_it`.
* `audit::a_serializable_reader_of_the_graph_before_a_load_cannot_commit_after_it`.

### 5. Crash or ENOSPC at each fast-load step

**Met.** The FastLoad sweep ran every crash point and every ENOSPC and EIO point, in full, on tmpfs, ext4 and XFS ([table](#fault-matrix-counts)). After each run the reopened graph is empty, or complete when the crash came after the install commit's group was durable. A partial graph is never accepted. The oracle `fast_matches_model` compares nodes, labels, properties, edges, adjacency and both indexes with a shadow model, and the integrity check passes. `audit::identifiers_and_the_install_survive_crashes_during_recovery` covers crashes during the recovery replay of the install group.

### 6. Laboratory report, unverified points, throughput

Throughput is deferred to Sprint 6 (UPDATE 56). Points 4, 12 and 13 are handled as plan §1.3 sets out: see [Unverified points](#unverified-points-4-12-and-13).

### Other in-sprint correctness items (plan §1.2)

* **Memory within the cap:** met; see [Memory](#memory).
* **Temporary-file bytes through the storage trait:** met; see [Temporary-file bytes](#temporary-file-bytes).
* **`cargo-llvm-cov` on every importer and exporter branch:** met at branch level, with 80 branch outcomes that no test reaches listed as test gaps; see [Coverage](#coverage).

## Acceptance criteria per task

| Task | Criterion | State | Evidence |
|---|---|---|---|
| #54 | `rustup check` no update; cargo tools equal crates.io; tools listed | met | [Tools](#tools) |
| #54 | 10 000 seeded values cover every type, NaN, ±inf, −0.0, a two-type property; deterministic | met | `test_support::values::tests::the_generator_covers_every_type_and_is_deterministic` passes |
| #54 | `graphml_check.py` on the probes: NetworkX reads all three; the schema rejects the first two, accepts the third | met | [raw](raw/graphml-check-probes.txt) |
| #54 | `neo4j_import_check.sh` on the CSV probe: 2 nodes, 1 relationship, SRIDs 7203 and 4979, NaN, Infinity | met | [raw](raw/neo4j-check-probe.txt) |
| #54 | spilling `Sorter`: bytes written and read non-zero; written = sum of run-file lengths | met | `storage::sort::tests::the_temporary_file_bytes_of_a_spilling_sort_are_counted`: runs 5, written 193 948 = run lengths 193 948, read 193 948 |
| #54 | `cargo deny check licenses`, `cargo +1.89 build`; tree unchanged by #54 | met | [Runs](#runs); #54 added no dependency |
| #56 | 100 000 values: `parse(type_name(v), lexical(v))` bit-equal | met | `every_generated_value_round_trips_through_its_lexical_form`: 50 000 with seed 1 and 50 000 with `LPG_SEED` or a random seed |
| #56 | each TS-21 example row is a unit test | met | 18 `types::lexical::tests::ts21_*` tests pass |
| #56 | `lexical_parse` fuzz, 1 000 000 runs, exit 0 | met | [Fuzzing](#fuzzing) |
| #56 | llvm-cov: 100 % of the functions of the new lexical code; each uncovered region with its reason | **met** (second pass) | 54 of 54 functions. The property closure of `write_edge` (`src/types/lexical.rs:417`), never executed in the first pass, runs in `types::lexical::tests::an_edge_record_form_holds_its_properties`. Each of the 40 uncovered regions of `src/types/lexical.rs` has its reason ([raw](raw/coverage-uncovered-regions.tsv)) |
| #56 | deny, MSRV, nextest exit 0 | met | [Runs](#runs) |
| #57 | two graphs, an index and a constraint: dump round trip | met | `framework::a_database_with_two_graphs_an_index_and_a_constraint_round_trips_through_a_dump` |
| #57 | graph-scope dump exact; query→dump and database→CSV fail, no file | met | `framework::a_graph_scope_dump_holds_exactly_its_graph`, `framework::excluded_pairs_and_existing_targets_fail_before_any_file`, `api::tests::bulk::a_query_result_with_a_node_and_an_integer_column` |
| #57 | one invalid record: abort adds nothing, skip imports the rest, report names it | met | `framework::one_invalid_record_aborts_by_default_and_is_skipped_on_request` |
| #57 | dump truncated at each block boundary or with a flipped byte per block fails, adds nothing | met | `framework::a_damaged_dump_fails_and_adds_nothing` |
| #57 | import beyond the cap fails with a GQLSTATUS error, graph unchanged; peak ≤ cap; heaptrack in the report | met | `framework::an_import_beyond_the_memory_cap_fails_and_changes_nothing`: peak 8 388 096 ≤ 8 388 608; heaptrack in [Memory](#memory) |
| #57 | ENOSPC at each write of an export: fails, no target file | met | `framework::enospc_at_each_write_of_an_export_fails_it_and_leaves_no_file` |
| #57 | `dump_read` fuzz, 1 000 000 runs; llvm-cov lists every uncovered region of `src/ops/bulk/` with its reason | **met** (second pass) | Fuzz: met. Each of the 745 uncovered regions of `src/ops/bulk/` has a category and a reason: inherited from the uncovered branch outcome or never-executed function it lies in (named in the `inherits` column), or written for that region ([raw](raw/coverage-uncovered-regions.tsv); see [Coverage](#coverage)) |
| #58 | load into a one-node graph fails; empty graph with uniqueness, existence and index: equals input, index = scan, integrity | met | `fast::a_fast_load_into_a_graph_with_one_node_fails_and_changes_nothing`, `fast::a_fast_load_with_constraints_and_indexes_equals_the_input` |
| #58 | duplicate id, unknown and skipped endpoints, each constraint violation reported; abort adds nothing; skip loads the rest | met | `fast::the_invalid_records_of_a_fast_load_are_reported`, `fast::both_import_modes_report_the_same_invalid_records`, `fast::both_import_modes_enforce_edge_uniqueness` (skip: 3 edges, 2 lines 22N79; abort: 22N79, graph empty; same in both modes) |
| #58 | paused load: writer waits, backup 51N41 writes nothing, reader sees empty; a load during a backup starts after it | met | `fast::during_a_load_writers_wait_backups_fail_and_older_snapshots_read_the_graph_empty`, `fast::a_load_requested_during_a_backup_waits_for_it` |
| #58 | snapshot before the install reads empty after it; one after reads all | met | the same test; `audit::the_install_batch_hides_the_loaded_trees_from_older_snapshots`, `audit::an_index_scan_from_a_snapshot_older_than_the_load_returns_nothing` |
| #58 | fault sweep: every crash and ENOSPC point of `FastLoad`; empty or complete; counts in the report | met | [Fault matrix counts](#fault-matrix-counts) |
| #58 | the sort's reservation never exceeds the cap; temporary-file bytes in the report | met | [Temporary-file bytes](#temporary-file-bytes) |
| #58 | llvm-cov lists every uncovered region of the load code with its reason | **met** (second pass) | each of the 138 uncovered regions of `src/ops/bulk/fast.rs` has a category and a reason ([raw](raw/coverage-uncovered-regions.tsv), `src/ops/bulk/fast.rs` rows) |
| #59 | 200 graphs × four formats × two modes, same content, bit-equal floats | met | `formats::two_hundred_generated_graphs_round_trip_through_every_format_and_mode` |
| #59 | six FR-IMP-006 cases as named tests | met | `formats::fr_imp_006_*` (6) |
| #59 | query result with a node and an integer column | met | `api::tests::bulk::a_query_result_with_a_node_and_an_integer_column` |
| #59 | Neo4j imports lpg's CSV export and returns its values; NetworkX reads the GraphML export; schema result as item B predicts | met | [External checks](#external-checks-54-59) |
| #59 | each fuzz target, 1 000 000 runs, exit 0 | met | [Fuzzing](#fuzzing) |
| #59 | llvm-cov lists every uncovered branch of the three formats with its reason; deny and MSRV | met | [raw](raw/coverage-uncovered-branches.tsv) (`csv.rs`, `jsonl.rs`, `graphml.rs` rows); [Runs](#runs) |
| #60 | one section per exit condition, each citing a test or run and its result | met | this report |
| #60 | all suites and fuzz runs exit 0 on the final commit; commit named | **met** (second pass) | All suites exit 0 on the final tree (`bcd5085` plus the second-pass test changes, to be committed): the tmpfs suite and `gates.sh` with ext4 and XFS, run with nothing else running ([Runs](#runs)); fuzz exit 0 at `bcd5085` (the second pass changed no reader). The first pass's gates run failed once in `suites::kill::kill_and_reopen` (XFS); the lab rerun and 14 kill-suite reruns passed (see Findings) |
| #60 | coverage table lists every importer and exporter branch as covered or with its reason | met | [Coverage](#coverage) |
| #60 | accountant's peak never exceeds the cap; heaptrack peaks in the report | met | [Memory](#memory) |
| #60 | `quality-and-testing.md` states points 4, 12, 13 | not done here | left to the technical-writer by the coordinator; inputs in [Unverified points](#unverified-points-4-12-and-13) |
| #60 | `cargo tree -p lpg -e normal` lists no crate outside TS-31 | **not met as written** | Every crate Sprint 4 added is in TS-31: csv, csv-core, itoa, memchr, quick-xml, ryu, serde_core, serde_json, zmij. Nine transitive crates present before Sprint 4 are not named in the TS-31 tables: bitflags and linux-raw-sys (from rustix), cfg-if, chacha20, cpufeatures, getrandom, libc and rand_core (from rand), jiff-core (from jiff) |

## Coverage

`cargo +nightly llvm-cov nextest -p lpg --branch` over the whole suite, second pass (587 tests). Functions count closures separately; a branch is counted per outcome ([raw](raw/coverage-summary.tsv)). First-pass values are in parentheses where they changed.

| File | Functions | Regions | Branch outcomes | Lines |
|---|---|---|---|---|
| `src/ops/bulk/csv.rs` | 72/78 (65) | 741/836 (677) | 108/130 (104) | 612/658 (567) |
| `src/ops/bulk/dump.rs` | 45/57 | 537/646 | 60/80 | 467/516 |
| `src/ops/bulk/export.rs` | 42/52 | 378/489 | 8/20 | 318/371 |
| `src/ops/bulk/fast.rs` | 43/44 | 769/907 (757) | 114/136 (109) | 669/717 (662) |
| `src/ops/bulk/files.rs` | 15/16 | 120/136 | 22/34 | 115/124 |
| `src/ops/bulk/graphml.rs` | 50/58 (49) | 545/656 (527) | 58/78 (52) | 474/534 (465) |
| `src/ops/bulk/import.rs` | 35/43 (34) | 449/557 (429) | 53/74 (49) | 402/466 (380) |
| `src/ops/bulk/jsonl.rs` | 41/44 | 319/366 | 27/32 | 261/300 |
| `src/ops/bulk/mod.rs` | 10/10 | 81/86 | 14/20 | 107/110 |
| `src/ops/bulk/model.rs` | 31/32 | 176/187 | 14/16 (13) | 183/192 |
| `src/types/lexical.rs` | 54/54 (53) | 649/689 (647) | 122/154 | 516/533 (515) |

The other new code in `src/types/` was moved out of `#[cfg(test)]`: `Point::lexical` and `Point::parse`, and `temporal::format`. The lexical tests reach it; it was not tabled separately.

The three lists below take a region, branch outcome or function as uncovered only when **no** instantiation of its function runs it. llvm-cov's file summary counts the code of some functions with several instantiations once per instantiation, so its totals are a little higher: 174 uncovered branch outcomes and 791 uncovered regions, against 170 and 785 in the lists. The difference is at `lexical.rs:384`, `387`, `764` and `dump.rs:492` (branches), and in `parse_document` (`graphml.rs`) and `each_line` (`jsonl.rs`) (regions), each recorded twice or three times in the coverage data.

**Uncovered branch outcomes: 170**, on 159 conditions. Each condition, with its file, line, the outcome not taken (`true`, `false` or `both`), its category and its reason, is in [raw/coverage-uncovered-branches.tsv](raw/coverage-uncovered-branches.tsv). The first pass's table counted conditions, not outcomes; this one counts outcomes.

| Category | Meaning | csv | dump | export | fast | files | graphml | import | jsonl | mod | model | lexical | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| test gap | valid input or behaviour no test exercises | 7 | 2 | 10 | 15 | 4 | 15 | 16 | 3 | 0 | 1 | 7 | 80 |
| malformed input | a refusal of malformed input no test feeds. The fuzz targets reach the readers, but fuzz runs are not instrumented | 13 | 14 | 0 | 0 | 0 | 2 | 0 | 2 | 0 | 0 | 14 | 45 |
| defensive | a check of an internal invariant, unreachable while it holds | 1 | 2 | 2 | 7 | 3 | 3 | 2 | 0 | 0 | 1 | 2 | 23 |
| invalid request | a refused request argument no test passes | 0 | 0 | 0 | 0 | 5 | 0 | 3 | 0 | 6 | 0 | 0 | 14 |
| test helper | the false outcome of `bit_eq`, reached only by a failing round trip | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 7 | 7 |
| fuzz entry | the fuzz-only `read_all` entry | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |

Eleven conditions of the first pass are now covered: the Neo4j header options and geographic point maps (`csv.rs:430`, `csv.rs:540`), the fast load's edge uniqueness (`fast.rs:675`, `709`, `711`, `723`), the GraphML default (`graphml.rs:741`), the transactional import's constraint violations, skipped endpoints and edge uniqueness (`import.rs:298`, `341`, `368`), and `model.rs:397`. Six more are now covered on one outcome (`csv.rs:382`, `graphml.rs:746` to `750`); their reasons were rewritten for the outcome left.

**Never-executed functions: 50** (closures included), each with its category and reason in [raw/coverage-uncovered-functions.tsv](raw/coverage-uncovered-functions.tsv): 21 malformed input, 11 defensive, 8 test gap, 4 test helper (`dump::Memory`, a read-only file), 2 I/O error, 2 invalid request, 2 fuzz entry.

**Test gaps of most consequence:**

* **Neo4j point header options and point maps** (`csv.rs` `options`, `neo4j_point`). **Covered** (second pass): `formats::the_neo4j_point_probe_imports_in_both_modes`. Still uncovered: maps without `srid` or `crs`, the `crs` key, and non-finite coordinates (`csv.rs:533` to `558`).
* **Query-result GraphML exports holding edges or paths, and query-result CSV with edge columns or NULL cells** (`export.rs`, `graphml.rs`, `csv.rs`). Not covered; not checked by this pass.
* **Transactional import with constraint violations, unknown or skipped endpoints, or edge uniqueness** (`import.rs`). **Covered** (second pass): `fast::both_import_modes_report_the_same_invalid_records`, `fast::both_import_modes_enforce_edge_uniqueness`. Still uncovered: an edge failing an existence or type constraint (`import.rs:364`).
* **Edge constraints in the fast load** (`fast.rs`). Uniqueness **covered** (second pass) by `fast::both_import_modes_enforce_edge_uniqueness`. Still uncovered: an edge failing an existence or type constraint (`fast.rs:704`).
* **GraphML `<default>`** (`graphml.rs`). **Covered** (second pass): `formats::a_graphml_default_applies_to_nodes_without_the_data_in_both_modes` (default 1.5 for the node without data, 2.5 kept on the other, both modes). Still uncovered: defaults on the `labels` and `label` keys (`graphml.rs:746` to `752`).
* **Fast load refused by a POPULATING index; database-import refusals other than "another object"; inputs over the key and record size limits; nodes without an input id; JSON numbers above `i64::MAX`.** Not covered; not checked by this pass.

**Regions.** 785 regions are uncovered. Each row of [raw/coverage-uncovered-regions.tsv](raw/coverage-uncovered-regions.tsv) gives the region's position and text, its category and its reason, and in `inherits` the item it takes them from:

* 207 lie in an uncovered branch outcome and inherit its category and reason (`inherits` names the branch);
* 122 lie in a never-executed function and inherit its category and reason (`inherits` names the function);
* 456 have a reason written for them (`inherits` is `-`). Most are `?` error returns and match arms that no branch record covers: Rust's branch coverage instruments `if` conditions, not `?` or `match`.

Two categories appear only for regions: **I/O error**, a storage or file error no instrumented test injects at that call, and **memory cap**, a reservation beyond the cap no test reaches at that call. The coverage run executes the reduced tmpfs fault sweep; the full fault matrix ran outside coverage, so an I/O-error region may still be reached by the full sweep, which is not measured here. Several reasons rest on a measured property of the code: the export and report writers buffer 64 KiB, and every test export is smaller, so an injected ENOSPC fails at the commit's flush, never at an earlier write.

| Category | csv | dump | export | fast | files | graphml | import | jsonl | mod | model | lexical | Total |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| test gap | 32 | 9 | 51 | 39 | 7 | 63 | 59 | 8 | 0 | 5 | 6 | 279 |
| malformed input | 34 | 63 | 0 | 2 | 0 | 28 | 19 | 29 | 0 | 0 | 30 | 205 |
| I/O error | 16 | 18 | 22 | 65 | 3 | 9 | 11 | 5 | 0 | 4 | 0 | 153 |
| memory cap | 1 | 1 | 0 | 19 | 0 | 1 | 5 | 0 | 0 | 1 | 0 | 28 |
| defensive | 6 | 3 | 38 | 13 | 2 | 5 | 2 | 4 | 0 | 0 | 2 | 75 |
| invalid request | 0 | 0 | 0 | 0 | 4 | 0 | 12 | 0 | 5 | 1 | 0 | 22 |
| test helper | 0 | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | 14 |
| fuzz entry | 6 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 9 |
| Total | 95 | 109 | 111 | 138 | 16 | 106 | 108 | 46 | 5 | 11 | 40 | 785 |

## Memory

The accountant's peak comes from the crate's tests, the only callers of `MemoryAccountant::peak` (`pub(crate)`). The scratch tests `lab60_*` measure one operation each in a fresh engine and process:

* the data is one generated graph: `property_graph(60, 3 000, 6 000, mixed)`;
* `lab60_setup` builds it and its four exports first;
* the cap is 32 MiB (33 554 432);
* before the operation, every engine's peak is 854 016 bytes.

The heaptrack peak is the process's peak heap: test harness, engine, test input and the operation ([raw](raw/accountant-peaks.txt), [raw](raw/heaptrack-peaks.tsv)).

| Operation | Accountant peak (bytes) | Cap (bytes) | heaptrack peak heap |
|---|---|---|---|
| Transactional import, CSV | 9 455 899 | 33 554 432 | 17.91 M |
| Transactional import, JSON Lines | 9 455 899 | 33 554 432 | 17.91 M |
| Transactional import, GraphML | 9 455 899 | 33 554 432 | 17.91 M |
| Transactional import, dump | 9 456 177 | 33 554 432 | 17.92 M |
| Fast load, CSV | 3 476 197 | 33 554 432 | 3.59 M |
| Fast load, JSON Lines | 3 476 197 | 33 554 432 | 3.59 M |
| Fast load, GraphML | 3 476 197 | 33 554 432 | 3.59 M |
| Fast load, dump | 3 476 475 | 33 554 432 | 3.59 M |
| Export, CSV (1 724 716 bytes written) | 1 591 296 | 33 554 432 | 2.37 M |
| Export, JSON Lines (4 997 904 bytes) | 1 591 296 | 33 554 432 | 2.24 M |
| Export, GraphML (4 300 934 bytes) | 1 591 296 | 33 554 432 | 2.29 M |
| Export, dump (756 215 bytes) | 1 591 296 | 33 554 432 | 2.19 M |
| `framework::an_import_beyond_the_memory_cap_fails_and_changes_nothing` (fails with 51N72 as designed) | 8 388 096 | 8 388 608 | 25.62 M |
| `fast::a_fast_load_with_constraints_and_indexes_equals_the_input` | 4 000 475 | 33 554 432 | 6.19 M |
| `fast::a_spilling_fast_load_stays_within_the_cap_and_counts_its_temporary_bytes` | 871 141 | 8 388 608 | 18.16 M |
| `audit::a_large_fast_load_with_a_uniqueness_constraint_fits_a_small_cap` | ≤ 8 388 608 (asserted) | 8 388 608 | 3.61 M |
| `audit::a_dump_block_beyond_the_cap_fails_with_51n72` | ≤ 8 388 608 (asserted) | 8 388 608 | 67.44 M |

Every accountant peak is within its cap. Every export reserves the same 737 280 bytes above the opening peak, whatever the format. Three heap peaks exceed their test's cap. heaptrack's peak consumers (`heaptrack_print --print-peaks`) attribute each excess to the test, not to the code under test:

* 25.24 M of the 25.62 M is the 12.8 MB input string the test builds (`framework.rs:375`);
* 16.78 M of the 18.16 M is `FaultStorage`'s in-memory model of the WAL segment the engine zero-fills at creation (`wal.rs:1034`);
* 33.55 M of the 67.44 M is the test writing its own oversized 16 MiB dump block through `DumpWriter` (`audit.rs:493`).

## Temporary-file bytes

`FaultStorage::bytes` counters, task #54:

| Test | Written | Read | Other |
|---|---|---|---|
| `storage::sort::tests::the_temporary_file_bytes_of_a_spilling_sort_are_counted` | 193 948 | 193 948 | 5 runs; sum of run-file lengths 193 948 |
| `ops::bulk::tests::fast::a_spilling_fast_load_stays_within_the_cap_and_counts_its_temporary_bytes` (3 000 nodes, 3 000 edges, 8 MiB cap) | 2 033 684 | 2 033 684 | accountant peak 871 141; no `sort-temp-*` file left |

Three runs of the fast-load test at `bcd5085` gave the same counts. The 2 016 065 bytes of the rmp #58 TEST entry were measured before the fix passes, which added the input ordinal to each record (DECISION 462).

## External checks (#54, #59)

| Check | Command | Result | Raw |
|---|---|---|---|
| GraphML probes | `/tmp/lpg-lab-venv/bin/python -I docs/evidence/lab-sprint-4/graphml_check.py --probes DIR` | `t`, `t2`, `t3`: NetworkX reads each (2 nodes, 1 edge). `graphml.xsd`: `t` and `t2` invalid (`lpg:type`), `t3` valid. `graphml-lpg.xsd`: `t` invalid (`labels` attribute), `t2` and `t3` valid | [raw](raw/graphml-check-probes.txt) |
| GraphML of lpg's export | `graphml_check.py` on `graph.graphml` from `write_samples_for_the_external_checks` (generated graph, every property type, mixed properties) | NetworkX reads 40 nodes and 60 edges; `graphml.xsd` invalid (`lpg:type`); `graphml-lpg.xsd` valid, as item B predicts | [raw](raw/graphml-check-export.txt) |
| Neo4j CSV probe | `neo4j_import_check.sh --probe DIR` | "IMPORT DONE", 2 nodes, 1 relationship. `cypher-shell`: `point({srid:7203, x:-1.0, y:0.25})` and `point({srid:7203, x:1.5, y:2.0})`, srid3 4979 on both, `Infinity` and `NaN` | [raw](raw/neo4j-check-probe.txt) |
| Neo4j on lpg's CSV export | `neo4j_import_check.sh DIR nodes.csv edges.csv QUERY` on the `neo4j` sample (no `lpg:` column) | 40 nodes, 60 relationships imported. Points (SRIDs 9157 and 7203, `z: -0.0`), doubles, `long` up to 9223372036854775807, dates, local times, local date-times, zoned times, date-times with `[UTC]` and `long[]` are returned. They equal the exported cells in Neo4j's own notation. Edge counts of non-empty point, date-time and double cells: 16, 38, 30, equal to the file's | [raw](raw/neo4j-check-export.txt), [expected](raw/neo4j-check-export-expected.txt) |
| Neo4j `point[]` with NaN | `neo4j_import_check.sh --probe-point-list DIR` | exit 70 as expected: "Cannot create a point with non-finite coordinate values: [NaN, 4.0]" at line 3; reproduces FINDING 467 (DECISION 448 stands) | [raw](raw/neo4j-check-point-list.txt) |

## LDBC SNB SF 0.003 (item A, #54)

* **Generation.** The datagen image `ldbc/datagen-standalone:0.5.1-19-829b7a04-2.12_spark3.2` was run with `--format csv --scale-factor 0.003 --mode bi --explode-edges --epoch-millis --format-options header=true,delimiter='|'` into `/tmp/ldbc-sf0003`. Run as the host user, Spark and Hadoop fail: "basedir must be absolute: ?/.ivy2/local", then a `KerberosAuthException` for the user that has no passwd entry. It was therefore run as the container's root, with `chown` of the output back to the host user before exit. The image's `run.py` offers only the modes `raw` and `bi`. The `initial_snapshot` of the `bi` mode was used.
* **Neo4j layout.** Spark datagen writes the `composite-projected-fk` layout. Its column names and order differ from the Hadoop layout that ldbc-snb-impls @ `11db98c` `cypher/scripts/headers.txt` expects; for example `creationDate` comes first, and `Person1Id|Person2Id` stand where the Hadoop layout has `id` and `:START_ID`. [`ldbc-convert.py`](raw/ldbc-convert.py) rewrites only each file's header line and copies the data bytes unchanged. The new headers keep headers.txt's names, types and id spaces, with `language` as `speaks:STRING[]`. Country, City → Place and Company, University → Organisation. Labels and relationship types per file come from `import-to-neo4j.sh`. No field holds `"` or `\`, so no requoting was needed. 31 files: 29 657 node rows and 42 623 edge rows, counted with Python's `csv` reader ([manifest](raw/ldbc-manifest.tsv)).
* **Import.** [`ldbc-driver-main.rs`](raw/ldbc-driver-main.rs) is a standalone crate on lpg's public API (`Session::import`). It uses CSV, the `|` delimiter, labels and edge type per file, id spaces and `id_type` = integer, into `/tmp/lpgram/<run>/`.

| Mode | Summary | Graph nodes / edges | Labels and types | `id` property |
|---|---|---|---|---|
| Transactional | 29 657 nodes, 42 623 edges, 0 invalid, report empty | 29 657 / 42 623 = the files | each of 9 labels and 15 types equals its file sum | 29 657 values, all INT64 |
| Fast load | the same | the same | the same | the same |

Person 14 has the properties of its row (`Hossein`, `creationDate` 1262531431499, `speaks` `[fa, ku, en]`). It has 3 outgoing KNOWS edges, the count of its rows in the file ([raw](raw/ldbc-import-transactional.txt), [raw](raw/ldbc-import-fast.txt)). The generated data and both graph directories were deleted after the checks.

## Unverified points 4, 12 and 13

As plan §1.3 decides:

* **Point 4, correctness part: closed.**
  * `formats::readers_during_a_load_of_a_three_level_graph_see_it_empty_then_whole`: 3 000 nodes of 1 KB, node tree of three or more levels. Three readers during the paused load read 0; a snapshot older than the install reads 0 after it; a new one reads 3 000; the integrity check passes.
  * The visibility-window audit tests confirm the hiding rule at every install step (`audit::the_install_batch_hides_the_loaded_trees_from_older_snapshots`, `audit::an_index_scan_from_a_snapshot_older_than_the_load_returns_nothing`).
  * Open: the committer, GC and index-builder parts are not Sprint 4 scope. Concurrency at full scale is Sprint 6.
* **Point 12: closed on lpg's real files.**
  * Neo4j 2026.09.0 imports lpg's CSV export, with the point form that has a space after each colon, and returns its point, temporal and numeric values.
  * NetworkX 3.7 reads lpg's GraphML export.
  * The GraphML 1.0 XSD rejects the export and lpg's extension XSD accepts it.
  * Neo4j refuses non-finite coordinates in `point` and `point[]` columns, which DECISIONs 445 and 448 handle.
  * Evidence: [External checks](#external-checks-54-59).
* **Point 13: no correctness part.** Every limit concerns the throughput measurement. The whole point moves to Sprint 6.

## Findings

* **Intermittent kill-suite failure, cause unknown.** In the first pass's gates run, `suites::kill::kill_and_reopen` failed on XFS with "the child commits: Disconnected" (`src/suites/kill.rs:543`). A child process ended before it printed its first commit. The fuzz campaigns, the LDBC datagen and the loom and shuttle builds were running at the same time.
  * The test discards the child's stderr (`src/test_support/child.rs:62`, `Stdio::null()`), so the child's last message is lost.
  * 14 reruns passed. They include both seeds of the failed run on XFS, a run with only 2.8 GiB free on XFS, and 11 runs with the child's stderr inherited in the scratch copy; no child wrote a panic or an error.
  * Not reproduced; the cause is unverified. A full XFS file system is not excluded: no space was sampled during the failed run.
  * Second pass: the child's stderr is now captured in memory, never in a file. `child::spawn` pipes it and a thread drains it into a buffer while the caller reads stdout, so neither pipe can fill and block the child (the `Stdio::piped` deadlock warning of the std documentation; `Child::wait_with_output` reads both pipes at once for the same reason). Turso's `testing/sqltest/src/backends/cli.rs` pipes stderr the same way and puts it in its error message. `kill_and_reopen` now prints the child's stderr with "the child commits", a failed filler step, or unreadable child output.
  * Second pass result: `gates.sh` with ext4 and XFS, on the final tree with nothing else running, exited 0; `kill_and_reopen` passed in 85 s. Free space was sampled every 2 s: XFS used at most 3 376 MiB of its 8 GiB, so it was never full during this run ([raw](raw/gates.txt), [df](raw/gates-df-samples.txt)). The first-pass failure was not reproduced, and its cause stays unverified; a recurrence would now show the child's last messages.
* **#56 function coverage 53 of 54 in the first pass; 54 of 54 in the second.** `an_edge_record_form_holds_its_properties` executes the property closure of `write_edge` (see AC table).
* **80 uncovered branch outcomes are test gaps** (see Coverage). The checks this pass promoted to committed tests found no defect in the five behaviours they cover.
* **TS-31 does not name nine transitive crates** of rand, rustix and jiff that were in the tree before Sprint 4 (see AC #60). This is a specification gap for the technical-writer, not a dependency change.
* **LDBC SNB datagen 0.5.1 (Spark) output is not in the Hadoop layout** that ldbc-snb-impls `headers.txt` describes. A header rewrite is needed (see LDBC); its data needs no other change for lpg's Neo4j-subset importer.

No defect was found in product code, so no product file changed.

## Not run, and limits

* `quality-and-testing.md` was not edited: the coordinator assigned it to another agent.
* The fuzz campaigns ran at `bcd5085` only; the second pass changed tests, not readers.
* Query-result exports were not measured for the accountant's peak; only graph-scope exports were.
* The XFS lab file system is a loop device of 8 GiB. Every XFS run fitted.

## Deferred to Sprint 6 (timing, UPDATE 56)

* Fast-load throughput against M-LAYOUT's 1.88 M edges/s.
* `fio` sequential read and write ceilings on the lab devices; `iostat -x` and `pidstat -d` fractions of those ceilings for import, export and fast load.
* LDBC and M-LAYOUT runs at the sizes where the cache holds 1 % and 10 % of the data (NFR-PERF-005).
* Unverified point 13 as a whole, and the full-scale concurrency part of point 4.

## RAM space

| When | `/dev/shm` | `/tmp` | `/tmp/lpgram` |
|---|---|---|---|
| Before the pass | 980 M (`/dev/shm/lpg-target`) | 1.7 G | 0 |
| Largest single-use build directories | `scratch-copy` 2.5 G, `cov` 2.3 G, `fuzz-s4` 492 M, `ldbc-driver` 119 M, `msrv` (deleted after its build) | — | ≤ 81 M per LDBC run; 36.4 MiB peak in the tmpfs full sweeps |
| After the pass | 1.3 G | 1.7 G | 0 |

Kept in `/dev/shm/lpg-target`, because they are the builds in use: `debug` (1.1 G), `clippy` (198 M, kept by `gates.sh`) and an editor's `flycheck0` (36 K). Kept in `/tmp`, because the external checks use them: `/tmp/lpg-lab-venv` (20 M) and `/tmp/lpg-graphml-xsd` (144 K). Deleted:

* the five single-use build directories (fuzz, coverage, MSRV, scratch copy, LDBC driver);
* the coverage JSON and the heaptrack recordings;
* every `/tmp/lpgram/<run>`;
* the LDBC data `/tmp/ldbc-sf0003`;
* the scratch copy.

The lab directories `/data/tmp/lpg-lab` and `/mnt/lpg-xfs` are empty.

Second pass (`df -h`, `du -sh`):

| When | `/dev/shm` | `/tmp` | `/tmp/lpgram` |
|---|---|---|---|
| Before the second pass | 1.3 G (`debug` 1.1 G, `clippy` 198 M, `flycheck0` 36 K) | 1.7 G | 0 |
| Largest single-use build directory | `cov` 2.3 G, deleted after the coverage run with its 7.1 MB JSON | — | 0 after each run |
| After the second pass | 2.4 G (`debug` 2.1 G, `clippy` 281 M) | 1.7 G | 0 |

Kept: `debug` and `clippy`, the builds in use. `debug` grew by 1.0 G with the incremental artefacts of the rebuilt test binary. The editor's `flycheck0` was deleted. `gates.sh` deleted its own single-use directories (`cfgfuzz`, `loom`, `shuttle`). `/tmp/lpgram`, `/data/tmp/lpg-lab` and `/mnt/lpg-xfs` are empty.
