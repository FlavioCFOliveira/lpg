//! `lab::migrate`: format migration N-1 to N (task #19).
//!
//! [`migrate`] builds, under `$LPG_LAB_EXT4`, a database of
//! `LPG_MIGRATE_GIB` (default 1 and 10) GiB of data pages (one bulk-built
//! tree, task #17), closes it, rewrites it in the test-only format 0
//! (`ops::migrate::tests`), and migrates it to format 1. It reports, to
//! `$LPG_LAB_OUT/t19-lab-migrate.md`: seconds and MiB/s of the pages
//! converted, the bytes the migration required against what
//! `available_space` reported, the peak bytes of the location's files,
//! sampled every 10 ms by a second thread, and the bytes the process read
//! and wrote (`/proc/self/io`). `strace -c` runs alongside (see the
//! report).

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{Probe, env_dir, env_list, header, write_text};
use crate::obs::recording::RecordingSink;
use crate::ops::migrate::migrate_with;
use crate::ops::migrate::tests::{TEST_FORMATS, downgrade_to_format_0};
use crate::storage::btree::bulk::SortedEntries;
use crate::storage::engine::Engine;
use crate::storage::error::Result;
use crate::storage::{FsStorage, Storage};

const CAP: u64 = 1 << 30;

struct Counting {
    n: u64,
    i: u64,
    k: [u8; 8],
    v: [u8; 400],
}

impl SortedEntries for Counting {
    fn next_entry(&mut self) -> Result<Option<(&[u8], &[u8])>> {
        if self.i == self.n {
            return Ok(None);
        }
        self.k = self.i.to_be_bytes();
        self.i += 1;
        Ok(Some((&self.k, &self.v)))
    }
}

/// Bytes of the files of `dir`.
fn bytes(dir: &Path) -> u64 {
    FsStorage
        .list(dir)
        .unwrap_or_default()
        .iter()
        .filter_map(|n| FsStorage.open(&dir.join(n)).ok()?.len().ok())
        .sum()
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn migrate() {
    let sizes = env_list("LPG_MIGRATE_GIB", &[1, 10]);
    let base = env_dir("LPG_LAB_EXT4");
    let mut md = header(
        "lab::migrate",
        &[
            ("data", "one bulk-built tree of 8-byte keys and 400-byte values, closed cleanly, rewritten in format 0".into()),
            ("directory", base.display().to_string()),
        ],
    );
    md.push_str(
        "\n| GiB | pages | seconds | MiB/s | required MiB | available MiB | before MiB | peak MiB | after MiB | read MiB | written MiB |\n\
         |---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for &gib in &sizes {
        let dir = tempfile::tempdir_in(&base).unwrap();
        let e = Engine::create(
            dir.path(),
            Arc::new(FsStorage),
            CAP,
            Arc::new(RecordingSink::default()),
        )
        .unwrap();
        e.bulk_build(&mut Counting {
            n: (gib << 30) / 440,
            i: 0,
            k: [0; 8],
            v: [3; 400],
        })
        .unwrap();
        e.close().unwrap();
        downgrade_to_format_0(dir.path());
        let before = bytes(dir.path());
        let peak = AtomicU64::new(before);
        let done = AtomicBool::new(false);
        let p0 = Probe::now();
        let t = Instant::now();
        let m = std::thread::scope(|s| {
            s.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    peak.fetch_max(bytes(dir.path()), Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            let m = migrate_with(dir.path(), &FsStorage, &TEST_FORMATS)
                .unwrap()
                .unwrap();
            done.store(true, Ordering::Relaxed);
            m
        });
        let secs = t.elapsed().as_secs_f64();
        let p1 = Probe::now();
        let mib = |b: u64| b as f64 / 1048576.0;
        let line = format!(
            "| {gib} | {} | {secs:.2} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} |",
            m.pages,
            mib(m.pages * 8192) / secs,
            mib(m.required),
            mib(m.available),
            mib(before),
            mib(peak.load(Ordering::Relaxed)),
            mib(bytes(dir.path())),
            mib(p1.read_bytes - p0.read_bytes),
            mib(p1.write_bytes - p0.write_bytes),
        );
        println!("{line}");
        writeln!(md, "{line}").unwrap();
        Engine::open(
            dir.path(),
            Arc::new(FsStorage),
            CAP,
            Arc::new(RecordingSink::default()),
        )
        .unwrap()
        .close()
        .unwrap();
    }
    let path = env_dir("LPG_LAB_OUT").join("t19-lab-migrate.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}
