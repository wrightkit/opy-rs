//! Translation lowering and catalog lifecycle coverage.

use std::path::Path;

use crate::{Compiler, compile};

#[test]
fn literal_translation_uses_the_declared_language_order() {
    let hir = compile(
        "#!translations en fr\nrule \"translation\":\n    @Event global\n    bigMessage(getAllPlayers(), _(\"Hello\"))\n",
        "translation.opy",
        Path::new("."),
    )
    .expect("translation source should lower");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains("ＴＬＥｒｒHelloHello"));
    assert!(artifact.emitted.contains("White0Blanc"));
    assert_eq!(artifact.translation_files.len(), 1);
    assert!(artifact.translation_files[0].1.contains("msgid \"Hello\""));
}

#[test]
fn translation_calls_require_a_translation_declaration() {
    let hir = compile(
        "rule \"translation\":\n    @Event global\n    bigMessage(getAllPlayers(), _(\"Hello\"))\n",
        "translation.opy",
        Path::new("."),
    )
    .expect("frontend keeps the translation call for lowering diagnostics");
    let error = match Compiler::new().unwrap().compile_hir(&hir) {
        Ok(_) => panic!("translation without a declaration must fail"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "translations-invalid");
}

#[test]
fn player_translation_mode_allocates_the_reserved_player_variable() {
    let hir = compile(
        "#!translations en fr\n#!translateWithPlayerVar noDetectionRule\nrule \"translation\":\n    @Event eachPlayer\n    bigMessage(getAllPlayers(), _(\"Hello\"))\n",
        "translation-player.opy",
        Path::new("."),
    )
    .expect("player translation source should lower");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact
            .wir
            .player_variables
            .iter()
            .any(|variable| variable.name == "__languageIndex__" && variable.index == Some(127))
    );
    assert!(
        artifact
            .emitted
            .contains("(Local Player).__languageIndex__")
    );
}
