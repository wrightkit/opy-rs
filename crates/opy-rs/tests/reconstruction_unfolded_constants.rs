use std::path::Path;
use std::process::Command;

use opy_rs::Compiler;
use opy_rs::manifest::Manifest;
use opy_rs::reconstruct::reconstruct_with;
use workshop_rs::catalog::{Catalog, Locale};

const CASES: [(&str, &str); 2] = [
    (
        "en-US",
        include_str!("fixtures/reconstruction/unfolded_constants/en-US.ow"),
    ),
    (
        "zh-CN",
        include_str!("fixtures/reconstruction/unfolded_constants/zh-CN.ow"),
    ),
];

fn compare_reconstruction(
    language: &str,
    text: &str,
    compile: impl FnOnce(&str, &Locale) -> String,
) {
    let catalog = Catalog::builtin_ref().unwrap();
    let locale = Locale::new(language);
    let input = workshop_rs::parser::parse(text, catalog, &locale)
        .unwrap_or_else(|error| panic!("{language}: fixture does not parse: {error:?}"));
    let opy = reconstruct_with(&input, Manifest::builtin().unwrap(), catalog, &locale)
        .unwrap_or_else(|error| panic!("{language}: reconstruction refused: {error}"));
    let output = compile(&opy, &locale);
    let reparsed = workshop_rs::parser::parse(&output, catalog, &locale).unwrap_or_else(|error| {
        panic!("{language}: invalid recompiled Workshop: {error:?}\n{output}")
    });
    assert!(
        workshop_rs::compare::equivalent(&input, &reparsed),
        "{language}: {:?}\n{opy}",
        workshop_rs::compare::difference(&input, &reparsed)
    );
}

fn compile_native(opy: &str, locale: &Locale) -> String {
    Compiler::new()
        .unwrap()
        .compile_source_with_locale(opy, "reconstructed.opy", Path::new("."), locale)
        .unwrap_or_else(|error| panic!("{error:?}\n{opy}"))
        .emitted
}

#[test]
fn unfolded_constants_round_trip_en_us() {
    compare_reconstruction(CASES[0].0, CASES[0].1, compile_native);
}

#[test]
fn unfolded_constants_round_trip_zh_cn() {
    compare_reconstruction(CASES[1].0, CASES[1].1, compile_native);
}

#[test]
fn literal_and_runtime_indices_keep_the_existing_source() {
    let text = r#"variables {
    global:
        0: A
        1: x
}
rule ("indices") {
    event { Ongoing - Global; }
    actions {
        Set Global Variable At Index(A, 1, Global.x);
        Set Global Variable At Index(A, Add(Global.x, 1), Global.x);
    }
}"#;
    let expected = "#!optimizeStrict\n\nglobalvar A 0\nglobalvar x 1\n\n\
                    rule \"indices\":\n    @Event global\n    A[1] = x\n    A[(x + 1)] = x\n\n";
    compare_reconstruction("en-US", text, |opy, locale| {
        assert_eq!(opy, expected);
        compile_native(opy, locale)
    });
}

#[test]
fn refuses_actions_where_disabling_optimization_changes_the_shape() {
    let text = r#"variables {
    global:
        0: A
}
rule ("incompatible action forms") {
    event { Ongoing - Global; }
    actions {
        Set Global Variable At Index(A, Add(0, 1), First Of(Global.A));
        Modify Global Variable At Index(A, Add(0, 1), Add, 2);
    }
}"#;
    let catalog = Catalog::builtin_ref().unwrap();
    let locale = Locale::new("en-US");
    let program = workshop_rs::parser::parse(text, catalog, &locale).unwrap();
    let error = reconstruct_with(&program, Manifest::builtin().unwrap(), catalog, &locale)
        .expect_err("both actions need optimized and unoptimized lowering in the same statement");
    assert_eq!(error.issues.len(), 2, "{error}");
    for (index, (issue, name)) in error
        .issues
        .iter()
        .zip(["setGlobalVariableAtIndex", "modifyGlobalVariableAtIndex"])
        .enumerate()
    {
        assert_eq!(issue.code, "unsupported-value-call");
        assert!(issue.message.contains(name), "{error}");
        assert_eq!(issue.span.unwrap(), program.action_span(0, index).unwrap());
    }
}

#[test]
fn refuses_rewritten_conditions_and_loop_bounds_at_their_spans() {
    let catalog = Catalog::builtin_ref().unwrap();
    let english = Locale::new("en-US");
    let text = include_str!("fixtures/reconstruction/unfolded_constants/conditions.ow");
    let input = workshop_rs::parser::parse(text, catalog, &english).unwrap();
    for (language, _) in CASES {
        let locale = Locale::new(language);
        let localized = workshop_rs::emitter::emit(&input, catalog, &locale).unwrap();
        let program = workshop_rs::parser::parse(&localized, catalog, &locale).unwrap();
        let error = reconstruct_with(&program, Manifest::builtin().unwrap(), catalog, &locale)
            .expect_err("strict optimization would change the rule or a block header");
        let if_span = program.action_span(0, 0);
        let expected = [
            ("rule condition", program.condition_span(0, 0)),
            ("if", if_span),
            ("elseIf", program.action_span(0, 2).or(if_span)),
            ("while", program.action_span(0, 5)),
            ("forGlobalVariable", program.action_span(0, 8)),
        ];
        assert_eq!(error.issues.len(), expected.len(), "{language}: {error}");
        for (name, span) in expected {
            let span = span.expect("the parsed construct has a span");
            let issue = error
                .issues
                .iter()
                .find(|issue| issue.message.contains(&format!("'{name}'")))
                .unwrap_or_else(|| panic!("{language}: missing diagnostic for {name}: {error}"));
            assert_eq!(issue.code, "unsupported-value-call", "{language}: {error}");
            assert_eq!(issue.span, Some(span), "{language}: {error}");
        }
    }
}

#[test]
#[ignore = "requires the pinned OverPy install in tools/overpy/oracle"]
fn unfolded_constants_round_trip_with_pinned_overpy() {
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/overpy/oracle/node_modules/overpy");
    for (language, text) in CASES {
        compare_reconstruction(language, text, |opy, _| {
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
                .arg(&oracle)
                .args([language, opy])
                .output()
                .expect("node must be available");
            assert!(
                output.status.success(),
                "{language}: pinned OverPy rejected the reconstruction:\n{}\n{opy}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).expect("OverPy output is UTF-8")
        });
    }
}
