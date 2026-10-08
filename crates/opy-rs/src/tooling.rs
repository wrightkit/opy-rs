//! Workshop-independent tooling APIs: check a project and query the resolved
//! semantic model.
//!
//! This module is the public tooling surface for Wright and other consumers
//! that want to parse, check, inspect, and reason about OPY projects before
//! any Workshop backend is connected (issue #7):
//!
//! * [`check`] / [`check_with_overlay`] run the full frontend pipeline
//!   (preprocess → parse → resolve) on a main file plus its includes and
//!   return every structured diagnostic together with the file registry,
//!   without requiring lowering to any Workshop backend. Resolution stops at
//!   the Opy HIR semantic model ([`hir::Program`]); Workshop emission,
//!   decompilation, and catalog behavior are deliberately out of scope here.
//!   The one Workshop-bound input, the settings block, is additionally checked
//!   against the canonical emission table so `check` never accepts a settings
//!   key `compile` would reject (issue #411).
//! * [`SemanticModel`] wraps the resolved program and answers semantic
//!   queries: declarations, rule listing, symbol/reference lookup by name or
//!   span, custom-enum declarations, macro defines, and source provenance
//!   (span → file id, path, line/col).
//!
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

use std::path::Path;

use serde::Serialize;

use crate::cst;
use crate::diag::{OpyError, Position, Span};
use crate::hir;
use crate::hir::types::{
    Declaration, Define, Expr as HirExpr, RuleEntry, SourceFile, Stmt as HirStmt,
};
use crate::preprocess::{FileRecord, PreprocessOutcome, PreprocessWarning, Preprocessed};

fn visible_warnings(preprocessed: &Preprocessed) -> impl Iterator<Item = &PreprocessWarning> {
    preprocessed.warnings.iter().filter(|warning| {
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
    /// This is the declaration record, not an execution result: the frontend
    /// recognizes, parses, validates, and records the directive, but never
    /// executes the hook. Execution against the final Workshop text is
    /// lowering-dependent (workshop-rs emission, issue #8); the frontend
    /// never fabricates a Workshop payload.
    pub post_compile_hook: Option<crate::preprocess::PostCompileHook>,
    /// The directory the `files` display paths resolve against — the
    /// `#!mainFile` effective directory when the entry redirects the project
    /// root, otherwise the canonicalized input root.
    pub display_root: std::path::PathBuf,
}

impl CheckOutcome {
    fn failure(
        diagnostics: Vec<Diagnostic>,
        files: Vec<FileRecord>,
        display_root: std::path::PathBuf,
    ) -> Self {
        Self {
            diagnostics,
            model: None,
            files,
            post_compile_hook: None,
            display_root,
        }
    }

    /// Whether the project checked clean.
    pub fn is_clean(&self) -> bool {
        self.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != DiagnosticSeverity::Error)
    }
}

/// Check one `.opy` project: preprocess (includes/defines) → parse (CST) →
/// resolve (Opy HIR). `main_path` is the display path recorded in the file
/// registry; `root` is the include base. No Workshop backend is required;
/// settings blocks are validated against the canonical emission table so the
/// `check` verdict agrees with `compile` on settings keys (issue #411).
pub fn check(source: &str, main_path: &str, root: &Path) -> CheckOutcome {
    check_with_overlay(source, main_path, root, &std::collections::BTreeMap::new())
}

/// [`check`] with open-document overlays (unsaved editor buffers participate
/// in include resolution, see [`crate::preprocess::preprocess_with_overlay`]).
pub fn check_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CheckOutcome {
    let PreprocessOutcome {
        result,
        files,
        display_root,
        warnings,
    } = crate::preprocess::preprocess_with_overlay_outcome(source, main_path, root, overlay);
    let preprocessed = match result {
        Ok((preprocessed, _)) => preprocessed,
        Err(error) => {
            let mut diagnostics = warnings
                .iter()
                .map(|warning| Diagnostic::from_warning(warning, &files))
                .collect::<Vec<_>>();
            diagnostics.push(Diagnostic::from_error(error, &files));
            return CheckOutcome::failure(diagnostics, files, display_root);
        }
    };
    let parsed = crate::parser::parse_with_options(
        &preprocessed.tokens,
        preprocessed.preprocessing.allow_macro_redeclaration,
    );
    let Some(mut program) = parsed.program else {
        // The parser recovers at statement boundaries; every collected error
        // is reported (the compile pipeline reads only the first).
        let mut diagnostics = visible_warnings(&preprocessed)
            .map(|warning| Diagnostic::from_warning(warning, &files))
            .collect::<Vec<_>>();
        diagnostics.extend(
            parsed
                .errors
                .iter()
                .map(|error| Diagnostic::from_error(error.clone(), &files)),
        );
        return CheckOutcome::failure(diagnostics, files, display_root);
    };
    // Parse the extracted settings block into the CST; expression values are
    // resolved after ordinary CST-to-HIR lowering so they use the shared OPY
    // semantic path (#86, #188).
    if let Some(block) = &preprocessed.settings {
        match crate::settings::parse_block(block) {
            Ok(parsed_settings) => program.settings = Some(parsed_settings),
            Err(error) => {
                let mut diagnostics = visible_warnings(&preprocessed)
                    .map(|warning| Diagnostic::from_warning(warning, &files))
                    .collect::<Vec<_>>();
                diagnostics.push(Diagnostic::from_error(error, &files));
                return CheckOutcome::failure(diagnostics, files, display_root);
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
            let mut diagnostics = visible_warnings(&preprocessed)
                .map(|warning| Diagnostic::from_warning(warning, &files))
                .collect::<Vec<_>>();
            hir.preprocessing = preprocessed.preprocessing;
            if let Err(error) = crate::settings::resolve_hir_settings(&mut hir, &program) {
                diagnostics.push(Diagnostic::from_error(error, &files));
                return CheckOutcome::failure(diagnostics, files, display_root);
            }
            // Settings are Workshop-bound data: convert them through the same
            // path the compiler uses and report every member the canonical
            // emission table cannot emit, so `check` never accepts a settings
            // key that `compile` would reject (#411).
            match crate::compiler::settings::workshop_settings(&hir) {
                Ok((settings, verbatim)) => {
                    diagnostics.extend(verbatim.into_iter().map(|member| {
                        Diagnostic::from_verbatim_setting(member, hir.settings.as_ref(), &files)
                    }));
                    if let Some(settings) = settings {
                        diagnostics.extend(
                            workshop_rs::settings::check_emission_diagnostics(&settings)
                                .into_iter()
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
                return CheckOutcome::failure(diagnostics, files, display_root);
            }
            CheckOutcome {
                diagnostics,
                model: Some(SemanticModel::build(hir, &program)),
                files,
                // The directive was parsed, validated, and recorded by
                // preprocessing; the frontend never executes the hook (real hook
                // execution receives the final Workshop text and is
                // lowering-dependent, issue #8).
                post_compile_hook: preprocessed.post_compile_hook,
                display_root,
            }
        }
        Err(error) => {
            let mut diagnostics = visible_warnings(&preprocessed)
                .map(|warning| Diagnostic::from_warning(warning, &files))
                .collect::<Vec<_>>();
            diagnostics.push(Diagnostic::from_error(error, &files));
            CheckOutcome::failure(diagnostics, files, display_root)
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

    /// A settings key outside the catalog that compiles to `key: value` as
    /// written. It is accepted like upstream, but usually a misspelling.
    fn from_verbatim_setting(
        member: crate::compiler::settings::VerbatimMember,
        hir_settings: Option<&crate::hir::types::Settings>,
        files: &[FileRecord],
    ) -> Diagnostic {
        let anchor = workshop_rs::WorkshopError::malformed(String::new(), member.span);
        let candidates = crate::matcher::settings_member_candidates(hir_settings, &anchor, None);
        let message = format!(
            "settings key '{}' is not in the Workshop settings catalog and is emitted verbatim",
            member.name
        );
        let span = member.span.map(|span| {
            Span::new(
                span.file.index() as u32,
                Position::new(span.start.line, span.start.col),
                Position::new(span.end.line, span.end.col),
            )
        });
        Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "settings-verbatim".to_string(),
            message: crate::matcher::did_you_mean(message, &candidates),
            span: span.and_then(|span| resolve_record_span(span, files)),
        }
    }

    /// A settings-conversion failure at the Workshop integration boundary.
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

/// The severity of a diagnostic in the frontend contract.
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

/// A resolved source location: a span's file id and path (through the file
/// registry) plus its 1-based line/column interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub file_id: u32,
    pub path: String,
    pub start: Position,
    pub end: Position,
}

impl SourceLocation {
    /// Recover the frontend span (file id + positions) of this location.
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

/// Resolve a span through the preprocess file registry (used for
/// diagnostics, where the model may not exist).
fn resolve_record_span(span: Span, files: &[FileRecord]) -> Option<SourceLocation> {
    let path = files.iter().find(|file| file.id == span.file)?.path.clone();
    Some(SourceLocation {
        file_id: span.file,
        path,
        start: span.start,
        end: span.end,
    })
}

/// Map an Opy HIR span (the protocol type) back to the frontend span type;
/// positions are the same 1-based source coordinates carried through
/// lowering.
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
                        .map(|(member, span)| EnumMember {
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

    /// The HIR declarations (globals, players, subroutines, constants,
    /// macros). Custom enums are queried through [`SemanticModel::enums`].
    pub fn declarations(&self) -> &[Declaration] {
        &self.hir.declarations
    }

    /// The rule listing: rules and subroutine definitions.
    pub fn rules(&self) -> &[RuleEntry] {
        &self.hir.rules
    }

    /// The recorded preprocessing defines (macro-expansion provenance).
    pub fn defines(&self) -> &[Define] {
        &self.hir.defines
    }

    /// The custom-enum declarations of the project.
    pub fn enums(&self) -> &[EnumDecl] {
        &self.enums
    }

    /// Every indexed program-scope symbol with its declaration site and
    /// reference sites.
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

    /// Resolve a span to its file id, path, and line/column through the file
    /// registry.
    pub fn provenance(&self, span: Span) -> Option<SourceLocation> {
        resolve_span(span, &self.hir.files)
    }

    /// The registry path of a file id.
    pub fn file(&self, id: u32) -> Option<&str> {
        self.hir
            .files
            .iter()
            .find(|file| file.id == id)
            .map(|file| file.path.as_str())
    }

    /// Index every program-scope binding, then attach resolved reference
    /// sites by name/kind.
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

    /// Record a reference site for the first symbol of `kind` named `name`.
    /// A call site is offered to both `subroutine` and `def` bindings.
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

/// A custom `enum` declaration (CST-retained; enums fold to constants in the
/// HIR).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumDecl {
    pub name: String,
    pub members: Vec<EnumMember>,
}

/// One custom-enum member with its declaration site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumMember {
    pub name: String,
    pub span: SourceLocation,
}

/// The kind of a program-scope symbol.
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

/// A program-scope symbol with its declaration site and resolved reference
/// sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub declaration: SourceLocation,
    pub references: Vec<SourceLocation>,
}

/// Whether the interval `outer` contains the interval `inner` (half-open
/// end positions, so a 1:1 zero-width span is contained by itself).
fn span_contains(outer: Span, inner: Span) -> bool {
    position_leq(outer.start, inner.start) && position_leq(inner.end, outer.end)
}

fn position_leq(a: Position, b: Position) -> bool {
    a.line < b.line || (a.line == b.line && a.col <= b.col)
}

#[cfg(test)]
mod tests {
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
        let outcome =
            check_source("globalvar total\nrule \"r\":\n    @Event global\n    total = 1\n");
        let model = outcome.model.expect("clean project");
        let total = model.symbol("total").expect("symbol by name");
        assert_eq!(total.kind, SymbolKind::Global);
        // The declaration site answers span lookup…
        let at_decl = model
            .symbol_at(total.declaration.to_span())
            .expect("symbol at declaration span");
        assert_eq!(at_decl.name, "total");
        // …and so does a reference site.
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
        let outcome =
            check_source("globalvar total\nrule \"r\":\n    @Event global\n    total = 1\n");
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
    fn verbatim_setting_names_the_keys_valid_at_the_path() {
        // `notASetting` sits under `gamemodes.ffa`; nothing is near, so the
        // warning falls back to the bounded list of keys the path accepts
        // (issue #469).
        let outcome = check_source(
            "settings {\n    \"gamemodes\": {\"ffa\": {\"notASetting\": true}}\n}\nrule \"a\":\n    @Event global\n    wait(1)\n",
        );
        let diagnostic = outcome
            .diagnostics
            .iter()
            .find(|d| d.code == "settings-verbatim")
            .expect("verbatim settings diagnostic");
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
    fn verbatim_setting_preserves_template_paths_and_percent_suffixes() {
        let outcome = check_source(
            "settings {\n    \"main\": {\"description\": \"t\"},\n    \"gamemodes\": {},\n    \"heroes\": {\"team1\": {\"general\": {\"damageReceiveed%\": 50}}}\n}\nrule \"a\":\n    @Event global\n    wait(1)\n",
        );
        let diagnostic = outcome
            .diagnostics
            .iter()
            .find(|d| d.code == "settings-verbatim")
            .expect("verbatim settings diagnostic");
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
        assert!(
            diagnostic.message.contains("'damageReceived%'"),
            "message: {}",
            diagnostic.message
        );
    }

    #[test]
    fn check_reports_every_parse_error() {
        // The parser recovers at statement boundaries; check collects all
        // parse diagnostics (compile reads only the first). The two rules
        // missing their colon and the stray directive line yield three
        // parse-error diagnostics.
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
        // #410: canonical Workshop ids that upstream OverPy does not spell
        // are rejected with a source span, not accepted as callables.
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

        // The upstream spellings stay clean.
        let outcome = check_source(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    g = getTankHeroes()\n    g = getLastCreatedEntity()\n    g = evalOnce(1)\n    g = eventPlayer.isHoldingButton(Button.PRIMARY_FIRE)\n    destroyAllHudTexts()\n",
        );
        assert!(
            outcome.is_clean(),
            "unexpected diagnostics: {:?}",
            outcome.diagnostics
        );
    }
}
