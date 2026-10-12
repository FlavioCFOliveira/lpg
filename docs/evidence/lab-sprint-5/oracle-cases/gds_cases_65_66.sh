#!/usr/bin/env bash
# rmp #65 and #66: GDS oracle cases, one container per fixture
# (`gds_batch.sh`). Each case is OUT;;QUERY on graph `g` (NATURAL) or `u`
# (UNDIRECTED); OUT is written to src/algo/testdata/expected.
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
B=$L/docs/evidence/lab-sprint-5/gds_batch.sh
U='gds.util.asNode'
N() { echo "COLLECT { MATCH (n {uid: $1}) RETURN n }[0]"; }
SCORE="YIELD nodeId, score RETURN {nodeId: $U(nodeId).uid, score: score} AS row"
RANK="YIELD ranIterations, didConverge RETURN {ranIterations: ranIterations, didConverge: didConverge} AS row"
SIM="YIELD node1, node2, similarity RETURN {node1: $U(node1).uid, node2: $U(node2).uid, similarity: similarity} AS row"
COMM="YIELD nodeId, communityId RETURN {nodeId: $U(nodeId).uid, communityId: communityId} AS row"
HITS="YIELD nodeId, values RETURN {nodeId: $U(nodeId).uid, auth: values.auth, hub: values.hub} AS row"
W="relationshipWeightProperty: 'weight'"

centrality() { # fixture-prefix graph
  local f=$1 g=$2
  cat <<EOF
$f.gds.degree.jsonl;;CALL gds.degree.stream('$g', {}) $SCORE
$f.gds.degree.reverse.weighted.jsonl;;CALL gds.degree.stream('$g', {orientation: 'REVERSE', $W}) $SCORE
$f.gds.degree.undirected.jsonl;;CALL gds.degree.stream('$g', {orientation: 'UNDIRECTED'}) $SCORE
$f.gds.betweenness.jsonl;;CALL gds.betweenness.stream('$g', {}) $SCORE
$f.gds.betweenness.weighted.jsonl;;CALL gds.betweenness.stream('$g', {$W}) $SCORE
$f.gds.closeness.jsonl;;CALL gds.closeness.stream('$g', {}) $SCORE
$f.gds.closeness.wf.jsonl;;CALL gds.closeness.stream('$g', {useWassermanFaust: true}) $SCORE
$f.gds.harmonic.jsonl;;CALL gds.closeness.harmonic.stream('$g', {}) $SCORE
$f.gds.eigenvector.jsonl;;CALL gds.eigenvector.stream('$g', {}) $SCORE
$f.gds.eigenvector.stats.jsonl;;CALL gds.eigenvector.stats('$g', {}) $RANK
$f.gds.eigenvector.weighted.jsonl;;CALL gds.eigenvector.stream('$g', {$W}) $SCORE
$f.gds.eigenvector.converged.jsonl;;CALL gds.eigenvector.stream('$g', {maxIterations: 100, tolerance: 1.0E-9}) $SCORE
$f.gds.eigenvector.converged.stats.jsonl;;CALL gds.eigenvector.stats('$g', {maxIterations: 100, tolerance: 1.0E-9}) $RANK
$f.gds.articlerank.jsonl;;CALL gds.articleRank.stream('$g', {}) $SCORE
$f.gds.articlerank.weighted.jsonl;;CALL gds.articleRank.stream('$g', {$W}) $SCORE
$f.gds.articlerank.converged.jsonl;;CALL gds.articleRank.stream('$g', {maxIterations: 100, tolerance: 1.0E-9}) $SCORE
$f.gds.articlerank.converged.stats.jsonl;;CALL gds.articleRank.stats('$g', {maxIterations: 100, tolerance: 1.0E-9}) $RANK
$f.gds.hits.jsonl;;CALL gds.hits.stream('$g', {}) $HITS
$f.gds.hits.5.jsonl;;CALL gds.hits.stream('$g', {hitsIterations: 5}) $HITS
$f.gds.hits.stats.jsonl;;CALL gds.hits.stats('$g', {hitsIterations: 5}) $RANK
$f.gds.nodesim.jsonl;;CALL gds.nodeSimilarity.stream('$g', {}) $SIM
$f.gds.nodesim.overlap.jsonl;;CALL gds.nodeSimilarity.stream('$g', {similarityMetric: 'OVERLAP', topK: 3, topN: 20}) $SIM
$f.gds.nodesim.cosine.weighted.jsonl;;CALL gds.nodeSimilarity.stream('$g', {similarityMetric: 'COSINE', $W, bottomK: 2}) $SIM
$f.gds.nodesim.jaccard.weighted.jsonl;;CALL gds.nodeSimilarity.stream('$g', {$W, topK: 4}) $SIM
$f.gds.nodesim.cutoffs.jsonl;;CALL gds.nodeSimilarity.stream('$g', {degreeCutoff: 3, similarityCutoff: 0.2, useComponents: true}) $SIM
EOF
}

{
  centrality zipf60 g
  cat <<EOF
zipf60.gds.betweenness.undirected.jsonl;;CALL gds.betweenness.stream('u', {}) $SCORE
zipf60.gds.closeness.undirected.jsonl;;CALL gds.closeness.stream('u', {}) $SCORE
zipf60.gds.harmonic.undirected.jsonl;;CALL gds.closeness.harmonic.stream('u', {}) $SCORE
zipf60.gds.articlerank.undirected.jsonl;;CALL gds.articleRank.stream('u', {}) $SCORE
zipf60.gds.ppr.jsonl;;CALL gds.pageRank.stream('g', {sourceNodes: [$(N 1), $(N 8)], maxIterations: 100, tolerance: 1.0E-9}) $SCORE
zipf60.gds.ppr.stats.jsonl;;CALL gds.pageRank.stats('g', {sourceNodes: [$(N 1), $(N 8)], maxIterations: 100, tolerance: 1.0E-9}) $RANK
zipf60.gds.nodesim.filtered.jsonl;;CALL gds.nodeSimilarity.filtered.stream('g', {sourceNodeFilter: COLLECT { MATCH (n:A) RETURN n }, targetNodeFilter: COLLECT { MATCH (n:B) RETURN n }}) $SIM
zipf60.gds.knn.jsonl;;CALL gds.knn.stream('g', {nodeProperties: 'vec', topK: 5, randomSeed: 42, concurrency: 1}) $SIM
zipf60.gds.knn.filtered.jsonl;;CALL gds.knn.filtered.stream('g', {nodeProperties: 'vec', topK: 5, randomSeed: 42, concurrency: 1, sourceNodeFilter: COLLECT { MATCH (n:A) RETURN n }, targetNodeFilter: COLLECT { MATCH (n:B) RETURN n }}) $SIM
zipf60.gds.celf.jsonl;;CALL gds.influenceMaximization.celf.stream('g', {seedSetSize: 5, randomSeed: 42, concurrency: 1}) YIELD nodeId, spread RETURN {nodeId: $U(nodeId).uid, spread: spread} AS row
zipf60.gds.celf.p1.jsonl;;CALL gds.influenceMaximization.celf.stream('g', {seedSetSize: 4, propagationProbability: 1.0, randomSeed: 42, concurrency: 1}) YIELD nodeId, spread RETURN {nodeId: $U(nodeId).uid, spread: spread} AS row
EOF
  for d in BOTH OUTGOING INCOMING; do
    for pair in "1 8" "26 1" "3 40" "26 26"; do
      set -- $pair
      for fn in adamicAdar commonNeighbors preferentialAttachment resourceAllocation totalNeighbors; do
        echo "zipf60.gds.lp.$fn.$d.$1-$2.jsonl;;MATCH (a {uid: $1}), (b {uid: $2}) RETURN {value: gds.linkprediction.$fn(a, b, {direction: '$d'})} AS row"
      done
    done
  done
  for fn in adamicAdar commonNeighbors preferentialAttachment resourceAllocation totalNeighbors; do
    echo "zipf60.gds.lp.$fn.R.1-8.jsonl;;MATCH (a {uid: 1}), (b {uid: 8}) RETURN {value: gds.linkprediction.$fn(a, b, {relationshipQuery: 'R'})} AS row"
  done
  i=0
  for v in "[1, 2, 3];;[3, 2, 1]" "[1.5, 0, -2];;[0.5, 4, 1]" "[1, 1, 2, 3];;[1, 2, 2, 5.0]" "[1, 2];;[1.0, 2]" "[0, 0, 0];;[1, 2, 3]" "[];;[]" "[1, null, 3];;[2, 2, null]"; do
    a=${v%%;;*}; b=${v#*;;}
    for fn in cosine euclidean euclideanDistance jaccard overlap pearson; do
      echo "zipf60.gds.simfn.$fn.$i.jsonl;;RETURN {value: gds.similarity.$fn($a, $b)} AS row"
    done
    i=$((i+1))
  done
} > /tmp/lpg-gds-zipf60.cases

{
  centrality uniform60 g
  cat <<EOF
uniform60.gds.ppr.jsonl;;CALL gds.pageRank.stream('g', {sourceNodes: [$(N 14)], maxIterations: 100, tolerance: 1.0E-9}) $SCORE
uniform60.gds.ppr.stats.jsonl;;CALL gds.pageRank.stats('g', {sourceNodes: [$(N 14)], maxIterations: 100, tolerance: 1.0E-9}) $RANK
uniform60.gds.knn.jsonl;;CALL gds.knn.stream('g', {nodeProperties: 'vec', topK: 5, randomSeed: 42, concurrency: 1}) $SIM
uniform60.gds.celf.jsonl;;CALL gds.influenceMaximization.celf.stream('g', {seedSetSize: 5, randomSeed: 42, concurrency: 1}) YIELD nodeId, spread RETURN {nodeId: $U(nodeId).uid, spread: spread} AS row
EOF
  for d in BOTH OUTGOING; do
    for pair in "14 15" "3 21"; do
      set -- $pair
      for fn in adamicAdar commonNeighbors preferentialAttachment resourceAllocation totalNeighbors; do
        echo "uniform60.gds.lp.$fn.$d.$1-$2.jsonl;;MATCH (a {uid: $1}), (b {uid: $2}) RETURN {value: gds.linkprediction.$fn(a, b, {direction: '$d'})} AS row"
      done
    done
  done
} > /tmp/lpg-gds-uniform60.cases

# zipf300: blocks of more than one node (n > 64) in every parallel form.
cat > /tmp/lpg-gds-zipf300.cases <<EOF
zipf300.gds.degree.weighted.jsonl;;CALL gds.degree.stream('g', {$W}) $SCORE
zipf300.gds.betweenness.jsonl;;CALL gds.betweenness.stream('g', {}) $SCORE
zipf300.gds.betweenness.weighted.jsonl;;CALL gds.betweenness.stream('g', {$W}) $SCORE
zipf300.gds.betweenness.undirected.jsonl;;CALL gds.betweenness.stream('u', {}) $SCORE
zipf300.gds.closeness.jsonl;;CALL gds.closeness.stream('g', {}) $SCORE
zipf300.gds.harmonic.jsonl;;CALL gds.closeness.harmonic.stream('g', {}) $SCORE
zipf300.gds.eigenvector.jsonl;;CALL gds.eigenvector.stream('g', {}) $SCORE
zipf300.gds.eigenvector.sources.jsonl;;CALL gds.eigenvector.stream('g', {sourceNodes: [$(N 0), $(N 7), $(N 42)], $W}) $SCORE
zipf300.gds.articlerank.jsonl;;CALL gds.articleRank.stream('g', {}) $SCORE
zipf300.gds.hits.jsonl;;CALL gds.hits.stream('g', {}) $HITS
zipf300.gds.nodesim.jsonl;;CALL gds.nodeSimilarity.stream('g', {}) $SIM
zipf300.gds.nodesim.cosine.jsonl;;CALL gds.nodeSimilarity.stream('g', {similarityMetric: 'COSINE', topK: 3}) $SIM
zipf300.gds.nodesim.overlap.weighted.jsonl;;CALL gds.nodeSimilarity.stream('g', {similarityMetric: 'OVERLAP', $W, topK: 3}) $SIM
EOF
cat >> /tmp/lpg-gds-uniform60.cases <<EOF
uniform60.gds.knn.euclidean.jsonl;;CALL gds.knn.stream('g', {nodeProperties: {vec: 'EUCLIDEAN'}, topK: 5, randomSeed: 42, concurrency: 1}) $SIM
uniform60.gds.knn.pearson.jsonl;;CALL gds.knn.stream('g', {nodeProperties: {vec: 'PEARSON'}, topK: 5, randomSeed: 42, concurrency: 1}) $SIM
EOF

MOD="YIELD communityId, modularity RETURN {communityId: communityId, modularity: modularity} AS row"
CON="YIELD community, conductance RETURN {community: community, conductance: conductance} AS row"
cat > /tmp/lpg-gds-zipf60c.cases <<EOF
zipf60c.gds.modularity.jsonl;;CALL gds.modularity.stream('g', {communityProperty: 'community'}) $MOD
zipf60c.gds.modularity.undirected.jsonl;;CALL gds.modularity.stream('u', {communityProperty: 'community'}) $MOD
zipf60c.gds.modularity.weighted.undirected.jsonl;;CALL gds.modularity.stream('u', {communityProperty: 'community', $W}) $MOD
zipf60c.gds.conductance.jsonl;;CALL gds.conductance.stream('g', {communityProperty: 'community'}) $CON
zipf60c.gds.conductance.undirected.jsonl;;CALL gds.conductance.stream('u', {communityProperty: 'community'}) $CON
zipf60c.gds.conductance.weighted.undirected.jsonl;;CALL gds.conductance.stream('u', {communityProperty: 'community', $W}) $CON
EOF

community() { # fixture graph
  local f=$1 g=$2
  cat <<EOF
$f.gds.louvain.jsonl;;CALL gds.louvain.stream('$g', {}) $COMM
$f.gds.louvain.stats.jsonl;;CALL gds.louvain.stats('$g', {}) YIELD modularity, modularities, ranLevels, communityCount RETURN {modularity: modularity, modularities: modularities, ranLevels: ranLevels, communityCount: communityCount} AS row
$f.gds.leiden.jsonl;;CALL gds.leiden.stream('$g', {randomSeed: 42, concurrency: 1}) $COMM
$f.gds.leiden.stats.jsonl;;CALL gds.leiden.stats('$g', {randomSeed: 42, concurrency: 1}) YIELD modularity, modularities, ranLevels, communityCount, didConverge RETURN {modularity: modularity, modularities: modularities, ranLevels: ranLevels, communityCount: communityCount, didConverge: didConverge} AS row
$f.gds.lpa.jsonl;;CALL gds.labelPropagation.stream('$g', {}) $COMM
$f.gds.lpa.stats.jsonl;;CALL gds.labelPropagation.stats('$g', {}) YIELD ranIterations, didConverge, communityCount RETURN {ranIterations: ranIterations, didConverge: didConverge, communityCount: communityCount} AS row
$f.gds.mo.jsonl;;CALL gds.modularityOptimization.stream('$g', {}) $COMM
$f.gds.mo.stats.jsonl;;CALL gds.modularityOptimization.stats('$g', {}) YIELD ranIterations, didConverge, modularity, communityCount RETURN {ranIterations: ranIterations, didConverge: didConverge, modularity: modularity, communityCount: communityCount} AS row
$f.gds.sllpa.jsonl;;CALL gds.sllpa.stream('$g', {maxIterations: 20, concurrency: 1}) YIELD nodeId, values RETURN {nodeId: $U(nodeId).uid, communityIds: values.communityIds} AS row
$f.gds.maxkcut.jsonl;;CALL gds.maxkcut.stream('$g', {randomSeed: 42, concurrency: 1}) $COMM
EOF
}

{
  centrality karate u
  community karate u
} > /tmp/lpg-gds-karate.cases

{
  community rc24 u
  cat <<EOF
rc24.gds.modularity.jsonl;;CALL gds.modularity.stream('g', {communityProperty: 'community'}) YIELD communityId, modularity RETURN {communityId: communityId, modularity: modularity} AS row
rc24.gds.modularity.undirected.jsonl;;CALL gds.modularity.stream('u', {communityProperty: 'community'}) YIELD communityId, modularity RETURN {communityId: communityId, modularity: modularity} AS row
rc24.gds.conductance.jsonl;;CALL gds.conductance.stream('g', {communityProperty: 'community'}) YIELD community, conductance RETURN {community: community, conductance: conductance} AS row
rc24.gds.conductance.undirected.jsonl;;CALL gds.conductance.stream('u', {communityProperty: 'community'}) YIELD community, conductance RETURN {community: community, conductance: conductance} AS row
EOF
} > /tmp/lpg-gds-rc24.cases

{
  community planted60 u
  cat <<EOF
planted60.gds.louvain.weighted.jsonl;;CALL gds.louvain.stream('u', {$W}) $COMM
planted60.gds.louvain.weighted.stats.jsonl;;CALL gds.louvain.stats('u', {$W}) YIELD modularity, ranLevels RETURN {modularity: modularity, ranLevels: ranLevels} AS row
planted60.gds.modularity.jsonl;;CALL gds.modularity.stream('g', {communityProperty: 'community'}) YIELD communityId, modularity RETURN {communityId: communityId, modularity: modularity} AS row
planted60.gds.modularity.weighted.undirected.jsonl;;CALL gds.modularity.stream('u', {communityProperty: 'community', $W}) YIELD communityId, modularity RETURN {communityId: communityId, modularity: modularity} AS row
planted60.gds.conductance.jsonl;;CALL gds.conductance.stream('g', {communityProperty: 'community'}) YIELD community, conductance RETURN {community: community, conductance: conductance} AS row
planted60.gds.conductance.weighted.undirected.jsonl;;CALL gds.conductance.stream('u', {communityProperty: 'community', $W}) YIELD community, conductance RETURN {community: community, conductance: conductance} AS row
planted60.gds.maxkcut.k4.jsonl;;CALL gds.maxkcut.stream('u', {k: 4, randomSeed: 42, concurrency: 1}) $COMM
EOF
  echo "planted60.gds.lp.sameCommunity.jsonl;;MATCH (a), (b) WHERE a.uid IN [0, 5, 20] AND b.uid IN [1, 16, 44] RETURN {a: a.uid, b: b.uid, value: gds.linkprediction.sameCommunity(a, b)} AS row"
  echo "planted60.gds.lp.sameCommunity.other.jsonl;;MATCH (a {uid: 0}), (b {uid: 1}) RETURN {value: gds.linkprediction.sameCommunity(a, b, 'missing')} AS row"
} > /tmp/lpg-gds-planted60.cases

cat > /tmp/lpg-gds-blobs40.cases <<EOF
blobs40.gds.kmeans.jsonl;;CALL gds.kmeans.stream('g', {nodeLabels: ['C'], nodeProperty: 'vec', k: 4, randomSeed: 42, concurrency: 1}) YIELD nodeId, communityId, distanceFromCentroid, silhouette RETURN {nodeId: $U(nodeId).uid, communityId: communityId, distanceFromCentroid: distanceFromCentroid, silhouette: silhouette} AS row
blobs40.gds.kmeans.pp.jsonl;;CALL gds.kmeans.stream('g', {nodeLabels: ['C'], nodeProperty: 'vec', k: 4, initialSampler: 'kmeans++', computeSilhouette: true, randomSeed: 42, concurrency: 1}) YIELD nodeId, communityId, distanceFromCentroid, silhouette RETURN {nodeId: $U(nodeId).uid, communityId: communityId, distanceFromCentroid: distanceFromCentroid, silhouette: silhouette} AS row
blobs40.gds.hdbscan.jsonl;;CALL gds.hdbscan.stream('g', {nodeProperty: 'vec', minClusterSize: 5, samples: 3}) YIELD nodeId, label RETURN {nodeId: $U(nodeId).uid, label: label} AS row
blobs40.gds.hdbscan.default.jsonl;;CALL gds.hdbscan.stream('g', {nodeProperty: 'vec'}) YIELD nodeId, label RETURN {nodeId: $U(nodeId).uid, label: label} AS row
EOF

cat > /tmp/lpg-gds-cyc12.cases <<EOF
cyc12.gds.maxkcut.jsonl;;CALL gds.maxkcut.stream('u', {randomSeed: 42, concurrency: 1}) $COMM
cyc12.gds.maxkcut.k3.jsonl;;CALL gds.maxkcut.stream('u', {k: 3, randomSeed: 42, concurrency: 1}) $COMM
EOF

# GDS's SLLPA rejects `randomSeed` (measured: "Unexpected configuration
# key: randomSeed"); the blobs project their labels so that `nodeLabels`
# selects the clusters.
i=0
for f in ${ONLY:-zipf60 uniform60 karate rc24 planted60 blobs40 cyc12 zipf60c zipf300}; do
  if [[ $f == blobs40 ]]; then labels="['C', 'N']"; else labels=""; fi
  # FILTER (an extended regular expression) reruns the matching cases only.
  grep -E "${FILTER:-.}" /tmp/lpg-gds-$f.cases > /tmp/lpg-gds-$f.run || true
  LABELS="$labels" bash "$B" "$T/$f" /tmp/lpg-gds-$f.run "$E" &
  i=$((i+1)); if (( i % 3 == 0 )); then wait; fi
done
wait
rm -f /tmp/lpg-gds-*.cases /tmp/lpg-gds-*.run
