use crate::macro_js::error::MacroError;
use crate::macro_js::limits::Limits;
use crate::macro_js::runtime::MacroRuntime;

fn runtime() -> MacroRuntime {
    MacroRuntime::new(Limits::default())
}

#[test]
fn hook_receives_content_and_returns_transformed_text() {
    let rt = runtime();
    let result = rt.run_hook("content + \"!\";", "Hello", "hook.js").unwrap();
    assert_eq!(result.text, "Hello!");
}

#[test]
fn hook_content_is_json_escaped_before_injection() {
    let rt = runtime();
    let content = "say \"hi\" \\ now\nnext line";
    let result = rt.run_hook("content;", content, "hook.js").unwrap();
    assert_eq!(result.text, content);
}

#[test]
fn hook_line_separator_characters_are_escaped() {
    let rt = runtime();
    let content = "before\u{2028}after";
    let result = rt.run_hook("content;", content, "hook.js").unwrap();
    assert_eq!(result.text, content);
}

#[test]
fn hook_upstream_documentation_example() {
    let rt = runtime();
    let source = r#"content = content.replace(/abc/g, "def");
content.toString();"#;
    let result = rt.run_hook(source, "abc abc", "hook.js").unwrap();
    assert_eq!(result.text, "def def");
}

#[test]
fn hook_exception_carries_provenance() {
    let rt = runtime();
    let source = "if (content === \"boom\") {\n    throw new Error(\"hook failed\");\n}\ncontent;";
    let error = rt.run_hook(source, "boom", "hook.js").unwrap_err();
    match error {
        MacroError::Script(script) => {
            assert_eq!(script.message, "hook failed");
            assert_eq!(script.source_name.as_deref(), Some("hook.js"));
            assert_eq!(script.line, Some(2));
        }
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn hook_non_string_result_is_rejected() {
    let rt = runtime();
    let error = rt.run_hook("content.length;", "hello", "h.js").unwrap_err();
    match error {
        MacroError::InvalidResult { type_name } => assert_eq!(type_name, "number"),
        other => panic!("expected InvalidResult, got {other:?}"),
    }
}

#[test]
fn hook_error_lines_ignore_the_content_prologue() {
    let rt = runtime();
    let source = "// first line\nthrow new Error(\"x\");";
    let error = rt.run_hook(source, "any", "lines.js").unwrap_err();
    match error {
        MacroError::Script(script) => {
            assert_eq!(script.line, Some(2));
            let stack = script.stack.as_deref().expect("stack should be present");
            assert!(stack.contains("lines.js:2:"), "stack: {stack}");
        }
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn hook_console_output_is_captured() {
    let rt = runtime();
    let source = r#"console.log("before:", content); content.toUpperCase();"#;
    let result = rt.run_hook(source, "mix", "log-hook.js").unwrap();
    assert_eq!(result.text, "MIX");
    assert_eq!(result.console_output, vec!["before: mix"]);
}
