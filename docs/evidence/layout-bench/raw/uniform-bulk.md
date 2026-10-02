# layout-bench uniform bulk n=1000000 m=10000000 alpha=0 scale=1
clock: Instant::now() pair costs 2757 ns (checksum 5)
graph generated in 0.2s: max out-degree 27, max in-degree 29, nodes with out-degree 0: 43, nodes with total degree > 50: 0, out-degree p50 10 p90 14 p99 18 p99.9 21

### Build (sorted bulk load, created_ts = 0 (frozen))

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 3.65 | 2739636 | 0.02 | 0.02 | includes sort | 1389 |
| B | 3.70 | 2699165 | 0.01 | 0.01 | includes sort | 2597 |
| C | 3.46 | 2888870 | 0.40 | 0.02 | includes sort | 4171 |
| D | 8.96 | 1115677 | 16.90 | 0.01 | existing records rewritten/edge 3.90, densified nodes 0 | 4171 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.26 | 38012095 | 123460 | 4b99dd115ab53ad0 |
| B btree-compressed | 0.34 | 29135431 | 23180 | 4b99dd115ab53ad0 |
| C node-blocks | 0.29 | 35036104 | 3999912 | 4b99dd115ab53ad0 |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.18 | 33702 |
| G csr-paged-compressed | 0.66 | 19362 |

### Space

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

### Warm: all pages resident


1-hop OUT, all types, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1076 [1076, 1078] | 107.7 | n/a | n/a | 10.0 | 4.06 | 4.06 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1326 [1321, 1341] | 132.7 | n/a | n/a | 10.0 | 3.01 | 3.01 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 451 [447, 452] | 45.2 | n/a | n/a | 10.0 | 2.00 | 2.00 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 3196 [3191, 3267] | 319.9 | n/a | n/a | 10.0 | 33.98 | 25.02 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 365 [365, 367] | 36.5 | n/a | n/a | 10.0 | 2.02 | 2.02 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 415 [415, 424] | 41.5 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 327 [325, 334] | 32.7 | n/a | n/a | 10.0 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1004 [966, 1077] | 100.5 | n/a | n/a | 10.0 | 4.06 | 4.06 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, type = 2, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1006 [1000, 1027] | 399.1 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1226 [1221, 1235] | 486.2 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 395 [392, 400] | 156.6 | n/a | n/a | 2.5 | 2.00 | 2.00 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 2855 [2845, 2883] | 1132.0 | n/a | n/a | 2.5 | 26.55 | 17.57 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 349 [346, 349] | 138.4 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 413 [409, 419] | 163.6 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 261 [260, 262] | 103.4 | n/a | n/a | 2.5 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 933 [890, 1004] | 369.8 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, all types, degree-weighted start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1098 [1088, 1102] | 99.5 | n/a | n/a | 11.0 | 4.07 | 4.07 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1349 [1338, 1358] | 122.3 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 454 [454, 462] | 41.2 | n/a | n/a | 11.0 | 2.00 | 2.00 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 3287 [3250, 3351] | 298.1 | n/a | n/a | 11.0 | 36.06 | 26.07 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 371 [371, 372] | 33.6 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 421 [420, 422] | 38.2 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 334 [332, 334] | 30.3 | n/a | n/a | 11.0 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1017 [985, 1121] | 92.2 | n/a | n/a | 11.0 | 4.07 | 4.07 | 0.00 | - | 0.0 | 60000 |

2-hop OUT (distinct), uniform start (1000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 11477 [10989, 11593] | 104.9 | n/a | n/a | 109.4 | 44.52 | 26.41 | 0.00 | - | 0.0 | 3000 |
| B btree-compressed | 14721 [14716, 15456] | 134.5 | n/a | n/a | 109.4 | 32.94 | 22.47 | 0.00 | - | 0.0 | 3000 |
| C node-blocks | 4650 [4418, 5706] | 42.5 | n/a | n/a | 109.4 | 21.87 | 21.86 | 0.00 | - | 0.0 | 3000 |
| D record-chains | 25924 [24583, 26853] | 236.9 | n/a | n/a | 109.4 | 242.79 | 134.78 | 0.00 | - | 0.0 | 3000 |
| F csr-paged | 4063 [3739, 4098] | 37.1 | n/a | n/a | 109.4 | 22.04 | 21.98 | 0.00 | - | 0.0 | 3000 |
| G csr-paged-compressed | 4194 [3755, 4197] | 38.3 | n/a | n/a | 109.4 | 21.97 | 21.90 | 0.00 | - | 0.0 | 3000 |
| E csr-memory | 2880 [2729, 2886] | 26.3 | n/a | n/a | 109.4 | - | - | - | - | - | 3000 |
| A btree-fixed(rep) | 10640 [10553, 11047] | 97.2 | n/a | n/a | 109.4 | 44.52 | 26.41 | 0.00 | - | 0.0 | 3000 |

2-hop OUT (distinct), degree-weighted start (200 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 11984 [11764, 12426] | 101.6 | n/a | n/a | 118.0 | 47.86 | 28.14 | 0.00 | - | 0.0 | 600 |
| B btree-compressed | 16837 [16792, 18091] | 142.7 | n/a | n/a | 118.0 | 35.46 | 24.08 | 0.00 | - | 0.0 | 600 |
| C node-blocks | 5731 [5557, 6247] | 48.6 | n/a | n/a | 118.0 | 23.53 | 23.50 | 0.00 | - | 0.0 | 600 |
| D record-chains | 32596 [30035, 32821] | 276.2 | n/a | n/a | 118.0 | 260.69 | 144.21 | 0.00 | - | 0.0 | 600 |
| F csr-paged | 4707 [4270, 4958] | 39.9 | n/a | n/a | 118.0 | 23.73 | 23.64 | 0.00 | - | 0.0 | 600 |
| G csr-paged-compressed | 4884 [4484, 5047] | 41.4 | n/a | n/a | 118.0 | 23.62 | 23.54 | 0.00 | - | 0.0 | 600 |
| E csr-memory | 3346 [3231, 3434] | 28.4 | n/a | n/a | 118.0 | - | - | - | - | - | 600 |
| A btree-fixed(rep) | 11120 [8024, 11169] | 94.2 | n/a | n/a | 118.0 | 47.86 | 28.14 | 0.00 | - | 0.0 | 600 |

3-hop OUT (distinct), uniform start (50 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 116637 [113316, 118112] | 106.7 | 122293 | 229710 | 1092.8 | 444.66 | 218.90 | 0.00 | - | 0.0 | 150 |
| B btree-compressed | 152188 [151317, 160727] | 139.3 | 155329 | 311914 | 1092.8 | 329.60 | 176.82 | 0.00 | - | 0.0 | 150 |
| C node-blocks | 52779 [49088, 54780] | 48.3 | 51543 | 121595 | 1092.8 | 218.60 | 216.58 | 0.00 | - | 0.0 | 150 |
| D record-chains | 252971 [249856, 263303] | 231.5 | 261348 | 544837 | 1092.8 | 2413.82 | 1313.66 | 0.00 | - | 0.0 | 150 |
| F csr-paged | 41663 [40961, 42530] | 38.1 | 41835 | 95683 | 1092.8 | 219.84 | 211.96 | 0.00 | - | 0.0 | 150 |
| G csr-paged-compressed | 43369 [38977, 45806] | 39.7 | 43371 | 100921 | 1092.8 | 219.46 | 211.26 | 0.00 | - | 0.0 | 150 |
| E csr-memory | 30739 [28977, 31458] | 28.1 | 30451 | 65791 | 1092.8 | - | - | - | - | - | 150 |
| A btree-fixed(rep) | 107807 [101021, 108270] | 98.6 | 107836 | 203239 | 1092.8 | 444.66 | 218.90 | 0.00 | - | 0.0 | 150 |

BFS OUT (unbounded depth), degree-weighted start (2 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 918358978 [915678756, 920012592] | 91.8 | 925177886 | 925177886 | 9999533.0 | 4068048.00 | 124015.00 | 0.00 | - | 0.0 | 6 |
| B btree-compressed | 1182136960 [1176633362, 1182211242] | 118.2 | 1183607834 | 1183607834 | 9999533.0 | 3012777.00 | 23283.00 | 0.00 | - | 0.0 | 6 |
| C node-blocks | 299498836 [285321384, 301940330] | 30.0 | 303875443 | 303875443 | 9999533.0 | 1999867.00 | 64832.00 | 0.00 | - | 0.0 | 6 |
| D record-chains | 2006412176 [2005709664, 2046519868] | 200.7 | 2010573706 | 2010573706 | 9999533.0 | 20999491.00 | 67048.00 | 0.00 | - | 0.0 | 6 |
| F csr-paged | 241106752 [241003629, 243257916] | 24.1 | 242369528 | 242369528 | 9999533.0 | 2015147.00 | 16851.00 | 0.00 | - | 0.0 | 6 |
| G csr-paged-compressed | 271411602 [270671558, 275770470] | 27.1 | 274192846 | 274192846 | 9999533.0 | 2009418.00 | 9681.00 | 0.00 | - | 0.0 | 6 |
| E csr-memory | 123660973 [121238961, 124790736] | 12.4 | 125989258 | 125989258 | 9999533.0 | - | - | - | - | - | 6 |
| A btree-fixed(rep) | 918712830 [917109570, 919816790] | 91.9 | 920118052 | 920118052 | 9999533.0 | 4068048.00 | 124015.00 | 0.00 | - | 0.0 | 6 |

## Constrained cache: 14425 frames = 112.7 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 9.1% of its 158432 pages
- B btree-compressed: cache = 25.0% of its 57700 pages
- C node-blocks: cache = 9.0% of its 160180 pages
- D record-chains: cache = 17.3% of its 83250 pages
- F csr-paged: cache = 42.8% of its 33702 pages
- G csr-paged-compressed: cache = 74.5% of its 19362 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 117196 [117196, 117196] | 11772.5 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.15 | 98.5 (io/miss 1.00) | 9.2 | 5000 |
| B btree-compressed | 91454 [91454, 91454] | 9186.7 | n/a | n/a | 10.0 | 3.01 | 3.01 | 0.92 | 95.4 (io/miss 1.00) | 7.4 | 5000 |
| C node-blocks | 146203 [146203, 146203] | 14686.4 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.51 | 94.8 (io/miss 1.00) | 12.1 | 5000 |
| D record-chains | 1478016 [1478016, 1478016] | 148469.7 | n/a | n/a | 10.0 | 33.91 | 24.99 | 10.57 | 137.6 (io/miss 1.00) | 84.6 | 5000 |
| F csr-paged | 104814 [104814, 104814] | 10528.8 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 96.9 (io/miss 1.00) | 8.5 | 5000 |
| G csr-paged-compressed | 80477 [80477, 80477] | 8084.0 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 82.4 (io/miss 1.00) | 7.6 | 5000 |

### Constrained 25%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 94088 [92357, 102546] | 9423.9 | n/a | n/a | 10.1 | 4.07 | 4.07 | 0.97 | 96.2 (io/miss 1.00) | 7.8 | 6000 |
| B btree-compressed | 55178 [38952, 77637] | 5526.6 | n/a | n/a | 10.1 | 3.01 | 3.01 | 0.58 | 93.9 (io/miss 1.00) | 4.7 | 6000 |
| C node-blocks | 79355 [75505, 106762] | 7948.2 | n/a | n/a | 10.1 | 2.00 | 2.00 | 0.93 | 91.4 (io/miss 1.00) | 7.5 | 6000 |
| D record-chains | 933011 [926205, 934414] | 92952.5 | n/a | n/a | 10.1 | 34.14 | 25.12 | 9.90 | 92.1 (io/miss 1.00) | 79.2 | 6000 |
| F csr-paged | 40351 [23516, 65859] | 4041.5 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.46 | 91.6 (io/miss 1.00) | 3.7 | 6000 |
| G csr-paged-compressed | 17949 [7206, 41938] | 1797.8 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.27 | 78.6 (io/miss 1.00) | 2.2 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 87576 [86417, 92800] | 34752.5 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.92 | 93.7 (io/miss 1.00) | 7.4 | 6000 |
| B btree-compressed | 48374 [37669, 68904] | 19044.7 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.54 | 91.5 (io/miss 1.00) | 4.3 | 6000 |
| C node-blocks | 78283 [77224, 94023] | 30820.1 | n/a | n/a | 2.5 | 2.00 | 2.00 | 0.88 | 92.0 (io/miss 1.00) | 7.1 | 6000 |
| D record-chains | 927018 [907726, 943662] | 364967.5 | n/a | n/a | 2.5 | 26.64 | 17.64 | 9.86 | 92.0 (io/miss 1.00) | 78.9 | 6000 |
| F csr-paged | 35730 [22252, 58807] | 14067.1 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.41 | 92.0 (io/miss 1.00) | 3.3 | 6000 |
| G csr-paged-compressed | 14164 [6405, 34570] | 5576.5 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.21 | 81.8 (io/miss 1.00) | 1.7 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 92352 [91161, 95428] | 8441.3 | n/a | n/a | 11.0 | 4.07 | 4.07 | 0.96 | 94.1 (io/miss 1.00) | 7.7 | 6000 |
| B btree-compressed | 48520 [36130, 62358] | 4434.9 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.50 | 93.1 (io/miss 1.00) | 4.0 | 6000 |
| C node-blocks | 79584 [78922, 87062] | 7213.7 | n/a | n/a | 11.0 | 2.00 | 2.00 | 0.87 | 91.6 (io/miss 1.00) | 7.0 | 6000 |
| D record-chains | 924446 [917068, 945578] | 83573.3 | n/a | n/a | 11.0 | 36.05 | 26.07 | 9.98 | 91.2 (io/miss 1.00) | 79.9 | 6000 |
| F csr-paged | 29364 [18501, 49253] | 2683.9 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.35 | 90.4 (io/miss 1.00) | 2.8 | 6000 |
| G csr-paged-compressed | 10206 [5142, 25168] | 932.9 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.17 | 77.6 (io/miss 1.00) | 1.3 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 988431 [987667, 1017411] | 9155.2 | n/a | n/a | 108.1 | 44.06 | 26.23 | 10.35 | 93.6 (io/miss 1.00) | 82.8 | 300 |
| B btree-compressed | 454833 [400052, 643832] | 4207.1 | n/a | n/a | 108.1 | 32.55 | 22.18 | 5.17 | 91.5 (io/miss 1.00) | 41.4 | 300 |
| C node-blocks | 882517 [835792, 883794] | 8158.3 | n/a | n/a | 108.1 | 21.63 | 21.63 | 9.12 | 93.1 (io/miss 1.00) | 73.0 | 300 |
| D record-chains | 9269148 [9079968, 9299446] | 85564.0 | n/a | n/a | 108.1 | 239.49 | 132.88 | 99.30 | 91.1 (io/miss 1.00) | 794.4 | 300 |
| F csr-paged | 314008 [201764, 498776] | 2904.5 | n/a | n/a | 108.1 | 21.81 | 21.78 | 3.65 | 89.6 (io/miss 1.00) | 29.2 | 300 |
| G csr-paged-compressed | 109260 [53247, 243012] | 1010.6 | n/a | n/a | 108.1 | 21.74 | 21.71 | 1.61 | 78.2 (io/miss 1.00) | 12.9 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1099516 [1090779, 1106112] | 9421.7 | n/a | n/a | 117.8 | 47.62 | 28.02 | 11.48 | 93.0 (io/miss 1.00) | 91.8 | 90 |
| B btree-compressed | 519621 [490772, 669514] | 4292.5 | n/a | n/a | 117.8 | 35.33 | 23.99 | 5.83 | 91.0 (io/miss 1.00) | 46.7 | 90 |
| C node-blocks | 896977 [896055, 923866] | 7678.3 | n/a | n/a | 117.8 | 23.44 | 23.44 | 9.83 | 90.1 (io/miss 1.00) | 78.7 | 90 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 325156 [217867, 524680] | 2658.0 | n/a | n/a | 117.8 | 23.71 | 23.67 | 3.90 | 88.5 (io/miss 1.00) | 31.2 | 90 |
| G csr-paged-compressed | 117688 [56251, 230606] | 962.0 | n/a | n/a | 117.8 | 23.54 | 23.50 | 1.64 | 77.1 (io/miss 1.00) | 13.2 | 90 |

## Constrained cache: 5770 frames = 45.1 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.6% of its 158432 pages
- B btree-compressed: cache = 10.0% of its 57700 pages
- C node-blocks: cache = 3.6% of its 160180 pages
- D record-chains: cache = 6.9% of its 83250 pages
- F csr-paged: cache = 17.1% of its 33702 pages
- G csr-paged-compressed: cache = 29.8% of its 19362 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 110816 [110816, 110816] | 11131.7 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.15 | 93.6 (io/miss 1.00) | 9.2 | 5000 |
| B btree-compressed | 86872 [86872, 86872] | 8726.5 | n/a | n/a | 10.0 | 3.01 | 3.01 | 0.92 | 91.3 (io/miss 1.00) | 7.4 | 5000 |
| C node-blocks | 142131 [142131, 142131] | 14277.4 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.53 | 91.2 (io/miss 1.00) | 12.2 | 5000 |
| D record-chains | 1389883 [1389883, 1389883] | 139616.6 | n/a | n/a | 10.0 | 33.91 | 24.99 | 15.00 | 91.0 (io/miss 1.00) | 120.0 | 5000 |
| F csr-paged | 99199 [99199, 99199] | 9964.7 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 92.0 (io/miss 1.00) | 8.5 | 5000 |
| G csr-paged-compressed | 78459 [78459, 78459] | 7881.4 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 80.7 (io/miss 1.00) | 7.6 | 5000 |

### Constrained 10%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 98227 [97664, 98568] | 9729.9 | n/a | n/a | 10.1 | 4.07 | 4.07 | 1.02 | 93.6 (io/miss 1.00) | 8.2 | 6000 |
| B btree-compressed | 73354 [71618, 73791] | 7308.0 | n/a | n/a | 10.1 | 3.01 | 3.01 | 0.77 | 91.4 (io/miss 1.00) | 6.2 | 6000 |
| C node-blocks | 123339 [118983, 124303] | 12262.9 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.31 | 91.7 (io/miss 1.00) | 10.5 | 6000 |
| D record-chains | 1407675 [1378702, 1409633] | 139065.0 | n/a | n/a | 10.1 | 34.14 | 25.12 | 15.12 | 90.8 (io/miss 1.00) | 121.0 | 6000 |
| F csr-paged | 64017 [63896, 65865] | 6411.9 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.70 | 90.1 (io/miss 1.00) | 5.6 | 6000 |
| G csr-paged-compressed | 36681 [36565, 41172] | 3674.0 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.48 | 77.8 (io/miss 1.00) | 3.8 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 92967 [91872, 93994] | 36558.1 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.98 | 92.5 (io/miss 1.00) | 7.8 | 6000 |
| B btree-compressed | 72477 [70395, 73223] | 28500.6 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.76 | 91.7 (io/miss 1.00) | 6.1 | 6000 |
| C node-blocks | 123201 [120961, 124028] | 48504.5 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.32 | 91.2 (io/miss 1.00) | 10.6 | 6000 |
| D record-chains | 1234754 [1218764, 1238019] | 485550.1 | n/a | n/a | 2.5 | 26.64 | 17.64 | 13.34 | 90.6 (io/miss 1.00) | 106.7 | 6000 |
| F csr-paged | 66520 [65023, 69382] | 26396.9 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.71 | 92.6 (io/miss 1.00) | 5.7 | 6000 |
| G csr-paged-compressed | 36129 [35782, 36415] | 14207.1 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.45 | 77.9 (io/miss 1.00) | 3.6 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 98504 [97727, 101795] | 8902.3 | n/a | n/a | 11.0 | 4.07 | 4.07 | 1.03 | 94.1 (io/miss 1.00) | 8.2 | 6000 |
| B btree-compressed | 72187 [72038, 75997] | 6523.9 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.76 | 92.6 (io/miss 1.00) | 6.1 | 6000 |
| C node-blocks | 121690 [121203, 123377] | 10997.8 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.32 | 91.0 (io/miss 1.00) | 10.5 | 6000 |
| D record-chains | 1400428 [1399470, 1400798] | 126597.2 | n/a | n/a | 11.0 | 36.05 | 26.07 | 15.19 | 90.5 (io/miss 1.00) | 121.6 | 6000 |
| F csr-paged | 64948 [64486, 65015] | 5869.6 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.70 | 90.1 (io/miss 1.00) | 5.6 | 6000 |
| G csr-paged-compressed | 35374 [34665, 37935] | 3196.9 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.44 | 79.0 (io/miss 1.00) | 3.5 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1091947 [1067285, 1095242] | 10100.3 | n/a | n/a | 108.1 | 44.06 | 26.23 | 11.13 | 94.7 (io/miss 1.00) | 89.1 | 300 |
| B btree-compressed | 784892 [767738, 831409] | 7275.6 | n/a | n/a | 108.1 | 32.55 | 22.18 | 8.18 | 93.3 (io/miss 1.00) | 65.4 | 300 |
| C node-blocks | 1337139 [1328838, 1360009] | 12343.2 | n/a | n/a | 108.1 | 21.63 | 21.63 | 14.41 | 91.3 (io/miss 1.00) | 115.3 | 300 |
| D record-chains | 10983734 [10950278, 10999887] | 101747.2 | n/a | n/a | 108.1 | 239.49 | 132.88 | 118.38 | 91.1 (io/miss 1.00) | 947.0 | 300 |
| F csr-paged | 710293 [697604, 738958] | 6570.1 | n/a | n/a | 108.1 | 21.81 | 21.78 | 7.66 | 91.2 (io/miss 1.00) | 61.3 | 300 |
| G csr-paged-compressed | 417252 [405335, 417565] | 3851.7 | n/a | n/a | 108.1 | 21.74 | 21.71 | 5.05 | 79.2 (io/miss 1.00) | 40.4 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1152070 [1141426, 1197884] | 9792.0 | n/a | n/a | 117.8 | 47.62 | 28.02 | 12.10 | 93.5 (io/miss 1.00) | 96.8 | 90 |
| B btree-compressed | 862782 [833719, 900437] | 7360.5 | n/a | n/a | 117.8 | 35.33 | 23.99 | 9.07 | 91.7 (io/miss 1.00) | 72.5 | 90 |
| C node-blocks | 1468111 [1411323, 1489762] | 12343.9 | n/a | n/a | 117.8 | 23.44 | 23.44 | 15.78 | 90.5 (io/miss 1.00) | 126.2 | 90 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 790752 [789926, 813888] | 6768.9 | n/a | n/a | 117.8 | 23.71 | 23.67 | 8.52 | 91.5 (io/miss 1.00) | 68.2 | 90 |
| G csr-paged-compressed | 427619 [390998, 433862] | 3495.5 | n/a | n/a | 117.8 | 23.54 | 23.50 | 5.27 | 76.6 (io/miss 1.00) | 42.1 | 90 |

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5946 [5946, 5946] | 597.3 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.15 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 4697 [4697, 4697] | 471.9 | n/a | n/a | 10.0 | 3.01 | 3.01 | 0.92 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7079 [7079, 7079] | 711.1 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.53 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 68065 [68065, 68065] | 6837.3 | n/a | n/a | 10.0 | 33.91 | 24.99 | 15.00 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4828 [4828, 4828] | 485.0 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 2.9 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4373 [4373, 4373] | 439.3 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 2.9 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5569 [5568, 5640] | 556.4 | n/a | n/a | 10.1 | 4.07 | 4.07 | 1.02 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4337 [4282, 4365] | 430.7 | n/a | n/a | 10.1 | 3.01 | 3.01 | 0.77 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6207 [6203, 6297] | 621.2 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.31 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 68524 [67959, 68711] | 6778.6 | n/a | n/a | 10.1 | 34.14 | 25.12 | 15.12 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3470 [3388, 3493] | 342.3 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.70 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2492 [2375, 2629] | 245.8 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.48 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5411 [5317, 5441] | 2127.6 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.98 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4192 [4110, 4209] | 1655.0 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.76 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6350 [6261, 6379] | 2508.3 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.33 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 60156 [59243, 60965] | 23683.6 | n/a | n/a | 2.5 | 26.64 | 17.64 | 13.34 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3449 [3425, 3500] | 1357.8 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.71 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2438 [2405, 2465] | 958.9 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.46 | 3.1 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5645 [5612, 5656] | 510.2 | n/a | n/a | 11.0 | 4.07 | 4.07 | 1.03 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4338 [4299, 4339] | 392.1 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.76 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6258 [6212, 6354] | 572.0 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.32 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 68797 [68681, 69817] | 6277.7 | n/a | n/a | 11.0 | 36.05 | 26.07 | 15.20 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3461 [3458, 3595] | 312.9 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.70 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2381 [2374, 2465] | 217.0 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.45 | 3.1 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 63375 [62546, 64169] | 587.5 | n/a | n/a | 108.1 | 44.06 | 26.23 | 11.13 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 50515 [49804, 51051] | 467.3 | n/a | n/a | 108.1 | 32.55 | 22.18 | 8.18 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 69900 [68150, 70419] | 647.9 | n/a | n/a | 108.1 | 21.63 | 21.63 | 14.37 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 543535 [533962, 548391] | 5038.3 | n/a | n/a | 108.1 | 239.49 | 132.88 | 118.38 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 39019 [37997, 40964] | 361.7 | n/a | n/a | 108.1 | 21.81 | 21.78 | 7.67 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 28311 [27604, 29357] | 261.9 | n/a | n/a | 108.1 | 21.74 | 21.71 | 5.05 | 3.1 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 68918 [67700, 70771] | 592.1 | n/a | n/a | 117.8 | 47.62 | 28.02 | 12.10 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 59321 [50933, 59810] | 484.9 | n/a | n/a | 117.8 | 35.33 | 23.99 | 9.11 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 77317 [75478, 79908] | 660.2 | n/a | n/a | 117.8 | 23.44 | 23.44 | 15.76 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 596578 [594296, 608940] | 5112.1 | n/a | n/a | 117.8 | 261.19 | 144.87 | 130.56 | 3.0 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 43465 [43248, 44885] | 370.6 | n/a | n/a | 117.8 | 23.71 | 23.67 | 8.50 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 30521 [28600, 30786] | 251.7 | n/a | n/a | 117.8 | 23.54 | 23.50 | 5.34 | 3.1 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 666327 [586728, 696743] | 580.5 | 632628 | 1076822 | 1099.2 | 447.60 | 220.30 | 112.43 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 558882 [462507, 576831] | 471.8 | 561319 | 931551 | 1099.2 | 331.53 | 178.00 | 84.93 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 706577 [647567, 777668] | 640.6 | 688642 | 1136467 | 1099.2 | 220.07 | 218.00 | 145.10 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 384508 [359008, 435702] | 356.3 | 393489 | 638564 | 1099.2 | 221.77 | 214.77 | 79.13 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 265923 [260580, 305188] | 249.9 | 279507 | 462493 | 1099.2 | 221.00 | 213.57 | 50.77 | 3.1 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5299420456 [5277066740, 5309677573] | 530.0 | 5299420456 | 5299420456 | 9999533.0 | 4068048.00 | 124015.00 | 1027703.33 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 4135899232 [4131706375, 4157453643] | 413.6 | 4135899232 | 4135899232 | 9999533.0 | 3012777.00 | 23283.00 | 769923.33 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 6081648657 [6070066664, 6088728931] | 608.2 | 6081648657 | 6081648657 | 9999533.0 | 1999867.00 | 64832.00 | 1325999.00 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 3299744010 [3298679326, 3304615425] | 330.0 | 3299744010 | 3299744010 | 9999533.0 | 2015147.00 | 16851.00 | 711277.00 | 2.9 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 2264820463 [2264753186, 2265665121] | 226.5 | 2264820463 | 2264820463 | 9999533.0 | 2009418.00 | 9681.00 | 456297.67 | 2.9 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 577 frames = 4.5 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 158432 pages
- B btree-compressed: cache = 1.0% of its 57700 pages
- C node-blocks: cache = 0.4% of its 160180 pages
- D record-chains: cache = 0.7% of its 83250 pages
- F csr-paged: cache = 1.7% of its 33702 pages
- G csr-paged-compressed: cache = 3.0% of its 19362 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 158071 [158071, 158071] | 15878.6 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.64 | 94.1 (io/miss 1.00) | 13.1 | 5000 |
| B btree-compressed | 99432 [99432, 99432] | 9988.2 | n/a | n/a | 10.0 | 3.01 | 3.01 | 1.01 | 95.2 (io/miss 1.00) | 8.1 | 5000 |
| C node-blocks | 182286 [182286, 182286] | 18311.0 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.92 | 93.3 (io/miss 1.00) | 15.4 | 5000 |
| D record-chains | 2046027 [2046027, 2046027] | 205527.6 | n/a | n/a | 10.0 | 33.91 | 24.99 | 22.05 | 91.2 (io/miss 1.00) | 176.4 | 5000 |
| F csr-paged | 165763 [165763, 165763] | 16651.2 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.73 | 94.0 (io/miss 1.00) | 13.9 | 5000 |
| G csr-paged-compressed | 158785 [158785, 158785] | 15950.2 | n/a | n/a | 10.0 | 2.01 | 2.01 | 1.71 | 91.4 (io/miss 1.00) | 13.7 | 5000 |

### Constrained 1%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 162509 [154700, 162905] | 16071.1 | n/a | n/a | 10.1 | 4.07 | 4.07 | 1.62 | 95.9 (io/miss 1.00) | 13.0 | 6000 |
| B btree-compressed | 99273 [94498, 99818] | 9793.6 | n/a | n/a | 10.1 | 3.01 | 3.01 | 1.00 | 94.8 (io/miss 1.00) | 8.0 | 6000 |
| C node-blocks | 185766 [180028, 185867] | 18326.4 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.92 | 93.9 (io/miss 1.00) | 15.3 | 6000 |
| D record-chains | 2132248 [2051809, 2145084] | 211619.8 | n/a | n/a | 10.1 | 34.14 | 25.12 | 22.15 | 93.4 (io/miss 1.00) | 177.2 | 6000 |
| F csr-paged | 167712 [162089, 168762] | 16545.3 | n/a | n/a | 10.1 | 2.02 | 2.02 | 1.72 | 94.7 (io/miss 1.00) | 13.8 | 6000 |
| G csr-paged-compressed | 164409 [157500, 166186] | 16394.8 | n/a | n/a | 10.1 | 2.01 | 2.01 | 1.69 | 94.1 (io/miss 1.00) | 13.6 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 155814 [150728, 157534] | 61344.3 | n/a | n/a | 2.5 | 4.02 | 4.02 | 1.58 | 95.5 (io/miss 1.00) | 12.6 | 6000 |
| B btree-compressed | 97052 [93605, 98408] | 38209.6 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.98 | 94.5 (io/miss 1.00) | 7.9 | 6000 |
| C node-blocks | 186622 [178043, 187512] | 73473.2 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.92 | 93.8 (io/miss 1.00) | 15.4 | 6000 |
| D record-chains | 1481045 [1450337, 1494625] | 587716.4 | n/a | n/a | 2.5 | 26.64 | 17.64 | 15.40 | 93.8 (io/miss 1.00) | 123.2 | 6000 |
| F csr-paged | 168738 [168573, 169896] | 66367.3 | n/a | n/a | 2.5 | 2.02 | 2.02 | 1.72 | 96.2 (io/miss 1.00) | 13.8 | 6000 |
| G csr-paged-compressed | 166106 [165849, 167090] | 65783.3 | n/a | n/a | 2.5 | 2.01 | 2.01 | 1.70 | 95.9 (io/miss 1.00) | 13.6 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 162720 [161851, 167105] | 14793.8 | n/a | n/a | 11.0 | 4.07 | 4.07 | 1.63 | 98.0 (io/miss 1.00) | 13.0 | 6000 |
| B btree-compressed | 100134 [98937, 100170] | 9052.5 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.99 | 96.5 (io/miss 1.00) | 8.0 | 6000 |
| C node-blocks | 186685 [185933, 188244] | 17017.9 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.92 | 95.1 (io/miss 1.00) | 15.4 | 6000 |
| D record-chains | 2221316 [2213622, 2226914] | 201321.2 | n/a | n/a | 11.0 | 36.05 | 26.07 | 22.99 | 94.6 (io/miss 1.00) | 183.9 | 6000 |
| F csr-paged | 168718 [168021, 171115] | 15421.4 | n/a | n/a | 11.0 | 2.02 | 2.02 | 1.73 | 95.9 (io/miss 1.00) | 13.8 | 6000 |
| G csr-paged-compressed | 166157 [165968, 168261] | 15187.4 | n/a | n/a | 11.0 | 2.01 | 2.01 | 1.70 | 95.9 (io/miss 1.00) | 13.6 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1774551 [1763898, 1785437] | 16449.3 | n/a | n/a | 108.1 | 44.06 | 26.23 | 17.61 | 97.9 (io/miss 1.00) | 140.9 | 300 |
| B btree-compressed | 1082826 [1065888, 1089550] | 10016.0 | n/a | n/a | 108.1 | 32.55 | 22.18 | 10.71 | 96.6 (io/miss 1.00) | 85.7 | 300 |
| C node-blocks | 2033479 [2008011, 2048706] | 18849.5 | n/a | n/a | 108.1 | 21.63 | 21.63 | 20.73 | 95.8 (io/miss 1.00) | 165.8 | 300 |
| D record-chains | 12663680 [12418482, 12719623] | 116899.1 | n/a | n/a | 108.1 | 239.49 | 132.88 | 129.98 | 94.8 (io/miss 1.00) | 1039.8 | 300 |
| F csr-paged | 1828943 [1819850, 1855309] | 16953.5 | n/a | n/a | 108.1 | 21.81 | 21.78 | 18.63 | 96.3 (io/miss 1.00) | 149.0 | 300 |
| G csr-paged-compressed | 1804112 [1794051, 1832507] | 16687.7 | n/a | n/a | 108.1 | 21.74 | 21.71 | 18.38 | 96.2 (io/miss 1.00) | 147.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1876668 [1868366, 1978607] | 16173.9 | n/a | n/a | 117.8 | 47.62 | 28.02 | 19.10 | 97.1 (io/miss 1.00) | 152.8 | 90 |
| B btree-compressed | 1163117 [1146403, 1195384] | 9966.7 | n/a | n/a | 117.8 | 35.33 | 23.99 | 11.63 | 96.3 (io/miss 1.00) | 93.1 | 90 |
| C node-blocks | 2179416 [2179169, 2230790] | 18675.4 | n/a | n/a | 117.8 | 23.44 | 23.44 | 22.53 | 95.4 (io/miss 1.00) | 180.3 | 90 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 2008688 [1981377, 2029477] | 17212.4 | n/a | n/a | 117.8 | 23.71 | 23.67 | 20.24 | 96.9 (io/miss 1.00) | 162.0 | 90 |
| G csr-paged-compressed | 1936727 [1934450, 1984413] | 16576.3 | n/a | n/a | 117.8 | 23.54 | 23.50 | 19.84 | 96.2 (io/miss 1.00) | 158.8 | 90 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2394 [2324, 2648] | 11.02 | 3.01 |  |
| B btree-compressed | 3921 [3893, 3958] | 9.02 | 3.01 |  |
| C node-blocks | 827 [819, 883] | 9.33 | 5.00 |  |
| D record-chains | 1587 [1478, 1703] | 20.01 | 6.00 | existing records rewritten/insert 4.00 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2463 [2366, 2615] | 13.00 | 2.00 |
| B btree-compressed | 4629 [4603, 4887] | 9.00 | 2.00 |
| C node-blocks | 1346 [1344, 1388] | 9.00 | 2.00 |
| D record-chains | 817 [814, 818] | 4.00 | 1.00 |

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
| A btree-fixed | 124015 | 101.6 | 34933 | 130.2 | leaves 123457, height 4, entries 20300000, leaf fill 0.91; edge table 34933 pages |
| B btree-compressed | 23283 | 19.1 | 34933 | 47.7 | leaves 23178, height 3, entries 20300000, leaf bytes used 17.6 B/edge (8.6 B/entry), leaf fill 0.92; edge table 34933 pages |
| C node-blocks | 128633 | 105.4 | 34934 | 134.0 | dir 3922, block slabs 124711, chain 0; entry bytes 73.1 B/edge, block bytes reserved 99.7 B/edge; relocations 46486, entries copied 399504; edge table 34933 pages |
| D record-chains | 68007 | 55.7 | 16426 | 69.2 | node recs 2942, rel recs 65065, groups 0; node map 1475 pages, edge map 14950 pages; densified 0 |
| F csr-paged | 33702 | 27.6 | 0 | 27.6 | offset pages 1954, entry pages 31748, entry stream 26.0 B/edge; read-only snapshot, no stamps |
| G csr-paged-compressed | 19362 | 15.9 | 0 | 15.9 | offset pages 1954, entry pages 17408, entry stream 14.3 B/edge; read-only snapshot, no stamps |
| E csr-memory | - | 27.6 | - | 27.6 | in-memory arrays, 276000016 bytes |

RSS end: 4738 MiB, peak 4738 MiB
