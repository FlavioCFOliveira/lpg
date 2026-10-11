# Sprint 3, round C: API evidence (rmp #47 to #51)

Coverage, mutation, concurrency and gate records of the public async API, authorisation, cancellation and limits, the procedure registry and built-ins, and the metrics and events. Measured on 2026-10-11 on the working tree based on commit `46764d5`. No timing figure here is a performance claim (Sprint 3 validates correctness only).

## Coverage per function

Recipe of [`report.md`](report.md) "Coverage per function": `cargo llvm-cov nextest -p lpg --no-fail-fast --json --output-path cov.json` (whole suite, 493 tests), then `python3 -I docs/evidence/lab-sprint-3/cov-functions.py cov.json <path>` for `src/api/`, `src/auth/` and `src/proc/`, test files removed. Table: [raw/coverage-api-round-c.tsv](raw/coverage-api-round-c.tsv) (365 functions and closures; 171 named functions, **160 executed**).

| File | Functions and closures executed | Code regions covered |
|---|---:|---:|
| `src/api/database.rs` | 72 / 77 | 655 / 721 (90.8 %) |
| `src/api/mod.rs` | 61 / 82 | 223 / 267 (83.5 %) |
| `src/api/session.rs` (sessions, transactions, management API) | 91 / 96 | 965 / 1 099 (87.8 %) |
| `src/api/status.rs` | 9 / 9 | 58 / 59 (98.3 %) |
| `src/api/stream.rs` | 22 / 23 | 330 / 371 (88.9 %) |
| `src/auth/mod.rs` | 11 / 15 | 204 / 261 (78.2 %) |
| `src/proc/builtin.rs` (built-ins) | 35 / 38 | 570 / 617 (92.4 %) |
| `src/proc/mod.rs` (registry, contexts) | 22 / 25 | 166 / 188 (88.3 %) |

**11 named functions are never executed**, each for a stated reason:

| Function | Reason |
|---|---|
| `Database`, `Options`, `Session`, `Transaction`, `ResultStream` as `Debug`; `Procs` as `Debug` | Debug formatting; no test prints these handles. |
| `api::internal` | 50N00 for a panicked blocking closure or an authorisation request outside a runtime; neither happens in the tests. |
| `Authoriser::authenticate` (default), `Authoriser::impersonate` (default, for `TestAuth`) | Called only by the Bolt endpoints (Sprint 6); the Rust API does not authenticate (FR-SES-001). |
| `Procs as PartialEq`, `RoutineRef as PartialEq` | Needed by the derived comparisons of `SessionCtx` and the bound form; no code compares two sessions or two bound calls. |

The unexecuted closures (`src/api/mod.rs` holds 21 of them) are the `map_err` closures of `spawn_blocking` join errors (a panicking statement thread) and the error-mapping closures of calls that succeed in the tests.

## Mutation checks

Each control was disabled in the source, the tests that guard it were run, and the source was restored ([raw/mutations-api-round-c.txt](raw/mutations-api-round-c.txt)). Every mutation is detected:

| Mutation | Tests | Result |
|---|---|---|
| M1 the plan's requests are not asked (`open_in`) | `api::tests::auth` | 3 of 7 fail |
| M2 the status check is not installed | `api::tests::limits` | 3 of 8 fail |
| M3 the row and byte limits are not checked | `row_and_byte_limits…` | fails |
| M4 the transaction memory limit is not checked | `a_statement_over_the_memory_limit…` | fails |
| M5 a procedure's returned rows are not charged | `a_routines_own_memory…` | fails |
| M6 the point searches ignore the point index | `the_point_searches…` | fails |
| M7 a read-write routine is allowed in a query | `read_write_procedures…` | fails |
| M8 nested GQL of a read-only routine may write | `read_write_procedures…` | fails |
| M9 the commit ignores a stopped status | `a_terminated_transactions…` | fails |

The `loom` model of a cancel racing a commit has its own mutation: `loom_the_model_fails_when_the_commit_ignores_the_status` passes only if the model fails when the commit stores `COMMITTING` without the compare-and-swap.

## Concurrency

* `loom`: `RUSTFLAGS="--cfg loom" cargo test -p lpg --lib --release --target-dir target/loom loom_` → **13 passed** (11 earlier models and the 2 of `src/api/status.rs`) ([raw/loom-api-round-c.log](raw/loom-api-round-c.log)).
* Stress profile: the 6 new tests that run concurrent tasks against one database were added to `.config/nextest.toml`; `cargo nextest run -p lpg --profile stress --no-fail-fast api::` → **6 tests run: 6 passed**, 16 copies each, 0 copies failed ([raw/stress-api-round-c.log](raw/stress-api-round-c.log)).
* Concurrent steps are ordered by pause points: a test procedure signals on a channel once it runs, and the test acts only after receiving it; no test sleeps for ordering.

## Gates on `lpg`

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all -- --check` | exit 0 |
| Lints | `cargo clippy --all-targets -- -D warnings`; the same with `RUSTFLAGS="--cfg fuzzing"` (`--target-dir target/fuzzcfg`) | exit 0, exit 0 |
| Tests | `cargo nextest run -p lpg --no-fail-fast` | **493 passed**, 28 skipped (existing `#[ignore]` laboratory workloads) ([raw](raw/nextest-api-round-c.log)) |
| No Bolt (FR-API-003) | `cargo build -p lpg`, then `nm -C target/debug/liblpg.rlib \| grep -ci bolt` | build exit 0; **0** symbols |
| Dependency tree | `cargo tree -p lpg -e normal --depth 1` | `crc32c`, `jiff`, `rand`, `rustix`, `tokio` 1.53.2, `tracing` 0.1.44; new transitive crates: `pin-project-lite`, `tracing-core`, `once_cell` |
| Licences | `cargo deny check licenses` | "licenses ok" |
| MSRV | `cargo +1.89 build -p lpg` and `--tests` | exit 0 |

All gate outputs: [raw/gates-api-round-c.txt](raw/gates-api-round-c.txt). After every run `/tmp/lpgram` held only the directory of the binder fuzz run in progress.
