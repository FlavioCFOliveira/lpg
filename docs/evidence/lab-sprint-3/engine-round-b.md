# Sprint 3, round B: engine evidence (rmp #42 to #46)

Coverage, memory and fuzzing records of the query engine: expressions, binder, planner, executor with PROFILE, catalog statements and closed graph types. Measured on 2026-10-10 on the working tree based on commit `92c0529`. No timing figure here is a performance claim (Sprint 3 validates correctness only).

## Coverage per function

Recipe of [`report.md`](report.md) "Coverage": `cargo llvm-cov nextest -p lpg --json --output-path cov.json -E 'test(/^query::/)'` (74 tests), then `python3 -I docs/evidence/lab-sprint-3/cov-functions.py cov.json src/query/`, without the lexer, parser, AST and test files. Table: [raw/coverage-query-engine.tsv](raw/coverage-query-engine.tsv) (784 functions and closures; 342 named functions).

**14 named functions are never executed**, each for a stated reason:

| Function | Reason |
|---|---|
| `NoEnv::{param, property, tokens, exists, principal, token}` | The environment of constant folding: a constant expression reads no parameter, property, label, subquery, session or late token, so these are unreachable by construction. |
| `expr::internal` | Defensive internal error (50N00) of states the engine never produces. |
| `Cursor as Debug` | Debug formatting; no test prints a cursor. |
| `Argument::held`, `Catalog::held` | The trait's default (0); PROFILE reads `held` of the operators it wraps, and no profiled test plan contains these two. |
| `Apply::reset`, `Project::reset`, `Write::reset`, `Catalog::reset` | `reset` runs on the right side of an `Apply` and on `EXISTS` subplans; no plan puts these operators there. |

Unexecuted closures (442 entries in the table) are the error-mapping and error-position closures of paths that succeed in the tests.

### Function kernels (rmp #42)

Every kernel of the function table and every accumulator method is executed: `string_kernel` 13, `cut_kernel` 5, `abs_kernel` 3, `now_kernel` 8, `temporal_kernel` 21, `between_kernel` 4, `temporal_arith` 6, `shift` 3, `parse_duration` 6, `temporal_record` 5, `duration_record` 1; `Accumulator::{new, update, add, finish, held, wrong}` (counts in the table).

### Binder functions (rmp #43)

Every named function of `src/query/binder/` is executed (none in the list above).

### Planner rules (rmp #44)

Every function of `src/query/plan/` is executed: access choice (`Planner::access`, 5 312 calls; index seeks by equality, range and point, label scan, all-nodes scan), predicate placement (`Planner::pattern`), estimates (`Estimates::{nodes, edges, all_nodes}`, `node_estimate`, `edge_probability`), join order (`join::exact` 4 703, `join::greedy` 2, `order`), projection and aggregation lowering (`Planner::project`), EXPLAIN (`describe`, `op_name`).

### Operators (rmp #45)

`next_batch` of every operator is executed: `Scan` 15 869, `Expand` 13 936, `Filter` 23 806, `Apply` 4 095, `Argument` 7 090, `Project` 8 084, `Aggregate` 2 637, `Distinct` 1 683, `Sort` 1 760, `Write` 69, `Catalog` 20, the PROFILE decorator 6 025, `Cursor::next_batch` 10 737.

### Catalog statements and closed-type check (rmp #46)

Every function of `src/query/graph_type.rs`, `src/query/binder/catalog.rs` and the catalog and check functions of `src/query/exec/write.rs` is executed (`run_catalog` 20, `check_closed` 8, `GraphType::{encode, decode, node_type, edge_matches}`), except `Write::reset` and `Catalog::reset` (above).

## Blocking operators: peak heap and peak reservation (rmp #45)

Recipe: [`heaptrack-tests.sh`](heaptrack-tests.sh) over the test binary with the filter `query::exec::tests::memory::`. Each test profiles one blocking operator over the same 10 000 rows (the product of 100 nodes with themselves) and prints the operator's peak reservation from PROFILE. Raw: [raw/heaptrack-blocking-operators.tsv](raw/heaptrack-blocking-operators.tsv).

| Operator | Peak reservation (bytes) | Test peak heap |
|---|---:|---:|
| `Sort` | 1 703 936 | 3.32M |
| `EagerAggregation` | 2 097 152 | 4.60M |
| `Distinct` | 786 432 | 2.66M |
| `Create` (`INSERT`) | 1 310 720 | 20.80M |

The peak heap is the whole test process: harness, database and the transaction's write set (the `INSERT` test writes 10 000 edges). Reservations are multiples of the 128 KiB chunk (#39 point 8).

## Binder fuzzing (rmp #43)

Target `binder` (`fuzz/fuzz_targets/binder.rs`, `lpg::fuzz::binder`): parse, then bind every accepted program in a fresh transaction of one database under `/tmp/lpgram/fuzz-binder` (deleted at the end). Corpus seeded with the parser corpus (13 640 files from the #36 generator and fuzzing). Result: [raw/fuzz-binder.txt](raw/fuzz-binder.txt).
