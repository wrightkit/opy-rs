//! Public compile-path regression coverage for issue #129.

use std::collections::BTreeMap;
use std::path::Path;

use crate::{compile_with_overlay, compile_with_overlay_outcome};

fn child_overlay(source: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(String::from("child.opy"), source.to_string())])
}

#[test]
fn included_main_file_is_a_scoped_directive_with_source_provenance() {
    let hir = compile_with_overlay(
        "#!include \"child.opy\"\nrule \"root\":\n    @Event global\n    pass\n",
        "main.opy",
        Path::new("."),
        &child_overlay(
            "#!mainFile \"../main.opy\"\nrule \"included\":\n    @Event global\n    pass\n",
        ),
    )
    .expect("an included mainFile directive must not redirect the root project");

    assert!(hir.preprocessing.main_file.is_none());
    assert_eq!(hir.rules.len(), 2);
    assert_eq!(hir.files[1].path, "child.opy");

    let directive = hir
        .preprocessing
        .directives
        .iter()
        .find(|directive| directive.name == "mainFile")
        .expect("the included mainFile directive must remain inspectable");
    assert_eq!(directive.value.as_deref(), Some("../main.opy"));
    assert_eq!(directive.scope_depth, 1);
    let span = directive.span.expect("directive provenance");
    assert_eq!(span.file, 1);
    assert_eq!(span.start.line, 1);
    assert_eq!(span.start.col, 1);
}

#[test]
fn malformed_included_main_file_is_source_attributed() {
    let outcome = compile_with_overlay_outcome(
        "#!include \"child.opy\"\n",
        "main.opy",
        Path::new("."),
        &child_overlay("#!mainFile ../main.opy\n"),
    );
    let error = outcome.error.expect("malformed mainFile must fail");
    assert_eq!(error.code, "main-file-invalid");
    let span = error.span.expect("diagnostic provenance");
    assert_eq!(span.file, 1);
    assert_eq!(span.start.line, 1);
    assert_eq!(outcome.files[1].path, "child.opy");
}
