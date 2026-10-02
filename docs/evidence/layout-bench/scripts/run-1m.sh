#!/bin/bash
# Runs the 1 M node / 10 M edge matrix sequentially. Page files on /data (970 EVO Plus).
S=$(cd "$(dirname "$0")/.." && pwd)   # docs/evidence/layout-bench
W=${LB_WORK:-/data/tmp/lpg-layout-bench}   # page files; large free space needed
B=$S/src-v1/target/release/layout-bench   # v1 build (device matrix)
R=$S/raw
for cfg in "zipf online" "zipf bulk" "uniform online" "uniform bulk"; do
  set -- $cfg
  tag=$1-$2
  echo "start $tag $(date -Is) load $(cat /proc/loadavg)" >> $R/run-1m.log
  /usr/bin/time -f "wall %e s, maxrss %M KB, exit %x" $B $1 $2 1000000 10000000 $W/1m 1.0 > $R/$tag.md 2> $R/$tag.err
  echo "end $tag $(date -Is) load $(cat /proc/loadavg) | $(tail -1 $R/$tag.err)" >> $R/run-1m.log
done
echo ALLDONE >> $R/run-1m.log
