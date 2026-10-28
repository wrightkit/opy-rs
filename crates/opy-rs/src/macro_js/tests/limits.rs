use std::time::{Duration, Instant};

use crate::macro_js::error::MacroError;
use crate::macro_js::limits::Limits;
use crate::macro_js::runtime::MacroRuntime;

#[test]
fn busy_loop_is_interrupted_within_budget() {
    let limits = Limits {
        macro_time_budget: Duration::from_millis(50),
        ..Limits::default()
    };
    let rt = MacroRuntime::new(limits);

    let start = Instant::now();
    let error = rt
        .run_macro("while (true) {}", &[], "runaway.js")
        .unwrap_err();
    let elapsed = start.elapsed();

    match error {
        MacroError::Script(script) => {
            assert_eq!(script.message, "interrupted");
            assert_eq!(script.source_name.as_deref(), Some("runaway.js"));
        }
        other => panic!("expected Script error, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "interruption took too long: {elapsed:?}"
    );
}

#[test]
fn hook_busy_loop_is_interrupted_within_budget() {
    let limits = Limits {
        hook_time_budget: Duration::from_millis(50),
        ..Limits::default()
    };
    let rt = MacroRuntime::new(limits);

    let start = Instant::now();
    let error = rt
        .run_hook("while (true) {}", "content", "runaway-hook.js")
        .unwrap_err();
    let elapsed = start.elapsed();

    match error {
        MacroError::Script(script) => assert_eq!(script.message, "interrupted"),
        other => panic!("expected Script error, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "interruption took too long: {elapsed:?}"
    );
}

#[test]
fn memory_limit_aborts_allocation_loop() {
    let limits = Limits {
        memory_limit_bytes: 2 * 1024 * 1024,
        ..Limits::default()
    };
    let rt = MacroRuntime::new(limits);

    let error = rt
        .run_macro(
            r#"var a = []; for (var i = 0; i < 1000000; i++) { a.push("x".repeat(1024)); }"#,
            &[],
            "mem.js",
        )
        .unwrap_err();
    match error {
        MacroError::Script(script) => assert_eq!(script.message, "out of memory"),
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn stack_limit_aborts_deep_recursion() {
    let limits = Limits {
        max_stack_bytes: 128 * 1024,
        ..Limits::default()
    };
    let rt = MacroRuntime::new(limits);

    let error = rt
        .run_macro("function f() { return f(); } f();", &[], "stack.js")
        .unwrap_err();
    match error {
        MacroError::Script(script) => {
            assert_eq!(script.message, "Maximum call stack size exceeded")
        }
        other => panic!("expected Script error, got {other:?}"),
    }
}

#[test]
fn runtime_instance_is_reusable_across_invocations() {
    let rt = MacroRuntime::new(Limits::default());
    for _ in 0..5 {
        let result = rt.run_macro("(1 + 1).toString();", &[], "n.js").unwrap();
        assert_eq!(result.text, "2");
    }
}
