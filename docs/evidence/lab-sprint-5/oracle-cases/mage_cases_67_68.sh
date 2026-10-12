#!/usr/bin/env bash
# rmp #68: MAGE oracle cases of ALG-178 (`set_cover.greedy`). OUT DIR QUERY
# MAGE's "greedy" solver shuffles the sets with Python's `random` and
# takes them in that order until every element is covered
# (`python/mage/constraint_programming/solver.py:128-139`), so its cover
# size varies between runs: three runs are recorded.
set -u
L=$(cd "$(dirname "$0")/../../../.." && pwd)
T=$L/src/algo/testdata
E=$T/expected
M=$L/docs/evidence/lab-sprint-5/mage_oracle.sh
run() {
  if bash "$M" "$T/$2" "$3" > "$E/$1.tmp" 2> "/tmp/$1.err"; then mv "$E/$1.tmp" "$E/$1"; echo "ok $1 $(wc -l < "$E/$1")"; else echo "FAIL $1"; rm -f "$E/$1.tmp"; tail -3 "/tmp/$1.err"; fi
  rm -f "/tmp/$1.err"
}
# Each edge L → R of bip20 is one (element, set) pair: the R node is the
# element, the L node the set.
Q="MATCH (s:L)-[]->(e:R) WITH collect(e) AS es, collect(s) AS ss CALL set_cover.greedy(es, ss) YIELD containing_set RETURN containing_set"
for i in 1 2 3; do
  run bip20.mage.setcover.$i.jsonl bip20 "$Q" &
done
wait
