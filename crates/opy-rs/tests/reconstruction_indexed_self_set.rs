use std::path::Path;

use opy_rs::Compiler;
use opy_rs::reconstruct::reconstruct;
use workshop_rs::Program;
use workshop_rs::catalog::{Catalog, Locale};

const LANGUAGES: [&str; 2] = ["en-US", "zh-CN"];

fn workshop_program(actions: &str, locale: &Locale) -> Program {
    let catalog = Catalog::builtin_ref().expect("catalog loads");
    let english = Locale::new("en-US");
    let text = format!(
        "variables {{\n    global:\n        0: g\n        1: h\n        2: i\n    player:\n        0: p\n}}\n\
         rule (\"indexed sets\") {{\n\
             event {{ Player Took Damage; All; All; }}\n\
             actions {{ {actions} }}\n\
         }}"
    );
    let program = workshop_rs::parser::parse(&text, catalog, &english)
        .unwrap_or_else(|error| panic!("{error:?}\n{text}"));
    if locale == &english {
        return program;
    }
    let localized = workshop_rs::emitter::emit(&program, catalog, locale).expect("text emits");
    workshop_rs::parser::parse(&localized, catalog, locale).expect("localized text parses")
}

fn assert_refused(action: &str, set_id: &str, operation_id: &str) {
    for language in LANGUAGES {
        let program = workshop_program(action, &Locale::new(language));
        let error =
            reconstruct(&program).expect_err("indexed self-referential Set must be refused");
        let [issue] = error.issues.as_slice() else {
            panic!("{language}: expected one refusal for {action}: {error}");
        };
        assert_eq!(issue.code, "unsupported-value-call", "{language}: {error}");
        assert!(
            issue.message.contains(&format!("'{set_id}'")),
            "{language}: {error}"
        );
        assert!(
            issue.message.contains(&format!("'{operation_id}'")),
            "{language}: {error}"
        );
        let action_span = program.action_span(0, 0).expect("parsed action has a span");
        assert_eq!(issue.span, Some(action_span), "{language}: {error}");
    }
}

#[test]
fn indexed_sets_that_would_become_modify_are_refused() {
    for (operation, id) in [
        ("Add", "add"),
        ("Subtract", "subtract"),
        ("Multiply", "multiply"),
        ("Divide", "divide"),
        ("Modulo", "modulo"),
        ("Raise To Power", "raiseToPower"),
        ("Min", "min"),
        ("Max", "max"),
        ("Append To Array", "appendToArray"),
    ] {
        assert_refused(
            &format!(
                "Set Global Variable At Index(g, 1, {operation}(Value In Array(Global.g, 1), 5));"
            ),
            "setGlobalVariableAtIndex",
            id,
        );
        assert_refused(
            &format!(
                "Set Player Variable At Index(Event Player, p, 1, {operation}(Value In Array(Player Variable(Event Player, p), 1), 5));"
            ),
            "setPlayerVariableAtIndex",
            id,
        );
    }
}

#[test]
fn first_of_and_expression_indexes_are_checked_structurally() {
    assert_refused(
        "Set Global Variable At Index(g, 0, Add(First Of(Global.g), 5));",
        "setGlobalVariableAtIndex",
        "add",
    );
    assert_refused(
        "Set Player Variable At Index(Attacker, p, 0, Add(First Of(Player Variable(Attacker, p)), 5));",
        "setPlayerVariableAtIndex",
        "add",
    );
    assert_refused(
        "Set Player Variable At Index(Attacker, p, Multiply(Add(Global.h, Global.i), 2), \
         Add(Value In Array(Player Variable(Attacker, p), \
         Multiply(Add(Global.h, Global.i), 2)), 5));",
        "setPlayerVariableAtIndex",
        "add",
    );
}

#[test]
fn indexed_sets_with_different_read_targets_and_existing_modifies_round_trip() {
    let actions = "Set Global Variable At Index(g, 1, Add(Value In Array(Global.h, 1), 5));\n\
        Set Global Variable At Index(g, Multiply(Add(Global.h, Global.i), 2), \
        Add(Value In Array(Global.g, Add(Global.h, Multiply(Global.i, 2))), 5));\n\
        Set Player Variable At Index(Event Player, p, 1, \
        Add(Value In Array(Player Variable(Event Player, p), 2), 5));\n\
        Set Player Variable At Index(Event Player, p, 1, \
        Add(Value In Array(Player Variable(Attacker, p), 1), 5));\n\
        Modify Global Variable At Index(g, 1, Append To Array, 5);\n\
        Modify Player Variable At Index(Event Player, p, 1, Add, 5);";
    let catalog = Catalog::builtin_ref().expect("catalog loads");
    let compiler = Compiler::new().expect("compiler loads");
    for language in LANGUAGES {
        let locale = Locale::new(language);
        let program = workshop_program(actions, &locale);
        let opy = reconstruct(&program).unwrap_or_else(|error| panic!("{language}: {error}"));
        let recompiled = compiler
            .compile_source_with_locale(&opy, "indexed.opy", Path::new("."), &locale)
            .unwrap_or_else(|error| panic!("{language}: {error:?}\n{opy}"));
        let reparsed = workshop_rs::parser::parse(&recompiled.emitted, catalog, &locale)
            .expect("recompiled text parses");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{language}: {:?}\n{opy}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }
}
