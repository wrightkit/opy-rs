//! Public project-composition coverage for issue #161.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::compile;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue-161-project")
}

#[test]
fn directory_includes_are_sorted_and_preserve_nested_source_ownership() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("main.opy")).unwrap();
    let hir = compile(&source, "main.opy", &dir).expect("directory include must compile");

    let paths = hir
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "main.opy",
            "modules/a.opy",
            "modules/nested/first.opy",
            "modules/nested/scripted.opy",
            "modules/rules.opy",
        ]
    );
    let names = hir
        .rules
        .iter()
        .filter_map(|entry| match entry {
            crate::hir::RuleEntry::Rule(rule) => Some(rule.name.as_str()),
            crate::hir::RuleEntry::SubroutineDef { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["nested", "a", "rules", "main"]);
}

#[test]
fn included_script_macro_resolves_relative_to_its_definition_file() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("main.opy")).unwrap();
    let source = format!("{source}\nrule \"script\":\n    @Event global\n    A = addFive(1)\n");
    let hir = compile(&source, "main.opy", &dir).expect("included script macro must compile");
    assert!(hir.dump().contains("assign A = 6"), "{}", hir.dump());
}

#[test]
fn included_script_macro_errors_keep_the_resolved_script_provenance() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("main.opy")).unwrap();
    let source = format!("{source}\nrule \"script error\":\n    @Event global\n    A = fail(1)\n");
    let error = crate::compile(&source, "main.opy", &dir).expect_err("script must fail");
    assert_eq!(error.code, "script-error");
    assert!(
        error.message.contains("modules/nested/scripts/fail.js"),
        "{}",
        error.message
    );
}

#[test]
fn included_script_macro_reads_an_open_document_overlay() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("main.opy")).unwrap();
    let source =
        format!("{source}\nrule \"script overlay\":\n    @Event global\n    A = addFive(1)\n");
    let overlay = BTreeMap::from([(
        "modules/nested/scripts/add.js".to_string(),
        "(value + 7).toString();".to_string(),
    )]);
    let hir = crate::compile_with_overlay(&source, "main.opy", &dir, &overlay)
        .expect("included script macro must read the overlay");
    assert!(hir.dump().contains("assign A = 8"), "{}", hir.dump());
}

#[test]
fn empty_macro_replacement_is_a_preprocessing_error() {
    let error = crate::preprocess::preprocess("#!define EMPTY\n", "main.opy", Path::new("."))
        .expect_err("empty macro replacements must be rejected");
    assert_eq!(error.code, "define-invalid");
}
