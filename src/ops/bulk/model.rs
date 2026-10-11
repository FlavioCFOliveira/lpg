//! The record model every importer reads into (TS-26 Records), the report
//! of invalid records (TS-26 item C), the checks shared by both import
//! modes, and the memory charge of an import (TS-26 Memory).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;

use super::OnInvalid;
use super::files::{Outputs, io_error};
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::{GqlStatus, codes};
use crate::index::{Element, Entity, SchemaDef, SchemaKind, check_constraint, index_keys};
use crate::storage::Storage;
use crate::storage::engine::Engine;
use crate::storage::memory::Reservation;
use crate::sync::Arc;
use crate::types::Value;
use crate::types::lexical::is_property_value;

/// Where a record lies in its file, for the report (TS-26 Report): the
/// line of a CSV or JSON Lines record, the element ordinal of a GraphML
/// record, the block and record ordinals of a native dump record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pos {
    /// A line, from 1.
    Line(u64),
    /// An element ordinal, from 1.
    Ordinal(u64),
    /// A block ordinal and a record ordinal in it, from 1.
    Dump(u64, u64),
}

impl Pos {
    fn json(self) -> serde_json::Value {
        match self {
            Pos::Line(n) | Pos::Ordinal(n) => n.into(),
            Pos::Dump(b, r) => serde_json::Value::Array(vec![b.into(), r.into()]),
        }
    }
}

/// A node record (TS-26 Records): its input identifier in its id space
/// (none: no edge can name it), labels and properties.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeIn {
    /// The file it was read from.
    pub(crate) file: Arc<str>,
    /// Its place in the file.
    pub(crate) pos: Pos,
    /// Its input identifier.
    pub(crate) id: Option<String>,
    /// The id space of `id`; empty for the default space.
    pub(crate) space: String,
    /// Its labels.
    pub(crate) labels: Vec<String>,
    /// Its properties, by name.
    pub(crate) props: Vec<(String, Value)>,
}

/// An edge record: its input identifier (for the report only), its start
/// and end nodes by input identifier and id space, its type and its
/// properties.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeIn {
    /// The file it was read from.
    pub(crate) file: Arc<str>,
    /// Its place in the file.
    pub(crate) pos: Pos,
    /// Its input identifier, if the format has one.
    pub(crate) id: Option<String>,
    /// The id space and input identifier of its start node.
    pub(crate) start: (String, String),
    /// The id space and input identifier of its end node.
    pub(crate) end: (String, String),
    /// Its edge type.
    pub(crate) ty: String,
    /// Its properties, by name.
    pub(crate) props: Vec<(String, Value)>,
}

/// An invalid record (TS-26 Invalid records): where it is and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid {
    pub(crate) file: Arc<str>,
    pub(crate) pos: Pos,
    pub(crate) id: Option<String>,
    /// Its GQLSTATUS and reason, boxed: an invalid record is returned in
    /// the `Err` of every record reader, whose `Ok` it must not enlarge.
    pub(crate) error: Box<Error>,
}

impl Invalid {
    /// An invalid record at `pos` of `file` with status `status`.
    pub(crate) fn new(
        file: &Arc<str>,
        pos: Pos,
        id: Option<&str>,
        status: GqlStatus,
        reason: &str,
    ) -> Invalid {
        Invalid {
            file: file.clone(),
            pos,
            id: id.map(str::to_owned),
            error: Box::new(Error::new(status, reason)),
        }
    }
}

/// Labels and properties by name: a node's contents as an export or a
/// lexical form shows them.
pub(crate) type Labelled = (Vec<String>, Vec<(String, Value)>);

/// The callback a [`Source`] calls for each record: a record, or an
/// invalid one the reader found; an `Err` stops the reading.
pub(crate) type Sink<'a, T> = dyn FnMut(Result<T, Invalid>) -> Result<(), Error> + 'a;

/// The records of an import, nodes first, then edges (TS-26 Records). A
/// format that allows edges before nodes reads its input twice (GraphML).
pub(crate) trait Source {
    /// Calls `f` with every node record, in file order.
    ///
    /// # Errors
    ///
    /// A malformed header or a damaged native dump (TS-26: not records),
    /// I/O failures, and the errors of `f`.
    fn nodes(&mut self, f: &mut Sink<'_, NodeIn>) -> Result<(), Error>;

    /// Calls `f` with every edge record, in file order.
    ///
    /// # Errors
    ///
    /// As [`Source::nodes`].
    fn edges(&mut self, f: &mut Sink<'_, EdgeIn>) -> Result<(), Error>;
}

/// The report file of an import (TS-26 item C): one JSON object per
/// invalid record, keys `file`, `record`, `id`, `gqlstatus`, `reason`,
/// written as an export file.
pub(crate) struct Report<'s> {
    out: Outputs<'s>,
    file: usize,
    on_invalid: OnInvalid,
    /// Invalid records so far.
    pub(crate) count: u64,
}

impl<'s> Report<'s> {
    /// Creates the report `name` in `location`.
    ///
    /// # Errors
    ///
    /// 22N11 if it exists; 50N00.
    pub(crate) fn create(
        storage: &'s dyn Storage,
        location: &Path,
        name: &str,
        on_invalid: OnInvalid,
    ) -> Result<Report<'s>, Error> {
        let mut out = Outputs::new(storage, location, &[name])?;
        let file = out.create(name)?;
        Ok(Report {
            out,
            file,
            on_invalid,
            count: 0,
        })
    }

    /// Whether the import stops at its first invalid record.
    pub(crate) fn aborts(&self) -> bool {
        self.on_invalid == OnInvalid::Abort
    }

    /// Reports `inv` and counts it; with abort, returns its error, which
    /// stops the import (FR-IMP-004).
    ///
    /// # Errors
    ///
    /// The record's error under abort; 50N00 if the line cannot be
    /// written.
    pub(crate) fn invalid(&mut self, inv: Invalid) -> Result<(), Error> {
        self.count += 1;
        let line = serde_json::json!({
            "file": &*inv.file,
            "record": inv.pos.json(),
            "id": inv.id,
            "gqlstatus": inv.error.code(),
            "reason": inv.error.message(),
        });
        let w = self.out.writer(self.file);
        serde_json::to_writer(&mut *w, &line).map_err(|e| io_error(e.into()))?;
        w.write_all(b"\n").map_err(io_error)?;
        match self.on_invalid {
            OnInvalid::Abort => Err(*inv.error),
            OnInvalid::Skip => Ok(()),
        }
    }

    /// Commits the report file.
    ///
    /// # Errors
    ///
    /// 50N00.
    pub(crate) fn commit(self) -> Result<(), Error> {
        self.out.commit()
    }
}

/// Memory an import charges to the cap: its identifier map, and the sets
/// of values and identifiers it checks (TS-26 Memory). Released when
/// dropped.
pub(crate) struct Charge {
    engine: Engine,
    held: Option<Reservation>,
}

/// Bytes charged per map entry besides its key: a hash-map slot with its
/// value and allocation overhead, a fixed estimate as the write set's
/// `WRITE_OVERHEAD` is.
pub(crate) const ENTRY_OVERHEAD: u64 = 48;

impl Charge {
    pub(crate) fn new(engine: &Engine) -> Charge {
        Charge {
            engine: Engine::handle(engine.core_arc()),
            held: None,
        }
    }

    /// Reserves `bytes` more.
    ///
    /// # Errors
    ///
    /// 51N72 when the cap cannot hold them.
    pub(crate) fn add(&mut self, bytes: u64) -> Result<(), Error> {
        let r = self.engine.core().pool.reserve(bytes)?;
        match &mut self.held {
            Some(h) => h.absorb(r),
            None => self.held = Some(r),
        }
        Ok(())
    }
}

/// Token identifiers of names, created at once when missing by an inner
/// commit of their own (TS-17 Tokens; TS-26 Records: tokens left by an
/// aborted import stay unused).
pub(crate) struct Tokens {
    engine: Engine,
    cache: HashMap<(TokenKind, String), u64>,
}

impl Tokens {
    pub(crate) fn new(engine: &Engine) -> Tokens {
        Tokens {
            engine: Engine::handle(engine.core_arc()),
            cache: HashMap::new(),
        }
    }

    /// The token of `(kind, name)`.
    ///
    /// # Errors
    ///
    /// 22N11 for a name too long for a token; commit errors.
    pub(crate) fn get(&mut self, kind: TokenKind, name: &str) -> Result<u64, Error> {
        if let Some(&id) = self.cache.get(&(kind, name.to_owned())) {
            return Ok(id);
        }
        let id = self.engine.token(kind, name)?;
        self.cache.insert((kind, name.to_owned()), id);
        Ok(id)
    }
}

/// An element as the stores and the constraint checks take it: tokens
/// (sorted labels, or the edge type) and properties sorted by key token.
pub(crate) struct Resolved {
    pub(crate) tokens: Vec<u64>,
    pub(crate) props: Vec<(u64, Value)>,
}

impl Resolved {
    pub(crate) fn element(&self) -> Element {
        Element {
            tokens: self.tokens.clone(),
            props: self.props.clone(),
        }
    }
}

/// Resolves the labels or type `names` (`kind`) and the properties
/// `props` of a record to tokens. A property value that a property cannot
/// hold (IV011) is an invalid record with 22G03; a name too long for a
/// token one with the token's error.
///
/// # Errors
///
/// The record's reason, to be wrapped into an [`Invalid`]; commit errors
/// of the token creation are returned as they are, with `true`.
pub(crate) fn resolve(
    tokens: &mut Tokens,
    kind: TokenKind,
    names: &[String],
    props: &[(String, Value)],
) -> Result<Resolved, (Error, bool)> {
    let mut t = Vec::with_capacity(names.len());
    for n in names {
        t.push(tokens.get(kind, n).map_err(fatal_or_record)?);
    }
    t.sort_unstable();
    t.dedup();
    let mut p = Vec::with_capacity(props.len());
    for (name, v) in props {
        if !is_property_value(v) {
            return Err((
                Error::new(
                    codes::C22G03,
                    "invalid value type: a property cannot hold this value",
                ),
                false,
            ));
        }
        let k = tokens
            .get(TokenKind::PropertyKey, name)
            .map_err(fatal_or_record)?;
        p.retain(|(x, _): &(u64, Value)| *x != k);
        p.push((k, v.clone()));
    }
    p.sort_by_key(|(k, _)| *k);
    Ok(Resolved {
        tokens: t,
        props: p,
    })
}

/// A token error: a name too long (22N11) is the record's fault; any other
/// failure stops the import.
fn fatal_or_record(e: Error) -> (Error, bool) {
    let fatal = e.code() != "22N11";
    (e, fatal)
}

/// The constraints of a graph that an import checks, from the latest
/// committed catalog (TS-18): existence, type and node-key existence per
/// record ([`Constraints::check`]), uniqueness by the owned index's keys
/// ([`Constraints::unique_keys`]).
pub(crate) struct Constraints {
    defs: Vec<SchemaDef>,
    /// Owned indexes, by the constraint that owns them.
    owned: HashMap<u64, SchemaDef>,
}

impl Constraints {
    /// The constraints of graph `graph`.
    pub(crate) fn of(engine: &Engine, graph: u64) -> Constraints {
        let cache = engine.core().catalog.schema();
        let defs: Vec<SchemaDef> = cache
            .values()
            .filter(|d| d.graph == graph && !d.kind.is_index())
            .cloned()
            .collect();
        let owned = defs
            .iter()
            .filter(|d| d.kind.owns_index())
            .filter_map(|d| cache.get(&d.link).map(|ix| (d.id, ix.clone())))
            .collect();
        Constraints { defs, owned }
    }

    /// Fails with the constraint violation of `el`, an element of
    /// `entity`, against every existence, type and node-key existence
    /// constraint (TS-18).
    ///
    /// # Errors
    ///
    /// 22N77 or 22N78.
    pub(crate) fn check(&self, entity: Entity, el: &Element) -> Result<(), Error> {
        for d in &self.defs {
            if d.entity == entity && el.tokens.contains(&d.token) {
                check_constraint(d, el)?;
            }
        }
        Ok(())
    }

    /// The owned-index keys of element `id`, `el`, under each uniqueness
    /// and node-key constraint of `entity`: two elements violate one when
    /// their keys share the prefix without the element
    /// ([`crate::index::prefix_of`]).
    pub(crate) fn unique_keys(&self, entity: Entity, id: u64, el: &Element) -> Vec<(u64, Vec<u8>)> {
        let mut out = Vec::new();
        for d in &self.defs {
            if d.entity == entity
                && matches!(d.kind, SchemaKind::Unique | SchemaKind::NodeKey)
                && let Some(ix) = self.owned.get(&d.id)
            {
                for k in index_keys(ix, id, el) {
                    out.push((d.id, k));
                }
            }
        }
        out
    }

    /// Whether any uniqueness or node-key constraint applies to `entity`.
    pub(crate) fn has_unique(&self, entity: Entity) -> bool {
        self.defs
            .iter()
            .any(|d| d.entity == entity && d.kind.owns_index())
    }
}

/// The reason of a duplicate input identifier (TS-26 Invalid records).
pub(crate) const DUPLICATE_ID: &str = "a duplicate input identifier in its id space";
/// The reason of an edge whose endpoint no node has.
pub(crate) const UNKNOWN_ENDPOINT: &str = "an edge endpoint that no node of the input has";
/// The reason of an edge whose endpoint was skipped.
pub(crate) const SKIPPED_ENDPOINT: &str = "an edge endpoint whose node record was skipped";

/// The error of a text that is not a lexical form of its type (TS-26
/// GQLSTATUS table): 22007 for a temporal instant, 22G0H for a duration,
/// 22018 for any other type.
pub(crate) fn lexical_error(t: &crate::types::lexical::TypeName) -> Error {
    use crate::types::lexical::TypeName as T;
    match t {
        T::Date | T::LocalTime | T::ZonedTime | T::LocalDateTime | T::ZonedDateTime => Error::new(
            codes::C22007,
            "invalid datetime format: a text that is not a lexical form of its type",
        ),
        T::YearToMonth | T::DayToSecond => Error::new(
            codes::C22G0H,
            "invalid duration format: a text that is not a lexical form of its type",
        ),
        _ => Error::new(
            codes::C22018,
            "invalid character value for cast: a text that is not a lexical form of its type",
        ),
    }
}

/// Reads `<type name>:<lexical form>` (an `lpg:annotated` CSV field):
/// 22N05 for an unknown type name, else the error of
/// [`lexical_error`].
pub(crate) fn annotated_value(text: &str) -> Result<Value, Error> {
    use crate::types::lexical::{TypeName, parse};
    let (t, rest) = text
        .split_once(':')
        .ok_or_else(|| super::invalid_input("an annotated value without a type name"))?;
    let t = TypeName::parse(t).ok_or_else(|| super::invalid_input("an unknown type name"))?;
    parse(&t, rest).ok_or_else(|| lexical_error(&t))
}

/// Reads the lexical form `text` of type `t`, with the error of
/// [`lexical_error`].
pub(crate) fn typed_value(t: &crate::types::lexical::TypeName, text: &str) -> Result<Value, Error> {
    crate::types::lexical::parse(t, text).ok_or_else(|| lexical_error(t))
}
