---
title: Sprint 3 correctness laboratory
status: living record
last-reviewed: 2026-10-08
related:
  - ../README.md
  - ../lab-sprint-2/report.md
  - ../planning/sprint-3-plan.md
---

# Sprint 3 correctness laboratory

Record of the instruments set up for Sprint 3, GQL engine and async Rust API (rmp #36): the toolchain, the ANTLR parser oracle, the sentence generator, fuzzing, coverage per function and peak heap per test. Run on 2026-10-08 at commit `7bce61f` plus the working-tree changes of #36. Sprint 3 verifies correctness only (sprint UPDATE 55): no figure here is a timing or performance claim.

## Environment

| Item | Value | Source |
|---|---|---|
| CPU | AMD Ryzen 9 5900HX, 16 logical CPUs, 2 threads per core | `lscpu` |
| Memory | 30 GiB | `free -g` |
| Operating system | Ubuntu 26.04.1 LTS, kernel 7.0.0-38-generic | `/etc/os-release`, `uname -r` |
| `/tmp/lpgram` | `tmpfs` (`rw,nosuid,nodev,nr_inodes=1048576,inode64,usrquota`). Used only by `cargo nextest run -p lpg` below. It is not a production scenario, and no durability or I/O claim rests on it (CLAUDE.md). | `findmnt -T /tmp` |

## Tools

All versions were read on 2026-10-08. The crates.io versions come from `https://crates.io/api/v1/crates/<name>` (`max_stable_version`), queried with the User-Agent `lpg-lab (https://github.com/FlavioCFOliveira/lpg)`. No cargo tool needed an update: each installed version already equals crates.io's.

| Tool | Version | crates.io `max_stable_version` | Command |
|---|---|---|---|
| rustup | 1.29.1 (d95a37b6a 2026-08-13) | — | `rustup self update` ("unchanged"), `rustup --version` |
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | — | `rustup update stable nightly`, `rustc --version` |
| Rust nightly | 1.101.0-nightly (1d81eb4ad 2026-10-07) | — | `rustc +nightly --version` |
| Rust MSRV | 1.89.0 (29483883e 2025-08-04) | — | `rustc +1.89 --version` |
| `rustup check` | every toolchain and rustup "up to date" ([raw](raw/rustup-check.txt)) | — | `rustup check` |
| cargo-deny | 0.20.2 | 0.20.2 | `cargo deny --version` |
| cargo-llvm-cov | 0.9.1 | 0.9.1 | `cargo llvm-cov --version` |
| cargo-nextest | 0.9.148 (cd1d6d546 2026-10-07) | 0.9.148 | `cargo nextest --version` |
| cargo-fuzz | 0.13.2 | 0.13.2 | `cargo fuzz --version` |
| inferno | 0.12.8 | 0.12.8 | `cargo install --list` |
| Java | OpenJDK 25.0.4.1 (build 25.0.4.1+1-1-26.04.4-Ubuntu) | — | `java -version` |
| heaptrack | 1.5.0, with `heaptrack_print` | — | `heaptrack --version` |
| ANTLR tool jar | `antlr4-4.13.3-SNAPSHOT-complete.jar`, asset of antlr4rust release v0.6.0 (2026-09-27), SHA-256 `391e77311794f7c8fed2bc6d291f54b04725e37456d79e1981edfd3fc919b677`, equal to the digest the GitHub API reports for the asset | — | `sha256sum`; `api.github.com/repos/antlr4rust/antlr4/releases/tags/v0.6.0` |
| antlr4rust | 0.6.0 (BSD-3-Clause), oracle crate only | 0.6.0 | `tools/gql-oracle/Cargo.lock` |
| libfuzzer-sys | 0.4.13, fuzz crate only | 0.4.13 | `fuzz/Cargo.lock` |
| GNU c++filt | 2.46 (demangling for the coverage table) | — | `c++filt --version` |
| Python | 3.14.4 | — | `python3 --version` |

## Parser oracle

**Crate.** `tools/gql-oracle/`: own `[workspace]`, `publish = false`, not a member or dependency of `lpg`. `grammar/GQL.g4` and `grammar/LICENSE` are unmodified copies of opengql/grammar @ `16ea71b` (SHA-256 in `grammar/SOURCE.md`). `generate.sh` checks the jar's SHA-256 (downloading it to `target/antlr/` when `ANTLR_JAR` is unset) and writes the parser to `src/gen/`, which is not checked in. Two generations gave byte-identical files: `gqllexer.rs` (`bdecf82b…`), `gqlparser.rs` (`e6327a7d…`) and `gqllistener.rs` (`f24a509b…`), 3.2 MB in all.

**API.** `gql_oracle::accepts(text) -> bool`; `first_error(text)` gives the first lexer or parser error. The parser keeps ANTLR's default LL prediction mode (antlr4rust 0.6.0 `src/parser_atn_simulator.rs:144`); the test `oracle_uses_ll_prediction` asserts it.

**Command.** `gql-oracle label [DIR]` prints `ACCEPT` or `REJECT` with the first error for each file of `DIR`, or for each line of stdin.

**Result.** `gql-oracle label docs/evidence/antlr-exp/corpus` accepts the 14 opengql/grammar samples and the six `q0*` inputs, and rejects `zz_invalid.gql` ("no viable alternative at input 'MATCH (n:Person WHERE n.id = $id RETURN'") ([raw](raw/oracle-corpus.txt)). This settles the plan's unverified point that antlr4rust 0.6.0 and its jar generate a working parser from `GQL.g4`.

## Sentence generator

In the same crate: `g4` reads the `GQL.g4` constructs (1 018 rules: 574 parser rules, 390 tokens, 54 fragments); `generate` walks them.

* **Walk.** A token budget is drawn per sentence; a choice (alternative, option, repetition) is taken only if its minimum derivation cost fits the budget left after the minimum cost of the siblings that follow. Alternatives are weighted by `1 / (1 + times taken)`. Token texts are generated from the lexer rules, with random letter case where the rule is case-insensitive. The walk records the parser rules it expanded.
* **Directed phase.** While a reachable rule has not been expanded, the next sentence is steered at it along a minimum-cost path. Without it, 10 000 random sentences leave 72, 33 and 18 of the 571 reachable rules unexpanded at budgets of 32, 64 and 128 tokens (seed 20261008).
* **Shrinker.** Undoes choices (drops an option, trims a repetition, replaces a rule or alternative by its smallest derivation), so every candidate is a sentence of the grammar.
* **Mutants.** One token deleted, inserted (a literal token of the grammar) or swapped with its neighbour, from a separate random stream.
* **Seed.** `LPG_SEED` (decimal or `0x` hexadecimal); default 20261008.

**Commands and result** (seed 20261008, budget up to 64 tokens; [raw](raw/generator-check.txt)):

| Check | Command | Result |
|---|---|---|
| Rule coverage | `LPG_SEED=20261008 gql-oracle check 10000` | 10 000 sentences, 83 272 tokens (mean 8.3, maximum 64); 125 directed sentences; **571 of 571 parser rules reachable from `gqlProgram` expanded** |
| Determinism | `LPG_SEED=20261008 gql-oracle generate 10000 \| sha256sum`, twice | both `5a5fbab0…db525296` ([raw](raw/generator-sha256.txt)); `LPG_SEED=1` gives a different hash |
| Oracle on sentences | `… generate 10000 \| gql-oracle label` | **10 000 accepted, 0 rejected** ([raw](raw/generator-labels.txt)); no rejected class |
| Oracle on mutants | `… mutants 10000 \| gql-oracle label` | 210 accepted, 9 790 rejected ([raw](raw/mutants-labels.txt)) |

The 10 000 sentences (477 139 bytes) are not stored; the command and the hash reproduce them. The three parser rules not reachable from `gqlProgram` are `pathVariableReference`, `listValueConstructor` and `durationValueExpression` (round-s also records three).

**Limits of the generated sentences.** Tokens are separated by one space; comments and other white space are never generated. Characters of `\p{ID_Start}`, `\p{ID_Continue}`, `\p{Pc}` and `\p{Sc}` come from a fixed subset (ASCII letters, `é`, `λ`, digits, `_`, `$`, `€`); negated sets draw from printable ASCII and `é`, `λ`. A token text that an earlier literal-only token rule would take (for example an identifier spelled as a keyword) is regenerated; none kept colliding in this run.

**Tests** (`cargo nextest run` in `tools/gql-oracle/`): 8 passed, the longest 0.99 s. Removing the directed phase makes `generator_expands_every_reachable_rule_and_the_oracle_accepts_its_sentences` fail; the source was then restored.

## Fuzzing

`fuzz/` was created by `cargo fuzz init`, with its own `[workspace]`. `src/fuzz.rs` in `lpg` is compiled only under `cfg(fuzzing)` (declared in the `check-cfg` list of `Cargo.toml`). The smoke target `record_decode` decodes arbitrary bytes with `types::record::decode` and `decode_labels`.

| Command | Result |
|---|---|
| `cargo +nightly fuzz run record_decode -- -runs=1000000` | **exit 0**, "Done 1000000 runs", no crash; libFuzzer seed 927527442, 1 492 coverage edges, corpus of 1 030 inputs ([raw](raw/fuzz-record_decode.txt)) |

## Coverage per function

Recipe [`cov-functions.py`](cov-functions.py): `cargo llvm-cov nextest -p lpg --json --output-path cov.json [-E FILTER]`, then `python3 -I docs/evidence/lab-sprint-3/cov-functions.py cov.json [PATH]`. One TSV row per function: file, demangled name, execution count, covered and total code regions.

Exercised with `-E 'test(/^types::tests::/)'` (21 tests) and `PATH = src/types/record.rs`: 25 functions ([raw](raw/coverage-record-functions.tsv)); `decode_labels` is the only one these tests never execute.

## Peak heap per test

Recipe [`heaptrack-tests.sh`](heaptrack-tests.sh): `heaptrack-tests.sh <test-binary> [FILTER]` runs each matching test alone under `heaptrack --record-only` and prints the peak heap from `heaptrack_print`. The test binary path is the `Executable` line of `cargo test -p lpg --lib --no-run`.

Exercised on the 21 `types::tests::` tests ([raw](raw/heaptrack-types-tests.tsv)): peaks from 169.36K to 1.87M (`records_round_trip_after_commit_and_reopen`). The peak includes the test harness: the smallest tests all peak at about 169K.

## Gates on `lpg`

| Gate | Command | Result |
|---|---|---|
| Licences | `cargo deny check licenses` | **exit 0**, "licenses ok" ([raw](raw/cargo-deny-licenses.txt)) |
| MSRV build | `cargo +1.89 build` | **exit 0** ([raw](raw/cargo-1.89-build.txt)) |
| Dependency tree | `cargo tree -p lpg -e normal` | **neither `antlr4rust` nor `libfuzzer-sys`** ([raw](raw/cargo-tree-lpg-normal.txt)) |
| Package contents | `cargo package --list --allow-dirty` | no file under `tools/` or `fuzz/` |
| Tests | `cargo nextest run -p lpg` | **373 passed**, 24 skipped by the existing `#[ignore]` laboratory workloads and nextest filter ([raw](raw/nextest-lpg-summary.txt)) |
| Lints | `cargo fmt --all -- --check`; `cargo clippy --all-targets -- -D warnings`; the same clippy with `RUSTFLAGS="--cfg fuzzing"` on `lpg` and on `fuzz/`; `cargo clippy --all-targets -- -D warnings` in `tools/gql-oracle/` | all clean |

## Findings

* **The generated parser contains `unsafe`** (antlr4rust `cast_mut` in `gqlparser.rs`). The oracle crate denies `unsafe_code` and allows it only in the three generated modules. `lpg` is unaffected.
* **`cargo fmt` rewrites the generated files** through their `#[path]` modules; the modules carry `#[rustfmt::skip]`.
* **The generated files say "Generated … by ANTLR 4.13.2"** although the jar is named 4.13.3-SNAPSHOT; the jar's SHA-256 is the release asset's.
* **gql-compat states no copyright notice and has no NOTICE file**; its Apache-2.0 licence is kept as `tools/gql-oracle/LICENSE-gql-compat`.
* **A random walk alone does not cover the grammar** (see Sentence generator); the directed phase does.
* **No item is missing:** every tool of the task is installed at its latest version, and no root access was needed.

## Front end: errors, lexer and parser (rmp #40, #41)

Measured on 2026-10-09 on the working tree of round A (commit `25cae08` plus the uncommitted #40 and #41 changes). Correctness only.

| Check | Command | Result |
|---|---|---|
| Differential, generated | `cargo nextest run -p lpg --run-ignored all -E 'test(/differential::/)' --no-capture` | seeds 20261008, 1, 12648430: 100 000 sentences and 100 000 mutants each, **0 differences** ([raw](raw/differential-100k.txt)); `LPG_SEED=42` overrides the seeds ([raw](raw/differential-seed-override.txt)) |
| Differential, stored | same run | 21 samples, 142 miniGU inputs, 70 test texts: **0 differences** |
| Coverage per grammar rule | `cargo llvm-cov nextest … -E 'test(/differential::(stored_corpora\|differential_generated\|test_texts)/)'`, [`cov-rules.py`](cov-rules.py) | 155 rule functions, **0 never executed** ([table](parser-rule-coverage.md)) |
| Fuzz, lexer | `cargo +nightly fuzz run lexer -- -runs=10000000` | **exit 0**, "Done 10000000 runs in 704 second(s)", no crash, no artifact ([raw](raw/fuzz-lexer.txt)) |
| Fuzz, parser | `cargo +nightly fuzz run parser -- -runs=10000000` | **exit 0**, "Done 10000000 runs in 5438 second(s)", no crash, no artifact ([raw](raw/fuzz-parser.txt)) |
| Nesting | `deep_texts_fail_with_42001_on_a_2_mib_stack` | 28 shapes at 100 000 levels fail with 42001 on a 2 MiB stack; deepest accepted text per shape in [raw](raw/nesting-depth.txt) |
| Ambiguities | `GqlProbe.java` (round-s) on 600 000 generated texts | 34 ambiguous decisions ([raw](raw/ambiguities-generated.txt)); each compared with the ANTLR tree printed by [`TreeDump.java`](TreeDump.java); recorded in TS-19 |
| Error module coverage | `cargo llvm-cov nextest -p lpg -E 'test(/^(error\|gqlstatus\|storage::error)::/)' --summary-only` | [raw](raw/coverage-error-module.txt) |

The fuzz logs keep the libFuzzer header, the final status and the recommended dictionary; the `NEW` and `REDUCE` progress lines were dropped. The fuzz corpora under `fuzz/corpus/` were seeded with the three stored corpora and are not checked in.
