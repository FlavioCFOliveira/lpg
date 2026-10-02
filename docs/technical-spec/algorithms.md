---
title: Graph algorithms
status: draft
last-reviewed: 2026-10-02
related:
  - interfaces.md
  - transactions-and-recovery.md
  - ../functional-spec/graph-algorithms.md
---

# Graph algorithms

## TS-25 Graph algorithms

- **Decision**:
  - **Names.** Each algorithm is a built-in procedure ([TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events)) with the name, parameters, modes and output columns fixed by FR-ALG-007 and the [inventory](../functional-spec/graph-algorithms.md): the GDS name in the `gds.` namespace, otherwise the MAGE name, otherwise the derived `gds.` name. Only the stream and write modes exist, with no graph-name argument. Name validity in GQL is provisional (OQ-005).
  - **One code path.** A `CALL` and a Rust API call reach the same function with the same parameter record.
  - **Projection.**
    - A run reads the caller's snapshot, filtered by node labels and edge types.
    - It maps element identifiers to dense indexes and builds a `petgraph::csr::Csr` by one ordered scan of the adjacency tree, which is already sorted by source ([TS-06](storage.md#ts-06-graph-layout)).
    - Indexes are `u32` when the projection has fewer than 2^32 nodes, otherwise `usize`.
    - The projection and all run state are reserved from the memory accountant before they are built. If a reservation fails, the statement fails (FR-ALG-004, FR-LIM-001).
  - **Implementations.**
    - Every entry classified as parallel is written in lpg with the block scheme below.
    - An entry that runs on one thread calls petgraph where petgraph provides it, and is otherwise written in lpg over petgraph's visit traits.
    - Dense linear algebra uses `nalgebra`: ALG-076 spectral clustering, ALG-143 Laplacian matrix, ALG-151 number of spanning trees.
  - **Parallelism.**
    - Every algorithm accepts `concurrency` (default 4): the maximum number of worker threads, not counting the thread that runs the calling statement. Workers are `std::thread::scope` threads started by the run.
    - An entry is **parallel** exactly when a reference source of the inventory (Neo4j GDS, Memgraph MAGE, petgraph or graphalgs) provides a parallel version of it. Parallel entries run on up to `concurrency` workers; every other entry runs on one worker, whatever `concurrency` is set to.
    - Classified so far: ALG-024 Johnson (petgraph `parallel_johnson`) and ALG-042 PageRank (petgraph `parallel_page_rank`). Every other entry is classified, with its reference, before the first release (NFR-REL-004). That classification has not been done yet (unverified).
    - Work in a parallel entry is split into blocks of node indexes. The block size is `max(1, min(4096, ceil(n / 64)))`, where `n` is the projected node count. It depends only on `n`, never on `concurrency`. Workers take blocks in index order.
    - A projection with fewer than two blocks (`n` = 1) has nothing to split and runs on one worker.
    - Per-block partial results are merged in block order, so floating-point sums are always added in the same order.
    - Iterative parallel entries use synchronous rounds: round *k* reads only the state of round *k*−1. Single-worker entries keep the reference's sequential formulation.
  - **Randomness.** Randomised algorithms take a `seed` parameter. When it is absent, a fixed default seed (0) is used. Each block draws from its own generator, seeded from `(seed, block index)`.
  - **Determinism.** For one snapshot and equal parameters, every run gives equal results, whatever `concurrency` is.
  - **Write-back** writes properties through the caller's transaction write set, bounded by the memory cap ([TS-14](transactions-and-recovery.md#ts-14-mvcc), FR-LIM-007).
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
  - rayon leaves the order of `reduce` and `sum` unspecified, so floating-point results "are not fully deterministic" (D-RAYON).
  - petgraph `Csr` implements `IntoNeighbors`, `IntoEdges`, `NodeIndexable` and the other visit traits (R-PETGRAPH `crates/petgraph/src/csr.rs:551-811`). Its parallel functions need its optional `rayon` feature (`crates/petgraph/Cargo.toml:50`), which is not enabled; the entries they cover are written in lpg instead.
  - graphalgs, a source of the inventory, uses petgraph and nalgebra (R-GALGS `Cargo.toml`; `src/spec/laplacian.rs`, `src/spec/st.rs`).
- **Crates**:
  - `petgraph` 0.8.3 (MIT OR Apache-2.0, 533.5 M downloads): dependency, default features only.
  - `nalgebra` 0.35.0 (Apache-2.0, 92.7 M downloads): dependency.
  - `rand` 0.10.3: dependency.
  - `rayon` 1.12.0 (MIT OR Apache-2.0, 573.6 M downloads): rejected, because its reductions have no fixed order (D-RAYON).
  - `rustworkx-core` 0.18.1 (R-RWX): study only; it pulls in rayon, ndarray and several other crates (`rustworkx-core/Cargo.toml`).
  - `graphalgs` 0.2.0 (16.9 k downloads): study only.
  - `faer`: not used.
- **Rejected**:
  - Running algorithms on the stored trees directly: random I/O per hop, and the GDS reference projects first.
  - A work-stealing pool: completion order would decide the merge order.
  - Splitting work into one block per thread: results would then depend on `concurrency`.

**Consequence for parity with GDS.** In GDS, these algorithms update shared state in parallel without order; lpg's synchronous-round, block-ordered versions are deterministic, but for equal inputs their results may differ from GDS's:
- documented as non-deterministic in GDS: ALG-106 KNN, ALG-078 Approximate Maximum k-cut, ALG-064 SLLPA;
- not stated in the GDS documentation: ALG-061 Louvain, ALG-062 Leiden, ALG-063 Label Propagation, ALG-065 Modularity Optimisation, ALG-176 K-1 colouring, ALG-080 K-means, ALG-081 HDBSCAN, ALG-006 random walk, ALG-059 CELF (unverified).

Whether a synchronous round converges as fast as the asynchronous reference is not measured (unverified).
