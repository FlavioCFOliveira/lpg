# For each lpg run of t15-session-log.txt: the iostat -x 1 s intervals that lie
# entirely inside the measured phase [start_ms, end_ms]; mean w/s, wkB/s,
# w_await, f/s, f_await per device, and wkB per commit (nvme0n1).
import re, sys, datetime, statistics as st
out = sys.argv[1]
iv = []  # (end_epoch_s, {dev: fields})
cur = None; hdr = None
for l in open(f"{out}/t15-session-iostat.txt"):
    m = re.match(r"(\d\d)/(\d\d)/(\d\d) (\d\d:\d\d:\d\d)", l)
    if m:
        t = datetime.datetime.strptime(f"20{m.group(3)}-{m.group(1)}-{m.group(2)} {m.group(4)}", "%Y-%m-%d %H:%M:%S")
        cur = (t.timestamp(), {}); iv.append(cur); continue
    p = l.split()
    if p and p[0] == "Device": hdr = p; continue
    if p and hdr and p[0] in ("nvme0n1", "loop80") and cur: cur[1][p[0]] = dict(zip(hdr[1:], map(float, p[1:])))
print("| rep | fs | threads | intervals | commits/s | device | w/s | wkB/s | w_await ms | f/s | f_await ms | wkB/commit |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|")
for l in open(f"{out}/t15-session-log.txt"):
    if not (l.startswith("rep=") and "LAB" in l): continue
    kv = dict(x.split("=", 1) for x in l.replace("LAB ", "").split())
    a, b = int(kv["start_ms"]) / 1e3, int(kv["end_ms"]) / 1e3
    sel = [d for e, d in iv if e - 1 >= a and e <= b]
    cps = float(kv["commits_per_s"])
    for dev in (["nvme0n1"] if kv["fs"] == "ext4" else ["loop80", "nvme0n1"]):
        g = lambda k: st.mean(d[dev][k] for d in sel) if sel else float("nan")
        print(f"| {kv['rep']} | {kv['fs']} | {kv['threads']} | {len(sel)} | {cps:.0f} | {dev} | {g('w/s'):.0f} | {g('wkB/s'):.0f} | {g('w_await'):.2f} | {g('f/s'):.0f} | {g('f_await'):.2f} | {g('wkB/s')/cps:.1f} |")
