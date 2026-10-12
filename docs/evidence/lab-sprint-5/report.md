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
