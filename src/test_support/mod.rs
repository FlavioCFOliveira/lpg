//! Helpers shared by the crate's tests.

use std::ffi::OsString;
use std::path::Path;

use crate::storage::{FsStorage, Storage};

pub(crate) mod child;

/// Directories a test runs in: one under `LPG_LAB_EXT4` and one under
/// `LPG_LAB_XFS` when set, else one under the system temporary directory.
pub(crate) fn test_dirs() -> Vec<tempfile::TempDir> {
    let dirs: Vec<_> = ["LPG_LAB_EXT4", "LPG_LAB_XFS"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|d| tempfile::tempdir_in(d).expect("create test directory"))
        .collect();
    if dirs.is_empty() {
        vec![tempfile::tempdir().expect("create test directory")]
    } else {
        dirs
    }
}

/// Every file of `dir` with its full content, read through [`FsStorage`],
/// sorted by name. Comparing two snapshots compares every byte, which is
/// stronger than comparing digests.
pub(crate) fn snapshot(dir: &Path) -> Vec<(OsString, Vec<u8>)> {
    FsStorage
        .list(dir)
        .expect("list directory")
        .into_iter()
        .map(|name| {
            let file = FsStorage.open(&dir.join(&name)).expect("open file");
            let mut bytes = vec![0; file.len().expect("file length") as usize];
            let n = file.read_at(&mut bytes, 0).expect("read file");
            assert_eq!(n, bytes.len());
            (name, bytes)
        })
        .collect()
}
