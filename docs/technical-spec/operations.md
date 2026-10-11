---
title: Operations
status: draft
last-reviewed: 2026-10-11
related:
  - storage.md
  - transactions-and-recovery.md
  - values-and-types.md
---

# Operations

## TS-26 Backup, restore, import and export

- **Decision**:
  - **Backup** is physical and full, and runs through the Rust API only.
    - If a fast initial load is running, the backup fails with an error (FR-OPS-001).
    - Steps:
      1. Force a checkpoint and take `start` = its redo point. Hold WAL recycling at `start`.
      2. Copy `CONTROL` and every page of `data-<g>` through the storage trait.
      3. Flush the WAL, take `stop` and the length of `data-<g>`, and copy the WAL from `start` to `stop`.
      4. Release WAL recycling.
      5. Write the manifest, as below.
    - Every page written during step 2 was either changed after `start` through a logged commit, so its full image is in the copied WAL, or written by a bulk build, outside the WAL, into a page its build record owns ([TS-05](storage.md#ts-05-files-and-pages) Build pages).
      - A fast initial load cannot run during a backup: a load requested while a backup runs waits until the backup ends.
      - Any other build (an index, or a constraint's owned index) runs during a backup, but no commit that installs its tree or releases its pages falls between `start` and `stop` (Exclusion, below). The restored database therefore holds either the installed tree, whose pages were synced before `start`, or the build record, whose pages its open releases; an index left POPULATING is built again ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints) step 7).
    - **Exclusion.** Two latches, each taken before the commit latch:
      - **Fast initial load** (`Core::backup_or_load`, `src/storage/engine.rs:208-213`). A backup and the bulk build of a fast initial load (fast-load steps 3, 5 and 6, below) exclude each other. The backup tries the latch without waiting (`Core::try_backup_or_load`, `engine.rs:269-278`): if a build holds it, the backup fails with `Error::LoadRunning` before it writes anything to the target (`src/ops/backup.rs:258-261`, `src/storage/error.rs:56`); otherwise it holds the latch from step 1 to its end. The build waits for the latch while a backup holds it (`Core::backup_or_load`, `engine.rs:250-267`), takes it before its first run of pages ([TS-05](storage.md#ts-05-files-and-pages) Build pages), and holds it until its tree is installed or dropped (`src/storage/btree/bulk.rs:72-78, 684-688`). Other builds do not take it (`Engine::bulk_build_pages_as`, `bulk.rs:690-705`).
      - **Install and release steps** (`Core::backup_or_install`, a read-write latch, `engine.rs:218-225`). A backup holds it exclusively from before step 1 to its end (`backup.rs:262-263`). Each install commit of a build (`bulk.rs:643-653`; for a constraint's owned index, the creating transaction's commit, `src/tx/commit.rs:352-354`) and each release step, from its page writes to its commit (`bulk.rs:745-747`), holds it shared (`Core::install_step`, `engine.rs:280-296`). A build's scan, sort, runs and page writes take neither latch. A backup therefore waits only for the steps in progress, and a step waits for a running backup; a constraint's commit that arrives during a backup waits for its end, a cost in liveness only.
      - **Bounded wait.** `std`'s futex-based read-write lock admits no new reader while a writer waits (`is_read_lockable`, Rust 1.99.0 `library/std/src/sys/sync/rwlock/futex.rs:47-54`), so a backup waits for at most one step of each running build. That implementation serves Linux, Windows other than `win7`, and the other targets listed in `library/std/src/sys/sync/rwlock/mod.rs:2-13`. For other targets, macOS among them, the bound is not confirmed.
    - **Target.** A location the caller provides. It must exist, because the storage trait cannot create a directory ([TS-03](interfaces.md#ts-03-storage-abstraction)), and it must be empty: if `list` returns any entry, the backup fails before writing. Files of an earlier backup can therefore never mix with this one.
    - **Artefact.** Flat files directly in the target, written through the storage trait, as in the database location ([TS-05](storage.md#ts-05-files-and-pages)):
      - `CONTROL`, the copy from step 2;
      - `data-<g>`, the copy of `data-<g>`;
      - `wal-<g>-<lsn>`, the WAL bytes from `start` to `stop`: one file per database segment that holds an LSN from `start` to `stop`, both included, named as that segment ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)). The file of the segment that holds `stop` may be empty; restore still needs it, because open requires the segment that holds the WAL end ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 4);
      - `MANIFEST`: format version, database UUID, generation `<g>`, page size, `start`, `stop`, the length of `data-<g>` at `stop`, the length and CRC-32C of every other file, and a CRC-32C of the manifest itself.

      The artefact holds no `LOCK`, so an open of the target fails ([TS-27](storage.md#ts-27-single-process-lock)); only restore reads it.
    - **Completion marker.** `MANIFEST` is written last. After every other file has had `sync_data` and the target `sync_dir`, it is written as `MANIFEST.tmp`, synced, renamed to `MANIFEST`, and the target synced again. An artefact without a valid `MANIFEST` is incomplete.
  - **Restore** reads the artefact's flat files and writes into a location that holds no database. Steps:
    1. If the target holds `CONTROL`, fail (FR-OPS-009).
    2. Read the artefact's `MANIFEST`. If it is absent or its CRC-32C fails, fail: the backup is incomplete. Check its format version as open does ([TS-28](#ts-28-format-versioning-and-migration)).
    3. Check the length and CRC-32C of the artefact's `CONTROL`, `data-<g>` and every `wal-<g>-<lsn>` against `MANIFEST`. On a mismatch, fail (FR-REC-003).
    4. Create and lock `LOCK` in the target. List the target again; if it holds `CONTROL`, fail with "database exists": a create may have finished since step 1 (`src/ops/restore.rs:208-219`). Write `data-<g>` from the artefact's `data-<g>`, reserved with `fallocate` to the length recorded at `stop`, and one segment `wal-<g>-<lsn>` per artefact file of the same name ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)), each 16 MiB, zero-filled, with the copied bytes at their offsets. Write `BACKUP_END`: `stop` and the CRC-32C of those 8 bytes, little-endian (`encode_backup_end`, `src/storage/engine.rs:76-86`). `sync_data` each file, then `sync_dir`.
    5. Write `CONTROL` last, atomically ([TS-05](storage.md#ts-05-files-and-pages)): one valid slot holding the artefact's authoritative slot with its redo point set to `start` and the clean-shutdown flag cleared; the other slot zeroed.
    6. Open the database. Recovery ([TS-13](transactions-and-recovery.md#ts-13-recovery)) replays the WAL from `start` to its end, which is `stop`.

    **Backup end.** While `BACKUP_END` exists, every open of the location reads it before recovery and fails as corruption if the file fails its CRC-32C or if the recovered WAL ends before `stop`. That open writes no `CONTROL` slot. After the open checkpoint has recorded a redo point at or past `stop` ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 8), open removes `BACKUP_END` and calls `sync_dir` (`src/storage/engine.rs:571-580, 606-611, 636-643`). A later open of a restore whose WAL is incomplete therefore fails too, not only the open of step 6.

    The result is the state of one commit point, with element identifiers unchanged. A restore interrupted before step 5 completes leaves no `CONTROL`, so the location holds no database; a new restore, after step 4 takes `LOCK`, removes `wal-temp` and every `data-<h>` and `wal-<h>-<lsn>` the artefact does not contain, of any generation, then overwrites the files it writes (`src/ops/restore.rs:220-226`).
  - **Export layouts** (database, graph or query result, as FR-IMP-003 assigns):
    - **CSV**: separate node and edge files with the Neo4j header fields `:ID`, `:LABEL`, `:START_ID`, `:END_ID`, `:TYPE`.
      - A graph export writes one node file and one edge file. The property columns of each are the union of the property names of its elements; an absent property is an empty field.
      - Fields are quoted as RFC 4180 requires.
      - Inside an array or `:LABEL` field, elements are separated by `;`, and `\` escapes `;` and `\`.
      - Column types are Neo4j's lower-case names `long`, `double`, `boolean`, `string`, `date`, `localtime`, `time`, `localdatetime`, `datetime`, `point` and `<type>[]`; import reads them case-insensitively (R-NEO `community/import-util/.../csv/DataFactories.java:593`; `community/csv/.../reader/Extractors.java:173-204`). Import also reads `int`, `short` and `byte` as `long`, and `float` as `double`: LDBC's Neo4j headers use `INT` (R-LDBCI `cypher/scripts/headers.txt`).
      - A `double` column writes NaN and ±infinity as `NaN`, `Infinity` and `-Infinity`. Neo4j parses a `double` with `Double.parseDouble` (`Extractors.java:611-627`); measured: Neo4j 2026.09.0 imported them and returned `NaN` and `Infinity` (M-S4 Appendix A).
      - A `point` column writes each value in the [TS-21](values-and-types.md#ts-21-values-and-types) form, with its `srid`. Neo4j merges the header's and the value's maps, the value winning, and accepts `srid` (R-NEO `community/values/.../storable/PointValue.java:311-321, 474-478`); `crs` and `srid` together fail (`PointValue.java:336`). Measured: header `point{srid:7203}` with a value holding `srid` imported with SRID 7203 (M-S4 Appendix A).
      - A header field `<name>:lpg:<type name>` or `<name>:lpg:annotated` takes everything after `lpg:` as a type name of [TS-21](values-and-types.md#ts-21-values-and-types) or `annotated`. Any other field follows APOC's field grammar, `name`, `:type`, `{options}`, `(idspace)`, `[]` (R-APOC `core/src/main/java/apoc/export/csv/CsvLoaderConstants.java:28-29`, ported).
      - A property name that holds `:` fails the export with an error naming it: the field grammar's name is `[^:]*` (`CsvLoaderConstants.java:29`).
      - **Neo4j input** (user decision 2026-10-11, item A option (a), rmp #55 DECISION 399). Import also accepts id spaces (`:ID(<space>)`, `:START_ID(<space>)`, `:END_ID(<space>)`), `:IGNORE` columns, a one-byte delimiter (default `,`) and array delimiter (default `;`) that differ, and labels for a node file or an edge type for an edge file given with the file. LDBC's Neo4j load files therefore import unchanged (R-LDBCI `cypher/scripts/import-to-neo4j.sh:32-51`; R-NEO `community/import-tool/.../ImportCommand.java:372-393`). An input id is unique within its id space; a file without one uses the default space. Export writes `,`, `;` and no id space.
    - **JSON Lines**: one object per node (`type`, `id`, `labels`, `properties`) and per edge (`type`, `id`, `label`, `properties`, `start`, `end`), as APOC writes them [S44].
      - `id` is a JSON string. `start` and `end` are objects `{"id", "labels"}`; import reads only their `id` (R-APOC `core/src/main/java/apoc/export/json/JsonFormatSerializer.java:37-51, 88, 106-116`).
      - A JSON `null` property value is an invalid record: a property cannot hold null (IV011, [TS-21](values-and-types.md#ts-21-values-and-types)).
      - A number without fraction or exponent is an integer. serde_json reads one from 2<sup>63</sup> to 2<sup>64</sup> − 1 as `u64`, which is an invalid record (`i64` is the one integer type). It reads `-0` and every integer of larger magnitude as `f64` (serde_json 1.0.154 `src/de.rs:486-494, 517-524`); import keeps those as floats. lpg's export writes neither.
    - **GraphML**: one `<key>` per property.
      - Node labels are only in the data key `labels`, written `:A:B`; the edge type is only in the data key `label`; in both, `\` escapes `:` and `\`. `<node>` and `<edge>` carry no `labels` or `label` attribute: APOC writes both the attribute and the data key (R-APOC `core/src/main/java/apoc/export/graphml/XmlGraphMLWriter.java:129-156, 169-170`), and the GraphML 1.0 schema rejects the attribute (measured, M-S4 Appendix A).
      - **XML characters.** XML 1.0 `Char` excludes U+0000–U+0008, U+000B, U+000C, U+000E–U+001F, U+FFFE and U+FFFF (D-XML §2.2). A string value that holds one is annotated as `STRING`, in the escaped lexical form of [TS-21](values-and-types.md#ts-21-values-and-types); a label, edge type or property name that holds one fails the export with an error naming it. `\r` is written `&#13;` (quick-xml 0.42.0 `escape`, `src/escape.rs:93-110`); the reader applies XML 1.0 end-of-line handling (D-XML §2.11; quick-xml `xml10_content`, `src/events/mod.rs:625-628`).
      - A `<data>` element with no text reads as the empty string (R-NX `readwrite/graphml.py:1056-1057`).
      - The reader reads its file twice, nodes first, then edges: GraphML allows an edge before its nodes, and NetworkX reads every node before any edge (`readwrite/graphml.py:902-960`).
      - **Extension schema** (user decision 2026-10-11, item B option (a), rmp #55 DECISION 400). `src/ops/bulk/graphml-lpg.xsd`, in the GraphML namespace, `xs:redefine`s `http://graphml.graphdrawing.org/xmlns/1.0/graphml.xsd` and adds the attribute `type` of the namespace `https://crates.io/crates/lpg/graphml` to the attribute groups `key.extra.attrib` and `data.extra.attrib`, the extension points the GraphML schema provides (`graphml-structure.xsd:44-49, 60-66`; `graphml-attributes.xsd:27-45` extends `key.extra.attrib` the same way; D-GMLXSD). The attribute is declared in `src/ops/bulk/graphml-lpg-type.xsd`, whose target namespace is the lpg namespace: an XSD document declares attributes of its own target namespace only. Both files ship in the crate package, and neither copies text of the GraphML schema files, whose licence is not confirmed. Every GraphML export validates against `graphml-lpg.xsd`. Measured 2026-10-11 with xmlschema 4.3.2 on a probe of the two files: a file with `lpg:type` on `<key>` and `<data>` is valid against it and invalid against `graphml.xsd`; a file with another lpg attribute, or with a `labels` attribute on `<node>`, is invalid against both (rmp #55 TEST).
    - **Native dump**: an lpg binary format using the record encoding of [TS-21](values-and-types.md#ts-21-values-and-types).
      - A header: magic, dump format version, scope (database or graph), the crate's format version ([TS-28](#ts-28-format-versioning-and-migration)).
      - Blocks in this order: catalog definitions and tokens, node records, edge records, index and constraint definitions. Each block is length-prefixed and carries a CRC-32C.
      - A trailer: the count of each block kind, and its own CRC-32C, so a truncated dump fails.
      - A dump that fails a length or CRC-32C check fails the import whichever invalid-record option is set: no record of a damaged block is known to be complete.
      - The checks follow the backup `MANIFEST` (above; `src/ops/backup.rs:132-160`); the order follows Turso's `.dump`, which writes each table's definition and rows, then the indexes and the other schema objects (R-TURSO @ `be5bab8` `cli/app.rs:1847-1874`).
      - **Database scope**: what a backup covers (FR-OPS-008), the catalog, every graph, and every index and constraint definition; not registered procedures and functions. Owners are not dumped.
      - **Database-scope import** (user decision 2026-10-11, item D option (a), rmp #55 DECISION 402). The target catalog must hold only the objects `create` writes, `/public` and an empty `/public/lpg` ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)); otherwise the import fails. One transaction creates the dump's other catalog objects (directories, schemas, graph types, then graphs) and every index and constraint definition; the importing principal owns each object it creates ([TS-23](interfaces.md#ts-23-authentication-and-authorisation)). Each graph is then loaded by the fast initial load, which builds those indexes and checks those constraints (steps 2 and 4, below). A failure leaves the created objects, and the graphs loaded before it, in place. The import mode option does not apply to this scope.
    - **Query results**: CSV and JSON Lines write one record per result row, with the result's columns. GraphML writes the nodes and edges the result contains, including those inside paths and lists, and leaves out the other values.
  - **Type annotations (FR-IMP-006).** Values use the lexical forms and type names of [TS-21](values-and-types.md#ts-21-values-and-types). A property, or a query-result column, is **uniform** when all its exported values have one type, and **mixed** otherwise.

    | Value | CSV | JSON Lines | GraphML |
    |---|---|---|---|
    | BOOLEAN, uniform | `boolean` column | JSON boolean | `attr.type="boolean"` |
    | 64-bit integer, uniform | `long` column | JSON number without fraction or exponent | `attr.type="long"` |
    | 64-bit float, finite, uniform | `double` column | JSON number with a fraction or exponent | `attr.type="double"` |
    | 64-bit float, NaN or ±infinity | `double` column: `NaN`, `Infinity`, `-Infinity` | tagged object | `attr.type="string"` + lpg type attribute |
    | character string, uniform, never empty | `string` column | JSON string | `attr.type="string"` |
    | DATE, LOCAL TIME, ZONED TIME, LOCAL DATETIME, ZONED DATETIME, uniform | `date`, `localtime`, `time`, `localdatetime`, `datetime` column | tagged object | `attr.type="string"` + lpg type attribute |
    | point, uniform | `point` column | tagged object | `attr.type="string"` + lpg type attribute |
    | list of one of the above, uniform | `<type>[]` column | JSON array | `attr.type="string"` + lpg type attribute |
    | any other type (both duration kinds, byte string, record, nested list, other numerics, node, edge, path) | `lpg:<type name>` column | tagged object | `attr.type="string"` + lpg type attribute |
    | any value of a mixed property or column, or a string column that holds an empty string | `lpg:annotated` column; each value written as `<type name>:<lexical form>` | each value by its own row above | `attr.type="string"`, lpg type attribute on each `<data>` element |

    - A tagged object has exactly the two keys `"$lpg_type"`, the type name, and `"$value"`, a JSON string holding the lexical form. Every JSON object in a property or value position is read as a tagged object.
    - The GraphML type attribute is `type` in the namespace `https://crates.io/crates/lpg/graphml`.
  - **Records.** Importers process nodes before edges. Tokens new to the catalog (labels, edge types, property names) are created by an ordinary transaction before the import or load writes; tokens left by an abort stay unused ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics); `src/catalog/mod.rs`).
  - **Invalid records** (FR-IMP-004): a malformed record; a value not convertible to its column type or tagged type; a duplicate input id in one id space; an edge with an unknown endpoint; an edge whose endpoint was skipped; a constraint violation. Neo4j names the same cases in `--skip-duplicate-nodes` and `--skip-bad-relationships` (R-NEO `community/import-tool/.../ImportCommand.java:158-159`).
    - A malformed header, or a damaged native dump, fails the import: it is not a record.
    - With abort, the default, the import stops at the first invalid record and adds nothing. With skip, it continues without the record.
    - **Report** (user decision 2026-10-11, item C option (a), rmp #55 DECISION 401). The application names a report file. The crate writes it as an export file (Export files, below), one line per invalid record, and the call returns the counts of imported nodes, imported edges and invalid records. Neo4j's import tool writes its report to the file `--report-file` names, by default `report.json.log` (R-NEO `community/import-tool/.../ImportCommand.java:261`, `ImportContext.java:114`).
    - Each report line is one JSON object with the keys `file`; `record`, the line for CSV and JSON Lines, the element ordinal for GraphML, the block and record ordinals for the native dump; `id`, the input id or `null`; `gqlstatus`; and `reason`. Kuzu keeps the message, file path and line number of each skipped record (R-KUZU `src/include/processor/warning_context.h`; `PopulatedCopyFromError`, `src/include/processor/operator/persistent/reader/copy_from_error.h:82-87`); PostgreSQL's `COPY` with `LOG_VERBOSITY verbose` reports the input line and the column of each discarded row (R-PG @ `1b5dd3a` `doc/src/sgml/ref/copy.sgml:467-475`).
  - **GQLSTATUS of import and export**, by the rule of [GQLSTATUS codes](interfaces.md#gqlstatus-codes). ISO lines are those of the conditions insert; Neo4j lines those of `GqlStatusInfoCodes.java` @ `54a7dcf`. The ISO text is not available, so an ISO code is chosen by its condition name (provisional, OQ-005).

    | Failure | GQLSTATUS | Source |
    |---|---|---|
    | A malformed record or header; a damaged native dump; an unknown type name; a duplicate input id in one id space; an edge with an unknown or skipped endpoint | 22N05 | Neo4j "input failed validation" (557), by description |
    | A text that is not a lexical form of its column or tagged type: a temporal instant; a duration; any other type | 22007; 22G0H; 22018 | ISO (74; 93; 79), by name |
    | A JSON integer outside the `i64` range | 22003 | ISO (72) |
    | A JSON `null` property value | 22004 | ISO (73), by name |
    | A tagged type that a property cannot hold | 22G03 | IV011 ([TS-21](values-and-types.md#ts-21-values-and-types)) |
    | A constraint violation | as `ConstraintViolation` | [GQLSTATUS codes](interfaces.md#gqlstatus-codes) |
    | An import that aborts on an invalid record | the code of that record | Invalid records, above |
    | An excluded format and scope pair; a name a format cannot hold; an export or report file that exists; a delimiter equal to the array delimiter | 22N11 | `InvalidArgument` ([GQLSTATUS codes](interfaces.md#gqlstatus-codes)), as for a backup target that is not empty (`src/ops/backup.rs:255-256`) |
    | A fast initial load into a graph that is not empty; a database-scope import into a catalog that holds other objects | 51N41 | Neo4j "admin operation failed" (3201), by description, as `LoadRunning` |
    | Storage full, or any other I/O failure | 50N00 | `Io` ([GQLSTATUS codes](interfaces.md#gqlstatus-codes)); neither the ISO insert nor `GqlStatusInfoCodes.java` has a code for a full device (searched 2026-10-11) |
    | The memory cap; the transaction memory limit | 51N72; 51N73 | [GQLSTATUS codes](interfaces.md#gqlstatus-codes) |
    | A refusal of the authoriser | 42NFF | [GQLSTATUS codes](interfaces.md#gqlstatus-codes) |

  - **Authorisation** (FR-AUTH-001). Import and export are `Session` operations; each asks once per `(action, object)` pair ([TS-23](interfaces.md#ts-23-authentication-and-authorisation); `src/auth/mod.rs:50-80`):
    - export: `TRAVERSE` and `READ` on each graph of its scope; a query-result export asks what `execute` asks for its statement;
    - import and fast initial load: `CREATE`, `SET LABEL` and `SET PROPERTY` on the target graph;
    - database-scope import: those on each graph, `CREATE` of each catalog object kind it creates, on the object's path, and `CREATE INDEX` and `CREATE CONSTRAINT` on each graph with definitions.
  - **Export files.** The caller names each output file, in a location that exists. Each file is written as `<name>.tmp`, synced, renamed to `<name>`, and the location is then synced, as `MANIFEST` is (above; R-PG `src/bin/pg_basebackup/pg_basebackup.c:2298-2321`). An export fails if a target file exists, and removes its files on failure (UC-OPS-004 A1).
  - **Memory.** The transactional import's id map is charged to the memory cap. Parse and write buffers, the report's included, have fixed sizes outside the cap, as the 1 MiB copy buffer has ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap)). The fast initial load's memory is its sort reservations (FR-LIM-003, FR-LIM-007).
  - **Transactional import** runs as one transaction. It adds all records or none (FR-IMP-004), and its writes with their old versions must fit the memory cap (FR-LIM-007).
  - **Fast initial load** targets an empty graph.
    1. Wait for a running backup to end. Take the X graph lock ([TS-15](transactions-and-recovery.md#ts-15-serializable-isolation-and-write-locks)), so writers of the graph wait; readers keep reading the empty graph. If the graph is not empty, fail.
    2. External merge sort of the input into temporary files through the storage trait, in the database location ([TS-05](storage.md#ts-05-files-and-pages)):
       - nodes by (id space, input id), which assigns identifiers in order and finds duplicate input ids;
       - edges by start input id, merged with the sorted nodes to resolve starts, then by end input id to resolve ends;
       - the adjacency keys (`adj_key`), and each index's keys (`index_keys`).

       On the sorted runs, check the constraints already defined on the graph; uniqueness is checked on adjacent sorted keys (`UniqueSorted`, `src/index/build.rs:554-575`, a port of PostgreSQL's `enforceUnique`). Each violation is an invalid record, handled by the abort-or-skip option (FR-IMP-004). An edge whose endpoint was skipped is invalid as well.
    3. Build the node, edge and adjacency trees bottom-up, filled to 90 %, into pages taken in logged runs and owned by the build's record ([TS-05](storage.md#ts-05-files-and-pages) Build pages).
    4. Then build every index already defined on the graph bottom-up from sorted runs. The label or edge-type lookup tree is built only if that index exists.
    5. `sync_data`.
    6. Write one WAL group that installs the tree roots, the new allocation state, the statistics and a new version of the graph's catalog entry.
       - **Statistics**: the counts per label and per `(label, type, direction)` gathered during the build, added in one batch as an index's ONLINE commit adds its build's counts ([TS-18](catalog-and-indexes.md#ts-18-indexes-and-constraints) step 6; `switch`, `src/index/mod.rs:974`).
       - **Catalog version.** Built entries carry stamp 0, which every snapshot sees (`src/storage/btree/bulk.rs:223, 774`; [TS-06](storage.md#ts-06-graph-layout)). A snapshot that reads the older version of the graph's catalog entry reads the graph as empty, which is its exact state at that snapshot: the graph was empty and had no writer since step 1. The catalog tree is stamped ([TS-17](catalog-and-indexes.md#ts-17-catalog-and-statistics)).
    7. Release the graph lock.

    If the load fails or crashes before step 6, the graph stays empty and its pages stay owned by its build record, whose release runs at once after a failure (again at close or the next open if that release fails) and at the next open after a crash ([TS-05](storage.md#ts-05-files-and-pages) Build pages). ENOSPC during reservation fails the load.
  - **Element identifiers.** Both import modes assign new identifiers; input identifiers only connect edges to nodes.
- **Satisfies**: FR-OPS-001, FR-OPS-002, FR-OPS-003, FR-OPS-007, FR-OPS-008, FR-OPS-009, FR-OPS-010, FR-REC-003, FR-REC-004, FR-IMP-001, FR-IMP-002, FR-IMP-003, FR-IMP-004, FR-IMP-005, FR-IMP-006, FR-IDX-001, FR-LIM-003, FR-LIM-007, FR-AUTH-001, BR-TYPE-002, NFR-PERF-002, NFR-PERF-005.
- **Evidence**:
  - PostgreSQL base backup: checkpoint, file copy while running, then the WAL from start to stop replayed at restore. Neo4j full backup copies the store and recovers ([P-STO](../evidence/research/research-01-storage.md) §Q7).
  - PostgreSQL's backup manifest records the WAL range (R-PG `src/backend/backup/basebackup.c:657-660`) and a CRC-32C per file by default (`basebackup.c:729`). `pg_basebackup` syncs the data, then durably renames `backup_manifest.tmp` to `backup_manifest`, so that an interrupted copy is not "mistaken for a valid backup" (`src/bin/pg_basebackup/pg_basebackup.c:2298-2321`).
  - PostgreSQL starts recovery of a restored backup from the start location in `backup_label`, not from the copied `pg_control` (R-PG `src/backend/access/transam/xlogrecovery.c:543-557, 1217-1230`). A checkpoint that completes during the copy can leave a later redo point in the copied `CONTROL`; restore therefore sets it to `start`.
  - `CONTROL` written last makes the location a database only when every other file is durable, as at create ([TS-05](storage.md#ts-05-files-and-pages)).
  - Backup end. PostgreSQL reads `backup_label` at the start of recovery and copies its requirement to reach the end of the backup into its control data, `ControlFile->backupEndRequired` (R-PG `src/backend/access/transam/xlogrecovery.c:345-347, 548-549, 943-963`); if the WAL ends first, startup fails with "WAL ends before end of online backup" (`src/backend/access/transam/xlog.c:6583-6599`).
  - Target. `pg_basebackup` requires its target directory to be empty, or creates it (R-PG `src/bin/pg_basebackup/pg_basebackup.c:739-782`, called at `2830`), and writes `backup_manifest` at the top of the target (`pg_basebackup.c:2313-2314`). RocksDB `Checkpoint` refuses a target that exists (R-ROCKS `utilities/checkpoint/checkpoint_impl.cc:409-417`) and copies every file, WAL included, flat into one directory by its file name (`checkpoint_impl.cc:472-484`). The SQLite backup API copies into one destination database (R-SQLITE `src/backup.c:133-146`). lpg keeps the empty-target rule, and cannot create the target.
  - Neo4j imports only into an empty database, and creates indexes and constraints during the import with `--schema` [S12].
  - Neo4j import header format and data types (D-HDR; [S38]).
  - APOC JSON Lines and GraphML layouts, and APOC query-result export (D-APOC; [S44]). GraphML allows six `attr.type` values (D-GML).
  - External sort and bottom-up build: R-PG `src/backend/utils/sort/tuplesort.c:11-15`, `src/backend/access/nbtree/nbtsort.c:4`.
  - Catalog version of a fast load. PostgreSQL documents the defect of loading rows visible to older snapshots: `COPY FREEZE` "violates the normal rules of MVCC visibility" (R-PG @ `1b5dd3a` `doc/src/sgml/ref/copy.sgml:275-278`), and `TRUNCATE` "is not MVCC-safe" (`doc/src/sgml/ref/truncate.sgml:147-149`).
  - Sprint 4 planning measurements and reference study: [M-S4](../evidence/planning/sprint-4-plan.md) §3, §4 and Appendix A; the decisions on rmp #55.
  - Measured: bottom-up load of the adjacency tree at 1.88 M edges/s with a 1 GiB build cache, at 100 M edges ([M-LAYOUT](../evidence/layout-bench/results.md)).
- **Crates**:
  - `csv` 1.4.0 (Unlicense/MIT): dependency.
  - `serde_json` 1.0.154 with the feature `float_roundtrip`: dependency ([TS-31](dependencies.md#ts-31-dependency-set)).
  - `quick-xml` 0.42.0 (MIT): dependency.
- **Rejected**:
  - Logical backup at an MVCC snapshot: a long reader holds old versions in memory for the whole copy (FR-LIM-005).
  - Restoring the copied `CONTROL` unchanged: its redo point can be later than `start`, and recovery would then skip changes to pages copied before it.
  - A manifest without file checksums: a backup damaged on its medium would be found only when a damaged page is read.
  - A `wal/` subdirectory in the artefact (`pg_basebackup` plain format writes WAL into `pg_wal/`, R-PG `src/bin/pg_basebackup/pg_basebackup.c:648-652, 684-696`; RocksDB `BackupEngine` keeps `meta/`, `private/<id>/` and `shared_checksum/` subdirectories, R-ROCKS `utilities/backup/backup_engine.cc:74-80`, because it shares files between incremental backups): [TS-03](interfaces.md#ts-03-storage-abstraction) has no directory operation, and the database itself keeps its WAL flat ([TS-05](storage.md#ts-05-files-and-pages)). Backups here are full only (FR-OPS-007).
  - A target created by the backup (`pg_basebackup`, RocksDB `Checkpoint`): the storage trait cannot create a directory.
  - Transactional import in independent batches: an abort would leave earlier batches committed, against FR-IMP-004.
  - Fast load by ordinary inserts: random I/O at 1–10 % cache.
  - APOC's conversion of mixed-type properties to plain strings: lossy, against FR-IMP-006.
  - Items A to D, the options the user did not choose (rmp #55 DECISION 399 to 402): lpg's CSV layout only, with a converter for LDBC in Sprint 6, or Neo4j's whole import option set; the GraphML type attribute without a schema, or a companion `<data>` key; the report in memory charged to the cap, or a callback per record; a database dump imported graph by graph into existing graphs, or a database scope for export only.
  - Loaded entries left visible to snapshots older than the install: a snapshot taken before the load would see the graph change, against serializable isolation (README) and step 1.
  - serde_json's `arbitrary_precision` feature, to read every JSON integer outside the `i64` range as invalid: Cargo builds a dependency with the union of the features every package enables (D-CARGOFEAT), so it would change serde_json for the application's own uses.
  - ISO G1003, "endpoint node not in current working graph", for an edge with an unknown endpoint: G1003 names a node that exists elsewhere, while no node has that input id.

Unverified: whether `neo4j-admin database import` accepts the point form of [TS-21](values-and-types.md#ts-21-values-and-types), which has a space after each colon; the measured probe had none ([unverified point 12](quality-and-testing.md#unverified-points)).

## TS-28 Format versioning and migration

- **Decision**:
  - **Version.** One `u32` format version, stored in each `CONTROL` slot and in page 0 of `data-<g>` ([TS-05](storage.md#ts-05-files-and-pages)). The `CONTROL` slot layout is the same in every format version, so any crate version can read it.
  - **At open.** An older version fails with "migration required". A newer version fails without writing anything.
  - **Migration** from N−1 to N runs on the closed location:
    1. Take `LOCK`; if the database is open, fail.
    2. If `CONTROL` does not show a clean shutdown, fail, asking for the database to be opened and closed with the previous crate version.
    3. If `available_space` is less than the size of `data-<g>` plus its `wal-<g>-<lsn>` segments, fail and change nothing.
    4. Write `data-<g+1>` in the new format inside the location, `sync_data` it, then `sync_dir`. Every page of `data-<g+1>` has a page LSN of at most R, the redo point of the authoritative `<g>` slot; copying each page's old LSN meets this. Step 4 writes no WAL segment of generation `<g+1>`.
    5. Write the next `CONTROL` slot with generation `<g+1>`, version N and redo point R; its other fields are those of the authoritative `<g>` slot. This one slot write is the switch.
    6. Remove the generation-`<g>` files.
  - **WAL position.** LSNs continue across generations. Step 2 requires a clean shutdown, which ends with the clean-close checkpoint ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)), so R is the end of the `<g>` WAL and nothing follows it. The WAL writer creates the first segment of `<g+1>` as `wal-<g+1>-<R>`, starting at LSN R ([TS-09](transactions-and-recovery.md#ts-09-wal-and-group-commit)). Until then the database has no `<g+1>` segment; the step-5 slot keeps the clean-shutdown flag set, so open reads this as an empty log ([TS-13](transactions-and-recovery.md#ts-13-recovery) step 4). Every page LSN is therefore at most the durable LSN, as the WAL-before-data rule requires ([TS-08](storage.md#ts-08-buffer-pool-and-memory-cap), [TS-10](transactions-and-recovery.md#ts-10-torn-page-protection)).
  - **Crash safety.** A crash before step 5 leaves the old database intact; the next open removes `<g+1>`. A crash after step 5 leaves the new database; the next open removes `<g>`.
  - **Rules.** Element identifiers are copied unchanged. An older format needs the intermediate crate versions; the error names the version required. There is no downgrade.
- **Satisfies**: FR-OPS-004, FR-OPS-011, FR-OPS-012, FR-OPS-013, BR-TYPE-002, FR-REC-007.
- **Evidence**:
  - Neo4j migrates only a stopped database and rewrites all data, and does not support downgrades ([S8], [S9], [S10]).
  - PostgreSQL's `pg_upgrade` refuses a source cluster that was not shut down cleanly [S47].
  - WAL position. `pg_upgrade` transfers relation files by copy, clone or link, so their page LSNs are kept (R-PG `src/bin/pg_upgrade/relfilenumber.c:531, 590-611`). It then sets the new cluster's next WAL file to the old cluster's "First log segment after reset" (`src/bin/pg_upgrade/pg_upgrade.c:866-872`; `src/bin/pg_upgrade/controldata.c:295-311`). `pg_resetwal` never moves the WAL start backwards: it starts beyond the old redo point and every existing segment (`src/bin/pg_resetwal/pg_resetwal.c:961-1020, 517-518`) and sets the redo point there (`pg_resetwal.c:914-919`). A page LSN beyond the flushed WAL is an error (`src/backend/access/transam/xlog.c:3026-3030`).
- **Rejected**:
  - Rewriting pages one by one inside the live files: a crash midway would leave a mixed format.
  - Migration at open: FR-OPS-004 requires an explicit migration.
  - Switching by renaming files: several renames cannot be made atomic, while one `CONTROL` slot write is.
  - Restarting LSNs at zero in `<g+1>`: step 4 would have to reset every page LSN, and no requirement gains from it.
  - Writing a first `<g+1>` segment during migration (`pg_resetwal` writes a segment holding a checkpoint record, `WriteEmptyXLOG`, `pg_resetwal.c:1138-1221`): lpg's checkpoint writes no WAL record ([TS-11](transactions-and-recovery.md#ts-11-checkpoint)), so the segment would hold no frame.
