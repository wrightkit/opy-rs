//! Wait lowering against the pinned OverPy default and size-optimization semantics.

use std::path::Path;

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
fn default_wait_remains_concrete_without_size_optimization() {
    let source = "rule \"default wait\":\n    @Event global\n    wait()\n";
    let hir =
        crate::compile(source, "default-wait.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("source must lower to canonical WIR");

    assert!(artifact.emitted.contains("Wait(0.016, Ignore Condition);"));
}
