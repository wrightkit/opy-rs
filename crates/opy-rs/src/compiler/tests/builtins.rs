//! Inventory-backed builtin and alias coverage.

use std::path::Path;

use crate::Compiler;

fn value_call_names(artifact: &crate::CompilationArtifact) -> Vec<String> {
    let program = &artifact.wir;
    let mut names = Vec::new();
    for rule in &program.rules {
        for condition in &rule.conditions {
            collect_value_call_names(&condition.value, &mut names);
        }
        for action in &rule.actions {
            collect_action_call_names(action, &mut names);
        }
    }
    names
}

fn collect_action_call_names(action: &workshop_rs::Action, names: &mut Vec<String>) {
    use workshop_rs::Action;
    let values: Vec<&workshop_rs::Value> = match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::AssignMember { value, .. } => vec![value],
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => vec![player, value],
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            vec![condition]
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => vec![start, stop, step],
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            vec![player, start, stop, step]
        }
        Action::Call { args, .. } => args.iter().collect(),
        Action::Disabled { action } => {
            collect_action_call_names(action, names);
            Vec::new()
        }
        Action::CallSubroutine { .. } | Action::Else | Action::End => Vec::new(),
    };
    for value in values {
        collect_value_call_names(value, names);
    }
    if let Action::Call { name, .. } = action {
        names.push(name.clone());
    }
}

fn collect_value_call_names(value: &workshop_rs::Value, names: &mut Vec<String>) {
    match value {
        workshop_rs::Value::Call { name, args } => {
            names.push(name.clone());
            for arg in args {
                collect_value_call_names(arg, names);
            }
        }
        workshop_rs::Value::Array(values) => {
            for value in values {
                collect_value_call_names(value, names);
            }
        }
        workshop_rs::Value::Vector { x, y, z } => {
            collect_value_call_names(x, names);
            collect_value_call_names(y, names);
            collect_value_call_names(z, names);
        }
        workshop_rs::Value::PlayerVariable { player, .. } => {
            collect_value_call_names(player, names)
        }
        workshop_rs::Value::Number(_)
        | workshop_rs::Value::String(_)
        | workshop_rs::Value::LocalizedString(_)
        | workshop_rs::Value::Bool(_)
        | workshop_rs::Value::Null
        | workshop_rs::Value::Enum { .. }
        | workshop_rs::Value::GlobalVariable(_)
        | workshop_rs::Value::Subroutine(_)
        | workshop_rs::Value::EventPlayer => {}
    }
}

#[test]
fn catalog_backed_builtin_inventory_lowers_to_canonical_calls() {
    let source = r#"globalvar g

rule "builtin surface":
    @Event global
    @Condition isAssemblingHeroes() == false
    @Condition isInSetup() == false
    @Condition eventPlayer.getEyePosition() == vect(0, 0, 0)
    @Condition any([g > 0 for g in [1, 2]])
    @Condition all([g > 0 for g in [1, 2]])
    @Condition eventPlayer.getHero() == Hero.ANA
    @Condition eventPlayer.hasStatus(Status.ROOTED)
    @Condition distance(eventPlayer.getPosition(), vect(0, 0, 0)) < 2
    @Condition strContains("abc", "b")
    @Condition ceil(1.2) == 2
    @Condition floor(1.8) == 1
    @Condition round(1.5) == 2
    g = getPlayers(Team.ALL)
    g = getAllHeroes()
    g = heroIcon(Hero.ANA)
    g = getMatchTime()
    g = random.randint(1, 3)
    g = random.shuffle([1, 2])
    g = g.concat([1])
    g = g.exclude(1)
    g = vectorTowards(vect(0, 0, 0), vect(1, 0, 0))
    g = dotProduct(vect(1, 0, 0), vect(1, 0, 0))
    g = getServerLoad()
    g = getAverageServerLoad()
    g = getPeakServerLoad()
    g = evalOnce(g)
    g = abilityIconString(Hero.ANA, Button.ABILITY_1)
    g = updateEveryTick(g)
    g = sinDeg(g)
    g = cosDeg(g)
    g = rgb(1, 2, 3)
    g = eventPlayer.getSlot()
    g = localPlayer
    g = eventPlayer.getMaxHealth()
    g = eventPlayer.getUltCharge()
    eventPlayer.addToScore(1)
    g = max(1, 2)
    g = getLastCreatedEntity()
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION)
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION).x
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION).y
    g = worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION).z
    g = "abc".charAt(0)
    g = [1, 2].last()
    eventPlayer.setWeapon(1)
    eventPlayer.startForcingHero(Hero.ANA)
    eventPlayer.startForcingPosition(vect(0, 0, 0), false)
    eventPlayer.disallowButton(Button.ULTIMATE)
    eventPlayer.allowButton(Button.ULTIMATE)
    eventPlayer.startHoT(null, 9999, 9999)
    eventPlayer.attachTo(eventPlayer, vect(0, 0, 0))
    eventPlayer.disableEnvironmentCollision(true)
    eventPlayer.setAbility1Enabled(false)
    eventPlayer.setAbility2Enabled(false)
    eventPlayer.setUltEnabled(false)
    eventPlayer.setPrimaryFireEnabled(false)
    eventPlayer.setSecondaryFireEnabled(false)
    eventPlayer.clearStatusEffect(Status.ROOTED)
    destroyAllEffects()
    destroyAllDummies()
    setObjectiveDescription(getAllPlayers(), "objective", HudReeval.VISIBILITY)
    eventPlayer.setFacing(vect(0, 0, 1), Relativity.TO_WORLD)
    createEffect(getAllPlayers(), Effect.BAD_AURA, Color.GREEN, vect(0, 0, 0), 2, EffectReeval.VISIBILITY_POSITION_AND_RADIUS)
    createInWorldText(getAllPlayers(), "header", vect(0, 0, 0), 1, Clip.NONE, WorldTextReeval.VISIBILITY, Color.WHITE, SpecVisibility.DEFAULT)
    eventPlayer.startForcingButton(Button.SECONDARY_FIRE)
    eventPlayer.stopForcingButton(Button.SECONDARY_FIRE)
    eventPlayer.forceButtonPress(Button.ABILITY_1)
    eventPlayer.stopFacing()
    eventPlayer.stopForcingCurrentHero()
    destroyAllHudTexts()
    disableGamemodeCompletion()
    disableScoring()
    eventPlayer.disableKillFeed()
    eventPlayer.startForcingHero(Hero.MCCREE)
    eventPlayer.startForcingHero(Hero.HAMMOND)
    declareTeamVictory(Team.1)
    declarePlayerVictory(eventPlayer)
    setSlowMotion(10)
    stopChasingVariable(g)
    chaseAtRate(g, 1, 1)
    createDummy(Hero.ANA, Team.ALL, -1, vect(0, 0, 0), vect(0, 0, 0))
    hudHeader(getAllPlayers(), "header", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudHeader(getAllPlayers(), "always", HudPosition.TOP, 1, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.ALWAYS)
    hudSubtext(getAllPlayers(), "text", HudPosition.TOP, 1, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudText(getAllPlayers(), "header", "subheader", "text", HudPosition.TOP, 1, Color.WHITE, Color.WHITE, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.DEFAULT)
    hudHeader(text="default header")
    hudSubtext(text="default text")
    bigMessage(getAllPlayers(), "big")
    smallMessage(getAllPlayers(), "small")
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
        "allHeroes",
        "heroIconString",
        "getMatchTime",
        "randomInteger",
        "randomizedArray",
        "roundToInteger",
        "customString",
        "strContains",
        "vectorTowards",
        "dotProduct",
        "getServerLoad",
        "getAverageServerLoad",
        "getPeakServerLoad",
        "evaluateOnce",
        "abilityIconString",
        "updateEveryFrame",
        "sinDeg",
        "cosDeg",
        "getEyePosition",
        "customColor",
        "getSlot",
        "getMaxHealth",
        "getUltCharge",
        "max",
        "lastCreatedEntity",
        "charAt",
        "__xComponentOf__",
        "__yComponentOf__",
        "__zComponentOf__",
        "lastOf",
        "appendToArray",
        "removeFromArray",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected} call"
        );
    }
    assert!(artifact.emitted.contains("Chase Global Variable At Rate"));
    assert!(artifact.emitted.contains("Create Dummy Bot"));
    assert!(artifact.emitted.contains("Start Forcing Player To Be Hero"));
    assert!(artifact.emitted.contains("Start Forcing Player Position"));
    assert!(artifact.emitted.contains("Disallow Button"));
    assert!(artifact.emitted.contains("Start Heal Over Time"));
    assert!(artifact.emitted.contains("Modify Player Score"));
    assert!(artifact.emitted.contains("Attach Players"));
    assert!(
        artifact
            .emitted
            .contains("Disable Movement Collision With Environment")
    );
    assert!(artifact.emitted.contains("Set Ability 1 Enabled"));
    assert!(artifact.emitted.contains("Set Ability 2 Enabled"));
    assert!(artifact.emitted.contains("Set Ultimate Ability Enabled"));
    assert!(artifact.emitted.contains("Set Primary Fire Enabled"));
    assert!(artifact.emitted.contains("Set Secondary Fire Enabled"));
    assert!(artifact.emitted.contains("Clear Status"));
    assert!(artifact.emitted.contains("Hero Of"));
    assert!(names.iter().any(|name| name == "localPlayer"));
    assert!(artifact.emitted.contains("Local Player"));
    assert!(artifact.emitted.contains("Has Status"));
    assert!(artifact.emitted.contains("Destroy All Effects"));
    assert!(artifact.emitted.contains("Destroy All Dummy Bots"));
    assert!(artifact.emitted.contains("Set Objective Description"));
    assert!(artifact.emitted.contains("Set Facing"));
    assert!(artifact.emitted.contains("Create Effect"));
    assert!(artifact.emitted.contains("Create In-World Text"));
    assert!(artifact.emitted.contains("Start Holding Button"));
    assert!(artifact.emitted.contains("Stop Holding Button"));
    assert!(artifact.emitted.contains("Press Button"));
    assert!(artifact.emitted.contains("Stop Facing"));
    assert!(artifact.emitted.contains("Stop Forcing Player To Be Hero"));
    assert!(artifact.emitted.contains("Destroy All HUD Text"));
    assert!(
        artifact
            .emitted
            .contains("Disable Built-In Game Mode Completion")
    );
    assert!(
        artifact
            .emitted
            .contains("Disable Built-In Game Mode Scoring")
    );
    assert!(artifact.emitted.contains("Disable Kill Feed"));
    assert!(artifact.emitted.contains("Cassidy"));
    assert!(artifact.emitted.contains("Wrecking Ball"));
    assert!(artifact.emitted.contains("Declare Team Victory"));
    assert!(artifact.emitted.contains("Stop Chasing Global Variable"));
    assert!(artifact.emitted.contains("Big Message"));
    assert!(artifact.emitted.contains("Small Message"));
    assert!(artifact.emitted.contains("Wait Until"));
    assert_eq!(artifact.emitted.matches("Create HUD Text").count(), 6);
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
fn get_all_players_value_uses_the_all_team_domain() {
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = getAllPlayers()\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("getAllPlayers must lower in a value position");
    let program = super::canonical_program(&artifact);
    let workshop_rs::Action::SetGlobalVariable { value, .. } = &program.rules[0].actions[0] else {
        panic!("expected a global assignment");
    };
    let workshop_rs::Value::Call { name, args } = value else {
        panic!("expected a canonical allPlayers value call");
    };
    assert_eq!(name, "allPlayers");
    assert_eq!(args.len(), 1);
    assert!(matches!(
        &args[0],
        workshop_rs::Value::Enum { value_type, value } if value_type == "Team" && value == "ALL"
    ));
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
    assert!(artifact.emitted.contains("Null"));
}

#[test]
fn builtin_surface_maps_overpy_enum_aliases_to_canonical_members() {
    let source = "rule \"r\":\n    @Event global\n    hudHeader(getAllPlayers(), \"text\", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.ALWAYS)\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("SpecVisibility.ALWAYS must lower");
    let program = super::canonical_program(&artifact);
    let workshop_rs::Action::Call { args, .. } = &program.rules[0].actions[0] else {
        panic!("expected HUD action");
    };
    assert!(matches!(
        &args[10],
        workshop_rs::Value::Enum { value_type, value }
            if value_type == "SpecVisibility" && value == "VISIBLE_ALWAYS"
    ));
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
    let program = super::canonical_program(&artifact);
    let workshop_rs::Action::Call { name, args } = &program.rules[0].actions[0] else {
        panic!("expected waitUntil action");
    };
    assert_eq!(name, "waitUntil");
    let workshop_rs::Value::Call {
        name: combined_name,
        args: combined_args,
    } = &args[0]
    else {
        panic!("expected combined rule condition");
    };
    assert_eq!(combined_name, "and");
    let workshop_rs::Value::Call {
        name: first_pair_name,
        args: first_pair_args,
    } = &combined_args[0]
    else {
        panic!("expected left-associated condition pair");
    };
    assert_eq!(first_pair_name, "and");
    assert!(matches!(
        first_pair_args[0],
        workshop_rs::Value::Call { .. }
    ));
    assert!(matches!(
        first_pair_args[1],
        workshop_rs::Value::Call { .. }
    ));
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
