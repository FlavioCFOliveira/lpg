#!/usr/bin/env bash
# The test gates of TS-30 in one command (docs/technical-spec/quality-and-testing.md,
# "Gate runner"). The independent gates run at once, each in its own target
# directory under /dev/shm/lpg-target, so no gate waits on another's cargo
# lock: fmt, clippy, clippy under `--cfg fuzzing`, nextest, loom and
# shuttle. The stress profile then runs alone: its load of 16 copies per
# test is part of its design.
#
#     LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs .config/gates.sh
#
# The lab variables, when set, reach nextest and the stress profile. Each
# gate's output goes to $LPG_GATE_LOGS (default: a new directory under
# /tmp), and one line per gate gives its exit status and wall time. The
# single-use target directories (cfgfuzz, loom, shuttle) are deleted at the
# end; the main one and clippy's are kept for the next incremental build.
# Exits 0 only if every gate passed.
set -u
cd "$(dirname "$0")/.."
t=/dev/shm/lpg-target
logs=${LPG_GATE_LOGS:-$(mktemp -d /tmp/lpg-gates.XXXXXX)}
mkdir -p "$logs"
echo "gate logs: $logs"

# gate NAME COMMAND...: runs COMMAND with its output in $logs/NAME.log and
# prints its exit status and wall time.
gate() {
    local name=$1 start=$SECONDS
    shift
    "$@" >"$logs/$name.log" 2>&1
    local rc=$?
    echo "$name: exit $rc, $((SECONDS - start)) s"
    return $rc
}

start=$SECONDS
pids=() names=()
run() { gate "$@" & pids+=($!); names+=("$1"); }
run fmt cargo fmt --all -- --check
run clippy cargo clippy --all-targets --target-dir "$t/clippy" -- -D warnings
run cfgfuzz env RUSTFLAGS="--cfg fuzzing" cargo clippy --lib --target-dir "$t/cfgfuzz" -- -D warnings
run nextest cargo nextest run -p lpg --no-fail-fast
run loom env RUSTFLAGS="--cfg loom" cargo test -p lpg --lib --release --target-dir "$t/loom" loom_
run shuttle env RUSTFLAGS="--cfg shuttle" cargo test -p lpg --lib --target-dir "$t/shuttle" shuttle_
failed=0
for i in "${!pids[@]}"; do
    wait "${pids[$i]}" || { failed=$((failed + 1)); echo "FAILED: ${names[$i]} ($logs/${names[$i]}.log)"; }
done
rm -rf "$t/cfgfuzz" "$t/loom" "$t/shuttle"
echo "concurrent gates: $((SECONDS - start)) s"
gate stress cargo nextest run -p lpg --profile stress --no-fail-fast ||
    { failed=$((failed + 1)); echo "FAILED: stress ($logs/stress.log)"; }
echo "all gates: $((SECONDS - start)) s, $failed failed"
exit $((failed > 0))
