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

#[test]
fn player_translation_mode_matches_the_pinned_option_shapes() {
    let default_hir = compile(
        "#!translations en fr\n#!translateWithPlayerVar\nrule \"translation\":\n    @Event global\n    bigMessage(getAllPlayers(), _(\"Hello\"))\n",
        "translation-player-default.opy",
        Path::new("."),
    )
    .unwrap();
    let default_output = Compiler::new()
        .unwrap()
        .compile_hir(&default_hir)
        .unwrap()
        .emitted;
    assert!(default_output.contains("OverPy translation setup - Determine the player's language"));
    assert!(default_output.contains("Start Facing"));
    assert!(default_output.contains("Set Player Variable At Index"));

    let no_detection_hir = compile(
        "#!translations en fr\n#!translateWithPlayerVar noDetectionRule\nrule \"translation\":\n    @Event global\n    bigMessage(getAllPlayers(), _(\"Hello\"))\n",
        "translation-player-no-detection.opy",
        Path::new("."),
    )
    .unwrap();
    let no_detection_output = Compiler::new()
        .unwrap()
        .compile_hir(&no_detection_hir)
        .unwrap()
        .emitted;
    assert!(!no_detection_output.contains("Determine the player's language"));

    let no_tl_err_hir = compile(
        "#!translations en fr\n#!translateWithPlayerVar noTlErr\nrule \"translation\":\n    @Event global\n    bigMessage(getAllPlayers(), _(\"Hello\"))\n",
        "translation-player-no-tlerr.opy",
        Path::new("."),
    )
    .unwrap();
    let no_tl_err_output = Compiler::new()
        .unwrap()
        .compile_hir(&no_tl_err_hir)
        .unwrap()
        .emitted;
    assert!(no_tl_err_output.contains("Set Player Variable(Event Player, __languageIndex__, 0.1)"));
    assert!(!no_tl_err_output.contains("ＴＬＥｒｒ"));
}

#[test]
fn translated_format_strings_use_replacements_after_the_workshop_argument_limit() {
    let hir = compile(
        "#!translations en fr\nrule \"translation\":\n    @Event eachPlayer\n    bigMessage(getAllPlayers(), _(\"{} {} {} {} {}\".format(eventPlayer, eventPlayer, eventPlayer, eventPlayer, eventPlayer)))\n",
        "translation-format.opy",
        Path::new("."),
    )
    .unwrap();
    let output = Compiler::new().unwrap().compile_hir(&hir).unwrap().emitted;
    assert!(output.contains("String Replace"));
    assert!(!output.contains("translated format strings support"));
}
