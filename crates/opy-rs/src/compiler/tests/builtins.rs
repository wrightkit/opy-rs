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
fn start_hot_keyword_arguments_bind_like_positional_arguments() {
    let source = include_str!("../../manifest/probes/start-hot-signature.opy");
    let hir = crate::compile(source, "start-hot-signature.opy", Path::new("."))
        .expect("pinned startHoT positional and keyword forms must resolve natively");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("pinned startHoT positional and keyword forms must lower natively");

    assert_eq!(artifact.wir.rules.len(), 2);
    assert_eq!(artifact.wir.rules[0].actions.len(), 1);
    assert_eq!(artifact.wir.rules[1].actions.len(), 1);
    let mut positional = artifact.wir.clone();
    positional.rules.truncate(1);
    let mut keyword = artifact.wir;
    keyword.rules.remove(0);
    keyword.rules[0].name = positional.rules[0].name.clone();
    assert!(
        workshop_rs::roundtrip::equivalent(&positional, &keyword),
        "keyword arguments must bind to the same Workshop arguments as positional calls"
    );
}

#[test]
fn stop_hot_uses_the_upstream_keyword_name() {
    let source = "rule \"r\":\n    stopHoT(healOverTimeId=getLastHealingOverTimeId())\n";
    let hir = crate::compile(source, "stop-hot.opy", Path::new(".")).unwrap();
    Compiler::new().unwrap().compile_hir(&hir).unwrap();

    let source = include_str!("../../manifest/probes/stop-hot-unknown-keyword.opy");
    let error = crate::compile(source, "stop-hot.opy", Path::new(".")).unwrap_err();
    assert_eq!(error.code, "unknown-keyword");
}

#[test]
fn stop_chasing_variable_dispatches_by_variable_kind() {
    let source = "globalvar g\nplayervar p\nrule \"r\":\n    @Event eachPlayer\n    stopChasingVariable(g)\n    stopChasingVariable(eventPlayer.p)\n";
    let hir = crate::compile(source, "stop-chasing.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(artifact.emitted.contains("Stop Chasing Global Variable"));
    assert!(artifact.emitted.contains("Stop Chasing Player Variable"));
}

#[test]
fn builtin_alias_preserves_source_identity_and_canonical_target() {
    let source = "globalvar value\nrule \"r\":\n    @Event global\n    @Condition horizontalAngleFromDirection(vect(1, 0, 0)) == 90\n    value = 1\n";
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
    assert!(
        artifact
            .emitted
            .contains("-1, Vector(0, 0, 0), Vector(0, 0, 0));")
    );
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

fn emitted(source: &str) -> String {
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("source must lower")
        .emitted
}

#[test]
fn start_rule_takes_a_declared_subroutine_like_async() {
    let out = emitted(
        "subroutine sub\nrule \"r\":\n    @Event global\n    startRule(sub, AsyncBehavior.RESTART)\n",
    );
    assert!(out.contains("Start Rule(sub, Restart Rule)"), "{out}");
}

#[test]
fn create_projectile_defaults_owner_position_and_direction_to_null() {
    let out = emitted(
        "rule \"r\":\n    @Event global\n    createProjectile(Projectile.ORB, damage=5, speed=3)\n",
    );
    assert!(
        out.contains("Create Projectile(Orb Projectile, Null, Null, Null, To World"),
        "{out}"
    );
}

#[test]
fn declare_round_victory_accepts_its_reference_keyword() {
    let out = emitted(
        "rule \"r\":\n    @Event global\n    declareRoundVictory(roundWinningTeam=Team.1)\n",
    );
    assert!(out.contains("Declare Round Victory"), "{out}");
}

#[test]
fn member_lambda_calls_reject_keyword_arguments() {
    let error = crate::compile(
        "globalvar g\nrule \"r\":\n    @Event global\n    g = [1, 2].filter(condition=lambda x: x > 0)\n",
        "source.opy",
        Path::new("."),
    )
    .expect_err("the reference grammar cannot spell a keyword on member lambda calls");
    assert_eq!(error.code, "keyword-unsupported");
    assert!(error.message.contains("condition"));

    let error = crate::compile(
        "globalvar g\nrule \"r\":\n    @Event global\n    g = getAllPlayers().all(condition=lambda x: x)\n",
        "source.opy",
        Path::new("."),
    )
    .expect_err("member lambda calls take a bare lambda argument");
    assert_eq!(error.code, "keyword-unsupported");
}
