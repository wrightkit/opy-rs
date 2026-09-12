//! Canonical-WIR lowering evidence for member values.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::Value;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/synthetic/member-values")
}

#[test]
fn is_dummy_member_lowers_to_the_catalog_value_in_canonical_wir() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source is readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture resolves");
    let artifact = Compiler::new()
        .expect("compiler initializes")
        .compile_hir(&hir)
        .expect("isDummy lowers to canonical WIR");

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
    assert!(equivalent(
        &super::canonical_program(&artifact),
        &oracle_wir
    ));

    let program = super::canonical_program(&artifact);
    let rule = program.rules.first().expect("fixture has one rule");
    let condition = rule.conditions.first().expect("rule has one condition");
    let Value::Call {
        name: comparison,
        args: comparison_args,
    } = &condition.value
    else {
        panic!("expected condition comparison, got {:?}", condition.value);
    };
    assert_eq!(comparison, "==");
    assert_eq!(comparison_args.len(), 2);
    assert!(matches!(&comparison_args[1], Value::Bool(true)));
    let Value::Call { name, args } = &comparison_args[0] else {
        panic!("expected catalog member value call");
    };
    assert_eq!(name, "isDummy");
    assert_eq!(args.len(), 1);
    assert!(matches!(&args[0], Value::EventPlayer));
}
