//! The old-version store (TS-14 Mutable records).
//!
//! A stamped tree keeps the newest committed version of each entry in its
//! page; the versions it replaced live here, in memory, as chains from
//! newest to oldest per `(tree, key)`. Each version is a whole before-image
//! (value bytes, overflow included, and stamps), so no old version refers
//! to a page (TS-05 Reuse). A creation has no before-image: a chain that
//! ends without a version visible to a snapshot means the key did not exist
//! for it. Memgraph keeps the same newest-to-oldest chains and walks them
//! until a version committed before the reader's start (R-MG @ `847e224`
//! `src/storage/v2/mvcc.hpp:34-90`).
//!
//! The store is not durable: recovery starts with it empty (TS-13 step 7),
//! since every snapshot after an open is at or above the highest recovered
//! commit timestamp.
//!
//! A version replaced by the commit at timestamp `r` is needed only by
//! snapshots below `r`. Garbage collection frees it once the horizon (the
//! oldest snapshot, TS-14) is at or above `r`; versions are freed in the
//! order they were replaced, oldest first, from a queue.

use std::collections::{HashMap, VecDeque};

use crate::storage::btree::Stamps;
use crate::storage::memory::Reservation;
use crate::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Bytes an old version is charged to the memory cap with, beyond its key
/// and value: its stamps, chain and queue entries.
const VERSION_OVERHEAD: u64 = 96;

/// The charge of the before-image of `key` with a value of `value_len`
/// bytes (TS-14: reserved when the write enters the write set).
pub(crate) fn version_bytes(key: &[u8], value_len: usize) -> u64 {
    (key.len() + value_len) as u64 + VERSION_OVERHEAD
}

/// One replaced version.
#[derive(Debug)]
struct Version {
    value: Box<[u8]>,
    stamps: Stamps,
    /// Commit timestamp of the commit that replaced it.
    replaced: u64,
    /// Its bytes in the memory accountant.
    _charge: Reservation,
}

/// What a snapshot sees of a key whose page version is too new for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Older {
    /// This value.
    Value(Vec<u8>),
    /// Nothing: the key was absent at the snapshot.
    Absent,
}

/// Whether an entry with `stamps` is visible to snapshot `s`: `Some(true)`
/// visible, `Some(false)` deleted for it, `None` created after it.
pub(crate) fn visible(stamps: Stamps, s: u64) -> Option<bool> {
    if stamps.created > s {
        None
    } else {
        Some(stamps.deleted == 0 || stamps.deleted > s)
    }
}

#[derive(Debug, Default)]
struct Store {
    /// Newest first.
    chains: HashMap<(u64, Box<[u8]>), VecDeque<Version>>,
    /// `(replaced, tree, key)` in commit order.
    queue: VecDeque<(u64, u64, Box<[u8]>)>,
    bytes: u64,
}

/// The old-version store of one database.
#[derive(Debug, Default)]
pub(crate) struct Versions {
    store: RwLock<Store>,
}

impl Versions {
    fn read(&self) -> RwLockReadGuard<'_, Store> {
        // Every update completes before anything that can panic.
        self.store.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Store> {
        self.store.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds the before-image `value` with `stamps` of `key` in `tree`,
    /// replaced by the commit at `replaced`, as the newest version of its
    /// chain. `charge` holds its bytes. Called under the commit latch,
    /// before the page changes (TS-16 step 6), so a reader that finds the
    /// page changed finds the version here.
    pub(crate) fn push(
        &self,
        tree: u64,
        key: &[u8],
        value: Vec<u8>,
        stamps: Stamps,
        replaced: u64,
        charge: Reservation,
    ) {
        let mut st = self.write();
        st.bytes += charge.bytes();
        st.chains
            .entry((tree, key.into()))
            .or_default()
            .push_front(Version {
                value: value.into(),
                stamps,
                replaced,
                _charge: charge,
            });
        st.queue.push_back((replaced, tree, key.into()));
    }

    /// Removes the newest version of `key` in `tree`, added by the commit
    /// at `replaced`, which is being undone (TS-16 step 6 undo).
    pub(crate) fn undo(&self, tree: u64, key: &[u8], replaced: u64) {
        let mut st = self.write();
        let k = (tree, Box::<[u8]>::from(key));
        if let Some(chain) = st.chains.get_mut(&k)
            && chain.front().is_some_and(|v| v.replaced == replaced)
        {
            let v = chain.pop_front().expect("checked above");
            if chain.is_empty() {
                st.chains.remove(&k);
            }
            st.bytes -= v._charge.bytes();
        }
        if let Some(i) = st
            .queue
            .iter()
            .rposition(|(r, t, q)| *r == replaced && *t == tree && **q == *key)
        {
            st.queue.remove(i);
        }
    }

    /// What snapshot `s` sees of `key` in `tree` when the page version is
    /// newer than `s`: the newest version created at or below `s`, or
    /// nothing if the chain has none.
    pub(crate) fn find(&self, tree: u64, key: &[u8], s: u64) -> Older {
        let st = self.read();
        let Some(chain) = st.chains.get(&(tree, Box::<[u8]>::from(key))) else {
            return Older::Absent;
        };
        for v in chain {
            match visible(v.stamps, s) {
                None => continue,
                Some(true) => return Older::Value(v.value.to_vec()),
                Some(false) => return Older::Absent,
            }
        }
        Older::Absent
    }

    /// Frees every version replaced at or below `horizon` (TS-14 Garbage
    /// collection). Returns how many were freed.
    pub(crate) fn collect(&self, horizon: u64) -> usize {
        let mut st = self.write();
        let mut n = 0;
        while st.queue.front().is_some_and(|q| q.0 <= horizon) {
            let (replaced, tree, key) = st.queue.pop_front().expect("checked above");
            let k = (tree, key);
            let Some(chain) = st.chains.get_mut(&k) else {
                continue;
            };
            // Versions of one key are replaced in commit order, so the
            // oldest is the one this queue entry names.
            if chain.back().is_some_and(|v| v.replaced == replaced) {
                let v = chain.pop_back().expect("checked above");
                if chain.is_empty() {
                    st.chains.remove(&k);
                }
                st.bytes -= v._charge.bytes();
                n += 1;
            }
        }
        n
    }

    /// Versions held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.read().queue.len()
    }

    /// Bytes charged for the versions held.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> u64 {
        self.read().bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryAccountant;

    fn st(created: u64, deleted: u64) -> Stamps {
        Stamps { created, deleted }
    }

    #[test]
    fn a_chain_returns_the_version_each_snapshot_sees() {
        let a = MemoryAccountant::new(1 << 20);
        let v = Versions::default();
        let charge = |n| a.try_reserve(n).unwrap();
        // Versions of key k: created at 2 (replaced at 5), created at 5
        // and deleted at 7 (replaced at 9 by a re-creation in the page).
        v.push(1, b"k", b"two".to_vec(), st(2, 0), 5, charge(10));
        v.push(1, b"k", b"five".to_vec(), st(5, 7), 9, charge(10));
        assert_eq!(v.find(1, b"k", 1), Older::Absent);
        assert_eq!(v.find(1, b"k", 4), Older::Value(b"two".to_vec()));
        assert_eq!(v.find(1, b"k", 6), Older::Value(b"five".to_vec()));
        assert_eq!(v.find(1, b"k", 8), Older::Absent);
        assert_eq!(v.find(2, b"k", 4), Older::Absent);
        assert_eq!((v.len(), v.bytes(), a.used()), (2, 20, 20));
        // The horizon passes the first replacement only.
        assert_eq!(v.collect(5), 1);
        assert_eq!(v.find(1, b"k", 6), Older::Value(b"five".to_vec()));
        assert_eq!((v.len(), a.used()), (1, 10));
        v.undo(1, b"k", 9);
        assert_eq!((v.len(), v.bytes(), a.used()), (0, 0, 0));
        assert_eq!(v.collect(u64::MAX), 0);
    }

    #[test]
    fn visibility_compares_both_stamps_with_the_snapshot() {
        assert_eq!(visible(st(0, 0), 1), Some(true));
        assert_eq!(visible(st(3, 0), 2), None);
        assert_eq!(visible(st(3, 0), 3), Some(true));
        assert_eq!(visible(st(3, 5), 4), Some(true));
        assert_eq!(visible(st(3, 5), 5), Some(false));
        assert_eq!(visible(st(0, 5), 9), Some(false));
    }
}
