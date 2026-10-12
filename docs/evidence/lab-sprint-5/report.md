---
title: Sprint 5 correctness laboratory
status: living record
last-reviewed: 2026-10-12
related:
  - ../planning/sprint-5-plan.md
  - ../planning/sprint-5-reuse-map.md
  - ../lab-sprint-4/report.md
---

# Sprint 5 correctness laboratory

This report records the Sprint 5 laboratory (rmp #61) and the algorithm runtime with its pilot entry, PageRank (rmp #63). The runs used branch `feature/5-graph-algorithm-library-all-97-inventory-entries` at `9a85eed` plus the uncommitted changes of #61 and #63, on 2026-10-12. Sprint 5 verifies correctness only (sprint UPDATE 57). No timing was measured, and durations printed by tools are not performance claims.

Graph data lived in `/tmp/lpgram` (tmpfs). No durability or I/O claim rests on these runs.

## Environment

| Item | Value | Source |
|---|---|---|
| CPU | AMD Ryzen 9 5900HX, 16 logical CPUs | `lscpu` |
| Memory | 30 GiB | `free -g` |
| Operating system | Ubuntu 26.04.1 LTS, kernel 7.0.0-38-generic | `/etc/os-release`, `uname -r` |
| `/tmp/lpgram` | tmpfs | `df -h /tmp` |
| Build output | `/dev/shm/lpg-target` (tmpfs) | `.cargo/config.toml` |

## Tools

The crates.io versions come from `https://crates.io/api/v1/crates/<name>` (`max_stable_version`), queried on 2026-10-12 with the User-Agent `lpg-lab (https://github.com/FlavioCFOliveira/lpg)`.

| Tool | Version | crates.io | Command |
|---|---|---|---|
| rustup | 1.29.1 (d95a37b6a 2026-08-13) | — | `rustup self update` ("rustup unchanged") |
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | — | `rustup update stable nightly` (unchanged) |
| Rust nightly | 1.101.0-nightly (080cc2714 2026-10-11) | — | same (updated from 6f5da406b 2026-10-10) |
| Rust MSRV | 1.89.0 (29483883e 2025-08-04) | — | `rustc +1.89 --version` |
| `rustup check` | every toolchain and rustup "up to date" (after the update) | — | `rustup check` |
| cargo-nextest | 0.9.148 (cd1d6d546 2026-10-07) | 0.9.148 | `cargo nextest --version` |
| cargo-deny | 0.20.2 | 0.20.2 | `cargo deny --version` |
| cargo-llvm-cov | 0.9.1 | 0.9.1 | `cargo llvm-cov --version` |
| cargo-fuzz | 0.13.2 | 0.13.2 | `cargo fuzz --version` |
| inferno | 0.12.8 (not run: Sprint 6) | 0.12.8 | `cargo install --list` |
| heaptrack | 1.5.0 | — | `heaptrack --version` |
| perf | 7.0.14 (not run: Sprint 6) | — | `perf --version` |
| Docker | 27.2.0 | — | `docker --version` |
| Neo4j and GDS | Neo4j 2026.09.0 community, image `dd2aca3682c1`; GDS 2026.09.0 bundled | — | `docker images` |
| Python | 3.14.4 (system); venv `/tmp/lpg-lab-venv` | — | `python3 --version` |
| NetworkX, numpy, scipy | 3.7, 2.5.4, 1.18.1 | — | `import …; __version__` in the venv |

## Laboratory instruments (rmp #61)

| Instrument | Where | What it does |
|---|---|---|
| NetworkX oracle | [`nx_oracle.py`](nx_oracle.py) | Reads an oracle graph (`nodes.csv`, `edges.csv`). Calls `networkx.<FUNCTION>(G, **PARAMS)` on a `MultiDiGraph` (a `MultiGraph` with `--undirected`) and prints JSON Lines. Run it with `/tmp/lpg-lab-venv/bin/python -I`. |
| GDS oracle | [`gds_oracle.sh`](gds_oracle.sh) | Imports the same files into `neo4j:2026.09.0-community` with `neo4j-admin database import full`. Installs the bundled GDS the way the image entrypoint does for `NEO4J_PLUGINS='["graph-data-science"]'`. Projects graph `g` with an orientation, runs one call, prints JSON Lines and removes the container. GDS is GPL: run only, never copied. |
| Generators | `src/lab/generator.rs` | Seeded (`ChaCha8Rng`) generators: Zipf and uniform graphs, directed or undirected, with labels, edge types, `weight` and a node vector `vec`; DAGs; bipartite graphs; flow networks with `capacity` and `cost`. Karate club and Petersen are copied from rustworkx. One writer (`write_oracle`) and one loader (`load_oracle`, lpg's CSV fast load). |
| Determinism harness | `src/test_support/algo.rs` (`determinism`, `check_cases`) | Runs at `concurrency` 1 to 16, twice each, and compares rows bit for bit. A difference names the `concurrency`, the run and the row. |
| Worker-count probe | `src/algo/sched.rs` (`probe`) | Each worker thread records its thread identifier when it starts. A call's count is the number of distinct threads. Counts are keyed by the database location. |
| Memory | `allocation-counter` (heap peak of a closure on one thread), `MemoryAccountant::peak`, [`heaptrack.sh`](heaptrack.sh) | Heap and accountant peaks. |
| Fixtures | `src/algo/testdata/{karate,zipf300}`; oracle outputs in `src/algo/testdata/expected` | Committed, so `cargo test` needs neither Docker nor Python. `zipf300` is `power_law(5, 300, 1500, zipf, directed)`. |

### Oracle runs

| Run | Command | Result |
|---|---|---|
| NetworkX PageRank, karate | `python -I nx_oracle.py src/algo/testdata/karate pagerank '{}' --undirected` | exit 0; 34 values; `math.fsum` = 0.9999999999999999, \|sum − 1\| = 1.1e-16 (≤ 1e-12) |
| NetworkX, unknown name | `… nx_oracle.py src/algo/testdata/karate no_such_algo` | exit 2, "unknown NetworkX function: no_such_algo" |
| GDS PageRank, karate | `gds_oracle.sh src/algo/testdata/karate gds.pageRank.stream '{concurrency: 4}' '' UNDIRECTED` | exit 0; 34 rows |
| GDS stats, karate | `gds_oracle.sh src/algo/testdata/karate gds.pageRank.stats '{concurrency: 4}' 'YIELD ranIterations, didConverge RETURN {…} AS row' UNDIRECTED` | exit 0; `ranIterations` 20, `didConverge` false |
| GDS, unknown name | `gds_oracle.sh src/algo/testdata/karate gds.noSuchAlgo.stream '{}'` | exit 1; 42N08 "no such procedure" ([raw](raw/gds-unknown-procedure.txt)) |
| GDS PageRank, `zipf300` | `gds_oracle.sh src/algo/testdata/zipf300 gds.pageRank.stream CONFIG [RET] [ORIENTATION]` for the seven cases below; `gds.pageRank.stats` returning `{ranIterations, didConverge}` for four of them | exit 0 each; 300 rows (stream), 1 row (stats); outputs in `src/algo/testdata/expected/zipf300.gds.pagerank.*.jsonl` |

One personalised `gds.pageRank.stream` run printed 300 rows but exited 1, because the script file was edited while it ran. It was run again with the final script: exit 0, and the output was byte-identical (`cmp`). Every container was removed: `docker ps -a --filter ancestor=neo4j:2026.09.0-community` lists none.

## Results (rmp #63)

### PageRank against GDS 2026.09.0 (rule F)

Test `algo::tests::pagerank_is_deterministic_and_matches_gds`. Each case must:

* give bit-equal rows at `concurrency` 1 to 16, twice each;
* start at most `concurrency` workers per run (probe);
* stay within the case's tolerance against GDS, as `|a − b| / max(1, |b|)`.

| Case | Configuration | Largest relative difference | Tolerance |
|---|---|---|---|
| natural | `{}` | 4.96e-16 | 1e-7 |
| converged | `maxIterations: 100` | 4.80e-16 | 1e-7 |
| weighted | `relationshipWeightProperty: "weight"` | 6.87e-16 | 1e-7 |
| personalised | `sourceNodes`: uids 0, 7, 42 | 4.16e-17 | 1e-7 |
| reverse | `orientation: "REVERSE"` | 4.17e-16 | 1e-7 |
| undirected | `orientation: "undirected"` | 6.71e-16 | 1e-7 |
| scaled | `scaler: "L2NORM", dampingFactor: 0.5, tolerance: 1e-4, maxIterations: 7` | 1.11e-16 | 1e-4 |
| karate, `UNDIRECTED` | test `karate_pagerank_matches_the_oracles` | within 1e-7 (passes) | 1e-7 |

### Rounds to convergence against GDS `ranIterations`

lpg's values come from `gds.pageRank.write` in a rolled-back transaction; GDS's from `gds.pageRank.stats`.

| Case | lpg `ranIterations`, `didConverge` | GDS `ranIterations`, `didConverge` |
|---|---|---|
| natural | 20, false | 20, false |
| converged (`maxIterations: 100`) | 79, true | 79, true |
| weighted | 20, false | 20, false |
| scaled (`maxIterations: 7`) | 7, false | 7, false |
| karate, `UNDIRECTED` (GDS only) | — | 20, false |

### Determinism and worker counts

| Check | Test | Result |
|---|---|---|
| The harness passes a block-ordered float sum and fails a per-worker sum, naming `concurrency` and the row | `algo::tests::determinism_harness_detects_order_dependence` | pass |
| The probe reports k for k scoped threads, k = 1, 4, 16 | `algo::tests::worker_count_probe_counts_threads` | pass |
| 2-block projection without `concurrency`: 2 workers (> 1, ≤ 4); `concurrency` 1: 1 worker; 0 and 1 node: 1 worker | `algo::tests::worker_counts_follow_concurrency` | pass |
| `zipf300` (300 nodes, block size 5, 60 blocks), k = 1 to 16: exactly min(k, 60) = k workers | same | pass |
| Bit-equal rows at `concurrency` 1 to 16, twice each, seven cases | `pagerank_is_deterministic_and_matches_gds` | pass |

### Memory

Test `algo::tests::lab_zipf_projection_memory` (ignored; run alone). The graph `power_law(11, 20 000, 200 000, zipf, directed)` is loaded by the CSV fast load into a database with a 64 MiB cap. The test builds a projection of every node and edge, then runs `gds.pageRank.write`.

| Measure | Value | How |
|---|---|---|
| Projection arrays (capacity) | 6 083 584 B | `Projection::bytes` |
| Heap peak of the projection build, statement thread | 18 436 540 B | `allocation-counter` around `projection::build` |
| Bytes the run had reserved when the build ended | 15 575 040 B | the run's `Held` peak |
| Accountant peak before the run / after both runs | 6 957 797 B / 24 842 105 B | `MemoryAccountant::peak` |
| Memory cap | 67 108 864 B | — |
| Heap peak of the whole test | 76.81 MB | `heaptrack.sh <test-binary> algo::tests::lab_zipf_projection_memory 4algo10projection5build` |
| heaptrack peak consumers under `projection::build` | 6.05 MB, 2.89 MB, 1.60 MB, 3.33 KB (merged backtraces; heaptrack states their peaks are approximate) | same |

**Open point (unverified): the heap peak of the build exceeds the run's reservation by 2 861 500 B (about 2.86 MB).** The cause is not established.

* **Indication.** A temporary measurement of the adjacency scan alone gave a heap peak of 14 024 288 B in the first build and 10 041 790 B in the second, on the same graph and the same arrays. The instrumentation was removed afterwards. The 3 982 498 B difference fits buffer-pool frames allocated on the statement thread when pages are first read. The pool reserves those frames from the accountant itself, not through the run. This is an indication, not an attribution.
* **Known uncharged memory.** By design (`src/algo/mem.rs`), the run does not charge two things: the instant of a reallocation, when an array's old buffer is still held beside its new one, and the copy of one scan chunk (4 096 entries).
* **Not shown.** That pool frames plus these two items account for the whole 2.86 MB, so the run may hold heap memory that no reservation covers. Closing this needs a measurement that separates pool-frame allocations from the run's own.

The accountant's peak stayed at or below the cap in every test that checks it: `filters_snapshot_and_memory_cap`, `write_mode_and_signatures` and `lab_zipf_projection_memory`.

## Acceptance criteria

### rmp #61

| Criterion | Evidence | Result |
|---|---|---|
| `rustup check` reports no update; cargo tools at `max_stable_version`; the report lists the tools | [Tools](#tools) | met |
| `nx_oracle.py` on karate: 34 values summing to 1 within 1e-12; `gds_oracle.sh`: 34 rows; both exit non-zero for an unknown name | [Oracle runs](#oracle-runs); `algo::tests::karate_pagerank_matches_the_oracles` re-checks the committed NetworkX output | met |
| The determinism harness passes block order and fails a `concurrency`-dependent order, naming `concurrency` and the row | `determinism_harness_detects_order_dependence` | met |
| The probe reports k for k = 1, 4, 16 | `worker_count_probe_counts_threads` | met |
| Byte-identical generator files per seed; DAGs acyclic; bipartite graphs two-colourable; flow capacities non-negative | `lab::generator::tests::laboratory_graphs_are_reproducible_and_well_formed`, which also checks that the committed fixtures equal the generator's output | met |
| `cargo deny check licenses` and `cargo +1.89 build` exit 0; `cargo tree -p lpg -e normal` unchanged by #61 | both exit 0; #61 added no dependency, and the tree changed only by #63's crates | met |

### rmp #63

| Criterion | Evidence | Result |
|---|---|---|
| A label filter returns only A nodes; a node committed after the snapshot is absent; a projection over the cap fails with a GQLSTATUS error and aborts the transaction | `filters_snapshot_and_memory_cap`: 51N73 with a 1 KiB transaction memory limit, 51N72 with the pool budget exhausted; the next statement fails with 25N05 | met |
| `.write` rolled back leaves no property; `.stream` changes nothing; a write-back over the cap fails | `write_mode_and_signatures`: 20 nodes of 20 KB with 256 KiB left gives 51N72 | met |
| GDS parameters and output columns, no graph-name argument; `.stats` fails | `write_mode_and_signatures`: `.stats` and `.mutate` give 42N08; `('g', $c)` gives 22N11; columns are `Score` and `RankWrite` | met |
| `CALL` and `run_algorithm` give bit-equal rows on one snapshot | `pagerank_is_deterministic_and_matches_gds`: `Transaction::run_algorithm` in the same transaction, `Session::run_algorithm` after the commit | met |
| Worker counts (FR-ALG-006) | `worker_counts_follow_concurrency` | met |
| Zipf rows bit-equal at 1 to 16, twice each; GDS rule F; rounds and `ranIterations` in the report | [PageRank against GDS](#pagerank-against-gds-2026090-rule-f), [Rounds](#rounds-to-convergence-against-gds-raniterations) | met |
| A multigraph projection keeps parallel edges; forward and reverse rows equal the adjacency; petgraph `toposort`, `kosaraju_scc` and `dinics` run on the wrapper | `projection_keeps_the_multigraph_and_runs_petgraph`; `u32` and `usize` indexes also give equal PageRank output | met |
| The accountant's peak stays at or below the cap; accountant and heaptrack peaks in the report | [Memory](#memory); the 2.86 MB gap between heap peak and run reservation is open | met, with the open point |
| `cargo deny check licenses` and `cargo +1.89 build` exit 0; `cargo tree -p lpg -e normal` lists only TS-31 crates | both exit 0; 47 crates besides lpg, each a row of TS-31 (`once_cell`, `pin-project-lite` and `tracing-core` are in the `tracing` row) | met |

## Quality gate

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 607 passed, 31 skipped, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --all -- --check` | clean |
| `cargo deny check licenses` | "licenses ok", exit 0 |
| `cargo +1.89 build` | exit 0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | fails on five doc links in the module text of `src/auth`, `src/gqlstatus` and `src/proc`. The baseline without these tasks' changes fails on the same kind of link. None of the failures is in the files #61 and #63 added. |

## Not run, and limits

* No timing, flame graph or speedup measurement (Sprint 6).
* GDS reports `centralityDistribution` from an HdrHistogram. lpg's percentiles are exact nearest-rank values and were not compared with GDS's.
* A source node outside the projection fails with 22N11. This follows GDS's source code (`SourceNodesConfig.nodesExistInGraph`), not a GDS run.
* Synchronous convergence: PageRank's rounds are GDS's own synchronous supersteps, so equal `ranIterations` says nothing about the other entries.
* The 2.86 MB gap between the build's heap peak and the run's reservation is open and unverified ([Memory](#memory)).
* MAGE was not pulled; no #63 check needs it.

## RAM space

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | the venv and reference clones of the plan; no new file |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 195 M | 5.3 G | the build in use (the dev, test, clippy and 1.89 builds share `debug/`); `doc/` was deleted after the documentation check |
