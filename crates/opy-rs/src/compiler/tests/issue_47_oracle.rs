//! Oracle-backed control-flow lowering evidence for issue #47.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::{self, Action, Value};

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
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
        equivalent(&artifact.wir, &oracle),
        "native WIR diverged\n{}",
        artifact.emitted
    );
}

#[derive(Debug, PartialEq, Eq)]
enum NativeInstruction {
    If,
    Else,
    End,
    Skip(wir::ValueId),
    SetGlobal(i64),
}

fn flatten_action(
    program: &wir::Program,
    action_id: wir::ActionId,
    rule_final: bool,
    instructions: &mut Vec<NativeInstruction>,
) {
    match program.actions.get(action_id).unwrap() {
        Action::SetGlobalVariable { value, .. } => {
            let Value::Number { value, .. } = &program.values.get(*value).unwrap().value else {
                panic!("switch behavior probe expects numeric assignments")
            };
            instructions.push(NativeInstruction::SetGlobal(*value as i64));
        }
        Action::If {
            branches,
            else_body,
            ..
        } => {
            for (index, branch) in branches.iter().enumerate() {
                if index > 0 {
                    instructions.push(NativeInstruction::Else);
                }
                instructions.push(NativeInstruction::If);
                for action in &branch.body {
                    flatten_action(program, *action, false, instructions);
                }
            }
            if let Some(else_body) = else_body {
                instructions.push(NativeInstruction::Else);
                for action in else_body {
                    flatten_action(program, *action, false, instructions);
                }
            }
            if !rule_final {
                instructions.push(NativeInstruction::End);
            }
        }
        Action::Call { name, args, .. } if name == "skip" => {
            instructions.push(NativeInstruction::Skip(args[0]));
        }
        other => panic!("switch behavior probe found unexpected action: {other:?}"),
    }
}

fn array_values(program: &wir::Program, value_id: wir::ValueId) -> Vec<wir::ValueId> {
    match &program.values.get(value_id).unwrap().value {
        Value::Array(values) => values.clone(),
        Value::Call { name, args } if name == "array" => args.clone(),
        other => panic!("expected an array value, got {other:?}"),
    }
}

fn numeric_value(program: &wir::Program, value_id: wir::ValueId, selector: i64) -> i64 {
    match &program.values.get(value_id).unwrap().value {
        Value::Number { value, .. } => *value as i64,
        Value::Null => 0,
        Value::GlobalVariable(_) => selector,
        Value::Array(values) => panic!("array value must be consumed by a call: {values:?}"),
        Value::Call { name, args } => match name.as_str() {
            "add" => args
                .iter()
                .map(|value| numeric_value(program, *value, selector))
                .sum(),
            "indexOfArrayValue" => {
                let values = array_values(program, args[0]);
                let needle = numeric_value(program, args[1], selector);
                values
                    .iter()
                    .position(|value| numeric_value(program, *value, selector) == needle)
                    .map_or(-1, |index| index as i64)
            }
            "valueInArray" => {
                let values = array_values(program, args[0]);
                let index = numeric_value(program, args[1], selector);
                numeric_value(program, values[index as usize], selector)
            }
            other => panic!("switch behavior probe found unexpected value call: {other}"),
        },
        other => panic!("switch behavior probe found unexpected value: {other:?}"),
    }
}

fn matching_end(instructions: &[NativeInstruction], else_index: usize) -> usize {
    let mut nested = 0;
    for (index, instruction) in instructions.iter().enumerate().skip(else_index + 1) {
        match instruction {
            NativeInstruction::If => nested += 1,
            NativeInstruction::End if nested == 0 => return index,
            NativeInstruction::End => nested -= 1,
            _ => {}
        }
    }
    instructions.len()
}

fn native_switch_trace(program: &wir::Program, rule: &wir::Rule, selector: i64) -> Vec<i64> {
    let mut instructions = Vec::new();
    for (index, action) in rule.actions.iter().enumerate() {
        flatten_action(
            program,
            *action,
            index + 1 == rule.actions.len(),
            &mut instructions,
        );
    }

    let mut trace = Vec::new();
    let mut if_stack = Vec::new();
    let mut pc = 0;
    while pc < instructions.len() {
        match &instructions[pc] {
            NativeInstruction::If => {
                if_stack.push(true);
                pc += 1;
            }
            NativeInstruction::Else => {
                if if_stack.pop().unwrap_or(false) {
                    pc = matching_end(&instructions, pc) + 1;
                } else {
                    if_stack.push(true);
                    pc += 1;
                }
            }
            NativeInstruction::End => {
                if_stack.pop();
                pc += 1;
            }
            NativeInstruction::Skip(value) => {
                let count = numeric_value(program, *value, selector);
                assert!(count >= 0, "switch skip count must be non-negative");
                pc += count as usize + 1;
            }
            NativeInstruction::SetGlobal(value) => {
                trace.push(*value);
                pc += 1;
            }
        }
    }
    trace
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
fn issue_47_switch_lowering_matches_the_pinned_oracle() {
    for name in [
        "issue-33-switch-break",
        "issue-47-control-flow",
        "issue-47-switch-order",
        "issue-47-switch-structured-target",
    ] {
        assert_native_wir_equivalent(name);
    }
}

#[test]
fn issue_47_do_while_break_shapes_match_the_pinned_oracle() {
    assert_native_wir_equivalent("issue-47-do-while-shapes");
}

#[test]
fn issue_47_multiple_switch_breaks_match_independent_semantic_oracle() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-switch-multiple-break");
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
        equivalent(&artifact.wir, &semantic_wir),
        "native WIR diverged from the independent switch semantic oracle\n{}",
        artifact.emitted
    );
}

#[test]
fn pinned_overpy_switch_action_trace() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-switch-multiple-break");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let artifact = compiler
        .compile_hir(&hir)
        .expect("multi-break switch must lower");
    let rule = artifact
        .wir
        .rules
        .iter()
        .find(|rule| rule.name == "issue 47 switch multiple break")
        .unwrap();
    for (selector, expected) in pinned_switch_traces(&dir) {
        assert_eq!(
            native_switch_trace(&artifact.wir, rule, selector),
            expected,
            "native switch behavior diverged from the pinned OverPy action trace for selector {selector}"
        );
    }
}

#[test]
fn issue_47_invalid_do_while_placement_is_source_attributed() {
    let dir = fixture_dir("issue-47-do-while-invalid-placement");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let error = crate::compile(&source, "source.opy", &dir)
        .expect_err("invalid do-while placement must be rejected");
    assert_eq!(error.code, "do-while-placement");
    assert_eq!(error.span.unwrap().start.line, 6);
}

#[test]
fn issue_47_nested_switch_break_is_source_attributed_when_not_representable() {
    let compiler = Compiler::new().unwrap();
    let dir = fixture_dir("issue-47-unsupported");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).unwrap();
    let error = match compiler.compile_hir(&hir) {
        Ok(_) => panic!("nested switch break unexpectedly lowered"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(error.diagnostic.span.unwrap().start.line, 7);
}
