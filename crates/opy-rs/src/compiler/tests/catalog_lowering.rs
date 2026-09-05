//! Oracle-backed catalog/member/enum lowering evidence.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
        .join(name)
}

fn compile_fixture(name: &str) -> crate::CompilationArtifact {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR")
}

fn compile_real_world(name: &str, source_name: &str) -> crate::CompilationArtifact {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/real-world")
        .join(name);
    let source = std::fs::read_to_string(dir.join(source_name)).expect("source must be readable");
    let hir = crate::compile(&source, source_name, &dir).expect("real-world source must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("real-world source must lower to canonical WIR")
}

fn oracle_workshop(name: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir(name).join("oracle.json"))
            .expect("oracle must be readable"),
    )
    .expect("oracle must parse");
    value["compile"]["workshop"]
        .as_str()
        .expect("oracle must contain Workshop output")
        .to_string()
}

fn assert_matches_oracle(name: &str) {
    let artifact = compile_fixture(name);
    let catalog = Catalog::builtin().expect("catalog must load");
    let locale = Locale::new("en-US");
    let native =
        workshop_rs::parser::parse(&artifact.emitted, &catalog, &locale).unwrap_or_else(|error| {
            panic!("native output must reparse: {error}\n{}", artifact.emitted)
        });
    let oracle = workshop_rs::parser::parse(&oracle_workshop(name), &catalog, &locale)
        .expect("oracle output must reparse");
    assert!(
        equivalent(&native, &oracle),
        "lowering diverged for {name}\n--- native ---\n{}\n--- oracle ---\n{}",
        artifact.emitted,
        oracle_workshop(name)
    );
}

#[test]
fn catalog_backed_receiver_calls_match_the_pinned_oracle() {
    assert_matches_oracle("receiver-calls");
}

#[test]
fn catalog_backed_contextual_chase_calls_match_the_pinned_oracle() {
    let artifact = compile_fixture("chase-condition-agentlab");
    assert!(
        artifact
            .emitted
            .contains("Chase Global Variable Over Time(Global.Round_Attack_Time, 0, 30, None);")
    );
}

#[test]
fn catalog_enum_members_lower_and_validate() {
    let artifact = compile_fixture("chase-enums");
    for expected in [
        "Set Global Variable(time_reeval, None);",
        "Set Global Variable(time_reeval, Destination and Duration);",
        "Set Global Variable(rate_reeval, None);",
        "Set Global Variable(rate_reeval, Destination and Rate);",
    ] {
        assert!(artifact.emitted.contains(expected), "missing {expected}");
    }
}

#[test]
fn aliased_member_lowers_to_the_canonical_catalog_identity() {
    let source =
        "rule \"r\":\n    @Event eachPlayer\n    @Condition eventPlayer.getHero() == None\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("frontend resolves");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("the member alias must lower to Hero Of");
    assert!(artifact.emitted.contains("Hero Of(Event Player)"));
}

#[test]
fn append_receiver_uses_the_canonical_modify_operation() {
    let source = "globalvar values\nrule \"r\":\n    @Event global\n    values.append(1)\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("frontend resolves");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("append lowers to canonical WIR");
    assert!(
        artifact
            .emitted
            .contains("Modify Global Variable(values, Append To Array, 1);")
    );
}

#[test]
fn real_world_cake_exercises_catalog_lowering_end_to_end() {
    let first = compile_real_world("overpy-cake", "source.opy");
    let second = compile_real_world("overpy-cake", "source.opy");
    assert_eq!(
        first.emitted, second.emitted,
        "emission must be deterministic"
    );

    // compile_hir already runs both structural WIR and canonical catalog-id
    // validation. The pinned cake Workshop text contains the existing
    // ambiguous `Visible To` spelling, so reparsing it would test a
    // workshop-rs parser limitation rather than this lowering contract.
    let mut calls = std::collections::BTreeSet::new();
    for index in 0..first.wir.rules.len() {
        let rule = first
            .wir
            .rules
            .get(workshop_rs::wir::RuleId::from_index(index))
            .unwrap();
        collect_action_calls(&first.wir, &rule.actions, &mut calls);
    }
    for expected in ["createBeamEffect", "playEffect"] {
        assert!(
            calls.contains(expected),
            "real-world cake must lower {expected}"
        );
    }
    let value_calls: std::collections::BTreeSet<_> = (0..first.wir.values.len())
        .filter_map(|index| {
            let node = first
                .wir
                .values
                .get(workshop_rs::wir::ValueId::from_index(index))?;
            let workshop_rs::wir::Value::Call { name, .. } = &node.value else {
                return None;
            };
            Some(name.as_str())
        })
        .collect();
    for expected in ["randomReal", "randomValueInArray"] {
        assert!(
            value_calls.contains(expected),
            "real-world cake must lower value {expected}"
        );
    }
}

fn collect_action_calls<'a>(
    program: &'a workshop_rs::wir::Program,
    actions: &[workshop_rs::wir::ActionId],
    calls: &mut std::collections::BTreeSet<&'a str>,
) {
    for action_id in actions {
        match program.actions.get(*action_id).unwrap() {
            workshop_rs::wir::Action::Call { name, .. } => {
                calls.insert(name.as_str());
            }
            workshop_rs::wir::Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    collect_action_calls(program, &branch.body, calls);
                }
                if let Some(body) = else_body {
                    collect_action_calls(program, body, calls);
                }
            }
            workshop_rs::wir::Action::While { body, .. }
            | workshop_rs::wir::Action::ForGlobalVariable { body, .. }
            | workshop_rs::wir::Action::ForPlayerVariable { body, .. } => {
                collect_action_calls(program, body, calls);
            }
            _ => {}
        }
    }
}
