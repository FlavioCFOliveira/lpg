# Sprint 4 plan: bulk import and export (draft for task #4)

Prepared 2026-10-11 on branch `feature/4-bulk-import-and-export-four-formats-and-fast-initi` at `6452095`. Read-only for the repository: no file other than this one and no `rmp` record was changed. The laboratory tools were updated (§9).

## 0. Basis, method and limits

* **Read:** `CLAUDE.md`, `README.md`; `rmp sprint get 4`; sprint comments #14 (scope), #15 (order and synergy), #16 (exit), #17 (laboratory), #35, #48, #56; task #4; the Sprint 3 plan (`docs/evidence/planning/sprint-3-plan.md`, the model for this file) and the comments of task #3 (#250–#260).
* **Specification read:** `docs/technical-spec/operations.md` (TS-26), `values-and-types.md` (TS-21), `dependencies.md` (TS-31), `quality-and-testing.md` (unverified points 1–19), `interfaces.md` (TS-23, `Database` row, storage trait rule at line 93), `storage.md` (Build pages, stamps, copy buffer), `transactions-and-recovery.md` (graph lock row, line 246), `catalog-and-indexes.md` (TS-18 build and ONLINE commit); `docs/functional-spec/functional-requirements.md` (FR-IMP-001..006, FR-OPS-001..013, FR-LIM-007, BR-TYPE-002), `use-cases.md` (UC-OPS-001, UC-OPS-003, UC-OPS-004), `non-functional-requirements.md` (NFR-PERF-002, NFR-PERF-005, NFR-DEP-001/002, NFR-SIMP-001), `open-questions.md` (OQ closures for import).
* **Code read:** `Cargo.toml`, `deny.toml`, and the items cited in §2.
* **Reference clones** (licence read in each repository's licence file, §4): APOC `e52bc20` (new, sparse: `core/.../apoc/export`, `common/.../apoc/export`, `apoc/util`, `apoc/load`), petgraph-graphml `4acc65f` (new); reused: Neo4j `54a7dcf` (sparse; `community/values`, `community/csv`, `community/import-tool` added for this plan; `community/import-util` read with `git show`), PostgreSQL `1b5dd3a` (docs read with `git show`), Kuzu `89f0263`, Turso `be5bab8`, libSQL `d6c75af`, ldbc-snb-impls `11db98c`, opengql/grammar `16ea71b`. Crate sources downloaded from `static.crates.io` to `/tmp/crates-src`: serde_json 1.0.154, quick-xml 0.42.0, csv 1.4.0, csv-core 0.1.13, ext-sort 0.1.6, extsort 0.5.0. NetworkX 3.7 read from its wheel.
* **Official sources fetched 2026-10-11** (User-Agent `lpg-planning (research)`, never the user's address): crates.io API (versions, licences, downloads); GitHub API (fio, sysstat tags; ldbc_snb_datagen_spark licence and releases); KDE GitLab API (heaptrack tags); PyPI JSON (networkx, xmlschema, elementpath, igraph); Docker Hub API (neo4j, ldbc images); the GraphML 1.0 schema files from `http://graphml.graphdrawing.org/xmlns/1.0/`.
* **Measured for this plan:**
  * NetworkX 3.7 and xmlschema 4.3.2 on three GraphML probe files (§3.2 item B; files in Appendix A).
  * `neo4j-admin database import full` of Neo4j 2026.09.0 community on a CSV probe with `point{srid:…}` headers and `NaN`/`Infinity` doubles, then a `cypher-shell` read of the values (§3.1 point 2; Appendix A).
  * `cargo deny check licenses` (cargo-deny 0.20.2, lpg's `deny.toml`) and `cargo tree -e normal` on a probe crate depending on csv 1.4.0, serde_json 1.0.154 (`float_roundtrip`) and quick-xml 0.42.0: "licenses ok"; tree: csv-core 0.1.13, itoa 1.0.18, ryu 1.0.23, serde_core 1.0.229, memchr 2.8.3, zmij 1.0.23. No `serde` crate in the tree.
* **Unverified (stated plainly):** Memgraph's import tools and DuckDB's `COPY` reject handling were not read (no clone made); Neo4j's import tool's id mapping by sorting was not read in its code (only the header and option code); the correctness of every copy or port candidate against lpg's requirements (none was run inside lpg); whether `neo4j-admin import` accepts the TS-21 point lexical form with spaces after the colons (the probe used no spaces); the GraphML schema files' licence (they are fetched at test time, never vendored).

## 1. Requirements digest

### 1.1 Technical decisions and requirements in scope

Scope ids from sprint comment #14, exit ids from #16, synergy ids from #15, laboratory ids from #17.

| Id | Demands (1–2 lines) | Where |
|---|---|---|
| TS-26 Export layouts | CSV node and edge files with Neo4j header fields, RFC 4180 quoting, `;` arrays with `\` escape; JSON Lines APOC layout; GraphML one `<key>` per property, `labels` data key; native dump with format version and CRC-32C per block, TS-21 record encoding; query results per FR-IMP-003. | `operations.md:51-58` |
| TS-26 Type annotations | Uniform and mixed properties; the per-type table; tagged object `{"$lpg_type", "$value"}`; GraphML type attribute in namespace `https://crates.io/crates/lpg/graphml`. | `operations.md:60-76` |
| TS-26 Transactional import | One transaction, all records or none, writes and old versions within the cap. | `operations.md:77` |
| TS-26 Fast initial load | Steps 1–7: wait for backup, X graph lock, empty check; external sort, constraint checks on sorted runs; bottom-up node, edge, adjacency trees at 90 %; indexes bottom-up; `sync_data`; one WAL group installs roots and allocation state; release lock. Failure or crash before step 6 leaves the graph empty. | `operations.md:78-87` |
| TS-26 Backup exclusion | `Core::backup_or_load` latch; a backup fails with `Error::LoadRunning` during a load; a load waits for a running backup. | `operations.md:27-30` |
| TS-21 Lexical forms | ISO 8601 temporal text, named zones in brackets, Neo4j date forms; points `{srid, x, y[, z]}`. | `values-and-types.md:38-40` |
| FR-IMP-001 | Four formats; non-native values annotated. | FS:416-417 |
| FR-IMP-002 | Transactional import (one transaction, cap) and fast initial load (empty graph, writers wait, indexes and constraints built in the load, backup fails, non-empty graph fails). | FS:418-419 |
| FR-IMP-003 | Database, graph, query-result scopes; format and scope matrix; excluded pairs fail. | FS:420-421 |
| FR-IMP-004 | Abort (default) or skip; every invalid record reported. | FS:422-423 |
| FR-IMP-005 | New element identifiers; input ids only connect edges. | FS:424-425 |
| FR-IMP-006 | Annotation per format; NaN, mixed types, points, temporals lossless; a reader that ignores the lpg namespace reads a string. | FS:426-431 |
| FR-OPS-001 (interlock) | Backup fails during a load; a load waits for a backup. | FS:387-388 |
| FR-OPS-003 | Bulk import and export, as FR-IMP-001..005. | FS:391-392 |
| FR-LIM-007 | Writes of one transaction and their old versions fit the cap. | FS:312-313 |
| BR-TYPE-002 (import part) | Import assigns new identifiers. | FS:259-260 |
| UC-OPS-003, UC-OPS-004 | Import (modes, report, A1–A4) and export (A1 storage full). | `use-cases.md:146-171` |
| NFR-PERF-005 | Graphs far larger than memory; judged at 1–10 % cache (measurement deferred, §1.2). | NFS:23 |
| NFR-PERF-002 | LDBC SNB baseline that Sprint 4's importers must be able to load (comment #15); measurement in Sprint 6. | NFS:18 |

### 1.2 Exit condition without timing

Exit comment #16 lists six conditions. Sprint UPDATE #56 removes timing from it: "The timing parts of this sprint's earlier DECISION entries (laboratory measurements, benchmarks, latency and throughput, cache-ratio runs) **no longer apply**; their correctness checks stay. Non-timing measurements that prove correctness (e.g. memory within the cap) remain in scope."

**In Sprint 4:** lossless round trips in four formats, including NaN, mixed types, points and temporals; exact scope contents and failing excluded pairs; abort and skip report every invalid record; new identifiers; fast load fails on a non-empty graph, makes writers wait, makes a backup fail, waits for a running backup; a crash or ENOSPC at each fast-load step leaves the graph empty; memory within the cap (accountant and `heaptrack`); temporary-file bytes through the storage trait; `cargo-llvm-cov` on every importer and exporter branch.

**Deferred to Sprint 6 (timing):** fast-load throughput against M-LAYOUT's 1.88 M edges/s; `fio` sequential ceilings, `iostat -x` and `pidstat -d` fractions of the device ceiling; LDBC and M-LAYOUT runs at the sizes where the cache holds 1 % and 10 % of the data.

### 1.3 Unverified points 4, 12 and 13 (decision of this plan)

Exit #16 asks the report to "close unverified points 4, 12 and 13 at full scale, with properties and concurrent readers". UPDATE #56 keeps correctness checks and removes timing. Applied per point (`quality-and-testing.md:148-157`):

| Point | Text (short) | Correctness part, Sprint 4 | Timing part, Sprint 6 |
|---|---|---|---|
| 4 | Compressed leaves with readers concurrent with the committer, GC, index builder and the fast load | Readers run during a fast load of a graph with properties whose trees have at least three levels: before the install they read the empty graph, after it the whole graph; a snapshot taken before the install still reads the empty graph after it (§3.1 point 1); integrity check passes. Tasks #58, #60. The committer, GC and index-builder parts are not Sprint 4 scope and stay open. | Concurrency at full scale. |
| 12 | Neo4j `point` header with `srid`; GraphML readers and an extension attribute | Closed: Neo4j source accepts `srid` in header and value maps (R-NEO `community/values/.../storable/PointValue.java:311-321, 474-478`); measured run accepted it (§3.1 point 2); NetworkX 3.7 reads the attribute's files and ignores it; the GraphML 1.0 XSD rejects it (§3.2 item B). Tasks #55, #59, #60 re-check on lpg's real export. | None. |
| 13 | M-LAYOUT limits: one host, no properties, `O_DIRECT`, one 100 M point | None: every limit concerns the throughput measurement. | Whole point (with the throughput run). |

## 2. Code reconnaissance

Sprints 1–3 code: 92 442 lines of Rust in `src/` (`find src -name '*.rs' | xargs wc -l`). No import, export or dump module exists; `csv`, `serde_json` and `quick-xml` are in TS-31 but not in `Cargo.toml`.

### What Sprint 4 builds on

| Piece | Where | Use in Sprint 4 |
|---|---|---|
| External merge sort `Sorter`, `Engine::sorter`, temp files `sort-temp-*` through the storage trait, removed at open | `src/storage/sort.rs:1-37, 179-346, 201` | fast-load step 2 |
| Bottom-up build `bulk_build_pages`, `Built`, `install_replacing`, page leases, release on failure and at open | `src/storage/btree/bulk.rs:1-88, 577-735, 738-809` | fast-load steps 3–6 |
| `install_tree` (page-level install inside a commit closure that returns a `Vec` of trees) | `src/storage/btree/write.rs:156-175`; `bulk.rs:635-666` | one WAL group for several roots |
| Commit carries the identifier counters `Ids` | `src/tx/commit.rs:59-66, 164` | allocation state in the install group |
| Constraint and owned-index install in one commit | `src/tx/mod.rs:877` (`commit_installing`) | pattern for the install group |
| Backup-or-load latch, try-latch, install step | `src/storage/engine.rs:254, 272, 283`; backup refusal `src/ops/backup.rs:258-261`; `Error::LoadRunning` → 51N41 `src/storage/error.rs:63, 425` | interlock (FR-OPS-001) |
| Graph lock resource (X for a fast load; no X taker yet) | `src/tx/lock.rs:85-87`; S taker `src/storage/graph.rs:243-248` | fast-load step 1 |
| Graph trees and keys, `adj_key`, `create_node`, `create_edge`, statistics `recount` | `src/storage/graph.rs:1-24, 72-80, 125, 307, 492, 767` | sort keys, transactional import, statistics |
| Index keys, constraint check, sorted uniqueness check | `src/index/mod.rs:477, 670`; `src/index/build.rs:554-575` | constraints on sorted runs, index build |
| Record encoding, value size | `src/types/record.rs:271, 316, 545` | native dump |
| Temporal lexical forms (format and parse), point lexical form (test-only) | `src/types/temporal.rs:64, 206`; `src/types/spatial.rs:153-190` (`#[cfg(test)]`) | type annotations |
| Random value generator (record round trip) | `src/types/tests.rs:196, 351` | round-trip value generator |
| Memory accountant | `src/storage/memory.rs:145` | id map, report entries, sort reservations |
| Fault-injecting storage and crash suite (`BulkLoad` workload) | `src/storage/fault.rs:440-590`; `src/suites/fault.rs:1-40, 166-183` | fast-load fault matrix |
| Session API, result stream | `src/api/session.rs:529, 581`; `src/api/stream.rs:340, 351` | query-result export, API entry points |
| Authorisation actions | `src/auth/mod.rs:50-80` | import and export authorisation |
| Backup manifest with CRC-32C | `src/ops/backup.rs:132-160` | native dump block and trailer pattern |
| M-LAYOUT generator port, bulk lab | `src/lab/generator.rs:1-40`; `src/lab/bulk.rs:1-30` | synthetic graphs with properties |
| Test directories in `/tmp/lpgram`, seeds, fuzz entry module | `src/test_support/mod.rs:47, 91-109`; `src/fuzz.rs` | every task |

### Changes to existing code

* **Several trees in one install group.** `Built::install_with` commits one tree (`bulk.rs:635-666`); the fast load needs every tree of the graph, its indexes, the counters, the statistics and the graph's catalog entry in one group (TS-26 step 6). The commit closure already returns a `Vec` of trees.
* **One backup-or-load guard for the whole load.** Each `Built` holds its own guard (`bulk.rs:577-587`); a load of several trees must hold one guard from its first run to its install.
* **Graph trees replaced, not created.** The graph's trees exist, empty, from its creation; the load installs into them (`install_replacing`, `bulk.rs:626-635`), not new trees (`bulk_build_pages` draws a new id, `bulk.rs:684-688`).
* **Visibility of loaded entries.** Built entries carry stamp 0, which is "frozen", visible to every snapshot (`bulk.rs:223, 774`; `storage.md:157`). §3.1 point 1.
* **Point lexical form** leaves `#[cfg(test)]` (`spatial.rs:153, 167`).

## 3. Specification gaps

### 3.1 Decidable from references (task #55 records each decision)

| # | Gap (cited) | Proposed decision | Reference |
|---|---|---|---|
| 1 | Bulk-built entries are frozen (stamp 0), so a snapshot taken before the install would read the loaded graph after it, against serializable isolation (README) and TS-26 step 1 ("readers keep reading the empty graph"). | The install group also writes a new version of the graph's catalog entry; a snapshot that reads the older version reads the graph as empty. The graph was empty and had no writer since step 1, so that is its exact state at such a snapshot. | PostgreSQL documents the defect of the alternative: `COPY FREEZE` "violates the normal rules of MVCC visibility" (R-PG `doc/src/sgml/ref/copy.sgml:275-278`), `TRUNCATE` "is not MVCC-safe" (`truncate.sgml:147-149`); lpg's catalog tree is MVCC-stamped (`catalog-and-indexes.md:29`). |
| 2 | NaN and ±infinity in CSV: TS-26 says "annotated", but Neo4j's `double` column parses `NaN`, `Infinity`, `-Infinity` with `Double.parseDouble` (R-NEO `community/csv/.../Extractors.java:611-627`). | A `double` column writes `NaN`, `Infinity`, `-Infinity`; the column stays `double`. | Measured 2026-10-11: Neo4j 2026.09.0 imported them and `cypher-shell` returned `NaN` and `Infinity` (Appendix A). |
| 3 | CSV point columns and SRIDs. | Header `point`; each value in the TS-21 form with its `srid`. Neo4j merges header and value maps, the value winning (`PointValue.java:311-321`), and accepts `srid` (`:474-478`); `crs` and `srid` together fail (`:336`). | Measured: header `point{srid:7203}` with a value holding `srid` imported with SRID 7203 (Appendix A). Spaces after colons: unverified (§0). |
| 4 | CSV file set and header names. | Graph export: one node file and one edge file, property columns the union over the elements, absent property empty (TS-26); type names in Neo4j's lower case (`long`, `double`, `boolean`, `string`, `date`, `localtime`, `time`, `localdatetime`, `datetime`, `point`, `<t>[]`); import reads them case-insensitively. | Neo4j lower-cases the type spec (`community/import-util/.../csv/DataFactories.java:593`); extractor names `Extractors.java:173-204`. |
| 5 | JSON Lines edge `start`, `end` shape and `id` type. | `start` and `end` are objects `{"id", "labels"}`; `id` a JSON string; import reads only `id`. | APOC `core/.../export/json/JsonFormatSerializer.java:37-51, 88, 106-116` (Apache-2.0). |
| 6 | GraphML labels and edge type. | Node labels only as data key `labels` (`:A:B`), edge type only as data key `label`; no `labels`/`label` attributes on `<node>`/`<edge>`; `\` escapes `:` and `\` in names. | APOC writes both the attribute and the data key (`XmlGraphMLWriter.java:129-156, 169-170`); the GraphML 1.0 XSD rejects the attribute (measured, §3.2 B). |
| 7 | Characters GraphML cannot hold. XML 1.0 `Char` excludes U+0000–U+0008, U+000B, U+000C, U+000E–U+001F, U+FFFE, U+FFFF; parsers turn `\r` into `\n`. | A string value holding an excluded character is annotated (type `STRING`, escaped lexical form of point 9); a label, edge type or property name holding one fails the export with an error naming it; `\r` is written `&#13;`; the reader applies XML 1.0 end-of-line handling. | W3C XML 1.0 (Fifth Edition) §2.2 `Char`, §2.11; quick-xml 0.42.0 `escape` escapes `\r` (`src/escape.rs:93-110`), `xml10_content` normalises (`src/events/mod.rs:625-628`). |
| 8 | JSON float parsing. | `serde_json` with feature `float_roundtrip`; TS-31 row 1.0.154; the `serde` row is removed (no crate needs it). | serde_json 1.0.154 parses floats by `POW10` products without the feature (`src/de.rs:638-660`) and with a correctly rounded parser with it (`src/de.rs:623-636`); serde_json and csv 1.4.0 depend on `serde_core`, not `serde` (their `Cargo.toml`; measured tree §0). |
| 9 | Lexical forms TS-21 lacks: byte strings, lists (nested, mixed), records, NaN and ±infinity, escaped strings, node, edge and path values of query results; the type-name grammar of `lpg:<type name>`. | GQL literal syntax where `GQL.g4` has one: byte string `X'…'` (`BYTE_STRING_LITERAL`, `GQL.g4:3188`), list `[…]` (`listValueConstructorByEnumeration`, `:2500`), record `RECORD {k: v}` (`recordConstructor`, `:2514`), strings quoted as `characterStringLiteral` (`:2972`); floats as Rust's shortest round-trip text, NaN and infinities as in point 2; node, edge, path as APOC's maps; type names those of IV008 (`values-and-types.md`, IV008 row). | APOC `FormatUtils.toMap`, `toString` (`common/.../export/util/FormatUtils.java:66-103`); Java `Double.toString`. |
| 10 | Native dump layout. | Header (magic, dump format version, scope, crate format version); blocks of catalog definitions and tokens, node records, edge records, then index and constraint definitions; each block length-prefixed with a CRC-32C; a trailer with counts and its own CRC-32C, so a truncated dump fails. | lpg backup `MANIFEST` (`src/ops/backup.rs:132-160`, TS-26); Turso `.dump` writes schema, data, then indexes (`cli/app.rs:1843-1874`, MIT). |
| 11 | What the database scope holds. | As FR-OPS-008 for backups: the catalog, every graph, every index and constraint definition; not registered procedures. | FR-OPS-008 (FS:401-402). |
| 12 | What an invalid record is. | Malformed record; value not convertible to its column or tagged type; duplicate input id in one id space; edge with an unknown endpoint; constraint violation; edge whose endpoint was skipped. | Neo4j `--skip-duplicate-nodes`, `--skip-bad-relationships` (R-NEO `community/import-tool/.../ImportCommand.java:158-159`); TS-26 step 2; FR-IMP-002. |
| 13 | Report entry content. | Source file, record number (line for CSV and JSON Lines, element ordinal for GraphML, block and record for the dump), input id, reason, GQLSTATUS. Delivery: §3.2 item C. | Kuzu `WarningContext` keeps file and line per warning (`src/include/processor/warning_context.h`, MIT); PostgreSQL `ON_ERROR` notices (`copy.sgml:444-466`). |
| 14 | GQLSTATUS of import and export failures (invalid record, excluded scope pair, non-empty graph, file exists, storage full). | ISO conditions insert where it has the condition, else Neo4j's code (the rule of `interfaces.md:29`); the list is recorded in TS-26. | `interfaces.md:29`; `Error::LoadRunning` already maps to 51N41 (`src/storage/error.rs:425`). |
| 15 | Authorisation of import and export (FR-AUTH-001). | Session operations; one request per (action, object): export asks `Traverse` and `Read` on each graph of the scope; import asks `Create`, `SetLabel`, `SetProperty` on the target graph; a database-scope import asks the catalog creation actions. | TS-23 (`interfaces.md:142-162`); `src/auth/mod.rs:50-80`. |
| 16 | Export file commit and target. | Each output file is written as `<name>.tmp`, synced, renamed, then the directory synced; export fails if a target file exists, and removes its files on failure (UC-OPS-004 A1). | TS-26 `MANIFEST` rule; `pg_basebackup` durable rename (R-PG `src/bin/pg_basebackup/pg_basebackup.c:2298-2321`, cited in TS-26). |
| 17 | Statistics after a fast load (TS-17). | Counts per label and per `(label, type, direction)` gathered during the build join the install group. | The ONLINE commit adds the build's counts in one batch (`catalog-and-indexes.md:99`; `switch`, `src/index/mod.rs:974`). |
| 18 | Tokens new to the catalog. | Created by an ordinary transaction before the load or import writes; tokens left by an abort stay unused. | The catalog token API (`src/catalog/mod.rs`); TS-17. |
| 19 | Fast-load sort plan. | Sort nodes by (id space, input id): assigns identifiers in order, finds duplicates; sort edges by start id, merge to resolve sources, then by end id for targets; sort adjacency keys (`adj_key`); sort each index's keys (`index_keys`); uniqueness on adjacent sorted keys (`UniqueSorted`). | TS-26 step 2; PostgreSQL `nbtsort.c` and `tuplesort.c` (cited in TS-26); lpg `build.rs:554-575` (a port of PostgreSQL's `enforceUnique`). |
| 20 | Memory of import and export. | The transactional import's id map and the report entries kept in memory are charged to the cap; parse and write buffers have fixed sizes outside the cap, like the 1 MiB copy buffer; the fast load's memory is its sort reservations. | `storage.md:247`; FR-LIM-003, FR-LIM-007. |
| 21 | Record order in files. | Importers process nodes before edges; the GraphML reader reads its file twice (nodes, then edges), since GraphML allows edges before nodes. | NetworkX reads all nodes before edges (`readwrite/graphml.py:902-960`, BSD-3-Clause). |
| 22 | JSON integers outside the `i64` range and JSON `null` property values. | Both are invalid records. | `i64` is the one integer type (TS-21 Numeric types); a property cannot hold null (IV011). |
| 23 | Empty strings. | CSV per TS-26 (`lpg:annotated`); GraphML `<data>` with no text reads as the empty string. | NetworkX reads `""` (`readwrite/graphml.py:1056-1057`). |

### 3.2 Needs a user decision (asked in task #55)

* **A. CSV input beyond lpg's own layout.** TS-26 names five header fields. LDBC's Neo4j load files, which Sprints 5 and 6 need (comment #15), use id spaces (`id:ID(Person)`, `:START_ID(Comment)`), the `|` delimiter and labels given per file (`--nodes=Comment:Message=…`) (ldbc-snb-impls `cypher/scripts/headers.txt`, `import-to-neo4j.sh`, Apache-2.0). Options: (a, recommended) accept that Neo4j subset: id spaces, `:IGNORE`, `|` or another one-character delimiter, array delimiter, labels or type per file; APOC's header regex is a port source (§4.1); (b) only lpg's layout; a converter for LDBC files is written in Sprint 6; (c) the whole Neo4j import option set.
* **B. GraphML type attribute and the GraphML schema.** Measured 2026-10-11 with xmlschema 4.3.2 against the official `graphml.xsd`: a file with the lpg attribute on `<key>` and `<data>` is invalid ("attribute not allowed for element"); without it, valid; NetworkX 3.7 reads all of them and ignores the attribute (Appendix A). The schema provides no `xs:anyAttribute`; it lets extensions redefine `key.extra.attrib` and `data.extra.attrib` (`graphml-structure.xsd:44-49`), as `graphml-attributes.xsd` does with `xs:redefine` (line 27). Options: (a, recommended) keep the attribute (FR-IMP-006) and ship an lpg extension schema that redefines those two groups, so the export validates against it; (b) keep the attribute, no schema; record that validation against the base schema fails; (c) amend FR-IMP-006: carry the type in a companion `<data>` key, valid against the base schema.
* **C. Delivery of the invalid-record report.** FR-IMP-004 requires every invalid record in the report; a skip run over a large file can report many. Options: (a, recommended) Neo4j's model: the report is written to a file the application names, through the storage trait, one line per invalid record; the call returns counts (Neo4j default `report.json.log`, R-NEO `community/import-tool/.../ImportContext.java:114`, `--report-file`); (b) returned in memory, charged to the cap; the import fails under FR-LIM-001 when it would exceed it; (c) passed to an application callback, one call per record.
* **D. Import of a database-scope native dump.** UC-OPS-003 imports into an existing graph. Options: (a, recommended) import into a database whose catalog holds no user object: one transaction creates the catalog objects and the index and constraint definitions, then each graph is fast-loaded; identifiers are new (BR-TYPE-002); (b) a database dump is imported graph by graph into existing graphs; (c) the database scope is export-only.

### 3.3 Decided in this planning task (no spec change; for task #4's DECISION comment)

* **Unverified points 4, 12, 13:** split as §1.3, from UPDATE #56.
* **No new dependency.** Runtime: `csv` 1.4.0, `serde_json` 1.0.154 with `float_roundtrip`, `quick-xml` 0.42.0, all in TS-31 (`dependencies.md:30-33`); `serde` dropped (§3.1 point 8). Dev: none; the round-trip suite uses lpg's seeded generator (§4.13).
* **External readers stay outside the crate:** NetworkX and xmlschema in `/tmp/lpg-lab-venv`, Neo4j in Docker; scripts under `docs/evidence/lab-sprint-4/`; neither enters `cargo tree -p lpg`.
* **Timing tools are not used** (UPDATE #56): `fio` 3.43 and sysstat 12.8.0 are installed and recorded, not run.
* **Module:** `src/ops/bulk/` with `csv`, `jsonl`, `graphml`, `dump`, `fast` and a shared record model (TS-01 `ops`).

## 4. Reuse inventory

Verdicts: **COPY** (files copied, then adapted), **PORT** (rewritten in Rust from another language or architecture, structure kept), **DEPEND** (a crate as a dependency), **STUDY** (read for decisions, nothing copied), **REJECT** (reason given). Licences were read in each clone's licence file, or in the crate's `Cargo.toml` and crates.io metadata for crates. Constraints every copy meets or is adapted to: no `unsafe`, edition 2024, every file through the storage trait (`interfaces.md:93`), memory charged to the cap, minimal dependencies (TS-31). Every copy keeps its notices and records project, path and commit in the file header and in its task's `rmp` log. GPL (Neo4j Community) and BSL (Memgraph) code is never copied.

### 4.0 Candidates searched

crates.io (2026-10-11): `csv`, `csv-core`, `serde`, `serde_json`, `simd-json`, `sonic-rs`, `jsonl`, `json-lines`, `serde-jsonlines`, `quick-xml`, `xml-rs`, `xml`, `roxmltree`, `xmlparser`, `petgraph`, `petgraph-graphml`, `graphml`, `proptest`, `quickcheck`, `arbitrary`, `extsort`, `ext-sort`, `external_sort`, `itoa`, `ryu`, `zmij`, `memchr`. Projects: APOC, Neo4j import tool, NetworkX, igraph, petgraph-graphml, Kuzu `COPY`, PostgreSQL `COPY`, Turso `.dump` and CSV extension, LDBC SNB datagen and impls. Not read: Memgraph (BSL; no clone), DuckDB (no clone).

### 4.1 CSV reader and writer, Neo4j header rules

| Candidate | Licence, commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| `csv` 1.4.0 (with `csv-core` 0.1.13) | Unlicense/MIT (crates.io; cargo-deny accepts it, measured §0), 263 M downloads | — | RFC 4180 reader and writer, quoting, any one-byte delimiter, `io::Read`/`io::Write` | Wrap `StorageFile` in `io::Read`/`io::Write` adapters | **DEPEND** (TS-31) |
| APOC `CsvHeaderField`, `CsvLoaderConstants` | Apache-2.0 (`LICENSE`), `e52bc20` | `core/.../export/csv/CsvHeaderField.java` (140), `CsvLoaderConstants.java:28-36` | Neo4j header field regex: name, type, `{options}`, `(idspace)`, `[]` | Java regex → hand-written parser, no `regex` crate | **PORT** the field grammar |
| APOC `CsvFormat`, `CsvEntityLoader` | Apache-2.0, `e52bc20` | `CsvFormat.java` (431), `CsvEntityLoader.java` (346) | Graph and query-result CSV export; import with id spaces | Lossy mixed types (TS-26 Rejected) | **STUDY** |
| Neo4j import tool, `DataFactories`, `Extractors`, `PointValue` | GPL-3.0 (`community/*/LICENSE.txt`), `54a7dcf` | `import-util/.../DataFactories.java` (619), `csv/.../Extractors.java:173-212, 611-627`, `values/.../PointValue.java:296-480` | The authority on header types, case rules, point and double parsing | GPL | **STUDY** only |
| Kuzu CSV reader | MIT (`LICENSE`), `89f0263` | `src/processor/operator/persistent/reader/csv/*.cpp` | Parallel reader, dialect detection, `IGNORE_ERRORS` | C++, its own column types | **STUDY** (error capture §4.9) |
| PostgreSQL `COPY` | PostgreSQL licence, `1b5dd3a` | `doc/src/sgml/ref/copy.sgml` | `ON_ERROR`, `REJECT_LIMIT`, `LOG_VERBOSITY` | Docs only | **STUDY** |
| Turso CSV virtual table | MIT (`LICENSE.md`), `be5bab8` | `extensions/csv/src/lib.rs` | SQLite `csv` vtab port | Query-time reader, no export | **STUDY** |

### 4.2 JSON Lines reader and writer

| Candidate | Licence, commit | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| `serde_json` 1.0.154, feature `float_roundtrip` | MIT OR Apache-2.0, 1 415 M downloads | `Value` model, writer with shortest float text (`zmij`), correctly rounded float parser with the feature | One `from_slice` per line read through a `BufRead` adapter; no `serde` derive | **DEPEND** (TS-31) |
| APOC `JsonFormatSerializer`, `JsonImporter` | Apache-2.0, `e52bc20` (`core/.../export/json/`, 130 and 388 lines) | Node and relationship object layout; import of the same | Lossy temporals as strings | **PORT** the layout; **STUDY** the importer |
| `simd-json` 0.18.1, `sonic-rs` 0.5.10 | Apache-2.0 OR MIT; Apache-2.0 | Faster parsing | Speed is not a Sprint 4 criterion; new dependencies (NFR-DEP-001) | **REJECT** |
| `jsonl` 4.0.1, `json-lines` 0.1.2, `serde-jsonlines` 0.7.0 | MIT OR Apache-2.0; MIT | Line framing over serde | A `BufRead` loop suffices; last releases 2021 to 2025 | **REJECT** |

### 4.3 GraphML reader and writer

| Candidate | Licence, commit | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| `quick-xml` 0.42.0 | MIT, 462 M downloads | Pull reader with namespace resolution, writer, `escape` with `&#13;`, XML 1.0 end-of-line normalisation | XML-excluded characters handled by lpg (§3.1 point 7) | **DEPEND** (TS-31) |
| NetworkX `readwrite/graphml.py` | BSD-3-Clause (wheel `LICENSE.txt`), 3.7 | Key table, `attr.type` mapping, `<default>`, boolean literals, empty `<data>` (lines 436-500, 1001-1093) | Python, in-memory DOM | **PORT** the reader's key and type handling |
| APOC `XmlGraphMLWriter`, `XmlGraphMLReader` | Apache-2.0, `e52bc20` (266 and 477 lines) | APOC's GraphML layout (keys, `labels`, `label`), import | Non-standard attributes (§3.1 point 6), `attr.list` | **PORT** the layout minus attributes; **STUDY** the reader |
| `petgraph-graphml` 5.0.0 | MIT OR Apache-2.0, `4acc65f` (`src/lib.rs`, 454) | Writer, all keys `string` | Writer only, `xml-rs`, petgraph types | **STUDY** |
| `graphml` 0.1.1 | MIT OR Apache-2.0 | — | 73 downloads (NFR-DEP-001) | **REJECT** |
| `roxmltree` 0.21.1, `xml-rs` 1.0.0, `xmlparser` 0.13.6 | MIT OR Apache-2.0; MIT; MIT/Apache-2.0 | DOM reader; pull parser | TS-31 already chose `quick-xml`; DOM holds the whole file in memory | **REJECT** |
| igraph | GPL (PyPI 1.0.0 metadata) | GraphML reader | GPL | **REJECT** |

### 4.4 Native dump

| Candidate | Licence, commit | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| lpg record encoding and backup manifest | MIT (own) | `src/types/record.rs:271-316, 545`; `src/ops/backup.rs:132-160` (CRC-32C, versioned header) | Block framing and trailer (§3.1 point 10) | **COPY** (own) |
| Turso `.dump` | MIT, `be5bab8` | `cli/app.rs:1828-2056`: schema, then data, then indexes and triggers | SQL text | **STUDY** the order |
| Neo4j `neo4j-admin database dump` | GPL | Physical store archive | GPL; physical, while lpg's dump is logical (new ids, BR-TYPE-002) | **REJECT** |

### 4.5 Type annotations and lexical forms (TS-21)

| Candidate | Licence, commit | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| lpg `temporal::format`, `temporal::parse`; `Point::lexical`, `Point::parse` | MIT (own) | `src/types/temporal.rs:64, 152-240`; `src/types/spatial.rs:153-190` | Point code is test-only; no form for bytes, lists, records, NaN, nodes (§3.1 point 9) | **COPY** (own), extend |
| GQL literal grammar | Apache-2.0, `16ea71b` | `GQL.g4:2500, 2514, 2972, 3188` | Lexical source for bytes, lists, records, strings | **DEPEND** (artefact, already used by TS-19) |
| APOC `FormatUtils.toMap`, `toString` | Apache-2.0, `e52bc20` | `common/.../export/util/FormatUtils.java:66-103`: node, relationship and path maps in CSV results | — | **PORT** |
| Neo4j `PointValue` map keys | GPL | `x`, `y`, `z`, `srid`, `crs`, `longitude`, `latitude`, `height` | GPL | **STUDY** (accepted keys only) |

### 4.6 Transactional import within the memory cap

| Candidate | Licence, commit | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| lpg transaction and graph API | MIT (own) | `create_node`, `create_edge` (`src/storage/graph.rs:307, 492`), `Transaction::commit` (`src/tx/mod.rs:857`), FR-LIM-007 charging (Sprint 3 #49) | Id map charged to the cap (§3.1 point 20) | **COPY** (own) |
| APOC `CsvEntityLoader` batching | Apache-2.0 | Batched commits | Batches violate FR-IMP-004 (TS-26 Rejected) | **REJECT** |

### 4.7 Fast initial load: sort, sorted-run constraint checks, bottom-up trees and indexes, one WAL group

| Candidate | Licence, commit | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| lpg `Sorter`, bulk build, `install_tree`, `UniqueSorted`, `index_keys`, `check_constraint`, `recount` | MIT (own) | §2 table | Several roots and counters in one group; one latch guard; graph catalog version (§2 Changes, §3.1 points 1, 17, 19) | **COPY** (own), extend |
| PostgreSQL `tuplesort.c`, `nbtsort.c` | PostgreSQL licence | Polyphase merge, bottom-up B-tree (already ported in Sprint 1, `sort.rs:15-23`, `bulk.rs:6-14`) | — | **STUDY** (done) |
| `ext-sort` 0.1.6, `extsort` 0.5.0, `external_sort` 0.1.2 | Unlicense; Apache-2.0; MIT | External sort of serialisable items | Temp files through `tempfile`/`std::fs`, not the storage trait; `rayon`; memory by item count, not by the accountant (`ext-sort` `src/sort.rs:22, 121, 170`; `extsort` `src/sorter.rs:48-49`) | **REJECT** |
| Neo4j import tool id mapping | GPL | Sort-based id mapper | GPL; not read in its code (§0) | **STUDY** (not done) |

### 4.8 Backup interlock

| Candidate | Licence | Covers | Verdict |
|---|---|---|---|
| lpg `Core::backup_or_load`, `try_backup_or_load`, `install_step`; backup refusal | MIT (own) | `src/storage/engine.rs:254-296`; `src/ops/backup.rs:258-261`; tests of Sprint 1 | **COPY** (own): one guard for the whole load |

### 4.9 Abort and skip handling, invalid-record report

| Candidate | Licence, commit | Covers | Verdict |
|---|---|---|---|
| Neo4j import options and report file | GPL, `54a7dcf` | `ImportCommand.java:158-159, 463`; `ImportContext.java:114` (`report.json.log`) | **STUDY** (defaults, §3.2 C) |
| Kuzu `WarningContext` | MIT, `89f0263` | Per-warning file and line, stored count limit (`src/include/processor/warning_context.h`) | **STUDY** |
| PostgreSQL `ON_ERROR`, `REJECT_LIMIT`, `LOG_VERBOSITY` | PostgreSQL licence | `copy.sgml:444-527` | **STUDY** |

### 4.10 Round-trip property tests

| Candidate | Licence | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|
| lpg `random_value` | MIT (own) | `src/types/tests.rs:351` and its record round trip (`:196`) | Every property and result type; mixed properties (task #54) | **COPY** (own) |
| `proptest` 1.12.0, `quickcheck` 1.1.0 | MIT OR Apache-2.0; Unlicense OR MIT | Generators with shrinking | A new dev dependency; lpg's suites use seeded `rand` workloads with `LPG_SEED` (TS-30 R5; `src/tx/tests/elle.rs:8`, `src/suites/fault.rs:41`); Turso uses both in places (`core/vector/operations/distance_l2.rs:158`, `testing/differential-oracle`) | **REJECT** as dependency; **STUDY** proptest's float edge values |
| `arbitrary` 1.5.0 | MIT OR Apache-2.0 | Structured fuzz input | Fuzz targets take bytes (`src/fuzz.rs`) | **REJECT** |

### 4.11 External GraphML reader and the Neo4j CSV check

| Candidate | Licence, version | Covers | Verdict |
|---|---|---|---|
| NetworkX | BSD-3-Clause, 3.7 | A widely used GraphML reader | **DEPEND** (lab tool only, `/tmp/lpg-lab-venv`) |
| xmlschema (+ elementpath) | MIT, 4.3.2 (5.1.4) | XSD 1.0 validation against the GraphML schema | **DEPEND** (lab tool only); `xmllint` is absent and needs root (`libxml2-utils`) |
| Neo4j `neo4j-admin database import full` | GPL, image `neo4j:2026.09.0-community` (`dd2aca3682c1`) | The authority on the header rules, run on lpg's CSV | **DEPEND** (lab tool only, run, never copied) |

### 4.12 Dataset generators

| Candidate | Licence, version | Covers | Gap | Verdict |
|---|---|---|---|---|
| lpg M-LAYOUT generator | MIT (own) | `src/lab/generator.rs` (Zipf and uniform adjacency keys) | Labels, edge types and properties (task #54) | **COPY** (own), extend |
| LDBC SNB datagen | Apache-2.0 (GitHub API); release v0.5.1 (2022-11-03); image `ldbc/datagen-standalone:0.5.1-19-829b7a04-2.12_spark3.2` (2024-03-23, pulled, `f6f3c449a392`) | SNB data | Loads only through §3.2 item A option (a) or a Sprint 6 converter; scale runs are timing (deferred) | **DEPEND** (lab tool) when item A is (a) |
| ldbc-snb-impls Neo4j headers | Apache-2.0, `11db98c` | `cypher/scripts/headers.txt`, `convert-csvs.sh` | Header set lpg must read for LDBC | **STUDY** |

### 4.13 Summary of verdicts

| Component | Main source | Verdict |
|---|---|---|
| CSV reader and writer | `csv` 1.4.0; APOC header grammar | DEPEND; PORT |
| Neo4j header rules | Neo4j import tool and `PointValue` | STUDY (GPL) |
| JSON Lines | `serde_json` 1.0.154 `float_roundtrip`; APOC JSON layout | DEPEND; PORT |
| GraphML | `quick-xml` 0.42.0; NetworkX reader; APOC layout | DEPEND; PORT; PORT |
| Native dump | lpg record encoding and backup manifest | COPY (own) |
| Lexical forms and type names | lpg temporal and point code; `GQL.g4` literals; APOC entity maps | COPY (own); artefact; PORT |
| Transactional import | lpg transaction API | COPY (own) |
| Fast initial load | lpg sort, bulk build, constraint and index code | COPY (own), extend |
| Backup interlock | lpg latches | COPY (own) |
| Invalid-record report | Neo4j report file; Kuzu warnings; PostgreSQL `ON_ERROR` | STUDY |
| Round-trip property tests | lpg `random_value`, `LPG_SEED` | COPY (own) |
| External GraphML reader | NetworkX 3.7, xmlschema 4.3.2 | lab tools |
| Neo4j CSV check | `neo4j:2026.09.0-community` | lab tool |
| Datasets | lpg M-LAYOUT generator; LDBC datagen image | COPY (own); lab tool |

## 5. Task decomposition

Seven tasks. Ids #54–#60 are provisional: `rmp` assigns them (highest existing id is #53). Every field is Markdown with `*` bullets; §8 checks lengths and the "no line starting with `- `" rule. Every test that writes graph data uses `/tmp/lpgram/<run>/` (`graph_dir`, `src/test_support/mod.rs:91`), deleted at its end; tests that need real `fsync` use `test_dirs` (`LPG_LAB_EXT4`, `LPG_LAB_XFS`).

**Grouping (CLAUDE.md "Synergy and convergence"):** the laboratory task holds every instrument, as #36 did in Sprint 3. All specification gaps and the four user questions are one task, because all block the same implementation tasks. The lexical forms are one table for three formats (comment #15) and one task, checkable without any file format. The import framework ships with the native dump, the format that needs no annotation, so the framework (scopes, matrix, abort and skip, report, new identifiers, memory cap, authorisation) is proved end to end before the text formats. The fast load is one task with its interlock, fault matrix and concurrent readers, which all exercise the same steps. The three text formats are one task with one round-trip suite over all four formats and both import modes (comment #15), run once on the final readers and writers. The exit task re-runs everything.

**Common method in every implementation task** (stated in each `-tr` as "Method"): study the cited references for each critical decision and record a `DECISION` comment with the evidence; record each copy or port (project, path, commit, licence) in the file header and in a `NOTE` on the task; measure the task's tests with `cargo nextest` and remove waste without weakening assertions; graph data only in `/tmp/lpgram/<run>/`, never for tests that depend on real flushes. No latency or throughput validation (UPDATE 56).

---

### #54 CHORE, priority 9, depends on: none

**Title:** Set up the Sprint 4 laboratory: round-trip instruments and external readers

**-fr**
```text
**Every Sprint 4 import and export claim must be checked by an instrument, not by reasoning.** Before any importer exists, the laboratory needs the latest toolchain and tools, a generator of values of every type, external readers that judge lpg's files (Neo4j's import tool for the CSV header rules, NetworkX and the GraphML schema for GraphML), byte counters for temporary files written through the storage trait, and a synthetic graph generator with properties (sprint DECISION 17).

* Correctness only: `fio` ceilings, `iostat -x` and `pidstat -d` fractions, throughput against M-LAYOUT and the 1 % and 10 % cache runs are deferred to Sprint 6 (sprint UPDATE 56).
```

**-tr**
```text
* **Toolchain:** `rustup self update`, `rustup update stable nightly`, keep 1.89 (MSRV, TS-29); `cargo install --locked <crate>@<max_stable_version>` for `cargo-nextest`, `cargo-deny`, `cargo-llvm-cov`, `cargo-fuzz` when crates.io lists a newer release; record `heaptrack`, `fio`, `iostat`, `pidstat` (fio-3.43 and sysstat v12.8.0 in `~/.local/bin`, installed by task #4, not run before Sprint 6) and `docker` versions.
* **Value generator:** move `random_value` (`src/types/tests.rs:351`, own) to `src/test_support/values.rs`; cover every type of TS-21 Runtime value and IV011, with NaN, ±infinity, −0.0, subnormals, `i64::MIN` and `i64::MAX`, empty strings, U+0000–U+001F, U+FFFE, U+FFFF, `\r`, named and offset-only zoned values, both durations, the four SRIDs, nested lists and records; a mode that yields mixed-type properties. Seeded by `LPG_SEED` (`src/test_support/mod.rs:47`). No new dependency (plan §4.10).
* **Graph generator:** extend `src/lab/generator.rs` (own, M-LAYOUT port) with labels, edge types and generated properties.
* **Temporary-file bytes:** byte counters per call kind in `FaultStorage` (`src/storage/fault.rs:553`), readable by a test.
* **External readers**, outside the repository tree of `lpg` and its dependency tree: `/tmp/lpg-lab-venv` with NetworkX 3.7, xmlschema 4.3.2 and elementpath 5.1.4 installed from wheels (the system Python has no `ensurepip`); `docs/evidence/lab-sprint-4/graphml_check.py`, which reads a file with NetworkX and validates it against the GraphML 1.0 schema fetched to `/tmp` with the SHA-256 values of plan §9; `docs/evidence/lab-sprint-4/neo4j_import_check.sh`, which runs `neo4j-admin database import full` of `neo4j:2026.09.0-community` on a directory of CSV files and prints the counts and the values a `cypher-shell` query returns. Neo4j is GPL: run, never copied.
* **LDBC:** record the datagen image of plan §9; generate scale factor 0.003 only if the user chose plan §3.2 item A option (a).
* **Record:** `docs/evidence/lab-sprint-4/report.md` with Environment and Tools (name, version, command).
* Out of scope: running `fio`, `iostat`, `pidstat`; flame graphs; any timing.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* `rustup check` reports no update; each cargo tool's `--version` equals its crates.io `max_stable_version`; the report lists every tool with version and command.
* With one fixed seed, 10 000 generated values include every type of the plan §4.5 scope, NaN, ±infinity, −0.0 and one property with two types; two runs with that seed are equal.
* `graphml_check.py` on the three probe files of plan Appendix A reports: NetworkX reads all three; the schema rejects the first two and accepts the third.
* `neo4j_import_check.sh` on the CSV probe of plan Appendix A reports 2 nodes, 1 relationship, SRIDs 7203 and 4979, `NaN` and `Infinity`.
* A test reads the bytes a spilling `Sorter` wrote and read through the storage trait: both non-zero, the written bytes equal to the sum of its run files' lengths.
* `cargo deny check licenses` and `cargo +1.89 build` exit 0; `cargo tree -p lpg -e normal` is unchanged by this task.
```

---

### #55 TASK, priority 9, depends on: none

**Title:** Decide and record the Sprint 4 specification gaps in TS-21, TS-26 and TS-31

**-fr**
```text
**Each Sprint 4 implementation task must start from a recorded decision, not from a choice made while coding.** Planning found 23 gaps decidable from references (plan §3.1) and four that need the user (plan §3.2). This task obtains the user's choices, records every decision with its evidence in the technical specification (and in the functional specification where a user choice changes a requirement), and updates the unverified points this sprint reaches (plan §1.3).

This task blocks every implementation task (#56 to #59).
```

**-tr**
```text
Owners: `technical-writer` for `docs/technical-spec/`, `specification-manager` for `docs/functional-spec/`.

1. **Ask the user** items A–D of plan §3.2, each with its options, recommended first; quote the answer in a `DECISION` comment per item.
2. **TS-26:** record plan §3.1 points 1–7 and 10–23, each with its reference as the plan cites it; the user's choices A–D.
3. **TS-21:** the lexical-form and type-name table of plan §3.1 point 9, one row per type of Runtime value, with the GQL.g4 rule or source of each form.
4. **TS-31:** `serde_json` 1.0.154 with feature `float_roundtrip` and its reason (point 8); remove the `serde` row; list the transitive crates measured in plan §0 with their licences.
5. **Unverified points** (`quality-and-testing.md`): point 12 with the planning evidence (Neo4j source lines, the Neo4j run, NetworkX, the schema result) and the item B choice; points 4 and 13 split as plan §1.3.
6. **Functional specification:** only where choice A, B, C or D changes FR-IMP-001..006 or UC-OPS-003; otherwise no change.
* Out of scope: code; decisions outside plan §3.
* Method: reference study per decision as `DECISION`.
```

**-ac**
```text
* Each of the 23 points of plan §3.1 and each of items A–D is stated in TS-21, TS-26 or TS-31 with its source; a reviewer finds each by its point number in the task's `NOTE` index (point → file and line).
* Four `DECISION` comments quote the user's answers to items A–D.
* TS-31 has no `serde` row and names `float_roundtrip`; `cargo deny check licenses` on a probe crate with the TS-31 Sprint 4 crates exits 0 (result in the `NOTE`).
* `quality-and-testing.md` point 12 cites the Neo4j source lines, the measured run and the schema result; points 4 and 13 state the Sprint 6 part.
* No line of the changed files contains `TBD`, `etc.` or `as needed`.
```

---

### #56 TASK, priority 8, depends on: #54, #55

**Title:** Implement lexical forms and lpg type names for every value type (TS-21)

**-fr**
```text
**One table must turn every lpg value into text and back without loss, so that CSV, JSON Lines and GraphML annotate the same way** (FR-IMP-006; sprint DECISION 15: "the lexical forms of TS-21 and one type-annotation table serve the three text formats"). Today only temporal values and, in tests, points have lexical forms; byte strings, lists, records, non-finite floats, escaped strings and query-result nodes, edges and paths have none.
```

**-tr**
```text
* In `src/types/`: `lexical(&Value) -> String` and `parse(type, &str) -> Option<Value>` for every type of TS-21 Runtime value, as the TS-21 table of task #55 fixes them; `type_name(&Value)` and its inverse for the `lpg:<type name>` and `"$lpg_type"` names (IV008 names).
* Copy sources: `temporal::format`, `temporal::parse` (`src/types/temporal.rs:64, 152-240`, own); `Point::lexical`, `Point::parse` (`src/types/spatial.rs:153-190`, own), out of `#[cfg(test)]`; literal forms of `GQL.g4` @ `16ea71b` (`BYTE_STRING_LITERAL` 3188, `listValueConstructorByEnumeration` 2500, `recordConstructor` 2514, `characterStringLiteral` 2972); node, edge and path maps ported from APOC `FormatUtils.toMap` (`common/src/main/java/apoc/export/util/FormatUtils.java:66-103` @ `e52bc20`, Apache-2.0, notice kept).
* Floats: Rust shortest round-trip text; `NaN`, `Infinity`, `-Infinity` (plan §3.1 point 2).
* A fuzz target `lexical_parse` in `fuzz/`: arbitrary type and text, never panics, and every accepted text re-formats and re-parses to an equal value.
* Out of scope: file formats, I/O, CSV header names (task #59).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* For 100 000 values of the task #54 generator (fixed seed, then `LPG_SEED`), `parse(type_name(v), lexical(v))` equals `v` under bit equality for floats other than NaN, and is NaN for NaN.
* Each example row of the TS-21 table of task #55 is a unit test that formats and parses its text.
* `cargo +nightly fuzz run lexical_parse -- -runs=1000000` exits 0.
* `cargo llvm-cov nextest` reports 100 % of the functions of the new lexical code, and the report lists each uncovered region with its reason.
* `cargo deny check licenses`, `cargo +1.89 build` and `cargo nextest run` exit 0.
```

---

### #57 TASK, priority 8, depends on: #54, #55

**Title:** Implement transactional import, export scopes and the native dump format

**-fr**
```text
**The application must be able to export a database or a graph to a native dump and import it back as one transaction, with new identifiers, abort or skip of invalid records and a report of each, within the memory cap** (FR-IMP-001 to FR-IMP-005, FR-LIM-007, BR-TYPE-002 import part, UC-OPS-003, UC-OPS-004).

This task builds the import and export framework that the text formats (#59) and the fast load (#58) reuse: API entry points, the format and scope matrix, the record model, storage-trait file adapters, the report and authorisation.
```

**-tr**
```text
* **API:** session methods to export (scope: database, graph, query result; format) and import (mode: transactional or fast load; on invalid record: abort, the default, or skip; report as task #55 item C fixes); excluded format and scope pairs fail before any write (FR-IMP-003 matrix); authorisation as plan §3.1 point 15.
* **Record model** in `src/ops/bulk/`: node (input id, id space, labels, properties) and edge (input id, start, end, type, properties) records, read in two passes when a format needs it (point 21); tokens created before writes (point 18).
* **Files:** `io::Read`, `io::BufRead`, `io::Write` adapters over `StorageFile` (`interfaces.md:93`); export file commit by `.tmp`, sync, rename, `sync_dir`; fail if a target exists; remove partial files on failure (point 16).
* **Transactional import:** one transaction through `create_node` and `create_edge` (`src/storage/graph.rs:307, 492`); the id map and in-memory report entries charged to the cap (point 20); invalid records as point 12; new identifiers (FR-IMP-005).
* **Export reader:** one read transaction per export; database scope as point 11.
* **Native dump:** layout of point 10; record encoding `src/types/record.rs:271, 545` and the CRC-32C manifest pattern `src/ops/backup.rs:132-160` (own); order from Turso `cli/app.rs:1843-1874` (studied); database-scope import as item D.
* **Fuzz:** a target `dump_read`: arbitrary bytes never panic and fail with an error.
* Out of scope: CSV, JSON Lines, GraphML (#59); fast load (#58).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* A database with two graphs, an index and a constraint, exported to a dump and imported, has the same nodes, edges, labels and property values per graph (bit-equal floats), new element identifiers, and the definitions as item D fixes.
* A graph-scope dump holds exactly that graph's elements; a query-result export to the dump and a database export to CSV fail with an error and write no file.
* An input with one invalid record: with the default the import adds nothing and the report names the record; with skip the other records are imported and the report names it (FR-IMP-004).
* A dump truncated at every block boundary and with one flipped byte per block fails with an error and adds nothing.
* An import needing more memory than the cap fails with a GQLSTATUS error and the graph is unchanged (FR-IMP-002, FR-LIM-007); the accountant's peak never exceeds the cap; `heaptrack` peak heap of the test is in the report.
* ENOSPC at each write of an export (`FaultStorage::fail_nth`) fails it and leaves no target file.
* `cargo +nightly fuzz run dump_read -- -runs=1000000` exits 0; `cargo llvm-cov nextest` lists every uncovered region of `src/ops/bulk/` with its reason.
```

---

### #58 TASK, priority 8, depends on: #55, #57

**Title:** Implement the fast initial load with its backup interlock and fault matrix

**-fr**
```text
**The application must be able to load a large dataset into an empty graph without random I/O: sorted runs, constraint checks on them, bottom-up trees and indexes, and one WAL group that makes the graph visible** (TS-26 fast-load steps 1–7, FR-IMP-002, NFR-PERF-005). While it runs, writers of the graph wait, readers keep reading the empty graph, a backup fails, and a load requested during a backup waits (FR-OPS-001, UC-OPS-001 A2, UC-OPS-003 A2 and A4). A crash or ENOSPC at any step leaves the graph empty (sprint DECISION 16).
```

**-tr**
```text
* **Steps** (TS-26 as task #55 completes it): take the backup-or-load guard (`Core::backup_or_load`, `src/storage/engine.rs:254`) once for the whole load; X lock on `Resource::Graph` (`src/tx/lock.rs:85-87`); fail if the graph is not empty; sorts of plan §3.1 point 19 with `Engine::sorter` (`src/storage/sort.rs:201`); constraint checks with `check_constraint` (`src/index/mod.rs:670`) and `UniqueSorted` (`src/index/build.rs:554-575`); invalid records with abort or skip and the #57 report; bottom-up node, edge and adjacency trees and every defined index into the graph's existing trees (`install_replacing`, `src/storage/btree/bulk.rs:626-635`); `sync_data`; one page-level commit that installs every root, the `Ids` counters (`src/tx/commit.rs:59-66`), the statistics (point 17) and the graph's new catalog version (point 1).
* **Failure:** the builds' pages released at once, at close or at the next open (`bulk.rs:738-809`, own).
* **Fault matrix:** a `FastLoad` workload in `src/suites/fault.rs` beside `BulkLoad` (`:166-183`): crash-point sweep and ENOSPC on every reservation, creation and write; oracle: after reopen the graph is empty or fully loaded, never partial, and the integrity check passes.
* **Concurrency:** readers during a load of a graph with properties and trees of at least three levels (plan §1.3 point 4); a writer of the graph waits; a backup fails with 51N41; a load waits for a running backup.
* Copy sources: the lpg code cited (own). Studied: PostgreSQL `nbtsort.c`, `tuplesort.c`, `copy.sgml:261-278`.
* Out of scope: throughput, M-LAYOUT comparison (Sprint 6).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* A fast load into a graph with one node fails and changes nothing; into an empty graph with a uniqueness, an existence constraint and a property index, the graph equals the input, indexes return the same rows as a scan, and the integrity check passes.
* A duplicate input id, an edge to an unknown node, an edge whose endpoint was skipped and each constraint violation are reported; abort adds nothing; skip loads the rest.
* While a load is paused before its install: a writer of the graph waits until it ends; a backup fails with GQLSTATUS 51N41 and writes nothing to its target; a reader sees the empty graph; a load requested during a backup starts only after the backup ends.
* A snapshot taken before the install reads the graph as empty after it; one taken after reads all of it.
* The fault sweep runs every crash point and every ENOSPC point of the `FastLoad` workload; after each, the reopened graph is empty or complete and passes the integrity check; the counts are in the report.
* The sort's reservation never exceeds the cap; temporary-file bytes written and read (task #54 counters) are in the report.
* `cargo llvm-cov nextest` lists every uncovered region of the load code with its reason.
```

---

### #59 TASK, priority 8, depends on: #56, #57, #58

**Title:** Implement CSV, JSON Lines and GraphML import and export with round trips

**-fr**
```text
**Graphs and query results must leave and re-enter lpg as CSV, JSON Lines and GraphML without loss, and the files must be readable by the tools those formats target** (FR-IMP-001, FR-IMP-003, FR-IMP-006). One round-trip suite proves every format and both import modes (sprint DECISION 15), and closes unverified point 12 on lpg's real files (plan §1.3).
```

**-tr**
```text
* **Dependencies** (TS-31): `csv` 1.4.0, `serde_json` 1.0.154 with `float_roundtrip`, `quick-xml` 0.42.0; nothing else.
* **CSV:** node and edge files (plan §3.1 point 4), annotation per TS-26 with the #56 forms; header grammar ported from APOC `CsvHeaderField` and `CsvLoaderConstants.java:28-36` @ `e52bc20` (Apache-2.0), scope as item A fixes; Neo4j rules studied (`DataFactories.java`, `Extractors.java`, `PointValue.java`; GPL, not copied).
* **JSON Lines:** APOC layout (`JsonFormatSerializer.java:37-116`, port) with tagged objects; one `serde_json::from_slice` per line.
* **GraphML:** APOC layout without attributes (point 6), lpg type attribute and schema as item B fixes; reader ported from NetworkX `readwrite/graphml.py:436-500, 1001-1093` (3.7, BSD-3-Clause, notice kept); XML characters as point 7.
* **Query results:** CSV and JSON Lines one record per row; GraphML the nodes and edges in the result, inside paths and lists too (FR-IMP-003).
* **Round-trip suite:** generated graphs (task #54) through each of the four formats and both modes; FR-IMP-006 acceptance cases as named tests.
* **External checks:** `graphml_check.py` and `neo4j_import_check.sh` (task #54) on lpg's exports.
* **Fuzz:** targets `csv_read`, `jsonl_read`, `graphml_read`.
* Out of scope: options outside item A's choice.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* For 200 generated graphs (fixed seeds, then `LPG_SEED`) with every property type and mixed-type properties, export and import in each of the four formats, by transactional import and by fast load, give the same nodes, edges, labels and property values, with bit-equal floats other than NaN.
* The six FR-IMP-006 acceptance cases (point to GraphML, date to JSON Lines, a non-Neo4j type to CSV, NaN to JSON Lines, integer and string on one property to GraphML and CSV, GraphML read with the namespace ignored) pass as named tests.
* A query result with a node column and an integer column: GraphML holds the node and not the integer; CSV and JSON Lines hold both per row.
* Neo4j 2026.09.0 imports lpg's CSV export of a generated graph without the lpg-only columns and returns its point, temporal and numeric values; NetworkX 3.7 reads the GraphML export; the schema check gives the result item B predicts.
* Each fuzz target runs `-runs=1000000` with exit 0.
* `cargo llvm-cov nextest` lists every uncovered branch of the three formats with its reason; `cargo deny check licenses` and `cargo +1.89 build` exit 0.
```

---

### #60 TASK, priority 7, depends on: #54, #56, #57, #58, #59

**Title:** Verify the Sprint 4 exit condition and write the laboratory report

**-fr**
```text
**The sprint ends only when every correctness condition of exit comment #16 holds on the final code, measured by the laboratory, and the deferred timing items are named** (sprint DECISION 16, UPDATE 56).
```

**-tr**
```text
* Re-run on the final code: the full test suite, the round-trip suite, the fault matrix (`BulkLoad` and `FastLoad`), every fuzz target of #56, #57, #59 with `-runs=1000000`, the external checks of #54.
* Coverage: `cargo llvm-cov nextest --branch` (nightly) over `src/ops/bulk/` and the new `src/types/` code; each uncovered branch listed with its reason.
* Memory: the accountant's peak and `heaptrack` peak heap for transactional import, fast load and each export (correctness: within the cap).
* Report `docs/evidence/lab-sprint-4/report.md`: one section per exit condition with the evidence; temporary-file bytes; unverified points 4, 12, 13 as plan §1.3; the Sprint 6 list (throughput against M-LAYOUT 1.88 M edges/s, `fio`, `iostat`, `pidstat`, 1 % and 10 % cache runs).
* Update `quality-and-testing.md` points 4, 12, 13 with the evidence (owner `technical-writer`).
* `cargo deny check licenses`, `cargo +1.89 build`, `cargo tree -p lpg -e normal` (shows only TS-31 crates).
* Out of scope: new features; timing.
```

**-ac**
```text
* The report has one section per condition of exit #16 without timing (plan §1.2), each citing a test or a run by name and its result.
* All suites and fuzz runs exit 0 on the final commit; the report names the commit.
* The coverage table lists every importer and exporter branch as covered or with its reason.
* The accountant's peak never exceeds the cap in any run; the `heaptrack` peaks are in the report.
* `quality-and-testing.md` states point 12 closed with its evidence, and points 4 and 13 with their Sprint 6 parts.
* `cargo tree -p lpg -e normal` lists no crate outside TS-31.
```

## 6. Execution order and dependency edges

Order for `rmp sprint reorder 4`: **#4, #54, #55, #56, #57, #58, #59, #60.**

| Task | Depends on | Why |
|---|---|---|
| #54 | none | first task after planning (task #4 `-fr`) |
| #55 | none | specification and user questions; runs while #54 runs |
| #56 | #54, #55 | value generator; TS-21 table |
| #57 | #54, #55 | generator, counters; points 10–16, 18, 20–22; items C, D |
| #58 | #55, #57 | points 1, 17, 19; record model, report, options |
| #59 | #56, #57, #58 | lexical forms; framework; the suite runs both import modes |
| #60 | #54, #56, #57, #58, #59 | runs on the final code (#55 reached through #56–#59) |

## 7. Traceability

Every id of scope comment #14, exit comment #16, synergy comment #15, every laboratory item of #17 and every unverified point named in #16 maps to at least one task; no task maps to an item outside them.

| Item | Tasks |
|---|---|
| TS-26 Export layouts | #55, #57 (dump), #59 (CSV, JSON Lines, GraphML) |
| TS-26 Type annotations | #55, #56, #59 |
| TS-26 Transactional import | #57 |
| TS-26 Fast initial load steps 1–7 | #55, #58 |
| TS-26 Backup exclusion (interlock) | #58 |
| TS-21 Lexical forms | #55, #56 |
| FR-IMP-001 | #57, #59, #60 |
| FR-IMP-002 | #57 (transactional), #58 (fast load), #60 |
| FR-IMP-003 | #57, #59, #60 |
| FR-IMP-004 | #57, #58, #60 |
| FR-IMP-005 | #57, #60 |
| FR-IMP-006 | #55, #56, #59, #60 |
| FR-OPS-001 (interlock) | #58, #60 |
| FR-OPS-003 | #57, #58, #59 |
| FR-LIM-007 | #57 |
| BR-TYPE-002 (import part) | #57 |
| UC-OPS-003 | #57, #58 |
| UC-OPS-004 | #57, #59 |
| NFR-PERF-005 | #58 (bottom-up load, no random I/O); measurement deferred (#60 names it) |
| NFR-PERF-002 | #55 (item A), #59 (LDBC CSV subset if item A is (a)); measurement deferred (#60 names it) |
| Unverified point 4 | #58, #60 (correctness part); Sprint 6 (timing) |
| Unverified point 12 | #55, #59, #60 |
| Unverified point 13 | #60 (named as Sprint 6) |
| Exit #16: lossless round trips incl. NaN, mixed, points, temporals | #59, #60 |
| Exit #16: exact scope contents, excluded pairs fail | #57, #59 |
| Exit #16: abort and skip report; new identifiers | #57, #58 |
| Exit #16: fast load fails non-empty, writers wait, backup fails, waits for backup | #58 |
| Exit #16: crash or ENOSPC at each step leaves the graph empty | #58, #60 |
| Exit #16: throughput against M-LAYOUT | deferred to Sprint 6 (UPDATE 56); #60 names it |
| Laboratory #17: toolchain refresh | #4 (done, §9), #54 |
| Laboratory #17: sequential I/O (`iostat`, `pidstat`, `fio`) | #4 and #54 (installed, recorded); runs deferred to Sprint 6 |
| Laboratory #17: external sort memory and temporary-file bytes | #54, #58, #60 |
| Laboratory #17: fault matrix with crash and ENOSPC per step | #58, #60 |
| Laboratory #17: datasets (LDBC datagen, M-LAYOUT generator) | #4 (availability, §9), #54; cache-ratio sizes deferred |
| Laboratory #17: property round trips, external GraphML reader, Neo4j CSV header rules | #54, #59, #60 |
| Laboratory #17: `cargo-llvm-cov` on every importer and exporter branch | #56, #57, #58, #59, #60 |

## 8. Checks run on this document

The `text` blocks of §5 were checked by script after writing (result appended below): character count per field, and no line starting with `- `.

Result (2026-10-11): 21 fields, longest 2 253 characters (limit 4 096), shortest 203, 0 lines starting with `- `; the 7 titles are 66–75 characters, plain text. A search for open-ended wording (`etc.`, `as needed`, `if necessary`, `where appropriate`, `TBD`) found it only in #55's `-ac`, which forbids those words in the specification it changes. The line citations of §2–§4 were re-checked with `grep -n` after writing; 15 were corrected.

## 9. Laboratory tool status

Measured 2026-10-11 after the updates below.

| Tool | Version | Command | State |
|---|---|---|---|
| rustup | 1.29.1 (d95a37b6a 2026-08-13) | `rustup self update` | "rustup unchanged" |
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | `rustup update stable nightly` | unchanged, up to date |
| Rust nightly | 1.101.0-nightly (6f5da406b 2026-10-10) | same | **updated** from 1d81eb4ad of 2026-10-07 |
| Toolchain 1.89 (MSRV) | 1.89.0 (29483883e 2025-08-04) | `rustup check` | up to date |
| `rustup check` | every toolchain and rustup "up to date" | `rustup check` | no update |
| Components (stable) | cargo, clippy, llvm-tools, rust-analyzer, rust-docs, rust-src, rust-std, rustc, rustfmt | `rustup component list --installed` | updated with the toolchain |
| Components (nightly) | as stable, plus miri; no rust-analyzer | same | updated with the toolchain |
| cargo-nextest | 0.9.148 | `cargo install --list` | = crates.io 0.9.148 |
| cargo-deny | 0.20.2 | same | = crates.io 0.20.2 |
| cargo-llvm-cov | 0.9.1 | same | = crates.io 0.9.1 |
| cargo-fuzz | 0.13.2 | same | = crates.io 0.13.2 |
| inferno | 0.12.8 | same | = crates.io 0.12.8; not needed (timing deferred) |
| heaptrack | 1.5.0 | `heaptrack --version` | latest tag `v1.5.0` (2023-09-29, KDE GitLab API) |
| fio | 3.43 | `fio --version` (`~/.local/bin/fio`) | **updated**: built from tag `fio-3.43` (2026-09-23) into `~/.local`; the distribution's 3.41 stays in `/usr/bin`; not run (timing deferred) |
| iostat, pidstat (sysstat) | 12.8.0 | `iostat -V`, `pidstat -V` (`~/.local/bin`) | **updated**: built from tag `v12.8.0`; the distribution's 12.7.7 stays in `/usr/bin`; not run (timing deferred) |
| Python | 3.14.4 | `python3 --version` | system; no `ensurepip` (venv created `--without-pip`) |
| NetworkX | 3.7 (= PyPI latest) | `/tmp/lpg-lab-venv/bin/python -I -c "import networkx; print(networkx.__version__)"` | **installed** from wheel |
| xmlschema, elementpath | 4.3.2, 5.1.4 (= PyPI latest) | same, `xmlschema.__version__` | **installed** from wheels |
| GraphML 1.0 schema | `graphml.xsd` c5d04fb9…af52, `graphml-structure.xsd` 78091238…a04b, `graphml-attributes.xsd` e13e2d5d…36bb, `graphml-parseinfo.xsd` 6d025e0b…92ca (SHA-256) | `curl` from `graphml.graphdrawing.org/xmlns/1.0/`; `sha256sum` | fetched to the session scratchpad; task #54 fetches again and checks these hashes |
| Docker | 27.2.0 | `docker --version` | usable without root |
| Neo4j | 2026.09.0 community, image `dd2aca3682c1` | `docker run --rm neo4j:2026.09.0-community neo4j-admin --version` | **pulled**; latest calendar release on Docker Hub (2026-10-11) |
| LDBC SNB datagen | image `ldbc/datagen-standalone:0.5.1-19-829b7a04-2.12_spark3.2` (`f6f3c449a392`, 2024-03-23); latest release v0.5.1 | `docker images` | **pulled**; available |
| Java | openjdk 25.0.4.1 (2026-08-18) | `java -version` | present (not needed in Sprint 4) |
| perf | 7.0.14 | `perf --version` | not needed (timing deferred) |
| xmllint | absent | `command -v xmllint` | not needed: xmlschema validates; installing it needs root (`libxml2-utils`) |

**Needs root: none.** **Pending on the user: none** (the four specification questions of §3.2 are decisions, not tools).

**RAM space** (`df -h`, `du -sh`): before the planning run `/tmp` used 1.6 G and `/dev/shm` 12 K; after it, 1.7 G and 160 M. This plan made no build and wrote no graph data to `/tmp/lpgram`. The 160 M are `/dev/shm/lpg-target`, created at 17:29:33 with a `flycheck0` directory, the mark of an editor's rust-analyzer check, not of this run; it was left in place because it is a build in use. Left in `/tmp` for later tasks: `/tmp/lpg-lab-venv` (20 M), the clones `/tmp/apoc` (1.8 M) and `/tmp/petgraph-graphml` (220 K), the crate sources in `/tmp/crates-src` (8.8 M).

## 10. rmp operations to enter the plan (for the roadmap-manager, not run here)

1. `rmp task create` × 7 with the titles, types, priorities and `-fr`/`-tr`/`-ac` of §5; then `rmp sprint add-tasks 4 <ids>`.
2. `rmp task add-dep` for each edge of §6, with the real ids.
3. `rmp sprint reorder 4` in the order of §6.
4. On task #4: a `DECISION` with the §7 table (real ids); a `DECISION` with the choices of §3.3, the grouping rationale of §5 and the reuse verdicts of §4.13; a `FINDING` with the 23 points of §3.1 and the four items of §3.2; a `TEST` with the tool versions of §9.

## Appendix A. Probe files and results (2026-10-11)

**GraphML probes** (`t.graphml`; `t2` = `t` without the `labels` and `label` attributes on `<node>`/`<edge>`; `t3` = `t2` without any `lpg:type` attribute):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns" xmlns:lpg="https://crates.io/crates/lpg/graphml">
<key id="labels" for="node" attr.name="labels" attr.type="string"/>
<key id="p" for="node" attr.name="p" attr.type="string" lpg:type="point"/>
<key id="m" for="node" attr.name="m" attr.type="string"/>
<graph id="G" edgedefault="directed">
<node id="n0" labels=":A"><data key="labels">:A</data><data key="p">{srid: 7203, x: 1.0, y: 2.0}</data><data key="m" lpg:type="long">5</data></node>
<node id="n1"><data key="m" lpg:type="string">x</data></node>
<edge id="e0" source="n0" target="n1" label="R"><data key="labels">R</data></edge>
</graph></graphml>
```

Results: NetworkX 3.7 `read_graphml` read all three, values as strings (`'p': '{srid: 7203, x: 1.0, y: 2.0}'`, `'m': '5'`). xmlschema 4.3.2 against `graphml.xsd`: `t` invalid ("'{https://crates.io/crates/lpg/graphml}type' attribute not allowed for element"; "'labels' attribute not allowed for element"), `t2` invalid (the `lpg:type` attributes), `t3` valid.

**CSV probe** (`nodes.csv`, `edges.csv`, imported with `neo4j-admin database import full neo4j --nodes=/import/nodes.csv --relationships=/import/edges.csv`):

```text
:ID,:LABEL,loc:point{srid:7203},loc3:point{srid:4979},d:double
n1,A;B,"{x:1.5,y:2.0}","{longitude:1.0,latitude:2.0,height:3.0}",NaN
n2,A,"{x:-1.0,y:0.25,srid:7203}","{longitude:1.0,latitude:2.0,height:3.0}",Infinity

:START_ID,:END_ID,:TYPE
n1,n2,R
```

Results: "IMPORT DONE … 2 nodes, 1 relationships, 6 properties". `cypher-shell` returned `point({srid:7203, x:-1.0, y:0.25})` and `point({srid:7203, x:1.5, y:2.0})`, `n.loc3.srid` 4979 on both, `n.d` `Infinity` and `NaN`.
