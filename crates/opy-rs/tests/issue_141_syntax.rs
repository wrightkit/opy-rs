use std::path::Path;

use opy_rs::hir::types::{Expr, Stmt};

#[test]
fn issue_141_surface_reaches_validated_hir_with_spans() {
    let source = concat!(
        "globalvar value\n",
        "rule \"syntax surface\":\n",
        "    @Event global\n",
        "    del value[1]\n",
        "    value min= 2\n",
        "    value max= 3\n",
        "    while value < 4:\n",
        "        continue\n",
        "    goto RULE_START\n",
        "    goto target\n",
        "    goto loc + value\n",
        "    target:\n",
        "    return\n",
    );
    let program = opy_rs::compile(source, "issue-141.opy", Path::new(""))
        .expect("the issue-141 source surface must lower to HIR");
    program.validate().expect("the generated HIR must validate");

    let rule = match &program.rules[0] {
        opy_rs::hir::types::RuleEntry::Rule(rule) => rule,
        other => panic!("expected rule, got {other:?}"),
    };
    assert!(matches!(
        rule.actions[0],
        Stmt::Delete { span: Some(_), .. }
    ));
    assert!(matches!(
        &rule.actions[1],
        Stmt::Assign {
            value,
            span: Some(_),
            ..
        } if matches!(value.as_ref(), Expr::Binary { op, .. } if op == "min")
    ));
    assert!(matches!(
        &rule.actions[2],
        Stmt::Assign {
            value,
            span: Some(_),
            ..
        } if matches!(value.as_ref(), Expr::Binary { op, .. } if op == "max")
    ));
    let Stmt::While {
        body,
        span: Some(_),
        ..
    } = &rule.actions[3]
    else {
        panic!("expected while with a source span");
    };
    assert!(matches!(
        body.as_slice(),
        [Stmt::Continue { span: Some(_) }]
    ));
    assert!(matches!(
        &rule.actions[4],
        Stmt::Goto {
            label: None,
            offset: None,
            rule_start: true,
            span: Some(_),
        }
    ));
    assert!(matches!(
        &rule.actions[5],
        Stmt::Goto {
            label: Some(_),
            offset: None,
            rule_start: false,
            span: Some(_),
        }
    ));
    assert!(matches!(
        &rule.actions[6],
        Stmt::Goto {
            label: None,
            offset: Some(_),
            rule_start: false,
            span: Some(_),
        }
    ));
    assert!(matches!(&rule.actions[7], Stmt::Label { name, span: Some(_) } if name == "target"));
    assert!(matches!(&rule.actions[8], Stmt::Return { span: Some(_) }));
}

#[test]
fn issue_141_return_reaches_abort_if_in_canonical_wir() {
    let source = "rule \"return\":\n    @Event global\n    return\n";
    let artifact = opy_rs::Compiler::new()
        .expect("the compiler contract loads")
        .compile_source(source, "issue-141-return.opy", Path::new(""))
        .expect("return must lower to a canonical abort action");
    assert!(artifact.emitted_workshop.contains("Abort If(True)"));
}

#[test]
fn issue_141_invalid_statement_contexts_are_source_diagnostics() {
    let cases = [
        (
            "rule \"invalid delete\":\n    @Event global\n    del value\n",
            "parse-error",
        ),
        (
            "rule \"invalid goto\":\n    @Event global\n    goto loc\n",
            "parse-error",
        ),
        (
            "rule \"outside continue\":\n    @Event global\n    continue\n",
            "continue-context",
        ),
    ];
    for (source, expected_code) in cases {
        let error = opy_rs::compile(source, "issue-141-invalid.opy", Path::new(""))
            .expect_err("invalid issue-141 form unexpectedly compiled");
        assert_eq!(error.code, expected_code, "source: {source}");
        assert!(
            error.span.is_some(),
            "source diagnostic lost its span: {source}"
        );
    }
}

#[test]
fn issue_141_backend_boundary_is_explicit_for_source_only_statements() {
    let cases = [
        (
            "rule \"delete\":\n    @Event global\n    del A[1]\n",
            "delete statements",
        ),
        (
            "rule \"goto\":\n    @Event global\n    goto target\n",
            "goto statements",
        ),
        (
            "rule \"goto rule start\":\n    @Event global\n    goto RULE_START\n",
            "goto statements",
        ),
        (
            "rule \"label\":\n    @Event global\n    target:\n",
            "labels",
        ),
    ];
    let compiler = opy_rs::Compiler::new().expect("the compiler contract loads");
    for (source, expected_text) in cases {
        let error = compiler
            .compile_source(source, "issue-141-backend.opy", Path::new(""))
            .expect_err("source-only syntax must not be silently discarded");
        assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
        assert!(error.diagnostic.message.contains(expected_text));
        assert!(error.diagnostic.span.is_some());
    }
}

#[test]
fn issue_141_continue_reaches_canonical_loop_control() {
    let source = concat!(
        "globalvar A\n",
        "rule \"continue\":\n",
        "    @Event global\n",
        "    while A < 1:\n",
        "        if A == 0:\n",
        "            continue\n",
        "        wait()\n",
    );
    let artifact = opy_rs::Compiler::new()
        .expect("the compiler contract loads")
        .compile_source(source, "issue-141-continue.opy", Path::new(""))
        .expect("continue must lower to canonical loop control");
    assert!(artifact.emitted_workshop.contains("Skip If("));
}

#[test]
fn issue_141_nested_continue_uses_the_nearest_loop_and_preserves_tail_flow() {
    let source = concat!(
        "globalvar A\n",
        "rule \"nested continue\":\n",
        "    @Event global\n",
        "    while A < 2:\n",
        "        if A == 0:\n",
        "            if A < 1:\n",
        "                continue\n",
        "            A = 1\n",
        "            A = A + 10\n",
        "        A = A + 1\n",
    );
    let artifact = opy_rs::Compiler::new()
        .expect("the compiler contract loads")
        .compile_source(source, "issue-141-nested-continue.opy", Path::new(""))
        .expect("nested continue must lower to canonical loop control");
    assert!(
        artifact
            .emitted_workshop
            .contains("Skip If(Compare(Global.A, <, 1), 4);")
    );
    assert!(
        artifact
            .emitted_workshop
            .contains("Modify Global Variable(A, Add, 1);")
    );
}

#[test]
fn issue_141_nested_loop_continue_targets_the_innermost_loop() {
    let source = concat!(
        "globalvar A\n",
        "rule \"nested loops\":\n",
        "    @Event global\n",
        "    while A < 2:\n",
        "        while A < 1:\n",
        "            continue\n",
        "            A = A + 10\n",
        "        A = A + 1\n",
    );
    let artifact = opy_rs::Compiler::new()
        .expect("the compiler contract loads")
        .compile_source(source, "issue-141-nested-loops.opy", Path::new(""))
        .expect("nested loop continue must lower");
    assert!(
        artifact
            .emitted_workshop
            .contains("While(Compare(Global.A, <, 1));\n                Skip(1);")
    );
    assert!(
        artifact
            .emitted_workshop
            .contains("Modify Global Variable(A, Add, 1);")
    );
}

#[test]
fn issue_141_continue_skips_later_conditional_branches() {
    let source = concat!(
        "globalvar A\n",
        "rule \"continue if else\":\n",
        "    @Event global\n",
        "    while A < 2:\n",
        "        if A == 0:\n",
        "            continue\n",
        "        else:\n",
        "            A = A + 10\n",
        "        A = A + 1\n",
    );
    let artifact = opy_rs::Compiler::new()
        .expect("the compiler contract loads")
        .compile_source(source, "issue-141-continue-if-else.opy", Path::new(""))
        .expect("continue in an if/else must lower to canonical loop control");
    assert!(artifact.emitted_workshop.contains("Skip(4);"));
    assert!(artifact.emitted_workshop.contains("Else;"));
}

#[test]
fn issue_141_multiline_grouped_expressions_reach_hir_with_spans() {
    let source = concat!(
        "rule \"multiline expressions\":\n",
        "    @Event global\n",
        "    debug(\n",
        "        (\n",
        "            1\n",
        "            + 2\n",
        "        ) if (\n",
        "            true\n",
        "        ) else (\n",
        "            3\n",
        "            * 4\n",
        "        )\n",
        "    )\n",
    );
    let program = opy_rs::compile(source, "issue-141-expressions.opy", Path::new(""))
        .expect("multiline grouped expressions must reach HIR");
    program.validate().expect("the generated HIR must validate");

    let rule = match &program.rules[0] {
        opy_rs::hir::types::RuleEntry::Rule(rule) => rule,
        other => panic!("expected rule, got {other:?}"),
    };
    let Stmt::Expr { expr, .. } = &rule.actions[0] else {
        panic!("expected an expression statement");
    };
    let Expr::Call { args, span, .. } = &**expr else {
        panic!("expected a call expression, got {expr:?}");
    };
    assert_eq!(span.unwrap().start.line, 3);
    assert_eq!(span.unwrap().end.line, 13);
    let Expr::Conditional {
        then_value,
        condition,
        else_value,
        span,
    } = &args[0]
    else {
        panic!("expected a conditional value, got {:?}", args[0]);
    };
    assert_eq!(span.unwrap().start.line, 5);
    assert_eq!(span.unwrap().end.line, 11);
    assert!(matches!(
        then_value.as_ref(),
        Expr::Binary { op, .. } if op == "+"
    ));
    assert!(matches!(
        else_value.as_ref(),
        Expr::Binary { op, .. } if op == "*"
    ));
    assert_eq!(then_value.span().unwrap().start.line, 5);
    assert_eq!(condition.span().unwrap().start.line, 8);
    assert_eq!(else_value.span().unwrap().start.line, 10);
}

#[test]
fn issue_141_multiline_conditional_missing_else_is_a_source_diagnostic() {
    let source = concat!(
        "rule \"invalid multiline expression\":\n",
        "    @Event global\n",
        "    debug(\n",
        "        1 if\n",
        "        true\n",
        "    )\n",
    );
    let error = opy_rs::compile(source, "issue-141-invalid-expression.opy", Path::new(""))
        .expect_err("a conditional without else must remain rejected");
    assert_eq!(error.code, "parse-error");
    assert!(error.message.contains("expected `else`"));
    assert!(error.span.is_some());
}
