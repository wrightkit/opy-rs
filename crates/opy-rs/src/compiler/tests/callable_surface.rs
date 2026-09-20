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
