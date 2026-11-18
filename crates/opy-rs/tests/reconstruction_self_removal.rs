//! `attacker`, `victim`, `healer`, `healee` and `removeFromArray` value calls
//! in Workshop → OPY reconstruction (opy-rs#744). A Set whose
//! `removeFromArray` operand is its own target recompiles to a Modify form,
//! so it is refused by construct name; every other shape must round-trip.
//! Inputs are Workshop text, not source compiled from OPY first.

use std::path::Path;

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use workshop_rs::catalog::{Catalog, Locale};

fn rule(actions: &str) -> String {
    format!(
        "variables {{\n    global:\n        0: g\n        1: h\n        2: i\n    player:\n        0: p\n}}\n\n\
         rule (\"r\") {{\n    event {{\n        Player Took Damage;\n        All;\n        All;\n    }}\n    actions {{\n{actions}\n    }}\n}}\n"
    )
}

/// The program parsed from en-US text and re-emitted in `language`.
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

fn assert_refused(actions: &str, count: usize) {
    for language in ["en-US", "zh-CN"] {
        let program = program_in(&rule(actions), language);
        let message = reconstruct(&program).expect_err("refused").to_string();
        assert_eq!(
            message.matches("unsupported-value-call").count(),
            count,
            "{language}: {message}"
        );
        assert_eq!(
            message.matches("'removeFromArray'").count(),
            count,
            "{message}"
        );
    }
}

#[test]
fn event_player_values_and_exclude_round_trip() {
    assert_round_trip(
        "Set Global Variable(g, Attacker);\n\
         Set Global Variable(h, Victim);\n\
         Set Global Variable(i, Remove From Array(Global Variable(h), 5));",
        &["= attacker", "= victim", ".exclude(5)"],
    );
}

#[test]
fn modify_form_and_different_array_round_trip() {
    assert_round_trip(
        "Modify Global Variable(g, Remove From Array By Value, 5);\n\
         Set Global Variable(h, Remove From Array(Global Variable(g), 5));\n\
         Set Player Variable(Event Player, p, Remove From Array(Global Variable(g), 5));\n\
         Set Global Variable At Index(g, 1, Remove From Array(Value In Array(Global Variable(h), 1), 5));",
        &[".exclude(5)"],
    );
}

#[test]
fn indexes_differing_only_in_grouping_round_trip() {
    assert_round_trip(
        "Set Global Variable At Index(g, Multiply(Add(Global Variable(h), Global Variable(i)), 2), \
         Remove From Array(Value In Array(Global Variable(g), \
         Add(Global Variable(h), Multiply(Global Variable(i), 2))), 5));\n\
         Set Player Variable At Index(Event Player, p, Multiply(Add(Global Variable(h), Global Variable(i)), 2), \
         Remove From Array(Value In Array(Player Variable(Event Player, p), \
         Add(Global Variable(h), Multiply(Global Variable(i), 2))), 5));",
        &[".exclude(5)"],
    );
}

#[test]
fn self_referential_plain_set_is_refused() {
    assert_refused(
        "Set Global Variable(g, Remove From Array(Global Variable(g), 5));\n\
         Set Player Variable(Event Player, p, Remove From Array(Player Variable(Event Player, p), 5));",
        2,
    );
}

#[test]
fn self_referential_indexed_set_is_refused() {
    assert_refused(
        "Set Global Variable At Index(g, 1, Remove From Array(Value In Array(Global Variable(g), 1), 5));\n\
         Set Player Variable At Index(Event Player, p, 1, \
         Remove From Array(Value In Array(Player Variable(Event Player, p), 1), 5));\n\
         Set Global Variable At Index(g, Multiply(Add(Global Variable(h), Global Variable(i)), 2), \
         Remove From Array(Value In Array(Global Variable(g), \
         Multiply(Add(Global Variable(h), Global Variable(i)), 2)), 5));",
        3,
    );
}
