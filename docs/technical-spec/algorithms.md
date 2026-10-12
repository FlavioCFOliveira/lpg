---
title: Graph algorithms
status: draft
last-reviewed: 2026-10-12
related:
  - algorithm-entries.md
  - interfaces.md
  - transactions-and-recovery.md
  - ../functional-spec/graph-algorithms.md
---

# Graph algorithms

## TS-25 Graph algorithms

- **Decision**:
  - **Names and signatures.** Each algorithm is a built-in routine ([TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events)) with the name, parameters, modes and output columns fixed by FR-ALG-007 and the [inventory](../functional-spec/graph-algorithms.md): the GDS name in the `gds.` namespace, otherwise the MAGE name, otherwise the derived `gds.` name. Only the stream and write modes exist, with no graph-name argument. Name validity in GQL is provisional (OQ-005).
    - The exact signature of every entry is in [algorithm-entries.md](algorithm-entries.md#signatures), with the rules that produced it: GDS rows as GDS 2026.09.0 reports them (`gds.list()` and the `configuration` a run returns), MAGE rows from the MAGE source [S27], derived rows from the petgraph or graphalgs function.
    - GDS and derived routines take one argument, a configuration record. A `CALL` passes it as a query parameter, since a record literal needs GV45, which is not supported (TS-24 Point procedures). MAGE routines take MAGE's positional parameters, then the same configuration record for the common keys, and have no write mode: a caller writes their result with `CALL … YIELD … SET` in its own transaction (user decision 2026-10-12, FR-ALG-003).
    - Common keys on every procedure (not on the functions ALG-107 and ALG-112 to ALG-117): `nodeLabels`, `relationshipTypes`, `concurrency`, `orientation`, and `randomSeed` on randomised entries. Keys that tune only GDS's own machinery (`jobId`, `logProgress`, `sudo`, `writeConcurrency`, `writeToResultStore`, and those marked "no effect") are accepted and change nothing.
    - An unknown key, or a value outside its domain, fails with 22N11; a value of the wrong type with 22G03, as for the other built-ins (`src/proc/builtin.rs:164`). GDS also rejects unknown keys ("Unexpected configuration key", measured on GDS 2026.09.0, rmp #62).
  - **Rust API** (one code path). `Session::run_algorithm(name, Parameters)` and `Transaction::run_algorithm(name, Parameters) -> ResultStream` look up the registry entry by its whole dotted name and fill the same parameter record as `CALL`; the keys of `Parameters` (`src/api/mod.rs:51`) are the signature's parameter names. A `CALL` and a Rust API call reach the same function.
  - **Projection.**
    - A run reads the caller's snapshot, filtered by `nodeLabels` and `relationshipTypes` (FR-ALG-004). Its scans register their intervals in the read set like a query's scans ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks) Read sets; `Transaction::scan_range`, `src/tx/mod.rs:596`).
    - It maps element identifiers to dense indexes and builds a forward and a reverse compressed sparse row array, in petgraph's `Csr` layout, in one ordered scan of the adjacency tree. The tree holds an `OUT` and an `IN` entry for every edge, keyed by node, so the scan yields each node's outgoing row and then its incoming row (`src/storage/graph.rs:125, 529`).
    - Parallel edges are kept, one entry each, as GDS's default projection keeps them (measured: two edges 0→1 gave node 0 degree 2.0, [M-S5](../evidence/planning/sprint-5-plan.md) §0).
    - A wrapper implements petgraph's visit traits on the two arrays, `IntoNeighborsDirected` and `IntoEdgesDirected` included, so petgraph functions compile unchanged. petgraph's own `Csr` is not used: it has no incoming-edge access, so 10 petgraph functions do not compile on it, and it rejects parallel edges ([reuse map](../evidence/planning/sprint-5-reuse-map.md) §3.1).
    - Indexes are `u32` when the projection has fewer than 2^32 nodes, otherwise `usize`.
    - Properties. The edge property named by `relationshipWeightProperty` (or the entry's other edge-property keys, such as `capacityProperty`) is loaded as `f64`, and each node property a key names as an array; an absent or non-numeric value is NaN, as GDS projects it (measured, M-S5 §0).
  - **Orientation** (user decision 2026-10-12, rmp #62 item F). Every procedure accepts `orientation`, with GDS's values: `NATURAL` (the default) reads the forward array, `REVERSE` the reverse array, `UNDIRECTED` both, so each edge is seen from both ends and a self-loop twice, as GDS's `UNDIRECTED` projection (measured: edges x→y, y→x, x→x gave degrees 4 and 2, rmp #62). The 20 entries marked U in [algorithm-entries.md](algorithm-entries.md#signatures) are defined only for undirected graphs and always run `UNDIRECTED`, whatever `orientation` says; 10 of them are those GDS 2026.09.0 rejects on a directed projection (measured, rmp #62). The GDS functions ALG-107 and ALG-112 to ALG-117 take neither `orientation` nor `concurrency`; ALG-112 to ALG-117 keep GDS's own `direction` key, default `BOTH` (user decision 2026-10-12, FR-ALG-006, FR-ALG-007).
  - **Memory.** The projection and the run state are reserved from the memory accountant ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)) in chunks, before each array grows, and checked against the transaction's memory limit, as `Held::grow` does (`src/query/exec/mod.rs:283-303`); the edge count is known only once the scan ends. The reservation is held for the whole run and released at its end. A refused reservation fails the statement with 51N72 or 51N73 (FR-ALG-004, FR-LIM-001).
    - Large and exponential outputs (ALG-025, ALG-027, ALG-031, ALG-169, ALG-181, ALG-190): result rows are produced and charged in batches (TS-24 Built), and the search state is reserved like the projection, so a run that exceeds the cap fails under FR-LIM-001.
  - **Implementations.**
    - Every parallel entry is written in lpg with the block scheme below.
    - An entry that runs on one worker calls petgraph where petgraph provides it, and is otherwise written in lpg over petgraph's visit traits.
    - Dense linear algebra uses `nalgebra`: ALG-026 Seidel, ALG-076 spectral clustering, ALG-077 balanced-cut clustering, ALG-143 Laplacian matrix, ALG-151 number of spanning trees ([reuse map](../evidence/planning/sprint-5-reuse-map.md) §3.4).
    - ALG-153 is GDS's rooted, directed Steiner tree heuristic, parallel (user decision 2026-10-12, item C).
    - ALG-173 is greedy colouring with the NetworkX strategies, on one worker, under MAGE's name `graph_coloring` and its output columns; MAGE's metaheuristic parameters fail with 22N11 (user decision 2026-10-12, item B).
    - ALG-188 is a deterministic Clarke-Wright savings heuristic followed by 2-opt, under MAGE's name and parameters (user decision 2026-10-12, item D).
    - Copies of open-source code: the module header names the project, path, commit and licence; MIT and BSD copies quote the copyright and the full licence text (`src/sync.rs:24-50`); Apache-2.0 copies quote the copyright, point to `LICENSE-APACHE` and state that the file was modified (Apache-2.0 §4(b)); each copy is listed in its task's `rmp` `NOTE`. GPL code (GDS, igraph, Raphtory, grappolo files with GPL headers), BSL code and Memgraph Enterprise code is never copied. GDS documentation examples are not copied: expected values come from GDS runs ([reuse map](../evidence/planning/sprint-5-reuse-map.md) §1, §3.5).
  - **Parallelism.**
    - Every procedure accepts `concurrency` (default 4): the maximum number of worker threads, not counting the thread that runs the calling statement. It is an integer of at least 1; any other value fails with 22N11, as GDS rejects it. Workers are `std::thread::scope` threads started by the run.
    - An entry is **parallel** exactly when a reference source of the inventory (Neo4j GDS, Memgraph MAGE, petgraph or graphalgs) provides a parallel CPU version of it; a version that runs only on a GPU does not count (user decision 2026-10-12, item A). Parallel entries run on up to `concurrency` workers; every other entry runs on one worker, whatever `concurrency` is set to.
    - The classification of all 97 entries, with the reference of each, is in [algorithm-entries.md](algorithm-entries.md#parallel-classification): 38 parallel, 59 single-thread.
    - Work in a parallel entry is split into blocks of node indexes. The block size is `max(1, min(4096, ceil(n / 64)))`, where `n` is the projected node count. It depends only on `n`, never on `concurrency`. Workers take blocks in index order.
    - A projection with fewer than two blocks (`n` = 1) has nothing to split and runs on one worker.
    - Per-block partial results are merged in block order, so floating-point sums are always added in the same order.
    - Iterative parallel entries use synchronous rounds: round *k* reads only the state of round *k*−1. Single-worker entries keep the reference's sequential formulation.
  - **Randomness.** Randomised entries take GDS's seed key, `randomSeed` (`samplingSeed` for ALG-035); when it is absent the seed is 0. Block *b* draws from `ChaCha8Rng::seed_from_u64(seed)` after `set_stream(b)`; an entry on one worker uses stream 0. `StdRng` is not used: its documentation states that "any future library version may replace the algorithm" ([reuse map](../evidence/planning/sprint-5-reuse-map.md) §3.3, `rand-0.10.3/src/rngs/std.rs:20-37`).
  - **Determinism.** For one snapshot and equal parameters, every run gives equal results, whatever `concurrency` is.
  - **Write-back** writes properties and relationships through the caller's transaction write set, bounded by the memory cap ([TS-14](transactions-and-recovery.md#ts-14-mvcc), FR-LIM-007).
  - **Verification.** Each entry has an oracle and a comparison rule in [algorithm-entries.md](algorithm-entries.md#verification): a GDS run for GDS-named entries, since GDS's values differ from NetworkX's (PageRank is not normalised to sum 1, M-S5 §0); NetworkX or petgraph otherwise; a MAGE run where only MAGE defines the semantics. Floats are compared within the entry's own tolerance parameter, or 1e-9 relative where it has none; results that are not unique by validity and equal objective value.
  - **Parity entries.** These 15 entries are verified by determinism and a quality measure, not by equality with a reference ([algorithm-entries.md](algorithm-entries.md#verification), rule Q):
    - GDS updates shared state in parallel without order, so lpg's results may differ from GDS's: documented as non-deterministic in GDS, ALG-106 KNN, ALG-078 Approximate Maximum k-cut, ALG-064 SLLPA; not stated in the GDS documentation, ALG-061 Louvain, ALG-062 Leiden, ALG-063 Label Propagation, ALG-065 Modularity Optimisation, ALG-176 K-1 colouring, ALG-080 K-means, ALG-081 HDBSCAN, ALG-006 random walk, ALG-059 CELF (unverified).
    - No runnable reference (user decision 2026-10-12, item E): ALG-076 (modularity), ALG-077 (ratio-cut value), ALG-188 (feasibility and distance against the exact optimum on small instances).
- **Satisfies**: FR-ALG-001, FR-ALG-002, FR-ALG-003, FR-ALG-004, FR-ALG-005, FR-ALG-006, FR-ALG-007, FR-LIM-001, FR-LIM-003, FR-LIM-007, NFR-REL-004, NFR-PERF-007.
- **Evidence**:
  - Neo4j GDS (D-GDS):
    - algorithms run on in-memory, compressed projections;
    - `concurrency` defaults to 4 ([S41]);
    - KNN is deterministic only with `concurrency` 1 and an explicit `randomSeed`;
    - Approximate Maximum k-cut "will not be deterministic unless running single-threaded … and using the same random seed";
    - SLLPA "will not produce deterministic results";
    - Label Propagation breaks ties "arbitrarily but deterministically";
    - Betweenness sampling with the same `samplingSeed` "will yield the same results".
  - GDS 2026.09.0 run in `neo4j:2026.09.0-community` (rmp #62): signatures, keys and defaults (algorithm-entries.md); `randomSeed` on KNN and Approximate Maximum k-cut fails unless `concurrency` is 1; `concurrency` above 4 fails without a licence key (M-S5 §9).
  - rayon leaves the order of `reduce` and `sum` unspecified, so floating-point results "are not fully deterministic" (D-RAYON).
  - petgraph `Csr` implements `IntoNeighbors`, `IntoEdges`, `NodeIndexable` and the other visit traits, but not `IntoNeighborsDirected` (R-PETGRAPH `crates/petgraph/src/csr.rs:551-811`). Its parallel functions need its optional `rayon` feature (`crates/petgraph/Cargo.toml:50`), which is not enabled; the entries they cover are written in lpg instead.
  - graphalgs, a source of the inventory, uses petgraph and nalgebra (R-GALGS `Cargo.toml`; `src/spec/laplacian.rs`, `src/spec/st.rs`).
- **Crates** (versions and features: [TS-31](dependencies.md#ts-31-dependency-set)):
  - `petgraph` 0.8.3: dependency.
  - `nalgebra` 0.35.0: dependency.
  - `rand` 0.10.3 with feature `chacha`: dependency.
  - `rayon` 1.12.0: rejected, because its reductions have no fixed order (D-RAYON).
  - `rustworkx-core` 0.18.1 (R-RWX): copied from, not depended on; it pulls in rayon, ndarray and several other crates (`rustworkx-core/Cargo.toml`).
  - `graphalgs` 0.2.0: copied from, not depended on.
  - `faer`: not used.
- **Rejected**:
  - Running algorithms on the stored trees directly: random I/O per hop, and the GDS reference projects first.
  - petgraph's `Csr` as the projection: no reverse adjacency, no parallel edges (above).
  - A work-stealing pool: completion order would decide the merge order.
  - Splitting work into one block per thread: results would then depend on `concurrency`.
  - `StdRng`: not portable across rand versions.

Whether a synchronous round converges as fast as the asynchronous reference is not measured (unverified).
