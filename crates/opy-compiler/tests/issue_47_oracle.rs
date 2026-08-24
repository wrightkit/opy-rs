//! Oracle-backed control-flow lowering evidence for issue #47.

use std::path::{Path, PathBuf};

use opy_compiler::Compiler;
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
fn issue_47_control_flow_matches_the_pinned_oracle() {
    let dir = fixture_dir("issue-47-control-flow");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = opy_frontend::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let native = workshop_rs::parser::parse(&artifact.emitted, &catalog, &locale).unwrap();
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(
        equivalent(&native, &oracle),
        "native lowering diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn issue_47_nested_switch_break_matches_the_pinned_oracle() {
    let dir = fixture_dir("issue-33-switch-break");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = opy_frontend::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let native = workshop_rs::parser::parse(&artifact.emitted, &catalog, &locale).unwrap();
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(
        equivalent(&native, &oracle),
        "nested native lowering diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn issue_47_switch_order_matches_the_pinned_oracle() {
    let dir = fixture_dir("issue-47-switch-order");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = opy_frontend::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let native = workshop_rs::parser::parse(&artifact.emitted, &catalog, &locale).unwrap();
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(
        equivalent(&native, &oracle),
        "switch order diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn issue_47_do_while_break_shapes_match_the_pinned_oracle() {
    let dir = fixture_dir("issue-47-do-while-shapes");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = opy_frontend::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let native = workshop_rs::parser::parse(&artifact.emitted, &catalog, &locale).unwrap();
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(
        equivalent(&native, &oracle),
        "do-while break shapes diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn issue_47_multiple_switch_breaks_are_not_silently_dropped() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-switch-multiple-break");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = opy_frontend::compile(&source, "source.opy", &dir).unwrap();
    let error = match compiler.compile_hir(&hir) {
        Ok(_) => panic!("multi-break switch must not be silently truncated"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(error.diagnostic.span.unwrap().start.line, 11);
}

#[test]
fn issue_47_invalid_do_while_placement_is_source_attributed() {
    let dir = fixture_dir("issue-47-do-while-invalid-placement");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let error = opy_frontend::compile(&source, "source.opy", &dir)
        .expect_err("invalid do-while placement must be rejected");
    assert_eq!(error.code, "do-while-placement");
    assert_eq!(error.span.unwrap().start.line, 6);
}

#[test]
fn issue_47_nested_switch_break_is_source_attributed_when_not_representable() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-unsupported");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = opy_frontend::compile(&source, "source.opy", &dir).unwrap();
    let error = match compiler.compile_hir(&hir) {
        Ok(_) => panic!("nested switch break unexpectedly lowered"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(error.diagnostic.span.unwrap().start.line, 7);
}
