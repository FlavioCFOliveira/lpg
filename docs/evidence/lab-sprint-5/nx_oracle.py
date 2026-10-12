"""NetworkX oracle of the Sprint 5 laboratory (rmp #61).

Reads an oracle graph (the `nodes.csv` and `edges.csv` that
`src/lab/generator.rs` writes, Neo4j import layout), runs one NetworkX
function on it and prints its result as JSON Lines.

Usage (always with the laboratory interpreter, isolated):

    /tmp/lpg-lab-venv/bin/python -I nx_oracle.py DIR FUNCTION [PARAMS] [--undirected]

* DIR holds `nodes.csv` (`id:ID,:LABEL,uid:long[,vec:double[]]`) and
  `edges.csv` (`:START_ID,:END_ID,:TYPE[,weight:double][,capacity:double]
  [,cost:double]`). Nodes are keyed by `uid`.
* FUNCTION is a function of the `networkx` namespace, called as
  `FUNCTION(G, **PARAMS)`; PARAMS is a JSON object (default `{}`).
* The graph is a `MultiDiGraph`, or a `MultiGraph` with `--undirected`, so
  parallel edges are kept; edge columns are edge attributes.
* Output: a dict result gives one line `{"node": k, "value": v}` per key in
  key order; any other iterable one `{"value": v}` per element; a scalar one
  `{"value": v}` line.

Exit status 2 for an unknown function or bad arguments.
"""

import csv
import json
import os
import sys

import networkx as nx


def column(header):
    name, _, kind = header.partition(":")
    return name, kind


def read(path, directed):
    g = nx.MultiDiGraph() if directed else nx.MultiGraph()
    ids = {}
    with open(os.path.join(path, "nodes.csv"), newline="") as f:
        rows = csv.reader(f)
        heads = [column(h) for h in next(rows)]
        for row in rows:
            attrs = {}
            key = None
            for (name, kind), cell in zip(heads, row):
                if kind == "ID":
                    key = cell
                elif kind == "LABEL":
                    attrs["labels"] = cell.split(";") if cell else []
                elif kind == "long":
                    attrs[name] = int(cell)
                elif kind == "double[]":
                    attrs[name] = [float(x) for x in cell.split(";")] if cell else []
                elif kind == "double":
                    attrs[name] = float(cell)
            ids[key] = attrs["uid"]
            g.add_node(attrs["uid"], **attrs)
    with open(os.path.join(path, "edges.csv"), newline="") as f:
        rows = csv.reader(f)
        heads = [column(h) for h in next(rows)]
        for row in rows:
            attrs = {}
            for (name, kind), cell in zip(heads, row):
                if kind == "START_ID":
                    src = ids[cell]
                elif kind == "END_ID":
                    dst = ids[cell]
                elif kind == "TYPE":
                    attrs["type"] = cell
                elif kind == "double" and cell != "":
                    attrs[name] = float(cell)
            g.add_edge(src, dst, **attrs)
    return g


def emit(result):
    out = sys.stdout
    if isinstance(result, dict):
        for k in sorted(result):
            out.write(json.dumps({"node": k, "value": result[k]}) + "\n")
    elif isinstance(result, (str, bytes, int, float, bool)) or result is None:
        out.write(json.dumps({"value": result}) + "\n")
    else:
        for v in result:
            out.write(json.dumps({"value": v}, default=list) + "\n")


def main(argv):
    args = [a for a in argv if a != "--undirected"]
    if len(args) not in (2, 3):
        sys.stderr.write(__doc__)
        return 2
    path, name = args[0], args[1]
    params = json.loads(args[2]) if len(args) == 3 else {}
    fn = getattr(nx, name, None)
    if not callable(fn):
        sys.stderr.write(f"unknown NetworkX function: {name}\n")
        return 2
    g = read(path, "--undirected" not in argv)
    emit(fn(g, **params))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
