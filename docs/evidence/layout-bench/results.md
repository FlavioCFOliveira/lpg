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


## 1 M nodes / 10 M edges: zipf-online

`layout-bench zipf online n=1000000 m=10000000 alpha=0.8 scale=1`

**Build (online inserts in random order, created_ts = edge id + 1)**

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 20.25 | 493922 | 10.63 | 3.04 |  | 1636 |
| B | 39.20 | 255085 | 9.22 | 3.01 |  | 2210 |
| C | 5.77 | 1732281 | 9.88 | 4.99 | relocations/edge 0.469, entries copied/edge 2.06 | 3448 |
| D | 14.14 | 707422 | 24.93 | 6.90 | existing records rewritten/edge 5.16, densified nodes 45809 | 4106 |

**CSR projection (E) built by one sequential pass, all pages resident**

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.31 | 32332126 | 158832 | a7c193c4f2af92bf |
| B btree-compressed | 0.37 | 26767335 | 38270 | a7c193c4f2af92bf |
| C node-blocks | 0.67 | 15004968 | 3945261 | a7c193c4f2af92bf |

**Paged CSR (F fixed, G compressed) written from the projection**

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.31 | 33702 |
| G csr-paged-compressed | 0.67 | 18369 |

**Space**

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 159857 | 131.0 | 34466 | 159.2 | leaves 158829, height 4, entries 20000000, leaf fill 0.69; edge table 34466 pages |
| B btree-compressed | 38526 | 31.6 | 34466 | 59.8 | leaves 38267, height 4, entries 20000000, leaf bytes used 23.6 B/edge (11.8 B/entry), leaf fill 0.75; edge table 34466 pages |
| C node-blocks | 122826 | 100.6 | 34466 | 128.9 | dir 3922, block slabs 91452, chain 27452; entry bytes 72.0 B/edge, block bytes reserved 70.2 B/edge; relocations 4688476, entries copied 20643765; edge table 34466 pages |
| D record-chains | 67495 | 55.3 | 16225 | 68.6 | node recs 2942, rel recs 64103, groups 450; node map 1475 pages, edge map 14750 pages; densified 45809 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 18369 | 15.0 | 0 | 15.0 | offset pages 1954, entry pages 16415, entry stream 13.4 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |
RSS after build: 4793 MiB, peak 4857 MiB

**Warm: all pages resident**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 1.14 [1.14-1.17] | 1.09 [1.08-1.19] | 23.4 [23.3-23.9]; 5.9 ns/e | 210 [207-220] | 8,000 [7,763-8,067]; 13.1 ns/e | 37,275 [37,192-38,316]; 17.5 ns/e | 875,590 [875,141-928,953]; 88.5 ns/e |
| B btree-compressed | 1.44 [1.43-1.47] | 1.33 [1.33-1.44] | 39.8 [39.7-40.5]; 10.0 ns/e | 336 [330-354] | 12,842 [12,719-13,375]; 21.0 ns/e | 58,809 [58,455-58,872]; 27.6 ns/e | 1,213,879 [1,203,992-1,219,837]; 122.7 ns/e |
| C node-blocks | 0.437 [0.434-0.447] | 0.418 [0.407-0.419] | 11.4 [11.4-12.1]; 2.9 ns/e | 172 [169-183] | 4,582 [4,445-4,712]; 7.5 ns/e | 18,521 [18,072-18,823]; 8.7 ns/e | 247,525 [246,094-256,529]; 25.0 ns/e |
| D record-chains | 3.37 [3.36-3.38] | 2.64 [2.63-2.65] | 561 [559-581]; 143.3 ns/e (21264 ops) | 3,053 [3,036-3,486] | 156,608 [156,517-156,774]; 149.8 ns/e (96 ops) | 427,409 [427,065-431,071]; 156.5 ns/e (30 ops) | 2,367,390 [2,367,390-2,383,140]; 239.3 ns/e |
| F csr-paged | 0.323 [0.316-0.328] | 0.3 [0.292-0.304] | 7.14 [7.14-7.24]; 1.8 ns/e | 129 [128-138] | 3,597 [3,567-3,616]; 5.9 ns/e | 13,537 [13,424-13,610]; 6.3 ns/e | 169,124 [168,799-180,134]; 17.1 ns/e |
| G csr-paged-compressed | 0.398 [0.39-0.403] | 0.398 [0.391-0.401] | 26.2 [26.2-26.3]; 6.6 ns/e | 216 [213-254] | 6,218 [6,180-6,356]; 10.2 ns/e | 23,489 [23,445-23,580]; 11.0 ns/e | 234,327 [231,882-257,131]; 23.7 ns/e |
| E csr-memory | 0.269 [0.259-0.27] | 0.252 [0.242-0.252] | 5.61 [5.59-5.62]; 1.4 ns/e | 91.5 [90.5-94.3] | 2,366 [2,353-2,392]; 3.9 ns/e | 8,720 [8,659-8,742]; 4.1 ns/e | 95,577 [95,024-98,397]; 9.7 ns/e |
| A btree-fixed(rep) | 1.09 [1-1.14] | 1.06 [0.927-1.18] | 23.4 [23.3-23.7]; 5.8 ns/e | 215 [204-216] | 8,025 [7,771-8,218]; 13.1 ns/e | 37,456 [37,204-38,249]; 17.6 ns/e | 878,943 [876,534-941,793]; 88.8 ns/e |

**Constrained 25%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 97.7 [96.6-101]; 8.1 KiB | 90.8 [90.2-92.1]; 7.4 KiB | 528 [390-583]; 38.8 KiB; 151.4 ns/e | 3,151 [3,133-3,717]; 247.4 KiB | 631,667 [391,704-847,783]; 50508.5 KiB; 937.5 ns/e (48 ops) |
| B btree-compressed | 55.4 [53.5-82.2]; 5.1 KiB | 53.1 [52.9-75.1]; 4.8 KiB | 95.6 [83.9-215]; 7.3 KiB; 24.2 ns/e | 1,292 [1,011-1,619]; 68.7 KiB | 133,591 [115,606-359,742]; 13067.4 KiB; 267.8 ns/e (76 ops) |
| C node-blocks | 90 [85.5-107]; 7.6 KiB | 84.1 [81.5-92.6]; 7.0 KiB | 295 [206-357]; 21.8 KiB; 85.3 ns/e | 2,199 [2,096-2,479]; 154.9 KiB | 433,526 [236,674-610,308]; 29965.7 KiB; 643.4 ns/e (62 ops) |
| D record-chains | 1,222 [1,161-1,296]; 100.9 KiB | 852 [822-856]; 69.4 KiB | 659,839 [201,999-750,639]; 33339.6 KiB; 75632.1 ns/e (80 ops) | 3,785,610 [3,022,903-4,398,065]; 309131.0 KiB (48 ops) | not run |
| F csr-paged | 1.22 [0.462-60.1]; 1.7 KiB | 1.03 [0.416-52.4]; 1.4 KiB | 10.7 [8.35-117]; 3.7 KiB; 2.4 ns/e | 265 [204-911]; 20.8 KiB | 4,544 [3,122-21,616]; 538.2 KiB; 6.3 ns/e |
| G csr-paged-compressed | 0.553 [0.548-37.7]; 1.2 KiB | 0.538 [0.533-27.5]; 0.9 KiB | 28.8 [22.4-76.4]; 1.6 KiB; 6.5 ns/e | 317 [260-575]; 9.4 KiB | 7,693 [4,854-9,699]; 146.3 KiB; 10.1 ns/e |

**Constrained 10%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 103 [102-108]; 8.5 KiB | 95.3 [94-98.6]; 7.8 KiB | 715 [706-739]; 57.1 KiB; 191.9 ns/e | 7,235 [7,078-8,371]; 594.0 KiB | 818,704 [499,346-1,116,689]; 66095.0 KiB; 1215.1 ns/e (48 ops) |
| B btree-compressed | 81.4 [79.7-82.8]; 6.6 KiB | 77.3 [75.7-80.9]; 6.3 KiB | 227 [184-229]; 13.3 KiB; 58.9 ns/e | 1,844 [1,800-2,247]; 113.7 KiB | 373,719 [199,015-509,318]; 25172.4 KiB; 554.7 ns/e (62 ops) |
| C node-blocks | 123 [113-139]; 10.2 KiB | 114 [109-129]; 9.6 KiB | 418 [393-418]; 31.6 KiB; 108.5 ns/e | 3,660 [3,309-4,179]; 268.5 KiB | 647,930 [385,229-934,909]; 51987.3 KiB; 961.7 ns/e (48 ops) |
| D record-chains | 1,644 [1,600-1,694]; 135.6 KiB | 1,164 [1,121-1,171]; 95.0 KiB | 696,491 [246,108-803,765]; 37030.1 KiB; 80984.9 ns/e (80 ops) | 4,462,237 [3,369,116-5,207,038]; 360201.0 KiB (48 ops) | not run |
| F csr-paged | 60.7 [59.8-64.9]; 5.0 KiB | 55.6 [52.6-61.3]; 4.6 KiB | 118 [106-134]; 9.5 KiB; 30.6 ns/e | 1,327 [1,305-1,592]; 89.8 KiB | 118,900 [109,095-189,509]; 16605.2 KiB; 249.4 ns/e |
| G csr-paged-compressed | 19.5 [17.8-36.2]; 2.4 KiB | 17.9 [17.5-27.5]; 2.0 KiB | 60.3 [50.1-75.1]; 3.3 KiB; 14.5 ns/e | 619 [595-735]; 30.2 KiB | 49,887 [45,453-85,150]; 5352.1 KiB; 106.9 ns/e |

**Constrained 1%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 172 [171-173]; 14.1 KiB | 163 [163-164]; 13.4 KiB | 3,261 [2,815-3,770]; 265.2 KiB; 785.0 ns/e (4704 ops) | 26,775 [26,679-31,186]; 2293.7 KiB | 944,414 [589,981-1,223,113]; 74756.0 KiB; 1401.7 ns/e (48 ops) |
| B btree-compressed | 112 [104-114]; 8.9 KiB | 104 [104-104]; 8.4 KiB | 560 [503-567]; 40.1 KiB; 145.3 ns/e | 6,197 [5,804-6,996]; 466.5 KiB | 466,952 [259,888-608,009]; 31592.8 KiB; 693.1 ns/e (62 ops) |
| C node-blocks | 182 [177-183]; 14.9 KiB | 183 [180-184]; 14.9 KiB | 1,660 [1,492-1,729]; 131.4 KiB; 430.6 ns/e | 16,037 [15,983-17,990]; 1318.1 KiB | 929,975 [535,387-1,403,046]; 76336.0 KiB; 1380.3 ns/e (48 ops) |
| D record-chains | 2,385 [2,242-2,387]; 193.2 KiB | 1,415 [1,361-1,429]; 115.6 KiB | 1,255,125 [410,259-1,445,613]; 65030.2 KiB; 145655.6 ns/e (80 ops) | 4,662,577 [3,419,535-5,433,145]; 373671.8 KiB (48 ops) | not run |
| F csr-paged | 159 [156-159]; 12.9 KiB | 156 [155-157]; 12.7 KiB | 370 [350-373]; 40.2 KiB; 96.9 ns/e | 4,405 [4,246-5,156]; 475.2 KiB | 215,302 [190,788-325,189]; 27863.9 KiB; 428.0 ns/e |
| G csr-paged-compressed | 150 [147-150]; 12.6 KiB | 147 [146-148]; 12.5 KiB | 263 [260-266]; 19.7 KiB; 67.6 ns/e | 3,163 [3,118-3,832]; 260.7 KiB | 174,790 [142,456-238,258]; 21068.8 KiB; 313.6 ns/e |

**Constrained 25%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 5.62 [5.53-6.26]; 0.93 miss (0.0% seq) | 5.26 [5.19-5.58]; 0.86 miss (0.0% seq) | 49.2 [47.8-53.7]; 5.85 miss (0.0% seq); 12.8 ns/e | 400 [373-458]; 30.89 miss (0.0% seq) | 24,541 [23,606-48,926]; 5253.24 miss (0.0% seq); 50.6 ns/e | 153,516 [118,001-259,849]; 29744.33 miss (0.0% seq); 98.1 ns/e | 5,096,012 [5,070,420-5,100,770]; 875733.00 miss (0.0% seq); 515.0 ns/e |
| B btree-compressed | 3.37 [3.31-5.19]; 0.51 miss (0.0% seq) | 3.18 [3.16-4.66]; 0.48 miss (0.0% seq) | 46.3 [43-52.5]; 1.56 miss (0.0% seq); 12.0 ns/e | 464 [458-582]; 10.67 miss (0.0% seq) | 15,091 [13,091-28,681]; 1749.87 miss (0.0% seq); 30.2 ns/e | 84,478 [68,673-143,403]; 11014.07 miss (0.0% seq); 54.0 ns/e | 2,947,268 [2,931,254-2,985,182]; 365206.67 miss (0.0% seq); 297.9 ns/e |
| C node-blocks | 3.26 [3.23-5.74]; 0.73 miss (0.4% seq) | 3.39 [3.26-5.12]; 0.69 miss (0.4% seq) | 26 [25-29.4]; 3.47 miss (5.3% seq); 6.5 ns/e | 314 [273-351]; 19.48 miss (8.7% seq) | 15,815 [14,137-32,126]; 3349.34 miss (6.1% seq); 31.7 ns/e | 105,825 [77,446-177,607]; 20828.93 miss (4.8% seq); 67.6 ns/e | 3,374,491 [3,359,598-3,414,425]; 623892.00 miss (0.5% seq); 341.0 ns/e |
| D record-chains | 63.2 [60.9-66.2]; 12.59 miss (0.0% seq) | 44.5 [42.8-44.7]; 8.68 miss (0.0% seq) | 15,609 [13,207-15,845]; 3080.45 miss (0.0% seq); 3748.8 ns/e (1648 ops) | 130,725 [122,889-170,048]; 28551.99 miss (0.0% seq) (192 ops) | 2,440,960 [1,650,565-2,908,697]; 480703.19 miss (0.0% seq); 3622.9 ns/e (48 ops) | not run | not run |
| F csr-paged | 0.407 [0.405-3.21]; 0.20 miss (1.2% seq) | 0.371 [0.362-2.89]; 0.18 miss (1.1% seq) | 8.45 [6.47-13.1]; 0.46 miss (62.3% seq); 1.9 ns/e | 202 [175-215]; 2.59 miss (34.1% seq) | 3,689 [2,997-4,625]; 66.71 miss (17.9% seq); 6.1 ns/e | 10,174 [9,703-16,406]; 3.03 miss (1.1% seq); 6.5 ns/e | 186,137 [179,408-186,800]; 0.67 miss (0.0% seq); 18.8 ns/e |
| G csr-paged-compressed | 0.465 [0.456-2.51]; 0.15 miss (0.7% seq) | 0.456 [0.444-1.99]; 0.11 miss (0.2% seq) | 28.3 [23.3-30.5]; 0.20 miss (47.7% seq); 6.8 ns/e | 283 [268-326]; 1.18 miss (19.5% seq) | 5,153 [5,015-7,944]; 18.28 miss (10.8% seq); 10.5 ns/e | 19,551 [16,953-28,657]; 0.03 miss (0.0% seq); 11.7 ns/e | 256,858 [236,164-258,557]; 0.00 miss (-% seq); 26.0 ns/e |

**Constrained 10%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 6.21 [6.21-6.29]; 1.02 miss (0.0% seq) | 5.86 [5.73-5.87]; 0.95 miss (0.0% seq) | 56.4 [53.2-58.1]; 7.24 miss (0.0% seq); 14.6 ns/e | 591 [588-674]; 73.39 miss (0.0% seq) | 30,987 [28,968-60,807]; 6899.77 miss (0.0% seq); 62.1 ns/e | 165,503 [133,773-275,092]; 32921.17 miss (0.0% seq); 105.8 ns/e | 5,538,438 [5,528,201-5,590,048]; 985217.00 miss (0.0% seq); 559.7 ns/e |
| B btree-compressed | 5 [4.99-5.17]; 0.78 miss (0.0% seq) | 4.87 [4.74-4.89]; 0.75 miss (0.0% seq) | 46.4 [42.8-53]; 1.83 miss (0.0% seq); 12.1 ns/e | 473 [462-581]; 14.20 miss (0.0% seq) | 18,335 [16,875-35,483]; 2853.50 miss (0.0% seq); 36.7 ns/e | 112,435 [83,302-181,920]; 17992.43 miss (0.0% seq); 71.9 ns/e | 4,459,374 [4,423,847-4,497,034]; 734779.00 miss (0.0% seq); 450.7 ns/e |
| C node-blocks | 5.75 [5.67-6.08]; 1.09 miss (0.3% seq) | 5.86 [5.84-5.88]; 1.10 miss (0.2% seq) | 28.7 [28.4-31.3]; 4.07 miss (4.6% seq); 7.5 ns/e | 371 [329-416]; 34.38 miss (5.9% seq) | 22,011 [20,431-47,267]; 5289.08 miss (4.6% seq); 44.1 ns/e | 140,504 [105,223-235,629]; 29436.03 miss (3.7% seq); 89.8 ns/e | 5,530,774 [5,524,472-5,536,849]; 1079005.00 miss (0.3% seq); 559.0 ns/e |
| D record-chains | 83.2 [81.3-84.4]; 16.95 miss (0.0% seq) | 59 [55.6-59.7]; 11.87 miss (0.0% seq) | 16,876 [14,943-17,161]; 3454.61 miss (0.0% seq); 4254.7 ns/e (1504 ops) | 148,894 [131,652-192,278]; 32801.97 miss (0.0% seq) (176 ops) | 2,911,586 [1,975,591-3,434,247]; 584533.23 miss (0.0% seq); 4321.4 ns/e (48 ops) | not run | not run |
| F csr-paged | 2.71 [2.68-3.26]; 0.52 miss (1.1% seq) | 2.65 [2.56-2.88]; 0.49 miss (0.9% seq) | 14.1 [13.8-15.5]; 1.54 miss (59.1% seq); 3.7 ns/e | 227 [226-271]; 11.34 miss (27.8% seq) | 10,177 [9,589-18,989]; 2075.31 miss (48.7% seq); 20.5 ns/e | 62,398 [47,428-103,425]; 12677.10 miss (39.1% seq); 39.9 ns/e | 2,420,380 [2,406,930-2,429,214]; 465211.00 miss (8.6% seq); 244.6 ns/e |
| G csr-paged-compressed | 1.01 [0.948-2.52]; 0.22 miss (0.5% seq) | 0.993 [0.951-1.99]; 0.18 miss (0.2% seq) | 28.4 [26-32.7]; 0.59 miss (51.1% seq); 7.4 ns/e | 292 [282-342]; 4.02 miss (18.0% seq) | 7,463 [6,865-12,574]; 666.34 miss (30.0% seq); 15.0 ns/e | 32,712 [28,225-53,333]; 3778.03 miss (24.1% seq); 20.9 ns/e | 790,078 [784,837-795,532]; 108695.00 miss (4.4% seq); 79.8 ns/e |

**Constrained 1%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 9.37 [9.09-9.41]; 1.75 miss (0.0% seq) | 8.68 [8.66-8.69]; 1.67 miss (0.0% seq) | 161 [144-185]; 31.88 miss (0.0% seq); 41.6 ns/e | 1,544 [1,521-1,785]; 286.67 miss (0.0% seq) | 36,116 [33,187-64,508]; 7962.69 miss (0.0% seq); 72.4 ns/e | 180,975 [147,995-291,364]; 37836.27 miss (0.0% seq); 115.7 ns/e | 7,721,998 [7,716,234-7,722,358]; 1556955.67 miss (0.0% seq); 780.4 ns/e |
| B btree-compressed | 5.92 [5.92-6.2]; 1.07 miss (0.0% seq) | 5.75 [5.71-5.95]; 1.04 miss (0.0% seq) | 61.8 [55.6-66.6]; 5.02 miss (0.0% seq); 16.0 ns/e | 672 [647-844]; 58.35 miss (0.0% seq) | 21,324 [19,334-39,539]; 3630.29 miss (0.0% seq); 42.7 ns/e | 125,000 [95,835-199,410]; 21462.50 miss (0.0% seq); 79.9 ns/e | 5,330,003 [5,317,855-5,346,744]; 1000361.67 miss (0.0% seq); 538.7 ns/e |
| C node-blocks | 9.12 [8.93-9.26]; 1.87 miss (0.2% seq) | 8.98 [8.88-9.04]; 1.86 miss (0.1% seq) | 82.1 [74.8-87.3]; 16.42 miss (1.6% seq); 21.3 ns/e | 971 [929-1,065]; 164.76 miss (1.9% seq) | 30,020 [26,581-61,896]; 7613.62 miss (3.3% seq); 60.2 ns/e | 184,685 [134,669-307,946]; 42081.17 miss (2.6% seq); 118.0 ns/e | 8,305,445 [8,296,950-8,324,677]; 1800880.67 miss (0.2% seq); 839.4 ns/e |
| D record-chains | 113 [106-113]; 24.15 miss (0.0% seq) | 67.5 [65.3-68.7]; 14.45 miss (0.0% seq) | 25,918 [25,507-26,987]; 5993.65 miss (0.0% seq); 6847.3 ns/e (960 ops) | 146,414 [129,185-186,221]; 34013.48 miss (0.0% seq) (176 ops) | 3,006,391 [2,034,664-3,540,602]; 634159.17 miss (0.0% seq); 4462.1 ns/e (48 ops) | not run | not run |
| F csr-paged | 7.54 [7.42-7.63]; 1.60 miss (0.8% seq) | 7.28 [7.24-7.39]; 1.59 miss (1.0% seq) | 29.3 [26.3-29.3]; 5.02 miss (63.4% seq); 7.6 ns/e | 428 [419-499]; 59.40 miss (60.0% seq) | 14,742 [14,584-25,924]; 3482.99 miss (45.2% seq); 31.2 ns/e | 96,846 [74,798-145,912]; 21728.30 miss (33.6% seq); 59.4 ns/e | 6,056,159 [6,052,600-6,083,907]; 1385034.67 miss (5.5% seq); 612.1 ns/e |
| G csr-paged-compressed | 7.42 [7.16-7.46]; 1.57 miss (0.6% seq) | 7.07 [7.06-7.17]; 1.56 miss (0.4% seq) | 36.2 [34-40.1]; 2.47 miss (31.2% seq); 9.4 ns/e | 404 [398-482]; 32.59 miss (31.7% seq) | 14,070 [13,702-23,512]; 2633.60 miss (40.2% seq); 30.1 ns/e | 89,504 [70,290-133,369]; 17999.23 miss (34.4% seq); 54.3 ns/e | 5,894,189 [5,891,688-5,901,161]; 1334444.33 miss (6.0% seq); 595.7 ns/e |

**Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2380 [2377, 2396] | 11.10 | 3.04 |  |
| B btree-compressed | 4478 [4454, 4489] | 11.05 | 3.02 |  |
| C node-blocks | 751 [746, 751] | 9.89 | 5.01 |  |
| D record-chains | 1573 [1557, 1615] | 26.66 | 7.29 | existing records rewritten/insert 5.62 |

**Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2588 [2576, 2594] | 13.00 | 2.00 |
| B btree-compressed | 4944 [4921, 5027] | 11.01 | 2.00 |
| C node-blocks | 8982 [8766, 9201] | 26.58 | 2.00 |
| D record-chains | 834 [833, 840] | 4.00 | 1.00 |

**MVCC visibility control (A-D must agree and match the generator's counts)**

| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |
|---|---|---|
| A btree-fixed | 12160037 | 12163086 |
| B btree-compressed | 12160037 | 12163086 |
| C node-blocks | 12160037 | 12163086 |
| D record-chains | 12160037 | 12163086 |
expected from generator: 12160037 / 12163086 (difference proves the stamps are checked)

## 1 M nodes / 10 M edges: zipf-bulk

`layout-bench zipf bulk n=1000000 m=10000000 alpha=0.8 scale=1`

**Build (sorted bulk load, created_ts = 0 (frozen))**

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 3.60 | 2774942 | 0.02 | 0.02 | includes sort | 1397 |
| B | 3.81 | 2624060 | 0.01 | 0.01 | includes sort | 2607 |
| C | 2.90 | 3451832 | 0.39 | 0.02 | includes sort | 4084 |
| D | 9.26 | 1079839 | 22.04 | 0.01 | existing records rewritten/edge 5.15, densified nodes 45809 | 4110 |

**CSR projection (E) built by one sequential pass, all pages resident**

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.24 | 41228031 | 123460 | a7c193c4f2af92bf |
| B btree-compressed | 0.34 | 29772579 | 22002 | a7c193c4f2af92bf |
| C node-blocks | 0.29 | 33907381 | 3945261 | a7c193c4f2af92bf |

**Paged CSR (F fixed, G compressed) written from the projection**

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.28 | 33702 |
| G csr-paged-compressed | 0.66 | 18369 |

**Space**

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 124015 | 101.6 | 34417 | 129.8 | leaves 123457, height 4, entries 20000000, leaf fill 0.89; edge table 34417 pages |
| B btree-compressed | 22100 | 18.1 | 34417 | 46.3 | leaves 22000, height 3, entries 20000000, leaf bytes used 16.2 B/edge (8.1 B/entry), leaf fill 0.90; edge table 34417 pages |
| C node-blocks | 120104 | 98.4 | 34418 | 126.6 | dir 3922, block slabs 88730, chain 27452; entry bytes 72.0 B/edge, block bytes reserved 70.2 B/edge; relocations 0, entries copied 0; edge table 34417 pages |
| D record-chains | 67495 | 55.3 | 16205 | 68.6 | node recs 2942, rel recs 64103, groups 450; node map 1475 pages, edge map 14729 pages; densified 45809 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 18369 | 15.0 | 0 | 15.0 | offset pages 1954, entry pages 16415, entry stream 13.4 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |
RSS after build: 4605 MiB, peak 4668 MiB

**Warm: all pages resident**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 1.03 [1.02-1.05] | 1 [0.974-1.02] | 18.2 [18-18.2]; 4.6 ns/e | 164 [164-168] | 6,350 [6,336-6,359]; 10.4 ns/e | 30,272 [30,236-30,366]; 14.2 ns/e | 771,982 [769,277-778,552]; 78.0 ns/e |
| B btree-compressed | 1.31 [1.29-1.31] | 1.23 [1.23-1.27] | 33.8 [33.7-33.9]; 8.5 ns/e | 278 [277-279] | 10,678 [10,658-10,688]; 17.4 ns/e | 48,705 [48,547-48,829]; 22.8 ns/e | 1,047,248 [1,046,575-1,049,760]; 105.8 ns/e |
| C node-blocks | 0.399 [0.382-0.4] | 0.37 [0.348-0.375] | 7.63 [7.58-7.67]; 1.9 ns/e | 121 [120-123] | 3,575 [3,569-3,584]; 5.8 ns/e | 14,474 [14,380-14,624]; 6.8 ns/e | 211,896 [211,534-212,629]; 21.4 ns/e |
| D record-chains | 2.41 [2.31-2.42] | 2.06 [1.95-2.06] | 126 [126-126]; 31.5 ns/e | 333 [333-339] | 13,361 [13,236-13,476]; 21.8 ns/e | 65,630 [65,390-67,335]; 30.8 ns/e | 1,152,143 [1,150,332-1,161,707]; 116.4 ns/e |
| F csr-paged | 0.326 [0.318-0.327] | 0.301 [0.295-0.302] | 7.18 [7.17-7.24]; 1.8 ns/e | 131 [130-132] | 3,586 [3,584-3,594]; 5.9 ns/e | 13,521 [13,520-13,553]; 6.3 ns/e | 170,807 [168,459-171,182]; 17.3 ns/e |
| G csr-paged-compressed | 0.397 [0.389-0.398] | 0.396 [0.39-0.402] | 26.3 [26.3-26.3]; 6.6 ns/e | 215 [215-216] | 6,211 [6,211-6,252]; 10.1 ns/e | 23,512 [23,472-23,550]; 11.0 ns/e | 229,025 [227,743-234,986]; 23.1 ns/e |
| E csr-memory | 0.265 [0.264-0.266] | 0.25 [0.247-0.252] | 5.64 [5.62-5.68]; 1.4 ns/e | 92.4 [92.2-94.4] | 2,364 [2,360-2,382]; 3.9 ns/e | 8,711 [8,706-8,769]; 4.1 ns/e | 96,024 [94,081-97,317]; 9.7 ns/e |
| A btree-fixed(rep) | 0.953 [0.915-0.959] | 0.905 [0.864-1.01] | 18.1 [18.1-18.2]; 4.5 ns/e | 164 [164-166] | 6,337 [6,333-6,347]; 10.3 ns/e | 30,220 [30,184-30,264]; 14.2 ns/e | 774,856 [768,144-779,381]; 78.3 ns/e |

**Constrained 25%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 97.9 [96.5-99.1]; 7.8 KiB | 90.7 [89.9-93.1]; 7.3 KiB | 308 [244-309]; 32.3 KiB; 79.9 ns/e | 2,465 [2,437-3,064]; 214.1 KiB | 477,076 [288,298-640,013]; 41293.0 KiB; 708.1 ns/e (62 ops) |
| B btree-compressed | 36.5 [36.1-70.4]; 3.8 KiB | 34.6 [34.4-64.6]; 3.6 KiB | 71.2 [65-136]; 4.7 KiB; 18.8 ns/e | 964 [750-1,246]; 46.8 KiB | 90,366 [78,926-182,744]; 8822.3 KiB; 181.1 ns/e |
| C node-blocks | 87.2 [78.8-87.7]; 7.0 KiB | 73.3 [68.6-81.8]; 6.2 KiB | 232 [187-233]; 24.0 KiB; 60.5 ns/e | 1,982 [1,935-2,428]; 164.6 KiB | 368,384 [222,003-493,626]; 32280.1 KiB; 546.8 ns/e (62 ops) |
| D record-chains | 599 [518-615]; 48.2 KiB | 503 [498-511]; 42.2 KiB | 1,015 [996-1,023]; 70.0 KiB; 258.7 ns/e | 8,110 [7,826-9,779]; 652.8 KiB | not run |
| F csr-paged | 19.1 [17.7-58]; 2.7 KiB | 17.9 [17.6-50.6]; 2.4 KiB | 32.5 [32.3-117]; 4.9 KiB; 9.3 ns/e | 535 [436-975]; 35.6 KiB | 47,609 [45,050-90,277]; 4937.6 KiB; 96.5 ns/e |
| G csr-paged-compressed | 0.554 [0.542-39.2]; 1.2 KiB | 0.556 [0.508-28.9]; 0.9 KiB | 29.1 [23.6-79]; 1.6 KiB; 6.8 ns/e | 314 [263-612]; 9.4 KiB | 7,847 [4,918-9,669]; 146.3 KiB; 10.3 ns/e |

**Constrained 10%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 102 [100-105]; 8.2 KiB | 95.9 [94.6-96.8]; 7.7 KiB | 367 [366-367]; 45.9 KiB; 95.2 ns/e | 4,257 [4,204-5,009]; 486.3 KiB | 542,385 [338,521-725,622]; 57196.5 KiB; 805.0 ns/e (48 ops) |
| B btree-compressed | 74.5 [71.1-77.6]; 6.0 KiB | 72.4 [69.3-74.3]; 5.8 KiB | 144 [138-157]; 8.6 KiB; 39.8 ns/e | 1,517 [1,496-1,904]; 91.6 KiB | 168,049 [159,792-260,207]; 18798.3 KiB; 342.3 ns/e |
| C node-blocks | 123 [109-142]; 10.4 KiB | 112 [112-126]; 9.8 KiB | 317 [313-318]; 35.1 KiB; 81.4 ns/e | 3,581 [3,432-4,224]; 337.6 KiB | 541,866 [338,530-645,474]; 51139.7 KiB; 804.2 ns/e (48 ops) |
| D record-chains | 872 [796-884]; 71.2 KiB | 719 [699-725]; 59.9 KiB | 1,943 [1,915-1,983]; 147.9 KiB; 504.5 ns/e | 14,880 [13,464-18,135]; 1231.7 KiB | not run |
| F csr-paged | 68.4 [58.9-72.2]; 5.6 KiB | 65.2 [59.1-68]; 5.4 KiB | 135 [132-144]; 11.5 KiB; 37.3 ns/e | 1,455 [1,417-1,763]; 105.1 KiB | 120,947 [105,617-211,947]; 19203.4 KiB; 259.1 ns/e |
| G csr-paged-compressed | 39.7 [36.1-40.1]; 3.5 KiB | 34.4 [30.1-38.6]; 3.1 KiB | 87.1 [79.9-95.8]; 5.2 KiB; 23.1 ns/e | 925 [923-1,117]; 55.5 KiB | 84,644 [63,545-123,505]; 10141.8 KiB; 162.5 ns/e |

**Constrained 1%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 160 [159-160]; 12.8 KiB | 153 [151-154]; 12.4 KiB | 984 [914-1,206]; 202.9 KiB; 263.7 ns/e | 9,598 [9,514-10,967]; 1839.1 KiB | 599,833 [377,806-780,422]; 63728.5 KiB; 890.3 ns/e (48 ops) |
| B btree-compressed | 98.1 [97.8-98.5]; 7.9 KiB | 95.9 [95.4-97.5]; 7.8 KiB | 255 [247-262]; 20.6 KiB; 66.1 ns/e | 2,995 [2,856-3,595]; 253.0 KiB | 203,327 [187,259-302,480]; 22772.7 KiB; 401.1 ns/e |
| C node-blocks | 179 [178-181]; 15.1 KiB | 178 [176-179]; 15.0 KiB | 743 [698-817]; 145.5 KiB; 192.9 ns/e | 7,540 [7,351-10,029]; 1352.8 KiB | 597,701 [367,530-682,873]; 56668.8 KiB; 876.1 ns/e (48 ops) |
| D record-chains | 1,521 [1,460-1,533]; 125.9 KiB | 933 [908-933]; 77.5 KiB | 103,854 [63,299-117,970]; 7514.0 KiB; 24441.7 ns/e (192 ops) | 27,781 [27,695-47,190]; 2414.5 KiB | not run |
| F csr-paged | 160 [158-161]; 13.5 KiB | 157 [157-161]; 13.4 KiB | 398 [372-470]; 46.9 KiB; 104.1 ns/e | 4,442 [4,314-7,998]; 523.8 KiB | 206,439 [184,122-313,694]; 27991.3 KiB; 412.8 ns/e |
| G csr-paged-compressed | 150 [150-150]; 13.3 KiB | 148 [146-150]; 13.2 KiB | 274 [267-319]; 21.5 KiB; 71.1 ns/e | 3,247 [3,166-3,662]; 279.2 KiB | 180,164 [145,952-244,846]; 21201.4 KiB; 322.2 ns/e |

**Constrained 25%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 5.5 [5.31-5.89]; 0.91 miss (6.0% seq) | 5.21 [5.13-5.49]; 0.86 miss (2.0% seq) | 40 [38.9-42.6]; 4.74 miss (81.9% seq); 10.4 ns/e | 339 [317-379]; 26.41 miss (57.6% seq) | 20,672 [20,278-42,763]; 4743.73 miss (49.6% seq); 43.4 ns/e | 135,101 [99,925-225,142]; 26991.30 miss (39.9% seq); 86.3 ns/e | 4,872,917 [4,868,447-4,926,606]; 862920.00 miss (8.3% seq); 492.5 ns/e |
| B btree-compressed | 2.48 [2.41-4.48]; 0.37 miss (1.1% seq) | 2.39 [2.31-4.1]; 0.35 miss (0.3% seq) | 38.1 [33.7-42.6]; 0.95 miss (53.4% seq); 9.7 ns/e | 378 [367-443]; 6.82 miss (24.3% seq) | 10,956 [9,950-20,671]; 1098.93 miss (24.2% seq); 22.0 ns/e | 59,955 [47,428-96,574]; 6726.43 miss (17.8% seq); 38.3 ns/e | 2,054,519 [2,046,796-2,068,600]; 210070.67 miss (3.5% seq); 207.6 ns/e |
| C node-blocks | 2.28 [2.25-4.78]; 0.55 miss (1.4% seq) | 2.35 [2.21-4.11]; 0.50 miss (1.1% seq) | 24.5 [23-27.7]; 3.44 miss (75.8% seq); 6.1 ns/e | 254 [235-293]; 20.11 miss (48.1% seq) | 15,434 [15,090-31,104]; 3702.54 miss (31.1% seq); 32.3 ns/e | 95,483 [71,789-160,746]; 20649.97 miss (24.8% seq); 61.0 ns/e | 2,394,113 [2,385,871-2,402,069]; 444029.67 miss (4.1% seq); 242.0 ns/e |
| D record-chains | 35.3 [28.2-36.2]; 6.51 miss (0.1% seq) | 27 [26.5-27.3]; 5.26 miss (0.0% seq) | 187 [168-203]; 8.73 miss (0.6% seq); 48.4 ns/e | 800 [798-965]; 81.62 miss (0.8% seq) | 68,822 [61,579-156,651]; 17401.40 miss (0.6% seq); 137.9 ns/e | not run | not run |
| F csr-paged | 0.456 [0.441-3.24]; 0.21 miss (1.3% seq) | 0.396 [0.394-2.88]; 0.18 miss (1.2% seq) | 13.1 [11.6-13.7]; 1.14 miss (73.7% seq); 3.4 ns/e | 194 [190-226]; 5.93 miss (42.1% seq) | 5,162 [4,692-9,075]; 628.28 miss (26.0% seq); 10.3 ns/e | 23,106 [21,128-35,969]; 3130.43 miss (24.5% seq); 14.6 ns/e | 247,048 [245,943-252,482]; 13789.00 miss (14.7% seq); 25.0 ns/e |
| G csr-paged-compressed | 0.491 [0.461-2.52]; 0.15 miss (0.7% seq) | 0.459 [0.445-1.99]; 0.11 miss (0.2% seq) | 28.6 [23.4-29.8]; 0.20 miss (47.7% seq); 6.7 ns/e | 283 [268-321]; 1.18 miss (19.5% seq) | 5,058 [4,912-7,936]; 18.28 miss (10.8% seq); 10.4 ns/e | 17,465 [16,401-27,461]; 0.03 miss (0.0% seq); 11.2 ns/e | 237,000 [234,809-237,464]; 0.00 miss (-% seq); 24.0 ns/e |

**Constrained 10%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 5.92 [5.88-5.97]; 0.99 miss (6.0% seq) | 5.7 [5.66-5.79]; 0.95 miss (2.0% seq) | 44.3 [42.5-46.4]; 5.74 miss (82.9% seq); 11.5 ns/e | 493 [478-546]; 60.72 miss (74.2% seq) | 25,527 [23,738-50,625]; 5908.66 miss (56.8% seq); 51.2 ns/e | 143,752 [112,259-237,364]; 29390.70 miss (41.5% seq); 91.9 ns/e | 5,307,718 [5,289,372-5,315,132]; 965239.67 miss (8.4% seq); 536.4 ns/e |
| B btree-compressed | 4.41 [4.39-4.44]; 0.70 miss (0.9% seq) | 4.26 [4.26-4.28]; 0.69 miss (0.3% seq) | 38.3 [35.4-43.9]; 1.28 miss (43.2% seq); 9.9 ns/e | 383 [381-466]; 11.50 miss (18.7% seq) | 14,642 [13,420-27,842]; 2349.78 miss (41.3% seq); 29.3 ns/e | 92,205 [67,466-146,980]; 15543.43 miss (32.7% seq); 58.9 ns/e | 3,854,322 [3,851,787-3,882,101]; 665470.33 miss (7.9% seq); 389.5 ns/e |
| C node-blocks | 5.54 [5.52-5.87]; 1.09 miss (0.7% seq) | 5.85 [5.84-5.98]; 1.14 miss (0.5% seq) | 27.8 [27.8-29.8]; 4.39 miss (64.1% seq); 7.2 ns/e | 343 [328-395]; 42.15 miss (48.6% seq) | 21,399 [20,619-41,344]; 5367.51 miss (33.3% seq); 44.2 ns/e | 135,510 [101,884-215,453]; 29957.03 miss (19.3% seq); 86.6 ns/e | 5,327,921 [5,323,649-5,361,668]; 1085725.67 miss (1.9% seq); 538.5 ns/e |
| D record-chains | 44.8 [40.7-45.3]; 8.93 miss (0.1% seq) | 36.5 [35.8-37.3]; 7.49 miss (0.0% seq) | 221 [209-244]; 18.48 miss (0.3% seq); 57.4 ns/e | 1,110 [1,046-1,364]; 153.96 miss (0.5% seq) | 81,458 [70,377-180,319]; 20481.98 miss (0.6% seq); 163.3 ns/e | not run | not run |
| F csr-paged | 3.3 [3.29-3.32]; 0.61 miss (1.1% seq) | 3.21 [3.18-3.29]; 0.60 miss (0.9% seq) | 14.7 [14.3-16.3]; 1.67 miss (56.5% seq); 3.8 ns/e | 220 [219-266]; 13.05 miss (26.9% seq) | 11,152 [10,470-21,073]; 2399.18 miss (50.4% seq); 22.4 ns/e | 73,270 [53,567-117,041]; 15016.13 miss (41.9% seq); 46.8 ns/e | 2,940,845 [2,930,786-2,972,798]; 587066.67 miss (9.8% seq); 297.2 ns/e |
| G csr-paged-compressed | 2.03 [2.02-2.51]; 0.36 miss (0.5% seq) | 2.02 [1.98-2.04]; 0.33 miss (0.2% seq) | 29.4 [27.1-34.1]; 0.80 miss (41.5% seq); 7.6 ns/e | 306 [302-363]; 7.04 miss (14.9% seq) | 9,428 [8,778-16,611]; 1269.67 miss (45.6% seq); 18.9 ns/e | 53,148 [39,351-81,994]; 8196.87 miss (39.8% seq); 33.4 ns/e | 1,753,675 [1,745,384-1,770,061]; 315262.00 miss (9.2% seq); 177.2 ns/e |

**Constrained 1%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 8.23 [8.03-8.35]; 1.60 miss (4.0% seq) | 7.76 [7.69-7.77]; 1.55 miss (1.4% seq) | 124 [114-143]; 25.37 miss (92.4% seq); 32.2 ns/e | 1,195 [1,164-1,391]; 229.88 miss (89.1% seq) | 29,579 [25,976-52,543]; 6723.71 miss (57.0% seq); 59.3 ns/e | 151,596 [120,466-243,165]; 32909.33 miss (39.3% seq); 96.9 ns/e | 6,909,984 [6,899,964-6,958,770]; 1428002.00 miss (6.0% seq); 698.4 ns/e |
| B btree-compressed | 5.26 [5.19-5.48]; 0.99 miss (1.1% seq) | 5.07 [5.06-5.16]; 0.98 miss (0.4% seq) | 43.8 [40.8-48.8]; 2.56 miss (57.4% seq); 11.4 ns/e | 473 [457-592]; 31.63 miss (55.0% seq) | 16,019 [14,852-29,708]; 2846.59 miss (44.2% seq); 32.1 ns/e | 98,547 [74,297-157,245]; 18123.90 miss (33.5% seq); 63.0 ns/e | 4,604,394 [4,594,799-4,648,017]; 917023.33 miss (7.6% seq); 465.3 ns/e |
| C node-blocks | 8.92 [8.63-8.94]; 1.88 miss (0.4% seq) | 8.59 [8.49-8.71]; 1.87 miss (0.3% seq) | 79.4 [72.3-91.6]; 18.19 miss (89.1% seq); 20.6 ns/e | 872 [827-974]; 169.10 miss (84.8% seq) | 23,466 [22,317-42,591]; 6030.80 miss (37.0% seq); 47.8 ns/e | 142,075 [108,837-225,845]; 33236.07 miss (19.5% seq); 90.8 ns/e | 7,480,515 [7,478,655-7,556,849]; 1687006.33 miss (1.3% seq); 756.0 ns/e |
| D record-chains | 70.8 [68.4-70.9]; 15.74 miss (0.0% seq) | 44.5 [43.1-44.5]; 9.69 miss (0.0% seq) | 4,222 [3,746-4,801]; 1008.05 miss (0.0% seq); 1056.8 ns/e (5600 ops) | 1,689 [1,680-1,981]; 301.83 miss (0.3% seq) | 86,459 [72,117-187,725]; 22699.22 miss (0.6% seq); 173.3 ns/e | not run | not run |
| F csr-paged | 7.77 [7.67-7.82]; 1.68 miss (0.8% seq) | 7.52 [7.36-7.59]; 1.68 miss (0.9% seq) | 32.5 [29.2-32.9]; 5.86 miss (67.9% seq); 8.4 ns/e | 433 [428-517]; 65.48 miss (63.0% seq) | 15,046 [14,225-26,067]; 3498.91 miss (45.3% seq); 30.5 ns/e | 96,673 [74,287-147,506]; 21821.00 miss (33.5% seq); 60.1 ns/e | 6,268,851 [6,226,513-6,298,732]; 1439147.33 miss (5.3% seq); 633.6 ns/e |
| G csr-paged-compressed | 7.71 [7.41-7.72]; 1.66 miss (0.5% seq) | 7.34 [7.31-7.41]; 1.65 miss (0.4% seq) | 37.2 [34.7-40.9]; 2.69 miss (34.6% seq); 9.7 ns/e | 420 [406-497]; 34.90 miss (34.0% seq) | 14,307 [13,956-23,401]; 2650.17 miss (40.1% seq); 29.9 ns/e | 89,900 [70,891-133,027]; 18103.50 miss (34.3% seq); 54.2 ns/e | 6,089,095 [6,082,575-6,167,905]; 1392865.67 miss (5.8% seq); 615.4 ns/e |

**Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2288 [2257, 2315] | 11.02 | 3.01 |  |
| B btree-compressed | 3843 [3839, 3859] | 9.02 | 3.01 |  |
| C node-blocks | 661 [653, 709] | 9.89 | 5.01 |  |
| D record-chains | 1515 [1462, 1543] | 26.66 | 7.16 | existing records rewritten/insert 5.62 |

**Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2429 [2385, 2433] | 13.00 | 2.00 |
| B btree-compressed | 4558 [4506, 4583] | 9.00 | 2.00 |
| C node-blocks | 5918 [5750, 5976] | 26.57 | 2.00 |
| D record-chains | 821 [821, 826] | 4.00 | 1.00 |

**MVCC visibility control (A-D must agree and match the generator's counts)**

| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |
|---|---|---|
| A btree-fixed | 12160037 | 12163086 |
| B btree-compressed | 12160037 | 12163086 |
| C node-blocks | 12160037 | 12163086 |
| D record-chains | 12160037 | 12163086 |
expected from generator: 12160037 / 12163086 (difference proves the stamps are checked)

## 1 M nodes / 10 M edges: uniform-online

`layout-bench uniform online n=1000000 m=10000000 alpha=0 scale=1`

**Build (online inserts in random order, created_ts = edge id + 1)**

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 20.05 | 498691 | 10.64 | 3.04 |  | 1623 |
| B | 38.55 | 259392 | 9.25 | 3.01 |  | 2210 |
| C | 6.80 | 1471542 | 9.73 | 4.95 | relocations/edge 0.732, entries copied/edge 2.53 | 3805 |
| D | 12.59 | 794432 | 19.76 | 5.90 | existing records rewritten/edge 3.90, densified nodes 0 | 4459 |

**CSR projection (E) built by one sequential pass, all pages resident**

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.31 | 31989382 | 158139 | 4b99dd115ab53ad0 |
| B btree-compressed | 0.37 | 26858745 | 40028 | 4b99dd115ab53ad0 |
| C node-blocks | 0.70 | 14344355 | 3999912 | 4b99dd115ab53ad0 |

**Paged CSR (F fixed, G compressed) written from the projection**

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.31 | 33702 |
| G csr-paged-compressed | 0.70 | 19362 |

**Space**

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 159169 | 130.4 | 34466 | 158.6 | leaves 158136, height 4, entries 20000000, leaf fill 0.70; edge table 34466 pages |
| B btree-compressed | 40284 | 33.0 | 34466 | 61.2 | leaves 40025, height 4, entries 20000000, leaf bytes used 24.5 B/edge (12.3 B/entry), leaf fill 0.75; edge table 34466 pages |
| C node-blocks | 166180 | 136.1 | 34466 | 164.4 | dir 3922, block slabs 162258, chain 0; entry bytes 72.0 B/edge, block bytes reserved 98.3 B/edge; relocations 7323162, entries copied 25291862; edge table 34466 pages |
| D record-chains | 67045 | 54.9 | 16225 | 68.2 | node recs 2942, rel recs 64103, groups 0; node map 1475 pages, edge map 14750 pages; densified 0 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 19362 | 15.9 | 0 | 15.9 | offset pages 1954, entry pages 17408, entry stream 14.3 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |
RSS after build: 5154 MiB, peak 5221 MiB

**Warm: all pages resident**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 1.2 [1.11-1.2] | 1.11 [1.09-1.11] | 1.2 [1.19-1.22]; 109.2 ns/e | 13 [13-13] | 13.6 [13.3-14.1]; 114.8 ns/e | 135 [133-140]; 123.8 ns/e | 995,420 [994,711-997,819]; 99.5 ns/e |
| B btree-compressed | 1.48 [1.47-1.48] | 1.36 [1.36-1.37] | 1.48 [1.48-1.49]; 134.2 ns/e | 16.5 [16.4-18.6] | 19.3 [19.1-20.2]; 163.4 ns/e | 170 [169-181]; 155.8 ns/e | 1,338,295 [1,335,073-1,341,647]; 133.8 ns/e |
| C node-blocks | 0.501 [0.497-0.504] | 0.435 [0.433-0.439] | 0.506 [0.504-0.512]; 45.9 ns/e | 5.5 [5.15-6.2] | 6.52 [6.21-6.84]; 55.3 ns/e | 59 [54.9-60.3]; 54.0 ns/e | 332,610 [332,132-334,194]; 33.3 ns/e |
| D record-chains | 4.46 [4.4-4.49] | 4.06 [4.05-4.1] | 4.63 [4.61-4.71]; 420.2 ns/e | 37.7 [37.7-37.9] | 48 [48-48.1]; 406.9 ns/e | 382 [380-396]; 349.6 ns/e | 3,243,992 [3,243,897-3,249,646]; 324.4 ns/e |
| F csr-paged | 0.371 [0.361-0.373] | 0.351 [0.344-0.361] | 0.376 [0.372-0.38]; 34.1 ns/e | 3.86 [3.78-4.01] | 4.69 [4.67-4.84]; 39.8 ns/e | 40.9 [40.9-42.2]; 37.4 ns/e | 241,164 [240,899-241,243]; 24.1 ns/e |
| G csr-paged-compressed | 0.422 [0.415-0.424] | 0.419 [0.413-0.428] | 0.43 [0.428-0.431]; 39.0 ns/e | 4.09 [4.02-4.15] | 4.89 [4.88-5.24]; 41.4 ns/e | 43.6 [43.5-44.9]; 39.9 ns/e | 270,804 [270,384-274,081]; 27.1 ns/e |
| E csr-memory | 0.329 [0.317-0.336] | 0.264 [0.259-0.267] | 0.337 [0.336-0.338]; 30.5 ns/e | 2.85 [2.85-2.86] | 3.31 [3.31-3.46]; 28.0 ns/e | 30.2 [30-30.3]; 27.6 ns/e | 123,020 [122,273-123,677]; 12.3 ns/e |
| A btree-fixed(rep) | 1.11 [1.03-1.17] | 1.04 [0.943-1.08] | 1.13 [1.05-1.2]; 102.3 ns/e | 12.1 [11.2-12.5] | 12.3 [12.2-12.6]; 103.9 ns/e | 120 [118-123]; 109.8 ns/e | 995,460 [993,332-997,125]; 99.6 ns/e |

**Constrained 25%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 95.2 [93.2-103]; 8.0 KiB | 89.6 [88.2-93.7]; 7.4 KiB | 93.8 [93.7-98.9]; 7.8 KiB; 8565.9 ns/e | 994 [989-1,073]; 83.5 KiB | 1,143 [1,107-1,158]; 93.0 KiB; 9485.2 ns/e |
| B btree-compressed | 70.4 [56.8-84.8]; 5.8 KiB | 65.4 [53.2-79.6]; 5.4 KiB | 63.7 [52.4-75.8]; 5.2 KiB; 5819.8 ns/e | 665 [548-827]; 55.2 KiB | 752 [678-839]; 61.2 KiB; 6149.1 ns/e |
| C node-blocks | 88.7 [87.2-113]; 8.0 KiB | 88.1 [85.9-102]; 7.7 KiB | 86.7 [86.5-96.9]; 7.5 KiB; 7927.9 ns/e | 926 [922-1,052]; 79.3 KiB | 1,053 [1,008-1,094]; 88.2 KiB; 8813.7 ns/e |
| D record-chains | 1,505 [1,497-1,525]; 128.9 KiB | 1,500 [1,493-1,512]; 128.2 KiB | 1,589 [1,586-1,592]; 135.4 KiB; 143962.9 ns/e | 15,467 [15,344-15,501]; 1318.6 KiB | not run |
| F csr-paged | 41.6 [24.9-66.8]; 3.7 KiB | 37.2 [23.3-58.4]; 3.3 KiB | 30.7 [18.5-51.9]; 2.8 KiB; 2805.6 ns/e | 324 [201-526]; 29.2 KiB | 349 [234-555]; 31.2 KiB; 2853.8 ns/e |
| G csr-paged-compressed | 18 [7.77-42.1]; 2.2 KiB | 14.6 [6.02-33.2]; 1.7 KiB | 10.6 [5.25-25.3]; 1.3 KiB; 964.8 ns/e | 111 [51.6-242]; 12.9 KiB | 116 [58.5-236]; 13.2 KiB; 948.0 ns/e |

**Constrained 10%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 101 [101-103]; 8.3 KiB | 95.1 [94.8-96.1]; 7.8 KiB | 102 [101-103]; 8.4 KiB; 9248.5 ns/e | 1,103 [1,090-1,105]; 89.9 KiB | 1,214 [1,123-1,217]; 97.7 KiB; 9952.0 ns/e |
| B btree-compressed | 80.8 [79.9-84.2]; 6.7 KiB | 79.7 [79.5-80.5]; 6.6 KiB | 80.2 [80-83]; 6.7 KiB; 7245.1 ns/e | 886 [863-889]; 72.0 KiB | 988 [975-1,011]; 79.6 KiB; 8354.2 ns/e |
| C node-blocks | 117 [115-120]; 9.8 KiB | 116 [116-119]; 9.8 KiB | 118 [115-119]; 9.8 KiB; 10677.0 ns/e | 1,262 [1,255-1,292]; 106.3 KiB | 1,444 [1,360-1,448]; 117.8 KiB; 11897.4 ns/e |
| D record-chains | 2,173 [2,154-2,186]; 184.9 KiB | 1,995 [1,986-1,999]; 169.7 KiB | 2,277 [2,276-2,283]; 193.5 KiB; 206344.5 ns/e | 19,026 [18,864-19,121]; 1620.4 KiB | not run |
| F csr-paged | 58 [56-66.7]; 5.0 KiB | 58.2 [57.5-59]; 4.8 KiB | 57.1 [56.8-57.7]; 4.7 KiB; 5189.0 ns/e | 627 [620-651]; 52.4 KiB | 690 [689-717]; 58.0 KiB; 5902.9 ns/e |
| G csr-paged-compressed | 21.3 [21-42.3]; 2.7 KiB | 21.4 [20.9-33]; 2.4 KiB | 21.6 [21.1-25.5]; 2.2 KiB; 1971.3 ns/e | 233 [230-243]; 23.0 KiB | 255 [247-272]; 24.8 KiB; 2113.2 ns/e |

**Constrained 1%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 171 [170-171]; 14.1 KiB | 164 [163-164]; 13.6 KiB | 171 [171-173]; 14.2 KiB; 15637.8 ns/e | 1,836 [1,814-1,885]; 151.7 KiB | 1,995 [1,990-2,060]; 166.0 KiB; 17049.8 ns/e |
| B btree-compressed | 103 [102-105]; 8.6 KiB | 101 [101-101]; 8.4 KiB | 103 [103-106]; 8.6 KiB; 9312.1 ns/e | 1,146 [1,102-1,169]; 92.9 KiB | 1,205 [1,195-1,311]; 101.2 KiB; 10539.5 ns/e |
| C node-blocks | 182 [181-182]; 15.2 KiB | 181 [181-182]; 15.2 KiB | 182 [181-182]; 15.2 KiB; 16467.0 ns/e | 1,961 [1,948-1,983]; 164.1 KiB | 2,099 [2,095-2,182]; 178.6 KiB; 17953.0 ns/e |
| D record-chains | 2,917 [2,909-2,928]; 248.1 KiB (5168 ops) | 2,290 [2,260-2,292]; 194.5 KiB | 3,083 [3,073-3,101]; 262.5 KiB; 280286.2 ns/e (4880 ops) | 21,210 [21,015-21,293]; 1802.7 KiB | not run |
| F csr-paged | 157 [156-158]; 13.2 KiB | 157 [155-158]; 13.1 KiB | 157 [156-159]; 13.2 KiB; 14347.1 ns/e | 1,709 [1,691-1,720]; 142.1 KiB | 1,891 [1,846-1,892]; 155.6 KiB; 16142.7 ns/e |
| G csr-paged-compressed | 132 [130-132]; 12.9 KiB | 132 [131-134]; 12.9 KiB | 132 [131-132]; 12.9 KiB; 11901.7 ns/e | 1,427 [1,420-1,432]; 139.4 KiB | 1,557 [1,556-1,565]; 151.9 KiB; 13333.4 ns/e |

**Constrained 25%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 5.95 [5.94-6.41]; 0.98 miss (0.0% seq) | 5.69 [5.67-5.99]; 0.93 miss (0.0% seq) | 6 [5.83-6.26]; 0.98 miss (0.0% seq); 542.8 ns/e | 67.7 [65.2-71.4]; 10.55 miss (0.0% seq) | 74.2 [72.8-74.6]; 11.47 miss (0.0% seq); 635.5 ns/e | 676 [626-724]; 108.40 miss (0.0% seq); 619.5 ns/e | 5,811,182 [5,802,763-5,833,761]; 976861.33 miss (0.0% seq); 581.1 ns/e |
| B btree-compressed | 4.15 [4.04-5.45]; 0.64 miss (0.0% seq) | 4.02 [4-5.16]; 0.63 miss (0.0% seq) | 4.24 [4.12-5.1]; 0.62 miss (0.0% seq); 387.2 ns/e | 48 [47-59.5]; 6.67 miss (0.0% seq) | 55.1 [51.7-63.3]; 7.28 miss (0.0% seq); 452.3 ns/e | 540 [429-550]; 66.17 miss (0.0% seq); 441.6 ns/e | 4,009,537 [3,998,160-4,057,401]; 546013.00 miss (0.0% seq); 401.0 ns/e |
| C node-blocks | 4.92 [4.9-6.16]; 0.99 miss (0.0% seq) | 4.88 [4.86-5.68]; 0.95 miss (0.0% seq) | 5 [4.91-5.47]; 0.94 miss (0.0% seq); 452.1 ns/e | 54.6 [52.1-57.7]; 9.81 miss (0.0% seq) | 61.5 [61.3-64.1]; 11.03 miss (0.0% seq); 535.8 ns/e | 560 [516-603]; 100.50 miss (0.0% seq); 510.5 ns/e | 4,787,256 [4,781,225-4,794,498]; 904840.00 miss (0.0% seq); 478.7 ns/e |
| D record-chains | 81.2 [81.1-83.1]; 16.21 miss (0.0% seq) | 80.1 [79.9-80.9]; 16.04 miss (0.0% seq) | 85.2 [84.4-85.3]; 16.94 miss (0.0% seq); 7707.2 ns/e | 824 [817-828]; 164.85 miss (0.0% seq) | 884 [884-943]; 179.20 miss (0.0% seq); 7704.6 ns/e | not run | not run |
| F csr-paged | 0.462 [0.458-3.68]; 0.23 miss (1.2% seq) | 0.438 [0.43-3.3]; 0.20 miss (0.6% seq) | 0.478 [0.46-3]; 0.18 miss (0.7% seq); 43.2 ns/e | 6 [5.94-32.1]; 1.82 miss (0.4% seq) | 7.35 [7.25-33.9]; 1.92 miss (0.6% seq); 64.3 ns/e | 56.7 [55.1-283]; 15.97 miss (0.6% seq); 54.5 ns/e | 247,045 [242,998-283,232]; 2227.00 miss (0.4% seq); 24.7 ns/e |
| G csr-paged-compressed | 0.49 [0.489-2.95]; 0.17 miss (0.6% seq) | 0.486 [0.482-2.42]; 0.13 miss (0.1% seq) | 0.503 [0.499-1.98]; 0.10 miss (0.3% seq); 45.6 ns/e | 6.42 [4.78-20.9]; 0.99 miss (0.3% seq) | 6.78 [6.31-21.9]; 0.97 miss (0.0% seq); 59.3 ns/e | 55.3 [52.3-181]; 8.03 miss (0.0% seq); 54.7 ns/e | 286,678 [278,088-290,131]; 617.33 miss (0.1% seq); 28.7 ns/e |

**Constrained 10%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 6.33 [6.28-6.45]; 1.04 miss (0.0% seq) | 6.01 [5.98-6.06]; 0.98 miss (0.0% seq) | 6.42 [6.36-6.48]; 1.05 miss (0.0% seq); 585.8 ns/e | 72.1 [71-72.6]; 11.24 miss (0.0% seq) | 79.5 [79.3-80.6]; 12.21 miss (0.0% seq); 681.0 ns/e | 693 [673-817]; 115.10 miss (0.0% seq); 666.2 ns/e | 6,108,747 [6,094,789-6,120,601]; 1046700.33 miss (0.0% seq); 610.9 ns/e |
| B btree-compressed | 5.37 [5.3-5.46]; 0.84 miss (0.0% seq) | 5.18 [5.17-5.21]; 0.82 miss (0.0% seq) | 5.3 [5.29-5.45]; 0.83 miss (0.0% seq); 478.7 ns/e | 62.7 [61.2-64.2]; 9.05 miss (0.0% seq) | 69.5 [67.1-72.5]; 9.91 miss (0.0% seq); 593.0 ns/e | 598 [572-712]; 91.97 miss (0.0% seq); 565.4 ns/e | 5,264,375 [5,248,384-5,269,423]; 835027.00 miss (0.0% seq); 526.5 ns/e |
| C node-blocks | 6.52 [6.41-6.58]; 1.23 miss (0.0% seq) | 6.4 [6.35-6.53]; 1.22 miss (0.0% seq) | 6.46 [6.43-6.57]; 1.23 miss (0.0% seq); 587.8 ns/e | 71.6 [70.9-72.6]; 13.28 miss (0.0% seq) | 81.4 [77.5-83.2]; 14.70 miss (0.0% seq); 679.9 ns/e | 684 [678-835]; 133.97 miss (0.0% seq); 670.4 ns/e | 6,303,778 [6,296,576-6,314,967]; 1230546.67 miss (0.0% seq); 630.4 ns/e |
| D record-chains | 113 [113-116]; 23.12 miss (0.0% seq) | 103 [103-104]; 21.22 miss (0.0% seq) | 118 [118-119]; 24.19 miss (0.0% seq); 10735.7 ns/e | 1e+03 [992-1,003]; 202.55 miss (0.0% seq) | 1,079 [1,056-1,169]; 220.73 miss (0.0% seq); 9242.6 ns/e | not run | not run |
| F csr-paged | 3.3 [3.22-3.7]; 0.63 miss (1.0% seq) | 3.33 [3.29-3.35]; 0.61 miss (0.8% seq) | 3.32 [3.28-3.35]; 0.60 miss (1.1% seq); 302.6 ns/e | 38.1 [37.9-38.6]; 6.61 miss (0.8% seq) | 41.6 [41-42]; 7.18 miss (1.2% seq); 351.0 ns/e | 366 [348-430]; 67.17 miss (0.9% seq); 344.2 ns/e | 3,161,093 [3,153,513-3,163,685]; 603529.00 miss (0.9% seq); 316.1 ns/e |
| G csr-paged-compressed | 1.81 [1.69-2.89]; 0.34 miss (0.4% seq) | 1.77 [1.69-2.37]; 0.31 miss (0.3% seq) | 1.75 [1.67-1.96]; 0.27 miss (0.2% seq); 158.4 ns/e | 21.2 [21.2-21.7]; 2.95 miss (0.1% seq) | 22.7 [22.7-22.9]; 3.00 miss (0.0% seq); 194.9 ns/e | 197 [185-242]; 29.03 miss (0.0% seq); 185.5 ns/e | 1,579,439 [1,575,146-1,580,870]; 259228.00 miss (0.2% seq); 158.0 ns/e |

**Constrained 1%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 9.47 [9.45-9.56]; 1.76 miss (0.0% seq) | 9.11 [8.84-9.19]; 1.70 miss (0.0% seq) | 9.56 [9.48-9.64]; 1.78 miss (0.0% seq); 871.2 ns/e | 106 [105-108]; 18.96 miss (0.0% seq) | 119 [114-120]; 20.74 miss (0.0% seq); 983.0 ns/e | 1,008 [989-1,157]; 192.97 miss (0.0% seq); 947.5 ns/e | 8,945,066 [8,942,407-8,952,573]; 1767677.00 miss (0.0% seq); 894.5 ns/e |
| B btree-compressed | 6.3 [6.21-6.34]; 1.07 miss (0.0% seq) | 6.02 [6.01-6.09]; 1.05 miss (0.0% seq) | 6.06 [5.95-6.24]; 1.07 miss (0.0% seq); 553.8 ns/e | 72.9 [71.6-74.5]; 11.63 miss (0.0% seq) | 76.8 [76.5-84]; 12.64 miss (0.0% seq); 668.7 ns/e | 679 [658-787]; 117.10 miss (0.0% seq); 643.4 ns/e | 5,985,453 [5,939,500-5,991,311]; 1067981.00 miss (0.0% seq); 598.6 ns/e |
| C node-blocks | 9.42 [9.23-9.52]; 1.90 miss (0.0% seq) | 9.43 [8.99-9.46]; 1.90 miss (0.0% seq) | 9.25 [9.09-9.36]; 1.90 miss (0.0% seq); 845.4 ns/e | 104 [103-105]; 20.51 miss (0.0% seq) | 117 [112-119]; 22.32 miss (0.0% seq); 972.4 ns/e | 999 [965-1,146]; 208.73 miss (0.0% seq); 938.4 ns/e | 8,888,477 [8,877,604-8,894,607]; 1904887.00 miss (0.0% seq); 888.9 ns/e |
| D record-chains | 140 [140-143]; 31.02 miss (0.0% seq) | 112 [108-113]; 24.31 miss (0.0% seq) | 150 [149-150]; 32.84 miss (0.0% seq); 13546.2 ns/e | 1,054 [1,041-1,059]; 225.33 miss (0.0% seq) | 1,149 [1,131-1,188]; 245.94 miss (0.0% seq); 9710.9 ns/e | not run | not run |
| F csr-paged | 7.78 [7.69-7.79]; 1.65 miss (0.9% seq) | 7.77 [7.47-7.78]; 1.64 miss (0.9% seq) | 7.76 [7.72-7.88]; 1.65 miss (0.9% seq); 709.2 ns/e | 87.5 [86.7-88.3]; 17.76 miss (0.9% seq) | 95.1 [95-96]; 19.46 miss (1.3% seq); 814.9 ns/e | 829 [804-956]; 179.73 miss (0.9% seq); 781.9 ns/e | 7,349,997 [7,349,005-7,354,153]; 1643443.00 miss (0.9% seq); 735.0 ns/e |
| G csr-paged-compressed | 7.71 [7.58-7.73]; 1.62 miss (0.5% seq) | 7.64 [7.38-7.66]; 1.61 miss (0.6% seq) | 7.67 [7.6-7.71]; 1.62 miss (0.4% seq); 696.6 ns/e | 85.8 [84.5-87]; 17.43 miss (0.6% seq) | 95.7 [94.3-97.7]; 18.99 miss (0.4% seq); 808.3 ns/e | 833 [791-949]; 175.77 miss (0.5% seq); 782.3 ns/e | 7,352,175 [7,345,182-7,356,723]; 1613823.33 miss (0.5% seq); 735.3 ns/e |

**Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2813 [2737, 2857] | 11.10 | 3.04 |  |
| B btree-compressed | 4683 [4649, 4701] | 11.06 | 3.02 |  |
| C node-blocks | 870 [832, 901] | 9.33 | 5.00 |  |
| D record-chains | 1442 [1373, 1715] | 20.01 | 6.00 | existing records rewritten/insert 4.00 |

**Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2614 [2559, 2842] | 13.00 | 2.00 |
| B btree-compressed | 4965 [4935, 5061] | 11.01 | 2.01 |
| C node-blocks | 1381 [1374, 1384] | 9.00 | 1.99 |
| D record-chains | 815 [813, 826] | 4.00 | 1.00 |

**MVCC visibility control (A-D must agree and match the generator's counts)**

| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |
|---|---|---|
| A btree-fixed | 31967 | 30935 |
| B btree-compressed | 31967 | 30935 |
| C node-blocks | 31967 | 30935 |
| D record-chains | 31967 | 30935 |
expected from generator: 31967 / 30935 (difference proves the stamps are checked)

## 1 M nodes / 10 M edges: uniform-bulk

`layout-bench uniform bulk n=1000000 m=10000000 alpha=0 scale=1`

**Build (sorted bulk load, created_ts = 0 (frozen))**

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 3.65 | 2739636 | 0.02 | 0.02 | includes sort | 1389 |
| B | 3.70 | 2699165 | 0.01 | 0.01 | includes sort | 2597 |
| C | 3.46 | 2888870 | 0.40 | 0.02 | includes sort | 4171 |
| D | 8.96 | 1115677 | 16.90 | 0.01 | existing records rewritten/edge 3.90, densified nodes 0 | 4171 |

**CSR projection (E) built by one sequential pass, all pages resident**

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.26 | 38012095 | 123460 | 4b99dd115ab53ad0 |
| B btree-compressed | 0.34 | 29135431 | 23180 | 4b99dd115ab53ad0 |
| C node-blocks | 0.29 | 35036104 | 3999912 | 4b99dd115ab53ad0 |

**Paged CSR (F fixed, G compressed) written from the projection**

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.18 | 33702 |
| G csr-paged-compressed | 0.66 | 19362 |

**Space**

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 124015 | 101.6 | 34417 | 129.8 | leaves 123457, height 4, entries 20000000, leaf fill 0.89; edge table 34417 pages |
| B btree-compressed | 23283 | 19.1 | 34417 | 47.3 | leaves 23178, height 3, entries 20000000, leaf bytes used 17.1 B/edge (8.5 B/entry), leaf fill 0.90; edge table 34417 pages |
| C node-blocks | 125762 | 103.0 | 34418 | 131.2 | dir 3922, block slabs 121840, chain 0; entry bytes 72.0 B/edge, block bytes reserved 98.3 B/edge; relocations 0, entries copied 0; edge table 34417 pages |
| D record-chains | 67045 | 54.9 | 16205 | 68.2 | node recs 2942, rel recs 64103, groups 0; node map 1475 pages, edge map 14729 pages; densified 0 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 19362 | 15.9 | 0 | 15.9 | offset pages 1954, entry pages 17408, entry stream 14.3 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |
RSS after build: 4574 MiB, peak 4642 MiB

**Warm: all pages resident**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 1.08 [1.08-1.08] | 1.01 [1-1.03] | 1.1 [1.09-1.1]; 99.5 ns/e | 11.5 [11-11.6] | 12 [11.8-12.4]; 101.6 ns/e | 117 [113-118]; 106.7 ns/e | 918,359 [915,679-920,013]; 91.8 ns/e |
| B btree-compressed | 1.33 [1.32-1.34] | 1.23 [1.22-1.24] | 1.35 [1.34-1.36]; 122.3 ns/e | 14.7 [14.7-15.5] | 16.8 [16.8-18.1]; 142.7 ns/e | 152 [151-161]; 139.3 ns/e | 1,182,137 [1,176,633-1,182,211]; 118.2 ns/e |
| C node-blocks | 0.451 [0.447-0.452] | 0.395 [0.392-0.4] | 0.454 [0.454-0.462]; 41.2 ns/e | 4.65 [4.42-5.71] | 5.73 [5.56-6.25]; 48.6 ns/e | 52.8 [49.1-54.8]; 48.3 ns/e | 299,499 [285,321-301,940]; 30.0 ns/e |
| D record-chains | 3.2 [3.19-3.27] | 2.85 [2.85-2.88] | 3.29 [3.25-3.35]; 298.1 ns/e | 25.9 [24.6-26.9] | 32.6 [30-32.8]; 276.2 ns/e | 253 [250-263]; 231.5 ns/e | 2,006,412 [2,005,710-2,046,520]; 200.7 ns/e |
| F csr-paged | 0.365 [0.365-0.367] | 0.349 [0.346-0.349] | 0.371 [0.371-0.372]; 33.6 ns/e | 4.06 [3.74-4.1] | 4.71 [4.27-4.96]; 39.9 ns/e | 41.7 [41-42.5]; 38.1 ns/e | 241,107 [241,004-243,258]; 24.1 ns/e |
| G csr-paged-compressed | 0.415 [0.415-0.424] | 0.413 [0.409-0.419] | 0.421 [0.42-0.422]; 38.2 ns/e | 4.19 [3.75-4.2] | 4.88 [4.48-5.05]; 41.4 ns/e | 43.4 [39-45.8]; 39.7 ns/e | 271,412 [270,672-275,770]; 27.1 ns/e |
| E csr-memory | 0.327 [0.325-0.334] | 0.261 [0.26-0.262] | 0.334 [0.332-0.334]; 30.3 ns/e | 2.88 [2.73-2.89] | 3.35 [3.23-3.43]; 28.4 ns/e | 30.7 [29-31.5]; 28.1 ns/e | 123,661 [121,239-124,791]; 12.4 ns/e |
| A btree-fixed(rep) | 1 [0.966-1.08] | 0.933 [0.89-1] | 1.02 [0.985-1.12]; 92.2 ns/e | 10.6 [10.6-11] | 11.1 [8.02-11.2]; 94.2 ns/e | 108 [101-108]; 98.6 ns/e | 918,713 [917,110-919,817]; 91.9 ns/e |

**Constrained 25%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 94.1 [92.4-103]; 7.8 KiB | 87.6 [86.4-92.8]; 7.4 KiB | 92.4 [91.2-95.4]; 7.7 KiB; 8441.3 ns/e | 988 [988-1,017]; 82.8 KiB | 1,100 [1,091-1,106]; 91.8 KiB; 9421.7 ns/e |
| B btree-compressed | 55.2 [39-77.6]; 4.7 KiB | 48.4 [37.7-68.9]; 4.3 KiB | 48.5 [36.1-62.4]; 4.0 KiB; 4434.9 ns/e | 455 [400-644]; 41.4 KiB | 520 [491-670]; 46.7 KiB; 4292.5 ns/e |
| C node-blocks | 79.4 [75.5-107]; 7.5 KiB | 78.3 [77.2-94]; 7.1 KiB | 79.6 [78.9-87.1]; 7.0 KiB; 7213.7 ns/e | 883 [836-884]; 73.0 KiB | 897 [896-924]; 78.7 KiB; 7678.3 ns/e |
| D record-chains | 933 [926-934]; 79.2 KiB | 927 [908-944]; 78.9 KiB | 924 [917-946]; 79.9 KiB; 83573.3 ns/e | 9,269 [9,080-9,299]; 794.4 KiB | not run |
| F csr-paged | 40.4 [23.5-65.9]; 3.7 KiB | 35.7 [22.3-58.8]; 3.3 KiB | 29.4 [18.5-49.3]; 2.8 KiB; 2683.9 ns/e | 314 [202-499]; 29.2 KiB | 325 [218-525]; 31.2 KiB; 2658.0 ns/e |
| G csr-paged-compressed | 17.9 [7.21-41.9]; 2.2 KiB | 14.2 [6.41-34.6]; 1.7 KiB | 10.2 [5.14-25.2]; 1.3 KiB; 932.9 ns/e | 109 [53.2-243]; 12.9 KiB | 118 [56.3-231]; 13.2 KiB; 962.0 ns/e |

**Constrained 10%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 98.2 [97.7-98.6]; 8.2 KiB | 93 [91.9-94]; 7.8 KiB | 98.5 [97.7-102]; 8.2 KiB; 8902.3 ns/e | 1,092 [1,067-1,095]; 89.1 KiB | 1,152 [1,141-1,198]; 96.8 KiB; 9792.0 ns/e |
| B btree-compressed | 73.4 [71.6-73.8]; 6.2 KiB | 72.5 [70.4-73.2]; 6.1 KiB | 72.2 [72-76]; 6.1 KiB; 6523.9 ns/e | 785 [768-831]; 65.4 KiB | 863 [834-900]; 72.5 KiB; 7360.5 ns/e |
| C node-blocks | 123 [119-124]; 10.5 KiB | 123 [121-124]; 10.6 KiB | 122 [121-123]; 10.5 KiB; 10997.8 ns/e | 1,337 [1,329-1,360]; 115.3 KiB | 1,468 [1,411-1,490]; 126.2 KiB; 12343.9 ns/e |
| D record-chains | 1,408 [1,379-1,410]; 121.0 KiB | 1,235 [1,219-1,238]; 106.7 KiB | 1,400 [1,399-1,401]; 121.6 KiB; 126597.2 ns/e | 10,984 [10,950-11,000]; 947.0 KiB | not run |
| F csr-paged | 64 [63.9-65.9]; 5.6 KiB | 66.5 [65-69.4]; 5.7 KiB | 64.9 [64.5-65]; 5.6 KiB; 5869.6 ns/e | 710 [698-739]; 61.3 KiB | 791 [790-814]; 68.2 KiB; 6768.9 ns/e |
| G csr-paged-compressed | 36.7 [36.6-41.2]; 3.8 KiB | 36.1 [35.8-36.4]; 3.6 KiB | 35.4 [34.7-37.9]; 3.5 KiB; 3196.9 ns/e | 417 [405-418]; 40.4 KiB | 428 [391-434]; 42.1 KiB; 3495.5 ns/e |

**Constrained 1%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| A btree-fixed | 163 [155-163]; 13.0 KiB | 156 [151-158]; 12.6 KiB | 163 [162-167]; 13.0 KiB; 14793.8 ns/e | 1,775 [1,764-1,785]; 140.9 KiB | 1,877 [1,868-1,979]; 152.8 KiB; 16173.9 ns/e |
| B btree-compressed | 99.3 [94.5-99.8]; 8.0 KiB | 97.1 [93.6-98.4]; 7.9 KiB | 100 [98.9-100]; 8.0 KiB; 9052.5 ns/e | 1,083 [1,066-1,090]; 85.7 KiB | 1,163 [1,146-1,195]; 93.1 KiB; 9966.7 ns/e |
| C node-blocks | 186 [180-186]; 15.3 KiB | 187 [178-188]; 15.4 KiB | 187 [186-188]; 15.4 KiB; 17017.9 ns/e | 2,033 [2,008-2,049]; 165.8 KiB | 2,179 [2,179-2,231]; 180.3 KiB; 18675.4 ns/e |
| D record-chains | 2,132 [2,052-2,145]; 177.2 KiB | 1,481 [1,450-1,495]; 123.2 KiB | 2,221 [2,214-2,227]; 183.9 KiB; 201321.2 ns/e | 12,664 [12,418-12,720]; 1039.8 KiB | not run |
| F csr-paged | 168 [162-169]; 13.8 KiB | 169 [169-170]; 13.8 KiB | 169 [168-171]; 13.8 KiB; 15421.4 ns/e | 1,829 [1,820-1,855]; 149.0 KiB | 2,009 [1,981-2,029]; 162.0 KiB; 17212.4 ns/e |
| G csr-paged-compressed | 164 [158-166]; 13.6 KiB | 166 [166-167]; 13.6 KiB | 166 [166-168]; 13.6 KiB; 15187.4 ns/e | 1,804 [1,794-1,833]; 147.0 KiB | 1,937 [1,934-1,984]; 158.8 KiB; 16576.3 ns/e |

**Constrained 25%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 5.75 [5.74-6.03]; 0.97 miss (5.8% seq) | 5.54 [5.54-5.76]; 0.92 miss (1.8% seq) | 5.84 [5.72-5.87]; 0.96 miss (6.1% seq); 527.6 ns/e | 65 [64.7-65.6]; 10.41 miss (6.2% seq) | 71.3 [69.6-72.3]; 11.33 miss (5.4% seq); 608.9 ns/e | 630 [611-726]; 105.27 miss (5.6% seq); 593.5 ns/e | 5,467,339 [5,453,071-5,480,941]; 960541.67 miss (5.7% seq); 546.8 ns/e |
| B btree-compressed | 3.26 [3.01-4.71]; 0.51 miss (1.0% seq) | 3.19 [3.1-4.4]; 0.50 miss (0.3% seq) | 3.27 [3.2-4.26]; 0.47 miss (0.7% seq); 295.2 ns/e | 38.5 [36.7-48.6]; 4.95 miss (0.7% seq) | 43.7 [41.1-52.5]; 5.61 miss (0.8% seq); 359.9 ns/e | 431 [351-455]; 50.17 miss (0.7% seq); 352.8 ns/e | 3,023,216 [3,007,764-3,031,958]; 396638.67 miss (0.5% seq); 302.3 ns/e |
| C node-blocks | 4.41 [4.29-5.81]; 0.91 miss (0.0% seq) | 4.49 [4.39-5.17]; 0.87 miss (0.0% seq) | 4.55 [4.42-4.96]; 0.85 miss (0.0% seq); 416.1 ns/e | 50 [49.3-51.5]; 8.94 miss (0.0% seq) | 56.4 [55.4-57.2]; 9.80 miss (0.0% seq); 483.1 ns/e | 503 [466-568]; 91.17 miss (0.0% seq); 464.8 ns/e | 4,234,105 [4,229,654-4,243,727]; 812642.67 miss (0.0% seq); 423.4 ns/e |
| D record-chains | 51.2 [49.7-51.8]; 9.97 miss (0.0% seq) | 49.4 [48.2-50.2]; 9.88 miss (0.0% seq) | 50.6 [49.9-50.9]; 10.00 miss (0.0% seq); 4569.7 ns/e | 500 [500-503]; 99.29 miss (0.0% seq) | 559 [535-567]; 108.56 miss (0.0% seq); 4677.4 ns/e | not run | not run |
| F csr-paged | 1.24 [1.15-3.68]; 0.33 miss (0.9% seq) | 1.29 [1.22-3.31]; 0.31 miss (0.4% seq) | 1.29 [1.28-2.98]; 0.28 miss (0.5% seq); 117.1 ns/e | 14.8 [14.3-31.9]; 2.96 miss (0.5% seq) | 18.8 [17.6-35.4]; 3.34 miss (1.0% seq); 164.3 ns/e | 161 [123-282]; 27.10 miss (0.4% seq); 132.0 ns/e | 1,063,251 [1,057,393-1,069,320]; 161477.33 miss (0.2% seq); 106.3 ns/e |
| G csr-paged-compressed | 0.496 [0.491-2.86]; 0.17 miss (0.6% seq) | 0.493 [0.485-2.37]; 0.13 miss (0.1% seq) | 0.507 [0.506-1.95]; 0.10 miss (0.3% seq); 46.3 ns/e | 6.67 [6.49-21.1]; 0.99 miss (0.3% seq) | 7.48 [7.12-21.2]; 0.97 miss (0.0% seq); 62.3 ns/e | 71 [70.2-175]; 8.03 miss (0.0% seq); 70.3 ns/e | 286,160 [285,743-293,996]; 617.33 miss (0.1% seq); 28.6 ns/e |

**Constrained 10%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 6.06 [6.02-6.14]; 1.02 miss (6.1% seq) | 5.84 [5.8-5.87]; 0.98 miss (1.8% seq) | 6.1 [6.06-6.24]; 1.03 miss (6.6% seq); 551.6 ns/e | 68.6 [67.7-68.8]; 11.13 miss (6.7% seq) | 74.2 [72.1-74.8]; 12.10 miss (6.0% seq); 630.2 ns/e | 648 [627-798]; 112.43 miss (5.9% seq); 620.0 ns/e | 5,760,903 [5,695,982-5,761,428]; 1027703.33 miss (6.1% seq); 576.1 ns/e |
| B btree-compressed | 4.69 [4.66-4.74]; 0.77 miss (1.1% seq) | 4.56 [4.54-4.58]; 0.76 miss (0.4% seq) | 4.74 [4.68-4.77]; 0.76 miss (1.1% seq); 428.2 ns/e | 54.4 [53.6-54.5]; 8.18 miss (0.8% seq) | 61.5 [55.4-63.3]; 9.11 miss (1.3% seq); 517.2 ns/e | 526 [492-609]; 84.93 miss (1.1% seq); 494.5 ns/e | 4,554,181 [4,521,908-4,580,305]; 769923.33 miss (1.0% seq); 455.4 ns/e |
| C node-blocks | 6.74 [6.63-6.77]; 1.31 miss (0.0% seq) | 6.78 [6.77-6.83]; 1.33 miss (0.0% seq) | 6.74 [6.65-6.79]; 1.32 miss (0.0% seq); 608.8 ns/e | 75.4 [74.3-75.8]; 14.37 miss (0.0% seq) | 83.8 [80.6-84.3]; 15.76 miss (0.0% seq); 704.9 ns/e | 718 [702-833]; 145.10 miss (0.0% seq); 681.3 ns/e | 6,491,717 [6,477,048-6,549,768]; 1325999.00 miss (0.0% seq); 649.2 ns/e |
| D record-chains | 73.7 [73-75.6]; 15.12 miss (0.0% seq) | 65 [63-65.8]; 13.34 miss (0.0% seq) | 74.6 [72.5-74.9]; 15.20 miss (0.0% seq); 6744.6 ns/e | 584 [578-592]; 118.38 miss (0.0% seq) | 638 [626-662]; 130.56 miss (0.0% seq); 5410.6 ns/e | not run | not run |
| F csr-paged | 3.77 [3.74-3.79]; 0.70 miss (1.0% seq) | 3.77 [3.75-3.77]; 0.71 miss (1.0% seq) | 3.76 [3.75-3.77]; 0.70 miss (1.3% seq); 340.6 ns/e | 42 [41.9-43.3]; 7.67 miss (1.0% seq) | 46.9 [45.8-49.6]; 8.50 miss (1.3% seq); 401.9 ns/e | 419 [387-478]; 79.13 miss (1.0% seq); 390.8 ns/e | 3,579,130 [3,571,136-3,605,643]; 711277.00 miss (1.0% seq); 357.9 ns/e |
| G csr-paged-compressed | 2.71 [2.61-2.87]; 0.48 miss (0.5% seq) | 2.62 [2.62-2.67]; 0.46 miss (0.3% seq) | 2.6 [2.58-2.75]; 0.45 miss (0.5% seq); 237.5 ns/e | 31.1 [29.6-31.1]; 5.05 miss (0.5% seq) | 32.6 [30.9-33.3]; 5.34 miss (0.0% seq); 270.4 ns/e | 287 [277-320]; 50.77 miss (0.2% seq); 269.9 ns/e | 2,500,952 [2,479,177-2,504,607]; 456297.67 miss (0.4% seq); 250.1 ns/e |

**Constrained 1%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw | 3h-u | BFS |
|---|---|---|---|---|---|---|---|
| A btree-fixed | 8.16 [8.07-8.42]; 1.63 miss (4.1% seq) | 7.9 [7.77-8.08]; 1.58 miss (1.3% seq) | 8.1 [8.05-8.18]; 1.63 miss (4.5% seq); 735.5 ns/e | 91.5 [90.9-92.1]; 17.61 miss (4.6% seq) | 102 [102-107]; 19.10 miss (3.8% seq); 872.1 ns/e | 887 [858-1,032]; 178.63 miss (4.1% seq); 844.0 ns/e | 7,913,994 [7,903,612-7,941,519]; 1628613.33 miss (4.2% seq); 791.4 ns/e |
| B btree-compressed | 5.5 [5.37-6.08]; 1.00 miss (1.4% seq) | 5.22 [5.19-5.36]; 0.98 miss (0.4% seq) | 5.29 [5.26-5.29]; 0.99 miss (1.5% seq); 478.3 ns/e | 60.9 [60.7-61.4]; 10.71 miss (1.0% seq) | 67.9 [65.5-71.8]; 11.63 miss (1.4% seq); 582.2 ns/e | 599 [577-692]; 109.03 miss (1.2% seq); 566.4 ns/e | 5,143,129 [5,129,776-5,160,023]; 992710.33 miss (1.3% seq); 514.3 ns/e |
| C node-blocks | 9.14 [9.04-9.15]; 1.92 miss (0.0% seq) | 8.84 [8.83-8.84]; 1.92 miss (0.0% seq) | 8.81 [8.8-8.84]; 1.92 miss (0.0% seq); 799.0 ns/e | 98.4 [98.3-99.1]; 20.73 miss (0.0% seq) | 110 [109-114]; 22.53 miss (0.0% seq); 931.2 ns/e | 957 [918-1,120]; 210.50 miss (0.0% seq); 908.4 ns/e | 8,633,723 [8,624,449-8,643,181]; 1922313.33 miss (0.0% seq); 863.4 ns/e |
| D record-chains | 99.1 [98.7-99.2]; 22.15 miss (0.0% seq) | 70.2 [69.7-70.3]; 15.40 miss (0.0% seq) | 103 [103-103]; 22.99 miss (0.0% seq); 9281.0 ns/e | 603 [593-604]; 129.97 miss (0.0% seq) | 658 [649-679]; 141.98 miss (0.0% seq); 5561.0 ns/e | not run | not run |
| F csr-paged | 7.65 [7.63-8.11]; 1.72 miss (0.9% seq) | 7.74 [7.7-7.78]; 1.72 miss (0.9% seq) | 7.95 [7.79-8.13]; 1.73 miss (0.9% seq); 718.7 ns/e | 87.1 [86.8-87.4]; 18.63 miss (0.9% seq) | 96 [94.8-98.5]; 20.24 miss (1.2% seq); 822.9 ns/e | 844 [814-988]; 188.63 miss (0.9% seq); 805.6 ns/e | 7,567,311 [7,549,518-7,576,747]; 1722773.00 miss (0.8% seq); 756.8 ns/e |
| G csr-paged-compressed | 7.7 [7.61-7.93]; 1.70 miss (0.4% seq) | 7.7 [7.63-7.81]; 1.70 miss (0.6% seq) | 7.69 [7.62-7.72]; 1.70 miss (0.4% seq); 697.6 ns/e | 87.2 [87-88.3]; 18.38 miss (0.5% seq) | 96.1 [95.1-99]; 19.84 miss (0.4% seq); 823.8 ns/e | 842 [807-998]; 185.47 miss (0.5% seq); 798.5 ns/e | 7,555,076 [7,550,496-7,571,269]; 1699037.33 miss (0.5% seq); 755.5 ns/e |

**Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2394 [2324, 2648] | 11.02 | 3.01 |  |
| B btree-compressed | 3921 [3893, 3958] | 9.02 | 3.01 |  |
| C node-blocks | 827 [819, 883] | 9.33 | 5.00 |  |
| D record-chains | 1587 [1478, 1703] | 20.01 | 6.00 | existing records rewritten/insert 4.00 |

**Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)**

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2463 [2366, 2615] | 13.00 | 2.00 |
| B btree-compressed | 4629 [4603, 4887] | 9.00 | 2.00 |
| C node-blocks | 1346 [1344, 1388] | 9.00 | 2.00 |
| D record-chains | 817 [814, 818] | 4.00 | 1.00 |

**MVCC visibility control (A-D must agree and match the generator's counts)**

| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |
|---|---|---|
| A btree-fixed | 31967 | 30935 |
| B btree-compressed | 31967 | 30935 |
| C node-blocks | 31967 | 30935 |
| D record-chains | 31967 | 30935 |
expected from generator: 31967 / 30935 (difference proves the stamps are checked)

## 10 M nodes / 100 M edges: zipf, bulk load (B and G)

`layout-bench large zipf bulk n=10000000 m=100000000 alpha=0.8 arms=BG build cache 1024 MiB (131072 frames per arm)`

**Bulk load (bounded build cache, dirty pages written back with O_DIRECT)**

| layout | seconds | edges/s | pages | GiB written | peak RSS during build (MiB) | RSS before (MiB) |
|---|---|---|---|---|---|---|
| B btree-compressed | 53.2 | 1878525 | 585044 | 4.46 | 2725 | 1662 |
| G csr-paged-compressed | 28.0 | 3571681 | 201689 | 1.54 | 7224 | 2715 |

**Space**

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| B btree-compressed | 240894 | 19.7 | 344150 | 47.9 | leaves 239812, height 4, entries 200000000, leaf bytes used 17.7 B/edge (8.8 B/entry), leaf fill 0.90; edge table 344150 pages |
| G csr-paged-compressed | 201689 | 16.5 | 0 | 16.5 | offset pages 19532, entry pages 182157, entry stream 14.9 B/edge; read-only snapshot, no stamps |
RSS after build: 3729 MiB

**Constrained 10%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| B btree-compressed | 5.25 [5.1-6.27]; 0.82 miss (1.3% seq) | 4.97 [4.88-5.9]; 0.80 miss (0.3% seq) | 133 [131-135]; 3.19 miss (75.0% seq); 9.4 ns/e | 3,461 [3,459-3,932]; 18.64 miss (57.7% seq) | 144,957 [124,516-272,844]; 12893.37 miss (35.5% seq); 54.5 ns/e |
| G csr-paged-compressed | 2.65 [2.52-7.24]; 0.75 miss (0.7% seq) | 2.5 [2.35-6.54]; 0.69 miss (0.6% seq) | 101 [99.4-103]; 2.38 miss (69.2% seq); 7.1 ns/e | 2,493 [2,320-2,889]; 14.16 miss (50.5% seq) | 97,336 [79,259-181,480]; 7789.62 miss (45.1% seq); 36.2 ns/e |

**Constrained 10%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| B btree-compressed | 78.7 [75.5-100]; 6.7 KiB | 76.6 [73.6-94]; 6.6 KiB | 277 [238-350]; 19.8 KiB; 18.7 ns/e | 4,830 [4,368-5,262]; 134.1 KiB | 1,231,076 [914,936-1,709,724]; 101792.5 KiB; 338.9 ns/e |
| G csr-paged-compressed | 44 [39.5-141]; 6.2 KiB | 44.7 [38.3-125]; 5.7 KiB | 197 [164-316]; 14.7 KiB; 13.3 ns/e | 3,537 [3,082-4,093]; 101.0 KiB | 591,898 [318,605-981,395]; 62028.1 KiB; 152.0 ns/e |

**Constrained 1%, misses served by the OS page cache (buffered pread)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| B btree-compressed | 7.22 [6.08-7.71]; 1.19 miss (1.2% seq) | 6.18 [5.79-6.26]; 1.01 miss (0.3% seq) | 136 [133-141]; 4.36 miss (75.1% seq); 9.5 ns/e | 3,501 [3,238-4,018]; 35.38 miss (63.6% seq) | 156,276 [130,108-293,805]; 15510.17 miss (39.4% seq); 58.6 ns/e |
| G csr-paged-compressed | 8.46 [8.01-8.66]; 1.69 miss (0.6% seq) | 8.08 [7.88-8.09]; 1.64 miss (0.5% seq) | 107 [106-111]; 3.97 miss (55.8% seq); 7.5 ns/e | 2,570 [2,355-3,003]; 29.05 miss (38.6% seq) | 124,538 [94,020-233,869]; 14738.34 miss (41.0% seq); 44.8 ns/e |

**Constrained 1%, misses read from the device (pread, O_DIRECT)**

| layout | 1h-u | 1h-t2 | 1h-dw | 2h-u | 2h-dw |
|---|---|---|---|---|---|
| B btree-compressed | 115 [99.8-122]; 9.0 KiB | 100 [96.2-102]; 8.1 KiB | 383 [371-391]; 35.0 KiB; 26.5 ns/e | 5,687 [5,269-6,740]; 283.1 KiB | 1,435,765 [953,257-2,020,219]; 121940.6 KiB; 400.5 ns/e |
| G csr-paged-compressed | 167 [158-170]; 13.7 KiB | 158 [157-160]; 13.2 KiB | 384 [380-389]; 31.8 KiB; 26.7 ns/e | 4,883 [4,382-5,699]; 229.6 KiB | 1,005,682 [516,080-2,058,045]; 117474.8 KiB; 252.8 ns/e |

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

