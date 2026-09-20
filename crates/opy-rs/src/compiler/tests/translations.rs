//! Translation lowering and catalog lifecycle coverage.

use std::fs;
use std::path::Path;

use crate::{Compiler, compile};

fn pinned_workshop(id: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus")
        .join(id)
        .join("oracle.json");
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("pinned oracle snapshot"))
            .expect("valid pinned oracle snapshot");
    snapshot["compile"]["workshop"]
        .as_str()
        .expect("pinned oracle workshop output")
        .to_string()
}

fn fixture_output(id: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus")
        .join(id);
    let source = fs::read_to_string(root.join("source.opy")).expect("fixture source");
    let hir = compile(&source, "source.opy", &root).expect("fixture source should lower");
    Compiler::new()
        .expect("compiler")
        .compile_hir(&hir)
        .expect("fixture should compile")
        .emitted
}

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

#[test]
fn pinned_translation_fixtures_cover_detection_and_fallback_shapes() {
    let default_oracle = pinned_workshop("synthetic/translations-player-default-331");
    let default_output = fixture_output("synthetic/translations-player-default-331");
    assert!(default_oracle.contains("Determine the player's language"));
    assert!(default_output.contains("Determine the player's language"));
    assert!(default_oracle.contains("Not(Modulo("));
    assert!(default_output.contains("Not(Modulo("));

    let no_detection_oracle = pinned_workshop("synthetic/translations-player-no-detection-331");
    let no_detection_output = fixture_output("synthetic/translations-player-no-detection-331");
    assert!(!no_detection_oracle.contains("Determine the player's language"));
    assert!(!no_detection_output.contains("Determine the player's language"));

    let no_tl_err_oracle = pinned_workshop("synthetic/translations-player-no-tlerr-331");
    let no_tl_err_output = fixture_output("synthetic/translations-player-no-tlerr-331");
    assert!(no_tl_err_oracle.contains("__languageIndex__, Subtract"));
    assert!(no_tl_err_output.contains("__languageIndex__, Subtract"));
    assert!(!no_tl_err_oracle.contains("ＴＬＥｒｒ"));
    assert!(!no_tl_err_output.contains("ＴＬＥｒｒ"));

    let long_oracle = pinned_workshop("synthetic/translations-long-string-331");
    let long_output = fixture_output("synthetic/translations-long-string-331");
    assert!(long_oracle.contains("String Replace"));
    assert!(long_output.contains("String Replace"));
}
