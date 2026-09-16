//! Scoped directive lowering against the pinned canonical-WIR oracle.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

#[test]
fn translated_implicit_subroutine_fixture_matches_the_pinned_oracle() {
    let dir = fixture_dir("directives-scoped");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let oracle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("oracle.json")).unwrap()).unwrap();
    let expected = workshop_rs::parser::parse(
        oracle["compile"]["workshop"].as_str().unwrap(),
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(equivalent(&super::canonical_program(&artifact), &expected));
}

#[test]
fn disabled_rule_is_lowered_with_rule_and_action_provenance() {
    let source =
        "globalvar value\nrule \"disabled\":\n    @Event global\n    @Disabled\n    value = 1\n";
    let hir = crate::compile(source, "disabled.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert_eq!(artifact.wir.rules.len(), 1);
    let rule = &artifact.wir.rules[0];
    assert_eq!(rule.name, "disabled");
    assert!(rule.disabled);
    assert_eq!(rule.actions.len(), 1);
    assert_eq!(artifact.wir.rule_span(0).unwrap().start.line, 2);
    assert_eq!(artifact.wir.action_span(0, 0).unwrap().start.line, 5);
    assert!(
        artifact
            .final_output
            .contains("disabled rule (\"disabled\")")
    );
    assert!(
        !artifact
            .final_output
            .contains("disabled rule (\"enabled\")")
    );
    let emitted = workshop_rs::parser::parse(
        &artifact.final_output,
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert_eq!(emitted.rules.len(), 1);
    assert!(emitted.rules[0].disabled);
}

#[test]
fn delimiter_is_consumed_after_preserving_its_unprefixed_name() {
    let source = "#!rulePrefix \"Section\"\nrule \"ordinary\":\n    @Event global\n    pass\nrule \"delimiter\":\n    @Event global\n    @Disabled\n    @Delimiter\n";
    let hir = crate::compile(source, "delimiters.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert_eq!(
        artifact
            .wir
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        ["[Section] ordinary", "delimiter"]
    );
    assert!(!artifact.wir.rules[0].disabled);
    assert!(artifact.wir.rules[1].disabled);
    assert!(
        artifact
            .final_output
            .contains("rule (\"[Section] ordinary\")")
    );
    assert!(
        artifact
            .final_output
            .contains("disabled rule (\"delimiter\")")
    );
}

#[test]
fn public_source_compile_keeps_enabled_and_disabled_rule_identity() {
    let fixture = fixture_dir("directives").join("regressions/disabled-bastion.opy");
    let source = std::fs::read_to_string(&fixture).unwrap();
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(&source, "disabled-bastion.opy", fixture.parent().unwrap())
        .unwrap();

    assert_eq!(
        artifact
            .wir
            .rules
            .iter()
            .map(|rule| (rule.name.as_str(), rule.disabled))
            .collect::<Vec<_>>(),
        [
            ("enabled", false),
            ("disabled", true),
            ("disabled delimiter", true),
        ]
    );
    assert!(artifact.final_output.contains("rule (\"enabled\")"));
    assert!(
        artifact
            .final_output
            .contains("disabled rule (\"disabled\")")
    );
    assert!(
        artifact
            .final_output
            .contains("disabled rule (\"disabled delimiter\")")
    );
    assert!(
        !artifact
            .final_output
            .contains("disabled rule (\"enabled\")")
    );
    assert_eq!(artifact.wir.rule_span(1).unwrap().start.line, 7);
    assert_eq!(artifact.wir.action_span(1, 0).unwrap().start.line, 10);
}
