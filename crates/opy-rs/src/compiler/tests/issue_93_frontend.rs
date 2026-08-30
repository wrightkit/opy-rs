use std::path::Path;

use crate::{CompileFailureClass, CompileStatus, Compiler};
use workshop_rs::catalog::Locale;

#[test]
fn frontend_diagnostics_are_public_and_source_attributed() {
    let cases = [
        (
            "issue-28-invalid-syntax.opy",
            "globalvar value\n\nrule \"issue 28 invalid syntax\":\n    @Event global\n    do:\n        value = 1\n    while\n    value = {\"key\"}\n    value = u\"invalid modifier\"\n",
            "parse-error",
        ),
        (
            "issue-29-invalid.opy",
            "globalvar value\n\nrule \"invalid directive\":\n    @Event global\n    @Name \"wrong\"\n    value = 1\n",
            "parse-error",
        ),
        (
            "issue-31-negative.opy",
            "#!translations en_US\nrule \"invalid translation\":\n    pass\n",
            "translations-invalid",
        ),
        (
            "issue-33-lambda-negative.opy",
            "globalvar value\n\nrule \"issue 33 lambda negative\":\n    @Event global\n    debug(lambda item: item)\n",
            "lambda-context",
        ),
    ];
    let compiler = Compiler::new().expect("released Workshop contract must load");

    for (source_name, source, code) in cases {
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
        let span = diagnostic
            .span
            .as_ref()
            .unwrap_or_else(|| panic!("missing source span for {source_name}"));
        assert_eq!(span.path, source_name, "{source_name}");
    }
}
