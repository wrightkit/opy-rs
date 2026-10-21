use super::*;

fn check_source(source: &str) -> CheckOutcome {
    check(source, "main.opy", Path::new(""))
}

#[test]
fn clean_project_has_no_diagnostics_and_a_model() {
    let outcome = check_source(
        "globalvar total = 0\nrule \"r\":\n    @Event global\n    total += 1\n    debug(total)\n",
    );
    assert!(
        outcome.is_clean(),
        "unexpected diagnostics: {:?}",
        outcome.diagnostics
    );
    let model = outcome.model.expect("a clean project resolves");
    assert_eq!(outcome.files.len(), 1);
    assert_eq!(model.declarations().len(), 1);
    assert_eq!(model.rules().len(), 1);
}

#[test]
fn symbols_index_declarations_and_references() {
    let outcome = check_source(
        "globalvar total\nplayervar P\nsubroutine reset\nmacro double(x):\n    x + x\nrule \"r\":\n    @Event eachPlayer\n    total = 1\n    eventPlayer.P = total\n    reset()\n    double(2)\n",
    );
    let model = outcome.model.expect("clean project");
    let names: Vec<(&str, SymbolKind)> = model
        .symbols()
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("total", SymbolKind::Global),
            ("P", SymbolKind::Player),
            ("reset", SymbolKind::Subroutine),
            ("double", SymbolKind::Macro),
        ]
    );
    assert_eq!(model.symbol("total").expect("symbol").references.len(), 2);
    assert_eq!(model.symbol("P").expect("symbol").references.len(), 1);
    let reset = model.symbol("reset").expect("symbol");
    assert_eq!(reset.references.len(), 1);
    assert_eq!(reset.references[0].path, "main.opy");
    assert_eq!(model.symbol("double").expect("symbol").references.len(), 1);
}

#[test]
fn symbol_lookup_by_name_and_span() {
    let outcome = check_source("globalvar total\nrule \"r\":\n    @Event global\n    total = 1\n");
    let model = outcome.model.expect("clean project");
    let total = model.symbol("total").expect("symbol by name");
    assert_eq!(total.kind, SymbolKind::Global);
    let at_decl = model
        .symbol_at(total.declaration.to_span())
        .expect("symbol at declaration span");
    assert_eq!(at_decl.name, "total");
    let at_ref = model
        .symbol_at(total.references[0].to_span())
        .expect("symbol at reference span");
    assert_eq!(at_ref.name, "total");
    assert!(
        model
            .symbol_at(Span::new(99, Position::new(1, 1), Position::new(1, 1)))
            .is_none()
    );
}

#[test]
fn provenance_resolves_through_the_file_registry() {
    let outcome = check_source("globalvar total\nrule \"r\":\n    @Event global\n    total = 1\n");
    let model = outcome.model.expect("clean project");
    let total = model.symbol("total").expect("symbol");
    let provenance = model
        .provenance(total.references[0].to_span())
        .expect("provenance");
    assert_eq!(provenance.file_id, 0);
    assert_eq!(provenance.path, "main.opy");
    assert_eq!(provenance.start.line, 4);
    assert_eq!(model.file(0), Some("main.opy"));
    assert_eq!(model.file(1), None);
}

#[test]
fn custom_enums_are_queried_from_the_model() {
    let outcome = check_source(
        "globalvar x\nenum Direction:\n    NORTH\n    SOUTH\nrule \"r\":\n    @Event global\n    x = Direction.SOUTH\n",
    );
    let model = outcome.model.expect("clean project");
    assert_eq!(model.enums().len(), 1);
    let direction = &model.enums()[0];
    assert_eq!(direction.name, "Direction");
    let members: Vec<&str> = direction
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(members, vec!["NORTH", "SOUTH"]);
    assert!(direction.members[0].span.path.ends_with("main.opy"));
}

#[test]
fn unknown_setting_names_the_keys_valid_at_the_path() {
    let outcome = check_source(
        "settings {\n    \"gamemodes\": {\"ffa\": {\"notASetting\": true}}\n}\nrule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|d| d.code == "unknown-setting")
        .expect("unknown setting diagnostic");
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert!(
        diagnostic.message.contains("(did you mean "),
        "message: {}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("'disabledMaps'"),
        "message: {}",
        diagnostic.message
    );
}

#[test]
fn unknown_setting_preserves_template_paths_and_percent_suffixes() {
    let outcome = check_source(
        "settings {\n    \"main\": {\"description\": \"t\"},\n    \"gamemodes\": {},\n    \"heroes\": {\"team1\": {\"general\": {\"damageReceiveed%\": 50}}}\n}\nrule \"a\":\n    @Event global\n    wait(1)\n",
    );
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|d| d.code == "unknown-setting")
        .expect("unknown setting diagnostic");
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert!(
        diagnostic.message.contains("'damageReceived%'"),
        "message: {}",
        diagnostic.message
    );
}

#[test]
fn check_reports_every_parse_error() {
    let outcome = check_source("rule \"a\"\n    @Event global\nrule \"b\"\n");
    assert!(!outcome.is_clean());
    assert!(outcome.model.is_none());
    assert_eq!(outcome.diagnostics.len(), 3);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "parse-error")
    );
}

#[test]
fn diagnostics_carry_severity_code_and_span() {
    let outcome = check_source("rule \"r\":\n    @Event global\n    frobnicate()\n");
    let diagnostic = &outcome.diagnostics[0];
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostic.code, "unknown-action");
    let span = diagnostic.span.as_ref().expect("source-located");
    assert_eq!(span.path, "main.opy");
    assert_eq!(span.start.line, 3);
}

#[test]
fn canonical_workshop_ids_are_source_diagnostics() {
    for (statement, code) in [
        ("g = allTankHeroes()", "unknown-value"),
        ("g = lastCreatedEntity()", "unknown-value"),
        ("g = evaluateOnce(1)", "unknown-value"),
        ("destroyAllHudText()", "unknown-action"),
    ] {
        let outcome = check_source(&format!(
            "globalvar g\nrule \"r\":\n    @Event global\n    {statement}\n"
        ));
        assert!(!outcome.is_clean(), "{statement}");
        let diagnostic = outcome
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap_or_else(|| panic!("{statement}: expected {code}"));
        let span = diagnostic.span.as_ref().expect("source-located");
        assert_eq!(span.start.line, 4, "{statement}");
    }

    let outcome = check_source(
        "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    g = eventPlayer.isButtonHeld(Button.PRIMARY_FIRE)\n",
    );
    assert!(!outcome.is_clean());
    let diagnostic = outcome
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "unknown-member")
        .expect("unknown-member diagnostic");
    assert_eq!(diagnostic.span.as_ref().unwrap().start.line, 4);

    let outcome = check_source(
        "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    g = getTankHeroes()\n    g = getLastCreatedEntity()\n    g = evalOnce(1)\n    g = eventPlayer.isHoldingButton(Button.PRIMARY_FIRE)\n    destroyAllHudTexts()\n",
    );
    assert!(
        outcome.is_clean(),
        "unexpected diagnostics: {:?}",
        outcome.diagnostics
    );
}
