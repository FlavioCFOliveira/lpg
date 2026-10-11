---
title: Values and types
status: draft
last-reviewed: 2026-10-11
related:
  - query-engine.md
  - bolt.md
  - operations.md
---

# Values and types

## TS-21 Values and types

- **Decision**:
  - **Runtime value.** A 24-byte `enum Value` (`src/types/mod.rs:38-81`; size checked by `a_value_is_24_bytes`, `src/types/tests.rs:91-92`):
    - inline: null, boolean, `i64`, `f64`, node and edge identifiers, date, local time, local date-time, zoned time (local time and offset; a time has no named zone), and both duration types;
    - reference-counted slices (`Arc<str>`, `Arc<[u8]>`, `Arc<[Value]>`): strings, byte strings, lists, records, paths;
    - boxed: every zoned date-time, named or offset-only, and points (`src/types/mod.rs:59-63, 80`).
  - **Temporal.** Arithmetic, IANA time zones and the current time come from `jiff`. Zone rules are read from the operating system (`/usr/share/zoneinfo`, or `TZDIR`) through jiff's `tzdb-zoneinfo` feature (`Cargo.toml:15`). New sessions use UTC. Transaction, statement and realtime clocks read the system clock.
    - **Zone links** (`src/types/zone_links.rs`). Zone names are normalised through a table built into the crate before they enter an index key or the zoned date-time order (`normalize_zone`, `src/types/temporal.rs:51-59`). The table holds the 253 `Link` lines of IANA tzdata 2026e in its default build, without `backzone` (`etcetera` 1, `backward` 252; public domain). It is not read from the host: a host's `tzdata.zi` may be built with `backzone`, which makes some links zones (`src/types/temporal.rs:17-23`), so index keys would depend on the host.
    - The table is part of format version 1. Changing it needs a new format version and a migration that rebuilds the index keys holding zone names ([TS-28](operations.md#ts-28-format-versioning-and-migration)).
    - **Zoned date-time flag.** `jiff` turns a zero offset into its UTC zone, which has a name (`TimeZone::fixed`, `iana_name`), so a runtime zoned date-time carries a `named` flag that keeps an offset-only `+00:00` value offset-only (`src/types/mod.rs:101-115`).
    - **Stored named zones.** Decoding a stored named zone keeps its name and its stored offset when the host lacks the zone or the zone's rules give another offset at that instant (`ZonedDateTime::stored`, `src/types/mod.rs:129-149`).
    - **`DURATION(DAY TO SECOND)`** holds seconds and nanoseconds; each day is folded into 86 400 seconds (`src/types/mod.rs:66-68, 172-181`).
  - **Spatial.**
    - `Point2D` and `Point3D` carry an SRID: 7203, 9157, 4326 or 4979.
    - Distance follows Neo4j:
      - WGS-84 2D: haversine with Earth radius 6 378 140 m;
      - WGS-84 3D: the haversine distance at the mean height, combined with the height difference by Pythagoras;
      - Cartesian: Euclidean;
      - points in different CRSs: null.
    - Point keys for indexes use the Hilbert encoding of [TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints).
  - **Record encoding** in the node and edge trees (`src/types/record.rs:1-32`): sorted label tokens, then properties sorted by key token. Each property has a compact header (type tag, key-token delta, length class) and a minimum-width payload. Values above the inline threshold go to overflow pages ([TS-05](storage.md#ts-05-files-and-pages)).
    - **Header byte.** Type tag in the high five bits, length class in the low three. Tags: 0 null, 1 boolean, 2 integer, 3 float, 4 string, 5 byte string, 6 list, 7 record, 8 path, 9 node, 10 edge, 11 date, 12 local time, 13 local date-time, 14 zoned time, 15 zoned date-time, 16 `DURATION(YEAR TO MONTH)`, 17 `DURATION(DAY TO SECOND)`, 18 point; 19–27 reserved (Numeric types, below).
    - **Zoned date-time.** Class 0: the instant, then the offset. Class 1: the instant, then the zone name, then the offset. The instant is the floor of its Unix seconds (zigzag varint), then nanoseconds in `0..1e9` (varint), as Neo4j's property store keeps it (R-NEO `community/record-storage-engine/.../store/TemporalType.java:418-446`, `community/values/.../utils/TemporalValueWriterAdapter.java:68-80`).
  - **Index key encoding.** Order-preserving bytes per type, separate from the record encoding.
  - **Lexical forms and type names**, used by import, export and literals. A lexical form is the text of a CSV field, of a GraphML `<data>` element and of the `"$value"` string of a JSON Lines tagged object; the type name is the text of an `lpg:<type name>` CSV column, of `"$lpg_type"` and of the GraphML type attribute ([TS-26](operations.md#ts-26-backup-restore-import-and-export)). One row per type of Runtime value; each example is a unit test that formats and parses it.

    | Type | Type name | Lexical form (written; read) | Source | Example |
    |---|---|---|---|---|
    | null | `NULL` | `NULL`; inside a list, a record or a result column only | `nullLiteral` (`GQL.g4:3008`) | `NULL` |
    | boolean | `BOOLEAN` | `TRUE` or `FALSE`; read case-insensitively | `BOOLEAN_LITERAL` (`GQL.g4:3111`) | `FALSE` |
    | integer | `INTEGER` | Rust's `i64` `Display`; read with `i64::from_str` (an optional sign, then digits) | Rust 1.99.0 `library/core/src/num/mod.rs:1608` | `-9223372036854775808` |
    | float | `FLOAT` | finite: Rust's `f64` `Debug` text, the shortest that round-trips, with a `.` or an `e` (`e` below 10<sup>−4</sup> and from 10<sup>16</sup>); non-finite: `NaN`, `Infinity`, `-Infinity`. Read: those three, or a text holding `.`, `e` or `E` that `f64::from_str` accepts, which returns "the closest representable" value | Rust 1.99.0 `library/core/src/fmt/float.rs:12-31, 180-203`, `library/core/src/num/float_parse.rs:56-60`; measured 2026-10-11: `1e16`, `1.25e-7`, `-0.0`, `5e-324` | `-0.0`, `1.25e-7`, `NaN` |
    | character string | `STRING` | single-quoted; `\` written `\\`, `'` written `\'`, and U+0000–U+001F, U+FFFE and U+FFFF written `\uXXXX` (upper-case hexadecimal). Read: `SINGLE_QUOTED_CHARACTER_SEQUENCE` without the `@` prefix, with every `ESCAPED_CHARACTER` and `''`; an escape that names no Unicode scalar value fails, where the GQL parser substitutes U+FFFD (`src/query/parser/mod.rs:525-581`) | `characterStringLiteral` (`GQL.g4:2972, 3117, 3145, 3157-3186`) | `'it\'s\u000D'` |
    | byte string | `BYTES` | `X'`, two upper-case hexadecimal digits per byte, `'`; read also with the spaces `BYTE_STRING_LITERAL` allows | `BYTE_STRING_LITERAL` (`GQL.g4:3188`) | `X'00FF'` |
    | date | `DATE` | ISO 8601 calendar date. Read also as an ISO 8601 week date (`2015-W30-2`), ordinal date (`2015-202`), basic form (`20150721`) or reduced-precision date (`2015-07`, `2015-W30`, `2015`), as Neo4j accepts (Cypher Manual, "Temporal values"; openCypher TCK `Temporal2.feature` [1]; rmp #52); Neo4j's quarter dates are not ISO 8601 and are not accepted | `temporal::format`, `parse_date` (`src/types/temporal.rs:64-105, 161-202`) | `2015-07-21` |
    | local time | `LOCAL TIME` | ISO 8601 `hh:mm:ss` with up to 9 fraction digits | `src/types/temporal.rs:64-105, 209` | `21:40:32.142` |
    | zoned time | `ZONED TIME` | local time, then the offset `±hh:mm` or `±hh:mm:ss` | `offset_text`, `split_offset` (`src/types/temporal.rs:109-118, 126-150, 211-217`) | `21:40:32+01:00` |
    | local date-time | `LOCAL DATETIME` | ISO 8601 `yyyy-mm-ddThh:mm:ss` with up to 9 fraction digits | `src/types/temporal.rs:64-105, 210` | `2015-07-21T21:40:32` |
    | zoned date-time | `ZONED DATETIME` | local date-time, the offset, then, for a named zone, the IANA zone name in brackets | `src/types/temporal.rs:64-82, 218-229` | `2015-07-21T21:40:32+02:00[Europe/Berlin]`; offset-only: `2015-07-21T21:40:32+01:00` |
    | `DURATION(YEAR TO MONTH)` | `DURATION(YEAR TO MONTH)` | an optional `-`, then `P<years>Y<months>M`, months below 12 | `src/types/temporal.rs:83-87, 230-240` | `-P1Y2M` |
    | `DURATION(DAY TO SECOND)` | `DURATION(DAY TO SECOND)` | an optional `-`, then `P<days>DT<hours>H<minutes>M<seconds>S`, the seconds with up to 9 fraction digits and no trailing zero | `src/types/temporal.rs:88-101, 241-268` | `P1DT2H3M4.5S` |
    | point | `POINT` | `{srid: S, x: X, y: Y}` or `{srid: S, x: X, y: Y, z: Z}`, coordinates in the float form | `Point::lexical`, `Point::parse` (`src/types/spatial.rs:153-190`) | `{srid: 4979, x: 1.0, y: 2.0, z: 3.0}` |
    | list | `LIST<T>` when the list is not empty, holds no null and every element has the type name T; otherwise `LIST<ANY>` | `[`, the elements in element form separated by `, `, `]` | `listValueConstructorByEnumeration` (`GQL.g4:2500`); `LIST<INNER_TYPE>` and `ANY` (Neo4j, "Types and their synonyms", Cypher 25, checked 2026-10-11) | `[1, 'a', DATE '2015-07-21', [2.5, NULL]]` |
    | record | `RECORD` | `RECORD {`, the fields `name: element` separated by `, `, `}`. A name that matches `[A-Za-z_][A-Za-z0-9_]*` is written bare; any other is accent-quoted with the string escapes, `` ` `` in place of `'` | `recordConstructor`, `field`, `ACCENT_QUOTED_CHARACTER_SEQUENCE` (`GQL.g4:2514, 2528, 3125`) | ``RECORD {a: 1, `b c`: 'x'}`` |
    | node | `NODE` | the record form of APOC's node map: `RECORD {id: <identifier>, labels: [<label strings>], properties: RECORD {…}}`. Read: the node with identifier `id` | APOC `FormatUtils.toMap` (R-APOC `common/src/main/java/apoc/export/util/FormatUtils.java:66-70`) | `RECORD {id: 12, labels: ['A'], properties: RECORD {name: 'x'}}` |
    | edge | `RELATIONSHIP` | `RECORD {id: <identifier>, type: '<edge type>', start: <identifier>, end: <identifier>, properties: RECORD {…}}`. Read: the edge with identifier `id` | `FormatUtils.java:72-85` | `RECORD {id: 7, type: 'R', start: 12, end: 13, properties: RECORD {}}` |
    | path | `PATH` | the list form of its nodes and edges in path order | `FormatUtils.java:96-100` | `[NODE 'RECORD {id: 1, labels: [], properties: RECORD {}}', RELATIONSHIP 'RECORD {id: 5, type: \'R\', start: 1, end: 2, properties: RECORD {}}', NODE 'RECORD {id: 2, labels: [], properties: RECORD {}}']` |

    - **Element form**, inside a list or a record: a null, boolean, integer, float, string, byte string, list or record is written in its lexical form, which identifies its type; any other value as its type name, a space, and its lexical form written as a character string (row above). A date element is therefore GQL's `dateLiteral` (`GQL.g4:2936`).
    - **Type names** are those of IV008 (below), written exactly as in the table and read case-sensitively. A type name holds no `:`, so in `<type name>:<lexical form>` the first `:` ends it.
    - A date shifted by a duration with a time-of-day part, or a time by a duration with months, raises 22G14 (gql-compat `condition/22014/incompatible-temporal-units`).
  - **Features.** The temporal types implement features GV39, GV40 and GV41, verified against the ISO features insert [S45].
  - **Implementation-defined choices** (BR-GQL-002). Each was approved by the user on 2026-10-07 (rmp task #26). Codes and line numbers are those of the ISO implementation-defined insert [S45], `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-implementation-defined.xml`.

    | Item | Codes (insert line) | Choice |
    |---|---|---|
    | Comparison across types | IV002 (398), IV010 (410) | Values of different type groups are not comparable: `=` is false, and `<`, `<=`, `>`, `>=` are null. Integers and floats are one group and compare by exact numeric value. |
    | Null ordering | IS001 (390) | NULL sorts after every other value in ascending order. |
    | Group order of the total order (`ORDER BY`, index keys) | IS001 (390) | Neo4j's: record, node, edge, list, path, point, zoned date-time, local date-time, date, zoned time, local time, duration, character string, byte string, boolean, number, then NULL. Neo4j has no byte-string group; its place right after character strings is a project decision from references (below). |
    | Order within a group | IA015 (98), IA016 (102) | Character strings by Unicode code point, which is the byte order of their UTF-8 encoding, with no padding. Byte strings that differ only in trailing X'00' bytes are different. Lists lexicographically, element by element. Byte strings by unsigned byte, a proper prefix first (project decision from references, below). NaN sorts above every number. −0.0 equals +0.0: the user's decision, which deviates from Neo4j's order (below). Zoned values with the same instant by offset, then offset-only zones before named zones, then named zones by their zone identifiers normalised once through the fixed IANA zone link table (Temporal, above) and compared as strings, then by chronology (Neo4j); the link table replaces Neo4j's `TimeZones.map`, which is GPL and not copied (user decision 2026-10-08, [FR-TYPE-007](../functional-spec/functional-requirements.md#types)). Durations by average length in seconds (months × 2 629 746 + days × 86 400 + seconds), then nanoseconds, months, days and seconds (Neo4j). A `DURATION(DAY TO SECOND)` folds its days into its seconds (Temporal, above), so its days part is 0 and `P1D` = `PT24H`: a user-approved exception to Neo4j's order (user decision 2026-10-08, [BR-GQL-002](../functional-spec/functional-requirements.md#gql)). Points by CRS, then coordinates in axis order, with −0.0 = +0.0. |
    | Numeric types | features GV11, GV13–GV17, GV20, GV25, GV26 (features insert lines 777–837); ID028 (174), ID034 (178), ID037 (182), IL011 (362), IL023 (382) | INT64 (64-bit two's complement) and FLOAT64 (IEEE 754 binary64) only. The listed features are not supported; the record encoding reserves a type tag for each of their types. There is no decimal type, so ID034 has no value. |
    | Temporal | IL024 (386), IA026 (130), ID048 (186) | Fractional seconds to nanoseconds (9 digits); no leap seconds; default time zone displacement UTC. |
    | Sizes | IL001 (342), IL002 (346), IL013 (366), IL015 (370) | Label-set and property-set cardinality, string length (in UTF-8 bytes) and the cardinality of lists and records are each bounded by a `u32` count or length field of the record encoding (2<sup>32</sup> − 1) and by the memory cap ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). An edge has exactly one edge type ([TS-06](storage.md#ts-06-graph-layout)), so its label set has one label. No other minimum is set: the minimum the standard requires is in its rules, which are not available (OQ-005). |
    | Transactions | ID006 (154), IE002 (306), IE004 (314) | Serializable by default; snapshot isolation is the one relaxation, selected per transaction through the Rust API only ([FR-TX-003, FR-TX-004](../functional-spec/functional-requirements.md#transactions); [TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)). Confirmed by the user on 2026-10-07. |
    | Directory nesting | IL020 (378) | No fixed maximum depth. The 1 024-byte key limit ([TS-05](storage.md#ts-05-files-and-pages)) bounds each name, not the depth, because a catalog key holds the parent's identifier and the name ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)). |

  - **Implementation-defined choices of 2026-10-08** (BR-GQL-002; the user approved the rule "Neo4j first, then Memgraph, else the reference the blocking task cites", rmp task #38). "zu" and the place of Memgraph are as in [TS-19](query-engine.md#ts-19-parser); tasks #42 and #43 cite zu. Neo4j paths are under R-NEO `community/`.

    | Item (line) | Choice | Source |
    |---|---|---|
    | IA003 (58) | No implicit normalisation: every operation reads the code points as written. | Neo4j compares strings with Java `String.equals` (`values/.../storable/StringValue.java:51-53`); zu. |
    | IW017 (490) | A concatenation is the left operand's code points followed by the right's, not normalised. | As IA003; zu. |
    | IA004 (62) | IEEE 754 binary64, round to nearest, ties to even. | Neo4j parses float literals with `Double.parseDouble` (`cypher/front-end/expressions/.../Literal.scala:136-141`), which rounds "by the usual round-to-nearest rule of IEEE 754" (Java SE API, `Double.valueOf(String)`). |
    | IA006 (70) | One approximation: the nearest `FLOAT64`, ties to even. | Neo4j has one approximate type, `FLOAT`, held as a Java `double` (`storable/DoubleValue.java:30`; numeric literals are `INTEGER` or `FLOAT`, Cypher Manual, "Boolean, numeric, and string literals") |
    | IA007 (74) | None: only `FLOAT64` values are approximate. | As IA006: Neo4j's `INTEGER` is a Java `long` (`storable/LongValue.java:30`) |
    | IA005 (66), IA021 (118) | The one implicit assignment that can lose digits is an `INT64` argument to a `FLOAT64` routine parameter: it rounds to nearest, ties to even, with no condition. A value that does not fit any other target fails with 22G03. | The openCypher TCK, from Neo4j, has a `FLOAT` parameter accept an `INTEGER` (R-OCTCK `clauses/call/Call3.feature`, scenario 5); the JVM converts `long` to `double` by round to nearest (JLS §5.1.2). |
    | IA010 (78) | `INT64` arithmetic is exact from −2<sup>63</sup> to 2<sup>63</sup> − 1. `+`, `−` and `*` outside that range, and `INT64` minimum ÷ −1, raise 22003; division by zero, integer or float, raises 22012. A `FLOAT64` operation whose result is infinite or NaN raises 22003: feature GA01, IEEE 754 operations, is not supported. User decision 2026-10-08, "Erro sempre" (rmp task #38). | Neo4j adds with `Math.addExact` (`values/.../utils/ValueMath.java:48-50`) and refuses an integer zero divisor (`cypher/runtime-util/.../CypherMath.java:274-280`), but returns infinity or NaN for floats (`storable/FloatingPointValue.java:124-125`) and wraps minimum ÷ −1 (`IntegralValue.java:128-130`); the user chose the error, as zu does. |
    | IA011 (82) | Integer division truncates toward zero (`7 / 2` is 3, `-7 / 2` is −3); float division rounds to nearest. | Neo4j divides `long` values with Java `/` (`values/.../storable/IntegralValue.java:128-130`). |
    | ID059 (202) | `COUNT` returns `INT64`. | Neo4j `count()` returns `INTEGER` (Cypher Manual, "Aggregating functions"). |
    | ID063 (214) | `FLOAT64` when either operand is `FLOAT64`; the `INT64` operand is converted. | Neo4j `IntegralValue.plus(double)` returns a `DoubleValue` (`IntegralValue.java:113-115`). |
    | ID064 (218) | `INT64` when both operands are `INT64`. | `ValueMath.java:48-50`. |
    | ID065 (222), ID066 (226), ID074 (246) | 64 bits; a result outside the range raises 22003. | IA010. |
    | ID067 (230) | `INT64`, scale 0, truncated toward zero; a zero divisor raises 22012. | IA010, IA011. |
    | ID068 (234) | `INT64`. | Neo4j's one exact type, `INTEGER` (IA007) |
    | ID075 (250) | Binary64: 53-bit significand. | Neo4j's `FLOAT` is a Java `double` (IA006) |
    | ID079 (258) | `FLOAT64`. | Neo4j types a decimal double literal `CTFloat` (`cypher/front-end/ast/.../SemanticExpressionCheck.scala:889-895`). |
    | ID095 (282) | `SUM` of `INT64` values is `INT64`; a total outside the `INT64` range becomes `FLOAT64`. | Neo4j sums with `overflowSafeAdd`, which "widen[s] type to use double" (`ValueMath.java:60-84`; `interpreted-runtime/.../aggregation/SumFunction.scala`). |
    | ID096 (286) | None: `AVG` of exact values is `FLOAT64`. | Neo4j `AvgFunction` returns a double for numbers (`interpreted-runtime/.../aggregation/AvgFunction.scala`, `result`); zu. |
    | ID097 (290), ID098 (294) | `FLOAT64`. | Neo4j `sum()` of floats, `avg()`, `stDev()` and `stDevP()` return `FLOAT` (Cypher Manual, "Aggregating functions"). |
    | IV023 (434) | Truncating whitespace is U+0020 only. User decision 2026-10-08, "Só U+0020" (rmp task #38). | Neo4j's conformance page states that GQL `TRIM` removes only space characters (Cypher Manual, "Supported mandatory GQL features", 20.24), unlike Cypher's `trim()` (`storable/StringWrappingStringValue.java:119-123`); zu. |
    | IL009 (354) | No minimum below the sum: a concatenation is as long as its operands together; one above 2<sup>32</sup> − 1 bytes fails with 22001. | Neo4j documents no limit; Memgraph has none in R-MG `src/query`, `src/utils`, `src/storage/v2` (searched 2026-10-08); zu. Sizes (IL013, above) |
    | IA014 (94), ID085 (262) | No condition: a NULL whose type no context gives has the type `NULL`. | Neo4j's type `NULL` (Cypher Manual, "Property, structural, and constructed values", "Types and their synonyms"). |
    | ID022 (166) | Unicode code-point order, with no locale. | Order within a group (IA015, above). |
    | ID023 (170) | `STRING`; `BYTES` for byte strings. | Neo4j's type name `STRING` ("Types and their synonyms"); neither Neo4j nor Memgraph has byte strings (Byte strings, below), so `BYTES` is zu's |
    | ID089 (270) | `GRAPH`. | Neo4j gives no type name to graph references (Cypher Manual, "Graph references"); Memgraph's `valueType()` names a graph value `GRAPH` (R-MG @ `847e224` `src/query/interpret/awesome_memgraph_functions.cpp:922-923`) |
    | ID090 (274) | `NODE`. | Neo4j (same page as ID023). |
    | ID091 (278) | `RELATIONSHIP`; `EDGE` is a synonym. | Neo4j (same page). zu chooses `EDGE`. |
    | IV008 (406) | Neo4j's default type names and union normalisation; for types Neo4j lacks: `BYTES`, `DURATION(YEAR TO MONTH)`, `DURATION(DAY TO SECOND)`, `RECORD`. | Neo4j, "Types and their synonyms", "Type normalization". |
    | IW019 (498) | The member that encompasses the others, if one does; otherwise the closed dynamic union of the members. | Neo4j, "Type normalization". |
    | IW021 (502) | Neo4j's order: `NOTHING`, `NULL`, `BOOLEAN`, `STRING`, `BYTES` (lpg, beside the other string type), `INTEGER`, `FLOAT`, `DATE`, `LOCAL TIME`, `ZONED TIME`, `LOCAL DATETIME`, `ZONED DATETIME`, `DURATION(YEAR TO MONTH)`, `DURATION(DAY TO SECOND)`, `POINT`, `NODE`, `RELATIONSHIP`, `RECORD` (Neo4j's `MAP` place), `LIST` by inner type, `PATH`, dynamic unions, `ANY`; subtypes before the types enclosing them. | Neo4j, "Type normalization", "Ordering of types". |
    | IV011 (414) | `PROPERTY VALUE`: `BOOLEAN`, `INTEGER`, `FLOAT`, `STRING`, `BYTES`, the five temporal instant types, both duration types, `POINT`, and lists of one of these without null. Setting any other value as a property fails with 22G03. | Neo4j's property types; lists "homogeneous and contain no `null` values" (Cypher Manual, "Property, structural, and constructed values"). zu adds byte strings, which lpg stores (tag 5). |
    | IV012 (418) | Every value type of Runtime value, above. | Neo4j `ANY`; zu. |
    | IV014 (422) | `ANY`. | Neo4j; zu. |
    | IL003 (350) | A node type and an edge type each have exactly one key label. Element types are written with feature GG03, supported since 2026-10-08 ("Só GG03", rmp task #37). | Neo4j: each node element type has one identifying label and each relationship element type one identifying type (Cypher Manual, "Set graph types"). zu allows 0 to 63. |
    | IV003 (402) | A graph type's normal form is the text that re-creates it, with element types, labels and property types sorted by name. Applies with GG03, as IL003. | Neo4j's `SHOW CURRENT GRAPH TYPE` returns the type "using the canonical form", reusable to re-create it; its examples sort by name, which the manual does not state as a rule (Cypher Manual, "Show graph types"). |

    Not reached by the supported codes (TS-19): ID004 (146, constructors without values: GV45, GV50), ID057 (194) and ID058 (198) (`WITH ORDINALITY`, `WITH OFFSET`: GQ11, GQ24), ID062 (210, `LIMIT`, `OFFSET`, path counts: GQ12, GQ13, G016, G019, G020), ID069 (238, GF01–GF03), ID070 (242, GF12, GF13), ID099 (298, GF11).
  - **Points** (user decision 2026-10-08, OQ-006): points enter and leave only as values, through parameters, properties and results. Constructors, distance and the distance-within search that uses the point index are built-in procedures invoked with `CALL`: `point`, `point.distance`, `point.nodesWithinDistance`, `point.edgesWithinDistance` ([TS-24](interfaces.md#ts-24-procedures-functions-metrics-and-events)). Whether Point2D and Point3D conflict with the norm stays unverified (OQ-006).
- **Satisfies**: FR-TYPE-001, FR-TYPE-002, FR-TYPE-004, FR-TYPE-006, FR-TYPE-007, FR-SES-003, FR-BOLT-005, BR-BOLT-001, FR-IMP-006, NFR-PERF-003, BR-GQL-002.
- **Evidence**:
  - jiff reads `/usr/share/zoneinfo`, or `TZDIR`, by default on Unix, and bundles zone data only with the `tzdb-bundle-*` features (D-JIFF).
  - jiff 0.2.38: `TimeZone::fixed` returns `TimeZone::UTC` for a zero offset (`src/tz/timezone.rs:457-464`), and `iana_name` returns `"UTC"` for it and `None` for a fixed offset; it only canonicalises case and resolves no link (`src/tz/timezone.rs:652-679`).
  - Zone link table. IANA's `Makefile` compiles `TDATA` = the primary regions, `etcetera`, `factory` and `backward`, not `backzone` (tzdata 2026e `Makefile:653-658`). Neo4j's `TimeZones.map` reads a data file shipped with Neo4j (R-NEO `storable/TimeZones.java`, resource `TZIDS`); it is GPL and is not copied (`src/types/temporal.rs:13-18`). Whether the link table gives the same order as `TimeZones.map` for every zone name is not verified. A jiff `Timestamp` has nanosecond precision, and "Jiff does not support leap seconds" (D-JIFF).
  - Implementation-defined choices, Neo4j behaviour (R-NEO `community/values/src/main/java/org/neo4j/values/`):
    - Different value groups compare as undefined, which yields null (`storable/ValueComparator.java:52-64`). Integers and floats are one group and compare by value (`storable/NumberValue.java:61-70`), exactly also beyond 2<sup>53</sup> (`storable/NumberValues.java:178-194`).
    - Group order: virtual values (`MAP`, `NODE`, `EDGE`, `LIST`, `PATH`, `virtual/VirtualValueGroup.java:26-33`) before stored values (`AnyValueComparator.java:69-84`), stored values in `storable/ValueGroup.java:55-66` order, lists placed in the `LIST` position (`AnyValueComparator.java:151-158`), and NULL above every value (`AnyValueComparator.java:45-55`).
    - Floats compare with Java `Double.compare` (`storable/FloatingPointValue.java:75-77`), which places NaN above every number and −0.0 below +0.0. Measured with OpenJDK 25.0.4.1: `Double.compare(Double.NaN, Double.POSITIVE_INFINITY)` is 1 and `Double.compare(-0.0, 0.0)` is −1. lpg's −0.0 = +0.0 is the user's decision of 2026-10-07, part of the option the user chose, and deviates from Neo4j.
    - Durations compare by average length in seconds, then nanoseconds, months, days and seconds (`storable/DurationValue.java:86-93`). The average length is seconds + days × `SECONDS_PER_DAY` + months × `AVG_SECONDS_PER_MONTH` (`DurationValue.java:263-271`), with `AVG_SECONDS_PER_MONTH = 2_629_746` and `SECONDS_PER_DAY` = `ChronoUnit.DAYS.getDuration().getSeconds()` (`utils/TemporalUtil.java:33-34`); measured with OpenJDK 25.0.4.1, the latter is 86 400.
    - Zoned date-times compare by instant, then offset, then offset-only zones before named zones, then the normalised zone identifiers, `TimeZones.map(TimeZones.map(id))` compared with `String.compareTo`, then chronology (`storable/DateTimeValue.java:477-507, 509-513`). Points compare by CRS code, then coordinates (`storable/PointValue.java:179-199`).
  - Byte strings. Neo4j has no byte-string value group (`storable/ValueGroup.java:32-66`), and Memgraph has no byte-string property type (R-MG @ `847e224` `src/storage/v2/property_value.cppm:42-59`). PostgreSQL orders `bytea` by `memcmp` over the common length, then the shorter first (R-PG @ `1f22453` `src/backend/utils/adt/bytea.c:959-978`); `memcmp` compares bytes "each interpreted as unsigned char" (Linux man-pages 6.17, memcmp(3)). The group sits after character strings, the other string type in GQL, so the two string groups are adjacent.
  - The byte order of UTF-8 strings equals their code-point order: "The byte-value lexicographic sorting order of UTF-8 strings is the same as if ordered by character numbers" (RFC 3629, §1).
  - chrono-tz compiles the IANA database into the binary (D-CTZ). time offers UTC offsets only (D-TIME).
  - Neo4j geographic distance (R-NEO `community/values/.../storable/CRSCalculator.java:130-166`).
  - Memgraph's compact property store (R-MG `src/storage/v2/property_store.cpp:63-118`).
  - Measured `Value` sizes (M-QE §Q5): 24 bytes with `Arc` slices, 32 bytes with owned `String` and `Vec`.
- **Crates**:
  - `jiff` 0.2.38 (Unlicense OR MIT, 216.0 M downloads, crates.io 2026-10-07): dependency.
  - `chrono` with `chrono-tz`: rejected, bundled zone data fails FR-TYPE-007.
  - `time`: rejected, no IANA zone names.
  - `geo` 0.33.1: not used; the two Neo4j formulas must be reproduced exactly and need no dependency.
- **Rejected**:
  - Owned `String` and `Vec` variants: 32 bytes per value, and every row clone copies the data.
  - A 16-byte value with thin pointers: one more indirection, with no measurement showing it pays.
