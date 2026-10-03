# lab::recovery recovery cost

- date: 2026-10-03T10:27:40+01:00
- commit: 622f600422be-dirty
- profile: lab (release + debug info)
- data: one tree, 100000 entries, 8-byte keys, 400-byte values; cap 256 MiB; periodic checkpoints off
- WAL: 100-page commits (keys 64 apart) until the WAL past the redo point reaches the target, then a drop without close
- open: Engine::open, database files evicted from the OS cache first; includes the open checkpoint (TS-13 step 8), reported apart
- replay MiB/s: WAL bytes replayed / (open time - open checkpoint time)
- rounds: 3; median [min, max]

## ext4 (/data/tmp/lpg-lab)

| target MiB | WAL B replayed | groups | pages | open ms | open checkpoint ms | replay MiB/s | read KiB (/proc/self/io) |
|---|---|---|---|---|---|---|---|
| 16 | 17443104 [17443104, 17443104] | 21 [21, 21] | 2100 [2100, 2100] | 61 [53, 67] | 18 [17, 21] | 376 [365, 484] | 48752 [33020, 50052] |
| 64 | 67280544 [67280544, 67280544] | 81 [81, 81] | 8100 [8100, 8100] | 123 [122, 124] | 30 [30, 30] | 689 [687, 697] | 82652 [82484, 82820] |
| 256 | 269122176 [269122176, 269122176] | 324 [324, 324] | 32400 [32400, 32400] | 367 [364, 382] | 64 [61, 67] | 848 [815, 848] | 279428 [278756, 294464] |

## xfs (/mnt/lpg-xfs)

| target MiB | WAL B replayed | groups | pages | open ms | open checkpoint ms | replay MiB/s | read KiB (/proc/self/io) |
|---|---|---|---|---|---|---|---|
| 16 | 17443104 [17443104, 17443104] | 21 [21, 21] | 2100 [2100, 2100] | 61 [54, 61] | 22 [22, 24] | 427 [421, 547] | 48752 [33020, 50052] |
| 64 | 67280544 [67280544, 67280544] | 81 [81, 81] | 8100 [8100, 8100] | 109 [109, 109] | 31 [31, 32] | 824 [824, 825] | 82652 [82484, 82820] |
| 256 | 269122176 [269122176, 269122176] | 324 [324, 324] | 32400 [32400, 32400] | 341 [338, 343] | 69 [69, 72] | 943 [934, 964] | 279428 [278756, 294464] |
