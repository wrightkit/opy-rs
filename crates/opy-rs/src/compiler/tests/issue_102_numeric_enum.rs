//! Oracle-backed coverage for numeric enum members in issue #102.

use std::path::{Path, PathBuf};

use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::Locale;
use workshop_rs::wir::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/real-world/overpy-parabola")
}

#[test]
fn numeric_team_member_reaches_the_canonical_catalog_identity() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("regressions/numeric-enum-member.opy"))
        .expect("minimized regression must be readable");
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_with_locale(
            &source,
            "numeric-enum-member.opy",
            &dir,
            &Locale::new("en-US"),
        )
        .expect("Team.2 must compile");

    assert!((0..artifact.wir.values.len()).any(|index| {
        matches!(
            artifact
                .wir
                .values
                .get(workshop_rs::wir::ValueId::from_index(index))
                .map(|node| &node.value),
            Some(Value::Enum { value_type, value })
                if value_type == "Team" && value == "TEAM_2"
        )
    }));
}

#[test]
fn invalid_numeric_and_named_team_members_keep_frontend_diagnostics() {
    let compiler = Compiler::new().expect("released Workshop contract must load");
    for (member, expected_message) in [
        ("0", "enum 'Team' has no member '0'"),
        ("foo", "enum 'Team' has no member 'foo'"),
    ] {
        let source =
            format!("rule \"invalid enum member\":\n    @Event global\n    debug(Team.{member})\n");
        let report = compiler.compile_source_report_with_locale(
            &source,
            "invalid-enum-member.opy",
            Path::new("."),
            &Locale::new("en-US"),
        );
        assert_eq!(report.compile.status, CompileStatus::Failure);
        assert_eq!(
            report.compile.failure_class,
            Some(CompileFailureClass::Frontend)
        );
        let diagnostic = report
            .compile
            .diagnostics
            .first()
            .expect("invalid member must report a diagnostic");
        assert_eq!(diagnostic.code, "unknown-enum-member");
        assert_eq!(diagnostic.message, expected_message);
        assert_eq!(
            diagnostic.span.as_ref().map(|span| span.start.line),
            Some(3)
        );
        assert_eq!(
            diagnostic.span.as_ref().map(|span| span.start.col),
            Some(11)
        );
    }
}
