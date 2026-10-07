# Median time of pwrite(W bytes, sequential) + fdatasync over a file pre-written
# in PREP-byte chunks (16 MiB), N writes. Also reports the run order.
import os, sys, time, statistics
d, prep, w, n, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]
p = os.path.join(d, "sync-probe.dat"); size = 16 << 20
fd = os.open(p, os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o644)
if mode == "write":
    z = bytes(prep)
    for off in range(0, size, prep): os.pwrite(fd, z, off)
else:  # fallocate only (unwritten extents)
    os.posix_fallocate(fd, 0, size)
os.fdatasync(fd); os.close(fd)
fd = os.open(p, os.O_RDWR); buf = b"\x01" * w; t = []
for i in range(n):
    a = time.perf_counter_ns(); os.pwrite(fd, buf, i * w); os.fdatasync(fd); t.append(time.perf_counter_ns() - a)
os.close(fd); os.unlink(p)
t.sort()
print(f"dir={d} prefill={mode}:{prep} write={w} n={n} p50_us={t[n//2]/1e3:.0f} p99_us={t[int(n*0.99)]/1e3:.0f}")
