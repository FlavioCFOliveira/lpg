# lab::recovery checkpoint cost

- date: 2026-10-03T10:07:42+01:00
- commit: 622f600422be-dirty
- profile: lab (release + debug info)
- data: one tree of about 17 × leaves entries, 8-byte keys, 400-byte values, key order; the pool holds every page
- dirty: one key in 8 replaced, 1000 per commit, before each round; every leaf dirty
- timed: one checkpoint (Engine::checkpoint): TS-11 steps 1-5
- rounds: 3; median [min, max]

## ext4 (/data/tmp/lpg-lab)

| target leaves | pages written | duration ms | pages/s | MiB/s | written KiB (/proc/self/io) | segments recycled + removed |
|---|---|---|---|---|---|---|
| 1024 | 967 [967, 967] | 17 [17, 24] | 56177 [41013, 58248] | 439 [320, 455] | 7740 [7740, 7740] | 1 |
| 8192 | 7737 [7737, 7737] | 79 [67, 97] | 97438 [79576, 115594] | 761 [622, 903] | 61900 [61900, 61900] | 11 |
| 65536 | 61895 [61895, 61895] | 510 [510, 517] | 121373 [119770, 121459] | 948 [936, 949] | 495164 [495164, 495164] | 92 |

## xfs (/mnt/lpg-xfs)

| target leaves | pages written | duration ms | pages/s | MiB/s | written KiB (/proc/self/io) | segments recycled + removed |
|---|---|---|---|---|---|---|
| 1024 | 967 [967, 967] | 15 [15, 17] | 65104 [55406, 66222] | 509 [433, 517] | 7744 [7744, 7744] | 1 |
| 8192 | 7737 [7737, 7737] | 75 [73, 110] | 102690 [70041, 105517] | 802 [547, 824] | 61904 [61904, 61904] | 11 |
| 65536 | 61895 [61895, 61895] | 850 [829, 870] | 72787 [71172, 74648] | 569 [556, 583] | 495168 [495168, 495168] | 92 |
