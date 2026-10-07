# lab::btree

- date: 2026-10-02T20:27:15+01:00
- commit: 
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: M-LAYOUT graph, 1000000 nodes, 10000000 edges, 2 entries per edge, 29-byte keys, empty values
- cache share: pool page buffers as % of the data pages (`cap_for`)
- cold: OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept
- put: one key per batch (`apply`), new edges of the same distribution

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| get (zipf) | zipf, 100 %, warm OS cache | 200000 | 1420 [1398, 1434] | 0.00 | 0.00 | 562188 | 607912 | - |
| prefix scan (zipf) | zipf, 100 %, warm OS cache | 50000 | 21657 [21533, 21932] | 0.00 | 0.00 | 562892 | 607912 | - |
| put (zipf) | zipf, 100 %, warm OS cache | 20000 | 3792 [3678, 3963] | 0.00 | 0.00 | 580100 | 607912 | page_cache.evictions +108; page_cache.flushes +6; store.size.reserved_unused =34406400 |
| get (zipf) | zipf, 100 %, cold | 5000 | 1931 [1696, 2123] | 0.01 | 0.00 | 580100 | 607912 | page_cache.evictions +24 |
| prefix scan (zipf) | zipf, 100 %, cold | 2000 | 21438 [19913, 21659] | 0.01 | 0.00 | 580100 | 607912 | page_cache.evictions +9 |
| put (zipf) | zipf, 100 %, cold | 2000 | 3753 [3552, 4217] | 0.00 | 0.00 | 580100 | 607912 | page_cache.evictions +9; store.size.reserved_unused =34332672 |
| get (zipf) | zipf, 10 %, warm OS cache | 200000 | 3723 [3714, 3738] | 0.00 | 0.00 | 580100 | 607912 | page_cache.evictions +537983 |
| prefix scan (zipf) | zipf, 10 %, warm OS cache | 50000 | 25196 [25190, 25527] | 0.00 | 0.00 | 580100 | 607912 | page_cache.evictions +192158 |
| put (zipf) | zipf, 10 %, warm OS cache | 20000 | 9320 [8646, 9507] | 0.00 | 2.63 | 580100 | 607912 | page_cache.evictions +54048; page_cache.flushes +48600; store.size.reserved_unused =33595392 |
| get (zipf) | zipf, 10 %, cold | 5000 | 81946 [81452, 82346] | 7.20 | 0.00 | 580100 | 607912 | page_cache.evictions +13402 |
| prefix scan (zipf) | zipf, 10 %, cold | 2000 | 157842 [141597, 204851] | 12.66 | 0.00 | 580100 | 607912 | page_cache.evictions +9484 |
| put (zipf) | zipf, 10 %, cold | 2000 | 46531 [14153, 65035] | 4.25 | 0.00 | 580100 | 607912 | page_cache.evictions +3192; store.size.reserved_unused =33570816 |
| get (zipf) | zipf, 1 %, warm OS cache | 200000 | 3932 [3930, 3972] | 0.00 | 0.00 | 580100 | 607912 | page_cache.evictions +685901 |
| prefix scan (zipf) | zipf, 1 %, warm OS cache | 50000 | 30088 [29928, 30374] | 0.00 | 0.00 | 580100 | 607912 | page_cache.evictions +537021 |
| put (zipf) | zipf, 1 %, warm OS cache | 20000 | 10249 [10022, 11001] | 0.00 | 2.64 | 580100 | 607912 | page_cache.evictions +77072; page_cache.flushes +59337; store.size.reserved_unused =32907264 |
| get (zipf) | zipf, 1 %, cold | 5000 | 92542 [91942, 92722] | 8.13 | 0.00 | 580100 | 607912 | page_cache.evictions +17156 |
| prefix scan (zipf) | zipf, 1 %, cold | 2000 | 231730 [226990, 235574] | 18.07 | 0.00 | 580100 | 607912 | page_cache.evictions +20845 |
| put (zipf) | zipf, 1 %, cold | 2000 | 91567 [87986, 96457] | 8.56 | 7.21 | 580100 | 607912 | page_cache.evictions +7735; page_cache.flushes +5545; store.size.reserved_unused =32825344 |
| get (uniform) | uniform, 100 %, warm OS cache | 200000 | 1399 [1392, 1413] | 0.00 | 0.00 | 583908 | 629496 | - |
| prefix scan (uniform) | uniform, 100 %, warm OS cache | 50000 | 1600 [1572, 1638] | 0.00 | 0.00 | 584496 | 629496 | - |
| put (uniform) | uniform, 100 %, warm OS cache | 20000 | 3750 [3677, 3909] | 0.00 | 0.00 | 594008 | 629496 | page_cache.evictions +125; page_cache.flushes +4; store.size.reserved_unused =12558336 |
| get (uniform) | uniform, 100 %, cold | 5000 | 1763 [1634, 2003] | 0.01 | 0.00 | 594008 | 629496 | page_cache.evictions +20 |
| prefix scan (uniform) | uniform, 100 %, cold | 2000 | 1936 [1836, 1970] | 0.01 | 0.00 | 594008 | 629496 | page_cache.evictions +8 |
| put (uniform) | uniform, 100 %, cold | 2000 | 3893 [3851, 4244] | 0.00 | 0.00 | 594008 | 629496 | page_cache.evictions +12; store.size.reserved_unused =12468224 |
| get (uniform) | uniform, 10 %, warm OS cache | 200000 | 3679 [3665, 3680] | 0.00 | 0.00 | 528656 | 629496 | page_cache.evictions +539044 |
| prefix scan (uniform) | uniform, 10 %, warm OS cache | 50000 | 3926 [3901, 3950] | 0.00 | 0.00 | 528656 | 629496 | page_cache.evictions +135352 |
| put (uniform) | uniform, 10 %, warm OS cache | 20000 | 9219 [8612, 9520] | 0.00 | 3.66 | 528656 | 629496 | page_cache.evictions +54221; page_cache.flushes +48423; store.size.reserved_unused =11689984 |
| get (uniform) | uniform, 10 %, cold | 5000 | 79512 [79032, 79584] | 7.18 | 0.00 | 528656 | 629496 | page_cache.evictions +13399 |
| prefix scan (uniform) | uniform, 10 %, cold | 2000 | 80996 [79706, 81346] | 7.22 | 0.00 | 528656 | 629496 | page_cache.evictions +5418 |
| put (uniform) | uniform, 10 %, cold | 2000 | 18188 [6236, 67699] | 3.09 | 0.00 | 528656 | 629496 | page_cache.evictions +2321; store.size.reserved_unused =11640832 |
| get (uniform) | uniform, 1 %, warm OS cache | 200000 | 3801 [3800, 3824] | 0.00 | 0.00 | 528524 | 629496 | page_cache.evictions +683447 |
| prefix scan (uniform) | uniform, 1 %, warm OS cache | 50000 | 4090 [4086, 4155] | 0.00 | 0.00 | 528524 | 629496 | page_cache.evictions +171789 |
| put (uniform) | uniform, 1 %, warm OS cache | 20000 | 10227 [10100, 10917] | 0.00 | 3.95 | 528524 | 629496 | page_cache.evictions +76644; page_cache.flushes +59395; store.size.reserved_unused =10862592 |
| get (uniform) | uniform, 1 %, cold | 5000 | 89225 [89065, 89329] | 8.11 | 0.00 | 528524 | 629496 | page_cache.evictions +17043 |
| prefix scan (uniform) | uniform, 1 %, cold | 2000 | 95453 [94837, 96047] | 8.65 | 0.00 | 528524 | 629496 | page_cache.evictions +6842 |
| put (uniform) | uniform, 1 %, cold | 2000 | 83094 [74980, 88993] | 7.99 | 7.11 | 528524 | 629496 | page_cache.evictions +7523; page_cache.flushes +5537; store.size.reserved_unused =10797056 |
