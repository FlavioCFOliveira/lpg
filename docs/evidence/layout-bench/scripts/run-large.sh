#!/bin/bash
# Large scale point: 10 M nodes / 100 M edges, Zipf, bulk; arms given as $1.
S=$(cd "$(dirname "$0")/.." && pwd)   # docs/evidence/layout-bench
W=${LB_WORK:-/data/tmp/lpg-layout-bench}   # page files; large free space needed
B=$S/target/release/layout-bench   # v2 build
R=$S/raw
echo "start large $(date -Is) load $(cat /proc/loadavg) df $(df -B1G --output=avail /data | tail -1)G" >> $R/run-large.log
/usr/bin/time -f "wall %e s, maxrss %M KB, exit %x" $B large zipf 10000000 100000000 $W/large $1 1024 > $R/large-zipf-bulk.md 2> $R/large-zipf-bulk.err
echo "end large $(date -Is) load $(cat /proc/loadavg) | $(tail -1 $R/large-zipf-bulk.err)" >> $R/run-large.log
echo ALLDONE >> $R/run-large.log
