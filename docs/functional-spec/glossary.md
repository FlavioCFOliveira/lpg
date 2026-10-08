---
title: Glossary
status: draft
last-reviewed: 2026-10-08
related:
  - README.md
---

# Glossary

- **Acceptance criterion**: a Given/When/Then statement that a test can check to show that a requirement is met.
- **Advertised address**: the Bolt address that the crate returns in a routing table; the application supplies it ([FR-BOLT-009](functional-requirements.md#bolt)).
- **Application**: the Rust program that embeds the crate and implements the application-provided abstractions.
- **Application-provided abstraction**: a Rust trait that the crate defines and calls and that the application implements. The closed list is in [application-abstractions.md](application-abstractions.md).
- **Auto-commit transaction**: over Bolt, the transaction in which a `RUN` sent outside an explicit transaction runs; it commits when the statement completes.
- **Bolt**: the client-server protocol used by Neo4j drivers, with PackStream as its encoding.
- **Bolt endpoint**: the crate's implementation of one side of the Bolt protocol (server-side or client-side) over a connection supplied by the application. It is not a server process.
- **Bookmark**: a Bolt value that the crate returns when a transaction commits, and that a later transaction can carry to require that it sees that commit ([FR-BOLT-017](functional-requirements.md#bolt)).
- **Catalog**: the GQL catalog: the directory hierarchy, schemas, graph types and graphs of a database. "Catalog" is the GQL term and keeps its spelling.
- **Coordinate reference system (CRS)**: the system in which a point's coordinates are expressed, identified by an SRID ([FR-TYPE-002](functional-requirements.md#types)).
- **Database**: one full GQL catalog with its data, stored at one location ([FR-DB-001](functional-requirements.md#database-lifecycle)).
- **Database handle**: the Rust value through which the application uses one open database ([FR-DB-002](functional-requirements.md#database-lifecycle)).
- **Deadlock**: a cycle of transactions in which each waits for a lock that the next holds.
- **Default schema, default graph**: the schema `public` and the graph `lpg` in it, at catalog path `/public/lpg`, that a new database's catalog contains, used as a session's home schema and home graph when none is supplied ([FR-DB-005](functional-requirements.md#database-lifecycle)).
- **Element identifier**: the identifier of a node or edge. It is stable for the element's lifetime and never reused.
- **Fast initial load**: the import mode that loads data into an empty graph ([FR-IMP-002](functional-requirements.md#import-and-export)).
- **Feature identifier**: a code, such as GG01, that ISO/IEC 39075 assigns to an optional GQL feature; the ISO features insert lists them ([S45](README.md#sources)).
- **Format version**: the version of the crate's on-disk format ([FR-OPS-004](functional-requirements.md#operations)).
- **GQL**: the Graph Query Language, ISO/IEC 39075.
- **GQLSTATUS**: the GQL status code that reports the outcome of a GQL request.
- **Group commit**: flushing the log records of several committing transactions to stable storage with one flush.
- **Home schema, home graph**: the schema and graph a session uses when a request names none ([FR-SES-002](functional-requirements.md#sessions)).
- **In-memory projection**: the copy of an algorithm's input graph that the crate holds in memory for one algorithm run ([FR-ALG-004](functional-requirements.md#graph-algorithms)).
- **Labeled Property Graph (LPG)**: a graph of nodes and edges that carry labels and properties.
- **Memory cap**: the database-wide limit on the memory used by the cache, query execution and the old versions kept for MVCC together; the application passes it to create and open, with no default ([FR-LIM-003](functional-requirements.md#resource-limits)).
- **Multiversion concurrency control (MVCC)**: keeping several committed versions of the data so that a transaction reads the versions of its snapshot without waiting for writers ([NFR-PERF-003](non-functional-requirements.md#performance), [FR-TX-006](functional-requirements.md#transactions)).
- **Old version**: a committed version of data that a later commit has replaced, kept while a transaction's snapshot may still read it ([FR-LIM-005](functional-requirements.md#resource-limits)).
- **Native dump**: the crate's own import and export format ([FR-IMP-001](functional-requirements.md#import-and-export)).
- **Owner**: the principal recorded on a catalog object, passed to the authorisation abstraction ([FR-AUTH-002](functional-requirements.md#authorisation)). The owner of the default schema and the default graph is the principal passed to the create operation ([FR-DB-005](functional-requirements.md#database-lifecycle)).
- **PackStream**: the binary encoding used by Bolt.
- **Principal**: the identity under which a session runs, returned by the authentication and authorisation abstraction (Bolt) or supplied by the application (Rust API). The owner principal passed to the create operation exists without a session ([FR-DB-005](functional-requirements.md#database-lifecycle), [FR-AUTH-001](functional-requirements.md#authorisation), [FR-AUTH-002](functional-requirements.md#authorisation)).
- **Read set**: the data that a transaction has read, validated when a read-write transaction commits ([FR-TX-009](functional-requirements.md#transactions)).
- **Read-only procedure, read-write procedure**: a registered procedure declared unable, or able, to modify data ([FR-PROC-002](functional-requirements.md#procedures-and-functions)).
- **Read-only transaction, read-write transaction**: a transaction that has written no data, or has written data, when it commits ([FR-TX-009](functional-requirements.md#transactions), [FR-TX-010](functional-requirements.md#transactions)).
- **Reference hardware**: the machine on which the crate's efficiency is judged ([NFR-PERF-006](non-functional-requirements.md#performance)).
- **Serialisation failure**: the failure of a serializable transaction whose effect, combined with concurrent transactions, would not match any serial order ([FR-TX-008](functional-requirements.md#transactions)). "Serializable" keeps its standard spelling as the name of the isolation level.
- **Session**: a GQL session: the context in which a principal runs requests. A Bolt connection carries one session ([FR-BOLT-014](functional-requirements.md#bolt)).
- **Snapshot isolation**: an isolation mode in which a transaction reads the committed state at its start; selected per transaction through the Rust API ([FR-TX-004](functional-requirements.md#transactions)).
- **SRID**: spatial reference identifier, the number that identifies a coordinate reference system.
- **Stable storage**: storage whose content survives process crash, power loss and operating-system crash.
- **Storage abstraction**: the application-provided abstraction through which the crate performs storage I/O; the crate provides a default filesystem implementation.
- **Torn page write**: a page write that a power loss or crash leaves only partly on storage.
- **Unusable database**: the state of a database after a checksum failure or a flush failure while it is open: every operation on its handle fails until the database is closed and reopened ([FR-REC-005](functional-requirements.md#recovery), [FR-REC-006](functional-requirements.md#recovery)).
