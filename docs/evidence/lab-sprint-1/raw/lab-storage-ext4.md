# lab::storage on ext4

- date: 2026-10-02T17:51:12+01:00
- commit: ef61d0f16c82-dirty
- profile: lab (release + debug info)
- directory: /data/tmp/lpg-lab
- data file: 131072 pages of 8192 B (1 GiB), reserved with reserve()
- rounds: 3, each timed as one batch

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| read_at 8 KiB random | warm (OS cache) | 200000 | 1411 [1286, 1423] | 0.00 | 0.00 | 5060 | 5060 | - |
| read_at 8 KiB random | cold (evicted, distinct pages) | 20000 | 95298 [94713, 95432] | 8.11 | 0.00 | 5884 | 5884 | - |
| write_at 8 KiB + sync_data | n/a (sequential, new reserved file) | 1000 | 5733534 [4306828, 6694410] | 0.00 | 8.00 | 5884 | 5884 | - |
