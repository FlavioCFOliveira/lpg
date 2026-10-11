//! Authentication and authorisation ([TS-23]; rmp #48).
//!
//! The application implements [`Authoriser`]; the crate asks it before
//! every catalog and data action (FR-AUTH-001):
//!
//! * **Statements.** After planning and before execution, the crate
//!   collects every `(action, object)` pair the plan can perform
//!   ([`requests`]) and asks once per distinct pair, with the object's owner
//!   (FR-AUTH-002). A refusal fails the statement with 42NFF before any
//!   effect. A statement therefore fails if its plan could perform a
//!   refused action, even when the data would not have led it to.
//! * **API calls** (index, constraint and directory management,
//!   transaction listing and termination) ask once per call. `create`
//!   asks nothing: no session or principal exists when it runs.
//! * **Actions** are Neo4j's privilege names ([`Action`]): `TRAVERSE` and
//!   `READ` for every graph a plan reads, `CREATE` for an insert, `SET
//!   PROPERTY` for `SET` and for `REMOVE` of a property (Neo4j's `SET
//!   PROPERTY` privilege covers the `SET` clause, Neo4j Operations Manual
//!   "Write privileges", read 2026-10-11), `CREATE` and `DROP` per catalog
//!   kind, and the index, constraint and transaction actions.
//! * **Objects** ([`Object`]): a catalog path for data, catalog, index and
//!   constraint actions, the object of a data action being its graph; a
//!   transaction identifier for `SHOW TRANSACTION` and `TERMINATE
//!   TRANSACTION`, whose owner is the transaction's principal.
//!
//! The trait is dyn-compatible: its methods return boxed futures, as
//! `async fn` in a trait is not object-safe, so no `async-trait`
//! dependency is needed (TS-23 Crates). Nothing here is reusable from a
//! crate; GraphLite's model was rejected for its own grant syntax
//! (BR-AUTH-001; task #3 plan §4.15).
//!
//! [TS-23]: ../../docs/technical-spec/interfaces.md

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::bound::CatalogOp;
use crate::query::plan::{Node, Op, Plan};
use crate::tx::Transaction;

/// A boxed future, `Send`, as the [`Authoriser`] methods return.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An action the crate asks to perform (TS-23; Neo4j privilege names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Find nodes and edges of a graph.
    Traverse,
    /// Read the properties of a graph's elements.
    Read,
    /// Create nodes and edges in a graph.
    Create,
    /// Delete nodes and edges of a graph.
    Delete,
    /// Add labels to nodes of a graph.
    SetLabel,
    /// Remove labels from nodes of a graph.
    RemoveLabel,
    /// Set or remove properties of a graph's elements.
    SetProperty,
    /// Create a catalog object of a kind.
    CreateDirectory,
    /// Drop a directory.
    DropDirectory,
    /// Create a schema.
    CreateSchema,
    /// Drop a schema.
    DropSchema,
    /// Create a graph type.
    CreateGraphType,
    /// Drop a graph type.
    DropGraphType,
    /// Create a graph.
    CreateGraph,
    /// Drop a graph.
    DropGraph,
    /// Create an index on a graph.
    CreateIndex,
    /// Drop an index of a graph.
    DropIndex,
    /// List the indexes of a graph.
    ShowIndex,
    /// Create a constraint on a graph.
    CreateConstraint,
    /// Drop a constraint of a graph.
    DropConstraint,
    /// List the constraints of a graph.
    ShowConstraint,
    /// See another principal's transaction.
    ShowTransaction,
    /// Terminate another principal's transaction.
    TerminateTransaction,
}

impl Action {
    /// The privilege name: Neo4j's, such as `TRAVERSE` or `CREATE INDEX`,
    /// and `CREATE` or `DROP` followed by the catalog kind.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Action::Traverse => "TRAVERSE",
            Action::Read => "READ",
            Action::Create => "CREATE",
            Action::Delete => "DELETE",
            Action::SetLabel => "SET LABEL",
            Action::RemoveLabel => "REMOVE LABEL",
            Action::SetProperty => "SET PROPERTY",
            Action::CreateDirectory => "CREATE DIRECTORY",
            Action::DropDirectory => "DROP DIRECTORY",
            Action::CreateSchema => "CREATE SCHEMA",
            Action::DropSchema => "DROP SCHEMA",
            Action::CreateGraphType => "CREATE GRAPH TYPE",
            Action::DropGraphType => "DROP GRAPH TYPE",
            Action::CreateGraph => "CREATE GRAPH",
            Action::DropGraph => "DROP GRAPH",
            Action::CreateIndex => "CREATE INDEX",
            Action::DropIndex => "DROP INDEX",
            Action::ShowIndex => "SHOW INDEX",
            Action::CreateConstraint => "CREATE CONSTRAINT",
            Action::DropConstraint => "DROP CONSTRAINT",
            Action::ShowConstraint => "SHOW CONSTRAINT",
            Action::ShowTransaction => "SHOW TRANSACTION",
            Action::TerminateTransaction => "TERMINATE TRANSACTION",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The object of an action.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Object {
    /// A catalog object by its absolute path, such as `/public/lpg`: the
    /// graph of a data, index or constraint action, the object a catalog
    /// action creates or drops.
    Catalog(String),
    /// A transaction by its identifier (`show_transactions`).
    Transaction(u64),
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Object::Catalog(p) => f.write_str(p),
            Object::Transaction(id) => write!(f, "transaction {id}"),
        }
    }
}

/// A principal an [`Authoriser::authenticate`] accepts, with its optional
/// home schema and home graph (FR-SES-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authenticated {
    /// The principal.
    pub principal: String,
    /// Its home schema's path, if it has one.
    pub home_schema: Option<String>,
    /// Its home graph's path, if it has one.
    pub home_graph: Option<String>,
}

/// The authentication and authorisation abstraction (ABS-002, TS-23),
/// implemented by the application.
///
/// Rust API sessions receive their principal from the application and are
/// not authenticated (FR-SES-001): only [`Authoriser::authorise`] is
/// called through the Rust API. `authenticate` and `impersonate` serve the
/// Bolt endpoints (Sprint 6); their default implementations refuse.
pub trait Authoriser: Send + Sync {
    /// The principal of `credentials` under `scheme`, or `None` for a
    /// refusal.
    fn authenticate<'a>(
        &'a self,
        scheme: &'a str,
        credentials: &'a str,
    ) -> BoxFuture<'a, Option<Authenticated>> {
        let _ = (scheme, credentials);
        Box::pin(async { None })
    }

    /// The principal `principal` may act as, given the impersonation
    /// target `target`, or `None` for a refusal.
    fn impersonate<'a>(
        &'a self,
        principal: &'a str,
        target: &'a str,
    ) -> BoxFuture<'a, Option<String>> {
        let _ = (principal, target);
        Box::pin(async { None })
    }

    /// Whether `principal` may perform `action` on `object`, whose owner
    /// is `owner` (FR-AUTH-002).
    fn authorise<'a>(
        &'a self,
        principal: &'a str,
        action: Action,
        object: &'a Object,
        owner: &'a str,
    ) -> BoxFuture<'a, bool>;
}

/// One authorisation request: an action, its object and the object's
/// owner.
pub(crate) type Request = (Action, Object, String);

/// The error of a refused request (42NFF, `interfaces.md`); the message
/// names the action and the object, never a value.
pub(crate) fn refused(action: Action, object: &Object) -> Error {
    Error::new(
        codes::C42NFF,
        format!("permission denied: {action} on {object}"),
    )
}

/// Asks `auth` every request of `requests` for `principal`, each once, in
/// order; the first refusal fails with 42NFF.
///
/// # Errors
///
/// 42NFF for a refused request.
pub(crate) async fn ask_all(
    auth: &dyn Authoriser,
    principal: &str,
    requests: &[Request],
) -> Result<(), Error> {
    let mut seen = BTreeSet::new();
    for (action, object, owner) in requests {
        if !seen.insert((*action, object)) {
            continue;
        }
        if !auth.authorise(principal, *action, object, owner).await {
            return Err(refused(*action, object));
        }
    }
    Ok(())
}

/// The absolute path of catalog object `id`, `/` joined.
///
/// # Errors
///
/// Storage errors of the catalog read.
pub(crate) fn path_of(tx: &mut Transaction, id: u64) -> Result<String, Error> {
    let p = tx.path(id)?.unwrap_or_default();
    Ok(format!("/{}", p.join("/")))
}

/// The owner of catalog object `id`; empty if it does not exist.
///
/// # Errors
///
/// Storage errors of the catalog read.
pub(crate) fn owner_of(tx: &mut Transaction, id: u64) -> Result<String, Error> {
    Ok(tx.object(id)?.map(|o| o.owner).unwrap_or_default())
}

/// The request about graph `id` for `action`.
///
/// # Errors
///
/// Storage errors of the catalog read.
pub(crate) fn on_graph(tx: &mut Transaction, action: Action, id: u64) -> Result<Request, Error> {
    Ok((action, Object::Catalog(path_of(tx, id)?), owner_of(tx, id)?))
}

/// Every request the plan `plan` can perform (module documentation), with
/// `graphs` the statement's graphs by index and `principal` the owner of
/// what a catalog statement creates in a parent the same statement
/// creates.
///
/// # Errors
///
/// Storage errors of the catalog reads.
pub(crate) fn requests(
    plan: &Plan,
    graphs: &[u64],
    tx: &mut Transaction,
    principal: &str,
) -> Result<Vec<Request>, Error> {
    let mut acts: BTreeSet<(Action, usize)> = BTreeSet::new();
    let mut catalog: Vec<CatalogOp> = Vec::new();
    let mut extra: Vec<Request> = Vec::new();
    let mut nodes: Vec<&Node> = vec![&plan.root];
    nodes.extend(plan.subs.iter());
    while let Some(n) = nodes.pop() {
        nodes.extend(n.children.iter());
        match &n.op {
            Op::AllNodesScan { graph, .. }
            | Op::LabelScan { graph, .. }
            | Op::IndexSeek { graph, .. }
            | Op::Expand { graph, .. } => {
                acts.insert((Action::Traverse, *graph));
                acts.insert((Action::Read, *graph));
            }
            Op::Insert { graph, .. } => {
                acts.insert((Action::Create, *graph));
            }
            Op::Set { graph, .. } | Op::Remove { graph, .. } => {
                acts.insert((Action::SetProperty, *graph));
            }
            Op::Catalog(ops) => catalog.extend(ops.iter().cloned()),
            Op::ProcedureCall { routine, graph, .. } => {
                for (action, all) in routine.requests() {
                    if all {
                        for g in tx.graph_ids()? {
                            extra.push(on_graph(tx, action, g)?);
                        }
                    } else {
                        acts.insert((action, *graph));
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (action, g) in acts {
        if let Some(&id) = graphs.get(g) {
            out.push(on_graph(tx, action, id)?);
        }
    }
    out.extend(extra);
    for op in &catalog {
        out.push(catalog_request(tx, op, principal)?);
    }
    Ok(out)
}

/// The request of one catalog statement: the created or dropped object's
/// path, with its owner for a drop, and for a create the owner of its
/// parent, or `principal` when the parent does not exist yet (the same
/// statement list creates it).
fn catalog_request(
    tx: &mut Transaction,
    op: &CatalogOp,
    principal: &str,
) -> Result<Request, Error> {
    let (action, path, create) = match op {
        CatalogOp::CreateSchema(p) => (Action::CreateSchema, p, true),
        CatalogOp::DropSchema(p) => (Action::DropSchema, p, false),
        CatalogOp::CreateGraph { path, .. } => (Action::CreateGraph, path, true),
        CatalogOp::DropGraph(p) => (Action::DropGraph, p, false),
        CatalogOp::CreateGraphType { path, .. } => (Action::CreateGraphType, path, true),
        CatalogOp::DropGraphType(p) => (Action::DropGraphType, p, false),
    };
    let names: Vec<&str> = path.iter().map(|s| &**s).collect();
    let object = Object::Catalog(format!("/{}", names.join("/")));
    let looked = if create {
        tx.resolve(&names[..names.len().saturating_sub(1)])?
    } else {
        tx.resolve(&names)?
    };
    let owner = match looked {
        Some(o) => o.owner,
        None if create => principal.to_string(),
        None => String::new(),
    };
    Ok((action, object, owner))
}
