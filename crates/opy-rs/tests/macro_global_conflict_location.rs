use std::path::Path;

use opy_rs::tooling::check;
use serde_json::Value;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/corpus/synthetic"
);

const FIXTURES: [&str; 5] = [
    "macro-default-global-invalid",
    "macro-after-global-invalid",
    "macro-default-global-disabled-optimizations-invalid",
    "macro-default-global-two-pragmas-invalid",
    "global-after-macro-invalid",
];

/// The pinned OverPy numbers the lines after a `#!disableOptimizations` or
/// `#!enableOptimizations` line one too high per such line, for every
/// diagnostic. A native diagnostic keeps the true source line, so the
/// reference line is the native line plus the exact pragma lines above it.
fn reference_line(source: &str, native_line: u32) -> u32 {
    let pragmas = source
        .lines()
        .take(native_line as usize - 1)
        .filter(|line| {
            matches!(
                line.trim_start(),
                "#!disableOptimizations" | "#!enableOptimizations"
            )
        })
        .count();
    native_line + pragmas as u32
}

fn reference_diagnostic(fixture: &Path) -> (String, u32, u32) {
    let snapshot: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.join("oracle.json")).unwrap())
            .unwrap();
    let text = snapshot["compile"]["diagnostics"][0]["text"]
        .as_str()
        .unwrap();
    let (message, location) = text
        .strip_prefix("Error: ")
        .and_then(|rest| rest.split_once("\n    | line "))
        .unwrap_or_else(|| panic!("unexpected reference diagnostic: {text}"));
    let (line, rest) = location.split_once(", col ").unwrap();
    let (col, _) = rest.split_once(',').unwrap();
    (
        message.to_string(),
        line.parse().unwrap(),
        col.parse().unwrap(),
    )
}

#[test]
fn macro_global_conflict_reports_the_reference_message_and_location() {
    for name in FIXTURES {
        let dir = Path::new(CORPUS).join(name);
        let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
        let (message, line, col) = reference_diagnostic(&dir);

        let checked = check(&source, "source.opy", &dir);
        let diagnostic = &checked.diagnostics[0];
        let start = diagnostic.span.as_ref().expect("conflict has a span").start;

        assert_eq!(diagnostic.message, message, "{name}: message");
        assert_eq!(
            (reference_line(&source, start.line), start.col),
            (line, col),
            "{name}: location"
        );
    }
}
