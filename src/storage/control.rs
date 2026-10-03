//! The `CONTROL` file: two checksummed 4 KiB slots (TS-05).
//!
//! Each slot holds one [`Control`] record and a CRC-32C, integers
//! little-endian. The layout is the same in every format version (TS-28), so
//! any crate version can read it:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 4 | CRC-32C of bytes 4..4096 |
//! | 4 | 8 | magic `LPGCTRL\0` |
//! | 12 | 8 | sequence number |
//! | 20 | 4 | format version |
//! | 24 | 4 | page size |
//! | 28 | 16 | database UUID |
//! | 44 | 8 | active generation `<g>` |
//! | 52 | 1 | clean-shutdown flag (0 or 1) |
//! | 53 | 3 | zero |
//! | 56 | 8 | checkpoint redo point (LSN) |
//! | 64 | 8 | highest commit timestamp |
//! | 72 | 8 | next element identifier |
//! | 80 | 8 | next catalog object identifier |
//! | 88 | 8 | next token identifier |
//! | 96 | 4000 | zero |
//!
//! A slot is valid when its checksum and magic match and its flag byte is 0
//! or 1. The valid slot with the higher sequence number is authoritative.
//! Each write goes to the other slot with the next sequence number, with
//! `write_at` and then `sync_data`, so a torn write can damage only the
//! slot that is not authoritative.

use std::path::Path;
use std::sync::Arc;

use super::error::{Error, Health, Result};
use super::{Storage, StorageFile};
use crate::obs::MetricsSink;
use crate::obs::names::checkpointing::CONTROL_SLOT_INVALID;

/// Bytes of one slot.
pub(crate) const SLOT_SIZE: usize = 4096;

const MAGIC: [u8; 8] = *b"LPGCTRL\0";

/// The fields of one `CONTROL` slot, without the sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Control {
    /// On-disk format version (TS-28).
    pub(crate) format_version: u32,
    /// Page size in bytes.
    pub(crate) page_size: u32,
    /// Database UUID.
    pub(crate) uuid: [u8; 16],
    /// Active file-set generation `<g>`.
    pub(crate) generation: u64,
    /// Whether the database was closed cleanly.
    pub(crate) clean_shutdown: bool,
    /// Checkpoint redo point: WAL position where recovery starts.
    pub(crate) redo_point: u64,
    /// Highest commit timestamp.
    pub(crate) commit_ts: u64,
    /// Next element identifier.
    pub(crate) next_element_id: u64,
    /// Next catalog object identifier.
    pub(crate) next_catalog_id: u64,
    /// Next token identifier.
    pub(crate) next_token_id: u64,
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8-byte field"))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4-byte field"))
}

/// Encodes one slot.
fn encode(c: &Control, sequence: u64) -> [u8; SLOT_SIZE] {
    let mut s = [0u8; SLOT_SIZE];
    s[4..12].copy_from_slice(&MAGIC);
    s[12..20].copy_from_slice(&sequence.to_le_bytes());
    s[20..24].copy_from_slice(&c.format_version.to_le_bytes());
    s[24..28].copy_from_slice(&c.page_size.to_le_bytes());
    s[28..44].copy_from_slice(&c.uuid);
    s[44..52].copy_from_slice(&c.generation.to_le_bytes());
    s[52] = u8::from(c.clean_shutdown);
    s[56..64].copy_from_slice(&c.redo_point.to_le_bytes());
    s[64..72].copy_from_slice(&c.commit_ts.to_le_bytes());
    s[72..80].copy_from_slice(&c.next_element_id.to_le_bytes());
    s[80..88].copy_from_slice(&c.next_catalog_id.to_le_bytes());
    s[88..96].copy_from_slice(&c.next_token_id.to_le_bytes());
    let crc = crc32c::crc32c(&s[4..]);
    s[0..4].copy_from_slice(&crc.to_le_bytes());
    s
}

/// Decodes one slot: its record and sequence number, or `None` if the slot
/// is invalid.
fn decode(s: &[u8]) -> Option<(Control, u64)> {
    if u32_at(s, 0) != crc32c::crc32c(&s[4..SLOT_SIZE]) || s[4..12] != MAGIC || s[52] > 1 {
        return None;
    }
    let control = Control {
        format_version: u32_at(s, 20),
        page_size: u32_at(s, 24),
        uuid: s[28..44].try_into().expect("16-byte field"),
        generation: u64_at(s, 44),
        clean_shutdown: s[52] == 1,
        redo_point: u64_at(s, 56),
        commit_ts: u64_at(s, 64),
        next_element_id: u64_at(s, 72),
        next_catalog_id: u64_at(s, 80),
        next_token_id: u64_at(s, 88),
    };
    Some((control, u64_at(s, 12)))
}

/// An open `CONTROL` file.
pub(crate) struct ControlSlots {
    file: Box<dyn StorageFile>,
    current: Control,
    sequence: u64,
    /// Index of the authoritative slot.
    slot: usize,
    health: Arc<Health>,
}

impl ControlSlots {
    /// Creates `path` holding `control` in both slots, with sequence
    /// numbers 0 and 1, and syncs it. Making the name durable is the
    /// caller's `sync_dir`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] from the storage; a failed write or sync also sets the
    /// unusable latch.
    pub(crate) fn create(
        storage: &dyn Storage,
        path: &Path,
        control: Control,
        health: Arc<Health>,
    ) -> Result<ControlSlots> {
        let file = storage.create(path)?;
        let mut both = [0u8; 2 * SLOT_SIZE];
        both[..SLOT_SIZE].copy_from_slice(&encode(&control, 0));
        both[SLOT_SIZE..].copy_from_slice(&encode(&control, 1));
        file.write_at(&both, 0)
            .and_then(|()| file.sync_data())
            .map_err(|e| health.fail(e.into()))?;
        Ok(ControlSlots {
            file,
            current: control,
            sequence: 1,
            slot: 1,
            health,
        })
    }

    /// Reads both slots of an open `CONTROL` file and keeps the valid one
    /// with the higher sequence number. If exactly one slot is invalid, the
    /// open goes on from the valid one and the event is counted in
    /// [`CONTROL_SLOT_INVALID`] (TS-05; FR-REC-003 exception). The next
    /// [`ControlSlots::write`] rewrites the invalid slot.
    ///
    /// # Errors
    ///
    /// [`Error::Corruption`] if both slots are invalid; [`Error::Io`] if the
    /// read fails.
    pub(crate) fn open(
        file: Box<dyn StorageFile>,
        health: Arc<Health>,
        sink: &dyn MetricsSink,
    ) -> Result<ControlSlots> {
        let mut both = [0u8; 2 * SLOT_SIZE];
        // A short file leaves zeros, which fail the checksum.
        file.read_at(&mut both, 0)?;
        let slots = [decode(&both[..SLOT_SIZE]), decode(&both[SLOT_SIZE..])];
        let slot = match slots {
            [None, None] => {
                return Err(Error::Corruption(
                    "both CONTROL slots fail their checksum".into(),
                ));
            }
            [Some(_), None] => 0,
            [None, Some(_)] => 1,
            // Equal sequence numbers do not occur; slot 0 wins, as in LMDB.
            [Some((_, a)), Some((_, b))] => usize::from(b > a),
        };
        if slots.iter().any(Option::is_none) {
            sink.counter(CONTROL_SLOT_INVALID, 1);
        }
        let (current, sequence) = slots[slot].expect("chosen slot is valid");
        Ok(ControlSlots {
            file,
            current,
            sequence,
            slot,
            health,
        })
    }

    /// The authoritative record.
    pub(crate) fn current(&self) -> &Control {
        &self.current
    }

    /// The sequence number of the authoritative slot. The one written by
    /// open's checkpoint is the salt of that open's WAL frames (TS-09).
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Writes `control` to the slot that is not authoritative, with the next
    /// sequence number, then syncs; that slot becomes authoritative.
    ///
    /// # Errors
    ///
    /// [`Error::Unusable`] if the latch is set; [`Error::Io`] if the write
    /// or sync fails, which also sets the latch (TS-12).
    pub(crate) fn write(&mut self, control: Control) -> Result<()> {
        self.health.check()?;
        let target = 1 - self.slot;
        let sequence = self.sequence + 1;
        let bytes = encode(&control, sequence);
        self.file
            .write_at(&bytes, (target * SLOT_SIZE) as u64)
            .and_then(|()| self.file.sync_data())
            .map_err(|e| self.health.fail(e.into()))?;
        self.current = control;
        self.sequence = sequence;
        self.slot = target;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::obs::recording::RecordingSink;
    use crate::storage::FsStorage;

    pub(crate) fn sample() -> Control {
        Control {
            format_version: 1,
            page_size: 8192,
            uuid: [7; 16],
            generation: 1,
            clean_shutdown: false,
            redo_point: 11,
            commit_ts: 12,
            next_element_id: 13,
            next_catalog_id: 14,
            next_token_id: 15,
        }
    }

    /// Rewrites slot `slot` of the `CONTROL` file at `path` as `f` changes
    /// it, with a correct checksum, through the storage abstraction.
    pub(crate) fn patch_slot(path: &Path, slot: usize, f: impl FnOnce(&mut Control)) {
        let file = FsStorage.open(path).unwrap();
        let mut s = [0u8; SLOT_SIZE];
        file.read_at(&mut s, (slot * SLOT_SIZE) as u64).unwrap();
        let (mut c, seq) = decode(&s).expect("valid slot");
        f(&mut c);
        file.write_at(&encode(&c, seq), (slot * SLOT_SIZE) as u64)
            .unwrap();
        file.sync_data().unwrap();
    }

    /// Flips one bit inside slot `slot` of the file at `path`.
    pub(crate) fn corrupt_slot(path: &Path, slot: usize) {
        let file = FsStorage.open(path).unwrap();
        let at = (slot * SLOT_SIZE + 60) as u64;
        let mut b = [0u8; 1];
        file.read_at(&mut b, at).unwrap();
        file.write_at(&[b[0] ^ 0x10], at).unwrap();
        file.sync_data().unwrap();
    }

    fn reopen(path: &Path, sink: &RecordingSink) -> Result<ControlSlots> {
        ControlSlots::open(
            crate::test_support::NoFlush.open(path)?,
            Arc::default(),
            sink,
        )
    }

    #[test]
    fn slot_round_trip_and_flag_check() {
        let c = sample();
        let s = encode(&c, 9);
        assert_eq!(decode(&s), Some((c, 9)));
        let mut bad = s;
        bad[52] = 2; // flag outside 0..=1, checksum recomputed
        let crc = crc32c::crc32c(&bad[4..]);
        bad[0..4].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(decode(&bad), None);
        assert_eq!(decode(&[0u8; SLOT_SIZE]), None);
    }

    #[test]
    fn writes_alternate_and_higher_sequence_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CONTROL");
        let sink = RecordingSink::default();
        let mut cf = ControlSlots::create(
            &crate::test_support::NoFlush,
            &path,
            sample(),
            Arc::default(),
        )
        .unwrap();
        assert_eq!(FsStorage.open(&path).unwrap().len().unwrap(), 8192);
        assert_eq!(reopen(&path, &sink).unwrap().slot, 1);
        for i in 0..4u64 {
            let next = Control {
                commit_ts: 100 + i,
                ..sample()
            };
            cf.write(next).unwrap();
            let again = reopen(&path, &sink).unwrap();
            assert_eq!(again.current().commit_ts, 100 + i);
            assert_eq!(
                again.slot,
                (i as usize) % 2,
                "write {i} went to the other slot"
            );
            assert_eq!(again.sequence, 2 + i);
        }
        assert_eq!(sink.snapshot().counter(CONTROL_SLOT_INVALID), 0);
    }

    #[test]
    fn one_invalid_slot_opens_from_the_other_counts_and_is_rewritten() {
        for torn in [0, 1] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("CONTROL");
            let sink = RecordingSink::default();
            let mut cf = ControlSlots::create(
                &crate::test_support::NoFlush,
                &path,
                sample(),
                Arc::default(),
            )
            .unwrap();
            cf.write(Control {
                commit_ts: 50,
                ..sample()
            })
            .unwrap(); // slot 0, sequence 2
            drop(cf);
            corrupt_slot(&path, torn);
            let before = sink.snapshot();
            let mut cf = reopen(&path, &sink).unwrap();
            assert_eq!(
                sink.snapshot().since(&before).counter(CONTROL_SLOT_INVALID),
                1
            );
            assert_eq!(cf.slot, 1 - torn, "the valid slot is used");
            let expected_ts = if torn == 0 { 12 } else { 50 };
            assert_eq!(cf.current().commit_ts, expected_ts);
            // The next write targets the invalid slot and repairs it.
            cf.write(Control {
                commit_ts: 60,
                ..sample()
            })
            .unwrap();
            let before = sink.snapshot();
            let cf = reopen(&path, &sink).unwrap();
            assert_eq!(
                sink.snapshot().since(&before).counter(CONTROL_SLOT_INVALID),
                0
            );
            assert_eq!(cf.slot, torn);
            assert_eq!(cf.current().commit_ts, 60);
        }
    }

    #[test]
    fn both_slots_invalid_is_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CONTROL");
        ControlSlots::create(
            &crate::test_support::NoFlush,
            &path,
            sample(),
            Arc::default(),
        )
        .unwrap();
        corrupt_slot(&path, 0);
        corrupt_slot(&path, 1);
        let sink = RecordingSink::default();
        assert!(matches!(reopen(&path, &sink), Err(Error::Corruption(_))));
    }

    #[test]
    fn failed_sync_sets_the_latch() {
        use crate::storage::fault::FaultStorage;
        let dir = tempfile::tempdir().unwrap();
        let s = FaultStorage::new(Box::new(FsStorage));
        let health = Arc::new(Health::default());
        let mut cf =
            ControlSlots::create(&s, &dir.path().join("CONTROL"), sample(), health.clone())
                .unwrap();
        s.fail_sync_data(2);
        assert!(matches!(cf.write(sample()), Err(Error::Io(_))));
        assert!(health.is_unusable());
        assert!(matches!(cf.write(sample()), Err(Error::Unusable)));
    }
}
