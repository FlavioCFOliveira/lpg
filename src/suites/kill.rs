//! The `kill -9` suite (TS-30 Process crash, rmp #21).
//!
//! The parent starts a child through the re-exec helper
//! ([`crate::test_support::child`]). The child opens the database on
//! [`FsStorage`] (creating it the first time) and runs four committing
//! threads; each commit puts two keys far apart and, after the commit
//! returns, the child prints its identifier on a line, line-buffered, on a
//! pipe. Two more threads keep busy the paths that a commit workload with
//! the default triggers never reaches in a short run (audit finding 8,
//! 2026-10-03): one runs checkpoints back to back, each of which recycles
//! or removes the segments below its redo point, and one runs online
//! backups back to back into fresh targets. Each prints a line when a
//! checkpoint or a backup starts and when it ends, so the parent knows
//! which of them the kill interrupted. After a seeded random delay the
//! parent sends SIGKILL
//! ([`std::process::Child::kill`]), reopens the database and checks it:
//!
//! - every printed commit is present, both of its keys;
//! - a commit the child started but did not print is present whole or
//!   absent (FR-REC-001: the commit point is the group's sync, TS-16), and
//!   each thread's present commits form a prefix of its sequence that ends
//!   at most one past its last printed commit;
//! - the integrity check passes.
//!
//! A fifth thread runs serializable transactions on a second tree (audit
//! Round A): each puts its own key and deletes the previous one, so each
//! commit is stamped and leaves a delete stamp; the first and every 16th
//! after it also creates a
//! tree, fills it and drops it in a second transaction. Commits notify the
//! garbage collector, whose groups remove the stamps and free the dropped
//! trees. It prints `TX` after each commit and `DROP` after each drop.
//! After the reopen, a snapshot sees exactly one key of the thread for the
//! iteration, the last printed or the one after it, every printed drop is
//! dropped, and once the collector's seed scan and pass have freed the
//! dropped trees, no entry keeps a delete stamp at or below the horizon of
//! a pass with no snapshot open, and the integrity check passes again.
//!
//! The parent counts the kills that fell inside a checkpoint and inside a
//! backup, and the checkpoints that recycled or removed a segment; the full
//! run requires each count, and the printed drops, to be above 0. In a
//! full run the first child, before its workload, fills the WAL past its
//! first segment and checkpoints, so at least one checkpoint recycles
//! whatever the file system's speed ([`fill_one_segment`]).
//!
//! The parent then closes the database, and the next iteration's child
//! opens it again. The suite runs 200 iterations in each lab directory
//! (`LPG_LAB_EXT4`, `LPG_LAB_XFS`), without privileges, and prints for
//! each directory the iterations, the printed commits, the recovery time
//! per reopen and the WAL bytes replayed (TS-13), and the child's commits
//! per second.

use crate::sync::Arc;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::suite_dirs;
use crate::obs::recording::RecordingSink;
use crate::storage::btree::{Tree, Write};
use crate::storage::engine::{CONTROL, Engine};
use crate::storage::{FsStorage, Storage};
use crate::test_support::child;
use crate::test_support::seed;
use crate::tx::Isolation;
use crate::tx::commit::Ids;

const CAP: u64 = 64 << 20;
const THREADS: u64 = 4;
const DIR_VAR: &str = "LPG_KILL_DIR";
const BACKUP_VAR: &str = "LPG_KILL_BACKUP_DIR";
const ITER_VAR: &str = "LPG_KILL_ITER";
/// Set for the first child of a full run: see [`fill_one_segment`].
const FILL_VAR: &str = "LPG_KILL_FILL";

/// The two keys of commit `i` of thread `t` in iteration `iter`.
fn keys(iter: u64, t: u64, i: u64) -> [Vec<u8>; 2] {
    [
        format!("a/{iter:03}/{t}/{i:07}").into_bytes(),
        format!("z/{iter:03}/{t}/{i:07}").into_bytes(),
    ]
}

/// The key of transaction `i` of the transaction thread in iteration
/// `iter`.
fn tx_key(iter: u64, i: u64) -> Vec<u8> {
    format!("x/{iter:03}/{i:07}").into_bytes()
}

/// The transaction thread: see the module documentation.
fn tx_work(e: &Engine, tree: Tree, iter: u64, say: &(dyn Fn(String) + Sync)) {
    for i in 0u64..u64::MAX {
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.put(tree, &tx_key(iter, i), &[9u8; 200]).unwrap();
        if i > 0 {
            assert!(tx.delete(tree, &tx_key(iter, i - 1)).unwrap());
        }
        tx.commit().unwrap();
        say(format!("TX {iter} {i}"));
        if i % 16 == 0 {
            let t = e.create_trees(1).unwrap()[0];
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            for k in 0..60u32 {
                tx.put(t, &k.to_be_bytes(), &[3u8; 300]).unwrap();
            }
            tx.commit().unwrap();
            let mut tx = e.begin(Isolation::Serializable).unwrap();
            tx.drop_tree(t).unwrap();
            tx.commit().unwrap();
            say(format!("DROP {}", t.id()));
        }
    }
}

/// Commits filler entries (prefix `f/`, outside every oracle's prefix)
/// into `tree` until the durable WAL passes its first segment, then runs a
/// checkpoint, printed like the checkpoint thread's: its redo point lies in
/// the second segment, and no backup holds the first, so it recycles or
/// removes it. A full run's workload alone writes a few segments' worth on
/// a slow file system, so whether a printed checkpoint recycled depended
/// on timing (audit Round A N2: 0, 3 and 1 on XFS); this makes it certain,
/// and the later children write over the recycled segment.
///
/// # Errors
///
/// The failure's message, which the child prints for the parent to report.
fn fill_one_segment(e: &Engine, tree: Tree, say: &(dyn Fn(String) + Sync)) -> Result<(), String> {
    let v = [5u8; 2000];
    let mut n = 0u64;
    while e.core().wal.durable_lsn() <= crate::storage::wal::SEGMENT_SPAN {
        let keys: Vec<Vec<u8>> = (0..64)
            .map(|k| format!("f/{n:06}/{k:02}").into_bytes())
            .collect();
        let batch: Vec<Write<'_>> = keys
            .iter()
            .map(|key| Write {
                tree,
                key,
                value: Some(&v),
                stamp: 0,
            })
            .collect();
        e.commit(&batch, Ids::default())
            .map_err(|err| format!("filler commit {n}: {err:?}"))?;
        n += 1;
    }
    say("CKPT start".into());
    let st = e
        .checkpoint()
        .map_err(|err| format!("filler checkpoint: {err:?}"))?;
    if st.recycled + st.removed == 0 {
        return Err("the filled segment was not recycled".into());
    }
    say(format!("CKPT end {}", st.recycled + st.removed));
    Ok(())
}

/// Parses a key back into `(iter, t, i)`.
fn parse(k: &[u8]) -> (u64, u64, u64) {
    let s = std::str::from_utf8(&k[2..]).unwrap();
    let mut p = s.split('/').map(|x| x.parse::<u64>().unwrap());
    (p.next().unwrap(), p.next().unwrap(), p.next().unwrap())
}

/// The child: commits from four threads until it is killed.
fn work() {
    let dir = std::env::var_os(DIR_VAR).expect("directory");
    let dir = Path::new(&dir);
    let iter: u64 = std::env::var(ITER_VAR).unwrap().parse().unwrap();
    let sink = Arc::new(RecordingSink::default());
    // In `/tmp/lpgram`, a `tmpfs`, `fsync` reaches no disk and `fallocate`
    // takes memory, so the child uses the test storage that skips both
    // ([`crate::test_support::NoFlush`]); a process crash keeps the page
    // cache either way. In the lab directories it uses `FsStorage`.
    let storage: Arc<dyn Storage> = if dir.starts_with(crate::test_support::GRAPH_DIR) {
        crate::test_support::no_flush()
    } else {
        Arc::new(FsStorage)
    };
    let e = if FsStorage.list(dir).unwrap().iter().any(|n| n == CONTROL) {
        Engine::open(dir, storage, CAP, sink).unwrap()
    } else {
        Engine::create(dir, storage, CAP, sink).unwrap()
    };
    // Tree 1 for the batches, tree 2 for the transactions.
    let (tree, tx_tree) = match e.store().tree(1).unwrap() {
        Some(t) => (t, e.store().tree(2).unwrap().expect("tree 2")),
        None => {
            let t = e.create_trees(2).unwrap();
            (t[0], t[1])
        }
    };
    let out = crate::sync::Mutex::new(std::io::stdout());
    let say = |line: String| {
        let mut o = out.lock().unwrap();
        writeln!(o, "{line}").unwrap();
        o.flush().unwrap();
    };
    let backups = std::env::var_os(BACKUP_VAR).expect("backup directory");
    let backups = Path::new(&backups);
    if std::env::var_os(FILL_VAR).is_some()
        && let Err(m) = fill_one_segment(&e, tx_tree, &say)
    {
        say(format!("FILL failed: {m}"));
        panic!("{m}");
    }
    std::thread::scope(|s| {
        let (e, say) = (&e, &say);
        s.spawn(move || {
            loop {
                say("CKPT start".into());
                let st = e.checkpoint().unwrap();
                say(format!("CKPT end {}", st.recycled + st.removed));
            }
        });
        s.spawn(move || {
            for n in 0u64..u64::MAX {
                // A fresh, empty target each time; the previous artefact is
                // removed, so the run stays small.
                let target = backups.join(format!("{iter}-{n}"));
                std::fs::create_dir(&target).unwrap();
                say("BACKUP start".into());
                e.backup(&target).unwrap();
                say("BACKUP end".into());
                std::fs::remove_dir_all(&target).unwrap();
            }
        });
        s.spawn(move || tx_work(e, tx_tree, iter, say));
        for t in 0..THREADS {
            s.spawn(move || {
                for i in 0u64..u64::MAX {
                    let [a, b] = keys(iter, t, i);
                    let v = [t as u8; 120];
                    let batch = [
                        Write {
                            tree,
                            key: &a,
                            value: Some(&v),
                            stamp: 0,
                        },
                        Write {
                            tree,
                            key: &b,
                            value: Some(&v),
                            stamp: 0,
                        },
                    ];
                    e.commit(&batch, Ids::default()).unwrap();
                    say(format!("COMMIT {iter} {t} {i}"));
                }
            });
        }
    });
}

/// Figures of one directory.
#[derive(Default)]
struct Figures {
    printed: u64,
    recovery_ms: Vec<f64>,
    replayed: Vec<u64>,
    commits_per_s: Vec<f64>,
    /// Kills that fell between a checkpoint's start and its end.
    in_checkpoint: u64,
    /// Kills that fell between a backup's start and its end.
    in_backup: u64,
    /// Completed checkpoints that recycled or removed a segment.
    recycling: u64,
    /// Trees whose drop was printed.
    drops: Vec<u64>,
}

/// One line of the child.
enum Line {
    Commit(u64, u64),
    CheckpointStart,
    CheckpointEnd {
        recycled: u64,
    },
    BackupStart,
    BackupEnd,
    /// Transaction `i` of the transaction thread returned.
    Tx(u64),
    /// The tree with this identifier was dropped.
    Drop(u64),
    /// The filler step failed with this message.
    FillFailed(String),
}

/// One kill-and-reopen iteration in `dir`. Adds the commits printed to
/// `printed`, keyed `(iter, t)`, and checks the reopened database.
fn iteration(
    dir: &Path,
    backups: &Path,
    iter: u64,
    delay: Duration,
    printed: &mut BTreeMap<(u64, u64), u64>,
    tree: &mut Option<Tree>,
    fig: &mut Figures,
) {
    let test = child::test_path(module_path!(), "kill_and_reopen");
    let iter_s = iter.to_string();
    let mut env = vec![
        (DIR_VAR, dir.as_os_str()),
        (BACKUP_VAR, backups.as_os_str()),
        (ITER_VAR, OsStr::new(&iter_s)),
    ];
    // The first child of a full run fills a segment first.
    if iter == 0 && suite_dirs().1 {
        env.push((FILL_VAR, OsStr::new("1")));
    }
    let mut c = child::spawn(&test, "work", &env);
    let stdout = c.stdout.take().unwrap();
    let (tx, rx) = crate::sync::mpsc::channel::<Line>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            // libtest prints `test <name> ... ` without a newline before
            // the test runs, so the first commit line follows it.
            let msg = if let Some(at) = line.find("COMMIT ") {
                let rest = &line[at + "COMMIT ".len()..];
                let v: Vec<u64> = rest.split(' ').map(|x| x.parse().unwrap()).collect();
                assert_eq!(v[0], iter);
                Line::Commit(v[1], v[2])
            } else if line.ends_with("CKPT start") {
                Line::CheckpointStart
            } else if let Some(at) = line.find("CKPT end ") {
                Line::CheckpointEnd {
                    recycled: line[at + "CKPT end ".len()..].parse().unwrap(),
                }
            } else if line.ends_with("BACKUP start") {
                Line::BackupStart
            } else if line.ends_with("BACKUP end") {
                Line::BackupEnd
            } else if let Some(at) = line.find("TX ") {
                let v: Vec<u64> = line[at + 3..]
                    .split(' ')
                    .map(|x| x.parse().unwrap())
                    .collect();
                assert_eq!(v[0], iter);
                Line::Tx(v[1])
            } else if let Some(at) = line.find("FILL failed: ") {
                Line::FillFailed(line[at + "FILL failed: ".len()..].to_owned())
            } else if let Some(at) = line.find("DROP ") {
                Line::Drop(line[at + 5..].parse().unwrap())
            } else {
                continue;
            };
            let _ = tx.send(msg);
        }
    });
    // The delay counts from the first printed commit, so that the kill
    // falls in the middle of the workload.
    let mut lines = Vec::new();
    loop {
        let l = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("the child commits");
        if let Line::FillFailed(m) = l {
            panic!("the child's filler step failed: {m}");
        }
        let commit = matches!(l, Line::Commit(..));
        lines.push(l);
        if commit {
            break;
        }
    }
    let started = Instant::now();
    std::thread::sleep(delay);
    c.kill().unwrap();
    c.wait().unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    reader.join().unwrap();
    lines.extend(rx.try_iter());
    let mut n = 0;
    let mut last = BTreeMap::new();
    let mut last_tx = None;
    let (mut checkpointing, mut backing_up) = (false, false);
    for l in lines {
        match l {
            Line::Commit(t, i) => {
                n += 1;
                let e = last.entry(t).or_insert(i);
                *e = (*e).max(i);
            }
            Line::CheckpointStart => checkpointing = true,
            Line::CheckpointEnd { recycled } => {
                checkpointing = false;
                fig.recycling += u64::from(recycled > 0);
            }
            Line::BackupStart => backing_up = true,
            Line::BackupEnd => backing_up = false,
            Line::Tx(i) => last_tx = Some(i),
            Line::Drop(id) => fig.drops.push(id),
            Line::FillFailed(m) => panic!("the child's filler step failed: {m}"),
        }
    }
    fig.in_checkpoint += u64::from(checkpointing);
    fig.in_backup += u64::from(backing_up);
    fig.printed += n;
    fig.commits_per_s.push(n as f64 / elapsed);
    for (t, i) in last {
        printed.insert((iter, t), i);
    }
    // Reopen and check.
    let t0 = Instant::now();
    let e = Engine::open(
        dir,
        Arc::new(FsStorage),
        CAP,
        Arc::new(RecordingSink::default()),
    )
    .unwrap_or_else(|e| panic!("iteration {iter}: reopen: {e}"));
    fig.recovery_ms.push(t0.elapsed().as_secs_f64() * 1e3);
    fig.replayed.push(e.recovered().bytes);
    let t = *tree.get_or_insert_with(|| e.store().tree(1).unwrap().expect("the tree exists"));
    let mut present: BTreeMap<(u64, u64), BTreeSet<u64>> = BTreeMap::new();
    let mut halves: BTreeMap<(u64, u64, u64), u8> = BTreeMap::new();
    e.store()
        .scan_prefix(t, b"", |k, _| {
            let (it, th, i) = parse(k);
            *halves.entry((it, th, i)).or_default() += 1;
            present.entry((it, th)).or_default().insert(i);
            true
        })
        .unwrap();
    for (&(it, th, i), &n) in &halves {
        assert_eq!(
            n, 2,
            "iteration {iter}: commit {it}/{th}/{i} is present in part"
        );
    }
    for (&(it, th), &last) in printed.iter() {
        let got = present.get(&(it, th)).map_or(0, |s| s.len() as u64);
        let max = present.get(&(it, th)).and_then(|s| s.last().copied());
        // Commits 0..=last were printed: all present, as a prefix, with at
        // most the one in flight after them.
        assert!(
            got > last && got <= last + 2 && max == Some(got - 1),
            "iteration {iter}: thread {th} of iteration {it}: {got} commits present, \
             last printed {last}"
        );
    }
    // A thread that printed nothing may have its first commit in flight.
    for (&(it, th), s) in &present {
        if !printed.contains_key(&(it, th)) {
            assert!(
                s.len() == 1 && s.contains(&0),
                "iteration {iter}: unprinted commits {s:?} of thread {th} of iteration {it}"
            );
        }
    }
    let r = e.store().check().unwrap();
    assert!(r.is_ok(), "iteration {iter}: {:?}", r.problems);
    check_transactions(&e, iter, last_tx, &fig.drops);
    e.close().unwrap();
}

/// The transaction thread's oracle after the reopen (module
/// documentation).
fn check_transactions(e: &Engine, iter: u64, last: Option<u64>, drops: &[u64]) {
    let t = e.store().tree(2).unwrap().expect("tree 2");
    let mut tx = e.begin(Isolation::Snapshot).unwrap();
    let mut seen = Vec::new();
    let prefix = format!("x/{iter:03}/").into_bytes();
    tx.scan_prefix(t, &prefix, |k, _| {
        let i = std::str::from_utf8(&k[prefix.len()..]).unwrap();
        seen.push(i.parse::<u64>().unwrap());
        true
    })
    .unwrap();
    drop(tx);
    let ok = match last {
        Some(l) => seen == [l] || seen == [l + 1],
        None => seen.is_empty() || seen == [0],
    };
    assert!(
        ok,
        "iteration {iter}: transaction keys {seen:?}, last printed {last:?}"
    );
    for &id in drops {
        let st = e.store().tree_status(id).unwrap();
        assert!(
            st.is_none_or(|(_, d)| d != 0),
            "iteration {iter}: printed drop of tree {id} lost"
        );
    }
    // The open recovered: the collector's seed scan and pass free the
    // dropped trees and remove the delete-stamped entries; then the
    // integrity check passes again.
    crate::test_support::wait_until(
        &format!("iteration {iter}: the dropped trees are freed"),
        || {
            drops
                .iter()
                .all(|&id| e.store().tree_status(id).unwrap().is_none())
        },
    );
    super::assert_stamps_collected(e, &format!("iteration {iter}"));
    let r = e.store().check().unwrap();
    assert!(
        r.is_ok(),
        "iteration {iter} after collection: {:?}",
        r.problems
    );
}

/// rmp #21: a process killed with SIGKILL during a commit workload reopens
/// with every commit that returned, on ext4 and XFS (NFR-PLAT-004).
#[test]
fn kill_and_reopen() {
    if child::role().as_deref() == Some("work") {
        work();
        return;
    }
    let (dirs, full) = suite_dirs();
    let iterations = if full { 200 } else { 3 };
    for (fs, base) in dirs {
        let started = Instant::now();
        let dir = tempfile::tempdir_in(&base).unwrap();
        let backups = tempfile::tempdir_in(&base).unwrap();
        let sd = seed();
        let mut rng = StdRng::seed_from_u64(sd);
        let mut printed = BTreeMap::new();
        let mut tree = None;
        let mut fig = Figures::default();
        for iter in 0..iterations {
            let delay = Duration::from_millis(rng.random_range(5..150));
            iteration(
                dir.path(),
                backups.path(),
                iter,
                delay,
                &mut printed,
                &mut tree,
                &mut fig,
            );
        }
        let pct = |v: &[f64], q: f64| {
            let mut s = v.to_vec();
            s.sort_by(f64::total_cmp);
            s[((q * s.len() as f64).ceil() as usize).clamp(1, s.len()) - 1]
        };
        let replayed: Vec<f64> = fig.replayed.iter().map(|&b| b as f64).collect();
        println!(
            "SUITE kill fs={fs} dir={} iterations={iterations} printed_commits={} \
             recovery_ms_p50={:.1} recovery_ms_p99={:.1} replayed_bytes_p50={:.0} \
             replayed_bytes_p99={:.0} child_commits_per_s_p50={:.0} \
             kills_in_checkpoint={} kills_in_backup={} recycling_checkpoints={} \
             printed_drops={} seconds={:.1}",
            base.display(),
            fig.printed,
            pct(&fig.recovery_ms, 0.5),
            pct(&fig.recovery_ms, 0.99),
            pct(&replayed, 0.5),
            pct(&replayed, 0.99),
            pct(&fig.commits_per_s, 0.5),
            fig.in_checkpoint,
            fig.in_backup,
            fig.recycling,
            fig.drops.len(),
            started.elapsed().as_secs_f64(),
        );
        if full {
            assert!(
                fig.in_checkpoint > 0
                    && fig.in_backup > 0
                    && fig.recycling > 0
                    && !fig.drops.is_empty(),
                "{fs}: no kill fell in a checkpoint or in a backup, no checkpoint recycled, \
                 or no drop was printed"
            );
        }
    }
}
