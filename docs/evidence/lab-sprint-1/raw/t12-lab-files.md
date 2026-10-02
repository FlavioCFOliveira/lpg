# lab::files

- memory cap: 67108864 B
- runs: 100 creates and 100 opens per directory, each in a new directory, each call timed alone
- profile: lab

| directory | create µs median [min, max] | create µs p90 | open µs median [min, max] | open µs p90 |
|---|---|---|---|---|
| ext4 | 5947 [5305, 13559] | 6503 | 85 [82, 253] | 104 |
| xfs | 11806 [11352, 28185] | 12864 | 83 [76, 150] | 115 |
