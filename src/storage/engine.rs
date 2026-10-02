//! Create and open of a database location (TS-05, TS-13 steps 1–3, TS-27,
//! TS-28).
//!
//! An [`Engine`] is one open database: it holds `LOCK` for its whole life,
//! the `CONTROL` file, the data file, the buffer pool, the memory
//! accountant and the unusable latch. The memory cap is a required argument
//! of both [`Engine::create`] and [`Engine::open`]; there is no default
//! (FR-LIM-003).

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use super::control::{Control, ControlSlots};
use super::datafile::Datafile;
use super::error::{Error, Health, Result};
use super::memory::MemoryAccountant;
use super::page::PAGE_SIZE;
use super::pool::{DEFAULT_FLOOR_PERMILLE, Pool, Sizing};
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

/// One open database.
pub(crate) struct Engine {
    health: Arc<Health>,
    durable_lsn: Arc<AtomicU64>,
    accountant: Arc<MemoryAccountant>,
    control: Mutex<ControlSlots>,
    pool: Pool,
    data: Arc<Datafile>,
    /// Declared last, so it is released after every file is closed.
    _lock: Box<dyn StorageLock>,
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
        Engine::assemble(memory_cap, sink, health, control, data, lock)
    }

    /// Opens the database at `location` (TS-13 steps 1–3, TS-28).
    /// `memory_cap` bounds the memory the database accounts for
    /// (FR-LIM-003).
    ///
    /// Steps: fail if `CONTROL` is absent, creating nothing; take `LOCK`,
    /// failing if it is absent and creating nothing, since recreating it
    /// would let a second handle open the database while the first still
    /// holds the lock on the deleted file; read both `CONTROL` slots and check
    /// the format version; read page 0 of `data-<g>` and check it holds the
    /// same version; remove the data files and WAL segments of every other
    /// generation. A missing `data-<g>` is corruption and is never
    /// recreated. WAL replay and the remaining recovery steps follow in
    /// task #16.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] if `memory_cap` is too small for the
    /// buffer pool; [`Error::NoDatabase`] (FR-DB-004); [`Error::Locked`]
    /// (FR-DB-006, FR-DB-007); [`Error::LockMissing`] if `CONTROL` exists
    /// and `LOCK` does not, with no file created (TS-27);
    /// [`Error::Corruption`] if both `CONTROL` slots
    /// are invalid, `data-<g>` is missing, page 0 fails its checksum, or the
    /// versions differ (FR-REC-003); [`Error::MigrationRequired`] for an older format and
    /// [`Error::NewerFormat`] for a newer one, both before anything is
    /// written (TS-28); [`Error::Io`].
    pub(crate) fn open(
        location: &Path,
        storage: Arc<dyn Storage>,
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
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
        let (data, header) = Datafile::open(data_file, health.clone(), sink.clone())?;
        if header.format_version != c.format_version {
            return Err(Error::Corruption(format!(
                "data file format version {} differs from CONTROL version {}",
                header.format_version, c.format_version
            )));
        }
        let mut removed = false;
        for name in s.list(location)? {
            if file_set_generation(&name).is_some_and(|g| g != c.generation) {
                s.remove(&location.join(name))?;
                removed = true;
            }
        }
        if removed {
            s.sync_dir(location)?;
        }
        Engine::assemble(memory_cap, sink, health, control, data, lock)
    }

    fn assemble(
        memory_cap: u64,
        sink: Arc<dyn MetricsSink>,
        health: Arc<Health>,
        control: ControlSlots,
        data: Datafile,
        lock: Box<dyn StorageLock>,
    ) -> Result<Engine> {
        let data = Arc::new(data);
        let accountant = MemoryAccountant::new(memory_cap);
        // Every page on disk is durable up to the redo point; the WAL
        // writer (task #15) and recovery (task #16) advance it.
        let durable_lsn = Arc::new(AtomicU64::new(control.current().redo_point));
        let pool = Pool::new(
            data.clone(),
            accountant.clone(),
            health.clone(),
            durable_lsn.clone(),
            sink,
            DEFAULT_FLOOR_PERMILLE,
        )?;
        Ok(Engine {
            health,
            durable_lsn,
            accountant,
            control: Mutex::new(control),
            pool,
            data,
            _lock: lock,
        })
    }

    /// Fails with [`Error::Unusable`] once any component has set the latch.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`].
    pub(crate) fn check(&self) -> Result<()> {
        self.health.check()
    }

    /// The buffer pool.
    pub(crate) fn pool(&self) -> &Pool {
        &self.pool
    }

    /// The data file.
    pub(crate) fn data(&self) -> &Datafile {
        &self.data
    }

    /// The memory accountant.
    pub(crate) fn accountant(&self) -> &MemoryAccountant {
        &self.accountant
    }

    /// The durable LSN: every WAL record up to it is on stable storage.
    pub(crate) fn durable_lsn(&self) -> &AtomicU64 {
        &self.durable_lsn
    }

    /// The `CONTROL` file.
    pub(crate) fn control(&self) -> &Mutex<ControlSlots> {
        &self.control
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
