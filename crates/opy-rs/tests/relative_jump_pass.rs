use std::path::{Path, PathBuf};
use std::process::Command;

use opy_rs::Compiler;
use workshop_rs::catalog::{Catalog, Locale};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

fn compare(name: &str, language: &str, reference: &str) {
    let dir = fixture(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let locale = Locale::new(language);
    let native = Compiler::new()
        .unwrap()
        .compile_source_with_locale(&source, "source.opy", &dir, &locale)
        .unwrap()
        .emitted;
    let reference = reference
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let catalog = Catalog::builtin_ref().unwrap();
    let expected = workshop_rs::parser::parse(&reference, catalog, &locale).unwrap();
    let actual = workshop_rs::parser::parse(&native, catalog, &locale).unwrap();
    assert!(
        workshop_rs::compare::equivalent(&actual, &expected),
        "{name}, {language}: {:?}\nnative:\n{native}\nreference:\n{reference}",
        workshop_rs::compare::difference(&actual, &expected)
    );
}

fn compare_snapshot(name: &str) {
    let snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture(name).join("oracle.json")).unwrap()).unwrap();
    compare(
        name,
        "en-US",
        snapshot["compile"]["workshop"].as_str().unwrap(),
    );
}

#[test]
fn relative_jump_pass_default() {
    compare_snapshot("relative-jump-pass");
}

#[test]
fn relative_jump_pass_unoptimized() {
    compare_snapshot("relative-jump-pass-unoptimized");
}

#[test]
fn relative_jump_pass_size() {
    compare_snapshot("relative-jump-pass-size");
}

#[test]
fn relative_jump_pass_aggressive_size() {
    compare_snapshot("relative-jump-pass-aggressive");
}

#[test]
fn relative_jump_pass_expansion() {
    compare_snapshot("relative-jump-expansion");
}

#[test]
fn relative_skip_pass_default() {
    compare_snapshot("relative-skip-pass");
}

#[test]
fn preserved_pass_keeps_its_source_location() {
    let source =
        "globalvar a\nrule \"r\":\n    @Event global\n    goto loc+1\n    pass\n    a = 1\n";
    let (report, mapped) = Compiler::new()
        .unwrap()
        .compile_source_report_mapped_with_language(source, "source.opy", Path::new("."), "en-US");
    assert_eq!(report.compile.status, opy_rs::CompileStatus::Success);
    let mapped = mapped.unwrap();
    let catalog = Catalog::builtin_ref().unwrap();
    let mut program =
        workshop_rs::parser::parse(&mapped.text, catalog, &Locale::new("en-US")).unwrap();
    mapped.map.apply(&mut program).unwrap();
    assert!(matches!(
        program.rules[0].actions[1],
        workshop_rs::Action::Disabled { .. }
    ));
    assert_eq!(
        program.action_span(0, 1).unwrap().start,
        workshop_rs::source::Position::new(5, 5)
    );
}

#[test]
#[ignore = "requires the pinned OverPy install in tools/overpy/oracle"]
fn relative_jump_pass_matches_pinned_overpy_in_both_locales() {
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/overpy/oracle/node_modules/overpy");
    for name in [
        "relative-jump-pass",
        "relative-jump-pass-unoptimized",
        "relative-jump-pass-size",
        "relative-jump-pass-aggressive",
        "relative-jump-expansion",
        "relative-skip-pass",
    ] {
        let dir = fixture(name);
        for language in ["en-US", "zh-CN"] {
            let output = Command::new("node")
                .arg("-e")
                .arg(
                    r#"
const [path, language, source] = process.argv.slice(1);
const oracle = require(path);
oracle.readyPromise.then(async () => {
    process.stdout.write((await oracle.compile(require('fs').readFileSync(source, 'utf8'), language)).result);
}).catch(error => { console.error(error); process.exitCode = 1; });
"#,
                )
                .arg(&oracle)
                .arg(language)
                .arg(dir.join("source.opy"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            compare(name, language, &String::from_utf8(output.stdout).unwrap());
        }
    }
}
