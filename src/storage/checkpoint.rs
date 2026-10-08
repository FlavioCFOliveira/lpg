//! Checkpoints (TS-11): steps 1–5, the triggers and the thread
//! `lpg-checkpoint`.
//!
//! 0. **Statistics first.** The statistics are written to the statistics
//!    tree as an internal batch built under the commit latch, and the
//!    checkpoint waits until its group is durable, so its images lie below
//!    the redo point ([`crate::catalog::stats::save`]). Crash safety alone
//!    would not need the wait (step 2 writes the batch's pages once durable,
//!    recovery replays every group from R, and open recounts statistics
//!    older than the recovered commit); the wait keeps the redo point of a
//!    clean close at the WAL end, so the next open replays nothing and a
//!    migration may remove the WAL (TS-28). A batch that fails with the
//!    database usable (ENOSPC, a refused memory reservation) is undone and
//!    the checkpoint goes on: the statistics tree keeps an older timestamp,
//!    which makes the open after a crash recount. The checkpoint at open
//!    skips this step: the WAL writer does not run yet, and the statistics
//!    were just loaded or recounted.
//!
//! 1. The redo point R is the durable LSN, read with the counters of the
//!    same durable group: every group below R is durable.
//! 2. Every dirty cache slot whose page LSN is durable is written. A slot
//!    whose page LSN is not yet durable has its latest image in a group at
//!    or after R, and that image holds every earlier change of the page; the
//!    checkpoint waits until the durable LSN reaches it instead of writing
//!    it, since recovery from R replays that group. PostgreSQL's
//!    `FlushBuffer` instead forces the WAL up to the page LSN and writes the
//!    page (R-PG `src/backend/storage/buffer/bufmgr.c:4565-4585`); here
//!    only the WAL writer syncs the WAL, and a page that a commit keeps
//!    changing is then never waited for twice. After the wait the skipped
//!    slots are visited again and written if their page LSN is now
//!    durable. An undone commit gives its frame positions back and puts
//!    pages back to older images, so the visits repeat until one wait and
//!    one visit see no undo.
//! 3. `sync_data` of the data file.
//! 4. The next `CONTROL` slot: R, the counters and the clean-shutdown flag.
//! 5. Segments below R are recycled or removed ([`Wal::recycle`]).
//!
//! Each checkpoint also sends the buffer pool's hit and miss counts to the
//! metrics sink ([`super::pool::Pool::publish_metrics`]).
//!
//! Checkpoints run one at a time. Triggers: every 15 minutes or every
//! 100 000 commits, whichever comes first (the thread); at clean close and
//! at open (the engine); on request ([`super::engine::Engine::checkpoint`]);
//! at the start of a backup ([`run_held`]).
//!
//! The thread `lpg-checkpoint` also creates WAL segments ahead of use when
//! the WAL writer asks for it ([`Checkpoints::request_precreate`]), so the
//! zero fill of a new segment never runs on the writer thread (task #15,
//! D4).
//!
//! [`Wal::recycle`]: super::wal::Wal::recycle

use crate::sync::atomic::{AtomicU64, Ordering};
use crate::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::control::Control;
use super::engine::{Core, Tuning};
use super::error::Result;
use super::page::PAGE_BYTES;
use crate::obs::names::checkpointing::{
    DURATION, EVENTS, FLUSHED_BYTES, PAGES_WRITTEN, SEGMENTS_RECYCLED, SEGMENTS_REMOVED,
};

/// Figures of one checkpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Stats {
    /// The redo point recorded.
    pub(crate) redo: u64,
    /// Pages written by step 2.
    pub(crate) pages_written: u64,
    /// Slots whose page LSN step 2 waited for instead of writing them.
    pub(crate) pages_waited: u64,
    /// Segments renamed by step 5.
    pub(crate) recycled: u64,
    /// Segments removed by step 5.
    pub(crate) removed: u64,
    /// Wall time.
    pub(crate) duration: Duration,
}

#[derive(Default)]
struct Trigger {
    stop: bool,
    /// Set by the WAL writer: create the next segment ahead of use.
    precreate: bool,
}

/// The checkpoint state of one database.
pub(crate) struct Checkpoints {
    /// Held while a checkpoint runs; the redo point of the last one.
    run: Mutex<u64>,
    trigger: Mutex<Trigger>,
    cv: Condvar,
    /// Commits since the last checkpoint started.
    commits: AtomicU64,
    tuning: Tuning,
}

impl Checkpoints {
    /// The state after open, whose `CONTROL` holds redo point `redo`.
    pub(crate) fn new(redo: u64, tuning: Tuning) -> Checkpoints {
        Checkpoints {
            run: Mutex::new(redo),
            trigger: Mutex::new(Trigger::default()),
            cv: Condvar::new(),
            commits: AtomicU64::new(0),
            tuning,
        }
    }

    fn trigger(&self) -> MutexGuard<'_, Trigger> {
        self.trigger.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts one commit; the commit that reaches the trigger count wakes
    /// the thread.
    pub(crate) fn note_commit(&self) {
        if self.commits.fetch_add(1, Ordering::Relaxed) + 1 == self.tuning.checkpoint_commits {
            let _t = self.trigger();
            self.cv.notify_one();
        }
    }

    /// Asks the thread to create the next WAL segment ahead of use
    /// ([`super::wal::Wal::precreate`]).
    pub(crate) fn request_precreate(&self) {
        self.trigger().precreate = true;
        self.cv.notify_one();
    }

    /// Asks the thread to stop.
    pub(crate) fn stop(&self) {
        self.trigger().stop = true;
        self.cv.notify_all();
    }
}

/// Runs one checkpoint (TS-11 steps 1–5). With `clean`, its `CONTROL` slot
/// records a clean shutdown; only clean close passes it.
///
/// # Errors
///
/// [`super::error::Error::Unusable`]; an I/O error, which sets the unusable
/// latch; the WAL writer's failure while step 2 waits for it.
pub(crate) fn run(core: &Core, clean: bool) -> Result<Stats> {
    run_with(core, clean, false, true)
}

/// The checkpoint of TS-13 step 8, before the WAL writer runs: without the
/// statistics batch (step 0).
///
/// # Errors
///
/// As [`run`].
pub(crate) fn run_open(core: &Core) -> Result<Stats> {
    run_with(core, false, false, false)
}

/// Runs one checkpoint at the start of a backup (TS-11 trigger, TS-26
/// backup step 1) and holds WAL recycling at its redo point R: the hold is
/// placed before step 5, under the checkpoint latch, so no checkpoint
/// recycles a segment that holds an LSN at or above R until
/// [`super::wal::Wal::release`]. Returns R in [`Stats::redo`].
///
/// # Errors
///
/// As [`run`]; no hold is left after an error.
pub(crate) fn run_held(core: &Core) -> Result<Stats> {
    run_with(core, false, true, true)
}

fn run_with(core: &Core, clean: bool, hold: bool, statistics: bool) -> Result<Stats> {
    let mut prev_redo = core
        .checkpoints
        .run
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    core.health.check()?;
    let started = Instant::now();
    core.checkpoints.commits.store(0, Ordering::Relaxed);
    // Step 0.
    if statistics {
        match crate::catalog::stats::save(core) {
            Ok(Some(seq)) => core.wal.wait_durable(seq)?,
            Ok(None) => {}
            Err(_) if !core.health.is_unusable() => {}
            Err(e) => return Err(e),
        }
    }
    // Step 1.
    let (redo, counters) = core.wal.durable_point();
    // Step 2. Every slot dirty at the start is written, as PostgreSQL's
    // `BufferSync` writes every buffer dirty when the checkpoint began
    // (R-PG `src/backend/storage/buffer/bufmgr.c:3598-3640`), except a slot
    // whose page LSN is not durable: its latest image is in a group at or
    // after R. The skipped slots are visited again once their groups are
    // durable, and written if their page LSN now is. A slot whose page LSN
    // rose meanwhile holds a later change; its image at the LSN it was
    // skipped with is durable after R. An undo puts pages back to older
    // images and page LSNs, which may be at or below R: so the undo epoch
    // is read before each pass, and the loop ends only after a wait and a
    // revisit during which no commit was undone.
    let mut epoch = core.wal.undo_epoch();
    let mut skipped = Vec::new();
    let mut written = core.pool.checkpoint_pass(None, &mut skipped)?;
    let waited = skipped.len() as u64;
    while let Some(target) = skipped.iter().map(|&(_, lsn)| lsn).max() {
        #[cfg(test)]
        super::wal::TestPoints::run(&core.wal.points.checkpoint_revisit);
        let undone = !core.wal.wait_lsn(target, epoch)?;
        epoch = core.wal.undo_epoch();
        let pages = std::mem::take(&mut skipped);
        written += core.pool.checkpoint_pass(Some(&pages), &mut skipped)?;
        if !undone && core.wal.undo_epoch() == epoch {
            break;
        }
    }
    // Step 3.
    core.data.sync()?;
    // Step 4.
    {
        let mut control = core.control();
        let c = *control.current();
        control.write(Control {
            clean_shutdown: clean,
            redo_point: redo,
            commit_ts: c.commit_ts.max(counters.commit_ts),
            next_element_id: c.next_element_id.max(counters.next_element_id),
            next_catalog_id: c.next_catalog_id.max(counters.next_catalog_id),
            next_token_id: c.next_token_id.max(counters.next_token_id),
            ..c
        })?;
    }
    // Step 5.
    if hold {
        core.wal.hold(redo);
    }
    let used = redo - *prev_redo;
    let recycled = match core.wal.recycle(redo, core.wal.durable_lsn(), used) {
        Ok(r) => r,
        Err(e) => {
            if hold {
                core.wal.release(redo);
            }
            return Err(e);
        }
    };
    *prev_redo = redo;
    let stats = Stats {
        redo,
        pages_written: written,
        pages_waited: waited,
        recycled: recycled.renamed,
        removed: recycled.removed,
        duration: started.elapsed(),
    };
    let sink = &core.sink;
    sink.counter(EVENTS, 1);
    sink.histogram(DURATION, stats.duration.as_nanos() as u64);
    sink.counter(PAGES_WRITTEN, written);
    sink.counter(FLUSHED_BYTES, written * PAGE_BYTES);
    sink.counter(SEGMENTS_RECYCLED, recycled.renamed);
    sink.counter(SEGMENTS_REMOVED, recycled.removed);
    // The pool counts hits and misses without a sink call per access; each
    // checkpoint sends what it counted since the last one (FR-OBS-001), as
    // PostgreSQL's checkpointer reports its pending statistics on each
    // cycle (R-PG `src/backend/postmaster/checkpointer.c:575-577`).
    core.pool.publish_metrics();
    Ok(stats)
}

/// The loop of thread `lpg-checkpoint`: a checkpoint every
/// `checkpoint_interval`, or as soon as `checkpoint_commits` commits have
/// been counted, until [`Checkpoints::stop`]. After a failed checkpoint
/// the database is unusable, and the thread only waits for the stop.
pub(crate) fn run_thread(core: &Core) {
    let cp = &core.checkpoints;
    let mut last = Instant::now();
    let mut broken = false;
    loop {
        {
            let mut t = cp.trigger();
            loop {
                if t.stop {
                    return;
                }
                if t.precreate && !broken {
                    t.precreate = false;
                    drop(t);
                    core.wal.precreate();
                    t = cp.trigger();
                    continue;
                }
                if !broken {
                    if cp.commits.load(Ordering::Relaxed) >= cp.tuning.checkpoint_commits {
                        break;
                    }
                    let elapsed = last.elapsed();
                    if elapsed >= cp.tuning.checkpoint_interval {
                        break;
                    }
                    t = cp
                        .cv
                        .wait_timeout(t, cp.tuning.checkpoint_interval - elapsed)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                } else {
                    t = cp.cv.wait(t).unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
        broken = run(core, false).is_err();
        last = Instant::now();
    }
}

#[cfg(test)]
mod tests;
