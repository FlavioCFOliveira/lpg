#!/bin/bash
# fdatasync ceiling in this session, interleaved, 3 reps x 10 s:
#  append: the task #7 job (8 KiB writes to a new file, fdatasync each);
#  overwrite: 16 KiB writes + fdatasync over a file fully written first
#  (the WAL pattern: segments are zero-filled at creation).
export LC_ALL=C
OUT=${OUT:-out}
for rep in 1 2 3; do
 for fs in ext4 xfs; do
  D=/data/tmp/lpg-lab; [ $fs = xfs ] && D=/mnt/lpg-xfs
  rm -f $D/fio-append.dat
  fio --name=append-8k-fdatasync --filename=$D/fio-append.dat --size=1G --bs=8k --rw=write \
      --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --fdatasync=1 --time_based --runtime=10 \
      --output-format=json --output=$OUT/t15-fio-append-$fs-rep$rep.json
  rm -f $D/fio-append.dat
  rm -f $D/fio-over.dat
  fio --name=layout --filename=$D/fio-over.dat --size=256M --bs=1M --rw=write --ioengine=psync --end_fsync=1 --output=/dev/null
  fio --name=overwrite-16k-fdatasync --filename=$D/fio-over.dat --size=256M --bs=16k --rw=write \
      --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --fdatasync=1 --time_based --runtime=10 \
      --output-format=json --output=$OUT/t15-fio-overwrite-$fs-rep$rep.json
  rm -f $D/fio-over.dat
 done
done
