//! Settings compile-time expression coverage.

use std::path::Path;

use crate::hir::SettingsNode;
use crate::{CompileStatus, Compiler};
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::settings::{
    Applicability, SettingId, SettingScope, SettingTarget, SettingValue, definitions_by_id,
};

fn source_with_settings(settings: &str) -> String {
    format!("{settings}\nrule \"settings\":\n    @Event global\n    pass\n")
}

#[test]
fn object_defines_chain_and_string_composition_resolve_before_emission() {
    let source = source_with_settings(
        r##"#!define BASE 3
#!define SCALE(value) value*2
#!define DOUBLE SCALE(BASE)
#!define TITLE "hello"
#!define FULL_TITLE TITLE + " world"
settings {
    "main": {
        "description": FULL_TITLE
    },
    "gamemodes": {},
    "heroes": {
        "allTeams": {
            "dva": {
                "health%": DOUBLE
            }
        }
    }
}"##,
    );
    let hir = crate::compile(&source, "source.opy", Path::new(".")).unwrap();
    let settings = hir.settings.as_ref().unwrap();
    let main = match &settings.children[0] {
        SettingsNode::Group { children, .. } => children,
        other => panic!("expected main group, got {other:?}"),
    };
    assert!(matches!(
        &main[0],
        SettingsNode::String { value, .. } if value == "hello world"
    ));
    let heroes = match &settings.children[2] {
        SettingsNode::Group { children, .. } => children,
        other => panic!("expected heroes group, got {other:?}"),
    };
    let all_teams = match &heroes[0] {
        SettingsNode::Group { children, .. } => children,
        other => panic!("expected allTeams group, got {other:?}"),
    };
    let dva = match &all_teams[0] {
        SettingsNode::Group { children, .. } => children,
        other => panic!("expected dva group, got {other:?}"),
    };
    assert!(matches!(
        &dva[0],
        SettingsNode::Number { value, .. } if *value == 6.0
    ));

    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains("Description: \"hello world\""));
    assert!(artifact.emitted.contains("Health: 6%"));
    let parsed = workshop_rs::parser::parse(
        &artifact.emitted,
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(equivalent(&super::canonical_program(&artifact), &parsed));
}

#[test]
fn source_function_macro_and_compile_time_math_resolve_to_a_number() {
    let source = source_with_settings(
        r##"settings {
    "gamemodes": {},
    "heroes": {
        "allTeams": {
            "ana": {
                "health%": percent(1 + 1)
            }
        }
    }
}
macro percent(value):
    100 * value"##,
    );
    let hir = crate::compile(&source, "source.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains("Health: 200%"));
}

#[test]
fn settings_expression_failure_is_source_attributed_and_frontend_classified() {
    let source = source_with_settings(
        r##"settings {
    "gamemodes": {},
    "main": {
        "description": runtime_value
    }
}"##,
    );
    let report =
        Compiler::new()
            .unwrap()
            .compile_source_report(&source, "settings.opy", Path::new("."));
    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert_eq!(
        report.compile.failure_class,
        Some(crate::CompileFailureClass::Frontend)
    );
    let diagnostic = report.compile.diagnostics.first().unwrap();
    assert_eq!(diagnostic.code, "settings-expression");
    assert_eq!(diagnostic.span.as_ref().unwrap().path, "settings.opy");
    assert_eq!(diagnostic.span.as_ref().unwrap().start.line, 4);
}

#[test]
fn settings_macro_failure_keeps_the_original_value_span() {
    let source = source_with_settings(
        r##"#!define LOOP LOOP
settings {
    "gamemodes": {},
    "main": {
        "description": LOOP
    }
}"##,
    );
    let report =
        Compiler::new()
            .unwrap()
            .compile_source_report(&source, "settings.opy", Path::new("."));
    assert_eq!(report.compile.status, CompileStatus::Failure);
    let diagnostic = report.compile.diagnostics.first().unwrap();
    assert_eq!(diagnostic.code, "macro-recursion");
    assert_eq!(diagnostic.span.as_ref().unwrap().path, "settings.opy");
    assert_eq!(diagnostic.span.as_ref().unwrap().start.line, 5);
}

#[test]
fn unresolved_expression_valued_setting_is_a_frontend_error() {
    let source = "settings {\n    \"lobby\": {\n        \"modeName\": GAMEMODE_NAME\" \"GAMEMODE_VERSION,\n    },\n    \"gamemodes\": {}\n}\nrule \"r\":\n    @Event global\n    pass\n";
    let error = crate::compile(source, "source.opy", Path::new(".")).unwrap_err();
    assert_eq!(error.code, "settings-expression");
    assert_eq!(error.span.unwrap().start.line, 3);
}

#[test]
fn hero_general_settings_are_emitted_at_team_scope() {
    let source = "settings {\n    \"gamemodes\": {},\n    \"heroes\": {\n        \"team1\": {\n            \"general\": {\n                \"damageReceived%\": 50\n            }\n        }\n    }\n}\nrule \"r\":\n    @Event global\n    pass\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("settings resolve");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("hero general settings must lower at team scope");
    assert!(artifact.emitted.contains("Team 1 {"));
    assert!(artifact.emitted.contains("Damage Received: 50%"));
}

#[test]
fn compiled_settings_are_queryable_through_the_canonical_consumer_api() {
    let source = source_with_settings(
        r#"settings {
    "gamemodes": {},
    "lobby": {
        "spectatorSlots": 2
    }
}"#,
    );
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_artifact(&source, "settings.opy", Path::new("."))
        .expect("OPY settings must compile");

    let definition = definitions_by_id(&SettingId::from("setting.lobby.spectatorSlots"))
        .next()
        .expect("canonical lobby setting");
    assert_eq!(definition.scope(), SettingScope::Lobby);
    assert_eq!(
        definition.presentation().localized_name("en-US"),
        Some("Max Spectators")
    );
    assert!(definition.provenance().reviewed);
    assert_eq!(
        definition
            .applicability(&SettingTarget::Global)
            .expect("global lobby applicability"),
        Applicability::Applicable
    );

    let occurrence = definition
        .read(
            artifact.wir.settings.as_ref().expect("compiled settings"),
            &SettingTarget::Global,
        )
        .expect("compiled setting must be readable through the typed API");
    assert_eq!(occurrence.authored, SettingValue::Number(2.0));
    assert!(artifact.emitted.contains("Max Spectators: 2"));
}
