//! Oracle-constrained #46 differential evidence (issue #46).
//!
//! The pinned OverPy oracle snapshot for the `synthetic/issue-46-primitives`
//! fixture is load-bearing for the native compiler: this suite compiles the
//! fixture source through the full native pipeline (frontend → OPY HIR →
//! canonical WIR → deterministic en-US emission). The native lowering is
//! compared directly with the oracle's parsed canonical WIR.
//!
//! The adjacent `synthetic/issue-46-unsupported` fixture exercises the
//! literal-dictionary lookup form that is now folded during OPY lowering.

use std::path::Path;

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
        .join(name)
}

fn oracle_workshop(dir: &Path) -> String {
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle.json must be readable"),
    )
    .expect("oracle.json must parse");
    oracle["compile"]["workshop"]
        .as_str()
        .expect("oracle snapshot records the compiled Workshop text")
        .to_string()
}

fn compile_fixture(dir: &Path) -> crate::CompilationArtifact {
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", dir).expect("fixture must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR")
}

#[test]
fn issue_46_native_wir_matches_the_pinned_oracle() {
    let dir = fixture_dir("issue-46-primitives");
    let artifact = compile_fixture(&dir);
    let catalog = Catalog::builtin().expect("catalog must load");
    let locale = Locale::new("en-US");

    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale)
        .expect("the pinned oracle Workshop text must reparse");

    assert!(equivalent(&artifact.wir, &oracle));
}

#[test]
fn issue_46_literal_dict_lookup_matches_the_pinned_oracle() {
    let dir = fixture_dir("issue-46-unsupported");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir)
        .expect("the frontend resolves the negative fixture");
    let artifact = Compiler::new()
        .unwrap()
        .compile_hir(&hir)
        .expect("literal dict lookup should lower to canonical WIR");
    let catalog = Catalog::builtin().expect("catalog must load");
    let oracle =
        workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &Locale::new("en-US"))
            .expect("the pinned oracle Workshop text must reparse");
    assert!(equivalent(&artifact.wir, &oracle));
}
