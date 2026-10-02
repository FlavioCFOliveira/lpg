#!/usr/bin/env python3
"""Builds compact tables from layout-bench raw outputs (stdout markdown)."""
import sys, re

SHORT = {
    "1-hop OUT, all types, uniform start": "1h-u",
    "1-hop OUT, type = 2, uniform start": "1h-t2",
    "1-hop OUT, all types, degree-weighted start": "1h-dw",
    "2-hop OUT (distinct), uniform start": "2h-u",
    "2-hop OUT (distinct), degree-weighted start": "2h-dw",
    "3-hop OUT (distinct), uniform start": "3h-u",
    "BFS OUT (unbounded depth), degree-weighted start": "BFS",
}


def parse(path):
    secs = {}  # section -> workload -> layout -> cells
    plain = {}  # section -> list of table rows
    sec = None
    wl = None
    hdr = []
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("## ") or line.startswith("### "):
            sec = line.lstrip("# ").strip()
            wl = None
            continue
        m = re.match(r"^(.*?) \(\d+ starts/round", line)
        if m and m.group(1) in SHORT:
            wl = SHORT[m.group(1)]
            continue
        if line.startswith("| layout"):
            hdr = [c.strip() for c in line.strip("|").split("|")]
            continue
        if line.startswith("| ") and not line.startswith("| source"):
            cells = [c.strip() for c in line.strip("|").split("|")]
            if wl and sec:
                secs.setdefault(sec, {}).setdefault(wl, {})[cells[0]] = dict(zip(hdr, cells))
            elif sec:
                plain.setdefault(sec, []).append(cells)
    return secs, plain


def us(ns_cell):
    """'97676 [96572, 100560]' -> '97.7 [96.6-100.6]' in microseconds."""
    if ns_cell == "not run":
        return "not run"
    m = re.match(r"(\d+) \[(\d+), (\d+)\]", ns_cell)
    a, b, c = (int(x) / 1000 for x in m.groups())
    f = lambda x: f"{x:.3g}" if x < 1000 else f"{x:,.0f}"
    return f"{f(a)} [{f(b)}-{f(c)}]"


def table(sec, mode):
    wls = [w for w in SHORT.values() if w in sec]
    lays = []
    for w in wls:
        for l in sec[w]:
            if l not in lays:
                lays.append(l)
    out = ["| layout | " + " | ".join(wls) + " |", "|---|" + "---|" * len(wls)]
    maxops = {w: max(int(c["ops done"]) for c in sec[w].values()) for w in wls}
    for l in lays:
        row = []
        for w in wls:
            c = sec[w].get(l)
            if not c:
                row.append("-")
                continue
            med = c["ns/op median [min, max]"]
            s = us(med)
            if med != "not run" and mode == "dev":
                s += f"; {c['device KiB/op']} KiB"
            if med != "not run" and mode == "ram" and c["misses/op"] != "-":
                s += f"; {c['misses/op']} miss"
                if "seq miss %" in c:
                    s += f" ({c['seq miss %']}% seq)"
            if med != "not run" and w in ("1h-dw", "2h-dw", "3h-u", "BFS"):
                s += f"; {c['ns/edge']} ns/e"
            if med != "not run" and int(c["ops done"]) < maxops[w]:
                s += f" ({c['ops done']} ops)"
            row.append(s)
        out.append(f"| {l} | " + " | ".join(row) + " |")
    return "\n".join(out)


if __name__ == "__main__":
    secs, plain = parse(sys.argv[1])
    for name, sec in secs.items():
        if "warm-up" in name:
            continue
        print(f"\n#### {name}\n")
        mode = "dev" if "device" in name else ("ram" if "buffered" in name else "warm")
        print(table(sec, mode))
