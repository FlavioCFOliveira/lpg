#!/usr/bin/env bash
# nextest wrapper of the `stress` profile (TS-30 R7, rmp #20): runs one test
# as LPG_STRESS_COPIES (default 16) concurrent copies of its process, each
# with its own seed, base + copy, where the base is LPG_SEED if set, else
# random (R5). Prints the seeds; fails if any copy fails, printing that
# copy's output and seed.
set -u
copies=${LPG_STRESS_COPIES:-16}
base=${LPG_SEED:-$(od -An -N4 -tu4 /dev/urandom | tr -d ' ')}
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
pids=()
for i in $(seq 0 $((copies - 1))); do
    LPG_SEED=$((base + i)) "$@" >"$out/$i" 2>&1 &
    pids+=($!)
done
failed=0
for i in $(seq 0 $((copies - 1))); do
    if ! wait "${pids[$i]}"; then
        failed=$((failed + 1))
        echo "copy $i FAILED with LPG_SEED=$((base + i)):"
        cat "$out/$i"
    fi
done
echo "stress: $copies copies, LPG_SEED $base to $((base + copies - 1)), $failed failed"
exit $((failed > 0))
