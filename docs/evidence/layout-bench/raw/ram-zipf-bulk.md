# layout-bench zipf bulk n=1000000 m=10000000 alpha=0.8 scale=1
clock: Instant::now() pair costs 2792 ns (checksum 3)
graph generated in 1.8s: max out-degree 133939, max in-degree 131746, nodes with out-degree 0: 38495, nodes with total degree > 50: 45809, out-degree p50 4 p90 14 p99 85 p99.9 535

### Build (sorted bulk load, created_ts = 0 (frozen))

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 3.61 | 2767509 | 0.02 | 0.02 | includes sort | 1397 |
| B | 3.40 | 2940054 | 0.01 | 0.01 | includes sort | 1841 |
| C | 3.20 | 3125912 | 0.39 | 0.02 | includes sort | 4012 |
| D | 9.24 | 1082272 | 22.04 | 0.01 | existing records rewritten/edge 5.15, densified nodes 45809 | 4055 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.27 | 37547086 | 123460 | a7c193c4f2af92bf |
| B btree-compressed | 0.34 | 29319565 | 22002 | a7c193c4f2af92bf |
| C node-blocks | 0.29 | 33974698 | 3945261 | a7c193c4f2af92bf |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.27 | 33702 |
| G csr-paged-compressed | 0.66 | 18369 |

### Space

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| A btree-fixed | 124015 | 101.6 | 34417 | 129.8 | leaves 123457, height 4, entries 20000000, leaf fill 0.89; edge table 34417 pages |
| B btree-compressed | 22100 | 18.1 | 34417 | 46.3 | leaves 22000, height 3, entries 20000000, leaf bytes used 16.2 B/edge (8.1 B/entry), leaf fill 0.90; edge table 34417 pages |
| C node-blocks | 120104 | 98.4 | 34418 | 126.6 | dir 3922, block slabs 88730, chain 27452; entry bytes 72.0 B/edge, block bytes reserved 70.2 B/edge; relocations 0, entries copied 0; edge table 34417 pages |
| D record-chains | 67495 | 55.3 | 16205 | 68.6 | node recs 2942, rel recs 64103, groups 450; node map 1475 pages, edge map 14729 pages; densified 45809 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 18369 | 15.0 | 0 | 15.0 | offset pages 1954, entry pages 16415, entry stream 13.4 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |

RSS after build: 4577 MiB, peak 4641 MiB

## Constrained cache: 14129 frames = 110.4 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 8.9% of its 158432 pages
- B btree-compressed: cache = 25.0% of its 56517 pages
- C node-blocks: cache = 9.1% of its 154522 pages
- D record-chains: cache = 16.9% of its 83700 pages
- F csr-paged: cache = 41.9% of its 33702 pages
- G csr-paged-compressed: cache = 76.9% of its 18369 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6526 [6526, 6526] | 712.9 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.13 | 5.3 | 3.5 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5105 [5105, 5105] | 557.7 | n/a | n/a | 9.2 | 3.01 | 3.01 | 0.89 | 0.9 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 6912 [6912, 6912] | 755.0 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.39 | 0.4 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 31248 [31248, 31248] | 3413.5 | n/a | n/a | 9.2 | 27.40 | 18.72 | 6.15 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4858 [4858, 4858] | 530.7 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 0.8 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4413 [4413, 4413] | 482.1 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 0.3 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 25%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5495 [5308, 5894] | 602.6 | n/a | n/a | 9.4 | 4.06 | 4.06 | 0.91 | 6.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 2482 [2412, 4480] | 273.8 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.37 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 2281 [2255, 4780] | 256.0 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.55 | 1.4 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 35311 [28225, 36219] | 3602.0 | n/a | n/a | 9.4 | 27.97 | 18.91 | 6.51 | 0.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 456 [441, 3242] | 51.8 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.21 | 1.3 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 491 [461, 2519] | 52.3 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.15 | 0.7 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5205 [5132, 5486] | 2504.5 | n/a | n/a | 2.2 | 4.02 | 4.02 | 0.86 | 2.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 2392 [2314, 4100] | 1131.5 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.35 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 2352 [2210, 4105] | 1080.6 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.50 | 1.1 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 26965 [26491, 27310] | 12309.9 | n/a | n/a | 2.2 | 16.94 | 11.96 | 5.26 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 396 [394, 2881] | 193.8 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.18 | 1.2 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 459 [445, 1986] | 217.3 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.11 | 0.2 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 39986 [38856, 42564] | 10.4 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 4.74 | 81.9 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 38059 [33697, 42617] | 9.7 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 0.95 | 53.4 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 24463 [22970, 27711] | 6.1 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 3.44 | 75.8 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 186906 [167805, 203169] | 48.4 | n/a | n/a | 3943.8 | 7897.57 | 456.56 | 8.73 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 13052 [11649, 13693] | 3.4 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.14 | 73.7 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 28621 [23363, 29814] | 6.7 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.20 | 47.7 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 338878 [317328, 379452] | 9.9 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 26.41 | 57.6 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 377823 [367348, 442833] | 11.5 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 6.82 | 24.3 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 254062 [235409, 292610] | 7.6 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 20.11 | 48.1 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 799663 [798281, 965362] | 25.0 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 81.62 | 0.8 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 194328 [190030, 226430] | 5.9 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 5.93 | 42.1 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 282538 [267757, 321225] | 8.4 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 1.18 | 19.5 | 3.1 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 20672058 [20278338, 42762556] | 43.4 | n/a | n/a | 575198.1 | 15182.43 | 6090.08 | 4743.73 | 49.6 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 10955815 [9949582, 20671443] | 22.0 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 1098.93 | 24.2 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 15433621 [15089891, 31104096] | 32.3 | n/a | n/a | 575198.1 | 7873.82 | 4822.22 | 3702.54 | 31.1 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 68821915 [61579471, 156650846] | 137.9 | n/a | n/a | 575198.1 | 599046.72 | 9157.42 | 17401.40 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 5161786 [4691714, 9074563] | 10.3 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 628.28 | 26.0 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 5058091 [4912110, 7935756] | 10.4 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 18.28 | 10.8 | 3.1 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 135100905 [99925204, 225141994] | 86.3 | 49100775 | 686123763 | 1858886.3 | 88260.77 | 25627.43 | 26991.30 | 39.9 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 59954673 [47427919, 96573684] | 38.3 | 28898056 | 297482404 | 1858886.3 | 59352.03 | 8319.20 | 6726.43 | 17.8 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 95482671 [71789216, 160745876] | 61.0 | 35509862 | 481524152 | 1858886.3 | 43764.83 | 19277.80 | 20649.97 | 24.8 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 23105934 [21127897, 35969114] | 14.6 | 14386344 | 76791329 | 1858886.3 | 40954.80 | 7853.83 | 3130.43 | 24.5 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 17464897 [16400834, 27461068] | 11.2 | 15267961 | 63871387 | 1858886.3 | 39407.73 | 4698.40 | 0.03 | 0.0 | 5.2 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 4872916531 [4868446790, 4926606412] | 492.5 | 4872916531 | 4872916531 | 9894697.0 | 3911407.00 | 109996.00 | 862920.00 | 8.3 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 2054518698 [2046795572, 2068599648] | 207.6 | 2054518698 | 2054518698 | 9894697.0 | 2895451.00 | 20978.00 | 210070.67 | 3.5 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 2394112735 [2385871100, 2402068507] | 242.0 | 2394112735 | 2394112735 | 9894697.0 | 1897490.00 | 61988.00 | 444029.67 | 4.1 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 247047517 [245942621, 252481639] | 25.0 | 247047517 | 247047517 | 9894697.0 | 1901817.00 | 16851.00 | 13789.00 | 14.7 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 236999906 [234809297, 237463511] | 24.0 | 236999906 | 236999906 | 9894697.0 | 1895619.00 | 9184.00 | 0.00 | - | - | 0.0 | 3 |

## Constrained cache: 5651 frames = 44.1 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.6% of its 158432 pages
- B btree-compressed: cache = 10.0% of its 56517 pages
- C node-blocks: cache = 3.7% of its 154522 pages
- D record-chains: cache = 6.8% of its 83700 pages
- F csr-paged: cache = 16.8% of its 33702 pages
- G csr-paged-compressed: cache = 30.8% of its 18369 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6310 [6310, 6310] | 689.4 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.13 | 5.3 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 4998 [4998, 4998] | 546.0 | n/a | n/a | 9.2 | 3.01 | 3.01 | 0.89 | 0.9 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 6882 [6882, 6882] | 751.8 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.41 | 0.4 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 41596 [41596, 41596] | 4543.9 | n/a | n/a | 9.2 | 27.40 | 18.72 | 8.60 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4857 [4857, 4857] | 530.6 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 0.8 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4410 [4410, 4410] | 481.8 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 0.3 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5921 [5884, 5969] | 608.9 | n/a | n/a | 9.4 | 4.06 | 4.06 | 0.99 | 6.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4409 [4392, 4444] | 457.4 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.70 | 0.9 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5545 [5516, 5873] | 604.5 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.09 | 0.7 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 44821 [40664, 45290] | 4572.1 | n/a | n/a | 9.4 | 27.97 | 18.91 | 8.93 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3303 [3294, 3321] | 339.9 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.61 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2032 [2020, 2508] | 230.7 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.36 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5701 [5664, 5787] | 2585.9 | n/a | n/a | 2.2 | 4.02 | 4.02 | 0.95 | 2.0 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4265 [4264, 4278] | 1953.0 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.69 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5851 [5841, 5982] | 2730.9 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.14 | 0.5 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 36493 [35761, 37274] | 16659.6 | n/a | n/a | 2.2 | 16.94 | 11.96 | 7.49 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3205 [3176, 3292] | 1503.1 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.60 | 0.9 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2025 [1981, 2042] | 932.2 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.33 | 0.2 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 44293 [42485, 46368] | 11.5 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 5.74 | 82.9 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 38252 [35439, 43914] | 9.9 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 1.28 | 43.2 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 27846 [27762, 29784] | 7.2 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 4.39 | 64.1 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 221105 [209163, 244498] | 57.4 | n/a | n/a | 3943.8 | 7897.57 | 456.56 | 18.48 | 0.3 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 14690 [14293, 16327] | 3.8 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.67 | 56.5 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 29372 [27140, 34147] | 7.6 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.80 | 41.5 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 492543 [478415, 545850] | 15.0 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 60.72 | 74.2 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 383260 [380589, 466258] | 12.0 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 11.50 | 18.7 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 343310 [328327, 394721] | 10.3 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 42.15 | 48.6 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 1110034 [1045628, 1364080] | 34.7 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 153.96 | 0.5 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 220065 [218982, 266095] | 6.9 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 13.05 | 26.9 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 306149 [301600, 363485] | 9.5 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 7.04 | 14.9 | 3.2 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 25526981 [23738097, 50625276] | 51.2 | n/a | n/a | 575198.1 | 15182.43 | 6090.08 | 5908.66 | 56.8 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 14641501 [13420137, 27842348] | 29.3 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 2349.78 | 41.3 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 21398625 [20619345, 41343736] | 44.2 | n/a | n/a | 575198.1 | 7873.82 | 4822.22 | 5367.51 | 33.3 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 81458118 [70376547, 180319126] | 163.3 | n/a | n/a | 575198.1 | 599046.72 | 9157.42 | 20481.98 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 11151546 [10469867, 21072685] | 22.4 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 2399.18 | 50.4 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 9427718 [8777596, 16611004] | 18.9 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 1269.67 | 45.6 | 3.1 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 143751893 [112258894, 237363773] | 91.9 | 70473763 | 742821719 | 1858886.3 | 88260.77 | 25627.43 | 29390.70 | 41.5 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 92205363 [67466413, 146979629] | 58.9 | 35511395 | 483571536 | 1858886.3 | 59352.03 | 8319.20 | 15543.43 | 32.7 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 135509666 [101884319, 215453294] | 86.6 | 52993836 | 683703057 | 1858886.3 | 43764.83 | 19277.80 | 29957.03 | 19.3 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 73270161 [53566862, 117041280] | 46.8 | 25764938 | 380930628 | 1858886.3 | 40954.80 | 7853.83 | 15016.13 | 41.9 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 53147756 [39350829, 81993547] | 33.4 | 24251601 | 247129206 | 1858886.3 | 39407.73 | 4698.40 | 8196.87 | 39.8 | 3.1 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5307717818 [5289371777, 5315132444] | 536.4 | 5307717818 | 5307717818 | 9894697.0 | 3911407.00 | 109996.00 | 965239.67 | 8.4 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 3854321625 [3851787120, 3882101134] | 389.5 | 3854321625 | 3854321625 | 9894697.0 | 2895451.00 | 20978.00 | 665470.33 | 7.9 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 5327920657 [5323649485, 5361667506] | 538.5 | 5327920657 | 5327920657 | 9894697.0 | 1897490.00 | 61988.00 | 1085725.67 | 1.9 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 2940845495 [2930785966, 2972797930] | 297.2 | 2940845495 | 2940845495 | 9894697.0 | 1901817.00 | 16851.00 | 587066.67 | 9.8 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 1753674655 [1745384193, 1770060784] | 177.2 | 1753674655 | 1753674655 | 9894697.0 | 1895619.00 | 9184.00 | 315262.00 | 9.2 | 3.1 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 565 frames = 4.4 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 158432 pages
- B btree-compressed: cache = 1.0% of its 56517 pages
- C node-blocks: cache = 0.4% of its 154522 pages
- D record-chains: cache = 0.7% of its 83700 pages
- F csr-paged: cache = 1.7% of its 33702 pages
- G csr-paged-compressed: cache = 3.1% of its 18369 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8023 [8023, 8023] | 876.4 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.62 | 3.9 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5267 [5267, 5267] | 575.4 | n/a | n/a | 9.2 | 3.01 | 3.01 | 1.00 | 0.9 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 8267 [8267, 8267] | 903.1 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.88 | 0.3 | 2.9 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 68340 [68340, 68340] | 7465.4 | n/a | n/a | 9.2 | 27.40 | 18.72 | 15.53 | 0.0 | 2.8 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 7532 [7532, 7532] | 822.7 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.68 | 0.7 | 2.9 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 7447 [7447, 7447] | 813.5 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.66 | 0.4 | 2.9 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 1%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8233 [8025, 8345] | 851.2 | n/a | n/a | 9.4 | 4.06 | 4.06 | 1.60 | 4.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5260 [5186, 5476] | 541.4 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.99 | 1.1 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 8917 [8631, 8936] | 911.5 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.88 | 0.4 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 70818 [68368, 70919] | 7298.9 | n/a | n/a | 9.4 | 27.97 | 18.91 | 15.74 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7773 [7667, 7821] | 797.9 | n/a | n/a | 9.4 | 1.98 | 1.98 | 1.68 | 0.8 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7713 [7412, 7724] | 787.9 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.66 | 0.5 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7760 [7691, 7772] | 3548.2 | n/a | n/a | 2.2 | 4.02 | 4.02 | 1.55 | 1.4 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5072 [5058, 5161] | 2315.3 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.98 | 0.4 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 8593 [8492, 8708] | 3975.2 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.87 | 0.3 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 44494 [43060, 44500] | 20315.1 | n/a | n/a | 2.2 | 16.94 | 11.96 | 9.69 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7522 [7357, 7587] | 3433.8 | n/a | n/a | 2.2 | 1.98 | 1.98 | 1.68 | 0.9 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7343 [7308, 7409] | 3382.2 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.65 | 0.4 | 2.9 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 123929 [113997, 143367] | 32.2 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 25.37 | 92.4 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 43769 [40813, 48757] | 11.4 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 2.56 | 57.4 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 79418 [72299, 91564] | 20.6 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 18.19 | 89.1 | 2.6 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 4222061 [3745606, 4801078] | 1056.8 | n/a | n/a | 4084.7 | 8179.33 | 462.33 | 1008.05 | 0.0 | 2.7 (io/miss 0.00) | 0.0 | 5600 |
| F csr-paged | 32496 [29193, 32910] | 8.4 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 5.86 | 67.9 | 2.7 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 37187 [34653, 40917] | 9.7 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 2.69 | 34.6 | 2.9 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1195336 [1164063, 1391212] | 36.4 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 229.88 | 89.1 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 472985 [456756, 592473] | 14.8 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 31.63 | 55.0 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 871637 [827222, 973687] | 25.8 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 169.10 | 84.8 | 2.8 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 1688783 [1680279, 1981442] | 52.3 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 301.83 | 0.3 | 2.9 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 433213 [427951, 516510] | 13.4 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 65.48 | 63.0 | 2.9 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 419877 [406018, 497101] | 12.9 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 34.90 | 34.0 | 3.0 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 29579162 [25976186, 52543148] | 59.3 | n/a | n/a | 575198.1 | 15182.43 | 6090.08 | 6723.71 | 57.0 | 3.0 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 16019019 [14851952, 29707642] | 32.1 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 2846.59 | 44.2 | 2.9 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 23465925 [22317339, 42590746] | 47.8 | n/a | n/a | 575198.1 | 7873.82 | 4822.22 | 6030.80 | 37.0 | 2.8 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 86459113 [72117380, 187725499] | 173.3 | n/a | n/a | 575198.1 | 599046.72 | 9157.42 | 22699.22 | 0.6 | 2.9 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 15046033 [14225497, 26067089] | 30.5 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 3498.91 | 45.3 | 2.8 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 14306832 [13955751, 23400921] | 29.9 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 2650.17 | 40.1 | 2.8 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 151596223 [120465729, 243164703] | 96.9 | 78853539 | 765552142 | 1858886.3 | 88260.77 | 25627.43 | 32909.33 | 39.3 | 3.0 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 98546898 [74296766, 157245119] | 63.0 | 40021021 | 495170124 | 1858886.3 | 59352.03 | 8319.20 | 18123.90 | 33.5 | 2.8 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 142074947 [108836667, 225845131] | 90.8 | 62241472 | 688279207 | 1858886.3 | 43764.83 | 19277.80 | 33236.07 | 19.5 | 2.9 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 96672632 [74287141, 147505522] | 60.1 | 46560889 | 431770323 | 1858886.3 | 40954.80 | 7853.83 | 21821.00 | 33.5 | 2.8 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 89899708 [70890842, 133027180] | 54.2 | 43024367 | 357895479 | 1858886.3 | 39407.73 | 4698.40 | 18103.50 | 34.3 | 2.8 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6909983540 [6899964439, 6958769888] | 698.4 | 6909983540 | 6909983540 | 9894697.0 | 3911407.00 | 109996.00 | 1428002.00 | 6.0 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 4604393805 [4594798903, 4648017231] | 465.3 | 4604393805 | 4604393805 | 9894697.0 | 2895451.00 | 20978.00 | 917023.33 | 7.6 | 2.8 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 7480514572 [7478655128, 7556848808] | 756.0 | 7480514572 | 7480514572 | 9894697.0 | 1897490.00 | 61988.00 | 1687006.33 | 1.3 | 2.9 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 6268851479 [6226512645, 6298731759] | 633.6 | 6268851479 | 6268851479 | 9894697.0 | 1901817.00 | 16851.00 | 1439147.33 | 5.3 | 2.8 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 6089094639 [6082575299, 6167904755] | 615.4 | 6089094639 | 6089094639 | 9894697.0 | 1895619.00 | 9184.00 | 1392865.67 | 5.8 | 2.8 (io/miss 0.00) | 0.0 | 3 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2244 [2225, 2255] | 11.02 | 3.01 |  |
| B btree-compressed | 3846 [3814, 3850] | 9.02 | 3.01 |  |
| C node-blocks | 708 [697, 744] | 9.89 | 5.01 |  |
| D record-chains | 1534 [1515, 1541] | 26.66 | 7.16 | existing records rewritten/insert 5.62 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2344 [2314, 2364] | 13.00 | 2.00 |
| B btree-compressed | 4522 [4494, 4541] | 9.00 | 2.00 |
| C node-blocks | 5887 [5723, 5927] | 26.57 | 2.00 |
| D record-chains | 806 [801, 810] | 4.00 | 1.00 |

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
| A btree-fixed | 124015 | 101.6 | 34933 | 130.2 | leaves 123457, height 4, entries 20300000, leaf fill 0.91; edge table 34933 pages |
| B btree-compressed | 22100 | 18.1 | 34933 | 46.7 | leaves 22000, height 3, entries 20300000, leaf bytes used 16.7 B/edge (8.2 B/entry), leaf fill 0.93; edge table 34933 pages |
| C node-blocks | 121928 | 99.9 | 34934 | 128.5 | dir 3922, block slabs 90074, chain 27932; entry bytes 73.1 B/edge, block bytes reserved 71.2 B/edge; relocations 48667, entries copied 322610; edge table 34933 pages |
| D record-chains | 68465 | 56.1 | 16426 | 69.5 | node recs 2942, rel recs 65065, groups 458; node map 1475 pages, edge map 14950 pages; densified 46697 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 18369 | 15.0 | 0 | 15.0 | offset pages 1954, entry pages 16415, entry stream 13.4 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |

RSS end: 4752 MiB, peak 4752 MiB
