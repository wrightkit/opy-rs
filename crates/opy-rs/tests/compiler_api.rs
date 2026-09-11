use std::path::Path;

use opy_rs::{CompileStatus, Compiler, FilesystemProject};

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

#[test]
fn filesystem_project_entry_owns_include_root_resolution() {
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic/project-entry/source.opy");
    let project = FilesystemProject::load(&entry).expect("filesystem project loads");
    let main_path = entry.canonicalize().expect("entry canonicalizes");

    assert_eq!(project.main_path(), entry);
    assert_eq!(project.root(), main_path.parent().expect("entry parent"));

    let outcome = opy_rs::tooling::check(
        project.source(),
        &project.main_path().to_string_lossy(),
        project.root(),
    );
    assert!(
        outcome.is_clean(),
        "entry project failed: {:?}",
        outcome.diagnostics
    );
    assert_eq!(outcome.files[1].path, "child.opy");
}
