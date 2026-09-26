//! Indexed assignment lowering and catalog numeric coercion coverage.

use std::path::{Path, PathBuf};

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name)
}

#[test]
fn nested_indexed_assignments_match_the_pinned_oracle() {
    let dir = fixture_dir("indexed-assignment-nested");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let oracle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("oracle.json")).unwrap()).unwrap();
    let expected = workshop_rs::parser::parse(
        oracle["compile"]["workshop"].as_str().unwrap(),
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(equivalent(&super::canonical_program(&artifact), &expected));
}

#[test]
fn collection_deletion_matches_the_pinned_oracle() {
    let dir = fixture_dir("collection-mutation-328");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let oracle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("oracle.json")).unwrap()).unwrap();
    let expected = workshop_rs::parser::parse(
        oracle["compile"]["workshop"].as_str().unwrap(),
        &Catalog::builtin().unwrap(),
        &Locale::new("en-US"),
    )
    .unwrap();
    assert!(equivalent(&super::canonical_program(&artifact), &expected));
}

#[test]
fn nested_random_delete_reaches_the_pinned_boundary() {
    let dir = fixture_dir("collection-mutation-328-random-invalid");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let report = Compiler::new()
        .unwrap()
        .compile_source_report(&source, "source.opy", &dir);
    assert_eq!(report.compile.status, crate::CompileStatus::Failure);
    assert_eq!(report.compile.diagnostics[0].code, "random-indexed-delete");
}

#[test]
fn nested_random_player_receiver_delete_reaches_the_pinned_boundary() {
    let dir = fixture_dir("collection-mutation-328-random-player-invalid");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let report = Compiler::new()
        .unwrap()
        .compile_source_report(&source, "source.opy", &dir);
    assert_eq!(report.compile.status, crate::CompileStatus::Failure);
    assert_eq!(report.compile.diagnostics[0].code, "random-indexed-delete");
}

#[test]
fn nested_player_collection_deletion_uses_the_canonical_indexed_action() {
    let source = r#"
playervar values
globalvar index
rule "delete nested player value":
    @Event eachPlayer
    del eventPlayer.values[index][0]
"#;
    let hir = crate::compile(source, "nested-player-delete.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(artifact.emitted.contains(
        "Modify Player Variable At Index((Event Player).values, index, Remove From Array By Index, 0);"
    ));
}

#[test]
fn indexed_assignment_indices_keep_the_authored_boolean_spelling() {
    let source = r#"
globalvar values = [0]
globalvar nested = [[0]]

rule "boolean indices":
    @Event global
    values[true] = 1
    values[false] += 1
    nested[true][false] = 2
"#;
    let hir = crate::compile(source, "indexed-assignment-indices.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable At Index(values, True, 1);")
    );
    assert!(
        artifact
            .emitted
            .contains("Modify Global Variable At Index(values, False, Add, 1);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable At Index(nested, True,")
    );
}

#[test]
fn indexed_receiver_mutations_update_only_the_selected_variable_slot() {
    let source = r#"
globalvar values
playervar slots

rule "indexed receiver mutations":
    @Event eachPlayer
    values[eventPlayer.getSlot()].append(getCurrentMap())
    eventPlayer.slots[eventPlayer.getSlot()].remove(eventPlayer.getHero())
"#;
    let hir = crate::compile(source, "indexed-receiver-mutations.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(artifact.emitted.contains(
        "Modify Global Variable At Index(values, Slot Of(Event Player), Append To Array, Current Map);"
    ));
    assert!(artifact.emitted.contains(
        "Modify Player Variable At Index((Event Player).slots, Slot Of(Event Player), Remove From Array, Hero Of(Event Player));"
    ));
    assert_eq!(artifact.emitted.matches("Slot Of(Event Player)").count(), 2);

    assert_eq!(artifact.wir.action_span(0, 0).unwrap().start.line, 7);
    assert_eq!(artifact.wir.action_span(0, 1).unwrap().start.line, 8);
    for (action, line) in [(0, 7), (1, 8)] {
        for argument in [0, 1, 3] {
            assert_eq!(
                artifact
                    .wir
                    .action_argument_span(0, action, argument)
                    .unwrap()
                    .start
                    .line,
                line
            );
        }
        assert!(artifact.wir.action_argument_span(0, action, 2).is_none());
    }
}
