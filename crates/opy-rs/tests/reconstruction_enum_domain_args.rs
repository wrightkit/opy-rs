//! Round trip for arguments in an enum domain that are not enum literals:
//! computed values, arrays and context-resolved constants (opy-rs#746).

use std::path::Path;

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use workshop_rs::catalog::{Catalog, Locale};

const SOURCE: &str = r#"
globalvar A
globalvar H
globalvar T

rule "enum domain arguments":
    @Event eachPlayer
    eventPlayer.setAllowedHeroes([Hero.ANA, Hero.REAPER])
    eventPlayer.setAllowedHeroes(getAllHeroes())
    eventPlayer.setAllowedHeroes(H)
    A = getLivingPlayers(eventPlayer.getTeam())
    A = getLivingPlayers(getOppositeTeam(eventPlayer.getTeam()))
    A = getLivingPlayers(getOppositeTeam(Team.2))
    A = getPlayers(getOppositeTeam(T))
    eventPlayer.applyImpulse(directionTowards(eventPlayer, eventPlayer), 3, Relativity.TO_WORLD, Impulse.CANCEL_CONTRARY_MOTION)
    eventPlayer.applyImpulse(eventPlayer.getFacingDirection(), 3, Relativity.TO_WORLD, Impulse.CANCEL_CONTRARY_MOTION)
    eventPlayer.startThrottleInDirection(eventPlayer.getFacingDirection(), 1, Relativity.TO_PLAYER, Throttle.REPLACE_EXISTING, ThrottleReeval.DIRECTION_AND_MAGNITUDE)
    eventPlayer.setFacing(worldVector(Vector.LEFT, eventPlayer, Transform.ROTATION), Relativity.TO_WORLD)
    hudText(eventPlayer, null, "x", null, HudPosition.TOP, 1, null, Color.WHITE, null, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    A = Vector.UP * eventPlayer.getHealth()
    eventPlayer.setFacing(worldVector(eventPlayer.getFacingDirection(), eventPlayer, Transform.ROTATION), Relativity.TO_WORLD)
"#;

fn round_trip(language: &str) {
    let compiler = Compiler::new().expect("compiler loads");
    let catalog = Catalog::builtin().expect("catalog loads");
    let locale = Locale::new(language);
    let compile = |source: &str, path: &str| {
        compiler
            .compile_source_with_locale(source, path, Path::new("."), &locale)
            .unwrap_or_else(|e| panic!("{path} does not compile: {e:?}\n{source}"))
    };

    let original = compile(SOURCE, "enum_domain.opy");
    let program = workshop_rs::parser::parse(&original.emitted, &catalog, &locale)
        .expect("compiled Workshop text parses");
    let opy = reconstruct(&program).unwrap_or_else(|e| panic!("{language}: refused: {e}"));

    let recompiled = compile(&opy, "reconstructed.opy");
    let reparsed = workshop_rs::parser::parse(&recompiled.emitted, &catalog, &locale)
        .expect("recompiled Workshop text parses");
    assert!(
        workshop_rs::compare::equivalent(&program, &reparsed),
        "{language}: {:?}\n{opy}",
        workshop_rs::compare::difference(&program, &reparsed)
    );
}

#[test]
fn enum_domain_arguments_round_trip_in_en_us() {
    round_trip("en-US");
}

#[test]
fn enum_domain_arguments_round_trip_in_zh_cn() {
    round_trip("zh-CN");
}
