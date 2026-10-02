# layout-bench uniform online n=1000000 m=10000000 alpha=0 scale=1
clock: Instant::now() pair costs 2797 ns (checksum 5)
graph generated in 0.2s: max out-degree 27, max in-degree 29, nodes with out-degree 0: 43, nodes with total degree > 50: 0, out-degree p50 10 p90 14 p99 18 p99.9 21

### Build (online inserts in random order, created_ts = edge id + 1)

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 19.99 | 500261 | 10.64 | 3.04 |  | 1623 |
| B | 38.61 | 258974 | 9.25 | 3.01 |  | 2210 |
| C | 6.78 | 1474353 | 9.73 | 4.95 | relocations/edge 0.732, entries copied/edge 2.53 | 3805 |
| D | 12.62 | 792496 | 19.76 | 5.90 | existing records rewritten/edge 3.90, densified nodes 0 | 4459 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.31 | 32419065 | 158139 | 4b99dd115ab53ad0 |
| B btree-compressed | 0.37 | 26836655 | 40028 | 4b99dd115ab53ad0 |
| C node-blocks | 0.71 | 14134279 | 3999912 | 4b99dd115ab53ad0 |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.31 | 33702 |
| G csr-paged-compressed | 0.70 | 19362 |

### Space

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 159169 | 130.4 | 34466 | 158.6 | leaves 158136, height 4, entries 20000000, leaf fill 0.70; edge table 34466 pages |
| B btree-compressed | 40284 | 33.0 | 34466 | 61.2 | leaves 40025, height 4, entries 20000000, leaf bytes used 24.5 B/edge (12.3 B/entry), leaf fill 0.75; edge table 34466 pages |
| C node-blocks | 166180 | 136.1 | 34466 | 164.4 | dir 3922, block slabs 162258, chain 0; entry bytes 72.0 B/edge, block bytes reserved 98.3 B/edge; relocations 7323162, entries copied 25291862; edge table 34466 pages |
| D record-chains | 67045 | 54.9 | 16225 | 68.2 | node recs 2942, rel recs 64103, groups 0; node map 1475 pages, edge map 14750 pages; densified 0 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 19362 | 15.9 | 0 | 15.9 | offset pages 1954, entry pages 17408, entry stream 14.3 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |

RSS after build: 5154 MiB, peak 5222 MiB

## Constrained cache: 18687 frames = 146.0 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 9.7% of its 193635 pages
- B btree-compressed: cache = 25.0% of its 74750 pages
- C node-blocks: cache = 9.3% of its 200646 pages
- D record-chains: cache = 22.4% of its 83270 pages
- F csr-paged: cache = 55.4% of its 33702 pages
- G csr-paged-compressed: cache = 96.5% of its 19362 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6906 [6906, 6906] | 693.7 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.27 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5672 [5672, 5672] | 569.7 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.00 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7765 [7765, 7765] | 780.1 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.54 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 83701 [83701, 83701] | 8408.0 | n/a | n/a | 10.0 | 33.91 | 33.89 | 16.90 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 5234 [5234, 5234] | 525.7 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 1.4 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4751 [4751, 4751] | 477.2 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 25%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5952 [5944, 6407] | 595.4 | n/a | n/a | 10.1 | 4.08 | 4.08 | 0.98 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4149 [4037, 5450] | 409.3 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.64 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 4923 [4899, 6162] | 490.7 | n/a | n/a | 10.1 | 2.00 | 2.00 | 0.99 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 81183 [81069, 83075] | 8131.3 | n/a | n/a | 10.1 | 34.14 | 34.12 | 16.21 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 462 [458, 3684] | 46.3 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.23 | 1.2 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 490 [489, 2949] | 49.0 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.17 | 0.6 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5694 [5666, 5986] | 2259.4 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.93 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4021 [3999, 5164] | 1595.6 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.63 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 4884 [4861, 5677] | 1928.8 | n/a | n/a | 2.5 | 2.00 | 2.00 | 0.95 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 80093 [79940, 80921] | 31783.0 | n/a | n/a | 2.5 | 26.64 | 26.64 | 16.04 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 438 [430, 3299] | 172.4 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.20 | 0.6 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 486 [482, 2420] | 191.4 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.13 | 0.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6005 [5834, 6260] | 542.8 | n/a | n/a | 11.0 | 4.10 | 4.10 | 0.98 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4236 [4120, 5102] | 387.2 | n/a | n/a | 11.0 | 4.03 | 4.03 | 0.62 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5001 [4910, 5468] | 452.1 | n/a | n/a | 11.0 | 2.00 | 2.00 | 0.94 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 85239 [84409, 85280] | 7707.2 | n/a | n/a | 11.0 | 36.05 | 36.03 | 16.94 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 478 [460, 3003] | 43.2 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.18 | 0.7 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 503 [499, 1983] | 45.6 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.10 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 67722 [65181, 71354] | 627.8 | n/a | n/a | 108.1 | 44.18 | 29.59 | 10.55 | 0.0 | 3.7 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 48034 [46986, 59550] | 445.3 | n/a | n/a | 108.1 | 43.50 | 24.63 | 6.67 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 54560 [52118, 57724] | 505.7 | n/a | n/a | 108.1 | 21.63 | 21.63 | 9.81 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 823733 [816678, 828234] | 7635.6 | n/a | n/a | 108.1 | 239.49 | 229.24 | 164.85 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 6005 [5943, 32090] | 55.7 | n/a | n/a | 108.1 | 21.81 | 21.78 | 1.82 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 6424 [4776, 20901] | 59.5 | n/a | n/a | 108.1 | 21.74 | 21.71 | 0.99 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 74161 [72757, 74598] | 635.5 | n/a | n/a | 117.8 | 47.96 | 31.89 | 11.47 | 0.0 | 3.7 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 55098 [51718, 63275] | 452.3 | n/a | n/a | 117.8 | 47.20 | 26.56 | 7.28 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 61508 [61261, 64085] | 535.8 | n/a | n/a | 117.8 | 23.44 | 23.44 | 11.03 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 884288 [883927, 942525] | 7704.6 | n/a | n/a | 117.8 | 261.19 | 249.97 | 179.20 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 7347 [7252, 33869] | 64.3 | n/a | n/a | 117.8 | 23.71 | 23.67 | 1.92 | 0.6 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 6779 [6311, 21858] | 59.3 | n/a | n/a | 117.8 | 23.54 | 23.50 | 0.97 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 675826 [626238, 723933] | 619.5 | 611886 | 1141358 | 1099.2 | 449.67 | 231.93 | 108.40 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 539886 [428921, 549782] | 441.6 | 479674 | 902918 | 1099.2 | 442.20 | 202.50 | 66.17 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 559944 [515978, 602876] | 510.5 | 515434 | 943984 | 1099.2 | 220.07 | 218.17 | 100.50 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 56684 [55063, 282574] | 54.5 | 50495 | 122852 | 1099.2 | 221.77 | 214.77 | 15.97 | 0.6 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 55266 [52276, 180556] | 54.7 | 50985 | 98896 | 1099.2 | 221.00 | 213.57 | 8.03 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5811181942 [5802762602, 5833761034] | 581.1 | 5811181942 | 5811181942 | 9999533.0 | 4086564.00 | 159169.00 | 976861.33 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 4009537220 [3998160485, 4057401320] | 401.0 | 4009537220 | 4009537220 | 9999533.0 | 4021775.00 | 40284.00 | 546013.00 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 4787255888 [4781225416, 4794497945] | 478.7 | 4787255888 | 4787255888 | 9999533.0 | 1999867.00 | 158129.00 | 904840.00 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 247045094 [242997960, 283231572] | 24.7 | 247045094 | 247045094 | 9999533.0 | 2015147.00 | 16851.00 | 2227.00 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 286677924 [278088471, 290130844] | 28.7 | 286677924 | 286677924 | 9999533.0 | 2009418.00 | 9681.00 | 617.33 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 7475 frames = 58.4 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.9% of its 193635 pages
- B btree-compressed: cache = 10.0% of its 74750 pages
- C node-blocks: cache = 3.7% of its 200646 pages
- D record-chains: cache = 9.0% of its 83270 pages
- F csr-paged: cache = 22.2% of its 33702 pages
- G csr-paged-compressed: cache = 38.6% of its 19362 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7271 [7271, 7271] | 730.4 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.27 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5870 [5870, 5870] | 589.6 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.00 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7738 [7738, 7738] | 777.3 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.54 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 111225 [111225, 111225] | 11172.8 | n/a | n/a | 10.0 | 33.91 | 33.89 | 22.98 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 5209 [5209, 5209] | 523.3 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 1.4 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4742 [4742, 4742] | 476.3 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6333 [6277, 6449] | 628.7 | n/a | n/a | 10.1 | 4.08 | 4.08 | 1.04 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5367 [5298, 5457] | 537.6 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.84 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6521 [6406, 6580] | 643.3 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.23 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 112739 [112561, 115940] | 11274.2 | n/a | n/a | 10.1 | 34.14 | 34.12 | 23.12 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3304 [3221, 3698] | 325.9 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.63 | 1.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1813 [1688, 2894] | 178.9 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.34 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6014 [5984, 6062] | 2386.4 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.98 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5185 [5166, 5210] | 2051.2 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.82 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6402 [6352, 6527] | 2540.6 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.22 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 103118 [102919, 104248] | 40840.7 | n/a | n/a | 2.5 | 26.64 | 26.64 | 21.22 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3333 [3288, 3347] | 1316.2 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.61 | 0.8 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1766 [1694, 2374] | 701.0 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.31 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6425 [6358, 6480] | 585.8 | n/a | n/a | 11.0 | 4.10 | 4.10 | 1.05 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5297 [5285, 5450] | 478.7 | n/a | n/a | 11.0 | 4.03 | 4.03 | 0.83 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6455 [6431, 6569] | 587.8 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.23 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 118454 [118342, 118753] | 10735.7 | n/a | n/a | 11.0 | 36.05 | 36.03 | 24.19 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3324 [3283, 3347] | 302.6 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.60 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1753 [1665, 1956] | 158.4 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.27 | 0.2 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 72100 [70986, 72633] | 665.6 | n/a | n/a | 108.1 | 44.18 | 29.59 | 11.24 | 0.0 | 3.7 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 62729 [61161, 64190] | 580.2 | n/a | n/a | 108.1 | 43.50 | 24.63 | 9.05 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 71568 [70945, 72561] | 660.6 | n/a | n/a | 108.1 | 21.63 | 21.63 | 13.28 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 999889 [992321, 1003407] | 9248.8 | n/a | n/a | 108.1 | 239.49 | 229.24 | 202.55 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 38109 [37890, 38586] | 352.5 | n/a | n/a | 108.1 | 21.81 | 21.78 | 6.61 | 0.8 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 21249 [21168, 21747] | 197.0 | n/a | n/a | 108.1 | 21.74 | 21.71 | 2.95 | 0.1 | 3.4 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 79476 [79336, 80598] | 681.0 | n/a | n/a | 117.8 | 47.96 | 31.89 | 12.21 | 0.0 | 3.8 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 69465 [67076, 72547] | 593.0 | n/a | n/a | 117.8 | 47.20 | 26.56 | 9.91 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 81357 [77483, 83180] | 679.9 | n/a | n/a | 117.8 | 23.44 | 23.44 | 14.70 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 1078617 [1055790, 1168860] | 9242.6 | n/a | n/a | 117.8 | 261.19 | 249.97 | 220.73 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 41638 [40967, 41959] | 351.0 | n/a | n/a | 117.8 | 23.71 | 23.67 | 7.18 | 1.2 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 22741 [22664, 22871] | 194.9 | n/a | n/a | 117.8 | 23.54 | 23.50 | 3.00 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 693280 [673354, 817096] | 666.2 | 634864 | 1193950 | 1099.2 | 449.67 | 231.93 | 115.10 | 0.0 | 3.7 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 598176 [571517, 711970] | 565.4 | 551402 | 1043510 | 1099.2 | 442.20 | 202.50 | 91.97 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 683586 [677607, 834591] | 670.4 | 658610 | 1156723 | 1099.2 | 220.07 | 218.17 | 133.97 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 365776 [347911, 430324] | 344.2 | 357731 | 589676 | 1099.2 | 221.77 | 214.77 | 67.17 | 0.9 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 197395 [185026, 241968] | 185.5 | 189412 | 349490 | 1099.2 | 221.00 | 213.57 | 29.03 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6108746922 [6094788503, 6120601071] | 610.9 | 6108746922 | 6108746922 | 9999533.0 | 4086564.00 | 159169.00 | 1046700.33 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 5264374608 [5248384224, 5269422915] | 526.5 | 5264374608 | 5264374608 | 9999533.0 | 4021775.00 | 40284.00 | 835027.00 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 6303777556 [6296576149, 6314967432] | 630.4 | 6303777556 | 6303777556 | 9999533.0 | 1999867.00 | 158129.00 | 1230546.67 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 3161093003 [3153513435, 3163685432] | 316.1 | 3161093003 | 3161093003 | 9999533.0 | 2015147.00 | 16851.00 | 603529.00 | 0.9 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 1579439136 [1575146334, 1580870043] | 158.0 | 1579439136 | 1579439136 | 9999533.0 | 2009418.00 | 9681.00 | 259228.00 | 0.2 | 3.2 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 747 frames = 5.8 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 193635 pages
- B btree-compressed: cache = 1.0% of its 74750 pages
- C node-blocks: cache = 0.4% of its 200646 pages
- D record-chains: cache = 0.9% of its 83270 pages
- F csr-paged: cache = 2.2% of its 33702 pages
- G csr-paged-compressed: cache = 3.9% of its 19362 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9263 [9263, 9263] | 930.5 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.78 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 6307 [6307, 6307] | 633.6 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.12 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 9108 [9108, 9108] | 914.9 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.90 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 139814 [139814, 139814] | 14044.6 | n/a | n/a | 10.0 | 33.91 | 33.89 | 30.85 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 7652 [7652, 7652] | 768.6 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.66 | 1.0 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 7519 [7519, 7519] | 755.3 | n/a | n/a | 10.0 | 2.01 | 2.01 | 1.63 | 0.5 | 3.0 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 1%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9466 [9445, 9557] | 942.8 | n/a | n/a | 10.1 | 4.08 | 4.08 | 1.76 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 6296 [6208, 6337] | 625.1 | n/a | n/a | 10.1 | 4.02 | 4.02 | 1.07 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 9421 [9230, 9522] | 939.4 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.90 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 140437 [140335, 142540] | 14062.1 | n/a | n/a | 10.1 | 34.14 | 34.12 | 31.02 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7777 [7693, 7786] | 768.1 | n/a | n/a | 10.1 | 2.02 | 2.02 | 1.65 | 0.9 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7715 [7579, 7733] | 761.1 | n/a | n/a | 10.1 | 2.01 | 2.01 | 1.62 | 0.5 | 3.1 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9111 [8835, 9188] | 3582.9 | n/a | n/a | 2.5 | 4.03 | 4.03 | 1.70 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 6020 [6010, 6086] | 2367.1 | n/a | n/a | 2.5 | 4.01 | 4.01 | 1.05 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 9434 [8989, 9464] | 3710.0 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.90 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 111933 [107972, 112941] | 44412.4 | n/a | n/a | 2.5 | 26.64 | 26.64 | 24.31 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7766 [7466, 7776] | 3053.7 | n/a | n/a | 2.5 | 2.02 | 2.02 | 1.64 | 0.9 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7641 [7381, 7661] | 3012.4 | n/a | n/a | 2.5 | 2.01 | 2.01 | 1.61 | 0.6 | 3.1 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9562 [9479, 9636] | 871.2 | n/a | n/a | 11.0 | 4.10 | 4.10 | 1.78 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 6058 [5954, 6239] | 553.8 | n/a | n/a | 11.0 | 4.03 | 4.03 | 1.07 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 9249 [9093, 9359] | 845.4 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.90 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 149735 [149360, 149841] | 13546.2 | n/a | n/a | 11.0 | 36.05 | 36.03 | 32.84 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7759 [7719, 7883] | 709.2 | n/a | n/a | 11.0 | 2.02 | 2.02 | 1.65 | 0.9 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7668 [7603, 7705] | 696.6 | n/a | n/a | 11.0 | 2.01 | 2.01 | 1.62 | 0.4 | 3.1 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 106335 [105311, 107532] | 985.7 | n/a | n/a | 108.1 | 44.18 | 29.59 | 18.96 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 72870 [71587, 74549] | 674.0 | n/a | n/a | 108.1 | 43.50 | 24.63 | 11.63 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 104002 [103053, 105014] | 964.1 | n/a | n/a | 108.1 | 21.63 | 21.63 | 20.51 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 1054042 [1041228, 1058959] | 9729.9 | n/a | n/a | 108.1 | 239.49 | 229.24 | 225.33 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 87460 [86699, 88305] | 807.4 | n/a | n/a | 108.1 | 21.81 | 21.78 | 17.76 | 0.9 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 85820 [84505, 86969] | 795.5 | n/a | n/a | 108.1 | 21.74 | 21.71 | 17.43 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 118720 [114455, 120259] | 983.0 | n/a | n/a | 117.8 | 47.96 | 31.89 | 20.74 | 0.0 | 3.7 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 76819 [76456, 83962] | 668.7 | n/a | n/a | 117.8 | 47.20 | 26.56 | 12.64 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 116715 [112236, 118955] | 972.4 | n/a | n/a | 117.8 | 23.44 | 23.44 | 22.32 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 1149208 [1130505, 1187968] | 9710.9 | n/a | n/a | 117.8 | 261.19 | 249.97 | 245.94 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 95097 [94962, 95972] | 814.9 | n/a | n/a | 117.8 | 23.71 | 23.67 | 19.46 | 1.3 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 95739 [94324, 97749] | 808.3 | n/a | n/a | 117.8 | 23.54 | 23.50 | 18.99 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1008218 [988676, 1157254] | 947.5 | 939165 | 1707987 | 1099.2 | 449.67 | 231.93 | 192.97 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 678585 [658030, 786638] | 643.4 | 629766 | 1168806 | 1099.2 | 442.20 | 202.50 | 117.10 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 998594 [965084, 1145737] | 938.4 | 960118 | 1720209 | 1099.2 | 220.07 | 218.17 | 208.73 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 829172 [803888, 956004] | 781.9 | 806187 | 1444892 | 1099.2 | 221.77 | 214.77 | 179.73 | 0.9 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 833090 [790702, 949292] | 782.3 | 812822 | 1437907 | 1099.2 | 221.00 | 213.57 | 175.77 | 0.5 | 3.1 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8945066459 [8942406848, 8952573075] | 894.5 | 8945066459 | 8945066459 | 9999533.0 | 4086564.00 | 159169.00 | 1767677.00 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 5985452696 [5939500286, 5991311210] | 598.6 | 5985452696 | 5985452696 | 9999533.0 | 4021775.00 | 40284.00 | 1067981.00 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 8888476662 [8877603516, 8894607173] | 888.9 | 8888476662 | 8888476662 | 9999533.0 | 1999867.00 | 158129.00 | 1904887.00 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 7349997195 [7349005135, 7354152883] | 735.0 | 7349997195 | 7349997195 | 9999533.0 | 2015147.00 | 16851.00 | 1643443.00 | 0.9 | 2.9 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 7352174513 [7345182095, 7356723022] | 735.3 | 7352174513 | 7352174513 | 9999533.0 | 2009418.00 | 9681.00 | 1613823.33 | 0.5 | 2.9 (io/miss 0.00) | 0.0 | 3 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2408 [2397, 2442] | 11.10 | 3.04 |  |
| B btree-compressed | 4511 [4506, 4513] | 11.06 | 3.02 |  |
| C node-blocks | 838 [833, 855] | 9.33 | 5.00 |  |
| D record-chains | 1368 [1354, 1415] | 20.01 | 6.00 | existing records rewritten/insert 4.00 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2601 [2560, 2694] | 13.00 | 2.00 |
| B btree-compressed | 4958 [4956, 5320] | 11.01 | 2.01 |
| C node-blocks | 1387 [1376, 1390] | 9.00 | 1.99 |
| D record-chains | 833 [814, 837] | 4.00 | 1.00 |

### MVCC visibility control (A-D must agree and match the generator's counts)

| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |
|---|---|---|
| A btree-fixed | 31967 | 30935 |
| B btree-compressed | 31967 | 30935 |
| C node-blocks | 31967 | 30935 |
| D record-chains | 31967 | 30935 |

expected from generator: 31967 / 30935 (difference proves the stamps are checked)

### Space

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 161601 | 132.4 | 34983 | 161.0 | leaves 160568, height 4, entries 20300000, leaf fill 0.70; edge table 34983 pages |
| B btree-compressed | 41964 | 34.4 | 34983 | 63.0 | leaves 41705, height 4, entries 20300000, leaf bytes used 25.0 B/edge (12.3 B/entry), leaf fill 0.73; edge table 34983 pages |
| C node-blocks | 169051 | 138.5 | 34983 | 167.1 | dir 3922, block slabs 165129, chain 0; entry bytes 73.1 B/edge, block bytes reserved 99.7 B/edge; relocations 7369648, entries copied 25691366; edge table 34983 pages |
| D record-chains | 68007 | 55.7 | 16446 | 69.2 | node recs 2942, rel recs 65065, groups 0; node map 1475 pages, edge map 14971 pages; densified 0 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 19362 | 15.9 | 0 | 15.9 | offset pages 1954, entry pages 17408, entry stream 14.3 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |

RSS end: 5370 MiB, peak 5370 MiB
