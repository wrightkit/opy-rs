use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus/synthetic/array-unique")
}

#[test]
fn array_unique_matches_the_pinned_canonical_wir() {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source is readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture resolves");
    let artifact = Compiler::new()
        .expect("compiler initializes")
        .compile_hir(&hir)
        .expect("unique lowers to canonical WIR");

    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle is readable"),
    )
    .expect("oracle is valid JSON");
    let oracle_wir = workshop_rs::parser::parse(
        oracle["compile"]["workshop"]
            .as_str()
            .expect("oracle workshop text is present"),
        &Catalog::builtin().expect("catalog is available"),
        &Locale::new("en-US"),
    )
    .expect("oracle workshop text parses");

    assert!(equivalent(&artifact.wir, &oracle_wir));
}
