//! Files of import and export through the storage trait (TS-03,
//! `interfaces.md:93`; TS-26 Export files).
//!
//! - [`Reader`] and [`Writer`]: `io::Read` and `io::Write` over a
//!   [`StorageFile`], each with a fixed buffer outside the memory cap, as
//!   the 1 MiB copy buffer is (TS-26 Memory, TS-08).
//! - [`Outputs`]: the files of one export or one report. Each is written
//!   as `<name>.tmp`; [`Outputs::commit`] syncs each, renames it to
//!   `<name>` and syncs the location, as the backup's `MANIFEST` is written
//!   (TS-26; `pg_basebackup`'s durable rename, R-PG
//!   `src/bin/pg_basebackup/pg_basebackup.c:2298-2321`, studied only).
//!   Creation fails if a target exists; a failure, or dropping the outputs
//!   before the commit, removes every file they wrote (UC-OPS-004 A1).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use super::invalid_argument;
use crate::error::Error;
use crate::storage::{Storage, StorageFile};

/// Bytes of the buffer of a [`Reader`] or a [`Writer`].
pub(crate) const BUFFER: usize = 64 << 10;

/// The suffix of an output file before its commit.
const TMP: &str = ".tmp";

/// Fails with 22N11 unless every name of `names` is a plain file name: not
/// empty, without `/` or NUL, not `.` or `..`, not ending with `.tmp`.
///
/// # Errors
///
/// 22N11.
pub(crate) fn check_names(names: &[&str]) -> Result<(), Error> {
    for (i, n) in names.iter().enumerate() {
        if n.is_empty() || n.contains(['/', '\0']) || *n == "." || *n == ".." || n.ends_with(TMP) {
            return Err(invalid_argument("a file name that is not a plain name"));
        }
        if names[..i].contains(n) {
            return Err(invalid_argument("a file name given twice"));
        }
    }
    Ok(())
}

/// `io::Read` over a storage file, from offset 0.
pub(crate) struct Reader {
    file: Box<dyn StorageFile>,
    at: u64,
}

impl Reader {
    /// Opens `location/name` through `storage`, buffered.
    ///
    /// # Errors
    ///
    /// 50N00 for an I/O failure, 22N11 for a missing file.
    pub(crate) fn open(
        storage: &dyn Storage,
        location: &Path,
        name: &str,
    ) -> Result<io::BufReader<Reader>, Error> {
        match storage.open(&location.join(name)) {
            Ok(file) => Ok(io::BufReader::with_capacity(BUFFER, Reader { file, at: 0 })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(invalid_argument("an input file does not exist"))
            }
            Err(e) => Err(io_error(e)),
        }
    }
}

impl io::Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read_at(buf, self.at)?;
        self.at += n as u64;
        Ok(n)
    }
}

/// `io::Write` over a storage file, from offset 0, through a fixed buffer.
pub(crate) struct Writer {
    file: Box<dyn StorageFile>,
    at: u64,
    buf: Vec<u8>,
}

impl Writer {
    fn new(file: Box<dyn StorageFile>) -> Writer {
        Writer {
            file,
            at: 0,
            buf: Vec::with_capacity(BUFFER),
        }
    }

    /// Writes the buffer and syncs the file.
    fn finish(&mut self) -> io::Result<()> {
        io::Write::flush(self)?;
        self.file.sync_data()
    }
}

impl io::Write for Writer {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buf.len() + data.len() > BUFFER {
            self.flush()?;
        }
        if data.len() >= BUFFER {
            self.file.write_at(data, self.at)?;
            self.at += data.len() as u64;
        } else {
            self.buf.extend_from_slice(data);
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.file.write_at(&self.buf, self.at)?;
            self.at += self.buf.len() as u64;
            self.buf.clear();
        }
        Ok(())
    }
}

/// 50N00 for an I/O failure (TS-26: storage full or any other I/O
/// failure).
pub(crate) fn io_error(e: io::Error) -> Error {
    Error::from(crate::storage::error::Error::Io(e))
}

/// The output files of one export or report (module documentation).
pub(crate) struct Outputs<'s> {
    storage: &'s dyn Storage,
    location: PathBuf,
    /// Each file's name, temporary path and writer, in creation order.
    files: Vec<(String, PathBuf, Writer)>,
    /// Names already renamed by a commit in progress.
    renamed: Vec<String>,
    committed: bool,
}

impl<'s> Outputs<'s> {
    /// Checks that no file of `names` exists in `location`, before any is
    /// created: the export then writes nothing if one does.
    ///
    /// # Errors
    ///
    /// 22N11 if a target exists or the location cannot be listed.
    pub(crate) fn new(
        storage: &'s dyn Storage,
        location: &Path,
        names: &[&str],
    ) -> Result<Outputs<'s>, Error> {
        let present: Vec<OsString> = storage
            .list(location)
            .map_err(|_| invalid_argument("the output location cannot be listed"))?;
        if names.iter().any(|n| present.iter().any(|p| p == *n)) {
            return Err(invalid_argument("an output file exists"));
        }
        Ok(Outputs {
            storage,
            location: location.to_path_buf(),
            files: Vec::new(),
            renamed: Vec::new(),
            committed: false,
        })
    }

    /// Creates the temporary file of `name` and returns its index for
    /// [`Outputs::writer`]. Each export has temporary names of its own,
    /// `<name>.<16 random hexadecimal digits>.tmp`, created exclusively: a
    /// concurrent export of the same name writes another file, and a file
    /// left by a crash never blocks a later export (rmp #57 FINDINGs 457,
    /// 473). The `tempfile` crate builds its names the same way, a random
    /// suffix created with `O_EXCL` and retried on a collision (3.27.0
    /// `src/util.rs` `create_helper`, `NUM_RETRIES`), and commits with a
    /// no-replace persist (`persist_noclobber`: `renameat2` with
    /// `RENAME_NOREPLACE`, else `link` and `unlink`, `src/file/imp/unix.rs:111-127`),
    /// as [`Outputs::commit`] does.
    ///
    /// # Errors
    ///
    /// 50N00.
    pub(crate) fn create(&mut self, name: &str) -> Result<usize, Error> {
        let mut tries = 0;
        let (path, file) = loop {
            let path = self
                .location
                .join(format!("{name}.{:016x}{TMP}", rand::random::<u64>()));
            match self.storage.create(&path) {
                Ok(f) => break (path, f),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && tries < 8 => tries += 1,
                Err(e) => return Err(io_error(e)),
            }
        };
        self.files.push((name.to_owned(), path, Writer::new(file)));
        Ok(self.files.len() - 1)
    }

    /// The writer of file `i`.
    pub(crate) fn writer(&mut self, i: usize) -> &mut Writer {
        &mut self.files[i].2
    }

    /// Syncs every file, renames each to its name without replacing an
    /// existing file ([`Storage::rename_noreplace`]) and syncs the
    /// location (module documentation). On failure every file this export
    /// wrote is removed.
    ///
    /// # Errors
    ///
    /// 22N11 if a target was created during the export; 50N00.
    pub(crate) fn commit(mut self) -> Result<(), Error> {
        for (_, _, w) in &mut self.files {
            w.finish().map_err(io_error)?;
        }
        for (name, tmp, _) in &self.files {
            match self
                .storage
                .rename_noreplace(tmp, &self.location.join(name))
            {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(invalid_argument("an output file exists"));
                }
                Err(e) => return Err(io_error(e)),
            }
            self.renamed.push(name.clone());
        }
        self.storage.sync_dir(&self.location).map_err(io_error)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Outputs<'_> {
    /// Removes every file of an export that did not commit: the renamed
    /// targets and the temporary files (UC-OPS-004 A1). A removal that
    /// fails leaves a target the next export of that name refuses, or a
    /// temporary file that blocks nothing.
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (name, tmp, _) in &self.files {
            let path = if self.renamed.contains(name) {
                self.location.join(name)
            } else {
                tmp.clone()
            };
            let _ = self.storage.remove(&path);
        }
        if !self.files.is_empty() {
            let _ = self.storage.sync_dir(&self.location);
        }
    }
}
