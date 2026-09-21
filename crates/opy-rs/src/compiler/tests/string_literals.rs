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

#[test]
fn unicode_escape_in_subroutine_name_reaches_canonical_workshop() {
    // Minimized from OWBastion/Bastion commit
    // f95d3159effed8aef0747a9fac5495e0651895b6,
    // Bastion/src/utilities/system/savePlayerData.opy:9.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/regressions/bastion-unicode-name.opy");
    let source = std::fs::read_to_string(&fixture).expect("Unicode regression must be readable");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_source_with_locale(
            &source,
            "bastion-unicode-name.opy",
            fixture.parent().expect("fixture has a parent"),
            &Locale::new("en-US"),
        )
        .expect("Unicode source name must compile");

    let program = super::canonical_program(&artifact);
    assert_eq!(program.rules.len(), 2);
    assert_eq!(
        program.rules[0].name,
        "Subroutine save player data if the player has passed round 2"
    );
    assert_eq!(program.rules[1].name, "Subroutine literal ufeff");
    assert!(artifact.emitted.contains("passed round 2"));
    assert!(!artifact.emitted.contains('\u{feff}'));
    assert!(artifact.emitted.contains("literal ufeff"));
}

#[test]
fn rule_name_formatting_strip_preserves_unrelated_unicode() {
    let source = "rule \"中文 é 😀\":\n    @Event global\n    debug(\"ok\")\n";
    let hir = crate::compile(source, "unicode.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert_eq!(artifact.wir.rules[0].name, "中文 é 😀");
    assert!(artifact.emitted.contains("中文 é 😀"));
}

#[test]
fn format_folds_constant_arguments_without_nested_string_chunks() {
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = \"Hold {}: {}% {} {} {} {}\".format(Button.RELOAD, 49 - 0, 2, 3, 4, 5)\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(g, Custom String(\"Hold {0}: 49% 2 3 4 5\", Reload));")
    );
    assert!(!artifact.emitted.contains("Custom String(\"{0}{1}\""));
}

#[test]
fn named_string_entities_reach_canonical_custom_strings() {
    let source = r#"rule "entities":
    @Event global
    print("a\&black_square;b\&fullwidth_space;c")
"#;
    let hir = crate::compile(source, "entities.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(artifact.emitted.contains("Custom String(\"a■b　c\")"));
}

#[test]
fn named_string_entities_reach_formatted_custom_strings() {
    let source = r#"globalvar value
rule "formatted entities":
    @Event global
    value = f"\&black_square; {getMatchTime()}"
"#;
    let hir = crate::compile(source, "formatted-entities.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(
        artifact
            .emitted
            .contains("Custom String(\"■ {0}\", Match Time)")
    );
}
