//! Durability suites (TS-30): the fault-injection crash suite ([`fault`],
//! rmp #20) and the `kill -9` suite ([`kill`], rmp #21).
//!
//! Both run in full in the directories named by `LPG_LAB_EXT4` and
//! `LPG_LAB_XFS` (NFR-PLAT-004), one directory after the other, and print
//! their counts. When neither variable is set, as in the default
//! `cargo nextest run -p lpg`, they run a sample in `/tmp/lpgram`
//! ([`crate::test_support::graph_base`], a `tmpfs`), so that the default
//! run stays fast. The acceptance runs are the ones on ext4 and XFS: a
//! `tmpfs` is never a production scenario and its runs do not replace them:
//!
//! ```text
//! LPG_LAB_EXT4=/data/tmp/lpg-lab LPG_LAB_XFS=/mnt/lpg-xfs \
//!     cargo nextest run -p lpg suites::fault --no-capture
//! ```

use std::path::PathBuf;

mod fault;
mod kill;

/// The directories a suite runs in, each with the file system that holds
/// it, and whether the run is the full one: the lab directories when set,
/// else `/tmp/lpgram` and a sample.
///
/// The file system is measured, not taken from the variable's name
/// (audit F7): `findmnt --target` reports the type and source of the
/// mount that holds the directory, from the kernel's mount table, and a
/// lab directory whose type differs from its variable's fails the run. The
/// label reads `<type> source=<device>`, as the suites print it.
///
/// # Panics
///
/// If `findmnt` fails, or a lab directory is not on the file system its
/// variable names.
pub(crate) fn suite_dirs() -> (Vec<(String, PathBuf)>, bool) {
    let mut dirs = Vec::new();
    for (want, var) in [("ext4", "LPG_LAB_EXT4"), ("xfs", "LPG_LAB_XFS")] {
        if let Some(d) = std::env::var_os(var) {
            let d = PathBuf::from(d);
            let (fs, source) = mount_of(&d);
            assert_eq!(fs, want, "{var}={} is on {fs} ({source})", d.display());
            dirs.push((format!("{fs} source={source}"), d));
        }
    }
    if dirs.is_empty() {
        let d = crate::test_support::graph_base().to_path_buf();
        let (fs, source) = mount_of(&d);
        (vec![(format!("{fs} source={source}"), d)], false)
    } else {
        (dirs, true)
    }
}

/// The file-system type and source of the mount that holds `dir`, as
/// `findmnt` reports them.
fn mount_of(dir: &std::path::Path) -> (String, String) {
    let out = std::process::Command::new("findmnt")
        .args(["--noheadings", "--output", "FSTYPE,SOURCE", "--target"])
        .arg(dir)
        .output()
        .unwrap_or_else(|e| panic!("findmnt: {e}"));
    assert!(out.status.success(), "findmnt {}: {out:?}", dir.display());
    let text = String::from_utf8_lossy(&out.stdout);
    let mut f = text.split_whitespace();
    match (f.next(), f.next()) {
        (Some(fs), Some(source)) => (fs.to_owned(), source.to_owned()),
        _ => panic!("findmnt {}: {text:?}", dir.display()),
    }
}

/// The crash oracle of the garbage collector (TS-14, audit Round A N3):
/// once the reopened database's seed scan has run, a pass with no snapshot
/// open leaves no entry of any live tree with a delete stamp at or below
/// its horizon. Entries the seed scan missed would stay forever, since the
/// commit queues that found them were lost with the process.
///
/// # Panics
///
/// If the seed scan does not succeed within
/// [`crate::test_support::WAIT_BOUND`], the pass fails, or an entry keeps
/// a collectable delete stamp.
fn assert_stamps_collected(e: &crate::storage::engine::Engine, what: &str) {
    use crate::storage::btree::{DIRECTORY, Tree, decode_directory_entry};
    use crate::sync::atomic::Ordering;
    crate::test_support::wait_until(&format!("{what}: the seed scan"), || {
        !e.core().mvcc.unseeded.load(Ordering::SeqCst)
    });
    let pass = crate::tx::gc::pass(e.core()).unwrap_or_else(|err| panic!("{what}: pass: {err}"));
    let store = e.store();
    let mut trees = Vec::new();
    store
        .scan_prefix(DIRECTORY, b"", |k, v| {
            if let (Ok(id), Some((root, 0))) = (<[u8; 8]>::try_from(k), decode_directory_entry(v)) {
                trees.push(Tree::new(u64::from_be_bytes(id), root));
            }
            true
        })
        .unwrap();
    for t in trees {
        store
            .scan_prefix_stamped(t, b"", |k, _, st| {
                assert!(
                    st.deleted == 0 || st.deleted > pass.horizon,
                    "{what}: tree {} key {k:?} keeps delete stamp {} at or below the horizon {}",
                    t.id(),
                    st.deleted,
                    pass.horizon
                );
                true
            })
            .unwrap();
    }
}

/// Opens a database with `open` while its index builds are held
/// ([`crate::index::build::HOLD_RESTARTS`]), so that nothing commits
/// between the open and [`assert_no_stamp_above`]; [`finish_builds`]
/// starts them.
///
/// # Errors
///
/// The error of `open`.
fn open_held<T>(
    dir: &std::path::Path,
    open: impl FnOnce() -> crate::storage::error::Result<T>,
) -> crate::storage::error::Result<T> {
    use crate::index::build::HOLD_RESTARTS;
    let hold = || HOLD_RESTARTS.lock().unwrap_or_else(|p| p.into_inner());
    hold().push(dir.to_path_buf());
    let r = open();
    hold().retain(|x| x != dir);
    r
}

/// The crash oracle of the commit timestamp (rmp #35): right after an
/// open, before anything commits ([`open_held`]; garbage-collection
/// batches draw no timestamp, TS-14), no entry of a live tree carries a
/// created or delete stamp above the recovered `commit_ts`, the highest
/// published one, and no tree a drop stamp above it. A stamp above it
/// would belong to a commit that recovery did not keep, or would make a
/// later commit at that timestamp ambiguous.
///
/// # Panics
///
/// If a stamp lies above the recovered `commit_ts`, or a scan fails.
fn assert_no_stamp_above(e: &crate::storage::engine::Engine, what: &str) {
    use crate::storage::btree::{DIRECTORY, Tree, decode_directory_entry};
    let ts = e.core().wal.published();
    let store = e.store();
    let mut trees = Vec::new();
    store
        .scan_prefix(DIRECTORY, b"", |k, v| {
            let (Ok(id), Some((root, dropped))) =
                (<[u8; 8]>::try_from(k), decode_directory_entry(v))
            else {
                panic!("{what}: malformed directory entry {k:?}");
            };
            assert!(
                dropped <= ts,
                "{what}: tree {} dropped at {dropped}, above the recovered commit_ts {ts}",
                u64::from_be_bytes(id)
            );
            // A dropped tree may be freed by the collector meanwhile.
            if dropped == 0 {
                trees.push(Tree::new(u64::from_be_bytes(id), root));
            }
            true
        })
        .unwrap();
    for t in trees {
        store
            .scan_prefix_stamped(t, b"", |k, _, st| {
                assert!(
                    st.created <= ts && st.deleted <= ts,
                    "{what}: tree {} key {k:?} stamped {st:?}, above the recovered commit_ts {ts}",
                    t.id()
                );
                true
            })
            .unwrap();
    }
    assert_eq!(
        e.core().wal.published(),
        ts,
        "{what}: a commit ran during the stamp check"
    );
}

/// Starts the index builds [`open_held`] held and waits for each; every
/// index is then ONLINE, so the integrity check compares it with the data
/// (TS-12). Returns the identifiers of the indexes.
///
/// # Panics
///
/// If a build fails or an index is left POPULATING.
fn finish_builds(e: &crate::storage::engine::Engine, what: &str) -> Vec<u64> {
    use crate::index::State;
    crate::index::restart_builds(e).unwrap_or_else(|x| panic!("{what}: restart builds: {x}"));
    let ids: Vec<u64> = e
        .core()
        .catalog
        .schema()
        .values()
        .filter(|d| d.kind.is_index())
        .map(|d| d.id)
        .collect();
    for &id in &ids {
        e.wait_index(id)
            .unwrap_or_else(|x| panic!("{what}: build of index {id}: {x}"));
        assert_eq!(
            e.core().catalog.schema()[&id].state,
            State::Online,
            "{what}: index {id}"
        );
    }
    ids
}
