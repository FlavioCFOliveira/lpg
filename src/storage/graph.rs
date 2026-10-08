//! The graph layout (TS-06): each graph's node, edge and adjacency trees,
//! the crate-internal API that writes them through transactions, the
//! statistics they feed (TS-17) and their cross-tree integrity check
//! (TS-12).
//!
//! **Trees** (created with the graph, [`crate::catalog`]):
//! - node tree: node identifier (8 bytes, big-endian) → record (TS-21
//!   Record encoding: label tokens and properties); a mutable record whose
//!   `created_ts` field holds its `version_ts` (TS-06 Stamps);
//! - edge tree: edge identifier → source, target and edge type (8 bytes
//!   big-endian each), then a record of the edge's properties, with no
//!   label;
//! - adjacency tree: `(node, direction, edge type, neighbour, edge)`
//!   ([`ADJ_KEY_LEN`] bytes, [`adj_key`]) → empty value, two immutable
//!   entries per edge, OUT at the source and IN at the target, in
//!   field-delta leaves ([`crate::storage::btree::Keys::Fields`]).
//!
//! **Identifiers** are drawn from the database's element counter when the
//! write that needs them enters the write set, and never reused (TS-07,
//! BR-TYPE-001).
//!
//! **Locks** (TS-15 Lock table). Every write takes the graph's S lock and
//! the S schema lock of each label or edge type it writes; a node's labels
//! before and after a change both count, as Neo4j takes shared label locks
//! for the labels it adds and removes (R-NEO
//! `community/kernel/.../newapi/Operations.java:1379-1383`). Node content X
//! and edge X are the X locks a write takes on the record's key. A node
//! delete takes the node's existence X; an edge create or delete takes
//! existence S at each endpoint, the lower node identifier first, as Neo4j
//! orders its endpoint locks (R-NEO
//! `community/record-storage-engine/.../RecordStorageLocks.java:179-188`).
//! Write-write rule: an endpoint whose `deleted_ts` is above the snapshot
//! fails the transaction with 25N11.
//!
//! **Node deletion under snapshot isolation.** An edge creator holds only
//! existence S until it ends, so a node deleter that waited for it, then
//! checks its snapshot, would not see the new edge and would leave it
//! dangling. With the existence X held, no edge at the node can commit any
//! more, so the deleter reads the node's latest committed adjacency
//! entries: a live one committed after its snapshot fails it with 25N11.
//! PostgreSQL's referential check does the same in a transaction-snapshot
//! transaction: it reads the latest committed rows and errors out on one
//! its snapshot does not see (R-PG @ `1f22453`
//! `src/backend/utils/adt/ri_triggers.c:2683-2699`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::catalog::stats::{Delta, StatKey};
use crate::catalog::{Definition, Kind, TokenKind};
use crate::index::{Element, Entity};
use crate::storage::btree::node::ADJ_KEY_LEN;
use crate::storage::btree::{Store, Tree};
use crate::storage::engine::{Core, Engine};
use crate::storage::error::{Error, Result};
use crate::tx::Transaction;
use crate::tx::lock::{Mode, Resource};
use crate::types::Value;
use crate::types::record;

#[cfg(test)]
mod tests;

/// The direction of an adjacency entry at its node: OUT at an edge's
/// source.
pub(crate) const OUT: u8 = 0;
/// The direction of an adjacency entry at its node: IN at an edge's
/// target.
pub(crate) const IN: u8 = 1;

/// The trees of one graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Graph {
    /// The graph's catalog object identifier.
    pub(crate) id: u64,
    /// The node tree.
    pub(crate) nodes: Tree,
    /// The edge tree.
    pub(crate) edges: Tree,
    /// The adjacency tree.
    pub(crate) adj: Tree,
}

/// One adjacency entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Adj {
    /// The node the entry is at.
    pub(crate) node: u64,
    /// [`OUT`] or [`IN`].
    pub(crate) dir: u8,
    /// The edge type.
    pub(crate) ty: u64,
    /// The edge's other endpoint.
    pub(crate) nbr: u64,
    /// The edge.
    pub(crate) edge: u64,
}

/// An edge record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeRecord {
    /// The source node.
    pub(crate) src: u64,
    /// The target node.
    pub(crate) dst: u64,
    /// The edge type.
    pub(crate) ty: u64,
    /// The properties, by key token.
    pub(crate) props: Vec<(u64, Value)>,
}

/// A node's labels and properties.
pub(crate) type NodeRecord = (Vec<u64>, Vec<(u64, Value)>);

/// The key of an element in the node or edge tree.
pub(crate) fn element_key(id: u64) -> [u8; 8] {
    id.to_be_bytes()
}

/// The element identifier of a node- or edge-tree key.
pub(crate) fn element_of(k: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(k.try_into().ok()?))
}

/// The adjacency key of `a`.
pub(crate) fn adj_key(a: Adj) -> [u8; ADJ_KEY_LEN] {
    let mut k = [0u8; ADJ_KEY_LEN];
    k[0..8].copy_from_slice(&a.node.to_be_bytes());
    k[8] = a.dir;
    k[9..17].copy_from_slice(&a.ty.to_be_bytes());
    k[17..25].copy_from_slice(&a.nbr.to_be_bytes());
    k[25..33].copy_from_slice(&a.edge.to_be_bytes());
    k
}

/// The adjacency entry of key `k`; `None` if malformed.
pub(crate) fn decode_adj(k: &[u8]) -> Option<Adj> {
    if k.len() != ADJ_KEY_LEN || k[8] > IN {
        return None;
    }
    let be = |at: usize| u64::from_be_bytes(k[at..at + 8].try_into().expect("8 bytes"));
    Some(Adj {
        node: be(0),
        dir: k[8],
        ty: be(9),
        nbr: be(17),
        edge: be(25),
    })
}

/// The adjacency prefix of `node`, of its entries in `dir`, of its
/// entries in `dir` of type `ty` (TS-06 Expansion).
fn adj_prefix(node: u64, dir: Option<u8>, ty: Option<u64>) -> Vec<u8> {
    let mut p = node.to_be_bytes().to_vec();
    if let Some(d) = dir {
        p.push(d);
        if let Some(t) = ty {
            p.extend_from_slice(&t.to_be_bytes());
        }
    }
    p
}

fn too_large(_: record::TooLarge) -> Error {
    Error::InvalidArgument("a record too large or with a value out of range")
}

fn encode_node(labels: &[u64], props: &[(u64, Value)]) -> Result<Vec<u8>> {
    let mut v = Vec::new();
    record::encode(labels, props, &mut v).map_err(too_large)?;
    Ok(v)
}

fn encode_edge(src: u64, dst: u64, ty: u64, props: &[(u64, Value)]) -> Result<Vec<u8>> {
    let mut v = Vec::with_capacity(32);
    v.extend_from_slice(&src.to_be_bytes());
    v.extend_from_slice(&dst.to_be_bytes());
    v.extend_from_slice(&ty.to_be_bytes());
    record::encode(&[], props, &mut v).map_err(too_large)?;
    Ok(v)
}

fn edge_ends(v: &[u8]) -> Option<(u64, u64, u64)> {
    let be = |at: usize| Some(u64::from_be_bytes(v.get(at..at + 8)?.try_into().ok()?));
    Some((be(0)?, be(8)?, be(16)?))
}

/// The edge record of an edge-tree value; `None` if malformed.
pub(crate) fn decode_edge(v: &[u8]) -> Option<EdgeRecord> {
    let (src, dst, ty) = edge_ends(v)?;
    let (labels, props) = record::decode(&v[24..])?;
    labels.is_empty().then_some(EdgeRecord {
        src,
        dst,
        ty,
        props,
    })
}

/// The element a node write gives the constraint checks ([`crate::index`]).
fn node_element(labels: &[u64], props: &[(u64, Value)]) -> Element {
    Element {
        tokens: labels.to_vec(),
        props: props.to_vec(),
    }
}

/// The element an edge write gives the constraint checks.
fn edge_element(ty: u64, props: &[(u64, Value)]) -> Element {
    Element {
        tokens: vec![ty],
        props: props.to_vec(),
    }
}

fn corrupt(what: &str) -> Error {
    Error::Corruption(what.into())
}

impl Transaction<'_> {
    /// The trees of graph `id`, as the snapshot sees its catalog entry.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if the snapshot sees no graph `id`; errors of
    /// [`Transaction::object`].
    pub(crate) fn graph(&mut self, id: u64) -> Result<Graph> {
        let Some(o) = self.object(id)? else {
            return Err(Error::NotFound("the graph"));
        };
        let (Kind::Graph, Definition::Graph(d)) = (o.kind, o.definition) else {
            return Err(Error::NotFound("the graph"));
        };
        Ok(Graph {
            id,
            nodes: self.tree(d.nodes)?,
            edges: self.tree(d.edges)?,
            adj: self.tree(d.adj)?,
        })
    }

    /// Takes the graph S lock of a writer of `g` and records `g` for the
    /// statistics of the commit.
    fn graph_writer(&mut self, g: &Graph) -> Result<()> {
        self.lock_resource(Resource::Graph(g.id), Mode::S)?;
        if !self.graphs.iter().any(|x| x.id == g.id) {
            self.graphs.push(*g);
        }
        Ok(())
    }

    /// Fails with [`Error::InvalidArgument`] unless every identifier of
    /// `ids` is a token of `kind`: a label or edge type is stored by its
    /// token, drawn from the token table (TS-17 Tokens), as Neo4j resolves
    /// a token identifier only through its token holder, which throws for
    /// an unknown one (R-NEO `community/token-api/.../TokenHolder.java:77`).
    fn tokens_are(&self, kind: TokenKind, ids: &[u64]) -> Result<()> {
        let tokens = &self.engine().core().catalog.tokens;
        if ids.iter().all(|&id| tokens.kind(id) == Some(kind)) {
            Ok(())
        } else {
            Err(Error::InvalidArgument(match kind {
                TokenKind::Label => "a label that is not a label token",
                TokenKind::EdgeType => "an edge type that is not an edge-type token",
                TokenKind::PropertyKey => "a property key that is not a property-key token",
            }))
        }
    }

    /// Takes the S schema lock of each label or edge type of `tokens`.
    fn schema(&mut self, g: &Graph, tokens: &[u64]) -> Result<()> {
        for &token in tokens {
            self.lock_resource(Resource::Schema { graph: g.id, token }, Mode::S)?;
        }
        Ok(())
    }

    /// Takes the existence lock of `node` in `mode`, then applies the
    /// write-write rule: a `deleted_ts` above the snapshot fails the
    /// transaction with [`Error::Serialization`]; label and property
    /// changes do not count (TS-15).
    fn existence(&mut self, g: &Graph, node: u64, mode: Mode) -> Result<()> {
        self.lock_resource(Resource::Existence { graph: g.id, node }, mode)?;
        if g.nodes.is_pending() {
            return Ok(());
        }
        let mut v = Vec::new();
        match self
            .engine()
            .store()
            .get_stamped(g.nodes, &element_key(node), &mut v)
        {
            Ok(Some(st)) if st.deleted > self.snapshot() => {
                Err(self.abort_with(Error::Serialization))
            }
            Ok(_) => Ok(()),
            Err(e) => Err(self.abort_with(e)),
        }
    }

    /// Creates a node of `g` with `labels` and `props` and returns its
    /// identifier.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a label that is not a label token or
    /// a record too large to encode; the errors of [`Transaction::put`].
    pub(crate) fn create_node(
        &mut self,
        g: &Graph,
        labels: &[u64],
        props: &[(u64, Value)],
    ) -> Result<u64> {
        self.check_active()?;
        self.tokens_are(TokenKind::Label, labels)?;
        let v = encode_node(labels, props)?;
        self.graph_writer(g)?;
        self.schema(g, labels)?;
        let id = match self.engine().core().ids.element() {
            Ok(id) => id,
            Err(e) => return Err(self.abort_with(e)),
        };
        self.unique_values(g, Entity::Node, id, &node_element(labels, props))?;
        self.put(g.nodes, &element_key(id), &v)?;
        Ok(id)
    }

    /// Node `id` of `g` as the snapshot sees it with this transaction's
    /// changes: its labels and properties.
    ///
    /// # Errors
    ///
    /// Errors of [`Transaction::get`]; [`Error::Corruption`] for a
    /// malformed record.
    pub(crate) fn node(&mut self, g: &Graph, id: u64) -> Result<Option<NodeRecord>> {
        match self.get(g.nodes, &element_key(id))? {
            None => Ok(None),
            Some(v) => record::decode(&v)
                .map(Some)
                .ok_or_else(|| corrupt("malformed node record")),
        }
    }

    /// Replaces the labels and properties of node `id` of `g` (FR-TYPE-003:
    /// its identifier stays).
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if the snapshot sees no such node; as
    /// [`Transaction::create_node`].
    pub(crate) fn set_node(
        &mut self,
        g: &Graph,
        id: u64,
        labels: &[u64],
        props: &[(u64, Value)],
    ) -> Result<()> {
        self.check_active()?;
        self.tokens_are(TokenKind::Label, labels)?;
        let v = encode_node(labels, props)?;
        self.graph_writer(g)?;
        let Some((old, _)) = self.node(g, id)? else {
            return Err(Error::NotFound("the node"));
        };
        let both: BTreeSet<u64> = old.iter().chain(labels).copied().collect();
        self.schema(g, &both.into_iter().collect::<Vec<_>>())?;
        self.unique_values(g, Entity::Node, id, &node_element(labels, props))?;
        self.put(g.nodes, &element_key(id), &v)
    }

    /// Changes node `id` of `g` with `f`, applied to its labels and
    /// properties.
    fn change_node(
        &mut self,
        g: &Graph,
        id: u64,
        f: impl FnOnce(&mut Vec<u64>, &mut Vec<(u64, Value)>),
    ) -> Result<()> {
        let Some((mut labels, mut props)) = self.node(g, id)? else {
            return Err(Error::NotFound("the node"));
        };
        f(&mut labels, &mut props);
        self.set_node(g, id, &labels, &props)
    }

    /// Adds label `label` to node `id` of `g`.
    ///
    /// # Errors
    ///
    /// As [`Transaction::set_node`].
    pub(crate) fn add_label(&mut self, g: &Graph, id: u64, label: u64) -> Result<()> {
        self.change_node(g, id, |l, _| l.push(label))
    }

    /// Removes label `label` from node `id` of `g`.
    ///
    /// # Errors
    ///
    /// As [`Transaction::set_node`].
    pub(crate) fn remove_label(&mut self, g: &Graph, id: u64, label: u64) -> Result<()> {
        self.change_node(g, id, |l, _| l.retain(|x| *x != label))
    }

    /// Sets property `key` of node `id` of `g` to `value`.
    ///
    /// # Errors
    ///
    /// As [`Transaction::set_node`].
    pub(crate) fn set_property(
        &mut self,
        g: &Graph,
        id: u64,
        key: u64,
        value: Value,
    ) -> Result<()> {
        self.change_node(g, id, |_, p| {
            p.retain(|(k, _)| *k != key);
            p.push((key, value));
        })
    }

    /// Removes property `key` of node `id` of `g`.
    ///
    /// # Errors
    ///
    /// As [`Transaction::set_node`].
    pub(crate) fn remove_property(&mut self, g: &Graph, id: u64, key: u64) -> Result<()> {
        self.change_node(g, id, |_, p| p.retain(|(k, _)| *k != key))
    }

    /// Deletes node `id` of `g`. A node with edges is deleted only with
    /// `detach`, which deletes its edges in the same call. Returns whether
    /// the snapshot saw the node.
    ///
    /// # Errors
    ///
    /// [`Error::EdgesStillExist`] (GQLSTATUS G1001) for a node with edges
    /// without `detach`; [`Error::Serialization`] when an edge at the node
    /// committed after the snapshot (module documentation); the errors of
    /// [`Transaction::delete`].
    pub(crate) fn delete_node(&mut self, g: &Graph, id: u64, detach: bool) -> Result<bool> {
        self.check_active()?;
        self.graph_writer(g)?;
        self.existence(g, id, Mode::X)?;
        let Some((labels, _)) = self.node(g, id)? else {
            return Ok(false);
        };
        self.schema(g, &labels)?;
        let mut edges = BTreeSet::new();
        self.scan_prefix(g.adj, &id.to_be_bytes(), |k, _| {
            if let Some(a) = decode_adj(k) {
                edges.insert(a.edge);
            }
            true
        })?;
        if !edges.is_empty() {
            if !detach {
                return Err(Error::EdgesStillExist);
            }
            for e in edges {
                self.delete_edge(g, e)?;
            }
        }
        if !g.adj.is_pending() {
            let snapshot = self.snapshot();
            let mut later = false;
            let r =
                self.engine()
                    .store()
                    .scan_prefix_stamped(g.adj, &id.to_be_bytes(), |_, _, st| {
                        later = st.deleted == 0 && st.created > snapshot;
                        !later
                    });
            if let Err(e) = r {
                return Err(self.abort_with(e));
            }
            if later {
                return Err(self.abort_with(Error::Serialization));
            }
        }
        self.delete(g.nodes, &element_key(id))
    }

    /// Creates an edge of type `ty` from `src` to `dst` in `g`, with
    /// `props`, and returns its identifier.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] for an endpoint the snapshot does not see;
    /// [`Error::Serialization`] for an endpoint deleted after the snapshot;
    /// [`Error::InvalidArgument`] for an edge type that is not an
    /// edge-type token; as [`Transaction::create_node`].
    pub(crate) fn create_edge(
        &mut self,
        g: &Graph,
        src: u64,
        dst: u64,
        ty: u64,
        props: &[(u64, Value)],
    ) -> Result<u64> {
        self.check_active()?;
        self.tokens_are(TokenKind::EdgeType, &[ty])?;
        let v = encode_edge(src, dst, ty, props)?;
        self.graph_writer(g)?;
        self.schema(g, &[ty])?;
        for n in [src.min(dst), src.max(dst)] {
            self.existence(g, n, Mode::S)?;
        }
        // Under the existence locks: no read is recorded, so a concurrent
        // label or property change of an endpoint fails nothing (TS-15).
        for n in [src, dst] {
            if self.get_locked(g.nodes, &element_key(n))?.is_none() {
                return Err(Error::NotFound("an endpoint node"));
            }
        }
        let id = match self.engine().core().ids.element() {
            Ok(id) => id,
            Err(e) => return Err(self.abort_with(e)),
        };
        self.unique_values(g, Entity::Edge, id, &edge_element(ty, props))?;
        self.put(g.edges, &element_key(id), &v)?;
        for (node, dir, nbr) in [(src, OUT, dst), (dst, IN, src)] {
            let k = adj_key(Adj {
                node,
                dir,
                ty,
                nbr,
                edge: id,
            });
            self.put(g.adj, &k, &[])?;
        }
        Ok(id)
    }

    /// Edge `id` of `g` as the snapshot sees it with this transaction's
    /// changes.
    ///
    /// # Errors
    ///
    /// As [`Transaction::node`].
    pub(crate) fn edge(&mut self, g: &Graph, id: u64) -> Result<Option<EdgeRecord>> {
        match self.get(g.edges, &element_key(id))? {
            None => Ok(None),
            Some(v) => decode_edge(&v)
                .map(Some)
                .ok_or_else(|| corrupt("malformed edge record")),
        }
    }

    /// Replaces the properties of edge `id` of `g`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] if the snapshot sees no such edge; as
    /// [`Transaction::create_node`].
    pub(crate) fn set_edge(&mut self, g: &Graph, id: u64, props: &[(u64, Value)]) -> Result<()> {
        self.check_active()?;
        self.graph_writer(g)?;
        let Some(e) = self.edge(g, id)? else {
            return Err(Error::NotFound("the edge"));
        };
        let v = encode_edge(e.src, e.dst, e.ty, props)?;
        self.schema(g, &[e.ty])?;
        self.unique_values(g, Entity::Edge, id, &edge_element(e.ty, props))?;
        self.put(g.edges, &element_key(id), &v)
    }

    /// Deletes edge `id` of `g` and its two adjacency entries. Returns
    /// whether the snapshot saw the edge.
    ///
    /// # Errors
    ///
    /// [`Error::Serialization`] for an endpoint deleted, or an edge
    /// changed, after the snapshot; the errors of [`Transaction::delete`].
    pub(crate) fn delete_edge(&mut self, g: &Graph, id: u64) -> Result<bool> {
        self.check_active()?;
        self.graph_writer(g)?;
        let Some(e) = self.edge(g, id)? else {
            return Ok(false);
        };
        self.schema(g, &[e.ty])?;
        for n in [e.src.min(e.dst), e.src.max(e.dst)] {
            self.existence(g, n, Mode::S)?;
        }
        self.delete(g.edges, &element_key(id))?;
        for (node, dir, nbr) in [(e.src, OUT, e.dst), (e.dst, IN, e.src)] {
            let k = adj_key(Adj {
                node,
                dir,
                ty: e.ty,
                nbr,
                edge: id,
            });
            self.delete(g.adj, &k)?;
        }
        Ok(true)
    }

    /// The adjacency entries of `node` in `g` in direction `dir`, of type
    /// `ty` or of every type, as the snapshot sees them with this
    /// transaction's changes: one descent and a scan of the prefix (TS-06
    /// Expansion).
    ///
    /// # Errors
    ///
    /// Errors of [`Transaction::scan_prefix`]; [`Error::Corruption`] for a
    /// malformed key.
    pub(crate) fn expand(
        &mut self,
        g: &Graph,
        node: u64,
        dir: u8,
        ty: Option<u64>,
    ) -> Result<Vec<Adj>> {
        let mut out = Vec::new();
        let mut bad = false;
        self.scan_prefix(g.adj, &adj_prefix(node, Some(dir), ty), |k, _| {
            match decode_adj(k) {
                Some(a) => out.push(a),
                None => bad = true,
            }
            true
        })?;
        if bad {
            return Err(corrupt("malformed adjacency key"));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Statistics (TS-17).

/// The labels of node `node` in the latest committed state (none for an
/// absent or deleted node), read under the commit latch.
fn committed_labels(
    store: &Store<'_>,
    nodes: Tree,
    node: u64,
    cache: &mut HashMap<u64, Vec<u64>>,
) -> Result<Vec<u64>> {
    if let Some(l) = cache.get(&node) {
        return Ok(l.clone());
    }
    let mut labels = Vec::new();
    if !nodes.is_pending() {
        let mut v = Vec::new();
        if let Some(st) = store.get_stamped(nodes, &element_key(node), &mut v)?
            && st.deleted == 0
        {
            labels = record::decode_labels(&v).ok_or_else(|| corrupt("malformed node record"))?;
        }
    }
    cache.insert(node, labels.clone());
    Ok(labels)
}

fn add_edges(d: &mut Delta, graph: u64, labels: &[u64], a: &Adj, by: i64) {
    for &label in labels {
        d.add(
            StatKey::Edges {
                graph,
                label,
                ty: a.ty,
                dir: a.dir,
            },
            by,
        );
    }
}

/// The statistics changes of `tx`'s commit (TS-17 Statistics), read under
/// the commit latch from its write set and the latest committed state,
/// before its pages change: node counts move from each written node's
/// labels before the commit to its labels after; an adjacency entry the
/// commit creates counts with its node's labels after, one it deletes with
/// its node's labels before; and every other adjacency entry of a node
/// whose labels change moves from the labels before to the labels after,
/// read from the node's adjacency prefix inside the serial section.
///
/// # Errors
///
/// Storage errors; [`Error::Corruption`] for a malformed committed record;
/// [`Error::InvalidArgument`] for a malformed record or key in the write
/// set.
pub(crate) fn stats_delta(_core: &Core, store: &Store<'_>, tx: &Transaction<'_>) -> Result<Delta> {
    let mut d = Delta::default();
    for g in &tx.graphs {
        let writes = |t: Tree| {
            tx.writes
                .range((t.id(), Vec::new())..)
                .take_while(move |((id, _), _)| *id == t.id())
        };
        let mut before: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut after: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut cache = HashMap::new();
        for ((_, key), p) in writes(g.nodes) {
            let node = element_of(key).ok_or(Error::InvalidArgument("a malformed node key"))?;
            let b = committed_labels(store, g.nodes, node, &mut cache)?;
            let a = match &p.value {
                Some(v) => record::decode_labels(v)
                    .ok_or(Error::InvalidArgument("a malformed node record"))?,
                None => Vec::new(),
            };
            for &label in &b {
                d.add(StatKey::Nodes { graph: g.id, label }, -1);
            }
            for &label in &a {
                d.add(StatKey::Nodes { graph: g.id, label }, 1);
            }
            before.insert(node, b);
            after.insert(node, a);
        }
        let mut own = HashSet::new();
        for ((_, key), p) in writes(g.adj) {
            let a = decode_adj(key).ok_or(Error::InvalidArgument("a malformed adjacency key"))?;
            own.insert(key.as_slice());
            let labels = match (&p.value, after.get(&a.node), before.get(&a.node)) {
                (Some(_), Some(l), _) | (None, _, Some(l)) => l.clone(),
                _ => committed_labels(store, g.nodes, a.node, &mut cache)?,
            };
            add_edges(
                &mut d,
                g.id,
                &labels,
                &a,
                if p.value.is_some() { 1 } else { -1 },
            );
        }
        if g.adj.is_pending() {
            continue;
        }
        for (node, b) in &before {
            let a = &after[node];
            if a == b {
                continue;
            }
            let mut bad = false;
            store.scan_prefix_stamped(g.adj, &node.to_be_bytes(), |k, _, st| {
                if st.deleted == 0 && !own.contains(k) {
                    match decode_adj(k) {
                        Some(e) => {
                            add_edges(&mut d, g.id, b, &e, -1);
                            add_edges(&mut d, g.id, a, &e, 1);
                        }
                        None => bad = true,
                    }
                }
                true
            })?;
            if bad {
                return Err(corrupt("malformed adjacency key"));
            }
        }
    }
    Ok(d)
}

/// The exact statistics of the latest committed state: one scan of each
/// live graph's node tree and of the adjacency prefix of each labelled
/// node, and one of each ONLINE index (TS-17 After a crash). Runs at open, before any transaction, so it
/// needs no snapshot.
///
/// # Errors
///
/// Storage errors; [`Error::Corruption`] for a malformed record or key or
/// a graph tree that the directory does not list.
pub(crate) fn recount(core: &Core) -> Result<BTreeMap<StatKey, u64>> {
    let store = core.store();
    let graphs: Vec<(u64, u64, u64)> = core
        .catalog
        .objects()
        .values()
        .filter_map(|o| match &o.definition {
            Definition::Graph(d) => Some((o.id, d.nodes, d.adj)),
            _ => None,
        })
        .collect();
    let mut counts: BTreeMap<StatKey, u64> = BTreeMap::new();
    let tree = |id| {
        store
            .tree(id)?
            .ok_or_else(|| corrupt("a graph tree is missing"))
    };
    for (graph, nodes, adj) in graphs {
        let (nodes, adj) = (tree(nodes)?, tree(adj)?);
        let mut err: Option<Error> = None;
        store.scan_prefix_stamped(nodes, b"", |k, v, st| {
            if st.deleted != 0 {
                return true;
            }
            let Some(labels) = record::decode_labels(v) else {
                err = Some(corrupt("malformed node record"));
                return false;
            };
            if labels.is_empty() {
                return true;
            }
            for &label in &labels {
                *counts.entry(StatKey::Nodes { graph, label }).or_insert(0) += 1;
            }
            let r = store.scan_prefix_stamped(adj, k, |ak, _, ast| {
                if ast.deleted != 0 {
                    return true;
                }
                let Some(a) = decode_adj(ak) else {
                    err = Some(corrupt("malformed adjacency key"));
                    return false;
                };
                for &label in &labels {
                    let key = StatKey::Edges {
                        graph,
                        label,
                        ty: a.ty,
                        dir: a.dir,
                    };
                    *counts.entry(key).or_insert(0) += 1;
                }
                true
            });
            if let Err(e) = r {
                err = Some(e);
            }
            err.is_none()
        })?;
        if let Some(e) = err {
            return Err(e);
        }
    }
    crate::index::recount(core, &mut counts)?;
    Ok(counts)
}

// ---------------------------------------------------------------------------
// Integrity (TS-12).

/// The problems a check reports, at most
/// [`crate::storage::check::MAX_PROBLEMS`] of them, as [`Store::check`].
#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn push(&mut self, msg: String) {
        if self.0.len() < crate::storage::check::MAX_PROBLEMS {
            self.0.push(msg);
        }
    }
}

/// The cross-tree checks of graph `gid` (see [`Engine::check_graphs`]):
/// one scan of the edge tree and one of the adjacency tree, each with
/// point lookups in the other trees, so the check holds one entry at a
/// time.
fn check_graph(store: &Store<'_>, gid: u64, g: &Graph, p: &mut Problems) -> Result<()> {
    let mut err: Option<Error> = None;
    let (mut a_val, mut n_val) = (Vec::new(), Vec::new());
    store.scan_prefix_stamped(g.edges, b"", |k, v, st| {
        let (Some(edge), Some((src, dst, ty))) = (element_of(k), edge_ends(v)) else {
            p.push(format!("graph {gid}: malformed edge entry {k:?}"));
            return true;
        };
        let r = (|| -> Result<()> {
            for (node, dir, nbr) in [(src, OUT, dst), (dst, IN, src)] {
                let a = Adj {
                    node,
                    dir,
                    ty,
                    nbr,
                    edge,
                };
                match store.get_stamped(g.adj, &adj_key(a), &mut a_val)? {
                    None => p.push(format!(
                        "graph {gid}: edge {edge} has no adjacency entry {a:?}"
                    )),
                    // TS-12: the delete stamps are equal (0: live).
                    Some(ast) if ast.deleted != st.deleted => p.push(format!(
                        "graph {gid}: edge {edge} deleted at {}, its entry {a:?} at {}",
                        st.deleted, ast.deleted
                    )),
                    Some(_) => {}
                }
            }
            for n in [src, dst].into_iter().take(if src == dst { 1 } else { 2 }) {
                let node = store.get_stamped(g.nodes, &element_key(n), &mut n_val)?;
                // A live edge needs a live endpoint. A deleted edge was
                // deleted no later than its endpoints (a node delete deletes
                // its edges or fails), and garbage collection may have
                // removed an endpoint deleted before it.
                let ok = match (st.deleted, node) {
                    (0, Some(ns)) => ns.deleted == 0,
                    (0, None) => false,
                    (_, None) => true,
                    (d, Some(ns)) => ns.deleted == 0 || ns.deleted >= d,
                };
                if !ok {
                    p.push(format!(
                        "graph {gid}: edge {edge} (deleted at {}) has endpoint {n}, \
                         which is not a node at that time ({node:?})",
                        st.deleted
                    ));
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            err = Some(e);
        }
        err.is_none()
    })?;
    if let Some(e) = err {
        return Err(e);
    }
    let mut e_val = Vec::new();
    store.scan_prefix_stamped(g.adj, b"", |k, _, _| {
        let Some(a) = decode_adj(k) else {
            p.push(format!("graph {gid}: malformed adjacency key {k:?}"));
            return true;
        };
        // The stamps of an entry whose edge matches were compared in the
        // edge scan.
        match store.get_stamped(g.edges, &element_key(a.edge), &mut e_val) {
            Ok(found) => {
                let ends = found.and_then(|_| edge_ends(&e_val));
                let matches = ends.is_some_and(|(src, dst, ty)| {
                    let (at, nbr) = if a.dir == OUT { (src, dst) } else { (dst, src) };
                    (at, nbr, ty) == (a.node, a.nbr, a.ty)
                });
                if !matches {
                    p.push(format!(
                        "graph {gid}: adjacency entry {a:?} belongs to no edge"
                    ));
                }
                true
            }
            Err(e) => {
                err = Some(e);
                false
            }
        }
    })?;
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl Engine {
    /// The cross-tree part of the integrity check (TS-12), over every
    /// graph of the latest committed catalog, under the commit latch, so
    /// no commit and no garbage-collection pass changes a tree while it
    /// runs, as [`Store::check`]. It reads every entry with its stamps:
    /// - each edge record has its OUT entry at its source and its IN entry
    ///   at its target, with its type, and their delete stamps equal the
    ///   record's, for every stamp present (0 for a live edge);
    /// - each adjacency entry belongs to an edge record that it matches;
    /// - a live edge's endpoints are live nodes of the graph; a deleted
    ///   edge's endpoint, if present, was not deleted before it;
    /// - each ONLINE index matches the data in both directions
    ///   ([`crate::index::check`]);
    /// - the statistics equal an exact recount ([`recount`]): a count that
    ///   a commit would take below zero is clamped
    ///   ([`crate::catalog::stats::Stats::apply`]) and reported here, as
    ///   Neo4j's consistency checker compares its counts store with the
    ///   counts it computes (R-NEO
    ///   `community/record-storage-engine/.../consistency/checker/CountsState.java:217-229`).
    ///
    /// It needs no snapshot: under the latch the trees hold exactly the
    /// latest committed state, and every version that any snapshot can
    /// still read. PostgreSQL's `bt_index_parent_check` likewise holds a
    /// lock that blocks every physical change of the heap and index for
    /// its whole run (R-PG @ `1f22453`
    /// `contrib/amcheck/verify_nbtree.c:278-302, 348-358`).
    ///
    /// Memory: each tree is scanned once and the other trees are probed by
    /// point lookups, one entry at a time, as SQLite's `integrity_check`
    /// probes each index for each row (R-SQLITE @ `97048e9`
    /// `src/pragma.c:2075-2095`); pages come from the buffer pool, within
    /// the cap. The problem list is bounded as [`Store::check`]'s; the
    /// recount holds one count per statistic, as the statistics do.
    ///
    /// Returns the problems found.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`]; storage errors, among them
    /// [`Error::MemoryLimit`] when the pool cannot hold a page.
    pub(crate) fn check_graphs(&self) -> Result<Vec<String>> {
        let core = self.core();
        core.health.check()?;
        let store = self.store();
        let _latch = store.lock();
        core.health.check()?;
        let graphs: Vec<(u64, crate::catalog::GraphDef)> = core
            .catalog
            .objects()
            .values()
            .filter_map(|o| match &o.definition {
                Definition::Graph(d) => Some((o.id, *d)),
                _ => None,
            })
            .collect();
        let mut p = Problems::default();
        for (gid, d) in graphs {
            let tree = |id| store.tree(id);
            let (Some(nodes), Some(edges), Some(adj)) =
                (tree(d.nodes)?, tree(d.edges)?, tree(d.adj)?)
            else {
                p.push(format!("graph {gid}: a tree of the graph is missing"));
                continue;
            };
            let g = Graph {
                id: gid,
                nodes,
                edges,
                adj,
            };
            check_graph(&store, gid, &g, &mut p)?;
        }
        crate::index::check(core, &mut |m| p.push(m))?;
        match recount(core) {
            Ok(want) => {
                let st = core.catalog.stats();
                let got = st.counts();
                for k in got.keys().chain(want.keys()).collect::<BTreeSet<_>>() {
                    let (a, b) = (got.get(k), want.get(k));
                    if a != b {
                        p.push(format!(
                            "statistic {k:?} is {}, a recount gives {}",
                            a.copied().unwrap_or(0),
                            b.copied().unwrap_or(0)
                        ));
                    }
                }
            }
            Err(Error::Corruption(m)) => p.push(format!("statistics recount: {m}")),
            Err(e) => return Err(e),
        }
        Ok(p.0)
    }

    /// The whole integrity check (TS-12): the page and tree checks of
    /// [`Store::check`], then [`Engine::check_graphs`]. Returns the
    /// problems found.
    ///
    /// # Errors
    ///
    /// As [`Store::check`] and [`Engine::check_graphs`].
    pub(crate) fn check_integrity(&self) -> Result<Vec<String>> {
        let mut problems = self.store().check()?.problems;
        problems.extend(self.check_graphs()?);
        Ok(problems)
    }
}
