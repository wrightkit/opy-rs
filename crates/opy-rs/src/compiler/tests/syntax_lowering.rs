use std::path::Path;

use crate::Compiler;

#[test]
fn literal_membership_folds_under_optimization() {
    let source = r#"globalvar a
globalvar b
globalvar c
globalvar d
globalvar e
globalvar f

rule "membership":
    @Event global
    a = 1 in [1, 2]
    b = 3 in [1, 2]
    c = 1 not in [1, 2]
    d = 3 not in [1, 2]
    e = "am" in ["**"]
    f = "am" not in ["**"]
"#;
    let hir = crate::compile(source, "membership.opy", Path::new(".")).expect("source resolves");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("hir lowers");
    assert!(artifact.emitted.contains("Set Global Variable(a, True);"));
    assert!(artifact.emitted.contains("Set Global Variable(b, False);"));
    assert!(artifact.emitted.contains("Set Global Variable(c, False);"));
    assert!(artifact.emitted.contains("Set Global Variable(d, True);"));
    assert!(artifact.emitted.contains("Set Global Variable(e, False);"));
    assert!(artifact.emitted.contains("Set Global Variable(f, True);"));
}

#[test]
fn strict_optimization_preserves_runtime_string_membership_and_safe_identical_fold() {
    let source = r#"#!optimizeStrict
globalvar safe_in
globalvar safe_not_in
globalvar runtime_in
globalvar runtime_not_in
globalvar number_in

rule "strict membership":
    @Event global
    safe_in = "am" in ["am", "**"]
    safe_not_in = "am" not in ["am", "**"]
    runtime_in = "am" in ["**", "%%"]
    runtime_not_in = "am" not in ["**", "%%"]
    number_in = 1 in [2, 3]
"#;
    let hir =
        crate::compile(source, "strict_membership.opy", Path::new(".")).expect("source resolves");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("hir lowers");

    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(safe_in, True);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(safe_not_in, False);")
    );

    assert!(artifact.emitted.contains(
        "Set Global Variable(runtime_in, Array Contains(Array(Custom String(\"**\"), Custom String(\"%%\")), Custom String(\"am\")));"
    ));
    assert!(artifact.emitted.contains(
        "Set Global Variable(runtime_not_in, Not(Array Contains(Array(Custom String(\"**\"), Custom String(\"%%\")), Custom String(\"am\"))));"
    ));

    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(number_in, False);")
    );
}
