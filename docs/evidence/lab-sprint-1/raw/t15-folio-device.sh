#!/bin/bash
# Device sectors written (field 7 of /sys/block/<dev>/stat) during 200 synced
# 16 448-byte writes into a file pre-written in PREP-byte chunks (XFS).
for prep in 4096 1048576 4096 1048576 4096 1048576; do
  python3 - "$prep" <<'PY' &
import os, sys, time
prep=int(sys.argv[1]); p="/mnt/lpg-xfs/folio-dev.dat"; size=16<<20
fd=os.open(p, os.O_RDWR|os.O_CREAT|os.O_TRUNC, 0o644)
for off in range(0,size,prep): os.pwrite(fd, bytes(prep), off)
os.fdatasync(fd); os.close(fd)
open("/tmp/claude-1000/-data-dev-xumiga-lpg/0f12d8f8-5752-44b8-ac3b-293650f54616/scratchpad/lab/folio-ready","w").close()
time.sleep(1.0)
fd=os.open(p, os.O_RDWR); buf=b"\x01"*16448
for i in range(200):
    os.pwrite(fd, buf, i*16448); os.fdatasync(fd)
os.close(fd); time.sleep(1.0); os.unlink(p)
PY
  pid=$!
  while [ ! -e folio-ready ]; do sleep 0.05; done; rm -f folio-ready
  sleep 0.5
  a=$(awk '{print $7, $5, $16}' /sys/block/loop80/stat); an=$(awk '{print $7}' /sys/block/nvme0n1/stat)
  wait $pid
  b=$(awk '{print $7, $5, $16}' /sys/block/loop80/stat); bn=$(awk '{print $7}' /sys/block/nvme0n1/stat)
  set -- $a $b
  echo "prep=$prep loop80: sectors_written=$(( $4-$1 )) KiB/write=$(( ($4-$1)*512/1024/200 )) write_ios=$(( $5-$2 )) flushes=$(( $6-$3 )); nvme0n1 KiB/write=$(( (bn-an)*512/1024/200 ))"
done
