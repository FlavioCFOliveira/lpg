---
title: Scope
status: draft
last-reviewed: 2026-09-24
related:
  - functional-requirements.md
  - application-abstractions.md
  - use-cases.md
  - graph-algorithms.md
  - open-questions.md
---

# Scope

## Overview

`lpg` is a Rust library that an application embeds in its own process to store and query Labeled Property Graph databases with GQL. Its primary interface is an in-process, asynchronous Rust API. Bolt protocol endpoints are an optional layer on top of that API.

## In scope

- Executing GQL and returning results through the Rust API ([FR-API-001](functional-requirements.md#api)).
- GQL sessions, transactions and the full, persistent catalog ([FR-API-002](functional-requirements.md#api), [FR-CAT-001](functional-requirements.md#catalog)).
- Databases: each database is one full GQL catalog; one process may open several databases, each through its own handle ([Database lifecycle](functional-requirements.md#database-lifecycle)).
- Conformance to ISO/IEC 39075:2024, delivered in phases ([FR-GQL-001](functional-requirements.md#gql)).
- Persistent storage, ACID transactions and recovery ([Storage](functional-requirements.md#storage), [Transactions](functional-requirements.md#transactions), [Recovery](functional-requirements.md#recovery)).
- Bolt server-side and client-side endpoints, versions 5.0 to the latest 6.x ([Bolt](functional-requirements.md#bolt)).
- A graph-algorithm library for analytics, closed by the [inventory](graph-algorithms.md) ([FR-ALG-001](functional-requirements.md#graph-algorithms)).
- Backup, restore, import, export and format migration ([Operations](functional-requirements.md#operations), [Import and export](functional-requirements.md#import-and-export)).

## Non-goals

- NG-1: Replication, clustering and sharding.
- NG-2: Query languages other than GQL, including SQL/PGQ, Cypher, Gremlin and SPARQL.
- NG-3: Full-text search and vector search.
- NG-4: Access to one database from more than one process. The crate detects and refuses it ([FR-DB-006](functional-requirements.md#database-lifecycle)).
- NG-5: A server process. The crate opens no sockets, negotiates no TLS and accepts no connections; the application does, through the transport abstraction ([ABS-001](application-abstractions.md#abstractions)).

## Decided exclusions

The user excluded these capabilities on 2026-09-24 under [NFR-SIMP-001](non-functional-requirements.md#simplicity). Each needs a change to this specification to be added:

- Differential or incremental backups, and point-in-time restore ([FR-OPS-007](functional-requirements.md#operations)).
- Restore over an existing database ([FR-OPS-009](functional-requirements.md#operations)).
- Memgraph as a target of the client-side Bolt endpoint ([FR-BOLT-020](functional-requirements.md#bolt)).
- An open option that verifies the whole database ([FR-REC-009](functional-requirements.md#recovery), withdrawn).
- Parameter values in query logging ([FR-OBS-002](functional-requirements.md#observability)).
- Graph algorithms that only NetworkX provides, graph learning and embeddings, and dynamic algorithms ([Withdrawn entries](graph-algorithms.md#withdrawn-entries)).

## Actors

The main flows between these actors and the crate are in [use-cases.md](use-cases.md).

- **Application**: the Rust program that embeds the crate, calls the Rust API and implements the [application-provided abstractions](application-abstractions.md).
- **Bolt client**: a driver that connects to the crate's server-side Bolt endpoint through a connection the application supplies.
- **Bolt server**: a remote server that the crate's client-side Bolt endpoint connects to through a connection the application supplies.
- **Principal**: the identity under which a session runs: returned by the authentication and authorisation abstraction for a Bolt session, or supplied by the application for a Rust API session ([ABS-002](application-abstractions.md#abstractions), [FR-SES-001](functional-requirements.md#sessions)).
