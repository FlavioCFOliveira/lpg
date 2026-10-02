---
title: Values and types
status: draft
last-reviewed: 2026-10-02
related:
  - query-engine.md
  - bolt.md
  - operations.md
---

# Values and types

## TS-21 Values and types

- **Decision**:
  - **Runtime value.** A 24-byte `enum Value`:
    - inline: null, boolean, `i64`, `f64`, node and edge identifiers, date, local time, local date-time, and both duration types;
    - reference-counted slices (`Arc<str>`, `Arc<[u8]>`, `Arc<[Value]>`): strings, byte strings, lists, records, paths;
    - boxed: rare types (zoned values with a named time zone, points, and any wider numeric types).
  - **Temporal.** Arithmetic, IANA time zones and the current time come from `jiff`. Zone data is read from the operating system (`/usr/share/zoneinfo`, or `TZDIR`). New sessions use UTC. Transaction, statement and realtime clocks read the system clock.
  - **Spatial.**
    - `Point2D` and `Point3D` carry an SRID: 7203, 9157, 4326 or 4979.
    - Distance follows Neo4j:
      - WGS-84 2D: haversine with Earth radius 6 378 140 m;
      - WGS-84 3D: the haversine distance at the mean height, combined with the height difference by Pythagoras;
      - Cartesian: Euclidean;
      - points in different CRSs: null.
    - Point keys for indexes use the Hilbert encoding of [TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints).
  - **Record encoding** in the node and edge trees: sorted label tokens, then properties sorted by key token. Each property has a compact header (type tag, key-token delta, length class) and a minimum-width payload. Values above the inline threshold go to overflow pages ([TS-05](storage.md#ts-05-files-and-pages)).
  - **Index key encoding.** Order-preserving bytes per type, separate from the record encoding.
  - **Lexical forms**, used by import, export and literals:
    - temporal values: ISO 8601 text, with an IANA zone name in brackets for named zones;
    - points: `{srid, x, y[, z]}`.
  - **Open points.** Wider numeric types (`INT128`, `INT256`, unsigned, `DECIMAL`, `FLOAT16`/`128`/`256`), any crate for them, cross-type comparison and the total order for `ORDER BY` wait for the ISO text (OQ-005, BR-GQL-002). Whether Point2D and Point3D conflict with the norm is OQ-006.
- **Satisfies**: FR-TYPE-001, FR-TYPE-002, FR-TYPE-004, FR-TYPE-006, FR-TYPE-007, FR-SES-003, FR-BOLT-005, BR-BOLT-001, FR-IMP-006, NFR-PERF-003.
- **Evidence**:
  - jiff reads `/usr/share/zoneinfo`, or `TZDIR`, by default on Unix, and bundles zone data only with the `tzdb-bundle-*` features (D-JIFF).
  - chrono-tz compiles the IANA database into the binary (D-CTZ). time offers UTC offsets only (D-TIME).
  - Neo4j geographic distance (R-NEO `community/values/.../storable/CRSCalculator.java:130-166`).
  - Memgraph's compact property store (R-MG `src/storage/v2/property_store.cpp:63-118`).
  - Measured `Value` sizes (M-QE §Q5): 24 bytes with `Arc` slices, 32 bytes with owned `String` and `Vec`.
- **Crates**:
  - `jiff` 0.2.37 (Unlicense OR MIT, 209.3 M downloads): dependency.
  - `chrono` with `chrono-tz`: rejected, bundled zone data fails FR-TYPE-007.
  - `time`: rejected, no IANA zone names.
  - `geo` 0.33.1: not used; the two Neo4j formulas must be reproduced exactly and need no dependency.
- **Rejected**:
  - Owned `String` and `Vec` variants: 32 bytes per value, and every row clone copies the data.
  - A 16-byte value with thin pointers: one more indirection, with no measurement showing it pays.
