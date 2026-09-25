//! Structural convergence with the pinned OverPy output.
//!
//! Each fixture pairs OPY source with the pinned oracle snapshot. Native output
//! must parse to a canonical Workshop program structurally equal to it.

use std::path::Path;

use crate::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

/// Everything the program says except where the text put it.
fn structure(program: &workshop_rs::Program) -> String {
    format!(
        "{:#?}{:#?}{:#?}{:#?}{:#?}",
        program.settings,
        program.global_variables,
        program.player_variables,
        program.subroutines,
        program.rules
    )
    .lines()
    .filter(|line| {
        let line = line.trim_start();
        !line.starts_with("line: ") && !line.starts_with("col: ")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

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
        structure(&native),
        structure(&expected),
        "{name} differs structurally from the pinned oracle"
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
fn goto_distance_excludes_instructions_the_output_drops() {
    assert_converges("structural-goto-dropped");
}

#[test]
fn rounding_bare_returns_and_map_comparisons_match_the_pinned_oracle() {
    assert_converges("structural-reference-quirks");
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

#[test]
fn empty_rules_and_subroutines_are_dropped_like_the_pinned_oracle() {
    assert_converges("structural-empty-rules");
}

#[test]
fn builtin_constant_folding_matches_the_pinned_oracle() {
    assert_converges("structural-builtin-folding");
}

#[test]
fn folds_on_the_authored_literal_match_the_pinned_oracle() {
    assert_converges("structural-authored-literals");
}

#[test]
fn action_rewrites_match_the_pinned_oracle() {
    assert_converges("structural-action-rewrites");
}

#[test]
fn size_optimization_action_rewrites_match_the_pinned_oracle() {
    assert_converges("structural-size-rewrites");
}

/// Approved exceptions (workshop-rs ADR-0014, wrightkit/opy-rs#372): the
/// pinned OverPy writes these programs, native compilation rejects them.
fn assert_rejected(source: &str, needle: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hir = crate::compile(source, "source.opy", dir).expect("source must resolve");
    let error = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .err()
        .expect("the exception must stay rejected");
    assert!(error.to_string().contains(needle), "{error}");
}

#[test]
fn an_array_for_an_object_text_parameter_stays_rejected() {
    assert_rejected(
        "rule \"x\":\n    @Event global\n    printLog([])\n",
        "semantic type 'Object'",
    );
}

#[test]
fn a_vector_that_folds_to_zero_in_a_vector_position_stays_rejected() {
    assert_rejected(
        "globalvar v\nrule \"x\":\n    @Event global\n    createEffect(getAllPlayers(), Effect.SPHERE, Color.RED, v - v, 1, EffectReeval.VISIBILITY)\n",
        "semantic type 'Vector|Player'",
    );
}

#[test]
fn an_omitted_optional_argument_does_not_shift_the_rest() {
    // The reference moves the remaining arguments left and writes a beam whose
    // colour is `None`; native compilation rejects the mistyped call.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = "rule \"x\":\n    @Event eachPlayer\n    createBeam(eventPlayer, Beam.GRAPPLE, Vector.UP, Vector.UP, EffectReeval.NONE)\n";
    match crate::compile(source, "source.opy", dir) {
        Ok(_) => panic!("the mistyped call must stay rejected"),
        Err(error) => assert_eq!(error.code, "missing-argument"),
    }
}
