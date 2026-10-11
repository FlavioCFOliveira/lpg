//! Drift test of the generated ISO condition table, and the shape of
//! both tables.
//!
//! `generated_table_matches_the_insert` regenerates `generated.rs` from
//! the unmodified ISO conditions insert and fails on any difference. Run
//! it with `LPG_UPDATE_GQLSTATUS=1` to rewrite the file after the insert
//! changes.
//!
//! Ported from zu @ `a821319` (<https://github.com/tamnd/zu>, Apache-2.0,
//! see `LICENSE-APACHE`), `crates/zu-common/tests/gqlstatus_table.rs`:
//! the scanner of `<class>` and `<subclass>` elements, which keeps
//! self-closing classes apart from open ones, the synthesised `{class}000`
//! rows, the renderer and the `rustfmt` pass. Adapted: the insert path,
//! the `ISO_COUNT` constant, and the assertions on lpg's own tables.

use super::{GqlStatus, Severity, codes, iso_conditions, neo4j};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
struct Row {
    code: String,
    category: String,
    class: String,
    subclass: Option<String>,
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn insert() -> String {
    std::fs::read_to_string(
        manifest_dir().join("docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-conditions.xml"),
    )
    .expect("the conditions insert")
}

fn severity_for(category: &str) -> &'static str {
    match category {
        "S" => "Severity::Success",
        "N" => "Severity::NoData",
        "W" => "Severity::Warning",
        "I" => "Severity::Informational",
        "X" => "Severity::Exception",
        other => panic!("unknown class category {other:?} in the insert"),
    }
}

/// The value of `key="value"` inside a tag.
fn attr(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(at) = rest.find(key) {
        let after = &rest[at + key.len()..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with('=') {
            rest = after;
            continue;
        }
        let value = trimmed[1..].trim_start().strip_prefix('"')?;
        let end = value.find('"')?;
        return Some(value[..end].to_owned());
    }
    None
}

/// `(tag name, inside, self-closing)` of every element start, in order.
fn scan_tags(body: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(lt) = body[i..].find('<') {
        let start = i + lt;
        let Some(end) = body[start..].find('>') else {
            break;
        };
        let inside = &body[start + 1..start + end];
        i = start + end + 1;
        if inside.starts_with(['/', '!', '?']) {
            continue;
        }
        let self_closing = inside.ends_with('/');
        let inside = inside.trim_end_matches('/');
        let name = inside
            .split(|c: char| c.is_ascii_whitespace())
            .next()
            .unwrap_or("")
            .to_owned();
        out.push((name, inside.to_owned(), self_closing));
    }
    out
}

/// Every class (as `{class}000`) and subclass of the insert, in code
/// order.
fn parse_insert(xml: &str) -> Vec<Row> {
    // Everything before <conditions> is the DTD, which declares elements
    // named `class` and `subclass`.
    let body = &xml[xml.find("<conditions>").expect("<conditions>")..];
    let mut rows = Vec::new();
    let mut open: Option<(String, String, String)> = None;
    for (name, inside, self_closing) in scan_tags(body) {
        match name.as_str() {
            "class" => {
                let category = attr(&inside, "category").expect("class category");
                let code = attr(&inside, "code").expect("class code");
                let class = attr(&inside, "name").expect("class name");
                assert_eq!(code.len(), 2, "class code {code:?}");
                rows.push(Row {
                    code: format!("{code}000"),
                    category: category.clone(),
                    class: class.clone(),
                    subclass: None,
                });
                // A self-closing class must not stay open, or the next
                // class's subclasses would be attributed to it.
                open = (!self_closing).then_some((category, code, class));
            }
            "subclass" => {
                let (category, class_code, class) =
                    open.as_ref().expect("subclass outside a class");
                let sub = attr(&inside, "code").expect("subclass code");
                assert_eq!(sub.len(), 3, "subclass code {sub:?}");
                rows.push(Row {
                    code: format!("{class_code}{sub}"),
                    category: category.clone(),
                    class: class.clone(),
                    subclass: Some(attr(&inside, "name").expect("subclass name")),
                });
            }
            _ => {}
        }
    }
    rows.sort_by(|a, b| a.code.cmp(&b.code));
    rows
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render(rows: &[Row]) -> String {
    let mut out = String::from(
        "//! Generated from the ISO/IEC 39075:2024 conditions insert\n\
         //! (`docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-conditions.xml`).\n\
         //! Do not edit by hand: run `LPG_UPDATE_GQLSTATUS=1 cargo test -p lpg\n\
         //! gqlstatus::tests::generated_table_matches_the_insert`.\n\
         //!\n\
         //! Codes and names are the insert's, verbatim. Layout ported from zu @\n\
         //! `a821319` (Apache-2.0), `crates/zu-common/src/gqlstatus/generated.rs`.\n\
         \n\
         use super::{Condition, GqlStatus, Severity};\n\
         \n\
         /// Every condition of the insert, in code order: one row per class\n\
         /// (`{class}000`) and one per subclass.\n\
         pub(super) static CONDITIONS: &[Condition] = &[\n",
    );
    for row in rows {
        let subclass = match &row.subclass {
            Some(s) => format!("Some(\"{}\")", escape(s)),
            None => "None".to_owned(),
        };
        out.push_str(&format!(
            "    Condition {{ code: \"{}\", severity: {}, class: \"{}\", subclass: {} }},\n",
            row.code,
            severity_for(&row.category),
            escape(&row.class),
            subclass,
        ));
    }
    out.push_str("];\n\n");
    out.push_str(&format!(
        "/// The number of rows of [`CONDITIONS`].\n\
         pub(super) const ISO_COUNT: u16 = {};\n\n",
        rows.len()
    ));
    out.push_str(
        "/// One constant per ISO condition: `C` followed by the code.\n\
         pub mod codes {\n    use super::GqlStatus;\n\n",
    );
    for (i, row) in rows.iter().enumerate() {
        let text = match &row.subclass {
            Some(s) => format!("{}, {s}", row.class),
            None => row.class.clone(),
        };
        out.push_str(&format!(
            "    /// `{}` {}\n    pub const C{}: GqlStatus = GqlStatus({i});\n",
            row.code,
            escape(&text),
            row.code,
        ));
    }
    out.push_str("}\n");
    out
}

/// Formats `source` with `rustfmt`, so the checked-in file passes
/// `cargo fmt --check` and the drift test compares formatted text.
pub(crate) fn rustfmt(source: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("rustfmt on PATH");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("write to rustfmt");
    let out = child.wait_with_output().expect("rustfmt output");
    assert!(
        out.status.success(),
        "rustfmt rejected the generated source"
    );
    String::from_utf8(out.stdout).expect("rustfmt emits UTF-8")
}

/// Compares `rendered` with the file at `path`, or writes it when the
/// variable `update` is set.
pub(crate) fn check_generated(path: &str, rendered: &str, update: &str) {
    let path = manifest_dir().join(path);
    if std::env::var_os(update).is_some() {
        std::fs::write(&path, rendered).expect("write the generated file");
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == rendered,
        "{} is stale; rerun the test with {update}=1",
        path.display()
    );
}

#[test]
fn generated_table_matches_the_insert() {
    let rendered = rustfmt(&render(&parse_insert(&insert())));
    check_generated(
        "src/gqlstatus/generated.rs",
        &rendered,
        "LPG_UPDATE_GQLSTATUS",
    );
}

#[test]
fn every_iso_class_and_subclass_is_present_once_with_its_category() {
    let rows = parse_insert(&insert());
    // 12 classes and 68 subclasses (the insert has 68 `<subclass>`).
    assert_eq!(rows.len(), 80);
    assert_eq!(rows.iter().filter(|r| r.subclass.is_some()).count(), 68);
    let table = iso_conditions();
    assert_eq!(table.len(), rows.len());
    for row in &rows {
        let matching: Vec<_> = table.iter().filter(|c| c.code == row.code).collect();
        assert_eq!(matching.len(), 1, "{} is present once", row.code);
        let c = matching[0];
        assert_eq!(
            c.severity.letter().to_string(),
            row.category,
            "{}",
            row.code
        );
        assert_eq!(c.class, row.class, "{}", row.code);
        assert_eq!(c.subclass, row.subclass.as_deref(), "{}", row.code);
        let status = GqlStatus::from_code(&row.code).expect("constructible");
        assert!(status.is_iso());
        assert_eq!(status.code(), row.code);
    }
    // Self-closing classes do not take the next class's subclasses (the
    // regression zu pins): 08007 and 40003 keep their classes.
    let find = |code: &str| table.iter().find(|c| c.code == code).expect(code);
    assert_eq!(find("08007").class, "connection exception");
    assert_eq!(find("40003").class, "transaction rollback");
    assert!(GqlStatus::from_code("02007").is_none());
}

#[test]
fn neo4j_codes_are_distinct_from_iso_and_well_formed() {
    let mut seen = std::collections::BTreeSet::new();
    for &c in neo4j::CONDITIONS {
        assert_eq!(c.code.len(), 5);
        assert_eq!(&c.code[2..3], "N", "{} is a Neo4j code", c.code);
        assert!(seen.insert(c.code), "{} listed once", c.code);
        assert!(iso_conditions().iter().all(|i| i.code != c.code));
        let status = GqlStatus::from_code(c.code).expect("constructible");
        assert!(!status.is_iso());
        assert_eq!(status.condition(), &c);
        assert_eq!(status.severity(), Severity::Exception);
    }
    let sorted: Vec<_> = neo4j::CONDITIONS.iter().map(|r| r.code).collect();
    let mut expected = sorted.clone();
    expected.sort_unstable();
    assert_eq!(sorted, expected, "the Neo4j table is in code order");
    // The constants and the table agree.
    assert_eq!(codes::C25N11.code(), "25N11");
    assert_eq!(codes::C50N05.code(), "50N05");
    assert_eq!(codes::C51N26.code(), "51N26");
    assert_eq!(codes::C42001.code(), "42001");
    assert_eq!(
        codes::C42001.description(),
        "syntax error or access rule violation, invalid syntax"
    );
    assert_eq!(codes::C00000.severity(), Severity::Success);
    assert_eq!(codes::C02000.severity(), Severity::NoData);
}

#[test]
fn a_status_shows_its_code_class_and_description() {
    let s = codes::C22012;
    assert_eq!(
        (s.to_string(), format!("{s:?}")),
        ("22012".to_owned(), "GqlStatus(22012)".to_owned())
    );
    assert_eq!(s.class(), "22");
    assert_eq!(s.description(), "data exception, division by zero");
    // A class row has no subclass: its description is the class name.
    assert_eq!(codes::C22000.description(), "data exception");
    assert_eq!(codes::C50N05.class(), "50");
    assert_eq!(
        codes::C50N05.description(),
        "general processing exception, deadlock detected"
    );
}
