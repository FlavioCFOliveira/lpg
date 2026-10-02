---
title: Use cases
status: draft
last-reviewed: 2026-10-02
related:
  - functional-requirements.md
  - non-functional-requirements.md
  - scope.md
---

# Use cases

## Overview

Each use case describes one main flow of the crate in terms of its [actors](scope.md#actors). The requirements cited in each use case are authoritative; a use case adds no requirement.

## Use cases

### UC-DB-001 Create a database

- **Actor**: Application.
- **Trigger**: The application requests create at a location, passing the memory cap ([FR-LIM-003](functional-requirements.md#resource-limits)).
- **Preconditions**: The location holds no database.
- **Main flow**:
  1. The crate creates the database with a default schema and a default graph ([FR-DB-005](functional-requirements.md#database-lifecycle)).
  2. The crate returns a handle to the new, open database.
- **Alternate flows**:
  - A1. The location holds a database: the crate fails with an error and leaves it unchanged ([FR-DB-003](functional-requirements.md#database-lifecycle)).
- **Postconditions**: A database exists at the location and is open in the process.

### UC-DB-002 Open a database

- **Actor**: Application.
- **Trigger**: The application requests open at a location, passing the memory cap ([FR-LIM-003](functional-requirements.md#resource-limits)).
- **Preconditions**: None; the alternate flows cover a location that holds no usable database.
- **Main flow**:
  1. The crate verifies the log and the metadata ([FR-REC-008](functional-requirements.md#recovery)).
  2. The crate runs recovery if the database was not closed cleanly ([FR-DB-009](functional-requirements.md#database-lifecycle), [UC-REC-001](#uc-rec-001-recover-after-a-crash)).
  3. The crate returns a handle.
  4. The application registers its procedures and functions ([FR-PROC-003](functional-requirements.md#procedures-and-functions)).
- **Alternate flows**:
  - A1. No database at the location: open fails ([FR-DB-004](functional-requirements.md#database-lifecycle)).
  - A2. Another process has the database open: open is refused ([FR-DB-006](functional-requirements.md#database-lifecycle)).
  - A3. This process has the database open: open is refused ([FR-DB-007](functional-requirements.md#database-lifecycle)).
  - A4. Corruption is detected: open fails and reports it ([FR-REC-003](functional-requirements.md#recovery)).
  - A5. The format is older or newer than the crate's: open fails ([FR-OPS-004](functional-requirements.md#operations), [FR-OPS-013](functional-requirements.md#operations)).
- **Postconditions**: The database is open through one handle.

### UC-DB-003 Close a database

- **Actor**: Application.
- **Trigger**: The application requests close on a handle.
- **Preconditions**: The database is open.
- **Main flow**:
  1. The crate waits for active sessions and transactions to end, up to the timeout ([FR-DB-008](functional-requirements.md#database-lifecycle)).
  2. The crate closes the database.
- **Alternate flows**:
  - A1. Transactions remain when the timeout expires: the crate aborts them, then closes.
  - A2. The handle is dropped without close: the next open runs recovery ([FR-DB-009](functional-requirements.md#database-lifecycle)).
- **Postconditions**: The database is closed; every committed transaction is durable.

### UC-QRY-001 Run a query through the Rust API

- **Actor**: Application.
- **Trigger**: The application submits a GQL request on a session.
- **Preconditions**: The database is open; the application has opened a session with a principal ([FR-SES-001](functional-requirements.md#sessions)).
- **Main flow**:
  1. The crate asks the authorisation abstraction before each catalog or data action ([FR-AUTH-001](functional-requirements.md#authorisation)).
  2. The crate executes the request and streams the result; the application pulls rows incrementally ([FR-API-010](functional-requirements.md#api)).
- **Alternate flows**:
  - A1. Another request of the session is running: the new request is refused ([FR-SES-004](functional-requirements.md#sessions)).
  - A2. Authorisation is refused: the statement fails ([FR-AUTH-001](functional-requirements.md#authorisation)).
  - A3. A resource limit is exceeded: the statement fails and the transaction is aborted ([FR-LIM-001](functional-requirements.md#resource-limits)).
  - A4. The application cancels: the statement fails and the transaction is aborted ([FR-API-009](functional-requirements.md#api)).
- **Postconditions**: The application has the result or a GQLSTATUS error.

### UC-BOLT-001 Serve a Bolt session

- **Actor**: Bolt client (primary); Application (supplies the connection).
- **Trigger**: The application hands an accepted connection to the server-side endpoint through [ABS-001](application-abstractions.md#abstractions).
- **Preconditions**: The database is open; the application has set the Bolt default limits ([FR-LIM-004](functional-requirements.md#resource-limits)) and the advertised address ([FR-BOLT-009](functional-requirements.md#bolt)).
- **Main flow**:
  1. The client and the crate negotiate a Bolt version from 5.0 to the latest 6.x ([FR-BOLT-003](functional-requirements.md#bolt)).
  2. The client authenticates; the crate passes the credentials to [ABS-002](application-abstractions.md#abstractions) and creates a GQL session bound to the returned principal ([FR-BOLT-011](functional-requirements.md#bolt), [FR-BOLT-013](functional-requirements.md#bolt), [FR-BOLT-014](functional-requirements.md#bolt)).
  3. The client sends `RUN` with GQL text, or `BEGIN`, `RUN` and `COMMIT`; the crate resolves `db` ([FR-BOLT-007](functional-requirements.md#bolt)), runs the transaction and returns a bookmark on commit ([FR-BOLT-016](functional-requirements.md#bolt), [FR-BOLT-017](functional-requirements.md#bolt)).
  4. The client sends `GOODBYE`; the crate ends the session.
- **Alternate flows**:
  - A1. Authentication is refused: the crate replies `FAILURE` ([FR-BOLT-013](functional-requirements.md#bolt)).
  - A2. The client sends `RESET`: the crate stops the statement, aborts the transaction and returns the session to its post-authentication state ([FR-BOLT-015](functional-requirements.md#bolt)).
  - A3. `RUN` carries a GQL session or transaction statement: the crate refuses it ([FR-BOLT-016](functional-requirements.md#bolt)).
  - A4. A value has no PackStream representation: the crate sends `UnsupportedType` or an error ([FR-BOLT-005](functional-requirements.md#bolt)).
- **Postconditions**: The session has ended; committed transactions are durable.

### UC-BOLT-002 Use a remote Bolt server

- **Actor**: Application; Bolt server (remote).
- **Trigger**: The application uses the client-side endpoint on an outgoing connection supplied through [ABS-001](application-abstractions.md#abstractions).
- **Preconditions**: The server is the crate's server-side endpoint or a Neo4j server that supports Bolt 5.0 or later ([FR-BOLT-020](functional-requirements.md#bolt)).
- **Main flow**:
  1. The client endpoint negotiates the version and authenticates.
  2. The application runs an auto-commit query or an explicit transaction and pulls the result incrementally ([FR-BOLT-019](functional-requirements.md#bolt)).
  3. The application passes the returned bookmark to a later transaction.
- **Alternate flows**:
  - A1. A transaction fails: the client reports the failure and does not retry ([BR-TX-001](functional-requirements.md#transactions)).
- **Postconditions**: The application has the results or the errors.

### UC-TX-001 Run concurrent transactions with a conflict or a deadlock

- **Actor**: Application.
- **Trigger**: Two sessions run transactions that touch the same data.
- **Preconditions**: The database is open.
- **Main flow**:
  1. Each transaction reads without waiting ([FR-TX-006](functional-requirements.md#transactions)).
  2. A transaction that needs a write lock held by the other waits ([FR-TX-005](functional-requirements.md#transactions)).
  3. Both commit when their combined effect matches a serial order.
- **Alternate flows**:
  - A1. The combined effect does not match a serial order: one transaction fails with a serialisation-failure GQLSTATUS error ([FR-TX-008](functional-requirements.md#transactions)); the application may retry it ([BR-TX-001](functional-requirements.md#transactions)).
  - A2. A lock request closes a wait cycle: the requesting transaction is aborted with a GQLSTATUS error ([FR-TX-007](functional-requirements.md#transactions)); the application may retry it.
- **Postconditions**: Each transaction has committed or failed; the database is consistent.

### UC-OPS-001 Back up a database

- **Actor**: Application.
- **Trigger**: The application requests a backup through the Rust API ([FR-OPS-010](functional-requirements.md#operations)).
- **Preconditions**: The database is open.
- **Main flow**:
  1. The crate writes a full, consistent backup of the whole database while transactions continue ([FR-OPS-001](functional-requirements.md#operations), [FR-OPS-007](functional-requirements.md#operations), [FR-OPS-008](functional-requirements.md#operations)).
- **Alternate flows**:
  - A1. The storage is full: the backup fails with an error ([FR-REC-007](functional-requirements.md#recovery)).
  - A2. A fast initial load is running: the backup fails with an error ([FR-IMP-002](functional-requirements.md#import-and-export)).
- **Postconditions**: A backup exists that represents one commit point.

### UC-OPS-002 Restore a database

- **Actor**: Application.
- **Trigger**: The application requests a restore through the Rust API.
- **Preconditions**: A backup exists; the target location holds no database.
- **Main flow**:
  1. The crate creates the database at the target location from the backup ([FR-OPS-009](functional-requirements.md#operations)), preserving element identifiers ([BR-TYPE-002](functional-requirements.md#types)).
  2. The application opens it ([UC-DB-002](#uc-db-002-open-a-database)).
- **Alternate flows**:
  - A1. The target holds a database: the restore fails and leaves it unchanged.
- **Postconditions**: The restored database contains the backed-up state.

### UC-OPS-003 Import data

- **Actor**: Application.
- **Trigger**: The application requests an import of files in a supported format ([FR-IMP-001](functional-requirements.md#import-and-export)).
- **Preconditions**: The database is open; the target graph exists.
- **Main flow**:
  1. The application chooses the transactional import or, for an empty graph, the fast initial load ([FR-IMP-002](functional-requirements.md#import-and-export)).
  2. The crate imports the records and assigns new element identifiers ([FR-IMP-005](functional-requirements.md#import-and-export)).
  3. The crate returns a report.
- **Alternate flows**:
  - A1. A record is invalid: the crate aborts (default) or skips it, and reports it ([FR-IMP-004](functional-requirements.md#import-and-export)).
  - A2. The fast initial load targets a graph that is not empty: it fails.
  - A3. A transactional import needs more memory than the cap: it fails and the graph is unchanged ([FR-IMP-002](functional-requirements.md#import-and-export), [FR-LIM-007](functional-requirements.md#resource-limits)).
  - A4. A backup is running when the fast initial load is requested: the load waits until the backup ends ([FR-OPS-001](functional-requirements.md#operations)).
- **Postconditions**: The graph contains the imported data, or is unchanged after an abort.

### UC-OPS-004 Export data

- **Actor**: Application.
- **Trigger**: The application requests an export of a database, a graph or a query result ([FR-IMP-003](functional-requirements.md#import-and-export)).
- **Preconditions**: The database is open.
- **Main flow**:
  1. The crate writes the data of the chosen scope in the chosen format.
- **Alternate flows**:
  - A1. The storage is full: the export fails with an error.
- **Postconditions**: The export files contain exactly the data of the scope.

### UC-IDX-001 Create an index or a constraint

- **Actor**: Application.
- **Trigger**: The application requests an index or a constraint on a graph through the Rust API ([FR-API-004](functional-requirements.md#api)).
- **Preconditions**: The database is open; the graph exists.
- **Main flow**:
  1. The crate creates the index or constraint on that graph ([FR-IDX-001](functional-requirements.md#indexes-and-constraints) to [FR-IDX-003](functional-requirements.md#indexes-and-constraints)).
  2. For an index, the crate builds it while writes continue ([FR-IDX-005](functional-requirements.md#indexes-and-constraints)). For a constraint, writers on its label or edge type wait until the existing data is validated ([FR-IDX-006](functional-requirements.md#indexes-and-constraints)).
- **Alternate flows**:
  - A1. Existing data violates the constraint: creation fails and no constraint exists ([FR-IDX-004](functional-requirements.md#indexes-and-constraints)).
- **Postconditions**: The index or constraint exists and applies to later writes.

### UC-PROC-001 Register and call a procedure

- **Actor**: Application.
- **Trigger**: The application registers a procedure on a handle, then a request calls it.
- **Preconditions**: The database is open.
- **Main flow**:
  1. The application registers the procedure through [ABS-005](application-abstractions.md#abstractions), declared read-only or read-write ([FR-PROC-002](functional-requirements.md#procedures-and-functions)).
  2. A GQL request invokes it with `CALL`; the crate calls the implementation in the caller's transaction ([FR-API-006](functional-requirements.md#api)).
- **Alternate flows**:
  - A1. A read-only procedure attempts to modify data: the statement fails.
  - A2. The database has been reopened and the procedure not registered again: the `CALL` fails ([FR-PROC-003](functional-requirements.md#procedures-and-functions)).
- **Postconditions**: The result is returned; a read-write procedure's changes commit or roll back with the caller's transaction.

### UC-ALG-001 Run a graph algorithm

- **Actor**: Application.
- **Trigger**: The application runs an algorithm with `CALL` or through the Rust API ([FR-ALG-002](functional-requirements.md#graph-algorithms)).
- **Preconditions**: The database is open; a transaction is active.
- **Main flow**:
  1. The crate reads the graph, filtered by labels and edge types, from the transaction's snapshot ([FR-ALG-004](functional-requirements.md#graph-algorithms)).
  2. The crate streams the results to the caller ([FR-ALG-003](functional-requirements.md#graph-algorithms)).
- **Alternate flows**:
  - A1. The caller chose write-back: the crate writes the results to properties in the caller's transaction.
  - A2. A resource limit is exceeded: the statement fails and the transaction is aborted ([FR-LIM-001](functional-requirements.md#resource-limits)).
- **Postconditions**: The caller has the results, or the properties are written when the transaction commits.

### UC-OPS-005 Migrate the on-disk format

- **Actor**: Application.
- **Trigger**: Open fails because the format is the preceding version ([FR-OPS-004](functional-requirements.md#operations)).
- **Preconditions**: The database was closed cleanly; a backup taken before migration exists.
- **Main flow**:
  1. The application runs the migration through the Rust API.
  2. The crate checks that the storage has free space at least equal to the database's size, writes a copy in the current format inside the same location and switches to it, preserving element identifiers ([FR-OPS-011](functional-requirements.md#operations), [BR-TYPE-002](functional-requirements.md#types)).
  3. The application opens the database.
- **Alternate flows**:
  - A1. The format is older than the preceding version: migration fails and names the intermediate crate version required ([FR-OPS-012](functional-requirements.md#operations)).
  - A2. The migration fails or must be undone: the application restores the pre-migration backup ([FR-OPS-013](functional-requirements.md#operations)).
  - A3. The free space is less than the database's size: migration fails before writing and the database is unchanged ([FR-OPS-011](functional-requirements.md#operations)).
  - A4. The database is open or was not closed cleanly: migration fails with an error ([FR-OPS-011](functional-requirements.md#operations)).
- **Postconditions**: The database is in the current format.

### UC-REC-001 Recover after a crash

- **Actor**: Application.
- **Trigger**: The application opens a database after a process crash, a power loss, an operating-system crash or a dropped handle.
- **Preconditions**: The database was not closed cleanly.
- **Main flow**:
  1. The crate verifies the log and the metadata ([FR-REC-008](functional-requirements.md#recovery)).
  2. The crate recovers every committed transaction and discards every uncommitted effect ([FR-REC-001](functional-requirements.md#recovery), [FR-REC-002](functional-requirements.md#recovery)).
  3. The crate returns a handle.
- **Alternate flows**:
  - A1. Corruption is detected: the open fails and reports it; the application restores a backup ([FR-REC-003](functional-requirements.md#recovery), [FR-REC-004](functional-requirements.md#recovery)).
- **Postconditions**: The database is open and holds exactly the committed transactions.
