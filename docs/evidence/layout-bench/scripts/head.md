# Graph layout benchmark (D7): results

Date: 2026-10-02. Every number below was measured in this session on the host described here, except where marked **derived**.

## Question

Which on-disk adjacency layout best meets D1/D3 (MVCC-compatible, fast traversals, least CPU, memory and I/O)? Under the 2026-10-02 directive the primary case is a graph much larger than memory: buffer cache at **10 %** of the data, with **1 %** as the stress case.

## Summary

- **Among the MVCC-capable layouts (A-D), B (clustered adjacency B+tree with compressed leaves) reads the fewest device bytes in all 60 device cells measured** (4 graph configurations × 25/10/1 % cache × 5 workloads). It is also the smallest of them: 46-61 B/edge in total, 18-33 B/edge of adjacency.
- The fixed-width tree A reads 1.18-9.85× more device bytes than B under cache pressure. It is 2.6-2.8× larger in total and 4.0-5.6× larger in adjacency bytes. A is faster than B only when every page is resident, where B takes 1.2-1.85× A's time (for example 1-hop 1.14 vs 1.44 µs, degree-weighted 2-hop 8.0 vs 12.8 ms, Zipf online), and for single-edge writes (insert 2.3-2.8 vs 3.8-4.7 µs).
- Compressed CSR (G, read-only, no MVCC stamps) reads fewer bytes than B in all 40 cells at 25 % and 10 % cache. At 1 % cache it reads as many or more in 16 of 20 cells, because its offset array (16 B per node) costs an extra miss per lookup. At 100 M edges and 1 % cache, B reads 9.0 KiB per uniform 1-hop against G's 13.7.
- Record chains (D) read 9.5-4567× more device bytes than B on every constrained workload. Node blocks (C) are the fastest MVCC layout when fully resident (0.20-0.51 of B's time), but read 1.38-7.06× more device bytes than B and are 2.2-2.8× larger.
- **TS-06 is confirmed in structure** (clustered adjacency tree keyed `(node, direction, type, neighbour, edge)` with created/deleted stamps; record chains and node blocks rejected). **Its clause "fixed-width first, leaf compression only as a later optimisation" is contradicted** by the cache-pressure measurements and should change to compressed leaves from the start. See the Conclusion.

## Previous run: why it stopped at the 10 % cache case

- Evidence: `zipf-online.err` of the previous run is empty, so there was no panic or `time` line. The session transcript `~/.claude/projects/-data-dev-xumiga-lpg/fc90747b-…jsonl` records, at 16:10:37 UTC (17:10:37 local), the shell task "Run full-scale power-law online benchmark" as "stopped by main session". All the other background tasks were stopped within the same second, the subagent "Graph layout microbenchmark in Rust" as "stopped by user", and the session was then cleared (`/clear`). The last write to the output file was at 17:05:22, during the 10 % O_DIRECT 1-hop set.
- Cause: the process was killed from outside when the session was interrupted. It was not a defect in the benchmark. The run was also slow (48 min for one configuration) because of 20-60 s time budgets and long record-chain operations.
- Reproduction: the same configuration (`zipf online`, 1 M / 10 M) now runs through the 10 % and 1 % cases to exit 0 (`zipf-online.err`: `wall 1850.86 s, maxrss 5117296 KB, exit 0`).
- Fix: runs are launched detached (`setsid nohup`), so a session interruption cannot kill them, and time budgets were tightened (see Method).

## Layouts (D7 coverage)

| id | layout | MVCC stamps | status in this run |
|---|---|---|---|
| A | clustered adjacency B+tree, fixed 45 B entries (29 B key `(node u64, dir u8, type u32, nbr u64, edge u64)` + created + deleted) + edge table B+tree | yes | existing |
| B | same tree, compressed leaves (flag byte, node/neighbour deltas, varints, stamps elided when frozen/live, restart points every 16 entries) + edge table | yes | existing; bulk loader made streaming (same pages; verified identical space and digest at 20 K / 200 K) |
| C | per-node adjacency blocks (size classes 1..226 entries, relocation, chained pages) + edge table | yes | existing |
| D | record chains (Neo4j style) with logical id → slot maps for nodes and edges, dense-node groups above 50 | yes | existing |
| E | CSR, in-memory arrays, uncompressed (projection of one snapshot) | no | existing |
| F | CSR stored in pool pages, fixed 13 B entries | no | **added** (needed to measure CSR under 25/10/1 % cache) |
| G | CSR stored in pool pages, compressed (varint neighbour deltas inside type runs, varint edge id) | no | **added** (D7 "CSR with compression" was missing) |

Source: `layout-bench/src/` (`pcsr.rs` and `large.rs` new; `pool.rs` gained a sequential-miss counter; `main.rs` gained the F/G arms, a per-set skip for D, time budgets, the 1 % case and the RAM mode).

## Method

- **Graphs**: synthetic and deterministic (`generator.rs`). Endpoints are Zipf (α = 0.8, power-law degrees; max out-degree 133 939 at 1 M nodes) or uniform. There are 4 edge types and no self-loops. Node ids are permuted so that id and degree are uncorrelated.
- **Construction**: *online* inserts edges one at a time in random order (`created_ts` = edge id + 1). *Bulk* is a sorted bottom-up build (`created_ts` = 0, frozen).
- **Storage**: 8 KiB pages behind one single-threaded buffer pool per layout (clock eviction). Cache sizes are **frames = 25 %, 10 % or 1 % of the smallest MVCC layout's pages (A-D)**, the same absolute RAM for every layout. Each layout's own percentage is printed in the raw files; for example at 10 %, A gets 3.8 % of its pages and G 29-40 %. 100 % means every page is resident and no I/O happens.
- **Two ways of serving a miss**:
  1. **Device** (primary calibration): `pread` with `O_DIRECT` on the NVMe (`/data/tmp/lpg-layout-bench`), one page per miss, queue depth 1, no read-ahead. This is cold for the OS cache. Root is not available (`sudo` requires a password), so the OS cache could not be dropped; O_DIRECT bypasses it.
  2. **RAM-backed** (coordinator's faster method): buffered `pread` with the whole file pre-read into the OS page cache. A miss costs a syscall plus an 8 KiB memcpy (measured 2.5-3.4 µs/miss). The miss counts match the device run wherever no time budget truncated a set (for example A, 10 %, 1-hop uniform: 1.02 vs 1.06 misses/op). That supports treating miss counts as a property of layout and cache ratio.
- **Metrics per workload**: median ns/op over 3 measured rounds with [min, max] across rounds; device KiB read per traversal (Linux `/proc/self/io` `read_bytes`, checked to equal misses × 8 KiB: "io/miss 1.00" in the raw files); misses/op and the share of misses that read the page after the previous miss ("seq"); ns/edge.
- **Workloads**: 1-hop OUT (all types / type = 2; uniform or degree-weighted start), 2-hop distinct (uniform / degree-weighted), 3-hop distinct and unbounded BFS (all-resident and RAM-backed modes only), single-edge insert, delete (stamp `deleted_ts`), and an MVCC visibility check (layouts A-D must agree with the generator at two snapshots).
- **Repetition and interleaving**: every set runs in 3 measured rounds. The arm order rotates each round. In the all-resident phase, A runs twice per round at opposite positions to give a same-versus-same noise floor. Each round draws its own random starts in the constrained phases.
- **Noise floor** (all-resident phase, |A − A(rep)| / min, 28 workloads): median 6.6 %, max 12.8 %. Differences below about 13 % in that phase are not findings.
- **Correctness gates** (the run aborts on failure): identical per-operation results (count and checksum) across all layouts; 1-hop counts equal the generator's out-degrees; the CSR projection digest is equal from A, B and C; the MVCC counts match the generator.
- **Time budgets** (to fit the time limit): a set stops for an arm after its budget of 4 s (all-resident), 5 s (device), 8 s (RAM-backed) or 20 s (warm-up), checked after each operation. Truncated cells are marked "(n ops)". **D is not run** on 2-hop degree-weighted in device mode, nor on 3-hop and BFS in constrained modes. Its measured cost on these sets in the earlier P4 run was 18.6 s, 7.1 s and 57 s per operation at 25 %.

## Hardware and environment (reference machine)

- CPU: AMD Ryzen 9 5900HX, 8 cores / 16 threads, 16 MiB L3; governor `powersave` (frequency not pinned); clock source HPET (an `Instant::now()` pair costs 2.79 µs, so 1-hop and 2-hop are timed in batches).
- RAM: 31.5 GiB (`MemTotal 32263888 kB`), 2 GiB swap.
- Page files: Samsung SSD 970 EVO Plus 1 TB (firmware 2B2QEXM7), `/dev/nvme1n1p1`, ext4 (`rw,nosuid,nodev,relatime`), scheduler `none`, mounted at `/data`; directory `/data/tmp/lpg-layout-bench` (removed after the runs).
- OS: Linux 6.8.0-139-generic. Toolchain: rustc / cargo 1.96.0; release profile with `debug = true`; dependency `libc 0.2` (O_DIRECT flag only).
- Other processes: mysqld, mongod, java, redis-server and dockerd were resident and idle. Load average was 0.7-1.6 before and after each run (about 1.0 is the benchmark thread itself).
- Binaries: v1 (device matrix) sha256 `66b2750c…170107`; v2 (RAM matrix, 100 M point; adds the miss counter, RAM mode and large mode) sha256 `66a028aa…235e427`. Sources are frozen in `bin-v1/src`, `bin-v2/src`.

## Commands

```
cd layout-bench && cargo build --release
# device matrix (run-1m.sh), 1 M nodes / 10 M edges, for d in zipf uniform, c in online bulk:
bin-v1/layout-bench $d $c 1000000 10000000 /data/tmp/lpg-layout-bench/1m 1.0 > lb-results/$d-$c.md
# RAM-backed matrix (run-ram.sh):
LB_RAM=1 bin-v2/layout-bench $d $c 1000000 10000000 /data/tmp/lpg-layout-bench/1m 1.0 > lb-results/ram-$d-$c.md
# 100 M edge point (run-ram.sh), layouts B and G, 1 GiB build cache per layout:
bin-v2/layout-bench large zipf 10000000 100000000 /data/tmp/lpg-layout-bench/large BG 1024 > lb-results/large-zipf-bulk.md
```

Each run was wrapped in `/usr/bin/time -f "wall %e s, maxrss %M KB, exit %x"` (in `*.err`). The start and end times and load averages are in `run-1m.log` and `run-ram.log`. Wall times: device matrix 31 / 12 / 8 / 5 min, RAM matrix 16 / 6 / 6 / 5 min, 100 M point 9.5 min; total about 1 h 40 min of runs.

## How to read the tables

Cells: `median [min-max]` in **µs per operation**, then `KiB` = device bytes read per operation (device tables), or `miss` = misses per operation with the sequential share (RAM-backed tables), then `ns/e` = ns per edge scanned for workloads with large fan-out. Columns: `1h-u` 1-hop uniform start, `1h-t2` 1-hop type = 2, `1h-dw` 1-hop degree-weighted start, `2h-u` / `2h-dw` 2-hop distinct, `3h-u` 3-hop distinct, `BFS` unbounded BFS. For degree-weighted sets truncated by a budget, compare ns/e rather than µs per operation, since different arms then cover different start subsets.
