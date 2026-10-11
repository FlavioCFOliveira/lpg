#!/usr/bin/env bash
# Neo4j import check of lpg's CSV exports (rmp #54, #59): runs
# `neo4j-admin database import full` of Neo4j 2026.09.0 community (Docker
# image neo4j:2026.09.0-community, GPL-3.0: run, never copied) on CSV files
# in a directory, starts the server and prints what `cypher-shell` returns.
#
# Usage:
#   neo4j_import_check.sh DIR NODES_CSV EDGES_CSV [CYPHER]
#   neo4j_import_check.sh --probe DIR
#   neo4j_import_check.sh --probe-point-list DIR
#
# DIR is mounted read-only at /import. CYPHER defaults to the node and
# relationship counts and the properties of up to 25 nodes. `--probe DIR`
# writes the CSV probe of the Sprint 4 plan, Appendix A, into DIR and
# checks it: the expected result is 2 nodes, 1 relationship, SRIDs 7203 and
# 4979, and the doubles NaN and Infinity. `--probe-point-list DIR` writes a
# node file with a `point[]` column whose second node holds a point with a
# NaN coordinate (rmp #59 FINDING 449): it measures whether Neo4j refuses
# such a list, as it refuses such a point (FINDING 436).
set -euo pipefail

IMAGE="neo4j:2026.09.0-community"

probe() {
    local dir="$1"
    mkdir -p "$dir"
    cat > "$dir/nodes.csv" <<'CSV'
:ID,:LABEL,loc:point{srid:7203},loc3:point{srid:4979},d:double
n1,A;B,"{x:1.5,y:2.0}","{longitude:1.0,latitude:2.0,height:3.0}",NaN
n2,A,"{x:-1.0,y:0.25,srid:7203}","{longitude:1.0,latitude:2.0,height:3.0}",Infinity
CSV
    cat > "$dir/edges.csv" <<'CSV'
:START_ID,:END_ID,:TYPE
n1,n2,R
CSV
}

probe_point_list() {
    local dir="$1"
    mkdir -p "$dir"
    cat > "$dir/nodes.csv" <<'CSV'
:ID,pts:point[]
n1,"{srid: 7203, x: 1.0, y: 2.0};{srid: 7203, x: 3.0, y: 4.0}"
n2,"{srid: 7203, x: 1.0, y: 2.0};{srid: 7203, x: NaN, y: 4.0}"
CSV
    cat > "$dir/edges.csv" <<'CSV'
:START_ID,:END_ID,:TYPE
n1,n2,R
CSV
}

if [[ "${1:-}" == "--probe-point-list" ]]; then
    dir="$(realpath "$2")"
    probe_point_list "$dir"
    set -- "$dir" nodes.csv edges.csv \
        "MATCH (n) RETURN count(n) AS nodes; MATCH (n) RETURN n.pts ORDER BY id(n);"
fi

if [[ "${1:-}" == "--probe" ]]; then
    dir="$(realpath "$2")"
    probe "$dir"
    set -- "$dir" nodes.csv edges.csv \
        "MATCH (n) RETURN count(n) AS nodes; MATCH ()-[r]->() RETURN count(r) AS relationships; MATCH (n) RETURN n.loc, n.loc3.srid AS srid3, n.d ORDER BY n.d;"
fi

if [[ $# -lt 3 ]]; then
    sed -n '2,16p' "$0"
    exit 2
fi

dir="$(realpath "$1")"
nodes="$2"
edges="$3"
query="${4:-MATCH (n) RETURN count(n) AS nodes; MATCH ()-[r]->() RETURN count(r) AS relationships; MATCH (n) RETURN labels(n), properties(n) LIMIT 25;}"

docker run --rm \
    --volume "$dir:/import:ro" \
    --env "QUERY=$query" \
    --env "NODES=$nodes" \
    --env "EDGES=$edges" \
    "$IMAGE" \
    bash -c '
        set -e
        # The image entrypoint, which reads NEO4J_AUTH, is bypassed here, so
        # authentication is disabled in the configuration directly.
        echo "dbms.security.auth_enabled=false" >> "$NEO4J_HOME/conf/neo4j.conf"
        neo4j-admin --version
        status=0
        neo4j-admin database import full neo4j \
            --overwrite-destination=true \
            --nodes="/import/$NODES" \
            --relationships="/import/$EDGES" > /tmp/import.log 2>&1 || status=$?
        grep -E "IMPORT DONE|^ *[0-9]+ (nodes|relationships)|ERROR|Exception|rror" /tmp/import.log || true
        if [ "$status" -ne 0 ]; then
            echo "IMPORT FAILED with exit status $status:"
            tail -40 /tmp/import.log
            exit "$status"
        fi
        neo4j start >/dev/null
        for _ in $(seq 1 60); do
            if cypher-shell "RETURN 1" >/dev/null 2>&1; then break; fi
            sleep 1
        done
        cypher-shell --format plain "$QUERY"
        neo4j stop >/dev/null
    '
