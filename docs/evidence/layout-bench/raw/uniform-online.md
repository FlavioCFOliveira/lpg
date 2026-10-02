# layout-bench uniform online n=1000000 m=10000000 alpha=0 scale=1
clock: Instant::now() pair costs 2736 ns (checksum 6)
graph generated in 0.2s: max out-degree 27, max in-degree 29, nodes with out-degree 0: 43, nodes with total degree > 50: 0, out-degree p50 10 p90 14 p99 18 p99.9 21

### Build (online inserts in random order, created_ts = edge id + 1)

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 20.05 | 498691 | 10.64 | 3.04 |  | 1623 |
| B | 38.55 | 259392 | 9.25 | 3.01 |  | 2210 |
| C | 6.80 | 1471542 | 9.73 | 4.95 | relocations/edge 0.732, entries copied/edge 2.53 | 3805 |
| D | 12.59 | 794432 | 19.76 | 5.90 | existing records rewritten/edge 3.90, densified nodes 0 | 4459 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.31 | 31989382 | 158139 | 4b99dd115ab53ad0 |
| B btree-compressed | 0.37 | 26858745 | 40028 | 4b99dd115ab53ad0 |
| C node-blocks | 0.70 | 14344355 | 3999912 | 4b99dd115ab53ad0 |

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

RSS after build: 5154 MiB, peak 5221 MiB

### Warm: all pages resident


1-hop OUT, all types, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1196 [1108, 1196] | 119.7 | n/a | n/a | 10.0 | 4.09 | 4.09 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1475 [1470, 1484] | 147.6 | n/a | n/a | 10.0 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 501 [497, 504] | 50.1 | n/a | n/a | 10.0 | 2.00 | 2.00 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 4457 [4403, 4488] | 446.1 | n/a | n/a | 10.0 | 33.98 | 33.95 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 371 [361, 373] | 37.2 | n/a | n/a | 10.0 | 2.02 | 2.02 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 422 [415, 424] | 42.2 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 329 [317, 336] | 32.9 | n/a | n/a | 10.0 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1112 [1032, 1172] | 111.3 | n/a | n/a | 10.0 | 4.09 | 4.09 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, type = 2, uniform start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1110 [1095, 1111] | 440.1 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1361 [1357, 1369] | 539.8 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 435 [433, 439] | 172.6 | n/a | n/a | 2.5 | 2.00 | 2.00 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 4063 [4053, 4100] | 1611.1 | n/a | n/a | 2.5 | 26.55 | 26.54 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 351 [344, 361] | 139.0 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 419 [413, 428] | 166.0 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 264 [259, 267] | 104.6 | n/a | n/a | 2.5 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1041 [943, 1085] | 412.9 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.00 | - | 0.0 | 60000 |

1-hop OUT, all types, degree-weighted start (20000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1204 [1193, 1218] | 109.2 | n/a | n/a | 11.0 | 4.09 | 4.09 | 0.00 | - | 0.0 | 60000 |
| B btree-compressed | 1480 [1477, 1493] | 134.2 | n/a | n/a | 11.0 | 4.02 | 4.02 | 0.00 | - | 0.0 | 60000 |
| C node-blocks | 506 [504, 512] | 45.9 | n/a | n/a | 11.0 | 2.00 | 2.00 | 0.00 | - | 0.0 | 60000 |
| D record-chains | 4633 [4613, 4710] | 420.2 | n/a | n/a | 11.0 | 36.06 | 36.03 | 0.00 | - | 0.0 | 60000 |
| F csr-paged | 376 [372, 380] | 34.1 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.00 | - | 0.0 | 60000 |
| G csr-paged-compressed | 430 [428, 431] | 39.0 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.00 | - | 0.0 | 60000 |
| E csr-memory | 337 [336, 338] | 30.5 | n/a | n/a | 11.0 | - | - | - | - | - | 60000 |
| A btree-fixed(rep) | 1128 [1053, 1203] | 102.3 | n/a | n/a | 11.0 | 4.09 | 4.09 | 0.00 | - | 0.0 | 60000 |

2-hop OUT (distinct), uniform start (1000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 13011 [12993, 13035] | 118.9 | n/a | n/a | 109.4 | 44.68 | 29.73 | 0.00 | - | 0.0 | 3000 |
| B btree-compressed | 16526 [16407, 18553] | 151.0 | n/a | n/a | 109.4 | 43.98 | 24.87 | 0.00 | - | 0.0 | 3000 |
| C node-blocks | 5498 [5151, 6195] | 50.2 | n/a | n/a | 109.4 | 21.87 | 21.86 | 0.00 | - | 0.0 | 3000 |
| D record-chains | 37715 [37697, 37905] | 344.6 | n/a | n/a | 109.4 | 242.79 | 232.45 | 0.00 | - | 0.0 | 3000 |
| F csr-paged | 3861 [3783, 4010] | 35.3 | n/a | n/a | 109.4 | 22.04 | 21.98 | 0.00 | - | 0.0 | 3000 |
| G csr-paged-compressed | 4089 [4022, 4153] | 37.4 | n/a | n/a | 109.4 | 21.97 | 21.90 | 0.00 | - | 0.0 | 3000 |
| E csr-memory | 2855 [2852, 2861] | 26.1 | n/a | n/a | 109.4 | - | - | - | - | - | 3000 |
| A btree-fixed(rep) | 12072 [11220, 12475] | 110.3 | n/a | n/a | 109.4 | 44.68 | 29.73 | 0.00 | - | 0.0 | 3000 |

2-hop OUT (distinct), degree-weighted start (200 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 13551 [13344, 14111] | 114.8 | n/a | n/a | 118.0 | 48.12 | 31.64 | 0.00 | - | 0.0 | 600 |
| B btree-compressed | 19287 [19129, 20239] | 163.4 | n/a | n/a | 118.0 | 47.30 | 26.52 | 0.00 | - | 0.0 | 600 |
| C node-blocks | 6520 [6207, 6836] | 55.3 | n/a | n/a | 118.0 | 23.53 | 23.50 | 0.00 | - | 0.0 | 600 |
| D record-chains | 48011 [47993, 48117] | 406.9 | n/a | n/a | 118.0 | 260.69 | 249.40 | 0.00 | - | 0.0 | 600 |
| F csr-paged | 4693 [4667, 4844] | 39.8 | n/a | n/a | 118.0 | 23.73 | 23.64 | 0.00 | - | 0.0 | 600 |
| G csr-paged-compressed | 4890 [4876, 5237] | 41.4 | n/a | n/a | 118.0 | 23.62 | 23.54 | 0.00 | - | 0.0 | 600 |
| E csr-memory | 3308 [3306, 3459] | 28.0 | n/a | n/a | 118.0 | - | - | - | - | - | 600 |
| A btree-fixed(rep) | 12259 [12204, 12595] | 103.9 | n/a | n/a | 118.0 | 48.12 | 31.64 | 0.00 | - | 0.0 | 600 |

3-hop OUT (distinct), uniform start (50 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 135268 [133152, 140278] | 123.8 | 137449 | 270079 | 1092.8 | 446.36 | 229.92 | 0.00 | - | 0.0 | 150 |
| B btree-compressed | 170248 [169376, 180857] | 155.8 | 176490 | 357242 | 1092.8 | 439.56 | 201.08 | 0.00 | - | 0.0 | 150 |
| C node-blocks | 59015 [54883, 60275] | 54.0 | 61600 | 127252 | 1092.8 | 218.60 | 216.68 | 0.00 | - | 0.0 | 150 |
| D record-chains | 382051 [379863, 395931] | 349.6 | 389788 | 846204 | 1092.8 | 2413.82 | 2262.36 | 0.00 | - | 0.0 | 150 |
| F csr-paged | 40923 [40911, 42172] | 37.4 | 40509 | 100293 | 1092.8 | 219.84 | 211.96 | 0.00 | - | 0.0 | 150 |
| G csr-paged-compressed | 43619 [43478, 44938] | 39.9 | 44350 | 103994 | 1092.8 | 219.46 | 211.26 | 0.00 | - | 0.0 | 150 |
| E csr-memory | 30188 [30015, 30291] | 27.6 | 30102 | 66419 | 1092.8 | - | - | - | - | - | 150 |
| A btree-fixed(rep) | 119949 [117778, 122775] | 109.8 | 121525 | 229012 | 1092.8 | 446.36 | 229.92 | 0.00 | - | 0.0 | 150 |

BFS OUT (unbounded depth), degree-weighted start (2 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 995419916 [994711240, 997819394] | 99.5 | 998359816 | 998359816 | 9999533.0 | 4086564.00 | 159169.00 | 0.00 | - | 0.0 | 6 |
| B btree-compressed | 1338294749 [1335072893, 1341646662] | 133.8 | 1344052096 | 1344052096 | 9999533.0 | 4021775.00 | 40284.00 | 0.00 | - | 0.0 | 6 |
| C node-blocks | 332609892 [332131748, 334193666] | 33.3 | 335683530 | 335683530 | 9999533.0 | 1999867.00 | 158129.00 | 0.00 | - | 0.0 | 6 |
| D record-chains | 3243992460 [3243897474, 3249646050] | 324.4 | 3246313623 | 3246313623 | 9999533.0 | 20999491.00 | 67048.00 | 0.00 | - | 0.0 | 6 |
| F csr-paged | 241163998 [240899468, 241243130] | 24.1 | 242012683 | 242012683 | 9999533.0 | 2015147.00 | 16851.00 | 0.00 | - | 0.0 | 6 |
| G csr-paged-compressed | 270804172 [270384000, 274080590] | 27.1 | 274920578 | 274920578 | 9999533.0 | 2009418.00 | 9681.00 | 0.00 | - | 0.0 | 6 |
| E csr-memory | 123019916 [122272574, 123676502] | 12.3 | 123708523 | 123708523 | 9999533.0 | - | - | - | - | - | 6 |
| A btree-fixed(rep) | 995459503 [993332000, 997125138] | 99.6 | 996305603 | 996305603 | 9999533.0 | 4086564.00 | 159169.00 | 0.00 | - | 0.0 | 6 |

## Constrained cache: 18687 frames = 146.0 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 9.7% of its 193635 pages
- B btree-compressed: cache = 25.0% of its 74750 pages
- C node-blocks: cache = 9.3% of its 200646 pages
- D record-chains: cache = 22.4% of its 83270 pages
- F csr-paged: cache = 55.4% of its 33702 pages
- G csr-paged-compressed: cache = 96.5% of its 19362 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 124105 [124105, 124105] | 12466.6 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.27 | 94.7 (io/miss 1.00) | 10.2 | 5000 |
| B btree-compressed | 96733 [96733, 96733] | 9717.0 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.00 | 92.5 (io/miss 1.00) | 8.0 | 5000 |
| C node-blocks | 146807 [146807, 146807] | 14747.1 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.54 | 93.1 (io/miss 1.00) | 12.3 | 5000 |
| D record-chains | 1587131 [1587131, 1587131] | 159430.6 | n/a | n/a | 10.0 | 33.91 | 33.89 | 16.90 | 91.8 (io/miss 1.00) | 135.2 | 5000 |
| F csr-paged | 101502 [101502, 101502] | 10196.0 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 93.7 (io/miss 1.00) | 8.5 | 5000 |
| G csr-paged-compressed | 77838 [77838, 77838] | 7819.0 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 79.5 (io/miss 1.00) | 7.6 | 5000 |

### Constrained 25%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 95169 [93170, 103481] | 9532.1 | n/a | n/a | 10.1 | 4.08 | 4.08 | 1.00 | 93.9 (io/miss 1.00) | 8.0 | 6000 |
| B btree-compressed | 70436 [56844, 84756] | 7054.9 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.72 | 93.0 (io/miss 1.00) | 5.8 | 6000 |
| C node-blocks | 88740 [87195, 112798] | 8888.2 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.01 | 93.3 (io/miss 1.00) | 8.0 | 6000 |
| D record-chains | 1505043 [1497000, 1525006] | 150447.0 | n/a | n/a | 10.1 | 34.14 | 34.12 | 16.11 | 91.5 (io/miss 1.00) | 128.9 | 6000 |
| F csr-paged | 41650 [24900, 66752] | 4171.6 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.46 | 93.8 (io/miss 1.00) | 3.7 | 6000 |
| G csr-paged-compressed | 18023 [7766, 42135] | 1805.2 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.27 | 79.0 (io/miss 1.00) | 2.2 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 89630 [88186, 93745] | 35287.3 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.93 | 93.9 (io/miss 1.00) | 7.4 | 6000 |
| B btree-compressed | 65440 [53216, 79611] | 25763.9 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.68 | 93.0 (io/miss 1.00) | 5.4 | 6000 |
| C node-blocks | 88120 [85901, 101839] | 34693.0 | n/a | n/a | 2.5 | 2.00 | 2.00 | 0.96 | 93.2 (io/miss 1.00) | 7.7 | 6000 |
| D record-chains | 1500381 [1492820, 1512278] | 595385.1 | n/a | n/a | 2.5 | 26.64 | 26.64 | 16.03 | 91.6 (io/miss 1.00) | 128.2 | 6000 |
| F csr-paged | 37199 [23304, 58404] | 14645.2 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.41 | 93.5 (io/miss 1.00) | 3.3 | 6000 |
| G csr-paged-compressed | 14606 [6021, 33167] | 5750.4 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.21 | 79.0 (io/miss 1.00) | 1.7 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 93820 [93716, 98856] | 8565.9 | n/a | n/a | 11.0 | 4.10 | 4.10 | 0.98 | 93.9 (io/miss 1.00) | 7.8 | 6000 |
| B btree-compressed | 63672 [52380, 75824] | 5819.8 | n/a | n/a | 11.0 | 4.03 | 4.03 | 0.66 | 92.6 (io/miss 1.00) | 5.2 | 6000 |
| C node-blocks | 86735 [86504, 96941] | 7927.9 | n/a | n/a | 11.0 | 2.00 | 2.00 | 0.94 | 93.5 (io/miss 1.00) | 7.5 | 6000 |
| D record-chains | 1588828 [1585623, 1592446] | 143962.9 | n/a | n/a | 11.0 | 36.05 | 36.03 | 16.92 | 91.7 (io/miss 1.00) | 135.4 | 6000 |
| F csr-paged | 30695 [18541, 51928] | 2805.6 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.35 | 93.6 (io/miss 1.00) | 2.8 | 6000 |
| G csr-paged-compressed | 10555 [5249, 25348] | 964.8 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.17 | 77.7 (io/miss 1.00) | 1.3 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 993814 [989211, 1073314] | 9192.6 | n/a | n/a | 108.1 | 44.18 | 29.59 | 10.43 | 94.1 (io/miss 1.00) | 83.5 | 300 |
| B btree-compressed | 665487 [548147, 827335] | 6155.6 | n/a | n/a | 108.1 | 43.50 | 24.63 | 6.90 | 93.1 (io/miss 1.00) | 55.2 | 300 |
| C node-blocks | 926184 [922116, 1052446] | 8567.1 | n/a | n/a | 108.1 | 21.63 | 21.63 | 9.91 | 95.0 (io/miss 1.00) | 79.3 | 300 |
| D record-chains | 15467431 [15344001, 15501151] | 143091.9 | n/a | n/a | 108.1 | 239.49 | 229.24 | 164.83 | 91.5 (io/miss 1.00) | 1318.6 | 300 |
| F csr-paged | 324472 [200807, 526408] | 3001.3 | n/a | n/a | 108.1 | 21.81 | 21.78 | 3.65 | 92.4 (io/miss 1.00) | 29.2 | 300 |
| G csr-paged-compressed | 110625 [51599, 242448] | 1023.3 | n/a | n/a | 108.1 | 21.74 | 21.71 | 1.61 | 77.4 (io/miss 1.00) | 12.9 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1143261 [1106920, 1158486] | 9485.2 | n/a | n/a | 117.8 | 47.96 | 31.89 | 11.62 | 94.3 (io/miss 1.00) | 93.0 | 90 |
| B btree-compressed | 752235 [678349, 839455] | 6149.1 | n/a | n/a | 117.8 | 47.20 | 26.56 | 7.64 | 93.5 (io/miss 1.00) | 61.2 | 90 |
| C node-blocks | 1052811 [1007702, 1094327] | 8813.7 | n/a | n/a | 117.8 | 23.44 | 23.44 | 11.02 | 92.9 (io/miss 1.00) | 88.2 | 90 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 349109 [233584, 554587] | 2853.8 | n/a | n/a | 117.8 | 23.71 | 23.67 | 3.90 | 93.6 (io/miss 1.00) | 31.2 | 90 |
| G csr-paged-compressed | 115972 [58451, 235532] | 948.0 | n/a | n/a | 117.8 | 23.54 | 23.50 | 1.64 | 76.9 (io/miss 1.00) | 13.2 | 90 |

## Constrained cache: 7475 frames = 58.4 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.9% of its 193635 pages
- B btree-compressed: cache = 10.0% of its 74750 pages
- C node-blocks: cache = 3.7% of its 200646 pages
- D record-chains: cache = 9.0% of its 83270 pages
- F csr-paged: cache = 22.2% of its 33702 pages
- G csr-paged-compressed: cache = 38.6% of its 19362 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 123378 [123378, 123378] | 12393.5 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.27 | 94.1 (io/miss 1.00) | 10.2 | 5000 |
| B btree-compressed | 97349 [97349, 97349] | 9778.9 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.00 | 93.1 (io/miss 1.00) | 8.0 | 5000 |
| C node-blocks | 147830 [147830, 147830] | 14849.8 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.54 | 93.8 (io/miss 1.00) | 12.3 | 5000 |
| D record-chains | 2158993 [2158993, 2158993] | 216875.2 | n/a | n/a | 10.0 | 33.91 | 33.89 | 22.98 | 91.9 (io/miss 1.00) | 183.8 | 5000 |
| F csr-paged | 101527 [101527, 101527] | 10198.6 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 93.7 (io/miss 1.00) | 8.5 | 5000 |
| G csr-paged-compressed | 77875 [77875, 77875] | 7822.8 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 79.6 (io/miss 1.00) | 7.6 | 5000 |

### Constrained 10%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 101032 [100507, 103071] | 10066.8 | n/a | n/a | 10.1 | 4.08 | 4.08 | 1.04 | 94.0 (io/miss 1.00) | 8.3 | 6000 |
| B btree-compressed | 80793 [79854, 84158] | 8092.2 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.84 | 92.5 (io/miss 1.00) | 6.7 | 6000 |
| C node-blocks | 117091 [115362, 119861] | 11554.6 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.22 | 93.6 (io/miss 1.00) | 9.8 | 6000 |
| D record-chains | 2172811 [2154292, 2186173] | 215673.4 | n/a | n/a | 10.1 | 34.14 | 34.12 | 23.12 | 91.8 (io/miss 1.00) | 184.9 | 6000 |
| F csr-paged | 58017 [56031, 66717] | 5811.0 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.62 | 93.9 (io/miss 1.00) | 5.0 | 6000 |
| G csr-paged-compressed | 21282 [21001, 42259] | 2131.6 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.34 | 79.3 (io/miss 1.00) | 2.7 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 95090 [94838, 96056] | 37634.3 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.98 | 93.9 (io/miss 1.00) | 7.8 | 6000 |
| B btree-compressed | 79679 [79508, 80470] | 31618.7 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.82 | 93.3 (io/miss 1.00) | 6.6 | 6000 |
| C node-blocks | 116095 [115807, 118868] | 46069.3 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.22 | 93.4 (io/miss 1.00) | 9.8 | 6000 |
| D record-chains | 1994801 [1985694, 1999372] | 786225.6 | n/a | n/a | 2.5 | 26.64 | 26.64 | 21.22 | 91.9 (io/miss 1.00) | 169.7 | 6000 |
| F csr-paged | 58201 [57547, 58972] | 22913.7 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.60 | 93.9 (io/miss 1.00) | 4.8 | 6000 |
| G csr-paged-compressed | 21447 [20862, 32957] | 8443.7 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.30 | 79.1 (io/miss 1.00) | 2.4 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 102217 [101183, 103125] | 9248.5 | n/a | n/a | 11.0 | 4.10 | 4.10 | 1.05 | 94.4 (io/miss 1.00) | 8.4 | 6000 |
| B btree-compressed | 80168 [79977, 83050] | 7245.1 | n/a | n/a | 11.0 | 4.03 | 4.03 | 0.83 | 93.1 (io/miss 1.00) | 6.7 | 6000 |
| C node-blocks | 118104 [115296, 119271] | 10677.0 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.23 | 93.5 (io/miss 1.00) | 9.8 | 6000 |
| D record-chains | 2276952 [2275698, 2283202] | 206344.5 | n/a | n/a | 11.0 | 36.05 | 36.03 | 24.19 | 92.1 (io/miss 1.00) | 193.5 | 6000 |
| F csr-paged | 57129 [56770, 57655] | 5189.0 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.59 | 93.7 (io/miss 1.00) | 4.7 | 6000 |
| G csr-paged-compressed | 21567 [21133, 25463] | 1971.3 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.27 | 79.0 (io/miss 1.00) | 2.2 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1103343 [1089566, 1104871] | 10185.0 | n/a | n/a | 108.1 | 44.18 | 29.59 | 11.24 | 94.4 (io/miss 1.00) | 89.9 | 300 |
| B btree-compressed | 886426 [863170, 889029] | 8199.3 | n/a | n/a | 108.1 | 43.50 | 24.63 | 9.00 | 93.1 (io/miss 1.00) | 72.0 | 300 |
| C node-blocks | 1262210 [1254949, 1291752] | 11675.2 | n/a | n/a | 108.1 | 21.63 | 21.63 | 13.29 | 93.2 (io/miss 1.00) | 106.3 | 300 |
| D record-chains | 19025638 [18863522, 19120904] | 176359.3 | n/a | n/a | 108.1 | 239.49 | 229.24 | 202.55 | 91.8 (io/miss 1.00) | 1620.4 | 300 |
| F csr-paged | 627155 [619801, 650546] | 5813.5 | n/a | n/a | 108.1 | 21.81 | 21.78 | 6.55 | 93.8 (io/miss 1.00) | 52.4 | 300 |
| G csr-paged-compressed | 232803 [230406, 243375] | 2149.0 | n/a | n/a | 108.1 | 21.74 | 21.71 | 2.87 | 77.9 (io/miss 1.00) | 23.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1214476 [1123074, 1217456] | 9952.0 | n/a | n/a | 117.8 | 47.96 | 31.89 | 12.21 | 93.7 (io/miss 1.00) | 97.7 | 90 |
| B btree-compressed | 988023 [974937, 1011332] | 8354.2 | n/a | n/a | 117.8 | 47.20 | 26.56 | 9.94 | 94.8 (io/miss 1.00) | 79.6 | 90 |
| C node-blocks | 1444228 [1360264, 1447730] | 11897.4 | n/a | n/a | 117.8 | 23.44 | 23.44 | 14.72 | 93.9 (io/miss 1.00) | 117.8 | 90 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 689859 [688865, 717083] | 5902.9 | n/a | n/a | 117.8 | 23.71 | 23.67 | 7.24 | 93.7 (io/miss 1.00) | 58.0 | 90 |
| G csr-paged-compressed | 254685 [246612, 272004] | 2113.2 | n/a | n/a | 117.8 | 23.54 | 23.50 | 3.10 | 79.1 (io/miss 1.00) | 24.8 | 90 |

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7273 [7273, 7273] | 730.6 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.27 | 3.5 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5772 [5772, 5772] | 579.8 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.00 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7484 [7484, 7484] | 751.8 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.54 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 110284 [110284, 110284] | 11078.2 | n/a | n/a | 10.0 | 33.91 | 33.89 | 22.98 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 5179 [5179, 5179] | 520.3 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4718 [4718, 4718] | 473.9 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6305 [6243, 6409] | 625.3 | n/a | n/a | 10.1 | 4.08 | 4.08 | 1.04 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5330 [5313, 5436] | 533.9 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.84 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6378 [6377, 6445] | 638.7 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.23 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 110418 [110316, 111122] | 10990.4 | n/a | n/a | 10.1 | 34.14 | 34.12 | 23.12 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3279 [3210, 3668] | 323.5 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.63 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1817 [1678, 2869] | 179.2 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.34 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6008 [6006, 6046] | 2380.4 | n/a | n/a | 2.5 | 4.03 | 4.03 | 0.98 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5146 [5139, 5170] | 2023.4 | n/a | n/a | 2.5 | 4.01 | 4.01 | 0.82 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6364 [6312, 6391] | 2516.1 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.22 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 101831 [99758, 102246] | 40043.5 | n/a | n/a | 2.5 | 26.64 | 26.64 | 21.22 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3333 [3290, 3342] | 1310.8 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.61 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1771 [1698, 2373] | 702.6 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.31 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6387 [6374, 6399] | 578.5 | n/a | n/a | 11.0 | 4.10 | 4.10 | 1.05 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5309 [5307, 5475] | 479.9 | n/a | n/a | 11.0 | 4.03 | 4.03 | 0.83 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6387 [6369, 6481] | 583.8 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.23 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 115784 [114375, 116960] | 10570.3 | n/a | n/a | 11.0 | 36.05 | 36.03 | 24.19 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3310 [3271, 3386] | 299.1 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.60 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 1727 [1655, 1959] | 156.1 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.27 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 70602 [69803, 72291] | 654.4 | n/a | n/a | 108.1 | 44.18 | 29.59 | 11.24 | 3.6 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 62670 [61390, 63666] | 579.7 | n/a | n/a | 108.1 | 43.50 | 24.63 | 9.05 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 71156 [70332, 71524] | 656.8 | n/a | n/a | 108.1 | 21.63 | 21.63 | 13.28 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 982317 [978125, 989418] | 9086.3 | n/a | n/a | 108.1 | 239.49 | 229.24 | 202.55 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 37732 [37145, 38087] | 349.8 | n/a | n/a | 108.1 | 21.81 | 21.78 | 6.61 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 21193 [21036, 21275] | 195.6 | n/a | n/a | 108.1 | 21.74 | 21.71 | 2.95 | 3.3 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 78034 [78018, 78405] | 668.5 | n/a | n/a | 117.8 | 47.96 | 31.89 | 12.21 | 3.7 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 68222 [67027, 70957] | 584.6 | n/a | n/a | 117.8 | 47.20 | 26.56 | 9.91 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 79974 [76258, 81124] | 667.0 | n/a | n/a | 117.8 | 23.44 | 23.44 | 14.70 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 1052548 [1051260, 1111541] | 9086.2 | n/a | n/a | 117.8 | 261.19 | 249.97 | 220.73 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 41670 [40611, 41959] | 348.0 | n/a | n/a | 117.8 | 23.71 | 23.67 | 7.18 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 22338 [22049, 23579] | 188.9 | n/a | n/a | 117.8 | 23.54 | 23.50 | 3.00 | 3.5 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 688515 [666243, 786357] | 647.0 | 624107 | 1194576 | 1099.2 | 449.67 | 231.93 | 115.10 | 3.6 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 597050 [566383, 705242] | 561.1 | 550913 | 1036384 | 1099.2 | 442.20 | 202.50 | 91.97 | 3.5 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 676063 [670922, 792782] | 648.4 | 664825 | 1145058 | 1099.2 | 220.07 | 218.17 | 133.97 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 360859 [345808, 420972] | 342.1 | 352562 | 591351 | 1099.2 | 221.77 | 214.77 | 67.17 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 195278 [183328, 233440] | 183.5 | 188014 | 341177 | 1099.2 | 221.00 | 213.57 | 29.03 | 3.3 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5946083537 [5931718968, 5967036149] | 594.6 | 5946083537 | 5946083537 | 9999533.0 | 4086564.00 | 159169.00 | 1046700.33 | 3.5 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 5131903389 [5120157305, 5174619907] | 513.2 | 5131903389 | 5131903389 | 9999533.0 | 4021775.00 | 40284.00 | 835027.00 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 6272378309 [6256538499, 6294046145] | 627.3 | 6272378309 | 6272378309 | 9999533.0 | 1999867.00 | 158129.00 | 1230546.67 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 3132433636 [3121603714, 3141552489] | 313.3 | 3132433636 | 3132433636 | 9999533.0 | 2015147.00 | 16851.00 | 603529.00 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 1563451413 [1553404180, 1565876240] | 156.4 | 1563451413 | 1563451413 | 9999533.0 | 2009418.00 | 9681.00 | 259228.00 | 3.2 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 747 frames = 5.8 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 193635 pages
- B btree-compressed: cache = 1.0% of its 74750 pages
- C node-blocks: cache = 0.4% of its 200646 pages
- D record-chains: cache = 0.9% of its 83270 pages
- F csr-paged: cache = 2.2% of its 33702 pages
- G csr-paged-compressed: cache = 3.9% of its 19362 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 173886 [173886, 173886] | 17467.2 | n/a | n/a | 10.0 | 4.09 | 4.09 | 1.78 | 94.8 (io/miss 1.00) | 14.3 | 5000 |
| B btree-compressed | 107990 [107990, 107990] | 10847.9 | n/a | n/a | 10.0 | 4.02 | 4.02 | 1.12 | 93.0 (io/miss 1.00) | 8.9 | 5000 |
| C node-blocks | 181221 [181221, 181221] | 18204.0 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.90 | 93.3 (io/miss 1.00) | 15.2 | 5000 |
| D record-chains | 2908457 [2908457, 2908457] | 292160.5 | n/a | n/a | 10.0 | 33.91 | 33.89 | 30.85 | 92.3 (io/miss 1.00) | 246.8 | 5000 |
| F csr-paged | 159135 [159135, 159135] | 15985.4 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.66 | 93.7 (io/miss 1.00) | 13.3 | 5000 |
| G csr-paged-compressed | 131834 [131834, 131834] | 13243.0 | n/a | n/a | 10.0 | 2.01 | 2.01 | 1.63 | 78.9 (io/miss 1.00) | 13.0 | 5000 |

### Constrained 1%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 170521 [169968, 171050] | 16933.3 | n/a | n/a | 10.1 | 4.08 | 4.08 | 1.76 | 94.0 (io/miss 1.00) | 14.1 | 6000 |
| B btree-compressed | 102656 [102422, 104815] | 10203.9 | n/a | n/a | 10.1 | 4.02 | 4.02 | 1.07 | 92.8 (io/miss 1.00) | 8.6 | 6000 |
| C node-blocks | 181586 [180843, 182299] | 18161.8 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.90 | 93.5 (io/miss 1.00) | 15.2 | 6000 |
| D record-chains | 2917441 [2908677, 2927902] | 290464.2 | n/a | n/a | 10.1 | 34.13 | 34.11 | 31.01 | 92.1 (io/miss 1.00) | 248.1 | 5168 |
| F csr-paged | 156919 [156248, 158341] | 15566.5 | n/a | n/a | 10.1 | 2.02 | 2.02 | 1.65 | 93.5 (io/miss 1.00) | 13.2 | 6000 |
| G csr-paged-compressed | 131835 [130438, 132094] | 13005.9 | n/a | n/a | 10.1 | 2.01 | 2.01 | 1.61 | 79.5 (io/miss 1.00) | 12.9 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 164092 [163337, 164224] | 64655.2 | n/a | n/a | 2.5 | 4.03 | 4.03 | 1.70 | 93.8 (io/miss 1.00) | 13.6 | 6000 |
| B btree-compressed | 101123 [101069, 101281] | 39827.4 | n/a | n/a | 2.5 | 4.01 | 4.01 | 1.05 | 92.9 (io/miss 1.00) | 8.4 | 6000 |
| C node-blocks | 181158 [180679, 181960] | 71553.2 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.90 | 93.2 (io/miss 1.00) | 15.2 | 6000 |
| D record-chains | 2289604 [2260377, 2291522] | 901109.5 | n/a | n/a | 2.5 | 26.64 | 26.64 | 24.31 | 91.8 (io/miss 1.00) | 194.5 | 6000 |
| F csr-paged | 157045 [155253, 157811] | 61755.7 | n/a | n/a | 2.5 | 2.02 | 2.02 | 1.64 | 93.5 (io/miss 1.00) | 13.1 | 6000 |
| G csr-paged-compressed | 132224 [130637, 133559] | 52056.8 | n/a | n/a | 2.5 | 2.01 | 2.01 | 1.61 | 80.0 (io/miss 1.00) | 12.9 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 171352 [171132, 173032] | 15637.8 | n/a | n/a | 11.0 | 4.10 | 4.10 | 1.78 | 93.9 (io/miss 1.00) | 14.2 | 6000 |
| B btree-compressed | 103006 [102745, 105980] | 9312.1 | n/a | n/a | 11.0 | 4.03 | 4.03 | 1.07 | 93.2 (io/miss 1.00) | 8.6 | 6000 |
| C node-blocks | 181893 [181091, 182208] | 16467.0 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.90 | 93.3 (io/miss 1.00) | 15.2 | 6000 |
| D record-chains | 3083151 [3073015, 3101021] | 280286.2 | n/a | n/a | 11.0 | 36.01 | 35.98 | 32.81 | 92.0 (io/miss 1.00) | 262.5 | 4880 |
| F csr-paged | 156965 [156138, 158922] | 14347.1 | n/a | n/a | 11.0 | 2.02 | 2.02 | 1.65 | 93.4 (io/miss 1.00) | 13.2 | 6000 |
| G csr-paged-compressed | 131651 [130556, 132270] | 11901.7 | n/a | n/a | 11.0 | 2.01 | 2.01 | 1.62 | 79.3 (io/miss 1.00) | 12.9 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1836308 [1813798, 1885149] | 17021.8 | n/a | n/a | 108.1 | 44.18 | 29.59 | 18.96 | 94.6 (io/miss 1.00) | 151.7 | 300 |
| B btree-compressed | 1145955 [1102423, 1169046] | 10599.9 | n/a | n/a | 108.1 | 43.50 | 24.63 | 11.62 | 93.9 (io/miss 1.00) | 92.9 | 300 |
| C node-blocks | 1960796 [1947774, 1983235] | 18137.0 | n/a | n/a | 108.1 | 21.63 | 21.63 | 20.51 | 93.6 (io/miss 1.00) | 164.1 | 300 |
| D record-chains | 21210383 [21014712, 21292903] | 196555.9 | n/a | n/a | 108.1 | 239.49 | 229.24 | 225.33 | 91.9 (io/miss 1.00) | 1802.7 | 300 |
| F csr-paged | 1708538 [1690555, 1719908] | 15803.7 | n/a | n/a | 108.1 | 21.81 | 21.78 | 17.76 | 94.0 (io/miss 1.00) | 142.1 | 300 |
| G csr-paged-compressed | 1427228 [1419949, 1431793] | 13217.0 | n/a | n/a | 108.1 | 21.74 | 21.71 | 17.43 | 79.7 (io/miss 1.00) | 139.4 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 1994917 [1989712, 2060289] | 17049.8 | n/a | n/a | 117.8 | 47.96 | 31.89 | 20.74 | 94.4 (io/miss 1.00) | 166.0 | 90 |
| B btree-compressed | 1205019 [1194757, 1311142] | 10539.5 | n/a | n/a | 117.8 | 47.20 | 26.56 | 12.66 | 93.6 (io/miss 1.00) | 101.2 | 90 |
| C node-blocks | 2098544 [2095113, 2181961] | 17953.0 | n/a | n/a | 117.8 | 23.44 | 23.44 | 22.32 | 93.1 (io/miss 1.00) | 178.6 | 90 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 1891228 [1845644, 1891975] | 16142.7 | n/a | n/a | 117.8 | 23.71 | 23.67 | 19.46 | 94.3 (io/miss 1.00) | 155.6 | 90 |
| G csr-paged-compressed | 1557370 [1556003, 1564908] | 13333.4 | n/a | n/a | 117.8 | 23.54 | 23.50 | 18.99 | 79.9 (io/miss 1.00) | 151.9 | 90 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2813 [2737, 2857] | 11.10 | 3.04 |  |
| B btree-compressed | 4683 [4649, 4701] | 11.06 | 3.02 |  |
| C node-blocks | 870 [832, 901] | 9.33 | 5.00 |  |
| D record-chains | 1442 [1373, 1715] | 20.01 | 6.00 | existing records rewritten/insert 4.00 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2614 [2559, 2842] | 13.00 | 2.00 |
| B btree-compressed | 4965 [4935, 5061] | 11.01 | 2.01 |
| C node-blocks | 1381 [1374, 1384] | 9.00 | 1.99 |
| D record-chains | 815 [813, 826] | 4.00 | 1.00 |

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

RSS end: 5364 MiB, peak 5364 MiB
