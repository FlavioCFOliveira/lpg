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

/// The directories a suite runs in, with a short name, and whether the run
/// is the full one: the lab directories when set, else `/tmp/lpgram` and a
/// sample.
pub(crate) fn suite_dirs() -> (Vec<(&'static str, PathBuf)>, bool) {
    let mut dirs = Vec::new();
    for (name, var) in [("ext4", "LPG_LAB_EXT4"), ("xfs", "LPG_LAB_XFS")] {
        if let Some(d) = std::env::var_os(var) {
            dirs.push((name, PathBuf::from(d)));
        }
    }
    if dirs.is_empty() {
        (
            vec![("tmpfs", crate::test_support::graph_base().to_path_buf())],
            false,
        )
    } else {
        (dirs, true)
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
