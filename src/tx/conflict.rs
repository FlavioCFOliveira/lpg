//! Read sets, write keys and commit-time validation (TS-15 Read sets, Write
//! keys, Validation).
//!
//! A serializable transaction records each read as a half-open interval
//! `[lo, hi)` of encoded keys of one tree. Every `(tree, key)` a commit
//! writes is a write key; each read-write commit registers its write keys
//! under the commit latch with its commit timestamp. At commit, under the
//! latch and after drawing `commit_ts`, a serializable transaction's read
//! set is checked against the write keys of every commit with a timestamp
//! between its snapshot and its own: a write key inside a read interval of
//! the same tree fails the commit with [`Error::Serialization`]
//! (GQLSTATUS 25N11, FR-TX-008, FR-TX-009).
//!
//! fjall's optimistic transactions keep the same records, a single key, a
//! range or a whole keyspace per read and every written key as a conflict
//! key (R-FJALL @ `17ae672` `src/tx/optimistic/conflict_manager.rs:14-22,
//! 46-55`), and reject a commit whose reads overlap the conflict keys of a
//! transaction that committed after its snapshot (`oracle.rs:23-59`).
//! fjall has no memory cap and never merges reads; the merging rule below
//! is lpg's (TS-15 Merging).

use std::collections::{BTreeMap, VecDeque};

use crate::storage::error::{Error, Result};
use crate::storage::memory::Reservation;
use crate::sync::{Mutex, MutexGuard, PoisonError};

/// Bytes an interval or a write key is charged with beyond its key bytes.
const ENTRY_OVERHEAD: u64 = 48;

/// A half-open interval `[lo, hi)` of keys of one tree; `hi` `None` is
/// unbounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interval {
    pub(crate) lo: Vec<u8>,
    pub(crate) hi: Option<Vec<u8>>,
}

impl Interval {
    /// The read of key `k`: `[k, k ‖ 0x00)`.
    pub(crate) fn point(k: &[u8]) -> Interval {
        let mut hi = k.to_vec();
        hi.push(0);
        Interval {
            lo: k.to_vec(),
            hi: Some(hi),
        }
    }

    /// The read of every key with prefix `p`: `[p, p⁺)`, where `p⁺` is `p`
    /// with its last byte below 0xFF incremented and the bytes after it
    /// removed; unbounded if every byte of `p` is 0xFF (the empty prefix
    /// included: the whole tree).
    pub(crate) fn prefix(p: &[u8]) -> Interval {
        let hi = p.iter().rposition(|&b| b < 0xFF).map(|i| {
            let mut h = p[..=i].to_vec();
            h[i] += 1;
            h
        });
        Interval { lo: p.to_vec(), hi }
    }

    fn contains(&self, k: &[u8]) -> bool {
        self.lo.as_slice() <= k && self.hi.as_deref().is_none_or(|h| k < h)
    }

    fn charge(&self) -> u64 {
        (self.lo.len() + self.hi.as_ref().map_or(0, Vec::len)) as u64 + ENTRY_OVERHEAD
    }
}

/// The reads of one serializable transaction.
#[derive(Debug, Default)]
pub(crate) struct ReadSet {
    trees: BTreeMap<u64, Vec<Interval>>,
    charge: Option<Reservation>,
}

impl ReadSet {
    /// Records `iv` as a read of `tree`, reserving its bytes with
    /// `reserve` (the buffer pool's, which evicts before it refuses, TS-08).
    /// When the reservation fails, the intervals of the tree with the most
    /// intervals are replaced by one interval from their lowest `lo` to
    /// their highest `hi`, and the reservation is tried again, tree by tree
    /// (TS-15 Merging). Merging only widens, so it can add false conflicts
    /// and never misses one.
    ///
    /// # Errors
    ///
    /// [`Error::MemoryLimit`] if every tree holds one interval and the
    /// reservation still fails; other errors of `reserve`.
    pub(crate) fn add(
        &mut self,
        reserve: impl Fn(u64) -> Result<Reservation>,
        tree: u64,
        iv: Interval,
    ) -> Result<()> {
        let bytes = iv.charge();
        loop {
            match reserve(bytes) {
                Ok(r) => {
                    match &mut self.charge {
                        Some(c) => c.absorb(r),
                        None => self.charge = Some(r),
                    }
                    self.trees.entry(tree).or_default().push(iv);
                    return Ok(());
                }
                Err(Error::MemoryLimit { .. }) => {
                    if !self.merge_largest() {
                        return Err(Error::MemoryLimit { requested: bytes });
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Merges the intervals of the tree with the most intervals into one,
    /// releasing the bytes saved. Returns `false` if every tree holds one.
    fn merge_largest(&mut self) -> bool {
        let Some((&tree, ivs)) = self
            .trees
            .iter()
            .filter(|(_, v)| v.len() > 1)
            .max_by_key(|(_, v)| v.len())
        else {
            return false;
        };
        let before: u64 = ivs.iter().map(Interval::charge).sum();
        let lo = ivs.iter().map(|i| &i.lo).min().expect("several").clone();
        let hi = if ivs.iter().any(|i| i.hi.is_none()) {
            None
        } else {
            ivs.iter().filter_map(|i| i.hi.clone()).max()
        };
        let merged = Interval { lo, hi };
        let after = merged.charge();
        self.trees.insert(tree, vec![merged]);
        if let Some(c) = &mut self.charge {
            c.shrink(before.saturating_sub(after));
        }
        true
    }

    /// Whether any write key of `keys` lies in a read interval of its
    /// tree.
    pub(crate) fn overlaps(&self, keys: &WriteKeys) -> bool {
        keys.keys.iter().any(|(tree, k)| {
            self.trees
                .get(tree)
                .is_some_and(|ivs| ivs.iter().any(|iv| iv.contains(k)))
        })
    }

    /// Intervals recorded, for tests.
    #[cfg(test)]
    pub(crate) fn intervals(&self, tree: u64) -> &[Interval] {
        self.trees.get(&tree).map_or(&[], Vec::as_slice)
    }
}

/// The write keys of one commit, sorted.
#[derive(Debug, Default)]
pub(crate) struct WriteKeys {
    keys: Vec<(u64, Box<[u8]>)>,
}

impl WriteKeys {
    /// The write keys `keys`.
    pub(crate) fn new(mut keys: Vec<(u64, Box<[u8]>)>) -> WriteKeys {
        keys.sort();
        keys.dedup();
        WriteKeys { keys }
    }

    /// The charge of the write key `key` (TS-14: reserved when the write
    /// enters the write set).
    pub(crate) fn charge_of(key: &[u8]) -> u64 {
        key.len() as u64 + ENTRY_OVERHEAD
    }
}

/// The write-key sets of committed read-write transactions, in commit
/// order (TS-14 Committed write-key sets).
#[derive(Debug, Default)]
pub(crate) struct Committed {
    sets: Mutex<VecDeque<(u64, WriteKeys, Reservation)>>,
}

impl Committed {
    fn sets(&self) -> MutexGuard<'_, VecDeque<(u64, WriteKeys, Reservation)>> {
        self.sets.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Validates `reads`, of a transaction with snapshot `snapshot`,
    /// against the write keys of every commit above `snapshot` (TS-15
    /// Validation). Called under the commit latch after `commit_ts` is
    /// drawn, so every earlier commit has registered its keys.
    ///
    /// # Errors
    ///
    /// [`Error::Serialization`] on an overlap.
    pub(crate) fn validate(&self, snapshot: u64, reads: &ReadSet) -> Result<()> {
        let sets = self.sets();
        if sets
            .iter()
            .filter(|(ts, _, _)| *ts > snapshot)
            .any(|(_, keys, _)| reads.overlaps(keys))
        {
            return Err(Error::Serialization);
        }
        Ok(())
    }

    /// Registers the write keys of the commit at `commit_ts`, charged by
    /// `charge` (TS-16 step 5).
    pub(crate) fn register(&self, commit_ts: u64, keys: WriteKeys, charge: Reservation) {
        self.sets().push_back((commit_ts, keys, charge));
    }

    /// Withdraws the write keys of the commit at `commit_ts`, which is
    /// being undone (TS-16 step 6 undo, item 4).
    pub(crate) fn withdraw(&self, commit_ts: u64) {
        let mut sets = self.sets();
        if sets.back().is_some_and(|s| s.0 == commit_ts) {
            sets.pop_back();
        }
    }

    /// Frees the sets of commits at or below `horizon`, the oldest snapshot
    /// of any running serializable transaction (TS-14 Garbage collection):
    /// no validation can need them. Returns how many were freed.
    pub(crate) fn collect(&self, horizon: u64) -> usize {
        let mut sets = self.sets();
        let mut n = 0;
        while sets.front().is_some_and(|s| s.0 <= horizon) {
            sets.pop_front();
            n += 1;
        }
        n
    }

    /// Sets held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.sets().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryAccountant;
    use crate::sync::Arc;

    /// A reservation from `acct`, as the pool's would be when nothing is
    /// left to evict.
    fn from(acct: &Arc<MemoryAccountant>) -> impl Fn(u64) -> Result<Reservation> + '_ {
        |b| {
            acct.try_reserve(b)
                .ok_or(Error::MemoryLimit { requested: b })
        }
    }

    #[test]
    fn intervals_are_half_open() {
        let p = Interval::point(b"ab");
        assert!(p.contains(b"ab"));
        assert!(!p.contains(b"ab\x00"));
        assert!(!p.contains(b"aa\xff"));
        let q = Interval::prefix(b"a\xff");
        assert_eq!(q.hi.as_deref(), Some(&b"b"[..]));
        assert!(q.contains(b"a\xff\xff\x01"));
        assert!(!q.contains(b"b"));
        assert_eq!(Interval::prefix(b"\xff\xff").hi, None);
        assert_eq!(Interval::prefix(b"").hi, None);
        assert!(Interval::prefix(b"").contains(b"anything"));
    }

    /// A refused reservation merges the intervals of the tree with the
    /// most intervals, which only widens what conflicts; when every tree
    /// holds one interval, the read fails with `MemoryLimit`.
    #[test]
    fn refused_reservations_merge_then_fail() {
        let one = Interval::point(b"k0").charge();
        let acct = MemoryAccountant::new(4 * one);
        let mut r = ReadSet::default();
        for k in [b"k1", b"k5", b"k9"] {
            r.add(from(&acct), 1, Interval::point(k)).unwrap();
        }
        r.add(from(&acct), 2, Interval::point(b"k0")).unwrap();
        // Full: tree 1 (three intervals) merges into [k1, k9 ‖ 0).
        r.add(from(&acct), 2, Interval::point(b"k2")).unwrap();
        assert_eq!(
            r.intervals(1),
            &[Interval {
                lo: b"k1".to_vec(),
                hi: Some(b"k9\x00".to_vec())
            }]
        );
        let keys = WriteKeys::new(vec![(1, b"k3".as_slice().into())]);
        assert!(r.overlaps(&keys), "k3 lies in the merged interval");
        assert!(acct.used() <= 4 * one);
        // Tree 2 now merges, then nothing is left to merge.
        let big = Interval::point(&[7u8; 300]);
        assert!(matches!(
            r.add(from(&acct), 3, big),
            Err(Error::MemoryLimit { .. })
        ));
        assert_eq!(r.intervals(2).len(), 1);
        drop(r);
        assert_eq!(acct.used(), 0);
    }

    #[test]
    fn validation_checks_only_commits_after_the_snapshot() {
        let acct = MemoryAccountant::new(1 << 20);
        let c = Committed::default();
        let keys = |k: &[u8]| WriteKeys::new(vec![(1, k.into())]);
        c.register(3, keys(b"x"), acct.try_reserve(1).unwrap());
        c.register(5, keys(b"y"), acct.try_reserve(1).unwrap());
        let mut r = ReadSet::default();
        r.add(from(&acct), 1, Interval::point(b"x")).unwrap();
        c.validate(3, &r).unwrap();
        assert!(matches!(c.validate(2, &r), Err(Error::Serialization)));
        r.add(from(&acct), 1, Interval::prefix(b"y")).unwrap();
        assert!(matches!(c.validate(4, &r), Err(Error::Serialization)));
        c.withdraw(5);
        c.validate(4, &r).unwrap();
        assert_eq!(c.collect(3), 1);
        assert_eq!(c.len(), 0);
    }
}
