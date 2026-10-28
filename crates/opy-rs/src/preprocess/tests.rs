use super::*;

#[test]
fn object_define_expands_at_use_site() {
    let (pre, _) = preprocess(
        "#!define SIDE 1.5\nrule \"r\":\n    x = SIDE\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    assert_eq!(pre.defines.len(), 1);
    assert_eq!(pre.defines[0].name, "SIDE");
    assert!(!pre.defines[0].is_function);
    assert!(!pre.defines[0].is_member);
    let numbers: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Number)
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["1.5"]);
}

#[test]
fn expanded_tokens_carry_the_use_site_span() {
    let (pre, _) = preprocess(
        "#!define T a_much_wider_replacement + another_token\nrule \"r\":\n    x = T\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let use_site = pre
        .tokens
        .iter()
        .find(|token| token.text == "a_much_wider_replacement")
        .expect("expanded token")
        .span;
    for token in pre.tokens.iter().filter(|token| {
        matches!(
            token.text.as_str(),
            "a_much_wider_replacement" | "+" | "another_token"
        )
    }) {
        assert_eq!(
            token.span, use_site,
            "{} must attribute to the use site",
            token.text
        );
    }
    assert_eq!(use_site.start, crate::diag::Position::new(3, 9));
    assert_eq!(use_site.end, crate::diag::Position::new(3, 10));
}

#[test]
fn function_define_substitutes_params() {
    let (pre, _) = preprocess(
        "#!define double(x) x + x\nrule \"r\":\n    y = double(3)\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let numbers: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Number)
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["3", "3"]);
}

#[test]
fn function_define_keeps_commas_inside_nested_collections() {
    let (pre, _) = preprocess(
        "#!define first(xs, fallback) xs[0]\nrule \"r\":\n    x = first([1, 2], 3)\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let numbers: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Number)
        .map(|token| token.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["1", "2", "0"]);
}

#[test]
fn zero_argument_function_define_accepts_empty_invocation() {
    let (pre, _) = preprocess(
        "#!define value() 3\nrule \"r\":\n    x = value()\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let numbers: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Number)
        .map(|token| token.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["3"]);
}

#[test]
fn macro_expanded_string_can_concatenate_with_following_literal() {
    let (pre, _) = preprocess(
        "#!define PREFIX \"one\"\nrule \"r\":\n    debug(PREFIX\n        \"two\")\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let output = crate::parser::parse(&pre.tokens);
    assert!(
        output.errors.is_empty(),
        "unexpected errors: {:?}",
        output.errors
    );
    let program = output.program.expect("expanded source must parse");
    let crate::cst::RuleEntry::Rule(rule) = &program.rules[0] else {
        panic!("expected rule");
    };
    let crate::cst::Stmt::Expr { expr, .. } = &rule.actions[0] else {
        panic!("expected expression statement");
    };
    let crate::cst::Expr::Call { args, .. } = expr else {
        panic!("expected call");
    };
    assert!(matches!(
        &args[0].value,
        crate::cst::Expr::String { value, .. } if value == "onetwo"
    ));
}

#[test]
fn recursive_defines_expand_transitively() {
    let (pre, _) = preprocess(
        "#!define A 2\n#!define B A + 1\nrule \"r\":\n    x = B\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let numbers: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Number)
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["2", "1"]);
}

#[test]
fn recursive_define_fails_structurally() {
    let error = preprocess(
        "#!define X X + 1\nrule \"r\":\n    x = X\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap_err();
    assert_eq!(error.code, "macro-recursion");
}

#[test]
fn defines_are_only_visible_after_their_directive() {
    let (pre, _) = preprocess(
        "VALUE\n#!define VALUE 1\nVALUE\n#!allowMacroRedeclaration\n#!define VALUE 2\nVALUE\n",
        "main.opy",
        Path::new("."),
    )
    .expect("ordered define expansion");
    let numbers: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Number)
        .map(|token| token.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["1", "2"]);
    assert!(pre.tokens.iter().any(|token| {
        token.kind == TokenKind::Ident && token.text == "VALUE" && token.span.start.line == 1
    }));
}

#[test]
fn function_define_substitutes_inside_string_text() {
    let (pre, _) = preprocess(
        "#!define wrap(value) \"value\"\nwrap(foo + bar)\n",
        "main.opy",
        Path::new("."),
    )
    .expect("textual define expansion");
    let strings: Vec<&str> = pre
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::String)
        .map(|token| token.text.as_str())
        .collect();
    assert_eq!(strings, vec!["foo + bar"]);
}

#[test]
fn multiline_define_expanded_tokens_attribute_to_the_use_site() {
    let (pre, _) = preprocess(
        "#!define block() A = 1\\\n    A = 2\nblock()\n",
        "main.opy",
        Path::new("."),
    )
    .expect("multiline define expansion");
    let expected = Span::new(
        0,
        crate::diag::Position::new(3, 1),
        crate::diag::Position::new(3, 6),
    );
    let numbers: Vec<Span> = pre
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Number)
        .map(|token| token.span)
        .collect();
    assert_eq!(numbers, vec![expected; 2]);
}

#[test]
fn function_define_requires_a_call_with_the_expected_arity() {
    let error = preprocess(
        "#!define value(argument) argument + 1\nvalue\n",
        "main.opy",
        Path::new("."),
    )
    .expect_err("a function-like define used without parentheses must fail arity checking");
    assert_eq!(error.code, "macro-arity");
}

#[test]
fn exact_object_define_self_reference_is_rejected_at_definition() {
    let error = preprocess("#!define VALUE VALUE\n", "main.opy", Path::new("."))
        .expect_err("an exact object define self-reference must fail at definition");
    assert_eq!(error.code, "macro-recursion");

    let error = preprocess(
        "#!define value(argument) value(argument)\n",
        "main.opy",
        Path::new("."),
    )
    .expect_err("an exact function define self-reference must fail at definition");
    assert_eq!(error.code, "macro-recursion");
}

#[test]
fn missing_include_is_structured() {
    let error = preprocess(
        "#!include \"nope.opy\"\n",
        "main.opy",
        Path::new("/nonexistent-root"),
    )
    .unwrap_err();
    assert_eq!(error.code, "include-not-found");
    assert!(error.span.is_some());
}

#[test]
fn include_cycle_is_detected() {
    let dir = std::env::temp_dir().join(format!("wright-opy-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.opy"), "#!include \"b.opy\"\n").unwrap();
    std::fs::write(dir.join("b.opy"), "#!include \"a.opy\"\n").unwrap();
    let main = std::fs::read_to_string(dir.join("a.opy")).unwrap();
    let error = preprocess(&main, "a.opy", &dir).unwrap_err();
    assert_eq!(error.code, "include-cycle");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unsupported_directive_is_structured() {
    let error = preprocess("#!frobnicate\n", "main.opy", Path::new(".")).unwrap_err();
    assert_eq!(error.code, "unsupported-directive");
}

#[test]
fn settings_block_is_extracted_before_lexing() {
    let (pre, _) = preprocess(
        "settings {\n    \"gamemodes\": {}\n}\nrule \"r\":\n    pass\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let block = pre.settings.expect("settings block extracted");
    assert!(block.text.contains("gamemodes"));
    assert!(
        !pre.tokens.iter().any(|t| t.text.contains("gamemodes")),
        "settings content must not be lexed"
    );
}

#[test]
fn external_settings_file_is_loaded_before_lexing() {
    let overlay = BTreeMap::from([(
        "settings.opy.json".to_string(),
        "{\n  \"gamemodes\": {},\n  \"lobby\": {\"maxPlayers\": 12}\n}\n".to_string(),
    )]);
    let (pre, _) = preprocess_with_overlay(
        "settings \"settings.opy.json\"\nrule \"r\":\n    pass\n",
        "main.opy",
        Path::new("."),
        &overlay,
    )
    .expect("external settings should load");
    let block = pre.settings.expect("external settings block");
    assert_eq!(block.external_path.as_deref(), Some("settings.opy.json"));
    assert!(block.text.contains("maxPlayers"));
    let parsed = crate::settings::parse_block(&block).expect("external JSONC object");
    assert_eq!(parsed.children.len(), 2);
}

#[test]
fn settings_in_include_is_extracted_with_source_provenance() {
    let overlay = BTreeMap::from([(
        "shared.opy".to_string(),
        "settings {\n    \"gamemodes\": {}\n}\n".to_string(),
    )]);
    let main = "#!include \"shared.opy\"\nrule \"r\":\n    pass\n";
    let (pre, files) = preprocess_with_overlay(main, "main.opy", Path::new("."), &overlay)
        .expect("included settings must be extracted");
    let block = pre.settings.expect("included settings block");
    assert_eq!(block.keyword_span.file, 1);
    assert_eq!(files[1].path, "shared.opy");
    assert!(!pre.tokens.iter().any(|token| token.text == "gamemodes"));
}

#[test]
fn duplicate_include_is_skipped_without_redeclaring_macros() {
    let overlay = BTreeMap::from([("shared.opy".to_string(), "#!define VALUE 2\n".to_string())]);
    let main = "#!include \"shared.opy\"\n#!include \"shared.opy\"\nrule \"r\":\n    x = VALUE\n";
    let (pre, files) = preprocess_with_overlay(main, "main.opy", Path::new("."), &overlay)
        .expect("duplicate includes must not redeclare macros");
    assert_eq!(pre.defines.len(), 1);
    assert_eq!(files.len(), 2);
    assert_eq!(pre.warnings.len(), 1);
    assert_eq!(pre.warnings[0].code, "w_already_imported");
    assert_eq!(
        pre.preprocessing
            .directives
            .iter()
            .filter(|directive| directive.name == "include")
            .count(),
        2
    );
}

#[test]
fn alias_include_paths_are_distinct_imports() {
    let overlay = BTreeMap::from([
        ("shared.opy".to_string(), "#!define FIRST 1\n".to_string()),
        (
            "dir/../shared.opy".to_string(),
            "#!define SECOND 2\n".to_string(),
        ),
    ]);
    let main = "#!include \"shared.opy\"\n#!include \"dir/../shared.opy\"\nrule \"r\":\n    x = FIRST\n    y = SECOND\n";
    let (pre, files) = preprocess_with_overlay(main, "main.opy", Path::new("."), &overlay)
        .expect("alias include paths must remain distinct imports");
    assert_eq!(files.len(), 3);
    assert_eq!(files[1].path, "shared.opy");
    assert_eq!(files[2].path, "dir/../shared.opy");
    assert_eq!(pre.defines.len(), 2);
    assert!(pre.warnings.is_empty());
}

#[test]
fn dict_literal_braces_reach_the_parser() {
    let (pre, _) = preprocess(
        "rule \"r\":\n    money += {\n        Mei.GENERIC: 10,\n    }\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    assert!(
        pre.tokens
            .iter()
            .any(|token| token.kind == TokenKind::LBrace)
    );
    assert!(
        pre.tokens
            .iter()
            .any(|token| token.kind == TokenKind::RBrace)
    );
}

#[test]
fn advanced_directives_preserve_frontend_state_without_catalog_data() {
    let (pre, _) = preprocess(
        "#!allowMacroRedeclaration\n#!translations en fr\n#!rulePrefix \"Effects\"\n#!optimizeForSize\n#!optimizeStrict\n#!replace0ByCapturePercentage\n#!define VALUE 1\n#!define VALUE 2\nrule \"r\":\n    x = VALUE\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    assert!(pre.preprocessing.allow_macro_redeclaration);
    assert_eq!(
        pre.preprocessing
            .translations
            .as_ref()
            .map(|state| state.languages.as_slice()),
        Some(["en".to_string(), "fr".to_string()].as_slice())
    );
    assert_eq!(
        pre.preprocessing
            .rule_prefix
            .as_ref()
            .map(|value| value.value.as_str()),
        Some("Effects")
    );
    assert!(pre.preprocessing.optimization.for_size);
    assert!(pre.preprocessing.optimization.strict);
    assert_eq!(
        pre.preprocessing.replacements[0].value,
        "getCapturePercentage"
    );
    assert_eq!(pre.defines.len(), 1);
}

#[test]
fn backend_only_directives_are_validated_and_recorded() {
    let (pre, _) = preprocess(
        "#!excludeVariablesInCompilation\n#!extension projectiles\n#!setupTags\n#!setupTx\n#!translateWithPlayerVar noDetectionRule noTlErr\n#!disableInspector\n#!writeToOutputFile\n#!disableTranslationSourceLines\n#!keepUnusedTranslations\n#!useVariableForCompressionAlphabet\n#!debugElementCount\n#!globalvarInitRuleName \"Init globals\"\n#!playervarInitRuleName \"Init players\"\nrule \"r\":\n    pass\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    let names: Vec<&str> = pre
        .preprocessing
        .directives
        .iter()
        .map(|directive| directive.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "excludeVariablesInCompilation",
            "extension",
            "setupTags",
            "setupTx",
            "translateWithPlayerVar",
            "disableInspector",
            "writeToOutputFile",
            "disableTranslationSourceLines",
            "keepUnusedTranslations",
            "useVariableForCompressionAlphabet",
            "debugElementCount",
            "globalvarInitRuleName",
            "playervarInitRuleName",
        ]
    );
    assert_eq!(
        pre.preprocessing.directives[1].value.as_deref(),
        Some("projectiles")
    );
    assert_eq!(
        pre.preprocessing.directives[4].value.as_deref(),
        Some("noDetectionRule noTlErr")
    );
}

#[test]
fn extension_directive_rejects_unknown_schema_values() {
    let error = preprocess(
        "#!extension notAnExtension\nrule \"r\":\n    pass\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap_err();
    assert_eq!(error.code, "directive-invalid");
    assert_eq!(error.span.expect("directive provenance").start.line, 1);
}

#[test]
fn translations_follow_pinned_codes_without_local_deduplication() {
    let (pre, _) = preprocess(
        "#!translations EN zh-cn en\nrule \"r\":\n    pass\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap();
    assert_eq!(
        pre.preprocessing.translations.unwrap().languages,
        vec!["en", "zh_cn", "en"]
    );
}

#[test]
fn translations_reject_codes_outside_the_pinned_oracle_set() {
    let error = preprocess(
        "#!translations en_US\nrule \"r\":\n    pass\n",
        "main.opy",
        Path::new("."),
    )
    .unwrap_err();
    assert_eq!(error.code, "translations-invalid");
}

#[test]
fn directive_records_expose_state_transitions_and_include_depth() {
    let root =
        std::env::temp_dir().join(format!("wright-opy-directive-scope-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("child.opy"),
        "#!rulePrefix \"inner\"\n#!disableOptimizations\n",
    )
    .unwrap();
    let (pre, _) = preprocess(
        "#!rulePrefix \"outer\"\n#!include \"child.opy\"\n#!enableOptimizations\n",
        "main.opy",
        &root,
    )
    .unwrap();
    let records = &pre.preprocessing.directives;
    assert_eq!(records[0].state.rule_prefix.as_deref(), Some("outer"));
    assert_eq!(records[0].scope_depth, 0);
    assert_eq!(records[1].name, "rulePrefix");
    assert_eq!(records[1].state.rule_prefix.as_deref(), Some("inner"));
    assert!(!records[2].state.optimization.enabled);
    assert_eq!(records[2].scope_depth, 1);
    assert_eq!(records[3].name, "include");
    assert_eq!(records[3].state.rule_prefix.as_deref(), Some("outer"));
    assert_eq!(records[4].name, "enableOptimizations");
    assert!(records[4].state.optimization.enabled);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn malformed_translation_state_is_source_located() {
    let error = preprocess("#!translations\n", "main.opy", Path::new(".")).unwrap_err();
    assert_eq!(error.code, "translations-invalid");
    assert!(error.span.is_some());
}
