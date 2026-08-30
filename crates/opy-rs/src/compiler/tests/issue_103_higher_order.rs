//! Oracle-backed coverage for the bounded higher-order array lowering in #103.

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

fn compile_source(source: &str) -> crate::CompilationArtifact {
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("source must lower to canonical WIR")
}

fn oracle_wir(name: &str) -> workshop_rs::wir::Program {
    let dir = fixture_dir(name);
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle must be readable"),
    )
    .expect("oracle must parse");
    workshop_rs::parser::parse(
        oracle["compile"]["workshop"]
            .as_str()
            .expect("oracle must contain Workshop output"),
        &Catalog::builtin().expect("catalog must load"),
        &Locale::new("en-US"),
    )
    .expect("oracle Workshop must reparse")
}

#[test]
fn demonstrated_comprehension_lowers_to_canonical_mapped_array() {
    let artifact = compile_source(
        "globalvar value\nrule \"r\":\n    @Event global\n    value = [item * 2 for item in [1, 2]]\n",
    );
    let expected = workshop_rs::parser::parse(
        "variables {\n    global:\n        0: value\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(value, Mapped Array(Array(1, 2), Multiply(Current Array Element, 2)));\n    }\n}\n",
        &Catalog::builtin().expect("catalog must load"),
        &Locale::new("en-US"),
    )
    .expect("expected canonical WIR must reparse");
    assert!(
        equivalent(&artifact.wir, &expected),
        "native WIR diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn demonstrated_sorted_array_form_matches_the_pinned_oracle() {
    let artifact = compile_fixture("issue-33-f-string");
    assert!(
        equivalent(&artifact.wir, &oracle_wir("issue-33-f-string")),
        "native WIR diverged for issue-33-f-string\n{}",
        artifact.emitted
    );
}

#[test]
fn unsupported_sorted_key_shape_remains_source_attributed() {
    let source =
        "globalvar value\nrule \"r\":\n    @Event global\n    value = sorted([1, 2], key=1)\n";
    let hir = crate::compile(source, "sorted.opy", Path::new(".")).expect("source must resolve");
    let error = match Compiler::new()
        .expect("compiler must load")
        .compile_hir(&hir)
    {
        Ok(_) => panic!("non-lambda sorted key unexpectedly compiled"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(
        error
            .diagnostic
            .span
            .expect("error must have a span")
            .start
            .line,
        4
    );
}
