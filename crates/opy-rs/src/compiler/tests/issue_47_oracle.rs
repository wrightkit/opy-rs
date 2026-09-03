//! Oracle-backed control-flow lowering evidence for issue #47.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::Action;

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

fn assert_native_wir_equivalent(name: &str) {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(
        equivalent(&artifact.wir, &oracle),
        "native WIR diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn issue_47_switch_lowering_matches_the_pinned_oracle() {
    for name in [
        "issue-33-switch-break",
        "issue-47-control-flow",
        "issue-47-switch-order",
        "issue-47-switch-structured-target",
    ] {
        assert_native_wir_equivalent(name);
    }
}

#[test]
fn issue_47_do_while_break_shapes_match_the_pinned_oracle() {
    assert_native_wir_equivalent("issue-47-do-while-shapes");
}

#[test]
fn issue_47_multiple_switch_breaks_use_shared_switch_exit_layers() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-switch-multiple-break");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let artifact = compiler
        .compile_hir(&hir)
        .expect("multi-break switch must lower");
    let rule = artifact
        .wir
        .rules
        .iter()
        .find(|rule| rule.name == "issue 47 switch multiple break")
        .expect("fixture rule must be present");
    let Action::If {
        branches,
        else_body: Some(else_body),
        ..
    } = artifact.wir.actions.get(rule.actions[0]).unwrap()
    else {
        panic!("multi-break switch must have an outer switch-exit layer")
    };
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].body.len(), 2);
    assert_eq!(else_body.len(), 1);

    let Action::If {
        branches,
        else_body: Some(else_body),
        ..
    } = artifact.wir.actions.get(else_body[0]).unwrap()
    else {
        panic!("the later case break must have its own exit layer")
    };
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].body.len(), 1);
    assert_eq!(else_body.len(), 2);
    assert!(artifact.emitted.contains("Array(6, 0, 2, 5)"));
}

#[test]
fn issue_47_invalid_do_while_placement_is_source_attributed() {
    let dir = fixture_dir("issue-47-do-while-invalid-placement");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let error = crate::compile(&source, "source.opy", &dir)
        .expect_err("invalid do-while placement must be rejected");
    assert_eq!(error.code, "do-while-placement");
    assert_eq!(error.span.unwrap().start.line, 6);
}

#[test]
fn issue_47_nested_switch_break_is_source_attributed_when_not_representable() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-unsupported");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let error = match compiler.compile_hir(&hir) {
        Ok(_) => panic!("nested switch break unexpectedly lowered"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(error.diagnostic.span.unwrap().start.line, 7);
}
