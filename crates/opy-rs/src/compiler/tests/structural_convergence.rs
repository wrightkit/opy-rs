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
fn omitted_arguments_fill_the_reference_defaults() {
    assert_converges("builtin-defaults");
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

#[test]
fn context_player_components_stay_rejected_under_canonical_validation() {
    // Pending workshop-rs admission evidence (ADR-0014): the pinned OverPy
    // emits `X Component Of(Event Player)`, but a bare Player has no
    // acceptance evidence for the component positions, so canonical
    // validation rejects them — a regression against the `eventPlayer.x`
    // behavior that compiled since #414, named as the consequence of
    // wrightkit/workshop-rs#336 option 1 and implemented by #350.
    // `context-player-component-validation` records the same gap for the
    // other context-player receivers.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic/context-player-components");
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    assert_rejected(&source, "semantic type 'Vector'");
}

#[test]
fn helper_macro_expansions_match_the_pinned_oracle() {
    assert_converges("structural-macro-expansions");
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
fn an_omitted_optional_argument_shifts_the_rest_but_stays_rejected() {
    // The reference moves the remaining arguments left and writes a beam whose
    // colour is `None`; canonical validation rejects the mistyped call until
    // the scoped admission contract lands (#392).
    assert_rejected(
        "rule \"x\":\n    @Event eachPlayer\n    createBeam(eventPlayer, Beam.GRAPPLE, Vector.UP, Vector.UP, EffectReeval.NONE)\n",
        "semantic type 'Color'",
    );
}

#[test]
fn a_bare_player_for_an_entity_parameter_stays_rejected() {
    // The reference writes `Destroy Effect(Event Player)` unchecked; the
    // canonical entity parameter requires EntityId and a bare Player has no
    // admission evidence, so the rejection stays pending workshop-rs
    // admission evidence and an owner call (probe gap
    // `validation-player-for-entity-id`).
    assert_rejected(
        "rule \"x\":\n    @Event eachPlayer\n    destroyEffect(eventPlayer)\n",
        "semantic type 'EntityId'",
    );
}

#[test]
fn a_non_finite_log_fold_keeps_the_pre_fold_expansion() {
    // The reference writes `-Infinity`/`NaN`, which the canonical grammar
    // cannot parse; pending wrightkit/workshop-rs#358 the call keeps emitting
    // the approximation it produced before folding was added.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = "globalvar v\nrule \"x\":\n    @Event global\n    v = log(0)\n    v = log(-1)\n    v = log(0, 10)\n    v = log(100, 0)\n";
    let hir = crate::compile(source, "source.opy", dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("the approximation must emit");
    assert!(
        artifact.emitted.contains(
            "Set Global Variable(v, Multiply(10000, Subtract(Raise To Power(0, 0.0001), 1)))"
        ),
        "{}",
        artifact.emitted
    );
    assert!(
        artifact
            .emitted
            .contains("Multiply(10000, Subtract(Raise To Power(-1, 0.0001), 1))"),
        "{}",
        artifact.emitted
    );
    // A non-finite operand keeps the whole call on the expansion it emitted
    // before folding, rather than a mix of folded and expanded operands.
    assert!(
        artifact.emitted.contains(
            "Divide(Multiply(10000, Subtract(Raise To Power(0, 0.0001), 1)), Multiply(10000, Subtract(Raise To Power(10, 0.0001), 1)))"
        ),
        "{}",
        artifact.emitted
    );
    // `log(100, 0)` still folds to the finite `0`, exactly as the reference.
    assert!(
        artifact.emitted.contains("Set Global Variable(v, 0);"),
        "{}",
        artifact.emitted
    );
}

#[test]
fn time_to_string_keeps_the_reference_shape_modulo_canonical_types() {
    // Approved exception (docs/architecture/language-core.md): the reference
    // writes `String Slice(Add(...), True, 2)` — a Number in the string
    // position and a Boolean in the start position, both rejected by
    // canonical validation — so the padded number goes through
    // `customString` and the start index stays `1`. Everything else,
    // including the unrounded minute division and the unevaluated slices on
    // a constant argument, matches the pinned oracle.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = "globalvar a\nglobalvar b\nrule \"x\":\n    @Event global\n    a = timeToString(getTotalTimeElapsed())\n    b = timeToString(3757.65)\n";
    let hir = crate::compile(source, "source.opy", dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("timeToString must emit");
    assert!(
        artifact.emitted.contains(
            "Custom String(\"{0}:{1}:{2}\", Round To Integer(Divide(Total Time Elapsed, 3600), Down), String Slice(Custom String(\"{0}\", Add(Divide(Modulo(Total Time Elapsed, 3600), 60), 100)), 1, 2), String Slice(Custom String(\"{0}\", Add(Modulo(Total Time Elapsed, 60), 100)), 1, 9999))"
        ),
        "{}",
        artifact.emitted
    );
    assert!(
        artifact.emitted.contains(
            "Custom String(\"1:{0}:{1}\", String Slice(Custom String(\"102.63\"), 1, 2), String Slice(Custom String(\"137.65\"), 1, 9999))"
        ),
        "{}",
        artifact.emitted
    );
}

#[test]
fn a_constant_substitution_optimizes_under_the_use_site_state() {
    // The reference substitutes a `macro` constant at the use site, so the
    // substituted expression optimizes under the caller's state: it folds
    // where the caller enables optimizations and stays unfolded where the
    // caller disables them, regardless of the definition site's state.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = "#!disableOptimizations\nmacro DISABLED_DEF = 2 + 3\n#!enableOptimizations\nmacro ENABLED_DEF = 4 + 5\nglobalvar v\nrule \"enabled\":\n    @Event global\n    v = DISABLED_DEF\n    v = ENABLED_DEF\n#!disableOptimizations\nrule \"disabled\":\n    @Event global\n    v = DISABLED_DEF\n    v = ENABLED_DEF\n";
    let hir = crate::compile(source, "source.opy", dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("the constants must emit");
    assert_eq!(
        artifact
            .emitted
            .matches("Set Global Variable(v, 5);")
            .count(),
        1,
        "{}",
        artifact.emitted
    );
    assert_eq!(
        artifact
            .emitted
            .matches("Set Global Variable(v, 9);")
            .count(),
        1,
        "{}",
        artifact.emitted
    );
    assert_eq!(
        artifact
            .emitted
            .matches("Set Global Variable(v, Add(2, 3));")
            .count(),
        1,
        "{}",
        artifact.emitted
    );
    assert_eq!(
        artifact
            .emitted
            .matches("Set Global Variable(v, Add(4, 5));")
            .count(),
        1,
        "{}",
        artifact.emitted
    );
}

const BUTTON_TO_STRING_EXPANSION: &str = "Mapped Array(Array(Input Binding String(Button(Jump))), Value In Array(String Split(Custom String(\"{0}(0.00, 1.00, 0.00)[{0}](0.00, 1.00, 0.00)[SHIFT](0.00, 1.00, 0.00)[CTRL](0.00, 1.00, 0.00)[ALT]\", Current Array Element), First Of(Up)), And(Compare(Modulo(String Length(Custom String(\"\\\\{0}{0}{0}{0}{0}{0}{0}\", Current Array Element)), 7), ==, 1), Absolute Value(Index Of Array Value(String Split(Custom String(\"\u{ec47}0\u{ec47}0LSHIFT0LCONTROL0LALT\"), First Of(Null)), Current Array Element)))))";

fn compile_button_to_string(dir: &Path, source: &str) -> String {
    let hir = crate::compile(source, "source.opy", dir).expect("source must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("buttonToString must emit")
        .emitted
}

#[test]
fn button_to_string_expands_with_the_canonical_array_wrap() {
    // Approved exception (docs/architecture/language-core.md): the reference
    // expands the `buttonToString` macro to `Mapped Array(Input Binding
    // String(b), ...)`, putting the binding String in the `Array` parameter;
    // the expansion selects the whole string, so the canonical form wraps it
    // in `Array(...)` — the only structural difference. The expansion stays
    // unfolded under `#!optimizeForSize`, exactly as the reference emits it.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let emitted = compile_button_to_string(
        dir,
        "globalvar a\nrule \"x\":\n    @Event global\n    a = buttonToString(Button.JUMP)\n",
    );
    assert!(emitted.contains(BUTTON_TO_STRING_EXPANSION), "{emitted}");
    // Without the `Array(` wrap the emission is the reference's own text,
    // which canonical validation cannot represent.
    let reference_shape = BUTTON_TO_STRING_EXPANSION.replacen(
        "Array(Input Binding String(Button(Jump)))",
        "Input Binding String(Button(Jump))",
        1,
    );
    assert!(!emitted.contains(&reference_shape), "{emitted}");
}

#[test]
fn button_to_string_keeps_the_expansion_under_size_optimization() {
    // The reference emits the macro expansion unfolded even under
    // `#!optimizeForSize`; the synthesized nodes stay outside the
    // optimization mark.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let emitted = compile_button_to_string(
        dir,
        "#!optimizeForSize\nglobalvar a\nrule \"x\":\n    @Event global\n    a = buttonToString(Button.JUMP)\n",
    );
    assert!(emitted.contains(BUTTON_TO_STRING_EXPANSION), "{emitted}");
}

#[test]
fn button_to_string_stays_unwrapped_in_a_boolean_position() {
    // The oracle's wrap-in-boolean check sees the expanded `mappedArray`
    // root, which is not a wrapped function, so the expansion stays bare —
    // unlike the former `inputBindingString` node, which the reference (and
    // this compiler) wrapped in `First Of`.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let emitted = compile_button_to_string(
        dir,
        "rule \"x\":\n    @Event global\n    waitUntil(buttonToString(Button.JUMP), 1)\n",
    );
    assert!(
        emitted.contains(&format!("Wait Until({BUTTON_TO_STRING_EXPANSION}, 1)")),
        "{emitted}"
    );
    assert!(!emitted.contains("First Of(Mapped Array"), "{emitted}");
}

#[test]
fn literal_domain_members_emit_the_reference_display_names() {
    // Pinned OverPy 9.7.10 emits a `*Literal` member as the bare
    // display-name lookup of its constant table — oracle output observed
    // for this exact source:
    //   `TeamLiteral.1`          → `Set Global Variable(g, Team 1)`
    //   `HeroLiteral.ANA`        → `Set Global Variable(g, Ana)`
    //   `ColorLiteral.WHITE`     → `Set Global Variable(g, White)`
    //   `ColorLiteral.LIGHT_RED` → `Set Global Variable(g, )`
    //   `Color.LIGHT_RED`        → `Set Global Variable(g, Custom Color(255, 112, 122, 255))`
    //   `GamemodeLiteral.ASSAULT`→ `Set Global Variable(g, Assault)`
    // Literal emission skips canonical wrappers like `Game Mode(...)`:
    // `Team 1`, `Assault`, and the empty slot are byte-identical to the
    // reference, while `Hero(Ana)`/`Color(White)` are the canonical forms
    // the grammar reads back as the same members (probe: `match`). The
    // `onlyInOverpy` empty slot is not canonical-parseable on either side —
    // emitted here through `workshop_rs::Value::Empty`
    // (wrightkit/workshop-rs#383); the durable probe records both sides as
    // `unparsable` (tools/overpy/probe-gaps.json).
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = "globalvar g\nrule \"x\":\n    @Event global\n    g = TeamLiteral.1\n    g = HeroLiteral.ANA\n    g = ColorLiteral.WHITE\n    g = ColorLiteral.LIGHT_RED\n    g = Color.LIGHT_RED\n    g = GamemodeLiteral.ASSAULT\n";
    let hir = crate::compile(source, "source.opy", dir).expect("source must resolve");
    let artifact = Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("the literal members must emit");
    for expected in [
        "Set Global Variable(g, Team 1)",
        "Set Global Variable(g, Hero(Ana))",
        "Set Global Variable(g, Color(White))",
        "Set Global Variable(g, )",
        "Set Global Variable(g, Assault)",
    ] {
        assert!(
            artifact.emitted.contains(expected),
            "{expected}\n{emitted}",
            emitted = artifact.emitted
        );
    }
    // `Color.LIGHT_RED` is the same OverPy-only member through the `Color`
    // receiver, where the pinned upstream lowers it to `rgb(...)`: exactly
    // one canonical `Custom Color` call, never the literal's empty slot.
    assert_eq!(
        artifact
            .emitted
            .matches("Set Global Variable(g, Custom Color(255, 112, 122, 255))")
            .count(),
        1,
        "{}",
        artifact.emitted
    );
}
