use std::path::{Path, PathBuf};

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

fn assert_rejected_postfix_fixture(name: &str, line: u32, col: u32) {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let error = crate::compile(&source, "source.opy", &dir)
        .expect_err("rejected postfix form must remain rejected");

    assert_eq!(error.code, "parse-error");
    let span = error.span.expect("diagnostic must be source-attributed");
    assert_eq!(span.start.line, line);
    assert_eq!(span.start.col, col);
}

#[test]
fn rejected_prefix_increment_form_has_stable_source_diagnostic() {
    assert_rejected_postfix_fixture("postfix-prefix-invalid", 6, 5);
}

#[test]
fn rejected_embedded_postfix_form_has_stable_source_diagnostic() {
    assert_rejected_postfix_fixture("postfix-embedded-invalid", 6, 20);
}
