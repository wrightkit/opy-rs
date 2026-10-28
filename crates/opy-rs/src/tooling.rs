//! Diagnostics contract: every [`Diagnostic`] carries a stable machine code,
//! a severity, a human message, and — when known — a resolved source location
//! (`path:line:col` through the file registry). Codes are the same ones the
//! compile pipeline emits (`lex-error`, `parse-error`, `workshop-source`,
//! `unknown-identifier`, `unknown-action`, `include-not-found`, …); see
//! `docs/opy/tooling-api.md` for the full table.
//!
//! Parse diagnostics are collected in full (the parser recovers at statement
//! boundaries); semantic-resolution diagnostics follow the compile contract
//! and report the first error, so `check` never disagrees with `compile`
//! about whether a project is clean.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod acceptance;

use serde::Serialize;

use crate::cst;
use crate::diag::{OpyError, Position, Span};
use crate::hir;
use crate::hir::types::{
    Declaration, Define, Expr as HirExpr, RuleEntry, SourceFile, Stmt as HirStmt,
};
use crate::preprocess::{FileRecord, PreprocessOutcome, PreprocessWarning, Preprocessed};

fn visible_warnings<'a>(
    preprocessed: &'a Preprocessed,
    parse_warnings: &'a [PreprocessWarning],
) -> impl Iterator<Item = &'a PreprocessWarning> {
    preprocessed
        .warnings
        .iter()
        .chain(parse_warnings)
        .filter(|warning| {
            !preprocessed
                .preprocessing
                .suppressed_warnings
                .iter()
                .any(|code| code == &warning.code)
        })
}

/// The outcome of [`check`]: structured diagnostics plus the resolved model.
///
/// `model` is present exactly when `diagnostics` contains no errors;
/// `files` is the frontend file registry (main file id 0, then one entry per
/// include) and is retained even on failure so diagnostics map to real
/// sources.
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub diagnostics: Vec<Diagnostic>,
    pub model: Option<SemanticModel>,
    pub files: Vec<FileRecord>,
    /// The declared `#!postCompileHook` script, when the source declared one
    /// and the project checked clean.
    ///
    /// This is the declaration record, not an execution result. `check` runs
    /// the hook through the bounded runtime as part of the compile-stage
    /// acceptance pass (issue #620), and a rewrite into a spelling the
    /// active locale rejects fails `check` the same way it fails `compile`;
    /// a hook that throws stays a `compile`-only failure.
    pub post_compile_hook: Option<crate::preprocess::PostCompileHook>,
    /// The directory the `files` display paths resolve against — the
    /// `#!mainFile` effective directory when the entry redirects the project
    /// root, otherwise the canonicalized input root.
    pub display_root: std::path::PathBuf,
    /// Filesystem paths the pipeline probed or read while producing this
    /// outcome, in the form each was accessed plus the resolved form when a
    /// probe canonicalized it.
    pub accessed: BTreeSet<PathBuf>,
}

impl CheckOutcome {
    fn failure(
        diagnostics: Vec<Diagnostic>,
        files: Vec<FileRecord>,
        display_root: std::path::PathBuf,
        accessed: BTreeSet<PathBuf>,
    ) -> Self {
        Self {
            diagnostics,
            model: None,
            files,
            post_compile_hook: None,
            display_root,
            accessed,
        }
    }

    pub fn is_clean(&self) -> bool {
        self.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != DiagnosticSeverity::Error)
    }
}

/// Check one `.opy` project: preprocess (includes/defines) → parse (CST) →
/// resolve (Opy HIR). `main_path` is the file's display path recorded in the file
/// registry; `root` is the include base.
///
/// Beyond the source pipeline, `check` validates the resolved settings block
/// against the canonical Workshop emission table (issue #411) and runs the
/// same compile-stage acceptance pass `compile` performs — lowering,
/// canonical validation, localized emission, the declared post-compile hook,
/// and re-validation of hook-rewritten output — under the active catalog
/// locale, because enum member spellings resolve under that locale (issue
/// #620). Whatever `check` accepts, `compile` does not reject for a
/// source-level reason: `check` surfaces the locale-dependent spelling
/// rejections the acceptance pass produces (`unknown-*`,
/// `missing-mapping`), while other integration-stage failures stay
/// `compile`-only diagnostics. The default locale is `en-US`;
/// [`check_with_locale`] selects another.
pub fn check(source: &str, main_path: &str, root: &Path) -> CheckOutcome {
    check_with_locale(
        source,
        main_path,
        root,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
}

/// [`check`] under a catalog locale name, without constructing a
/// `workshop-rs` `Locale` — the same locale `compile` emits under.
pub fn check_with_language(
    source: &str,
    main_path: &str,
    root: &Path,
    language: &str,
) -> CheckOutcome {
    check_with_locale(
        source,
        main_path,
        root,
        &workshop_rs::catalog::Locale::new(language),
    )
}

/// [`check`] under an explicit catalog locale: enum member spellings are
/// validated under `locale`, the same locale `compile` emits under.
pub fn check_with_locale(
    source: &str,
    main_path: &str,
    root: &Path,
    locale: &workshop_rs::catalog::Locale,
) -> CheckOutcome {
    check_with_overlay_and_locale(
        source,
        main_path,
        root,
        &std::collections::BTreeMap::new(),
        locale,
    )
}

/// [`check`] with open-document overlays (unsaved editor buffers participate
/// in include resolution, see [`crate::preprocess::preprocess_with_overlay`]).
pub fn check_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CheckOutcome {
    check_with_overlay_and_locale(
        source,
        main_path,
        root,
        overlay,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
}

/// [`check_with_overlay`] under an explicit catalog locale.
pub fn check_with_overlay_and_locale(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
    locale: &workshop_rs::catalog::Locale,
) -> CheckOutcome {
    check_pipeline(
        source,
        main_path,
        root,
        overlay,
        locale,
        CheckStage::CompileStageAcceptance,
    )
}

pub(crate) fn check_frontend_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
    locale: &workshop_rs::catalog::Locale,
) -> CheckOutcome {
    check_pipeline(
        source,
        main_path,
        root,
        overlay,
        locale,
        CheckStage::FrontendOnly,
    )
}

enum CheckStage {
    FrontendOnly,
    CompileStageAcceptance,
}

fn check_pipeline(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
    locale: &workshop_rs::catalog::Locale,
    stage: CheckStage,
) -> CheckOutcome {
    let PreprocessOutcome {
        result,
        files,
        display_root,
        warnings,
        accessed,
    } = crate::preprocess::preprocess_with_overlay_outcome(source, main_path, root, overlay);
    let preprocessed = match result {
        Ok((preprocessed, _)) => preprocessed,
        Err(error) => {
            let mut diagnostics = warnings
                .iter()
                .map(|warning| Diagnostic::from_warning(warning, &files))
                .collect::<Vec<_>>();
            diagnostics.push(Diagnostic::from_error(error, &files));
            return CheckOutcome::failure(diagnostics, files, display_root, accessed);
        }
    };
    let parsed = crate::parser::parse_with_macros(
        &preprocessed.tokens,
        preprocessed.preprocessing.allow_macro_redeclaration,
        &preprocessed.text_macros,
    );
    let mut diagnostics = visible_warnings(&preprocessed, &parsed.warnings)
        .map(|warning| Diagnostic::from_warning(warning, &files))
        .collect::<Vec<_>>();
    let Some(mut program) = parsed.program else {
        diagnostics.extend(
            parsed
                .errors
                .iter()
                .map(|error| Diagnostic::from_error(error.clone(), &files)),
        );
        return CheckOutcome::failure(diagnostics, files, display_root, accessed);
    };
    if let Some(block) = &preprocessed.settings {
        match crate::settings::parse_block(block) {
            Ok(parsed_settings) => program.settings = Some(parsed_settings),
            Err(error) => {
                diagnostics.push(Diagnostic::from_error(error, &files));
                return CheckOutcome::failure(diagnostics, files, display_root, accessed);
            }
        }
    }
    let defines = preprocessed
        .defines
        .iter()
        .map(|define| Define {
            name: define.name.clone(),
            is_function: define.is_function,
            is_member: define.is_member,
            span: define.span.map(Into::into),
        })
        .collect();
    let hir_files = files
        .iter()
        .map(|file| hir::types::SourceFile {
            id: file.id,
            path: file.path.clone(),
        })
        .collect();
    match crate::lower::lower_with_preprocessing(
        &program,
        hir_files,
        defines,
        &preprocessed.preprocessing,
    ) {
        Ok(mut hir) => {
            hir.preprocessing = preprocessed.preprocessing;
            if let Err(error) = crate::settings::resolve_hir_settings(&mut hir, &program) {
                diagnostics.push(Diagnostic::from_error(error, &files));
                return CheckOutcome::failure(diagnostics, files, display_root, accessed);
            }
            match crate::compiler::settings::workshop_settings(&hir, locale) {
                Ok((settings, unknown)) => {
                    diagnostics.extend(unknown.into_iter().map(|member| {
                        Diagnostic::from_unknown_setting(member, hir.settings.as_ref(), &files)
                    }));
                    if let Some(settings) = settings {
                        // workshop-rs also reports Warning entries for carried
                        // members near a declared spelling; those members are
                        // already covered by the `unknown-setting` warnings
                        // above, and emission accepts them, so only errors
                        // belong here.
                        diagnostics.extend(
                            workshop_rs::settings::check_emission_diagnostics(&settings)
                                .into_iter()
                                .filter(|diagnostic| {
                                    diagnostic.severity
                                        == workshop_rs::settings::DiagnosticSeverity::Error
                                })
                                .map(|diagnostic| {
                                    Diagnostic::from_settings_diagnostic(
                                        diagnostic,
                                        hir.settings.as_ref(),
                                        &files,
                                    )
                                }),
                        );
                    }
                }
                Err(error) => diagnostics.push(Diagnostic::from_integration_error(error, &files)),
            }
            if diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            {
                return CheckOutcome::failure(diagnostics, files, display_root, accessed);
            }
            if matches!(stage, CheckStage::CompileStageAcceptance) {
                let hook = preprocessed.post_compile_hook.as_ref().map(|hook| {
                    crate::PostCompileHookRecord {
                        script: hook.path.clone(),
                        source: hook.source.clone(),
                        span: Some(hook.span),
                    }
                });
                if let Some(error) =
                    acceptance::compile_stage_spelling_rejection(&hir, hook, locale)
                {
                    diagnostics.push(Diagnostic::from_integration_error(error, &files));
                    return CheckOutcome::failure(diagnostics, files, display_root, accessed);
                }
            }
            CheckOutcome {
                diagnostics,
                model: Some(SemanticModel::build(hir, &program)),
                files,
                post_compile_hook: preprocessed.post_compile_hook,
                display_root,
                accessed,
            }
        }
        Err(error) => {
            diagnostics.push(Diagnostic::from_error(error, &files));
            CheckOutcome::failure(diagnostics, files, display_root, accessed)
        }
    }
}

/// A structured, source-attributed diagnostic.
///
/// `code` is the stable machine contract (see the module docs and
/// `docs/opy/tooling-api.md`); `message` is human wording and not part of the
/// contract; `span` resolves through the file registry when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub span: Option<SourceLocation>,
}

impl Diagnostic {
    fn from_warning(warning: &PreprocessWarning, files: &[FileRecord]) -> Diagnostic {
        Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: warning.code.clone(),
            message: warning.message.clone(),
            span: resolve_record_span(warning.span, files),
        }
    }

    fn from_error(error: OpyError, files: &[FileRecord]) -> Diagnostic {
        Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: error.code,
            message: error.message,
            span: error.span.and_then(|span| resolve_record_span(span, files)),
        }
    }

    /// A canonical Workshop settings error surfaced under the same code the
    /// compile pipeline reports for emission failures. The message names the
    /// keys valid at the rejected member's path in a `did you mean` suffix
    /// (issue #469).
    fn from_settings_diagnostic(
        diagnostic: workshop_rs::settings::SettingsDiagnostic,
        hir_settings: Option<&crate::hir::types::Settings>,
        files: &[FileRecord],
    ) -> Diagnostic {
        let error = diagnostic.error;
        let span = crate::compiler::workshop_error_span(&error).map(|span| {
            Span::new(
                span.file.index() as u32,
                Position::new(span.start.line, span.start.col),
                Position::new(span.end.line, span.end.col),
            )
        });
        let candidates = crate::matcher::settings_member_candidates(
            hir_settings,
            &error,
            diagnostic.suggestion.as_deref(),
        );
        Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: "workshop-emission".to_string(),
            message: crate::matcher::did_you_mean(error.to_string(), &candidates),
            span: span.and_then(|span| resolve_record_span(span, files)),
        }
    }

    /// A settings key or enum value outside the catalog that compiles as
    /// written. It is accepted like upstream, but usually a misspelling.
    fn from_unknown_setting(
        member: crate::compiler::settings::UnknownSetting,
        hir_settings: Option<&crate::hir::types::Settings>,
        files: &[FileRecord],
    ) -> Diagnostic {
        let anchor = workshop_rs::WorkshopError::malformed(String::new(), member.span);
        let candidates = crate::matcher::settings_member_candidates(hir_settings, &anchor, None);
        let message = match &member.value {
            None => format!(
                "unknown settings key '{}' is passed through unchanged",
                member.name
            ),
            Some(value) => format!(
                "unknown value '{value}' for settings key '{}' is passed through unchanged",
                member.name
            ),
        };
        let span = member.span.map(|span| {
            Span::new(
                span.file.index() as u32,
                Position::new(span.start.line, span.start.col),
                Position::new(span.end.line, span.end.col),
            )
        });
        Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "unknown-setting".to_string(),
            message: crate::matcher::did_you_mean(message, &candidates),
            span: span.and_then(|span| resolve_record_span(span, files)),
        }
    }

    fn from_integration_error(
        error: crate::compiler::IntegrationError,
        files: &[FileRecord],
    ) -> Diagnostic {
        Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: error.diagnostic.code,
            message: error.diagnostic.message,
            span: error
                .diagnostic
                .span
                .map(to_frontend_span)
                .and_then(|span| resolve_record_span(span, files)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl DiagnosticSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub file_id: u32,
    pub path: String,
    pub start: Position,
    pub end: Position,
}

impl SourceLocation {
    pub fn to_span(&self) -> Span {
        Span::new(self.file_id, self.start, self.end)
    }
}

fn resolve_span(span: Span, files: &[SourceFile]) -> Option<SourceLocation> {
    let path = files.iter().find(|file| file.id == span.file)?.path.clone();
    Some(SourceLocation {
        file_id: span.file,
        path,
        start: span.start,
        end: span.end,
    })
}

fn resolve_record_span(span: Span, files: &[FileRecord]) -> Option<SourceLocation> {
    let path = files.iter().find(|file| file.id == span.file)?.path.clone();
    Some(SourceLocation {
        file_id: span.file,
        path,
        start: span.start,
        end: span.end,
    })
}

fn to_frontend_span(span: hir::types::Span) -> Span {
    Span::new(
        span.file,
        Position::new(span.start.line, span.start.col),
        Position::new(span.end.line, span.end.col),
    )
}

/// The resolved program model: the Opy HIR semantic program plus the
/// queryable symbol index and custom-enum declarations.
///
/// Custom enums are not retained in the Opy HIR (they fold to numeric
/// constants at use sites, reference behavior), so they are carried here from
/// the CST to keep declarations queryable.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticModel {
    pub hir: hir::Program,
    pub enums: Vec<EnumDecl>,
    pub symbols: Vec<Symbol>,
}

impl SemanticModel {
    /// Build the queryable model from a resolved HIR program and its parsed
    /// CST (required for custom-enum declarations).
    pub fn build(hir: hir::Program, cst: &cst::Program) -> SemanticModel {
        let enums = cst
            .declarations
            .iter()
            .filter_map(|decl| match decl {
                cst::Decl::Enum { name, members, .. } => Some(EnumDecl {
                    name: name.clone(),
                    members: members
                        .iter()
                        .map(|(member, _, span)| EnumMember {
                            name: member.clone(),
                            span: resolve_span(*span, &hir.files)
                                .expect("every token span resolves through the file registry"),
                        })
                        .collect(),
                }),
                _ => None,
            })
            .collect();
        let mut model = SemanticModel {
            hir,
            enums,
            symbols: Vec::new(),
        };
        model.index_symbols();
        model
    }

    pub fn declarations(&self) -> &[Declaration] {
        &self.hir.declarations
    }

    pub fn rules(&self) -> &[RuleEntry] {
        &self.hir.rules
    }

    pub fn defines(&self) -> &[Define] {
        &self.hir.defines
    }

    pub fn enums(&self) -> &[EnumDecl] {
        &self.enums
    }

    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// The first symbol bound under `name` (a `subroutine` declaration and a
    /// `def` definition of the same name index as separate symbols).
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }

    /// The symbol whose declaration site contains `span`, or — failing that —
    /// the symbol owning a reference site containing `span`.
    pub fn symbol_at(&self, span: Span) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| {
            span_contains(symbol.declaration.to_span(), span)
                || symbol
                    .references
                    .iter()
                    .any(|reference| span_contains(reference.to_span(), span))
        })
    }

    pub fn provenance(&self, span: Span) -> Option<SourceLocation> {
        resolve_span(span, &self.hir.files)
    }

    pub fn file(&self, id: u32) -> Option<&str> {
        self.hir
            .files
            .iter()
            .find(|file| file.id == id)
            .map(|file| file.path.as_str())
    }

    fn index_symbols(&mut self) {
        for decl in &self.hir.declarations {
            let (kind, name, span) = match decl {
                Declaration::GlobalVariable {
                    name,
                    name_span,
                    span,
                    ..
                } => (SymbolKind::Global, name, name_span.or(*span)),
                Declaration::PlayerVariable {
                    name,
                    name_span,
                    span,
                    ..
                } => (SymbolKind::Player, name, name_span.or(*span)),
                Declaration::Subroutine {
                    name,
                    name_span,
                    span,
                    ..
                } => (SymbolKind::Subroutine, name, name_span.or(*span)),
                Declaration::Constant { name, span, .. } => (SymbolKind::Constant, name, *span),
                Declaration::Macro { name, span, .. } => (SymbolKind::Macro, name, *span),
            };
            let Some(span) = span.map(to_frontend_span) else {
                // Foreign payloads may omit spans; such declarations are not
                // addressable and stay out of the index.
                continue;
            };
            let Some(declaration) = resolve_span(span, &self.hir.files) else {
                continue;
            };
            self.symbols.push(Symbol {
                name: name.clone(),
                kind,
                declaration,
                references: Vec::new(),
            });
        }
        for entry in &self.hir.rules {
            let RuleEntry::SubroutineDef {
                name,
                source_name,
                name_span,
                span,
                ..
            } = entry
            else {
                continue;
            };
            let Some(span) = name_span.or(*span).map(to_frontend_span) else {
                continue;
            };
            let Some(declaration) = resolve_span(span, &self.hir.files) else {
                continue;
            };
            self.symbols.push(Symbol {
                name: if source_name.is_empty() {
                    name.clone()
                } else {
                    source_name.clone()
                },
                kind: SymbolKind::Def,
                declaration,
                references: Vec::new(),
            });
        }

        let mut collector = ReferenceSiteCollector { sites: Vec::new() };
        hir::visit::walk_program(&mut collector, &self.hir);
        for (kind, name, span) in collector.sites {
            self.attach_reference(kind, &name, span);
        }
    }

    fn attach_reference(&mut self, kind: SymbolKind, name: &str, span: Span) {
        let Some(location) = resolve_span(span, &self.hir.files) else {
            return;
        };
        if let Some(index) = self
            .symbols
            .iter()
            .position(|symbol| symbol.kind == kind && symbol.name == name)
        {
            self.symbols[index].references.push(location);
        }
    }
}

struct ReferenceSiteCollector {
    sites: Vec<(SymbolKind, String, Span)>,
}

impl hir::visit::Visitor for ReferenceSiteCollector {
    fn visit_expr(&mut self, expression: &HirExpr) {
        match expression {
            HirExpr::GlobalVar {
                name,
                span: Some(span),
            } => {
                self.sites
                    .push((SymbolKind::Global, name.clone(), to_frontend_span(*span)));
            }
            HirExpr::Constant {
                name,
                span: Some(span),
            } => {
                self.sites
                    .push((SymbolKind::Constant, name.clone(), to_frontend_span(*span)));
            }
            HirExpr::PlayerVar {
                name,
                member_span,
                span,
                ..
            } => {
                if let Some(span) = member_span.as_ref().or(span.as_ref()) {
                    self.sites
                        .push((SymbolKind::Player, name.clone(), to_frontend_span(*span)));
                }
            }
            HirExpr::Call {
                name,
                span: Some(span),
                ..
            } => {
                let span = to_frontend_span(*span);
                self.sites
                    .push((SymbolKind::Subroutine, name.clone(), span));
                self.sites.push((SymbolKind::Def, name.clone(), span));
            }
            HirExpr::MacroCall {
                name,
                span: Some(span),
                ..
            } => {
                self.sites
                    .push((SymbolKind::Macro, name.clone(), to_frontend_span(*span)));
            }
            _ => {}
        }
        hir::visit::walk_expr(self, expression);
    }

    fn visit_comprehension(
        &mut self,
        element: &HirExpr,
        iterable: &HirExpr,
        condition: Option<&HirExpr>,
    ) {
        hir::visit::Visitor::visit_expr(self, iterable);
        hir::visit::Visitor::visit_expr(self, element);
        if let Some(condition) = condition {
            hir::visit::Visitor::visit_expr(self, condition);
        }
    }

    fn visit_stmt(&mut self, statement: &HirStmt) {
        if let HirStmt::CallSubroutine {
            name,
            span: Some(span),
        } = statement
        {
            let span = to_frontend_span(*span);
            self.sites
                .push((SymbolKind::Subroutine, name.clone(), span));
            self.sites.push((SymbolKind::Def, name.clone(), span));
        }
        hir::visit::walk_stmt(self, statement);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumDecl {
    pub name: String,
    pub members: Vec<EnumMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumMember {
    pub name: String,
    pub span: SourceLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SymbolKind {
    Global,
    Player,
    Subroutine,
    Def,
    Constant,
    Macro,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub declaration: SourceLocation,
    pub references: Vec<SourceLocation>,
}

fn span_contains(outer: Span, inner: Span) -> bool {
    position_leq(outer.start, inner.start) && position_leq(inner.end, outer.end)
}

fn position_leq(a: Position, b: Position) -> bool {
    a.line < b.line || (a.line == b.line && a.col <= b.col)
}

#[cfg(test)]
mod tests;
