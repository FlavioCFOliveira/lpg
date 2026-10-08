---
title: Values and types
status: draft
last-reviewed: 2026-10-08
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
  - **Lexical forms**, used by import, export and literals:
    - temporal values: ISO 8601 text, with an IANA zone name in brackets for named zones;
    - points: `{srid, x, y[, z]}`.
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

  - **Not decided.** Whether Point2D and Point3D conflict with the norm is OQ-006.
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
