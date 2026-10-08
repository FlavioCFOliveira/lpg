use std::path::Path;

use super::*;
use crate::storage::FsStorage;
use crate::storage::page::{self as pg, PageType};

/// A valid group found on disk.
#[derive(Debug, Clone)]
pub(crate) struct DiskGroup {
    pub(crate) start: u64,
    pub(crate) record: CommitRecord,
    pub(crate) pages: Vec<u64>,
}

/// The segments of generation `g` in `dir`, read through [`FsStorage`] (the
/// durable state when the database runs on a `FaultStorage`), by first LSN.
pub(crate) fn durable_segments(dir: &Path, g: u64) -> BTreeMap<u64, Vec<u8>> {
    let mut out = BTreeMap::new();
    for name in FsStorage.list(dir).unwrap() {
        if let Some((gen_, lsn)) = parse_segment_name(&name)
            && gen_ == g
        {
            let f = FsStorage.open(&dir.join(&name)).unwrap();
            let mut b = vec![0u8; f.len().unwrap() as usize];
            f.read_at(&mut b, 0).unwrap();
            out.insert(lsn, b);
        }
    }
    out
}

/// The frame at `lsn` in `segs`, if the bytes exist.
pub(crate) fn frame_at(segs: &BTreeMap<u64, Vec<u8>>, lsn: u64) -> Option<&[u8]> {
    let (&first, b) = segs.range(..=lsn).next_back()?;
    let off = (lsn - first) as usize;
    if lsn + FRAME_BYTES > first + SEGMENT_SPAN || off + FRAME_SIZE > b.len() {
        return None;
    }
    Some(&b[off..off + FRAME_SIZE])
}

fn sample_frame(lsn: u64, salt: u64, page: u64) -> Vec<u8> {
    let mut f = vec![0u8; FRAME_SIZE];
    pg::init(&mut f[FRAME_HEADER..], PageType::Leaf, 7);
    f[FRAME_HEADER + 100] = 0x5A;
    seal_frame(&mut f, KIND_PAGE, lsn, salt, page);
    f
}

#[test]
fn frame_geometry() {
    assert_eq!(FRAME_SIZE, 8224);
    assert_eq!(SEGMENT_FRAMES, 2040);
    assert_eq!(SEGMENT_SPAN, 0x00FF_FF00);
    const { assert!(SEGMENT_SPAN <= SEGMENT_SIZE) };
}

#[test]
fn segment_names_round_trip_and_reject_near_misses() {
    let n = segment_name(3, 0x00FF_FF00);
    assert_eq!(n, "wal-3-0000000000ffff00");
    assert_eq!(parse_segment_name(OsStr::new(&n)), Some((3, 0x00FF_FF00)));
    for bad in [
        "wal-3-0000000000FFFF00",
        "wal-03-0000000000ffff00",
        "wal-3-000000000ffff00",
        "wal-3-0000000000ffff00.tmp",
        "wal--0000000000ffff00",
        "data-3",
    ] {
        assert_eq!(parse_segment_name(OsStr::new(bad)), None, "{bad}");
    }
}

#[test]
fn frames_round_trip_and_every_bit_flip_or_wrong_position_is_rejected() {
    let f = sample_frame(5 * FRAME_BYTES, 9, 42);
    assert_eq!(
        decode_frame(&f, 5 * FRAME_BYTES),
        Some(FrameHead {
            kind: KIND_PAGE,
            salt: 9,
            page: 42
        })
    );
    // At another position the LSN does not match (TS-09 recycled frames).
    assert_eq!(decode_frame(&f, 6 * FRAME_BYTES), None);
    assert_eq!(decode_frame(&f[..FRAME_SIZE - 1], 5 * FRAME_BYTES), None);
    let bits = (0..FRAME_HEADER * 8).chain((FRAME_HEADER * 8..FRAME_SIZE * 8).step_by(67));
    for bit in bits {
        let mut g = f.clone();
        g[bit / 8] ^= 1 << (bit % 8);
        assert_eq!(decode_frame(&g, 5 * FRAME_BYTES), None, "bit {bit}");
    }
    // A commit frame carries its record.
    let mut c = vec![0u8; FRAME_SIZE];
    let r = CommitRecord {
        first_lsn: 3 * FRAME_BYTES,
        first_ts: 4,
        counters: Counters {
            commit_ts: 6,
            next_element_id: 7,
            next_catalog_id: 8,
            next_token_id: 9,
        },
    };
    put_commit(&mut c, &r);
    seal_frame(&mut c, KIND_COMMIT, 5 * FRAME_BYTES, 1, 0);
    assert_eq!(decode_frame(&c, 5 * FRAME_BYTES).unwrap().kind, KIND_COMMIT);
    assert_eq!(commit_record(&c), r);
}

#[test]
fn open_group_positions_rollback_and_close() {
    let mut g = OpenGroup::new(1, 1000 * FRAME_BYTES, Vec::new(), Counters::default());
    assert_eq!(g.page_lsn(7), 1001 * FRAME_BYTES);
    assert_eq!(g.page_lsn(9), 1002 * FRAME_BYTES);
    assert_eq!(
        g.page_lsn(7),
        1001 * FRAME_BYTES,
        "a page keeps its position"
    );
    let m = g.mark();
    assert_eq!(g.page_lsn(11), 1003 * FRAME_BYTES);
    g.rollback(m);
    assert_eq!(g.end(), 1003 * FRAME_BYTES);
    assert_eq!(
        g.page_lsn(12),
        1003 * FRAME_BYTES,
        "a rolled-back position is reused"
    );
    let mut img = vec![0u8; PAGE_SIZE];
    for (p, lsn) in [(7, 1001), (9, 1002), (12, 1003)] {
        pg::set_lsn(&mut img, lsn * FRAME_BYTES);
        g.capture(p, &img);
    }
    assert!(!g.has_commits());
    g.join(5, Counters::default());
    g.join(
        6,
        Counters {
            commit_ts: 6,
            ..Counters::default()
        },
    );
    let mut closed = g.close(Vec::new());
    assert_eq!((g.seq(), g.end()), (2, 1005 * FRAME_BYTES));
    closed.seal(77);
    assert_eq!(closed.frames.len(), 4 * FRAME_SIZE);
    for (i, f) in closed.frames.as_chunks::<FRAME_SIZE>().0.iter().enumerate() {
        let h = decode_frame(f, (1000 + i as u64) * FRAME_BYTES).expect("valid frame");
        assert_eq!(h.salt, 77);
        if i == 3 {
            let r = commit_record(f);
            assert_eq!(
                (r.first_lsn, r.first_ts, r.counters.commit_ts),
                (1000 * FRAME_BYTES, 5, 6)
            );
        } else {
            assert_eq!(h.page, [7, 9, 12][i]);
        }
    }
}

#[test]
fn counters_max_is_per_field() {
    let a = Counters {
        commit_ts: 5,
        next_element_id: 1,
        next_catalog_id: 9,
        next_token_id: 0,
    };
    let b = Counters {
        commit_ts: 2,
        next_element_id: 8,
        next_catalog_id: 3,
        next_token_id: 4,
    };
    assert_eq!(
        a.max(b),
        Counters {
            commit_ts: 5,
            next_element_id: 8,
            next_catalog_id: 9,
            next_token_id: 4
        }
    );
}

/// Reads the valid groups on disk incrementally, from LSN `from`, through
/// [`FsStorage`]: the durable state when the database runs on a
/// `FaultStorage`, which applies writes to the wrapped storage only in
/// `sync_data`. Valid while no segment is recycled.
pub(crate) struct DurableScan {
    dir: std::path::PathBuf,
    generation: u64,
    files: BTreeMap<u64, Box<dyn StorageFile>>,
    next: u64,
    min_salt: u64,
    pub(crate) groups: Vec<DiskGroup>,
}

impl DurableScan {
    pub(crate) fn new(dir: &Path, generation: u64, from: u64) -> DurableScan {
        DurableScan {
            dir: dir.to_path_buf(),
            generation,
            files: BTreeMap::new(),
            next: from,
            min_salt: 0,
            groups: Vec::new(),
        }
    }

    fn frame(&mut self, lsn: u64, buf: &mut [u8]) -> Option<FrameHead> {
        let holds = |files: &BTreeMap<u64, Box<dyn StorageFile>>| {
            files
                .range(..=lsn)
                .next_back()
                .is_some_and(|(&f, _)| lsn + FRAME_BYTES <= f + SEGMENT_SPAN)
        };
        if !holds(&self.files) {
            for name in FsStorage.list(&self.dir).unwrap() {
                if let Some((g, first)) = parse_segment_name(&name)
                    && g == self.generation
                    && !self.files.contains_key(&first)
                {
                    self.files
                        .insert(first, FsStorage.open(&self.dir.join(&name)).unwrap());
                }
            }
            if !holds(&self.files) {
                return None;
            }
        }
        let (&first, file) = self.files.range(..=lsn).next_back()?;
        if file.read_at(buf, lsn - first).ok()? < FRAME_SIZE {
            return None;
        }
        decode_frame(buf, lsn)
    }

    /// Reads every further valid group now on disk.
    pub(crate) fn advance(&mut self) {
        let mut buf = vec![0u8; FRAME_SIZE];
        loop {
            let x = self.next;
            let mut lsn = x;
            let mut pages = Vec::new();
            let mut salts = Vec::new();
            loop {
                let Some(h) = self.frame(lsn, &mut buf) else {
                    return;
                };
                if h.kind == KIND_PAGE {
                    pages.push(h.page);
                    salts.push(h.salt);
                    lsn += FRAME_BYTES;
                    continue;
                }
                let record = commit_record(&buf);
                if record.first_lsn != x
                    || h.salt < self.min_salt
                    || salts.iter().any(|&s| s != h.salt)
                {
                    return;
                }
                self.groups.push(DiskGroup {
                    start: x,
                    record,
                    pages,
                });
                self.min_salt = h.salt;
                self.next = lsn + FRAME_BYTES;
                break;
            }
        }
    }

    /// Whether a durable group holds commit timestamp `ts`.
    pub(crate) fn holds(&mut self, ts: u64) -> bool {
        let found = |g: &[DiskGroup]| {
            g.iter()
                .any(|g| g.record.first_ts <= ts && ts <= g.record.counters.commit_ts)
        };
        if found(&self.groups) {
            return true;
        }
        self.advance();
        found(&self.groups)
    }
}

/// `loom` model of the commit latch and publish order (TS-30, TS-16 steps
/// 3, 7, 8 and 9): two committers draw timestamps and join groups under a
/// latch, as [`crate::tx::commit`] does; the writer closes each group under
/// the latch and marks it durable through the real [`Progress`]; the main
/// thread reads the published timestamp. In every interleaving, a committer
/// returns only once its timestamp is published, a published timestamp
/// never exceeds the durable counters, and published timestamps never
/// decrease.
#[cfg(loom)]
const PREEMPTIONS: usize = 3;

#[cfg(loom)]
#[test]
fn loom_published_timestamps_increase_and_never_exceed_the_durable_ones() {
    use crate::sync::{Arc, Mutex};
    // Four threads over two mutexes and two condition variables: the
    // preemptions are bounded, as loom's documentation advises for models
    // of this size (`loom` 0.7.2 `src/model.rs`, `preemption_bound`).
    let mut b = loom::model::Builder::new();
    b.preemption_bound = Some(PREEMPTIONS);
    b.check(|| {
        let progress = Arc::new(Progress::new(
            Arc::new(AtomicU64::new(0)),
            Counters::default(),
        ));
        // The commit latch over the counter and the open group: last
        // timestamp drawn, open group number, its commits.
        let latch = Arc::new(Mutex::new((0u64, 1u64, 0u64)));
        let committers: Vec<_> = (0..2)
            .map(|_| {
                let (p, latch) = (Arc::clone(&progress), Arc::clone(&latch));
                loom::thread::spawn(move || {
                    let mut l = latch.lock().unwrap();
                    l.0 += 1;
                    l.2 += 1;
                    let (ts, seq) = (l.0, l.1);
                    p.note_join(seq);
                    drop(l);
                    p.wait_durable(seq).unwrap();
                    assert!(p.published() >= ts, "returned before publication");
                })
            })
            .collect();
        let writer = {
            let (p, latch) = (Arc::clone(&progress), Arc::clone(&latch));
            loom::thread::spawn(move || {
                while p.wait_for_group(Duration::ZERO) != Work::Stop {
                    let (seq, last, commits) = {
                        let mut l = latch.lock().unwrap();
                        let g = (l.1, l.0, l.2);
                        l.1 += 1;
                        l.2 = 0;
                        g
                    };
                    assert!(commits > 0, "a group is closed only after a join");
                    let counters = Counters {
                        commit_ts: last,
                        ..Counters::default()
                    };
                    p.group_durable(seq, seq * FRAME_BYTES, counters, commits);
                }
            })
        };
        let a = progress.published();
        let (_, durable) = progress.durable_point();
        assert!(
            durable.commit_ts >= a,
            "published {a} above durable {durable:?}"
        );
        let b = progress.published();
        assert!(b >= a);
        for c in committers {
            c.join().unwrap();
        }
        progress.stop();
        writer.join().unwrap();
        assert_eq!(progress.published(), 2);
    });
}
