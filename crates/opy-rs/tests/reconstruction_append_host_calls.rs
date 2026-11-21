//! `appendToArray`, `hostPlayer` and `gameMode` value calls in Workshop → OPY
//! reconstruction (opy-rs#744). Inputs are Workshop text, run in en-US and
//! zh-CN (the zh-CN text is the same program re-emitted by
//! `workshop_rs::emitter`).

use std::path::Path;

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use workshop_rs::catalog::{Catalog, Locale};

fn rule(actions: &str) -> String {
    format!(
        "variables {{\n    global:\n        0: g\n        1: h\n        2: i\n    player:\n        0: p\n}}\n\n\
         rule (\"r\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n{actions}\n    }}\n}}\n"
    )
}

fn program_in(text: &str, language: &str) -> workshop_rs::Program {
    let catalog = Catalog::builtin().expect("catalog loads");
    let program = workshop_rs::parser::parse(text, &catalog, &Locale::new("en-US")).unwrap();
    let locale = Locale::new(language);
    let text = workshop_rs::emitter::emit(&program, &catalog, &locale).expect("text emits");
    workshop_rs::parser::parse(&text, &catalog, &locale).expect("text parses")
}

fn assert_round_trip(actions: &str, expect: &[&str]) {
    let catalog = Catalog::builtin().expect("catalog loads");
    for language in ["en-US", "zh-CN"] {
        let program = program_in(&rule(actions), language);
        let opy = reconstruct(&program).unwrap_or_else(|e| panic!("{language}: refused: {e}"));
        for needle in expect {
            assert!(opy.contains(needle), "{language}: missing {needle}\n{opy}");
        }
        let locale = Locale::new(language);
        let recompiled = Compiler::new()
            .expect("compiler loads")
            .compile_source_with_locale(&opy, "r.opy", Path::new("."), &locale)
            .unwrap_or_else(|e| panic!("{language}: {e:?}\n{opy}"));
        let reparsed = workshop_rs::parser::parse(&recompiled.emitted, &catalog, &locale).unwrap();
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{language}: {:?}\n{opy}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }
}

#[test]
fn append_to_array_round_trips() {
    assert_round_trip(
        "Set Global Variable(g, Append To Array(Global Variable(h), Global Variable(i)));\n\
         Set Global Variable(h, Append To Array(Append To Array(Global Variable(h), Global Variable(i)), Array(1, 2)));\n\
         Set Global Variable(i, Append To Array(Global Variable(i), 1));\n\
         Set Player Variable(Event Player, p, Append To Array(Player Variable(Event Player, p), Global Variable(g)));\n\
         Set Global Variable At Index(g, 1, Append To Array(Value In Array(Global Variable(h), 1), Global Variable(i)));",
        &["(h).concat(i)", ".concat([1, 2])", "(i).concat(1)"],
    );
}

#[test]
fn host_player_round_trips_as_value_and_receiver() {
    assert_round_trip(
        "Set Global Variable(g, Host Player);\n\
         Set Global Variable(h, Player Variable(Host Player, p));\n\
         Set Player Variable(Host Player, p, Is Button Held(Host Player, Button(Ultimate)));",
        &["g = hostPlayer", "(hostPlayer).p"],
    );
}

#[test]
fn game_mode_round_trips() {
    assert_round_trip(
        "Set Global Variable(g, Game Mode(Skirmish));\n\
         Set Global Variable(h, Game Mode(Practice Range));",
        &["Gamemode.SKIRMISH", "Gamemode.PRACTICE_RANGE"],
    );
}
