#!/usr/bin/env python3
"""Page-touch simulation of graph adjacency layouts over fixed 8 KiB pages.

Measures, for one generated graph and one edge-creation order:
  * pages that must be read to enumerate the outgoing neighbours of a node
    (cold-cache I/O per 1-hop expansion, excluding B+tree inner pages);
  * bytes per edge on disk (including measured B+tree leaf fill);
  * pages dirtied and pre-existing records modified per edge insertion
    (the latter is the number of MVCC versions an insert creates).

Layouts:
  REC  Neo4j record-format style: 34 B relationship records addressed by id
       (id = creation order, never reused), doubly linked per-node chains.
       Sparse node (<= 50 rels): one chain with every relationship of the node.
       Dense node (> 50 rels): per-type group, walk only the outgoing chain.
       Record sizes and threshold from Neo4j sources (see report).
  BT47 clustered B+tree, one entry per (node, direction, type, neighbour, edge id)
       plus create/delete timestamps: 29 B key + 16 B stamps + 2 B slot = 47 B.
  BT31 same without in-entry timestamps: 29 B + 2 B = 31 B.
  CSR  packed per-node array of (neighbour, edge id) = 16 B per entry,
       sorted by node; lower bound, not updatable in place.

This is a model, not a benchmark of real code: it counts pages, not time.
"""
import bisect
import random
import statistics
import sys

PAGE = 8192
PAGE_HDR = 64          # assumed page header (checksum, LSN, counts)
REL_REC = 34           # Neo4j standard RelationshipRecordFormat.RECORD_SIZE
NODE_REC = 15          # Neo4j standard NodeRecordFormat.RECORD_SIZE
DENSE = 50             # Neo4j db.relationship_grouping_threshold default


def gen_graph(n, m, alpha, seed):
    rnd = random.Random(seed)
    perm = list(range(n))
    rnd.shuffle(perm)
    weights = [1.0 / (i + 1) ** alpha for i in range(n)]
    cum = []
    s = 0.0
    for w in weights:
        s += w
        cum.append(s)
    src = rnd.choices(range(n), cum_weights=cum, k=m)
    dst = rnd.choices(range(n), cum_weights=cum, k=m)
    return [(perm[a], perm[b]) for a, b in zip(src, dst)], rnd


class LeafBTree:
    """Leaf level of a B+tree: sorted leaves, 50/50 split, bisect routing."""

    def __init__(self, cap):
        self.cap = cap
        self.leaves = [[]]
        self.firsts = [None]

    def insert(self, key):
        i = bisect.bisect_right(self.firsts, key, lo=1) - 1 if len(self.firsts) > 1 else 0
        leaf = self.leaves[i]
        bisect.insort(leaf, key)
        touched = 1
        if len(leaf) > self.cap:
            half = len(leaf) // 2
            right = leaf[half:]
            del leaf[half:]
            self.leaves.insert(i + 1, right)
            self.firsts.insert(i + 1, right[0])
            touched = 2
        if i == 0:
            self.firsts[0] = None
        return i, touched


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p / 100.0 * len(xs)))]


def summarize(name, per_node, deg, nodes):
    uni = [per_node[v] for v in nodes]
    wsum = sum(deg[v] for v in nodes)
    wmean = sum(per_node[v] * deg[v] for v in nodes) / wsum
    return (f"{name:5s} pages/expand: mean(uniform)={statistics.mean(uni):6.2f} "
            f"mean(degree-weighted)={wmean:7.2f} p50={pct(uni,50):4d} "
            f"p99={pct(uni,99):5d} max={max(uni):5d}")


def run(n, m, alpha, order, seed):
    edges, rnd = gen_graph(n, m, alpha, seed)
    if order == "sorted":
        edges.sort()
    # edge id = creation index (monotonic, never reused)
    out_deg = [0] * n
    tot_deg = [0] * n
    for s, d in edges:
        out_deg[s] += 1
        tot_deg[s] += 1
        if d != s:
            tot_deg[d] += 1
    nodes = [v for v in range(n) if out_deg[v] > 0]
    print(f"graph n={n} m={m} alpha={alpha} order={order} seed={seed}; "
          f"nodes with out-edges={len(nodes)}; max out-degree={max(out_deg)}; "
          f"mean out-degree (of those)={m/len(nodes):.2f}")

    # ---------- REC ----------
    recs_per_page = PAGE // REL_REC
    node_per_page = PAGE // NODE_REC
    chain_all = [[] for _ in range(n)]
    chain_out = [[] for _ in range(n)]
    last_head = [None] * n
    ins_pages = []
    ins_versions = []
    for eid, (s, d) in enumerate(edges):
        pages = {eid // recs_per_page}
        versions = 0
        for v in ({s, d}):
            pages.add(v // node_per_page)        # node (or group) record: head pointer
            versions += 1
            if last_head[v] is not None:         # old first rel: prev pointer
                pages.add(last_head[v] // recs_per_page)
                versions += 1
            last_head[v] = eid
            chain_all[v].append(eid)
        chain_out[s].append(eid)
        ins_pages.append(len(pages))
        ins_versions.append(versions)
    rec_pages = [0] * n
    for v in nodes:
        chain = chain_all[v] if tot_deg[v] <= DENSE else chain_out[v]
        extra = 1 if tot_deg[v] <= DENSE else 2  # node record (+ group record)
        rec_pages[v] = len({e // recs_per_page for e in chain}) + extra
    print(summarize("REC", rec_pages, out_deg, nodes))
    rec_bytes = REL_REC + NODE_REC * n / m
    print(f"REC   bytes/edge={rec_bytes:.1f} (34 B record shared by both directions); "
          f"insert: pages dirtied mean={statistics.mean(ins_pages):.2f}, "
          f"pre-existing records modified (MVCC versions) mean={statistics.mean(ins_versions):.2f}")

    # ---------- B+trees ----------
    for name, esz in (("BT47", 47), ("BT31", 31), ("BT18", 18)):
        cap = (PAGE - PAGE_HDR) // esz
        t = LeafBTree(cap)
        ins_p = []
        for eid, (s, d) in enumerate(edges):
            _, a = t.insert((s, 0, d, eid))
            _, b = t.insert((d, 1, s, eid))
            ins_p.append(a + b + 1)  # + edge-record tree leaf (right-edge append)
        leaves_of = {}
        for leaf in t.leaves:
            seen = set()
            for k in leaf:
                if k[1] == 0:
                    seen.add(k[0])
            for v in seen:
                leaves_of[v] = leaves_of.get(v, 0) + 1
        bt_pages = [0] * n
        for v in nodes:
            bt_pages[v] = leaves_of.get(v, 0)
        nleaves = len(t.leaves)
        fill = 2 * m / (nleaves * cap)
        # height with fanout f of inner pages (key 29 B + child 8 B + slot 2 B)
        fan = (PAGE - PAGE_HDR) // 39
        h, x = 1, nleaves
        while x > 1:
            x = -(-x // fan)
            h += 1
        print(summarize(name, bt_pages, out_deg, nodes) +
              f"  (+{h-1} inner page(s) per descent, usually cached)")
        print(f"{name:5s} bytes/edge={nleaves*PAGE/m:.1f} (2 entries/edge, leaf fill={fill:.2f}, "
              f"leaves={nleaves}, height={h}); insert: pages dirtied mean={statistics.mean(ins_p):.2f}, "
              f"pre-existing records modified (MVCC versions)=0")

    # ---------- CSR lower bound ----------
    per = 16
    off = 0
    csr_pages = [0] * n
    for v in range(n):
        if out_deg[v]:
            a = off
            b = off + out_deg[v] * per - 1
            csr_pages[v] = b // PAGE - a // PAGE + 1
        off += out_deg[v] * per
    print(summarize("CSR", csr_pages, out_deg, nodes) + "  (+ offsets array page)")
    print(f"CSR   bytes/edge={2*per + 2*8*n/m:.1f} (both directions + offsets); not updatable in place")
    print()


if __name__ == "__main__":
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100_000
    m = int(sys.argv[2]) if len(sys.argv) > 2 else 1_000_000
    alpha = float(sys.argv[3]) if len(sys.argv) > 3 else 0.8
    for order in ("random", "sorted"):
        run(n, m, alpha, order, seed=42)
