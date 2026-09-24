//! Structural convergence with the pinned OverPy output.
//!
//! Each fixture pairs OPY source with the pinned oracle snapshot. Native output
//! must parse to a canonical Workshop program structurally equal to it.

use std::path::Path;

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn assert_converges(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic")
        .join(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR");
    let oracle: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("oracle.json")).expect("oracle.json must be readable"),
    )
    .expect("oracle.json must parse");
    let catalog = Catalog::builtin().expect("catalog must load");
    let locale = Locale::new("en-US");
    let parse = |text: &str| {
        workshop_rs::parser::parse(text, &catalog, &locale).expect("Workshop text must parse")
    };
    let expected = parse(
        oracle["compile"]["workshop"]
            .as_str()
            .expect("oracle snapshot records the compiled Workshop text"),
    );
    let native = parse(&artifact.emitted);
    assert_eq!(
        format!(
            "{:#?}{:#?}{:#?}",
            native.rules, native.global_variables, native.player_variables
        ),
        format!(
            "{:#?}{:#?}{:#?}",
            expected.rules, expected.global_variables, expected.player_variables
        ),
        "{name} rules differ structurally from the pinned oracle"
    );
    assert!(
        equivalent(&native, &expected),
        "{name} diverged from the pinned oracle:\n{}",
        artifact.emitted
    );
}

#[test]
fn size_optimization_literals_match_the_pinned_oracle() {
    assert_converges("structural-size-literals");
}

#[test]
fn operator_optimization_matches_the_pinned_oracle() {
    assert_converges("structural-operators");
}

#[test]
fn declarations_names_and_arrays_match_the_pinned_oracle() {
    assert_converges("structural-declarations");
}

#[test]
fn terminal_conditionals_match_the_pinned_oracle() {
    assert_converges("structural-control-flow");
}

#[test]
fn else_after_a_nested_conditional_belongs_to_the_outer_conditional() {
    assert_converges("structural-nested-else");
}

#[test]
fn goto_out_of_a_loop_stays_a_skip() {
    assert_converges("structural-loop-exit");
}

#[test]
fn skip_over_nothing_is_a_disabled_abort() {
    assert_converges("structural-zero-skip");
}

#[test]
fn custom_string_merging_and_splitting_match_the_pinned_oracle() {
    assert_converges("structural-strings");
}

#[test]
fn array_and_assignment_rewrites_match_the_pinned_oracle() {
    assert_converges("structural-arrays");
}

#[test]
fn bugged_map_handling_matches_the_pinned_oracle() {
    assert_converges("structural-maps");
}

#[test]
fn builtin_defaults_and_size_arguments_match_the_pinned_oracle() {
    assert_converges("structural-builtins");
}

#[test]
fn compression_matches_the_pinned_oracle() {
    assert_converges("structural-compression");
}

#[test]
fn team_settings_order_matches_the_pinned_oracle() {
    assert_converges("structural-settings");
}

#[test]
fn unoptimized_output_matches_the_pinned_oracle() {
    assert_converges("structural-unoptimized");
}
