//! `.opy` preprocessing: includes, `#!define` macros (textual and
//! `__script__` JavaScript-backed), `#!postCompileHook`, and expansion.
//!
//! Operates at the token level, matching the reference frontend's observable
//! behavior: `#!include "file.opy"` splices the included file's tokens at the
//! directive site; `#!define NAME value` and `#!define name(args) value`
//! register macros that expand at their use sites, recursively (a macro may
//! reference earlier macros). The output is a single-file token stream whose
//! spans point at use sites, mirroring the reference adapter's provenance
//! convention (the HIR file registry keeps the main file). Invalid include
//! graphs (cycles, missing files) and recursive defines fail deterministically
//! with structured diagnostics that name the offending file/line.
//!
//! # JavaScript macros and hooks
//!
//! A function-like define whose replacement starts with `__script__("…")`
//! (OverPy 9.7.10 ABI, `src/compiler/tokenizer.ts`) is a script macro: the
//! script path resolves root-relative at the define site (missing files are a
//! `script-not-found` diagnostic, mirroring the reference's ENOENT failure),
//! and each expansion runs the script through [`opy_macro_js::MacroRuntime`]
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
//! (frontend-supported); `#!postCompileHook` is recorded and executed only
//! against the real Workshop output (lowering-dependent). The runtime's hook
//! ABI is tested separately on synthetic content in `opy-macro-js` (see its
//! `hooks` test suite).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use opy_macro_js::{Limits, MacroArg, MacroError, MacroRuntime};

use crate::diag::{FrontendError, FrontendResult, Span};
use crate::hir::types::{
    DirectiveRecord, DirectiveValue, OptimizationState, PreprocessingSnapshot, PreprocessingState,
    TranslationState,
};
use crate::lexer::{LexInput, Token, TokenKind, lex};
use crate::settings::SettingsBlock;

/// A recorded preprocessing define (HIR provenance).
#[derive(Debug, Clone, PartialEq)]
pub struct DefineRecord {
    pub name: String,
    pub is_function: bool,
    pub span: Option<Span>,
}

/// A resolved `__script__("…")` macro backing.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMacro {
    /// The script path as declared (root-relative), used for diagnostics and
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
    /// The script path as declared (root-relative).
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
    /// The top-of-file `settings { ... }` block, when present (#86).
    pub settings: Option<SettingsBlock>,
    /// The registered `#!postCompileHook` script, when declared.
    pub post_compile_hook: Option<PostCompileHook>,
    /// Frontend-visible preprocessing state; backend effects are not run.
    pub preprocessing: PreprocessingState,
}

/// The output file registry: the main file only (reference convention).
#[derive(Debug, Clone, PartialEq)]
pub struct FileRecord {
    pub id: u32,
    pub path: String,
}

/// Preprocess the main source text with its include root.
pub fn preprocess(
    main_text: &str,
    main_path: &str,
    root: &Path,
) -> FrontendResult<(Preprocessed, Vec<FileRecord>)> {
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
) -> FrontendResult<(Preprocessed, Vec<FileRecord>)> {
    preprocess_with_overlay_outcome(main_text, main_path, root, overlay).result
}

/// The outcome of preprocessing with overlays, retaining the file registry
/// registered so far even when a directive or expansion fails, so callers can
/// map an error's span file id to its actual source.
pub struct PreprocessOutcome {
    pub result: FrontendResult<(Preprocessed, Vec<FileRecord>)>,
    pub files: Vec<FileRecord>,
}

/// Preprocess with open-document overlays while retaining the file registry
/// registered so far on failure.
pub fn preprocess_with_overlay_outcome(
    main_text: &str,
    main_path: &str,
    root: &Path,
    overlay: &BTreeMap<String, String>,
) -> PreprocessOutcome {
    let mut pre = Preprocessor {
        files: vec![FileRecord {
            id: 0,
            path: main_path.to_string(),
        }],
        next_file_id: 1,
        root: root.to_path_buf(),
        overlay: overlay.clone(),
        include_stack: Vec::new(),
        macros: Vec::new(),
        defines: Vec::new(),
        post_compile_hook: None,
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
            result: Err(FrontendError::at(
                "main-file-invalid",
                "`#!mainFile` expects one quoted path on the first line",
                span,
            )),
            files: pre.files,
        };
    }
    if let Some((main_file, span)) = first_main_file_directive(main_text) {
        let candidate = root.join(&main_file);
        let canonical = std::fs::canonicalize(&candidate).ok();
        let overlay_text = overlay
            .get(&main_file)
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| overlay.get(&path.to_string_lossy().into_owned()))
            })
            .cloned();
        let (text, display_path, new_root) = match overlay_text {
            Some(text) => {
                let display_path = candidate.to_string_lossy().into_owned();
                let new_root = candidate
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
                (text, display_path, new_root)
            }
            None => {
                let Some(canonical) = canonical else {
                    return PreprocessOutcome {
                        result: Err(FrontendError::at(
                            "main-file-not-found",
                            format!("cannot find main file '{main_file}'"),
                            span,
                        )),
                        files: pre.files,
                    };
                };
                let text = match std::fs::read_to_string(&canonical) {
                    Ok(text) => text,
                    Err(error) => {
                        return PreprocessOutcome {
                            result: Err(FrontendError::at(
                                "main-file-not-found",
                                format!("cannot read main file '{main_file}': {error}"),
                                span,
                            )),
                            files: pre.files,
                        };
                    }
                };
                let new_root = canonical
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
                (text, canonical.to_string_lossy().into_owned(), new_root)
            }
        };
        owned_main_text = Some(text);
        source_file_id = 1;
        pre.files.push(FileRecord {
            id: source_file_id,
            path: display_path,
        });
        pre.next_file_id = 2;
        pre.root = new_root;
        pre.preprocessing.main_file = Some(DirectiveValue {
            value: main_file.clone(),
            span: Some(span.into()),
        });
        pre.record("mainFile", Some(&main_file), span);
    }
    let source_text = owned_main_text.as_deref().unwrap_or(main_text);
    // The top-of-file settings block is extracted before lexing and blanked
    // out of the lexed text, so the lexer never sees the block's braces
    // (scoped settings lexing, #86).
    let settings = match crate::settings::find_blocks(source_text, source_file_id) {
        Ok(mut blocks) => blocks.pop(),
        Err(error) => {
            return PreprocessOutcome {
                result: Err(error),
                files: pre.files,
            };
        }
    };
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
            };
        }
    };
    if let Err(error) = pre.process_directives(&mut tokens) {
        return PreprocessOutcome {
            result: Err(error),
            files: pre.files,
        };
    }
    match pre.expand(tokens) {
        Ok(tokens) => {
            let result = Ok((
                Preprocessed {
                    tokens,
                    defines: pre.defines,
                    settings,
                    post_compile_hook: pre.post_compile_hook,
                    preprocessing: pre.preprocessing,
                },
                pre.files.clone(),
            ));
            PreprocessOutcome {
                result,
                files: pre.files,
            }
        }
        Err(error) => PreprocessOutcome {
            result: Err(error),
            files: pre.files,
        },
    }
}

struct Preprocessor {
    files: Vec<FileRecord>,
    next_file_id: u32,
    root: PathBuf,
    overlay: BTreeMap<String, String>,
    include_stack: Vec<PathBuf>,
    macros: Vec<MacroDef>,
    defines: Vec<DefineRecord>,
    post_compile_hook: Option<PostCompileHook>,
    preprocessing: PreprocessingState,
}

/// A registered macro: object-like, function-like, or a script macro.
struct MacroDef {
    name: String,
    params: Vec<String>,
    body: Vec<Token>,
    /// True when the body came from a `#!define name(args) value` form.
    is_function: bool,
    /// The resolved `__script__` backing, when the replacement is one.
    script: Option<ScriptMacro>,
}

fn first_main_file_directive(text: &str) -> Option<(String, Span)> {
    let line = text.lines().next()?.trim_end_matches('\r');
    let rest = line.strip_prefix("#!mainFile")?;
    let value = rest.trim();
    let value = strip_quoted(value)?.to_string();
    let end_col = line.chars().count() as u32 + 1;
    Some((
        value,
        Span::new(
            0,
            crate::diag::Position::new(1, 1),
            crate::diag::Position::new(1, end_col),
        ),
    ))
}

impl Preprocessor {
    /// Process `#!` directive tokens, splicing includes and registering
    /// defines. Non-directive tokens are kept in place.
    fn process_directives(&mut self, tokens: &mut Vec<Token>) -> FrontendResult<()> {
        let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
        for token in tokens.drain(..) {
            if token.kind == TokenKind::Directive {
                self.handle_directive(token, &mut out)?;
            } else if token.kind == TokenKind::Ident
                && matches!(token.text.as_str(), "rule" | "def")
                && self.preprocessing.rule_prefix.is_some()
            {
                let prefix = self
                    .preprocessing
                    .rule_prefix
                    .as_ref()
                    .map(|value| value.value.clone())
                    .unwrap_or_default();
                out.push(Token {
                    kind: TokenKind::RulePrefixMarker,
                    text: prefix,
                    span: token.span,
                });
                out.push(token);
            } else {
                out.push(token);
            }
        }
        *tokens = out;
        Ok(())
    }

    fn handle_directive(&mut self, token: Token, out: &mut Vec<Token>) -> FrontendResult<()> {
        let text = token.text.trim();
        let span = token.span;
        let (name, rest) = split_directive(text);
        if name == "include" {
            let rest = rest.trim();
            let include = rest
                .strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .or_else(|| rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')));
            let Some(include) = include else {
                return Err(FrontendError::at(
                    "include-invalid",
                    format!(
                        "invalid include directive: `{text}` (expected `#!include \"file.opy\"`)"
                    ),
                    span,
                ));
            };
            self.include(include, span, out)?;
            return Ok(());
        }
        if name == "define" {
            self.define(rest.trim(), span)?;
            return Ok(());
        }
        if name == "undef" {
            let name = rest.trim();
            if name.is_empty() || name.chars().any(|ch| !is_identifier_char(ch)) {
                return Err(FrontendError::at(
                    "undef-invalid",
                    "malformed `#!undef` directive: expected one macro name",
                    span,
                ));
            }
            self.macros.retain(|m| m.name != name);
            self.defines.retain(|define| define.name != name);
            self.record("undef", Some(name), span);
            return Ok(());
        }
        if name == "postCompileHook" {
            let rest = rest.trim();
            let Some(path) = strip_quoted(rest) else {
                return Err(FrontendError::at(
                    "script-invalid",
                    format!(
                        "invalid postCompileHook directive: `{text}` (expected `#!postCompileHook \"hook.js\"`)"
                    ),
                    span,
                ));
            };
            if self.post_compile_hook.is_some() {
                return Err(FrontendError::at(
                    "post-compile-hook-duplicate",
                    "post-compile hook is already defined".to_string(),
                    span,
                ));
            }
            let hook = self.resolve_script(path, span)?;
            self.post_compile_hook = Some(PostCompileHook {
                path: hook.path,
                source: hook.source,
                span,
            });
            self.record("postCompileHook", Some(path), span);
            return Ok(());
        }
        if name == "mainFile" {
            return Err(FrontendError::at(
                "main-file-placement",
                "`#!mainFile` must be the first directive in the main source",
                span,
            ));
        }
        if name == "allowMacroRedeclaration" {
            self.preprocessing.allow_macro_redeclaration = true;
            self.record(name, None, span);
            return Ok(());
        }
        if name == "translations" {
            let languages = parse_translations(rest.trim(), span)?;
            self.preprocessing.translations = Some(TranslationState {
                languages: languages.clone(),
                span: Some(span.into()),
            });
            self.record(name, Some(&languages.join(" ")), span);
            return Ok(());
        }
        if name == "suppressWarnings" {
            let warnings = parse_words(rest, "suppressWarnings", span)?;
            self.preprocessing
                .suppressed_warnings
                .extend(warnings.clone());
            self.record(name, Some(&warnings.join(" ")), span);
            return Ok(());
        }
        if name == "rulePrefix" {
            let prefix = strip_quoted(rest.trim()).ok_or_else(|| {
                FrontendError::at(
                    "rule-prefix-invalid",
                    "`#!rulePrefix` expects one quoted string",
                    span,
                )
            })?;
            self.preprocessing.rule_prefix = Some(DirectiveValue {
                value: prefix.to_string(),
                span: Some(span.into()),
            });
            self.record(name, Some(prefix), span);
            return Ok(());
        }
        if name == "rulePrefixTemplate" {
            if self.preprocessing.rule_prefix_template.is_some() {
                return Err(FrontendError::at(
                    "rule-prefix-template-duplicate",
                    "a rule prefix template is already defined",
                    span,
                ));
            }
            let template = if rest.trim().is_empty() {
                r#"f"[{$pathTitle.replace('_', ' ')}] {$rule}" if $rule and not $isDelimiter else $rule"#
            } else {
                rest.trim()
            };
            self.preprocessing.rule_prefix_template = Some(DirectiveValue {
                value: template.to_string(),
                span: Some(span.into()),
            });
            self.record(name, Some(template), span);
            return Ok(());
        }
        if let Some((directive, control)) = optimization_directive(name) {
            apply_optimization(&mut self.preprocessing.optimization, control);
            self.record(directive, None, span);
            return Ok(());
        }
        if let Some(replacement) = replacement_directive(name) {
            let family = replacement_family(name).expect("replacement directive family");
            if self
                .preprocessing
                .directives
                .iter()
                .filter_map(|item| replacement_family(&item.name))
                .any(|item_family| item_family == family)
            {
                return Err(FrontendError::at(
                    "replacement-duplicate",
                    format!("a replacement for `{family}` is already defined"),
                    span,
                ));
            }
            self.preprocessing.replacements.push(DirectiveValue {
                value: replacement.to_string(),
                span: Some(span.into()),
            });
            self.record(name, Some(replacement), span);
            return Ok(());
        }
        Err(FrontendError::at(
            "unsupported-directive",
            format!("unsupported preprocessing directive `#!{text}`"),
            span,
        ))
    }

    fn record(&mut self, name: &str, value: Option<&str>, span: Span) {
        let state = PreprocessingSnapshot {
            allow_macro_redeclaration: self.preprocessing.allow_macro_redeclaration,
            optimization: self.preprocessing.optimization.clone(),
            rule_prefix: self
                .preprocessing
                .rule_prefix
                .as_ref()
                .map(|value| value.value.clone()),
            rule_prefix_template: self
                .preprocessing
                .rule_prefix_template
                .as_ref()
                .map(|value| value.value.clone()),
            translations: self
                .preprocessing
                .translations
                .as_ref()
                .map(|translations| translations.languages.clone()),
            replacements: self
                .preprocessing
                .replacements
                .iter()
                .map(|value| value.value.clone())
                .collect(),
        };
        self.preprocessing.directives.push(DirectiveRecord {
            name: name.to_string(),
            value: value.map(str::to_string),
            scope_col: span.start.col,
            scope_depth: self.include_stack.len() as u32,
            state,
            span: Some(span.into()),
        });
    }

    /// Resolve a script path root-relative (the reference's
    /// `getFilePaths(path, rootPath)` convention) and read its text.
    fn resolve_script(&self, path: &str, span: Span) -> FrontendResult<ScriptMacro> {
        let canonical = self.root.join(path).canonicalize().map_err(|_| {
            FrontendError::at(
                "script-not-found",
                format!(
                    "cannot find script '{path}' under root '{}'",
                    self.root.display()
                ),
                span,
            )
        })?;
        let source = std::fs::read_to_string(&canonical).map_err(|error| {
            FrontendError::at(
                "script-not-found",
                format!("cannot read script '{path}': {error}"),
                span,
            )
        })?;
        Ok(ScriptMacro {
            path: path.to_string(),
            source,
        })
    }

    /// Resolve, lex, and splice one included file.
    fn include(&mut self, include: &str, span: Span, out: &mut Vec<Token>) -> FrontendResult<()> {
        // The include base is the root; the main file is the only file in the
        // registry (reference convention), so path resolution is root-based.
        let candidate = self.root.join(include);
        let canonical = std::fs::canonicalize(&candidate).ok();
        // An open-document overlay (an unsaved editor buffer) takes
        // precedence over the filesystem. Overlays are keyed by the include
        // string and by the resolved canonical path, so both spellings work.
        let overlay_text = self
            .overlay
            .get(include)
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| self.overlay.get(&path.to_string_lossy().into_owned()))
            })
            .cloned();

        // The include-cycle identity: the canonical path when the file exists,
        // otherwise the candidate path (overlays may not have a disk backing).
        let identity = canonical.clone().unwrap_or_else(|| candidate.clone());
        if self.include_stack.contains(&identity) {
            return Err(FrontendError::at(
                "include-cycle",
                format!(
                    "include cycle detected: '{}' is already being included",
                    identity.display()
                ),
                span,
            ));
        }

        let text = match overlay_text {
            Some(text) => text,
            None => {
                let canonical = canonical.ok_or_else(|| {
                    FrontendError::at(
                        "include-not-found",
                        format!(
                            "cannot find included file '{include}' under root '{}'",
                            self.root.display()
                        ),
                        span,
                    )
                })?;
                std::fs::read_to_string(&canonical).map_err(|error| {
                    FrontendError::at(
                        "include-not-found",
                        format!("cannot read included file '{include}': {error}"),
                        span,
                    )
                })?
            }
        };
        // Each include registers a file in the registry (reference behavior).
        let file_id = self.next_file_id;
        self.next_file_id += 1;
        self.files.push(FileRecord {
            id: file_id,
            path: include.to_string(),
        });
        self.include_stack.push(identity);
        let saved_prefix = self.preprocessing.rule_prefix.clone();
        let saved_optimization = self.preprocessing.optimization.clone();
        // Settings blocks are only supported in the main file; an included
        // file's block is rejected at its keyword span (file id of the
        // included file, #86).
        match crate::settings::find_blocks(&text, file_id) {
            Err(error) => return Err(error),
            Ok(blocks) if !blocks.is_empty() => {
                return Err(FrontendError::at(
                    "settings-placement",
                    "settings blocks are only supported in the main file".to_string(),
                    blocks[0].keyword_span,
                ));
            }
            Ok(_) => {}
        }
        let mut included = lex(LexInput {
            file_id,
            text: &text,
        })?;
        let processed = self.process_directives(&mut included);
        self.preprocessing.rule_prefix = saved_prefix;
        self.preprocessing.optimization = saved_optimization;
        if let Err(error) = processed {
            self.include_stack.pop();
            return Err(error);
        }
        // Drop the included file's Eof token (it terminates the file, not
        // the spliced stream).
        included.retain(|token| token.kind != TokenKind::Eof);
        // Included tokens keep their real positions so the parser's
        // indentation model works; span comparison is normalized away by the
        // differential suite. File identity beyond the main file is preserved
        // in diagnostics (include cycles/not-found name the real path).
        out.extend(included);
        self.include_stack.pop();
        self.record("include", Some(include), span);
        Ok(())
    }

    /// Register one `#!define` (object- or function-like).
    ///
    /// A define is function-like when `(` immediately follows the name
    /// (`cakeBeam(start, end)`); a parenthesized object-like value
    /// (`#!define X (a + b)`) keeps its parentheses as value tokens.
    fn define(&mut self, rest: &str, span: Span) -> FrontendResult<()> {
        let rest = rest.trim();
        let first_open = rest.find('(').unwrap_or(usize::MAX);
        let first_space = rest.find(char::is_whitespace).unwrap_or(usize::MAX);
        let is_function_like = first_open < first_space;

        let (name, params, body_text) = if is_function_like {
            let name = rest[..first_open].trim();
            let Some(close) = rest[first_open..].find(')') else {
                return Err(FrontendError::at(
                    "define-invalid",
                    format!("malformed function-like define `#!define {rest}`: missing `)`"),
                    span,
                ));
            };
            let close = first_open + close;
            let params: Vec<String> = rest[first_open + 1..close]
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            let body = rest[close + 1..].trim();
            (name.to_string(), params, body.to_string())
        } else {
            let name = rest[..first_space].trim();
            let body = if first_space == usize::MAX {
                String::new()
            } else {
                rest[first_space..].trim().to_string()
            };
            (name.to_string(), Vec::new(), body)
        };
        if name.is_empty() {
            return Err(FrontendError::at(
                "define-invalid",
                "malformed `#!define` directive: missing macro name",
                span,
            ));
        }
        if self.macros.iter().any(|macro_def| macro_def.name == name) {
            if !self.preprocessing.allow_macro_redeclaration {
                return Err(FrontendError::at(
                    "macro-redeclaration",
                    format!("macro '{name}' is already defined"),
                    span,
                ));
            }
            self.macros.retain(|macro_def| macro_def.name != name);
            self.defines.retain(|define| define.name != name);
        }
        let script = if is_function_like && body_text.starts_with("__script__(") {
            // The OverPy script-macro ABI: the replacement is exactly
            // `__script__("path.js")`; the reference extracts the path from
            // the text between the parentheses and resolves it root-relative
            // at the define site (missing files fail at compile time).
            let inner = &body_text["__script__(".len()..];
            let inner = inner.strip_suffix(')').ok_or_else(|| {
                FrontendError::at(
                    "script-invalid",
                    format!(
                        "malformed script macro `#!define {rest}`: expected `__script__(\"path.js\")`"
                    ),
                    span,
                )
            })?;
            let Some(path) = strip_quoted(inner.trim()) else {
                return Err(FrontendError::at(
                    "script-invalid",
                    format!(
                        "malformed script macro `#!define {rest}`: expected a quoted script path"
                    ),
                    span,
                ));
            };
            Some(self.resolve_script(path, span)?)
        } else {
            None
        };
        let body_tokens = lex(LexInput {
            file_id: span.file,
            text: &body_text,
        })?;
        // Drop the trailing EOF token from the value.
        let body_tokens: Vec<Token> = body_tokens
            .into_iter()
            .filter(|t| t.kind != TokenKind::Eof)
            .collect();
        let is_function = is_function_like;
        self.defines.push(DefineRecord {
            name: name.clone(),
            is_function,
            span: Some(span),
        });
        self.macros.push(MacroDef {
            name,
            params,
            body: body_tokens,
            is_function,
            script,
        });
        Ok(())
    }

    /// Expand all macros across the token stream, recursively.
    fn expand(&self, tokens: Vec<Token>) -> FrontendResult<Vec<Token>> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            if token.kind == TokenKind::Ident {
                let name = token.text.clone();
                if let Some(mac) = self.macros.iter().find(|m| m.name == name) {
                    if mac.is_function {
                        // Expect `(` args `)` immediately after the name.
                        let cursor = index + 1;
                        if cursor < tokens.len() && tokens[cursor].kind == TokenKind::LParen {
                            let (args, after) = self.collect_args(&tokens, cursor)?;
                            let mut expanded = self.expand_macro(mac, args, token.span)?;
                            self.expand_into(&mut expanded, &mut Vec::new(), 0)?;
                            out.append(&mut expanded);
                            index = after;
                            continue;
                        }
                        // A function-like macro used without arguments: leave
                        // the name as an ordinary identifier.
                        out.push(token.clone());
                        index += 1;
                        continue;
                    }
                    let mut expanded = self.expand_macro(mac, Vec::new(), token.span)?;
                    self.expand_into(&mut expanded, &mut Vec::new(), 0)?;
                    out.append(&mut expanded);
                    index += 1;
                    continue;
                }
            }
            out.push(token.clone());
            index += 1;
        }
        Ok(out)
    }

    /// Collect the argument token lists of a function-like macro call,
    /// returning `(args, index_after_closing_paren)`.
    fn collect_args(
        &self,
        tokens: &[Token],
        open: usize,
    ) -> FrontendResult<(Vec<Vec<Token>>, usize)> {
        let mut args: Vec<Vec<Token>> = Vec::new();
        let mut current: Vec<Token> = Vec::new();
        let mut depth = 0usize;
        let mut cursor = open + 1;
        while cursor < tokens.len() {
            let kind = tokens[cursor].kind;
            if kind == TokenKind::LParen {
                depth += 1;
                current.push(tokens[cursor].clone());
            } else if kind == TokenKind::RParen {
                if depth == 0 {
                    args.push(std::mem::take(&mut current));
                    return Ok((args, cursor + 1));
                }
                depth -= 1;
                current.push(tokens[cursor].clone());
            } else if kind == TokenKind::Comma && depth == 0 {
                args.push(std::mem::take(&mut current));
            } else {
                current.push(tokens[cursor].clone());
            }
            cursor += 1;
        }
        Err(FrontendError::new(
            "macro-invalid",
            "unterminated macro invocation: missing closing `)`",
        ))
    }

    /// Substitute macro params with the call arguments and stamp every
    /// expanded token with the use-site span.
    ///
    /// Expanded tokens share the use-site span: the differential suite
    /// normalizes spans away, and stamping the whole expansion with one
    /// monotonic span keeps downstream span validation trivially valid.
    fn expand_macro(
        &self,
        mac: &MacroDef,
        args: Vec<Vec<Token>>,
        use_site: Span,
    ) -> FrontendResult<Vec<Token>> {
        if mac.is_function && args.len() != mac.params.len() {
            return Err(FrontendError::at(
                "macro-arity",
                format!(
                    "macro '{}' expects {} argument(s) but got {}",
                    mac.name,
                    mac.params.len(),
                    args.len()
                ),
                use_site,
            ));
        }
        if let Some(script) = &mac.script {
            return self.expand_script(mac, script, args, use_site);
        }
        let mut out = Vec::new();
        for token in &mac.body {
            if mac.is_function
                && token.kind == TokenKind::Ident
                && mac.params.iter().any(|p| p == &token.text)
            {
                let param_index = mac
                    .params
                    .iter()
                    .position(|p| p == &token.text)
                    .expect("checked above");
                let mut replacement = args.get(param_index).cloned().unwrap_or_default();
                for replacement_token in &mut replacement {
                    replacement_token.span = use_site;
                }
                out.extend(replacement);
            } else {
                let mut token = token.clone();
                token.span = use_site;
                out.push(token);
            }
        }
        Ok(out)
    }

    /// Expand a script macro: run the resolved script through the bounded
    /// runtime with the call-site arguments injected, then lex the string
    /// completion value back into the token stream at the use site.
    ///
    /// Argument text is reconstructed from the call-site tokens (see
    /// [`raw_arg_text`]); the reference injects the raw source text, and the
    /// reconstruction is JavaScript-value-equivalent to it (string literals
    /// are re-quoted with JSON escaping, so quoting-style differences are
    /// unobservable to the script). The reference's per-line indentation rule
    /// is applied to the expansion text before lexing; the frontend parser
    /// never consumes indentation, so this is preserved in the text only.
    fn expand_script(
        &self,
        mac: &MacroDef,
        script: &ScriptMacro,
        args: Vec<Vec<Token>>,
        use_site: Span,
    ) -> FrontendResult<Vec<Token>> {
        let macro_args: Vec<MacroArg> = mac
            .params
            .iter()
            .zip(args.iter())
            .map(|(param, tokens)| MacroArg::new(param.clone(), raw_arg_text(tokens)))
            .collect();
        // Resource limits mirror the pinned reference constants (1000 ms macro
        // budget, 64 MiB memory, 512 KiB stack; see `opy_macro_js::Limits`).
        let runtime = MacroRuntime::new(Limits::default());
        let result = runtime
            .run_macro(&script.source, &macro_args, &script.path)
            .map_err(|error| map_macro_error(&error, &script.path, use_site))?;
        // Reference indentation rule (`resolveMacro`): every newline in the
        // replacement is followed by the call line's indentation.
        let indent = " ".repeat(use_site.start.col.saturating_sub(1) as usize);
        let indented = result.text.replace('\n', &format!("\n{indent}"));
        let mut tokens = lex(LexInput {
            file_id: use_site.file,
            text: &indented,
        })?;
        tokens.retain(|token| token.kind != TokenKind::Eof);
        for token in &mut tokens {
            token.span = use_site;
        }
        Ok(tokens)
    }

    /// Recursively expand macros inside an already-expanded run, guarding
    /// against direct recursion.
    fn expand_into(
        &self,
        tokens: &mut Vec<Token>,
        stack: &mut Vec<String>,
        depth: usize,
    ) -> FrontendResult<()> {
        if depth > 64 {
            return Err(FrontendError::new(
                "macro-recursion",
                "macro expansion exceeded the recursion limit (possible recursive define)",
            ));
        }
        let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            if token.kind == TokenKind::Ident {
                let name = token.text.clone();
                if let Some(mac) = self.macros.iter().find(|m| m.name == name) {
                    if stack.iter().any(|s| s == &name) {
                        return Err(FrontendError::new(
                            "macro-recursion",
                            format!("recursive macro expansion detected for '{name}'"),
                        ));
                    }
                    if mac.is_function {
                        if index + 1 < tokens.len() && tokens[index + 1].kind == TokenKind::LParen {
                            let (args, after) = self.collect_args(tokens, index)?;
                            let mut expanded = self.expand_macro(mac, args, token.span)?;
                            stack.push(name.clone());
                            self.expand_into(&mut expanded, stack, depth + 1)?;
                            stack.pop();
                            out.append(&mut expanded);
                            index = after;
                            continue;
                        }
                        out.push(token.clone());
                        index += 1;
                        continue;
                    }
                    let mut expanded = self.expand_macro(mac, Vec::new(), token.span)?;
                    stack.push(name.clone());
                    self.expand_into(&mut expanded, stack, depth + 1)?;
                    stack.pop();
                    out.append(&mut expanded);
                    index += 1;
                    continue;
                }
            }
            out.push(token.clone());
            index += 1;
        }
        *tokens = out;
        Ok(())
    }
}

fn split_directive(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(name, rest)| (name, rest))
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn parse_words(rest: &str, directive: &str, span: Span) -> FrontendResult<Vec<String>> {
    let words: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Err(FrontendError::at(
            "directive-invalid",
            format!("`#!{directive}` expects at least one argument"),
            span,
        ));
    }
    if words
        .iter()
        .any(|word| word.chars().any(|ch| !is_identifier_char(ch)))
    {
        return Err(FrontendError::at(
            "directive-invalid",
            format!("`#!{directive}` arguments must be identifiers"),
            span,
        ));
    }
    Ok(words)
}

fn parse_translations(rest: &str, span: Span) -> FrontendResult<Vec<String>> {
    let values: Vec<String> = rest
        .split_whitespace()
        .map(|language| language.replace('-', "_").to_lowercase())
        .collect();
    if values.is_empty() {
        return Err(FrontendError::at(
            "translations-invalid",
            "`#!translations` expects at least one language",
            span,
        ));
    }
    const PINNED_LANGUAGES: &[&str] = &[
        "de", "en", "es", "es_es", "es_mx", "fr", "it", "ja", "ko", "pl", "pt", "ru", "th", "tr",
        "zh", "zh_cn", "zh_tw",
    ];
    if values
        .iter()
        .any(|language| !PINNED_LANGUAGES.contains(&language.as_str()))
    {
        return Err(FrontendError::at(
            "translations-invalid",
            "invalid translation language; expected one of the pinned OverPy language codes",
            span,
        ));
    }
    if values.iter().any(|value| value == "es")
        && values
            .iter()
            .any(|value| value == "es_es" || value == "es_mx")
    {
        return Err(FrontendError::at(
            "translations-invalid",
            "cannot combine `es` with `es_es` or `es_mx`",
            span,
        ));
    }
    if values.iter().any(|value| value == "zh")
        && values
            .iter()
            .any(|value| value == "zh_cn" || value == "zh_tw")
    {
        return Err(FrontendError::at(
            "translations-invalid",
            "cannot combine `zh` with `zh_cn` or `zh_tw`",
            span,
        ));
    }
    Ok(values)
}

#[derive(Clone, Copy)]
enum OptimizationControl {
    Enable,
    Disable,
    ForSize,
    DisableForSize,
    ForSizeAggressive,
    Strict,
    DisableStrict,
}

fn optimization_directive(name: &str) -> Option<(&str, OptimizationControl)> {
    Some(match name {
        "disableOptimizations" => (name, OptimizationControl::Disable),
        "enableOptimizations" => (name, OptimizationControl::Enable),
        "optimizeForSize" => (name, OptimizationControl::ForSize),
        "disableOptimizeForSize" => (name, OptimizationControl::DisableForSize),
        "optimizeForSizeAggressive" => (name, OptimizationControl::ForSizeAggressive),
        "optimizeStrict" => (name, OptimizationControl::Strict),
        "disableOptimizeStrict" => (name, OptimizationControl::DisableStrict),
        _ => return None,
    })
}

fn apply_optimization(state: &mut OptimizationState, control: OptimizationControl) {
    match control {
        OptimizationControl::Enable => state.enabled = true,
        OptimizationControl::Disable => state.enabled = false,
        OptimizationControl::ForSize => state.for_size = true,
        OptimizationControl::DisableForSize => state.for_size = false,
        OptimizationControl::ForSizeAggressive => state.for_size_aggressive = true,
        OptimizationControl::Strict => state.strict = true,
        OptimizationControl::DisableStrict => state.strict = false,
    }
}

fn replacement_directive(name: &str) -> Option<&str> {
    Some(match name {
        "replace0ByCapturePercentage" => "getCapturePercentage",
        "replace0ByPayloadProgressPercentage" => "getPayloadProgressPercentage",
        "replace0ByIsMatchComplete" => "isMatchComplete",
        "replace1ByMatchRound" => "getMatchRound",
        "replaceTeam1ByControlScoringTeam" => "getControlScoringTeam",
        "replaceEmptyStringByEmptyArray" => "emptyArray",
        "replaceEmptyStringByVariable" => "variable",
        _ => return None,
    })
}

fn replacement_family(name: &str) -> Option<&str> {
    Some(match name {
        "replace0ByCapturePercentage"
        | "replace0ByPayloadProgressPercentage"
        | "replace0ByIsMatchComplete" => "0",
        "replace1ByMatchRound" => "1",
        "replaceTeam1ByControlScoringTeam" => "team1",
        "replaceEmptyStringByEmptyArray" | "replaceEmptyStringByVariable" => "emptyString",
        _ => return None,
    })
}

/// Strips a matched `"…"` or `'…'` pair, returning the inner text.
fn strip_quoted(text: &str) -> Option<&str> {
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
}

/// Reconstructs the raw call-site argument text from its tokens.
///
/// The reference injects the raw source substring as `var <name>=<raw>;`; the
/// token model stores string values unescaped, so string tokens are re-quoted
/// with JSON escaping. The reconstruction is JavaScript-value-equivalent to
/// the reference's raw injection: identifiers, numbers, operators, and
/// punctuation pass through verbatim, and string literals differ only in
/// quoting style, which is unobservable to the script.
fn raw_arg_text(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token.kind {
            TokenKind::String => out.push_str(&json_string_literal(&token.text)),
            TokenKind::Newline => out.push('\n'),
            _ => out.push_str(&token.text),
        }
    }
    out
}

/// Encodes `value` as a JSON string literal (double-quoted, escaped).
fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string is infallible")
}

/// Maps a runtime [`MacroError`] to a structured frontend diagnostic with the
/// script path as provenance and the directive/call-site span.
///
/// The runtime's QuickJS abort messages are classified into stable codes:
/// `script-timeout` (`"interrupted"`), `script-memory-limit`
/// (`"out of memory"`), `script-stack-limit`
/// (`"Maximum call stack size exceeded"`), and `script-error` for thrown
/// exceptions (with the script path and, when the engine provided one, the
/// line/column). Non-string completion values are `script-result-not-string`
/// with the reference's wording, and engine setup failures are
/// `script-internal`.
pub(crate) fn map_macro_error(error: &MacroError, script_path: &str, span: Span) -> FrontendError {
    match error {
        MacroError::Script(script) => {
            let code = match script.message.as_str() {
                "interrupted" => "script-timeout",
                "out of memory" => "script-memory-limit",
                "Maximum call stack size exceeded" => "script-stack-limit",
                _ => "script-error",
            };
            let location = match (script.line, script.column) {
                (Some(line), Some(column)) => format!(" (line {line}, column {column})"),
                (Some(line), None) => format!(" (line {line})"),
                _ => String::new(),
            };
            FrontendError::at(
                code,
                format!(
                    "script '{}' failed: {}{}",
                    script_path, script.message, location
                ),
                span,
            )
        }
        MacroError::InvalidResult { type_name } => FrontendError::at(
            "script-result-not-string",
            format!(
                "JavaScript macro returned value with type of {type_name}, expected string. Try using .toString()"
            ),
            span,
        ),
        MacroError::Internal(message) => FrontendError::at(
            "script-internal",
            format!("script '{}' runtime failure: {message}", script_path),
            span,
        ),
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
    fn settings_in_include_is_rejected() {
        let dir =
            std::env::temp_dir().join(format!("wright-opy-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("shared.opy"),
            "settings {\n    \"gamemodes\": {}\n}\n",
        )
        .unwrap();
        let main = "#!include \"shared.opy\"\nrule \"r\":\n    pass\n";
        let error = preprocess(main, "main.opy", &dir).unwrap_err();
        assert_eq!(error.code, "settings-placement");
        assert_eq!(
            error.span.unwrap().file,
            1,
            "the span names the included file"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
