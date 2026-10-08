//! `lab::backup`: online backup and restore (task #18).
//!
//! [`backup`] builds, under `$LPG_LAB_EXT4`, a database of
//! `LPG_BACKUP_GIB` (default 1 and 10) GiB of data pages: one tree
//! bulk-built from 400-byte values (task #17), then a WAL of one-page
//! commits. For each size it measures, to `$LPG_LAB_OUT/t18-lab-backup.md`:
//!
//! - a backup with no commit running, and a backup while 4 threads commit
//!   one-page batches: seconds, MiB/s of the copied bytes, the WAL held
//!   (`stop - start`, unverified point 2), the bytes the process read and
//!   wrote (`/proc/self/io`) against the database and WAL size;
//! - commit latency p99 during the backup and in an equal period before it;
//! - a restore of each artefact: seconds and MiB/s.
//!
//! `iostat -x` and `strace -c` run alongside (see the report).

use crate::sync::Arc;
use crate::sync::atomic::{AtomicBool, Ordering};
use std::fmt::Write as _;
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::{Probe, env_dir, env_list, header, percentile, write_text};
use crate::obs::names::transactions::COMMIT_LATENCY;
use crate::obs::recording::RecordingSink;
use crate::ops::restore::restore;
use crate::storage::FsStorage;
use crate::storage::btree::Write;
use crate::storage::btree::bulk::SortedEntries;
use crate::storage::engine::Engine;
use crate::storage::error::Result;
use crate::tx::commit::Ids;

const CAP: u64 = 1 << 30;

/// `n` entries with 8-byte keys `0..n` and 400-byte values, in order.
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

fn mib(b: u64) -> f64 {
    b as f64 / 1048576.0
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn backup() {
    let sizes = env_list("LPG_BACKUP_GIB", &[1, 10]);
    let base = env_dir("LPG_LAB_EXT4");
    let mut md = header(
        "lab::backup",
        &[
            ("data", "one bulk-built tree of 8-byte keys and 400-byte values, then 2 000 one-page commits".into()),
            ("cap", format!("{} MiB", CAP >> 20)),
            ("directory", base.display().to_string()),
        ],
    );
    md.push_str(
        "\n| GiB | load | backup s | backup MiB/s | WAL held B | read MiB | written MiB | db+WAL MiB | p99 µs before | p99 µs during | commits during | restore s | restore MiB/s |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for &gib in &sizes {
        let n = (gib << 30) / 440;
        let dir = tempfile::tempdir_in(&base).unwrap();
        let sink = Arc::new(RecordingSink::default());
        let e = Engine::create(
            dir.path(),
            Arc::new(FsStorage),
            CAP,
            crate::test_support::OWNER,
            sink.clone(),
        )
        .unwrap();
        let tree = e
            .bulk_build(&mut Counting {
                n,
                i: 0,
                k: [0; 8],
                v: [1; 400],
            })
            .unwrap();
        let one = |e: &Engine, rng: &mut StdRng| {
            let k = rng.random_range(0..n).to_be_bytes();
            e.commit(
                &[Write {
                    tree,
                    key: &k,
                    value: Some(&[2; 400]),
                    stamp: 0,
                }],
                Ids::default(),
            )
            .unwrap();
        };
        let mut rng = StdRng::seed_from_u64(gib);
        for _ in 0..2_000 {
            one(&e, &mut rng);
        }
        for load in ["idle", "4 committing threads"] {
            let target = tempfile::tempdir_in(&base).unwrap();
            let busy = load != "idle";
            let stop = AtomicBool::new(false);
            // Latency before: the same load for as long as the backup took
            // last time is not known yet, so one second.
            let lat_before = if busy {
                let s0 = sink.snapshot();
                let t = Instant::now();
                std::thread::scope(|s| {
                    for th in 0..4u64 {
                        let (e, one) = (&e, &one);
                        s.spawn(move || {
                            let mut rng = StdRng::seed_from_u64(th);
                            while t.elapsed().as_secs_f64() < 1.0 {
                                one(e, &mut rng);
                            }
                        });
                    }
                });
                let m = sink.snapshot().since(&s0);
                percentile(m.samples(COMMIT_LATENCY), 0.99) as f64 / 1e3
            } else {
                0.0
            };
            let s0 = sink.snapshot();
            let p0 = Probe::now();
            let t = Instant::now();
            let stats = std::thread::scope(|s| {
                if busy {
                    for th in 0..4u64 {
                        let (e, one, stop) = (&e, &one, &stop);
                        s.spawn(move || {
                            let mut rng = StdRng::seed_from_u64(100 + th);
                            while !stop.load(Ordering::Relaxed) {
                                one(e, &mut rng);
                            }
                        });
                    }
                }
                let st = e.backup(target.path()).unwrap();
                stop.store(true, Ordering::Relaxed);
                st
            });
            let secs = t.elapsed().as_secs_f64();
            let p1 = Probe::now();
            let m = sink.snapshot().since(&s0);
            let copied: u64 = stats.manifest.files.iter().map(|f| f.len).sum();
            let restored = tempfile::tempdir_in(&base).unwrap();
            let t = Instant::now();
            let r = restore(
                target.path(),
                restored.path(),
                Arc::new(FsStorage),
                CAP,
                Arc::new(RecordingSink::default()),
            )
            .unwrap();
            let rs = t.elapsed().as_secs_f64();
            drop(r);
            let line = format!(
                "| {gib} | {load} | {secs:.2} | {:.0} | {} | {:.0} | {:.0} | {:.0} | {lat_before:.0} | {:.0} | {} | {rs:.2} | {:.0} |",
                mib(copied) / secs,
                stats.wal_bytes,
                mib(p1.read_bytes - p0.read_bytes),
                mib(p1.write_bytes - p0.write_bytes),
                mib(e.data().reserved_pages() * 8192
                    + e.wal().segment_lsns().len() as u64 * (16 << 20)),
                percentile(m.samples(COMMIT_LATENCY), 0.99) as f64 / 1e3,
                m.samples(COMMIT_LATENCY).len(),
                mib(copied) / rs,
            );
            println!("{line}");
            writeln!(md, "{line}").unwrap();
        }
        e.close().unwrap();
    }
    let path = env_dir("LPG_LAB_OUT").join("t18-lab-backup.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}
