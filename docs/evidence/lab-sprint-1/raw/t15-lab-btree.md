# lab::btree

- date: 2026-10-03T10:02:14+01:00
- commit: 622f600422be
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: M-LAYOUT graph, 1000000 nodes, 10000000 edges, 2 entries per edge, 29-byte keys, empty values
- cache share: pool page buffers as % of the data pages (`cap_for`)
- cold: OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept
- put: one key per batch (`apply`), new edges of the same distribution

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| get (zipf) | zipf, 100 %, warm OS cache | 200000 | 1427 [1422, 1430] | 0.00 | 0.00 | 562844 | 608256 | - |
| prefix scan (zipf) | zipf, 100 %, warm OS cache | 50000 | 24042 [23890, 24134] | 0.00 | 0.00 | 563544 | 608256 | - |
| put (zipf) | zipf, 100 %, warm OS cache | 20000 | 3747 [3744, 3864] | 0.00 | 0.00 | 580752 | 608256 | page_cache.evictions +110; store.size.reserved_unused =34406400 |
| get (zipf) | zipf, 100 %, cold | 5000 | 1780 [1557, 1952] | 0.01 | 0.00 | 580752 | 608256 | page_cache.evictions +23 |
| prefix scan (zipf) | zipf, 100 %, cold | 2000 | 23416 [22221, 23677] | 0.01 | 0.00 | 580752 | 608256 | page_cache.evictions +9 |
| put (zipf) | zipf, 100 %, cold | 2000 | 3829 [3822, 4136] | 0.00 | 0.00 | 580752 | 608256 | page_cache.evictions +12; store.size.reserved_unused =34332672 |
| get (zipf) | zipf, 10 %, warm OS cache | 200000 | 3651 [3646, 3673] | 0.00 | 0.00 | 580752 | 608256 | page_cache.evictions +537983 |
| prefix scan (zipf) | zipf, 10 %, warm OS cache | 50000 | 27934 [27899, 28205] | 0.00 | 0.00 | 580752 | 608256 | page_cache.evictions +192158 |
| put (zipf) | zipf, 10 %, warm OS cache | 20000 | 44676 [32804, 45354] | 0.00 | 1.84 | 580752 | 608256 | page_cache.evictions +102006; page_cache.flushes +43152; store.size.reserved_unused =33595392 |
| get (zipf) | zipf, 10 %, cold | 5000 | 80128 [78885, 81124] | 7.20 | 0.00 | 580752 | 608256 | page_cache.evictions +13402 |
| prefix scan (zipf) | zipf, 10 %, cold | 2000 | 156881 [140957, 204967] | 12.66 | 0.00 | 580752 | 608256 | page_cache.evictions +9484 |
| put (zipf) | zipf, 10 %, cold | 2000 | 46337 [13385, 67287] | 4.25 | 0.00 | 580752 | 608256 | page_cache.evictions +3195; store.size.reserved_unused =33570816 |
| get (zipf) | zipf, 1 %, warm OS cache | 200000 | 3905 [3896, 3916] | 0.00 | 0.00 | 580752 | 608256 | page_cache.evictions +685901 |
| prefix scan (zipf) | zipf, 1 %, warm OS cache | 50000 | 32306 [32188, 32606] | 0.00 | 0.00 | 580752 | 608256 | page_cache.evictions +537021 |
| put (zipf) | zipf, 1 %, warm OS cache | 20000 | 14496 [14110, 14827] | 0.00 | 2.57 | 580752 | 608256 | page_cache.evictions +118517; page_cache.flushes +58316; store.size.reserved_unused =32907264 |
| get (zipf) | zipf, 1 %, cold | 5000 | 91789 [91640, 91805] | 8.13 | 0.00 | 580752 | 608256 | page_cache.evictions +17156 |
| prefix scan (zipf) | zipf, 1 %, cold | 2000 | 232510 [228426, 235849] | 18.07 | 0.00 | 580752 | 608256 | page_cache.evictions +20845 |
| put (zipf) | zipf, 1 %, cold | 2000 | 85020 [77283, 91381] | 8.16 | 6.30 | 580752 | 608256 | page_cache.evictions +10961; page_cache.flushes +4803; store.size.reserved_unused =32825344 |
| get (uniform) | uniform, 100 %, warm OS cache | 200000 | 1465 [1439, 1469] | 0.00 | 0.00 | 594680 | 629900 | - |
| prefix scan (uniform) | uniform, 100 %, warm OS cache | 50000 | 1702 [1682, 1706] | 0.00 | 0.00 | 594680 | 629900 | - |
| put (uniform) | uniform, 100 %, warm OS cache | 20000 | 3917 [3840, 3939] | 0.00 | 0.00 | 594680 | 629900 | page_cache.evictions +129; store.size.reserved_unused =12558336 |
| get (uniform) | uniform, 100 %, cold | 5000 | 1734 [1581, 2040] | 0.01 | 0.00 | 594680 | 629900 | page_cache.evictions +20 |
| prefix scan (uniform) | uniform, 100 %, cold | 2000 | 1854 [1784, 1858] | 0.01 | 0.00 | 594680 | 629900 | page_cache.evictions +8 |
| put (uniform) | uniform, 100 %, cold | 2000 | 3947 [3784, 3988] | 0.00 | 0.00 | 594680 | 629900 | page_cache.evictions +15; store.size.reserved_unused =12468224 |
| get (uniform) | uniform, 10 %, warm OS cache | 200000 | 3755 [3739, 3780] | 0.00 | 0.00 | 594680 | 629900 | page_cache.evictions +539044 |
| prefix scan (uniform) | uniform, 10 %, warm OS cache | 50000 | 3980 [3970, 4007] | 0.00 | 0.00 | 594680 | 629900 | page_cache.evictions +135352 |
| put (uniform) | uniform, 10 %, warm OS cache | 20000 | 46646 [33787, 47274] | 0.00 | 2.85 | 594680 | 629900 | page_cache.evictions +101843; page_cache.flushes +42766; store.size.reserved_unused =11689984 |
| get (uniform) | uniform, 10 %, cold | 5000 | 79902 [79660, 80205] | 7.18 | 0.00 | 594680 | 629900 | page_cache.evictions +13400 |
| prefix scan (uniform) | uniform, 10 %, cold | 2000 | 82705 [80456, 82820] | 7.22 | 0.00 | 594680 | 629900 | page_cache.evictions +5418 |
| put (uniform) | uniform, 10 %, cold | 2000 | 19423 [6036, 71766] | 3.09 | 0.00 | 594680 | 629900 | page_cache.evictions +2324; store.size.reserved_unused =11640832 |
| get (uniform) | uniform, 1 %, warm OS cache | 200000 | 3990 [3979, 3994] | 0.00 | 0.00 | 594680 | 629900 | page_cache.evictions +683447 |
| prefix scan (uniform) | uniform, 1 %, warm OS cache | 50000 | 4259 [4200, 4264] | 0.00 | 0.00 | 594680 | 629900 | page_cache.evictions +171789 |
| put (uniform) | uniform, 1 %, warm OS cache | 20000 | 15192 [15149, 15578] | 0.00 | 3.91 | 594680 | 629900 | page_cache.evictions +118813; page_cache.flushes +58667; store.size.reserved_unused =10862592 |
| get (uniform) | uniform, 1 %, cold | 5000 | 88769 [88582, 89229] | 8.11 | 0.00 | 594680 | 629900 | page_cache.evictions +17043 |
| prefix scan (uniform) | uniform, 1 %, cold | 2000 | 96111 [96000, 96815] | 8.65 | 0.00 | 594680 | 629900 | page_cache.evictions +6842 |
| put (uniform) | uniform, 1 %, cold | 2000 | 70818 [69731, 93280] | 7.33 | 5.39 | 594680 | 629900 | page_cache.evictions +10150; page_cache.flushes +4084; store.size.reserved_unused =10797056 |
