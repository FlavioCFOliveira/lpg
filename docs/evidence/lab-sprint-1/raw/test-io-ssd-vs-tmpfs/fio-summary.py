import json,glob,sys,re,statistics as S
# /sys/block stat fields (Documentation/block/stat.rst): d[0] read I/Os, d[4] write I/Os, d[15] flush I/Os, d[16] flush ticks ms.
R=sys.argv[1]
def med(v): v=sorted(v); return f"{S.median(v):.1f} [{v[0]:.1f}-{v[-1]:.1f}]"
for job,key in [("a-overwrite-fdatasync","write"),("b-randread","read"),("c-write-nosync","write")]:
  for fs in ["ext4","tmpfs"]:
    iops=[];p50=[];p99=[];sp50=[];sp99=[];bw=[]
    for f in sorted(glob.glob(f"{R}/fio-{job}-{fs}-rep*.json")):
      j=json.load(open(f))["jobs"][0]; d=j[key]
      iops.append(d["iops"]); bw.append(d["bw_bytes"]/2**20)
      p50.append(d["clat_ns"]["percentile"]["50.000000"]/1e3); p99.append(d["clat_ns"]["percentile"]["99.000000"]/1e3)
      if "fdatasync" in job:
        s=j["sync"]["lat_ns"]; sp50.append(s["percentile"]["50.000000"]/1e3); sp99.append(s["percentile"]["99.000000"]/1e3)
    print(f"{job} {fs}: IOPS {med(iops)} MiB/s {med(bw)} clat p50us {med(p50)} p99us {med(p99)}" + (f" fdatasync p50us {med(sp50)} p99us {med(sp99)}" if sp50 else ""))
# device counters
L=[l.split() for l in open(f"{R}/fio-log.txt") if " before " in l or " after " in l]
b={}
for l in L:
  k=(l[2],l[3]); f=list(map(int,l[4:]))
  if l[1]=="before": b[k]=f
  else:
    d=[x-y for x,y in zip(f,b[k])]
    fa=d[15] and d[16]/d[15]
    print(f"{k[0]} {k[1]}: nvme reads {d[0]} writes {d[4]} flush_ios {d[15]} flush_ms {d[16]} f_await_ms {fa:.2f}" if d[15] else f"{k[0]} {k[1]}: nvme reads {d[0]} writes {d[4]} flush_ios 0")
