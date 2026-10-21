use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

#[test]
fn string_modifier_lowering_matches_the_pinned_oracle() {
    let dir = fixture_dir("string-modifiers");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let oracle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("oracle.json")).unwrap()).unwrap();
    let expected = workshop_rs::parser::parse(
        oracle["compile"]["workshop"].as_str().unwrap(),
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(equivalent(&super::canonical_program(&artifact), &expected));
}

#[test]
fn combined_and_plain_modifiers_match_the_pinned_oracle() {
    // `p` is a no-op, and combinations apply in the reference's fixed
    // pipeline order (b → w → c) rather than source order.
    for (modifier, value, expected) in [
        ("p", "hello", "Custom String(\"hello\")"),
        ("pw", "hi!", "Custom String(\"ｈｉ！\")"),
        ("bw", "ab!", "Custom String(\"Αｂ！\")"),
        ("wb", "ab!", "Custom String(\"Αｂ！\")"),
    ] {
        let source =
            format!("globalvar g\nrule \"r\":\n    @Event global\n    g = {modifier}\"{value}\"\n");
        let hir = crate::compile(&source, "modifier.opy", Path::new(".")).unwrap();
        let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
        assert!(
            artifact.emitted.contains(expected),
            "{modifier}\"{value}\": {}",
            artifact.emitted
        );
    }
}

#[test]
fn combined_f_modifier_formats_and_transforms_the_template() {
    let source = "globalvar g\nglobalvar x\nrule \"r\":\n    @Event global\n    g = fw\"a{x}b\"\n";
    let hir = crate::compile(source, "fw.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact
            .emitted
            .contains("Custom String(\"ａ{0}ｂ\", Global.x)"),
        "{}",
        artifact.emitted
    );
}

#[test]
fn big_letters_modifier_requires_a_convertible_character() {
    // The reference rejects `b` literals whose text has no mappable char,
    // including braces that are not format placeholders.
    for value in ["!?", "{S}!"] {
        let source = format!("globalvar g\nrule \"r\":\n    @Event global\n    g = b\"{value}\"\n");
        let hir = crate::compile(&source, "big.opy", Path::new(".")).unwrap();
        let result = Compiler::new().unwrap().compile_hir(&hir);
        assert!(result.is_err(), "b\"{value}\" must be rejected");
    }
}
