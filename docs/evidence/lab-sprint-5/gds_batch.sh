#!/usr/bin/env bash
# Batched GDS oracle of the Sprint 5 laboratory (rmp #65, #66): runs many
# Cypher queries against Neo4j Graph Data Science on one oracle graph in
# one container, and writes the rows of each query as JSON Lines.
#
# Usage:
#   gds_batch.sh DIR CASES OUTDIR
#
# DIR holds the `nodes.csv` and `edges.csv` that `src/lab/generator.rs`
# writes. The container (`neo4j:2026.09.0-community` with its bundled GDS
# 2026.09.0, installed as `gds_oracle.sh` installs it) imports them, then
# projects two graphs: `g` with orientation NATURAL and `u` with
# orientation UNDIRECTED, each with every node and relationship, the
# `weight`, `capacity` and `cost` relationship properties that `edges.csv`
# has (rmp #68), and every node
# property column of `nodes.csv` except `uid` (`vec`, `community`, `lat`,
# `lng`). Nodes are projected under one label, `ALL`; with LABELS set (a
# Cypher list such as `['C', 'N']`) under those labels instead, so that
# `nodeLabels` can select them (every node must have one of them).
#
# CASES is a file of lines `OUT;;QUERY`: QUERY is a Cypher statement that
# returns one map column (it names graph `g` or `u`); its rows are
# written, one JSON object per line, to OUTDIR/OUT. A line that starts
# with `#` is skipped. Each query runs with `cypher-shell --format plain`,
# and the output is rewritten as `gds_oracle.sh` rewrites it. A failing
# query writes its error to OUTDIR/OUT.err and the next query runs. The
# container is removed.
set -euo pipefail

IMAGE="neo4j:2026.09.0-community"

if [[ $# -ne 3 ]]; then
    sed -n '2,24p' "$0"
    exit 2
fi

dir="$(realpath "$1")"
cases="$(realpath "$2")"
out="$(realpath "$3")"

props=()
for p in weight capacity cost; do
    if head -1 "$dir/edges.csv" | tr ',' '\n' | grep -qx "$p:double"; then
        props+=("$p: {property: '$p'}")
    fi
done
nprops=()
for c in $(head -1 "$dir/nodes.csv" | tr ',' '\n'); do
    name=${c%%:*}
    case "$c" in
        uid:*|id:ID|:LABEL) ;;
        *:double|*:long|*:double\[\]) nprops+=("'$name'") ;;
    esac
done
nodes="'*'"
if [[ -n "${LABELS:-}" ]]; then
    nodes="$LABELS"
    if [[ ${#nprops[@]} -gt 0 ]]; then
        nodes="{$(for l in $(echo "$LABELS" | tr -d "[]' " | tr ',' ' '); do printf "%s: {label: '%s', properties: [%s]}, " "$l" "$l" "$(IFS=,; echo "${nprops[*]}")"; done | sed 's/, $//')}"
    fi
elif [[ ${#nprops[@]} -gt 0 ]]; then
    nodes="{ALL: {label: '*', properties: [$(IFS=,; echo "${nprops[*]}")]}}"
fi
rel() { echo "{ALL: {type: '*', orientation: '$1', properties: {$(IFS=,; echo "${props[*]:-}")}}}"; }
project="CALL gds.graph.project('g', $nodes, $(rel NATURAL)) YIELD graphName WITH 1 AS x CALL gds.graph.project('u', $nodes, $(rel UNDIRECTED)) YIELD graphName RETURN graphName"

name="lpg-gds-batch-$$"
cleanup() { docker rm -f "$name" > /dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$name" --volume "$dir:/import:ro" --volume "$cases:/cases.txt:ro" \
    --env "PROJECT=$project" "$IMAGE" sleep infinity > /dev/null
docker exec "$name" bash -c '
    set -e
    conf="$NEO4J_HOME/conf/neo4j.conf"
    echo "dbms.security.auth_enabled=false" >> "$conf"
    jar=$(jq --raw-output ".\"graph-data-science\".location" /startup/neo4j-plugins.json)
    cp $jar "$NEO4J_HOME/plugins/"
    echo "dbms.security.procedures.unrestricted=gds.*" >> "$conf"
    neo4j-admin database import full neo4j --overwrite-destination=true \
        --nodes=/import/nodes.csv --relationships=/import/edges.csv > /tmp/import.log 2>&1 \
        || { tail -40 /tmp/import.log >&2; exit 3; }
    neo4j start > /dev/null
    for _ in $(seq 1 90); do
        if cypher-shell "RETURN 1" > /dev/null 2>&1; then break; fi
        sleep 1
    done
    cypher-shell "$PROJECT" > /dev/null
'
while IFS= read -r line; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    file=${line%%;;*}
    query=${line#*;;}
    if docker exec --env "Q=$query" "$name" bash -c 'cypher-shell --format plain "$Q"' < /dev/null \
        > "$out/$file.raw" 2> "$out/$file.err"; then
        tail -n +2 "$out/$file.raw" | sed -E -e 's/([{,] *)([A-Za-z_][A-Za-z0-9_]*): /\1"\2": /g' \
            -e 's/: TRUE([,}])/: true\1/g' -e 's/: FALSE([,}])/: false\1/g' \
            -e 's/: NULL([,}])/: null\1/g' -e 's/: NaN([,}])/: "nan"\1/g' -e 's/: Infinity([,}])/: "inf"\1/g' > "$out/$file"
        rm -f "$out/$file.raw" "$out/$file.err"
        echo "ok $file $(wc -l < "$out/$file")"
    else
        rm -f "$out/$file.raw"
        echo "FAIL $file: $(tail -2 "$out/$file.err")"
    fi
done < "$cases"
