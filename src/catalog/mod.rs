//! The persistent catalog (TS-17, FR-DB-001, FR-CAT-001, FR-CAT-002,
//! FR-CAT-004): one catalog tree per database, whose entries join the
//! transaction's write set, so catalog and data changes commit and roll
//! back together.
//!
//! **Catalog tree** (tree [`CATALOG_TREE`], [`Keys::Prefix`] leaves). Keys
//! start with a kind byte:
//!
//! | Key | Value | Stamps |
//! |---|---|---|
//! | `0x01`, parent id (8, big-endian), name (UTF-8) | kind (1), object id (8, LE), owner length (varint), owner, definition | stamped |
//! | `0x02`, object id (8, big-endian) | parent id (8, LE), name | stamped |
//! | `0x03`, token kind (1), name | token id (8, LE) | none |
//! | `0x04`, token id (8, big-endian) | token kind (1), name | none |
//! | `0x05`, graph id (8, big-endian), object id (8, big-endian) | index or constraint definition ([`crate::index::SchemaDef`]) | stamped |
//!
//! The name entry is `(parent_id, name) → (object_id, kind, owner,
//! definition)` and the reverse entry `object_id → (parent_id, name)`
//! (TS-17). The parent of a top-level directory or schema is the root
//! directory, [`ROOT`], which has no entry. A directory or schema lies in
//! the root or in a directory; a graph or graph type lies in a schema
//! (FR-CAT-001). Directory nesting is bounded only by the key size: each
//! name entry holds one name, at most [`MAX_NAME`] bytes (BR-GQL-002 item
//! IL020, user decision 2026-10-07).
//!
//! **Definitions.** A graph's: its node, edge and adjacency tree
//! identifiers and its graph type's object identifier (0 for none), each 8
//! bytes little-endian. A graph type's: the bytes it was created with,
//! never changed (its check is Sprint 3's). A directory's and a schema's:
//! none.
//!
//! **Creation** (TS-17 Creation). [`bootstrap`] gives the entries that
//! `create` writes into `data-1`, frozen: the default schema ([`DEFAULT_SCHEMA_ID`])
//! and the default graph ([`DEFAULT_GRAPH_ID`]) with the owner `create`
//! receives (FR-DB-005, FR-AUTH-002).
//!
//! **Names** (TS-17 Catalog names, TS-15). Creating or dropping an object
//! takes the X lock on its name key, then applies the write-write rule:
//! of two transactions that create the same name, the second waits, then
//! fails with 25N11 if the first committed, under snapshot isolation too.
//! Creating an object in a directory or schema also takes the S lock on the
//! parent's name key, and dropping a directory or schema takes X there and
//! then checks the latest committed children: a child created after the
//! dropper's snapshot fails it with 25N11 unless the child's drop has
//! committed, which the dropper learns by taking the S lock on the child's
//! name, so waiting for its dropper to end, and reading it again (a drop
//! the check sees may still be undone, TS-16 step 6; as PostgreSQL's
//! referential check reads the latest committed rows and fails a transaction-snapshot
//! transaction that finds one its snapshot does not see, R-PG @ `1f22453`
//! `src/backend/utils/adt/ri_triggers.c:2683-2699`). Without the parent
//! locks, under snapshot isolation a schema could be dropped while a graph
//! is created in it (TS-17 Parents).
//!
//! **Graph types in use** (FR-CAT-001). A graph names its graph type, so
//! the graph type cannot be dropped while a graph uses it, as PostgreSQL
//! refuses to drop an object other objects depend on unless the drop
//! cascades (RESTRICT, R-PG @ `1f22453`
//! `src/backend/catalog/dependency.c:1160-1168`, SQLSTATE 2BP01). Creating
//! a graph takes the S lock on its graph type's name, which the dropper
//! takes X, so one of the two waits for the other, as PostgreSQL's
//! foreign-key creation locks the referenced table against concurrent
//! changes (R-PG `src/backend/commands/tablecmds.c:10188-10190`). The
//! dropper then checks the latest committed graphs as for children above:
//! one that uses the type and was created after its snapshot fails it
//! with 25N11 unless its drop has committed; the creator that waited finds the type's name entry
//! delete-stamped after its snapshot and fails with 25N11 (the write-write
//! rule of [`Transaction::lock_key`]).
//!
//! **Tokens** ([`Tokens`], TS-17 Tokens). Unstamped and never deleted. A
//! missing token is created at once in an inner commit of its own, under a
//! per-database latch, after a second lookup (Neo4j `CreatingTokenHolder`,
//! R-NEO `community/token-api/.../CreatingTokenHolder.java:62-71`). The
//! committed tokens are kept in memory, loaded at open (Neo4j `TokenHolder`,
//! Memgraph `NameIdMapper`).
//!
//! **Cache** ([`Catalog::objects`]). The latest committed objects, loaded at
//! open and changed by each commit when it is applied, under the commit
//! latch ([`Effects`]), and the latest committed index and constraint
//! definitions ([`Catalog::schema`]), which committers read under the
//! commit latch (TS-18 step 2, [`crate::index`]).

pub(crate) mod stats;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};

use crate::index::{SCHEMA, SchemaDef};
use crate::storage::btree::node::put_varint;
use crate::storage::btree::{Keys, Store, Tree, Write};
use crate::storage::datafile::Bootstrap;
use crate::storage::engine::{Core, Engine};
use crate::storage::error::{Error, Result};
use crate::storage::graph::Graph;
use crate::sync::{Mutex, MutexGuard, PoisonError, RwLock};
use crate::tx::Transaction;
use crate::tx::commit::Ids;
use crate::tx::lock::{Mode, Resource};

/// The catalog tree's identifier, created by `create`.
pub(crate) const CATALOG_TREE: u64 = 1;
/// The statistics tree's identifier, created by `create` (TS-11).
pub(crate) const STATS_TREE: u64 = 2;
/// The node, edge and adjacency trees of the default graph.
pub(crate) const DEFAULT_GRAPH_TREES: [u64; 3] = [3, 4, 5];
/// The trees `create` writes: catalog, statistics, the default graph's.
pub(crate) const CREATED_TREES: u64 = 5;
/// The root directory: the parent of top-level objects. It has no entry.
pub(crate) const ROOT: u64 = 0;
/// The default schema's object identifier (FR-DB-005).
pub(crate) const DEFAULT_SCHEMA_ID: u64 = 1;
/// The default graph's object identifier (FR-DB-005).
pub(crate) const DEFAULT_GRAPH_ID: u64 = 2;
/// The next catalog-object identifier after create.
pub(crate) const FIRST_OBJECT_ID: u64 = 3;
/// The default schema's name (user decision 2026-10-08, rmp #31): GQL
/// fixes no name; PostgreSQL names its default schema `public`.
pub(crate) const DEFAULT_SCHEMA: &str = "public";
/// The default graph's name, in the default schema, so its catalog path is
/// `/public/lpg` (user decision 2026-10-08, rmp #31): named after the
/// product, as Neo4j and Memgraph name their default database.
pub(crate) const DEFAULT_GRAPH: &str = "lpg";
/// The longest name, in bytes: a name key is the kind byte, the parent
/// identifier and the name, at most 1 024 bytes (TS-05 Key size).
pub(crate) const MAX_NAME: usize = 1024 - 9;
/// The longest owner principal, in bytes: it lies in the name entry's
/// value, which `create` writes into one leaf.
pub(crate) const MAX_OWNER: usize = 1024;

const NAME: u8 = 1;
const REVERSE: u8 = 2;
const TOKEN: u8 = 3;
const TOKEN_ID: u8 = 4;

/// The kind of a catalog object (FR-CAT-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    /// A directory: holds directories and schemas.
    Directory = 1,
    /// A schema: holds graphs and graph types.
    Schema = 2,
    /// A graph type.
    GraphType = 3,
    /// A graph.
    Graph = 4,
}

impl Kind {
    fn from_byte(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::Directory,
            2 => Kind::Schema,
            3 => Kind::GraphType,
            4 => Kind::Graph,
            _ => return None,
        })
    }
}

/// The trees and graph type of a graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GraphDef {
    /// The node tree's identifier.
    pub(crate) nodes: u64,
    /// The edge tree's identifier.
    pub(crate) edges: u64,
    /// The adjacency tree's identifier.
    pub(crate) adj: u64,
    /// The graph type's object identifier, 0 for none.
    pub(crate) graph_type: u64,
}

/// What an object's entry defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Definition {
    /// A directory or a schema.
    None,
    /// A graph type: the bytes it was created with, never changed.
    GraphType(Vec<u8>),
    /// A graph.
    Graph(GraphDef),
}

/// A catalog object as its entries record it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Object {
    /// Its identifier: monotonic, never reused (TS-17).
    pub(crate) id: u64,
    /// Its parent's identifier, [`ROOT`] for a top-level object.
    pub(crate) parent: u64,
    /// Its name in its parent.
    pub(crate) name: String,
    /// Its kind.
    pub(crate) kind: Kind,
    /// The principal that created it (FR-AUTH-002).
    pub(crate) owner: String,
    /// Its definition.
    pub(crate) definition: Definition,
}

/// The name key of `name` in `parent`.
pub(crate) fn name_key(parent: u64, name: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(9 + name.len());
    k.push(NAME);
    k.extend_from_slice(&parent.to_be_bytes());
    k.extend_from_slice(name.as_bytes());
    k
}

/// The prefix of the name keys of `parent`'s children.
fn children_prefix(parent: u64) -> [u8; 9] {
    let mut k = [0u8; 9];
    k[0] = NAME;
    k[1..].copy_from_slice(&parent.to_be_bytes());
    k
}

/// The reverse key of object `id`.
pub(crate) fn reverse_key(id: u64) -> [u8; 9] {
    let mut k = [0u8; 9];
    k[0] = REVERSE;
    k[1..].copy_from_slice(&id.to_be_bytes());
    k
}

fn encode_object(o: &Object) -> Vec<u8> {
    let mut v = Vec::with_capacity(32 + o.owner.len());
    v.push(o.kind as u8);
    v.extend_from_slice(&o.id.to_le_bytes());
    put_varint(&mut v, o.owner.len() as u64);
    v.extend_from_slice(o.owner.as_bytes());
    match &o.definition {
        Definition::None => {}
        Definition::GraphType(d) => v.extend_from_slice(d),
        Definition::Graph(g) => {
            for x in [g.nodes, g.edges, g.adj, g.graph_type] {
                v.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    v
}

fn le(b: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(b.try_into().ok()?))
}

/// Decodes the name entry `key` → `value`; `None` if malformed.
fn decode_object(key: &[u8], value: &[u8]) -> Option<Object> {
    if key.len() < 9 || key[0] != NAME {
        return None;
    }
    let parent = u64::from_be_bytes(key[1..9].try_into().ok()?);
    let name = String::from_utf8(key[9..].to_vec()).ok()?;
    let kind = Kind::from_byte(*value.first()?)?;
    let id = le(value.get(1..9)?)?;
    let mut at = 9;
    let mut n = 0u64;
    let mut shift = 0;
    loop {
        let b = *value.get(at)?;
        at += 1;
        n |= u64::from(b & 0x7F) << shift;
        if b < 0x80 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    let n = usize::try_from(n).ok()?;
    let owner = String::from_utf8(value.get(at..at.checked_add(n)?)?.to_vec()).ok()?;
    let rest = &value[at + n..];
    let definition = match kind {
        Kind::Directory | Kind::Schema if rest.is_empty() => Definition::None,
        Kind::GraphType => Definition::GraphType(rest.to_vec()),
        Kind::Graph if rest.len() == 32 => Definition::Graph(GraphDef {
            nodes: le(&rest[0..8])?,
            edges: le(&rest[8..16])?,
            adj: le(&rest[16..24])?,
            graph_type: le(&rest[24..32])?,
        }),
        _ => return None,
    };
    Some(Object {
        id,
        parent,
        name,
        kind,
        owner,
        definition,
    })
}

fn reverse_value(parent: u64, name: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + name.len());
    v.extend_from_slice(&parent.to_le_bytes());
    v.extend_from_slice(name.as_bytes());
    v
}

fn decode_reverse(v: &[u8]) -> Option<(u64, String)> {
    Some((le(v.get(..8)?)?, String::from_utf8(v[8..].to_vec()).ok()?))
}

/// The entries of a tree that `create` writes, in key order.
pub(crate) type TreeEntries = Vec<(Vec<u8>, Vec<u8>)>;

/// The catalog entries and the trees that `create` writes into `data-1`
/// (TS-17 Creation): the catalog tree with the default schema and default
/// graph, owned by `owner`; the empty statistics tree; the default graph's
/// empty node, edge and adjacency trees. Tree `i` of the result is tree
/// `i + 1`.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for an empty owner or one longer than
/// [`MAX_OWNER`] bytes.
pub(crate) fn bootstrap(owner: &str) -> Result<Vec<(Keys, TreeEntries)>> {
    if owner.is_empty() || owner.len() > MAX_OWNER {
        return Err(Error::InvalidArgument(
            "the owner principal is empty or longer than 1 024 bytes",
        ));
    }
    let [nodes, edges, adj] = DEFAULT_GRAPH_TREES;
    let objects = [
        Object {
            id: DEFAULT_SCHEMA_ID,
            parent: ROOT,
            name: DEFAULT_SCHEMA.into(),
            kind: Kind::Schema,
            owner: owner.into(),
            definition: Definition::None,
        },
        Object {
            id: DEFAULT_GRAPH_ID,
            parent: DEFAULT_SCHEMA_ID,
            name: DEFAULT_GRAPH.into(),
            kind: Kind::Graph,
            owner: owner.into(),
            definition: Definition::Graph(GraphDef {
                nodes,
                edges,
                adj,
                graph_type: 0,
            }),
        },
    ];
    let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for o in &objects {
        entries.push((name_key(o.parent, &o.name), encode_object(o)));
        entries.push((reverse_key(o.id).to_vec(), reverse_value(o.parent, &o.name)));
    }
    entries.sort();
    Ok(vec![
        (Keys::Prefix, entries),
        (Keys::Prefix, Vec::new()),
        (Keys::Prefix, Vec::new()),
        (Keys::Prefix, Vec::new()),
        (Keys::Fields, Vec::new()),
    ])
}

/// [`bootstrap`] as the trees [`crate::storage::datafile::Datafile::create`]
/// takes.
pub(crate) fn bootstrap_trees(trees: &[(Keys, TreeEntries)]) -> Vec<Bootstrap<'_>> {
    trees
        .iter()
        .map(|(keys, entries)| Bootstrap {
            keys: *keys,
            entries,
        })
        .collect()
}

/// The ids `create` writes to `CONTROL`: the next catalog-object
/// identifier above the default schema and graph (TS-05 Create).
pub(crate) fn created_ids() -> Ids {
    Ids {
        next_element_id: 0,
        next_catalog_id: FIRST_OBJECT_ID,
        next_token_id: 0,
    }
}

// ---------------------------------------------------------------------------
// Tokens.

/// The kind of a token (TS-17 Token tables).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum TokenKind {
    /// A node label.
    Label = 0,
    /// An edge type.
    EdgeType = 1,
    /// A property key.
    PropertyKey = 2,
}

impl TokenKind {
    fn from_byte(b: u8) -> Option<TokenKind> {
        Some(match b {
            0 => TokenKind::Label,
            1 => TokenKind::EdgeType,
            2 => TokenKind::PropertyKey,
            _ => return None,
        })
    }
}

/// The token entries of `(kind, name)` with identifier `id`: the name
/// entry and the identifier entry.
fn token_entries(kind: TokenKind, name: &str, id: u64) -> [(Vec<u8>, Vec<u8>); 2] {
    let mut k = vec![TOKEN, kind as u8];
    k.extend_from_slice(name.as_bytes());
    let mut r = vec![TOKEN_ID];
    r.extend_from_slice(&id.to_be_bytes());
    let mut v = vec![kind as u8];
    v.extend_from_slice(name.as_bytes());
    [(k, id.to_le_bytes().to_vec()), (r, v)]
}

#[derive(Debug, Default)]
struct TokenMaps {
    ids: HashMap<(TokenKind, String), u64>,
    names: HashMap<u64, (TokenKind, String)>,
}

/// The committed tokens of a database and the latch that creates them
/// (TS-17 Tokens). The map is filled at open and by each creation, after
/// its commit is durable.
#[derive(Debug, Default)]
pub(crate) struct Tokens {
    maps: RwLock<TokenMaps>,
    latch: Mutex<()>,
}

impl Tokens {
    /// The identifier of `(kind, name)`, if the token exists.
    pub(crate) fn get(&self, kind: TokenKind, name: &str) -> Option<u64> {
        self.maps
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .ids
            .get(&(kind, name.to_owned()))
            .copied()
    }

    /// The kind of token `id`, if it exists.
    pub(crate) fn kind(&self, id: u64) -> Option<TokenKind> {
        self.maps
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .names
            .get(&id)
            .map(|(k, _)| *k)
    }

    /// The kind and name of token `id`, if it exists.
    pub(crate) fn name(&self, id: u64) -> Option<(TokenKind, String)> {
        self.maps
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .names
            .get(&id)
            .cloned()
    }

    fn insert(&self, kind: TokenKind, name: &str, id: u64) {
        let mut m = self.maps.write().unwrap_or_else(PoisonError::into_inner);
        m.ids.insert((kind, name.to_owned()), id);
        m.names.insert(id, (kind, name.to_owned()));
    }

    /// The identifier of `(kind, name)`, created if missing: under the
    /// latch, the name is looked up again, then `create` draws an
    /// identifier and makes its entries durable, then the map takes it.
    /// Two callers that need the same name get the same identifier
    /// (TS-17).
    ///
    /// # Errors
    ///
    /// Errors of `create`, with the map unchanged.
    pub(crate) fn get_or_create(
        &self,
        kind: TokenKind,
        name: &str,
        create: impl FnOnce() -> Result<u64>,
    ) -> Result<u64> {
        if let Some(id) = self.get(kind, name) {
            return Ok(id);
        }
        // A panic in `create` leaves no half-made token: the map changes
        // only after it returns.
        let _latch = self.latch.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(id) = self.get(kind, name) {
            return Ok(id);
        }
        let id = create()?;
        self.insert(kind, name, id);
        Ok(id)
    }

    /// Every token, sorted by identifier.
    pub(crate) fn all(&self) -> Vec<(u64, TokenKind, String)> {
        let m = self.maps.read().unwrap_or_else(PoisonError::into_inner);
        let mut v: Vec<_> = m
            .names
            .iter()
            .map(|(id, (k, n))| (*id, *k, n.clone()))
            .collect();
        v.sort();
        v
    }
}

// ---------------------------------------------------------------------------
// The catalog state of an open database.

/// The catalog state of one open database, in [`Core`].
#[derive(Debug, Default)]
pub(crate) struct Catalog {
    /// The committed tokens.
    pub(crate) tokens: Tokens,
    /// The latest committed objects by identifier (the catalog cache),
    /// changed under the commit latch.
    objects: Mutex<BTreeMap<u64, Object>>,
    /// The statistics (TS-17), changed under the commit latch.
    pub(crate) stats: Mutex<stats::Stats>,
    /// The latest committed index and constraint definitions by
    /// identifier (TS-18 step 2 reads them under the commit latch).
    schema: Mutex<BTreeMap<u64, SchemaDef>>,
}

impl Catalog {
    /// The catalog cache: the latest committed objects (TS-18 step 2 reads
    /// it under the commit latch).
    pub(crate) fn objects(&self) -> MutexGuard<'_, BTreeMap<u64, Object>> {
        self.objects.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The latest committed index and constraint definitions, locked.
    pub(crate) fn schema(&self) -> MutexGuard<'_, BTreeMap<u64, SchemaDef>> {
        self.schema.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The statistics, locked.
    pub(crate) fn stats(&self) -> MutexGuard<'_, stats::Stats> {
        self.stats.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The catalog tree's handle.
///
/// # Errors
///
/// [`Error::Corruption`] if the directory does not list it.
pub(crate) fn catalog_tree(store: &Store<'_>) -> Result<Tree> {
    store
        .tree(CATALOG_TREE)?
        .ok_or_else(|| Error::Corruption("the catalog tree is missing".into()))
}

/// TS-13 step 7 for the catalog, at open, before any transaction: loads
/// the tokens and the catalog cache from the latest committed entries, and
/// the statistics from the statistics tree, recounted when the recovered
/// commit timestamp `recovered_ts` is above the one they include.
///
/// # Errors
///
/// [`Error::Corruption`] for a malformed entry or a missing tree; storage
/// errors.
pub(crate) fn open(core: &Core, recovered_ts: u64) -> Result<()> {
    let store = core.store();
    let tree = catalog_tree(&store)?;
    let mut objects = BTreeMap::new();
    let mut bad = false;
    store.scan_prefix_stamped(tree, &[NAME], |k, v, st| {
        if st.deleted == 0 {
            match decode_object(k, v) {
                Some(o) => {
                    objects.insert(o.id, o);
                }
                None => bad = true,
            }
        }
        true
    })?;
    store.scan_prefix(tree, &[TOKEN_ID], |k, v| {
        let id = k
            .get(1..9)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_be_bytes);
        let kind = v.first().copied().and_then(TokenKind::from_byte);
        let name = v.get(1..).and_then(|n| String::from_utf8(n.to_vec()).ok());
        match (id, kind, name) {
            (Some(id), Some(kind), Some(name)) if k.len() == 9 => {
                core.catalog.tokens.insert(kind, &name, id);
            }
            _ => bad = true,
        }
        true
    })?;
    let mut schema = BTreeMap::new();
    store.scan_prefix_stamped(tree, &[SCHEMA], |k, v, st| {
        if st.deleted == 0 {
            match SchemaDef::decode(k, v) {
                Some(d) => {
                    schema.insert(d.id, d);
                }
                None => bad = true,
            }
        }
        true
    })?;
    if bad {
        return Err(core
            .health
            .fail(Error::Corruption("malformed catalog entry".into())));
    }
    *core.catalog.objects() = objects;
    *core.catalog.schema() = schema;
    let st = stats::load(core)?;
    *core.catalog.stats() = st;
    if core.catalog.stats().saved_ts() < recovered_ts {
        // TS-17 After a crash: an exact recount before open returns.
        let counts = crate::storage::graph::recount(core)?;
        core.catalog.stats().replace(counts);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commit effects.

/// What a commit changes in the catalog cache and the statistics, read
/// under the commit latch from the transaction's write set and the
/// committed state before the commit's pages change (TS-16 step 6), and
/// published once they have changed.
#[derive(Debug, Default)]
pub(crate) struct Effects {
    /// Objects created, by identifier.
    created: Vec<Object>,
    /// Objects dropped, by identifier.
    dropped: Vec<u64>,
    /// The statistics changes.
    stats: stats::Delta,
    /// Graphs dropped: their statistics go.
    dropped_graphs: Vec<u64>,
    /// Index and constraint definitions created or changed.
    schema_put: Vec<SchemaDef>,
    /// Index and constraint definitions dropped: an index's statistics go.
    schema_dropped: Vec<u64>,
}

impl Effects {
    /// The effects of `tx`'s commit.
    ///
    /// # Errors
    ///
    /// Storage errors reading the committed state.
    pub(crate) fn of(
        core: &Core,
        store: &Store<'_>,
        tx: &Transaction<'_>,
        index_stats: &stats::Delta,
    ) -> Result<Effects> {
        let mut e = Effects::default();
        for ((tree, key), p) in tx.writes.range((CATALOG_TREE, Vec::new())..) {
            if *tree != CATALOG_TREE {
                break;
            }
            match (key.first(), &p.value) {
                (Some(&NAME), Some(v)) => {
                    let o = decode_object(key, v).ok_or(Error::InvalidArgument(
                        "a malformed catalog entry in a write set",
                    ))?;
                    e.created.push(o);
                }
                (Some(&REVERSE), None) => {
                    if let Some(id) = key.get(1..9).and_then(|b| b.try_into().ok()) {
                        e.dropped.push(u64::from_be_bytes(id));
                    }
                }
                (Some(&SCHEMA), Some(v)) => {
                    let d = SchemaDef::decode(key, v).ok_or(Error::InvalidArgument(
                        "a malformed index definition in a write set",
                    ))?;
                    e.schema_put.push(d);
                }
                (Some(&SCHEMA), None) => {
                    if let Some(id) = key.get(9..17).and_then(|b| b.try_into().ok()) {
                        e.schema_dropped.push(u64::from_be_bytes(id));
                    }
                }
                _ => {}
            }
        }
        e.stats = crate::storage::graph::stats_delta(core, store, tx)?;
        e.stats.merge(index_stats);
        e.stats.merge(&tx.schema_stats);
        e.dropped_graphs.clone_from(&tx.dropped_graphs);
        Ok(e)
    }

    /// Applies the effects to the catalog cache and the statistics.
    pub(crate) fn publish(self, core: &Core) {
        {
            let mut objects = core.catalog.objects();
            for id in &self.dropped {
                objects.remove(id);
            }
            for o in self.created {
                objects.insert(o.id, o);
            }
        }
        {
            let mut schema = core.catalog.schema();
            for id in &self.schema_dropped {
                schema.remove(id);
            }
            for d in self.schema_put {
                schema.insert(d.id, d);
            }
        }
        let mut st = core.catalog.stats();
        st.apply(&self.stats);
        for g in &self.dropped_graphs {
            st.remove_graph(*g);
        }
        for i in &self.schema_dropped {
            st.remove_index(*i);
        }
    }
}

// ---------------------------------------------------------------------------
// The crate-internal catalog API, for tests until Sprint 3.

impl Engine {
    /// The identifier of the token `(kind, name)`, created at once if
    /// missing, in an inner commit of its own (TS-17 Tokens). The token
    /// stays whatever happens to the transaction that needed it.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a name longer than [`MAX_NAME`]
    /// bytes; errors of [`Engine::commit`].
    pub(crate) fn token(&self, kind: TokenKind, name: &str) -> Result<u64> {
        if name.len() > MAX_NAME - 1 {
            return Err(Error::InvalidArgument(
                "a token name longer than 1 014 bytes",
            ));
        }
        let core = self.core();
        core.catalog.tokens.get_or_create(kind, name, || {
            let id = core.ids.token()?;
            let tree = catalog_tree(&self.store())?;
            let entries = token_entries(kind, name, id);
            let batch: Vec<Write<'_>> = entries
                .iter()
                .map(|(k, v)| Write {
                    tree,
                    key: k,
                    value: Some(v),
                    stamp: 0,
                })
                .collect();
            self.commit(&batch, core.ids.current())?;
            Ok(id)
        })
    }

    /// The identifier of the token `(kind, name)`, if it exists.
    pub(crate) fn token_id(&self, kind: TokenKind, name: &str) -> Option<u64> {
        self.core().catalog.tokens.get(kind, name)
    }

    /// The kind and name of token `id`, if it exists.
    pub(crate) fn token_name(&self, id: u64) -> Option<(TokenKind, String)> {
        self.core().catalog.tokens.name(id)
    }
}

/// The parent kinds an object of `kind` may lie in (FR-CAT-001): the root
/// or a directory for directories and schemas, a schema for graphs and
/// graph types.
fn parent_allowed(kind: Kind, parent: Option<Kind>) -> bool {
    match kind {
        Kind::Directory | Kind::Schema => matches!(parent, None | Some(Kind::Directory)),
        Kind::Graph | Kind::GraphType => parent == Some(Kind::Schema),
    }
}

impl Transaction<'_> {
    fn catalog_tree(&mut self) -> Result<Tree> {
        self.tree(CATALOG_TREE)
    }

    /// The object named `name` in `parent`, as the snapshot sees it with
    /// this transaction's changes.
    ///
    /// # Errors
    ///
    /// Errors of [`Transaction::get`]; [`Error::Corruption`] for a
    /// malformed entry.
    pub(crate) fn lookup(&mut self, parent: u64, name: &str) -> Result<Option<Object>> {
        let t = self.catalog_tree()?;
        let key = name_key(parent, name);
        match self.get(t, &key)? {
            None => Ok(None),
            Some(v) => decode_object(&key, &v)
                .map(Some)
                .ok_or_else(|| Error::Corruption("malformed catalog name entry".into())),
        }
    }

    /// Object `id`, as the snapshot sees it with this transaction's
    /// changes.
    ///
    /// # Errors
    ///
    /// As [`Transaction::lookup`].
    pub(crate) fn object(&mut self, id: u64) -> Result<Option<Object>> {
        let t = self.catalog_tree()?;
        let Some(v) = self.get(t, &reverse_key(id))? else {
            return Ok(None);
        };
        let (parent, name) = decode_reverse(&v)
            .ok_or_else(|| Error::Corruption("malformed catalog reverse entry".into()))?;
        self.lookup(parent, &name)
    }

    /// The children of `parent`, in name order.
    ///
    /// # Errors
    ///
    /// As [`Transaction::lookup`].
    pub(crate) fn children(&mut self, parent: u64) -> Result<Vec<Object>> {
        let t = self.catalog_tree()?;
        let mut out = Vec::new();
        let mut bad = false;
        self.scan_prefix(t, &children_prefix(parent), |k, v| {
            match decode_object(k, v) {
                Some(o) => out.push(o),
                None => bad = true,
            }
            true
        })?;
        if bad {
            return Err(Error::Corruption("malformed catalog name entry".into()));
        }
        Ok(out)
    }

    /// The path of object `id`: the names from the root down, resolved
    /// through the reverse entries.
    ///
    /// # Errors
    ///
    /// As [`Transaction::lookup`].
    pub(crate) fn path(&mut self, id: u64) -> Result<Option<Vec<String>>> {
        let t = self.catalog_tree()?;
        let mut names = Vec::new();
        let mut at = id;
        while at != ROOT {
            let Some(v) = self.get(t, &reverse_key(at))? else {
                return Ok(None);
            };
            let (parent, name) = decode_reverse(&v)
                .ok_or_else(|| Error::Corruption("malformed catalog reverse entry".into()))?;
            names.push(name);
            if names.len() > 1 << 20 {
                return Err(Error::Corruption("a cycle of catalog parents".into()));
            }
            at = parent;
        }
        names.reverse();
        Ok(Some(names))
    }

    /// The identifiers of every graph the snapshot sees, ascending.
    ///
    /// # Errors
    ///
    /// As [`Transaction::lookup`].
    pub(crate) fn graph_ids(&mut self) -> Result<Vec<u64>> {
        let t = self.catalog_tree()?;
        let mut ids = Vec::new();
        self.scan_prefix(t, &[REVERSE], |k, _| {
            if let Some(id) = k.get(1..9).and_then(|b| b.try_into().ok()) {
                ids.push(u64::from_be_bytes(id));
            }
            true
        })?;
        let mut graphs = Vec::new();
        for id in ids {
            if self.object(id)?.is_some_and(|o| o.kind == Kind::Graph) {
                graphs.push(id);
            }
        }
        Ok(graphs)
    }

    /// The object at `path`, names from the root down.
    ///
    /// # Errors
    ///
    /// As [`Transaction::lookup`].
    pub(crate) fn resolve(&mut self, path: &[&str]) -> Result<Option<Object>> {
        let mut parent = ROOT;
        let mut found = None;
        for name in path {
            let Some(o) = self.lookup(parent, name)? else {
                return Ok(None);
            };
            parent = o.id;
            found = Some(o);
        }
        Ok(found)
    }

    /// Creates the object `name` of `kind` in `parent`, owned by `owner`.
    fn create_object(
        &mut self,
        parent: u64,
        name: &str,
        kind: Kind,
        owner: &str,
        definition: impl FnOnce(&mut Self) -> Result<Definition>,
    ) -> Result<u64> {
        self.check_active()?;
        if name.is_empty() || name.len() > MAX_NAME {
            return Err(Error::InvalidArgument(
                "a catalog name is empty or longer than 1 015 bytes",
            ));
        }
        if owner.is_empty() || owner.len() > MAX_OWNER {
            return Err(Error::InvalidArgument(
                "the owner principal is empty or longer than 1 024 bytes",
            ));
        }
        let t = self.catalog_tree()?;
        // The parent: it exists, may hold this kind, and stays until this
        // transaction ends (its name's S lock).
        let parent_kind = if parent == ROOT {
            None
        } else {
            let Some(p) = self.object(parent)? else {
                return Err(Error::NotFound("the parent catalog object"));
            };
            self.lock_key(t, &name_key(p.parent, &p.name), Mode::S)?;
            Some(p.kind)
        };
        if !parent_allowed(kind, parent_kind) {
            return Err(Error::InvalidArgument(
                "a catalog object of this kind cannot lie in that parent",
            ));
        }
        let key = name_key(parent, name);
        self.lock_key(t, &key, Mode::X)?;
        if self.get(t, &key)?.is_some() {
            return Err(Error::CatalogObjectExists);
        }
        let definition = definition(self)?;
        let id = match self.engine().core().ids.catalog() {
            Ok(id) => id,
            Err(e) => return Err(self.abort_with(e)),
        };
        let o = Object {
            id,
            parent,
            name: name.into(),
            kind,
            owner: owner.into(),
            definition,
        };
        self.put(t, &key, &encode_object(&o))?;
        self.put(t, &reverse_key(id), &reverse_value(parent, name))?;
        Ok(id)
    }

    /// Creates the directory `name` in the root or directory `parent`
    /// (FR-CAT-005: through the Rust API only), owned by `owner`.
    ///
    /// # Errors
    ///
    /// [`Error::CatalogObjectExists`]; [`Error::NotFound`] for a missing
    /// parent; [`Error::InvalidArgument`] for a parent of the wrong kind,
    /// an empty name or one over [`MAX_NAME`] bytes, or an owner as
    /// [`bootstrap`] refuses; [`Error::Serialization`] when a concurrent
    /// transaction committed the name, or dropped the parent, after the
    /// snapshot; the errors of [`Transaction::put`].
    pub(crate) fn create_directory(&mut self, parent: u64, name: &str, owner: &str) -> Result<u64> {
        self.create_object(parent, name, Kind::Directory, owner, |_| {
            Ok(Definition::None)
        })
    }

    /// Creates the schema `name` in the root or directory `parent`, owned
    /// by `owner`.
    ///
    /// # Errors
    ///
    /// As [`Transaction::create_directory`].
    pub(crate) fn create_schema(&mut self, parent: u64, name: &str, owner: &str) -> Result<u64> {
        self.create_object(parent, name, Kind::Schema, owner, |_| Ok(Definition::None))
    }

    /// Creates the graph type `name` in schema `schema`, owned by `owner`,
    /// defined by `definition`, which is stored unchanged and never
    /// changed.
    ///
    /// # Errors
    ///
    /// As [`Transaction::create_directory`].
    pub(crate) fn create_graph_type(
        &mut self,
        schema: u64,
        name: &str,
        owner: &str,
        definition: &[u8],
    ) -> Result<u64> {
        self.create_object(schema, name, Kind::GraphType, owner, |_| {
            Ok(Definition::GraphType(definition.to_vec()))
        })
    }

    /// Creates the graph `name` in schema `schema`, owned by `owner`, of
    /// graph type `graph_type` (none: an open graph), with its node, edge
    /// and adjacency trees, created by the same commit (TS-17 Graphs).
    ///
    /// # Errors
    ///
    /// As [`Transaction::create_directory`]; [`Error::NotFound`] for a
    /// graph type that the snapshot does not see;
    /// [`Error::Serialization`] when a concurrent transaction dropped the
    /// graph type after the snapshot.
    pub(crate) fn create_graph(
        &mut self,
        schema: u64,
        name: &str,
        owner: &str,
        graph_type: Option<u64>,
    ) -> Result<u64> {
        self.create_object(schema, name, Kind::Graph, owner, |tx| {
            if let Some(gt) = graph_type {
                match tx.object(gt)? {
                    // The graph type stays until this transaction ends
                    // (module documentation, Graph types in use).
                    Some(o) if o.kind == Kind::GraphType => {
                        let t = tx.catalog_tree()?;
                        tx.lock_key(t, &name_key(o.parent, &o.name), Mode::S)?;
                    }
                    _ => return Err(Error::NotFound("the graph type")),
                }
            }
            let nodes = tx.create_tree(Keys::Prefix)?.id();
            let edges = tx.create_tree(Keys::Prefix)?.id();
            let adj = tx.create_tree(Keys::Fields)?.id();
            Ok(Definition::Graph(GraphDef {
                nodes,
                edges,
                adj,
                graph_type: graph_type.unwrap_or(0),
            }))
        })
    }

    /// Drops the object `name` of `parent`: its entries get a delete stamp,
    /// and a graph's trees are dropped with it, so garbage collection frees
    /// them once no older snapshot remains (TS-17 Dropping a graph). A
    /// directory or schema must have no child, and a graph type no graph
    /// that uses it. Returns whether the object existed.
    ///
    /// # Errors
    ///
    /// [`Error::NotEmpty`] for a directory or schema with a child;
    /// [`Error::GraphTypeInUse`] for a graph type that a graph uses;
    /// [`Error::Serialization`] when a concurrent transaction committed a
    /// change of the name, or a child of the object or a graph of the
    /// graph type that is not dropped, after the snapshot;
    /// the errors of [`Transaction::lock_resource`],
    /// [`Transaction::delete`] and
    /// [`Transaction::drop_tree`].
    pub(crate) fn drop_object(&mut self, parent: u64, name: &str) -> Result<bool> {
        self.check_active()?;
        let t = self.catalog_tree()?;
        let key = name_key(parent, name);
        self.lock_key(t, &key, Mode::X)?;
        let Some(o) = self.lookup(parent, name)? else {
            return Ok(false);
        };
        match o.kind {
            Kind::Directory | Kind::Schema => {
                if !self.children(o.id)?.is_empty() {
                    return Err(Error::NotEmpty);
                }
                // Creators of children hold the S lock on this name until
                // they end, so the children created are final; a child
                // created after the snapshot fails this transaction unless
                // its drop committed (`none_created_later`).
                self.none_created_later(t, &children_prefix(o.id), |_, _| true)?;
            }
            Kind::Graph => {
                // Its indexes and constraints go with it (FR-IDX-003).
                self.drop_graph_schema(o.id)?;
                if let Definition::Graph(g) = o.definition {
                    for id in [g.nodes, g.edges, g.adj] {
                        let tree = self.tree(id)?;
                        self.drop_tree(tree)?;
                    }
                    self.graphs.retain(|gr: &Graph| gr.id != o.id);
                    self.dropped_graphs.push(o.id);
                }
            }
            Kind::GraphType => self.graph_type_unused(t, o.id)?,
        }
        self.delete(t, &key)?;
        self.delete(t, &reverse_key(o.id))?;
        Ok(true)
    }

    /// Fails unless no graph uses graph type `id`: none the snapshot sees
    /// with this transaction's changes ([`Error::GraphTypeInUse`]), and,
    /// with the X lock on the graph type's name held, none in the latest
    /// committed state committed after the snapshot
    /// ([`Error::Serialization`]; module documentation).
    fn graph_type_unused(&mut self, t: Tree, id: u64) -> Result<()> {
        let uses = |k: &[u8], v: &[u8]| {
            matches!(decode_object(k, v), Some(Object {
                definition: Definition::Graph(GraphDef { graph_type, .. }),
                ..
            }) if graph_type == id)
        };
        let mut used = false;
        self.scan_prefix(t, &[NAME], |k, v| {
            used = uses(k, v);
            !used
        })?;
        if used {
            return Err(Error::GraphTypeInUse);
        }
        self.none_created_later(t, &[NAME], uses)
    }

    /// Fails the transaction with [`Error::Serialization`] if the latest
    /// committed state holds an entry under `prefix` that `matches`
    /// selects and that was created after the snapshot, unless its drop
    /// has committed. The scan reads without the commit latch and can see
    /// a drop half applied, which may still be undone (TS-16 step 6). So
    /// for each such entry it takes the S lock on its key, which waits for
    /// any dropper of that name to end, and reads the entry again: still
    /// delete-stamped (or gone), its creation and drop both follow the
    /// snapshot and a serial order exists. The caller holds a lock that
    /// excludes new matching creations (module documentation), so no
    /// matching entry appears meanwhile.
    fn none_created_later(
        &mut self,
        t: Tree,
        prefix: &[u8],
        matches: impl Fn(&[u8], &[u8]) -> bool,
    ) -> Result<()> {
        let snapshot = self.snapshot();
        let mut later = Vec::new();
        let r = self
            .engine()
            .store()
            .scan_prefix_stamped(t, prefix, |k, v, st| {
                if st.created > snapshot && matches(k, v) {
                    later.push(k.to_vec());
                }
                true
            });
        if let Err(e) = r {
            return Err(self.abort_with(e));
        }
        for key in later {
            self.lock_resource(
                Resource::Key {
                    tree: t.id(),
                    key: key.as_slice().into(),
                },
                Mode::S,
            )?;
            let mut v = Vec::new();
            let st = match self.engine().store().get_stamped(t, &key, &mut v) {
                Ok(st) => st,
                Err(e) => return Err(self.abort_with(e)),
            };
            // A name dropped and created again after the snapshot is a new
            // entry: it fails only if it matches too.
            if st.is_some_and(|st| st.deleted == 0) && matches(&key, &v) {
                return Err(self.abort_with(Error::Serialization));
            }
        }
        Ok(())
    }

    /// The identifier of the token `(kind, name)`, created if missing
    /// ([`Engine::token`]): its own commit, which stays whether this
    /// transaction commits or aborts.
    ///
    /// # Errors
    ///
    /// As [`Engine::token`].
    pub(crate) fn token(&mut self, kind: TokenKind, name: &str) -> Result<u64> {
        self.check_active()?;
        self.engine().token(kind, name)
    }
}
