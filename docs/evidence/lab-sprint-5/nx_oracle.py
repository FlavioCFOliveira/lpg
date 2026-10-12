"""NetworkX oracle of the Sprint 5 laboratory (rmp #61).

Reads an oracle graph (the `nodes.csv` and `edges.csv` that
`src/lab/generator.rs` writes, Neo4j import layout), runs one NetworkX
function on it and prints its result as JSON Lines.

Usage (always with the laboratory interpreter, isolated):

    /tmp/lpg-lab-venv/bin/python -I nx_oracle.py DIR FUNCTION [PARAMS] [--undirected]
        [--simple] [--nodes LABEL] [--other LABEL] [--nograph]

* DIR holds `nodes.csv` (`id:ID,:LABEL,uid:long[,vec:double[]]`) and
  `edges.csv` (`:START_ID,:END_ID,:TYPE[,weight:double][,capacity:double]
  [,cost:double]`). Nodes are keyed by `uid`.
* FUNCTION is a function of the `networkx` namespace, dotted names
  included (`bipartite.sets`), called as `FUNCTION(G, **PARAMS)`; PARAMS
  is a JSON object (default `{}`).
* The graph is a `MultiDiGraph`, or a `MultiGraph` with `--undirected`, so
  parallel edges are kept; edge columns are edge attributes. `--simple`
  reads a `DiGraph` or `Graph` instead (for functions that refuse
  multigraphs; parallel edges collapse).
* `--nodes LABEL` keeps the nodes with label LABEL (and the edges between
  them); `--other LABEL` passes the subgraph of the nodes with label LABEL
  as a second graph, `FUNCTION(G, G2, **PARAMS)` (rmp #69).
* `--nograph` calls `FUNCTION(**PARAMS)` without a graph.
* Output: a dict result gives one line `{"node": k, "value": v}` per key in
  key order; a graph result one `{"value": [u, v]}` line per edge, sorted;
  any other iterable one `{"value": v}` per element; a scalar one
  `{"value": v}` line. Sets are printed sorted; an infinite or NaN float
  as the string `"inf"`, `"-inf"` or `"nan"`, which JSON cannot hold.

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


def read(path, directed, simple=False):
    if simple:
        g = nx.DiGraph() if directed else nx.Graph()
    else:
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


def plain(o):
    """JSON form of the values JSON does not hold: sets sorted, other
    iterables as lists."""
    if isinstance(o, (set, frozenset)):
        return sorted(o)
    return list(o)


def finite(o):
    """`o` with each non-finite float replaced by its name."""
    if isinstance(o, float) and o != o or o in (float("inf"), float("-inf")):
        return str(o)
    if isinstance(o, dict):
        return {k: finite(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)):
        return [finite(v) for v in o]
    if isinstance(o, (set, frozenset)):
        return finite(sorted(o))
    return o


def emit(result):
    out = sys.stdout
    if isinstance(result, nx.Graph):
        result = sorted([list(e) for e in result.edges()])
    if isinstance(result, dict):
        result = {k: finite(v) for k, v in result.items()}
        for k in sorted(result):
            out.write(json.dumps({"node": k, "value": result[k]}, default=plain) + "\n")
    elif isinstance(result, (str, bytes, int, float, bool)) or result is None:
        out.write(json.dumps({"value": result}) + "\n")
    else:
        for v in result:
            out.write(json.dumps({"value": finite(v)}, default=plain) + "\n")


def labelled(g, label):
    return g.subgraph([n for n, d in g.nodes(data=True) if label in d.get("labels", [])]).copy()


def main(argv):
    flags = {"--undirected", "--simple", "--nograph"}
    valued = {"--nodes": None, "--other": None}
    args = []
    rest = iter(argv)
    for a in rest:
        if a in valued:
            valued[a] = next(rest, None)
        elif a not in flags:
            args.append(a)
    if len(args) not in (2, 3):
        sys.stderr.write(__doc__)
        return 2
    path, name = args[0], args[1]
    params = json.loads(args[2]) if len(args) == 3 else {}
    fn = nx
    for part in name.split("."):
        fn = getattr(fn, part, None)
    if not callable(fn):
        sys.stderr.write(f"unknown NetworkX function: {name}\n")
        return 2
    if "--nograph" in argv:
        emit(fn(**params))
        return 0
    g = read(path, "--undirected" not in argv, "--simple" in argv)
    if valued["--other"] is not None:
        other = labelled(g, valued["--other"])
        if valued["--nodes"] is not None:
            g = labelled(g, valued["--nodes"])
        emit(fn(g, other, **params))
        return 0
    if valued["--nodes"] is not None:
        g = labelled(g, valued["--nodes"])
    emit(fn(g, **params))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
