#!/usr/bin/env bash
# GDS 2026.09.0 runs on the trace graphs of rmp #71 pass C
# (`trace_ref_c.py`): ALG-147, 149, 153 and 154 on GDS's own code, for
# the review table's GDS comparisons. Node `uid` i is lpg's node i; each
# graph is a component of its own (the k-tree and Prim span one component;
# Steiner reaches from its source), except the prize-collecting graphs,
# which run alone.
#
# Usage: gds_probes_71c.sh WORKDIR OUT
# WORKDIR receives the CSV directories and case files; OUT the JSON Lines,
# one file per case (`gds_batch.sh`). Every container is removed.
set -euo pipefail
work="$(realpath "$1")"
out="$(realpath "$2")"
here="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$work/kst" "$work/pc" "$work/pp" "$out"
N() { echo "COLLECT { MATCH (n {uid: $1}) RETURN n }[0]"; }
NS() { local o=""; for x in "$@"; do o="$o, $(N "$x")"; done; echo "[${o#, }]"; }
TREE="YIELD nodeId, parentId, weight RETURN {nodeId: gds.util.asNode(nodeId).uid, parentId: gds.util.asNode(parentId).uid, weight: weight} AS row"
K() { # case source k objective property
  echo "$1;;CALL gds.kSpanningTree.write('u', {sourceNode: $(N "$2"), k: $3, relationshipWeightProperty: 'weight', objective: '$4', writeProperty: '$5', concurrency: 1}) YIELD effectiveNodeCount WITH effectiveNodeCount AS c MATCH (n) WHERE n.$5 IS NOT NULL RETURN {uid: n.uid, k: n.$5, c: c} AS row"
}

# Component "st" (uids 0-8, directed) and "wt" (uids 20-27), and the
# k-tree probes "kq" (uids 30-35) and "kd" (uids 40-45).
{
  echo "id:ID,:LABEL,uid:long"
  for i in 0 1 2 3 4 5 6 7 8 20 21 22 23 24 25 26 27 30 31 32 33 34 35 40 41 42 43 44 45; do echo "$i,N,$i"; done
} > "$work/kst/nodes.csv"
cat > "$work/kst/edges.csv" <<'EOF'
:START_ID,:END_ID,:TYPE,weight:double
0,1,R,1.0
1,2,R,1.5
2,3,R,1.25
0,4,R,4.5
3,4,R,0.75
1,5,R,2.75
5,6,R,0.5
6,4,R,0.25
4,7,R,3.25
2,7,R,6.0
6,8,R,2.0
8,7,R,0.125
7,3,R,0.375
20,21,R,4.0
20,22,R,1.0
21,22,R,2.5
21,23,R,6.0
22,24,R,7.0
23,24,R,3.0
23,25,R,0.5
24,26,R,2.0
25,26,R,9.0
26,27,R,1.75
22,27,R,8.0
30,31,R,39.0
30,33,R,29.0
31,32,R,11.0
32,34,R,30.0
32,35,R,13.0
40,41,R,26.0
40,42,R,34.0
41,43,R,47.0
42,43,R,55.0
42,44,R,25.0
44,45,R,10.0
EOF
{
  for cfg in "steiner;relationshipWeightProperty: 'weight'" "steiner.reroute;relationshipWeightProperty: 'weight', applyRerouting: true" "steiner.unweighted;applyRerouting: false"; do
    name=${cfg%%;*}; c=${cfg#*;}
    echo "st.gds.$name.jsonl;;CALL gds.steinerTree.stream('g', {sourceNode: $(N 0), targetNodes: $(NS 3 7 8 6), $c, concurrency: 1}) $TREE"
  done
  echo "wt.gds.mst.jsonl;;CALL gds.spanningTree.stream('u', {sourceNode: $(N 20), relationshipWeightProperty: 'weight', concurrency: 1}) $TREE"
  echo "wt.gds.mst.max.jsonl;;CALL gds.spanningTree.stream('u', {sourceNode: $(N 23), relationshipWeightProperty: 'weight', objective: 'maximum', concurrency: 1}) $TREE"
  K wt.gds.kst.3.jsonl 20 3 minimum ka
  K wt.gds.kst.5.jsonl 27 5 minimum kb
  K wt.gds.kst.max.4.jsonl 20 4 maximum kc
  K kq.gds.kst.4.jsonl 30 4 minimum kd
  K kd.gds.kst.4.jsonl 40 4 minimum ke
  echo "kst.ids.jsonl;;MATCH (n) RETURN {uid: n.uid, id: id(n)} AS row"
} > "$work/kst.cases"

# Prize-collecting graphs "pc" and "pp" (prizes as a float property).
pcst() { # dir nodes(uid:prize ...) edges(a-b-w ...)
  local d=$1; shift
  echo "id:ID,:LABEL,uid:long,prize:double" > "$d/nodes.csv"
  for x in $1; do echo "${x%%:*},N,${x%%:*},${x#*:}" >> "$d/nodes.csv"; done
  echo ":START_ID,:END_ID,:TYPE,weight:double" > "$d/edges.csv"
  for e in $2; do IFS=- read -r a b w <<< "$e"; echo "$a,$b,R,$w" >> "$d/edges.csv"; done
}
pcst "$work/pc" "0:5.0 1:1.0 2:2.5 3:0.0 4:6.0 5:4.0 6:3.0" "0-1-3.0 1-2-2.0 0-2-6.5 2-3-4.0 3-4-1.0 4-5-8.0 5-6-0.5 1-6-11.0"
pcst "$work/pp" "0:1.0 1:10.0 2:6.0" "1-0-10.0 2-0-4.0 2-1-8.0"
for g in pc pp; do
  echo "$g.gds.pcst.jsonl;;CALL gds.prizeSteinerTree.stream('u', {prizeProperty: 'prize', relationshipWeightProperty: 'weight', concurrency: 1}) $TREE" > "$work/$g.cases"
  echo "$g.gds.pcst.write.jsonl;;CALL gds.prizeSteinerTree.write('u', {prizeProperty: 'prize', relationshipWeightProperty: 'weight', writeRelationshipType: 'PCST', writeProperty: 'w'}) YIELD effectiveNodeCount, totalWeight, sumOfPrizes RETURN {effectiveNodeCount: effectiveNodeCount, totalWeight: totalWeight, sumOfPrizes: sumOfPrizes} AS row" >> "$work/$g.cases"
done
chmod -R a+rX "$work"
bash "$here/gds_batch.sh" "$work/kst" "$work/kst.cases" "$out"
bash "$here/gds_batch.sh" "$work/pc" "$work/pc.cases" "$out"
bash "$here/gds_batch.sh" "$work/pp" "$work/pp.cases" "$out"
