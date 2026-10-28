use std::path::{Path, PathBuf};

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

#[test]
fn player_variable_range_binder_keeps_its_member_span() {
    let dir = fixture_dir("range-player-variable");
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let crate::hir::RuleEntry::Rule(rule) = &hir.rules[0] else {
        panic!("fixture must contain one rule");
    };
    let crate::hir::Stmt::For { variable, .. } = &rule.actions[0] else {
        panic!("fixture must contain a for statement");
    };
    let crate::hir::Expr::PlayerVar { member_span, .. } = variable.as_ref() else {
        panic!("fixture must contain a player-variable binder");
    };
    let member_span = member_span.expect("player binder member span");
    assert_eq!(member_span.start.line, 5);
    assert_eq!(member_span.start.col, 20);
    assert_eq!(member_span.end.line, 5);
    assert_eq!(member_span.end.col, 21);
    let hir_json = serde_json::to_value(&hir).expect("HIR must serialize");
    assert_eq!(
        hir_json["rules"][0]["actions"][0]["variable"]["member_span"]["start"]["col"],
        20
    );
}
