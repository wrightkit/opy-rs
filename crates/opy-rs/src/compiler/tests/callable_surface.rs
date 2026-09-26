//! Pinned OverPy callable-boundary regressions for #329.

use std::path::Path;

use crate::Compiler;

#[test]
fn translation_calls_use_the_existing_translation_helper() {
    let source = r#"#!translations en
globalvar translated
rule "translation":
    @Event global
    print(_("hello"))
    translated = ___("context", "hello")
"#;
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "translation.opy", Path::new("."))
        .expect("translation call forms must lower through the helper");

    assert!(artifact.emitted.contains("__overpyTranslationHelper__"));
    assert!(artifact.emitted.contains("String Split"));
}

#[test]
fn array_member_call_defaults_and_contextual_lambda_bind() {
    let source = r#"globalvar result
rule "array callable surface":
    @Event global
    result = [1, 2, 3].map(lambda value: value + 1)
    result = [1, 2, 3].all()
    result = [1, 2, 3].any(lambda value, index: value > index)
"#;
    Compiler::new()
        .unwrap()
        .compile_source(source, "array-callables.opy", Path::new("."))
        .expect("member array call forms must compile");
}

#[test]
fn member_macro_self_expands_as_the_receiver() {
    let source = r#"globalvar result
macro Player.copyScore():
    result = self.isAlive()
rule "member macro":
    @Event eachPlayer
    eventPlayer.copyScore()
"#;
    Compiler::new()
        .unwrap()
        .compile_source(source, "member-macro.opy", Path::new("."))
        .expect("member macro self must expand to the receiver");
}

#[test]
fn callable_helpers_have_a_native_lowering_path() {
    let source = r#"globalvar value
rule "helper callables":
    @Event global
    value = arrayToString([1, 2])
    value = compress([1, 2])
    value = compressed([1, 2])
    value = decompressNumbers("abc")
    value = decompressVectors("abc")
    value = hsl(0, 1, 0.5)
    value = log(10)
    value = spacesForLength(3)
    value = spacesForString("abc")
    value = strVisualLength("abc")
    value = timeToString(61)
    value = lerp(0, 10, 0.5)
    value = getSign(-1)
    value = Team.toArray()
    createCasedProgressBarIwt(text = "abc", position = vect(0, 0, 0), scale = 1)
"#;
    Compiler::new()
        .unwrap()
        .compile_source(source, "helper-callables.opy", Path::new("."))
        .expect("helper callable forms must lower");
}

#[test]
fn helper_callables_emit_pinned_width_array_and_time_shapes() {
    let source = r#"globalvar value
rule "helper behavior":
    @Event global
    value = strVisualLength("Wi")
    value = strVisualLength("é")
    value = spacesForLength(142)
    value = spacesForString("W")
    value = arrayToString([1, 2, 3], 1)
    value = timeToString(61)
"#;
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "helper-behavior.opy", Path::new("."))
        .expect("helper behavior must lower");
    assert!(artifact.emitted.contains("597"));
    assert!(artifact.emitted.contains("285"));
    assert!(artifact.emitted.contains("Custom String(\" \")"));
    assert!(artifact.emitted.contains("Custom String(\"{0}, …"));
    assert!(artifact.emitted.contains("Custom String(\"{0}:{1}:{2}\""));
}

#[test]
fn cased_progress_bar_preserves_rows_and_caller_arguments() {
    let source = r#"rule "cased progress":
    @Event global
    createCasedProgressBarIwt(textCount = 2, text = "abc", position = vect(0, 0, 0), scale = 1, clipping = Clip.NONE, textColor = Color.RED, reevaluation = ProgressWorldTextReeval.VISIBILITY_POSITION_VALUES_AND_COLOR, nonTeamSpectators = SpecVisibility.DEFAULT)
"#;
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "cased-progress.opy", Path::new("."))
        .expect("cased progress bar must lower");
    assert_eq!(
        artifact
            .emitted
            .matches("Create Progress Bar In-World Text")
            .count(),
        2
    );
    assert!(artifact.emitted.contains("ａ"));
    assert!(artifact.emitted.contains("Red"));
    assert!(artifact.emitted.contains("Do Not Clip"));
}

#[test]
fn cased_progress_bar_adds_soft_hyphen_to_each_multiline_row() {
    let source = r#"rule "multiline cased progress":
    @Event global
    createCasedProgressBarIwt(textCount = 2, text = "a\nb", position = vect(0, 0, 0), scale = 1)
"#;
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "multiline-cased-progress.opy", Path::new("."))
        .expect("multiline cased progress bar must lower");
    assert_eq!(artifact.emitted.matches('\u{ad}').count(), 4);
}

#[test]
fn tabular_compression_and_shape_validation_follow_oracle() {
    let source = r#"globalvar left
globalvar right
rule "tabular compressed":
    @Event global
    tabular([left, right], [1, 2, 3, 4], true)
"#;
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "tabular-compressed.opy", Path::new("."))
        .expect("compressed tabular must lower");
    assert!(artifact.emitted.contains("String Split"));
    assert!(artifact.emitted.contains("Index Of String Char"));

    let uncompressed = Compiler::new()
        .unwrap()
        .compile_source_artifact(
            &source.replace(", true)", ", false)"),
            "tabular-uncompressed.opy",
            Path::new("."),
        )
        .expect("uncompressed tabular must lower");
    assert_ne!(artifact.emitted, uncompressed.emitted);

    let invalid = r#"globalvar left
globalvar right
rule "tabular invalid":
    @Event global
    tabular([left, right], [1, 2, 3])
"#;
    let error = Compiler::new()
        .unwrap()
        .compile_source(invalid, "tabular-invalid.opy", Path::new("."))
        .expect_err("invalid tabular shape must be rejected");
    assert!(error.to_string().contains("multiple of 2"));
}

#[test]
fn tabular_compression_falls_back_per_non_literal_column() {
    let source = r#"globalvar left
globalvar right
globalvar dynamic
rule "tabular mixed compression":
    @Event global
    tabular([left, right], [1, dynamic, 3, 4], true)
"#;
    let artifact = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "tabular-mixed-compression.opy", Path::new("."))
        .expect("mixed tabular compression must preserve non-literal columns");
    assert_eq!(artifact.emitted.matches("String Split").count(), 1);
    assert!(artifact.emitted.contains("Global.dynamic"));
}

#[test]
fn tabular_and_chase_macro_forms_keep_statement_semantics() {
    let source = r#"globalvar heroes
globalvar scores
rule "tabular":
    @Event global
    tabular([heroes, scores], [Hero.ANA, 3, Hero.SOLDIER_76, 8])
    stopChasing(heroes)
"#;
    Compiler::new()
        .unwrap()
        .compile_source(source, "tabular.opy", Path::new("."))
        .expect("tabular and stopChasing must lower as statement forms");
}

#[test]
fn real_player_and_member_macro_callables_lower() {
    let source = r#"globalvar result
rule "real player helpers":
    @Event eachPlayer
    result = getRealClosestPlayers(eventPlayer.getPosition(), Team.ALL)
    result = getRealClosestPlayer(eventPlayer.getPosition())
    result = getRealFarthestPlayer(eventPlayer.getPosition(), Team.ALL)
    result = eventPlayer.getRealPlayersInViewAngle(Team.ALL, 30)
    result = eventPlayer.getRealPlayersClosestToReticle(Team.ALL)
    result = eventPlayer.getOppositeTeam()
    result = lineIntersectsSphere(eventPlayer.getPosition(), eventPlayer.getFacingDirection(), eventPlayer.getPosition(), 1)
"#;
    Compiler::new()
        .unwrap()
        .compile_source(source, "real-player-callables.opy", Path::new("."))
        .expect("real player callable forms must lower");
}

#[test]
fn pinned_value_functions_fold_literals_like_the_reference() {
    let source = r#"globalvar result
rule "folds":
    @Event global
    result = strLen("abc")
    result = acos(2)
    result = asin(-5)
    result = acosDeg(0.5)
    result = crossProduct(vect(1, 0, 0), vect(0, 1, 0))
    result = normalize(vect(0, 0, 5))
    result = angleDifference(10, 350)
"#;
    let emitted = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "folds.opy", Path::new("."))
        .expect("literal value functions must compile")
        .emitted;

    for expected in [
        "Set Global Variable(result, 3);",
        "Set Global Variable(result, 0);",
        "Set Global Variable(result, -1.570796326794896);",
        "Set Global Variable(result, 0.018277045187202);",
        "Set Global Variable(result, Forward);",
        "Set Global Variable(result, Angle Difference(10, 350));",
    ] {
        assert!(emitted.contains(expected), "missing {expected}\n{emitted}");
    }
}

#[test]
fn event_direction_flags_and_hero_setting_lower_to_their_canonical_calls() {
    let source = r#"globalvar result
globalvar heroSetting = createWorkshopSettingHero("cat", "hero", Hero.ANA)
rule "event values":
    @Event playerDealtDamage
    result = eventDirection
    result = eventWasEnvironment
    result = eventWasHealthPack
"#;
    let emitted = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "event-values.opy", Path::new("."))
        .expect("bare event values and the hero setting must compile")
        .emitted;

    for expected in [
        "Workshop Setting Hero(Custom String(\"cat\"), Custom String(\"hero\"), Ana, 0)",
        "Set Global Variable(result, Event Direction);",
        "Set Global Variable(result, Event Was Environment);",
        "Set Global Variable(result, Event Was Health Pack);",
    ] {
        assert!(emitted.contains(expected), "missing {expected}\n{emitted}");
    }
}

#[test]
fn normalized_vector_towards_becomes_direction_towards() {
    let source = r#"globalvar result
rule "normalize":
    @Event eachPlayer
    result = normalize(vectorTowards(eventPlayer.getPosition(), vect(1, 2, 3)))
"#;
    let emitted = Compiler::new()
        .unwrap()
        .compile_source_artifact(source, "normalize.opy", Path::new("."))
        .expect("normalize of vectorTowards must compile")
        .emitted;

    assert!(
        emitted.contains(
            "Set Global Variable(result, Direction Towards(Position Of(Event Player), Vector(1, 2, 3)));"
        ),
        "{emitted}"
    );
}
