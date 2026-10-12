#!/usr/bin/env bash
# rmp #64: GDS oracle cases. Each line: OUT;;DIR;;PROC;;CONFIG;;RET;;ORIENTATION.
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
G=$L/docs/evidence/lab-sprint-5/gds_oracle.sh
N() { echo "COLLECT { MATCH (n {uid: $1}) RETURN n }[0]"; }
U='gds.util.asNode'
TRAV="YIELD sourceNode, nodeIds RETURN {sourceNode: $U(sourceNode).uid, nodeIds: [x IN nodeIds | $U(x).uid]} AS row"
PATHR="YIELD index, sourceNode, targetNode, totalCost, nodeIds, costs RETURN {index: index, sourceNode: $U(sourceNode).uid, targetNode: $U(targetNode).uid, totalCost: totalCost, nodeIds: [x IN nodeIds | $U(x).uid], costs: costs} AS row"
BFR="YIELD index, sourceNode, targetNode, totalCost, nodeIds, costs, isNegativeCycle RETURN {index: index, sourceNode: $U(sourceNode).uid, targetNode: $U(targetNode).uid, totalCost: totalCost, nodeIds: [x IN nodeIds | $U(x).uid], costs: costs, isNegativeCycle: isNegativeCycle} AS row"
TOPO="YIELD nodeId, maxDistanceFromSource RETURN {nodeId: $U(nodeId).uid, maxDistanceFromSource: maxDistanceFromSource} AS row"
APSP="YIELD sourceNodeId, targetNodeId, distance RETURN {sourceNodeId: $U(sourceNodeId).uid, targetNodeId: $U(targetNodeId).uid, distance: distance} AS row"
W="relationshipWeightProperty: 'weight'"
cases=(
"dag40.gds.topo.jsonl;;dag40;;gds.dag.topologicalSort.stream;;{computeMaxDistanceFromSource: true};;$TOPO;;"
"dag40.gds.topo.plain.jsonl;;dag40;;gds.dag.topologicalSort.stream;;{};;$TOPO;;"
"zipf60.gds.topo.jsonl;;zipf60;;gds.dag.topologicalSort.stream;;{computeMaxDistanceFromSource: true};;$TOPO;;"
"uniform60.gds.topo.jsonl;;uniform60;;gds.dag.topologicalSort.stream;;{computeMaxDistanceFromSource: true};;$TOPO;;"
"zipf60.gds.bfs.jsonl;;zipf60;;gds.bfs.stream;;{sourceNode: $(N 1)};;$TRAV;;"
"zipf60.gds.bfs.depth2.jsonl;;zipf60;;gds.bfs.stream;;{sourceNode: $(N 1), maxDepth: 2};;$TRAV;;"
"zipf60.gds.bfs.target.jsonl;;zipf60;;gds.bfs.stream;;{sourceNode: $(N 1), targetNodes: [$(N 8)]};;$TRAV;;"
"uniform60.gds.bfs.jsonl;;uniform60;;gds.bfs.stream;;{sourceNode: $(N 14)};;$TRAV;;"
"zipf60.gds.dfs.jsonl;;zipf60;;gds.dfs.stream;;{sourceNode: $(N 1)};;$TRAV;;"
"zipf60.gds.dfs.target.jsonl;;zipf60;;gds.dfs.stream;;{sourceNode: $(N 1), targetNodes: [$(N 8)]};;$TRAV;;"
"uniform60.gds.dfs.depth3.jsonl;;uniform60;;gds.dfs.stream;;{sourceNode: $(N 14), maxDepth: 3};;$TRAV;;"
"zipf60.gds.dijkstra.pair.jsonl;;zipf60;;gds.shortestPath.dijkstra.stream;;{sourceNode: $(N 1), targetNode: $(N 8), $W};;$PATHR;;"
"uniform60.gds.dijkstra.targets.jsonl;;uniform60;;gds.shortestPath.dijkstra.stream;;{sourceNode: $(N 14), targetNodes: [$(N 15), $(N 3), $(N 40)], $W};;$PATHR;;"
"zipf60.gds.dijkstra.all.jsonl;;zipf60;;gds.allShortestPaths.dijkstra.stream;;{sourceNode: $(N 1), $W};;$PATHR;;"
"uniform60.gds.dijkstra.all.jsonl;;uniform60;;gds.allShortestPaths.dijkstra.stream;;{sourceNode: $(N 14), $W};;$PATHR;;"
"zipf60.gds.dijkstra.all.unweighted.jsonl;;zipf60;;gds.allShortestPaths.dijkstra.stream;;{sourceNode: $(N 1)};;$PATHR;;"
"geo30.gds.astar.jsonl;;geo30;;gds.shortestPath.astar.stream;;{sourceNode: $(N 0), targetNode: $(N 2), latitudeProperty: 'lat', longitudeProperty: 'lng', $W};;$PATHR;;"
"geo30.gds.astar.b.jsonl;;geo30;;gds.shortestPath.astar.stream;;{sourceNode: $(N 5), targetNode: $(N 27), latitudeProperty: 'lat', longitudeProperty: 'lng', $W};;$PATHR;;"
"negw30.gds.bellmanford.jsonl;;negw30;;gds.bellmanFord.stream;;{sourceNode: $(N 25), $W};;$BFR;;"
"negcyc30.gds.bellmanford.jsonl;;negcyc30;;gds.bellmanFord.stream;;{sourceNode: $(N 25), $W};;$BFR;;"
"zipf60.gds.bellmanford.jsonl;;zipf60;;gds.bellmanFord.stream;;{sourceNode: $(N 1), $W};;$BFR;;"
"uniform60.gds.bellmanford.jsonl;;uniform60;;gds.bellmanFord.stream;;{sourceNode: $(N 14), $W};;$BFR;;"
"zipf60.gds.delta.jsonl;;zipf60;;gds.allShortestPaths.delta.stream;;{sourceNode: $(N 1), delta: 0.3, $W};;$PATHR;;"
"uniform60.gds.delta.jsonl;;uniform60;;gds.allShortestPaths.delta.stream;;{sourceNode: $(N 14), $W};;$PATHR;;"
"zipf60.gds.apsp.jsonl;;zipf60;;gds.allShortestPaths.stream;;{$W};;$APSP;;"
"uniform60.gds.apsp.unweighted.jsonl;;uniform60;;gds.allShortestPaths.stream;;{};;$APSP;;"
"zipf60.gds.yens.jsonl;;zipf60;;gds.shortestPath.yens.stream;;{sourceNode: $(N 1), targetNode: $(N 8), k: 6, $W};;$PATHR;;"
"uniform60.gds.yens.jsonl;;uniform60;;gds.shortestPath.yens.stream;;{sourceNode: $(N 14), targetNode: $(N 15), k: 6, $W};;$PATHR;;"
"dag40.gds.longestpath.jsonl;;dag40;;gds.dag.longestPath.stream;;{$W};;$PATHR;;"
"zipf60.gds.longestpath.jsonl;;zipf60;;gds.dag.longestPath.stream;;{};;$PATHR;;"
)
run() {
  local rest=$1
  out=${rest%%;;*}; rest=${rest#*;;}
  dir=${rest%%;;*}; rest=${rest#*;;}
  proc=${rest%%;;*}; rest=${rest#*;;}
  cfg=${rest%%;;*}; rest=${rest#*;;}
  ret=${rest%%;;*}; rest=${rest#*;;}
  ori=$rest
  if bash "$G" "$T/$dir" "$proc" "$cfg" "$ret" "${ori:-NATURAL}" > "$E/$out.tmp" 2> "$out.err"; then
    mv "$E/$out.tmp" "$E/$out"; echo "ok $out $(wc -l < "$E/$out")"
  else
    echo "FAIL $out"; rm -f "$E/$out.tmp"; tail -3 "$out.err"
  fi
}
export -f run; export G T E
i=0
for c in "${cases[@]}"; do
  run "$c" &
  i=$((i+1)); if (( i % 3 == 0 )); then wait; fi
done
wait
