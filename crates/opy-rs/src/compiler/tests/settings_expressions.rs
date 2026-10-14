//! Settings compile-time expression coverage.

use std::collections::BTreeMap;
use std::path::Path;

use crate::hir::SettingsNode;
use crate::{CompileStatus, Compiler, compile_with_overlay};
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::settings::{
    Applicability, SettingId, SettingScope, SettingTarget, SettingValue, definitions_by_id,
};

fn source_with_settings(settings: &str) -> String {
    format!("{settings}\nrule \"settings\":\n    @Event global\n    pass\n")
}

#[test]
fn external_settings_jsonc_uses_the_same_canonical_emitter() {
    let overlay = BTreeMap::from([(
        "settings.opy.json".to_string(),
        r#"{
    "gamemodes": {
        "general": {
            "heroLimit": "off",
            "respawnTime%": 30
        }
    },
    "heroes": {
        "allTeams": {
            "mei": { "health%": 200 }
        }
    }
}"#
        .to_string(),
    )]);
    let hir = crate::compile_with_overlay(
        "settings \"settings.opy.json\"\nrule \"settings\":\n    @Event global\n    pass\n",
        "main.opy",
        Path::new("."),
        &overlay,
    )
    .expect("external settings should compile through the frontend");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains("Hero Limit: Off"));
    assert!(artifact.emitted.contains("Health: 200%"));
}

#[test]
fn external_settings_diagnostics_use_the_external_file_registry_entry() {
    let overlay = BTreeMap::from([(
        "settings.opy.json".to_string(),
        "{\n    \"gamemodes\": [\n".to_string(),
    )]);
    let error = crate::compile_with_overlay(
        "settings \"settings.opy.json\"\nrule \"settings\":\n    @Event global\n    pass\n",
        "main.opy",
        Path::new("."),
        &overlay,
    )
    .expect_err("malformed external settings must retain its source boundary");
    assert_eq!(error.code, "settings-invalid");
    assert_eq!(error.span.expect("external settings span").file, 1);
}

#[test]
fn multiline_define_string_composition_resolves_in_settings() {
    let overlay = BTreeMap::from([
        (
            "constants.opy".to_string(),
            r##"#!define DESCRIPTION \
    "hello" \
    "world"
"##
            .to_string(),
        ),
        (
            "settings.opy".to_string(),
            r##"settings {
    "main": {
        "description": DESCRIPTION
    },
    "gamemodes": {}
}"##
            .to_string(),
        ),
    ]);
    let source = "#!include \"constants.opy\"\n#!include \"settings.opy\"\nrule \"settings\":\n    @Event global\n    pass\n";
    let hir = compile_with_overlay(source, "source.opy", Path::new("."), &overlay).unwrap();
    let settings = hir.settings.as_ref().unwrap();
    let main = match &settings.children[0] {
        SettingsNode::Group { children, .. } => children,
        other => panic!("expected main group, got {other:?}"),
    };
    assert!(matches!(
        &main[0],
        SettingsNode::String { value, .. } if value == "helloworld"
    ));
}

#[test]
fn object_defines_chain_resolves_and_string_composition_fails_like_the_reference() {
    // A `#!define` chain expands into the settings expression the pinned
    // evaluator sees: a bare string or `number*number` resolves, while an
    // expanded `string + string` composition fails the reference's
    // `String(value)` conversion the same as a literal `"a"+"b"` (#512).
    let source = source_with_settings(
        r##"#!define BASE 3
#!define SCALE(value) value*2
#!define DOUBLE SCALE(BASE)
#!define TITLE "hello"
settings {
    "main": {
        "description": TITLE
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
        SettingsNode::String { value, .. } if value == "hello"
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
    assert!(artifact.emitted.contains("Description: \"hello\""));
    assert!(artifact.emitted.contains("Health: 6%"));
    let parsed = workshop_rs::parser::parse(
        &artifact.emitted,
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(equivalent(&super::canonical_program(&artifact), &parsed));

    let source = source_with_settings(
        r##"#!define TITLE "hello"
#!define FULL_TITLE TITLE + " world"
settings {
    "main": {
        "description": FULL_TITLE
    },
    "gamemodes": {}
}"##,
    );
    let error = crate::compile(&source, "source.opy", Path::new(".")).unwrap_err();
    assert_eq!(error.code, "settings-expression");
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
    assert_eq!(diagnostic.span.as_ref().unwrap().start.line, 1);
}

#[test]
fn custom_string_modifiers_resolve_as_settings_text() {
    // opy-rs#530: `w`/`b`/`c`/`f` literals are `__customString__` upstream
    // whose text tokens the modifier rewrites; the settings object walk
    // reads the rewritten text like a plain string, and `.format` binds the
    // `{n}`/`{}` formatters that survive the rewrite.
    let cases = [
        ("w\"Free for all\"", "Ｆｒｅｅ\u{2001}ｆｏｒ\u{2001}ａｌｌ"),
        ("b\"Free for all\"", "FrΕe for all"),
        ("c\"Free for all\"", "Fṛẹẹ ƒοṛ ạII"),
        ("f\"Free for all\"", "Free for all"),
        ("f\"abc{1 + 1}def\"", "abc2def"),
        ("w\"a{0}b\".format(\"x\")", "ａxｂ"),
        ("w\"a{}b\".format(\"x\")", "ａxｂ"),
        ("w\"{{0}}\"", "｛｛０｝｝"),
    ];
    for (value, expected) in cases {
        let source = source_with_settings(&format!(
            "settings {{\n    \"main\": {{\"modeName\": {value}}},\n    \"gamemodes\": {{}}\n}}"
        ));
        let artifact = Compiler::new()
            .expect("released Workshop contract must load")
            .compile_source_artifact(&source, "issue-530.opy", Path::new("."))
            .unwrap_or_else(|error| panic!("{value} must resolve in settings: {error}"));
        assert!(
            artifact
                .emitted
                .contains(&format!("Mode Name: \"{expected}\"")),
            "{value} must emit {expected}: {}",
            artifact.emitted
        );
    }
}

#[test]
fn unsupported_string_modifiers_stay_unresolvable_in_settings() {
    // `l`/`t` build AST kinds the pinned settings walk rejects; an unbound
    // formatter template, a `b` literal that rewrites no character, a
    // missing `.format` argument, and `.format` on an f-string are all
    // rejected upstream too.
    let cases = [
        "l\"err\"",
        "t\"err\"",
        "w\"a{0}b\"",
        "b\"??\"",
        "w\"a{0}b\".format()",
        "f\"a{1}b\".format(9)",
    ];
    for value in cases {
        let source = source_with_settings(&format!(
            "settings {{\n    \"main\": {{\"modeName\": {value}}},\n    \"gamemodes\": {{}}\n}}"
        ));
        let report = Compiler::new()
            .expect("released Workshop contract must load")
            .compile_source_report(&source, "issue-530.opy", Path::new("."));
        assert_eq!(
            report.compile.status,
            CompileStatus::Failure,
            "{value} must not resolve in settings"
        );
        assert_eq!(
            report.compile.diagnostics.first().map(|d| d.code.as_str()),
            Some("settings-expression"),
            "{value} must fail as a settings-expression"
        );
    }
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
fn general_keys_reject_directly_under_teams_and_accept_under_general() {
    // opy-rs#545: the pinned compiler resolves every `heroes.<team>` member
    // name through `tows(key, heroKw)` before the value is read, so each of
    // the `generalKeys` declared for `heroes.<team>.general` is rejected
    // when placed directly under a team and accepted beneath `general`.
    let compiler = Compiler::new().expect("compiler loads");
    let general_keys = crate::compiler::settings::general_hero_keys();
    assert_eq!(general_keys.len(), 25, "the pinned generalKeys inventory");
    // `__percent__` keys take numbers; every `__bool*` domain accepts `true`
    // upstream (`providedSettings[key] === true`). A group of misplaced
    // members rejects each individually, so one source per team still names
    // every key.
    let member_list = |keys: &[String]| {
        keys.iter()
            .map(|key| {
                let value = if key.ends_with('%') { "50" } else { "true" };
                format!("\"{key}\": {value}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    for team in ["allTeams", "team1", "team2"] {
        let direct = source_with_settings(&format!(
            "settings {{ \"gamemodes\": {{}}, \"heroes\": {{ \"{team}\": {{ {} }} }} }}",
            member_list(general_keys)
        ));
        let report = compiler.compile_source_report(&direct, "issue-545.opy", Path::new("."));
        assert_eq!(
            report.compile.status,
            CompileStatus::Failure,
            "{team} general keys directly under the team must reject"
        );
        // `check` and `compile` share the emission-acceptance pass (#411).
        let outcome = crate::tooling::check(&direct, "issue-545.opy", Path::new("."));
        for key in general_keys {
            let needle = format!("'{key}'");
            assert!(
                report.compile.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "workshop-emission" && diagnostic.message.contains(&needle)
                }),
                "{team}.{key} must reject at the emission boundary: {:?}",
                report.compile.diagnostics
            );
            assert!(
                outcome.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "workshop-emission" && diagnostic.message.contains(&needle)
                }),
                "check must report the same rejection for {team}.{key}: {:?}",
                outcome.diagnostics
            );
        }

        let nested = source_with_settings(&format!(
            "settings {{ \"gamemodes\": {{}}, \"heroes\": {{ \"{team}\": {{ \"general\": {{ {} }} }} }} }}",
            member_list(general_keys)
        ));
        compiler
            .compile_source_artifact(&nested, "issue-545.opy", Path::new("."))
            .unwrap_or_else(|error| panic!("{team}.general must compile: {error}"));
    }
}

#[test]
fn general_keys_compile_under_a_concrete_hero() {
    // The same keys stay accepted under an authored hero group, where the
    // pinned compiler looks them up in the hero's merged schema — keys the
    // schema does not apply to that hero are written back verbatim, which
    // upstream also accepts.
    let compiler = Compiler::new().expect("compiler loads");
    let catalog = workshop_rs::gameplay::data::builtin().expect("gameplay catalog");
    for (index, key) in crate::compiler::settings::general_hero_keys()
        .iter()
        .enumerate()
    {
        let team = ["allTeams", "team1", "team2"][index % 3];
        let value = if key.ends_with('%') { "50" } else { "true" };
        let hero = catalog
            .heroes()
            .iter()
            .map(|hero| hero.id().as_str())
            .find(|id| crate::compiler::settings::hero_setting_applies(id, key))
            .unwrap_or("mei");
        let source = source_with_settings(&format!(
            "settings {{ \"gamemodes\": {{}}, \"heroes\": {{ \"{team}\": {{ \"{hero}\": {{ \"{key}\": {value} }} }} }} }}"
        ));
        compiler
            .compile_source_artifact(&source, "issue-545.opy", Path::new("."))
            .unwrap_or_else(|error| panic!("{team}.{hero}.{key} must compile: {error}"));
    }
}

#[test]
fn misplaced_general_key_names_general_as_the_valid_location() {
    // #545 asks the rejection to name the valid location: a catalogued
    // `general` key at the hero-name position suggests the `general` group.
    let source = source_with_settings(
        "settings { \"gamemodes\": {}, \"heroes\": { \"allTeams\": { \"health%\": 100 } } }",
    );
    let report = Compiler::new()
        .expect("compiler loads")
        .compile_source_report(&source, "issue-545.opy", Path::new("."));
    assert_eq!(report.compile.status, CompileStatus::Failure);
    let diagnostic = report
        .compile
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "workshop-emission")
        .expect("the emission rejection");
    assert!(
        diagnostic.message.contains("health%"),
        "the rejected key must be named: {}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("general"),
        "the valid location must be named: {}",
        diagnostic.message
    );
}

#[test]
fn hero_named_leaves_and_lists_follow_the_reference_traversal() {
    // #545 lands a leaf in the same hero-name resolution upstream applies
    // to every `heroes.<team>` member: a hero name carrying a list iterates
    // `Object.keys` (index-keyed members), while a hero name carrying a
    // scalar dereferences it and fails.
    let compiler = Compiler::new().expect("compiler loads");
    let list = source_with_settings(
        "settings { \"gamemodes\": {}, \"heroes\": { \"allTeams\": { \"mei\": [1, 2] } } }",
    );
    let artifact = compiler
        .compile_source_artifact(&list, "issue-545.opy", Path::new("."))
        .expect("a hero-named list compiles as index-keyed members");
    assert!(artifact.emitted.contains("Mei {"));
    assert!(artifact.emitted.contains("0: 1"));
    assert!(artifact.emitted.contains("1: 2"));

    let scalar = source_with_settings(
        "settings { \"gamemodes\": {}, \"heroes\": { \"allTeams\": { \"mei\": 5 } } }",
    );
    let report = compiler.compile_source_report(&scalar, "issue-545.opy", Path::new("."));
    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert!(
        report
            .compile
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "workshop-emission"),
        "a hero-named scalar must reject at the emission boundary: {:?}",
        report.compile.diagnostics
    );
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
    assert!(definition.source().reviewed);
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

#[test]
fn extension_directives_lower_as_canonical_flags_without_authored_settings() {
    let source = "#!extension beamEffects\n#!extension projectiles\nrule \"extensions\":\n    @Event global\n    pass\n";
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_artifact(source, "extensions.opy", Path::new("."))
        .expect("extension directives must compile");

    let settings = artifact.wir.settings.as_ref().expect("generated settings");
    let workshop_rs::settings::SettingsNode::Group { children, .. } = &settings.children[0] else {
        panic!(
            "expected extensions group, got {:?}",
            settings.children[0].name()
        );
    };
    assert_eq!(
        children
            .iter()
            .map(workshop_rs::settings::SettingsNode::name)
            .collect::<Vec<_>>(),
        ["beamEffects", "projectiles"]
    );
    assert_eq!(
        children[0].span().expect("first directive span").start.line,
        1
    );
    assert_eq!(
        children[1]
            .span()
            .expect("second directive span")
            .start
            .line,
        2
    );
    assert!(artifact.emitted.contains("Beam Effects"));
    assert!(artifact.emitted.contains("Projectiles"));
}

#[test]
fn gamemode_kill_cam_compiles_into_the_deathmatch_block() {
    // opy-rs#411: `gamemodes.<mode>` inherits the `gamemodes.general`
    // settings the pinned OverPy merges into every mode schema, so
    // `enableKillCam` under `ffa` must reach the Deathmatch block instead of
    // dying as an outside-the-emission-table key.
    let source = source_with_settings(
        r#"settings {
    "main": {"description": "t"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"], "enableKillCam": false}}
}"#,
    );
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_artifact(&source, "issue-411.opy", Path::new("."))
        .expect("inherited gamemode setting must compile");
    let deathmatch = artifact
        .emitted
        .find("Deathmatch {")
        .expect("ffa lowers to the Deathmatch mode block");
    let mode_block = &artifact.emitted[deathmatch..];
    let mode_end = mode_block
        .find("\n        }")
        .expect("the mode block closes at mode depth");
    assert!(
        mode_block[..mode_end].contains("Kill Cam: Off"),
        "Kill Cam must emit inside the Deathmatch block: {}",
        &mode_block[..mode_end]
    );
}

#[test]
fn extension_directives_merge_with_authored_settings_and_preserve_provenance() {
    let source = "#!extension projectiles\n#!extension beamEffects\n#!extension projectiles\nsettings {\n    \"main\": {\n        \"description\": \"authored\"\n    },\n    \"gamemodes\": {}\n}\nrule \"extensions\":\n    @Event global\n    pass\n";
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_artifact(source, "extensions.opy", Path::new("."))
        .expect("extension directives must compile with authored settings");

    let settings = artifact.wir.settings.as_ref().expect("authored settings");
    let main = settings
        .children
        .iter()
        .find(|node| node.name() == "main")
        .expect("authored main settings");
    assert_eq!(main.span().expect("authored group span").start.line, 5);
    let workshop_rs::settings::SettingsNode::Group { children, .. } = main else {
        panic!("expected authored main group");
    };
    assert_eq!(
        children[0]
            .span()
            .expect("authored setting span")
            .start
            .line,
        6
    );

    let extensions = settings
        .children
        .iter()
        .find(|node| node.name() == "extensions")
        .expect("generated extensions group");
    let workshop_rs::settings::SettingsNode::Group { children, .. } = extensions else {
        panic!("expected generated extensions group");
    };
    assert_eq!(children.len(), 2);
    assert_eq!(
        children
            .iter()
            .map(workshop_rs::settings::SettingsNode::name)
            .collect::<Vec<_>>(),
        ["projectiles", "beamEffects"]
    );
    assert_eq!(children[0].span().expect("directive span").start.line, 1);
    assert!(artifact.emitted.contains("Description: \"authored\""));
    assert!(artifact.emitted.contains("Projectiles"));
}
