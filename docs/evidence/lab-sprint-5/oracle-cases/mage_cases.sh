#!/usr/bin/env bash
# rmp #64 and #69: MAGE oracle cases. OUT DIR QUERY
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
M=$L/docs/evidence/lab-sprint-5/mage_oracle.sh
run() {
  if bash "$M" "$T/$2" "$3" > "$E/$1.tmp" 2> "$1.err"; then mv "$E/$1.tmp" "$E/$1"; echo "ok $1 $(wc -l < "$E/$1")"; else echo "FAIL $1"; rm -f "$E/$1.tmp"; tail -3 "$1.err"; fi
}
S="MATCH (s {uid: 26})"
run zipf60.mage.athop.all2.jsonl zipf60 "$S CALL neighbors.at_hop(s, [], 2) YIELD nodes RETURN nodes" &
run zipf60.mage.athop.out2.jsonl zipf60 "$S CALL neighbors.at_hop(s, ['R>'], 2) YIELD nodes RETURN nodes" &
run zipf60.mage.athop.in1.jsonl zipf60 "$S CALL neighbors.at_hop(s, ['<S', 'R'], 1) YIELD nodes RETURN nodes" &
wait
run zipf60.mage.byhop.all3.jsonl zipf60 "$S CALL neighbors.by_hop(s, [], 3) YIELD nodes RETURN nodes" &
run zipf60.mage.byhop.out4.jsonl zipf60 "$S CALL neighbors.by_hop(s, ['R>', 'S>'], 4) YIELD nodes RETURN nodes" &
run cyc12.mage.expand.all.jsonl cyc12 "MATCH (s {uid: 5}) CALL path.expand(s, [], [], 0, 3) YIELD result RETURN result" &
wait
run cyc12.mage.expand.filtered.jsonl cyc12 "MATCH (s {uid: 5}) CALL path.expand([s], ['R>', '<S'], ['+A', '-X'], 1, 3) YIELD result RETURN result" &
run cyc12.mage.expand.both.jsonl cyc12 "MATCH (s {uid: 5}) CALL path.expand(s, ['<R>', 'S'], ['>B'], 1, 3) YIELD result RETURN result" &
run zipf60.mage.subgraphnodes.jsonl zipf60 "$S CALL path.subgraph_nodes(s, {maxHops: 2}) YIELD nodes RETURN nodes" &
wait
run zipf60.mage.subgraphnodes.filtered.jsonl zipf60 "$S CALL path.subgraph_nodes(s, {relationshipFilter: ['R>'], labelFilter: ['+B', '/A'], maxHops: 3}) YIELD nodes RETURN nodes" &
run zipf60.mage.subgraphnodes.both.jsonl zipf60 "$S CALL path.subgraph_nodes(s, {relationshipFilter: ['<R>', 'S>'], maxHops: 3, filterStartNode: false}) YIELD nodes RETURN nodes" &
run zipf60.mage.subgraphall.jsonl zipf60 "$S CALL path.subgraph_all(s, {relationshipFilter: ['R>'], maxHops: 2}) YIELD nodes, rels RETURN nodes, rels" &
wait
run petersen.mage.cycles.jsonl petersen "CALL cycles.get() YIELD cycle_id, node RETURN cycle_id, node" &
run cyc12.mage.cycles.jsonl cyc12 "CALL cycles.get() YIELD cycle_id, node RETURN cycle_id, node" &
run zipf60.mage.simplepaths.out.jsonl zipf60 "MATCH (s {uid: 1}), (t {uid: 8}) CALL algo.all_simple_paths(s, t, ['>'], 7) YIELD path RETURN path" &
wait
run cyc12.mage.simplepaths.any.jsonl cyc12 "MATCH (s {uid: 10}), (t {uid: 8}) CALL algo.all_simple_paths(s, t, [], 4) YIELD path RETURN path" &
run zipf60.mage.ancestors.jsonl zipf60 "$S CALL graph_util.ancestors(s) YIELD ancestors RETURN ancestors" &
run zipf60.mage.descendants.jsonl zipf60 "$S CALL graph_util.descendants(s) YIELD descendants RETURN descendants" &
wait
