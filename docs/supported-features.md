---
title: Supported GQL features
status: generated
related:
  - functional-spec/functional-requirements.md
  - technical-spec/query-engine.md
---

# Supported GQL features

<!-- Generated from src/query/features/ by the test
     `query::features::tests::published_feature_list_matches_the_gate_table`.
     Do not edit by hand: rerun it with LPG_UPDATE_FEATURE_LIST=1. -->

The crate supports the mandatory surface of GQL (ISO/IEC 39075:2024) and the optional features below, by their identifiers in the ISO features insert. The mandatory surface is every construct of the opengql/grammar `GQL.g4` that no feature identifier covers; this placement rule and the list are provisional ([FR-GQL-002](functional-spec/functional-requirements.md#gql), OQ-005).

| Feature | Description |
|---|---|
| GC01 | Graph schema management |
| GC04 | Graph management |
| GD01 | Updatable graphs |
| GG01 | Graph with an open graph type |
| GG02 | Graph with a closed graph type |
| GG03 | Graph type inline specification |
| GP04 | Named procedure calls |
| GP18 | Catalog and data statement mixing |
| GQ01 | USE graph clause |
| GT01 | Explicit transaction commands |
| GT03 | Use of multiple graphs in a transaction |
| GV39 | Temporal types: date, local datetime and local time support |
| GV40 | Temporal types: zoned datetime and zoned time support |
| GV41 | Temporal types: duration support |

The other 214 optional features of the insert are not supported. A request that uses one fails with GQLSTATUS 42001, whose cause 51N26 names the feature identifier.
