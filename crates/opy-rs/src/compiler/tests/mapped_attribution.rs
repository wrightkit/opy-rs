use std::path::Path;

use crate::Compiler;

/// Macro shapes whose expansion reaches lowering decisions: a body that
/// crosses an optimization directive, span-less nodes inside a body and in
/// substituted arguments, a macro call as an argument, and self-referencing
/// assignments that may lower to a modify action.
const SOURCE: &str = "playervar foo
globalvar a
globalvar g

macro pick():
    if 1 == 1:
        g = 1

macro inner():
    eventPlayer.foo + 1

macro outer(n):
    n * 2

macro bump(p):
    p.foo = p.foo + 1

macro scale(i):
    a[i] = a[i] * 2

macro hit(p):
    damage(p, null, 10)

#!disableOptimizations
rule \"r\":
    @Event eachPlayer
    pick()
    g = outer(inner())
    bump(eventPlayer)
    scale(eventPlayer.foo)
    hit(eventPlayer)
    eventPlayer.foo = outer(eventPlayer.foo)
";

fn hir() -> crate::hir::Program {
    crate::compile(SOURCE, "source.opy", Path::new(".")).expect("source compiles")
}

#[test]
fn mapped_artifact_equals_the_plain_compile() {
    let compiler = Compiler::new().unwrap();
    let hir = hir();
    let plain = compiler.compile_hir(&hir).unwrap();
    let (artifact, mapped) = compiler.compile_hir_mapped(&hir).unwrap();
    // `Program` has no `PartialEq`; its debug form covers nodes and provenance.
    assert_eq!(format!("{:?}", artifact.wir), format!("{:?}", plain.wir));
    assert_eq!(artifact.final_output, plain.final_output);
    assert_eq!(mapped.text, plain.final_output);
}

#[test]
fn map_attributes_macro_code_to_the_invocation_site() {
    let compiler = Compiler::new().unwrap();
    let (artifact, mapped) = compiler.compile_hir_mapped(&hir()).unwrap();
    // `g = 1` inside `pick()`: defined on line 7, invoked on line 27.
    let definition = artifact.wir.action_span(0, 1).expect("definition span");
    assert_eq!(definition.start.line, 7);
    let mut program = workshop_rs::parser::parse(
        &mapped.text,
        compiler.catalog,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
    .unwrap();
    mapped.map.apply(&mut program).unwrap();
    let site = program.action_span(0, 1).expect("invocation span");
    assert_eq!(site.start.line, 27);
}
