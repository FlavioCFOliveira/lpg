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
5. The write modes of 013, 014 (single pair), 015, 018, 021, 023, 028 and 029 share the PathWrite code that the 014 all-paths write test covers; they were not run one by one.
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
3. The Louvain and Leiden write modes write `communityId` even with `includeIntermediateCommunities`; GDS writes the list then (documentation, not measured). HITS's `nodePropertiesWritten` counts two properties per node; GDS's count is not measured.
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
5. **rustworkx's `augment_blossom` tests `> num_nodes`**, which misses the blossom numbered `num_nodes`; NetworkX tests any blossom; lpg tests `>=`.
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
