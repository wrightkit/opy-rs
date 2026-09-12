//! Public compile-path coverage for macro preprocessing.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::Locale;
use workshop_rs::{Action, Value};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/synthetic/preprocessing")
}

#[test]
fn included_macro_call_reaches_canonical_workshop_output() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_with_locale(&source, "source.opy", &dir, &Locale::new("en-US"))
        .expect("the included macro must lower through the public compile path");
    let program = super::canonical_program(&artifact);
    let rule = program.rules.first().expect("fixture has one rule");
    let Action::Call { name, args } = rule.actions.first().expect("fixture has one debug action")
    else {
        panic!("macro must lower to a native HUD action");
    };
    assert_eq!(name, "createHudText");
    let Value::Call {
        name: text_name,
        args: text_args,
    } = &args[2]
    else {
        panic!("debug text must lower to a canonical value call");
    };
    assert_eq!(text_name, "customString");
    assert!(matches!(
        &text_args[1],
        Value::Number(value) if *value == 2.0
    ));
}

#[test]
fn statement_macro_expands_into_multiple_canonical_actions() {
    let source = "macro emit(value):\n    debug(value)\n    debug(value)\n\nrule \"r\":\n    @Event global\n    emit(1)\n";
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_with_locale(source, "source.opy", Path::new("."), &Locale::new("en-US"))
        .expect("statement macro must expand before WIR lowering");
    let program = super::canonical_program(&artifact);
    let rule = program.rules.first().expect("source has one rule");
    assert_eq!(
        rule.actions
            .iter()
            .filter(|action| matches!(
                action,
                Action::Call { name, .. } if name == "createHudText"
            ))
            .count(),
        2
    );
}

#[test]
fn recursive_macro_expansion_is_a_structured_diagnostic() {
    let source =
        "macro recurse():\n    recurse()\n\nrule \"r\":\n    @Event global\n    recurse()\n";
    let error = match Compiler::new().unwrap().compile_source_with_locale(
        source,
        "source.opy",
        Path::new("."),
        &Locale::new("en-US"),
    ) {
        Ok(_) => panic!("recursive macro expansion must fail"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "macro-recursion");
    assert!(error.diagnostic.span.is_some());
}

#[test]
fn macro_arity_failure_is_a_structured_diagnostic() {
    let source =
        "macro double(value):\n    debug(value)\n\nrule \"r\":\n    @Event global\n    double()\n";
    let error = match Compiler::new().unwrap().compile_source_with_locale(
        source,
        "source.opy",
        Path::new("."),
        &Locale::new("en-US"),
    ) {
        Ok(_) => panic!("macro arity failure must be reported"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "macro-arity");
    assert_eq!(error.diagnostic.span.unwrap().start.line, 6);
}

#[test]
fn failed_preprocessing_keeps_a_structured_source_diagnostic() {
    let error = match Compiler::new().unwrap().compile_source_with_locale(
        "#!include \"missing.opy\"\nrule \"r\":\n    @Event global\n    pass\n",
        "source.opy",
        Path::new("."),
        &Locale::new("en-US"),
    ) {
        Ok(_) => panic!("missing includes must fail in preprocessing"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "include-not-found");
    assert_eq!(error.diagnostic.span.unwrap().start.line, 1);
}
