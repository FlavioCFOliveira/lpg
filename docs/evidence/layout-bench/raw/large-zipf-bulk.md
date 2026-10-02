# layout-bench large zipf bulk n=10000000 m=100000000 alpha=0.8 arms=BG build cache 1024 MiB (131072 frames per arm)
graph generated in 62.8s: max out-degree 825603, out-degree p50 4 p99 83 p99.9 522
sort of edge indexes by (src, type, dst, id) and (dst, type, src, id): 68.2s (shared by all layouts, not included below)

### Bulk load (bounded build cache, dirty pages written back with O_DIRECT)

| layout | seconds | edges/s | pages | GiB written | peak RSS during build (MiB) | RSS before (MiB) |
|---|---|---|---|---|---|---|
| B btree-compressed | 53.2 | 1878525 | 585044 | 4.46 | 2725 | 1662 |
| G csr-paged-compressed | 28.0 | 3571681 | 201689 | 1.54 | 7224 | 2715 |

(F and G time includes building the in-memory CSR from the sorted indexes once, charged to the first of them.)

### Space

| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |
|---|---|---|---|---|---|
| B btree-compressed | 240894 | 19.7 | 344150 | 47.9 | leaves 239812, height 4, entries 200000000, leaf bytes used 17.7 B/edge (8.8 B/entry), leaf fill 0.90; edge table 344150 pages |
| G csr-paged-compressed | 201689 | 16.5 | 0 | 16.5 | offset pages 19532, entry pages 182157, entry stream 14.9 B/edge; read-only snapshot, no stamps |

RSS after build: 3729 MiB

## Constrained cache: 58504 frames = 457.1 MiB = 10% of the smallest MVCC layout
- B btree-compressed: cache = 10.00% of its 585044 pages
- G csr-paged-compressed: cache = 29.01% of its 201689 pages

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 6902 [6902, 6902] | 864.7 | n/a | n/a | 8.0 | 4.01 | 4.01 | 1.21 | 0.9 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 8400 [8400, 8400] | 1052.5 | n/a | n/a | 8.0 | 1.97 | 1.97 | 1.71 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 10%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 5246 [5101, 6267] | 517.4 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.82 | 1.3 | 3.6 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2646 [2520, 7243] | 255.6 | n/a | n/a | 10.1 | 1.97 | 1.97 | 0.75 | 0.7 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 4969 [4876, 5901] | 2258.7 | n/a | n/a | 2.4 | 4.00 | 4.00 | 0.80 | 0.3 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 2504 [2349, 6536] | 1138.2 | n/a | n/a | 2.4 | 1.97 | 1.97 | 0.69 | 0.6 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 133190 [130786, 134577] | 9.4 | n/a | n/a | 14440.0 | 17.64 | 17.64 | 3.19 | 75.0 | 3.4 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 101283 [99384, 103003] | 7.1 | n/a | n/a | 14440.0 | 11.71 | 11.71 | 2.38 | 69.2 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 3461170 [3459002, 3931565] | 26.9 | n/a | n/a | 130087.7 | 162.16 | 146.03 | 18.64 | 57.7 | 3.5 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 2492880 [2319801, 2888735] | 19.8 | n/a | n/a | 130087.7 | 106.64 | 106.55 | 14.16 | 50.5 | 3.4 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 144956781 [124515500, 272843952] | 54.5 | n/a | n/a | 3401338.1 | 58904.83 | 11910.21 | 12893.37 | 35.5 | 3.3 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 97335790 [79259404, 181479997] | 36.2 | n/a | n/a | 3401338.1 | 30064.50 | 8032.78 | 7789.62 | 45.1 | 3.2 (io/miss 0.00) | 0.0 | 90 |

### constrained 10% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 121156 [121156, 121156] | 15179.4 | n/a | n/a | 8.0 | 4.01 | 4.01 | 1.21 | 0.9 | 96.5 (io/miss 1.00) | 9.7 | 5000 |
| G csr-paged-compressed | 163805 [163805, 163805] | 20522.9 | n/a | n/a | 8.0 | 1.97 | 1.97 | 1.71 | 0.5 | 93.8 (io/miss 1.00) | 13.7 | 5000 |

### Constrained 10%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 78697 [75494, 100479] | 7981.9 | n/a | n/a | 10.1 | 4.02 | 4.02 | 0.84 | 1.3 | 96.2 (io/miss 1.00) | 6.7 | 6000 |
| G csr-paged-compressed | 43952 [39478, 140668] | 4457.8 | n/a | n/a | 10.1 | 1.97 | 1.97 | 0.78 | 0.7 | 93.7 (io/miss 1.00) | 6.2 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 76575 [73565, 94005] | 33438.6 | n/a | n/a | 2.4 | 4.00 | 4.00 | 0.82 | 0.2 | 94.6 (io/miss 1.00) | 6.6 | 6000 |
| G csr-paged-compressed | 44748 [38271, 124897] | 17395.7 | n/a | n/a | 2.4 | 1.97 | 1.97 | 0.72 | 0.6 | 93.8 (io/miss 1.00) | 5.7 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 276566 [237668, 349965] | 18.7 | n/a | n/a | 14440.0 | 17.64 | 17.64 | 2.48 | 68.1 | 66.4 (io/miss 1.00) | 19.8 | 6000 |
| G csr-paged-compressed | 196742 [164450, 315572] | 13.3 | n/a | n/a | 14440.0 | 11.71 | 11.71 | 1.83 | 60.6 | 70.3 (io/miss 1.00) | 14.7 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 4829710 [4368065, 5262352] | 36.0 | n/a | n/a | 130087.7 | 162.16 | 146.03 | 16.77 | 53.7 | 79.8 (io/miss 1.00) | 134.1 | 300 |
| G csr-paged-compressed | 3536652 [3081656, 4092845] | 24.2 | n/a | n/a | 130087.7 | 106.64 | 106.55 | 12.62 | 45.3 | 83.1 (io/miss 1.00) | 101.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 1231075968 [914935535, 1709723520] | 338.9 | n/a | n/a | 3342962.5 | 57237.16 | 11950.63 | 12724.06 | 34.6 | 81.6 (io/miss 1.00) | 101792.5 | 62 |
| G csr-paged-compressed | 591898241 [318605067, 981395354] | 152.0 | n/a | n/a | 3342962.5 | 29223.55 | 8036.69 | 7753.52 | 43.1 | 59.4 (io/miss 1.00) | 62028.1 | 62 |

## Constrained cache: 5850 frames = 45.7 MiB = 1% of the smallest MVCC layout
- B btree-compressed: cache = 1.00% of its 585044 pages
- G csr-paged-compressed: cache = 2.90% of its 201689 pages

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 6923 [6923, 6923] | 867.3 | n/a | n/a | 8.0 | 4.01 | 4.01 | 1.21 | 0.9 | 3.4 (io/miss 0.00) | 0.0 | 5000 |
| G csr-paged-compressed | 8399 [8399, 8399] | 1052.3 | n/a | n/a | 8.0 | 1.97 | 1.97 | 1.73 | 0.5 | 3.2 (io/miss 0.00) | 0.0 | 5000 |

### Constrained 1%, misses served by the OS page cache (buffered pread)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 7221 [6080, 7707] | 649.4 | n/a | n/a | 10.1 | 4.02 | 4.02 | 1.19 | 1.2 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 8459 [8013, 8656] | 851.2 | n/a | n/a | 10.1 | 1.97 | 1.97 | 1.69 | 0.6 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 6178 [5794, 6259] | 2512.0 | n/a | n/a | 2.4 | 4.00 | 4.00 | 1.01 | 0.3 | 3.5 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 8082 [7875, 8093] | 3414.3 | n/a | n/a | 2.4 | 1.97 | 1.97 | 1.64 | 0.5 | 3.3 (io/miss 0.00) | 0.0 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 136401 [133158, 141345] | 9.5 | n/a | n/a | 14440.0 | 17.64 | 17.64 | 4.36 | 75.1 | 3.3 (io/miss 0.00) | 0.0 | 6000 |
| G csr-paged-compressed | 106715 [105787, 110502] | 7.5 | n/a | n/a | 14440.0 | 11.71 | 11.71 | 3.97 | 55.8 | 3.0 (io/miss 0.00) | 0.0 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 3501479 [3238270, 4018070] | 27.5 | n/a | n/a | 130087.7 | 162.16 | 146.03 | 35.38 | 63.6 | 3.4 (io/miss 0.00) | 0.0 | 300 |
| G csr-paged-compressed | 2570490 [2354624, 3002769] | 20.5 | n/a | n/a | 130087.7 | 106.64 | 106.55 | 29.05 | 38.6 | 3.4 (io/miss 0.00) | 0.0 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 156275619 [130108400, 293804581] | 58.6 | n/a | n/a | 3401338.1 | 58904.83 | 11910.21 | 15510.17 | 39.4 | 3.2 (io/miss 0.00) | 0.0 | 90 |
| G csr-paged-compressed | 124537692 [94020389, 233868717] | 44.8 | n/a | n/a | 3401338.1 | 30064.50 | 8032.78 | 14738.34 | 41.0 | 3.1 (io/miss 0.00) | 0.0 | 90 |

### constrained 1% warm-up (discarded)


constrained warm-up: 1-hop (5000 starts/round, 1 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 121141 [121141, 121141] | 15177.5 | n/a | n/a | 8.0 | 4.01 | 4.01 | 1.21 | 0.9 | 96.5 (io/miss 1.00) | 9.7 | 5000 |
| G csr-paged-compressed | 165363 [165363, 165363] | 20718.0 | n/a | n/a | 8.0 | 1.97 | 1.97 | 1.73 | 0.5 | 93.3 (io/miss 1.00) | 13.9 | 5000 |

### Constrained 1%, misses read from the device (pread, O_DIRECT)


1-hop OUT, all types, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 115106 [99838, 121979] | 10970.4 | n/a | n/a | 10.1 | 4.02 | 4.02 | 1.13 | 1.3 | 95.7 (io/miss 1.00) | 9.0 | 6000 |
| G csr-paged-compressed | 166983 [157908, 169832] | 16774.6 | n/a | n/a | 10.1 | 1.97 | 1.97 | 1.72 | 0.6 | 94.0 (io/miss 1.00) | 13.7 | 6000 |

1-hop OUT, type = 2, uniform start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 100076 [96223, 102018] | 41718.0 | n/a | n/a | 2.4 | 4.00 | 4.00 | 1.01 | 0.3 | 94.7 (io/miss 1.00) | 8.1 | 6000 |
| G csr-paged-compressed | 157634 [157047, 160293] | 68088.8 | n/a | n/a | 2.4 | 1.97 | 1.97 | 1.65 | 0.5 | 93.8 (io/miss 1.00) | 13.2 | 6000 |

1-hop OUT, all types, degree-weighted start (2000 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 382910 [370646, 391353] | 26.5 | n/a | n/a | 14440.0 | 17.64 | 17.64 | 4.38 | 75.3 | 57.6 (io/miss 1.00) | 35.0 | 6000 |
| G csr-paged-compressed | 383740 [380061, 388504] | 26.7 | n/a | n/a | 14440.0 | 11.71 | 11.71 | 3.97 | 55.8 | 69.0 (io/miss 1.00) | 31.8 | 6000 |

2-hop OUT (distinct), uniform start (100 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 5686681 [5268674, 6740152] | 46.1 | n/a | n/a | 130087.7 | 162.16 | 146.03 | 35.39 | 63.6 | 67.1 (io/miss 1.00) | 283.1 | 300 |
| G csr-paged-compressed | 4882694 [4382058, 5699425] | 39.0 | n/a | n/a | 130087.7 | 106.64 | 106.55 | 28.70 | 37.9 | 82.4 (io/miss 1.00) | 229.6 | 300 |

2-hop OUT (distinct), degree-weighted start (30 starts/round, 3 measured rounds, time-budgeted)

| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| B btree-compressed | 1435764520 [953256684, 2020219386] | 400.5 | n/a | n/a | 3342962.5 | 57237.16 | 11950.63 | 15242.58 | 37.7 | 77.8 (io/miss 1.00) | 121940.6 | 62 |
| G csr-paged-compressed | 1005682001 [516079948, 2058044980] | 252.8 | n/a | n/a | 3342962.5 | 29223.55 | 8036.69 | 14684.35 | 39.4 | 63.1 (io/miss 1.00) | 117474.8 | 62 |

RSS end: 2825 MiB, peak since last reset 7224 MiB
