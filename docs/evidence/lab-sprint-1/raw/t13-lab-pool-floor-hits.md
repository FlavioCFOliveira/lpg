# lab::pool floor hit rates

| case | workload | page buffers | held by the competing user B | hit rate |
|---|---|---|---|---|
| floor 1.0 %, alone | uniform random | 13108 | 0 | 0.0986 |
| floor 1.0 %, alone | Zipf 0.8 random | 13108 | 0 | 0.4762 |
| floor 0.5 %, competing | uniform random | 66 | 106840064 | 0.0036 |
| floor 0.5 %, competing | Zipf 0.8 random | 66 | 106840064 | 0.0485 |
| floor 1.0 %, competing | uniform random | 132 | 106299392 | 0.0041 |
| floor 1.0 %, competing | Zipf 0.8 random | 132 | 106299392 | 0.0766 |
| floor 2.0 %, competing | uniform random | 265 | 105209856 | 0.0050 |
| floor 2.0 %, competing | Zipf 0.8 random | 265 | 105209856 | 0.1098 |
| floor 5.0 %, competing | uniform random | 663 | 101949440 | 0.0080 |
| floor 5.0 %, competing | Zipf 0.8 random | 663 | 101949440 | 0.1637 |
