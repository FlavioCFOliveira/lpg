//! `lab::files`: create and open of a database location (task #12).
//!
//! - [`files`]: wall time of 100 creates and 100 opens in `LPG_LAB_EXT4`
//!   and `LPG_LAB_XFS`, each call timed alone (one `Instant` pair costs
//!   2.79 µs on this host, under 1 % of one call).
//! - [`files_one`]: one create (`LPG_FILES_MODE=create`), or one create then
//!   one open (`create_open`), or nothing (`none`), in `LPG_TEST_DIR`, for
//!   `strace -c -f`; subtracting runs isolates each call.
//! - [`files_growth`]: grows the data file by `LPG_GROW_GIB` GiB (default 4)
//!   one extent per `ensure`, in `LPG_TEST_DIR`, for
//!   `strace -T -e trace=fallocate`.

use crate::sync::Arc;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use super::{env_dir, median_range, write_text};
use crate::obs::MetricsSink;
use crate::obs::recording::RecordingSink;
use crate::storage::engine::Engine;
use crate::storage::page::PAGE_BYTES;
use crate::storage::{FsStorage, Storage};

/// Memory cap of every run: 64 MiB.
const CAP: u64 = 64 << 20;
const RUNS: usize = 100;

fn storage() -> Arc<dyn Storage> {
    Arc::new(FsStorage)
}

fn sink() -> Arc<dyn MetricsSink> {
    Arc::new(RecordingSink::default())
}

fn run(dir: &Path, fs_name: &str, md: &mut String) {
    let mut create_ns = Vec::with_capacity(RUNS);
    let mut open_ns = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let work = tempfile::tempdir_in(dir).expect("work directory");
        let t = Instant::now();
        let e = Engine::create(
            work.path(),
            storage(),
            CAP,
            crate::test_support::OWNER,
            sink(),
        )
        .expect("create");
        create_ns.push(t.elapsed().as_nanos() as f64);
        drop(e);
        let t = Instant::now();
        let e = Engine::open(work.path(), storage(), CAP, sink()).expect("open");
        open_ns.push(t.elapsed().as_nanos() as f64);
        drop(e);
    }
    let us = |v: &[f64]| median_range(&v.iter().map(|ns| ns / 1000.0).collect::<Vec<_>>());
    let p90 = |v: &[f64]| {
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        s[s.len() * 9 / 10] / 1000.0
    };
    writeln!(
        md,
        "| {fs_name} | {} | {:.0} | {} | {:.0} |",
        us(&create_ns),
        p90(&create_ns),
        us(&open_ns),
        p90(&open_ns)
    )
    .unwrap();
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn files() {
    let mut md = format!(
        "# lab::files\n\n- memory cap: {CAP} B\n- runs: {RUNS} creates and {RUNS} opens per \
         directory, each in a new directory, each call timed alone\n- profile: lab\n\n\
         | directory | create µs median [min, max] | create µs p90 | open µs median [min, max] | open µs p90 |\n\
         |---|---|---|---|---|\n"
    );
    run(&env_dir("LPG_LAB_EXT4"), "ext4", &mut md);
    run(&env_dir("LPG_LAB_XFS"), "xfs", &mut md);
    let path = env_dir("LPG_LAB_OUT").join("t12-lab-files.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}

#[test]
#[ignore = "laboratory run for strace; see the module documentation"]
fn files_one() {
    let mode = std::env::var("LPG_FILES_MODE").expect("set LPG_FILES_MODE");
    let work = tempfile::tempdir_in(env_dir("LPG_TEST_DIR")).expect("work directory");
    match mode.as_str() {
        "none" => {}
        "create" => drop(
            Engine::create(
                work.path(),
                storage(),
                CAP,
                crate::test_support::OWNER,
                sink(),
            )
            .unwrap(),
        ),
        "create_open" => {
            drop(
                Engine::create(
                    work.path(),
                    storage(),
                    CAP,
                    crate::test_support::OWNER,
                    sink(),
                )
                .unwrap(),
            );
            drop(Engine::open(work.path(), storage(), CAP, sink()).unwrap());
        }
        other => panic!("unknown LPG_FILES_MODE {other}"),
    }
}

#[test]
#[ignore = "laboratory run for strace; see the module documentation"]
fn files_growth() {
    let gib: u64 = std::env::var("LPG_GROW_GIB").map_or(4, |v| v.parse().expect("number"));
    let work = tempfile::tempdir_in(env_dir("LPG_TEST_DIR")).expect("work directory");
    let e = Engine::create(
        work.path(),
        storage(),
        CAP,
        crate::test_support::OWNER,
        sink(),
    )
    .unwrap();
    let data = e.data();
    let target = (gib << 30) / PAGE_BYTES;
    let mut extents = 0;
    while data.reserved_pages() < target {
        // One page more than is reserved: exactly one new extent.
        data.ensure(data.unused_pages() + 1).unwrap();
        extents += 1;
    }
    println!(
        "grew to {} pages ({} GiB) with {extents} extents after the first",
        data.reserved_pages(),
        (data.reserved_pages() * PAGE_BYTES) >> 30
    );
}
