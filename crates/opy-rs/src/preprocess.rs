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
    /// Filesystem paths preprocessing probed or read, in the form each was
    /// accessed plus the resolved form when a probe canonicalized it.
    pub accessed: BTreeSet<PathBuf>,
}

impl PreprocessOutcome {
    fn failure(error: OpyError, pre: Preprocessor) -> Self {
        Self {
            result: Err(error),
            files: pre.files,
            display_root: pre.display_root,
            warnings: pre.warnings,
            accessed: pre.accessed,
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
        accessed: BTreeSet::from([root.to_path_buf(), resolved_root.clone()]),
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
        pre.record_access(&candidate, canonical.as_deref());
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
        accessed: pre.accessed,
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
    accessed: BTreeSet<PathBuf>,
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
            self.accessed.insert(po_path.clone());
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
        self.record_access(&candidate, canonical.as_deref());
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
        self.record_access(candidate, canonical.as_deref());
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
mod tests;
