//! Transaction histories for Elle (TS-30 Isolation, R12).
//!
//! A [`History`] records, from many threads, the transactions a test runs
//! against lpg: one `invoke` event when a transaction starts, then one `ok`
//! event with the values it read when it commits, or one `fail` event when
//! it aborts. Operations follow Elle's two models: list-append (append an
//! element to a list, read a whole list) and rw-register (write an integer,
//! read an integer or nil). [`History::to_edn`] renders the events as EDN,
//! one map per line, the format `elle-cli` reads.
//!
//! [`elle`] writes a history under `/tmp/lpg-histories/` (a tool
//! intermediate, not graph data, so never in `/tmp/lpgram`) and checks it
//! with `elle-cli`, an external tool run as `java -jar "$LPG_ELLE_CLI"`
//! (TS-30: Elle runs outside the crate).
//!
//! **Origin.** The event and operation types and their EDN rendering follow
//! Turso's `testing/concurrent-simulator/elle.rs` (R-TURSO @ `8cc059b`, MIT
//! licence, "Copyright 2024 the Turso authors";
//! <https://github.com/tursodatabase/turso>), the starting point approved as
//! R12, adapted: keys are integers, events carry the recording thread, the
//! recorder is shared between threads, and it renders a `String` instead of
//! writing a file. Turso's licence notice: "Permission is hereby granted,
//! free of charge, to any person obtaining a copy of this software and
//! associated documentation files (the "Software"), to deal in the Software
//! without restriction, including without limitation the rights to use,
//! copy, modify, merge, publish, distribute, sublicense, and/or sell copies
//! of the Software, and to permit persons to whom the Software is furnished
//! to do so, subject to the following conditions: The above copyright notice
//! and this permission notice shall be included in all copies or substantial
//! portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
//! WARRANTY OF ANY KIND, EXPRESS OR IMPLIED."

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sync::{Mutex, PoisonError};

/// One micro-operation of a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// List-append: append `value` to list `key`.
    Append { key: u64, value: i64 },
    /// List-append: read list `key`; `None` before the read completes.
    ReadList { key: u64, result: Option<Vec<i64>> },
    /// Rw-register: write `value` to register `key`.
    Write { key: u64, value: i64 },
    /// Rw-register: read register `key`. `None` before the read completes;
    /// `Some(None)` reads an unwritten register (nil).
    Read {
        key: u64,
        result: Option<Option<i64>>,
    },
}

impl Op {
    fn edn(&self, out: &mut String) {
        match self {
            Op::Append { key, value } => {
                let _ = write!(out, "[:append {key} {value}]");
            }
            Op::ReadList { key, result } => {
                let _ = write!(out, "[:r {key} ");
                match result {
                    None => out.push_str("nil"),
                    Some(v) => {
                        out.push('[');
                        for (i, x) in v.iter().enumerate() {
                            if i > 0 {
                                out.push(' ');
                            }
                            let _ = write!(out, "{x}");
                        }
                        out.push(']');
                    }
                }
                out.push(']');
            }
            Op::Write { key, value } => {
                let _ = write!(out, "[:w {key} {value}]");
            }
            Op::Read { key, result } => match result {
                Some(Some(v)) => {
                    let _ = write!(out, "[:r {key} {v}]");
                }
                _ => {
                    let _ = write!(out, "[:r {key} nil]");
                }
            },
        }
    }
}

/// The type of a history event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    /// The transaction started.
    Invoke,
    /// The transaction committed.
    Ok,
    /// The transaction aborted.
    Fail,
}

/// A history shared by the threads of one test.
#[derive(Debug, Default)]
pub(crate) struct History {
    events: Mutex<Vec<(Event, usize, Vec<Op>)>>,
}

impl History {
    /// Records an event of `process` with operations `ops`. The index of
    /// an event is its position in the history, so events are ordered by
    /// the time they are recorded.
    pub(crate) fn record(&self, event: Event, process: usize, ops: Vec<Op>) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((event, process, ops));
    }

    /// Number of events.
    pub(crate) fn len(&self) -> usize {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// The history as EDN, one operation map per line.
    pub(crate) fn to_edn(&self) -> String {
        let events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out = String::new();
        for (index, (event, process, ops)) in events.iter().enumerate() {
            let ty = match event {
                Event::Invoke => ":invoke",
                Event::Ok => ":ok",
                Event::Fail => ":fail",
            };
            let _ = write!(out, "{{:type {ty}, :f :txn, :value [");
            for (i, op) in ops.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                op.edn(&mut out);
            }
            let _ = writeln!(out, "], :process {process}, :index {index}}}");
        }
        out
    }
}

/// What `elle-cli` reported for one history. Dropping it removes the
/// history file unless the thread is panicking.
#[derive(Debug)]
pub(crate) struct Verdict {
    /// Whether the history obeys the consistency model.
    pub(crate) valid: bool,
    /// The anomaly types found, such as `G2-item`.
    pub(crate) anomalies: Vec<String>,
    /// The checker's JSON output, for failure messages.
    pub(crate) output: String,
    /// Where the history was written.
    pub(crate) path: PathBuf,
}

impl Drop for Verdict {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            // A file already gone is no problem: nothing else uses it.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// The `elle-cli` jar: `$LPG_ELLE_CLI`, else the install path recorded in
/// the Sprint 2 laboratory report.
fn elle_jar() -> PathBuf {
    if let Some(p) = std::env::var_os("LPG_ELLE_CLI") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").expect("HOME is set");
    let p = Path::new(&home).join("opt/elle-cli-0.1.11/target/elle-cli-0.1.11-standalone.jar");
    assert!(
        p.exists(),
        "elle-cli not found: set LPG_ELLE_CLI to the elle-cli standalone jar \
         (docs/evidence/lab-sprint-2/report.md, Tools)"
    );
    p
}

/// Writes `edn` to `/tmp/lpg-histories/<name>-<pid>.edn` and checks it
/// with `elle-cli` under `model` (`list-append` or `rw-register`) and
/// `consistency` (such as `serializable`): the history is valid when it
/// has none of the anomalies that model prohibits. The process
/// identifier keeps the file of each concurrent copy of a test apart
/// (the `stress` profile runs 16); the file is removed when the
/// [`Verdict`] is dropped, unless the thread is panicking, so a failed
/// test keeps it as evidence.
///
/// # Panics
///
/// If the file cannot be written, Java cannot run, or the checker reports
/// neither valid nor invalid (for instance `unknown`).
pub(crate) fn elle(name: &str, edn: &str, model: &str, consistency: &str) -> Verdict {
    // A tool intermediate: on the RAM-backed `/tmp` (CLAUDE.md, "Build
    // output in RAM"), never in the repository on disk.
    let dir = std::env::temp_dir().join("lpg-histories");
    std::fs::create_dir_all(&dir).expect("create /tmp/lpg-histories");
    let path = dir.join(format!("{name}-{}.edn", std::process::id()));
    std::fs::write(&path, edn).expect("write the history");
    let out = Command::new("java")
        // The C1 compiler only: the checker's output is byte-identical to a
        // run with the default tiered compilation (11 histories compared,
        // docs/evidence/lab-sprint-3/test-speed.md), and the JVM spends
        // 2.5 times less CPU, which the concurrent tests of a run need.
        .arg("-XX:TieredStopAtLevel=1")
        .arg("-jar")
        .arg(elle_jar())
        // The anomalies searched for are those `consistency` prohibits;
        // naming more with `--anomalies` would make them prohibited too.
        .args(["--model", model, "--consistency-models", consistency])
        .args(["--output", "json"])
        .arg(&path)
        .output()
        .expect("run java -jar elle-cli");
    let output = String::from_utf8_lossy(&out.stdout).into_owned();
    let valid = if output.contains("\"valid?\":true") {
        true
    } else if output.contains("\"valid?\":false") {
        false
    } else {
        panic!(
            "elle-cli gave no verdict for {}: status {:?}\n{output}\n{}",
            path.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let anomalies = output
        .split_once("\"anomaly-types\":[")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| {
            list.split(',')
                .map(|s| s.trim().trim_matches('"').to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Verdict {
        valid,
        anomalies,
        output,
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_render_as_elle_edn() {
        let h = History::default();
        h.record(
            Event::Invoke,
            0,
            vec![
                Op::Append { key: 1, value: 2 },
                Op::ReadList {
                    key: 3,
                    result: None,
                },
            ],
        );
        h.record(
            Event::Ok,
            0,
            vec![
                Op::Append { key: 1, value: 2 },
                Op::ReadList {
                    key: 3,
                    result: Some(vec![4, 5]),
                },
            ],
        );
        h.record(
            Event::Fail,
            1,
            vec![
                Op::Write { key: 1, value: 7 },
                Op::Read {
                    key: 2,
                    result: Some(None),
                },
            ],
        );
        assert_eq!(
            h.to_edn(),
            "{:type :invoke, :f :txn, :value [[:append 1 2] [:r 3 nil]], :process 0, :index 0}\n\
             {:type :ok, :f :txn, :value [[:append 1 2] [:r 3 [4 5]]], :process 0, :index 1}\n\
             {:type :fail, :f :txn, :value [[:w 1 7] [:r 2 nil]], :process 1, :index 2}\n"
        );
        assert_eq!(h.len(), 3);
    }

    /// The checker rejects a hand-written G1c cycle: each transaction reads
    /// the other's write (TS-30 Isolation; #24 acceptance).
    #[test]
    fn elle_reports_a_hand_written_g1c_cycle_invalid() {
        let h = History::default();
        let t0 = |r| {
            vec![
                Op::Write { key: 1, value: 1 },
                Op::Read { key: 2, result: r },
            ]
        };
        let t1 = |r| {
            vec![
                Op::Write { key: 2, value: 1 },
                Op::Read { key: 1, result: r },
            ]
        };
        h.record(Event::Invoke, 0, t0(None));
        h.record(Event::Invoke, 1, t1(None));
        h.record(Event::Ok, 0, t0(Some(Some(1))));
        h.record(Event::Ok, 1, t1(Some(Some(1))));
        let v = elle(
            "g1c-hand-written",
            &h.to_edn(),
            "rw-register",
            "serializable",
        );
        assert!(!v.valid, "{}", v.output);
        assert!(v.anomalies.iter().any(|a| a == "G1c"), "{}", v.output);
    }
}
