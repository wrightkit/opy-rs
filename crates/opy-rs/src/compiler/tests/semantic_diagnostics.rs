//! Source-semantic failure-frontier evidence.

use std::path::Path;

use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::Locale;

#[test]
fn semantic_boundary_errors_are_reported_before_lowering() {
    let cases = [
        (
            "invalid-range-binder.opy",
            "rule \"invalid binder\":\n    @Event global\n    for 1 in range(3):\n        pass\n",
            "invalid-range-binder",
            "Expected variable for 1st argument",
            3,
            9,
        ),
        (
            "four-dimensional-assignment.opy",
            "globalvar nested\n\nrule \"four-dimensional indexed assignment\":\n    @Event global\n    nested[0][0][0][0] = 1\n",
            "four-dimensional-assignment",
            "Cannot assign to 4d array",
            5,
            5,
        ),
    ];
    let compiler = Compiler::new().expect("released Workshop contract must load");

    for (source_name, source, code, message, line, col) in cases {
        let report = compiler.compile_source_report_with_locale(
            source,
            source_name,
            Path::new("."),
            &Locale::new("en-US"),
        );
        assert_eq!(
            report.compile.status,
            CompileStatus::Failure,
            "{source_name}"
        );
        assert_eq!(
            report.compile.failure_class,
            Some(CompileFailureClass::Frontend),
            "{source_name}"
        );
        let diagnostic = report
            .compile
            .diagnostics
            .first()
            .unwrap_or_else(|| panic!("missing diagnostic for {source_name}"));
        assert_eq!(diagnostic.code, code, "{source_name}");
        assert!(diagnostic.message.contains(message), "{source_name}");
        let span = diagnostic
            .span
            .as_ref()
            .unwrap_or_else(|| panic!("missing source span for {source_name}"));
        assert_eq!(span.path, source_name, "{source_name}");
        assert_eq!(span.start.line, line, "{source_name}");
        assert_eq!(span.start.col, col, "{source_name}");
    }
}
