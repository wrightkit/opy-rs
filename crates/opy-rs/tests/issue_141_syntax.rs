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
        "    goto target\n",
        "    goto loc + value\n",
        "    target:\n",
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
            label: Some(_),
            offset: None,
            span: Some(_)
        }
    ));
    assert!(matches!(
        &rule.actions[5],
        Stmt::Goto {
            label: None,
            offset: Some(_),
            span: Some(_)
        }
    ));
    assert!(matches!(&rule.actions[6], Stmt::Label { name, span: Some(_) } if name == "target"));
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
            "rule \"continue\":\n    @Event global\n    while A < 1:\n        continue\n",
            "continue statements",
        ),
        (
            "rule \"goto\":\n    @Event global\n    goto target\n",
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
