#!/usr/bin/env bash
# rmp #67 and #68: GDS oracle cases, one container per fixture
# (`gds_batch.sh`). Each case is OUT;;QUERY on graph `g` (NATURAL) or `u`
# (UNDIRECTED); OUT is written to src/algo/testdata/expected. Write modes
# write to the container's database only; their values are read back with
# a MATCH in the same statement.
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
B=$L/docs/evidence/lab-sprint-5/gds_batch.sh
U='gds.util.asNode'
N() { echo "COLLECT { MATCH (n {uid: $1}) RETURN n }[0]"; }
NS() { local out=""; for x in "$@"; do out="$out, $(N "$x")"; done; echo "[${out#, }]"; }
W="relationshipWeightProperty: 'weight'"
COMP="YIELD nodeId, componentId RETURN {nodeId: $U(nodeId).uid, componentId: componentId} AS row"
TREE="YIELD nodeId, parentId, weight RETURN {nodeId: $U(nodeId).uid, parentId: $U(parentId).uid, weight: weight} AS row"
AP="YIELD nodeId, resultingComponents RETURN {nodeId: $U(nodeId).uid, resultingComponents: resultingComponents} AS row"
BR="YIELD from, to, remainingSizes RETURN {from: $U(from).uid, to: $U(to).uid, remainingSizes: remainingSizes} AS row"
KC="YIELD nodeId, coreValue RETURN {nodeId: $U(nodeId).uid, coreValue: coreValue} AS row"
TC="YIELD nodeId, triangleCount RETURN {nodeId: $U(nodeId).uid, triangleCount: triangleCount} AS row"
TL="YIELD nodeA, nodeB, nodeC RETURN {a: $U(nodeA).uid, b: $U(nodeB).uid, c: $U(nodeC).uid} AS row"
LCC="YIELD nodeId, localClusteringCoefficient RETURN {nodeId: $U(nodeId).uid, lcc: localClusteringCoefficient} AS row"
CQ="YIELD nodeId, counts RETURN {nodeId: $U(nodeId).uid, counts: counts} AS row"
COL="YIELD nodeId, color RETURN {nodeId: $U(nodeId).uid, color: color} AS row"
FLOW="YIELD source, target, flow RETURN {source: $U(source).uid, target: $U(target).uid, flow: flow} AS row"

# The undirected structure cases of fixture $1 (graph `u`).
undirected() {
  local f=$1
  cat <<EOF
$f.gds.wcc.u.jsonl;;CALL gds.wcc.stream('u', {}) $COMP
$f.gds.ap.jsonl;;CALL gds.articulationPoints.stream('u', {}) $AP
$f.gds.ap.write.jsonl;;CALL gds.articulationPoints.write('u', {writeProperty: 'ap'}) YIELD articulationPointCount, nodePropertiesWritten WITH articulationPointCount AS c, nodePropertiesWritten AS w MATCH (n) WHERE n.ap IS NOT NULL RETURN {uid: n.uid, ap: n.ap, count: c, written: w} AS row
$f.gds.bridges.jsonl;;CALL gds.bridges.stream('u', {}) $BR
$f.gds.kcore.jsonl;;CALL gds.kcore.stream('u', {}) $KC
$f.gds.kcore.write.jsonl;;CALL gds.kcore.write('u', {writeProperty: 'core'}) YIELD degeneracy, nodePropertiesWritten RETURN {degeneracy: degeneracy, written: nodePropertiesWritten} AS row
$f.gds.triangles.count.jsonl;;CALL gds.triangleCount.stream('u', {}) $TC
$f.gds.triangles.count.max5.jsonl;;CALL gds.triangleCount.stream('u', {maxDegree: 5}) $TC
$f.gds.triangles.write.jsonl;;CALL gds.triangleCount.write('u', {writeProperty: 'tc'}) YIELD globalTriangleCount, nodeCount, nodePropertiesWritten RETURN {global: globalTriangleCount, nodeCount: nodeCount, written: nodePropertiesWritten} AS row
$f.gds.triangles.list.jsonl;;CALL gds.triangles('u', {}) $TL
$f.gds.lcc.jsonl;;CALL gds.localClusteringCoefficient.stream('u', {}) $LCC
$f.gds.lcc.write.jsonl;;CALL gds.localClusteringCoefficient.write('u', {writeProperty: 'lcc'}) YIELD averageClusteringCoefficient, nodeCount RETURN {average: averageClusteringCoefficient, nodeCount: nodeCount} AS row
$f.gds.cliques.jsonl;;CALL gds.cliqueCounting.stream('u', {}) $CQ
$f.gds.cliques.write.jsonl;;CALL gds.cliqueCounting.write('u', {writeProperty: 'cq'}) YIELD globalCount, nodePropertiesWritten RETURN {globalCount: globalCount, written: nodePropertiesWritten} AS row
$f.gds.k1.u.jsonl;;CALL gds.k1coloring.stream('u', {concurrency: 1}) $COL
$f.gds.k1.u.stats.jsonl;;CALL gds.k1coloring.stats('u', {concurrency: 1}) YIELD colorCount, ranIterations, didConverge RETURN {colorCount: colorCount, ranIterations: ranIterations, didConverge: didConverge} AS row
EOF
}

# The spanning-tree cases of fixture $1 from node $2 (graph `u`).
trees() {
  local f=$1 s=$2
  cat <<EOF
$f.gds.mst.jsonl;;CALL gds.spanningTree.stream('u', {sourceNode: $(N "$s"), $W}) $TREE
$f.gds.mst.max.jsonl;;CALL gds.spanningTree.stream('u', {sourceNode: $(N "$s"), $W, objective: 'maximum'}) $TREE
$f.gds.mst.unweighted.jsonl;;CALL gds.spanningTree.stream('u', {sourceNode: $(N "$s")}) $TREE
$f.gds.mst.write.jsonl;;CALL gds.spanningTree.write('u', {sourceNode: $(N "$s"), $W, writeRelationshipType: 'MST', writeProperty: 'w'}) YIELD effectiveNodeCount, relationshipsWritten, totalWeight RETURN {effectiveNodeCount: effectiveNodeCount, relationshipsWritten: relationshipsWritten, totalWeight: totalWeight} AS row
$f.gds.kst.jsonl;;CALL gds.kSpanningTree.write('u', {sourceNode: $(N "$s"), k: 5, $W, writeProperty: 'kst'}) YIELD effectiveNodeCount WITH effectiveNodeCount AS c MATCH (n) RETURN {uid: n.uid, kst: n.kst, effectiveNodeCount: c} AS row
$f.gds.kst.max.jsonl;;CALL gds.kSpanningTree.write('u', {sourceNode: $(N "$s"), k: 4, $W, objective: 'maximum', writeProperty: 'kstmax'}) YIELD effectiveNodeCount WITH effectiveNodeCount AS c MATCH (n) RETURN {uid: n.uid, kst: n.kstmax, effectiveNodeCount: c} AS row
EOF
}

{
  undirected karate
  trees karate 0
  cat <<EOF
karate.gds.wcc.jsonl;;CALL gds.wcc.stream('g', {}) $COMP
karate.gds.scc.jsonl;;CALL gds.scc.stream('g', {}) $COMP
karate.gds.k1.jsonl;;CALL gds.k1coloring.stream('g', {concurrency: 1}) $COL
karate.gds.k1.stats.jsonl;;CALL gds.k1coloring.stats('g', {concurrency: 1}) YIELD colorCount, ranIterations, didConverge RETURN {colorCount: colorCount, ranIterations: ranIterations, didConverge: didConverge} AS row
EOF
} > /tmp/lpg-gds-karate.cases

directed() { # fixture source targets...
  local f=$1 s=$2; shift 2
  cat <<EOF
$f.gds.wcc.jsonl;;CALL gds.wcc.stream('g', {}) $COMP
$f.gds.wcc.threshold.jsonl;;CALL gds.wcc.stream('g', {$W, threshold: 0.5}) $COMP
$f.gds.wcc.consecutive.jsonl;;CALL gds.wcc.stream('g', {consecutiveIds: true}) $COMP
$f.gds.wcc.write.jsonl;;CALL gds.wcc.write('g', {writeProperty: 'wcc'}) YIELD componentCount, componentDistribution, nodePropertiesWritten RETURN {componentCount: componentCount, componentDistribution: componentDistribution, written: nodePropertiesWritten} AS row
$f.gds.scc.jsonl;;CALL gds.scc.stream('g', {}) $COMP
$f.gds.scc.write.jsonl;;CALL gds.scc.write('g', {writeProperty: 'scc'}) YIELD componentCount, componentDistribution, nodePropertiesWritten RETURN {componentCount: componentCount, componentDistribution: componentDistribution, written: nodePropertiesWritten} AS row
$f.gds.k1.jsonl;;CALL gds.k1coloring.stream('g', {concurrency: 1}) $COL
$f.gds.k1.stats.jsonl;;CALL gds.k1coloring.stats('g', {concurrency: 1}) YIELD colorCount, ranIterations, didConverge RETURN {colorCount: colorCount, ranIterations: ranIterations, didConverge: didConverge} AS row
$f.gds.steiner.jsonl;;CALL gds.steinerTree.stream('g', {sourceNode: $(N "$s"), targetNodes: $(NS "$@"), $W}) $TREE
$f.gds.steiner.reroute.jsonl;;CALL gds.steinerTree.stream('g', {sourceNode: $(N "$s"), targetNodes: $(NS "$@"), $W, applyRerouting: true}) $TREE
$f.gds.steiner.unweighted.jsonl;;CALL gds.steinerTree.stream('g', {sourceNode: $(N "$s"), targetNodes: $(NS "$@")}) $TREE
$f.gds.steiner.write.jsonl;;CALL gds.steinerTree.write('g', {sourceNode: $(N "$s"), targetNodes: $(NS "$@"), $W, writeRelationshipType: 'ST', writeProperty: 'w'}) YIELD effectiveNodeCount, effectiveTargetNodesCount, totalWeight, relationshipsWritten RETURN {effectiveNodeCount: effectiveNodeCount, effectiveTargetNodesCount: effectiveTargetNodesCount, totalWeight: totalWeight, relationshipsWritten: relationshipsWritten} AS row
EOF
}

{
  directed zipf60 13 0 4 9 17 25 33 41 50 58
  undirected zipf60
  trees zipf60 13
} > /tmp/lpg-gds-zipf60.cases

{
  directed uniform60 14 0 3 11 22 37 45 59
  undirected uniform60
  trees uniform60 14
} > /tmp/lpg-gds-uniform60.cases

{
  directed zipf300 218 0 5 13 50 77 123 200 299
  cat <<EOF
zipf300.gds.wcc.u.jsonl;;CALL gds.wcc.stream('u', {}) $COMP
zipf300.gds.kcore.jsonl;;CALL gds.kcore.stream('u', {}) $KC
zipf300.gds.triangles.count.jsonl;;CALL gds.triangleCount.stream('u', {}) $TC
zipf300.gds.lcc.jsonl;;CALL gds.localClusteringCoefficient.stream('u', {}) $LCC
zipf300.gds.cliques.jsonl;;CALL gds.cliqueCounting.stream('u', {}) $CQ
zipf300.gds.k1.u.jsonl;;CALL gds.k1coloring.stream('u', {concurrency: 1}) $COL
zipf300.gds.k1.u.stats.jsonl;;CALL gds.k1coloring.stats('u', {concurrency: 1}) YIELD colorCount, ranIterations, didConverge RETURN {colorCount: colorCount, ranIterations: ranIterations, didConverge: didConverge} AS row
EOF
} > /tmp/lpg-gds-zipf300.cases

cat > /tmp/lpg-gds-zipf60c.cases <<EOF
zipf60c.gds.wcc.seed.jsonl;;CALL gds.wcc.stream('g', {seedProperty: 'community'}) $COMP
EOF

# GDS's clique counting fails on a projection with self-loops or parallel
# edges (measured: ArrayIndexOutOfBoundsException on zipf60 and zipf300,
# NegativeArraySizeException on uniform60): it also runs on the simple
# undirected graph of each fixture, projected from its distinct node pairs
# (nodes without an edge to another node are not projected).
simple() { echo "MATCH (a)--(b) WHERE a.uid < b.uid WITH DISTINCT a, b WITH gds.graph.project('$1', a, b, {}, {undirectedRelationshipTypes: ['*']}) AS p"; }
for f in zipf60 uniform60 zipf300; do
  echo "$f.gds.cliques.simple.jsonl;;$(simple s) CALL gds.cliqueCounting.stream('s', {}) $CQ" >> /tmp/lpg-gds-$f.cases
done

# lpg's node order is the element identifier order, which its CSV import
# gives in the order of the `id:ID` strings (0, 1, 10, 11, …; measured,
# rmp #68); GDS's native projection numbers nodes in the file's numeric
# order. The order-dependent heuristics (k-spanning tree, Steiner
# rerouting) also run on a Cypher-aggregation projection whose nodes come
# first, in the string order, so that GDS numbers them as lpg does
# (`o<i>`; directed, or undirected with UNDIRECTED).
ordered() { # graph-name extra-configuration
  echo "MATCH (n) WITH n ORDER BY toString(n.uid) WITH collect(n) AS ns WITH ns, [(x)-[r]->(y) | {s: x, t: y, w: r.weight}] AS es UNWIND [n IN ns | {s: n, t: null, w: 0.0}] + es AS row WITH gds.graph.project('$1', row.s, row.t, {relationshipProperties: {weight: row.w}}, {$2}) AS p WITH count(*) AS c"
}
U2="undirectedRelationshipTypes: ['*']"
i=0
for spec in "zipf60 13 0 4 9 17 25 33 41 50 58" "uniform60 14 0 3 11 22 37 45 59" "zipf300 218 0 5 13 50 77 123 200 299"; do
  set -- $spec; f=$1; s=$2; shift 2
  for kind in "" ".reroute" ".unweighted"; do
    i=$((i+1))
    case $kind in
      .reroute) cfg="$W, applyRerouting: true" ;;
      .unweighted) cfg="applyRerouting: false" ;;
      *) cfg="$W" ;;
    esac
    echo "$f.gds.steiner$kind.o.jsonl;;$(ordered o$i '') CALL gds.steinerTree.stream('o$i', {sourceNode: $(N "$s"), targetNodes: $(NS "$@"), $cfg}) $TREE" >> /tmp/lpg-gds-$f.cases
  done
done
for spec in "karate 0" "zipf60 13" "uniform60 14"; do
  set -- $spec; f=$1; s=$2
  i=$((i+1))
  echo "$f.gds.kst.o.jsonl;;$(ordered o$i "$U2") CALL gds.kSpanningTree.write('o$i', {sourceNode: $(N "$s"), k: 5, $W, writeProperty: 'ko$i'}) YIELD effectiveNodeCount WITH effectiveNodeCount AS c MATCH (n) RETURN {uid: n.uid, kst: n.ko$i, effectiveNodeCount: c} AS row" >> /tmp/lpg-gds-$f.cases
  i=$((i+1))
  echo "$f.gds.kst.max.o.jsonl;;$(ordered o$i "$U2") CALL gds.kSpanningTree.write('o$i', {sourceNode: $(N "$s"), k: 4, $W, objective: 'maximum', writeProperty: 'ko$i'}) YIELD effectiveNodeCount WITH effectiveNodeCount AS c MATCH (n) RETURN {uid: n.uid, kst: n.ko$i, effectiveNodeCount: c} AS row" >> /tmp/lpg-gds-$f.cases
  i=$((i+1))
  echo "$f.gds.mst.o.jsonl;;$(ordered o$i "$U2") CALL gds.spanningTree.stream('o$i', {sourceNode: $(N "$s"), $W}) $TREE" >> /tmp/lpg-gds-$f.cases
done

# GDS's prize must be a float property (measured: a LONG property fails
# with "Value type required: [DOUBLE]"): `lat` and `lng` of geo30.
cat > /tmp/lpg-gds-geo30.cases <<EOF
geo30.gds.pcst.jsonl;;CALL gds.prizeSteinerTree.stream('u', {prizeProperty: 'lat', $W}) $TREE
geo30.gds.pcst.write.jsonl;;CALL gds.prizeSteinerTree.write('u', {prizeProperty: 'lat', $W, writeRelationshipType: 'PCST', writeProperty: 'w'}) YIELD effectiveNodeCount, totalWeight, sumOfPrizes RETURN {effectiveNodeCount: effectiveNodeCount, totalWeight: totalWeight, sumOfPrizes: sumOfPrizes} AS row
geo30.gds.pcst.lng.jsonl;;CALL gds.prizeSteinerTree.stream('u', {prizeProperty: 'lng', $W}) $TREE
EOF

cat > /tmp/lpg-gds-flow30.cases <<EOF
flow30.gds.maxflow.jsonl;;CALL gds.maxFlow.stream('g', {sourceNodes: $(NS 4 7), targetNodes: $(NS 28), capacityProperty: 'capacity'}) $FLOW
flow30.gds.maxflow.multi.jsonl;;CALL gds.maxFlow.stream('g', {sourceNodes: $(NS 4 7 9 10 15), targetNodes: $(NS 28 3 14), capacityProperty: 'capacity'}) $FLOW
flow30.gds.maxflow.nocap.jsonl;;CALL gds.maxFlow.stream('g', {sourceNodes: $(NS 4 7), targetNodes: $(NS 28)}) $FLOW
flow30.gds.maxflow.write.jsonl;;CALL gds.maxFlow.write('g', {sourceNodes: $(NS 4 7), targetNodes: $(NS 28), capacityProperty: 'capacity', writeRelationshipType: 'FLOW', writeProperty: 'flow'}) YIELD relationshipsWritten, totalFlow RETURN {relationshipsWritten: relationshipsWritten, totalFlow: totalFlow} AS row
flow30.gds.mincost.jsonl;;CALL gds.maxFlow.minCost.stream('g', {sourceNodes: $(NS 4 7), targetNodes: $(NS 28), capacityProperty: 'capacity', costProperty: 'cost'}) $FLOW
flow30.gds.mincost.multi.jsonl;;CALL gds.maxFlow.minCost.stream('g', {sourceNodes: $(NS 4 7 9 10 15), targetNodes: $(NS 28 3 14), capacityProperty: 'capacity', costProperty: 'cost'}) $FLOW
flow30.gds.mincost.write.jsonl;;CALL gds.maxFlow.minCost.write('g', {sourceNodes: $(NS 4 7), targetNodes: $(NS 28), capacityProperty: 'capacity', costProperty: 'cost', writeRelationshipType: 'MCF', writeProperty: 'flow'}) YIELD relationshipsWritten, totalFlow, totalCost RETURN {relationshipsWritten: relationshipsWritten, totalFlow: totalFlow, totalCost: totalCost} AS row
flow30.gds.mincost.multi.write.jsonl;;CALL gds.maxFlow.minCost.write('g', {sourceNodes: $(NS 4 7 9 10 15), targetNodes: $(NS 28 3 14), capacityProperty: 'capacity', costProperty: 'cost', writeRelationshipType: 'MCF2', writeProperty: 'flow'}) YIELD relationshipsWritten, totalFlow, totalCost RETURN {relationshipsWritten: relationshipsWritten, totalFlow: totalFlow, totalCost: totalCost} AS row
flow30.gds.mincost.alpha.write.jsonl;;CALL gds.maxFlow.minCost.write('g', {sourceNodes: $(NS 4 7), targetNodes: $(NS 28), capacityProperty: 'capacity', costProperty: 'cost', alpha: 2, writeRelationshipType: 'MCF3', writeProperty: 'flow'}) YIELD relationshipsWritten, totalFlow, totalCost RETURN {relationshipsWritten: relationshipsWritten, totalFlow: totalFlow, totalCost: totalCost} AS row
EOF

i=0
for f in ${ONLY:-karate zipf60 uniform60 zipf300 zipf60c flow30 geo30}; do
  # FILTER (an extended regular expression) reruns the matching cases only.
  grep -E "${FILTER:-.}" /tmp/lpg-gds-$f.cases > /tmp/lpg-gds-$f.run || true
  bash "$B" "$T/$f" /tmp/lpg-gds-$f.run "$E" &
  i=$((i+1)); if (( i % 3 == 0 )); then wait; fi
done
wait
rm -f /tmp/lpg-gds-*.cases /tmp/lpg-gds-*.run
