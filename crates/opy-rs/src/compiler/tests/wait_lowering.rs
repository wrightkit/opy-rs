use std::{collections::BTreeMap, path::Path};

use crate::Compiler;

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
            .matches("Wait(False, Ignore Condition);")
            .count(),
        2
    );
    assert!(!artifact.emitted.contains("Wait(0.016, Ignore Condition);"));
}
