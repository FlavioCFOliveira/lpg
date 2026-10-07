#!/bin/bash
# t14 put optimisation: A/B of the pre-change code (B) and the changed code (A).
# Each version is a copy of the working tree outside the repository, built with
#   CARGO_TARGET_DIR=<dir> cargo test -p lpg --profile lab --lib --no-run
# Both copies add the same build-only test to src/lab/btree.rs (it does not exist in the repository):
#   #[test] #[ignore] fn btree_build_only() { for dist in ["zipf", "uniform"] { println!("{dist}: {:?}", build(dist)); } }
# 1. B builds the databases; they are copied to $L/pristine and hashed (raw/t14-opt-build.txt).
#    A rebuilds them into another directory; data-1 is compared with cmp.
# 2. btree_fixed, 100 % cache, 3 rounds, B and A interleaved per configuration, perf stat;
#    per op = (run with N - run with 0) / N (raw/t14-opt-perf-stat.txt). The data files are
#    re-hashed afterwards: unchanged.
# 3. perf record of A's put (raw/t14-opt-perf-put.folded, raw/t14-opt-flamegraph-put.svg).
# 4. Full lab::btree, pristine databases restored before each run, order B A B A
#    (raw/t14-opt-lab-btree-{before,after}-{1,2}.md).
set -e
B=${B:?B lab binary}; A=${A:?A lab binary}; S=${SCRATCH:?directory outside the repository}
L=/data/tmp/lpg-lab
export LC_ALL=C LPG_LAB_EXT4=$L
EV=cycles,instructions,branch-misses,cache-misses,ls_any_fills_from_sys.mem_io_local
OUT=$S/t14-opt-perf-stat.txt; : > $OUT
for round in 1 2 3; do
  for dist in uniform zipf; do
    for cfg in "put 200000" "get 1000000"; do
      set -- $cfg
      for v in B A; do
        bin=$B; [ $v = A ] && bin=$A
        for n in $2 0; do
          echo "### round=$round dist=$dist mode=$1 n=$n bin=$v" >> $OUT
          LPG_BTREE_DIST=$dist LPG_BTREE_SHARE=100 LPG_BTREE_MODE=$1 LPG_PERF_N=$n perf stat -x, -e $EV \
            $bin --exact lab::btree::btree_fixed --ignored --test-threads=1 --nocapture 2>>$OUT | grep -o 'LAB.*' >> $OUT
        done
      done
    done
  done
done
LPG_BTREE_DIST=uniform LPG_BTREE_SHARE=100 LPG_BTREE_MODE=put LPG_PERF_N=300000 perf record --call-graph dwarf,16384 -F 999 \
  -o $S/t14-opt-put.perf.data -- $A --exact lab::btree::btree_fixed --ignored --test-threads=1 > /dev/null 2>&1
perf script -i $S/t14-opt-put.perf.data 2>/dev/null | inferno-collapse-perf | grep put_loop > $S/t14-opt-perf-put.folded
inferno-flamegraph --title "lab::btree put (uniform, 100 % cache), put_loop only, after the copy optimisation" \
  < $S/t14-opt-perf-put.folded > $S/t14-opt-flamegraph-put.svg
for run in 1 2; do
  for v in before after; do
    bin=$B; [ $v = after ] && bin=$A
    rm -rf $L/btree-zipf $L/btree-uniform
    cp -a $L/pristine/btree-zipf $L/pristine/btree-uniform $L/
    mkdir -p $S/full-$v-$run
    LPG_LAB_XFS=/mnt/lpg-xfs LPG_LAB_OUT=$S/full-$v-$run $bin --exact lab::btree::btree --ignored --test-threads=1
  done
done
