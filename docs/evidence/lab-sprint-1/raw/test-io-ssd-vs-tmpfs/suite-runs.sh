#!/bin/bash
# Test suite, tmpfs vs ext4 NVMe, same binaries, interleaved
# (tmpfs, ssd, tmpfs, ssd, ...). The ssd configuration makes /tmp/lpgram a
# symlink to /data/tmp/lpg-lab/lpgram; no code or environment variable changes.
# Per run: /usr/bin/time -v, nextest per-test lines, /sys/block/nvme1n1/stat
# before and after, load average before and after.
export LC_ALL=C
OUT=$1; REPS=${REPS:-3}; DEV=nvme1n1; LOG=$OUT/suite-log.txt
cd /data/dev/xumiga/lpg
setup(){ rm -rf /tmp/lpgram; if [ $1 = ssd ]; then mkdir -p /data/tmp/lpg-lab/lpgram; ln -s /data/tmp/lpg-lab/lpgram /tmp/lpgram; else mkdir -p /tmp/lpgram; fi; }
restore(){ rm -f /tmp/lpgram 2>/dev/null || rm -rf /tmp/lpgram; rm -rf /tmp/lpgram /data/tmp/lpg-lab/lpgram; mkdir -p /tmp/lpgram; }
for rep in $(seq 1 $REPS); do
 for cfg in tmpfs ssd; do
  setup $cfg
  echo "$(date +%T) rep$rep $cfg start loadavg $(cut -d' ' -f1-3 /proc/loadavg) lpgram=$(readlink -f /tmp/lpgram) fstype=$(stat -f -L -c %T /tmp/lpgram)" >>$LOG
  echo "$(date +%T.%N) before $cfg rep$rep $(cat /sys/block/$DEV/stat)" >>$LOG
  /usr/bin/time -v -o $OUT/suite-time-$cfg-rep$rep.txt \
    cargo nextest run -p lpg --color never --status-level all --final-status-level none \
    >$OUT/suite-nextest-$cfg-rep$rep.txt 2>&1
  echo "exit $?" >>$OUT/suite-nextest-$cfg-rep$rep.txt
  echo "$(date +%T.%N) after $cfg rep$rep $(cat /sys/block/$DEV/stat)" >>$LOG
  echo "$(date +%T) rep$rep $cfg end loadavg $(cut -d' ' -f1-3 /proc/loadavg) leftover=$(ls -A /tmp/lpgram/ | wc -l)" >>$LOG
  restore
  sleep 5
 done
done
