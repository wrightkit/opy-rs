//! Oracle evidence for residual native-WIR lowering cases (issue #89).

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::Action;

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

fn compile_fixture(name: &str) -> (crate::CompilationArtifact, workshop_rs::wir::Program) {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR");
    let catalog = Catalog::builtin().expect("catalog must load");
    let oracle =
        workshop_rs::parser::parse(&oracle_workshop(&dir), &catalog, &Locale::new("en-US"))
            .expect("oracle output must reparse");
    (artifact, oracle)
}

#[test]
fn issue_89_residual_lowering_cases_match_the_pinned_oracle() {
    for name in [
        "control-flow",
        "issue-33-switch-break",
        "issue-46-primitives",
        "issue-47-control-flow",
        "issue-47-switch-order",
        "issue-47-switch-structured-target",
    ] {
        let (artifact, oracle) = compile_fixture(name);
        assert!(
            equivalent(&artifact.wir, &oracle),
            "native WIR diverged for {name}\n{}",
            artifact.emitted
        );
    }
}

#[test]
fn issue_89_debug_lowers_to_a_native_hud_action() {
    let (artifact, oracle) = compile_fixture("control-flow");
    assert!(equivalent(&artifact.wir, &oracle));

    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .unwrap();
    let for_action = artifact.wir.actions.get(rule.actions[0]).unwrap();
    let Action::ForGlobalVariable { body, .. } = for_action else {
        panic!("control-flow fixture must lower its for loop");
    };
    let if_action = artifact.wir.actions.get(body[0]).unwrap();
    let Action::If { branches, .. } = if_action else {
        panic!("control-flow fixture must lower its conditional");
    };
    let hud_action = artifact.wir.actions.get(branches[0].body[0]).unwrap();
    let Action::Call { name, span, .. } = hud_action else {
        panic!("control-flow fixture debug must lower to a native HUD action");
    };
    assert_eq!(name, "createHudText");
    assert_eq!(span.unwrap().start.line, 7);
}
