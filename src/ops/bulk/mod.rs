//! Bulk import and export (TS-26; FR-IMP-001 to FR-IMP-006, FR-OPS-003,
//! UC-OPS-003, UC-OPS-004; rmp #57, #58, #59).
//!
//! - **Formats** ([`Format`]): CSV (`csv`), JSON Lines (`jsonl`),
//!   GraphML (`graphml`) and the native dump (`dump`).
//! - **Scopes** (FR-IMP-003): the native dump covers the database and graph
//!   scopes; CSV, JSON Lines and GraphML cover the graph and query-result
//!   scopes. An excluded pair fails with 22N11 before any file is written
//!   (`check_export`, `check_import`).
//! - **Records** (`model`): every importer reads nodes before edges into
//!   one record model; tokens new to the catalog are created by ordinary
//!   inner commits as records are read, before the import writes (TS-17).
//! - **Import modes**: one transaction (`import`), or the fast initial
//!   load into an empty graph (`fast`). Both assign new identifiers;
//!   input identifiers only connect edges to nodes (FR-IMP-005).
//! - **Invalid records** (FR-IMP-004): abort, the default, or skip; each
//!   one is a line of the report file the application names
//!   (`model::Report`).
//! - **Files** (`files`): every file goes through the storage trait;
//!   each output file, the report included, is written as `<name>.tmp`,
//!   synced, renamed and its location synced; an export fails if a target
//!   exists and removes its files on failure.

pub(crate) mod csv;
pub(crate) mod dump;
pub(crate) mod export;
pub(crate) mod fast;
pub(crate) mod files;
pub(crate) mod graphml;
pub(crate) mod import;
pub(crate) mod jsonl;
pub(crate) mod model;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use crate::api::Parameters;
use crate::error::Error;
use crate::gqlstatus::codes;

/// A file format of import and export (FR-IMP-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Separate node and edge files with Neo4j's header fields (TS-26).
    Csv,
    /// One JSON object per line, in APOC's layout (TS-26).
    JsonLines,
    /// GraphML 1.0 with lpg's type attribute (TS-26).
    GraphMl,
    /// lpg's native binary dump (TS-26).
    Dump,
}

/// What an export covers (FR-IMP-003).
#[derive(Debug, Clone, PartialEq)]
pub enum ExportScope {
    /// The whole database: its catalog, every graph, every index and
    /// constraint definition (FR-OPS-008). Native dump only.
    Database,
    /// The graph at this absolute catalog path, for example `/public/lpg`.
    Graph(String),
    /// The result of a GQL request, run as [`crate::Session::execute`]
    /// runs it. CSV, JSON Lines and GraphML only.
    Query {
        /// The request.
        gql: String,
        /// Its parameters.
        parameters: Parameters,
    },
}

/// What an export wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportSummary {
    /// Nodes written.
    pub nodes: u64,
    /// Edges written.
    pub edges: u64,
    /// Result rows written (query-result exports to CSV and JSON Lines).
    pub rows: u64,
}

/// How an import writes (FR-IMP-002).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImportMode {
    /// One transaction into an existing graph, within the memory cap.
    #[default]
    Transactional,
    /// The fast initial load into an empty graph (TS-26 steps 1 to 7).
    FastLoad,
}

/// What an import does with an invalid record (FR-IMP-004).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnInvalid {
    /// Stop at the first invalid record and add nothing (the default).
    #[default]
    Abort,
    /// Leave the record out and continue.
    Skip,
}

/// Where an import writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
    /// The graph at this absolute catalog path.
    Graph(String),
    /// The database: a database-scope native dump, into a catalog that
    /// holds only the default schema and an empty default graph (TS-26
    /// item D).
    Database,
}

/// One input file of an import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputFile {
    /// The file name, in the request's location.
    pub name: String,
    /// CSV node files: labels every node of the file gets, besides those
    /// of its `:LABEL` field (Neo4j's `--nodes=Label=file`).
    pub labels: Vec<String>,
    /// CSV edge files: the edge type of every edge of the file without a
    /// `:TYPE` value (Neo4j's `--relationships=TYPE=file`).
    pub edge_type: Option<String>,
}

impl InputFile {
    /// A file named `name`.
    #[must_use]
    pub fn new(name: &str) -> InputFile {
        InputFile {
            name: name.to_owned(),
            ..InputFile::default()
        }
    }
}

/// The type of the CSV `:ID`, `:START_ID` and `:END_ID` values (TS-26
/// Named ID column; FR-IMP-005), as Neo4j's `--id-type`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IdType {
    /// Text: identifiers match as written; a named ID column is a
    /// STRING property.
    #[default]
    String,
    /// Integers: identifiers match by value; a named ID column is an
    /// INT64 property; a value that is not an `i64` is an invalid record
    /// (22018).
    Integer,
}

/// An import request (UC-OPS-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRequest {
    /// The format of the files.
    pub format: Format,
    /// The graph or the database.
    pub target: ImportTarget,
    /// The location of the input files; it must exist.
    pub location: PathBuf,
    /// CSV node files; for the other formats, the one input file.
    pub nodes: Vec<InputFile>,
    /// CSV edge files; empty for the other formats.
    pub edges: Vec<InputFile>,
    /// Transactional (the default) or fast initial load. A database-scope
    /// dump is always fast-loaded.
    pub mode: ImportMode,
    /// Abort (the default) or skip invalid records.
    pub on_invalid: OnInvalid,
    /// The location of the report file; it must exist.
    pub report_location: PathBuf,
    /// The report file's name: one JSON line per invalid record. It must
    /// not exist (TS-26 item C).
    pub report: String,
    /// The CSV field delimiter, one byte; `,` by default.
    pub delimiter: u8,
    /// The CSV array and `:LABEL` delimiter, one byte, other than the
    /// field delimiter; `;` by default.
    pub array_delimiter: u8,
    /// The type of CSV identifiers; `string` by default.
    pub id_type: IdType,
}

impl ImportRequest {
    /// A request for `format` into `target` reading `file` in `location`,
    /// with the defaults: transactional, abort, `,` and `;`, the report
    /// file `report` in `location`.
    #[must_use]
    pub fn new(
        format: Format,
        target: ImportTarget,
        location: impl Into<PathBuf>,
        file: &str,
        report: &str,
    ) -> ImportRequest {
        let location = location.into();
        ImportRequest {
            format,
            target,
            report_location: location.clone(),
            location,
            nodes: vec![InputFile::new(file)],
            edges: Vec::new(),
            mode: ImportMode::default(),
            on_invalid: OnInvalid::default(),
            report: report.to_owned(),
            delimiter: b',',
            array_delimiter: b';',
            id_type: IdType::default(),
        }
    }

    /// A CSV request into graph `graph` with `nodes` and `edges` files in
    /// `location`, and the defaults of [`ImportRequest::new`].
    #[must_use]
    pub fn csv(
        graph: &str,
        location: impl Into<PathBuf>,
        nodes: Vec<InputFile>,
        edges: Vec<InputFile>,
        report: &str,
    ) -> ImportRequest {
        let mut r = ImportRequest::new(
            Format::Csv,
            ImportTarget::Graph(graph.to_owned()),
            location,
            "",
            report,
        );
        r.nodes = nodes;
        r.edges = edges;
        r
    }
}

/// What an import added (FR-IMP-004: the counts the call returns).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Nodes imported.
    pub nodes: u64,
    /// Edges imported.
    pub edges: u64,
    /// Invalid records found: each is a line of the report.
    pub invalid: u64,
    /// The failure of the report file's write, sync or rename, which
    /// happens after the data committed: the import added its data, and
    /// the report holds no line (TS-26 Report; once the commit record is
    /// written it is too late to abort, PostgreSQL `xact.c:2442-2445`).
    /// `None` when the report was written.
    pub report_error: Option<Error>,
}

/// 22N11, invalid argument (TS-26 GQLSTATUS table).
pub(crate) fn invalid_argument(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// 22N05, input failed validation (TS-26 GQLSTATUS table).
pub(crate) fn invalid_input(what: &str) -> Error {
    Error::new(codes::C22N05, format!("input failed validation: {what}"))
}

/// The number of output files an export of `scope` to `format` names:
/// two for a CSV graph export (nodes, then edges), one otherwise.
fn export_files(format: Format, scope: &ExportScope) -> usize {
    match (format, scope) {
        (Format::Csv, ExportScope::Graph(_)) => 2,
        _ => 1,
    }
}

/// Fails with 22N11 unless the format covers the scope (FR-IMP-003) and
/// `files` names the files the export writes, each a plain file name.
///
/// # Errors
///
/// 22N11.
pub(crate) fn check_export(
    format: Format,
    scope: &ExportScope,
    files: &[&str],
) -> Result<(), Error> {
    let covered = match scope {
        ExportScope::Database => format == Format::Dump,
        ExportScope::Graph(_) => true,
        ExportScope::Query { .. } => format != Format::Dump,
    };
    if !covered {
        return Err(invalid_argument(
            "the format does not cover the export scope",
        ));
    }
    if files.len() != export_files(format, scope) {
        return Err(invalid_argument(
            "the number of file names does not fit the format and scope",
        ));
    }
    files::check_names(files)
}

/// Fails with 22N11 unless the request is a covered format and target
/// pair with the files that format reads (FR-IMP-003), and its delimiters
/// differ.
///
/// # Errors
///
/// 22N11.
pub(crate) fn check_import(r: &ImportRequest) -> Result<(), Error> {
    if r.target == ImportTarget::Database && r.format != Format::Dump {
        return Err(invalid_argument(
            "only a native dump imports a database scope",
        ));
    }
    match r.format {
        Format::Csv => {
            if r.nodes.is_empty() && r.edges.is_empty() {
                return Err(invalid_argument("a CSV import names no file"));
            }
            if r.delimiter == r.array_delimiter {
                return Err(invalid_argument(
                    "the field delimiter equals the array delimiter",
                ));
            }
            if [r.delimiter, r.array_delimiter]
                .iter()
                .any(|d| matches!(d, b'"' | b'\\' | b'\r' | b'\n'))
            {
                return Err(invalid_argument("a delimiter that CSV quoting needs"));
            }
        }
        _ => {
            if r.nodes.len() != 1 || !r.edges.is_empty() {
                return Err(invalid_argument("this format reads exactly one file"));
            }
        }
    }
    let names: Vec<&str> = r
        .nodes
        .iter()
        .chain(&r.edges)
        .map(|f| f.name.as_str())
        .collect();
    files::check_names(&names)?;
    files::check_names(&[r.report.as_str()])
}
