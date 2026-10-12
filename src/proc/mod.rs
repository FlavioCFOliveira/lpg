//! Procedures and functions ([TS-24]; rmp #50).
//!
//! * **Registry** ([`Registry`]): one per database handle, never
//!   persisted (FR-PROC-003). It holds the built-in routines
//!   ([`builtin`]) and the application's, in one namespace (FR-API-006):
//!   a name is looked up by its whole dotted text, such as `point.distance`.
//!   Each routine has a [`Signature`]: its name, its parameters with their
//!   defaults and its result columns. Its mode is the kind of
//!   implementation it is registered with: [`ReadProcedure`] (read-only),
//!   [`WriteProcedure`] (read-write) or [`Function`] (read-only, one row of
//!   one column).
//! * **Calling.** A `CALL` runs the routine on the statement's thread,
//!   once per input row, with a context ([`ReadContext`],
//!   [`WriteContext`]) that gives the caller's transaction and snapshot
//!   through nested GQL, the cancellation status and the memory tracker.
//!   The rows it returns are pulled one batch at a time and charged to
//!   the memory cap while the batch is held; memory the routine allocates
//!   on its own is not (FR-LIM-006).
//! * **Read-only** is enforced three ways (TS-24): [`ReadContext`] has no
//!   write method; the binder rejects a read-write routine in a query,
//!   where only reads are allowed (42N02); and nested GQL run through a
//!   read-only context fails with 42N02 if it would modify data.
//!
//! The descriptor table (a name, its parameters with defaults, its result
//! columns, its mode and its implementation, looked up by name) is ported
//! from zu @ `a821319` (<https://github.com/tamnd/zu>, Apache-2.0, see
//! `LICENSE-APACHE`), `crates/zu-query/src/procedures.rs`
//! (`ProcedureDescriptor`, the registry map and its lookup); adapted to
//! lpg's values, to typed contexts instead of zu's mode flag, and to row
//! iterators. Studied only: Neo4j `Mode`, `ProcedureCaller` (GPL) and
//! Memgraph `mg_procedure_impl.cpp` (BSL), which check the mode at run time
//! and pass read procedures an immutable graph.
//!
//! [TS-24]: ../../docs/technical-spec/interfaces.md

pub(crate) mod builtin;

use std::collections::BTreeMap;
use std::fmt;

use crate::auth::{Action, Request};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::query::exec::{Cx, Held, State};
use crate::sync::{Arc, PoisonError, RwLock};
use crate::tx::Transaction;
use crate::types::Value;

/// A parameter of a routine: its name and, when an argument may be left
/// out, its default.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub(crate) name: String,
    pub(crate) default: Option<Value>,
}

impl Parameter {
    /// A parameter that every call must give.
    #[must_use]
    pub fn new(name: &str) -> Parameter {
        Parameter {
            name: name.to_string(),
            default: None,
        }
    }

    /// A parameter whose argument may be left out, taking `default`.
    #[must_use]
    pub fn with_default(name: &str, default: Value) -> Parameter {
        Parameter {
            name: name.to_string(),
            default: Some(default),
        }
    }
}

/// The signature of a routine: its name, parameters and result columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub(crate) name: String,
    pub(crate) params: Vec<Parameter>,
    pub(crate) columns: Vec<String>,
}

impl Signature {
    /// A signature. `name` may be dotted, such as `my.proc`; it is looked
    /// up by its whole text. A parameter with a default must not come
    /// before one without.
    #[must_use]
    pub fn new(name: &str, params: Vec<Parameter>, columns: &[&str]) -> Signature {
        Signature {
            name: name.to_string(),
            params,
            columns: columns.iter().map(|c| (*c).to_string()).collect(),
        }
    }
}

/// The mode of a routine (FR-PROC-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reads only.
    ReadOnly,
    /// May modify data, in the caller's transaction.
    ReadWrite,
}

/// The rows a procedure returns, each with one value per result column.
pub type Rows = Box<dyn Iterator<Item = Result<Vec<Value>, Error>> + Send>;

/// A read-only procedure, implemented by the application (ABS-005).
pub trait ReadProcedure: Send + Sync {
    /// Runs the procedure with `args`, one per parameter (defaults
    /// filled in), and returns its rows.
    ///
    /// # Errors
    ///
    /// The procedure's errors; they fail the statement.
    fn call(&self, cx: &mut ReadContext<'_>, args: &[Value]) -> Result<Rows, Error>;
}

/// A read-write procedure, implemented by the application (ABS-005).
pub trait WriteProcedure: Send + Sync {
    /// As [`ReadProcedure::call`], with a context that may modify data.
    ///
    /// # Errors
    ///
    /// The procedure's errors; they fail the statement.
    fn call(&self, cx: &mut WriteContext<'_>, args: &[Value]) -> Result<Rows, Error>;
}

/// A function, implemented by the application (ABS-005): a read-only
/// routine that returns one row of one column (FR-API-006).
pub trait Function: Send + Sync {
    /// The function's value for `args`.
    ///
    /// # Errors
    ///
    /// The function's errors; they fail the statement.
    fn call(&self, cx: &mut ReadContext<'_>, args: &[Value]) -> Result<Value, Error>;
}

/// What a read-only routine runs with: the caller's transaction, through
/// nested read-only GQL, the cancellation status and the memory tracker.
pub struct ReadContext<'a> {
    pub(crate) tx: &'a mut Transaction,
    pub(crate) st: &'a mut State,
    pub(crate) held: &'a mut Held,
    /// The working graph's index in the statement's graphs.
    pub(crate) graph: usize,
}

impl ReadContext<'_> {
    /// Runs the read-only GQL statement `gql` with `params` in the caller's
    /// transaction and snapshot, under the caller's principal, and returns
    /// its rows.
    ///
    /// # Errors
    ///
    /// 42N02 if the statement would modify data; the statement's errors,
    /// which also fail the calling statement.
    pub fn query(
        &mut self,
        gql: &str,
        params: &BTreeMap<String, Value>,
    ) -> Result<Vec<Vec<Value>>, Error> {
        crate::query::exec::run_nested(gql, self.tx, self.st, params, true)
    }

    /// Fails once the statement is cancelled, terminated or timed out; a
    /// long routine calls it from time to time.
    ///
    /// # Errors
    ///
    /// 25N14 with the reason.
    pub fn check(&self) -> Result<(), Error> {
        match &self.st.check {
            Some(c) => c(self.st.iterations),
            None => Ok(()),
        }
    }

    /// Charges `bytes` of the routine's own memory to the memory cap until
    /// the current batch of its rows is returned. Memory a routine does
    /// not charge is not counted (FR-LIM-006).
    ///
    /// # Errors
    ///
    /// 51N72 when the memory cap refuses; 51N73 beyond the transaction's
    /// memory limit.
    pub fn reserve(&mut self, bytes: u64) -> Result<(), Error> {
        let mut cx = Cx {
            tx: self.tx,
            st: self.st,
        };
        self.held.grow(&mut cx, bytes)
    }

    /// The caller's principal.
    #[must_use]
    pub fn principal(&self) -> &str {
        &self.st.session.principal
    }
}

/// What a read-write routine runs with: [`ReadContext`] and statements
/// that modify data in the caller's transaction.
pub struct WriteContext<'a> {
    pub(crate) read: ReadContext<'a>,
}

impl<'a> std::ops::Deref for WriteContext<'a> {
    type Target = ReadContext<'a>;

    fn deref(&self) -> &ReadContext<'a> {
        &self.read
    }
}

impl std::ops::DerefMut for WriteContext<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.read
    }
}

impl WriteContext<'_> {
    /// Runs the GQL statement `gql` with `params` in the caller's
    /// transaction, under the caller's principal; it may modify data.
    ///
    /// # Errors
    ///
    /// The statement's errors, which also fail the calling statement.
    pub fn execute(
        &mut self,
        gql: &str,
        params: &BTreeMap<String, Value>,
    ) -> Result<Vec<Vec<Value>>, Error> {
        crate::query::exec::run_nested(gql, self.read.tx, self.read.st, params, false)
    }
}

/// The implementation of a routine.
#[derive(Clone)]
pub(crate) enum Body {
    Read(Arc<dyn ReadProcedure>),
    Write(Arc<dyn WriteProcedure>),
    Function(Arc<dyn Function>),
    Builtin(builtin::Builtin),
    /// A graph algorithm in one of its modes ([`crate::algo`]).
    Algo(&'static crate::algo::Entry, crate::algo::AlgoMode),
}

/// A registered routine.
pub(crate) struct Routine {
    pub(crate) sig: Signature,
    pub(crate) body: Body,
}

impl Routine {
    /// The routine's mode.
    pub(crate) fn mode(&self) -> Mode {
        match self.body {
            Body::Write(_) | Body::Algo(_, crate::algo::AlgoMode::Write) => Mode::ReadWrite,
            _ => Mode::ReadOnly,
        }
    }

    /// Calls the routine with `args` (module documentation).
    ///
    /// # Errors
    ///
    /// The routine's errors.
    pub(crate) fn invoke(
        &self,
        cx: &mut Cx<'_>,
        held: &mut Held,
        graph: usize,
        args: &[Value],
    ) -> Result<Rows, Error> {
        let read = ReadContext {
            tx: cx.tx,
            st: cx.st,
            held,
            graph,
        };
        match &self.body {
            Body::Read(p) => p.call(&mut { read }, args),
            Body::Write(p) => p.call(&mut WriteContext { read }, args),
            Body::Function(f) => {
                let v = f.call(&mut { read }, args)?;
                Ok(Box::new(std::iter::once(Ok(vec![v]))))
            }
            Body::Builtin(b) => builtin::run(*b, &mut { read }, args),
            Body::Algo(e, m) => crate::algo::invoke(e, *m, &mut { read }, args),
        }
    }
}

/// A routine as the bound form and the plan hold it: compared by identity,
/// shown by name.
#[derive(Clone)]
pub(crate) struct RoutineRef(pub(crate) Arc<Routine>);

impl RoutineRef {
    /// The routine.
    pub(crate) fn get(&self) -> &Routine {
        &self.0
    }

    /// The actions a call can perform, asked before the statement runs
    /// (TS-23): each with whether it concerns every graph (`true`) or the
    /// working graph.
    pub(crate) fn requests(&self) -> Vec<(Action, bool)> {
        match self.0.body {
            Body::Builtin(b) => builtin::requests(b),
            Body::Algo(_, m) => crate::algo::requests(m),
            _ => Vec::new(),
        }
    }
}

impl PartialEq for RoutineRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for RoutineRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.sig.name)
    }
}

/// The routines of one database handle (module documentation).
pub(crate) struct Registry {
    map: BTreeMap<String, Arc<Routine>>,
}

impl Registry {
    /// A registry holding the built-in routines only.
    pub(crate) fn with_builtins() -> Registry {
        let mut map = BTreeMap::new();
        for r in builtin::all().into_iter().chain(crate::algo::routines()) {
            map.insert(r.sig.name.clone(), Arc::new(r));
        }
        Registry { map }
    }

    /// Registers `routine`.
    ///
    /// # Errors
    ///
    /// 22N11 if its name is taken, is empty or names a schema part (`/`),
    /// or if a parameter with a default comes before one without.
    pub(crate) fn register(&mut self, routine: Routine) -> Result<(), Error> {
        let name = &routine.sig.name;
        if name.is_empty() || name.contains('/') {
            return Err(Error::new(
                codes::C22N11,
                "invalid argument: a routine name must be non-empty and have no schema part",
            ));
        }
        let mut defaults = false;
        for p in &routine.sig.params {
            if p.default.is_some() {
                defaults = true;
            } else if defaults {
                return Err(Error::new(
                    codes::C22N11,
                    "invalid argument: a parameter without a default follows one with a default",
                ));
            }
        }
        if self.map.contains_key(name) {
            return Err(Error::new(
                codes::C22N11,
                "invalid argument: a routine with this name is registered",
            ));
        }
        self.map.insert(name.clone(), Arc::new(routine));
        Ok(())
    }

    /// The routine named `name`.
    pub(crate) fn get(&self, name: &str) -> Option<RoutineRef> {
        self.map.get(name).cloned().map(RoutineRef)
    }

    /// The names of every routine, in order.
    pub(crate) fn names(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }
}

/// What the API layer provides to statements: authorisation of nested
/// statements and the transaction list of `show_transactions`.
pub(crate) trait Host: Send + Sync {
    /// Asks every request for `principal`, blocking the statement's thread.
    ///
    /// # Errors
    ///
    /// 42NFF for a refused request.
    fn authorise(&self, principal: &str, requests: &[Request]) -> Result<(), Error>;

    /// The `show_transactions` rows `principal` may see (FR-OBS-003).
    fn transactions(&self, principal: &str) -> Vec<Vec<Value>>;
}

/// The registry and host of a session, as statements see them.
#[derive(Clone)]
pub(crate) struct Procs {
    pub(crate) registry: Arc<RwLock<Registry>>,
    pub(crate) host: Option<Arc<dyn Host>>,
}

impl Procs {
    /// A registry of the built-in routines, without a host.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn builtins() -> Procs {
        Procs {
            registry: Arc::new(RwLock::new(Registry::with_builtins())),
            host: None,
        }
    }

    /// The routine named `name`.
    pub(crate) fn get(&self, name: &str) -> Option<RoutineRef> {
        self.registry
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
    }
}

impl PartialEq for Procs {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.registry, &other.registry)
    }
}

impl Eq for Procs {}

impl fmt::Debug for Procs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Procs")
            .field("host", &self.host.is_some())
            .finish_non_exhaustive()
    }
}
