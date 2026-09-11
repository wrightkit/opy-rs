//! Canonical compiler coverage for string literals.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::Locale;
use workshop_rs::{Action, Value};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/real-world/overpy-inputhud")
}

#[test]
fn minimized_regression_reaches_canonical_debug_string() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("regressions/implicit-string-concatenation.opy"))
        .expect("minimized regression must be readable");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_source_with_locale(&source, "regression.opy", &dir, &Locale::new("en-US"))
        .expect("adjacent string literals must compile");

    let program = super::canonical_program(&artifact);
    let rule = program.rules.first().expect("regression has one rule");
    let Action::Call { args, .. } = rule
        .actions
        .first()
        .expect("regression has one debug action")
    else {
        panic!("debug must lower to a native HUD action");
    };
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
        Value::String(value) if value == "onetwo"
    ));
}
