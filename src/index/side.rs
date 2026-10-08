//! The side tree of an online index build (TS-18 steps 2, 5 and 6).
//!
//! While an index is POPULATING, each committer writes the changes of that
//! index its commit makes to the index's side tree, an ordinary logged
//! B+tree of frozen entries keyed in commit order: the commit timestamp and
//! a sequence number within the commit ([`record_key`]), each 8 and 4
//! bytes big-endian, and valued by the operation byte and the index key
//! ([`record_value`]). Each change carries the key the writer derived from
//! the record versions it replaced and wrote, so the replay never reads
//! records again (SurrealDB's index builder queues the old and new values
//! its writers saw, `surrealdb/core/src/kvs/index/` @ `18971ff`, BSL 1.1,
//! studied only).
//!
//! **Replay** ([`replay`]): the records are applied to the index in commit
//! order, idempotently: an insert of a present key and a delete of an
//! absent key do nothing; an insert adds the key and a delete removes it.
//! For every key with a record, the result is the operation of its last
//! record, whatever the builder's scan saw of it. This ports the merge
//! rules of TiDB's temporary index (R-TIDB @ `1f819a0`
//! `pkg/ddl/index_merge_tmp.go:86-126`, `pkg/ddl/index.go:1920-1979`,
//! Apache-2.0, concept only, no code copied): a temporary-index insert
//! found in the original index is skipped, and a delete removes the key;
//! TiDB's extra handle comparison for unique keys is not needed here,
//! since every index key ends with its element identifier.
//!
//! **ONLINE switch** ([`online_tail`]): under the commit latch, the
//! records written after the last one the builder replayed are replayed
//! too, in the batch that marks the index ONLINE, so no commit falls
//! between the last applied change and the switch (SurrealDB's `Closing`
//! phase stops admissions before its final drain, `builder.rs:686-687`,
//! studied only). The `loom` model `loom_side_tree_replay_and_online_switch`
//! (`src/index/tests.rs`) checks the protocol over these functions.

use crate::storage::error::{Error, Result};

/// Length of a side-tree key.
pub(crate) const RECORD_KEY_LEN: usize = 12;

/// A side-tree record's operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    /// The index key was removed.
    Delete = 0,
    /// The index key was added.
    Insert = 1,
}

/// The side-tree key of record `seq` of the commit at `ts`.
pub(crate) fn record_key(ts: u64, seq: u32) -> [u8; RECORD_KEY_LEN] {
    let mut k = [0u8; RECORD_KEY_LEN];
    k[..8].copy_from_slice(&ts.to_be_bytes());
    k[8..].copy_from_slice(&seq.to_be_bytes());
    k
}

/// The side-tree value of `op` on index key `key`.
pub(crate) fn record_value(op: Op, key: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(1 + key.len());
    v.push(op as u8);
    v.extend_from_slice(key);
    v
}

/// The operation and index key of a side-tree value.
///
/// # Errors
///
/// [`Error::Corruption`] for a malformed value.
pub(crate) fn decode_value(v: &[u8]) -> Result<(Op, &[u8])> {
    match v.split_first() {
        Some((0, k)) => Ok((Op::Delete, k)),
        Some((1, k)) => Ok((Op::Insert, k)),
        _ => Err(Error::Corruption("malformed side-tree record".into())),
    }
}

/// An index under construction, as the replay sees it.
pub(crate) trait Target {
    /// Whether `key` is in the index.
    ///
    /// # Errors
    ///
    /// Storage errors.
    fn present(&mut self, key: &[u8]) -> Result<bool>;

    /// Adds `key` (`true`) or removes it (`false`).
    fn set(&mut self, key: &[u8], present: bool);
}

/// Applies `records` to `target` in order, idempotently (module
/// documentation). Returns the change of the number of keys.
///
/// # Errors
///
/// Errors of the target.
pub(crate) fn replay<'a>(
    target: &mut impl Target,
    records: impl IntoIterator<Item = (Op, &'a [u8])>,
) -> Result<i64> {
    let mut delta = 0;
    for (op, key) in records {
        let present = target.present(key)?;
        match op {
            Op::Insert if !present => {
                target.set(key, true);
                delta += 1;
            }
            Op::Delete if present => {
                target.set(key, false);
                delta -= 1;
            }
            _ => {}
        }
    }
    Ok(delta)
}

/// The ONLINE switch's replay (TS-18 step 6), under the commit latch:
/// applies the records written after the builder's last replayed one,
/// `tail`, to `target`. The caller marks the index ONLINE in the same
/// batch. Returns the change of the number of keys.
///
/// # Errors
///
/// Errors of the target.
pub(crate) fn online_tail<'a>(
    target: &mut impl Target,
    tail: impl IntoIterator<Item = (Op, &'a [u8])>,
) -> Result<i64> {
    replay(target, tail)
}
