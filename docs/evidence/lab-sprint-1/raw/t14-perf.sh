#!/bin/bash
# t14 perf stat, perf record (DWARF) + inferno, heaptrack of lab::btree::btree_fixed.
# Per-op = (run with N - run with 0) / N. perf.data stays outside the repository.
set -e
cd /data/dev/xumiga/lpg
BIN=${BIN:?set BIN to target/lab/deps/lpg-<hash>}
R=docs/evidence/lab-sprint-1/raw
S=${SCRATCH:?set SCRATCH to a directory outside the repository}
export LC_ALL=C LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_BTREE_DIST=uniform
EV=cycles,instructions,branch-misses,cache-misses,ls_any_fills_from_sys.mem_io_local
run() { $BIN --exact lab::btree::btree_fixed --ignored --test-threads=1 --nocapture 2>&1 | grep -o 'LAB.*'; }
OUT=$R/t14-perf-stat.txt; : > $OUT
for round in 1 2 3; do
  for cfg in "100 get 1000000" "10 get 200000" "100 scan 200000" "100 put 200000"; do
    set -- $cfg
    for n in $3 0; do
      echo "### round=$round share=$1 mode=$2 n=$n" >> $OUT
      LPG_BTREE_SHARE=$1 LPG_BTREE_MODE=$2 LPG_PERF_N=$n perf stat -x, -e $EV \
        $BIN --exact lab::btree::btree_fixed --ignored --test-threads=1 --nocapture 2>>$OUT | grep -o 'LAB.*' >> $OUT
    done
  done
done
for mode in get put; do
  n=1000000; [ $mode = put ] && n=300000
  LPG_BTREE_SHARE=100 LPG_BTREE_MODE=$mode LPG_PERF_N=$n perf record --call-graph dwarf,16384 -F 999 \
    -o $S/t14-$mode.perf.data -- $BIN --exact lab::btree::btree_fixed --ignored --test-threads=1 > /dev/null 2>&1
  perf script -i $S/t14-$mode.perf.data 2>/dev/null | inferno-collapse-perf > $S/t14-$mode.all.folded
  grep "${mode}_loop" $S/t14-$mode.all.folded > $R/t14-perf-$mode.folded
  inferno-flamegraph --title "lab::btree $mode (uniform, 100 % cache), ${mode}_loop only" < $R/t14-perf-$mode.folded > $R/t14-flamegraph-$mode.svg
done
HT=$R/t14-heaptrack-summary.txt; : > $HT
for cfg in "100 get 1000000" "100 scan 200000" "100 put 200000"; do
  set -- $cfg
  for n in $3 0; do
    LPG_BTREE_SHARE=$1 LPG_BTREE_MODE=$2 LPG_PERF_N=$n heaptrack -o $S/t14-ht-$2-$n $BIN --exact lab::btree::btree_fixed --ignored --test-threads=1 > /dev/null 2>&1
    f=$(ls $S/t14-ht-$2-$n.*zst | head -1)
    echo "share=$1 mode=$2 n=$n | $(heaptrack_print $f 2>/dev/null | grep -E 'calls to allocation functions|peak heap memory' | tr '\n' ' ')" >> $HT
  done
done
