use std::path::{Path, PathBuf};

use opy_rs::compile;
use opy_rs::compile_with_overlay_outcome;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/macros")
        .to_path_buf()
}

fn compile_fixture(name: &str) -> Result<opy_rs::hir::Program, opy_rs::OpyError> {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join(name)).unwrap();
    compile(&source, name, &dir)
}

fn outcome(name: &str) -> opy_rs::CompileOutcome {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join(name)).unwrap();
    compile_with_overlay_outcome(&source, name, &dir, &Default::default())
}

#[test]
fn script_macro_expands_through_the_full_pipeline() {
    let program = compile_fixture("addfive.opy").unwrap();
    let dump = program.dump();
    assert!(dump.contains("rule \"add five\""), "dump: {dump}");
    assert!(
        dump.contains("assign A = 8"),
        "the script completion value must substitute as an expression: {dump}"
    );
}

#[test]
fn script_macro_string_argument_is_injected_as_a_js_value() {
    let program = compile_fixture("shout.opy").unwrap();
    let dump = program.dump();
    assert!(
        dump.contains("assign S = \"HELLO\""),
        "string argument must round-trip through the runtime: {dump}"
    );
}

#[test]
fn script_macro_helpers_surface_is_available() {
    let program = compile_fixture("vect-macro.opy").unwrap();
    let dump = program.dump();
    assert!(
        dump.contains("assign V = vect(1, 2, 3)"),
        "vect helper must reach the HIR as a vector: {dump}"
    );
}

#[test]
fn script_macro_multiline_output_preserves_block_structure() {
    let program = compile_fixture("blockgen.opy").unwrap();
    let opy_rs::hir::RuleEntry::Rule(rule) = &program.rules[0] else {
        panic!("expected a rule");
    };
    assert_eq!(rule.actions.len(), 1);
    let opy_rs::hir::Stmt::If { branches, .. } = &rule.actions[0] else {
        panic!("expected an if statement, got {:?}", rule.actions[0]);
    };
    assert_eq!(branches[0].body.len(), 2);
}

#[test]
fn missing_script_file_is_a_structured_diagnostic() {
    let error = compile(
        "#!define missing(x) __script__(\"nope.js\")\nrule \"r\":\n    A = missing(1)\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "script-not-found");
    assert!(error.message.contains("nope.js"));
    assert!(error.span.is_some());
}

#[test]
fn malformed_script_macro_is_a_structured_diagnostic() {
    let error = compile(
        "#!define bad(x) __script__(\"addfive.js\" + extra)\nrule \"r\":\n    A = bad(1)\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "script-invalid");
}

#[test]
fn macro_throw_maps_to_script_error_with_provenance() {
    let error = compile(
        "#!define boom(x) __script__(\"boom.js\")\nrule \"r\":\n    A = boom(1)\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "script-error");
    assert!(error.message.contains("kaboom"), "{}", error.message);
    assert!(error.message.contains("boom.js"), "{}", error.message);
    assert_eq!(error.span.unwrap().start.line, 3);
}

#[test]
fn runaway_script_hits_the_time_budget() {
    let error = compile(
        "#!define hang(x) __script__(\"runaway.js\")\nrule \"r\":\n    A = hang(1)\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "script-timeout");
    assert!(error.message.contains("runaway.js"));
}

#[test]
fn non_string_result_is_rejected_with_the_reference_wording() {
    let error = compile(
        "#!define bad(x) __script__(\"notstring.js\")\nrule \"r\":\n    A = bad(1)\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "script-result-not-string");
    assert!(
        error
            .message
            .contains("expected string. Try using .toString()"),
        "{}",
        error.message
    );
}

#[test]
fn post_compile_hook_is_parsed_validated_and_recorded_without_execution() {
    let outcome = outcome("hook.opy");
    let record = outcome
        .post_compile_hook
        .expect("the fixture declares a postCompileHook");
    assert_eq!(record.script, "hook.js");
    let span = record.span.expect("the directive span is recorded");
    assert_eq!(span.start.line, 1, "span points at the directive");
    let hir = outcome.hir.expect("recorded hooks keep the program");
    assert!(hir.dump().contains("rule \"setup\""));
}

#[test]
fn a_throwing_hook_script_is_recorded_but_never_executed() {
    let outcome = compile_with_overlay_outcome(
        "#!postCompileHook \"hook-boom.js\"\nrule \"r\":\n    pass\n",
        "main.opy",
        &fixture_dir(),
        &Default::default(),
    );
    let record = outcome
        .post_compile_hook
        .expect("the directive is recorded even though the hook would throw");
    assert_eq!(record.script, "hook-boom.js");
    outcome
        .hir
        .expect("a recorded, never-executed hook cannot fail the compile");
}

#[test]
fn duplicate_post_compile_hook_is_rejected() {
    let error = compile(
        "#!postCompileHook \"hook.js\"\n#!postCompileHook \"hook.js\"\nrule \"r\":\n    pass\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "post-compile-hook-duplicate");
}

#[test]
fn script_macro_arity_is_validated_like_other_macros() {
    let error = compile(
        "#!define addFive(x) __script__(\"addfive.js\")\nrule \"r\":\n    A = addFive(1, 2)\n",
        "main.opy",
        &fixture_dir(),
    )
    .unwrap_err();
    assert_eq!(error.code, "macro-arity");
}

#[test]
fn compile_time_scripts_are_listed_in_the_files_table() {
    let hook = outcome("hook.opy");
    assert!(
        hook.files.iter().any(|file| file.path == "hook.js"),
        "{:?}",
        hook.files
    );

    let source = "#!define twice(x) __script__(\"notstring.js\")\n#!define again(x) __script__(\"notstring.js\")\nrule \"r\":\n    pass\n";
    let macro_files =
        compile_with_overlay_outcome(source, "main.opy", &fixture_dir(), &Default::default()).files;
    let scripts: Vec<_> = macro_files
        .iter()
        .filter(|file| file.path == "notstring.js")
        .collect();
    assert_eq!(scripts.len(), 1, "{macro_files:?}");
}
