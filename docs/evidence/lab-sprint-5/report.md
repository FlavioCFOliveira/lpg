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

This report records the Sprint 5 laboratory (rmp #61), the algorithm runtime with its pilot entry, PageRank (rmp #63), the family tasks (rmp #64 to #69), the mechanical review (rmp #71) and the exit check (rmp #70, [last section](#exit-check-rmp-70)). Each section names the commit it ran on; the first sections used branch `feature/5-graph-algorithm-library-all-97-inventory-entries` at `9a85eed` plus the uncommitted changes of #61 and #63, on 2026-10-12. Sprint 5 verifies correctness only (sprint UPDATE 57). No timing was measured, and durations printed by tools are not performance claims.

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

**The heap peak of the build exceeds the run's reservation by 2 861 500 B (about 2.86 MB).** This point was open here; the exit check closed it: the excess is buffer-pool frames, which the pool charges to the accountant itself, and the run's own heap stays within its reservation ([Exit check, condition 5](#condition-5-a-projection-over-the-cap-fails-the-statement-fr-alg-004)). By design (`src/algo/mem.rs`), the run does not charge the instant of a reallocation, when an array's old buffer is still held beside its new one, nor the copy of one scan chunk (4 096 entries); in this measurement both fit within the reservation.

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
| The accountant's peak stays at or below the cap; accountant and heaptrack peaks in the report | [Memory](#memory); the 2.86 MB gap between heap peak and run reservation, open here, is closed in the exit check (pool frames) | met |
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
* The 2.86 MB gap between the build's heap peak and the run's reservation was open here; the exit check attributes it to buffer-pool frames ([Memory](#memory)).
* MAGE was not pulled; no #63 check needs it.

## RAM space

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | the venv and reference clones of the plan; no new file |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 195 M | 5.3 G | the build in use (the dev, test, clippy and 1.89 builds share `debug/`); `doc/` was deleted after the documentation check |

## #64 and #69

Traversal and shortest-path families (rmp #64, 26 entries) and the cycle, isomorphism and derived-structure families (rmp #69, 11 entries), built together on the runtime of #63, at `ef05369` plus uncommitted changes, on 2026-10-12. Correctness only: no timing was measured.

### Runtime extensions (generic, for #65 to #68)

| Extension | Where |
|---|---|
| Row outputs: each row charged before it is stored (`Run::emit`); the run's reservation then shrinks to the rows' share (`Held::shrink_to`) | `src/algo/mod.rs`, `src/query/exec/mod.rs` |
| Write kinds: node property, PathWrite and RelWrite relationships, edge flag (EdgePropWrite); entry-specific summary columns (`containsNegativeCycle`, `bipartite`) | `src/algo/mod.rs` |
| MAGE positional parameters before `configuration`; a second projection (`otherNodeLabels`, `otherRelationshipTypes`); edge types and node labels in the projection; node properties by fixed name (`lat`, `lng`) | `src/algo/mod.rs`, `src/algo/projection.rs` |
| Key types `Node`, `Bool`, `Ints`; the key sets NodeWrite, RelWrite, PathWrite | `src/algo/config.rs` |
| Shared kernels: GDS-order BFS (parallel) and DFS, Dijkstra with predecessors, Kahn levels (parallel), path rows | `src/algo/kernels.rs` |

### Instruments

| Instrument | Change |
|---|---|
| [`mage_oracle.sh`](mage_oracle.sh) (new) | Runs one Cypher query on `memgraph/memgraph-mage:3.8.0`, the first image published after MAGE commit `c72636014a8b` (2026-01-23); the MAGE repository has no tag after `v3.7.2`, so that the image holds this commit's C++ code is unverified. The container is removed after each run. |
| [`nx_oracle.py`](nx_oracle.py) | Dotted names (`bipartite.sets`), `--simple`, `--nodes LABEL`, `--other LABEL`, `--nograph`; graph results printed as edge lists; sets sorted; non-finite floats as strings. |
| [`gds_oracle.sh`](gds_oracle.sh) | Projects the `lat` and `lng` node properties when `nodes.csv` has them. |
| [`oracle-cases/`](oracle-cases/) (new) | The runners that wrote every oracle output of these tasks: 30 GDS runs, 18 MAGE runs, 55 NetworkX runs. |
| Fixtures (`src/lab/generator.rs` `fixtures()`) | `zipf60`, `uniform60`, `dag40`, `negw30` (negative weights by node potentials, no negative cycle), `negcyc30`, `geo30`, `cyc12`, `petersen`, `bip20`, `tour7`; `iso100` from petgraph 0.8.3 `tests/res/graph_100n_100e{,_iso}.txt`. 1.4 MB with the oracle outputs. |

Every GDS and MAGE container was removed (`docker ps -a` lists none of either image). The image `memgraph/memgraph-mage:3.8.0` stays in Docker's storage on disk.

### Oracle results

Tests `algo::tests_paths::*` and `algo::tests_structure::*`. Rule F columns give the largest relative difference `|a − b| / max(1, |b|)` over the case.

| ALG | Oracle (rule) | Graphs | Result |
|---|---|---|---|
| 001 | GDS (V) | `zipf60` (all, `maxDepth` 2, `targetNodes`), `uniform60` | same node sets, depth order; order equal to GDS's only for `maxDepth` 2 |
| 002 | GDS (V) | `zipf60` (all, target), `uniform60` (`maxDepth` 3) | same node sets; order equal for the target case only |
| 003 | MAGE (X) | `zipf60`: `at_hop` ×3, `by_hop` ×2 | equal |
| 004 | MAGE (X) | `cyc12` `pathExpand` ×3; `zipf60` `pathSubgraphNodes` ×3, `pathSubgraphAll` | equal, except the `<T>` case of `pathSubgraphNodes`: equal node sets, 36 rows against MAGE's 34 (open point 2) |
| 006 | Q | `zipf60`, seeds 0 and 7 | walks valid; first steps within 2.98 standard errors (bound 4) over 30 000 walks |
| 007 | GDS (V, X distances) | `dag40` (with and without distances), `zipf60`, `uniform60` | equal node sets and distances; GDS's filler rows left out (open point 3) |
| 008 | NetworkX (X) | `zipf60` (node 26, on a cycle), `uniform60` | equal |
| 009 | NetworkX (X) | `zipf60` (both directions), `uniform60` | equal |
| 011 | NetworkX (X) | `zipf60`, `uniform60` | equal |
| 013 | NetworkX (X lengths, V paths) | `zipf60` (source, target, pair), `uniform60` (all pairs) | equal |
| 014 | GDS (F, V) | `zipf60` (pair, all, unweighted), `uniform60` (targets, all) | 0 |
| 015 | NetworkX (F, V) | `zipf60`, `uniform60` | 0 |
| 018 | GDS (F, V) | `geo30`, two pairs | 0 |
| 019 | GDS (F, V, X flag) | `zipf60`, `uniform60`, `negw30`, `negcyc30` | 0; on `negcyc30` every row is a negative cycle, as GDS's |
| 020 | NetworkX (X existence, V) | `negw30`, `negcyc30` | equal; the cycle's weight is negative |
| 021 | NetworkX (F, V) | `zipf60`, `negw30`; `negcyc30` fails with 22N11 | 0 |
| 023 | GDS (F, V) | `zipf60` (`delta` 0.3), `uniform60` | 0 |
| 024 | NetworkX `floyd_warshall` (F) | `zipf60`, `uniform60`, `negw30` | 3.6e-16, 3.9e-16, 4.4e-16 |
| 025 | NetworkX (F) | `zipf60`, `uniform60`, `negw30` | 2.1e-16, 2.5e-16, 4.3e-16 |
| 026 | NetworkX (X) | `karate`, `cyc12`, `uniform60` undirected | equal |
| 027 | GDS (F) | `zipf60` weighted, `uniform60` unweighted | 0 |
| 028 | NetworkX (X) | `zipf60` (unweighted, weighted), `uniform60` | equal |
| 029 | GDS (F in order, V) | `zipf60`, `uniform60`, `k` 6 | 1.4e-16, 1.5e-16 |
| 030 | petgraph `k_shortest_path` (F) | `zipf60` (with and without target), `uniform60` | equal |
| 031 | NetworkX (X); MAGE | `zipf60` `[">"]`, `cyc12` `[]` | equal to both |
| 032 | GDS (F, V) | `dag40` weighted, `zipf60` | 0 |
| 180 | MAGE (X) | `petersen`, `cyc12` | 57 and 57 cycles, equal |
| 181 | NetworkX (X) | `cyc12`, `dag40` | equal |
| 187 | optimum by enumeration (V) | the 9 `S` points of `geo30`; 73 subsets | 9 points: 1.0854 (2-approx), 1.0854 (1.5-approx), 1.2058 (greedy); subsets: worst 1.3274 and 1.1149 |
| 188 | optimum (Q) | 9 points, 1 and 3 vehicles and the default | valid routes, no improving 2-opt move; ratio 1.0000 |
| 189 | NetworkX (X) | `iso100` P–Q (true), `tour7` T–U (false) | equal |
| 190 | petgraph `is_isomorphic_subgraph` (X) | `iso100`, `tour7` | equal |
| 194 | NetworkX (X, V sides) | `bip20`, `petersen`, `karate` | equal; sides a 2-colouring |
| 203 | NetworkX (X) | `tour7` T and U, `dag40` | equal |
| 204 | NetworkX `is_graphical` (X) | 9 sequences | equal |
| 205, 206 | NetworkX (X) | `dag40`; `cyc12` fails with 22N11 | equal |

### Determinism, worker counts, `CALL` and the Rust API

| Check | Test | Result |
|---|---|---|
| Every stream routine bit-equal at `concurrency` 1 to 16, twice each; 006 with seeds 0 and 7 | `Lab::det`, used for every entry | pass |
| `CALL` rows equal `Transaction::run_algorithm` rows in one transaction | `Lab::det` | pass for every entry |
| #64: the 9 parallel entries start exactly 2 workers on a 2-block projection at `concurrency` 4; the 17 others at most one at 16 | `worker_counts_of_the_family` | pass |
| #69: each of the 11 entries at most one worker at 16 | `worker_counts_of_the_structure_families` | pass |
| Each run starts at most `concurrency` workers (parallel entries) or one (others) | `Lab::det` | pass |

Single-thread entries that call no block function run on the statement's thread: the probe then records no worker thread, which the tests count as one worker.

### Rounds to convergence of 019

lpg's synchronous Bellman-Ford needs `h + 1` rounds, where `h` is the largest number of edges on a shortest path; the reference's `h` is read from NetworkX's paths (`single_source_bellman_ford`). GDS reports no round count.

| Graph | lpg rounds | `h` from NetworkX |
|---|---|---|
| `zipf60` | 7 | 6 |
| `negw30` | 6 | 5 |

### Memory

| Check | Test | Result |
|---|---|---|
| 028 and 031 on a layered graph (4 096 paths): 3 047 424 B and 753 664 B of rows; with a 512 KiB transaction limit both fail with 51N73 and the transaction is aborted | `exponential_outputs_respect_the_memory_cap` | pass |
| 181 on K8 (16 064 circuits, 6 568 384 B of rows) with a 1 MiB limit, 190 with a 1 KiB limit: 51N73 | `cycle_and_isomorphism_outputs_respect_the_memory_cap` | pass |
| Accountant peak ≤ cap in every memory test | same | pass |
| #64 family on `zipf300` (10 entries, up to 84 690 rows): accountant peak 19 787 177 B, cap 33 554 432 B; heaptrack peak of the whole test 15.12 MB | `heaptrack.sh … algo::tests_paths::lab_family64_memory` | measured |
| #69 family on `iso100` (6 entries; `cycles.get` 187 684 rows): accountant peak 17 762 193 B; heaptrack peak 25.72 MB | `heaptrack.sh … algo::tests_structure::lab_family69_memory` | measured |

The #69 heap peak includes the rows the test itself holds after the statement (187 684 rows); the accountant stops counting rows once they are delivered. Whether the run held uncharged heap is not separated (open point 6).

### Coverage

`cargo llvm-cov nextest -E 'test(/algo::/)'`, 31 tests. Merged per line over both index types:

| File | Regions | Uncovered lines |
|---|---|---|
| `kernels.rs` | 506 / 533 | 18 |
| `traversal.rs` | 1 342 / 1 432 | 26 |
| `paths.rs` | 2 658 / 2 904 | 116 |
| `cycles.rs` | 1 070 / 1 148 | 49 |
| `structure.rs` | 510 / 583 | 21 |
| `mod.rs` | — | 22 |

Every uncovered region, by reason:

* **Argument errors not exercised** (22G03 or 22N11): `kernels.rs:77-96` (a node given as an integer, a negative or foreign node); `traversal.rs:332-349, 377, 411, 537-566` (MAGE list, distance and filter errors); `paths.rs:433, 553-555, 720-722, 1666-1683` (NaN weights, no target, missing coordinates, `all_simple_paths` argument types); `cycles.rs:363, 392-412, 748-751` (missing coordinates, non-node points, method and vehicle types); `traversal.rs:933-935` (negative walk weights).
* **Memory errors at a given reservation** (each `)?;` after `r.mem.grow`): `kernels.rs:348`, `traversal.rs:900, 943, 1093, 1153`, `paths.rs:1092, 1169, 1233, 1284`, `cycles.rs:274, 436, 771`, `structure.rs:278`. The refusal path itself is covered by the memory tests through other reservations.
* **`usize` projections** (2^32 nodes or more): `mod.rs:575`, `structure.rs:181`. Not built in tests; #63 checked `u32` against `usize` on PageRank.
* **Evaluated at compile time**: the `const fn`s `Arg::new`, `path_entry`, `distance_entry` (`mod.rs:142-144`, `paths.rs:124-163`); `no_run` (`mod.rs:242-247`), the placeholder of `Entry::BASE` that every entry replaces.
* **Required by `Ord` only**: `PartialEq::eq` of `MinScored` and `Route` (`kernels.rs:212-214`, `paths.rs:1416-1418`).
* **Branches no case reaches (functional, not verified)**: the bidirectional search's fallback after a heap empties (`paths.rs:686-694`); A* and Yen early exits (`paths.rs:1474, 1486, 1554-1556`); `all_simple_paths` filters other than `[">"]` and `[]` (`paths.rs:1698-1730, 1766-1771`); 004 `pathExpand` relationship forms `<`, `>` alone and `<T>` without `seen` match (`traversal.rs:551-563, 663-664`); VF2 on `UNDIRECTED` projections (`structure.rs:157, 188-225`); the greedy matching above 20 odd vertices (`cycles.rs:593-609`); fewer than three TSP points (`cycles.rs:448, 505, 537-541`); more than 63 fundamental cycles (`cycles.rs:236-238`); a cross edge in MAGE's spanning tree (`cycles.rs:168`); `vrp.route` with more vehicles than routes after the savings pass (`cycles.rs:699`); a non-tournament with the edge count of one (`structure.rs:303-304`); write rows skipped for a deleted element or a non-node column (`mod.rs:711, 744-745, 800, 829, 833`); a closed graph type in a write (`mod.rs:731-735`); an empty `centralityDistribution` (`mod.rs:895`).

### Findings

1. **GDS's BFS and DFS `path`** joins consecutive visited nodes with virtual `NEXT` relationships (measured: `gds.bfs.stream` with `maxDepth` 1 on `zipf60` returned `(1)-[:NEXT]->(51)`). lpg's paths hold stored edges only, so lpg returns null in that column.
2. **MAGE `path.cpp` `<T>` filters**: `ExpandFromRelationships` inserts `{type, id}` into a `std::set<std::pair<std::string_view, int64_t>>` where `type` is a local `std::string` (`path.cpp:335, 352, 450, 471` @ `c72636014a8b`), so the stored views dangle; which `<T>` matches MAGE repeats is undefined. lpg compares type tokens.
3. **GDS's topological sort on a cyclic projection** streams one row per node: after the 14 sorted nodes of `zipf60` (the 4 of `uniform60`), 46 (56) rows of internal node 0 with distance 0.0 follow, the unfilled slots of its result array. lpg returns the sorted nodes only.
4. **GDS's all-pairs shortest paths** returns no row from a node to itself in its unweighted form (0 of 2 975 rows) and one per node in its weighted form (60 of 2 232 rows); lpg does the same.
5. **GDS's longest path on a cyclic projection** also returns nodes outside the DAG part that a sorted node points to, at the largest distance over their sorted predecessors (29 rows on `zipf60` against 14 sorted nodes); lpg does the same.
6. **MAGE `graph_util.ancestors` and `descendants`** include the node itself when it lies on a cycle (node 26 of `zipf60`: 48 and 46 nodes against NetworkX's 47 and 45). lpg follows NetworkX, the entry's oracle.
7. **MAGE's 1.5-approximation** pairs odd-degree vertices in list order (`travelling_salesman.py`, "Dummy perfect matchings"), which voids the 1.5 bound; lpg uses a minimum-weight perfect matching (exact up to 20 odd vertices).

### Open points (unverified or not done)

1. That `memgraph/memgraph-mage:3.8.0` holds the C++ code of commit `c72636014a8b` is not verified.
2. The `<T>` repetitions of `pathSubgraphNodes` (finding 2) are not compared with a reference.
3. Rule V is applied to 001 and 002; lpg's visit order differs from GDS's in 5 of 7 cases, while the node sets are equal and the BFS order keeps depth order.
4. Above 20 odd-degree vertices, 187's `1.5_approx` uses a greedy matching and its bound does not hold.
5. The write modes of 013, 014 (single pair), 015, 018, 021, 023, 028 and 029 share the PathWrite code that the 014 all-paths write test covers; they were not run one by one here. Closed in #71: `tests_trace_a::path_write_modes_one_by_one` runs each.
6. The #69 heap peak (25.72 MB) is above the accountant peak (17.76 MB); the share the test's own rows take is not separated from the run's uncharged memory. The #63 open point on the projection build is unchanged.
7. Rounds to convergence are compared with the hop depth of NetworkX's paths, not with a GDS round count, which GDS does not report.

### Quality gate

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 619 passed, 34 skipped, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --all -- --check` | clean |
| `cargo deny check licenses` | "licenses ok" |
| `cargo +1.89 build` | exit 0 |
| `cargo tree -p lpg -e normal` | 47 crates besides lpg, unchanged from #63 (no `Cargo.toml` change) |

### RAM space

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | no new file kept |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 5.3 G | 7.6 G | the build in use; the 2.4 G `llvm-cov-target` was deleted after the coverage run |

## #65 and #66

Centrality, similarity and link-prediction families (rmp #65, 19 entries) and the community-detection and clustering family (rmp #66, 11 entries), built together on the runtime of #63, at `2c0c41a` plus uncommitted changes, on 2026-10-12. Correctness only: no timing was measured.

### Runtime extensions (generic)

| Extension | Where |
|---|---|
| `Sched::fold`: block results merged in block order, blocks run in waves of `workers`, so at most that many partial results are held; `Sched::resized` for an aggregated graph | `src/algo/sched.rs` |
| Node vector properties (`Vectors`: a number is a vector of one value) and the entry field `vector_props`; edge properties named by positional arguments (`edge_args`); `ArgDefault::{Float, Int}` | `src/algo/projection.rs`, `src/algo/mod.rs` |
| Write kinds `NodeMap` (each record field as a property), `NodeField` (one record field as `writeProperty`), `RelsProp` (relationships with `writeProperty`); an entry's summary values take precedence | `src/algo/mod.rs` |
| Key types `Props`, `NodeFilter`, `Lists`; `Config::given`, `Config::has` | `src/algo/config.rs` |
| The GDS functions as one-column routines, reachable by `CALL` and by `run_algorithm` | `src/algo/functions.rs`, `src/algo/mod.rs` |
| Shared kernels: power iteration (PageRank, ArticleRank, eigenvector), Brandes and reverse BFS, the community `Net` with colour classes, local moving and modularity, k-means | `pagerank.rs`, `centrality.rs`, `community.rs`, `clustering.rs` |

### Decisions

| Decision | References studied | Pattern taken | Rejected |
|---|---|---|---|
| Deterministic parallel Louvain, Leiden, modularity optimisation, LPA, SLLPA: rounds over greedy colour classes; a class is computed by blocks from the state before it and applied in node order | GDS `modularityoptimization/ModularityOptimization.java` (K1 colouring), Lu, Halappanavar, Kalyanaraman 2015; NetworKit `PLM.cpp`, `PLP.cpp`; network_partitions `leiden/*` | colouring makes simultaneous moves of a class independent; GDS's own SLLPA runs asynchronously | fully synchronous rounds (LPA swaps across an edge; SLLPA did not settle the ring of cliques in 20 rounds, measured); NetworKit's unordered moves; GDS's lowest-label tie (it gives one community on `rc24` and `karate`, measured) |
| Sampling with per-block seeds: betweenness sampling draws per block from `rng(samplingSeed, b)`; CELF's cascade *j* is the live-edge cascade of stream *j*, read by word position | GDS `RandomDegreeSelectionStrategy.java`, `ICLazyForwardTask.java` (simulations partitioned by index); rand 0.10.3 `ChaCha8Rng::set_word_pos` | per-index streams independent of the block that reads them | GDS's `SplittableRandom.split()` per thread (order-dependent) |
| KNN candidate merge order: proposals of each block in generation order, applied in block order; ties to the lower index | MAGE `knn.hpp` (`CalculateKNN`, `UpdateNN`), GDS `similarity/knn/*` | NN-descent with old/new lists and reverse sampling | MAGE's shared generator under OpenMP and its uncleared reverse lists |
| HDBSCAN core distances by blocks over the projected vectors, all pairs | hdbscan 0.12 `core_distances/serial.rs` (brute force below 250 points), GDS documentation | brute force | the `kdtree` crate (not in TS-31) |
| k-means initialisation: stream 0; `UNIFORM` = k distinct points, `KMEANS++` = D² sampling; Lloyd sums by blocks | linfa `init.rs`, GDS `KmeansUniformSampler.java`, `KmeansPlusPlusSampler.java` | the reference samplers | changing `UNIFORM` to D² (alters GDS's semantics) |
| Eigensolver: dense nalgebra `SymmetricEigen`, eigenvalues sorted, the lower index on a tie | TS-25, cuGraph `spectral_clustering.cu` (cuVS spectral embedding, `n_init` 10) | Laplacian eigenmaps, then k-means | Lanczos (no sparse solver in TS-31); `ev_tolerance` and `ev_max_iter` therefore have no effect |
| Max k-cut: GRASP greedy construction then colour-class local search and Fiduccia-Mattheyses passes | GDS `approxmaxkcut/*`, Feo-Resende 1995, Fiduccia-Mattheyses 1982 | GRASP | GDS's uniform placement (lpg's cut fell to 130 against GDS's 131 on `planted60`, measured) |

### Instruments

| Instrument | Change |
|---|---|
| [`gds_batch.sh`](gds_batch.sh) (new) | Many queries on one fixture in one container: graphs `g` (NATURAL) and `u` (UNDIRECTED); every numeric node column projected; `LABELS` to project real labels. |
| [`oracle-cases/gds_cases_65_66.sh`](oracle-cases/gds_cases_65_66.sh), [`nx_cases_65_66.sh`](oracle-cases/nx_cases_65_66.sh), [`mage_cases_65_66.sh`](oracle-cases/mage_cases_65_66.sh) (new) | 280 GDS outputs from 17 containers (one per fixture and run, failed cases rerun with `FILTER` and `ONLY`), 5 NetworkX outputs, 4 MAGE outputs. Every container was removed (`docker ps -a` lists none). |
| Fixtures (`src/lab/generator.rs`) | `rc24` (ring of 4 cliques of 6), `planted60` (4 groups of 15, p 0.35/0.02), `blobs40` (4 clusters of 9 points and 4 noise points), `zipf60c` (`zipf60`'s generator, seed 31, with a `community` property); `write_oracle` writes integer properties. |

### Oracle results (rule F unless stated)

| ALG | Oracle | Graphs | Largest relative difference |
|---|---|---|---|
| 034 | GDS | `zipf60`, `uniform60` (natural, REVERSE weighted, UNDIRECTED), `karate`, `zipf300` | 3.9e-16 |
| 035 | GDS, no sampling | `zipf60`, `uniform60` (unweighted, weighted, undirected), `karate`, `zipf300` (blocks of 5) | 1.7e-15 |
| 038, 039 | GDS | same, with Wasserman-Faust | 1.7e-16 |
| 040 | GDS | same, weighted, `maxIterations` 100, `sourceNodes` | 3.3e-16 |
| 041 | MAGE 3.8.0 (F, `epsilon`) | `karate`, `cyc12`, `uniform60`, `zipf60` | 0 |
| 043 | GDS `pageRank` with `sourceNodes` | `zipf60`, `uniform60` | 6.9e-18 |
| 044 | GDS | `zipf60` (incl. undirected), `uniform60`, `karate`, `zipf300` | 3.3e-16 |
| 045 | GDS | `zipf60` (20 and 5 iterations), `uniform60`, `zipf300` | 1.1e-16 |
| 105 | GDS (F; V on `topN` ties) | Jaccard, overlap, cosine, weighted, cutoffs, components, filtered: `zipf60`, `uniform60`, `karate`, `zipf300` | equal similarities per source |
| 107 | GDS functions | 7 vector pairs × 6 functions | 42 equal (values or errors) |
| 112–115, 117 | GDS functions | `zipf60` (3 directions, 4 pairs, a type), `uniform60` | 85 equal |
| 116 | GDS function | `planted60`, 9 pairs and an absent property | equal |
| 082 | GDS and NetworkX `modularity` | `rc24`, `planted60`, `zipf60c`; natural, undirected, weighted | 8.3e-17; sums equal NetworkX's |

NetworkX's `partition_quality` and `constant_potts_model` were run but have no lpg column to compare (open point 4).

### Parity entries (rule Q)

| ALG | Measure | lpg | Reference on the same graph |
|---|---|---|---|
| 059 | p = 1: greedy reachability cover | `zipf60`: seeds and gains equal the test's greedy, cover 50 | GDS total 50 |
| 059 | spread ≥ GDS − 3 SE, 10 000 cascades | `zipf60` 10.836, `uniform60` 9.018 | 10.084 (SE 0.029), 8.880 (SE 0.024) |
| 106 | recall ≥ GDS's | 1.000 (`zipf60`, `uniform60`, two seeds, COSINE, EUCLIDEAN, PEARSON); filtered 0.994 | 1.000; filtered 0.961 |
| 061 | `rc24` planted (P); modularity | `rc24` 0.6875; `karate` 0.4188; `planted60` 0.6149 | GDS 0.6875, 0.4188, 0.6143 |
| 062 | as 061, seeds 42 and 7 | 0.6875, 0.4188, 0.6149 | GDS 0.6875, 0.4188, 0.6149 |
| 063 | as 061 | 0.6875 (4), `karate` 0.3991 (3), `planted60` 0.5701 (5) | GDS 0.0 (1 community), 0.0 (1), 0.3571 (2) |
| 064 | strongest label = clique on `rc24`, seeds 42 and 7 | 4 labels = the cliques; `karate` 3, `planted60` 5 and 7 strongest labels | GDS (strength 0.2): 30, 42, 79 labels in all |
| 065 | as 061 | 0.6875, 0.3466 (7), 0.5559 (6) | GDS 0.6875, 0.3466 (7), 0.5525 (8) |
| 076 | `rc24`, 4 clusters: planted | planted, modularity 0.6875; `karate` (2) 0.3715; `planted60` (4) 0.5947 | no runnable reference |
| 077 | as 076; ratio cut | planted, ratio cut 1.3333; `karate` 0.9379; `planted60` 3.5005 | no runnable reference |
| 078 | cut ≥ GDS's; optimum on 12 nodes | `rc24` 40; `karate` 61; `planted60` 134–135 (k 2), 174 (k 4); `cyc12` 19 (k 2), 23 (k 3) | GDS 40, 61, 131, 169, 19, 23; enumerated optimum 19 and 23 |
| 080 | 4 clusters (P), inertia ≤ GDS's | KMEANS++ seeds 42, 7: planted, inertia 20.3279 | GDS KMEANS++ 20.3279 |
| 081 | planted with noise (P), noise equal | planted, 4 noise points | GDS: the same |

Karate and `planted60` hold no point vectors, so 080 and 081 are measured on `blobs40` only.

### Rounds to convergence against the reference

| Entry | Graph | lpg | GDS |
|---|---|---|---|
| 040 eigenvector (`tolerance` 1e-9) | `zipf60`, `uniform60`, `karate` | 17, 42, 46 | 17, 42, 46 |
| 043 personalised PageRank | `zipf60`, `uniform60` | 73, 71 | 73, 71 |
| 044 ArticleRank (`tolerance` 1e-9) | `zipf60`, `uniform60`, `karate` | 26, 21, 26 | 26, 21, 26 |
| 045 HITS (`ranIterations` = 4 × `hitsIterations`) | `zipf60` | 20 | 20 |
| 061 Louvain (`ranLevels`) | `rc24`, `karate`, `planted60` | 1, 2, 2 | 1, 2, 2 |
| 062 Leiden (`ranLevels`) | same | 1, 2, 2 | 1, 2, 2 |
| 063 LPA (`ranIterations`) | same | 2, 4, 5 | 2, 4, 3 |
| 065 modularity optimisation | same | 3, 3, 4 | 3, 4, 5 |

### Determinism, worker counts, `CALL` and the Rust API

| Check | Test | Result |
|---|---|---|
| Every entry bit-equal at `concurrency` 1–16, twice; 035 with sampling; 059, 062, 064, 076–078, 080, 106 at two seeds | `Lab::det` in `tests_centrality.rs`, `tests_community.rs` | pass |
| `CALL` rows equal `Transaction::run_algorithm` rows, functions included | `Lab::det`, `link_prediction` | pass |
| The 20 parallel procedures of the families start exactly 2 workers on a 2-block projection at `concurrency` 4 (043's `gds.pageRank` in the #63 test); 041, 076, 077 at most one at 16 | `worker_counts_of_the_centrality_families`, `worker_counts_of_the_community_family` | pass |

### Memory

| Check | Result |
|---|---|
| 035, 105, 061, 080 under a 4 KiB transaction limit | 51N73 |
| Accountant peak ≤ cap in every test | pass |
| #65 family on `zipf300` (11 entries): accountant peak 3 476 197 B, cap 33 554 432 B; heaptrack peak of the whole test 3.56 MB | `heaptrack.sh … lab_family65_memory` |
| #66 family on `zipf300` (10 entries): accountant peak 3 476 197 B; heaptrack peak 3.56 MB | `heaptrack.sh … lab_family66_memory` |

Both peaks equal the fixture import's: no run of either family exceeded it, so the families' own peaks are below 3.56 MB but are not separated.

### Coverage

`cargo llvm-cov nextest -E 'test(/algo::/)'`, 44 tests. Regions covered: `centrality.rs` 969/1018, `similarity.rs` 1253/1277, `functions.rs` 610/661, `community.rs` 2287/2353, `clustering.rs` 1157/1213, `pagerank.rs` 394/420. Every uncovered region, by reason:

* **`usize` projections**: each `with_projection!` arm of 2^32 nodes or more (not built in tests).
* **Errors of a reservation, the scheduler or storage** (`)?;` after `r.mem.grow`, `r.mem.vec`, `sched.*`, `cx.tx.*`, `r.emit`, `check_now`): the refusal path is covered by the memory tests through other reservations.
* **Argument errors not exercised**: Katz and spectral argument types (`centrality.rs:565-569`, `clustering.rs:395-402, 900-914, 952`), function argument types and config keys (`functions.rs:149, 289, 329-330, 435-439`), an unknown KNN metric (`similarity.rs:721-724`), a source node outside the projection (`pagerank.rs:185-190`).
* **Degenerate inputs**: no node or a zero norm (`pagerank.rs:363, 435`), `2m = 0` and empty partitions (`community.rs:322, 759, 845, 1073`), fewer than two points or equal points (`clustering.rs:663, 736`), an isolated node in the normalised Laplacian and no weight property (`clustering.rs:922, 967-970`), empty vectors (`similarity.rs:332, 618, 621`), a null list or configuration, an absent type, a non-numeric community (`functions.rs:276, 382, 418, 500-502`).
* **Branches no case reaches**: CELF's heap emptied before `seedSetSize` (`centrality.rs:806`); k-means with all distances 0, an empty centroid, a singleton cluster in the silhouette (`clustering.rs:197, 271, 448, 456`); HDBSCAN selecting a cluster whose descendant was selected, and labels through nested clusters (`clustering.rs:802-837`); local moving that exhausts `maxIterations`, a NaN seed, a Louvain level without moves after the first, Leiden stopping by `tolerance` or without aggregation, a non-finite refinement sum, `approximate_exponent` below −256, LPA `consecutiveIds`, a node without community in 082 (`community.rs:431, 724, 932-933, 985-994, 1146, 1173, 1229, 1318, 1759`); the extremes of the exact integer–float comparison (`functions.rs:243-259, 323`); a non-float similarity in the distribution, an equal pair in `keep_best`, random joins without targets, a self-proposal (`similarity.rs:297, 434, 874, 890`).

### Findings

1. **GDS 2026.09.0's SLLPA rejects `randomSeed`** ("Unexpected configuration key: randomSeed", measured), while `algorithm-entries.md` marks ALG-064 R. lpg keeps `randomSeed`, as the specification says.
2. **GDS's HITS refuses an undirected projection** ("requires relationship projections to be directed", measured); lpg refuses `UNDIRECTED` with 22N11.
3. **GDS's label propagation fails its own quality measure**: one community on `rc24` and on `karate` (lowest-label ties); lpg keeps its label on a tie and otherwise draws by a fixed hash, and finds the cliques.
4. **GDS's node similarity** keeps the parallel edge of lowest relationship identifier (cosine of `zipf60` nodes 0 and 25 matches only that rule, measured) and counts a node's distinct targets for `degreeCutoff`; lpg does the same.
5. **NetworkX's `modularity` weighs by the `weight` attribute by default**; the unweighted oracle runs pass `"weight": null`.
6. **K-means `UNIFORM`** hits four clusters of 9 points with probability 27/35 × 18/34 × 9/33 = 0.11; GDS's seed-42 run found them, lpg's seed 42 does not. Rule Q is therefore checked with KMEANS++.
7. **A GDS `{ALL: {label: '*'}}` projection drops the node labels**, so label filters failed; `gds_batch.sh` projects real labels with `LABELS`, and the filtered cases pass node lists.

### Open points (unverified or not done)

1. GDS's SLLPA is asynchronous and draws from `SplittableRandom`; lpg's semi-synchronous form is compared by its quality measure only.
2. Accepted with no effect: `forceSeedOptimization` (its GDS semantics are not in the open repository), `seedTargetNodes` (initial samples always come from the targets), `ev_tolerance`, `ev_max_iter`, `leafSize`, `partitioning`, `mutateProperty`, `batchSize`; GDS-side behaviour of these is unverified.
3. The Louvain and Leiden write modes wrote `communityId` even with `includeIntermediateCommunities`; GDS writes the list then. Fixed in #71 (`louvain_and_leiden_write_the_intermediate_communities`). HITS's `nodePropertiesWritten` counts two properties per node, as GDS's own test expects (`HitsWriteProcTest.java:82`, read in #71, not run on GDS).
4. NetworkX's `partition_quality` and `constant_potts_model` have no lpg output to compare with.
5. The heap peaks of the families are below the fixture import's and are not separated (Memory).
6. MAGE's `GraphView::Neighbours` (the degree in Katz's `γ`) was not read (the `mg_graph` header is not in the clone); the results equal MAGE's on four graphs.
7. The column names of the GDS functions (`similarity`, `distance`, `score`) are lpg's: GDS's functions return a bare value.

### Quality gate

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 632 passed, 36 skipped, 0 failed (before the last header-comment edits, which change no code) |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --all -- --check` | clean |
| `cargo deny check licenses` | "licenses ok" |
| `cargo +1.89 build` | exit 0 |
| `cargo tree -p lpg -e normal` | 47 crates besides lpg; `Cargo.toml` and `Cargo.lock` unchanged |

### RAM space

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | no file kept; case files under `/tmp` deleted |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 7.7 G | 8.3 G | the build in use; the 2.7 G `llvm-cov-target` was deleted after each coverage run |

## #67 and #68

Components and structural measures (rmp #67, 13 entries) and spanning trees, flows, matching, covering, cliques and colouring (rmp #68, 16 entries), with the ALG-187 carry-over, built together on the runtime of #63, at `d2618b0` plus uncommitted changes, on 2026-10-12. Correctness only: no timing was measured.

### Runtime extensions (generic)

| Extension | Where |
|---|---|
| `Run::mode`: a write mode may write rows its stream does not return (ALG-090's 0/1 on every node; no root row for a tree's relationships) | `src/algo/mod.rs` |
| Write-only entries (empty `stream`, ALG-149); `WriteKind::NodeList` (each node of a list column, ALG-086) | `src/algo/mod.rs`, `tests.rs` invariants adjusted |
| `kernels::Simple` (simple graph, sorted rows) and `simple_undirected`, `intersect_count`; `kernels::IndexedHeap` (textbook 1-based binary heap with decrease-key) | `src/algo/kernels.rs` |
| Test probe of rounds (`sched::probe::record_rounds`) | `src/algo/sched.rs` |
| Fixture `flow30` (`flow_network(33, 30, 90)`); `gds_batch.sh` also projects `capacity` and `cost`; test `call_text` quotes each name part (`bipartite_matching.max` holds a reserved word) | `src/lab/generator.rs`, `gds_batch.sh`, `tests_paths.rs` |

### Decisions

| Decision | References studied | Taken | Rejected |
|---|---|---|---|
| 083 parallel form | Kùzu `weakly_connected_components.cpp`, NetworKit `ParallelConnectedComponents.cpp` (min-label rounds), LAGraph `LG_CC_FastSV6.c` (pointer jumping), GDS `wcc/` (CAS union-find, studied) | synchronous min-label rounds + pointer jumping, labels = smallest index | per-block union-find merged in block order (an O(n) structure per block) |
| 093 parallel form | GDS `KCoreDecomposition.java` (scan/act phases, studied), rustworkx `core_number.rs`, NetworKit `CoreDecomposition.cpp` | synchronous peeling rounds by blocks; degrees as GDS (parallel edges, self-loop twice; measured) | asynchronous cascades |
| 120/121 parallel form | graphina `parallel/triangles.rs`, LAGraph `LAGr_TriangleCount.c`, GDS `IntersectingTriangleCount.java` | each block counts its own nodes by sorted-row intersections | degree-ordered forward counting (increments to other blocks' nodes) |
| 155 | Goldberg–Tarjan; petgraph `dinics.rs`, NetworKit `Dinic.cpp`, pathfinding `edmonds_karp.rs`; GDS documents a parallel push-relabel (not in its open repository) | synchronous two-phase push-relabel (pushes from round-start heights; relabels after), applied in block order | Dinic (sequential), asynchronous push-relabel |
| 157 | NetworKit `SuccessiveShortestPath.cpp` (port), NetworkX simplex (studied) | successive shortest paths from a super source; `alpha` no effect | cost scaling (GDS) |
| 153 (item C) | GDS `ShortestPathsSteinerAlgorithm.java`, `SteinerBasedDeltaStepping.java`, `SimpleRerouter.java` (studied) | nearest-target growth; distances by synchronous Bellman-Ford rounds by blocks; GDS's simple rerouting with edges by target | delta-stepping buckets (`delta` has no effect) |
| 154 | GDS `pricesteiner/*` (studied), `pcst_fast` @ `25ab31a` (MIT, read) | Goemans–Williamson growth (events found by scanning edges) + strong pruning | pairing heaps (only change the cost) |
| 151 | graphalgs `st.rs:79` (f64 cast), NetworkX (float) | exact: determinants modulo 2^61−1 and 2^62−57, CRT, Hadamard–Fischer bound; nalgebra f64 only when the bound exceeds 2^123 | f64 determinant (karate: 5090996323019138 from nalgebra, exact 5090996323019136) |
| 176 (parity) | GDS `k1coloring/*` (speculative + repair, studied), LAGraph `LAGraph_coloring_independent_set.c` (rounds) | Jones–Plassmann rounds, largest-degree-first priority | speculative rounds (timing-free conflicts need repair rounds; more colours) |
| Matching (187, carry-over) | rustworkx `max_weight_matching.rs`, NetworkX `matching.py` | port of rustworkx (blossom, i128 duals), distances rounded to 2^-20 m | float duals |
| 090/091 | petgraph `bridges.rs:83` (skips every edge to the parent), GDS (skips one) | own low-link DFS with GDS's parent rule | calling petgraph (reports both parallel edges as bridges) |
| 178 | MAGE `solver.py:119-141` (random order, not greedy) | classic greedy (most uncovered elements, first listed) | MAGE's random order (not reproducible) |

### Copies

| Source | Commit | Licence | Into |
|---|---|---|---|
| rustworkx-core `max_weight_matching.rs` | `25398ab47f66` | Apache-2.0 (modified; notice in header) | `blossom.rs` |
| graphalgs `metrics.rs` (eccentricity, diameter, radius), `spec/laplacian.rs`; method of `spec/st.rs` | `1f8a412ef4af` | MIT | `measures.rs`, `trees.rs` |
| petgraph `maximal_cliques.rs`; `condensation` logic (`mod.rs:477`) | 0.8.3 | MIT | `matching.rs`, `components.rs` |
| NetworKit `flow/SuccessiveShortestPath.cpp` (port) | `5f03e17cec09` | MIT (text quoted) | `flow.rs` |
| MAGE `bipartite_matching.cpp` (port) | `c72636014a8b` | Apache-2.0 | `matching.rs` |
| NetworkX `greedy_coloring.py` strategies (port); `tests/test_matching.py` cases | 3.7 | BSD-3-Clause (text quoted) | `matching.rs`, `blossom.rs` tests |
| Called (dependency petgraph 0.8.3) | — | — | `tarjan_scc`, `UnionFind`, `maximum_matching`, `greedy_matching`, `dsatur_coloring`, `greedy_feedback_arc_set` (on a `DiGraph` in forward order), `bellman_ford` |

### Instruments

[`oracle-cases/gds_cases_67_68.sh`](oracle-cases/gds_cases_67_68.sh), [`nx_cases_67_68.sh`](oracle-cases/nx_cases_67_68.sh), [`mage_cases_67_68.sh`](oracle-cases/mage_cases_67_68.sh) (new): 142 GDS, 54 NetworkX and 3 MAGE outputs. Every container was removed (`docker ps -a` lists none). lpg's CSV import numbers nodes in the order of the `id:ID` strings (0, 1, 10, 11, …; measured), GDS's native projection in numeric order; the order-dependent heuristics (149, 153 rerouting) were also run on a GDS Cypher-aggregation projection that numbers nodes in lpg's order (`*.o.jsonl`), and the NetworkX colouring oracle renumbers nodes to lpg's order.

### Oracle results

| ALG | Oracle, rule | Graphs | Result |
|---|---|---|---|
| 083 | GDS, P (default, `UNDIRECTED`, threshold 0.5, `consecutiveIds`, seeded: ids equal too) | karate, zipf60, uniform60, zipf300, zipf60c | equal; write `componentCount` and distribution equal |
| 084 | NetworkX, P | zipf60, uniform60, zipf300 | equal |
| 085 | GDS, P | zipf60, uniform60, zipf300 | equal |
| 086 | NetworkX, P members, X edges | zipf60, uniform60, cyc12 | equal |
| 089 | NetworkX on the simple graph, P on edges | karate, zipf60, uniform60 | equal |
| 090, 091 | GDS, X (`resultingComponents`, unordered pairs with `remainingSizes`; write values 1/0) | karate, zipf60, uniform60 | equal |
| 093 | GDS, X (+ `degeneracy`) | karate, zipf60, uniform60, zipf300 | equal |
| 120 | GDS, X (counts, `maxDegree` 5, listing, `globalTriangleCount`) | same | equal |
| 121 | GDS, F | same | largest difference 0; average within 1e-9 |
| 123 | NetworkX, X / F weighted | karate, uniform60 (undirected), cyc12, zipf60 (directed, from NX shortest paths) | equal |
| 128 | NetworkX `girth`, X; directed: shortest NX simple cycle | karate, petersen, bip20, dag40, cyc12 | equal |
| 143 | NetworkX, F | karate (undirected), zipf60 (directed), both weighted | largest difference 1.9e-16 |
| 147 | GDS, V | karate, zipf60, uniform60 (min, max, unweighted) | same node set, totals within 1e-9 |
| 149 | GDS, V (k nodes; weight of the tree on them) | karate, zipf60, uniform60 (k 5 min, k 4 max) | equal node sets except karate k 5 (below) |
| 151 | rational determinant, X; 22003 above `INTEGER` | karate, petersen, negw30, flow30, dag40 (undirected, 22003), dag40/geo30/rc24 rooted | equal |
| 153 | GDS in lpg's order, V | zipf60, uniform60, zipf300 (weighted, rerouting, unweighted) | weighted equal; unweighted below |
| 154 | GDS, V | geo30 (`lat`; `lng` gives no row, as GDS) | weight 535.7222378524907 against 535.7222378524908; prizes equal |
| 155, 157 | GDS, V | flow30 (2 → 1 and 5 → 3 terminals, no capacity) | flow 46 and 79; cost 457 and 564 (also `alpha` 2), as GDS |
| 161 | NetworkX sizes, V; greedy maximal | karate, zipf60, uniform60, petersen | equal |
| 163 | NetworkX Hopcroft–Karp, X | bip20; karate gives 0 as MAGE | equal |
| 169 | NetworkX `find_cliques`, X | karate, zipf60, uniform60 | equal |
| 171 | GDS, X | karate; zipf60, uniform60, zipf300 against GDS on their simple graph | equal; `globalCount` [45, 11, 2] |
| 173 | NetworkX, V (proper, equal colour count) | 6 strategies × karate, zipf60, uniform60, petersen; `random_sequential` proper only | equal |
| 174 | petgraph `dsatur_coloring`, V | same | equal (5, 5, 4, 3) |
| 177 | petgraph on the same edge sequence, X; acyclic remainder | zipf60, cyc12 | equal (44 and 4 arcs) |
| 178 | MAGE (random), V | bip20 | lpg 3 sets = exhaustive optimum; MAGE runs 6, 4, 7 |
| 187 | NetworkX `min_weight_matching`, 24 points (24 odd vertices); exhaustive on random sets of 2–14 | — | 2132.585815131451 = NetworkX; NetworkX's 14 blossom cases equal |

### Parity entry 176 (rule Q)

Proper colouring, bit-equal at `concurrency` 1–16 twice; colours: karate 5 (GDS 6 natural, 6 undirected), zipf60 5 (6, 6), uniform60 4 (5, 6), zipf300 7 (11, 11). At most GDS's in every case.

### Rounds against the reference

| Graph | 083 rounds (lpg) | 093 rounds (lpg) | GDS k-core phases |
|---|---|---|---|
| karate | 3 | 7 | 4 |
| zipf60 | 4 | 17 | 8 |
| uniform60 | 4 | 11 | 4 |
| zipf300 | 4 | 42 | 20 |

GDS's k-core phases are counted from its control flow (`KCoreDecomposition.java`: one scan, and one act phase per level, a cascade inside a phase) applied to the core values; GDS's WCC has no rounds (one concurrent union-find pass). lpg's synchronous rounds are more numerous; no timing was measured.

### Determinism, worker counts, `CALL` and the Rust API

Every stream entry: bit-equal rows at `concurrency` 1–16, twice each, and `CALL` equal to `Transaction::run_algorithm` in one snapshot (`Lab::det`). On a 2-node projection the parallel entries (083, 093, 120, 121, 153, 155, 171, 176) start exactly `min(concurrency, 2)` workers at 1, 2 and 16; every other entry one (149 through its write mode).

### Memory

| Measure | Value |
|---|---|
| 143 on zipf300 under a 524 288 B transaction limit | 51N73 (300² floats = 720 000 B); accountant peak 5 269 825 B ≤ cap 33 554 432 B |
| #67 family on zipf300 (14 entries) | accountant peak 5 269 825 B; heaptrack peak of the whole test 4.23 MB (`heaptrack.sh … lab_family67_memory_and_rounds`, includes four fixture loads) |
| #68 family on zipf300, flow30, geo30 (14 entries) | accountant peak 3 476 197 B; heaptrack peak 3.85 MB (`… lab_family68_memory`) |

### Coverage

`cargo llvm-cov nextest -E 'test(/algo::(tests_(components|flow)|blossom|trees)::/)'`, 19 tests. Regions covered: `components.rs` 1052/1100, `measures.rs` 592/627, `trees.rs` 1556/1602, `flow.rs` 625/652, `matching.rs` 1262/1315, `blossom.rs` 938/939. Every uncovered region, by reason:

* **Errors of a reservation or of `emit`** (`)?;` after `r.mem.grow`, `r.emit`): the refusal path is covered by the memory tests.
* **`usize` projections**: each `with_projection!` arm of 2^32 nodes or more.
* **Argument errors not exercised**: `graph_coloring` and `set_cover` value types (`matching.rs:766-787, 959-965`); a negative-cost cycle in 157 and a negative cycle in weighted 123 (`flow.rs:447-459`, `measures.rs:380-384`).
* **Inputs the fixtures do not hold**: self-loops in flow30 and geo30 (`flow.rs:217`, `trees.rs:1093`), a self-loop in 163 (`matching.rs:331`), unseeded components with a seed property (`components.rs:397-408`), an excluded node in `gds.triangles` (`measures.rs:266`), a NaN triangle count (`measures.rs:319`), no node (`components.rs:240`, `measures.rs:332`, `trees.rs:578, 997`), an uncoverable element (`matching.rs:1008`).
* **Implicit `else` branches and loop exits**: the test-only rounds probe (`components.rs:384, 861`), Steiner's guard against a parent cycle and rerouting without change (`trees.rs:743, 785`), the k-tree's node dropped after its parent left (`trees.rs:395`), GW deactivation of an inactive cluster and the no-event exit (`trees.rs:881, 899, 961, 975`), a relabel without residual arc (`flow.rs:394`), `blossom.rs:97`.

### Findings

1. **GDS 2026.09.0's clique counting fails on a projection with self-loops or parallel edges**: `ArrayIndexOutOfBoundsException: Index 1 out of bounds for length 1` (zipf60, zipf300), `NegativeArraySizeException: -1` (uniform60). The simple-graph projection runs.
2. **GDS's prize-collecting Steiner tree refuses an integer prize** ("Value type required: [DOUBLE]").
3. **GDS's Steiner rows give rounded path-cost differences** (0.5641788520905568 for an edge of 0.5641788520905567); the tests compare edges within 1e-12.
4. **GDS's degrees differ by entry** (measured): k-core counts parallel edges and a self-loop twice; the clustering coefficient's degree counts distinct neighbours and the node itself when it has a self-loop; triangle counts are the simple graph's; `maxDegree` uses the projected degree.
5. **Withdrawn in #71 (finding C7).** This finding said that rustworkx's `augment_blossom` test `> num_nodes` misses the blossom numbered `num_nodes`. That blossom is never allocated (`blossom_never_allocates_blossom_n`), so the test names no reachable defect; lpg's `>=` behaves the same.
6. **MAGE's "greedy" set cover is a random order** (sizes 6, 4 and 7 in three runs on bip20).
7. **lpg's CSV import numbers nodes in the string order of `id:ID`**, so order-dependent results (ties, heuristics) differ from GDS's native projection; in lpg's order, 153's rerouting equals GDS (zipf60: 4.0411 both; natively GDS 3.9209).

### Divergences and open points (stated plainly)

1. **149 on karate, k 5, minimum**: lpg's tree weighs 7, GDS's 5 in lpg's node order (7 in GDS's own order). Karate's integer weights tie; the queue and leaf choices among equal weights are GDS's own code (GPL, not ported). Asserted on zipf60 and uniform60 (float weights, equal node sets), recorded on karate.
2. **153 unweighted on uniform60**: lpg 9, GDS 10 (lpg lighter); equal hop counts tie. Weighted cases asserted; unweighted recorded (zipf60 and zipf300 equal).
3. **151** uses nalgebra's `f64` determinant only to decide when the diagonal bound exceeds 2^123 and the residue fits `INTEGER`; that decision is not proved (exercised by a unit test). TS-25 names nalgebra for 151; the exact count does not use it otherwise.
4. **173**: NetworkX's `smallest_last` and `independent_set` take arbitrary set elements; the oracle runs copies taking the smallest node. `random_sequential` is checked for properness only.
5. **Accepted with no effect**: 153 `delta`, 157 `alpha`, 176 `maxIterations` and `batchSize` (`ranIterations` is 1), 173 `edge_property`, 083 and 121 `forceSeedOptimization`. **Refused**: 120 `labelFilter` (22N11; its GDS semantics were not measured).
6. **154**: the best root is detached before pruning, where GDS's code (studied) prunes the root above it; not triggered on geo30, GDS's behaviour in that case unverified.
7. **149**: a node whose tree parent has left the tree is dropped (GDS's code adds it); not reached by the fixtures (coverage).
8. **089, 128, 161, 169, 171, 173** read the simple graph (parallel edges once); 128 counts a self-loop as a cycle of one edge.
9. `algorithm-entries.md` and TS-25 were not edited (out of scope): the measured GDS rules above (findings 1–4, 7) and points 3, 5 are candidates.

### Quality gate

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 651 passed, 38 skipped, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --all -- --check` | clean |
| `cargo deny check licenses` | "licenses ok" |
| `cargo +1.89 build` | exit 0 |
| `Cargo.toml`, `Cargo.lock` | unchanged; no new crate |

### RAM space

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | `/tmp/pcst_fast` clone (reference, 9.5 MB) kept; case files deleted |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 8.2 G | 5.4 G | the build in use; 7.6 G of stale `debug/incremental` and the `llvm-cov-target` deleted (the first coverage run hit the quota) |

## #71 mechanical review

Independent review of every entry's operation sequence against its reference execution (rmp #71), in three serial passes. Pass A: the 37 entries of #64 and #69, at `3f9e2b2` plus uncommitted changes, on 2026-10-12. Correctness only; no timing.

### Trace infrastructure (shared by passes A, B and C)

* **`trace!(kind, format, args…)`** (`src/algo/mod.rs`): under `cfg(test)` it appends the event `"kind: text"` to the recording of the current thread; in normal and release builds it expands to nothing. Evidence: in the `cargo build --release` artefact `liblpg.rlib`, the trace-only literals `nontree`, `unblock`, `fallback`, `potential`, `spurs `, `changing ` and the symbols `5trace4push`, `trace5adopt` occur 0 times; in the test binary each occurs at least once.
* **`src/algo/trace.rs`** (`#[cfg(test)]` module): `Recording::start()` and `take()`. The sink follows the run onto the statement's blocking thread (`src/api/mod.rs` `blocking`, test-only lines) and onto the scheduler's workers (`src/algo/sched.rs` `workers`); an event emitted inside a block carries the block index, and `take()` sorts each run of block events by block, so the sequence is the block-ordered one at every `concurrency`. Per-round state is emitted from the coordinating thread after the merge.
* **`TraceGraph`** builds a small graph under `/tmp/lpgram`: node `i` has `uid` `i`, edge `i` has `eid` `i`; it asserts that dense index `i` is node `i` and that edge `i` has element identifier `n + i`, so a reference script can name nodes and edges. `trace()`, `workers()` (worker-count probe), `uid`, `uids`, `path_uids`; `only()` filters kinds; `assert_steps()` names the first differing step.
* **Reference cases**: `docs/evidence/lab-sprint-5/trace_ref_a.py` ports each reference (source file and line in each function's docstring) and writes `src/algo/testdata/expected/trace_a.json`: the trace graphs and, per case, the call, the event kinds, the expected events and optionally the expected rows. `tests_trace_a::reference_traces` runs every case at each listed `concurrency` (1 and 16 for parallel entries; a graph of n ≤ 64 nodes has n one-node blocks, and the test checks that more than one worker ran at 16) and compares step by step. Passes B and C add their own script, JSON file and runner on the same pattern.

Pass A: 126 reference cases (1 048 expected events), 12 tests in `src/algo/tests_trace_a.rs`.

### Review table

| Entry | Reference execution source | Trace graph | Verdict | Divergences | Tests added |
|---|---|---|---|---|---|
| 001 BFS | GDS `BFS.java`, `BFSTask.java` (level order: smallest predecessor position, then adjacency order); sequential FIFO by level | `dir` (7 nodes, parallel edge, self-loop); `zipf60`, `uniform60` | conforms | none: the 5 of 7 lab differences are GDS's algorithm on a different node numbering (below) | `reference_traces` (001 ×3, c 1 and 16); `bfs_and_dfs_orders_are_gds_executions_on_lpg_numbering` |
| 002 DFS | GDS `DFS.java` `compute` | `dir`; `zipf60`, `uniform60` | conforms | as 001 | `reference_traces` (002 ×3); as 001 |
| 003 neighbours at hop | MAGE `neighbors.cpp` `DetermineDirection`, `AtHop`, `ByHop` | `typed` (types A, B) | conforms | none | `reference_traces` (003 ×12, six filters) |
| 004 path expansion, subgraph | MAGE `path.cpp` `PathExpand::DFS`:359, `PathSubgraph::BFS`:488, `PathHelper`:86-104 | `typed` (labels X, Y) | fixed | `pathSubgraphAll` listed each relationship twice under `UNDIRECTED` | `reference_traces` (004 ×37: 30 expand filter pairs, 7 subgraph); `path_subgraph_all_lists_each_relationship_once` |
| 006 random walk | node2vec bias (Grover and Leskovec 2016 §3.2.2); GDS `RandomWalkSampler.java` `walkOneStep`, `RandomWalkTask.java` `run` | `dir` weighted, p 0.5, q 2.0; a DAG | fixed | walks started from nodes without outgoing edges | `random_walk_steps_follow_node2vec`; `random_walk_starts_no_walk_from_a_node_without_out_edges`; `random_walk_quality` count corrected |
| 007 topological sort | Kahn by levels; NetworkX `topological_generations` | `dag` (two sources, parallel edge), `dir` (cyclic) | conforms | none | `reference_traces` (007 ×2, c 1 and 16) |
| 008 ancestors, descendants | NetworkX `bfs_edges` (FIFO) | `dir` | conforms | none | `reference_traces` (008 ×2) |
| 009 reachability | petgraph `has_path_connecting` on a `DiGraph` of the same edges | `dir`, all 49 pairs | conforms | none | `petgraph_entries_equal_petgraph_on_the_same_graph` |
| 011 dominators | NetworkX `immediate_dominators`, `dominance_frontiers` climbs | `dir`, `und` | conforms | none | `reference_traces` (011 ×2) |
| 013 unweighted paths | BFS tree (NetworkX `bfs_predecessors`) | `dir`: source, target, pair | conforms | none | `reference_traces` (013 ×3); `path_write_modes_one_by_one` |
| 014 Dijkstra | rustworkx `dijkstra.rs` (petgraph fork); GDS settle order without ties | `dir`: pair, all, `REVERSE` | conforms | none | `reference_traces` (014 ×3); write mode |
| 015 bidirectional Dijkstra | petgraph `dijkstra.rs` `bidirectional_dijkstra`:287-407 | `dir`; all pairs of 3 graphs | conforms | none; the fallback is reached only with no meeting | `reference_traces` (015); `bidirectional_fallback_has_no_meeting`; write mode |
| 018 A* | petgraph `astar` on a `DiGraph`, GDS haversine heuristic | `dir` with coordinates, 2 targets | conforms | none | `petgraph_entries_…`; write mode |
| 019 Bellman-Ford | round form d_k = min(d_{k-1}, d_{k-1}(u) + w) (Bellman 1958; CLRS §24.1) | `neg`, `negcyc`, `dir` | conforms | none | `reference_traces` (019 ×3, c 1 and 16) |
| 020 negative cycle | petgraph `find_negative_cycle` on a `DiGraph` | `negcyc` | conforms | none | `petgraph_entries_…` |
| 021 SPFA | petgraph `spfa` on a `DiGraph` (distances, predecessors) | `neg` | conforms | none | `petgraph_entries_…`; `edge_cases_and_errors`; write mode |
| 023 delta-stepping | Meyer and Sanders 2003, Fig. 1 (light phases, heavy phase) | `dir`, Δ 2 and 3 | conforms | none | `reference_traces` (023 ×2, c 1 and 16); write mode |
| 024 Johnson | petgraph `johnson.rs`; potentials by the 019 rounds; NetworkX `johnson` distances | `neg` | conforms | none | `reference_traces` (024, c 1 and 16); `floyd_warshall_fails_on_a_negative_self_loop` |
| 025 Floyd-Warshall | petgraph `floyd_warshall` on a `DiGraph`; negative cycle by definition | `neg`; a negative self-loop | fixed | a negative self-loop returned distances instead of 22N11 | `petgraph_entries_…`; `floyd_warshall_fails_on_a_negative_self_loop` |
| 026 Seidel | graphalgs `seidel.rs` `apd` per component | `und`, `bip` | conforms | none | `reference_traces` (026 ×2) |
| 027 all-pairs distances | one BFS or Dijkstra per source | `dir`, weighted and not | conforms | none | `reference_traces` (027 ×2, c 1 and 16) |
| 028 all shortest paths | NetworkX `dijkstra_predecessor_and_distance`, `all_shortest_paths` | `und` (ties), `dir` | conforms | none | `reference_traces` (028 ×2); write mode |
| 029 Yen | pathfinding `yen.rs` `yen` with edge paths | `dir`, k 5 | conforms | none | `reference_traces` (029, c 1 and 16); write mode |
| 030 k-th shortest length | petgraph `k_shortest_path` on a `DiGraph` | `dir`, k 1-3, with a target | conforms | none | `petgraph_entries_…` |
| 031 all simple paths | MAGE `algo.cpp` `UpdateRelationshipDirection`:11, `DFS`:75 | `typed`, nine filters | conforms | none | `reference_traces` (031 ×9) |
| 032 longest path | DAG DP by levels (CLRS §24.2) | `dag` | conforms | none | `reference_traces` (032, c 1 and 16) |
| 180 cycles | MAGE `cycles.cpp` `FindNonSpanningTreeEdges`:29, `FindFundamentalCycle`:64, `CombineCycles`:93, `GetCycles`:170 | `und` (parallel edge, self-loop), `bip` | conforms | none | `reference_traces` (180 ×2) |
| 181 elementary circuits | graphalgs `elementary_circuits.rs`:13, 71; count checked with NetworkX `simple_cycles` | `dir`, `und` | conforms | none | `reference_traces` (181 ×2) |
| 187 TSP (`1.5_approx`) | NetworkX `christofides` steps: Kruskal tree, odd nodes, `min_weight_matching`; Euler circuit and shortcut checked | `geo` (6 points) | conforms | none | `reference_traces` (187); `christofides_circuit_and_shortcut` |
| 188 VRP | Clarke and Wright savings with TS-25's rules; 2-opt (Croes 1958) | `geo`, 1 and 2 vehicles; a crossed route | conforms | none | `reference_traces` (188 ×2); `two_opt_moves` |
| 189 isomorphism | NetworkX `is_isomorphic` with edge counts; lpg's `counted` collapse | `iso` (P, Q, U, S), `NATURAL` and `UNDIRECTED` | conforms | none | `reference_traces` (8 cases) |
| 190 subgraph isomorphism | NetworkX `subgraph_is_isomorphic` (node-induced) with edge counts | `iso`, both orientations | conforms | none | `reference_traces` (8 cases) |
| 194 bipartiteness | rustworkx `coloring.rs` `two_color` | `bip`, `und` (conflict) | conforms | none | `reference_traces` (194 ×2) |
| 203 tournament | NetworkX `is_tournament`, `is_directed_acyclic_graph` | `tour`: transitive, 3-cycle, non-tournament with n(n−1)/2 edges | conforms | none | `reference_traces` (203 ×3) |
| 204 graphical sequence | graphalgs `spec/mod.rs` (Havel-Hakimi pops); NetworkX `is_graphical` | five sequences | conforms | none | `reference_traces` (204 ×5) |
| 205 transitive closure | Kahn order handed to petgraph `tred`; NetworkX `transitive_closure_dag` rows | `dag` | conforms | none | `reference_traces` (205) |
| 206 transitive reduction | as 205; NetworkX `transitive_reduction` rows; first parallel edge's identifier | `dag` | conforms | none | `reference_traces` (206) |
| 034 degree | GDS `degree/DegreeCentrality.java:166-172` (pass B: `trace_ref_b.py`, `trace_b.json`, `tests_trace_b.rs`) | `pr` (6 nodes, weighted, self-loop, parallel edge): NATURAL, REVERSE, UNDIRECTED, weighted; `one` (one node, self-loop); `nil` (empty) | conforms | none | `tests_trace_b::reference_traces` (034 ×5, c 1 and 16) |
| 035 betweenness | rustworkx-core `centrality.rs:406-445` (BFS, σ, a predecessor per edge), `:254-279` (accumulation); GDS `betweenness/WeightedForwardTraverser.java` `traverse`, `BetweennessCentrality.java` (÷ 2 undirected) | `pr` unweighted, weighted, UNDIRECTED; `cm` (11 nodes: cliques 0-3, 4-7, triangle 8-10 in a ring, parallel edge, self-loop); `one`; `nil` | conforms | none: per source the settle order, σ and δ, then the block-ordered sum, at c 1 and 16 | `reference_traces` (035 ×5) |
| 038 closeness | GDS `closeness/ClosenessCentrality.java` `computeFarness`, `DefaultCentralityComputer`, `WassermanFaustCentralityComputer` | `pr` default, Wasserman-Faust, UNDIRECTED; `one`; `nil` | conforms | none: each node's reverse-BFS depth counts and value | `reference_traces` (038 ×4) |
| 039 harmonic | GDS `harmonic/HarmonicCentrality.java` | `pr` NATURAL, REVERSE; `one` (0 / 0: NaN, as Java's double division); `nil` | conforms | none | `reference_traces` (039 ×3) |
| 040 eigenvector | GDS `pagerank/EigenvectorComputation.java` (`compute`, `masterCompute`: L2 norm, convergence after superstep 0), `pregel/.../Pregel.java:204-242` | `pr` 30 iterations, weighted, `sourceNodes` [1, 3]; `one`; `nil` | conforms | none: the ranks of every superstep bit-equal, the stopping superstep equal | `reference_traces` (040 ×4) |
| 041 Katz | MAGE `katz_centrality_module/algorithm/katz.cpp` `KatzCentralityLoop`:204-231, `Converged`:150-195, γ :385-386; memgraph `include/mg_graph.hpp:225-234` (Δ: out-degree) | `pr`, α 0.25 and 0.125 (exact powers); `one` | conforms | none: c, ur and the sorted order per iteration, the stopping iteration | `reference_traces` (041 ×3) |
| 042 PageRank | GDS `pagerank/PageRankComputation.java` `compute`; `Pregel.java:204-242`; `ReducingMessenger.java:119-120` (a message sum of 0 is no message); `ComputeStep.java:71` | `pr` default, weighted, `tolerance` 0.01, REVERSE, UNDIRECTED; `one`; `nil` | conforms | none: ranks and senders of every superstep bit-equal at c 1 and 16; lpg's pull over incoming edges sums in GDS's single-thread send order on these graphs | `reference_traces` (042 ×6) |
| 043 personalised PageRank | as 042 with `sourceNodes` (`initialValue`) | `pr`, source 0 | conforms | none | `reference_traces` (043) |
| 044 ArticleRank | GDS `pagerank/ArticleRankComputation.java` (δ / (degree + average degree)) | `pr` default, weighted; `one`; `nil` | conforms | none | `reference_traces` (044 ×3) |
| 045 HITS | GDS `hits/HitsComputation.java`, `HitsState.java`; `HitsWriteProcTest.java:82` | `pr`, 1 and 3 iterations; `one`; `nil` | fixed | `partitioning` accepted any string (GDS `Partitioning.parse`: RANGE, DEGREE, AUTO); whitespace in `authProperty`, `hubProperty` accepted (GDS `HitsConfig.validate*Property`). Supersteps conform. Known input: `nodePropertiesWritten` 2 per node equals GDS (6 on 3 nodes) | `reference_traces` (045 ×3); `hits_write_counts_two_properties_per_node`; `parameter_domains_as_gds` |
| 059 CELF | Leskovec et al., KDD 2007 (lazy forward); GDS `influenceMaximization/CELF.java` (`greedyPart`, `lazyForwardPart`, queue: higher spread, then smaller node) | `pr`: p 1, p 0.5 and 0.3 (cascades from the ported generator); `one`; `nil` | fixed | `propagationProbability` below 0.01 accepted (GDS `InfluenceMaximizationBaseConfig`: [0.01, 1]). Pops, stale re-estimates and selections equal. GDS re-estimates a batch of the queue (`batchSize`), lpg the top only: same selection under exact gains; GDS draws a node's edges from a fresh `SplittableRandom(seed + simulation)` (`ICInitTask.java`), lpg one word per edge slot (parity entry) | `reference_traces` (059 ×4); `parameter_domains_as_gds` |
| 061 Louvain | Blondel et al. 2008; the colour step of GDS `modularityoptimization/ModularityOptimization.java` `optimizeColor` and `ModularityOptimizationTask.java` `run` (GDS's gain arithmetic checked to decide alike in the script) | `cm` NATURAL, UNDIRECTED; `one`; `nil` | fixed | (1) the write mode with `includeIntermediateCommunities` wrote the final community; GDS writes the list (`LouvainWriteStep.java:68-72`); (2) `includeIntermediateCommunities` with `consecutiveIds` accepted; GDS refuses (`LouvainBaseConfig.validate`). Class steps: as 065 | `reference_traces` (061 ×3); `louvain_and_leiden_write_the_intermediate_communities`; `parameter_domains_as_gds`; `community_options` updated |
| 062 Leiden | Traag et al. 2019; network_partitions `leiden/subnetwork.rs:44-148`, `quality_value_increment.rs`, `leiden_clustering.rs:150-226` | `cm` seeds 42 and 7 (refinement draws from the ported generator); `one`; `nil` | fixed | as 061 (1) and (2) (`LeidenWriteStep.java:71-75`, `LeidenBaseConfig.validate`). Local moves, refinement and aggregation conform | `reference_traces` (062 ×3); as 061 |
| 063 label propagation | Raghavan et al., Phys. Rev. E 76, 2007; GDS `labelpropagation/ComputeStepConsumer.java` `tallyVotes`, `ComputeStep.java`; NetworkX `community/label_propagation.py:220-231` | `cm` NATURAL, UNDIRECTED; `one`; `nil`; fixtures `rc24`, karate, `planted60` | conforms | none in code; the module documentation said "the lowest label on a tie" and is corrected (kept: a best current label, else least hash). A class equals the sequential updates of its nodes (asserted on `cm` and the fixtures). GDS's algorithm ported (asynchronous node order, lowest label) on GDS's numbering gives GDS's (ranIterations, communities) (2, 1), (4, 1), (3, 2); lpg's rule on lpg's numbering gives lpg's (2, 4), (4, 3), (5, 5) | `reference_traces` (063 ×3); `round_counts_against_gds_follow_numbering_and_tie_rule` |
| 064 SLLPA | Xie, Szymanski and Liu 2011; GDS `sllpa/SpeakerListenerLPAComputation.java` (`listen`, `speak`, `prune`) | `cm`, 6 iterations, seeds 42 and 5 (draws from the ported generator); `one`; `nil` | fixed | `partitioning` accepted any string (as 045). Steps conform; a class equals the sequential listens (asserted). Known input: `randomSeed` per the specification (R), GDS refuses it (specification point) | `reference_traces` (064 ×3); `parameter_domains_as_gds` |
| 065 modularity optimisation | GDS `ModularityOptimization.java` (`compute`, `optimizeColor`, `updateModularity`), `ModularityOptimizationTask.java` `run` | `cm`; `c4` (4-cycle); `nil`; fixtures | conforms | none in code; the module documentation claimed a class step equals the sequential local move: false, corrected (`c4`: the class {0, 2} moves 0 and 2 into node 1's community; one after the other, 2 joins 3). lpg's class step is GDS's colour step. The algorithm on GDS's numbering gives GDS's (3, 4), (4, 7), (5, 8); on lpg's, lpg's (3, 4), (3, 7), (4, 6): the round-count difference is the node numbering | `reference_traces` (065 ×2, `c4` with `differs`); `round_counts_against_gds_follow_numbering_and_tie_rule` |
| 076 spectral clustering | cuGraph `spectral_clustering.cu:36,110` (method); Ng, Jordan and Weiss 2001; NumPy `eigh` | `sp` (7 nodes, two groups, simple eigenvalues) | conforms | none: the Laplacian bit-equal, eigenvalues and eigenvectors 2-3 equal to 1e-6 up to sign; k-means: 080's kernel | `reference_traces` (076) |
| 077 balanced-cut clustering | von Luxburg 2007 (D − A); as 076 | `sp` | conforms | none | `reference_traces` (077) |
| 078 approximate maximum k-cut | Feo and Resende 1995; Fiduccia and Mattheyses 1982 (lpg's documented GRASP); GDS `ApproxMaxKCutBaseConfig` | `cm` k 2 and 3, 3 restarts (ported generator); `one`; `nil` | fixed | `k` above 127 accepted (GDS: `Byte.MAX_VALUE`). Order, construction, class search and FM passes conform; a class equals the sequential search (asserted) | `reference_traces` (078 ×3); `parameter_domains_as_gds` |
| 080 k-means | GDS `kmeans/Kmeans.java` (`compute`, `kMeans`, `updateBestSolution`), `KmeansIterationStopper.java`, `KmeansTask.java`, `CommunityAlgorithms.kMeans`, `KmeansBaseConfig`; linfa `k_means/init.rs` | `pts` (10 points, CSV fixture `pts10`): UNIFORM, KMEANS++, `maxIterations` 1, `deltaThreshold` 0.1, 3 restarts | fixed | (1) stopped below n·δ swaps; GDS from iteration 2 at ≤ ⌊n·δ⌋; (2) reassigned the points to the final centroids; GDS keeps the last assignment; (3) restart of least inertia; GDS of least average distance; (4) `k` > n failed 22N11; GDS puts each node alone; (5) `seedCentroids` not of `k` centroids accepted; (6) with restarts accepted; (7) a number property accepted. Known input: UNIFORM draws k distinct points uniformly (partial Fisher-Yates, draws equal the ported generator's); one per planted cluster with probability 9⁴ / C(36, 4) = 0.11 on `blobs40`, for GDS too | `reference_traces` (080 ×5); `kmeans_stops_and_reports_as_gds`; `kmeans_validates_as_gds`; two tests of `tests_community` updated |
| 081 HDBSCAN | hdbscan `core_distances/serial.rs:72-81`, `min_spanning_tree.rs:76-103`, `hdbscan.rs:293-408` (tree, condensation), `:486-536` (selection), `:729-787` (labels) | `pts`: (`minClusterSize`, `samples`) (2, 2), (3, 3) | conforms | none: core distances, MST order, condensed tree, stabilities and selected clusters | `reference_traces` (081 ×2) |
| 082 partition quality | GDS `modularity/RelationshipCountCollector.java` `run`, `ModularityCalculator.java:82-95` and `create`; `conductance/Conductance.java:276-280` | `cmc` (`cm` with `community`) NATURAL, UNDIRECTED | fixed | modularity skipped a negative community and its edges; GDS counts it. Conductance skips it, as GDS | `reference_traces` (082 ×2); `modularity_counts_a_negative_community` |
| 105 node similarity | GDS `similarity/nodesim/NodeSimilarity.java`, `*SimilarityComputer.java`, `NodeSimilarityBaseConfig` | `pr` Jaccard (topK 3), overlap, weighted, weighted cosine; `cm` cosine, UNDIRECTED (topK 2); `nil` | fixed | `similarityCutoff` above 1 accepted (GDS [0, 1]). Vectors and per-source lists conform | `reference_traces` (105 ×6); `parameter_domains_as_gds` |
| 106 KNN | MAGE `knn_module/algorithms/knn.hpp` `CalculateKNN`, `UpdateNN`; GDS `similarity/knn/JoinNeighbors.java` (random joins), `NeighborList.java` `add`:107-115, `KnnBaseConfig` | `pts` (scalar `x`), seeds 3, 9 and 0 (ported generator) | fixed | (1) `perturbationRate` let any candidate no better than a full list's worst replace it; GDS only one of equal similarity; (2) `sampleRate` 0 accepted (GDS (0, 1]). Samples, joins, proposals, updates and lists of every round conform | `reference_traces` (106 ×3); `knn_perturbation_replaces_only_an_equal_neighbour`; `parameter_domains_as_gds` |
| 107 vector similarity | GDS `SimilaritiesFunc.java` (formulas) | two vectors; two integer lists | conforms | none | `reference_traces` (107 ×6, rows) |
| 112 Adamic-Adar | GDS `alpha/alpha-proc/.../linkprediction/LinkPredictionFunc.java`, `NeighborsFinder.java` | `cm`: 4 pairs, BOTH, OUTGOING, INCOMING | conforms | none; unverified: the open-source `noCommonNeighbors` counts node2 as a common neighbour through its self-loop, lpg does not (not run on GDS 2026.09.0) | `reference_traces` (×4, rows) |
| 113 common neighbours | as 112 | `cm`: 5 pairs | conforms | as 112 | `reference_traces` (×5, rows) |
| 114 preferential attachment | as 112 | `cm`: 5 pairs | conforms | none | `reference_traces` (×5, rows) |
| 115 resource allocation | as 112 | `cm`: 5 pairs | conforms | as 112 | `reference_traces` (×5, rows) |
| 116 same community | `LinkPredictionFunc.sameCommunity` | `cmc`: 2 pairs | conforms | none | `reference_traces` (116 ×2, rows) |
| 117 total neighbours | `NeighborsFinder.findNeighbors` | `cm`: 5 pairs | conforms | none | `reference_traces` (×5, rows) |
| 083 weakly connected components | Kùzu `weakly_connected_components.cpp`, NetworKit `ParallelConnectedComponents.cpp` (minimum-label rounds), LAGraph `LG_CC_FastSV6.c` (pointer jump) (pass C: `trace_ref_c.py` `wcc_ref`, `trace_c.json`, `tests_trace_c.rs`); sequential: GDS's union-find partition (`wcc/Wcc.java`), asserted equal | `cc` (9 nodes: two SCCs, self-loop, edge without weight, parallel edge, isolated node); `cs` (`cc` with seeds); `one`; `nil` | conforms | none: the labels after each minimum and each jump phase equal at c 1 and 16. Unseeded components under `seedProperty` take ids in root order; GDS gives them in `setIdOf` call order (`HugeAtomicDisjointSetStruct.java:131-147`): ids may differ, partitions are equal (rule P) | `tests_trace_c::reference_traces` (083 ×4: default, `threshold` 1.0, seeds, `consecutiveIds`; c 1 and 16; one node) |
| 084 connected components | petgraph 0.8.3 `unionfind.rs` (`union`: rank; `find_mut`: path halving; `into_labeling`), ported; NetworkX `connected_components` | `ug` (13 nodes: K4, triangles, bridge, parallel pair, self-loop, isolated node, second component) | conforms | none: 35 unions with their merged flags and the labeling equal | `reference_traces` (084) |
| 085 strongly connected components | petgraph `scc/tarjan_scc.rs` `visit` (Pearce's variant), ported; NetworkX `strongly_connected_components` | `cc` | conforms | none: components in petgraph's emission and stack order | `reference_traces` (085) |
| 086 condensation | as 085; petgraph `condensation` (`mod.rs:477`); NetworkX `condensation` (sizes, edge count) | `cc` | conforms | none: components, then successor lists | `reference_traces` (086) |
| 089 biconnected components | Hopcroft and Tarjan's edge stack (NetworkX `biconnected.py` `_biconnected_dfs`, rustworkx `connectivity/biconnected.rs`), recursive; NetworkX `biconnected_component_edges` | `ug` | conforms | none: tree edges, back edges and each popped component in order | `reference_traces` (089) |
| 090 articulation points | Tarjan's low-link search with GDS's parent rule (`ArticulationPoints.java`; `Bridges.java` `visitEvent`); NetworkX `articulation_points` | `ug` | conforms | none: parent skip, tree, back and finish steps with low value and subtree size equal. GDS's search takes neighbours from an event stack (reverse order): same points, bridges and (on karate, zipf60, uniform60) bridge orientation and sizes | `reference_traces` (090) |
| 091 bridges | as 090; bridges by edge removal (brute force) | `ug`, also `REVERSE` (U entry) | conforms | none | `reference_traces` (091 ×2) |
| 093 k-core | synchronous peeling as documented (`components.rs:63-75`); sequential: Batagelj and Zaversnik's peeling, core values asserted equal; GDS `kcore/KCoreDecomposition.java` `compute`, `KCoreDecompositionTask.java` `act` | `ug`; `one` | conforms | none: each round's level and removed nodes at c 1 and 16; rounds against GDS's phases explained in finding C3 | `reference_traces` (093, c 1 and 16) |
| 120 triangle count, listing | per-node sorted-row intersections (graphina `parallel/triangles.rs`, LAGraph `LAGr_TriangleCount.c`; GDS `IntersectingTriangleCount.java`); NetworkX `triangles` | `ug`, also `maxDegree` 3; `one` | conforms | none: per node each neighbour's common count and the total | `reference_traces` (120 ×4 with listing; c 1 and 16) |
| 121 clustering coefficient | as 120, then `2t / (d (d − 1))` (GDS `LocalClusteringCoefficient.java`) | `ug`; `one` | conforms | none | `reference_traces` (121, c 1 and 16) |
| 123 eccentricity | graphalgs `metrics.rs` `eccentricity` (BFS per node), `weighted_eccentricity` (Bellman-Ford); NetworkX distances | `cc` unweighted; `ug` weighted (NATURAL) | conforms | none: each source's distance vector | `reference_traces` (123 ×2) |
| 128 girth | NetworkX `cycles.py` `girth` (:1212-1232; running minimum per source asserted); directed: NetworkX `simple_cycles` | `ug` (self-loop), `ug5` (`ug` without it) UNDIRECTED; `cc` NATURAL; `one`; `nil` | conforms | none | `reference_traces` (128 ×4); `errors_of_the_pass_c_entries` (empty: `Infinity`) |
| 143 Laplacian matrix | graphalgs `spec/laplacian.rs` `laplacian_matrix` (accumulation order) | `ug` UNDIRECTED weighted; `cc` NATURAL; `one` | conforms | none | `reference_traces` (143 ×3) |
| 147 minimum spanning tree | GDS `spanningtree/Prim.java` `compute` with `HugeLongPriorityQueue`'s sift rules; NetworkX MST weight; GDS run on `wt` (`gds_probes_71c.sh`): rows equal | `wt` (8 nodes) minimum, maximum; `ug` (parallel pair); unweighted | conforms | none: each pop (node, parent, cost) and key update | `reference_traces` (147 ×4) |
| 149 k-spanning tree | GDS `kspanningtree/KSpanningTree.java` `growApproach`; GDS runs on `wt`, `kq`, `kd` | `wt` k 3, 5 (root replaced), maximum 4; `kq`; `kd` | conforms | none in the algorithm: karate k 5 (lpg 7, GDS 5) and both rare branches in findings C2 and C4; GDS's tree equals lpg's on every trace graph but `kq`, where GDS writes 2 nodes for k 4 | `reference_traces` (149 ×5, written tree checked) |
| 151 number of spanning trees | matrix-tree theorem (graphalgs `spec/st.rs:79`); exact determinant (Python `fractions`) | `wt`; `ug5` rooted at 3; `cc` rooted at 0; `ring130` (bound above M); `chain12` (30^12 > 2^53); `chain13` (30^13 > INTEGER: 22003); `one` | conforms | none: both residues and the CRT residue exact; finding C5 | `reference_traces` (151 ×6); `count_spanning_trees_decides_exactly` |
| 153 Steiner tree | GDS `ShortestPathsSteinerAlgorithm.java`, `SteinerBasedDeltaStepping.java`, `SimpleRerouter.java`; sequential: Dijkstra from the tree before each join, asserted equal; GDS runs on `st`: rows equal in all three cases | `st` (9 nodes) weighted, `applyRerouting` (one reroute, one cut), unweighted (hop ties) | conforms | none: each Bellman-Ford round's state, each join (target, distance, path), reroutes and cuts at c 1 and 16; uniform60 in finding C6 | `reference_traces` (153 ×3, c 1 and 16) |
| 154 prize-collecting Steiner tree | Goemans-Williamson growth (`pcst_fast.cc`; GDS `GrowthPhase.java` `grow`), GDS `StrongPruning.java` `performPruning`; GDS runs on `pc`, `pp` | `pc` (7 nodes); `pp` (3 nodes) | conforms | none: each deactivation and merge (time, clusters), each peeled leaf, the best node. The rare branch (detach of the best node): finding C4; on `pp` GDS returns no node | `reference_traces` (154 ×2) |
| 155 maximum flow | synchronous push-relabel as documented (`flow.rs:13-29`), each round replayed operation by operation on Goldberg and Tarjan's generic algorithm (admissible arc, at most min(e, r), relabel without admissible arc, valid labelling after each step); NetworkX `maximum_flow_value` | `fl8` (8 nodes: parallel edge, self-loop, zero capacity, back edge), 0 → 7 and {0, 1} → {6, 7} | conforms | none: each round's pushes, heights and excesses at c 1 and 16 | `reference_traces` (155 ×2, c 1 and 16) |
| 157 minimum-cost flow | NetworKit `flow/SuccessiveShortestPath.cpp` (`computeNodePotentials`, `dijkstraOnResidualGraph`); NetworkX `max_flow_min_cost` cost | `fl8` as 155 | conforms | none: each potential pass and augmenting path (bottleneck, distance) | `reference_traces` (157 ×2); `errors_of_the_pass_c_entries` (negative-cost cycle: 22N11) |
| 161 maximum, maximal matching | petgraph `matching.rs:211` `greedy_matching`, `:250` `non_backtracking_dfs` with `Graph::neighbors` order, ported; maximum: petgraph's Gabow not traced, size against NetworkX `max_weight_matching` (maximum cardinality) | `ug` | conforms | none: the graph handed to petgraph and the greedy mates | `reference_traces` (161 ×2) |
| 163 bipartite matching | MAGE `bipartite_matching.cpp` (`IsSubgraphBipartiteColoring`, `BipartiteMatchingDFS`) in lpg's node order; NetworkX Hopcroft-Karp size | `bp` (7 nodes, a re-routing search); `ug5` (odd cycle: 0); `one`; `nil` | conforms | none: colours, each descent, each search's free node | `reference_traces` (163 ×3) |
| 169 maximal cliques | petgraph `maximal_cliques.rs` `bron_kerbosch_pivot` with lpg's first pivot; NetworkX `find_cliques` | `ug`; `nil` | fixed | the empty projection returned one row with the empty clique (petgraph's result); NetworkX's `find_cliques` yields none (`clique.py:250-251`). Pivots and cliques on `ug` conform | `reference_traces` (169; empty); `maximal_cliques_of_the_empty_projection_are_none` |
| 171 clique counting | per-node cliques among the neighbours by increasing index; NetworkX `enumerate_all_cliques` (global counts) | `ug`; `one` | conforms | none | `reference_traces` (171, c 1 and 16) |
| 173 greedy colouring | NetworkX `greedy_coloring.py` strategies (smallest index where NetworkX is arbitrary; lpg's generator for `random_sequential`); NetworkX `greedy_color` with the same order, and with its own strategy for the four without arbitrary choice | `ug`, 7 strategies | conforms | none: the order, then each colour | `reference_traces` (173 ×7); `errors_of_the_pass_c_entries` |
| 174 DSatur colouring | petgraph `coloring.rs:63` `dsatur_coloring`, with Rust's `BinaryHeap` sift rules (`binary_heap/mod.rs`), ported | `ug` | conforms | none | `reference_traces` (174) |
| 176 K-1 colouring | Jones and Plassmann's rounds (SIAM J. Sci. Comput. 14(3), 1993); sequential: greedy colouring in priority order, asserted equal | `ug`; `cc` (both directions); `one` | conforms | none: each round's coloured nodes at c 1 and 16 | `reference_traces` (176 ×3, c 1 and 16) |
| 177 greedy feedback arc set | petgraph `feedback_arc_set.rs` `good_node_sequence` (Eades, Lin and Smyth 1993), ported; acyclic remainder (NetworkX) | `cc`; `st` | conforms | none: the edges handed to petgraph and the arcs | `reference_traces` (177 ×2) |
| 178 set cover | classic greedy (Johnson 1974; Chvátal 1979), MAGE's element-set pairing (`python/set_cover.py`) | `bp`: 8 pairs, two ties | conforms | none | `reference_traces` (178); `errors_of_the_pass_c_entries` |

`edge_cases_and_errors` (all entries): the empty projection, one node with a self-loop, and the error codes (22N11 missing or unknown key, node outside the projection, `maxDepth` below −1, negative cost, negative cycle, no target; 22G03 wrong type).

### Findings of pass A

1. **ALG-006** (fixed). GDS starts no walk from a node without outgoing edges (`traversal/RandomWalkTask.java` `run`: `if (graph.degree(nodeId) == 0) continue;`); lpg returned `walksPerNode` one-node walks for it (measured: `[0, 0, 4, 4]` start nodes for `sourceNodes` [4, 0] on a DAG where node 4 has no outgoing edge). Fix: such start nodes are skipped (`traversal.rs` `random_walk`). The existing `random_walk_quality` expected 120 walks on `zipf60` (60 × 2); 10 of its nodes have no outgoing edge, so it now expects 2 × 50.
2. **ALG-025** (fixed). A negative self-loop is a negative cycle, which fails 025 with 22N11 (TS-25 Costs); lpg returned distances (measured on 0→1, 1→1 at −1, 1→2: 6 rows, no error; 021 and 024 failed with 22N11). Cause: petgraph 0.8.3 `floyd_warshall` sets every node's distance to itself to 0 after loading the edges (`src/algo/floyd_warshall.rs:284-288`). Fix: `paths.rs` `floyd` fails with 22N11 on a negative self-loop before calling petgraph.
3. **ALG-004** (fixed). `gds.pathSubgraphAll.stream` under `UNDIRECTED` listed every relationship twice and a self-loop four times (measured: `[3, 3, 4, 4, 5, 5, 6, 6]` for 4 relationships), since `out` lists an edge from both ends. Fix: each relationship once (`traversal.rs` `subgraph_all`).
4. **ALG-001, 002** (no divergence). `trace_ref_a.py` runs GDS's BFS and DFS ported from the GDS source on `zipf60` and `uniform60` with two node numberings. On GDS's numbering (Neo4j's import order: numeric `uid`) it reproduces all 7 GDS outputs exactly; on lpg's (element identifiers, assigned by lpg's CSV import in the string order of `id:ID`, measured: dense order 0, 1, 10, 11, …, 19, 2, 20, …) it gives lpg's 7 outputs exactly, 2 of them equal to GDS's. lpg's orders are legal executions of GDS's algorithm. GDS documents BFS as visiting "in order of increasing distance" (`doc/.../algorithms/bfs.adoc:17`) and no neighbour order, which follows its internal node identifiers; matching GDS byte for byte would need lpg to number nodes in Neo4j's import order.

### Reported, not changed (specification or open points)

1. **ALG-004 `pathExpand` and ALG-031 under `UNDIRECTED`** return each path twice: MAGE's DFS follows a node's incoming and then its outgoing relationships, and under `UNDIRECTED` both lists hold every incident edge (measured: `algo.all_simple_paths` from node 0 to node 2 returned 10 rows for 3 distinct paths: each one-edge path twice, each two-edge path four times). MAGE has no orientation; the specification does not say how MAGE's direction filters apply under `UNDIRECTED`. A specification decision is needed.
2. **ALG-006 verification rule Q** says "`walksPerNode` walks per start node"; after finding 1 it holds per start node with an outgoing edge.
3. **ALG-006 zero weights**: when every outgoing weight of a node is 0, GDS's `randomNeighbour` takes the first neighbour (`RandomWalkSampler.java`: `randomWeight <= currentWeight` with both 0) and lpg ends the walk. From the source only; not run on GDS.
4. **ALG-015**: the comment of the fallback branch (`paths.rs` `bidirectional`) says the best meeting found there is the shortest path; the branch is reached only with no meeting (t unreachable), and returns no path, as petgraph does (`bidirectional_fallback_has_no_meeting`, 110 pairs).
5. **Limits of the method.** The reference sequences are Python ports of the cited sources, not instrumented runs of GDS, MAGE or petgraph; they share lpg's adjacency order (type, neighbour, edge identifier), which a step comparison needs. For the entries that call petgraph (009, 018, 020, 021, 025, 030), the mechanical check covers lpg's own steps (the projection handed to petgraph, petgraph's raw result, the rows built from it), compared with petgraph on a `DiGraph` of the same edges; petgraph's internals are not traced. Ties are avoided in the trace graphs except where the case tests them (028).

### Quality gate (pass A)

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 663 passed, 38 skipped, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --check` | clean |
| `cargo deny check licenses` | "licenses ok" |
| `cargo build --release` | trace code absent (literals and symbols above) |
| `Cargo.toml`, `Cargo.lock` | unchanged; no new crate |

### RAM space (pass A)

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | no new file |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 5.4 G | 7.4 G | the build in use (`debug` 7.1 G, `release` 249 M, which held its dependencies before this pass) |

### Pass C: trace cases

Pass C reviewed the 29 entries of #67 and #68 and the blossom matching of `blossom.rs` (ALG-187's), at `e762faf` plus uncommitted changes, on 2026-10-12. `docs/evidence/lab-sprint-5/trace_ref_c.py` (imports pass B's helpers and generator port) writes `src/algo/testdata/expected/trace_c.json` and the CSV fixture `fl8`: 100 reference cases (766 expected events) and 9 blossom cases (120 events); every reference is also asserted against an independent oracle (NetworkX, a sequential formulation or a brute force) before the file is written. `src/algo/tests_trace_c.rs`: 7 tests. GDS 2026.09.0 ran on the trace graphs `st`, `wt`, `kq`, `kd`, `pc`, `pp` (`oracle-cases/gds_probes_71c.sh`, concurrency 1; outputs in `raw/gds-probes-71c.txt`; every container removed). The parallel entries (083, 093, 120, 121, 153, 155, 171, 176) ran at `concurrency` 1 and 16 on projections of one node per block, with more than one worker at 16, and equal their sequential formulation round by round.

### Findings of pass C

1. **C1 ALG-169** (fixed). On the empty projection `gds.maximalCliques.stream` returned one row, `(0, [])`, the empty clique that petgraph's `bron_kerbosch_pivot` returns for empty P and X; NetworkX's `find_cliques` (the 169 oracle) yields none (`clique.py:250-251`). Fix: `matching.rs` `maximal_cliques` searches only when the projection has a node. Regression: `maximal_cliques_of_the_empty_projection_are_none` (fails on the old code with `[[Int(0), List([])]]`, passes after) and the trace case `gds.maximalCliques.stream empty`.
2. **C2 ALG-149 karate k 5 (lpg 7, GDS 5): node numbering and adjacency order, not the algorithm.** A port of GDS's `growApproach` and `Prim` with `HugeLongPriorityQueue`'s rules (scratch, not committed: GPL) reproduces all 6 native GDS k-tree outputs and 5 of the 6 `.o` outputs; the sixth (karate k 5) is reproduced only when neighbours are taken in descending order: the Cypher-aggregation projection of the `.o` runs does not list them ascending as the native one does. With ascending neighbours on lpg's numbering, GDS's rules give lpg's tree (weight 7); lpg's code on GDS's numbering gives GDS's native tree (`[0, 7, 12, 28, 31]`). The step where the two first differ on lpg's numbering is a tie between leaves of weight 2 (GDS's max-queue top 32, lpg's smallest index 14): both reject the next node. Karate's integer weights tie; the tree of weight 5 (`[0, 12, 19, 20, 33]`) is the result of the other tie choices.
3. **C3 ALG-093 rounds (7, 17, 11, 42) against GDS (4, 8, 4, 20).** GDS's `compute` runs one SCAN per loop iteration and, when a node of exactly the scanning degree remains, one ACT phase that removes the level's whole cascade through a stack (`KCoreDecompositionTask.java` `act`, `relax`); otherwise the SCAN jumps to the next degree. lpg removes one synchronous wave per round. Computed from the core values (`trace_c.json` `analysis`): lpg's rounds are the sum over levels of the cascade depth (karate 1 + 1 + 2 + 3 = 7; zipf60 17; uniform60 11; zipf300 42); GDS's loop iterations are its ACT phases (4, 7, 4, 18, the distinct core values of nodes with an edge) plus the SCANs that jump over a missing level (0, 1, 0, 2): 4, 8, 4, 20, the numbers of #67. Core values equal GDS's on all four fixtures (asserted).
4. **C4 the rare branches of 149 and 154, triggered and run on GDS.**
   * 149, a node whose tree parent left (graph `kd`, k 4): lpg drops node 3; GDS compares it with the worst leaf and rejects it; both write the tree `[0, 2, 4, 5]` (GDS measured). In 100 000 random graphs (3 to 13 nodes, distinct weights) the GDS port and lpg give different trees only in the 351 runs where GDS re-queues a new root (next point).
   * 149, the root trimmed (graph `kq`, k 4): GDS adds the new root to its growth queue (`KSpanningTree.java:213-218`, `priorityQueue.add(root, …)`; the comment says it should become a trim candidate), later re-adds it and trims a leaf for it: GDS 2026.09.0 writes a tree of 2 nodes (`[1, 2]`) and reports `effectiveNodeCount` 4 (measured). lpg makes the new root a leaf candidate with its child's weight and writes 4 nodes (`[1, 2, 4, 5]`). Rule V asks for a tree of k nodes: lpg kept, GDS's behaviour is a defect.
   * 154, the best node below a root (graph `pp`): GDS's `pruneUnnecessarySubTrees` prunes every root but the best node; when the best node was kept under a parent, it is pruned with that root's subtree, and GDS 2026.09.0 returns no node (`effectiveNodeCount` 0, `sumOfPrizes` 0, measured; the stream is empty in both since a root has no row). lpg detaches the best node and keeps its subtree: node 1 (prize 10). In 40 random graphs of 3 to 6 nodes the case arose 5 times. lpg kept: strong pruning returns the subtree of best value.
5. **C5 ALG-151's `f64` decision** (verified, not replaced). nalgebra's estimate is read only when the diagonal bound reaches M ≈ 2^123; it can mislead only by underestimating a count ≥ M by a factor above 2^23. Measured: the estimate's relative error is 2.1e-14 on `ring130` (bound 2^129, count 130) and 2.5e-15 on `chain12` (count 30^12 = 531441000000000000 > 2^53); a matrix of determinant M + P1 − 1, whose residue P1 − 1 lies inside `INTEGER`, fails as it must (`exact_det` returns none: 22003). Counts equal the exact rational determinant on all six trace graphs. Not proved: a general error bound for nalgebra's LU determinant.
6. **C6 ALG-153 uniform60 unweighted (lpg 9, GDS 10): tie choices.** A port of GDS's `SteinerBasedDeltaStepping` (one task) reproduces GDS's `.o` output exactly when neighbours are taken in edge insertion order, and gives 11 edges with ascending neighbours; lpg gives 9. The first difference is the first join: GDS takes target 23, lpg target 3, both at hop distance 2. On the trace graph `st`, with and without weights (hop ties included) and with rerouting, GDS 2026.09.0's rows equal lpg's.
7. **C7 blossom (`blossom.rs`).** On NetworkX's 9 blossom test graphs, every blossom formed (id, base, vertices), expanded, augmentation, augmentation through a blossom and dual change (type, amount) equals NetworkX 3.7's `max_weight_matching` instrumented the same way (`blossom_steps_follow_networkx`). The port's `>=` where rustworkx tests `> num_nodes` in `augment_blossom` changes nothing: ids come from the end of the free list `n..2n` and return on expansion, and at most ⌊(n − 1)/2⌋ blossoms exist at once (a laminar family, each member with at least three children), so id `n` is never taken; measured on 3 000 random graphs, 2 774 blossoms, smallest id − n = 2 (`blossom_never_allocates_blossom_n`). The #67/#68 finding "misses the blossom numbered `num_nodes`" names no reachable defect.

### Reported, not changed (pass C)

1. **Specification (#70):** rule V of 149 and 154 names GDS as the oracle; lpg departs from GDS's code where GDS's result breaks the rule (C4); the spec should state lpg's behaviour. 083 with `seedProperty`: GDS numbers unseeded components in `setIdOf` call order, lpg in root order; the spec should state rule P for them or lpg's order. TS-25 does not say that a procedure returns no row on an empty projection (C1 relied on NetworkX).
2. **Laboratory:** the `.o` oracle runs (Cypher-aggregation projection in lpg's node order) do not list neighbours as GDS's native projection does (C2, C6), so they are not GDS on lpg's adjacency for tie-dependent heuristics.
3. 153: GDS stores a path edge's cost as a difference of cumulative distances (`processPath`), lpg the edge weight; a rerouting test `w < cost` on equal weights could then differ by rounding. Not observed.
4. Limits: references are Python ports of the cited sources; petgraph's Gabow matching (161 maximum) and nalgebra's determinant are not traced (size and result checked); GDS's rules are read from source and confirmed by the GDS runs listed above.

### Quality gate (pass C)

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 680 passed, 38 skipped, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --check` | clean |
| `cargo deny check licenses` | "licenses ok" |
| `cargo build --release` | trace code absent: in `liblpg.rlib` the trace literals `augblossom`, `deactivate`, `residue`, `jump `, `peel`, `dsatur:` and the symbol `5trace4push` occur 0 times; each occurs at least once in the test binary |
| `Cargo.toml`, `Cargo.lock` | unchanged; no new crate |

### RAM space (pass C)

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | GDS probe inputs and outputs (scratchpad) deleted after the evidence was written |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 9.5 G | 8.0 G | 7.0 G of stale `debug/incremental` deleted first (2.5 G); the build in use remains (`debug` 7.8 G, `release` 249 M) |

### Review totals (passes A, B and C)

The review table has 97 rows, one per entry. Pass A: 37 entries, 34 conform, 3 fixed (004, 006, 025); 126 cases, 1 048 events. Pass B: 31 entries, 21 conform, 10 fixed (045, 059, 061, 062, 064, 078, 080, 082, 105, 106); 126 cases, 1 582 events. Pass C: 29 entries, 28 conform, 1 fixed (169); 100 cases and 9 blossom cases, 886 events. In total 83 entries conform and 14 were fixed, each fix with a regression test that fails on the code before it.

## Exit check (rmp #70)

The exit condition of Sprint 5 (sprint DECISION 20, amended by UPDATE 57: timing deferred to Sprint 6) checked on the final code. **Tested commit: `dff277c`** (HEAD of `feature/5-graph-algorithm-library-all-97-inventory-entries` at the time of the runs, 2026-10-12) plus the uncommitted changes of #70, which add tests, laboratory instruments and one documentation comment and change no product code: `src/algo/tests_exit.rs` (new), one `#[cfg(test)]` module line in `src/algo/mod.rs`, the memory probe of `src/algo/tests.rs`, the FR-ALG-001 table in the documentation of `Session::run_algorithm` (`src/api/session.rs`), [`oracle-cases/gds_cases_70.sh`](oracle-cases/gds_cases_70.sh), [`oracle-cases/det_compare.py`](oracle-cases/det_compare.py), two GDS outputs in `src/algo/testdata/expected` [`raw/gds-determinism-70.txt`](raw/gds-determinism-70.txt) and [`raw/exit-check-70.txt`](raw/exit-check-70.txt). Toolchain as in [Tools](#tools) (rustc 1.99.0, nightly 1.101.0 `080cc2714`, cargo-nextest 0.9.148, cargo-llvm-cov 0.9.1, heaptrack 1.5.0). Correctness only: no timing was measured. Graph data lived in `/tmp/lpgram` (tmpfs); no durability or I/O claim rests on these runs.

### Instruments added

| Test or tool | What it checks |
|---|---|
| `algo::tests_exit::all_97_entries_meet_the_exit_conditions` | One table of 111 calls: the stream routine of every entry (the write routine of ALG-149, which has no stream) and the 12 functions. It asserts that the table names exactly the registered algorithm routines and covers the 97 inventory numbers, and that `Entry::parallel` equals the parallel column of `algorithm-entries.md`. For each call: rows bit-equal at `concurrency` 1 to 16, twice each; the worker count of each run; `CALL` and `Transaction::run_algorithm` equal in one transaction; under a 4 096 B transaction memory limit, 51N73, then 25N05 on the next statement. Graph: 11 nodes and 22 edges with every property an entry reads (`weight`, `capacity`, `cost`, `neg`, `lat`, `lng`, `vec`, `p`, `c`), two labels, two edge types, a DAG subgraph, a bridge, a negative cycle under `neg`, so that every routine returns at least one row. 3.1 s. Per-routine output: [`raw/exit-check-70.txt`](raw/exit-check-70.txt). |
| `algo::tests_exit::rounds_against_the_references` | The round counts of ALG-176 and ALG-106 against GDS (condition 6), pinned. |
| `algo::tests_exit::the_documented_entries_are_the_registered_ones` | The FR-ALG-001 table of the crate documentation equals the registry, row by row. |
| `algo::tests::lab_zipf_projection_memory` (extended) | Also measures the buffer-pool frames the projection build adds (`Pool::buffer_frames`, × 8 192 B) and a second, warm build in the same transaction; asserts heap ≤ reservation + pool frames (cold) and heap ≤ reservation with no new frame (warm). |
| [`oracle-cases/gds_cases_70.sh`](oracle-cases/gds_cases_70.sh), [`det_compare.py`](oracle-cases/det_compare.py) | GDS 2026.09.0: `gds.knn.stats` rounds on `zipf60` and `uniform60`; 5 repetitions of each GDS parity procedure on `zipf300`. Five containers in all (the script ran twice), each removed (`docker ps -a --filter ancestor=neo4j:2026.09.0-community` lists none). |

### Condition 1: `CALL` and the Rust API give equal results on one snapshot (FR-ALG-002)

**Met for all 97 entries.** `all_97_entries_meet_the_exit_conditions` runs each of the 111 routines by `CALL` and by `Transaction::run_algorithm` in one transaction and compares the rows bit for bit (for ALG-149's write routine: the summary columns other than the `…Millis` times and `configuration`, then every node's written property); all equal. The family tests (`Lab::det`, rmp #64 to #69) did the same on the oracle fixtures; they pass on the tested commit (quality gate below).

### Condition 2: each entry matches its reference, except the parity entries

**Met, with three divergences stated in the table.** The full suite runs every committed oracle comparison of rmp #63 to #69 and every trace case of rmp #71 (1 048 + 1 582 + 886 expected events): 681 tests pass, none fails. The 82 non-parity entries match their reference under their rule. Three cases do not reach rule V's "objective value equal to the oracle's", and each was attributed by measurement: ALG-004's `<T>` filter of `pathSubgraphNodes` (MAGE's result is undefined: dangling `string_view`, #64 finding 2); ALG-149 on karate, k 5 (lpg's tree weighs 7, GDS's 5: GDS's own algorithm, ported, gives 7 on lpg's node numbering; #71 C2); ALG-153 unweighted on `uniform60` (9 edges against GDS's 10: hop-count ties; #71 C6). The specification items these raise are listed in #71 "Reported, not changed (pass C)". The 15 parity entries (TS-25: 006, 059, 061–065, 076–078, 080, 081, 106, 176, 188) are verified by determinism and their quality measure ([Parity entries](#parity-entries-quality-measures-and-gds-determinism)).

### Condition 3: equal results at every `concurrency` 1 to 16

**Met for all 97 entries.** `all_97_entries_meet_the_exit_conditions`: every routine's rows at `concurrency` 1 to 16, twice each, are bit-equal to the first run's (floats by their bits). Functions take no `concurrency`; they were run 33 times and gave equal values. Randomised entries ran with `randomSeed` 7. The family tests check the same on the oracle fixtures (`Lab::det`, `check_cases`), and #71 compared each parallel entry's trace step by step at `concurrency` 1 and 16.

### Condition 4: parallel entries use more than one and at most `concurrency` workers, the others one (FR-ALG-006)

**Met for all 97 entries.** The 11-node graph gives every projection 11 one-node blocks. Measured by the worker-count probe in `all_97_entries_meet_the_exit_conditions`: each of the 38 parallel entries (42 routines) started exactly `min(concurrency, 11)` workers: 1, 2, …, 10 at `concurrency` 1 to 10 and 11 at 11 to 16; the test asserts this equality. Each of the 59 other entries started at most one worker at every `concurrency` (the statement's thread counts as one). The classification asserted equals `algorithm-entries.md`: 38 parallel, 59 single-thread.

### Condition 5: a projection over the cap fails the statement (FR-ALG-004)

**Met for the 90 entries that build a projection.** Under a 4 096 B transaction memory limit, every one of the 99 projecting routines failed with 51N73, and the next statement of the transaction failed with 25N05 (aborted); a streaming `MATCH` of the 11 nodes ran within the same limit first (`all_97_entries_meet_the_exit_conditions`). The 7 function entries (107, 112–117) build no projection. The memory cap itself (51N72 when the pool refuses) is covered by `algo::tests::filters_snapshot_and_memory_cap` (rmp #63). The accountant's peak never exceeded the cap in any run (table below).

**Memory gap of rmp #63: closed, no accounting change.** Measured with `lab_zipf_projection_memory` (20 000 nodes, 200 000 edges, 64 MiB cap) and `heaptrack.sh … 4algo10projection5build`:

| Measure | Cold build (pages read first) | Warm build (same transaction, pages cached) |
|---|---|---|
| Heap peak on the statement thread (`allocation-counter`) | 18 436 540 B | 13 988 236 B |
| The run's reservation (`Held` peak) | 15 575 040 B | 15 575 040 B |
| Buffer-pool frames added during the build (`Pool::buffer_frames` × 8 192) | 4 448 256 B (543 frames) | 0 B |
| Heap peak minus pool frames | 13 988 284 B | 13 988 236 B |
| Margin of the reservation over the run's own heap | 1 586 756 B | 1 586 804 B |

heaptrack attributes the frames by call stack: under `projection::build`, 539 + 4 allocations from `Pool::grow` (`src/storage/pool.rs:601`, reached through `Pool::fetch` from the projection's `scan`), 4.42 MB + 32.77 KB, the 543 frames above. The pool charges a frame to the accountant itself (`Pool::grow`: `try_charge(PAGE_BYTES)` above its floor, whose frames its fixed charge covers), never to the run. Without frames the build's heap stays below its reservation, and the cold build's heap minus its frames equals the warm build's heap within 48 B. The 2 861 500 B gap is therefore the pool's frames. The run's two uncharged instants by design (`mem.rs`: a reallocation's old buffer, one scan chunk) fit within the reservation here; that they do on every graph is not shown.

**Accountant and heap peaks per family** (`heaptrack.sh <test-binary> <test>`, each test alone; the heap peak is of the whole test, fixture import included):

| Family | Test | Accountant peak | Cap | heaptrack peak |
|---|---|---|---|---|
| #63 PageRank projection, 20 000 / 200 000 | `algo::tests::lab_zipf_projection_memory` | 24 842 105 B | 67 108 864 B | 76.91 MB |
| #64 traversal and paths (10 entries, `zipf300`) | `algo::tests_paths::lab_family64_memory` | 19 787 177 B | 33 554 432 B | 15.19 MB |
| #65 centrality and similarity (11 entries) | `algo::tests_centrality::lab_family65_memory` | 3 476 197 B | 33 554 432 B | 3.58 MB |
| #66 community and clustering (10 entries) | `algo::tests_community::lab_family66_memory` | 3 476 197 B | 33 554 432 B | 3.58 MB |
| #67 components and measures (14 entries) | `algo::tests_components::lab_family67_memory_and_rounds` | 5 269 825 B | 33 554 432 B | 4.23 MB |
| #68 trees, flows, matching, colouring (14 entries) | `algo::tests_flow::lab_family68_memory` | 3 476 197 B | 33 554 432 B | 3.85 MB |
| #69 cycles, isomorphism, structure (6 entries) | `algo::tests_structure::lab_family69_memory` | 17 762 193 B | 33 554 432 B | 25.79 MB |
| All 97 entries on the 11-node graph | `algo::tests_exit::all_97_entries_meet_the_exit_conditions` | 1 118 527 B | 33 554 432 B | not run under heaptrack (`heaptrack.sh` runs ignored tests only) |

The accountant's peak is below the cap in every row. In the #65, #66 and #68 rows the peak is the fixture import's, so those families' own peaks are lower and not separated (as in their sections). The #69 heap peak includes the 187 684 rows the test holds after the statement, which the accountant no longer counts once delivered; this share is still not separated (#64/#69 open point 6).

### Condition 6: convergence of synchronous rounds against the reference

Each iterative parallel entry, its rounds against the reference on the same graphs. The parallel entries without rounds run one pass or independent per-node or per-source work: 006, 024, 027, 029, 034, 035, 038, 039, 059, 081, 082, 105, 120, 121, 171.

| ALG | Rounds counted | Graphs | lpg | Reference | Evidence |
|---|---|---|---|---|---|
| 001 | BFS levels | trace graphs `dir`, `zipf60`, `uniform60` | each level equal to GDS's BFS ported | GDS `BFS.java` levels | #71 `tests_trace_a::reference_traces`, c 1 and 16 |
| 007 | Kahn levels | `dag` | each level equal | NetworkX `topological_generations` | #71 |
| 019 | Bellman-Ford rounds | `zipf60`, `negw30` | 7, 6 | h + 1 = 7, 6 (h from NetworkX's paths) | #64 |
| 023 | light and heavy phases | `dir`, Δ 2 and 3 | each phase equal | Meyer and Sanders 2003 ported; GDS reports no count | #71 |
| 032 | DAG levels | `dag` | each level equal | DAG DP by levels | #71 |
| 040 | iterations | `zipf60`, `uniform60`, karate | 17, 42, 46 | GDS 17, 42, 46 | #65 |
| 042 | iterations | `zipf300`: natural, converged, weighted, scaled | 20, 79, 20, 7 | GDS 20, 79, 20, 7 | #63 |
| 043 | iterations | `zipf60`, `uniform60` | 73, 71 | GDS 73, 71 | #65 |
| 044 | iterations | `zipf60`, `uniform60`, karate | 26, 21, 26 | GDS 26, 21, 26 | #65 |
| 045 | iterations | `zipf60` | 20 | GDS 20 | #65 |
| 061 | levels | `rc24`, karate, `planted60` | 1, 2, 2 | GDS 1, 2, 2 | #66 |
| 062 | levels | same | 1, 2, 2 | GDS 1, 2, 2 | #66 |
| 063 | iterations | same | 2, 4, 5 | GDS 2, 4, 3 | #66; #71: GDS's algorithm on GDS's numbering gives GDS's counts, lpg's rule on lpg's numbering lpg's |
| 064 | iterations | — | `maxIterations` (fixed) | GDS: `maxIterations` supersteps (`SpeakerListenerLPAComputation.java` `compute`: no vote to halt; read, not run) | code |
| 065 | iterations | same as 061 | 3, 3, 4 | GDS 3, 4, 5 | #66; #71: node numbering, as 063 |
| 078 | restarts | — | `iterations` (fixed) | GDS `iterations` | not convergence-based |
| 080 | Lloyd iterations | `pts` (10 points) | each iteration equal | GDS `Kmeans.java` ported; GDS reports no count (`KmeansStatsResult` fields) | #71 |
| 083 | minimum-label and jump rounds | karate, `zipf60`, `uniform60`, `zipf300` | 3, 4, 4, 4 | GDS: no rounds (one union-find pass) | #67 |
| 093 | peeling rounds | same | 7, 17, 11, 42 | GDS phases 4, 8, 4, 20 | #67; #71 C3: lpg's rounds are the cascade depths, GDS's phases its scan and act steps |
| 106 | iterations | `zipf60`, `uniform60` | 7, 6, converged | GDS 5, 4, converged (`concurrency` 1, seed 42) | #70 `rounds_against_the_references` |
| 153 | Bellman-Ford rounds per join | `st` | each round equal to Dijkstra from the tree, sequential | GDS reports no count | #71 |
| 155 | push-relabel rounds | `fl8` | each round replayed on Goldberg and Tarjan's generic algorithm | GDS reports no count | #71 |
| 176 | Jones-Plassmann rounds | karate, `zipf60`, `uniform60`, `zipf300` | 6, 9, 13, 24 | GDS `ranIterations` 6, 3, 3, 4 (speculative colouring and validation passes, a different quantity) | #70 `rounds_against_the_references` |

Where both report a count of the same quantity, lpg's synchronous rounds equal the reference's (040 to 045, 061, 062) or equal the hop-depth bound h + 1 (019). Where they differ (063, 065, 093, 106, 176), the cause is measured (node numbering, tie rule, quantity counted) or, for 106, lpg's sampling differs from GDS's by design (parity entry). No timing was measured: more rounds say nothing about time here.

### Parity entries: quality measures and GDS determinism

The 15 parity entries of TS-25, each bit-equal at `concurrency` 1 to 16 (condition 3), with its quality measure (rule Q) and the reference value on the same graph, from the family sections. The last column is new (rmp #70): each GDS procedure ran 5 times on `zipf300` in one container ([`raw/gds-determinism-70.txt`](raw/gds-determinism-70.txt); rows ordered by node, so only a change of result counts).

| ALG | Quality measure | lpg | Reference value | GDS 2026.09.0, 5 runs |
|---|---|---|---|---|
| 006 | walks valid; first-step frequencies within 4 SE of the weighted probabilities | largest 2.98 SE over 30 000 walks | the probabilities | `concurrency` 4, seed 42: 1 distinct result |
| 059 | `propagationProbability` 1: exact greedy cover; default: spread ≥ GDS − 3 SE (10 000 cascades) | cover 50; spread 10.836 (`zipf60`), 9.018 (`uniform60`) | GDS 50; 10.084 (SE 0.029), 8.880 (SE 0.024) | `concurrency` 4, seed 42: 1 |
| 061 | planted partition of `rc24`; modularity | 0.6875 (planted), karate 0.4188, `planted60` 0.6149 | GDS 0.6875, 0.4188, 0.6143 | `concurrency` 4: 1 |
| 062 | as 061 | 0.6875, 0.4188, 0.6149 | GDS 0.6875, 0.4188, 0.6149 | `concurrency` 4, seed 42: **5 distinct partitions** |
| 063 | as 061 | 0.6875 (planted), 0.3991, 0.5701 | GDS 0.0 (one community), 0.0, 0.3571 | `concurrency` 4: 1 |
| 064 | strongest label of each `rc24` node = its clique | 4 labels, the cliques | GDS (strength 0.2): 30, 42, 79 labels | `concurrency` 4: **5 distinct**; `concurrency` 1: **5 distinct** (GDS refuses `randomSeed`) |
| 065 | as 061 | 0.6875 (planted), 0.3466, 0.5559 | GDS 0.6875, 0.3466, 0.5525 | `concurrency` 4: 1 |
| 076 | planted partition of `rc24` (4 clusters); modularity | planted, 0.6875; karate 0.3715; `planted60` 0.5947 | no runnable reference (TS-25 item E) | not applicable |
| 077 | as 076; ratio cut | planted, 1.3333; karate 0.9379; `planted60` 3.5005 | no runnable reference | not applicable |
| 078 | cut ≥ GDS's; enumerated optimum on 12 nodes | `rc24` 40, karate 61, `planted60` 134–135 (k 2) and 174 (k 4), `cyc12` 19 and 23 | GDS 40, 61, 131, 169, 19, 23; optimum 19, 23 | `concurrency` 4 (seed refused): **5 distinct**; `concurrency` 1, seed 42: 1 |
| 080 | four planted clusters; inertia ≤ GDS's | planted; 20.3279 (KMEANS++, seeds 42 and 7) | GDS 20.3279 | `concurrency` 4, seed 42: 1 |
| 081 | planted clusters with noise; noise labels equal | planted, 4 noise points | GDS: the same | `concurrency` 4: 1 |
| 106 | recall against the exact top-K ≥ GDS's | 1.000; filtered 0.994 | GDS 1.000; filtered 0.961 | `concurrency` 4 (seed refused): **5 distinct**; `concurrency` 1, seed 42: 1 |
| 176 | proper colouring; colours ≤ GDS's | 5, 5, 4, 7 (karate, `zipf60`, `uniform60`, `zipf300`) | GDS 6, 6, 5, 11 | `concurrency` 4: **2 distinct** |
| 188 | valid routes; no improving 2-opt move; ratio to the enumerated optimum | ratio 1.0000 | optimum (9 points) | no runnable reference |

Against the GDS statements of TS-25 ("Parity entries", "Evidence"):

* **Documented as non-deterministic, confirmed:** 106 and 078 gave a different result in each run at `concurrency` 4, where GDS refuses `randomSeed` (rmp #62), and equal results at `concurrency` 1 with a seed, as GDS documents; 064 differed in each run at both values, since GDS refuses `randomSeed` (#66 finding 1) and seeds from the clock otherwise (`SpeakerListenerLPAComputation.java:45-47`, read). For 106 and 078 the runs at 4 also differ in seed, so threads and seed are not separated.
* **Not stated by GDS, observed non-deterministic:** 062 Leiden (5 distinct partitions with a fixed seed at `concurrency` 4) and 176 K-1 colouring (2 distinct colourings, no randomness involved).
* **Not stated by GDS, no difference observed:** 006, 059, 061, 063, 065, 080, 081 gave one result in 5 runs at `concurrency` 4 on 300 nodes. This does not show that they are deterministic; the TS-25 statement for them stays unverified. Their verification in lpg does not depend on it: rule Q compares quality, not equality.

### The 97 entries

Reference and rule from `algorithm-entries.md` Verification; result from the family sections (oracle runs) and the #71 review. Every row also passed `all_97_entries_meet_the_exit_conditions` (conditions 1, 3, 4 and, for the 90 projecting entries, 5).

| ALG | Name | Parallel | Reference | Oracle rule | Result |
|---|---|---|---|---|---|
| 001 | Breadth-first search | yes | GDS `gds.bfs.stream` | V: same node set, each node at its BFS depth | same node sets, BFS depth order (`zipf60`, `uniform60`, #64); #71 conforms: lpg's order is GDS's algorithm on lpg's node numbering |
| 002 | Depth-first search | no | GDS `gds.dfs.stream` | V: a DFS preorder of the projection over the same node set | same node sets, DFS preorder (#64); #71 conforms (as 001) |
| 003 | Neighbourhood at hop | no | MAGE `neighbors.*` | X | equal, 5 cases (#64); #71 conforms |
| 004 | Path expansion and reachable subgraph | no | MAGE `path.*` | X (paths and node sets) | equal, except the repetitions of the `<T>` case of `pathSubgraphNodes` (MAGE dangling `string_view`, #64 finding 2); #71 fixed (`UNDIRECTED` `pathSubgraphAll` duplicates) |
| 006 | Random walk | yes | none (rule Q) | Q: every walk is a walk of the projection, `walkLength` long or ending at a node with no out-edge; `walksPerNode` walks per start node; over 10 000 walks with `returnFactor` = `inOutFactor` = 1, each node's first-step frequencies within 4 standard errors of its weighted out-edge probabilities | valid walks; first steps within 2.98 SE (bound 4) over 30 000 walks (#64); #71 fixed (no walk from a node without out-edges) |
| 007 | Topological sort | yes | GDS | V: a topological order of the same node set; `maxDistanceFromSource` X | equal node sets and distances; GDS's filler rows left out (#64 finding 3); #71 conforms |
| 008 | Ancestors and descendants | no | NetworkX `ancestors`, `descendants` | X | equal (#64); #71 conforms |
| 009 | Reachability test | no | NetworkX `has_path` | X | equal (#64); #71 conforms |
| 011 | Dominators | no | NetworkX `immediate_dominators`, `dominance_frontiers` | X | equal (#64); #71 conforms |
| 013 | Unweighted shortest paths | no | NetworkX `shortest_path_length`, `shortest_path` | X lengths; V paths | equal (#64); #71 conforms |
| 014 | Dijkstra | no | GDS (both procedures) | F costs; V paths | largest difference 0 (#64); #71 conforms |
| 015 | Bidirectional Dijkstra | no | NetworkX `bidirectional_dijkstra` | F cost; V path | 0 (#64); #71 conforms |
| 018 | A* | no | GDS | F costs; V paths | 0 on `geo30` (#64); #71 conforms |
| 019 | Bellman-Ford | yes | GDS | F costs; V paths; `isNegativeCycle` X | 0; negative cycles flagged as GDS (#64); #71 conforms |
| 020 | Negative-cycle detection | no | NetworkX `negative_edge_cycle`, `find_negative_cycle` | X existence; V: the cycle's weight is negative | equal; the cycle's weight is negative (#64); #71 conforms |
| 021 | SPFA | no | NetworkX `single_source_bellman_ford` | F costs; V paths | 0 (#64); #71 conforms |
| 023 | Delta-stepping | yes | GDS | F costs; V paths | 0 (#64); #71 conforms |
| 024 | Johnson | yes | NetworkX `johnson`; petgraph `johnson` | F | at most 4.4e-16 (#64); #71 conforms |
| 025 | Floyd-Warshall | no | NetworkX `floyd_warshall` | F | at most 4.3e-16 (#64); #71 fixed (negative self-loop: 22N11) |
| 026 | Seidel | no | NetworkX `all_pairs_shortest_path_length` | X | equal (#64); #71 conforms |
| 027 | All-pairs shortest distances | yes | GDS | F | 0 (#64); #71 conforms |
| 028 | All shortest paths between nodes | no | NetworkX `all_shortest_paths` | X (path sets) | equal path sets (#64); #71 conforms |
| 029 | Yen's k shortest paths | yes | GDS | F costs in order; V paths | at most 1.5e-16, costs in order (#64); #71 conforms |
| 030 | Length of the k-th shortest path | no | petgraph `k_shortest_path` | F | equal (#64); #71 conforms |
| 031 | All simple paths | no | NetworkX `all_simple_paths` with `cutoff` | X (path sets) | equal to NetworkX and MAGE (#64); #71 conforms |
| 032 | Longest path in a DAG | yes | GDS | F cost; V path | 0 (#64); #71 conforms |
| 034 | Degree centrality | yes | GDS | F | 3.9e-16 (#65); #71 conforms |
| 035 | Betweenness centrality | yes | GDS, without sampling | F | 1.7e-15 (#65); #71 conforms |
| 038 | Closeness centrality | yes | GDS | F | 1.7e-16 (#65); #71 conforms |
| 039 | Harmonic centrality | yes | GDS | F | 1.7e-16 (#65); #71 conforms |
| 040 | Eigenvector centrality | yes | GDS | F | 3.3e-16 (#65); #71 conforms |
| 041 | Katz centrality | no | MAGE `katz_centrality.get` | F (`epsilon`) | 0 (#65); #71 conforms |
| 042 | PageRank | yes | GDS | F | at most 6.9e-16, 8 cases (#63); #71 conforms |
| 043 | Personalised PageRank | yes | GDS `gds.pageRank` with `sourceNodes` | F | 6.9e-18 (#65), 4.2e-17 (#63); #71 conforms |
| 044 | ArticleRank | yes | GDS | F | 3.3e-16 (#65); #71 conforms |
| 045 | HITS | yes | GDS | F | 1.1e-16 (#65); #71 fixed (parameter domains) |
| 059 | Influence maximisation (CELF) | yes | GDS at `concurrency` 1 | Q: with `propagationProbability` 1, the spread equals the exact greedy reachability cover; at the default, the spread of lpg's seed set is at least GDS's minus 3 standard errors, both re-estimated with 10 000 cascades | Q met: p = 1 cover 50 = GDS; spread at least GDS − 3 SE (#65); #71 fixed (`propagationProbability` domain) |
| 061 | Louvain | yes | GDS | Q: the ring of cliques gives its planted partition (P); karate-club modularity recorded with GDS's | Q met: planted partition of `rc24`; modularity at least GDS's (#66); #71 fixed (write of intermediate communities) |
| 062 | Leiden | yes | GDS | Q: as 061 | Q met: as 061, modularity equal to GDS's (#66); #71 fixed (as 061) |
| 063 | Label propagation | yes | GDS | Q: as 061 | Q met: planted partition of `rc24` (GDS: 1 community) (#66); #71 conforms |
| 064 | SLLPA | yes | GDS | Q: on the ring of cliques each node's strongest label is its clique; karate-club result recorded with GDS's | Q met: strongest label = clique on `rc24`, seeds 42 and 7 (#66); #71 fixed (`partitioning` domain) |
| 065 | Modularity optimisation | yes | GDS | Q: as 061 | Q met: planted partition; modularity at least GDS's (#66); #71 conforms |
| 076 | Spectral clustering | no | none (rule Q) | Q: the ring of cliques with `num_clusters` 4 gives its planted partition (P); modularity recorded | Q met: planted partition, modularity 0.6875 (#66); #71 conforms |
| 077 | Balanced-cut clustering | no | none (rule Q) | Q: as 076; ratio-cut value recorded | Q met: planted partition, ratio cut 1.3333 (#66); #71 conforms |
| 078 | Approximate maximum k-cut | yes | GDS at `concurrency` 1 with `randomSeed` | Q: cut weight at least GDS's on the same graph; the optimum, enumerated on graphs of at most 12 nodes, recorded | Q met: cut at least GDS's in 6 cases, the enumerated optimum on `cyc12` (#66); #71 fixed (`k` domain) |
| 080 | K-means | yes | GDS at `concurrency` 1 with `randomSeed` | Q: four well-separated planted clusters give their partition (P); inertia at most GDS's under F | Q met: planted partition, inertia 20.3279 = GDS's (#66); #71 fixed (7 behaviours of GDS) |
| 081 | HDBSCAN | yes | GDS | Q: planted clusters with noise points give their partition (P), noise labels equal | Q met: planted partition and 4 noise points as GDS (#66); #71 conforms |
| 082 | Partition quality | yes | GDS `gds.modularity`, `gds.conductance`; NetworkX `modularity`, `partition_quality`, `constant_potts_model` | F | 8.3e-17; sums equal NetworkX's (#66); #71 fixed (negative community) |
| 083 | Weakly connected components | yes | GDS | P | equal partitions, also ids when seeded (#67); #71 conforms |
| 084 | Connected components | no | NetworkX `connected_components` | P | equal (#67); #71 conforms |
| 085 | Strongly connected components | no | GDS | P | equal (#67); #71 conforms |
| 086 | Condensation | no | NetworkX `condensation` | P members; X component edges | equal (#67); #71 conforms |
| 089 | Biconnected components | no | NetworkX `biconnected_component_edges` | P on edges | equal (#67); #71 conforms |
| 090 | Articulation points | no | GDS | X | equal (#67); #71 conforms |
| 091 | Bridges | no | GDS | X (unordered pairs, `remainingSizes`) | equal (#67); #71 conforms |
| 093 | k-core decomposition | yes | GDS | X | equal, with `degeneracy` (#67); #71 conforms |
| 105 | Node similarity | yes | GDS (both procedures) | F; V where `topK` cuts through equal similarities | equal similarities per source (#65); #71 fixed (`similarityCutoff` domain) |
| 106 | K-nearest neighbours | yes | exact brute-force top-K; GDS at `concurrency` 1 with `randomSeed` | Q: recall against the exact top-K at least GDS's on the same graph | Q met: recall 1.000 (GDS 1.000), filtered 0.994 (GDS 0.961) (#65); #71 fixed (perturbation, `sampleRate`) |
| 107 | Vector similarity functions | no | GDS functions | F | 42 values or errors equal (#65); #71 conforms |
| 112 | Adamic-Adar | no | GDS function | F | equal (#65); #71 conforms; GDS's self-loop rule read, not run |
| 113 | Common neighbours | no | GDS function | F | equal (#65); #71 conforms; as 112 |
| 114 | Preferential attachment | no | GDS function | F | equal (#65); #71 conforms |
| 115 | Resource allocation | no | GDS function | F | equal (#65); #71 conforms; as 112 |
| 116 | Same community | no | GDS function | F | equal (#65); #71 conforms |
| 117 | Total neighbours | no | GDS function | F | equal (#65); #71 conforms |
| 120 | Triangle count and listing | yes | GDS `gds.triangleCount`, `gds.triangles` | X | equal (#67); #71 conforms |
| 121 | Clustering coefficient | yes | GDS | F | 0; average within 1e-9 (#67); #71 conforms |
| 123 | Eccentricity and related measures | no | NetworkX `eccentricity`, `diameter`, `radius` | X unweighted; F weighted | equal (#67); #71 conforms |
| 128 | Girth | no | NetworkX `girth` | X | equal (#67); #71 conforms |
| 143 | Laplacian matrix | no | NetworkX `laplacian_matrix` | F | 1.9e-16 (#67); #71 conforms |
| 147 | Minimum spanning tree | no | GDS | V: total weight F | same node sets, totals within 1e-9 (#68); #71 conforms |
| 149 | k-spanning tree | no | GDS | V: a tree of `k` nodes; total weight F | equal node sets except karate k 5: weight 7 against GDS's 5, a tie choice (#68; #71 C2); #71 conforms |
| 151 | Number of spanning trees | no | determinant of the reduced Laplacian over rationals (Python `fractions`) | X | equal; 22003 above `INTEGER` (#68); #71 conforms |
| 153 | Steiner tree | yes | GDS | V: a tree reaching every reachable target; total weight F | weighted cases equal; unweighted `uniform60` 9 edges against GDS's 10, a tie choice (#68; #71 C6); #71 conforms |
| 154 | Prize-collecting Steiner tree | no | GDS | V: a tree; `totalWeight` and `sumOfPrizes` F | weight equal within 2e-16 relative; prizes equal (#68); #71 conforms |
| 155 | Maximum flow | yes | GDS | V: capacities and conservation hold; `totalFlow` F | flows 46 and 79 as GDS (#68); #71 conforms |
| 157 | Minimum-cost flow | no | GDS | V: as 155; `totalFlow`, `totalCost` F | costs 457 and 564 as GDS (#68); #71 conforms |
| 161 | Maximum and maximal matching | no | NetworkX `max_weight_matching` (`maxcardinality`), `is_maximal_matching` | V: matching size equal; greedy form a maximal matching | sizes equal (#68); #71 conforms |
| 163 | Bipartite matching | no | NetworkX `hopcroft_karp_matching` | X size | equal (#68); #71 conforms |
| 169 | Maximal cliques | no | NetworkX `find_cliques` | X (clique sets) | equal (#68); #71 fixed (empty projection) |
| 171 | Clique counting | yes | GDS | X | equal (#68); #71 conforms |
| 173 | Greedy colouring | no | NetworkX `greedy_color`, same strategy and node order | V: proper colouring; number of colours equal | equal colour counts, 6 strategies (#68); #71 conforms |
| 174 | DSatur colouring | no | petgraph `dsatur_coloring` | V: proper colouring; number of colours equal | equal (5, 5, 4, 3) (#68); #71 conforms |
| 176 | K-1 colouring | yes | GDS at `concurrency` 1 | Q: proper colouring with at most GDS's number of colours | Q met: proper; colours 5, 5, 4, 7 against GDS's 6, 6, 5, 11 (#68); #71 conforms |
| 177 | Greedy feedback arc set | no | petgraph `greedy_feedback_arc_set` | X; V: removing the arcs leaves a DAG | equal; acyclic remainder (#68); #71 conforms |
| 178 | Set cover | no | MAGE `set_cover.greedy` | V: a cover of equal size | 3 sets = exhaustive optimum (MAGE random: 6, 4, 7) (#68); #71 conforms |
| 180 | Cycle detection | no | MAGE `cycles.get` | X (cycles as node sets) | 57 and 57 cycles, equal (#69); #71 conforms |
| 181 | Simple-cycle enumeration | no | NetworkX `simple_cycles` | X (cycles up to rotation) | equal (#69); #71 conforms |
| 187 | Travelling salesman | no | optimum enumerated on graphs of at most 9 nodes | V: a tour of every point; cost at most 2 times (`2_approx`) or 1.5 times (`1.5_approx`) the optimum | within the bounds: worst ratio 1.3274 (2-approx), 1.1149 (1.5-approx) (#69); #71 conforms |
| 188 | Vehicle routing | no | optimum enumerated on graphs of at most 9 nodes | Q: every non-depot node on exactly one route, each route starts and ends at the depot, at most `number_of_vehicles` routes; no 2-opt move shortens a route; ratio to the optimum recorded | Q met: valid routes, no improving 2-opt move, ratio 1.0000 (#69); #71 conforms |
| 189 | Graph isomorphism | no | NetworkX `is_isomorphic` on multigraphs | X | equal (#69); #71 conforms |
| 190 | Subgraph isomorphism | no | petgraph `is_isomorphic_subgraph` | X | equal (#69); #71 conforms |
| 194 | Bipartiteness | no | NetworkX `is_bipartite`, bipartite `sets` | X; V sides | equal; sides a 2-colouring (#69); #71 conforms |
| 203 | Tournament tests | no | NetworkX `is_tournament`, `is_directed_acyclic_graph` (a tournament is transitive exactly when acyclic) | X | equal (#69); #71 conforms |
| 204 | Degree-sequence realisability | no | NetworkX `is_graphical` | X | equal (#69); #71 conforms |
| 205 | Transitive closure | no | NetworkX `transitive_closure_dag` | X | equal (#69); #71 conforms |
| 206 | Transitive reduction | no | NetworkX `transitive_reduction` | X | equal (#69); #71 conforms |

### Coverage

`cargo +nightly llvm-cov nextest --branch --no-fail-fast --lcov` over the whole suite (683 tests pass) on the tested code; branch outcomes counted from the `BRDA` records of the 23 product files of `src/algo/` (the `tests*.rs` and `trace.rs` modules excluded). The coverage build directory (3.8 G) was deleted after each run. **2 118 of 2 324 branch outcomes covered (91.1 %)**:

Per file (covered / total): `blossom.rs` 157 / 162, `centrality.rs` 68 / 74, `clustering.rs` 105 / 124, `community.rs` 189 / 206, `components.rs` 86 / 92, `config.rs` 31 / 42, `cycles.rs` 113 / 126, `flow.rs` 58 / 60, `functions.rs` 32 / 40, `kernels.rs` 77 / 82, `matching.rs` 112 / 114, `measures.rs` 69 / 72, `mem.rs` 2 / 2, `mod.rs` 63 / 76, `pagerank.rs` 52 / 54, `paths.rs` 246 / 268, `projection.rs` 42 / 46, `scale.rs` 11 / 18, `sched.rs` 16 / 18, `similarity.rs` 132 / 148, `structure.rs` 56 / 58, `traversal.rs` 190 / 208, `trees.rs` 211 / 234.

The 206 uncovered outcomes lie on 179 lines. Each line falls in one group (lines of `src/algo/<file>.rs`):

1. **Test-only code in product files** (8 lines, 11 outcomes): a `debug_assert!` (`blossom` 85); assertions of the unit tests inside `config` 587, `sched` 439, `trees` 1250; a `trace!` argument (`measures` 468); the release arm of the worker-count probe, which every test run sets (`components` 388, 878; `paths` 811).
2. **Argument errors not exercised** (22G03 or 22N11; 27 lines, 39 outcomes): `config` 436, 457, 468, 469, 509, 518, 519 (value types of keys); `kernels` 77 (a node given as a negative integer); `matching` 810; `functions` 275, 483; `paths` 422, 432, 725, 1766 (NaN or negative weights, missing coordinates, a non-integer `max_length`); `traversal` 377, 803, 807, 808, 953; `clustering` 328, 1000, 1005 (vector and cluster-count checks); `cycles` 371; `scale` 84, 108 (scaler offsets); `similarity` 449 (`topN` with `bottomN`).
3. **Degenerate inputs the fixtures do not hold** (47 lines, 52 outcomes): zero norms, empty or equal vectors, constant values and NaN in `pagerank` 471, `scale` 144, 160, 174, 182, 190, `similarity` 332, 619, 622, `functions` 180, 242, 249, 252, 258 (the extremes of the exact integer–float comparison), `community` 791, 1261, 1317, 1856, `measures` 324, `trees` 892, 1028, 1157 (NaN prizes); empty or single-node inputs in `clustering` 166, 255, 480, 488, 696, 768, 1075, `trees` 593, 1035, `centrality` 291, 301, `kernels` 162, 262, `cycles` 456, 513, 525, 545, 547 (fewer than three TSP points), `similarity` 882, 899, `projection` 641; no weight property or an isolated node in the spectral Laplacian (`clustering` 976, 1020, 1022); an edge without the projected property (`projection` 435).
4. **Internal invariants and states one transaction cannot produce** (26 lines, 27 outcomes): a corrupt adjacency (`projection` 411); projections of 2^32 nodes or more (`projection` 458); `Config::set` of a key the parser always fills (`config` 369); `let … else` on values present by construction and walk-back guards (`paths` 869, 874, 880, 884, 1446, 1547, 1897; `components` 584; `matching` 1059; `centrality` 837; `trees` 729, 770); a thread that fails to start (`sched` 278); write rows for an element deleted since the projection, a column of another type, or a closed graph type (`mod` 885, 905, 941, 944, 964, 983, 997, 1058, 1087, 1091).
5. **Functional branches no case reaches** (71 lines, 77 outcomes; reachable in principle, not verified): blossom expansion and relabel cases (`blossom` 95, 248, 445); a stale heap entry and a graph without edges (`centrality` 370, 597, 770); a visited HDBSCAN cluster (`clustering` 764); local moving that exhausts `maxIterations`, Leiden stopping by `tolerance`, refinement corner cases, LPA `consecutiveIds`, small-community merging (`community` 998, 1055, 1073, 1190, 1213, 1219, 1223, 1228, 1245, 1412, 1631, 1794); unseeded `consecutiveIds`, a WCC round without change, articulation sizes (`components` 291, 375, 739); MAGE cycle filters, more than 63 fundamental cycles, VRP passes (`cycles` 142, 169, 239, 691, 694); push-relabel without an active node or residual arc (`flow` 339, 397); path-search early exits and ties (`paths` 747, 788, 898, 968, 1397, 1455, 1559, 1587, 1624, 1975); the eigenvector L2 scaler (`pagerank` 227); similarity filters, ties and joins (`similarity` 274, 282, 431, 509, 534, 680, 780, 802, 931); a non-tournament with the edge count of one (`structure` 308); `pathExpand` relationship forms and hop limits, zero walk weights (`traversal` 552, 567, 604, 614, 701, 825, 847, 1030); k-tree and Steiner rare branches, GW events (`trees` 408, 452, 467, 787, 799, 838, 913, 944, 1004, 1048, 1143); Bellman-Ford write with a negative cycle (`mod` 1053, 1054).

### Quality gate

| Command | Result |
|---|---|
| `cargo nextest run --no-fail-fast` | 683 passed, 38 skipped, 0 failed (680 before #70, plus the three `tests_exit` tests) |
| `cargo clippy --all-targets -- -D warnings` | no warning |
| `cargo fmt --all -- --check` | clean |
| `cargo deny check licenses` | "licenses ok" |
| `cargo +1.89 build` | exit 0 |
| `cargo tree -p lpg -e normal` | 47 crates besides lpg; each crate and version is a row of TS-31 (`dependencies.md`), and `once_cell`, `pin-project-lite`, `tracing-core` are named in the `tracing` row; `Cargo.toml` and `Cargo.lock` unchanged |
| `cargo doc --no-deps` | builds; the FR-ALG-001 table renders in `Session::run_algorithm` (97 rows). 7 warnings, all links to private items in files #70 did not change (`src/auth`, `src/gqlstatus`, `src/proc`, `DayToSecond`), as before (#63 gate) |
| Ignored laboratory tests (`heaptrack.sh`, each alone) | the 7 `lab_*memory*` tests pass |

### RAM space

| Location | Before | After | Left and why |
|---|---|---|---|
| `/tmp` | 2.5 G used | 2.5 G used | heaptrack recordings, coverage data and GDS outputs in the session scratchpad deleted after use |
| `/tmp/lpgram` | empty | empty | every test removes its directory |
| `/dev/shm/lpg-target` | 8.2 G | 8.8 G | the build in use (`debug` 8.5 G, of which `debug/incremental` 6.4 G, shared by the test, clippy and 1.89 builds; `release` 249 M; `doc` 18 M); each `llvm-cov-target` (3.8 G) deleted after its run |

### Deferred to Sprint 6 (timing, UPDATE 57)

* Time per entry for all 97 entries, on the reference hardware, with projections read from persistent storage.
* Speedup at 1, 2, 4, 8 and 16 workers for the 38 parallel entries.
* `perf stat` cache-miss rates on projections.
* Flame graphs per family.
* Whether the extra synchronous rounds of 019, 093 and 176 (condition 6) cost time against the references.

### Evidence for the specification update of unverified point 9

For the technical-writer pass on `quality-and-testing.md` point 9 (and TS-25 where it states the GDS determinism statements):

1. **Classification.** Closed by rmp #62 (38 parallel, 59 single-thread). Re-checked on the tested code: `algo::tests_exit::all_97_entries_meet_the_exit_conditions` asserts `Entry::parallel` equal to `algorithm-entries.md` for all 97 entries, and measures exactly `min(concurrency, 11)` workers for every parallel entry and at most one for the others, at `concurrency` 1 to 16 ([condition 4](#condition-4-parallel-entries-use-more-than-one-and-at-most-concurrency-workers-the-others-one-fr-alg-006)).
2. **lpg's determinism.** All 97 entries (111 routines) bit-equal at `concurrency` 1 to 16, twice each, in the same test ([condition 3](#condition-3-equal-results-at-every-concurrency-1-to-16)); the family tests on the oracle fixtures (`Lab::det`, `check_cases`, rmp #63 to #69); #71's step-by-step traces of the parallel entries at `concurrency` 1 and 16.
3. **GDS determinism statements** ([parity entries](#parity-entries-quality-measures-and-gds-determinism); [`raw/gds-determinism-70.txt`](raw/gds-determinism-70.txt), GDS 2026.09.0, `zipf300`, 5 runs): confirmed non-deterministic as documented: 106, 078 (at `concurrency` 4; equal at 1 with a seed), 064 (at 4 and at 1, no seed accepted); observed non-deterministic, not documented by GDS: 062 (seeded), 176; no difference in 5 runs, so still unverified: 006, 059, 061, 063, 065, 080, 081. Quality measures and reference values of all 15 parity entries in the same table.
4. **Convergence counts** ([condition 6](#condition-6-convergence-of-synchronous-rounds-against-the-reference)): equal to GDS for 040, 042, 043, 044, 045 (iterations) and 061, 062 (levels); 019 h + 1 rounds against NetworkX's hop depth h; 063 (2, 4, 5 against GDS's 2, 4, 3), 065 (3, 3, 4 against 3, 4, 5) explained by node numbering and tie rule (#71); 093 (7, 17, 11, 42 against GDS phases 4, 8, 4, 20, #71 C3); 106 (7, 6 against 5, 4); 176 (6, 9, 13, 24 Jones-Plassmann rounds against GDS's 6, 3, 3, 4 passes, a different quantity); per-round equality with the ported reference for 001, 007, 023, 032, 080, 153, 155 (#71); no reference count for 064, 078 (fixed counts), 083, 153, 155.

### Unverified points and limits

1. The GDS determinism statements for 006, 059, 061, 063, 065, 080, 081 (no difference in 5 runs). For 106 and 078 at `concurrency` 4 the seed differs between runs too.
2. That the run's uncharged reallocation instant and scan chunk stay within its reservation on every graph; shown for the measured projection only.
3. The #69 heap share of the rows held by the test after the statement (#64/#69 open point 6).
4. The three rule-V divergences of condition 2 (004 `<T>`, 149 karate k 5, 153 `uniform60` unweighted) await the specification decisions #71 reported.
5. 064's GDS superstep count is read from source, not run; 080's GDS iteration count is not reported by GDS.
6. The 77 functional branch outcomes of coverage group 5 are not reached by any test.
