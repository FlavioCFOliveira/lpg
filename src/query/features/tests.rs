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

/// The published list of supported feature identifiers (FR-GQL-002),
/// rendered from the gate table: [`SUPPORTED`] with the insert's
/// descriptions, and the count of the codes that fail with 42001 + 51N26.
fn render_feature_list() -> String {
    let mut out = String::from(
        "---\n\
         title: Supported GQL features\n\
         status: generated\n\
         related:\n  \
         - functional-spec/functional-requirements.md\n  \
         - technical-spec/query-engine.md\n\
         ---\n\n\
         # Supported GQL features\n\n\
         <!-- Generated from src/query/features/ by the test\n     \
         `query::features::tests::published_feature_list_matches_the_gate_table`.\n     \
         Do not edit by hand: rerun it with LPG_UPDATE_FEATURE_LIST=1. -->\n\n\
         The crate supports the mandatory surface of GQL (ISO/IEC 39075:2024) and the \
         optional features below, by their identifiers in the ISO features insert. The \
         mandatory surface is every construct of the opengql/grammar `GQL.g4` that no \
         feature identifier covers; this placement rule and the list are provisional \
         ([FR-GQL-002](functional-spec/functional-requirements.md#gql), OQ-005).\n\n\
         | Feature | Description |\n|---|---|\n",
    );
    for code in SUPPORTED {
        let d = features()
            .iter()
            .find(|f| f.code == code)
            .map_or("", |f| f.description);
        out.push_str(&format!("| {code} | {d} |\n"));
    }
    let gates = features().len() - SUPPORTED.len();
    out.push_str(&format!(
        "\nThe other {gates} optional features of the insert are not supported. A request that \
         uses one fails with GQLSTATUS 42001, whose cause 51N26 names the feature \
         identifier.\n"
    ));
    out
}

#[test]
fn published_feature_list_matches_the_gate_table() {
    let list = render_feature_list();
    // The list holds exactly the supported codes, each once.
    for f in features() {
        let row = format!("| {} |", f.code);
        assert_eq!(
            list.matches(&row).count(),
            usize::from(is_supported(f.code)),
            "{}",
            f.code
        );
    }
    check_generated(
        "docs/supported-features.md",
        &list,
        "LPG_UPDATE_FEATURE_LIST",
    );
}
