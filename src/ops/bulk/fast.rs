//! The fast initial load (TS-26 steps 1 to 7; FR-IMP-002, FR-OPS-001,
//! NFR-PERF-005; rmp #58).
//!
//! 1. **Locks.** The backup-or-load latch is taken first and held for the
//!    whole load ([`crate::storage::engine::Core::backup_or_load`]): a load
//!    requested during a backup waits for it, and a backup requested during
//!    the load fails with 51N41. The load's transaction then takes the X
//!    lock on the graph's catalog name (no index or constraint is created
//!    and the graph is not dropped meanwhile) and the X graph lock, so
//!    writers of the graph wait; readers take no lock and keep reading the
//!    empty graph. The graph must be empty: no entry of its trees is
//!    visible to any snapshot that can still run.
//! 2. **Sorts** through [`Engine::sorter`], whose temporary files go
//!    through the storage trait and whose memory is one reservation each
//!    (TS-26 Memory): nodes by (id space, input identifier), which finds
//!    duplicates and assigns identifiers in that order; edges by start,
//!    merged with the sorted nodes to resolve starts, then by end; the
//!    adjacency keys (`adj_key`) and each index's keys (`index_keys`).
//!    Constraints are checked on the way: existence, type and node-key
//!    existence per record, uniqueness on adjacent sorted owned-index keys,
//!    as [`crate::index::build::UniqueSorted`] does (a port of PostgreSQL's
//!    `enforceUnique`). Each violation is an invalid record (FR-IMP-004).
//! 3. **Trees** bottom-up, filled to 90 %, into pages the builds own
//!    ([`Engine::bulk_build_pages_into`]): node, edge and adjacency trees.
//! 4. **Indexes**: every index defined on the graph, from its sorted keys.
//! 5. `sync_data` of the data file (each build).
//! 6. **Install**: one commit ([`Transaction::commit_installing_all`])
//!    installs every root into the graph's existing trees, raises the
//!    identifier counters, adds the statistics gathered during the build,
//!    and writes a new version of the graph's catalog entry; snapshots older
//!    than the commit read the loaded trees empty
//!    ([`crate::catalog::Loaded`]).
//! 7. The locks and the latch are released.
//!
//! A failure or crash before step 6 leaves the graph empty: the builds'
//! pages stay owned by their records, released at once on failure, or at
//! the next open after a crash (`src/storage/btree/bulk.rs`, own code).
//! References studied: PostgreSQL `tuplesort.c` and `nbtsort.c` (already
//! ported in Sprint 1), `copy.sgml:261-278` (`COPY FREEZE`).

use std::collections::{BTreeMap, HashSet};

use super::model::{
    Charge, Constraints, DUPLICATE_ID, ENTRY_OVERHEAD, Invalid, Pos, Report, SKIPPED_ENDPOINT,
    Source, Tokens, UNKNOWN_ENDPOINT, resolve,
};
use crate::catalog::stats::StatKey;
use crate::catalog::{CATALOG_TREE, Definition, TokenKind, name_key};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::index::build::{Phase, pause};
use crate::index::{Element, Entity, SchemaDef, State, index_keys, prefix_of};
use crate::storage::btree::bulk::SortedEntries;
use crate::storage::btree::node::{ADJ_KEY_LEN, put_varint};
use crate::storage::btree::{Keys, MAX_KEY, Store, Tree};
use crate::storage::engine::Engine;
use crate::storage::error::{Error as StorageError, Result as StorageResult, Violation};
use crate::storage::graph::{Adj, IN, OUT, adj_key};
use crate::storage::sort::{MIN_MEMORY, Sorted, Sorter};
use crate::sync::Arc;
use crate::tx::Isolation;
use crate::tx::lock::{Mode, Resource};
use crate::types::record;

/// The memory of each sort of a load: a sixty-fourth of the cap, between
/// 64 KiB and 16 MiB (TS-08). A load runs several sorts at once.
fn sort_memory(engine: &Engine) -> u64 {
    (engine.core().accountant.cap() / 64).clamp(MIN_MEMORY, 16 << 20)
}

/// Whether trees `trees` hold no entry that a snapshot at or above
/// `horizon` sees: every entry is delete-stamped at or below it.
fn trees_empty(store: &Store<'_>, trees: &[Tree], horizon: u64) -> StorageResult<bool> {
    for &t in trees {
        let mut found = false;
        store.scan_prefix_stamped(t, b"", |_, _, st| {
            found = st.deleted == 0 || st.deleted > horizon;
            !found
        })?;
        if found {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether graph `graph` of the latest committed catalog is empty to every
/// snapshot that can still run (TS-26 step 1).
///
/// # Errors
///
/// 22N51 for a missing graph; storage errors.
pub(crate) fn is_empty(engine: &Engine, graph: u64) -> Result<bool, Error> {
    let core = engine.core();
    let d = match core
        .catalog
        .objects()
        .get(&graph)
        .map(|o| o.definition.clone())
    {
        Some(Definition::Graph(d)) => d,
        _ => {
            return Err(Error::new(
                codes::C22N51,
                "graph reference not found: the graph does not exist",
            ));
        }
    };
    let store = engine.store();
    let mut trees = Vec::new();
    for id in [d.nodes, d.edges, d.adj] {
        if let Some(t) = store.tree(id)? {
            trees.push(t);
        }
    }
    let horizon = crate::tx::mvcc::horizon(&core.mvcc.snapshots, core.wal.progress.published_ts());
    Ok(trees_empty(&store, &trees, horizon)?)
}

fn not_empty() -> Error {
    Error::new(
        codes::C51N41,
        "admin operation failed: a fast initial load needs an empty graph",
    )
}

/// The report fields of a record kept in a sort: its ordinal in the input
/// (nodes and edges each counted from 1 in file order), file, place and
/// input identifier.
struct Meta {
    ord: u64,
    file: u32,
    pos: Pos,
    id: Option<String>,
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn put_meta(out: &mut Vec<u8>, m: &Meta) {
    put_varint(out, m.ord);
    put_varint(out, u64::from(m.file));
    match m.pos {
        Pos::Line(n) => {
            out.push(0);
            put_varint(out, n);
        }
        Pos::Ordinal(n) => {
            out.push(1);
            put_varint(out, n);
        }
        Pos::Dump(b, r) => {
            out.push(2);
            put_varint(out, b);
            put_varint(out, r);
        }
    }
    match &m.id {
        Some(id) => {
            out.push(1);
            put_bytes(out, id.as_bytes());
        }
        None => out.push(0),
    }
}

/// A reader of the fields this module writes into sort records.
struct Fields<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn new(b: &'a [u8]) -> Fields<'a> {
        Fields { b, at: 0 }
    }

    fn byte(&mut self) -> u8 {
        let v = self.b[self.at];
        self.at += 1;
        v
    }

    fn varint(&mut self) -> u64 {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.byte();
            v |= u64::from(b & 0x7F) << shift;
            if b < 0x80 {
                return v;
            }
            shift += 7;
        }
    }

    fn u64(&mut self) -> u64 {
        let v = u64::from_be_bytes(self.b[self.at..self.at + 8].try_into().expect("8 bytes"));
        self.at += 8;
        v
    }

    fn bytes(&mut self) -> &'a [u8] {
        let n = self.varint() as usize;
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        s
    }

    fn rest(&self) -> &'a [u8] {
        &self.b[self.at..]
    }

    fn meta(&mut self) -> Meta {
        let ord = self.varint();
        let file = self.varint() as u32;
        let pos = match self.byte() {
            0 => Pos::Line(self.varint()),
            1 => Pos::Ordinal(self.varint()),
            _ => {
                let b = self.varint();
                Pos::Dump(b, self.varint())
            }
        };
        let id = match self.byte() {
            0 => None,
            _ => Some(String::from_utf8_lossy(self.bytes()).into_owned()),
        };
        Meta { ord, file, pos, id }
    }
}

/// The sort key of an input identifier in its id space: the space's
/// length, the space and the identifier, so that equal pairs are equal
/// keys and nothing else is.
fn id_key(space: &str, id: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(5 + space.len() + id.len());
    k.push(0);
    k.extend_from_slice(&(space.len() as u32).to_be_bytes());
    k.extend_from_slice(space.as_bytes());
    k.extend_from_slice(id.as_bytes());
    k
}

/// The state of one load.
struct Load<'e, 'r, 's> {
    engine: &'e Engine,
    report: &'r mut Report<'s>,
    files: Vec<Arc<str>>,
    tokens: Tokens,
    constraints: Constraints,
    charge: Charge,
    /// Nodes and edges rejected after their identifiers were assigned (a
    /// uniqueness violation), charged to the cap.
    rejected: HashSet<u64>,
    memory: u64,
    /// Under abort: the invalid record of the phase with the lowest input
    /// ordinal, reported when the phase ends ([`Load::settle`]).
    first: Option<(u64, Invalid)>,
}

impl<'e> Load<'e, '_, '_> {
    fn sorter(&self) -> Result<Sorter<'e>, Error> {
        Ok(self.engine.sorter(self.memory)?)
    }

    fn file(&mut self, f: &Arc<str>) -> u32 {
        if let Some(i) = self
            .files
            .iter()
            .rposition(|x| Arc::ptr_eq(x, f) || **x == **f)
        {
            return i as u32;
        }
        self.files.push(f.clone());
        (self.files.len() - 1) as u32
    }

    /// An invalid record at input ordinal `ord`. With skip it is reported
    /// at once; with abort the load finds the phase's invalid records in
    /// sorted order, not in input order, so it keeps the one with the
    /// lowest ordinal and reports only that one when the phase ends: the
    /// record a transactional import of the same input would stop at
    /// (FR-IMP-004; rmp #58 FINDING 452).
    fn invalid_at(&mut self, ord: u64, inv: Invalid) -> Result<(), Error> {
        if !self.report.aborts() {
            return self.report.invalid(inv);
        }
        if self.first.as_ref().is_none_or(|(o, _)| ord < *o) {
            self.first = Some((ord, inv));
        }
        Ok(())
    }

    fn invalid(&mut self, m: &Meta, e: Error) -> Result<(), Error> {
        let inv = Invalid {
            file: self.files[m.file as usize].clone(),
            pos: m.pos,
            id: m.id.clone(),
            error: Box::new(e),
        };
        self.invalid_at(m.ord, inv)
    }

    /// Ends a phase: under abort, reports its first invalid record, which
    /// stops the load.
    fn settle(&mut self) -> Result<(), Error> {
        match self.first.take() {
            Some((_, inv)) => self.report.invalid(inv),
            None => Ok(()),
        }
    }

    /// Rejects element `id`; `false` if it was rejected already, so a
    /// record that breaks two constraints is reported once (rmp #58
    /// FINDING 455).
    fn reject(&mut self, id: u64) -> Result<bool, Error> {
        if self.rejected.contains(&id) {
            return Ok(false);
        }
        self.charge.add(8 + ENTRY_OVERHEAD)?;
        self.rejected.insert(id);
        Ok(true)
    }

    /// The uniqueness check of TS-26 step 2 with the semantics of the
    /// transactional import (rmp #58 DECISION 461): the candidates of
    /// `sorter`, keyed by input ordinal, are taken in input order; a
    /// record whose value of any uniqueness or node-key constraint an
    /// earlier accepted record holds is rejected, and a rejected record
    /// takes no value. This is the greedy fixpoint that repeated adjacent
    /// checks converge to, computed in one ordered pass; its set of taken
    /// values is charged to the cap, as the transactional import's is.
    fn check_unique(&mut self, sorter: Sorter<'_>) -> Result<(), Error> {
        let mut sorted = sorter.finish()?;
        let mut taken: HashSet<Vec<u8>> = HashSet::new();
        while let Some((_, v)) = sorted.next_record()? {
            let mut f = Fields::new(v);
            let id = f.u64();
            let m = f.meta();
            let mut keys = Vec::new();
            for _ in 0..f.varint() {
                keys.push(f.bytes().to_vec());
            }
            if keys.iter().any(|k| taken.contains(k)) {
                if self.reject(id)? {
                    self.invalid(
                        &m,
                        StorageError::ConstraintViolation(Violation::ValueInUse).into(),
                    )?;
                }
                continue;
            }
            for k in keys {
                self.charge.add(k.len() as u64 + ENTRY_OVERHEAD)?;
                taken.insert(k);
            }
        }
        Ok(())
    }
}

/// The candidate record of [`Load::check_unique`]: its element, report
/// fields and the value prefixes of its owned-index keys.
fn unique_candidate(id: u64, m: &Meta, keys: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut v = id.to_be_bytes().to_vec();
    put_meta(&mut v, m);
    put_varint(&mut v, keys.len() as u64);
    for (_, k) in keys {
        put_bytes(&mut v, prefix_of(k));
    }
    v
}

/// Loads `source` into the empty graph `graph` (module documentation),
/// reporting invalid records to `report`. Returns the nodes and edges
/// loaded.
///
/// # Errors
///
/// 51N41 if the graph is not empty or an index of it is being built; the
/// code of the invalid record that aborted the load; 22N05 for a malformed
/// header or a damaged dump; 51N72 when a sort's reservation does not fit
/// the cap; 50N00 for I/O failures, ENOSPC among them.
pub(crate) fn load(
    engine: &Engine,
    graph: u64,
    source: &mut dyn Source,
    report: &mut Report<'_>,
) -> Result<(u64, u64), Error> {
    let core = engine.core();
    // Step 1. Declared first, so that every build below is installed or
    // dropped while it is held.
    let _latch = core.backup_or_load();
    let mut tx = engine.begin(Isolation::Serializable)?;
    let Some(object) = tx.object(graph)? else {
        return Err(Error::new(
            codes::C22N51,
            "graph reference not found: the graph does not exist",
        ));
    };
    let cat = tx.tree(CATALOG_TREE)?;
    let name = name_key(object.parent, &object.name);
    tx.lock_key(cat, &name, Mode::X)?;
    tx.lock_resource(Resource::Graph(graph), Mode::X)?;
    let g = tx.graph(graph)?;
    let horizon = crate::tx::mvcc::horizon(&core.mvcc.snapshots, core.wal.progress.published_ts());
    if !trees_empty(&engine.store(), &[g.nodes, g.edges, g.adj], horizon)? {
        return Err(not_empty());
    }
    let mut indexes: Vec<SchemaDef> = {
        let schema = core.catalog.schema();
        if schema
            .values()
            .any(|d| d.graph == graph && d.kind.is_index() && d.state == State::Populating)
        {
            return Err(Error::new(
                codes::C51N41,
                "admin operation failed: an index of the graph is being built",
            ));
        }
        schema
            .values()
            .filter(|d| d.graph == graph && d.kind.is_index())
            .cloned()
            .collect()
    };
    indexes.sort_by_key(|d| d.id);
    pause(core, Phase::LoadLocked);
    let mut l = Load {
        engine,
        report,
        files: Vec::new(),
        tokens: Tokens::new(engine),
        constraints: Constraints::of(engine, graph),
        charge: Charge::new(engine),
        rejected: HashSet::new(),
        memory: sort_memory(engine),
        first: None,
    };
    // Step 2: nodes by (id space, input identifier).
    let mut by_id = l.sorter()?;
    let mut ordinal = 0u64;
    source.nodes(&mut |item| {
        ordinal += 1;
        let n = match item {
            Ok(n) => n,
            Err(inv) => return l.invalid_at(ordinal, inv),
        };
        let m = Meta {
            ord: ordinal,
            file: l.file(&n.file),
            pos: n.pos,
            id: n.id.clone(),
        };
        let r = match resolve(&mut l.tokens, TokenKind::Label, &n.labels, &n.props) {
            Ok(r) => r,
            Err((e, true)) => return Err(e),
            Err((e, false)) => return l.invalid(&m, e),
        };
        let mut key = match &n.id {
            Some(id) => id_key(&n.space, id),
            None => vec![1],
        };
        key.extend_from_slice(&ordinal.to_be_bytes());
        if key.len() > MAX_KEY {
            return l.invalid(
                &m,
                super::invalid_input("an input identifier longer than 1 000 bytes"),
            );
        }
        let mut v = Vec::new();
        put_meta(&mut v, &m);
        if record::encode(&r.tokens, &r.props, &mut v).is_err() {
            return l.invalid(&m, super::invalid_argument("a record too large to store"));
        }
        by_id.push(&key, &v)?;
        Ok(())
    })?;
    // Identifiers in sorted order; duplicates and per-record constraints.
    let mut nodes_by_id = l.sorter()?;
    let mut starts = l.sorter()?;
    let mut ends = l.sorter()?;
    let mut unique = if l.constraints.has_unique(Entity::Node) {
        Some(l.sorter()?)
    } else {
        None
    };
    {
        let mut sorted = by_id.finish()?;
        let mut last: Option<Vec<u8>> = None;
        while let Some((k, v)) = sorted.next_record()? {
            let ident = &k[..k.len() - 8];
            let mut f = Fields::new(v);
            let m = f.meta();
            let rec = f.rest();
            let has_id = ident.first() == Some(&0);
            if has_id && last.as_deref() == Some(ident) {
                l.invalid(
                    &m,
                    Error::new(
                        codes::C22N05,
                        format!("input failed validation: {DUPLICATE_ID}"),
                    ),
                )?;
                continue;
            }
            if has_id {
                last = Some(ident.to_vec());
            }
            let (labels, props) = record::decode(rec).expect("encoded above");
            let el = Element {
                tokens: labels,
                props,
            };
            if let Err(e) = l.constraints.check(Entity::Node, &el) {
                if has_id {
                    starts.push(ident, &u64::MAX.to_be_bytes())?;
                    ends.push(ident, &u64::MAX.to_be_bytes())?;
                }
                l.invalid(&m, e)?;
                continue;
            }
            let id = core.ids.element()?;
            if let Some(u) = unique.as_mut() {
                let keys = l.constraints.unique_keys(Entity::Node, id, &el);
                if !keys.is_empty() {
                    u.push(&m.ord.to_be_bytes(), &unique_candidate(id, &m, &keys))?;
                }
            }
            nodes_by_id.push(&id.to_be_bytes(), rec)?;
            if has_id {
                starts.push(ident, &id.to_be_bytes())?;
                ends.push(ident, &id.to_be_bytes())?;
            }
        }
    }
    if let Some(u) = unique.take() {
        l.check_unique(u)?;
    }
    l.settle()?;
    // Edges by start, merged with the nodes to resolve starts.
    let mut by_start = l.sorter()?;
    let mut ordinal = 0u64;
    source.edges(&mut |item| {
        ordinal += 1;
        let e = match item {
            Ok(e) => e,
            Err(inv) => return l.invalid_at(ordinal, inv),
        };
        let m = Meta {
            ord: ordinal,
            file: l.file(&e.file),
            pos: e.pos,
            id: e.id.clone(),
        };
        let r = match resolve(
            &mut l.tokens,
            TokenKind::EdgeType,
            std::slice::from_ref(&e.ty),
            &e.props,
        ) {
            Ok(r) => r,
            Err((err, true)) => return Err(err),
            Err((err, false)) => return l.invalid(&m, err),
        };
        let mut key = id_key(&e.start.0, &e.start.1);
        key.extend_from_slice(&ordinal.to_be_bytes());
        let end = id_key(&e.end.0, &e.end.1);
        if key.len() > MAX_KEY || end.len() + 8 > MAX_KEY {
            return l.invalid(
                &m,
                super::invalid_input("an input identifier longer than 1 000 bytes"),
            );
        }
        let mut v = Vec::new();
        put_meta(&mut v, &m);
        put_bytes(&mut v, &end);
        v.extend_from_slice(&r.tokens[0].to_be_bytes());
        if record::encode(&[], &r.props, &mut v).is_err() {
            return l.invalid(&m, super::invalid_argument("a record too large to store"));
        }
        by_start.push(&key, &v)?;
        Ok(())
    })?;
    let mut by_end = l.sorter()?;
    {
        let mut edges = by_start.finish()?;
        let mut map = starts.finish()?;
        let mut cur: Option<(Vec<u8>, u64)> = None;
        while let Some((k, v)) = edges.next_record()? {
            let ident = k[..k.len() - 8].to_vec();
            let start = lookup(&mut map, &mut cur, &ident)?;
            let mut f = Fields::new(v);
            let m = f.meta();
            let start = match resolved(&mut l, &m, start)? {
                Some(id) => id,
                None => continue,
            };
            let end = f.bytes().to_vec();
            let rest = f.rest();
            let mut key = end;
            key.extend_from_slice(&m.ord.to_be_bytes());
            let mut nv = start.to_be_bytes().to_vec();
            put_meta(&mut nv, &m);
            nv.extend_from_slice(rest);
            by_end.push(&key, &nv)?;
        }
    }
    // Then by end: edge identifiers and per-record constraints.
    let mut edges_by_id = l.sorter()?;
    let mut unique = if l.constraints.has_unique(Entity::Edge) {
        Some(l.sorter()?)
    } else {
        None
    };
    {
        let mut edges = by_end.finish()?;
        let mut map = ends.finish()?;
        let mut cur: Option<(Vec<u8>, u64)> = None;
        while let Some((k, v)) = edges.next_record()? {
            let ident = k[..k.len() - 8].to_vec();
            let end = lookup(&mut map, &mut cur, &ident)?;
            let mut f = Fields::new(v);
            let start = f.u64();
            let m = f.meta();
            let end = match resolved(&mut l, &m, end)? {
                Some(id) => id,
                None => continue,
            };
            let ty = f.u64();
            let rec = f.rest();
            let (_, props) = record::decode(rec).expect("encoded above");
            let el = Element {
                tokens: vec![ty],
                props,
            };
            if let Err(e) = l.constraints.check(Entity::Edge, &el) {
                l.invalid(&m, e)?;
                continue;
            }
            let id = core.ids.element()?;
            if let Some(u) = unique.as_mut() {
                let keys = l.constraints.unique_keys(Entity::Edge, id, &el);
                if !keys.is_empty() {
                    u.push(&m.ord.to_be_bytes(), &unique_candidate(id, &m, &keys))?;
                }
            }
            let mut tv = Vec::with_capacity(24 + rec.len());
            for x in [start, end, ty] {
                tv.extend_from_slice(&x.to_be_bytes());
            }
            tv.extend_from_slice(rec);
            edges_by_id.push(&id.to_be_bytes(), &tv)?;
        }
    }
    if let Some(u) = unique.take() {
        l.check_unique(u)?;
    }
    l.settle()?;
    // Steps 3 to 5: the node tree, gathering the index keys of every
    // index in one sort keyed by index identifier first (rmp #58 FINDING
    // 454: one reservation whatever the number of indexes), the labels and
    // the node statistics.
    let node_indexes: Vec<SchemaDef> = indexes
        .iter()
        .filter(|d| d.entity == Entity::Node)
        .cloned()
        .collect();
    let edge_indexes: Vec<SchemaDef> = indexes
        .iter()
        .filter(|d| d.entity == Entity::Edge)
        .cloned()
        .collect();
    let mut index_keys_sort = l.sorter()?;
    let mut stats = crate::catalog::stats::Delta::default();
    let mut built = Vec::new();
    let mut loaded = (0u64, 0u64);
    let mut labels = l.sorter()?;
    {
        let mut src = Elements {
            sorted: nodes_by_id.finish()?,
            rejected: &l.rejected,
            entity: Entity::Node,
            indexes: &node_indexes,
            keys: &mut index_keys_sort,
            labels: Some(&mut labels),
            adj: None,
            counts: BTreeMap::new(),
            n: 0,
        };
        let b = engine.bulk_build_pages_into(&mut src, g.nodes.id(), Keys::Prefix)?;
        for (&label, &n) in &src.counts {
            stats.add(StatKey::Nodes { graph, label }, n as i64);
        }
        loaded.0 = src.n;
        drop(src);
        built.push((b, g.nodes));
    }
    // The edge tree, gathering the adjacency and index keys.
    let mut adj = l.sorter()?;
    {
        let mut src = Elements {
            sorted: edges_by_id.finish()?,
            rejected: &l.rejected,
            entity: Entity::Edge,
            indexes: &edge_indexes,
            keys: &mut index_keys_sort,
            labels: None,
            adj: Some(&mut adj),
            counts: BTreeMap::new(),
            n: 0,
        };
        let b = engine.bulk_build_pages_into(&mut src, g.edges.id(), Keys::Prefix)?;
        loaded.1 = src.n;
        drop(src);
        built.push((b, g.edges));
    }
    // The adjacency tree, with the edge statistics of labelled endpoints.
    {
        let mut src = Adjacency {
            sorted: adj.finish()?,
            labels: labels.finish()?,
            current: None,
            graph,
            stats: &mut stats,
        };
        let b = engine.bulk_build_pages_into(&mut src, g.adj.id(), Keys::Fields)?;
        drop(src);
        built.push((b, g.adj));
    }
    // Step 4: each index in identifier order, from its run of the one
    // sorted key stream, with its statistics.
    let mut trees = vec![g.nodes.id(), g.edges.id(), g.adj.id()];
    let mut stream = KeyStream {
        sorted: index_keys_sort.finish()?,
        head: None,
        done: false,
    };
    for d in &indexes {
        let mut src = Counted {
            stream: &mut stream,
            index: d.id,
            strip: d.kind == crate::index::SchemaKind::Lookup,
            last: None,
            entries: 0,
            distinct: 0,
            out: Vec::new(),
        };
        let b = engine.bulk_build_pages_into(&mut src, d.tree, Keys::Prefix)?;
        for (distinct, n) in [(false, src.entries), (true, src.distinct)] {
            stats.add(
                StatKey::Index {
                    graph,
                    index: d.id,
                    distinct,
                },
                n as i64,
            );
        }
        drop(src);
        let old = tx.tree(d.tree)?;
        trees.push(d.tree);
        built.push((b, old));
    }
    drop(stream);
    pause(core, Phase::LoadBuilt);
    // Step 6: one commit installs every tree, the counters, the
    // statistics and a new version of the graph's catalog entry.
    let entry = tx
        .get(cat, &name)?
        .ok_or_else(|| crate::api::internal("the loaded graph's catalog entry"))?;
    tx.put(cat, &name, &entry)?;
    tx.schema_stats.merge(&stats);
    tx.loaded_trees = trees;
    tx.commit_installing_all(built)?;
    Ok(loaded)
}

/// Resolves an endpoint looked up in the identifier map: the node, or an
/// invalid record for an unknown or a skipped one.
fn resolved(l: &mut Load<'_, '_, '_>, m: &Meta, found: Option<u64>) -> Result<Option<u64>, Error> {
    match found {
        Some(u64::MAX) => {
            l.invalid(
                m,
                Error::new(
                    codes::C22N05,
                    format!("input failed validation: {SKIPPED_ENDPOINT}"),
                ),
            )?;
            Ok(None)
        }
        Some(id) if l.rejected.contains(&id) => {
            l.invalid(
                m,
                Error::new(
                    codes::C22N05,
                    format!("input failed validation: {SKIPPED_ENDPOINT}"),
                ),
            )?;
            Ok(None)
        }
        Some(id) => Ok(Some(id)),
        None => {
            l.invalid(
                m,
                Error::new(
                    codes::C22N05,
                    format!("input failed validation: {UNKNOWN_ENDPOINT}"),
                ),
            )?;
            Ok(None)
        }
    }
}

/// The node of input identifier key `ident` in the sorted identifier map
/// `map`, merged in order: `cur` holds the map record read last. The map
/// holds one record per identifier (the node, or `u64::MAX` for a node
/// that failed a per-record check); the identifiers asked come in
/// ascending order.
fn lookup(
    map: &mut Sorted<'_>,
    cur: &mut Option<(Vec<u8>, u64)>,
    ident: &[u8],
) -> Result<Option<u64>, Error> {
    loop {
        if let Some((k, v)) = cur
            && k.as_slice() >= ident
        {
            return Ok((k.as_slice() == ident).then_some(*v));
        }
        match map.next_record()? {
            Some((k, v)) => {
                *cur = Some((
                    k.to_vec(),
                    u64::from_be_bytes(v.try_into().expect("8 bytes")),
                ));
            }
            None => {
                *cur = Some((vec![0xFF; MAX_KEY + 1], 0));
            }
        }
    }
}

/// The sorted records of the node or edge tree, without the rejected
/// elements, as a bulk build's source; each element's index keys go to
/// their sorts, a node's labels to `labels`, an edge's adjacency keys to
/// `adj`.
struct Elements<'a, 's> {
    sorted: Sorted<'s>,
    rejected: &'a HashSet<u64>,
    entity: Entity,
    indexes: &'a [SchemaDef],
    /// The one sort of every index's keys, each key led by its index's
    /// identifier (a lookup index's keys get it added).
    keys: &'a mut Sorter<'s>,
    labels: Option<&'a mut Sorter<'s>>,
    adj: Option<&'a mut Sorter<'s>>,
    /// Nodes per label.
    counts: BTreeMap<u64, u64>,
    /// Elements written.
    n: u64,
}

impl SortedEntries for Elements<'_, '_> {
    fn next_entry(&mut self) -> StorageResult<Option<(&[u8], &[u8])>> {
        loop {
            let Some((k, v)) = self.sorted.next_record()? else {
                return Ok(None);
            };
            let id = u64::from_be_bytes(k.try_into().expect("8 bytes"));
            if self.rejected.contains(&id) {
                continue;
            }
            let el = match self.entity {
                Entity::Node => {
                    let (tokens, props) = record::decode(v).expect("encoded by the load");
                    if !tokens.is_empty() {
                        for &t in &tokens {
                            *self.counts.entry(t).or_insert(0) += 1;
                        }
                        if let Some(l) = self.labels.as_deref_mut() {
                            let mut b = Vec::new();
                            for &t in &tokens {
                                put_varint(&mut b, t);
                            }
                            l.push(k, &b)?;
                        }
                    }
                    Element { tokens, props }
                }
                Entity::Edge => {
                    let be = |i: usize| u64::from_be_bytes(v[i..i + 8].try_into().expect("8"));
                    let (src, dst, ty) = (be(0), be(8), be(16));
                    if let Some(a) = self.adj.as_deref_mut() {
                        for (node, dir, nbr) in [(src, OUT, dst), (dst, IN, src)] {
                            a.push(
                                &adj_key(Adj {
                                    node,
                                    dir,
                                    ty,
                                    nbr,
                                    edge: id,
                                }),
                                &[],
                            )?;
                        }
                    }
                    let (_, props) = record::decode(&v[24..]).expect("encoded by the load");
                    Element {
                        tokens: vec![ty],
                        props,
                    }
                }
            };
            for d in self.indexes {
                for key in index_keys(d, id, &el) {
                    if key.len() > MAX_KEY {
                        return Err(StorageError::InvalidArgument(
                            "a value too large for an index key of 1 024 bytes",
                        ));
                    }
                    if d.kind == crate::index::SchemaKind::Lookup {
                        self.keys
                            .push(&[&d.id.to_be_bytes()[..], &key].concat(), &[])?;
                    } else {
                        // A range or point key starts with its index's
                        // identifier already.
                        self.keys.push(&key, &[])?;
                    }
                }
            }
            self.n += 1;
            // Re-borrowed: the record read above.
            return self.sorted.current();
        }
    }
}

/// The sorted adjacency keys as a bulk build's source; each key at a
/// labelled node adds to the `(label, type, direction)` statistics, the
/// labels read in the same node order from `labels`.
struct Adjacency<'a, 's> {
    sorted: Sorted<'s>,
    labels: Sorted<'s>,
    current: Option<(u64, Vec<u64>)>,
    graph: u64,
    stats: &'a mut crate::catalog::stats::Delta,
}

impl SortedEntries for Adjacency<'_, '_> {
    fn next_entry(&mut self) -> StorageResult<Option<(&[u8], &[u8])>> {
        let Some((k, _)) = self.sorted.next_record()? else {
            return Ok(None);
        };
        debug_assert_eq!(k.len(), ADJ_KEY_LEN);
        let a = crate::storage::graph::decode_adj(k).expect("built by the load");
        while self.current.as_ref().is_none_or(|(n, _)| *n < a.node) {
            match self.labels.next_record()? {
                Some((lk, lv)) => {
                    let node = u64::from_be_bytes(lk.try_into().expect("8 bytes"));
                    let mut f = Fields::new(lv);
                    let mut ls = Vec::new();
                    while f.at < lv.len() {
                        ls.push(f.varint());
                    }
                    self.current = Some((node, ls));
                }
                None => {
                    self.current = Some((u64::MAX, Vec::new()));
                    break;
                }
            }
        }
        if let Some((node, ls)) = &self.current
            && *node == a.node
        {
            for &label in ls {
                self.stats.add(
                    StatKey::Edges {
                        graph: self.graph,
                        label,
                        ty: a.ty,
                        dir: a.dir,
                    },
                    1,
                );
            }
        }
        self.sorted.current()
    }
}

/// The sorted keys of every index, each led by its index's identifier:
/// the runs of consecutive indexes, read one after another.
struct KeyStream<'s> {
    sorted: Sorted<'s>,
    /// The first key of the next index's run, read past the end of the
    /// previous run.
    head: Option<Vec<u8>>,
    done: bool,
}

impl KeyStream<'_> {
    /// The next key if it belongs to index `index`.
    fn next_of(&mut self, index: u64) -> StorageResult<Option<Vec<u8>>> {
        if self.head.is_none() && !self.done {
            match self.sorted.next_record()? {
                Some((k, _)) => self.head = Some(k.to_vec()),
                None => self.done = true,
            }
        }
        match &self.head {
            Some(k) if k[..8] == index.to_be_bytes() => Ok(self.head.take()),
            _ => Ok(None),
        }
    }
}

/// The run of index `index` of a [`KeyStream`] as a bulk build's source,
/// counting its entries and distinct keys (TS-17).
struct Counted<'a, 's> {
    stream: &'a mut KeyStream<'s>,
    index: u64,
    /// Whether the stream added the index identifier (a lookup index).
    strip: bool,
    last: Option<Vec<u8>>,
    entries: u64,
    distinct: u64,
    out: Vec<u8>,
}

impl SortedEntries for Counted<'_, '_> {
    fn next_entry(&mut self) -> StorageResult<Option<(&[u8], &[u8])>> {
        let Some(k) = self.stream.next_of(self.index)? else {
            return Ok(None);
        };
        self.out = if self.strip { k[8..].to_vec() } else { k };
        self.entries += 1;
        let p = prefix_of(&self.out);
        if self.last.as_deref() != Some(p) {
            self.distinct += 1;
            self.last = Some(p.to_vec());
        }
        Ok(Some((&self.out, &[])))
    }
}
