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
