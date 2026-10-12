#!/usr/bin/env bash
# GDS oracle of the Sprint 5 laboratory (rmp #61): runs one Neo4j Graph
# Data Science call on an oracle graph and prints its rows as JSON Lines.
#
# Usage:
#   gds_oracle.sh DIR PROCEDURE [CONFIG] [YIELD_RETURN] [ORIENTATION]
#
# DIR holds the `nodes.csv` and `edges.csv` that `src/lab/generator.rs`
# writes (Neo4j import layout; nodes keyed by their `uid` property). The
# script starts `neo4j:2026.09.0-community` (GPL-3.0; run, never copied)
# with its bundled GDS 2026.09.0 installed as the image's entrypoint
# installs it for NEO4J_PLUGINS='["graph-data-science"]' (the jar of
# `/startup/neo4j-plugins.json` "location" copied into `plugins/`, and
# `dbms.security.procedures.unrestricted=gds.*`); the entrypoint itself is
# bypassed so that `neo4j-admin database import full` runs first. It then
# projects every node and every relationship as graph `g` with ORIENTATION
# (`NATURAL` by default) and the `weight`, `capacity` and `cost` properties
# that `edges.csv` has, and runs
#
#   CALL PROCEDURE('g', CONFIG) YIELD_RETURN
#
# CONFIG is a Cypher map (default `{}`; `concurrency` at most 4, the limit
# without a licence key). YIELD_RETURN must return one map column; the
# default suits the `Score` procedures:
#   YIELD nodeId, score RETURN {nodeId: gds.util.asNode(nodeId).uid, score: score} AS row
# Each returned map is printed as one JSON line (cypher-shell's plain map,
# with its keys quoted and TRUE, FALSE and NULL in lower case). The container is removed.
# Exit status: that of the call (non-zero for an unknown procedure).
set -euo pipefail

IMAGE="neo4j:2026.09.0-community"

if [[ $# -lt 2 ]]; then
    sed -n '2,27p' "$0"
    exit 2
fi

dir="$(realpath "$1")"
procedure="$2"
config="${3:-}"
ret="${4:-}"
orientation="${5:-NATURAL}"
[[ -n "$config" ]] || config='{}'
[[ -n "$ret" ]] || ret='YIELD nodeId, score RETURN {nodeId: gds.util.asNode(nodeId).uid, score: score} AS row'

props=()
for p in weight capacity cost; do
    if head -1 "$dir/edges.csv" | tr ',' '\n' | grep -qx "$p:double"; then
        props+=("$p: {property: '$p'}")
    fi
done
projection="{ALL: {type: '*', orientation: '$orientation', properties: {$(IFS=,; echo "${props[*]:-}")}}}"
query="CALL gds.graph.project('g', '*', $projection) YIELD graphName WITH graphName CALL $procedure(graphName, $config) $ret"

docker run --rm \
    --volume "$dir:/import:ro" \
    --env "QUERY=$query" \
    "$IMAGE" \
    bash -c '
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
        cypher-shell --format plain "$QUERY" > /tmp/out.txt
        neo4j stop > /dev/null
        tail -n +2 /tmp/out.txt
    ' | sed -E -e 's/([{,] *)([A-Za-z_][A-Za-z0-9_]*): /\1"\2": /g' \
        -e 's/: TRUE([,}])/: true\1/g' -e 's/: FALSE([,}])/: false\1/g' -e 's/: NULL([,}])/: null\1/g'
