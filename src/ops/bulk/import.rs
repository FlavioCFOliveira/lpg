//! Import (TS-26; FR-IMP-002 to FR-IMP-005, UC-OPS-003; rmp #57).
//!
//! [`run`] checks the request, creates the report, resolves and authorises
//! the target, opens the input as a [`Source`] and imports it by one
//! transaction ([`transactional`]) or by the fast initial load
//! ([`super::fast::load`]); a database-scope dump is imported as TS-26
//! item D fixes ([`database`]). The report is committed whatever the
//! outcome, so an aborted import's report names the record that stopped it
//! (FR-IMP-004).
//!
//! **Transactional import** (TS-26): one transaction through
//! `create_node` and `create_edge` (`src/storage/graph.rs`); it adds all
//! its records or none, and its writes with their old versions must fit
//! the memory cap (FR-LIM-007). Its identifier map, and the uniqueness
//! values it checks within the input, are charged to the cap
//! ([`Charge`]). Each record is checked before it is written: duplicate
//! input identifiers, unknown or skipped endpoints, and the graph's
//! constraints (existence, type and node-key existence per record,
//! uniqueness against the committed data and within the input), so that
//! a skipped record never reaches the commit's own checks.

use std::collections::{HashMap, HashSet};

use super::dump::{self, Scope};
use super::model::{
    Charge, Constraints, DUPLICATE_ID, ENTRY_OVERHEAD, Invalid, Report, SKIPPED_ENDPOINT, Tokens,
    UNKNOWN_ENDPOINT, resolve,
};
use super::{
    Format, ImportMode, ImportRequest, ImportSummary, ImportTarget, check_import, invalid_argument,
};
use crate::auth::{Action, Object, Request};
use crate::catalog::{DEFAULT_GRAPH_ID, DEFAULT_SCHEMA_ID, Kind, TokenKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::index::{Entity, SchemaDef, State, schema_key};
use crate::storage::btree::Keys;
use crate::storage::engine::Engine;
use crate::storage::error::Error as StorageError;
use crate::tx::{Isolation, Transaction};

use super::model::Source;

/// The data actions of an import into the graph at `path` owned by
/// `owner` (TS-26 Authorisation).
fn data_requests(path: &str, owner: &str) -> Vec<Request> {
    [Action::Create, Action::SetLabel, Action::SetProperty]
        .into_iter()
        .map(|a| (a, Object::Catalog(path.to_owned()), owner.to_owned()))
        .collect()
}

/// The graph at absolute path `path`: its identifier and owner.
///
/// # Errors
///
/// 22N51 if no graph has that path.
pub(crate) fn graph_at(engine: &Engine, path: &str) -> Result<(u64, String, String), Error> {
    let mut tx = engine.begin(Isolation::Snapshot)?;
    let names: Vec<&str> = path.split('/').filter(|n| !n.is_empty()).collect();
    let found = tx.resolve(&names)?;
    tx.abort();
    match found {
        Some(o) if o.kind == Kind::Graph => Ok((o.id, format!("/{}", names.join("/")), o.owner)),
        _ => Err(Error::new(
            codes::C22N51,
            "graph reference not found: the path names no graph",
        )),
    }
}

/// Runs import `req` for `principal`, asking `auth` once per `(action,
/// object)` pair, with the transaction memory limit `memory_limit`
/// (module documentation).
///
/// # Errors
///
/// 22N11 for a request that does not fit (FR-IMP-003); 22N51 for a missing
/// graph; 42NFF for a refusal; the code of the invalid record that aborted
/// the import; 22N05 for a malformed header or a damaged dump; 51N41 for a
/// fast load into a graph that is not empty or a database import into a
/// catalog that holds other objects; 51N72, 51N73 for the memory cap and
/// the transaction limit; 50N00 for I/O failures.
pub(crate) fn run(
    engine: &Engine,
    req: &ImportRequest,
    principal: &str,
    memory_limit: Option<u64>,
    auth: &dyn Fn(&[Request]) -> Result<(), Error>,
) -> Result<ImportSummary, Error> {
    check_import(req)?;
    let storage = engine.core().storage.clone();
    let mut report = Report::create(&*storage, &req.report_location, &req.report, req.on_invalid)?;
    let r = run_with(engine, req, principal, memory_limit, auth, &mut report);
    let invalid = report.count;
    let committed = report.commit();
    // The data commit is the import's commit point: after it the outcome
    // is success whatever happens to the report, which is reported beside
    // the counts (rmp #57 DECISION on FINDING 456). Before it, a failed
    // report is the cause of the import's error.
    match r {
        Ok(mut summary) => {
            summary.invalid = invalid;
            summary.report_error = committed.err();
            Ok(summary)
        }
        Err(e) => Err(match committed {
            Ok(()) => e,
            Err(re) => e.caused_by(re),
        }),
    }
}

fn run_with(
    engine: &Engine,
    req: &ImportRequest,
    principal: &str,
    memory_limit: Option<u64>,
    auth: &dyn Fn(&[Request]) -> Result<(), Error>,
    report: &mut Report<'_>,
) -> Result<ImportSummary, Error> {
    let storage = engine.core().storage.clone();
    let open = |name: &str| -> Result<Box<dyn crate::storage::StorageFile>, Error> {
        storage
            .open(&req.location.join(name))
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => invalid_argument("an input file does not exist"),
                _ => super::files::io_error(e),
            })
    };
    if req.target == ImportTarget::Database {
        let file = open(&req.nodes[0].name)?;
        let d = dump::verify(&*file, &req.nodes[0].name, Some(&engine.core().pool))?;
        if d.scope != Scope::Database {
            return Err(invalid_argument(
                "a graph-scope dump imports into a graph, not a database",
            ));
        }
        return database(engine, &d, principal, auth, report);
    }
    let ImportTarget::Graph(path) = &req.target else {
        unreachable!("handled above")
    };
    let (graph, path, owner) = graph_at(engine, path)?;
    auth(&data_requests(&path, &owner))?;
    let mut go = |source: &mut dyn Source| -> Result<ImportSummary, Error> {
        let (nodes, edges) = match req.mode {
            ImportMode::Transactional => {
                let mut tx = engine.begin(Isolation::Serializable)?;
                tx.memory_limit = memory_limit;
                match transactional(&mut tx, engine, graph, source, report) {
                    Ok(n) => {
                        tx.commit()?;
                        n
                    }
                    Err(e) => {
                        tx.abort();
                        return Err(e);
                    }
                }
            }
            ImportMode::FastLoad => super::fast::load(engine, graph, source, report)?,
        };
        Ok(ImportSummary {
            nodes,
            edges,
            ..ImportSummary::default()
        })
    };
    match req.format {
        Format::Dump => {
            let file = open(&req.nodes[0].name)?;
            let d = dump::verify(&*file, &req.nodes[0].name, Some(&engine.core().pool))?;
            if d.scope != Scope::Graph {
                return Err(invalid_argument(
                    "a database-scope dump imports into a database, not a graph",
                ));
            }
            go(&mut d.graph(0))
        }
        Format::JsonLines => {
            let file = &req.nodes[0].name;
            go(&mut super::jsonl::Input::new(
                &*storage,
                &req.location,
                file,
            ))
        }
        Format::GraphMl => {
            let file = &req.nodes[0].name;
            go(&mut super::graphml::Input::new(
                &*storage,
                &req.location,
                file,
            ))
        }
        Format::Csv => go(&mut super::csv::Input::new(&*storage, req)),
    }
}

/// The identifier map and checks of one transactional import.
struct State_<'r, 's> {
    report: &'r mut Report<'s>,
    tokens: Tokens,
    constraints: Constraints,
    map: HashMap<(String, String), u64>,
    skipped: HashSet<(String, String)>,
    unique: HashSet<Vec<u8>>,
    charge: Charge,
}

impl State_<'_, '_> {
    /// Reports `inv`; under abort its error stops the import.
    fn invalid(&mut self, inv: Invalid) -> Result<(), Error> {
        self.report.invalid(inv)
    }

    /// The owned-index prefixes of `el` already taken in this import.
    fn unique_taken(&self, keys: &[(u64, Vec<u8>)]) -> bool {
        keys.iter()
            .any(|(_, k)| self.unique.contains(crate::index::prefix_of(k)))
    }

    fn take_unique(&mut self, keys: Vec<(u64, Vec<u8>)>) -> Result<(), Error> {
        for (_, k) in keys {
            let p = crate::index::prefix_of(&k).to_vec();
            self.charge.add(p.len() as u64 + ENTRY_OVERHEAD)?;
            self.unique.insert(p);
        }
        Ok(())
    }
}

/// Imports `source` into graph `graph` in transaction `tx` (module
/// documentation), reporting invalid records to `report`. Returns the
/// nodes and edges written; the caller commits or aborts `tx`.
///
/// # Errors
///
/// As [`run`].
pub(crate) fn transactional(
    tx: &mut Transaction,
    engine: &Engine,
    graph: u64,
    source: &mut dyn Source,
    report: &mut Report<'_>,
) -> Result<(u64, u64), Error> {
    let g = tx.lock_graph(graph)?;
    let mut st = State_ {
        report,
        tokens: Tokens::new(engine),
        constraints: Constraints::of(engine, graph),
        map: HashMap::new(),
        skipped: HashSet::new(),
        unique: HashSet::new(),
        charge: Charge::new(engine),
    };
    let (mut nodes, mut edges) = (0u64, 0u64);
    source.nodes(&mut |item| {
        let n = match item {
            Ok(n) => n,
            Err(inv) => return st.invalid(inv),
        };
        let key = n.id.as_ref().map(|id| (n.space.clone(), id.clone()));
        let fail = |st: &mut State_<'_, '_>, e: Error| -> Result<(), Error> {
            if let Some(k) = &key
                && !st.map.contains_key(k)
                && !st.skipped.contains(k)
            {
                st.charge
                    .add((k.0.len() + k.1.len()) as u64 + ENTRY_OVERHEAD)?;
                st.skipped.insert(k.clone());
            }
            st.invalid(Invalid {
                file: n.file.clone(),
                pos: n.pos,
                id: n.id.clone(),
                error: Box::new(e),
            })
        };
        if let Some(k) = &key
            && (st.map.contains_key(k) || st.skipped.contains(k))
        {
            return st.invalid(Invalid::new(
                &n.file,
                n.pos,
                n.id.as_deref(),
                codes::C22N05,
                &format!("input failed validation: {DUPLICATE_ID}"),
            ));
        }
        let r = match resolve(&mut st.tokens, TokenKind::Label, &n.labels, &n.props) {
            Ok(r) => r,
            Err((e, true)) => return Err(e),
            Err((e, false)) => return fail(&mut st, e),
        };
        let el = r.element();
        if let Err(e) = st.constraints.check(Entity::Node, &el) {
            return fail(&mut st, e);
        }
        let keys = st.constraints.unique_keys(Entity::Node, 0, &el);
        if st.unique_taken(&keys) {
            return fail(
                &mut st,
                StorageError::ConstraintViolation(crate::storage::error::Violation::ValueInUse)
                    .into(),
            );
        }
        let id = match tx.create_node(&g, &r.tokens, &r.props) {
            Ok(id) => id,
            Err(e @ StorageError::ConstraintViolation(_)) => return fail(&mut st, e.into()),
            Err(e) => return Err(e.into()),
        };
        st.take_unique(keys)?;
        if let Some(k) = key {
            st.charge
                .add((k.0.len() + k.1.len()) as u64 + 8 + ENTRY_OVERHEAD)?;
            st.map.insert(k, id);
        }
        nodes += 1;
        Ok(())
    })?;
    source.edges(&mut |item| {
        let e = match item {
            Ok(e) => e,
            Err(inv) => return st.invalid(inv),
        };
        let invalid = |st: &mut State_<'_, '_>, err: Error| {
            st.invalid(Invalid {
                file: e.file.clone(),
                pos: e.pos,
                id: e.id.clone(),
                error: Box::new(err),
            })
        };
        let mut ends = [0u64; 2];
        for (i, k) in [&e.start, &e.end].into_iter().enumerate() {
            match st.map.get(k) {
                Some(&id) => ends[i] = id,
                None => {
                    let why = if st.skipped.contains(k) {
                        SKIPPED_ENDPOINT
                    } else {
                        UNKNOWN_ENDPOINT
                    };
                    return invalid(
                        &mut st,
                        Error::new(codes::C22N05, format!("input failed validation: {why}")),
                    );
                }
            }
        }
        let r = match resolve(
            &mut st.tokens,
            TokenKind::EdgeType,
            std::slice::from_ref(&e.ty),
            &e.props,
        ) {
            Ok(r) => r,
            Err((err, true)) => return Err(err),
            Err((err, false)) => return invalid(&mut st, err),
        };
        let el = r.element();
        if let Err(err) = st.constraints.check(Entity::Edge, &el) {
            return invalid(&mut st, err);
        }
        let keys = st.constraints.unique_keys(Entity::Edge, 0, &el);
        if st.unique_taken(&keys) {
            return invalid(
                &mut st,
                StorageError::ConstraintViolation(crate::storage::error::Violation::ValueInUse)
                    .into(),
            );
        }
        match tx.create_edge(&g, ends[0], ends[1], r.tokens[0], &r.props) {
            Ok(_) => {}
            Err(err @ StorageError::ConstraintViolation(_)) => {
                return invalid(&mut st, err.into());
            }
            Err(err) => return Err(err.into()),
        }
        st.take_unique(keys)?;
        edges += 1;
        Ok(())
    })?;
    Ok((nodes, edges))
}

/// The absolute path of dump object `id` (TS-17 catalog paths).
fn dump_path(objects: &[dump::DumpObject], id: u64) -> Option<String> {
    let mut parts = Vec::new();
    let mut at = id;
    let mut steps = 0;
    while at != crate::catalog::ROOT {
        let o = objects.iter().find(|o| o.id == at)?;
        parts.push(o.name.as_str());
        at = o.parent;
        steps += 1;
        if steps > objects.len() {
            return None;
        }
    }
    parts.reverse();
    Some(format!("/{}", parts.join("/")))
}

/// The create action of a catalog object of `kind`.
fn create_action(kind: Kind) -> Action {
    match kind {
        Kind::Directory => Action::CreateDirectory,
        Kind::Schema => Action::CreateSchema,
        Kind::GraphType => Action::CreateGraphType,
        Kind::Graph => Action::CreateGraph,
    }
}

/// The objects `create` writes, which a database import maps instead of
/// creating: the default schema `/public` and the default graph
/// `/public/lpg` (TS-17 Creation).
fn default_object(path: &str) -> Option<u64> {
    match path {
        "/public" => Some(DEFAULT_SCHEMA_ID),
        "/public/lpg" => Some(DEFAULT_GRAPH_ID),
        _ => None,
    }
}

/// Imports a database-scope dump (TS-26 item D, rmp #55 DECISION 402 and
/// 413): the target catalog must hold only `/public` and an empty
/// `/public/lpg`; one transaction creates the dump's other objects
/// (directories, schemas, graph types, then graphs, owned by `principal`)
/// and every index and constraint definition, ONLINE with empty trees;
/// then each graph is fast-loaded, which builds those indexes and checks
/// those constraints. A failure after that transaction leaves what it
/// created, and the graphs loaded before it.
fn database(
    engine: &Engine,
    d: &dump::Dump<'_>,
    principal: &str,
    auth: &dyn Fn(&[Request]) -> Result<(), Error>,
    report: &mut Report<'_>,
) -> Result<ImportSummary, Error> {
    let core = engine.core();
    let not_empty = || {
        Error::new(
            codes::C51N41,
            "admin operation failed: the catalog holds objects other than the default schema \
             and an empty default graph",
        )
    };
    {
        let objects = core.catalog.objects();
        if objects
            .keys()
            .copied()
            .ne([DEFAULT_SCHEMA_ID, DEFAULT_GRAPH_ID])
        {
            return Err(not_empty());
        }
    }
    if !core.catalog.schema().is_empty() || !super::fast::is_empty(engine, DEFAULT_GRAPH_ID)? {
        return Err(not_empty());
    }
    let mut objects = d.catalog.objects.clone();
    let rank = |k: Kind| match k {
        Kind::Directory => 0,
        Kind::Schema => 1,
        Kind::GraphType => 2,
        Kind::Graph => 3,
    };
    objects.sort_by_key(|o| (rank(o.kind), o.id));
    let mut paths = HashMap::new();
    for o in &objects {
        let p = dump_path(&objects, o.id)
            .ok_or_else(|| super::invalid_input("a native dump object without a parent"))?;
        paths.insert(o.id, p);
    }
    // TS-26 Authorisation: the create action of each object created, on
    // its path; the index and constraint actions on each graph with
    // definitions; the data actions on each graph.
    let mut requests: Vec<Request> = Vec::new();
    for o in &objects {
        let p = &paths[&o.id];
        if default_object(p).is_none() {
            requests.push((
                create_action(o.kind),
                Object::Catalog(p.clone()),
                principal.to_owned(),
            ));
        }
        if o.kind == Kind::Graph {
            requests.extend(data_requests(p, principal));
            let defs = d.definitions.iter().filter(|s| s.graph == o.id);
            for s in defs {
                let action = if !s.kind.is_index() {
                    Action::CreateConstraint
                } else if s.link == 0 {
                    Action::CreateIndex
                } else {
                    // An owned index comes with its constraint.
                    continue;
                };
                requests.push((action, Object::Catalog(p.clone()), principal.to_owned()));
            }
        }
    }
    requests.sort();
    requests.dedup();
    auth(&requests)?;
    let mut tokens = Tokens::new(engine);
    let token = |tokens: &mut Tokens, id: u64, kind: TokenKind| -> Result<u64, Error> {
        let name = d
            .catalog
            .tokens
            .iter()
            .find(|(t, k, _)| *t == id && *k == kind)
            .map(|(_, _, n)| n.clone())
            .ok_or_else(|| super::invalid_input("a definition names an unknown token"))?;
        tokens.get(kind, &name)
    };
    // One transaction: objects, then definitions.
    let mut tx = engine.begin(Isolation::Serializable)?;
    let made = (|| -> Result<HashMap<u64, u64>, Error> {
        // The empty-catalog rule read again inside the serializable
        // transaction: a catalog object, a definition or an element
        // committed concurrently fails its validation (rmp #57 FINDING
        // 460). The check above only spares a refused import the
        // authoriser's questions.
        let root: Vec<u64> = tx
            .children(crate::catalog::ROOT)?
            .iter()
            .map(|o| o.id)
            .collect();
        let public: Vec<u64> = tx
            .children(DEFAULT_SCHEMA_ID)?
            .iter()
            .map(|o| o.id)
            .collect();
        if root != [DEFAULT_SCHEMA_ID] || public != [DEFAULT_GRAPH_ID] {
            return Err(not_empty());
        }
        if !tx.schema_defs(DEFAULT_GRAPH_ID)?.is_empty() {
            return Err(not_empty());
        }
        let g = tx.graph(DEFAULT_GRAPH_ID)?;
        for t in [g.nodes, g.edges] {
            let mut any = false;
            tx.scan_chunk(t, b"", None, 1, |_, _| {
                any = true;
                false
            })?;
            if any {
                return Err(not_empty());
            }
        }
        crate::index::build::pause(core, crate::index::build::Phase::DatabaseChecked);
        let mut ids: HashMap<u64, u64> = HashMap::new();
        ids.insert(crate::catalog::ROOT, crate::catalog::ROOT);
        for o in &objects {
            if let Some(existing) = default_object(&paths[&o.id]) {
                ids.insert(o.id, existing);
                continue;
            }
            let parent = *ids
                .get(&o.parent)
                .ok_or_else(|| super::invalid_input("a native dump object before its parent"))?;
            let new = match o.kind {
                Kind::Directory => tx.create_directory(parent, &o.name, principal)?,
                Kind::Schema => tx.create_schema(parent, &o.name, principal)?,
                Kind::GraphType => {
                    tx.create_graph_type(parent, &o.name, principal, &o.definition)?
                }
                Kind::Graph => {
                    let gt = if o.graph_type == 0 {
                        None
                    } else {
                        Some(*ids.get(&o.graph_type).ok_or_else(|| {
                            super::invalid_input("a graph whose graph type the dump lacks")
                        })?)
                    };
                    tx.create_graph(parent, &o.name, principal, gt)?
                }
            };
            ids.insert(o.id, new);
        }
        let mut def_ids: HashMap<u64, u64> = HashMap::new();
        for s in &d.definitions {
            def_ids.insert(s.id, core.ids.catalog()?);
        }
        let cat = tx.tree(crate::catalog::CATALOG_TREE)?;
        for s in &d.definitions {
            let graph = *ids
                .get(&s.graph)
                .ok_or_else(|| super::invalid_input("a definition of a graph the dump lacks"))?;
            let token_kind = match s.entity {
                Entity::Node => TokenKind::Label,
                Entity::Edge => TokenKind::EdgeType,
            };
            let def = SchemaDef {
                id: def_ids[&s.id],
                graph,
                kind: s.kind,
                entity: s.entity,
                token: if s.token == 0 && s.kind == crate::index::SchemaKind::Lookup {
                    0
                } else {
                    token(&mut tokens, s.token, token_kind)?
                },
                props: s
                    .props
                    .iter()
                    .map(|&p| token(&mut tokens, p, TokenKind::PropertyKey))
                    .collect::<Result<_, _>>()?,
                state: State::Online,
                tree: if s.kind.is_index() {
                    tx.create_tree(Keys::Prefix)?.id()
                } else {
                    0
                },
                side: 0,
                link: if s.link == 0 {
                    0
                } else {
                    *def_ids
                        .get(&s.link)
                        .ok_or_else(|| super::invalid_input("a definition links an unknown one"))?
                },
            };
            tx.put(cat, &schema_key(graph, def.id), &def.encode())?;
        }
        Ok(ids)
    })();
    let ids = match made {
        Ok(ids) => ids,
        Err(e) => {
            tx.abort();
            return Err(e);
        }
    };
    tx.commit()?;
    let mut summary = ImportSummary::default();
    for o in objects.iter().filter(|o| o.kind == Kind::Graph) {
        let (n, e) = super::fast::load(engine, ids[&o.id], &mut d.graph(o.id), report)?;
        summary.nodes += n;
        summary.edges += e;
    }
    Ok(summary)
}
