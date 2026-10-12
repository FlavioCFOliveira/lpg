#!/usr/bin/env python3
"""Reference executions of rmp #71 pass C (the entries of tasks #67, #68).

Each function below re-executes, on a small trace graph, the operations of
the algorithm's reference, step by step, and emits the events in the format
of lpg's test trace (`src/algo/trace.rs`): "kind: text". The source of each
reference is cited in its docstring (file:line at the commit the sprint
studied: GDS @ 417d7ecf973d (read only, GPL: rules described, no code
copied), MAGE @ c72636014a8b, petgraph 0.8.3, rustworkx @ 25398ab47f66,
graphalgs @ 1f8a412ef4af, NetworKit @ 5f03e17cec09, NetworkX 3.7 in the
lab venv). The script writes `src/algo/testdata/expected/trace_c.json` and
the CSV fixture `fl8`; `src/algo/tests_trace_c.rs` runs every case on lpg
and compares.

Run: /tmp/lpg-lab-venv/bin/python -I docs/evidence/lab-sprint-5/trace_ref_c.py

Conventions are pass A's and B's (`trace_ref_b.py`, imported for its
formatting helpers, adjacency order and ChaCha8 port): node i is dense
index i; edge i of a graph's list has element identifier n + i; adjacency
rows are in adjacency-tree order (type token, neighbour, edge identifier);
under UNDIRECTED a node's edges are its outgoing row then its incoming
row; floats print as Rust's `{:?}`. A graph of n <= 64 nodes has n blocks
of one node.

Every reference is also checked here against an independent oracle where
one exists (NetworkX, a sequential formulation, a brute force); those
checks are `assert`s, so the script fails rather than write a wrong case.
"""

import heapq
import importlib.util
import itertools
import json
import math
import os
from fractions import Fraction

import networkx as nx

import sys

sys.dont_write_bytecode = True  # no __pycache__ next to the evidence
HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("trace_ref_b", os.path.join(HERE, "trace_ref_b.py"))
B = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(B)
rf, fmt, take, adjacency, rng = B.rf, B.fmt, B.take, B.adjacency, B.rng

OUT = os.path.join(HERE, "../../../src/algo/testdata/expected/trace_c.json")
T = os.path.join(HERE, "../../../src/algo/testdata")
NONE = -1


def opt(x):
    """Rust's `{:?}` of an `Option<usize>`."""
    return "None" if x is None else f"Some({x})"


def slots(g):
    """Edge identifier -> forward position (`Projection::out` yields it)."""
    fwd, _ = adjacency(g)
    out, i = {}, 0
    for row in fwd:
        for _, e in row:
            out[e] = i
            i += 1
    return out


def weight(g, eid):
    return g["edges"][eid - g["n"]][3]


def out_rows(g, orientation="NATURAL"):
    """`p.out(v)` of every node: (other end, forward position, edge id)."""
    out, _ = adjacency(g, orientation)
    sl = slots(g)
    return [[(w, sl[e], e) for w, e in row] for row in out]


def inc_rows(g, orientation="NATURAL"):
    _, inc = adjacency(g, orientation)
    sl = slots(g)
    return [[(w, sl[e], e) for w, e in row] for row in inc]


def simple(g, orientation="UNDIRECTED", both=False):
    """lpg `kernels::Simple`: distinct other ends, sorted, no self."""
    if both:
        fwd, rev = adjacency(g)
        rows = [fwd[v] + rev[v] for v in range(g["n"])]
    else:
        rows, _ = adjacency(g, orientation)
    return [sorted({w for w, _ in r if w != v}) for v, r in enumerate(rows)]


# ------------------------------------------------------------- graphs

GRAPHS = {
    # Directed: SCCs {0,1,2} and {3,4}, a self-loop (7), an edge without
    # weight (8, NaN: heavier than any threshold), a parallel pair (9, 10),
    # node 8 isolated. Weights for 083's threshold.
    "cc": dict(n=9, edges=[
        (0, 1, "R", 2.0), (1, 2, "R", 2.0), (2, 0, "R", 2.0), (2, 3, "R", 0.5),
        (3, 4, "R", 2.0), (4, 3, "R", 2.0), (4, 5, "R", 2.0), (5, 5, "R", 1.0),
        (6, 5, "R", None), (7, 6, "R", 2.0), (7, 6, "R", 2.0)]),
    # Undirected entries: K4 {0..3}, triangle {3,4,5}, bridge 5-6, a
    # parallel pair 6-7 (no bridge), triangle {7,8,9}, a self-loop on 8,
    # node 10 isolated, a second component 11-12. Distinct weights.
    "ug": dict(n=13, edges=[
        (0, 1, "R", 3.0), (0, 2, "R", 1.5), (0, 3, "R", 4.0), (1, 2, "R", 2.5),
        (1, 3, "R", 0.5), (2, 3, "R", 5.0), (3, 4, "R", 1.0), (4, 5, "R", 6.0),
        (5, 3, "R", 2.0), (5, 6, "R", 7.0), (6, 7, "R", 1.25), (7, 6, "R", 3.5),
        (7, 8, "R", 2.25), (8, 9, "R", 0.75), (9, 7, "R", 5.5), (8, 8, "R", 1.0),
        (11, 12, "R", 4.5)]),
    # Bipartite: left {0..3}, right {4..6}; Kuhn's search must re-route.
    "bp": dict(n=7, edges=[
        (0, 4, "R", None), (0, 5, "R", None), (1, 4, "R", None), (2, 5, "R", None),
        (2, 6, "R", None), (3, 6, "R", None), (3, 4, "R", None)]),
    # Weighted, undirected use, distinct weights: Prim and the k-tree.
    "wt": dict(n=8, edges=[
        (0, 1, "R", 4.0), (0, 2, "R", 1.0), (1, 2, "R", 2.5), (1, 3, "R", 6.0),
        (2, 4, "R", 7.0), (3, 4, "R", 3.0), (3, 5, "R", 0.5), (4, 6, "R", 2.0),
        (5, 6, "R", 9.0), (6, 7, "R", 1.75), (2, 7, "R", 8.0)]),
    # GDS probe graphs (`oracle-cases/gds_probes_71c.sh`, measured on GDS
    # 2026.09.0): the new root re-queued by GDS's growth (kq), a child of a
    # trimmed node (kd).
    "kq": dict(n=6, edges=[(0, 1, "R", 39.0), (0, 3, "R", 29.0), (1, 2, "R", 11.0), (2, 4, "R", 30.0), (2, 5, "R", 13.0)]),
    "kd": dict(n=6, edges=[(0, 1, "R", 26.0), (0, 2, "R", 34.0), (1, 3, "R", 47.0), (2, 3, "R", 55.0), (2, 4, "R", 25.0), (4, 5, "R", 10.0)]),
    # Steiner, directed, distinct weights; rerouting replaces 4's edge.
    "st": dict(n=9, edges=[
        (0, 1, "R", 1.0), (1, 2, "R", 1.5), (2, 3, "R", 1.25), (0, 4, "R", 4.5),
        (3, 4, "R", 0.75), (1, 5, "R", 2.75), (5, 6, "R", 0.5), (6, 4, "R", 0.25),
        (4, 7, "R", 3.25), (2, 7, "R", 6.0), (6, 8, "R", 2.0), (8, 7, "R", 0.125),
        (7, 3, "R", 0.375)]),
    # Prize-collecting Steiner tree: prizes as node properties.
    "pc": dict(n=7, edges=[
        (0, 1, "R", 3.0), (1, 2, "R", 2.0), (0, 2, "R", 6.5), (2, 3, "R", 4.0),
        (3, 4, "R", 1.0), (4, 5, "R", 8.0), (5, 6, "R", 0.5), (1, 6, "R", 11.0)],
        nodes=[":N {prize: 5e0,", ":N {prize: 1e0,", ":N {prize: 2.5e0,", ":N {prize: 0e0,",
               ":N {prize: 6e0,", ":N {prize: 4e0,", ":N {prize: 3e0,"]),
    # GDS probe: GDS 2026.09.0 returns no node (measured), the best
    # subtree is node 1 alone.
    "pp": dict(n=3, edges=[(1, 0, "R", 10.0), (2, 0, "R", 4.0), (2, 1, "R", 8.0)],
               nodes=[":N {prize: 1e0,", ":N {prize: 1e1,", ":N {prize: 6e0,"]),
    # 083 with seeds (cc's edges): seeds 4 on node 1, 9 on node 4, 3 on 7.
    "cs": None,
    # Flows: CSV fixture fl8 (capacity and cost properties).
    "fl": dict(n=8, edges=[], fixture="fl8"),
    "nil": dict(n=0, edges=[]),
    "one": dict(n=1, edges=[(0, 0, "R", 1.0)]),
}
GRAPHS["cs"] = dict(GRAPHS["cc"], nodes=[":N", ":N {seed: 4,", ":N", ":N", ":N {seed: 9,", ":N", ":N", ":N {seed: 3,", ":N"])
SEEDS = {1: 4, 4: 9, 7: 3}

# fl8: (source, target, capacity, cost); the self-loop gets capacity 0.
FL8 = [(0, 1, 4.0, 1.0), (0, 2, 3.0, 2.0), (1, 2, 2.0, 1.0), (1, 3, 3.0, 3.0), (2, 4, 4.0, 1.0),
       (3, 5, 2.0, 1.0), (4, 3, 1.0, 1.0), (4, 6, 3.0, 2.0), (3, 7, 2.0, 4.0), (5, 7, 3.0, 1.0),
       (6, 7, 4.0, 1.0), (6, 7, 1.0, 5.0), (2, 2, 5.0, 1.0), (1, 0, 1.0, 1.0), (5, 4, 0.0, 1.0)]


def write_fl8():
    d = os.path.join(T, "fl8")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "nodes.csv"), "w") as f:
        f.write("id:ID,:LABEL,uid:long\n")
        for i in range(8):
            f.write(f"{i},N,{i}\n")
    with open(os.path.join(d, "edges.csv"), "w") as f:
        f.write(":START_ID,:END_ID,:TYPE,capacity:double,cost:double\n")
        for a, b, c, k in FL8:
            f.write(f"{a},{b},R,{c!r},{k!r}\n")


GRAPHS["fl"]["edges"] = [(a, b, "R", None) for a, b, _, _ in FL8]


# ------------------------------------------------- 083 weakly connected

def wcc_ref(g, threshold=None):
    """083: synchronous minimum-label rounds with pointer jumping (Kùzu
    `weakly_connected_components.cpp`, NetworKit
    `ParallelConnectedComponents.cpp`; FastSV's jump, LAGraph
    `LG_CC_FastSV6.c`), as lpg documents them (`components.rs:8-31`):
    a node takes the smallest label of itself and of its neighbours over
    both directions of every edge (with a threshold, only edges heavier
    than it or without weight, GDS `Wcc.java` `defaultWeight`), then the
    label of that label. Sequential formulation: these loops."""
    n = g["n"]
    out, inc = adjacency(g)

    def keep(e):
        w = weight(g, e)
        return threshold is None or w is None or w > threshold

    labels, ev, r = list(range(n)), [], 0
    while True:
        r += 1
        nxt = [min([labels[v]] + [labels[w] for w, e in out[v] + inc[v] if keep(e)]) for v in range(n)]
        jumped = [nxt[nxt[v]] for v in range(n)]
        ev.append(f"round: {r} min {fmt(nxt)} jump {fmt(jumped)}")
        done = nxt == labels and jumped == nxt
        labels = jumped
        if done:
            break
    # GDS's union-find gives the same partition (`wcc/Wcc.java`).
    parent = list(range(n))

    def find(x):
        while parent[x] != x:
            x = parent[x]
        return x
    for v in range(n):
        for w, e in out[v]:
            if keep(e):
                parent[find(v)] = find(w)
    part = lambda lab: sorted(sorted(v for v in range(n) if lab[v] == c) for c in set(lab))
    assert part(labels) == part([find(v) for v in range(n)])
    return ev, labels


def wcc_ids(labels, seeds=None):
    """lpg's identifiers (`components.rs` `wcc`): the smallest node, or
    with seeds the smallest seed of the component, else the next
    identifier above the largest seed, components in root order."""
    if not seeds:
        return labels
    mx = max(seeds.values())
    ids, fresh = {}, mx
    for root in sorted(set(labels)):
        s = [seeds[v] for v in range(len(labels)) if labels[v] == root and v in seeds]
        if s:
            ids[root] = min(s)
        else:
            fresh += 1
            ids[root] = fresh
    return [ids[l] for l in labels]


# --------------------------------------------- 084 connected (UnionFind)

def connected_ref(g):
    """084: petgraph 0.8.3 `unionfind.rs` (`union`: union by rank,
    `find_mut_recursive`: path halving; `into_labeling`), over every edge
    of the UNDIRECTED projection in `p.out` order."""
    n = g["n"]
    out, _ = adjacency(g, "UNDIRECTED")
    parent, rank, ev = list(range(n)), [0] * n, []

    def find_mut(x):
        p = parent[x]
        while p != x:
            gp = parent[p]
            parent[x] = gp
            x, p = p, gp
        return x

    def union(x, y):
        if x == y:
            return False
        xr, yr = find_mut(x), find_mut(y)
        if xr == yr:
            return False
        if rank[xr] < rank[yr]:
            parent[xr] = yr
        elif rank[xr] > rank[yr]:
            parent[yr] = xr
        else:
            parent[yr] = xr
            rank[xr] += 1
        return True
    for v in range(n):
        for w, _ in out[v]:
            ev.append(f"union: {v} {w} {fmt(union(v, w))}")
    for ix in range(n):
        parent[ix] = find_mut(parent[ix])
    ev.append(f"label: {fmt(parent)}")
    smallest = {}
    for v, c in enumerate(parent):
        smallest.setdefault(c, v)
    ids = [smallest[c] for c in parent]
    G = nx.Graph()
    G.add_nodes_from(range(n))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"])
    assert sorted(map(sorted, nx.connected_components(G))) == sorted(sorted(v for v in range(n) if ids[v] == c) for c in set(ids))
    return ev, ids


# --------------------------------------------------- 085, 086 Tarjan SCC

def tarjan_ref(g):
    """085, 086: petgraph 0.8.3 `scc/tarjan_scc.rs` `TarjanScc::visit`
    (Pearce's variant: root indexes, the stack filled when backtracking),
    over `p.out` targets in adjacency order; components in emission order,
    each in stack order."""
    n = g["n"]
    out, _ = adjacency(g)
    root = [None] * n
    st = {"index": 1, "count": float("inf")}
    stack, comps = [], []

    def visit(v):
        local = True
        root[v] = st["index"]
        st["index"] += 1
        for w, _ in out[v]:
            if root[w] is None:
                visit(w)
            if root[w] < root[v]:
                root[v] = root[w]
                local = False
        if local:
            adj = 1
            start = 0
            for i in range(len(stack) - 1, -1, -1):
                if root[v] > root[stack[i]]:
                    start = i + 1
                    break
                root[stack[i]] = st["count"]
                adj += 1
            root[v] = st["count"]
            stack.append(v)
            comps.append(list(stack[start:]))
            del stack[start:]
            st["index"] -= adj
            st["count"] -= 1
        else:
            stack.append(v)
    for v in range(n):
        if root[v] is None:
            visit(v)
    G = nx.MultiDiGraph()
    G.add_nodes_from(range(n))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"])
    assert sorted(map(sorted, comps)) == sorted(map(sorted, nx.strongly_connected_components(G)))
    return comps


def scc_cases(g):
    comps = tarjan_ref(g)
    ev = [f"scc: {fmt(c)}" for c in comps]
    srt = sorted((sorted(c) for c in comps), key=lambda c: c[0])
    of = {v: i for i, c in enumerate(srt) for v in c}
    out, _ = adjacency(g)
    succ = []
    for i, c in enumerate(srt):
        s = sorted({of[w] for v in c for w, _ in out[v] if of[w] != i})
        succ.append(s)
        ev.append(f"succ: {i} {fmt(s)}")
    rows = [[{"node": v}, srt[of[v]][0]] for v in range(g["n"])]
    crows = [[i, [{"node": v} for v in c], s] for i, (c, s) in enumerate(zip(srt, succ))]
    G = nx.DiGraph()
    G.add_nodes_from(range(g["n"]))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"])
    C = nx.condensation(G)
    assert sorted(len(C.nodes[x]["members"]) for x in C) == sorted(len(c) for c in srt)
    assert C.number_of_edges() == sum(len(s) for s in succ)
    return ev, rows, crows


# ------------------------------------------ 089 biconnected components

def bcc_ref(g):
    """089: Hopcroft and Tarjan's edge-stack depth-first search on the
    simple undirected graph (NetworkX `components/biconnected.py`
    `_biconnected_dfs`; rustworkx `connectivity/biconnected.rs`), written
    recursively; neighbours ascending."""
    n = g["n"]
    s = simple(g)
    disc, low, parent = [None] * n, [0] * n, [None] * n
    edges, comps, ev = [], [], []
    t = [0]

    def dfs(v):
        for w in s[v]:
            if w == parent[v]:
                continue
            if disc[w] is None:
                parent[w] = v
                disc[w] = low[w] = t[0]
                t[0] += 1
                ev.append(f"tree: {v} {w}")
                edges.append((v, w))
                dfs(w)
                low[v] = min(low[v], low[w])
                if low[w] >= disc[v]:
                    comp = []
                    while True:
                        e = edges.pop()
                        comp.append(e)
                        if e == (v, w):
                            break
                    comp.reverse()
                    ev.append(f"comp: {fmt(comp)}")
                    comps.append(comp)
            elif disc[w] < disc[v]:
                low[v] = min(low[v], disc[w])
                ev.append(f"back: {v} {w}")
                edges.append((v, w))
    for r in range(n):
        if disc[r] is None:
            disc[r] = low[r] = t[0]
            t[0] += 1
            dfs(r)
    G = nx.Graph()
    G.add_edges_from((a, b) for a, b, *_ in g["edges"] if a != b)
    want = sorted(sorted(tuple(sorted(e)) for e in c) for c in nx.biconnected_component_edges(G))
    assert sorted(sorted(tuple(sorted(e)) for e in c) for c in comps) == want
    rows = [[i, {"node": a}, {"node": b}] for i, c in enumerate(comps) for a, b in c]
    return ev, rows


# ------------------------------------ 090, 091 low-link (GDS parent rule)

def lowlink_ref(g):
    """090, 091: Tarjan's low-link depth-first search (Tarjan 1974;
    Hopcroft and Tarjan 1973) over the UNDIRECTED projection, neighbours
    ascending with their multiplicity, with GDS's parent rule
    (`bridges/Bridges.java` `visitEvent`, `articulationpoints/
    ArticulationPoints.java`: only the first edge back to the parent is
    skipped, so a parallel edge to the parent is a back edge)."""
    n = g["n"]
    out, _ = adjacency(g, "UNDIRECTED")
    adj = [sorted(w for w, _ in out[v]) for v in range(n)]
    tin, low, par, size, root = [None] * n, [None] * n, [None] * n, [1] * n, [None] * n
    ev, t = [], [0]

    def dfs(v, start):
        skipped = False
        for w in adj[v]:
            if w == par[v] and not skipped:
                skipped = True
                ev.append(f"skip: {v} {w}")
                continue
            if tin[w] is None:
                par[w], tin[w], low[w], root[w] = v, t[0], t[0], start
                t[0] += 1
                ev.append(f"tree: {v} {w}")
                dfs(w, start)
            else:
                low[v] = min(low[v], tin[w])
                ev.append(f"back: {v} {w}")
        ev.append(f"done: {v} low {low[v]} size {size[v]}")
        if par[v] is not None:
            low[par[v]] = min(low[par[v]], low[v])
            size[par[v]] += size[v]
    for s in range(n):
        if tin[s] is None:
            tin[s], low[s], root[s] = t[0], t[0], s
            t[0] += 1
            dfs(s, s)
    # Articulation points and their pieces; bridges with the sides' sizes.
    pieces = [[] for _ in range(n)]
    for c in range(n):
        v = par[c]
        if v is not None and (root[v] == v or low[c] >= tin[v]):
            pieces[v].append(size[c])
    ap_rows = []
    for v in range(n):
        sz = pieces[v]
        isroot = root[v] == v
        if not (len(sz) > 1 if isroot else sz):
            continue
        if not isroot:
            rest = size[root[v]] - 1 - sum(sz)
            if rest > 0:
                sz = sz + [rest]
        ap_rows.append([{"node": v}, {"count": len(sz), "max": max(sz), "min": min(sz)}])
    br_rows = []
    for c in range(n):
        v = par[c]
        if v is None or low[c] <= tin[v]:
            continue
        tot = size[root[c]]
        br_rows.append([{"node": v}, {"node": c}, [tot - size[c], size[c]]])
    G = nx.MultiGraph()
    G.add_nodes_from(range(n))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"] if a != b)
    assert {r[0]["node"] for r in ap_rows} == set(nx.articulation_points(nx.Graph(G)))
    # A bridge of the multigraph: an edge whose removal disconnects.
    bridges = set()
    for k, (a, b, *_) in enumerate(g["edges"]):
        if a == b:
            continue
        H = nx.MultiGraph()
        H.add_nodes_from(range(n))
        H.add_edges_from((x, y) for i, (x, y, *_) in enumerate(g["edges"]) if i != k and x != y)
        if not nx.has_path(H, a, b):
            bridges.add(tuple(sorted((a, b))))
    assert {tuple(sorted((r[0]["node"], r[1]["node"]))) for r in br_rows} == bridges
    return ev, ap_rows, br_rows


# ------------------------------------------------------- 093 k-core

def kcore_ref(g):
    """093: synchronous peeling rounds (`components.rs:63-75`): a round
    removes every node whose remaining degree is at most the level k and
    lowers its remaining neighbours' degrees; with no such node the level
    rises to the smallest remaining degree. Degrees of the UNDIRECTED
    projection (parallel edges, a self-loop twice), as GDS's
    (`kcore/KCoreDecomposition.java`). Sequential formulation:
    Batagelj and Zaversnik's bucket peeling (asserted equal below)."""
    n = g["n"]
    out, _ = adjacency(g, "UNDIRECTED")
    deg = [len(out[v]) for v in range(n)]
    core, k, ev, rounds, levels = [None] * n, 0, [], 0, {}
    left = n
    while left:
        removed = [v for v in range(n) if core[v] is None and deg[v] <= k]
        if not removed:
            k = min(deg[v] for v in range(n) if core[v] is None)
            ev.append(f"level: {k}")
            continue
        rounds += 1
        levels[k] = levels.get(k, 0) + 1
        ev.append(f"peel: {k} {fmt(removed)}")
        for v in removed:
            core[v] = k
        left -= len(removed)
        for v in removed:
            for w, _ in out[v]:
                if core[w] is None:
                    deg[w] -= 1
    # Batagelj-Zaversnik: repeatedly remove a node of least degree.
    d = [len(out[v]) for v in range(n)]
    gone, bz, kk = [False] * n, [0] * n, 0
    for _ in range(n):
        v = min((x for x in range(n) if not gone[x]), key=lambda x: d[x])
        kk = max(kk, d[v])
        bz[v], gone[v] = kk, True
        for w, _ in out[v]:
            if not gone[w]:
                d[w] -= 1
    assert bz == core
    return ev, core, rounds, levels


def kcore_analysis():
    """093 rounds against GDS's phases on the lab fixtures (rmp #67
    input): GDS's `compute` loop runs one SCAN per iteration and one ACT
    phase per level at which the SCAN finds a node of exactly that degree;
    an ACT phase removes the level's whole cascade through its stack
    (`KCoreDecompositionTask.java` `act`, `relax`). So GDS's phases are the
    distinct core values of the nodes with an edge; lpg's synchronous
    rounds are, per level, the depth of that level's cascade."""
    out = {}
    for name in ("karate", "zipf60", "uniform60", "zipf300"):
        n, edges = B.fixture(name)
        rank = {v: i for i, v in enumerate(sorted(range(n), key=str))}
        g = dict(n=n, edges=[(rank[a], rank[b], "R", None) for a, b in edges])
        _, core, rounds, levels = kcore_ref(g)
        gds = [json.loads(l) for l in open(os.path.join(T, "expected", f"{name}.gds.kcore.jsonl"))]
        gcore = {r["nodeId"]: r["coreValue"] for r in gds}
        mine = {int(u): core[rank[int(u)]] for u in range(n)}
        assert all(gcore[u] == mine[u] for u in gcore), name
        undirected_deg = [0] * n
        for a, b in edges:
            undirected_deg[a] += 1
            undirected_deg[b] += 1
        levels_gds = sorted({gcore[u] for u in gcore if undirected_deg[u] > 0})
        # GDS's loop (`compute`): scanning degree 1; each iteration one
        # SCAN; an ACT when a node of exactly that degree remains, then the
        # next degree; otherwise a jump to the smallest remaining degree.
        scans, scanning = 0, 1
        for lv in levels_gds:
            if lv > scanning:
                scans += 1  # the SCAN that finds no node and jumps
                scanning = lv
            scans += 1  # SCAN + ACT at this level
            scanning = lv + 1
        out[name] = dict(lpg_rounds=rounds, rounds_per_level={str(k): v for k, v in sorted(levels.items())},
                         gds_act_phases=len(levels_gds), gds_loop_iterations=scans)
    return out


# ------------------------------------------------ 120, 121 triangles

def triangles_ref(g, max_degree=None):
    """120, 121: per node, the intersections of its sorted row with each
    neighbour's (graphina `parallel/triangles.rs`, LAGraph
    `LAGr_TriangleCount.c`; GDS `triangle/IntersectingTriangleCount.java`
    intersects sorted adjacency too); nodes whose projected degree exceeds
    `maxDegree` get -1 and count in no triangle (GDS, measured rmp #67)."""
    n = g["n"]
    s = simple(g)
    out, _ = adjacency(g, "UNDIRECTED")
    ex = [max_degree is not None and len(out[v]) > max_degree for v in range(n)]
    ev, t = [], []
    for v in range(n):
        if ex[v]:
            t.append(-1)
            continue
        each = []
        for w in s[v]:
            if ex[w]:
                continue
            each.append((w, len([x for x in set(s[v]) & set(s[w]) if not ex[x]])))
        t.append(sum(c for _, c in each) // 2)
        ev.append(f"tri: {v} {fmt(each)} {t[v]}")
    lst = [(a, b, c) for a in range(n) if not ex[a] for i, b in enumerate(s[a]) if b > a and not ex[b]
           for c in s[a][i + 1:] if not ex[c] and c in s[b]]
    if max_degree is None:
        G = nx.Graph()
        G.add_nodes_from(range(n))
        G.add_edges_from((a, b) for a, b, *_ in g["edges"] if a != b)
        assert t == [nx.triangles(G, v) for v in range(n)]
    return ev, t, lst


def lcc_ref(g, t):
    """121: `2t / (d (d - 1))`, d the distinct neighbours plus the node
    itself with a self-loop (GDS `LocalClusteringCoefficient.java`
    `degreeWithoutParallelRelationships`, measured)."""
    n = g["n"]
    s = simple(g)
    out, _ = adjacency(g, "UNDIRECTED")
    ev, vals = [], []
    for v in range(n):
        tv = float(t[v])
        d = float(len(s[v]) + (1 if any(w == v for w, _ in out[v]) else 0))
        x = math.nan if tv == -1.0 else (0.0 if tv == 0.0 else tv * 2.0 / (d * (d - 1.0)))
        vals.append(x)
        ev.append(f"lcc: {v} {rf(tv)} {rf(d)} {rf(x)}")
    return ev, vals


# ---------------------------------------------------- 123 eccentricity

def ecc_ref(g, weighted):
    """123: graphalgs `metrics.rs` `eccentricity` (breadth-first distances
    per node) and `weighted_eccentricity` (petgraph `bellman_ford`
    distances), NATURAL; the eccentricity is the largest distance."""
    n = g["n"]
    out = out_rows(g)
    ev, ecc = [], []
    for s in range(n):
        dist = [math.inf] * n
        dist[s] = 0.0
        if weighted:
            for _ in range(n):
                for u in range(n):
                    for w, _, e in out[u]:
                        if dist[u] + weight(g, e) < dist[w]:
                            dist[w] = dist[u] + weight(g, e)
        else:
            q = [s]
            for u in q:
                for w, _, _ in out[u]:
                    if dist[w] == math.inf:
                        dist[w] = dist[u] + 1.0
                        q.append(w)
        ev.append(f"dist: {s} {fmt(dist)}")
        ecc.append(max([0.0] + dist))
    G = nx.DiGraph()
    G.add_nodes_from(range(n))
    for a, b, _, w in g["edges"]:
        if not G.has_edge(a, b) or (w or 1.0) < G[a][b]["weight"]:
            G.add_edge(a, b, weight=w if weighted else 1.0)
    for s in range(n):
        d = nx.single_source_dijkstra_path_length(G, s) if weighted else nx.single_source_shortest_path_length(G, s)
        assert ecc[s] == (max(d.values()) if len(d) == n else math.inf)
    return ev, ecc


# ------------------------------------------------------------ 128 girth

def girth_ref(g, orientation):
    """128: NetworkX `cycles.py` `girth` (a breadth-first search from every
    node; a non-tree edge u-v closes a cycle of depth(u) + depth(v) + 1
    edges), as lpg runs it (`measures.rs:35-45`): tree edges and their
    parallel copies close nothing, a self-loop closes a cycle of one edge;
    directed, an edge u -> s closes depth(u) + 1. The BFS from s stops at
    the first node whose edges cannot beat the best so far; the search
    stops at 1."""
    n = g["n"]
    out, _ = adjacency(g, orientation)
    und = orientation == "UNDIRECTED"
    best, ev = math.inf, []
    for s in range(n):
        depth, tree = {s: 0}, {}
        q = [s]
        for v in q:
            floor = 2 * depth[v] if und else depth[v] + 1
            if floor >= best:
                break
            for w, _ in out[v]:
                if und:
                    if w != v and (tree.get(v) == w or tree.get(w) == v):
                        continue
                    if w not in depth:
                        depth[w], tree[w] = depth[v] + 1, v
                        q.append(w)
                    else:
                        best = min(best, depth[v] + depth[w] + 1)
                elif w == s:
                    best = min(best, depth[v] + 1)
                elif w not in depth:
                    depth[w] = depth[v] + 1
                    q.append(w)
        ev.append(f"girth: {s} {0 if best == math.inf else best}")
        if best == 1:
            break
    if und:
        G = nx.Graph()
        G.add_edges_from((a, b) for a, b, *_ in g["edges"])
        assert nx.girth(G) == best
    else:
        G = nx.DiGraph()
        G.add_edges_from((a, b) for a, b, *_ in g["edges"])
        assert min((len(c) for c in nx.simple_cycles(G)), default=math.inf) == best
    return ev, float(best)


# -------------------------------------------------------- 143 Laplacian

def laplacian_ref(g, weighted, orientation="NATURAL"):
    """143: graphalgs `spec/laplacian.rs` `laplacian_matrix`: each edge
    u -> v, u != v, adds its weight to L[u][u] and subtracts it from
    L[u][v], in `p.out` order."""
    n = g["n"]
    out = out_rows(g, orientation)
    L = [[0.0] * n for _ in range(n)]
    diag, ev = [0.0] * n, []
    for i in range(n):
        for j, _, e in out[i]:
            if i == j:
                continue
            c = weight(g, e) if weighted else 1.0
            ev.append(f"cell: {i} {j} {rf(c)}")
            L[i][j] -= c
            diag[i] += c
    for i in range(n):
        L[i][i] = diag[i]
    return ev, L


# ---------------------------------------------------- 147 Prim, 149

class Heap:
    """lpg `kernels::IndexedHeap`, the textbook 1-based array heap: an
    element moves only past a strictly larger cost (GDS
    `HugeLongPriorityQueue` `upHeap`, `downHeap` follow the same rule)."""

    def __init__(self, n):
        self.heap, self.at, self.cost = [0], [0] * n, [0.0] * n

    def contains(self, e):
        return self.at[e] > 0

    def _place(self, p, e):
        self.heap[p], self.at[e] = e, p

    def _up(self, i0):
        e, i = self.heap[i0], i0
        while i > 1 and self.cost[e] < self.cost[self.heap[i // 2]]:
            self._place(i, self.heap[i // 2])
            i //= 2
        self._place(i, e)
        return i != i0

    def _down(self, i):
        size, e = len(self.heap) - 1, self.heap[i]
        while True:
            j = 2 * i
            if j > size:
                break
            if j < size and self.cost[self.heap[j + 1]] < self.cost[self.heap[j]]:
                j += 1
            if not self.cost[self.heap[j]] < self.cost[e]:
                break
            self._place(i, self.heap[j])
            i = j
        self._place(i, e)

    def set(self, e, c):
        self.cost[e] = c
        if self.contains(e):
            if not self._up(self.at[e]):
                self._down(self.at[e])
        else:
            self.heap.append(e)
            self.at[e] = len(self.heap) - 1
            self._up(len(self.heap) - 1)

    def pop(self):
        if len(self.heap) == 1:
            return None
        top, last = self.heap[1], self.heap.pop()
        self.at[top] = 0
        if len(self.heap) > 1:
            self._place(1, last)
            self._down(1)
        return top


def prim_ref(g, source, weighted=True, mx=False):
    """147: Prim's algorithm as GDS's `spanningtree/Prim.java` `compute`
    runs it (a min queue keyed by edge cost with decrease-key, a node's
    edges in adjacency order: by target, then edge; the maximum by negated
    costs), on the UNDIRECTED projection."""
    n = g["n"]
    out = out_rows(g, "UNDIRECTED")
    parent, cost, via, done, order = [NONE] * n, [0.0] * n, [NONE] * n, [False] * n, []
    h = Heap(n)
    via[source] = source
    h.set(source, 0.0)
    ev = []
    while True:
        u = h.pop()
        if u is None:
            break
        done[u], parent[u] = True, via[u]
        key = h.cost[u]
        cost[u] = 0.0 if u == source else (-key if mx else key)
        order.append(u)
        ev.append(f"pop: {u} {parent[u]} {rf(cost[u])}")
        for w, sl, e in sorted(out[u], key=lambda x: (x[0], x[1])):
            if done[w]:
                continue
            c = weight(g, e) if weighted else 1.0
            k = -c if mx else c
            if not h.contains(w) or k < h.cost[w]:
                h.set(w, k)
                via[w] = u
                ev.append(f"key: {w} {u} {rf(k)}")
    G = nx.Graph()
    G.add_nodes_from(range(n))
    for a, b, _, w in g["edges"]:
        if a != b:
            c = (w if weighted else 1.0) * (-1 if mx else 1)
            if not G.has_edge(a, b) or c < G[a][b]["weight"]:
                G.add_edge(a, b, weight=c)
    comp = nx.node_connected_component(G, source)
    want = nx.minimum_spanning_tree(G.subgraph(comp)).size(weight="weight")
    got = sum(cost[v] * (-1 if mx else 1) for v in order if v != source)
    assert abs(want - got) < 1e-12 and set(order) == comp
    return ev, parent, cost, order


def ktree_ref(g, source, k, mx=False):
    """149: GDS's `kspanningtree/KSpanningTree.java` `growApproach` as lpg
    documents it (`trees.rs:18-30`): nodes are taken from a min queue of
    their tree-edge weights (lpg's `IndexedHeap`, fed with a node's tree
    children by index), starting at the source; while the tree holds
    fewer than k nodes a node joins; then it joins only by replacing the
    worst leaf (largest key, then smallest index; the root is a leaf with
    its child's key once a removal has left it one child), never its own
    parent, and only if its key is smaller. Two documented departures
    from GDS's code, both decided in rmp #71 with GDS runs
    (`gds_probes_71c.sh`): a node whose tree parent left is dropped (GDS
    compares it with the worst leaf and rejects it in every case found,
    graph `kd`), and a new root with one child becomes a leaf candidate
    (GDS re-queues it for growth, which shrinks its tree below k, graph
    `kq`)."""
    n = g["n"]
    _, parent, cost, order = prim_ref(g, source, True, mx)
    ev = []
    inc = [False] * n
    if len(order) <= k:
        return ev, set(order), source
    ch = [[] for _ in range(n)]
    for v in order:
        if parent[v] != v:
            ch[parent[v]].append(v)
    for c in ch:
        c.sort()
    key = (lambda c: -c) if mx else (lambda c: c)
    kids, h, count, root, rl = [0] * n, Heap(n), 0, source, False
    h.set(source, 0.0)
    while True:
        v = h.pop()
        if v is None:
            break
        c, pa = h.cost[v], parent[v]
        if v != source and not inc[pa]:
            ev.append(f"drop: {v}")
            continue
        if count >= k:
            worst = None
            for u in range(n):
                if not inc[u]:
                    continue
                if u == root:
                    if not rl or kids[u] != 1:
                        continue
                    lc = key(cost[next(x for x in ch[u] if inc[x])])
                elif kids[u] == 0:
                    lc = key(cost[u])
                else:
                    continue
                if worst is None or lc > worst[0]:
                    worst = (lc, u)
            if worst is None:
                ev.append(f"reject: {v} no leaf")
                continue
            lc, leaf = worst
            if pa == leaf:
                ev.append(f"skip: {v} parent {leaf}")
                continue
            if c >= lc:
                ev.append(f"reject: {v} {rf(c)} worst {leaf} {rf(lc)}")
                continue
            inc[leaf] = False
            count -= 1
            ev.append(f"trim: {leaf} for {v}")
            if leaf == root:
                root = next(x for x in ch[leaf] if inc[x])
                rl = kids[root] == 1
                ev.append(f"root: {root} leaf {fmt(rl)}")
            else:
                pl = parent[leaf]
                kids[pl] -= 1
                if pl == root and kids[pl] == 1:
                    rl = True
        inc[v] = True
        count += 1
        ev.append(f"add: {v} {rf(c)}")
        if v != root:
            kids[pa] += 1
            if pa == root:
                rl = False
        for w in ch[v]:
            if not h.contains(w):
                h.set(w, key(cost[w]))
    tree = {v for v in range(n) if inc[v]}
    # A tree of k nodes: connected through MST edges.
    assert len(tree) == k
    seen, stack = {root}, [root]
    while stack:
        x = stack.pop()
        for y in tree:
            if y not in seen and (parent[y] == x or parent[x] == y):
                seen.add(y)
                stack.append(y)
    assert seen == tree
    return ev, tree, root


# ------------------------------------------------ 151 spanning trees

P1, P2 = 2305843009213693951, 4611686018427387847


def det_exact(a):
    """Exact determinant by fraction elimination (Python `fractions`)."""
    k = len(a)
    m = [[Fraction(x) for x in row] for row in a]
    det = Fraction(1)
    for c in range(k):
        piv = next((r for r in range(c, k) if m[r][c] != 0), None)
        if piv is None:
            return 0
        if piv != c:
            m[c], m[piv] = m[piv], m[c]
            det = -det
        det *= m[c][c]
        for r in range(c + 1, k):
            f = m[r][c] / m[c][c]
            if f:
                for j in range(c, k):
                    m[r][j] -= f * m[c][j]
    assert det.denominator == 1
    return int(det)


def count_ref(g, root, orientation):
    """151: the matrix-tree theorem (graphalgs `spec/st.rs:79`): the
    Laplacian of `p.out` (out-degree Laplacian when directed) without the
    root's row and column; its determinant counts the spanning trees
    (oriented to the root when directed). lpg's exact method
    (`trees.rs:31-46`): determinants modulo 2^61 - 1 and 2^62 - 57, the
    CRT residue modulo their product M, the Hadamard-Fischer bound (the
    diagonal's product, saturated at 2^128 - 1)."""
    n = g["n"]
    out = out_rows(g, orientation)
    k = n - 1
    pos = lambda v: v if v < root else v - 1
    a = [[0] * k for _ in range(k)]
    for v in range(n):
        if v == root:
            continue
        for w, _, _ in out[v]:
            if w == v:
                continue
            a[pos(v)][pos(v)] += 1
            if w != root:
                a[pos(v)][pos(w)] -= 1
    det = det_exact(a)
    r1, r2 = det % P1, det % P2
    residue = det % (P1 * P2)
    bound = 1
    for i in range(k):
        bound = min(bound * max(a[i][i], 0), (1 << 128) - 1)
    ev = [f"residue: {r1} {r2}", f"crt: {residue} bound {bound} {fmt(bound < P1 * P2)}"]
    return ev, det, a


def cycle_chain(cycles, length):
    """Cycles of `length` nodes joined in a chain at shared nodes: the
    count is length^cycles; the diagonal's product is about 2^nodes."""
    edges, start, nxt = [], 0, 1
    for _ in range(cycles):
        ring = [start] + list(range(nxt, nxt + length - 1))
        nxt += length - 1
        for i in range(length):
            edges.append((ring[i], ring[(i + 1) % length], "R", None))
        start = ring[length // 2]
    return dict(n=nxt, edges=edges)


# ------------------------------------------------------- 153 Steiner

def steiner_ref(g, source, targets, weighted=True, reroute=False):
    """153: GDS's rooted heuristic (`steiner/ShortestPathsSteinerAlgorithm
    .java` `compute`, `SteinerBasedDeltaStepping.java`: the nearest
    unreached target joins with its shortest path from the tree, whose
    nodes then have distance 0) with lpg's distances
    (`trees.rs:47-65`): synchronous Bellman-Ford rounds, each node taking
    the smallest (distance, parent, edge) of its own and of
    distance(u) + w over its incoming edges, reading the last round; the
    nearest target, the smaller index on a tie. Rerouting: GDS's
    `SimpleRerouter.java` `reroute` (each tree node s in index order offers
    each out-edge s -> t, by target then edge, to a non-root tree node t
    whose edge is heavier, when s is not below t; `cutNodesAfterRerouting`
    keeps the paths from targets to the source). Sequential formulation:
    Dijkstra from the tree gives the same distances and nearest target
    (asserted)."""
    n = g["n"]
    inc = inc_rows(g)
    outr = out_rows(g)
    INF = math.inf
    cost = lambda e: weight(g, e) if weighted else 1.0
    parent, pcost = [NONE] * n, [0.0] * n
    parent[source] = source
    state = [(INF, NONE, NONE)] * n
    state[source] = (0.0, NONE, NONE)
    un = sorted(set(t for t in targets if t != source))
    ev, rounds = [], 0
    while un:
        for _ in range(n + 1):
            rounds += 1
            nxt, changed = list(state), False
            for v in range(n):
                best = state[v]
                if parent[v] == NONE:
                    for u, sl, e in inc[v]:
                        if state[u][0] == INF:
                            continue
                        cand = (state[u][0] + cost(e), u, sl)
                        if cand < best:
                            best = cand
                changed |= best != state[v]
                nxt[v] = best
            state = nxt
            ev.append(f"round: {rounds} {fmt([(v, d[0], d[1]) for v, d in enumerate(state) if d[0] < INF and d[1] != NONE])}")
            if not changed:
                break
        # Dijkstra from the tree: the same distances.
        dist = {v: 0.0 for v in range(n) if parent[v] != NONE}
        pq = [(0.0, v) for v in dist]
        heapq.heapify(pq)
        while pq:
            d, u = heapq.heappop(pq)
            if d > dist.get(u, INF):
                continue
            for w, _, e in outr[u]:
                if parent[w] == NONE and d + cost(e) < dist.get(w, INF):
                    dist[w] = d + cost(e)
                    heapq.heappush(pq, (dist[w], w))
        assert all(abs(dist.get(v, INF) - state[v][0]) < 1e-12 or dist.get(v, INF) == state[v][0] for v in range(n))
        cand = [t for t in un if state[t][0] < INF]
        if not cand:
            break
        t = min(cand, key=lambda x: (state[x][0], x))
        un.remove(t)
        v, path = t, []
        while parent[v] == NONE:
            path.append(v)
            _, u, sl = state[v]
            parent[v] = u
            e = next(e for w, s2, e in outr[u] if s2 == sl)
            pcost[v] = cost(e)
            v = u
        ev.append(f"join: {t} {rf(state[t][0])} {fmt([v] + path[::-1])}")
        for x in reversed(path):
            state[x] = (0.0, NONE, NONE)
    if reroute:
        changed = False

        def below(s, t):
            v = s
            for _ in range(n + 1):
                if v == t:
                    return True
                if parent[v] == v or parent[v] == NONE:
                    return False
                v = parent[v]
            return True
        for s in range(n):
            if parent[s] == NONE:
                continue
            for t, sl, e in sorted(outr[s], key=lambda x: (x[0], x[1])):
                pa = parent[t]
                if pa == NONE or pa == t:
                    continue
                w = cost(e)
                if w < pcost[t] and not below(s, t):
                    ev.append(f"reroute: {t} {pa} -> {s} {rf(w)}")
                    parent[t], pcost[t], changed = s, w, True
        if changed:
            keep = [False] * n
            keep[source] = True
            for t in set(targets):
                if parent[t] == NONE:
                    continue
                v = t
                while not keep[v]:
                    keep[v] = True
                    v = parent[v]
            for v in range(n):
                if not keep[v]:
                    if parent[v] != NONE:
                        ev.append(f"cut: {v}")
                    parent[v] = NONE
    rows = [[{"node": v}, {"node": parent[v]}, 0.0 if parent[v] == v else pcost[v]] for v in range(n) if parent[v] != NONE]
    return ev, rows


# ------------------------------------------ 154 prize-collecting Steiner

def pcst_ref(g):
    """154: Goemans and Williamson's growth (Hegde, Indyk and Schmidt,
    `pcst_fast` @ 25ab31a `pcst_fast.cc`, MIT; GDS `pricesteiner/
    GrowthPhase.java` `grow`, studied): every node is an active cluster
    whose budget is its prize; moats grow at rate 1 on active clusters; an
    edge whose two moat sums reach its weight merges its clusters into an
    active one; a cluster whose moats reach its budget becomes inactive;
    until one active cluster is left; a negative edge merges its ends
    first (`handleNegativeEdge`). Equal times: the cluster event first
    (GDS: `clusterEventTime <= edgeEventTime`), then the smaller cluster
    or edge index. Strong pruning: GDS's `StrongPruning.java`
    `performPruning` (leaves peeled in index order, a subtree kept under
    its parent when its value exceeds the edge's weight, the best value
    the strict maximum) with lpg's change (`trees.rs:80-83`): the best
    node is detached and its subtree kept. Edges: each undirected edge
    once, by its larger end then the smaller (GDS `initializeEdgeParts`)."""
    n = g["n"]
    prizes = [float(x.split("prize: ")[1].rstrip(",")) for x in g["nodes"]]
    fwd, _ = adjacency(g)
    edges = []
    for u in range(n):
        for v, e in fwd[u]:
            if u != v:
                edges.append((max(u, v), min(u, v), weight(g, e)))
    edges.sort(key=lambda x: (x[0], x[1]))
    ev = []
    cl = [dict(parent=NONE, active=True, since=0.0, moat=0.0, left=p, ch=None) for p in prizes]
    active, tree = n, []

    def top(v):
        while cl[v]["parent"] != NONE:
            v = cl[v]["parent"]
        return v

    def moat(c, t):
        return t - c["since"] if c["active"] else c["moat"]

    def total(v, t):
        s = moat(cl[v], t)
        while cl[v]["parent"] != NONE:
            v = cl[v]["parent"]
            s += moat(cl[v], t)
        return s

    def merge(a, b, t):
        left = cl[a]["left"] + cl[b]["left"] - moat(cl[a], t) - moat(cl[b], t)
        new = len(cl)
        cl.append(dict(parent=NONE, active=True, since=t, moat=0.0, left=left, ch=(a, b)))
        for c in (a, b):
            if cl[c]["active"]:
                cl[c]["moat"], cl[c]["active"], cl[c]["since"] = moat(cl[c], t), False, t
            cl[c]["parent"] = new
        return new
    for i, (u, v, w) in enumerate(edges):
        if w < 0:
            a, b = top(u), top(v)
            if a != b:
                before = int(cl[a]["active"]) + int(cl[b]["active"])
                new = merge(a, b, 0.0)
                cl[new]["left"] -= w
                active += 1 - before
                ev.append(f"negative: {i} {a} {b} -> {new}")
                tree.append(i)
    now = 0.0
    while active > 1:
        ce = None
        for c, x in enumerate(cl):
            if x["active"] and (ce is None or x["since"] + x["left"] < ce[0]):
                ce = (x["since"] + x["left"], c)
        ee = None
        for i, (u, v, w) in enumerate(edges):
            if w < 0:
                continue
            a, b = top(u), top(v)
            if a == b or not (cl[a]["active"] or cl[b]["active"]):
                continue
            rest = w - total(u, now) - total(v, now)
            dt = rest / 2.0 if cl[a]["active"] and cl[b]["active"] else rest
            t = now + max(dt, 0.0)
            if ee is None or t < ee[0]:
                ee = (t, i, a, b)
        if ce is not None and (ee is None or ce[0] <= ee[0]):
            now = max(now, ce[0])
            c = cl[ce[1]]
            c["moat"], c["active"], c["since"] = moat(c, now), False, now
            active -= 1
            ev.append(f"deactivate: {ce[1]} {rf(now)}")
        else:
            now = max(now, ee[0])
            a, b = ee[2], ee[3]
            before = int(cl[a]["active"]) + int(cl[b]["active"])
            new = merge(a, b, now)
            active += 1 - before
            ev.append(f"merge: {ee[1]} {a} {b} -> {new} {rf(now)}")
            tree.append(ee[1])
    mem = [False] * n
    last = next((c for c in range(len(cl)) if cl[c]["active"] and cl[c]["parent"] == NONE), None)
    if last is not None:
        st = [last]
        while st:
            c = st.pop()
            if c < n:
                mem[c] = True
            else:
                st += list(cl[c]["ch"])
    # Strong pruning.
    cnt = sum(mem)
    par, pc = [NONE] * n, [0.0] * n
    best, dp = None, [0.0] * n
    if cnt == 1:
        v = mem.index(True)
        par[v] = v
    elif cnt > 1:
        adj, deg = [[] for _ in range(n)], [0] * n
        for i in tree:
            u, v, w = edges[i]
            if mem[u] and mem[v]:
                adj[u].append((v, w))
                adj[v].append((u, w))
                deg[u] += 1
                deg[v] += 1
        for a in adj:
            a.sort(key=lambda x: x[0])
        q = [v for v in range(n) if deg[v] == 1]
        at = 0
        while at < len(q):
            leaf = q[at]
            at += 1
            dp[leaf] += prizes[leaf]
            deg[leaf] = 0
            par[leaf] = leaf
            if best is None or dp[best] < dp[leaf]:
                best = leaf
            nb = [(x, w) for x, w in adj[leaf] if deg[x] > 0]
            if not nb:
                ev.append(f"leaf: {leaf} {rf(dp[leaf])} root")
                continue
            pa, w = nb[0]
            if w < dp[leaf]:
                dp[pa] += dp[leaf] - w
                par[leaf], pc[leaf] = pa, w
            ev.append(f"leaf: {leaf} {rf(dp[leaf])} {'keep' if par[leaf] == pa else 'cut'} {pa}")
            deg[pa] -= 1
            if deg[pa] == 1:
                q.append(pa)
        ev.append(f"best: {best} {rf(dp[best])} detached {fmt(par[best] != best)}")
        par[best] = best
        keep = {best: True}
        for v in range(n):
            if par[v] == NONE or v in keep:
                continue
            chain, x = [v], v
            while True:
                if x in keep:
                    verdict = keep[x]
                    break
                if par[x] == x:
                    verdict = False
                    break
                x = par[x]
                chain.append(x)
            for c in chain:
                keep.setdefault(c, verdict)
        par = [par[v] if keep.get(v) else NONE for v in range(n)]
    rows = [[{"node": v}, {"node": par[v]}, pc[v]] for v in range(n) if par[v] != NONE and par[v] != v]
    return ev, rows, par


# ---------------------------------------------------------- 155, 157

def network(g, sources, targets, cost=False):
    """lpg `flow::network`: arcs 2e (along edge e, forward position order)
    and 2e + 1; a super source feeding each source with its total outgoing
    capacity and each target feeding a super sink with its incoming one."""
    n = g["n"]
    fwd, _ = adjacency(g)
    head, res, cst, tails = [], [], [], []
    oc, ic = [0.0] * n, [0.0] * n
    for u in range(n):
        for v, e in fwd[u]:
            _, _, c, k = FL8[e - n]
            c = 0.0 if u == v else max(c, 0.0)
            head += [v, u]
            res += [c, 0.0]
            cst += [k, -k] if cost else [0.0, -0.0]
            tails += [u, v]
            oc[u] += c
            ic[v] += c
    m = len(head) // 2
    s, t = n, n + 1
    for x in sorted(set(sources)):
        head += [x, s]
        res += [oc[x], 0.0]
        cst += [0.0, 0.0]
        tails += [s, x]
    for x in sorted(set(targets)):
        head += [t, x]
        res += [ic[x], 0.0]
        cst += [0.0, 0.0]
        tails += [x, t]
    arcs = [[a for a in range(len(tails)) if tails[a] == v] for v in range(n + 2)]
    return dict(n=n, m=m, head=head, res=res, cost=cst, tail=tails, arcs=arcs)


def push_relabel_ref(net):
    """155: lpg's synchronous push-relabel (`flow.rs:13-29`; Goldberg and
    Tarjan, J. ACM 35(4) 1988, generic algorithm): heights 0, the super
    source at n + 2, its arcs saturated; each round, a push phase in
    which every active node pushes min(remaining excess, residual) along
    its admissible arcs (h(u) = h(v) + 1) in arc order, from the round's
    start state, applied in node order; then a relabel phase in which
    every active node without an admissible arc takes one more than the
    lowest height of its residual arcs' heads, reading the heights the
    round started with. The rounds are replayed below operation by
    operation on the sequential generic algorithm: every push is along an
    admissible arc and at most min(e(u), r(a)) at its time, every relabel
    raises a node without admissible arc and keeps the labelling valid."""
    n = net["n"]
    head, res, tail, arcs = net["head"], net["res"], net["tail"], net["arcs"]
    s = n
    h = [0] * (n + 2)
    ex = [0.0] * (n + 2)
    h[s] = n + 2
    for a in arcs[s]:
        d = res[a]
        if d > 0.0:
            res[a] -= d
            res[a ^ 1] += d
            ex[head[a]] += d
            ex[s] -= d
    ev = [f"init: {fmt(ex[:n])}"]
    r = 0
    active = lambda v: v < n and ex[v] > 0.0

    def valid():
        return all(h[tail[a]] <= h[head[a]] + 1 for a in range(len(head)) if res[a] > 0.0)
    assert valid()
    while any(active(v) for v in range(n)):
        r += 1
        pushes = []
        for u in range(n):
            if not active(u):
                continue
            left = ex[u]
            for a in arcs[u]:
                if res[a] > 0.0 and h[u] == h[head[a]] + 1:
                    d = min(left, res[a])
                    pushes.append((a, d))
                    left -= d
                    if left == 0.0:
                        break
        ev.append(f"push: {r} {fmt([(tail[a], head[a], a, d) for a, d in pushes])}")
        for a, d in pushes:
            # Sequential replay: admissible, within the excess and residual.
            assert h[tail[a]] == h[head[a]] + 1 and d <= res[a] and d <= ex[tail[a]] + 1e-12
            res[a] -= d
            res[a ^ 1] += d
            ex[tail[a]] -= d
            ex[head[a]] += d
            assert valid()
        nh = list(h)
        for u in range(n):
            if not active(u):
                continue
            if any(res[a] > 0.0 and h[u] == h[head[a]] + 1 for a in arcs[u]):
                continue
            hs = [h[head[a]] for a in arcs[u] if res[a] > 0.0]
            if hs:
                nh[u] = min(hs) + 1
        for u in range(n):
            if nh[u] != h[u]:
                # Sequential relabel: no admissible arc at its time, and
                # the labelling stays valid (heights only grow).
                assert nh[u] > h[u]
                assert not any(res[a] > 0.0 and h[u] == h[head[a]] + 1 for a in arcs[u])
                h[u] = nh[u]
                assert valid()
        ev.append(f"height: {r} {fmt(h[:n])} {fmt(ex[:n])}")
    return ev


def flow_rows(g, net):
    rows = []
    n = g["n"]
    for e in range(net["m"]):
        f = net["res"][2 * e + 1]
        if f > 0.0:
            rows.append([{"node": net["head"][2 * e + 1]}, {"node": net["head"][2 * e]}, f])
    return rows


def ssp_ref(net):
    """157: NetworKit `flow/SuccessiveShortestPath.cpp`
    (`computeNodePotentials`: Bellman-Ford passes; `dijkstraOnResidualGraph`
    on reduced costs; potentials raised by the distances; the bottleneck
    pushed), from the super source to the super sink, as lpg ports it
    (`flow.rs:30-38`): Bellman-Ford over the arcs with capacity, Dijkstra
    with ties by node index (a lazy heap of (distance, node))."""
    total = net["n"] + 2
    head, res, cst, tail, arcs = net["head"], net["res"], net["cost"], net["tail"], net["arcs"]
    s, t = total - 2, total - 1
    pot = [0.0] * total
    ev = []
    for i in range(total + 1):
        upd = False
        for a in range(len(head)):
            if res[a] <= 0.0:
                continue
            u, v = tail[a], head[a]
            if pot[u] + cst[a] < pot[v]:
                pot[v] = pot[u] + cst[a]
                upd = True
        if not upd:
            break
        ev.append(f"potential: {i} {fmt(pot)}")
        assert i < total
    while True:
        dist, via, done = [math.inf] * total, [NONE] * total, [False] * total
        dist[s] = 0.0
        pq = [(0.0, s)]
        while pq:
            d, u = heapq.heappop(pq)
            if done[u]:
                continue
            done[u] = True
            for a in arcs[u]:
                if res[a] <= 0.0:
                    continue
                v = head[a]
                nd = d + (cst[a] + pot[u] - pot[v])
                if nd < dist[v]:
                    dist[v], via[v] = nd, a
                    heapq.heappush(pq, (nd, v))
        if dist[t] == math.inf:
            break
        for v in range(total):
            if dist[v] < math.inf:
                pot[v] += dist[v]
        b, v, path = math.inf, t, [t]
        while v != s:
            a = via[v]
            b = min(b, res[a])
            v = tail[a]
            path.append(v)
        v = t
        while v != s:
            a = via[v]
            res[a] -= b
            res[a ^ 1] += b
            v = tail[a]
        ev.append(f"augment: {fmt(path[::-1])} {rf(b)} {rf(dist[t])}")
    return ev


# ------------------------------------------------------ 161 matching

def ungraph_neighbors(s):
    """petgraph `Graph::neighbors` on lpg's `ungraph` (edges v < w added in
    order): outgoing edges newest first, then incoming newest first."""
    n = len(s)
    edges = [(v, w) for v in range(n) for w in s[v] if v < w]
    outl = [[w for (a, w) in reversed(edges) if a == v] for v in range(n)]
    inl = [[a for (a, w) in reversed(edges) if w == v] for v in range(n)]
    return edges, [outl[v] + inl[v] for v in range(n)]


def greedy_matching_ref(g):
    """161 `greedy`: petgraph 0.8.3 `matching.rs:211` `greedy_matching` and
    `non_backtracking_dfs:250` (from each node, a path that never
    backtracks; alternate edges matched)."""
    s = simple(g)
    n = len(s)
    edges, nb = ungraph_neighbors(s)
    mate, visited = [None] * n, [False] * n
    for start in range(n):
        last = [start]

        def dfs(x):
            if not visited[x]:
                visited[x] = True
                for y in nb[x]:
                    if not visited[y]:
                        if last[0] is not None:
                            p = last[0]
                            last[0] = None
                            mate[p], mate[y] = y, p
                        else:
                            last[0] = y
                        dfs(y)
                        break
        dfs(start)
    return [f"graph: {fmt(edges)}", "mate: [" + ", ".join(opt(m) for m in mate) + "]"], mate


# --------------------------------------------- 163 bipartite matching

def bipartite_ref(g):
    """163: MAGE `bipartite_matching.cpp` (`IsSubgraphBipartiteColoring`: a
    BFS two-colouring starting with colour 1; `BipartiteMatchingDFS`:
    Kuhn's augmenting search, visited marks on left nodes) on the simple
    graph, left = colour 0 in node order, neighbours ascending (lpg's
    documented order, `matching.rs:15-22`)."""
    s = simple(g)
    n = len(s)
    color = [None] * n
    for st in range(n):
        if color[st] is not None:
            continue
        color[st] = 1
        q = [st]
        for v in q:
            for w in s[v]:
                assert color[w] != color[v]
                if color[w] is None:
                    color[w] = 1 - color[v]
                    q.append(w)
    ev = [f"colour: {fmt(color)}"]
    matched = [None] * n
    size = 0
    for u in (v for v in range(n) if color[v] == 0):
        visited = [False] * n
        found = [None]

        def dfs(x):
            if visited[x]:
                return False
            visited[x] = True
            if x != u:
                pass
            for y in s[x]:
                if matched[y] is None:
                    found[0] = found[0] if found[0] is not None else y
                    matched[y] = x
                    return True
                z = matched[y]
                if not visited[z]:
                    ev.append(f"visit: {u} {x} {y} {z}")
                if dfs(z):
                    matched[y] = x
                    return True
            return False
        ok = dfs(u)
        size += ok
        ev.append(f"search: {u} {opt(found[0])}")
    G = nx.Graph()
    G.add_nodes_from(range(n))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"])
    top = {v for v in range(n) if color[v] == 0}
    assert size == len(nx.bipartite.hopcroft_karp_matching(G, top)) // 2
    return ev, size


# ------------------------------------------------ 169 maximal cliques

def bk_ref(g):
    """169: petgraph 0.8.3 `maximal_cliques.rs` `bron_kerbosch_pivot` (pivot:
    a node of P with the most neighbours; candidates P minus the pivot's
    neighbours, taken from the end) with lpg's documented change: sorted
    vectors, the first such pivot (`matching.rs:23-28`)."""
    s = simple(g)
    ev, found = [], []

    def bk(clique, p, x):
        if not p:
            if not x:
                c = sorted(clique)
                ev.append(f"clique: {fmt(c)}")
                found.append(c)
            return
        u = p[0]
        for v in p:
            if len(s[v]) > len(s[u]):
                u = v
        ev.append(f"pivot: {fmt(clique)} {fmt(p)} {fmt(x)} {u}")
        todo = [v for v in p if v == u or v not in s[u]]
        while todo:
            v = todo.pop()
            p = [w for w in p if w != v]
            bk(clique + [v], [w for w in p if w in s[v]], [w for w in x if w in s[v]])
            x = sorted(x + [v])
    bk([], list(range(len(s))), [])
    G = nx.Graph()
    G.add_nodes_from(range(len(s)))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"] if a != b)
    assert sorted(found) == sorted(sorted(c) for c in nx.find_cliques(G))
    return ev, found


# ------------------------------------------------- 171 clique counting

def clique_count_ref(g):
    """171: for each node, the cliques among its neighbours by increasing
    index (`matching.rs:29-37`, written for lpg); counts[i] = cliques of
    i + 3 nodes holding the node; globalCount[i] their number."""
    s = simple(g)
    ev, per = [], []

    def ext(cand, size, counts):
        for i, c in enumerate(cand):
            k = size + 1
            if k >= 2:
                while len(counts) < k - 1:
                    counts.append(0)
                counts[k - 2] += 1
            nxt = [w for w in cand[i + 1:] if w in s[c]]
            if nxt:
                ext(nxt, k, counts)
    for v in range(len(s)):
        counts = []
        ext(s[v], 0, counts)
        per.append(counts)
        ev.append(f"cq: {v} {fmt(counts)}")
    glob = []
    for c in per:
        while len(glob) < len(c):
            glob.append(0)
        for i, x in enumerate(c):
            glob[i] += x
    glob = [x // (i + 3) for i, x in enumerate(glob)]
    G = nx.Graph()
    G.add_nodes_from(range(len(s)))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"] if a != b)
    sizes = {}
    for c in nx.enumerate_all_cliques(G):
        if len(c) >= 3:
            sizes[len(c)] = sizes.get(len(c), 0) + 1
    assert glob == [sizes.get(i + 3, 0) for i in range(len(glob))]
    return ev, per, glob


# ------------------------------------------------ 173, 174, 176 colouring

def strategy_ref(s, name, seed=0):
    """173: NetworkX 3.7 `coloring/greedy_coloring.py` strategies, with
    lpg's documented change (`matching.rs:38-46`): where NetworkX takes an
    arbitrary set element, the smallest index; `random_sequential` shuffles
    with lpg's generator (stream 0, Fisher-Yates from the end)."""
    n = len(s)
    deg = lambda v: len(s[v])
    if name == "largest_first":
        return sorted(range(n), key=lambda v: (-deg(v), v))
    if name == "random_sequential":
        o, gen = list(range(n)), rng(seed, 0)
        for i in range(n - 1, 0, -1):
            j = gen.range_incl(0, i)
            o[i], o[j] = o[j], o[i]
        return o
    if name == "smallest_last":
        d = [deg(v) for v in range(n)]
        removed, out = [False] * n, []
        for _ in range(n):
            u = min((v for v in range(n) if not removed[v]), key=lambda v: (d[v], v))
            out.insert(0, u)
            removed[u] = True
            for w in s[u]:
                if not removed[w]:
                    d[w] -= 1
        return out
    if name == "independent_set":
        rem, out = set(range(n)), []
        while rem:
            left, chosen = set(rem), []
            while left:
                v = min(left, key=lambda v: (len([w for w in s[v] if w in left]), v))
                chosen.append(v)
                left.discard(v)
                left -= set(s[v])
            chosen.sort()
            rem -= set(chosen)
            out += chosen
        return out
    if name in ("connected_sequential_bfs", "connected_sequential_dfs"):
        seen, out = [False] * n, []
        for src in range(n):
            if seen[src]:
                continue
            seen[src] = True
            out.append(src)
            if name.endswith("dfs"):
                def dfs(v):
                    for w in s[v]:
                        if not seen[w]:
                            seen[w] = True
                            out.append(w)
                            dfs(w)
                dfs(src)
            else:
                q = [src]
                for v in q:
                    for w in s[v]:
                        if not seen[w]:
                            seen[w] = True
                            out.append(w)
                            q.append(w)
        return out
    if name == "saturation_largest_first":
        col, dist, out = [None] * n, [set() for _ in range(n)], []
        for _ in range(n):
            v = None
            for x in range(n):
                if col[x] is None and (v is None or (len(dist[x]), deg(x)) > (len(dist[v]), deg(v))):
                    v = x
            c = 0
            while any(col[w] == c for w in s[v]):
                c += 1
            col[v] = c
            out.append(v)
            for w in s[v]:
                dist[w].add(c)
        return out
    raise ValueError(name)


def greedy_ref(s, order):
    ev, col = [], [None] * len(s)
    for v in order:
        used = {col[w] for w in s[v] if col[w] is not None}
        c = next(c for c in itertools.count() if c not in used)
        col[v] = c
        ev.append(f"colour: {v} {c}")
    return ev, col


def coloring_ref(g, name, seed=0):
    s = simple(g)
    order = strategy_ref(s, name, seed)
    ev, col = greedy_ref(s, order)
    G = nx.Graph()
    G.add_nodes_from(range(len(s)))
    G.add_edges_from((a, b) for a, b, *_ in g["edges"] if a != b)
    want = nx.greedy_color(G, strategy=lambda G, colors: iter(order))
    assert want == {v: col[v] for v in range(len(s))}
    if name in ("largest_first", "connected_sequential_bfs", "connected_sequential_dfs", "saturation_largest_first"):
        # NetworkX's own strategy orders (no arbitrary choice here).
        assert nx.greedy_color(G, strategy=name) == want, name
    return [f"order: {fmt(order)}"] + ev, col


class RustHeap:
    """Rust std `BinaryHeap` (max heap; `push`: `sift_up`; `pop`:
    `sift_down_to_bottom` then `sift_up`, `binary_heap/mod.rs:675-925`)."""

    def __init__(self):
        self.d = []

    def push(self, item):
        self.d.append(item)
        self._up(0, len(self.d) - 1)

    def _up(self, start, pos):
        e = self.d[pos]
        while pos > start:
            par = (pos - 1) // 2
            if e[0] <= self.d[par][0]:
                break
            self.d[pos] = self.d[par]
            pos = par
        self.d[pos] = e
        return pos

    def pop(self):
        if not self.d:
            return None
        item = self.d.pop()
        if self.d:
            item, self.d[0] = self.d[0], item
            end, pos = len(self.d), 0
            e = self.d[0]
            child = 1
            while child <= end - 2:
                if self.d[child][0] <= self.d[child + 1][0]:
                    child += 1
                self.d[pos] = self.d[child]
                pos = child
                child = 2 * pos + 1
            if child == end - 1:
                self.d[pos] = self.d[child]
                pos = child
            self.d[pos] = e
            self._up(0, pos)
        return item


def dsatur_ref(g):
    """174: petgraph 0.8.3 `coloring.rs:63` `dsatur_coloring` on the
    UNDIRECTED projection (degree: `edges(node).count()`, parallel edges
    and a self-loop twice; a max `BinaryHeap` of (saturation, degree)
    entries, ties as the heap leaves them)."""
    n = g["n"]
    out, _ = adjacency(g, "UNDIRECTED")
    degree = [len(out[v]) for v in range(n)]
    q = RustHeap()
    for v in range(n):
        q.push(((0, degree[v]), v))
    col, adj, seen, mx = [None] * n, [set() for _ in range(n)], [False] * n, 0
    while True:
        it = q.pop()
        if it is None:
            break
        _, v = it
        if seen[v]:
            continue
        seen[v] = True
        c = 0
        while c in adj[v]:
            c += 1
        col[v] = c
        mx = max(mx, c)
        for w, _ in out[v]:
            adj[w].add(c)
            q.push(((len(adj[w]), degree[w]), w))
    return [f"dsatur: [{', '.join(opt(c) for c in col)}] {mx + 1 if n else 1}"], col


def jp_ref(g):
    """176: Jones and Plassmann's rounds (SIAM J. Sci. Comput. 14(3), 1993;
    LAGraph `LAGraph_coloring_independent_set.c`) with the
    largest-degree-first priority (`matching.rs:47-65`): each round, every
    uncoloured node all of whose higher-priority neighbours are coloured
    takes the smallest colour its coloured neighbours lack, reading the
    last round. Sequential formulation: greedy colouring in priority
    order gives the same colours (asserted)."""
    s = simple(g, both=True)
    n = len(s)
    first = lambda a, b: len(s[a]) > len(s[b]) or (len(s[a]) == len(s[b]) and a < b)
    col, ev, r = [None] * n, [], 0
    while any(c is None for c in col):
        r += 1
        nxt = list(col)
        for v in range(n):
            if col[v] is not None or any(col[w] is None and first(w, v) for w in s[v]):
                continue
            used = {col[w] for w in s[v] if col[w] is not None}
            nxt[v] = next(c for c in itertools.count() if c not in used)
        ev.append(f"round: {r} {fmt([(v, nxt[v]) for v in range(n) if nxt[v] is not None and col[v] is None])}")
        col = nxt
    order = sorted(range(n), key=lambda v: (-len(s[v]), v))
    assert greedy_ref(s, order)[1] == col
    return ev, col


# ----------------------------------------------- 177 feedback arc set

def fas_ref(g):
    """177: petgraph 0.8.3 `feedback_arc_set.rs` `good_node_sequence` (Eades,
    Lin and Smyth 1993): nodes in order of first appearance in the edge
    list; buckets of sinks, sources and by out-in degree, each a list with
    `push_front` and `pop` from the front; sinks then sources then the
    highest degree difference; the arcs u -> v with seq(u) >= seq(v), in
    edge order. Edges: `p.out` in forward order."""
    n = g["n"]
    fwd, _ = adjacency(g)
    edges = [(u, v) for u in range(n) for v, _ in fwd[u]]
    idx, nodes = {}, []
    for a, b in edges:
        for x in (a, b):
            if x not in idx:
                idx[x] = len(nodes)
                nodes.append(dict(g=x, out=[], inn=[], od=0, id=0, inlist=False))
        nodes[idx[a]]["out"].append(idx[b])
        nodes[idx[b]]["inn"].append(idx[a])
    for nd in nodes:
        nd["od"], nd["id"] = len(nd["out"]), len(nd["inn"])
    sinks, sources, pos, neg = [], [], [], []

    def bucket(i):
        nd = nodes[i]
        if nd["od"] == 0:
            return sinks
        if nd["id"] == 0:
            return sources
        dd = nd["od"] - nd["id"]
        lst, b = (pos, dd) if dd >= 0 else (neg, -dd - 1)
        while len(lst) <= b:
            lst.append([])
        return lst[b]

    def push(i):
        bucket(i).insert(0, i)
        nodes[i]["inlist"] = True

    def remove(i):
        bucket(i).remove(i)
        nodes[i]["inlist"] = False

    def pop(lst):
        i = lst.pop(0)
        nodes[i]["inlist"] = False
        return i

    def update(i):
        for o in nodes[i]["out"]:
            if o == i or not nodes[o]["inlist"]:
                continue
            remove(o)
            nodes[o]["id"] -= 1
            push(o)
        for o in nodes[i]["inn"]:
            if o == i or not nodes[o]["inlist"]:
                continue
            remove(o)
            nodes[o]["od"] -= 1
            push(o)

    def trim(lst):
        while lst and not lst[-1]:
            lst.pop()
    for i in range(len(nodes)):
        push(i)
    s1, s2 = [], []
    while True:
        moved = False
        while sinks:
            i = pop(sinks)
            moved = True
            update(i)
            s2.insert(0, nodes[i]["g"])
        while sources:
            i = pop(sources)
            moved = True
            update(i)
            s1.append(nodes[i]["g"])
        lst = next((b for b in list(reversed(pos)) + neg if b), None)
        if lst is not None:
            i = pop(lst)
            moved = True
            update(i)
            s1.append(nodes[i]["g"])
            trim(pos)
            trim(neg)
        if not moved:
            break
    seq = {x: k for k, x in enumerate(s1 + s2)}
    arcs = [(a, b) for a, b in edges if seq[a] >= seq[b]]
    D = nx.MultiDiGraph()
    D.add_nodes_from(range(n))
    rest = list(edges)
    for a in arcs:
        rest.remove(a)
    D.add_edges_from(rest)
    assert nx.is_directed_acyclic_graph(D)
    return [f"graph: {fmt(edges)}", f"arcs: {fmt(arcs)}"], arcs


# --------------------------------------------------------- 178 set cover

def cover_ref(elements, sets):
    """178: the classic greedy set cover (Johnson 1974; Chvátal 1979):
    while an element is uncovered, the set covering the most uncovered
    elements, the first listed on a tie (`matching.rs:66-80`)."""
    order, members = [], {}
    for e, s in zip(elements, sets):
        if s not in members:
            order.append(s)
        members.setdefault(s, set()).add(e)
    unc, ev, out = set(elements), [], []
    while unc:
        best = None
        for s in order:
            gain = len(members[s] & unc)
            if gain > 0 and (best is None or gain > best[0]):
                best = (gain, s)
        if best is None:
            break
        ev.append(f"pick: {best[1]} {best[0]}")
        unc -= members[best[1]]
        out.append(best[1])
    return ev, out


# ------------------------------------------------- blossom (ALG-187)

def blossom_ref(n, edges, maxcard):
    """`blossom.rs` (rustworkx `max_weight_matching.rs`, a port of van
    Rantwijk's `mwmatching.py`): the reference is NetworkX 3.7's
    `max_weight_matching` (`algorithms/matching.py:321`), the same
    algorithm on dictionaries, instrumented by editing its source at run
    time: an event at each blossom formed (`addBlossom`), expanded
    (`expandBlossom`), augmentation (`augmentMatching`), augmentation
    through a blossom (`augmentBlossom`) and dual change (`deltatype`,
    `delta`). Blossoms are numbered as rustworkx numbers them: ids n..2n
    taken from the end of a free list and returned on expansion. One
    change: `augmentMatching(v, w)` takes the edge in its input
    orientation, as rustworkx's `augment_matching(k)` (`edges[k]`), which
    fixes which side is traced back first."""
    path = nx.algorithms.matching.__file__
    text = open(path).read()
    start = text.index("def max_weight_matching(")
    nxt = [i for i in (text.find("\n@", start), text.find("\ndef ", start + 1)) if i > 0]
    src = text[start:min(nxt) if nxt else len(text)]

    def sub(old, new, count=1):
        nonlocal src
        assert src.count(old) == count, (old, src.count(old))
        src = src.replace(old, new)
    sub("        b = Blossom()\n", "        b = Blossom()\n        IDS[b] = FREE.pop()\n")
    sub("        # Set label to S.\n        assert label[bb] == 1\n",
        "        EV.append(f\"blossom: {IDS[b]} base {base} {sorted(b.leaves())}\")\n        # Set label to S.\n        assert label[bb] == 1\n")
    sub("        def _recurse(b, endstage):\n", "        def _recurse(b, endstage):\n            EV.append(f\"expand: {IDS[b]} {'true' if endstage else 'false'} {sorted(b.leaves())}\")\n")
    sub("            del blossomdual[b]\n", "            del blossomdual[b]\n            FREE.append(IDS[b])\n")
    sub("        def _recurse(b, v):\n", "        def _recurse(b, v):\n            EV.append(f\"augblossom: {IDS[b]} {v}\")\n")
    sub("    def augmentMatching(v, w):\n", "    def augmentMatching(v, w):\n        v, w = ORIENT[(v, w)]\n        EV.append(f\"augment: {v} {w}\")\n")
    sub("            # Update dual variables according to delta.\n",
        "            EV.append(f\"delta: {deltatype} {delta}\")\n            # Update dual variables according to delta.\n")
    ns = dict(nx.algorithms.matching.__dict__)
    ev = []
    orient = {}
    for a, b, _ in edges:
        orient[(a, b)] = (a, b)
        orient[(b, a)] = (a, b)
    ns.update(EV=ev, FREE=list(range(n, 2 * n)), IDS={}, ORIENT=orient)
    exec(src, ns)
    G = nx.Graph()
    G.add_nodes_from(range(n))
    for a, b, w in edges:
        G.add_edge(a, b, weight=w)
    mate = ns["max_weight_matching"](G, maxcardinality=maxcard)
    pairs = sorted(tuple(sorted(p)) for p in mate)
    assert pairs == sorted(tuple(sorted(p)) for p in nx.max_weight_matching(G, maxcardinality=maxcard))
    # The numbers rustworkx's `> num_nodes` test would miss: blossom n.
    ids = [int(e.split()[1]) for e in ev if e.startswith("blossom:")]
    return ev, pairs, ids


BLOSSOM_CASES = [
    # NetworkX `tests/test_matching.py` (BSD-3-Clause): S-blossom,
    # T-blossom, nested S-blossom with relabel and expand, nasty cases.
    (False, [(1, 2, 8), (1, 3, 9), (2, 3, 10), (3, 4, 7)]),
    (False, [(1, 2, 9), (1, 3, 8), (2, 3, 10), (1, 4, 5), (4, 5, 4), (1, 6, 3)]),
    (False, [(1, 2, 9), (1, 3, 9), (2, 3, 10), (2, 4, 8), (3, 5, 8), (4, 5, 10), (5, 6, 6)]),
    (False, [(1, 2, 10), (1, 7, 10), (2, 3, 12), (3, 4, 20), (3, 5, 20), (4, 5, 25), (5, 6, 10), (6, 7, 10), (7, 8, 8)]),
    (False, [(1, 2, 23), (1, 5, 22), (1, 6, 15), (2, 3, 25), (3, 4, 22), (4, 5, 25), (4, 8, 14), (5, 7, 13)]),
    (False, [(1, 2, 45), (1, 5, 45), (2, 3, 50), (3, 4, 45), (4, 5, 50), (1, 6, 30), (3, 9, 35), (4, 8, 26), (5, 7, 40), (9, 10, 5)]),
    (False, [(1, 2, 45), (1, 7, 45), (2, 3, 50), (3, 4, 45), (4, 5, 95), (4, 6, 94), (5, 6, 94), (6, 7, 50), (1, 8, 30), (3, 11, 35), (5, 9, 36), (7, 10, 26), (11, 12, 5)]),
    (False, [(1, 2, 40), (1, 3, 40), (2, 3, 60), (2, 4, 55), (3, 5, 55), (4, 5, 50), (1, 8, 15), (5, 7, 30), (7, 6, 10), (8, 10, 10), (4, 9, 30)]),
    (True, [(1, 2, 1), (2, 3, 10), (3, 4, 1)]),
]


# ---------------------------------------------------------------- cases

def case(graph, proc, config=None, args=None, kinds=None, want=None, concurrency=(1,), rows=None, source=None, **extra):
    c = dict(config or {})
    want = [e for e in want if e.split(":")[0] in kinds]
    out = dict(graph=graph, proc=proc, args=args or [], config=c, kinds=kinds, want=want,
               concurrency=list(concurrency), source=source)
    if rows is not None:
        # GDS procedures return node identifiers (integers); MAGE ones,
        # nodes.
        out["rows"] = plain(rows) if proc.startswith("gds.") else rows
    out.update(extra)
    return out


def plain(x):
    if isinstance(x, dict) and set(x) == {"node"}:
        return x["node"]
    if isinstance(x, list):
        return [plain(y) for y in x]
    return x


def N(i):
    return {"node": i}


def ids_rows(ids):
    return [[N(v), c] for v, c in enumerate(ids)]


def main():
    G = GRAPHS
    par = (1, 16)
    cases = {}
    # 083 WCC.
    ev, lab = wcc_ref(G["cc"])
    cases["083"] = case("cc", "gds.wcc.stream", {}, kinds=["round"], want=ev, concurrency=par, rows=ids_rows(lab),
                        source="trace_ref_c.py wcc_ref; components.rs:8-31")
    ev, lab = wcc_ref(G["cc"], threshold=1.0)
    cases["083 threshold"] = case("cc", "gds.wcc.stream", {"relationshipWeightProperty": "weight", "threshold": 1.0}, kinds=["round"],
                                  want=ev, concurrency=par, rows=ids_rows(lab), source="wcc_ref; GDS Wcc.java defaultWeight")
    cases["083 seeds"] = case("cs", "gds.wcc.stream", {"relationshipWeightProperty": "weight", "threshold": 1.0, "seedProperty": "seed"},
                              kinds=["round"], want=ev, concurrency=par, rows=ids_rows(wcc_ids(lab, SEEDS)),
                              source="wcc_ref, wcc_ids; GDS HugeAtomicDisjointSetStruct setIdOf")
    _, lab = wcc_ref(G["cc"])
    cons = {c: i for i, c in enumerate(dict.fromkeys(lab))}
    cases["083 consecutiveIds"] = case("cc", "gds.wcc.stream", {"consecutiveIds": True}, kinds=[], want=[], concurrency=(1,),
                                       rows=ids_rows([cons[c] for c in lab]), source="components.rs consecutive")
    # 084.
    ev, ids = connected_ref(G["ug"])
    cases["084"] = case("ug", "gds.connectedComponents.stream", {}, kinds=["union", "label"], want=ev, rows=ids_rows(ids),
                        source="petgraph unionfind.rs union, into_labeling")
    # 085, 086.
    ev, rows, crows = scc_cases(G["cc"])
    cases["085"] = case("cc", "gds.scc.stream", {}, kinds=["scc"], want=ev, rows=rows, source="petgraph scc/tarjan_scc.rs visit")
    cases["086"] = case("cc", "gds.condensation.stream", {}, kinds=["scc", "succ"], want=ev, rows=crows,
                        source="petgraph mod.rs:477 condensation; tarjan_scc.rs")
    # 089.
    ev, rows = bcc_ref(G["ug"])
    cases["089"] = case("ug", "biconnected_components.get", {}, kinds=["tree", "back", "comp"], want=ev, rows=rows,
                        source="NetworkX biconnected.py _biconnected_dfs; Hopcroft-Tarjan 1973")
    # 090, 091.
    ev, ap, br = lowlink_ref(G["ug"])
    cases["090"] = case("ug", "gds.articulationPoints.stream", {}, kinds=["skip", "tree", "back", "done"], want=ev, rows=ap,
                        source="GDS ArticulationPoints.java, Bridges.java visitEvent (parent rule)")
    cases["091"] = case("ug", "gds.bridges.stream", {}, kinds=["skip", "tree", "back", "done"], want=ev, rows=br,
                        source="GDS Bridges.java visitEvent")
    cases["091 orientation ignored"] = case("ug", "gds.bridges.stream", {"orientation": "REVERSE"}, kinds=["tree"], want=ev, rows=br,
                                            source="TS-25 Orientation: U entries run UNDIRECTED")
    # 093.
    ev, core, _, _ = kcore_ref(G["ug"])
    cases["093"] = case("ug", "gds.kcore.stream", {}, kinds=["peel", "level"], want=ev, concurrency=par, rows=ids_rows(core),
                        source="kcore_ref; GDS KCoreDecomposition.java; Batagelj-Zaversnik")
    # 120, 121.
    ev, t, lst = triangles_ref(G["ug"])
    cases["120"] = case("ug", "gds.triangleCount.stream", {}, kinds=["tri"], want=ev, concurrency=par, rows=ids_rows(t),
                        source="graphina parallel/triangles.rs; GDS IntersectingTriangleCount.java")
    cases["120 listing"] = case("ug", "gds.triangles", {}, kinds=[], want=[], concurrency=par, rows=[[N(a), N(b), N(c)] for a, b, c in lst],
                                source="triangles_ref")
    ev5, t5, lst5 = triangles_ref(G["ug"], max_degree=3)
    cases["120 maxDegree 3"] = case("ug", "gds.triangleCount.stream", {"maxDegree": 3}, kinds=["tri"], want=ev5, concurrency=par,
                                    rows=ids_rows(t5), source="triangles_ref; GDS maxDegree measured rmp #67")
    cases["120 listing maxDegree 3"] = case("ug", "gds.triangles", {"maxDegree": 3}, kinds=[], want=[], concurrency=(1,),
                                            rows=[[N(a), N(b), N(c)] for a, b, c in lst5], source="triangles_ref")
    ev, vals = lcc_ref(G["ug"], t)
    # Both phases run by blocks with no event between them: the
    # recording sorts the run by block (`trace.rs` `Recording::take`).
    tri_ev = triangles_ref(G["ug"])[0]
    ev = take([(e, int(e.split()[1])) for e in tri_ev] + [(e, int(e.split()[1])) for e in ev])
    cases["121"] = case("ug", "gds.localClusteringCoefficient.stream", {}, kinds=["tri", "lcc"], want=ev,
                        concurrency=par, rows=[[N(v), x] for v, x in enumerate(vals)], source="lcc_ref; GDS LocalClusteringCoefficient.java")
    # 123.
    ev, ecc = ecc_ref(G["cc"], False)
    cases["123"] = case("cc", "gds.eccentricity.stream", {}, kinds=["dist"], want=ev,
                        rows=None, source="graphalgs metrics.rs eccentricity")
    ev, ecc = ecc_ref(G["ug"], True)
    cases["123 weighted"] = case("ug", "gds.eccentricity.stream", {"relationshipWeightProperty": "weight"}, kinds=["dist"], want=ev,
                                 source="graphalgs metrics.rs weighted_eccentricity (petgraph bellman_ford)")
    # 128.
    ev, gi = girth_ref(G["ug"], "UNDIRECTED")
    cases["128 UNDIRECTED"] = case("ug", "gds.girth.stream", {"orientation": "UNDIRECTED"}, kinds=["girth"], want=ev, rows=[[gi]],
                                   source="NetworkX cycles.py girth:1212-1232")
    ug_noloop = dict(G["ug"], edges=[e for e in G["ug"]["edges"] if e[0] != e[1]])
    G["ug5"] = ug_noloop
    ev, gi = girth_ref(ug_noloop, "UNDIRECTED")
    cases["128 UNDIRECTED no self-loop"] = case("ug5", "gds.girth.stream", {"orientation": "UNDIRECTED"}, kinds=["girth"], want=ev,
                                                rows=[[gi]], source="NetworkX girth")
    ev, gi = girth_ref(G["cc"], "NATURAL")
    cases["128 NATURAL"] = case("cc", "gds.girth.stream", {}, kinds=["girth"], want=ev, rows=[[gi]],
                                source="girth_ref directed; NetworkX simple_cycles (asserted)")
    # 143.
    ev, L = laplacian_ref(G["ug"], True, "UNDIRECTED")
    cases["143 UNDIRECTED weighted"] = case("ug", "gds.laplacianMatrix.stream", {"orientation": "UNDIRECTED", "relationshipWeightProperty": "weight"},
                                            kinds=["cell"], want=ev, rows=[[N(i), L[i]] for i in range(13)],
                                            source="graphalgs spec/laplacian.rs laplacian_matrix")
    ev, L = laplacian_ref(G["cc"], False)
    cases["143 NATURAL"] = case("cc", "gds.laplacianMatrix.stream", {}, kinds=["cell"], want=ev, rows=[[N(i), L[i]] for i in range(9)],
                                source="graphalgs spec/laplacian.rs")
    # 147.
    for label, gname, src, kw, cfg in (("147", "wt", 0, {}, {"relationshipWeightProperty": "weight"}),
                                       ("147 maximum", "wt", 3, dict(mx=True), {"relationshipWeightProperty": "weight", "objective": "maximum"}),
                                       ("147 ug", "ug", 6, {}, {"relationshipWeightProperty": "weight"}),
                                       ("147 unweighted", "ug", 0, dict(weighted=False), {})):
        ev, parent, cost, order = prim_ref(G[gname], src, **kw)
        rows = [[N(v), N(parent[v]), 0.0 if parent[v] == v else cost[v]] for v in range(G[gname]["n"]) if parent[v] != NONE]
        cases[label] = case(gname, "gds.spanningTree.stream", dict(cfg, sourceNode=N(src)), kinds=["pop", "key"], want=ev, rows=rows,
                            source="GDS spanningtree/Prim.java compute; HugeLongPriorityQueue")
    # 149 (write mode only: events).
    for label, gname, src, k, mx in (("149", "wt", 0, 3, False), ("149 k 5", "wt", 7, 5, False), ("149 maximum", "wt", 0, 4, True),
                                     ("149 root replaced (GDS re-queues it)", "kq", 0, 4, False),
                                     ("149 child of a trimmed node", "kd", 0, 4, False)):
        ev, tree, root = ktree_ref(G[gname], src, k, mx)
        cfg = {"relationshipWeightProperty": "weight", "sourceNode": N(src), "k": k, "writeProperty": "kst"}
        if mx:
            cfg["objective"] = "maximum"
        cases[label] = case(gname, "gds.kSpanningTree.write", cfg, kinds=["add", "trim", "reject", "skip", "drop", "root"], want=ev,
                            source="GDS kspanningtree/KSpanningTree.java growApproach; trees.rs:18-30",
                            tree=sorted(tree), root=root)
    # 151.
    for label, gname, cfg, root, orient in (("151 UNDIRECTED", "wt", {"orientation": "UNDIRECTED"}, 0, "UNDIRECTED"),
                                            ("151 UNDIRECTED ug", "ug5", {"orientation": "UNDIRECTED", "rootNode": N(3)}, 3, "UNDIRECTED"),
                                            ("151 NATURAL rooted", "cc", {"rootNode": N(0)}, 0, "NATURAL")):
        ev, det, _ = count_ref(G[gname], root, orient)
        cases[label] = case(gname, "gds.countSpanningTrees.stream", cfg, kinds=["residue", "crt"], want=ev, rows=[[det]],
                            source="graphalgs spec/st.rs:79; exact determinant (fractions)")
    G["ring130"] = cycle_chain(1, 130)
    ev, det, _ = count_ref(G["ring130"], 0, "UNDIRECTED")
    assert det == 130 and "False" not in ev[1] and ev[1].endswith("false")
    cases["151 ring of 130: bound above M"] = case("ring130", "gds.countSpanningTrees.stream", {"orientation": "UNDIRECTED"},
                                                   kinds=["residue", "crt"], want=ev, rows=[[det]], source="count_ref; exact 130")
    G["chain12"] = cycle_chain(12, 30)
    ev, det, _ = count_ref(G["chain12"], 0, "UNDIRECTED")
    assert det == 30 ** 12 and det < 2 ** 63 and ev[1].endswith("false")
    cases["151 chain of 12 30-cycles: count above 2^53"] = case("chain12", "gds.countSpanningTrees.stream", {"orientation": "UNDIRECTED"},
                                                                kinds=["residue", "crt"], want=ev, rows=[[det]], source="count_ref; exact 30^12")
    G["chain13"] = cycle_chain(13, 30)
    ev, det, _ = count_ref(G["chain13"], 0, "UNDIRECTED")
    assert det == 30 ** 13 and det > 2 ** 63
    cases["151 chain of 13 30-cycles: above INTEGER"] = case("chain13", "gds.countSpanningTrees.stream", {"orientation": "UNDIRECTED"},
                                                             kinds=["residue", "crt"], want=ev, source="count_ref; 30^13 > 2^63 - 1: 22003",
                                                             error="22003")
    # 153.
    st = G["st"]
    for label, cfg, kw in (("153", {"relationshipWeightProperty": "weight"}, {}),
                           ("153 rerouting", {"relationshipWeightProperty": "weight", "applyRerouting": True}, dict(reroute=True)),
                           ("153 unweighted", {}, dict(weighted=False))):
        ev, rows = steiner_ref(st, 0, [3, 7, 8, 6], **kw)
        cases[label] = case("st", "gds.steinerTree.stream", dict(cfg, sourceNode=N(0), targetNodes=[N(3), N(7), N(8), N(6)]),
                            kinds=["round", "join", "reroute", "cut"], want=ev, concurrency=par, rows=rows,
                            source="GDS ShortestPathsSteinerAlgorithm.java, SimpleRerouter.java; trees.rs:47-65")
    assert any(e.startswith("reroute:") for e in cases["153 rerouting"]["want"])
    # 154.
    for label, gname in (("154", "pc"), ("154 best node below the root (GDS returns no node)", "pp")):
        ev, rows, _ = pcst_ref(G[gname])
        cases[label] = case(gname, "gds.prizeSteinerTree.stream", {"prizeProperty": "prize", "relationshipWeightProperty": "weight"},
                            kinds=["negative", "deactivate", "merge", "leaf", "best"], want=ev, rows=rows,
                            source="pcst_fast.cc; GDS GrowthPhase.java grow, StrongPruning.java performPruning")
    assert cases["154 best node below the root (GDS returns no node)"]["rows"] == []  # root only: no row
    # 155, 157.
    for label, srcs, tgts in (("155", [0], [7]), ("155 two sources", [0, 1], [6, 7])):
        net = network(G["fl"], srcs, tgts)
        ev = push_relabel_ref(net)
        rows = flow_rows(G["fl"], net)
        D = nx.DiGraph()
        for a, b, c, _ in FL8:
            if a != b:
                D.add_edge(a, b, capacity=D[a][b]["capacity"] + c if D.has_edge(a, b) else c)
        for x in srcs:
            D.add_edge("s", x, capacity=math.inf)
        for x in tgts:
            D.add_edge(x, "t", capacity=math.inf)
        into_sink = sum(net["res"][a ^ 1] for a in range(len(net["head"])) if a % 2 == 0 and net["head"][a] == net["n"] + 1)
        assert abs(nx.maximum_flow_value(D, "s", "t") - into_sink) < 1e-9, label
        cases[label] = case("fl", "gds.maxFlow.stream", {"sourceNodes": [N(x) for x in srcs], "targetNodes": [N(x) for x in tgts],
                                                          "capacityProperty": "capacity"},
                            kinds=["init", "push", "height"], want=ev, concurrency=par, rows=rows,
                            source="push_relabel_ref; Goldberg-Tarjan 1988; flow.rs:13-29")
    for label, srcs, tgts in (("157", [0], [7]), ("157 two sources", [0, 1], [6, 7])):
        net = network(G["fl"], srcs, tgts, cost=True)
        ev = ssp_ref(net)
        rows = flow_rows(G["fl"], net)
        D = nx.DiGraph()
        for a, b, c, k in FL8:
            if a == b or c <= 0:
                continue
            if D.has_edge(a, b):  # a parallel edge: split by a middle node
                D.add_edge(a, (a, b, k), capacity=c, weight=k)
                D.add_edge((a, b, k), b, capacity=c, weight=0)
            else:
                D.add_edge(a, b, capacity=c, weight=k)
        for x in srcs:
            D.add_edge("s", x, capacity=1e9, weight=0)
        for x in tgts:
            D.add_edge(x, "t", capacity=1e9, weight=0)
        fd = nx.max_flow_min_cost(D, "s", "t")
        want_cost = nx.cost_of_flow(D, fd)
        got_cost = sum(net["res"][2 * e + 1] * net["cost"][2 * e] for e in range(net["m"]) if net["res"][2 * e + 1] > 0.0)
        assert abs(want_cost - got_cost) < 1e-9, (want_cost, got_cost)
        cases[label] = case("fl", "gds.maxFlow.minCost.stream", {"sourceNodes": [N(x) for x in srcs], "targetNodes": [N(x) for x in tgts],
                                                                  "capacityProperty": "capacity", "costProperty": "cost"},
                            kinds=["potential", "augment"], want=ev, rows=rows,
                            source="NetworKit flow/SuccessiveShortestPath.cpp run, dijkstraOnResidualGraph")
    # 161.
    ev, mate = greedy_matching_ref(G["ug"])
    cases["161 greedy"] = case("ug", "gds.maximumMatching.stream", {"greedy": True}, kinds=["graph", "mate"], want=ev,
                               rows=[[N(v), N(m)] for v, m in enumerate(mate) if m is not None and v < m],
                               source="petgraph matching.rs:211 greedy_matching, :250 non_backtracking_dfs")
    Gm = nx.Graph()
    Gm.add_edges_from((a, b) for a, b, *_ in G["ug"]["edges"] if a != b)
    cases["161"] = case("ug", "gds.maximumMatching.stream", {}, kinds=["graph"], want=ev,
                        source="petgraph matching.rs:373 maximum_matching (Gabow); size from NetworkX max_weight_matching",
                        matching_size=len(nx.max_weight_matching(Gm, maxcardinality=True)))
    # 163.
    ev, size = bipartite_ref(G["bp"])
    cases["163"] = case("bp", "bipartite_matching.max", {}, kinds=["colour", "visit", "search"], want=ev, rows=[[size]],
                        source="MAGE bipartite_matching.cpp BipartiteMatchingDFS, IsSubgraphBipartiteColoring")
    cases["163 odd cycle"] = case("ug5", "bipartite_matching.max", {}, kinds=[], want=[], rows=[[0]], source="MAGE: not bipartite gives 0")
    # 169.
    ev, cl = bk_ref(G["ug"])
    cases["169"] = case("ug", "gds.maximalCliques.stream", {}, kinds=["pivot", "clique"], want=ev,
                        rows=[[i, [N(v) for v in c]] for i, c in enumerate(cl)], source="petgraph maximal_cliques.rs bron_kerbosch_pivot")
    # 171.
    ev, per, glob = clique_count_ref(G["ug"])
    cases["171"] = case("ug", "gds.cliqueCounting.stream", {}, kinds=["cq"], want=ev, concurrency=par,
                        rows=[[N(v), c] for v, c in enumerate(per)], source="clique_count_ref; NetworkX enumerate_all_cliques (asserted)")
    # 173.
    for name in ("largest_first", "random_sequential", "smallest_last", "independent_set", "connected_sequential_bfs",
                 "connected_sequential_dfs", "saturation_largest_first"):
        seed = 5 if name == "random_sequential" else 0
        ev, col = coloring_ref(G["ug"], name, seed)
        cfg = {"randomSeed": seed} if seed else {}
        cases[f"173 {name}"] = case("ug", "graph_coloring.color_graph", cfg, args=[{"strategy": name}], kinds=["order", "colour"], want=ev,
                                    rows=[[N(v), c] for v, c in enumerate(col)], source="NetworkX greedy_coloring.py strategy_" + name)
    # 174.
    ev, col = dsatur_ref(G["ug"])
    cases["174"] = case("ug", "gds.dsaturColoring.stream", {}, kinds=["dsatur"], want=ev, rows=[[N(v), c] for v, c in enumerate(col)],
                        source="petgraph coloring.rs:63 dsatur_coloring; std BinaryHeap")
    # 176.
    ev, col = jp_ref(G["ug"])
    cases["176"] = case("ug", "gds.k1coloring.stream", {}, kinds=["round"], want=ev, concurrency=par,
                        rows=[[N(v), c] for v, c in enumerate(col)], source="Jones-Plassmann 1993; LAGraph_coloring_independent_set.c")
    ev, col = jp_ref(G["cc"])
    cases["176 directed: both directions"] = case("cc", "gds.k1coloring.stream", {}, kinds=["round"], want=ev, concurrency=par,
                                                  rows=[[N(v), c] for v, c in enumerate(col)], source="jp_ref")
    # 177.
    ev, arcs = fas_ref(G["cc"])
    cases["177"] = case("cc", "gds.greedyFeedbackArcSet.stream", {}, kinds=["graph", "arcs"], want=ev,
                        source="petgraph feedback_arc_set.rs good_node_sequence")
    ev, arcs = fas_ref(G["st"])
    cases["177 st"] = case("st", "gds.greedyFeedbackArcSet.stream", {}, kinds=["graph", "arcs"], want=ev,
                           source="petgraph feedback_arc_set.rs")
    # 178.
    elements = [0, 1, 2, 0, 3, 3, 1, 2]
    sets = [4, 4, 5, 5, 5, 6, 6, 6]
    ev, cov = cover_ref(elements, sets)
    cases["178"] = case("bp", "set_cover.greedy", {}, args=[[N(e) for e in elements], [N(s) for s in sets]], kinds=["pick"], want=ev,
                        rows=[[N(s)] for s in cov], source="Johnson 1974 greedy; MAGE python/set_cover.py pairing")
    # Empty projection: no row (every pass C procedure).
    for proc, cfg in (("gds.wcc.stream", {}), ("gds.connectedComponents.stream", {}), ("gds.scc.stream", {}),
                      ("gds.condensation.stream", {}), ("biconnected_components.get", {}), ("gds.articulationPoints.stream", {}),
                      ("gds.bridges.stream", {}), ("gds.kcore.stream", {}), ("gds.triangleCount.stream", {}), ("gds.triangles", {}),
                      ("gds.localClusteringCoefficient.stream", {}), ("gds.eccentricity.stream", {}),
                      ("gds.laplacianMatrix.stream", {}), ("gds.maximumMatching.stream", {}),
                      ("gds.maximalCliques.stream", {}), ("gds.cliqueCounting.stream", {}), ("gds.dsaturColoring.stream", {}),
                      ("gds.k1coloring.stream", {}), ("gds.greedyFeedbackArcSet.stream", {}), ("graph_coloring.color_graph", {})):
        cases[f"{proc} empty"] = case("nil", proc, cfg, kinds=[], want=[], rows=[], source="TS-25: no row on an empty projection")
    cases["bipartite_matching.max empty"] = case("nil", "bipartite_matching.max", {}, kinds=[], want=[], rows=[[0]], source="MAGE")
    # One node with a self-loop.
    one = G["one"]
    for label, proc, cfg, rows in (("083", "gds.wcc.stream", {}, [[N(0), 0]]), ("093", "gds.kcore.stream", {}, [[N(0), 2]]),
                                   ("120", "gds.triangleCount.stream", {}, [[N(0), 0]]),
                                   ("121", "gds.localClusteringCoefficient.stream", {}, [[N(0), 0.0]]),
                                   ("128", "gds.girth.stream", {}, [[1.0]]), ("090", "gds.articulationPoints.stream", {}, []),
                                   ("091", "gds.bridges.stream", {}, []), ("176", "gds.k1coloring.stream", {}, [[N(0), 0]]),
                                   ("171", "gds.cliqueCounting.stream", {}, [[N(0), []]]),
                                   ("151", "gds.countSpanningTrees.stream", {"orientation": "UNDIRECTED"}, [[1]]),
                                   ("163", "bipartite_matching.max", {}, [[0]]),
                                   ("143", "gds.laplacianMatrix.stream", {}, [[N(0), [0.0]]])):
        cases[f"{label} one node"] = case("one", proc, cfg, kinds=[], want=[], rows=rows, source="definition")
    # Blossom.
    blossom = []
    for maxcard, edges in BLOSSOM_CASES:
        n = max(max(a, b) for a, b, _ in edges) + 1
        ev, pairs, ids = blossom_ref(n, edges, maxcard)
        assert all(i > n for i in ids)
        blossom.append(dict(n=n, edges=edges, maxcard=maxcard, want=ev, pairs=pairs,
                            source="NetworkX 3.7 matching.py max_weight_matching (instrumented); rustworkx max_weight_matching.rs"))
    graphs = {k: v for k, v in GRAPHS.items() if v is not None}
    write_fl8()
    analysis = dict(kcore=kcore_analysis())
    json.dump(dict(graphs=graphs, cases=cases, blossom=blossom, analysis=analysis), open(OUT, "w"), indent=1)
    n_ev = sum(len(c["want"]) for c in cases.values()) + sum(len(b["want"]) for b in blossom)
    print(f"{len(cases)} cases, {len(blossom)} blossom cases, {n_ev} events written to {os.path.normpath(OUT)}")
    print(json.dumps(analysis, indent=1))


if __name__ == "__main__":
    main()
