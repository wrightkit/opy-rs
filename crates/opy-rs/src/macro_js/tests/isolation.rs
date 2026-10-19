//! Independent evidence for the fresh-engine isolation contract.

use std::time::Duration;

use crate::macro_js::error::MacroError;
use crate::macro_js::limits::Limits;
use crate::macro_js::runtime::MacroRuntime;

fn runtime() -> MacroRuntime {
    MacroRuntime::new(Limits::default())
}

#[test]
fn global_state_does_not_cross_invocations() {
    let rt = runtime();
    rt.run_macro(r#"globalThis.opyLeak = "secret"; "ok";"#, &[], "first.js")
        .unwrap();
    let result = rt
        .run_macro("typeof globalThis.opyLeak;", &[], "second.js")
        .unwrap();
    assert_eq!(result.text, "undefined");
}

#[test]
fn prototype_mutation_does_not_cross_invocations() {
    let rt = runtime();
    rt.run_macro(
        r#"Object.prototype.opyLeak = "secret"; "ok";"#,
        &[],
        "first.js",
    )
    .unwrap();
    let result = rt
        .run_macro("typeof ({}).opyLeak;", &[], "second.js")
        .unwrap();
    assert_eq!(result.text, "undefined");
}

#[test]
fn helper_bindings_and_console_output_are_per_invocation() {
    let rt = runtime();
    let first = rt
        .run_macro(
            r#"Map.opyLeak = "secret"; console.log("first"); "ok";"#,
            &[],
            "first.js",
        )
        .unwrap();
    assert_eq!(first.console_output, vec!["first"]);

    let second = rt
        .run_macro("typeof Map.opyLeak;", &[], "second.js")
        .unwrap();
    assert_eq!(second.text, "undefined");
    assert!(second.console_output.is_empty());
}

#[test]
fn exception_does_not_poison_the_next_invocation() {
    let rt = runtime();
    let error = rt
        .run_macro("throw new Error(\"boom\");", &[], "first.js")
        .unwrap_err();
    assert!(matches!(error, MacroError::Script(_)));

    let result = rt.run_macro(r#""still works";"#, &[], "second.js").unwrap();
    assert_eq!(result.text, "still works");
}

#[test]
fn interrupted_invocation_does_not_poison_the_next_invocation() {
    let rt = MacroRuntime::new(Limits {
        macro_time_budget: Duration::from_millis(20),
        ..Limits::default()
    });
    let error = rt
        .run_macro("while (true) {}", &[], "first.js")
        .unwrap_err();
    assert!(matches!(error, MacroError::Script(_)));

    let result = rt.run_macro(r#""still works";"#, &[], "second.js").unwrap();
    assert_eq!(result.text, "still works");
}

#[test]
fn concurrent_runtime_instances_are_independent() {
    std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|index| {
                scope.spawn(move || {
                    let rt = runtime();
                    let source =
                        format!(r#"globalThis.opyWorker = {index}; (opyWorker + 1).toString();"#);
                    let result = rt.run_macro(&source, &[], "concurrent.js").unwrap();
                    assert_eq!(result.text, (index + 1).to_string());
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

#[test]
fn memory_limit_recovery_keeps_each_invocation_bounded() {
    // An invocation that exhausts the memory limit must neither leak its
    // allocation accounting nor disable the limit for the next invocation.
    let limits = Limits {
        memory_limit_bytes: 2 * 1024 * 1024,
        ..Limits::default()
    };
    let rt = MacroRuntime::new(limits);
    let allocation_loop =
        r#"var a = []; for (var i = 0; i < 1000000; i++) { a.push("x".repeat(1024)); }"#;

    for _ in 0..2 {
        let error = rt.run_macro(allocation_loop, &[], "mem.js").unwrap_err();
        match error {
            MacroError::Script(ref script) => assert_eq!(script.message, "out of memory"),
            ref other => panic!("expected Script error, got {other:?}"),
        }
    }
    let result = rt.run_macro(r#""recovered";"#, &[], "after.js").unwrap();
    assert_eq!(result.text, "recovered");
}

#[test]
fn stack_limit_recovery_keeps_each_invocation_bounded() {
    let limits = Limits {
        max_stack_bytes: 128 * 1024,
        ..Limits::default()
    };
    let rt = MacroRuntime::new(limits);

    for _ in 0..2 {
        let error = rt
            .run_macro("function f() { return f(); } f();", &[], "stack.js")
            .unwrap_err();
        match error {
            MacroError::Script(ref script) => {
                assert_eq!(script.message, "Maximum call stack size exceeded")
            }
            ref other => panic!("expected Script error, got {other:?}"),
        }
    }
    let result = rt.run_macro(r#""recovered";"#, &[], "after.js").unwrap();
    assert_eq!(result.text, "recovered");
}

#[test]
fn deadline_state_is_re_armed_per_invocation() {
    // After an interrupted invocation the interrupt handler must be reset:
    // the next call gets a fresh deadline, not the expired one.
    let rt = MacroRuntime::new(Limits {
        macro_time_budget: Duration::from_millis(50),
        ..Limits::default()
    });
    rt.run_macro("while (true) {}", &[], "first.js")
        .unwrap_err();
    // Succeeding invocations after the abort get a full fresh budget.
    for _ in 0..3 {
        let result = rt
            .run_macro(
                r#"var t = Date.now(); while (Date.now() - t < 1) {} "ok";"#,
                &[],
                "second.js",
            )
            .unwrap();
        assert_eq!(result.text, "ok");
    }
}

#[test]
fn console_sink_is_rebound_per_invocation() {
    // `console.log` writes to the invocation's own buffer: a previous call's
    // sink must not receive or retain lines.
    let rt = runtime();
    let first = rt
        .run_macro(r#"console.log("a"); "ok";"#, &[], "first.js")
        .unwrap();
    assert_eq!(first.console_output, vec!["a"]);

    let error = rt
        .run_macro(r#"console.log("b"); throw new Error("x");"#, &[], "err.js")
        .unwrap_err();
    assert!(matches!(error, MacroError::Script(_)));

    let second = rt
        .run_macro(r#"console.log("c", 1 + 1); "ok";"#, &[], "second.js")
        .unwrap();
    assert_eq!(second.console_output, vec!["c 2"]);
}

#[test]
fn many_consecutive_invocations_stay_independent() {
    // Sweep: interleave leaking, throwing, and clean invocations on one
    // runtime; every result must reflect only its own invocation.
    let rt = runtime();
    for i in 0..32 {
        // Set marker{i} and verify it is visible in the same invocation;
        // a later script then asserts it is gone again.
        let leak =
            format!(r#"globalThis.marker{i} = {i}; (globalThis.marker{i} === {i}).toString();"#);
        let leaked = rt.run_macro(&leak, &[], "leak.js").unwrap();
        assert_eq!(leaked.text, "true");
        let check = rt
            .run_macro(&format!(r#"typeof globalThis.marker{i};"#), &[], "check.js")
            .unwrap();
        assert_eq!(check.text, "undefined");
        if i > 0 {
            let previous = rt
                .run_macro(
                    &format!(r#"typeof globalThis.marker{};"#, i - 1),
                    &[],
                    "check-prev.js",
                )
                .unwrap();
            assert_eq!(previous.text, "undefined");
        }
        if i % 3 == 0 {
            rt.run_macro("throw new Error(\"x\");", &[], "err.js")
                .unwrap_err();
        }
    }
}

#[test]
fn builtin_intrinsics_are_fully_reset() {
    // Mutation of shared-looking intrinsics (Object.prototype, Array methods,
    // the builtins script's `vect`) must not reach the next invocation.
    let rt = runtime();
    rt.run_macro(
        r#"Array.prototype.push = function() { return -1; };
           vect = function() { return "tampered"; };
           Date.now = function() { return 0; };
           "ok";"#,
        &[],
        "poison.js",
    )
    .unwrap();
    let result = rt
        .run_macro(
            r#"var a = [1]; a.push(2);
               (a.length === 2 && typeof vect(1,2,3).x === "number" && Date.now() !== 0).toString();"#,
            &[],
            "verify.js",
        )
        .unwrap();
    assert_eq!(result.text, "true");
}
