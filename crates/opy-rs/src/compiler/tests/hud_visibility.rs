//! Canonical WIR lowering coverage for HUD visibility.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::{Action, Value};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic/hud-visibility")
}

fn oracle_workshop() -> String {
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("oracle.json"))
            .expect("oracle must be readable"),
    )
    .expect("oracle must parse");
    value["compile"]["workshop"]
        .as_str()
        .expect("oracle must contain Workshop output")
        .to_string()
}

#[test]
fn never_maps_to_visible_never_in_canonical_wir() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source is readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_hir(&hir)
        .expect("SpecVisibility.NEVER must lower");

    let catalog = Catalog::builtin().expect("catalog must load");
    let oracle = workshop_rs::parser::parse(&oracle_workshop(), &catalog, &Locale::new("en-US"))
        .expect("oracle output must reparse");
    assert!(
        equivalent(&artifact.wir, &oracle),
        "SpecVisibility.NEVER WIR diverged from the pinned oracle\n--- native ---\n{}\n--- oracle ---\n{}",
        artifact.emitted,
        oracle_workshop()
    );

    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .expect("fixture has one rule");
    let Action::Call { args, .. } = artifact.wir.actions.get(rule.actions[0]).unwrap() else {
        panic!("hudSubheader must lower to a canonical action call");
    };
    assert!(matches!(
        &artifact.wir.values.get(args[10]).unwrap().value,
        Value::Enum { value_type, value }
            if value_type == "SpecVisibility" && value == "VISIBLE_NEVER"
    ));
}

#[test]
fn invalid_spec_visibility_members_keep_source_attributed_diagnostics() {
    let source = "rule \"r\":\n    @Event global\n    hudSubheader(getAllPlayers(), \"text\", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.INVALID)\n";
    let error =
        crate::compile(source, "source.opy", Path::new(".")).expect_err("invalid member must fail");
    assert_eq!(error.code, "unknown-enum-member");
    assert_eq!(
        error.message,
        "enum 'SpecVisibility' has no member 'INVALID'"
    );
    let span = error.span.expect("diagnostic provenance");
    assert_eq!(span.start.line, 3);
    assert_eq!(span.start.col, 98);
}
