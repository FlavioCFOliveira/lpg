//! CSV (TS-26 Export layouts; FR-IMP-001, FR-IMP-006; rmp #59).
//!
//! **Files.** A graph export writes one node file (`:ID`, `:LABEL`, then
//! one column per property name) and one edge file (`:START_ID`,
//! `:END_ID`, `:TYPE`, then the property columns); the property columns
//! are the union of the property names, and an absent property is an
//! empty field. Fields are quoted as RFC 4180 requires (`csv` 1.4.0).
//! Inside an array or `:LABEL` field the elements are separated by the
//! array delimiter, `;` on export, and `\` escapes it and itself.
//!
//! **Column types** (TS-26 table, [`Kind`]): a uniform property of a
//! Neo4j type has that column type (`long`, `double`, `boolean`, `string`,
//! `date`, `localtime`, `time`, `localdatetime`, `datetime`, `point`, and
//! `<type>[]`); any other uniform type is `lpg:<type name>`; a mixed
//! property, or a string property that holds an empty string, is
//! `lpg:annotated`, each value written `<type name>:<lexical form>`. A
//! property name that holds `:`, or is empty, fails the export with 22N11.
//!
//! **Header grammar** on import: `<name>:lpg:<type name>` and
//! `<name>:lpg:annotated`, else APOC's field grammar `name`, `:type`,
//! `{options}`, `(idspace)`, `[]`, ported to a hand-written parser from
//! APOC's `CsvHeaderField` and `CsvLoaderConstants.FIELD_PATTERN`
//! (R-APOC @ `e52bc20`
//! `core/src/main/java/apoc/export/csv/CsvHeaderField.java`,
//! `CsvLoaderConstants.java:28-36`; Apache-2.0,
//! `Copyright (c) "Neo4j" Neo4j Sweden AB [http://neo4j.com]`; notice in
//! `LICENSE-APACHE`). Type names are read case-insensitively; `int`, `short` and `byte` read as
//! `long`, `float` as `double` (TS-26; Neo4j's `DataFactories` and
//! `Extractors` were studied, GPL, nothing copied). Neo4j input (TS-26
//! item A): id spaces, `:IGNORE`, a one-byte delimiter and array
//! delimiter, and labels or an edge type per file. A named `:ID` column
//! connects edges only: input identifiers are not stored (FR-IMP-005).

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::export::{Analysis, ContentsMap, Rows, Seen, each_edge, each_node};
use super::files::{Outputs, Reader, Writer, io_error};
use super::model::{
    EdgeIn, Invalid, NodeIn, Pos, Sink, Source, annotated_value, lexical_error, typed_value,
};
use super::{ExportSummary, ImportRequest, InputFile, invalid_argument, invalid_input};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::storage::Storage;
use crate::storage::engine::Engine;
use crate::storage::graph::Graph;
use crate::sync::Arc;
use crate::tx::Transaction;
use crate::types::lexical::{Contents, NoContents, TypeName, annotated, float_text, lexical_with};
use crate::types::{Point, Srid, Value};

/// The column type of a property or result column (TS-26 table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A Neo4j column type, of a list column if `list`.
    Neo4j(TypeName, bool),
    /// `lpg:<type name>`.
    Lpg(TypeName),
    /// `lpg:annotated`.
    Annotated,
}

/// The Neo4j type name of a column of values of type `t`, if one applies.
fn neo4j_name(t: &TypeName) -> Option<&'static str> {
    Some(match t {
        TypeName::Boolean => "boolean",
        TypeName::Integer => "long",
        TypeName::Float => "double",
        TypeName::String => "string",
        TypeName::Date => "date",
        TypeName::LocalTime => "localtime",
        TypeName::ZonedTime => "time",
        TypeName::LocalDateTime => "localdatetime",
        TypeName::ZonedDateTime => "datetime",
        TypeName::Point => "point",
        _ => return None,
    })
}

/// The column type of values `seen` (TS-26 table).
pub(crate) fn kind_of(seen: &Seen) -> Kind {
    let Some(t) = seen.uniform() else {
        return Kind::Annotated;
    };
    if seen.empty_string {
        return Kind::Annotated;
    }
    // Neo4j 2026.09.0 refuses a point with a non-finite coordinate
    // (measured, rmp #59 FINDING 436): such a `point` or `point[]` column
    // is an `lpg:` column (DECISION 445, 448).
    if seen.non_finite_point {
        return Kind::Lpg(t.clone());
    }
    if neo4j_name(t).is_some() {
        return Kind::Neo4j(t.clone(), false);
    }
    if let TypeName::List(Some(inner)) = t
        && neo4j_name(inner).is_some()
    {
        return Kind::Neo4j((**inner).clone(), true);
    }
    Kind::Lpg(t.clone())
}

/// The header field of property `name` of column type `kind`.
///
/// # Errors
///
/// 22N11 for a name that holds `:` or is empty: APOC's field grammar
/// names a column `[^:]*` (`CsvLoaderConstants.java:29`).
pub(crate) fn header_field(name: &str, kind: &Kind) -> Result<String, Error> {
    if name.is_empty() || name.contains(':') {
        return Err(invalid_argument(&format!(
            "the property name {name:?} cannot be a CSV column name"
        )));
    }
    Ok(match kind {
        Kind::Neo4j(t, false) => format!("{name}:{}", neo4j_name(t).expect("a Neo4j type")),
        Kind::Neo4j(t, true) => format!("{name}:{}[]", neo4j_name(t).expect("a Neo4j type")),
        Kind::Lpg(t) => format!("{name}:lpg:{t}"),
        Kind::Annotated => format!("{name}:lpg:annotated"),
    })
}

/// Appends `s` with `delim` and `\` escaped by `\`.
fn push_escaped(out: &mut String, s: &str, delim: char) {
    for c in s.chars() {
        if c == delim || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
}

/// The text of a value in a Neo4j column: a string as itself, a float in
/// the float form (`NaN`, `Infinity`, `-Infinity` included), every other
/// value in its lexical form.
fn neo4j_text(v: &Value, c: &dyn Contents) -> String {
    match v {
        Value::String(s) => s.to_string(),
        Value::Float(x) => float_text(*x),
        v => lexical_with(v, c),
    }
}

/// The field of value `v` in a column of type `kind`; `pre` is its
/// lexical form when already rendered with its contents.
pub(crate) fn field(v: &Value, kind: &Kind, c: &dyn Contents) -> String {
    match kind {
        Kind::Neo4j(_, false) => neo4j_text(v, c),
        Kind::Neo4j(_, true) => {
            let Value::List(items) = v else {
                return neo4j_text(v, c);
            };
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(';');
                }
                push_escaped(&mut out, &neo4j_text(item, c), ';');
            }
            out
        }
        Kind::Lpg(_) => lexical_with(v, c),
        Kind::Annotated => annotated(v, c),
    }
}

fn csv_error(e: ::csv::Error) -> Error {
    match e.into_kind() {
        ::csv::ErrorKind::Io(e) => io_error(e),
        _ => crate::api::internal("a CSV write failed"),
    }
}

fn writer(w: &mut Writer) -> ::csv::Writer<&mut Writer> {
    ::csv::WriterBuilder::new().flexible(true).from_writer(w)
}

/// The labels field: the labels separated by `;`, each escaped.
fn labels_field(labels: &[String]) -> String {
    let mut out = String::new();
    for (i, l) in labels.iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        push_escaped(&mut out, l, ';');
    }
    out
}

/// Writes graph `g` to the node file `files[0]` and the edge file
/// `files[1]` of `out`, with the column types of `a`.
///
/// # Errors
///
/// 22N11 for a property name a CSV column cannot hold; storage and I/O
/// errors.
pub(crate) fn write_graph(
    engine: &Engine,
    tx: &mut Transaction,
    g: &Graph,
    a: &Analysis,
    out: &mut Outputs<'_>,
    files: &[usize],
    summary: &mut ExportSummary,
) -> Result<(), Error> {
    let columns = |m: &BTreeMap<String, Seen>| -> Result<Vec<(String, Kind, String)>, Error> {
        m.iter()
            .map(|(n, s)| {
                let k = kind_of(s);
                let h = header_field(n, &k)?;
                Ok((n.clone(), k, h))
            })
            .collect()
    };
    let (node_cols, edge_cols) = (columns(&a.node)?, columns(&a.edge)?);
    {
        let mut w = writer(out.writer(files[0]));
        let mut head = vec![":ID".to_owned(), ":LABEL".to_owned()];
        head.extend(node_cols.iter().map(|(_, _, h)| h.clone()));
        w.write_record(&head).map_err(csv_error)?;
        each_node(engine, tx, g, &mut |n| {
            let mut rec = vec![n.id.to_string(), labels_field(&n.labels)];
            for (name, kind, _) in &node_cols {
                rec.push(match n.props.iter().find(|(k, _)| k == name) {
                    Some((_, v)) => field(v, kind, &NoContents),
                    None => String::new(),
                });
            }
            summary.nodes += 1;
            w.write_record(&rec).map_err(csv_error)
        })?;
        w.flush().map_err(io_error)?;
    }
    let mut w = writer(out.writer(files[1]));
    let mut head = vec![
        ":START_ID".to_owned(),
        ":END_ID".to_owned(),
        ":TYPE".to_owned(),
    ];
    head.extend(edge_cols.iter().map(|(_, _, h)| h.clone()));
    w.write_record(&head).map_err(csv_error)?;
    each_edge(engine, tx, g, &mut |e| {
        let mut rec = vec![e.src.to_string(), e.dst.to_string(), e.ty.clone()];
        for (name, kind, _) in &edge_cols {
            rec.push(match e.props.iter().find(|(k, _)| k == name) {
                Some((_, v)) => field(v, kind, &NoContents),
                None => String::new(),
            });
        }
        summary.edges += 1;
        w.write_record(&rec).map_err(csv_error)
    })?;
    w.flush().map_err(io_error)
}

/// Writes the rows of a query result: one record per row, a column per
/// result column, typed as a property column is; a null is an empty
/// field. The rows are spooled first ([`super::export`]), since the column
/// types depend on every row.
///
/// # Errors
///
/// The statement's errors; 22N11 for a column name a CSV column cannot
/// hold; storage and I/O errors.
pub(crate) fn write_rows(
    engine: &Engine,
    rows: &mut dyn Rows,
    w: &mut Writer,
    summary: &mut ExportSummary,
) -> Result<(), Error> {
    let columns = rows.columns();
    let mut seen = vec![Seen::default(); columns.len()];
    let memory =
        (engine.core().accountant.cap() / 64).clamp(crate::storage::sort::MIN_MEMORY, 16 << 20);
    let mut spool = engine.sorter(memory)?;
    let mut contents = ContentsMap::default();
    let mut n = 0u64;
    while let Some(row) = rows.next_row(&mut contents)? {
        // Each cell: its value, and its lexical form when it holds nodes
        // or edges, rendered now with their contents.
        let mut cells = Vec::with_capacity(row.len() * 2);
        for (i, v) in row.iter().enumerate() {
            if *v != Value::Null {
                seen[i].add(v);
            }
            cells.push((2 * i as u64, v.clone()));
            let (mut ns, mut es) = (Vec::new(), Vec::new());
            super::export::elements(v, &mut ns, &mut es);
            if !ns.is_empty() || !es.is_empty() {
                cells.push((2 * i as u64 + 1, Value::string(&lexical_with(v, &contents))));
            }
        }
        let mut b = Vec::new();
        crate::types::record::encode(&[], &cells, &mut b)
            .map_err(|_| invalid_argument("a result row too large to export"))?;
        spool.push(&n.to_be_bytes(), &b)?;
        n += 1;
        contents = ContentsMap::default();
    }
    let kinds: Vec<Kind> = seen.iter().map(kind_of).collect();
    let mut cw = writer(w);
    let head = columns
        .iter()
        .zip(&kinds)
        .map(|(c, k)| header_field(c, k))
        .collect::<Result<Vec<_>, _>>()?;
    cw.write_record(&head).map_err(csv_error)?;
    let mut sorted = spool.finish()?;
    while let Some((_, b)) = sorted.next_record()? {
        let (_, cells) = crate::types::record::decode(b)
            .ok_or_else(|| crate::api::internal("a spooled row does not decode"))?;
        let mut rec = Vec::with_capacity(columns.len());
        for (i, kind) in kinds.iter().enumerate() {
            let v = cells
                .iter()
                .find(|(k, _)| *k == 2 * i as u64)
                .map(|(_, v)| v);
            let pre =
                cells
                    .iter()
                    .find(|(k, _)| *k == 2 * i as u64 + 1)
                    .and_then(|(_, v)| match v {
                        Value::String(s) => Some(s.to_string()),
                        _ => None,
                    });
            rec.push(match (v, pre) {
                (None | Some(Value::Null), _) => String::new(),
                (Some(v), Some(pre)) => match kind {
                    Kind::Annotated => format!("{}:{pre}", TypeName::of(v)),
                    _ => pre,
                },
                (Some(v), None) => field(v, kind, &NoContents),
            });
        }
        cw.write_record(&rec).map_err(csv_error)?;
        summary.rows += 1;
    }
    cw.flush().map_err(io_error)
}

// ---------------------------------------------------------------------------
// Import.

/// The type of a property column on import.
#[derive(Debug, Clone, PartialEq)]
enum ColType {
    /// A Neo4j scalar type, of an array column if the flag is set.
    Neo4j(TypeName, bool),
    Lpg(TypeName),
    Annotated,
}

/// One header field on import.
#[derive(Debug, Clone, PartialEq)]
enum Field {
    /// `name:ID(space)`: the id space, and the property name, empty for
    /// an unnamed column.
    Id(String, String),
    StartId(String),
    EndId(String),
    Label,
    Type,
    Ignore,
    Property {
        name: String,
        ty: ColType,
        /// The header's `{options}`: a point's `srid` or `crs`.
        options: Vec<(String, String)>,
    },
}

/// The options of `{k: v, …}`, as APOC's `KEY_VALUE_PATTERN` reads them
/// (`CsvLoaderConstants.java:31-32`).
fn options(text: &str) -> Vec<(String, String)> {
    text.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once(':')?;
            let (k, v) = (k.trim(), v.trim());
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_owned(), v.to_owned()))
        })
        .collect()
}

/// Parses header field `raw` (module documentation).
///
/// # Errors
///
/// 22N05 for a malformed field (TS-26: a malformed header fails the
/// import).
fn parse_field(raw: &str) -> Result<Field, Error> {
    let bad = || invalid_input("a malformed CSV header field");
    if let Some(i) = raw.find(":lpg:") {
        let name = &raw[..i];
        let t = &raw[i + 5..];
        if name.contains(':') {
            return Err(bad());
        }
        let ty = if t == "annotated" {
            ColType::Annotated
        } else {
            ColType::Lpg(TypeName::parse(t).ok_or_else(bad)?)
        };
        return Ok(Field::Property {
            name: name.to_owned(),
            ty,
            options: Vec::new(),
        });
    }
    // `^(?<name>[^:]*)(:(?<type>\w+))?(?<optPar>\{.*})?(\((?<idspace>[-a-zA-Z_0-9]+)\))?(?<array>\[\])?$`
    let (name, mut rest) = match raw.find(':') {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };
    let mut ty = None;
    if let Some(r) = rest.strip_prefix(':') {
        let n = r
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if n == 0 {
            return Err(bad());
        }
        ty = Some(&r[..n]);
        rest = &r[n..];
    }
    let mut opts = Vec::new();
    if rest.starts_with('{') {
        let close = rest.rfind('}').ok_or_else(bad)?;
        opts = options(&rest[1..close]);
        rest = &rest[close + 1..];
    }
    let mut space = String::new();
    if let Some(r) = rest.strip_prefix('(') {
        let close = r.find(')').ok_or_else(bad)?;
        space = r[..close].to_owned();
        if space.is_empty()
            || !space
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(bad());
        }
        rest = &r[close + 1..];
    }
    let array = match rest {
        "" => false,
        "[]" => true,
        _ => return Err(bad()),
    };
    let upper = ty.map(str::to_ascii_uppercase);
    let special = |f: Field| {
        if array { Err(bad()) } else { Ok(f) }
    };
    let neo4j = |t: TypeName| -> Result<Field, Error> {
        if !space.is_empty() {
            return Err(bad());
        }
        Ok(Field::Property {
            name: name.to_owned(),
            ty: ColType::Neo4j(t, array),
            options: opts.clone(),
        })
    };
    match upper.as_deref() {
        Some("ID") => special(Field::Id(space.clone(), name.to_owned())),
        Some("START_ID") => special(Field::StartId(space.clone())),
        Some("END_ID") => special(Field::EndId(space.clone())),
        Some("LABEL") => Ok(Field::Label),
        Some("TYPE") => special(Field::Type),
        Some("IGNORE") => Ok(Field::Ignore),
        None | Some("STRING") => neo4j(TypeName::String),
        Some("LONG" | "INT" | "SHORT" | "BYTE") => neo4j(TypeName::Integer),
        Some("DOUBLE" | "FLOAT") => neo4j(TypeName::Float),
        Some("BOOLEAN") => neo4j(TypeName::Boolean),
        Some("DATE") => neo4j(TypeName::Date),
        Some("LOCALTIME") => neo4j(TypeName::LocalTime),
        Some("TIME") => neo4j(TypeName::ZonedTime),
        Some("LOCALDATETIME") => neo4j(TypeName::LocalDateTime),
        Some("DATETIME") => neo4j(TypeName::ZonedDateTime),
        Some("POINT") => neo4j(TypeName::Point),
        Some(_) => Err(bad()),
    }
}

/// Splits an array or `:LABEL` field at `delim`, `\` escaping it and
/// itself.
fn split_array(text: &str, delim: char) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in text.chars() {
        if esc {
            out.last_mut().expect("one").push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == delim {
            out.push(String::new());
        } else {
            out.last_mut().expect("one").push(c);
        }
    }
    out
}

/// A point of a Neo4j map `{k: v, …}` with the keys `x`, `y`, `z`,
/// `longitude`, `latitude`, `height`, `srid` and `crs`, the header's
/// options merged in, the value's winning (Neo4j's `PointValue`, GPL,
/// studied only: TS-26 point column).
fn neo4j_point(text: &str, header: &[(String, String)]) -> Option<Point> {
    let body = text.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut m: BTreeMap<String, String> = header
        .iter()
        .map(|(k, v)| {
            (
                k.to_ascii_lowercase(),
                v.trim_matches(['\'', '"']).to_owned(),
            )
        })
        .collect();
    for (k, v) in options(body) {
        m.insert(
            k.to_ascii_lowercase(),
            v.trim_matches(['\'', '"']).to_owned(),
        );
    }
    let num = |k: &str| -> Option<Option<f64>> {
        match m.get(k) {
            None => Some(None),
            Some(v) => Some(Some(match v.as_str() {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                v => v.parse().ok()?,
            })),
        }
    };
    let geo = m.contains_key("longitude") || m.contains_key("latitude");
    let x = num("x")?.or(num("longitude")?)?;
    let y = num("y")?.or(num("latitude")?)?;
    let z = num("z")?.or(num("height")?);
    let srid = match (m.get("srid"), m.get("crs")) {
        (Some(_), Some(_)) => return None,
        (Some(s), None) => Srid::from_code(s.parse().ok()?)?,
        (None, Some(c)) => match c.to_ascii_lowercase().as_str() {
            "cartesian" => Srid::Cartesian2D,
            "cartesian-3d" => Srid::Cartesian3D,
            "wgs-84" => Srid::Wgs84_2D,
            "wgs-84-3d" => Srid::Wgs84_3D,
            _ => return None,
        },
        (None, None) => match (geo, z.is_some()) {
            (false, false) => Srid::Cartesian2D,
            (false, true) => Srid::Cartesian3D,
            (true, false) => Srid::Wgs84_2D,
            (true, true) => Srid::Wgs84_3D,
        },
    };
    match z {
        Some(z) => Point::new(srid, &[x, y, z]),
        None => Point::new(srid, &[x, y]),
    }
}

/// The value of `text` in a Neo4j column of type `t`.
fn neo4j_value(t: &TypeName, text: &str, options: &[(String, String)]) -> Result<Value, Error> {
    let err = || lexical_error(t);
    Ok(match t {
        TypeName::String => Value::string(text),
        TypeName::Integer => Value::Int(text.parse().map_err(|_| err())?),
        TypeName::Float => Value::Float(match text {
            "NaN" => f64::NAN,
            "Infinity" | "+Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => text.parse().map_err(|_| err())?,
        }),
        TypeName::Boolean => {
            if text.eq_ignore_ascii_case("true") {
                Value::Bool(true)
            } else if text.eq_ignore_ascii_case("false") {
                Value::Bool(false)
            } else {
                return Err(err());
            }
        }
        TypeName::Point => match Point::parse(text).or_else(|| neo4j_point(text, options)) {
            Some(p) => Value::Point(Box::new(p)),
            None => return Err(err()),
        },
        t => typed_value(t, text)?,
    })
}

/// The value of a non-empty field `text` of a property column.
fn property_value(
    ty: &ColType,
    text: &str,
    options: &[(String, String)],
    adelim: char,
) -> Result<Value, Error> {
    match ty {
        ColType::Neo4j(t, false) => neo4j_value(t, text, options),
        ColType::Neo4j(t, true) => Ok(Value::List(
            split_array(text, adelim)
                .iter()
                .map(|e| neo4j_value(t, e, options))
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        )),
        ColType::Lpg(t) => typed_value(t, text),
        ColType::Annotated => annotated_value(text),
    }
}

/// The CSV files of an import request.
pub(crate) struct Input<'s> {
    storage: &'s dyn Storage,
    location: PathBuf,
    nodes: Vec<InputFile>,
    edges: Vec<InputFile>,
    delimiter: u8,
    array: char,
    ids: super::IdType,
}

impl<'s> Input<'s> {
    pub(crate) fn new(storage: &'s dyn Storage, req: &ImportRequest) -> Input<'s> {
        Input {
            storage,
            location: req.location.clone(),
            nodes: req.nodes.clone(),
            edges: req.edges.clone(),
            delimiter: req.delimiter,
            array: char::from(req.array_delimiter),
            ids: req.id_type,
        }
    }
}

/// Reads the CSV text of `r`, named `name`, with `delimiter`: its header,
/// then calls `f` with each record's line and fields, or an invalid record
/// for a record that is not UTF-8 or has the wrong number of fields.
///
/// # Errors
///
/// 22N05 for a malformed or missing header; I/O errors; those of `f`.
pub(crate) fn read_file(
    r: impl std::io::Read,
    name: &Arc<str>,
    delimiter: u8,
    f: &mut RecordFn<'_>,
) -> Result<(), Error> {
    let mut rd = ::csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(r);
    let mut rec = ::csv::ByteRecord::new();
    let more = rd.read_byte_record(&mut rec).map_err(csv_error)?;
    if !more {
        return Err(invalid_input("a CSV file without a header"));
    }
    let mut fields = Vec::with_capacity(rec.len());
    for raw in rec.iter() {
        let raw = std::str::from_utf8(raw)
            .map_err(|_| invalid_input("a CSV header that is not UTF-8"))?;
        fields.push(Field_(parse_field(raw)?));
    }
    loop {
        if !rd.read_byte_record(&mut rec).map_err(csv_error)? {
            return Ok(());
        }
        let line = rec.position().map_or(0, ::csv::Position::line);
        let bad = |why: &str| {
            Err(Invalid::new(
                name,
                Pos::Line(line),
                None,
                codes::C22N05,
                &format!("input failed validation: {why}"),
            ))
        };
        let item = if rec.len() != fields.len() {
            bad("a CSV record whose field count differs from its header")
        } else {
            rec.iter()
                .map(|b| std::str::from_utf8(b).map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()
                .or_else(|_| bad("a CSV record that is not UTF-8"))
        };
        f(&fields, line, item)?;
    }
}

/// The callback of [`read_file`]: the header, a record's line, and its
/// fields or an invalid record.
pub(crate) type RecordFn<'a> =
    dyn FnMut(&[Field_], u64, Result<Vec<String>, Invalid>) -> Result<(), Error> + 'a;

/// A parsed header field, opaque outside this module.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Field_(Field);

impl Input<'_> {
    fn open(&self, name: &str) -> Result<std::io::BufReader<Reader>, Error> {
        Reader::open(self.storage, &self.location, name)
    }
}

/// The properties of a record's `values` under header `fields`.
fn props(
    fields: &[Field_],
    values: &[String],
    adelim: char,
) -> Result<Vec<(String, Value)>, Error> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (f, v) in fields.iter().zip(values) {
        if let Field::Property { name, ty, options } = &f.0
            && !v.is_empty()
        {
            let value = property_value(ty, v, options, adelim)?;
            out.retain(|(n, _)| n != name);
            out.push((name.clone(), value));
        }
    }
    Ok(out)
}

/// Fails with 22N05 unless `fields` is the header of a file of the kind
/// asked: a node file has no `:START_ID`, `:END_ID` or `:TYPE`, an edge
/// file one `:START_ID` and one `:END_ID` and no `:ID` or `:LABEL`; no
/// special field or property name appears twice.
fn check_header(fields: &[Field_], edges: bool) -> Result<(), Error> {
    let count = |p: &dyn Fn(&Field) -> bool| fields.iter().filter(|f| p(&f.0)).count();
    let id = count(&|f| matches!(f, Field::Id(..)));
    let start = count(&|f| matches!(f, Field::StartId(_)));
    let end = count(&|f| matches!(f, Field::EndId(_)));
    let label = count(&|f| matches!(f, Field::Label));
    let ty = count(&|f| matches!(f, Field::Type));
    let ok = if edges {
        start == 1 && end == 1 && id == 0 && label == 0 && ty <= 1
    } else {
        start == 0 && end == 0 && ty == 0 && id <= 1 && label <= 1
    };
    let mut names: Vec<&str> = fields
        .iter()
        .filter_map(|f| match &f.0 {
            Field::Property { name, .. } => Some(name.as_str()),
            Field::Id(_, name) if !name.is_empty() => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    if ok && names.len() == n {
        Ok(())
    } else {
        Err(invalid_input(
            "a CSV header whose fields do not fit its file",
        ))
    }
}

/// How the identifiers of a CSV import read (TS-26 Named ID column).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Opts {
    /// The array and `:LABEL` delimiter.
    pub(crate) array: char,
    /// The type of `:ID`, `:START_ID` and `:END_ID` values.
    pub(crate) ids: super::IdType,
}

/// The identifier `v` of an `:ID`, `:START_ID` or `:END_ID` field under
/// `ids`: the text itself, or, for `integer`, the integer's decimal form,
/// so that `042` and `42` name one node, as Neo4j's `--id-type=integer`
/// compares numbers (rmp #59 DECISION 444; TS-26 Named ID column). The
/// value of a named column is returned with it.
fn id_value(v: &str, ids: super::IdType) -> Result<(String, Value), Error> {
    match ids {
        super::IdType::String => Ok((v.to_owned(), Value::string(v))),
        super::IdType::Integer => match v.parse::<i64>() {
            Ok(i) => Ok((i.to_string(), Value::Int(i))),
            Err(_) => Err(lexical_error(&TypeName::Integer)),
        },
    }
}

/// The node record of `values` on `line` of file `file`. A named `:ID`
/// column is also stored as a property of its name, typed by `id_type`
/// (FR-IMP-005, TS-26 Named ID column; Neo4j's import tool stores named
/// ID parts as properties, `CsvInputParser.java:202-210`, studied only).
pub(crate) fn node_record(
    file: &InputFile,
    name: &Arc<str>,
    fields: &[Field_],
    line: u64,
    values: &[String],
    o: Opts,
) -> Result<NodeIn, Invalid> {
    let mut id = None;
    let mut space = String::new();
    let mut labels = file.labels.clone();
    let mut named = None;
    let invalid = |id: Option<String>, e: Error| Invalid {
        file: name.clone(),
        pos: Pos::Line(line),
        id,
        error: Box::new(e),
    };
    for (f, v) in fields.iter().zip(values) {
        match &f.0 {
            Field::Id(s, prop) => {
                space.clone_from(s);
                if !v.is_empty() {
                    let (key, value) =
                        id_value(v, o.ids).map_err(|e| invalid(Some(v.clone()), e))?;
                    id = Some(key);
                    if !prop.is_empty() {
                        named = Some((prop.clone(), value));
                    }
                }
            }
            Field::Label if !v.is_empty() => labels.extend(split_array(v, o.array)),
            _ => {}
        }
    }
    let mut props = props(fields, values, o.array).map_err(|e| invalid(id.clone(), e))?;
    if let Some(p) = named {
        props.push(p);
    }
    Ok(NodeIn {
        file: name.clone(),
        pos: Pos::Line(line),
        id,
        space,
        labels,
        props,
    })
}

/// The edge record of `values` on `line` of file `file`.
pub(crate) fn edge_record(
    file: &InputFile,
    name: &Arc<str>,
    fields: &[Field_],
    line: u64,
    values: &[String],
    o: Opts,
) -> Result<EdgeIn, Invalid> {
    let invalid = |e: Error| Invalid {
        file: name.clone(),
        pos: Pos::Line(line),
        id: None,
        error: Box::new(e),
    };
    let mut start = (String::new(), String::new());
    let mut end = (String::new(), String::new());
    let mut ty = file.edge_type.clone();
    for (f, v) in fields.iter().zip(values) {
        match &f.0 {
            Field::StartId(s) => start = (s.clone(), id_value(v, o.ids).map_err(invalid)?.0),
            Field::EndId(s) => end = (s.clone(), id_value(v, o.ids).map_err(invalid)?.0),
            Field::Type if !v.is_empty() => ty = Some(v.clone()),
            _ => {}
        }
    }
    let Some(ty) = ty else {
        return Err(invalid(invalid_input("an edge without a type")));
    };
    let props = props(fields, values, o.array).map_err(invalid)?;
    Ok(EdgeIn {
        file: name.clone(),
        pos: Pos::Line(line),
        id: None,
        start,
        end,
        ty,
        props,
    })
}

impl Source for Input<'_> {
    fn nodes(&mut self, f: &mut Sink<'_, NodeIn>) -> Result<(), Error> {
        let files = self.nodes.clone();
        for file in &files {
            let name: Arc<str> = Arc::from(file.name.as_str());
            let r = self.open(&file.name)?;
            let adelim = Opts {
                array: self.array,
                ids: self.ids,
            };
            let mut checked = false;
            read_file(r, &name, self.delimiter, &mut |fields, line, item| {
                if !checked {
                    check_header(fields, false)?;
                    checked = true;
                }
                f(item.and_then(|values| node_record(file, &name, fields, line, &values, adelim)))
            })?;
        }
        Ok(())
    }

    fn edges(&mut self, f: &mut Sink<'_, EdgeIn>) -> Result<(), Error> {
        let files = self.edges.clone();
        for file in &files {
            let name: Arc<str> = Arc::from(file.name.as_str());
            let r = self.open(&file.name)?;
            let adelim = Opts {
                array: self.array,
                ids: self.ids,
            };
            let mut checked = false;
            read_file(r, &name, self.delimiter, &mut |fields, line, item| {
                if !checked {
                    check_header(fields, true)?;
                    checked = true;
                }
                f(item.and_then(|values| edge_record(file, &name, fields, line, &values, adelim)))
            })?;
        }
        Ok(())
    }
}

/// The options of the `csv_read` fuzz target.
#[cfg(any(test, fuzzing))]
const FUZZ_OPTS: Opts = Opts {
    array: ';',
    ids: super::IdType::String,
};

/// Reads `data` as a CSV node file, then as an edge file, as an import
/// does (the `csv_read` fuzz target): never panics.
#[cfg(any(test, fuzzing))]
pub(crate) fn read_all(data: &[u8]) -> u64 {
    let name: Arc<str> = Arc::from("fuzz");
    let file = InputFile::new("fuzz");
    let mut n = 0u64;
    for edges in [false, true] {
        let _ = read_file(data, &name, b',', &mut |fields, line, item| {
            check_header(fields, edges)?;
            let ok = match item {
                Ok(values) if edges => {
                    edge_record(&file, &name, fields, line, &values, FUZZ_OPTS).is_ok()
                }
                Ok(values) => node_record(&file, &name, fields, line, &values, FUZZ_OPTS).is_ok(),
                Err(_) => false,
            };
            n += u64::from(ok);
            Ok(())
        });
    }
    n
}
