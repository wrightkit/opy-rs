use std::path::Path;

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::parser;
use workshop_rs::roundtrip::equivalent;

const PINNED_WORKSHOP_LOCALES: &[&str] = &[
    "en-US", "de-DE", "es-ES", "es-MX", "fr-FR", "it-IT", "ja-JP", "ko-KR", "pl-PL", "pt-BR",
    "ru-RU", "th-TH", "tr-TR", "zh-CN", "zh-TW",
];

const LOCALE_SOURCE: &str = r#"globalvar probe

rule "locale surface":
    @Event global
    probe = 1
    disableInspector()
"#;

#[test]
fn forward_compilation_reparses_in_every_pinned_workshop_locale() {
    let compiler = Compiler::new().expect("released Workshop contract must load");
    let catalog = Catalog::builtin().expect("canonical catalog must load");

    let english_locale = Locale::new("en-US");
    let english = compiler
        .compile_source_with_locale(LOCALE_SOURCE, "locale.opy", Path::new("."), &english_locale)
        .expect("English source must compile");
    let english_program = parser::parse(&english.emitted, &catalog, &english_locale)
        .expect("English output must reparse");
    let english_rule = catalog
        .spelling(Kind::Structural, &english_locale, "rule")
        .expect("catalog must expose the English rule spelling");

    for language in PINNED_WORKSHOP_LOCALES {
        let locale = Locale::new(language);
        assert!(
            catalog.supports(&locale),
            "canonical catalog must support pinned locale {language}"
        );
        let artifact = compiler
            .compile_source_with_language(LOCALE_SOURCE, "locale.opy", Path::new("."), language)
            .unwrap_or_else(|error| panic!("{language} must compile: {error}"));
        let program = parser::parse(&artifact.emitted_workshop, &catalog, &locale)
            .unwrap_or_else(|error| panic!("{language} output must reparse: {error}"));
        assert!(
            equivalent(&english_program, &program),
            "{language} output changed canonical Workshop identity"
        );
        let rule = catalog
            .spelling(Kind::Structural, &locale, "rule")
            .unwrap_or_else(|| panic!("catalog must expose the {language} rule spelling"));
        assert!(
            artifact
                .emitted_workshop
                .lines()
                .any(|line| line.starts_with(&format!("{rule} ("))),
            "{language} output did not use its canonical rule spelling: {}",
            artifact.emitted_workshop.lines().next().unwrap_or_default()
        );
        if rule != english_rule {
            assert!(
                !artifact
                    .emitted_workshop
                    .lines()
                    .any(|line| line.starts_with(&format!("{english_rule} ("))),
                "{language} output silently fell back to en-US"
            );
        }
    }
}
