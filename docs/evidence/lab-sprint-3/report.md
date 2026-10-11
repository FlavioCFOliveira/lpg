---
title: Sprint 3 correctness laboratory
status: living record
last-reviewed: 2026-10-11
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

## Round D: conformance and exit verification (rmp #52, #53)

Run on 2026-10-11 on commit `44c2dcb` plus the round-D working tree. The last code change, the per-process database directory of the binder fuzz target in `src/fuzz.rs` (for fork mode), came after the test runs; `src/fuzz.rs` is compiled only under `cfg(fuzzing)`, so no test build contains it, and the `--cfg fuzzing` clippy and `cargo fmt --check` were rerun on it (clean). Correctness only (sprint UPDATE 55): the seconds printed by nextest are run durations, not performance claims. Graph data of the default suite in `/tmp/lpgram` (tmpfs: no durability or I/O claim rests on it); the flush-dependent suites on ext4 (`LPG_LAB_EXT4=/data/tmp/lpg-lab`, NVMe).

### Conformance runner

`src/tck/` runs the scenarios of [`tests/tck/`](../../../tests/tck/README.md) through `Session`, one new database each, every statement under PROFILE. The selection is not the engine's: [`tck_manifest.py`](tck_manifest.py) applies the TS-30 rule (the ANTLR parse of `GQL.g4` and the round-S anchors) and writes `tests/tck/selection.tsv` ([raw](raw/tck-manifest.txt)). The gql-compat cases are translated by [`gqlcompat_translate.py`](gqlcompat_translate.py) ([raw](raw/gqlcompat-translate.txt)). One line per scenario with its verdict and reason: [raw/tck-results.tsv](raw/tck-results.tsv); totals: [raw/tck-summary.txt](raw/tck-summary.txt).

**Counts per artefact.** Whole artefacts as the TS-30 rule counts them (reproduces TS-30): opengql/tck 206 instances, 45 selected; openCypher TCK 3 897, 200 selected; gql-compat 107 mandatory cases, 106 selected, and 69 condition cases, 24 selected.

Counts after the user's decisions on the disputes (DECISION 386 on rmp #52), rerun on 2026-10-11 on `aa25a75` plus the working tree ([raw](raw/tck-summary.txt)):

| Artefact | Selected | Passed | Gate (code) | Not GQL | Recorded divergence | Not translated |
|---|---|---|---|---|---|---|
| opengql/tck @ `e0700a7` | 45 | 40 | 0 | 0 | 5 | – |
| openCypher TCK @ `677cbaf` | 200 | 184 | 10 (GB02 8, GF20 2) | 6 | 0 | – |
| gql-compat @ `9de1991` (translated) | 130 | 121 | 1 (GQ14) | 0 | 2 | 6 |
| lpg's own (`approved-features.feature`) | 14 | 14 | 0 | 0 | 0 | – |

* **Gate**: an engine gate whose code has no `GQL.g4` construct, so the TS-30 rule cannot see it; the runner checks 42001 with the cause 51N26 naming the code. GB02: `(a)--(b)` is a `--` comment in GQL; GF20: an aggregate in a sort key; GQ14: a sort key that is not a result column (gql-compat places it in the mandatory surface; TS-20 reads GQ14 more strictly; unverified, needs the ISO text).
* **Not GQL**: duration strings with weeks, fractions outside seconds or mixed year-month and day-time parts (ISO BNF insert, lines 2639-2670), and a duration record with years and days (no GV41 type holds it); the runner checks 22G0H or 22G07.
* **Decided, now passing** (DECISION 386 on rmp #52; [raw](raw/mutations-disputes.txt): each fix reverted made its scenarios fail): opengql/tck catalog failures (7): a catalog-modifying statement that fails with 42N11, 42002 or G1000 reports 42000 with that error as its cause; openCypher `Call1.feature` [12]: a `CALL` in a larger statement without `YIELD` fails with 42002 and the cause 42N25, as Neo4j requires (revises DECISION 360 on rmp #50); gql-compat redeclared variable: 42000 with the cause 22N27, which the translated case now expects (its comment states the change).
* **Recorded divergence** (kept by the user, DECISION 386 on rmp #52; pinned, never counted as passed): opengql/tck needs `CREATE SCHEMA` to create directories, which FR-CAT-005 forbids (4; the setup fails with 42000, cause 42002); `Aggregation3.feature` [1] is defective upstream (1); gql-compat `SESSION SET SCHEMA /` (the root is a directory, TS-17) and `1 < 'a'` (NULL, IV002/IV010, user decision 2026-10-07) (2).
* **Not translated**: 6 gql-compat cases whose condition cannot arise in lpg (`unless GP18`, `unless GT03`, `limit IL001` for nodes, `limit IW014`, 2 `fault cut-after-send`).
* **Unselected** instances of the vendored files, by excluding code (one instance may need several): opengql/tck GV50 47, GV45 32, GL04 17, GQ10 8, GQ23 8, GQ08 4, GG05 3, GC02 2, GG04 1, GQ20 1, GT02 1, rejected by `GQL.g4` 6; openCypher rejected by `GQL.g4` 652, G043 100, GV50 50, GL04 50, GV45 48, G004 38, GE07 4.

| Check (rmp #52 AC) | Test or evidence | Result |
|---|---|---|
| Every selected scenario runs; each names its clause or feature code | `tck::{opengql_tck,opencypher_tck,lpg}_*_pass`; `tck::every_scenario_names_its_clause_or_feature_code` (feature-code tag, `@clause-N.N`, or `tests/tck/clauses.tsv`, each a `GQL.g4` subclause heading); `tck::the_selection_manifest_matches_the_vendored_files` | pass; 0 failed |
| Published feature list equals the gate table | `query::features::tests::published_feature_list_matches_the_gate_table` → [`docs/supported-features.md`](../../supported-features.md) | pass |
| Oracle differential on every scenario text | `query::parser::differential::scenario_texts_match_the_oracle_labels`, `stored_corpora_match_the_oracle_labels`; labels from `gql-oracle label` (745 ACCEPT, 641 REJECT) | 1 386 scenario texts, 0 differences |
| A wrong expected row fails the runner | `Literals2.feature` [1] edited `1` → `2`: `opencypher_tck_selected_scenarios_pass` failed ("rows [[Int(1)]], expected [[Int(2)]]"); file restored, `cmp` identical to upstream, test passes ([raw](raw/tck-mutation.txt)); permanent check `tck::a_wrong_expected_row_fails_the_runner` | pass |

**Engine defects found and fixed** (regression tests in `query::binder::tests` and `query::exec::tests::conformance`; each fix reverted at once made its tests and scenarios fail, [raw](raw/mutations-round-d.txt)): a static type error raised 22G03 instead of 42000, and an element reference was accepted as a boolean condition; an edge variable of two edge patterns of one graph pattern bound; a non-boolean search condition dropped rows; a double assignment in one `SET` passed (22G0M); a date shifted by hours, or a time by months, passed (22G14); a `SET` on a NULL element counted a property; ISO 8601 week, ordinal, basic and reduced-precision dates were rejected; and, found by the binder fuzz target, the new date parser panicked on a multi-byte character (`DATE '1\u{19}\u{fffd}-9'`).

### Micro-workloads (NFR-PERF-002)

Defined in `src/lab/micro.rs` and checked in the default suite (`lab::micro::*`, 2 tests, pass). Timing is Sprint 6. PROFILE counters, summed over the operators (peak memory: the largest operator's reservation, bytes), one run each ([raw](raw/micro-workloads.tsv)); buffer hits and misses depend on the pool state of that run.

| Id | GQL text (parameters) | Expected | Rows | Batches | Entries read | Hits | Misses | Peak memory |
|---|---|---|---|---|---|---|---|---|
| L1 | `MATCH (a:N {id: $s})-[]->(b) RETURN COUNT(*) AS c` (hub, $s = 47) | 95 | 1 | 6 | 697 | 405 | 0 | 131 072 |
| L2 | `MATCH (a:N {id: $s})-[:T2]->(b) RETURN COUNT(*) AS c` (47) | 27 | 1 | 6 | 629 | 405 | 0 | 131 072 |
| L3 | `MATCH (a:N {id: $s})-[]->()-[]->(c) RETURN COUNT(DISTINCT c) AS c` (47) | 140 | 1 | 10 | 1 933 | 595 | 0 | 131 072 |
| L4 | `MATCH (a:N {id: $s})-[]->()-[]->()-[]->(c) RETURN COUNT(DISTINCT c) AS c` (47) | 180 | 1 | 94 | 30 049 | 3 067 | 0 | 131 072 |
| L1-low | L1 ($s = 0, out-degree 1) | 1 | 1 | 6 | 603 | 405 | 0 | 131 072 |
| L2-low | L2 (0) | 0 | 1 | 5 | 602 | 405 | 0 | 0 |
| L3-low | L3 (0) | 7 | 1 | 8 | 611 | 407 | 0 | 131 072 |
| L4-low | L4 (0) | 84 | 1 | 11 | 859 | 423 | 0 | 131 072 |
| L5 | `MATCH (a:N {id: $s}), (b:N {id: $t}) INSERT (a)-[:T0]->(b)` (0, 47) | 1 edge created; L1 from 0 then 2 | 0 | 9 | 1 207 | 821 | 0 | 131 072 |
| IS1 | profile of a person (`personId` 3) | 1 row: Cy, Ng, 1985-07-07, … | 1 | 7 | 46 | 32 | 0 | 0 |
| IS2 | recent posts of a person (1), `ORDER BY messageCreationDate DESC, messageId ASC` | posts 101, 100 | 2 | 8 | 55 | 40 | 0 | 131 072 |
| IS3 | friends of a person (4), `ORDER BY friendshipCreationDate DESC, personId ASC` | 2 (220), 3 (205) | 2 | 8 | 49 | 34 | 0 | 131 072 |
| IS4 | content of a message (101) | 310, `photo.jpg` | 1 | 5 | 40 | 26 | 0 | 0 |
| IS5 | creator of a message (202) | 3, Cy, Ng | 1 | 7 | 43 | 29 | 0 | 0 |
| IS6 | forum of a post (102) | 51, Wall of Bo, moderator 2 | 1 | 10 | 44 | 30 | 0 | 0 |
| IS7 | replies of a message (100), knows-flag | 201 (Di, false), 200 (Bo, true) | 2 | 20 | 66 | 49 | 0 | 131 072 |

The full GQL texts of IS1–IS7 are in `src/lab/micro.rs` and in the raw file. Layout graph: M-LAYOUT generator, Zipf α = 0.8, 200 nodes, 1 000 edges, four types, seed 20261011; expected values from a reference evaluation of the edge list (trail ends under `DIFFERENT EDGES`, ID086). SNB graph: 4 persons, 3 cities, 6 messages, 2 forums; expected rows from a reference evaluation of its data. Adaptations forced by gates: right-pointing edges only (G043), no `LIMIT` (GQ13), no `REPLY_OF*0..` (G035).

### Full run and gates (rmp #53)

| Run or gate | Command | Result | Raw |
|---|---|---|---|
| Gates after the dispute decisions (DECISION 386 on rmp #52) | `LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs .config/gates.sh` (XFS mounted: `/dev/loop59`) | exit 0, 0 failed: fmt, clippy, `--cfg fuzzing` clippy clean; nextest 510 passed, 28 skipped; loom 13; shuttle 4; stress 69 passed | [raw](raw/gates-disputes.txt) |
| Full suite, ext4 lab directory | `LPG_LAB_EXT4=/data/tmp/lpg-lab cargo nextest run -p lpg --no-fail-fast` | exit 0; 508 passed (2 slow), 28 skipped | [log](raw/nextest-full-round-d.log) |
| XFS lab directory | `LPG_LAB_XFS=/mnt/lpg-xfs` | **not run**: `/mnt/lpg-xfs` is not mounted (root needed) | – |
| Stress profile | `LPG_LAB_EXT4=/data/tmp/lpg-lab cargo nextest run -p lpg --profile stress --no-fail-fast` | exit 0; 69 passed, 16 copies each | [log](raw/stress-round-d.log) |
| loom | `RUSTFLAGS="--cfg loom" cargo test -p lpg --lib --release --target-dir /dev/shm/lpg-target/loom loom_` | exit 0; 13 passed | [log](raw/loom-round-d.log) |
| shuttle | `RUSTFLAGS="--cfg shuttle" cargo test -p lpg --lib --target-dir /dev/shm/lpg-target/shuttle shuttle_` | exit 0; 4 passed | [log](raw/shuttle-round-d.log) |
| Differential, generated and stored | `cargo nextest run -p lpg --run-ignored all -E 'test(/differential::/)' --no-capture` | exit 0; seeds 20261008, 1, 12648430: 100 000 sentences and 100 000 mutants each, 0 differences; samples 21, miniGU 142, test texts 70, scenario texts 1 386: 0 differences | [raw](raw/differential-round-d.txt) |
| MSRV | `cargo +1.89 build` | exit 0 | [raw](raw/cargo-1.89-build-round-d.txt) |
| Licences | `cargo deny check licenses` | exit 0, "licenses ok" | [raw](raw/cargo-deny-licenses-round-d.txt) |
| Lints | `cargo fmt --all -- --check`; `cargo clippy --all-targets -- -D warnings`; the same clippy of the library with `--cfg fuzzing` | exit 0 each | [raw](raw/gates-round-d.txt) |
| `forbid(unsafe_code)` | `grep -rn 'forbid(unsafe_code)' src/lib.rs` | `src/lib.rs:9` | [raw](raw/gates-round-d.txt) |
| FR-STO-002 grep | `grep -rnE 'std::fs\|OpenOptions\|File::(open\|create)' src --include=*.rs` | product code matches only `src/storage/fs.rs`; every other match is test code (`src/tck/`, `src/lab/`, `*/tests.rs`, `differential.rs`, `suites/`, `test_support/`), and, after the fork-mode change, `src/fuzz.rs` (compiled only under `cfg(fuzzing)`) | [raw](raw/gates-round-d.txt) |
| NFR-PERF-007 over every scenario | the operator counter of #45, read for each statement the runner profiles | 428 statements profiled to completion (26 + 136 + 266; `Call1.feature` [12] now fails when compiled), at most 1 operator running at once | [raw](raw/tck-summary.txt) |
| Coverage, query engine | `cargo llvm-cov nextest -p lpg --json -E 'test(/^(query\|tck\|lab::micro)::/)'`, `cov-functions.py`, `cov-rules.py` | executor (`src/query/exec/`): 288 functions and closures, 22 never executed (`reset` of 5 operators, `Debug`, `run_nested`, closures); rule functions without the generated sentences: 151 of 155 executed. The parser is unchanged in round D, so round A's 155 of 155 with the generated sentences stands | [raw](raw/coverage-query-round-d.tsv) |
| Peak heap per blocking operator | round B, not rerun (the operators are unchanged in round D) | see [engine-round-b.md](engine-round-b.md) | [raw](raw/heaptrack-blocking-operators.tsv) |
| Fuzz, lexer | `cargo +nightly fuzz run --target-dir /dev/shm/lpg-target/fuzz-final lexer fuzz/corpus/lexer -- -runs=10000000` | exit 0, "Done 10000000 runs in 876 second(s)", no artifact | [raw](raw/fuzz-lexer-final.txt) |
| Fuzz, parser | the same with `parser`, `-fork=4 -ignore_timeouts=0 -ignore_ooms=0` (libFuzzer fork mode, coordinator's request) | exit 0, "fuzzed for 10111794 iterations", `oom/timeout/crash: 0/0/0` in every stats line, no artifact; wall clock 3 575 s | [raw](raw/fuzz-parser-final.txt) |
| Fuzz, binder | the same with `binder` on a copy of `fuzz/corpus/binder`, `-fork=10 -ignore_timeouts=0 -ignore_ooms=0`, `LPG_FUZZ_DIR=/tmp/lpgram/fuzz-binder-final` (one database per child process, dead children's directories removed every 3 s, the directory removed at the end) | exit 0, "fuzzed for 10038368 iterations", crash 0, no artifact; wall clock 2 279 s. Its first run (single process) crashed on a seed input in the new date parser, fixed before this run (above) | [raw](raw/fuzz-binder-final.txt) |
| Fuzz, record_decode | `cargo +nightly fuzz run --target-dir /dev/shm/lpg-target/fuzz-final record_decode fuzz/corpus/record_decode -- -runs=1000000` | exit 0, "Done 1000000 runs in 10 second(s)", no artifact | [raw](raw/fuzz-record_decode-final.txt) |

### Exit criteria of sprint DECISION 12 (timing excluded)

| Criterion | Passing tests | State |
|---|---|---|
| The parser accepts and rejects as the ANTLR oracle on the samples, every GQL text of the tests and the generated sentences | `query::parser::differential::*` (above), now with the 1 386 scenario texts | met |
| Every scenario of the approved feature set passes and names its clause or feature code | `tck::*` | **met except 7 recorded divergences**: 359 pass, 11 gate and 6 not-GQL exclusions are verified; the 7 divergences the user kept (DECISION 386 on rmp #52) stay pinned and do not pass |
| FR-API-001..010 (Rust API part), FR-SES-001..004, FR-AUTH-001/002, FR-PROC-001..004, FR-LIM-001/002/006, FR-OBS-002..006 through the Rust API | `api::tests::{api,auth,limits,procs,obs}::*` (rmp #47–#51 TEST entries), in the full run | met |
| FR-CAT-001, FR-IDX-001, FR-TYPE-005, FR-LIM-003/005/007, FR-GQL-004, FR-OPS-001/002/008/010, FR-REC-004/010 through a statement, a procedure or the API | `api::tests::procs::the_show_procedures_list_their_objects_with_the_specified_columns`, `…point_searches_return_the_same_rows_with_and_without_the_index`, `…no_built_in_backs_up_restores_or_creates_a_directory`; `api::tests::auth::every_index_and_constraint_kind_is_managed_through_the_api`, `…a_restored_backup_holds_graphs_index_and_constraint`; `api::tests::limits::{a_statement_over_the_memory_limit_fails_and_earlier_writes_vanish, writes_and_old_versions_at_the_cap_fail_with_51n72}`; `api::tests::api::backup_restore_integrity_check_and_migration_through_the_api`; scenarios `Create2.feature` [1]–[3], `approved-features.feature` [13], [14] (FR-GQL-004) | met; the FR-OPS-001 load half cannot run through the API before Sprint 4 (FINDING 353 on rmp #47) |
| A query uses at most one thread at a time (NFR-PERF-007) | `query::exec::tests::…across_a_thousand_batches_at_most_one_operator_runs_at_once`; the runner's counter over every scenario statement | met |
| Micro-workloads defined (and measured); point 8 closed; point 1 restated | `lab::micro::*`; `quality-and-testing.md` unverified points 1 and 8 | defined and checked; point 1 restated; measurement and point 8 deferred to Sprint 6 |

### Deferred to Sprint 6 (timing, sprint UPDATE 55)

* Timing of the micro-workloads above and the LDBC SNB baseline against Neo4j and Memgraph (NFR-PERF-002), and unverified point 8 (parse speed).
* The laboratory timing items of sprint DECISION 13: flame graphs per phase, parse time against M-PARSE, `spawn_blocking` hand-off cost against M-TOKIO, tokio runtime metrics, cancellation latency, and PROFILE's time per batch.
* The parser fuzz throughput drop (FINDING 317 on rmp #41).
