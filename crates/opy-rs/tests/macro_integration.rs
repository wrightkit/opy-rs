//! Frontend integration of the JavaScript macro runtime.
//!
//! OverPy-compatible `__script__("…")` macros run at compile time through
//! `opy_macro_js::MacroRuntime`; `#!postCompileHook` scripts are parsed,
//! validated, and recorded only (never executed by the frontend). The
//! reference ABI (OverPy 9.7.10, `src/compiler/tokenizer.ts`,
//! `src/quickjs.ts`, `src/globalVars.ts`) is:
//!
//! * `#!define name(args) __script__("path.js")` — the script path resolves
//!   root-relative at the define site; each expansion injects the call-site
//!   arguments as `var <name>=<raw>;` and evaluates the script; the string
//!   completion value becomes the expanded text;
//! * `#!postCompileHook "hook.js"` — the directive is recognized, parsed,
//!   validated, and recorded (script path + span on the compile outcome);
//!   the frontend never executes the hook (real execution runs against the
//!   final Workshop text and is lowering-dependent, issue #8); a second hook
//!   declaration is rejected;
//! * resource limits mirror the reference constants
//!   (`opy_macro_js::Limits::default()`: 1000 ms macro budget, 64 MiB memory,
//!   512 KiB stack).
//!
//! Fixtures live in `tests/fixtures/macros/`. Workshop-emission wiring (hook
//! execution against emitted Workshop text, catalog constant population)
//! stays lowering-dependent; the runtime's hook ABI on synthetic content is
//! tested in the internal macro-runtime unit tests.

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
    // Mirrors the upstream doc example (`src/data/opy/preprocessing.ts`):
    // `#!define addFive(x) __script__("addfive.js")` where the script
    // contains `x+5`; `addFive(3)` must expand to `8`.
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
    // The raw call-site argument text is injected as `var x="hello";`; the
    // script uppercases the value and returns a string literal, which must
    // lex back into the token stream as a string value.
    let program = compile_fixture("shout.opy").unwrap();
    let dump = program.dump();
    assert!(
        dump.contains("assign S = \"HELLO\""),
        "string argument must round-trip through the runtime: {dump}"
    );
}

#[test]
fn script_macro_helpers_surface_is_available() {
    // The runtime always defines the reference's `vect` helper
    // (`builtInJsFunctions` in `src/globalVars.ts`); its `toString()`
    // produces `vect(1,2,3)`, which lowers to a HIR vector.
    let program = compile_fixture("vect-macro.opy").unwrap();
    let dump = program.dump();
    assert!(
        dump.contains("assign V = vect(1, 2, 3)"),
        "vect helper must reach the HIR as a vector: {dump}"
    );
}

#[test]
fn missing_script_file_is_a_structured_diagnostic() {
    // The reference resolves the script path at the define site and fails
    // with ENOENT; the native frontend rejects with `script-not-found` at the
    // directive span.
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
    // The 1000 ms macro budget aborts the script with QuickJS's
    // "interrupted"; the frontend classifies it as `script-timeout`.
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
    // The frontend recognizes, parses, validates, and records
    // `#!postCompileHook`; it never executes the hook. The recorded record is
    // the declaration only: script path + directive span (execution against
    // the final Workshop text is lowering-dependent, issue #8).
    let outcome = outcome("hook.opy");
    let record = outcome
        .post_compile_hook
        .expect("the fixture declares a postCompileHook");
    assert_eq!(record.script, "hook.js");
    let span = record.span.expect("the directive span is recorded");
    assert_eq!(span.start.line, 1, "span points at the directive");
    // A successful compile keeps the HIR; nothing was executed or
    // transformed (there is no payload: the frontend never fabricates one).
    let hir = outcome.hir.expect("recorded hooks keep the program");
    assert!(hir.dump().contains("rule \"setup\""));
}

#[test]
fn a_throwing_hook_script_is_recorded_but_never_executed() {
    // The hook script throws ("hook failed"); because the frontend never
    // executes hooks, the declaration alone cannot fail the compile. Real
    // hook execution receives the final Workshop text (lowering-dependent).
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
