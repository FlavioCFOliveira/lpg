# lab::btree

- date: 2026-10-02T20:22:55+01:00
- commit: 
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: M-LAYOUT graph, 1000000 nodes, 10000000 edges, 2 entries per edge, 29-byte keys, empty values
- cache share: pool page buffers as % of the data pages (`cap_for`)
- cold: OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept
- put: one key per batch (`apply`), new edges of the same distribution

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| get (zipf) | zipf, 100 %, warm OS cache | 200000 | 1428 [1398, 1433] | 0.00 | 0.00 | 562276 | 608016 | - |
| prefix scan (zipf) | zipf, 100 %, warm OS cache | 50000 | 22132 [21868, 22325] | 0.00 | 0.00 | 562980 | 608016 | - |
| put (zipf) | zipf, 100 %, warm OS cache | 20000 | 5759 [5756, 6022] | 0.00 | 0.00 | 580188 | 608016 | page_cache.evictions +108; page_cache.flushes +6; store.size.reserved_unused =34406400 |
| get (zipf) | zipf, 100 %, cold | 5000 | 1864 [1592, 2064] | 0.01 | 0.00 | 580188 | 608016 | page_cache.evictions +24 |
| prefix scan (zipf) | zipf, 100 %, cold | 2000 | 21267 [20088, 21622] | 0.01 | 0.00 | 580188 | 608016 | page_cache.evictions +9 |
| put (zipf) | zipf, 100 %, cold | 2000 | 5745 [5612, 6337] | 0.00 | 0.00 | 580188 | 608016 | page_cache.evictions +9; store.size.reserved_unused =34332672 |
| get (zipf) | zipf, 10 %, warm OS cache | 200000 | 3677 [3677, 3693] | 0.00 | 0.00 | 580188 | 608016 | page_cache.evictions +537983 |
| prefix scan (zipf) | zipf, 10 %, warm OS cache | 50000 | 25344 [25268, 25396] | 0.00 | 0.00 | 580188 | 608016 | page_cache.evictions +192158 |
| put (zipf) | zipf, 10 %, warm OS cache | 20000 | 11161 [10686, 11410] | 0.00 | 2.63 | 580188 | 608016 | page_cache.evictions +54048; page_cache.flushes +48600; store.size.reserved_unused =33595392 |
| get (zipf) | zipf, 10 %, cold | 5000 | 81116 [80411, 82422] | 7.20 | 0.00 | 580188 | 608016 | page_cache.evictions +13402 |
| prefix scan (zipf) | zipf, 10 %, cold | 2000 | 155572 [140434, 204587] | 12.66 | 0.00 | 580188 | 608016 | page_cache.evictions +9484 |
| put (zipf) | zipf, 10 %, cold | 2000 | 46136 [14584, 68060] | 4.25 | 0.00 | 580188 | 608016 | page_cache.evictions +3192; store.size.reserved_unused =33570816 |
| get (zipf) | zipf, 1 %, warm OS cache | 200000 | 3946 [3939, 3962] | 0.00 | 0.00 | 580188 | 608016 | page_cache.evictions +685901 |
| prefix scan (zipf) | zipf, 1 %, warm OS cache | 50000 | 30372 [30228, 30777] | 0.00 | 0.00 | 580188 | 608016 | page_cache.evictions +537021 |
| put (zipf) | zipf, 1 %, warm OS cache | 20000 | 12225 [11980, 12970] | 0.00 | 2.64 | 580188 | 608016 | page_cache.evictions +77072; page_cache.flushes +59337; store.size.reserved_unused =32907264 |
| get (zipf) | zipf, 1 %, cold | 5000 | 90763 [90555, 91371] | 8.13 | 0.00 | 580188 | 608016 | page_cache.evictions +17156 |
| prefix scan (zipf) | zipf, 1 %, cold | 2000 | 229082 [222726, 230699] | 18.07 | 0.00 | 580188 | 608016 | page_cache.evictions +20845 |
| put (zipf) | zipf, 1 %, cold | 2000 | 87882 [86405, 89748] | 8.56 | 7.21 | 580188 | 608016 | page_cache.evictions +7735; page_cache.flushes +5545; store.size.reserved_unused =32825344 |
| get (uniform) | uniform, 100 %, warm OS cache | 200000 | 1436 [1421, 1440] | 0.00 | 0.00 | 583968 | 629704 | - |
| prefix scan (uniform) | uniform, 100 %, warm OS cache | 50000 | 1617 [1607, 1629] | 0.00 | 0.00 | 584556 | 629704 | - |
| put (uniform) | uniform, 100 %, warm OS cache | 20000 | 5858 [5842, 5883] | 0.00 | 0.00 | 594068 | 629704 | page_cache.evictions +125; page_cache.flushes +4; store.size.reserved_unused =12558336 |
| get (uniform) | uniform, 100 %, cold | 5000 | 1781 [1656, 2082] | 0.01 | 0.00 | 594068 | 629704 | page_cache.evictions +20 |
| prefix scan (uniform) | uniform, 100 %, cold | 2000 | 1979 [1847, 2010] | 0.01 | 0.00 | 594068 | 629704 | page_cache.evictions +8 |
| put (uniform) | uniform, 100 %, cold | 2000 | 5747 [5636, 6000] | 0.00 | 0.00 | 594068 | 629704 | page_cache.evictions +12; store.size.reserved_unused =12468224 |
| get (uniform) | uniform, 10 %, warm OS cache | 200000 | 3662 [3658, 3678] | 0.00 | 0.00 | 528616 | 629704 | page_cache.evictions +539044 |
| prefix scan (uniform) | uniform, 10 %, warm OS cache | 50000 | 3851 [3844, 3879] | 0.00 | 0.00 | 528616 | 629704 | page_cache.evictions +135352 |
| put (uniform) | uniform, 10 %, warm OS cache | 20000 | 11523 [10545, 11636] | 0.00 | 3.66 | 528616 | 629704 | page_cache.evictions +54221; page_cache.flushes +48423; store.size.reserved_unused =11689984 |
| get (uniform) | uniform, 10 %, cold | 5000 | 78402 [77792, 78507] | 7.18 | 0.00 | 528616 | 629704 | page_cache.evictions +13399 |
| prefix scan (uniform) | uniform, 10 %, cold | 2000 | 79794 [79721, 79913] | 7.22 | 0.00 | 528616 | 629704 | page_cache.evictions +5418 |
| put (uniform) | uniform, 10 %, cold | 2000 | 19942 [7815, 69897] | 3.09 | 0.00 | 528616 | 629704 | page_cache.evictions +2321; store.size.reserved_unused =11640832 |
| get (uniform) | uniform, 1 %, warm OS cache | 200000 | 3845 [3832, 3856] | 0.00 | 0.00 | 528616 | 629704 | page_cache.evictions +683447 |
| prefix scan (uniform) | uniform, 1 %, warm OS cache | 50000 | 4150 [4082, 4211] | 0.00 | 0.00 | 528616 | 629704 | page_cache.evictions +171789 |
| put (uniform) | uniform, 1 %, warm OS cache | 20000 | 12287 [12188, 12952] | 0.00 | 3.95 | 528616 | 629704 | page_cache.evictions +76644; page_cache.flushes +59395; store.size.reserved_unused =10862592 |
| get (uniform) | uniform, 1 %, cold | 5000 | 86998 [86876, 88785] | 8.11 | 0.00 | 528616 | 629704 | page_cache.evictions +17043 |
| prefix scan (uniform) | uniform, 1 %, cold | 2000 | 95041 [94271, 95041] | 8.65 | 0.00 | 528616 | 629704 | page_cache.evictions +6842 |
| put (uniform) | uniform, 1 %, cold | 2000 | 82801 [77256, 92263] | 7.99 | 7.11 | 528616 | 629704 | page_cache.evictions +7523; page_cache.flushes +5537; store.size.reserved_unused =10797056 |
