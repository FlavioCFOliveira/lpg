#!/bin/bash
# Device level, ext4 on NVMe (/data/tmp/lpg-lab) vs tmpfs (/tmp/lpgram/fio),
# interleaved, 3 reps, 10 s per job, one job, psync, iodepth 1, buffered
# (O_DIRECT is not supported on tmpfs, so no job uses it).
#  a overwrite-8k-fdatasync: 8 KiB sequential writes + fdatasync over a 256 MiB
#    file fully written first (the overwrite pattern of t15-fio-ceiling.sh, bs 8k).
#  b randread-8k-buffered: 8 KiB random reads of a 256 MiB file just written
#    (page-cache resident on both file systems).
#  c write-8k-nosync: 8 KiB sequential writes of a new 512 MiB file, no sync.
# /sys/block/nvme1n1/stat is read before and after every job (both fs).
export LC_ALL=C
OUT=$1; DEV=nvme1n1; LOG=$OUT/fio-log.txt
st(){ echo "$(date +%T.%N) $1 $2 $3 $(cat /sys/block/$DEV/stat)" >>$LOG; }
for rep in 1 2 3; do
 for fs in ext4 tmpfs; do
  D=/data/tmp/lpg-lab; [ $fs = tmpfs ] && { D=/tmp/lpgram/fio; mkdir -p $D; }
  echo "$(date +%T) rep$rep $fs loadavg $(cut -d' ' -f1-3 /proc/loadavg)" >>$LOG
  rm -f $D/f.dat
  fio --name=layout --filename=$D/f.dat --size=256M --bs=1M --rw=write --ioengine=psync --end_fsync=1 --output=/dev/null
  st before a-$fs rep$rep
  fio --name=overwrite-8k-fdatasync --filename=$D/f.dat --size=256M --bs=8k --rw=write \
      --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --fdatasync=1 --time_based --runtime=10 \
      --output-format=json --output=$OUT/fio-a-overwrite-fdatasync-$fs-rep$rep.json
  st after a-$fs rep$rep
  st before b-$fs rep$rep
  fio --name=randread-8k-buffered --filename=$D/f.dat --size=256M --bs=8k --rw=randread \
      --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --randrepeat=0 --norandommap --time_based --runtime=10 \
      --output-format=json --output=$OUT/fio-b-randread-$fs-rep$rep.json
  st after b-$fs rep$rep
  rm -f $D/f.dat
  st before c-$fs rep$rep
  fio --name=write-8k-nosync --filename=$D/f.dat --size=512M --bs=8k --rw=write \
      --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 \
      --output-format=json --output=$OUT/fio-c-write-nosync-$fs-rep$rep.json
  st after c-$fs rep$rep
  rm -f $D/f.dat
  [ $fs = tmpfs ] && rmdir $D
 done
done
