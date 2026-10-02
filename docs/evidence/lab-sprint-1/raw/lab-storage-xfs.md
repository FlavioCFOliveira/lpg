# lab::storage on xfs

- date: 2026-10-02T17:51:53+01:00
- commit: ef61d0f16c82-dirty
- profile: lab (release + debug info)
- directory: /mnt/lpg-xfs
- data file: 131072 pages of 8192 B (1 GiB), reserved with reserve()
- rounds: 3, each timed as one batch

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| read_at 8 KiB random | warm (OS cache) | 200000 | 1253 [1248, 1294] | 0.00 | 0.00 | 5476 | 5684 | - |
| read_at 8 KiB random | cold (evicted, distinct pages) | 20000 | 106823 [106770, 106830] | 8.14 | 0.00 | 6460 | 6460 | - |
| write_at 8 KiB + sync_data | n/a (sequential, new reserved file) | 1000 | 10615366 [9794341, 13217804] | 0.00 | 8.00 | 6460 | 6460 | - |
