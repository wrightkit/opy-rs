use std::path::Path;

#[test]
fn invalid_spec_visibility_members_keep_source_attributed_diagnostics() {
    let source = "rule \"r\":\n    @Event global\n    hudSubheader(getAllPlayers(), \"text\", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.INVALID)\n";
    let error =
        crate::compile(source, "source.opy", Path::new(".")).expect_err("invalid member must fail");
    assert_eq!(error.code, "unknown-enum-member");
    assert!(
        error
            .message
            .starts_with("enum 'SpecVisibility' has no member 'INVALID' (did you mean '"),
        "message: {}",
        error.message
    );
    let span = error.span.expect("diagnostic provenance");
    assert_eq!(span.start.line, 3);
    assert_eq!(span.start.col, 98);
}
