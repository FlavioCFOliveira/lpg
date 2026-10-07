# lab::pool

- date: 2026-10-02T18:31:56+01:00
- commit: e1ed45c25fd1-dirty
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data: 131072 pages (1 GiB), OS cache warm
- operation: fetch + shared latch + 1 byte read + unpin
- cache share: pool page buffers as % of the data pages

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| miss (sequential first touches) | 100 % | 32768 | 12084 [11818, 12122] | 7.43 | 0.00 | 808348 | 808348 | page_cache.page_faults +98304; page_cache.accounted_bytes =822209412 |
| hit (random in a resident set) | 100 % | 1000000 | 50 [50, 63] | 0.00 | 0.00 | 1070996 | 1070996 | page_cache.hits +3000001; page_cache.page_faults +32767; page_cache.accounted_bytes =1090636676 |
| uniform random | 100 % | 200000 | 162 [158, 166] | 0.00 | 0.00 | 1074312 | 1074312 | page_cache.hits +600000 |
| Zipf 0.8 random | 100 % | 200000 | 217 [211, 247] | 0.00 | 0.00 | 1073284 | 1073408 | page_cache.hits +600000 |
| miss (sequential first touches) | 10 % | 65536 | 3063 [3029, 3109] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.evictions +183500; page_cache.page_faults +196608; page_cache.accounted_bytes =108736220 |
| hit (random in a resident set) | 10 % | 1000000 | 31 [31, 31] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.hits +3000000; page_cache.page_faults +3277 |
| uniform random | 10 % | 200000 | 3084 [3038, 3118] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.evictions +540089; page_cache.hits +59911; page_cache.page_faults +540089 |
| Zipf 0.8 random | 10 % | 200000 | 1994 [1977, 2035] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.evictions +313941; page_cache.hits +286059; page_cache.page_faults +313941 |
| miss (sequential first touches) | 1 % | 65536 | 2683 [2665, 2730] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.evictions +195297; page_cache.page_faults +196608; page_cache.accounted_bytes =10891132 |
| hit (random in a resident set) | 1 % | 1000000 | 22 [22, 22] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.hits +3000000; page_cache.page_faults +327 |
| uniform random | 1 % | 200000 | 2921 [2915, 2948] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.evictions +593937; page_cache.hits +6063; page_cache.page_faults +593937 |
| Zipf 0.8 random | 1 % | 200000 | 2524 [2493, 2550] | 0.00 | 0.00 | 528480 | 1073408 | page_cache.evictions +472144; page_cache.hits +127856; page_cache.page_faults +472144 |
