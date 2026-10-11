//! Indexes and constraints (TS-18; FR-IDX-001 to FR-IDX-006,
//! FR-TYPE-005).
//!
//! **Definitions** are catalog entries keyed by `(graph_id, object_id)`
//! (kind byte [`SCHEMA`], TS-17), stamped like every catalog entry, so they
//! commit and roll back with the transaction that writes them. The latest
//! committed ones are kept in the catalog cache ([`crate::catalog::Catalog::schema`]),
//! loaded at open and changed when each commit is applied, under the commit
//! latch. A definition ([`SchemaDef`]) is an index of a kind of
//! [`SchemaKind::is_index`] in state POPULATING or ONLINE, or a constraint.
//!
//! **Index keys** ([`index_keys`]), in one B+tree per index, of stamped
//! entries with empty values (TS-18 table), with the key encoding of
//! [`crate::types::key`]:
//!
//! | Kind | Key |
//! |---|---|
//! | label or edge-type lookup | token, element (8 bytes big-endian each), one per label of a node |
//! | range, one property or composite | index id (8, BE), the values' encodings, element |
//! | point | index id, SRID (4, BE), Hilbert key (8, BE), element |
//!
//! A range index covers an element with its label or edge type and every
//! property present and not null, as Neo4j's range indexes do; a point
//! index covers an element whose property is a point, keyed by its
//! [`crate::types::spatial::canonical`] point. The distinct-key count of
//! an index counts the keys without their element ([`prefix_of`]).
//!
//! **Maintenance** ([`derive`], TS-18 step 2). No index entry is written at
//! write time: under the commit latch, before the write keys are registered
//! (TS-16 step 5), the committer derives, from each record its write set
//! changes and that record's latest committed version, the keys every index
//! of the catalog cache loses and gains. For an ONLINE index they are
//! stamped deletes and puts of this commit, with their write keys, so a
//! serializable reader of an index range conflicts with them (TS-15). For a
//! POPULATING index they are records of its side tree ([`side`]). The
//! write-write rule makes the replaced record versions the latest committed
//! ones, and the index keys end with the element, so two transactions never
//! write one index key.
//!
//! **Reads** ([`Transaction::index_scan`], [`Transaction::index_within`])
//! use an index only from the snapshot that sees its definition ONLINE
//! (TS-18), read its entries at the snapshot, record the ranges they read
//! (TS-15 Read sets), and apply the transaction's own record changes.
//!
//! **Constraints** (TS-18 Constraints). Uniqueness and node key own a range
//! index over their properties, created ONLINE with them. A writer of an
//! element with a constrained value takes the X lock on `(constraint,
//! value)` and fails at once if the owned index holds the value for another
//! element ([`Transaction::unique_values`]); the commit checks again under
//! the latch, as Turso checks unique index keys at commit
//! ([`unique_violation`]). Property existence, property type and node key
//! existence are checked over the write set at commit
//! ([`check_constraint`]). A node or edge missing one of the properties of
//! a uniqueness or property type constraint is not subject to it, as the
//! Neo4j Cypher Manual states for both ("Nodes or relationships missing
//! one or more of the specified properties are not subject to this rule";
//! "Nodes or relationships without the constrained property are not
//! subject to this rule", <https://neo4j.com/docs/cypher-manual/current/schema/constraints/create-constraints/>,
//! read 2026-10-08). Creation is in [`build`].

pub(crate) mod build;
pub(crate) mod side;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashSet};

pub(crate) use build::{Builds, restart_builds};

use crate::catalog::stats::{Delta, StatKey};
use crate::catalog::{CATALOG_TREE, Kind, name_key};
use crate::storage::btree::{MAX_KEY, Store, Tree};
use crate::storage::engine::{Core, Engine};
use crate::storage::error::{Error, Missing, Result, Violation};
use crate::storage::graph::{EdgeRecord, Graph, decode_edge, element_key, element_of};
use crate::tx::Transaction;
use crate::tx::lock::{Mode, Resource};
use crate::types::spatial::{self, Point};
use crate::types::{Value, key, record};

/// The kind byte of definition entries in the catalog tree (TS-17).
pub(crate) const SCHEMA: u8 = 5;

/// The most properties of one index or constraint.
pub(crate) const MAX_PROPERTIES: usize = 16;

/// What an index or constraint is defined on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Entity {
    /// Nodes, by label.
    Node = 0,
    /// Edges, by edge type.
    Edge = 1,
}

/// A property type of a property type constraint: the Neo4j Cypher
/// Manual's allowed types ("Create constraints", "Allowed types",
/// <https://neo4j.com/docs/cypher-manual/current/schema/constraints/create-constraints/>,
/// read 2026-10-08) that lpg's values have, and `BYTES`. `DURATION` covers
/// both of lpg's durations. Neo4j's `UUID` and `VECTOR` have no lpg value
/// and are not offered; `MAP` is not an allowed type there either.
/// `BYTES` (and so `LIST<BYTES NOT NULL>`) is an lpg extension: Neo4j has
/// no byte-string property type, lpg has byte-string values (TS-21; user
/// decision 2026-10-08, rmp #34).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PropertyType {
    Boolean = 0,
    Integer = 1,
    Float = 2,
    String = 3,
    Date = 4,
    LocalTime = 5,
    LocalDateTime = 6,
    ZonedTime = 7,
    ZonedDateTime = 8,
    Duration = 9,
    Point = 10,
    /// lpg extension: a byte string.
    Bytes = 11,
}

impl PropertyType {
    /// The type of `v`; `None` for a value no property type names.
    pub(crate) fn of(v: &Value) -> Option<PropertyType> {
        Some(match v {
            Value::Bool(_) => PropertyType::Boolean,
            Value::Int(_) => PropertyType::Integer,
            Value::Float(_) => PropertyType::Float,
            Value::String(_) => PropertyType::String,
            Value::Date(_) => PropertyType::Date,
            Value::LocalTime(_) => PropertyType::LocalTime,
            Value::LocalDateTime(_) => PropertyType::LocalDateTime,
            Value::ZonedTime(_) => PropertyType::ZonedTime,
            Value::ZonedDateTime(_) => PropertyType::ZonedDateTime,
            Value::YearToMonth(_) | Value::DayToSecond(_) => PropertyType::Duration,
            Value::Point(_) => PropertyType::Point,
            Value::Bytes(_) => PropertyType::Bytes,
            _ => return None,
        })
    }
}

/// The types a property type constraint accepts: a closed union of
/// property types `T` and typed lists `LIST<T NOT NULL>` (the same page:
/// "Any closed dynamic union of the above types, e.g. `INTEGER | FLOAT |
/// STRING`"). Bit `t` stands for `T`, bit `16 + t` for `LIST<T NOT NULL>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TypeSet(u32);

/// The bit of `LIST<T NOT NULL>` above the bit of `T`.
const LIST_SHIFT: u32 = 16;

/// Every bit a [`TypeSet`] may hold.
const TYPE_BITS: u32 = ((1 << 12) - 1) | (((1 << 12) - 1) << LIST_SHIFT);

impl TypeSet {
    /// The set of `T` alone.
    pub(crate) fn of(t: PropertyType) -> TypeSet {
        TypeSet(1 << t as u32)
    }

    /// The set of `LIST<T NOT NULL>` alone.
    pub(crate) fn list(t: PropertyType) -> TypeSet {
        TypeSet(1 << (LIST_SHIFT + t as u32))
    }

    /// The union of both sets.
    #[must_use]
    pub(crate) fn or(self, other: TypeSet) -> TypeSet {
        TypeSet(self.0 | other.0)
    }

    /// The set of `bits`; `None` if empty or with a bit no type has.
    fn from_bits(bits: u64) -> Option<TypeSet> {
        let b = u32::try_from(bits).ok()?;
        (b != 0 && b & !TYPE_BITS == 0).then_some(TypeSet(b))
    }

    /// Whether `v` has one of the set's types. A list has `LIST<T NOT
    /// NULL>` when every element has type `T` and none is null; an empty
    /// list has every list type, as the Cypher Manual's type predicates
    /// state ("An empty list will match on all inner types",
    /// <https://neo4j.com/docs/cypher-manual/current/expressions/predicates/type-predicate-expressions/>,
    /// read 2026-10-08).
    pub(crate) fn accepts(self, v: &Value) -> bool {
        match v {
            Value::List(items) => {
                let lists = self.0 >> LIST_SHIFT;
                match items.first() {
                    None => lists != 0,
                    Some(first) => PropertyType::of(first).is_some_and(|t| {
                        lists & (1 << t as u32) != 0
                            && items.iter().all(|x| PropertyType::of(x) == Some(t))
                    }),
                }
            }
            _ => PropertyType::of(v).is_some_and(|t| self.0 & (1 << t as u32) != 0),
        }
    }
}

/// The kind of an index or constraint (FR-IDX-001, FR-IDX-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SchemaKind {
    /// A label or edge-type lookup index: every element by its tokens.
    Lookup,
    /// A range index on one property, or on several (composite).
    Range,
    /// A point index on one property.
    Point,
    /// A uniqueness constraint.
    Unique,
    /// A property existence constraint.
    Exists,
    /// A property type constraint.
    Type(TypeSet),
    /// A node key constraint: existence and uniqueness.
    NodeKey,
}

impl SchemaKind {
    /// Whether this is an index kind.
    pub(crate) fn is_index(self) -> bool {
        matches!(
            self,
            SchemaKind::Lookup | SchemaKind::Range | SchemaKind::Point
        )
    }

    /// Whether this constraint owns a range index.
    pub(crate) fn owns_index(self) -> bool {
        matches!(self, SchemaKind::Unique | SchemaKind::NodeKey)
    }

    fn bytes(self) -> [u8; 2] {
        match self {
            SchemaKind::Lookup => [1, 0],
            SchemaKind::Range => [2, 0],
            SchemaKind::Point => [3, 0],
            SchemaKind::Unique => [4, 0],
            SchemaKind::Exists => [5, 0],
            SchemaKind::Type(_) => [6, 0],
            SchemaKind::NodeKey => [7, 0],
        }
    }

    /// The kind of bytes `a`, `b`; a property type constraint's types are
    /// `types`.
    fn from_bytes(a: u8, b: u8, types: u64) -> Option<SchemaKind> {
        Some(match (a, b) {
            (1, 0) => SchemaKind::Lookup,
            (2, 0) => SchemaKind::Range,
            (3, 0) => SchemaKind::Point,
            (4, 0) => SchemaKind::Unique,
            (5, 0) => SchemaKind::Exists,
            (6, 0) => SchemaKind::Type(TypeSet::from_bits(types)?),
            (7, 0) => SchemaKind::NodeKey,
            _ => return None,
        })
    }
}

/// The state of an index (TS-18 Online build).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Being built: committers write its changes to its side tree.
    Populating = 0,
    /// Built: committers maintain it, readers may use it.
    Online = 1,
}

/// An index or constraint definition (module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaDef {
    /// Its catalog object identifier.
    pub(crate) id: u64,
    /// The graph it is defined on (FR-IDX-003).
    pub(crate) graph: u64,
    /// Its kind.
    pub(crate) kind: SchemaKind,
    /// Nodes or edges.
    pub(crate) entity: Entity,
    /// The label or edge type; 0 for a lookup index.
    pub(crate) token: u64,
    /// The property keys, in order.
    pub(crate) props: Vec<u64>,
    /// An index's state; ONLINE for a constraint.
    pub(crate) state: State,
    /// An index's tree; 0 for a constraint.
    pub(crate) tree: u64,
    /// A POPULATING index's side tree; 0 otherwise.
    pub(crate) side: u64,
    /// An index's owning constraint, a constraint's owned index; 0 for
    /// none.
    pub(crate) link: u64,
}

/// The catalog key of definition `id` of `graph`.
pub(crate) fn schema_key(graph: u64, id: u64) -> [u8; 17] {
    let mut k = [0u8; 17];
    k[0] = SCHEMA;
    k[1..9].copy_from_slice(&graph.to_be_bytes());
    k[9..].copy_from_slice(&id.to_be_bytes());
    k
}

fn graph_prefix(graph: u64) -> [u8; 9] {
    let mut k = [0u8; 9];
    k[0] = SCHEMA;
    k[1..].copy_from_slice(&graph.to_be_bytes());
    k
}

/// Bytes of a definition entry's value before its property keys
/// ([`SchemaDef::encode`]).
const HEADER: usize = 44;

fn le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

impl SchemaDef {
    /// The definition entry's value: kind (2 bytes), entity, state, then
    /// token, tree, side tree, link and the types of a property type
    /// constraint ([`TypeSet`], 0 otherwise) (8 bytes little-endian each),
    /// then the property keys (8 bytes each).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(HEADER + 8 * self.props.len());
        v.extend_from_slice(&self.kind.bytes());
        v.push(self.entity as u8);
        v.push(self.state as u8);
        let types = match self.kind {
            SchemaKind::Type(t) => u64::from(t.0),
            _ => 0,
        };
        for x in [self.token, self.tree, self.side, self.link, types] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        for p in &self.props {
            v.extend_from_slice(&p.to_le_bytes());
        }
        v
    }

    /// The definition of entry `key` → `value`; `None` if malformed.
    pub(crate) fn decode(key: &[u8], v: &[u8]) -> Option<SchemaDef> {
        if key.len() != 17
            || key[0] != SCHEMA
            || v.len() < HEADER
            || !(v.len() - HEADER).is_multiple_of(8)
        {
            return None;
        }
        let be = |at: usize| u64::from_be_bytes(key[at..at + 8].try_into().expect("8 bytes"));
        Some(SchemaDef {
            graph: be(1),
            id: be(9),
            kind: SchemaKind::from_bytes(v[0], v[1], le(v, 36)?)?,
            entity: match v[2] {
                0 => Entity::Node,
                1 => Entity::Edge,
                _ => return None,
            },
            state: match v[3] {
                0 => State::Populating,
                1 => State::Online,
                _ => return None,
            },
            token: le(v, 4)?,
            tree: le(v, 12)?,
            side: le(v, 20)?,
            link: le(v, 28)?,
            props: (HEADER..v.len())
                .step_by(8)
                .map(|at| le(v, at))
                .collect::<Option<_>>()?,
        })
    }

    /// The bytes every key of this index starts with: its identifier,
    /// none for a lookup index.
    fn key_prefix(&self) -> Vec<u8> {
        match self.kind {
            SchemaKind::Lookup => Vec::new(),
            _ => self.id.to_be_bytes().to_vec(),
        }
    }
}

// ---------------------------------------------------------------------------
// Keys.

/// An element's tokens (its labels, or its edge type) and properties.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Element {
    /// The labels of a node; the edge type of an edge.
    pub(crate) tokens: Vec<u64>,
    /// The properties.
    pub(crate) props: Vec<(u64, Value)>,
}

/// The element of record `v` in the node or edge tree; `None` if
/// malformed.
pub(crate) fn decode_element(entity: Entity, v: &[u8]) -> Option<Element> {
    match entity {
        Entity::Node => {
            let (tokens, props) = record::decode(v)?;
            Some(Element { tokens, props })
        }
        Entity::Edge => {
            let EdgeRecord { ty, props, .. } = decode_edge(v)?;
            Some(Element {
                tokens: vec![ty],
                props,
            })
        }
    }
}

/// The value of property `key`, absent if null.
fn prop(props: &[(u64, Value)], key: u64) -> Option<&Value> {
    props
        .iter()
        .find(|(k, v)| *k == key && *v != Value::Null)
        .map(|(_, v)| v)
}

/// The key encoding of `def`'s properties of `el`, if `el` has `def`'s
/// token and every property: the value a uniqueness constraint or a range
/// index covers.
fn values(def: &SchemaDef, el: &Element) -> Option<Vec<u8>> {
    if !el.tokens.contains(&def.token) {
        return None;
    }
    let mut out = Vec::new();
    for &p in &def.props {
        key::encode(prop(&el.props, p)?, &mut out);
    }
    Some(out)
}

/// The keys of element `id`, `el`, in index `def` (module documentation).
pub(crate) fn index_keys(def: &SchemaDef, id: u64, el: &Element) -> Vec<Vec<u8>> {
    let tail = id.to_be_bytes();
    match def.kind {
        SchemaKind::Lookup => el
            .tokens
            .iter()
            .map(|t| [t.to_be_bytes(), tail].concat())
            .collect(),
        SchemaKind::Range => values(def, el)
            .map(|v| vec![[&def.id.to_be_bytes()[..], &v, &tail].concat()])
            .unwrap_or_default(),
        SchemaKind::Point => {
            if !el.tokens.contains(&def.token) {
                return Vec::new();
            }
            match def.props.first().and_then(|&p| prop(&el.props, p)) {
                Some(Value::Point(p)) => vec![point_key(def.id, p, id)],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn point_key(index: u64, p: &Point, id: u64) -> Vec<u8> {
    let c = spatial::canonical(p);
    let mut k = Vec::with_capacity(28);
    k.extend_from_slice(&index.to_be_bytes());
    k.extend_from_slice(&c.srid().code().to_be_bytes());
    k.extend_from_slice(&spatial::key(&c).to_be_bytes());
    k.extend_from_slice(&id.to_be_bytes());
    k
}

/// An index key without its element: the key whose distinct values the
/// distinct-key count counts.
pub(crate) fn prefix_of(key: &[u8]) -> &[u8] {
    &key[..key.len().saturating_sub(8)]
}

/// The element of an index key.
pub(crate) fn element_of_key(key: &[u8]) -> Option<u64> {
    element_of(key.get(key.len().checked_sub(8)?..)?)
}

/// Whether `tree` holds, in its latest committed state, a live key that
/// starts with `prefix`, has an element after it, and is not in `skip`.
fn live_with_prefix(
    store: &Store<'_>,
    tree: Tree,
    prefix: &[u8],
    skip: &HashSet<Vec<u8>>,
) -> Result<bool> {
    let mut found = false;
    store.scan_prefix_stamped(tree, prefix, |k, _, st| {
        found = st.deleted == 0 && k.len() == prefix.len() + 8 && !skip.contains(k);
        !found
    })?;
    Ok(found)
}

fn tree_of(store: &Store<'_>, id: u64) -> Result<Tree> {
    store
        .tree(id)?
        .ok_or_else(|| Error::Corruption(format!("index tree {id} is missing")))
}

/// The element tree of `entity` of graph `graph` in the latest committed
/// catalog.
pub(crate) fn element_tree(core: &Core, graph: u64, entity: Entity) -> Result<Option<Tree>> {
    let d = match core
        .catalog
        .objects()
        .get(&graph)
        .map(|o| o.definition.clone())
    {
        Some(crate::catalog::Definition::Graph(d)) => d,
        _ => return Ok(None),
    };
    core.store().tree(match entity {
        Entity::Node => d.nodes,
        Entity::Edge => d.edges,
    })
}

// ---------------------------------------------------------------------------
// Commit-time maintenance.

/// A write that a commit derives from its record changes.
#[derive(Debug)]
pub(crate) struct DerivedWrite {
    /// The index or side tree.
    pub(crate) tree: Tree,
    /// The key.
    pub(crate) key: Vec<u8>,
    /// The value to put, or `None` to delete.
    pub(crate) value: Option<Vec<u8>>,
    /// The commit timestamp it stamps, or 0 for a frozen write.
    pub(crate) stamp: u64,
}

/// The index and side-tree writes, write keys and statistics a commit
/// derives ([`derive`]).
#[derive(Debug, Default)]
pub(crate) struct Derived {
    /// The writes, applied with the commit's batch.
    pub(crate) writes: Vec<DerivedWrite>,
    /// The write keys of the stamped index writes (TS-15).
    pub(crate) keys: Vec<(u64, Box<[u8]>)>,
    /// The index statistics changes (TS-17).
    pub(crate) stats: Delta,
}

/// The ONLINE switch of an index build (TS-18 step 6), carried by the
/// transaction that commits it ([`Transaction::online`]).
#[derive(Debug, Clone)]
pub(crate) struct Online {
    /// The index.
    pub(crate) index: u64,
    /// The last side-tree key the builder replayed; empty for none.
    pub(crate) from: Vec<u8>,
    /// The index's entries after the builder's replay.
    pub(crate) entries: u64,
    /// Its distinct keys after the builder's replay.
    pub(crate) distinct: u64,
}

fn stat(graph: u64, index: u64, distinct: bool) -> StatKey {
    StatKey::Index {
        graph,
        index,
        distinct,
    }
}

/// The keys an index loses and gains in one commit.
#[derive(Debug, Default)]
struct Change {
    added: BTreeSet<Vec<u8>>,
    removed: HashSet<Vec<u8>>,
}

/// The statistics change of index `def` in `tree` when its latest
/// committed state loses `removed` and gains `added` (TS-17).
fn index_stats(
    store: &Store<'_>,
    def: &SchemaDef,
    tree: Tree,
    added: &BTreeSet<Vec<u8>>,
    removed: &HashSet<Vec<u8>>,
    d: &mut Delta,
) -> Result<()> {
    d.add(
        stat(def.graph, def.id, false),
        added.len() as i64 - removed.len() as i64,
    );
    let prefixes: BTreeSet<&[u8]> = added
        .iter()
        .chain(removed.iter())
        .map(|k| prefix_of(k))
        .collect();
    let none = HashSet::new();
    for p in prefixes {
        let before = live_with_prefix(store, tree, p, &none)?;
        let after =
            added.iter().any(|k| prefix_of(k) == p) || live_with_prefix(store, tree, p, removed)?;
        d.add(
            stat(def.graph, def.id, true),
            i64::from(after) - i64::from(before),
        );
    }
    Ok(())
}

/// The latest committed version of record `key` of `tree`, live.
fn committed_element(
    store: &Store<'_>,
    tree: Tree,
    entity: Entity,
    key: &[u8],
    buf: &mut Vec<u8>,
) -> Result<Option<Element>> {
    match store.get_stamped(tree, key, buf)? {
        Some(st) if st.deleted == 0 => decode_element(entity, buf)
            .map(Some)
            .ok_or_else(|| Error::Corruption("malformed record".into())),
        _ => Ok(None),
    }
}

/// Fails with [`Error::ConstraintViolation`] if element `el`, which has
/// the constraint's token, violates existence, type or node-key existence
/// constraint `def`.
pub(crate) fn check_constraint(def: &SchemaDef, el: &Element) -> Result<()> {
    match def.kind {
        SchemaKind::Exists | SchemaKind::NodeKey => {
            if def.props.iter().any(|&p| prop(&el.props, p).is_none()) {
                return Err(Error::ConstraintViolation(Violation::MissingProperty));
            }
        }
        SchemaKind::Type(t) => {
            if let Some(v) = prop(&el.props, def.props[0])
                && !t.accepts(v)
            {
                return Err(Error::ConstraintViolation(Violation::PropertyType));
            }
        }
        _ => {}
    }
    Ok(())
}

/// The uniqueness check at commit (TS-18: "checked again when the commit
/// is applied"), under the commit latch: fails if the commit adds to an
/// owned index two keys with one value, or one whose value a live entry
/// of the index's latest committed state, not removed by this commit,
/// holds.
///
/// A port of Turso's `check_index_for_conflicts` (R-TURSO @ `8cc059b`
/// `core/mvcc/database/mod.rs:2129-2190`, MIT licence, "Copyright 2024 the
/// Turso authors"; notice in `src/sync.rs`): a prefix scan of the unique
/// index over the indexed values, the entries without the row identifier,
/// for an entry of another row, with the latest committed state in place
/// of Turso's version chains.
fn unique_violation(store: &Store<'_>, tree: Tree, change: &Change) -> Result<bool> {
    let mut seen: HashSet<&[u8]> = HashSet::new();
    for k in &change.added {
        let p = prefix_of(k);
        if !seen.insert(p) || live_with_prefix(store, tree, p, &change.removed)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// TS-18 step 2 and the constraint checks of a commit at `ts`, under the
/// commit latch, before its write keys are registered (module
/// documentation): the index changes of every index of the catalog cache
/// on a graph the transaction wrote, the ONLINE switch of
/// [`Transaction::online`], and the checks of every constraint.
///
/// # Errors
///
/// [`Error::ConstraintViolation`]; [`Error::InvalidArgument`] for an index
/// key longer than 1 024 bytes or a malformed record in the write set;
/// storage errors.
pub(crate) fn derive(
    core: &Core,
    store: &Store<'_>,
    tx: &Transaction<'_>,
    ts: u64,
) -> Result<Derived> {
    let mut d = Derived::default();
    if let Some(on) = &tx.online {
        switch(core, store, on, &mut d)?;
    }
    let defs: Vec<SchemaDef> = {
        let cache = core.catalog.schema();
        cache
            .values()
            .filter(|s| tx.graphs.iter().any(|g| g.id == s.graph))
            .cloned()
            .collect()
    };
    if defs.is_empty() {
        return Ok(d);
    }
    let mut changes: BTreeMap<u64, Change> = BTreeMap::new();
    let mut buf = Vec::new();
    for g in &tx.graphs {
        for (entity, tree) in [(Entity::Node, g.nodes), (Entity::Edge, g.edges)] {
            let these: Vec<&SchemaDef> = defs
                .iter()
                .filter(|s| s.graph == g.id && s.entity == entity)
                .collect();
            if these.is_empty() || tree.is_pending() {
                continue;
            }
            let writes = tx
                .writes
                .range((tree.id(), Vec::new())..)
                .take_while(|((t, _), _)| *t == tree.id());
            for ((_, key), p) in writes {
                let id =
                    element_of(key).ok_or(Error::InvalidArgument("a malformed element key"))?;
                let old = committed_element(store, tree, entity, key, &mut buf)?;
                let new = match &p.value {
                    Some(v) => Some(
                        decode_element(entity, v)
                            .ok_or(Error::InvalidArgument("a malformed record"))?,
                    ),
                    None => None,
                };
                for def in &these {
                    if def.kind.is_index() {
                        let before: BTreeSet<Vec<u8>> = old
                            .as_ref()
                            .map(|e| index_keys(def, id, e))
                            .unwrap_or_default()
                            .into_iter()
                            .collect();
                        let after: BTreeSet<Vec<u8>> = new
                            .as_ref()
                            .map(|e| index_keys(def, id, e))
                            .unwrap_or_default()
                            .into_iter()
                            .collect();
                        let c = changes.entry(def.id).or_default();
                        for k in after.difference(&before) {
                            if k.len() > MAX_KEY {
                                return Err(Error::InvalidArgument(
                                    "a value too large for an index key of 1 024 bytes",
                                ));
                            }
                            c.added.insert(k.clone());
                        }
                        c.removed.extend(before.difference(&after).cloned());
                    } else if let Some(el) = &new
                        && el.tokens.contains(&def.token)
                    {
                        check_constraint(def, el)?;
                    }
                }
            }
        }
    }
    // Uniqueness first: a violation fails the commit before any write.
    for c in defs.iter().filter(|s| s.kind.owns_index()) {
        if let (Some(change), Some(ix)) =
            (changes.get(&c.link), defs.iter().find(|s| s.id == c.link))
            && !change.added.is_empty()
            && unique_violation(store, tree_of(store, ix.tree)?, change)?
        {
            return Err(Error::ConstraintViolation(Violation::ValueInUse));
        }
    }
    let mut seq = 0u32;
    for (index, c) in changes {
        let def = defs.iter().find(|s| s.id == index).expect("from defs");
        match def.state {
            State::Populating => {
                let side_tree = tree_of(store, def.side)?;
                let ops = c
                    .removed
                    .iter()
                    .map(|k| (side::Op::Delete, k))
                    .chain(c.added.iter().map(|k| (side::Op::Insert, k)));
                for (op, k) in ops {
                    d.writes.push(DerivedWrite {
                        tree: side_tree,
                        key: side::record_key(ts, seq).to_vec(),
                        value: Some(side::record_value(op, k)),
                        stamp: 0,
                    });
                    seq = seq.checked_add(1).ok_or(Error::InvalidArgument(
                        "too many index changes in one commit",
                    ))?;
                }
            }
            State::Online => {
                let t = tree_of(store, def.tree)?;
                index_stats(store, def, t, &c.added, &c.removed, &mut d.stats)?;
                for k in &c.removed {
                    d.keys.push((t.id(), k.as_slice().into()));
                    d.writes.push(DerivedWrite {
                        tree: t,
                        key: k.clone(),
                        value: None,
                        stamp: ts,
                    });
                }
                for k in c.added {
                    d.keys.push((t.id(), k.as_slice().into()));
                    d.writes.push(DerivedWrite {
                        tree: t,
                        key: k,
                        value: Some(Vec::new()),
                        stamp: ts,
                    });
                }
            }
        }
    }
    Ok(d)
}

/// An index tree as a replay sees it: its latest committed entries, with
/// the replay's own changes over them; the changes become frozen writes.
pub(crate) struct BatchTarget<'s> {
    store: Store<'s>,
    tree: Tree,
    /// Each key the replay touched: whether the tree held it, and whether
    /// it holds it now.
    pending: BTreeMap<Vec<u8>, (bool, bool)>,
}

impl<'s> BatchTarget<'s> {
    pub(crate) fn new(store: Store<'s>, tree: Tree) -> BatchTarget<'s> {
        BatchTarget {
            store,
            tree,
            pending: BTreeMap::new(),
        }
    }

    /// The keys added and removed: those whose state differs from the
    /// tree's.
    fn change(&self) -> Change {
        let mut c = Change::default();
        for (k, &(was, is)) in &self.pending {
            match (was, is) {
                (false, true) => {
                    c.added.insert(k.clone());
                }
                (true, false) => {
                    c.removed.insert(k.clone());
                }
                _ => {}
            }
        }
        c
    }

    /// The frozen writes of the changes (TS-18 step 5: an insert adds a
    /// frozen entry and a delete removes the entry).
    pub(crate) fn into_writes(self) -> Vec<DerivedWrite> {
        let tree = self.tree;
        self.pending
            .into_iter()
            .filter(|(_, (was, is))| was != is)
            .map(|(key, (_, present))| DerivedWrite {
                tree,
                key,
                value: present.then(Vec::new),
                stamp: 0,
            })
            .collect()
    }
}

impl side::Target for BatchTarget<'_> {
    fn present(&mut self, key: &[u8]) -> Result<bool> {
        if let Some(&(_, is)) = self.pending.get(key) {
            return Ok(is);
        }
        let mut v = Vec::new();
        let was = self
            .store
            .get_stamped(self.tree, key, &mut v)?
            .is_some_and(|st| st.deleted == 0);
        self.pending.insert(key.to_vec(), (was, was));
        Ok(was)
    }

    fn set(&mut self, key: &[u8], present: bool) {
        // The replay asks `present` before every `set`.
        if let Some(e) = self.pending.get_mut(key) {
            e.1 = present;
        }
    }
}

/// A side-tree record: its key, operation and index key.
pub(crate) type SideRecord = (Vec<u8>, side::Op, Vec<u8>);

/// The side-tree records after key `after` (all if empty), at most
/// `limit`, in commit order: `(key, operation, index key)`.
///
/// # Errors
///
/// [`Error::Corruption`] for a malformed record; storage errors.
pub(crate) fn read_side(
    store: &Store<'_>,
    side_tree: Tree,
    after: &[u8],
    limit: usize,
) -> Result<Vec<SideRecord>> {
    let mut lo = after.to_vec();
    if !lo.is_empty() {
        lo.push(0);
    }
    let mut out = Vec::new();
    let mut bad = None;
    store.scan_range_stamped(side_tree, &lo, None, |k, v, _| {
        match side::decode_value(v) {
            Ok((op, key)) if k.len() == side::RECORD_KEY_LEN => {
                out.push((k.to_vec(), op, key.to_vec()));
            }
            Ok(_) => bad = Some(Error::Corruption("malformed side-tree key".into())),
            Err(e) => bad = Some(e),
        }
        bad.is_none() && out.len() < limit
    })?;
    match bad {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// The ONLINE switch's part of a commit (TS-18 step 6), under the commit
/// latch: replays the side tree after the builder's last replayed record
/// into the index, as frozen writes, and adds the index's statistics.
fn switch(core: &Core, store: &Store<'_>, on: &Online, d: &mut Derived) -> Result<()> {
    let def = core.catalog.schema().get(&on.index).cloned();
    let Some(def) = def.filter(|s| s.state == State::Populating) else {
        return Err(Error::NotFound(Missing::PopulatingIndex));
    };
    let t = tree_of(store, def.tree)?;
    let records = read_side(store, tree_of(store, def.side)?, &on.from, usize::MAX)?;
    let mut target = BatchTarget::new(*store, t);
    let delta = side::online_tail(
        &mut target,
        records.iter().map(|(_, op, k)| (*op, k.as_slice())),
    )?;
    let c = target.change();
    debug_assert_eq!(delta, c.added.len() as i64 - c.removed.len() as i64);
    d.stats
        .add(stat(def.graph, def.id, false), on.entries as i64);
    d.stats
        .add(stat(def.graph, def.id, true), on.distinct as i64);
    index_stats(store, &def, t, &c.added, &c.removed, &mut d.stats)?;
    d.writes.extend(target.into_writes());
    Ok(())
}

/// The entries and distinct keys of index tree `tree` in its latest
/// committed state, by one ordered scan (TS-17 recount).
///
/// # Errors
///
/// Storage errors.
pub(crate) fn count(store: &Store<'_>, tree: Tree) -> Result<(u64, u64)> {
    let (mut entries, mut distinct) = (0u64, 0u64);
    let mut last: Option<Vec<u8>> = None;
    store.scan_prefix_stamped(tree, b"", |k, _, st| {
        if st.deleted == 0 {
            entries += 1;
            let p = prefix_of(k);
            if last.as_deref() != Some(p) {
                distinct += 1;
                last = Some(p.to_vec());
            }
        }
        true
    })?;
    Ok((entries, distinct))
}

/// The statistics of every ONLINE index of the catalog cache, recounted
/// (TS-17 After a crash; [`crate::storage::graph::recount`]).
///
/// # Errors
///
/// Storage errors; [`Error::Corruption`] for a missing index tree.
pub(crate) fn recount(core: &Core, counts: &mut BTreeMap<StatKey, u64>) -> Result<()> {
    let store = core.store();
    let defs: Vec<SchemaDef> = core.catalog.schema().values().cloned().collect();
    for def in defs
        .iter()
        .filter(|s| s.kind.is_index() && s.state == State::Online)
    {
        let (entries, distinct) = count(&store, tree_of(&store, def.tree)?)?;
        for (k, n) in [(false, entries), (true, distinct)] {
            if n > 0 {
                counts.insert(stat(def.graph, def.id, k), n);
            }
        }
    }
    Ok(())
}

/// The index-against-data check of TS-12 (storage.md: "each entry of an
/// ONLINE index matches a record with the indexed values, and each record
/// that an ONLINE index of its graph covers has its entry there"), over
/// the latest committed state, under the commit latch the caller holds:
/// one scan of the element tree probing the index for each key the
/// record gives, and one scan of the index probing the record of each
/// live entry. Both directions are exact and hold one entry at a time, as
/// SQLite's `integrity_check` probes each index for each row (R-SQLITE @
/// `97048e9` `src/pragma.c:2072-2095`) and oxigraph's `validate` probes in
/// one direction after comparing counts (R-OXIGRAPH @ `fda6793`
/// `lib/oxigraph/src/storage/rocksdb.rs:894-1005`). POPULATING indexes
/// are not compared.
///
/// # Errors
///
/// Storage errors.
pub(crate) fn check(core: &Core, push: &mut dyn FnMut(String)) -> Result<()> {
    let store = core.store();
    let defs: Vec<SchemaDef> = core.catalog.schema().values().cloned().collect();
    for def in defs
        .iter()
        .filter(|s| s.kind.is_index() && s.state == State::Online)
    {
        let Some(elements) = element_tree(core, def.graph, def.entity)? else {
            push(format!(
                "index {}: its graph {} is missing",
                def.id, def.graph
            ));
            continue;
        };
        let Some(t) = store.tree(def.tree)? else {
            push(format!(
                "index {}: its tree {} is missing",
                def.id, def.tree
            ));
            continue;
        };
        let mut err = None;
        let mut v = Vec::new();
        store.scan_prefix_stamped(elements, b"", |k, rec, st| {
            if st.deleted != 0 {
                return true;
            }
            let (Some(id), Some(el)) = (element_of(k), decode_element(def.entity, rec)) else {
                push(format!("index {}: malformed record {k:?}", def.id));
                return true;
            };
            for key in index_keys(def, id, &el) {
                match store.get_stamped(t, &key, &mut v) {
                    Ok(Some(s)) if s.deleted == 0 => {}
                    Ok(_) => push(format!(
                        "index {}: element {id} has no entry {key:?}",
                        def.id
                    )),
                    Err(e) => {
                        err = Some(e);
                        return false;
                    }
                }
            }
            true
        })?;
        if let Some(e) = err {
            return Err(e);
        }
        store.scan_prefix_stamped(t, b"", |key, _, st| {
            if st.deleted != 0 {
                return true;
            }
            let Some(id) = element_of_key(key) else {
                push(format!("index {}: malformed key {key:?}", def.id));
                return true;
            };
            match committed_element(&store, elements, def.entity, &element_key(id), &mut v) {
                Ok(Some(el)) if index_keys(def, id, &el).iter().any(|k| k == key) => {}
                Ok(_) => push(format!(
                    "index {}: entry {key:?} matches no record of element {id}",
                    def.id
                )),
                Err(e) => {
                    err = Some(e);
                    return false;
                }
            }
            true
        })?;
        if let Some(e) = err {
            return Err(e);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The transaction API: definitions, reads, constraint locks, drops.

impl Transaction<'_> {
    /// Definition `id` of `graph` as the snapshot sees it.
    ///
    /// # Errors
    ///
    /// Errors of [`Transaction::get`]; [`Error::Corruption`] for a
    /// malformed entry.
    pub(crate) fn schema_def(&mut self, graph: u64, id: u64) -> Result<Option<SchemaDef>> {
        let t = self.tree(CATALOG_TREE)?;
        let k = schema_key(graph, id);
        match self.get(t, &k)? {
            None => Ok(None),
            Some(v) => SchemaDef::decode(&k, &v)
                .map(Some)
                .ok_or_else(|| Error::Corruption("malformed index definition".into())),
        }
    }

    /// Every definition of `graph` the snapshot sees, by identifier.
    ///
    /// # Errors
    ///
    /// As [`Transaction::schema_def`].
    pub(crate) fn schema_defs(&mut self, graph: u64) -> Result<Vec<SchemaDef>> {
        let t = self.tree(CATALOG_TREE)?;
        let mut out = Vec::new();
        let mut bad = false;
        self.scan_prefix(t, &graph_prefix(graph), |k, v| {
            match SchemaDef::decode(k, v) {
                Some(s) => out.push(s),
                None => bad = true,
            }
            true
        })?;
        if bad {
            return Err(Error::Corruption("malformed index definition".into()));
        }
        Ok(out)
    }

    /// Index `id` of `g`, if the snapshot sees it ONLINE, and its tree.
    fn online_index(&mut self, g: &Graph, id: u64) -> Result<(SchemaDef, Tree)> {
        match self.schema_def(g.id, id)? {
            Some(def) if def.kind.is_index() && def.state == State::Online => {
                let t = self.tree(def.tree)?;
                Ok((def, t))
            }
            _ => Err(Error::NotFound(Missing::OnlineIndex)),
        }
    }

    /// The elements of index `id` of graph `g` whose keys, without the
    /// index's own prefix (its identifier; none for a lookup index), lie
    /// in `[lo, hi)`, `hi` `None` for the end of the index, in key order:
    /// the index entries the snapshot sees, with this transaction's record
    /// changes applied. The range is recorded as a read (TS-15).
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if the snapshot does not see the index ONLINE;
    /// errors of [`Transaction::scan_range`].
    pub(crate) fn index_scan(
        &mut self,
        g: &Graph,
        id: u64,
        lo: &[u8],
        hi: Option<&[u8]>,
    ) -> Result<Vec<u64>> {
        self.index_scan_chunk(g, id, lo, hi, usize::MAX)
            .map(|(v, _)| v)
    }

    /// [`Transaction::index_scan`] that reads at most `limit` committed
    /// index entries ([`Transaction::scan_chunk`]): the elements of the keys
    /// up to the last one read, and the key (without the index's prefix)
    /// to resume from, `None` when the range is done (rmp #45).
    ///
    /// # Errors
    ///
    /// As [`Transaction::index_scan`].
    pub(crate) fn index_scan_chunk(
        &mut self,
        g: &Graph,
        id: u64,
        lo: &[u8],
        hi: Option<&[u8]>,
        limit: usize,
    ) -> Result<(Vec<u64>, Option<Vec<u8>>)> {
        let (def, t) = self.online_index(g, id)?;
        let prefix = def.key_prefix();
        let flo = [&prefix[..], lo].concat();
        let fhi = match hi {
            Some(h) => Some([&prefix[..], h].concat()),
            None => crate::tx::conflict::Interval::prefix(&prefix).hi,
        };
        let mut keys: BTreeSet<Vec<u8>> = BTreeSet::new();
        let resume = self.scan_chunk(t, &flo, fhi.as_deref(), limit, |k, _| {
            keys.insert(k.to_vec());
            true
        })?;
        let chunk_hi = resume.clone().or(fhi);
        let in_range = |k: &[u8]| k >= flo.as_slice() && chunk_hi.as_deref().is_none_or(|h| k < h);
        // This transaction's record changes, which no index entry holds
        // until it commits.
        let elements = match def.entity {
            Entity::Node => g.nodes,
            Entity::Edge => g.edges,
        };
        let own: Vec<(Vec<u8>, Option<Vec<u8>>)> = self
            .writes
            .range((elements.id(), Vec::new())..)
            .take_while(|((t, _), _)| *t == elements.id())
            .map(|((_, k), p)| (k.clone(), p.value.clone()))
            .collect();
        for (k, new) in own {
            let Some(eid) = element_of(&k) else { continue };
            let old = self.read_committed(elements, &k)?;
            for (rec, add) in [(old, false), (new, true)] {
                let Some(el) = rec.as_deref().and_then(|v| decode_element(def.entity, v)) else {
                    continue;
                };
                for key in index_keys(&def, eid, &el) {
                    if in_range(&key) {
                        if add {
                            keys.insert(key);
                        } else {
                            keys.remove(&key);
                        }
                    }
                }
            }
        }
        let resume = resume.map(|r| r[prefix.len()..].to_vec());
        Ok((
            keys.iter().filter_map(|k| element_of_key(k)).collect(),
            resume,
        ))
    }

    /// The elements of graph `g` whose property of point index `id` is a
    /// point at most `radius` from `center` ([`spatial::distance`],
    /// FR-TYPE-005), ascending: the candidates of the Hilbert ranges
    /// covering [`spatial::distance_boxes`], each range recorded as a read,
    /// filtered by the exact distance of the record the snapshot sees.
    ///
    /// # Errors
    ///
    /// As [`Transaction::index_scan`]; [`Error::InvalidArgument`] if `id`
    /// is not a point index.
    pub(crate) fn index_within(
        &mut self,
        g: &Graph,
        id: u64,
        center: &Point,
        radius: f64,
    ) -> Result<Vec<u64>> {
        let (def, _) = self.online_index(g, id)?;
        if def.kind != SchemaKind::Point {
            return Err(Error::InvalidArgument("not a point index"));
        }
        let srid = center.srid().code().to_be_bytes();
        let mut candidates = BTreeSet::new();
        for (lo, hi) in spatial::distance_boxes(center, radius) {
            for (a, b) in spatial::box_ranges(&lo, &hi).unwrap_or_default() {
                let from = [&srid[..], &a.to_be_bytes()].concat();
                let to = match b.checked_add(1) {
                    Some(n) => Some([&srid[..], &n.to_be_bytes()].concat()),
                    None => crate::tx::conflict::Interval::prefix(&srid).hi,
                };
                candidates.extend(self.index_scan(g, id, &from, to.as_deref())?);
            }
        }
        let tree = match def.entity {
            Entity::Node => g.nodes,
            Entity::Edge => g.edges,
        };
        let mut out = Vec::new();
        for e in candidates {
            let Some(v) = self.get(tree, &element_key(e))? else {
                continue;
            };
            let el = decode_element(def.entity, &v)
                .ok_or_else(|| Error::Corruption("malformed record".into()))?;
            if let Some(Value::Point(p)) = prop(&el.props, def.props[0])
                && spatial::distance(center, p).is_some_and(|d| d <= radius)
            {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// The write-time part of uniqueness and node-key constraints (TS-18,
    /// TS-15 unique value): for each such constraint of the latest
    /// committed catalog that covers element `id` of `g` with tokens and
    /// properties `el`, takes the X lock on `(constraint, value)`, then
    /// fails if the owned index's latest committed state holds the value
    /// for another element that this transaction has not written. Holding
    /// the lock, a writer of the same value waits until this transaction
    /// ends and then checks again, as PostgreSQL's `_bt_check_unique` waits
    /// for the inserter of a conflicting entry and then rechecks (R-PG @
    /// `1f22453` `src/backend/access/nbtree/nbtinsert.c:411, 584-599`).
    ///
    /// # Errors
    ///
    /// [`Error::ConstraintViolation`], which does not abort the
    /// transaction; errors of [`Transaction::lock_resource`].
    pub(crate) fn unique_values(
        &mut self,
        g: &Graph,
        entity: Entity,
        id: u64,
        el: &Element,
    ) -> Result<()> {
        let pairs: Vec<(SchemaDef, SchemaDef)> = {
            let cache = self.engine().core().catalog.schema();
            cache
                .values()
                .filter(|c| {
                    c.graph == g.id
                        && c.entity == entity
                        && c.kind.owns_index()
                        && el.tokens.contains(&c.token)
                })
                .filter_map(|c| cache.get(&c.link).map(|ix| (c.clone(), ix.clone())))
                .collect()
        };
        let elements = match entity {
            Entity::Node => g.nodes,
            Entity::Edge => g.edges,
        };
        for (c, ix) in pairs {
            let Some(value) = values(&c, el) else {
                continue;
            };
            self.lock_resource(
                Resource::Unique {
                    constraint: c.id,
                    value: value.as_slice().into(),
                },
                Mode::X,
            )?;
            let store = self.engine().store();
            let t = match tree_of(&store, ix.tree) {
                Ok(t) => t,
                Err(e) => return Err(self.abort_with(e)),
            };
            let prefix = [&ix.id.to_be_bytes()[..], &value].concat();
            let mut other = false;
            let r = store.scan_prefix_stamped(t, &prefix, |k, _, st| {
                if st.deleted == 0
                    && k.len() == prefix.len() + 8
                    && let Some(e) = element_of_key(k)
                {
                    other = e != id
                        && !self
                            .writes
                            .contains_key(&(elements.id(), element_key(e).to_vec()));
                }
                !other
            });
            if let Err(e) = r {
                return Err(self.abort_with(e));
            }
            if other {
                return Err(Error::ConstraintViolation(Violation::ValueInUse));
            }
        }
        Ok(())
    }

    /// Drops index or constraint `id` of graph `graph`: its definition gets
    /// a delete stamp, and its trees, and a constraint's owned index with
    /// its tree, are dropped with it. A POPULATING index's build stops
    /// when it sees the definition gone. Returns whether the snapshot saw
    /// it.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an index that a constraint owns, which
    /// is dropped with the constraint; [`Error::Serialization`] when a
    /// concurrent transaction changed the definition after the snapshot;
    /// the errors of [`Transaction::delete`] and
    /// [`Transaction::drop_tree`].
    pub(crate) fn drop_schema(&mut self, graph: u64, id: u64) -> Result<bool> {
        self.check_active()?;
        let Some(def) = self.schema_def(graph, id)? else {
            return Ok(false);
        };
        if def.kind.is_index() && def.link != 0 {
            return Err(Error::InvalidArgument(
                "an index that a constraint owns is dropped with the constraint",
            ));
        }
        let mut all = vec![def.clone()];
        if def.kind.owns_index()
            && let Some(ix) = self.schema_def(graph, def.link)?
        {
            all.push(ix);
        }
        let t = self.tree(CATALOG_TREE)?;
        for d in all {
            self.lock_key(t, &schema_key(graph, d.id), Mode::X)?;
            self.delete(t, &schema_key(graph, d.id))?;
            for tree in [d.tree, d.side] {
                if tree != 0 {
                    let tr = self.tree(tree)?;
                    self.drop_tree(tr)?;
                }
            }
        }
        Ok(true)
    }

    /// Drops every index and constraint of graph `graph` (a graph drop,
    /// [`crate::catalog`]).
    ///
    /// # Errors
    ///
    /// As [`Transaction::drop_schema`].
    pub(crate) fn drop_graph_schema(&mut self, graph: u64) -> Result<()> {
        for d in self.schema_defs(graph)? {
            if !(d.kind.is_index() && d.link != 0) {
                self.drop_schema(graph, d.id)?;
            }
        }
        Ok(())
    }

    /// Checks that graph `graph` exists at the snapshot and takes the S
    /// lock on its name, so that it is not dropped before this
    /// transaction ends ([`crate::catalog`] Graph types in use).
    pub(crate) fn lock_graph(&mut self, graph: u64) -> Result<Graph> {
        let Some(o) = self.object(graph)? else {
            return Err(Error::NotFound(Missing::Graph));
        };
        if o.kind != Kind::Graph {
            return Err(Error::NotFound(Missing::Graph));
        }
        let t = self.tree(CATALOG_TREE)?;
        self.lock_key(t, &name_key(o.parent, &o.name), Mode::S)?;
        self.graph(graph)
    }
}

/// Validates the shape of a definition (FR-IDX-001, FR-IDX-002).
fn validate(
    engine: &Engine,
    kind: SchemaKind,
    entity: Entity,
    token: u64,
    props: &[u64],
) -> Result<()> {
    use crate::catalog::TokenKind;
    let tokens = &engine.core().catalog.tokens;
    let n = props.len();
    let shape = match kind {
        SchemaKind::Lookup => n == 0 && token == 0,
        SchemaKind::Range | SchemaKind::Unique => (1..=MAX_PROPERTIES).contains(&n),
        SchemaKind::NodeKey => entity == Entity::Node && (1..=MAX_PROPERTIES).contains(&n),
        SchemaKind::Point | SchemaKind::Exists | SchemaKind::Type(_) => n == 1,
    };
    let distinct = props.iter().collect::<HashSet<_>>().len() == n;
    let token_ok = kind == SchemaKind::Lookup
        || tokens.kind(token)
            == Some(match entity {
                Entity::Node => TokenKind::Label,
                Entity::Edge => TokenKind::EdgeType,
            });
    let props_ok = props
        .iter()
        .all(|&p| tokens.kind(p) == Some(TokenKind::PropertyKey));
    if shape && distinct && token_ok && props_ok {
        Ok(())
    } else {
        Err(Error::InvalidArgument(
            "an index or constraint of this kind cannot have these tokens or properties",
        ))
    }
}
