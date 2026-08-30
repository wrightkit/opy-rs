//! Canonical compiler coverage for issue #101.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::Locale;
use workshop_rs::wir::{Action, RuleId, Value};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/real-world/overpy-inputhud")
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

    let rule = artifact
        .wir
        .rules
        .get(RuleId::from_index(0))
        .expect("regression has one rule");
    let Action::Call { args, .. } = artifact
        .wir
        .actions
        .get(rule.actions[0])
        .expect("regression has one debug action")
    else {
        panic!("debug must lower to a native HUD action");
    };
    let Value::Call {
        name: text_name,
        args: text_args,
    } = &artifact.wir.values.get(args[2]).unwrap().value
    else {
        panic!("debug text must lower to a canonical value call");
    };
    assert_eq!(text_name, "customString");
    assert!(matches!(
        &artifact.wir.values.get(text_args[1]).unwrap().value,
        Value::String(value) if value == "onetwo"
    ));
}
