//! End-to-end conditional-expression coverage.

use std::path::Path;

use crate::hir::Expr;
use crate::{CompilationArtifact, Compiler};
use workshop_rs::catalog::Locale;

fn conditional_calls(artifact: &CompilationArtifact) -> Vec<Vec<workshop_rs::Value>> {
    let program = super::canonical_program(artifact);
    let mut calls = Vec::new();
    for rule in &program.rules {
        for condition in &rule.conditions {
            collect_value(&condition.value, &mut calls);
        }
        for action in &rule.actions {
            collect_action(action, &mut calls);
        }
    }
    calls
}

fn collect_action(action: &workshop_rs::Action, calls: &mut Vec<Vec<workshop_rs::Value>>) {
    use workshop_rs::Action;
    match action {
        Action::SetGlobalVariable { value, .. }
        | Action::ModifyGlobalVariable { value, .. }
        | Action::AssignMember { value, .. } => collect_value(value, calls),
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            collect_value(player, calls);
            collect_value(value, calls);
        }
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            collect_value(condition, calls)
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            collect_value(start, calls);
            collect_value(stop, calls);
            collect_value(step, calls);
        }
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            collect_value(player, calls);
            collect_value(start, calls);
            collect_value(stop, calls);
            collect_value(step, calls);
        }
        Action::Call { args, .. } => {
            for arg in args {
                collect_value(arg, calls);
            }
        }
        Action::Disabled { action } => collect_action(action, calls),
        Action::CallSubroutine { .. } | Action::Else | Action::End => {}
    }
}

fn collect_value(value: &workshop_rs::Value, calls: &mut Vec<Vec<workshop_rs::Value>>) {
    match value {
        workshop_rs::Value::Call { name, args } => {
            if name == "ifThenElse" {
                calls.push(args.clone());
            }
            for arg in args {
                collect_value(arg, calls);
            }
        }
        workshop_rs::Value::Array(values) => {
            for value in values {
                collect_value(value, calls);
            }
        }
        workshop_rs::Value::Vector { x, y, z } => {
            collect_value(x, calls);
            collect_value(y, calls);
            collect_value(z, calls);
        }
        workshop_rs::Value::PlayerVariable { player, .. } => collect_value(player, calls),
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
fn chained_conditional_lowers_to_right_associative_canonical_values() {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/corpus/real-world/overpy-client-to-server/regressions/chained-ternary.opy",
    ))
    .expect("the minimized regression must be readable");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_with_locale(&source, "source.opy", Path::new("."), &Locale::new("en-US"))
        .expect("chained conditional must lower");

    let calls = conditional_calls(&artifact);
    assert!(
        calls.len() >= 2,
        "outer and nested conditional values must remain visible"
    );
    assert!(calls.iter().all(|args| args.len() == 3));
    assert!(calls.iter().any(|args| {
        matches!(args[0], workshop_rs::Value::Bool(true))
            && matches!(args[1], workshop_rs::Value::Number(1.0))
            && matches!(&args[2], workshop_rs::Value::Call { name, args }
                if name == "ifThenElse"
                    && matches!(args[0], workshop_rs::Value::Bool(false))
                    && matches!(args[1], workshop_rs::Value::Number(2.0))
                    && matches!(args[2], workshop_rs::Value::Number(3.0)))
    }));
}

#[test]
fn preprocessor_conditional_preserves_macro_argument_provenance() {
    let source = "#!define choose(value) value if value else 0\n\nglobalvar result\n\nrule \"r\":\n    @Event global\n    result = choose(1)\n";
    let hir = crate::compile(source, "source.opy", Path::new("."))
        .expect("macro-expanded conditional must parse");
    hir.validate().expect("conditional HIR must validate");
    let round_trip = crate::hir::parse_value(
        serde_json::to_value(&hir).expect("conditional HIR must serialize"),
    )
    .expect("conditional HIR must round-trip");
    round_trip
        .validate()
        .expect("round-tripped HIR must validate");
    assert_eq!(hir.dump(), round_trip.dump());
    let Expr::Conditional {
        span,
        then_value,
        condition,
        else_value,
    } = find_first_conditional(&hir)
    else {
        panic!("expected a conditional expression in the expanded HIR");
    };
    assert_eq!(span.unwrap().start.line, 7);
    assert_eq!(span.unwrap().end.line, 7);
    assert_eq!(then_value.span().unwrap().start.line, 7);
    assert_eq!(condition.span().unwrap().start.line, 7);
    assert_eq!(else_value.span().unwrap().start.line, 7);

    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("macro-expanded conditional must lower");
    assert_eq!(conditional_calls(&artifact).len(), 1);

    let invalid = "#!define broken(value) value if value\n\nglobalvar result\n\nrule \"r\":\n    @Event global\n    result = broken(1)\n";
    let error = crate::compile(invalid, "source.opy", Path::new("."))
        .expect_err("missing else in a macro expansion must remain a parse error");
    assert_eq!(error.code, "parse-error");
    assert_eq!(error.span.unwrap().start.line, 7);
}

fn find_first_conditional(program: &crate::hir::Program) -> &Expr {
    for entry in &program.rules {
        let crate::hir::RuleEntry::Rule(rule) = entry else {
            continue;
        };
        for statement in &rule.actions {
            let expression = match statement {
                crate::hir::Stmt::Expr { expr, .. }
                | crate::hir::Stmt::Assign { value: expr, .. } => expr,
                _ => continue,
            };
            if let Some(conditional) = find_conditional(expression) {
                return conditional;
            }
        }
    }
    panic!("conditional expression not found")
}

fn find_conditional(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Conditional { .. } => Some(expr),
        Expr::Call { args, .. } | Expr::MacroCall { args, .. } => {
            args.iter().find_map(find_conditional)
        }
        Expr::ReceiverCall { receiver, args, .. } => {
            find_conditional(receiver).or_else(|| args.iter().find_map(find_conditional))
        }
        _ => None,
    }
}
