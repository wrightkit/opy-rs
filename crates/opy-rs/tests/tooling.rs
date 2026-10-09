//! Integration tests for the Workshop-independent tooling API:
//! multi-file project validation through [`opy_rs::tooling::check`],
//! semantic queries on the resolved model, and stable diagnostic codes for
//! representative malformed inputs.

use std::path::Path;

use opy_rs::diag::{Position, Span};
use opy_rs::tooling::{self, SymbolKind, check};

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
    assert_eq!(model.declarations().len(), 5);
    assert!(model
        .declarations()
        .iter()
        .any(|decl| matches!(decl, opy_rs::hir::types::Declaration::GlobalVariable { name, .. } if name == "total")));
    assert!(model
        .declarations()
        .iter()
        .any(|decl| matches!(decl, opy_rs::hir::types::Declaration::PlayerVariable { name, .. } if name == "P")));

    // Rule listing: the rule plus the def'd subroutine (the include splices
    // first, so the def entry precedes the rule entry).
    assert_eq!(model.rules().len(), 2);
    assert!(model.rules().iter().any(|entry| matches!(
        entry,
        opy_rs::hir::types::RuleEntry::Rule(rule) if rule.name == "collect"
    )));
    assert!(model.rules().iter().any(|entry| matches!(
        entry,
        opy_rs::hir::types::RuleEntry::SubroutineDef { name, .. } if name == "finish"
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
    let finish_symbols: Vec<_> = model
        .symbols()
        .iter()
        .filter(|symbol| symbol.name == "finish")
        .collect();
    assert_eq!(finish_symbols.len(), 2);
    assert!(
        finish_symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Subroutine)
    );
    assert!(
        finish_symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Def)
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

#[test]
fn tooling_exposes_define_member_identity() {
    let outcome = check(
        "#!defineMember VALUE 2\nglobalvar result\nrule \"member define\":\n    @Event global\n    result = VALUE\n",
        "main.opy",
        Path::new("."),
    );
    let model = outcome
        .model
        .expect("member define project must check clean");

    assert_eq!(model.defines().len(), 1);
    assert!(model.defines()[0].is_member);
}

/// Representative malformed inputs with their stable diagnostic codes (the
/// machine contract: codes and source locations, not wording).
#[rustfmt::skip]
const STABLE_DIAGNOSTICS: &[(&str, &str)] = &[
    // A bare dictionary is now lexed and rejected at the OPY semantic
    // boundary rather than being mistaken for settings content.
    ("globalvar money\nrule \"r\":\n    @Event global\n    money += {\n        Mei.GENERIC: 10,\n    }\n", "dict-access"),
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
fn workshop_script_source_reports_exactly_one_diagnostic() {
    // opy-rs#420: pasted Workshop script must surface as exactly one
    // `workshop-source` diagnostic through `check` — the surface Wright/LPP
    // consume — not only at the parser layer.
    let outcome = check(
        "rule \"workshop style\" {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Wait(1, Ignore Condition);\n    }\n}\n",
        "main.opy",
        Path::new(""),
    );
    assert_eq!(
        outcome.diagnostics.len(),
        1,
        "expected one diagnostic, got {:?}",
        outcome.diagnostics
    );
    let diagnostic = &outcome.diagnostics[0];
    assert_eq!(diagnostic.code, "workshop-source");
    assert_eq!(diagnostic.span.as_ref().expect("span").path, "main.opy");
    assert!(outcome.model.is_none(), "a failing project has no model");
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
    let compile_error = opy_rs::compile(
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

#[test]
fn player_member_references_use_exact_member_span() {
    let source = "playervar I\nrule \"r\":\n    @Event global\n    for hostPlayer.I in range(3):\n        hostPlayer.I = 1\n";
    let outcome = check(source, "main.opy", Path::new(""));
    assert!(
        outcome.is_clean(),
        "player member source must check clean: {:?}",
        outcome.diagnostics
    );
    let model = outcome.model.expect("clean project");
    let symbol = model.symbol("I").expect("player variable symbol");

    assert_eq!(symbol.references.len(), 2);
    assert!(
        symbol
            .references
            .iter()
            .any(|reference| reference.start.line == 4 && reference.start.col == 20)
    );
    assert!(
        symbol
            .references
            .iter()
            .any(|reference| reference.start.line == 5 && reference.start.col == 20)
    );
}

#[test]
fn settings_emission_agreement_between_check_and_compile() {
    // opy-rs#411: `check` and `compile` must agree on every settings member.
    // A member the catalog does not declare compiles as the pinned OverPy
    // writes it and checks with a warning rather than an error: an unknown
    // key with a scalar, empty, list, or object value, and an enum key with
    // an undeclared value.
    let unknown = concat!(
        "settings {\n",
        "    \"main\": {\"description\": \"t\", \"emptyKey\": \"\"},\n",
        "    \"lobby\": {\"mapRotation\": \"sometimes\"},\n",
        "    \"gamemodes\": {\"ffa\": {\"notASetting\": 3, \"aList\": [\"x\", 1], \"anObject\": {\"k\": true}}},\n",
        "    \"heroes\": {\"allTeams\": {\"shion\": {\"ability2Duration\": \"500%\"}}}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let outcome = check(unknown, "main.opy", Path::new(""));
    assert!(outcome.model.is_some(), "{:?}", outcome.diagnostics);
    let warnings = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.severity, diagnostic.code.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        warnings,
        [(tooling::DiagnosticSeverity::Warning, "unknown-setting"); 6]
    );
    let lines = compiled_lines(unknown);
    for line in [
        "emptyKey:",
        "Map Rotation: sometimes",
        "notASetting: 3",
        "aList {",
        "x",
        "1",
        "anObject {",
        "k: true",
        "ability2Duration: 500%",
    ] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }

    // A scalar directly under a team (upstream reads it as a hero name) still
    // fails both entry points under the same `workshop-emission` code.
    let invalid = concat!(
        "settings {\n",
        "    \"main\": {\"description\": \"t\"},\n",
        "    \"gamemodes\": {}, \"heroes\": {\"allTeams\": {\"notAHero\": 1}}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let outcome = check(invalid, "main.opy", Path::new(""));
    assert!(outcome.model.is_none());
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == tooling::DiagnosticSeverity::Error)
        .expect("a team-level scalar must fail check");
    assert_eq!(diagnostic.code, "workshop-emission");
    assert_eq!(diagnostic.span.as_ref().expect("span").path, "main.opy");
    let compile_error = opy_rs::compile(invalid, "main.opy", Path::new("")).unwrap_err();
    assert_eq!(compile_error.code, "workshop-emission");

    // The inherited `gamemodes.general` key that motivated the issue passes
    // both entry points.
    let valid = concat!(
        "settings {\n",
        "    \"main\": {\"description\": \"t\"},\n",
        "    \"gamemodes\": {\"ffa\": {\"enabledMaps\": [\"workshopIsland\"], \"enableKillCam\": false}}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let outcome = check(valid, "main.opy", Path::new(""));
    assert!(
        outcome.is_clean(),
        "inherited gamemode key must check clean: {:?}",
        outcome.diagnostics
    );
}

#[test]
fn settings_numbers_render_like_the_pinned_oracle() {
    // opy-rs#496: the pinned OverPy writes numeric settings values with
    // JavaScript `String(value)` semantics and reads decimal exponent forms
    // plus `0x`/`0b`/`0o` integer literals, in scalar positions and inside
    // list elements.
    let source = concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"main\": {\"a\": 0b101, \"b\": 0o17, \"c\": 0x1F, \"d\": 1e21,\n",
        "               \"e\": 0.0000001, \"f\": 123456789012345678901234, \"g\": 0.5e3,\n",
        "               \"h\": 1.0, \"i\": 1e20, \"j\": 12e2, \"k\": 0.1,\n",
        "               \"list\": [0b101, 0o17, 0x1F, 1e21, 0.0000001]}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let lines = compiled_lines(source);
    for line in [
        "a: 5",
        "b: 15",
        "c: 31",
        "d: 1e+21",
        "e: 1e-7",
        "f: 1.2345678901234569e+23",
        "g: 500",
        "h: 1",
        "i: 100000000000000000000",
        "j: 1200",
        "k: 0.1",
    ] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }
    let list = lines
        .iter()
        .position(|line| line == "list {")
        .expect("list block");
    assert_eq!(
        &lines[list + 1..list + 6],
        ["5", "15", "31", "1e+21", "1e-7"]
    );
}

#[test]
fn hero_settings_apply_the_pinned_per_hero_schema() {
    // opy-rs#495: the pinned schema merges hero settings per hero, so a
    // catalogued key is only translated where it applies; elsewhere it is
    // written fully verbatim.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"ana\": {\"ability3Cooldown%\": 50},\n",
        "        \"wreckingBall\": {\"ability3Cooldown%\": 50},\n",
        "        \"reinhardt\": {\"ammoClipSize%\": 50},\n",
        "        \"mercy\": {\"ammoClipSize%\": 50},\n",
        "        \"dva\": {\"ability2Height%\": 50},\n",
        "        \"freja\": {\"ability2Height%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    for line in [
        "ability3Cooldown%: 50",
        "ammoClipSize%: 50",
        "ability2Height%: 50",
        "Piledriver Cooldown Time: 50%",
        "Ammunition Clip Size Scalar: 50%",
        "Updraft Height: 50%",
    ] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }
    for translated in [
        "Sleep Dart Cooldown Time",
        "Dynamite Cooldown Time",
        "Biotic Grenade",
    ] {
        assert!(
            !lines.iter().any(|line| line.contains(translated)),
            "{translated:?} unexpectedly in {lines:?}"
        );
    }
}

#[test]
fn hero_settings_rewrite_the_pinned_name_and_key_aliases() {
    // opy-rs#495: the pinned compiler rewrites `mccree`/`hammond` hero names
    // and `ability1KB%` member keys before its schema lookup.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"mccree\": {\"ability1Cooldown%\": 50},\n",
        "        \"dva\": {\"ability1KB%\": 50},\n",
        "        \"ana\": {\"ability1KB%\": 50},\n",
        "        \"hammond\": {\"ability3Cooldown%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    for line in [
        "Cassidy {",
        "Combat Roll Cooldown Time: 50%",
        "Boosters Knockback Scalar: 50%",
        "ability1Kb%: 50",
        "Piledriver Cooldown Time: 50%",
    ] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }
    assert!(
        !lines.iter().any(|line| line.contains("ability1KB%")),
        "authored spelling must not survive in {lines:?}"
    );
}

#[test]
fn hero_roster_lists_emit_after_hero_groups_with_canonical_names() {
    // opy-rs#495: the pinned compiler emits `enabledHeroes`/`disabledHeroes`
    // last inside a team block, and rewrites the same hero aliases in them.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"enabledHeroes\": [\"mccree\", \"hammond\"],\n",
        "        \"dva\": {\"health%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    let enabled = lines
        .iter()
        .position(|line| line == "enabled heroes {")
        .expect("hero list");
    let dva = lines
        .iter()
        .position(|line| line == "D.Va {")
        .expect("hero group");
    assert!(
        dva < enabled,
        "hero list must follow hero groups: {lines:?}"
    );
    for line in ["Cassidy", "Wrecking Ball"] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }
}

#[test]
fn hero_kb_rename_uses_the_pinned_assign_delete_order() {
    // opy-rs#495: upstream rewrites `ability1KB%` via assign+delete, so the
    // renamed member moves to the end, or collapses onto an authored
    // `ability1Kb%` position with the source value.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"dva\": {\"ability1KB%\": 20, \"health%\": 150}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    let health = lines
        .iter()
        .position(|line| line == "Health: 150%")
        .expect("health member");
    let kb = lines
        .iter()
        .position(|line| line == "Boosters Knockback Scalar: 20%")
        .expect("renamed member");
    assert!(health < kb, "renamed member must move last: {lines:?}");

    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"dva\": {\"ability1Kb%\": 10, \"health%\": 150, \"ability1KB%\": 20}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    let health = lines
        .iter()
        .position(|line| line == "Health: 150%")
        .expect("health member");
    let kb = lines
        .iter()
        .position(|line| line == "Boosters Knockback Scalar: 20%")
        .expect("renamed member");
    assert!(
        kb < health,
        "collapsed member keeps the destination position: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("10%")),
        "the source value wins over the authored destination: {lines:?}"
    );
}

#[test]
fn hero_group_rename_uses_the_pinned_assign_delete_order() {
    // opy-rs#495: upstream renames alias hero groups via assign+delete, so a
    // renamed group moves to the end, or collapses onto the canonical
    // group's position with the source members.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"mccree\": {\"health%\": 50},\n",
        "        \"ana\": {\"health%\": 150}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    let ana = lines
        .iter()
        .position(|line| line == "Ana {")
        .expect("ana group");
    let cassidy = lines
        .iter()
        .position(|line| line == "Cassidy {")
        .expect("renamed group");
    assert!(ana < cassidy, "renamed group must move last: {lines:?}");

    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"mccree\": {\"health%\": 50},\n",
        "        \"cassidy\": {\"health%\": 10}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    assert_eq!(
        lines.iter().filter(|line| **line == "Cassidy {").count(),
        1,
        "the canonical group collapses to one: {lines:?}"
    );
    assert!(
        lines.contains(&"Health: 50%".to_string())
            && !lines.iter().any(|line| line.contains("10%")),
        "the source members win: {lines:?}"
    );

    // Both aliases in one team: upstream renames per alias in its fixed
    // order, so appended groups follow that order, not authored order.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"hammond\": {\"health%\": 50},\n",
        "        \"mccree\": {\"health%\": 60}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    let cassidy = lines
        .iter()
        .position(|line| line == "Cassidy {")
        .expect("cassidy group");
    let wrecking_ball = lines
        .iter()
        .position(|line| line == "Wrecking Ball {")
        .expect("wrecking ball group");
    assert!(
        cassidy < wrecking_ball,
        "the mccree pass appends before the hammond pass: {lines:?}"
    );
}

#[test]
fn general_children_are_not_reprocessed_as_hero_members() {
    // opy-rs#495: the flattened `general` members take only the team-level
    // pass-through — hero-name and applicability passes must not reach them.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"general\": {\n",
        "            \"dva\": {\"ability1KB%\": 50},\n",
        "            \"damageDealt%\": 50\n",
        "        },\n",
        "        \"ana\": {\"health%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    for line in ["ability1KB%: 50", "Damage Dealt: 50%", "Health: 50%"] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("Boosters Knockback Scalar")),
        "a nested hero dict stays verbatim: {lines:?}"
    );
}

#[test]
fn a_non_dict_general_yields_index_members_or_drops() {
    // opy-rs#495: upstream iterates the value's `Object.keys`, so a string or
    // list `general` emits index-keyed members while numbers/booleans drop.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"general\": \"ab\",\n",
        "        \"ana\": {\"health%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    for line in ["0: a", "1: b", "Health: 50%"] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }

    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"general\": [10, 20],\n",
        "        \"ana\": {\"health%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    for line in ["0: 10", "1: 20"] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }

    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"general\": 5,\n",
        "        \"ana\": {\"health%\": 50}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    assert!(
        lines.contains(&"Health: 50%".to_string())
            && !lines.iter().any(|line| line == "general: 5"),
        "{lines:?} must drop the number member and keep the hero members"
    );
}

#[test]
fn both_hero_lists_in_one_team_error() {
    // opy-rs#495: upstream rejects a team carrying both rosters.
    let error = opy_rs::compile(
        concat!(
            "settings {\n",
            "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
            "    \"heroes\": {\"allTeams\": {\n",
            "        \"enabledHeroes\": [\"ana\"],\n",
            "        \"disabledHeroes\": [\"genji\"]\n",
            "    }}\n",
            "}\n",
            "rule \"a\":\n    @Event global\n    wait(1)\n",
        ),
        "main.opy",
        Path::new(""),
    )
    .unwrap_err();
    assert_eq!(error.code, "settings-hero-lists");
    assert_eq!(
        error.message,
        "Cannot have both 'enabledHeroes' and 'disabledHeroes' in team 'allTeams'"
    );
}

#[test]
fn inapplicable_hero_key_with_enum_value_stays_verbatim() {
    // opy-rs#495: a non-applicable key is unknown for that hero, so both the
    // name and the value keep their authored spelling.
    let lines = compiled_lines(concat!(
        "settings {\n",
        "    \"gamemodes\": {\"ffa\": {\"enabled\": true}},\n",
        "    \"heroes\": {\"allTeams\": {\n",
        "        \"ana\": {\"enableSecondaryFire\": false},\n",
        "        \"genji\": {\"enableGenericSecondaryFire\": true}\n",
        "    }}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    for line in [
        "enableSecondaryFire: false",
        "enableGenericSecondaryFire: true",
    ] {
        assert!(lines.contains(&line.to_string()), "{line:?} in {lines:?}");
    }
}

#[test]
fn misspelled_settings_key_warns_with_a_single_candidate_suffix() {
    // A near-miss settings key compiles unchanged like upstream; `check` warns
    // and names the canonical key once.
    let source = concat!(
        "settings {\n",
        "    \"main\": { \"descriptino\": \"x\" },\n",
        "    \"gamemodes\": {}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let expected = "unknown settings key 'descriptino' is passed through unchanged (did you mean 'description'?)";

    let outcome = check(source, "main.opy", Path::new(""));
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "unknown-setting")
        .expect("the misspelled key must warn");
    assert_eq!(diagnostic.severity, tooling::DiagnosticSeverity::Warning);
    assert_eq!(diagnostic.message, expected);

    assert!(compiled_lines(source).contains(&"descriptino: x".to_string()));
}

fn compiled_lines(source: &str) -> Vec<String> {
    opy_rs::Compiler::new()
        .expect("compiler")
        .compile_source(source, "main.opy", Path::new(""))
        .expect("compiles")
        .workshop
        .lines()
        .map(|line| line.trim().to_string())
        .collect()
}
