//! Index and constraint creation (TS-18 Online build, Constraints).
//!
//! **Online build** ([`Engine::create_index`], thread `lpg-index-build`,
//! TS-02):
//!
//! 1. A transaction commits the definition POPULATING with its index tree
//!    and side tree, both empty, at `S`; from then on every committer
//!    writes the index's changes to the side tree ([`super::derive`]).
//! 2. The builder scans the latest committed elements of the graph without
//!    a snapshot (each tree read operation registers for page reuse only,
//!    TS-05), skipping delete-stamped entries, and sorts the index keys
//!    with [`crate::storage::sort::Sorter`], memory from the accountant.
//!    The scan can see a commit's batch half applied; if a commit is
//!    undone between the scan's start and the install, the build scans
//!    again ([`scan_and_install`]).
//! 3. It builds the index bottom-up ([`Engine::bulk_build_pages_as`]),
//!    writing its pages with no latch held into pages it owns, and installs
//!    it into the index tree, which keeps its root page
//!    ([`crate::storage::btree::bulk::Built::install_replacing`]): the
//!    entries are frozen. Commits wait for the build only during the short
//!    page-level commits that take its pages
//!    ([`crate::storage::btree::bulk`]).
//! 4. It replays the side tree in chunks of [`CHUNK`] records, each one
//!    page-level commit of frozen writes ([`super::side::replay`]); a
//!    chunk read while a commit was undone is read again ([`commit_chunk`]).
//! 5. A transaction marks the index ONLINE and drops the side tree; its
//!    commit replays, under the commit latch, the records written since
//!    the last chunk ([`super::Online`]).
//!
//! Every write of the builder to a tree happens under the commit latch
//! after it has checked there that the catalog cache still holds the
//! definition POPULATING, so a build whose index or graph is dropped stops
//! without writing to a tree; the pages it built are then released. The
//! trees it reads stay allocated: garbage collection does not free a
//! dropped tree that a build uses ([`Builds::uses`]), which the builder
//! registers under the commit latch while the definition is present. At
//! open, every POPULATING index is built again ([`restart_builds`]): from
//! step 2 if its tree is one empty leaf, else from step 4, since only an
//! install fills the tree; its side tree, logged, still holds every change
//! since `S`, and replaying all of it again is idempotent. A restarted
//! build so never frees an installed tree, whose pages would all be freed
//! in one latched commit. TiDB likewise resumes an interrupted index
//! reorganisation from the progress it persisted instead of starting over
//! (R-TIDB @ `1f819a0` `pkg/ddl/reorg.go:963-1147`, `getReorgInfo`,
//! Apache-2.0, studied only).
//!
//! **Constraints** ([`Engine::create_constraint`]): a transaction takes the
//! X schema lock on the label or edge type in the graph, which waits for
//! its writers and makes later ones wait (FR-IDX-006); validates the
//! latest committed elements; and, for uniqueness and node key, sorts the
//! owned index's keys, failing on two adjacent keys with one value while
//! they stream into the bulk build ([`UniqueSorted`]), and commits the
//! constraint, the ONLINE owned index and its tree together
//! ([`crate::tx::Transaction::commit_installing`]). No side tree is
//! needed: no writer of the label or type runs (TS-18). The bulk build
//! holds no lock that writers of other labels and types need.

use std::collections::BTreeMap;

use super::{
    Entity, Online, SchemaDef, SchemaKind, State, check_constraint, decode_element, element_tree,
    index_keys, prefix_of, read_side, schema_key, side, stat, tree_of, validate,
};
use crate::catalog::CATALOG_TREE;
use crate::storage::btree::bulk::SortedEntries;
use crate::storage::btree::{Keys, Write};
use crate::storage::engine::{Core, Engine};
use crate::storage::error::{Error, Result, Violation};
use crate::storage::graph::element_of;
use crate::storage::sort::{MIN_MEMORY, Sorted};
use crate::sync::atomic::{AtomicBool, Ordering};
use crate::sync::thread::JoinHandle;
use crate::sync::{Mutex, MutexGuard, PoisonError};
use crate::tx::Isolation;
use crate::tx::commit::Ids;
use crate::tx::lock::{Mode, Resource};

/// Side-tree records a builder replays in one page-level commit.
pub(crate) const CHUNK: usize = 128;

/// The largest sort memory of a build: 16 MiB.
const MAX_SORT_MEMORY: u64 = 16 << 20;

/// A point of a build or a constraint creation where a test can pause it
/// (TS-30; as `pause_gc`, a hook of the crate's own, `cfg(test)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Before each scan of step 2.
    Scanning,
    /// The scan of step 2 has ended.
    Scanned,
    /// The index tree is installed, or found installed by an earlier run.
    Installed,
    /// A chunk of side-tree records is read, before its commit.
    ChunkRead,
    /// The first chunk of step 4 is committed.
    Replaying,
    /// Before the ONLINE commit of step 5.
    BeforeOnline,
    /// A constraint creation holds its schema lock, before it validates.
    Validating,
}

/// A test hook called at each [`Phase`].
#[cfg(test)]
pub(crate) type Hook = std::sync::Arc<dyn Fn(Phase) + Send + Sync>;

/// The online index builds of an open database (in [`Core`]).
#[derive(Default)]
pub(crate) struct Builds {
    /// Set by close: builds stop at their next step.
    stop: AtomicBool,
    /// The running builds' threads, by index.
    threads: Mutex<Vec<(u64, JoinHandle<Result<()>>)>>,
    /// Trees the builds read, which garbage collection does not free.
    trees: Mutex<BTreeMap<u64, usize>>,
    /// Tests only: the pause hook.
    #[cfg(test)]
    pub(crate) hook: Mutex<Option<Hook>>,
}

impl std::fmt::Debug for Builds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builds").finish_non_exhaustive()
    }
}

impl Builds {
    fn threads(&self) -> MutexGuard<'_, Vec<(u64, JoinHandle<Result<()>>)>> {
        self.threads.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn trees(&self) -> MutexGuard<'_, BTreeMap<u64, usize>> {
        self.trees.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stops every build and waits for its thread; the indexes stay
    /// POPULATING.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let threads: Vec<_> = self.threads().drain(..).collect();
        for (_, t) in threads {
            let _ = t.join();
        }
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Whether a build reads tree `id` (garbage collection keeps it).
    pub(crate) fn uses(&self, id: u64) -> bool {
        self.trees().contains_key(&id)
    }

    fn take(&self, ids: &[u64]) {
        let mut t = self.trees();
        for &id in ids {
            *t.entry(id).or_insert(0) += 1;
        }
    }

    fn give_back(&self, ids: &[u64]) {
        let mut t = self.trees();
        for id in ids {
            if let Some(n) = t.get_mut(id) {
                *n -= 1;
                if *n == 0 {
                    t.remove(id);
                }
            }
        }
    }
}

/// Calls the test hook at `phase`.
pub(crate) fn pause(core: &Core, phase: Phase) {
    #[cfg(test)]
    {
        let hook = core
            .builds
            .hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(h) = hook {
            h(phase);
        }
    }
    #[cfg(not(test))]
    let _ = (core, phase);
}

/// The sort memory of a build: a sixteenth of the cap, between
/// [`MIN_MEMORY`] and [`MAX_SORT_MEMORY`] (TS-08).
fn sort_memory(core: &Core) -> u64 {
    (core.accountant.cap() / 16).clamp(MIN_MEMORY, MAX_SORT_MEMORY)
}

/// Starts the build of index `index` on its own thread.
fn spawn(engine: &Engine, index: u64) -> Result<()> {
    let core = engine.core_arc();
    let handle = crate::sync::thread::Builder::new()
        .name("lpg-index-build".to_owned())
        .spawn(move || {
            let e = Engine::handle(core);
            build(&e, index)
        })?;
    engine.core().builds.threads().push((index, handle));
    Ok(())
}

/// TS-18 step 7: starts the build of every POPULATING index of the catalog
/// cache, at open.
///
/// # Errors
///
/// A thread that cannot be spawned.
pub(crate) fn restart_builds(engine: &Engine) -> Result<()> {
    #[cfg(test)]
    if HOLD_RESTARTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&engine.core().location)
    {
        return Ok(());
    }
    let ids: Vec<u64> = engine
        .core()
        .catalog
        .schema()
        .values()
        .filter(|s| s.kind.is_index() && s.state == State::Populating)
        .map(|s| s.id)
        .collect();
    for id in ids {
        spawn(engine, id)?;
    }
    Ok(())
}

/// Tests only: locations whose open does not restart builds, so that a
/// test reads the recovered state first and restarts them itself. Keyed
/// by location, so tests in one process do not interfere.
#[cfg(test)]
pub(crate) static HOLD_RESTARTS: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

/// Index `index`'s definition if the catalog cache holds it POPULATING.
fn populating(core: &Core, index: u64) -> Option<SchemaDef> {
    core.catalog
        .schema()
        .get(&index)
        .filter(|s| s.state == State::Populating)
        .cloned()
}

/// The trees a build reads, released when dropped.
struct Uses<'c> {
    core: &'c Core,
    ids: Vec<u64>,
}

impl Drop for Uses<'_> {
    fn drop(&mut self) {
        self.core.builds.give_back(&self.ids);
    }
}

/// Runs the build of index `index` (module documentation). Returns
/// without error, leaving the index as it is, when the database closes or
/// the index or its graph is dropped.
fn build(e: &Engine, index: u64) -> Result<()> {
    let core = e.core();
    let store = e.store();
    // The definition and the trees, read and registered under the latch.
    let (def, elements, _uses) = {
        let _latch = store.lock();
        let Some(def) = populating(core, index) else {
            return Ok(());
        };
        let Some(elements) = element_tree(core, def.graph, def.entity)? else {
            return Ok(());
        };
        let ids = vec![elements.id(), def.tree, def.side];
        core.builds.take(&ids);
        (def, elements, Uses { core, ids })
    };
    // Only the install and the replay write the index tree: a tree that is
    // not one empty leaf was installed by an earlier run of this build,
    // which a crash or a close interrupted. The build resumes at step 4
    // and replays the whole side tree, which is idempotent (`side`), so the
    // old tree is never freed in one latched commit.
    let t = tree_of(&store, def.tree)?;
    let t = if store.is_empty_leaf(t)? {
        match scan_and_install(e, &def, elements, index)? {
            Some(t) => t,
            None => return Ok(()),
        }
    } else {
        t
    };
    pause(core, Phase::Installed);
    // Step 4.
    let side_tree = tree_of(&store, def.side)?;
    let mut from = Vec::new();
    let mut first = true;
    loop {
        if core.builds.stopped() {
            return Ok(());
        }
        // Read before the records: a chunk read while a commit was undone
        // is read again (`Chunk::Undone`).
        let epoch = core.wal.undo_epoch();
        let records = read_side(&store, side_tree, &from, CHUNK)?;
        let Some((last, _, _)) = records.last() else {
            break;
        };
        let last = last.clone();
        pause(core, Phase::ChunkRead);
        let mut target = super::BatchTarget::new(store, t);
        side::replay(
            &mut target,
            records.iter().map(|(_, op, k)| (*op, k.as_slice())),
        )?;
        let writes = target.into_writes();
        match commit_chunk(e, index, epoch, &writes)? {
            Chunk::Gone => return Ok(()),
            Chunk::Undone => continue,
            Chunk::Committed => {}
        }
        from = last;
        if first {
            first = false;
            pause(core, Phase::Replaying);
        }
    }
    // Only this builder writes the index tree until it is ONLINE.
    let (entries, distinct) = super::count(&store, t)?;
    pause(core, Phase::BeforeOnline);
    if core.builds.stopped() {
        return Ok(());
    }
    // Step 5.
    let mut tx = e.begin(Isolation::Serializable)?;
    let Some(mut cur) = tx.schema_def(def.graph, index)? else {
        return Ok(());
    };
    if cur.state != State::Populating {
        return Ok(());
    }
    cur.state = State::Online;
    cur.side = 0;
    let cat = tx.tree(CATALOG_TREE)?;
    tx.put(cat, &schema_key(def.graph, index), &cur.encode())?;
    let s = tx.tree(def.side)?;
    tx.drop_tree(s)?;
    tx.online = Some(Online {
        index,
        from,
        entries,
        distinct,
    });
    tx.commit()?;
    Ok(())
}

/// Steps 2 and 3 of the build of index `def`: scans `elements`, builds
/// the index's pages and installs them. Returns the index tree, or `None`
/// when the database closes or the index or its graph is dropped.
///
/// The scan reads the latest committed pages without the commit latch, so
/// it can see a commit's batch half applied. A batch that then fails is
/// undone under the latch and raises the WAL's undo epoch
/// ([`crate::storage::wal::Wal::undo_epoch`]) before it releases it. The
/// install therefore checks, under the latch, that the epoch is the one
/// read before the scan: if it is not, the scan may hold an element of an
/// undone batch, which no side-tree record would remove, and the build
/// scans again. This is a sequence lock's read-validate-retry (Linux
/// `read_seqbegin`/`read_seqretry`, `include/linux/seqlock.h`, studied
/// only), as the checkpointer revisits its pages after an undo
/// ([`crate::storage::checkpoint`]).
fn scan_and_install(
    e: &Engine,
    def: &SchemaDef,
    elements: crate::storage::btree::Tree,
    index: u64,
) -> Result<Option<crate::storage::btree::Tree>> {
    let core = e.core();
    let store = e.store();
    loop {
        let epoch = core.wal.undo_epoch();
        pause(core, Phase::Scanning);
        // Step 2.
        let mut sorter = e.sorter(sort_memory(core))?;
        let mut err = None;
        let mut seen = 0u64;
        store.scan_prefix_stamped(elements, b"", |k, v, st| {
            seen += 1;
            if seen.is_multiple_of(1024) && core.builds.stopped() {
                return false;
            }
            if st.deleted != 0 {
                return true;
            }
            let (Some(id), Some(el)) = (element_of(k), decode_element(def.entity, v)) else {
                err = Some(Error::Corruption("malformed record".into()));
                return false;
            };
            for key in index_keys(def, id, &el) {
                if let Err(x) = sorter.push(&key, &[]) {
                    err = Some(x);
                    return false;
                }
            }
            true
        })?;
        if let Some(x) = err {
            return Err(x);
        }
        if core.builds.stopped() {
            return Ok(None);
        }
        pause(core, Phase::Scanned);
        let mut sorted = sorter.finish()?;
        // Step 3.
        let old = tree_of(&store, def.tree)?;
        let built = e.bulk_build_pages_as(&mut sorted, def.tree)?;
        drop(sorted);
        // Installed only if, under the commit latch, the index is still
        // POPULATING and no commit was undone since the scan began;
        // otherwise the build's pages are released.
        let mut undone = false;
        let installed = built.install_replacing(old, || {
            if populating(core, index).is_none() {
                return false;
            }
            undone = core.wal.undo_epoch() != epoch;
            !undone
        })?;
        match installed {
            Some(t) => return Ok(Some(t)),
            None if undone => continue,
            None => return Ok(None),
        }
    }
}

/// The outcome of [`commit_chunk`].
enum Chunk {
    /// The chunk's writes are committed.
    Committed,
    /// The index or its graph is dropped: nothing is written.
    Gone,
    /// A commit was undone since the chunk was read: nothing is written,
    /// and the chunk is read again.
    Undone,
}

/// Commits the frozen `writes` of a replay chunk, read when the undo epoch
/// was `epoch`, as one page-level commit if, under the commit latch, the
/// catalog cache still holds index `index` POPULATING and the epoch is
/// unchanged ([`scan_and_install`] gives the reason).
fn commit_chunk(
    e: &Engine,
    index: u64,
    epoch: u64,
    writes: &[super::DerivedWrite],
) -> Result<Chunk> {
    let core = e.core();
    let store = core.store();
    let batch: Vec<Write<'_>> = writes
        .iter()
        .map(|w| Write {
            tree: w.tree,
            key: &w.key,
            value: w.value.as_deref(),
            stamp: 0,
        })
        .collect();
    core.data.ensure(store.estimate(&batch, 0)?)?;
    let started = std::time::Instant::now();
    let st = store.lock();
    let mut outcome = Chunk::Committed;
    let r = e.commit_latched(st, Ids::default(), |store, st, wal, _| {
        if populating(core, index).is_none() {
            outcome = Chunk::Gone;
            return Ok(Vec::new());
        }
        if core.wal.undo_epoch() != epoch {
            outcome = Chunk::Undone;
            return Ok(Vec::new());
        }
        store.apply_batch(st, Some(wal), &batch, 0)
    });
    if matches!(outcome, Chunk::Committed) {
        e.note_commit_result(started, &r);
    }
    r?;
    Ok(outcome)
}

/// The records of a sort of an owned index's keys, as a bulk build's
/// source, failing on two adjacent keys with one value, and counting the
/// distinct values.
///
/// A port of PostgreSQL's `enforceUnique` check (R-PG @ `1f22453`
/// `src/backend/utils/sort/tuplesortvariants.c:1664-1700`): two equal keys
/// of a unique index are equal neighbours in the sorted order, so
/// comparing each key with the one before it finds every duplicate in the
/// pass that builds the index, with no separate validation. PostgreSQL's
/// licence (its `COPYRIGHT` file @ `1f22453`):
///
/// > PostgreSQL Database Management System
/// > (also known as Postgres, formerly known as Postgres95)
/// >
/// > Portions Copyright (c) 1996-2026, PostgreSQL Global Development Group
/// >
/// > Portions Copyright (c) 1994, The Regents of the University of California
/// >
/// > Permission to use, copy, modify, and distribute this software and its
/// > documentation for any purpose, without fee, and without a written
/// > agreement is hereby granted, provided that the above copyright notice
/// > and this paragraph and the following two paragraphs appear in all
/// > copies.
/// >
/// > IN NO EVENT SHALL THE UNIVERSITY OF CALIFORNIA BE LIABLE TO ANY PARTY
/// > FOR DIRECT, INDIRECT, SPECIAL, INCIDENTAL, OR CONSEQUENTIAL DAMAGES,
/// > INCLUDING LOST PROFITS, ARISING OUT OF THE USE OF THIS SOFTWARE AND ITS
/// > DOCUMENTATION, EVEN IF THE UNIVERSITY OF CALIFORNIA HAS BEEN ADVISED OF
/// > THE POSSIBILITY OF SUCH DAMAGE.
/// >
/// > THE UNIVERSITY OF CALIFORNIA SPECIFICALLY DISCLAIMS ANY WARRANTIES,
/// > INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
/// > AND FITNESS FOR A PARTICULAR PURPOSE. THE SOFTWARE PROVIDED HEREUNDER IS
/// > ON AN "AS IS" BASIS, AND THE UNIVERSITY OF CALIFORNIA HAS NO OBLIGATIONS
/// > TO PROVIDE MAINTENANCE, SUPPORT, UPDATES, ENHANCEMENTS, OR
/// > MODIFICATIONS.
pub(crate) struct UniqueSorted<'a> {
    sorted: Sorted<'a>,
    last: Vec<u8>,
    first: bool,
    /// Distinct values so far.
    pub(crate) distinct: u64,
}

impl<'a> UniqueSorted<'a> {
    fn new(sorted: Sorted<'a>) -> UniqueSorted<'a> {
        UniqueSorted {
            sorted,
            last: Vec::new(),
            first: true,
            distinct: 0,
        }
    }
}

impl SortedEntries for UniqueSorted<'_> {
    fn next_entry(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        let Some((k, v)) = self.sorted.next_record()? else {
            return Ok(None);
        };
        let p = prefix_of(k);
        if !self.first && p == self.last.as_slice() {
            return Err(Error::ConstraintViolation(Violation::DuplicateValues));
        }
        self.first = false;
        self.last.clear();
        self.last.extend_from_slice(p);
        self.distinct += 1;
        Ok(Some((k, v)))
    }
}

impl Engine {
    /// Creates an index of `kind` on graph `graph` (FR-IDX-001), on
    /// `entity` with label or edge type `token` (0 for a lookup index) and
    /// properties `props`, and starts its build on thread `lpg-index-build`
    /// (module documentation). Returns its identifier once the definition
    /// is committed POPULATING; [`Engine::wait_index`] waits for the build.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a kind, token or properties that do
    /// not fit; [`Error::NotFound`] for a missing graph; errors of
    /// [`crate::tx::Transaction::commit`]; a thread that cannot be spawned.
    pub(crate) fn create_index(
        &self,
        graph: u64,
        kind: SchemaKind,
        entity: Entity,
        token: u64,
        props: &[u64],
    ) -> Result<u64> {
        if !kind.is_index() {
            return Err(Error::InvalidArgument("not an index kind"));
        }
        validate(self, kind, entity, token, props)?;
        let mut tx = self.begin(Isolation::Serializable)?;
        tx.lock_graph(graph)?;
        let tree = tx.create_tree(Keys::Prefix)?.id();
        let side_tree = tx.create_tree(Keys::Prefix)?.id();
        let id = match self.core().ids.catalog() {
            Ok(id) => id,
            Err(e) => return Err(tx.abort_with(e)),
        };
        let def = SchemaDef {
            id,
            graph,
            kind,
            entity,
            token,
            props: props.to_vec(),
            state: State::Populating,
            tree,
            side: side_tree,
            link: 0,
        };
        let cat = tx.tree(CATALOG_TREE)?;
        tx.put(cat, &schema_key(graph, id), &def.encode())?;
        tx.commit()?;
        spawn(self, id)?;
        Ok(id)
    }

    /// Waits for the build of index `index` started by this open, if one
    /// runs, and returns its result.
    ///
    /// # Errors
    ///
    /// The build's error; [`Error::Unusable`] if its thread panicked.
    pub(crate) fn wait_index(&self, index: u64) -> Result<()> {
        let mine: Vec<_> = {
            let mut t = self.core().builds.threads();
            let (mine, rest) = t.drain(..).partition(|(i, _)| *i == index);
            *t = rest;
            mine
        };
        for (_, h) in mine {
            h.join().map_err(|_| Error::Unusable)??;
        }
        Ok(())
    }

    /// Creates a constraint of `kind` on graph `graph` (FR-IDX-002), on
    /// `entity` with label or edge type `token` and properties `props`
    /// (module documentation): it holds the X schema lock while it
    /// validates the latest committed data, and commits the constraint,
    /// with its ONLINE owned index for uniqueness and node key, only if no
    /// element violates it (FR-IDX-004, FR-IDX-006).
    ///
    /// # Errors
    ///
    /// [`Error::ConstraintViolation`] if an element violates it, with
    /// nothing created; [`Error::InvalidArgument`] for a kind, token or
    /// properties that do not fit; [`Error::NotFound`] for a missing graph;
    /// errors of the lock, the sort, the bulk build and the commit.
    pub(crate) fn create_constraint(
        &self,
        graph: u64,
        kind: SchemaKind,
        entity: Entity,
        token: u64,
        props: &[u64],
    ) -> Result<u64> {
        if kind.is_index() {
            return Err(Error::InvalidArgument("not a constraint kind"));
        }
        validate(self, kind, entity, token, props)?;
        let core = self.core();
        let store = self.store();
        let mut tx = self.begin(Isolation::Serializable)?;
        let g = tx.lock_graph(graph)?;
        tx.lock_resource(Resource::Schema { graph, token }, Mode::X)?;
        pause(core, Phase::Validating);
        let draw = |tx: &mut crate::tx::Transaction<'_>| match core.ids.catalog() {
            Ok(id) => Ok(id),
            Err(e) => Err(tx.abort_with(e)),
        };
        let id = draw(&mut tx)?;
        let owned = if kind.owns_index() {
            Some(SchemaDef {
                id: draw(&mut tx)?,
                graph,
                kind: SchemaKind::Range,
                entity,
                token,
                props: props.to_vec(),
                state: State::Online,
                tree: store.draw_tree_id(),
                side: 0,
                link: id,
            })
        } else {
            None
        };
        let def = SchemaDef {
            id,
            graph,
            kind,
            entity,
            token,
            props: props.to_vec(),
            state: State::Online,
            tree: 0,
            side: 0,
            link: owned.as_ref().map_or(0, |o| o.id),
        };
        // The latest committed elements: their writers wait for the schema
        // lock, so they are stable.
        let elements = match entity {
            Entity::Node => g.nodes,
            Entity::Edge => g.edges,
        };
        let mut sorter = match &owned {
            Some(_) => Some(self.sorter(sort_memory(core))?),
            None => None,
        };
        let mut err = None;
        store.scan_prefix_stamped(elements, b"", |k, v, st| {
            if st.deleted != 0 {
                return true;
            }
            let (Some(eid), Some(el)) = (element_of(k), decode_element(entity, v)) else {
                err = Some(Error::Corruption("malformed record".into()));
                return false;
            };
            if !el.tokens.contains(&token) {
                return true;
            }
            if let Err(x) = check_constraint(&def, &el) {
                err = Some(x);
                return false;
            }
            if let (Some(ix), Some(s)) = (&owned, sorter.as_mut()) {
                for key in index_keys(ix, eid, &el) {
                    if let Err(x) = s.push(&key, &[]) {
                        err = Some(x);
                        return false;
                    }
                }
            }
            true
        })?;
        if let Some(x) = err {
            return Err(x);
        }
        let cat = tx.tree(CATALOG_TREE)?;
        tx.put(cat, &schema_key(graph, id), &def.encode())?;
        let Some(ix) = owned else {
            tx.commit()?;
            return Ok(id);
        };
        tx.put(cat, &schema_key(graph, ix.id), &ix.encode())?;
        let mut source = UniqueSorted::new(sorter.expect("made with owned").finish()?);
        let built = self.bulk_build_pages_as(&mut source, ix.tree)?;
        for (distinct, n) in [(false, built.entries), (true, source.distinct)] {
            tx.schema_stats.add(
                stat(graph, ix.id, distinct),
                i64::try_from(n).unwrap_or(i64::MAX),
            );
        }
        drop(source);
        tx.commit_installing(built)?;
        Ok(id)
    }
}
