//! Canonical-WIR lowering evidence for member angles.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;
use workshop_rs::wir::{Action, Value};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic/member-angle")
}

#[test]
fn horizontal_facing_angle_member_matches_the_pinned_canonical_wir() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source is readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("compiler initializes")
        .compile_hir(&hir)
        .expect("getHorizontalFacingAngle lowers to canonical WIR");

    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle is readable"),
    )
    .expect("oracle is valid JSON");
    let oracle_wir = workshop_rs::parser::parse(
        oracle["compile"]["workshop"]
            .as_str()
            .expect("oracle workshop text is present"),
        &Catalog::builtin().expect("catalog is available"),
        &Locale::new("en-US"),
    )
    .expect("oracle workshop text parses");
    assert!(equivalent(&artifact.wir, &oracle_wir));

    let rule = artifact
        .wir
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .expect("fixture has one rule");
    let Action::SetGlobalVariable {
        variable, value, ..
    } = artifact
        .wir
        .actions
        .get(rule.actions[0])
        .expect("rule captures the member value")
    else {
        panic!("expected direct global assignment");
    };
    assert_eq!(
        artifact
            .wir
            .global_variables
            .get(*variable)
            .expect("global variable")
            .name,
        "horizontalAngle"
    );
    let Value::Call { name, args } = &artifact
        .wir
        .values
        .get(*value)
        .expect("horizontal facing angle value")
        .value
    else {
        panic!("expected catalog member value call");
    };
    assert_eq!(name, "getHorizontalFacingAngle");
    assert_eq!(args.len(), 1);
    assert!(matches!(
        &artifact
            .wir
            .values
            .get(args[0])
            .expect("receiver value")
            .value,
        Value::EventPlayer
    ));
}

#[test]
fn horizontal_facing_angle_arity_failure_keeps_source_provenance() {
    let source = "rule \"r\":\n    @Event eachPlayer\n    @Condition eventPlayer.getHorizontalFacingAngle(1) == 0\n";
    let error = crate::compile(source, "source.opy", Path::new("."))
        .expect_err("member arity must be rejected");
    assert_eq!(error.code, "invalid-arity");
    assert_eq!(
        error
            .span
            .expect("diagnostic is source-attributed")
            .start
            .line,
        3
    );
}
