//! `lab::pool`: the buffer pool (task #13).
//!
//! All workloads read one database of [`DATA_PAGES`] tagged pages (1 GiB),
//! built once under `$LPG_LAB_EXT4/pool-db` and reused. Each operation is
//! one `fetch`, one shared latch, one byte read and the unpin.
//!
//! - [`pool`]: per cache share (100 %, 10 %, 1 % of the data): pure hits,
//!   misses, uniform and Zipf random fetches.
//! - [`pool_fixed`]: a fixed run for `perf stat`, `strace -c` and
//!   heaptrack. `LPG_POOL_SHARE` (100, 10, 1), `LPG_POOL_MODE` (`hit`,
//!   `miss`), `LPG_PERF_N` operations (0 gives the setup to subtract). It
//!   prints the counters, the accounted bytes and the VmRSS growth since
//!   before the open; one process per share keeps the RSS figures apart.
//! - [`pool_floor`]: unverified point 5, the pool floor at 0.5, 1, 2 and
//!   5 % of a cap sized for 10 % of the data, while another user holds all
//!   the memory it can reserve.
//! - [`pool_threads`]: fetch throughput at 1, 4, 8 and 16 threads, at 100 %
//!   and 10 % cache. `LPG_LATCH` names the latch build in the output file.
//! - [`pool_double_cache`]: OS-cache residency of `data-1` while the pool
//!   is full, and miss cost with the OS cache warm and evicted.

use crate::sync::Arc;
use crate::sync::atomic::AtomicU64;
use std::fmt::Write as _;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rand::rngs::StdRng;
use rand::seq::index;
use rand::{RngExt, SeedableRng};

use super::generator::{Sampler, ZIPF_ALPHA};
use super::{
    Measured, Probe, ROUNDS, env_dir, evict_from_os_cache, measure, median_range, write_table,
    write_text,
};
use crate::obs::MetricsSink;
use crate::obs::recording::RecordingSink;
use crate::storage::datafile::Datafile;
use crate::storage::engine::{CONTROL, Engine, data_name};
use crate::storage::error::Health;
use crate::storage::memory::MemoryAccountant;
use crate::storage::page::{self, HEADER_LEN, PAGE_BYTES, PAGE_SIZE, PageType};
use crate::storage::pool::{DEFAULT_FLOOR_PERMILLE, Pool, Sizing};
use crate::storage::{FsStorage, Storage};

/// Data pages: 1 GiB.
const DATA_PAGES: u64 = 131_072;

/// The shared database, built on first use.
fn database() -> PathBuf {
    let dir = env_dir("LPG_LAB_EXT4").join("pool-db");
    if FsStorage.open(&dir.join(CONTROL)).is_ok() {
        return dir;
    }
    // The storage abstraction has no directory creation; this is the lab's
    // own directory, like the files `lab` writes with rustix.
    match rustix::fs::mkdir(&dir, rustix::fs::Mode::from_raw_mode(0o755)) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(e) => panic!("mkdir {}: {e}", dir.display()),
    }
    let sink: Arc<dyn MetricsSink> = Arc::new(RecordingSink::default());
    let e = Engine::create(
        &dir,
        Arc::new(FsStorage),
        16 << 20,
        crate::test_support::OWNER,
        sink,
    )
    .expect("create");
    let data = e.data();
    data.ensure(DATA_PAGES).expect("ensure");
    let mut buf = vec![0u8; PAGE_SIZE];
    for _ in 1..DATA_PAGES {
        let p = data.allocate().expect("allocate");
        page::init(&mut buf, PageType::Leaf, p);
        buf[HEADER_LEN..HEADER_LEN + 8].copy_from_slice(&p.to_le_bytes());
        page::seal(&mut buf, p);
        data.write_page(p, &buf).expect("write");
    }
    data.sync().expect("sync");
    dir
}

/// A memory cap whose pool holds `pages` page buffers.
pub(crate) fn cap_for(pages: u64) -> u64 {
    let mut cap = pages * PAGE_BYTES;
    for _ in 0..4 {
        let overhead = Sizing::new(cap, DEFAULT_FLOOR_PERMILLE)
            .expect("sizing")
            .overhead_bytes();
        cap = pages * PAGE_BYTES + overhead;
    }
    cap
}

/// Pool pages for a cache share of `percent` % of the data.
fn share_pages(percent: u64) -> u64 {
    (DATA_PAGES * percent).div_ceil(100)
}

fn open(dir: &Path, cap: u64, sink: Arc<RecordingSink>) -> Engine {
    Engine::open(dir, Arc::new(FsStorage), cap, sink).expect("open")
}

/// One fetch: pin, shared latch, one byte read, unpin.
#[inline]
fn touch(pool: &Pool, p: u64) {
    let pinned = pool.fetch(p).expect("fetch");
    let g = pinned.read().expect("latch");
    black_box(g[HEADER_LEN]);
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn pool() {
    const HITS: u64 = 1_000_000;
    const RANDOM: u64 = 200_000;
    let dir = database();
    let mut rows: Vec<Measured> = Vec::new();
    for percent in [100, 10, 1] {
        let pool_pages = share_pages(percent);
        let cap = cap_for(pool_pages);
        let sink = Arc::new(RecordingSink::default());
        let e = open(&dir, cap, sink.clone());
        let pool = e.pool();
        let label = format!("{percent} %");
        // Misses: a cyclic scan over pages not in the pool. Each round
        // scans `n` pages; at 100 % the pool only grows.
        let miss_n = if percent == 100 {
            DATA_PAGES / 4
        } else {
            DATA_PAGES / 2
        };
        let mut next = 1u64;
        rows.push(measure(
            "miss (sequential first touches)",
            &label,
            miss_n,
            &sink,
            |_| {},
            |_| {
                for _ in 0..miss_n {
                    touch(pool, next);
                    next = next % (DATA_PAGES - 1) + 1;
                }
                pool.publish_metrics();
            },
        ));
        // Hits: random fetches within a hot set loaded beforehand.
        let hot = (pool_pages / 4).max(1);
        for p in 1..=hot {
            touch(pool, DATA_PAGES - p);
        }
        let mut rng = StdRng::seed_from_u64(1);
        rows.push(measure(
            "hit (random in a resident set)",
            &label,
            HITS,
            &sink,
            |_| {},
            |_| {
                for _ in 0..HITS {
                    touch(pool, DATA_PAGES - rng.random_range(1..=hot));
                }
                pool.publish_metrics();
            },
        ));
        for (name, mut sampler) in [
            ("uniform random", Sampler::uniform(DATA_PAGES - 1, 2)),
            (
                "Zipf 0.8 random",
                Sampler::zipf(DATA_PAGES - 1, ZIPF_ALPHA, 3),
            ),
        ] {
            rows.push(measure(
                name,
                &label,
                RANDOM,
                &sink,
                |_| {},
                |_| {
                    for _ in 0..RANDOM {
                        touch(pool, sampler.node() + 1);
                    }
                    pool.publish_metrics();
                },
            ));
        }
    }
    write_table(
        "t13-lab-pool.md",
        "lab::pool",
        &[
            ("directory", env_dir("LPG_LAB_EXT4").display().to_string()),
            ("data", format!("{DATA_PAGES} pages (1 GiB), OS cache warm")),
            (
                "operation",
                "fetch + shared latch + 1 byte read + unpin".to_owned(),
            ),
            (
                "cache share",
                "pool page buffers as % of the data pages".to_owned(),
            ),
        ],
        &rows,
    );
}

#[test]
#[ignore = "laboratory run for perf, strace and heaptrack; see the module documentation"]
fn pool_fixed() {
    let percent: u64 = std::env::var("LPG_POOL_SHARE").map_or(10, |v| v.parse().unwrap());
    let mode = std::env::var("LPG_POOL_MODE").expect("set LPG_POOL_MODE");
    let n: u64 = std::env::var("LPG_PERF_N").map_or(100_000, |v| v.parse().unwrap());
    let dir = database();
    let pool_pages = share_pages(percent);
    let sink = Arc::new(RecordingSink::default());
    let rss0 = Probe::now().rss_kib;
    let e = open(&dir, cap_for(pool_pages), sink.clone());
    let pool = e.pool();
    match mode.as_str() {
        "hit" => {
            let hot = (pool_pages / 4).max(1);
            for p in 1..=hot {
                touch(pool, p);
            }
            let mut rng = StdRng::seed_from_u64(1);
            let picks: Vec<u64> = (0..n).map(|_| rng.random_range(1..=hot)).collect();
            for p in picks {
                touch(pool, p);
            }
        }
        "miss" => {
            // Fill the pool (at 100 %: half the data), then touch pages
            // never touched before.
            let fill = if percent == 100 {
                DATA_PAGES / 2
            } else {
                pool_pages
            };
            for p in 1..=fill {
                touch(pool, p);
            }
            assert!(fill + n < DATA_PAGES, "LPG_PERF_N too large");
            for p in fill + 1..=fill + n {
                touch(pool, p);
            }
        }
        other => panic!("unknown LPG_POOL_MODE {other}"),
    }
    pool.publish_metrics();
    let m = sink.snapshot();
    let rss1 = Probe::now().rss_kib;
    println!(
        "LAB share={percent} mode={mode} n={n} hits={} misses={} evictions={} buffers={} \
         accounted={} rss_before_open_kib={rss0} rss_end_kib={rss1} rss_growth_bytes={}",
        m.counter(crate::obs::names::cache::HITS),
        m.counter(crate::obs::names::cache::PAGE_FAULTS),
        m.counter(crate::obs::names::cache::EVICTIONS),
        pool.buffer_frames(),
        e.accountant().used(),
        (rss1 - rss0) * 1024
    );
}

/// A pool on the shared database with floor `permille`, outside an
/// [`Engine`], so the floor can vary.
fn pool_with_floor(dir: &Path, cap: u64, permille: u64, sink: Arc<RecordingSink>) -> Pool {
    let health = Arc::new(Health::default());
    let file = FsStorage.open(&dir.join(data_name(1))).expect("open data");
    let (data, _) = Datafile::open(file, health.clone(), sink.clone()).expect("datafile");
    Pool::new(
        Arc::new(data),
        MemoryAccountant::new(cap),
        health,
        Arc::new(AtomicU64::new(0)),
        sink,
        permille,
    )
    .expect("pool")
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn pool_floor() {
    const OPS: u64 = 200_000;
    let dir = database();
    let cap = cap_for(share_pages(10));
    let mut rows = Vec::new();
    let mut md = String::new();
    for (permille, competing) in [(10, false), (5, true), (10, true), (20, true), (50, true)] {
        let sink = Arc::new(RecordingSink::default());
        let pool = pool_with_floor(&dir, cap, permille, sink.clone());
        // Fill the pool.
        let mut fill = Sampler::uniform(DATA_PAGES - 1, 4);
        for _ in 0..2 * share_pages(10) {
            touch(&pool, fill.node() + 1);
        }
        // The competing user takes all the memory it can.
        let mut held = Vec::new();
        if competing {
            while let Ok(r) = pool.reserve(1 << 20) {
                held.push(r);
            }
            while let Ok(r) = pool.reserve(PAGE_BYTES) {
                held.push(r);
            }
        }
        let label = if competing {
            format!("floor {:.1} %, competing", permille as f64 / 10.0)
        } else {
            format!("floor {:.1} %, alone", permille as f64 / 10.0)
        };
        for (name, mut sampler) in [
            ("uniform random", Sampler::uniform(DATA_PAGES - 1, 5)),
            (
                "Zipf 0.8 random",
                Sampler::zipf(DATA_PAGES - 1, ZIPF_ALPHA, 6),
            ),
        ] {
            let before = sink.snapshot();
            let m = measure(
                name,
                &label,
                OPS,
                &sink,
                |_| {},
                |_| {
                    for _ in 0..OPS {
                        touch(&pool, sampler.node() + 1);
                    }
                    pool.publish_metrics();
                },
            );
            let d = sink.snapshot().since(&before);
            let hits = d.counter(crate::obs::names::cache::HITS);
            let misses = d.counter(crate::obs::names::cache::PAGE_FAULTS);
            writeln!(
                md,
                "| {label} | {name} | {} | {} | {:.4} |",
                pool.buffer_frames(),
                held.iter().map(|r| r.bytes()).sum::<u64>(),
                hits as f64 / (hits + misses) as f64
            )
            .unwrap();
            rows.push(m);
        }
    }
    write_table(
        "t13-lab-pool-floor.md",
        "lab::pool floor (unverified point 5)",
        &[
            (
                "cap",
                format!("{cap} B, a pool of 10 % of the data when alone"),
            ),
            ("data", format!("{DATA_PAGES} pages (1 GiB), OS cache warm")),
        ],
        &rows,
    );
    let out = format!(
        "# lab::pool floor hit rates\n\n| case | workload | page buffers | held by the competing user B | hit rate |\n|---|---|---|---|---|\n{md}"
    );
    write_text(
        &env_dir("LPG_LAB_OUT").join("t13-lab-pool-floor-hits.md"),
        &out,
    );
    println!("{out}");
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn pool_threads() {
    const OPS_PER_THREAD: u64 = 400_000;
    let latch = std::env::var("LPG_LATCH").unwrap_or_else(|_| "std".into());
    let dir = database();
    let mut md = format!(
        "# lab::pool threads, latch build `{latch}`\n\n- {OPS_PER_THREAD} fetches per thread, \
         uniform random pages; {ROUNDS} rounds\n\n| cache | threads | Mfetch/s median [min, max] | \
         ns per fetch per thread median |\n|---|---|---|---|\n"
    );
    for percent in [100, 10] {
        let sink = Arc::new(RecordingSink::default());
        let e = open(&dir, cap_for(share_pages(percent)), sink);
        let pool = e.pool();
        // Warm: at 100 % every page becomes resident.
        for p in 1..DATA_PAGES {
            touch(pool, p);
        }
        for threads in [1u64, 4, 8, 16] {
            let mut rates = Vec::new();
            let mut per_op = Vec::new();
            for round in 0..ROUNDS as u64 {
                let picks: Vec<Vec<u64>> = (0..threads)
                    .map(|t| {
                        let mut rng = StdRng::seed_from_u64(round * 100 + t);
                        (0..OPS_PER_THREAD)
                            .map(|_| rng.random_range(1..DATA_PAGES))
                            .collect()
                    })
                    .collect();
                let t = Instant::now();
                std::thread::scope(|s| {
                    for p in &picks {
                        s.spawn(move || {
                            for &page in p {
                                touch(pool, page);
                            }
                        });
                    }
                });
                let ns = t.elapsed().as_nanos() as f64;
                rates.push((threads * OPS_PER_THREAD) as f64 / ns * 1000.0);
                per_op.push(ns / OPS_PER_THREAD as f64);
            }
            let fmt3 = |v: &[f64]| {
                let mut s = v.to_vec();
                s.sort_by(f64::total_cmp);
                format!("{:.2} [{:.2}, {:.2}]", s[1], s[0], s[2])
            };
            writeln!(
                md,
                "| {percent} % | {threads} | {} | {} |",
                fmt3(&rates),
                median_range(&per_op)
            )
            .unwrap();
        }
    }
    let path = env_dir("LPG_LAB_OUT").join(format!("t13-lab-pool-threads-{latch}.md"));
    write_text(&path, &md);
    println!("{md}");
}

/// Resident bytes of `path` in the OS cache, from `fincore`.
fn fincore(path: &Path) -> String {
    let out = Command::new("fincore")
        .args(["--bytes", "--noheadings", "--output", "RES,SIZE"])
        .arg(path)
        .output()
        .expect("run fincore");
    String::from_utf8(out.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn pool_double_cache() {
    const MISSES: u64 = 10_000;
    let dir = database();
    let data_path = dir.join(data_name(1));
    let pool_pages = share_pages(10);
    let sink = Arc::new(RecordingSink::default());
    let e = open(&dir, cap_for(pool_pages), sink.clone());
    let pool = e.pool();
    let mut md = String::from("# lab::pool double caching\n\n");
    evict_from_os_cache(&data_path);
    writeln!(
        md,
        "- fincore after fadvise(DONTNEED): `{}`",
        fincore(&data_path)
    )
    .unwrap();
    // Fill the pool with uniform random pages.
    let mut fill = Sampler::uniform(DATA_PAGES - 1, 7);
    for _ in 0..2 * pool_pages {
        touch(pool, fill.node() + 1);
    }
    pool.publish_metrics();
    writeln!(
        md,
        "- pool: {} page buffers ({} B); fincore with the pool full: `{}` (RES SIZE, bytes)",
        pool.buffer_frames(),
        pool.buffer_frames() * PAGE_BYTES,
        fincore(&data_path)
    )
    .unwrap();
    // Miss cost: distinct pages per round, not in the pool.
    let mut rng = StdRng::seed_from_u64(8);
    let mut sets: Vec<Vec<usize>> = (0..2 * ROUNDS)
        .map(|_| index::sample(&mut rng, DATA_PAGES as usize - 1, MISSES as usize).into_vec())
        .collect();
    let mut rows = Vec::new();
    for (cold, name) in [
        (false, "miss, OS cache warm"),
        (true, "miss, OS cache evicted"),
    ] {
        let base = if cold { ROUNDS } else { 0 };
        rows.push(measure(
            name,
            "10 %",
            MISSES,
            &sink,
            |r| {
                if cold {
                    evict_from_os_cache(&data_path);
                } else {
                    // Make every page resident in the OS cache, not the pool.
                    let file = FsStorage.open(&data_path).expect("open");
                    let mut chunk = vec![0u8; 1 << 20];
                    for off in (0..DATA_PAGES * PAGE_BYTES).step_by(1 << 20) {
                        file.read_at(&mut chunk, off).expect("read");
                    }
                }
                let _ = r;
            },
            |r| {
                for &p in &std::mem::take(&mut sets[base + r]) {
                    touch(pool, p as u64 + 1);
                }
                pool.publish_metrics();
            },
        ));
    }
    writeln!(md, "- fincore at the end: `{}`", fincore(&data_path)).unwrap();
    write_text(
        &env_dir("LPG_LAB_OUT").join("t13-lab-pool-double-cache-notes.md"),
        &md,
    );
    println!("{md}");
    write_table(
        "t13-lab-pool-double-cache.md",
        "lab::pool miss cost, OS cache warm and evicted",
        &[
            ("data", format!("{DATA_PAGES} pages (1 GiB)")),
            ("pool", "10 % of the data, full".to_owned()),
            (
                "misses",
                format!("{MISSES} distinct random pages per round"),
            ),
        ],
        &rows,
    );
}
