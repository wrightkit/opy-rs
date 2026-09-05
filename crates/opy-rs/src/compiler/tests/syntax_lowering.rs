//! Canonical-WIR coverage for the supported syntax lowering surface.

use std::path::{Path, PathBuf};

use crate::Compiler;

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

#[test]
fn broad_supported_syntax_lowering_reaches_canonical_wir() {
    let artifact = compile_fixture("syntax-surface");
    assert!(artifact.emitted.contains("Loop If(Not(Array Contains"));
    assert!(artifact.emitted.contains("Mapped Array"));
    assert!(artifact.emitted.contains("Custom String(\"ｗｉｄｅ\")"));
    assert!(artifact.emitted.contains("Set Global Variable(value, 1);"));
}
