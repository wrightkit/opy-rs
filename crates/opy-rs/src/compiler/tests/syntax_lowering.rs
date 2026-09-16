//! Canonical-WIR coverage for the supported syntax lowering surface.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
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

#[test]
fn broad_supported_syntax_lowering_reaches_canonical_wir() {
    let artifact = compile_fixture("syntax-surface");
    assert!(artifact.emitted.contains("Loop If(Not(Array Contains"));
    assert!(artifact.emitted.contains("Mapped Array"));
    assert!(artifact.emitted.contains("Custom String(\"ｗｉｄｅ\")"));
    assert!(artifact.emitted.contains("Set Global Variable(value, 1);"));
}

#[test]
fn syntax_surface_fixture_matches_the_pinned_canonical_wir() {
    let artifact = compile_fixture("syntax-surface");
    let dir = fixture_dir("syntax-surface");
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle.json must be readable"),
    )
    .expect("oracle.json must parse");
    let oracle_wir = workshop_rs::parser::parse(
        oracle["compile"]["workshop"]
            .as_str()
            .expect("compile.workshop must exist"),
        &Catalog::builtin().expect("catalog must load"),
        &Locale::new("en-US"),
    )
    .expect("oracle workshop must parse");
    assert!(equivalent(&artifact.wir, &oracle_wir));
}

#[test]
fn literal_membership_folds_under_optimization() {
    let source = r#"globalvar a
globalvar b
globalvar c
globalvar d

rule "membership":
    @Event global
    a = 1 in [1, 2]
    b = 3 in [1, 2]
    c = 1 not in [1, 2]
    d = 3 not in [1, 2]
"#;
    let hir = crate::compile(source, "membership.opy", Path::new(".")).expect("source resolves");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("hir lowers");
    assert!(artifact.emitted.contains("Set Global Variable(a, True);"));
    assert!(artifact.emitted.contains("Set Global Variable(b, False);"));
    assert!(artifact.emitted.contains("Set Global Variable(c, False);"));
    assert!(artifact.emitted.contains("Set Global Variable(d, True);"));
}
