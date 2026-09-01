//! Inventory-backed builtin and alias coverage for issue #144.

use std::path::Path;

use crate::Compiler;
use workshop_rs::wir::Value;

fn value_call_names(artifact: &crate::CompilationArtifact) -> Vec<String> {
    (0..artifact.wir.values.len())
        .filter_map(|index| {
            let node = artifact
                .wir
                .values
                .get(workshop_rs::wir::ValueId::from_index(index))?;
            match &node.value {
                Value::Call { name, .. } => Some(name.clone()),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn catalog_backed_builtin_inventory_lowers_to_canonical_calls() {
    let source = r#"globalvar g

rule "builtin surface":
    @Event global
    @Condition isAssemblingHeroes() == false
    @Condition any([g > 0 for g in [1, 2]])
    @Condition all([g > 0 for g in [1, 2]])
    @Condition ceil(1.2) == 2
    @Condition floor(1.8) == 1
    @Condition round(1.5) == 2
    g = getPlayers(Team.ALL)
    g = random.randint(1, 3)
    g = random.shuffle([1, 2])
    eventPlayer.setWeapon(1)
    chaseAtRate(g, 1, 1)
    createDummy(Hero.ANA, Team.ALL, -1, vect(0, 0, 0), vect(0, 0, 0))
    hudHeader(getAllPlayers(), "header", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudSubtext(getAllPlayers(), "text", HudPosition.TOP, 1, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudHeader(text="default header")
    hudSubtext(text="default text")
    waitUntil(true, 1)
"#;
    let hir = crate::compile(source, "builtin-surface.opy", Path::new("."))
        .expect("the audited builtin source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("the audited builtin source must lower");
    let names = value_call_names(&artifact);
    for expected in [
        "isAssemblingHeroes",
        "isTrueForAny",
        "isTrueForAll",
        "allPlayers",
        "randomInteger",
        "randomizedArray",
        "roundToInteger",
        "customString",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected} call"
        );
    }
    assert!(artifact.emitted.contains("Chase Global Variable At Rate"));
    assert!(artifact.emitted.contains("Create Dummy Bot"));
    assert!(artifact.emitted.contains("Wait Until"));
    assert_eq!(artifact.emitted.matches("Create HUD Text").count(), 4);
}

#[test]
fn builtin_alias_preserves_source_identity_and_canonical_target() {
    let source = "rule \"r\":\n    @Event global\n    @Condition horizontalAngleFromDirection(vect(1, 0, 0)) == 90\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("alias must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("alias must lower through the canonical catalog");
    assert!(
        value_call_names(&artifact)
            .iter()
            .any(|name| name == "horizontalAngleFromDirection")
    );
}

#[test]
fn builtin_surface_rejects_invalid_arity_and_keyword_with_source_diagnostics() {
    let error = crate::compile(
        "rule \"r\":\n    @Event global\n    createDummy(Hero.ANA, Team.ALL)\n",
        "source.opy",
        Path::new("."),
    )
    .expect_err("createDummy must require its source signature");
    assert_eq!(error.code, "missing-argument");
    assert_eq!(error.span.expect("arity provenance").start.line, 3);

    let error = crate::compile(
        "rule \"r\":\n    @Event global\n    hudHeader(getAllPlayers(), \"header\", bad=HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY)\n",
        "source.opy",
        Path::new("."),
    )
    .expect_err("unknown HUD keyword must be rejected");
    assert_eq!(error.code, "unknown-keyword");
    assert!(error.message.contains("bad"));
}

#[test]
fn create_dummy_uses_the_reference_facing_default() {
    let source =
        "rule \"r\":\n    @Event global\n    createDummy(Hero.ANA, Team.ALL, -1, vect(0, 0, 0))\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("createDummy's facing default must lower");
    assert!(artifact.emitted.contains("Create Dummy Bot"));
    assert!(artifact.emitted.contains("Vector(0, 0, 0)"));
}

#[test]
fn rule_condition_lowers_the_current_rule_conditions_in_order() {
    let source = r#"rule "rule condition":
    @Event global
    @Condition isGameInProgress()
    @Condition isAssemblingHeroes() == false
    @Condition isGameInProgress()
    waitUntil(RULE_CONDITION, 1)
"#;
    let hir =
        crate::compile(source, "rule-condition.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("ruleCondition must lower inside a rule");
    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .expect("rule must be present");
    let action = artifact
        .wir
        .actions
        .get(rule.actions[0])
        .expect("waitUntil action must be present");
    let workshop_rs::wir::Action::Call { name, args, .. } = action else {
        panic!("expected waitUntil action");
    };
    assert_eq!(name, "waitUntil");
    let Value::Call {
        name: combined_name,
        args: combined_args,
    } = &artifact
        .wir
        .values
        .get(args[0])
        .expect("condition value")
        .value
    else {
        panic!("expected combined rule condition");
    };
    assert_eq!(combined_name, "and");
    assert_eq!(combined_args[1], rule.conditions[2]);
    let Value::Call {
        name: first_pair_name,
        args: first_pair_args,
    } = &artifact
        .wir
        .values
        .get(combined_args[0])
        .expect("first condition pair")
        .value
    else {
        panic!("expected left-associated condition pair");
    };
    assert_eq!(first_pair_name, "and");
    assert_eq!(first_pair_args, &rule.conditions[..2]);
}

#[test]
fn rule_condition_rejects_values_outside_rule_context() {
    let hir = crate::compile(
        "globalvar invalid = RULE_CONDITION\n",
        "rule-condition.opy",
        Path::new("."),
    )
    .expect("frontend should preserve the special value for integration validation");
    let error = match Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
    {
        Ok(_) => panic!("ruleCondition must require a rule context"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert!(
        error
            .diagnostic
            .message
            .contains("only valid inside a rule")
    );
}

#[test]
fn get_ammo_uses_the_reference_zero_clip_default() {
    let source = r#"globalvar g
rule "r":
    @Event eachPlayer
    g = eventPlayer.getAmmo()
"#;
    let hir = crate::compile(source, "get-ammo-default.opy", Path::new("."))
        .expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("getAmmo's omitted clip must lower");
    assert!(artifact.emitted.contains("Ammo(Event Player, 0)"));
}
