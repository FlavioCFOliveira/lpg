---
title: Query engine
status: draft
last-reviewed: 2026-10-11
related:
  - architecture.md
  - catalog-and-indexes.md
  - values-and-types.md
  - quality-and-testing.md
---

# Query engine

## TS-19 Parser

- **Decision**:
  - **Grammar reference.** `GQL.g4` of opengql/grammar at commit `16ea71b` (R-GQL) is the trusted reference for GQL syntax (user decision 2026-10-02). The ISO/IEC 39075:2024 text is not available.
  - **Supplementary source.** The ISO electronic inserts in [docs/standards/iso-iec-39075/ed-1/](../standards/iso-iec-39075/ed-1/README.md) supply only what the grammar does not contain: GQLSTATUS condition codes, feature identifiers and the list of implementation-defined items. They are not the grammar authority.
  - **Parser.** A hand-written, case-insensitive lexer, a recursive-descent parser and a Pratt parser for expressions, producing a typed AST with source spans.
    - It follows the rules of `GQL.g4` clause by clause; each function names the rule it implements.
    - It parses the whole of `GQL.g4`. Constructs outside the supported feature set fail in the binder ([TS-20](#ts-20-binder-planner-and-execution)).
  - **Keywords.** The reserved, pre-reserved and non-reserved words are those of `GQL.g4` (`GQL.g4:3276-3584`: 218, 39 and 47 words), generated from it with a drift test. Where the ISO BNF insert differs, `GQL.g4` is followed (NFR-VER-002); provisional (OQ-005):
    - `TRUE`, `FALSE`, `UNKNOWN` (BNF reserved words) are the token `BOOLEAN_LITERAL` (`GQL.g4:3111-3115`), and `IMPLIES` (BNF reserved) is a token for `IMPLIES` or `=>` (`GQL.g4:3050-3053`): none of the four can be a regular identifier, as in the BNF.
    - `WHITESPACE` (BNF pre-reserved) is a regular identifier.
    - `TEMP` (BNF non-reserved, in `[ TEMP ] <element variable>`, `ISO_IEC_39075(en).bnf.txt:753-754`) is a regular identifier, and the `TEMP` prefix is a syntax error: `elementVariableDeclaration` has no such prefix (`GQL.g4:1001-1003`).
  - **Identifier characters.** `IDENTIFIER_START` is `\p{ID_Start}` or `\p{Pc}` and `IDENTIFIER_EXTEND` is `\p{ID_Continue}` (`GQL.g4:3612-3627`). The lexer uses tables generated from the Unicode Character Database 15.0.0, the version the oracle's ANTLR tool jar uses, with a test that regenerates them and fails on drift. Changing the version needs the oracle's jar to change with it.
  - **Ambiguities.** Each ambiguous decision is resolved to its lowest-numbered alternative, which is the oracle's choice (antlr4rust 0.6.0 `src/prediction_mode.rs:117-119, 185-196`, used at `src/parser_atn_simulator.rs:494`). Decisions found ambiguous ([round-s](../evidence/planning/round-s/README.md) `ambiguities.py`, 2026-10-08; 4 978 accepted texts):

    | Rule (decision) | Alternatives | Resolution | Example | Texts |
    |---|---|---|---|---|
    | `statement` | 1 `compositeQueryStatement`, 3 `linearDataModifyingStatement` | a query | `MATCH (n) RETURN n` | 692 |
    | `statement` | 2 `linearCatalogModifyingStatement`, 3 `linearDataModifyingStatement` | a catalog-modifying procedure call; the binder treats every `CALL` by the routine's mode ([TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events)) | `CALL { CREATE GRAPH g ANY }` | 10 |
    | `labelAndPropertySetSpecification` | 1, 2 | 1: labels, then properties | `INSERT (:L {p: 1})` | 23 |
    | `insertElementPatternFiller` | 1, 2 | 1: variable, then specification | `INSERT (a:L)` | 8 |
    | `valueFunction` | 1 `numericValueFunction`, 4 `durationValueFunction` | `ABS` as a numeric function; the binder applies it to a duration when the argument is one | `ABS(x)` | 3 |
    | `existsPredicate` (inner block) | 3 match block in braces, 5 nested query | the match block | `EXISTS { MATCH (a) MATCH (b) }` | 3 |
    | `graphPattern` (optional `WHERE`) | 1 take, 2 skip | the `WHERE` belongs to the graph pattern | `SELECT … FROM g MATCH (p) WHERE …` | 2 |
    | `endpointPair` | 1 directed, 2 undirected | directed (`TO`) | `CONNECTING (a TO b)` | 1 |
    | `result` | 1 `resultExpression`, 2 `nullLiteral` | an expression | `CASE … THEN NULL …` | 1 |
    | `castOperand` | 1 `valueExpression`, 2 `nullLiteral` | an expression | `CAST(NULL AS …)` | 1 |

    Decisions also found ambiguous on 600 000 generated texts, 306 093 of them accepted (rmp #41, 2026-10-09: `GqlProbe.java` of round-s in `LL_EXACT_AMBIG_DETECTION` mode over the 100 000 sentences and 100 000 mutants of each seed 20261008, 1 and 12648430 of the `tools/gql-oracle` generator). For each, lpg's AST of the example equals the ANTLR parse tree of the same text, and `src/query/parser/tests.rs` pins it:

    | Rule (decision) | Alternatives | Resolution | Example | Texts |
    |---|---|---|---|---|
    | `graphExpression` | 1 `graphReference`, 2 `objectExpressionPrimary` | a delimited graph name, not a string literal | `USE "" FINISH` | 3 359 |
    | `bindingTableExpression` | 2 `bindingTableReference`, 3 `objectExpressionPrimary` | a delimited binding table name | `SESSION SET TABLE $m = ""` | 2 254 |
    | `simpleDataAccessingStatement` | 1 `simpleQueryStatement`, 2 `simpleDataModifyingStatement` | `CALL` as a query statement | `USE g CALL p()` | 2 094 |
    | `valueType` (inner `\|` loop) | 1, 2 | inside `ANY < … >`, a member may be a union: `ANY < A \| B >` has one member, `A \| B` | `VALUE v ANY < NULL \| VERTEX > = 1 FINISH` | 1 401 |
    | `absoluteCatalogSchemaReference` | 1 `/`, 2 directory path and schema name | the root `/`; a following non-reserved word starts the next clause | `AT / PROPERTY GRAPH g = x FINISH` | 849 |
    | `caseOperand` | 1 `nonParenthesizedValueExpressionPrimary`, 2 `elementVariableReference` | a value primary (both give the variable) | `CASE w WHEN 1 THEN 2 END` | 471 |
    | `nodeTypePhrase`, `edgeTypePhrase` (`TYPE?`) | 1 take, 2 skip | `TYPE` is the keyword, not a type name | `NODE TYPE { }` | 416, 360 |
    | `primitiveCatalogModifyingStatement` | 3 `createGraphStatement`, 5 `createGraphTypeStatement`; 4 `dropGraphStatement`, 6 `dropGraphTypeStatement` | a graph statement: `TYPE` is a parent object name | `DROP GRAPH TYPE . x` | 215, 16 |
    | `bindingTableReference`, `graphReference` | 1 parent and name, 3 and 4 `referenceParameterSpecification` | a parameter is the parent schema of the following name | `TABLE s = $$IB BINDING TABLE t = x FINISH` | 188, 111 |
    | `valueExpression` | 4 `GRAPH graphExpression`, 5 `TABLE bindingTableExpression`, 7 a predicate or property of a variable | the graph or binding table expression | `CALL p ( GRAPH . groups )` | 33, 55 |
    | `orderByAndPageStatement` (optional `OFFSET`, `LIMIT`) | 1 take, 2 skip | taken by the same statement | `ORDER BY x SKIP 2 FINISH` | 47 |
    | `relativeDirectoryPath`, `simpleDirectoryPath`, `absoluteDirectoryPath` (repetitions) | 1 continue, 2 stop | the directories end where a schema name and the parent separator still follow | `TABLE .. / d / e / f` | 16, 16, 7 |
    | `valueType` | 3 `listValueTypeAlt1`, 4 `listValueTypeAlt2` | `LIST < … > NOT NULL`: `NOT NULL` belongs to the list | `IS TYPED LIST < { } > NOT NULL` | 2 |
    | `catalogGraphParentAndName`, `catalogObjectParentReference` (repetitions) | 1, 2 | the longest parent | `CREATE GRAPH TYPE . a . b . c AS COPY OF $$ys` | 2, 2 |
    | `nodeTypeFiller` (inner) | 1, 2 | an empty key label set `=>`, then the implied content | `VERTEX => IS NORMALIZED` | 1 |

    The probe also reported every decision of the first table on the generated texts except `graphPattern`. Any other ambiguous decision is resolved the same way and added to these tables.
  - **Feature classification** (provisional, OQ-005). Every parser rule is classified as mandatory or as needing feature codes of the ISO features insert, by gql-compat's placement rule (a construct that a code covers is optional; everything else is mandatory; gql-compat README @ `9de1991`, not ISO text). Method, anchors and evidence: [round-s](../evidence/planning/round-s/README.md). Result: [Rule-to-feature table](#rule-to-feature-table).
    - **Supported codes** (user decisions 2026-10-08, rmp task #37: "Seis + as exigidas", then "Só GG03"): the mandatory surface and GC01, GC04, GD01, GG01, GG02, GG03, GP04, GP18, GQ01, GT01, GT03, GV39, GV40, GV41. The graph-type creation construct the user approved is `createGraphTypeStatement`, which gql-compat's cases place under GC01; its body, `nestedGraphTypeSpecification`, is GG03, which FR-GQL-004 needs to make a closed graph type.
    - **Not supported:** the other 214 codes of the insert. Each fails with the "feature not supported" condition ([interfaces.md](interfaces.md#gqlstatus-codes)).
    - 53 codes have no `GQL.g4` construct of their own (semantic, lexical or refinements): their gates live in the binder and the lexer.
  - **Implementation-defined choices, lexical** (BR-GQL-002; user approval 2026-10-08 of the rule "Neo4j first, then Memgraph, else the reference the blocking task cites", rmp task #38). Codes and lines are those of `ISO_IEC_39075(en)-implementation-defined.xml`. "zu" is zu @ `a821319` `docs/gql-implementation-defined.md` (R-ZU), the reference task #41 cites, used where neither Neo4j nor Memgraph defines the item. Memgraph (documentation and R-MG source) was checked on 2026-10-08 for every item first answered from zu; it is cited where it defines one, and changed no choice.

    | Item (line) | Choice | Source |
    |---|---|---|
    | IV001 (394) | Unicode: the text is a Rust `&str`, so any Unicode scalar value, UTF-8 encoded. | Neo4j lexes the query as a Java `String`, which holds Unicode text (R-NEO `Cypher25AstLexer.java:71-76`). |
    | IA019 (110) | Bidirectional control characters are permitted in string literals. | Neo4j string literals accept every character except the quote and the backslash (R-NEO `Cypher25Lexer.g4:83-93`). |
    | IA020 (114) | Characters of category Co are permitted in delimited identifiers. Regular identifiers cannot contain them (`ID_Continue` excludes Co). | Neo4j backtick names accept every character but the backtick (`Cypher25Lexer.g4:95-97`). |
    | IA023 (122) | U+000A and U+000D both end a simple comment (`GQL.g4:3748-3750`). Line numbers in error positions count U+000A only, as the oracle does. | Neo4j: `//` comments end at `\r` or `\n` (`Cypher25Lexer.g4:51-53`). The oracle's runtime starts a new line at `\n` only (antlr4rust 0.6.0 `src/lexer_atn_simulator.rs:123-131`). |
    | IL010 (358) | No limit on the number of digits. An integer literal above 9 223 372 036 854 775 807 fails with 22003; a minus sign directly before the literal 9 223 372 036 854 775 808 gives the minimum `INT64`. An approximate literal that overflows `FLOAT64` fails with 22003. | Neo4j: "integer is too large" and an infinite float literal raise 22003 (R-NEO `SemanticExpressionCheck.scala:836-842, 889-895`; `SemanticError.scala:950-953`); its literal includes the sign (`^-?[1-9][0-9]*$`). |
    | IE003 (310) | The UAX #31 R1-1 profile of `GQL.g4`: start characters `ID_Start` plus `Pc`, continue characters `ID_Continue` (above). | `GQL.g4:3612-3627` (NFR-VER-002). |
    | IE005 (318) | Text that `GQL.g4` does not accept fails with 42001, with the position and the expected tokens. | BR-GQL-001. |
    | IW014 (478) | No visual-confusability test: two identifiers are confusable only if equal, so 42004 is never raised. | Neither Neo4j nor Memgraph has such a test (no match for "confusab" in R-NEO `community/cypher`, R-MG `src/query`, `src/utils`); zu declines it (`crates/zu-query/src/lexer.rs:780-785`) |
    | IW023 (510) | A regular identifier's canonical form is its text as written; a delimited identifier's is its characters after escapes are resolved. Neither is case-folded or trimmed. | Neo4j: names are case-sensitive (Cypher Manual, "Naming rules and recommendations"); a backtick name is its inner text with each doubled backtick made single (R-NEO `ExpressionBuilder.scala:1102-1105`). |

    Not reached by the supported codes: IL018 (374), the upper bound of a general quantifier (G060, G061).
  - **Narrowing.** `GQL.g4` states that it is "more permissive than a GQL implementation would be" and leaves invalid type and function combinations to the implementation. The binder therefore rejects those combinations ([TS-20](#ts-20-binder-planner-and-execution)). A rejection made when the statement is compiled is a syntax rule violation, 42000: a non-boolean literal or a node or edge reference as an operand of `NOT`, `AND`, `OR`, `IS` or as a search condition (opengql/tck `Boolean1`, `Boolean2`, `Boolean4.feature`; rmp #52). The same operands met at run time raise the data exception 22G03, as does a search condition whose value is not a boolean (gql-compat `condition/22g03/non-boolean-search-condition`).
  - **Test oracle.** The ANTLR-generated parser from the same `GQL.g4` ([TS-30](quality-and-testing.md#ts-30-testing-and-conformance)).
  - **Syntax errors.** Input that `GQL.g4` does not accept fails with GQLSTATUS 42001 "syntax error or access rule violation – invalid syntax" (ISO conditions insert, class 42 subclass 001), reported with the position and the expected tokens.
  - **Nesting limit** (rmp #41). Each recursive rule entered, and each operator of a left-deep chain (`a + b + c`, `a.b.c`, label and type unions, query conjunctions), takes one level; more than 256 levels, or more than 1 MiB of stack used by the parse (measured from the address of a local variable, as PostgreSQL's `stack_is_too_deep` does, `src/backend/utils/misc/stack_depth.c` @ `1b5dd3a`), fails with 42001. The AST is therefore at most 256 levels deep, and recursive walks of it cannot overflow the stack. Texts that `GQL.g4` accepts but nest deeper fail. Measured on a 2 MiB stack (`docs/evidence/lab-sprint-3/raw/nesting-depth.txt`): chains admit 251 to 254 operators in any build; the costliest recursive shape, nested `EXISTS { MATCH … }`, admits 50 levels at opt-level 1 (the test profile) and 23 at opt-level 0; nested parentheses 126 and 61. The reference parsers count nesting only (datafusion-sqlparser-rs 50, zu 64, Grafeo 128) and bound no chain.
  - **Provisional rules.** Rules that need the ISO text are provisional (OQ-005), for example every Syntax Rule and the feature classification below.
  - **Parameters** are never inlined into the text.
- **Satisfies**: FR-GQL-001, FR-GQL-002, BR-GQL-001, BR-GQL-002, FR-API-004, NFR-VER-002, NFR-SAFE-001, NFR-DEP-001, NFR-PERF-003.
- **Evidence**:
  - Neo4j, Memgraph and Kuzu generate their parsers with ANTLR 4 (R-NEO `community/cypher/front-end/parser/v25/.../Cypher25Parser.g4`; R-MG `src/query/frontend/opencypher/grammar/Cypher.g4`; R-KUZU `src/antlr4/Cypher.g4`).
  - `GQL.g4` has 3 774 lines (R-GQL).
  - Measured with antlr4rust 0.5.2 (R-ANTLR) and the unmodified `GQL.g4` ([M-QE](../evidence/research/research-03-query-engine.md) §Q1.3; [antlr-exp](../evidence/antlr-exp/bench/src/main.rs)):
    - default LL mode: 2.9–6.8 ms per 200–290-byte MATCH query, with grammar ambiguities on 10 of 20 inputs;
    - SLL mode: 32–129 µs;
    - a hand-written SQL parser (`sqlparser`) on comparable SQL: 24–45 µs.
  - The generated parser has 113 000 lines and no `unsafe`.
  - Keywords, measured by `keywords.py` ([round-s](../evidence/planning/round-s/README.md), 2026-10-08): the ISO BNF insert has 222 reserved, 40 pre-reserved and 48 non-reserved words; `GQL.g4` lacks exactly the six listed above.
  - Unicode version, measured by `round-s/unicode/` (2026-10-08): the tool jar of antlr4rust release v0.6.0 (`antlr4-4.13.3-SNAPSHOT-complete.jar`, SHA-256 `391e7731…b677`) generates 136 345 `ID_Start` and 139 482 `ID_Continue` code points, equal to Unicode 15.0.0 `DerivedCoreProperties.txt` and different from 13.0.0, 14.0.0, 15.1.0 and 16.0.0.
  - zu generates its Unicode tables from checked-in UCD files with a drift test (R-ZU `crates/zu-common/tests/unicode_tables.rs:1-12`).
- **Crates**:
  - `antlr4rust` 0.6.0 (BSD-3-Clause, 3.3 M downloads, released 2026-09-27): dev dependency of the test-only oracle crate only.
  - `sqlparser` 0.63.0: study only, as the reference for hand-written recursive descent with Pratt expressions.
  - `pest`, `peg`, `lalrpop`, `chumsky`, `nom`, `logos`: not used.
  - `unicode-ident` 1.0.26: not used. It implements `XID_Start` and `XID_Continue` (`src/lib.rs:261, 274`), not the `ID_Start` and `ID_Continue` of `GQL.g4`, at Unicode 18.0.0 (`src/tables.rs:12`).
  - Rust's `char` methods: not used for identifiers. They follow the toolchain's Unicode version, 17.0.0 on Rust 1.99.0 and 16.0.0 on 1.89 (`char::UNICODE_VERSION`, printed 2026-10-08).
  - `gqlparser`, `drasi-query-gql`, `gqlitedb` (3–12 k downloads): too little use (NFR-DEP-001); study only.
- **Rejected**:
  - An ANTLR-generated product parser: measured parse cost, 113 000 generated lines, a Java tool chain, a generic parse tree.
  - LR generators: `GQL.g4` is ambiguous, so each conflict would need a rewrite.
  - PEG: ordered choice hides ambiguity.
  - Parser combinators: still hand-written rule by rule, plus a dependency.

## TS-20 Binder, planner and execution

- **Decision**:
  - **Binder.** One pass from the AST to a typed intermediate form.
    - Every variable is resolved to a slot, with its kind (node, edge, path, value, graph, binding table), its degree (singleton, group, conditional singleton) and its static or dynamic type.
    - Path-pattern rules are checked at bind time: normalisation, termination of unbounded quantifiers, conditional-singleton joins.
    - Type and function combinations that `GQL.g4` accepts but GQL does not allow are rejected here ([TS-19](#ts-19-parser)).
    - Predicates use three-valued logic.
    - Each optional feature of the ISO features insert maps to a gate that fails with a "feature not supported" GQLSTATUS until the feature is delivered: 42001 with the cause 51N26 naming the feature code ([interfaces.md](interfaces.md#gqlstatus-codes)). The ISO conditions insert has no general code for this, so the code is provisional (OQ-005). The supported codes are listed in [TS-19](#ts-19-parser).
    - Each implementation-defined item is implemented only after the user approves its choice (BR-GQL-002).
    - **Bound form** (rmp #43, `src/query/bound.rs`): a statement binds to one frame of slots, its parameter names in order of first use (never inlined), the graphs it uses, and a pipeline of parts (`Match`, `Filter`, `Project`, `Sort`, `Insert`, `Set`, `Remove`), each over the binding table of the one before. A projection replaces the scope with its columns. `EXISTS` subqueries are part lists over the same frame. Catalog statements keep absolute paths and resolve parents when they run, so one list can create a schema and then a graph in it. Static types are the element reference types (IW011, IW012); other values are typed when evaluated.
    - Aggregation without `GROUP BY` (GQ15) groups by the non-aggregate items, as Neo4j and zu do. Sort keys after `RETURN` name result columns: any other expression is GQ14, a renamed variable GQ16, a discarded one GA07, an aggregate GF20.
    - The degree analysis of path patterns runs before their feature gates, so the conditional-singleton join rule (42000) is checked although unions, alternations and `?` are not supported. Codes: an undefined name 42002; a duplicate binding, a forbidden join or a misplaced aggregate 42000, with the cause 22N27 for a variable that a graph pattern binds as two element kinds; in a catalog-modifying statement, 42N11, 42002 and G1000 become the cause of 42000 ([interfaces.md](interfaces.md) "GQLSTATUS codes").
    - `CALL` of a named procedure resolves in the session's registry (rmp #50; [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events) Built): `Part::Call` with the routine, its arguments with defaults filled in and the columns it yields. A read-write routine inside a query fails with 42N02; a name with a schema part, or none registered, with 42N08; a `CALL` in a larger statement without `YIELD`, of a routine with result columns, with 42002 and the cause 42N25.
    - Not gated, because their rule needs the ISO text (OQ-005): GB01 (identifier length) and GA04 (universal comparison; lpg's cross-type comparison follows IV002 and IV010).
  - **Planner.**
    - Rule rewrites (rmp #44, `src/query/plan/`), applied while the bound pipeline is lowered to operators in one pass: predicate pushdown (each conjunct after the first operator that binds its slots), projection of only the returned items, constant folding (binder), and index seeks for indexed equality and ranges. Limit pushdown does not apply while `LIMIT` (GQ13) is not supported.
    - Index choice for a start node, on indexes the snapshot sees ONLINE only: equality on a leading prefix of a range index's properties, then a point index for an equality with a point (Neo4j's point indexes solve `n.prop = point(...)`, Cypher Manual "Create indexes", checked 2026-10-10), then a range on a single-property range index, then a label lookup index, else `AllNodesScan`. The predicate stays as a filter after the seek. Indexes on edge properties are not used for seeks.
    - Join order: left-deep plans (a start node, then one expansion at a time). Exact dynamic programming over (start, connected edge subset) up to 7 edges, greedy beyond (Kuzu's threshold, `subplans_table.h:11`). Bushy plans are not enumerated: there is no join operator between subplans.
    - Estimates: `card(S) = Π n(v) · Π p(e)`, `n(v)` a node's label count times its filters' selectivities, `p(e)` the edges of its type between the endpoints' labels over the product of their counts. The statistics have no total node count, so an unlabelled node estimate is the sum of the label counts.
    - Cost: the sum of intermediate cardinalities from the [TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics) statistics, with defaults of 0.01 for equality and 0.1 for other predicates.
    - Operators: `NodeById`, `LabelScan`, `IndexSeek`, `Expand` (a prefix scan of the adjacency key), `ExpandInto` (a point lookup of the key `(node, direction, type, neighbour)`), `Repeat` per path restrictor, and a product-graph breadth-first search for path selectors.
    - No plan cache until a measurement shows parse and plan time matter.
  - **Execution.**
    - Pull-based, batch at a time (rmp #45, `src/query/exec/`): `next_batch` returns up to 1 024 rows of `Value`, held row after row in one vector, so an expression reads a row as a slice. The transaction and the statement state are passed to each call, so the cursor owns its operator tree and is `Send`.
    - Scans read in chunks of 1 024 entries (`Transaction::scan_chunk`), so the first batch comes after at most one chunk per scan run.
    - `INSERT`, `SET` and `REMOVE` read all their input before they write (Neo4j's `Eager`): a statement sees the binding table of the one before it, and its writes do not feed its own input.
    - A statement that fails aborts its transaction; later statements fail with 25N05 (a Bolt failure ends an explicit Neo4j transaction).
    - One thread per statement, through `spawn_blocking` per batch ([TS-02](architecture.md#ts-02-async-api-and-threading)). No intra-query parallelism (NFR-PERF-007).
    - Only blocking operators (sort, aggregation, distinct, the write operators) hold rows. Each reserves from the memory accountant ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)) in chunks: when its held bytes no longer cover the rows it holds, it reserves the larger of the shortfall and 128 KiB. It releases everything when it ends.
  - **Limits and cancellation.**
    - One atomic status per transaction. It is set by Rust API cancel, Bolt `RESET`, termination, and a tokio timer. The timer measures the time limit, or `tx_timeout`, from the start of the transaction (FR-API-007).
    - Operators check it once per batch, and every 1 024 iterations of a loop that can run longer than one batch, such as an expansion over one node's adjacency, a sort or an aggregation's input (K = 1 024).
    - **Limits** (FR-API-007, FR-LIM-002): time, memory, rows and bytes, each optional. None has a default.
      - A `Session` holds the limits its transactions inherit. `begin(options)` replaces any of them for that transaction. An auto-commit statement uses the session's.
      - Time: from the start of the transaction.
      - Memory: the bytes the transaction holds from the accountant: its operators' reservations, its write set and the old versions its writes create (FR-LIM-007).
      - Rows and bytes: per statement, counted at the result sink. The bytes of a row are the sum of its values' sizes in the record encoding of [TS-21](values-and-types.md#ts-21-values-and-types) (header and payload).
    - Crossing any limit fails the statement and aborts the transaction.
    - **Built** (rmp #49). The status is `src/api/status.rs`: a cancel, termination or timeout moves it from active to its reason with a compare-and-swap, and a commit from active to committing the same way, so exactly one wins (a `loom` model checks it, and fails when the commit ignores the status). The cursor's check receives the statement's loop iteration count; a sort counts its comparisons as iterations and checks every 1 024 of them. The memory limit is checked by the kernel transaction (`Transaction::check_memory`): its write-set charge plus its statements' operator reservations, counted through a shared counter, must stay within the limit (51N73). Rows and bytes are counted by the result stream per batch; a batch that crosses a limit fails the statement with 51N66 and none of its rows is returned. A statement of an aborted transaction fails with 25N05 before it binds.
  - **EXPLAIN and PROFILE.**
    - EXPLAIN returns the plan with estimated rows per operator and does not execute.
    - PROFILE wraps each operator in a decorator that records rows, batches, time per batch, storage entries read, buffer hits and misses, and peak memory. Normal plans contain no decorator. Each counter excludes the operator's inputs. Entries read are B+tree entries returned to the operator's reads, counted per thread (`src/storage/counters.rs`), as are buffer hits and misses; peak memory is the most the operator reserved from the accountant at once.
    - **Result.** A tree of plan nodes: operator name, details (index, direction, types, labels), identifiers, estimated rows, children. PROFILE adds per node: rows, batches, time, entries read (Neo4j's "DB Hits"), buffer hits and misses (Neo4j's "Page Cache Hits/Misses") and peak memory (Neo4j's "Memory (Bytes)"). `Session::explain` returns the tree. `Session::profile` returns a `ResultStream` whose `summary()` holds the profiled tree once the last row is pulled, as Bolt returns `plan` and `profile` in the last `PULL` success.
    - **Operator names.** Neo4j's, where Neo4j has the operator: `NodeByIdSeek` (TS-20 `NodeById`), `AllNodesScan`, `NodeByLabelScan` (`LabelScan`), `NodeIndexSeek`, `NodeIndexSeekByRange`, `NodeUniqueIndexSeek`, `DirectedRelationshipIndexSeek`, `DirectedRelationshipIndexSeekByRange` (`IndexSeek`), `Expand(All)` (`Expand`), `Expand(Into)` (`ExpandInto`), `Filter`, `Projection`, `EagerAggregation`, `Sort`, `Distinct`, `NodeHashJoin`, `ValueHashJoin`, `CartesianProduct`, `Apply`, `Optional`, `Create`, `SetProperty`, `SetProperties`, `ProcedureCall`, `EmptyResult`, `ProduceResults`. An operator Neo4j lacks (catalog statements, `REMOVE` of a property, `Repeat`) keeps its lpg name. Implemented (rmp #44, #45): `Argument`, `AllNodesScan`, `NodeByLabelScan`, `NodeIndexSeek` (equality, also through a point index), `NodeIndexSeekByRange`, `Expand(All)`, `Expand(Into)`, `Filter`, `Apply` (also the product of independent scans), `Optional`, `Projection`, `EagerAggregation`, `Distinct`, `Sort`, `Create`, `SetProperty`, `SetProperties`, `EmptyResult`, `ProduceResults`, `ProcedureCall` (rmp #50); lpg's `RemoveProperty` and `CatalogStatements`.
  - **Implementation-defined choices, statements and sessions** (BR-GQL-002; approval rule of 2026-10-08 and "zu" as in [TS-19](#ts-19-parser); the zu reference is the one tasks #43, #46, #47 and #48 cite).

    | Item (line) | Choice | Source |
    |---|---|---|
    | IA017 (106) | Several assignments to one property of one element in one `SET` raise no condition: they apply in order and the last one stays. | Neo4j applies each `SET` item as its own operation on each row (R-NEO `SetPipe.scala:26-36`), and states that Cypher's `SET` is order-dependent (Cypher Manual, "Supported mandatory GQL features", 13.3). zu raises 22G0M instead. |
    | ID086 (266) | `DIFFERENT EDGES`: an edge is not matched twice within one graph-pattern match. An edge variable declared by two edge patterns of one graph pattern could never bind and fails with 42000, as in Neo4j (openCypher TCK `Match3.feature` [29]; rmp #52). | Neo4j: "By default, Cypher does not allow relationships to be re-traversed in the same graph pattern match" (Cypher Manual, "Match modes"). |
    | IW011 (470) | An element variable of a graph pattern has the open node or edge reference type (`NODE`, `RELATIONSHIP`). | Neo4j's structural types `NODE` and `RELATIONSHIP` (Cypher Manual, "Property, structural, and constructed values"). |
    | IW012 (474) | A node variable of an insert node pattern has the open node reference type. | As IW011. |
    | IE006 (322) | No restriction on transactions that mix catalog and data statements. | FR-CAT-004 (user decision overriding Neo4j, which forbids the mix). |
    | IE007 (326) | None: there is no restriction to violate. | Follows from IE006; zu. |
    | IA001 (50) | The declared type of a result is not exposed: a result carries column names and values. | Bolt's `RUN` success carries `fields`, a list of names (Neo4j Bolt documentation, "Message: RUN"). |
    | IA002 (54) | A failure is one `Error` whose cause may be another `Error`, to any depth. | Bolt's `FAILURE` carries a `cause` with its own nested `cause` (Neo4j Bolt documentation, "Message: FAILURE"). |
    | ID001 (134) | The principal is the string the application passes to `session` (Rust API) or the authoriser returns (Bolt). | FR-SES-001, ABS-002 ([TS-23](interfaces.md#ts-23-authentication-and-authorisation)). |
    | ID002 (138) | The home schema and graph come from the caller or the authoriser, else `/public` and `/public/lpg`. | FR-SES-002, FR-DB-005. |
    | ID003 (142) | The privileges are the answers of the application's authoriser. | FR-AUTH-001, [TS-23](interfaces.md#ts-23-authentication-and-authorisation). |
    | ID016 (158) | No translations: messages are in English. | Neo4j's status descriptions are English (Status Codes manual); zu. |
    | ID049 (190) | No session parameters. | Neo4j supports none of GS01–GS03 (Cypher Manual, "Supported optional GQL features"); zu. |
    | ID061 (206) | `SESSION_USER` is a `STRING`: the session's principal. | Neo4j does not support `SESSION_USER` (Cypher Manual, "Supported mandatory GQL features", 20.3). Memgraph's `SHOW CURRENT USER` returns the session's user name as a string, or null without one (R-MG @ `847e224` `src/query/interpreter.cpp:1664-1668`); an lpg session always has a principal |
    | IV016 (430) | Each `Error` carries an English message saying what failed. It never contains a value (FR-OBS-002). | Bolt's `FAILURE` has a `message` beside `gql_status` and `description`. |
    | IW001 (438) | `Database::session` creates a session; dropping the `Session`, or `SESSION CLOSE`, ends it; `execute` submits a request. | FR-API-001, [interfaces.md](interfaces.md#public-api-shape). |
    | IW002 (442) | The application maps credentials to principals through its authoriser. | ABS-002, [TS-23](interfaces.md#ts-23-authentication-and-authorisation). |
    | IW003 (446) | The last request is the one before `SESSION CLOSE` or before the `Session` is dropped. | [TS-02](architecture.md#ts-02-async-api-and-threading). |
    | IW004 (450) | `Session::begin`, `Transaction::commit` and `Transaction::rollback`. | FR-GQL-005, FR-TX-004. |
    | IW005 (454) | Each call returns `Result<_, Error>`. | [interfaces.md](interfaces.md#public-api-shape). |
    | IW006 (458) | The `parameters` argument of `execute`, a map from name to value. | [interfaces.md](interfaces.md#public-api-shape). |
    | IW007 (462) | Exceptions as `Error`; completion conditions in `ResultStream::summary()`. | [interfaces.md](interfaces.md#public-api-shape). |
    | IW010 (466) | Procedures and functions are registered on the database handle through the Rust API. | FR-API-005, [TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events). |
    | IW015 (482) | `create` makes the root directory, schema `public` and graph `lpg`; nothing else is created automatically. | FR-DB-005, [TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics). |
    | IW016 (486) | A new schema is empty. | zu: neither Neo4j (GC01 unsupported, "Supported optional GQL features") nor Memgraph (no schema statement; only `SHOW SCHEMA INFO`, R-MG `MemgraphCypher.g4:967`) has GQL schemas |
    | IE008 (330), IE009 (334) | None: only the ISO conditions are raised. User decision 2026-10-08, "Só ISO" (rmp task #38). | Neo4j adds notifications (01N…, 03N…; Neo4j Status Codes, "Notifications"); the user chose ISO only (NFR-SIMP-001). |
    | IE010 (338) | `00001` "omitted result" only. Same decision. | ISO conditions insert (57); Neo4j and zu use it. |
    | IW025 (514) | Every catalog-modifying procedure is under transaction control, any number in one transaction. | FR-CAT-004, FR-PROC-002. |

    Not reached by the supported codes: IA013 (90, selective path patterns, G005, G015–G020), ID005 (150, `ELEMENTS`, GF04), ID017 (162, diagnostic records, GA08), ID076 (254, `ELEMENT_ID`, G100), IE001 (302, URIs: no `GQL.g4` rule takes one), IV015 (426, `<authorization identifier>` is used by no rule of either grammar), IW018 (494, lax casts: no `CAST`, GA05), IW022 (506, `NOT NULL` sites, GV90).
- **Satisfies**: FR-API-007, FR-API-008, FR-API-009, FR-API-010, FR-CAT-003, FR-GQL-002, FR-GQL-004, BR-GQL-002, FR-LIM-001, FR-LIM-002, FR-LIM-004, FR-BOLT-015, FR-BOLT-021, FR-OBS-004, FR-OBS-005, FR-OBS-006, NFR-PERF-007.
- **Evidence** (M-QE §Q2–Q4, §Q7, with file and line references):
  - Memgraph: symbol table and pull cursors; cancellation checked every 20th call (Check interval, below).
  - Kuzu: binder; dynamic-programming join order up to 7 relationships; cost as a sum of cardinalities; default selectivities. Kuzu materialises the whole result before returning, which fails FR-API-010.
  - Neo4j: chunked memory reservations (`LocalMemoryTracker`) and the `PROFILE` columns.
  - Chunk size. Neo4j's tracker reserves the larger of the request and its grab size from its pool (R-NEO `community/common/.../memory/LocalMemoryTracker.java:182-188`); the grab size is 2 MiB per transaction and 128 KiB per worker (`GraphDatabaseInternalSettings.java:1167-1178`). Under NFR-PERF-007 a statement is one worker, hence 128 KiB. DataFusion grows a reservation by the exact size of each batch (R-DF `datafusion/physical-plan/src/sorts/sort.rs:868-870`).
  - Check interval. Memgraph checks for abort on every 20th call, in cursor pulls and inside long loops (R-MG @ `847e224` `src/query/plan/operator.cpp:571-588`). Kuzu checks once per `getNextTuple`, that is once per vector of tuples (R-KUZU `src/processor/operator/physical_operator.cpp:186-189`). lpg's unit of work is the batch of 1 024 rows, so loops check at that granularity.
  - Limits. Neo4j's transaction timeout and per-transaction memory limit default to 0, meaning none (R-NEO `GraphDatabaseSettings.java:486-491, 1062-1068`), and a timeout the client gives for one transaction replaces the server's (`KernelImpl.java:105-119`). Neo4j has no result-size limit; the byte measure is lpg's own record encoding.
  - Operator names and `PROFILE` columns: Cypher Manual, "Operators" and "Operators in detail" (checked 2026-10-08). Bolt `PULL` success fields `plan` and `profile`: Neo4j Bolt documentation, "Message: PULL".
  - Measured on the reference hardware: batches of 1 024 rows were 2.3× faster than row-at-a-time in a toy pipeline, and an `Instant::now()` pair costs 2.75 µs, which is why time is read per batch, not per row.
- **Crates**: none used. DataFusion is a relational engine over Arrow batches whose operators return a stream that the consumer pulls (`ExecutionPlan::execute` returns a `SendableRecordBatchStream`, R-DF `datafusion/physical-plan/src/execution_plan.rs:696-700`); study only.
- **Rejected**:
  - Push pipelines (DuckDB, Neo4j pipelined): buffers and backpressure for no stated need.
  - Row-at-a-time execution: measured 2.3× slower.
  - Factorised vectors (Kuzu): little benefit on short reads, per Kuzu's own evaluation (D-KUZU §4.1, quoted in M-QE §Q3.1).
  - IDP join ordering (Neo4j): more complex, with no measured case that needs it.
  - Execution inline on tokio workers: page misses block.

## Rule-to-feature table

Provisional (OQ-005). Every parser rule of `GQL.g4` @ `16ea71b`, grouped by what it needs ([TS-19](#ts-19-parser) Feature classification). Generated by `docs/evidence/planning/round-s/feature_map.py` on 2026-10-08; do not edit by hand. The exact condition of each rule is in [rules.tsv](../evidence/planning/round-s/rules.tsv).

- **Needs**: `mandatory`; or the rule's own condition (`+`: all of; `or`: any of); and, after `inside`, the codes of the constructs the rule is reachable only within.
- **Status**: `supported` when one of the rule's conditions uses only supported codes.
- **Partial anchors**: codes that cover one alternative or element of a supported rule; that part fails with "feature not supported" unless its code is supported.

| Needs | Status | Rules |
|---|---|---|
| mandatory | supported | `gqlProgram`, `programActivity`, `sessionActivity`, `transactionActivity`, `sessionSetCommand`, `sessionSetSchemaClause`, `sessionSetGraphClause`, `sessionCloseCommand`, `procedureSpecification`, `nestedDataModifyingProcedureSpecification`, `nestedQuerySpecification`, `procedureBody`, `statementBlock`, `statement`, `graphExpression`, `currentGraph`, `objectExpressionPrimary`, `linearDataModifyingStatement`, `ambientLinearDataModifyingStatement`, `ambientLinearDataModifyingStatementBody`, `simpleLinearDataAccessingStatement`, `simpleDataAccessingStatement`, `compositeQueryStatement`, `compositeQueryExpression`, `compositeQueryPrimary`, `linearQueryStatement`, `focusedLinearQueryStatement`, `ambientLinearQueryStatement`, `simpleLinearQueryStatement`, `simpleQueryStatement`, `primitiveQueryStatement`, `matchStatement`, `simpleMatchStatement`, `optionalMatchStatement`, `optionalOperand`, `matchStatementBlock`, `orderByAndPageStatement`, `primitiveResultStatement`, `returnStatement`, `returnStatementBody`, `returnItemList`, `returnItem`, `returnItemAlias`, `selectStatement`, `selectItemList`, `selectItem`, `selectItemAlias`, `havingClause`, `selectStatementBody`, `selectGraphMatchList`, `selectGraphMatch`, `selectQuerySpecification`, `graphPatternBindingTable`, `graphPattern`, `pathPatternList`, `pathPattern`, `graphPatternWhereClause`, `pathPatternExpression`, `pathTerm`, `pathFactor`, `pathPrimary`, `elementPattern`, `nodePattern`, `elementPatternFiller`, `elementVariableDeclaration`, `isLabelExpression`, `isOrColon`, `elementPatternPredicate`, `elementPatternWhereClause`, `elementPropertySpecification`, `propertyKeyValuePairList`, `propertyKeyValuePair`, `edgePattern`, `fullEdgePattern`, `fullEdgePointingRight`, `labelExpression`, `elementVariableReference`, `whereClause`, `orderByClause`, `sortSpecificationList`, `sortSpecification`, `sortKey`, `orderingSpecification`, `schemaReference`, `absoluteCatalogSchemaReference`, `relativeCatalogSchemaReference`, `predefinedSchemaReference`, `absoluteDirectoryPath`, `relativeDirectoryPath`, `simpleDirectoryPath`, `graphReference`, `homeGraph`, `catalogObjectParentReference`, `searchCondition`, `predicate`, `compOp`, `existsPredicate`, `nullPredicate`, `nullPredicatePart2`, `normalizedPredicatePart2`, `valueExpression`, `valueFunction`, `booleanValueExpression`, `characterOrByteStringFunction`, `subCharacterOrByteString`, `trimSingleCharacterOrByteString`, `foldCharacterString`, `normalizeCharacterString`, `aggregatingValueExpression`, `valueExpressionPrimary`, `parenthesizedValueExpression`, `nonParenthesizedValueExpressionPrimary`, `nonParenthesizedValueExpressionPrimarySpecialCase`, `unsignedValueSpecification`, `generalValueSpecification`, `dynamicParameterSpecification`, `caseExpression`, `caseAbbreviation`, `caseSpecification`, `simpleCase`, `searchedCase`, `simpleWhenClause`, `searchedWhenClause`, `elseClause`, `caseOperand`, `whenOperandList`, `whenOperand`, `result`, `resultExpression`, `aggregateFunction`, `generalSetFunction`, `generalSetFunctionType`, `setQuantifier`, `bindingVariableReference`, `truthValue`, `numericValueExpression`, `numericValueFunction`, `lengthExpression`, `charLengthExpression`, `characterStringValueExpression`, `trimOperands`, `trimCharacterOrByteStringSource`, `normalForm`, `stringLength`, `objectName`, `objectNameOrBindingVariable`, `directoryName`, `schemaName`, `graphName`, `delimitedGraphName`, `labelName`, `propertyName`, `elementVariable`, `bindingVariable`, `unsignedLiteral`, `generalLiteral`, `identifier`, `regularIdentifier`, `characterStringLiteral`, `unsignedNumericLiteral`, `exactNumericLiteral`, `approximateNumericLiteral`, `unsignedInteger`, `nullLiteral`, `nonReservedWords` (165) |
| G002 | not supported | `differentEdgesMatchMode` (1) |
| G002 or G003 | not supported | `matchMode` (1) |
| G003 | not supported | `repeatableElementsMatchMode` (1) |
| G004 | not supported | `pathVariableDeclaration` (1) |
| G005+G006+G015 or G005+G006+G016 or G005+G006+G017 or G005+G006+G018 or G005+G006+G019 or G005+G006+G020 or G005+G007+G015 or G005+G007+G016 or G005+G007+G017 or G005+G007+G018 or G005+G007+G019 or G005+G007+G020 or G006+G010 or G006+G011 or G006+G012 or G006+G013 or G007+G010 or G007+G011 or G007+G012 or G007+G013 | not supported | `keepClause` (1) |
| G005+G015 or G005+G016 or G005+G017 or G005+G018 or G005+G019 or G005+G020 | not supported | `pathSearchPrefix` (1) |
| G005+G015 or G005+G016 or G005+G017 or G005+G018 or G005+G019 or G005+G020 or G010 or G011 or G012 or G013 | not supported | `pathPatternPrefix` (1) |
| G010 or G011 or G012 or G013 | not supported | `pathModePrefix`, `pathMode` (2) |
| G014 | not supported | `pathOrPaths` (1) |
| G015; inside G005 | not supported | `allPathSearch` (1) |
| G016; inside G005 | not supported | `anyPathSearch` (1) |
| G017 or G018 or G019 or G020; inside G005 | not supported | `shortestPathSearch` (1) |
| G017; inside G005 | not supported | `allShortestPathSearch` (1) |
| G018; inside G005 | not supported | `anyShortestPathSearch` (1) |
| G019; inside G005 | not supported | `countedShortestPathSearch` (1) |
| G020; inside G005 | not supported | `countedShortestGroupSearch` (1) |
| G030; inside G039, G080 | not supported | `simplifiedMultisetAlternation` (1) |
| G032; inside G039, G080 | not supported | `simplifiedPathUnion` (1) |
| G035+G060 or G035+G061 or G036+G060 or G036+G061; inside G039, G080 | not supported | `simplifiedQuantified` (1) |
| G037; inside G039, G080 | not supported | `simplifiedQuestioned` (1) |
| G038 | not supported | `parenthesizedPathPatternExpression` (1) |
| G039 or G080 | not supported | `simplifiedPathPatternExpression` (1) |
| G043 | not supported | `fullEdgePointingLeft`, `fullEdgeLeftOrRight`, `fullEdgeAnyDirection` (3) |
| G043+GH02 | not supported | `fullEdgeLeftOrUndirected`, `fullEdgeUndirectedOrRight` (2) |
| G044 or G045 | not supported | `abbreviatedEdgePattern` (1) |
| G048; inside G038 | not supported | `subpathVariableDeclaration` (1) |
| G050; inside G038 | not supported | `parenthesizedPathPatternWhereClause` (1) |
| G060 or G061; inside G035, G036 | not supported | `graphPatternQuantifier`, `generalQuantifier` (2) |
| G081 or G082; inside G039, G080 | not supported | `simplifiedDirectionOverride` (1) |
| G100 | not supported | `element_idFunction` (1) |
| G110 | not supported | `directedPredicate`, `directedPredicatePart2` (2) |
| G111 | not supported | `labeledPredicate`, `labeledPredicatePart2` (2) |
| G112 | not supported | `sourceDestinationPredicate`, `sourcePredicatePart2`, `destinationPredicatePart2` (3) |
| G113 | not supported | `all_differentPredicate` (1) |
| G114 | not supported | `samePredicate` (1) |
| G115 | not supported | `property_existsPredicate` (1) |
| GA03 | not supported | `nullOrdering` (1) |
| GA05 | not supported | `castSpecification` (1) |
| GA06 | not supported | `valueTypePredicate`, `valueTypePredicatePart2` (2) |
| GC01 | supported | `createSchemaStatement`, `dropSchemaStatement`, `createGraphTypeStatement`, `dropGraphTypeStatement` (4) |
| GC01 or GC04 | supported | `primitiveCatalogModifyingStatement` (1) |
| GC01 or GC04 or GP01 or GP04 | supported | `linearCatalogModifyingStatement`, `simpleCatalogModifyingStatement` (2) |
| GC04 | supported | `dropGraphStatement` (1) |
| GC04+GG01 or GC04+GG02 or GC04+GG04 | supported | `createGraphStatement` (1) |
| GD01 | supported | `primitiveDataModifyingStatement`, `insertStatement`, `setStatement`, `removeStatement` (4) |
| GD01 or GP01 or GP04 | supported | `simpleDataModifyingStatement` (1) |
| GD01+GD03 or GD01+GD04 | not supported | `deleteStatement` (1) |
| GD02; inside GD01 | not supported | `setLabelItem`, `removeLabelItem` (2) |
| GD03 or GD04; inside GD01 | not supported | `deleteItemList`, `deleteItem` (2) |
| GE03 | not supported | `letValueExpression` (1) |
| GE06 | not supported | `pathValueConstructor` (1) |
| GE08 | not supported | `referenceParameterSpecification` (1) |
| GF04 | not supported | `elementsFunction` (1) |
| GF04 or GV50 | not supported | `listValueFunction` (1) |
| GF05 | not supported | `trimMultiCharacterCharacterString` (1) |
| GF06 | not supported | `trimSpecification`, `trimCharacterOrByteString` (2) |
| GF11 | not supported | `binarySetFunction` (1) |
| GF12 or GF13 | not supported | `cardinalityExpression` (1) |
| GG01; inside GC04 | supported | `openGraphType` (1) |
| GG02 or GG04; inside GC04 | supported | `ofGraphType` (1) |
| GG03; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `closedGraphReferenceValueType` (1) |
| GG03; inside GA05, GA06, GC01, GC04, GE03, GG02, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `nestedGraphTypeSpecification` (1) |
| GG04; inside GC01, GC04 | not supported | `graphTypeLikeGraph` (1) |
| GG05; inside GC04 | not supported | `graphSource` (1) |
| GG20; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `nodeTypeName`, `edgeTypeName` (2) |
| GG21; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `nodeTypeKeyLabelSet`, `edgeTypeKeyLabelSet` (2) |
| GH02 | not supported | `fullEdgeUndirected` (1) |
| GH02; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `edgeTypePatternUndirected`, `endpointPairUndirected` (2) |
| GH02; inside GD01 | not supported | `insertEdgeUndirected` (1) |
| GP01 | not supported | `inlineProcedureCall` (1) |
| GP01 or GP04 | supported | `callCatalogModifyingProcedureStatement`, `callDataModifyingProcedureStatement`, `callQueryStatement`, `callProcedureStatement`, `procedureCall` (5) |
| GP03; inside GP01 | not supported | `variableScopeClause` (1) |
| GP04 | supported | `namedProcedureCall` (1) |
| GP05 or GP08 or GP11; inside GP17 | not supported | `bindingVariableDefinition` (1) |
| GP05+GP17 or GP08+GP17 or GP11+GP17 | not supported | `bindingVariableDefinitionBlock` (1) |
| GP16 | not supported | `atSchemaClause` (1) |
| GQ01 | supported | `focusedLinearDataModifyingStatement`, `focusedLinearDataModifyingStatementBody`, `focusedNestedDataModifyingProcedureSpecification`, `focusedLinearQueryStatementPart`, `focusedLinearQueryAndPrimitiveResultStatementPart`, `focusedPrimitiveResultStatement`, `focusedNestedQuerySpecification`, `useGraphClause` (8) |
| GQ02 or GQ03 or GQ04 or GQ05 or GQ06 or GQ07 | not supported | `queryConjunction` (1) |
| GQ03 or GQ04 or GQ05 or GQ06 or GQ07 | not supported | `setOperator` (1) |
| GQ08 | not supported | `filterStatement` (1) |
| GQ09 | not supported | `letStatement` (1) |
| GQ10 or GQ23 | not supported | `forStatement` (1) |
| GQ11 or GQ24; inside GQ10, GQ23 | not supported | `forOrdinalityOrOffset` (1) |
| GQ12 | not supported | `offsetClause` (1) |
| GQ13 | not supported | `limitClause` (1) |
| GQ15 | not supported | `groupByClause` (1) |
| GQ18 | not supported | `valueQueryExpression` (1) |
| GQ19 | not supported | `graphPatternYieldClause` (1) |
| GQ20 | not supported | `nextStatement` (1) |
| GS01 | not supported | `sessionSetGraphParameterClause` (1) |
| GS01 or GS02 or GS03 | not supported | `sessionSetParameterClause` (1) |
| GS02 | not supported | `sessionSetBindingTableParameterClause` (1) |
| GS03 | not supported | `sessionSetValueParameterClause` (1) |
| GS04 or GS05 or GS06 or GS07 or GS08 or GS16 | not supported | `sessionResetCommand` (1) |
| GS15 | not supported | `sessionSetTimeZoneClause` (1) |
| GT01 | supported | `endTransactionCommand`, `startTransactionCommand`, `rollbackCommand`, `commitCommand` (4) |
| GT02; inside GT01 | not supported | `transactionCharacteristics` (1) |
| GV17; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `decimalExactNumericType` (1) |
| GV35; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `byteStringType` (1) |
| GV39 | supported | `dateFunction`, `localtimeFunction`, `localdatetimeFunction`, `dateLiteral` (4) |
| GV39 or GV40 | supported | `datetimeValueFunction`, `temporalLiteral` (2) |
| GV39 or GV40 or GV41; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `temporalType` (1) |
| GV39 or GV40; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `temporalInstantType` (1) |
| GV39; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `localdatetimeType`, `dateType`, `localtimeType` (3) |
| GV40 | supported | `timeFunction`, `datetimeFunction`, `timeLiteral`, `datetimeLiteral` (4) |
| GV40; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `datetimeType`, `timeType` (2) |
| GV41 | supported | `datetimeSubtraction`, `durationValueFunction`, `durationFunction`, `durationLiteral` (4) |
| GV41; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `temporalDurationType` (1) |
| GV45 | not supported | `recordLiteral` (1) |
| GV46 or GV47; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `recordType` (1) |
| GV50 | not supported | `trimListFunction`, `listValueConstructorByEnumeration`, `listLiteral` (3) |
| GV55; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `pathValueType` (1) |
| GV60; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV61 | not supported | `graphReferenceValueType` (1) |
| GV61; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60 | not supported | `bindingTableReferenceValueType` (1) |
| GV71 or GV72; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `immaterialValueType` (1) |
| GV71; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `nullType` (1) |
| GV72; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `emptyType` (1) |
| GV90; inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `notNull` (1) |
| inside G002 | not supported | `edgeBindingsOrEdges`, `edgesSynonym` (2) |
| inside G002, GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `edgeSynonym` (1) |
| inside G003 | not supported | `elementBindingsOrElements` (1) |
| inside G004 | not supported | `pathVariable` (1) |
| inside G005, G016, G019 | not supported | `numberOfPaths` (1) |
| inside G005, G016, G019, G020, GQ12, GQ13 | not supported | `nonNegativeIntegerSpecification` (1) |
| inside G005, G020 | not supported | `numberOfGroups` (1) |
| inside G035, G036, G060 | not supported | `fixedQuantifier` (1) |
| inside G035, G036, G060, G061 | not supported | `lowerBound`, `upperBound` (2) |
| inside G038, G048 | not supported | `subpathVariable` (1) |
| inside G039, G080 | not supported | `simplifiedDefaultingLeft`, `simplifiedDefaultingUndirected`, `simplifiedDefaultingRight`, `simplifiedDefaultingLeftOrUndirected`, `simplifiedDefaultingUndirectedOrRight`, `simplifiedDefaultingLeftOrRight`, `simplifiedDefaultingAnyDirection`, `simplifiedContents`, `simplifiedTerm`, `simplifiedFactorLow`, `simplifiedFactorHigh`, `simplifiedTertiary`, `simplifiedSecondary`, `simplifiedNegation`, `simplifiedPrimary` (15) |
| inside G039, G080, G081, G082 | not supported | `simplifiedOverrideLeft`, `simplifiedOverrideUndirected`, `simplifiedOverrideRight`, `simplifiedOverrideLeftOrUndirected`, `simplifiedOverrideUndirectedOrRight`, `simplifiedOverrideLeftOrRight`, `simplifiedOverrideAnyDirection` (7) |
| inside G111 | not supported | `isLabeledOrColon` (1) |
| inside G112 | not supported | `nodeReference`, `edgeReference` (2) |
| inside GA05 | not supported | `castOperand`, `castTarget` (2) |
| inside GA05, GA06, GC01, GC04, GD01, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `labelSetSpecification` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG01, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03 | supported | `typed` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GH02, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `arcTypeUndirected`, `connectorUndirected` (2) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP04, GP05, GP08, GP17, GQ09, GQ20, GS01, GS02, GS03, GV39, GV40, GV41, GV45, GV46, GV60, GV61 | supported | `fieldName` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `openGraphReferenceValueType` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `nodeTypeSpecification`, `nodeTypePattern`, `nodeTypePhrase`, `nodeTypePhraseFiller`, `nodeTypeFiller`, `localNodeTypeAlias`, `nodeTypeImpliedContent`, `nodeTypeLabelSet`, `nodeTypePropertyTypes`, `edgeTypeSpecification`, `edgeTypePattern`, `edgeTypePhrase`, `edgeTypePhraseFiller`, `edgeTypeFiller`, `edgeTypeImpliedContent`, `edgeTypeLabelSet`, `edgeTypePropertyTypes`, `edgeTypePatternDirected`, `edgeTypePatternPointingRight`, `edgeTypePatternPointingLeft`, `arcTypePointingRight`, `arcTypePointingLeft`, `sourceNodeTypeReference`, `destinationNodeTypeReference`, `edgeKind`, `endpointPairPhrase`, `endpointPair`, `endpointPairDirected`, `endpointPairPointingRight`, `endpointPairPointingLeft`, `connectorPointingRight`, `sourceNodeTypeAlias`, `destinationNodeTypeAlias`, `labelSetPhrase`, `propertyTypesSpecification`, `propertyTypeList`, `propertyType`, `propertyValueType`, `valueType`, `predefinedType`, `booleanType`, `characterStringType`, `minLength`, `maxLength`, `numericType`, `exactNumericType`, `binaryExactNumericType`, `signedBinaryExactNumericType`, `unsignedBinaryExactNumericType`, `verboseBinaryExactNumericType`, `precision`, `scale`, `approximateNumericType`, `referenceValueType`, `nodeReferenceValueType`, `closedNodeReferenceValueType`, `openNodeReferenceValueType`, `edgeReferenceValueType`, `closedEdgeReferenceValueType`, `openEdgeReferenceValueType`, `unsignedDecimalInteger`, `nodeSynonym` (62) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV32, GV35, GV60, GV61 | not supported | `fixedLength` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV46, GV60, GV61 | not supported | `fieldTypesSpecification`, `fieldTypeList`, `fieldType` (3) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP08, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | not supported | `bindingTableType` (1) |
| inside GA05, GA06, GC01, GC04, GE03, GG02, GG03, GP05, GP11, GP17, GQ09, GS01, GS02, GS03, GV60, GV61 | supported | `graphTypeSpecificationBody`, `elementTypeList`, `elementTypeSpecification` (3) |
| inside GC01 | supported | `graphTypeSource`, `copyOfGraphType`, `catalogSchemaParentAndName` (3) |
| inside GC01, GC04, GG02 | supported | `graphTypeReference`, `catalogGraphTypeParentAndName`, `graphTypeName` (3) |
| inside GC04 | supported | `catalogGraphParentAndName` (1) |
| inside GD01 | supported | `setItemList`, `setItem`, `setPropertyItem`, `setAllPropertiesItem`, `removeItemList`, `removeItem`, `removePropertyItem`, `insertGraphPattern`, `insertPathPatternList`, `insertPathPattern`, `insertNodePattern`, `insertEdgePattern`, `insertEdgePointingLeft`, `insertEdgePointingRight`, `insertElementPatternFiller`, `labelAndPropertySetSpecification` (16) |
| inside GE02, GP08, GP17, GS02 | not supported | `bindingTableExpression`, `nestedBindingTableQuerySpecification`, `bindingTableReference`, `bindingTableName`, `delimitedBindingTableName` (5) |
| inside GE03, GP05, GP17, GQ09 | not supported | `valueVariableDefinition` (1) |
| inside GE03, GP05, GP17, GQ09, GS03 | not supported | `optTypedValueInitializer`, `valueInitializer` (2) |
| inside GE03, GQ09 | not supported | `letVariableDefinitionList`, `letVariableDefinition` (2) |
| inside GE06 | not supported | `nodeReferenceValueExpression`, `edgeReferenceValueExpression`, `pathValueConstructorByEnumeration`, `pathElementList`, `pathElementListStart`, `pathElementListStep` (6) |
| inside GF01 | not supported | `modulusExpression`, `numericValueExpressionDividend`, `numericValueExpressionDivisor`, `squareRoot`, `floorFunction`, `ceilingFunction` (6) |
| inside GF01, GF03 | not supported | `exponentialFunction`, `powerFunction`, `numericValueExpressionBase`, `numericValueExpressionExponent` (4) |
| inside GF01, GV41 | supported | `absoluteValueExpression` (1) |
| inside GF02 | not supported | `trigonometricFunction`, `trigonometricFunctionName` (2) |
| inside GF03 | not supported | `generalLogarithmFunction`, `generalLogarithmBase`, `generalLogarithmArgument`, `commonLogarithm`, `naturalLogarithm` (5) |
| inside GF04 | not supported | `pathValueExpression`, `pathLengthExpression` (2) |
| inside GF11 | not supported | `binarySetFunctionType`, `dependentValueExpression`, `independentValueExpression` (3) |
| inside GF12 | not supported | `cardinalityExpressionArgument` (1) |
| inside GF13, GV50 | not supported | `listValueExpression` (1) |
| inside GP01 | not supported | `nestedProcedureSpecification` (1) |
| inside GP01, GP03 | not supported | `bindingVariableReferenceList` (1) |
| inside GP04 | supported | `procedureArgumentList`, `procedureArgument`, `procedureReference`, `catalogProcedureParentAndName`, `procedureName` (5) |
| inside GP04, GQ20 | supported | `yieldClause`, `yieldItemList`, `yieldItem`, `yieldItemName`, `yieldItemAlias` (5) |
| inside GP08, GP17 | not supported | `bindingTableVariableDefinition` (1) |
| inside GP08, GP17, GS02 | not supported | `optTypedBindingTableInitializer`, `bindingTableInitializer` (2) |
| inside GP11, GP17 | not supported | `graphVariableDefinition` (1) |
| inside GP11, GP17, GS01 | not supported | `optTypedGraphInitializer`, `graphInitializer` (2) |
| inside GQ10, GQ23 | not supported | `forItem`, `forItemAlias`, `forItemSource` (3) |
| inside GQ12 | not supported | `offsetSynonym` (1) |
| inside GQ15 | not supported | `groupingElementList`, `groupingElement`, `emptyGroupingSet` (3) |
| inside GQ19 | not supported | `graphPatternYieldItemList`, `graphPatternYieldItem` (2) |
| inside GS01, GS02, GS03 | not supported | `sessionSetParameterName` (1) |
| inside GS01, GS02, GS03, GS04, GS05, GS06, GS07, GS08, GS16 | not supported | `sessionParameterSpecification` (1) |
| inside GS04, GS05, GS06, GS07, GS08, GS16 | not supported | `sessionResetArguments` (1) |
| inside GS15 | not supported | `setTimeZoneValue`, `timeZoneString` (2) |
| inside GT01, GT02 | not supported | `transactionMode`, `transactionAccessMode` (2) |
| inside GV35 | not supported | `byteLengthExpression`, `byteStringValueExpression` (2) |
| inside GV39 | supported | `dateFunctionParameters`, `dateString` (2) |
| inside GV39, GV40 | supported | `timeFunctionParameters`, `datetimeFunctionParameters`, `timeString`, `datetimeString` (4) |
| inside GV39, GV40, GV41, GV45 | supported | `recordConstructor`, `fieldsSpecification`, `fieldList`, `field` (4) |
| inside GV41 | supported | `temporalDurationQualifier`, `datetimeValueExpression`, `datetimeSubtractionParameters`, `datetimeValueExpression1`, `datetimeValueExpression2`, `durationFunctionParameters`, `durationString` (7) |
| inside GV50 | not supported | `listValueTypeName`, `listValueTypeNameSynonym`, `listElementList`, `listElement` (4) |
| unreachable from gqlProgram | never parsed | `pathVariableReference`, `listValueConstructor`, `durationValueExpression` (3) |

| Partial anchors inside supported rules | Rule |
|---|---|
| GC02 (IF) | `createSchemaStatement` |
| GC02 (IF) | `dropSchemaStatement` |
| GC05 (IF) | `createGraphStatement` |
| GG02 (alternative 2); GG02 (alternative 3) | `ofGraphType` |
| GC05 (IF) | `dropGraphStatement` |
| GC03 (IF) | `createGraphTypeStatement` |
| GC03 (IF) | `dropGraphTypeStatement` |
| G030 (alternative 2); G032 (alternative 3) | `pathPatternExpression` |
| G035 or G036 (alternative 2); G037 (alternative 3) | `pathFactor` |
| G074 (alternative 5) | `labelExpression` |
| GV50 (alternative 3); GV50 (alternative 4); GV50 (alternative 5); GV66 (alternative 7); GV67 (alternative 10); GV67 (alternative 9); GV68 (alternative 8) | `valueType` |
| GV30 (minLength); GV31 (alternative 3); GV31 (maxLength); GV32 (alternative 2) | `characterStringType` |
| GV02 (alternative 1); GV04 (alternative 2); GV07 (alternative 3); GV09 (precision); GV12 (alternative 4); GV14 (alternative 5); GV16 (alternative 6); GV18 (alternative 7); GV19 (alternative 9) | `signedBinaryExactNumericType` |
| GV01 (alternative 1); GV03 (alternative 2); GV05 (alternative 7); GV06 (alternative 3); GV08 (alternative 8); GV09 (precision); GV10 (alternative 9); GV11 (alternative 4); GV13 (alternative 5); GV15 (alternative 6) | `unsignedBinaryExactNumericType` |
| GV02 (alternative 1); GV04 (alternative 2); GV07 (alternative 3); GV09 (precision); GV12 (alternative 4); GV14 (alternative 5); GV16 (alternative 6); GV18 (alternative 7); GV19 (alternative 9) | `verboseBinaryExactNumericType` |
| GV20 (alternative 1); GV21 (alternative 2); GV22 (precision); GV23 (alternative 7); GV23 (alternative 8); GV24 (alternative 3); GV25 (alternative 4); GV26 (alternative 5) | `approximateNumericType` |
| GE01 (alternative 12); GE02 (alternative 13); GE07 (XOR) | `valueExpression` |
| GF10 (alternative 6) | `generalSetFunctionType` |
| GF01 (alternative 11); GF01 (alternative 12); GF01 (alternative 13); GF01 (alternative 3); GF01 (alternative 4); GF01 or GF03 (alternative 10); GF01 or GF03 (alternative 9); GF02 (alternative 5); GF03 (alternative 6); GF03 (alternative 7); GF03 (alternative 8) | `numericValueFunction` |
| GF04 (alternative 3); GV35 (alternative 2) | `lengthExpression` |
| GV41 (alternative 2) | `durationValueFunction` |
| GV35 (alternative 3) | `generalLiteral` |
| GL04 (alternative 3); GL05 (alternative 2); GL05 (alternative 4); GL06 (alternative 1) | `exactNumericLiteral` |
| GL07 (alternative 3); GL07 (alternative 4); GL08 (alternative 1) | `approximateNumericLiteral` |
| GL01 (alternative 2); GL02 (alternative 3); GL03 (alternative 4) | `unsignedInteger` |
