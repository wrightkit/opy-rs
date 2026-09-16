use std::path::Path;

use super::super::Compiler;

#[test]
fn compressed_literals_inline_the_oracle_alphabet_once() {
    let compiler = Compiler::new().expect("compiler initializes");
    let hir = crate::compile(
        "globalvar values = compressed([1, 2, 3, 4, 5, 6, 7])\nrule \"compression\":\n    @Event global\n    print(values)\n",
        "compression.opy",
        Path::new("."),
    )
    .expect("compressed literal resolves");
    let artifact = compiler
        .compile_hir(&hir)
        .expect("compressed literal lowers");

    assert!(artifact.emitted.contains(
        "Mapped Array(Mapped Array(String Split(Custom String(\"\u{2}0\u{3}0\u{4}0\u{5}0\u{6}0\u{7}0\u{8}\"), First Of(Null)), Append To Array(Current Array Element, Custom String(\""
    ));
    assert!(!artifact.emitted.contains("__compressionAlphabet__"));
}

#[test]
fn compression_directive_uses_a_shared_global_for_vectors() {
    let compiler = Compiler::new().expect("compiler initializes");
    let hir = crate::compile(
        "#!useVariableForCompressionAlphabet\nglobalvar values = compressed([vect(1, 2, 3), vect(4, 5, 6), vect(7, 8, 9), vect(10, 11, 12), vect(13, 14, 15)])\nrule \"compression\":\n    @Event global\n    print(values)\n",
        "compression.opy",
        Path::new("."),
    )
    .expect("compressed vectors resolve");
    let artifact = compiler
        .compile_hir(&hir)
        .expect("compressed vectors lower");

    assert!(artifact.emitted.contains("127: __compressionAlphabet__"));
    assert!(artifact.emitted.contains(
        "Custom String(\"\u{2}\u{4}\u{3}0\u{5}\u{7}\u{6}0\u{8}\\n\t0\u{b}\\r\u{c}0\u{e}\u{10}\u{f}\")"
    ));
    assert!(artifact.emitted.contains(
        "Vector(Index Of String Char(Global.__compressionAlphabet__, Char In String(Current Array Element, 0)), Index Of String Char(Global.__compressionAlphabet__, Char In String(Current Array Element, 2)), Index Of String Char(Global.__compressionAlphabet__, Char In String(Current Array Element, 1)))"
    ));
}

#[test]
fn compressed_requires_a_literal_array() {
    let compiler = Compiler::new().expect("compiler initializes");
    let hir = crate::compile(
        "globalvar values = [1, 2]\nglobalvar compressed_values = compressed(values)\nrule \"compression\":\n    @Event global\n    print(compressed_values)\n",
        "compression.opy",
        Path::new("."),
    )
    .expect("source resolves before integration validation");
    let error = match compiler.compile_hir(&hir) {
        Ok(_) => panic!("variable compression input must be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert!(error.diagnostic.message.contains("literal array"));
}

#[test]
fn compressed_zero_and_negative_scalars_match_the_oracle_shape() {
    let compiler = Compiler::new().expect("compiler initializes");
    let hir = crate::compile(
        "globalvar zero = compressed([0])\nglobalvar negative = compressed([-2, 1])\nrule \"compression\":\n    @Event global\n    print(zero)\n    print(negative)\n",
        "compression.opy",
        Path::new("."),
    )
    .expect("scalar compression resolves");
    let artifact = compiler
        .compile_hir(&hir)
        .expect("zero and negative scalars lower without panicking");

    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(zero, Mapped Array(")
    );
    assert!(artifact.emitted.contains(")), 0));"));
    assert!(artifact.emitted.contains("Add(Index Of String Char"));
    assert!(artifact.emitted.contains(", -2)));"));
}
