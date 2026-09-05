//! Canonical WIR lowering coverage for HUD actions.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::{Action, Value};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic/hud-subheader")
}

fn oracle_workshop() -> String {
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("oracle.json"))
            .expect("oracle must be readable"),
    )
    .expect("oracle must parse");
    value["compile"]["workshop"]
        .as_str()
        .expect("oracle must contain Workshop output")
        .to_string()
}

#[test]
fn hud_subheader_matches_the_pinned_canonical_wir() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("hudSubheader must lower to canonical WIR");

    let catalog = Catalog::builtin().expect("catalog must load");
    let oracle = workshop_rs::parser::parse(&oracle_workshop(), &catalog, &Locale::new("en-US"))
        .expect("oracle output must reparse");
    assert!(
        equivalent(&artifact.wir, &oracle),
        "hudSubheader WIR diverged from the pinned oracle\n--- native ---\n{}\n--- oracle ---\n{}",
        artifact.emitted,
        oracle_workshop()
    );

    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .expect("fixture has one rule");
    let Action::Call { name, args, .. } = artifact
        .wir
        .actions
        .get(rule.actions[0])
        .expect("fixture has one action")
    else {
        panic!("hudSubheader must lower to a canonical action call");
    };
    assert_eq!(name, "createHudText");
    assert_eq!(args.len(), 11);
    assert!(matches!(
        &artifact.wir.values.get(args[0]).unwrap().value,
        Value::Call { name, args } if name == "allPlayers" && args.len() == 1
    ));
    assert!(matches!(
        artifact.wir.values.get(args[1]).unwrap().value,
        Value::Null
    ));
    assert!(matches!(
        artifact.wir.values.get(args[3]).unwrap().value,
        Value::Null
    ));
    assert!(matches!(
        artifact.wir.values.get(args[6]).unwrap().value,
        Value::Null
    ));
    assert!(matches!(
        artifact.wir.values.get(args[8]).unwrap().value,
        Value::Null
    ));
    assert!(matches!(
        &artifact.wir.values.get(args[4]).unwrap().value,
        Value::Enum { value_type, value }
            if value_type == "HudPosition" && value == "TOP"
    ));
    assert!(matches!(
        &artifact.wir.values.get(args[7]).unwrap().value,
        Value::Enum { value_type, value }
            if value_type == "Color" && value == "WHITE"
    ));
    assert!(matches!(
        &artifact.wir.values.get(args[9]).unwrap().value,
        Value::Enum { value_type, value }
            if value_type == "HudReeval" && value == "VISIBILITY"
    ));
    assert!(matches!(
        &artifact.wir.values.get(args[10]).unwrap().value,
        Value::Enum { value_type, value }
            if value_type == "SpecVisibility" && value == "DEFAULT"
    ));
}

#[test]
fn hud_subheader_omitted_spectators_use_default_visibility() {
    let source = "rule \"r\":\n    @Event global\n    hudSubheader(getAllPlayers(), \"text\", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY)\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("hudSubheader default visibility must lower");
    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .expect("rule must exist");
    let Action::Call { args, .. } = artifact.wir.actions.get(rule.actions[0]).unwrap() else {
        panic!("hudSubheader must lower to a canonical action call");
    };
    assert!(matches!(
        &artifact.wir.values.get(args[10]).unwrap().value,
        Value::Enum { value_type, value }
            if value_type == "SpecVisibility" && value == "DEFAULT"
    ));
}

#[test]
fn other_hud_helpers_lower_to_their_canonical_text_slots() {
    for helper in ["hudHeader", "hudSubtext"] {
        let source = format!(
            "rule \"r\":\n    @Event global\n    {helper}(getAllPlayers(), \"text\", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)\n"
        );
        let hir =
            crate::compile(&source, "source.opy", Path::new(".")).expect("source must resolve");
        let artifact = Compiler::new()
            .expect("released workshop contract must load")
            .compile_hir(&hir)
            .expect("HUD helper must lower");
        let rule = artifact
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(0))
            .expect("rule must exist");
        let Action::Call { name, args, .. } = artifact.wir.actions.get(rule.actions[0]).unwrap()
        else {
            panic!("HUD helper must lower to a canonical action call");
        };
        assert_eq!(name, "createHudText");
        let text_slot = if helper == "hudHeader" { 1 } else { 3 };
        assert!(matches!(
            &artifact.wir.values.get(args[text_slot]).unwrap().value,
            Value::Call { name, args } if name == "customString" && args.len() == 1
        ));
    }
}
