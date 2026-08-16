//! Resource-limit and context-lifetime fixtures.
//!
//! Defaults and abort semantics mirror the pinned OverPy reference
//! (`src/quickjs.ts`): deadline-based interruption (`"interrupted"`),
//! `"out of memory"`, and `"Maximum call stack size exceeded"`.

use std::time::{Duration, Instant};

use opy_macro_js::{Limits, MacroError, MacroRuntime};

#[test]
fn busy_loop_is_interrupted_within_budget() {
    // The interrupt handler is deadline-based and polled by the engine, so a
    // 50 ms budget aborts the loop at the next poll point; the assertion that
    // the call returns quickly keeps this reliable on slow CI machines.
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
    // A 2 MiB engine limit makes the allocation loop fail fast, long before
    // the default time budget.
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
    // A small stack limit trips deterministically at the first poll even in
    // unoptimized (debug) builds.
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

#[test]
fn a_failed_invocation_does_not_poison_the_runtime() {
    let rt = MacroRuntime::new(Limits::default());
    assert!(
        rt.run_macro("throw new Error(\"boom\");", &[], "bad.js")
            .is_err()
    );
    let result = rt.run_macro(r#""still works";"#, &[], "good.js").unwrap();
    assert_eq!(result.text, "still works");
}
