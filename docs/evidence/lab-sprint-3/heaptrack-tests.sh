#!/bin/sh
# Peak heap per test of a Rust test binary, measured by heaptrack.
#
# Usage: docs/evidence/lab-sprint-3/heaptrack-tests.sh <test-binary> [FILTER] > peaks.tsv
#
# The test binary comes from `cargo test -p lpg --no-run` (its path is
# printed as "Executable ..."). Each test whose name contains FILTER runs
# alone (`--exact --test-threads=1`) under `heaptrack --record-only`;
# heaptrack_print reports its peak heap. Prints: test, peak heap. The
# recordings go to a temporary directory that is deleted at the end.
set -eu
bin=$1
filter=${2:-}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf 'test\tpeak_heap\n'
"$bin" --list --format terse ${filter:+"$filter"} | sed -n 's/: test$//p' | while read -r t; do
    heaptrack --record-only -o "$work/h" "$bin" --exact "$t" --test-threads=1 -q >/dev/null 2>&1
    peak=$(heaptrack_print "$work"/h.* 2>/dev/null | sed -n 's/^peak heap memory consumption: //p')
    printf '%s\t%s\n' "$t" "$peak"
    rm -f "$work"/h.*
done
