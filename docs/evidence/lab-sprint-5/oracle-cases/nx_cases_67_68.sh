#!/usr/bin/env bash
# rmp #67 and #68: NetworkX 3.7 oracle cases (ALG-084, 086, 089, 123,
# 128, 143, 151, 161, 163, 169, 173). Plain calls go through
# `nx_oracle.py` (OUT DIR FUNCTION PARAMS [flags...]); the cases whose
# output `nx_oracle.py` cannot print run as small programs over the same
# graph reader (`py OUT DIR CODE`, CODE reading the multigraph `M`, its
# undirected form `U` and the simple undirected graph `S` without
# self-loops, and printing JSON lines).
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
LAB=$L/docs/evidence/lab-sprint-5
PY="/tmp/lpg-lab-venv/bin/python -I"
P="$PY $LAB/nx_oracle.py"
run() {
  local out=$1 dir=$2 fn=$3 params=$4; shift 4
  if $P "$T/$dir" "$fn" "$params" "$@" > "$E/$out"; then echo "ok $out $(wc -l < "$E/$out")"; else echo "FAIL $out"; rm -f "$E/$out"; fi
}
py() {
  local out=$1 dir=$2 code=$3
  if $PY -c "
import sys, json
sys.path.insert(0, '$LAB')
import networkx as nx
from nx_oracle import read
M = read('$T/$dir', True)
U = read('$T/$dir', False)
S = nx.Graph(); S.add_nodes_from(U.nodes()); S.add_edges_from((a, b) for a, b in U.edges() if a != b)
$code
" > "$E/$out"; then echo "ok $out $(wc -l < "$E/$out")"; else echo "FAIL $out"; rm -f "$E/$out"; fi
}
W='"weight": "weight"'

# 084: components of the undirected multigraph.
for f in zipf60 uniform60 zipf300; do
  run $f.nx.cc.jsonl $f connected_components '{}' --undirected
done
# 086: condensation members and component edges (as member sets).
for f in zipf60 uniform60 cyc12; do
  py $f.nx.condensation.jsonl $f "
C = nx.condensation(M)
for c in sorted(C.nodes, key=lambda c: min(C.nodes[c]['members'])):
    succ = sorted(min(C.nodes[d]['members']) for d in C.successors(c))
    print(json.dumps({'members': sorted(C.nodes[c]['members']), 'successors': succ}))"
done
# 089: biconnected component edges of the simple undirected graph, each
# edge as a sorted pair with its component number.
for f in karate zipf60 uniform60; do
  py $f.nx.bcc.jsonl $f "
for i, comp in enumerate(nx.biconnected_component_edges(S)):
    for a, b in comp:
        print(json.dumps({'edge': sorted([a, b]), 'bcc': i}))"
done
# 123: eccentricity, diameter and radius on connected graphs.
for f in karate uniform60; do
  run $f.nx.ecc.jsonl $f eccentricity '{}' --undirected
  run $f.nx.ecc.weighted.jsonl $f eccentricity "{$W}" --undirected
  run $f.nx.diameter.jsonl $f diameter '{}' --undirected
  run $f.nx.radius.jsonl $f radius '{}' --undirected
  run $f.nx.diameter.weighted.jsonl $f diameter "{$W}" --undirected
  run $f.nx.radius.weighted.jsonl $f radius "{$W}" --undirected
done
# Directed eccentricity: NetworkX's `eccentricity` refuses a digraph that
# is not strongly connected, so it is the largest shortest-path length
# from the node, "inf" when a node is unreachable.
for f in cyc12 zipf60; do
  py $f.nx.ecc.jsonl $f "
n = M.number_of_nodes()
for v in sorted(M.nodes):
    d = nx.single_source_shortest_path_length(M, v)
    print(json.dumps({'node': v, 'value': max(d.values()) if len(d) == n else 'inf'}))"
  py $f.nx.ecc.weighted.jsonl $f "
n = M.number_of_nodes()
for v in sorted(M.nodes):
    d = nx.single_source_dijkstra_path_length(M, v, weight='weight')
    print(json.dumps({'node': v, 'value': max(d.values()) if len(d) == n else 'inf'}))"
done
# 128: girth of simple undirected graphs.
for f in karate petersen bip20 dag40; do
  py $f.nx.girth.jsonl $f "print(json.dumps({'value': nx.girth(S) if nx.girth(S) != float('inf') else 'inf'}))"
done
# 143: Laplacian rows in node order.
py karate.nx.laplacian.jsonl karate "
A = nx.laplacian_matrix(U, nodelist=sorted(U.nodes), weight=None).toarray()
for row in A: print(json.dumps({'value': [float(x) for x in row]}))"
py karate.nx.laplacian.weighted.jsonl karate "
A = nx.laplacian_matrix(U, nodelist=sorted(U.nodes), weight='weight').toarray()
for row in A: print(json.dumps({'value': [float(x) for x in row]}))"
py zipf60.nx.laplacian.jsonl zipf60 "
A = nx.laplacian_matrix(M, nodelist=sorted(M.nodes), weight=None).toarray()
for row in A: print(json.dumps({'value': [float(x) for x in row]}))"
py zipf60.nx.laplacian.weighted.jsonl zipf60 "
A = nx.laplacian_matrix(M, nodelist=sorted(M.nodes), weight='weight').toarray()
for row in A: print(json.dumps({'value': [float(x) for x in row]}))"
# 151: determinant of the reduced Laplacian over the rationals (spanning
# trees of the undirected multigraph; arborescences oriented to the root,
# out-degree Laplacian, for the directed one).
count='
from fractions import Fraction
def det(m):
    m = [[Fraction(x) for x in r] for r in m]
    n, d = len(m), Fraction(1)
    for c in range(n):
        p = next((r for r in range(c, n) if m[r][c] != 0), None)
        if p is None: return 0
        if p != c: m[c], m[p] = m[p], m[c]; d = -d
        d *= m[c][c]
        for r in range(c + 1, n):
            f = m[r][c] / m[c][c]
            for k in range(c, n): m[r][k] -= f * m[c][k]
    return d
def count(G, root):
    nodes = sorted(G.nodes)
    ix = {v: i for i, v in enumerate(nodes)}
    L = [[0] * len(nodes) for _ in nodes]
    for a, b in G.edges():
        if a == b: continue
        L[ix[a]][ix[a]] += 1; L[ix[a]][ix[b]] -= 1
        if not G.is_directed(): L[ix[b]][ix[b]] += 1; L[ix[b]][ix[a]] -= 1
    r = ix[root]
    return int(det([[L[i][j] for j in range(len(nodes)) if j != r] for i in range(len(nodes)) if i != r]))
'
# karate, negw30 and flow30 have counts that an `f64` determinant does not
# give exactly (measured with numpy: 5090996323019105.0 for
# 5090996323019136); dag40's undirected count exceeds the INTEGER range.
for f in karate petersen negw30 flow30 dag40; do
  py $f.nx.st.jsonl $f "$count
print(json.dumps({'value': count(U, 0)}))"
done
py dag40.nx.st.directed.jsonl dag40 "$count
print(json.dumps({'root': 2, 'value': count(M, 2)}))"
py geo30.nx.st.directed.jsonl geo30 "$count
print(json.dumps({'root': 28, 'value': count(M, 28)}))"
py rc24.nx.st.directed.jsonl rc24 "$count
for r in range(4): print(json.dumps({'root': r, 'value': count(M, r)}))"
# 161: maximum-cardinality matching size of the simple undirected graph.
for f in karate zipf60 uniform60 petersen; do
  py $f.nx.matching.jsonl $f "print(json.dumps({'value': len(nx.max_weight_matching(S, maxcardinality=True, weight=None))}))"
done
# 163: maximum bipartite matching size (Hopcroft-Karp), left side L.
py bip20.nx.bipartite.matching.jsonl bip20 "
top = [v for v, d in U.nodes(data=True) if 'L' in d['labels']]
print(json.dumps({'value': len(nx.bipartite.hopcroft_karp_matching(S, top)) // 2}))"
# 169: maximal cliques of the simple undirected graph.
for f in karate zipf60 uniform60; do
  py $f.nx.cliques.jsonl $f "
for c in sorted(sorted(c) for c in nx.find_cliques(S)): print(json.dumps({'value': c}))"
done
# 173: greedy colouring, each NetworkX strategy except random_sequential
# (its order comes from Python's generator), number of colours. lpg's
# node order is the element identifier order, which lpg's CSV import
# gives in the order of the `id:ID` strings (measured: 0, 1, 10, 11, …):
# the simple graph's nodes are renumbered by their rank in that order and
# added, with each node's neighbours, in it, so that NetworkX's set
# iteration (by integer) and insertion orders are lpg's index order.
for f in karate zipf60 uniform60 petersen; do
  py $f.nx.greedy_color.jsonl $f "
rank = {v: i for i, v in enumerate(sorted(S.nodes, key=str))}
O = nx.Graph(); O.add_nodes_from(range(len(rank)))
O.add_edges_from(sorted(tuple(sorted((rank[a], rank[b]))) for a, b in S.edges()))
import itertools
from collections import defaultdict, deque
# NetworkX's smallest_last and independent_set take an arbitrary element
# of a Python set (set.pop(), min over a set), whose order follows the
# hash table; these copies of them take the smallest node instead, lpg's
# rule (greedy_coloring.py, NetworkX 3.7).
def smallest_last(G, colors):
    H = G.copy(); result = deque(); degrees = defaultdict(set); lbound = float('inf')
    for node, d in H.degree():
        degrees[d].add(node); lbound = min(lbound, d)
    for _ in G:
        min_degree = next(d for d in itertools.count(lbound) if d in degrees)
        u = min(degrees[min_degree]); degrees[min_degree].remove(u)
        if not degrees[min_degree]: del degrees[min_degree]
        result.appendleft(u)
        for v in H[u]:
            degree = H.degree(v); degrees[degree].remove(v)
            if not degrees[degree]: del degrees[degree]
            degrees[degree - 1].add(v)
        H.remove_node(u); lbound = min_degree - 1
    return result
def independent_set(G, colors):
    remaining_nodes = set(G)
    while remaining_nodes:
        H = G.subgraph(remaining_nodes); result = set(); remaining = set(H)
        while remaining:
            H = H.subgraph(remaining)
            v = min(sorted(remaining), key=H.degree)
            result.add(v); remaining -= set(H[v]) | {v}
        remaining_nodes -= result
        yield from sorted(result)
for s in ['largest_first', 'smallest_last', 'independent_set', 'connected_sequential_bfs', 'connected_sequential_dfs', 'saturation_largest_first']:
    c = nx.greedy_color(O, strategy={'smallest_last': smallest_last, 'independent_set': independent_set}.get(s, s))
    print(json.dumps({'strategy': s, 'colors': len(set(c.values())), 'value': c}))"
done
# 187 carry-over: NetworkX's `min_weight_matching` on the complete graph
# of 24 points (x, y) = (1000 (37 i mod 101) / 101, 1000 (61 i mod 103) /
# 103), Euclidean distances; the Rust test builds the same points.
$PY -c "
import json, math
import networkx as nx
pts = [(1000.0 * ((37 * i) % 101) / 101.0, 1000.0 * ((61 * i) % 103) / 103.0) for i in range(24)]
G = nx.Graph()
for a in range(24):
    for b in range(a + 1, 24):
        dx, dy = pts[a][0] - pts[b][0], pts[a][1] - pts[b][1]
        G.add_edge(a, b, weight=math.sqrt(dx * dx + dy * dy))
m = nx.min_weight_matching(G)
print(json.dumps({'value': sum(G[a][b]['weight'] for a, b in m), 'pairs': sorted(sorted(e) for e in m)}))
" > $E/points24.nx.min_matching.jsonl && echo "ok points24.nx.min_matching.jsonl"
