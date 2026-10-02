
## Scale trend, 1 M → 10 M nodes (Zipf, bulk load; B and G)

| metric | B 1 M / 10 M | B 10 M / 100 M | G 1 M / 10 M | G 10 M / 100 M |
|---|---|---|---|---|
| tree height | 3 | 4 | - | - |
| adjacency B/edge | 18.1 | 19.7 | 15.0 | 16.5 |
| total B/edge (B includes the 28.2 B/edge edge table) | 46.3 | 47.9 | 15.0 | 16.5 |
| bulk-load throughput, edges/s | 2.62 M (includes the 1 M-edge sort) | 1.88 M (excludes the shared 68.2 s sort; 1 GiB build cache, 4.46 GiB written) | page writes only: 0.66 s | 3.57 M (includes the in-memory CSR build; 1.54 GiB written) |
| peak RSS during build | process-wide only (all layouts in one process: 4.5-5.2 GiB) | 2725 MiB (1662 MiB before: about +1.0 GiB, the build cache) | process-wide only | 7224 MiB (2715 MiB before: the in-memory CSR and its encoded stream) |
| 10 % cache, device: 1-hop uniform | 74.5 µs, 6.0 KiB | 78.7 µs, 6.7 KiB | 39.7 µs, 3.5 KiB | 44.0 µs, 6.2 KiB |
| 10 % cache, device: 2-hop uniform | 1517 µs, 91.6 KiB | 4830 µs, 134.1 KiB | 925 µs, 55.5 KiB | 3537 µs, 101.0 KiB |
| 1 % cache, device: 1-hop uniform | 98.1 µs, 7.9 KiB | 115 µs, 9.0 KiB | 150 µs, 13.3 KiB | 167 µs, 13.7 KiB |
| 1 % cache, device: 1-hop degree-weighted | 255 µs, 20.6 KiB | 383 µs, 35.0 KiB | 274 µs, 21.5 KiB | 384 µs, 31.8 KiB |
| 1 % cache, device: 2-hop uniform | 2995 µs, 253 KiB | 5687 µs, 283 KiB | 3247 µs, 279 KiB | 4883 µs, 230 KiB |

The 2-hop cost grows with graph size at a fixed cache ratio partly because the 100 M graph has more edges per 2-hop (larger hub degrees: max out-degree 825 603 vs 133 939); compare KiB within one scale.

## Largest graph per layout on this machine (derived, not measured)

Free space on `/data`: 478 GiB (`df -B1G`). Edges that fit = free bytes / total B/edge. These are adjacency plus edge table only, with no node records, properties, WAL or free-space overhead.

| layout | B/edge used | source of B/edge | edges that fit (derived) |
|---|---|---|---|
| A btree-fixed | 129.8 | 1 M bulk | 3.95 G |
| B btree-compressed | 47.9 | 100 M bulk | 10.7 G |
| B btree-compressed, online-built | 59.8 | 1 M online | 8.6 G |
| C node-blocks | 126.6 | 1 M bulk | 4.05 G |
| D record-chains | 68.6 | 1 M bulk | 7.5 G |
| F csr-paged | 27.6 | 1 M | 18.6 G |
| G csr-paged-compressed | 16.5 | 100 M | 31.1 G |

## Conclusion

**Best for graphs much larger than memory: B, the clustered adjacency B+tree with compressed leaves**, among the layouts that meet D1's MVCC requirement. The evidence:

1. **I/O, the primary criterion at giant scale.** B reads the fewest device bytes of A-D in **60 of 60** device cells (25/10/1 % cache × 4 configurations × 5 workloads). At the primary 10 % case (1 M nodes), B reads 6.0-6.7 KiB per uniform 1-hop against A's 8.2-8.5. On uniform 2-hop it reads 65-114 KiB against A's 89-594, and on 1-hop degree-weighted 6.1-13.3 against 8.2-57.1. Its latency follows: 2-hop uniform takes 785-1844 µs against A's 1092-7235.
2. **Footprint.** B is 46-61 B/edge in total (18-33 adjacency) against A's 130-162 (102-133). At equal RAM, B therefore keeps 2.6-2.8× more of its data cached, and that is where most of the I/O gap comes from.
3. **The trend holds at 10×.** From 1 M to 100 M edges, B's adjacency grows from 18.1 to 19.7 B/edge and the tree from height 3 to 4. Uniform 1-hop at 10 % goes from 6.0 to 6.7 KiB and 74.5 to 78.7 µs. Bulk load runs at 1.88 M edges/s in bounded memory (+1.0 GiB, the build cache).
4. **The price of B.** When every page is resident, B takes 1.2-1.85× A's CPU time per traversal. A single-edge insert costs 3.8-4.7 µs against 2.3-2.8 µs, and a delete (stamp) 4.6-5.0 µs against 2.4-2.6 µs (all resident). Pages dirtied per insert are the same (3.0). Under the giant-graph criterion this CPU cost is smaller than the I/O cost it saves: one device miss cost 38-109 µs on this NVMe (per-cell means).

**Read-optimised CSR.** Compressed CSR (G) reads fewer bytes than B in all 40 cells at 25 % and 10 % cache; for example at 10 %, 2.4-3.8 vs 6.0-6.7 KiB per uniform 1-hop at 1 M. It does not win where the cache is tiny relative to node count. At 1 % cache it reads as much as or more than B in 16 of 20 cells, and at 100 M / 1 % it reads 13.7 vs 9.0 KiB per uniform 1-hop. Its dense offset array (16 B per node and both directions; 160 MB at 10 M nodes) misses on almost every lookup, while B's inner pages (about 0.5 % of the tree) stay cached. G is therefore a candidate for an optional read-only analytic projection, not for the primary store.

**CSR and MVCC, stated explicitly.** CSR (E, F, G) holds one snapshot. It has no created/deleted stamps, and an edge cannot be inserted or deleted in place.
- *What was measured*: the cost of rebuilding the projection. At 1 M / 10 M, a resident scan of A or B takes 0.24-0.37 s and writing F or G 0.18-0.70 s. At 100 M, G is built in 28.0 s from the shared 68.2 s sort.
- *What was not measured*: any per-update path. That means a delta store merged at read time, a rebuild concurrent with readers, the memory needed to keep the old and new snapshot during a swap, and the read cost of filtering a delta. The CSR read figures here are an upper bound for a CSR that also had to honour MVCC visibility.

**TS-06 verdict.**
- **Confirmed**: one clustered adjacency tree per graph keyed `(node_id, direction, edge_type, neighbour_id, edge_id)`, two entries per edge, each with `created_ts` and `deleted_ts`, plus the edge tree. Record chains are rejected: 9.5-4567× B's device bytes. Node blocks are rejected: 1.38-7.06× B's device bytes, 2.2-2.8× B's size; they are the fastest only when resident.
- **Must change**: "fixed-width entries first; leaf prefix compression (P4 model B) as the first candidate optimisation, added only when the NFR-PERF-002 baseline shows a target missed". Under the giant-graph criterion (10 % cache primary, 1 % stress), the fixed-width format A loses to B in every cache-pressure cell by 1.18-9.85× in device bytes and is 2.6-2.8× larger. The measured data support compressed leaves (B: deltas, varints, stamps elided when frozen or live, restart points) as the initial on-disk format. The cost is 1.2-1.85× CPU when resident and about 1.7-1.9× per single-edge write.
- **Not measured, so not settled**: concurrent access (latches, contention) for either format, and the overhead of B's group re-encode under concurrent writers. These need the NFR-PERF-002 harness.

## Refuted or weakened hypotheses

- "CSR is always the most I/O-efficient layout for reads": refuted at 1 % cache and at 10 M nodes (see above).
- "Fixed-width entries are cheaper overall because decoding costs CPU": true only when every page is resident (B 1.2-1.85× slower). False under cache pressure.
- "Record chains with an id-to-slot map are competitive with a fixed node-id-to-slot map" was not tested as such. Both maps (`nmap`, `emap`) exist in D, and D's I/O is dominated by chain pointer chasing: 12.3-26.6 distinct pages per 1-hop, all resident, Zipf.

## Limits of the method

- One host, one device (Samsung 970 EVO Plus), one synthetic graph family (Zipf α = 0.8 or uniform, 4 types, no properties, no self-loops). Real graphs with community structure may have better locality for every layout.
- Single-threaded buffer pool with clock eviction. No latching, no concurrency, no WAL, no checksums. Device reads are synchronous at queue depth 1 with no prefetch, so sequential-miss shares (up to 75 % for B on degree-weighted sets at 100 M) are not exploited. A read-ahead design would favour layouts with sequential misses (bulk-built B, F, G) more than shown.
- The OS page cache could not be dropped (no root). Device reads use O_DIRECT, which bypasses it. The SSD's internal DRAM cache may still serve some reads; per-miss latency was 38-109 µs (per-cell means) on this drive.
- The RAM-backed mode serves misses from the OS page cache through `pread` (2.5-3.4 µs per miss). It is not an in-process `Vec` device, so its latencies include a syscall.
- Equal absolute RAM for all layouts, set as a fraction of the smallest MVCC layout. Smaller layouts therefore hold a larger fraction of themselves; this is the intended comparison under the "least memory" criterion, but it is not "each layout at x % of its own size".
- Repetitions are 3 rounds inside one process per configuration, interleaved, reported as median [min, max]. There are no repeated processes or rebuilds, and the 1 M matrix ran once per mode. The 100 M point ran once. Noise floor (all-resident only): median 6.6 %, max 12.8 %.
- Time budgets truncated some degree-weighted cells (marked "(n ops)"). D was not run on 3 slow workload/mode combinations. 3-hop and BFS were measured only all-resident and RAM-backed, not with device reads.
- Space for A-D includes a 28 B/edge edge table that CSR does not have; the adjacency B/edge columns are the like-for-like comparison. No node records or properties are stored in any layout.
- At 1 M, peak RSS is process-wide (all layouts plus generator in one process: 4642-5364 MiB). Per-layout build peak RSS was measured only at 100 M (by resetting `VmHWM` via `/proc/self/clear_refs`).
- The 100 M point covers B and G only (the coordinator's directive) and only bulk load, Zipf, 10 % and 1 % cache, 1-hop and 2-hop. A, C, D, F and the uniform and online variants were not run at that size; inserts and deletes were not measured at 100 M.
- 1 M bulk build times include the sort; 100 M build times exclude the shared sort (68.2 s) and graph generation (62.8 s).
- CPU governor `powersave` with frequency not pinned; HPET clock (2.79 µs per `Instant` pair), so 1-hop and 2-hop are batch-timed and have no per-operation percentiles.
- Total run time was about 1 h 40 min of runs, above the 1.5 h target, because the 1 % case, the RAM-backed matrix and the 100 M point were added during the task.

## Files

- Raw outputs: `lb-results/{zipf,uniform}-{online,bulk}.md` (device matrix), `lb-results/ram-*.md` (RAM-backed matrix), `lb-results/large-zipf-bulk.md` (100 M point), matching `*.err` (wall time, max RSS, exit status), `run-1m.log`, `run-ram.log`.
- Crate: `layout-bench/`; frozen binaries and sources: `bin-v1/`, `bin-v2/`. Run scripts: `run-1m.sh`, `run-ram.sh`. Table generation: `mk_tables.py`, `assemble.py`.
