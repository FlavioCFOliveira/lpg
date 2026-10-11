//! Drift test of the feature table and checks of the gates (rmp #43).
//!
//! The generator is ported from zu @ `a821319` (Apache-2.0)
//! `crates/zu-cli/tests/statement.rs`: it reads every `<feature>` of the
//! insert, folds the whitespace of its description and renders the table.

use super::*;
use crate::gqlstatus::tests::{check_generated, rustfmt};

fn insert() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-features.xml"),
    )
    .expect("the features insert")
}

/// The text of each `<tag>` element of `body`, in order.
fn elements(body: &str, tag: &str) -> Vec<String> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let end = after.find(&close).expect("the element closes");
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

/// `(code, description)` of every feature, descriptions with `<squote/>`
/// as `'` and whitespace folded, in code order.
fn rows() -> Vec<(String, String)> {
    let xml = insert();
    let body = &xml[xml.find("<features>").expect("the features element")..];
    let mut rows: Vec<(String, String)> = elements(body, "feature")
        .iter()
        .map(|f| {
            let code = elements(f, "code").remove(0).trim().to_string();
            let d = elements(f, "description").remove(0);
            let d = d.replace("<squote/>", "'").replace("<squote />", "'");
            (code, d.split_whitespace().collect::<Vec<_>>().join(" "))
        })
        .collect();
    rows.sort();
    rows
}

fn render(rows: &[(String, String)]) -> String {
    let mut out = String::from(
        "//! Generated from `docs/standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-features.xml`.\n\
         //! Do not edit by hand: run the drift test in `tests.rs` with\n\
         //! `LPG_UPDATE_FEATURES=1`.\n\n\
         use super::Feature;\n\n\
         /// Every optional feature of ISO/IEC 39075:2024, in code order.\n\
         pub(super) static FEATURES: &[Feature] = &[\n",
    );
    for (code, d) in rows {
        let d = d.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!(
            "    Feature {{ code: \"{code}\", description: \"{d}\" }},\n"
        ));
    }
    out.push_str("];\n");
    out
}

#[test]
fn generated_table_matches_the_insert() {
    let rows = rows();
    assert_eq!(rows.len(), 228, "the insert lists 228 features");
    check_generated(
        "src/query/features/generated.rs",
        &rustfmt(&render(&rows)),
        "LPG_UPDATE_FEATURES",
    );
}

#[test]
fn every_code_is_supported_or_a_gate_and_the_supported_codes_exist() {
    let all = features();
    assert_eq!(all.len(), 228);
    for code in SUPPORTED {
        assert!(
            all.iter().any(|f| f.code == code),
            "{code} is in the insert"
        );
    }
    let mut gates = 0;
    for f in all {
        match gate(f.code) {
            Ok(()) => assert!(is_supported(f.code)),
            Err(e) => {
                gates += 1;
                assert_eq!(e.code(), "42001");
                let cause = e.cause().expect("a cause");
                assert_eq!(cause.code(), "51N26");
                assert!(cause.message().contains(f.code), "{}", cause.message());
            }
        }
    }
    assert_eq!(gates, 214);
}
