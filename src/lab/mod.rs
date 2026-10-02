//! Laboratory harness (TS-30 Performance), derived from M-LAYOUT
//! (`docs/evidence/layout-bench`).
//!
//! Each workload is an `#[ignore]` test:
//!
//! ```text
//! LPG_LAB_EXT4=… LPG_LAB_XFS=… LPG_LAB_OUT=docs/evidence/lab-sprint-1/raw \
//!     cargo test -p lpg --profile lab --lib -- --ignored lab::
//! ```
//!
//! Every workload runs 3 measured rounds and reports median [min, max]. The
//! host clock source makes an `Instant` pair cost 2.79 µs (M-LAYOUT), so
//! rounds are timed as one batch each. Process I/O comes from
//! `/proc/self/io` and memory from `/proc/self/status`. OS access here uses
//! `rustix` directly: these files are not database files, so they do not go
//! through the storage abstraction.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rustix::fs::{Advice, Mode, OFlags};

use crate::obs::recording::{Metrics, RecordingSink};

mod generator;
mod obs;
mod storage;

/// Measured rounds per workload.
pub(crate) const ROUNDS: usize = 3;

/// Reads a whole file, such as one under `/proc`.
pub(crate) fn read_text(path: &Path) -> String {
    let fd = rustix::fs::open(path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())
        .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = rustix::io::read(&fd, &mut buf[..]).expect("read");
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(out).expect("UTF-8 text")
}

/// Creates or replaces a file holding `text`.
pub(crate) fn write_text(path: &Path, text: &str) {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::CLOEXEC;
    let fd = rustix::fs::open(path, flags, Mode::from_raw_mode(0o644))
        .unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut rest = text.as_bytes();
    while !rest.is_empty() {
        let n = rustix::io::write(&fd, rest).expect("write");
        rest = &rest[n..];
    }
}

/// Drops a file's clean pages from the OS cache (`POSIX_FADV_DONTNEED`).
pub(crate) fn evict_from_os_cache(path: &Path) {
    let fd = rustix::fs::open(path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty()).expect("open");
    rustix::fs::fadvise(&fd, 0, None, Advice::DontNeed).expect("fadvise");
}

/// The numeric value of the line starting with `key` in `text`.
fn field(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no numeric field {key}"))
}

/// Process counters at one instant.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Probe {
    /// `read_bytes` of `/proc/self/io`: bytes this process caused to be read
    /// from storage.
    pub(crate) read_bytes: u64,
    /// `write_bytes` of `/proc/self/io`.
    pub(crate) write_bytes: u64,
    /// `VmRSS` in KiB.
    pub(crate) rss_kib: u64,
    /// `VmHWM` in KiB.
    pub(crate) hwm_kib: u64,
}

impl Probe {
    pub(crate) fn now() -> Probe {
        let io = read_text(Path::new("/proc/self/io"));
        let status = read_text(Path::new("/proc/self/status"));
        Probe {
            read_bytes: field(&io, "read_bytes:"),
            write_bytes: field(&io, "write_bytes:"),
            rss_kib: field(&status, "VmRSS:"),
            hwm_kib: field(&status, "VmHWM:"),
        }
    }
}

/// One workload's measurement: `ROUNDS` rounds of `ops` operations each.
#[derive(Debug)]
pub(crate) struct Measured {
    pub(crate) name: String,
    pub(crate) cache: String,
    pub(crate) ops: u64,
    ns_per_op: Vec<f64>,
    read_bytes: u64,
    write_bytes: u64,
    end: Probe,
    sink: Metrics,
}

/// Runs `round` `ROUNDS` times, timing each call as one batch of `ops`
/// operations. `prepare` runs untimed before each round, outside the I/O
/// accounting.
pub(crate) fn measure(
    name: &str,
    cache: &str,
    ops: u64,
    sink: &RecordingSink,
    mut prepare: impl FnMut(usize),
    mut round: impl FnMut(usize),
) -> Measured {
    let mut ns_per_op = Vec::with_capacity(ROUNDS);
    let (mut read_bytes, mut write_bytes) = (0, 0);
    let before = sink.snapshot();
    for r in 0..ROUNDS {
        prepare(r);
        let p0 = Probe::now();
        let t = Instant::now();
        round(r);
        let ns = t.elapsed().as_nanos() as f64;
        let p1 = Probe::now();
        ns_per_op.push(ns / ops as f64);
        read_bytes += p1.read_bytes - p0.read_bytes;
        write_bytes += p1.write_bytes - p0.write_bytes;
    }
    Measured {
        name: name.to_owned(),
        cache: cache.to_owned(),
        ops,
        ns_per_op,
        read_bytes,
        write_bytes,
        end: Probe::now(),
        sink: sink.snapshot().since(&before),
    }
}

/// `median [min, max]` of `v`.
pub(crate) fn median_range(v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let mid = s.len() / 2;
    let median = if s.len() % 2 == 1 {
        s[mid]
    } else {
        (s[mid - 1] + s[mid]) / 2.0
    };
    format!("{median:.0} [{:.0}, {:.0}]", s[0], s[s.len() - 1])
}

/// `git` revision of the source tree, with `-dirty` if anything differs from
/// it, including untracked files.
fn commit() -> String {
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("run git");
        String::from_utf8(out.stdout).expect("UTF-8")
    };
    let rev = git(&["rev-parse", "--short=12", "HEAD"]).trim().to_owned();
    if git(&["status", "--porcelain"]).trim().is_empty() {
        rev
    } else {
        format!("{rev}-dirty")
    }
}

fn date() -> String {
    let out = Command::new("date")
        .arg("-Iseconds")
        .output()
        .expect("run date");
    String::from_utf8(out.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

/// A required environment variable naming a directory.
pub(crate) fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {var}; see src/lab/mod.rs"))
}

/// Writes the Markdown table of `rows` to `$LPG_LAB_OUT/<file>`.
pub(crate) fn write_table(file: &str, title: &str, context: &[(&str, String)], rows: &[Measured]) {
    let mut md = format!("# {title}\n\n");
    let mut lines = vec![
        ("date", date()),
        ("commit", commit()),
        ("profile", "lab (release + debug info)".to_owned()),
    ];
    lines.extend(context.iter().map(|(k, v)| (*k, v.clone())));
    for (k, v) in lines {
        writeln!(md, "- {k}: {v}").unwrap();
    }
    md.push_str(
        "\n| workload | cache mode | ops/round | ns/op median [min, max] | KiB read/op | KiB written/op | VmRSS KiB | VmHWM KiB | sink difference |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    for m in rows {
        let total = (m.ops * ROUNDS as u64) as f64;
        writeln!(
            md,
            "| {} | {} | {} | {} | {:.2} | {:.2} | {} | {} | {} |",
            m.name,
            m.cache,
            m.ops,
            median_range(&m.ns_per_op),
            m.read_bytes as f64 / 1024.0 / total,
            m.write_bytes as f64 / 1024.0 / total,
            m.end.rss_kib,
            m.end.hwm_kib,
            m.sink,
        )
        .unwrap();
    }
    let path = env_dir("LPG_LAB_OUT").join(file);
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}

mod tests {
    use super::*;

    #[test]
    fn median_range_of_three() {
        assert_eq!(median_range(&[3.0, 1.0, 2.0]), "2 [1, 3]");
    }

    #[test]
    fn probe_reads_proc() {
        let p = Probe::now();
        assert!(p.rss_kib > 0 && p.hwm_kib >= p.rss_kib);
    }

    #[test]
    fn text_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.md");
        write_text(&path, "one");
        write_text(&path, "two");
        assert_eq!(read_text(&path), "two");
    }
}
