#!/bin/bash
# Task #11 session: fio ceiling jobs (as task #7), lab::storage, fio again.
set -euo pipefail
cd /data/dev/xumiga/lpg
RAW=docs/evidence/lab-sprint-1/raw
LOG=$RAW/t11-session-log.txt
declare -A DIR=([ext4]=/data/tmp/lpg-lab [xfs]=/mnt/lpg-xfs)
: > $LOG
log() { echo "$(date -Iseconds) $*" | tee -a $LOG; }
for fs in ext4 xfs; do
  D=${DIR[$fs]}
  log "layout $fs"
  fio --name=layout --filename=$D/fio-randread.dat --size=4G --bs=1M --rw=write --create_only=1 >/dev/null
  fio --name=layout --filename=$D/fio-randread.dat --size=4G --bs=1M --rw=write --direct=1 --ioengine=psync >/dev/null
done
fioset() {
  local tag=$1
  for job in randread fsyncwrite; do for fs in ext4 xfs; do
    D=${DIR[$fs]}
    log "fio $job $fs $tag start; load $(cut -d' ' -f1 /proc/loadavg)"
    if [ $job = randread ]; then
      fio --name=randread-8k --filename=$D/fio-randread.dat --size=4G --bs=8k --rw=randread \
        --direct=1 --ioengine=psync --iodepth=1 --numjobs=1 --time_based --runtime=30 \
        --randrepeat=0 --norandommap --output-format=json --output=$RAW/t11-fio-randread-$fs-$tag.json
    else
      rm -f $D/fio-fsyncwrite.dat
      fio --name=seqwrite-8k-fdatasync --filename=$D/fio-fsyncwrite.dat --size=1G --bs=8k --rw=write \
        --direct=0 --ioengine=psync --iodepth=1 --numjobs=1 --fdatasync=1 --time_based --runtime=30 \
        --output-format=json --output=$RAW/t11-fio-fsyncwrite-$fs-$tag.json
      rm -f $D/fio-fsyncwrite.dat
    fi
  done; done
}
fioset before
log "lab::storage start; load $(cut -d' ' -f1 /proc/loadavg)"
LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs LPG_LAB_OUT=$RAW \
  cargo test -p lpg --profile lab --lib -- --ignored lab::storage 2>&1 | tee -a $LOG
log "lab::storage end"
fioset after
rm -f /data/tmp/lpg-lab/fio-randread.dat /mnt/lpg-xfs/fio-randread.dat
log "done"
