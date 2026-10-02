#!/bin/bash
# RAM-backed 1 M matrix (LB_RAM=1), then the 100 M edge point for B and G.
S=$(cd "$(dirname "$0")/.." && pwd)   # docs/evidence/layout-bench
W=${LB_WORK:-/data/tmp/lpg-layout-bench}   # page files; large free space needed
B=$S/target/release/layout-bench   # v2 build
R=$S/raw
for cfg in "zipf online" "zipf bulk" "uniform online" "uniform bulk"; do
  set -- $cfg
  tag=ram-$1-$2
  echo "start $tag $(date -Is) load $(cat /proc/loadavg)" >> $R/run-ram.log
  LB_RAM=1 /usr/bin/time -f "wall %e s, maxrss %M KB, exit %x" $B $1 $2 1000000 10000000 $W/1m 1.0 > $R/$tag.md 2> $R/$tag.err
  echo "end $tag $(date -Is) load $(cat /proc/loadavg) | $(tail -1 $R/$tag.err)" >> $R/run-ram.log
done
echo "start large $(date -Is) load $(cat /proc/loadavg) free $(free -g | awk '/Mem/{print $7}')G avail, /data $(df -B1G --output=avail /data | tail -1)G" >> $R/run-ram.log
/usr/bin/time -f "wall %e s, maxrss %M KB, exit %x" $B large zipf 10000000 100000000 $W/large BG 1024 > $R/large-zipf-bulk.md 2> $R/large-zipf-bulk.err
echo "end large $(date -Is) load $(cat /proc/loadavg) | $(tail -1 $R/large-zipf-bulk.err)" >> $R/run-ram.log
echo ALLDONE >> $R/run-ram.log
