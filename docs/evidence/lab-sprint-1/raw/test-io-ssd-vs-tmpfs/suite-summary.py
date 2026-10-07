import re,glob,sys,statistics as S
R=sys.argv[1]
def rng(v): v=sorted(v); return f"{S.median(v):.3f} [{v[0]:.3f}-{v[-1]:.3f}]"
t={}
for cfg in ["tmpfs","ssd"]:
  wall=[];usr=[];sys_=[]
  for f in sorted(glob.glob(f"{R}/suite-time-{cfg}-rep*.txt")):
    s=open(f).read()
    m=re.search(r"Elapsed.*: (\d+):([\d.]+)",s); wall.append(int(m[1])*60+float(m[2]))
    usr.append(float(re.search(r"User time.*: ([\d.]+)",s)[1])); sys_.append(float(re.search(r"System time.*: ([\d.]+)",s)[1]))
  t[cfg]=(wall,usr,sys_)
  print(f"{cfg}: wall {rng(wall)} s, user {rng(usr)} s, sys {rng(sys_)} s, user+sys {rng([a+b for a,b in zip(usr,sys_)])}")
w=t["tmpfs"][0]; s=t["ssd"][0]
ratios=[b/a for a in w for b in s]
print(f"speed-up ssd/tmpfs wall: median-ratio {S.median(s)/S.median(w):.3f}, all 9 pairwise ratios [{min(ratios):.3f}-{max(ratios):.3f}], paired by rep {[round(b/a,3) for a,b in zip(w,s)]}")
d={}
for cfg in ["tmpfs","ssd"]:
  for f in glob.glob(f"{R}/suite-nextest-{cfg}-rep*.txt"):
    for m in re.finditer(r"PASS \[\s*([\d.]+)s\] \([^)]*\) lpg (\S+)",open(f).read()):
      d.setdefault(m[2],{}).setdefault(cfg,[]).append(float(m[1]))
rows=[(S.median(v["ssd"])-S.median(v["tmpfs"]),k,v) for k,v in d.items()]
rows.sort(reverse=True)
print(f"sum of per-test median durations: tmpfs {sum(S.median(v['tmpfs']) for _,_,v in rows):.2f} s, ssd {sum(S.median(v['ssd']) for _,_,v in rows):.2f} s")
print("top 15 by ssd-tmpfs median difference (s): diff | tmpfs median [range] | ssd median [range] | test")
for diff,k,v in rows[:15]: print(f"{diff:+.3f} | {rng(v['tmpfs'])} | {rng(v['ssd'])} | {k}")
print("bottom 5:")
for diff,k,v in rows[-5:]: print(f"{diff:+.3f} | {rng(v['tmpfs'])} | {rng(v['ssd'])} | {k}")
