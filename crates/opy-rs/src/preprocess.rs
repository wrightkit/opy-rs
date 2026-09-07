//! `.opy` preprocessing: includes, `#!define`/`#!defineMember` macros (textual and
//! `__script__` JavaScript-backed), `#!postCompileHook`, and expansion.
//!
//! Operates at the token level, matching the reference frontend's observable
//! behavior: `#!include "file.opy"` splices the included file's tokens at the
//! directive site; `#!define NAME value` and `#!define name(args) value`
//! register macros that expand at their use sites, recursively (a macro may
//! reference earlier macros). The output is a single-file token stream whose
//! spans point at use sites, mirroring the reference adapter's provenance
//! convention (the HIR file registry records the included sources). Invalid
//! include graphs (cycles, missing files) and recursive defines fail
//! deterministically with structured diagnostics that name the offending file/line.
//!
//! # JavaScript macros and hooks
//!
//! A function-like define whose replacement starts with `__script__("…")`
//! (OverPy 9.7.10 ABI, `src/compiler/tokenizer.ts`) is a script macro: the
//! script path resolves relative to the definition file (missing files are a
//! `script-not-found` diagnostic, mirroring the reference's ENOENT failure),
//! and each expansion runs the script through [`crate::macro_js::MacroRuntime`]
//! with the call-site arguments injected as `var <name>=<raw>;` declarations
//! (the reference's `resolveMacro`). The string completion value is lexed
//! back into the token stream at the call site, with the reference's
//! per-line indentation rule applied to the text; the frontend token model
//! makes indentation unobservable (the parser never consumes it), so the rule
//! is preserved in the expansion text only. Runtime failures map to the
//! structured `script-*` diagnostics with the script path, line, and column.
//!
//! `#!postCompileHook "hook.js"` registers the post-compile hook script
//! (duplicate declarations are rejected like the reference). The frontend
//! recognizes, parses, validates, and records the directive only — it never
//! executes the hook: real hook execution receives the final Workshop text
//! produced by lowering and is lowering-dependent (workshop-rs emission,
//! issue #8); the frontend never fabricates a Workshop payload.
//!
//! Boundary: `__script__` macros expand at compile time through the runtime
//! (source-supported); `#!postCompileHook` is recorded and executed only
//! against the real Workshop output (lowering-dependent). The runtime's hook
//! ABI is tested separately on synthetic content in the internal macro runtime
//! module (see its `hooks` test suite).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diag::{OpyError, OpyResult, Span};
use crate::hir::types::{
    DirectiveRecord, DirectiveValue, OptimizationState, PreprocessingSnapshot, PreprocessingState,
    TranslationState,
};
use crate::lexer::{LexInput, Token, TokenKind, lex};
use crate::settings::SettingsBlock;

mod directives;
mod macros;
mod project;
mod scripts;

use macros::MacroDef;
use project::{display_path, first_main_file_directive};

/// A recorded preprocessing define (HIR provenance).
#[derive(Debug, Clone, PartialEq)]
pub struct DefineRecord {
    pub name: String,
    pub is_function: bool,
    pub is_member: bool,
    pub span: Option<Span>,
}

/// A resolved `__script__("…")` macro backing.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMacro {
    /// The resolved project-relative script path, used for diagnostics and
    /// runtime attribution.
    pub path: String,
    /// The script text, read at the define site.
    pub source: String,
}

/// A registered `#!postCompileHook` script (the declaration record).
///
/// The frontend recognizes, parses, validates, and records the directive; it
/// never executes the hook. Execution against the final Workshop text is
/// lowering-dependent (issue #8).
#[derive(Debug, Clone, PartialEq)]
pub struct PostCompileHook {
    /// The resolved project-relative script path.
    pub path: String,
    /// The script text, read at the directive site.
    pub source: String,
    /// The directive's source span, used for error attribution.
    pub span: Span,
}

/// The result of preprocessing.
#[derive(Debug, Clone)]
pub struct Preprocessed {
    /// The expanded, single-file token stream.
    pub tokens: Vec<Token>,
    /// The recorded defines in definition order.
    pub defines: Vec<DefineRecord>,
    /// The project `settings { ... }` block, when present (#86).
    pub settings: Option<SettingsBlock>,
    /// Warnings emitted while composing the project.
    pub warnings: Vec<PreprocessWarning>,
    /// The registered `#!postCompileHook` script, when declared.
    pub post_compile_hook: Option<PostCompileHook>,
    /// Frontend-visible preprocessing state; backend effects are not run.
    pub preprocessing: PreprocessingState,
}

/// The output file registry, in include order, preserving source provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRecord {
    pub id: u32,
    pub path: String,
}

/// A source-attributed preprocessing warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessWarning {
    pub code: String,
    pub message: String,
    pub span: Span,
}

/// Preprocess the main source text with its include root.
pub fn preprocess(
    main_text: &str,
    main_path: &str,
    root: &Path,
) -> OpyResult<(Preprocessed, Vec<FileRecord>)> {
    preprocess_with_overlay(main_text, main_path, root, &BTreeMap::new())
}

/// Preprocess with open-document overlays: includes resolve to overlay text
/// (keyed by the include string or the resolved canonical path) before the
/// filesystem. Overlays model unsaved editor buffers without changing the
/// compiler's source-loading contract.
pub fn preprocess_with_overlay(
    main_text: &str,
    main_path: &str,
    root: &Path,
    overlay: &BTreeMap<String, String>,
) -> OpyResult<(Preprocessed, Vec<FileRecord>)> {
    preprocess_with_overlay_outcome(main_text, main_path, root, overlay).result
}

/// The outcome of preprocessing with overlays, retaining the file registry
/// registered so far even when a directive or expansion fails, so callers can
/// map an error's span file id to its actual source.
pub struct PreprocessOutcome {
    pub result: OpyResult<(Preprocessed, Vec<FileRecord>)>,
    pub files: Vec<FileRecord>,
    pub warnings: Vec<PreprocessWarning>,
}

/// Preprocess with open-document overlays while retaining the file registry
/// registered so far on failure.
pub fn preprocess_with_overlay_outcome(
    main_text: &str,
    main_path: &str,
    root: &Path,
    overlay: &BTreeMap<String, String>,
) -> PreprocessOutcome {
    let resolved_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut pre = Preprocessor {
        files: vec![FileRecord {
            id: 0,
            path: main_path.to_string(),
        }],
        next_file_id: 1,
        root: resolved_root.clone(),
        display_root: resolved_root,
        overlay: overlay.clone(),
        include_stack: Vec::new(),
        imported_files: BTreeSet::new(),
        macros: Vec::new(),
        defines: Vec::new(),
        post_compile_hook: None,
        settings: None,
        warnings: Vec::new(),
        preprocessing: PreprocessingState::default(),
    };
    let mut owned_main_text = None;
    let mut source_file_id = 0;
    let first_line = main_text.lines().next().unwrap_or_default();
    if first_line.trim_start().starts_with("#!mainFile")
        && first_main_file_directive(main_text).is_none()
    {
        let span = Span::new(
            0,
            crate::diag::Position::new(1, 1),
            crate::diag::Position::new(1, first_line.chars().count() as u32 + 1),
        );
        return PreprocessOutcome {
            result: Err(OpyError::at(
                "main-file-invalid",
                "`#!mainFile` expects one quoted path on the first line",
                span,
            )),
            files: pre.files,
            warnings: pre.warnings,
        };
    }
    if let Some((main_file, span)) = first_main_file_directive(main_text) {
        let candidate = pre.root.join(&main_file);
        let canonical = std::fs::canonicalize(&candidate).ok();
        let overlay_text = overlay
            .get(&main_file)
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| overlay.get(&path.to_string_lossy().into_owned()))
            })
            .cloned();
        let (text, canonical_path, new_root) = match overlay_text {
            Some(text) => {
                let new_root = candidate
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| pre.root.clone());
                (text, canonical, new_root)
            }
            None => {
                let Some(canonical) = canonical else {
                    return PreprocessOutcome {
                        result: Err(OpyError::at(
                            "main-file-not-found",
                            format!("cannot find main file '{main_file}'"),
                            span,
                        )),
                        files: pre.files,
                        warnings: pre.warnings,
                    };
                };
                let text = match std::fs::read_to_string(&canonical) {
                    Ok(text) => text,
                    Err(error) => {
                        return PreprocessOutcome {
                            result: Err(OpyError::at(
                                "main-file-not-found",
                                format!("cannot read main file '{main_file}': {error}"),
                                span,
                            )),
                            files: pre.files,
                            warnings: pre.warnings,
                        };
                    }
                };
                let new_root = canonical
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| pre.root.clone());
                (text, Some(canonical), new_root)
            }
        };
        let display_path =
            display_path(&candidate, canonical_path.as_deref(), &new_root, &main_file);
        owned_main_text = Some(text);
        source_file_id = 1;
        pre.files.push(FileRecord {
            id: source_file_id,
            path: display_path,
        });
        pre.next_file_id = 2;
        pre.root = new_root.clone();
        pre.display_root = new_root;
        pre.preprocessing.main_file = Some(DirectiveValue {
            value: main_file.clone(),
            span: Some(span.into()),
        });
        pre.record("mainFile", Some(&main_file), span);
    }
    let source_text = owned_main_text.as_deref().unwrap_or(main_text);
    // The project settings block is extracted before lexing and blanked out of
    // the owning file's lexed text, so the lexer never sees its braces (#86).
    let settings = match crate::settings::find_blocks(source_text, source_file_id) {
        Ok(mut blocks) => blocks.pop(),
        Err(error) => {
            return PreprocessOutcome {
                result: Err(error),
                files: pre.files,
                warnings: pre.warnings,
            };
        }
    };
    pre.settings = settings.clone();
    let tokens = match &settings {
        Some(block) => {
            let sanitized = crate::settings::sanitize_for_lex(source_text, block);
            lex(LexInput {
                file_id: source_file_id,
                text: &sanitized,
            })
        }
        None => lex(LexInput {
            file_id: source_file_id,
            text: source_text,
        }),
    };
    let mut tokens = match tokens {
        Ok(tokens) => tokens,
        Err(error) => {
            return PreprocessOutcome {
                result: Err(error),
                files: pre.files,
                warnings: pre.warnings,
            };
        }
    };
    if let Err(error) = pre.process_directives(&mut tokens, false) {
        return PreprocessOutcome {
            result: Err(error),
            files: pre.files,
            warnings: pre.warnings,
        };
    }
    match pre.expand(tokens) {
        Ok(tokens) => {
            let settings = match pre
                .settings
                .take()
                .map(|block| pre.expand_settings(block))
                .transpose()
            {
                Ok(settings) => settings,
                Err(error) => {
                    return PreprocessOutcome {
                        result: Err(error),
                        files: pre.files,
                        warnings: pre.warnings,
                    };
                }
            };
            let result = Ok((
                Preprocessed {
                    tokens,
                    defines: pre.defines,
                    settings,
                    warnings: pre.warnings.clone(),
                    post_compile_hook: pre.post_compile_hook,
                    preprocessing: pre.preprocessing,
                },
                pre.files.clone(),
            ));
            PreprocessOutcome {
                result,
                files: pre.files,
                warnings: pre.warnings,
            }
        }
        Err(error) => PreprocessOutcome {
            result: Err(error),
            files: pre.files,
            warnings: pre.warnings,
        },
    }
}

fn render_tokens(tokens: &[Token]) -> String {
    let mut rendered = String::new();
    let mut previous: Option<&Token> = None;
    for token in tokens {
        if token.kind == TokenKind::Eof {
            continue;
        }
        if let Some(previous) = previous
            && can_merge_without_separator(previous.kind, token.kind)
        {
            rendered.push(' ');
        }
        if token.kind == TokenKind::Newline {
            rendered.push('\n');
        } else if token.kind == TokenKind::String {
            rendered.push('"');
            rendered.push_str(token.raw.as_deref().unwrap_or(&token.text));
            rendered.push('"');
        } else {
            rendered.push_str(&token.text);
        }
        previous = Some(token);
    }
    rendered
}

fn can_merge_without_separator(previous: TokenKind, current: TokenKind) -> bool {
    matches!(previous, TokenKind::Ident | TokenKind::Number)
        && matches!(current, TokenKind::Ident | TokenKind::Number)
}

fn shift_settings_span(span: Span, origin: crate::diag::Position) -> Span {
    fn shift(
        position: crate::diag::Position,
        origin: crate::diag::Position,
    ) -> crate::diag::Position {
        crate::diag::Position::new(
            origin.line + position.line.saturating_sub(1),
            if position.line == 1 {
                origin.col + position.col.saturating_sub(1)
            } else {
                position.col
            },
        )
    }
    Span::new(
        span.file,
        shift(span.start, origin),
        shift(span.end, origin),
    )
}

struct Preprocessor {
    files: Vec<FileRecord>,
    next_file_id: u32,
    root: PathBuf,
    display_root: PathBuf,
    overlay: BTreeMap<String, String>,
    include_stack: Vec<PathBuf>,
    imported_files: BTreeSet<PathBuf>,
    macros: Vec<MacroDef>,
    settings: Option<SettingsBlock>,
    defines: Vec<DefineRecord>,
    post_compile_hook: Option<PostCompileHook>,
    warnings: Vec<PreprocessWarning>,
    preprocessing: PreprocessingState,
}

impl Preprocessor {
    /// Apply the same token macro expansion used by ordinary OPY source to
    /// the extracted settings values. Settings are removed before the source
    /// token stream is processed, so this pass is the point where `#!define`
    /// values become visible to the settings parser.
    fn expand_settings(&self, block: SettingsBlock) -> OpyResult<SettingsBlock> {
        let tokens = lex(LexInput {
            file_id: block.span.file,
            text: &block.text,
        })?;
        let mut tokens = tokens;
        for token in &mut tokens {
            token.span = shift_settings_span(token.span, block.text_start);
        }
        let tokens = self.expand(tokens)?;
        Ok(SettingsBlock {
            text: render_tokens(&tokens),
            ..block
        })
    }
}

#[cfg(test)]
mod tests {
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
        // The block never enters the token stream.
        assert!(
            !pre.tokens.iter().any(|t| t.text.contains("gamemodes")),
            "settings content must not be lexed"
        );
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
        let overlay =
            BTreeMap::from([("shared.opy".to_string(), "#!define VALUE 2\n".to_string())]);
        let main =
            "#!include \"shared.opy\"\n#!include \"shared.opy\"\nrule \"r\":\n    x = VALUE\n";
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
        // Scoped settings lexing must not consume expression-level braces.
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
}
