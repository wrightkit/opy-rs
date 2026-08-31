//! Canonical-WIR lowering coverage for issue #145.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
        .join(name)
}

fn compile_fixture(name: &str) -> crate::CompilationArtifact {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR")
}

fn assert_fixture_matches_oracle(name: &str) {
    let dir = fixture_dir(name);
    let artifact = compile_fixture(name);
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle must be readable"),
    )
    .expect("oracle must parse");
    let workshop = oracle["compile"]["workshop"]
        .as_str()
        .expect("oracle must contain Workshop output");
    let catalog = Catalog::builtin().expect("catalog must load");
    let oracle = workshop_rs::parser::parse(workshop, &catalog, &Locale::new("en-US"))
        .expect("oracle Workshop output must reparse");
    assert!(
        equivalent(&artifact.wir, &oracle),
        "issue #145 native WIR diverged for {name}\n--- native ---\n{}\n--- oracle ---\n{}",
        artifact.emitted,
        workshop
    );
}

#[test]
fn contextual_chase_lowering_matches_the_pinned_oracle() {
    let source = r#"
globalvar value

rule "chase":
    @Event global
    chase(value, 10, rate=2, ChaseReeval.NONE)
    chase(value, 10, duration=3, ChaseReeval.NONE)
"#;
    let hir =
        crate::compile(source, "issue-145-chase.opy", Path::new(".")).expect("source resolves");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("contextual chase must lower");
    assert!(
        artifact
            .emitted
            .contains("Chase Global Variable At Rate(Global.value, 10, 2, None);")
    );
    assert!(
        artifact
            .emitted
            .contains("Chase Global Variable Over Time(Global.value, 10, 3, None);")
    );
}

#[test]
fn string_modifier_lowering_matches_the_pinned_oracle() {
    assert_fixture_matches_oracle("issue-28-string-modifiers");
}

#[test]
fn broad_supported_syntax_lowering_reaches_canonical_wir() {
    let artifact = compile_fixture("issue-28-syntax");
    assert!(artifact.emitted.contains("Loop If(Not(Array Contains"));
    assert!(artifact.emitted.contains("Mapped Array"));
    assert!(artifact.emitted.contains("Custom String(\"ｗｉｄｅ\")"));
    assert!(artifact.emitted.contains("Set Global Variable(value, 1);"));
}

#[test]
fn literal_dict_lookup_lowering_matches_the_pinned_oracle() {
    assert_fixture_matches_oracle("issue-46-unsupported");
}

#[test]
fn null_initializers_are_dropped_but_float_zero_is_preserved() {
    let source = r#"
globalvar null_value = null
globalvar integer_zero = 0
globalvar float_zero = 0.0

rule "initializer contract":
    @Event global
    pass
"#;
    let hir = crate::compile(source, "issue-145.opy", Path::new(".")).expect("source resolves");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("initializers must lower");

    assert!(!artifact.emitted.contains("Set Global Variable(null_value,"));
    assert!(
        !artifact
            .emitted
            .contains("Set Global Variable(integer_zero,")
    );
    assert!(artifact.emitted.contains("Set Global Variable(float_zero,"));
}
