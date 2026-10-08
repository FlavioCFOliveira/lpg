//! `lab::recovery`: checkpoints and recovery at open (task #16).
//!
//! Every workload runs in each lab directory that is set ([`lab_dirs`]), on
//! new databases in temporary directories there, removed afterwards. The
//! trees and commits are those of [`super::commit`]: 8-byte keys and
//! 400-byte values in key order, and batches that replace values with
//! values of the same length, one leaf per key.
//!
//! - [`checkpoint_cost`]: checkpoint time as a function of dirty pages. For
//!   each `LPG_CHECKPOINT_PAGES` (default 1 024, 8 192, 65 536) leaves, a
//!   tree of about that many leaves in a pool that holds it; per round, one
//!   key in every 8 is replaced (every leaf holds more than 8 entries, so
//!   every leaf becomes dirty), then one checkpoint is timed. Pages written
//!   and duration come from the checkpoint's own figures; bytes from
//!   `/proc/self/io`. To `$LPG_LAB_OUT/t16-lab-checkpoint.md`.
//! - [`checkpoint_trigger`]: WAL bytes between two checkpoints at the
//!   100 000-commit trigger (TS-11; `LPG_TRIGGER_COMMITS` changes the
//!   count for a short check), with one-page commits from
//!   `LPG_TRIGGER_THREADS` (default 1, 16) threads; the time trigger is set
//!   to 24 h so that only the commit count triggers. Bytes are the
//!   difference of the redo points of the two `CONTROL` slots and the
//!   `log.appended_bytes` counter. To `$LPG_LAB_OUT/t16-lab-trigger.md`.
//! - [`recovery_cost`]: recovery time as a function of WAL bytes replayed.
//!   For each `LPG_RECOVERY_MIB` (default 16, 64, 256), per round: 100-page
//!   commits until that much WAL lies past the redo point, a drop without
//!   close (TS-02: a crash for recovery), the database files evicted from
//!   the OS cache (cold) or not (warm, `LPG_LAB_COLD=0`), then a timed open.
//!   Open time includes the open checkpoint (TS-13 step 8), whose duration
//!   is reported apart. To `$LPG_LAB_OUT/t16-lab-recovery.md`.
//! - [`checkpoint_fixed`]: one checkpoint of `LPG_CHECKPOINT_PAGES` (default
//!   65 536) dirty leaves in `LPG_LAB_FS` (`ext4` or `xfs`), for `iostat -x`;
//!   prints the Unix time in milliseconds around the checkpoint.
//! - [`recovery_fixed`]: for `strace -c -f` and `perf record` of an open
//!   that recovers. `LPG_RECOVERY_STEP=prepare` creates a database in
//!   `LPG_TEST_DIR` (an empty directory), commits `LPG_RECOVERY_MIB`
//!   (default 256) MiB of WAL and drops it without close;
//!   `LPG_RECOVERY_STEP=open` evicts the files from the OS cache unless
//!   `LPG_LAB_COLD=0`, opens the database, prints what recovery did and
//!   drops it. The open step can run once per prepare.

use crate::sync::Arc;
use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;

use super::commit::{
    Db, KEYS, STRIDE, VALUE, build, commit_pages, key, no_trigger, populate, unix_ms,
};
use super::{
    Probe, ROUNDS, env_dir, env_list, env_or, evict_from_os_cache, header, lab_dirs, median_range,
    write_text,
};
use crate::obs::names::checkpointing::{DURATION, EVENTS};
use crate::obs::names::transaction_log::{APPENDED_BYTES, GROUPS};
use crate::obs::names::transactions::COMMITTED;
use crate::obs::recording::RecordingSink;
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::{Engine, Tuning};
use crate::storage::page::PAGE_BYTES;
use crate::storage::{FsStorage, Storage};
use crate::tx::commit::Ids;

/// Entries per leaf of a tree built in key order: (8 192 − header) × 0.9
/// full / about 420 bytes per entry, rounded down.
const ENTRIES_PER_LEAF: u64 = 17;
/// Keys between two replaced keys when every leaf must change.
const DIRTY_STRIDE: u64 = 8;
/// Keys per commit while pages are made dirty.
const DIRTY_BATCH: u64 = 1_000;
/// Memory cap of the trigger and recovery runs.
const CAP: u64 = 256 << 20;

fn fs_dir(fs: &str) -> std::path::PathBuf {
    match fs {
        "ext4" => env_dir("LPG_LAB_EXT4"),
        "xfs" => env_dir("LPG_LAB_XFS"),
        other => panic!("unknown LPG_LAB_FS {other}"),
    }
}

/// A cap whose pool holds `leaves` leaves and their inner pages, plus the
/// page copies of one commit and room for the rest.
fn cap_for_leaves(leaves: u64) -> u64 {
    (leaves + leaves / 50) * PAGE_BYTES + (64 << 20)
}

/// Replaces one key in every [`DIRTY_STRIDE`] of `keys`, [`DIRTY_BATCH`]
/// keys per commit: every leaf becomes dirty.
fn dirty_every_leaf(e: &Engine, tree: Tree, keys: u64, fill: u8) {
    let value = [fill; VALUE];
    let all: Vec<u64> = (0..keys).step_by(DIRTY_STRIDE as usize).collect();
    for chunk in all.chunks(DIRTY_BATCH as usize) {
        let ks: Vec<[u8; 8]> = chunk.iter().map(|&i| key(i)).collect();
        let batch: Vec<Write<'_>> = ks
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(&value),
                stamp: 0,
            })
            .collect();
        e.commit(&batch, Ids::default()).expect("commit");
    }
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn checkpoint_cost() {
    let sizes = env_list("LPG_CHECKPOINT_PAGES", &[1_024, 8_192, 65_536]);
    let mut md = header(
        "lab::recovery checkpoint cost",
        &[
            (
                "data",
                format!(
                    "one tree of about {ENTRIES_PER_LEAF} × leaves entries, 8-byte keys, {}-byte values, key order; the pool holds every page",
                    VALUE
                ),
            ),
            (
                "dirty",
                format!(
                    "one key in {DIRTY_STRIDE} replaced, {DIRTY_BATCH} per commit, before each round; every leaf dirty"
                ),
            ),
            (
                "timed",
                "one checkpoint (Engine::checkpoint): TS-11 steps 1-5".to_owned(),
            ),
            ("rounds", format!("{ROUNDS}; median [min, max]")),
        ],
    );
    for (fs, base) in lab_dirs() {
        writeln!(
            md,
            "\n## {fs} ({})\n\n\
             | target leaves | pages written | duration ms | pages/s | MiB/s | written KiB (/proc/self/io) | segments recycled + removed |\n\
             |---|---|---|---|---|---|---|",
            base.display()
        )
        .unwrap();
        for &leaves in &sizes {
            let keys = leaves * ENTRIES_PER_LEAF;
            let db = build(&base, keys, cap_for_leaves(leaves), no_trigger());
            let mut pages = Vec::new();
            let mut ms = Vec::new();
            let mut pps = Vec::new();
            let mut mibps = Vec::new();
            let mut kib = Vec::new();
            let mut recycled = 0;
            for r in 0..ROUNDS {
                dirty_every_leaf(&db.engine, db.tree, keys, r as u8 + 1);
                let p0 = Probe::now();
                let st = db.engine.checkpoint().expect("checkpoint");
                let p1 = Probe::now();
                let secs = st.duration.as_secs_f64();
                pages.push(st.pages_written as f64);
                ms.push(secs * 1e3);
                pps.push(st.pages_written as f64 / secs);
                mibps.push((st.pages_written * PAGE_BYTES) as f64 / secs / f64::from(1 << 20));
                kib.push((p1.write_bytes - p0.write_bytes) as f64 / 1024.0);
                recycled += st.recycled + st.removed;
            }
            let line = format!(
                "| {leaves} | {} | {} | {} | {} | {} | {recycled} |",
                median_range(&pages),
                median_range(&ms),
                median_range(&pps),
                median_range(&mibps),
                median_range(&kib),
            );
            println!("{line}");
            md.push_str(&line);
            md.push('\n');
            db.engine.close().expect("close");
        }
    }
    let path = env_dir("LPG_LAB_OUT").join("t16-lab-checkpoint.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}

/// The redo point of the authoritative `CONTROL` slot.
fn redo_point(e: &Engine) -> u64 {
    e.control().lock().expect("CONTROL").current().redo_point
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn checkpoint_trigger() {
    let commits: u64 = env_or("LPG_TRIGGER_COMMITS", 100_000);
    let threads = env_list("LPG_TRIGGER_THREADS", &[1, 16]);
    let tuning = Tuning {
        checkpoint_interval: Duration::from_secs(24 * 3600),
        checkpoint_commits: commits,
        ..Tuning::default()
    };
    let mut md = header(
        "lab::recovery WAL between checkpoints at the commit trigger",
        &[
            (
                "data",
                format!(
                    "one tree, {KEYS} entries, 8-byte keys, {}-byte values; cap {} MiB",
                    VALUE,
                    CAP >> 20
                ),
            ),
            (
                "commits",
                format!(
                    "{commits} one-page commits (one value replaced at a random key) after a checkpoint; trigger {commits} commits, time trigger 24 h"
                ),
            ),
            ("runs", "one per thread count".to_owned()),
        ],
    );
    for (fs, base) in lab_dirs() {
        writeln!(
            md,
            "\n## {fs} ({})\n\n\
             | threads | seconds | commits/group | WAL B (redo points) | WAL B (log.appended_bytes) | WAL B/commit | triggered checkpoint ms |\n\
             |---|---|---|---|---|---|---|",
            base.display()
        )
        .unwrap();
        for &t in &threads {
            let db = build(&base, KEYS, CAP, tuning);
            let redo0 = redo_point(&db.engine);
            let before = db.sink.snapshot();
            let started = Instant::now();
            std::thread::scope(|s| {
                for th in 0..t {
                    let (e, tree) = (&db.engine, db.tree);
                    let n = commits / t + u64::from(th < commits % t);
                    s.spawn(move || {
                        let mut rng = StdRng::seed_from_u64(th);
                        for i in 0..n {
                            commit_pages(e, tree, KEYS, 1, i as u8, &mut rng);
                        }
                    });
                }
            });
            let seconds = started.elapsed().as_secs_f64();
            let deadline = Instant::now() + Duration::from_secs(600);
            while db.sink.snapshot().since(&before).counter(EVENTS) == 0 {
                assert!(Instant::now() < deadline, "no checkpoint was triggered");
                std::thread::sleep(Duration::from_millis(10));
            }
            let m = db.sink.snapshot().since(&before);
            assert_eq!(m.counter(COMMITTED), commits);
            let redo1 = redo_point(&db.engine);
            let appended = m.counter(APPENDED_BYTES);
            let line = format!(
                "| {t} | {seconds:.1} | {:.2} | {} | {appended} | {:.0} | {:.1} |",
                commits as f64 / m.counter(GROUPS) as f64,
                redo1 - redo0,
                appended as f64 / commits as f64,
                m.samples(DURATION).first().copied().unwrap_or(0) as f64 / 1e6,
            );
            println!("{line}");
            md.push_str(&line);
            md.push('\n');
            db.engine.close().expect("close");
        }
    }
    let path = env_dir("LPG_LAB_OUT").join("t16-lab-trigger.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}

/// Commits 100-page batches until `mib` MiB of WAL lie past the redo point.
fn fill_wal(e: &Engine, tree: Tree, mib: u64, seed: u64) {
    let mut rng = StdRng::seed_from_u64(seed);
    let target = redo_point(e) + (mib << 20);
    let mut i = 0u8;
    while e.wal().durable_lsn() < target {
        commit_pages(e, tree, KEYS, 100, i, &mut rng);
        i = i.wrapping_add(1);
    }
}

/// Evicts every file of the database at `dir` from the OS cache.
fn evict_database(dir: &Path) {
    for name in FsStorage.list(dir).expect("list") {
        evict_from_os_cache(&dir.join(name));
    }
}

/// What one open measured.
struct Opened {
    engine: Engine,
    seconds: f64,
    checkpoint_ms: f64,
    read_bytes: u64,
}

/// Opens the database at `dir`, timed.
fn timed_open(dir: &Path, cold: bool) -> Opened {
    if cold {
        evict_database(dir);
    }
    let sink = Arc::new(RecordingSink::default());
    let p0 = Probe::now();
    let t = Instant::now();
    let engine = Engine::open_tuned(dir, Arc::new(FsStorage), CAP, sink.clone(), no_trigger())
        .expect("open");
    let seconds = t.elapsed().as_secs_f64();
    let p1 = Probe::now();
    let checkpoint_ms = sink
        .snapshot()
        .samples(DURATION)
        .first()
        .copied()
        .unwrap_or(0) as f64
        / 1e6;
    Opened {
        engine,
        seconds,
        checkpoint_ms,
        read_bytes: p1.read_bytes - p0.read_bytes,
    }
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn recovery_cost() {
    let sizes = env_list("LPG_RECOVERY_MIB", &[16, 64, 256]);
    let cold = env_or("LPG_LAB_COLD", 1u8) != 0;
    let mut md = header(
        "lab::recovery recovery cost",
        &[
            (
                "data",
                format!(
                    "one tree, {KEYS} entries, 8-byte keys, {}-byte values; cap {} MiB; periodic checkpoints off",
                    VALUE,
                    CAP >> 20
                ),
            ),
            (
                "WAL",
                format!(
                    "100-page commits (keys {STRIDE} apart) until the WAL past the redo point reaches the target, then a drop without close"
                ),
            ),
            (
                "open",
                format!(
                    "Engine::open, {}; includes the open checkpoint (TS-13 step 8), reported apart",
                    if cold {
                        "database files evicted from the OS cache first"
                    } else {
                        "OS cache warm"
                    }
                ),
            ),
            (
                "replay MiB/s",
                "WAL bytes replayed / (open time - open checkpoint time)".to_owned(),
            ),
            ("rounds", format!("{ROUNDS}; median [min, max]")),
        ],
    );
    for (fs, base) in lab_dirs() {
        writeln!(
            md,
            "\n## {fs} ({})\n\n\
             | target MiB | WAL B replayed | groups | pages | open ms | open checkpoint ms | replay MiB/s | read KiB (/proc/self/io) |\n\
             |---|---|---|---|---|---|---|---|",
            base.display()
        )
        .unwrap();
        for &mib in &sizes {
            let Db {
                mut engine,
                tree,
                dir,
                ..
            } = build(&base, KEYS, CAP, no_trigger());
            let (mut bytes, mut groups, mut pages) = (Vec::new(), Vec::new(), Vec::new());
            let (mut open_ms, mut cp_ms, mut rate, mut read) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for r in 0..ROUNDS {
                fill_wal(&engine, tree, mib, r as u64);
                drop(engine);
                let o = timed_open(dir.path(), cold);
                let rec = o.engine.recovered();
                bytes.push(rec.bytes as f64);
                groups.push(rec.groups as f64);
                pages.push(rec.pages as f64);
                open_ms.push(o.seconds * 1e3);
                cp_ms.push(o.checkpoint_ms);
                rate.push(
                    rec.bytes as f64 / f64::from(1 << 20) / (o.seconds - o.checkpoint_ms / 1e3),
                );
                read.push(o.read_bytes as f64 / 1024.0);
                engine = o.engine;
            }
            let line = format!(
                "| {mib} | {} | {} | {} | {} | {} | {} | {} |",
                median_range(&bytes),
                median_range(&groups),
                median_range(&pages),
                median_range(&open_ms),
                median_range(&cp_ms),
                median_range(&rate),
                median_range(&read),
            );
            println!("{line}");
            md.push_str(&line);
            md.push('\n');
            engine.close().expect("close");
            drop(dir);
        }
    }
    let path = env_dir("LPG_LAB_OUT").join("t16-lab-recovery.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}

#[test]
#[ignore = "laboratory run for iostat; see the module documentation"]
fn checkpoint_fixed() {
    let fs: String = env_or("LPG_LAB_FS", "ext4".to_owned());
    let leaves: u64 = env_or("LPG_CHECKPOINT_PAGES", 65_536);
    let keys = leaves * ENTRIES_PER_LEAF;
    let db = build(&fs_dir(&fs), keys, cap_for_leaves(leaves), no_trigger());
    dirty_every_leaf(&db.engine, db.tree, keys, 1);
    let start = unix_ms();
    let st = db.engine.checkpoint().expect("checkpoint");
    let end = unix_ms();
    println!(
        "LAB fs={fs} leaves={leaves} start_ms={start} end_ms={end} pages_written={} duration_ms={:.1} \
         recycled={} removed={}",
        st.pages_written,
        st.duration.as_secs_f64() * 1e3,
        st.recycled,
        st.removed,
    );
    db.engine.close().expect("close");
}

#[test]
#[ignore = "laboratory run for strace and perf; see the module documentation"]
fn recovery_fixed() {
    let dir = env_dir("LPG_TEST_DIR");
    let step: String = env_or("LPG_RECOVERY_STEP", String::new());
    let mib: u64 = env_or("LPG_RECOVERY_MIB", 256);
    match step.as_str() {
        "prepare" => {
            let sink = Arc::new(RecordingSink::default());
            let e = Engine::create_tuned(&dir, Arc::new(FsStorage), CAP, sink, no_trigger())
                .expect("create (LPG_TEST_DIR must hold no database)");
            let tree = populate(&e, KEYS);
            fill_wal(&e, tree, mib, 0);
            println!(
                "LAB prepared {} with {mib} MiB of WAL past the redo point",
                dir.display()
            );
            drop(e);
        }
        "open" => {
            let cold = env_or("LPG_LAB_COLD", 1u8) != 0;
            let o = timed_open(&dir, cold);
            let rec = o.engine.recovered();
            println!(
                "LAB cold={cold} open_ms={:.1} open_checkpoint_ms={:.1} wal_bytes={} groups={} pages={} read_bytes={}",
                o.seconds * 1e3,
                o.checkpoint_ms,
                rec.bytes,
                rec.groups,
                rec.pages,
                o.read_bytes,
            );
            drop(o.engine);
        }
        other => panic!("set LPG_RECOVERY_STEP to prepare or open, not {other:?}"),
    }
}
