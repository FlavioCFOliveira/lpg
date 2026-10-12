#!/usr/bin/env python3
"""Reference executions of rmp #71 pass A (the entries of tasks #64 and #69).

Each function below re-executes, on a small trace graph, the operations of
the algorithm's reference, step by step, and emits the events in the format
of lpg's test trace (`src/algo/trace.rs`): "kind: text". The source of each
reference is cited in its docstring (file:line at the commit the sprint
studied). The script writes `src/algo/testdata/expected/trace_a.json`: the
trace graphs and, per case, the procedure call and the expected events.
`src/algo/tests_trace_a.rs` runs every case on lpg and compares.

Run: /tmp/lpg-lab-venv/bin/python -I docs/evidence/lab-sprint-5/trace_ref_a.py
(NetworkX 3.7 in the venv; used only where a case says so.)

Conventions shared with lpg's trace: nodes are dense indexes (node i is
`uid` i); edge i of a graph's list has element identifier n + i (checked by
`TraceGraph`); a node's outgoing edges are in adjacency-tree order (edge type
token, neighbour, edge identifier; `src/storage/graph.rs` `adj_key`), its
incoming edges likewise; under UNDIRECTED a node's edges are its outgoing
row, then its incoming row (`src/algo/projection.rs` `out`, `inc`).
"""

import csv
import heapq
import json
import math
import os
import sys
from collections import deque

import networkx as nx

OUT = os.path.join(os.path.dirname(__file__), "../../../src/algo/testdata/expected/trace_a.json")

# ---------------------------------------------------------------- graphs

GRAPHS = {
    # Directed, weighted, tie-free shortest paths from 0; a parallel edge
    # (9), a self-loop (8), a cycle 3→4→5→3, node 6 only reaches 0.
    "dir": dict(n=7, edges=[
        (0, 1, "R", 4.0), (0, 2, "R", 1.0), (2, 1, "R", 2.0), (1, 3, "R", 1.0),
        (2, 3, "R", 5.0), (3, 4, "R", 3.0), (1, 4, "R", 7.0), (4, 5, "R", 1.0),
        (2, 2, "R", 1.0), (0, 2, "R", 3.0), (5, 3, "R", 2.0), (6, 0, "R", 1.0)]),
    # Negative weights, no negative cycle (the cycle 2→1→3→4→2 weighs 1).
    "neg": dict(n=6, edges=[
        (0, 1, "R", 4.0), (0, 2, "R", 2.0), (2, 1, "R", -3.0), (1, 3, "R", 2.0),
        (2, 3, "R", 5.0), (3, 4, "R", -1.0), (4, 2, "R", 3.0), (5, 0, "R", 1.0)]),
    # As "neg" with 4→2 at -2: the cycle 2→1→3→4→2 weighs -4.
    "negcyc": dict(n=6, edges=[
        (0, 1, "R", 4.0), (0, 2, "R", 2.0), (2, 1, "R", -3.0), (1, 3, "R", 2.0),
        (2, 3, "R", 5.0), (3, 4, "R", -1.0), (4, 2, "R", -2.0), (5, 0, "R", 1.0)]),
    # A negative self-loop, a negative cycle of one edge.
    "selfneg": dict(n=3, edges=[(0, 1, "R", 1.0), (1, 1, "R", -1.0), (1, 2, "R", 1.0)]),
    # A DAG with two sources (0, 5) and a parallel edge (8).
    "dag": dict(n=6, edges=[
        (0, 1, "R", 3.0), (0, 2, "R", 1.0), (1, 3, "R", 2.0), (2, 3, "R", 4.0),
        (3, 4, "R", 1.0), (0, 3, "R", 1.0), (1, 4, "R", 5.0), (5, 4, "R", 2.0),
        (0, 1, "R", 2.0)]),
    # Typed edges for MAGE's direction filters.
    "typed": dict(n=4, edges=[
        (0, 1, "A", None), (1, 2, "B", None), (2, 0, "A", None), (3, 1, "B", None),
        (1, 3, "A", None), (0, 3, "B", None), (0, 2, "A", None)],
        nodes=[":N&X", ":N", ":N&Y", ":N&X"]),
    # Undirected use: triangles 0-1-2 and 2-3-4, chord 1-3, a parallel
    # edge 0-1 and a self-loop at 4.
    "und": dict(n=5, edges=[
        (0, 1, "R", None), (1, 2, "R", None), (2, 0, "R", None), (2, 3, "R", None),
        (3, 4, "R", None), (4, 2, "R", None), (1, 3, "R", None), (1, 0, "R", None),
        (4, 4, "R", None)]),
    # Points with coordinates (degrees) for 187 and 188.
    "geo": dict(n=6, edges=[], nodes=[
        ":N {lat: 3.87e1, lng: -9.14e0,", ":N {lat: 4.11e1, lng: -8.61e0,",
        ":N {lat: 3.70e1, lng: -7.93e0,", ":N {lat: 4.02e1, lng: -8.41e0,",
        ":N {lat: 3.85e1, lng: -7.91e0,", ":N {lat: 4.15e1, lng: -8.42e0,"]),
    # Isomorphism: P (0-3) and Q (4-7) isomorphic with the parallel edge;
    # U (8-11) as P without it; S (12-14) a directed path.
    "iso": dict(n=15, nodes=[":P"] * 4 + [":Q"] * 4 + [":U"] * 4 + [":S"] * 3, edges=[
        (0, 1, "R", None), (1, 2, "R", None), (2, 3, "R", None), (3, 0, "R", None),
        (0, 1, "R", None), (0, 2, "R", None),
        (4, 6, "R", None), (6, 5, "R", None), (5, 7, "R", None), (7, 4, "R", None),
        (4, 6, "R", None), (4, 5, "R", None),
        (8, 9, "R", None), (9, 10, "R", None), (10, 11, "R", None), (11, 8, "R", None),
        (8, 10, "R", None),
        (12, 13, "R", None), (13, 14, "R", None)]),
    # Tournaments: T (0-3) transitive, C (4-6) a 3-cycle, X (7-9) three
    # edges but a pair joined both ways.
    "tour": dict(n=10, nodes=[":T"] * 4 + [":C"] * 3 + [":X"] * 3, edges=[
        (0, 1, "R", None), (0, 2, "R", None), (0, 3, "R", None), (1, 2, "R", None),
        (1, 3, "R", None), (2, 3, "R", None),
        (4, 5, "R", None), (5, 6, "R", None), (6, 4, "R", None),
        (7, 8, "R", None), (8, 7, "R", None), (8, 9, "R", None)]),
    # Two bipartite components: the square 0-1-2-3 and the edge 4-5.
    "bip": dict(n=6, edges=[(0, 1, "R", None), (2, 1, "R", None), (2, 3, "R", None),
                            (0, 3, "R", None), (5, 4, "R", None)]),
}


def adjacency(g, orientation="NATURAL"):
    """out[v], inc[v]: lists of (other end, edge id) in lpg's order."""
    n = g["n"]
    types = []
    for e in g["edges"]:
        if e[2] not in types:
            types.append(e[2])
    fwd = [[] for _ in range(n)]
    rev = [[] for _ in range(n)]
    for i, (a, b, t, _) in enumerate(g["edges"]):
        fwd[a].append((types.index(t), b, n + i))
        rev[b].append((types.index(t), a, n + i))
    fwd = [[(x[1], x[2]) for x in sorted(r)] for r in fwd]
    rev = [[(x[1], x[2]) for x in sorted(r)] for r in rev]
    if orientation == "NATURAL":
        return fwd, rev
    if orientation == "REVERSE":
        return rev, fwd
    both = [fwd[v] + rev[v] for v in range(n)]
    return both, both


def weight(g, eid):
    return g["edges"][eid - g["n"]][3]


def targets(out, v):
    """lpg `kernels::targets`: distinct targets, ascending (GDS order)."""
    return sorted({w for w, _ in out[v]})


# ------------------------------------------------------- traversal (#64)

def bfs_gds(g, s, max_depth=None, targets_of=()):
    """GDS @ 417d7ecf973d BFS.java `compute` and BFSTask.java `run`,
    `relaxNode`, `syncNextChunk`: per level, stop at `maximumDepth`; the
    exit predicate is tested on each node of the level (targetFoundIndex is
    the smallest position of a target, result cut after it); the new nodes
    are appended in order of (position of the predecessor with the
    smallest position, then its adjacency order). Events: lpg's block
    order lists the expanded nodes of a level by node index."""
    out, _ = adjacency(g)
    ev = []
    order, visited, start, depth = [s], {s}, 0, 0
    while True:
        if max_depth is not None and depth == max_depth:
            ev.append(f"stop: maxDepth {depth}")
            break
        level = order[start:]
        ev.append(f"level: {depth} {level}")
        hit = [i for i, v in enumerate(level) if v in targets_of]
        if hit:
            ev.append(f"stop: target {level[hit[0]]}")
            order = order[:start + hit[0] + 1]
            break
        for pos, v in sorted(((p, v) for p, v in enumerate(level)), key=lambda x: x[1]):
            ev.append(f"expand: {v} at {pos}")
        nxt = len(order)
        for v in level:
            for w in targets(out, v):
                if w not in visited:
                    visited.add(w)
                    order.append(w)
        ev.append(f"append: {order[nxt:]}")
        if len(order) == nxt:
            ev.append("stop: empty")
            break
        start, depth = nxt, depth + 1
    return ev, order


def dfs_gds(g, s, max_depth=None, targets_of=()):
    """GDS @ 417d7ecf973d DFS.java `compute`: a stack; a node is marked
    visited when pushed; its targets pushed in adjacency order; the exit
    predicate (TargetExitPredicate) tested on each popped node; neighbours
    expanded while the depth (OneHopAggregator weight) is below maxDepth."""
    out, _ = adjacency(g)
    ev, res, st, vis = [], [], [(s, 0)], {s}
    while st:
        v, d = st.pop()
        ev.append(f"pop: {v} {d}")
        res.append(v)
        if v in targets_of:
            ev.append(f"stop: target {v}")
            break
        if max_depth is None or d < max_depth:
            for w in targets(out, v):
                if w not in vis:
                    ev.append(f"push: {w} {d + 1}")
                    vis.add(w)
                    st.append((w, d + 1))
    return ev, res


def hop_levels(g, start, rel_types, distance, stop_empty):
    """MAGE @ c72636014a8b cpp/neighbors_module/algorithm/neighbors.cpp
    `DetermineDirection`, `AtHop` (stops at an empty level), `ByHop`: each
    level holds the nodes reached from the last level over admitted
    incoming then outgoing relationships and not known in an earlier
    level (a set; lpg lists it in index order)."""
    out, inc = adjacency(g)
    if not rel_types:
        rel_types = [""]
    ins, outs = set(), set()
    for t in rel_types:
        if t.startswith("<"):
            ins.add(t[1:])
        elif t.endswith(">"):
            outs.add(t[:-1])
        else:
            ins.add(t)
            outs.add(t)
    ty = lambda eid: g["edges"][eid - g["n"]][2]
    levels, known, ev = [[start]], {start}, []
    while len(levels) <= distance:
        new = set()
        for v in levels[-1]:
            if ins:
                for u, e in inc[v]:
                    if ("" in ins or ty(e) in ins) and u not in known:
                        new.add(u)
            if outs:
                for w, e in out[v]:
                    if ("" in outs or ty(e) in outs) and w not in known:
                        new.add(w)
        ev.append(f"level: {len(levels)} {sorted(new)}")
        if not new and stop_empty:
            return ev
        known |= new
        levels.append(sorted(new))
    return ev


class PathHelper:
    """MAGE @ c72636014a8b cpp/path_module/algorithm/path.cpp
    `PathHelper` (labels, relationship directions, AreLabelsValid:86,
    ContinueExpanding:93, PathSizeOk:100, PathTooBig:104) and path.hpp
    `IsNotStartOrFiltersStartNode`, `IsNotStartOrFilterStartRel`:111-112."""

    def __init__(self, labels, rels, min_hops=0, max_hops=math.inf):
        self.term, self.black, self.white, self.end = set(), set(), set(), set()
        for l in labels:
            c = l[0]
            {"-": self.black, ">": self.end, "+": self.white, "/": self.term}.get(c, self.white).add(
                l[1:] if c in "->+/" else l)
        self.rels, self.any_in, self.any_out = {}, False, False
        if not rels:
            self.any_in = self.any_out = True
        for t in rels:
            s, e = t.startswith("<"), t.endswith(">")
            if len(t) == 1:
                if s:
                    self.any_in = True
                elif e:
                    self.any_out = True
                else:
                    self.rels[t] = "any"
                continue
            if s and e:
                self.rels[t[1:-1]] = "both"
            elif s:
                self.rels[t[1:]] = "in"
            elif e:
                self.rels[t[:-1]] = "out"
            else:
                self.rels[t] = "any"
        self.min_hops, self.max_hops = min_hops, max_hops
        self.filter_start_node, self.begin_at_start = True, True

    def bools(self, labels):
        return dict(black=bool(labels & self.black), term=bool(labels & self.term),
                    end=bool(labels & self.end), white=bool(labels & self.white))

    def whitelisted(self, w):
        return not self.white or w

    def valid(self, b):
        return not b["black"] and ((b["end"] and bool(self.end)) or b["term"] or (
            not self.term and not self.end and self.whitelisted(b["white"])))

    def cont(self, b, size):
        return size <= self.max_hops and ((not b["black"] and not b["term"] and (
            b["end"] or self.whitelisted(b["white"]))) or (size == 1 and not self.filter_start_node))

    def any_dir(self, outgoing):
        return self.any_out if outgoing else self.any_in


def node_labels(g, v):
    spec = g.get("nodes", [":N"] * g["n"])[v]
    return set(x for x in spec.split("{")[0].strip().replace("&", ":").split(":") if x)


def path_expand(g, starts, rels, labels, min_hops, max_hops):
    """MAGE path.cpp `PathExpand::DFS`:359, `ExpandFromRelationships`:331,
    `ExpandPath`:323: a depth-first search of trails (a relationship once
    per path), incoming relationships first, then outgoing; a path is
    returned when its size is within the hops and its last node's labels
    are valid."""
    out, inc = adjacency(g)
    h = PathHelper(labels, rels, min_hops, max_hops)
    ty = lambda eid: g["edges"][eid - g["n"]][2]
    ev = []

    def dfs(trail, used):
        v, size = trail[-1], (len(trail) - 1) // 2
        b = h.bools(node_labels(g, v))
        ev.append(f"enter: {trail}")
        if h.min_hops <= size <= h.max_hops and h.valid(b):
            ev.append(f"emit: {trail}")
        if not h.cont(b, size + 1):
            return
        seen = set()
        for outgoing, adj in ((False, inc[v]), (True, out[v])):
            for w, e in adj:
                wanted = h.rels.get(ty(e), "none")
                if (wanted == "none" and not h.any_dir(outgoing)) or e in used:
                    continue
                cur = "out" if outgoing else "in"
                take = wanted == "any" or cur == wanted or h.any_dir(outgoing)
                if not take and wanted == "both":
                    src = v if outgoing else w
                    if outgoing and (ty(e), w) in seen:
                        take = True
                    else:
                        seen.add((ty(e), src))
                if take:
                    dfs(trail + [e, w], used | {e})

    for s in sorted(set(starts)):
        dfs([s], frozenset())
    return ev


def path_subgraph(g, starts, rels, labels, min_hops=0, max_hops=math.inf):
    """MAGE path.cpp `PathSubgraph::BFS`:488, `TryInsertNode`:477,
    `ExpandFromRelationships`:439: a FIFO queue of (node, hops) from the
    start nodes, a node visited once; incoming relationships first."""
    out, inc = adjacency(g)
    h = PathHelper(labels, rels, min_hops, max_hops)
    ty = lambda eid: g["edges"][eid - g["n"]][2]
    ev, q, visited, res = [], deque(), set(), []
    for s in sorted(set(starts)):
        q.append((s, 0))
        visited.add(s)
    while q:
        v, hops = q.popleft()
        ev.append(f"pop: {v} {hops}")
        if hops > h.max_hops:
            continue
        b = h.bools(node_labels(g, v))
        if h.filter_start_node or hops != 0:
            if h.valid(b):
                res.append(v)
        else:
            res.append(v)
        if not h.cont(b, hops + 1):
            continue
        seen = set()
        for outgoing, adj in ((False, inc[v]), (True, out[v])):
            for w, e in adj:
                if w in visited:
                    continue
                wanted = h.rels.get(ty(e), "none")
                if (h.begin_at_start or hops != 0) and wanted == "none" and not h.any_dir(outgoing):
                    continue
                cur = "out" if outgoing else "in"
                if wanted == "any" or cur == wanted or h.any_dir(outgoing):
                    ev.append(f"push: {w} {hops + 1}")
                    visited.add(w)
                    q.append((w, hops + 1))
                elif wanted == "both":
                    if outgoing and (ty(e), w) in seen:
                        ev.append(f"push: {w} {hops + 1} both")
                        visited.add(w)
                        q.append((w, hops + 1))
                        res.append(w)
                    else:
                        seen.add((ty(e), v if outgoing else w))
    return ev, res


def kahn_levels(g):
    """Kahn (1962) by levels; NetworkX 3.7 `topological_generations`
    (networkx/algorithms/dag.py) yields the same generations: level 0 the
    nodes of in-degree 0, level k+1 the nodes whose in-degree reaches 0
    once level k is removed. Each level listed in index order."""
    G = nx.MultiDiGraph()
    G.add_nodes_from(range(g["n"]))
    G.add_edges_from((a, b) for a, b, _, _ in g["edges"])
    ev, levels = [], []
    try:
        gens = [sorted(x) for x in nx.topological_generations(G)]
    except nx.NetworkXUnfeasible:
        # A cycle: Kahn's levels stop at the nodes the cycle blocks.
        indeg = {v: G.in_degree(v) for v in G}
        level = sorted(v for v in G if indeg[v] == 0)
        gens = []
        while level:
            gens.append(level)
            nxt = []
            for v in level:
                for _, w in G.out_edges(v):
                    indeg[w] -= 1
                    if indeg[w] == 0:
                        nxt.append(w)
            level = sorted(nxt)
    for k, level in enumerate(gens):
        ev.append(f"level: {k} {level}")
    return ev, gens


def bfs_reach(g, s, forward):
    """NetworkX 3.7 `descendants`/`ancestors` (networkx/algorithms/dag.py)
    are the nodes of `bfs_edges` from s (on G or its reverse,
    networkx/algorithms/traversal/breadth_first_search.py `generic_bfs_edges`):
    each new node in FIFO order, neighbours in adjacency order."""
    out, inc = adjacency(g)
    adj = out if forward else inc
    ev, seen, q = [], {s}, deque([s])
    while q:
        v = q.popleft()
        for w, _ in adj[v]:
            if w not in seen:
                ev.append(f"visit: {w} from {v}")
                seen.add(w)
                q.append(w)
    return ev


def dominance(g, root):
    """Immediate dominators from NetworkX 3.7 `immediate_dominators`
    (networkx/algorithms/dominance.py, Cooper, Harvey and Kennedy 2006,
    the algorithm petgraph's `simple_fast` implements), and the frontier
    climbs of `dominance_frontiers` (same file): for u the root or with two
    or more distinct predecessors, each reached predecessor v climbs the
    dominator tree until idom[u], adding u to the frontier of each node it
    passes. Order: u ascending, predecessors ascending (lpg's loop)."""
    G = nx.MultiDiGraph()
    G.add_nodes_from(range(g["n"]))
    G.add_edges_from((a, b) for a, b, _, _ in g["edges"])
    idom = nx.immediate_dominators(G, root)
    idom.pop(root, None)
    ev = [f"idom: {v} {idom[v]}" for v in sorted(idom)]
    full = dict(idom)
    full[root] = None
    for u in sorted(full):
        preds = sorted(set(G.predecessors(u)))
        if u == root or len(preds) >= 2:
            for v in preds:
                if v not in full:
                    continue
                while v != full[u]:
                    ev.append(f"frontier: {v} gets {u}")
                    if v == root:
                        break
                    v = full[v]
    return ev


# ---------------------------------------------------------- paths (#64)

def bfs_tree(g, s, forward=True):
    """A breadth-first tree (Moore 1959; NetworkX `bfs_predecessors`):
    each node discovered from the first dequeued node with an edge to it,
    in adjacency order."""
    out, inc = adjacency(g)
    adj = out if forward else inc
    ev, dist, q = [], {s: 0}, deque([s])
    while q:
        v = q.popleft()
        for w, _ in adj[v]:
            if w not in dist:
                ev.append(f"visit: {w} from {v}")
                dist[w] = dist[v] + 1
                q.append(w)
    return ev, dist


def dijkstra(g, s, orientation="NATURAL", weighted=True, stop=None):
    """Dijkstra (1959) as rustworkx-core @ 25398ab47f66
    src/shortest_path/dijkstra.rs (forked from petgraph): a binary heap of
    (score, node), a popped node already visited is skipped, edges to
    visited nodes are skipped, a neighbour is pushed on strict improvement.
    GDS's Dijkstra (paths/dijkstra/Dijkstra.java) settles the same nodes in
    the same order when no two tentative distances tie."""
    out, _ = adjacency(g, orientation)
    dist, visited, heap, ev = {s: 0.0}, set(), [(0.0, s)], []
    while heap:
        d, u = heapq.heappop(heap)
        if u in visited:
            continue
        ev.append(f"pop: {u} {d!r}")
        if u == stop:
            break
        for w, e in out[u]:
            if w in visited:
                continue
            nxt = d + (weight(g, e) if weighted else 1.0)
            if nxt < dist.get(w, math.inf):
                ev.append(f"relax: {w} {nxt!r} {u} {e}")
                dist[w] = nxt
                heapq.heappush(heap, (nxt, w))
        visited.add(u)
    return ev, dist


def bidirectional(g, s, t):
    """petgraph 0.8.3 src/algo/dijkstra.rs `bidirectional_dijkstra`:287-407:
    each iteration pops one node from each heap (stale entries included),
    marks both visited, relaxes the forward node's outgoing and the backward
    node's incoming edges, updates the best meeting cost on an edge to a
    node the other side has visited, and stops when d(u) + d(v) ≥ best."""
    out, inc = adjacency(g)
    fd, bd = {s: 0.0}, {t: 0.0}
    fv, bv = set(), set()
    fh, bh = [(0.0, s)], [(0.0, t)]
    best, ev = None, []
    while fh and bh:
        _, u = heapq.heappop(fh)
        _, v = heapq.heappop(bh)
        fv.add(u)
        bv.add(v)
        du, dv = fd[u], bd[v]
        ev.append(f"pop: {u} {du!r} {v} {dv!r}")
        for x, e in out[u]:
            c = weight(g, e)
            if x not in fv and du + c < fd.get(x, math.inf):
                fd[x] = du + c
                heapq.heappush(fh, (du + c, x))
            if x in bv:
                total = du + c + bd[x]
                if best is None or total < best:
                    ev.append(f"best: {total!r} {u} {x}")
                    best = total
        for x, e in inc[v]:
            c = weight(g, e)
            if x not in bv and dv + c < bd.get(x, math.inf):
                bd[x] = dv + c
                heapq.heappush(bh, (dv + c, x))
            if x in fv:
                total = dv + c + fd[x]
                if best is None or total < best:
                    ev.append(f"best: {total!r} {x} {v}")
                    best = total
        if best is not None and du + dv >= best:
            ev.append(f"stop: {du + dv!r} >= {best!r}")
            break
    return ev


def bellman_rounds(g, sources, weighted=True):
    """The round formulation of Bellman-Ford (Bellman 1958, "On a routing
    problem"; CLRS 3rd ed. §24.1 with all edges relaxed from the previous
    round's distances): d_k(v) = min(d_{k-1}(v), min over edges (u, v) of
    d_{k-1}(u) + w(u, v)); at most n rounds; a change in round n means a
    negative cycle. The sequential reference of lpg's synchronous rounds
    (TS-25 Parallelism)."""
    n = g["n"]
    _, inc = adjacency(g)
    cur = [math.inf] * n
    for s in (sources or range(n)):
        cur[s] = 0.0
    ev = []
    for k in range(1, n + 1):
        nxt = list(cur)
        changed = []
        for v in range(n):
            for u, e in inc[v]:
                if cur[u] == math.inf:
                    continue
                c = cur[u] + (weight(g, e) if weighted else 1.0)
                if c < nxt[v]:
                    nxt[v] = c
                    if v not in changed:
                        changed.append(v)
        cur = nxt
        ev.append(f"round: {k} {[float(x) for x in cur]}")
        if not changed:
            break
        if k == n:
            ev.append(f"stop: changing {sorted(changed)}")
    return ev, cur


def delta_stepping(g, s, delta):
    """Delta-stepping (Meyer and Sanders 2003, "Δ-stepping: a parallelizable
    shortest path algorithm", Fig. 1): for the smallest non-empty bucket i,
    repeatedly remove its nodes, add them to R, relax their light edges
    (w ≤ Δ); then relax the heavy edges (w > Δ) of R. A relaxation moves
    the node to bucket ⌊d/Δ⌋. Within a phase, requests are made from the
    distances before the phase and applied in node order, then edge order
    (lpg's block order); a node is in bucket i when ⌊d/Δ⌋ = i."""
    out, _ = adjacency(g)
    dist = {s: 0.0}
    buckets = {0: [s]}
    b = lambda d: math.floor(d / delta)
    ev = []
    while buckets:
        i = min(buckets)
        settled = []
        while i in buckets:
            frontier = sorted(set(buckets.pop(i)))
            frontier = [v for v in frontier if b(dist[v]) == i]
            if not frontier:
                break
            ev.append(f"light: {i} {frontier}")
            settled += frontier
            reqs = [(x, dist[v] + weight(g, e), v) for v in frontier for x, e in out[v]
                    if weight(g, e) <= delta]
            for x, d, v in reqs:
                if d < dist.get(x, math.inf):
                    ev.append(f"relax: {x} {d!r} {v}")
                    dist[x] = d
                    buckets.setdefault(b(d), []).append(x)
        settled = sorted(set(settled))
        ev.append(f"heavy: {i} {settled}")
        reqs = [(x, dist[v] + weight(g, e), v) for v in settled for x, e in out[v]
                if weight(g, e) > delta]
        for x, d, v in reqs:
            if d < dist.get(x, math.inf):
                ev.append(f"relax: {x} {d!r} {v}")
                dist[x] = d
                buckets.setdefault(b(d), []).append(x)
        buckets = {k: v for k, v in buckets.items() if k > i}
    return ev


def johnson(g):
    """Johnson (1977) as petgraph 0.8.3 src/algo/johnson.rs `johnson`,
    `johnson_reweight`: potentials h from Bellman-Ford from a virtual source
    joined to every node at cost 0 (lpg runs the rounds of 019 with every
    node a source at 0, the same distances), then one Dijkstra per source
    on w(u, v) + h(u) − h(v), distances corrected by h(t) − h(s). NetworkX
    3.7 `johnson` (networkx/algorithms/shortest_paths/weighted.py) gives the
    same distances (checked below)."""
    _, h = bellman_rounds(g, [])
    out, _ = adjacency(g)
    ev = [f"potential: {[float(x) for x in h]}"]
    n = g["n"]
    G = nx.MultiDiGraph()
    G.add_nodes_from(range(n))
    for a, b2, _, w in g["edges"]:
        G.add_edge(a, b2, weight=w)
    nxj = nx.johnson(G)
    for s in range(n):
        dist, vis, heap = [math.inf] * n, set(), [(0.0, s)]
        dist[s] = 0.0
        while heap:
            d, u = heapq.heappop(heap)
            if u in vis:
                continue
            vis.add(u)
            for x, e in out[u]:
                if x in vis:
                    continue
                nd = d + (weight(g, e) + h[u] - h[x])
                if nd < dist[x]:
                    dist[x] = nd
                    heapq.heappush(heap, (nd, x))
        dist = [d + h[t] - h[s] if d != math.inf else d for t, d in enumerate(dist)]
        for t, d in enumerate(dist):
            if d != math.inf:
                p = nxj[s][t]
                assert abs(sum(G[a][b2][0]["weight"] if len(G[a][b2]) == 1 else min(
                    x["weight"] for x in G[a][b2].values()) for a, b2 in zip(p, p[1:])) - d) < 1e-9
        ev.append(f"source: {s} {dist}")
    return ev


def apd(A):
    """graphalgs @ 1f8a412ef4af src/shortest_path/seidel.rs `apd` (Seidel
    1995): if A is complete, return it; else Z = A², B = A or Z (off the
    diagonal), T = apd(B), X = T·A, D = 2T or 2T − 1 by X ≥ T·deg."""
    n = len(A)
    ev = []
    if all(i == j or A[i][j] for i in range(n) for j in range(n)):
        return ["apd: complete"], A
    Z = [[sum(A[i][k] * A[k][j] for k in range(n)) for j in range(n)] for i in range(n)]
    B = [[1 if i != j and (A[i][j] == 1 or Z[i][j] > 0) else 0 for j in range(n)] for i in range(n)]
    sub, T = apd(B)
    ev += sub
    X = [[sum(T[i][k] * A[k][j] for k in range(n)) for j in range(n)] for i in range(n)]
    deg = [sum(r) for r in A]
    D = [[2 * T[i][j] if X[i][j] >= T[i][j] * deg[j] else 2 * T[i][j] - 1 for j in range(n)]
         for i in range(n)]
    ev.append(f"apd: {D}")
    return ev, D


def seidel(g):
    """Seidel per connected component of the undirected projection (lpg's
    modification: APD ends only on a connected graph); components found
    by a breadth-first scan from the smallest unassigned node."""
    out, _ = adjacency(g, "UNDIRECTED")
    n = g["n"]
    comp, ev = [None] * n, []
    for s in range(n):
        if comp[s] is not None:
            continue
        members, at = [s], 0
        comp[s] = s
        while at < len(members):
            v = members[at]
            at += 1
            for x, _ in out[v]:
                if comp[x] is None:
                    comp[x] = s
                    members.append(x)
        members.sort()
        ev.append(f"component: {members}")
        k = len(members)
        A = [[0] * k for _ in range(k)]
        for i, v in enumerate(members):
            for x, _ in out[v]:
                j = members.index(x)
                if i != j:
                    A[i][j] = A[j][i] = 1
        sub, _ = apd(A)
        ev += sub
    return ev


def all_pairs(g, weighted):
    """One search per source (GDS allShortestPaths: MSBFS unweighted,
    one Dijkstra per source weighted): the distance array of each source."""
    n = g["n"]
    ev = []
    for s in range(n):
        if weighted:
            _, d = dijkstra(g, s)
        else:
            _, d = bfs_tree(g, s)
        ev.append(f"source: {s} {[float(d[t]) if t in d else math.inf for t in range(n)]}")
    return ev


def all_shortest_between(g, s, t):
    """NetworkX 3.7 `all_shortest_paths` (shortest_paths/generic.py) with
    `method='dijkstra'`: predecessors p of v with d(p) + w(p, v) = d(v)
    (`dijkstra_predecessor_and_distance`), paths built from the target
    back (`_build_paths_from_predecessors`). lpg lists a node's
    predecessors ascending; the events give each node's predecessor set
    as lpg's depth-first enumeration reaches it, and the paths in that
    enumeration's order."""
    _, dist = dijkstra(g, s)
    _, inc = adjacency(g)
    preds = {}
    for v in range(g["n"]):
        best = {}
        for u, e in inc[v]:
            if u == v or u not in dist:
                continue
            best[u] = min(best.get(u, math.inf), weight(g, e))
        preds[v] = sorted(u for u, c in best.items() if v in dist and dist[u] + c == dist[v])
    G = nx.MultiDiGraph()
    G.add_nodes_from(range(g["n"]))
    for a, b, _, w in g["edges"]:
        G.add_edge(a, b, weight=w)
    nxp, _ = nx.dijkstra_predecessor_and_distance(G, s)
    for v in nxp:
        assert sorted(set(nxp[v])) == preds[v], (v, nxp[v], preds[v])
    ev, paths = [], []

    def back(v, suffix):
        if v == s:
            p = [s] + suffix
            ev.append(f"path: {p}")
            paths.append(p)
            return
        ev.append(f"preds: {v} {preds[v]}")
        for u in preds[v]:
            back(u, [v] + suffix)

    back(t, [])
    assert sorted(paths) == sorted(nx.all_shortest_paths(G, s, t, weight="weight"))
    return ev


def yens(g, s, t, k):
    """pathfinding @ cd4600b36ec0 src/directed/yen.rs `yen` with paths as
    edge sequences (lpg's documented modification, GDS's parallel-edge
    paths): for round ki the spur nodes from the route's spur index; edges
    out of the spur node used by found routes sharing the root are
    removed, as are the root's nodes; a new path (by edges) becomes a
    candidate; the cheapest candidate (cost, then length) is accepted with
    every candidate of equal cost."""
    n = g["n"]
    out, _ = adjacency(g)
    w = lambda e: weight(g, e)

    def spur_dijkstra(src, removed_nodes, removed_edges):
        dist, vis, pred, heap = {src: 0.0}, set(removed_nodes), {}, [(0.0, src)]
        while heap:
            d, u = heapq.heappop(heap)
            if u in vis:
                continue
            if u == t:
                break
            vis.add(u)
            for x, e in out[u]:
                if x in vis or e in removed_edges:
                    continue
                if d + w(e) < dist.get(x, math.inf):
                    dist[x] = d + w(e)
                    pred[x] = (u, e)
                    heapq.heappush(heap, (d + w(e), x))
        if t not in dist:
            return None
        nodes, edges, v = [t], [], t
        while v != src:
            u, e = pred[v]
            nodes.append(u)
            edges.append(e)
            v = u
        return nodes[::-1], edges[::-1], dist[t]

    first = spur_dijkstra(s, set(), set())
    if first is None:
        return []
    routes = [dict(nodes=first[0], edges=first[1], cost=first[2], spur=0)]
    visited, cands, ev = set(), [], []
    for ki in range(k - 1):
        if len(routes) <= ki or len(routes) == k:
            break
        prev = routes[ki]
        rc = [0.0]
        for e in prev["edges"]:
            rc.append(rc[-1] + w(e))
        ev.append(f"round: {ki} spurs {prev['spur']}..{len(prev['nodes']) - 1}")
        for i in range(prev["spur"], len(prev["nodes"]) - 1):
            spur = prev["nodes"][i]
            root_n, root_e = prev["nodes"][:i], prev["edges"][:i]
            rem_e = {r["edges"][i] for r in routes if len(r["edges"]) > i and r["nodes"][:i] == root_n
                     and r["edges"][:i] == root_e and r["nodes"][i] == spur}
            found = spur_dijkstra(spur, set(root_n), rem_e)
            if found is None:
                continue
            nodes, edges = root_n + found[0], root_e + found[1]
            if tuple(edges) in visited:
                continue
            visited.add(tuple(edges))
            c = dict(nodes=nodes, edges=edges, cost=rc[i] + found[2], spur=i)
            ev.append(f"candidate: {c['cost']!r} {nodes} {i}")
            cands.append(c)
        if cands:
            cands.sort(key=lambda c: (c["cost"], len(c["nodes"]), c["edges"]))
            r = cands.pop(0)
            ev.append(f"accept: {r['cost']!r} {r['nodes']}")
            routes.append(r)
            while len(routes) < k and cands and cands[0]["cost"] == r["cost"]:
                x = cands.pop(0)
                ev.append(f"accept: {x['cost']!r} {x['nodes']}")
                routes.append(x)
    return ev


def all_simple(g, s, t, rel_types, max_length, orientation="NATURAL"):
    """MAGE @ c72636014a8b cpp/algo_module/algorithm/algo.cpp
    `UpdateRelationshipDirection`:11, `DFS`:75: at the end node a path is
    returned; at max_length the search turns back; otherwise the node is
    visited, its incoming relationships (unless all_outgoing) then its
    outgoing ones (unless all_incoming) are followed to unvisited nodes
    when the type's direction admits them (`<T>` needs the incoming edge
    from that node first, in the `seen` set)."""
    out, inc = adjacency(g, orientation)
    ty = lambda eid: g["edges"][eid - g["n"]][2]
    any_in = any_out = all_in = all_out = False
    dirs = {}
    if not rel_types:
        any_in = any_out = True
    else:
        in_rel = out_rel = False
        for t2 in rel_types:
            st, en = t2.startswith("<"), t2.endswith(">")
            if len(t2) == 1:
                if st:
                    any_in = in_rel = True
                elif en:
                    any_out = out_rel = True
                else:
                    dirs[t2] = "any"
                    in_rel = out_rel = True
                continue
            if st and en:
                dirs[t2[1:-1]] = "both"
                in_rel = out_rel = True
            elif st:
                dirs[t2[1:]] = "in"
                in_rel = True
            elif en:
                dirs[t2[:-1]] = "out"
                out_rel = True
            else:
                dirs[t2] = "any"
                in_rel = out_rel = True
        if not in_rel:
            all_out = True
        elif not out_rel:
            all_in = True
    ev, visited = [], set()

    def dfs(trail):
        v = trail[-1]
        ev.append(f"enter: {trail}")
        if v == t:
            ev.append("emit: ")
            return
        if (len(trail) - 1) // 2 == max_length:
            return
        visited.add(v)
        seen = set()
        follow = []
        for outgoing, adj, active in ((False, inc[v], not all_out), (True, out[v], not all_in)):
            if not active:
                continue
            always = any_out if outgoing else any_in
            for x, e in adj:
                if x in visited:
                    continue
                wanted = dirs.get(ty(e), "none")
                cur = "out" if outgoing else "in"
                if always or wanted == "any" or wanted == cur:
                    follow.append((x, e))
                elif wanted == "both":
                    if outgoing and x in seen:
                        follow.append((x, e))
                    elif not outgoing:
                        seen.add(x)
        for x, e in follow:
            dfs(trail + [e, x])
        visited.discard(v)

    dfs([s])
    return ev


def longest(g):
    """The longest path in a DAG by topological levels (CLRS 3rd ed. §24.2,
    DAG-SHORTEST-PATHS with maximisation): each node of a level takes the
    largest d(u) + w(u, v) over its incoming edges (the first of equal
    ones in incoming order); a source has 0 and no predecessor (-1)."""
    _, levels = kahn_levels(g)
    _, inc = adjacency(g)
    d, ev = {}, []
    for level in levels:
        settled = {}
        for v in level:
            best = None
            for u, e in inc[v]:
                c = d[u] + weight(g, e)
                if best is None or c > best[0]:
                    best = (c, u)
            settled[v] = best if best is not None else (0.0, -1)
            ev.append(f"settle: {v} {settled[v][0]!r} {settled[v][1]}")
        d.update({v: x[0] for v, x in settled.items()})
    return ev


# --------------------------------------------------- cycles etc. (#69)

def mage_cycles(g):
    """MAGE @ c72636014a8b cpp/cycles_module/algorithm/cycles.cpp
    `FindNonSpanningTreeEdges`:29 (depth-first; the parent and repeated
    neighbours skipped; a visited neighbour gives a non-tree edge),
    `FindFundamentalCycle`:64, `CombineCycles`:93 for every mask, then a
    two-node cycle per parallel pair and self-loop (`GetCycles`:170)."""
    out, _ = adjacency(g, "UNDIRECTED")
    n = g["n"]
    visited, parent, depth = [False] * n, [None] * n, [0] * n
    ev = []

    def fnst(v, non_tree):
        visited[v] = True
        unique = set()
        for x, _ in out[v]:
            if parent[v] is not None and (x == parent[v] or x in unique):
                continue
            unique.add(x)
            if visited[x]:
                ev.append(f"nontree: {v} {x}")
                non_tree.add((min(v, x), max(v, x)))
                continue
            ev.append(f"tree: {v} {x}")
            parent[x] = v
            depth[x] = depth[v] + 1
            fnst(x, non_tree)

    def fundamental(a, b):
        if depth[a] < depth[b]:
            a, b = b, a
        c = [a]
        while depth[a] > depth[b]:
            a = parent[a]
            c.append(a)
        assert a == b
        c.append(c[0])
        return c

    def combine(mask, funds):
        cnt = {}
        for i, f in enumerate(funds):
            if mask & (1 << i):
                for x, y in zip(f, f[1:]):
                    k = (min(x, y), max(x, y))
                    cnt[k] = cnt.get(k, 0) + 1
        adj = {}
        for (a, b), c in sorted(cnt.items()):
            if c % 2:
                adj.setdefault(a, []).append(b)
                adj.setdefault(b, []).append(a)
        if any(len(v) != 2 for v in adj.values()) or not adj:
            return None
        cur = min(adj)
        seen, cyc = set(), []
        while cur not in seen:
            seen.add(cur)
            cyc.append(cur)
            for x in adj[cur]:
                if x not in seen:
                    cur = x
                    break
        return cyc if len(seen) == len(adj) and len(cyc) > 2 else None

    for root in range(n):
        if visited[root]:
            continue
        parent[root] = root
        non_tree = set()
        fnst(root, non_tree)
        funds = [fundamental(a, b) for a, b in sorted(non_tree)]
        ev.append(f"fundamental: {funds}")
        for mask in range(1, 1 << len(funds)):
            c = combine(mask, funds)
            if c:
                ev.append(f"cycle: {mask} {c}")
    pairs = {}
    for a, b, _, _ in g["edges"]:
        k = (min(a, b), max(a, b))
        pairs[k] = pairs.get(k, 0) + 1
    for (a, b), c in sorted(pairs.items()):
        if c > 1 or a == b:
            ev.append(f"cycle: pair {a} {b}")
    return ev


def circuits(g):
    """graphalgs @ 1f8a412ef4af src/elementary_circuits.rs
    `elementary_circuits`:71, `elementary_circuits_starting_at_s`:13: for
    each s, successors restricted to nodes above s (each target once), a
    task stack of Search and Unblock; a path is a circuit when its last
    node has an edge to s. Successors in ascending order (lpg's
    `targets`; graphalgs keeps edge order, ascending for these graphs)."""
    out, _ = adjacency(g)
    n = g["n"]
    succ = [targets(out, v) for v in range(n)]
    ev = []
    for s in range(n):
        is_pred = [s in succ[x] for x in range(n)]
        path, blocked, tasks = [], [False] * n, [("S", s)]
        while tasks:
            kind, v = tasks.pop()
            if kind == "S":
                ev.append(f"search: {v}")
                path.append(v)
                blocked[v] = True
                tasks.append(("U", v))
                if is_pred[v]:
                    ev.append(f"circuit: {path}")
                for w in succ[v]:
                    if w > s and not blocked[w]:
                        tasks.append(("S", w))
            else:
                ev.append(f"unblock: {v}")
                blocked[v] = False
                path.pop()
    G = nx.DiGraph([(a, b) for a, b, _, _ in g["edges"]])
    found = sorted(tuple(e[len("circuit: "):]) for e in ev if e.startswith("circuit"))
    assert len(found) == len(list(nx.simple_cycles(G)))
    return ev


def two_color(g):
    """rustworkx-core @ 25398ab47f66 src/coloring.rs `two_color`: each
    uncoloured node starts a component with colour 1; a stack; each
    neighbour (outgoing, then incoming, edges) gets the other colour; a
    neighbour of the same colour ends the run (not bipartite)."""
    out, inc = adjacency(g, "UNDIRECTED")
    n = g["n"]
    col, ev = [None] * n, []
    for node in range(n):
        if col[node] is not None:
            continue
        stack = [node]
        ev.append(f"colour: {node} 1")
        col[node] = 1
        while stack:
            v = stack.pop()
            for w, _ in out[v] + inc[v]:
                if col[w] == col[v]:
                    ev.append(f"conflict: {v} {w}")
                    return ev
                if col[w] is None:
                    ev.append(f"colour: {w} {1 - col[v]}")
                    col[w] = 1 - col[v]
                    stack.append(w)
    return ev


def havel_hakimi(seq):
    """graphalgs @ 1f8a412ef4af src/spec/mod.rs
    `is_degree_sequence_graphlike` (Havel 1955, Hakimi 1962): odd sum
    fails; a max-heap of positive degrees; pop d, fail if d exceeds the
    rest, decrement the d largest. Event: the popped degree and the
    heap's size after the pop."""
    if sum(seq) % 2:
        return []
    h = [-d for d in seq if d > 0]
    heapq.heapify(h)
    ev = []
    while h:
        d = -heapq.heappop(h)
        ev.append(f"pop: {d} {len(h)}")
        if d > len(h):
            return ev
        tmp = [-heapq.heappop(h) - 1 for _ in range(d)]
        for x in tmp:
            if x:
                heapq.heappush(h, -x)
    return ev


def coords(g):
    out = []
    for spec in g["nodes"]:
        body = spec.split("{")[1]
        lat = float(body.split("lat:")[1].split(",")[0])
        lng = float(body.split("lng:")[1].split(",")[0])
        out.append((lat, lng))
    return out


def metres(lat1, lng1, lat2, lng2):
    """MAGE @ c72636014a8b python/mage/geography/distance_calculator.py
    `calculate_distance_between_points` (metric "m"): the haversine
    distance, earth radius 6371e3 m."""
    r = 6371e3
    rad = math.pi / 180.00
    phi1, phi2 = lat1 * rad, lat2 * rad
    d_phi, d_lambda = (lat2 - lat1) * rad, (lng2 - lng1) * rad
    a = math.sin(d_phi / 2) ** 2 + math.cos(phi1) * math.cos(phi2) * math.sin(d_lambda / 2) ** 2
    return r * 2 * math.atan2(math.sqrt(a), math.sqrt(1 - a))


def distance_matrix(pts):
    k = len(pts)
    dm = [[0.0] * k for _ in range(k)]
    for i in range(k):
        for j in range(i + 1, k):
            dm[i][j] = dm[j][i] = metres(*pts[i], *pts[j])
    return dm


def christofides_steps(dm):
    """Christofides (1976) as NetworkX 3.7 `christofides`
    (networkx/algorithms/approximation/traveling_salesman.py): a minimum
    spanning tree (Kruskal, NetworkX's default `minimum_spanning_tree`:
    edges by weight, union-find, `kruskal_mst_edges` in
    networkx/algorithms/tree/mst.py), the odd-degree nodes of the tree, a
    minimum-weight perfect matching of them (`min_weight_matching`). The
    Euler circuit and the shortcut are checked by the Rust test."""
    k = len(dm)
    edges = sorted((dm[i][j], i, j) for i in range(k) for j in range(i + 1, k))
    root = list(range(k))

    def find(x):
        while root[x] != x:
            root[x] = root[root[x]]
            x = root[x]
        return x
    tree = []
    for _, i, j in edges:
        a, b = find(i), find(j)
        if a != b:
            root[a] = b
            tree.append((i, j))
    G = nx.Graph()
    for i in range(k):
        for j in range(i + 1, k):
            G.add_edge(i, j, weight=dm[i][j])
    nxt = nx.minimum_spanning_tree(G)
    assert sorted(tuple(sorted(e)) for e in nxt.edges) == sorted(tree)
    deg = [0] * k
    for i, j in tree:
        deg[i] += 1
        deg[j] += 1
    odd = [v for v in range(k) if deg[v] % 2]
    L = G.subgraph(odd)
    matching = sorted(tuple(sorted(e)) for e in nx.min_weight_matching(L))
    return [f"mst: {tree}", f"odd: {odd}", f"matching: {matching}"]


def savings_steps(dm, vehicles):
    """Clarke and Wright (1964) savings with the rules of TS-25 (user
    decision 2026-10-12, item D) and lpg's module documentation: savings
    s(i, j) = d(0, i) + d(0, j) − d(i, j) for customers i < j, in
    decreasing order (ties by pair); a pass over the positive savings,
    then, while more routes than vehicles remain, a pass over the rest;
    two routes merge when i and j are ends of different routes, as
    "… i, j …"; then first-improvement 2-opt on each route (Croes 1958)."""
    k = len(dm)
    routes = {i: [i] for i in range(1, k)}
    of = list(range(k))
    sv = sorted(((dm[0][i] + dm[0][j] - dm[i][j], i, j) for i in range(1, k) for j in range(i + 1, k)),
                key=lambda x: (-x[0], x[1], x[2]))
    ev = [f"savings: {[(i, j) for _, i, j in sv]}"]
    count = k - 1
    for pas in (0, 1):
        for sav, i, j in sv:
            if pas == 0 and sav <= 0:
                break
            if pas == 1 and count <= vehicles:
                break
            ri, rj = of[i], of[j]
            if ri == rj:
                continue
            a, b = routes[ri], routes[rj]
            if i not in (a[0], a[-1]) or j not in (b[0], b[-1]):
                continue
            a, b = list(a), list(b)
            if a[-1] != i:
                a.reverse()
            if b[0] != j:
                b.reverse()
            a += b
            del routes[rj]
            routes[ri] = a
            ev.append(f"merge: {pas} {i} {j} {a}")
            for v in a:
                of[v] = ri
            count -= 1
    for key in sorted(routes):
        route = routes[key]
        n = len(route)
        while True:
            improved = False
            for i in range(n):
                for j in range(i + 1, n):
                    a = 0 if i == 0 else route[i - 1]
                    d = 0 if j + 1 == n else route[j + 1]
                    b, c = route[i], route[j]
                    if dm[a][c] + dm[b][d] - dm[a][b] - dm[c][d] < -1e-9:
                        route[i:j + 1] = route[i:j + 1][::-1]
                        ev.append(f"move: {i} {j} {route}")
                        improved = True
            if not improved:
                break
    return ev


def iso_cases(g):
    """lpg collapses each projection's parallel edges into one edge
    weighted by their number (`structure.rs` `counted`); the answer is
    NetworkX 3.7's `is_isomorphic` and, for 190, `subgraph_is_isomorphic`
    (node-induced; networkx/algorithms/isomorphism/isomorphvf2.py) with
    edges matched by that number, on directed and undirected graphs."""
    from networkx.algorithms import isomorphism as iso
    groups = {}
    for v, spec in enumerate(g["nodes"]):
        groups.setdefault(spec[1:], []).append(v)

    def counted(label, undirected):
        members = groups[label]
        cnt = {}
        for a, b, _, _ in g["edges"]:
            if a in members and b in members:
                k = (members.index(a), members.index(b))
                if undirected:
                    k = (min(k), max(k))
                cnt[k] = cnt.get(k, 0) + 1
        G = nx.Graph() if undirected else nx.DiGraph()
        G.add_nodes_from(range(len(members)))
        for (a, b), c in cnt.items():
            G.add_edge(a, b, count=c)
        return sorted((a, b, c) for (a, b), c in cnt.items()), G

    out = {}
    em = lambda x, y: x["count"] == y["count"]
    for first, other in (("P", "Q"), ("P", "U"), ("P", "S"), ("U", "S")):
        for und in (False, True):
            a, ga = counted(first, und)
            b, gb = counted(other, und)
            ev = [f"counted: {'true' if und else 'false'} {a} {b}"]
            whole = nx.is_isomorphic(ga, gb, edge_match=em)
            M = (iso.GraphMatcher if und else iso.DiGraphMatcher)(ga, gb, edge_match=em)
            sub = M.subgraph_is_isomorphic()
            for proc, ans in (("gds.isIsomorphic.stream", whole), ("gds.isIsomorphicSubgraph.stream", sub)):
                out[f"{proc.split('.')[1]} {first}-{other} {'UNDIRECTED' if und else 'NATURAL'}"] = case(
                    "iso", proc, {"nodeLabels": [first], "otherNodeLabels": [other], "otherRelationshipTypes": ["*"]},
                    kinds=["counted"], want=ev, orientation="UNDIRECTED" if und else "NATURAL", rows=[[ans]])
    return out


def savings_steps_route(dm, route):
    """The 2-opt part of `savings_steps` on one route (Croes 1958, first
    improvement, the scan continuing after a reversal, until a pass makes
    none)."""
    ev, n = [], len(route)
    while True:
        improved = False
        for i in range(n):
            for j in range(i + 1, n):
                a = 0 if i == 0 else route[i - 1]
                d = 0 if j + 1 == n else route[j + 1]
                b, c = route[i], route[j]
                if dm[a][c] + dm[b][d] - dm[a][b] - dm[c][d] < -1e-9:
                    route[i:j + 1] = route[i:j + 1][::-1]
                    ev.append(f"move: {i} {j} {route}")
                    improved = True
        if not improved:
            return ev


# ---------------------------------------------------------------- cases

def case(graph, proc, config=None, args=None, kinds=None, want=None, orientation=None,
         concurrency=(1,), rows=None):
    c = dict(config or {})
    if orientation:
        c["orientation"] = orientation
    out = dict(graph=graph, proc=proc, args=args or [], config=c, kinds=kinds, want=want,
               concurrency=list(concurrency))
    if rows is not None:
        out["rows"] = rows
    return out


def N(i):
    return {"node": i}


def main():
    G = GRAPHS
    W = {"relationshipWeightProperty": "weight"}
    par = (1, 16)
    cases = {}
    # 001: GDS BFS; all, maxDepth, target.
    cases["001 all"] = case("dir", "gds.bfs.stream", {"sourceNode": N(0)}, kinds=["level", "expand", "append", "stop"], want=bfs_gds(G["dir"], 0)[0], concurrency=par)
    cases["001 maxDepth 2"] = case("dir", "gds.bfs.stream", {"sourceNode": N(0), "maxDepth": 2}, kinds=["level", "expand", "append", "stop"], want=bfs_gds(G["dir"], 0, 2)[0], concurrency=par)
    cases["001 target"] = case("dir", "gds.bfs.stream", {"sourceNode": N(0), "targetNodes": [N(4)]}, kinds=["level", "expand", "append", "stop"], want=bfs_gds(G["dir"], 0, None, (4,))[0], concurrency=par)
    # 002: GDS DFS.
    cases["002 all"] = case("dir", "gds.dfs.stream", {"sourceNode": N(0)}, kinds=["pop", "push", "stop"], want=dfs_gds(G["dir"], 0)[0])
    cases["002 maxDepth 1"] = case("dir", "gds.dfs.stream", {"sourceNode": N(0), "maxDepth": 1}, kinds=["pop", "push", "stop"], want=dfs_gds(G["dir"], 0, 1)[0])
    cases["002 target"] = case("dir", "gds.dfs.stream", {"sourceNode": N(0), "targetNodes": [N(3)]}, kinds=["pop", "push", "stop"], want=dfs_gds(G["dir"], 0, None, (3,))[0])
    # 003: MAGE neighbors.
    for types in ([], ["A>"], ["<B"], ["A"], ["<"], [">"]):
        cases[f"003 by_hop {types}"] = case("typed", "neighbors.by_hop", args=[N(0), types, 3], kinds=["level"], want=hop_levels(G["typed"], 0, types, 3, False))
        cases[f"003 at_hop {types}"] = case("typed", "neighbors.at_hop", args=[N(0), types, 2], kinds=["level"], want=hop_levels(G["typed"], 0, types, 2, True))
    # 004: MAGE path.
    for rels in ([], ["A>"], ["<B"], ["<A>"], ["B", "<A"], [">"]):
        for labels in ([], ["-Y"], [">X"], ["/Y"], ["+X"]):
            cases[f"004 expand {rels} {labels}"] = case("typed", "gds.pathExpand.stream", {"start": [N(0)], "relationships": rels, "labels": labels, "minHops": 0, "maxHops": 3}, kinds=["enter", "emit"], want=path_expand(G["typed"], [0], rels, labels, 0, 3))
        cases[f"004 subgraph {rels}"] = case("typed", "gds.pathSubgraphNodes.stream", {"startNode": [N(0)], "relationshipFilter": rels}, kinds=["pop", "push"], want=path_subgraph(G["typed"], [0], rels, [])[0])
    cases["004 subgraph labels"] = case("typed", "gds.pathSubgraphNodes.stream", {"startNode": [N(0)], "labelFilter": ["-Y"], "maxHops": 1}, kinds=["pop", "push"], want=path_subgraph(G["typed"], [0], [], ["-Y"], 0, 1)[0])
    # 007: Kahn levels.
    cases["007 dag"] = case("dag", "gds.dag.topologicalSort.stream", {"computeMaxDistanceFromSource": True}, kinds=["level"], want=kahn_levels(G["dag"])[0], concurrency=par)
    cases["007 cyclic"] = case("dir", "gds.dag.topologicalSort.stream", {}, kinds=["level"], want=kahn_levels(G["dir"])[0], concurrency=par)
    # 008: ancestors and descendants.
    cases["008 descendants"] = case("dir", "graph_util.descendants", args=[N(0)], kinds=["visit"], want=bfs_reach(G["dir"], 0, True))
    cases["008 ancestors"] = case("dir", "graph_util.ancestors", args=[N(3)], kinds=["visit"], want=bfs_reach(G["dir"], 3, False))
    # 011: dominators.
    cases["011"] = case("dir", "gds.dominatorsSimpleFast.stream", {"sourceNode": N(0)}, kinds=["idom", "frontier"], want=dominance(G["dir"], 0))
    cases["011 und"] = case("und", "gds.dominatorsSimpleFast.stream", {"sourceNode": N(0)}, kinds=["idom", "frontier"], want=dominance(G["und"], 0))
    # 013: breadth-first trees.
    cases["013 source"] = case("dir", "gds.unweightedShortestPaths.stream", {"sourceNode": N(0)}, kinds=["visit"], want=bfs_tree(G["dir"], 0)[0])
    cases["013 target"] = case("dir", "gds.unweightedShortestPaths.stream", {"targetNode": N(5)}, kinds=["visit"], want=bfs_tree(G["dir"], 5, False)[0])
    cases["013 pair"] = case("dir", "gds.unweightedShortestPaths.stream", {"sourceNode": N(0), "targetNode": N(5)}, kinds=["visit"], want=bfs_tree(G["dir"], 0)[0])
    # 014: Dijkstra.
    cases["014 pair"] = case("dir", "gds.shortestPath.dijkstra.stream", {"sourceNode": N(0), "targetNode": N(5), **W}, kinds=["pop", "relax"], want=dijkstra(G["dir"], 0)[0])
    cases["014 all"] = case("dir", "gds.allShortestPaths.dijkstra.stream", {"sourceNode": N(0), **W}, kinds=["pop", "relax"], want=dijkstra(G["dir"], 0)[0])
    cases["014 reverse"] = case("dir", "gds.allShortestPaths.dijkstra.stream", {"sourceNode": N(5), **W}, kinds=["pop", "relax"], want=dijkstra(G["dir"], 5, "REVERSE")[0], orientation="REVERSE")
    # 015: bidirectional Dijkstra.
    cases["015"] = case("dir", "gds.bidirectionalDijkstra.stream", {"sourceNode": N(0), "targetNode": N(5), **W}, kinds=["pop", "best", "stop"], want=bidirectional(G["dir"], 0, 5))
    # 019: Bellman-Ford rounds.
    cases["019 neg"] = case("neg", "gds.bellmanFord.stream", {"sourceNode": N(0), **W}, kinds=["round", "stop"], want=bellman_rounds(G["neg"], [0])[0], concurrency=par)
    cases["019 negcyc"] = case("negcyc", "gds.bellmanFord.stream", {"sourceNode": N(0), **W}, kinds=["round", "stop"], want=bellman_rounds(G["negcyc"], [0])[0], concurrency=par)
    cases["019 dir"] = case("dir", "gds.bellmanFord.stream", {"sourceNode": N(0), **W}, kinds=["round", "stop"], want=bellman_rounds(G["dir"], [0])[0], concurrency=par)
    # 023: delta-stepping.
    for d in (2.0, 3.0):
        cases[f"023 delta {d}"] = case("dir", "gds.allShortestPaths.delta.stream", {"sourceNode": N(0), "delta": d, **W}, kinds=["light", "heavy", "relax"], want=delta_stepping(G["dir"], 0, d), concurrency=par)
    # 024: Johnson.
    cases["024"] = case("neg", "gds.johnson.stream", W, kinds=["round", "potential", "source"], want=bellman_rounds(G["neg"], [])[0] + johnson(G["neg"]), concurrency=par)
    # 026: Seidel.
    cases["026 und"] = case("und", "gds.seidel.stream", {}, kinds=["component", "apd"], want=seidel(G["und"]))
    cases["026 bip"] = case("bip", "gds.seidel.stream", {}, kinds=["component", "apd"], want=seidel(G["bip"]))
    # 027: all pairs.
    cases["027 weighted"] = case("dir", "gds.allShortestPaths.stream", W, kinds=["source"], want=all_pairs(G["dir"], True), concurrency=par)
    cases["027 unweighted"] = case("dir", "gds.allShortestPaths.stream", {}, kinds=["source"], want=all_pairs(G["dir"], False), concurrency=par)
    # 028: all shortest paths between two nodes (ties: unweighted).
    cases["028 unweighted"] = case("und", "gds.allShortestPathsBetweenNodes.stream", {"sourceNode": N(0), "targetNode": N(4)}, kinds=["preds", "path"], want=all_shortest_between(dict(G["und"], edges=[(a, b, t, 1.0) for a, b, t, _ in G["und"]["edges"]]), 0, 4))
    cases["028 weighted"] = case("dir", "gds.allShortestPathsBetweenNodes.stream", {"sourceNode": N(0), "targetNode": N(5), **W}, kinds=["preds", "path"], want=all_shortest_between(G["dir"], 0, 5))
    # 029: Yen.
    cases["029"] = case("dir", "gds.shortestPath.yens.stream", {"sourceNode": N(0), "targetNode": N(5), "k": 5, **W}, kinds=["round", "candidate", "accept"], want=yens(G["dir"], 0, 5, 5), concurrency=par)
    # 031: MAGE all simple paths, typed filters.
    for types in ([], ["A>"], ["<B"], ["A"], ["<A>"], ["A>", "<B"], ["<"], [">"], ["B", "<A>"]):
        cases[f"031 {types}"] = case("typed", "algo.all_simple_paths", args=[N(0), N(3), types, 4], kinds=["enter", "emit"], want=all_simple(G["typed"], 0, 3, types, 4))
    # 032: longest path by levels.
    cases["032"] = case("dag", "gds.dag.longestPath.stream", W, kinds=["level", "settle"], want=kahn_levels(G["dag"])[0] + longest(G["dag"]), concurrency=par)
    # 180: MAGE cycles.
    cases["180"] = case("und", "cycles.get", {}, kinds=["tree", "nontree", "fundamental", "cycle"], want=mage_cycles(G["und"]))
    cases["180 bip"] = case("bip", "cycles.get", {}, kinds=["tree", "nontree", "fundamental", "cycle"], want=mage_cycles(G["bip"]))
    # 181: graphalgs circuits.
    cases["181"] = case("dir", "gds.elementaryCircuits.stream", {}, kinds=["search", "unblock", "circuit"], want=circuits(G["dir"]))
    cases["181 und"] = case("und", "gds.elementaryCircuits.stream", {}, kinds=["search", "unblock", "circuit"], want=circuits(G["und"]))
    # 194: rustworkx two_color.
    cases["194 bip"] = case("bip", "gds.isBipartiteUndirected.stream", {}, kinds=["colour", "conflict"], want=two_color(G["bip"]))
    cases["194 und"] = case("und", "gds.isBipartiteUndirected.stream", {}, kinds=["colour", "conflict"], want=two_color(G["und"]))
    # 204: Havel-Hakimi.
    for seq in ([3, 3, 2, 2, 1, 1], [3, 3, 3, 1], [4, 1, 1, 1, 1], [2, 2, 2], [5, 1, 1]):
        cases[f"204 {seq}"] = case("bip", "gds.isDegreeSequenceGraphlike.stream", {"degreeSequence": seq}, kinds=["pop"], want=havel_hakimi(seq))
    # 205/206: the topological order handed to petgraph.
    cases["205"] = case("dag", "gds.transitiveClosure.stream", {}, kinds=["level", "order"], want=kahn_levels(G["dag"])[0] + [f"order: {[v for l in kahn_levels(G['dag'])[1] for v in l]}"])
    pts = coords(G["geo"])
    cases["187"] = case("geo", "tsp.solve", args=[[N(i) for i in range(6)], "1.5_approx"], kinds=["mst", "odd", "matching"], want=christofides_steps(distance_matrix(pts)))
    for v in (1, 2):
        cases[f"188 vehicles {v}"] = case("geo", "vrp.route", args=[N(0), v], kinds=["savings", "merge", "move"], want=savings_steps(distance_matrix(pts), v))
    cases["206"] = case("dag", "gds.transitiveReduction.stream", {}, kinds=["level", "order"], want=kahn_levels(G["dag"])[0] + [f"order: {[v for l in kahn_levels(G['dag'])[1] for v in l]}"])
    cases.update(iso_cases(G["iso"]))
    # 203: NetworkX 3.7 `is_tournament` (networkx/algorithms/tournament.py)
    # and `is_directed_acyclic_graph` (a tournament is transitive exactly
    # when acyclic); no trace: the answer per group.
    for label, members in (("T", range(0, 4)), ("C", range(4, 7)), ("X", range(7, 10))):
        D = nx.DiGraph()
        D.add_nodes_from(members)
        D.add_edges_from((a, b) for a, b, _, _ in G["tour"]["edges"] if a in members and b in members)
        t = nx.tournament.is_tournament(D)
        cases[f"203 {label}"] = case("tour", "gds.isTournament.stream", {"nodeLabels": [label]}, kinds=[], want=[], rows=[[t, t and nx.is_directed_acyclic_graph(D)]])
    # 204: the answers of NetworkX 3.7 `is_graphical` (Erdős–Gallai).
    for k in [k for k in cases if k.startswith("204 ")]:
        seq = cases[k]["config"]["degreeSequence"]
        cases[k]["rows"] = [[nx.is_graphical(seq)]]
    # 001, 002 on the lab fixtures: GDS's outputs are the GDS algorithm on
    # GDS's node numbering (Neo4j import order = numeric uid); lpg numbers
    # nodes by element identifier, which its CSV import assigns in the
    # string order of `id:ID`; the expected lpg orders are the same
    # algorithm on that numbering.
    fixtures = {}
    T = os.path.join(os.path.dirname(__file__), "../../../src/algo/testdata")
    for name, alg, f, kw in (
            ("zipf60", "bfs", "zipf60.gds.bfs.jsonl", dict(s=1)),
            ("zipf60", "bfs", "zipf60.gds.bfs.depth2.jsonl", dict(s=1, max_depth=2)),
            ("zipf60", "bfs", "zipf60.gds.bfs.target.jsonl", dict(s=1, targets_of=(8,))),
            ("uniform60", "bfs", "uniform60.gds.bfs.jsonl", dict(s=14)),
            ("zipf60", "dfs", "zipf60.gds.dfs.jsonl", dict(s=1)),
            ("zipf60", "dfs", "zipf60.gds.dfs.target.jsonl", dict(s=1, targets_of=(8,))),
            ("uniform60", "dfs", "uniform60.gds.dfs.depth3.jsonl", dict(s=14, max_depth=3))):
        rows = list(csv.reader(open(os.path.join(T, name, "edges.csv"))))[1:]
        n = len(list(csv.reader(open(os.path.join(T, name, "nodes.csv"))))) - 1
        gds = json.loads(open(os.path.join(T, "expected", f)).readline())["nodeIds"]
        orders = {}
        for label, key in (("numeric", lambda x: x), ("string", lambda x: str(x))):
            # Relabel nodes by rank under `key`, run the reference, map back.
            rank = {v: i for i, v in enumerate(sorted(range(n), key=key))}
            back = {i: v for v, i in rank.items()}
            gg = dict(n=n, edges=[(rank[int(r[0])], rank[int(r[1])], "R", None) for r in rows])
            kw2 = dict(kw)
            kw2["s"] = rank[kw["s"]]
            if "targets_of" in kw:
                kw2["targets_of"] = tuple(rank[x] for x in kw["targets_of"])
            fn = bfs_gds if alg == "bfs" else dfs_gds
            orders[label] = [back[x] for x in fn(gg, kw2.pop("s"), **kw2)[1]]
        assert orders["numeric"] == gds, f
        fixtures[f] = dict(fixture=name, proc=f"gds.{alg}.stream", source=kw["s"],
                           maxDepth=kw.get("max_depth"), targets=list(kw.get("targets_of", ())),
                           lpg=orders["string"], gds_equal=orders["string"] == gds)
    # 205, 206 rows: NetworkX 3.7 `transitive_closure_dag` and
    # `transitive_reduction` (networkx/algorithms/dag.py); node i has
    # element identifier i and edge i has n + i (TraceGraph); a reduction
    # edge's relationship is the first of its parallel edges in forward
    # order (adjacency-tree order: the smallest identifier).
    D = nx.DiGraph([(a, b) for a, b, _, _ in G["dag"]["edges"]])
    D.add_nodes_from(range(G["dag"]["n"]))
    cases["205"]["rows"] = [[a, b] for a, b in sorted(nx.transitive_closure_dag(D).edges)]
    first = {}
    for i, (a, b, _, _) in enumerate(G["dag"]["edges"]):
        first.setdefault((a, b), G["dag"]["n"] + i)
    cases["206"]["rows"] = [[a, b, first[(a, b)]] for a, b in sorted(nx.transitive_reduction(D).edges)]
    # 188's 2-opt on a route the savings pass leaves crossed: the points of
    # a square's corners and midpoints visited out of order.
    sq = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.5), (0.5, 1.0), (1.0, 0.5)]
    dm = distance_matrix(sq)
    route = [2, 4, 6, 1, 3, 5]
    two_opt = [e for e in savings_steps_route(dm, list(route))]
    json.dump(dict(graphs=GRAPHS, cases=cases, fixtures=fixtures,
                   two_opt=dict(dm=dm, route=route, want=two_opt)), open(OUT, "w"), indent=1)
    print(f"{len(cases)} cases written to {os.path.normpath(OUT)}")


if __name__ == "__main__":
    main()
