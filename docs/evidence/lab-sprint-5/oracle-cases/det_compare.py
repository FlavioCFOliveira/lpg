"""Compares the repetitions of the GDS determinism runs (rmp #70).

Usage: det_compare.py DIR. DIR holds `det.<case>.<i>.jsonl`, each one
JSON object `{"r": [...]}` (see `gds_cases_70.sh`). For each case prints
the number of repetitions, how many distinct results they gave, and, for
a list of `[uid, id]` pairs, how many distinct partitions (results equal
up to renaming of the ids).
"""
import collections
import json
import pathlib
import sys

runs = collections.defaultdict(list)
for f in sorted(pathlib.Path(sys.argv[1]).glob("det.*.jsonl")):
    case = f.name[len("det."):].rsplit(".", 2)[0]
    runs[case].append(f.read_text())
for case, texts in sorted(runs.items()):
    distinct = len(set(texts))
    parts = "-"
    try:
        rows = [json.loads(t)["r"] for t in texts]
        if all(len(p) == 2 and not isinstance(p[1], (list, float)) for r in rows for p in r):
            def partition(r):
                groups = collections.defaultdict(list)
                for u, c in r:
                    groups[c].append(u)
                return frozenset(tuple(sorted(g)) for g in groups.values())
            parts = str(len({partition(r) for r in rows}))
    except (ValueError, KeyError, TypeError):
        parts = "unparsed"
    print(f"{case}: {len(texts)} runs, {distinct} distinct results, {parts} distinct partitions")
