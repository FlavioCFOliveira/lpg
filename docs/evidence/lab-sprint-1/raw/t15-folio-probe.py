# Probe: /proc/self/io write_bytes per small write into a file pre-written
# with chunks of PREP bytes, with or without fdatasync after each write.
import os, sys
d, prep, sync, n = sys.argv[1], int(sys.argv[2]), sys.argv[3] == "1", int(sys.argv[4])
p = os.path.join(d, "folio-probe.dat")
size = 16 << 20
def io():
    t = open("/proc/self/io").read().split("\n")
    return {l.split(":")[0]: int(l.split(":")[1]) for l in t if l}
fd = os.open(p, os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o644)
z = bytes(prep)
for off in range(0, size, prep):
    os.pwrite(fd, z, off)
os.fdatasync(fd)
os.close(fd)
fd = os.open(p, os.O_RDWR)
buf = b"\x01" * 16448
a = io()
for i in range(n):
    os.pwrite(fd, buf, i * 16448)
    if sync:
        os.fdatasync(fd)
b = io()
os.fdatasync(fd)
c = io()
print(f"dir={d} prep={prep} sync={int(sync)} n={n} write_bytes/write={(b['write_bytes']-a['write_bytes'])/n:.0f} after_final_sync/write={(c['write_bytes']-a['write_bytes'])/n:.0f} wchar/write={(b['wchar']-a['wchar'])/n:.0f}")
os.close(fd); os.unlink(p)
