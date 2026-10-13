use std::path::{Path, PathBuf};

use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::{Catalog, Locale};

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

fn reference_wir(dir: &Path) -> workshop_rs::Program {
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
            workshop_rs::roundtrip::equivalent(
                &super::canonical_program(&artifact),
                &reference_wir(&dir),
            ),
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

#[test]
fn boolean_comparison_fold_keeps_the_pinned_literal_domain() {
    // `astIsLiteral` in the reference marks numbers, strings, literal
    // vectors and enum constants literal — but not `true`/`false`, `null`,
    // or arrays. `1 == 2` folds to `False` by the numeric rule, yet the
    // outer `False == 3` must stay `Compare(False, ==, 3)` upstream.
    let source =
        "globalvar g\nrule \"r\":\n    @Event global\n    g = 1 == 2 == 3\n    g = 3 != false\n";
    let hir = crate::compile(source, "cmp.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact.emitted.contains("Compare(False, ==, 3)"),
        "{}",
        artifact.emitted
    );
    assert!(
        artifact.emitted.contains("Compare(3, !=, False)"),
        "{}",
        artifact.emitted
    );
}

#[test]
fn membership_fold_matches_the_pinned_reference() {
    // `__arrayContains__` upstream: an `areAstsAlwaysEqual` hit folds true
    // (numbers by AST spelling, so `1` never matches `1.0`); a literal
    // needle filters literal elements and folds `False` on empty or an
    // `Equals` on a single survivor. Booleans and `null` are not literal.
    let source = concat!(
        "globalvar x\nglobalvar g\nrule \"r\":\n    @Event global\n",
        "    g = 1 in [1.0]\n",
        "    g = 1 in [1.0, x]\n",
        "    g = false in [1, 2, 3]\n",
        "    g = x in []\n",
        "    g = vect(1, 2, 3) in [vect(1, 2, 4), x]\n",
        "    g = 2 in [x, 3]\n",
    );
    let hir = crate::compile(source, "in.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    for expected in [
        "Set Global Variable(g, False)",
        "Set Global Variable(g, Compare(1, ==, Global.x))",
        "Set Global Variable(g, Array Contains(Array(1, 2, 3), False))",
        "Set Global Variable(g, False)",
        "Set Global Variable(g, Compare(Vector(1, 2, 3), ==, Global.x))",
        "Set Global Variable(g, Compare(2, ==, Global.x))",
    ] {
        assert!(
            artifact.emitted.contains(expected),
            "{expected}\n{}",
            artifact.emitted
        );
    }
}

#[test]
fn enum_members_resume_counting_after_explicit_values() {
    // Upstream `__enum__`: a member without `=` takes the last numeric
    // value plus one, so `ON = 5; AUTO` is `6`, and `1 + 2` breaks the
    // chain outright rather than folding to `3` (#512).
    let source = concat!(
        "enum Mode:\n    OFF = 0\n    ON = 5\n    AUTO\n",
        "globalvar g\nrule \"r\":\n    @Event global\n    g = Mode.AUTO\n    g = Mode.ON + 1\n",
    );
    let hir = crate::compile(source, "enum.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact.emitted.contains("Set Global Variable(g, 6);"),
        "{}",
        artifact.emitted
    );
}

#[test]
fn enum_auto_increment_rejects_a_non_numeric_predecessor() {
    let source = "enum E:\n    X = 1 + 2\n    Y\nrule \"r\":\n    @Event global\n    pass\n";
    let error = crate::compile(source, "enum.opy", Path::new(".")).unwrap_err();
    assert!(error.message.contains("auto-increment"), "{error:?}");
}
