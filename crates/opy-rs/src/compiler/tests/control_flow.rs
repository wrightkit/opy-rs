//! Oracle-backed control-flow lowering and behavior evidence.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

fn oracle_workshop(dir: &Path) -> String {
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle.json must be readable"),
    )
    .expect("oracle snapshot must parse");
    oracle["compile"]["workshop"]
        .as_str()
        .expect("oracle snapshot must contain Workshop text")
        .to_string()
}

fn assert_native_wir_equivalent(name: &str) {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let locale = Locale::new("en-US");
    let oracle = workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &locale).unwrap();

    assert!(
        equivalent(&super::canonical_program(&artifact), &oracle),
        "native WIR diverged\n{}",
        artifact.emitted
    );
}

fn pinned_switch_traces(dir: &Path) -> Vec<(i64, Vec<i64>)> {
    let workshop = oracle_workshop(dir);
    let offsets = workshop
        .split("Skip(Value In Array(Array(")
        .nth(1)
        .unwrap()
        .split("), Add")
        .next()
        .unwrap()
        .split(", ")
        .map(|value| value.parse::<i64>().unwrap())
        .collect::<Vec<_>>();
    let mut actions = Vec::new();
    for line in workshop.lines() {
        if let Some(value) = line
            .trim()
            .strip_prefix("Set Global Variable(value, ")
            .and_then(|line| line.strip_suffix(");"))
        {
            actions.push(value.parse::<i64>().unwrap());
        } else if line.trim() == "Else;" {
            actions.push(-1);
        }
    }
    let trace_at = |offset: i64| {
        let mut trace = Vec::new();
        for action in actions.iter().skip(offset as usize) {
            if *action == -1 {
                break;
            }
            trace.push(*action);
        }
        trace
    };
    [0, 1, 2, 3, 99]
        .into_iter()
        .map(|selector| {
            let case_offset = [1, 2, 3]
                .iter()
                .position(|value| *value == selector)
                .map_or(offsets[0], |index| offsets[index + 1]);
            (selector, trace_at(case_offset))
        })
        .collect()
}

#[test]
fn switch_lowering_matches_the_pinned_oracle() {
    for name in [
        "control-flow",
        "control-flow-lowering",
        "switch-break",
        "switch-order",
        "switch-structured-target",
    ] {
        assert_native_wir_equivalent(name);
    }
}

#[test]
fn control_flow_debug_lowers_to_a_native_hud_action() {
    let dir = fixture_dir("control-flow");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    let program = super::canonical_program(&artifact);
    let rule = &program.rules[0];
    let workshop_rs::Action::ForGlobalVariable { .. } = &rule.actions[0] else {
        panic!("control-flow fixture must lower its for loop");
    };
    let workshop_rs::Action::If { .. } = &rule.actions[1] else {
        panic!("control-flow fixture must lower its conditional");
    };
    let workshop_rs::Action::Call { name, .. } = &rule.actions[2] else {
        panic!("control-flow fixture debug must lower to a native HUD action");
    };
    assert_eq!(name, "createHudText");
}

#[test]
fn aggressive_size_optimization_lowers_a_tail_comparison_to_skip_if() {
    let source = "#!optimizeForSize\n#!optimizeForSizeAggressive\nglobalvar g\nrule \"r\":\n    @Event global\n    if g == 1:\n        g = 2\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(
        artifact
            .emitted
            .contains("Skip If(Not(Compare(Global.g, ==, 1)), 1);")
    );
    assert!(!artifact.emitted.contains("If(Compare(Global.g, ==, 1));"));
}

#[test]
fn aggressive_size_optimization_respects_directive_boundaries() {
    let source = "globalvar g\nrule \"before\":\n    @Event global\n    if g == 1:\n        g = 2\n#!optimizeForSize\n#!optimizeForSizeAggressive\nrule \"after\":\n    @Event global\n    if g == 1:\n        g = 2\n#!disableOptimizations\nrule \"disabled\":\n    @Event global\n    if g == 1:\n        g = 2\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert_eq!(artifact.emitted.matches("Skip If(").count(), 1);
    assert_eq!(
        artifact
            .emitted
            .matches("If(Compare(Global.g, ==, 1));")
            .count(),
        2
    );
}

#[test]
fn conditional_forward_gotos_lower_to_single_evaluation_skips() {
    let source = r#"
globalvar state

rule "conditional goto":
    @Event global
    if state == 1:
        goto done
    state = 2
    done:
    state = 3

rule "nested conditional goto":
    @Event global
    if state == 1:
        if state == 2:
            goto nested_done
    state = 4
    nested_done:
    state = 5

rule "conditional goto with else":
    @Event global
    if state == 1:
        goto else_done
    else:
        state = 6
    state = 7
    else_done:
    state = 8

rule "leading goto with tail":
    @Event global
    if state == 1:
        goto leading_done
        state = 9
    leading_done:
    state = 10
"#;
    let hir = crate::compile(source, "source.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(artifact.emitted.contains(
        "Skip If(Compare(Global.state, ==, 1), 1);\n        Set Global Variable(state, 2);\n        Set Global Variable(state, 3);"
    ));
    assert!(artifact.emitted.contains(
        "Skip If(And(Compare(Global.state, ==, 1), Compare(Global.state, ==, 2)), 1);\n        Set Global Variable(state, 4);\n        Set Global Variable(state, 5);"
    ));
    assert!(artifact.emitted.contains(
        "If(Compare(Global.state, ==, 1));\n            Skip(1);\n        Else;\n            Set Global Variable(state, 6);\n        End;\n        Set Global Variable(state, 7);\n        Set Global Variable(state, 8);"
    ));
    assert!(artifact.emitted.contains(
        "Skip If(Compare(Global.state, ==, 1), 1);\n        Set Global Variable(state, 9);\n        Set Global Variable(state, 10);"
    ));
    assert_eq!(artifact.emitted.matches("Skip If(").count(), 3);
}

#[test]
fn do_while_break_shapes_match_the_pinned_oracle() {
    assert_native_wir_equivalent("do-while-break");
}

#[test]
fn multiple_switch_breaks_match_independent_semantic_oracle() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("switch-multiple-break");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let artifact = compiler
        .compile_hir(&hir)
        .expect("multi-break switch must lower");
    let semantic_oracle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("semantic-oracle.json")).unwrap())
            .unwrap();
    let catalog = Catalog::builtin().unwrap();
    let semantic_wir = workshop_rs::parser::parse(
        semantic_oracle["compile"]["workshop"].as_str().unwrap(),
        &catalog,
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(
        equivalent(&super::canonical_program(&artifact), &semantic_wir),
        "native WIR diverged from the independent switch semantic oracle\n{}",
        artifact.emitted
    );
}

#[test]
fn pinned_overpy_switch_action_trace() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("switch-multiple-break");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let artifact = compiler
        .compile_hir(&hir)
        .expect("multi-break switch must lower");
    let expected = pinned_switch_traces(&dir);
    assert!(!expected.is_empty());
    assert!(artifact.emitted.contains("Skip(Value In Array"));
}

#[test]
fn invalid_do_while_placement_is_source_attributed() {
    let dir = fixture_dir("do-while-invalid");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let error = crate::compile(&source, "source.opy", &dir)
        .expect_err("invalid do-while placement must be rejected");
    assert_eq!(error.code, "do-while-placement");
    assert_eq!(error.span.unwrap().start.line, 6);
}

#[test]
fn nested_switch_break_matches_upstream_noop_elision() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("switch-break-unsupported");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let artifact = compiler
        .compile_hir(&hir)
        .expect("nested switch break must follow the pinned upstream lowering");
    assert!(artifact.wir.rules.is_empty());
}
