//! Round trip for the player-variable chase actions: the global and player
//! forms share one OPY call (opy-rs#745).

use std::path::Path;

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use workshop_rs::catalog::{Catalog, Locale};

const SOURCE: &str = r#"
globalvar G
playervar A

rule "chase":
    @Event eachPlayer
    chaseAtRate(eventPlayer.A, 100, 10, ChaseRateReeval.DESTINATION_AND_RATE)
    chaseOverTime(eventPlayer.A, 100, 2, ChaseTimeReeval.DESTINATION_AND_DURATION)
    chaseAtRate(G, 100, 10, ChaseRateReeval.NONE)
    chaseOverTime(G, 100, 2, ChaseTimeReeval.NONE)
    stopChasingVariable(G)
    stopChasingVariable(eventPlayer.A)
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

    let original = compile(SOURCE, "chase.opy");
    let program = workshop_rs::parser::parse(&original.emitted, &catalog, &locale)
        .expect("compiled Workshop text parses");
    let opy = reconstruct(&program).unwrap_or_else(|e| panic!("{language}: refused: {e}"));
    assert!(opy.contains("chaseAtRate(eventPlayer.A"), "{opy}");
    assert!(opy.contains("chaseOverTime(eventPlayer.A"), "{opy}");
    assert!(opy.contains("stopChasingVariable(eventPlayer.A"), "{opy}");

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
fn player_variable_chase_actions_round_trip_in_en_us() {
    round_trip("en-US");
}

#[test]
fn player_variable_chase_actions_round_trip_in_zh_cn() {
    round_trip("zh-CN");
}
