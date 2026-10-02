---
title: Bolt
status: draft
last-reviewed: 2026-10-02
related:
  - interfaces.md
  - values-and-types.md
  - quality-and-testing.md
---

# Bolt

## TS-22 Bolt and PackStream

- **Decision**: the crate implements both Bolt endpoints and PackStream itself, behind the Cargo feature `bolt`.
  - **Transport.** Each endpoint runs over one connection the application passes in, of any type that implements `tokio::io::AsyncRead + AsyncWrite + Unpin + Send + 'static`. The crate opens no socket and does no TLS.
  - **Handshake.**
    - The crate accepts version ranges and the manifest v1 handshake (marker `00 00 01 FF`, Bolt 5.7 and later).
    - Supported versions: 5.0, 5.1, 5.2, 5.3, 5.4, 5.6, 5.7, 5.8 and 6.0. Version 5.5 is never negotiated.
    - Bolt 4.4 and earlier are refused.
    - The capabilities bitmask is sent as zero.
  - **Messages.** One state machine (states: negotiation, authentication, ready, streaming, transaction, failed, defunct) with a message table per version:
    - `LOGON` and `LOGOFF` from 5.1;
    - `TELEMETRY` from 5.4, accepted and ignored;
    - `gql_status` in `FAILURE` from 5.7, with code and message mapped from GQLSTATUS before 5.7;
    - `Vector` and `UnsupportedType` from 6.0.
  - **Sessions.** One GQL session per connection, created when authentication succeeds and ended by `LOGOFF`, `GOODBYE` or connection close.
    - `RUN` outside a transaction runs in an auto-commit transaction.
    - A GQL session or transaction statement inside `RUN` is refused.
    - `RESET` cancels the running statement and aborts the transaction ([TS-20](query-engine.md#ts-20-binder-planner-and-execution)).
    - Every Bolt transaction is serializable.
  - **`db` field.** Resolved as a catalog reference: absent, null or empty selects the home graph; a graph or a schema becomes the transaction's current graph or schema; anything else is an error.
  - **Bookmarks.** A bookmark is `<database UUID>:<commit_ts>`. A bookmark with this database's UUID and a `commit_ts` no higher than the highest published one starts the transaction at once. Any other bookmark is an error. The second check rejects bookmarks from the source of a restored copy that refer to commits after the backup point.
  - **ROUTE.** A routing table with the application's advertised address in the `ROUTE`, `READ` and `WRITE` roles, `ttl` 300.
  - **Values.**
    - Nodes and relationships carry `element_id` as the identifier's decimal text and `id` as its integer ([TS-07](storage.md#ts-07-element-identifiers)).
    - Values are never converted with loss. A value without a PackStream representation is sent as `UnsupportedType` on 6.0 and later, and fails with an error on earlier versions.
  - **Client endpoint.** Connect and authenticate, auto-commit queries, explicit transactions, incremental `PULL`, and bookmarks. It never retries.
- **Satisfies**: FR-BOLT-001 to FR-BOLT-021, BR-BOLT-001, ABS-001, FR-API-003, FR-API-008, FR-TX-004, BR-TX-001, NFR-VER-003, NFR-VER-005.
- **Evidence**:
  - Neo4j server (R-NEO `community/bolt`):
    - one protocol module per version, `protocol/v40` to `v61`;
    - the state machine `fsm/StateMachineImpl.java`;
    - PackStream `packstream/io/PackstreamBuf.java`;
    - chunk framing `packstream/codec/transport/ChunkFrameDecoder.java` and `ChunkFrameEncoder.java`.
  - Memgraph server states `handshake`, `init`, `executing`, `error` (R-MG `src/communication/bolt/v1/states/`).
  - Handshake: D-HS. Version list and the 5.5 gap: D-BC. Structures, `element_id` from 5.0, `Vector` 0x56 and `UnsupportedType` 0x3F from 6.0: D-SS.
- **Crates**:
  - `neo4rs` 0.8.0 (MIT, 1.2 M downloads): client only, and it offers Bolt 4.x only ([S37]); study only.
  - `bolt-proto` and `bolt-client` (MIT, about 49 k downloads each, last release 2022-12-28): unmaintained; study only.
  - `boltr` and `meshdb-bolt` (at most 6.3 k downloads): too little use; study only.
- **Rejected**: wrapping any of the crates above, for the reasons given.

Neo4j's source at `54a7dcf` already contains `BoltProtocolV61` (Bolt 6.1), while D-BC lists 6.0 as the latest version. FR-BOLT-003 follows the documentation, so 6.1 is not supported until the documentation lists it. The changes in 6.1 were not examined (unverified).
