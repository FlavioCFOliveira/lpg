#!/bin/bash
# t13 perf stat: per share and mode, N ops and N=0 (setup), 3 interleaved rounds of perf stat -r 1.
cd /data/dev/xumiga/lpg
BIN=target/lab/deps/lpg-ae66f89beb44ac48
R=docs/evidence/lab-sprint-1/raw
export LC_ALL=C LPG_LAB_EXT4=/data/tmp/lpg-lab
EV=cycles,instructions,branch-misses,ls_any_fills_from_sys.mem_io_local
OUT=$R/t13-perf-stat.txt
: > $OUT
for round in 1 2 3; do
  for share in 100 10 1; do
    for mode in hit miss; do
      if [ $mode = hit ]; then N=1000000; else N=30000; fi
      for n in $N 0; do
        echo "### round=$round share=$share mode=$mode n=$n" >> $OUT
        LPG_POOL_SHARE=$share LPG_POOL_MODE=$mode LPG_PERF_N=$n perf stat -x, -e $EV \
          $BIN --exact lab::pool::pool_fixed --ignored --test-threads=1 --nocapture 2>>$OUT | grep -o 'LAB.*' >> $OUT
      done
    done
  done
done
