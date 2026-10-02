# layout-bench zipf online n=1000000 m=10000000 alpha=0.8 scale=1
clock: Instant::now() pair costs 2789 ns (checksum 5)
graph generated in 2.1s: max out-degree 133939, max in-degree 131746, nodes with out-degree 0: 38495, nodes with total degree > 50: 45809, out-degree p50 4 p90 14 p99 85 p99.9 535

### Build (online inserts in random order, created_ts = edge id + 1)

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 20.25 | 493922 | 10.63 | 3.04 |  | 1636 |
| B | 39.20 | 255085 | 9.22 | 3.01 |  | 2210 |
| C | 5.77 | 1732281 | 9.88 | 4.99 | relocations/edge 0.469, entries copied/edge 2.06 | 3448 |
| D | 14.14 | 707422 | 24.93 | 6.90 | existing records rewritten/edge 5.16, densified nodes 45809 | 4106 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.31 | 32332126 | 158832 | a7c193c4f2af92bf |
| B btree-compressed | 0.37 | 26767335 | 38270 | a7c193c4f2af92bf |
| C node-blocks | 0.67 | 15004968 | 3945261 | a7c193c4f2af92bf |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.31 | 33702 |
| G csr-paged-compressed | 0.67 | 18369 |

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

### Warm: all pages resident


1-hop OUT, all types, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1143 [1138, 1170] | 125.0 | n/a | n/a | 9.1 | 4.08 | 4.08 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1443 [1432, 1465] | 157.7 | n/a | n/a | 9.1 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 437 [434, 447] | 47.8 | n/a | n/a | 9.1 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 3373 [3361, 3383] | 368.8 | n/a | n/a | 9.1 | 27.34 | 26.58 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 323 [316, 328] | 35.3 | n/a | n/a | 9.1 | 1.98 | 1.98 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 398 [390, 403] | 43.6 | n/a | n/a | 9.1 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 269 [259, 270] | 29.4 | n/a | n/a | 9.1 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1091 [1001, 1143] | 119.2 | n/a | n/a | 9.1 | 4.08 | 4.08 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, type = 2, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1090 [1082, 1190] | 433.7 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1335 [1333, 1442] | 531.3 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 418 [407, 419] | 166.4 | n/a | n/a | 2.5 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 2643 [2629, 2655] | 1052.0 | n/a | n/a | 2.5 | 17.64 | 17.41 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 300 [292, 304] | 119.4 | n/a | n/a | 2.5 | 1.98 | 1.98 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 398 [391, 401] | 158.3 | n/a | n/a | 2.5 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 252 [242, 252] | 100.1 | n/a | n/a | 2.5 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1057 [927, 1178] | 420.5 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, all types, degree-weighted start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 23411 [23277, 23853] | 5.9 | n/a | n/a | 3994.8 | 35.51 | 35.51 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 39799 [39745, 40472] | 10.0 | n/a | n/a | 3994.8 | 10.52 | 10.52 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 11441 [11440, 12076] | 2.9 | n/a | n/a | 3994.8 | 19.44 | 19.44 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 560665 [558685, 580552] | 143.3 | n/a | n/a | 3931.8 | 7873.55 | 2913.44 | 0.00 | - | 0.0 | 21264 |
| F csr-paged | 7143 [7136, 7236] | 1.8 | n/a | n/a | 3994.8 | 8.33 | 8.33 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 26234 [26179, 26268] | 6.6 | n/a | n/a | 3994.8 | 4.54 | 4.54 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 5605 [5591, 5624] | 1.4 | n/a | n/a | 3994.8 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 23368 [23285, 23710] | 5.8 | n/a | n/a | 3994.8 | 35.51 | 35.51 | 0.00 | - | 0.0 | 60000 |

2-hop OUT (distinct), uniform start (1000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 209823 [207200, 219993] | 7.9 | n/a | n/a | 26437.8 | 246.65 | 231.83 | 0.00 | - | 0.0 | 3000 |
| B btree-compressed | 335758 [329565, 353930] | 12.7 | n/a | n/a | 26437.8 | 81.31 | 63.97 | 0.00 | - | 0.0 | 3000 |
| C node-blocks | 172007 [169148, 183416] | 6.5 | n/a | n/a | 26437.8 | 133.79 | 133.69 | 0.00 | - | 0.0 | 3000 |
| D record-chains | 3052716 [3035827, 3486017] | 115.5 | n/a | n/a | 26437.8 | 26510.60 | 11896.43 | 0.00 | - | 0.0 | 3000 |
| F csr-paged | 129015 [128412, 137972] | 4.9 | n/a | n/a | 26437.8 | 60.70 | 60.26 | 0.00 | - | 0.0 | 3000 |
| G csr-paged-compressed | 215506 [213117, 253914] | 8.2 | n/a | n/a | 26437.8 | 35.72 | 35.27 | 0.00 | - | 0.0 | 3000 |
| E csr-memory | 91462 [90503, 94299] | 3.5 | n/a | n/a | 26437.8 | - | - | - | - | - | 3000 |
| A btree-fixed(rep) | 215208 [203674, 216130] | 8.1 | n/a | n/a | 26437.8 | 246.65 | 231.83 | 0.00 | - | 0.0 | 3000 |

2-hop OUT (distinct), degree-weighted start (200 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7999913 [7762851, 8067078] | 13.1 | n/a | n/a | 612551.1 | 21221.85 | 8263.88 | 0.00 | - | 0.0 | 600 |
| B btree-compressed | 12842191 [12719496, 13375442] | 21.0 | n/a | n/a | 612551.1 | 17402.47 | 3048.72 | 0.00 | - | 0.0 | 600 |
| C node-blocks | 4581688 [4445074, 4712230] | 7.5 | n/a | n/a | 612551.1 | 10270.20 | 5887.69 | 0.00 | - | 0.0 | 600 |
| D record-chains | 156608309 [156516520, 156773644] | 149.8 | n/a | n/a | 1045782.5 | 1141662.56 | 47082.25 | 0.00 | - | 0.0 | 96 |
| F csr-paged | 3596517 [3566640, 3616112] | 5.9 | n/a | n/a | 612551.1 | 9072.85 | 2283.74 | 0.00 | - | 0.0 | 600 |
| G csr-paged-compressed | 6218281 [6180120, 6355798] | 10.2 | n/a | n/a | 612551.1 | 8539.80 | 1460.94 | 0.00 | - | 0.0 | 600 |
| E csr-memory | 2365723 [2352886, 2392452] | 3.9 | n/a | n/a | 612551.1 | - | - | - | - | - | 600 |
| A btree-fixed(rep) | 8025252 [7771297, 8218466] | 13.1 | n/a | n/a | 612551.1 | 21221.85 | 8263.88 | 0.00 | - | 0.0 | 600 |

3-hop OUT (distinct), uniform start (50 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 37274855 [37192260, 38316416] | 17.5 | 14615515 | 132712409 | 2132064.1 | 129463.54 | 37582.44 | 0.00 | - | 0.0 | 150 |
| B btree-compressed | 58809113 [58454848, 58871695] | 27.6 | 24620980 | 197427409 | 2132064.1 | 116184.50 | 13973.62 | 0.00 | - | 0.0 | 150 |
| C node-blocks | 18521191 [18071879, 18823141] | 8.7 | 9181823 | 57681505 | 2132064.1 | 61802.56 | 27689.68 | 0.00 | - | 0.0 | 150 |
| D record-chains | 427409202 [427065159, 431070732] | 156.5 | 528969397 | 838641743 | 2730877.9 | 3044582.40 | 60885.20 | 0.00 | - | 0.0 | 30 |
| F csr-paged | 13537164 [13424318, 13610102] | 6.3 | 8019585 | 39453423 | 2132064.1 | 59074.28 | 8188.36 | 0.00 | - | 0.0 | 150 |
| G csr-paged-compressed | 23488881 [23444999, 23580181] | 11.0 | 14415349 | 65324015 | 2132064.1 | 57316.12 | 4794.22 | 0.00 | - | 0.0 | 150 |
| E csr-memory | 8720154 [8658869, 8742149] | 4.1 | 5270891 | 24791815 | 2132064.1 | - | - | - | - | - | 150 |
| A btree-fixed(rep) | 37455952 [37204292, 38248807] | 17.6 | 15041688 | 131084542 | 2132064.1 | 129463.54 | 37582.44 | 0.00 | - | 0.0 | 150 |

BFS OUT (unbounded depth), degree-weighted start (2 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 875589782 [875140628, 928952858] | 88.5 | 878055093 | 878055093 | 9894697.0 | 3930660.00 | 140243.00 | 0.00 | - | 0.0 | 6 |
| B btree-compressed | 1213879032 [1203992310, 1219836542] | 122.7 | 1217143927 | 1217143927 | 9894697.0 | 3865082.00 | 35948.00 | 0.00 | - | 0.0 | 6 |
| C node-blocks | 247524878 [246094191, 256528720] | 25.0 | 249127677 | 249127677 | 9894697.0 | 1897490.00 | 101486.00 | 0.00 | - | 0.0 | 6 |
| D record-chains | 2367390462 [2367390385, 2383140206] | 239.3 | 2368059160 | 2368059160 | 9894697.0 | 15900512.00 | 67498.00 | 0.00 | - | 0.0 | 6 |
| F csr-paged | 169124446 [168798592, 180133909] | 17.1 | 170202866 | 170202866 | 9894697.0 | 1901817.00 | 16851.00 | 0.00 | - | 0.0 | 6 |
| G csr-paged-compressed | 234326756 [231881604, 257131351] | 23.7 | 236404864 | 236404864 | 9894697.0 | 1895619.00 | 9184.00 | 0.00 | - | 0.0 | 6 |
| E csr-memory | 95577010 [95024242, 98397492] | 9.7 | 97547771 | 97547771 | 9894697.0 | - | - | - | - | - | 6 |
| A btree-fixed(rep) | 878942785 [876533871, 941792646] | 88.8 | 884823535 | 884823535 | 9894697.0 | 3930660.00 | 140243.00 | 0.00 | - | 0.0 | 6 |

## Constrained cache: 18248 frames = 142.6 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 9.4% of its 194323 pages
- B btree-compressed: cache = 25.0% of its 72992 pages
- C node-blocks: cache = 11.6% of its 157292 pages
- D record-chains: cache = 21.8% of its 83720 pages
- F csr-paged: cache = 54.1% of its 33702 pages
- G csr-paged-compressed: cache = 99.3% of its 18369 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 120196 [120196, 120196] | 13130.1 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.24 | 93.8 (io/miss 1.00) | 9.9 | 5000 |
| B btree-compressed | 94190 [94190, 94190] | 10289.3 | n/a | n/a | 9.2 | 4.02 | 4.02 | 0.97 | 92.6 (io/miss 1.00) | 7.8 | 5000 |
| C node-blocks | 142473 [142473, 142473] | 15563.7 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.47 | 95.0 (io/miss 1.00) | 11.7 | 5000 |
| D record-chains | 1203846 [1203846, 1203846] | 131507.5 | n/a | n/a | 9.2 | 27.40 | 26.85 | 12.41 | 94.8 (io/miss 1.00) | 99.3 | 5000 |
| F csr-paged | 96872 [96872, 96872] | 10582.3 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 95.5 (io/miss 1.00) | 7.9 | 5000 |
| G csr-paged-compressed | 75342 [75342, 75342] | 8230.4 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 82.8 (io/miss 1.00) | 7.1 | 5000 |

### Constrained 25%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 97676 [96572, 100560] | 10349.4 | n/a | n/a | 9.4 | 4.09 | 4.09 | 1.01 | 94.2 (io/miss 1.00) | 8.1 | 6000 |
| B btree-compressed | 55433 [53547, 82225] | 6079.0 | n/a | n/a | 9.4 | 4.02 | 4.02 | 0.63 | 95.5 (io/miss 1.00) | 5.1 | 6000 |
| C node-blocks | 89961 [85504, 107004] | 10212.9 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.95 | 96.3 (io/miss 1.00) | 7.6 | 6000 |
| D record-chains | 1222280 [1161247, 1295969] | 132201.2 | n/a | n/a | 9.4 | 27.97 | 27.31 | 12.62 | 95.0 (io/miss 1.00) | 100.9 | 6000 |
| F csr-paged | 1220 [462, 60124] | 138.5 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.21 | 95.7 (io/miss 1.00) | 1.7 | 6000 |
| G csr-paged-compressed | 553 [548, 37683] | 62.2 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.15 | 81.9 (io/miss 1.00) | 1.2 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 90782 [90217, 92139] | 42062.9 | n/a | n/a | 2.2 | 4.03 | 4.03 | 0.93 | 95.0 (io/miss 1.00) | 7.4 | 6000 |
| B btree-compressed | 53108 [52924, 75106] | 25873.1 | n/a | n/a | 2.2 | 4.00 | 4.00 | 0.60 | 95.4 (io/miss 1.00) | 4.8 | 6000 |
| C node-blocks | 84077 [81503, 92640] | 39844.9 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.88 | 95.8 (io/miss 1.00) | 7.0 | 6000 |
| D record-chains | 851780 [822101, 855953] | 388851.8 | n/a | n/a | 2.2 | 16.94 | 16.77 | 8.68 | 94.9 (io/miss 1.00) | 69.4 | 6000 |
| F csr-paged | 1029 [416, 52419] | 453.2 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.18 | 95.9 (io/miss 1.00) | 1.4 | 6000 |
| G csr-paged-compressed | 538 [533, 27462] | 260.6 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.11 | 80.7 (io/miss 1.00) | 0.9 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 527611 [390495, 582823] | 151.4 | n/a | n/a | 3943.8 | 35.13 | 35.13 | 4.85 | 95.4 (io/miss 1.00) | 38.8 | 6000 |
| B btree-compressed | 95626 [83935, 214677] | 24.2 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 0.91 | 93.4 (io/miss 1.00) | 7.3 | 6000 |
| C node-blocks | 295468 [205897, 356618] | 85.3 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 2.73 | 98.3 (io/miss 1.00) | 21.8 | 6000 |
| D record-chains | 659838553 [201999294, 750639322] | 75632.1 | n/a | n/a | 5321.7 | 10653.71 | 3342.44 | 4167.45 | 94.7 (io/miss 1.00) | 33339.6 | 80 |
| F csr-paged | 10662 [8350, 117094] | 2.4 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 0.46 | 77.1 (io/miss 1.00) | 3.7 | 6000 |
| G csr-paged-compressed | 28751 [22386, 76357] | 6.5 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.20 | 78.9 (io/miss 1.00) | 1.6 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 3150834 [3133105, 3716643] | 98.0 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 30.92 | 94.8 (io/miss 1.00) | 247.4 | 300 |
| B btree-compressed | 1291524 [1010839, 1619364] | 33.6 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 8.59 | 92.9 (io/miss 1.00) | 68.7 | 300 |
| C node-blocks | 2199260 [2096249, 2479426] | 65.6 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 19.36 | 100.1 (io/miss 1.00) | 154.9 | 300 |
| D record-chains | 3785610498 [3022902774, 4398065134] | 70491.3 | n/a | n/a | 52969.0 | 53144.10 | 17225.96 | 38641.38 | 94.6 (io/miss 1.00) | 309131.0 | 48 |
| F csr-paged | 265379 [203857, 910930] | 6.9 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 2.60 | 93.7 (io/miss 1.00) | 20.8 | 300 |
| G csr-paged-compressed | 317226 [260104, 575130] | 8.3 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 1.18 | 85.0 (io/miss 1.00) | 9.4 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 631667024 [391704240, 847782934] | 937.5 | n/a | n/a | 638150.4 | 20390.85 | 8432.31 | 6313.56 | 95.0 (io/miss 1.00) | 50508.5 | 48 |
| B btree-compressed | 133590660 [115606241, 359742254] | 267.8 | n/a | n/a | 545308.6 | 11814.00 | 2566.54 | 1633.42 | 95.3 (io/miss 1.00) | 13067.4 | 76 |
| C node-blocks | 433526482 [236674397, 610307999] | 643.4 | n/a | n/a | 600907.1 | 8702.77 | 5488.82 | 3745.71 | 98.7 (io/miss 1.00) | 29965.7 | 62 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 4544458 [3122355, 21616006] | 6.3 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 67.28 | 89.3 (io/miss 1.00) | 538.2 | 90 |
| G csr-paged-compressed | 7692855 [4853850, 9698746] | 10.1 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 18.29 | 85.8 (io/miss 1.00) | 146.3 | 90 |

## Constrained cache: 7299 frames = 57.0 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.8% of its 194323 pages
- B btree-compressed: cache = 10.0% of its 72992 pages
- C node-blocks: cache = 4.6% of its 157292 pages
- D record-chains: cache = 8.7% of its 83720 pages
- F csr-paged: cache = 21.7% of its 33702 pages
- G csr-paged-compressed: cache = 39.7% of its 18369 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 121191 [121191, 121191] | 13238.8 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.24 | 94.6 (io/miss 1.00) | 9.9 | 5000 |
| B btree-compressed | 96532 [96532, 96532] | 10545.1 | n/a | n/a | 9.2 | 4.02 | 4.02 | 0.97 | 95.1 (io/miss 1.00) | 7.8 | 5000 |
| C node-blocks | 143623 [143623, 143623] | 15689.3 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.47 | 95.8 (io/miss 1.00) | 11.7 | 5000 |
| D record-chains | 1603011 [1603011, 1603011] | 175112.1 | n/a | n/a | 9.2 | 27.40 | 26.85 | 16.55 | 94.8 (io/miss 1.00) | 132.4 | 5000 |
| F csr-paged | 97642 [97642, 97642] | 10666.4 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 96.3 (io/miss 1.00) | 7.9 | 5000 |
| G csr-paged-compressed | 75128 [75128, 75128] | 8206.9 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 82.6 (io/miss 1.00) | 7.1 | 5000 |

### Constrained 10%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 103267 [101901, 108167] | 10534.2 | n/a | n/a | 9.4 | 4.09 | 4.09 | 1.06 | 95.1 (io/miss 1.00) | 8.5 | 6000 |
| B btree-compressed | 81429 [79746, 82801] | 8380.5 | n/a | n/a | 9.4 | 4.02 | 4.02 | 0.82 | 94.9 (io/miss 1.00) | 6.6 | 6000 |
| C node-blocks | 122890 [113216, 138568] | 12535.9 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.27 | 95.8 (io/miss 1.00) | 10.2 | 6000 |
| D record-chains | 1644079 [1599793, 1694319] | 172836.8 | n/a | n/a | 9.4 | 27.97 | 27.31 | 16.95 | 95.0 (io/miss 1.00) | 135.6 | 6000 |
| F csr-paged | 60713 [59818, 64926] | 6623.1 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.62 | 96.4 (io/miss 1.00) | 5.0 | 6000 |
| G csr-paged-compressed | 19470 [17813, 36167] | 2022.3 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.29 | 79.4 (io/miss 1.00) | 2.4 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 95300 [93957, 98623] | 43436.6 | n/a | n/a | 2.2 | 4.03 | 4.03 | 0.98 | 94.8 (io/miss 1.00) | 7.8 | 6000 |
| B btree-compressed | 77312 [75700, 80900] | 35631.1 | n/a | n/a | 2.2 | 4.00 | 4.00 | 0.78 | 95.3 (io/miss 1.00) | 6.3 | 6000 |
| C node-blocks | 113756 [108919, 128632] | 55612.7 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.19 | 95.8 (io/miss 1.00) | 9.6 | 6000 |
| D record-chains | 1164396 [1121073, 1171333] | 531566.1 | n/a | n/a | 2.2 | 16.94 | 16.77 | 11.87 | 94.9 (io/miss 1.00) | 95.0 | 6000 |
| F csr-paged | 55613 [52567, 61322] | 24493.9 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.57 | 95.9 (io/miss 1.00) | 4.6 | 6000 |
| G csr-paged-compressed | 17920 [17479, 27514] | 8760.7 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.25 | 80.6 (io/miss 1.00) | 2.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 714993 [705857, 739058] | 191.9 | n/a | n/a | 3943.8 | 35.13 | 35.13 | 7.14 | 95.0 (io/miss 1.00) | 57.1 | 6000 |
| B btree-compressed | 226646 [183877, 229411] | 58.9 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 1.66 | 94.8 (io/miss 1.00) | 13.3 | 6000 |
| C node-blocks | 417766 [392900, 417816] | 108.5 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 3.95 | 98.3 (io/miss 1.00) | 31.6 | 6000 |
| D record-chains | 696490830 [246107579, 803765252] | 80984.9 | n/a | n/a | 5321.7 | 10653.71 | 3342.44 | 4628.76 | 94.7 (io/miss 1.00) | 37030.1 | 80 |
| F csr-paged | 118063 [106047, 133586] | 30.6 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.19 | 89.3 (io/miss 1.00) | 9.5 | 6000 |
| G csr-paged-compressed | 60324 [50143, 75085] | 14.5 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.41 | 82.6 (io/miss 1.00) | 3.3 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7235195 [7077679, 8371118] | 220.5 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 74.25 | 94.8 (io/miss 1.00) | 594.0 | 300 |
| B btree-compressed | 1843694 [1799645, 2247068] | 57.7 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 14.21 | 94.9 (io/miss 1.00) | 113.7 | 300 |
| C node-blocks | 3659831 [3309369, 4179177] | 108.7 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 33.56 | 99.7 (io/miss 1.00) | 268.5 | 300 |
| D record-chains | 4462236662 [3369116222, 5207038310] | 82322.9 | n/a | n/a | 52969.0 | 53144.10 | 17225.96 | 45025.12 | 94.5 (io/miss 1.00) | 360201.0 | 48 |
| F csr-paged | 1326844 [1305102, 1592053] | 41.4 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 11.23 | 99.9 (io/miss 1.00) | 89.8 | 300 |
| G csr-paged-compressed | 618936 [595152, 734625] | 19.1 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 3.77 | 87.3 (io/miss 1.00) | 30.2 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 818704165 [499346160, 1116688858] | 1215.1 | n/a | n/a | 638150.4 | 20390.85 | 8432.31 | 8261.88 | 94.8 (io/miss 1.00) | 66095.0 | 48 |
| B btree-compressed | 373718548 [199015196, 509318151] | 554.7 | n/a | n/a | 600907.1 | 14180.18 | 2967.48 | 3146.55 | 95.3 (io/miss 1.00) | 25172.4 | 62 |
| C node-blocks | 647930060 [385228699, 934909124] | 961.7 | n/a | n/a | 638150.4 | 9878.71 | 5983.60 | 6498.42 | 97.9 (io/miss 1.00) | 51987.3 | 48 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 118899783 [109095110, 189509422] | 249.4 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 2075.66 | 62.9 (io/miss 1.00) | 16605.2 | 90 |
| G csr-paged-compressed | 49886600 [45453301, 85149827] | 106.9 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 669.01 | 76.4 (io/miss 1.00) | 5352.1 | 90 |

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7089 [7089, 7089] | 774.4 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.24 | 3.5 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5633 [5633, 5633] | 615.4 | n/a | n/a | 9.2 | 4.02 | 4.02 | 0.97 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7295 [7295, 7295] | 796.9 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.47 | 3.3 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 79426 [79426, 79426] | 8676.5 | n/a | n/a | 9.2 | 27.40 | 26.85 | 16.55 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4842 [4842, 4842] | 529.0 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4398 [4398, 4398] | 480.4 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6244 [6224, 6437] | 662.5 | n/a | n/a | 9.4 | 4.09 | 4.09 | 1.02 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5007 [4943, 5123] | 527.3 | n/a | n/a | 9.4 | 4.02 | 4.02 | 0.78 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5726 [5609, 6083] | 626.1 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.09 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 81300 [80362, 83679] | 8536.0 | n/a | n/a | 9.4 | 27.97 | 27.31 | 16.95 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 2712 [2670, 3241] | 303.1 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.52 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1007 [952, 2515] | 108.1 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.22 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5839 [5828, 5855] | 2665.6 | n/a | n/a | 2.2 | 4.03 | 4.03 | 0.95 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4776 [4730, 4828] | 2180.3 | n/a | n/a | 2.2 | 4.00 | 4.00 | 0.75 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5835 [5812, 5863] | 2653.1 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.10 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 57097 [55551, 59075] | 26065.7 | n/a | n/a | 2.2 | 16.94 | 16.77 | 11.87 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 2631 [2605, 2863] | 1273.4 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.49 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1004 [949, 1986] | 490.9 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.18 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 55322 [52644, 58138] | 14.4 | n/a | n/a | 3943.8 | 35.13 | 35.13 | 7.24 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 46129 [43060, 52211] | 12.0 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 1.83 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 28601 [28226, 30773] | 7.4 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 4.07 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 16781059 [14830018, 16995289] | 4221.7 | n/a | n/a | 3828.6 | 7667.38 | 2802.36 | 3454.61 | 3.1 (io/miss 0.00) | 0.0 | 1504 |
| F csr-paged | 14074 [13729, 15461] | 3.7 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.54 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 27644 [25502, 31776] | 7.2 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.59 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 580481 [569165, 661471] | 17.8 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 73.39 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 469160 [461245, 580330] | 14.7 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 14.20 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 384476 [325296, 411175] | 10.7 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 34.38 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 148424876 [130891717, 191292714] | 3959.2 | n/a | n/a | 38768.7 | 38881.41 | 14659.14 | 32801.97 | 3.1 (io/miss 0.00) | 0.0 | 176 |
| F csr-paged | 225151 [224108, 271261] | 7.0 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 11.34 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 284125 [279001, 342796] | 8.9 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 4.02 | 3.2 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 30823694 [28793687, 60726895] | 61.8 | n/a | n/a | 575198.1 | 16193.66 | 7277.83 | 6899.77 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 18207236 [16550141, 35296994] | 36.5 | n/a | n/a | 575198.1 | 12607.51 | 2823.57 | 2853.50 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 21628426 [20253885, 46630721] | 43.4 | n/a | n/a | 575198.1 | 7873.82 | 5145.52 | 5289.08 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 2882119994 [1961893558, 3411886164] | 4277.7 | n/a | n/a | 638150.4 | 669827.17 | 41560.90 | 584533.23 | 3.2 (io/miss 0.00) | 0.0 | 48 |
| F csr-paged | 10151777 [9552364, 18898379] | 20.5 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 2075.31 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 7283834 [6734500, 12322436] | 14.6 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 666.34 | 3.1 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 165219904 [132453565, 272964094] | 105.6 | 86368987 | 825168146 | 1858886.3 | 91562.03 | 30012.70 | 32921.17 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 110813092 [81870664, 178761331] | 70.8 | 43107353 | 585466939 | 1858886.3 | 79980.67 | 12102.17 | 17992.43 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 138672808 [104000438, 232821903] | 88.6 | 49225288 | 746395642 | 1858886.3 | 43764.83 | 22107.43 | 29436.03 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 62309625 [46784763, 102150797] | 39.8 | 23237004 | 318497177 | 1858886.3 | 40954.80 | 7853.83 | 12677.10 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 32344903 [27473721, 52733122] | 20.7 | 20015899 | 139212365 | 1858886.3 | 39407.73 | 4698.40 | 3778.03 | 3.1 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5549825529 [5535155808, 5567239942] | 560.9 | 5549825529 | 5549825529 | 9894697.0 | 3930660.00 | 140243.00 | 985217.00 | 3.5 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 4411919711 [4391363672, 4422816718] | 445.9 | 4411919711 | 4411919711 | 9894697.0 | 3865082.00 | 35948.00 | 734779.00 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 5487503635 [5485648144, 5497423616] | 554.6 | 5487503635 | 5487503635 | 9894697.0 | 1897490.00 | 101486.00 | 1079005.00 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 2390058758 [2384937685, 2409538732] | 241.5 | 2390058758 | 2390058758 | 9894697.0 | 1901817.00 | 16851.00 | 465211.00 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 784332098 [782539628, 792847294] | 79.3 | 784332098 | 784332098 | 9894697.0 | 1895619.00 | 9184.00 | 108695.00 | 3.1 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 729 frames = 5.7 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 194323 pages
- B btree-compressed: cache = 1.0% of its 72992 pages
- C node-blocks: cache = 0.5% of its 157292 pages
- D record-chains: cache = 0.9% of its 83720 pages
- F csr-paged: cache = 2.2% of its 33702 pages
- G csr-paged-compressed: cache = 4.0% of its 18369 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 171760 [171760, 171760] | 18762.9 | n/a | n/a | 9.2 | 4.08 | 4.08 | 1.75 | 95.5 (io/miss 1.00) | 14.0 | 5000 |
| B btree-compressed | 110536 [110536, 110536] | 12074.9 | n/a | n/a | 9.2 | 4.02 | 4.02 | 1.12 | 95.1 (io/miss 1.00) | 9.0 | 5000 |
| C node-blocks | 181064 [181064, 181064] | 19779.3 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.86 | 95.2 (io/miss 1.00) | 14.9 | 5000 |
| D record-chains | 2283430 [2283430, 2283430] | 249440.6 | n/a | n/a | 9.2 | 27.40 | 26.85 | 23.68 | 94.4 (io/miss 1.00) | 189.4 | 5000 |
| F csr-paged | 157198 [157198, 157198] | 17172.2 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.61 | 95.7 (io/miss 1.00) | 12.9 | 5000 |
| G csr-paged-compressed | 148489 [148489, 148489] | 16220.9 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.58 | 91.8 (io/miss 1.00) | 12.7 | 5000 |

### Constrained 1%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 171633 [171489, 173180] | 17649.3 | n/a | n/a | 9.4 | 4.09 | 4.09 | 1.76 | 95.0 (io/miss 1.00) | 14.1 | 6000 |
| B btree-compressed | 111780 [103900, 114423] | 11402.7 | n/a | n/a | 9.4 | 4.02 | 4.02 | 1.11 | 95.4 (io/miss 1.00) | 8.9 | 6000 |
| C node-blocks | 182075 [176823, 183052] | 18573.4 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.87 | 94.7 (io/miss 1.00) | 14.9 | 6000 |
| D record-chains | 2385482 [2242026, 2387439] | 245709.8 | n/a | n/a | 9.4 | 27.97 | 27.31 | 24.15 | 94.8 (io/miss 1.00) | 193.2 | 6000 |
| F csr-paged | 158644 [155596, 159296] | 16183.2 | n/a | n/a | 9.4 | 1.98 | 1.98 | 1.61 | 95.8 (io/miss 1.00) | 12.9 | 6000 |
| G csr-paged-compressed | 149671 [146546, 150105] | 15267.9 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.58 | 92.2 (io/miss 1.00) | 12.6 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 163358 [163006, 164028] | 74415.0 | n/a | n/a | 2.2 | 4.03 | 4.03 | 1.67 | 95.0 (io/miss 1.00) | 13.4 | 6000 |
| B btree-compressed | 103889 [103725, 104286] | 47352.4 | n/a | n/a | 2.2 | 4.00 | 4.00 | 1.05 | 95.3 (io/miss 1.00) | 8.4 | 6000 |
| C node-blocks | 183002 [180007, 183686] | 83855.7 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.86 | 95.7 (io/miss 1.00) | 14.9 | 6000 |
| D record-chains | 1414557 [1361403, 1428703] | 645769.1 | n/a | n/a | 2.2 | 16.94 | 16.77 | 14.45 | 94.9 (io/miss 1.00) | 115.6 | 6000 |
| F csr-paged | 156200 [154524, 156965] | 71657.1 | n/a | n/a | 2.2 | 1.98 | 1.98 | 1.59 | 95.9 (io/miss 1.00) | 12.7 | 6000 |
| G csr-paged-compressed | 147439 [146128, 148127] | 67622.3 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.56 | 92.3 (io/miss 1.00) | 12.5 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 3261104 [2815260, 3769992] | 785.0 | n/a | n/a | 4115.7 | 36.48 | 36.48 | 33.15 | 94.7 (io/miss 1.00) | 265.2 | 4704 |
| B btree-compressed | 559890 [503453, 567111] | 145.3 | n/a | n/a | 3943.8 | 10.45 | 10.45 | 5.01 | 95.1 (io/miss 1.00) | 40.1 | 6000 |
| C node-blocks | 1659763 [1492109, 1728864] | 430.6 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 16.42 | 96.3 (io/miss 1.00) | 131.4 | 6000 |
| D record-chains | 1255125298 [410258608, 1445613428] | 145655.6 | n/a | n/a | 5321.7 | 10653.71 | 3342.44 | 8128.77 | 94.8 (io/miss 1.00) | 65030.2 | 80 |
| F csr-paged | 370163 [350462, 373297] | 96.9 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 5.02 | 68.6 (io/miss 1.00) | 40.2 | 6000 |
| G csr-paged-compressed | 262561 [260499, 265911] | 67.6 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 2.47 | 89.6 (io/miss 1.00) | 19.7 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 26774912 [26678798, 31185955] | 834.1 | n/a | n/a | 34166.3 | 322.25 | 299.76 | 286.71 | 95.1 (io/miss 1.00) | 2293.7 | 300 |
| B btree-compressed | 6196704 [5803856, 6996376] | 182.0 | n/a | n/a | 34166.3 | 108.88 | 82.74 | 58.31 | 95.3 (io/miss 1.00) | 466.5 | 300 |
| C node-blocks | 16036823 [15983381, 17990349] | 499.6 | n/a | n/a | 34166.3 | 174.48 | 174.22 | 164.76 | 97.1 (io/miss 1.00) | 1318.1 | 300 |
| D record-chains | 4662576621 [3419534780, 5433144723] | 86019.0 | n/a | n/a | 52969.0 | 53144.10 | 17225.96 | 46708.98 | 94.4 (io/miss 1.00) | 373671.8 | 48 |
| F csr-paged | 4404727 [4246392, 5156348] | 134.2 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 59.40 | 71.0 (io/miss 1.00) | 475.2 | 300 |
| G csr-paged-compressed | 3163231 [3117897, 3832454] | 99.0 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 32.59 | 88.5 (io/miss 1.00) | 260.7 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 944414094 [589980905, 1223113412] | 1401.7 | n/a | n/a | 638150.4 | 20390.85 | 8432.31 | 9344.50 | 95.1 (io/miss 1.00) | 74756.0 | 48 |
| B btree-compressed | 466952317 [259887549, 608008689] | 693.1 | n/a | n/a | 600907.1 | 14180.18 | 2967.48 | 3949.10 | 95.4 (io/miss 1.00) | 31592.8 | 62 |
| C node-blocks | 929974948 [535386839, 1403045749] | 1380.3 | n/a | n/a | 638150.4 | 9878.71 | 5983.60 | 9542.00 | 97.5 (io/miss 1.00) | 76336.0 | 48 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 215302318 [190788049, 325189055] | 428.0 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 3482.99 | 66.7 (io/miss 1.00) | 27863.9 | 90 |
| G csr-paged-compressed | 174789706 [142456247, 238257641] | 313.6 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 2633.60 | 65.4 (io/miss 1.00) | 21068.8 | 90 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2380 [2377, 2396] | 11.10 | 3.04 |  |
| B btree-compressed | 4478 [4454, 4489] | 11.05 | 3.02 |  |
| C node-blocks | 751 [746, 751] | 9.89 | 5.01 |  |
| D record-chains | 1573 [1557, 1615] | 26.66 | 7.29 | existing records rewritten/insert 5.62 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2588 [2576, 2594] | 13.00 | 2.00 |
| B btree-compressed | 4944 [4921, 5027] | 11.01 | 2.00 |
| C node-blocks | 8982 [8766, 9201] | 26.58 | 2.00 |
| D record-chains | 834 [833, 840] | 4.00 | 1.00 |

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

RSS end: 4997 MiB, peak 4997 MiB
