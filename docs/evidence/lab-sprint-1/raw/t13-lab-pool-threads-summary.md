# lab::pool threads: std vs parking_lot latches

Mfetch/s, the median of each run's 3 rounds; 3 runs per build, interleaved (parking_lot run 1, then std and parking_lot alternating). Cell: median of the 3 runs [min, max].

| cache | threads | std | parking_lot | parking_lot / std |
|---|---|---|---|---|
| 100 % | 1 | 7.38 [7.13, 7.57] | 8.96 [8.86, 9.35] | 1.214 |
| 100 % | 4 | 31.30 [31.19, 32.19] | 36.34 [36.30, 36.39] | 1.161 |
| 100 % | 8 | 59.12 [57.71, 59.28] | 64.59 [63.00, 68.12] | 1.093 |
| 100 % | 16 | 75.73 [71.52, 77.04] | 76.24 [74.65, 80.71] | 1.007 |
| 10 % | 1 | 0.33 [0.33, 0.34] | 0.34 [0.34, 0.34] | 1.030 |
| 10 % | 4 | 1.03 [1.03, 1.04] | 1.05 [1.05, 1.06] | 1.019 |
| 10 % | 8 | 1.30 [1.29, 1.31] | 1.28 [1.28, 1.29] | 0.985 |
| 10 % | 16 | 1.21 [1.21, 1.22] | 1.20 [1.20, 1.21] | 0.992 |
