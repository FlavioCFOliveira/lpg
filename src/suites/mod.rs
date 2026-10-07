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
