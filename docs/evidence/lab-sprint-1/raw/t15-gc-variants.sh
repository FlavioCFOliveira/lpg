#!/bin/bash
# Group-commit variants (scratch copy only), ext4, interleaved per rep:
# base; per-group wait slots (LPG_X_SLOTS=1); writer delay before closing a
# group (LPG_X_DELAY_US, as PostgreSQL commit_delay). Per run: perf stat
# context switches and task clock of the whole process (a run with 0
# commits is subtracted later), nvme0n1 flush await from
# /sys/block/nvme0n1/stat fields 16-17.
export LC_ALL=C LPG_LAB_EXT4=/data/tmp/lpg-lab
OUT=$1; XB=$2; : > $OUT/gc-runs.txt
run() { # rep variant threads n
   local env=""; case $2 in slots) env="LPG_X_SLOTS=1";; d*) env="LPG_X_DELAY_US=${2#d}";; esac
   local a=$(awk '{print $16, $17}' /sys/block/nvme0n1/stat)
   local r=$(env $env LPG_LAB_FS=ext4 LPG_COMMIT_THREADS=$3 LPG_PERF_N=$4 perf stat -x, -e context-switches,task-clock -o $OUT/gc-perf-$2-t$3-n$4-rep$1.txt \
       $XB --exact lab::commit::commit_trace --ignored --test-threads=1 --nocapture 2>&1 | grep -o "LAB.*")
   local b=$(awk '{print $16, $17}' /sys/block/nvme0n1/stat)
   set -- $1 $2 $a $b
   local fa=$(python3 -c "print(f'{($6-$4)/max(1,$5-$3):.2f}')")
   local cs=$(awk -F, '/context-switches/{print $1}' $OUT/gc-perf-$2-t*-rep$1.txt | tail -1)
   echo "rep=$1 variant=$2 f_await_ms=$fa $r" >> $OUT/gc-runs.txt
}
for rep in 1 2 3; do
 for t in 1 4 16; do
  for v in base slots d20 d50 d100 d200; do
   n=$(( t==1 ? 2000 : (t==4 ? 1000 : 400) ))
   run $rep $v $t $n
   run $rep $v $t 0
  done
 done
done
