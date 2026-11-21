use std::path::Path;

use opy_rs::Compiler;
use workshop_rs::catalog::{Catalog, Locale};

#[test]
fn expression_gamemodes_match_the_reference_in_both_locales() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/synthetic");
    let compiler = Compiler::new().expect("compiler loads");
    let catalog = Catalog::builtin_ref().expect("catalog loads");

    for fixture in [
        "settings-expression-gamemodes",
        "settings-expression-gamemodes-variants",
    ] {
        let root = corpus.join(fixture);
        let source = std::fs::read_to_string(root.join("source.opy")).unwrap();
        let snapshot: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("oracle.json")).unwrap())
                .unwrap();
        let hir = opy_rs::compile(&source, "source.opy", &root).expect("source resolves");
        hir.validate().expect("resolved settings produce valid HIR");

        for language in ["en-US", "zh-CN"] {
            let locale = Locale::new(language);
            let expected = if language == "en-US" {
                snapshot["compile"]["workshop"]
                    .as_str()
                    .unwrap()
                    .to_string()
            } else {
                std::fs::read_to_string(root.join("oracle.zh-CN.txt")).unwrap()
            };
            let actual = compiler
                .compile_source_with_locale(&source, "source.opy", &root, &locale)
                .unwrap_or_else(|error| panic!("{fixture} {language}: {error}"));
            let expected = workshop_rs::parser::parse(&expected, catalog, &locale)
                .unwrap_or_else(|error| panic!("{fixture} {language} oracle: {error}"));
            let actual = workshop_rs::parser::parse(&actual.emitted, catalog, &locale)
                .unwrap_or_else(|error| panic!("{fixture} {language} native: {error}"));
            assert!(
                workshop_rs::compare::equivalent(&expected, &actual),
                "{fixture} {language}: {:?}",
                workshop_rs::compare::difference(&expected, &actual)
            );
            assert!(
                workshop_rs::roundtrip::equivalent(&expected, &actual),
                "{fixture} {language}: canonical structure differs"
            );
        }
    }
}

#[test]
fn expression_gamemodes_reject_duplicate_keys() {
    for value in [
        r#"({"skirmish": {}, "skirmish": {}})"#,
        r#"({"skirmish": {}, 'skirmish': {}})"#,
        r#"({"skirmish": {}, "{}".format("skirmish"): {}})"#,
        r#"({"skirmish": {"enabledMaps": [{"watchpointGibraltar": ["123"], "watchpointGibraltar": ["456"]}]}})"#,
    ] {
        let source = format!("settings {{ \"gamemodes\": {value} }}\nrule \"r\":\n    pass\n");
        let error = opy_rs::compile(&source, "duplicate.opy", Path::new(".")).unwrap_err();
        assert_eq!(error.code, "settings-expression", "{value}");
    }

    let source = r#"settings { "gamemodes": ({"skirmish": {}} if true else {"ffa": {}, "{}".format("ffa"): {}}) }
rule "r":
    pass
"#;
    opy_rs::compile(source, "untaken.opy", Path::new("."))
        .expect("the reference accepts duplicate computed keys in an untaken branch");
}

#[test]
fn settings_without_gamemodes_remain_invalid() {
    let source = "settings { \"lobby\": {\"maxSpectators\": 2} }\nrule \"r\":\n    pass\n";
    let root = Path::new(".");
    let error = opy_rs::compile(source, "missing.opy", root).unwrap_err();
    assert_eq!(error.code, "settings-invalid");
    assert!(error.message.contains("gamemodes"));

    let outcome = opy_rs::tooling::check(source, "missing.opy", root);
    assert!(outcome.model.is_none());
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|d| d.code == "settings-invalid")
    );

    let error = Compiler::new()
        .unwrap()
        .compile_source(source, "missing.opy", root)
        .unwrap_err();
    assert_eq!(error.diagnostic.code, "settings-invalid");
}
