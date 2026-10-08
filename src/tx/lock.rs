//! The lock table (TS-15 Lock table, Queue, Deadlocks).
//!
//! Transactions lock resources in two modes, S and X; X conflicts with
//! every mode, S only with X. Locks are held until the transaction ends
//! (TS-16 step 10).
//!
//! - **Queue.** A request is granted when it conflicts with no lock that
//!   another transaction holds and with no earlier waiting request;
//!   otherwise it waits, in arrival order. PostgreSQL makes a request that
//!   conflicts with a waiter's request wait the same way (`lock->waitMask`,
//!   R-PG @ `1f22453` `src/backend/storage/lmgr/lock.c:1135-1145`), so a
//!   stream of S requests cannot keep an X request waiting without bound
//!   (FR-IDX-006).
//! - **Upgrade.** A request for X by a holder of S waits only for the other
//!   holders of S and goes ahead of every waiting request, as PostgreSQL
//!   puts a requester that already holds a conflicting lock ahead of the
//!   waiters it conflicts with (`src/backend/storage/lmgr/proc.c:1251-1266`)
//!   and Neo4j upgrades in place (`ForsetiClient.java:784-802`).
//! - **Deadlocks.** Checked when a request would wait: a depth-first search
//!   of the waits-for graph from the requester. Edges run from a waiting
//!   request to every other transaction that holds a conflicting lock and,
//!   except for an upgrade, to every earlier waiting request that conflicts
//!   with it. If the search returns to the requester, the request closes a
//!   cycle: it is refused with [`Error::Deadlock`] (GQLSTATUS 50N05) and the
//!   requester aborts (FR-TX-007). PostgreSQL's `FindLockCycleRecurse`
//!   searches the same graph, hard edges to holders and soft edges to
//!   earlier waiters, and reports a deadlock only for a cycle through the
//!   start point (R-PG `src/backend/storage/lmgr/deadlock.c:444-530`); it
//!   runs after `deadlock_timeout` and may pick another victim, which TS-15
//!   rejects.
//! - **Waking.** A transaction that ends releases every lock it holds and
//!   grants, resource by resource, the waiting requests the queue rule now
//!   allows; the waiters wake on one condition variable.
//!
//! The table is one mutex over the whole map, with no file I/O, so `loom`
//! explores it as it runs (TS-30).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::storage::error::{Error, Result};
use crate::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// A transaction identifier.
pub(crate) type TxId = u64;

/// A lock mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Shared.
    S,
    /// Exclusive.
    X,
}

impl Mode {
    /// Whether a lock in `self` and one in `other`, held by two
    /// transactions, conflict.
    fn conflicts(self, other: Mode) -> bool {
        self == Mode::X || other == Mode::X
    }
}

/// A lockable resource (TS-15 Lock table). The transaction API over trees
/// locks keys and whole trees. Of the graph resources of TS-15, node
/// content, edge and catalog name are the keys that hold them: the node's
/// key in its graph's node tree, the edge's key in its edge tree, and the
/// `(parent_id, name)` key in the catalog tree ([`crate::catalog`],
/// [`crate::storage::graph`]). Graph, schema and node existence have no key
/// and are resources of their own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Resource {
    /// One key of a tree.
    Key {
        /// Tree identifier.
        tree: u64,
        /// The key.
        key: Box<[u8]>,
    },
    /// A whole tree: S for every writer of it, X to drop it.
    Tree(u64),
    /// A graph, by its catalog object identifier: S for every writer of
    /// the graph, X for a fast initial load (TS-26).
    Graph(u64),
    /// A label or an edge type of a graph (schema): S for every writer of
    /// it, X for a constraint creation (TS-18).
    Schema {
        /// The graph's catalog object identifier.
        graph: u64,
        /// The label or edge-type token.
        token: u64,
    },
    /// The existence of a node: X to delete it, S to create or delete an
    /// edge at it. Label and property changes do not take it.
    Existence {
        /// The graph's catalog object identifier.
        graph: u64,
        /// The node's element identifier.
        node: u64,
    },
    /// A value of a uniqueness or node-key constraint: X for every writer
    /// of an element with that value (TS-15 unique value, TS-18).
    Unique {
        /// The constraint's catalog object identifier.
        constraint: u64,
        /// The key encoding of the constrained values ([`crate::types::key`]).
        value: Box<[u8]>,
    },
}

impl Resource {
    /// Bytes a lock entry on this resource is charged to the memory cap
    /// with (TS-14, TS-08): the resource, its holder entry and its map
    /// entry.
    pub(crate) fn charge(&self) -> u64 {
        let key = match self {
            Resource::Key { key, .. } => key.len() as u64,
            Resource::Unique { value, .. } => value.len() as u64,
            Resource::Tree(_)
            | Resource::Graph(_)
            | Resource::Schema { .. }
            | Resource::Existence { .. } => 0,
        };
        key + 128
    }
}

/// A waiting request.
#[derive(Debug, Clone, Copy)]
struct Waiter {
    tx: TxId,
    mode: Mode,
    /// An S holder asking for X.
    upgrade: bool,
}

/// The locks of one resource.
#[derive(Debug, Default)]
struct Entry {
    /// Holders, each once, with the strongest mode it holds.
    granted: Vec<(TxId, Mode)>,
    /// Waiting requests in queue order; upgrades first.
    waiting: VecDeque<Waiter>,
}

impl Entry {
    /// The mode `tx` holds, if any.
    fn held(&self, tx: TxId) -> Option<Mode> {
        self.granted.iter().find(|g| g.0 == tx).map(|g| g.1)
    }

    /// Whether the waiter at queue position `i` (or a new request at the
    /// end, `i == waiting.len()`) may be granted now (TS-15 Queue).
    fn grantable(&self, w: Waiter, i: usize) -> bool {
        let held = self
            .granted
            .iter()
            .any(|&(t, m)| t != w.tx && m.conflicts(w.mode));
        if held {
            return false;
        }
        if w.upgrade {
            return true;
        }
        !self
            .waiting
            .iter()
            .take(i)
            .any(|e| e.tx != w.tx && e.mode.conflicts(w.mode))
    }

    /// The transactions a waiter at position `i` waits for: the other
    /// holders of a conflicting lock and, except for an upgrade, every
    /// earlier waiting request that conflicts with it (TS-15 Queue).
    fn blockers(&self, w: Waiter, i: usize, out: &mut Vec<TxId>) {
        for &(t, m) in &self.granted {
            if t != w.tx && m.conflicts(w.mode) {
                out.push(t);
            }
        }
        if !w.upgrade {
            for e in self.waiting.iter().take(i) {
                if e.tx != w.tx && e.mode.conflicts(w.mode) {
                    out.push(e.tx);
                }
            }
        }
    }
}

/// The state under the table's mutex.
#[derive(Debug, Default)]
struct Table {
    entries: HashMap<Resource, Entry>,
    /// The resource each waiting transaction waits on: a transaction makes
    /// one request at a time, so it waits on at most one.
    waits_on: HashMap<TxId, Resource>,
    /// The resources each transaction holds or waits for.
    owned: HashMap<TxId, Vec<Resource>>,
}

impl Table {
    /// The transactions `tx` waits for, if it waits.
    fn waits_for(&self, tx: TxId, out: &mut Vec<TxId>) {
        let Some(res) = self.waits_on.get(&tx) else {
            return;
        };
        let e = &self.entries[res];
        let i = e
            .waiting
            .iter()
            .position(|w| w.tx == tx)
            .expect("a waiting transaction is queued");
        e.blockers(e.waiting[i], i, out);
    }

    /// Whether a request `w` that would wait on `res` closes a cycle of the
    /// waits-for graph: a depth-first search from the requester's
    /// blockers that reaches the requester (TS-15 Deadlocks).
    fn closes_cycle(&self, res: &Resource, w: Waiter) -> bool {
        let e = &self.entries[res];
        let at = if w.upgrade { 0 } else { e.waiting.len() };
        let mut stack = Vec::new();
        e.blockers(w, at, &mut stack);
        let mut seen = HashSet::new();
        while let Some(t) = stack.pop() {
            if t == w.tx {
                return true;
            }
            if seen.insert(t) {
                self.waits_for(t, &mut stack);
            }
        }
        false
    }

    /// Grants, in queue order, every waiting request on `res` that the
    /// queue rule allows. Returns whether any was granted.
    fn grant_waiters(&mut self, res: &Resource) -> bool {
        let Some(e) = self.entries.get_mut(res) else {
            return false;
        };
        let mut any = false;
        let mut i = 0;
        while i < e.waiting.len() {
            let w = e.waiting[i];
            if e.grantable(w, i) {
                e.waiting.remove(i);
                match e.granted.iter_mut().find(|g| g.0 == w.tx) {
                    Some(g) => g.1 = Mode::X,
                    None => e.granted.push((w.tx, w.mode)),
                }
                self.waits_on.remove(&w.tx);
                any = true;
            } else {
                i += 1;
            }
        }
        any
    }
}

/// How a lock request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Granted {
    /// The transaction already held the lock in this mode or a stronger
    /// one: no new entry.
    Already,
    /// Granted without waiting.
    Now,
    /// Granted after waiting.
    AfterWait,
    /// An S holder's X request, granted now or after waiting: the
    /// holder's entry changes mode, so no new entry.
    Upgraded,
}

/// The lock table of one database.
#[derive(Debug, Default)]
pub(crate) struct LockTable {
    table: Mutex<Table>,
    /// Waiters wait here; every release that grants wakes them all.
    cv: Condvar,
}

impl LockTable {
    fn table(&self) -> MutexGuard<'_, Table> {
        // The table's updates complete before any call that can panic.
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Locks `res` in `mode` for `tx`, waiting while the queue rule
    /// requires (TS-15).
    ///
    /// # Errors
    ///
    /// [`Error::Deadlock`] if waiting would close a cycle of waiting
    /// transactions; the request is then not queued and `tx` keeps the
    /// locks it holds, which the caller releases when it aborts.
    pub(crate) fn lock(&self, tx: TxId, res: &Resource, mode: Mode) -> Result<Granted> {
        let mut t = self.table();
        let held = t.entries.get(res).and_then(|e| e.held(tx));
        if held == Some(Mode::X) || held == Some(mode) {
            return Ok(Granted::Already);
        }
        let w = Waiter {
            tx,
            mode,
            upgrade: held == Some(Mode::S),
        };
        let e = t.entries.entry(res.clone()).or_default();
        let at = if w.upgrade { 0 } else { e.waiting.len() };
        if e.grantable(w, at) {
            match e.granted.iter_mut().find(|g| g.0 == tx) {
                Some(g) => g.1 = mode,
                None => {
                    e.granted.push((tx, mode));
                    t.owned.entry(tx).or_default().push(res.clone());
                }
            }
            return Ok(if w.upgrade {
                Granted::Upgraded
            } else {
                Granted::Now
            });
        }
        if t.closes_cycle(res, w) {
            if t.entries[res].granted.is_empty() && t.entries[res].waiting.is_empty() {
                t.entries.remove(res);
            }
            return Err(Error::Deadlock);
        }
        let e = t.entries.get_mut(res).expect("entry created above");
        if w.upgrade {
            // Ahead of every waiter, behind earlier upgrades.
            let i = e.waiting.iter().take_while(|x| x.upgrade).count();
            e.waiting.insert(i, w);
        } else {
            e.waiting.push_back(w);
        }
        t.waits_on.insert(tx, res.clone());
        if !w.upgrade {
            t.owned.entry(tx).or_default().push(res.clone());
        }
        loop {
            t = self.cv.wait(t).unwrap_or_else(PoisonError::into_inner);
            if !t.waits_on.contains_key(&tx) {
                return Ok(if w.upgrade {
                    Granted::Upgraded
                } else {
                    Granted::AfterWait
                });
            }
        }
    }

    /// Releases every lock `tx` holds (TS-16 step 10, or its abort) and
    /// grants the waiting requests this allows.
    pub(crate) fn release_all(&self, tx: TxId) {
        let mut t = self.table();
        let Some(owned) = t.owned.remove(&tx) else {
            return;
        };
        let mut woke = false;
        for res in owned {
            if let Some(e) = t.entries.get_mut(&res) {
                e.granted.retain(|g| g.0 != tx);
                e.waiting.retain(|w| w.tx != tx);
            }
            woke |= t.grant_waiters(&res);
            if t.entries
                .get(&res)
                .is_some_and(|e| e.granted.is_empty() && e.waiting.is_empty())
            {
                t.entries.remove(&res);
            }
        }
        t.waits_on.remove(&tx);
        drop(t);
        if woke {
            self.cv.notify_all();
        }
    }

    /// The number of waiting requests, so a test can see a request wait
    /// without timing it.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.table().waits_on.len()
    }

    /// The locks `tx` holds, for tests.
    #[cfg(test)]
    pub(crate) fn held(&self, tx: TxId) -> Vec<(Resource, Mode)> {
        let t = self.table();
        let mut out: Vec<_> = t
            .entries
            .iter()
            .filter_map(|(r, e)| e.held(tx).map(|m| (r.clone(), m)))
            .collect();
        out.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        out
    }

    /// Whether no lock is held or awaited.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        let t = self.table();
        t.entries.is_empty() && t.waits_on.is_empty() && t.owned.is_empty()
    }
}

#[cfg(test)]
mod tests;
