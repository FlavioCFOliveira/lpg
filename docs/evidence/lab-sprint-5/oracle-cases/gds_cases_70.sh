#!/usr/bin/env bash
# GDS runs of the Sprint 5 exit check (rmp #70), in two parts:
# 1. the rounds of ALG-106 (`gds.knn.stats` `ranIterations`,
#    `didConverge`) on `zipf60` and `uniform60`, with the configuration of
#    the #65 oracle cases (`topK` 5, `randomSeed` 42, `concurrency` 1).
#    Outputs go to `src/algo/testdata/expected`;
#    `algo::tests_exit::rounds_against_the_references` reads them;
# 2. the determinism runs of the parity entries (below); the summary is
#    `raw/gds-determinism-70.txt`.
# Every container is removed by `gds_batch.sh`.
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
B=$L/docs/evidence/lab-sprint-5/gds_batch.sh
RANK="YIELD ranIterations, didConverge RETURN {ranIterations: ranIterations, didConverge: didConverge} AS row"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for f in zipf60 uniform60; do
  echo "$f.gds.knn.stats.jsonl;;CALL gds.knn.stats('g', {nodeProperties: 'vec', topK: 5, randomSeed: 42, concurrency: 1}) $RANK" > "$work/$f.cases"
  bash "$B" "$T/$f" "$work/$f.cases" "$E"
done


# The GDS determinism statements of TS-25 for the parity entries (rmp #70,
# unverified point 9): each GDS procedure of a parity entry runs REPS
# times on `zipf300` in one container, at `concurrency` 4 (the most
# without a licence key, rmp #62) and, for the entries GDS documents as
# deterministic single-threaded with a seed, at `concurrency` 1. Each
# query returns its rows ordered by node `uid` (walks sorted), so only a
# change of result, not of row order, differs. Outputs go to DET_OUT
# (default: a temporary directory, deleted); `det_compare.py` compares
# the repetitions.
REPS=${REPS:-5}
U='gds.util.asNode'
BY="WITH $U(nodeId).uid AS u"
det=$work/det.cases
: > "$det"
q() { # name query
  for i in $(seq 1 "$REPS"); do echo "det.$1.$i.jsonl;;$2" >> "$det"; done
}
q 006.c4 "CALL gds.randomWalk.stream('g', {walkLength: 10, walksPerNode: 2, randomSeed: 42, concurrency: 4}) YIELD nodeIds WITH [x IN nodeIds | $U(x).uid] AS w ORDER BY w RETURN {r: collect(w)} AS row"
q 059.c4 "CALL gds.influenceMaximization.celf.stream('g', {seedSetSize: 5, randomSeed: 42, concurrency: 4}) YIELD nodeId, spread $BY, spread ORDER BY u RETURN {r: collect([u, spread])} AS row"
q 061.c4 "CALL gds.louvain.stream('u', {concurrency: 4}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 062.c4 "CALL gds.leiden.stream('u', {randomSeed: 42, concurrency: 4}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 063.c4 "CALL gds.labelPropagation.stream('u', {concurrency: 4}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 064.c4 "CALL gds.sllpa.stream('u', {maxIterations: 10, concurrency: 4}) YIELD nodeId, values $BY, values.communityIds AS c ORDER BY u RETURN {r: collect([u, c])} AS row"
q 064.c1 "CALL gds.sllpa.stream('u', {maxIterations: 10, concurrency: 1}) YIELD nodeId, values $BY, values.communityIds AS c ORDER BY u RETURN {r: collect([u, c])} AS row"
q 065.c4 "CALL gds.modularityOptimization.stream('u', {concurrency: 4}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 078.c4 "CALL gds.maxkcut.stream('u', {concurrency: 4}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 078.c1 "CALL gds.maxkcut.stream('u', {randomSeed: 42, concurrency: 1}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 080.c4 "CALL gds.kmeans.stream('g', {nodeProperty: 'vec', k: 4, randomSeed: 42, concurrency: 4}) YIELD nodeId, communityId $BY, communityId ORDER BY u RETURN {r: collect([u, communityId])} AS row"
q 081.c4 "CALL gds.hdbscan.stream('g', {nodeProperty: 'vec', concurrency: 4}) YIELD nodeId, label $BY, label ORDER BY u RETURN {r: collect([u, label])} AS row"
q 106.c4 "CALL gds.knn.stream('g', {nodeProperties: 'vec', topK: 5, concurrency: 4}) YIELD node1, node2, similarity WITH $U(node1).uid AS a, $U(node2).uid AS b, similarity ORDER BY a, b RETURN {r: collect([a, b, similarity])} AS row"
q 106.c1 "CALL gds.knn.stream('g', {nodeProperties: 'vec', topK: 5, randomSeed: 42, concurrency: 1}) YIELD node1, node2, similarity WITH $U(node1).uid AS a, $U(node2).uid AS b, similarity ORDER BY a, b RETURN {r: collect([a, b, similarity])} AS row"
q 176.c4 "CALL gds.k1coloring.stream('g', {concurrency: 4}) YIELD nodeId, color $BY, color ORDER BY u RETURN {r: collect([u, color])} AS row"
out=${DET_OUT:-$work/det}
mkdir -p "$out"
bash "$B" "$T/zipf300" "$det" "$out"
python3 -I "$(dirname "$0")/det_compare.py" "$out"
