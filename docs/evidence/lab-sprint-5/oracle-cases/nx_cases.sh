#!/usr/bin/env bash
# rmp #64 and #69: NetworkX oracle cases. OUT DIR FUNCTION PARAMS [flags...]
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
P="/tmp/lpg-lab-venv/bin/python -I $L/docs/evidence/lab-sprint-5/nx_oracle.py"
run() {
  local out=$1 dir=$2 fn=$3 params=$4; shift 4
  if $P "$T/$dir" "$fn" "$params" "$@" > "$E/$out"; then echo "ok $out $(wc -l < "$E/$out")"; else echo "FAIL $out"; rm -f "$E/$out"; fi
}
W='"weight": "weight"'
# 008
run zipf60.nx.ancestors.jsonl zipf60 ancestors '{"source": 26}'
run zipf60.nx.descendants.jsonl zipf60 descendants '{"source": 26}'
run uniform60.nx.ancestors.jsonl uniform60 ancestors '{"source": 14}'
# 009
run zipf60.nx.haspath.1-8.jsonl zipf60 has_path '{"source": 1, "target": 8}'
run zipf60.nx.haspath.8-1.jsonl zipf60 has_path '{"source": 8, "target": 1}'
run uniform60.nx.haspath.14-15.jsonl uniform60 has_path '{"source": 14, "target": 15}'
# 011
run zipf60.nx.idom.jsonl zipf60 immediate_dominators '{"start": 1}'
run zipf60.nx.frontiers.jsonl zipf60 dominance_frontiers '{"start": 1}'
run uniform60.nx.idom.jsonl uniform60 immediate_dominators '{"start": 14}'
run uniform60.nx.frontiers.jsonl uniform60 dominance_frontiers '{"start": 14}'
# 013
run zipf60.nx.sp.length.source.jsonl zipf60 shortest_path_length '{"source": 1}'
run zipf60.nx.sp.length.target.jsonl zipf60 shortest_path_length '{"target": 8}'
run zipf60.nx.sp.length.pair.jsonl zipf60 shortest_path_length '{"source": 1, "target": 8}'
run uniform60.nx.sp.length.all.jsonl uniform60 all_pairs_shortest_path_length '{}'
# 015
run zipf60.nx.bidijkstra.jsonl zipf60 bidirectional_dijkstra "{\"source\": 1, \"target\": 8, $W}"
run uniform60.nx.bidijkstra.jsonl uniform60 bidirectional_dijkstra "{\"source\": 14, \"target\": 15, $W}"
# 020
run negcyc30.nx.negcycle.exists.jsonl negcyc30 negative_edge_cycle "{$W}"
run negw30.nx.negcycle.exists.jsonl negw30 negative_edge_cycle "{$W}"
run negcyc30.nx.negcycle.jsonl negcyc30 find_negative_cycle "{\"source\": 25, $W}"
# 021
run negw30.nx.bellmanford.jsonl negw30 single_source_bellman_ford "{\"source\": 25, $W}"
run zipf60.nx.bellmanford.jsonl zipf60 single_source_bellman_ford "{\"source\": 1, $W}"
# 024, 025
run negw30.nx.floyd.jsonl negw30 floyd_warshall "{$W}"
run zipf60.nx.floyd.jsonl zipf60 floyd_warshall "{$W}"
run uniform60.nx.floyd.jsonl uniform60 floyd_warshall "{$W}"
# 026
run karate.nx.apsp.length.jsonl karate all_pairs_shortest_path_length '{}' --undirected
run cyc12.nx.apsp.length.jsonl cyc12 all_pairs_shortest_path_length '{}' --undirected
run uniform60.nx.apsp.length.undirected.jsonl uniform60 all_pairs_shortest_path_length '{}' --undirected
# 028
run zipf60.nx.allsp.jsonl zipf60 all_shortest_paths '{"source": 1, "target": 8}'
run uniform60.nx.allsp.jsonl uniform60 all_shortest_paths '{"source": 14, "target": 15}'
run zipf60.nx.allsp.weighted.jsonl zipf60 all_shortest_paths "{\"source\": 1, \"target\": 8, $W}"
# 031
run zipf60.nx.simplepaths.jsonl zipf60 all_simple_paths '{"source": 1, "target": 8, "cutoff": 7}'
run cyc12.nx.simplepaths.undirected.jsonl cyc12 all_simple_paths '{"source": 10, "target": 8, "cutoff": 4}' --undirected
# 181
run cyc12.nx.cycles.jsonl cyc12 simple_cycles '{}'
run dag40.nx.cycles.jsonl dag40 simple_cycles '{}'
# 189
run iso100.nx.iso.jsonl iso100 is_isomorphic '{}' --nodes P --other Q
run tour7.nx.iso.jsonl tour7 is_isomorphic '{}' --nodes T --other U
# 194
run bip20.nx.bipartite.jsonl bip20 is_bipartite '{}' --undirected
run karate.nx.bipartite.jsonl karate is_bipartite '{}' --undirected
run petersen.nx.bipartite.jsonl petersen is_bipartite '{}' --undirected
# 203
run tour7.nx.tournament.T.jsonl tour7 is_tournament '{}' --simple --nodes T
run tour7.nx.tournament.U.jsonl tour7 is_tournament '{}' --simple --nodes U
run tour7.nx.dag.T.jsonl tour7 is_directed_acyclic_graph '{}' --simple --nodes T
run tour7.nx.dag.U.jsonl tour7 is_directed_acyclic_graph '{}' --simple --nodes U
run dag40.nx.tournament.jsonl dag40 is_tournament '{}' --simple
# 204
for seq in '[]' '[1]' '[1,1]' '[3,3,2,2]' '[3,3,3,1]' '[4,4,4,4,4]' '[5,5,4,3,2,2,2,1]' '[5,5,5,4,2,1,1,1]' '[2,2,2,2,2,0]'; do
  echo "{\"sequence\": $seq, \"graphical\": $($P x is_graphical "{\"sequence\": $seq}" --nograph | sed 's/{"value": \(.*\)}/\1/')}"
done > $E/nx.graphical.jsonl; echo "ok nx.graphical.jsonl $(wc -l < $E/nx.graphical.jsonl)"
# 205, 206
run dag40.nx.closure.jsonl dag40 transitive_closure_dag '{}' --simple
run dag40.nx.reduction.jsonl dag40 transitive_reduction '{}' --simple
