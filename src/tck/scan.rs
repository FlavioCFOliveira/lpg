//! Scanner of `.feature` files: scenarios, outlines, examples, tags,
//! steps with their docstrings and tables.
//!
//! Adapted from the feature scanner of cyrs @ `6a6bb34`
//! (<https://github.com/phall1/cyrs>, `crates/cyrs-tck/tests/full.rs:88-360`,
//! Copyright Patrick Hall, licensed MIT OR Apache-2.0; used here under
//! Apache-2.0, see `LICENSE-APACHE`): the depth-first walk of the
//! `features/` tree, the line scan without a Gherkin dependency, the
//! triple-quote toggle independent of the scan state, the expansion of a
//! `Scenario Outline` once per `Examples` row by substituting
//! `<placeholder>`, and the rule that a blank line does not end a scenario.
//! Changed: cyrs keeps only the query docstring of each scenario; this
//! scanner keeps every step (its text, docstring and table), the
//! `Background` steps, and the tags of the feature, the scenario and the
//! `Examples` block, and it removes the docstring indentation as
//! Gherkin does (the column of the opening `"""`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One step: its text after the keyword, its docstring and its table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Step {
    /// The text after `Given`, `When`, `Then`, `And` or `But`.
    pub(super) text: String,
    /// The docstring that follows the step.
    pub(super) doc: Option<String>,
    /// The table rows that follow the step, cells trimmed.
    pub(super) table: Vec<Vec<String>>,
}

/// One scenario instance: a `Scenario`, or one `Examples` row of a
/// `Scenario Outline`, with the `Background` steps first.
#[derive(Debug, Clone)]
pub(super) struct Scenario {
    /// The feature file, relative to its `features/` directory.
    pub(super) file: String,
    /// The trimmed `Scenario:` or `Scenario Outline:` line.
    pub(super) title: String,
    /// The cells of the `Examples` row, joined by ` | `; empty for a
    /// plain scenario.
    pub(super) row: String,
    /// The tags of the feature, the scenario and the `Examples` block,
    /// without `@`.
    pub(super) tags: BTreeSet<String>,
    /// The steps, `Background` first.
    pub(super) steps: Vec<Step>,
}

/// Tags whose `Examples` rows are left out, as the selection manifest
/// leaves them out (`docs/evidence/lab-sprint-3/tck_manifest.py`).
const EXCLUDED_EXAMPLES: [&str; 2] = ["ignore", "Todo"];

/// Every `.feature` file under `root`, sorted.
pub(super) fn walk_features(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("feature") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The tags of a tag line: each `@name`, a trailing comma removed.
fn tags_of(line: &str) -> impl Iterator<Item = String> + '_ {
    line.split_whitespace()
        .filter_map(|t| t.strip_prefix('@'))
        .map(|t| t.trim_end_matches(',').to_string())
        .filter(|t| !t.is_empty())
}

/// The cells of a table row `| a | b |`; `\|`, `\\` and `\n` unescaped.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim();
    let inner = inner.strip_prefix('|').unwrap_or(inner);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('|') => cell.push('|'),
                Some('n') => cell.push('\n'),
                Some('\\') => cell.push('\\'),
                Some(other) => {
                    cell.push('\\');
                    cell.push(other);
                }
                None => cell.push('\\'),
            },
            '|' => out.push(std::mem::take(&mut cell).trim().to_string()),
            c => cell.push(c),
        }
    }
    out.push(cell.trim().to_string());
    out
}

/// An `Examples` block: its tags, header and rows.
type Examples = (BTreeSet<String>, Option<Vec<String>>, Vec<Vec<String>>);

/// A scenario being read.
struct Open {
    title: String,
    outline: bool,
    tags: BTreeSet<String>,
    steps: Vec<Step>,
    /// `Examples` blocks: tags, header, rows.
    examples: Vec<Examples>,
}

/// Replaces every `<name>` of `text` by the cell of column `name`.
fn substitute(text: &str, header: &[String], row: &[String]) -> String {
    let mut out = text.to_string();
    for (h, v) in header.iter().zip(row) {
        out = out.replace(&format!("<{h}>"), v);
    }
    out
}

/// Every scenario instance of the feature text `src`, read from `file`.
pub(super) fn scan(file: &str, src: &str) -> Vec<Scenario> {
    let mut out = Vec::new();
    let mut feature_tags = BTreeSet::new();
    let mut pending = BTreeSet::new();
    let mut background: Vec<Step> = Vec::new();
    let mut in_background = false;
    let mut current: Option<Open> = None;
    let mut in_examples = false;
    let lines: Vec<&str> = src.lines().collect();

    let flush = |open: Open, background: &[Step], out: &mut Vec<Scenario>| {
        let tags: BTreeSet<String> = open.tags.clone();
        if !open.outline {
            let mut steps = background.to_vec();
            steps.extend(open.steps);
            out.push(Scenario {
                file: file.to_string(),
                title: open.title,
                row: String::new(),
                tags,
                steps,
            });
            return;
        }
        // A Scenario Outline with no Examples yields nothing.
        for (ex_tags, header, rows) in &open.examples {
            let Some(header) = header else { continue };
            if EXCLUDED_EXAMPLES.iter().any(|t| ex_tags.contains(*t)) {
                continue;
            }
            for row in rows {
                let mut steps = background.to_vec();
                steps.extend(open.steps.iter().map(|s| {
                    Step {
                        text: substitute(&s.text, header, row),
                        doc: s.doc.as_ref().map(|d| substitute(d, header, row)),
                        table: s
                            .table
                            .iter()
                            .map(|r| r.iter().map(|c| substitute(c, header, row)).collect())
                            .collect(),
                    }
                }));
                let mut tags = tags.clone();
                tags.extend(ex_tags.iter().cloned());
                out.push(Scenario {
                    file: file.to_string(),
                    title: open.title.clone(),
                    row: row.join(" | "),
                    tags,
                    steps,
                });
            }
        }
    };

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        i += 1;

        // A docstring belongs to the last step read.
        if trimmed == "\"\"\"" {
            let indent = line.find('"').unwrap_or(0);
            let mut body = Vec::new();
            while i < lines.len() && lines[i].trim() != "\"\"\"" {
                let l = lines[i];
                let cut = match l.get(..indent) {
                    Some(lead) if lead.trim().is_empty() => &l[indent..],
                    _ => l,
                };
                body.push(cut);
                i += 1;
            }
            i += 1; // the closing quotes
            let doc = body.join("\n");
            let steps = match (&mut current, in_background) {
                (_, true) => &mut background,
                (Some(open), false) => &mut open.steps,
                (None, false) => continue,
            };
            if let Some(step) = steps.last_mut() {
                step.doc = Some(doc);
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('@') {
            pending.extend(tags_of(trimmed));
            continue;
        }
        if trimmed.starts_with("Feature:") {
            feature_tags = std::mem::take(&mut pending);
            continue;
        }
        if trimmed.starts_with("Background:") {
            in_background = true;
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("Scenario Outline:")
            .map(|_| true)
            .or_else(|| trimmed.strip_prefix("Scenario:").map(|_| false))
        {
            if let Some(open) = current.take() {
                flush(open, &background, &mut out);
            }
            in_background = false;
            in_examples = false;
            let mut tags = feature_tags.clone();
            tags.extend(std::mem::take(&mut pending));
            current = Some(Open {
                title: trimmed.to_string(),
                outline: rest,
                tags,
                steps: Vec::new(),
                examples: Vec::new(),
            });
            continue;
        }
        if trimmed.starts_with("Examples:") {
            in_examples = true;
            if let Some(open) = &mut current {
                open.examples
                    .push((std::mem::take(&mut pending), None, Vec::new()));
            }
            continue;
        }
        if trimmed.starts_with('|') {
            let row = cells(trimmed);
            if in_examples {
                if let Some((_, header, rows)) =
                    current.as_mut().and_then(|o| o.examples.last_mut())
                {
                    if header.is_none() {
                        *header = Some(row);
                    } else {
                        rows.push(row);
                    }
                }
                continue;
            }
            let steps = match (&mut current, in_background) {
                (_, true) => &mut background,
                (Some(open), false) => &mut open.steps,
                (None, false) => continue,
            };
            if let Some(step) = steps.last_mut() {
                step.table.push(row);
            }
            continue;
        }
        let step_text = ["Given ", "When ", "Then ", "And ", "But "]
            .iter()
            .find_map(|k| trimmed.strip_prefix(k));
        if let Some(text) = step_text {
            let step = Step {
                text: text.trim().to_string(),
                doc: None,
                table: Vec::new(),
            };
            if in_background {
                background.push(step);
            } else if let Some(open) = &mut current {
                open.steps.push(step);
            }
        }
    }
    if let Some(open) = current.take() {
        flush(open, &background, &mut out);
    }
    out
}

/// Every scenario instance of every `.feature` file under `root`.
pub(super) fn scan_tree(root: &Path) -> Vec<Scenario> {
    let mut out = Vec::new();
    for path in walk_features(root) {
        let src = std::fs::read_to_string(&path).expect("a feature file");
        let rel = path
            .strip_prefix(root)
            .expect("under the root")
            .to_string_lossy()
            .into_owned();
        out.extend(scan(&rel, &src));
    }
    out
}
