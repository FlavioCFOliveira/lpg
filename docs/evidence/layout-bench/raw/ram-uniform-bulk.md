# layout-bench uniform bulk n=1000000 m=10000000 alpha=0 scale=1
clock: Instant::now() pair costs 2785 ns (checksum 1)
graph generated in 0.2s: max out-degree 27, max in-degree 29, nodes with out-degree 0: 43, nodes with total degree > 50: 0, out-degree p50 10 p90 14 p99 18 p99.9 21

### Build (sorted bulk load, created_ts = 0 (frozen))

| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |
|---|---|---|---|---|---|---|
| A | 3.56 | 2811273 | 0.02 | 0.02 | includes sort | 1389 |
| B | 3.37 | 2970846 | 0.01 | 0.01 | includes sort | 1842 |
| C | 3.77 | 2652563 | 0.40 | 0.02 | includes sort | 4139 |
| D | 9.00 | 1110994 | 16.90 | 0.01 | existing records rewritten/edge 3.90, densified nodes 0 | 4139 |

### CSR projection (E) built by one sequential pass, all pages resident

| source | seconds | edges/s | page fetches | digest |
|---|---|---|---|---|
| A btree-fixed | 0.27 | 36783186 | 123460 | 4b99dd115ab53ad0 |
| B btree-compressed | 0.36 | 27837808 | 23180 | 4b99dd115ab53ad0 |
| C node-blocks | 0.30 | 33415822 | 3999912 | 4b99dd115ab53ad0 |

### Paged CSR (F fixed, G compressed) written from the projection

| layout | seconds | pages |
|---|---|---|
| F csr-paged | 0.25 | 33702 |
| G csr-paged-compressed | 0.69 | 19362 |

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

RSS after build: 4577 MiB, peak 4645 MiB

## Constrained cache: 14425 frames = 112.7 MiB = 25% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 9.1% of its 158432 pages
- B btree-compressed: cache = 25.0% of its 57700 pages
- C node-blocks: cache = 9.0% of its 160180 pages
- D record-chains: cache = 17.3% of its 83250 pages
- F csr-paged: cache = 42.8% of its 33702 pages
- G csr-paged-compressed: cache = 74.5% of its 19362 pages

### constrained 25% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6434 [6434, 6434] | 646.3 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.15 | 5.5 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5170 [5170, 5170] | 519.3 | n/a | n/a | 10.0 | 3.01 | 3.01 | 0.92 | 1.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7401 [7401, 7401] | 743.4 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.51 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 52399 [52399, 52399] | 5263.6 | n/a | n/a | 10.0 | 33.91 | 24.99 | 10.57 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 5231 [5231, 5231] | 525.4 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 1.4 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4736 [4736, 4736] | 475.7 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 0.6 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 25%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5750 [5740, 6031] | 575.9 | n/a | n/a | 10.1 | 4.07 | 4.07 | 0.97 | 5.8 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 3259 [3011, 4715] | 321.5 | n/a | n/a | 10.1 | 3.01 | 3.01 | 0.51 | 1.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 4410 [4287, 5814] | 435.1 | n/a | n/a | 10.1 | 2.00 | 2.00 | 0.91 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 51241 [49720, 51836] | 5055.1 | n/a | n/a | 10.1 | 34.14 | 25.12 | 9.97 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 1243 [1147, 3677] | 122.6 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.33 | 0.9 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 496 [491, 2861] | 49.2 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.17 | 0.6 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5543 [5540, 5762] | 2199.5 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.92 | 1.8 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 3186 [3096, 4399] | 1264.3 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.50 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 4493 [4395, 5167] | 1769.0 | n/a | n/a | 2.5 | 2.00 | 2.00 | 0.87 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 49402 [48157, 50224] | 19426.7 | n/a | n/a | 2.5 | 26.64 | 17.64 | 9.88 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 1289 [1220, 3311] | 511.6 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.31 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 493 [485, 2370] | 194.0 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.13 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5838 [5722, 5867] | 527.6 | n/a | n/a | 11.0 | 4.07 | 4.07 | 0.96 | 6.1 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 3265 [3202, 4263] | 295.2 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.47 | 0.7 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 4552 [4424, 4958] | 416.1 | n/a | n/a | 11.0 | 2.00 | 2.00 | 0.85 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 50564 [49922, 50933] | 4569.7 | n/a | n/a | 11.0 | 36.05 | 26.07 | 10.00 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 1288 [1281, 2983] | 117.1 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.28 | 0.5 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 507 [506, 1948] | 46.3 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.10 | 0.3 | 3.2 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 65010 [64658, 65598] | 602.6 | n/a | n/a | 108.1 | 44.06 | 26.23 | 10.41 | 6.2 | 3.6 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 38467 [36689, 48602] | 355.8 | n/a | n/a | 108.1 | 32.55 | 22.18 | 4.95 | 0.7 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 50024 [49304, 51550] | 463.7 | n/a | n/a | 108.1 | 21.63 | 21.63 | 8.94 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 500229 [500123, 503342] | 4635.9 | n/a | n/a | 108.1 | 239.49 | 132.88 | 99.29 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 14830 [14274, 31927] | 137.5 | n/a | n/a | 108.1 | 21.81 | 21.78 | 2.96 | 0.5 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 6666 [6488, 21135] | 61.7 | n/a | n/a | 108.1 | 21.74 | 21.71 | 0.99 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 71323 [69623, 72303] | 608.9 | n/a | n/a | 117.8 | 47.62 | 28.02 | 11.33 | 5.4 | 3.6 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 43651 [41149, 52524] | 359.9 | n/a | n/a | 117.8 | 35.33 | 23.99 | 5.61 | 0.8 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 56381 [55364, 57187] | 483.1 | n/a | n/a | 117.8 | 23.44 | 23.44 | 9.80 | 0.0 | 3.5 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 559313 [534780, 566968] | 4677.4 | n/a | n/a | 117.8 | 261.19 | 144.87 | 108.56 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 18783 [17563, 35391] | 164.3 | n/a | n/a | 117.8 | 23.71 | 23.67 | 3.34 | 1.0 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 7482 [7124, 21239] | 62.3 | n/a | n/a | 117.8 | 23.54 | 23.50 | 0.97 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 630289 [610852, 725617] | 593.5 | 597289 | 1070818 | 1099.2 | 447.60 | 220.30 | 105.27 | 5.6 | 3.6 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 431358 [351166, 455286] | 352.8 | 424849 | 695836 | 1099.2 | 331.53 | 178.00 | 50.17 | 0.7 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 503177 [465979, 568325] | 464.8 | 473948 | 852003 | 1099.2 | 220.07 | 218.00 | 91.17 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 161349 [123166, 282434] | 132.0 | 165526 | 285305 | 1099.2 | 221.77 | 214.77 | 27.10 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 71036 [70219, 174661] | 70.3 | 73404 | 123969 | 1099.2 | 221.00 | 213.57 | 8.03 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5467338797 [5453070891, 5480941134] | 546.8 | 5467338797 | 5467338797 | 9999533.0 | 4068048.00 | 124015.00 | 960541.67 | 5.7 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 3023216304 [3007764351, 3031957788] | 302.3 | 3023216304 | 3023216304 | 9999533.0 | 3012777.00 | 23283.00 | 396638.67 | 0.5 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 4234104855 [4229654364, 4243727351] | 423.4 | 4234104855 | 4234104855 | 9999533.0 | 1999867.00 | 64832.00 | 812642.67 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 1063251369 [1057393026, 1069320088] | 106.3 | 1063251369 | 1063251369 | 9999533.0 | 2015147.00 | 16851.00 | 161477.33 | 0.2 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 286160141 [285742764, 293996135] | 28.6 | 286160141 | 286160141 | 9999533.0 | 2009418.00 | 9681.00 | 617.33 | 0.1 | 3.2 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 5770 frames = 45.1 MiB = 10% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 3.6% of its 158432 pages
- B btree-compressed: cache = 10.0% of its 57700 pages
- C node-blocks: cache = 3.6% of its 160180 pages
- D record-chains: cache = 6.9% of its 83250 pages
- F csr-paged: cache = 17.1% of its 33702 pages
- G csr-paged-compressed: cache = 29.8% of its 19362 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6471 [6471, 6471] | 650.0 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.15 | 5.5 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5140 [5140, 5140] | 516.3 | n/a | n/a | 10.0 | 3.01 | 3.01 | 0.92 | 1.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 7470 [7470, 7470] | 750.4 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.53 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 72471 [72471, 72471] | 7279.9 | n/a | n/a | 10.0 | 33.91 | 24.99 | 15.00 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 5227 [5227, 5227] | 525.1 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.06 | 1.4 | 3.2 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 4736 [4736, 4736] | 475.8 | n/a | n/a | 10.0 | 2.01 | 2.01 | 0.95 | 0.6 | 3.1 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6058 [6018, 6142] | 599.5 | n/a | n/a | 10.1 | 4.07 | 4.07 | 1.02 | 6.1 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4688 [4662, 4736] | 467.0 | n/a | n/a | 10.1 | 3.01 | 3.01 | 0.77 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6736 [6635, 6767] | 664.5 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.31 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 73711 [73046, 75599] | 7343.6 | n/a | n/a | 10.1 | 34.14 | 25.12 | 15.12 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3765 [3739, 3791] | 374.5 | n/a | n/a | 10.1 | 2.02 | 2.02 | 0.70 | 1.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2709 [2613, 2869] | 267.2 | n/a | n/a | 10.1 | 2.01 | 2.01 | 0.48 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5842 [5800, 5874] | 2312.6 | n/a | n/a | 2.5 | 4.02 | 4.02 | 0.98 | 1.8 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4560 [4542, 4583] | 1795.4 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.76 | 0.4 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6779 [6774, 6833] | 2688.1 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.33 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 64959 [63046, 65829] | 25544.3 | n/a | n/a | 2.5 | 26.64 | 17.64 | 13.34 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3770 [3747, 3771] | 1484.5 | n/a | n/a | 2.5 | 2.02 | 2.02 | 0.71 | 1.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2623 [2618, 2666] | 1031.4 | n/a | n/a | 2.5 | 2.01 | 2.01 | 0.46 | 0.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 6103 [6057, 6238] | 551.6 | n/a | n/a | 11.0 | 4.07 | 4.07 | 1.03 | 6.6 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 4737 [4680, 4773] | 428.2 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.76 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 6737 [6646, 6789] | 608.8 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.32 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 74629 [72457, 74884] | 6744.6 | n/a | n/a | 11.0 | 36.05 | 26.07 | 15.20 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 3757 [3748, 3769] | 340.6 | n/a | n/a | 11.0 | 2.02 | 2.02 | 0.70 | 1.3 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2598 [2578, 2753] | 237.5 | n/a | n/a | 11.0 | 2.01 | 2.01 | 0.45 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 68601 [67720, 68785] | 633.3 | n/a | n/a | 108.1 | 44.06 | 26.23 | 11.13 | 6.7 | 3.6 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 54408 [53572, 54487] | 503.0 | n/a | n/a | 108.1 | 32.55 | 22.18 | 8.18 | 0.8 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 75408 [74264, 75839] | 699.0 | n/a | n/a | 108.1 | 21.63 | 21.63 | 14.37 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 583668 [578348, 591954] | 5398.8 | n/a | n/a | 108.1 | 239.49 | 132.88 | 118.38 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 41973 [41940, 43306] | 388.8 | n/a | n/a | 108.1 | 21.81 | 21.78 | 7.67 | 1.0 | 3.3 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 31082 [29644, 31136] | 286.9 | n/a | n/a | 108.1 | 21.74 | 21.71 | 5.05 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 74154 [72058, 74775] | 630.2 | n/a | n/a | 117.8 | 47.62 | 28.02 | 12.10 | 6.0 | 3.6 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 61491 [55443, 63275] | 517.2 | n/a | n/a | 117.8 | 35.33 | 23.99 | 9.11 | 1.3 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 83850 [80591, 84274] | 704.9 | n/a | n/a | 117.8 | 23.44 | 23.44 | 15.76 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 638298 [625528, 661897] | 5410.6 | n/a | n/a | 117.8 | 261.19 | 144.87 | 130.56 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 46899 [45830, 49565] | 401.9 | n/a | n/a | 117.8 | 23.71 | 23.67 | 8.50 | 1.3 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 32574 [30912, 33291] | 270.4 | n/a | n/a | 117.8 | 23.54 | 23.50 | 5.34 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 648029 [626706, 797715] | 620.0 | 602247 | 1085554 | 1099.2 | 447.60 | 220.30 | 112.43 | 5.9 | 3.7 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 526197 [492260, 609371] | 494.5 | 524304 | 880358 | 1099.2 | 331.53 | 178.00 | 84.93 | 1.1 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 717550 [702129, 833019] | 681.3 | 692204 | 1224958 | 1099.2 | 220.07 | 218.00 | 145.10 | 0.0 | 3.4 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 418556 [387365, 477880] | 390.8 | 439796 | 704636 | 1099.2 | 221.77 | 214.77 | 79.13 | 1.0 | 3.3 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 287176 [276902, 320379] | 269.9 | 306676 | 496926 | 1099.2 | 221.00 | 213.57 | 50.77 | 0.2 | 3.3 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 5760902773 [5695981620, 5761428288] | 576.1 | 5760902773 | 5760902773 | 9999533.0 | 4068048.00 | 124015.00 | 1027703.33 | 6.1 | 3.4 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 4554181055 [4521907815, 4580304868] | 455.4 | 4554181055 | 4554181055 | 9999533.0 | 3012777.00 | 23283.00 | 769923.33 | 1.0 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 6491717142 [6477047569, 6549767728] | 649.2 | 6491717142 | 6491717142 | 9999533.0 | 1999867.00 | 64832.00 | 1325999.00 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 3579130165 [3571135653, 3605643211] | 357.9 | 3579130165 | 3579130165 | 9999533.0 | 2015147.00 | 16851.00 | 711277.00 | 1.0 | 3.1 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 2500951837 [2479176654, 2504606700] | 250.1 | 2500951837 | 2500951837 | 9999533.0 | 2009418.00 | 9681.00 | 456297.67 | 0.4 | 3.1 (io/miss 0.00) | 0.0 | 3 |

## Constrained cache: 577 frames = 4.5 MiB = 1% of the smallest MVCC layout (A-D)
- A btree-fixed: cache = 0.4% of its 158432 pages
- B btree-compressed: cache = 1.0% of its 57700 pages
- C node-blocks: cache = 0.4% of its 160180 pages
- D record-chains: cache = 0.7% of its 83250 pages
- F csr-paged: cache = 1.7% of its 33702 pages
- G csr-paged-compressed: cache = 3.0% of its 19362 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8186 [8186, 8186] | 822.3 | n/a | n/a | 10.0 | 4.07 | 4.07 | 1.64 | 4.0 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| B btree-compressed | 5287 [5287, 5287] | 531.0 | n/a | n/a | 10.0 | 3.01 | 3.01 | 1.01 | 1.1 | 3.1 (io/miss 0.00) | 0.0 | 5000 |
| C node-blocks | 8688 [8688, 8688] | 872.7 | n/a | n/a | 10.0 | 2.00 | 2.00 | 1.92 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| D record-chains | 98395 [98395, 98395] | 9884.0 | n/a | n/a | 10.0 | 33.91 | 24.99 | 22.05 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 5000 |
| F csr-paged | 7803 [7803, 7803] | 783.8 | n/a | n/a | 10.0 | 2.02 | 2.02 | 1.73 | 1.0 | 3.0 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 7623 [7623, 7623] | 765.8 | n/a | n/a | 10.0 | 2.01 | 2.01 | 1.71 | 0.5 | 2.9 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 1%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8164 [8073, 8418] | 805.5 | n/a | n/a | 10.1 | 4.07 | 4.07 | 1.63 | 4.1 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5498 [5368, 6085] | 550.7 | n/a | n/a | 10.1 | 3.01 | 3.01 | 1.00 | 1.4 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 9145 [9043, 9155] | 912.1 | n/a | n/a | 10.1 | 2.00 | 2.00 | 1.92 | 0.0 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 99101 [98694, 99249] | 9873.0 | n/a | n/a | 10.1 | 34.14 | 25.12 | 22.15 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7651 [7633, 8110] | 762.3 | n/a | n/a | 10.1 | 2.02 | 2.02 | 1.72 | 0.9 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7696 [7611, 7930] | 759.3 | n/a | n/a | 10.1 | 2.01 | 2.01 | 1.70 | 0.4 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7902 [7773, 8078] | 3111.0 | n/a | n/a | 2.5 | 4.02 | 4.02 | 1.58 | 1.3 | 3.2 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5223 [5191, 5361] | 2072.7 | n/a | n/a | 2.5 | 3.00 | 3.00 | 0.98 | 0.4 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 8835 [8833, 8844] | 3478.3 | n/a | n/a | 2.5 | 2.00 | 2.00 | 1.92 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 70175 [69737, 70284] | 27670.7 | n/a | n/a | 2.5 | 26.64 | 17.64 | 15.40 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7739 [7699, 7781] | 3055.3 | n/a | n/a | 2.5 | 2.02 | 2.02 | 1.72 | 0.9 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7695 [7634, 7813] | 3029.4 | n/a | n/a | 2.5 | 2.01 | 2.01 | 1.70 | 0.6 | 2.9 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 8101 [8047, 8176] | 735.5 | n/a | n/a | 11.0 | 4.07 | 4.07 | 1.63 | 4.5 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| B btree-compressed | 5292 [5264, 5295] | 478.3 | n/a | n/a | 11.0 | 3.01 | 3.01 | 0.99 | 1.5 | 3.1 (io/miss 0.00) | 0.0 | 6000 |
| C node-blocks | 8807 [8804, 8841] | 799.0 | n/a | n/a | 11.0 | 2.00 | 2.00 | 1.92 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| D record-chains | 102662 [102513, 102761] | 9281.0 | n/a | n/a | 11.0 | 36.05 | 26.07 | 22.99 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 6000 |
| F csr-paged | 7953 [7786, 8128] | 718.7 | n/a | n/a | 11.0 | 2.02 | 2.02 | 1.73 | 0.9 | 3.0 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 7690 [7625, 7717] | 697.6 | n/a | n/a | 11.0 | 2.01 | 2.01 | 1.70 | 0.4 | 2.9 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 91512 [90862, 92099] | 844.8 | n/a | n/a | 108.1 | 44.06 | 26.23 | 17.61 | 4.6 | 3.2 (io/miss 0.00) | 0.0 | 300 |
| B btree-compressed | 60865 [60712, 61425] | 562.8 | n/a | n/a | 108.1 | 32.55 | 22.18 | 10.71 | 1.0 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| C node-blocks | 98384 [98338, 99090] | 912.0 | n/a | n/a | 108.1 | 21.63 | 21.63 | 20.73 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 300 |
| D record-chains | 603008 [592509, 604223] | 5577.6 | n/a | n/a | 108.1 | 239.49 | 132.88 | 129.97 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| F csr-paged | 87089 [86750, 87400] | 803.9 | n/a | n/a | 108.1 | 21.81 | 21.78 | 18.63 | 0.9 | 3.0 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 87194 [86956, 88317] | 804.9 | n/a | n/a | 108.1 | 21.74 | 21.71 | 18.38 | 0.5 | 3.0 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 101669 [101532, 106691] | 872.1 | n/a | n/a | 117.8 | 47.62 | 28.02 | 19.10 | 3.8 | 3.4 (io/miss 0.00) | 0.0 | 90 |
| B btree-compressed | 67942 [65458, 71779] | 582.2 | n/a | n/a | 117.8 | 35.33 | 23.99 | 11.63 | 1.4 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| C node-blocks | 110337 [108676, 113540] | 931.2 | n/a | n/a | 117.8 | 23.44 | 23.44 | 22.53 | 0.0 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| D record-chains | 657684 [648970, 678523] | 5561.0 | n/a | n/a | 117.8 | 261.19 | 144.87 | 141.98 | 0.0 | 3.0 (io/miss 0.00) | 0.0 | 90 |
| F csr-paged | 96028 [94815, 98538] | 822.9 | n/a | n/a | 117.8 | 23.71 | 23.67 | 20.24 | 1.2 | 3.1 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 96140 [95125, 99020] | 823.8 | n/a | n/a | 117.8 | 23.54 | 23.50 | 19.84 | 0.4 | 3.2 (io/miss 0.00) | 0.0 | 90 |

3-hop OUT (distinct), uniform start (10 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 886868 [857661, 1032008] | 844.0 | 855494 | 1436441 | 1099.2 | 447.60 | 220.30 | 178.63 | 4.1 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| B btree-compressed | 598560 [576658, 692477] | 566.4 | 600013 | 1000696 | 1099.2 | 331.53 | 178.00 | 109.03 | 1.2 | 3.2 (io/miss 0.00) | 0.0 | 30 |
| C node-blocks | 957457 [918242, 1119974] | 908.4 | 927222 | 1651483 | 1099.2 | 220.07 | 218.00 | 210.50 | 0.0 | 3.1 (io/miss 0.00) | 0.0 | 30 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 843552 [814254, 987700] | 805.6 | 824066 | 1449152 | 1099.2 | 221.77 | 214.77 | 188.63 | 0.9 | 3.0 (io/miss 0.00) | 0.0 | 30 |
| G csr-paged-compressed | 842190 [807130, 998421] | 798.5 | 830771 | 1454599 | 1099.2 | 221.00 | 213.57 | 185.47 | 0.5 | 3.0 (io/miss 0.00) | 0.0 | 30 |

BFS OUT (unbounded depth), degree-weighted start (1 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A btree-fixed | 7913994231 [7903611724, 7941518959] | 791.4 | 7913994231 | 7913994231 | 9999533.0 | 4068048.00 | 124015.00 | 1628613.33 | 4.2 | 3.0 (io/miss 0.00) | 0.0 | 3 |
| B btree-compressed | 5143129117 [5129776245, 5160023490] | 514.3 | 5143129117 | 5143129117 | 9999533.0 | 3012777.00 | 23283.00 | 992710.33 | 1.3 | 2.9 (io/miss 0.00) | 0.0 | 3 |
| C node-blocks | 8633722810 [8624448798, 8643180925] | 863.4 | 8633722810 | 8633722810 | 9999533.0 | 1999867.00 | 64832.00 | 1922313.33 | 0.0 | 2.9 (io/miss 0.00) | 0.0 | 3 |
| D record-chains | not run | - | - | - | - | - | - | - | - | - | - | 0 |
| F csr-paged | 7567311013 [7549517804, 7576746859] | 756.8 | 7567311013 | 7567311013 | 9999533.0 | 2015147.00 | 16851.00 | 1722773.00 | 0.8 | 2.8 (io/miss 0.00) | 0.0 | 3 |
| G csr-paged-compressed | 7555075557 [7550495835, 7571269027] | 755.5 | 7555075557 | 7555075557 | 9999533.0 | 2009418.00 | 9681.00 | 1699037.33 | 0.5 | 2.8 (io/miss 0.00) | 0.0 | 3 |

### Online single-edge insert on the built graph (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |
|---|---|---|---|---|
| A btree-fixed | 2292 [2271, 2418] | 11.02 | 3.01 |  |
| B btree-compressed | 3864 [3830, 3878] | 9.02 | 3.01 |  |
| C node-blocks | 793 [778, 793] | 9.33 | 5.00 |  |
| D record-chains | 1371 [1345, 1390] | 20.01 | 6.00 | existing records rewritten/insert 4.00 |

### Delete (set deleted_ts) of random existing edges by edge id (all resident; 3 rounds x 50000 edges, interleaved)

| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |
|---|---|---|---|
| A btree-fixed | 2338 [2335, 2370] | 13.00 | 2.00 |
| B btree-compressed | 4501 [4494, 4506] | 9.00 | 2.00 |
| C node-blocks | 1290 [1285, 1294] | 9.00 | 2.00 |
| D record-chains | 811 [807, 826] | 4.00 | 1.00 |

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

RSS end: 4746 MiB, peak 4746 MiB
