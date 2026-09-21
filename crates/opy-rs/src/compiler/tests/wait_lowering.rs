//! Wait lowering against the pinned OverPy default and size-optimization semantics.

use std::{collections::BTreeMap, path::Path};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/synthetic/wait-optimization")
}

#[test]
fn optimized_wait_forms_match_the_pinned_oracle() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR");
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle.json must be readable"),
    )
    .expect("oracle.json must parse");
    let oracle_workshop = oracle["compile"]["workshop"]
        .as_str()
        .expect("oracle snapshot records the compiled Workshop text");
    let oracle_wir = workshop_rs::parser::parse(
        oracle_workshop,
        &Catalog::builtin().expect("catalog must load"),
        &Locale::new("en-US"),
    )
    .expect("the pinned oracle Workshop text must reparse");

    assert!(equivalent(&artifact.wir, &oracle_wir));
    assert_eq!(
        artifact
            .emitted
            .matches("Wait(0, Ignore Condition);")
            .count(),
        2
    );
    assert!(artifact.emitted.contains("Wait(1, Ignore Condition);"));
    assert!(artifact.emitted.contains("Wait(1, Abort When False);"));
}

#[test]
fn disabled_optimizations_keep_default_wait_concrete() {
    let source = "#!disableOptimizations\nrule \"default wait\":\n    @Event global\n    wait()\n";
    let hir =
        crate::compile(source, "default-wait.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("source must lower to canonical WIR");

    assert!(artifact.emitted.contains("Wait(0.016, Ignore Condition);"));
}

#[test]
fn optimized_for_loop_is_meaningful_without_body_actions() {
    let source = "globalvar index\nrule \"for loop\":\n    @Event global\n    for index in range(0, 1):\n        pass\n";
    let hir = crate::compile(source, "for-loop.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("source must lower to canonical WIR");
    let program = super::canonical_program(&artifact);

    assert_eq!(program.rules.len(), 1);
    assert!(matches!(
        program.rules[0].actions.first(),
        Some(workshop_rs::Action::ForGlobalVariable { .. })
    ));
}

#[test]
fn included_size_optimization_applies_to_following_rules() {
    let hir = crate::compile_with_overlay(
        "globalvar marker\n\n#!include \"child.opy\"\n\nrule \"root\":\n    @Event global\n    marker = 1\n    wait()\n    marker = 2\n",
        "source.opy",
        Path::new("."),
        &BTreeMap::from([(
            String::from("child.opy"),
            String::from(
                "#!optimizeForSize\n\nrule \"child\":\n    @Event global\n    marker = 3\n    wait()\n    marker = 4\n",
            ),
        )]),
    )
    .expect("included optimization source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("included optimization source must lower");

    assert_eq!(
        artifact
            .emitted
            .matches("Wait(0, Ignore Condition);")
            .count(),
        2
    );
    assert!(!artifact.emitted.contains("Wait(0.016, Ignore Condition);"));
}
