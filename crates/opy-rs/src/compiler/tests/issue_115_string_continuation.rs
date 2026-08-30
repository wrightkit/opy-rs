//! Canonical compiler coverage for issue #115.

use std::path::Path;

use crate::Compiler;
use workshop_rs::catalog::Locale;
use workshop_rs::wir::{Action, RuleId, Value};

#[test]
fn backslash_continued_string_reaches_the_expected_canonical_value() {
    let source = r#"rule "backslash string continuation":
    @Event global
    debug(("one\n"\
        "two"))
"#;
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_with_locale(source, "source.opy", Path::new("."), &Locale::new("en-US"))
        .expect("backslash-continued string must compile");

    let rule = artifact
        .wir
        .rules
        .get(RuleId::from_index(0))
        .expect("source has one rule");
    let Action::Call { args, .. } = artifact
        .wir
        .actions
        .get(rule.actions[0])
        .expect("source has one debug action")
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
        Value::String(value) if value == "one\ntwo"
    ));
}
