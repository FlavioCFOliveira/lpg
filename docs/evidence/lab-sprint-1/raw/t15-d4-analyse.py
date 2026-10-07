# D4: commits whose interval [end - latency, end] overlaps a segment
# creation [start, end] versus the others.
import glob, statistics as st, sys
def pct(v, q):
    v = sorted(v); return v[min(len(v) - 1, max(0, int(-(-q * len(v) // 1)) - 1))] if v else float("nan")
print("| trace | segments | creation ms median [min, max] | commits overlapping | their p50 / max ms | other commits p50 / p99 / max ms | top-10 slowest commits overlapping a creation |")
print("|---|---|---|---|---|---|---|")
for f in sorted(glob.glob(sys.argv[1] + "/trace-*.txt")):
    C, S = [], []
    for l in open(f):
        p = l.split()
        if p[0] == "c": C.append((int(p[1]) - int(p[2]), int(p[1])))
        else: S.append((int(p[1]), int(p[2])))
    if not S: continue
    inside = [b - a for a, b in C if any(a < se and b > ss for ss, se in S)]
    other = [b - a for a, b in C if not any(a < se and b > ss for ss, se in S)]
    top = sorted(C, key=lambda x: x[0] - x[1])[:10]
    ntop = sum(1 for a, b in top if any(a < se and b > ss for ss, se in S))
    d = [(e - s) / 1e6 for s, e in S]
    print(f"| {f.split('/')[-1][6:-4]} | {len(S)} | {st.median(d):.1f} [{min(d):.1f}, {max(d):.1f}] | {len(inside)} of {len(C)} | {pct(inside,.5)/1e6:.2f} / {max(inside)/1e6:.2f} | {pct(other,.5)/1e6:.2f} / {pct(other,.99)/1e6:.2f} / {max(other)/1e6:.2f} | {ntop} of 10 |")
