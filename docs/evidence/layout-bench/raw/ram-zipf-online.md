# layout-bench zipf online n=1000000 m=10000000 alpha=0.8 scale=1
clock: Instant::now() pair costs 2720 ns (checksum 3)
graph generated in 1.8s: max out-degree 133939, max in-degree 131746, nodes with out-degree 0: 38495, nodes with total degree > 50: 45809, out-degree p50 4 p90 14 p99 85 p99.9 535

### Build (online inserts in random order, created_ts = edge id + 1)

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 20.78 | 481309 | 10.63 | 3.04 |  | 1636 |
| B | 38.15 | 262091 | 9.22 | 3.01 |  | 2210 |
| C | 5.84 | 1712812 | 9.88 | 4.99 | relocations/edge 0.469, entries copied/edge 2.06 | 3448 |
| D | 13.36 | 748237 | 24.93 | 6.90 | existing records rewritten/edge 5.16, densified nodes 45809 | 4106 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.30 | 33200705 | 158832 | a7c193c4f2af92bf |
| B btree-compressed | 0.37 | 27308112 | 38270 | a7c193c4f2af92bf |
| C node-blocks | 0.62 | 16043945 | 3945261 | a7c193c4f2af92bf |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.30 | 33702 |
| G csr-paged-compressed | 0.65 | 18369 |

### Space

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

## Constrained cache: 18248 frames = 142.6 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 9.4% of its 194323 pages
- B btree-compressed: cache = 25.0% of its 72992 pages
- C node-blocks: cache = 11.6% of its 157292 pages
- D record-chains: cache = 21.8% of its 83720 pages
- F csr-paged: cache = 54.1% of its 33702 pages
- G csr-paged-compressed: cache = 99.3% of its 18369 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7120 [7120, 7120] | 777.8 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.24 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5781 [5781, 5781] | 631.5 | n/a | n/a | 9.2 | 4.02 | 4.02 | 0.97 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7437 [7437, 7437] | 812.4 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.47 | 0.2 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 61992 [61992, 61992] | 6772.0 | n/a | n/a | 9.2 | 27.40 | 26.85 | 12.41 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4893 [4893, 4893] | 534.6 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 0.8 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4412 [4412, 4412] | 482.0 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 0.3 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 25%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5617 [5528, 6263] | 627.6 | n/a | n/a | 9.4 | 4.09 | 4.09 | 0.93 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 3374 [3307, 5186] | 383.1 | n/a | n/a | 9.4 | 4.02 | 4.02 | 0.51 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 3262 [3233, 5735] | 367.0 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.73 | 0.4 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 63246 [60863, 66245] | 6757.6 | n/a | n/a | 9.4 | 27.97 | 27.31 | 12.59 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 407 [405, 3212] | 46.0 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.20 | 1.2 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 465 [456, 2507] | 51.8 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.15 | 0.7 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5263 [5187, 5582] | 2536.1 | n/a | n/a | 2.2 | 4.03 | 4.03 | 0.86 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 3180 [3156, 4662] | 1554.7 | n/a | n/a | 2.2 | 4.00 | 4.00 | 0.48 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 3387 [3256, 5117] | 1591.9 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.69 | 0.4 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 44516 [42763, 44653] | 20322.3 | n/a | n/a | 2.2 | 16.94 | 16.77 | 8.68 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 371 [362, 2887] | 177.1 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.18 | 1.1 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 456 [444, 1994] | 217.3 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.11 | 0.2 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 49192 [47784, 53695] | 12.8 | n/a | n/a | 3943.8 | 35.13 | 35.13 | 5.85 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 46305 [43028, 52501] | 12.0 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 1.56 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 25989 [24956, 29384] | 6.5 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 3.47 | 5.3 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 15608799 [13207028, 15845488] | 3748.8 | n/a | n/a | 3945.8 | 7901.72 | 2892.68 | 3080.45 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 1648 |
| F csr-paged | 8453 [6473, 13058] | 1.9 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 0.46 | 62.3 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 28347 [23316, 30545] | 6.8 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.20 | 47.7 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 399868 [373124, 457575] | 11.9 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 30.89 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 463895 [458032, 582483] | 14.5 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 10.67 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 313801 [273386, 350586] | 9.1 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 19.48 | 8.7 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 130724695 [122889475, 170047611] | 3513.9 | n/a | n/a | 39089.9 | 39205.01 | 14629.73 | 28551.99 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 192 |
| F csr-paged | 202423 [174991, 214701] | 5.6 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 2.59 | 34.1 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 283481 [268365, 325873] | 8.5 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 1.18 | 19.5 | 3.2 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 24541386 [23606046, 48925812] | 50.6 | n/a | n/a | 575198.1 | 16193.66 | 7277.83 | 5253.24 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 15091130 [13091191, 28681039] | 30.2 | n/a | n/a | 575198.1 | 12607.51 | 2823.57 | 1749.87 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 15815290 [14137343, 32126175] | 31.7 | n/a | n/a | 575198.1 | 7873.82 | 5145.52 | 3349.34 | 6.1 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 2440959974 [1650565317, 2908696692] | 3622.9 | n/a | n/a | 638150.4 | 669827.17 | 41560.90 | 480703.19 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 48 |
| F csr-paged | 3689124 [2997036, 4624635] | 6.1 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 66.71 | 17.9 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 5152988 [5014617, 7943640] | 10.5 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 18.28 | 10.8 | 3.1 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 153515867 [118001123, 259848752] | 98.1 | 61734591 | 748049984 | 1858886.3 | 91562.03 | 30012.70 | 29744.33 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 84477719 [68673353, 143403291] | 54.0 | 38717291 | 430547878 | 1858886.3 | 79980.67 | 12102.17 | 11014.07 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 105825351 [77446042, 177606972] | 67.6 | 38417946 | 551717853 | 1858886.3 | 43764.83 | 22107.43 | 20828.93 | 4.8 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 10173854 [9702619, 16405626] | 6.5 | 8791959 | 38797982 | 1858886.3 | 40954.80 | 7853.83 | 3.03 | 1.1 | 3.6 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 19550584 [16953402, 28656789] | 11.7 | 15588523 | 68918597 | 1858886.3 | 39407.73 | 4698.40 | 0.03 | 0.0 | 5.4 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5096011560 [5070420434, 5100769600] | 515.0 | 5096011560 | 5096011560 | 9894697.0 | 3930660.00 | 140243.00 | 875733.00 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 2947268274 [2931253850, 2985182049] | 297.9 | 2947268274 | 2947268274 | 9894697.0 | 3865082.00 | 35948.00 | 365206.67 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 3374490961 [3359598191, 3414424574] | 341.0 | 3374490961 | 3374490961 | 9894697.0 | 1897490.00 | 101486.00 | 623892.00 | 0.5 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 186136913 [179408244, 186800029] | 18.8 | 186136913 | 186136913 | 9894697.0 | 1901817.00 | 16851.00 | 0.67 | 0.0 | 5.2 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 256858104 [236164335, 258556662] | 26.0 | 256858104 | 256858104 | 9894697.0 | 1895619.00 | 9184.00 | 0.00 | - | - | 0.0 | 3 |

## Constrained cache: 7299 frames = 57.0 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.8% of its 194323 pages
- B btree-compressed: cache = 10.0% of its 72992 pages
- C node-blocks: cache = 4.6% of its 157292 pages
- D record-chains: cache = 8.7% of its 83720 pages
- F csr-paged: cache = 21.7% of its 33702 pages
- G csr-paged-compressed: cache = 39.7% of its 18369 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7057 [7057, 7057] | 770.9 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.24 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5704 [5704, 5704] | 623.1 | n/a | n/a | 9.2 | 4.02 | 4.02 | 0.97 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7333 [7333, 7333] | 801.0 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.47 | 0.2 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 80977 [80977, 80977] | 8845.9 | n/a | n/a | 9.2 | 27.40 | 26.85 | 16.55 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4867 [4867, 4867] | 531.7 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 0.8 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4414 [4414, 4414] | 482.2 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 0.3 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6214 [6212, 6293] | 647.7 | n/a | n/a | 9.4 | 4.09 | 4.09 | 1.02 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5003 [4989, 5175] | 532.6 | n/a | n/a | 9.4 | 4.02 | 4.02 | 0.78 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5748 [5673, 6085] | 626.2 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.09 | 0.3 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 83215 [81299, 84388] | 8608.3 | n/a | n/a | 9.4 | 27.97 | 27.31 | 16.95 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 2706 [2683, 3257] | 304.6 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.52 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1013 [948, 2522] | 107.6 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.22 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5858 [5733, 5866] | 2674.4 | n/a | n/a | 2.2 | 4.03 | 4.03 | 0.95 | 0.0 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4871 [4739, 4887] | 2231.0 | n/a | n/a | 2.2 | 4.00 | 4.00 | 0.75 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5858 [5839, 5885] | 2674.3 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.10 | 0.2 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 59044 [55646, 59724] | 26954.4 | n/a | n/a | 2.2 | 16.94 | 16.77 | 11.87 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 2649 [2555, 2875] | 1249.1 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.49 | 0.9 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 993 [951, 1988] | 485.2 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.18 | 0.2 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 56351 [53246, 58141] | 14.6 | n/a | n/a | 3943.8 | 35.13 | 35.13 | 7.24 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 46404 [42830, 53029] | 12.1 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 1.83 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 28749 [28425, 31325] | 7.5 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 4.07 | 4.6 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 16875766 [14942806, 17160518] | 4254.7 | n/a | n/a | 3828.6 | 7667.38 | 2802.36 | 3454.61 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 1504 |
| F csr-paged | 14114 [13822, 15545] | 3.7 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.54 | 59.1 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 28423 [25997, 32740] | 7.4 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.59 | 51.1 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 591452 [588224, 674071] | 18.3 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 73.39 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 472956 [462483, 581170] | 14.8 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 14.20 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 370916 [329065, 415675] | 10.8 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 34.38 | 5.9 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 148894146 [131651589, 192277992] | 3982.2 | n/a | n/a | 38768.7 | 38881.41 | 14659.14 | 32801.97 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 176 |
| F csr-paged | 227218 [225519, 271310] | 7.1 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 11.34 | 27.8 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 291888 [282106, 341560] | 8.9 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 4.02 | 18.0 | 3.2 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 30987334 [28968077, 60806777] | 62.1 | n/a | n/a | 575198.1 | 16193.66 | 7277.83 | 6899.77 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 18334826 [16874654, 35483069] | 36.7 | n/a | n/a | 575198.1 | 12607.51 | 2823.57 | 2853.50 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 22010836 [20431301, 47266623] | 44.1 | n/a | n/a | 575198.1 | 7873.82 | 5145.52 | 5289.08 | 4.6 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 2911585834 [1975590757, 3434247303] | 4321.4 | n/a | n/a | 638150.4 | 669827.17 | 41560.90 | 584533.23 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 48 |
| F csr-paged | 10176729 [9588851, 18989444] | 20.5 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 2075.31 | 48.7 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 7463433 [6864703, 12574281] | 15.0 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 666.34 | 30.0 | 3.1 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 165503042 [133773429, 275092162] | 105.8 | 90347440 | 828072551 | 1858886.3 | 91562.03 | 30012.70 | 32921.17 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 112435388 [83301968, 181919882] | 71.9 | 43515037 | 593724487 | 1858886.3 | 79980.67 | 12102.17 | 17992.43 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 140504137 [105222526, 235628813] | 89.8 | 49771850 | 755307818 | 1858886.3 | 43764.83 | 22107.43 | 29436.03 | 3.7 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 62398101 [47427666, 103424893] | 39.9 | 23916201 | 318939727 | 1858886.3 | 40954.80 | 7853.83 | 12677.10 | 39.1 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 32711628 [28225240, 53332742] | 20.9 | 20692844 | 141080816 | 1858886.3 | 39407.73 | 4698.40 | 3778.03 | 24.1 | 3.1 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5538437607 [5528201428, 5590047642] | 559.7 | 5538437607 | 5538437607 | 9894697.0 | 3930660.00 | 140243.00 | 985217.00 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 4459374194 [4423847370, 4497033844] | 450.7 | 4459374194 | 4459374194 | 9894697.0 | 3865082.00 | 35948.00 | 734779.00 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 5530774005 [5524472250, 5536848680] | 559.0 | 5530774005 | 5530774005 | 9894697.0 | 1897490.00 | 101486.00 | 1079005.00 | 0.3 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 2420379702 [2406929826, 2429214470] | 244.6 | 2420379702 | 2420379702 | 9894697.0 | 1901817.00 | 16851.00 | 465211.00 | 8.6 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 790078311 [784836820, 795532001] | 79.8 | 790078311 | 790078311 | 9894697.0 | 1895619.00 | 9184.00 | 108695.00 | 4.4 | 3.1 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 729 frames = 5.7 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 194323 pages
- B btree-compressed: cache = 1.0% of its 72992 pages
- C node-blocks: cache = 0.5% of its 157292 pages
- D record-chains: cache = 0.9% of its 83720 pages
- F csr-paged: cache = 2.2% of its 33702 pages
- G csr-paged-compressed: cache = 4.0% of its 18369 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9090 [9090, 9090] | 993.0 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.75 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 6225 [6225, 6225] | 680.0 | n/a | n/a | 9.2 | 4.02 | 4.02 | 1.12 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 8740 [8740, 8740] | 954.8 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.86 | 0.2 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 107897 [107897, 107897] | 11786.6 | n/a | n/a | 9.2 | 27.40 | 26.85 | 23.68 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 7374 [7374, 7374] | 805.5 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.61 | 0.8 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 7124 [7124, 7124] | 778.2 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.58 | 0.4 | 2.9 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 1%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9368 [9088, 9410] | 955.7 | n/a | n/a | 9.4 | 4.09 | 4.09 | 1.75 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5924 [5918, 6195] | 609.7 | n/a | n/a | 9.4 | 4.02 | 4.02 | 1.07 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 9117 [8928, 9258] | 944.4 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.87 | 0.2 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 113163 [105812, 113342] | 11664.9 | n/a | n/a | 9.4 | 27.97 | 27.31 | 24.15 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7536 [7420, 7628] | 778.1 | n/a | n/a | 9.4 | 1.98 | 1.98 | 1.60 | 0.8 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7416 [7155, 7456] | 760.6 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.57 | 0.6 | 3.1 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8678 [8663, 8686] | 3954.6 | n/a | n/a | 2.2 | 4.03 | 4.03 | 1.67 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5749 [5709, 5951] | 2624.4 | n/a | n/a | 2.2 | 4.00 | 4.00 | 1.04 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 8978 [8881, 9043] | 4098.6 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.86 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 67472 [65284, 68678] | 30802.0 | n/a | n/a | 2.2 | 16.94 | 16.77 | 14.45 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7278 [7236, 7391] | 3322.6 | n/a | n/a | 2.2 | 1.98 | 1.98 | 1.59 | 1.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7074 [7062, 7175] | 3275.4 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.56 | 0.4 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 160778 [144157, 184681] | 41.6 | n/a | n/a | 3943.8 | 35.13 | 35.13 | 31.88 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 61763 [55580, 66552] | 16.0 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 5.02 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 82147 [74769, 87259] | 21.3 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 16.42 | 1.6 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 25917758 [25507011, 26987162] | 6847.3 | n/a | n/a | 3822.2 | 7654.51 | 2878.44 | 5993.65 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 960 |
| F csr-paged | 29268 [26334, 29312] | 7.6 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 5.02 | 63.4 | 2.8 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 36158 [34020, 40131] | 9.4 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 2.47 | 31.2 | 2.9 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1543706 [1521067, 1784892] | 47.6 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 286.67 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 672337 [647092, 844439] | 21.0 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 58.35 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 970514 [928772, 1065107] | 28.9 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 164.76 | 1.9 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 146414422 [129184973, 186221249] | 3907.6 | n/a | n/a | 38768.7 | 38881.41 | 14659.14 | 34013.48 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 176 |
| F csr-paged | 427946 [418595, 498690] | 13.0 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 59.40 | 60.0 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 403759 [398264, 482199] | 12.5 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 32.59 | 31.7 | 3.0 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 36116382 [33186969, 64507780] | 72.4 | n/a | n/a | 575198.1 | 16193.66 | 7277.83 | 7962.69 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 21323993 [19333974, 39538863] | 42.7 | n/a | n/a | 575198.1 | 12607.51 | 2823.57 | 3630.29 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 30019752 [26580594, 61895835] | 60.2 | n/a | n/a | 575198.1 | 7873.82 | 5145.52 | 7613.62 | 3.3 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 3006390602 [2034663763, 3540601950] | 4462.1 | n/a | n/a | 638150.4 | 669827.17 | 41560.90 | 634159.17 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 48 |
| F csr-paged | 14741974 [14583942, 25924191] | 31.2 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 3482.99 | 45.2 | 2.8 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 14069963 [13702048, 23511576] | 30.1 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 2633.60 | 40.2 | 2.8 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 180975450 [147995452, 291364063] | 115.7 | 101291309 | 894676933 | 1858886.3 | 91562.03 | 30012.70 | 37836.27 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 125000229 [95834950, 199410212] | 79.9 | 56076158 | 649001697 | 1858886.3 | 79980.67 | 12102.17 | 21462.50 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 184685092 [134669486, 307945863] | 118.0 | 68696206 | 1036012228 | 1858886.3 | 43764.83 | 22107.43 | 42081.17 | 2.6 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 96845707 [74798186, 145912354] | 59.4 | 43228004 | 427549624 | 1858886.3 | 40954.80 | 7853.83 | 21728.30 | 33.6 | 2.8 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 89504224 [70290324, 133368779] | 54.3 | 43319426 | 362298540 | 1858886.3 | 39407.73 | 4698.40 | 17999.23 | 34.4 | 2.8 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7721998150 [7716233963, 7722357518] | 780.4 | 7721998150 | 7721998150 | 9894697.0 | 3930660.00 | 140243.00 | 1556955.67 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 5330003065 [5317854999, 5346743654] | 538.7 | 5330003065 | 5330003065 | 9894697.0 | 3865082.00 | 35948.00 | 1000361.67 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 8305445311 [8296949922, 8324677288] | 839.4 | 8305445311 | 8305445311 | 9894697.0 | 1897490.00 | 101486.00 | 1800880.67 | 0.2 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 6056159273 [6052600012, 6083906910] | 612.1 | 6056159273 | 6056159273 | 9894697.0 | 1901817.00 | 16851.00 | 1385034.67 | 5.5 | 2.8 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 5894189010 [5891688289, 5901161022] | 595.7 | 5894189010 | 5894189010 | 9894697.0 | 1895619.00 | 9184.00 | 1334444.33 | 6.0 | 2.8 (io/miss 0.00) | 0.0 | 3 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2476 [2470, 2477] | 11.10 | 3.04 |  |
| B btree-compressed | 4465 [4459, 4472] | 11.05 | 3.02 |  |
| C node-blocks | 711 [711, 748] | 9.89 | 5.01 |  |
| D record-chains | 1517 [1493, 1522] | 26.66 | 7.29 | existing records rewritten/insert 5.62 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2559 [2559, 2580] | 13.00 | 2.00 |
| B btree-compressed | 4969 [4968, 4980] | 11.01 | 2.00 |
| C node-blocks | 8826 [8574, 9057] | 26.58 | 2.00 |
| D record-chains | 839 [835, 840] | 4.00 | 1.00 |

### MVCC visibility control (A-D must agree and match the generator's counts)

| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |
|---|---|---|
| A btree-fixed | 12160037 | 12163086 |
| B btree-compressed | 12160037 | 12163086 |
| C node-blocks | 12160037 | 12163086 |
| D record-chains | 12160037 | 12163086 |

expected from generator: 12160037 / 12163086 (difference proves the stamps are checked)

### Space

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 162305 | 133.0 | 34983 | 161.6 | leaves 161274, height 4, entries 20300000, leaf fill 0.69; edge table 34983 pages |
| B btree-compressed | 39809 | 32.6 | 34983 | 61.3 | leaves 39550, height 4, entries 20300000, leaf bytes used 24.1 B/edge (11.9 B/entry), leaf fill 0.74; edge table 34983 pages |
| C node-blocks | 124649 | 102.1 | 34983 | 130.8 | dir 3922, block slabs 92795, chain 27932; entry bytes 73.1 B/edge, block bytes reserved 71.2 B/edge; relocations 4737143, entries copied 20966375; edge table 34983 pages |
| D record-chains | 68465 | 56.1 | 16446 | 69.6 | node recs 2942, rel recs 65065, groups 458; node map 1475 pages, edge map 14971 pages; densified 46697 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 18369 | 15.0 | 0 | 15.0 | offset pages 1954, entry pages 16415, entry stream 13.4 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |

RSS end: 5002 MiB, peak 5002 MiB
