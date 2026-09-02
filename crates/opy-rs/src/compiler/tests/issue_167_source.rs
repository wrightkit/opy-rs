//! Pinned source failure-frontier coverage for issue #167.

use std::path::{Path, PathBuf};

use crate::hir::SettingsNode;
use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::Locale;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(name)
}

#[test]
fn issue_29_reaches_the_duplicate_rule_name_frontier() {
    let dir = fixture_dir("synthetic/issue-29-invalid");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let report = Compiler::new().unwrap().compile_source_report_with_locale(
        &source,
        "source.opy",
        &dir,
        &Locale::new("en-US"),
    );

    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert_eq!(
        report.compile.failure_class,
        Some(CompileFailureClass::Frontend)
    );
    let diagnostic = report.compile.diagnostics.first().unwrap();
    assert_eq!(diagnostic.code, "duplicate-rule-name");
    assert!(
        diagnostic
            .message
            .contains("Rule name was already declared")
    );
    let span = diagnostic.span.as_ref().unwrap();
    assert_eq!(span.path, "source.opy");
    assert_eq!(span.start.line, 5);
}

#[test]
fn six_v_six_reaches_the_pinned_unknown_member_frontier() {
    let dir = fixture_dir("real-world/6v6-adjustments");
    let source = std::fs::read_to_string(dir.join("main.opy")).unwrap();
    let report = Compiler::new().unwrap().compile_source_report_with_locale(
        &source,
        "main.opy",
        &dir,
        &Locale::new("en-US"),
    );

    assert_eq!(report.compile.status, CompileStatus::Failure);
    assert_eq!(
        report.compile.failure_class,
        Some(CompileFailureClass::Frontend)
    );
    let diagnostic = report.compile.diagnostics.first().unwrap();
    assert_eq!(diagnostic.code, "unknown-member");
    assert!(diagnostic.message.contains("unknown member"));
    let span = diagnostic.span.as_ref().unwrap();
    assert_eq!(span.path, "utilities/custom_hp.opy");
    assert_eq!(span.start.line, 31);
}

#[test]
fn expression_valued_setting_stays_raw_through_compiler() {
    let source = "settings {\n    \"lobby\": {\n        \"modeName\": GAMEMODE_NAME\" \"GAMEMODE_VERSION,\n    },\n    \"gamemodes\": {}\n}\nrule \"r\":\n    @Event global\n    pass\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).unwrap();
    let lobby = match &hir.settings.as_ref().unwrap().children[0] {
        SettingsNode::Group { children, .. } => children,
        other => panic!("expected lobby group, got {other:?}"),
    };
    assert!(matches!(
        &lobby[0],
        SettingsNode::Raw { value, .. }
            if value == "GAMEMODE_NAME\" \"GAMEMODE_VERSION"
    ));

    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact
            .emitted
            .contains("modeName: GAMEMODE_NAME\" \"GAMEMODE_VERSION")
    );
}
