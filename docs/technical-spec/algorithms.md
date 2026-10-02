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
  - **One code path.** A `CALL` and a Rust API call reach the same function with the same parameter record. Each algorithm is registered as a built-in procedure ([TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events)).
  - **Projection.**
    - A run reads the caller's snapshot, filtered by node labels and edge types.
    - It maps element identifiers to dense indexes and builds a `petgraph::csr::Csr` by one ordered scan of the adjacency tree, which is already sorted by source ([TS-06](storage.md#ts-06-graph-layout)).
    - Indexes are `u32` when the projection has fewer than 2^32 nodes, otherwise `usize`.
    - The projection and all run state are reserved from the memory accountant before they are built. If the reservation fails, the run fails (FR-LIM-001). A graph whose projection does not fit within the memory cap therefore cannot run an algorithm.
  - **Implementations.**
    - Algorithms that petgraph provides are called on the projection.
    - Every other inventory entry is written in lpg over petgraph's visit traits.
    - Dense linear algebra uses `nalgebra`: ALG-076 spectral clustering, ALG-143 Laplacian matrix, ALG-151 number of spanning trees.
  - **Parallelism.**
    - Every algorithm accepts `concurrency` (default 4), the maximum number of worker threads. Workers are `std::thread::scope` threads started by the run.
    - **Parallel algorithms** are exactly those for which a reference source of the inventory (Neo4j GDS, Memgraph MAGE, petgraph or graphalgs) provides a parallel version. They run on `concurrency` threads. Every other algorithm runs on one thread, whatever `concurrency` is set to.
    - The classification of each inventory entry is recorded in this file, with the reference that justifies it, as the entry is implemented. Every entry is classified before the first release (NFR-REL-004); the classification has not been done yet.
    - Work is split into blocks of node indexes. The block size is `max(1, min(4096, ceil(n / 64)))`, where `n` is the projected node count. It depends only on `n`, never on `concurrency`, and gives at least two blocks whenever `n ≥ 2`, so a parallel run with `concurrency` > 1 uses more than one thread. Threads take blocks in index order.
    - Per-block partial results are merged in block order, so floating-point sums are always added in the same order.
    - Iterative algorithms use synchronous rounds: round *k* reads only the state of round *k*−1.
  - **Randomness.** Randomised algorithms take a `seed` parameter. When it is absent, a fixed default seed (0) is used. Each block draws from its own generator, seeded from `(seed, block index)`.
  - **Determinism.** It follows that, for one snapshot and equal parameters, every run gives equal results, whatever `concurrency` is.
  - **Write-back** writes properties through the caller's transaction write set ([TS-14](transactions-and-recovery.md#ts-14-mvcc)).
- **Satisfies**: FR-ALG-001, FR-ALG-002, FR-ALG-003, FR-ALG-004, FR-ALG-005, FR-ALG-006, FR-LIM-001, FR-LIM-003, NFR-REL-004, NFR-PERF-007.
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
  - petgraph `Csr` implements `IntoNeighbors`, `IntoEdges`, `NodeIndexable` and the other visit traits (R-PETGRAPH `crates/petgraph/src/csr.rs:551-811`). Its parallel functions need its optional `rayon` feature (`crates/petgraph/Cargo.toml:50`), which is not enabled.
  - graphalgs, a source of the inventory, uses petgraph and nalgebra (R-GALGS `Cargo.toml`; `src/spec/laplacian.rs`, `src/spec/st.rs`).
- **Crates**:
  - `petgraph` 0.8.3 (MIT OR Apache-2.0, 533.5 M downloads): dependency, default features only.
  - `nalgebra` 0.35.0 (Apache-2.0, 92.7 M downloads): dependency.
  - `rand` 0.10.3: dependency.
  - `rayon` 1.12.0 (MIT OR Apache-2.0, 573.6 M downloads): rejected, because its reductions have no fixed order (D-RAYON).
  - `rustworkx-core` 0.18.1: study only; it pulls in rayon, ndarray and several other crates.
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
