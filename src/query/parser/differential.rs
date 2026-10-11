//! Differential tests of the parser against the ANTLR oracle
//! (`tools/gql-oracle`, NFR-VER-002, TS-30).
//!
//! The oracle is a separate workspace and never a dependency of `lpg`
//! (`cargo tree -p lpg` has no `antlr4rust`). It is used in two ways:
//!
//! * **Stored labels**, checked by the default suite: the oracle's
//!   verdicts on the fixed corpora (the 14 opengql/grammar samples and
//!   the lab inputs, the miniGU inputs, and every GQL text of lpg's
//!   tests) are files under `src/query/testdata/`. The tests compare lpg
//!   with them, so the default suite needs no oracle.
//! * **Live runs**, `#[ignore]`d: the oracle binary
//!   (`tools/gql-oracle/target/release/gql-oracle`, or `LPG_ORACLE`)
//!   generates sentences and near-miss mutants for each seed and labels
//!   them, and the labels of the stored corpora are re-checked. Run with
//!   `cargo nextest run -p lpg --run-ignored only -E 'test(differential)'`.

use super::parse_program;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether lpg accepts `text`: it parses, or fails with something other
/// than a syntax error (none exists today).
pub(super) fn lpg_accepts(text: &str) -> bool {
    match parse_program(text) {
        Ok(_) => true,
        Err(e) => e.code() != "42001",
    }
}

/// The oracle binary: `LPG_ORACLE`, or the release build of
/// `tools/gql-oracle`.
fn oracle() -> PathBuf {
    std::env::var_os("LPG_ORACLE").map_or_else(
        || manifest_dir().join("tools/gql-oracle/target/release/gql-oracle"),
        PathBuf::from,
    )
}

/// Runs the oracle with `args` and `stdin`; its standard output.
fn run_oracle(args: &[&str], stdin: &str, seed: Option<u64>) -> String {
    let path = oracle();
    assert!(
        path.exists(),
        "the oracle binary {} is missing: run `cargo build --release` in tools/gql-oracle",
        path.display()
    );
    let mut cmd = Command::new(&path);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(seed) = seed {
        cmd.env("LPG_SEED", seed.to_string());
    }
    let mut child = cmd.spawn().expect("spawn the oracle");
    let mut input = child.stdin.take().expect("stdin");
    let data = stdin.to_owned();
    let writer = std::thread::spawn(move || input.write_all(data.as_bytes()));
    let out = child.wait_with_output().expect("oracle output");
    writer.join().expect("writer").expect("write to the oracle");
    assert!(out.status.success(), "the oracle failed: {args:?}");
    String::from_utf8(out.stdout).expect("UTF-8")
}

/// The oracle's verdicts on `texts` (one line each), labelled by
/// parallel `gql-oracle label` processes.
fn oracle_labels(texts: &[String]) -> Vec<bool> {
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let chunk = texts.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = texts
            .chunks(chunk)
            .map(|c| {
                s.spawn(move || {
                    let out = run_oracle(&["label"], &(c.join("\n") + "\n"), None);
                    let labels: Vec<bool> = out.lines().map(|l| l.starts_with("ACCEPT")).collect();
                    assert_eq!(labels.len(), c.len(), "one label per text");
                    labels
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("labeller"))
            .collect()
    })
}

/// lpg's verdicts on `texts`, in parallel.
fn lpg_labels(texts: &[String]) -> Vec<bool> {
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let chunk = texts.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = texts
            .chunks(chunk)
            .map(|c| s.spawn(move || c.iter().map(|t| lpg_accepts(t)).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("parser thread"))
            .collect()
    })
}

/// The texts on which lpg and `expected` differ, with the oracle's
/// verdict.
fn differences<'a>(texts: &'a [String], expected: &[bool]) -> Vec<(&'a str, bool)> {
    let got = lpg_labels(texts);
    texts
        .iter()
        .zip(expected)
        .zip(got)
        .filter(|((_, e), g)| **e != *g)
        .map(|((t, e), _)| (t.as_str(), *e))
        .collect()
}

/// Reports `diffs` (at most 40 shown) and writes all of them to
/// `LPG_DIFF_OUT` when set.
fn report(name: &str, total: usize, diffs: &[(&str, bool)]) {
    println!("{name}: {total} texts, {} differences", diffs.len());
    for (t, oracle) in diffs.iter().take(40) {
        let verdict = if *oracle {
            "oracle ACCEPTS"
        } else {
            "oracle REJECTS"
        };
        println!("  {verdict}: {t}");
    }
    if let Some(path) = std::env::var_os("LPG_DIFF_OUT") {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open LPG_DIFF_OUT");
        for (t, oracle) in diffs {
            writeln!(
                f,
                "{name}\t{}\t{t}",
                if *oracle { "ACCEPT" } else { "REJECT" }
            )
            .expect("write");
        }
    }
}

/// A labelled corpus directory: each file is one text; `labels.tsv` is
/// the output of `gql-oracle label <dir>` (`ACCEPT\tname` or
/// `REJECT\tname\terror`).
fn labelled_dir(dir: &Path) -> Vec<(String, String, bool)> {
    let labels = std::fs::read_to_string(dir.join("labels.tsv")).expect("labels.tsv");
    labels
        .lines()
        .map(|line| {
            let mut f = line.split('\t');
            let verdict = f.next().unwrap_or("");
            let name = f.next().expect("name").to_owned();
            let text = std::fs::read_to_string(dir.join(&name)).expect("corpus file");
            (name, text, verdict == "ACCEPT")
        })
        .collect()
}

/// The corpora with stored labels.
fn corpora() -> Vec<PathBuf> {
    vec![
        manifest_dir().join("src/query/testdata/samples"),
        manifest_dir().join("src/query/testdata/minigu"),
        manifest_dir().join(TEST_TEXTS),
        manifest_dir().join(SCENARIO_TEXTS),
    ]
}

/// The corpus of the GQL texts of the conformance scenarios
/// (`tests/tck/`, `crate::tck::scenario_texts`).
const SCENARIO_TEXTS: &str = "src/query/testdata/scenarios";

/// The corpus of the GQL texts of lpg's tests.
const TEST_TEXTS: &str = "src/query/testdata/tests";

/// The test helpers whose first argument is a GQL text. A text written
/// as a string literal in an unqualified call to one of them, anywhere
/// under `src/`, belongs to the corpus; texts built at run time
/// (`format!`, loops) do not.
const TEXT_HELPERS: &[&str] = &[
    "parse_program",
    "parse",
    "syntax_error",
    "procedure",
    "statement",
    "returned",
    "ast_contains",
    "tokenize",
    "kinds",
];

/// Every GQL text of lpg's tests: the literal first arguments of the
/// [`TEXT_HELPERS`] calls in the `.rs` files under `src/`.
fn collect_test_texts() -> std::collections::BTreeSet<String> {
    let mut files = Vec::new();
    let mut dirs = vec![manifest_dir().join("src")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    let mut texts = std::collections::BTreeSet::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("read a source file");
        for helper in TEXT_HELPERS {
            let call = format!("{helper}(");
            let mut from = 0;
            while let Some(i) = source[from..].find(&call) {
                let at = from + i;
                from = at + call.len();
                let before = source[..at].chars().next_back();
                // `x.parse(…)` and `Point::parse(…)` are other functions.
                if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ':')) {
                    continue;
                }
                if let Some(text) = string_literal(source[from..].trim_start()) {
                    texts.insert(text);
                }
            }
        }
    }
    texts
}

/// The value of the Rust string literal at the start of `s`, plain
/// (`"…"`, with its escapes) or raw (`r"…"`, `r#"…"#`).
fn string_literal(s: &str) -> Option<String> {
    if let Some(raw) = s.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let body = raw[hashes..].strip_prefix('"')?;
        let end = body.find(&format!("\"{}", "#".repeat(hashes)))?;
        return Some(body[..end].to_owned());
    }
    let mut chars = s.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'x' => {
                    let hex: String = chars.by_ref().take(2).collect();
                    out.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
                }
                'u' => {
                    let rest = chars.as_str();
                    let close = rest.find('}')?;
                    let code = u32::from_str_radix(&rest[1..close], 16).ok()?;
                    out.push(char::from_u32(code)?);
                    chars = rest[close + 1..].chars();
                }
                '\n' => {
                    // A line continuation skips the leading white space
                    // of the next line.
                    chars = chars.as_str().trim_start().chars();
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}

/// The corpus file of a text: a 64-bit FNV-1a hash of it, so the name
/// is stable across runs and machines.
fn test_text_file(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}.gql")
}

/// The corpus in `dir` holds exactly `texts`, each with a stored oracle
/// label. With `update` set in the environment the corpus files are
/// rewritten; the labels then come from
/// `differential_stored_labels_are_current` with `LPG_UPDATE_LABELS=1`.
fn check_text_corpus(dir: &str, texts: &std::collections::BTreeSet<String>, update: &str) {
    let dir = manifest_dir().join(dir);
    let wanted: std::collections::BTreeMap<String, String> = texts
        .iter()
        .map(|t| (test_text_file(t), t.clone()))
        .collect();
    let stored: std::collections::BTreeSet<String> = std::fs::read_dir(&dir)
        .map(|d| {
            d.map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".gql"))
                .collect()
        })
        .unwrap_or_default();
    if std::env::var_os(update).is_some() {
        std::fs::create_dir_all(&dir).expect("create the corpus directory");
        for name in stored.iter().filter(|n| !wanted.contains_key(*n)) {
            std::fs::remove_file(dir.join(name)).expect("remove a stale text");
        }
        for (name, text) in &wanted {
            std::fs::write(dir.join(name), text).expect("write a text");
        }
        return;
    }
    let names: std::collections::BTreeSet<String> = wanted.keys().cloned().collect();
    assert_eq!(
        stored, names,
        "the texts changed: rerun with {update}=1, then relabel"
    );
    // `stored_corpora_match_the_oracle_labels` compares lpg with the
    // labels; here every text must have one.
    assert_eq!(
        labelled_dir(&dir).len(),
        names.len(),
        "one label per text: relabel"
    );
}

/// The corpus of test texts holds exactly the texts the tests use now,
/// each with a stored oracle label (`LPG_UPDATE_TEST_TEXTS=1` rewrites it).
#[test]
fn test_texts_match_the_oracle_labels() {
    let texts = collect_test_texts();
    assert!(
        texts.len() > 50,
        "the collector found {} texts",
        texts.len()
    );
    check_text_corpus(TEST_TEXTS, &texts, "LPG_UPDATE_TEST_TEXTS");
}

/// The corpus of scenario texts holds exactly the GQL texts of the
/// conformance scenarios, each with a stored oracle label
/// (`LPG_UPDATE_SCENARIO_TEXTS=1` rewrites it).
#[test]
fn scenario_texts_match_the_oracle_labels() {
    let texts = crate::tck::scenario_texts();
    assert!(texts.len() > 500, "{} scenario texts", texts.len());
    check_text_corpus(SCENARIO_TEXTS, &texts, "LPG_UPDATE_SCENARIO_TEXTS");
}

#[test]
fn stored_corpora_match_the_oracle_labels() {
    for dir in corpora() {
        let rows = labelled_dir(&dir);
        assert!(!rows.is_empty(), "{}", dir.display());
        let mismatches: Vec<_> = rows
            .iter()
            .filter(|(_, text, accepted)| lpg_accepts(text) != *accepted)
            .map(|(name, _, accepted)| (name.clone(), *accepted))
            .collect();
        assert!(
            mismatches.is_empty(),
            "{}: lpg differs from the oracle on {mismatches:?}",
            dir.display()
        );
    }
}

/// The stored labels are the oracle's current verdicts.
#[test]
#[ignore = "needs the oracle binary (tools/gql-oracle)"]
fn differential_stored_labels_are_current() {
    for dir in corpora() {
        let out = run_oracle(&["label", dir.to_str().expect("UTF-8 path")], "", None);
        // The oracle labels every file; the README, the licences and
        // labels.tsv are not corpus texts.
        let live: String = out
            .lines()
            .filter(|l| {
                let name = l.split('\t').nth(1).unwrap_or("");
                !(name == "README.md" || name == "labels.tsv" || name.starts_with("LICENSE"))
            })
            .map(|l| format!("{l}\n"))
            .collect();
        if std::env::var_os("LPG_UPDATE_LABELS").is_some() {
            std::fs::write(dir.join("labels.tsv"), &live).expect("write labels.tsv");
            continue;
        }
        let stored = std::fs::read_to_string(dir.join("labels.tsv")).expect("labels.tsv");
        assert_eq!(live, stored, "{}: rerun gql-oracle label", dir.display());
    }
}

/// The seeds of the generated runs: `LPG_SEED`, or three fixed ones.
fn seeds() -> Vec<u64> {
    match std::env::var("LPG_SEED") {
        Ok(s) => vec![s.parse().expect("LPG_SEED is a decimal number")],
        Err(_) => vec![20_261_008, 1, 0x00C0_FFEE],
    }
}

fn count() -> usize {
    std::env::var("LPG_DIFF_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000)
}

/// Generated sentences and near-miss mutants of each seed: lpg and the
/// oracle agree on every one.
#[test]
#[ignore = "needs the oracle binary (tools/gql-oracle); a laboratory run"]
fn differential_generated_sentences_and_mutants() {
    let n = count();
    let mut total_diffs = 0;
    for seed in seeds() {
        for kind in ["generate", "mutants"] {
            let out = run_oracle(&[kind, &n.to_string()], "", Some(seed));
            let texts: Vec<String> = out.lines().map(str::to_owned).collect();
            let labels = oracle_labels(&texts);
            let accepted = labels.iter().filter(|a| **a).count();
            let diffs = differences(&texts, &labels);
            println!(
                "seed {seed} {kind}: {} texts, {accepted} accepted by the oracle",
                texts.len()
            );
            report(&format!("seed {seed} {kind}"), texts.len(), &diffs);
            total_diffs += diffs.len();
        }
    }
    assert_eq!(total_diffs, 0, "lpg differs from the oracle");
}

/// Texts from a file (`LPG_DIFF_INPUT`, one per line), for
/// investigations.
#[test]
#[ignore = "needs the oracle binary and LPG_DIFF_INPUT"]
fn differential_input_file() {
    let Some(path) = std::env::var_os("LPG_DIFF_INPUT") else {
        return;
    };
    let texts: Vec<String> = std::fs::read_to_string(path)
        .expect("input")
        .lines()
        .map(str::to_owned)
        .collect();
    let labels = oracle_labels(&texts);
    let diffs = differences(&texts, &labels);
    report("input", texts.len(), &diffs);
    assert!(diffs.is_empty());
}

/// Prints lpg's verdict on each line of `LPG_DEBUG_FILE`.
#[test]
#[ignore = "an investigation aid"]
fn debug_parse_file() {
    let Some(path) = std::env::var_os("LPG_DEBUG_FILE") else {
        return;
    };
    for line in std::fs::read_to_string(path).expect("input").lines() {
        let text = line.rsplit('\t').next().unwrap_or(line);
        match parse_program(text) {
            Ok(_) => println!("OK      {text}"),
            Err(e) => println!("{e}\n        {text}"),
        }
    }
}
