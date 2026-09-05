use std::path::{Path, PathBuf};

use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::{Catalog, Locale};

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
        .join(name)
}

fn reference_wir(dir: &Path) -> workshop_rs::wir::Program {
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle is readable"),
    )
    .expect("oracle is valid JSON");
    workshop_rs::parser::parse(
        oracle["compile"]["workshop"]
            .as_str()
            .expect("oracle workshop text is present"),
        &Catalog::builtin().expect("catalog is available"),
        &Locale::new("en-US"),
    )
    .expect("oracle workshop text parses")
}

#[test]
fn compiler_semantic_wir_matches_fixtures() {
    let compiler = Compiler::new().expect("compiler initializes");
    for name in [
        "declarations-rules",
        "expressions-values",
        "receiver-playervar",
    ] {
        let dir = fixture_dir(name);
        let source = std::fs::read_to_string(dir.join("source.opy")).expect("source is readable");
        let hir = crate::compile(&source, "source.opy", &dir).expect("fixture resolves");
        let artifact = compiler.compile_hir(&hir).expect("fixture lowers");
        assert!(
            workshop_rs::roundtrip::equivalent(&artifact.wir, &reference_wir(&dir)),
            "native WIR diverged for {name}"
        );
    }
}

#[test]
fn compiler_diagnostic_contract_matches_fixture() {
    let dir = fixture_dir("diagnostics");
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source is readable");
    let report = Compiler::new()
        .expect("compiler initializes")
        .compile_source_report_with_locale(&source, "source.opy", &dir, &Locale::new("en-US"));

    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert_eq!(
        report.compile.failure_class,
        Some(CompileFailureClass::Frontend)
    );
    assert!(report.compile.workshop.is_empty());
    let diagnostic = &report.compile.diagnostics[0];
    assert_eq!(diagnostic.code, "parse-error");
    let span = diagnostic.span.as_ref().expect("diagnostic has a span");
    assert_eq!(span.path, "source.opy");
    assert_eq!((span.start.line, span.start.col), (1, 21));
}
