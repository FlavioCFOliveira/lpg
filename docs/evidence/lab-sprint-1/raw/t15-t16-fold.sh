#!/bin/bash
# perf.data -> demangled folded stacks (c++filt handles Rust v0 symbols) -> SVG
perf script -i "$1" 2>/dev/null | c++filt | sed 's/\[[0-9a-f]\{16\}\]//g' | inferno-collapse-perf > "$2.folded"
inferno-flamegraph --title "$3" "$2.folded" > "$2.svg"
