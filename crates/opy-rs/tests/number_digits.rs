//! Folded numbers keep the digits the pinned OverPy 9.7.17 writes (#779).

use std::path::Path;
use std::process::Command;

use opy_rs::Compiler;
use workshop_rs::catalog::{Catalog, Locale};

const LITERALS: [&str; 4] = [
    "100.01666666666667",
    "100/3",
    "-100.01666666666667",
    "99.99999999999999",
];

// (argument, minute-slice number, second-slice number) as written by the oracle.
const TIME_TO_STRING: [(&str, &str); 6] = [
    ("61", "101.01666666666667"),
    ("0.5", "100.00833333333334"),
    ("59.999", "100.99998333333333"),
    ("100000000", "146.66666666666666"),
    ("86399.999", "159.99998333333326"),
    ("3757.65", "102.6275"),
];

fn source(expression: &str) -> String {
    format!("globalvar a\nrule \"t\":\n    @Event global\n    a = {expression}\n")
}

fn compile(source: &str, language: &str) -> String {
    Compiler::new()
        .unwrap()
        .compile_source_with_locale(source, "source.opy", Path::new("."), &Locale::new(language))
        .unwrap_or_else(|error| panic!("{language}: {error}"))
        .emitted
}

#[test]
fn constant_time_to_string_keeps_the_oracle_digits() {
    // Authority: the oracle output recorded in the Issue (`String Slice(Custom
    // String("{0}", 101.01666666666667), 1, 2)`).
    for language in ["en-US", "zh-CN"] {
        for (argument, number) in TIME_TO_STRING {
            let output = compile(&source(&format!("timeToString({argument})")), language);
            assert!(
                output.contains(&format!("\"{{0}}\", {number})")),
                "{language} {argument}: {output}"
            );
        }
    }
}

#[test]
fn plain_literals_keep_the_oracle_digits() {
    for language in ["en-US", "zh-CN"] {
        let output = compile(&source("100.01666666666667"), language);
        assert!(
            output.contains("100.01666666666667"),
            "{language}: {output}"
        );
    }
}

#[test]
#[ignore = "requires the pinned OverPy install in tools/overpy/oracle"]
fn folded_numbers_match_pinned_overpy_in_both_locales() {
    let catalog = Catalog::builtin_ref().unwrap();
    for language in ["en-US", "zh-CN"] {
        let locale = Locale::new(language);
        let parse = |text: &str| workshop_rs::parser::parse(text, catalog, &locale).unwrap();
        for literal in LITERALS {
            let source = source(literal);
            let (native, reference) = (compile(&source, language), oracle(&source, language));
            let (native, reference) = (parse(&native), parse(&reference));
            assert!(
                workshop_rs::compare::equivalent(&native, &reference),
                "{language} {literal}: {:?}",
                workshop_rs::compare::difference(&native, &reference)
            );
        }
        // `timeToString` differs from the oracle by the approved canonical-type
        // wrapping of `String Slice`, so the folded numbers are compared as text.
        for (argument, _) in TIME_TO_STRING {
            let source = source(&format!("timeToString({argument})"));
            let native = compile(&source, language);
            let reference = oracle(&source, language);
            for token in numbers(&reference) {
                assert!(
                    native.contains(&token),
                    "{language} {argument}: {token} missing\nnative: {native}\noracle: {reference}"
                );
            }
        }
    }
}

fn numbers(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter(|token| token.contains('.') && token.len() > 8)
        .map(str::to_owned)
        .collect()
}

fn oracle(source: &str, language: &str) -> String {
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/overpy/oracle/node_modules/overpy");
    let output = Command::new("node")
        .arg("-e")
        .arg(
            r#"
const [path, language, source] = process.argv.slice(1);
const oracle = require(path);
oracle.readyPromise.then(async () => {
    process.stdout.write((await oracle.compile(source, language)).result);
}).catch(error => {
    console.error(error);
    process.exitCode = 1;
});
"#,
        )
        .arg(oracle)
        .args([language, source])
        .output()
        .expect("node must be available");
    assert!(
        output.status.success(),
        "{language}: pinned OverPy failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("OverPy output is UTF-8")
}
