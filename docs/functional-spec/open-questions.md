---
title: Open questions
status: draft
last-reviewed: 2026-09-24
related:
  - functional-requirements.md
  - non-functional-requirements.md
  - application-abstractions.md
  - graph-algorithms.md
---

# Open questions

Each item under "Open" is undecided. When the user decides an item, the decision is recorded in the affected requirements and the item moves to "Closed" with a pointer to them. Identifiers are never reused.

## Open

### OQ-001

Performance targets ([NFR-PERF-001](non-functional-requirements.md#performance)). The workloads and the comparison are decided ([NFR-PERF-002](non-functional-requirements.md#performance)). The metrics and the numeric targets are open, to be set from the measured baseline.

### OQ-005

GQL implementation-defined choices and feature identifiers ([FR-GQL-001](functional-requirements.md#gql)). The choices that ISO/IEC 39075:2024 leaves to the implementation are decided one by one with the user's explicit approval ([BR-GQL-002](functional-requirements.md#gql)); they stay open until the normative text is available, which the user will provide outside the repository. The feature identifiers cited in this specification (GG01, GG02, GV39) the serialisation-failure GQLSTATUS code ([FR-TX-008](functional-requirements.md#transactions)) and the validity of the built-in procedure names as GQL identifiers ([FR-PROC-004](functional-requirements.md#procedures-and-functions)) are unverified until checked against that text. The concrete order of optional features ([FR-GQL-006](functional-requirements.md#gql)) also depends on it.

### OQ-006

Spatial types and the standard ([FR-TYPE-002](functional-requirements.md#types)). The spatial behaviour is decided ([FR-TYPE-002](functional-requirements.md#types), [FR-TYPE-004](functional-requirements.md#types), [FR-TYPE-005](functional-requirements.md#types)). Whether Point2D and Point3D conflict with, or are covered by, ISO/IEC 39075:2024, and how a declared extension is expressed without new syntax ([BR-GQL-001](functional-requirements.md#gql)), remain to be verified against the norm.

## Closed

### OQ-002

Deadlock handling. Closed 2026-09-24: deadlocks are detected and the requesting transaction that closes the cycle is aborted ([FR-TX-005](functional-requirements.md#transactions), [FR-TX-007](functional-requirements.md#transactions)).

### OQ-003

Bulk import and export formats. Closed 2026-09-24 ([FR-IMP-001](functional-requirements.md#import-and-export) to [FR-IMP-005](functional-requirements.md#import-and-export)); the format-scope matrix was closed under [OQ-017](#oq-017).

### OQ-004

Graph-algorithm scope and phases. Closed 2026-09-24: the inventory is reduced to the entries that Neo4j Graph Data Science, Memgraph MAGE, petgraph or graphalgs provide, and the whole inventory ships in the first release ([FR-ALG-001](functional-requirements.md#graph-algorithms) to [FR-ALG-005](functional-requirements.md#graph-algorithms), [graph-algorithms.md](graph-algorithms.md), [NFR-REL-004](non-functional-requirements.md#release)).

### OQ-007

Clock abstraction. Closed 2026-09-24: no clock abstraction; the crate reads the operating-system clock ([FR-TYPE-006](functional-requirements.md#types)).

### OQ-008

Time-zone data source. Closed 2026-09-24: the crate reads the operating system's IANA time-zone data ([FR-TYPE-007](functional-requirements.md#types)).

### OQ-009

Procedure and function registration. Closed 2026-09-24: the interface is the abstraction [ABS-005](application-abstractions.md#abstractions) ([FR-PROC-001](functional-requirements.md#procedures-and-functions) to [FR-PROC-003](functional-requirements.md#procedures-and-functions)).

### OQ-010

Serializable isolation with non-blocking reads. Closed 2026-09-24: a serializable transaction may fail with a serialisation-failure GQLSTATUS error and the application retries ([FR-TX-006](functional-requirements.md#transactions), [FR-TX-008](functional-requirements.md#transactions), [BR-TX-001](functional-requirements.md#transactions)).

### OQ-011

Bolt `db` field. Closed 2026-09-24 ([FR-BOLT-007](functional-requirements.md#bolt)).

### OQ-012

GQL-over-Bolt mapping. Closed 2026-09-24 ([FR-BOLT-004](functional-requirements.md#bolt), [FR-BOLT-007](functional-requirements.md#bolt), [FR-BOLT-009](functional-requirements.md#bolt), [FR-BOLT-014](functional-requirements.md#bolt) to [FR-BOLT-018](functional-requirements.md#bolt), [FR-BOLT-021](functional-requirements.md#bolt)).

### OQ-013

Second-process access. Closed 2026-09-24: detected and refused ([FR-DB-006](functional-requirements.md#database-lifecycle)).

### OQ-014

Corruption detected while open. Closed 2026-09-24: the operation fails and the database becomes unusable until reopened ([FR-REC-005](functional-requirements.md#recovery)).

### OQ-015

GQL delivery phases. Closed 2026-09-24: the first release delivers minimum conformance, version 1.0 full conformance, and optional features are ordered by support in Neo4j and Memgraph ([FR-GQL-006](functional-requirements.md#gql), [NFR-REL-004](non-functional-requirements.md#release), [NFR-REL-005](non-functional-requirements.md#release)). The concrete feature list depends on [OQ-005](#oq-005).

### OQ-016

Names and result columns of the built-in procedures. Closed 2026-09-24: they follow the Neo4j `SHOW` commands ([FR-PROC-004](functional-requirements.md#procedures-and-functions)).

### OQ-017

Export formats per scope. Closed 2026-09-24 ([FR-IMP-003](functional-requirements.md#import-and-export)).
