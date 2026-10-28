use std::path::Path;

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn compile_source(source: &str) -> crate::CompilationArtifact {
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("source must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("source must lower to canonical WIR")
}

#[test]
fn demonstrated_comprehension_lowers_to_canonical_mapped_array() {
    let artifact = compile_source(
        "globalvar value\nrule \"r\":\n    @Event global\n    value = [item * 2 for item in [1, 2]]\n",
    );
    let expected = workshop_rs::parser::parse(
        "variables {\n    global:\n        0: value\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(value, Mapped Array(Array(1, 2), Multiply(Current Array Element, 2)));\n    }\n}\n",
        &Catalog::builtin().expect("catalog must load"),
        &Locale::new("en-US"),
    )
    .expect("expected canonical WIR must reparse");
    assert!(
        equivalent(&super::canonical_program(&artifact), &expected),
        "native WIR diverged\n{}",
        artifact.emitted
    );
}

#[test]
fn unsupported_sorted_key_shape_remains_source_attributed() {
    let source =
        "globalvar value\nrule \"r\":\n    @Event global\n    value = sorted([1, 2], key=1)\n";
    let hir = crate::compile(source, "sorted.opy", Path::new(".")).expect("source must resolve");
    let error = match Compiler::new()
        .expect("compiler must load")
        .compile_hir(&hir)
    {
        Ok(_) => panic!("non-lambda sorted key unexpectedly compiled"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(
        error
            .diagnostic
            .span
            .expect("error must have a span")
            .start
            .line,
        4
    );
}

#[test]
fn scalar_player_comprehension_is_wrapped_before_filtering() {
    let source = "globalvar x\nrule \"r\":\n    @Event eachPlayer\n    x = [player for player in eventPlayer.getRealPlayerClosestToReticle(Team.ALL) if player.isAlive()]\n";
    let hir = crate::compile(source, "source.opy", Path::new(".")).expect("frontend resolves");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("scalar player comprehension must lower to canonical WIR");
    assert!(artifact.emitted.contains("Sorted Array"));
    assert!(
        artifact
            .emitted
            .contains("Has Spawned(Current Array Element)")
    );
    assert!(artifact.emitted.contains("Is Alive(Current Array Element)"));
}
