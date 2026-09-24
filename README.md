# lpg

An embeddable Rust library (crate) for Labeled Property Graph (LPG) databases, queried with GQL (ISO/IEC 39075:2024).

## What the crate is

- A library that an application links into its own process. The primary interface is an in-process, asynchronous Rust API that runs GQL and returns results.
- A persistent graph database engine: on-disk storage with an in-memory cache, ACID transactions and a full, persistent GQL catalog.
- Optionally, the Bolt protocol endpoints, both server-side and client-side, layered on top of the Rust API.

The crate is not a server process. It does not open sockets, negotiate TLS or accept connections; the application does, and hands the crate a connection through an abstraction the crate defines.

## Scope

- GQL sessions, transactions and catalog semantics, exposed through the Rust API.
- Full conformance to ISO/IEC 39075:2024 as the target: minimum conformance plus all optional features, delivered in phases, with a public list of supported feature IDs.
- Bolt versions 5.0 to the latest 6.x, carrying GQL only.
- A graph-algorithm library for analytics.
- Online backup and restore, bulk import and export, and on-disk format migration.

## Non-goals

- Replication, clustering and sharding.
- Query languages other than GQL, including SQL/PGQ, Cypher, Gremlin and SPARQL.
- Full-text and vector search.
- Access to one database from more than one process.

## Application-provided abstractions

The crate defines the following abstractions (traits). The application implements them:

- **Transport**: Bolt server and client connections.
- **Authentication and authorisation**: receives an authentication scheme and credentials, and returns a principal or a refusal; authorises each catalog and data action before the crate performs it.
- **Storage**: optional; the crate provides a default filesystem implementation.
- **Metrics sink**: receives the crate's metrics.
- **Procedures and functions**: the application's procedures and functions, registered on each open database and invoked with `CALL`.

## Key commitments

- **Durability**: a commit returns only after its log records are on stable storage. Group commit is allowed.
- **Isolation**: serializable by default; snapshot isolation as an additional mode.
- **Concurrency**: many threads in one process read and write concurrently. Reads never block and are never blocked by writes. Writers wait on write locks.
- **Recovery**: from process crash, power loss and operating-system crash, including torn page writes. Corruption is detected by checksums and reported: at open, the database is not opened; while open, the database becomes unusable until it is reopened. Media failure is repaired by restoring a backup.
- **Standard GQL only**: no syntax beyond the standard. Indexes and constraints are managed through the Rust API. Extensions are procedures and functions registered through the API and invoked with `CALL`.
- **Platform**: Linux only; Rust edition 2024 with `std`; public API asynchronous on tokio; `unsafe` code forbidden.
- **Dependencies**: minimal, widely used and maintained, with licences compatible with MIT, checked by `cargo-deny`.
- **Versioning**: SemVer, with 0.x releases unstable; version 1.0 requires full GQL conformance; a versioned on-disk format with explicit migration; published on crates.io as `lpg`.
- **Performance**: targets are set from a measured baseline and are not yet defined.

## Specification

The functional specification is in [`docs/functional-spec/`](docs/functional-spec/README.md). It holds the requirements with stable identifiers, the glossary and the open questions.

## Licence

MIT. See [LICENSE](LICENSE).
