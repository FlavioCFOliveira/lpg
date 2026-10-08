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
//! - **Create** writes the files of TS-05, `data-1` with the catalog tree
//!   holding the default schema and default graph owned by the principal it
//!   receives, the statistics tree and the default graph's trees (TS-17
//!   Creation), then opens the location as open does: its log is empty and
//!   its flag set.
//! - **Open** also loads the catalog (tokens, cache, statistics) before its
//!   checkpoint, recounting the statistics after a crash (TS-13 step 7;
//!   [`crate::catalog::open`]).
//! - **Close** stops the checkpointer, runs the final checkpoint with the
//!   flag set, stops the writer, then releases `LOCK` (TS-11). An unusable
//!   database runs no checkpoint (TS-12).
//! - **Drop** without close stops the threads without a checkpoint; the
//!   flag stays clear, so the next open recovers (FR-DB-009).

use crate::sync::atomic::{AtomicU64, Ordering};
use crate::sync::thread::JoinHandle;
use crate::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
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
use super::sort::is_sort_temp;
use super::wal::{ClosedGroup, OpenGroup, TEMP_SEGMENT, Wal};
use super::{Storage, StorageLock};
use crate::catalog::{self, Catalog};
use crate::obs::MetricsSink;
use crate::tx::commit::{IdCounters, Ids};
use crate::tx::gc;
use crate::tx::mvcc::Mvcc;

/// The on-disk format version this crate reads and writes (TS-28).
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Name of the lock target (TS-27).
pub(crate) const LOCK: &str = "LOCK";
/// Name of the control file (TS-05).
pub(crate) const CONTROL: &str = "CONTROL";
/// Name `CONTROL` is written under before its rename (TS-05, TS-26 restore
/// step 5).
pub(crate) const CONTROL_TMP: &str = "CONTROL.tmp";
/// Name of the file a restore writes before `CONTROL` (TS-26 restore steps 4
/// and 6): the backup's `stop`, which recovery must reach. Every open of
/// the location enforces it until an open has recovered past `stop` and
/// written its checkpoint; that open removes the file. PostgreSQL keeps the
/// same requirement in `backup_label` and then in `pg_control`
/// (`backupEndRequired`, R-PG `src/backend/access/transam/xlogrecovery.c:546-560,
/// 931-964`), and refuses to start with "WAL ends before end of online
/// backup" (`src/backend/access/transam/xlog.c:6583-6599`).
pub(crate) const BACKUP_END: &str = "BACKUP_END";

/// The content of [`BACKUP_END`]: `stop`, then the CRC-32C of those 8
/// bytes, little-endian.
pub(crate) fn encode_backup_end(stop: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[..8].copy_from_slice(&stop.to_le_bytes());
    let crc = crc32c::crc32c(&b[..8]);
    b[8..].copy_from_slice(&crc.to_le_bytes());
    b
}

/// The `stop` a [`BACKUP_END`] file holds, or `None` if it is damaged.
fn decode_backup_end(b: &[u8]) -> Option<u64> {
    let (stop, crc) = (b.get(..8)?, b.get(8..12)?);
    (b.len() == 12 && crc32c::crc32c(stop).to_le_bytes() == crc)
        .then(|| u64::from_le_bytes(stop.try_into().expect("8 bytes")))
}

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
pub(crate) fn file_set_generation(name: &OsStr) -> Option<u64> {
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

/// Whether create or open removes `name` from a location whose live
/// generation is `live` (`None` for a location without `CONTROL`): a file
/// of the file set of any other generation, a segment or a `CONTROL` whose
/// creation a crash interrupted before its rename (TS-09, TS-05), or the run
/// file of an interrupted sort ([`super::sort`]). Only names the crate
/// creates match; `LOCK` and [`BACKUP_END`] never do.
fn is_leftover(name: &OsStr, live: Option<u64>) -> bool {
    file_set_generation(name).is_some_and(|g| Some(g) != live)
        || name == TEMP_SEGMENT
        || name == CONTROL_TMP
        || is_sort_temp(name)
}

/// Checkpoint triggers (TS-11): every `interval` or every `commits`
/// commits, whichever comes first; and the period of garbage collection
/// (TS-14).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tuning {
    /// Time between periodic checkpoints.
    pub(crate) checkpoint_interval: Duration,
    /// Commits between checkpoints.
    pub(crate) checkpoint_commits: u64,
    /// Time between periodic garbage-collection passes; commits also wake
    /// the collector.
    pub(crate) gc_interval: Duration,
}

impl Default for Tuning {
    /// Neo4j's `db.checkpoint.interval.time` and `db.checkpoint.interval.tx`
    /// defaults, 15 minutes and 100 000 transactions (TS-11), and
    /// Memgraph's garbage-collection period, 30 seconds (TS-14).
    fn default() -> Tuning {
        Tuning {
            checkpoint_interval: Duration::from_secs(15 * 60),
            checkpoint_commits: 100_000,
            gc_interval: Duration::from_secs(30),
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
    /// Whether `CONTROL` recorded a clean shutdown.
    pub(crate) clean: bool,
}

/// The state of one open database, shared with its threads.
pub(crate) struct Core {
    /// The database location.
    pub(crate) location: PathBuf,
    /// The storage of the location.
    pub(crate) storage: Arc<dyn Storage>,
    pub(crate) health: Arc<Health>,
    pub(crate) durable_lsn: Arc<AtomicU64>,
    pub(crate) accountant: Arc<MemoryAccountant>,
    pub(crate) control: Mutex<ControlSlots>,
    pub(crate) pool: Pool,
    pub(crate) data: Arc<Datafile>,
    pub(crate) trees: Trees,
    pub(crate) wal: Wal,
    pub(crate) checkpoints: Checkpoints,
    /// The MVCC state (TS-14); empty at open (TS-13 step 7).
    pub(crate) mvcc: Mvcc,
    /// The identifier counters (TS-07), from the recovered high-water
    /// marks.
    pub(crate) ids: IdCounters,
    /// The catalog state: tokens, cache, statistics (TS-17).
    pub(crate) catalog: Catalog,
    pub(crate) sink: Arc<dyn MetricsSink>,
    pub(crate) recovered: RecoveryStats,
    /// Sorts started in this open: the number of the next one's files.
    pub(crate) sorts: AtomicU64,
    /// Held by a backup and by the bulk build of a fast initial load for
    /// their whole run: they exclude each other (TS-26, FR-OPS-001). A
    /// fast load that finds it held waits ([`Core::backup_or_load`]); a
    /// backup that finds it held fails ([`Core::try_backup_or_load`]).
    /// Taken before [`Core::backup_or_install`] and the commit latch.
    pub(crate) backup_or_load: Mutex<()>,
    /// Tests only: how often a bulk build found [`Core::backup_or_load`]
    /// held and waited for it.
    #[cfg(test)]
    pub(crate) load_waits: AtomicU64,
    /// Held exclusively by a backup for its whole run, and shared by each
    /// step of a bulk build that writes pages the WAL does not cover and
    /// commits their use: its install and each step of its release
    /// ([`crate::storage::btree::bulk`]). A backup therefore waits only
    /// for the steps in progress, and such a step waits for a running
    /// backup. Taken after [`Core::backup_or_load`] and before the commit
    /// latch.
    pub(crate) backup_or_install: RwLock<()>,
    /// Tests only: how often a step of a bulk build found
    /// [`Core::backup_or_install`] held by a backup and waited for it.
    #[cfg(test)]
    pub(crate) install_waits: AtomicU64,
    /// Tests only: skip TS-16 step 2, so that the apply meets an overrun.
    #[cfg(test)]
    pub(crate) skip_reserve: crate::sync::atomic::AtomicBool,
    /// The online index builds of this open (TS-18).
    pub(crate) builds: crate::index::Builds,
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

    /// The backup-or-load latch ([`Core::backup_or_load`]), for a fast
    /// initial load: waits while a backup holds it (TS-26 "a load requested
    /// while a backup runs waits"). Counts a wait in tests. A panic under it
    /// leaves nothing half done that the latch protects.
    pub(crate) fn backup_or_load(&self) -> MutexGuard<'_, ()> {
        match self.backup_or_load.try_lock() {
            Ok(g) => g,
            Err(crate::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(crate::sync::TryLockError::WouldBlock) => {
                #[cfg(test)]
                self.load_waits
                    .fetch_add(1, crate::sync::atomic::Ordering::SeqCst);
                self.backup_or_load
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
            }
        }
    }

    /// The backup-or-load latch ([`Core::backup_or_load`]), for a backup:
    /// `None`, without waiting, while a bulk build holds it (FR-OPS-001 "a
    /// backup requested during a fast initial load fails").
    pub(crate) fn try_backup_or_load(&self) -> Option<MutexGuard<'_, ()>> {
        match self.backup_or_load.try_lock() {
            Ok(g) => Some(g),
            Err(crate::sync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
            Err(crate::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// The backup-or-install latch ([`Core::backup_or_install`]), shared,
    /// for one install or release step of a bulk build: waits while a
    /// backup holds it. Counts a wait in tests.
    pub(crate) fn install_step(&self) -> RwLockReadGuard<'_, ()> {
        match self.backup_or_install.try_read() {
            Ok(g) => g,
            Err(crate::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(crate::sync::TryLockError::WouldBlock) => {
                #[cfg(test)]
                self.install_waits
                    .fetch_add(1, crate::sync::atomic::Ordering::SeqCst);
                self.backup_or_install
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
            }
        }
    }

    /// The backup-or-install latch ([`Core::backup_or_install`]),
    /// exclusive, for a backup: waits for the install and release steps in
    /// progress. Linux's `std` read-write lock lets no new reader in while
    /// a writer waits (`is_read_lockable`,
    /// `library/std/src/sys/sync/rwlock/futex.rs:47-54`, Rust 1.99.0), so
    /// the wait is bounded by one step of each running build.
    pub(crate) fn backup_installs(&self) -> RwLockWriteGuard<'_, ()> {
        self.backup_or_install
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The `CONTROL` file, locked.
    pub(crate) fn control(&self) -> MutexGuard<'_, ControlSlots> {
        // A `CONTROL` write either completes or sets the unusable latch.
        self.control.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Closes the open group for the writer, under the commit latch, if a
    /// member, a commit or an internal batch, has joined it (TS-09
    /// Members).
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
        log.group.has_members().then(|| log.group.close(spare))
    }
}

/// One open database: its core and its threads.
pub(crate) struct Engine {
    core: Arc<Core>,
    threads: Vec<JoinHandle<()>>,
    /// Whether this handle owns the database: its drop stops the threads.
    /// An index build thread works through a handle that does not
    /// ([`Engine::handle`]).
    owner: bool,
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
    /// (FR-LIM-003). `owner` is the principal recorded as the owner of the
    /// default schema and default graph; it is required and has no default
    /// (FR-DB-005, FR-AUTH-002). No authorisation request is made.
    ///
    /// Order: fail if `CONTROL` exists; create and lock `LOCK`; fail if
    /// `CONTROL` exists now; remove every file of the file set, `wal-temp`,
    /// `CONTROL.tmp`, [`BACKUP_END`] and sort run files left by an
    /// interrupted create or restore, never other files; create
    /// `data-1` with its header page and first extent, synced; `sync_dir`;
    /// write `CONTROL` last, as `CONTROL.tmp`, synced, renamed to `CONTROL`;
    /// `sync_dir`. A location is a database exactly when `CONTROL` exists,
    /// and it exists only whole, so a failure or a crash before the rename
    /// leaves no database, and a later create overwrites what it left.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `memory_cap` is too small for the
    /// buffer pool, or `owner` is empty or longer than 1 024 bytes, both with
    /// nothing changed; [`Error::DatabaseExists`] if `CONTROL` exists, with
    /// nothing changed (FR-DB-003); [`Error::Locked`] if another handle holds
    /// `LOCK`; [`Error::LockMissing`] if `LOCK` is deleted between its
    /// creation and the lock; [`Error::Io`].
    pub(crate) fn create(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        owner: &str,
        sink: Arc<dyn MetricsSink>,
    ) -> Result<Engine> {
        Engine::create_tuned(
            location,
            storage,
            memory_cap,
            owner,
            sink,
            Tuning::default(),
        )
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
        owner: &str,
        sink: Arc<dyn MetricsSink>,
        tuning: Tuning,
    ) -> Result<Engine> {
        Sizing::new(memory_cap, DEFAULT_FLOOR_PERMILLE)?;
        let trees = catalog::bootstrap(owner)?;
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
            // Without `CONTROL` the location holds no database (TS-05), so
            // every file of the file set and every leftover the crate
            // names is stale, as is the backup end of a restore that never
            // wrote `CONTROL`: none of them applies to the new database. A
            // WAL segment of another database would otherwise be replayed
            // from the new redo point 0. As PostgreSQL removes its leftover
            // temporary files at startup (R-PG
            // `src/backend/storage/file/fd.c:3321`), only names the crate
            // creates are removed.
            if is_leftover(&name, None) || name == BACKUP_END {
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
            &catalog::bootstrap_trees(&trees),
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
            next_element_id: catalog::created_ids().next_element_id,
            next_catalog_id: catalog::created_ids().next_catalog_id,
            next_token_id: catalog::created_ids().next_token_id,
        };
        let control = ControlSlots::create(s, location, control, health.clone())?;
        Engine::start(
            location, storage, memory_cap, sink, health, control, data, lock, tuning, 0,
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
    /// other generation, a leftover temporary segment, a leftover
    /// `CONTROL.tmp` and the temporary files of interrupted sorts
    /// ([`super::sort`]); recover from the WAL ([`super::recovery`]), which
    /// must reach the `stop` of a [`BACKUP_END`] left by a restore; read
    /// page 0 of `data-<g>` and check it holds the same version; run the
    /// open checkpoint (TS-13 step 8) and start the threads; then remove
    /// [`BACKUP_END`]. A missing `data-<g>` is corruption and is never
    /// recreated.
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
    /// corrupt (FR-REC-008), or the WAL ends before the `stop` of a
    /// [`BACKUP_END`] or that file is damaged (TS-26 restore step 6);
    /// [`Error::MigrationRequired`] for an older format and
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
        let mut min_wal_end = 0;
        let mut backup_end = false;
        for name in s.list(location)? {
            if name == BACKUP_END {
                let f = s.open(&location.join(BACKUP_END))?;
                let mut b = [0u8; 13];
                let n = f.read_at(&mut b, 0)?;
                min_wal_end = decode_backup_end(&b[..n])
                    .ok_or_else(|| Error::Corruption(format!("{BACKUP_END} fails its checksum")))?;
                backup_end = true;
                continue;
            }
            // Files of other generations (TS-05); a segment or a `CONTROL`
            // whose creation a crash interrupted before its rename (TS-09,
            // TS-05); the run files of a sort a crash interrupted, as
            // PostgreSQL removes temporary files at startup (R-PG
            // `src/backend/postmaster/postmaster.c:1332`).
            if is_leftover(&name, Some(c.generation)) {
                s.remove(&location.join(name))?;
                removed = true;
            }
        }
        if removed {
            s.sync_dir(location)?;
        }
        let engine = Engine::start(
            location,
            storage.clone(),
            memory_cap,
            sink,
            health,
            control,
            data,
            lock,
            tuning,
            min_wal_end,
        )?;
        if backup_end {
            // Recovery reached `stop` and the open checkpoint recorded a
            // redo point at or past it, so later opens no longer need it.
            storage.remove(&location.join(BACKUP_END))?;
            storage.sync_dir(location)?;
        }
        Ok(engine)
    }

    /// TS-13 steps 4–6 and 8 on a location whose `LOCK` is held and whose
    /// `CONTROL` was read, then the threads. Recovery must reach
    /// `min_wal_end` (TS-26 restore step 6).
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
        min_wal_end: u64,
    ) -> Result<Engine> {
        let c = *control.current();
        let rec = recovery::recover(&*storage, location, c.generation, &c, &data)?;
        if rec.wal_end < min_wal_end {
            // PostgreSQL: "WAL ends before end of online backup" (R-PG
            // `src/backend/access/transam/xlog.c:6598`).
            return Err(Error::Corruption(format!(
                "the WAL ends at {:#x}, before the end of the backup at {min_wal_end:#x}",
                rec.wal_end
            )));
        }
        let header = data.load_header()?;
        if header.format_version != c.format_version {
            return Err(Error::Corruption(format!(
                "data file format version {} differs from CONTROL version {}",
                header.format_version, c.format_version
            )));
        }
        let data = Arc::new(data);
        let accountant = MemoryAccountant::with_sink(memory_cap, sink.clone());
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
            clean: c.clean_shutdown,
        };
        let wal = Wal::new(
            location,
            storage.clone(),
            c.generation,
            rec.segments,
            durable_lsn.clone(),
            rec.counters,
            health.clone(),
            sink.clone(),
        );
        let core = Arc::new(Core {
            location: location.to_path_buf(),
            storage,
            health,
            durable_lsn,
            accountant,
            control: Mutex::new(control),
            pool,
            data,
            trees,
            wal,
            checkpoints: Checkpoints::new(c.redo_point, tuning),
            // TS-13 step 7: the old-version store starts empty.
            mvcc: Mvcc::default(),
            ids: IdCounters::new(Ids {
                next_element_id: rec.counters.next_element_id,
                next_catalog_id: rec.counters.next_catalog_id,
                next_token_id: rec.counters.next_token_id,
            }),
            catalog: Catalog::default(),
            sink,
            recovered,
            sorts: AtomicU64::new(0),
            backup_or_load: Mutex::new(()),
            #[cfg(test)]
            load_waits: AtomicU64::new(0),
            backup_or_install: RwLock::new(()),
            #[cfg(test)]
            install_waits: AtomicU64::new(0),
            #[cfg(test)]
            skip_reserve: crate::sync::atomic::AtomicBool::new(false),
            builds: crate::index::Builds::default(),
            _lock: lock,
        });
        // Step 7: the catalog and the statistics.
        catalog::open(&core, rec.counters.commit_ts)?;
        // Step 8.
        if rec.empty_log {
            core.wal.create_first(rec.wal_end)?;
        }
        // Build records a crash left (`storage::btree::bulk`), read before
        // the open checkpoint publishes what open read.
        let leftover = core.store().build_records()?;
        checkpoint::run_open(&core)?;
        core.wal.set_salt(core.control().sequence());
        let mut engine = Engine {
            core,
            threads: Vec::with_capacity(3),
            owner: true,
        };
        let core = engine.core.clone();
        engine.spawn("lpg-wal", move || {
            core.wal.run_writer(
                |spare| core.close_group(spare),
                || core.checkpoints.request_precreate(),
            );
        })?;
        let core = engine.core.clone();
        engine.spawn("lpg-checkpoint", move || checkpoint::run_thread(&core))?;
        let core = engine.core.clone();
        let seed = !c.clean_shutdown;
        core.mvcc.unseeded.store(seed, Ordering::SeqCst);
        engine.spawn("lpg-gc", move || {
            gc::run_thread(&core, tuning.gc_interval, seed);
        })?;
        // The pages of builds a crash interrupted, still owned by their
        // records, are released (`storage::btree::bulk`). ENOSPC or the
        // memory cap leaves a record in place, for a later release; the
        // database is consistent and stays usable (FR-REC-007).
        for (owner, _) in leftover {
            match engine.release_build(owner) {
                Err(e) if !(e.is_no_space() || matches!(e, Error::MemoryLimit { .. })) => {
                    return Err(e);
                }
                _ => {}
            }
        }
        // TS-18 step 7: every POPULATING index is built again.
        crate::index::restart_builds(&engine)?;
        Ok(engine)
    }

    /// A handle of the open database for a thread of its own (an index
    /// build, TS-18): it reads and commits like the owner's handle, and
    /// its drop stops nothing.
    pub(crate) fn handle(core: Arc<Core>) -> Engine {
        Engine {
            core,
            threads: Vec::new(),
            owner: false,
        }
    }

    /// The shared state, for a thread of its own ([`Engine::handle`]).
    pub(crate) fn core_arc(&self) -> Arc<Core> {
        self.core.clone()
    }

    fn spawn(&mut self, name: &str, f: impl FnOnce() + Send + 'static) -> Result<()> {
        let handle = crate::sync::thread::Builder::new()
            .name(name.to_owned())
            .spawn(f)?;
        self.threads.push(handle);
        Ok(())
    }

    /// Stops the garbage collector, the checkpointer and the WAL writer and
    /// waits for them. The writer first makes every joined group durable.
    fn stop_threads(&mut self) {
        self.core.builds.stop();
        self.core.mvcc.stop_gc();
        self.core.checkpoints.stop();
        self.core.wal.stop();
        for t in self.threads.drain(..) {
            // A panicked thread has nothing left to stop.
            let _ = t.join();
        }
    }

    /// Stops the garbage collector and the checkpointer only (clean close,
    /// before the last garbage-collection pass and the final checkpoint).
    fn stop_checkpointer(&mut self) {
        // A stopped build leaves its index POPULATING: the next open builds
        // it again (TS-18 step 7).
        self.core.builds.stop();
        self.core.mvcc.stop_gc();
        self.core.checkpoints.stop();
        while self.threads.len() > 1 {
            let _ = self.threads.pop().expect("thread handle").join();
        }
    }

    /// Closes the database cleanly (TS-11): stops the checkpointer, runs
    /// the last garbage-collection pass and the final checkpoint, whose
    /// `CONTROL` slot records a clean shutdown, stops the WAL writer and
    /// releases `LOCK`. No commit can be running: `close` takes the engine
    /// by value.
    ///
    /// The clean-shutdown flag also tells the next open that no garbage is
    /// left to find (`tx::gc` work queues). When the last pass fails with
    /// the database usable (ENOSPC or [`Error::MemoryLimit`], undone), or
    /// the seed scan of a crash-open never succeeded, the close still
    /// completes, with the flag clear: the next open recovers and scans for
    /// the garbage again. Shutdown does not depend on garbage collection in
    /// Memgraph (its storage destructor stops the collector without a last
    /// pass, R-MG @ `847e224` `src/storage/v2/inmemory/storage.cpp:545-563`)
    /// or PostgreSQL (the shutdown checkpoint runs no vacuum, R-PG @
    /// `1f22453` `src/backend/access/transam/xlog.c:7381-7420`).
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the database is unusable: then no checkpoint
    /// runs and the flag stays clear, so the next open recovers (TS-11);
    /// errors of the checkpoint, and of the wait for the last pass's WAL
    /// group. `LOCK` is released in every case.
    pub(crate) fn close(mut self) -> Result<()> {
        self.stop_checkpointer();
        self.core.health.check()?;
        let mut clean = !self.core.mvcc.unseeded.load(Ordering::SeqCst);
        // The builds have stopped: pages a failed release left in a build
        // record are released now. If that fails too, the shutdown is not
        // clean, so the next open releases them, and migration, which needs
        // a clean shutdown, never reads their unwritten pages
        // (`storage::btree::bulk`).
        match self.release_builds() {
            Ok(()) => {}
            Err(_) if !self.core.health.is_unusable() => clean = false,
            Err(e) => return Err(e),
        }
        // No transaction is left, so the last pass removes every queued
        // delete stamp and dropped tree; its group is made durable before
        // the final checkpoint (TS-14; `tx::gc`).
        match gc::pass(&self.core) {
            Ok(p) => {
                if let Some(seq) = p.group {
                    self.core.wal.wait_durable(seq)?;
                }
            }
            Err(_) if !self.core.health.is_unusable() => clean = false,
            Err(e) => return Err(e),
        }
        checkpoint::run(&self.core, clean)?;
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
        if self.owner {
            self.stop_threads();
        }
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
