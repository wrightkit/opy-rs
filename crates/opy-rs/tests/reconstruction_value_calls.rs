//! Round trip for value calls whose OPY form is a literal or a factory:
//! array literals, the Workshop setting values, event-player keywords and
//! `exclude` (opy-rs#744).

use std::path::Path;

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use workshop_rs::catalog::{Catalog, Locale};

const SOURCE: &str = r#"
globalvar A
globalvar B
globalvar C
globalvar D
globalvar E
globalvar F

rule "values":
    @Event global
    A = [1, 2, 3]
    B = [[1, 2], [A, B]]
    C = createWorkshopSettingBool("Category", "Flag", true, 1)
    D = createWorkshopSettingInt("Category", "Count", 5, 0, 10, 2)
    E = createWorkshopSettingEnum("Category", "Mode", 1, ["first", "second"], 3)
    F = createWorkshopSettingHero("Category", "Pick", Hero.ANA, 4)
    A = createWorkshopSettingFloat("", "Scale", 1.5, 0.5, 2.5)
"#;

const EVENT_SOURCE: &str = r#"
globalvar A
globalvar B
globalvar C

rule "event players":
    @Event playerTookDamage
    A = attacker
    B = victim
    C = [1, 2, 3].exclude(B)
    C = [4, 5].exclude(2)
"#;

fn round_trip(language: &str, source: &str, expect: &[&str]) {
    let compiler = Compiler::new().expect("compiler loads");
    let catalog = Catalog::builtin().expect("catalog loads");
    let locale = Locale::new(language);
    let compile = |source: &str, path: &str| {
        compiler
            .compile_source_with_locale(source, path, Path::new("."), &locale)
            .unwrap_or_else(|e| panic!("{path} does not compile: {e:?}\n{source}"))
    };

    let original = compile(source, "values.opy");
    let program = workshop_rs::parser::parse(&original.emitted, &catalog, &locale)
        .expect("compiled Workshop text parses");
    let opy = reconstruct(&program).unwrap_or_else(|e| panic!("{language}: refused: {e}"));
    for needle in expect {
        assert!(opy.contains(needle), "missing {needle}\n{opy}");
    }

    let recompiled = compile(&opy, "reconstructed.opy");
    let reparsed = workshop_rs::parser::parse(&recompiled.emitted, &catalog, &locale)
        .expect("recompiled Workshop text parses");
    assert!(
        workshop_rs::compare::equivalent(&program, &reparsed),
        "{language}: {:?}\n{opy}",
        workshop_rs::compare::difference(&program, &reparsed)
    );
}

const VALUE_EXPECT: &[&str] = &[
    "[1, 2, 3]",
    "createWorkshopSettingBool(",
    "createWorkshopSettingInt(",
    "createWorkshopSettingEnum(",
];
const EVENT_EXPECT: &[&str] = &["= attacker", "= victim", ".exclude("];

#[test]
fn array_and_setting_value_calls_round_trip_in_en_us() {
    round_trip("en-US", SOURCE, VALUE_EXPECT);
}

#[test]
fn array_and_setting_value_calls_round_trip_in_zh_cn() {
    round_trip("zh-CN", SOURCE, VALUE_EXPECT);
}

#[test]
fn event_player_and_exclude_value_calls_round_trip_in_en_us() {
    round_trip("en-US", EVENT_SOURCE, EVENT_EXPECT);
}

#[test]
fn event_player_and_exclude_value_calls_round_trip_in_zh_cn() {
    round_trip("zh-CN", EVENT_SOURCE, EVENT_EXPECT);
}

// The Set form of a self-referential `removeFromArray` is not the Modify form,
// and `g = g.exclude(v)` recompiles to the Modify form, so reconstruction
// refuses it by construct name.
const SELF_EXCLUDE_TEXT: &str = r#"variables {
    global:
        0: g
    player:
        0: p
}

rule ("self exclude") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(g, Remove From Array(Global Variable(g), Victim));
        Set Player Variable(Event Player, p, Remove From Array(Player Variable(Event Player, p), Victim));
        Set Global Variable At Index(g, 1, Remove From Array(Value In Array(Global Variable(g), 1), 5));
        Set Player Variable At Index(Event Player, p, 1, Remove From Array(Value In Array(Player Variable(Event Player, p), 1), 5));
    }
}
"#;

fn self_exclude_refusal(language: &str) -> String {
    let catalog = Catalog::builtin().expect("catalog loads");
    let program =
        workshop_rs::parser::parse(SELF_EXCLUDE_TEXT, &catalog, &Locale::new("en-US")).unwrap();
    let locale = Locale::new(language);
    let text = workshop_rs::emitter::emit(&program, &catalog, &locale).expect("text emits");
    let program = workshop_rs::parser::parse(&text, &catalog, &locale).expect("text parses");
    reconstruct(&program)
        .expect_err("self-referential set is refused")
        .to_string()
}

#[test]
fn self_referential_remove_from_array_set_is_refused_in_en_us() {
    let message = self_exclude_refusal("en-US");
    assert_eq!(
        message.matches("unsupported-value-call").count(),
        4,
        "{message}"
    );
    assert!(message.contains("removeFromArray"), "{message}");
}

#[test]
fn self_referential_remove_from_array_set_is_refused_in_zh_cn() {
    let message = self_exclude_refusal("zh-CN");
    assert_eq!(
        message.matches("unsupported-value-call").count(),
        4,
        "{message}"
    );
    assert!(message.contains("removeFromArray"), "{message}");
}
