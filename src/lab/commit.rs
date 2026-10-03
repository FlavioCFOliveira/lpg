//! `lab::commit`: the commit path and group commit (task #15).
//!
//! Every workload runs in each lab directory that is set ([`lab_dirs`]), on
//! a new database in a temporary directory there, removed afterwards. The
//! database holds one tree of [`KEYS`] entries with [`VALUE`]-byte values,
//! inserted in key order (about 17 entries per leaf), and the memory cap
//! [`CAP`] keeps every page in the pool, so a commit reads nothing from the
//! device. A commit replaces values with values of the same length, so no
//! page splits: replacing `n` keys [`STRIDE`] keys apart changes exactly
//! `n` leaves. The periodic checkpoints are switched off (interval 1 h, no
//! commit trigger); an untimed checkpoint before each round writes the
//! dirty pages and recycles the WAL.
//!
//! - [`commit`], 3 rounds per configuration, median [min, max] of the
//!   per-round figures, to `$LPG_LAB_OUT/t15-lab-commit.md`:
//!   - `LPG_COMMIT_THREADS` (default 1, 4, 16) threads, each committing
//!     `LPG_COMMIT_N` (default 1 000) one-page batches at random keys;
//!   - one thread committing batches that change `LPG_COMMIT_PAGES`
//!     (default 1, 10, 100) pages.
//!
//!   Per configuration: commits/s; commit latency p50 and p99 from the
//!   `transaction.commit_latency` histogram, which the engine records from
//!   the call of `commit` to its return; commits per WAL group; WAL frames,
//!   bytes and `sync_data` calls (`log.flushes`) per commit; bytes the
//!   process wrote to storage per commit (`/proc/self/io`); segments
//!   created.
//! - [`commit_fixed`]: one configuration for `strace -c -f`, `perf record`
//!   and `iostat -x`, in `LPG_LAB_FS` (`ext4`, the default, or `xfs`):
//!   `LPG_COMMIT_THREADS` threads (default 16), `LPG_COMMIT_PAGES` pages
//!   per batch (default 1), `LPG_PERF_N` commits per thread (default
//!   1 000; 0 gives the setup to subtract). It prints the counters and the Unix
//!   time in milliseconds at the start and end of the measured phase, to
//!   select the matching `iostat` samples.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::{
    Probe, ROUNDS, env_dir, env_list, env_or, header, lab_dirs, median_range, percentile,
    write_text,
};
use crate::obs::names::transaction_log::{
    APPEND_BATCH_SIZE, APPENDED_BYTES, FLUSHES, FRAMES, GROUPS, SEGMENTS_CREATED,
};
use crate::obs::names::transactions::{COMMIT_LATENCY, COMMITTED};
use crate::obs::recording::{Metrics, RecordingSink};
use crate::storage::FsStorage;
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::{Engine, Tuning};
use crate::tx::commit::Ids;

/// Entries of the tree.
pub(crate) const KEYS: u64 = 100_000;
/// Bytes of each value.
pub(crate) const VALUE: usize = 400;
/// Keys between two keys of one batch: more than the entries of a leaf.
pub(crate) const STRIDE: u64 = 64;
/// Memory cap: the whole tree and the page copies of a batch fit.
const CAP: u64 = 256 << 20;
/// Keys per commit while the tree is built.
const BUILD_BATCH: u64 = 1_000;

/// The key of entry `i`.
pub(crate) fn key(i: u64) -> [u8; 8] {
    i.to_be_bytes()
}

/// No periodic checkpoint during a measurement.
pub(crate) fn no_trigger() -> Tuning {
    Tuning {
        checkpoint_interval: Duration::from_secs(3600),
        checkpoint_commits: u64::MAX,
    }
}

/// The Unix time in milliseconds.
pub(crate) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_millis()
}

/// A database with one tree of [`KEYS`] entries, in a temporary directory.
pub(crate) struct Db {
    pub(crate) engine: Engine,
    pub(crate) tree: Tree,
    pub(crate) sink: Arc<RecordingSink>,
    pub(crate) dir: tempfile::TempDir,
}

/// Creates a tree in `engine` and fills it with `keys` entries in key
/// order, [`BUILD_BATCH`] per commit; then runs a checkpoint.
pub(crate) fn populate(engine: &Engine, keys: u64) -> Tree {
    let tree = engine.create_trees(1).expect("tree")[0];
    let value = [0u8; VALUE];
    for start in (0..keys).step_by(BUILD_BATCH as usize) {
        let ks: Vec<[u8; 8]> = (start..(start + BUILD_BATCH).min(keys)).map(key).collect();
        let batch: Vec<Write<'_>> = ks
            .iter()
            .map(|k| Write {
                tree,
                key: k,
                value: Some(&value),
            })
            .collect();
        engine.commit(&batch, Ids::default()).expect("build commit");
    }
    engine.checkpoint().expect("checkpoint");
    tree
}

/// Creates a database in a temporary directory under `base` with `cap` and
/// `tuning`, and [`populate`]s it with `keys` entries.
pub(crate) fn build(base: &Path, keys: u64, cap: u64, tuning: Tuning) -> Db {
    let dir = tempfile::tempdir_in(base).expect("work directory");
    let sink = Arc::new(RecordingSink::default());
    let engine = Engine::create_tuned(dir.path(), Arc::new(FsStorage), cap, sink.clone(), tuning)
        .expect("create");
    let tree = populate(&engine, keys);
    Db {
        engine,
        tree,
        sink,
        dir,
    }
}

/// Commits one batch replacing the values of `pages` keys [`STRIDE`] apart
/// from a random start below `keys`, with `fill` bytes.
pub(crate) fn commit_pages(
    e: &Engine,
    tree: Tree,
    keys: u64,
    pages: u64,
    fill: u8,
    rng: &mut StdRng,
) {
    let span = (pages - 1) * STRIDE + 1;
    let first = rng.random_range(0..keys - span + 1);
    let ks: Vec<[u8; 8]> = (0..pages).map(|j| key(first + j * STRIDE)).collect();
    let value = [fill; VALUE];
    let batch: Vec<Write<'_>> = ks
        .iter()
        .map(|k| Write {
            tree,
            key: k,
            value: Some(&value),
        })
        .collect();
    e.commit(&batch, Ids::default()).expect("commit");
}

/// What one round measured.
struct Round {
    seconds: f64,
    write_bytes: u64,
    sink: Metrics,
}

/// `threads` threads each commit `n` batches of `pages` pages.
fn round(db: &Db, threads: u64, pages: u64, n: u64, seed: u64) -> Round {
    let before = db.sink.snapshot();
    let p0 = Probe::now();
    let t = Instant::now();
    std::thread::scope(|s| {
        for th in 0..threads {
            let (e, tree) = (&db.engine, db.tree);
            s.spawn(move || {
                let mut rng = StdRng::seed_from_u64(seed * 1_000 + th);
                for i in 0..n {
                    commit_pages(e, tree, KEYS, pages, (th + i) as u8, &mut rng);
                }
            });
        }
    });
    let seconds = t.elapsed().as_secs_f64();
    let p1 = Probe::now();
    Round {
        seconds,
        write_bytes: p1.write_bytes - p0.write_bytes,
        sink: db.sink.snapshot().since(&before),
    }
}

/// The per-round figures of one configuration, as table cells.
fn row(threads: u64, pages: u64, n: u64, rounds: &[Round]) -> String {
    let f = |g: &dyn Fn(&Round) -> f64| -> String {
        median_range(&rounds.iter().map(g).collect::<Vec<_>>())
    };
    let f2 = |g: &dyn Fn(&Round) -> f64| -> String {
        let mut v: Vec<f64> = rounds.iter().map(g).collect();
        v.sort_by(f64::total_cmp);
        format!("{:.2} [{:.2}, {:.2}]", v[v.len() / 2], v[0], v[v.len() - 1])
    };
    let commits = |r: &Round| r.sink.counter(COMMITTED) as f64;
    let per_commit = |r: &Round, name: &str| r.sink.counter(name) as f64 / commits(r);
    let lat = |r: &Round, q: f64| percentile(r.sink.samples(COMMIT_LATENCY), q) as f64 / 1e3;
    format!(
        "| {threads} | {pages} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        threads * n,
        f(&|r| commits(r) / r.seconds),
        f(&|r| lat(r, 0.5)),
        f(&|r| lat(r, 0.99)),
        f2(&|r| commits(r) / r.sink.counter(GROUPS) as f64),
        rounds
            .iter()
            .map(|r| r
                .sink
                .samples(APPEND_BATCH_SIZE)
                .iter()
                .max()
                .copied()
                .unwrap_or(0))
            .max()
            .unwrap_or(0),
        f2(&|r| per_commit(r, FRAMES)),
        f(&|r| per_commit(r, APPENDED_BYTES)),
        f2(&|r| per_commit(r, FLUSHES)),
        f(&|r| r.write_bytes as f64 / commits(r)),
        rounds
            .iter()
            .map(|r| r.sink.counter(SEGMENTS_CREATED))
            .sum::<u64>(),
    )
}

/// Runs `ROUNDS` rounds of one configuration, each after an untimed
/// checkpoint.
fn configuration(db: &Db, threads: u64, pages: u64, n: u64) -> String {
    let rounds: Vec<Round> = (0..ROUNDS as u64)
        .map(|r| {
            db.engine.checkpoint().expect("checkpoint");
            round(db, threads, pages, n, r)
        })
        .collect();
    let line = row(threads, pages, n, &rounds);
    println!("{line}");
    line
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn commit() {
    let threads = env_list("LPG_COMMIT_THREADS", &[1, 4, 16]);
    let pages = env_list("LPG_COMMIT_PAGES", &[1, 10, 100]);
    let n: u64 = env_or("LPG_COMMIT_N", 1_000);
    let mut md = header(
        "lab::commit",
        &[
            (
                "data",
                format!(
                    "one tree, {KEYS} entries of 8-byte keys and {VALUE}-byte values in key order; cap {} MiB, every page in the pool",
                    CAP >> 20
                ),
            ),
            (
                "commit",
                format!(
                    "replaces the values of `pages` keys {STRIDE} apart (one leaf each), same length; random start"
                ),
            ),
            (
                "rounds",
                format!(
                    "{ROUNDS}, each after an untimed checkpoint; periodic checkpoints off; median [min, max] of the per-round figures"
                ),
            ),
            (
                "latency",
                "transaction.commit_latency histogram: from the call of commit to its return"
                    .to_owned(),
            ),
            (
                "written B/commit",
                "write_bytes of /proc/self/io (WAL, segment creation, no data pages)".to_owned(),
            ),
        ],
    );
    for (fs, base) in lab_dirs() {
        let db = build(&base, KEYS, CAP, no_trigger());
        writeln!(
            md,
            "\n## {fs} ({})\n\n\
             | threads | pages/commit | commits/round | commits/s | latency p50 µs | latency p99 µs | commits/group | max commits/group | frames/commit | WAL B/commit | sync_data/commit | written B/commit | segments created |\n\
             |---|---|---|---|---|---|---|---|---|---|---|---|---|",
            base.display()
        )
        .unwrap();
        for &t in &threads {
            md.push_str(&configuration(&db, t, 1, n));
            md.push('\n');
        }
        for &p in &pages {
            md.push_str(&configuration(&db, 1, p, n));
            md.push('\n');
        }
        db.engine.close().expect("close");
    }
    let path = env_dir("LPG_LAB_OUT").join("t15-lab-commit.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}

#[test]
#[ignore = "laboratory run for strace, perf and iostat; see the module documentation"]
fn commit_fixed() {
    let fs: String = env_or("LPG_LAB_FS", "ext4".to_owned());
    let base = match fs.as_str() {
        "ext4" => env_dir("LPG_LAB_EXT4"),
        "xfs" => env_dir("LPG_LAB_XFS"),
        other => panic!("unknown LPG_LAB_FS {other}"),
    };
    let threads: u64 = env_or("LPG_COMMIT_THREADS", 16);
    let pages: u64 = env_or("LPG_COMMIT_PAGES", 1);
    let n: u64 = env_or("LPG_PERF_N", 1_000);
    let db = build(&base, KEYS, CAP, no_trigger());
    let start = unix_ms();
    let r = round(&db, threads, pages, n, 0);
    let end = unix_ms();
    let m = &r.sink;
    let commits = m.counter(COMMITTED);
    let lat = m.samples(COMMIT_LATENCY);
    println!(
        "LAB fs={fs} threads={threads} pages={pages} n={n} start_ms={start} end_ms={end} \
         seconds={:.3} commits={commits} commits_per_s={:.0} p50_us={:.1} p99_us={:.1} \
         groups={} frames={} wal_bytes={} wal_sync_data={} segments_created={} write_bytes={}",
        r.seconds,
        commits as f64 / r.seconds,
        percentile(lat, 0.5) as f64 / 1e3,
        percentile(lat, 0.99) as f64 / 1e3,
        m.counter(GROUPS),
        m.counter(FRAMES),
        m.counter(APPENDED_BYTES),
        m.counter(FLUSHES),
        m.counter(SEGMENTS_CREATED),
        r.write_bytes,
    );
    // Drop without close: no final checkpoint in the traced run.
    drop(db.engine);
    drop(db.dir);
}
