# lpg

A Rust library (crate) that provides embeddable Labeled Property Graph (LPG) functionality for Rust applications.

## Scope

This crate is a library, not a graph application or server. It provides the functionality an application or server needs to operate on LPG graphs.

The crate is responsible for the graphs themselves. Other functionality defined by the GQL standard, such as sessions, transactions, and catalog management (schemas and graphs), is exposed through an API so that consumers can integrate it according to their own needs.

For application-level concerns, the crate only defines the abstractions (traits) it requires. The crate calls these abstractions to provide the functionality, but does not implement them. The application implements them according to its own criteria and requirements, while honouring the contracts defined by the crate.

Examples of application-level concerns:

- network transport (sockets, TLS, accepting and managing client connections);
- authentication and authorisation;
- the server process and its operational management.

## Requirements

1. **MVCC compliant** — Designed for high-load workloads, with no contention or blocking between reads and writes.
2. **ACID compliant** — Full ACID support, with guaranteed durability.
3. **GQL compliant** — Full, unrestricted support for the `ISO/IEC 39075:2024 GQL (Graph Query Language)` standard.
4. **Bolt compliant** — Implements the Bolt protocol (PackStream encoding, messages, and the protocol state machine) over a transport abstraction provided by the application. GQL is the only query language accepted over Bolt; Cypher/openCypher is not supported.
