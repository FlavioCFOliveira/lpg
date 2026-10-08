---
title: Evidence
status: frozen records
last-reviewed: 2026-10-08
related:
  - ../technical-spec/README.md
---

# Evidence

This folder keeps the research notes, measurement reports and experiment sources that the [technical specification](../technical-spec/README.md) cites. Each item is a record of the day it was made. Later decisions live in the specification, not here.

## Index

| ID | Folder | What it studied or measured | Date | Reproduce |
|---|---|---|---|---|
| P-STO | [research/research-01-storage.md](research/research-01-storage.md) | Storage engine: build vs reuse, pages, buffer pool, WAL, torn pages, checksums, backup, lock, file layout | 2026-09-24 | Read only (source study) |
| P-TX | [research/research-02-layout-mvcc.md](research/research-02-layout-mvcc.md) | Graph layout, MVCC, serializable validation, locks, indexes | 2026-09-24 | Read only (source study) |
| M-QE | [research/research-03-query-engine.md](research/research-03-query-engine.md) | Parser, binder, planner, execution, values, catalog, procedures, conformance; measurements in §Q1.3, §Q4.2, §Q5 | 2026-09-24 | See the experiment folders below |
| P-TEST | [research/research-04-turso-libsql.md](research/research-04-turso-libsql.md) | Turso and libSQL kernel components and test dimensions compared with lpg Sprint 1; test-speed and flaky-test measurements in §5 | 2026-10-03 | Read only (source study). §5.2: `cargo nextest run -p lpg` with `CARGO_PROFILE_TEST_OPT_LEVEL=0` and `=1`; `TMPDIR=/dev/shm` for the tmpfs run. The record used `CARGO_PROFILE_DEV_OPT_LEVEL=1`, which no longer changes the test build since `Cargo.toml` sets `[profile.test] opt-level = 1` |
| R-REG | [research/decision-register.md](research/decision-register.md) | Phase-1 decision register that preceded the technical specification | 2026-10-02 | Read only |
| M-LAYOUT | [layout-bench/](layout-bench/results.md) | Seven adjacency layouts at 100 %, 25 %, 10 % and 1 % cache; 1 M nodes / 10 M edges; 10 M nodes / 100 M edges | 2026-10-02 | `cargo build --release` in `layout-bench/` and `layout-bench/src-v1/`, then `scripts/run-1m.sh` and `scripts/run-ram.sh` |
| M-SIM | [layout-sim/](layout-sim/pagesim.py) | Page-touch model of five layouts (cited by P-TX §L1.3) | 2026-09-24 | `python3 layout-sim/pagesim.py 100000 1000000 0.8` |
| M-PARSE | [antlr-exp/](antlr-exp/bench/src/main.rs) | antlr4rust parser from `GQL.g4` against `sqlparser` (M-QE §Q1.3) | 2026-09-24 | Generate the parser (below), then `cargo run --release -- ../corpus 2000` in `antlr-exp/bench/`; `cargo run --release` in `antlr-exp/sqlbench/` |
| M-TOKIO | [tokio-exp/](tokio-exp/src/main.rs) | Cost of the `spawn_blocking` and channel hand-offs (M-QE §Q4.2) | 2026-09-24 | `cargo run --release` in `tokio-exp/` |
| M-EXEC | [exec-exp/](exec-exp/e.rs) | Row-at-a-time against batch execution in a toy pipeline (M-QE §Q4.2) | 2026-09-24 | `rustc -O exec-exp/e.rs -o e && ./e` |
| M-VALUE | [value-exp/](value-exp/v.rs) | `Value` enum sizes (`v.rs`, `w.rs`), clock-read cost (`t.rs`), `rdtsc` under `forbid(unsafe_code)` (`r.rs`, expected not to compile) (M-QE §Q4.2, §Q5) | 2026-09-24 | `rustc -O value-exp/<file>.rs -o x && ./x` |
| M-FEAT | [planning/round-s/](planning/round-s/README.md) | `GQL.g4` rule-to-feature classification; TCK and gql-compat selection counts; ambiguous `GQL.g4` decisions; keyword differences with the ISO BNF; Unicode version of the oracle's ANTLR jar | 2026-10-08 | Commands in [planning/round-s/README.md](planning/round-s/README.md) |
| M-LOCK | [locktest/](locktest/main.rs) | `File::try_lock` within one process and across processes (P-STO §Q8) | 2026-09-24 | `rustc -O locktest/main.rs -o lt && ./lt` (needs `flock(1)`) |

**Hardware for every measurement**: the reference hardware of [NFR-PERF-006](../functional-spec/non-functional-requirements.md#performance): AMD Ryzen 9 5900HX (8 cores, 16 threads), about 31 GiB RAM, Samsung 970 EVO Plus 1 TB NVMe with ext4, Linux 6.8, rustc 1.96.0. M-LAYOUT records its full environment in [layout-bench/results.md](layout-bench/results.md).

## Notes

- **Standalone crates.** Each Cargo project here (`layout-bench`, `layout-bench/src-v1`, `tokio-exp`, `antlr-exp/bench`, `antlr-exp/sqlbench`) declares its own empty `[workspace]` and `publish = false`. None may become a member of the `lpg` workspace. The `lpg` package should exclude `docs/evidence/` from what it publishes.
- **Paths inside the records.** The notes and reports were written in scratch directories. Their old paths map to this folder as follows:
  - `scratchpad/research-0N-*.md` → `research/`
  - `lb-results/layout-bench-results.md` → `layout-bench/results.md`
  - `lb-results/` → `layout-bench/raw/`
  - `bin-v1/src` → `layout-bench/src-v1/src` (the device matrix)
  - `bin-v2/src` and `layout-bench/src` → `layout-bench/src`
  - other `scratchpad/<name>/` → `<name>/`

  Paths such as `/tmp/neo4j` point to the reference clones; their projects and commits are listed in the [technical specification sources](../technical-spec/README.md#sources).
- **Binaries.** Only their SHA-256 hashes are kept ([layout-bench/SHA256-binaries.txt](layout-bench/SHA256-binaries.txt)).
- **Generated parser for M-PARSE.** It is not stored, because it can be regenerated:
  1. Clone opengql/grammar at commit `16ea71b`.
  2. Take the ANTLR tool jar `antlr4-4.13.3-SNAPSHOT-complete.jar` from the antlr4rust 0.5.0 release.
  3. Run `java -jar antlr4-4.13.3-SNAPSHOT-complete.jar -Dlanguage=Rust GQL.g4 -o antlr-exp/bench/src/gen`.
- **opengql/grammar material.** Fourteen files in `antlr-exp/corpus/` are unmodified copies of the `samples/` directory of opengql/grammar (<https://github.com/opengql/grammar>, commit `16ea71b`, Apache-2.0; licence in [antlr-exp/corpus/LICENSE-opengql-grammar](antlr-exp/corpus/LICENSE-opengql-grammar)). They are kept as test artefacts under the exception in the project's `CLAUDE.md` and NFR-VER-002. The files `q01`–`q06` and `zz_invalid.gql` were written for this project.

## Not kept here

| Item | Reason | Cited instead as |
|---|---|---|
| Papers (HyPer MVCC, Wu et al., Kuzu, Gupta et al., LiveGraph, Deutsch et al. GPML) and their text extractions | Third-party works | Bibliographic references in [research/research-02-layout-mvcc.md](research/research-02-layout-mvcc.md) §1 and [research/research-03-query-engine.md](research/research-03-query-engine.md) §1 |
| Copies of `RELEASES.md` (rust-lang), Neo4j documentation pages and crates.io API responses | Third-party content | URLs in the [technical specification sources](../technical-spec/README.md#sources) |
| Copy of `GQL.g4`, the generated parser, the ANTLR tool jar | Available upstream and reproducible | The commands above |
| `target/` directories, built binaries, benchmark page files (about 3.9 GiB) | Build and run outputs | The hashes and commands above |
| Smoke runs (`smoke/`, `smoke2/`, `smoke-*.txt`) and the earlier interrupted layout run | Superseded by M-LAYOUT and not cited | — |
| `bsec.rs`, `user-decisions.md` | Not cited; the user decisions are recorded in the functional specification | — |
