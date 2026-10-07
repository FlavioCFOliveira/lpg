import re, sys, collections, statistics
rows=[]
for l in open(sys.argv[1]):
    d=dict(re.findall(r'(\w+)=([\w.]+)', l))
    if 'p50_us' not in d: continue
    fa=int(d['f_ms'])/int(d['f_ios'])
    d['f_await']=fa
    d['state']='fast' if fa<1.3 else ('slow' if fa>2.3 else 'mixed')
    rows.append(d)
g=collections.defaultdict(list)
for d in rows: g[(d['state'],int(d['threads']),d['variant'])].append(d)
def mr(v):
    v=sorted(v); m=statistics.median(v); return f"{m:.0f} [{v[0]:.0f}, {v[-1]:.0f}]"
print("| state | threads | variant | runs | f_await ms | p50 µs | p99 µs | max µs | commits/s | commits/group |")
print("|---|---|---|---|---|---|---|---|---|---|")
for k in sorted(g):
    v=g[k]
    fa=sorted(x['f_await'] for x in v)
    print(f"| {k[0]} | {k[1]} | {k[2]} | {len(v)} | {fa[0]:.2f}-{fa[-1]:.2f} | {mr([float(x['p50_us']) for x in v])} | {mr([float(x['p99_us']) for x in v])} | {mr([float(x['max_us']) for x in v])} | {mr([float(x['commits_per_s']) for x in v])} | {statistics.median([int(x['commits'])/int(x['groups']) for x in v]):.2f} |")
