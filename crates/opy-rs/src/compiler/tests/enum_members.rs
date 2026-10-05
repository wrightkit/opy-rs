//! Oracle-backed coverage for numeric enum members.

use std::path::{Path, PathBuf};

use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::Locale;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/real-world/overpy-parabola")
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

    assert!(
        super::canonical_program(&artifact)
            .dump()
            .contains("TEAM_2")
    );
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
        // The message carries the member candidates of the Team domain
        // (issue #469).
        assert!(
            diagnostic
                .message
                .starts_with(&format!("{expected_message} (did you mean '")),
            "message: {}",
            diagnostic.message
        );
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

fn member_source(member_expr: &str) -> String {
    format!("rule \"enum spelling\":\n    @Event global\n    debug({member_expr})\n")
}

fn compile_member(member_expr: &str) -> crate::CompileReport {
    Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_report_with_locale(
            &member_source(member_expr),
            "enum-spelling.opy",
            Path::new("."),
            &Locale::new("en-US"),
        )
}

#[test]
fn catalog_only_spellings_name_the_valid_opy_spelling() {
    // wrightkit/opy-rs#466: the Workshop catalog id is not an OverPy
    // spelling; the diagnostic names the spelling the pinned reference
    // accepts.
    for (member_expr, expected_message) in [
        (
            "Hero.SOLDIER_76",
            "enum 'Hero' has no member 'SOLDIER_76'; the OverPy spelling is 'Hero.SOLDIER'",
        ),
        (
            "Hero.JINYU",
            "enum 'Hero' has no member 'JINYU'; the OverPy spelling is 'Hero.DOMINA'",
        ),
        (
            "Map.ROUTE_66",
            "enum 'Map' has no member 'ROUTE_66'; the OverPy spelling is 'Map.ROUTE66'",
        ),
        (
            "Gamemode.BOUNTYHUNTER",
            "enum 'Gamemode' has no member 'BOUNTYHUNTER'; the OverPy spelling is \
             'Gamemode.BOUNTY_HUNTER'",
        ),
        (
            "Team.TEAM_1",
            "enum 'Team' has no member 'TEAM_1'; the OverPy spelling is 'Team.1'",
        ),
        (
            "Clip.DO_NOT_CLIP",
            "enum 'Clip' has no member 'DO_NOT_CLIP'; the OverPy spelling is 'Clip.NONE'",
        ),
    ] {
        let report = compile_member(member_expr);
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
    }
}

#[test]
fn catalog_member_without_opy_spelling_reports_unspellable() {
    let report = compile_member("Map.LIJIANG_TOWER_LUNAR");
    assert_eq!(report.compile.status, CompileStatus::Failure);
    let diagnostic = report
        .compile
        .diagnostics
        .first()
        .expect("invalid member must report a diagnostic");
    assert_eq!(diagnostic.code, "unknown-enum-member");
    assert_eq!(
        diagnostic.message,
        "enum 'Map' has no member 'LIJIANG_TOWER_LUNAR'; the canonical member has \
         no OverPy spelling"
    );
}

#[test]
fn overpy_spellings_lower_to_canonical_catalog_identities() {
    let source = r#"globalvar value

rule "overpy spellings":
    @Event global
    value = Hero.SOLDIER
    value = Hero.DOMINA
    value = Hero.DMON
    value = Hero.MCCREE
    value = Hero.HAMMOND
    value = Map.ROUTE66
    value = Gamemode.BOUNTY_HUNTER
    value = Team.1
    value = Clip.NONE
    value = HudPosition.ACTUALLY_LEFT
    value = SpecVisibility.ALWAYS
"#;
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_artifact(source, "overpy-spellings.opy", Path::new("."))
        .expect("OverPy spellings must compile");
    let emitted = &artifact.emitted;
    for expected in [
        "Soldier: 76",
        "Domina",
        "D.Mon",
        "Cassidy",
        "Wrecking Ball",
        "Route 66",
        "Bounty Hunter",
        "Team 1",
        "Do Not Clip",
        "Left",
        "Visible Always",
    ] {
        assert!(emitted.contains(expected), "missing {expected}: {emitted}");
    }
}

#[test]
fn event_filter_annotations_follow_the_opy_spelling_surface() {
    let compiler = Compiler::new().expect("released Workshop contract must load");
    for source in [
        "rule \"hero kw\":\n    @Event eachPlayer\n    @Hero soldier\n    disableInspector()\n",
        "rule \"hero dotted\":\n    @Event eachPlayer\n    @Hero Hero.SOLDIER\n    disableInspector()\n",
        "rule \"hero legacy\":\n    @Event eachPlayer\n    @Hero mccree\n    disableInspector()\n",
        "rule \"team dotted\":\n    @Event playerJoined\n    @Team Team.2\n    disableInspector()\n",
        "rule \"slot\":\n    @Event eachPlayer\n    @Slot 7\n    disableInspector()\n",
    ] {
        compiler
            .compile_source(source, "filters.opy", Path::new("."))
            .unwrap_or_else(|error| panic!("{source:?} must compile: {error}"));
    }
    for (source, fragment) in [
        (
            "rule \"hero catalog\":\n    @Event eachPlayer\n    @Hero SOLDIER_76\n    disableInspector()\n",
            "unknown event player filter 'SOLDIER_76'; the OverPy spelling is 'soldier'",
        ),
        (
            "rule \"hero catalog dotted\":\n    @Event eachPlayer\n    @Hero Hero.SOLDIER_76\n    disableInspector()\n",
            "unknown event player filter 'Hero.SOLDIER_76'; the OverPy spelling is 'soldier'",
        ),
        (
            "rule \"slot mccree\":\n    @Event eachPlayer\n    @Slot mccree\n    disableInspector()\n",
            "unknown event player filter 'mccree'",
        ),
        (
            "rule \"team catalog\":\n    @Event playerJoined\n    @Team TEAM_1\n    disableInspector()\n",
            "unknown event team filter 'TEAM_1'",
        ),
    ] {
        let report = compiler.compile_source_report_with_locale(
            source,
            "filters.opy",
            Path::new("."),
            &Locale::new("en-US"),
        );
        assert_eq!(report.compile.status, CompileStatus::Failure);
        assert!(
            report
                .compile
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(fragment)),
            "expected diagnostic containing {fragment:?}, got {:?}",
            report.compile.diagnostics
        );
    }
}

#[test]
fn literal_domains_use_the_same_opy_spellings() {
    let source = r#"globalvar value

rule "literal domains":
    @Event global
    value = HeroLiteral.SOLDIER
    value = MapLiteral.ROUTE66
    value = GamemodeLiteral.BOUNTY_HUNTER
    value = TeamLiteral.1
    value = ButtonLiteral.INTERACT
    value = ColorLiteral.WHITE
"#;
    let artifact = Compiler::new()
        .expect("released Workshop contract must load")
        .compile_source_artifact(source, "literal-domains.opy", Path::new("."))
        .expect("literal enum spellings must compile");
    let emitted = &artifact.emitted;
    for expected in [
        "Soldier: 76",
        "Route 66",
        "Bounty Hunter",
        "Team 1",
        "Interact",
        "White",
    ] {
        assert!(emitted.contains(expected), "missing {expected}: {emitted}");
    }
}
