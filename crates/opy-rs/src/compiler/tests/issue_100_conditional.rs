//! End-to-end conditional-expression coverage for issue #100.

use std::path::Path;

use crate::hir::Expr;
use crate::{CompilationArtifact, Compiler};
use workshop_rs::catalog::Locale;
use workshop_rs::wir::Value;

fn conditional_calls(artifact: &CompilationArtifact) -> Vec<Vec<workshop_rs::wir::ValueId>> {
    (0..artifact.wir.values.len())
        .filter_map(|index| {
            let node = artifact
                .wir
                .values
                .get(workshop_rs::wir::ValueId::from_index(index))?;
            let Value::Call { name, args } = &node.value else {
                return None;
            };
            (name == "ifThenElse").then(|| args.clone())
        })
        .collect()
}

#[test]
fn chained_conditional_lowers_to_right_associative_canonical_values() {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../compatibility/fixtures/real-world/overpy-client-to-server/regressions/chained-ternary.opy",
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
        matches!(
            artifact.wir.values.get(args[0]).map(|node| &node.value),
            Some(Value::Bool(true))
        ) && matches!(
            artifact.wir.values.get(args[1]).map(|node| &node.value),
            Some(Value::Number { value, .. }) if *value == 1.0
        ) && matches!(
            artifact.wir.values.get(args[2]).map(|node| &node.value),
            Some(Value::Call { name, args })
                if name == "ifThenElse"
                    && matches!(
                        artifact.wir.values.get(args[0]).map(|node| &node.value),
                        Some(Value::Bool(false))
                    )
                    && matches!(
                        artifact.wir.values.get(args[1]).map(|node| &node.value),
                        Some(Value::Number { value, .. }) if *value == 2.0
                    )
                    && matches!(
                        artifact.wir.values.get(args[2]).map(|node| &node.value),
                        Some(Value::Number { value, .. }) if *value == 3.0
                    )
        )
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
