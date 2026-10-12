#!/usr/bin/env bash
# MAGE oracle of the Sprint 5 laboratory (rmp #64, #69): runs one Cypher
# query against Memgraph MAGE on an oracle graph and prints its rows as
# JSON Lines.
#
# Usage:
#   mage_oracle.sh DIR QUERY
#
# DIR holds the `nodes.csv` and `edges.csv` that `src/lab/generator.rs`
# writes (Neo4j import layout). The script starts
# `memgraph/memgraph-mage:3.8.0` (the first image published after MAGE
# commit `c72636014a8b`, 2026-01-23, the inventory's commit; Apache-2.0
# MAGE modules, BSL Memgraph core: run, never copied), creates one node per
# row of `nodes.csv` with its labels and its `uid`, `lat` and `lng`
# properties, and one edge per row of `edges.csv` with its type and its
# `weight`, then runs QUERY and prints each row as one JSON object. A node
# is printed as its `uid`, an edge as `[source uid, target uid]`, a path as
# the list of its node `uid`s. The container is removed.
# Exit status: that of the query (non-zero on a Memgraph error).
set -euo pipefail

IMAGE="memgraph/memgraph-mage:3.8.0"

if [[ $# -ne 2 ]]; then
    sed -n '2,19p' "$0"
    exit 2
fi

dir="$(realpath "$1")"
query="$2"
name="lpg-mage-oracle-$$"

cleanup() { docker rm -f "$name" > /dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$name" --volume "$dir:/import:ro" "$IMAGE" > /dev/null

docker exec -i -e "QUERY=$query" "$name" python3 - <<'PY'
import csv
import json
import os
import sys
import time

from gqlalchemy import Memgraph
from gqlalchemy.models import Node, Path, Relationship

for _ in range(120):
    try:
        db = Memgraph()
        db.execute("RETURN 1")
        break
    except Exception:
        time.sleep(0.5)
else:
    sys.exit("Memgraph did not start")

db.execute("CREATE INDEX ON :__N(uid)")
with open("/import/nodes.csv", newline="") as f:
    rows = csv.reader(f)
    heads = next(rows)
    for row in rows:
        cells = dict(zip(heads, row))
        labels = [l for l in cells.get(":LABEL", "").split(";") if l] + ["__N"]
        props = {"uid": int(cells["uid:long"])}
        for k in ("lat", "lng"):
            if cells.get(f"{k}:double"):
                props[k] = float(cells[f"{k}:double"])
        db.execute(
            "CREATE (n" + "".join(f":`{l}`" for l in labels) + " $p)", {"p": props}
        )
with open("/import/edges.csv", newline="") as f:
    rows = csv.reader(f)
    heads = next(rows)
    for row in rows:
        cells = dict(zip(heads, row))
        props = {}
        if cells.get("weight:double"):
            props["weight"] = float(cells["weight:double"])
        db.execute(
            "MATCH (a:__N {uid: $s}), (b:__N {uid: $d}) CREATE (a)-[:`"
            + cells[":TYPE"]
            + "` $p]->(b)",
            {"s": int(cells[":START_ID"]), "d": int(cells[":END_ID"]), "p": props},
        )
db.execute("DROP INDEX ON :__N(uid)")


def plain(v):
    # gqlalchemy converts a column's own value; values nested in lists
    # stay mgclient objects (`properties`, `start_id`, `nodes`).
    if isinstance(v, Node):
        return v._properties.get("uid")
    if isinstance(v, Relationship):
        return [v._start_node_id, v._end_node_id]
    if isinstance(v, Path):
        return [plain(n) for n in v._nodes]
    if hasattr(v, "start_id") and hasattr(v, "end_id"):
        return ["edge", v.start_id, v.end_id]
    if hasattr(v, "nodes") and hasattr(v, "relationships"):
        return [plain(n) for n in v.nodes]
    if hasattr(v, "properties") and hasattr(v, "labels"):
        return v.properties.get("uid")
    if isinstance(v, (list, tuple)):
        return [plain(x) for x in v]
    if isinstance(v, dict):
        return {k: plain(x) for k, x in v.items()}
    return v


# Edge endpoints are internal ids: map them to uids.
uid = {r["i"]: r["u"] for r in db.execute_and_fetch("MATCH (n:__N) RETURN id(n) AS i, n.uid AS u")}
for row in db.execute_and_fetch(os.environ["QUERY"]):
    out = {}
    for k, v in row.items():
        p = plain(v)
        if isinstance(v, Relationship):
            p = [uid[p[0]], uid[p[1]]]
        elif isinstance(p, list):
            p = [
                [uid[x[1]], uid[x[2]]] if isinstance(x, list) and x[:1] == ["edge"] else x
                for x in p
            ]
        out[k] = p
    print(json.dumps(out))
PY
