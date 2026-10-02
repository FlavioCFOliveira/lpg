#!/usr/bin/env python3
"""Assembles ../results.md: hand-written head (head.md) + generated tables + tail (tail.md)."""
import os, re, sys
sys.path.insert(0, os.path.dirname(__file__))
from mk_tables import parse, table

S = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(S, "..", "raw")
CFG = ["zipf-online", "zipf-bulk", "uniform-online", "uniform-bulk"]


def raw_tables(path, titles):
    """Copies raw markdown tables that follow a heading starting with one of `titles` (first occurrence)."""
    out, take, seen = [], False, set()
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("### ") or line.startswith("## "):
            t = line.lstrip("# ")
            take = any(t.startswith(x) for x in titles) and t not in seen
            if take:
                seen.add(t)
                out.append(f"\n**{t}**\n")
            continue
        if take and (line.startswith("|") or line.startswith("expected") or line.startswith("RSS")):
            out.append(line)
    return "\n".join(out)


def gen_tables(path, keep):
    secs, _ = parse(path)
    out = []
    for name, sec in secs.items():
        if "warm-up" in name or not keep(name):
            continue
        mode = "dev" if "device" in name else ("ram" if "buffered" in name else "warm")
        out.append(f"\n**{name}**\n")
        out.append(table(sec, mode))
    return "\n".join(out)


parts = [open(os.path.join(S, "head.md")).read()]
for c in CFG:
    d = os.path.join(R, c + ".md")
    r = os.path.join(R, "ram-" + c + ".md")
    hdr = open(d).readline().strip("# \n")
    parts.append(f"\n## 1 M nodes / 10 M edges: {c}\n\n`{hdr}`")
    parts.append(raw_tables(d, ["Build", "CSR projection", "Paged CSR", "Space"]))
    parts.append(gen_tables(d, lambda n: n.startswith("Warm")))
    parts.append(gen_tables(d, lambda n: "device" in n))
    parts.append(gen_tables(r, lambda n: "buffered" in n))
    parts.append(raw_tables(d, ["Online single-edge insert", "Delete", "MVCC visibility"]))
L = os.path.join(R, "large-zipf-bulk.md")
parts.append(f"\n## 10 M nodes / 100 M edges: zipf, bulk load (B and G)\n\n`{open(L).readline().strip('# ').strip()}`")
parts.append(raw_tables(L, ["Bulk load", "Space"]))
parts.append(gen_tables(L, lambda n: True))
parts.append(open(os.path.join(S, "tail.md")).read())
open(os.path.join(S, "..", "results.md"), "w").write("\n".join(parts) + "\n")
print("written")
