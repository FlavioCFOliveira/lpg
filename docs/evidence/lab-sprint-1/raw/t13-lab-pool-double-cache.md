# lab::pool miss cost, OS cache warm and evicted

- date: 2026-10-02T18:31:28+01:00
- commit: e1ed45c25fd1-dirty
- profile: lab (release + debug info)
- data: 131072 pages (1 GiB)
- pool: 10 % of the data, full
- misses: 10000 distinct random pages per round

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| miss, OS cache warm | 10 % | 10000 | 3065 [3026, 3219] | 0.00 | 0.00 | 113288 | 113288 | page_cache.evictions +28056; page_cache.hits +1944; page_cache.page_faults +28056 |
| miss, OS cache evicted | 10 % | 10000 | 93267 [92559, 93558] | 7.51 | 0.00 | 113288 | 113288 | page_cache.evictions +28042; page_cache.hits +1958; page_cache.page_faults +28042 |
