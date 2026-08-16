//! Integration tests for the Workshop-independent tooling API (issue #7):
//! multi-file project validation through [`opy_frontend::tooling::check`],
//! semantic queries on the resolved model, and stable diagnostic codes for
//! representative malformed inputs.

use std::path::Path;

use opy_frontend::diag::{Position, Span};
use opy_frontend::tooling::{self, SymbolKind, check};

/// The WrightKit-authored multi-file fixture: `main.opy` includes
/// `shared/defs.opy`, declares `playervar P`, and uses symbols declared in
/// the included file (globalvar, subroutine, enum, macro) plus a `#!define`.
const MULTI_MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/multi-file/main.opy"
);

#[test]
fn multi_file_project_checks_and_resolves_end_to_end() {
    let source = std::fs::read_to_string(MULTI_MAIN).expect("fixture exists");
    let root = Path::new(MULTI_MAIN).parent().expect("fixture parent");
    let outcome = check(&source, MULTI_MAIN, root);

    assert!(
        outcome.is_clean(),
        "multi-file fixture must check clean, got: {:?}",
        outcome.diagnostics
    );
    let model = outcome.model.expect("a clean project resolves");

    // File registry: main file (id 0) plus one entry per include.
    assert_eq!(outcome.files.len(), 2);
    assert_eq!(outcome.files[0].id, 0);
    assert_eq!(outcome.files[1].path, "shared/defs.opy");
    assert_eq!(model.file(1), Some("shared/defs.opy"));

    // Declarations across both files: globalvar/subroutine/macro from the
    // include, playervar from the main file (enums are not retained in the
    // Opy HIR; they are queried separately).
    assert_eq!(model.declarations().len(), 4);
    assert!(model
        .declarations()
        .iter()
        .any(|decl| matches!(decl, opy_frontend::hir::types::Declaration::GlobalVariable { name, .. } if name == "total")));
    assert!(model
        .declarations()
        .iter()
        .any(|decl| matches!(decl, opy_frontend::hir::types::Declaration::PlayerVariable { name, .. } if name == "P")));

    // Rule listing: the rule plus the def'd subroutine (the include splices
    // first, so the def entry precedes the rule entry).
    assert_eq!(model.rules().len(), 2);
    assert!(model.rules().iter().any(|entry| matches!(
        entry,
        opy_frontend::hir::types::RuleEntry::Rule(rule) if rule.name == "collect"
    )));
    assert!(model.rules().iter().any(|entry| matches!(
        entry,
        opy_frontend::hir::types::RuleEntry::SubroutineDef { name, .. } if name == "finish"
    )));

    // Macro-expansion provenance: the #!define is recorded with its site.
    assert_eq!(model.defines().len(), 1);
    assert_eq!(model.defines()[0].name, "SCALE");

    // Custom enums are queryable even though they fold in the HIR.
    assert_eq!(model.enums().len(), 1);
    assert_eq!(model.enums()[0].name, "Direction");
    assert_eq!(model.enums()[0].members.len(), 2);

    // Symbols: bindings from both files with declaration provenance.
    let total = model.symbol("total").expect("globalvar from the include");
    assert_eq!(total.kind, SymbolKind::Global);
    assert_eq!(total.declaration.path, "shared/defs.opy");
    assert_eq!(total.declaration.start.line, 1);

    let p = model.symbol("P").expect("playervar from the main file");
    assert_eq!(p.kind, SymbolKind::Player);
    assert!(p.declaration.path.ends_with("main.opy"));

    let reset = model
        .symbol("resetScore")
        .expect("subroutine from the include");
    assert_eq!(reset.kind, SymbolKind::Subroutine);
    assert_eq!(
        model
            .symbol("doubleIt")
            .expect("macro from the include")
            .kind,
        SymbolKind::Macro
    );
    assert_eq!(
        model.symbol("finish").expect("def binding").kind,
        SymbolKind::Def
    );

    // References: uses in the main file and in the def body resolve to the
    // included-file binding, each with its own file provenance. The
    // augmented assignment lowers to a Binary whose left re-uses the target,
    // so line 9 contributes two reference sites.
    assert_eq!(total.references.len(), 4);
    let main_refs: Vec<_> = total
        .references
        .iter()
        .filter(|reference| reference.file_id == 0)
        .collect();
    assert_eq!(main_refs.len(), 3);
    assert!(
        main_refs
            .iter()
            .all(|reference| reference.path.ends_with("main.opy"))
    );
    let defs_ref = total
        .references
        .iter()
        .find(|reference| reference.file_id == 1)
        .expect("the def body reference");
    assert_eq!(defs_ref.path, "shared/defs.opy");
    assert_eq!(defs_ref.start.line, 13);
    assert_eq!(reset.references.len(), 1);
    assert_eq!(reset.references[0].start.line, 11);
    assert_eq!(model.symbol("doubleIt").expect("macro").references.len(), 1);

    // Span → (file id, path, line/col) provenance, and span-based lookup.
    let main_reference = total
        .references
        .iter()
        .find(|reference| reference.start.line == 9)
        .expect("the augmented-assignment reference in the main file");
    let at_reference = model
        .provenance(main_reference.to_span())
        .expect("reference provenance");
    assert_eq!(at_reference.path, MULTI_MAIN);
    assert_eq!(at_reference.start.line, 9);
    assert_eq!(
        model
            .symbol_at(main_reference.to_span())
            .expect("symbol at reference")
            .name,
        "total"
    );

    // The model serializes for `opy-cli inspect` (declarations, rules,
    // references as one JSON document).
    let json = serde_json::to_value(&model).expect("the model serializes");
    assert_eq!(json["hir"]["protocol"]["name"], "wright/opy-hir");
    assert!(json["symbols"].as_array().expect("symbols array").len() >= 5);
    assert!(json["enums"].as_array().expect("enums array").len() == 1);
}

/// Representative malformed inputs with their stable diagnostic codes (the
/// machine contract: codes and source locations, not wording).
#[rustfmt::skip]
const STABLE_DIAGNOSTICS: &[(&str, &str)] = &[
    // Expression-level dict braces stay a lex error (scoped settings lexing
    // must not mask them).
    ("rule \"r\":\n    @Event global\n    money += {\n        Mei.GENERIC: 10,\n    }\n", "lex-error"),
    // Missing colon after the rule name.
    ("rule \"x\"\n    @Event global\n", "parse-error"),
    // Unknown builtin in statement position.
    ("rule \"r\":\n    @Event global\n    frobnicate()\n", "unknown-action"),
    // Unknown builtin in value position.
    ("globalvar x\nrule \"r\":\n    @Event global\n    x = frobnicate()\n", "unknown-value"),
    // Undeclared identifier.
    ("globalvar x\nrule \"r\":\n    @Event global\n    x = nope\n", "unknown-identifier"),
    // Unknown member function.
    ("rule \"r\":\n    @Event eachPlayer\n    eventPlayer.frobnicate()\n", "unknown-member"),
    // Declared enum domain without the member.
    ("globalvar x\nrule \"r\":\n    @Event global\n    x = Color.CYAN\n", "unknown-enum-member"),
    // Enum-domain mismatch on a builtin argument.
    ("globalvar g\nrule \"r\":\n    @Event global\n    chaseOverTime(g, 10, 3, Invis.ALL)\n", "enum-domain-mismatch"),
    // Positional overflow.
    ("globalvar g\nrule \"r\":\n    @Event global\n    chaseOverTime(g, 10, 3, 4, 5)\n", "invalid-arity"),
    // Value function in action position.
    ("rule \"r\":\n    @Event global\n    isGameInProgress()\n", "value-in-action-position"),
    // Macro arity mismatch at the expansion site (`#!define` macros expand
    // during preprocessing; `macro` statements do not arity-check).
    ("#!define double(x) x + x\nrule \"r\":\n    @Event global\n    double(1, 2)\n", "macro-arity"),
    // Unterminated settings block.
    ("settings {\n    \"gamemodes\": {}\n", "settings-invalid"),
];

#[test]
fn diagnostic_codes_are_stable_for_malformed_inputs() {
    for (source, expected_code) in STABLE_DIAGNOSTICS {
        let outcome = check(source, "main.opy", Path::new(""));
        let diagnostic = outcome
            .diagnostics
            .first()
            .unwrap_or_else(|| panic!("expected a '{expected_code}' diagnostic for:\n{source}"));
        assert_eq!(
            diagnostic.code, *expected_code,
            "unexpected first diagnostic for:\n{source}\ngot: {:?}",
            outcome.diagnostics
        );
        assert!(
            diagnostic.span.is_some(),
            "the '{expected_code}' diagnostic must be source-located:\n{source}"
        );
        assert!(outcome.model.is_none(), "a failing project has no model");
    }
}

#[test]
fn include_failures_are_stable_and_source_located() {
    // Missing include names the directive site.
    let outcome = check(
        "#!include \"nope.opy\"\n",
        "main.opy",
        Path::new("/nonexistent-opy-root"),
    );
    let diagnostic = outcome.diagnostics.first().expect("include-not-found");
    assert_eq!(diagnostic.code, "include-not-found");
    assert_eq!(diagnostic.span.as_ref().expect("span").start.line, 1);

    // An include cycle is detected through the tooling API too.
    let dir = std::env::temp_dir().join(format!("opy-tooling-cycle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.opy"), "#!include \"b.opy\"\n").unwrap();
    std::fs::write(dir.join("b.opy"), "#!include \"a.opy\"\n").unwrap();
    let main = std::fs::read_to_string(dir.join("a.opy")).unwrap();
    let outcome = check(&main, "a.opy", &dir);
    assert_eq!(
        outcome.diagnostics.first().expect("include-cycle").code,
        "include-cycle"
    );
    // The registry retains every file registered before the failure: a (main),
    // b, and the re-included a that triggered the cycle.
    assert_eq!(outcome.files.len(), 3);
    assert_eq!(outcome.files[1].path, "b.opy");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn all_parse_diagnostics_are_reported() {
    // Three broken constructs — two rules missing their colon and a stray
    // directive line — all surface as parse-error diagnostics (the parser
    // recovers at statement boundaries; the tooling API reports every one).
    let outcome = check(
        "rule \"a\"\n    @Event global\nrule \"b\"\n",
        "main.opy",
        Path::new(""),
    );
    assert_eq!(outcome.diagnostics.len(), 3);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "parse-error")
    );
    assert!(outcome.model.is_none());
}

#[test]
fn semantic_errors_follow_the_compile_first_error_contract() {
    // `x = frobnicate()` produces two semantic errors (unknown-identifier and
    // unknown-value); check reports the first, matching `compile`, so the two
    // entry points never disagree about a project's verdict.
    let outcome = check(
        "rule \"r\":\n    @Event global\n    x = frobnicate()\n",
        "main.opy",
        Path::new(""),
    );
    let diagnostic = outcome.diagnostics.first().expect("first semantic error");
    assert_eq!(diagnostic.code, "unknown-identifier");
    let compile_error = opy_frontend::compile(
        "rule \"r\":\n    @Event global\n    x = frobnicate()\n",
        "main.opy",
        Path::new(""),
    )
    .unwrap_err();
    assert_eq!(compile_error.code, diagnostic.code);
}

#[test]
fn check_with_overlay_resolves_unsaved_includes() {
    let mut overlay = std::collections::BTreeMap::new();
    overlay.insert(
        "shared/defs.opy".to_string(),
        "globalvar total\n".to_string(),
    );
    let outcome = tooling::check_with_overlay(
        "#!include \"shared/defs.opy\"\nrule \"r\":\n    @Event global\n    total = 1\n",
        "main.opy",
        Path::new(""),
        &overlay,
    );
    assert!(
        outcome.is_clean(),
        "overlay include must resolve: {:?}",
        outcome.diagnostics
    );
    let model = outcome.model.expect("model");
    assert_eq!(
        model.symbol("total").expect("symbol").kind,
        SymbolKind::Global
    );
}

#[test]
fn unknown_span_lookup_returns_none() {
    let outcome = check("globalvar total\n", "main.opy", Path::new(""));
    let model = outcome.model.expect("clean project");
    let nowhere = Span::new(42, Position::new(1, 1), Position::new(1, 1));
    assert!(model.symbol_at(nowhere).is_none());
    assert!(model.provenance(nowhere).is_none());
    assert_eq!(model.file(42), None);
}
