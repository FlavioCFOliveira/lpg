//! Create, open, close and drop of a database location (TS-02, TS-05,
//! TS-11, TS-13, TS-27, TS-28).
//!
//! An [`Engine`] is one open database. Its [`Core`] holds `LOCK` for its
//! whole life, the `CONTROL` file, the data file, the buffer pool, the
//! memory accountant, the unusable latch, the trees, the WAL and the
//! checkpoint state; the WAL writer thread `lpg-wal` and the checkpointer
//! thread `lpg-checkpoint` share it. The memory cap is a required argument
//! of both [`Engine::create`] and [`Engine::open`]; there is no default
//! (FR-LIM-003).
//!
//! - **Open** runs TS-13 steps 1–3 here, steps 4–6 in [`super::recovery`],
//!   then step 8: it creates the first WAL segment if the log was empty and
//!   runs a checkpoint, whose `CONTROL` slot clears the clean-shutdown flag
//!   and whose sequence number becomes this open's salt (TS-09). Only then
//!   do the threads start.
//! - **Create** writes the files of TS-05, then opens the location as open
//!   does: its log is empty and its flag set.
//! - **Close** stops the checkpointer, runs the final checkpoint with the
//!   flag set, stops the writer, then releases `LOCK` (TS-11). An unusable
//!   database runs no checkpoint (TS-12).
//! - **Drop** without close stops the threads without a checkpoint; the
//!   flag stays clear, so the next open recovers (FR-DB-009).

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use super::btree::{LogState, Store, Trees};
use super::checkpoint::{self, Checkpoints};
use super::control::{Control, ControlSlots};
use super::datafile::Datafile;
use super::error::{Error, Health, Result};
use super::freelist::FreeList;
use super::memory::MemoryAccountant;
use super::page::PAGE_SIZE;
use super::pool::{DEFAULT_FLOOR_PERMILLE, Pool, Sizing};
use super::recovery;
use super::wal::{ClosedGroup, OpenGroup, TEMP_SEGMENT, Wal};
use super::{Storage, StorageLock};
use crate::obs::MetricsSink;

/// The on-disk format version this crate reads and writes (TS-28).
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Name of the lock target (TS-27).
pub(crate) const LOCK: &str = "LOCK";
/// Name of the control file (TS-05).
pub(crate) const CONTROL: &str = "CONTROL";

/// Name of the data file of generation `g`.
pub(crate) fn data_name(g: u64) -> String {
    format!("data-{g}")
}

/// The value of `digits` if it is a `u64` in canonical decimal form: no
/// sign and no leading zero, so that it names exactly one generation.
fn decimal(digits: &str) -> Option<u64> {
    let v: u64 = digits.parse().ok()?;
    (v.to_string() == digits).then_some(v)
}

/// The generation of a data file name `data-<g>` (TS-05).
fn data_generation(name: &OsStr) -> Option<u64> {
    decimal(name.to_str()?.strip_prefix("data-")?)
}

/// The generation of a file of the file set: `data-<g>` (TS-05) or a WAL
/// segment `wal-<g>-<lsn>`, with `<lsn>` 16 lowercase hexadecimal digits
/// (TS-09). Any other name is not part of the file set.
fn file_set_generation(name: &OsStr) -> Option<u64> {
    if let Some(g) = data_generation(name) {
        return Some(g);
    }
    let (g, lsn) = name.to_str()?.strip_prefix("wal-")?.split_once('-')?;
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    if lsn.len() != 16 || !lsn.bytes().all(hex) {
        return None;
    }
    decimal(g)
}

/// Checkpoint triggers (TS-11): every `interval` or every `commits`
/// commits, whichever comes first.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tuning {
    /// Time between periodic checkpoints.
    pub(crate) checkpoint_interval: Duration,
    /// Commits between checkpoints.
    pub(crate) checkpoint_commits: u64,
}

impl Default for Tuning {
    /// Neo4j's `db.checkpoint.interval.time` and `db.checkpoint.interval.tx`
    /// defaults, 15 minutes and 100 000 transactions (TS-11).
    fn default() -> Tuning {
        Tuning {
            checkpoint_interval: Duration::from_secs(15 * 60),
            checkpoint_commits: 100_000,
        }
    }
}

/// What recovery did at open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RecoveryStats {
    /// Groups replayed.
    pub(crate) groups: u64,
    /// Page images written.
    pub(crate) pages: u64,
    /// WAL bytes from the redo point to the WAL end.
    pub(crate) bytes: u64,
    /// Whether the log was empty.
    pub(crate) empty_log: bool,
}

/// The state of one open database, shared with its threads.
pub(crate) struct Core {
    pub(crate) health: Arc<Health>,
    pub(crate) durable_lsn: Arc<AtomicU64>,
    pub(crate) accountant: Arc<MemoryAccountant>,
    pub(crate) control: Mutex<ControlSlots>,
    pub(crate) pool: Pool,
    pub(crate) data: Arc<Datafile>,
    pub(crate) trees: Trees,
    pub(crate) wal: Wal,
    pub(crate) checkpoints: Checkpoints,
    pub(crate) sink: Arc<dyn MetricsSink>,
    pub(crate) recovered: RecoveryStats,
    /// Tests only: skip TS-16 step 2, so that the apply meets an overrun.
    #[cfg(test)]
    pub(crate) skip_reserve: std::sync::atomic::AtomicBool,
    /// Declared last, so it is released after every file is closed.
    _lock: Box<dyn StorageLock>,
}

impl Core {
    /// The trees of the database.
    pub(crate) fn store(&self) -> Store<'_> {
        Store {
            pool: &self.pool,
            data: &self.data,
            trees: &self.trees,
            health: &self.health,
        }
    }

    /// The `CONTROL` file, locked.
    pub(crate) fn control(&self) -> MutexGuard<'_, ControlSlots> {
        // A `CONTROL` write either completes or sets the unusable latch.
        self.control.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Closes the open group for the writer, under the commit latch, if a
    /// commit has joined it.
    ///
    /// Once the database is unusable it closes nothing (TS-12): the writer
    /// then fails the waiting commits. Checked under the latch, after any
    /// batch that held it has undone itself or joined.
    fn close_group(&self, spare: Vec<u8>) -> Option<ClosedGroup> {
        let mut st = self.store().lock();
        if self.health.is_unusable() {
            return None;
        }
        let log = st.log();
        log.group.has_commits().then(|| log.group.close(spare))
    }
}

/// One open database: its core and its threads.
pub(crate) struct Engine {
    core: Arc<Core>,
    threads: Vec<JoinHandle<()>>,
}

/// Takes `LOCK` (TS-27). A missing `LOCK` is [`Error::LockMissing`] and is
/// never recreated: another handle may still hold the lock on the deleted
/// file, and a new file would let a second handle open the database
/// (FR-DB-006, FR-DB-007).
fn take_lock(storage: &dyn Storage, path: &Path) -> Result<Box<dyn StorageLock>> {
    storage.try_lock(path).map_err(|e| match e.kind() {
        io::ErrorKind::WouldBlock => Error::Locked,
        io::ErrorKind::NotFound => Error::LockMissing,
        _ => Error::Io(e),
    })
}

/// Whether the location holds `CONTROL`, that is a database (TS-05).
fn has_control(storage: &dyn Storage, location: &Path) -> Result<bool> {
    Ok(storage.list(location)?.iter().any(|n| n == CONTROL))
}

impl Engine {
    /// Creates a database at `location`, an existing directory, and opens
    /// it (TS-05). `memory_cap` bounds the memory the database accounts for
    /// (FR-LIM-003).
    ///
    /// Order: fail if `CONTROL` exists; create and lock `LOCK`; create
    /// `data-1` with its header page and first extent, synced; `sync_dir`;
    /// write `CONTROL` last, synced; `sync_dir`. A location is a database
    /// exactly when `CONTROL` exists, so a crash before the last `sync_dir`
    /// leaves no database, and a later create overwrites what it left.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `memory_cap` is too small for the
    /// buffer pool; [`Error::DatabaseExists`] if `CONTROL` exists, with
    /// nothing changed (FR-DB-003); [`Error::Locked`] if another handle holds
    /// `LOCK`; [`Error::LockMissing`] if `LOCK` is deleted between its
    /// creation and the lock; [`Error::Io`].
    pub(crate) fn create(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
    ) -> Result<Engine> {
        Engine::create_tuned(location, storage, memory_cap, sink, Tuning::default())
    }

    /// [`Engine::create`] with the checkpoint triggers `tuning`.
    ///
    /// # Errors
    ///
    /// As [`Engine::create`].
    pub(crate) fn create_tuned(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
        tuning: Tuning,
    ) -> Result<Engine> {
        Sizing::new(memory_cap, DEFAULT_FLOOR_PERMILLE)?;
        let s = &*storage;
        if has_control(s, location)? {
            return Err(Error::DatabaseExists);
        }
        let lock_path = location.join(LOCK);
        match s.create(&lock_path) {
            Ok(_) => {}
            // Left by an interrupted create.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let lock = take_lock(s, &lock_path)?;
        // Another creator may have finished between the check and the lock.
        if has_control(s, location)? {
            return Err(Error::DatabaseExists);
        }
        for name in s.list(location)? {
            if data_generation(&name).is_some() {
                s.remove(&location.join(name))?;
            }
        }
        let health = Arc::new(Health::default());
        let data = Datafile::create(
            s,
            &location.join(data_name(1)),
            FORMAT_VERSION,
            health.clone(),
            sink.clone(),
        )?;
        // The names of LOCK and data-1 are durable before CONTROL exists.
        s.sync_dir(location)?;
        let control = Control {
            format_version: FORMAT_VERSION,
            page_size: PAGE_SIZE as u32,
            uuid: new_uuid(),
            generation: 1,
            clean_shutdown: true,
            redo_point: 0,
            commit_ts: 0,
            next_element_id: 0,
            next_catalog_id: 0,
            next_token_id: 0,
        };
        let control = ControlSlots::create(s, &location.join(CONTROL), control, health.clone())?;
        s.sync_dir(location)?;
        Engine::start(
            location, storage, memory_cap, sink, health, control, data, lock, tuning,
        )
    }

    /// Opens the database at `location` (TS-13, TS-28). `memory_cap`
    /// bounds the memory the database accounts for (FR-LIM-003).
    ///
    /// Steps: fail if `CONTROL` is absent, creating nothing; take `LOCK`,
    /// failing if it is absent and creating nothing, since recreating it
    /// would let a second handle open the database while the first still
    /// holds the lock on the deleted file; read both `CONTROL` slots and check
    /// the format version; remove the data files and WAL segments of every
    /// other generation and a leftover temporary segment; recover from the
    /// WAL ([`super::recovery`]); read
    /// page 0 of `data-<g>` and check it holds the same version; run the
    /// open checkpoint (TS-13 step 8) and start the threads. A missing
    /// `data-<g>` is corruption and is never recreated.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `memory_cap` is too small for the
    /// buffer pool; [`Error::NoDatabase`] (FR-DB-004); [`Error::Locked`]
    /// (FR-DB-006, FR-DB-007); [`Error::LockMissing`] if `CONTROL` exists
    /// and `LOCK` does not, with no file created (TS-27);
    /// [`Error::Corruption`] if both `CONTROL` slots
    /// are invalid, `data-<g>` is missing, page 0 fails its checksum, the
    /// versions differ (FR-REC-003), a WAL segment is missing or the WAL is
    /// corrupt (FR-REC-008); [`Error::MigrationRequired`] for an older format and
    /// [`Error::NewerFormat`] for a newer one, both before anything is
    /// written (TS-28); [`Error::Io`].
    pub(crate) fn open(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
    ) -> Result<Engine> {
        Engine::open_tuned(location, storage, memory_cap, sink, Tuning::default())
    }

    /// [`Engine::open`] with the checkpoint triggers `tuning`.
    ///
    /// # Errors
    ///
    /// As [`Engine::open`].
    pub(crate) fn open_tuned(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
        tuning: Tuning,
    ) -> Result<Engine> {
        Sizing::new(memory_cap, DEFAULT_FLOOR_PERMILLE)?;
        let s = &*storage;
        // Step 1.
        let control_file = match s.open(&location.join(CONTROL)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Error::NoDatabase),
            Err(e) => return Err(e.into()),
        };
        // Step 2.
        let lock = take_lock(s, &location.join(LOCK))?;
        // Step 3.
        let health = Arc::new(Health::default());
        let control = ControlSlots::open(control_file, health.clone(), &*sink)?;
        let c = *control.current();
        if c.format_version < FORMAT_VERSION {
            return Err(Error::MigrationRequired {
                found: c.format_version,
                required: FORMAT_VERSION,
            });
        }
        if c.format_version > FORMAT_VERSION {
            return Err(Error::NewerFormat {
                found: c.format_version,
                supported: FORMAT_VERSION,
            });
        }
        if c.page_size as usize != PAGE_SIZE {
            return Err(Error::Corruption(format!(
                "CONTROL page size {} differs from {PAGE_SIZE}",
                c.page_size
            )));
        }
        let data_path = location.join(data_name(c.generation));
        let data_file = s.open(&data_path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => {
                Error::Corruption(format!("{} is missing", data_path.display()))
            }
            _ => Error::Io(e),
        })?;
        let data = Datafile::attach(data_file, health.clone(), sink.clone())?;
        let mut removed = false;
        for name in s.list(location)? {
            // Files of other generations (TS-05), and a segment whose
            // creation a crash interrupted before its rename (TS-09).
            if file_set_generation(&name).is_some_and(|g| g != c.generation) || name == TEMP_SEGMENT
            {
                s.remove(&location.join(name))?;
                removed = true;
            }
        }
        if removed {
            s.sync_dir(location)?;
        }
        Engine::start(
            location, storage, memory_cap, sink, health, control, data, lock, tuning,
        )
    }

    /// TS-13 steps 4–6 and 8 on a location whose `LOCK` is held and whose
    /// `CONTROL` was read, then the threads.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts create and open have prepared"
    )]
    fn start(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
        health: Arc<Health>,
        control: ControlSlots,
        data: Datafile,
        lock: Box<dyn StorageLock>,
        tuning: Tuning,
    ) -> Result<Engine> {
        let c = *control.current();
        let rec = recovery::recover(&*storage, location, c.generation, &c, &data)?;
        let header = data.load_header()?;
        if header.format_version != c.format_version {
            return Err(Error::Corruption(format!(
                "data file format version {} differs from CONTROL version {}",
                header.format_version, c.format_version
            )));
        }
        let data = Arc::new(data);
        let accountant = MemoryAccountant::new(memory_cap);
        // Every page in the data file is durable up to the WAL end.
        let durable_lsn = Arc::new(AtomicU64::new(rec.wal_end));
        let pool = Pool::new(
            data.clone(),
            accountant.clone(),
            health.clone(),
            durable_lsn.clone(),
            sink.clone(),
            DEFAULT_FLOOR_PERMILLE,
        )?;
        let free = FreeList::load(&pool, header.free_list_head, header.page_count)?;
        let log = LogState {
            group: OpenGroup::new(1, rec.wal_end, Vec::new(), rec.counters),
            counters: rec.counters,
        };
        let trees = Trees::new(&header, free, log);
        let recovered = RecoveryStats {
            groups: rec.groups,
            pages: rec.pages,
            bytes: rec.bytes,
            empty_log: rec.empty_log,
        };
        let wal = Wal::new(
            location,
            storage,
            c.generation,
            rec.segments,
            durable_lsn.clone(),
            rec.counters,
            health.clone(),
            sink.clone(),
        );
        let core = Arc::new(Core {
            health,
            durable_lsn,
            accountant,
            control: Mutex::new(control),
            pool,
            data,
            trees,
            wal,
            checkpoints: Checkpoints::new(c.redo_point, tuning),
            sink,
            recovered,
            #[cfg(test)]
            skip_reserve: std::sync::atomic::AtomicBool::new(false),
            _lock: lock,
        });
        // Step 8.
        if rec.empty_log {
            core.wal.create_first(rec.wal_end)?;
        }
        checkpoint::run(&core, false)?;
        core.wal.set_salt(core.control().sequence());
        let mut engine = Engine {
            core,
            threads: Vec::with_capacity(2),
        };
        let core = engine.core.clone();
        engine.spawn("lpg-wal", move || {
            core.wal.run_writer(|spare| core.close_group(spare));
        })?;
        let core = engine.core.clone();
        engine.spawn("lpg-checkpoint", move || checkpoint::run_thread(&core))?;
        Ok(engine)
    }

    fn spawn(&mut self, name: &str, f: impl FnOnce() + Send + 'static) -> Result<()> {
        let handle = std::thread::Builder::new().name(name.to_owned()).spawn(f)?;
        self.threads.push(handle);
        Ok(())
    }

    /// Stops the checkpointer and the WAL writer and waits for them. The
    /// writer first makes every joined group durable.
    fn stop_threads(&mut self) {
        self.core.checkpoints.stop();
        self.core.wal.stop();
        for t in self.threads.drain(..) {
            // A panicked thread has nothing left to stop.
            let _ = t.join();
        }
    }

    /// Stops the checkpointer only (clean close, before the final
    /// checkpoint).
    fn stop_checkpointer(&mut self) {
        self.core.checkpoints.stop();
        if self.threads.len() == 2 {
            let _ = self.threads.pop().expect("checkpointer handle").join();
        }
    }

    /// Closes the database cleanly (TS-11): stops the checkpointer, runs
    /// the final checkpoint, whose `CONTROL` slot records a clean shutdown,
    /// stops the WAL writer and releases `LOCK`. No commit can be running:
    /// `close` takes the engine by value.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the database is unusable: then no checkpoint
    /// runs and the flag stays clear, so the next open recovers (TS-11);
    /// errors of the checkpoint. `LOCK` is released in every case.
    pub(crate) fn close(mut self) -> Result<()> {
        self.stop_checkpointer();
        self.core.health.check()?;
        checkpoint::run(&self.core, true)?;
        self.stop_threads();
        Ok(())
    }

    /// Runs a checkpoint now (TS-11 trigger "on request", used by backup).
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`]; an I/O error, which sets the unusable latch.
    pub(crate) fn checkpoint(&self) -> Result<checkpoint::Stats> {
        checkpoint::run(&self.core, false)
    }

    /// The shared state.
    pub(crate) fn core(&self) -> &Core {
        &self.core
    }

    /// What recovery did at open.
    pub(crate) fn recovered(&self) -> RecoveryStats {
        self.core.recovered
    }

    /// Fails with [`Error::Unusable`] once any component has set the latch.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`].
    pub(crate) fn check(&self) -> Result<()> {
        self.core.health.check()
    }

    /// The buffer pool.
    pub(crate) fn pool(&self) -> &Pool {
        &self.core.pool
    }

    /// The data file.
    pub(crate) fn data(&self) -> &Datafile {
        &self.core.data
    }

    /// The memory accountant.
    pub(crate) fn accountant(&self) -> &MemoryAccountant {
        &self.core.accountant
    }

    /// The durable LSN: every WAL record up to it is on stable storage.
    pub(crate) fn durable_lsn(&self) -> &AtomicU64 {
        &self.core.durable_lsn
    }

    /// The trees of the database.
    pub(crate) fn store(&self) -> Store<'_> {
        self.core.store()
    }

    /// The `CONTROL` file.
    pub(crate) fn control(&self) -> &Mutex<ControlSlots> {
        &self.core.control
    }

    /// The WAL.
    pub(crate) fn wal(&self) -> &Wal {
        &self.core.wal
    }
}

impl Drop for Engine {
    /// Drop without close (FR-DB-009): the threads stop without a
    /// checkpoint, the clean-shutdown flag stays clear, and the next open
    /// recovers.
    fn drop(&mut self) {
        self.stop_threads();
    }
}

/// A random (version 4) UUID (RFC 9562 §5.4).
fn new_uuid() -> [u8; 16] {
    let mut u: [u8; 16] = rand::random();
    u[6] = (u[6] & 0x0F) | 0x40;
    u[8] = (u[8] & 0x3F) | 0x80;
    u
}

#[cfg(test)]
mod tests;
