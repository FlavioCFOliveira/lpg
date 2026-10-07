# lab::btree

- date: 2026-10-02T19:53:44+01:00
- commit: 7f2f831dec4e-dirty
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: M-LAYOUT graph, 1000000 nodes, 10000000 edges, 2 entries per edge, 29-byte keys, empty values
- cache share: pool page buffers as % of the data pages (`cap_for`)
- cold: OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept
- put: one key per batch (`apply`), new edges of the same distribution

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| get (zipf) | zipf, 100 %, warm OS cache | 200000 | 1421 [1405, 1432] | 0.00 | 0.00 | 562388 | 607956 | - |
| prefix scan (zipf) | zipf, 100 %, warm OS cache | 50000 | 21718 [21554, 21926] | 0.00 | 0.00 | 563076 | 607956 | - |
| put (zipf) | zipf, 100 %, warm OS cache | 20000 | 5905 [5878, 6026] | 0.00 | 0.00 | 580296 | 607956 | page_cache.evictions +108; page_cache.flushes +6; store.size.reserved_unused =34406400 |
| get (zipf) | zipf, 100 %, cold | 5000 | 1738 [1594, 1944] | 0.01 | 0.00 | 580296 | 607956 | page_cache.evictions +24 |
| prefix scan (zipf) | zipf, 100 %, cold | 2000 | 21161 [20065, 21431] | 0.01 | 0.00 | 580296 | 607956 | page_cache.evictions +9 |
| put (zipf) | zipf, 100 %, cold | 2000 | 5830 [5778, 6538] | 0.00 | 0.00 | 580296 | 607956 | page_cache.evictions +9; store.size.reserved_unused =34332672 |
| get (zipf) | zipf, 10 %, warm OS cache | 200000 | 3618 [3616, 3630] | 0.00 | 0.00 | 580296 | 607956 | page_cache.evictions +537983 |
| prefix scan (zipf) | zipf, 10 %, warm OS cache | 50000 | 25210 [25090, 25434] | 0.00 | 0.00 | 580296 | 607956 | page_cache.evictions +192158 |
| put (zipf) | zipf, 10 %, warm OS cache | 20000 | 11132 [10815, 11515] | 0.00 | 2.63 | 580296 | 607956 | page_cache.evictions +54048; page_cache.flushes +48600; store.size.reserved_unused =33595392 |
| get (zipf) | zipf, 10 %, cold | 5000 | 80622 [79863, 81626] | 7.20 | 0.00 | 580296 | 607956 | page_cache.evictions +13402 |
| prefix scan (zipf) | zipf, 10 %, cold | 2000 | 155065 [139252, 200154] | 12.66 | 0.00 | 580296 | 607956 | page_cache.evictions +9484 |
| put (zipf) | zipf, 10 %, cold | 2000 | 46121 [14636, 68257] | 4.25 | 0.00 | 580296 | 607956 | page_cache.evictions +3192; store.size.reserved_unused =33570816 |
| get (zipf) | zipf, 1 %, warm OS cache | 200000 | 3803 [3801, 3807] | 0.00 | 0.00 | 580296 | 607956 | page_cache.evictions +685901 |
| prefix scan (zipf) | zipf, 1 %, warm OS cache | 50000 | 29558 [29442, 29764] | 0.00 | 0.00 | 580296 | 607956 | page_cache.evictions +537021 |
| put (zipf) | zipf, 1 %, warm OS cache | 20000 | 11978 [11969, 12660] | 0.00 | 2.64 | 580296 | 607956 | page_cache.evictions +77072; page_cache.flushes +59337; store.size.reserved_unused =32907264 |
| get (zipf) | zipf, 1 %, cold | 5000 | 91696 [91623, 92097] | 8.13 | 0.00 | 580296 | 607956 | page_cache.evictions +17156 |
| prefix scan (zipf) | zipf, 1 %, cold | 2000 | 231081 [223948, 231102] | 18.07 | 0.00 | 580296 | 607956 | page_cache.evictions +20845 |
| put (zipf) | zipf, 1 %, cold | 2000 | 89416 [86077, 92507] | 8.56 | 7.21 | 580296 | 607956 | page_cache.evictions +7735; page_cache.flushes +5545; store.size.reserved_unused =32825344 |
| get (uniform) | uniform, 100 %, warm OS cache | 200000 | 1417 [1403, 1436] | 0.00 | 0.00 | 594208 | 629388 | - |
| prefix scan (uniform) | uniform, 100 %, warm OS cache | 50000 | 1591 [1586, 1638] | 0.00 | 0.00 | 594208 | 629388 | - |
| put (uniform) | uniform, 100 %, warm OS cache | 20000 | 5799 [5798, 5888] | 0.00 | 0.00 | 594208 | 629388 | page_cache.evictions +125; page_cache.flushes +4; store.size.reserved_unused =12558336 |
| get (uniform) | uniform, 100 %, cold | 5000 | 1663 [1512, 1998] | 0.01 | 0.00 | 594208 | 629388 | page_cache.evictions +20 |
| prefix scan (uniform) | uniform, 100 %, cold | 2000 | 1806 [1678, 1815] | 0.01 | 0.00 | 594208 | 629388 | page_cache.evictions +8 |
| put (uniform) | uniform, 100 %, cold | 2000 | 5674 [5557, 5893] | 0.00 | 0.00 | 594208 | 629388 | page_cache.evictions +12; store.size.reserved_unused =12468224 |
| get (uniform) | uniform, 10 %, warm OS cache | 200000 | 3668 [3659, 3681] | 0.00 | 0.00 | 594208 | 629388 | page_cache.evictions +539044 |
| prefix scan (uniform) | uniform, 10 %, warm OS cache | 50000 | 3874 [3823, 3875] | 0.00 | 0.00 | 594208 | 629388 | page_cache.evictions +135352 |
| put (uniform) | uniform, 10 %, warm OS cache | 20000 | 11224 [10766, 11536] | 0.00 | 3.66 | 594208 | 629388 | page_cache.evictions +54221; page_cache.flushes +48423; store.size.reserved_unused =11689984 |
| get (uniform) | uniform, 10 %, cold | 5000 | 78236 [78084, 79261] | 7.18 | 0.00 | 594208 | 629388 | page_cache.evictions +13399 |
| prefix scan (uniform) | uniform, 10 %, cold | 2000 | 80190 [79856, 80553] | 7.22 | 0.00 | 594208 | 629388 | page_cache.evictions +5418 |
| put (uniform) | uniform, 10 %, cold | 2000 | 20451 [7643, 69246] | 3.09 | 0.00 | 594208 | 629388 | page_cache.evictions +2321; store.size.reserved_unused =11640832 |
| get (uniform) | uniform, 1 %, warm OS cache | 200000 | 3796 [3780, 3832] | 0.00 | 0.00 | 594208 | 629388 | page_cache.evictions +683447 |
| prefix scan (uniform) | uniform, 1 %, warm OS cache | 50000 | 4050 [4037, 4126] | 0.00 | 0.00 | 594208 | 629388 | page_cache.evictions +171789 |
| put (uniform) | uniform, 1 %, warm OS cache | 20000 | 12160 [12159, 12830] | 0.00 | 3.95 | 594208 | 629388 | page_cache.evictions +76644; page_cache.flushes +59395; store.size.reserved_unused =10862592 |
| get (uniform) | uniform, 1 %, cold | 5000 | 87440 [87269, 87451] | 8.11 | 0.00 | 594208 | 629388 | page_cache.evictions +17043 |
| prefix scan (uniform) | uniform, 1 %, cold | 2000 | 94975 [94113, 95662] | 8.65 | 0.00 | 594208 | 629388 | page_cache.evictions +6842 |
| put (uniform) | uniform, 1 %, cold | 2000 | 81749 [75507, 91481] | 7.99 | 7.11 | 594208 | 629388 | page_cache.evictions +7523; page_cache.flushes +5537; store.size.reserved_unused =10797056 |
