//! Oracle-backed player-variable range-for evidence for issue #65.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::{Action, Value};

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

#[test]
fn player_variable_range_binder_matches_the_pinned_oracle() {
    let dir = fixture_dir("issue-65-player-range");
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
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR");
    let catalog = Catalog::builtin().expect("catalog must load");
    let oracle =
        workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &Locale::new("en-US"))
            .expect("oracle output must reparse");

    assert!(
        equivalent(&artifact.wir, &oracle),
        "native WIR diverged\n{}",
        artifact.emitted
    );

    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .expect("fixture must contain one rule");
    let Action::ForPlayerVariable { player, span, .. } =
        artifact.wir.actions.get(rule.actions[0]).unwrap()
    else {
        panic!("fixture must lower to For Player Variable");
    };
    assert_eq!(span.unwrap().start.line, 5);
    assert!(matches!(
        artifact.wir.values.get(*player).unwrap().value,
        Value::Call { ref name, ref args } if name == "hostPlayer" && args.is_empty()
    ));
}
