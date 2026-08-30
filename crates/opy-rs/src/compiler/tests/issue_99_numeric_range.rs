//! Numeric range setting coverage for issue #99.

use std::path::{Path, PathBuf};

use crate::hir::Expr;
use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::Locale;
use workshop_rs::wir::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/real-world/overpy-broken-weapons")
}

#[test]
fn minimized_numeric_range_reaches_canonical_setting_value() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("regressions/numeric-range-setting.opy"))
        .expect("minimized regression must be readable");
    let compiler = Compiler::new().expect("released Workshop contract must load");
    let artifact = compiler
        .compile_source_with_locale(
            &source,
            "regressions/numeric-range-setting.opy",
            &dir,
            &Locale::new("en-US"),
        )
        .expect("numeric range setting must lower");

    let setting = (0..artifact.wir.values.len())
        .filter_map(|index| {
            artifact
                .wir
                .values
                .get(workshop_rs::wir::ValueId::from_index(index))
        })
        .find(|node| {
            matches!(&node.value, Value::Call { name, .. } if name == "createWorkshopSettingFloat")
        })
        .expect("global initializer must contain the canonical setting value");
    let Value::Call { name, args } = &setting.value else {
        unreachable!("the search above only returns call values");
    };
    assert_eq!(name, "createWorkshopSettingFloat");
    assert_eq!(args.len(), 6);
    assert!(matches!(
        artifact.wir.values.get(args[0]).map(|node| &node.value),
        Some(Value::String(value)) if value == "\u{3000}"
    ));
    assert!(matches!(
        artifact.wir.values.get(args[3]).map(|node| &node.value),
        Some(Value::Number { value, .. }) if *value == 0.5
    ));
    assert!(matches!(
        artifact.wir.values.get(args[4]).map(|node| &node.value),
        Some(Value::Number { value, .. }) if *value == 10.0
    ));
}

#[test]
fn numeric_range_preserves_hir_type_span_and_round_trip() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("regressions/numeric-range-setting.opy"))
        .expect("minimized regression must be readable");
    let hir = crate::compile(&source, "numeric-range-setting.opy", &dir)
        .expect("numeric range source must resolve");
    hir.validate().expect("numeric range HIR must validate");
    let round_trip = crate::hir::parse_value(
        serde_json::to_value(&hir).expect("numeric range HIR must serialize"),
    )
    .expect("numeric range HIR must round-trip");
    round_trip
        .validate()
        .expect("round-tripped HIR must validate");
    assert_eq!(hir.dump(), round_trip.dump());

    let Expr::Call { args, .. } = find_initializer(&hir) else {
        panic!("expected createWorkshopSetting initializer");
    };
    let Expr::Type { name, args, span } = &args[0] else {
        panic!("expected a type literal as the first setting argument");
    };
    assert_eq!(name, "float");
    assert_eq!(args.len(), 2);
    assert_eq!(span.unwrap().start.line, 1);
    assert_eq!(span.unwrap().start.col, 46);
    assert_eq!(span.unwrap().end.col, 59);
}

#[test]
fn malformed_numeric_range_keeps_frontend_diagnostic_boundary() {
    let source = "globalvar value = createWorkshopSetting(float[0.5:10, \"\", \"name\", 1, 0)\n";
    let report = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_report_with_locale(
            source,
            "numeric-range-invalid.opy",
            Path::new("."),
            &Locale::new("en-US"),
        );
    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert_eq!(
        report.compile.failure_class,
        Some(CompileFailureClass::Frontend)
    );
    assert_eq!(report.compile.diagnostics[0].code, "parse-error");
}

#[test]
fn motivating_project_advances_past_numeric_range_frontend_gap() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("broken_weapons.opy"))
        .expect("motivating project must be readable");
    let report = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_report_with_locale(
            &source,
            "broken_weapons.opy",
            &dir,
            &Locale::new("en-US"),
        );

    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert_eq!(report.compile.diagnostics[0].code, "unknown-value");
    assert!(
        report.compile.diagnostics[0]
            .message
            .contains("isAssemblingHeroes")
    );
}

fn find_initializer(program: &crate::hir::Program) -> &Expr {
    program
        .declarations
        .iter()
        .find_map(|declaration| match declaration {
            crate::hir::Declaration::GlobalVariable {
                initializer: Some(initializer),
                ..
            } => Some(initializer.as_ref()),
            _ => None,
        })
        .expect("global initializer must exist")
}
