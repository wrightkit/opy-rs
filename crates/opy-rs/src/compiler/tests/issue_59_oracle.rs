//! Oracle-backed postfix increment/decrement assignment evidence for issue #59.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
        .join(name)
}

fn oracle_workshop(dir: &Path) -> String {
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle.json must be readable"),
    )
    .expect("oracle snapshot must parse");
    oracle["compile"]["workshop"]
        .as_str()
        .expect("oracle snapshot must contain Workshop text")
        .to_string()
}

#[test]
fn issue_59_postfix_assignments_match_the_pinned_oracle() {
    let dir = fixture_dir("issue-59-postfix-assignment");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(equivalent(&artifact.wir, &oracle), "{}", artifact.emitted);
}

fn assert_rejected_postfix_fixture(name: &str, line: u32, col: u32) {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let error = crate::compile(&source, "source.opy", &dir)
        .expect_err("rejected postfix form must remain rejected");

    assert_eq!(error.code, "parse-error");
    let span = error.span.expect("diagnostic must be source-attributed");
    assert_eq!(span.start.line, line);
    assert_eq!(span.start.col, col);
}

#[test]
fn issue_59_rejected_prefix_increment_form_has_stable_source_diagnostic() {
    assert_rejected_postfix_fixture("issue-59-postfix-negative", 6, 5);
}

#[test]
fn issue_59_rejected_embedded_postfix_form_has_stable_source_diagnostic() {
    assert_rejected_postfix_fixture("issue-59-embedded-postfix-negative", 6, 20);
}
