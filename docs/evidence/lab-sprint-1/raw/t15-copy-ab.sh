#!/bin/bash
# Page-copy A/B (scratch copy binary XB, one binary, LPG_X_NOCOPY switches the
# step-6 copies and their reservation off). Uniform database, puts of new
# keys one per batch (Store::apply), at 100, 10 and 1 % cache; each run on a
# fresh copy of the same database; interleaved; perf stat of the process; a
# run with 0 puts gives the setup to subtract.
export LC_ALL=C
OUT=$1; XB=$2; P=/data/tmp/lpg-lab/pristine-uniform; R=/data/tmp/lpg-lab/run
: > $OUT/copy-ab-runs.txt
for rep in 1 2 3; do
 for share in 100 10 1; do
  for v in base nocopy; do
   for n in 20000 0; do
    rm -rf $R; mkdir -p $R; cp -a $P $R/btree-uniform; sync
    env $([ $v = nocopy ] && echo LPG_X_NOCOPY=1) LPG_LAB_EXT4=$R LPG_BTREE_DIST=uniform LPG_BTREE_SHARE=$share LPG_BTREE_MODE=put LPG_PERF_N=$n \
      perf stat -x, -e task-clock,cycles,instructions -o $OUT/copy-ab-perf-$v-s$share-n$n-rep$rep.txt \
      $XB --exact lab::btree::btree_fixed --ignored --test-threads=1 --nocapture 2>&1 | grep -o "LAB.*" | sed "s/^/rep=$rep variant=$v /" >> $OUT/copy-ab-runs.txt
   done
  done
 done
done
rm -rf $R
