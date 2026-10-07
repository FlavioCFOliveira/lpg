#!/bin/bash
# Task #15 session: per rep and file system, the fio overwrite+fdatasync
# ceiling (5 s), then lab::commit::commit_fixed at 1, 4 and 16 threads.
# Each run is bracketed by /sys/block/{nvme0n1,loop80}/stat; iostat -x -t 1
# runs throughout. Usage: BIN=... OUT=... t15-session.sh
export LC_ALL=C LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs
OUT=${OUT:-out}; LOG=$OUT/t15-session-log.txt; : > $LOG
iostat -x -t -y nvme0n1 loop80 1 > $OUT/t15-session-iostat.txt &
IOSTAT=$!
stat_line() { echo "$(date +%s%3N) nvme0n1 $(cat /sys/block/nvme0n1/stat) loop80 $(cat /sys/block/loop80/stat)"; }
n_for() { # fs threads -> commits per thread (runs of about 5 s)
  case "$1-$2" in
    ext4-1) echo 5000;; ext4-4) echo 2500;; ext4-16) echo 2500;;
    xfs-1) echo 1000;; xfs-4) echo 500;; xfs-16) echo 400;;
  esac; }
echo "load $(cat /proc/loadavg)" >> $LOG
for rep in 1 2 3; do
  for fs in ext4 xfs; do
    D=$LPG_LAB_EXT4; [ $fs = xfs ] && D=$LPG_LAB_XFS
    rm -f $D/fio-over.dat
    fio --name=layout --filename=$D/fio-over.dat --size=256M --bs=1M --rw=write --ioengine=psync --end_fsync=1 --output=/dev/null
    echo "BEGIN fio rep=$rep fs=$fs $(stat_line)" >> $LOG
    fio --name=overwrite-16k-fdatasync --filename=$D/fio-over.dat --size=256M --bs=16k --rw=write \
        --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --fdatasync=1 --time_based --runtime=5 \
        --output-format=json --output=$OUT/t15-session-fio-$fs-rep$rep.json
    echo "END fio rep=$rep fs=$fs $(stat_line)" >> $LOG
    rm -f $D/fio-over.dat
    for t in 1 4 16; do
      echo "BEGIN lpg rep=$rep fs=$fs threads=$t $(stat_line)" >> $LOG
      LPG_LAB_FS=$fs LPG_COMMIT_THREADS=$t LPG_PERF_N=$(n_for $fs $t) \
        $BIN --exact lab::commit::commit_fixed --ignored --test-threads=1 --nocapture 2>&1 \
        | grep -o "LAB.*" | sed "s/^/rep=$rep /" >> $LOG
      echo "END lpg rep=$rep fs=$fs threads=$t $(stat_line)" >> $LOG
    done
  done
done
echo "load $(cat /proc/loadavg)" >> $LOG
kill $IOSTAT
