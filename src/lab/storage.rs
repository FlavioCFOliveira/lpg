//! `lab::storage`: 8 KiB positional I/O through [`FsStorage`] as the kernel
//! will use it (`&dyn Storage`, `Box<dyn StorageFile>`), in `LPG_LAB_EXT4`
//! and `LPG_LAB_XFS`.
//!
//! - Warm random reads: a 1 GiB data file resident in the OS cache, pages
//!   drawn uniformly.
//! - Cold random reads: the file is evicted from the OS cache before each
//!   round, and each round reads distinct pages, so every read misses.
//! - Write + sync: sequential 8 KiB `write_at`, each followed by
//!   `sync_data`, into a new file reserved beforehand (as fio preallocates
//!   its file in the task #7 ceiling job).

use std::cell::RefCell;
use std::hint::black_box;
use std::path::Path;

use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::index;

use super::generator::{PAGE_SIZE, Sampler};
use super::{Measured, ROUNDS, env_dir, evict_from_os_cache, measure, write_table};
use crate::obs::recording::RecordingSink;
use crate::storage::{FsStorage, Storage, StorageFile};

/// Data file pages: 1 GiB.
const DATA_PAGES: u64 = 131_072;
const WARM_READS: u64 = 200_000;
const COLD_READS: u64 = 20_000;
const SYNCED_WRITES: u64 = 1_000;

const PAGE: usize = PAGE_SIZE as usize;

fn run(dir: &Path, fs_name: &str) {
    let storage: &dyn Storage = &FsStorage;
    let sink = RecordingSink::default();
    let work = tempfile::tempdir_in(dir).expect("create work directory");
    let data_path = work.path().join("data");
    let data = storage.create(&data_path).expect("create data file");
    data.reserve(0, DATA_PAGES * PAGE_SIZE).expect("reserve");
    let mut chunk = vec![0u8; 128 * PAGE];
    for (i, page) in chunk.chunks_mut(PAGE).enumerate() {
        page[..8].copy_from_slice(&(i as u64).to_le_bytes());
    }
    for c in 0..DATA_PAGES / 128 {
        data.write_at(&chunk, c * 128 * PAGE_SIZE).expect("write");
    }
    data.sync_data().expect("sync");
    let mut buf = vec![0u8; PAGE];
    let read = |file: &dyn StorageFile, buf: &mut [u8], page: u64| {
        let n = file
            .read_at(black_box(buf), page * PAGE_SIZE)
            .expect("read");
        assert_eq!(n, PAGE);
    };

    let mut rows: Vec<Measured> = Vec::new();

    // Warm: one untimed pass makes every page resident.
    for p in 0..DATA_PAGES {
        read(&*data, &mut buf, p);
    }
    let mut pages = Sampler::uniform(DATA_PAGES, 11);
    rows.push(measure(
        "read_at 8 KiB random",
        "warm (OS cache)",
        WARM_READS,
        &sink,
        |_| {},
        |_| {
            for _ in 0..WARM_READS {
                read(&*data, &mut buf, pages.node());
            }
        },
    ));

    // Cold: distinct pages per round, file evicted before the round.
    let mut rng = StdRng::seed_from_u64(12);
    let mut cold: Vec<Vec<usize>> = (0..ROUNDS)
        .map(|_| index::sample(&mut rng, DATA_PAGES as usize, COLD_READS as usize).into_vec())
        .collect();
    rows.push(measure(
        "read_at 8 KiB random",
        "cold (evicted, distinct pages)",
        COLD_READS,
        &sink,
        |_| evict_from_os_cache(&data_path),
        |r| {
            for &p in &std::mem::take(&mut cold[r]) {
                read(&*data, &mut buf, p as u64);
            }
        },
    ));

    // Write + sync_data into a new reserved file per round.
    let wal_path = work.path().join("wal");
    let wal: RefCell<Option<Box<dyn StorageFile>>> = RefCell::new(None);
    let page = vec![0x5Au8; PAGE];
    rows.push(measure(
        "write_at 8 KiB + sync_data",
        "n/a (sequential, new reserved file)",
        SYNCED_WRITES,
        &sink,
        |_| {
            if wal.borrow_mut().take().is_some() {
                storage.remove(&wal_path).expect("remove");
            }
            let f = storage.create(&wal_path).expect("create");
            f.reserve(0, SYNCED_WRITES * PAGE_SIZE).expect("reserve");
            f.sync_data().expect("sync");
            storage.sync_dir(work.path()).expect("sync dir");
            *wal.borrow_mut() = Some(f);
        },
        |_| {
            let wal = wal.borrow();
            let f = wal.as_deref().expect("prepared");
            for i in 0..SYNCED_WRITES {
                f.write_at(&page, i * PAGE_SIZE).expect("write");
                f.sync_data().expect("sync");
            }
        },
    ));

    write_table(
        &format!("lab-storage-{fs_name}.md"),
        &format!("lab::storage on {fs_name}"),
        &[
            ("directory", dir.display().to_string()),
            (
                "data file",
                format!("{DATA_PAGES} pages of {PAGE_SIZE} B (1 GiB), reserved with reserve()"),
            ),
            ("rounds", format!("{ROUNDS}, each timed as one batch")),
        ],
        &rows,
    );
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn storage() {
    run(&env_dir("LPG_LAB_EXT4"), "ext4");
    run(&env_dir("LPG_LAB_XFS"), "xfs");
}
