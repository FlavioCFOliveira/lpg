#!/bin/sh
# Peak heap of one test of a Rust test binary, measured by heaptrack, in
# total and for the allocations whose backtrace contains a function
# (after docs/evidence/lab-sprint-3/heaptrack-tests.sh; rmp #61).
#
# Usage: docs/evidence/lab-sprint-5/heaptrack.sh <test-binary> <test> [SYMBOL]
#
# The test binary comes from `cargo test -p lpg --no-run`; the test runs
# alone (`--exact --ignored --test-threads=1`) under `heaptrack
# --record-only`. Prints the test's `LAB` lines and its peak heap, then,
# when SYMBOL is given, the five largest peak consumers among the
# allocations whose backtrace holds the function whose Rust v0 symbol
# ends with SYMBOL (heaptrack prints v0 symbols mangled), such as
# `4algo10projection5build` for `lpg::algo::projection::build`
# (`heaptrack_print --filter-bt-function -p`; heaptrack notes that the
# peaks of merged backtraces are approximate). The recording goes to a
# temporary directory that is deleted at the end.
set -eu
bin=$1
test=$2
func=${3:-}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
heaptrack --record-only -o "$work/h" "$bin" --exact "$test" --ignored --test-threads=1 --nocapture >"$work/out" 2>&1
grep -o 'LAB.*' "$work/out" || true
printf 'peak heap (whole test): %s\n' \
    "$(heaptrack_print -f "$work"/h.* 2>/dev/null | sed -n 's/^peak heap memory consumption: //p')"
if [ -n "$func" ]; then
    sym=$(heaptrack_print -f "$work"/h.* -p -n 1000 2>/dev/null | grep -o "_R[A-Za-z0-9_]*$func\$" | head -1)
    printf 'peak consumers under %s:\n' "$sym"
    heaptrack_print -f "$work"/h.* --filter-bt-function "$sym" -p -n 5 -s 0 2>/dev/null \
        | grep 'peak memory consumed'
fi
