#!/usr/bin/env bash
# rmp #66: NetworkX oracle cases of ALG-082 (modularity of the planted
# partitions, undirected, as the `UNDIRECTED` orientation reads them).
# OUT DIR FUNCTION PARAMS [flags...]
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
P="/tmp/lpg-lab-venv/bin/python -I $L/docs/evidence/lab-sprint-5/nx_oracle.py"
run() {
  local out=$1 dir=$2 fn=$3 params=$4; shift 4
  if $P "$T/$dir" "$fn" "$params" "$@" > "$E/$out"; then echo "ok $out $(wc -l < "$E/$out")"; else echo "FAIL $out"; rm -f "$E/$out"; fi
}
# NetworkX's `modularity` weighs by the `weight` attribute unless
# `weight` is null. The planted partitions: consecutive uids, 6 per
# clique (rc24), 15 per group (planted60).
groups() { python3 -c "import json; print(json.dumps([list(range(i*$2, (i+1)*$2)) for i in range($1)]))"; }
RC=$(groups 4 6)
PL=$(groups 4 15)
run rc24.nx.modularity.jsonl rc24 community.modularity "{\"communities\": $RC, \"weight\": null}" --undirected
run planted60.nx.modularity.jsonl planted60 community.modularity "{\"communities\": $PL, \"weight\": null}" --undirected
run planted60.nx.modularity.weighted.jsonl planted60 community.modularity "{\"communities\": $PL, \"weight\": \"weight\"}" --undirected
# zipf60c: the `community` column of its nodes.
ZC=$(python3 -c "
import csv, json
g = {}
for r in csv.DictReader(open('$T/zipf60c/nodes.csv')):
    g.setdefault(int(r['community:long']), []).append(int(r['uid:long']))
print(json.dumps([g[k] for k in sorted(g)]))")
run zipf60c.nx.modularity.jsonl zipf60c community.modularity "{\"communities\": $ZC, \"weight\": null}" --undirected
run zipf60c.nx.modularity.weighted.jsonl zipf60c community.modularity "{\"communities\": $ZC, \"weight\": \"weight\"}" --undirected
