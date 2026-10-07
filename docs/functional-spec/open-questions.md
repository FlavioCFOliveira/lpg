---
title: Open questions
status: draft
last-reviewed: 2026-10-07
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

Performance targets ([NFR-PERF-001](non-functional-requirements.md#performance)). The workloads and the comparison are decided ([NFR-PERF-002](non-functional-requirements.md#performance)). The metrics and the numeric targets are open, to be set from the measured baseline. No absolute graph size is set; efficiency is judged on the reference hardware with the cache holding 1% to 10% of the data ([NFR-PERF-005](non-functional-requirements.md#performance), [NFR-PERF-006](non-functional-requirements.md#performance)).

### OQ-005

GQL implementation-defined choices, rules and corrigendum ([FR-GQL-001](functional-requirements.md#gql)). The ISO/IEC 39075:2024 text is not available to the project (user decision 2026-10-02). The authority for GQL syntax is opengql/grammar [S4] ([NFR-VER-002](non-functional-requirements.md#verification)); against it are verified the shape of a procedure call and the identifier characters ([FR-PROC-004](functional-requirements.md#procedures-and-functions)), dotted procedure references ([FR-ALG-007](functional-requirements.md#graph-algorithms)) and the absence of a directory-creation statement ([FR-CAT-005](functional-requirements.md#catalog)). The ISO electronic inserts [S45] are a supplementary source; against them are verified the feature identifiers GG01, GG02 ([FR-GQL-004](functional-requirements.md#gql)), GV39, GV40, GV41 ([FR-TYPE-001](functional-requirements.md#types)) and GP18 ([FR-CAT-004](functional-requirements.md#catalog)), the list of optional features ([FR-GQL-001](functional-requirements.md#gql), [FR-GQL-006](functional-requirements.md#gql)), the syntax-error code 42001 ([BR-GQL-001](functional-requirements.md#gql)), the absence of a serialisation-failure condition ([FR-TX-008](functional-requirements.md#transactions)) and the list of 117 implementation-defined items ([BR-GQL-002](functional-requirements.md#gql)). opengql/grammar, opengql/tck and the inserts are the provisional authority; every requirement that depends on the standard's Syntax Rules, General Rules or Conformance Rules is marked "provisional (OQ-005)", and the text may be obtained later. Open until then: whether the implementation-defined subclass 25N11 chosen for the serialisation failure is permitted ([FR-TX-008](functional-requirements.md#transactions)); how the standard resolves the built-in and algorithm procedure names ([FR-PROC-004](functional-requirements.md#procedures-and-functions), [FR-ALG-007](functional-requirements.md#graph-algorithms)); the 96 implementation-defined items not yet decided, which are decided one by one with the user's explicit approval ([BR-GQL-002](functional-requirements.md#gql)); whether the 21 items decided on 2026-10-07 and recorded in BR-GQL-002 meet the context and any minimum that the Syntax Rules and General Rules set; and the content of minimum conformance ([FR-GQL-001](functional-requirements.md#gql)). The effect of ISO/IEC 39075:2024/Cor 1:2026 is also unverified ([FR-GQL-003](functional-requirements.md#gql)).

### OQ-006

Spatial types and the standard ([FR-TYPE-002](functional-requirements.md#types)). The spatial behaviour is decided ([FR-TYPE-002](functional-requirements.md#types), [FR-TYPE-004](functional-requirements.md#types), [FR-TYPE-005](functional-requirements.md#types)). Whether Point2D and Point3D conflict with, or are covered by, ISO/IEC 39075:2024, and how a declared extension is expressed without new syntax ([BR-GQL-001](functional-requirements.md#gql)), remain to be verified against the norm. The ISO features insert lists no spatial feature [S45].

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

Serializable isolation with non-blocking reads. Closed 2026-09-24: a serializable transaction may fail with a serialisation-failure GQLSTATUS error and the application decides whether to retry ([FR-TX-006](functional-requirements.md#transactions), [FR-TX-008](functional-requirements.md#transactions), [BR-TX-001](functional-requirements.md#transactions)). On 2026-09-24 the user also decided that serializable isolation is enforced by validating the read sets of read-write transactions at commit ([FR-TX-009](functional-requirements.md#transactions)); read-only transactions are not validated ([FR-TX-010](functional-requirements.md#transactions)).

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

### OQ-018

Algorithm procedure names. Closed 2026-10-02: GDS names in the GDS namespace where GDS provides the entry, MAGE names where only MAGE provides it ([FR-ALG-007](functional-requirements.md#graph-algorithms)); the remaining entries moved to [OQ-020](#oq-020).

### OQ-019

Fast initial load and the rest of the database. Closed 2026-10-02: writers to the graph wait, existing indexes and constraints are built after the load, and a backup requested during the load fails ([FR-IMP-002](functional-requirements.md#import-and-export), [FR-OPS-001](functional-requirements.md#operations)).

### OQ-020

Names of the algorithm procedures that neither GDS nor MAGE provides. Closed 2026-10-02: the `gds.` namespace and a camelCase name derived from the source function or algorithm name ([FR-ALG-007](functional-requirements.md#graph-algorithms), [Derived procedure names](graph-algorithms.md#derived-procedure-names)).
