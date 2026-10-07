# Summarises t15-session-log.txt: per run, device figures computed as
# iostat -x computes them, from /sys/block/<dev>/stat deltas over the run
# (fields per Documentation/block/stat.rst: 5 write I/Os, 7 write sectors,
# 8 write ticks ms, 16 flush I/Os, 17 flush ticks ms), plus the fio sync
# latency of the same rep and file system.
import json, re, sys
out = sys.argv[1]
lines = open(f"{out}/t15-session-log.txt").read().splitlines()
def parse_stat(l):
    m = re.search(r"(\d{13}) nvme0n1 (.*) loop80 (.*)$", l)
    return int(m.group(1)), list(map(int, m.group(2).split())), list(map(int, m.group(3).split()))
def dev(a, b, secs):
    d = [y - x for x, y in zip(a, b)]
    w, ws, wt, f, ft = d[4], d[6], d[7], d[15], d[16]
    return dict(w_s=w / secs, wkB_s=ws * 512 / 1024 / secs, w_await=wt / w if w else 0,
                f_s=f / secs, f_await=ft / f if f else 0, flushes=f, writes=w, wkB=ws * 512 / 1024)
rows = []
begin = None
lab = None
for l in lines:
    if l.startswith("BEGIN"):
        begin = (l, parse_stat(l))
    elif l.startswith("rep=") and "LAB" in l:
        lab = dict(kv.split("=", 1) for kv in l.replace("LAB ", "").split())
    elif l.startswith("END"):
        e = parse_stat(l); b = begin[1]
        tag = dict(kv.split("=", 1) for kv in begin[0].split()[1:4] if "=" in kv)
        kind = begin[0].split()[1]
        if kind == "fio":
            j = json.load(open(f"{out}/t15-session-fio-{tag['fs']}-rep{tag['rep']}.json"))["jobs"][0]
            secs = (e[0] - b[0]) / 1e3
            rows.append(dict(kind="fio", rep=tag["rep"], fs=tag["fs"], threads="-",
                             p50=j["sync"]["lat_ns"]["percentile"]["50.000000"] / 1e3,
                             p99=j["sync"]["lat_ns"]["percentile"]["99.000000"] / 1e3,
                             rate=j["write"]["iops"], groups=j["sync"]["lat_ns"]["N"],
                             nv=dev(b[1], e[1], secs), lo=dev(b[2], e[2], secs)))
        else:
            # The lpg run includes the untimed build; device figures are taken over
            # the whole process. The timed phase is start_ms..end_ms.
            secs = (e[0] - b[0]) / 1e3
            rows.append(dict(kind="lpg", rep=lab["rep"], fs=lab["fs"], threads=lab["threads"],
                             p50=float(lab["p50_us"]), p99=float(lab["p99_us"]),
                             rate=float(lab["commits_per_s"]), groups=int(lab["groups"]),
                             commits=int(lab["commits"]), seconds=float(lab["seconds"]),
                             nv=dev(b[1], e[1], secs), lo=dev(b[2], e[2], secs)))
print("| rep | fs | run | threads | p50 µs | p99 µs | commits/s (fio IOPS) | commits/group | nvme f_await ms | nvme w_await ms | loop80 w_await ms |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for r in rows:
    cg = f"{r['commits']/r['groups']:.2f}" if r["kind"] == "lpg" else "-"
    lo = f"{r['lo']['w_await']:.2f}" if r["fs"] == "xfs" else "-"
    print(f"| {r['rep']} | {r['fs']} | {r['kind']} | {r['threads']} | {r['p50']:.0f} | {r['p99']:.0f} | {r['rate']:.0f} | {cg} | {r['nv']['f_await']:.2f} | {r['nv']['w_await']:.2f} | {lo} |")
