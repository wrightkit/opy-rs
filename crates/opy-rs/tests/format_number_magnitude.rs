use std::path::Path;
use std::process::Command;

use opy_rs::Compiler;
use workshop_rs::catalog::{Catalog, Locale};

const MAGNITUDE_CASES: [(&str, &str); 2] = [
    (
        "default",
        include_str!("fixtures/corpus/synthetic/format-number-magnitude/source.opy"),
    ),
    (
        "optimizeForSize",
        include_str!("fixtures/corpus/synthetic/format-number-magnitude-size/source.opy"),
    ),
];

const EXPONENT_SOURCE: &str = "globalvar result\nrule \"exponent\":\n    @Event global\n    debug(1e20)\n    result = \"{0}\".format(1e20)\n";

#[test]
fn exponent_arguments_remain_in_emitted_strings() {
    let compiler = Compiler::new().unwrap();
    for directive in ["", "#!optimizeForSize\n", "#!disableOptimizations\n"] {
        let source = format!("{directive}{EXPONENT_SOURCE}");
        for language in ["en-US", "zh-CN"] {
            let output = compiler
                .compile_source_with_locale(
                    &source,
                    "source.opy",
                    Path::new("."),
                    &Locale::new(language),
                )
                .unwrap_or_else(|error| panic!("{directive} {language}: {error}"))
                .emitted;
            assert!(output.contains("= {0}"), "{directive} {language}: {output}");
            assert!(
                output.contains("\"{0}\", 100000000000000000000)"),
                "{directive} {language}: {output}"
            );
            assert_eq!(
                output.matches(", 100000000000000000000").count(),
                2,
                "{directive} {language}: {output}"
            );
            assert!(
                !output.contains("9223372036854775807"),
                "{directive} {language}: {output}"
            );
        }
    }
}

#[test]
#[ignore = "requires the pinned OverPy install in tools/overpy/oracle"]
fn number_magnitudes_match_pinned_overpy_in_both_locales() {
    let compiler = Compiler::new().unwrap();
    let catalog = Catalog::builtin_ref().unwrap();
    for (mode, source) in MAGNITUDE_CASES {
        for language in ["en-US", "zh-CN"] {
            let reference = compile_with_pinned_overpy(source, language);
            let locale = Locale::new(language);
            let output = compiler
                .compile_source_with_locale(source, "source.opy", Path::new("."), &locale)
                .unwrap_or_else(|error| panic!("{mode} {language}: {error}"))
                .emitted;
            let parse = |text| workshop_rs::parser::parse(text, catalog, &locale).unwrap();
            let native = parse(&output);
            let reference = parse(&reference);
            assert!(
                workshop_rs::compare::equivalent(&native, &reference),
                "{mode} {language}: {:?}",
                workshop_rs::compare::difference(&native, &reference)
            );
        }
    }
    for directive in ["", "#!optimizeForSize\n", "#!disableOptimizations\n"] {
        let source = format!("{directive}{EXPONENT_SOURCE}");
        for language in ["en-US", "zh-CN"] {
            let reference = compile_with_pinned_overpy(&source, language);
            assert!(
                reference.contains("= {0}"),
                "{directive} {language}: {reference}"
            );
            assert!(
                reference.contains("\"{0}\", 1e20)"),
                "{directive} {language}: {reference}"
            );
            assert_eq!(
                reference.matches(", 1e20").count(),
                2,
                "{directive} {language}: {reference}"
            );
            assert!(
                !reference.contains("9223372036854775807"),
                "{directive} {language}: {reference}"
            );
        }
    }
}

fn compile_with_pinned_overpy(source: &str, language: &str) -> String {
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
