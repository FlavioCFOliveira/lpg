#!/usr/bin/env bash
# rmp #65: MAGE oracle cases of ALG-041 (`katz_centrality.get`). OUT DIR QUERY
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
M=$L/docs/evidence/lab-sprint-5/mage_oracle.sh
run() {
  if bash "$M" "$T/$2" "$3" > "$E/$1.tmp" 2> "/tmp/$1.err"; then mv "$E/$1.tmp" "$E/$1"; echo "ok $1 $(wc -l < "$E/$1")"; else echo "FAIL $1"; rm -f "$E/$1.tmp"; tail -3 "/tmp/$1.err"; fi
  rm -f "/tmp/$1.err"
}
K="YIELD node, rank RETURN node, rank"
run karate.mage.katz.jsonl karate "CALL katz_centrality.get() $K" &
run cyc12.mage.katz.jsonl cyc12 "CALL katz_centrality.get(0.1, 0.001) $K" &
run uniform60.mage.katz.jsonl uniform60 "CALL katz_centrality.get(0.05, 0.01) $K" &
run zipf60.mage.katz.jsonl zipf60 "CALL katz_centrality.get(0.01, 0.0001) $K" &
wait
