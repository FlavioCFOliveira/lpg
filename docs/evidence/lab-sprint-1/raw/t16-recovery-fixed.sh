#!/bin/bash
# recovery_fixed per file system and WAL size: prepare (commit MiB of WAL,
# drop without close), list the WAL segments, then a cold open under
# strace -c -f; a second prepare + cold open under perf record.
export LC_ALL=C
OUT=$1; : > $OUT/t16-recovery-fixed-log.txt
for fs in ext4 xfs; do
 B=/data/tmp/lpg-lab; [ $fs = xfs ] && B=/mnt/lpg-xfs
 for mib in 16 64 256; do
  for mode in strace perf; do
   D=$B/recovery-fixed; rm -rf $D; mkdir -p $D
   LPG_TEST_DIR=$D LPG_RECOVERY_STEP=prepare LPG_RECOVERY_MIB=$mib $BIN --exact lab::recovery::recovery_fixed --ignored --test-threads=1 --nocapture > /dev/null 2>&1
   echo "fs=$fs mib=$mib mode=$mode segments=$(ls $D | grep -c '^wal-') wal_bytes_on_disk=$(du -cb $D/wal-* | tail -1 | cut -f1)" >> $OUT/t16-recovery-fixed-log.txt
   if [ $mode = strace ]; then
     LPG_TEST_DIR=$D LPG_RECOVERY_STEP=open strace -f -c -o $OUT/t16-strace-c-recovery-$fs-$mib.txt $BIN --exact lab::recovery::recovery_fixed --ignored --test-threads=1 --nocapture 2>&1 | grep -o "LAB.*" >> $OUT/t16-recovery-fixed-log.txt
   else
     LPG_TEST_DIR=$D LPG_RECOVERY_STEP=open perf record -F 999 --call-graph dwarf,16384 -o $S/perf/recovery-$fs-$mib.data $BIN --exact lab::recovery::recovery_fixed --ignored --test-threads=1 --nocapture 2>&1 | grep -o "LAB.*" >> $OUT/t16-recovery-fixed-log.txt
   fi
  done
 done
 rm -rf $B/recovery-fixed
done
