# lab::btree

- date: 2026-10-02T20:24:21+01:00
- commit: 
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: M-LAYOUT graph, 1000000 nodes, 10000000 edges, 2 entries per edge, 29-byte keys, empty values
- cache share: pool page buffers as % of the data pages (`cap_for`)
- cold: OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept
- put: one key per batch (`apply`), new edges of the same distribution

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| get (zipf) | zipf, 100 %, warm OS cache | 200000 | 1435 [1389, 1440] | 0.00 | 0.00 | 562372 | 608388 | - |
| prefix scan (zipf) | zipf, 100 %, warm OS cache | 50000 | 21693 [21593, 22013] | 0.00 | 0.00 | 563076 | 608388 | - |
| put (zipf) | zipf, 100 %, warm OS cache | 20000 | 3820 [3753, 4015] | 0.00 | 0.00 | 580284 | 608388 | page_cache.evictions +108; page_cache.flushes +6; store.size.reserved_unused =34406400 |
| get (zipf) | zipf, 100 %, cold | 5000 | 1937 [1719, 2078] | 0.01 | 0.00 | 580284 | 608388 | page_cache.evictions +24 |
| prefix scan (zipf) | zipf, 100 %, cold | 2000 | 21590 [20179, 21621] | 0.01 | 0.00 | 580284 | 608388 | page_cache.evictions +9 |
| put (zipf) | zipf, 100 %, cold | 2000 | 3925 [3812, 4200] | 0.00 | 0.00 | 580284 | 608388 | page_cache.evictions +9; store.size.reserved_unused =34332672 |
| get (zipf) | zipf, 10 %, warm OS cache | 200000 | 3751 [3742, 3771] | 0.00 | 0.00 | 580284 | 608388 | page_cache.evictions +537983 |
| prefix scan (zipf) | zipf, 10 %, warm OS cache | 50000 | 25133 [24967, 25260] | 0.00 | 0.00 | 580284 | 608388 | page_cache.evictions +192158 |
| put (zipf) | zipf, 10 %, warm OS cache | 20000 | 9209 [8737, 9636] | 0.00 | 2.63 | 580284 | 608388 | page_cache.evictions +54048; page_cache.flushes +48600; store.size.reserved_unused =33595392 |
| get (zipf) | zipf, 10 %, cold | 5000 | 81629 [81551, 82634] | 7.20 | 0.00 | 580284 | 608388 | page_cache.evictions +13402 |
| prefix scan (zipf) | zipf, 10 %, cold | 2000 | 153334 [139415, 204075] | 12.66 | 0.00 | 580284 | 608388 | page_cache.evictions +9484 |
| put (zipf) | zipf, 10 %, cold | 2000 | 47186 [13501, 65810] | 4.25 | 0.00 | 580284 | 608388 | page_cache.evictions +3192; store.size.reserved_unused =33570816 |
| get (zipf) | zipf, 1 %, warm OS cache | 200000 | 3923 [3908, 3939] | 0.00 | 0.00 | 580284 | 608388 | page_cache.evictions +685901 |
| prefix scan (zipf) | zipf, 1 %, warm OS cache | 50000 | 30216 [30162, 30571] | 0.00 | 0.00 | 580284 | 608388 | page_cache.evictions +537021 |
| put (zipf) | zipf, 1 %, warm OS cache | 20000 | 10402 [9904, 11190] | 0.00 | 2.64 | 580284 | 608388 | page_cache.evictions +77072; page_cache.flushes +59337; store.size.reserved_unused =32907264 |
| get (zipf) | zipf, 1 %, cold | 5000 | 92386 [92316, 92766] | 8.13 | 0.00 | 580284 | 608388 | page_cache.evictions +17156 |
| prefix scan (zipf) | zipf, 1 %, cold | 2000 | 233702 [226008, 235844] | 18.07 | 0.00 | 580284 | 608388 | page_cache.evictions +20845 |
| put (zipf) | zipf, 1 %, cold | 2000 | 91304 [88298, 91938] | 8.56 | 7.21 | 580284 | 608388 | page_cache.evictions +7735; page_cache.flushes +5545; store.size.reserved_unused =32825344 |
| get (uniform) | uniform, 100 %, warm OS cache | 200000 | 1419 [1412, 1419] | 0.00 | 0.00 | 584092 | 629828 | - |
| prefix scan (uniform) | uniform, 100 %, warm OS cache | 50000 | 1592 [1584, 1596] | 0.00 | 0.00 | 584680 | 629828 | - |
| put (uniform) | uniform, 100 %, warm OS cache | 20000 | 3722 [3658, 3915] | 0.00 | 0.00 | 594192 | 629828 | page_cache.evictions +125; page_cache.flushes +4; store.size.reserved_unused =12558336 |
| get (uniform) | uniform, 100 %, cold | 5000 | 1744 [1567, 2125] | 0.01 | 0.00 | 594192 | 629828 | page_cache.evictions +20 |
| prefix scan (uniform) | uniform, 100 %, cold | 2000 | 1834 [1754, 1890] | 0.01 | 0.00 | 594192 | 629828 | page_cache.evictions +8 |
| put (uniform) | uniform, 100 %, cold | 2000 | 3748 [3724, 4051] | 0.00 | 0.00 | 594192 | 629828 | page_cache.evictions +12; store.size.reserved_unused =12468224 |
| get (uniform) | uniform, 10 %, warm OS cache | 200000 | 3692 [3684, 3707] | 0.00 | 0.00 | 528840 | 629828 | page_cache.evictions +539044 |
| prefix scan (uniform) | uniform, 10 %, warm OS cache | 50000 | 3886 [3871, 3948] | 0.00 | 0.00 | 528840 | 629828 | page_cache.evictions +135352 |
| put (uniform) | uniform, 10 %, warm OS cache | 20000 | 9270 [8607, 9542] | 0.00 | 3.66 | 528840 | 629828 | page_cache.evictions +54221; page_cache.flushes +48423; store.size.reserved_unused =11689984 |
| get (uniform) | uniform, 10 %, cold | 5000 | 77856 [77752, 79089] | 7.18 | 0.00 | 528840 | 629828 | page_cache.evictions +13399 |
| prefix scan (uniform) | uniform, 10 %, cold | 2000 | 80636 [80108, 80931] | 7.22 | 0.00 | 528840 | 629828 | page_cache.evictions +5418 |
| put (uniform) | uniform, 10 %, cold | 2000 | 18085 [6175, 66687] | 3.09 | 0.00 | 528840 | 629828 | page_cache.evictions +2321; store.size.reserved_unused =11640832 |
| get (uniform) | uniform, 1 %, warm OS cache | 200000 | 3812 [3794, 3819] | 0.00 | 0.00 | 528708 | 629828 | page_cache.evictions +683447 |
| prefix scan (uniform) | uniform, 1 %, warm OS cache | 50000 | 4077 [4075, 4204] | 0.00 | 0.00 | 528708 | 629828 | page_cache.evictions +171789 |
| put (uniform) | uniform, 1 %, warm OS cache | 20000 | 10269 [10151, 10915] | 0.00 | 3.95 | 528708 | 629828 | page_cache.evictions +76644; page_cache.flushes +59395; store.size.reserved_unused =10862592 |
| get (uniform) | uniform, 1 %, cold | 5000 | 86923 [86797, 87241] | 8.11 | 0.00 | 528708 | 629828 | page_cache.evictions +17043 |
| prefix scan (uniform) | uniform, 1 %, cold | 2000 | 94599 [94334, 95848] | 8.65 | 0.00 | 528708 | 629828 | page_cache.evictions +6842 |
| put (uniform) | uniform, 1 %, cold | 2000 | 80145 [75112, 90113] | 7.99 | 7.13 | 528708 | 629828 | page_cache.evictions +7523; page_cache.flushes +5537; store.size.reserved_unused =10797056 |
