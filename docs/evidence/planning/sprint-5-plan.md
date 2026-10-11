# Sprint 5 plan: graph-algorithm library (draft for task #5)

Prepared 2026-10-12 on branch `feature/5-graph-algorithm-library-all-97-inventory-entries` at `f56e44f`. Read-only for the repository except this file and [sprint-5-reuse-map.md](sprint-5-reuse-map.md); no `rmp` record was changed while drafting; the coordinator then entered the tasks as `rmp` #61–#70 (§10). The laboratory tools were updated (§9).

## 0. Basis, method and limits

* **Read:** `CLAUDE.md`, `README.md`; `rmp task get 5`; `rmp sprint get 5`; sprint comments #18 (scope), #19 (order and synergy), #20 (exit), #21 (laboratory), #36, #49, #57 (UPDATE: timing deferred to Sprint 6); the Sprint 4 plan (`sprint-4-plan.md`, the model for this file); the Sprint 4 laboratory report (`docs/evidence/lab-sprint-4/report.md`, Tools).
* **Specification read:** `docs/technical-spec/algorithms.md` (TS-25), `dependencies.md` (TS-31), `interfaces.md` (TS-24), `architecture.md` (TS-01), `quality-and-testing.md` (unverified points 1–19); `docs/functional-spec/functional-requirements.md` (FR-ALG-001..007, FR-LIM-001..007), `non-functional-requirements.md` (NFR-REL-004, NFR-PERF-007), `use-cases.md` (UC-ALG-001), `graph-algorithms.md` (the inventory and its derived names).
* **Code read:** `Cargo.toml`, `deny.toml` and the items cited in §2.
* **Reuse map:** [sprint-5-reuse-map.md](sprint-5-reuse-map.md), produced for this plan from the reference clones under `/tmp/<project>` (petgraph, rustworkx, graphalgs, pathfinding, graphrs, graphina, graspologic-native, hdbscan, linfa, NetworkX, NetworKit, Kùzu, MAGE, Memgraph, cuGraph, LAGraph, GDS, igraph, Raphtory, grafeo, minigu). This plan cites it and does not repeat it.
* **Official sources fetched 2026-10-12** (User-Agent `lpg-planning (https://github.com/FlavioCFOliveira/lpg)`; no personal data sent): crates.io API (tool and crate versions, licences), PyPI JSON (networkx, numpy, scipy, pip), Docker Hub API (`neo4j`, `memgraph/memgraph-mage` tags).
* **Measured for this plan** (reuse map §6 holds the details):
  * GDS 2026.09.0, bundled in `neo4j:2026.09.0-community`, runs: `gds.version()` = `"2026.09.0"`; 493 procedures, among them HDBSCAN, maximum flow, minimum-cost flow, clique counting and prize-collecting Steiner tree; `concurrency` above 4 fails without a licence key; PageRank is not normalised to sum 1; a native projection keeps parallel edges (node degree 2.0 from two edges to one neighbour) and projects a missing weight as NaN.
  * Probe crate with petgraph 0.8.3, nalgebra 0.35.0 and rand 0.10.3: `cargo tree -e normal` and `cargo deny check licenses` with lpg's `deny.toml` (§3.1 point 12); a compiled run of `SymmetricEigen`, `determinant`, `ChaCha8Rng::set_stream` and `Csr::from_sorted_edges`.
  * NetworkX 3.7 with numpy 2.5.4 and scipy 1.18.1: PageRank, HITS and spanning-tree count on small graphs.
* **Unverified (stated plainly):** whether any copy or port candidate meets lpg's requirements (none was built inside lpg); the convergence of synchronous rounds (closed by the sprint); the MAGE image as an oracle (tag known, image not pulled; whether it contains GEKKO for `vrp` is unknown); the five reuse-map open points not closed here (reuse map §5).

## 1. Requirements digest

### 1.1 Requirements in scope

Scope ids from sprint comment #18, exit ids from #20, synergy from #19, laboratory items from #21.

| Id | Demands (short) | Where |
|---|---|---|
| TS-25 Names | GDS name, else MAGE, else the derived `gds.` name; stream and write modes; no graph-name argument. | `algorithms.md` Names |
| TS-25 One code path | `CALL` and the Rust API reach one function with one parameter record. | One code path |
| TS-25 Projection | Caller's snapshot, label and edge-type filters, dense indexes, CSR by one ordered scan of the adjacency tree; `u32` or `usize` indexes; projection and run state reserved before they are built. | Projection |
| TS-25 Implementations | Parallel entries written in lpg; single-thread entries call petgraph or use its visit traits; dense algebra with `nalgebra` (ALG-076, 143, 151). | Implementations |
| TS-25 Parallelism | `concurrency` (default 4), scoped workers; parallel when a reference has a parallel version; blocks of `max(1, min(4096, ceil(n/64)))` nodes; block-order merge; synchronous rounds. | Parallelism |
| TS-25 Randomness, determinism | `seed` (default 0); one generator per block from `(seed, block index)`; equal results at every `concurrency`. | Randomness, Determinism |
| TS-25 Write-back | Through the caller's write set, bounded by the cap. | Write-back |
| TS-25 Parity | 12 entries verified by determinism and a quality measure: ALG-006, 059, 061, 062, 063, 064, 065, 078, 080, 081, 106, 176. | Consequence for parity |
| FR-ALG-001 | The release lists the entries it provides. | FS:226 |
| FR-ALG-002 | `CALL` and Rust API, equal results on one snapshot. | FS:228 |
| FR-ALG-003 | Stream or write-back in the caller's transaction, within the cap. | FS:230 |
| FR-ALG-004 | Whole graph or label and type filter, caller's snapshot, in-memory projection; over the cap fails. | FS:232 |
| FR-ALG-005 | Every entry of the closed inventory. | FS:234 |
| FR-ALG-006 | Parallel exactly when a reference is; `concurrency` maximum workers, default 4, calling thread not counted. | FS:236 |
| FR-ALG-007 | GDS name, parameters, modes and output columns; MAGE name otherwise; derived names; other modes fail. | FS:238 |
| FR-LIM-001 | Exceeding a limit fails the statement and aborts the transaction. | FS:300 |
| FR-LIM-003 | The memory cap covers query execution (here: projection and run state). | FS:304 |
| FR-LIM-007 | Write-back writes and their old versions fit the cap. | FS:312 |
| NFR-REL-004 (algorithm part) | Every kept inventory entry present and tested. | NFS:62 |
| UC-ALG-001 | Run with `CALL` or the Rust API; stream; A1 write-back; A2 limit exceeded. | `use-cases.md:199-210` |
| Unverified point 9 | Parallel classification of every entry; the GDS determinism statements; convergence of synchronous rounds. | `quality-and-testing.md:155` |

### 1.2 Exit condition without timing

Exit comment #20 has six conditions. UPDATE #57 removes timing: "The timing parts of this sprint's earlier DECISION entries (laboratory measurements, benchmarks, latency and throughput, cache-ratio runs) **no longer apply**; their correctness checks stay. Non-timing measurements that prove correctness (e.g. memory within the cap) remain in scope."

**In Sprint 5:** each entry through `CALL` and the Rust API with equal results; each entry against its reference oracle, or, for the 12 TS-25 parity entries (and any entry the user adds under §3.2 item E), by determinism and a quality measure; equal results at every `concurrency` 1–16; the worker-count rule; a projection over the cap fails; memory (accountant peak and `heaptrack` peak) within the cap; the number of synchronous rounds to convergence against the reference's iteration count (a count, not a time); the parallel classification of every entry with its reference (unverified point 9).

**Deferred to Sprint 6 (timing):** time per entry, speedup curves at 1, 2, 4, 8 and 16 workers, `perf stat` cache-miss rates on projections, flame graphs per family. The tools are recorded in §9 and not run.

## 2. Code reconnaissance

104 269 lines of Rust in `src/` (`find src -name '*.rs' | xargs wc -l`). No `algo` module exists, though TS-01 names one (`architecture.md:40`). `petgraph` and `nalgebra` are in TS-31 but not in `Cargo.toml`.

### What Sprint 5 builds on

| Piece | Where | Use in Sprint 5 |
|---|---|---|
| Procedure registry, `Signature`, `Parameter` with defaults, `ReadProcedure`, `WriteProcedure`, `ReadContext::{check, reserve}`, `WriteContext::execute`, built-ins enum | `src/proc/mod.rs:52-260`; `src/proc/builtin.rs:40-180` | registration of 97 entries in two modes; cancellation; row charging |
| Rows charged per batch by the `ProcedureCall` operator; `Held::grow` reserves in chunks with `check_memory` (transaction limit) and `pool.reserve` (cap) | `src/query/exec/mod.rs:266-303`; TS-24 Built | stream mode; pattern for the run-long reservation |
| Memory accountant, `try_reserve`, `Reservation::{split, shrink, absorb}`, `peak` | `src/storage/memory.rs:29-193` | projection and run state (FR-ALG-004) |
| Snapshot scans `Transaction::scan_prefix`, `scan_range`, `scan_chunk` | `src/tx/mod.rs:579-620` | the projection's one ordered scan, in the caller's snapshot |
| Adjacency key `(node, dir, type, neighbour, edge)`, both directions per edge, `decode_adj`, `expand` | `src/storage/graph.rs:125-160, 529, 615-640` | forward and reverse CSR from one scan (reuse map §3.1) |
| Node and edge records, `set_property`, `set_edge` | `src/storage/graph.rs:342, 416, 548, 563` | label filter, property projection, write-back |
| Session and transaction API, `ResultStream`, `Database::register_*`, `routines` | `src/api/session.rs:529-1190`; `src/api/database.rs:280-339` | Rust API entry point (§3.1 point 9) |
| Workload generators (Zipf, uniform), `property_graph` | `src/lab/generator.rs:34-160` | test graphs |
| Test support: `seed()` (`LPG_SEED`), `graph_dir()` in `/tmp/lpgram`, `thread_names()` from `/proc/self/task`, `values` | `src/test_support/mod.rs:15-110`; `src/test_support/values.rs` | seeds, test directories, worker counts |
| Heap peak per closure (`allocation-counter` 0.8.1, dev dependency) | `Cargo.toml` dev-dependencies; used in `src/api/tests/procs.rs:565` | memory record per run |
| Sprint 4 CSV export and the Neo4j import check | `src/ops/bulk/csv.rs`; `docs/evidence/lab-sprint-4/neo4j_import_check.sh` | loading lpg's test graphs into Neo4j for the GDS oracle |

### Changes to existing code

* **New module `src/algo/`** (TS-01): projection, scheduler, parameter record, modes, registration, and the entries.
* **Run-long reservation.** `ReadContext::reserve` charges only until the current batch of rows is returned (`proc/mod.rs:191`). A projection and its run state must stay reserved for the whole run, so the algorithm built-ins hold their own `Reservation` from the pool, checked against the transaction's memory limit like `Held::grow`.
* **Built-ins with a write mode.** `Builtin` (`proc/builtin.rs:40`) has read-only routines only; `.write` procedures are `ReadWrite` routines.
* **Dependencies:** `petgraph`, `nalgebra` and rand's `chacha` feature join `Cargo.toml`; `deny.toml` adds `Zlib` (§3.1 point 12).

## 3. Specification gaps

### 3.1 Decidable from references (task #62 records each decision)

| # | Gap (cited) | Proposed decision | Reference |
|---|---|---|---|
| 1 | TS-25 builds one `petgraph::csr::Csr`. `Csr` has no incoming-edge access, so 10 petgraph functions do not compile on it; it rejects parallel edges and does not mirror undirected edges. | lpg builds its own forward and reverse CSR arrays (petgraph's `row` and `column` layout) in the one ordered scan, keeps parallel edges, and implements petgraph's visit traits, `IntoNeighborsDirected` and `IntoEdgesDirected` included, on a thin wrapper, so petgraph functions compile unchanged. | Reuse map §3.1 (compile probe; `csr.rs:243, 249-252, 547-860`); lpg writes OUT and IN entries per edge (`graph.rs:529`). |
| 2 | Parallel edges in a projection. | Kept, one CSR entry each (GDS's default aggregation `NONE`). | Measured: GDS 2026.09.0 projected two edges 0→1 and gave node 0 degree 2.0 (§0). |
| 3 | Edge weights and node properties in the projection (`relationshipWeightProperty`, `nodeProperties` of GDS). | The projection loads the edge property named by the parameter as `f64`, and node property arrays as named; an absent or non-numeric value is NaN, as GDS projects it; arrays are charged to the cap. | Measured: GDS projected NaN for an edge without `w` (§0). |
| 4 | Reservation timing: TS-25 reserves "before they are built", but the edge count is known only after the scan. | Reserve in chunks before each growth of an array (the `Held::grow` pattern, `query/exec/mod.rs:283-303`); release at the end of the run. | TS-08 accountant; `Held::grow`. |
| 5 | Read set of a projection under serializable isolation. | The scan registers its intervals like a query scan (TS-15 Read sets); no special case. | `Transaction::scan_range` (`tx/mod.rs:596`). |
| 6 | Generator per block. | `ChaCha8Rng::seed_from_u64(seed)` then `set_stream(block index)`; rand feature `chacha` (not a default feature); never `StdRng`. | Reuse map §3.3 and §6 (`rand-0.10.3/src/rngs/std.rs:20-37`; compile probe). |
| 7 | Parallel classification of every entry (unverified point 9). | Write the reuse map's 97-row classification, with its reference per entry, into the inventory (a "Parallel" column) and TS-25, after §3.2 items A–C. | Reuse map §2. |
| 8 | Exact signatures. FR-ALG-007 fixes them by reference, but no table exists. | One table per entry: procedure names, parameters with defaults, output columns per mode. GDS entries from `gds.list()` `signature` of GDS 2026.09.0 (run, not copied as text); MAGE entries from the MAGE source at `c72636014a8b` (Apache-2.0); derived entries from the petgraph or graphalgs function. Parameters that only tune GDS's own machinery (`writeConcurrency`, `jobId`, `logProgress`, `sudo`) are accepted and have no effect. | `gds.list()` (§0); `graph-algorithms.md` derived names. |
| 9 | Rust API shape (TS-25 "one code path"). No API method exists for a routine call (`api/session.rs`). | `Session::run_algorithm` and `Transaction::run_algorithm(name, Parameters) -> ResultStream`, which look up the same registry entry and build the same parameter record as `CALL`. | TS-25 One code path; TS-24 registry (`proc/mod.rs`). |
| 10 | Verification rule per entry. | A table in TS-25: oracle (GDS run for GDS-named entries, as GDS's values differ from NetworkX's, e.g. PageRank scale; NetworkX or petgraph tests otherwise; MAGE run where only MAGE defines the semantics); comparison (exact for integer and set results; floats within the reference's own tolerance parameter, or 1e-9 relative where it has none; for entries whose reference result is not unique, such as DFS order, colourings, matchings, MST ties and heuristics, a validity check plus equal objective value); the 12 parity entries by determinism and a quality measure (modularity for 061, 062, 063, 065; conductance or cut weight for 078; inertia for 080; #62 names the measure of each other parity entry). | Measured GDS scale (§0); NetworkX tests use relative tolerances (`networkx/algorithms/**/tests`). |
| 11 | `concurrency` out of range. | An integer ≥ 1; any other value fails with a GQLSTATUS error, as GDS rejects it. Tests cover 1–16 (NFR-PERF-006: 16 hardware threads). GDS oracle runs use `concurrency` ≤ 4 (unlicensed limit, §0). | Measured GDS limit (§0); NFR-PERF-006. |
| 12 | TS-31 rows. | `petgraph` 0.8.3, default features; `nalgebra` 0.35.0 with `default-features = false, features = ["std"]`, which drops `nalgebra-macros`, `proc-macro2`, `quote`, `syn` and `unicode-ident` and needs no Unicode-3.0 allowance; rand feature `chacha`; the transitive crates of §9 with licences; `deny.toml` allows `Zlib` (foldhash 0.1.5, via petgraph's hashbrown 0.15.5), which is on the NFR-DEP-002 list and which `deny.toml` says to add when a dependency brings it. | Probe (§0, §9): with lpg's `deny.toml`, "licenses FAILED" on `foldhash` (Zlib) and `unicode-ident` (Unicode-3.0); with `Zlib` added and nalgebra without default features, "licenses ok". |
| 13 | Licence and attribution of copies. | As earlier sprints: the module header names project, path, commit and licence; MIT and BSD copies quote the copyright and the full licence text (`src/sync.rs:24-50`); Apache-2.0 copies quote the copyright, point to `LICENSE-APACHE` and state that the file was modified (Apache-2.0 §4(b)); each copy is listed in the task's `rmp` `NOTE`. GPL (GDS, igraph, Raphtory, grappolo files with GPL headers), BSL and Memgraph Enterprise code is never copied; GDS documentation examples are not copied: expected values come from GDS runs. | `CLAUDE.md` "Copying code"; `src/sync.rs`, `src/ops/bulk/csv.rs:20-32`; reuse map §3.5. |
| 14 | Exponential outputs (ALG-031, 169, 181, 190) and large outputs (ALG-025, 027). | Result rows are produced in batches and charged per batch (TS-24); the search state is reserved like the projection, so a run that exceeds the cap fails under FR-LIM-001. | TS-24 Built; FR-LIM-003. |

### 3.2 Needs a user decision (asked in task #62)

Each item lists the options, recommended first. None blocks the runtime task #63; each blocks only the family task that holds its entries.

* **A. GPU-only parallel versions (ALG-076 spectral clustering, ALG-077 balanced-cut clustering; ALG-043 noted).** The only parallel source of 076 and 077 is MAGE's cuGraph GPU module; TS-25's rule counts "a parallel version" without naming the processor. ALG-043 is parallel through GDS PageRank's `sourceNodes` either way, though the inventory lists only MAGE cuGraph for it. Options: (a, recommended) a GPU-only version does not count: 076 and 077 run on one thread with dense `nalgebra` (TS-25), and FR-ALG-006 and TS-25 say "a parallel CPU version"; (b) it counts: 076 and 077 are parallel, which needs a parallel eigensolver written in lpg (nalgebra's `SymmetricEigen` is sequential); (c) it counts for the k-means stage only.
* **B. ALG-173 greedy colouring.** FR-ALG-007 gives it MAGE's name `graph_coloring`, but MAGE's procedure is a parallel quantum-annealing-style metaheuristic with its own parameters (`parallel_algorithm.py:9,55`), not a greedy algorithm. Options: (a, recommended) greedy colouring with the NetworkX strategies, single-thread, under MAGE's name and output columns; MAGE's metaheuristic parameters fail with a GQLSTATUS error; (b) MAGE's metaheuristic, parallel, with its parameters; (c) both, selected by MAGE's `algorithm` parameter.
* **C. ALG-153 Steiner tree semantics.** GDS `gds.steinerTree` is a rooted, directed heuristic and is parallel; petgraph, rustworkx and NetworkX solve the undirected metric-closure 2-approximation. FR-ALG-007 gives the entry GDS's name and parameters (`sourceNode`, `targetNodes`, `delta`, `applyRerouting`). Options: (a, recommended) GDS's rooted directed algorithm, parallel, checked against a GDS run; (b) the undirected 2-approximation under GDS's name, single-thread, checked against NetworkX; (c) both, selected by a parameter.
* **D. ALG-188 vehicle routing.** MAGE's `vrp` calls the GEKKO MINLP solver; no portable source exists. Options: (a, recommended) a deterministic heuristic under MAGE's name and parameters: Clarke-Wright savings, then 2-opt; verified by feasibility and by its total distance against the exact optimum on instances small enough to enumerate; (b) an exact branch-and-bound written in lpg, bounded by the memory cap; (c) a port of `vrp-core` 1.25.0 (Apache-2.0, a large framework).
* **E. Entries with no runnable reference.** ALG-076 and ALG-077 have only MAGE's cuGraph module, which needs an NVIDIA container runtime this machine lacks (§9); ALG-188 has only MAGE's GEKKO solver (presence in the MAGE image unverified). Exit #20 lists 12 parity entries verified by a quality measure. Options: (a, recommended) add 076, 077 and 188 to the TS-25 parity list, verified by determinism and a quality measure (modularity for 076, ratio-cut value for 077, feasibility and distance for 188); (b) the user installs `nvidia-container-toolkit` (needs root) and the laboratory runs `memgraph/memgraph-mage:3.13.2-relwithdebinfo-cugraph` (9.5 GB) as the oracle for 043, 076 and 077; 188 as option (a); (c) as (a), and sklearn's `SpectralClustering` in the laboratory venv as an approximate reference for 076.
* **F. Edge orientation.** GDS sets orientation (`NATURAL`, `REVERSE`, `UNDIRECTED`) when a graph is projected, not per algorithm; lpg has no projection step (FR-ALG-007), so undirected entries run on GDS users' `UNDIRECTED` projections have no lpg equivalent. Options: (a, recommended) every algorithm accepts `orientation` with GDS's values and default `NATURAL`; entries defined only for undirected graphs (for example ALG-084, 161, 169, 194) always mirror; FR-ALG-007 records the added parameter; (b) a fixed orientation per entry, the one its GDS documentation examples use; (c) GDS's `undirectedRelationshipTypes` list as the added parameter.

### 3.3 Decided in this planning task (no specification change; for task #5's DECISION comment)

* **Timing:** not run (UPDATE #57); `perf` 7.0.14 and `inferno` 0.12.8 recorded only. Rounds-to-convergence counts and memory peaks stay: they are not timings.
* **External oracles stay outside the crate:** NetworkX, numpy and scipy in `/tmp/lpg-lab-venv`; GDS in Docker; scripts under `docs/evidence/lab-sprint-5/`; none enters `cargo tree -p lpg`.
* **PageRank (ALG-042) is the pilot entry of the runtime task #63**, since the FR-ALG-003 and FR-ALG-007 acceptance texts name it and TS-25 already classifies it; the other 96 entries are in the family tasks.
* **Families:** six implementation tasks by the reuse map's grouping (reuse map §4), so that each shares its kernels, fixtures and oracle runs.
* **No dependency beyond TS-31.** The copy candidates (rustworkx, graphalgs, pathfinding, network_partitions, hdbscan, linfa) are copied, never depended on; `kdtree` (an hdbscan dependency) is not needed if lpg computes core distances on its projection.

## 4. Reuse inventory

The per-entry sources (best copy or port source, fallback, parallel reference), the shared-infrastructure findings (CSR limits, block scheduler, generators, nalgebra, test oracles) and the open points are in [sprint-5-reuse-map.md](sprint-5-reuse-map.md) §1–§6. Summary by component:

| Component | Main source (verdict) | Map section |
|---|---|---|
| Projection (forward and reverse CSR, visit traits) | petgraph `Csr` layout (copy); lpg adjacency scan (own) | §3.1 |
| Block scheduler, frontier | petgraph `parallel_page_rank`, `parallel_johnson` (copy); Kùzu frontier (port) | §3.2 |
| Generators per block | rand 0.10.3 `ChaCha8Rng` with `chacha` (dependency) | §3.3 |
| Dense algebra | nalgebra 0.35.0 (dependency); graphalgs `laplacian.rs`, `st.rs`, `seidel.rs` (copy) | §3.4 |
| Traversal and paths | petgraph (call, copy); rustworkx, graphalgs, pathfinding (copy); MAGE `neighbors`, `path` (port); LAGraph delta-stepping (method) | §2 |
| Centrality and similarity | petgraph, rustworkx (copy); MAGE node similarity, KNN (port); NetworkX link prediction (port) | §2 |
| Community | graphrs Louvain, network_partitions Leiden (copy); NetworKit PLM, PLP (port); linfa k-means (port); hdbscan (copy); NetworkX quality (port) | §2 |
| Components and structure | petgraph (call); rustworkx core number, transitivity (copy); graphalgs metrics (copy) | §2 |
| Trees, flows, matching, colouring | petgraph (call, copy); NetworKit SSP (port); MAGE bipartite matching, set cover (port) | §2 |
| Cycles, isomorphism, derived | petgraph (call, copy); graphalgs (copy); MAGE TSP (port) | §2 |
| Oracles | NetworkX 3.7 (venv); GDS 2026.09.0 (Docker, run only); petgraph, rustworkx, graphalgs tests | §3.5 |
| Written from scratch | 023, 044, 059, 064, 065, 078, 107, 113, 116, 117, 149, 154, 171, 176, 188 | §4 |

## 5. Task decomposition

Ten tasks, `rmp` #61–#70 (labelled T1–T10 while drafting). Every field is Markdown with `*` bullets; §8 checks lengths and the "no line starting with `- `" rule. Every test that writes graph data uses `/tmp/lpgram/<run>/` (`graph_dir`, `src/test_support/mod.rs:92`), deleted at its end; no Sprint 5 test depends on real flushes.

**Grouping (`CLAUDE.md` "Synergy and convergence"):** the laboratory task holds every instrument (oracles, generators, determinism and worker harnesses, memory checks). All specification gaps and the six user questions are one task, since all block the same implementation tasks. The runtime task builds the projection, scheduler, parameters, modes, registration and Rust API once for all 97 entries and proves them on PageRank. The 96 other entries are six family tasks (reuse map §4): each family shares kernels, fixtures and one oracle run. The exit task re-runs everything and writes the report.

**Common method in every implementation task** (stated in each `-tr` as "Method"): study the cited references for each critical decision and record a `DECISION` comment with its evidence; record each copy or port (project, path, commit, licence) in the file header (plan §3.1 point 13) and in a `NOTE`; keep tests as fast as possible without weakening assertions: the smallest graphs that exercise each branch, fixtures shared within the family, one oracle run per family; graph data only in `/tmp/lpgram/<run>/`, deleted after. No time measurement (UPDATE #57).

**Common acceptance of the family tasks** (each task's `-ac` states it in full): (1) every entry of the task is registered under its #62 name with stream and write modes; (2) results equal the #62 oracle under the #62 comparison rule on the shared fixtures and on seeded Zipf and uniform graphs, or, for parity entries, are deterministic and reach the #62 quality measure; (3) the determinism harness gives bit-equal results at `concurrency` 1–16, twice each, with two seeds for randomised entries; (4) parallel entries use more than one and at most `concurrency` workers on a projection of at least two blocks, single-thread entries one worker at `concurrency` 16; (5) `CALL` and the Rust API give equal results; (6) the accountant's peak never exceeds the cap, a run over a cap smaller than its projection fails with a GQLSTATUS error, and the `heaptrack` peak of the family's suite is in the report.

---

### #61 CHORE, priority 9, depends on: none

**Title:** Set up the Sprint 5 laboratory: oracles, generators and determinism harness

**-fr**
```text
**Every Sprint 5 correctness claim must be checked by an instrument, not by reasoning.** Before any algorithm exists, the laboratory needs the latest toolchain, reference oracles that run the reference implementations on lpg's test graphs (NetworkX, and Neo4j GDS in Docker), seeded Zipf and uniform graph generators, a harness that compares runs bit for bit at every `concurrency` value, a worker-count probe, and memory-peak checks (sprint DECISION 21).

* Correctness only: `perf stat`, flame graphs, speedup curves and time measurements are deferred to Sprint 6 (sprint UPDATE 57).
```

**-tr**
```text
* **Toolchain:** `rustup self update`, `rustup update stable nightly`, keep 1.89 (MSRV); `cargo install --locked <crate>@<max_stable_version>` for `cargo-nextest`, `cargo-deny`, `cargo-llvm-cov`, `cargo-fuzz`, `inferno` when crates.io lists a newer release; record `heaptrack`, `perf`, `docker`, `python3` versions.
* **NetworkX oracle:** `/tmp/lpg-lab-venv` (NetworkX 3.7, numpy 2.5.4, scipy 1.18.1, installed in planning); `docs/evidence/lab-sprint-5/nx_oracle.py` reads a graph file and an algorithm call (name, parameters) and writes the result as JSON Lines; run with `python -I`.
* **GDS oracle:** `docs/evidence/lab-sprint-5/gds_oracle.sh` starts `neo4j:2026.09.0-community` with `NEO4J_PLUGINS='["graph-data-science"]'` (bundled GDS 2026.09.0), imports the graph files with `neo4j-admin database import full` (Sprint 4 CSV layout), runs one GDS call with `concurrency` at most 4 (unlicensed limit) and writes its rows as JSON Lines; removes the container. GPL: run only, never copied.
* **Graph generators** in `src/lab/generator.rs` (own): seeded Zipf and uniform graphs, directed and undirected, with labels, edge types, `f64` weights and node vector properties; DAGs, bipartite graphs, flow networks with capacities and costs; fixtures karate club and Petersen copied from rustworkx `generators/karate_club.rs`, `petersen_graph.rs` @ `25398ab47f66` (Apache-2.0); one writer for the oracle graph file and one loader into an lpg graph in `/tmp/lpgram/<run>/`.
* **Determinism harness** in `src/test_support/`: runs a closure at `concurrency` 1–16, twice each, compares rows bit for bit (floats by `to_bits`) and names the first difference.
* **Worker-count probe:** counts the threads a run starts (`thread_names`, `src/test_support/mod.rs:15`, or a test hook in the scheduler) so a test can check FR-ALG-006.
* **Memory:** accountant peak (`MemoryAccountant::peak`, `src/storage/memory.rs:102`), heap peak per closure (`allocation-counter`), `docs/evidence/lab-sprint-5/heaptrack.sh` after the Sprint 3 script.
* **Record:** `docs/evidence/lab-sprint-5/report.md` with Environment and Tools (name, version, command).
* Out of scope: timing, flame graphs, speedup.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* `rustup check` reports no update; each cargo tool's `--version` equals its crates.io `max_stable_version`; the report lists every tool with version and command.
* `nx_oracle.py` on the karate-club fixture returns 34 PageRank values that sum to 1 within 1e-12; `gds_oracle.sh` on the same files returns 34 rows of `gds.pageRank.stream`; both exit non-zero for an unknown algorithm name.
* The determinism harness passes a closure that adds floats in block order and fails, naming the `concurrency` value and the row, a closure whose addition order depends on `concurrency`.
* The worker-count probe reports k for a closure that starts k scoped threads, for k = 1, 4 and 16.
* Two generator runs with one seed write byte-identical graph files; each generated DAG is acyclic, each bipartite graph two-colourable, each flow network has non-negative capacities.
* `cargo deny check licenses` and `cargo +1.89 build` exit 0; `cargo tree -p lpg -e normal` is unchanged by this task.
```

---

### #62 TASK, priority 9, depends on: none

**Title:** Record the Sprint 5 specification gaps in TS-25, TS-31 and the inventory

**-fr**
```text
**Each Sprint 5 implementation task must start from a recorded decision, not from a choice made while coding.** Planning found 14 gaps decidable from references (plan §3.1) and six that need the user (plan §3.2, items A to F). This task obtains the user's choices, records every decision with its evidence in the technical specification (and in the functional specification where a choice changes a requirement), and writes the parallel classification of all 97 entries with its references (unverified point 9, classification part).

Items A to F block only the family tasks that hold their entries; the §3.1 points block the runtime task.
```

**-tr**
```text
Owners: `technical-writer` for `docs/technical-spec/`, `specification-manager` for `docs/functional-spec/`.

1. **Ask the user** items A to F of plan §3.2, each with its options, recommended first; quote each answer in a `DECISION` comment.
2. **TS-25:** plan §3.1 points 1 to 11, 13 and 14 with their references; the choices A to F; the parity list as item E fixes it.
3. **Classification:** a "Parallel" column in `graph-algorithms.md` and a table in TS-25, 97 rows, each with its reference as reuse map §2 cites it, after items A to C.
4. **Signatures (point 8):** one row per entry with procedure names, parameters and defaults, output columns per mode; GDS rows from `gds.list()` of GDS 2026.09.0 in Docker; MAGE rows from `/tmp/mage` @ `c72636014a8b` (Apache-2.0).
5. **Verification table (point 10):** oracle and comparison rule per entry.
6. **TS-31 (point 12):** `petgraph` 0.8.3; `nalgebra` 0.35.0 without default features plus `std`; rand feature `chacha`; the transitive crates of plan §9 with licences; `Zlib` in the allow-list.
7. **Unverified point 9** (`quality-and-testing.md`): classification part closed with its evidence; convergence left to the exit task.
8. **Functional specification:** only where a choice changes FR-ALG-006, FR-ALG-007 or the inventory.
* Out of scope: code; decisions outside plan §3.
* Method: reference study per decision as `DECISION`.
```

**-ac**
```text
* Each of plan §3.1 points 1 to 14 and items A to F is stated in TS-25, TS-31 or the inventory with its source; a reviewer finds each by its number in the task's `NOTE` index (point to file and line).
* Six `DECISION` comments quote the user's answers to items A to F.
* The inventory's "Parallel" column has a value and a cited reference for each of the 97 entries; the count of parallel entries equals the count in TS-25.
* The signature table and the verification table each have exactly 97 rows; each GDS row matches the `gds.list()` signature of GDS 2026.09.0 (the run's output is in the `NOTE`).
* `cargo deny check licenses` on a probe crate with the TS-31 Sprint 5 crates and the updated allow-list exits 0 (result in the `NOTE`).
* No line of the changed files contains `TBD`, `etc.` or `as needed`.
```

---

### #63 TASK, priority 9, depends on: #61, #62

**Title:** Build the algorithm runtime: projection, block scheduler, modes and Rust API

**-fr**
```text
**One projection, one block scheduler and one procedure binding must serve all 97 entries** (sprint DECISION 19). A run reads the caller's snapshot, filtered by labels and edge types, into an in-memory projection reserved from the memory cap; parallel entries split work into blocks of node indexes merged in block order; each run streams rows or writes properties in the caller's transaction; `CALL` and the Rust API reach the same function (TS-25, FR-ALG-002, FR-ALG-003, FR-ALG-004, FR-ALG-006, FR-ALG-007, FR-LIM-001, FR-LIM-003, FR-LIM-007, UC-ALG-001).

PageRank (ALG-042) is the pilot entry that proves the runtime (plan §3.3).
```

**-tr**
```text
* **Module `src/algo/`** (TS-01). Dependencies as TS-31 after #62: `petgraph` 0.8.3, `nalgebra` 0.35.0 (no default features, `std`), rand feature `chacha`; `deny.toml` adds `Zlib`.
* **Projection:** one ordered scan of the adjacency tree through the caller's transaction (`Transaction::scan_range`, `src/tx/mod.rs:596`); dense indexes, `u32` below 2^32 nodes; forward and reverse CSR with parallel edges (plan §3.1 points 1, 2); orientation as #62 item F fixes; label and edge-type filters; weight and node-property arrays (point 3); petgraph visit traits on a wrapper, `IntoNeighborsDirected` and `IntoEdgesDirected` included; layout copied from petgraph `src/csr.rs` (0.8.3, MIT OR Apache-2.0).
* **Memory:** run-long `Reservation` from the pool, grown in chunks before each array grows and checked against the transaction limit (point 4); over the cap fails under FR-LIM-001.
* **Scheduler:** block size `max(1, min(4096, ceil(n / 64)))`; `std::thread::scope` workers, `min(concurrency, blocks)` for parallel entries, one otherwise; blocks taken in index order; partial results merged in block order; a synchronous-round helper (round k reads round k−1); a generator per block (point 6); workers stop at the next block when the statement is cancelled (`ReadContext::check`).
* **Parameters:** one record for `CALL` and the Rust API: `concurrency` (default 4, at least 1), `seed` (default 0), `nodeLabels`, `relationshipTypes`, the #62 signature parameters.
* **Modes:** `.stream` as a `ReadOnly` routine; `.write` as `ReadWrite`, writing properties through the caller's write set (`set_property`, `src/storage/graph.rs:416`) within the cap (FR-LIM-007); other modes fail.
* **Rust API:** `Session::run_algorithm`, `Transaction::run_algorithm` as #62 point 9 fixes.
* **Pilot ALG-042:** copy petgraph `parallel_page_rank` (`src/algo/page_rank.rs:134`, 0.8.3) into the block scheme; GDS parameters and output columns from #62.
* Out of scope: the other 96 entries (#64 to #69).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* On a graph with labels A and B, PageRank filtered to A returns only A nodes; a node committed by another transaction during the run is absent; with a cap smaller than the projection the statement fails with a GQLSTATUS error and the transaction is aborted (FR-ALG-004).
* `gds.pageRank.write` in a transaction that rolls back leaves no score property; `.stream` returns rows and changes no property; a write-back needing more than the cap fails (FR-ALG-003, FR-LIM-007).
* `gds.pageRank.stream` and `.write` take the #62 parameters and output columns, with no graph-name argument; `gds.pageRank.stats` fails with a GQLSTATUS error (FR-ALG-007).
* `CALL` and `run_algorithm` on one snapshot give bit-equal rows (FR-ALG-002).
* Without `concurrency`, PageRank on a 2-block projection uses more than one and at most 4 workers; with 1, one; on a 1-node projection, one; for k in 1 to 16, at most k (FR-ALG-006).
* PageRank on a seeded Zipf graph is bit-equal at `concurrency` 1 to 16, twice each, and matches the GDS oracle under the #62 rule; rounds to convergence and GDS `ranIterations` are in the report.
* A multigraph projection keeps every parallel edge; forward and reverse rows equal the adjacency scan; petgraph `toposort`, `kosaraju_scc` and `dinics` run on the wrapper.
* The accountant's peak never exceeds the cap; its peak and the `heaptrack` peak of a Zipf projection are in the report.
* `cargo deny check licenses`, `cargo +1.89 build` exit 0; `cargo tree -p lpg -e normal` lists only TS-31 crates.
```

---

### #64 TASK, priority 8, depends on: #63

**Title:** Implement the traversal and shortest-path families (26 entries)

**-fr**
```text
**The traversal and ordering family and the shortest and weighted paths family must run as specified, verified against their references** (FR-ALG-005, NFR-REL-004; sprint DECISIONS 18, 20). Entries: ALG-001, 002, 003, 004, 006, 007, 008, 009, 011, 013, 014, 015, 018, 019, 020, 021, 023, 024, 025, 026, 027, 028, 029, 030, 031, 032.

Parallel (reuse map §2, after #62): 001, 006, 007, 019, 023, 024, 027, 029, 032. Parity entry: 006 (random walk). Shared kernels: BFS frontier, Dijkstra, topological levels.
```

**-tr**
```text
* **Sources** (reuse map §2; each copy in its file header):
  * petgraph 0.8.3 (MIT OR Apache-2.0): call `Bfs`, `Dfs`, `has_path_connecting`, `dominators::simple_fast`, `dijkstra`, `astar`, `bellman_ford`, `find_negative_cycle`, `spfa`, `johnson`, `floyd_warshall`, `k_shortest_path`; copy `toposort`, `bidirectional_dijkstra`, `all_simple_paths` if the #63 wrapper does not suffice; `parallel_johnson` (`johnson.rs:203`) into blocks.
  * rustworkx-core @ `25398ab47f66` (Apache-2.0): `traversal/random_walk.rs`, `traversal/mod.rs` (ancestors), `dag_algo.rs` (longest path), `shortest_path/all_shortest_paths.rs`, `dijkstra.rs` (paths).
  * graphalgs @ `1f8a412ef4af` (MIT): `shortest_path/seidel.rs`, `shortest_distances.rs`.
  * pathfinding @ `cd4600b36ec0` (Apache-2.0/MIT): `directed/yen.rs`, `directed/bfs.rs`.
  * MAGE @ `c72636014a8b` (Apache-2.0): port `cpp/neighbors_module/algorithm/neighbors.cpp`, `cpp/path_module/algorithm/path.cpp`.
  * Kùzu @ `89f0263cc7a1` (MIT): port the frontier of `src/function/gds/gds_frontier.cpp` for parallel BFS.
  * LAGraph @ `7543de1f3103` (BSD-2-Clause): method of `LAGr_SingleSourceShortestPath.c` for delta-stepping (023), written in lpg.
  * Studied only: GDS (GPL) parameters and outputs; Memgraph core (BSL).
* **Parallel forms:** level-synchronous BFS; Bellman-Ford relaxation rounds; Kahn by levels; one source block per worker for 024, 027; spur nodes of a round for 029.
* **Oracles:** #62 verification table (GDS run for GDS-named entries; NetworkX; petgraph tests `tests/*.rs`).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* Each of the 26 entries is registered under its #62 name with `.stream` and `.write` (where #62 gives a write mode) and runs through `CALL` and the Rust API with bit-equal results.
* On the shared fixtures and on one seeded Zipf and one uniform graph, each entry other than 006 equals its #62 oracle under the #62 rule; for 006, walks are valid paths of the projection and the #62 quality measure holds.
* Each entry is bit-equal at `concurrency` 1 to 16, twice each; 006 with two seeds.
* Each parallel entry uses more than one and at most `concurrency` workers on a 2-block projection; each other entry uses one worker at `concurrency` 16.
* 031 and 028 on a graph whose output exceeds a small cap fail with a GQLSTATUS error; the accountant's peak never exceeds the cap; the family's `heaptrack` peak and the rounds to convergence of 019 against the reference are in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the family's code with its reason; `cargo +1.89 build` exits 0.
```

---

### #65 TASK, priority 8, depends on: #63

**Title:** Implement the centrality, similarity and link-prediction families (19 entries)

**-fr**
```text
**The centrality family (without the #63 pilot PageRank) and the similarity and link-prediction family must run as specified, verified against their references** (FR-ALG-005, NFR-REL-004). Entries: ALG-034, 035, 038, 039, 040, 041, 043, 044, 045, 059, 105, 106, 107, 112, 113, 114, 115, 116, 117.

Parallel (after #62): 034, 035, 038, 039, 040, 043, 044, 045, 059, 105, 106. Parity entries: 059 (CELF), 106 (KNN). Shared kernels: the #63 power iteration (040, 043, 044), per-source BFS (035, 038, 039), neighbour-set operations (105, 112 to 117).
```

**-tr**
```text
* **Sources** (reuse map §2):
  * rustworkx-core @ `25398ab47f66` (Apache-2.0): `centrality.rs` (`betweenness_centrality`, `closeness_centrality`, `eigenvector_centrality`, `katz_centrality`); port `src/link_analysis.rs:272` (`hits`).
  * graphina @ `212717ec4749` (MIT OR Apache-2.0): `centrality/harmonic.rs`, `centrality/personalized_pagerank.rs`.
  * MAGE @ `c72636014a8b` (Apache-2.0): port `cpp/node_similarity_module/algorithms/node_similarity.hpp`, `cpp/knn_module/algorithms/knn.hpp`.
  * NetworkX @ `6da4704cbf32` (BSD-3-Clause): port `link_prediction.py` (112, 114, 115).
  * Written from scratch: 044 (ArticleRank from the 042 kernel), 059 (CELF with Monte Carlo cascades per block), 107, 113, 116, 117.
  * Studied only: GDS (GPL) `betweenness`, `closeness`, `harmonic`, `pagerank`, `hits`, `influenceMaximization`, `similarity`.
* **Parallel forms:** per-source blocks with partial sums merged in block order (035, 038, 039); synchronous power iteration (040, 043, 044, 045); per-block generators for 035 sampling, 059 and 106.
* **Oracles:** #62 verification table.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* Each of the 19 entries is registered under its #62 name with its #62 modes and runs through `CALL` and the Rust API with bit-equal results.
* On the shared fixtures and on one seeded Zipf and one uniform graph, each entry other than 059 and 106 equals its #62 oracle under the #62 rule; 059 and 106 are deterministic and meet their #62 quality measures (expected spread for 059; recall against exact KNN for 106).
* Each entry is bit-equal at `concurrency` 1 to 16, twice each; 035 with sampling, 059 and 106 with two seeds.
* Each parallel entry uses more than one and at most `concurrency` workers on a 2-block projection; each other entry uses one worker at `concurrency` 16.
* The accountant's peak never exceeds the cap; a run over a cap smaller than its projection fails with a GQLSTATUS error; the family's `heaptrack` peak and the rounds to convergence of 040, 043, 044, 045 against the reference are in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the family's code with its reason; `cargo +1.89 build` exits 0.
```

---

### #66 TASK, priority 8, depends on: #63

**Title:** Implement the community-detection and clustering family (11 entries)

**-fr**
```text
**The community detection and clustering family must run as specified and be verified by a reference or, for the TS-25 parity entries, by determinism and a quality measure** (FR-ALG-005, NFR-REL-004; TS-25 "Consequence for parity with GDS"). Entries: ALG-061, 062, 063, 064, 065, 076, 077, 078, 080, 081, 082.

Parallel (after #62 item A): 061, 062, 063, 064, 065, 078, 080, 081, 082, and 076, 077 if item A is (b) or (c). Parity entries: 061, 062, 063, 064, 065, 078, 080, 081, and 076, 077 if item E is (a). Shared kernels: local move (061, 062, 065), label propagation (063, 064), k-means (080, reused by 076, 077), partition quality (082, used to verify the others).
```

**-tr**
```text
* **Sources** (reuse map §2):
  * graphrs @ `1cddb87eb47c` (MIT): `community/louvain.rs`.
  * network_partitions (graspologic-native) @ `773181488b4e` (MIT): `leiden/*.rs`, `random_vector.rs`; local-moving phase parallelised per block.
  * NetworKit @ `5f03e17cec09` (MIT): port `community/PLM.cpp`, `PLP.cpp` after conversion to synchronous rounds.
  * graphina @ `212717ec4749` (MIT OR Apache-2.0): `community/label_propagation.rs`, `community/spectral.rs` with nalgebra `SymmetricEigen`.
  * linfa-clustering @ `3b411a4c985c` (MIT OR Apache-2.0): port `k_means/algorithm.rs`, `init.rs`.
  * hdbscan @ `82ac6aa9a378` (MIT OR Apache-2.0): `hdbscan.rs`, `core_distances*`, `min_spanning_tree.rs`, `union_find.rs`.
  * NetworkX @ `6da4704cbf32` (BSD-3-Clause): port `community/quality.py`.
  * cuGraph @ `c9406a29ba00` (Apache-2.0): method of `spectral_clustering.cu:36,110` only.
  * Written from scratch: 064, 065 (on the 061 kernel), 078.
  * Studied only: GDS (GPL); grappolo (GPL headers): never ported.
* **Determinism:** synchronous rounds with a fixed tie-break (lowest community id) for 061, 063, 065; per-block generators for 062, 064, 078, 080.
* **Oracles:** #62 verification table (082 against NetworkX `quality.py` and GDS `modularity`, `conductance`).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* Each of the 11 entries is registered under its #62 name with its #62 modes and runs through `CALL` and the Rust API with bit-equal results.
* 082 equals NetworkX and the GDS oracle under the #62 rule on the fixtures and on a seeded Zipf graph; each other entry that #62 does not list as a parity entry equals its oracle under the #62 rule.
* Each parity entry is deterministic and meets its #62 quality measure on the karate-club fixture and on a seeded graph with planted communities; the measured value and the reference's value on the same graph are in the report.
* Each entry is bit-equal at `concurrency` 1 to 16, twice each, with two seeds for the randomised entries.
* Each parallel entry uses more than one and at most `concurrency` workers on a 2-block projection; each other entry uses one worker at `concurrency` 16.
* The accountant's peak never exceeds the cap; the family's `heaptrack` peak and the rounds to convergence of 061, 062, 063, 065 against the reference are in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the family's code with its reason; `cargo +1.89 build` exits 0.
```

---

### #67 TASK, priority 8, depends on: #63

**Title:** Implement the components and structural-measure families (13 entries)

**-fr**
```text
**The components and connectivity family and the structural measures family must run as specified, verified against their references** (FR-ALG-005, NFR-REL-004). Entries: ALG-083, 084, 085, 086, 089, 090, 091, 093, 120, 121, 123, 128, 143.

Parallel (after #62): 083, 093, 120, 121. No parity entry. Shared kernels: union-find, DFS low-link, triangle intersection over sorted CSR rows, BFS distances.
```

**-tr**
```text
* **Sources** (reuse map §2):
  * petgraph 0.8.3 (MIT OR Apache-2.0): call `connected_components`, `UnionFind`, `tarjan_scc`, `articulation_points`, `bridges`; copy the logic of `condensation` (`mod.rs:477`) over `tarjan_scc`.
  * rustworkx-core @ `25398ab47f66` (Apache-2.0): `connectivity/biconnected.rs`, `connectivity/core_number.rs`, `transitivity.rs`.
  * graphalgs @ `1f8a412ef4af` (MIT): `metrics.rs:28-466, 534`, `spec/laplacian.rs:109` (nalgebra 0.33 to 0.35).
  * graphina @ `212717ec4749` (MIT OR Apache-2.0): `parallel/triangles.rs` (rayon replaced by blocks).
  * Kùzu @ `89f0263cc7a1` (MIT): port `weakly_connected_components.cpp`, `k_core_decomposition.cpp`.
  * Studied only: GDS (GPL) `wcc`, `kcore`, `triangle`.
* **Parallel forms:** minimum-label rounds or union-find per block merged in block order (083); peeling rounds (093); forward intersection counted per block (120, 121).
* **Oracles:** #62 verification table.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* Each of the 13 entries is registered under its #62 name with its #62 modes and runs through `CALL` and the Rust API with bit-equal results.
* On the shared fixtures and on one seeded Zipf and one uniform graph, each entry equals its #62 oracle under the #62 rule; component ids are compared as partitions.
* Each entry is bit-equal at `concurrency` 1 to 16, twice each.
* Each parallel entry uses more than one and at most `concurrency` workers on a 2-block projection; each other entry uses one worker at `concurrency` 16.
* 143 on a projection whose dense matrix exceeds the cap fails with a GQLSTATUS error; the accountant's peak never exceeds the cap; the family's `heaptrack` peak and the rounds of 083 and 093 against the reference are in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the family's code with its reason; `cargo +1.89 build` exits 0.
```

---

### #68 TASK, priority 8, depends on: #63

**Title:** Implement the spanning-tree, flow, matching and colouring families (16 entries)

**-fr**
```text
**The spanning trees, flows and cuts family and the matching, covering, cliques and colouring family must run as specified, verified against their references** (FR-ALG-005, NFR-REL-004). Entries: ALG-147, 149, 151, 153, 154, 155, 157, 161, 163, 169, 171, 173, 174, 176, 177, 178.

Parallel (after #62 items B and C): 155, 157, 171, 176, and 153 if item C is (a), 173 if item B is (b) or (c). Parity entry: 176 (K-1 colouring). Shared kernels: spanning trees, augmenting paths, colouring.
```

**-tr**
```text
* **Sources** (reuse map §2):
  * petgraph 0.8.3 (MIT OR Apache-2.0): call `min_spanning_tree`, `min_spanning_tree_prim`, `maximum_matching`, `greedy_matching`, `maximal_cliques`, `dsatur_coloring`; copy `maximum_flow/dinics.rs`, `feedback_arc_set.rs`, `steiner_tree.rs` as item C fixes.
  * graphalgs @ `1f8a412ef4af` (MIT): `spec/st.rs:79` (151; exact integer count instead of the `f64` cast, `st.rs:86`), `mst/boruvka.rs`.
  * rustworkx-core @ `25398ab47f66` (Apache-2.0): `coloring.rs:106`, `steiner_tree.rs`.
  * NetworKit @ `5f03e17cec09` (MIT): port `flow/SuccessiveShortestPath.cpp`, `flow/Dinic.cpp`.
  * MAGE @ `c72636014a8b` (Apache-2.0): port `bipartite_matching.cpp`, `python/set_cover.py` (greedy only).
  * pathfinding @ `cd4600b36ec0` (Apache-2.0/MIT): `kuhn_munkres.rs`, `edmonds_karp.rs` (fallback).
  * Written from scratch: 149 (on 147), 154, 171, 176, the deterministic parallel push-relabel of 155.
  * Studied only: GDS (GPL) `kspanningtree`, `pricesteiner`, `k1coloring`, `steiner`.
* **Oracles:** #62 verification table; non-unique results (MST ties, matchings, colourings) by validity and equal objective.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* Each of the 16 entries is registered under its #62 name with its #62 modes and runs through `CALL` and the Rust API with bit-equal results.
* On the shared fixtures and on seeded graphs (flow networks for 155, 157; bipartite graphs for 163), each entry other than 176 equals its #62 oracle under the #62 rule: equal total weight, flow value, cost, matching size, clique counts; 151 equals the exact count on graphs where the `f64` determinant is not exact.
* 176 gives a proper colouring, is deterministic and meets its #62 quality measure (number of colours against GDS's on the same graph).
* Each entry is bit-equal at `concurrency` 1 to 16, twice each.
* Each parallel entry uses more than one and at most `concurrency` workers on a 2-block projection; each other entry uses one worker at `concurrency` 16.
* The accountant's peak never exceeds the cap; the family's `heaptrack` peak is in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the family's code with its reason; `cargo +1.89 build` exits 0.
```

---

### #69 TASK, priority 8, depends on: #63, #64

**Title:** Implement the cycle, isomorphism and derived-structure families (11 entries)

**-fr**
```text
**The cycles, tours and routing family, the isomorphism and graph recognition family and the derived structures family must run as specified, verified against their references** (FR-ALG-005, NFR-REL-004). Entries: ALG-180, 181, 187, 188, 189, 190, 194, 203, 204, 205, 206.

All 11 are single-thread (reuse map §2). It depends on #64 for the topological sort (ALG-007) that 205 and 206 use.
```

**-tr**
```text
* **Sources** (reuse map §2):
  * petgraph 0.8.3 (MIT OR Apache-2.0): call `is_cyclic_directed`, `is_cyclic_undirected`, `is_bipartite_undirected`, `dag_transitive_reduction_closure` (after #64's topological sort); copy `isomorphism.rs` (VF2) if the #63 wrapper does not suffice, `tred.rs:71`.
  * graphalgs @ `1f8a412ef4af` (MIT): `elementary_circuits.rs:71`, `tournament.rs:36,121`, `spec/mod.rs:29`.
  * rustworkx-core @ `25398ab47f66` (Apache-2.0): `connectivity/find_cycle.rs`.
  * MAGE @ `c72636014a8b` (Apache-2.0): port `python/mage/geography/travelling_salesman.py` (2-approximation, 1.5-approximation, greedy).
  * 188 as #62 item D fixes.
* **Oracles:** #62 verification table; 187 and 188 by validity and objective value as #62 fixes.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* Each of the 11 entries is registered under its #62 name with its #62 modes and runs through `CALL` and the Rust API with bit-equal results.
* On the shared fixtures, on petgraph's `tests/res` isomorphism graphs and on seeded graphs, each entry equals its #62 oracle under the #62 rule; 187 and 188 return valid tours and routes whose cost meets the #62 rule.
* Each entry is bit-equal at `concurrency` 1 to 16, twice each, and uses one worker at `concurrency` 16.
* 181 and 190 on a graph whose output exceeds a small cap fail with a GQLSTATUS error; the accountant's peak never exceeds the cap; the family's `heaptrack` peak is in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the family's code with its reason; `cargo +1.89 build` exits 0.
```

---

### #70 TASK, priority 7, depends on: #61, #63, #64, #65, #66, #67, #68, #69

**Title:** Verify the Sprint 5 exit condition and write the laboratory report

**-fr**
```text
**The sprint ends only when every correctness condition of exit comment 20 holds on the final code for all 97 entries, measured by the laboratory, and the deferred timing items are named** (sprint DECISION 20, UPDATE 57). The report closes unverified point 9.
```

**-tr**
```text
* Re-run on the final code: the full test suite, every family suite with its oracle runs, the determinism harness at `concurrency` 1 to 16 for all 97 entries, the worker-count checks, the over-cap checks.
* Coverage: `cargo llvm-cov nextest --branch` (nightly) over `src/algo/`; each uncovered branch listed with its reason.
* Memory: accountant peak and `heaptrack` peak per family (correctness: within the cap).
* Convergence: rounds of each iterative parallel entry against the reference's iterations on the same graphs.
* Report `docs/evidence/lab-sprint-5/report.md`: one section per exit condition without timing (plan §1.2), a 97-row table (entry, name, parallel, reference, oracle, result), the parity entries with their quality measures; the Sprint 6 list (time per entry, speedup at 1, 2, 4, 8, 16 workers, `perf stat`, flame graphs).
* `quality-and-testing.md` point 9 updated with the evidence (owner `technical-writer`); the FR-ALG-001 list of provided entries in the crate documentation.
* `cargo deny check licenses`, `cargo +1.89 build`, `cargo tree -p lpg -e normal` (TS-31 crates only).
* Out of scope: new features; timing.
```

**-ac**
```text
* The report has one section per condition of exit 20 without timing, each citing a test or run by name and its result, and a 97-row table with no empty cell.
* All suites exit 0 on the final commit; the report names the commit.
* For each of the 97 entries the report shows bit-equal results at `concurrency` 1 to 16 and equal `CALL` and Rust API results.
* The accountant's peak never exceeds the cap in any run; the `heaptrack` peaks are in the report.
* `quality-and-testing.md` states point 9 closed with the classification, the determinism evidence and the convergence counts.
* `cargo tree -p lpg -e normal` lists no crate outside TS-31; `cargo deny check licenses` exits 0.
```

## 6. Execution order and dependency edges

Order for `rmp sprint reorder 5`: **#5, #61, #62, #63, #64, #65, #66, #67, #68, #69, #70.**

| Task | Depends on | Why |
|---|---|---|
| #61 | none | first task after planning (task #5 `-fr`) |
| #62 | none | specification and user questions; runs while #61 runs |
| #63 | #61, #62 | oracles, harness, generators; §3.1 points |
| #64 | #63 | projection, scheduler, modes |
| #65 | #63 | as #64; the PageRank kernel of #63 |
| #66 | #63 | as #64; items A and E |
| #67 | #63 | as #64 |
| #68 | #63 | as #64; items B and C |
| #69 | #63, #64 | topological sort (ALG-007); item D |
| #70 | #61, #63, #64, #65, #66, #67, #68, #69 | runs on the final code (#62 reached through #63) |

#62's user items gate entries, not tasks: a family task may start once #63 is done and its own items are answered.

## 7. Traceability

Every id of scope comment #18, exit comment #20 and synergy comment #19, every laboratory item of #21 and the unverified point named in #20 map to at least one task; no task maps to an item outside them.

| Item | Tasks |
|---|---|
| TS-25 Names (and derived names) | #62, #63, #64–#69 |
| TS-25 One code path | #62 (point 9), #63 |
| TS-25 Projection (snapshot, filters, CSR, index width, reservation) | #62 (points 1–5), #63 |
| TS-25 Implementations (petgraph, lpg, nalgebra for 076, 143, 151) | #63, #64–#69 (076 in #66, 143 in #67, 151 in #68) |
| TS-25 Parallelism (`concurrency`, blocks, merge order, synchronous rounds) | #62 (points 7, 11), #63, #64–#68 |
| TS-25 Randomness | #62 (point 6), #63, #64, #65, #66, #68 |
| TS-25 Determinism | #61 (harness), #63, #64–#69, #70 |
| TS-25 Write-back | #63 |
| TS-25 Parity list (12 entries) | #62 (item E), #64 (006), #65 (059, 106), #66 (061–065, 078, 080, 081), #68 (176) |
| FR-ALG-001 | #70 |
| FR-ALG-002 | #63, #64–#69, #70 |
| FR-ALG-003 | #63 |
| FR-ALG-004 | #63, #64–#69 (over-cap checks) |
| FR-ALG-005 | #64–#69, #70 |
| FR-ALG-006 | #61 (probe), #62 (classification), #63, #64–#69, #70 |
| FR-ALG-007 | #62 (signatures, items B, F), #63, #64–#69 |
| FR-LIM-001 | #63, #64, #67, #69 |
| FR-LIM-003 | #63, #64–#69, #70 |
| FR-LIM-007 | #63 |
| NFR-REL-004 (algorithm part) | #64–#69, #70 |
| UC-ALG-001 (main flow, A1, A2) | #63 |
| Unverified point 9 (classification) | #62, #70 |
| Unverified point 9 (GDS determinism statements) | #62 (parity list), #64–#68 (determinism), #70 |
| Unverified point 9 (convergence of synchronous rounds) | #63, #64, #65, #66, #67, #70 |
| Scope #18: dense algebra with `nalgebra` (076, 143, 151) | #62 (point 12), #66, #67, #68 |
| Scope #18: parallel classification with its reference | #62 |
| Exit #20: `CALL` and Rust API equal | #63, #64–#69, #70 |
| Exit #20: reference or quality measure for each entry | #62 (point 10, item E), #64–#69, #70 |
| Exit #20: equal results at every `concurrency` | #61, #63, #64–#69, #70 |
| Exit #20: worker-count rule | #61, #63, #64–#69 |
| Exit #20: projection over the cap fails | #63, #64–#69 |
| Exit #20: report (memory, convergence; time and speedup deferred) | #70; timing to Sprint 6 (UPDATE #57) |
| Synergy #19: one projection, scheduler and binding | #63 |
| Synergy #19: planning by family, shared tests | #64–#69 |
| Synergy #19: single-thread entries call petgraph | #64, #67, #68, #69 |
| Laboratory #21: toolchain refresh | #5 (done, §9), #61 |
| Laboratory #21: oracles (NetworkX, GDS in Docker) | #5 (availability, §9), #61 |
| Laboratory #21: CPU (`perf stat`, flame graphs) | recorded in §9; deferred to Sprint 6 |
| Laboratory #21: scaling at 1–16 workers, speedup | worker counts in #61, #63–#69; speedup deferred to Sprint 6 |
| Laboratory #21: convergence against the asynchronous reference | #63, #64–#67, #70 |
| Laboratory #21: memory against the cap, RSS and `heaptrack` | #61, #63–#69, #70 |
| Laboratory #21: determinism at every `concurrency` | #61, #63–#69, #70 |
| Laboratory #21: datasets (Sprint 4 fast load, Zipf, uniform) | #61 |

## 8. Checks run on this document

The `text` blocks of §5 were checked by script after writing (result below): character count per field, and no line starting with `- `.

Result (2026-10-12): 30 fields, longest 2 282 characters (limit 4 096), shortest 259; 0 lines starting with `- `; the 10 titles are 63–79 characters, plain text. A search for open-ended wording (`etc.`, `as needed`, `if necessary`, `where appropriate`, `TBD`) found it only in #62's `-ac`, which forbids those words in the specification it changes. Line citations of §0–§3 were re-checked with `grep -n` after writing; 3 were corrected. Entry counts: 1 (#63) + 26 + 19 + 11 + 13 + 16 + 11 = 97; parallel entries 1 + 9 + 11 + 11 + 4 + 5 = 41, as the reuse map's tally.

## 9. Laboratory tool status

Measured 2026-10-12.

| Tool | Version | Command | State |
|---|---|---|---|
| rustup | 1.29.1 | `rustup self update` | "rustup unchanged" |
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | `rustup update stable nightly` | unchanged, up to date |
| Rust nightly | 1.101.0-nightly (6f5da406b 2026-10-10) | same | unchanged, up to date |
| Toolchain 1.89 (MSRV, `Cargo.toml` `rust-version = "1.89"`) | 1.89.0 (29483883e 2025-08-04) | `rustup check` | up to date |
| `rustup check` | every toolchain and rustup "up to date" | `rustup check` | no update |
| cargo-nextest | 0.9.148 | `cargo install --list`; crates.io `max_stable_version` | = 0.9.148 |
| cargo-deny | 0.20.2 | same | = 0.20.2 |
| cargo-llvm-cov | 0.9.1 | same | = 0.9.1 |
| cargo-fuzz | 0.13.2 | same | = 0.13.2 |
| inferno | 0.12.8 | same | = 0.12.8; not needed (timing deferred) |
| heaptrack | 1.5.0 | `heaptrack --version` | as Sprint 4 (latest tag v1.5.0, checked 2026-10-11) |
| perf | 7.0.14 | `perf --version` | recorded; not run (timing deferred) |
| Docker | 27.2.0 | `docker --version` | usable without root |
| Python | 3.14.4 | `python3 --version` | system |
| NetworkX | 3.7 (= PyPI latest) | `/tmp/lpg-lab-venv/bin/python -I -c "import networkx; print(networkx.__version__)"` | present |
| numpy, scipy | 2.5.4, 1.18.1 (= PyPI latest) | same, `__version__` | **installed** from PyPI wheels (SHA-256 checked against PyPI), with pip 26.2.1 run from its wheel; NetworkX `pagerank`, `hits` and the Laplacian functions need them |
| Neo4j and GDS | Neo4j 2026.09.0 community, image `dd2aca3682c1` (latest calendar tag on Docker Hub, 2026-10-12); GDS 2026.09.0 bundled | `RETURN gds.version()` | **runs**; `concurrency` ≤ 4 without a licence key |
| Memgraph MAGE | `memgraph/memgraph-mage:3.13.2-relwithdebinfo` (1.7 GB) and `latest` (746 MB) on Docker Hub, 2026-10-09 | Docker Hub API | not pulled; #61 pulls it only if #62 needs a MAGE run |
| NVIDIA container runtime | absent: `docker info` lists `runc` only; GPU RTX 3070 Laptop, driver 580.178.04 | `docker info`, `nvidia-smi` | needed only for §3.2 item E option (b); installing it needs root |

**Sprint 5 crates, probe** (`cargo tree -e normal`, petgraph 0.8.3, nalgebra 0.35.0 without default features plus `std`, rand 0.10.3 with `chacha`): new transitive crates approx 0.5.1, bytemuck 1.25.2, equivalent 1.0.2, fixedbitset 0.5.7, foldhash 0.1.5 (Zlib), hashbrown 0.15.5 and 0.17.1, indexmap 2.14.2, matrixmultiply 0.3.11, num-complex 0.4.6, num-integer 0.1.47, num-rational 0.4.2, num-traits 0.2.19, rawpointer 0.2.1, safe_arch 1.2.0, simba 0.10.2, typenum 1.20.1, wide 1.7.1. Licences from `cargo metadata`: all MIT, Apache-2.0 or alternatives including one of them, except foldhash (Zlib only). With nalgebra's default features the tree adds nalgebra-macros, proc-macro2, quote, syn and unicode-ident ((MIT OR Apache-2.0) AND Unicode-3.0).

**Needs root: none for this sprint's plan.** **Pending on the user:** the six decisions of §3.2; the NVIDIA container runtime only if item E option (b) is chosen.

**RAM space** (`df -h`, `du -sh`): before the planning run `/tmp` used 2.3 G and `/dev/shm` 12 K; after it, `/tmp` 2.5 G (the venv measured 20 M in the Sprint 4 plan §9 and 227 M after numpy and scipy were added) and `/dev/shm` 195 M. The 195 M are `/dev/shm/lpg-target`, created at 00:22:05 with a `flycheck0` directory, the mark of an editor's rust-analyzer check, not of this run (its probes used `/dev/shm/lpg-s5probe`); it was left in place as a build in use. `/tmp/lpgram` is empty (no graph data written). Single-use build directories (`/dev/shm/lpg-s5probe`, 168 M at its peak) were deleted after each probe; no lpg build was made. Left for later tasks: `/tmp/lpg-lab-venv` (227 M, numpy and scipy added), `/tmp/crates-src/nalgebra-0.35.0`, and the reference clones of the reuse map.

## 10. rmp operations to enter the plan (for the roadmap-manager; not run here)

1. `rmp task create` × 10 with the titles, types, priorities and `-fr`, `-tr`, `-ac` of §5; then `rmp sprint add-tasks 5 <ids>`.
2. `rmp task add-dep` for each edge of §6, with the real ids.
3. `rmp sprint reorder 5` in the order of §6.
4. On task #5: a `DECISION` with the §7 table (real ids); a `DECISION` with the choices of §3.3 and the grouping rationale of §5; a `FINDING` with the 14 points of §3.1 and the six items of §3.2; a `TEST` with the tool versions of §9.
