# lab::btree build, shape and in-leaf costs

## Build (random order, one edge = two entries per batch, full cache)

| dist | entries | seconds | entries/s | ns/entry | write_dirty | VmHWM growth KiB |
|---|---|---|---|---|---|---|
| zipf | 20000000 | 73.4 | 272634 | 3668 | 61234 pages, 0.6 s | 506796 |
| uniform | 20000000 | 68.6 | 291749 | 3428 | 63892 pages, 0.6 s | 0 |

## Shape (integrity check figures)

| dist | entries | height | leaves | inner | pages | leaf B/entry | leaf fill | file B/entry | check s |
|---|---|---|---|---|---|---|---|---|---|
| zipf | 20000000 | 3 | 60970 | 262 | 61234 | 16.81 | 0.673 | 25.08 | 1.2 |
| uniform | 20000000 | 3 | 63618 | 272 | 63892 | 17.88 | 0.686 | 26.17 | 1.3 |

## In-leaf costs (2 000 resident leaf images, 3 rounds)

| dist | entries/leaf | ns per leaf_get median [min, max] | ns per entry decoded median [min, max] |
|---|---|---|---|
| zipf | 347 | 235 [234, 235] | 10.2 [10.1, 10.2] |
| uniform | 329 | 240 [237, 244] | 11.0 [11.0, 11.2] |

## Entries per prefix scan

| dist | cache | mode | entries/scan |
|---|---|---|---|
| zipf | 100 % | warm OS cache | 1007.1 |
| zipf | 100 % | cold | 961.1 |
| zipf | 10 % | warm OS cache | 1010.4 |
| zipf | 10 % | cold | 964.3 |
| zipf | 1 % | warm OS cache | 1013.7 |
| zipf | 1 % | cold | 967.5 |
| uniform | 100 % | warm OS cache | 3.5 |
| uniform | 100 % | cold | 3.6 |
| uniform | 10 % | warm OS cache | 3.5 |
| uniform | 10 % | cold | 3.6 |
| uniform | 1 % | warm OS cache | 3.5 |
| uniform | 1 % | cold | 3.6 |
