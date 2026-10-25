use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diag::{OpyError, OpyResult, Span};
use crate::hir::types::{
    DirectiveRecord, DirectiveValue, OptimizationState, PreprocessingSnapshot, PreprocessingState,
    TranslationEntry, TranslationState,
};
use crate::lexer::{LexInput, Token, TokenKind, lex};
use crate::settings::SettingsBlock;

mod directives;
pub(crate) mod macros;
mod project;
mod scripts;

use macros::{MacroArgument, MacroDef};
use project::{display_path, first_main_file_directive};

#[derive(Debug, Clone, PartialEq)]
pub struct DefineRecord {
    pub name: String,
    pub is_function: bool,
    pub is_member: bool,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMacro {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostCompileHook {
    pub path: String,
    pub source: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Preprocessed {
    pub tokens: Vec<Token>,
    pub defines: Vec<DefineRecord>,
    pub(crate) text_macros: Vec<macros::MacroDef>,
    pub settings: Option<SettingsBlock>,
    pub warnings: Vec<PreprocessWarning>,
    pub post_compile_hook: Option<PostCompileHook>,
    pub preprocessing: PreprocessingState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileRecord {
    pub id: u32,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessWarning {
    pub code: String,
    pub message: String,
    pub span: Span,
}

/// The `#!mainFile` redirect target declared on `text`'s first line, if any —
/// the same directive the preprocessor honors when the text is checked as an
/// entry. Callers grouping documents by their effective entry (for example a
/// provider checking a document set) can use this to avoid re-running one
/// identical project parse per document.
pub fn main_file_directive(text: &str) -> Option<String> {
    first_main_file_directive(text).map(|(value, _)| value)
}

/// The `#!include` targets declared in `text`, in source order — the same
/// directive spellings (`"..."` or `'...'`) the preprocessor honors. Callers
/// mapping a document set's include graph (for example a provider ordering
/// entry checks before member fallbacks) can use this without reimplementing
/// the directive syntax. A text that fails to lex contributes no targets.
pub fn include_directives(text: &str) -> Vec<String> {
    let Ok(tokens) = lex(LexInput { file_id: 0, text }) else {
        return Vec::new();
    };
    tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Directive)
        .filter_map(|token| {
            let text = token.text.trim();
            let (name, rest) = directives::split_directive(text);
            (name == "include")
                .then(|| rest.trim())
                .and_then(directives::strip_quoted)
                .map(str::to_string)
        })
        .collect()
}

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
    /// The directory the file-registry `display_path`s resolve against — the
    /// `#!mainFile` effective directory when the first line redirects the
    /// project root, otherwise the canonicalized input root.
    pub display_root: PathBuf,
    pub warnings: Vec<PreprocessWarning>,
}

impl PreprocessOutcome {
    fn failure(error: OpyError, pre: Preprocessor) -> Self {
        Self {
            result: Err(error),
            files: pre.files,
            display_root: pre.display_root,
            warnings: pre.warnings,
        }
    }
}

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
        root: resolved_root.clone(),
        display_root: resolved_root,
        overlay: overlay.clone(),
        source_texts: BTreeMap::from([(0, main_text.to_string())]),
        line_offsets: std::cell::RefCell::new(BTreeMap::new()),
        include_stack: Vec::new(),
        last_macro_include_path: None,
        imported_files: BTreeSet::new(),
        macros: Vec::new(),
        defines: Vec::new(),
        post_compile_hook: None,
        settings: None,
        warnings: Vec::new(),
        preprocessing: PreprocessingState::default(),
    };
    pre.preprocessing
        .source_file_initial_optimization
        .insert(0, pre.preprocessing.optimization.clone());
    let mut owned_main_text = None;
    let mut source_file_id = 0;
    let first_line = main_text.lines().next().unwrap_or_default();
    let main_file_directive = first_main_file_directive(main_text);
    if first_line.trim_start().starts_with("#!mainFile") && main_file_directive.is_none() {
        let span = Span::new(
            0,
            crate::diag::Position::new(1, 1),
            crate::diag::Position::new(1, first_line.chars().count() as u32 + 1),
        );
        return PreprocessOutcome::failure(
            OpyError::at(
                "main-file-invalid",
                "`#!mainFile` expects one quoted path on the first line",
                span,
            ),
            pre,
        );
    }
    if let Some((main_file, span)) = main_file_directive {
        let candidate = pre.root.join(&main_file);
        let canonical = std::fs::canonicalize(&candidate).ok();
        let overlay_text = overlay
            .get(&main_file)
            .or_else(|| overlay.get(candidate.to_string_lossy().as_ref()))
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| overlay.get(path.to_string_lossy().as_ref()))
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
                    return PreprocessOutcome::failure(
                        OpyError::at(
                            "main-file-not-found",
                            format!("cannot find main file '{main_file}'"),
                            span,
                        ),
                        pre,
                    );
                };
                let text = match std::fs::read_to_string(&canonical) {
                    Ok(text) => text,
                    Err(error) => {
                        return PreprocessOutcome::failure(
                            OpyError::at(
                                "main-file-not-found",
                                format!("cannot read main file '{main_file}': {error}"),
                                span,
                            ),
                            pre,
                        );
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
        source_file_id = 1;
        pre.source_texts.insert(source_file_id, text.clone());
        owned_main_text = Some(text);
        pre.files.push(FileRecord {
            id: source_file_id,
            path: display_path,
        });
        pre.preprocessing
            .source_file_initial_optimization
            .insert(source_file_id, pre.preprocessing.optimization.clone());
        pre.root = new_root.clone();
        pre.display_root = new_root;
        pre.preprocessing.main_file = Some(DirectiveValue {
            value: main_file.clone(),
            span: Some(span.into()),
        });
        pre.record("mainFile", Some(&main_file), span);
    }
    let source_text = owned_main_text.as_deref().unwrap_or(main_text);
    let settings = match crate::settings::find_blocks(source_text, source_file_id) {
        Ok(mut blocks) => blocks.pop(),
        Err(error) => return PreprocessOutcome::failure(error, pre),
    };
    let settings = match settings
        .map(|block| pre.resolve_settings_source(block))
        .transpose()
    {
        Ok(settings) => settings,
        Err(error) => return PreprocessOutcome::failure(error, pre),
    };
    let sanitized = settings
        .as_ref()
        .map(|block| crate::settings::sanitize_for_lex(source_text, block));
    let mut tokens = match lex(LexInput {
        file_id: source_file_id,
        text: sanitized.as_deref().unwrap_or(source_text),
    }) {
        Ok(tokens) => tokens,
        Err(error) => return PreprocessOutcome::failure(error, pre),
    };
    if let Err(error) = pre.process_directives(&mut tokens, false, settings) {
        return PreprocessOutcome::failure(error, pre);
    }
    if let Err(error) = pre.load_translation_catalog() {
        return PreprocessOutcome::failure(error, pre);
    }
    let result = Ok((
        Preprocessed {
            tokens,
            defines: pre.defines,
            text_macros: pre.macros,
            settings: pre.settings,
            warnings: pre.warnings.clone(),
            post_compile_hook: pre.post_compile_hook,
            preprocessing: pre.preprocessing,
        },
        pre.files.clone(),
    ));
    PreprocessOutcome {
        result,
        files: pre.files,
        display_root: pre.display_root,
        warnings: pre.warnings,
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
    Span::new(
        span.file,
        crate::diag::shift_position(span.start, origin),
        crate::diag::shift_position(span.end, origin),
    )
}

fn external_text_span(file: u32, text: &str) -> Span {
    let line = text.matches('\n').count() as u32 + 1;
    let col = text
        .rsplit('\n')
        .next()
        .map_or(1, |tail| tail.chars().count() as u32 + 1);
    Span::new(
        file,
        crate::diag::Position::new(1, 1),
        crate::diag::Position::new(line, col),
    )
}

struct Preprocessor {
    files: Vec<FileRecord>,
    root: PathBuf,
    display_root: PathBuf,
    overlay: BTreeMap<String, String>,
    source_texts: BTreeMap<u32, String>,
    line_offsets: std::cell::RefCell<BTreeMap<u32, macros::LineOffsets>>,
    include_stack: Vec<PathBuf>,
    // OverPy retains this file context after multiline macro expansion in an
    // included settings block; established projects rely on that lookup base.
    last_macro_include_path: Option<PathBuf>,
    imported_files: BTreeSet<PathBuf>,
    macros: Vec<MacroDef>,
    settings: Option<SettingsBlock>,
    defines: Vec<DefineRecord>,
    post_compile_hook: Option<PostCompileHook>,
    warnings: Vec<PreprocessWarning>,
    preprocessing: PreprocessingState,
}

impl Preprocessor {
    fn load_translation_catalog(&mut self) -> OpyResult<()> {
        let Some(translations) = self.preprocessing.translations.as_ref() else {
            return Ok(());
        };
        let languages = translations.languages.clone();
        let source_path = self
            .files
            .get(1)
            .or_else(|| self.files.first())
            .map(|file| PathBuf::from(&file.path))
            .unwrap_or_else(|| PathBuf::from("main.opy"));
        let base = self.root.join(
            source_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("main.opy")),
        );
        let mut entries = BTreeMap::<(Option<String>, String), TranslationEntry>::new();
        for language in languages.iter().skip(1) {
            let po_path = base.with_extension(format!("{language}.po"));
            let Some(text) = std::fs::read_to_string(&po_path).ok() else {
                continue;
            };
            let file_id = self.register_external_file(&po_path, language, text.clone());
            let parsed = parse_po(&text, language, external_text_span(file_id, &text))?;
            for entry in parsed {
                let key = (entry.context.clone(), entry.msgid.clone());
                entries
                    .entry(key)
                    .and_modify(|existing| existing.translations.extend(entry.translations.clone()))
                    .or_insert(entry);
            }
        }
        if let Some(translations) = self.preprocessing.translations.as_mut() {
            translations.entries = entries.into_values().collect();
        }
        Ok(())
    }

    pub(super) fn resolve_settings_source(
        &mut self,
        mut block: SettingsBlock,
    ) -> OpyResult<SettingsBlock> {
        let Some(requested) = block.external_path.clone() else {
            return Ok(block);
        };
        let requested = requested.replace('\\', "/");
        let candidate = self.include_base().join(&requested);
        let candidate_path = candidate.to_string_lossy().into_owned();
        let canonical = std::fs::canonicalize(&candidate).ok();
        let lexical_path = candidate.to_string_lossy().replace('\\', "/");
        let content = self
            .overlay
            .get(&requested)
            .or_else(|| self.overlay.get(&candidate_path))
            .or_else(|| self.overlay.get(&lexical_path))
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| self.overlay.get(&path.to_string_lossy().into_owned()))
            })
            .cloned()
            .or_else(|| canonical.and_then(|path| std::fs::read_to_string(path).ok()))
            .ok_or_else(|| {
                OpyError::at(
                    "settings-not-found",
                    format!("cannot find external settings file '{requested}'"),
                    block.keyword_span,
                )
            })?;
        let file_id = self.register_external_file(&candidate, &requested, content.clone());
        block.content_file = file_id;
        block.text_start = crate::diag::Position::new(1, 1);
        block.content_span = external_text_span(file_id, &content);
        block.text = content;
        Ok(block)
    }

    fn register_external_file(&mut self, candidate: &Path, requested: &str, text: String) -> u32 {
        let canonical = std::fs::canonicalize(candidate).ok();
        let file_id = self.files.len() as u32;
        self.source_texts.insert(file_id, text);
        self.files.push(FileRecord {
            id: file_id,
            path: display_path(
                candidate,
                canonical.as_deref(),
                &self.display_root,
                requested,
            ),
        });
        file_id
    }

    pub(super) fn expand_settings(&self, block: SettingsBlock) -> OpyResult<SettingsBlock> {
        // A block that does not lex — an empty radix literal such as `0x`,
        // which the pinned tokenizer still reads as one word token — carries
        // no expandable content either; the raw text reaches the settings
        // parser so those elements can emit `NaN` (#512).
        let Ok(tokens) = lex(LexInput {
            file_id: block.content_file,
            text: &block.text,
        }) else {
            return Ok(block);
        };
        let mut tokens = tokens;
        for token in &mut tokens {
            token.span = shift_settings_span(token.span, block.text_start);
            token.layout = token.span;
        }
        let tokens = self.expand(tokens)?;
        Ok(SettingsBlock {
            text: render_tokens(&tokens),
            ..block
        })
    }
}

fn parse_po(text: &str, language: &str, span: Span) -> OpyResult<Vec<TranslationEntry>> {
    let mut header_language = None;
    let mut entries = Vec::new();
    let mut current: Option<(Option<String>, String, String)> = None;
    let mut field: Option<&str> = None;
    for line in text.lines().chain(std::iter::once("")) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some((context, msgid, msgstr)) = current.take()
                && !msgid.is_empty()
            {
                let mut translations = BTreeMap::new();
                translations.insert(language.to_string(), msgstr);
                entries.push(TranslationEntry {
                    msgid,
                    context,
                    translations,
                });
            }
            field = None;
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('"') {
            let value = po_quoted(trimmed, span)?;
            let Some((context, msgid, msgstr)) = current.as_mut() else {
                return Err(OpyError::at(
                    "translations-invalid",
                    "PO continuation has no preceding field",
                    span,
                ));
            };
            match field {
                Some("msgid") => msgid.push_str(&value),
                Some("msgstr") => msgstr.push_str(&value),
                Some("msgctxt") => context.get_or_insert_default().push_str(&value),
                _ => {
                    return Err(OpyError::at(
                        "translations-invalid",
                        "PO continuation has no recognized field",
                        span,
                    ));
                }
            }
            if msgid.is_empty()
                && let Some(language) = po_language(msgstr)
            {
                header_language = Some(language);
            }
            continue;
        }
        let (name, value) = trimmed
            .split_once(' ')
            .ok_or_else(|| OpyError::at("translations-invalid", "malformed PO entry", span))?;
        let value = po_quoted(value, span)?;
        if !matches!(name, "msgctxt" | "msgid" | "msgstr") {
            return Err(OpyError::at(
                "translations-invalid",
                format!("unsupported PO field '{name}'"),
                span,
            ));
        }
        let entry = current.get_or_insert_with(|| (None, String::new(), String::new()));
        match name {
            "msgctxt" => entry.0 = Some(value),
            "msgid" => entry.1 = value,
            _ => entry.2 = value,
        }
        field = Some(name);
    }
    if header_language.as_deref() != Some(language) {
        return Err(OpyError::at(
            "translations-invalid",
            format!("PO language header does not match '{language}'"),
            span,
        ));
    }
    Ok(entries)
}

fn po_quoted(text: &str, span: Span) -> OpyResult<String> {
    serde_json::from_str::<String>(text).map_err(|error| {
        OpyError::at(
            "translations-invalid",
            format!("malformed PO string: {error}"),
            span,
        )
    })
}

fn po_language(msgstr: &str) -> Option<String> {
    msgstr
        .lines()
        .filter_map(|line| line.strip_prefix("Language:").map(str::trim))
        .next_back()
        .map(str::to_string)
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
}
