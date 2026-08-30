use std::path::Path;

use opy_rs::{CompileStatus, Compiler};

#[test]
fn ordinary_compile_api_uses_only_opy_types() {
    let compiler = Compiler::new().expect("the embedded compiler contract loads");
    let output = compiler
        .compile_source(
            "rule \"api\":\n    @Event global\n    pass\n",
            "api.opy",
            Path::new("."),
        )
        .expect("ordinary source compilation succeeds");

    assert!(output.workshop.contains("rule (\"api\")"));
    assert_eq!(output.emitted_workshop, output.workshop);
    assert!(output.hook_console_output.is_empty());

    let report = compiler.compile_source_report_with_language(
        "rule \"api\":\n    @Event global\n    pass\n",
        "api.opy",
        Path::new("."),
        "en-US",
    );
    assert_eq!(report.compile.status, CompileStatus::Success);
    assert_eq!(report.compiler.name, "opy-rs");
}
