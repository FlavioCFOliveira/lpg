# lab::pool floor (unverified point 5)

- date: 2026-10-02T18:25:51+01:00
- commit: e1ed45c25fd1-dirty
- profile: lab (release + debug info)
- cap: 108736220 B, a pool of 10 % of the data when alone
- data: 131072 pages (1 GiB), OS cache warm

| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |
|---|---|---|---|---|---|---|---|---|
| uniform random | floor 1.0 %, alone | 200000 | 3067 [3051, 3092] | 0.00 | 0.00 | 114720 | 114720 | page_cache.evictions +540204; page_cache.hits +61754; page_cache.page_faults +564462; page_cache.accounted_bytes =108736220 |
| Zipf 0.8 random | floor 1.0 %, alone | 200000 | 2005 [1983, 2027] | 0.00 | 0.00 | 113868 | 114432 | page_cache.evictions +314294; page_cache.hits +285706; page_cache.page_faults +314294 |
| uniform random | floor 0.5 %, competing | 200000 | 4073 [4034, 4098] | 0.00 | 0.00 | 103436 | 114432 | page_cache.evictions +599716; page_cache.hits +2242; page_cache.page_faults +623974; page_cache.accounted_bytes =108736220 |
| Zipf 0.8 random | floor 0.5 %, competing | 200000 | 4054 [4051, 4066] | 0.00 | 0.00 | 103436 | 114432 | page_cache.evictions +570890; page_cache.hits +29110; page_cache.page_faults +570890 |
| uniform random | floor 1.0 %, competing | 200000 | 3453 [3441, 3472] | 0.00 | 0.00 | 103436 | 114432 | page_cache.evictions +599412; page_cache.hits +2546; page_cache.page_faults +623670; page_cache.accounted_bytes =108736220 |
| Zipf 0.8 random | floor 1.0 %, competing | 200000 | 3379 [3359, 3398] | 0.00 | 0.00 | 103436 | 114432 | page_cache.evictions +554019; page_cache.hits +45981; page_cache.page_faults +554019 |
| uniform random | floor 2.0 %, competing | 200000 | 3202 [3193, 3213] | 0.00 | 0.00 | 103436 | 114432 | page_cache.evictions +598800; page_cache.hits +3158; page_cache.page_faults +623058; page_cache.accounted_bytes =108736220 |
| Zipf 0.8 random | floor 2.0 %, competing | 200000 | 3068 [3061, 3090] | 0.00 | 0.00 | 103436 | 114432 | page_cache.evictions +534093; page_cache.hits +65907; page_cache.page_faults +534093 |
| uniform random | floor 5.0 %, competing | 200000 | 3104 [3097, 3110] | 0.00 | 0.00 | 111492 | 114432 | page_cache.evictions +596967; page_cache.hits +4991; page_cache.page_faults +621225; page_cache.accounted_bytes =108736220 |
| Zipf 0.8 random | floor 5.0 %, competing | 200000 | 2846 [2818, 2848] | 0.00 | 0.00 | 111492 | 114432 | page_cache.evictions +501756; page_cache.hits +98244; page_cache.page_faults +501756 |
