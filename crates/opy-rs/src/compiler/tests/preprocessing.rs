//! Public compile-path coverage for preprocessor member defines.

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
fn multiline_function_define_preserves_statement_boundaries() {
    let hir = crate::compile(
        "#!define reset() A = null\\\nB = []\nglobalvar A\nglobalvar B\nrule \"multiline macro\":\n    @Event global\n    reset()\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect("multiline function-like defines must preserve statement boundaries");

    let RuleEntry::Rule(rule) = &hir.rules[0] else {
        panic!("expected a rule");
    };
    assert_eq!(rule.actions.len(), 2);
    assert!(matches!(rule.actions[0], Stmt::Assign { .. }));
    assert!(matches!(rule.actions[1], Stmt::Assign { .. }));
}

#[test]
fn multiline_object_define_preserves_statement_boundaries() {
    let hir = crate::compile(
        "#!define reset A = null\\\nB = []\nglobalvar A\nglobalvar B\nrule \"multiline object macro\":\n    @Event global\n    reset\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect("multiline object-like defines must preserve statement boundaries");

    let RuleEntry::Rule(rule) = &hir.rules[0] else {
        panic!("expected a rule");
    };
    assert_eq!(rule.actions.len(), 2);
    assert!(matches!(rule.actions[0], Stmt::Assign { .. }));
    assert!(matches!(rule.actions[1], Stmt::Assign { .. }));
}

#[test]
fn multiline_function_define_preserves_block_structure() {
    // #506: expanded tokens keep the expansion's relative layout for the
    // indentation-sensitive parser even though their authored provenance is
    // the use site — a collapsed layout would let `C = 2` escape the body.
    let hir = crate::compile(
        "#!define check() if A == 1:\\\n    B = 1\\\n    C = 2\nglobalvar A\nglobalvar B\nglobalvar C\nrule \"block macro\":\n    @Event global\n    check()\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect("multiline function-like defines must preserve block structure");

    let RuleEntry::Rule(rule) = &hir.rules[0] else {
        panic!("expected a rule");
    };
    assert_eq!(rule.actions.len(), 1);
    let Stmt::If { branches, .. } = &rule.actions[0] else {
        panic!("expected an if statement, got {:?}", rule.actions[0]);
    };
    assert_eq!(branches.len(), 1);
    assert_eq!(
        branches[0].body.len(),
        2,
        "both expansion statements must stay inside the if body"
    );
}

#[test]
fn multiline_function_define_preserves_for_block() {
    let hir = crate::compile(
        "#!define gen() for i in range(0, 3):\\\n    B = i\nglobalvar B\nglobalvar i\nrule \"for macro\":\n    @Event global\n    gen()\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect("multiline defines must keep expect_block_indent constructs intact");

    let RuleEntry::Rule(rule) = &hir.rules[0] else {
        panic!("expected a rule");
    };
    assert!(matches!(rule.actions[0], Stmt::For { .. }));
}

#[test]
fn expanded_f_string_interpolation_errors_stay_inside_the_use_site() {
    // #506: an interpolation inside an expanded string has no authored
    // extent; its derived positions clamp into the use-site span rather
    // than fabricating columns past the authored line.
    let error = crate::compile(
        "globalvar B\nrule \"bad f-string\":\n    @Event global\n    A = f\"hp: {B +}\"\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect_err("the malformed interpolation must fail");
    let span = error.span.expect("the error must carry a span");
    assert_eq!(span.start.line, 4);

    let error = crate::compile(
        "#!define M f\"hp: {B +}\"\nglobalvar A\nglobalvar B\nrule \"expanded bad f-string\":\n    @Event global\n    A = M\n",
        "main.opy",
        std::path::Path::new("."),
    )
    .expect_err("the malformed expanded interpolation must fail");
    let span = error.span.expect("the error must carry a span");
    // `M` is the use site at 6:9-6:10; the fabricated interpolation column
    // (string start + index) must clamp into it instead of escaping.
    assert_eq!(span.start.line, 6);
    assert!(
        (9..=10).contains(&span.start.col) && (9..=10).contains(&span.end.col),
        "the reported position must stay inside the authored use site, got {span:?}"
    );
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
fn entry_includes_continue_from_the_latest_resolved_file_base() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/project-preprocessing/latest-entry-base/src");
    let source = std::fs::read_to_string(dir.join("main.opy")).unwrap();
    let hir = crate::compile(&source, "main.opy", &dir)
        .expect("entry includes must preserve OverPy's resolved-file lookup base");

    assert!(hir.dump().contains("assign A = (6 + 2)"), "{}", hir.dump());
    assert_eq!(
        hir.files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        [
            "main.opy",
            "env/env.opy",
            "locales/en.opy",
            "composition/bootstrap.opy",
            "env/vars.opy",
            "composition/context.opy",
            "env/game.opy"
        ]
    );
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
