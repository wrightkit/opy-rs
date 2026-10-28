use std::path::Path;

use crate::Compiler;

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
