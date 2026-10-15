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
    // The reference splices literal string arguments into the debug format
    // text, so the concatenated literal must appear inline after `= `.
    assert!(matches!(
        &text_args[0],
        Value::String(value) if value.contains("= onetwo")
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
    // The names matter here, so keep the otherwise empty subroutines.
    let source = format!("#!disableOptimizations\n{source}");
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

    let rule = artifact
        .wir
        .rules
        .iter()
        .find(|rule| rule.name == "r")
        .expect("string-format rule must lower");
    assert!(matches!(
        rule.actions.first(),
        Some(Action::SetGlobalVariable {
            variable,
            value: Value::Call { name, args },
        }) if variable == "g"
            && name == "customString"
            && matches!(
                args.as_slice(),
                [Value::String(text), Value::Enum { value_type, value }]
                    if text == "Hold {0}: 49% 2 3 4 5"
                        && value_type == "Button"
                        && value == "RELOAD"
            )
    ));
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
fn modifier_format_binds_the_surviving_formatters() {
    // opy-rs#530: `.format` on a `w`/`b`/`c` literal binds the `{n}`/`{}`
    // formatters that survive the text rewrite, like a plain string.
    let source =
        "globalvar g\nrule \"r\":\n    @Event global\n    g = w\"a{0}b\".format(getMatchTime())\n";
    let hir = crate::compile(source, "issue-530.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(g, Custom String(\"ａ{0}ｂ\", Match Time))")
    );
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

#[test]
fn string_addition_folds_to_a_canonical_string() {
    // Documented deviation: the reference emits
    // `Add(Custom String, Custom String)` for `"a" + "b"`, a call the
    // canonical catalog's `add` domain rejects. opy-rs folds to the literal
    // instead, which stays canonical and produces the same runtime text.
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g.append(\"x\" + \"y\")\n";
    let hir = crate::compile(source, "concat.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact.emitted.contains("Custom String(\"xy\")"),
        "{}",
        artifact.emitted
    );
}

#[test]
fn string_multiplication_folds_to_a_canonical_string() {
    // Documented deviation: the reference emits
    // `Multiply(Custom String("a"), 3)` for `"a" * 3`, a call the canonical
    // catalog's `multiply` domain rejects. opy-rs folds the repetition to a
    // literal instead, which stays canonical and produces the same runtime
    // text.
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = \"a\" * 3\n    g = \"ab\" * 0\n    g = 2 * \"z\"\n";
    let hir = crate::compile(source, "strmul.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact.emitted.contains("Custom String(\"aaa\")"),
        "{}",
        artifact.emitted
    );
    assert!(
        artifact.emitted.contains("Custom String(\"\")"),
        "{}",
        artifact.emitted
    );
    assert!(
        artifact.emitted.contains("Custom String(\"zz\")"),
        "{}",
        artifact.emitted
    );
}

#[test]
fn debug_splices_a_plain_string_argument_like_the_pinned_reference() {
    // `debug("lit")` merges the `__customString__` argument into the HUD
    // label upstream instead of leaving a `{0}` slot.
    let source = "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    debug(\"lit\")\n";
    let hir = crate::compile(source, "debug-literal.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains("= lit"), "{}", artifact.emitted);
    assert!(!artifact.emitted.contains("= {0}"), "{}", artifact.emitted);
}

#[test]
fn unbound_format_placeholders_in_plain_strings_are_an_error() {
    // The pinned `__customString__` check rejects `{n}`/`{}` formatters when
    // the string carries fewer arguments than required.
    for literal in ["{0}", "{}", "a{9}b", "a{}{}b"] {
        let source =
            format!("globalvar g\nrule \"r\":\n    @Event global\n    g = \"{literal}\"\n");
        let hir = crate::compile(&source, "unbound.opy", Path::new(".")).unwrap();
        let result = Compiler::new().unwrap().compile_hir(&hir);
        assert!(result.is_err(), "\"{literal}\" must be rejected");
    }
}

#[test]
fn format_arity_mismatches_are_an_error() {
    // `.format` fails when `{n}` indexes an argument that does not exist,
    // when `{}` does not get exactly one argument each, and when numbered
    // and unnumbered formatters mix.
    for (text, count) in [
        ("\"x{0}{1}y\"", 1),
        ("\"x{2}y\"", 1),
        ("\"x{}y\"", 2),
        ("\"a{2}{}\"", 4),
    ] {
        let args = (0..count)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let source =
            format!("globalvar g\nrule \"r\":\n    @Event global\n    g = {text}.format({args})\n");
        let hir = crate::compile(&source, "format-arity.opy", Path::new(".")).unwrap();
        let result = Compiler::new().unwrap().compile_hir(&hir);
        assert!(result.is_err(), "{text}.format({args}) must be rejected");
    }
}

#[test]
fn format_arity_huge_placeholder_rejects_without_overflow() {
    // A numbered placeholder at or beyond the `usize` range — including one
    // too large to parse, which maps to usize::MAX — overflows `max + 1`.
    // The oracle computes expected = max + 1 as a JS number and prints its
    // f64 form; the check compares `arg_count <= max` so no arithmetic
    // overflow occurs and the diagnostic still rejects the call.
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = \"{18446744073709551615}x\".format()\n";
    let hir = crate::compile(source, "format-overflow.opy", Path::new(".")).unwrap();
    let error = Compiler::new()
        .unwrap()
        .compile_hir(&hir)
        .err()
        .expect("a usize::MAX placeholder must be rejected");
    assert!(
        error
            .to_string()
            .contains("Too few arguments in .format() function"),
        "the rejection must be the arity diagnostic, got: {error}"
    );
    // A 20+-digit placeholder that cannot even parse as usize rejects the
    // same way instead of silently skipping the check.
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = \"{99999999999999999999}x\".format()\n";
    let hir = crate::compile(source, "format-overflow2.opy", Path::new(".")).unwrap();
    assert!(
        Compiler::new().unwrap().compile_hir(&hir).is_err(),
        "an unparseable placeholder must be rejected"
    );
}

#[test]
fn format_without_placeholders_ignores_extra_arguments() {
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = \"s\".format(3, 3, 3)\n";
    let hir = crate::compile(source, "format-extra.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains("Custom String(\"s\")"));
}

#[test]
fn f_string_interpolations_expand_active_defines() {
    // The reference re-expands `#!define` text macros inside f-string
    // interpolations: `f"{S}!{N}={M(6)}"` sees `"a"`, `7`, and `6+1`.
    let source = "#!define S \"a\"\n#!define N 7\n#!define M(x) x+1\nglobalvar g\nrule \"r\":\n    @Event global\n    g = f\"{S}!{N}={M(6)}\"\n";
    let hir = crate::compile(source, "fdefine.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact.emitted.contains("Custom String(\"a!7=7\")"),
        "{}",
        artifact.emitted
    );
}
