//! Macro ABI fixtures: execution, argument injection, results, helpers, and
//! error provenance.
//!
//! Expected behavior mirrors the pinned OverPy reference (v9.7.10):
//! `src/compiler/tokenizer.ts` (argument injection, `resolveMacro`),
//! `src/quickjs.ts` (result type check, console install), `src/globalVars.ts`
//! (`vect`, constant objects), `src/tests/quickjs-*.js` (test fixtures).

use crate::macro_js::error::MacroError;
use crate::macro_js::helpers::Helpers;
use crate::macro_js::limits::Limits;
use crate::macro_js::runtime::{MacroArg, MacroRuntime};

fn runtime() -> MacroRuntime {
    MacroRuntime::new(Limits::default())
}

#[test]
fn successful_execution_returns_string_completion_value() {
    let rt = runtime();
    let result = rt
        .run_macro(r#""hello world";"#, &[], "success.js")
        .unwrap();
    assert_eq!(result.text, "hello world");
    assert!(result.console_output.is_empty());
}

#[test]
fn modern_syntax_fixture_matches_upstream_quickjs_modern() {
    // Mirrors the reference fixture src/tests/quickjs-modern.js: modern syntax
    // (optional chaining, nullish coalescing) must run without transpilation,
    // and `modern(4)` must expand to a string "4".
    let rt = runtime();
    let source = r#"const wrapper = {
    result: value?.toString?.() ?? "0",
};
wrapper.result;"#;
    let result = rt
        .run_macro(source, &[MacroArg::new("value", "4")], "quickjs-modern.js")
        .unwrap();
    assert_eq!(result.text, "4");
}

#[test]
fn multiple_arguments_are_injected_as_var_declarations() {
    // Upstream injects `var x=40;var y=2;` ahead of the script text.
    let rt = runtime();
    let result = rt
        .run_macro(
            "(x + y).toString();",
            &[MacroArg::new("x", "40"), MacroArg::new("y", "2")],
            "add.js",
        )
        .unwrap();
    assert_eq!(result.text, "42");
}

#[test]
fn raw_argument_text_is_injected_verbatim() {
    // The call-site argument text is inserted as-is (tokenizer.ts):
    // `addFive("a,b")` injects `var x="a,b";`.
    let rt = runtime();
    let result = rt
        .run_macro(
            "x.toUpperCase();",
            &[MacroArg::new("x", r#""a,b""#)],
            "raw.js",
        )
        .unwrap();
    assert_eq!(result.text, "A,B");
}

#[test]
fn macro_without_arguments_runs() {
    let rt = runtime();
    let result = rt.run_macro(r#""no args";"#, &[], "noargs.js").unwrap();
    assert_eq!(result.text, "no args");
}

#[test]
fn thrown_exception_maps_to_structured_error_with_provenance() {
    let rt = runtime();
    let source = "function boom() {\n    throw new Error(\"kaboom\");\n}\nboom();";
    let error = rt.run_macro(source, &[], "macro/gen.js").unwrap_err();
    match error {
        MacroError::Script(script) => {
            assert_eq!(script.message, "kaboom");
            assert_eq!(script.source_name.as_deref(), Some("macro/gen.js"));
            // The throw is on line 2 of the user's script; the injected
            // prologue must not shift the reported line.
            assert_eq!(script.line, Some(2));
            assert!(script.column.is_some_and(|column| column >= 1));
            let stack = script.stack.as_deref().expect("stack should be present");
            assert!(stack.contains("macro/gen.js:2:"), "stack: {stack}");
            assert!(stack.contains("macro/gen.js:4:"), "stack: {stack}");
        }
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn thrown_non_error_value_keeps_its_message() {
    // `throw 42` has no Error shape; the reference falls back to the
    // stringified thrown value.
    let rt = runtime();
    let error = rt
        .run_macro("throw 42;", &[], "throw-number.js")
        .unwrap_err();
    match error {
        MacroError::Script(script) => {
            assert_eq!(script.message, "42");
            assert_eq!(script.source_name.as_deref(), Some("throw-number.js"));
        }
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn syntax_error_carries_source_name_and_line() {
    let rt = runtime();
    let error = rt.run_macro("function {", &[], "bad.js").unwrap_err();
    match error {
        MacroError::Script(script) => {
            assert!(!script.message.is_empty(), "message must not be empty");
            assert_eq!(script.source_name.as_deref(), Some("bad.js"));
            assert_eq!(script.line, Some(1));
        }
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn object_result_is_rejected_like_upstream() {
    // Mirrors the reference fixture src/tests/quickjs-invalid-return.js: the
    // script returns an object and compilation fails with "expected string".
    let rt = runtime();
    let source = "const invalid = { invalid: true };\ninvalid;";
    let error = rt
        .run_macro(source, &[], "quickjs-invalid-return.js")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "JavaScript macro returned value with type of object, expected string. Try using .toString()"
    );
    match error {
        MacroError::InvalidResult { type_name } => assert_eq!(type_name, "object"),
        other => panic!("expected InvalidResult, got {other:?}"),
    }
}

#[test]
fn number_result_is_rejected() {
    // Upstream README's `addfive.js` (`x+5`, no `return`) yields a number and
    // is rejected by the implementation exactly like this.
    let rt = runtime();
    let error = rt
        .run_macro("x + 5;", &[MacroArg::new("x", "123")], "addfive.js")
        .unwrap_err();
    match error {
        MacroError::InvalidResult { type_name } => assert_eq!(type_name, "number"),
        other => panic!("expected InvalidResult, got {other:?}"),
    }
}

#[test]
fn boolean_result_is_rejected() {
    let rt = runtime();
    let error = rt.run_macro("true;", &[], "bool.js").unwrap_err();
    match error {
        MacroError::InvalidResult { type_name } => assert_eq!(type_name, "boolean"),
        other => panic!("expected InvalidResult, got {other:?}"),
    }
}

#[test]
fn undefined_result_is_rejected() {
    let rt = runtime();
    let error = rt.run_macro("var x = 1;", &[], "undef.js").unwrap_err();
    match error {
        MacroError::InvalidResult { type_name } => assert_eq!(type_name, "undefined"),
        other => panic!("expected InvalidResult, got {other:?}"),
    }
}

#[test]
fn vect_helper_matches_upstream_abi() {
    // `vect` returns `{x, y, z}` with the documented `toString()` shape
    // (globalVars.ts, README "Javascript macros").
    let rt = runtime();
    let result = rt
        .run_macro("vect(1, 2, 3).toString();", &[], "vect.js")
        .unwrap();
    assert_eq!(result.text, "vect(1,2,3)");

    let result = rt
        .run_macro(
            "(vect(1, 2, 3).x + vect(1, 2, 3).y + vect(1, 2, 3).z).toString();",
            &[],
            "vect.js",
        )
        .unwrap();
    assert_eq!(result.text, "6");
}

#[test]
fn constant_objects_are_defined_and_populated_from_helpers() {
    // Upstream ABI: `Map.KANEZAKA` evaluates to `"Map.KANEZAKA"` (README).
    let mut rt = runtime();
    let mut helpers = Helpers::new();
    helpers.set_constant("Map", "KANEZAKA", "Map.KANEZAKA");
    rt.set_helpers(helpers);
    let result = rt.run_macro("Map.KANEZAKA;", &[], "map.js").unwrap();
    assert_eq!(result.text, "Map.KANEZAKA");
}

#[test]
fn all_six_constant_objects_exist_empty_by_default() {
    let rt = runtime();
    let source = r#"["Map", "Hero", "Gamemode", "Color", "Team", "Button"]
    .map((n) => n + "=" + typeof globalThis[n])
    .join(",");"#;
    let result = rt.run_macro(source, &[], "constants.js").unwrap();
    assert_eq!(
        result.text,
        "Map=object,Hero=object,Gamemode=object,Color=object,Team=object,Button=object"
    );
}

#[test]
fn console_log_is_captured_instead_of_reaching_the_host() {
    let rt = runtime();
    let result = rt
        .run_macro(r#"console.log("a", 1); "done";"#, &[], "log.js")
        .unwrap();
    assert_eq!(result.text, "done");
    assert_eq!(result.console_output, vec!["a 1"]);
}

#[test]
fn console_log_hostile_to_string_falls_back() {
    // The reference wraps argument rendering in try/catch and falls back to
    // "[unserializable]" (quickjs.ts, createConsoleLog).
    let rt = runtime();
    let source = r#"console.log({ toString() { throw new Error("nope"); } }); "ok";"#;
    let result = rt.run_macro(source, &[], "log-hostile.js").unwrap();
    assert_eq!(result.console_output, vec!["[unserializable]"]);
}

#[test]
fn engine_intrinsics_are_available() {
    let rt = runtime();
    let result = rt
        .run_macro(
            "Math.sqrt(16).toString() + JSON.stringify({ a: 1 });",
            &[],
            "intrinsics.js",
        )
        .unwrap();
    assert_eq!(result.text, r#"4{"a":1}"#);
}

#[test]
fn invocations_do_not_share_state() {
    // Each invocation runs on a fresh engine: globals set by one macro must
    // not leak into the next, even when reusing the same runtime instance.
    let rt = runtime();
    rt.run_macro(r#"globalThis.leak = "secret"; "ok";"#, &[], "a.js")
        .unwrap();
    let result = rt
        .run_macro("typeof globalThis.leak;", &[], "b.js")
        .unwrap();
    assert_eq!(result.text, "undefined");
}
