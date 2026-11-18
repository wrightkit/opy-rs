//! Round trip for value calls whose OPY form is a literal or a factory:
//! array literals and the Workshop setting values (opy-rs#744).

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

fn round_trip(language: &str) {
    let compiler = Compiler::new().expect("compiler loads");
    let catalog = Catalog::builtin().expect("catalog loads");
    let locale = Locale::new(language);
    let compile = |source: &str, path: &str| {
        compiler
            .compile_source_with_locale(source, path, Path::new("."), &locale)
            .unwrap_or_else(|e| panic!("{path} does not compile: {e:?}\n{source}"))
    };

    let original = compile(SOURCE, "values.opy");
    let program = workshop_rs::parser::parse(&original.emitted, &catalog, &locale)
        .expect("compiled Workshop text parses");
    let opy = reconstruct(&program).unwrap_or_else(|e| panic!("{language}: refused: {e}"));
    assert!(opy.contains("[1, 2, 3]"), "{opy}");
    assert!(opy.contains("createWorkshopSettingBool("), "{opy}");
    assert!(opy.contains("createWorkshopSettingInt("), "{opy}");
    assert!(opy.contains("createWorkshopSettingEnum("), "{opy}");

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
fn array_and_setting_value_calls_round_trip_in_en_us() {
    round_trip("en-US");
}

#[test]
fn array_and_setting_value_calls_round_trip_in_zh_cn() {
    round_trip("zh-CN");
}
