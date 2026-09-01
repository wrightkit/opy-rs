//! Public compile-path coverage for issue #142's preprocessor member defines.

use std::collections::BTreeMap;

use crate::compile_with_overlay;
use crate::hir::{Expr, RuleEntry, Stmt};

#[test]
fn included_define_member_expands_with_definition_provenance() {
    let mut overlay = BTreeMap::new();
    overlay.insert(
        "shared.opy".to_string(),
        "#!defineMember VALUE 2\n".to_string(),
    );
    let hir = compile_with_overlay(
        "#!include \"shared.opy\"\nrule \"member define\":\n    @Event global\n    A = VALUE\n",
        "main.opy",
        std::path::Path::new("."),
        &overlay,
    )
    .expect("included member defines must expand through the public compile path");

    assert!(hir.dump().contains("assign A = 2"), "{}", hir.dump());
    assert_eq!(hir.defines.len(), 1);
    assert_eq!(hir.defines[0].name, "VALUE");
    assert!(hir.defines[0].is_member);
    assert_eq!(hir.defines[0].span.expect("define span").file, 1);
    assert_eq!(hir.files[1].path, "shared.opy");
}

#[test]
fn function_define_member_uses_the_same_textual_macro_contract() {
    let hir = crate::compile(
        "#!defineMember add(value) value + 1\nrule \"member function\":\n    @Event global\n    A = add(2)\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect("function-like member defines must expand");
    assert!(hir.defines[0].is_member);

    let RuleEntry::Rule(rule) = &hir.rules[0] else {
        panic!("expected a rule");
    };
    let Stmt::Assign { value, .. } = &rule.actions[0] else {
        panic!("expected an assignment");
    };
    let Expr::Binary { left, right, .. } = &**value else {
        panic!("expected the expanded expression to preserve its operands");
    };
    assert!(matches!(&**left, Expr::Number { value, .. } if *value == 2.0));
    assert!(matches!(&**right, Expr::Number { value, .. } if *value == 1.0));
}

#[test]
fn nested_includes_resolve_relative_to_the_including_file() {
    let overlay = BTreeMap::from([
        (
            "dir/child.opy".to_string(),
            "#!include \"grandchild.opy\"\n".to_string(),
        ),
        (
            "dir/grandchild.opy".to_string(),
            "#!defineMember VALUE 2\n".to_string(),
        ),
    ]);
    let hir = compile_with_overlay(
        "#!include \"dir/child.opy\"\nrule \"nested include\":\n    @Event global\n    A = VALUE\n",
        "main.opy",
        std::path::Path::new("."),
        &overlay,
    )
    .expect("nested include paths must be relative to the including file");

    assert!(hir.dump().contains("assign A = 2"), "{}", hir.dump());
    assert_eq!(hir.defines[0].span.expect("define span").file, 2);
    assert_eq!(hir.files[1].path, "dir/child.opy");
    assert_eq!(hir.files[2].path, "dir/grandchild.opy");
}

#[test]
fn included_settings_are_extracted_with_file_provenance() {
    let overlay = BTreeMap::from([(
        "shared.opy".to_string(),
        "settings {\n    \"gamemodes\": {}\n}\n".to_string(),
    )]);
    let hir = compile_with_overlay(
        "#!include \"shared.opy\"\nrule \"included settings\":\n    @Event global\n    pass\n",
        "main.opy",
        std::path::Path::new("."),
        &overlay,
    )
    .expect("included settings must compile through the public path");

    let settings = hir.settings.expect("included settings");
    assert_eq!(settings.span.expect("settings span").file, 1);
    assert_eq!(hir.files[1].path, "shared.opy");
}

#[test]
fn duplicate_include_warning_is_exposed_by_frontend_tooling() {
    let overlay = BTreeMap::from([("shared.opy".to_string(), "#!define VALUE 2\n".to_string())]);
    let outcome = crate::tooling::check_with_overlay(
        "#!include \"shared.opy\"\n#!include \"shared.opy\"\nrule \"duplicate include\":\n    @Event global\n    A = VALUE\n",
        "main.opy",
        std::path::Path::new("."),
        &overlay,
    );

    assert!(outcome.is_clean());
    let warning = outcome
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "w_already_imported")
        .expect("duplicate include warning");
    assert_eq!(
        warning.severity,
        crate::tooling::DiagnosticSeverity::Warning
    );
    assert_eq!(
        warning.span.as_ref().expect("warning span").path,
        "main.opy"
    );
}
