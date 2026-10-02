# layout-bench zipf bulk n=1000000 m=10000000 alpha=0.8 scale=1
clock: Instant::now() pair costs 2732 ns (checksum 0)
graph generated in 1.8s: max out-degree 133939, max in-degree 131746, nodes with out-degree 0: 38495, nodes with total degree > 50: 45809, out-degree p50 4 p90 14 p99 85 p99.9 535

### Build (sorted bulk load, created_ts = 0 (frozen))

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 3.60 | 2774942 | 0.02 | 0.02 | includes sort | 1397 |
| B | 3.81 | 2624060 | 0.01 | 0.01 | includes sort | 2607 |
| C | 2.90 | 3451832 | 0.39 | 0.02 | includes sort | 4084 |
| D | 9.26 | 1079839 | 22.04 | 0.01 | existing records rewritten/edge 5.15, densified nodes 45809 | 4110 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.24 | 41228031 | 123460 | a7c193c4f2af92bf |
| B btree-compressed | 0.34 | 29772579 | 22002 | a7c193c4f2af92bf |
| C node-blocks | 0.29 | 33907381 | 3945261 | a7c193c4f2af92bf |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.28 | 33702 |
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

RSS after build: 4605 MiB, peak 4668 MiB

### Warm: all pages resident


1-hop OUT, all types, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1035 [1025, 1052] | 113.2 | n/a | n/a | 9.1 | 4.06 | 4.06 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1312 [1292, 1314] | 143.4 | n/a | n/a | 9.1 | 3.01 | 3.01 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 399 [382, 400] | 43.7 | n/a | n/a | 9.1 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 2407 [2310, 2417] | 263.2 | n/a | n/a | 9.1 | 27.34 | 18.48 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 326 [318, 327] | 35.7 | n/a | n/a | 9.1 | 1.98 | 1.98 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 397 [389, 398] | 43.4 | n/a | n/a | 9.1 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 265 [264, 266] | 29.0 | n/a | n/a | 9.1 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 953 [915, 959] | 104.2 | n/a | n/a | 9.1 | 4.06 | 4.06 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, type = 2, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1002 [974, 1018] | 398.9 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1228 [1227, 1271] | 488.5 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 370 [348, 375] | 147.1 | n/a | n/a | 2.5 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 2057 [1949, 2064] | 818.8 | n/a | n/a | 2.5 | 17.64 | 12.30 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 301 [295, 302] | 119.7 | n/a | n/a | 2.5 | 1.98 | 1.98 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 396 [390, 402] | 157.7 | n/a | n/a | 2.5 | 1.97 | 1.97 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 250 [247, 252] | 99.6 | n/a | n/a | 2.5 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 905 [864, 1008] | 360.2 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, all types, degree-weighted start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 18196 [18020, 18222] | 4.6 | n/a | n/a | 3994.8 | 28.68 | 28.68 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 33816 [33715, 33931] | 8.5 | n/a | n/a | 3994.8 | 6.54 | 6.54 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 7634 [7576, 7673] | 1.9 | n/a | n/a | 3994.8 | 19.44 | 19.44 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 125864 [125658, 126060] | 31.5 | n/a | n/a | 3994.8 | 7999.65 | 467.73 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 7178 [7167, 7244] | 1.8 | n/a | n/a | 3994.8 | 8.33 | 8.33 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 26303 [26288, 26317] | 6.6 | n/a | n/a | 3994.8 | 4.54 | 4.54 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 5637 [5617, 5676] | 1.4 | n/a | n/a | 3994.8 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 18130 [18096, 18229] | 4.5 | n/a | n/a | 3994.8 | 28.68 | 28.68 | 0.00 | - | 0.0 | 60000 |

2-hop OUT (distinct), uniform start (1000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 164177 [164153, 167964] | 6.2 | n/a | n/a | 26437.8 | 201.23 | 184.98 | 0.00 | - | 0.0 | 3000 |
| B btree-compressed | 277966 [276638, 278776] | 10.5 | n/a | n/a | 26437.8 | 51.98 | 41.43 | 0.00 | - | 0.0 | 3000 |
| C node-blocks | 121097 [120240, 123367] | 4.6 | n/a | n/a | 26437.8 | 133.79 | 133.69 | 0.00 | - | 0.0 | 3000 |
| D record-chains | 332886 [332562, 339053] | 12.6 | n/a | n/a | 26437.8 | 26510.60 | 232.87 | 0.00 | - | 0.0 | 3000 |
| F csr-paged | 130714 [130498, 131951] | 4.9 | n/a | n/a | 26437.8 | 60.70 | 60.26 | 0.00 | - | 0.0 | 3000 |
| G csr-paged-compressed | 215462 [214739, 215586] | 8.1 | n/a | n/a | 26437.8 | 35.72 | 35.27 | 0.00 | - | 0.0 | 3000 |
| E csr-memory | 92393 [92182, 94354] | 3.5 | n/a | n/a | 26437.8 | - | - | - | - | - | 3000 |
| A btree-fixed(rep) | 163758 [163561, 165928] | 6.2 | n/a | n/a | 26437.8 | 201.23 | 184.98 | 0.00 | - | 0.0 | 3000 |

2-hop OUT (distinct), degree-weighted start (200 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6350059 [6335734, 6359308] | 10.4 | n/a | n/a | 612551.1 | 20142.72 | 6922.60 | 0.00 | - | 0.0 | 600 |
| B btree-compressed | 10678260 [10657741, 10688129] | 17.4 | n/a | n/a | 612551.1 | 12844.98 | 2097.31 | 0.00 | - | 0.0 | 600 |
| C node-blocks | 3575242 [3569152, 3583561] | 5.8 | n/a | n/a | 612551.1 | 10270.20 | 5289.19 | 0.00 | - | 0.0 | 600 |
| D record-chains | 13360692 [13236434, 13476056] | 21.8 | n/a | n/a | 612551.1 | 646620.14 | 9303.35 | 0.00 | - | 0.0 | 600 |
| F csr-paged | 3586309 [3583776, 3593705] | 5.9 | n/a | n/a | 612551.1 | 9072.85 | 2283.74 | 0.00 | - | 0.0 | 600 |
| G csr-paged-compressed | 6211296 [6210531, 6251774] | 10.1 | n/a | n/a | 612551.1 | 8539.80 | 1460.94 | 0.00 | - | 0.0 | 600 |
| E csr-memory | 2364387 [2359591, 2382118] | 3.9 | n/a | n/a | 612551.1 | - | - | - | - | - | 600 |
| A btree-fixed(rep) | 6337131 [6332775, 6346620] | 10.3 | n/a | n/a | 612551.1 | 20142.72 | 6922.60 | 0.00 | - | 0.0 | 600 |

3-hop OUT (distinct), uniform start (50 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 30271684 [30235685, 30365976] | 14.2 | 11606201 | 106039832 | 2132064.1 | 125675.80 | 32064.32 | 0.00 | - | 0.0 | 150 |
| B btree-compressed | 48705055 [48547047, 48828979] | 22.8 | 20849786 | 162576483 | 2132064.1 | 86416.02 | 9015.74 | 0.00 | - | 0.0 | 150 |
| C node-blocks | 14473948 [14380277, 14624043] | 6.8 | 7429722 | 43913220 | 2132064.1 | 61802.56 | 22820.72 | 0.00 | - | 0.0 | 150 |
| D record-chains | 65629844 [65390050, 67334718] | 30.8 | 23206610 | 243107947 | 2132064.1 | 2369434.68 | 33045.04 | 0.00 | - | 0.0 | 150 |
| F csr-paged | 13521113 [13520013, 13552850] | 6.3 | 8051804 | 38860505 | 2132064.1 | 59074.28 | 8188.36 | 0.00 | - | 0.0 | 150 |
| G csr-paged-compressed | 23512068 [23472407, 23550174] | 11.0 | 14326687 | 65770626 | 2132064.1 | 57316.12 | 4794.22 | 0.00 | - | 0.0 | 150 |
| E csr-memory | 8710772 [8706366, 8768649] | 4.1 | 5339630 | 24910891 | 2132064.1 | - | - | - | - | - | 150 |
| A btree-fixed(rep) | 30220329 [30183967, 30263941] | 14.2 | 11478182 | 105714774 | 2132064.1 | 125675.80 | 32064.32 | 0.00 | - | 0.0 | 150 |

BFS OUT (unbounded depth), degree-weighted start (2 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 771982132 [769276640, 778552437] | 78.0 | 774629737 | 774629737 | 9894697.0 | 3911407.00 | 109996.00 | 0.00 | - | 0.0 | 6 |
| B btree-compressed | 1047248114 [1046575436, 1049759666] | 105.8 | 1053635830 | 1053635830 | 9894697.0 | 2895451.00 | 20978.00 | 0.00 | - | 0.0 | 6 |
| C node-blocks | 211896438 [211534160, 212629304] | 21.4 | 213721994 | 213721994 | 9894697.0 | 1897490.00 | 61988.00 | 0.00 | - | 0.0 | 6 |
| D record-chains | 1152143306 [1150332110, 1161706802] | 116.4 | 1154485785 | 1154485785 | 9894697.0 | 15900512.00 | 67498.00 | 0.00 | - | 0.0 | 6 |
| F csr-paged | 170807427 [168458799, 171182060] | 17.3 | 171372239 | 171372239 | 9894697.0 | 1901817.00 | 16851.00 | 0.00 | - | 0.0 | 6 |
| G csr-paged-compressed | 229024815 [227742816, 234986014] | 23.1 | 231634880 | 231634880 | 9894697.0 | 1895619.00 | 9184.00 | 0.00 | - | 0.0 | 6 |
| E csr-memory | 96023510 [94081146, 97317463] | 9.7 | 96638469 | 96638469 | 9894697.0 | - | - | - | - | - | 6 |
| A btree-fixed(rep) | 774856237 [768143566, 779380904] | 78.3 | 776791350 | 776791350 | 9894697.0 | 3911407.00 | 109996.00 | 0.00 | - | 0.0 | 6 |

## Constrained cache: 14129 frames = 110.4 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 8.9% of its 158432 pages
- B btree-compressed: cache = 25.0% of its 56517 pages
- C node-blocks: cache = 9.1% of its 154522 pages
- D record-chains: cache = 16.9% of its 83700 pages
- F csr-paged: cache = 41.9% of its 33702 pages
- G csr-paged-compressed: cache = 76.9% of its 18369 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 114025 [114025, 114025] | 12456.0 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.13 | 97.6 (io/miss 1.00) | 9.1 | 5000 |
| B btree-compressed | 88341 [88341, 88341] | 9650.3 | n/a | n/a | 9.2 | 3.01 | 3.01 | 0.89 | 95.2 (io/miss 1.00) | 7.1 | 5000 |
| C node-blocks | 132801 [132801, 132801] | 14507.1 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.39 | 93.3 (io/miss 1.00) | 11.1 | 5000 |
| D record-chains | 586496 [586496, 586496] | 64068.5 | n/a | n/a | 9.2 | 27.40 | 18.72 | 6.15 | 93.0 (io/miss 1.00) | 49.2 | 5000 |
| F csr-paged | 93719 [93719, 93719] | 10237.8 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 92.3 (io/miss 1.00) | 7.9 | 5000 |
| G csr-paged-compressed | 78621 [78621, 78621] | 8588.6 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 86.5 (io/miss 1.00) | 7.1 | 5000 |

### Constrained 25%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 97909 [96461, 99133] | 10202.6 | n/a | n/a | 9.4 | 4.06 | 4.06 | 0.97 | 96.9 (io/miss 1.00) | 7.8 | 6000 |
| B btree-compressed | 36498 [36120, 70367] | 4143.5 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.48 | 94.6 (io/miss 1.00) | 3.8 | 6000 |
| C node-blocks | 87226 [78810, 87724] | 9028.4 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.88 | 93.7 (io/miss 1.00) | 7.0 | 6000 |
| D record-chains | 599492 [517656, 615006] | 62736.5 | n/a | n/a | 9.4 | 27.97 | 18.91 | 6.03 | 93.3 (io/miss 1.00) | 48.2 | 6000 |
| F csr-paged | 19091 [17669, 58025] | 2167.3 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.33 | 92.1 (io/miss 1.00) | 2.7 | 6000 |
| G csr-paged-compressed | 554 [542, 39216] | 61.5 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.15 | 85.4 (io/miss 1.00) | 1.2 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 90678 [89856, 93072] | 41020.9 | n/a | n/a | 2.2 | 4.02 | 4.02 | 0.92 | 96.0 (io/miss 1.00) | 7.3 | 6000 |
| B btree-compressed | 34569 [34429, 64649] | 16900.2 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.45 | 94.4 (io/miss 1.00) | 3.6 | 6000 |
| C node-blocks | 73323 [68600, 81795] | 33537.1 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.78 | 93.7 (io/miss 1.00) | 6.2 | 6000 |
| D record-chains | 502532 [498283, 511203] | 229414.5 | n/a | n/a | 2.2 | 16.94 | 11.96 | 5.27 | 93.2 (io/miss 1.00) | 42.2 | 6000 |
| F csr-paged | 17896 [17632, 50604] | 8748.7 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.30 | 92.7 (io/miss 1.00) | 2.4 | 6000 |
| G csr-paged-compressed | 556 [508, 28908] | 248.4 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.11 | 85.2 (io/miss 1.00) | 0.9 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 307537 [243726, 308605] | 79.9 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 4.04 | 63.2 (io/miss 1.00) | 32.3 | 6000 |
| B btree-compressed | 71183 [65047, 135847] | 18.8 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 0.59 | 89.8 (io/miss 1.00) | 4.7 | 6000 |
| C node-blocks | 232214 [186589, 233122] | 60.5 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 3.00 | 66.8 (io/miss 1.00) | 24.0 | 6000 |
| D record-chains | 1014922 [996142, 1022669] | 258.7 | n/a | n/a | 3943.8 | 7897.57 | 456.56 | 8.75 | 93.4 (io/miss 1.00) | 70.0 | 6000 |
| F csr-paged | 32485 [32334, 116624] | 9.3 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 0.61 | 81.0 (io/miss 1.00) | 4.9 | 6000 |
| G csr-paged-compressed | 29079 [23561, 78955] | 6.8 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.20 | 83.0 (io/miss 1.00) | 1.6 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 2465445 [2437038, 3064358] | 77.1 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 26.76 | 86.4 (io/miss 1.00) | 214.1 | 300 |
| B btree-compressed | 963885 [749978, 1245852] | 25.1 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 5.85 | 95.5 (io/miss 1.00) | 46.8 | 300 |
| C node-blocks | 1981856 [1935400, 2427900] | 62.0 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 20.58 | 90.2 (io/miss 1.00) | 164.6 | 300 |
| D record-chains | 8110485 [7826031, 9778639] | 253.8 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 81.60 | 94.3 (io/miss 1.00) | 652.8 | 300 |
| F csr-paged | 534779 [435590, 974765] | 13.9 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 4.45 | 94.1 (io/miss 1.00) | 35.6 | 300 |
| G csr-paged-compressed | 313704 [262683, 612245] | 8.2 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 1.18 | 89.8 (io/miss 1.00) | 9.4 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 477075984 [288297900, 640013472] | 708.1 | n/a | n/a | 600907.1 | 16870.40 | 6478.29 | 5161.63 | 79.2 (io/miss 1.00) | 41293.0 | 62 |
| B btree-compressed | 90366140 [78926350, 182744248] | 181.1 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 1102.79 | 92.5 (io/miss 1.00) | 8822.3 | 90 |
| C node-blocks | 368383693 [222002955, 493626011] | 546.8 | n/a | n/a | 600907.1 | 8702.77 | 5089.85 | 4035.02 | 78.5 (io/miss 1.00) | 32280.1 | 62 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 47608685 [45050320, 90277086] | 96.5 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 617.20 | 89.0 (io/miss 1.00) | 4937.6 | 90 |
| G csr-paged-compressed | 7846658 [4917883, 9668615] | 10.3 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 18.29 | 84.6 (io/miss 1.00) | 146.3 | 90 |

## Constrained cache: 5651 frames = 44.1 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.6% of its 158432 pages
- B btree-compressed: cache = 10.0% of its 56517 pages
- C node-blocks: cache = 3.7% of its 154522 pages
- D record-chains: cache = 6.8% of its 83700 pages
- F csr-paged: cache = 16.8% of its 33702 pages
- G csr-paged-compressed: cache = 30.8% of its 18369 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 112708 [112708, 112708] | 12312.1 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.13 | 96.5 (io/miss 1.00) | 9.1 | 5000 |
| B btree-compressed | 88311 [88311, 88311] | 9647.0 | n/a | n/a | 9.2 | 3.01 | 3.01 | 0.89 | 95.2 (io/miss 1.00) | 7.1 | 5000 |
| C node-blocks | 134275 [134275, 134275] | 14668.1 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.41 | 93.1 (io/miss 1.00) | 11.3 | 5000 |
| D record-chains | 819272 [819272, 819272] | 89496.8 | n/a | n/a | 9.2 | 27.40 | 18.72 | 8.60 | 93.1 (io/miss 1.00) | 68.8 | 5000 |
| F csr-paged | 94023 [94023, 94023] | 10271.1 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 92.6 (io/miss 1.00) | 7.9 | 5000 |
| G csr-paged-compressed | 78413 [78413, 78413] | 8565.7 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 86.3 (io/miss 1.00) | 7.1 | 5000 |

### Constrained 10%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 101891 [100294, 104829] | 10393.8 | n/a | n/a | 9.4 | 4.06 | 4.06 | 1.02 | 96.9 (io/miss 1.00) | 8.2 | 6000 |
| B btree-compressed | 74480 [71141, 77625] | 7918.5 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.75 | 94.8 (io/miss 1.00) | 6.0 | 6000 |
| C node-blocks | 122547 [108819, 141826] | 12500.9 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.30 | 93.4 (io/miss 1.00) | 10.4 | 6000 |
| D record-chains | 871509 [795805, 883694] | 88902.3 | n/a | n/a | 9.4 | 27.97 | 18.91 | 8.90 | 93.3 (io/miss 1.00) | 71.2 | 6000 |
| F csr-paged | 68377 [58888, 72241] | 7369.3 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.70 | 92.8 (io/miss 1.00) | 5.6 | 6000 |
| G csr-paged-compressed | 39714 [36130, 40081] | 4088.6 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.43 | 86.0 (io/miss 1.00) | 3.5 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 95862 [94569, 96844] | 43172.4 | n/a | n/a | 2.2 | 4.02 | 4.02 | 0.97 | 95.6 (io/miss 1.00) | 7.7 | 6000 |
| B btree-compressed | 72446 [69333, 74323] | 31907.6 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.73 | 94.7 (io/miss 1.00) | 5.8 | 6000 |
| C node-blocks | 112029 [111521, 126443] | 54768.4 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.22 | 93.3 (io/miss 1.00) | 9.8 | 6000 |
| D record-chains | 719471 [698708, 725039] | 330992.3 | n/a | n/a | 2.2 | 16.94 | 11.96 | 7.49 | 93.1 (io/miss 1.00) | 59.9 | 6000 |
| F csr-paged | 65172 [59069, 67975] | 28704.0 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.67 | 92.9 (io/miss 1.00) | 5.4 | 6000 |
| G csr-paged-compressed | 34355 [30095, 38553] | 15131.0 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.39 | 85.2 (io/miss 1.00) | 3.1 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 366719 [366077, 367315] | 95.2 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 5.74 | 57.9 (io/miss 1.00) | 45.9 | 6000 |
| B btree-compressed | 144191 [137829, 157060] | 39.8 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 1.08 | 93.9 (io/miss 1.00) | 8.6 | 6000 |
| C node-blocks | 316994 [313431, 318124] | 81.4 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 4.39 | 67.3 (io/miss 1.00) | 35.1 | 6000 |
| D record-chains | 1942549 [1914630, 1982568] | 504.5 | n/a | n/a | 3943.8 | 7897.57 | 456.56 | 18.49 | 93.4 (io/miss 1.00) | 147.9 | 6000 |
| F csr-paged | 135052 [132271, 143719] | 37.3 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.44 | 85.6 (io/miss 1.00) | 11.5 | 6000 |
| G csr-paged-compressed | 87058 [79948, 95844] | 23.1 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.65 | 87.6 (io/miss 1.00) | 5.2 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 4257257 [4203794, 5008596] | 131.0 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 60.79 | 67.0 (io/miss 1.00) | 486.3 | 300 |
| B btree-compressed | 1517091 [1496081, 1904246] | 47.5 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 11.45 | 97.8 (io/miss 1.00) | 91.6 | 300 |
| C node-blocks | 3581258 [3431609, 4223922] | 109.9 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 42.20 | 81.0 (io/miss 1.00) | 337.6 | 300 |
| D record-chains | 14880285 [13464256, 18134518] | 465.6 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 153.96 | 94.0 (io/miss 1.00) | 1231.7 | 300 |
| F csr-paged | 1454642 [1416579, 1762996] | 45.5 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 13.13 | 96.2 (io/miss 1.00) | 105.1 | 300 |
| G csr-paged-compressed | 924895 [923002, 1117239] | 28.9 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 6.93 | 90.8 (io/miss 1.00) | 55.5 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 542384941 [338521174, 725622474] | 805.0 | n/a | n/a | 638150.4 | 19269.54 | 7069.48 | 7149.56 | 71.6 (io/miss 1.00) | 57196.5 | 48 |
| B btree-compressed | 168049150 [159792465, 260207379] | 342.3 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 2349.79 | 75.7 (io/miss 1.00) | 18798.3 | 90 |
| C node-blocks | 541865584 [338530137, 645473626] | 804.2 | n/a | n/a | 638150.4 | 9878.71 | 5484.27 | 6392.46 | 76.7 (io/miss 1.00) | 51139.7 | 48 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 120947218 [105616737, 211946714] | 259.1 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 2400.42 | 57.2 (io/miss 1.00) | 19203.4 | 90 |
| G csr-paged-compressed | 84644019 [63544725, 123505406] | 162.5 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 1267.72 | 63.5 (io/miss 1.00) | 10141.8 | 90 |

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6246 [6246, 6246] | 682.3 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.13 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 4950 [4950, 4950] | 540.7 | n/a | n/a | 9.2 | 3.01 | 3.01 | 0.89 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 6835 [6835, 6835] | 746.7 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.41 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 41554 [41554, 41554] | 4539.4 | n/a | n/a | 9.2 | 27.40 | 18.72 | 8.60 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 4855 [4855, 4855] | 530.3 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.99 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4409 [4409, 4409] | 481.7 | n/a | n/a | 9.2 | 1.97 | 1.97 | 0.88 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5921 [5870, 5937] | 609.3 | n/a | n/a | 9.4 | 4.06 | 4.06 | 0.99 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4409 [4314, 4445] | 453.8 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.70 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5517 [5515, 5808] | 597.7 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.09 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 44617 [40988, 45045] | 4551.3 | n/a | n/a | 9.4 | 27.97 | 18.91 | 8.93 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3301 [3274, 3302] | 339.8 | n/a | n/a | 9.4 | 1.98 | 1.98 | 0.61 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2021 [2014, 2514] | 229.4 | n/a | n/a | 9.4 | 1.97 | 1.97 | 0.36 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5700 [5699, 5715] | 2602.4 | n/a | n/a | 2.2 | 4.02 | 4.02 | 0.95 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4237 [4236, 4286] | 1956.4 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.69 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 5794 [5761, 5968] | 2724.3 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.14 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 36724 [35505, 37070] | 16923.1 | n/a | n/a | 2.2 | 16.94 | 11.96 | 7.49 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3208 [3151, 3285] | 1499.6 | n/a | n/a | 2.2 | 1.98 | 1.98 | 0.60 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2033 [1971, 2060] | 940.5 | n/a | n/a | 2.2 | 1.97 | 1.97 | 0.33 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 44187 [41776, 45578] | 11.5 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 5.74 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 38124 [34866, 43074] | 9.9 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 1.28 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 28198 [27489, 30075] | 7.3 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 4.39 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 219699 [209117, 239689] | 57.1 | n/a | n/a | 3943.8 | 7897.57 | 456.56 | 18.48 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 14446 [13861, 16013] | 3.8 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 1.67 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 28693 [26459, 32764] | 7.5 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 0.80 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 492395 [469628, 551217] | 14.7 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 60.72 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 391486 [384226, 471070] | 12.2 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 11.50 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 345077 [331194, 394439] | 10.3 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 42.15 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 1130002 [1042406, 1373224] | 35.4 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 153.96 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 215784 [215756, 266908] | 6.8 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 13.05 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 295816 [292963, 362263] | 9.3 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 7.04 | 3.2 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 24630907 [23071340, 48984986] | 49.4 | n/a | n/a | 575198.1 | 15182.43 | 6090.08 | 5908.66 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 14614542 [13345012, 27741267] | 29.3 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 2349.78 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 21108129 [20444436, 40910028] | 43.8 | n/a | n/a | 575198.1 | 7873.82 | 4822.22 | 5367.51 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 77311047 [68089921, 175284003] | 155.0 | n/a | n/a | 575198.1 | 599046.72 | 9157.42 | 20481.98 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 10913378 [10276657, 20815592] | 22.0 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 2399.18 | 3.0 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 9221930 [8650553, 16401335] | 18.5 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 1269.67 | 3.0 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 138258037 [108517887, 228846913] | 88.4 | 71219714 | 719322609 | 1858886.3 | 88260.77 | 25627.43 | 29390.70 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 91495957 [65725932, 145424052] | 58.5 | 34798188 | 481718145 | 1858886.3 | 59352.03 | 8319.20 | 15543.43 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 133194749 [100503734, 212450717] | 85.1 | 51835082 | 677533006 | 1858886.3 | 43764.83 | 19277.80 | 29957.03 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 71743094 [52813679, 115734404] | 45.9 | 25389501 | 376030318 | 1858886.3 | 40954.80 | 7853.83 | 15016.13 | 3.0 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 52368432 [38318578, 80232239] | 32.7 | 23418422 | 244326863 | 1858886.3 | 39407.73 | 4698.40 | 8196.87 | 3.0 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5217523182 [5208461044, 5237844490] | 527.3 | 5217523182 | 5217523182 | 9894697.0 | 3911407.00 | 109996.00 | 965239.67 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 3801865045 [3783788863, 3802639036] | 384.2 | 3801865045 | 3801865045 | 9894697.0 | 2895451.00 | 20978.00 | 665470.33 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 5252496813 [5238852827, 5265234760] | 530.8 | 5252496813 | 5252496813 | 9894697.0 | 1897490.00 | 61988.00 | 1085725.67 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 2897519277 [2877182402, 2919564110] | 292.8 | 2897519277 | 2897519277 | 9894697.0 | 1901817.00 | 16851.00 | 587066.67 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 1718166098 [1704717675, 1738041861] | 173.6 | 1718166098 | 1718166098 | 9894697.0 | 1895619.00 | 9184.00 | 315262.00 | 3.1 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 565 frames = 4.4 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 158432 pages
- B btree-compressed: cache = 1.0% of its 56517 pages
- C node-blocks: cache = 0.4% of its 154522 pages
- D record-chains: cache = 0.7% of its 83700 pages
- F csr-paged: cache = 1.7% of its 33702 pages
- G csr-paged-compressed: cache = 3.1% of its 18369 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 161171 [161171, 161171] | 17606.3 | n/a | n/a | 9.2 | 4.06 | 4.06 | 1.62 | 97.0 (io/miss 1.00) | 12.9 | 5000 |
| B btree-compressed | 99173 [99173, 99173] | 10833.6 | n/a | n/a | 9.2 | 3.01 | 3.01 | 1.00 | 95.0 (io/miss 1.00) | 8.0 | 5000 |
| C node-blocks | 178301 [178301, 178301] | 19477.5 | n/a | n/a | 9.2 | 1.96 | 1.96 | 1.88 | 93.0 (io/miss 1.00) | 15.0 | 5000 |
| D record-chains | 1482982 [1482982, 1482982] | 162000.2 | n/a | n/a | 9.2 | 27.40 | 18.72 | 15.53 | 93.4 (io/miss 1.00) | 124.3 | 5000 |
| F csr-paged | 159469 [159469, 159469] | 17420.3 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.68 | 92.8 (io/miss 1.00) | 13.5 | 5000 |
| G csr-paged-compressed | 149136 [149136, 149136] | 16291.5 | n/a | n/a | 9.2 | 1.97 | 1.97 | 1.66 | 87.8 (io/miss 1.00) | 13.3 | 5000 |

### Constrained 1%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 159876 [159163, 160191] | 16486.5 | n/a | n/a | 9.4 | 4.06 | 4.06 | 1.61 | 96.6 (io/miss 1.00) | 12.8 | 6000 |
| B btree-compressed | 98110 [97815, 98470] | 10097.2 | n/a | n/a | 9.4 | 3.01 | 3.01 | 0.99 | 95.3 (io/miss 1.00) | 7.9 | 6000 |
| C node-blocks | 179334 [178301, 180515] | 18578.2 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.88 | 93.2 (io/miss 1.00) | 15.1 | 6000 |
| D record-chains | 1520784 [1459620, 1532597] | 157731.4 | n/a | n/a | 9.4 | 27.97 | 18.91 | 15.74 | 93.5 (io/miss 1.00) | 125.9 | 6000 |
| F csr-paged | 159628 [157810, 160591] | 16381.8 | n/a | n/a | 9.4 | 1.98 | 1.98 | 1.69 | 92.4 (io/miss 1.00) | 13.5 | 6000 |
| G csr-paged-compressed | 149917 [149666, 149917] | 15429.1 | n/a | n/a | 9.4 | 1.97 | 1.97 | 1.66 | 88.1 (io/miss 1.00) | 13.3 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 153398 [150874, 153900] | 70028.7 | n/a | n/a | 2.2 | 4.02 | 4.02 | 1.55 | 95.5 (io/miss 1.00) | 12.4 | 6000 |
| B btree-compressed | 95858 [95360, 97455] | 44490.1 | n/a | n/a | 2.2 | 3.00 | 3.00 | 0.98 | 94.7 (io/miss 1.00) | 7.8 | 6000 |
| C node-blocks | 177508 [175728, 179196] | 81806.1 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.87 | 92.7 (io/miss 1.00) | 15.0 | 6000 |
| D record-chains | 932559 [908324, 932608] | 425728.8 | n/a | n/a | 2.2 | 16.94 | 11.96 | 9.68 | 93.3 (io/miss 1.00) | 77.5 | 6000 |
| F csr-paged | 157414 [156546, 160694] | 73359.6 | n/a | n/a | 2.2 | 1.98 | 1.98 | 1.68 | 92.4 (io/miss 1.00) | 13.4 | 6000 |
| G csr-paged-compressed | 148018 [146164, 149612] | 68300.2 | n/a | n/a | 2.2 | 1.97 | 1.97 | 1.65 | 87.6 (io/miss 1.00) | 13.2 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 983534 [913610, 1206182] | 263.7 | n/a | n/a | 3943.8 | 28.36 | 28.36 | 25.37 | 38.1 (io/miss 1.00) | 202.9 | 6000 |
| B btree-compressed | 254620 [246689, 262158] | 66.1 | n/a | n/a | 3943.8 | 6.49 | 6.49 | 2.58 | 78.5 (io/miss 1.00) | 20.6 | 6000 |
| C node-blocks | 742626 [698496, 817453] | 192.9 | n/a | n/a | 3943.8 | 19.22 | 19.22 | 18.19 | 39.0 (io/miss 1.00) | 145.5 | 6000 |
| D record-chains | 103853711 [63299385, 117969723] | 24441.7 | n/a | n/a | 3751.6 | 7514.03 | 457.90 | 939.26 | 94.3 (io/miss 1.00) | 7514.0 | 192 |
| F csr-paged | 398340 [371680, 469838] | 104.1 | n/a | n/a | 3943.8 | 8.26 | 8.26 | 5.86 | 66.8 (io/miss 1.00) | 46.9 | 6000 |
| G csr-paged-compressed | 273642 [267281, 319286] | 71.1 | n/a | n/a | 3943.8 | 4.51 | 4.51 | 2.69 | 90.4 (io/miss 1.00) | 21.5 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 9597788 [9513992, 10966814] | 296.4 | n/a | n/a | 34166.3 | 263.50 | 239.12 | 229.88 | 40.8 (io/miss 1.00) | 1839.1 | 300 |
| B btree-compressed | 2994919 [2855783, 3595018] | 93.5 | n/a | n/a | 34166.3 | 69.97 | 53.42 | 31.62 | 79.8 (io/miss 1.00) | 253.0 | 300 |
| C node-blocks | 7539629 [7350987, 10028699] | 235.9 | n/a | n/a | 34166.3 | 174.48 | 174.21 | 169.10 | 45.4 (io/miss 1.00) | 1352.8 | 300 |
| D record-chains | 27780604 [27694763, 47190394] | 866.5 | n/a | n/a | 34166.3 | 34267.38 | 305.19 | 301.82 | 108.5 (io/miss 1.00) | 2414.5 | 300 |
| F csr-paged | 4442137 [4314399, 7998213] | 139.0 | n/a | n/a | 34166.3 | 80.24 | 79.26 | 65.48 | 79.8 (io/miss 1.00) | 523.8 | 300 |
| G csr-paged-compressed | 3246518 [3166452, 3661864] | 98.6 | n/a | n/a | 34166.3 | 48.10 | 47.08 | 34.90 | 83.6 (io/miss 1.00) | 279.2 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 599832825 [377805628, 780422216] | 890.3 | n/a | n/a | 638150.4 | 19269.54 | 7069.48 | 7966.06 | 70.5 (io/miss 1.00) | 63728.5 | 48 |
| B btree-compressed | 203326649 [187258654, 302479963] | 401.1 | n/a | n/a | 575198.1 | 9261.99 | 2022.14 | 2846.59 | 74.8 (io/miss 1.00) | 22772.7 | 90 |
| C node-blocks | 597700935 [367529840, 682872900] | 876.1 | n/a | n/a | 638150.4 | 9878.71 | 5484.27 | 7083.60 | 74.8 (io/miss 1.00) | 56668.8 | 48 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 206438701 [184121926, 313694072] | 412.8 | n/a | n/a | 575198.1 | 6673.02 | 2247.40 | 3498.91 | 63.8 (io/miss 1.00) | 27991.3 | 90 |
| G csr-paged-compressed | 180164118 [145952456, 244846010] | 322.2 | n/a | n/a | 575198.1 | 6166.04 | 1440.69 | 2650.18 | 67.0 (io/miss 1.00) | 21201.4 | 90 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2288 [2257, 2315] | 11.02 | 3.01 |  |
| B btree-compressed | 3843 [3839, 3859] | 9.02 | 3.01 |  |
| C node-blocks | 661 [653, 709] | 9.89 | 5.01 |  |
| D record-chains | 1515 [1462, 1543] | 26.66 | 7.16 | existing records rewritten/insert 5.62 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2429 [2385, 2433] | 13.00 | 2.00 |
| B btree-compressed | 4558 [4506, 4583] | 9.00 | 2.00 |
| C node-blocks | 5918 [5750, 5976] | 26.57 | 2.00 |
| D record-chains | 821 [821, 826] | 4.00 | 1.00 |

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

RSS end: 4775 MiB, peak 4775 MiB
