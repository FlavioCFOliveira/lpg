#!/usr/bin/env bash
# A/B of lab::btree::btree_fixed puts: each run on a fresh copy of the
# pristine uniform database, variants interleaved per repetition.
S=$(dirname "$0"); OUT=$S/ab-btree.txt; : > $OUT
for rep in 1 2 3; do
 for share in 10 1 100; do
  for v in base new; do
   rm -rf /data/tmp/lpg-lab/ab/btree-uniform && cp -r /data/tmp/lpg-lab/pristine/btree-uniform /data/tmp/lpg-lab/ab/
   line=$(cd /data/tmp/lpg-lab/ab && LPG_LAB_EXT4=/data/tmp/lpg-lab/ab LPG_BTREE_DIST=uniform LPG_BTREE_SHARE=$share LPG_BTREE_MODE=put LPG_PERF_N=20000 $S/lpg-$v --exact lab::btree::btree_fixed --ignored --nocapture --test-threads=1 2>/dev/null | grep -o 'LAB .*')
   echo "rep=$rep variant=$v $line" | tee -a $OUT
  done
 done
done
rm -rf /data/tmp/lpg-lab/ab/btree-uniform
