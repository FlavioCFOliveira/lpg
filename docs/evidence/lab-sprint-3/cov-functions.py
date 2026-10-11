#!/usr/bin/env python3
"""Per-function coverage table from `cargo llvm-cov ... --json`.

Usage:
    cargo llvm-cov nextest -p lpg --json [--output-path cov.json] [-E FILTER]
    python3 -I docs/evidence/lab-sprint-3/cov-functions.py cov.json [PATH_SUBSTRING] > table.tsv

Reads the llvm-cov export JSON, demangles the function names with c++filt
(legacy and v0 Rust mangling), drops the hashes, merges generic
instantiations of one function, and prints one TSV row per function:
file, function, execution count, covered code regions / regions. Only
functions whose file contains PATH_SUBSTRING are listed (default `src/`).
Rows are sorted by file, then function.
"""

import json
import re
import subprocess
import sys


def main() -> None:
    path = sys.argv[1]
    want = sys.argv[2] if len(sys.argv) > 2 else "src/"
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    funcs = [fn for d in data["data"] for fn in d["functions"]]
    names = subprocess.run(
        ["c++filt"], input="\n".join(fn["name"] for fn in funcs),
        capture_output=True, text=True, check=True,
    ).stdout.splitlines()
    rows = {}
    for fn, name in zip(funcs, names):
        file = fn["filenames"][0]
        if want not in file:
            continue
        # Legacy mangling ends in a hash; v0 tags crate names with one.
        name = re.sub(r"::h[0-9a-f]{16}$", "", name)
        name = re.sub(r"\[[0-9a-f]+\]", "", name)
        # Code regions (kind 0) of the function's own file (file id 0).
        regions = [r for r in fn["regions"] if r[7] == 0 and r[5] == 0]
        key = (file, name)
        count, covered, total = rows.get(key, (0, set(), set()))
        for r in regions:
            span = tuple(r[0:4])
            total.add(span)
            if r[4] > 0:
                covered.add(span)
        rows[key] = (count + fn["count"], covered, total)
    print("file\tfunction\tcount\tregions_covered\tregions")
    for (file, name), (count, covered, total) in sorted(rows.items()):
        print(f"{file}\t{name}\t{count}\t{len(covered)}\t{len(total)}")


if __name__ == "__main__":
    main()
