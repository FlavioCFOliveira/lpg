# Sprint 3 plan: GQL engine and async Rust API (draft for task #3)

Prepared 2026-10-08 on branch `feature/3-gql-engine-and-async-rust-api-first-end-to-end-sli` at `b3c249c`. Read-only for the repository: no file other than this one and no `rmp` record was changed. The laboratory tools were updated (§9).

## 0. Basis, method and limits

* **Read:** `CLAUDE.md`, `README.md`; `rmp sprint get 3`; sprint comments #10 (scope), #11 (risks), #12 (exit), #13 (laboratory), #34, #47, #55; task #3; the Sprint 2 plan (`docs/evidence/planning/sprint-2-plan.md`, the model for this file) and the Sprint 2 task list (`rmp sprint tasks 2`).
* **Specification read:** `docs/technical-spec/` (query-engine TS-19/TS-20, interfaces TS-03/TS-23/TS-24 and "Public API shape", quality-and-testing TS-29/TS-30 and the unverified points, dependencies TS-31, values-and-types TS-21, catalog-and-indexes TS-17/TS-18, architecture TS-02, operations headings); `docs/functional-spec/functional-requirements.md`, `non-functional-requirements.md`, `open-questions.md` (OQ-005, OQ-006); `docs/evidence/research/research-03-query-engine.md` (M-QE), `decision-register.md`; `docs/evidence/antlr-exp/`; the ISO inserts in `docs/standards/iso-iec-39075/ed-1/` (read, never edited).
* **Code read:** `src/lib.rs`, `Cargo.toml`, `deny.toml`, `.config/nextest.toml`, and the public items of `src/{tx,catalog,index,types,storage,obs,ops}` (§2).
* **Reference clones made for this plan** (shallow, under `/tmp`; licence read in each repository's licence file, §4): GraphLite `a370a1c`, Grafeo `3c23dbe`, miniGU `fc2ccf5`, drasi-core `261b8d4`, gqlite `e6e9ca7`, frogql `b5dd0f0`, zu `a821319`, cyrs `6a6bb34`, gql-compat `9de1991`, open-cypher `c7a4875`, lance-graph `3014793`, datafusion-sqlparser-rs `14cbf75`, GlueSQL `42a4b41`, Oxigraph `fda6793`, gherkin `4c30e3a`, opengql/grammar `16ea71b` (the pinned commit is the current `main` head), opengql/tck `e0700a7`, openCypher `677cbaf`, Kuzu `89f0263` (sparse: binder, planner, optimizer, processor), DataFusion `54a4bbf` (sparse: `datafusion/physical-plan/src`), grammars-v4 `7df52be` (sparse: `antlr/antlr4`). Reused: `/tmp/turso` @ `8cc059b`, `/tmp/neo4j`, `/tmp/memgraph` (GPL and BSL: study only).
* **Official sources fetched 2026-10-08:** Neo4j Cypher Manual "Supported optional GQL features" (`last-modified` 2026-10-08 10:48 GMT) and "Supported mandatory GQL features"; Neo4j Status Codes "GQLSTATUS error codes" (453 codes); crates.io API; GitHub API (licences, releases); KDE GitLab tags of heaptrack.
* **Measured for this plan:** keyword sets of `GQL.g4` against the ISO BNF insert (§3.1 point 1); GQL.g4 rule names matched by each candidate parser (§4.2); `gherkin` 0.16.0 parsing every TCK feature file (§4.16); SHA-256 of the ISO artefacts vendored by zu and gql-compat against lpg's inserts (§4.17).
* **Disclosure:** one crates.io request of this planning session sent the user's e-mail address in its `User-Agent` header; later requests did not.
* **Unverified (stated plainly):** Memgraph's support of GQL optional features (no Memgraph source or page was read); the Unicode version used by the ANTLR 4.13.3 tool jar for `\p{ID_Start}` (§3.1 point 2); whether `antlr4rust` 0.6.0 and its jar generate a working parser from `GQL.g4` (verified for 0.5.2 only, M-QE §Q1.3); the cause of the `gherkin` failure on `Literals6.feature`; the correctness of every copy or port candidate's code (none was run against lpg's requirements); gql-compat's "placement rule" (§3.2 item A) is that project's reading of ISO, not the Conformance Rules, which are unavailable (OQ-005).

## 1. Requirements digest

Scope ids from sprint comment #10 and exit ids from #12, ranges expanded. FS = `docs/functional-spec/functional-requirements.md` (acceptance on the line after the id), NFS = `non-functional-requirements.md`.

### 1.1 Technical decisions

| Id | Demands (1–2 lines) | Where |
|---|---|---|
| TS-02 | Sync core; async API on the application's tokio runtime; each result batch pulled through `spawn_blocking`; `Database` cheap clone; session "busy" flag; GQL session statements and API calls reach the same functions. | `architecture.md:45-62` |
| TS-17 | Closed graph types checked at the end of each `INSERT`, `SET`, `REMOVE` over the elements written; matching rules provisional (OQ-005). | `catalog-and-indexes.md:32` |
| TS-19 | Hand-written case-insensitive lexer, recursive descent and Pratt parser following `GQL.g4` @ `16ea71b` rule by rule; typed AST with spans; ambiguities resolved explicitly and recorded; syntax errors 42001 with position and expected tokens; parameters never inlined. | `query-engine.md:14-45` |
| TS-20 | Binder (slots, kind, degree, types, path-pattern rules, 3VL, feature gates, impl-defined only after approval); planner (rewrites, index seeks, DP join order ≤ 7 edges then greedy, cost = Σ intermediate cardinalities, 0.01/0.1 defaults); pull, batch of ≤ 1 024 rows, one thread per statement; chunked reservations; one atomic status per transaction; limits at the sink; EXPLAIN; PROFILE decorators. | `query-engine.md:47-87` |
| TS-23 | One dyn-compatible trait with boxed futures: `authenticate`, `impersonate`, `authorise(principal, action, object, owner)`; Neo4j privilege names; one request per (action, object) per statement after planning; `create` asks nothing. | `interfaces.md:142-162` |
| TS-24 | Registry per handle of typed signatures (mode `ReadOnly`/`ReadWrite`); synchronous iterator procedures with a context; three read-only enforcements; returned rows charged; four `show_*` procedures; own metrics sink; `tracing` events with query text and `tx_metadata`, never parameter values. | `interfaces.md:164-186` |
| TS-30 | ANTLR oracle crate (test-only); differential parsing on samples, every test text and generated sentences; Gherkin-style runner for opengql/tck and applicable openCypher TCK; each scenario names its clause or feature code. | `quality-and-testing.md:23-33` |
| interfaces.md | `Database`, `Session`, `Transaction`, `ResultStream`, `Error` (GQLSTATUS, message, cause); GQLSTATUS from the ISO insert, else Neo4j. | `interfaces.md:100-116` |

### 1.2 Functional and non-functional requirements

| Id | Demands | Acceptance (FS line) | Sprint 3 form |
|---|---|---|---|
| FR-API-001 | In-process Rust API runs GQL. | result or GQLSTATUS error, no Bolt (FS:22) | API |
| FR-API-002 | Sessions, transactions, catalog via API. | new session sees committed graph (FS:24) | API + catalog statements |
| FR-API-003 | Bolt optional. | build without Bolt, every API call works (FS:26) | API, no `bolt` feature exists yet |
| FR-API-004 | Index and constraint management through the API only. | index exists; non-standard syntax → syntax error (FS:28) | management API + parser |
| FR-API-005 | Register procedures and functions. | `CALL` of registered name succeeds (FS:30) | registry |
| FR-API-006 | `CALL` executes registered routines. | result contains the row (FS:32) | registry |
| FR-API-007 | Limits per session and transaction; time from transaction start. | 1 s limit: 0.6 s + > 0.4 s fails (FS:34) | limits |
| FR-API-008 | Cooperative cancellation (Rust part). | statement stops, GQLSTATUS (FS:36); Bolt part Sprint 6 | limits |
| FR-API-009 | Cancel fails statement and aborts transaction. | written node never visible (FS:38) | limits |
| FR-API-010 | Streaming results. | first of 10^6 rows before the rest is produced (FS:40) | executor + API |
| FR-DB-002 | Several databases per process. | independent handles (FS:47) | API |
| FR-DB-008 | Close waits up to a timeout (10 s default), then aborts. | (FS:60) | API |
| FR-SES-001..004 | Trusted principal; home schema and graph; UTC; second concurrent request refused. | (FS:67, 69, 71, 73) | API |
| FR-AUTH-001/002, BR-AUTH-001 | Ask before every action; owner in each request; no permission syntax. | (FS:78-79, 81, 83) | authorisation |
| FR-GQL-001/002 | Conformance in phases; published list of supported feature ids. | list + tests per feature (FS:88, 90) | feature set, runner |
| FR-GQL-004 | GG01, GG02. | closed type rejects unknown label; open accepts (FS:94) | catalog statements |
| FR-GQL-005 | Session and transaction statements in text and API, same semantics. | (FS:96) | API |
| BR-GQL-001 | Standard syntax only; 42001 otherwise. | (FS:98) | parser |
| BR-GQL-002 | Each implementation-defined item approved by the user. | (FS:109-110) | approval task |
| FR-CAT-001 | Full catalog listed by procedure (exit #12). | (FS:117) | statements + `show_catalog` |
| FR-CAT-003 | Queries spanning several graphs. | one query reads two graphs (FS:121) | catalog statements |
| FR-CAT-005 | Directories through the API only. | listed; no syntax or procedure creates one (FS:125) | management API |
| FR-IDX-001 | Five index kinds (exit #12). | listing procedure lists five (FS:130) | management API + `show_indexes` |
| FR-TYPE-005 | Point index serves distance queries (exit #12). | indexed = unindexed (FS:186) | depends on OQ-006 (§3.2 item C) |
| FR-PROC-001..004 | Calls through ABS-005; modes; per-handle, not persisted; four `show_*`. | (FS:146, 148, 150, 152-157) | registry |
| FR-LIM-001/002/006 | Limit failure aborts; row and byte limits; procedure memory outside cap, rows inside. | (FS:235, 237, 245) | limits, registry |
| FR-LIM-003/005/007 | Cap, old versions, writes of one transaction, through statements (exit #12). | (FS:239, 243, 247) | limits |
| FR-OBS-001 (Rust part) | Metric categories. | ≥ 1 metric per category (FS:369) | metrics |
| FR-OBS-002..006 | Query text without parameters; list and terminate transactions; EXPLAIN; PROFILE. | (FS:371, 373, 375, 377, 379) | metrics, limits, planner, executor |
| FR-OPS-001/002/008/010, FR-REC-004/010 | Backup, restore, integrity check through the API (exit #12). | (FS:322, 324, 336, 340, 258, 270) | API |
| FR-OPS-005/006 | `tracing` events; metrics sink. | (FS:330, 332) | metrics |
| FR-TX-004, BR-TX-001 (Rust part) | Snapshot isolation per transaction via the API; no automatic retry. | (FS:216, 226) | API |
| NFR-VER-001/002 | Scenarios traced to clauses and feature codes; `GQL.g4` the syntax authority and the ANTLR oracle. | NFS:71, 73 | runner, oracle |
| NFR-PERF-002 | Micro-workloads defined (exit #12). Measurement is timing: deferred (UPDATE 55). | NFS:18 | final task |
| NFR-PERF-007 | No intra-query parallelism. | NFS:26 | executor |
| NFR-PLAT-003 | Async API on tokio. | NFS:35 | API |

**Optional features named by the scope:** GG01, GG02 (FR-GQL-004), GP18 (FR-CAT-004), GV39, GV40, GV41 (FR-TYPE-001). **Risks (comment #11):** OQ-005, BR-GQL-002, FR-GQL-006, OQ-006.

**Exit (#12) without timing:** parser = oracle on samples, test texts and generated sentences; every approved scenario passes and names its clause or feature; the acceptance criteria above pass through the API; at most one thread per query; micro-workloads defined; unverified point 1 restated. **Timing deferred to Sprint 6 (UPDATE 55):** flame graphs per phase, parse time against M-PARSE, `spawn_blocking` hand-off cost against M-TOKIO, tokio runtime metrics, cancellation latency, the measurement half of NFR-PERF-002 and the closing of unverified point 8 (parse speed), which is a timing measurement (§3.2 item E).

## 2. Code reconnaissance

Sprint 1 and 2 code: 51 082 lines of Rust in `src/` (`find src -name '*.rs' | xargs wc -l`). No query, API or authorisation module exists. `tokio` and `tracing` are in TS-31 but not yet in `Cargo.toml` (runtime dependencies today: `rustix`, `crc32c`, `rand`, `jiff`; `cargo tree -e normal`).

### What Sprint 3 builds on

| Piece | Where | Use in Sprint 3 |
|---|---|---|
| Transactions: `Engine::begin(isolation)`, `get`, `scan_prefix`, `put`, `delete`, `commit`, `abort`, `abort_with` | `src/tx/mod.rs:1-43, 158-230` | statement execution; snapshot isolation option (FR-TX-004) |
| Catalog: objects, tokens, owners, `resolve`, `create_directory/schema/graph_type/graph`, `drop_object`, `path`, `children` | `src/catalog/mod.rs:874-1211, 1332` | binder name resolution, catalog statements, `show_catalog`, directory API |
| Statistics: counts per label and `(label, type, direction)`, per index | `src/catalog/stats.rs:48-302` | planner cost (TS-20) |
| Graph store: `create_node`, `set_property`, `delete_node`, `create_edge`, `expand`, `check_integrity` | `src/storage/graph.rs:307-607, 1062` | operators, `check_integrity` API (FR-REC-010) |
| Indexes and constraints: `index_scan`, `index_within`, `schema_defs`, `drop_schema`, `PropertyType`/`TypeSet` | `src/index/mod.rs:97-201, 1124-1406` | index seeks, management API, `show_indexes`, `show_constraints` |
| Values: 24-byte `Value`, `group`, `total_cmp` (approved order), key encoding | `src/types/mod.rs:40-414`, `src/types/key.rs:134-227` | evaluation, `ORDER BY`, grouping |
| Memory accountant: `try_reserve`, `Reservation::split/shrink/absorb` | `src/storage/memory.rs:145-186` | chunked operator reservations (TS-20) |
| Metrics sink and names (transactions, log, checkpointing, cache, store size) | `src/obs/mod.rs:17-101` | add query execution, data counts, indexes |
| Backup, restore, migrate | `src/ops/backup.rs:251`, `src/ops/restore.rs:107`, `src/ops/migrate.rs:122` | API wrappers (FR-OPS-001/002/010) |
| Error kinds and the two GQLSTATUS codes so far | `src/storage/error.rs:9-104, 217-223` | full mapping (task #40) |
| Test instruments: `/tmp/lpgram` dirs, `allocation-counter`, stress profile, loom/shuttle facade | `src/suites/mod.rs`, `Cargo.toml:21-24`, `.config/nextest.toml`, `src/sync.rs` | all tasks |

### Changes to existing code

* **Owned transactions for the async boundary.** `Transaction<'e>` holds `&'e Engine` and `Registered<'e>` (`src/tx/mod.rs:108-114`); `spawn_blocking` needs `'static` values. `Engine::handle(Arc<Core>)` already gives an owned, non-owning handle (`src/storage/engine.rs:771-786`). §3.1 point 4.
* **Dead-code allowances** in `src/lib.rs` say "first non-test callers come with Sprint 3" for `catalog`, `index`, `tx` and `types`; they are removed when the API calls them.
* **Error kinds** gain GQLSTATUS codes for every kind (`src/storage/error.rs:217-223` covers two).

## 3. Specification gaps

### 3.1 Decidable from references (task #39 records each decision)

| # | Gap (cited) | Proposed decision | Reference |
|---|---|---|---|
| 1 | Keyword sets of `GQL.g4` (218 reserved, 39 pre-reserved, 77 non-reserved; `GQL.g4:3276-3661`) differ from the ISO BNF insert as zu extracted it (222, 40, 48): the BNF also reserves `FALSE`, `IMPLIES`, `SKIP`, `TRUE`, `UNKNOWN`, pre-reserves `WHITESPACE`, and has the non-reserved `TEMP` with `[ TEMP ] <element variable>` (`ISO_IEC_39075(en).bnf.txt:754`). Measured by script, 2026-10-08. | Follow `GQL.g4` (NFR-VER-002); record each difference in TS-19 as provisional (OQ-005). | NFR-VER-002; zu `crates/zu-common/src/keywords/generated.rs` (generated from the same BNF, byte-identical, §4.17). |
| 2 | Identifier characters: `IDENTIFIER_START` = `\p{ID_Start}` or `Pc`, `IDENTIFIER_EXTEND` = `\p{ID_Continue}` (`GQL.g4:3612-3623`). `unicode-ident` implements XID_Start/XID_Continue, which differ from ID_Start/ID_Continue; `char::is_alphabetic` differs too. | Tables generated from the Unicode Character Database of the Unicode version the ANTLR jar uses, with a drift test; record that version. | zu generates Unicode tables with a drift test (`crates/zu-common/tests/unicode_tables.rs`); UAX #31. Unicode version of the jar: unverified. |
| 3 | Grammar ambiguities: `statement`, `labelAndPropertySetSpecification`, `insertElementPatternFiller` (M-QE §Q1.3). | Resolve each to the lowest-numbered alternative, which is the oracle's rule; record in TS-19. | antlr4rust 0.5.2 `src/prediction_mode.rs:185-196` (`get_single_viable_alt` returns the minimum alternative), `src/parser_atn_simulator.rs:497-499`. |
| 4 | Async ownership (§2). | A transaction owns an `Engine::handle(Arc<Core>)`; snapshot registrations hold owned references; no self-referential crate. | Turso `bindings/rust` keeps owned `Arc` handles (`src/connection.rs`, `src/rows.rs` @ `8cc059b`). |
| 5 | GQLSTATUS of each internal error kind and of the API failures (busy session, terminated, timed out, memory, constraint, feature not supported). | ISO conditions insert where it has the condition; else the Neo4j GQLSTATUS page (e.g. `25N03` "concurrent access violation", `51N36` "out of memory" were found there). | `interfaces.md:116`; Neo4j Status Codes "GQLSTATUS error codes" (fetched 2026-10-08). |
| 6 | Procedure names and functions. `procedureReference` is `catalogProcedureParentAndName` or a reference parameter (`GQL.g4:1458-1461`), but no catalog object is a procedure; `GQL.g4` has no general function invocation in value expressions (`valueExpressionPrimary`, `GQL.g4:2220-2235`), so registered functions can be invoked only with `CALL`. | Unqualified names resolve in one built-in namespace per database handle; a registered function is a `CALL`-able routine that returns one row; record as provisional. | FR-API-006 wording ("with the standard `CALL` statement"); gql-compat README (`9de1991`) on GP04/GP14/GP15 waiting on a procedure name the standard never supplies. |
| 7 | TS-20 calls DataFusion "push-based" (`query-engine.md:81`); its operators return a pulled async stream. | Correct the wording; the decision (study only) is unchanged. | DataFusion `datafusion/physical-plan/src/execution_plan.rs` (`execute` returns `SendableRecordBatchStream`) @ `54a4bbf`. |
| 8 | Chunk size of operator reservations (TS-20:66). | Decide from the references; fixed constant. | Neo4j `LocalMemoryTracker` grab size (GPL, study; line to record); DataFusion `MemoryReservation` (to record). |
| 9 | Interval K of the cancellation check in long loops (TS-20:69). | Every K iterations with K fixed. | Memgraph checks every 20th pull (`src/query/plan/operator.cpp:510-528`, cited in TS-20). |
| 10 | Limits: which live on `Session` and which on `Transaction` (FR-API-007); defaults; what "bytes" counts for FR-LIM-002. | No default limit; a transaction inherits its session's limits; bytes = the record encoding size of TS-21 per value. | Neo4j `db.transaction.timeout` default (to record); TS-21 record encoding. |
| 11 | `ResultStream::summary()` content. | Neo4j's query counters (nodes, relationships, labels, properties created or removed) and the completion GQLSTATUS (`00000` or `02000`). | Neo4j driver `ResultSummary` (to record); ISO conditions insert class 02 "no data". |
| 12 | EXPLAIN and PROFILE result types and operator names. | A tree of operators with Neo4j operator names where one exists, estimated rows (EXPLAIN), and the TS-20 counters (PROFILE). | Neo4j "Execution plans" (M-QE §Q7.1). |
| 13 | Metric names for query execution, data counts and indexes. | Neo4j metric names without the database prefix, as `src/obs/mod.rs:1-9` does. | Neo4j Operations Manual "Metrics reference". |
| 14 | `tracing` events: level, target, fields. | One span per request with the query text and `tx_metadata`; events at `debug` and `info`; no field holds a parameter value. | Neo4j query log fields; FR-OBS-002. |
| 15 | Session and transaction statements in text against the API (FR-GQL-005): `START TRANSACTION` while one is open, `COMMIT` with none, `SESSION CLOSE`. | Neo4j explicit-transaction semantics; each case fails with its ISO class-25 code where one exists. | ISO conditions insert class 25; Neo4j status codes. |

### 3.2 Needs a user decision (asked in task #37, item E in task #39)

* **A. Provisional feature set (OQ-005).** No Conformance Rules exist to the project. Proposal: the mandatory surface is every `GQL.g4` construct that no features-insert code covers (gql-compat states this placement rule in its README; it is not ISO text), plus the six named optional features, plus the optional features that in-scope acceptance criteria require but comment #10 does not name: **GD01** (FR-GQL-004, FR-API-009, FR-LIM-007 insert and update data), **GT01** (FR-GQL-005), **GP04** (FR-API-006, FR-PROC-004 named `CALL`), **GC01** and **GC04** and the graph-type creation construct (FR-API-002, FR-CAT-001), **GQ01** and **GT03** (FR-CAT-003). Options: (a, recommended) that set; (b) the six named features only, removing the acceptance criteria that need the others from Sprint 3; (c) a larger set the user names. Path quantifiers, path modes beyond WALK and path selectors (G010–G020, G035, G036, G060, G061), `LIMIT`/`OFFSET` (GQ12, GQ13), `CAST` (GA05) are optional features and fall outside the recommended set; if the user adds any, a task for them is created then.
* **B. FR-GQL-006 order conflict.** Neo4j lists 63 supported optional features (fetched 2026-10-08); GG02, GP18, GV41, GC01, GC04 and GT03 are not among them, while FR-GQL-004, FR-CAT-004, FR-TYPE-001, FR-API-002, FR-CAT-001 and FR-CAT-003 need them. Delivering them now precedes Neo4j-supported features (for example GQ13). Memgraph's list: unverified. Options: (a, recommended) a requirement that names or needs a feature overrides the FR-GQL-006 order for that feature; record the exception in FR-GQL-006; (b) deliver every Neo4j- or Memgraph-supported feature first; (c) defer the conflicting requirements.
* **C. OQ-006, points without new syntax.** `GQL.g4` has no point type, no point literal and no general function invocation (§3.1 point 6). Options: (a, recommended) points enter and leave only as values (parameters, properties, results); constructors, distance, and a distance-within search that uses the point index (FR-TYPE-005) are built-in procedures invoked with `CALL`; (b) no GQL access to points in Sprint 3, FR-TYPE-005 verified through the Rust API only; (c) Neo4j's `point()` function syntax, which BR-GQL-001 forbids.
* **D. gql-compat as an additional conformance artefact.** gql-compat (Apache-2.0, `9de1991`) has 590 cases citing ISO subclauses, feature codes and GQLSTATUS codes, including 93 mandatory cases. NFR-VER-002 and `CLAUDE.md` name only opengql/grammar, opengql/tck and the openCypher TCK. Options: (a, recommended) use its mandatory and condition cases as an input to lpg's own scenarios, translated and attributed; (b) not used; (c) vendored and run unchanged, which needs a YAML reader (a new dev dependency).
* **E. Timing items of exit #12.** "Unverified point 8 is closed" and "micro-workloads … measured" are timing measurements, which UPDATE 55 defers. Options: (a, recommended) Sprint 3 defines the micro-workloads and checks their results; their measurement and point 8 move to Sprint 6; (b) measure them in Sprint 3 as an exception.

### 3.3 BR-GQL-002 items that need the user's approval before the task that implements them (task #38)

96 of the 117 items in `ISO_IEC_39075(en)-implementation-defined.xml` are undecided (21 decided on 2026-10-07, `values-and-types.md:42-54`). The candidates Sprint 3 code can touch, by the task they block (insert line in brackets); task #38 drops those the approved feature set does not reach:

* **Lexical and parsing (blocks #41):** IV001 (394), IA019 (110), IA020 (114), IA023 (122), IL010 (358), IL018 (374), IE003 (310), IE005 (318), IW014 (478), IW023 (510).
* **Numbers, strings and functions (blocks #42):** IA003 (58), IA004 (62), IA005 (66), IA006 (70), IA007 (74), IA010 (78), IA011 (82), IA021 (118), ID057–ID059 and ID062–ID070 (194–242), ID074 (246), ID075 (250), ID079 (258), ID095–ID099 (282–298), IL009 (354), IV023 (434), IW017 (490).
* **Types, binding and patterns (blocks #43, #46):** IA013 (90), IA014 (94), IA017 (106), ID004 (146), ID005 (150), ID022 (166), ID023 (170), ID076 (254), ID085 (262), ID086 (266), ID089–ID091 (270–278), IL003 (350), IV003 (402), IV008 (406), IV011 (414), IV012 (418), IV014 (422), IW011 (470), IW012 (474), IW018 (494), IW019 (498), IW021 (502), IW022 (506), IE006 (322), IE007 (326).
* **Sessions, API, catalog and status (blocks #40, #47, #48):** IA001 (50), IA002 (54), ID001 (134), ID002 (138), ID003 (142), ID016 (158), ID017 (162), ID049 (190), ID061 (206), IE001 (302), IE008 (330), IE009 (334), IE010 (338), IV015 (426), IV016 (430), IW001–IW007 (438–462), IW010 (466), IW015 (482), IW016 (486), IW025 (514).
* **Not reached by the recommended set:** IA012 (86, GQL Flagger), IA025 (126, feature GA01).

Recommendation for each: Neo4j's behaviour where it has one, else the reference the task cites (user instruction "Neo4j/Memgraph defaults", Neo4j first).

### 3.4 Decided in this planning task (no spec change; for task #3's DECISION comment)

* **The parser covers all of `GQL.g4`**, not only the approved set: exit #12 compares acceptance with the oracle on generated sentences of the whole grammar. Constructs outside the approved set fail in the binder with the "feature not supported" code (TS-20).
* **The oracle crate and the generator live outside the `lpg` package** (`tools/gql-oracle/`, own `[workspace]`, `publish = false`), as the `docs/evidence/` experiments do (`docs/evidence/README.md:34`); `cargo tree -p lpg` shows neither.
* **The fuzz crate** (`fuzz/`, `cargo fuzz init`) is its own workspace too: `libfuzzer-sys` 0.4.13 is `(MIT OR Apache-2.0) AND NCSA` (crates.io), and NCSA is not on the NFR-DEP-002 list; it is never part of `lpg`'s dependency tree. Task #36 verifies that `cargo deny check licenses` on `lpg` is unaffected.
* **New runtime dependencies in Sprint 3:** `tokio` 1.53.2 and `tracing` 0.1.44, both already in TS-31 (`dependencies.md:208, 210`; TS-31 records tokio 1.53.1, crates.io has 1.53.2 of 2026-10-03). No other runtime dependency is proposed (§4).
* **Timing tools are not set up** (UPDATE 55): flame graphs, `perf`, `tokio-metrics`, parse and hand-off timing. `heaptrack` 1.5.0 is used for memory only.
* New modules follow TS-01: `query` (lexer, parser, AST, binder, planner, executor), `api` (public handles), `auth`, `proc`.

## 4. Reuse inventory

Verdicts: **COPY** (files copied, then adapted), **PORT** (rewritten in Rust from another language or another architecture, structure kept), **DEPEND** (a crate as a dependency), **STUDY** (read for decisions, nothing copied), **REJECT** (reason given). Licences were read in each clone's licence file unless marked "API only". Constraints every copy must meet or be adapted to: no `unsafe`, edition 2024, Linux, tokio async API, minimal dependencies (TS-31), standard GQL only. Every copy keeps its notices and records project, path and commit in the file header and in its task's `rmp` log.

### 4.0 Candidates searched

crates.io searches (2026-10-08) for `gql`, `iso gql`, `cypher`, `opencypher`, `gherkin`, `cucumber`, `graph query`, `property graph`; GitHub for `gql-compat`. Cloned and read: the projects in §0. Not cloned, decided from earlier evidence or metadata: DuckDB (MIT, push pipelines rejected by TS-20:83), LadybugDB (MIT fork of Kuzu; Kuzu read instead), Databend (GitHub API `NOASSERTION`), RisingWave (streaming SQL), Polars (dataframes), IndraDB and Cozo (MPL-2.0, `dependencies.md:234`), Shopify `cypher-parser` (GitHub API `NOASSERTION`, Cypher), GraphFusion `gql-parser` (63 downloads), `cucumber` 0.23.0 (a runner with its own async framework; TS-30:85 "a small runner suffices").

### 4.1 Lexer

| Candidate | Licence (file), commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| zu keyword tables | Apache-2.0, `a821319` | `crates/zu-common/src/keywords.rs` (179), `keywords/generated.rs` (76), `tests/keyword_table.rs` (drift test) | Reserved, pre-reserved, non-reserved lists generated from the ISO BNF with a drift test | Source must be `GQL.g4` (§3.1 point 1), not the BNF | **PORT** the generator and drift test to read `GQL.g4:3276-3661` |
| sqlparser-rs tokenizer | Apache-2.0 (`LICENSE.TXT`), `14cbf75` | `src/tokenizer.rs:546-760` (`Location`, `Span`, `TokenWithSpan`) | Line, column and span tracking | No dependency on `log` | **COPY** the span types |
| Grafeo GQL lexer | Apache-2.0, `3c23dbe` | `crates/grafeo-adapters/src/query/gql/lexer.rs` (1 175), `forbid(unsafe_code)` | Hand-written GQL lexer | Mixes Cypher tokens; `is_alphabetic` identifiers | **STUDY** |
| miniGU lexer | Apache-2.0, `fc2ccf5` | `minigu/gql/parser/src/lexer.rs` (1 567) | Full GQL token set | `logos` dependency; `unsafe` at `lexer.rs:838-867` | **REJECT** (unsafe, dependency); its token list is checked against the generated table |
| zu lexer | Apache-2.0 | `crates/zu-query/src/lexer.rs` (977) | Comments (GB02 double minus) | zuQL, not `GQL.g4` | **STUDY** |
| `logos`, `unicode-ident` | crates.io | — | Lexer generator; XID tables | TS-19 rejects `logos`; XID ≠ ID (§3.1 point 2) | **REJECT** |

### 4.2 Recursive-descent and Pratt parser, typed AST following `GQL.g4`

Measured: parser functions whose names equal a `GQL.g4` parser rule (568 rules found by the script; M-QE reports 574): miniGU 205 of 393 functions, GraphLite 37 of 218, zu 18 of 390, Grafeo 10 of 372.

| Candidate | Licence (file), commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **miniGU `gql-parser`** | Apache-2.0, `fc2ccf5` | AST `minigu/gql/parser/src/ast/*.rs` (1 311); rule functions `src/parser/impls/*.rs` (4 841); `span.rs` (46); 143 `insta` snapshot inputs | Functions named after `GQL.g4` rules; spans; Pratt for value and label expressions | Built on `winnow` combinators and `logos`; `unsafe` in `lexer.rs` and `error.rs:205`; TODOs for simplified path patterns, union and reference types (`ast/common.rs:83`, `ast/type_element.rs:8`) | **COPY** the AST, **PORT** the rule functions to plain recursive descent; reuse the snapshot inputs as test texts |
| sqlparser-rs | Apache-2.0, `14cbf75` | `src/parser/mod.rs:95-140` (`RecursionCounter`, `DepthGuard`), `:1424-1460` (`parse_subexpr` precedence loop) | Hand-written RD + Pratt, recursion guard, 0 `unsafe` | SQL grammar | **COPY** the guard and the precedence loop |
| Grafeo GQL parser | Apache-2.0, `3c23dbe` | `query/gql/parser.rs` (11 256), `ast.rs` (978) | Hand-written RD, nesting limit | Cypher extensions (`MERGE`, `UNWIND`, `LOAD CSV`, `CREATE INDEX`; `parser.rs:358-458, 1135, 4717-4813`), BR-GQL-001 forbids them; not rule-by-rule | **STUDY** |
| zu parser | Apache-2.0, `a821319` | `crates/zu-query/src/parser.rs` (11 390) | GQL-like RD parser with GQLSTATUS errors | zuQL accepts `UNWIND`, `WITH` | **STUDY** |
| GraphLite | Apache-2.0, `a370a1c` | `graphlite/src/ast/parser.rs` (5 564, `nom`) | GQL subset | Combinators, non-standard statements | **REJECT** |
| frogql | MIT, `b5dd0f0` | `src/parser/grammar.rs` (2 968) | Path patterns | `String` errors, no spans, `NEAREST` extension | **STUDY** |
| drasi `query-gql`, gqlite `gqlparser`, open-cypher, lance-graph | Apache-2.0 / MIT | `peg` (891), Cypher `nom`+`logos`, `lalrpop`, `nom` | Subsets or Cypher | PEG, combinators, Cypher | **REJECT** |

### 4.3 Test-only ANTLR oracle crate

| Candidate | Licence, commit | Paths | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| `antlr4rust` 0.6.0 | BSD-3-Clause (crates.io and GitHub API), release v0.6.0 of 2026-09-27 with `antlr4-4.13.3-SNAPSHOT-complete.jar` | — | Runtime of the generated parser | Dependency of the oracle crate only (TS-31 row) | **DEPEND** (oracle crate only) |
| lpg's own experiment | MIT (lpg) | `docs/evidence/antlr-exp/bench/src/main.rs` (54), `Cargo.toml` | `parse()` with error listeners, ambiguity counting | Use LL, not SLL, for exact acceptance | **COPY** (own code) |
| opengql/grammar | Apache-2.0, `16ea71b` | `GQL.g4` (3 774), `samples/` (14) | Grammar and samples | Unmodified, licence kept | **DEPEND** (artefact, NFR-VER-002) |

### 4.4 GQL sentence generator from `GQL.g4`

| Candidate | Licence, commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **gql-compat grammar walk** | Apache-2.0 (file), `9de1991` | `grammar/generate.go` (566), `reduce.go` (266), `leaves.go` (107) | Seeded, cost-bounded walk with derivation paths; shrinker that undoes choices | Go; walks the ISO BNF XML; must read `GQL.g4` | **PORT** |
| grammarinator | BSD-3-Clause (`LICENSE.rst`, read 2026-10-08; GitHub API reports `NOASSERTION`), `3a6a745` | Python generator from ANTLR grammars | Min-depth expansion from `.g4` | Python tool; TS-30:88 rejected it on the `NOASSERTION` report, which the licence file contradicts | **STUDY** (strategy); the spec's reason is corrected in task #39 |
| grammars-v4 `ANTLRv4Parser.g4` | BSD (file header), `7df52be` | `antlr/antlr4/ANTLRv4{Lexer,Parser}.g4` (879) | A parser for `.g4` files | Its lexer needs a target-specific `LexerAdaptor`; none for Rust in the repository | **REJECT**; a small own reader of the constructs `GQL.g4` uses is simpler |
| Oxigraph `sparql-smith` | MIT/Apache-2.0, `fda6793` | `lib/sparql-smith/src/lib.rs` (2 452) | `arbitrary`-driven grammar generator | Hand-written per SPARQL rule | **STUDY** |

### 4.5 Binder, semantic analysis, type system

| Candidate | Licence, commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **miniGU binder** | Apache-2.0, `fc2ccf5` | `minigu/gql/planner/src/binder/*.rs` (≈ 2 000), `bound/*.rs` | Binding over the AST of §4.2, GQL rule names | Its own catalog and types | **PORT** (pairs with the copied AST) |
| zu feature table | Apache-2.0, `a821319` | `crates/zu-cli/src/statement/features.rs` (925, generated from the features insert by `crates/zu-cli/tests/statement.rs`) | All 228 feature codes with descriptions | Generator to point at lpg's insert | **PORT** (source of the feature-gate table) |
| zu binder | Apache-2.0 | `crates/zu-query/src/binder.rs` (9 178, 0 `unsafe`) | Scopes, types, GQLSTATUS | zuQL | **STUDY** |
| frogql typing | MIT, `b5dd0f0` | `src/typing/checker.rs` (1 898), `inference.rs` | Gradual typing of path patterns (paper DOI 10.1145/3763061) | Research prototype | **STUDY** |
| Kuzu binder | MIT, `89f0263` | `src/binder/` | Cypher binder, expression typing | C++, Cypher | **STUDY** |
| Neo4j semantics, Memgraph symbol table | GPLv3 / BSL | (TS-20 citations) | — | Not copyable | **STUDY** |

### 4.6 Logical and physical planner with index seeks

| Candidate | Licence, commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **Grafeo join ordering** | Apache-2.0, `3c23dbe` | `crates/grafeo-engine/src/query/optimizer/join_order.rs` (1 378, DPccp) | Exact DP over connected subgraphs | No size threshold; its cost model | **COPY**, adapted to TS-20's cost and threshold |
| **Kuzu join order** | MIT (file), `89f0263` | `src/planner/plan/plan_join_order.cpp` (627; threshold at `:136-160`), `src/include/planner/subplans_table.h:11` (`MAX_LEVEL_TO_PLAN_EXACTLY = 7`), `join_order/cardinality_estimator.cpp` (247), `cost_model.cpp` (57) | The exact TS-20 policy | C++ | **PORT** the threshold, greedy fallback and estimator |
| Grafeo cardinality and cost | Apache-2.0 | `optimizer/cardinality.rs` (2 798), `cost.rs` (1 593) | Histograms | TS-20 rejects histograms (M-QE §Q3) | **STUDY** |
| DataFusion optimizer, miniGU optimizer | Apache-2.0 | `datafusion/optimizer` (not cloned), `minigu/gql/planner/src/optimizer` (241) | Rule rewrites | Relational, Arrow | **STUDY** |

### 4.7 Pull-based batch executor and operators

| Candidate | Licence, commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **Grafeo operators** | Apache-2.0, `3c23dbe` | `crates/grafeo-core/src/execution/operators/`: pull trait `mod.rs:302-322`; `aggregate.rs` (1 977), `sort.rs` (665), `join.rs` (1 850), `expand.rs` (610), `limit.rs` (513), `distinct.rs` (327), `accumulator.rs` (321) | Pull, batch-at-a-time operators in Rust | Typed `ValueVector` chunks, its own storage, `parking_lot`; `unsafe` only in `top_k.rs` | **PORT** the algorithms to columns of `Value`, the accountant and lpg's trees |
| Grafeo path operators | Apache-2.0 | `variable_length_expand.rs` (1 564; WALK, TRAIL, SIMPLE, ACYCLIC), `shortest_path.rs` (1 275) | Quantified paths and selectors | Optional features outside the recommended set (§3.2 A) | **STUDY** now; candidate if the user adds those features |
| Kuzu processor | MIT, `89f0263` | `src/processor/operator/` | Factorised operators | C++, factorisation rejected (TS-20:85) | **STUDY** |
| DataFusion physical plan | Apache-2.0, `54a4bbf` | `physical-plan/src/sorts/sort.rs` (4 291), `memory` reservations | Spilling sort, memory pool | Arrow, async streams, spilling (lpg never spills) | **STUDY** |
| zu executor, GlueSQL executor, miniGU execution | Apache-2.0 | `zu-query/src/exec.rs` (12 140; `frame.rs` has `unsafe`), `core/src/executor` (10 183, async, `chrono`), `minigu/gql/execution` (14 328, `arrow`) | Executors | `unsafe`, heavy dependencies | **REJECT** |
| Memgraph cursors | BSL-1.1 | `src/query/plan/operator.cpp` | Pull cursors | Not copyable | **STUDY** |

### 4.8 Expression evaluation and three-valued logic

| Candidate | Licence, commit | Paths, size | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **zu function registry** | Apache-2.0, `a821319` | `crates/zu-query/src/functions.rs` (2 835): one row per function with signature, kernel, determinism; constant folding | ISO clause 20 functions | Its `Decimal`, `Temporal`; adapt to lpg `Value` and `jiff` | **PORT** |
| zu CAST | Apache-2.0 | `crates/zu-query/src/cast.rs` (1 270) | `CAST` rules | GA05 is optional, outside the recommended set | **STUDY** |
| lpg `types` | MIT (lpg) | `src/types/mod.rs:206-414` (`group`, `float_cmp`, `total_cmp`) | Approved comparison and order | — | reuse (own code) |
| Oxigraph `spareval` | MIT/Apache-2.0, `fda6793` | `lib/spareval/src/expression.rs` (2 915), 0 `unsafe` | Evaluation with error values | SPARQL semantics | **STUDY** |
| Grafeo filter, drasi functions | Apache-2.0 | `operators/filter.rs` (7 065); `functions-gql` (501) | Predicate evaluation | Vectorised types; continuous queries | **STUDY** |

### 4.9 Closed-graph-type check

| Candidate | Licence, commit | Paths | Covers | Gap and adaptation | Verdict |
|---|---|---|---|---|---|
| **zu graph types** | Apache-2.0, `a821319` | `crates/zu-zu1/src/catalog.rs` (`GraphType`, `holder`, `types_for`; struct at `:298-305`, checks at `:476, 489, 977`), `crates/zu/src/write.rs:1228-1236`, `crates/zu/src/declare.rs:335-380`, `catalog_stmt.rs` (700) | Element-type matching by label set (GG02, GG20–GG26 handling) | Tied to zu's columnar files; its matching rules are its reading of ISO (OQ-005) | **PORT** the matching functions; record the rules as provisional |
| frogql typing | MIT | `src/typing/` | Type descriptors of graph types | Research model | **STUDY** |
| GraphLite schema enforcement | Apache-2.0 | `graphlite/src/schema/enforcement/` | Schema validation | Own schema language | **REJECT** |

### 4.10 Session, transaction and `ResultStream` async API with `spawn_blocking`

Searched for an async API over a synchronous engine that hands each batch to `spawn_blocking`: none found. Turso's async API steps its own I/O state machine (`bindings/rust/src/rows.rs:47-80`); GraphLite uses `async-trait` and `rayon`; Grafeo and zu are synchronous.

| Candidate | Licence, commit | Paths | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| Turso Rust bindings | MIT, `8cc059b` | `bindings/rust/src/{connection,rows,transaction}.rs` (5 743 in all) | `ConnectionOperationGate` (one request at a time, FR-SES-004), `DropBehavior` of a dangling transaction, `async fn next` | Different async model | **STUDY** |
| neo4rs `RowStream::next` | MIT | (TS-02 citation) | Row pull API | Bolt client | **STUDY** |
| `tokio` 1.53.2 | MIT, crates.io | `spawn_blocking`, `time` | Runtime, timers | Features `rt`, `time` only | **DEPEND** (TS-31) |

### 4.11 Cancellation and resource limits

| Candidate | Licence, commit | Paths | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| zu `Interrupt` | Apache-2.0, `a821319` | `crates/zu-common/src/interrupt.rs` (163) | Shared stop flag checked per chunk | One reason only; lpg needs cancel, terminate, timeout | **STUDY** (an atomic status is a few lines) |
| Memgraph check every 20th pull | BSL-1.1 | `src/query/plan/operator.cpp:510-528` | Interval | — | **STUDY** |
| lpg accountant | MIT (lpg) | `src/storage/memory.rs:145-186` | Reservations | — | reuse (own code) |

### 4.12 Procedure and function registry, `CALL`

| Candidate | Licence, commit | Paths | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| **zu procedure table** | Apache-2.0, `a821319` | `crates/zu-query/src/procedures.rs` (304) | Descriptor table: name, arity, argument types, columns; schema resolution | Built-ins only, no registration, no modes | **PORT** the descriptor shape |
| Neo4j `Mode`, `ProcedureCaller`; Memgraph `mgp` | GPLv3 / BSL | (TS-24 citations) | Modes, immutable graph for read procedures | Not copyable | **STUDY** |
| Kuzu table functions | MIT | `src/function` (not in the sparse clone) | Table functions | C++ | **STUDY** |

### 4.13 EXPLAIN and PROFILE

| Candidate | Licence, commit | Paths | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| **Grafeo `ProfiledOperator`** | Apache-2.0, `3c23dbe` | `crates/grafeo-core/src/execution/profile.rs` (187) | Decorator around a pull operator: rows, time, calls | `parking_lot::Mutex` (single-threaded statement needs none); add batches, entries read, buffer hits and misses, peak memory | **COPY** |
| Grafeo engine profile, DataFusion `metrics.rs`, Memgraph `ScopedProfile` | Apache-2.0 / BSL | `grafeo-engine/src/query/profile.rs` (244); `physical-plan/src/metrics.rs` | Plan rendering, metric sets | — | **STUDY** |

### 4.14 Metrics and `tracing`

| Candidate | Licence, commit | Paths | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| `tracing` 0.1.44 | MIT, crates.io | — | Events and spans | — | **DEPEND** (TS-31) |
| Turso core | MIT, `8cc059b` | `#[instrument]` in 27 files of `core/` | Span placement | Logs parameters in places (to check per span) | **STUDY** |
| `metrics`, `tracing-subscriber`, `tokio-metrics` | crates.io | — | Global recorder; subscriber; runtime timing | TS-24 rejects `metrics`; a test subscriber is a few lines over `tracing::Subscriber`; runtime timing deferred | **REJECT** |
| lpg `MetricsSink` | MIT (lpg) | `src/obs/mod.rs:17-101` | Sink and names | Add three categories | reuse (own code) |

### 4.15 Authentication and authorisation trait

| Candidate | Licence, commit | Paths | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| GraphLite security | Apache-2.0, `a370a1c` | `graphlite/src/exec/schema_engine/operations/security/` | Roles inside the engine | Own GQL grant syntax (BR-AUTH-001 forbids) | **REJECT** |
| Neo4j privilege model | documentation (D-PRIV) | — | Action names | — | **STUDY** |

Nothing reusable exists for a plan-time `(action, object, owner)` request to an application trait: own code (TS-23 fixes the shape).

### 4.16 Gherkin-style TCK conformance runner

Measured: `gherkin` 0.16.0 parses 219 of 220 openCypher TCK files (1 602 scenarios, 276 outlines; `Literals6.feature` fails) and 14 of 14 opengql/tck files (92 scenarios); it brings 40 crates into the test build, ICU among them (`cargo tree`, probe in the session scratchpad). Step vocabulary counted: openCypher `When executing query:` 1 615, `Then the result should be, in any order:` 1 258, `And no side effects` 1 179, `Given an empty graph` 949; opengql/tck adds `Given an empty catalog`, `When executing the program:`, `Then an exception condition should be raised:`.

| Candidate | Licence, commit | Paths, size | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| **cyrs feature scanner** | MIT OR Apache-2.0 (files), `6a6bb34` | `crates/cyrs-tck/tests/full.rs:88-360` (scanner of `Scenario`, `Scenario Outline`, `Examples`, query docstrings), `forbid(unsafe_code)` | Line scanner without dependencies | Only query steps; extend to all steps | **COPY** |
| `gherkin` 0.16.0 | MIT OR Apache-2.0 | — | Full Gherkin parser | 40 transitive crates; one TCK file fails | **REJECT** |
| `cucumber` 0.23.0 | MIT OR Apache-2.0 | — | Runner framework | TS-30:85 | **REJECT** |
| opengql/tck runner | Apache-2.0, `e0700a7` | `runner/main.go` (18 lines, a stub) | — | Nothing to reuse | **REJECT** |
| zu TCK scoreboard | Apache-2.0 | `crates/zu/tests/tck.rs` (403) | Hand-written subset | Not a Gherkin runner | **STUDY** |
| **opengql/tck, openCypher TCK** | Apache-2.0, `e0700a7`, `677cbaf` | `features/` | The scenarios | Licence and attribution unchanged | **DEPEND** (artefacts, NFR-VER-002) |
| gql-compat corpus | Apache-2.0, `9de1991` | `corpus/suite/*.yaml` (590 cases) | ISO-cited cases | Not named in NFR-VER-002 | user decision (§3.2 D) |

### 4.17 GQLSTATUS conditions and error model

Measured: zu's `crates/zu-common/artifacts/gql-conditions.xml` and `gql-bnf.xml` are byte-identical to lpg's inserts (`cmp`); gql-compat's six ISO artefacts have the SHA-256 values of `docs/standards/iso-iec-39075/ed-1/README.md`.

| Candidate | Licence, commit | Paths, size | Covers | Gap | Verdict |
|---|---|---|---|---|---|
| **zu GQLSTATUS** | Apache-2.0, `a821319` | `crates/zu-common/src/gqlstatus.rs` (682), `src/gqlstatus/generated.rs` (658), `tests/gqlstatus_table.rs` (347), 0 `unsafe` | Table generated from the conditions insert, drift test, severity per class | Forbids non-ISO codes by design; lpg also needs Neo4j codes (`interfaces.md:116`); `thiserror` | **COPY**, with a second table for the Neo4j codes used |
| Grafeo `gqlstatus.rs` | Apache-2.0 | `crates/grafeo-common/src/utils/gqlstatus.rs` (443) | Hand-written constants | Not generated | **STUDY** |
| Neo4j GQLSTATUS page | documentation | 453 codes | Codes outside the insert | — | **STUDY** (source of codes) |

### 4.18 Summary of verdicts

| Component | Main source | Verdict |
|---|---|---|
| Lexer | zu keyword generator (→ `GQL.g4`); sqlparser-rs spans | PORT; COPY |
| Parser and AST | miniGU AST and rule functions; sqlparser-rs guard and precedence loop | COPY + PORT; COPY |
| ANTLR oracle crate | lpg `antlr-exp/bench`; `antlr4rust` 0.6.0 | COPY (own); DEPEND (oracle only) |
| Sentence generator | gql-compat `grammar/generate.go`, `reduce.go` | PORT |
| Binder and types | miniGU binder; zu feature table | PORT; PORT |
| Planner | Grafeo DPccp; Kuzu threshold and estimator | COPY; PORT |
| Executor and operators | Grafeo pull operators | PORT |
| Expressions and 3VL | zu function registry; lpg `types` | PORT; own |
| Closed graph types | zu `GraphType` matching | PORT |
| Async API | Turso bindings, neo4rs (study); `tokio` | STUDY; DEPEND |
| Cancellation and limits | zu `Interrupt`, Memgraph (study) | STUDY |
| Procedures and `CALL` | zu procedure descriptors | PORT |
| EXPLAIN and PROFILE | Grafeo `ProfiledOperator` | COPY |
| Metrics and `tracing` | `tracing`; lpg sink | DEPEND; own |
| Authorisation | none reusable | own code |
| TCK runner | cyrs feature scanner; TCK artefacts | COPY; DEPEND (artefacts) |
| GQLSTATUS and errors | zu generated condition table | COPY |

## 5. Task decomposition

Eighteen tasks. Ids #36–#53 are provisional: `rmp` assigns them (highest existing id is #35). Every field is Markdown with `*` bullets; §8 checks lengths and the "no line starting with `- `" rule. Every test that writes graph data uses `/tmp/lpgram/<run>/`, deleted at its end; tests that need real `fsync` use the lab directories of Sprints 1 and 2 (`LPG_LAB_EXT4`, `LPG_LAB_XFS`).

**Grouping (CLAUDE.md "Synergy and convergence"):** the laboratory task holds every instrument (oracle, generator, fuzz, coverage, heaptrack), as Sprint 2's #24 did. The feature set (#37), the implementation-defined items (#38) and the specification gaps (#39) are separate because each blocks different tasks and two of them wait on the user. The engine is split along TS-20's phases, each with its own evidence (parser: differential and fuzz; binder: gates and fuzz; planner: DP against brute force; executor: reference evaluator, PROFILE, heaptrack). Cancellation, limits and transaction listing and termination share one status per transaction (comment #11) and are one task. Authorisation and the management API are one task because every management call is authorised and records an owner. The conformance runner comes last among the engine tasks so every scenario runs once on the final engine; the exit task re-runs everything.

**Common method in every implementation task** (stated in each `-tr` as "Method"): study the cited references for each critical decision and record a `DECISION` comment with the evidence; record each copy or port (project, path, commit, licence) in the file header and in a `NOTE` on the task; measure the task's tests with `cargo nextest` and remove waste without weakening assertions; graph data only in `/tmp/lpgram/<run>/`, never for tests that depend on real flushes. No latency or throughput validation (UPDATE 55).

---

### #36 CHORE, priority 9, depends on: none

**Title:** Set up the Sprint 3 laboratory: ANTLR oracle, sentence generator, fuzzing

**-fr**
```text
**Every Sprint 3 parser and engine claim must be checked by an instrument, not by reasoning.** Before any engine code exists, the laboratory needs the latest toolchain, the test-only parser oracle generated from the unmodified `GQL.g4` (TS-30, NFR-VER-002), a sentence generator from `GQL.g4` (TS-30), a fuzzing set-up, coverage per function, and `heaptrack` for memory, so later tasks only add targets and inputs.

* Correctness only: the timing items of sprint comment #13 (flame graphs, parse time, hand-off cost, runtime metrics, cancellation latency) are deferred to Sprint 6 (sprint UPDATE 55).
```

**-tr**
```text
* **Toolchain:** `rustup self update`, `rustup update stable nightly`, keep 1.89 (MSRV, TS-29); `cargo install --locked <crate>@<max_stable_version>` for `cargo-deny`, `cargo-llvm-cov`, `cargo-nextest`, `cargo-fuzz`, `inferno` when crates.io lists a newer release; record `java -version` and `heaptrack --version`.
* **Oracle crate** `tools/gql-oracle/` with its own `[workspace]` and `publish = false`, never a member or dependency of `lpg`. `antlr4rust` 0.6.0 (BSD-3-Clause); parser generated by `antlr4-4.13.3-SNAPSHOT-complete.jar` of antlr4rust release v0.6.0 from `GQL.g4` of opengql/grammar @ `16ea71b`, unmodified, its licence beside it. Default LL prediction, not SLL. API `accepts(text) -> bool` and a command that labels each input of a directory or of stdin. Starting point: `docs/evidence/antlr-exp/bench/src/main.rs` (own code). A `DECISION` records whether generated files are checked in or generated by a script.
* **Generator** in the same crate: a small reader of the `GQL.g4` constructs (rules, alternatives, `?`, `*`, `+`, groups, literals, token and fragment references, character sets); a seeded walk bounded by minimum derivation cost that records the rules it expanded; a shrinker that undoes choices; near-miss mutants (token deletion, insertion, swap) for rejection cases. Port of gql-compat `grammar/generate.go`, `reduce.go` @ `9de1991` (Apache-2.0); grammarinator @ `3a6a745` (BSD-3-Clause) studied. `LPG_SEED` sets the seed.
* **Fuzzing:** `fuzz/` from `cargo fuzz init`, own workspace (`libfuzzer-sys` includes NCSA, not on the NFR-DEP-002 list); a `#[cfg(fuzzing)]` entry module in `lpg`; one smoke target that decodes arbitrary bytes as a record (`src/types/record.rs`) and must never panic.
* **Coverage per function:** a recipe that turns `cargo llvm-cov nextest --json` into a per-function table, used later per grammar rule and per operator.
* **heaptrack:** a recipe for a test binary, peak heap per test.
* **Record:** `docs/evidence/lab-sprint-3/report.md` with Environment and Tools (name, version, command).
* Out of scope: flame graphs, `perf`, `tokio-metrics`, any timing.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`.
```

**-ac**
```text
* `rustup check` reports no update; each cargo tool's `--version` equals its crates.io `max_stable_version`, shown in the report with `java -version` and `heaptrack --version`.
* The oracle accepts the 14 samples of opengql/grammar and the six `q0*` inputs of `docs/evidence/antlr-exp/corpus/`, and rejects `zz_invalid.gql`.
* With one fixed seed the generator writes at least 10 000 sentences whose derivations expand every parser rule of `GQL.g4` reachable from `gqlProgram`; two runs with that seed give byte-identical output; the oracle accepts every positive sentence, or the report lists each rejected class with its cause.
* `cargo +nightly fuzz run <smoke target> -- -runs=1000000` exits 0.
* `cargo deny check licenses` and `cargo +1.89 build` on `lpg` exit 0; `cargo tree -p lpg -e normal` lists neither `antlr4rust` nor `libfuzzer-sys`.
* The report lists every tool with version and command; a `FINDING` names any missing item.
```

---

### #37 TASK, priority 9, depends on: none

**Title:** Propose the provisional GQL feature set and obtain the user's approval

**-fr**
```text
**The engine must implement a feature set the user approved, because the Conformance Rules that define minimum conformance are not available** (OQ-005; sprint DECISION 11: "the planning task proposes a provisional feature set … and the user approves it before any engine task"). The approval also settles the FR-GQL-006 order conflict and OQ-006 (points without new syntax), which the same risk comment names.

This task blocks every engine task (#40 to #53) and the implementation-defined approvals of #38.
```

**-tr**
```text
Owners: `specification-manager` for `docs/functional-spec/`, `technical-writer` for `docs/technical-spec/`.

1. **Mandatory surface:** classify every parser rule of `GQL.g4` @ `16ea71b` as covered by a code of `ISO_IEC_39075(en)-features.xml` or as mandatory. The placement rule is gql-compat's (README @ `9de1991`), not ISO text: mark it provisional.
2. **Optional features** proposed: GG01, GG02, GP18, GV39, GV40, GV41 (named by requirements) and GD01, GT01, GP04, GC01, GC04, the graph-type creation construct, GQ01, GT03 (needed by in-scope acceptance criteria; task #3's plan §3.2 A). Verify each against `GQL.g4` and the insert.
3. **FR-GQL-006:** compare the set with Neo4j's "Supported optional GQL features" (63 codes, fetched 2026-10-08) and with Memgraph's documentation (locate it, or state that none exists). Report each feature neither supports (planning found GG02, GP18, GV41, GC01, GC04, GT03) with options: (a, recommended) a requirement overrides the order for the feature it needs; (b) deliver every Neo4j- or Memgraph-supported feature first; (c) defer those requirements.
4. **OQ-006:** options of plan §3.2 C; recommended (a) points as values, constructors, distance and a distance-within search through built-in `CALL` procedures.
5. **TCK applicability:** the rule that selects opengql/tck and openCypher TCK scenarios for the set (tags, constructs), with counts per artefact.
6. **gql-compat** as an input to lpg's scenarios: options of plan §3.2 D.

Ask the user each point with two to four options, the recommended one first. Record the answers, then close. Out of scope: implementation-defined items (#38) and any code.
```

**-ac**
```text
* The approved feature list, with the date and the user's words, is recorded where FR-GQL-002 publishes supported features and in OQ-005.
* `docs/technical-spec/query-engine.md` holds the table `GQL.g4` rule → feature code or "mandatory", marked provisional (OQ-005).
* FR-GQL-006 records the user's answer on the order conflict; OQ-006 records the answer on points; the TCK selection rule and its counts are recorded in TS-30.
* A `DECISION` comment on this task quotes the user's answer to each point.
* The closing commit changes only files under `docs/` (`git show --stat`).
```

---

### #38 TASK, priority 9, depends on: #37

**Title:** Obtain the user's approval of the implementation-defined items Sprint 3 uses

**-fr**
```text
**Each implementation-defined choice the engine makes must have the user's explicit approval before the code that depends on it** (BR-GQL-002; sprint DECISION 11). 96 of the 117 items of the ISO implementation-defined insert are undecided. This task asks those the approved feature set (#37) reaches, grouped by the task each one blocks.
```

**-tr**
```text
Owner: `specification-manager`, with `technical-writer` for TS-19, TS-20 and TS-21. For each item give its code, its line in `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-implementation-defined.xml`, Neo4j's behaviour (official documentation or study of source, never copied), and two to four options, the recommended one first. Drop the items the approved set does not reach and list them as not decided.

1. **Lexical and parsing (blocks #41):** IV001 (394), IA019 (110), IA020 (114), IA023 (122), IL010 (358), IL018 (374), IE003 (310), IE005 (318), IW014 (478), IW023 (510).
2. **Numbers, strings and functions (blocks #42):** IA003 to IA007, IA010, IA011 (58–82), IA021 (118), ID057 to ID059, ID062 to ID070 (194–242), ID074 (246), ID075 (250), ID079 (258), ID095 to ID099 (282–298), IL009 (354), IV023 (434), IW017 (490).
3. **Types, binding and patterns (blocks #43, #46):** IA013 (90), IA014 (94), IA017 (106), ID004 (146), ID005 (150), ID022 (166), ID023 (170), ID076 (254), ID085 (262), ID086 (266), ID089 to ID091 (270–278), IL003 (350), IV003 (402), IV008 (406), IV011 (414), IV012 (418), IV014 (422), IW011 (470), IW012 (474), IW018, IW019, IW021, IW022 (494–506), IE006 (322), IE007 (326).
4. **Sessions, API, catalog and status (blocks #40, #47, #48):** IA001 (50), IA002 (54), ID001 to ID003 (134–142), ID016 (158), ID017 (162), ID049 (190), ID061 (206), IE001 (302), IE008 to IE010 (330–338), IV015 (426), IV016 (430), IW001 to IW007 (438–462), IW010 (466), IW015 (482), IW016 (486), IW025 (514).

Out of scope: items already decided on 2026-10-07 (TS-21), IA012 (Flagger) and IA025 (feature GA01) unless #37 approved GA01.
```

**-ac**
```text
* BR-GQL-002 lists every approved code with its choice and the date; TS-19, TS-20 or TS-21 holds the detail of each, with its reference.
* The items left undecided are listed with the reason (not reached by the approved feature set).
* A `DECISION` comment on this task quotes the user's answer to each group.
* The closing commit changes only files under `docs/` (`git show --stat`).
```

---

### #39 TASK, priority 9, depends on: none

**Title:** Resolve the specification gaps that block Sprint 3 implementation

**-fr**
```text
**Sprint 3 code must implement what the specification states, not assumptions made while coding.** Planning (task #3) found 15 points where `docs/technical-spec/` is silent, ambiguous or inaccurate for the Sprint 3 scope, and one point that needs the user. Each point blocks the task named beside it.
```

**-tr**
```text
Owners: `technical-writer` (`docs/technical-spec/`), `specification-manager` (`docs/functional-spec/`). Decide points 1 to 15 as `CLAUDE.md` "Research and decision-making" requires; proposals and references are in task #3's plan §3.1.

1. Keywords: follow `GQL.g4` (3276–3661) where the ISO BNF differs (`FALSE`, `IMPLIES`, `SKIP`, `TRUE`, `UNKNOWN`, `WHITESPACE`, `TEMP`); record in TS-19. Blocks #41.
2. Identifier characters `\p{ID_Start}` or `Pc`, `\p{ID_Continue}` (`GQL.g4:3612-3623`): generated tables of the Unicode version the ANTLR jar uses; `unicode-ident` is XID. Blocks #41.
3. Ambiguities resolved to the lowest alternative, as antlr4rust does (`prediction_mode.rs:185-196`). Blocks #41.
4. Owned transactions for `spawn_blocking` (`src/tx/mod.rs:108-114`; `Engine::handle`, `src/storage/engine.rs:771`). Blocks #45, #47.
5. GQLSTATUS of every internal error kind and API failure: ISO insert, else Neo4j's GQLSTATUS page; the "feature not supported" code. Blocks #40.
6. Procedure name resolution and functions invoked only by `CALL` (`GQL.g4:1458-1461, 2220-2235`). Blocks #43, #50.
7. Correct TS-20's "push-based" for DataFusion (pull streams, `execution_plan.rs` @ `54a4bbf`); correct TS-30's grammarinator reason (`LICENSE.rst` is BSD-3-Clause @ `3a6a745`). Blocks none.
8. Reservation chunk size of blocking operators (TS-20:66). Blocks #45.
9. Cancellation check interval K (TS-20:69; Memgraph 20). Blocks #45, #49.
10. Limits on `Session` and `Transaction`, no defaults, bytes as record encoding size (FR-API-007, FR-LIM-002). Blocks #49.
11. `ResultStream::summary()` counters and completion code. Blocks #47.
12. EXPLAIN and PROFILE result types, Neo4j operator names. Blocks #44, #45.
13. Metric names for query execution, data counts, indexes. Blocks #51.
14. `tracing` span and event levels and fields without parameter values. Blocks #51.
15. Text session and transaction statements against API state (FR-GQL-005), class-25 codes. Blocks #47.

### Ask the user
16. Exit #12 asks to close unverified point 8 and to measure the micro-workloads; both are timing, which UPDATE 55 defers. Options: (a, recommended) define and check the micro-workloads now, measure them and close point 8 in Sprint 6; (b) measure in Sprint 3 as an exception. Blocks #53.
```

**-ac**
```text
* Each of points 1 to 16 is answered in the `docs/technical-spec/` or `docs/functional-spec/` section it cites, with its evidence: the reference studied (project, file and line) or the user's decision and its date.
* A `DECISION` comment on this task maps each point to the file and heading that now answer it.
* The closing commit changes only files under `docs/` (`git show --stat`).
```

---

### #40 TASK, priority 8, depends on: #36, #37, #39

**Title:** Implement the public error type and the GQLSTATUS condition table

**-fr**
```text
**Every failure the public API returns must carry a GQLSTATUS code, a message and its cause** (`interfaces.md` "Public API shape", `Error` row; FR-API-001). Codes come from the ISO conditions insert where it defines the condition, otherwise from Neo4j (`interfaces.md:116`), as task #39 point 5 decided.
```

**-tr**
```text
* **Condition table** generated from the unmodified `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-conditions.xml`, with a drift test that regenerates it and fails on any difference. Copy: zu @ `a821319` (Apache-2.0) `crates/zu-common/src/gqlstatus.rs` (682 lines), `src/gqlstatus/generated.rs` (658), `tests/gqlstatus_table.rs` (347); adapt the insert path, drop `thiserror`, keep the notice. zu's artefact is byte-identical to lpg's insert (`cmp`, task #3 plan §4.17).
* **Neo4j codes:** a second, hand-listed table of the non-ISO codes #39 point 5 chose (25N11, 50N05 and the others), each with the Neo4j page it comes from.
* **`lpg::Error`:** public; GQLSTATUS, message, optional cause; every kind of `src/storage/error.rs:9-104` maps to a code (exhaustive `match`, replacing `gqlstatus` at `:217-223`).
* **Severity** per class category (S, N, W, I, X) from the insert.
* Study: Grafeo `crates/grafeo-common/src/utils/gqlstatus.rs` @ `3c23dbe`.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * the generated table equals the insert (drift test); a code edited in the generated file makes the test fail (mutation, then restored);
  * every ISO class and subclass of the insert is present once, with its category;
  * every internal error kind maps to a code (a test enumerates the kinds), and 25N11 and 50N05 are unchanged;
  * `Error` exposes code, message and cause, and its `Display` names the code.
* `cargo deny check licenses` exits 0 and `cargo tree -e normal --depth 1` gains no crate.
* Coverage of the error module from `cargo llvm-cov` is recorded in the task log.
```

---

### #41 TASK, priority 8, depends on: #36, #38, #39, #40

**Title:** Implement the GQL lexer, recursive-descent and Pratt parser, and typed AST

**-fr**
```text
**lpg must accept and reject exactly the inputs that `GQL.g4` accepts and rejects, with a typed AST that keeps source spans** (TS-19; NFR-VER-002; exit criterion 1 of sprint DECISION 12). The parser covers the whole grammar; constructs outside the approved feature set are refused later by the binder. Syntax that GQL does not define fails with GQLSTATUS 42001 and the position and expected tokens (BR-GQL-001), including index-management syntax (FR-API-004) and permission statements (BR-AUTH-001).
```

**-tr**
```text
* **Module** `src/query/` (`lexer`, `parser`, `ast`): case-insensitive lexer, recursive descent per `GQL.g4` rule (each function names its rule), Pratt for expressions; spans on every node; parameters never inlined.
* **Sources:** COPY the AST of miniGU @ `fc2ccf5` (Apache-2.0) `minigu/gql/parser/src/ast/*.rs` (1 311 lines); PORT its rule functions `src/parser/impls/*.rs` (4 841) from `winnow` combinators to plain recursive descent; do not copy its `logos` lexer or the `unsafe` of `lexer.rs:838-867` and `error.rs:205`. COPY from datafusion-sqlparser-rs @ `14cbf75` (Apache-2.0) `Location`/`Span` (`src/tokenizer.rs:546-760`), `RecursionCounter`/`DepthGuard` (`src/parser/mod.rs:95-140`) and the precedence loop of `parse_subexpr` (`:1424-1460`). PORT zu @ `a821319` `crates/zu-common/tests/keyword_table.rs` to generate the keyword table from `GQL.g4:3276-3661`. Study Grafeo `query/gql/lexer.rs`, `parser.rs` @ `3c23dbe`.
* **Decisions applied:** #39 points 1 to 3 (keywords, identifier tables with a drift test, lowest-alternative ambiguity resolution, recorded in TS-19); #38 group 1.
* **Errors:** 42001 with position and expected-token set (#40's `Error`).
* **Differential tests** against the #36 oracle: the 14 samples; every GQL text in `lpg`'s tests (collected by a test); the 143 miniGU snapshot inputs (attribution kept); generated sentences and near-miss mutants of three seeds.
* **Fuzz targets:** lexer and parser, no panic, no stack overflow (depth guard).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. The differential tests report 0 differences on the samples, every test text, the miniGU inputs, and at least 100 000 generated sentences and 100 000 mutants for each of three seeds (seeds printed; `LPG_SEED` overrides).
* Coverage per grammar rule (`cargo llvm-cov`, per function): every rule function is executed by the differential corpus; the table is in `docs/evidence/lab-sprint-3/`.
* Each fuzz target runs 10 000 000 inputs without a crash (`-runs=10000000`); a 100 000-deep nested expression fails with a GQLSTATUS error, not a stack overflow.
* Tests show 42001 with position and expected tokens for a misspelt keyword; `CREATE INDEX …` and `GRANT …` fail with 42001.
* Swapping the resolution of one ambiguous rule to its second alternative makes a differential or AST test fail (mutation, then restored).
* The keyword and identifier tables pass their drift tests; TS-19 records each ambiguity resolution.
```

---

### #42 TASK, priority 8, depends on: #37, #38, #40

**Title:** Implement expression evaluation: three-valued logic, values and functions

**-fr**
```text
**Every value expression of the approved feature set must evaluate as GQL defines it, with three-valued logic and the approved value semantics** (TS-20 "Predicates use three-valued logic"; BR-GQL-002 choices of 2026-10-07 and of #38; FR-TYPE-001 GV39–GV41 temporal values). Errors are GQLSTATUS data exceptions from the ISO insert.
```

**-tr**
```text
* **Bound expression form** (`src/query/expr.rs`): typed nodes with slots, built by the binder (#43); evaluated over a batch of `Value` columns.
* **Three-valued logic:** `AND`, `OR`, `XOR` (if GE07 is approved), `NOT`, `IS [NOT] TRUE|FALSE|UNKNOWN|NULL`; NULL as UNKNOWN in boolean context; filters keep only TRUE.
* **Comparison and order:** reuse `src/types/mod.rs:206-414` (`group`, `float_cmp`, `total_cmp`), which implement the approved rules.
* **Arithmetic and functions** of the approved set with the #38 group 2 choices; overflow, division by zero and invalid arguments raise the insert's class-22 codes.
* **Function table:** one row per function with signature, determinism and kernel; constant folding of deterministic calls on literals. PORT zu @ `a821319` (Apache-2.0) `crates/zu-query/src/functions.rs` (2 835 lines), adapted to `Value` and `jiff`. Temporal functions through `jiff` (TS-21).
* **Aggregate accumulators** (count, sum, avg, min, max, collect) used by #45.
* Study: Oxigraph `lib/spareval/src/expression.rs` @ `fda6793`; Grafeo `operators/filter.rs`, `accumulator.rs` @ `3c23dbe`.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg query::expr` passes. Tests show that:
  * the truth tables of every logical operator hold for all combinations of TRUE, FALSE and UNKNOWN;
  * each operator and function of the approved set returns NULL for a NULL argument where GQL requires it (table-driven);
  * `1 = 1.0` is TRUE, `1 < 'a'` is NULL, and NULL sorts last ascending (BR-GQL-002 acceptance);
  * integer overflow and division by zero fail with their ISO class-22 codes;
  * each temporal function of GV39–GV41 returns the value `jiff` computes independently in the test.
* Coverage per function kernel (`cargo llvm-cov`): every kernel of the table is executed; the table is in `docs/evidence/lab-sprint-3/`.
```

---

### #43 TASK, priority 8, depends on: #41, #42

**Title:** Implement the binder: scopes, types, path-pattern rules and feature gates

**-fr**
```text
**One binder pass must turn the AST into a typed form in which every variable is a slot and every construct outside the approved feature set fails with the "feature not supported" code** (TS-20 Binder; FR-GQL-002). Type and function combinations that `GQL.g4` accepts but GQL forbids are rejected here (TS-19 Narrowing). Parameters are never inlined.
```

**-tr**
```text
* **Scopes** per GQL composition (statement blocks, `NEXT`, `YIELD`, `RETURN`, subqueries); each variable has a slot, a kind (node, edge, path, value, graph, binding table), a degree (singleton, group, conditional singleton) and a static or dynamic type.
* **Path patterns:** normalisation and the conditional-singleton join rule ([GPML] §4–§6, M-QE §Q2); the termination rule applies only if #37 approved quantifiers.
* **Catalog resolution** of labels, edge types and property keys to tokens (`src/catalog/mod.rs:874-905`), graphs and schemas by path (`:1035`), procedures by #39 point 6.
* **Feature gates:** a table of all 228 codes of the features insert, generated with a drift test; each code is "supported" or a gate that fails with the #39 point 5 code. PORT zu @ `a821319` (Apache-2.0) `crates/zu-cli/src/statement/features.rs` (generated) and its generator `crates/zu-cli/tests/statement.rs`.
* **Sources:** PORT miniGU @ `fc2ccf5` (Apache-2.0) `minigu/gql/planner/src/binder/*.rs` (about 2 000 lines), matching the AST copied in #41. Study: zu `crates/zu-query/src/binder.rs`; frogql @ `b5dd0f0` (MIT) `src/typing/checker.rs`; Kuzu @ `89f0263` (MIT) `src/binder/`.
* **Decisions applied:** #38 group 3.
* **Fuzz target:** parse then bind generated sentences (#36 generator); no panic.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * every code of the features insert is in the gate table (drift test), and one statement per unsupported construct class fails with the "feature not supported" code;
  * one statement per approved construct binds; an undefined variable, a duplicate binding and an implicit join on a conditional singleton fail with their codes;
  * a parameter stays a parameter in the bound form (no literal is created from it).
* The binder fuzz target runs 10 000 000 inputs without a crash.
* Coverage per binder function (`cargo llvm-cov`) is recorded in `docs/evidence/lab-sprint-3/`, with each unexecuted function named and the reason.
```

---

### #44 TASK, priority 8, depends on: #43

**Title:** Implement the planner with index seeks, join ordering and EXPLAIN

**-fr**
```text
**The planner must choose index seeks for indexed equality and range predicates and order pattern expansions by estimated cost, and EXPLAIN must return the plan without executing it** (TS-20 Planner; FR-OBS-005; FR-IDX-001 and FR-TYPE-005 through queries, the latter as #37 decides for OQ-006).
```

**-tr**
```text
* **Logical plan** from the bound form; rewrites: predicate and projection pushdown, constant folding; limit pushdown only if GQ13 is approved.
* **Index selection** on ONLINE indexes only: `index_scan`, `index_within` (`src/index/mod.rs:1178, 1241`).
* **Join order:** exact dynamic programming up to 7 expansions, greedy beyond; cost = sum of intermediate cardinalities from TS-17 statistics (`src/catalog/stats.rs`), 0.01 for equality, 0.1 otherwise. COPY Grafeo @ `3c23dbe` (Apache-2.0) `crates/grafeo-engine/src/query/optimizer/join_order.rs` (1 378 lines, DPccp), adapted to this cost; PORT from Kuzu @ `89f0263` (MIT) the threshold and greedy fallback (`src/planner/plan/plan_join_order.cpp:136-160`, `src/include/planner/subplans_table.h:11`) and `join_order/cardinality_estimator.cpp`.
* **Physical operators** of TS-20: `NodeById`, `LabelScan`, `IndexSeek`, `Expand`, `ExpandInto`, plus filter, project, join, aggregate, sort, distinct and data-modifying operators of the approved set.
* **EXPLAIN:** the operator tree with estimated rows and details (index, direction, types), in the result type of #39 point 12; nothing executes.
* Study: Grafeo `optimizer/cardinality.rs`, `cost.rs`; miniGU `planner/src/optimizer`.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * EXPLAIN of a statement that inserts a node returns a plan, and no node exists afterwards (FR-OBS-005);
  * an indexed equality and an indexed range use `IndexSeek` on an ONLINE index and not on a POPULATING one;
  * for seeded graphs and queries, results with and without each of the five index kinds are equal;
  * for patterns of 2 to 7 expansions the chosen order has the minimum cost found by enumerating every order; at 8 the greedy path is taken (counted);
  * estimates read the statistics: after inserting 1 000 `L` nodes the `LabelScan` estimate is 1 000.
* Coverage per planner rule (`cargo llvm-cov`) is recorded in `docs/evidence/lab-sprint-3/`.
```

---

### #45 TASK, priority 8, depends on: #39, #44

**Title:** Implement the pull-based batch executor, its operators and PROFILE

**-fr**
```text
**Statements must execute pull-based, one batch of at most 1 024 rows at a time, on one thread, with blocking operators reserving memory in chunks, and PROFILE must report measured statistics per operator** (TS-20 Execution and EXPLAIN/PROFILE; FR-API-010; NFR-PERF-007; FR-OBS-006; FR-LIM-001, FR-LIM-003). Data-modifying statements of the approved set (GD01 if approved) run in the caller's transaction.
```

**-tr**
```text
* **Operators** of #44's plan over columns of `Value`: scans and seeks over lpg's trees (`src/storage/graph.rs:307-607`, `src/index/mod.rs:1178-1299`), filter, project, joins, aggregation (#42 accumulators), sort, distinct, insert, set, remove, delete.
* **Memory:** sort, aggregation and hash build reserve in chunks of #39 point 8 through `MemoryAccountant::try_reserve` (`src/storage/memory.rs:145`); a refused reservation fails the statement with the memory code and aborts the transaction.
* **Cursor** owned and `Send`, ready for `spawn_blocking` (#39 point 4); a status check per batch and every K iterations (#39 point 9), wired by #49.
* **PROFILE:** COPY Grafeo @ `3c23dbe` (Apache-2.0) `crates/grafeo-core/src/execution/profile.rs` (187 lines), `parking_lot` removed; counters rows, batches, time per batch, entries read, buffer hits and misses, peak memory; normal plans hold no decorator.
* **Sources:** PORT the algorithms of Grafeo `crates/grafeo-core/src/execution/operators/` `aggregate.rs`, `sort.rs`, `join.rs`, `expand.rs`, `distinct.rs`, `limit.rs` (pull trait `mod.rs:302-322`). Study: DataFusion @ `54a4bbf` `physical-plan/src/sorts/sort.rs`; Kuzu `src/processor/operator/`.
* **Reference evaluator** in test code: a brute-force evaluator over a dump of the snapshot, the oracle for seeded random queries.
* **Internal entry point** `run(statement, transaction)` for tests until #47.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * for 5 000 seeded queries over seeded graphs, results equal the reference evaluator's (seed printed);
  * a query of 1 000 000 rows returns its first batch after its scan produced at most 1 024 rows (scan counter) (FR-API-010);
  * across 1 000 batches a per-statement counter of running operators never exceeds 1 (NFR-PERF-007);
  * profiling a query that returns 10 rows returns the rows and a final operator reporting 10 rows; each operator's entries read equals an independent B+tree read counter (FR-OBS-006);
  * a sort and an aggregation over an input larger than the cap fail with the memory GQLSTATUS, the transaction is aborted, and the accountant never exceeds the cap.
* `heaptrack` peak heap and peak reservation of each blocking operator on the same inputs are recorded in `docs/evidence/lab-sprint-3/`.
* Coverage per operator (`cargo llvm-cov`) is recorded there.
```

---

### #46 TASK, priority 8, depends on: #45

**Title:** Implement catalog statements, multi-graph queries and closed graph types

**-fr**
```text
**Catalog-modifying statements of the approved set must run in transactions that may also change data, queries may read several graphs, and closed graph types must reject what they do not describe** (FR-GQL-004 GG01 and GG02; FR-CAT-004 GP18; FR-CAT-003; FR-CAT-001 through statements; TS-17 Closed graph types). No statement creates a directory (FR-CAT-005).
```

**-tr**
```text
* **Statements** of the approved set (planned: `CREATE`/`DROP` `SCHEMA`, `GRAPH`, `GRAPH TYPE`) over `src/catalog/mod.rs:1119-1211`, each joining the caller's transaction (GP18), each recording the session principal as owner.
* **Multi-graph:** the graph reference of the approved constructs (planned: GQ01 `USE`, GT03) resolved per statement part.
* **Closed graph types:** at the end of each `INSERT`, `SET`, `REMOVE` on a graph of a closed type, every element the statement wrote must match an element type (labels, edge endpoints, property names and types); a failure fails the statement with the #39 point 5 code and leaves none of its writes; open types run no check. PORT zu @ `a821319` (Apache-2.0) `GraphType` matching (`crates/zu-zu1/src/catalog.rs:298-305, 476-489, 977`; `crates/zu/src/declare.rs:335-380`). Record the matching rules in TS-17 as provisional (OQ-005).
* **Decisions applied:** #38 group 3 (IE006, IE007 mixed-mode transactions; IL003).
* Study: frogql @ `b5dd0f0` (MIT) `src/typing/`.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * a closed graph type rejects an inserted node whose label it does not allow with a GQLSTATUS error, and the same statement succeeds on an open graph (FR-GQL-004);
  * a transaction that creates a graph and inserts nodes into it, then rolls back, leaves neither (FR-CAT-004);
  * one query reads two graphs and its result combines both (FR-CAT-003);
  * a directory, a schema in it, a graph type and two graphs created in one transaction are present after reopen (FR-CAT-001, statement part);
  * a failed closed-type check leaves none of the statement's writes; a statement on an open graph calls the check 0 times (counter);
  * no statement of the grammar creates a directory (binder test over the catalog-modifying rules).
* Coverage of the catalog-statement and type-check code (`cargo llvm-cov`) is recorded in `docs/evidence/lab-sprint-3/`.
```

---

### #47 TASK, priority 8, depends on: #39, #46

**Title:** Implement the async API: database, session, transaction and result stream

**-fr**
```text
**The application must run GQL end to end through an asynchronous Rust API on tokio** (TS-02; `interfaces.md` Public API shape; FR-API-001, FR-API-002, FR-API-003, FR-API-010; FR-DB-002, FR-DB-008; FR-SES-001 to FR-SES-004; FR-GQL-005; FR-TX-004 and BR-TX-001 Rust parts; NFR-PLAT-003). Backup, restore, migration and the integrity check are API calls (FR-OPS-001, FR-OPS-002, FR-OPS-010, FR-REC-004, FR-REC-010); FR-OPS-008 is verified in #48, once indexes and constraints can be created through the API.
```

**-tr**
```text
* **Dependency:** `tokio` 1.53.2 (TS-31; crates.io 2026-10-03), features `rt`, `time` only.
* **Handles** (`src/api/`): `Database` (`create(location, storage, memory_cap, owner, options)`, `open`, `close(timeout)` with 10 s default, `session(principal, home)`, `backup`, `restore`, `migrate`, `check_integrity`), a cheap clone of one engine; `Session` (`execute(gql, parameters)`, `begin(options)` with isolation, limits added by #49); `Transaction` (`execute`, `commit`, `rollback`); `ResultStream` (`async fn next()` one row, `summary()` per #39 point 11).
* **Boundary:** each pull moves the cursor into `tokio::task::spawn_blocking`, fills one batch, returns it; no thread held while the caller does not pull; owned transactions (#39 point 4).
* **Sessions:** atomic busy flag (FR-SES-004, code per #39 point 5); home schema and graph from the caller or the defaults `/public` and `/public/lpg`; UTC.
* **GQL text** session and transaction commands reach the same functions as the API (FR-GQL-005), per #39 point 15.
* **Close:** waits for active transactions up to the timeout, then aborts them.
* Remove the Sprint 3 dead-code allowances of `src/lib.rs`.
* Study: Turso @ `8cc059b` (MIT) `bindings/rust/src/{rows,connection,transaction}.rs`; neo4rs `RowStream`; tokio `spawn_blocking` documentation. Nothing to copy (task #3 plan §4.10).
* Method: reference study per critical decision as `DECISION`; tests measured with `cargo nextest`, waste removed without weakening assertions; graph data in `/tmp/lpgram/<run>/`.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes; the API tests run on a `current_thread` and on a `multi_thread` runtime. They show the acceptance of FR-API-001, FR-API-002, FR-API-010 (first of 1 000 000 rows before the rest is produced), FR-DB-002, FR-DB-008 (the default is 10 s; a test passes 200 ms and checks the abort), FR-SES-001 to FR-SES-004, FR-GQL-005, FR-TX-004 (write skew under snapshot isolation commits both), BR-TX-001 (one execution per failed transaction), FR-OPS-001, FR-OPS-002, FR-REC-004 and FR-REC-010.
* A `ResultStream` dropped after one row ends its transaction's read, and the accountant returns to its earlier level.
* `cargo build -p lpg` without features succeeds and no `bolt` symbol exists (FR-API-003).
* Every public operation that performs I/O or waits is `async` (signature test).
* Coverage of `src/api` (`cargo llvm-cov`) is recorded in `docs/evidence/lab-sprint-3/`.
```

---

### #48 TASK, priority 8, depends on: #47

**Title:** Implement the authorisation trait and the index, constraint and directory API

**-fr**
```text
**Every catalog and data action must be authorised by the application before it happens, with the object's owner in the request, and indexes, constraints and directories are managed through the API only** (TS-23; FR-AUTH-001, FR-AUTH-002, BR-AUTH-001; FR-DB-005 owner acceptance; FR-API-004; FR-CAT-005; FR-IDX-001 creation part; FR-OPS-008 through the API).
```

**-tr**
```text
* **Trait** (`src/auth/`), dyn-compatible, boxed futures: `authenticate(scheme, credentials)`, `impersonate(principal, target)`, `authorise(principal, action, object, owner)`; Neo4j action names of TS-23.
* **Statements:** after planning, the crate collects every `(action, object)` pair the plan can perform and asks once per pair before execution; a refusal fails the statement before any effect.
* **API calls** ask once per call; `create` asks nothing (FR-AUTH-001).
* **Owners** from the catalog (`src/catalog/mod.rs`), passed with each request.
* **Management API** on `Session`, each call authorised, recording its owner, joining a transaction: create and drop the five index kinds and the four constraint kinds with the FR-IDX-002 property types (`src/index/mod.rs:97-201, 1378`); create a directory (FR-CAT-005).
* Nothing reusable exists (task #3 plan §4.15; GraphLite rejected: own grant syntax). Study: Neo4j privilege documentation (D-PRIV).
* Method: reference study per critical decision as `DECISION`; tests measured with `cargo nextest`, waste removed without weakening assertions; graph data in `/tmp/lpgram/<run>/`.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * an authoriser that refuses "create node" on g1 for p1 fails p1's insert with a GQLSTATUS error and no node exists (FR-AUTH-001);
  * creating a database makes no request; a graph created by p1 is named with owner p1 in every later request; the default schema and graph name the `create` owner (FR-AUTH-002, FR-DB-005);
  * a counting authoriser receives each `(action, object)` pair once per statement;
  * an index created through the API exists; one of each of the five kinds is created (FR-API-004, FR-IDX-001 creation);
  * a directory is created through the API and the catalog lists it (FR-CAT-005);
  * no GQL statement grants or revokes permissions (`GRANT`, `REVOKE` fail with 42001; BR-AUTH-001);
  * a database with two graphs, an index and a constraint, backed up and restored through the API, holds all four (FR-OPS-008).
* Coverage of `src/auth` and the management API (`cargo llvm-cov`) is recorded in `docs/evidence/lab-sprint-3/`.
```

---

### #49 TASK, priority 8, depends on: #48

**Title:** Implement cancellation, resource limits, transaction listing and termination

**-fr**
```text
**One atomic status per transaction, set by API cancel, termination and a timer, must stop the running statement with a GQLSTATUS error and abort its transaction; memory, time and result-size limits must fail the statement that crosses them** (TS-20 Limits and cancellation; sprint DECISION 11 synergy; FR-API-007, FR-API-008 Rust part, FR-API-009; FR-LIM-001, FR-LIM-002, FR-LIM-003, FR-LIM-005, FR-LIM-007 through statements; FR-OBS-003, FR-OBS-004).
```

**-tr**
```text
* **Status:** one atomic per transaction with a reason (cancelled, terminated, timed out); checked by operators per batch and every K iterations (#39 point 9).
* **Time limit:** a `tokio` timer from the start of the transaction (FR-API-007); session and transaction limits as #39 point 10 decides.
* **Memory limit** per transaction under the database cap; **row and byte limits** counted at the result sink.
* **Transactions:** `list_transactions` (principal, status, elapsed time, current statement, `tx_metadata`) and `terminate_transaction` on `Session`; a principal sees and terminates its own; others need the authoriser's consent (#48).
* **`loom` model:** a cancel racing a commit: either the commit completes and the cancel has no effect, or the transaction aborts; never both.
* Study: zu @ `a821319` (Apache-2.0) `crates/zu-common/src/interrupt.rs`; Memgraph `operator.cpp:510-528` (BSL, study only).
* Method: reference study per critical decision as `DECISION`; tests measured with `cargo nextest`, waste removed without weakening assertions; graph data in `/tmp/lpgram/<run>/`.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests, synchronised with pause points and never sleeping for ordering, show:
  * FR-API-007 with the limit scaled to 200 ms: the second statement fails with a GQLSTATUS error after the transaction's limit, measured from its start;
  * FR-API-008 and FR-API-009: a cancelled statement fails, its transaction aborts, and its written node is never visible;
  * a cancel during a long expansion, sort and aggregation stops each within K iterations (counter);
  * FR-LIM-001 and FR-LIM-003: a statement over the memory limit fails and earlier writes are invisible; the accountant never exceeds the cap;
  * FR-LIM-002: a row limit of 10 fails an 11-row query; with no limit all rows return; a byte limit fails at its boundary;
  * FR-LIM-005 and FR-LIM-007 acceptance through GQL statements, with GQLSTATUS codes;
  * FR-OBS-003 (p1 sees only its own with a refusing authoriser) and FR-OBS-004 (a terminated transaction's statement fails and its writes are discarded).
* The `loom` model passes and fails when the commit ignores the status (mutation).
* The stress profile runs the new concurrency tests with no failure.
```

---

### #50 TASK, priority 8, depends on: #49

**Title:** Implement the procedure and function registry, CALL and the show procedures

**-fr**
```text
**Applications register typed procedures and functions on a database handle and invoke them with the standard `CALL`; read-only routines cannot modify data; four built-in procedures list transactions, indexes, constraints and the catalog** (TS-24; FR-API-005, FR-API-006; FR-PROC-001 to FR-PROC-004; FR-LIM-006; listing parts of FR-CAT-001, FR-CAT-005, FR-IDX-001; FR-OPS-010; FR-TYPE-005 if #37 chose built-in point procedures for OQ-006). Named calls are feature GP04 if #37 approved it.
```

**-tr**
```text
* **Registry** per handle, not persisted: name, parameters with defaults, result columns, mode `ReadOnly` or `ReadWrite`; functions are `CALL`-able routines returning one row (#39 point 6).
* **Calling:** synchronous implementations returning row iterators on the statement's thread, with a context giving the snapshot, the cancellation status (#49) and the memory tracker; returned rows charged to the cap; memory the routine allocates itself is not (FR-LIM-006).
* **Read-only:** the read-only context has no write methods; the binder rejects a `ReadWrite` routine where only reads are allowed; nested GQL through a read-only context fails if it modifies data.
* **Built-ins:** `show_transactions`, `show_indexes`, `show_constraints`, `show_catalog` with the FR-PROC-004 columns; each authorised as `SHOW TRANSACTION`, `SHOW INDEX`, `SHOW CONSTRAINT` (TS-23).
* **Point procedures**, only if #37 chose OQ-006 option (a): constructors, distance, and a distance-within search that uses the point index (FR-TYPE-005).
* **Sources:** PORT zu @ `a821319` (Apache-2.0) `crates/zu-query/src/procedures.rs` (304 lines) descriptor table. Study: Neo4j `Mode`, `ProcedureCaller` (GPL); Memgraph `mg_procedure_impl.cpp` (BSL).
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions; graph data in `/tmp/lpgram/<run>/`.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show the acceptance of FR-API-005, FR-API-006, FR-PROC-001 (one call with the request's arguments), FR-PROC-002 (a read-write routine's node is gone after rollback; a read-only routine that writes fails with a GQLSTATUS error), FR-PROC-003 (after reopen the call fails until registered again) and FR-PROC-004 (one graph, index, constraint and transaction are each listed with the specified columns).
* FR-LIM-006: a routine that allocates 64 MiB on its own and returns rows leaves the accountant's peak below 64 MiB, while the returned rows are charged (per-thread heap from `allocation-counter`).
* `show_catalog` lists a directory, a schema, a graph type and two graphs (FR-CAT-001, FR-CAT-005); `show_indexes` lists five indexes with their kinds (FR-IDX-001).
* The built-in list holds no backup or restore procedure (FR-OPS-010).
* If #37 chose OQ-006 option (a): for seeded points in the four CRSs, the distance-within procedure returns the same set with and without the point index (FR-TYPE-005).
* Coverage of the registry and built-ins (`cargo llvm-cov`) is recorded in `docs/evidence/lab-sprint-3/`.
```

---

### #51 TASK, priority 8, depends on: #47

**Title:** Implement query metrics and tracing events without parameter values

**-fr**
```text
**The application must receive the crate's metrics through its sink and `tracing` events for every request, and no event may contain a parameter value** (TS-24 Metrics and Events; FR-OBS-001 Rust-API categories; FR-OBS-002; FR-OPS-005; FR-OPS-006). Bolt metrics are Sprint 6.
```

**-tr**
```text
* **Dependency:** `tracing` 0.1.44 (TS-31).
* **Events:** one span per request with the query text and `tx_metadata`; levels, targets and fields per #39 point 14; parameter values never recorded, in fields, messages or `Debug` output of errors.
* **Metrics:** query execution, data counts and indexes categories added to `src/obs/mod.rs` with the names of #39 point 13; emitted by the executor, the API and the index code.
* **Test subscriber:** a minimal `tracing::Subscriber` in test code that records every field and message; no `tracing-subscriber` dependency.
* Study: Turso @ `8cc059b` (MIT) span placement in `core/`; Neo4j "Metrics reference". Nothing to copy (task #3 plan §4.14).
* Method: reference study per critical decision as `DECISION`; tests measured with `cargo nextest`, waste removed without weakening assertions.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes. Tests show that:
  * a subscriber receives events for a query (FR-OPS-005);
  * with the parameter value "secret-42", an event carries the query text and no recorded field or message contains "secret-42", also when the statement fails (FR-OBS-002);
  * a recording sink receives at least one metric of each FR-OBS-001 category except Bolt during a workload covering them (FR-OBS-001);
  * committing transactions produces transaction metrics (FR-OPS-006).
* `cargo tree -e normal --depth 1` adds only `tracing` and its dependencies; `cargo deny check licenses` exits 0.
```

---

### #52 TASK, priority 8, depends on: #46, #49, #50, #51

**Title:** Implement the conformance runner and pass the approved scenarios

**-fr**
```text
**Every scenario of the approved feature set must pass through the public API and name its clause or feature code, and the crate must publish the list of supported feature identifiers** (TS-30 Semantics; NFR-VER-001; FR-GQL-001, FR-GQL-002; exit criteria 1 and 2 of sprint DECISION 12). The test artefacts keep their licences and attribution unchanged (NFR-VER-002).
```

**-tr**
```text
* **Runner** (`cfg(test)`): COPY the feature scanner of cyrs @ `6a6bb34` (MIT OR Apache-2.0) `crates/cyrs-tck/tests/full.rs:88-360` (scenarios, outlines, examples, docstrings), extended to every step the selected scenarios use (counts in task #3 plan §4.16). Own parts: the expected-value parser of TCK result tables, row comparison in order and in any order, side-effect counters, error steps mapped to GQLSTATUS.
* **Artefacts:** opengql/tck @ `e0700a7` and the openCypher TCK scenarios at `677cbaf` selected by #37's rule, vendored under `tests/tck/` with their `LICENSE` and `NOTICE`; lpg's own scenarios for approved features without TCK coverage, each naming its clause or feature code; gql-compat cases only as #37 decided.
* **Feature list:** the supported identifiers generated from #43's gate table into the document FR-GQL-002 names; a drift test.
* **Parser check:** every scenario text also goes through the #41 differential test against the oracle.
* Method: reference study per critical decision as `DECISION`; copies recorded in file headers and a `NOTE`; tests measured with `cargo nextest`, waste removed without weakening assertions; graph data in `/tmp/lpgram/<run>/`.
```

**-ac**
```text
* `cargo nextest run -p lpg` passes, with every selected scenario run; each scenario names its clause or feature code (test over the scenario set).
* The report in `docs/evidence/lab-sprint-3/` lists counts per artefact: selected, passed, and skipped with the gate code that excludes each skipped scenario.
* The published feature list equals the gate table (drift test).
* The oracle differential reports 0 differences on every scenario text.
* A scenario edited to expect a wrong row fails the runner (mutation, then restored).
```

---

### #53 TASK, priority 7, depends on: #39, #52

**Title:** Verify every Sprint 3 exit criterion and define the micro-workloads

**-fr**
```text
**The sprint ends only on evidence.** Every exit criterion of sprint DECISION 12 that is not timing must hold on the final code (sprint UPDATE 55). The project's own micro-workloads are defined and their results checked (NFR-PERF-002); their measurement and unverified point 8 follow the user's answer to task #39 point 16. Unverified point 1 is restated with the provisional choices recorded.
```

**-tr**
```text
* **Full run:** `cargo nextest run -p lpg` with `LPG_LAB_EXT4` and `LPG_LAB_XFS` set (Sprint 1 and 2 suites included), the stress profile, the `loom` and `shuttle` runs, the oracle differential on samples, test texts, scenario texts and generated sentences, each fuzz target for its run count.
* **Gates:** `cargo +1.89 build`, `cargo deny check licenses`, `cargo clippy --all-targets -- -D warnings`, `forbid(unsafe_code)`, the FR-STO-002 grep of Sprint 1.
* **NFR-PERF-007:** the operator concurrency counter of #45 over every scenario.
* **Micro-workloads:** defined from the M-LAYOUT harness and the LDBC SNB Interactive short reads, written in approved GQL, with expected results; PROFILE counters recorded (rows, batches, entries read, buffer hits and misses, peak memory).
* **Record** in `docs/evidence/lab-sprint-3/report.md`: each exit criterion with its tests; coverage per grammar rule and per operator; differential totals; fuzz runs; `heaptrack` peaks per blocking operator; PROFILE counters; the deferred timing items.
* **Spec:** restate unverified point 1 in `quality-and-testing.md` with the approved feature set and items; update point 8 per #39 point 16.
* Method: tests measured with `cargo nextest`; graph data in `/tmp/lpgram/<run>/`, lab directories for flush-dependent suites.
```

**-ac**
```text
* The full run and the gates exit 0; outputs saved in `docs/evidence/lab-sprint-3/raw/`.
* The report maps each exit criterion of sprint DECISION 12 (minus timing) to passing tests, and lists the deferred timing items as Sprint 6.
* Each micro-workload is listed with its GQL text and expected result, and passes; its PROFILE counters are in the report.
* `quality-and-testing.md` point 1 names the approved feature set and items; point 8 reflects #39 point 16.
* `grep -rn 'forbid(unsafe_code)' src/lib.rs` matches.
```

## 6. Execution order and dependency edges

Order for `rmp sprint reorder 3`: **#3, #36, #37, #38, #39, #40, #41, #42, #43, #44, #45, #46, #47, #48, #49, #50, #51, #52, #53.**

| Task | Depends on | Why |
|---|---|---|
| #36 | none | first task after planning (task #3 `-fr`) |
| #37 | none | user questions; must precede every engine task (sprint DECISION 11) |
| #38 | #37 | asks only the items the approved set reaches |
| #39 | none | specification only; runs while #37 waits on the user |
| #40 | #36, #37, #39 | codes of point 5; no engine task before the approval (sprint DECISION 11) |
| #41 | #36, #38, #39, #40 | oracle and generator; lexical items; points 1–3; error type |
| #42 | #37, #38, #40 | approved functions and items; error codes |
| #43 | #41, #42 | AST and expression form |
| #44 | #43 | bound form |
| #45 | #39, #44 | plan; points 4, 8, 9, 12 |
| #46 | #45 | statements execute through the executor |
| #47 | #39, #46 | API runs catalog statements (FR-API-002); points 4, 11, 15 |
| #48 | #47 | trait wired into sessions |
| #49 | #48 | listing and termination of others' transactions need the authoriser |
| #50 | #49 | `show_transactions` needs #49's registry; context needs the status |
| #51 | #47 | events per API request |
| #52 | #46, #49, #50, #51 | scenarios cover all of them |
| #53 | #39, #52 | runs on the final code; point 16 |

If #37's approval adds optional features outside the recommended set (for example quantified paths or path selectors), a task for them is created after #45 and before #52, with Grafeo's path operators (§4.7) as its port candidate.

## 7. Traceability

Every id of scope comment #10 and exit comment #12, every risk of #11 and every laboratory item of #13 maps to at least one task; no task maps to an item outside them (#36 maps to #13 and TS-30, as Sprint 2's #24 did).

| Item | Tasks |
|---|---|
| TS-02 | #47 |
| TS-17 (closed graph type check) | #46 |
| TS-19 | #39, #41 |
| TS-20 | #39, #42, #43, #44, #45, #49 |
| TS-23 | #48 |
| TS-24 | #50, #51 |
| TS-30 | #36, #41, #52 |
| `interfaces.md` Public API shape, `Error` | #40, #47 |
| FR-API-001, FR-API-002, FR-API-003, FR-API-010 | #47 (FR-API-010 also #45) |
| FR-API-004 | #41, #48 |
| FR-API-005, FR-API-006 | #50 |
| FR-API-007, FR-API-008 (Rust part), FR-API-009 | #49 |
| FR-DB-002, FR-DB-008 | #47 |
| FR-SES-001, FR-SES-002, FR-SES-003, FR-SES-004 | #47 |
| FR-AUTH-001, FR-AUTH-002, BR-AUTH-001 | #48 (BR-AUTH-001 also #41) |
| FR-GQL-001, FR-GQL-002 | #37, #43, #52 |
| FR-GQL-004 (GG01, GG02) | #46 |
| FR-GQL-005 | #47 |
| BR-GQL-001 | #41, #52 |
| BR-GQL-002 | #38 (applied in #41, #42, #43, #46, #47) |
| FR-CAT-001 | #46, #50 |
| FR-CAT-003 | #46 |
| FR-CAT-004 (GP18) | #46 |
| FR-CAT-005 | #46, #48, #50 |
| FR-IDX-001 | #44, #48, #50 |
| FR-TYPE-005 | #37 (OQ-006), #44, #50 |
| FR-TYPE-001 (GV39, GV40, GV41) | #42 |
| FR-PROC-001, FR-PROC-002, FR-PROC-003, FR-PROC-004 | #50 |
| FR-LIM-001, FR-LIM-002, FR-LIM-003, FR-LIM-005, FR-LIM-007 | #49 (FR-LIM-001, FR-LIM-003 also #45) |
| FR-LIM-006 | #50 |
| FR-OBS-001 (Rust part), FR-OBS-002 | #51 |
| FR-OBS-003, FR-OBS-004 | #49 |
| FR-OBS-005 | #44 |
| FR-OBS-006 | #45 |
| FR-OPS-001, FR-OPS-002 | #47 |
| FR-OPS-008 | #48 |
| FR-OPS-005, FR-OPS-006 | #51 |
| FR-OPS-010 | #47, #50 |
| FR-REC-004, FR-REC-010 | #47 |
| FR-TX-004 (Rust API part), BR-TX-001 (Rust part) | #47 |
| NFR-VER-001 | #52 |
| NFR-VER-002 | #36, #41, #52 |
| NFR-PERF-002 (micro-workloads defined) | #39 (item 16), #53 |
| NFR-PERF-007 | #45, #53 |
| NFR-PLAT-003 | #47 |
| Risk OQ-005 | #37, #53 |
| Risk BR-GQL-002 | #38 |
| Risk FR-GQL-006 | #37 |
| Risk OQ-006 | #37 |
| Unverified point 1 (restated) | #53 |
| Unverified point 8 | #39 (item 16), #53 |
| Laboratory #13: toolchain refresh | #36 |
| Laboratory #13: differential parsing against the ANTLR oracle | #36, #41, #52 |
| Laboratory #13: sentence generator from `GQL.g4` | #36, #41 |
| Laboratory #13: `cargo-fuzz` on lexer, parser, binder | #36, #41, #43 |
| Laboratory #13: `cargo-llvm-cov` per grammar rule and per operator | #36, #41, #45, #53 |
| Laboratory #13: `heaptrack` per batch and operator; reservations against memory | #36, #45, #53 |
| Laboratory #13: PROFILE output on the micro-workloads | #45, #53 |

**Deferred to Sprint 6 (timing; sprint UPDATE 55):** flame graphs split by phase; parse time against M-PARSE; `spawn_blocking` hand-off cost against M-TOKIO; tokio runtime metrics; cancellation latency; the measurement of the micro-workloads (NFR-PERF-002) and the closing of unverified point 8, subject to #39 item 16.

## 8. Checks run on this document

The `text` blocks of §5 were checked by script after writing (result appended below): character count per field, and no line starting with `- `.

Result (2026-10-08): 54 fields, longest 2275 characters (limit 4 096), 0 lines starting with `- `; the 18 titles are 64–77 characters, plain text. A search for open-ended wording (`etc.`, `as needed`, `if necessary`, `where appropriate`, `TBD`) found 0.

## 9. Tool status for task #3's last acceptance criterion

Measured 2026-10-08 after the updates below.

| Tool | Version | Command | State |
|---|---|---|---|
| rustup | 1.29.1 (d95a37b6a 2026-08-13) | `rustup self update` | "rustup unchanged" |
| Rust stable | 1.99.0 (b940084d7 2026-09-28) | `rustup update stable nightly` | up to date |
| Rust nightly | 1.101.0-nightly (1d81eb4ad 2026-10-07) | same | **updated** from 8d1a76430 of 2026-10-06 |
| Toolchain 1.89 (MSRV) | 1.89.0 (29483883e 2025-08-04) | `rustup check` | up to date |
| `rustup check` | all toolchains and rustup "up to date" | `rustup check` | no update |
| cargo-nextest | 0.9.148 (cd1d6d546 2026-10-07) | `cargo install --locked cargo-nextest@0.9.148` | **updated** from 0.9.146; crates.io `max_stable_version` 0.9.148 |
| cargo-deny | 0.20.2 | `cargo deny --version` | = crates.io 0.20.2 |
| cargo-llvm-cov | 0.9.1 | `cargo llvm-cov --version` | = crates.io 0.9.1; `llvm-tools` installed on stable |
| cargo-fuzz | 0.13.2 | `cargo fuzz --version` | = crates.io 0.13.2 |
| inferno | 0.12.8 | `cargo install --list` | = crates.io 0.12.8 |
| Java | openjdk 25.0.4.1 (2026-08-18) | `java -version` | present (ANTLR tool) |
| heaptrack | 1.5.0, `heaptrack_print` present | `heaptrack --version` | latest tag `v1.5.0` (2023-09-29, KDE GitLab API) |
| ANTLR tool jar | `antlr4-4.13.3-SNAPSHOT-complete.jar` of antlr4rust v0.6.0 (2026-09-27) | GitHub release assets | **pending, no root needed:** task #36 downloads it |
| perf | 7.0.14 | `perf --version` | not needed in Sprint 3 (timing deferred) |

**Needs root: none.** **Pending on the user: none.**

## 10. rmp operations to enter the plan (for the roadmap-manager, not run here)

1. `rmp task create` × 18 with the titles, types, priorities and `-fr`/`-tr`/`-ac` of §5; then `rmp sprint add-tasks 3 <ids>`.
2. `rmp task add-dep` for each edge of §6, with the real ids.
3. `rmp sprint reorder 3` in the order of §6.
4. On task #3: a `DECISION` with the §7 table (real ids); a `DECISION` with the choices of §3.4, the grouping rationale of §5 and the reuse verdicts of §4.18; a `FINDING` with the 15 + 5 specification points of §3.1 and §3.2 and the item groups of §3.3; a `TEST` with the tool versions of §9.
