# lab::btree

- date: 2026-10-02T20:25:49+01:00
- commit: 
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: M-LAYOUT graph, 1000000 nodes, 10000000 edges, 2 entries per edge, 29-byte keys, empty values
- cache share: pool page buffers as % of the data pages (`cap_for`)
- cold: OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept
- put: one key per batch (`apply`), new edges of the same distribution

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| get (zipf) | zipf, 100 %, warm OS cache | 200000 | 1417 [1411, 1439] | 0.00 | 0.00 | 562244 | 607912 | - |
| prefix scan (zipf) | zipf, 100 %, warm OS cache | 50000 | 22527 [21976, 23075] | 0.00 | 0.00 | 562948 | 607912 | - |
| put (zipf) | zipf, 100 %, warm OS cache | 20000 | 5764 [5737, 5994] | 0.00 | 0.00 | 580156 | 607912 | page_cache.evictions +108; page_cache.flushes +6; store.size.reserved_unused =34406400 |
| get (zipf) | zipf, 100 %, cold | 5000 | 1980 [1688, 2075] | 0.01 | 0.00 | 580156 | 607912 | page_cache.evictions +24 |
| prefix scan (zipf) | zipf, 100 %, cold | 2000 | 22022 [20486, 22113] | 0.01 | 0.00 | 580156 | 607912 | page_cache.evictions +9 |
| put (zipf) | zipf, 100 %, cold | 2000 | 5969 [5916, 6240] | 0.00 | 0.00 | 580156 | 607912 | page_cache.evictions +9; store.size.reserved_unused =34332672 |
| get (zipf) | zipf, 10 %, warm OS cache | 200000 | 3745 [3742, 3761] | 0.00 | 0.00 | 580156 | 607912 | page_cache.evictions +537983 |
| prefix scan (zipf) | zipf, 10 %, warm OS cache | 50000 | 25587 [25434, 25866] | 0.00 | 0.00 | 580156 | 607912 | page_cache.evictions +192158 |
| put (zipf) | zipf, 10 %, warm OS cache | 20000 | 11404 [10686, 11507] | 0.00 | 2.63 | 580156 | 607912 | page_cache.evictions +54048; page_cache.flushes +48600; store.size.reserved_unused =33595392 |
| get (zipf) | zipf, 10 %, cold | 5000 | 80753 [80704, 81610] | 7.20 | 0.00 | 580156 | 607912 | page_cache.evictions +13402 |
| prefix scan (zipf) | zipf, 10 %, cold | 2000 | 155330 [139644, 199392] | 12.66 | 0.00 | 580156 | 607912 | page_cache.evictions +9484 |
| put (zipf) | zipf, 10 %, cold | 2000 | 46847 [14369, 68273] | 4.25 | 0.00 | 580156 | 607912 | page_cache.evictions +3192; store.size.reserved_unused =33570816 |
| get (zipf) | zipf, 1 %, warm OS cache | 200000 | 3982 [3978, 3982] | 0.00 | 0.00 | 580156 | 607912 | page_cache.evictions +685901 |
| prefix scan (zipf) | zipf, 1 %, warm OS cache | 50000 | 30228 [30163, 30578] | 0.00 | 0.00 | 580156 | 607912 | page_cache.evictions +537021 |
| put (zipf) | zipf, 1 %, warm OS cache | 20000 | 12256 [12014, 12969] | 0.00 | 2.64 | 580156 | 607912 | page_cache.evictions +77072; page_cache.flushes +59337; store.size.reserved_unused =32907264 |
| get (zipf) | zipf, 1 %, cold | 5000 | 92240 [92195, 92407] | 8.13 | 0.00 | 580156 | 607912 | page_cache.evictions +17156 |
| prefix scan (zipf) | zipf, 1 %, cold | 2000 | 232778 [222089, 234127] | 18.07 | 0.00 | 580156 | 607912 | page_cache.evictions +20845 |
| put (zipf) | zipf, 1 %, cold | 2000 | 89924 [89500, 92316] | 8.56 | 7.21 | 580156 | 607912 | page_cache.evictions +7735; page_cache.flushes +5545; store.size.reserved_unused =32825344 |
| get (uniform) | uniform, 100 %, warm OS cache | 200000 | 1429 [1425, 1435] | 0.00 | 0.00 | 583936 | 629468 | - |
| prefix scan (uniform) | uniform, 100 %, warm OS cache | 50000 | 1627 [1623, 1628] | 0.00 | 0.00 | 584524 | 629468 | - |
| put (uniform) | uniform, 100 %, warm OS cache | 20000 | 5774 [5707, 5893] | 0.00 | 0.00 | 594036 | 629468 | page_cache.evictions +125; page_cache.flushes +4; store.size.reserved_unused =12558336 |
| get (uniform) | uniform, 100 %, cold | 5000 | 1738 [1622, 2130] | 0.01 | 0.00 | 594036 | 629468 | page_cache.evictions +20 |
| prefix scan (uniform) | uniform, 100 %, cold | 2000 | 1913 [1752, 1930] | 0.01 | 0.00 | 594036 | 629468 | page_cache.evictions +8 |
| put (uniform) | uniform, 100 %, cold | 2000 | 5883 [5795, 5946] | 0.00 | 0.00 | 594036 | 629468 | page_cache.evictions +12; store.size.reserved_unused =12468224 |
| get (uniform) | uniform, 10 %, warm OS cache | 200000 | 3714 [3711, 3718] | 0.00 | 0.00 | 528584 | 629468 | page_cache.evictions +539044 |
| prefix scan (uniform) | uniform, 10 %, warm OS cache | 50000 | 3979 [3921, 4006] | 0.00 | 0.00 | 528584 | 629468 | page_cache.evictions +135352 |
| put (uniform) | uniform, 10 %, warm OS cache | 20000 | 11327 [10674, 11687] | 0.00 | 3.66 | 528584 | 629468 | page_cache.evictions +54221; page_cache.flushes +48423; store.size.reserved_unused =11689984 |
| get (uniform) | uniform, 10 %, cold | 5000 | 77891 [77759, 78567] | 7.18 | 0.00 | 528584 | 629468 | page_cache.evictions +13399 |
| prefix scan (uniform) | uniform, 10 %, cold | 2000 | 80017 [79368, 80033] | 7.22 | 0.00 | 528584 | 629468 | page_cache.evictions +5418 |
| put (uniform) | uniform, 10 %, cold | 2000 | 19786 [7896, 70300] | 3.09 | 0.00 | 528584 | 629468 | page_cache.evictions +2321; store.size.reserved_unused =11640832 |
| get (uniform) | uniform, 1 %, warm OS cache | 200000 | 3873 [3872, 3886] | 0.00 | 0.00 | 528584 | 629468 | page_cache.evictions +683447 |
| prefix scan (uniform) | uniform, 1 %, warm OS cache | 50000 | 4075 [4056, 4082] | 0.00 | 0.00 | 528584 | 629468 | page_cache.evictions +171789 |
| put (uniform) | uniform, 1 %, warm OS cache | 20000 | 12304 [12213, 13313] | 0.00 | 3.95 | 528584 | 629468 | page_cache.evictions +76644; page_cache.flushes +59395; store.size.reserved_unused =10862592 |
| get (uniform) | uniform, 1 %, cold | 5000 | 87891 [87759, 88149] | 8.11 | 0.00 | 528584 | 629468 | page_cache.evictions +17043 |
| prefix scan (uniform) | uniform, 1 %, cold | 2000 | 94828 [94156, 95388] | 8.65 | 0.00 | 528584 | 629468 | page_cache.evictions +6842 |
| put (uniform) | uniform, 1 %, cold | 2000 | 83890 [75009, 91483] | 7.99 | 7.11 | 528584 | 629468 | page_cache.evictions +7523; page_cache.flushes +5537; store.size.reserved_unused =10797056 |
