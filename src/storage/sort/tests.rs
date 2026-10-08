use crate::sync::Arc;
use std::path::Path;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::*;
use crate::obs::recording::RecordingSink;
use crate::storage::FsStorage;
use crate::storage::engine::Engine;
use crate::storage::fault::{Call, Fault, FaultStorage, Kind};
use crate::test_support::{seed, test_dirs};

const CAP: u64 = 16 << 20;

fn engine(dir: &Path, storage: Arc<dyn Storage>) -> Engine {
    Engine::create(dir, storage, CAP, Arc::new(RecordingSink::default())).unwrap()
}

/// `n` records with random keys of 8 to 40 bytes and values of 0 to 60
/// bytes, from `seed`.
fn records(n: usize, seed: u64) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..n)
        .map(|i| {
            let mut k = vec![0u8; rng.random_range(8..=40)];
            rng.fill(&mut k[..]);
            // A unique suffix, so that the expected order is total.
            k.extend_from_slice(&(i as u32).to_be_bytes());
            let v = vec![(i % 251) as u8; rng.random_range(0..=60)];
            (k, v)
        })
        .collect()
}

fn drain(sorted: &mut Sorted<'_>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    while let Some((k, v)) = sorted.next_record().unwrap() {
        out.push((k.to_vec(), v.to_vec()));
    }
    out
}

/// rmp #17: an input 20 times the sort's reservation spills runs to
/// temporary files through the storage trait, merges them (in more than one
/// pass here), returns every record in key order, never holds more than
/// its reservation nor makes the accountant exceed the cap, and removes its
/// files at the end.
#[test]
fn an_input_twenty_times_the_reservation_spills_runs_and_comes_back_sorted() {
    let dir = test_dirs().remove(0);
    let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    fault.skip_device_sync();
    let e = engine(dir.path(), fault.clone());
    let memory = MIN_MEMORY;
    let mut input = records(0, 0);
    let mut bytes = 0u64;
    let mut sd = seed();
    while bytes < 20 * memory {
        let more = records(1_000, sd);
        sd = sd.wrapping_add(1);
        bytes += more
            .iter()
            .map(|(k, v)| (HEAD + k.len() + v.len()) as u64)
            .sum::<u64>();
        input.extend(more);
    }
    let tmp = tempfile::tempdir_in(dir.path()).unwrap();
    let calls = fault.calls().len();
    let mut sorter = Sorter::new(&*fault, tmp.path(), "sort", e.pool(), memory).unwrap();
    assert_eq!(sorter.memory(), memory);
    for (k, v) in &input {
        sorter.push(k, v).unwrap();
        assert!(sorter.allocated() as u64 <= memory);
    }
    assert!(sorter.runs() >= 19, "{} runs", sorter.runs());
    // More runs than one merge can open: some are merged in an earlier pass.
    assert!(sorter.runs() > sorter_runs_bound());
    let mut sorted = sorter.finish().unwrap();
    assert_eq!(sorted.len(), input.len() as u64);
    let out = drain(&mut sorted);
    let mut expected = input.clone();
    expected.sort();
    assert!(out == expected, "the output is not the sorted input");
    assert!(e.accountant().peak() <= CAP);
    let log = &fault.calls()[calls..];
    let spilled =
        |c: &Call| matches!(c, Call::WriteAt { path, .. } if path.starts_with(tmp.path()));
    assert!(log.iter().filter(|c| spilled(c)).count() >= 20);
    drop(sorted);
    assert!(fault.list(tmp.path()).unwrap().is_empty(), "files left");
}

/// The fan-in of the final merge at [`MIN_MEMORY`] with short records.
fn sorter_runs_bound() -> usize {
    MIN_MEMORY as usize / (MIN_READ + HEAP_ENTRY)
}

/// A sort that fits its reservation writes no file.
#[test]
fn a_sort_that_fits_in_memory_writes_no_file() {
    let dir = test_dirs().remove(0);
    let e = engine(dir.path(), crate::test_support::no_flush());
    let tmp = tempfile::tempdir_in(dir.path()).unwrap();
    let input = records(500, seed());
    let mut sorter = Sorter::new(&FsStorage, tmp.path(), "s", e.pool(), 1 << 20).unwrap();
    for (k, v) in &input {
        sorter.push(k, v).unwrap();
    }
    let mut sorted = sorter.finish().unwrap();
    assert_eq!(sorted.files(), 0);
    let mut expected = input;
    expected.sort();
    assert!(drain(&mut sorted) == expected);
    assert!(FsStorage.list(tmp.path()).unwrap().is_empty());
}

/// rmp #17: the temporary files are removed when a sort fails, and its
/// reservation is given back.
#[test]
fn a_failed_sort_removes_its_files_and_releases_its_memory() {
    let dir = test_dirs().remove(0);
    let fault = Arc::new(FaultStorage::new(Box::new(FsStorage)));
    fault.skip_device_sync();
    let e = engine(dir.path(), fault.clone());
    let tmp = tempfile::tempdir_in(dir.path()).unwrap();
    let used = e.accountant().used();
    let input = records(20_000, seed());
    // The third run's first write fails.
    let mut sorter = Sorter::new(&*fault, tmp.path(), "s", e.pool(), MIN_MEMORY).unwrap();
    let mut armed = false;
    let mut err = None;
    for (k, v) in &input {
        if sorter.runs() == 2 && !armed {
            fault.fail_nth(
                Kind::WriteAt,
                fault.count(Kind::WriteAt) + 1,
                Fault::NoSpace,
            );
            armed = true;
        }
        if let Err(x) = sorter.push(k, v) {
            err = Some(x);
            break;
        }
    }
    assert!(
        err.is_some_and(|x| x.is_no_space()),
        "the injected ENOSPC fails the sort"
    );
    assert!(
        !fault.list(tmp.path()).unwrap().is_empty(),
        "precondition: runs were written"
    );
    drop(sorter);
    assert!(fault.list(tmp.path()).unwrap().is_empty(), "files left");
    assert_eq!(e.accountant().used(), used);
}

/// A reservation the cap cannot hold fails before anything is written.
#[test]
fn a_reservation_beyond_the_cap_fails() {
    let dir = test_dirs().remove(0);
    let e = engine(dir.path(), crate::test_support::no_flush());
    let r = Sorter::new(&FsStorage, dir.path(), "s", e.pool(), 2 * CAP);
    assert!(matches!(r, Err(Error::MemoryLimit { .. })));
    assert!(matches!(
        Sorter::new(&FsStorage, dir.path(), "s", e.pool(), MIN_MEMORY - 1),
        Err(Error::InvalidArgument(_))
    ));
}

/// Audit (2026-10-03), rmp #17 "a crash leaves no visible state": the run
/// files are never synced and are removed only by `Drop`, so a process
/// crash during a sort leaves `<prefix>-<n>` files in the sort directory.
/// The next sort with the same prefix in that directory then fails on its
/// first spill, because `Storage::create` refuses an existing name. Nothing
/// removes the leftovers (PostgreSQL removes temporary files at startup,
/// `RemovePgTempFiles`, R-PG `src/backend/postmaster/postmaster.c:1332`).
/// Fails until a sort tolerates or removes files left by an earlier one.
#[test]
fn audit_a_run_file_left_by_a_crashed_sort_does_not_fail_the_next_sort() {
    let dir = test_dirs().remove(0);
    let e = engine(dir.path(), crate::test_support::no_flush());
    let tmp = tempfile::tempdir_in(dir.path()).unwrap();
    // What a sort killed after its first spill leaves.
    FsStorage
        .create(&tmp.path().join("s-0"))
        .unwrap()
        .write_at(&[1; 100], 0)
        .unwrap();
    let input = records(20_000, seed());
    let mut sorter = Sorter::new(&FsStorage, tmp.path(), "s", e.pool(), MIN_MEMORY).unwrap();
    for (k, v) in &input {
        sorter
            .push(k, v)
            .unwrap_or_else(|err| panic!("a leftover run file fails the sort: {err}"));
    }
    let mut sorted = sorter.finish().unwrap();
    assert_eq!(drain(&mut sorted).len(), input.len());
}
