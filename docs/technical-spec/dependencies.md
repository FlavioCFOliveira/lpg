---
title: Dependencies
status: draft
last-reviewed: 2026-10-02
related:
  - quality-and-testing.md
  - README.md
---

# Dependencies

## TS-31 Dependency set

- **Decision**: the crate depends on the crates in the table below and on nothing else. Each one serves a decision. Licences are checked by `cargo-deny` ([TS-30](quality-and-testing.md#ts-30-testing-and-conformance)).
- **Satisfies**: NFR-DEP-001, NFR-DEP-002, NFR-SIMP-001.

crates.io data as of 2026-10-02 (downloads: total / last 90 days).

| Crate | Version | Licence | Downloads | Last release | Kind | Used by |
|---|---|---|---|---|---|---|
| tokio | 1.53.1 | MIT | 1 020.7 M / 242.4 M | 2026-07-20 | runtime | [TS-02](architecture.md#ts-02-async-api-and-threading), [TS-22](bolt.md#ts-22-bolt-and-packstream) |
| tracing | 0.1.44 | MIT | 893.2 M / 206.0 M | 2025-12-18 | runtime | [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events) |
| crc32c | 0.6.8 | Apache-2.0/MIT | 85.9 M / 16.5 M | 2024-06-09 | runtime | [TS-12](storage.md#ts-12-checksums-corruption-and-integrity-check) |
| jiff | 0.2.37 | Unlicense OR MIT | 209.3 M / 75.3 M | 2026-09-12 | runtime | [TS-21](values-and-types.md#ts-21-values-and-types) |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | 533.5 M / 114.7 M | 2025-09-30 | runtime | [TS-25](algorithms.md#ts-25-graph-algorithms) |
| nalgebra | 0.35.0 | Apache-2.0 | 92.7 M / 16.1 M | 2026-05-24 | runtime | [TS-25](algorithms.md#ts-25-graph-algorithms) |
| rand | 0.10.3 | MIT OR Apache-2.0 | 1 815.9 M / 475.0 M | 2026-09-20 | runtime | [TS-25](algorithms.md#ts-25-graph-algorithms), database UUID ([TS-05](storage.md#ts-05-files-and-pages)) |
| csv | 1.4.0 | Unlicense/MIT | 256.8 M / 52.4 M | 2025-10-17 | runtime | [TS-26](operations.md#ts-26-backup-restore-import-and-export) |
| serde, serde_json | serde_json 1.0.151 | MIT OR Apache-2.0 | 1 372.9 M / 338.0 M (serde_json) | 2026-07-20 | runtime | [TS-26](operations.md#ts-26-backup-restore-import-and-export) |
| quick-xml | 0.42.0 | MIT | 444.2 M / 123.2 M | 2026-08-22 | runtime | [TS-26](operations.md#ts-26-backup-restore-import-and-export) |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | 853.1 M / 195.4 M | 2026-03-11 | dev | [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |
| loom | 0.7.2 | MIT | 68.6 M / 15.5 M | 2024-04-23 | dev | [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |
| antlr4rust | 0.6.0 | BSD-3-Clause | 3.3 M / 2.6 M | 2026-09-27 | dev, test-only oracle crate, not a dependency of `lpg` | [TS-19](query-engine.md#ts-19-parser), [TS-30](quality-and-testing.md#ts-30-testing-and-conformance) |

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
| fs2, fs4, fd-lock | `std` `File::try_lock` since 1.89 | [TS-27](storage.md#ts-27-single-process-lock) |
| pest, peg, lalrpop, chumsky, nom, logos, sqlparser, gqlparser, drasi-query-gql, gqlitedb | hand-written parser | [TS-19](query-engine.md#ts-19-parser) |
| chrono, chrono-tz, time, geo | bundled zone data, no zone names, formulas reproduced exactly | [TS-21](values-and-types.md#ts-21-values-and-types) |
| rstar, fast_hilbert, hilbert_2d | in-memory only, 2D only | [TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints) |
| neo4rs, bolt-proto, bolt-client, boltr, meshdb-bolt | Bolt 4.x only, unmaintained, too little use | [TS-22](bolt.md#ts-22-bolt-and-packstream) |
| rayon, rustworkx-core, graphalgs, faer | unordered reductions, heavy dependencies, little use | [TS-25](algorithms.md#ts-25-graph-algorithms) |
| metrics, async-trait, thiserror, futures | no per-handle sink; boxed futures and a hand-written error suffice | [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events), [TS-23](interfaces.md#ts-23-authentication-and-authorisation) |
