# lab::recovery WAL between checkpoints at the commit trigger

- date: 2026-10-03T10:10:07+01:00
- commit: 622f600422be-dirty
- profile: lab (release + debug info)
- data: one tree, 100000 entries, 8-byte keys, 400-byte values; cap 256 MiB
- commits: 100000 one-page commits (one value replaced at a random key) after a checkpoint; trigger 100000 commits, time trigger 24 h
- runs: one per thread count

## ext4 (/data/tmp/lpg-lab)

| threads | seconds | commits/group | WAL B (redo points) | WAL B (log.appended_bytes) | WAL B/commit | triggered checkpoint ms |
|---|---|---|---|---|---|---|
| 1 | 223.1 | 1.00 | 1644800000 | 1644800000 | 16448 | 705.2 |
| 16 | 39.4 | 8.07 | 923464736 | 923464736 | 9235 | 389.8 |

## xfs (/mnt/lpg-xfs)

| threads | seconds | commits/group | WAL B (redo points) | WAL B (log.appended_bytes) | WAL B/commit | triggered checkpoint ms |
|---|---|---|---|---|---|---|
| 1 | 691.1 | 1.00 | 1644800000 | 1644800000 | 16448 | 706.3 |
| 16 | 83.8 | 8.10 | 923300256 | 923300256 | 9233 | 454.0 |
