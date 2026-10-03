#!/bin/bash
# checkpoint_fixed (65 536 dirty leaves), 3 reps per file system, interleaved,
# with iostat -x -t 1 throughout; the LAB line gives the checkpoint window.
export LC_ALL=C LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs
OUT=$1; : > $OUT/t16-checkpoint-fixed-log.txt
iostat -x -t -y nvme0n1 loop80 1 > $OUT/t16-checkpoint-fixed-iostat.txt & I=$!
for rep in 1 2 3; do for fs in ext4 xfs; do
  LPG_LAB_FS=$fs $BIN --exact lab::recovery::checkpoint_fixed --ignored --test-threads=1 --nocapture 2>&1 | grep -o "LAB.*" | sed "s/^/rep=$rep /" >> $OUT/t16-checkpoint-fixed-log.txt
  sleep 3
done; done
kill $I
