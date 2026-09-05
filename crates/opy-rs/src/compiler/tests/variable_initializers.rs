//! Global variable initializer lowering coverage.

use std::path::Path;

use crate::Compiler;

#[test]
fn null_initializers_are_dropped_but_float_zero_is_preserved() {
    let source = r#"
globalvar null_value = null
globalvar integer_zero = 0
globalvar float_zero = 0.0

rule "initializer contract":
    @Event global
    pass
"#;
    let hir = crate::compile(source, "initializer-contract.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(!artifact.emitted.contains("Set Global Variable(null_value,"));
    assert!(
        !artifact
            .emitted
            .contains("Set Global Variable(integer_zero,")
    );
    assert!(artifact.emitted.contains("Set Global Variable(float_zero,"));
}
