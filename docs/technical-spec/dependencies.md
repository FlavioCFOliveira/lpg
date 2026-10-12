---
title: Dependencies
status: draft
last-reviewed: 2026-10-12
related:
  - quality-and-testing.md
  - README.md
---

# Dependencies

## TS-31 Dependency set

- **Decision**: the crate depends on the crates in the table below and on nothing else. Each one serves a decision. Licences are checked by `cargo-deny` ([TS-30](quality-and-testing.md#ts-30-testing-and-conformance)).
- **Licence allow-list** for `cargo-deny` (user decision 2026-10-02): MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, BSL-1.0, CC0-1.0, Unicode-3.0. Copyleft licences are excluded. A crate whose licence expression offers alternatives passes when one of them is listed (D-DENY). `deny.toml` allows the listed licences that occur in the resolved tree: Apache-2.0, Apache-2.0 WITH LLVM-exception, MIT and, from Sprint 5, Zlib, which `foldhash` 0.1.5 needs (below).
- **Satisfies**: NFR-DEP-001, NFR-DEP-002, NFR-SIMP-001.

crates.io data as of 2026-10-02, as of 2026-10-07 for `jiff` and `shuttle`, and as of 2026-10-11 for `tokio`, `tracing` and `serde_json` (downloads: total / last 90 days).

| Crate | Version | Licence | Downloads | Last release | Kind | Used by |
|---|---|---|---|---|---|---|
| tokio | 1.53.2 | MIT | 1 051.0 M / 253.8 M | 2026-10-03 | runtime, features `rt` and `time`; test builds add `rt-multi-thread` (both runtime flavours) and, under `cfg(loom)` only, `sync`, without which tokio 1.53.2 does not compile there (`src/runtime/time/entry.rs:62`) | [TS-02](architecture.md#ts-02-async-api-and-threading), [TS-22](bolt.md#ts-22-bolt-and-packstream) |
| rustix | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 1 197.0 M / 275.0 M | 2026-09-16 | runtime, feature `fs` | [TS-03](interfaces.md#ts-03-storage-abstraction) (`fstatvfs`, `fallocate`, `renameat2`) |
| tracing | 0.1.44 | MIT | 920.3 M / 216.9 M | 2025-12-18 | runtime, feature `std` only (no `attributes` proc-macro crate); brings `tracing-core` 0.1.36, `once_cell`, `pin-project-lite` | [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events) |
| crc32c | 0.6.8 | Apache-2.0/MIT | 85.9 M / 16.5 M | 2024-06-09 | runtime | [TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check) |
| jiff | 0.2.38 | Unlicense OR MIT | 216.0 M / 79.1 M | 2026-10-06 | runtime | [TS-21](values-and-types.md#ts-21-values-and-types) |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | 533.5 M / 114.7 M | 2025-09-30 | runtime, default features (`std`, `graphmap`, `stable_graph`, `matrix_graph`; `Cargo.toml` `[features]`); not `rayon` | [TS-25](algorithms.md#ts-25-graph-algorithms) |
| nalgebra | 0.35.0 | Apache-2.0 | 92.7 M / 16.1 M | 2026-05-24 | runtime, `default-features = false, features = ["std"]`: the default feature `macros` brings `nalgebra-macros`, `proc-macro2`, `quote`, `syn` and `unicode-ident` ((MIT OR Apache-2.0) AND Unicode-3.0), and no decision uses its macros (`Cargo.toml` `[features]`) | [TS-25](algorithms.md#ts-25-graph-algorithms) |
| rand | 0.10.3 | MIT OR Apache-2.0 | 1 815.9 M / 475.0 M | 2026-09-20 | runtime, default features plus `chacha`, which alone exports `ChaCha8Rng` (`src/rngs/mod.rs:115-116`) | [TS-25](algorithms.md#ts-25-graph-algorithms), database UUID ([TS-05](storage.md#ts-05-files-and-pages)) |
| csv | 1.4.0 | Unlicense/MIT | 256.8 M / 52.4 M | 2025-10-17 | runtime | [TS-26](operations.md#ts-26-backup-restore-import-and-export) |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | 1 415.2 M / 354.6 M | 2026-10-11 | runtime, feature `float_roundtrip`, which "makes f64 -> JSON -> f64 produce output identical to the input" (`Cargo.toml.orig:67-74`); without it, floats are parsed by multiplying or dividing by powers of ten (`src/de.rs:623-660`). FR-IMP-006 needs the lossless round trip. No `serde` derive | [TS-26](operations.md#ts-26-backup-restore-import-and-export) |
| quick-xml | 0.42.0 | MIT | 444.2 M / 123.2 M | 2026-08-22 | runtime | [TS-26](operations.md#ts-26-backup-restore-import-and-export) |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | 853.1 M / 195.4 M | 2026-03-11 | dev | [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |
| loom | 0.7.2 | MIT | 68.6 M / 15.5 M | 2024-04-23 | dev | [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |
| shuttle | 0.9.5 | Apache-2.0 | 6.1 M / 1.8 M | 2026-10-01 | dev | [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |
| antlr4rust | 0.6.0 | BSD-3-Clause | 3.3 M / 2.6 M | 2026-09-27 | dev, test-only oracle crate, not a dependency of `lpg` | [TS-19](query-engine.md#ts-19-parser), [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |

**Transitive crates of `csv`, `serde_json` and `quick-xml`**, measured 2026-10-11 with `cargo tree -e normal` on a probe crate that depends on the three at the versions above (rmp #55 TEST). No `serde` crate is in the tree: `csv` and `serde_json` depend on `serde_core` (their `Cargo.toml`). `cargo deny check licenses` with lpg's `deny.toml` passes on the probe ([unverified point 11](quality-and-testing.md#unverified-points)).

| Crate | Version | Licence | Brought by |
|---|---|---|---|
| csv-core | 0.1.13 | Unlicense/MIT | csv |
| itoa | 1.0.18 | MIT OR Apache-2.0 | csv, serde_json |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | csv |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | csv, serde_json |
| memchr | 2.8.3 | Unlicense OR MIT | csv-core, quick-xml, serde_json |
| zmij | 1.0.23 | MIT | serde_json |

**Transitive crates of `petgraph`, `nalgebra` and rand's `chacha` feature**, measured 2026-10-12 with `cargo tree -e normal` on a probe crate that depends on the three with the features above (rmp #62). Licences from `cargo metadata`. `chacha20`, `cfg-if`, `cpufeatures`, `getrandom`, `libc` and `rand_core` are already in the table below. `cargo deny check licenses` (cargo-deny 0.20.2) on the probe prints "licenses FAILED" (exit 4) with lpg's `deny.toml`, rejecting `foldhash`, and "licenses ok" (exit 0) once `Zlib` is allowed.

| Crate | Version | Licence | Brought by |
|---|---|---|---|
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | petgraph |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 | petgraph |
| foldhash | 0.1.5 | Zlib | hashbrown 0.15.5 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | petgraph |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | indexmap |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | indexmap |
| approx | 0.5.1 | Apache-2.0 | nalgebra, simba |
| matrixmultiply | 0.3.11 | MIT/Apache-2.0 | nalgebra |
| rawpointer | 0.2.1 | MIT/Apache-2.0 | matrixmultiply |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | nalgebra, simba |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | num-rational |
| num-rational | 0.4.2 | MIT OR Apache-2.0 | nalgebra |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | nalgebra and its dependencies |
| simba | 0.10.2 | Apache-2.0 | nalgebra |
| wide | 1.7.1 | Zlib OR Apache-2.0 OR MIT | simba |
| safe_arch | 1.2.0 | Zlib OR Apache-2.0 OR MIT | wide |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | wide, safe_arch |
| typenum | 1.20.1 | MIT OR Apache-2.0 | nalgebra |

**Other transitive crates**, present before Sprint 4 ([Sprint 4 lab report](../evidence/lab-sprint-4/report.md#acceptance-criteria-per-task), AC #60). Versions and parents from `cargo tree -p lpg -e normal` ([raw](../evidence/lab-sprint-4/raw/cargo-tree-lpg-normal.txt)); licences from `cargo metadata`, equal to the crates.io `license` field of each version (queried 2026-10-11). `tokio` and `tracing` bring theirs in their own row above.

| Crate | Version | Licence | Brought by |
|---|---|---|---|
| bitflags | 2.13.2 | MIT OR Apache-2.0 | rustix |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | rustix |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 | rand |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | rand |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | rand, chacha20, getrandom |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | chacha20, getrandom |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | chacha20 |
| libc | 0.2.189 | MIT OR Apache-2.0 | getrandom |
| jiff-core | 0.1.1 | Unlicense OR MIT | jiff |

## Considered and not used

Each crate is studied, not depended on. The reason is in the decision cited.

| Crate | Reason | Decision |
|---|---|---|
| redb, fjall, sled, heed, rocksdb, surrealkv, turso_core | requirements a dependency cannot meet | [TS-04](storage.md#ts-04-own-storage-engine) |
| okaywal | too little use, no release since 2023 | [TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit) |
| txn, skipdb | too little use | [TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks) |
| indradb, cozo | MPL-2.0, little use, key-value encodings | [TS-06](storage.md#ts-06-graph-layout) |
| memmap2, parking_lot, dashmap | `unsafe` mmap; `std` suffices until measured | [TS-08](storage.md#ts-08-buffer-pool-and-memory-cap) |
| crc-fast, crc32fast, xxhash-rust | fallback, wrong polynomial, extra licence | [TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check) |
| fs2, fs4, fd-lock, nix | `std` `File::try_lock` since 1.89; `rustix` used directly for `fstatvfs`, `fallocate` and `renameat2` | [TS-27](storage.md#ts-27-single-process-lock), [TS-03](interfaces.md#ts-03-storage-abstraction) |
| pest, peg, lalrpop, chumsky, nom, logos, sqlparser, gqlparser, drasi-query-gql, gqlitedb | hand-written parser | [TS-19](query-engine.md#ts-19-parser) |
| chrono, chrono-tz, time, geo | bundled zone data, no zone names, formulas reproduced exactly | [TS-21](values-and-types.md#ts-21-values-and-types) |
| rstar, fast_hilbert, hilbert_2d | in-memory only, 2D only | [TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints) |
| neo4rs, bolt-proto, bolt-client, boltr, meshdb-bolt | Bolt 4.x only, unmaintained, too little use | [TS-22](bolt.md#ts-22-bolt-and-packstream) |
| rayon, rustworkx-core, graphalgs, faer | unordered reductions, heavy dependencies, little use | [TS-25](algorithms.md#ts-25-graph-algorithms) |
| metrics, async-trait, thiserror, futures | no per-handle sink; boxed futures and a hand-written error suffice | [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events), [TS-23](interfaces.md#ts-23-authentication-and-authorisation) |
