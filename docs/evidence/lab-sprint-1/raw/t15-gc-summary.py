# Summarises the group-commit variants: per threads and variant, median
# [min, max] over reps of p50, p99, commits/s, commits/group; context
# switches and task-clock ms per commit = (run - run with 0 commits of the
# same rep) / commits; nvme flush await per run.
import re, sys, statistics as st, collections
out = sys.argv[1]
def perf(path):
    d = {}
    for l in open(path):
        p = l.strip().split(",")
        if len(p) > 3 and p[2] in ("context-switches", "task-clock"): d[p[2]] = float(p[0])
    return d
R = collections.defaultdict(list)
for l in open(f"{out}/gc-runs.txt"):
    kv = dict(x.split("=", 1) for x in l.split() if "=" in x)
    if "n" not in kv: continue
    t, v, rep = kv["threads"], kv["variant"], kv["rep"]
    a = perf(f"{out}/gc-perf-{v}-t{t}-n{kv['n']}-rep{rep}.txt"); b = perf(f"{out}/gc-perf-{v}-t{t}-n0-rep{rep}.txt")
    c = int(kv["commits"])
    R[(int(t), v)].append(dict(p50=float(kv["p50_us"]), p99=float(kv["p99_us"]), cps=float(kv["commits_per_s"]),
        cpg=c / int(kv["groups"]), cs=(a["context-switches"] - b["context-switches"]) / c,
        cpu=(a["task-clock"] - b["task-clock"]) / c * 1e3, fa=float(kv["f_await_ms"])))
def mr(v, f="{:.0f}"):
    return f"{f.format(st.median(v))} [{f.format(min(v))}, {f.format(max(v))}]"
print("| threads | variant | p50 µs | p99 µs | commits/s | commits/group | context switches/commit | CPU µs/commit | nvme f_await ms per rep |")
print("|---|---|---|---|---|---|---|---|---|")
order = ["base", "slots", "d20", "d50", "d100", "d200"]
for (t, v) in sorted(R, key=lambda k: (k[0], order.index(k[1]))):
    r = R[(t, v)]
    print(f"| {t} | {v} | {mr([x['p50'] for x in r])} | {mr([x['p99'] for x in r])} | {mr([x['cps'] for x in r])} | {mr([x['cpg'] for x in r], '{:.2f}')} | {mr([x['cs'] for x in r], '{:.2f}')} | {mr([x['cpu'] for x in r], '{:.1f}')} | {', '.join(f'{x['fa']:.2f}' for x in r)} |")
