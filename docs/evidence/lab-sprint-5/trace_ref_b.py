#!/usr/bin/env python3
"""Reference executions of rmp #71 pass B (the entries of tasks #63, #65, #66).

Each function below re-executes, on a small trace graph, the operations of
the algorithm's reference, step by step, and emits the events in the format
of lpg's test trace (`src/algo/trace.rs`): "kind: text". The source of each
reference is cited in its docstring (file:line at the commit the sprint
studied: GDS @ 417d7ecf973d, MAGE @ c72636014a8b, rustworkx @ 25398ab47f66,
network_partitions (graspologic-native) @ 773181488b4e, hdbscan @
82ac6aa9a378, linfa @ 3b411a4c985c). The script writes
`src/algo/testdata/expected/trace_b.json`; `src/algo/tests_trace_b.rs`
runs every case on lpg and compares.

Run: /tmp/lpg-lab-venv/bin/python -I docs/evidence/lab-sprint-5/trace_ref_b.py
(NumPy in the venv, for the spectral entries' eigen decomposition only.)

Conventions shared with lpg's trace and with pass A (`trace_ref_a.py`):
node i is dense index i; edge i of a graph's list has element identifier
n + i; adjacency rows are in adjacency-tree order (type token, neighbour,
edge identifier); under UNDIRECTED a node's edges are its outgoing row then
its incoming row. Floats print as Rust's `{:?}` (`rf`). A graph of n <= 64
nodes has n blocks of one node, so a block's generator is its node's.

Randomised entries draw from `ChaCha8Rng::seed_from_u64(seed)` with
`set_stream(block)` (TS-25 Randomness). `ChaCha8` below is a port of that
generator and of the `rand` 0.10.3 samplers lpg calls; `rng_cases()`
records draws that `tests_trace_b::python_generator_is_chacha8` checks
against the real generator, so the references below replay the exact
random choices of a run. Sources: chacha20 0.10.2 `src/rng.rs`
(`set_word_pos`, `set_stream`), rand_core 0.10.1 `src/block.rs`
(`next_word`, `next_u64_from_u32`), `src/seedable_rng.rs:109-121, 192-206`
(`seed_from_u64`, PCG32), rand 0.10.3 `src/distr/uniform_int.rs:168-224,
552-597` (`sample_single`, `sample_single_inclusive`, the biased Canon
method: the `unbiased` feature is off), `src/distr/float.rs:117-130` (f64
from the top 53 bits).
"""

import bisect
import heapq
import json
import math
import os

import numpy as np

OUT = os.path.join(os.path.dirname(__file__), "../../../src/algo/testdata/expected/trace_b.json")
T = os.path.join(os.path.dirname(__file__), "../../../src/algo/testdata")

M32 = 0xFFFFFFFF
M64 = 0xFFFFFFFFFFFFFFFF

# ------------------------------------------------------------ generator


def _rotl(x, n):
    return ((x << n) | (x >> (32 - n))) & M32


def _qr(s, a, b, c, d):
    s[a] = (s[a] + s[b]) & M32; s[d] = _rotl(s[d] ^ s[a], 16)
    s[c] = (s[c] + s[d]) & M32; s[b] = _rotl(s[b] ^ s[c], 12)
    s[a] = (s[a] + s[b]) & M32; s[d] = _rotl(s[d] ^ s[a], 8)
    s[c] = (s[c] + s[d]) & M32; s[b] = _rotl(s[b] ^ s[c], 7)


def _block(state):
    x = list(state)
    for _ in range(4):  # ChaCha8: 4 double rounds
        _qr(x, 0, 4, 8, 12); _qr(x, 1, 5, 9, 13); _qr(x, 2, 6, 10, 14); _qr(x, 3, 7, 11, 15)
        _qr(x, 0, 5, 10, 15); _qr(x, 1, 6, 11, 12); _qr(x, 2, 7, 8, 13); _qr(x, 3, 4, 9, 14)
    return [(x[i] + state[i]) & M32 for i in range(16)]


def _pcg32(state):
    state = (state * 0x5851F42D4C957F2D + 0xA17654E46FBE17F3) & M64
    xs = (((state >> 18) ^ state) >> 27) & M32
    rot = state >> 59
    return state, ((xs >> rot) | (xs << ((32 - rot) & 31))) & M32


class ChaCha8:
    """`ChaCha8Rng` with a buffer of 4 blocks (64 words)."""

    def __init__(self, seed):
        st, key = seed & M64, []
        for _ in range(8):
            st, x = _pcg32(st)
            key.append(x)
        self.state = [0x61707865, 0x3320646E, 0x79622D32, 0x6B206574] + key + [0, 0, 0, 0]
        self.buf, self.index = [0] * 64, 64

    def _generate(self):
        out = []
        for _ in range(4):
            out.extend(_block(self.state))
            ctr = ((self.state[13] << 32) | self.state[12]) + 1
            self.state[12], self.state[13] = ctr & M32, (ctr >> 32) & M32
        self.buf = out

    def set_stream(self, stream):
        self.state[14], self.state[15] = stream & M32, (stream >> 32) & M32
        self.state[12] = self.state[13] = 0
        self.index = 64

    def set_word_pos(self, w):
        idx, ctr = w % 16, w // 16
        self.state[12], self.state[13] = ctr & M32, (ctr >> 32) & M32
        if idx == 0:
            self.index = 64
        else:
            self._generate()
            self.index = idx

    def u32(self):
        if self.index >= 64:
            self._generate()
            self.index = 0
        v = self.buf[self.index]
        self.index += 1
        return v

    def u64(self):
        i = self.index
        if i < 63:
            lo, hi = self.buf[i], self.buf[i + 1]
            self.index = i + 2
        else:
            lo = self.buf[63]
            self._generate()
            hi, ni = self.buf[0], 1
            if i >= 64:
                lo, hi, ni = hi, self.buf[1], 2
            self.index = ni
        return (hi << 32) | lo

    def f64(self):
        return (self.u64() >> 11) * (1.0 / (1 << 53))

    def _incl(self, low, high, bits):
        mask = M32 if bits == 32 else M64
        nxt = self.u32 if bits == 32 else self.u64
        rng = (high - low + 1) & mask
        if rng == 0:
            return nxt()
        m = nxt() * rng
        result, lo = m >> bits, m & mask
        if lo > ((-rng) & mask):
            new_hi = (nxt() * rng) >> bits
            if lo + new_hi > mask:
                result += 1
        return low + result

    def range(self, low, high):
        """`random_range(low..high)` on `usize`."""
        assert low < high
        return self._incl(low, high - 1, 64 if high > M32 else 32)

    def range_incl(self, low, high):
        """`random_range(low..=high)` on `usize`."""
        assert low <= high
        return self._incl(low, high, 64 if high > M32 else 32)


def rng(seed, block):
    """lpg `sched::rng`."""
    g = ChaCha8(seed)
    g.set_stream(block)
    return g


def rng_cases():
    """Draws for `python_generator_is_chacha8`: (seed, stream, word
    position or None, operations, results)."""
    out = []
    for seed, stream, pos in ((0, 0, None), (42, 3, None), (7, 5, 2 * 13), (1, 1 << 40 | 3, None)):
        g = rng(seed, stream)
        if pos is not None:
            g.set_word_pos(pos)
        ops, res = [], []
        for i in range(150):
            kind = ("u64", "f64", "range", "incl", "u32")[i % 5]
            if kind == "range":
                hi = 1 + (i * 7919) % 1000
                ops.append(["range", 0, hi]); res.append(g.range(0, hi))
            elif kind == "incl":
                ops.append(["incl", 1, 6]); res.append(g.range_incl(1, 6))
            elif kind == "f64":
                ops.append(["f64"]); res.append(g.f64())
            elif kind == "u32":
                ops.append(["u32"]); res.append(g.u32())
            else:
                ops.append(["u64"]); res.append(str(g.u64()))
        out.append(dict(seed=seed, stream=stream, pos=pos, ops=ops, results=res))
    return out

# ------------------------------------------------------------ formatting


def rf(x):
    """Rust's `{:?}` of an f64."""
    if math.isnan(x):
        return "NaN"
    if math.isinf(x):
        return "inf" if x > 0 else "-inf"
    r = repr(float(x))
    if "e" in r:
        m, e = r.split("e")
        return f"{m}e{int(e)}"
    return r


def fmt(x):
    """Rust's `{:?}` of numbers, tuples, lists and ordered maps."""
    if isinstance(x, bool):
        return "true" if x else "false"
    if isinstance(x, float):
        return rf(x)
    if isinstance(x, int):
        return str(x)
    if isinstance(x, tuple):
        return "(" + ", ".join(fmt(y) for y in x) + ")"
    if isinstance(x, list):
        return "[" + ", ".join(fmt(y) for y in x) + "]"
    if isinstance(x, dict):
        return "{" + ", ".join(f"{fmt(k)}: {fmt(v)}" for k, v in sorted(x.items())) + "}"
    raise TypeError(x)


def fdiv(a, b):
    """IEEE division: x / 0.0 is NaN or an infinity, as in Rust."""
    if b == 0.0:
        return math.nan if a == 0.0 or math.isnan(a) else math.copysign(math.inf, a) * math.copysign(1.0, b)
    return a / b


def take(events):
    """lpg `trace::Recording::take`: each run of consecutive block events
    (text, block) is sorted by block, stably; other events are (text, None)."""
    out, run = [], []
    for text, b in events:
        if b is None:
            run.sort(key=lambda x: x[0])
            out.extend(t for _, t in run)
            run = []
            out.append(text)
        else:
            run.append((b, text))
    run.sort(key=lambda x: x[0])
    out.extend(t for _, t in run)
    return out


def rsum(xs):
    """Rust's `Iterator::sum` of f64: it starts at -0.0."""
    acc = -0.0
    for x in xs:
        acc += x
    return acc

# ---------------------------------------------------------------- graphs


def write_points(coords):
    """The fixture `pts10`: node i has uid i and element identifier i."""
    d = os.path.join(T, "pts10")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "nodes.csv"), "w") as f:
        f.write("id:ID,:LABEL,uid:long,x:double,vec:double[]\n")
        for i, (x, y) in enumerate(coords):
            f.write(f"{i},N,{i},{x!r},{x!r};{y!r}\n")
    with open(os.path.join(d, "edges.csv"), "w") as f:
        f.write(":START_ID,:END_ID,:TYPE\n")


COORDS = [(0.0, 0.0), (0.5, 0.2), (0.1, 0.6), (5.0, 5.0), (5.4, 5.1), (4.8, 5.6),
          (5.2, 4.7), (10.0, 0.0), (10.3, 0.4), (2.5, 9.0)]

GRAPHS = {
    # Directed, weighted: a self-loop (8), a parallel edge (9), node 4
    # without incoming edges, node 5 without outgoing edges.
    "pr": dict(n=6, edges=[
        (0, 1, "R", 1.0), (0, 2, "R", 2.0), (1, 2, "R", 1.0), (2, 0, "R", 1.0),
        (2, 3, "R", 3.0), (3, 2, "R", 1.0), (4, 2, "R", 1.0), (4, 3, "R", 0.5),
        (1, 1, "R", 1.0), (0, 1, "R", 1.0), (4, 5, "R", 1.0)]),
    # Communities: cliques {0..3}, {4..7}, triangle {8, 9, 10}, joined in a
    # ring; a parallel edge (15) and a self-loop (16).
    "cm": dict(n=11, edges=[
        (0, 1, "R", None), (0, 2, "R", None), (0, 3, "R", None), (1, 2, "R", None),
        (1, 3, "R", None), (2, 3, "R", None), (4, 5, "R", None), (4, 6, "R", None),
        (4, 7, "R", None), (5, 6, "R", None), (5, 7, "R", None), (6, 7, "R", None),
        (8, 9, "R", None), (8, 10, "R", None), (9, 10, "R", None),
        (1, 2, "R", None), (5, 5, "R", None),
        (3, 4, "R", None), (7, 8, "R", None), (10, 0, "R", None)]),
    # The 4-cycle on which one colour-class step differs from the
    # sequential local move (found by a search over small graphs).
    "c4": dict(n=4, edges=[(0, 1, "R", None), (0, 3, "R", None), (1, 2, "R", None), (2, 3, "R", None)]),
    # Points: three groups and a noise point; `x` is the first coordinate.
    # Loaded from the CSV fixture `pts10` (written below): INSERT cannot
    # write a list value (GV50).
    "pts": dict(n=10, edges=[], fixture="pts10"),
    # Spectral: an irregular graph of two groups (simple eigenvalues).
    "sp": dict(n=7, edges=[
        (0, 1, "R", None), (0, 2, "R", None), (1, 2, "R", None), (2, 3, "R", None),
        (1, 3, "R", None), (4, 5, "R", None), (5, 6, "R", None), (4, 6, "R", None),
        (3, 4, "R", None)]),
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


def slots(g):
    """Edge id -> forward position (`Projection::out` yields it)."""
    fwd, _ = adjacency(g)
    out, i = {}, 0
    for row in fwd:
        for _, e in row:
            out[e] = i
            i += 1
    return out

# --------------------------------------------- 040, 042-044: power iteration


def power(g, variant="pr", orientation="NATURAL", damping=0.85, tol=1e-7, maxit=20,
          weighted=False, sources=()):
    """GDS's Pregel supersteps: `pagerank/PageRankComputation.java`
    (`compute`: superstep 0 sends rank/degree; then delta = damping x sum,
    rank += delta, send delta/degree while delta > tolerance, else vote to
    halt), `ArticleRankComputation.java` (delta / (degree + averageDegree)),
    `EigenvectorComputation.java` (next = rank + sum; L2-normalised in
    `masterCompute`; converged when no rank moved by more than tolerance
    after superstep 0), `pregel/.../Pregel.java:204-242` (`run`:
    ranIterations is the superstep that converged), `ReducingMessenger.java:
    119-120` (a node has messages when their sum is not 0), `ComputeStep.java:71`
    (a node computes with messages or when not halted). Messages to a node
    are summed over its incoming edges in adjacency order, which is the
    order of GDS's single-thread sends on these graphs."""
    out, inc = adjacency(g, orientation)
    n = g["n"]
    w = lambda e: weight(g, e)
    if weighted:
        deg = [rsum(w(e) for _, e in out[v] if w(e) > 0.0) for v in range(n)]
    else:
        deg = [float(len(out[v])) for v in range(n)]
    add = float(sum(len(out[v]) for v in range(n))) / float(n) if variant == "ar" else 0.0
    ev = []
    if variant == "ev":
        init = 1.0 / (len(sources) if sources else n)
        rank = [init if (not sources or v in sources) else 0.0 for v in range(n)]
        delta = list(rank)

        def normalise(old):
            norm = math.sqrt(rsum(d * d for d in delta))
            new = [0.0 if norm < 1e-15 else d / norm for d in delta]
            moved = old is not None and any(abs(new[v] - old[v]) > tol for v in range(n))
            return new, old is not None and not moved
        rank, _ = normalise(None)
        ev.append(f"round: 0 {fmt(rank)}")
        for it in range(1, maxit):
            nxt = []
            for v in range(n):
                s = rank[v]
                for u, e in inc[v]:
                    d = deg[u] if weighted else 1.0
                    m = delta[u] / d
                    s += m * w(e) if weighted else m
                nxt.append(s)
            old, delta = rank, nxt
            rank, conv = normalise(old)
            ev.append(f"round: {it} {fmt(rank)}")
            if conv:
                ev.append(f"stop: {it} converged")
                return ev, rank, it
        ev.append(f"stop: {maxit} max")
        return ev, rank, maxit
    alpha = 1.0 - damping
    st = []
    for v in range(n):
        r = alpha if (not sources or v in sources) else 0.0
        st.append(dict(rank=r, delta=r, sent=deg[v] > 0.0, halted=False))

    def emit(k):
        ev.append(f"round: {k} {fmt([s['rank'] for s in st])}")
        ev.append(f"send: {k} {fmt([v for v in range(n) if st[v]['sent']])}")
    emit(0)
    for it in range(1, maxit):
        new = []
        for v in range(n):
            s = 0.0
            for u, e in inc[v]:
                if st[u]["sent"]:
                    m = st[u]["delta"] / (deg[u] + add)
                    s += m * w(e) if weighted else m
            me = st[v]
            if s != 0.0 or not me["halted"]:
                d = damping * s
                r = me["rank"] + d
                if d > tol:
                    new.append(dict(rank=r, delta=d, sent=deg[v] > 0.0, halted=False))
                else:
                    new.append(dict(rank=r, delta=d, sent=False, halted=True))
            else:
                new.append(dict(me, sent=False))
        st = new
        emit(it)
        if all(not s["sent"] and s["halted"] for s in st):
            ev.append(f"stop: {it} converged")
            return ev, [s["rank"] for s in st], it
    ev.append(f"stop: {maxit} max")
    return ev, [s["rank"] for s in st], maxit

# --------------------------------------------------- 034, 035, 038, 039


def degree_ref(g, orientation="NATURAL", weighted=False):
    """GDS `degree/DegreeCentrality.java:166-172`: the out-degree in the
    orientation, or the sum of the positive weights."""
    out, _ = adjacency(g, orientation)
    ev = []
    for v in range(g["n"]):
        x = rsum(weight(g, e) for _, e in out[v] if weight(g, e) > 0.0) if weighted else float(len(out[v]))
        ev.append(f"degree: {v} {rf(x)}")
    return ev


def brandes(g, orientation="NATURAL", weighted=False):
    """Brandes (2001) as rustworkx-core `centrality.rs:406-445`
    (`shortest_path_for_centrality`: BFS, sigma += sigma[v] and a
    predecessor per edge on the next level) and `:254-279`
    (`_accumulate_vertices`: coeff = (1 + delta[w]) / sigma[w], delta[v] +=
    sigma[v] x coeff in reverse BFS order); weighted: GDS
    `betweenness/WeightedForwardTraverser.java` (`traverse`: settle by
    smallest cost, equal cost appends a predecessor, smaller resets);
    GDS `BetweennessCentrality.java` divides each dependency by 2 on an
    undirected graph."""
    out, _ = adjacency(g, orientation)
    n = g["n"]
    div = 2.0 if orientation == "UNDIRECTED" else 1.0
    acc = [0.0] * n
    ev = [f"sources: {fmt(list(range(n)))}"]
    for s in range(n):
        sigma, delta = [0.0] * n, [0.0] * n
        dist = [math.inf] * n
        settled = [False] * n
        preds = [[] for _ in range(n)]
        stack = []
        sigma[s], dist[s] = 1.0, 0.0
        if weighted:
            heap = [(0.0, s)]
            while heap:
                d, u = heapq.heappop(heap)
                if settled[u] or d > dist[u]:
                    continue
                settled[u] = True
                stack.append(u)
                for x, e in out[u]:
                    if settled[x]:
                        continue
                    nx_ = d + weight(g, e)
                    if nx_ < dist[x]:
                        dist[x], sigma[x], preds[x] = nx_, sigma[u], [u]
                        heapq.heappush(heap, (nx_, x))
                    elif nx_ == dist[x]:
                        sigma[x] += sigma[u]
                        preds[x].append(u)
        else:
            stack.append(s)
            head = 0
            while head < len(stack):
                u = stack[head]
                head += 1
                for x, _ in out[u]:
                    if math.isinf(dist[x]):
                        dist[x] = dist[u] + 1.0
                        stack.append(x)
                    if dist[x] == dist[u] + 1.0:
                        sigma[x] += sigma[u]
                        preds[x].append(u)
        part = [0.0] * n
        for w_ in reversed(stack):
            coeff = (1.0 + delta[w_]) / sigma[w_]
            for v in preds[w_]:
                delta[v] += sigma[v] * coeff
            if w_ != s:
                part[w_] += delta[w_] / div
        ev.append(f"source: {s} order={fmt(stack)} sigma={fmt(sigma)} delta={fmt(delta)}")
        for v in range(n):
            acc[v] += part[v]
    ev.append(f"score: {fmt(acc)}")
    return ev


def closeness_ref(g, orientation="NATURAL", harmonic=False, wf=False):
    """GDS `closeness/ClosenessCentrality.java` (`computeFarness`: the
    multi-source BFS adds len x depth to a node's farness and len to its
    component, over the sources reaching it), `DefaultCentralityComputer`
    (component / farness, 0 when farness is 0), `WassermanFaustCentralityComputer`
    (x component / (n - 1)); `harmonic/HarmonicCentrality.java` (len / depth,
    over n - 1). So each node's BFS runs over its incoming edges."""
    _, inc = adjacency(g, orientation)
    n = g["n"]
    ev = []
    for v in range(n):
        dist = {v: 0}
        q = [v]
        counts = []
        head = 0
        while head < len(q):
            u = q[head]
            head += 1
            for x, _ in inc[u]:
                if x not in dist:
                    dist[x] = dist[u] + 1
                    if len(counts) <= dist[u]:
                        counts.append(0)
                    counts[dist[u]] += 1
                    q.append(x)
        if harmonic:
            s = 0.0
            for d, c in enumerate(counts):
                s += float(c) * (1.0 / (float(d) + 1.0))
            x = fdiv(s, float(n) - 1.0)
        else:
            reached = sum(counts)
            farness = sum((d + 1) * c for d, c in enumerate(counts))
            if farness == 0:
                x = 0.0
            elif wf:
                x = (float(reached) / float(farness)) * (float(reached) / (float(n) - 1.0))
            else:
                x = float(reached) / float(farness)
        ev.append(f"depths: {v} {fmt(counts)} {rf(x)}")
    return ev

# ------------------------------------------------------------- 041 Katz


def katz_ref(g, alpha, eps):
    """MAGE `cpp/katz_centrality_module/algorithm/katz.cpp`:
    `KatzCentralityLoop` (:204-231: omega_k(v) = sum of omega_{k-1}(u)
    over in-edges, c_k = c_{k-1} + alpha^k omega_k, lr = c_k, ur = c_k +
    alpha^{k+1} omega_k gamma), :385-386 (gamma = Delta / (1 - alpha^2
    Delta), Delta the largest out-degree, `Neighbours` of a directed
    `mg_graph` being the out-neighbours: memgraph `include/mg_graph.hpp:
    225-234`), `Converged` (:150-195: sorted by centrality, each ur - eps
    below the previous lr). alpha is a power of 2 here, so alpha^k is
    exact in both `pow` and `powi`."""
    out, inc = adjacency(g)
    n = g["n"]
    mx = float(max(len(out[v]) for v in range(n)))
    gamma = mx / (1.0 - alpha * alpha * mx)
    omega = [1.0] * n
    c = [0.0] * n
    ur = [0.0] * n
    ev = []
    k = 0
    while True:
        k += 1
        nxt = [0.0] * n
        for v in range(n):
            nxt[v] = rsum(omega[u] for u, _ in inc[v])
            c[v] += alpha ** k * nxt[v]
            ur[v] = c[v] + alpha ** (k + 1) * nxt[v] * gamma
        omega = nxt
        order = sorted(range(n), key=lambda v: (-c[v], v))
        ev.append(f"round: {k} c={fmt(c)} ur={fmt(ur)} order={fmt(order)}")
        if all(ur[order[i + 1]] - eps < c[order[i]] for i in range(n - 1)):
            ev.append(f"stop: {k}")
            return ev

# -------------------------------------------------------------- 045 HITS


def hits_ref(g, iterations):
    """GDS `hits/HitsComputation.java` (`compute`: INIT auth = in-degree;
    NORMALIZE divides by the norm of the step before and sends; CALCULATE
    sums the messages; `masterCompute`: the norm is the square root of the
    sum of squares) and `HitsState.java` (INIT, NORMALIZE_AUTHS,
    CALCULATE_HUBS, NORMALIZE_HUBS, CALCULATE_AUTHS, ...), 4 x
    hitsIterations supersteps."""
    out, inc = adjacency(g)
    n = g["n"]
    auth = [float(len(inc[v])) for v in range(n)]
    hub = [0.0] * n
    l2 = lambda xs: math.sqrt(rsum(-0.0 + x * x for x in xs))
    norm = l2(auth)
    ev = [f"step: 0 auth={fmt(auth)} hub={fmt(hub)} norm={rf(norm)}"]
    for step in range(1, 4 * iterations):
        if step % 4 == 1:
            auth = [x / norm for x in auth]
        elif step % 4 == 3:
            hub = [x / norm for x in hub]
        elif step % 4 == 2:
            hub = [rsum(auth[x] for x, _ in out[v]) for v in range(n)]
            norm = l2(hub)
        else:
            auth = [rsum(hub[u] for u, _ in inc[v]) for v in range(n)]
            norm = l2(auth)
        ev.append(f"step: {step} auth={fmt(auth)} hub={fmt(hub)} norm={rf(norm)}")
    return ev

# -------------------------------------------------------------- 059 CELF


def celf_ref(g, k, mc, prob, seed):
    """CELF (Leskovec et al., "Cost-effective outbreak detection in
    networks", KDD 2007, lazy forward) as GDS `influenceMaximization/
    CELF.java` (`greedyPart`: the highest single spread first; `spreads`
    priority queue: higher first, the smaller node on equal cost;
    `lazyForwardPart`: a node whose gain is stale is re-estimated, a fresh
    top is selected with its marginal gain) with lpg's cascade j: edge
    slot e is live when word 2e of stream j of the seed's generator is
    below p (module documentation of `centrality.rs`); a spread counts the
    seed set (GDS `ICInitTask.java`: `active.cardinality()`). GDS
    re-estimates a batch of the queue at once; selection is the same."""
    out, _ = adjacency(g)
    n = g["n"]
    sl = slots(g)
    live_cache = {}

    def live(j, slot):
        key = (j, slot)
        if key not in live_cache:
            x = ChaCha8(seed)
            x.set_stream(0)
            x.set_stream(j)
            x.set_word_pos(2 * slot)
            live_cache[key] = (x.u64() >> 11) * (1.0 / (1 << 53)) < prob
        return live_cache[key]

    def spread(seeds):
        total = 0
        for j in range(mc):
            seen = set()
            q = []
            for s in seeds:
                if s not in seen:
                    seen.add(s)
                    q.append(s)
            head = 0
            while head < len(q):
                u = q[head]
                head += 1
                for x, e in out[u]:
                    if x not in seen and live(j, sl[e]):
                        seen.add(x)
                        q.append(x)
            total += len(q)
        return float(total) / float(mc)

    single = [spread([v]) for v in range(n)]
    ev = [f"single: {fmt(single)}"]
    heap = [(-single[v], v, 0) for v in range(n)]
    heapq.heapify(heap)
    chosen, total = [], 0.0
    while len(chosen) < min(k, n) and heap:
        ng, v, rnd = heapq.heappop(heap)
        gain = -ng
        ev.append(f"pop: {v} {rf(gain)} {rnd}")
        if rnd == len(chosen):
            ev.append(f"select: {v} {rf(gain)}")
            chosen.append(v)
            total += gain
            continue
        new = spread(chosen + [v]) - total
        ev.append(f"gain: {v} {rf(new)}")
        heapq.heappush(heap, (-new, v, len(chosen)))
    return ev, chosen

# ------------------------------------------------------ 105 node similarity


def node_similarity_ref(g, metric="JACCARD", top_k=10, weighted=False, orientation="NATURAL"):
    """GDS `similarity/nodesim/NodeSimilarity.java` (vectors: the distinct
    targets without the node itself; `DegreeComputer`), `algo-params/.../
    nodesim/JaccardSimilarityComputer.java`, `OverlapSimilarityComputer`,
    `CosineSimilarityComputer` (weighted: sum of minima over sum of
    maxima; overlap over the smaller total; cosine of the weights), topK
    by similarity then by target."""
    out, _ = adjacency(g, orientation)
    n = g["n"]
    vec = []
    for v in range(n):
        es = sorted(((x, e) for x, e in out[v] if x != v), key=lambda t: (t[0], t[1]))
        seen, row = set(), []
        for x, e in es:
            if x not in seen:
                seen.add(x)
                row.append((x, weight(g, e) if weighted else 1.0))
        vec.append(row)

    def sim(a, b):
        da, db = dict(a), dict(b)
        if not weighted:
            inter = float(len(set(da) & set(db)))
            if metric == "JACCARD":
                union = float(len(a)) + float(len(b)) - inter
                return 0.0 if union == 0.0 else inter / union
            if metric == "OVERLAP":
                return inter / float(min(len(a), len(b)))
            return inter / (math.sqrt(float(len(a))) * math.sqrt(float(len(b))))
        x = y = z = 0.0
        i = j = 0
        while i < len(a) and j < len(b):
            if a[i][0] == b[j][0]:
                w1, w2 = a[i][1], b[j][1]
                if metric == "JACCARD":
                    x += min(w1, w2); y += max(w1, w2)
                elif metric == "OVERLAP":
                    x += min(w1, w2); y += w1; z += w2
                else:
                    x += w1 * w2; y += w1 * w1; z += w2 * w2
                i += 1; j += 1
            elif a[i][0] < b[j][0]:
                y += a[i][1] * a[i][1] if metric == "COSINE" else a[i][1]
                i += 1
            else:
                if metric == "JACCARD":
                    y += b[j][1]
                elif metric == "OVERLAP":
                    z += b[j][1]
                else:
                    z += b[j][1] * b[j][1]
                j += 1
        for _, w1 in a[i:]:
            y += w1 * w1 if metric == "COSINE" else w1
        for _, w2 in b[j:]:
            if metric == "JACCARD":
                y += w2
            elif metric == "OVERLAP":
                z += w2
            else:
                z += w2 * w2
        if metric == "JACCARD":
            return x / y
        if metric == "OVERLAP":
            return x / min(y, z)
        return x / math.sqrt(y * z)

    ev = []
    adm = [v for v in range(n) if len(vec[v]) >= 1]
    for s in adm:
        lst = []
        for t in adm:
            if t == s:
                continue
            x = sim(vec[s], vec[t])
            if x >= 1e-42:
                lst.append((x, t))
        lst.sort(key=lambda p: (-p[0], p[1]))
        ev.append(f"source: {s} {fmt(lst[:top_k])}")
    return ev

# --------------------------------------------------------------- 106 KNN


def knn_ref(g, top_k, sample_rate, joins, maxit, delta, seed, perturbation=0.0):
    """NN-descent (Dong, Moses and Li, WWW 2011) as MAGE `cpp/knn_module/
    algorithms/knn.hpp` (`CalculateKNN`: old and sampled new neighbours,
    reverse samples, joins new x new and new x old, stop below
    delta x n x topK updates), with lpg's deterministic changes (module
    documentation of `similarity.rs`), GDS's random joins (`JoinNeighbors.
    java` `randomJoins`) and GDS's perturbation (`NeighborList.java`
    `add`:107-115: only a neighbour of equal similarity to the worst may
    replace it); a scalar property is compared by 1 / (1 + |a - b|) (GDS
    `similarity/knn/metrics`)."""
    n = g["n"]
    xs = [c[0] for c in COORDS[:n]]
    sim = lambda a, b: 1.0 / (1.0 + abs(xs[a] - xs[b]))
    gens = [rng(seed, b) for b in range(n)]
    rho = max(1, int(sample_rate * float(top_k)))
    targets = list(range(n))

    def sample(gg, frm, count):
        pool = list(frm)
        count = min(count, len(pool))
        for i in range(count):
            j = gg.range(i, len(pool))
            pool[i], pool[j] = pool[j], pool[i]
        return pool[:count]

    def update(lst, idx, s, force):
        if any(x["id"] == idx for x in lst):
            return False
        pos = next((i for i, x in enumerate(lst) if s > x["sim"] or (s == x["sim"] and idx < x["id"])), len(lst))
        if pos >= top_k:
            if not force or not lst or s != lst[-1]["sim"]:
                return False
            lst.pop()
            lst.append(dict(id=idx, sim=s, new=True))
            return True
        lst.insert(pos, dict(id=idx, sim=s, new=True))
        del lst[top_k:]
        return True

    ev = []
    lists = [[] for _ in range(n)]
    for v in range(n):
        rest = [t for t in targets if t != v]
        picked = sample(gens[v], rest, top_k)
        ev.append((f"init: {v} {fmt(picked)}", v))
        for t in picked:
            update(lists[v], t, sim(v, t), False)
    perturb = rng(seed, n)
    for ran in range(1, maxit + 1):
        olds, news = [], []
        for v in range(n):
            lst = lists[v]
            fresh = [i for i in range(len(lst)) if lst[i]["new"]]
            old = [x["id"] for x in lst if not x["new"]]
            new = []
            for i in sample(gens[v], fresh, rho):
                new.append(lst[i]["id"])
                lst[i]["new"] = False
            ev.append((f"split: {v} old={fmt(old)} new={fmt(new)}", v))
            olds.append(old)
            news.append(new)
        old_rev = [[] for _ in range(n)]
        new_rev = [[] for _ in range(n)]
        for v in range(n):
            for u in olds[v]:
                old_rev[u].append(v)
            for u in news[v]:
                new_rev[u].append(v)
        props = []
        for v in range(n):
            gg = gens[v]
            old = list(olds[v])
            for u in sample(gg, old_rev[v], rho):
                if u not in old:
                    old.append(u)
            new = list(news[v])
            for u in sample(gg, new_rev[v], rho):
                if u not in new:
                    new.append(u)
            ev.append((f"join: {v} old={fmt(old)} new={fmt(new)}", v))
            for i in range(len(new)):
                for j in range(i + 1, len(new)):
                    s = sim(new[i], new[j])
                    props.append((new[i], new[j], s)); props.append((new[j], new[i], s))
                for o in old:
                    if o != new[i]:
                        s = sim(new[i], o)
                        props.append((new[i], o, s)); props.append((o, new[i], s))
            for _ in range(joins):
                u = targets[gg.range(0, len(targets))]
                ev.append((f"random: {v} {u}", v))
                if u != v:
                    props.append((v, u, sim(v, u)))
        updates = 0
        for a, b, s in props:
            if a == b:
                continue
            force = perturbation > 0.0 and perturb.f64() < perturbation
            if update(lists[a], b, s, force):
                ev.append((f"update: {a} {b} {rf(s)}", None))
                updates += 1
        ev.append((f"round: {ran} {updates} {fmt([[x['id'] for x in l] for l in lists])}", None))
        if float(updates) < delta * float(top_k) * float(n):
            break
    return take(ev), lists

# -------------------------------------------- the community net (061-065, 078)


class Net:
    """lpg's `community::Net` (module documentation of `community.rs`):
    the weighted undirected graph of a run; parallel edges added, a
    self-loop 2w under NATURAL, w per appearance under UNDIRECTED."""

    def __init__(self, adj, selfw):
        self.adj, self.selfw = adj, selfw
        self.k = [rsum(w for _, w in adj[v]) + selfw[v] for v in range(len(adj))]
        self.two_m = rsum(self.k)

    @staticmethod
    def from_lists(lists, selfw):
        adj = []
        for l in lists:
            l = sorted(l, key=lambda x: x[0])
            m = []
            for v, x in l:
                if m and m[-1][0] == v:
                    m[-1] = (v, m[-1][1] + x)
                else:
                    m.append((v, x))
            adj.append(m)
        return Net(adj, selfw)

    @staticmethod
    def of(g, orientation="NATURAL", weighted=False):
        out, _ = adjacency(g, orientation)
        n = g["n"]
        lf = 1.0 if orientation == "UNDIRECTED" else 2.0
        lists = [[] for _ in range(n)]
        selfw = [0.0] * n
        for u in range(n):
            for v, e in out[u]:
                w = weight(g, e) if weighted else 1.0
                if u == v:
                    selfw[u] += lf * w
                else:
                    lists[u].append((v, w))
                    lists[v].append((u, w))
        return Net.from_lists(lists, selfw)

    def __len__(self):
        return len(self.adj)

    def colours(self):
        """Greedy colouring in index order (the smallest free colour)."""
        col = [None] * len(self)
        classes = []
        for v in range(len(self)):
            used = {col[u] for u, _ in self.adj[v] if col[u] is not None}
            c = 0
            while c in used:
                c += 1
            col[v] = c
            while len(classes) <= c:
                classes.append([])
            classes[c].append(v)
        return classes

    def modularity(self, part, gamma=1.0):
        """NetworkX `community/quality.py` `modularity`, in lpg's order."""
        if self.two_m == 0.0:
            return 0.0
        inside, tot = {}, {}
        for u in range(len(self)):
            tot[part[u]] = tot.get(part[u], 0.0) + self.k[u]
            x = self.selfw[u]
            for v, w in self.adj[u]:
                if part[v] == part[u]:
                    x += w
            inside[part[u]] = inside.get(part[u], 0.0) + x
        return rsum(inside[c] / self.two_m - gamma * (t / self.two_m) * (t / self.two_m)
                    for c, t in sorted(tot.items()))

    def aggregate(self, part, count):
        lists = [[] for _ in range(count)]
        selfw = [0.0] * count
        for u in range(len(self)):
            cu = part[u]
            selfw[cu] += self.selfw[u]
            for v, w in self.adj[u]:
                cv = part[v]
                if cu == cv:
                    selfw[cu] += w
                else:
                    lists[cu].append((cv, w))
        return Net.from_lists(lists, selfw)


def best_move(net, part, tot, v, gamma, check=True):
    """The move rule of GDS `modularityoptimization/ModularityOptimizationTask.
    java` `run` (the community of greatest positive gain against the
    community weights before the colour, the lowest community on a tie),
    in lpg's arithmetic; `check` asserts that GDS's own arithmetic decides
    the same on the trace graphs (no near tie)."""
    own, kv = part[v], net.k[v]
    links = sorted([(part[u], w) for u, w in net.adj[v]], key=lambda x: x[0])
    scale = gamma * kv / net.two_m
    own_link = rsum(w for c, w in links if c == own)
    stay = own_link - scale * (tot[own] - kv)
    best, bc = stay, None
    i = 0
    groups = []
    while i < len(links):
        c, x = links[i][0], 0.0
        while i < len(links) and links[i][0] == c:
            x += links[i][1]
            i += 1
        groups.append((c, x))
        if c == own:
            continue
        gain = x - scale * tot[c]
        if gain > best:
            best, bc = gain, c
    if check and gamma == 1.0:
        # GDS: (eiy - eix) / (m) + 2 k (ax - ay) / (2m)^2 > 0, lowest c.
        eix = dict(groups).get(own, 0.0)
        ax = tot[own] - kv
        mg, nc = 0.0, own
        for c, x in sorted(groups):
            if c == own:
                continue
            gg = (x - eix) / (net.two_m / 2.0) + (2 * kv * ax - 2 * kv * tot[c]) / net.two_m ** 2
            if gg > mg or (gg == mg and gg != 0.0 and nc > c):
                mg, nc = gg, c
        assert (None if nc == own else nc) == bc, ("GDS and lpg arithmetic differ", v, part)
    return bc


def local_move(net, part, gamma, max_rounds, tol, ev, sequential=False):
    """Colour-class rounds (Lu, Halappanavar and Kalyanaraman, Parallel
    Computing 47, 2015) as GDS `ModularityOptimization.java` `optimizeColor`
    (each node of a colour decides from the communities and community
    weights before the colour; the weight updates are applied after it),
    with lpg's greedy colouring; `sequential` applies each node's move at
    once instead (Blondel et al. 2008), for comparison."""
    n = len(net)
    tot = [0.0] * n
    for v in range(n):
        tot[part[v]] += net.k[v]
    classes = net.colours()
    ev.append(f"colours: {fmt(classes)}")
    q = net.modularity(part, gamma)
    moved_any = False
    for rnd in range(1, max_rounds + 1):
        moved = 0
        for ci, cls in enumerate(classes):
            if sequential:
                moves = []
                for v in cls:
                    c = best_move(net, part, tot, v, gamma, check=False)
                    if c is not None:
                        tot[part[v]] -= net.k[v]; tot[c] += net.k[v]; part[v] = c
                        moves.append((v, c))
            else:
                snap, tsnap = list(part), list(tot)
                moves = [(v, c) for v in cls for c in [best_move(net, snap, tsnap, v, gamma)] if c is not None]
                for v, c in moves:
                    tot[part[v]] -= net.k[v]; tot[c] += net.k[v]; part[v] = c
            ev.append(f"class: {rnd} {ci} {fmt(moves)}")
            moved += len(moves)
        ev.append(f"round: {rnd} {fmt(part)}")
        if moved == 0:
            ev.append(f"stop: {rnd} no move")
            return rnd, moved_any
        moved_any = True
        nq = net.modularity(part, gamma)
        gain, q = nq - q, nq
        if gain < tol:
            ev.append(f"stop: {rnd} gain")
            return rnd, moved_any
    ev.append(f"stop: {max_rounds} max")
    return max_rounds, moved_any


def renumber(part):
    m = {}
    for i, c in enumerate(part):
        part[i] = m.setdefault(c, len(m))
    return len(m)


def louvain_ref(g, orientation="NATURAL", max_levels=10, rounds=10, tol=1e-4):
    """Louvain (Blondel et al., J. Stat. Mech. 2008): local moving, then
    each community aggregated into one node (GDS `louvain/Louvain.java`
    runs `ModularityOptimization` per level), level after level until a
    level moves nothing, gains less than tolerance, or maxLevels."""
    net = Net.of(g, orientation)
    n = g["n"]
    part = list(range(n))
    node_of = list(range(n))
    ev, levels, mods = [], [], []
    while len(levels) < max_levels:
        _, moved = local_move(net, part, 1.0, rounds, tol, ev)
        c = renumber(part)
        if not moved and levels:
            break
        q = net.modularity(part)
        member = [part[node_of[v]] for v in range(n)]
        gain = q - (mods[-1] if mods else -math.inf)
        ev.append(f"level: {len(levels)} {fmt(member)}")
        levels.append(member)
        mods.append(q)
        if c == len(net) or gain < tol:
            break
        node_of = [part[x] for x in node_of]
        net = net.aggregate(part, c)
        part = list(range(c))
    return ev, levels


def approx_exp(x):
    """network_partitions `leiden/subnetwork.rs:262-277`."""
    if x < -256.0:
        return 0.0
    y = 1.0 + x / 256.0
    for _ in range(8):
        y *= y
    return y


def subnetwork(net, part, members, res, randomness, gg):
    """network_partitions `leiden/subnetwork.rs:44-148`
    (`subnetwork_clustering`: a random order by swaps, `node_can_move`,
    `best_cluster_for_node`: the qvi of each well-connected neighbouring
    cluster, a cluster drawn with weight approximate_exp(qvi / theta),
    the external weights updated) and `quality_value_increment.rs`
    (`as_reference_impl`)."""
    ln = len(members)
    clustering = list(range(ln))
    if ln == 1:
        return clustering
    loc = {v: i for i, v in enumerate(members)}
    c0 = part[members[0]]
    adj = [[(loc[u], w) for u, w in net.adj[v] if part[u] == c0 and u in loc] for v in members]
    order = list(range(ln))
    for i in range(ln):
        j = gg.range(0, ln)
        order[i], order[j] = order[j], order[i]
    nw = [net.k[v] for v in members]
    cw, ext = list(nw), list(nw)
    total = rsum(nw)
    single = [True] * ln
    nbw = [0.0] * ln
    for node in order:
        if not (single[node] and ext[node] >= cw[node] * (total - cw[node]) * res):
            continue
        nbw = [0.0] * ln
        neigh = [node]
        summed = []
        cw[node] = 0.0
        ext[node] = 0.0
        for u, w in adj[node]:
            c = clustering[u]
            if nbw[c] == 0.0:
                neigh.append(c)
            nbw[c] += w
        best, mq, tadj = node, 0.0, 0.0
        for c in neigh:
            if ext[c] >= cw[c] * (total - cw[c]) * res:
                qvi = nbw[c] - nw[node] * cw[c] * res
                if qvi > mq:
                    best, mq = c, qvi
                if qvi >= 0.0:
                    tadj += approx_exp(qvi / randomness)
            if not math.isnan(tadj):
                summed.append(tadj)
            nbw[c] = 0.0
        if math.isfinite(tadj):
            x = tadj * gg.f64()
            at = bisect.bisect_left(summed, x)
            chosen = neigh[min(at, len(neigh) - 1)]
        else:
            chosen = best
        cw[chosen] += nw[node]
        for u, w in adj[node]:
            if clustering[u] == chosen:
                ext[chosen] -= w
            else:
                ext[chosen] += w
        if chosen != node:
            clustering[node] = chosen
            single[chosen] = False
    first = {}
    return [first.setdefault(c, i) for i, c in enumerate(clustering)]


def leiden_ref(g, seed, gamma=1.0, theta=0.01, max_levels=10, tol=1e-4):
    """Leiden (Traag, Waltman and van Eck, Sci. Rep. 9, 2019): local
    moving, refinement of each community (`subnetwork`, one generator
    stream per level and community), aggregation by the refined
    communities whose nodes start in their unrefined community
    (network_partitions `leiden/leiden_clustering.rs:150-226`
    `improve_clustering_recursive`); UNDIRECTED (TS-25 U)."""
    net = Net.of(g, "UNDIRECTED")
    n = g["n"]
    part = list(range(n))
    node_of = list(range(n))
    ev, levels, mods = [], [], []
    while len(levels) < max_levels:
        _, moved = local_move(net, part, gamma, 1 << 62, 0.0, ev)
        c = renumber(part)
        if not moved and levels:
            break
        q = net.modularity(part, gamma)
        gain = q - (mods[-1] if mods else -math.inf)
        levels.append([part[node_of[v]] for v in range(n)])
        ev.append(f"level: {len(levels) - 1} {fmt(levels[-1])}")
        mods.append(q)
        if gain < tol and len(levels) > 1:
            break
        members = [[] for _ in range(c)]
        for v in range(len(net)):
            members[part[v]].append(v)
        res = gamma / net.two_m
        refined = [0] * len(net)
        for cc in range(c):
            gg = ChaCha8(seed)
            gg.set_stream(0)
            gg.set_stream(((len(levels) - 1) << 40) | cc)
            local = subnetwork(net, part, members[cc], res, theta, gg)
            for i, v in enumerate(members[cc]):
                refined[v] = members[cc][local[i]]
        rc = renumber(refined)
        ev.append(f"refine: {len(levels) - 1} {fmt(refined)}")
        if rc == len(net):
            break
        nxt = [0] * rc
        for v in range(len(net)):
            nxt[refined[v]] = part[v]
        node_of = [refined[x] for x in node_of]
        net = net.aggregate(refined, rc)
        part = nxt
    return ev, levels

# ----------------------------------------------------- 063 LPA, 064 SLLPA


def mix(z):
    """SplitMix64's finaliser (lpg's LPA tie order)."""
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def rotl64(x, n):
    return ((x << n) | (x >> (64 - n))) & M64


def lpa_ref(g, orientation="NATURAL", maxit=10, rule="lpg", order="classes", labels=None, live=False):
    """Label propagation (Raghavan, Albert and Kumara, Phys. Rev. E 76,
    2007): a node takes the label of greatest weight among its
    neighbours'. `rule` "lpg": a tie keeps the current label if it is
    among the best, else the best whose hash `mix` is least (lpg's rule;
    the NetworkX `asyn_lpa_communities` rule with a deterministic choice);
    "gds": the lowest label (GDS `labelpropagation/ComputeStepConsumer.java`
    `tallyVotes`). `order` "classes": lpg's colour classes, each node of a
    class reading the labels before the class; "gds": in node order, each
    update seen at once (GDS `ComputeStep.java` `compute`, one batch).
    `live`: the nodes of a class read each other's new labels (the
    sequential application to the class). Returns the events, the rounds
    and the labels."""
    out, _ = adjacency(g, orientation)
    n = g["n"]
    label = list(labels) if labels is not None else list(range(n))
    cls = Net.of(g, orientation).colours() if order == "classes" else [list(range(n))]
    ev = [f"colours: {fmt(cls)}"] if order == "classes" else []
    ran = 0
    while ran < maxit:
        ran += 1
        rnd = rotl64(ran, 48)
        changed = 0
        for ci, c in enumerate(cls):
            snap = list(label) if order == "classes" and not live else label
            ups = []
            for v in c:
                votes = sorted([(snap[u], 1.0) for u, _ in out[v]], key=lambda x: x[0])
                if not votes:
                    continue
                tally = []
                for l, x in votes:
                    if tally and tally[-1][0] == l:
                        tally[-1] = (l, tally[-1][1] + x)
                    else:
                        tally.append((l, x))
                mx = max(t[1] for t in tally)
                best = [t[0] for t in tally if t[1] == mx]
                if rule == "gds":
                    pick = min(best)
                    if pick == snap[v]:
                        continue
                else:
                    if snap[v] in best:
                        continue
                    pick = min(best, key=lambda l: (mix(l ^ rotl64(v, 32) ^ rnd), l))
                ups.append((v, pick))
                if order == "gds" or live:
                    label[v] = pick
            if order == "classes":
                ev.append(f"class: {ran} {ci} {fmt(ups)}")
                for v, l in ups:
                    label[v] = l
            changed += len(ups)
        if order == "classes":
            ev.append(f"round: {ran} {fmt(label)}")
        if changed == 0:
            return ev, ran, label, True
    return ev, ran, label, False


def sllpa_ref(g, steps, seed, live=False):
    """SLPA (Xie, Szymanski and Liu, 2011) as GDS `sllpa/
    SpeakerListenerLPAComputation.java` (`compute`: memory of maxIterations
    slots filled with the node's own label; a listener hears one label per
    incoming edge, each a uniformly drawn slot the speaker has written, and
    writes the most frequent) with lpg's colour-class order and generator
    (module documentation of `community.rs`)."""
    _, inc = adjacency(g)
    n = g["n"]
    mem = [[v] * steps for v in range(n)]
    written = [1] * n
    gens = [rng(seed, b) for b in range(n)]
    cls = Net.of(g).colours()
    ev = []
    for step in range(1, steps):
        for c in cls:
            heard = []
            for v in c:
                gg = gens[v]
                ls = [mem[u][gg.range(0, written[u])] for u, _ in inc[v]]
                if not ls:
                    heard.append((v, None))
                    continue
                ev.append(f"hear: {step} {v} {fmt(ls)}")
                ls.sort()
                best, most, i = [], 0, 0
                while i < len(ls):
                    j = i
                    while j < len(ls) and ls[j] == ls[i]:
                        j += 1
                    if j - i > most:
                        most, best = j - i, []
                    if j - i == most:
                        best.append(ls[i])
                    i = j
                l = best[0] if len(best) == 1 else best[gg.range(0, len(best))]
                heard.append((v, l))
                if live:
                    mem[v][step] = l
                    written[v] = step + 1
            for v, l in heard:
                if l is not None:
                    mem[v][step] = l
                written[v] = step + 1
            ev.append(f"memory: {step} {fmt([mem[v][step] for v in range(n)])}")
    return ev

# --------------------------------------------------------- 078 max k-cut


def kcut_ref(g, k, restarts, seed, live=False):
    """GRASP (Feo and Resende 1995) as lpg's 078 (module documentation of
    `community.rs`): a random order (Fisher-Yates on stream 0), greedy
    placement in the community of least placed-neighbour weight, colour-
    class local search (a node's best gain depends only on its neighbours,
    none of which share its class), then Fiduccia-Mattheyses passes
    (Fiduccia and Mattheyses, DAC 1982: each node moves once, the best
    move first, the best prefix kept)."""
    net = Net.of(g)
    n = len(net)
    cls = net.colours()
    gg = rng(seed, 0)
    ev = []
    best = None

    def cut(part):
        x = 0.0
        for u in range(n):
            for v, w in net.adj[u]:
                if u < v and part[u] != part[v]:
                    x += w
        return x

    def best_of(part, v):
        to = [0.0] * k
        for u, w in net.adj[v]:
            to[part[u]] += w
        own = part[v]
        b = (-math.inf, own)
        for c, x in enumerate(to):
            if c != own and to[own] - x > b[0]:
                b = (to[own] - x, c)
        return b

    def fm(part):
        version, locked = [0] * n, [False] * n
        heap = []
        for v in range(n):
            gn, c = best_of(part, v)
            heapq.heappush(heap, (-gn, v, c, 0))
        moves, total, bt, bl = [], 0.0, 0.0, 0
        while heap:
            ng, v, c, ver = heapq.heappop(heap)
            gn = -ng
            if locked[v] or ver != version[v] or c == part[v] or not math.isfinite(gn):
                continue
            own = part[v]
            part[v] = c
            locked[v] = True
            moves.append((v, own))
            total += gn
            if total > bt:
                bt, bl = total, len(moves)
            for u, _ in net.adj[v]:
                if not locked[u]:
                    version[u] += 1
                    g2, c2 = best_of(part, u)
                    heapq.heappush(heap, (-g2, u, c2, version[u]))
        for v, own in reversed(moves[bl:]):
            part[v] = own
        return bt

    for _ in range(restarts):
        perm = list(range(n))
        for i in range(n):
            j = gg.range(i, n)
            perm[i], perm[j] = perm[j], perm[i]
        ev.append(f"perm: {fmt(perm)}")
        part = [None] * n
        for v in perm:
            to = [0.0] * k
            for u, w in net.adj[v]:
                if part[u] is not None:
                    to[part[u]] += w
            c = 0
            for i, x in enumerate(to):
                if x < to[c]:
                    c = i
            part[v] = c
        ev.append(f"construct: {fmt(part)}")
        while True:
            moved = 0
            for c in cls:
                snap = part if live else list(part)
                for v in c:
                    to = [0.0] * k
                    for u, w in net.adj[v]:
                        to[snap[u]] += w
                    own = snap[v]
                    b = (0.0, own)
                    for cc, x in enumerate(to):
                        if cc != own and to[own] - x > b[0]:
                            b = (to[own] - x, cc)
                    if b[1] != own:
                        part[v] = b[1]
                        moved += 1
            if moved == 0:
                break
        ev.append(f"search: {fmt(part)}")
        while fm(part) > 0.0:
            ev.append(f"fm: {fmt(part)}")
        value = cut(part)
        ev.append(f"cut: {rf(value)}")
        if best is None or value > best[0]:
            best = (value, list(part))
    return ev, best[1]

# ---------------------------------------------------------- 080 k-means


def sqd(a, b):
    return rsum((x - y) * (x - y) for x, y in zip(a, b))


def kmeans_ref(pts, k, maxit, delta, seed, sampler="UNIFORM", restarts=1):
    """Lloyd's iterations as GDS `kmeans/Kmeans.java` `kMeans` (assign each
    node to its closest centroid, the first on a tie: `ClusterManager.
    findClosestCentroid`; recompute the centroids; stop after maxIterations
    or, from the second iteration, at most (long) (n x deltaThreshold)
    swaps: `KmeansIterationStopper.shouldQuit`; keep the last assignment and
    measure its distance to the final centroids: `KmeansTask.
    calculateFinalDistance`; the restart of least average distance wins:
    `updateBestSolution`). Initial centroids: `UNIFORM` k distinct points
    (lpg: a partial Fisher-Yates shuffle on stream 0), `KMEANS++` D^2
    sampling (linfa-clustering `k_means/init.rs`
    `weighted_k_means_plusplus`)."""
    n, d = len(pts), len(pts[0])
    gg = rng(seed, 0)
    ev = []
    best = None
    for _ in range(restarts):
        if sampler == "UNIFORM":
            pool = list(range(n))
            cent = []
            for i in range(k):
                j = gg.range(i, n)
                pool[i], pool[j] = pool[j], pool[i]
                cent.append(list(pts[pool[i]]))
        else:
            cent = [list(pts[gg.range(0, n)])]
            dist = [sqd(p, cent[0]) for p in pts]
            for c in range(1, k):
                total = rsum(dist)
                if total > 0.0:
                    r = gg.f64() * total
                    acc, pick = 0.0, n - 1
                    for i, x in enumerate(dist):
                        acc += x
                        if acc > r:
                            pick = i
                            break
                else:
                    pick = 0
                cent.append(list(pts[pick]))
                dist = [min(x, sqd(pts[i], cent[c])) for i, x in enumerate(dist)]
        ev.append(f"init: {fmt([x for c in cent for x in c])}")
        assign = [None] * n
        for it in range(1, maxit + 1):
            sums = [[0.0] * d for _ in range(k)]
            counts = [0] * k
            changed = 0
            for i, p in enumerate(pts):
                bc, bd = 0, math.inf
                for j in range(k):
                    dd = sqd(p, cent[j])
                    if dd < bd:
                        bd, bc = dd, j
                if assign[i] != bc:
                    changed += 1
                    assign[i] = bc
                counts[bc] += 1
                for t in range(d):
                    sums[bc][t] += p[t]
            for j in range(k):
                if counts[j]:
                    for t in range(d):
                        cent[j][t] = sums[j][t] / float(counts[j])
            ev.append(f"iteration: {it} {fmt(assign)} {changed} {fmt([x for c in cent for x in c])}")
            if it > 1 and changed <= int(float(n) * delta):
                break
        total = 0.0
        for i, p in enumerate(pts):
            total += math.sqrt(sqd(p, cent[assign[i]]))
        score = total / float(n)
        ev.append(f"restart: {fmt(assign)} {rf(score)}")
        if best is None or score < best[0]:
            best = (score, list(assign), [list(c) for c in cent])
    return ev, best[1], best[2]

# ---------------------------------------------------------- 081 HDBSCAN


def hdbscan_ref(pts, min_size, samples):
    """hdbscan crate: `core_distances/serial.rs:72-81` (the samples-th
    smallest distance, the point itself included), `min_spanning_tree.rs:
    76-103` (Prim from node 0 on mutual-reachability distances, sorted),
    `hdbscan.rs:293-318` (`make_single_linkage_tree`), `:320-408`
    (`condense_tree`), `:486-536` (`extract_winning_clusters`, excess of
    mass), `:729-787` (`label_data`)."""
    n = len(pts)
    dist = lambda a, b: math.sqrt(sqd(pts[a], pts[b]))
    core = [sorted(dist(i, j) for j in range(n))[samples - 1] for i in range(n)]
    ev = [f"core: {fmt(core)}"]
    mrd = lambda a, b: max(max(core[a], core[b]), dist(a, b))
    in_tree = [False] * n
    dd = [math.inf] * n
    mst = []
    left = right = 0
    for _ in range(1, n):
        in_tree[left] = True
        cur = math.inf
        for i in range(n):
            if in_tree[i]:
                continue
            m = mrd(left, i)
            if m < dd[i]:
                dd[i] = m
            if dd[i] < cur:
                right, cur = i, dd[i]
        mst.append((left, right, cur))
        left = right
    mst.sort(key=lambda e: e[2])
    ev.append(f"mst: {fmt(mst)}")
    parent = [None] * (2 * n - 1)
    size = [1 if i < n else 0 for i in range(2 * n - 1)]
    nxt = n

    def find(x):
        root = x
        while parent[root] is not None:
            root = parent[root]
        while x != root and parent[x] != root:
            p = parent[x]
            parent[x] = root
            x = p
        return root
    slt = []
    for a, b, d in mst:
        l, r = find(a), find(b)
        slt.append((l, r, d, size[l] + size[r]))
        parent[l] = parent[r] = nxt
        size[nxt] = size[l] + size[r]
        nxt += 1

    def children(root):
        q, out_ = [root], []
        while q:
            c = q.pop(0)
            out_.append(c)
            if c >= n:
                q.append(slt[c - n][0]); q.append(slt[c - n][1])
        return out_
    sz = lambda c: 1 if c < n else slt[c - n][3]
    top = (n - 1) * 2
    new_ids = [0] * (top + 1)
    new_ids[top] = n
    next_parent = n + 1
    visited = [False] * (top + 1)
    tree = []
    for c in children(top):
        if visited[c] or c < n:
            continue
        l, r, d, _ = slt[c - n]
        lam = 1.0 / d if d > 0.0 else math.inf
        ls, rs = sz(l), sz(r)

        def add(child, par):
            for x in children(child):
                if x < n:
                    tree.append((x, par, lam, 1))
                visited[x] = True
        if ls >= min_size and rs >= min_size:
            for child, s in ((l, ls), (r, rs)):
                new_ids[child] = next_parent
                next_parent += 1
                tree.append((new_ids[child], new_ids[c], lam, s))
        elif ls < min_size and rs < min_size:
            add(l, new_ids[c]); add(r, new_ids[c])
        elif rs >= min_size:
            new_ids[r] = new_ids[c]
            add(l, new_ids[c])
        else:
            new_ids[l] = new_ids[c]
            add(r, new_ids[c])
    ev.append(f"condensed: {fmt(tree)}")
    lower = n + 1
    upper = lower + max(0, len(tree) - n)

    def birth(c):
        for t in tree:
            if t[0] == c:
                return t[2]
        return 0.0
    stab = {c: rsum((t[2] - birth(c)) * float(t[3]) for t in tree if t[1] == c) for c in range(lower, upper)}
    sel = {c: False for c in range(lower, upper)}

    def child_clusters(root):
        q, out_ = [root], []
        while q:
            cur = q.pop(0)
            for t in tree:
                if t[0] >= n and t[1] == cur:
                    out_.append(t[0]); q.append(t[0])
        return out_
    for c in range(upper - 1, lower - 1, -1):
        child = rsum(stab[t[0]] for t in tree if t[1] == c and t[0] >= n)
        if stab[c] > child:
            sel[c] = True
            for x in child_clusters(c):
                sel[x] = False
        else:
            stab[c] = child
    winners = [c for c in range(lower, upper) if sel[c]]
    ev.append(f"selected: {fmt(winners)} {fmt([stab[c] for c in range(lower, upper)])}")
    labels = [-1] * n
    for lab, c in enumerate(winners):
        q = [c]
        while q:
            cur = q.pop(0)
            for t in tree:
                if t[1] != cur:
                    continue
                if t[0] >= n:
                    q.append(t[0])
                else:
                    labels[t[0]] = lab
    return ev, labels

# ------------------------------------------------- 076, 077 spectral inputs


def rounded(x):
    if abs(x) < 5e-7:
        return 0.0
    y = x * 1e6
    return math.copysign(math.floor(abs(y) + 0.5), y) / 1e6


def spectral_ref(g, normalised, e):
    """The spectral embedding of cuGraph `spectral_clustering.cu:36,110`
    (method only): the normalised Laplacian I - D^-1/2 A D^-1/2 (Ng, Jordan
    and Weiss 2001) or D - A (von Luxburg 2007) of the undirected net, its
    eigenvalues ascending and eigenvectors 2..e+1 (here NumPy's LAPACK
    `eigh`; compared rounded to 1e-6, each eigenvector up to its sign)."""
    net = Net.of(g, "UNDIRECTED", weighted=False)
    n = len(net)
    deg = net.k
    A = lambda u, v: net.selfw[u] if u == v else dict(net.adj[u]).get(v, 0.0)
    m = []
    for u in range(n):
        for v in range(n):
            a = A(u, v)
            l = deg[u] - a if u == v else -a
            if normalised:
                if deg[u] > 0.0 and deg[v] > 0.0:
                    m.append(l / math.sqrt(deg[u] * deg[v]))
                elif u == v:
                    m.append(1.0)
                else:
                    m.append(0.0)
            else:
                m.append(l)
    ev = [f"matrix: {fmt(m)}"]
    vals, vecs = np.linalg.eigh(np.array(m).reshape(n, n))
    vals = [float(x) for x in vals]
    assert all(vals[i] < vals[i + 1] - 1e-6 for i in range(n - 1)), "simple eigenvalues"
    ev.append(f"eigenvalues: {fmt([rounded(x) for x in vals])}")
    for col in range(1, e + 1):
        v = [float(x) for x in vecs[:, col]]
        sign = next((math.copysign(1.0, x) for x in v if abs(x) > 1e-6), 1.0)
        ev.append(f"eigenvector: {fmt([rounded(sign * x) for x in v])}")
    return ev

# ------------------------------------------------------ 082 partition quality


def counts_ref(g, comm, orientation="NATURAL"):
    """GDS `modularity/RelationshipCountCollector.java` `run` (per node:
    every relationship adds its weight to the community's total, and to
    its inside weight when both ends share the community) and
    `ModularityCalculator.java:82-95` ((e_c - K_c^2 / m) / m)."""
    out, _ = adjacency(g, orientation)
    m, total = {}, 0.0
    for v in range(g["n"]):
        c = comm[v]
        if c is None:
            continue
        e = m.setdefault(c, (0.0, 0.0))
        t = 0.0
        for u, _ in out[v]:
            e = (e[0] + 1.0, e[1] + 1.0) if comm[u] == c else (e[0], e[1] + 1.0)
            t += 1.0
        m[c] = e
        total += t
    return [f"counts: {fmt(m)} {rf(total)}"], m, total

# ---------------------------------------------------------------- cases


def case(graph, proc, config=None, args=None, kinds=None, want=None, orientation=None,
         concurrency=(1, 16), rows=None, **extra):
    c = dict(config or {})
    if orientation:
        c["orientation"] = orientation
    want = [e for e in want if e.split(":")[0] in kinds]
    out = dict(graph=graph, proc=proc, args=args or [], config=c, kinds=kinds, want=want,
               concurrency=list(concurrency))
    if rows is not None:
        out["rows"] = rows
    out.update(extra)
    return out


def N(i):
    return {"node": i}


def fixture(name):
    import csv
    rows = list(csv.reader(open(os.path.join(T, name, "edges.csv"))))[1:]
    n = len(list(csv.reader(open(os.path.join(T, name, "nodes.csv"))))) - 1
    return n, [(int(r[0]), int(r[1])) for r in rows]


def relabel(n, edges, key):
    """The fixture under a node numbering: rank of each uid under `key`."""
    rank = {v: i for i, v in enumerate(sorted(range(n), key=key))}
    return dict(n=n, edges=[(rank[a], rank[b], "R", None) for a, b in edges]), rank


def analysis():
    """063 and 065 round counts against GDS on the lab fixtures (rmp #65,
    #66 inputs): GDS runs on the UNDIRECTED projection (`oracle-cases/
    gds_cases_65_66.sh` `community`); GDS numbers nodes by import order
    (the numeric uid), lpg by element identifier (the string order of
    `id:ID`, measured in pass A)."""
    out = {}
    for name in ("rc24", "karate", "planted60"):
        n, edges = fixture(name)
        stats = json.loads(open(os.path.join(T, "expected", f"{name}.gds.lpa.stats.jsonl")).readline())
        mo = json.loads(open(os.path.join(T, "expected", f"{name}.gds.mo.stats.jsonl")).readline())
        row = dict(gds_lpa=[stats["ranIterations"], stats["communityCount"]],
                   gds_mo=[mo["ranIterations"], mo["communityCount"]])
        for numbering, key in (("gds", lambda x: x), ("lpg", lambda x: str(x))):
            g, rank = relabel(n, edges, key)
            for rule in ("gds", "lpg"):
                for order in ("gds", "classes"):
                    _, ran, lab, _ = lpa_ref(g, "UNDIRECTED", 10, rule, order)
                    row[f"lpa {numbering} numbering, {rule} tie, {order} order"] = [ran, len(set(lab))]
            # Modularity optimisation: lpg's colour classes and GDS's stop
            # rule (iteration 0 never stops; then stop unless modularity
            # rose by more than tolerance).
            net = Net.of(g, "UNDIRECTED")
            part = list(range(n))
            ev = []
            rounds, _ = local_move(net, part, 1.0, 10, 1e-4, ev)
            qs = []
            # Replay round by round to get the modularity after each.
            for r in range(1, 11):
                p2 = list(range(n))
                local_move(net, p2, 1.0, r, -math.inf, [])
                qs.append(net.modularity(p2))
            q0 = net.modularity(list(range(n)))
            gds_iter = None
            prev = q0
            for i, q in enumerate(qs):
                if i > 0 and not (q > prev and abs(q - prev) > 1e-4):
                    gds_iter = i + 1
                    break
                prev = q
            row[f"mo {numbering} numbering, lpg classes"] = dict(
                lpg_rounds=rounds, communities=len(set(part)), gds_stop_rule_rounds=gds_iter)
        out[name] = row
    return out


def main():
    G = GRAPHS
    cases = {}
    par = (1, 16)
    pr = G["pr"]
    # 042 PageRank, 043 personalised, 044 ArticleRank, 040 eigenvector.
    for label, cfg, kw in (
            ("042", {}, {}),
            ("042 weighted", {"relationshipWeightProperty": "weight"}, dict(weighted=True)),
            ("042 tolerance 0.01", {"tolerance": 0.01}, dict(tol=0.01)),
            ("042 REVERSE", {"orientation": "REVERSE"}, dict(orientation="REVERSE")),
            ("042 UNDIRECTED", {"orientation": "UNDIRECTED"}, dict(orientation="UNDIRECTED")),
            ("043", {"sourceNodes": [N(0)]}, dict(sources=(0,))),
            ("044", {}, dict(variant="ar")),
            ("044 weighted", {"relationshipWeightProperty": "weight", "tolerance": 1e-3}, dict(variant="ar", weighted=True, tol=1e-3)),
            ("040", {"maxIterations": 30}, dict(variant="ev", maxit=30)),
            ("040 weighted", {"relationshipWeightProperty": "weight", "tolerance": 1e-4}, dict(variant="ev", weighted=True, tol=1e-4)),
            ("040 sourceNodes", {"sourceNodes": [N(1), N(3)], "maxIterations": 5}, dict(variant="ev", sources=(1, 3), maxit=5))):
        ev, _, _ = power(pr, **kw)
        proc = {"pr": "gds.pageRank.stream", "ar": "gds.articleRank.stream", "ev": "gds.eigenvector.stream"}[kw.get("variant", "pr")]
        cases[label] = case("pr", proc, cfg, kinds=["round", "send", "stop"], want=ev, concurrency=par)
    # 034 degree.
    for label, cfg, kw in (("034", {}, {}), ("034 weighted", {"relationshipWeightProperty": "weight"}, dict(weighted=True)),
                           ("034 REVERSE", {"orientation": "REVERSE"}, dict(orientation="REVERSE")),
                           ("034 UNDIRECTED", {"orientation": "UNDIRECTED"}, dict(orientation="UNDIRECTED"))):
        cases[label] = case("pr", "gds.degree.stream", cfg, kinds=["degree"], want=degree_ref(pr, **kw), concurrency=par)
    # 035 betweenness.
    for label, cfg, kw in (("035", {}, {}), ("035 weighted", {"relationshipWeightProperty": "weight"}, dict(weighted=True)),
                           ("035 UNDIRECTED", {"orientation": "UNDIRECTED"}, dict(orientation="UNDIRECTED")),
                           ("035 cm", {}, {})):
        gname = "cm" if label.endswith("cm") else "pr"
        cases[label] = case(gname, "gds.betweenness.stream", cfg, kinds=["sources", "source", "score"], want=brandes(G[gname], **kw), concurrency=par)
    # 038, 039.
    for label, proc, cfg, kw in (("038", "gds.closeness.stream", {}, {}),
                                 ("038 WF", "gds.closeness.stream", {"useWassermanFaust": True}, dict(wf=True)),
                                 ("038 UNDIRECTED", "gds.closeness.stream", {"orientation": "UNDIRECTED"}, dict(orientation="UNDIRECTED")),
                                 ("039", "gds.closeness.harmonic.stream", {}, dict(harmonic=True)),
                                 ("039 REVERSE", "gds.closeness.harmonic.stream", {"orientation": "REVERSE"}, dict(harmonic=True, orientation="REVERSE"))):
        cases[label] = case("pr", proc, cfg, kinds=["depths"], want=closeness_ref(pr, **kw), concurrency=par)
    # 041 Katz (MAGE positional alpha, epsilon).
    for a, e in ((0.25, 0.01), (0.125, 1e-6)):
        cases[f"041 alpha {a}"] = case("pr", "katz_centrality.get", {}, args=[a, e], kinds=["round", "stop"], want=katz_ref(pr, a, e), concurrency=(1,))
    # 045 HITS.
    for it in (1, 3):
        cases[f"045 hitsIterations {it}"] = case("pr", "gds.hits.stream", {"hitsIterations": it}, kinds=["step"], want=hits_ref(pr, it), concurrency=par)
    # 059 CELF.
    for p, mc, seed, k in ((1.0, 4, 0, 3), (0.5, 20, 7, 3), (0.3, 30, 11, 4)):
        ev, _ = celf_ref(pr, k, mc, p, seed)
        cases[f"059 p {p}"] = case("pr", "gds.influenceMaximization.celf.stream",
                                   {"seedSetSize": k, "monteCarloSimulations": mc, "propagationProbability": p, "randomSeed": seed},
                                   kinds=["single", "pop", "select", "gain"], want=ev, concurrency=par)
    # 105 node similarity.
    for label, cfg, kw, gname in (("105 JACCARD", {"topK": 3}, dict(top_k=3), "pr"),
                                  ("105 OVERLAP", {"similarityMetric": "OVERLAP"}, dict(metric="OVERLAP"), "pr"),
                                  ("105 COSINE", {"similarityMetric": "COSINE"}, dict(metric="COSINE"), "cm"),
                                  ("105 weighted", {"relationshipWeightProperty": "weight"}, dict(weighted=True), "pr"),
                                  ("105 weighted COSINE", {"relationshipWeightProperty": "weight", "similarityMetric": "COSINE"}, dict(weighted=True, metric="COSINE"), "pr"),
                                  ("105 UNDIRECTED", {"orientation": "UNDIRECTED", "topK": 2}, dict(orientation="UNDIRECTED", top_k=2), "cm")):
        cases[label] = case(gname, "gds.nodeSimilarity.stream", cfg, kinds=["source"], want=node_similarity_ref(G[gname], **kw), concurrency=par)
    # 106 KNN (scalar property x).
    for seed, joins, rate in ((3, 2, 0.5), (9, 0, 1.0)):
        ev, _ = knn_ref(G["pts"], 2, rate, joins, 6, 0.001, seed)
        cases[f"106 seed {seed}"] = case("pts", "gds.knn.stream",
                                         {"nodeProperties": ["x"], "topK": 2, "sampleRate": rate, "randomJoins": joins,
                                          "maxIterations": 6, "randomSeed": seed},
                                         kinds=["init", "split", "join", "random", "update", "round"], want=ev, concurrency=par)
    # 106 perturbation (GDS: only an equal similarity replaces the worst);
    # differs from lpg before rmp #71's fix (found by a search over seeds).
    ev, lists = knn_ref(G["pts"], 2, 0.5, 0, 6, 0.001, 0, perturbation=1.0)
    rows = [[v, x["id"], x["sim"]] for v in range(len(lists)) for x in lists[v]]
    cases["106 perturbationRate 1"] = case("pts", "gds.knn.stream",
                                           {"nodeProperties": ["x"], "topK": 2, "sampleRate": 0.5, "randomJoins": 0,
                                            "maxIterations": 6, "randomSeed": 0, "perturbationRate": 1.0},
                                           kinds=["init", "split", "join", "update", "round"], want=ev, concurrency=par, rows=rows)
    # 061 Louvain, 065 modularity optimisation, 062 Leiden.
    cm = G["cm"]
    for label, gname, orient in (("061", "cm", None), ("061 UNDIRECTED", "cm", "UNDIRECTED")):
        ev, _ = louvain_ref(G[gname], orient or "NATURAL")
        cases[label] = case(gname, "gds.louvain.stream", {}, orientation=orient, kinds=["colours", "class", "round", "stop", "level"], want=ev, concurrency=par)
    ev = []
    local_move(Net.of(cm), list(range(cm["n"])), 1.0, 10, 1e-4, ev)
    cases["065"] = case("cm", "gds.modularityOptimization.stream", {}, kinds=["colours", "class", "round", "stop"], want=ev, concurrency=par)
    ev, seq = [], []
    local_move(Net.of(G["c4"]), [0, 1, 2, 3], 1.0, 1, 1e-4, ev)
    local_move(Net.of(G["c4"]), [0, 1, 2, 3], 1.0, 1, 1e-4, seq, sequential=True)
    assert ev != seq
    cases["065 c4 class step"] = case("c4", "gds.modularityOptimization.stream", {"maxIterations": 1},
                                      kinds=["colours", "class", "round", "stop"], want=ev, concurrency=par, differs=seq)
    for seed in (42, 7):
        ev, _ = leiden_ref(cm, seed)
        cases[f"062 seed {seed}"] = case("cm", "gds.leiden.stream", {"randomSeed": seed}, kinds=["level", "refine", "class", "stop"], want=ev, concurrency=par)
    # 063 label propagation, 064 SLLPA.
    for label, orient in (("063", None), ("063 UNDIRECTED", "UNDIRECTED")):
        ev, _, _, _ = lpa_ref(cm, orient or "NATURAL")
        cases[label] = case("cm", "gds.labelPropagation.stream", {}, orientation=orient, kinds=["colours", "class", "round"], want=ev, concurrency=par)
    # A colour class of 063, 064 and 078's local search is the sequential
    # update of its nodes: no two are adjacent, and a node's step reads
    # only its neighbours' state.
    for orient in ("NATURAL", "UNDIRECTED"):
        assert lpa_ref(cm, orient)[0] == lpa_ref(cm, orient, live=True)[0]
    for name in ("rc24", "karate", "planted60"):
        n, edges = fixture(name)
        g, _ = relabel(n, edges, lambda x: str(x))
        assert lpa_ref(g, "UNDIRECTED")[0] == lpa_ref(g, "UNDIRECTED", live=True)[0], name
    for seed in (42, 5):
        assert sllpa_ref(cm, 6, seed) == sllpa_ref(cm, 6, seed, live=True)
    for k, seed in ((2, 42), (3, 1)):
        assert kcut_ref(cm, k, 3, seed)[0] == kcut_ref(cm, k, 3, seed, live=True)[0]
    for seed in (42, 5):
        cases[f"064 seed {seed}"] = case("cm", "gds.sllpa.stream", {"maxIterations": 6, "randomSeed": seed}, kinds=["hear", "memory"], want=sllpa_ref(cm, 6, seed), concurrency=par)
    # 078 approximate maximum k-cut.
    for k, seed in ((2, 42), (3, 1)):
        ev, _ = kcut_ref(cm, k, 3, seed)
        cases[f"078 k {k}"] = case("cm", "gds.maxkcut.stream", {"k": k, "iterations": 3, "randomSeed": seed}, kinds=["perm", "construct", "search", "fm", "cut"], want=ev, concurrency=par)
    # 080 k-means, 081 HDBSCAN on the points.
    pts = [list(c) for c in COORDS]
    # The last three differ from lpg before rmp #71's fix (a final
    # reassignment, a stop at fewer than n x deltaThreshold swaps, the
    # restart of least inertia): found by a search over these parameters.
    for label, cfg, kw in (("080 UNIFORM", {"k": 3, "randomSeed": 42}, dict(k=3, seed=42)),
                           ("080 KMEANS++", {"k": 4, "randomSeed": 7, "initialSampler": "KMEANS++", "deltaThreshold": 0.2},
                            dict(k=4, seed=7, sampler="KMEANS++", delta=0.2)),
                           ("080 maxIterations 1", {"k": 3, "randomSeed": 1, "maxIterations": 1}, dict(k=3, seed=1, maxit=1)),
                           ("080 deltaThreshold 0.1", {"k": 3, "randomSeed": 23, "maxIterations": 3, "deltaThreshold": 0.1},
                            dict(k=3, seed=23, maxit=3, delta=0.1)),
                           ("080 restarts", {"k": 2, "randomSeed": 24, "numberOfRestarts": 3, "deltaThreshold": 0.1},
                            dict(k=2, seed=24, restarts=3, delta=0.1))):
        kw.setdefault("delta", 0.05)
        ev, assign, cent = kmeans_ref(pts, kw.pop("k"), kw.pop("maxit", 10), kw.pop("delta"), kw.pop("seed"), **kw)
        rows = [[i, assign[i], math.sqrt(sqd(pts[i], cent[assign[i]])), -1.0] for i in range(len(pts))]
        cases[label] = case("pts", "gds.kmeans.stream", dict(cfg, nodeProperty="vec"), kinds=["init", "iteration", "restart"], want=ev, concurrency=par, rows=rows)
    for ms, smp in ((2, 2), (3, 3)):
        ev, _ = hdbscan_ref(pts, ms, smp)
        cases[f"081 {ms} {smp}"] = case("pts", "gds.hdbscan.stream", {"nodeProperty": "vec", "minClusterSize": ms, "samples": smp},
                                        kinds=["core", "mst", "condensed", "selected"], want=ev, concurrency=par)
    # 076, 077 spectral inputs.
    for label, proc, normalised in (("076", "cugraph.spectral_clustering.get", True), ("077", "cugraph.balanced_cut_clustering.get", False)):
        cases[label] = case("sp", proc, {}, args=[2, 2], kinds=["matrix", "eigenvalues", "eigenvector"], want=spectral_ref(G["sp"], normalised, 2), concurrency=(1,))
    # 082 counts.
    comm_cm = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2]
    ev, m, total = counts_ref(cm, comm_cm)
    rows = [[c, (e[0] - e[1] * e[1] * (1.0 / total)) / total] for c, e in sorted(m.items())]
    cases["082 modularity"] = case("cmc", "gds.modularity.stream", {"communityProperty": "community"}, kinds=["counts"], want=ev, concurrency=par, rows=rows)
    ev, m, _ = counts_ref(cm, comm_cm, "UNDIRECTED")
    rows = [[c, (e[1] - e[0]) / ((e[1] - e[0]) + e[0])] for c, e in sorted(m.items()) if e[1] > 0.0]
    cases["082 conductance"] = case("cmc", "gds.conductance.stream", {"communityProperty": "community", "orientation": "UNDIRECTED"}, kinds=["counts"], want=ev, concurrency=par, rows=rows)
    # 107: GDS `SimilaritiesFunc` formulas (lpg's arithmetic order).
    a, b = [1.0, 2.0, 3.0, 0.5], [2.0, 4.0, 7.0, 1.5]
    dot = xx = yy = 0.0
    for x, y in zip(a, b):
        dot += x * y; xx += x * x; yy += y * y
    cos = dot / math.sqrt(xx * yy)
    ma, mb = rsum(a) / 4.0, rsum(b) / 4.0
    dot = xx = yy = 0.0
    for x, y in zip(a, b):
        dx, dy = x - ma, y - mb
        dot += dx * dy; xx += dx * dx; yy += dy * dy
    pear = dot / math.sqrt(xx * yy)
    dist = math.sqrt(rsum((x - y) * (x - y) for x, y in zip(a, b)))
    for name, val in (("cosine", cos), ("pearson", pear), ("euclideanDistance", dist), ("euclidean", 1.0 / (1.0 + dist))):
        cases[f"107 {name}"] = case("nil", f"gds.similarity.{name}", None, args=[a, b], kinds=[], want=[], concurrency=(), rows=[[val]], function=True)
    cases["107 jaccard"] = case("nil", "gds.similarity.jaccard", None, args=[[1, 2, 2, 3], [2, 3, 4]], kinds=[], want=[], concurrency=(),
                                rows=[[2.0 / 5.0]], function=True)
    cases["107 overlap"] = case("nil", "gds.similarity.overlap", None, args=[[1, 2, 3], [2, 3, 4, 5]], kinds=[], want=[], concurrency=(),
                                rows=[[2.0 / 3.0]], function=True)
    # 112-115, 117: GDS `alpha/alpha-proc/.../linkprediction/LinkPredictionFunc.java`
    # and `NeighborsFinder.java` (neighbours: the other ends, the node
    # itself excluded; degree: the relationships in the direction).
    out, inc = adjacency(cm)
    def nb(v, d):
        es = (out[v] if d in ("OUTGOING", "BOTH") else []) + (inc[v] if d in ("INCOMING", "BOTH") else [])
        return {x for x, _ in es if x != v}, len({e for _, e in es})
    for (u, v, d) in ((0, 3, "BOTH"), (0, 1, "OUTGOING"), (8, 10, "INCOMING"), (1, 2, "BOTH"), (3, 7, "BOTH")):
        (na, da), (nb_, db) = nb(u, d), nb(v, d)
        common = sorted(na & nb_) if u != v else []
        aa = 0.0
        ra = 0.0
        for c in common:
            aa += fdiv(1.0, math.log(float(nb(c, d)[1])))
            ra += 1.0 / float(nb(c, d)[1])
        for name, val in (("adamicAdar", aa), ("commonNeighbors", float(len(common))), ("preferentialAttachment", float(da * db)),
                          ("resourceAllocation", ra), ("totalNeighbors", float(len(na | nb_)))):
            if not math.isfinite(val):
                continue  # JSON has no infinity; ln(1) = 0 gives one
            cases[f"11x {name} {u} {v} {d}"] = case("cm", f"gds.linkprediction.{name}", {"direction": d}, args=[N(u), N(v)], kinds=[], want=[],
                                                    concurrency=(), rows=[[val]], function=True)
    for (u, v) in ((0, 1), (3, 4)):
        cases[f"116 {u} {v}"] = case("cmc", "gds.linkprediction.sameCommunity", None, args=[N(u), N(v), "community"], kinds=[], want=[],
                                     concurrency=(), rows=[[1.0 if comm_cm[u] == comm_cm[v] else 0.0]], function=True)
    # One node with a self-loop, every entry that needs no property.
    one = dict(n=1, edges=[(0, 0, "R", 1.0)])
    G["one"] = one
    for label, proc, cfg, kinds, want in (
            ("042", "gds.pageRank.stream", {}, ["round", "send", "stop"], power(one)[0]),
            ("044", "gds.articleRank.stream", {}, ["round", "send", "stop"], power(one, "ar")[0]),
            ("040", "gds.eigenvector.stream", {}, ["round", "stop"], power(one, "ev")[0]),
            ("034", "gds.degree.stream", {}, ["degree"], degree_ref(one)),
            ("035", "gds.betweenness.stream", {}, ["sources", "source", "score"], brandes(one)),
            ("038", "gds.closeness.stream", {}, ["depths"], closeness_ref(one)),
            ("039", "gds.closeness.harmonic.stream", {}, ["depths"], closeness_ref(one, harmonic=True)),
            ("045", "gds.hits.stream", {"hitsIterations": 2}, ["step"], hits_ref(one, 2)),
            ("059", "gds.influenceMaximization.celf.stream", {"seedSetSize": 2, "propagationProbability": 1.0, "monteCarloSimulations": 2},
             ["single", "pop", "select", "gain"], celf_ref(one, 2, 2, 1.0, 0)[0]),
            ("061", "gds.louvain.stream", {}, ["class", "round", "stop", "level"], louvain_ref(one)[0]),
            ("062", "gds.leiden.stream", {}, ["level", "refine", "class", "stop"], leiden_ref(one, 0)[0]),
            ("063", "gds.labelPropagation.stream", {}, ["class", "round"], lpa_ref(one)[0]),
            ("064", "gds.sllpa.stream", {"maxIterations": 3}, ["hear", "memory"], sllpa_ref(one, 3, 0)),
            ("078", "gds.maxkcut.stream", {"iterations": 1}, ["perm", "construct", "search", "fm", "cut"], kcut_ref(one, 2, 1, 0)[0])):
        cases[f"{label} one node"] = case("one", proc, cfg, kinds=kinds, want=want, concurrency=(1,))
    cases["041 one node"] = case("one", "katz_centrality.get", {}, args=[0.25, 0.01], kinds=["round", "stop"], want=katz_ref(one, 0.25, 0.01), concurrency=(1,))
    # The empty projection: no row.
    for proc, cfg in (("gds.pageRank.stream", {}), ("gds.articleRank.stream", {}), ("gds.eigenvector.stream", {}),
                      ("gds.degree.stream", {}), ("gds.betweenness.stream", {}), ("gds.closeness.stream", {}),
                      ("gds.closeness.harmonic.stream", {}), ("gds.hits.stream", {}),
                      ("gds.influenceMaximization.celf.stream", {"seedSetSize": 1}), ("gds.nodeSimilarity.stream", {}),
                      ("gds.louvain.stream", {}), ("gds.leiden.stream", {}), ("gds.labelPropagation.stream", {}),
                      ("gds.sllpa.stream", {"maxIterations": 3}), ("gds.modularityOptimization.stream", {}),
                      ("gds.maxkcut.stream", {})):
        cases[f"{proc} empty"] = case("nil", proc, cfg, kinds=[], want=[], concurrency=(1,), rows=[])
    graphs = dict(GRAPHS)
    graphs["nil"] = dict(n=0, edges=[])
    graphs["cmc"] = dict(cm, nodes=[f":N {{community: {c}," for c in comm_cm])
    write_points(COORDS)
    json.dump(dict(graphs=graphs, cases=cases, rng=rng_cases(), analysis=analysis()), open(OUT, "w"), indent=1)
    print(f"{len(cases)} cases, {sum(len(c['want']) for c in cases.values())} events written to {os.path.normpath(OUT)}")
    print(json.dumps(json.load(open(OUT))["analysis"], indent=1))


if __name__ == "__main__":
    main()
