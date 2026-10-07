#!/usr/bin/env bash
# A/B of lab::commit::commit_fixed: variants interleaved per repetition,
# each run bracketed by /sys/block/nvme0n1/stat (flush ios field 16, flush ms field 17).
S=$(dirname "$0"); OUT=$S/ab-commit.txt
for rep in ${REPS:-1 2 3}; do
 for t in 1 4 16; do
  for v in base new nogather; do
   read -ra a < /sys/block/nvme0n1/stat
   line=$(cd /data/tmp/lpg-lab/ab && LPG_LAB_EXT4=/data/tmp/lpg-lab/ab LPG_LAB_FS=ext4 LPG_COMMIT_THREADS=$t LPG_PERF_N=1000 $S/lpg-$v --exact lab::commit::commit_fixed --ignored --nocapture --test-threads=1 2>/dev/null | grep -o 'LAB .*')
   read -ra b < /sys/block/nvme0n1/stat
   fios=$(( b[15] - a[15] )); fms=$(( b[16] - a[16] ))
   echo "rep=$rep variant=$v f_ios=$fios f_ms=$fms $line" | tee -a $OUT
  done
 done
done
